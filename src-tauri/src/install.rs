//! Wan2GP install / reinstall / uninstall / kernel sync / core update.
//! Hardened installer: target-folder triage (classify_target), exact-pinned
//! Python preflight via uv (ensure_uv_python, port of Electron installPython),
//! and setup.py exit-code propagation (no more false "Installation complete!").
use tauri::Emitter;
use std::path::{Path, PathBuf};
use crate::base::*;
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;
use crate::{hw::{build_install_plan, get_gpu_info_sync, kernel_profile_key}, status::get_active_env};

/// Pull the first X.Y[.Z] out of a version string ("3.11.14", "3.11", ">=3.11").
fn scan_version(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let mut j = i;
            let mut dots = 0;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || (bytes[j] == b'.' && dots < 2)) {
                if bytes[j] == b'.' { dots += 1; }
                j += 1;
            }
            let cand = s[i..j].trim_matches('.');
            if cand.contains('.') && cand.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return cand.to_string();
            }
            i = j;
        } else { i += 1; }
    }
    String::new()
}

/// Exact Python pin setup.py will request via `uv venv --python X`.
/// Read from the freshly-cloned setup_config.json when present (per-profile
/// `python`, else common global keys) so an upstream pin bump can't silently
/// re-open the ATFGriff hole; falls back to the README matrix (GTX 10xx →
/// 3.10.9, everything else 3.11.14). A minor-only value ("3.11") maps to the
/// known-good patch — requesting the *minor* is what caused the original
/// failure (uv provisions 3.11.x while setup.py demands the exact patch).
pub(crate) fn pinned_python_wanted() -> String {
    let repo = get_repo_dir();
    if let Ok(s) = std::fs::read_to_string(repo.join("setup_config.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
            let gpu = get_gpu_info_sync();
            let profile = kernel_profile_key(
                gpu.get("vendor").and_then(|v| v.as_str()).unwrap_or(""),
                gpu.get("name").and_then(|v| v.as_str()).unwrap_or(""));
            let mut cands: Vec<&serde_json::Value> = Vec::new();
            if let Some(p) = v.get("gpu_profiles").and_then(|g| g.get(&profile)).and_then(|p| p.get("python")) { cands.push(p); }
            for key in ["python", "python_version"] {
                if let Some(p) = v.get(key) { cands.push(p); }
            }
            if let Some(p) = v.get("components").and_then(|c| c.get("python")) { cands.push(p); }
            for c in cands {
                let ver = scan_version(c.as_str().unwrap_or(""));
                if ver.is_empty() { continue; }
                if ver.chars().filter(|c| *c == '.').count() >= 2 { return ver; }
                // minor-only (upstream uses "3.11"): resolve the exact patch
                // from components.python.<minor>.ver, fallback to known-good.
                if ver.starts_with("3.") {
                    if let Some(exact) = v.get("components").and_then(|c| c.get("python"))
                        .and_then(|p| p.get(ver.as_str())).and_then(|e| e.get("ver"))
                        .and_then(|x| x.as_str())
                    {
                        let full = scan_version(exact);
                        if full.chars().filter(|c| *c == '.').count() >= 2 { return full; }
                    }
                    if ver.starts_with("3.11") { return "3.11.14".into(); }
                    if ver.starts_with("3.10") { return "3.10.9".into(); }
                }
                return ver;
            }
        }
    }
    let gpu = get_gpu_info_sync();
    let vendor = gpu.get("vendor").and_then(|v| v.as_str()).unwrap_or("");
    let name = gpu.get("name").and_then(|v| v.as_str()).unwrap_or("");
    if kernel_profile_key(vendor, name) == "GTX_10" { "3.10.9".into() } else { "3.11.14".into() }
}

/// Run a uv subcommand, stream its output to the console, return (exit_ok, output).
async fn uv_capture(app: &tauri::AppHandle, emit: impl Fn(&str) + Send + Sync, args: &[&str]) -> (bool, String) {
    let (mut rx, _child) = match app.shell().command("uv").args(args).spawn() {
        Ok(t) => t,
        Err(e) => return (false, e.to_string()),
    };
    let mut out = String::new();
    let mut code: Option<i32> = None;
    while let Some(ev) = rx.recv().await {
        match ev {
            CommandEvent::Stdout(b) | CommandEvent::Stderr(b) => {
                let s = String::from_utf8_lossy(&b).to_string();
                out.push_str(&s); emit(&s);
            }
            CommandEvent::Terminated(p) => { code = p.code; }
            CommandEvent::Error(e) => { out.push_str(&e); }
            _ => {}
        }
    }
    (code.unwrap_or(-1) == 0, out)
}

/// Port of Electron installPython(): make sure `uv` can hand setup.py the exact
/// pinned interpreter *before* the 20-minute install starts. Order matters:
/// find FIRST (a manually installed exact Python counts — setup.py's
/// `uv venv --python X` reuses discovered interpreters), provision second.
/// Aborting on a failed download while a usable copy sits on disk was
/// exactly the "installed 3.11 manually but still not found" complaint.
async fn ensure_uv_python(app: &tauri::AppHandle, emit: impl Fn(&str) + Send + Sync, wanted: &str) -> Result<String, String> {
    // Never let a user/system config with `python-downloads = "never"` silently
    // break provisioning — spawned processes inherit our env.
    std::env::set_var("UV_PYTHON_DOWNLOADS", "automatic");
    emit(&format!("[*] Ensuring Python {wanted} via uv (setup.py needs this exact version)…\n"));
    // Best-effort self-update first: an old uv doesn't know new patches exist
    // (3.11.14) and fails with the same "No interpreter found" error.
    // Harmless when offline or already current — failures are ignored.
    let _ = uv_capture(app, &emit, &["self", "update"]).await;
    // Resolve + verify the EXACT pin actually executes (a neighbouring patch
    // or a corrupted copy won't satisfy setup.py — report it, don't use it).
    let verify_exact = |p: &str| -> Option<String> {
        silent_command(p).arg("-c").arg("import sys; print(sys.version)").output().ok()
            .and_then(|o| if o.status.success() { Some(String::from_utf8_lossy(&o.stdout).trim().split_whitespace().next().unwrap_or("").to_string()) } else { None })
            .filter(|v| v.starts_with(wanted))
    };
    let find = || silent_command("uv").args(["python", "find", wanted]).output().ok()
        .and_then(|o| if o.status.success() { Some(String::from_utf8_lossy(&o.stdout).trim().to_string()) } else { None })
        .filter(|s| !s.is_empty());
    // 1) Fast path: already provisioned or uv-discoverable (managed OR a
    // manually installed exact copy on PATH/registry/launcher).
    if let Some(p) = find() {
        if let Some(v) = verify_exact(&p) {
            emit(&format!("[*] Python {wanted} ready: {p} ({v})\n"));
            return Ok(p);
        }
        emit(&format!("[!] Found Python at {p} but it won't run — forcing a clean reinstall…\n"));
    }
    // 2) Provision via uv.
    let (dl_ok, _) = uv_capture(app, &emit, &["python", "install", wanted]).await;
    if dl_ok {
        if let Some(p) = find() {
            if let Some(v) = verify_exact(&p) {
                emit(&format!("[*] Python {wanted} ready: {p} ({v})\n"));
                return Ok(p);
            }
        }
    }
    // 3) Download failed but a usable copy may still exist (uv venv reuses
    // discovered interpreters, so setup.py can proceed without any download).
    if !dl_ok {
        emit(&format!("[!] uv could not download Python {wanted} — checking for a manually installed copy…\n"));
        if let Some(p) = find() {
            if let Some(v) = verify_exact(&p) {
                emit(&format!("[*] Using existing Python {wanted}: {p} ({v}) — setup.py will reuse it.\n"));
                return Ok(p);
            }
        }
    }
    // 4) Force reinstall of a corrupted managed copy, then give up with
    // diagnostics (list what IS installed so the fix is obvious).
    if dl_ok {
        emit(&format!("[!] Managed Python {wanted} is broken (found but won't run). Forcing a clean reinstall…\n"));
        let (re_ok, _) = uv_capture(app, &emit, &["python", "install", "--reinstall", wanted]).await;
        if !re_ok {
            // Older uv without --reinstall: uninstall + install.
            let _ = uv_capture(app, &emit, &["python", "uninstall", wanted]).await;
            let (ok2, _) = uv_capture(app, &emit, &["python", "install", wanted]).await;
            if !ok2 { return Err(diagnose_python_fail(wanted)); }
        }
        if let Some(p) = find() {
            if let Some(v) = verify_exact(&p) {
                emit(&format!("[*] Python {wanted} reinstalled: {p} ({v})\n"));
                return Ok(p);
            }
        }
    }
    Err(diagnose_python_fail(wanted))
}

/// Final failure message with discovery diagnostics: enumerate system Pythons
/// so "I installed 3.11 manually" turns into "found 3.11.9 at P — need
/// exactly 3.11.14" instead of a bare "not found".
fn diagnose_python_fail(wanted: &str) -> String {
    fn ver_of(prog: &str, args: &[&str]) -> Option<String> {
        silent_command(prog).args(args).arg("-c").arg("import sys; print(sys.version)").output().ok()
            .and_then(|o| if o.status.success() { Some(String::from_utf8_lossy(&o.stdout).trim().split_whitespace().next().unwrap_or("").to_string()) } else { None })
            .filter(|v| !v.is_empty())
    }
    let mut cands: Vec<String> = Vec::new();
    // PATH (skip the Microsoft Store shim — it opens the Store, not Python).
    if let Ok(o) = silent_command("where").arg("python").output() {
        if o.status.success() {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let p = line.trim();
                if !p.is_empty() && !p.to_lowercase().contains("windowsapps") { cands.push(p.to_string()); }
            }
        }
    }
    #[cfg(windows)]
    for fixed in [
        std::env::var("LOCALAPPDATA").ok().map(|a| format!("{a}\\Programs\\Python\\Python311\\python.exe")),
        Some("C:\\Python311\\python.exe".into()),
        Some("C:\\Program Files\\Python311\\python.exe".into()),
    ].into_iter().flatten() {
        if !cands.iter().any(|c| c.eq_ignore_ascii_case(&fixed)) { cands.push(fixed); }
    }
    #[cfg(not(windows))]
    for fixed in ["/usr/bin/python3.11", "/usr/local/bin/python3.11"] {
        if !cands.contains(&fixed.to_string()) { cands.push(fixed.into()); }
    }
    let mut lines: Vec<String> = Vec::new();
    for c in &cands {
        match ver_of(c, &[]) {
            Some(v) => lines.push(format!("  found Python {v} at {c}")),
            None => lines.push(format!("  {c} (not runnable)")),
        }
    }
    if let Some(v) = ver_of("py", &["-3.11"]) {
        lines.push(format!("  found Python {v} via py launcher (-3.11)"));
    }
    let found_txt = if lines.is_empty() { "  (no Python 3.11 found on PATH, in registry spots, or via py launcher)".into() } else { lines.join("\n") };
    format!("uv could not provision Python {wanted}, and no usable copy was found.\n\
        What we found:\n{found_txt}\n\
        setup.py needs EXACTLY {wanted} (a neighbouring patch like 3.11.9 does not count).\n\
        Fix options:\n\
        1. Let uv fetch it: `uv self update`, then `uv python install {wanted}` — needs network to python-build-standalone (check proxy/VPN/antivirus; uv downloads live under %APPDATA%\\uv).\n\
        2. Install exactly Python {wanted} from https://www.python.org/downloads/ — tick \u{201c}Add python.exe to PATH\u{201d} during setup, then retry.\n\
        3. Already installed 3.11 manually? It must be exactly {wanted} (see versions above), reachable via PATH or `py -3.11`, and NOT the Microsoft Store stub (Settings → Apps → Advanced app settings → App execution aliases → turn Python off), then retry.")
}

/// Check-only preflight for the installer checklist UI (no downloads).
/// Covers BOTH drives of a split install: the target drive (J: — venv,
/// wheels, models) and uv's own data drive (C: — managed Pythons live in
/// %APPDATA%\uv\python, as ATFGriff's log shows). A full C: fails the
/// Python download even when J: has terabytes free.
#[tauri::command]
pub fn python_preflight() -> serde_json::Value {
    let wanted = pinned_python_wanted();
    let uv_ver = silent_command("uv").arg("--version").output().ok()
        .and_then(|o| if o.status.success() { Some(String::from_utf8_lossy(&o.stdout).trim().to_string()) } else { None });
    let path = silent_command("uv").args(["python", "find", &wanted]).output().ok()
        .and_then(|o| if o.status.success() { Some(String::from_utf8_lossy(&o.stdout).trim().to_string()) } else { None })
        .filter(|s| !s.is_empty());
    // Version-exact: a neighbouring patch (or a dead exe) must read as NOT ok.
    let runs = path.as_ref().is_some_and(|p| silent_command(p).arg("-c").arg("import sys; print(sys.version)").output().ok()
        .and_then(|o| if o.status.success() { Some(String::from_utf8_lossy(&o.stdout).trim().split_whitespace().next().unwrap_or("").to_string()) } else { None })
        .is_some_and(|v| v.starts_with(&wanted)));
    let downloads_blocked = std::env::var("UV_PYTHON_DOWNLOADS").is_ok_and(|v| v.eq_ignore_ascii_case("never"));
    // Where would `uv python install` put the interpreter? Explicit override,
    // else uv's default data dir (Windows: %APPDATA%\uv).
    let uv_data_dir = std::env::var("UV_PYTHON_INSTALL_DIR").ok().filter(|s| !s.is_empty()).or_else(|| {
        std::env::var("APPDATA").ok().map(|a| format!("{a}\\uv"))
    }).unwrap_or_default();
    let uv_data_free_gb = if uv_data_dir.is_empty() { None } else {
        crate::config::disk_for_path(&uv_data_dir).map(|(free, _)| free as f64 / 1073741824.0)
    };
    let cramped_c = uv_data_free_gb.is_some_and(|gb| gb < 2.0);
    let mut hint = if uv_ver.is_none() {
        "uv not found — install it (Manage → prerequisite) and retry".to_string()
    } else if downloads_blocked {
        "UV_PYTHON_DOWNLOADS=never is set — the launcher overrides it during install".to_string()
    } else if path.is_none() {
        format!("Python {wanted} not cached yet — the installer will download it automatically")
    } else if !runs {
        format!("Managed Python {wanted} looks corrupted — the installer will force a reinstall")
    } else { String::new() };
    if cramped_c {
        hint = format!("{}uv's own data drive ({} — {:.1} GB free) is nearly full, so the Python download itself may fail. Free space there too.", if hint.is_empty() { String::new() } else { hint + " " }, uv_data_dir, uv_data_free_gb.unwrap_or(0.0));
    }
    serde_json::json!({
        "wanted": wanted, "uvVersion": uv_ver, "path": path,
        "runs": runs, "ok": uv_ver.is_some() && !downloads_blocked && !cramped_c && (path.is_none() || runs),
        "hint": hint, "uvDataDir": uv_data_dir, "uvDataFreeGb": uv_data_free_gb,
    })
}

/// Pinokio-managed folder? Markers (pinokio.js/pinokio.json/.pinokio) live in
/// the app dir or up to two levels above it (`api/<name>.git/app` layout),
/// so check self + parent + grandparent. Returns the marker dir, if any.
/// A Pinokio install has its own lifecycle scripts and env — installing,
/// env-repairing or wiping inside it would corrupt it, so install()/reinstall()
/// refuse and the UI guides to fresh-install + reuse its model folders.
pub(crate) fn pinokio_root(repo: &Path) -> Option<PathBuf> {
    let mut cur = Some(repo);
    for _ in 0..3 {
        let dir = cur?;
        if dir.join("pinokio.js").exists() || dir.join("pinokio.json").exists() || dir.join(".pinokio").exists() {
            return Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    None
}

/// Target-folder triage: what is already in the install location?
/// Verdicts: empty | ours_healthy | ours_broken_env | repo_no_env |
/// pinokio | foreign. The installer UI turns this into Fresh / Adopt-Reuse /
/// Pick-another-folder choices instead of silently merging over unknowns.
#[tauri::command]
pub fn classify_target() -> serde_json::Value {
    let repo = get_repo_dir();
    let no_target = serde_json::json!({
        "verdict": "empty", "repo": repo.to_string_lossy().to_string(),
        "exists": false, "entries": [], "hint": "Empty folder — clean install."});
    if !repo.exists() { return no_target; }
    let mut entries: Vec<String> = Vec::new();
    let mut count = 0usize;
    if let Ok(rd) = std::fs::read_dir(&repo) {
        for e in rd.flatten() {
            count += 1;
            if entries.len() < 12 { entries.push(e.file_name().to_string_lossy().to_string()); }
        }
    }
    if count == 0 { return no_target; }
    // Only our own bookkeeping files (e.g. desktop-config.json written at boot)
    // means "empty" for install purposes — don't scare fresh users.
    const BENIGN: &[&str] = &["desktop-config.json", ".uv-cache", "envs.json"];
    if entries.len() == count && entries.iter().all(|n| BENIGN.contains(&n.as_str())) { return no_target; }
    let has_wgp = repo.join("wgp.py").exists();
    let has_git = repo.join(".git").exists();
    // Upstream remote without spawning git: read .git/config.
    let remote: Option<String> = std::fs::read_to_string(repo.join(".git").join("config")).ok()
        .and_then(|s| s.lines().find(|l| l.trim_start().starts_with("url ="))
            .map(|l| l.trim_start()["url =".len()..].trim().to_string()));
    let mut envs = serde_json::Map::new();
    for ed in ["env_uv", "env_venv", "env_conda"] {
        let base = repo.join(ed);
        if !base.exists() { continue; }
        #[cfg(windows)] let py = base.join("Scripts\\python.exe");
        #[cfg(not(windows))] let py = base.join("bin/python");
        envs.insert(ed.into(), serde_json::json!({"exists": true, "healthy": py.exists()}));
    }
    let any_healthy = envs.values().any(|e| e.get("healthy").and_then(|h| h.as_bool()).unwrap_or(false));
    let managed_active = !crate::status::get_active_env().is_null();
    let has_config = repo.join("wgp_config.json").exists();
    let pinokio_dir = pinokio_root(&repo);
    let pinokio = pinokio_dir.is_some();
    let stale_tmp: Vec<String> = entries.iter().filter(|n| n.starts_with(".wan2gp-clone-tmp-")).cloned().collect();
    let models = ["ckpts", "loras", "outputs"].iter()
        .filter(|d| repo.join(d).exists()).map(|d| d.to_string()).collect::<Vec<_>>();
    let (verdict, hint) = if pinokio {
        ("pinokio", "Reusing Pinokio's Wan2GP install directly is not possible — Pinokio owns that folder's lifecycle and environment. Install fresh into an empty folder and reuse Pinokio's model folders instead (no re-downloads, Pinokio keeps working).")
    } else if has_wgp && any_healthy {
        ("ours_healthy", "A working Wan2GP install is already here — you can reuse it instead of reinstalling.")
    } else if has_wgp && !envs.is_empty() {
        ("ours_broken_env", "Wan2GP repo is here but its Python environment is broken/incomplete — repair keeps your models and settings.")
    } else if has_wgp {
        ("repo_no_env", "Wan2GP repo without a Python environment — install just the environment, no re-clone needed.")
    } else {
        ("foreign", "Folder isn't empty and isn't a Wan2GP install — pick an empty folder or wipe it first so upstream files can't collide.")
    };
    serde_json::json!({
        "verdict": verdict, "hint": hint,
        "repo": repo.to_string_lossy().to_string(), "entries": entries, "entryCount": count,
        "hasRepo": has_wgp, "hasGit": has_git, "gitRemote": remote,
        "envs": envs, "managedActive": managed_active, "hasConfig": has_config,
        "pinokio": pinokio, "pinokioRoot": pinokio_dir.map(|p| p.to_string_lossy().to_string()),
        "staleCloneTmp": stale_tmp, "modelDirs": models,
    })
}

/// Classify uv's piped download lines into live install-progress events for
/// the installer's download panel (piped uv shows no byte-bars of its own,
/// but it prints "Downloading X (Y MiB)" → "Prepared/Installed N" →
/// "+ x==ver" — enough for per-file rows with sizes and versions).
fn install_progress_classify(app: &tauri::AppHandle, chunk: &str) {
    for raw in chunk.split('\n') {
        let t = raw.trim();
        if t.is_empty() { continue; }
        let low = t.to_lowercase();
        let ev = if let Some(rest) = low.strip_prefix("downloading ") {
            let (name, size) = rest.split_once('(')
                .map(|(n, s)| (n.trim().to_string(), s.trim_end_matches(')').trim().to_string()))
                .unwrap_or((rest.to_string(), String::new()));
            // skip venv/python bootstraps noise — keep real wheels
            if name.is_empty() { continue; }
            Some(serde_json::json!({"phase": "downloading", "pkg": name, "size": size}))
        } else if let Some(rest) = t.strip_prefix("Resolved ") {
            let n = rest.split_whitespace().next().unwrap_or("?");
            Some(serde_json::json!({"phase": "resolved", "count": n}))
        } else if t.starts_with("Prepared ") {
            Some(serde_json::json!({"phase": "prepared"}))
        } else if t.starts_with("Installed ") {
            Some(serde_json::json!({"phase": "installed-batch"}))
        } else if let Some(stripped) = t.strip_prefix("+ ") {
            if let Some((pkg, ver)) = stripped.split_once("==") {
                Some(serde_json::json!({"phase": "package-installed", "pkg": pkg.trim(), "version": ver.trim()}))
            } else { None }
        } else { None };
        if let Some(ev) = ev { let _ = app.emit("install-progress", ev); }
    }
}
fn install_failure_hint(tail: &str) -> String {
    let low = tail.to_lowercase();
    if low.contains("no interpreter found for python") {
        let ver = pinned_python_wanted();
        return format!("uv couldn't provision Python {ver} (exact pin from setup.py). Fix: `uv self update`, then `uv python install {ver}`, and retry. Offline? Install Python {ver} from https://www.python.org/downloads/ and retry.");
    }
    if low.contains("no space left on device") || low.contains("not enough space") || low.contains("disk full") {
        return "Disk full on the install drive — free space (50+ GB recommended) and retry.".into();
    }
    if low.contains("permission denied") || low.contains("access is denied") || low.contains("winerror 5") {
        return "Files locked or access denied — close programs using the install folder, exclude it from antivirus/ransomware protection, and retry.".into();
    }
    if low.contains("failed to fetch") || low.contains("connection reset") || low.contains("temporary failure in name resolution") || low.contains("proxy") {
        return "Network error fetching packages — check connection/proxy/VPN and retry.".into();
    }
    if low.contains("git clone failed") || low.contains("could not resolve host: github.com") {
        return "GitHub unreachable — check connection/proxy and retry.".into();
    }
    format!("setup.py failed — see the console output above for the failing command.")
}

#[tauri::command]
pub async fn install(app: tauri::AppHandle, env_type: Option<String>) -> Result<serde_json::Value,String> {
    
    mutating_try("install")?;
    let env = env_type.unwrap_or("uv".into()); // uv | venv | conda
    let repo = get_repo_dir();
    let emit = |msg: &str| { crate::base::push_log(msg, "setup"); let _ = app.emit("setup-output", msg.to_string()); };
    // hardware-aware header (driver warning surfaces before 20min install)
    let gpu = get_gpu_info_sync();
    let plan = build_install_plan(&gpu);
    emit(&format!("[hw] GPU: {} ({}) — {} / {} — profile {}\n", plan["gpuName"].as_str().unwrap_or("?"), plan["vendor"].as_str().unwrap_or("?"), plan["cuda"].as_str().unwrap_or("?"), plan["torch"].as_str().unwrap_or("?"), plan["profile"].as_str().unwrap_or("?")));
    if let Some(w)=plan["driverWarning"].as_str() { if !w.is_empty() { emit(&format!("[warn] {w}\n")); } }
    emit(&format!("[env] requested: {env}\n"));
    let emit_phase = |id: &str, label: &str, done: bool| { let _ = app.emit("setup-phase", serde_json::json!({"id": id, "label": label, "done": done})); };
    // Never install/repair inside a Pinokio-managed tree (own lifecycle + env).
    // Fresh-install elsewhere and point the model folders at its library.
    if let Some(where_) = pinokio_root(&repo) {
        mutating_done();
        return Err(format!("This folder is Pinokio-managed ({}). Installing here would corrupt Pinokio's Wan2GP. Pick an empty folder and reuse Pinokio's ckpts/loras/outputs as your model folders — no re-downloads, Pinokio keeps working.", where_.display()));
    }
    if repo.join("wgp.py").exists() {
        emit_phase("clone", "Clone Wan2GP repository", true);
    } else {
        emit_phase("clone", "Clone Wan2GP repository", false);
        emit(&format!("[*] Cloning Wan2GP into {}\n", repo.display()));
        std::fs::create_dir_all(&repo).map_err(|e| e.to_string())?;
        // If repo already exists but is not empty (e.g. contains desktop-config.json from previous launch),
        // git clone directly into it fails ("already exists and is not empty"). Clone into a temp dir
        // inside the target (same volume) then merge, preserving user files — mirrors Electron mergeDir.
        let needs_tmp = repo.exists() && std::fs::read_dir(&repo).is_ok_and(|mut it| it.next().is_some());
        if needs_tmp {
            let tmp = repo.join(format!(".wan2gp-clone-tmp-{}", std::process::id()));
            if tmp.exists() { let _ = std::fs::remove_dir_all(&tmp); }
            emit(&format!("[*] Target not empty — cloning into temp {}\n", tmp.display()));
            use tauri_plugin_shell::ShellExt;
            let (mut rx, _child) = app.shell().command("git").args(["clone","--depth","1","https://github.com/deepbeepmeep/Wan2GP.git", &tmp.to_string_lossy()]).spawn().map_err(|e| e.to_string())?;
            use tauri_plugin_shell::process::CommandEvent;
            while let Some(ev) = rx.recv().await { match ev { CommandEvent::Stdout(b) => emit(&String::from_utf8_lossy(&b)), CommandEvent::Stderr(b) => emit(&String::from_utf8_lossy(&b)), _ => {} } }
            if !tmp.join("wgp.py").exists() { mutating_done(); return Err("git clone failed — check output above".into()); }
            // merge tmp into repo, keep user files (desktop-config.json, wgp_config.json, .electron)
            const KEEP: &[&str] = &["desktop-config.json", "wgp_config.json", ".electron", "envs.json"];
            for e in std::fs::read_dir(&tmp).map_err(|e| e.to_string())? {
                let e = e.map_err(|e| e.to_string())?; let name = e.file_name().to_string_lossy().to_string();
                if KEEP.contains(&name.as_str()) { continue; }
                let dst = repo.join(&name);
                if dst.exists() && name==".git" { let _ = std::fs::remove_dir_all(&dst); }
                let _ = std::fs::rename(e.path(), &dst).or_else(|_| { if e.path().is_dir() { fs_extra_fallback_copy_dir(&e.path(), &dst) } else { std::fs::copy(e.path(), &dst).map(|_| ()).map_err(|e| e.to_string()) } });
            }
            let _ = std::fs::remove_dir_all(&tmp);
        } else {
            let (mut rx, _child) = app.shell().command("git").args(["clone","--depth","1","https://github.com/deepbeepmeep/Wan2GP.git", &repo.to_string_lossy()]).spawn().map_err(|e| e.to_string())?;
            while let Some(ev) = rx.recv().await { match ev { CommandEvent::Stdout(b) => emit(&String::from_utf8_lossy(&b)), CommandEvent::Stderr(b) => emit(&String::from_utf8_lossy(&b)), _ => {} } }
        }
        if !repo.join("wgp.py").exists() { mutating_done(); emit_phase("clone", "Clone Wan2GP repository", true); return Err("git clone failed — check output above".into()); }
        emit("[*] Repository cloned.\n");
        emit_phase("clone", "Clone Wan2GP repository", true);
    }
    emit(&format!("[*] Installing env={env} via setup.py (streaming)…\n"));
    // ponytail: don't pre-create env here — setup.py does `uv venv --seed` itself and fails if dir already exists.
    // If Tauri pre-creates env_uv then setup.py's `uv venv --seed env_uv` hits "already exists at env_uv".
    // Let setup.py own env creation; we only ensure envs.json is updated after success.
    let env_path = match env.as_str() {
        "conda" => repo.join("env_conda"),
        "venv" => repo.join("env_venv"),
        _ => repo.join("env_uv"),
    };
    // If a previous half-created env blocks setup.py, remove only stale env (python.exe missing).
    // Don't delete a valid env on every Install — that would force full re-download (slow).
    // Also catch present-but-broken: python.exe exists but won't run (interrupted
    // venv creation). Without this, a Retry fails the same way forever.
    #[cfg(windows)] let py_exe = env_path.join("Scripts\\python.exe");
    #[cfg(not(windows))] let py_exe = env_path.join("bin/python");
    let stale = env_path.exists() && !py_exe.exists();
    let broken = env != "conda" && py_exe.exists()
        && silent_command(&py_exe).arg("-c").arg("import sys").output().is_ok_and(|o| !o.status.success());
    if stale || broken {
        emit(&format!("[*] Removing {} env at {} …\n", if broken { "broken" } else { "stale" }, env_path.display()));
        let _ = std::fs::remove_dir_all(&env_path);
    }
    // fix: hardlink warning when cache (C:) and target (D:) differ → move cache to repo/.uv-cache on same drive so hardlink works (fast)
    let uv_cache = repo.join(".uv-cache");
    let _ = std::fs::create_dir_all(&uv_cache);
    std::env::set_var("UV_CACHE_DIR", uv_cache.to_string_lossy().to_string());
    // don't force copy — hardlink on same drive is faster; warning disappears when cache is on D:
    // Always allow uv to download the pinned interpreter (a user/system
    // `python-downloads = "never"` config otherwise fails with
    // "No interpreter found for Python X" — ATFGriff's exact error).
    std::env::set_var("UV_PYTHON_DOWNLOADS", "automatic");
    // Clean leftover clone-tmp dirs from previously interrupted installs.
    if let Ok(rd) = std::fs::read_dir(&repo) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with(".wan2gp-clone-tmp-") {
                let _ = std::fs::remove_dir_all(e.path());
                emit(&format!("[*] Removed leftover {n} from an interrupted install.\n"));
            }
        }
    }
    // Pre-provision the exact Python setup.py will demand via
    // `uv venv --python X` (uv env only — conda/venv bring their own).
    // Fail fast here with a copy-paste fix instead of 10 minutes in.
    if env == "uv" {
        let wanted = pinned_python_wanted();
        emit_phase("venv", "Create Python virtual environment", false);
        if let Err(e) = ensure_uv_python(&app, &emit, &wanted).await {
            mutating_done();
            return Err(e);
        }
    }
    // run setup.py with the env's python (hardware-aware: setup.py reads setup_config.json + GPU)
    {
        let (py, args): (String, Vec<String>) = if env.as_str() == "conda" { ("conda".into(), vec!["run".into(), "-p".into(), env_path.to_string_lossy().to_string(), "python".into(), "setup.py".into(), "install".into(), "--env".into(), env.clone(), "--auto".into()]) } else {
            let p = if env=="uv" { env_path.join(if cfg!(windows){"Scripts\\python.exe"} else {"bin/python"}) } else { env_path.join(if cfg!(windows){"Scripts\\python.exe"} else {"bin/python3"}) };
            let py_bin = if p.exists() { p.to_string_lossy().to_string() } else if env=="uv" { "uv".into() } else { "python".into() };
            if py_bin=="uv" { ("uv".into(), vec!["run".into(), "--with".into(), "setuptools".into(), "python".into(), "setup.py".into(), "install".into(), "--env".into(), env.clone(), "--auto".into()]) }
            else { (py_bin, vec!["setup.py".into(), "install".into(), "--env".into(), env.clone(), "--auto".into()]) }
        };
        let (mut rx, _child) = app.shell().command(&py).args(args).current_dir(&repo).spawn().map_err(|e| e.to_string())?;
        // track which phases we've started / finished — done events fire once:
        // the sliding window re-matches old tokens every chunk, and the
        // frontend completes the RUNNING phase on any foreign done event.
        let mut phases = std::collections::HashSet::new();
        let mut phases_done = std::collections::HashSet::new();
        let mut do_phase = |id: &str, label: &str| { if phases.insert(id.to_string()) { let _ = app.emit("setup-phase", serde_json::json!({"id": id, "label": label, "done": false})); } };
        let mut done_phase = |id: &str, label: &str| { if phases_done.insert(id.to_string()) { let _ = app.emit("setup-phase", serde_json::json!({"id": id, "label": label, "done": true})); } };
        let mut tail = String::new();
        let mut exit_code: Option<i32> = None;
        // Sliding window: shell chunks split anywhere, so markers spanning a
        // boundary ("[2/3]", "+ torch==…") are matched against the tail.
        let mut window = String::new();
        while let Some(ev) = rx.recv().await {
            match ev {
                CommandEvent::Stdout(b) | CommandEvent::Stderr(b) => {
                    let txt = String::from_utf8_lossy(&b).to_string();
                    emit(&txt);
                    install_progress_classify(&app, &txt);
                    tail.push_str(&txt);
                    if tail.len() > 8000 { tail.drain(..tail.len() - 8000); }
                    window.push_str(&txt.to_lowercase());
                    if window.len() > 600 { window.drain(..window.len() - 600); }
                    let low = window.as_str();
                    // Phase starts: setup.py's own "[*] Install <Component>" headers
                    // (e.g. "[*] Install Flash Attention spas-sage-attn") plus the
                    // [1/3]-style tags and uv's download lines as backstops.
                    if low.contains("[1/3]") || low.contains("preparing environment") { do_phase("venv", "Create Python virtual environment"); }
                    if low.contains("[2/3]") || low.contains("installing torch") || low.contains("download.pytorch.org") { done_phase("venv", "Create Python virtual environment"); do_phase("torch", "Install PyTorch + CUDA"); }
                    if low.contains("[3/3]") || low.contains("installing requirements") || low.contains("-r requirements") { done_phase("torch", "Install PyTorch + CUDA"); do_phase("reqs", "Install Python dependencies"); }
                    if let Some(h) = low.split("[*] install").nth(1) {
                        // Component header — check flash/sparge before sage
                        // ("spas-sage-attn" contains "sage").
                        if h.starts_with("flash") || h.contains("spas-sage") || h.contains("sparge") { do_phase("flash", "Install Flash Attention"); }
                        else if h.contains("sage") { do_phase("sage", "Install Sage Attention kernel"); }
                        else if h.contains("triton") { do_phase("triton", "Install Triton compiler"); }
                        else if h.contains("torch") || h.contains("cuda") { done_phase("venv", "Create Python virtual environment"); do_phase("torch", "Install PyTorch + CUDA"); }
                        else if h.contains("nunchaku") || h.contains("gguf") || h.contains("kernel") || h.contains("lightx2v") { do_phase("kernels", "Install GPU kernels (nunchaku/GGUF)"); }
                        else if h.contains("requirement") { done_phase("torch", "Install PyTorch + CUDA"); do_phase("reqs", "Install Python dependencies"); }
                    }
                    if low.contains("downloading triton") || low.contains("+ triton") { do_phase("triton", "Install Triton compiler"); }
                    if low.contains("downloading sageattention") || low.contains("+ sageattention") { do_phase("sage", "Install Sage Attention kernel"); }
                    if low.contains("downloading flash") || low.contains("+ flash") { do_phase("flash", "Install Flash Attention"); }
                    if low.contains("downloading nunchaku") || low.contains("+ nunchaku") { do_phase("kernels", "Install GPU kernels (nunchaku/GGUF)"); }
                    // Completions: uv's "+ <pkg>==" resolved lines (each arrives
                    // separately from "Installed 1 package", so single tokens).
                    if low.contains("+ torch==") { done_phase("torch", "Install PyTorch + CUDA"); }
                    if low.contains("+ triton") { done_phase("reqs", "Install Python dependencies"); done_phase("triton", "Install Triton compiler"); }
                    if low.contains("+ sageattention") { done_phase("sage", "Install Sage Attention kernel"); }
                    if low.contains("+ spas-sage") || low.contains("+ sparge") { /* sparge done — flash-attn still ahead */ }
                    if low.contains("+ flash") { done_phase("flash", "Install Flash Attention"); }
                    if low.contains("+ llamacpp") || low.contains("+ lightx2v") { done_phase("kernels", "Install GPU kernels (nunchaku/GGUF)"); }
                    // setup.py's own finale. (NOT "is now active" — it prints that
                    // at env activation too, which would complete everything
                    // while kernels still download. The end-of-stream block below
                    // is the backstop.)
                    if low.contains("automatic install complete") {
                        done_phase("venv", "Create Python virtual environment");
                        done_phase("torch", "Install PyTorch + CUDA");
                        done_phase("reqs", "Install Python dependencies");
                        done_phase("triton", "Install Triton compiler");
                        done_phase("sage", "Install Sage Attention kernel");
                        done_phase("flash", "Install Flash Attention");
                        done_phase("kernels", "Install GPU kernels (nunchaku/GGUF)");
                    }
                }
                CommandEvent::Terminated(p) => { exit_code = p.code; }
                CommandEvent::Error(e) => { tail.push_str(&e); emit(&format!("[!] {e}\n")); }
                _ => {}
            }
        }
        let code = exit_code.unwrap_or(-1);
        if code != 0 {
            // setup.py failed — report honestly, no false "Installation complete!".
            // (ATFGriff: exit 2 from `uv venv --python 3.11.14` was swallowed here.)
            let hint = install_failure_hint(&tail);
            emit(&format!("[!] setup.py exited with code {code}.\n[!] {hint}\n"));
            mutating_done();
            return Err(format!("Install failed (setup.py exited code {code}). {hint}"));
        }
        done_phase("venv", "Create Python virtual environment");
        done_phase("torch", "Install PyTorch + CUDA");
        done_phase("reqs", "Install Python dependencies");
        done_phase("triton", "Install Triton compiler");
        done_phase("sage", "Install Sage Attention kernel");
        done_phase("flash", "Install Flash Attention");
        done_phase("kernels", "Install GPU kernels (nunchaku/GGUF)");
        emit_phase("done", "Finalize installation", true);
    }
    // Post-install smoke test: exit 0 from setup.py is not proof the env works
    // (ATFGriff got exit 2 AND a success message; subtler breakage can exit 0).
    // Gate favourite-plugins + "Installation complete!" on torch importing
    // and the GPU being visible from inside the new env.
    if env == "uv" || env == "venv" {
        #[cfg(windows)] let smoke_py = env_path.join("Scripts\\python.exe");
        #[cfg(not(windows))] let smoke_py = if env == "uv" { env_path.join("bin/python") } else { env_path.join("bin/python3") };
        emit("[*] Verifying install: importing torch in the new environment…\n");
        let smoke = silent_command(&smoke_py).args(["-c", "import torch; print('torch ' + torch.__version__ + ' cuda=' + str(torch.cuda.is_available()))"]).current_dir(&repo).output();
        match smoke {
            Ok(o) if o.status.success() => {
                let line = String::from_utf8_lossy(&o.stdout).trim().to_string();
                emit(&format!("[✓] Smoke test passed: {line}\n"));
                let vendor = get_gpu_info_sync().get("vendor").and_then(|v| v.as_str()).unwrap_or("").to_uppercase();
                if vendor == "NVIDIA" && !line.contains("cuda=True") {
                    mutating_done();
                    return Err("Install finished but torch can't see the NVIDIA GPU (cuda=False) — likely a driver/CUDA mismatch. Update to NVIDIA R580+, reboot, then repair the environment.".into());
                }
            }
            _ => {
                mutating_done();
                return Err(format!("Install finished but `import torch` fails in {} — the environment is broken. Retry the install (the broken env is removed automatically) or report it with Copy diagnostics.", smoke_py.display()));
            }
        }
    }
    emit("[*] Install finished.\n");
    // Remember where the working install lives (next to the data-dir override
    // in the home dir, so it survives drive changes). If the drive letter
    // changes or the drive disconnects later, first-run warns instead of
    // silently showing a blank installer.
    let _ = atomic_write(&home_dir().join(".wan2gp-tauri-installed"), repo.to_string_lossy().as_ref());
    // favourite plugins (Manage → Plugins ★): auto-clone after fresh setup
    crate::plugins::ensure_favorite_plugins(app.clone()).await;
    mutating_done();
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command]
pub async fn reinstall(app: tauri::AppHandle, options: Option<serde_json::Value>) -> Result<serde_json::Value,String> {
    mutating_try("reinstall")?;
    let repo = get_repo_dir();
    let emit = |msg: &str| { crate::base::push_log(msg, "setup"); let _ = app.emit("setup-output", msg.to_string()); };
    emit("[*] Removing existing installation...\n");
    // A wipe inside a Pinokio tree would destroy Pinokio's install — refuse.
    if let Some(where_) = pinokio_root(&repo) {
        mutating_done();
        return Err(format!("This folder is Pinokio-managed ({}). Wiping it would destroy Pinokio's Wan2GP. Uninstall from inside Pinokio instead, or pick another folder.", where_.display()));
    }
    // Optional model relocation FIRST (backup dialog: move libraries out before
    // the wipe). Aborts before touching anything when a move fails.
    let mut moved_models: Vec<String> = Vec::new();
    if let Some(moves) = options.as_ref().and_then(|o| o.get("moveModels")).and_then(|m| m.as_array()) {
        for mv in moves {
            let from = mv.get("from").and_then(|x| x.as_str()).unwrap_or("");
            let to = mv.get("to").and_then(|x| x.as_str()).unwrap_or("");
            if from.is_empty() || to.is_empty() { continue; }
            // Never move the repo itself, and never move INTO the wiped folder.
            let low = |p: &str| p.to_lowercase();
            if low(from) == low(&repo.to_string_lossy()) { emit(&format!("[!] Skipping move of the repo itself: {from}\n")); continue; }
            if low(to).starts_with(&low(&repo.to_string_lossy())) { emit(&format!("[!] Skipping move into the wiped folder (would be deleted): {to}\n")); continue; }
            emit(&format!("[*] Moving models out before wipe: {from} → {to}\n"));
            match crate::system::move_path_inner(&app, Path::new(from), Path::new(to)).await {
                Ok(_) => moved_models.push(format!("{from} → {to}")),
                Err(e) => { mutating_done(); return Err(format!("Could not move models ({e}). Wipe aborted — nothing deleted.")); }
            }
        }
    }
    // backup plugins/finetunes (ponytail: xcopy fallback) — skippable via dialog.
    let want_backup = options.as_ref().and_then(|o| o.get("backup")).and_then(|b| b.as_bool()).unwrap_or(true);
    if !want_backup {
        emit("[!] Backup skipped by user choice — plugins/finetunes/settings will be deleted.\n");
    } else {
    let backup = get_data_dir().join(".reinstall-backup");
    let _ = std::fs::remove_dir_all(&backup);
    let _ = std::fs::create_dir_all(&backup);
    for sub in ["plugins","finetunes"] { let s = repo.join(sub); if s.exists() { let d = backup.join(sub); let _ = silent_command("xcopy").args(["/E","/I", s.to_string_lossy().as_ref(), d.to_string_lossy().as_ref()]).output(); } }
    if repo.join("wgp_config.json").exists() { let _ = std::fs::copy(repo.join("wgp_config.json"), backup.join("wgp_config.json")); }
    }
    if repo.exists() {
        // ponytail: .electron is the live WebView2 Shared Dictionary — locked while launcher runs, keep it (Electron d186d49+e3e8505)
        // .reinstall-backup must survive too (data_dir == repo on default installs) — restore_backup() merges it back after install.
        const KEEP: &[&str] = &[".electron", ".reinstall-backup"];
        let trash = repo.with_file_name(format!("{}.trash-{}", repo.file_name().unwrap_or_default().to_string_lossy(), std::process::id()));
        let mut ok = true;
        if let Ok(ents) = std::fs::read_dir(&repo) {
            let _ = std::fs::create_dir_all(&trash);
            for e in ents.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if KEEP.contains(&name.as_str()) { continue; }
                let src = e.path(); let dst = trash.join(&name);
                if std::fs::rename(&src, &dst).is_err() {
                    // locked .git/.uv-cache — clear read-only and retry, then blocking rm fallback
                    #[cfg(windows)] { let _ = silent_command("cmd").args(["/C", &format!("attrib -R /S /D \"{}\"", src.display())]).output(); }
                    if std::fs::rename(&src, &dst).is_err() {
                        let mut moved = false;
                        for _ in 0..5 {
                            if std::fs::remove_dir_all(&src).is_ok() || std::fs::remove_file(&src).is_ok() { moved = true; break; }
                            std::thread::sleep(std::time::Duration::from_millis(400));
                        }
                        if !moved { ok = false; }
                        let _ = std::fs::remove_dir_all(&dst);
                    }
                }
            }
            // ensure .git is gone before clone
            for _ in 0..5 {
                let git = repo.join(".git");
                if !git.exists() { break; }
                #[cfg(windows)] { let _ = silent_command("cmd").args(["/C", &format!("attrib -R /S /D \"{}\"", git.display())]).output(); }
                let _ = std::fs::remove_dir_all(&git);
                if git.exists() { std::thread::sleep(std::time::Duration::from_millis(400)); } else { break; }
            }
            // background delete of trash
            let trash_clone = trash.clone();
            std::thread::spawn(move || { let _ = std::fs::remove_dir_all(&trash_clone); });
            if ok { emit("[*] Old installation moved to trash (kept .electron) — fresh install starting...\n"); }
        }
        if !ok {
            emit("[!] Some files were locked — fresh install will reuse the folder.\n");
        }
    }
    let _ = std::fs::remove_file(get_envs_file());
    let _ = std::fs::remove_dir_all(get_data_dir().join(".py-shim"));
    mutating_done(); Ok(serde_json::json!({"ok": true, "success": true, "movedModels": moved_models}))
}
/// Merge the reinstall backup back after a fresh install (plugins/finetunes/
/// wgp_config.json). Previously the backup was written but never restored —
/// and wiped with everything else when data_dir == repo. Only entries missing
/// from the fresh clone are moved back (upstream ships its own system
/// plugins); a conflicting wgp_config.json is kept aside, never overwritten.
#[tauri::command]
pub async fn restore_backup(app: tauri::AppHandle) -> Result<serde_json::Value,String> {
    use tauri::Emitter;
    let repo = get_repo_dir();
    let backup = get_data_dir().join(".reinstall-backup");
    let emit = |msg: &str| { crate::base::push_log(msg, "setup"); let _ = app.emit("setup-output", msg.to_string()); };
    if !backup.exists() { return Ok(serde_json::json!({"ok": true, "success": true, "restored": []})); }
    let mut restored: Vec<String> = Vec::new();
    for sub in ["plugins", "finetunes"] {
        let s = backup.join(sub);
        if !s.exists() { continue; }
        let d = repo.join(sub);
        let _ = std::fs::create_dir_all(&d);
        if let Ok(rd) = std::fs::read_dir(&s) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                let dst = d.join(&name);
                if dst.exists() { continue; } // fresh clone's own copy wins
                if std::fs::rename(e.path(), &dst).is_ok() { restored.push(format!("{sub}/{name}")); }
            }
        }
    }
    let bc = backup.join("wgp_config.json");
    if bc.exists() {
        if !repo.join("wgp_config.json").exists() {
            if std::fs::copy(&bc, repo.join("wgp_config.json")).is_ok() { restored.push("wgp_config.json".into()); }
        } else if std::fs::copy(&bc, repo.join("wgp_config.backup.json")).is_ok() {
            restored.push("wgp_config.backup.json (your old settings — review & merge manually)".into());
        }
    }
    let _ = std::fs::remove_dir_all(&backup);
    emit(&format!("[*] Backup restored: {}\n", if restored.is_empty() { "nothing new (fresh defaults kept)".into() } else { restored.join(", ") }));
    Ok(serde_json::json!({"ok": true, "success": true, "restored": restored}))
}
#[tauri::command]
pub async fn uninstall(app: tauri::AppHandle, options: Option<serde_json::Value>) -> Result<serde_json::Value,String> {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    mutating_try("uninstall")?;
    let repo = get_repo_dir();
    if !repo.exists() { mutating_done(); return Err("Wan2GP not installed".into()); }
    if let Some(where_) = pinokio_root(&repo) {
        mutating_done();
        return Err(format!("This folder is Pinokio-managed ({}). Uninstall it from inside Pinokio — the launcher won't touch it.", where_.display()));
    }
    // Explicit choice comes from the uninstall modal (Keep my models /
    // Delete everything / Cancel-abort). Legacy callers without options get
    // the old native confirms.
    let keep = match options.as_ref().and_then(|o| o.get("keepModels")).and_then(|k| k.as_bool()) {
        Some(k) => k,
        None => {
            if !app.dialog().message("Uninstall Wan2GP?\n\nRemoves the app, its Python environment and packages.").title("Uninstall Wan2GP").kind(MessageDialogKind::Info).blocking_show() {
                mutating_done();
                return Ok(serde_json::json!({"cancelled": true}));
            }
            app.dialog().message("Keep your downloaded files? (OK = keep, Cancel = delete)").title("Keep models?").kind(MessageDialogKind::Info).blocking_show()
        }
    };
    // Stop a running server first (locked files won't delete).
    let _ = crate::launch::stop_wangp(app.clone());
    // Keep-dirs under the repo survive; outside-repo model folders survive on their own.
    let mut keep_dirs: Vec<PathBuf> = Vec::new();
    if keep {
        let mp = crate::config::get_model_paths();
        for key in ["checkpoints", "loras", "output"] {
            if let Some(p) = mp.get(key).and_then(|v| v.as_str()).filter(|s| !s.is_empty() && *s != ".") {
                let abs = if Path::new(p).is_absolute() { PathBuf::from(p) } else { repo.join(p) };
                if abs.starts_with(&repo) && abs.exists() { keep_dirs.push(abs); }
            }
        }
    }
    let under_keep = |entry: &Path| keep_dirs.iter().any(|k| k == entry || k.starts_with(entry));
    if let Ok(rd) = std::fs::read_dir(&repo) {
        for e in rd.flatten() {
            let p = e.path();
            if under_keep(&p) { continue; }
            if p.is_dir() { let _ = std::fs::remove_dir_all(&p); }
            else { let _ = std::fs::remove_file(&p); }
        }
    }
    let _ = std::fs::remove_file(get_envs_file());
    let kept_paths: Vec<String> = keep_dirs.iter()
        .filter(|p| p.exists())
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    let leftover = if repo.exists() {
        match std::fs::remove_dir(&repo) {
            Ok(()) => None,
            Err(_) => Some(repo.to_string_lossy().to_string()),
        }
    } else { None };
    crate::base::invalidate_path_cache();
    let _ = std::fs::remove_file(home_dir().join(".wan2gp-tauri-installed"));
    mutating_done();
    Ok(serde_json::json!({"success": true, "keptFiles": keep && !kept_paths.is_empty(), "keptPaths": kept_paths, "leftoverFolder": leftover}))
}
#[tauri::command]
pub async fn sync_kernels(app: tauri::AppHandle) -> Result<serde_json::Value,String> {
    mutating_try("sync-kernels")?;
    let repo = get_repo_dir(); let cfg_path = repo.join("setup_config.json");
    let env = get_active_env();
    let raw = env.get("path").and_then(|p| p.as_str()).unwrap_or(""); let base = if std::path::Path::new(raw).is_absolute() { PathBuf::from(raw) } else { repo.join(raw.trim_start_matches(".\\").trim_start_matches("./")) };
    let py = if cfg!(windows) { base.join("Scripts\\python.exe") } else { base.join("bin/python3") };
    if !py.exists() { mutating_done(); return Err("python not found for active env".into()); }
    // ponytail: a058daf — remote fallback + commit/gguf log proves deepbeepmeep leading
    let emit_log = |msg: &str| { crate::base::push_log(msg, "setup"); let _ = app.emit("launch-log", msg.to_string()); };
    let cfg: serde_json::Value = if cfg_path.exists() {
        serde_json::from_str(&std::fs::read_to_string(&cfg_path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?
    } else {
        emit_log("[*] setup_config.json not found locally — fetching deepbeepmeep's wanted wheels from origin/main...\n");
        // try curl then powershell (mirrors get_wangp_upstream_info)
        let url = "https://raw.githubusercontent.com/deepbeepmeep/Wan2GP/main/setup_config.json";
        let mut raw = String::new();
        if let Ok(o) = silent_command("curl").args(["-sL", url]).output() { if o.status.success() { raw = String::from_utf8_lossy(&o.stdout).to_string(); } }
        if raw.trim().is_empty() || !raw.trim().starts_with('{') {
            if let Ok(o) = silent_command("powershell").args(["-NoProfile","-Command", &format!("(Invoke-WebRequest -Uri '{url}' -UseBasicParsing).Content")]).output() {
                if o.status.success() { let s = String::from_utf8_lossy(&o.stdout).to_string(); if s.trim().starts_with('{') { raw = s; } }
            }
        }
        if raw.trim().is_empty() || !raw.trim().starts_with('{') { mutating_done(); return Err("setup_config.json missing and remote fetch failed".into()); }
        emit_log("[*] using remote setup_config.json (origin/main) — deepbeepmeep's wanted wheels\n");
        serde_json::from_str(&raw).map_err(|e| e.to_string())?
    };
    // log commit + gguf version (a058daf)
    {
        let head = silent_command("git").args(["rev-parse","--short","HEAD"]).current_dir(&repo).output().ok().and_then(|o| if o.status.success() { Some(String::from_utf8_lossy(&o.stdout).trim().to_string()) } else { None }).unwrap_or("unknown".into());
        let gguf_url = cfg.get("components").and_then(|c| c.get("kernels")).and_then(|m| m.get("gguf")).and_then(|e| e.get("cmd")).and_then(|c| c.get("win")).and_then(|u| u.as_str()).unwrap_or("");
        // wheelDistVersion extract: <dist>-<version>-cp... -> version
        let gguf_ver = gguf_url.split('/').next_back().unwrap_or("").split(".whl").next().unwrap_or("").split('-').nth(1).unwrap_or("?");
        let v = if gguf_ver.is_empty() { "?" } else { gguf_ver };
        emit_log(&format!("[*] setup_config.json @ {head} (gguf {v}) — deepbeepmeep's wanted wheels\n"));
    }
    let gpu = get_gpu_info_sync(); let profile = kernel_profile_key(gpu.get("vendor").and_then(|v| v.as_str()).unwrap_or(""), gpu.get("name").and_then(|v| v.as_str()).unwrap_or(""));
    let kernels = cfg.get("gpu_profiles").and_then(|p| p.get(&profile)).and_then(|pr| pr.get("kernels")).and_then(|k| k.as_array()).cloned().unwrap_or_default();
    use tauri_plugin_shell::ShellExt; use tauri_plugin_shell::process::CommandEvent;
    let sage_safe = load_config_value().get("sageSafe").and_then(serde_json::Value::as_bool) != Some(false); // ponytail: default safe post6 (1348e5b) — only false opts into upstream post4
    // ponytail: Sage wheel is not in gpu_profiles[RTX_30].kernels (only nunchaku+gguf) — handle it separately like Electron's setSageAttentionSafe
    let mut all_kernels = kernels.clone();
    if ["RTX_30","RTX_40","RTX_50"].contains(&profile.as_str()) {
        // ensure sage is in the sync list when toggling safe/upstream, so the wheel actually swaps
        if !all_kernels.iter().any(|k| k.as_str()==Some("sage") || k.as_str()==Some("sageattention")) {
            all_kernels.push(serde_json::json!("sage"));
        }
    }
    for k in all_kernels {
        if let Some(name) = k.as_str() {
            // find wheel url — nunchaku/gguf are under components.kernels, sage is under components.sage[profile.sage]
            let mut url = if name=="sage" || name=="sageattention" {
                // sage: look up via gpu_profiles[profile].sage -> components.sage[version].cmd[win]
                let sage_ver = cfg.get("gpu_profiles").and_then(|p| p.get(&profile)).and_then(|pr| pr.get("sage")).and_then(|v| v.as_str()).unwrap_or("v220_cu13");
                cfg.get("components").and_then(|c| c.get("sage")).and_then(|m| m.get(sage_ver)).and_then(|e| e.get("cmd")).and_then(|c| c.get("win")).and_then(|u| u.as_str()).unwrap_or("").to_string()
            } else {
                cfg.get("components").and_then(|c| c.get("kernels")).and_then(|m| m.get(name)).and_then(|e| e.get("cmd")).and_then(|c| c.get("win")).and_then(|u| u.as_str()).unwrap_or("").to_string()
            };
            if url.is_empty() && (name=="sage" || name=="sageattention") {
                // fallback to known URLs if setup_config missing sage entry
                url = if sage_safe {
                    "https://github.com/woct0rdho/SageAttention/releases/download/v2.2.0-windows.post6/sageattention-2.2.0+cu130torch2.10.0andhigher.post6-cp310-abi3-win_amd64.whl".into()
                } else {
                    "https://github.com/woct0rdho/SageAttention/releases/download/v2.2.0-windows.post4/sageattention-2.2.0+cu130torch2.9.0andhigher.post4-cp39-abi3-win_amd64.whl".into()
                };
            }
            if url.is_empty() { continue; }
            // Sage safe toggle: post4 (upstream) vs post6 (safe) — respects Manage → Settings
            if name=="sage" || name=="sageattention" {
                if sage_safe && url.contains("sageattention-2.2.0+cu130torch2.9.0andhigher.post4") {
                    url = "https://github.com/woct0rdho/SageAttention/releases/download/v2.2.0-windows.post6/sageattention-2.2.0+cu130torch2.10.0andhigher.post6-cp310-abi3-win_amd64.whl".into();
                } else if !sage_safe && url.contains("sageattention-2.2.0+cu130torch2.10.0andhigher.post6") {
                    // user chose upstream, but setup_config has safe — revert to post4
                    url = "https://github.com/woct0rdho/SageAttention/releases/download/v2.2.0-windows.post4/sageattention-2.2.0+cu130torch2.9.0andhigher.post4-cp39-abi3-win_amd64.whl".into();
                }
            }
            let m = format!("[*] sync kernel {name}\n"); crate::base::push_log(&m, "setup"); let _ = app.emit("launch-log", m);
            let (mut rx, _) = app.shell().command(&py).args(["-m","pip","install", &url, "--upgrade"]).spawn().map_err(|e| e.to_string())?;
            while let Some(ev) = rx.recv().await { match ev { CommandEvent::Stdout(b)|CommandEvent::Stderr(b) => { let s = String::from_utf8_lossy(&b).to_string(); crate::base::push_log(&s, "setup"); let _ = app.emit("launch-log", s); }, _=>{} } }
        }
    }
    mutating_done(); Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command]
pub async fn update(app: tauri::AppHandle) -> Result<serde_json::Value,String> {
    mutating_try("update")?;
    let repo = get_repo_dir();
    if !repo.join(".git").exists() { mutating_done(); return Err("not a git repo".into()); }
    let (mut rx, _) = app.shell().command("git").args(["pull"]).current_dir(&repo).spawn().map_err(|e| e.to_string())?;
    while let Some(ev) = rx.recv().await { match ev { CommandEvent::Stdout(b)|CommandEvent::Stderr(b) => { let s = String::from_utf8_lossy(&b).to_string(); crate::base::push_log(&s, "setup"); let _ = app.emit("launch-log", s); }, _=>{} } }
    mutating_done(); Ok(serde_json::json!({"ok": true, "success": true}))
}
pub(crate) fn fs_extra_fallback_copy_dir(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for e in std::fs::read_dir(src).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        let s = e.path(); let d = dst.join(e.file_name());
        if s.is_dir() { fs_extra_fallback_copy_dir(&s, &d)?; } else { std::fs::copy(&s, &d).map_err(|e| e.to_string())?; }
    }
    Ok(())
}
#[tauri::command]
pub async fn install_prerequisite(app: tauri::AppHandle, tool: String) -> Result<serde_json::Value,String> {
    use tauri_plugin_shell::ShellExt;
    use tauri_plugin_shell::process::CommandEvent;
    // Silent winget installs (Electron parity: git + uv + python + conda).
    // Previously python/conda fell into `unknown tool` and the help card's
    // Download button died with a toast — and output went to the wrong channel.
    let cmd: Vec<&str> = match tool.as_str() {
        "git" => vec!["winget","install","--id","Git.Git","-e","--accept-package-agreements","--accept-source-agreements","--silent"],
        "uv" => vec!["winget","install","--id","astral-sh.uv","-e","--accept-package-agreements","--accept-source-agreements","--silent"],
        "python" => vec!["winget","install","--id","Python.Python.3.11","-e","--accept-package-agreements","--accept-source-agreements","--silent"],
        "conda" => vec!["winget","install","--id","Anaconda.Miniconda3","-e","--accept-package-agreements","--accept-source-agreements","--silent"],
        _ => return Err(format!("unknown tool {tool}")),
    };
    let emit = |msg: &str| { crate::base::push_log(msg, "setup"); let _ = app.emit("setup-output", msg.to_string()); };
    emit(&format!("[*] Installing {tool} via winget (silent, a few minutes)…\n"));
    let (mut rx, _) = app.shell().command(cmd[0]).args(&cmd[1..]).spawn().map_err(|e| e.to_string())?;
    let mut failed = false;
    while let Some(ev) = rx.recv().await {
        match ev {
            CommandEvent::Stdout(b) | CommandEvent::Stderr(b) => {
                let s = String::from_utf8_lossy(&b).to_string();
                crate::base::push_log(&s, "setup");
                let _ = app.emit("setup-output", s);
            }
            CommandEvent::Terminated(p) => { if p.code != Some(0) { failed = true; } }
            CommandEvent::Error(e) => { failed = true; emit(&format!("[!] {e}\n")); }
            _ => {}
        }
    }
    if failed { return Err(format!("{tool} install failed — see output above (winget needs network; some packages need admin approval)")); }
    emit(&format!("[✓] {tool} installed — restart the launcher so PATH picks it up.\n"));
    Ok(serde_json::json!({"ok": true, "success": true}))
}

// Pinned manifest mirrors upstream scripts/install_dlss5.ps1: one row per installed
// file (path, package id, version, expected file SHA-256).
// ponytail: the backend owns versions/SHAs so the panel can't go stale like the old hardcoded frontend copy.
const DLSS5_FILES: &[(&str, &str, &str, &str)] = &[
  ("host/nr-depth-worker.exe", "workers", "Workers v1.1.3", "F8E2967912E5D596E8E36049370487B83620B0CB5845937B681CF835BAFC6D0B"),
  ("host/nvngx.dll", "workers", "Workers v1.1.3", "58191F4D38288C6BFBDA47EF56911D32052A9789E65714F4583F426E01464638"),
  ("dlssg/dlssg-worker.exe", "workers", "Workers v1.1.3", "D93084633E0AAB4A08C43A5EE240176716EF73D87F06F35C2293509FBFC8BD00"),
  ("host/dxgi.dll", "reshade", "ReShade 6.8.0", "0CEE63F9C9F13F3AC909C5B4903F4DBB4B719A7AB3B4F13B0DEAF83C814B94F7"),
  ("host/renodx-dlss5.addon64", "renodx", "RenoDX DLSS5 4.70", "D5ADF82EB44B065F4C590AC91FE824BAB07AFEA0EB9F994BDE936710C8593952"),
  ("host/nvngx_dlssnr.dll", "dlssnr", "DLSSNR 310.8.SF-v2", "6EB209E764F39872625DEBD6ABAF45E2BB6322F6F270F781F70C059AE30B3927"),
  ("dlss/nvngx_dlss.dll", "dlss", "DLSS Super Resolution 310.8.0", "C85F971CE023C9F3492FC7455F0B01A24BA18EA39636407A846902C4360B0B7E"),
  ("dlssg/nvngx_dlssg.dll", "dlssg", "DLSS Frame Generation 310.7.0", "135EAF0733C1E37381A8C28ABCF7A862404A54132B81787C04E35D09EFC5E36F"),
];

#[tauri::command]
pub fn dlss5_status() -> serde_json::Value {
    let repo = get_repo_dir();
    if !repo.join("wgp.py").exists() { return serde_json::json!({"ok": false, "error": "Wan2GP not installed"}); }
    let dlss5 = repo.join("dlss5");
    let files: Vec<serde_json::Value> = DLSS5_FILES.iter().map(|(path, pkg, version, sha)| {
        let ok = dlss5.join(path).exists();
        serde_json::json!({"id": path, "pkg": pkg, "version": version, "sha": sha, "installed": ok})
    }).collect();
    let present = files.iter().filter(|f| f["installed"].as_bool().unwrap_or(false)).count();
    serde_json::json!({"ok": true, "installed": present > 0, "complete": present == files.len(), "present": present, "total": files.len(), "files": files})
}

// Optional DLSS5 runtime (docs/DLSS5.md): runs Wan2GP's own Install-DLSS5.ps1.
// Consent ("I ACCEPT") is taken in the UI modal, so the script gets
// -AcceptThirdPartyRisk and never blocks on Read-Host. Verdict comes from
// re-probing dlss5/, not from parsing script output.
// Best-effort classification of Install-DLSS5.ps1 output into checklist events.
// The script stays the integrity authority (pinned SHA-256 + NVIDIA sig check);
// this only mirrors its Downloading / verified / Installed lines to the UI.
fn dlss5_classify(app: &tauri::AppHandle, chunk: &str) {
    let pkg = |name: &str| -> &str {
        let n = name.to_lowercase();
        if n.contains("workers") { "workers" }
        else if n.contains("reshade") { "reshade" }
        else if n.contains("renodx") { "renodx" }
        else if n.contains("dlssnr") { "dlssnr" }
        else if n.contains("frame generation") { "dlssg" }
        else if n.contains("super resolution") { "dlss" }
        else { "other" }
    };
    for raw in chunk.split('\n') {
        let t = raw.trim().trim_end_matches('.').trim();
        if t.is_empty() { continue; }
        let ev = if let Some(name) = t.strip_prefix("Downloading ") {
            Some(serde_json::json!({"phase": "downloading", "pkg": pkg(name), "label": name.trim()}))
        } else if let Some(sha) = t.strip_prefix("verified ") {
            Some(serde_json::json!({"phase": "verified", "sha": sha.trim()}))
        } else if let Some(p) = t.strip_prefix("Installed: ") {
            Some(serde_json::json!({"phase": "installed", "path": p.trim()}))
        } else if let Some(p) = t.strip_prefix("Already installed: ") {
            Some(serde_json::json!({"phase": "present", "path": p.trim()}))
        } else if t.contains("DLSS 5 components are installed") {
            Some(serde_json::json!({"phase": "done"}))
        } else { None };
        if let Some(ev) = ev { let _ = app.emit("dlss5-progress", ev); }
    }
}

#[tauri::command]
pub async fn install_dlss5(app: tauri::AppHandle, force: bool) -> Result<serde_json::Value,String> {
    mutating_try("install-dlss5")?;
    #[cfg(not(windows))] { mutating_done(); return Err("DLSS5 is Windows-only".into()); }
    let repo = get_repo_dir();
    let emit = |msg: &str| { crate::base::push_log(msg, "setup"); let _ = app.emit("setup-output", msg.to_string()); };
    if !repo.join("wgp.py").exists() { mutating_done(); return Err("Wan2GP not installed".into()); }
    let ps1 = repo.join("scripts/install_dlss5.ps1");
    if !ps1.exists() { mutating_done(); return Err("install_dlss5.ps1 not found — update Wan2GP first".into()); }
    emit("[*] Installing DLSS5 runtime (upstream script — progress below)…\n");
    emit("[*] Stop Wan2GP first — files under dlss5/ can't be replaced while in use.\n");
    let mut args = vec!["-NoProfile".to_string(), "-ExecutionPolicy".into(), "Bypass".into(), "-File".into(), ps1.to_string_lossy().to_string(), "-WanGPRoot".into(), repo.to_string_lossy().to_string(), "-AcceptThirdPartyRisk".into()];
    if force { args.push("-Force".into()); }
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let (mut rx, _) = app.shell().command("powershell").args(&arg_refs).spawn().map_err(|e| e.to_string())?;
    let mut spawn_err = false;
    while let Some(ev) = rx.recv().await {
        match ev {
            CommandEvent::Stdout(b) | CommandEvent::Stderr(b) => {
                let s = String::from_utf8_lossy(&b).to_string();
                emit(&s);
                dlss5_classify(&app, &s);
            }
            CommandEvent::Error(e) => { emit(&format!("[!] {e}\n")); spawn_err = true; }
            _ => {}
        }
    }
    let st = dlss5_status();
    let complete = st.get("complete").and_then(|v| v.as_bool()).unwrap_or(false);
    let installed = st.get("installed").and_then(|v| v.as_bool()).unwrap_or(false);
    mutating_done();
    if complete && !spawn_err { return Ok(serde_json::json!({"ok": true, "success": true, "complete": true})); }
    if installed { return Ok(serde_json::json!({"ok": true, "success": true, "complete": false, "hint": "Partial install — see console; rerun with Force if files conflict."})); }
    Err("DLSS5 install failed — see console output".into())
}

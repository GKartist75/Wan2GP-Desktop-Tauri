//! Wan2GP install / reinstall / uninstall / kernel sync / core update.
//! Hardened installer: target-folder triage (classify_target), exact-pinned
//! Python preflight via uv (ensure_uv_python, port of Electron installPython),
//! and setup.py exit-code propagation (no more false "Installation complete!").
use tauri::Emitter;
use std::path::{Path, PathBuf};
use crate::base::*;
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;
use crate::{hw::{apply_gguf_override, build_install_plan, classify_amd_driver, get_gpu_info_sync, kernel_profile_key, wmi_all_gpus}, status::{get_active_env, resolve_env_python}};

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
async fn run_capture(app: &tauri::AppHandle, emit: impl Fn(&str) + Send + Sync, prog: &str, args: &[&str]) -> (bool, String) {
    let (mut rx, _child) = match app.shell().command(prog).args(args).spawn() {
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
/// Launcher-owned uv: <dataDir>/.tools/uv[.exe] (+ uvx/uvw siblings).
/// PATH uv may belong to another app (0.5.3 report: the Hermes agent's
/// bundled uv — self-updating it hits file locks, and it can vanish under
/// us). We prefer our own copy; PATH is fallback. Pure glue, unit-tested.
///
/// Ownership MUST come from the official standalone installer (UV_INSTALL_DIR
/// run): uv self-update only works with an install receipt, and a plain file
/// copy refuses with "Self-update is only available..." (seen live on a
/// snapshot copy). `.launcher-uv` marker = receipted install, no re-fetch.
#[cfg(windows)] const UV_EXE: &str = "uv.exe";
#[cfg(not(windows))] const UV_EXE: &str = "uv";
/// Marker proving the tools copy came from the installer (self-updatable).
const UV_MARKER: &str = ".launcher-uv";

fn owned_uv_in(data_dir: &std::path::Path) -> Option<String> {
    let p = data_dir.join(".tools").join(UV_EXE);
    // Receipted install (marker) that exists AND runs — anything else (plain
    // copy, dead file) is worse than PATH fallback.
    if !data_dir.join(".tools").join(UV_MARKER).is_file() { return None; }
    silent_command(&p).arg("--version").output().ok()
        .filter(|o| o.status.success())
        .map(|_| p.to_string_lossy().to_string())
}
fn owned_uv() -> Option<String> { owned_uv_in(&get_data_dir()) }

/// Locate a working uv on PATH (absolute file, not just `where` success).
#[cfg(windows)]
fn path_uv_exe() -> Option<std::path::PathBuf> {
    silent_command("where").arg("uv.exe").output().ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8_lossy(&o.stdout).lines().next().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_file())
}
#[cfg(not(windows))]
fn path_uv_exe() -> Option<std::path::PathBuf> {
    silent_command("which").arg("uv").output().ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8_lossy(&o.stdout).lines().next().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_file())
}

/// Run the official standalone installer INTO our tools dir (writes the
/// install receipt, so `uv self update` keeps working on our copy).
/// Best-effort — None offline or on installer failure (caller falls back).
fn installer_owned_uv_in(data_dir: &std::path::Path) -> Option<String> {
    let dir = data_dir.join(".tools");
    std::fs::create_dir_all(&dir).ok()?;
    let dir_s = dir.to_string_lossy().to_string();
    #[cfg(windows)] let ok = std::process::Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "ByPass", "-Command", "irm https://astral.sh/uv/install.ps1 | iex"])
        .env("UV_INSTALL_DIR", &dir_s).output().ok().is_some_and(|o| o.status.success());
    #[cfg(not(windows))] let ok = std::process::Command::new("sh")
        .args(["-c", "curl -LsSf https://astral.sh/uv/install.sh | sh"])
        .env("UV_INSTALL_DIR", &dir_s).output().ok().is_some_and(|o| o.status.success());
    if !ok { return None; }
    // Marker + version stamp (owned_uv_in requires both marker and a run).
    let ver = silent_command(dir.join(UV_EXE)).arg("--version").output().ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())?;
    std::fs::write(dir.join(UV_MARKER), &ver).ok()?;
    crate::base::push_log(&format!("[*] uv installed into launcher tools ({ver})."), "setup");
    owned_uv_in(data_dir)
}

/// Offline last resort: copy a working PATH uv (+ siblings) into our tools
/// dir. Works, but carries NO install receipt (self-update refuses) and NO
/// marker — a later online run replaces it with a proper install.
/// Best-effort — None when nothing usable is around.
fn copy_owned_uv_in(data_dir: &std::path::Path) -> Option<String> {
    if owned_uv_in(data_dir).is_some() { return owned_uv_in(data_dir); }
    let src = path_uv_exe()?;
    // Must actually run before we adopt it (Store shims, dead links).
    silent_command(&src).arg("--version").output().ok().filter(|o| o.status.success())?;
    let dir = data_dir.join(".tools");
    std::fs::create_dir_all(&dir).ok()?;
    if let Some(parent) = src.parent() {
        #[cfg(windows)] let siblings = ["uv.exe", "uvx.exe", "uvw.exe"];
        #[cfg(not(windows))] let siblings = ["uv", "uvx"];
        for name in siblings {
            let s = parent.join(name);
            if s.is_file() { let _ = std::fs::copy(&s, dir.join(name)); }
        }
    }
    crate::base::push_log(&format!("[*] uv copied into launcher tools (no install receipt — self-update unavailable until a proper install)."), "setup");
    // Runs-check only (marker deliberately absent).
    let p = dir.join(UV_EXE);
    silent_command(&p).arg("--version").output().ok()
        .filter(|o| o.status.success())
        .map(|_| p.to_string_lossy().to_string())
}

/// Ensure a launcher-owned uv: receipted install already present → use it;
/// else proper installer run → else offline copy → else None (PATH fallback).
fn ensure_owned_uv_in(data_dir: &std::path::Path) -> Option<String> {
    if let Some(owned) = owned_uv_in(data_dir) { return Some(owned); }
    if let Some(fresh) = installer_owned_uv_in(data_dir) { return Some(fresh); }
    copy_owned_uv_in(data_dir)
}
fn snapshot_owned_uv() -> Option<String> { ensure_owned_uv_in(&get_data_dir()) }

/// uv command to invoke: owned copy first, snapshot-then-owned, PATH fallback.
/// Never fails — bare "uv" lets the caller surface the real spawn error.
fn uv_command() -> String {
    if let Some(owned) = owned_uv() { return owned; }
    if let Some(snap) = snapshot_owned_uv() { return snap; }
    "uv".into()
}

/// Well-known install locations per tool (pure tables — unit-tested).
/// These make prerequisite installs usable INSTANTLY: no PATH refresh, no
/// launcher restart. `home` = %USERPROFILE%. Only directly-spawnable images
/// (never .bat — CreateProcess can't run those without cmd /C).
#[cfg(windows)]
fn tool_candidates(tool: &str, home: &str) -> Vec<std::path::PathBuf> {
    let p = |s: String| std::path::PathBuf::from(s);
    match tool {
        "git" => vec![
            p("C:\\Program Files\\Git\\bin\\git.exe".into()),
            p("C:\\Program Files (x86)\\Git\\bin\\git.exe".into()),
        ],
        // Scripts\conda.exe first: directly executable (condabin\conda.bat
        // is not a valid process image).
        "conda" => vec![
            p(format!("{home}\\Miniconda3\\Scripts\\conda.exe")),
            p(format!("{home}\\Anaconda3\\Scripts\\conda.exe")),
            p(format!("{home}\\miniconda3\\Scripts\\conda.exe")),
        ],
        // py launcher (system-placed, survives reinstalls).
        "py" => vec![p("C:\\Windows\\py.exe".into())],
        "python" => vec![
            p(format!("{home}\\AppData\\Local\\Programs\\Python\\Python311\\python.exe")),
            p("C:\\Python311\\python.exe".into()),
            p("C:\\Program Files\\Python311\\python.exe".into()),
        ],
        _ => vec![],
    }
}
#[cfg(not(windows))]
fn tool_candidates(tool: &str, home: &str) -> Vec<std::path::PathBuf> {
    let p = |s: String| std::path::PathBuf::from(s);
    match tool {
        "git" => vec![p("/usr/bin/git".into())],
        "conda" => vec![
            p(format!("{home}/miniconda3/bin/conda")),
            p(format!("{home}/anaconda3/bin/conda")),
        ],
        "py" | "python" => vec![p("/usr/bin/python3".into())],
        _ => vec![],
    }
}

/// First existing candidate, if any (no writes, no spawning — safe in checks).
fn known_tool_path(tool: &str) -> Option<String> {
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default();
    tool_candidates(tool, &home).into_iter().find(|p| p.is_file())
        .map(|p| p.to_string_lossy().to_string())
}

/// Absolute path to invoke: known location first, owned-copy logic for uv,
/// bare name fallback (caller surfaces the real spawn error).
pub(crate) fn tool_path(tool: &str) -> String {
    if tool == "uv" { return uv_command(); }
    known_tool_path(tool).unwrap_or_else(|| tool.to_string())
}

/// Probe for gates: absolute hit OR runnable on PATH (Store-shim filtered).
/// Cross-platform (tool_usable is Windows-only; elsewhere `which`).
pub(crate) fn tool_found(tool: &str) -> bool {
    if known_tool_path(tool).is_some() { return true; }
    if tool == "uv" && owned_uv().is_some() { return true; }
    #[cfg(windows)]
    { crate::base::tool_usable(tool) }
    #[cfg(not(windows))]
    { silent_command("which").arg(tool).output().is_ok_and(|o| o.status.success()) }
}

async fn ensure_uv_python(app: &tauri::AppHandle, emit: impl Fn(&str) + Send + Sync, wanted: &str) -> Result<String, String> {
    // Never let a user/system config with `python-downloads = "never"` silently
    // break provisioning — spawned processes inherit our env.
    std::env::set_var("UV_PYTHON_DOWNLOADS", "automatic");
    // Owned copy first (snapshot-then-owned inside) — never self-update
    // somebody else's binary if we can help it.
    let uv_bin = uv_command();
    emit(&format!("[*] Ensuring Python {wanted} via {uv_bin} (setup.py needs this exact version)…\n"));
    // Best-effort self-update first: an old uv doesn't know new patches exist
    // (3.11.14) and fails with the same "No interpreter found" error.
    // Harmless when offline or already current — failures are ignored.
    let _ = run_capture(app, &emit, uv_bin.as_str(), &["self", "update"]).await;
    // Resolve + verify the EXACT pin actually executes (a neighbouring patch
    // or a corrupted copy won't satisfy setup.py — report it, don't use it).
    let verify_exact = |p: &str| -> Option<String> {
        silent_command(p).arg("-c").arg("import sys; print(sys.version)").output().ok()
            .and_then(|o| if o.status.success() { Some(String::from_utf8_lossy(&o.stdout).trim().split_whitespace().next().unwrap_or("").to_string()) } else { None })
            .filter(|v| v.starts_with(wanted))
    };
    let find = || silent_command(uv_bin.as_str()).args(["python", "find", wanted]).output().ok()
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
    let (dl_ok, _) = run_capture(app, &emit, uv_bin.as_str(), &["python", "install", wanted]).await;
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
    // 3b) Provisioning failed and nothing usable exists — uv itself may be
    // corrupt (not just outdated: self-update can't fix a broken install).
    // This is the exact case users fixed by reinstalling uv manually, so do
    // it for them: official installer, refresh PATH, retry the download once.
    if !dl_ok {
        emit("[*] uv itself may be broken — reinstalling uv from the official installer…\n");
        #[cfg(windows)]
        let reinstalled = run_capture(app, &emit, "powershell", &["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", "& { iwr -useb https://astral.sh/uv/install.ps1 | iex }"]).await.0;
        #[cfg(not(windows))]
        let reinstalled = run_capture(app, &emit, "sh", &["-c", "curl -LsSf https://astral.sh/uv/install.sh | sh"]).await.0;
        if reinstalled {
            #[cfg(windows)]
            refresh_path_from_registry();
            #[cfg(not(windows))]
            if let Ok(h) = std::env::var("HOME") {
                let mut cur = std::env::var("PATH").unwrap_or_default();
                for d in [format!("{h}/.local/bin"), format!("{h}/.cargo/bin")] {
                    if !cur.split(':').any(|pp| pp == d) { cur = format!("{d}:{cur}"); }
                }
                std::env::set_var("PATH", cur);
            }
            emit(&format!("[*] uv reinstalled — retrying Python {wanted}…\n"));
            let (retry_ok, _) = run_capture(app, &emit, uv_bin.as_str(), &["python", "install", wanted]).await;
            if retry_ok {
                if let Some(p) = find() {
                    if let Some(v) = verify_exact(&p) {
                        emit(&format!("[*] Python {wanted} ready after uv reinstall: {p} ({v})\n"));
                        return Ok(p);
                    }
                }
            }
            emit("[!] Still failing after uv reinstall — see diagnostics below.\n");
        } else {
            emit("[!] Automatic uv reinstall failed — see manual command in the diagnostics below.\n");
        }
    }
    // 4) Force reinstall of a corrupted managed Python copy, then give up with
    // diagnostics (list what IS installed so the fix is obvious).
    if dl_ok {
        emit(&format!("[!] Managed Python {wanted} is broken (found but won't run). Forcing a clean reinstall…\n"));
        let (re_ok, _) = run_capture(app, &emit, uv_bin.as_str(), &["python", "install", "--reinstall", wanted]).await;
        if !re_ok {
            // Older uv without --reinstall: uninstall + install.
            let _ = run_capture(app, &emit, uv_bin.as_str(), &["python", "uninstall", wanted]).await;
            let (ok2, _) = run_capture(app, &emit, uv_bin.as_str(), &["python", "install", wanted]).await;
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
    // `where` is Windows-only; POSIX uses `which -a` (a bare `where` probe
    // just fails there and the diagnostic comes back empty).
    #[cfg(windows)]
    let path_hits: Vec<String> = silent_command("where").arg("python").output().ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).lines()
            .map(|l| l.trim().to_string())
            .filter(|p| !p.is_empty() && !p.to_lowercase().contains("windowsapps"))
            .collect())
        .unwrap_or_default();
    #[cfg(not(windows))]
    let path_hits: Vec<String> = silent_command("which").args(["-a", "python3.11"]).output().ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).lines()
            .map(|l| l.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect())
        .unwrap_or_default();
    cands.extend(path_hits);
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
        1. Repair uv itself — a corrupt uv can't be updated, only reinstalled (the installer already tried automatically; do it manually):\n           powershell -ExecutionPolicy ByPass -c 'irm https://astral.sh/uv/install.ps1 | iex'\n           (macOS/Linux: curl -LsSf https://astral.sh/uv/install.sh | sh)\n           then 'uv self update' + 'uv python install {wanted}' — needs network to python-build-standalone (check proxy/VPN/antivirus; uv downloads live under %APPDATA%\\uv).\n\
        2. Install exactly Python {wanted} from https://www.python.org/downloads/ — tick \u{201c}Add python.exe to PATH\u{201d} during setup, then retry.\n\
        3. Already installed 3.11 manually? It must be exactly {wanted} (see versions above), reachable via PATH or 'py -3.11', and NOT the Microsoft Store stub (Settings → Apps → Advanced app settings → App execution aliases → turn Python off), then retry.")
}

/// Check-only preflight for the installer checklist UI (no downloads).
/// Covers BOTH drives of a split install: the target drive (J: — venv,
/// wheels, models) and uv's own data drive (C: — managed Pythons live in
/// %APPDATA%\uv\python, as ATFGriff's log shows). A full C: fails the
/// Python download even when J: has terabytes free.
#[tauri::command]
pub fn python_preflight() -> serde_json::Value {
    let wanted = pinned_python_wanted();
    let uv_bin = uv_command();
    let uv_ver = silent_command(uv_bin.as_str()).arg("--version").output().ok()
        .and_then(|o| if o.status.success() { Some(String::from_utf8_lossy(&o.stdout).trim().to_string()) } else { None });
    let path = silent_command(uv_bin.as_str()).args(["python", "find", &wanted]).output().ok()
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

/// Quick `import torch` probe: Some(version) iff torch imports (no CUDA
/// judgment — just "is there a torch?"). Cheap enough to run before every
/// install; distinguishes a finished env from a failed-halfway one.
fn torch_probe(py: &Path) -> Option<String> {
    if !py.exists() { return None; }
    silent_command(py).args(["-c", "import torch; print(torch.__version__)"]).output().ok()
        .and_then(|o| o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string()))
        .filter(|v| !v.is_empty())
}

/// Full env verification: torch imports AND (on NVIDIA) the GPU is visible.
/// Ok(torch_line) on success; Err(message) otherwise. Shared by the
/// pre-install reuse check and the post-install gate so both judge by the
/// same rule. A cuda=False Err contains "cuda=False" (driver gap —
/// reinstalling won't fix it); any other Err means a broken env.
fn smoke_verify(py: &Path, repo: &Path) -> Result<String, String> {
    let out = silent_command(py).args(["-c", "import torch; print('torch ' + torch.__version__ + ' cuda=' + str(torch.cuda.is_available()))"]).current_dir(repo).output()
        .map_err(|e| format!("`import torch` failed to spawn ({e})"))?;
    if !out.status.success() {
        return Err("`import torch` fails in the environment".into());
    }
    let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let vendor = get_gpu_info_sync().get("vendor").and_then(|v| v.as_str()).unwrap_or("").to_uppercase();
    if vendor == "NVIDIA" && !line.contains("cuda=True") {
        return Err("torch can't see the NVIDIA GPU (cuda=False) — likely a driver/CUDA mismatch. Update to NVIDIA R580+, reboot, then repair the environment.".into());
    }
    if vendor == "AMD" && !line.contains("cuda=True") {
        return Err("torch can't see the AMD GPU (cuda=False) — update to a recent Adrenalin/Pro driver (>= 24.5), confirm the TheRock index URL matches your GPU family (docs/AMD-INSTALLATION.md), reboot, then repair the environment.".into());
    }
    Ok(line)
}

/// Looks like a transient network failure (worth one automatic setup.py retry)?
/// Mirrors install_failure_hint's network arms plus uv's own retry wording.
fn is_network_failure(tail: &str) -> bool {
    let low = tail.to_lowercase();
    ["failed to fetch", "operation timed out", "connection reset", "temporary failure",
     "could not resolve", "name resolution", "network is unreachable", "connection timed out",
     "connect error", "request failed after"].iter().any(|m| low.contains(m))
}

/// AMD TheRock torch source. Returns (primary, fallback) full torch-step
/// commands for setup.py's `{pip} {torch_cmd}` splice.
///
/// Primary is the exact-pinned ROCm 7.15 stack on the legacy multi-arch
/// aggregate index — verified 2026-09-07: pip resolves every profile to
/// torch 2.12.0+rocm7.15.0a20260728 (confirmed working on RDNA 4 / R9700),
/// and pip's own resolver refuses the 10.x builds
/// there, so the pin can't drift to ROCm 10. The device packs are pinned
/// explicitly (bracket-free, so setup.py's plain string splice can't mangle
/// them); they pull the matching rocm-sdk-device packs, torch pulls
/// rocm[libraries] itself. No `rocm[devel]` (~875MB SDK tools our AMD
/// profile never invokes — no AMD attention builds) and no [device-*]
/// extras (same reason: explicit pins, zero brackets).
///
/// Fallback is the community staging float (no rocm[devel]: per-family
/// staging indexes carry no `rocm` win wheels).
pub(crate) fn amd_therock_torch_cmds(profile: &str, gpu_name: &str) -> Option<(String, String)> {
    const MULTI: &str = "https://rocm.nightlies.amd.com/whl-multi-arch/";
    const STAGE: &str = "https://rocm.nightlies.amd.com/v2-staging";
    const PIN_TORCH: &str = "2.12.0+rocm7.15.0a20260728";
    const PIN_VISION: &str = "0.27.0+rocm7.15.0a20260728";
    const PIN_AUDIO: &str = "2.11.0+rocm7.15.0a20260728";
    let g = gpu_name.to_uppercase();
    // (device targets, staging family) per installer profile. Every target
    // below was verified to carry 2.12.0+rocm7.15 cp311-win device builds.
    let (targets, fam): (&[&str], &str) = match profile {
        "AMD_GFX1201" => (&["gfx1200", "gfx1201"], "gfx120X-all"),
        "AMD_GFX110X" => (&["gfx1100", "gfx1101", "gfx1102", "gfx1103"], "gfx110X-all"),
        "AMD_GFX1151" if g.contains("890M") || g.contains("PHOENIX") || g.contains("1150") => (&["gfx1150"], "gfx1150"),
        "AMD_GFX1151" => (&["gfx1150", "gfx1151"], "gfx1151"),
        "AMD_GFX103X" => (&["gfx1030", "gfx1031", "gfx1032", "gfx1033", "gfx1034", "gfx1035", "gfx1036"], "gfx103X-dgpu"),
        _ => return None,
    };
    let mut primary = format!("--pre torch=={PIN_TORCH} torchvision=={PIN_VISION} torchaudio=={PIN_AUDIO}");
    for t in targets {
        primary.push_str(&format!(" amd-torch-device-{t}=={PIN_TORCH} amd-torchvision-device-{t}=={PIN_VISION}"));
    }
    primary.push_str(&format!(" --index-url {MULTI}"));
    let fallback = format!("--pre torch torchvision torchaudio --index-url {STAGE}/{fam}/");
    Some((primary, fallback))
}

/// Patch the CLONED setup_config.json's rocm65.win torch command to our
/// TheRock command (exact-pinned 7.15 primary, staging float fallback).
/// Upstream's entry is stale gfx110x-only 6.5-era wheels. setup.py runs
/// `{pip} {torch_cmd}` where pip already ends in `install`, so the callers
/// pass a complete flags + packages + index-URL string. Re-applied every
/// install (a repo update restores upstream's file) and logged, so drift
/// is visible.
fn patch_therock_torch_cmd(repo: &std::path::Path, torch_cmd: &str) -> Result<(), String> {
    let path = repo.join("setup_config.json");
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("setup_config.json unreadable: {e}"))?;
    let mut cfg: serde_json::Value = serde_json::from_str(&raw).map_err(|e| format!("setup_config.json invalid: {e}"))?;
    let cmd = torch_cmd.to_string();
    match cfg.get_mut("components").and_then(|c| c.get_mut("torch")).and_then(|t| t.get_mut("rocm65")).and_then(|r| r.get_mut("cmd")).and_then(|c| c.get_mut("win")) {
        Some(slot) => { *slot = serde_json::Value::String(cmd); }
        None => return Err("setup_config.json has no components.torch.rocm65.cmd.win — upstream schema changed".into()),
    }
    std::fs::write(&path, serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?).map_err(|e| format!("setup_config.json unwritable: {e}"))?;
    Ok(())
}

/// setup.py profiles the launcher may force via WAN2GP_TAURI_GPU_PROFILE.
/// Allowlist = keys that exist in upstream setup_config.json gpu_profiles
/// (verified 2026-09-07): anything else (AMD_GFX103X, INTEL_XPU, CPU) leaves
/// setup.py's own detection alone — forcing an unknown key would KeyError.
/// Our keys already match upstream's for these families (and ours are more
/// correct: upstream matches bare "50" anywhere, so a GTX 1050 reads as
/// RTX_50 there). Pure + unit-tested.
pub(crate) fn setup_py_forced_profile(plan_profile: &str) -> Option<&'static str> {
    match plan_profile {
        "RTX_50" | "RTX_40" | "RTX_30" | "RTX_20" | "GTX_10"
        | "AMD_GFX110X" | "AMD_GFX1151" | "AMD_GFX1201" | "MPS" => Some(match plan_profile {
            "RTX_50" => "RTX_50", "RTX_40" => "RTX_40", "RTX_30" => "RTX_30",
            "RTX_20" => "RTX_20", "GTX_10" => "GTX_10",
            "AMD_GFX110X" => "AMD_GFX110X", "AMD_GFX1151" => "AMD_GFX1151",
            "AMD_GFX1201" => "AMD_GFX1201", _ => "MPS",
        }),
        _ => None,
    }
}

/// Clear setup.py child env (process-scoped like the py-shim PATH prepend —
/// set before the setup.py child spawns, cleared after setup so later
/// launches never inherit a stale verdict). Covers the WAN2GP_TAURI_*
/// overrides plus PYTHONUNBUFFERED (log ordering, below).
fn clear_setup_child_env() {
    std::env::remove_var("WAN2GP_TAURI_GPU_PROFILE");
    std::env::remove_var("WAN2GP_TAURI_VRAM_GB");
    std::env::remove_var("PYTHONUNBUFFERED");
}

const SETUP_PY_OVERRIDE_MARKER: &str = "# Launcher override (Tauri installer)";

/// Patch the CLONED setup.py so its `--auto` run uses the launcher's
/// hardware verdict instead of its own:
/// (a) profile override — setup.py detects via wmic.exe, which is REMOVED
/// on current Windows 11 (→ Unknown → RTX_40 → full CUDA stack on AMD
/// boxes; the 0.5.1 R9700 report), and its AMD name table misses RDNA 4
/// PRO cards (R9700/9070 match no token → AMD_GFX110X default). The patch
/// honors WAN2GP_TAURI_GPU_PROFILE when it names a real setup_config.json
/// profile key, and says so in the log.
/// (b) VRAM override — setup.py reads VRAM via nvidia-smi ONLY (→ 8GB
/// default → wrong quality profile id on 32GB AMD cards). The patch honors
/// WAN2GP_TAURI_VRAM_GB (GB, from our WMI/registry/known-card detection).
/// Idempotent (marker check — re-applied every install since a repo update
/// restores upstream's file). Returns Err naming the missed anchor when
/// upstream's source drifts — caller logs and runs setup.py unpatched.
fn patch_setup_py_overrides(repo: &std::path::Path) -> Result<(), String> {
    let path = repo.join("setup.py");
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("setup.py unreadable: {e}"))?;
    if raw.contains(SETUP_PY_OVERRIDE_MARKER) { return Ok(()); }
    // git on Windows often checks out CRLF — match on normalized LF, then
    // restore the original endings so the diff stays minimal.
    let had_crlf = raw.contains("\r\n");
    let hay: String = if had_crlf { raw.replace("\r\n", "\n") } else { raw.clone() };
    const PROFILE_ANCHOR: &str = "    gpu_name, vendor = get_gpu_info()\n    profile_key = get_profile_key(gpu_name, vendor)\n    profile = cfg['gpu_profiles'][profile_key]";
    const PROFILE_PATCH: &str = "    gpu_name, vendor = get_gpu_info()\n    profile_key = get_profile_key(gpu_name, vendor)\n    # Launcher override (Tauri installer): setup.py's own detection uses\n    # wmic.exe (removed on current Windows 11) and its AMD name table\n    # misses RDNA 4 PRO cards — the launcher passes its setup_config.json\n    # profile key via WAN2GP_TAURI_GPU_PROFILE.\n    _forced = os.environ.get(\"WAN2GP_TAURI_GPU_PROFILE\", \"\").strip()\n    if _forced:\n        if _forced in cfg.get('gpu_profiles', {}):\n            print(f\"[*] Launcher-forced GPU profile: {_forced} (setup.py auto-detect said {profile_key})\")\n            profile_key = _forced\n        else:\n            print(f\"[!] Ignoring unknown launcher profile {_forced!r} (setup.py auto-detect said {profile_key})\")\n    profile = cfg['gpu_profiles'][profile_key]";
    const VRAM_ANCHOR: &str = "    except:\n        print(\"[!] Warning: Could not detect VRAM via nvidia-smi. Defaulting to 8GB.\")\n        vram_gb = 8";
    const VRAM_PATCH: &str = "    except:\n        # Launcher override (Tauri installer): AMD/Intel boxes have no\n        # nvidia-smi — the launcher passes its WMI/registry VRAM figure (GB).\n        _lvram = os.environ.get(\"WAN2GP_TAURI_VRAM_GB\", \"\").strip()\n        try:\n            vram_gb = float(_lvram)\n            print(f\"[*] Launcher-provided VRAM: {vram_gb:g}GB (no nvidia-smi on this box)\")\n        except:\n            print(\"[!] Warning: Could not detect VRAM via nvidia-smi. Defaulting to 8GB.\")\n            vram_gb = 8";
    let mut patched = hay;
    let mut missed: Vec<&str> = Vec::new();
    if patched.contains(PROFILE_ANCHOR) { patched = patched.replacen(PROFILE_ANCHOR, PROFILE_PATCH, 1); }
    else { missed.push("profile (__main__ gpu_name/profile_key block)"); }
    if patched.contains(VRAM_ANCHOR) { patched = patched.replacen(VRAM_ANCHOR, VRAM_PATCH, 1); }
    else { missed.push("vram (get_system_specs nvidia-smi fallback)"); }
    if !missed.is_empty() { return Err(format!("anchors missed: {}", missed.join(", ")) ); }
    // Sanity: patched file must still parse (never ship a SyntaxError
    // into a 20-minute install). python may be absent — skip then.
    // Check the normalized text through a temp file so CRLF originals
    // don't matter here either.
    let check_src = patched.clone();
    let check_ok = (|| -> bool {
        // Unique per call — tests patch in parallel in one process.
        static CHECK_N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = CHECK_N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = std::env::temp_dir().join(format!("wgp-setup-py-check-{}-{n}.py", std::process::id()));
        if std::fs::write(&tmp, check_src).is_err() { return true; }
        let arg = tmp.to_string_lossy().to_string();
        let py = tool_path("python");
        let ok = silent_command(py.as_str()).args(["-c", "import ast,sys; ast.parse(open(sys.argv[1]).read())", &arg]).output()
            .map(|o| o.status.success()).unwrap_or(true);
        let _ = std::fs::remove_file(&tmp);
        ok
    })();
    if !check_ok { return Err("patched setup.py failed ast.parse — upstream drift, refusing to write".into()); }
    let out = if had_crlf { patched.replace('\n', "\r\n") } else { patched };
    std::fs::write(&path, out).map_err(|e| format!("setup.py unwritable: {e}"))?;
    Ok(())
}

/// Pre-install detection gate: fail fast on missing tooling / missing
/// vendor driver, and report healable state BEFORE any download (the 0.5.x
/// AMD saga was all discovered after 20-minute installs). Returns (fatal,
/// checks); levels ok/info/warn/fail. WMI/registry reads stay in hw —
/// this only assembles verdicts, so the pure parts are unit-testable.
pub(crate) struct PreflightCheck { pub id: &'static str, pub level: &'static str, pub msg: String }

pub(crate) fn run_preflight_checks(repo: &std::path::Path, hw: &serde_json::Value, plan: &serde_json::Value) -> (bool, Vec<PreflightCheck>) {
    let mut checks: Vec<PreflightCheck> = Vec::new();
    let mut fatal = false;
    let vendor = hw.get("vendor").and_then(|v| v.as_str()).unwrap_or("");
    let name = hw.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let profile = plan.get("profile").and_then(|v| v.as_str()).unwrap_or("");
    // 1. git — the clone needs it; without it setup.py dies pages later.
    match silent_command(tool_path("git").as_str()).args(["--version"]).output() {
        Ok(o) if o.status.success() => checks.push(PreflightCheck { id: "git", level: "ok", msg: format!("git present ({})", String::from_utf8_lossy(&o.stdout).trim()) }),
        _ => { fatal = true; checks.push(PreflightCheck { id: "git", level: "fail", msg: "git not found — install it (`winget install Git.Git`), then retry.".into() }); }
    }
    // 2. display driver — Basic Adapter means no vendor driver at all.
    if name.to_lowercase().contains("basic display") {
        fatal = true;
        checks.push(PreflightCheck { id: "driver", level: "fail", msg: "Microsoft Basic Display Adapter is active — no vendor GPU driver. Install the AMD/NVIDIA driver first, reboot, then retry.".into() });
    } else if vendor == "AMD" {
        let ver = hw.get("driverVersion").and_then(|v| v.as_str()).unwrap_or("");
        match classify_amd_driver(ver) {
            "ok" => checks.push(PreflightCheck { id: "driver", level: "ok", msg: format!("AMD driver {ver} (24.x era)") }),
            "old" => checks.push(PreflightCheck { id: "driver", level: "warn", msg: format!("AMD driver {ver} predates 2024 — TheRock wants Adrenalin/Pro >= 24.5; update to avoid kernel surprises.") }),
            _ => checks.push(PreflightCheck { id: "driver", level: "warn", msg: "could not read the AMD driver version — if the install misbehaves, update Adrenalin/Pro first.".into() }),
        }
    }
    // 3. several AMD controllers (dGPU + iGPU laptops): first-wins order
    // is firmware-dependent — make the pick visible.
    #[cfg(windows)] {
        let amds: Vec<String> = wmi_all_gpus().into_iter().filter(|(_, v, _, _)| v == "AMD").map(|(n, _, _, _)| n).collect();
        if amds.len() > 1 {
            checks.push(PreflightCheck { id: "multi-gpu", level: "warn", msg: format!("{} AMD GPUs visible ({}); using {} — order is firmware-dependent, confirm it picked your dGPU.", amds.len(), amds.join(" + "), name) });
        }
    }
    // 4. unreadable VRAM mistiers the quality profile.
    if vendor == "AMD" && plan.get("vramGb").and_then(|v| v.as_f64()).unwrap_or(0.0) < 2048.0 {
        checks.push(PreflightCheck { id: "vram", level: "warn", msg: "VRAM unreadable on this AMD card — quality profile may mistier; check the generated wgp_config profiles after install.".into() });
    }
    // 5. stale completion marker (removed/failed env) — install rebuilds.
    if let Ok(s) = std::fs::read_to_string(repo.join(".wan2gp-install-ok")) {
        let env_dir = match s.split_whitespace().next().unwrap_or("") {
            "uv" => repo.join("env_uv"), "venv" => repo.join("env_venv"), "conda" => repo.join("env_conda"), _ => repo.join(".none"),
        };
        #[cfg(windows)] let py_gone = !env_dir.join("Scripts\\python.exe").exists();
        #[cfg(not(windows))] let py_gone = !env_dir.join("bin/python").exists() && !env_dir.join("bin/python3").exists();
        if py_gone {
            checks.push(PreflightCheck { id: "stale-marker", level: "info", msg: "stale install marker points at a removed env — installing fresh (models/settings kept).".into() });
        }
    }
    // 6. CUDA-era config on an AMD box — setup.py regenerates it.
    if profile.starts_with("AMD") {
        if let Ok(raw) = std::fs::read_to_string(repo.join("wgp_config.json")) {
            let stale = serde_json::from_str::<serde_json::Value>(&raw).ok()
                .and_then(|v| v.get("attention_mode").and_then(|a| a.as_str()).map(str::to_string))
                .map_or(false, |a| a.starts_with("sage"));
            if stale {
                checks.push(PreflightCheck { id: "stale-config", level: "info", msg: "stale CUDA-era wgp_config.json on an AMD box — it will be regenerated.".into() });
            }
        }
        // 7. recorded HSA mode from a previous probe.
        if let Some(c) = crate::amd::read_hsa_choice(repo) {
            checks.push(PreflightCheck { id: "hsa", level: "info", msg: format!("launch will use recorded HSA mode: {c:?}") });
        }
    }
    // 8. Defender exclusion — warned per-vendor (see av_exclusion_msg):
    // read-only query, skip silently when unavailable.
    #[cfg(windows)] {
        if av_exclusion_msg(vendor).is_some() {
        if let Ok(o) = silent_command("powershell").args(["-NoProfile", "-Command", "(Get-MpPreference).ExclusionPath -join \"`n\""]).output() {
            if o.status.success() {
                let rl = repo.to_string_lossy().to_lowercase();
                let covered = String::from_utf8_lossy(&o.stdout).lines().any(|l| {
                    let el = l.trim().trim_end_matches('\\').to_lowercase();
                    !el.is_empty() && (rl == el || rl.starts_with(&format!("{el}\\")))
                });
                checks.push(if covered {
                    PreflightCheck { id: "av", level: "ok", msg: "Defender exclusion covers the install folder.".into() }
                } else {
                    PreflightCheck { id: "av", level: "warn", msg: av_exclusion_msg(vendor).unwrap_or("no Defender exclusion for the install folder.").into() }
                });
            }
        }
        }
    }
    (fatal, checks)
}

/// Defender-exclusion warning by vendor. Proactive warnings need observed
/// risk, not plausible risk: AMD nightlies were quarantined on a real box
/// (hourly CI builds, zero reputation). NVIDIA wheels are unsigned too
/// but have no observed case — they stay on the reactive missing-DLL hint
/// (all vendors, fires on actual damage), not a proactive warn.
/// PyPI-stable CPU installs skip the noise entirely. Pure + tested.
pub(crate) fn av_exclusion_msg(vendor: &str) -> Option<&'static str> {
    match vendor {
        "AMD" => Some("no Defender exclusion for the install folder — nightly DLLs are quarantined heuristically; consider adding one."),
        _ => None,
    }
}

#[tauri::command]
pub async fn preflight_check() -> Result<serde_json::Value, String> {
    // Read-only (no mutating guard): future UI can run this any time.
    let repo = get_repo_dir();
    let gpu = get_gpu_info_sync();
    let plan = build_install_plan(&gpu);
    let (fatal, checks) = run_preflight_checks(&repo, &gpu, &plan);
    Ok(serde_json::json!({
        "ok": !fatal,
        "profile": plan.get("profile"),
        "checks": checks.iter().map(|c| serde_json::json!({"id": c.id, "level": c.level, "msg": c.msg})).collect::<Vec<_>>(),
    }))
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
    // Pre-install detection gate: fail fast (no git, no vendor driver)
    // and report healable state BEFORE any download. A fatal finding
    // aborts here — nothing was downloaded, nothing was touched.
    {
        let (fatal, checks) = run_preflight_checks(&repo, &gpu, &plan);
        for c in &checks {
            let tag = match c.level { "ok" => "[✓]", "info" => "[i]", "fail" => "[!]", _ => "[!]" };
            emit(&format!("{tag} preflight {}: {}\n", c.id, c.msg));
        }
        if fatal {
            let reasons: Vec<&str> = checks.iter().filter(|c| c.level == "fail").map(|c| c.msg.as_str()).collect();
            mutating_done();
            return Err(format!("Pre-install check failed: {}", reasons.join(" ")));
        }
    }
    let emit_phase = |id: &str, label: &str, done: bool| { let _ = app.emit("setup-phase", serde_json::json!({"id": id, "label": label, "done": done})); };
    // Never install/repair inside a Pinokio-managed tree (own lifecycle + env).
    // Fresh-install elsewhere and point the model folders at its library.
    if let Some(where_) = pinokio_root(&repo) {
        mutating_done();
        return Err(format!("This folder is Pinokio-managed ({}). Installing here would corrupt Pinokio's Wan2GP. Pick an empty folder and reuse Pinokio's ckpts/loras/outputs as your model folders — no re-downloads, Pinokio keeps working.", where_.display()));
    }
    // Fail fast on a full target drive: a complete install needs tens of GB
    // (env alone is ~10 GB before models). Dying of ENOSPC 15 minutes into
    // setup.py helps nobody — abort here with a copy-paste fix instead.
    if let Some((free, _)) = crate::config::disk_for_path(&repo.to_string_lossy()) {
        let free_gb = free as f64 / 1073741824.0;
        if free_gb < 10.0 {
            mutating_done();
            return Err(format!("Only {free_gb:.1} GB free on the install drive ({}). A full install needs 50+ GB (env alone is ~10 GB before models). Free space or pick another folder, then retry — nothing was downloaded.", repo.display()));
        }
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
            let git = tool_path("git");
            let (mut rx, _child) = app.shell().command(git.as_str()).args(["clone","--depth","1","https://github.com/deepbeepmeep/Wan2GP.git", &tmp.to_string_lossy()]).spawn().map_err(|e| e.to_string())?;;
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
            let (mut rx, _child) = app.shell().command(tool_path("git")).args(["clone","--depth","1","https://github.com/deepbeepmeep/Wan2GP.git", &repo.to_string_lossy()]).spawn().map_err(|e| e.to_string())?;
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
    // Completion marker — written only after the smoke test passes. setup.py
    // cannot resume into an existing env dir (`uv venv` refuses it), so a
    // Retry after ANY failed/interrupted run must start the env clean, while
    // a finished install must never re-download. The old stale/broken check
    // missed the common case (healthy python, torch never installed) and
    // retried straight into the venv-exists crash. Marker separates the two.
    let marker = repo.join(".wan2gp-install-ok");
    // Resume/reuse probe: all layouts (uv/venv Scripts|bin, conda root).
    // Legacy default when nothing resolves yet (fresh path — probe is free).
    #[cfg(windows)] let legacy_py = env_path.join("Scripts\\python.exe");
    #[cfg(not(windows))] let legacy_py = env_path.join("bin/python");
    let py_exe = resolve_env_python(&repo, &env_path.to_string_lossy()).unwrap_or(legacy_py);
    let marked_same_env = std::fs::read_to_string(&marker).ok()
        .is_some_and(|s| s.split_whitespace().next() == Some(env.as_str()));
    // torch_probe is free when py_exe is missing (no spawn) — the fresh path.
    if marked_same_env || torch_probe(&py_exe).is_some() {
        // Finished (or legacy/partial with working torch): verify instead of
        // re-downloading — setup.py would refuse the existing dir anyway.
        match smoke_verify(&py_exe, &repo) {
            Ok(line) => {
                emit(&format!("[*] Previous install verified ({line}) — skipping re-download…\n"));
                let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                let _ = std::fs::write(&marker, format!("{env} {stamp}"));
                mutating_done();
                return Ok(serde_json::json!({"ok": true, "success": true, "reused": true}));
            }
            Err(e) if e.contains("cuda=False") => {
                // Torch present but GPU invisible: reinstalling won't fix a driver gap.
                mutating_done();
                return Err(format!("Previous install found but {e}"));
            }
            Err(e) => {
                // Missing-DLL shape (WinError 126 / 0xc0000135) almost always
                // means antivirus quarantine — say so explicitly instead of
                // a generic "damaged".
                let mut msg = String::from("[!] Previous env is damaged (torch won't import) — rebuilding clean…\n");
                let low = e.to_lowercase();
                if low.contains("dll") || low.contains("winerror 126") || low.contains("0xc0000135") || low.contains("error_mod_not_found") {
                    msg.push_str("[!] Missing-DLL shape: check antivirus quarantine and add an exclusion for the install folder, or it will eat the rebuild too.\n");
                }
                emit(&msg);
                let _ = std::fs::remove_file(&marker);
                let _ = std::fs::remove_dir_all(&env_path);
            }
        }
    } else if env_path.exists() {
        emit("[*] Removing incomplete env from a failed/interrupted install (setup.py can't resume into it)…\n");
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
    // venv mode on Windows needs `py -3.11` (Electron parity: isolated shim
    // instead of a global Python install). Without a launcher, setup.py dies
    // looking for it even when a perfect uv-managed copy exists.
    #[cfg(windows)]
    let mut saved_path: Option<String> = None;
    #[cfg(windows)]
    if env == "venv" {
        let shim_ok = silent_command(tool_path("py").as_str()).args(["-3.11", "-c", "import sys"]).output().is_ok_and(|o| o.status.success());
        if !shim_ok {
            let wanted = pinned_python_wanted();
            match ensure_uv_python(&app, &emit, &wanted).await {
                Ok(uvpy) => {
                    let shim_dir = get_data_dir().join(".py-shim");
                    let _ = std::fs::create_dir_all(&shim_dir);
                    // Batch shim: strips the -3.11 selector, delegates the rest.
                    let shim = format!("@echo off\r\nsetlocal enabledelayedexpansion\r\nset \"args=%*\"\r\nset \"args=!args:-3.11 =!\"\r\nif \"!args!\"==\"%*\" set \"args=!args:-3.11=!\"\r\n\"{uvpy}\" !args!\r\nexit /b %errorlevel%\r\n");
                    let _ = std::fs::write(shim_dir.join("py.cmd"), &shim);
                    let _ = std::fs::write(shim_dir.join("py.bat"), &shim);
                    let old = std::env::var("PATH").unwrap_or_default();
                    let add = shim_dir.to_string_lossy().to_string();
                    if !old.split(';').any(|p| p.eq_ignore_ascii_case(&add)) {
                        std::env::set_var("PATH", format!("{add};{old}"));
                    }
                    saved_path = Some(old);
                    if silent_command(tool_path("py").as_str()).args(["-3.11", "-c", "import sys"]).output().is_ok_and(|o| o.status.success()) {
                        emit("[*] py launcher shim ready (isolated Python, no global install)\n");
                    } else {
                        emit("[!] py shim created but `py -3.11` still not found — venv setup may fail; install Python 3.11 (Download button) or use the uv env type.\n");
                    }
                }
                Err(e) => { mutating_done(); return Err(e); }
            }
        }
    }
    // AMD TheRock torch (doc-leading): patch the cloned setup_config.json so
    // AMD TheRock torch (exact-pinned ROCm 7.15 primary): patch the cloned
    // setup_config.json so setup.py's own [2/3] torch step installs the
    // verified 7.15 stack instead of the stale gfx110x-only wheels.
    // NVIDIA path untouched.
    let amd_cmds = if plan["profile"].as_str().unwrap_or("").starts_with("AMD") {
        amd_therock_torch_cmds(plan["profile"].as_str().unwrap_or(""), plan["gpuName"].as_str().unwrap_or(""))
    } else { None };
    if let Some((primary, _)) = &amd_cmds {
        match patch_therock_torch_cmd(&repo, primary) {
            Ok(()) => emit(&format!("[*] AMD TheRock torch: exact-pinned ROCm 7.15 (torch 2.12.0+rocm7.15.0a20260728, whl-multi-arch)\n{primary}\n")),
            Err(e) => emit(&format!("[!] AMD torch patch skipped ({e}) — setup.py will use upstream's entry.\n")),
        }
    }
    // setup.py override patch (0.5.2): setup.py --auto re-detects the GPU
    // itself via wmic.exe (removed on current Windows 11 → Unknown →
    // RTX_40 → CUDA stack on AMD boxes) and reads VRAM via nvidia-smi
    // only (→ 8GB default). Patch the CLONED setup.py to honor our
    // verdict through WAN2GP_TAURI_* env vars (validated inside setup.py).
    // Attempted on every install (a repo update restores upstream's file);
    // a missed anchor only logs — setup.py runs as before.
    match patch_setup_py_overrides(&repo) {
        Ok(()) => emit("[*] setup.py launcher-override patch ready (profile + VRAM via WAN2GP_TAURI_*).\n"),
        Err(e) => emit(&format!("[!] setup.py override patch skipped ({e}) — setup.py will use its own detection.\n")),
    }
    // Stale wgp_config.json from the 0.5.1 failure mode (CUDA-era attention
    // sage/sage2 written for an AMD box): setup.py's create_wgp_config
    // early-returns when the file exists, so a stale file would pin the
    // wrong attention forever. AMD-correct is "" — remove only sage*,
    // keep anything else (user-tuned or already-AMD). restore_backup()
    // only restores when the repo file is ABSENT, so this can't clobber.
    if plan["profile"].as_str().unwrap_or("").starts_with("AMD") {
        let cfg_path = repo.join("wgp_config.json");
        if let Ok(raw) = std::fs::read_to_string(&cfg_path) {
            let stale = serde_json::from_str::<serde_json::Value>(&raw).ok()
                .and_then(|v| v.get("attention_mode").and_then(|a| a.as_str()).map(str::to_string))
                .map_or(false, |a| a.starts_with("sage"));
            if stale {
                let _ = std::fs::remove_file(&cfg_path);
                emit("[*] Removed stale wgp_config.json (CUDA-era attention on an AMD box) — setup.py will regenerate it.\n");
            }
        }
    }
    // Env for the setup.py child below: our profile key (allowlisted —
    // unknown keys unset so setup.py falls back to its own detection)
    // plus our VRAM figure in GB (setup.py only knows nvidia-smi).
    // Process-scoped like the py-shim PATH prepend; cleared after setup.
    if let Some(key) = setup_py_forced_profile(plan["profile"].as_str().unwrap_or("")) {
        std::env::set_var("WAN2GP_TAURI_GPU_PROFILE", key);
        emit(&format!("[*] setup.py will run with forced GPU profile: {key}\n"));
    } else {
        std::env::remove_var("WAN2GP_TAURI_GPU_PROFILE");
    }
    match plan.get("vramGb").and_then(|v| v.as_f64()) {
        Some(mb) if mb >= 2048.0 => {
            std::env::set_var("WAN2GP_TAURI_VRAM_GB", format!("{}", (mb / 1024.0).round() as u64));
            emit(&format!("[*] setup.py will run with launcher VRAM: {}GB\n", (mb / 1024.0).round() as u64));
        }
        _ => std::env::remove_var("WAN2GP_TAURI_VRAM_GB"),
    }
    // Unbuffered setup.py stdout: its prints share the pipe with uv's
    // stderr (uv writes human output to stderr, live). Buffered, setup.py's
    // own lines ([1/3], `>>> Running:`) arrive in one late dump AFTER uv's
    // output — the Intel 0.5.3 log read as if setup.py ran twice. Cleared
    // with the overrides above when setup ends.
    std::env::set_var("PYTHONUNBUFFERED", "1");
    // run setup.py with the env's python (hardware-aware: setup.py reads setup_config.json + GPU)
    {
        let (py, args): (String, Vec<String>) = if env.as_str() == "conda" { (tool_path("conda"), vec!["run".into(), "-p".into(), env_path.to_string_lossy().to_string(), "python".into(), "setup.py".into(), "install".into(), "--env".into(), env.clone(), "--auto".into()]) } else {
            let p = if env=="uv" { env_path.join(if cfg!(windows){"Scripts\\python.exe"} else {"bin/python"}) } else { env_path.join(if cfg!(windows){"Scripts\\python.exe"} else {"bin/python3"}) };
            // Missing env interpreter + uv env → run setup.py via `uv run`
            // (uv provisions Python itself). uv_command() is a path or bare
            // "uv" fallback — track the branch explicitly, not by string.
            let (py_bin, via_uv_run) = if p.exists() { (p.to_string_lossy().to_string(), false) } else if env=="uv" { (uv_command(), true) } else { ("python".into(), false) };
            if via_uv_run { (py_bin, vec!["run".into(), "--with".into(), "setuptools".into(), "python".into(), "setup.py".into(), "install".into(), "--env".into(), env.clone(), "--auto".into()]) }
            else { (py_bin, vec!["setup.py".into(), "install".into(), "--env".into(), env.clone(), "--auto".into()]) }
        };
        // setup.py gets up to 2 attempts: a transient network death (the common
        // flake — CDN timeout after minutes of downloading) retries once
        // automatically instead of sending the user back to the button.
        // Anything else fails immediately; no silent third tries.
        let mut tail = String::new();
        for attempt in 1..=2 {
            tail.clear();
            // Fresh phase tracking per attempt so a retry replays the phases
            // instead of showing attempt-1's done states.
            let mut phases = std::collections::HashSet::new();
            let mut phases_done = std::collections::HashSet::new();
            let mut do_phase = |id: &str, label: &str| { if phases.insert(id.to_string()) { let _ = app.emit("setup-phase", serde_json::json!({"id": id, "label": label, "done": false})); } };
            let mut done_phase = |id: &str, label: &str| { if phases_done.insert(id.to_string()) { let _ = app.emit("setup-phase", serde_json::json!({"id": id, "label": label, "done": true})); } };
            if attempt == 2 {
                emit("[*] Retrying setup.py (attempt 2 of 2)…\n");
                // AMD: retry falls back to the community staging float
                // (setup_config.json lives in the repo root,
                // not the cleared env dir — re-patch here so [2/3] uses it).
                // The numpy pin below keys off the installed torch build,
                // so a retry that replaced 7.15 with the staging float
                // still gets its 1.26.4 pin.
                if let Some((_, staging)) = &amd_cmds {
                    match patch_therock_torch_cmd(&repo, staging) {
                        Ok(()) => emit(&format!("[*] AMD retry via staging float: {staging}\n")),
                        Err(e) => emit(&format!("[!] AMD staging patch skipped ({e}).\n")),
                    }
                }
            }
            let (mut rx, _child) = match app.shell().command(&py).args(args.clone()).current_dir(&repo).spawn() {
                Ok(t) => t,
                Err(e) => {
                    #[cfg(windows)]
                    if let Some(old) = saved_path.clone() { std::env::set_var("PATH", old); }
                    clear_setup_child_env();
                    mutating_done();
                    return Err(format!("setup.py failed to start ({e})"));
                }
            };
            // Sliding window: shell chunks split anywhere, so markers spanning a
            // boundary ("[2/3]", "+ torch==…") are matched against the tail.
            let mut window = String::new();
            let mut exit_code: Option<i32> = None;
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
            if code == 0 { break; }
            // setup.py failed — report honestly, no false "Installation complete!".
            let hint = install_failure_hint(&tail);
            emit(&format!("[!] setup.py exited with code {code} (attempt {attempt} of 2).\n[!] {hint}\n"));
            if attempt == 1 && is_network_failure(&tail) {
                emit("[*] Transient network failure — retrying setup.py once automatically (uv cache makes the re-fetch fast)…\n");
                // setup.py can't resume into the half-built env — clear it first.
                let _ = std::fs::remove_dir_all(&env_path);
                continue;
            }
            // Restore PATH before returning (shim prepend is install-scoped).
            #[cfg(windows)]
            if let Some(old) = saved_path.clone() { std::env::set_var("PATH", old); }
            clear_setup_child_env();
            mutating_done();
            return Err(format!("Install failed (setup.py exited code {code}). {hint}"));
        } // end for attempt — success broke out; all failures returned above
        clear_setup_child_env();
        for (id, label) in [("venv", "Create Python virtual environment"), ("torch", "Install PyTorch + CUDA"), ("reqs", "Install Python dependencies"), ("triton", "Install Triton compiler"), ("sage", "Install Sage Attention kernel"), ("flash", "Install Flash Attention"), ("kernels", "Install GPU kernels (nunchaku/GGUF)")] {
            let _ = app.emit("setup-phase", serde_json::json!({"id": id, "label": label, "done": true}));
        }
        // Restore PATH now that setup is done (shim prepend is install-scoped).
        #[cfg(windows)]
        if let Some(old) = saved_path.clone() { std::env::set_var("PATH", old); }
        emit_phase("done", "Finalize installation", true);
    }
    // Post-install smoke test: exit 0 from setup.py is not proof the env works
    // (ATFGriff got exit 2 AND a success message; subtler breakage can exit 0).
    // Gate favourite-plugins + "Installation complete!" on torch importing
    // and the GPU being visible from inside the new env. All env types:
    // conda's interpreter lives at the env root (no Scripts dir).
    if env == "uv" || env == "venv" || env == "conda" {
        let Some(smoke_py) = resolve_env_python(&repo, &env_path.to_string_lossy()) else {
            mutating_done();
            return Err(format!("Install finished but no Python interpreter found in {} — the environment is broken. Retry the install (the broken env is removed automatically) or report it with Copy diagnostics.", env_path.display()));
        };
        emit("[*] Verifying install: importing torch in the new environment…\n");
        match smoke_verify(&smoke_py, &repo) {
            Ok(line) => emit(&format!("[✓] Smoke test passed: {line}\n")),
            Err(e) if e.contains("cuda=False") => {
                mutating_done();
                return Err(format!("Install finished but {e}"));
            }
            Err(_) => {
                mutating_done();
                return Err(format!("Install finished but `import torch` fails in {} — the environment is broken. Retry the install (the broken env is removed automatically) or report it with Copy diagnostics.", smoke_py.display()));
            }
        }
        // AMD GPU compute probe: import-torch is not proof the stack runs
        // (0.5.2 imported fine, first int8 GEMM died with hipErrorInvalidValue
        // on gfx1201). Probe the installed torch in both HSA modes (override
        // from setup_config, then native); on double failure re-seat torch to
        // the staging float (the only known-good lineage on gfx1201) and
        // probe again. Records the winning HSA mode for launch. Any
        // remaining failure is honest (no marker, no false success).
        if amd_cmds.is_some() {
            let profile = plan["profile"].as_str().unwrap_or("");
            let hsa_ver = crate::amd::profile_hsa_version(&repo, profile);
            // Override first: matches current launch behavior, so a pass
            // changes nothing for already-working rigs.
            let mut modes: Vec<(&str, Option<String>)> = Vec::new();
            if let Some(v) = hsa_ver { modes.push(("override", Some(v))); }
            modes.push(("native", None));
            let mut winner: Option<crate::amd::HsaChoice> = None;
            let mut last_err = String::new();
            let mut probe_modes = |winner: &mut Option<crate::amd::HsaChoice>, last_err: &mut String, emit: &dyn Fn(&str)| {
                for (label, hsa) in &modes {
                    emit(&format!("[*] GPU compute probe (HSA {label}{})…\n", hsa.as_ref().map(|v| format!("={v}")).unwrap_or_default()));
                    match crate::amd::run_compute_probe(&smoke_py, hsa.as_deref()) {
                        Ok(p) => {
                            emit(&format!("[✓] GPU compute passed (HSA {label}): torch {} on {}.\n", p.torch, p.device));
                            *winner = Some(match hsa { Some(v) => crate::amd::HsaChoice::Override(v.clone()), None => crate::amd::HsaChoice::Native });
                            break;
                        }
                        Err(e) => { emit(&format!("[!] GPU compute probe (HSA {label}) failed: {e}\n")); *last_err = e; }
                    }
                }
            };
            probe_modes(&mut winner, &mut last_err, &emit);
            if winner.is_none() && (env == "uv" || env == "venv") {
                if let Some((_, staging)) = &amd_cmds {
                    emit("[*] Installed ROCm torch fails compute in both HSA modes — re-seating torch to the staging float…\n");
                    let (prog, mut args): (String, Vec<String>) = if env == "uv" {
                        (uv_command(), vec!["pip".into(), "install".into(), "--index-strategy".into(), "unsafe-best-match".into(), "--python".into(), env_path.to_string_lossy().to_string()])
                    } else {
                        #[cfg(windows)] let py = env_path.join("Scripts\\python.exe");
                        #[cfg(not(windows))] let py = env_path.join("bin/python3");
                        (py.to_string_lossy().to_string(), vec!["-m".into(), "pip".into(), "install".into()])
                    };
                    args.extend(staging.split_whitespace().map(str::to_string));
                    match silent_command(&prog).args(&args).current_dir(&repo).output() {
                        Ok(o) if o.status.success() => {
                            emit("[✓] Staging torch seated — probing again…\n");
                            probe_modes(&mut winner, &mut last_err, &emit);
                        }
                        Ok(o) => {
                            let tail: String = String::from_utf8_lossy(&o.stderr).chars().rev().take(400).collect::<String>().chars().rev().collect();
                            last_err = format!("staging re-seat failed: {tail}");
                            emit(&format!("[!] {last_err}\n"));
                        }
                        Err(e) => {
                            last_err = format!("staging re-seat spawn failed ({e})");
                            emit(&format!("[!] {last_err}\n"));
                        }
                    }
                }
            }
            match winner {
                Some(c) => {
                    crate::amd::write_hsa_choice(&repo, &c);
                    emit(&format!("[i] HSA mode recorded for launch: {c:?}\n"));
                }
                None => {
                    clear_setup_child_env();
                    mutating_done();
                    return Err(format!("Install finished but GPU compute fails on every torch build + HSA mode. Last error: {last_err} Copy diagnostics (System → Troubleshooting) and report it — do not try generating until Verify passes."));
                }
            }
        }
        // AMD TheRock compat: the staging-float fallback path (community
        // recipe) wants the numpy 1.26.4 pin — requirements.txt may have
        // pulled numpy 2.x. The exact-pinned 7.15 primary resolves WITH
        // numpy 2.x (verified pip closure), so downgrading under it risks
        // breaking torch — skip the pin when torch reports a 7.15 build.
        // Warn-only: never turn a passing smoke test into a failure.
        if amd_cmds.is_some() {
            let torch_ver = silent_command(&smoke_py).args(["-c", "import torch; print(torch.__version__)"]).current_dir(&repo).output().ok()
                .and_then(|o| if o.status.success() { Some(String::from_utf8_lossy(&o.stdout).trim().to_string()) } else { None })
                .unwrap_or_default();
            if torch_ver.contains("rocm7.15") {
                emit(&format!("[*] AMD env: torch {torch_ver} (ROCm 7.15, numpy 2.x compatible) — skipping numpy pin.\n"));
            } else {
                emit("[*] AMD env: pinning numpy==1.26.4 for ROCm torch compat…\n");
                match silent_command(&smoke_py).args(["-m", "pip", "install", "numpy==1.26.4", "setuptools", "hf-xet"]).current_dir(&repo).output() {
                    Ok(o) if o.status.success() => emit("[✓] numpy pin applied.\n"),
                    Ok(o) => emit(&format!("[!] numpy pin exited {} — ROCm torch may want numpy==1.26.4 installed manually.\n", o.status.code().unwrap_or(-1))),
                    Err(e) => emit(&format!("[!] numpy pin spawn failed ({e}).\n")),
                }
            }
        }
    }
    // Completion marker — future Install calls verify instead of re-downloading.
    {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = std::fs::write(&marker, format!("{env} {stamp}"));
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
    for sub in ["plugins","finetunes","deepy_sessions"] { let s = repo.join(sub); if s.exists() { let d = backup.join(sub); let _ = silent_command("xcopy").args(["/E","/I", s.to_string_lossy().as_ref(), d.to_string_lossy().as_ref()]).output(); } }
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
    for sub in ["plugins", "finetunes", "deepy_sessions"] {
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
        let head = silent_command(tool_path("git").as_str()).args(["rev-parse","--short","HEAD"]).current_dir(&repo).output().ok().and_then(|o| if o.status.success() { Some(String::from_utf8_lossy(&o.stdout).trim().to_string()) } else { None }).unwrap_or("unknown".into());
        let gguf_url = cfg.get("components").and_then(|c| c.get("kernels")).and_then(|m| m.get("gguf")).and_then(|e| e.get("cmd")).and_then(|c| c.get("win")).and_then(|u| u.as_str()).unwrap_or("");
        // wheelDistVersion extract: <dist>-<version>-cp... -> version
        let gguf_ver = gguf_url.split('/').next_back().unwrap_or("").split(".whl").next().unwrap_or("").split('-').nth(1).unwrap_or("?");
        let v = if gguf_ver.is_empty() { "?" } else { gguf_ver };
        emit_log(&format!("[*] setup_config.json @ {head} (gguf {v}) — deepbeepmeep's wanted wheels\n"));
    }
    let gpu = get_gpu_info_sync(); let profile = kernel_profile_key(gpu.get("vendor").and_then(|v| v.as_str()).unwrap_or(""), gpu.get("name").and_then(|v| v.as_str()).unwrap_or(""));
    let kernels = cfg.get("gpu_profiles").and_then(|p| p.get(&profile)).and_then(|pr| pr.get("kernels")).and_then(|k| k.as_array()).cloned().unwrap_or_default();
    let sage_safe = load_config_value().get("sageSafe").and_then(serde_json::Value::as_bool) != Some(false); // ponytail: default safe post6 (1348e5b) — only false opts into upstream post4
    // ponytail: Sage wheel is not in gpu_profiles[RTX_30].kernels (only nunchaku+gguf) — handle it separately like Electron's setSageAttentionSafe
    let mut all_kernels = kernels.clone();
    if ["RTX_30","RTX_40","RTX_50"].contains(&profile.as_str()) {
        // ensure sage is in the sync list when toggling safe/upstream, so the wheel actually swaps
        if !all_kernels.iter().any(|k| k.as_str()==Some("sage") || k.as_str()==Some("sageattention")) {
            all_kernels.push(serde_json::json!("sage"));
        }
    }
    let mut failed: Vec<String> = Vec::new();
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
            // GGUF 1.0.21 override (docs prescription over setup_config lag).
            let url = apply_gguf_override(&url);
            let m = format!("[*] sync kernel {name}\n"); crate::base::push_log(&m, "setup"); let _ = app.emit("launch-log", m);
            let emit_k = |s: &str| { crate::base::push_log(s, "setup"); let _ = app.emit("launch-log", s.to_string()); };
            let py_s = py.to_string_lossy().to_string();
            if !run_logged(&app, &py_s, &["-m","pip","install", url.as_str(), "--upgrade"], None, emit_k).await {
                failed.push(name.to_string());
            }
        }
    }
    mutating_done();
    if !failed.is_empty() {
        return Err(format!("kernel sync failed for: {} — see console output", failed.join(", ")));
    }
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command]
pub async fn update(app: tauri::AppHandle) -> Result<serde_json::Value,String> {
    mutating_try("update")?;
    let repo = get_repo_dir();
    if !repo.join(".git").exists() { mutating_done(); return Err("not a git repo".into()); }
    let emit = |m: &str| { crate::base::push_log(m, "setup"); let _ = app.emit("launch-log", m.to_string()); };
    if !run_logged(&app, "git", &["pull"], Some(&repo), emit).await {
        mutating_done();
        return Err("git pull failed — see console output (offline? diverged branch?)".into());
    }
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
/// Re-read the registry PATH (HKCU + HKLM) into this process after a
/// winget install, so newly installed tools resolve WITHOUT a launcher
/// restart. Merges registry entries into the live PATH (deduplicated) —
/// never removes anything. Windows-only; no-op elsewhere.
#[cfg(windows)]
fn refresh_path_from_registry() {
    let mut additions: Vec<String> = Vec::new();
    for (hk, sub) in [
        ("HKCU", "Environment"),
        ("HKLM", "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment"),
    ] {
        let target = format!("{hk}\\{sub}");
        let Ok(o) = silent_command("reg").args(["query", &target, "/v", "Path"]).output() else { continue };
        if !o.status.success() { continue; }
        for line in String::from_utf8_lossy(&o.stdout).lines() {
            let low = line.to_lowercase();
            let Some(pos) = low.find("reg_expand_sz").or_else(|| low.find("reg_sz")) else { continue };
            let after_type = line[pos..].split_whitespace().skip(1).collect::<Vec<_>>().join(" ");
            if after_type.is_empty() { continue; }
            // Expand %VAR% against the live environment.
            let mut expanded = after_type;
            let mut i = 0;
            while i < expanded.len() {
                let rest = &expanded[i..];
                let Some(a) = rest.find('%') else { break };
                let a = a + i;
                let Some(b) = expanded[a + 1..].find('%') else { break };
                let b = b + a + 1;
                let name = expanded[a + 1..b].to_string();
                match std::env::var(&name) {
                    Ok(v) => { expanded.replace_range(a..=b, &v); i = a + v.len(); }
                    Err(_) => { i = b + 1; }
                }
            }
            for part in expanded.split(';').map(|s| s.trim()).filter(|s| !s.is_empty()) {
                additions.push(part.to_string());
            }
        }
    }
    if additions.is_empty() { return; }
    let cur = std::env::var("PATH").unwrap_or_default();
    let mut merged = cur.clone();
    for a in &additions {
        if !cur.split(';').any(|p| p.eq_ignore_ascii_case(a)) {
            merged.push(';');
            merged.push_str(a);
        }
    }
    std::env::set_var("PATH", merged);
}

#[tauri::command]
pub async fn install_prerequisite(app: tauri::AppHandle, tool: String) -> Result<serde_json::Value,String> {
    if !["git", "uv", "python", "conda"].contains(&tool.as_str()) {
        return Err(format!("unknown tool {tool}"));
    }
    #[cfg(not(windows))]
    return Err(format!("{tool} must be installed with your system package manager (one-click install is Windows-only). git: `sudo apt install git` / `brew install git`; python: `brew install python@3.11`; uv: `curl -LsSf https://astral.sh/uv/install.sh | sh`; conda: Miniconda installer from repo.anaconda.com."));
    #[cfg(windows)]
    return install_prerequisite_windows(app, tool).await;
}

/// Stream a child process to the installer console, return exit-ok.
#[cfg(windows)]
async fn run_live(app: &tauri::AppHandle, prog: &str, args: Vec<String>) -> bool {
    use tauri_plugin_shell::ShellExt;
    use tauri_plugin_shell::process::CommandEvent;
    let emit = |msg: String| { crate::base::push_log(&msg, "setup"); let _ = app.emit("setup-output", msg); };
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let (mut rx, _) = match app.shell().command(prog).args(&arg_refs[..]).spawn() {
        Ok(t) => t,
        Err(e) => { emit(format!("[!] spawn failed ({prog}): {e}\n")); return false; }
    };
    let mut code: Option<i32> = None;
    while let Some(ev) = rx.recv().await {
        match ev {
            CommandEvent::Stdout(b) | CommandEvent::Stderr(b) => emit(String::from_utf8_lossy(&b).to_string()),
            CommandEvent::Terminated(p) => { code = p.code; }
            CommandEvent::Error(e) => emit(format!("[!] {e}\n")),
            _ => {}
        }
    }
    code == Some(0)
}

/// Is the tool usable right now? PATH probes plus well-known locations
/// (conda and python.org installs don't always land on PATH immediately).
#[cfg(windows)]
fn probe_tool(tool: &str) -> bool {
    let on_path = |exe: &str| silent_command("where").arg(exe).output().is_ok_and(|o| o.status.success());
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    let exists = |p: String| PathBuf::from(&p).exists();
    match tool {
        "git" => on_path("git") || known_tool_path("git").is_some(),
        "uv" => on_path("uv") || owned_uv().is_some() || exists(format!("{home}\\.local\\bin\\uv.exe")) || exists(format!("{home}\\.cargo\\bin\\uv.exe")),
        "python" => tool_usable("python") || tool_usable("py") || known_tool_path("python").is_some() || known_tool_path("py").is_some(),
        "conda" => on_path("conda") || known_tool_path("conda").is_some()
            || exists(format!("{home}\\Miniconda3\\condabin\\conda.bat"))
            || exists(format!("{home}\\Miniconda3\\Scripts\\conda.exe"))
            || exists(format!("{home}\\Anaconda3\\condabin\\conda.bat")),
        _ => false,
    }
}

/// Does this interpreter report exactly the wanted X.Y.Z?
/// (`sys.version` prints e.g. "3.11.14 ..." — the first token must start with
/// the full pin; a neighbouring patch like 3.11.9 does not count.)
#[cfg(windows)]
fn python_matches_pin(prog: &str, extra_args: &[&str], wanted: &str) -> bool {
    let mut args: Vec<&str> = extra_args.to_vec();
    args.extend(["-c", "import sys; print(sys.version)"]);
    silent_command(prog).args(&args).output().ok()
        .and_then(|o| o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().split_whitespace().next().unwrap_or("").to_string()))
        .is_some_and(|v| v.starts_with(wanted))
}

/// SHA-256 of a downloaded installer matches the pinned value? Downloads are
/// TLS-only by default; a pin turns a MITM (or a truncated download) into a
/// loud refusal instead of executed bytes. Streamed — the git installer is ~120 MB.
#[cfg(windows)]
fn verify_sha256(path: &str, expected_hex: &str, emit: &impl Fn(String)) -> bool {
    use sha2::Digest;
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => { emit(format!("[!] checksum: cannot read {path}: {e}\n")); return false; }
    };
    let mut hasher = sha2::Sha256::new();
    if std::io::copy(&mut file, &mut hasher).is_err() {
        emit(format!("[!] checksum: failed reading {path}\n"));
        return false;
    }
    let got = format!("{:x}", hasher.finalize());
    if got.eq_ignore_ascii_case(expected_hex) { return true; }
    emit(format!("[!] CHECKSUM MISMATCH for {path}\n    got      {got}\n    expected {expected_hex}\n    refusing to run it — re-download manually from the official site.\n"));
    false
}

/// (file-version, tag) of the newest Git for Windows, e.g.
/// ("2.55.0.5", "v2.55.0.windows.5"). None when the API is unreachable —
/// callers fall back to the pinned release.
#[cfg(windows)]
async fn latest_git_release() -> Option<(String, String)> {
    let v: serde_json::Value = reqwest::Client::builder().user_agent("wan2gp-tauri")
        .timeout(std::time::Duration::from_secs(10)).build().ok()?
        .get("https://api.github.com/repos/git-for-windows/git/releases/latest")
        .send().await.ok()?
        .error_for_status().ok()?
        .json().await.ok()?;
    let tag = v.get("tag_name")?.as_str()?;
    let base = tag.strip_prefix('v').unwrap_or(tag).split(".windows").next()?;
    let n = tag.rsplit('.').next()?;
    if base.is_empty() || !n.chars().all(|c| c.is_ascii_digit()) { return None; }
    Some((format!("{base}.{n}"), tag.to_string()))
}

/// Official installers, used when winget is missing or fails (LTSC, removed
/// App Installer, corporate blocks). Mirrors Electron's install-prerequisite.
#[cfg(windows)]
async fn official_fallback(app: &tauri::AppHandle, tool: &str) -> bool {
    let emit = |msg: String| { crate::base::push_log(&msg, "setup"); let _ = app.emit("setup-output", msg); };
    let tmp = std::env::var("TEMP").or_else(|_| std::env::var("TMP")).unwrap_or("C:\\Windows\\Temp".into());
    // PowerShell download without the progress bar (which stalls/hangs piped runs).
    let download = |url: &str, dest: &str| -> Vec<String> {
        vec!["-NoProfile".into(), "-Command".into(),
            format!("$ProgressPreference='SilentlyContinue'; Invoke-WebRequest -Uri '{url}' -OutFile '{dest}'")]
    };
    match tool {
        "uv" => {
            emit("[*] Installing uv via the official script…\n".into());
            run_live(app, "powershell", vec!["-NoProfile".into(), "-ExecutionPolicy".into(), "Bypass".into(), "-Command".into(), "& { iwr -useb https://astral.sh/uv/install.ps1 | iex }".into()]).await
        }
        "git" => {
            // Live-resolve the newest release; pinned fallback keeps offline installs working.
            // The SHA pin covers the pinned fallback only — a newer live release is TLS-only (logged).
            const FALLBACK_TAG: &str = "v2.55.0.windows.5";
            const FALLBACK_VER: &str = "2.55.0.5";
            const FALLBACK_SHA: &str = "d065a4e23c3d9a6b5073d609b5be0830227ec3ca053c083ba385061ddfaf94c6";
            let (ver, tag) = latest_git_release().await
                .unwrap_or((FALLBACK_VER.to_string(), FALLBACK_TAG.to_string()));
            let mut url = format!("https://github.com/git-for-windows/git/releases/download/{tag}/Git-{ver}-64-bit.exe");
            let mut dest = format!("{tmp}\\Git-{ver}-64-bit.exe");
            let mut sha: Option<&str> = if ver.as_str() == FALLBACK_VER { Some(FALLBACK_SHA) } else { None };
            emit("[*] Downloading Git for Windows (~120 MB)…\n".into());
            if !run_live(app, "powershell", download(&url, &dest)).await || !std::path::Path::new(&dest).exists() {
                if ver.as_str() != FALLBACK_VER {
                    // Live asset name changed or offline — retry the pinned release.
                    emit(format!("[!] Git {ver} download failed — retrying pinned Git {FALLBACK_VER}…\n"));
                    url = format!("https://github.com/git-for-windows/git/releases/download/{FALLBACK_TAG}/Git-{FALLBACK_VER}-64-bit.exe");
                    dest = format!("{tmp}\\Git-{FALLBACK_VER}-64-bit.exe");
                    sha = Some(FALLBACK_SHA);
                    if !run_live(app, "powershell", download(&url, &dest)).await { return false; }
                } else { return false; }
            }
            match sha {
                Some(s) if !verify_sha256(&dest, s, &emit) => return false,
                None => emit(format!("[!] No pinned checksum for Git {ver} — trusting TLS.\n")),
                _ => {}
            }
            emit("[*] Installing silently — a couple of minutes…\n".into());
            run_live(app, &dest, vec!["/VERYSILENT".into(), "/NORESTART".into(), "/SUPPRESSMSGBOXES".into(), "/CLOSEAPPLICATIONS".into()]).await
        }
        "python" => {
            // python.org publishes NO binary installer for 3.11.14 (source-only
            // security release — the old 3.11.14 URL 404s). 3.11.9 is the newest
            // 3.11 with an official amd64 installer: good enough as a `py -3.11`
            // source, while the exact 3.11.14 pin still comes via uv
            // (python-build-standalone) — unaffected by this fallback. SHA
            // pinned (hashed from python.org over TLS).
            let url = "https://www.python.org/ftp/python/3.11.9/python-3.11.9-amd64.exe";
            let dest = format!("{tmp}\\python-3.11.9-amd64.exe");
            const SHA: &str = "5ee42c4eee1e6b4464bb23722f90b45303f79442df63083f05322f1785f5fdde";
            emit("[*] Downloading Python 3.11 (~25 MB)…\n".into());
            if !run_live(app, "powershell", download(url, &dest)).await { return false; }
            if !verify_sha256(&dest, SHA, &emit) { return false; }
            emit("[*] Installing silently — a couple of minutes…\n".into());
            run_live(app, &dest, vec!["/quiet".into(), "InstallAllUsers=0".into(), "PrependPath=1".into(), "Include_test=0".into()]).await
        }
        "conda" => {
            // Trust surface (documented, not fixed): Miniconda3-latest is a moving
            // target with no hash sidecar, so it cannot be SHA-pinned without
            // freezing a version that would rot. TLS-only — same trust as setup.py itself.
            let url = "https://repo.anaconda.com/miniconda/Miniconda3-latest-Windows-x86_64.exe";
            let dest = format!("{tmp}\\Miniconda3-latest-Windows-x86_64.exe");
            let home = std::env::var("USERPROFILE").unwrap_or("C:\\Users\\Default".into());
            emit("[*] Downloading Miniconda (~90 MB)…\n".into());
            if !run_live(app, "powershell", download(url, &dest)).await { return false; }
            emit("[*] Installing silently — a few minutes…\n".into());
            run_live(app, &dest, vec!["/InstallationType=JustMe".into(), "/RegisterPython=0".into(), "/S".into(), format!("/D={home}\\Miniconda3")]).await
        }
        _ => false,
    }
}

#[cfg(windows)]
async fn install_prerequisite_windows(app: tauri::AppHandle, tool: String) -> Result<serde_json::Value,String> {
    let emit = |msg: &str| { crate::base::push_log(msg, "setup"); let _ = app.emit("setup-output", msg.to_string()); };
    // Already there (installed manually meanwhile)? Skip the download.
    if probe_tool(&tool) {
        emit(&format!("[*] {tool} is already installed.\n"));
        return Ok(serde_json::json!({"ok": true, "success": true, "ready": true}));
    }
    emit(&format!("[*] Installing {tool} via winget (silent, a few minutes)…\n"));
    let winget_id = match tool.as_str() {
        "git" => "Git.Git", "uv" => "astral-sh.uv",
        "python" => "Python.Python.3.11", "conda" => "Anaconda.Miniconda3",
        _ => return Err(format!("unknown tool {tool}")),
    };
    let mut ok = run_live(&app, "winget", vec!["install".into(), "--id".into(), winget_id.into(), "-e".into(), "--accept-package-agreements".into(), "--accept-source-agreements".into(), "--silent".into()]).await;
    if !ok {
        emit(&format!("[!] winget failed or is missing — falling back to the official {tool} installer…\n"));
        ok = official_fallback(&app, &tool).await;
    }
    if !ok { return Err(format!("{tool} install failed — see output above (needs network; some packages need admin approval; check antivirus)")); }
    // Pick up the new PATH without a restart when possible.
    refresh_path_from_registry();
    // winget's Python.Python.3.11 is latest 3.11.x, but setup.py demands the
    // exact pin (pinned_python_wanted, e.g. 3.11.14). No usable python at all
    // → take the (pinned 3.11.9) official installer; a real-but-neighbouring
    // patch → keep it (uv provisions the exact pin automatically at install
    // time) instead of pointlessly stacking a second interpreter.
    if tool == "python" {
        let wanted = pinned_python_wanted();
        if !python_matches_pin("python", &[], &wanted) && !python_matches_pin("py", &["-3.11"], &wanted) {
            if probe_tool(&tool) {
                emit(&format!("[*] winget's Python isn't exactly {wanted} — keeping it; the installer provisions exactly {wanted} via uv automatically.\n"));
            } else {
                emit(&format!("[!] winget's Python isn't exactly {wanted} and no usable python found — installing the pinned build…\n"));
                ok = official_fallback(&app, &tool).await;
                if !ok { return Err(format!("{tool} install failed — see output above (needs network; some packages need admin approval; check antivirus)")); }
                refresh_path_from_registry();
            }
        }
    }
    if probe_tool(&tool) {
        // uv is ours now: adopt the installed copy into the launcher tools
        // dir so later runs (incl. self-update) never touch a foreign binary.
        if tool == "uv" {
            if let Some(owned) = snapshot_owned_uv() {
                emit(&format!("[*] uv adopted: {owned}\n"));
            }
        }
        emit(&format!("[✓] {tool} installed and on PATH — continuing…\n"));
        return Ok(serde_json::json!({"ok": true, "success": true, "ready": true}));
    }
    emit(&format!("[✓] {tool} installed — restart the launcher so PATH picks it up.\n"));
    Ok(serde_json::json!({"ok": true, "success": true, "ready": false}))
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
// Trust surface (documented, not fixed): upstream script executed with Bypass —
// same trust as setup.py itself. Integrity comes from the pinned SHA-256
// manifest above (verdict re-probes dlss5/, not script output).
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

#[cfg(test)]
mod amd_therock_tests {
    use super::amd_therock_torch_cmds;
    /// Primary pins the exact verified 7.15 stack (bracket-free for
    /// setup.py's plain splice); fallback is the community staging float.
    fn check_primary(p: &str, targets: &[&str]) {
        assert!(p.starts_with("--pre "), "got {p}");
        assert!(p.contains("torch==2.12.0+rocm7.15.0a20260728"), "got {p}");
        assert!(p.contains("torchvision==0.27.0+rocm7.15.0a20260728"), "got {p}");
        assert!(p.contains("torchaudio==2.11.0+rocm7.15.0a20260728"), "got {p}");
        assert!(p.contains("--index-url https://rocm.nightlies.amd.com/whl-multi-arch/"), "got {p}");
        assert!(!p.contains('[') && !p.contains(']'), "bracket-free: {p}");
        for t in targets {
            assert!(p.contains(&format!("amd-torch-device-{t}==2.12.0+rocm7.15.0a20260728")), "{t} missing: {p}");
            assert!(p.contains(&format!("amd-torchvision-device-{t}==0.27.0+rocm7.15.0a20260728")), "{t} missing: {p}");
        }
    }
    #[test]
    fn pinned_primary_per_profile() {
        let (p, f) = amd_therock_torch_cmds("AMD_GFX1201", "AMD Radeon AI PRO R9700").unwrap();
        check_primary(&p, &["gfx1200", "gfx1201"]);
        assert!(f.contains("/v2-staging/gfx120X-all/"), "got {f}");
        let (p, f) = amd_therock_torch_cmds("AMD_GFX110X", "AMD Radeon RX 7900 XTX").unwrap();
        check_primary(&p, &["gfx1100", "gfx1101", "gfx1102", "gfx1103"]);
        assert!(f.contains("/v2-staging/gfx110X-all/"), "got {f}");
        // Strix Halo gets both APUs; Strix Point 890M narrows to gfx1150.
        let (p, f) = amd_therock_torch_cmds("AMD_GFX1151", "AMD Ryzen AI Max+ PRO 395").unwrap();
        check_primary(&p, &["gfx1150", "gfx1151"]);
        assert!(f.contains("/v2-staging/gfx1151/"), "got {f}");
        let (p, f) = amd_therock_torch_cmds("AMD_GFX1151", "AMD Radeon 890M").unwrap();
        check_primary(&p, &["gfx1150"]);
        assert!(!p.contains("gfx1151"), "got {p}");
        assert!(f.contains("/v2-staging/gfx1150/"), "got {f}");
        // RDNA 2: full discrete target set on the pinned primary.
        let (p, f) = amd_therock_torch_cmds("AMD_GFX103X", "AMD Radeon RX 6800 XT").unwrap();
        check_primary(&p, &["gfx1030", "gfx1031", "gfx1032", "gfx1033", "gfx1034", "gfx1035", "gfx1036"]);
        assert!(f.contains("/v2-staging/gfx103X-dgpu/"), "got {f}");
        assert!(amd_therock_torch_cmds("RTX_50", "NVIDIA GeForce RTX 5090").is_none());
    }
}

#[cfg(test)]
mod setup_py_override_tests {
    use super::{patch_setup_py_overrides, setup_py_forced_profile};
    /// Minimal fixture carrying the two upstream anchors (same shape as
    /// deepbeepmeep/Wan2GP setup.py: __main__ profile block + VRAM
    /// fallback). Must stay valid Python — the patch refuses to write a
    /// file that fails ast.parse.
    const FIXTURE: &str = "import os\nimport subprocess\ndef get_system_specs():\n    try:\n        out = subprocess.check_output([\"nvidia-smi\"])\n        vram_gb = float(out.split('\\n')[0]) / 1024\n    except:\n        print(\"[!] Warning: Could not detect VRAM via nvidia-smi. Defaulting to 8GB.\")\n        vram_gb = 8\n    return vram_gb\nif __name__ == \"__main__\":\n    gpu_name, vendor = get_gpu_info()\n    profile_key = get_profile_key(gpu_name, vendor)\n    profile = cfg['gpu_profiles'][profile_key]\n";
    fn fixture_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("wgp-setup-py-override-{tag}-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        std::fs::write(d.join("setup.py"), FIXTURE).unwrap();
        d
    }
    #[test]
    fn forced_profile_allowlist() {
        // Every key setup_config.json actually defines passes through.
        for k in ["RTX_50", "RTX_40", "RTX_30", "RTX_20", "GTX_10",
                  "AMD_GFX110X", "AMD_GFX1151", "AMD_GFX1201", "MPS"] {
            assert_eq!(setup_py_forced_profile(k), Some(k), "{k}");
        }
        // No upstream profile → setup.py keeps its own detection (a forced
        // unknown key would KeyError inside setup.py).
        for k in ["AMD_GFX103X", "INTEL_XPU", "CPU", "", "RTX_99"] {
            assert_eq!(setup_py_forced_profile(k), None, "{k}");
        }
    }
    #[test]
    fn patch_applies_both_overrides_and_is_idempotent() {
        let d = fixture_dir("ok");
        patch_setup_py_overrides(&d).unwrap();
        let out = std::fs::read_to_string(d.join("setup.py")).unwrap();
        assert!(out.contains("WAN2GP_TAURI_GPU_PROFILE"), "profile override missing");
        assert!(out.contains("WAN2GP_TAURI_VRAM_GB"), "vram override missing");
        assert!(out.contains("Launcher-forced GPU profile"), "log line missing");
        assert!(out.contains("Launcher-provided VRAM"), "log line missing");
        // Second run is a no-op (repo updates re-patch from scratch).
        patch_setup_py_overrides(&d).unwrap();
        let out2 = std::fs::read_to_string(d.join("setup.py")).unwrap();
        assert_eq!(out, out2, "patch not idempotent");
        let _ = std::fs::remove_dir_all(&d);
    }
    #[test]
    fn patch_handles_crlf_checkout() {
        // git on Windows often checks out CRLF — anchors still match and
        // endings are preserved.
        let d = std::env::temp_dir().join(format!("wgp-setup-py-crlf-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        std::fs::write(d.join("setup.py"), FIXTURE.replace('\n', "\r\n")).unwrap();
        patch_setup_py_overrides(&d).unwrap();
        let out = std::fs::read_to_string(d.join("setup.py")).unwrap();
        assert!(out.contains("WAN2GP_TAURI_GPU_PROFILE"), "profile override missing");
        assert!(out.contains("\r\n"), "CRLF endings not preserved");
        // Second run is a no-op (marker survives the round-trip).
        patch_setup_py_overrides(&d).unwrap();
        let _ = std::fs::remove_dir_all(&d);
    }
    #[test]
    fn patch_reports_upstream_drift() {
        let d = std::env::temp_dir().join(format!("wgp-setup-py-drift-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        std::fs::write(d.join("setup.py"), "print('entirely different file')\n").unwrap();
        let err = patch_setup_py_overrides(&d).unwrap_err();
        assert!(err.contains("profile") && err.contains("vram"), "got {err}");
        // Drifted file left untouched.
        assert_eq!(std::fs::read_to_string(d.join("setup.py")).unwrap(), "print('entirely different file')\n");
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
mod preflight_tests {
    use super::run_preflight_checks;
    fn tmp_repo(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("wgp-preflight-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn amd_hw(vram: &str, driver: &str) -> serde_json::Value {
        serde_json::json!({"vendor": "AMD", "name": "AMD Radeon AI PRO R9700", "vramMB": vram, "driverVersion": driver, "raw": "test"})
    }
    fn level(checks: &[super::PreflightCheck], id: &str) -> Option<&'static str> {
        checks.iter().find(|c| c.id == id).map(|c| c.level)
    }
    #[test]
    fn healthy_amd_box_passes_driver_and_vram() {
        let repo = tmp_repo("ok");
        let hw = amd_hw("32768 MiB", "32.0.11029.1008");
        let plan = crate::hw::build_install_plan(&hw);
        let (fatal, checks) = run_preflight_checks(&repo, &hw, &plan);
        assert_eq!(level(&checks, "driver"), Some("ok"));
        assert_eq!(level(&checks, "vram"), None); // known 32GB: no warning
        assert!(!fatal || level(&checks, "git") == Some("ok"), "only git may fail, nothing fabricated");
        let _ = std::fs::remove_dir_all(&repo);
    }
    #[test]
    fn basic_adapter_is_fatal() {
        let repo = tmp_repo("basic");
        let hw = serde_json::json!({"vendor": "unknown", "name": "Microsoft Basic Display Adapter", "vramMB": "0 MiB", "driverVersion": "", "raw": "test"});
        let plan = crate::hw::build_install_plan(&hw);
        let (fatal, checks) = run_preflight_checks(&repo, &hw, &plan);
        assert!(fatal);
        assert_eq!(level(&checks, "driver"), Some("fail"));
        let _ = std::fs::remove_dir_all(&repo);
    }
    #[test]
    fn old_driver_warns_not_fails() {
        let repo = tmp_repo("old");
        let hw = amd_hw("16384 MiB", "31.0.21029.1006");
        let plan = crate::hw::build_install_plan(&hw);
        let (fatal, checks) = run_preflight_checks(&repo, &hw, &plan);
        assert_eq!(level(&checks, "driver"), Some("warn"));
        assert!(!fatal);
        let _ = std::fs::remove_dir_all(&repo);
    }
    #[test]
    fn stale_state_is_reported() {
        let repo = tmp_repo("stale");
        // Marker for a removed env + CUDA-era config on an AMD box.
        std::fs::write(repo.join(".wan2gp-install-ok"), "uv 12345").unwrap();
        std::fs::write(repo.join("wgp_config.json"), r#"{"attention_mode": "sage2"}"#).unwrap();
        let hw = amd_hw("0 MiB", "32.0.11029.1008");
        let plan = crate::hw::build_install_plan(&hw);
        let (_, checks) = run_preflight_checks(&repo, &hw, &plan);
        assert_eq!(level(&checks, "stale-marker"), Some("info"));
        assert_eq!(level(&checks, "stale-config"), Some("info"));
        assert_eq!(level(&checks, "vram"), Some("warn")); // unknown VRAM mistiers
        let _ = std::fs::remove_dir_all(&repo);
    }
}

#[cfg(test)]
mod av_msg_tests {
    use super::av_exclusion_msg;
    #[test]
    fn vendor_tailored() {
        // Proactive warns need observed risk: only AMD nightlies were
        // ever quarantined. NVIDIA stays on the reactive missing-DLL
        // hint; everything else is silent.
        assert!(av_exclusion_msg("AMD").unwrap().contains("nightly"));
        assert_eq!(av_exclusion_msg("NVIDIA"), None);
        assert_eq!(av_exclusion_msg("INTEL"), None);
        assert_eq!(av_exclusion_msg("CPU"), None);
        assert_eq!(av_exclusion_msg("APPLE"), None);
        assert_eq!(av_exclusion_msg("unknown"), None);
    }
}

#[cfg(test)]
mod owned_uv_tests {
    use super::{copy_owned_uv_in, owned_uv_in, path_uv_exe, UV_EXE, UV_MARKER};
    fn tmp_data(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("wgp-owned-uv-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    #[test]
    fn empty_tools_dir_means_no_owned_uv() {
        let data = tmp_data("empty");
        assert!(owned_uv_in(&data).is_none());
        let _ = std::fs::remove_dir_all(&data);
    }
    #[test]
    fn dead_copy_is_not_adopted() {
        let data = tmp_data("dead");
        let tools = data.join(".tools");
        std::fs::create_dir_all(&tools).unwrap();
        std::fs::write(tools.join(UV_EXE), b"not-an-executable").unwrap();
        assert!(owned_uv_in(&data).is_none());
        let _ = std::fs::remove_dir_all(&data);
    }
    #[test]
    fn receiptless_copy_is_not_owned_but_runnable() {
        // Live 0.5.3 state: snapshot copy without marker — must NOT count
        // as owned (self-update refuses it), but the copy fallback sees it.
        let Some(real) = path_uv_exe() else { return; };
        let data = tmp_data("nocopy");
        let tools = data.join(".tools");
        std::fs::create_dir_all(&tools).unwrap();
        std::fs::copy(&real, tools.join(UV_EXE)).unwrap();
        assert!(owned_uv_in(&data).is_none());
        assert!(copy_owned_uv_in(&data).is_some());
        let _ = std::fs::remove_dir_all(&data);
    }
    #[test]
    fn marker_plus_working_binary_is_owned() {
        let Some(real) = path_uv_exe() else { return; };
        let data = tmp_data("marked");
        let tools = data.join(".tools");
        std::fs::create_dir_all(&tools).unwrap();
        std::fs::copy(&real, tools.join(UV_EXE)).unwrap();
        std::fs::write(tools.join(UV_MARKER), "test").unwrap();
        let owned = owned_uv_in(&data).expect("marked working copy is owned");
        assert!(std::process::Command::new(&owned).arg("--version").output().is_ok_and(|o| o.status.success()));
        let _ = std::fs::remove_dir_all(&data);
    }
    // installer_owned_uv_in is proven manually (network + writes the real
    // receipt): not in unit tests.
}

#[cfg(test)]
mod tool_path_tests {
    use super::{known_tool_path, tool_candidates, tool_found, tool_path};
    use std::path::PathBuf;
    #[test]
    fn tool_path_never_empty_and_falls_back_to_bare_name() {
        for t in ["git", "conda", "py", "python", "whatever"] {
            let r = tool_path(t);
            assert!(!r.is_empty());
            // Either an absolute hit or the bare name (never a dir, never blank).
            assert!(r == t || std::path::Path::new(&r).is_absolute());
        }
        // uv resolves to owned-or-PATH (never empty either).
        assert!(!tool_path("uv").is_empty());
    }
    #[cfg(windows)]
    #[test]
    fn candidate_tables_cover_stock_layouts() {
        let home = "C:\\Users\\t";
        let git = tool_candidates("git", home);
        assert!(git.contains(&PathBuf::from("C:\\Program Files\\Git\\bin\\git.exe")));
        let conda = tool_candidates("conda", home);
        assert!(conda.iter().any(|p| p.to_string_lossy().contains("Miniconda3\\Scripts\\conda.exe")));
        // .bat is never offered as a spawn target (not a process image).
        assert!(!conda.iter().any(|p| p.extension().is_some_and(|e| e == "bat")));
        assert_eq!(tool_candidates("py", home), vec![PathBuf::from("C:\\Windows\\py.exe")]);
        assert!(tool_candidates("nope", home).is_empty());
    }
    #[test]
    fn gates_agree_with_resolution() {
        // Whatever tool_path resolves absolutely must also probe found.
        for t in ["git", "conda", "py", "python"] {
            let r = tool_path(t);
            if r != t {
                assert!(std::path::Path::new(&r).is_file());
                assert!(tool_found(t), "{t} resolves but does not probe");
            }
        }
        let _ = known_tool_path("definitely-not-a-tool");
    }
}

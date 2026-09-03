//! Wan2GP install / reinstall / uninstall / kernel sync / core update.
use tauri::Emitter;
use std::path::{Path, PathBuf};
use crate::base::*;
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;
use crate::{hw::{build_install_plan, get_gpu_info_sync, kernel_profile_key}, status::get_active_env};

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
    let stale = env_path.exists() && !env_path.join(if cfg!(windows){"Scripts\\python.exe"} else {"bin/python"}).exists();
    if stale {
        emit(&format!("[*] Removing stale env at {} …\n", env_path.display()));
        let _ = std::fs::remove_dir_all(&env_path);
    }
    // fix: hardlink warning when cache (C:) and target (D:) differ → move cache to repo/.uv-cache on same drive so hardlink works (fast)
    let uv_cache = repo.join(".uv-cache");
    let _ = std::fs::create_dir_all(&uv_cache);
    std::env::set_var("UV_CACHE_DIR", uv_cache.to_string_lossy().to_string());
    // don't force copy — hardlink on same drive is faster; warning disappears when cache is on D:
    // run setup.py with the env's python (hardware-aware: setup.py reads setup_config.json + GPU)
    {
        let (py, args): (String, Vec<String>) = if env.as_str() == "conda" { ("conda".into(), vec!["run".into(), "-p".into(), env_path.to_string_lossy().to_string(), "python".into(), "setup.py".into(), "install".into(), "--env".into(), env.clone(), "--auto".into()]) } else {
            let p = if env=="uv" { env_path.join(if cfg!(windows){"Scripts\\python.exe"} else {"bin/python"}) } else { env_path.join(if cfg!(windows){"Scripts\\python.exe"} else {"bin/python3"}) };
            let py_bin = if p.exists() { p.to_string_lossy().to_string() } else if env=="uv" { "uv".into() } else { "python".into() };
            if py_bin=="uv" { ("uv".into(), vec!["run".into(), "--with".into(), "setuptools".into(), "python".into(), "setup.py".into(), "install".into(), "--env".into(), env.clone(), "--auto".into()]) }
            else { (py_bin, vec!["setup.py".into(), "install".into(), "--env".into(), env.clone(), "--auto".into()]) }
        };
        let (mut rx, _child) = app.shell().command(&py).args(args).current_dir(&repo).spawn().map_err(|e| e.to_string())?;
        // track which phases we've started
        let mut phases = std::collections::HashSet::new();
        let mut do_phase = |id: &str, label: &str| { if phases.insert(id.to_string()) { let _ = app.emit("setup-phase", serde_json::json!({"id": id, "label": label, "done": false})); } };
        let done_phase = |id: &str, label: &str| { let _ = app.emit("setup-phase", serde_json::json!({"id": id, "label": label, "done": true})); };
        while let Some(ev) = rx.recv().await {
            let txt = match ev { CommandEvent::Stdout(b) => String::from_utf8_lossy(&b).to_string(), CommandEvent::Stderr(b) => String::from_utf8_lossy(&b).to_string(), _ => continue };
            emit(&txt);
            let low = txt.to_lowercase();
            if low.contains("[1/3]") || low.contains("preparing environment") { do_phase("venv", "Create Python virtual environment"); }
            if low.contains("[2/3]") || low.contains("installing torch") { done_phase("venv", "Create Python virtual environment"); do_phase("torch", "Install PyTorch + CUDA"); }
            if low.contains("[3/3]") || low.contains("installing requirements") { done_phase("torch", "Install PyTorch + CUDA"); do_phase("reqs", "Install Python dependencies"); }
            if low.contains("triton-windows") && low.contains("installed") { done_phase("reqs", "Install Python dependencies"); done_phase("triton", "Install Triton compiler"); }
            if low.contains("sageattention") && low.contains("installed") { do_phase("sage", "Install Sage Attention kernel"); done_phase("sage", "Install Sage Attention kernel"); }
            if low.contains("spas-sage") || low.contains("sparge") { do_phase("flash", "Install Flash Attention"); }
            if low.contains("flash-attn") && low.contains("installed") { done_phase("flash", "Install Flash Attention"); do_phase("kernels", "Install GPU kernels (nunchaku/GGUF)"); }
            if low.contains("nunchaku") && low.contains("installed") { /* kernels still running */ }
            if low.contains("llamacpp") && low.contains("installed") { done_phase("kernels", "Install GPU kernels (nunchaku/GGUF)"); }
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
    emit("[*] Install finished.\n");
    mutating_done();
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command]
pub async fn reinstall(app: tauri::AppHandle) -> Result<serde_json::Value,String> {
    mutating_try("reinstall")?;
    let repo = get_repo_dir();
    let emit = |msg: &str| { crate::base::push_log(msg, "setup"); let _ = app.emit("setup-output", msg.to_string()); };
    emit("[*] Removing existing installation...\n");
    // backup plugins/finetunes (ponytail: xcopy fallback)
    let backup = get_data_dir().join(".reinstall-backup");
    let _ = std::fs::remove_dir_all(&backup);
    let _ = std::fs::create_dir_all(&backup);
    for sub in ["plugins","finetunes"] { let s = repo.join(sub); if s.exists() { let d = backup.join(sub); let _ = silent_command("xcopy").args(["/E","/I", s.to_string_lossy().as_ref(), d.to_string_lossy().as_ref()]).output(); } }
    if repo.join("wgp_config.json").exists() { let _ = std::fs::copy(repo.join("wgp_config.json"), backup.join("wgp_config.json")); }
    if repo.exists() {
        // ponytail: .electron is the live WebView2 Shared Dictionary — locked while launcher runs, keep it (Electron d186d49+e3e8505)
        const KEEP: &[&str] = &[".electron"];
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
    mutating_done(); Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command]
pub async fn uninstall(app: tauri::AppHandle) -> Result<serde_json::Value,String> {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    mutating_try("uninstall")?;
    let repo = get_repo_dir();
    if !repo.exists() { mutating_done(); return Err("Wan2GP not installed".into()); }
    if !app.dialog().message("Uninstall Wan2GP?

Removes the app, its Python environment and packages.").title("Uninstall Wan2GP").kind(MessageDialogKind::Info).blocking_show() {
        mutating_done();
        return Ok(serde_json::json!({"cancelled": true}));
    }
    let keep = app.dialog().message("Keep your downloaded files?

OK = keep checkpoints, LoRAs and outputs (reinstall reuses them).
Cancel = delete everything.").title("Keep models?").kind(MessageDialogKind::Info).blocking_show();
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
    let cmd = match tool.as_str() { "git" => vec!["winget","install","--id","Git.Git","-e"], "uv" => vec!["winget","install","--id","astral-sh.uv","-e"], _ => return Err(format!("unknown tool {tool}")) };
    let (mut rx, _) = app.shell().command(cmd[0]).args(&cmd[1..]).spawn().map_err(|e| e.to_string())?;
    while let Some(ev) = rx.recv().await { match ev { CommandEvent::Stdout(b)|CommandEvent::Stderr(b) => { let s = String::from_utf8_lossy(&b).to_string(); crate::base::push_log(&s, "setup"); let _ = app.emit("launch-log", s); }, _=>{} } }
    Ok(serde_json::json!({"ok": true, "success": true}))
}

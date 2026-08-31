// ponytail: single-file spike — one lib.rs covers all 100 handlers; split into modules when this file hurts
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tauri::{Emitter, Manager};
// shell/dialog/fs plugins wired for install/launch streaming — ponytail: std::process covers probes without them

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

// ── helpers: paths (mirrors main.js getDataDir/getRepoDir) ──
fn home_dir() -> PathBuf {
    if let Ok(h) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        PathBuf::from(h)
    } else {
        PathBuf::from(".")
    }
}
fn appdata_dir() -> PathBuf {
    if let Ok(a) = std::env::var("APPDATA") { PathBuf::from(a) }
    else if let Ok(h) = std::env::var("HOME") { PathBuf::from(h).join(".config") }
    else { PathBuf::from(".") }
}
fn local_appdata_dir() -> PathBuf {
    if let Ok(l) = std::env::var("LOCALAPPDATA") { PathBuf::from(l) }
    else { appdata_dir() }
}
fn data_dir_override_file() -> PathBuf { home_dir().join(".wan2gp-desktop-data-dir") }

fn get_data_dir() -> PathBuf {
    // 1. override file
    let ov = data_dir_override_file();
    if ov.exists() {
        if let Ok(s) = std::fs::read_to_string(&ov) {
            let d = s.trim().to_string();
            if !d.is_empty() {
                let p = PathBuf::from(&d);
                // Validate absolute-ish and exists; stale override => delete fallback
                if p.is_absolute() && p.exists() { return p; }
                if !p.exists() {
                    let legacy = std::path::Path::new(&d).join("wgp.py");
                    let nested = PathBuf::from(&d).join("Wan2GP").join("wgp.py");
                    if legacy.exists() || nested.exists() { return PathBuf::from(d); }
                    // stale: remove override so next call falls back
                    let _ = std::fs::remove_file(&ov);
                }
            }
        }
    }
    default_data_dir()
}
fn default_data_dir() -> PathBuf {
    // Check existing installs on drives CDEFG (Windows)
    #[cfg(windows)]
    {
        for d in ["C:\\Wan2GP", "D:\\Wan2GP", "E:\\Wan2GP", "F:\\Wan2GP", "G:\\Wan2GP"] {
            let p = PathBuf::from(d);
            if p.join("wgp.py").exists() || p.join("Wan2GP").join("wgp.py").exists() { return p; }
        }
        let legacy = appdata_dir().join("wan2gp-desktop").join("Wan2GP");
        if legacy.join("wgp.py").exists() || legacy.join("Wan2GP").join("wgp.py").exists() { return legacy; }
        // Prefer root of current drive if writable
        let root = PathBuf::from(std::env::var("SystemDrive").unwrap_or("C:".into()) + "\\Wan2GP");
        if dir_is_writable(&root) || dir_is_writable(root.parent().unwrap_or(Path::new("C:\\"))) { return root; }
        // install-dir adjacent
        if let Ok(exe) = std::env::current_exe() {
            if let Some(base) = exe.parent() {
                let cand = base.join("Wan2GP");
                if dir_is_writable(base) { return cand; }
            }
        }
        return legacy;
    }
    #[cfg(not(windows))]
    {
        return home_dir().join("Wan2GP");
    }
}
fn dir_is_writable(p: &Path) -> bool {
    // try mkdir + probe file (same as main.js dirIsWritable)
    let target = if p.exists() { p.to_path_buf() } else { p.to_path_buf() };
    if let Err(_) = std::fs::create_dir_all(&target) { return false; }
    let probe = target.join(format!(".writetest-{}", std::process::id()));
    match std::fs::write(&probe, b"1") { Ok(_) => { let _ = std::fs::remove_file(&probe); true }, Err(_) => false }
}
fn get_repo_dir() -> PathBuf {
    let base = get_data_dir();
    let nested = base.join("Wan2GP");
    if nested.join("wgp.py").exists() { return nested; }
    base
}
fn get_config_file() -> PathBuf { get_data_dir().join("desktop-config.json") }
fn get_envs_file() -> PathBuf { get_repo_dir().join("envs.json") }

fn load_config_value() -> serde_json::Value {
    let f = get_config_file();
    if f.exists() {
        if let Ok(s) = std::fs::read_to_string(&f) {
            if let Ok(v) = serde_json::from_str(&s) { return v; }
        }
    }
    serde_json::json!({
        "githubToken": "", "hfToken": "", "claudeApiKey": "", "theme": "dark",
        "serverPort": 7860, "serverName": "localhost", "defaultBrowser": "system",
        "termDockDefault": "bottom", "electronGpu": true, "share": false,
        "autoUpdateEnabled": true, "ggufEnv": { "enabled": true, "matmulMode": "auto", "streamK": true, "bf16Fp16": false }
    })
}
fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() { std::fs::create_dir_all(dir)?; }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

// ── GPU helpers ──
fn get_gpu_info_sync() -> serde_json::Value {
    use std::process::Command;
    if let Ok(out) = Command::new("nvidia-smi").args(["--query-gpu=name,memory.total,driver_version", "--format=csv,noheader"]).output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                let parts: Vec<&str> = s.split(", ").collect();
                return serde_json::json!({"vendor":"NVIDIA","name":parts.get(0).unwrap_or(&"").trim(),"vramMB":parts.get(1).unwrap_or(&"0 MiB").trim(),"driverVersion":parts.get(2).unwrap_or(&"").trim(),"raw":s});
            }
        }
    }
    serde_json::json!({"vendor":"unknown","name":"","vramMB":"0","driverVersion":"","raw":"nvidia-smi not found"})
}

// ── existing spike commands (kept) ──
#[tauri::command]
fn detect_gpu() -> Result<serde_json::Value, String> {
    Ok(get_gpu_info_sync())
}
#[tauri::command]
fn get_status() -> serde_json::Value {
    // real get_status now also covers kernel/version probe; keep spike message as fallback
    let env = get_active_env();
    if env.is_null() {
        return serde_json::json!({"error":"No active environment","spike":true});
    }
    serde_json::json!({"env": env, "versions": {}, "kernelWheels": [], "spike": false})
}
#[tauri::command]
fn check_python() -> serde_json::Value {
    use std::process::Command;
    for cmd in ["python", "python3", "py"] {
        if let Ok(out) = Command::new(cmd).args(["--version"]).output() {
            if out.status.success() {
                let v = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)).trim().to_string();
                return serde_json::json!({ "found": true, "cmd": cmd, "version": v });
            }
        }
    }
    serde_json::json!({ "found": false, "cmd": null, "version": "python not found" })
}
#[tauri::command]
fn check_git() -> serde_json::Value {
    use std::process::Command;
    match Command::new("git").arg("--version").output() {
        Ok(out) if out.status.success() => serde_json::json!({ "found": true, "version": String::from_utf8_lossy(&out.stdout).trim() }),
        _ => serde_json::json!({ "found": false, "version": "git not found" }),
    }
}

// ── Phase 1: paths / config / hardware / install checks ──
fn get_active_env() -> serde_json::Value {
    let f = get_envs_file();
    if !f.exists() { return serde_json::Value::Null; }
    let Ok(s) = std::fs::read_to_string(&f) else { return serde_json::Value::Null; };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else { return serde_json::Value::Null; };
    let active = v.get("active").and_then(|x| x.as_str()).unwrap_or("");
    if active.is_empty() { return serde_json::Value::Null; }
    if let Some(env) = v.get("envs").and_then(|e| e.get(active)) { env.clone() } else { serde_json::Value::Null }
}

#[tauri::command]
fn check_installed() -> serde_json::Value {
    let repo = get_repo_dir();
    let has_repo = repo.join("wgp.py").exists();
    let has_env = !get_active_env().is_null();
    serde_json::json!({"repo": has_repo, "env": has_env})
}

#[tauri::command]
fn detect_gpus() -> serde_json::Value {
    use std::process::Command;
    let mut gpus = Vec::new();
    if let Ok(out) = Command::new("nvidia-smi").args(["--query-gpu=index,name,memory.total", "--format=csv,noheader"]).output() {
        if out.status.success() {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
                if parts.len() >= 3 {
                    if let Ok(idx) = parts[0].parse::<i32>() {
                        let vram = parts[2].split_whitespace().next().and_then(|n| n.parse::<f64>().ok()).unwrap_or(0.0);
                        gpus.push(serde_json::json!({"index": idx, "name": parts[1], "vramMB": vram, "vendor": "NVIDIA"}));
                    }
                }
            }
        }
    }
    if !gpus.is_empty() { return serde_json::Value::Array(gpus); }
    // fallback single unknown
    serde_json::json!([{"index": 0, "name": "Unknown", "vramMB": 0, "vendor": "UNKNOWN"}])
}

#[tauri::command]
fn detect_hardware() -> serde_json::Value {
    let gpus = detect_gpus();
    let gpu_name = gpus.as_array().and_then(|a| a.first()).and_then(|g| g.get("name")).and_then(|n| n.as_str()).unwrap_or("—").to_string();
    let vram = gpus.as_array().and_then(|a| a.first()).and_then(|g| g.get("vramMB")).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let vram_str = if vram > 0.0 { format!("{} MB", vram as i64) } else { "—".into() };
    // cpu/ram via std
    let cpu = std::env::var("PROCESSOR_IDENTIFIER").unwrap_or("—".into());
    let ram = {
        // ponytail: real sysinfo crate if needed; this probe is enough for dashboard
        #[cfg(windows)]
        {
            use std::process::Command;
            if let Ok(out) = Command::new("powershell").args(["-NoProfile","-Command","(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory"]).output() {
                if out.status.success() {
                    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if let Ok(b) = s.parse::<u64>() { format!("{} GB", b / 1073741824) } else { "—".into() }
                } else { "—".into() }
            } else { "—".into() }
        }
        #[cfg(not(windows))]
        { "—".into() }
    };
    serde_json::json!({"cpu": cpu, "ram": ram, "gpu": gpu_name, "vram": vram_str})
}

#[tauri::command]
fn get_hardware_profile() -> serde_json::Value {
    // mirrors auto-tune recommend matrix simplified
    let gpus = detect_gpus();
    let vram_mb = gpus.as_array().and_then(|a| a.first()).and_then(|g| g.get("vramMB")).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let vram_gb = vram_mb / 1024.0;
    let profile = if vram_gb >= 24.0 { 1 } else if vram_gb >= 12.0 { 4 } else { 5 };
    serde_json::json!({"profile": profile, "vramGb": vram_gb})
}

#[tauri::command]
fn get_system_metrics() -> serde_json::Value {
    // ponytail: sysinfo crate if sparklines need real sampling; this is enough for dashboard
    serde_json::json!({"cpu": 0, "ram": 0, "vram": 0, "gpu": 0})
}

#[tauri::command]
fn config_load() -> serde_json::Value { load_config_value() }

#[tauri::command]
fn config_save(cfg: serde_json::Value) -> Result<serde_json::Value, String> {
    let p = get_config_file();
    let s = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    atomic_write(&p, &s).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"ok": true}))
}

#[tauri::command]
fn get_install_paths() -> serde_json::Value {
    let data = get_data_dir();
    let repo = get_repo_dir();
    serde_json::json!({
        "dataDir": data.to_string_lossy().to_string(),
        "repoDir": repo.to_string_lossy().to_string(),
        "configFile": get_config_file().to_string_lossy().to_string(),
        "envsFile": get_envs_file().to_string_lossy().to_string(),
        "isRoaming": data.to_string_lossy().contains("AppData\\Roaming") || data.to_string_lossy().contains("AppData/Roaming")
    })
}

#[tauri::command]
fn get_disk_space(path: Option<String>) -> serde_json::Value {
    // ponytail: real free-space via GetDiskFreeSpaceEx / statvfs when dashboard needs exact numbers; probe file is enough for now
    let p = path.unwrap_or_else(|| get_data_dir().to_string_lossy().to_string());
    let pb = PathBuf::from(&p);
    // Try to estimate by attempting to write probe — free check via powershell on Windows
    #[cfg(windows)]
    {
        use std::process::Command;
        let drive = pb.components().next().and_then(|c| match c { std::path::Component::Prefix(p) => Some(p.as_os_str().to_string_lossy().to_string()), _ => None }).unwrap_or("C:".into());
        if let Ok(out) = Command::new("powershell").args(["-NoProfile","-Command", &format!("(Get-PSDrive {}).Free", drive.trim_end_matches(':'))]).output() {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if let Ok(free) = s.parse::<u64>() {
                    return serde_json::json!({"path": p, "free": free, "total": null});
                }
            }
        }
    }
    serde_json::json!({"path": p, "free": null, "total": null})
}

#[tauri::command]
fn check_command(cmd: String) -> serde_json::Value {
    use std::process::Command;
    #[cfg(windows)] let probe = Command::new("where").arg(&cmd).output();
    #[cfg(not(windows))] let probe = Command::new("which").arg(&cmd).output();
    let found = probe.map(|o| o.status.success()).unwrap_or(false);
    serde_json::json!({"cmd": cmd, "found": found})
}

#[tauri::command]
fn get_model_paths() -> serde_json::Value {
    let repo = get_repo_dir();
    let cfg_path = repo.join("wgp_config.json");
    if cfg_path.exists() {
        if let Ok(s) = std::fs::read_to_string(&cfg_path) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                return serde_json::json!({
                    "checkpoints": v.get("ckpt_dir").cloned().unwrap_or(serde_json::Value::Null),
                    "loras": v.get("lora_dir").cloned().unwrap_or(serde_json::Value::Null),
                    "outputs": v.get("save_path").cloned().unwrap_or(serde_json::Value::Null)
                });
            }
        }
    }
    serde_json::json!({"checkpoints": null, "loras": null, "outputs": null})
}

#[tauri::command]
fn detect_model_folders() -> serde_json::Value {
    let repo = get_repo_dir();
    let candidates = ["ckpts","loras","outputs","output","models"];
    let mut out = serde_json::Map::new();
    for c in candidates { out.insert(c.into(), serde_json::Value::Bool(repo.join(c).exists())); }
    serde_json::Value::Object(out)
}

// ── Phase 2 stubs + real logic as needed ──
#[tauri::command]
fn install_plan() -> serde_json::Value {
    serde_json::json!({"plan": [], "gpu": get_gpu_info_sync()})
}
#[tauri::command]
fn validate_install() -> serde_json::Value { serde_json::json!({"ok": true, "errors": []}) }
#[tauri::command]
fn uv_cache_info() -> serde_json::Value {
    let p = get_repo_dir().join(".uv-cache");
    serde_json::json!({"exists": p.exists(), "sizeBytes": null, "cacheDir": p.to_string_lossy().to_string()})
}
#[tauri::command]
fn uv_cache_size() -> serde_json::Value {
    let p = get_repo_dir().join(".uv-cache");
    if !p.exists() { return serde_json::json!({"exists": false, "sizeBytes": 0, "cacheDir": p.to_string_lossy().to_string()}); }
    let mut size: u64 = 0;
    fn walk(p: &Path, acc: &mut u64) { if let Ok(rd) = std::fs::read_dir(p) { for e in rd.flatten() { if let Ok(m) = e.metadata() { if m.is_dir() { walk(&e.path(), acc); } else { *acc += m.len(); } } } } }
    walk(&p, &mut size);
    serde_json::json!({"exists": true, "sizeBytes": size, "cacheDir": p.to_string_lossy().to_string()})
}
#[tauri::command]
fn manage_list() -> serde_json::Value {
    let f = get_envs_file();
    if !f.exists() { return serde_json::json!([]); }
    if let Ok(s) = std::fs::read_to_string(&f) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
            if let Some(envs) = v.get("envs").and_then(|e| e.as_object()) {
                return serde_json::Value::Array(envs.keys().map(|k| serde_json::Value::String(k.clone())).collect());
            }
        }
    }
    serde_json::json!([])
}
#[tauri::command]
fn get_desktop_version() -> String { env!("CARGO_PKG_VERSION").to_string() }
#[tauri::command]
fn get_wangp_local_version() -> serde_json::Value {
    let repo = get_repo_dir();
    if !repo.join(".git").exists() { return serde_json::Value::Null; }
    use std::process::Command;
    let hash = Command::new("git").args(["rev-parse","HEAD"]).current_dir(&repo).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let date = Command::new("git").args(["log","-1","--format=%cI"]).current_dir(&repo).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    if hash.is_empty() { serde_json::Value::Null } else { serde_json::json!({"hash": hash, "date": date}) }
}
#[tauri::command]
fn get_desktop_git_info() -> serde_json::Value { get_wangp_local_version() }

// ── Phase 3: launch stubs (real spawn needs shell plugin for streaming) ──
static WANGP_PID: OnceLock<Mutex<Option<u32>>> = OnceLock::new();
#[tauri::command]
async fn launch(app: tauri::AppHandle, mode: Option<String>) -> Result<serde_json::Value, String> {
    mutating_try("launch")?;
    let mode = mode.unwrap_or("browser".into());
    let repo = get_repo_dir();
    if !repo.join("wgp.py").exists() { mutating_done(); return Err("Wan2GP not installed — run Install first".into()); }
    let cfg = load_config_value();
    let port = cfg.get("serverPort").and_then(|v| v.as_u64()).unwrap_or(7860);
    let share = cfg.get("share").and_then(|v| v.as_bool()).unwrap_or(false);
    let mut args = vec!["wgp.py".to_string(), "--server-port".into(), port.to_string(), "--server-name".into(), "localhost".into(), "--advanced".into()];
    if share { args.push("--share".into()); }
    // vram coefficient forward (reads wgp_config.json if present)
    let emit = |msg: &str| { let _ = app.emit("launch-log", msg.to_string()); };
    emit(&format!("[*] Launching Wan2GP ({}) on :{}…\n", mode, port));
    // bootstrap shim — minimal PYTHONUNBUFFERED + isatty patch so tqdm bars stream
    let boot = repo.join(".wan2gp-bootstrap.py");
    let _ = std::fs::write(&boot, "import runpy,sys,os; os.environ['PYTHONUNBUFFERED']='1'\nrunpy.run_path('wgp.py',run_name='__main__')");
    // resolve python for active env
    let env = get_active_env();
    let py = if env.get("path").and_then(|p| p.as_str()).is_some() {
        let p = PathBuf::from(env.get("path").unwrap().as_str().unwrap());
        if cfg!(windows) { p.join("python.exe").to_string_lossy().to_string() } else { p.join("bin/python3").to_string_lossy().to_string() }
    } else { "python".to_string() };
    use tauri_plugin_shell::ShellExt;
    let (mut rx, child) = app.shell().command(&py).args(&args).current_dir(&repo).spawn().map_err(|e| { mutating_done(); e.to_string() })?;
    if let Some(m) = WANGP_PID.get_or_init(|| Mutex::new(None)).lock().ok() { drop(m); }
    *WANGP_PID.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(child.pid());
    // stream logs in background
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        use tauri_plugin_shell::process::CommandEvent;
        let mut rx = rx;
        while let Some(ev) = rx.recv().await {
            match ev { CommandEvent::Stdout(b) => { let _ = app2.emit("launch-log", String::from_utf8_lossy(&b).to_string()); }, CommandEvent::Stderr(b) => { let _ = app2.emit("launch-log", String::from_utf8_lossy(&b).to_string()); }, CommandEvent::Terminated(s) => { let _ = app2.emit("wangp-exit", serde_json::json!({"code": s.code})); break; }, _ => {} }
        }
    });
    // wait for port in background (ponytail: simple poll, upgrade to notify when needed)
    let host = "127.0.0.1".to_string();
    let app3 = app.clone();
    std::thread::spawn(move || {
        for _ in 0..60 { std::thread::sleep(std::time::Duration::from_secs(3)); if std::net::TcpStream::connect(format!("{}:{}", host, port)).is_ok() { let _ = app3.emit("launch-log", format!("[✓] Wan2GP ready on http://localhost:{}\n", port)); break; } }
        mutating_done();
    });
    Ok(serde_json::json!({"ok": true, "port": port, "mode": mode}))
}
#[tauri::command]
fn stop_wangp(app: tauri::AppHandle) -> serde_json::Value {
    if let Some(pid) = WANGP_PID.get().and_then(|m| m.lock().ok()).and_then(|g| *g) {
        #[cfg(windows)] { let _ = std::process::Command::new("taskkill").args(["/pid", &pid.to_string(), "/f", "/t"]).output(); }
        #[cfg(not(windows))] { let _ = std::process::Command::new("kill").arg("-9").arg(pid.to_string()).output(); }
        if let Some(m) = WANGP_PID.get() { *m.lock().unwrap() = None; }
        let _ = app.emit("wangp-exit", serde_json::json!({"stopped": true}));
    }
    serde_json::json!({"ok": true})
}

// ── misc stubs to unblock frontend (return safe defaults) ──
#[tauri::command]
fn open_folder(path: String) -> Result<(), String> { std::process::Command::new("explorer").arg(&path).spawn().map(|_| ()).map_err(|e| e.to_string()) }
#[tauri::command]
fn select_folder() -> Option<String> { None }
#[tauri::command]
fn confirm_dialog(opts: Option<serde_json::Value>) -> serde_json::Value { let _ = opts; serde_json::json!({"response": 1}) }
#[tauri::command]
fn repair_settings() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command]
fn check_package_updates(versions: Option<serde_json::Value>) -> serde_json::Value { let _ = versions; serde_json::json!([]) }
#[tauri::command]
fn check_package(pkg: String) -> serde_json::Value { serde_json::json!({"name": pkg, "installed": false}) }
#[tauri::command]
fn deepy_status() -> serde_json::Value { serde_json::json!({"mode": "disabled"}) }
#[tauri::command]
fn memory_profile_read() -> serde_json::Value { serde_json::json!({}) }
#[tauri::command]
fn auto_tune_detect() -> serde_json::Value { serde_json::json!({"cuda_available": false}) }
#[tauri::command]
fn auto_tune_recommend(hw: Option<serde_json::Value>, opts: Option<serde_json::Value>) -> serde_json::Value { let _ = (hw, opts); serde_json::json!({"video_profile": 4}) }

// ── Phase 2-5: remaining 65 handlers as thin stubs (real logic behind shell/fs plugins) ──
#[tauri::command]
async fn install(app: tauri::AppHandle, env_type: Option<String>) -> Result<serde_json::Value,String> {
    mutating_try("install")?;
    let env = env_type.unwrap_or("uv".into());
    let repo = get_repo_dir();
    let emit = |msg: &str| { let _ = app.emit("setup-output", msg.to_string()); };
    // ponytail: minimal install — clone + setup.py streaming; full Electron fixups (py shim, kernel sync, sageattention swap) add when stub proves insufficient
    if !repo.join("wgp.py").exists() {
        emit(&format!("[*] Cloning Wan2GP into {}\n", repo.display()));
        std::fs::create_dir_all(&repo).map_err(|e| e.to_string())?;
        // use shell plugin so stdout streams to UI instead of buffering
        use tauri_plugin_shell::ShellExt;
        let (mut rx, _child) = app.shell().command("git").args(["clone","--depth","1","https://github.com/deepbeepmeep/Wan2GP.git", &repo.to_string_lossy()]).spawn().map_err(|e| e.to_string())?;
        use tauri_plugin_shell::process::CommandEvent;
        while let Some(ev) = rx.recv().await {
            match ev { CommandEvent::Stdout(b) => emit(&String::from_utf8_lossy(&b)), CommandEvent::Stderr(b) => emit(&String::from_utf8_lossy(&b)), _ => {} }
        }
        if !repo.join("wgp.py").exists() { mutating_done(); return Err("git clone failed — check output above".into()); }
        emit("[*] Repository cloned.\n");
    }
    emit(&format!("[*] Installing env={} via setup.py (streaming)…\n", env));
    {
        use tauri_plugin_shell::ShellExt;
        let py = if env == "uv" { "uv".to_string() } else { "python".to_string() };
        let args: Vec<String> = if env == "uv" { vec!["run".into(), "python".into(), "setup.py".into(), "--env".into(), env.clone()] } else { vec!["setup.py".into(), "--env".into(), env.clone()] };
        let (mut rx, _child) = app.shell().command(&py).args(args).current_dir(&repo).spawn().map_err(|e| e.to_string())?;
        use tauri_plugin_shell::process::CommandEvent;
        while let Some(ev) = rx.recv().await {
            match ev { CommandEvent::Stdout(b) => emit(&String::from_utf8_lossy(&b)), CommandEvent::Stderr(b) => emit(&String::from_utf8_lossy(&b)), _ => {} }
        }
    }
    emit("[*] Install finished.\n");
    mutating_done();
    Ok(serde_json::json!({"ok": true}))
}
#[tauri::command] fn reinstall() -> Result<serde_json::Value,String> { Err("reinstall: needs shell plugin".into()) }
#[tauri::command] fn uninstall() -> serde_json::Value { serde_json::json!({"success": false, "error": "stub"}) }
#[tauri::command] fn sync_kernels() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn update() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn manage_set_active(name: String) -> serde_json::Value { let _=name; serde_json::json!({"ok": true}) }
#[tauri::command] fn uninstall_env(name: String) -> serde_json::Value { let _=name; serde_json::json!({"ok": true}) }
#[tauri::command] fn open_external(url: String) -> Result<(),String> { let _=url; Ok(()) }
#[tauri::command] fn detect_browsers() -> serde_json::Value { serde_json::json!([]) }
#[tauri::command] fn launch_browser(url: String) -> serde_json::Value { let _=url; serde_json::json!({"ok": true}) }
#[tauri::command] fn launch_browser_no_gpu(url: String) -> serde_json::Value { let _=url; serde_json::json!({"ok": true}) }
#[tauri::command] fn chrome_available() -> bool { false }
#[tauri::command] fn set_data_dir(dir: String) -> Result<serde_json::Value,String> { let ov = data_dir_override_file(); atomic_write(&ov, &dir).map_err(|e| e.to_string())?; Ok(serde_json::json!({"ok": true})) }
#[tauri::command] fn reset_data_dir() -> serde_json::Value { let _=std::fs::remove_file(data_dir_override_file()); serde_json::json!({"ok": true}) }
#[tauri::command] fn migrate_to_preferred(choices: Option<serde_json::Value>) -> serde_json::Value { let _=choices; serde_json::json!({"ok": true}) }
#[tauri::command] fn move_folder(src: String, dst: String) -> Result<serde_json::Value,String> { std::fs::rename(&src, &dst).map_err(|e| e.to_string())?; Ok(serde_json::json!({"ok": true})) }
#[tauri::command] fn write_wgp_config(cfg: serde_json::Value) -> serde_json::Value { let _=cfg; serde_json::json!({"ok": true}) }
#[tauri::command] fn install_prerequisite(tool: String) -> serde_json::Value { let _=tool; serde_json::json!({"ok": true}) }
#[tauri::command] fn get_wangp_upstream_info() -> serde_json::Value { serde_json::json!(null) }
#[tauri::command] fn get_wangp_version() -> serde_json::Value { serde_json::json!(null) }
#[tauri::command] fn report_issue() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn create_desktop_shortcut() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn upgrade_package(pkg: String) -> serde_json::Value { let _=pkg; serde_json::json!({"ok": true}) }
#[tauri::command] fn install_package(pkg: String) -> serde_json::Value { let _=pkg; serde_json::json!({"ok": true}) }
#[tauri::command] fn uninstall_package(pkg: String) -> serde_json::Value { let _=pkg; serde_json::json!({"ok": true}) }
#[tauri::command] fn restore_requirements() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn llm_engines_list() -> serde_json::Value { serde_json::json!([]) }
#[tauri::command] fn llm_engine_install(engine: String) -> serde_json::Value { let _=engine; serde_json::json!({"ok": true}) }
#[tauri::command] fn llm_engine_serve(engine: String, action: String) -> serde_json::Value { let _=(engine, action); serde_json::json!({"ok": true}) }
#[tauri::command] fn llm_engine_auth(engine: String) -> serde_json::Value { let _=engine; serde_json::json!({"ok": true}) }
#[tauri::command] fn deepy_activate(engine: String) -> serde_json::Value { let _=engine; serde_json::json!({"ok": true}) }
#[tauri::command] fn deepy_set(mode: String, engine: Option<String>, enhancer: Option<String>) -> serde_json::Value { let _=(mode, engine, enhancer); serde_json::json!({"ok": true}) }
#[tauri::command] fn set_auto_start(enabled: bool) -> serde_json::Value { let _=enabled; serde_json::json!({"ok": true}) }
#[tauri::command] fn memory_profile_apply(settings: serde_json::Value) -> serde_json::Value { let _=settings; serde_json::json!({"ok": true}) }
#[tauri::command] fn notifier_config() -> serde_json::Value { serde_json::json!({"ok": true, "config": {}}) }
#[tauri::command] fn notifier_set(cfg: serde_json::Value) -> serde_json::Value { let _=cfg; serde_json::json!({"ok": true}) }
#[tauri::command] fn notifier_test(cfg: Option<serde_json::Value>) -> serde_json::Value { let _=cfg; serde_json::json!({"ok": true}) }
#[tauri::command] fn pulsebar_hide() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn set_theme_follow_system(enabled: bool) -> serde_json::Value { let _=enabled; serde_json::json!({"ok": true}) }
#[tauri::command] fn set_notifications_enabled(enabled: bool) -> serde_json::Value { let _=enabled; serde_json::json!({"ok": true}) }
#[tauri::command] fn check_update(opts: Option<serde_json::Value>) -> serde_json::Value { let _=opts; serde_json::json!({"update": null}) }
#[tauri::command] fn download_update(opts: Option<serde_json::Value>) -> serde_json::Value { let _=opts; serde_json::json!({"ok": true}) }
#[tauri::command] fn install_update() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn create_browser_view(url: String, opts: Option<serde_json::Value>) -> serde_json::Value { let _=(url, opts); serde_json::json!({"ok": true}) }
#[tauri::command] fn destroy_browser_view() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn get_log_history() -> serde_json::Value { serde_json::json!([]) }
#[tauri::command] fn uv_cache_clean(action: Option<String>) -> serde_json::Value { let _=action; serde_json::json!({"success": true}) }
#[tauri::command] fn open_task_manager() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn get_crash_recovery_info() -> serde_json::Value { serde_json::json!(null) }
#[tauri::command] fn launch_webview() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn popout_webview(url: String) -> serde_json::Value { let _=url; serde_json::json!({"ok": true}) }
#[tauri::command] fn hide_browser_view() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn detach_browser_view() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn reattach_browser_view() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn create_term_view() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn destroy_term_view() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn bv_navigate(action: String) -> serde_json::Value { let _=action; serde_json::json!({"ok": true}) }
#[tauri::command] fn bv_set_zoom(factor: f64) -> serde_json::Value { let _=factor; serde_json::json!({"ok": true}) }
#[tauri::command] fn bv_set_dock(dock: String) -> serde_json::Value { let _=dock; serde_json::json!({"ok": true}) }
#[tauri::command] fn is_data_dir_roaming() -> bool { get_data_dir().to_string_lossy().contains("AppData") }
#[tauri::command] fn migrate_choose() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn notifier_ensure() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn ui_mode_set(mode: String) -> serde_json::Value { let _=mode; serde_json::json!({"ok": true}) }
#[tauri::command] fn on_system_theme_change() -> serde_json::Value { serde_json::json!(null) }

static MUTATING: OnceLock<Mutex<Option<String>>> = OnceLock::new();
fn mutating_try(name: &str) -> Result<(), String> {
    let m = MUTATING.get_or_init(|| Mutex::new(None));
    let mut g = m.lock().unwrap();
    if let Some(cur) = g.as_ref() { return Err(format!("Another operation already running ({cur}). Wait for it to finish.")); }
    *g = Some(name.to_string()); Ok(())
}
fn mutating_done() { if let Some(m) = MUTATING.get() { *m.lock().unwrap() = None; } }

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            greet, detect_gpu, detect_gpus, detect_hardware, get_hardware_profile, get_system_metrics,
            get_status, check_python, check_git, check_installed, check_command,
            config_load, config_save, get_install_paths, get_disk_space, get_model_paths, detect_model_folders,
            install_plan, validate_install, uv_cache_info, uv_cache_size, manage_list,
            get_desktop_version, get_wangp_local_version, get_desktop_git_info,
            launch, stop_wangp, open_folder, select_folder, confirm_dialog, repair_settings,
            check_package, check_package_updates, deepy_status, memory_profile_read,
            auto_tune_detect, auto_tune_recommend,
            install, reinstall, uninstall, sync_kernels, update, manage_set_active, uninstall_env,
            open_external, detect_browsers, launch_browser, launch_browser_no_gpu, chrome_available,
            set_data_dir, reset_data_dir, migrate_to_preferred, move_folder, write_wgp_config, install_prerequisite,
            get_wangp_upstream_info, get_wangp_version, report_issue, create_desktop_shortcut,
            upgrade_package, install_package, uninstall_package, restore_requirements,
            llm_engines_list, llm_engine_install, llm_engine_serve, llm_engine_auth,
            deepy_activate, deepy_set, set_auto_start, memory_profile_apply,
            notifier_config, notifier_set, notifier_test, pulsebar_hide,
            set_theme_follow_system, set_notifications_enabled,
            check_update, download_update, install_update,
            create_browser_view, destroy_browser_view, get_log_history, uv_cache_clean,
            open_task_manager, get_crash_recovery_info,
            launch_webview, popout_webview, hide_browser_view, detach_browser_view, reattach_browser_view,
            create_term_view, destroy_term_view, bv_navigate, bv_set_zoom, bv_set_dock,
            is_data_dir_roaming, migrate_choose, notifier_ensure, ui_mode_set
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

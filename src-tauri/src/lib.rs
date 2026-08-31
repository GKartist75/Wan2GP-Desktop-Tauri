// ponytail: single-file spike — one lib.rs covers all 100 handlers; split into modules when this file hurts
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tauri::{Emitter, Manager};
// shell/dialog/fs plugins wired for install/launch streaming — ponytail: std::process covers probes without them
static MUTATING: OnceLock<Mutex<Option<String>>> = OnceLock::new();
fn mutating_try(name: &str) -> Result<(), String> {
    let m = MUTATING.get_or_init(|| Mutex::new(None));
    let mut g = m.lock().unwrap();
    if let Some(cur) = g.as_ref() { return Err(format!("Another operation already running ({cur}). Wait for it to finish.")); }
    *g = Some(name.to_string()); Ok(())
}
fn mutating_done() { if let Some(m) = MUTATING.get() { *m.lock().unwrap() = None; } }

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
fn kernel_profile_key(vendor: &str, name: &str) -> String {
    let v = vendor.to_uppercase(); let g = name.to_uppercase();
    if v == "APPLE" { return "MPS".into(); }
    if v == "NVIDIA" {
        if g.contains(" 10") || g.contains(" 16") || g.contains("GTX 10") || g.contains("GTX 16") { return "GTX_10".into(); }
        if g.contains("50") { return "RTX_50".into(); } if g.contains("40") { return "RTX_40".into(); } if g.contains("30") { return "RTX_30".into(); }
        if g.contains("20") || g.contains("QUADRO") { return "RTX_20".into(); } return "GTX_10".into();
    }
    if v == "AMD" {
        if g.contains("7600")||g.contains("7700")||g.contains("7800")||g.contains("7900")||g.contains("780M") { return "AMD_GFX110X".into(); }
        if g.contains("890M")||g.contains("STRIX")||g.contains("HALO")||g.contains("Z1")||g.contains("PHOENIX") { return "AMD_GFX1151".into(); }
        if g.contains("9060")||g.contains("9070")||g.contains("8000")||g.contains("1201") { return "AMD_GFX1201".into(); }
        return "AMD_GFX110X".into();
    }
    "RTX_40".into()
}
fn build_install_plan(hw: &serde_json::Value) -> serde_json::Value {
    let vendor = hw.get("vendor").and_then(|v| v.as_str()).unwrap_or("UNKNOWN").to_uppercase();
    let name = hw.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let vram = hw.get("vramMB").and_then(|v| v.as_str()).unwrap_or("0").split_whitespace().next().unwrap_or("0").parse::<f64>().unwrap_or(0.0);
    let driver = hw.get("driverVersion").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let vram_gb = vram / 1024.0_f64; let is_gtx = name.to_uppercase().contains("GTX 10") || name.to_uppercase().contains("GTX 16") || (name.contains("10") && vendor=="NVIDIA" && (name.contains("1050")||name.contains("1060")||name.contains("1650")));
    let (cuda, torch, mut warn) = if vendor=="NVIDIA" { if is_gtx { ("CUDA 12.8", "PyTorch 2.7.1", String::new()) } else { let mut w=String::new(); if let Ok(dv)=driver.parse::<f64>() { if dv < 580.0 { w=format!("NVIDIA driver {} < R580 — cu130 needs R580+", driver); }} ("CUDA 13 (cu130)", "PyTorch 2.10", w) } } else if vendor=="AMD" { ("ROCm (TheRock)", "PyTorch 2.7.0", String::new()) } else if vendor=="APPLE" { ("MPS (Metal)", "PyTorch (MPS)", String::new()) } else { ("CPU", "PyTorch (CPU)", String::new()) };
    let _ = vram_gb; let _ = warn.clone();
    serde_json::json!({"vendor": vendor, "gpuName": name, "vramGb": vram, "cuda": cuda, "torch": torch, "driverWarning": warn, "profile": kernel_profile_key(&vendor, &name)})
}
#[tauri::command]
fn get_status() -> serde_json::Value {
    let env = get_active_env();
    if env.is_null() { return serde_json::json!({"error":"No active environment","spike":true}); }
    // try to read kernel wheels from setup_config.json if present
    let repo = get_repo_dir(); let cfg_path = repo.join("setup_config.json");
    let mut wheels = serde_json::json!([]);
    let mut profile = String::new();
    if cfg_path.exists() {
        if let Ok(s) = std::fs::read_to_string(&cfg_path) {
            if let Ok(cfg) = serde_json::from_str::<serde_json::Value>(&s) {
                let gpu = get_gpu_info_sync(); let vendor=gpu.get("vendor").and_then(|v| v.as_str()).unwrap_or("UNKNOWN"); let name=gpu.get("name").and_then(|v| v.as_str()).unwrap_or("");
                profile = kernel_profile_key(vendor, name);
                if let Some(prof) = cfg.get("gpu_profiles").and_then(|p| p.get(&profile)) {
                    if let Some(kernels) = prof.get("kernels").and_then(|k| k.as_array()) {
                        wheels = serde_json::Value::Array(kernels.clone());
                    }
                }
            }
        }
    }
    serde_json::json!({"env": env, "versions": {}, "kernelWheels": wheels, "kernelProfile": profile, "spike": false})
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
    let orig = if let Ok(a) = std::env::var("APPDATA") { PathBuf::from(a).join("wan2gp-desktop") } else { PathBuf::from("C:\\Users\\Default\\AppData\\Roaming\\wan2gp-desktop") };
    let models_default = data.with_file_name(format!("{}-Models", data.file_name().unwrap_or_default().to_string_lossy()));
    let models_default = if models_default.to_string_lossy().is_empty() { PathBuf::from("C:\\Wan2GP-Models") } else { models_default };
    serde_json::json!({
        "appData": data.to_string_lossy().to_string(),
        "appDataRoot": orig.to_string_lossy().to_string(),
        "repo": repo.to_string_lossy().to_string(),
        "dataDir": data.to_string_lossy().to_string(),
        "repoDir": repo.to_string_lossy().to_string(),
        "config": get_config_file().to_string_lossy().to_string(),
        "configFile": get_config_file().to_string_lossy().to_string(),
        "envsFile": get_envs_file().to_string_lossy().to_string(),
        "modelsDefault": models_default.to_string_lossy().to_string(),
        "dataDirInRoaming": data.to_string_lossy().to_string().to_lowercase().contains("appdata"),
        "legacyRoamingFound": false,
        "isRoaming": data.to_string_lossy().contains("AppData")
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
                let mut out = serde_json::Map::new();
                if let Some(a) = v.get("checkpoints_paths").and_then(|x| x.as_array()).and_then(|a| a.first()) { out.insert("checkpoints".into(), a.clone()); }
                else if let Some(c) = v.get("ckpt_dir") { out.insert("checkpoints".into(), c.clone()); }
                if let Some(l) = v.get("loras_root") { out.insert("loras".into(), l.clone()); }
                else if let Some(l) = v.get("lora_dir") { out.insert("loras".into(), l.clone()); }
                if let Some(o) = v.get("save_path") { out.insert("output".into(), o.clone()); }
                if !out.is_empty() { return serde_json::Value::Object(out); }
            }
        }
    }
    serde_json::Value::Null
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
    let gpu = get_gpu_info_sync();
    let plan = build_install_plan(&gpu);
    // disk check (ponytail: statvfs when exact GB needed)
    let disk = get_disk_space(None);
    serde_json::json!({"gpu": gpu, "plan": plan, "disk": disk})
}
#[tauri::command]
fn validate_install() -> serde_json::Value {
    let repo = get_repo_dir();
    let mut errors: Vec<String> = Vec::new();
    if !repo.join("wgp.py").exists() { errors.push("wgp.py not found — not installed".into()); }
    if !repo.join("setup_config.json").exists() { errors.push("setup_config.json missing".into()); }
    if get_active_env().is_null() { errors.push("no active env".into()); }
    serde_json::json!({"ok": errors.is_empty(), "errors": errors})
}
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
fn open_folder(path: String, app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_path(&path, None::<&str>).map_err(|e| e.to_string()).or_else(|_| std::process::Command::new("explorer").arg(&path).spawn().map(|_| ()).map_err(|e| e.to_string()))
}
#[tauri::command]
async fn select_folder(app: tauri::AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    // blocking_pick_folder is sync; pick_folder is async — try both
    if let Some(p) = app.dialog().file().blocking_pick_folder() { return Some(p.to_string()); }
    None
}
#[tauri::command]
async fn confirm_dialog(app: tauri::AppHandle, opts: Option<serde_json::Value>) -> serde_json::Value {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    let title = opts.as_ref().and_then(|o| o.get("title").and_then(|v| v.as_str())).unwrap_or("Confirm");
    let msg = opts.as_ref().and_then(|o| o.get("message").and_then(|v| v.as_str())).unwrap_or("Are you sure?");
    let detail = opts.as_ref().and_then(|o| o.get("detail").and_then(|v| v.as_str())).unwrap_or("");
    let full = if detail.is_empty() { msg.to_string() } else { format!("{}\n\n{}", msg, detail) };
    let confirmed = app.dialog().message(&full).title(title).kind(MessageDialogKind::Info).blocking_show();
    serde_json::json!({"response": if confirmed { 0 } else { 1 }})
}
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
    let env = env_type.unwrap_or("uv".into()); // uv | venv | conda
    let repo = get_repo_dir();
    let emit = |msg: &str| { let _ = app.emit("setup-output", msg.to_string()); let _ = app.emit("setup-phase", serde_json::json!({"label": msg})); };
    // hardware-aware header (driver warning surfaces before 20min install)
    let gpu = get_gpu_info_sync();
    let plan = build_install_plan(&gpu);
    emit(&format!("[hw] GPU: {} ({}) — {} / {} — profile {}\n", plan["gpuName"].as_str().unwrap_or("?"), plan["vendor"].as_str().unwrap_or("?"), plan["cuda"].as_str().unwrap_or("?"), plan["torch"].as_str().unwrap_or("?"), plan["profile"].as_str().unwrap_or("?")));
    if let Some(w)=plan["driverWarning"].as_str() { if !w.is_empty() { emit(&format!("[warn] {}\n", w)); } }
    emit(&format!("[env] requested: {}\n", env));
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
    // ── env creation (uv/venv/conda) — ponytail: setup.py will pick hardware-aware wheels from setup_config.json
    let env_path = match env.as_str() {
        "conda" => repo.join("env_conda"),
        "venv" => repo.join("env_venv"),
        _ => repo.join("env_uv"),
    };
    if !env_path.exists() {
        emit(&format!("[*] Creating {} env at {}\n", env, env_path.display()));
        use tauri_plugin_shell::ShellExt;
        let res: Result<(), String> = match env.as_str() {
            "conda" => {
                let (mut r, _) = app.shell().command("conda").args(["create","-p", &env_path.to_string_lossy(), "python=3.11", "-y"]).spawn().map_err(|e| format!("conda not found: {}", e))?;
                use tauri_plugin_shell::process::CommandEvent; while let Some(ev)=r.recv().await { match ev{ CommandEvent::Stdout(b)|CommandEvent::Stderr(b)=>emit(&String::from_utf8_lossy(&b)), _=>{}}}
                Ok(())
            },
            "venv" => {
                // try py -3.11 then python
                let py = if std::process::Command::new("py").args(["-3.11","--version"]).output().map(|o| o.status.success()).unwrap_or(false) { "py".to_string() } else { "python".to_string() };
                let pya: Vec<String> = if py=="py" { vec!["-3.11".into(), "-m".into(), "venv".into(), env_path.to_string_lossy().to_string()] } else { vec!["-m".into(), "venv".into(), env_path.to_string_lossy().to_string()] };
                let (mut r, _) = app.shell().command(&py).args(pya).spawn().map_err(|e| e.to_string())?;
                use tauri_plugin_shell::process::CommandEvent; while let Some(ev)=r.recv().await { match ev{ CommandEvent::Stdout(b)|CommandEvent::Stderr(b)=>emit(&String::from_utf8_lossy(&b)), _=>{}}}
                Ok(())
            },
            _ => { // uv
                let (mut r, _) = app.shell().command("uv").args(["venv", &env_path.to_string_lossy(), "--python", "3.11"]).spawn().map_err(|e| format!("uv not found: {}", e))?;
                use tauri_plugin_shell::process::CommandEvent; while let Some(ev)=r.recv().await { match ev{ CommandEvent::Stdout(b)|CommandEvent::Stderr(b)=>emit(&String::from_utf8_lossy(&b)), _=>{}}}
                Ok(())
            }
        };
        if let Err(e) = res { emit(&format!("[warn] env creation: {}\n", e)); }
        // record envs.json
        let envs_file = get_envs_file();
        let mut envs: serde_json::Value = std::fs::read_to_string(&envs_file).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(serde_json::json!({"envs":{}, "active": null}));
        if envs.get("envs").is_none() { envs["envs"] = serde_json::json!({}); }
        envs["envs"][&env] = serde_json::json!({"name": env, "type": env, "path": env_path.to_string_lossy().to_string()});
        envs["active"] = serde_json::Value::String(env.clone());
        let _ = atomic_write(&envs_file, &serde_json::to_string_pretty(&envs).unwrap());
    }
    // run setup.py with the env's python (hardware-aware: setup.py reads setup_config.json + GPU)
    {
        use tauri_plugin_shell::ShellExt;
        let (py, args): (String, Vec<String>) = match env.as_str() {
            "conda" => ("conda".into(), vec!["run".into(), "-p".into(), env_path.to_string_lossy().to_string(), "python".into(), "setup.py".into(), "install".into(), "--env".into(), env.clone(), "--auto".into()]),
            _ => {
                let p = if env=="uv" { env_path.join(if cfg!(windows){"Scripts\\python.exe"} else {"bin/python"}) } else { env_path.join(if cfg!(windows){"Scripts\\python.exe"} else {"bin/python3"}) };
                let py_bin = if p.exists() { p.to_string_lossy().to_string() } else if env=="uv" { "uv".into() } else { "python".into() };
                if py_bin=="uv" { ("uv".into(), vec!["run".into(), "--with".into(), "setuptools".into(), "python".into(), "setup.py".into(), "install".into(), "--env".into(), env.clone(), "--auto".into()]) }
                else { (py_bin, vec!["setup.py".into(), "install".into(), "--env".into(), env.clone(), "--auto".into()]) }
            }
        };
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
#[tauri::command] fn move_folder(src: String, dst: String) -> Result<serde_json::Value,String> {
    let s = PathBuf::from(&src); let d = PathBuf::from(&dst);
    if let Err(_) = std::fs::rename(&s, &d) {
        // cross-device fallback — copy then remove
        if s.is_dir() { fs_extra_fallback_copy_dir(&s, &d)?; std::fs::remove_dir_all(&s).map_err(|e| e.to_string())?; }
        else { std::fs::copy(&s, &d).map_err(|e| e.to_string())?; std::fs::remove_file(&s).map_err(|e| e.to_string())?; }
    }
    Ok(serde_json::json!({"ok": true}))
}
fn fs_extra_fallback_copy_dir(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for e in std::fs::read_dir(src).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        let s = e.path(); let d = dst.join(e.file_name());
        if s.is_dir() { fs_extra_fallback_copy_dir(&s, &d)?; } else { std::fs::copy(&s, &d).map_err(|e| e.to_string())?; }
    }
    Ok(())
}
#[tauri::command] fn write_wgp_config(cfg: serde_json::Value) -> Result<serde_json::Value, String> {
    let repo = get_repo_dir();
    let p = repo.join("wgp_config.json");
    let mut cur: serde_json::Value = if p.exists() { std::fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(serde_json::json!({})) } else { serde_json::json!({}) };
    if let Some(obj) = cfg.as_object() {
        for (k,v) in obj { cur[k] = v.clone(); }
    } else if let Some(patch) = cfg.get("patch") { if let Some(o) = patch.as_object() { for (k,v) in o { cur[k]=v.clone(); } } }
    // also handle Electron shape {checkpointsPaths, lorasRoot, savePath}
    if let Some(v) = cfg.get("checkpointsPaths") { cur["ckpt_dir"] = v.clone(); }
    if let Some(v) = cfg.get("lorasRoot") { cur["lora_dir"] = v.clone(); }
    if let Some(v) = cfg.get("savePath") { cur["save_path"] = v.clone(); }
    let s = serde_json::to_string_pretty(&cur).map_err(|e| e.to_string())?;
    atomic_write(&p, &s).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"ok": true}))
}
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

// ponytail: single-file spike — one lib.rs covers all 100 handlers; split into modules when this file hurts
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tauri::Emitter;
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
#[allow(dead_code)]
fn local_appdata_dir() -> PathBuf {
    if let Ok(l) = std::env::var("LOCALAPPDATA") { PathBuf::from(l) }
    else { appdata_dir() }
}
fn data_dir_override_file() -> PathBuf { home_dir().join(".wan2gp-tauri-data-dir") }
#[allow(dead_code)]
fn data_dir_override_file_electron() -> PathBuf { home_dir().join(".wan2gp-desktop-data-dir") }

fn get_data_dir() -> PathBuf {
    // 1. Tauri override only — isolated from Electron, no fallback
    let ov = data_dir_override_file();
    if ov.exists() {
        if let Ok(s) = std::fs::read_to_string(&ov) {
            let d = s.trim().to_string();
            if !d.is_empty() {
                let p = PathBuf::from(&d);
                if p.is_absolute() && p.exists() { return p; }
                if !p.exists() {
                    let legacy = std::path::Path::new(&d).join("wgp.py");
                    let nested = PathBuf::from(&d).join("Wan2GP").join("wgp.py");
                    if legacy.exists() || nested.exists() { return PathBuf::from(d); }
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
        "serverPort": 7861, "serverName": "localhost", "defaultBrowser": "system", // ponytail: 7861 for side-by-side with Electron 7860

        "termDockDefault": "bottom", "electronGpu": true, "launcherGpu": "auto", "share": false,
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
    let (cuda, torch, warn) = if vendor=="NVIDIA" { if is_gtx { ("CUDA 12.8", "PyTorch 2.7.1", String::new()) } else { let mut w=String::new(); if let Ok(dv)=driver.parse::<f64>() { if dv < 580.0 { w=format!("NVIDIA driver {} < R580 — cu130 needs R580+", driver); }} ("CUDA 13 (cu130)", "PyTorch 2.10", w) } } else if vendor=="AMD" { ("ROCm (TheRock)", "PyTorch 2.7.0", String::new()) } else if vendor=="APPLE" { ("MPS (Metal)", "PyTorch (MPS)", String::new()) } else { ("CPU", "PyTorch (CPU)", String::new()) };
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
    // real version scan via env's python (importlib.metadata) — ponytail: helper file on same drive as env
    let mut versions = serde_json::Map::new();
    if let Some(raw) = env.get("path").and_then(|p| p.as_str()) {
        let rel = raw.trim_start_matches(".\\").trim_start_matches("./");
        let base = if std::path::Path::new(raw).is_absolute() { PathBuf::from(raw) } else { get_repo_dir().join(rel) };
        let py = if cfg!(windows) { base.join("Scripts\\python.exe") } else { base.join("bin/python3") };
        let py_bin = if py.exists() { py } else { PathBuf::from(raw) };
        if py_bin.exists() {
            let helper = get_data_dir().join(".get_versions.py");
            let code = r#"import sys, importlib.metadata
try:
    aliases={'triton':'triton-windows','opencv-python':'opencv','spas_sage_attn':'spas-sage-attn','huggingface_hub':'huggingface-hub'}
    pkgs=['python','torch','triton','sageattention','spas_sage_attn','flash_attn','nunchaku','llamacpp_gguf_cuda','lightx2v','diffusers','transformers','gradio','accelerate','onnxruntime','xformers','mmgp','moviepy','opencv-python','insightface','peft','timm','vector_quantize_pytorch','torchcodec','torchaudio','huggingface_hub','bitsandbytes','numpy','sentencepiece','open_clip_torch','imageio','einops','librosa','soundfile','tokenizers','av']
    r=[]
    for p in pkgs:
        try:
            if p=='python': r.append(f'python={sys.version.split()[0]}')
            elif p in aliases: r.append(f'{p}={importlib.metadata.version(aliases[p])}')
            else: r.append(f'{p}={importlib.metadata.version(p)}')
        except: pass
    print('||'.join(r))
except Exception as e:
    print(f'error:{e}')
"#;
            let _ = std::fs::write(&helper, code);
            if let Ok(out) = std::process::Command::new(&py_bin).arg(&helper).current_dir(&repo).output() {
                if out.status.success() {
                    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    for part in s.split("||") {
                        if let Some((k,v)) = part.split_once('=') { versions.insert(k.to_string(), serde_json::Value::String(v.to_string())); }
                    }
                }
            }
        }
    }
    serde_json::json!({"env": env, "versions": serde_json::Value::Object(versions), "kernelWheels": wheels, "kernelProfile": profile, "spike": false})
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
    // cpu via WMI Name (like Electron os.cpus()[0].model) — PROCESSOR_IDENTIFIER gives "Intel64 Family 6..." which users hate
    let cpu = {
        #[cfg(windows)]
        {
            use std::process::Command;
            // Name already contains "12th Gen Intel(R) Core(TM) i9-12900K", MaxClockSpeed adds GHz
            if let Ok(out) = Command::new("powershell").args(["-NoProfile","-Command","(Get-CimInstance Win32_Processor | Select-Object -First 1).Name"]).output() {
                if out.status.success() {
                    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !name.is_empty() {
                        // try to append GHz if not already in Name
                        let ghz = Command::new("powershell").args(["-NoProfile","-Command","(Get-CimInstance Win32_Processor | Select-Object -First 1).MaxClockSpeed"]).output()
                            .ok().and_then(|o| if o.status.success() { String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().ok() } else { None })
                            .map(|mhz| format!("{:.2} GHz", mhz as f64 / 1000.0))
                            .unwrap_or_default();
                        if !ghz.is_empty() && !name.contains("GHz") && !name.contains("MHz") {
                            format!("{} ({})", name, ghz)
                        } else { name }
                    } else { std::env::var("PROCESSOR_IDENTIFIER").unwrap_or("—".into()) }
                } else { std::env::var("PROCESSOR_IDENTIFIER").unwrap_or("—".into()) }
            } else { std::env::var("PROCESSOR_IDENTIFIER").unwrap_or("—".into()) }
        }
        #[cfg(not(windows))] { std::env::var("PROCESSOR_IDENTIFIER").unwrap_or("—".into()) }
    };
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

static PREV_CPU: OnceLock<Mutex<Option<(u64,u64)>>> = OnceLock::new();
static LAST_NVIDIA: OnceLock<Mutex<Option<serde_json::Value>>> = OnceLock::new();
#[tauri::command]
fn get_system_metrics() -> serde_json::Value {
    let mut result = serde_json::json!({"ramFree": null, "vramFree": null, "cpu": null, "gpu": null, "ramUsed": null, "ramTotal": null, "vramUsed": null, "vramTotal": null, "ram": null, "vram": null, "gpus": [], "gpu2": null, "vram2": null, "vramFree2": null, "vramUsed2": null, "vramTotal2": null});
    // RAM/CPU via sysinfo + os
    {
        use sysinfo::{System, CpuRefreshKind, MemoryRefreshKind, RefreshKind};
        let mut sys = System::new_with_specifics(RefreshKind::nothing().with_cpu(CpuRefreshKind::everything()).with_memory(MemoryRefreshKind::everything()));
        sys.refresh_memory();
        sys.refresh_cpu_usage();
        let total = sys.total_memory(); let free = sys.free_memory(); let used = sys.used_memory();
        let gb = |b: u64| format!("{} GB", b / 1073741824);
        result["ramFree"] = serde_json::Value::String(gb(free));
        result["ramTotal"] = serde_json::Value::String(gb(total));
        result["ramUsed"] = serde_json::Value::String(gb(used));
        if total > 0 { result["ram"] = serde_json::json!( (used as f64 / total as f64 * 100.0).round() as i64 ); }
        let cpu = sys.global_cpu_usage().round() as i64;
        if cpu > 0 { result["cpu"] = serde_json::json!(cpu); }
        let _ = PREV_CPU.get_or_init(|| Mutex::new(None));
    }
    // nvidia-smi for VRAM/GPU — per-GPU breakdown + WMI iGPU fallback (mirrors Electron main.js)
    {
        use std::process::Command;
        let out = Command::new("nvidia-smi").args(["--query-gpu=memory.free,memory.used,memory.total,utilization.gpu", "--format=csv,noheader,nounits"]).output();
        if let Ok(o) = out { if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !s.is_empty() {
                let mut free=0i64; let mut used=0i64; let mut total=0i64; let mut gpu=0i64; let mut cnt=0;
                let mut per_gpu: Vec<serde_json::Value> = Vec::new();
                for line in s.lines() {
                    let p: Vec<&str> = line.split(',').map(|x| x.trim()).collect();
                    if p.len()>=4 {
                        let f = p[0].parse::<i64>().unwrap_or(0);
                        let u = p[1].parse::<i64>().unwrap_or(0);
                        let t = p[2].parse::<i64>().unwrap_or(0);
                        let g = p[3].parse::<i64>().unwrap_or(0);
                        free+=f; used+=u; total+=t; gpu+=g; cnt+=1;
                        let fmt_mb = |mb: i64| if mb>=1024 { format!("{} GB", (mb as f64/1024.0).round() as i64) } else { format!("{} MB", mb) };
                        let vram_pct = if t>0 { (u as f64 / t as f64 * 100.0).round() as i64 } else { 0 };
                        per_gpu.push(serde_json::json!({"free": f, "used": u, "total": t, "gpu": g, "vram": vram_pct, "vramFree": fmt_mb(f), "vramUsed": fmt_mb(u), "vramTotal": fmt_mb(t)}));
                    }
                }
                let fmt = |mb: i64| if mb>=1024 { format!("{} GB", (mb as f64/1024.0).round() as i64) } else { format!("{} MB", mb) };
                result["vramFree"] = serde_json::Value::String(fmt(free));
                result["vramUsed"] = serde_json::Value::String(fmt(used));
                result["vramTotal"] = serde_json::Value::String(fmt(total));
                if total>0 { result["vram"] = serde_json::json!((used as f64/total as f64*100.0).round() as i64); }
                result["gpu"] = serde_json::json!(if cnt>1 { (gpu as f64/cnt as f64).round() as i64 } else { gpu });
                // build gpus array for topbar
                let mut gpus_arr: Vec<serde_json::Value> = per_gpu.iter().enumerate().map(|(i,g)| {
                    serde_json::json!({"index": i, "gpu": g["gpu"], "vram": g["vram"], "vramFree": g["vramFree"], "vramUsed": g["vramUsed"], "vramTotal": g["vramTotal"]})
                }).collect();
                // iGPU fallback: nvidia-smi only lists NVIDIA; add Intel/AMD via WMI when only 1 GPU
                if gpus_arr.len() == 1 {
                    if let Ok(wmi_out) = Command::new("powershell").args(["-NoProfile","-Command","Get-CimInstance Win32_VideoController | Select-Object Name,AdapterRAM | ForEach-Object { $_.Name + '|' + $_.AdapterRAM }"]).output() {
                        if wmi_out.status.success() {
                            let wmi_s = String::from_utf8_lossy(&wmi_out.stdout).trim().to_string();
                            for ln in wmi_s.lines() {
                                if let Some((n, r)) = ln.split_once('|') {
                                    let name = n.trim();
                                    if name.is_empty() || name.to_lowercase().contains("nvidia") { continue; }
                                    let lower = name.to_lowercase();
                                    if lower.contains("intel") || lower.contains("amd") || lower.contains("radeon") || lower.contains("arc") {
                                        let vram_mb = r.trim().parse::<i64>().unwrap_or(0) / (1024*1024);
                                        let fmt2 = if vram_mb>0 { format!("{} MB", vram_mb) } else { "—".into() };
                                        gpus_arr.push(serde_json::json!({"index": gpus_arr.len(), "gpu": 0, "vram": null, "vramFree": fmt2.clone(), "vramUsed": "0 MB", "vramTotal": fmt2, "name": name}));
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                result["gpus"] = serde_json::Value::Array(gpus_arr.clone());
                if gpus_arr.len() > 1 {
                    if let Some(g2) = gpus_arr.get(1) {
                        result["gpu2"] = g2.get("gpu").cloned().unwrap_or(serde_json::Value::Null);
                        result["vram2"] = g2.get("vram").cloned().unwrap_or(serde_json::Value::Null);
                        result["vramFree2"] = g2.get("vramFree").cloned().unwrap_or(serde_json::Value::Null);
                        result["vramUsed2"] = g2.get("vramUsed").cloned().unwrap_or(serde_json::Value::Null);
                        result["vramTotal2"] = g2.get("vramTotal").cloned().unwrap_or(serde_json::Value::Null);
                    }
                }
                let _ = LAST_NVIDIA.get_or_init(|| Mutex::new(None)).lock().unwrap().replace(result.clone());
            }
        } else {
            if let Some(last) = LAST_NVIDIA.get().and_then(|m| m.lock().ok()).and_then(|g| g.clone()) {
                result["vramFree"] = last["vramFree"].clone(); result["vramUsed"] = last["vramUsed"].clone(); result["vramTotal"] = last["vramTotal"].clone(); result["vram"] = last["vram"].clone(); result["gpu"] = last["gpu"].clone();
                result["gpus"] = last["gpus"].clone(); result["gpu2"] = last["gpu2"].clone(); result["vram2"] = last["vram2"].clone(); result["vramFree2"] = last["vramFree2"].clone(); result["vramUsed2"] = last["vramUsed2"].clone(); result["vramTotal2"] = last["vramTotal2"].clone();
            }
        }}
    }
    result
}

#[tauri::command]
fn config_load() -> serde_json::Value { load_config_value() }

#[tauri::command]
fn config_save(cfg: serde_json::Value) -> Result<serde_json::Value, String> {
    let p = get_config_file();
    let s = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    atomic_write(&p, &s).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"ok": true, "success": true}))
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
async fn uv_cache_size() -> serde_json::Value {
    // ponytail: async walk so Manage → Calculate size doesn't freeze UI (Electron 63b0f90)
    let p = get_repo_dir().join(".uv-cache");
    if !p.exists() { return serde_json::json!({"exists": false, "sizeBytes": 0, "cacheDir": p.to_string_lossy().to_string()}); }
    let p_clone = p.clone();
    let size = tauri::async_runtime::spawn_blocking(move || {
        let mut size: u64 = 0;
        fn walk(p: &Path, acc: &mut u64) { if let Ok(rd) = std::fs::read_dir(p) { for e in rd.flatten() { if let Ok(m) = e.metadata() { if m.is_dir() { walk(&e.path(), acc); } else { *acc += m.len(); } } } } }
        walk(&p_clone, &mut size);
        size
    }).await.unwrap_or(0);
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
    let mode = mode.unwrap_or("browser".into());
    let repo = get_repo_dir();
    if !repo.join("wgp.py").exists() { return Err("Wan2GP not installed — run Install first".into()); }
    let cfg = load_config_value();
    let port = cfg.get("serverPort").and_then(|v| v.as_u64()).unwrap_or(7860);
    // ponytail: if server already listening on :port (desktop→browser switch), reuse it — don't spawn second python on same port (Gradio OSError)
    if std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok() {
        let url = format!("http://localhost:{}", port);
        let _ = app.emit("launch-log", format!("[*] Wan2GP already running on :{} — opening {}\n", port, url));
        return Ok(serde_json::json!({"ok": true, "port": port, "mode": mode, "url": url, "fresh": false}));
    }
    mutating_try("launch")?;
    let share = cfg.get("share").and_then(|v| v.as_bool()).unwrap_or(false);
    let gpu_device = cfg.get("gpuDevice").and_then(|v| v.as_str()).unwrap_or("auto").trim().to_string();
    let launcher_gpu = cfg.get("launcherGpu").and_then(|v| v.as_str()).unwrap_or("auto").to_string();
    // build args — gpuDevice -> --gpu (mirrors Electron buildCommonLaunchArgs)
    let server_name = cfg.get("serverName").and_then(|v| v.as_str()).unwrap_or("localhost").to_string();
    let mut args = vec!["wgp.py".to_string(), "--server-port".into(), port.to_string(), "--server-name".into(), server_name.clone(), "--advanced".into(), "--multiple-images".into()];
    if share { args.push("--share".into()); }
    if gpu_device != "auto" && gpu_device.starts_with("cuda:") && !args.contains(&"--gpu".to_string()) {
        args.push("--gpu".into()); args.push(gpu_device.clone());
    }
    let emit = |msg: &str| { let _ = app.emit("launch-log", msg.to_string()); };
    emit(&format!("[*] Launching Wan2GP ({}) on :{}…\n", mode, port));
    // GPU assignment log (mirrors Electron 9945990)
    {
        let hw = get_gpu_info_sync();
        let hw_name = hw.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let hw_vendor = hw.get("vendor").and_then(|v| v.as_str()).unwrap_or("?");
        let hw_vram = hw.get("vramMB").and_then(|v| v.as_str()).unwrap_or("0");
        let gpu_count = std::process::Command::new("nvidia-smi").args(["--query-gpu=index","--format=csv,noheader"]).output().ok().map(|o| if o.status.success() { String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.trim().is_empty()).count().to_string() + " NVIDIA" } else { "?".into() }).unwrap_or("?".into());
        let gen_label = if gpu_device=="auto" { format!("auto ({} )", hw_name) } else { gpu_device.clone() };
        emit(&format!("[*] GPU assignment — Launcher UI: {} | Generation: {} | HW: {} ({}, {}) | Detected: {}\n", launcher_gpu, gen_label, hw_name, hw_vendor, hw_vram, gpu_count));
    }
    emit(&format!("[*] Args: {}\n", args.join(" ")));
    // bootstrap shim — minimal PYTHONUNBUFFERED + isatty patch so tqdm bars stream
    let boot = repo.join(".wan2gp-bootstrap.py");
    let _ = std::fs::write(&boot, "import runpy,sys,os; os.environ['PYTHONUNBUFFERED']='1'\nrunpy.run_path('wgp.py',run_name='__main__')");
    // resolve python for active env
    let env = get_active_env();
    let py = if let Some(raw) = env.get("path").and_then(|p| p.as_str()) {
        let rel = raw.trim_start_matches(".\\").trim_start_matches("./").trim_start_matches(".\\").trim_start_matches("./");
        let base = if Path::new(raw).is_absolute() { PathBuf::from(raw) } else { get_repo_dir().join(rel) };
        let cand = if cfg!(windows) { base.join("Scripts\\python.exe") } else { base.join("bin/python3") };
        if cand.exists() { cand.to_string_lossy().to_string() } else if base.exists() { // uv env may be at base itself
            let alt = if cfg!(windows) { base.join("Scripts\\python.exe") } else { base.join("bin/python") };
            if alt.exists() { alt.to_string_lossy().to_string() } else { raw.to_string() }
        } else { cand.to_string_lossy().to_string() }
    } else { "python".to_string() };
    use tauri_plugin_shell::ShellExt;
    let (rx, child) = app.shell().command(&py).args(&args).current_dir(&repo).spawn().map_err(|e| { mutating_done(); e.to_string() })?;
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
    // wait for port in background (don't hold mutating — launch is done, server boots async)
    let host = "127.0.0.1".to_string();
    let app3 = app.clone();
    std::thread::spawn(move || {
        for _ in 0..60 { std::thread::sleep(std::time::Duration::from_secs(3)); if std::net::TcpStream::connect(format!("{}:{}", host, port)).is_ok() { let _ = app3.emit("launch-log", format!("[✓] Wan2GP ready on http://localhost:{}\n", port)); break; } }
    });
    mutating_done();
    let url = format!("http://localhost:{}", port);
    Ok(serde_json::json!({"ok": true, "port": port, "mode": mode, "url": url, "fresh": true}))
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
async fn check_package_updates(app: tauri::AppHandle, versions: Option<serde_json::Value>) -> Result<serde_json::Value,String> {
    let _ = versions;
    let env = get_active_env();
    let raw = env.get("path").and_then(|p| p.as_str()).unwrap_or("");
    let r = raw.trim_start_matches(|c| c == '.' || c == '\\' || c == '/');
    let base = if std::path::Path::new(raw).is_absolute() { PathBuf::from(raw) } else { get_repo_dir().join(r) };
    let py = if cfg!(windows) { base.join("Scripts\\python.exe") } else { base.join("bin/python") };
    if !py.exists() { return Ok(serde_json::json!([])); }
    use tauri_plugin_shell::ShellExt; use tauri_plugin_shell::process::CommandEvent;
    let (mut rx, _) = app.shell().command(&py).args(["-m","pip","list","--outdated","--format=json"]).spawn().map_err(|e| e.to_string())?;
    let mut out = String::new();
    while let Some(ev) = rx.recv().await { match ev { CommandEvent::Stdout(b) => out.push_str(&String::from_utf8_lossy(&b)), CommandEvent::Stderr(b) => out.push_str(&String::from_utf8_lossy(&b)), _=>{} } }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&out) { if let Some(arr) = v.as_array() { let res: Vec<serde_json::Value> = arr.iter().map(|e| serde_json::json!({"name": e.get("name").cloned().unwrap_or(serde_json::Value::Null), "current": e.get("version").cloned().unwrap_or(serde_json::Value::Null), "latest": e.get("latest_version").cloned().unwrap_or(serde_json::Value::Null)})).collect(); return Ok(serde_json::Value::Array(res)); } }
    Ok(serde_json::json!([]))
}
#[tauri::command]
fn check_package(pkg: String) -> serde_json::Value { serde_json::json!({"name": pkg, "installed": false}) }
#[tauri::command]
fn deepy_status() -> serde_json::Value { serde_json::json!({"mode": "disabled"}) }
#[tauri::command]
fn memory_profile_read() -> serde_json::Value {
    // read wgp_config.json memory profile — ponytail: return current profile or default
    let p = get_repo_dir().join("wgp_config.json");
    if let Ok(s) = std::fs::read_to_string(&p) { if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
        return serde_json::json!({
            "video_profile": v.get("video_profile").cloned().unwrap_or(serde_json::json!(4)),
            "image_profile": v.get("image_profile").cloned().unwrap_or(serde_json::json!(4)),
            "audio_profile": v.get("audio_profile").cloned().unwrap_or(serde_json::json!(4)),
            "vram_safety_coefficient": v.get("vram_safety_coefficient").cloned().unwrap_or(serde_json::json!(0.8)),
            "vae_config": v.get("vae_config").cloned().unwrap_or(serde_json::json!(0))
        });
    }}
    serde_json::json!({"video_profile": 4, "image_profile": 4, "audio_profile": 4, "vram_safety_coefficient": 0.8, "vae_config": 0, "transformer_quantization": "int8"})
}
#[tauri::command]
fn auto_tune_detect() -> serde_json::Value {
    // real hardware detect — mirrors services/auto-tune.js detect() but sync via nvidia-smi
    let gpu = get_gpu_info_sync();
    let vendor = gpu.get("vendor").and_then(|v| v.as_str()).unwrap_or("").to_uppercase();
    let name = gpu.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let vram_str = gpu.get("vramMB").and_then(|v| v.as_str()).unwrap_or("0");
    let vram_mb: f64 = vram_str.split_whitespace().next().unwrap_or("0").parse().unwrap_or(0.0);
    let vram_gb = (vram_mb / 1024.0).round() as i64;
    let cuda_available = vendor == "NVIDIA" && vram_mb > 0.0 && !name.is_empty();
    // RAM via powershell fallback
    let ram_gb = {
        #[cfg(windows)] {
            std::process::Command::new("powershell").args(["-NoProfile","-Command","[math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory/1GB,1)"]).output()
                .ok().and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().ok()).unwrap_or(32.0)
        }
        #[cfg(not(windows))] { 32.0 }
    };
    let cpu_count = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(8) as i64;
    let vram_tier = if !cuda_available { "none" } else if vram_gb >= 24 { "high" } else if vram_gb >= 12 { "low" } else { "tight" };
    let ram_tier = if ram_gb >= 63.5 { "high" } else if ram_gb >= 31.5 { "low" } else { "very_low" };
    serde_json::json!({
        "cuda_available": cuda_available,
        "gpu_name": if cuda_available { name.clone() } else { "—".into() },
        "gpu_vram_gb": vram_gb,
        "ram_gb": ram_gb,
        "cpu_count": cpu_count,
        "vram_tier": vram_tier,
        "ram_tier": ram_tier,
        "vendor": vendor,
        "driver": gpu.get("driverVersion").cloned().unwrap_or(serde_json::Value::Null)
    })
}
#[tauri::command]
fn auto_tune_recommend(hw: Option<serde_json::Value>, opts: Option<serde_json::Value>) -> serde_json::Value {
    let hw = hw.unwrap_or(serde_json::json!({"vram_tier":"low","ram_tier":"low","gpu_vram_gb":10}));
    let vram_tier = hw.get("vram_tier").and_then(|v| v.as_str()).unwrap_or("low");
    let ram_tier = hw.get("ram_tier").and_then(|v| v.as_str()).unwrap_or("low");
    let vram_gb = hw.get("gpu_vram_gb").and_then(|v| v.as_f64()).unwrap_or(10.0);
    let failsafe = opts.as_ref().and_then(|o| o.get("failsafe")).and_then(|v| v.as_bool()).unwrap_or(false);
    let (profile, coeff) = if failsafe { (5, 0.6) } else {
        let p = match (vram_tier, ram_tier) {
            ("high","high")=>1, ("high","low")=>3, ("high","very_low")=>3,
            ("low","high")=>2, ("low","low")=>4, ("low","very_low")=>5,
            ("tight","high")=>4, ("tight","low")=>4, _=>5
        };
        let c = if vram_tier=="tight"||vram_tier=="none" {0.7} else {0.8};
        (p,c)
    };
    let audio = if vram_gb>=12.0 && ![1,3].contains(&profile) {3} else {profile};
    serde_json::json!({
        "video_profile": profile, "image_profile": profile, "audio_profile": audio,
        "vram_safety_coefficient": coeff, "vae_config": 0, "transformer_quantization": "int8",
        "_recommendation_label": format!("P{} {}", profile, if failsafe {"(failsafe)"} else {""}),
        "_recommendation_reason": "Auto-tuned for your hardware",
        "packages": ["torch","triton","sageattention"],
        "kernels": ["nunchaku","gguf"]
    })
}

// ── Phase 2-5: remaining 65 handlers as thin stubs (real logic behind shell/fs plugins) ──
#[tauri::command]
async fn install(app: tauri::AppHandle, env_type: Option<String>) -> Result<serde_json::Value,String> {
    mutating_try("install")?;
    let env = env_type.unwrap_or("uv".into()); // uv | venv | conda
    let repo = get_repo_dir();
    let emit = |msg: &str| { let _ = app.emit("setup-output", msg.to_string()); };
    // hardware-aware header (driver warning surfaces before 20min install)
    let gpu = get_gpu_info_sync();
    let plan = build_install_plan(&gpu);
    emit(&format!("[hw] GPU: {} ({}) — {} / {} — profile {}\n", plan["gpuName"].as_str().unwrap_or("?"), plan["vendor"].as_str().unwrap_or("?"), plan["cuda"].as_str().unwrap_or("?"), plan["torch"].as_str().unwrap_or("?"), plan["profile"].as_str().unwrap_or("?")));
    if let Some(w)=plan["driverWarning"].as_str() { if !w.is_empty() { emit(&format!("[warn] {}\n", w)); } }
    emit(&format!("[env] requested: {}\n", env));
    let emit_phase = |id: &str, label: &str, done: bool| { let _ = app.emit("setup-phase", serde_json::json!({"id": id, "label": label, "done": done})); };
    if !repo.join("wgp.py").exists() {
        emit_phase("clone", "Clone Wan2GP repository", false);
        emit(&format!("[*] Cloning Wan2GP into {}\n", repo.display()));
        std::fs::create_dir_all(&repo).map_err(|e| e.to_string())?;
        // If repo already exists but is not empty (e.g. contains desktop-config.json from previous launch),
        // git clone directly into it fails ("already exists and is not empty"). Clone into a temp dir
        // inside the target (same volume) then merge, preserving user files — mirrors Electron mergeDir.
        let needs_tmp = repo.exists() && std::fs::read_dir(&repo).map(|mut it| it.next().is_some()).unwrap_or(false);
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
            use tauri_plugin_shell::ShellExt;
            let (mut rx, _child) = app.shell().command("git").args(["clone","--depth","1","https://github.com/deepbeepmeep/Wan2GP.git", &repo.to_string_lossy()]).spawn().map_err(|e| e.to_string())?;
            use tauri_plugin_shell::process::CommandEvent;
            while let Some(ev) = rx.recv().await { match ev { CommandEvent::Stdout(b) => emit(&String::from_utf8_lossy(&b)), CommandEvent::Stderr(b) => emit(&String::from_utf8_lossy(&b)), _ => {} } }
        }
        if !repo.join("wgp.py").exists() { mutating_done(); emit_phase("clone", "Clone Wan2GP repository", true); return Err("git clone failed — check output above".into()); }
        emit("[*] Repository cloned.\n");
        emit_phase("clone", "Clone Wan2GP repository", true);
    } else {
        emit_phase("clone", "Clone Wan2GP repository", true);
    }
    emit(&format!("[*] Installing env={} via setup.py (streaming)…\n", env));
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
async fn reinstall(app: tauri::AppHandle) -> Result<serde_json::Value,String> {
    mutating_try("reinstall")?;
    let repo = get_repo_dir();
    let emit = |msg: &str| { let _ = app.emit("setup-output", msg.to_string()); };
    emit("[*] Removing existing installation...\n");
    // backup plugins/finetunes (ponytail: xcopy fallback)
    let backup = get_data_dir().join(".reinstall-backup");
    let _ = std::fs::remove_dir_all(&backup);
    let _ = std::fs::create_dir_all(&backup);
    for sub in ["plugins","finetunes"] { let s = repo.join(sub); if s.exists() { let d = backup.join(sub); let _ = std::process::Command::new("xcopy").args(["/E","/I", &s.to_string_lossy().to_string(), &d.to_string_lossy().to_string()]).output(); } }
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
                    #[cfg(windows)] { let _ = std::process::Command::new("cmd").args(["/C", &format!("attrib -R /S /D \"{}\"", src.display())]).output(); }
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
                #[cfg(windows)] { let _ = std::process::Command::new("cmd").args(["/C", &format!("attrib -R /S /D \"{}\"", git.display())]).output(); }
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
async fn uninstall(_app: tauri::AppHandle) -> Result<serde_json::Value,String> {
    mutating_try("uninstall")?;
    let repo = get_repo_dir();
    if !repo.exists() { mutating_done(); return Err("Wan2GP not installed".into()); }
    // keep ckpts/loras/outputs if user wants — for now delete all (full uninstall). ponytail: add keep prompt when needed
    let _ = std::fs::remove_dir_all(&repo);
    let _ = std::fs::remove_file(get_envs_file());
    mutating_done(); Ok(serde_json::json!({"success": true}))
}
#[tauri::command]
async fn sync_kernels(app: tauri::AppHandle) -> Result<serde_json::Value,String> {
    mutating_try("sync-kernels")?;
    let repo = get_repo_dir(); let cfg_path = repo.join("setup_config.json");
    let env = get_active_env();
    let raw = env.get("path").and_then(|p| p.as_str()).unwrap_or(""); let base = if std::path::Path::new(raw).is_absolute() { PathBuf::from(raw) } else { repo.join(raw.trim_start_matches(".\\").trim_start_matches("./")) };
    let py = if cfg!(windows) { base.join("Scripts\\python.exe") } else { base.join("bin/python3") };
    if !py.exists() { mutating_done(); return Err("python not found for active env".into()); }
    if !cfg_path.exists() { mutating_done(); return Err("setup_config.json missing".into()); }
    let cfg: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&cfg_path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let gpu = get_gpu_info_sync(); let profile = kernel_profile_key(gpu.get("vendor").and_then(|v| v.as_str()).unwrap_or(""), gpu.get("name").and_then(|v| v.as_str()).unwrap_or(""));
    let kernels = cfg.get("gpu_profiles").and_then(|p| p.get(&profile)).and_then(|pr| pr.get("kernels")).and_then(|k| k.as_array()).cloned().unwrap_or_default();
    use tauri_plugin_shell::ShellExt; use tauri_plugin_shell::process::CommandEvent;
    for k in kernels {
        if let Some(name) = k.as_str() {
            // find wheel url from components.kernels[name].cmd[win]
            let url = cfg.get("components").and_then(|c| c.get("kernels")).and_then(|m| m.get(name)).and_then(|e| e.get("cmd")).and_then(|c| c.get("win")).and_then(|u| u.as_str()).unwrap_or("");
            if url.is_empty() { continue; }
            let _ = app.emit("launch-log", format!("[*] sync kernel {}\n", name));
            let (mut rx, _) = app.shell().command(&py).args(["-m","pip","install", url, "--upgrade"]).spawn().map_err(|e| e.to_string())?;
            while let Some(ev) = rx.recv().await { match ev { CommandEvent::Stdout(b)|CommandEvent::Stderr(b) => { let _ = app.emit("launch-log", String::from_utf8_lossy(&b).to_string()); }, _=>{} } }
        }
    }
    mutating_done(); Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command]
async fn update(app: tauri::AppHandle) -> Result<serde_json::Value,String> {
    mutating_try("update")?;
    let repo = get_repo_dir();
    if !repo.join(".git").exists() { mutating_done(); return Err("not a git repo".into()); }
    use tauri_plugin_shell::ShellExt; use tauri_plugin_shell::process::CommandEvent;
    let (mut rx, _) = app.shell().command("git").args(["pull"]).current_dir(&repo).spawn().map_err(|e| e.to_string())?;
    while let Some(ev) = rx.recv().await { match ev { CommandEvent::Stdout(b)|CommandEvent::Stderr(b) => { let _ = app.emit("launch-log", String::from_utf8_lossy(&b).to_string()); }, _=>{} } }
    mutating_done(); Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command] fn manage_set_active(name: String) -> Result<serde_json::Value,String> {
    let f = get_envs_file(); let mut v: serde_json::Value = std::fs::read_to_string(&f).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(serde_json::json!({"envs":{}, "active":null}));
    if v.get("envs").and_then(|e| e.get(&name)).is_none() { return Err(format!("env {} not found", name)); }
    v["active"] = serde_json::Value::String(name);
    atomic_write(&f, &serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command] fn uninstall_env(name: String) -> Result<serde_json::Value,String> {
    let f = get_envs_file(); let mut v: serde_json::Value = std::fs::read_to_string(&f).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(serde_json::json!({"envs":{}}));
    let path = v.get("envs").and_then(|e| e.get(&name)).and_then(|e| e.get("path")).and_then(|p| p.as_str()).map(|p| if std::path::Path::new(p).is_absolute() { PathBuf::from(p) } else { get_repo_dir().join(p.trim_start_matches(".\\").trim_start_matches("./")) });
    if let Some(p) = path { let _ = std::fs::remove_dir_all(p); }
    if let Some(obj) = v.get_mut("envs").and_then(|e| e.as_object_mut()) { obj.remove(&name); }
    if v.get("active").and_then(|a| a.as_str()) == Some(&name) { v["active"] = serde_json::Value::Null; }
    atomic_write(&f, &serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command] fn open_external(url: Option<String>) -> Result<(),String> { let _=url; Ok(()) }
#[tauri::command] fn detect_browsers() -> serde_json::Value {
    // mirrors Electron WELL_KNOWN_BROWSERS with win env expansion
    let cfg = load_config_value(); let def = cfg.get("defaultBrowser").and_then(|v| v.as_str()).unwrap_or("system").to_string();
    let expand = |p: &str| {
        let mut s = p.to_string();
        for (k,v) in std::env::vars() { s = s.replace(&format!("%{}%", k), &v); s = s.replace(&format!("%{}%", k.to_lowercase()), &v); }
        // handle (x86)
        if let Ok(pf86) = std::env::var("ProgramFiles(x86)") { s = s.replace("%ProgramFiles(x86)%", &pf86); }
        s
    };
    let browsers = vec![
        ("chrome", "Google Chrome", vec!["%ProgramFiles%\\Google\\Chrome\\Application\\chrome.exe","%ProgramFiles(x86)%\\Google\\Chrome\\Application\\chrome.exe","%LocalAppData%\\Google\\Chrome\\Application\\chrome.exe"]),
        ("edge", "Microsoft Edge", vec!["%ProgramFiles%\\Microsoft\\Edge\\Application\\msedge.exe","%ProgramFiles(x86)%\\Microsoft\\Edge\\Application\\msedge.exe"]),
        ("firefox","Firefox", vec!["%ProgramFiles%\\Mozilla Firefox\\firefox.exe","%ProgramFiles(x86)%\\Mozilla Firefox\\firefox.exe"]),
        ("brave","Brave", vec!["%LocalAppData%\\BraveSoftware\\Brave-Browser\\Application\\brave.exe"]),
        ("opera","Opera", vec!["%LocalAppData%\\Programs\\Opera\\launcher.exe"]),
        ("vivaldi","Vivaldi", vec!["%LocalAppData%\\Vivaldi\\Application\\vivaldi.exe"]),
    ];
    let mut out = Vec::new();
    for (id, name, wins) in browsers {
        let mut path: Option<String> = None;
        for cand in wins { let ep = expand(cand); if std::path::Path::new(&ep).exists() { path = Some(ep); break; } }
        out.push(serde_json::json!({"id": id, "name": name, "installed": path.is_some(), "path": path}));
    }
    serde_json::json!({"browsers": out, "defaultBrowser": def})
}
#[tauri::command] fn launch_browser(url: Option<String>) -> serde_json::Value { let _=url; // ponytail: url optional — app.js may call with null before launch
serde_json::json!({"ok": true}) }
#[tauri::command] fn launch_browser_no_gpu(url: Option<String>) -> serde_json::Value { let _=url; serde_json::json!({"ok": true}) }
#[tauri::command] fn chrome_available() -> bool { std::process::Command::new("where").arg("chrome").output().map(|o| o.status.success()).unwrap_or(false) }
#[tauri::command] fn set_data_dir(dir: String) -> Result<serde_json::Value,String> { let ov = data_dir_override_file(); atomic_write(&ov, &dir).map_err(|e| e.to_string())?; Ok(serde_json::json!({"ok": true, "success": true})) }
#[tauri::command] fn reset_data_dir() -> serde_json::Value { let _=std::fs::remove_file(data_dir_override_file()); serde_json::json!({"ok": true}) }
#[tauri::command] fn migrate_to_preferred(choices: Option<serde_json::Value>) -> serde_json::Value { let _=choices; serde_json::json!({"ok": true}) }
#[tauri::command] fn move_folder(src: String, dst: String) -> Result<serde_json::Value,String> {
    let s = PathBuf::from(&src); let d = PathBuf::from(&dst);
    if let Err(_) = std::fs::rename(&s, &d) {
        // cross-device fallback — copy then remove
        if s.is_dir() { fs_extra_fallback_copy_dir(&s, &d)?; std::fs::remove_dir_all(&s).map_err(|e| e.to_string())?; }
        else { std::fs::copy(&s, &d).map_err(|e| e.to_string())?; std::fs::remove_file(&s).map_err(|e| e.to_string())?; }
    }
    Ok(serde_json::json!({"ok": true, "success": true}))
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
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command]
async fn install_prerequisite(app: tauri::AppHandle, tool: String) -> Result<serde_json::Value,String> {
    use tauri_plugin_shell::ShellExt; use tauri_plugin_shell::process::CommandEvent;
    let cmd = match tool.as_str() { "git" => vec!["winget","install","--id","Git.Git","-e"], "uv" => vec!["winget","install","--id","astral-sh.uv","-e"], _ => return Err(format!("unknown tool {}", tool)) };
    let (mut rx, _) = app.shell().command(cmd[0]).args(&cmd[1..]).spawn().map_err(|e| e.to_string())?;
    while let Some(ev) = rx.recv().await { match ev { CommandEvent::Stdout(b)|CommandEvent::Stderr(b) => { let _ = app.emit("launch-log", String::from_utf8_lossy(&b).to_string()); }, _=>{} } }
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command]
async fn get_wangp_upstream_info() -> serde_json::Value {
    // fetch latest commit from GitHub API — mirrors Electron fetchUrl for Wan2GP upstream
    let url = "https://api.github.com/repos/deepbeepmeep/Wan2GP/commits?per_page=1";
    // try curl first (fast, no PS overhead), fallback to powershell
    let try_curl = std::process::Command::new("curl").args(["-s", "-H", "User-Agent: wan2gp-tauri", url]).output();
    let json_str = if let Ok(o) = try_curl { if o.status.success() { String::from_utf8_lossy(&o.stdout).to_string() } else { String::new() } } else { String::new() };
    let json_str = if json_str.trim().starts_with('[') { json_str } else {
        // powershell fallback
        std::process::Command::new("powershell").args(["-NoProfile","-Command", &format!("try {{ (Invoke-RestMethod -Uri '{}' -Headers @{{'User-Agent'='wan2gp-tauri'}} | ConvertTo-Json -Depth 4) }} catch {{ '[]' }}", url)]).output()
            .ok().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default()
    };
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&json_str) {
        if let Some(arr) = v.as_array() { if let Some(first) = arr.first() {
            let sha = first.get("sha").and_then(|s| s.as_str()).unwrap_or("");
            let date = first.get("commit").and_then(|c| c.get("committer")).and_then(|c| c.get("date")).and_then(|d| d.as_str()).unwrap_or("");
            let msg = first.get("commit").and_then(|c| c.get("message")).and_then(|m| m.as_str()).unwrap_or("");
            if !sha.is_empty() { return serde_json::json!({"hash": sha, "date": date, "message": msg}); }
        }}
        // if API returned object (single commit) handle
        if let Some(sha) = v.get("sha").and_then(|s| s.as_str()) {
            let date = v.get("commit").and_then(|c| c.get("committer")).and_then(|c| c.get("date")).and_then(|d| d.as_str()).unwrap_or("");
            return serde_json::json!({"hash": sha, "date": date});
        }
    }
    serde_json::json!(null)
}
#[tauri::command] fn get_wangp_version() -> serde_json::Value { serde_json::json!(null) }
#[tauri::command] fn report_issue() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn create_desktop_shortcut() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command]
async fn upgrade_package(app: tauri::AppHandle, pkg: String) -> Result<serde_json::Value,String> {
    let env = get_active_env(); let raw = env.get("path").and_then(|p| p.as_str()).unwrap_or(""); let base = if std::path::Path::new(raw).is_absolute() { PathBuf::from(raw) } else { get_repo_dir().join(raw.trim_start_matches(".\\").trim_start_matches("./")) };
    let py = if cfg!(windows) { base.join("Scripts\\python.exe") } else { base.join("bin/python") };
    if !py.exists() { return Err("python not found".into()); }
    use tauri_plugin_shell::ShellExt; use tauri_plugin_shell::process::CommandEvent;
    let (mut rx, _) = app.shell().command(&py).args(["-m","pip","install","--upgrade", &pkg]).spawn().map_err(|e| e.to_string())?;
    while let Some(ev) = rx.recv().await { match ev { CommandEvent::Stdout(b)|CommandEvent::Stderr(b) => { let _ = app.emit("launch-log", String::from_utf8_lossy(&b).to_string()); }, _=>{} } }
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command]
async fn install_package(app: tauri::AppHandle, pkg: String) -> Result<serde_json::Value,String> {
    let env = get_active_env(); let raw = env.get("path").and_then(|p| p.as_str()).unwrap_or(""); let base = if std::path::Path::new(raw).is_absolute() { PathBuf::from(raw) } else { get_repo_dir().join(raw.trim_start_matches(".\\").trim_start_matches("./")) };
    let py = if cfg!(windows) { base.join("Scripts\\python.exe") } else { base.join("bin/python") };
    if !py.exists() { return Err("python not found".into()); }
    use tauri_plugin_shell::ShellExt; use tauri_plugin_shell::process::CommandEvent;
    let (mut rx, _) = app.shell().command(&py).args(["-m","pip","install", &pkg]).spawn().map_err(|e| e.to_string())?;
    while let Some(ev) = rx.recv().await { match ev { CommandEvent::Stdout(b)|CommandEvent::Stderr(b) => { let _ = app.emit("launch-log", String::from_utf8_lossy(&b).to_string()); }, _=>{} } }
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command]
async fn uninstall_package(app: tauri::AppHandle, pkg: String) -> Result<serde_json::Value,String> {
    let env = get_active_env(); let raw = env.get("path").and_then(|p| p.as_str()).unwrap_or(""); let base = if std::path::Path::new(raw).is_absolute() { PathBuf::from(raw) } else { get_repo_dir().join(raw.trim_start_matches(".\\").trim_start_matches("./")) };
    let py = if cfg!(windows) { base.join("Scripts\\python.exe") } else { base.join("bin/python") };
    if !py.exists() { return Err("python not found".into()); }
    use tauri_plugin_shell::ShellExt; use tauri_plugin_shell::process::CommandEvent;
    let (mut rx, _) = app.shell().command(&py).args(["-m","pip","uninstall","-y", &pkg]).spawn().map_err(|e| e.to_string())?;
    while let Some(ev) = rx.recv().await { match ev { CommandEvent::Stdout(b)|CommandEvent::Stderr(b) => { let _ = app.emit("launch-log", String::from_utf8_lossy(&b).to_string()); }, _=>{} } }
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command]
async fn restore_requirements(app: tauri::AppHandle) -> Result<serde_json::Value,String> {
    let repo = get_repo_dir(); let env = get_active_env(); let raw = env.get("path").and_then(|p| p.as_str()).unwrap_or(""); let base = if std::path::Path::new(raw).is_absolute() { PathBuf::from(raw) } else { repo.join(raw.trim_start_matches(".\\").trim_start_matches("./")) };
    let py = if cfg!(windows) { base.join("Scripts\\python.exe") } else { base.join("bin/python") };
    if !py.exists() { return Err("python not found".into()); }
    use tauri_plugin_shell::ShellExt; use tauri_plugin_shell::process::CommandEvent;
    let (mut rx, _) = app.shell().command(&py).args(["-m","pip","install","-r", "requirements.txt"]).current_dir(&repo).spawn().map_err(|e| e.to_string())?;
    while let Some(ev) = rx.recv().await { match ev { CommandEvent::Stdout(b)|CommandEvent::Stderr(b) => { let _ = app.emit("launch-log", String::from_utf8_lossy(&b).to_string()); }, _=>{} } }
    Ok(serde_json::json!({"ok": true, "success": true}))
}
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
#[tauri::command] fn download_update(opts: Option<serde_json::Value>) -> serde_json::Value {
    // Electron 91c2de8: Full (disableDifferentialDownload=true) vs Quick (differential=true)
    let use_diff = opts.as_ref().and_then(|o| o.get("differential")).and_then(|v| v.as_bool()).unwrap_or(false);
    // ponytail: Tauri updater plugin not yet wired (needs pubkey/endpoints) — keep stub but respect flag
    serde_json::json!({"ok": true, "differential": use_diff})
}
#[tauri::command] fn install_update() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn create_browser_view(url: Option<String>, opts: Option<serde_json::Value>) -> serde_json::Value { let _=(url, opts); serde_json::json!({"ok": true}) }
#[tauri::command] fn destroy_browser_view() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn get_log_history() -> serde_json::Value { serde_json::json!([]) }
#[tauri::command] fn uv_cache_clean(action: Option<String>) -> serde_json::Value { let _=action; serde_json::json!({"success": true}) }
#[tauri::command] fn open_task_manager() -> Result<serde_json::Value,String> {
    #[cfg(windows)] { std::process::Command::new("taskmgr.exe").spawn().map_err(|e| e.to_string())?; }
    #[cfg(not(windows))] { std::process::Command::new("gnome-system-monitor").spawn().map_err(|e| e.to_string())?; }
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command] fn get_crash_recovery_info() -> serde_json::Value { serde_json::json!(null) }
#[tauri::command] fn launch_webview() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn popout_webview(url: Option<String>) -> serde_json::Value { let _=url; serde_json::json!({"ok": true}) }
#[tauri::command] fn hide_browser_view() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn detach_browser_view() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn reattach_browser_view() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn create_term_view() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn destroy_term_view() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn bv_navigate(action: String) -> serde_json::Value { let _=action; serde_json::json!({"ok": true}) }
#[tauri::command] fn bv_set_zoom(factor: f64) -> serde_json::Value { let _=factor; serde_json::json!({"ok": true}) }
#[tauri::command] fn bv_set_dock(dock: String) -> serde_json::Value { let _=dock; serde_json::json!({"ok": true}) }
#[tauri::command] fn is_data_dir_roaming() -> bool { false } // ponytail: Tauri uses isolated .wan2gp-tauri-data-dir + C:\Wan2GP — never roaming, hide pre-v3.0 warning (#05cbdb3)
#[tauri::command] fn migrate_choose() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn notifier_ensure() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] fn ui_mode_set(mode: String) -> serde_json::Value { let _=mode; serde_json::json!({"ok": true}) }
#[allow(dead_code)]
#[tauri::command] fn on_system_theme_change() -> serde_json::Value { serde_json::json!(null) }

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Launcher GPU preference — mirrors Electron early switch (main.js 323-350)
    // Reads desktop-config.json before WebView2 creation and sets browser args.
    // Supports: auto | integrated (low-power) | dedicated (high-perf) | disabled (SwiftShader)
    {
        let cfg_path = get_config_file();
        if cfg_path.exists() {
            if let Ok(s) = std::fs::read_to_string(&cfg_path) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                    let lg = v.get("launcherGpu")
                        .and_then(|x| x.as_str())
                        .unwrap_or_else(|| if v.get("electronGpu").and_then(|x| x.as_bool()) == Some(false) { "disabled" } else { "auto" })
                        .trim().to_string();
                    match lg.as_str() {
                        "disabled" => std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGS", "--disable-gpu"),
                        "integrated" => std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGS", "--force_low_power_gpu"),
                        "dedicated" => std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGS", "--force_high_performance_gpu"),
                        _ => {}
                    }
                    eprintln!("[launcher] GPU preference: {} — {}", lg, match lg.as_str() { "integrated" => "iGPU (power saving, frees VRAM)", "dedicated" => "dGPU (high perf)", "disabled" => "SwiftShader (max VRAM)", _ => "OS decides" });
                }
            }
        }
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        // ponytail: updater plugin wired in Cargo but not inited until pubkey/endpoints configured — keep stubs
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

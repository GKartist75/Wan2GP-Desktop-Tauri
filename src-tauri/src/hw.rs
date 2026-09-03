//! GPU / hardware probing and system metrics.
use std::sync::{Mutex, OnceLock};
use crate::base::*;

// ── GPU helpers ──
pub(crate) fn get_gpu_info_sync() -> serde_json::Value {
    
    if let Ok(out) = silent_command("nvidia-smi").args(["--query-gpu=name,memory.total,driver_version", "--format=csv,noheader"]).output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                let parts: Vec<&str> = s.split(", ").collect();
                return serde_json::json!({"vendor":"NVIDIA","name":parts.first().unwrap_or(&"").trim(),"vramMB":parts.get(1).unwrap_or(&"0 MiB").trim(),"driverVersion":parts.get(2).unwrap_or(&"").trim(),"raw":s});
            }
        }
    }
    serde_json::json!({"vendor":"unknown","name":"","vramMB":"0","driverVersion":"","raw":"nvidia-smi not found"})
}

// ── existing spike commands (kept) ──
#[tauri::command]
pub fn detect_gpu() -> serde_json::Value {
    get_gpu_info_sync()
}
pub(crate) fn kernel_profile_key(vendor: &str, name: &str) -> String {
    let v = vendor.to_uppercase(); let g = name.to_uppercase();
    if v == "APPLE" { return "MPS".into(); }
    if v == "NVIDIA" {
        if g.contains(" 10") || g.contains(" 16") || g.contains("GTX 10") || g.contains("GTX 16") { return "GTX_10".into(); }
        if g.contains("50") { return "RTX_50".into(); }
        if g.contains("40") { return "RTX_40".into(); }
        if g.contains("30") { return "RTX_30".into(); }
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
pub(crate) fn build_install_plan(hw: &serde_json::Value) -> serde_json::Value {
    let vendor = hw.get("vendor").and_then(|v| v.as_str()).unwrap_or("UNKNOWN").to_uppercase();
    let name = hw.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let vram = hw.get("vramMB").and_then(|v| v.as_str()).unwrap_or("0").split_whitespace().next().unwrap_or("0").parse::<f64>().unwrap_or(0.0);
    let driver = hw.get("driverVersion").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let vram_gb = vram / 1024.0_f64; let is_gtx = name.to_uppercase().contains("GTX 10") || name.to_uppercase().contains("GTX 16") || (name.contains("10") && vendor=="NVIDIA" && (name.contains("1050")||name.contains("1060")||name.contains("1650")));
    let (cuda, torch, warn) = if vendor=="NVIDIA" { if is_gtx { ("CUDA 12.8", "PyTorch 2.7.1", String::new()) } else { let mut w=String::new(); if let Ok(dv)=driver.parse::<f64>() { if dv < 580.0 { w=format!("NVIDIA driver {driver} < R580 — cu130 needs R580+"); }} ("CUDA 13 (cu130)", "PyTorch 2.10", w) } } else if vendor=="AMD" { ("ROCm (TheRock)", "PyTorch 2.7.0", String::new()) } else if vendor=="APPLE" { ("MPS (Metal)", "PyTorch (MPS)", String::new()) } else { ("CPU", "PyTorch (CPU)", String::new()) };
    let _ = vram_gb; let _ = warn.clone();
    serde_json::json!({"vendor": vendor, "gpuName": name, "vramGb": vram, "cuda": cuda, "torch": torch, "driverWarning": warn, "profile": kernel_profile_key(&vendor, &name)})
}
#[tauri::command]
pub fn detect_gpus() -> serde_json::Value {
    
    let mut gpus = Vec::new();
    if let Ok(out) = silent_command("nvidia-smi").args(["--query-gpu=index,name,memory.total", "--format=csv,noheader"]).output() {
        if out.status.success() {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                let parts: Vec<&str> = line.split(',').map(str::trim).collect();
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
pub fn detect_hardware() -> serde_json::Value {
    let gpus = detect_gpus();
    let gpu_name = gpus.as_array().and_then(|a| a.first()).and_then(|g| g.get("name")).and_then(|n| n.as_str()).unwrap_or("—").to_string();
    let vram = gpus.as_array().and_then(|a| a.first()).and_then(|g| g.get("vramMB")).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
    let vram_str = if vram > 0.0 { format!("{} MB", vram as i64) } else { "—".into() };
    // CPU/RAM via sysinfo — ~0ms vs 800ms powershell; brand string is equivalent to WMI Name
    let (cpu, ram) = {
        use sysinfo::{System, RefreshKind, MemoryRefreshKind, CpuRefreshKind};
        let mut sys = SYSINFO_CACHE.get_or_init(|| Mutex::new(System::new_with_specifics(RefreshKind::nothing().with_cpu(CpuRefreshKind::everything()).with_memory(MemoryRefreshKind::everything())))).lock().unwrap();
        // refresh only if stale (>4s) to avoid re-allocating every detect_hardware call
        sys.refresh_memory();
        sys.refresh_cpu_all();
        let total = sys.total_memory();
        let ram_s = if total > 0 { format!("{} GB", total / 1073741824) } else { "—".into() };
        let cpu_s = sys.cpus().first().map(|c| c.brand().trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| std::env::var("PROCESSOR_IDENTIFIER").unwrap_or("—".into()));
        // If brand already contains GHz, don't append; else append frequency hint from sysinfo
        let cpu_s = if cpu_s.contains("GHz") || cpu_s.contains("MHz") { cpu_s } else {
            let freq = sys.cpus().first().map_or(0, sysinfo::Cpu::frequency);
            if freq > 0 { format!("{} ({:.2} GHz)", cpu_s, freq as f64 / 1000.0) } else { cpu_s }
        };
        (cpu_s, ram_s)
    };
    serde_json::json!({"cpu": cpu, "ram": ram, "gpu": gpu_name, "vram": vram_str})
}

#[tauri::command]
pub fn get_hardware_profile() -> serde_json::Value {
    // mirrors Electron get-hardware-profile — returns profile string + packages/kernels so frontend .join() never crashes
    let gpus = detect_gpus();
    let vram_mb = gpus.as_array().and_then(|a| a.first()).and_then(|g| g.get("vramMB")).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
    let vram_gb = vram_mb / 1024.0;
    let gpu = get_gpu_info_sync();
    let vendor = gpu.get("vendor").and_then(|v| v.as_str()).unwrap_or("UNKNOWN");
    let name = gpu.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let key = kernel_profile_key(vendor, name);
    let (profile_str, packages, kernels) = match key.as_str() {
        "RTX_50" => ("RTX_50", vec!["torch","triton","sageattention","spas_sage_attn","flash_attn"], vec!["nunchaku","lightx2v","gguf"]),
        "RTX_40" | "RTX_30" | "RTX_20" => (key.as_str(), vec!["torch","triton","sageattention","spas_sage_attn","flash_attn"], vec!["nunchaku","gguf"]),
        "GTX_10" => ("GTX_10", vec!["torch"], vec![] as Vec<&str>),
        _ => (key.as_str(), vec!["torch"], vec![] as Vec<&str>),
    };
    let pnum = if vram_gb >= 24.0 { 1 } else if vram_gb >= 12.0 { 4 } else { 5 };
    serde_json::json!({
        "profile": profile_str,
        "vramGb": vram_gb,
        "profileNum": pnum,
        "detail": { "kernels": kernels.clone(), "python": "3.11.14", "torch": "2.10.0 CU13", "profile": profile_str },
        "packages": packages,
        "kernels": kernels.iter().map(|k| serde_json::json!({"label": *k, "dist": *k})).collect::<Vec<_>>(),
        "kernelsRaw": kernels
    })
}

static PREV_CPU: OnceLock<Mutex<Option<(u64,u64)>>> = OnceLock::new();
static LAST_NVIDIA: OnceLock<Mutex<Option<serde_json::Value>>> = OnceLock::new();
pub(crate) fn get_cached_igpu() -> Option<serde_json::Value> {
    // ponytail: WMI probed once, cached forever — was running powershell every 2s in hot loop
    let m = CACHED_IGPU.get_or_init(|| Mutex::new(None));
    if let Ok(g) = m.lock() { if let Some(v) = g.clone() { return Some(v); } }
    // first call: run WMI, cache result (even None as explicit)
    let mut igpu: Option<serde_json::Value> = None;
    #[cfg(windows)] {
        if let Ok(wmi_out) = silent_command("powershell").args(["-NoProfile","-Command","Get-CimInstance Win32_VideoController | Select-Object Name,AdapterRAM | ForEach-Object { $_.Name + '|' + $_.AdapterRAM }"]).output() {
            if wmi_out.status.success() {
                let wmi_s = String::from_utf8_lossy(&wmi_out.stdout).trim().to_string();
                for ln in wmi_s.lines() {
                    if let Some((n, r)) = ln.split_once('|') {
                        let name = n.trim();
                        if name.is_empty() || name.to_lowercase().contains("nvidia") { continue; }
                        let lower = name.to_lowercase();
                        if lower.contains("intel") || lower.contains("amd") || lower.contains("radeon") || lower.contains("arc") {
                            let vram_mb = r.trim().parse::<i64>().unwrap_or(0) / (1024*1024);
                            let fmt2 = if vram_mb>0 { format!("{vram_mb} MB") } else { "—".into() };
                            igpu = Some(serde_json::json!({"name": name, "vram": fmt2}));
                            break;
                        }
                    }
                }
            }
        }
    }
    // cache sentinel: if no igpu found, store Null so we don't re-probe
    if let Ok(mut g) = m.lock() { *g = igpu.clone().or(Some(serde_json::Value::Null)); }
    igpu
}
#[tauri::command]
pub fn get_system_metrics() -> serde_json::Value {
    // throttle: if called <1.2s ago, return cached metrics (prevents double-fire from dashboard+polling)
    if let Some(m) = METRICS_CACHE.get() {
        if let Ok(g) = m.lock() {
            if let Some((t, v)) = g.as_ref() {
                if t.elapsed() < std::time::Duration::from_millis(1200) { return v.clone(); }
            }
        }
    }
    let mut result = serde_json::json!({"ramFree": null, "vramFree": null, "cpu": null, "gpu": null, "ramUsed": null, "ramTotal": null, "vramUsed": null, "vramTotal": null, "ram": null, "vram": null, "gpus": [], "gpu2": null, "vram2": null, "vramFree2": null, "vramUsed2": null, "vramTotal2": null});
    // RAM/CPU via reused sysinfo instance (no alloc per tick)
    {
        use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind};
        let m = SYSINFO_CACHE.get_or_init(|| Mutex::new(sysinfo::System::new_with_specifics(RefreshKind::nothing().with_cpu(CpuRefreshKind::everything()).with_memory(MemoryRefreshKind::everything()))));
        let mut sys = m.lock().unwrap();
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
        let out = silent_command("nvidia-smi").args(["--query-gpu=memory.free,memory.used,memory.total,utilization.gpu", "--format=csv,noheader,nounits"]).output();
        if let Ok(o) = out { if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !s.is_empty() {
                let mut free=0i64; let mut used=0i64; let mut total=0i64; let mut gpu=0i64; let mut cnt=0;
                let mut per_gpu: Vec<serde_json::Value> = Vec::new();
                for line in s.lines() {
                    let p: Vec<&str> = line.split(',').map(str::trim).collect();
                    if p.len()>=4 {
                        let f = p[0].parse::<i64>().unwrap_or(0);
                        let u = p[1].parse::<i64>().unwrap_or(0);
                        let t = p[2].parse::<i64>().unwrap_or(0);
                        let g = p[3].parse::<i64>().unwrap_or(0);
                        free+=f; used+=u; total+=t; gpu+=g; cnt+=1;
                        let fmt_mb = |mb: i64| if mb>=1024 { format!("{} GB", (mb as f64/1024.0).round() as i64) } else { format!("{mb} MB") };
                        let vram_pct = if t>0 { (u as f64 / t as f64 * 100.0).round() as i64 } else { 0 };
                        per_gpu.push(serde_json::json!({"free": f, "used": u, "total": t, "gpu": g, "vram": vram_pct, "vramFree": fmt_mb(f), "vramUsed": fmt_mb(u), "vramTotal": fmt_mb(t)}));
                    }
                }
                let fmt = |mb: i64| if mb>=1024 { format!("{} GB", (mb as f64/1024.0).round() as i64) } else { format!("{mb} MB") };
                result["vramFree"] = serde_json::Value::String(fmt(free));
                result["vramUsed"] = serde_json::Value::String(fmt(used));
                result["vramTotal"] = serde_json::Value::String(fmt(total));
                if total>0 { result["vram"] = serde_json::json!((used as f64/total as f64*100.0).round() as i64); }
                result["gpu"] = serde_json::json!(if cnt>1 { (gpu as f64/f64::from(cnt)).round() as i64 } else { gpu });
                // build gpus array for topbar
                let mut gpus_arr: Vec<serde_json::Value> = per_gpu.iter().enumerate().map(|(i,g)| {
                    serde_json::json!({"index": i, "gpu": g["gpu"], "vram": g["vram"], "vramFree": g["vramFree"], "vramUsed": g["vramUsed"], "vramTotal": g["vramTotal"]})
                }).collect();
                // iGPU fallback: cached once, not every 2s (was 400ms powershell in hot loop)
                if gpus_arr.len() == 1 {
                    if let Some(igpu) = get_cached_igpu() {
                        if !igpu.is_null() {
                            let name = igpu.get("name").and_then(|v| v.as_str()).unwrap_or("");
                            let fmt2 = igpu.get("vram").and_then(|v| v.as_str()).unwrap_or("—").to_string();
                            gpus_arr.push(serde_json::json!({"index": gpus_arr.len(), "gpu": 0, "vram": null, "vramFree": fmt2.clone(), "vramUsed": "0 MB", "vramTotal": fmt2, "name": name}));
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
        } else if let Some(last) = LAST_NVIDIA.get().and_then(|m| m.lock().ok()).and_then(|g| g.clone()) {
            result["vramFree"] = last["vramFree"].clone(); result["vramUsed"] = last["vramUsed"].clone(); result["vramTotal"] = last["vramTotal"].clone(); result["vram"] = last["vram"].clone(); result["gpu"] = last["gpu"].clone();
            result["gpus"] = last["gpus"].clone(); result["gpu2"] = last["gpu2"].clone(); result["vram2"] = last["vram2"].clone(); result["vramFree2"] = last["vramFree2"].clone(); result["vramUsed2"] = last["vramUsed2"].clone(); result["vramTotal2"] = last["vramTotal2"].clone();
        }}
    }
    // cache for throttle
    if let Ok(mut g) = METRICS_CACHE.get_or_init(|| Mutex::new(None)).lock() { *g = Some((std::time::Instant::now(), result.clone())); }
    result
}


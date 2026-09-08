//! GPU / hardware probing and system metrics.
use std::sync::{Mutex, OnceLock};
use crate::base::*;

// ── GPU helpers ──
pub(crate) fn get_gpu_info_sync() -> serde_json::Value {
    
    if let Ok(out) = probe_command("NVIDIA_SMI", "nvidia-smi").args(["--query-gpu=name,memory.total,driver_version", "--format=csv,noheader"]).output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                let parts: Vec<&str> = s.split(", ").collect();
                return serde_json::json!({"vendor":"NVIDIA","name":parts.first().unwrap_or(&"").trim(),"vramMB":parts.get(1).unwrap_or(&"0 MiB").trim(),"driverVersion":parts.get(2).unwrap_or(&"").trim(),"raw":s});
            }
        }
    }
    // No NVIDIA driver — WMI fallback for AMD/Intel (Electron queryGpuList parity).
    // Doc-leading: deepbeepmeep docs/AMD-INSTALLATION.md is the spec for AMD support.
    if let Some((name, vendor, _raw_bytes, driver)) = wmi_gpu_fallback() {
        // Prefer 64-bit registry VRAM (AdapterRAM caps ~4GB / often 0).
        match wmi_dedicated_vram_mb(&name) {
            Some(mb) => return serde_json::json!({"vendor":vendor,"name":name,"vramMB":format!("{mb} MiB"),"driverVersion":driver,"raw":format!("WMI+REG: {name}")}),
            // Known-card table before giving up (R9700 PRO driver layout
            // reports no parseable registry size).
            None => match known_vram_mb(&name) {
                Some(mb) => return serde_json::json!({"vendor":vendor,"name":name,"vramMB":format!("{mb} MiB"),"driverVersion":driver,"raw":format!("WMI+TABLE: {name}")}),
                None => return serde_json::json!({"vendor":vendor,"name":name,"vramMB":"0 MiB","driverVersion":driver,"raw":format!("WMI: {name} (VRAM unknown)")}),
            },
        }
    }
    serde_json::json!({"vendor":"unknown","name":"","vramMB":"0","driverVersion":"","raw":"nvidia-smi not found"})
}

/// Probe spawn with a hardware-simulation hook for integration tests.
/// `WGP_PROBE_<KIND>` (WGP_PROBE_NVIDIA_SMI / WGP_PROBE_POWERSHELL) redirects
/// the probe through `cmd /C <fake>`; unset in production, where this is
/// exactly silent_command. Lets tests simulate an AMD box (no nvidia-smi,
/// canned WMI/registry answers) on any machine without touching prod behavior.
pub(crate) fn probe_command(kind: &str, bin: &str) -> std::process::Command {
    if let Ok(fake) = std::env::var(format!("WGP_PROBE_{kind}")) {
        let mut c = silent_command("cmd");
        c.args(["/C", &fake, "--"]);
        let _ = bin;
        return c;
    }
    silent_command(bin)
}

/// WMI fallback for non-NVIDIA GPUs on Windows (AMD/Intel).
/// Returns (display name, vendor, AdapterRAM bytes). AdapterRAM is a 32-bit
/// field — capped at ~4GB and frequently 0 — so callers must treat small/zero
/// values as "VRAM unknown", never as truth (mirrors queryGpuList).
/// All non-NVIDIA video controllers: (display name, vendor, AdapterRAM
/// bytes, driver version). One WMI call serves fallback selection,
/// multi-GPU warnings and the driver preflight check. Missing DriverVersion
/// (older query shapes, canned test fakes) parses as "".
#[cfg(windows)]
pub(crate) fn wmi_all_gpus() -> Vec<(String, String, u64, String)> {
    let out = match probe_command("POWERSHELL", "powershell").args(["-NoProfile","-Command","Get-CimInstance Win32_VideoController | ForEach-Object { $_.Name + '|' + $_.AdapterRAM + '|' + $_.DriverVersion }"]).output() {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };
    let s = String::from_utf8_lossy(&out.stdout);
    let mut all = Vec::new();
    for ln in s.lines() {
        let mut parts = ln.split('|');
        let (Some(n), Some(r)) = (parts.next(), parts.next()) else { continue; };
        let name = n.trim().to_string();
        if name.is_empty() || name.to_lowercase().contains("nvidia") { continue; }
        let lower = name.to_lowercase();
        let vendor = if lower.contains("amd") || lower.contains("radeon") { "AMD" }
            else if lower.contains("intel") || lower.contains("arc") { "INTEL" }
            else { continue; };
        let raw = r.trim().parse::<u64>().unwrap_or(0);
        let driver = parts.next().unwrap_or("").trim().to_string();
        all.push((name, vendor.to_string(), raw, driver));
    }
    all
}
#[cfg(not(windows))]
pub(crate) fn wmi_all_gpus() -> Vec<(String, String, u64, String)> { Vec::new() }

#[cfg(windows)]
pub(crate) fn wmi_gpu_fallback() -> Option<(String, String, u64, String)> {
    // First non-NVIDIA controller wins (historical behavior); preflight
    // warns when several AMD entries make that order significant.
    wmi_all_gpus().into_iter().next()
}
#[cfg(not(windows))]
pub(crate) fn wmi_gpu_fallback() -> Option<(String, String, u64, String)> { None }

/// Win32_VideoController.AdapterRAM is a 32-bit field: cards with ≥4GB VRAM
/// report 0 or the 0xFFFFFFFF cap (≈4095MB) — a 32GB R9700 otherwise shows
/// up as a 4GB card (the exact user report). Either value is "VRAM unknown",
/// never truth. (A genuine sub-2GB iGPU is likewise unknown for tiering.)
/// Pure + cross-platform so unit tests cover it on any host.
pub(crate) fn adapter_ram_known_mb(raw: u64) -> Option<f64> {
    if raw == 0 || raw >= 0xFFF0_0000 {
        return None;
    }
    let mb = raw as f64 / (1024.0 * 1024.0);
    if mb < 2048.0 || mb >= 4095.0 { None } else { Some(mb) }
}

/// AMD display-driver verdict from a Win32_VideoController DriverVersion
/// (`32.0.11029.1008` style). AMD moved to 32.x with the 2024 / Adrenalin
/// 24.x releases, so major < 32 ≈ pre-24.x → update recommended (TheRock
/// wants ≥ 24.5). Unparseable/empty → "unknown" (warn, don't block).
/// "Basic Display Adapter" is detected on the NAME by callers, not here.
/// Pure + unit-tested.
pub(crate) fn classify_amd_driver(version: &str) -> &'static str {
    let major: Option<u64> = version.trim().split(['.', ' ']).next().and_then(|m| m.parse().ok());
    match major {
        None => "unknown",
        Some(m) if m < 32 => "old",
        Some(_) => "ok",
    }
}

/// Distinctive GPU-name tokens: alphanumeric runs (len ≥ 4) containing a
/// digit — R9700, 9070, 7900XTX. Lets the registry match survive the small
/// WMI-vs-DriverDesc wording differences ("Radeon AI PRO R9700" vs
/// "AMD Radeon AI PRO R9700") without ever matching a generic iGPU entry.
fn distinctive_tokens(name: &str) -> Vec<String> {
    name.to_uppercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| t.len() >= 4 && t.bytes().any(|b| b.is_ascii_digit()))
        .map(str::to_string)
        .collect()
}

/// Dedicated-VRAM fallback (MB) for known cards by name token.
/// The registry probe (wmi_dedicated_vram_mb) misses some driver layouts
/// (the R9700 reporter's PRO driver returned nothing) — for known cards
/// the size is fixed, so a name match beats "VRAM unknown" (which tiers a
/// 32GB card as 8GB downstream in setup.py's pid selection). Consulted
/// ONLY when WMI AdapterRAM and the registry both yield nothing, and only
/// on the non-NVIDIA path. Laptop S-suffixed SKUs may differ by a few GB —
/// tier-safe (thresholds are 11/22GB). Pure + unit-tested.
pub(crate) fn known_vram_mb(name: &str) -> Option<u64> {
    let g = name.to_uppercase();
    const GB: u64 = 1024;
    // Radeon PRO first: W7900/W7800/W6800 contain consumer tokens
    // ("7900"/"7800"/"6800") with DIFFERENT sizes.
    if g.contains("W7900") { return Some(48 * GB); }
    if g.contains("W7800") || g.contains("W6800") { return Some(32 * GB); }
    if g.contains("W7700") { return Some(16 * GB); }
    if g.contains("W6600") || g.contains("W6400") { return Some(8 * GB); }
    if g.contains("R9700") { return Some(32 * GB); }
    // RDNA 4 consumer (9070 GRE is the 12GB exception).
    if g.contains("9070") { return Some(if g.contains("GRE") { 12 * GB } else { 16 * GB }); }
    // RDNA 3 dGPU.
    if g.contains("7900") {
        if g.contains("XTX") { return Some(24 * GB); }
        if g.contains("GRE") { return Some(16 * GB); }
        return Some(20 * GB); // 7900 XT
    }
    if g.contains("7800") { return Some(16 * GB); }
    if g.contains("7700") { return Some(12 * GB); }
    if g.contains("7600") { return Some(if g.contains("XT") { 16 * GB } else { 8 * GB }); }
    // RDNA 2 dGPU.
    if g.contains("6950") || g.contains("6900") || g.contains("6800") { return Some(16 * GB); }
    if g.contains("6750") || g.contains("6700") { return Some(12 * GB); }
    if g.contains("6650") || g.contains("6600") { return Some(8 * GB); }
    if g.contains("6500") || g.contains("6400") { return Some(4 * GB); }
    None
}

/// 64-bit dedicated VRAM (MB) from the display-driver registry key.
/// WMI AdapterRAM is uint32 (caps ~4GB, often 0); the driver also reports
/// the size as a QWORD — exact for 32GB cards. Consumer drivers expose it
/// as HardwareInformation.MemorySize, AMD PRO drivers (R9700 reporter box)
/// as HardwareInformation.qwMemorySize — read both, first parseable wins.
/// Fast (<300ms, detection paths only, never the metrics tick), no new deps.
/// Returns None when absent/implausible — callers keep "VRAM unknown".
#[cfg(windows)]
pub(crate) fn wmi_dedicated_vram_mb(display_name: &str) -> Option<u64> {
    let out = probe_command("POWERSHELL", "powershell").args(["-NoProfile","-Command",
        "Get-ChildItem 'HKLM:\\SYSTEM\\CurrentControlSet\\Control\\Class\\{4d36e968-e325-11ce-bfc1-08002be10318}' | ForEach-Object { $d = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue; if ($d.DriverDesc) { $d.DriverDesc.ToString() + '|' + $d.'HardwareInformation.MemorySize' + '|' + $d.'HardwareInformation.qwMemorySize' } }"
    ]).output().ok()?;
    if !out.status.success() { return None; }
    let s = String::from_utf8_lossy(&out.stdout);
    let want = display_name.to_lowercase();
    let want_toks = distinctive_tokens(display_name);
    let mut cands: Vec<(String, u64)> = Vec::new();
    for ln in s.lines() {
        let mut parts = ln.split('|');
        let Some(desc) = parts.next() else { continue; };
        // First parseable QWORD wins (MemorySize on consumer, qwMemorySize
        // on PRO drivers; single-pipe legacy output keeps working).
        let mut mb: Option<u64> = None;
        for mem in parts {
            if let Ok(bytes) = mem.trim().parse::<u64>() {
                if let Some(m) = bytes.checked_div(1024 * 1024) {
                    if m != 0 && m <= 262144 { mb = Some(m); break; }
                }
            }
        }
        let Some(mb) = mb else { continue; };
        let d = desc.trim().to_lowercase();
        if d.is_empty() || d.contains("nvidia") { continue; }
        cands.push((d, mb));
    }
    // 1) Name match (WMI vs DriverDesc can differ slightly).
    for (d, mb) in &cands {
        if d.contains(&want) || want.contains(d) { return Some(*mb); }
    }
    // 2) Distinctive-token match (R9700/9070/…); max VRAM wins so a
    // duplicated driver key can't under-report, and a generic iGPU entry
    // (no digit tokens) can never match.
    if !want_toks.is_empty() {
        let mut best: Option<u64> = None;
        for (d, mb) in &cands {
            let dtoks = distinctive_tokens(d);
            if want_toks.iter().any(|t| dtoks.contains(t)) {
                best = Some(best.map_or(*mb, |b: u64| b.max(*mb)));
            }
        }
        if let Some(mb) = best { return Some(mb); }
    }
    // 3) Single-candidate fallback only — never attribute an iGPU's VRAM
    // to a dGPU or vice versa.
    if cands.len() == 1 { return Some(cands[0].1); }
    None
}
#[cfg(not(windows))]
pub(crate) fn wmi_dedicated_vram_mb(_display_name: &str) -> Option<u64> { None }

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
        // RDNA 2 (gfx103X-dgpu): no upstream setup_config profile — dedicated key so
        // install/launch treat it per docs/AMD-INSTALLATION.md instead of GFX110X.
        if g.contains("GFX103")||g.contains("RX 6")||["6300","6400","6450","6500","6600","6650","6700","6750","6800","6850","6900","6950","W6200","W6400","W6600","W6800"].iter().any(|x| g.contains(x)) { return "AMD_GFX103X".into(); }
        if g.contains("7600")||g.contains("7700")||g.contains("7800")||g.contains("7900")||g.contains("780M") { return "AMD_GFX110X".into(); }
        if g.contains("890M")||g.contains("STRIX")||g.contains("HALO")||g.contains("Z1")||g.contains("PHOENIX") { return "AMD_GFX1151".into(); }
        if g.contains("9060")||g.contains("9070")||g.contains("9700")||g.contains("9000")||g.contains("8000")||g.contains("1201") { return "AMD_GFX1201".into(); }
        return "AMD_GFX110X".into();
    }
    // Intel → INTEL_XPU key, but CPU torch (no XPU backend exists upstream;
    // overview shows INTEL_CPU via hardware_profile_detail). Unknown → CPU.
    // Never alias an NVIDIA profile: the overview would promise CUDA wheels
    // the installer never installs (must match kernel-resolver.js kernelProfileKey).
    if v == "INTEL" { return "INTEL_XPU".into(); }
    "CPU".into()
}

/// GGUF wheel URLs shipped by upstream (docs/INSTALLATION.md#gguf-llamacpp-cuda-kernels).
const GGUF_1021_WIN_PY311: &str = "https://github.com/deepbeepmeep/kernels/releases/download/gguf-v1.0.21/llamacpp_gguf_cuda-1.0.21%2Btorch210cu130py311-cp311-cp311-win_amd64.whl";
const GGUF_1021_WIN_PY310: &str = "https://github.com/deepbeepmeep/kernels/releases/download/gguf-v1.0.21/llamacpp_gguf_cuda-1.0.21%2Btorch271cu128py310-cp310-cp310-win_amd64.whl";
/// GGUF wheel override toward the documented 1.0.21 build (RTX50 SM120
/// kernels, 50-100% Deepy decode speedup). setup_config.json still ships
/// 1.0.14, so swap 1.0.14 → 1.0.21 — but pass anything else through untouched,
/// so the day upstream flips setup_config we follow it verbatim with no code
/// change (same shape as the Sage post4/post6 swap in sync_kernels).
/// Applies to every kernel URL (no-op unless it's a 1.0.14 GGUF link), so both
/// the sync installer and the overview's want/have comparison share it.
pub(crate) fn apply_gguf_override(url: &str) -> String {
    if !url.contains("llamacpp_gguf_cuda-1.0.14") { return url.to_string(); }
    if url.contains("py310") { GGUF_1021_WIN_PY310.into() } else { GGUF_1021_WIN_PY311.into() }
}

#[cfg(test)]
mod amd_profile_tests {
    use super::kernel_profile_key;
    #[test]
    fn r9700_maps_to_gfx1201() {
        // Radeon AI PRO R9700 = gfx1201 (Navi 48, RDNA 4) — doc-leading per
        // docs/AMD-INSTALLATION.md; must not fall through to AMD_GFX110X.
        assert_eq!(kernel_profile_key("AMD", "AMD Radeon AI PRO R9700"), "AMD_GFX1201");
        assert_eq!(kernel_profile_key("AMD", "AMD Radeon RX 9070 XT"), "AMD_GFX1201");
        assert_eq!(kernel_profile_key("AMD", "AMD Radeon RX 7900 XTX"), "AMD_GFX110X");
        // RDNA 2 has its own key (no upstream setup_config profile to collide with).
        assert_eq!(kernel_profile_key("AMD", "AMD Radeon RX 6800 XT"), "AMD_GFX103X");
        assert_eq!(kernel_profile_key("AMD", "AMD Radeon RX 6700S"), "AMD_GFX103X");
    }
}
#[cfg(test)]
mod amd_driver_tests {
    use super::classify_amd_driver;
    #[test]
    fn driver_verdicts() {
        // Adrenalin/Pro 24.x era (32.x) — current.
        assert_eq!(classify_amd_driver("32.0.11029.1008"), "ok");
        assert_eq!(classify_amd_driver("32.0.12019.1028"), "ok");
        // 23.x era (31.x) — predates the 24.5 TheRock floor: warn.
        assert_eq!(classify_amd_driver("31.0.21029.1006"), "old");
        assert_eq!(classify_amd_driver("30.0.13025.1000"), "old");
        // Unreadable — warn, never block.
        assert_eq!(classify_amd_driver(""), "unknown");
        assert_eq!(classify_amd_driver("not-a-version"), "unknown");
    }
}
#[cfg(test)]
mod known_vram_tests {
    use super::known_vram_mb;
    #[test]
    fn known_cards_resolve() {
        // The 0.5.1 reporter card: registry probe missed, table must hit.
        assert_eq!(known_vram_mb("AMD Radeon AI PRO R9700"), Some(32768));
        assert_eq!(known_vram_mb("AMD Radeon RX 9070 XT"), Some(16384));
        assert_eq!(known_vram_mb("AMD Radeon RX 9070 GRE"), Some(12288));
        assert_eq!(known_vram_mb("AMD Radeon RX 7900 XTX"), Some(24576));
        assert_eq!(known_vram_mb("AMD Radeon RX 7900 XT"), Some(20480));
        assert_eq!(known_vram_mb("AMD Radeon RX 7800 XT"), Some(16384));
        assert_eq!(known_vram_mb("AMD Radeon RX 7700 XT"), Some(12288));
        assert_eq!(known_vram_mb("AMD Radeon RX 7600"), Some(8192));
        assert_eq!(known_vram_mb("AMD Radeon RX 7600 XT"), Some(16384));
        assert_eq!(known_vram_mb("AMD Radeon RX 6800 XT"), Some(16384));
        assert_eq!(known_vram_mb("AMD Radeon RX 6700 XT"), Some(12288));
        assert_eq!(known_vram_mb("AMD Radeon RX 6600"), Some(8192));
        assert_eq!(known_vram_mb("AMD Radeon PRO W7900"), Some(49152));
        assert_eq!(known_vram_mb("AMD Radeon PRO W7800"), Some(32768));
    }
    #[test]
    fn unknown_names_stay_unknown() {
        assert_eq!(known_vram_mb("AMD Radeon Graphics"), None); // generic iGPU
        assert_eq!(known_vram_mb("Intel Arc A770"), None);
        assert_eq!(known_vram_mb(""), None);
    }
}
#[cfg(test)]
mod gguf_override_tests {
    use super::apply_gguf_override;
    #[test]
    fn swaps_1014_for_1021() {
        let old_win = "https://github.com/deepbeepmeep/kernels/releases/download/GGUF_Kernels/llamacpp_gguf_cuda-1.0.14+torch210cu130py311-cp311-cp311-win_amd64.whl";
        let new = apply_gguf_override(old_win);
        assert!(new.contains("gguf-v1.0.21") && new.contains("1.0.21"), "got {new}");
        assert!(!new.contains("1.0.14"));
        let old_310 = old_win.replace("py311", "py310").replace("torch210cu130py311", "torch271cu128py310");
        assert!(apply_gguf_override(&old_310).contains("torch271cu128py310"));
    }
    #[test]
    fn passes_other_urls_through() {
        for u in [
            "https://github.com/deepbeepmeep/kernels/releases/download/gguf-v1.0.21/llamacpp_gguf_cuda-1.0.21+torch210cu130py311-cp311-cp311-win_amd64.whl",
            "https://github.com/nunchaku-ai/nunchaku/releases/download/v1.2.1/nunchaku-1.2.1+cu13.0torch2.10-cp311-cp311-win_amd64.whl",
        ] { assert_eq!(apply_gguf_override(u), u); }
    }
}
pub(crate) fn build_install_plan(hw: &serde_json::Value) -> serde_json::Value {
    let vendor = hw.get("vendor").and_then(|v| v.as_str()).unwrap_or("UNKNOWN").to_uppercase();
    let name = hw.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let vram = hw.get("vramMB").and_then(|v| v.as_str()).unwrap_or("0").split_whitespace().next().unwrap_or("0").parse::<f64>().unwrap_or(0.0);
    let driver = hw.get("driverVersion").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let vram_gb = vram / 1024.0_f64; let is_gtx = name.to_uppercase().contains("GTX 10") || name.to_uppercase().contains("GTX 16") || (name.contains("10") && vendor=="NVIDIA" && (name.contains("1050")||name.contains("1060")||name.contains("1650")));
    let (cuda, torch, warn) = if vendor=="NVIDIA" { if is_gtx { ("CUDA 12.8", "PyTorch 2.7.1", String::new()) } else { let mut w=String::new(); if let Ok(dv)=driver.parse::<f64>() { if dv < 580.0 { w=format!("NVIDIA driver {driver} < R580 — cu130 needs R580+"); }} ("CUDA 13 (cu130)", "PyTorch 2.10", w) } } else if vendor=="AMD" { ("ROCm 7.15 (TheRock)", "PyTorch 2.12 (ROCm 7.15)", String::new()) } else if vendor=="APPLE" { ("MPS (Metal)", "PyTorch (MPS)", String::new()) } else { ("CPU", "PyTorch (CPU)", String::new()) };
    let _ = vram_gb; let _ = warn.clone();
    serde_json::json!({"vendor": vendor, "gpuName": name, "vramGb": vram, "cuda": cuda, "torch": torch, "driverWarning": warn, "profile": kernel_profile_key(&vendor, &name)})
}
#[tauri::command]
pub fn detect_gpus() -> serde_json::Value {
    
    let mut gpus = Vec::new();
    if let Ok(out) = probe_command("NVIDIA_SMI", "nvidia-smi").args(["--query-gpu=index,name,memory.total", "--format=csv,noheader"]).output() {
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
    // WMI fallback for AMD/Intel (Electron queryGpuList parity, dropped in port).
    if let Some((name, vendor, raw, _driver)) = wmi_gpu_fallback() {
        // 64-bit registry VRAM first — exact on 32GB cards (R9700 shows 32768).
        if let Some(mb) = wmi_dedicated_vram_mb(&name) {
            return serde_json::json!([{"index": 0, "name": name, "vramMB": mb as f64, "vendor": vendor}]);
        }
        // AdapterRAM caps at ~4GB (0/0xFFFFFFFF on big cards) — never truth.
        // Known-card table before giving up (same fallback as detect path).
        match adapter_ram_known_mb(raw) {
            Some(mb) => return serde_json::json!([{"index": 0, "name": name, "vramMB": mb, "vendor": vendor}]),
            None => match known_vram_mb(&name) {
                Some(mb) => return serde_json::json!([{"index": 0, "name": name, "vramMB": mb as f64, "vendor": vendor}]),
                None => return serde_json::json!([{"index": 0, "name": format!("{name} (VRAM unknown)"), "vramMB": 0.0, "vendor": vendor}]),
            },
        }
    }
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

/// Friendly component labels (mirrors Electron COMP_LABEL — same strings).
fn comp_label(code: &str) -> String {
    match code {
        "cu128" => "PyTorch 2.7.1 + CUDA 12.8".into(),
        "cu130" => "PyTorch 2.10.0 + CUDA 13.0".into(),
        // Key stays rocm65 (upstream setup_config.json schema) — the label is
        // what the installer actually puts down (exact-pinned 7.15 stack).
        "rocm65" => "PyTorch 2.12 + ROCm 7.15 (TheRock)".into(),
        "mps" => "PyTorch (MPS)".into(),
        "v33" => "Triton < 3.3".into(),
        "v34" => "Triton < 3.4".into(),
        "latest" => "Triton (latest)".into(),
        "v1" => "Sage Attention 1.0.6".into(),
        "v211" => "Sage Attention 2.1.1".into(),
        "v220" | "v220_cu13" => "Sage Attention 2.2.0 (CUDA 13)".into(),
        "v010_cu128" => "Sparge 0.1.0 (CUDA 12.8)".into(),
        "v010_cu13" => "Sparge 0.1.0 (CUDA 13)".into(),
        "v210" => "Flash Attention 2.8.3".into(),
        other => other.to_string(),
    }
}

/// Kernel display names (mirrors Electron KERNEL_DISPLAY).
fn kernel_display(key: &str) -> (&str, &str) {
    match key {
        "nunchaku" | "nunchaku_cu13" => ("Nunchaku", "nunchaku"),
        "gguf" | "llamacpp_gguf_cuda" => ("GGUF (llamacpp)", "llamacpp_gguf_cuda"),
        "lightx2v" | "light2xv" | "lightx2v_kernel" => ("LightX2V", "lightx2v_kernel"),
        "sageattention" => ("SageAttention", "sageattention"),
        "spas_sage_attn" => ("Sparge (Sage)", "spas_sage_attn"),
        "flash_attn" => ("FlashAttention", "flash_attn"),
        "bitsandbytes" => ("bitsandbytes NF4", "bitsandbytes"),
        _ => (key, key),
    }
}

#[tauri::command]
pub fn get_hardware_profile() -> serde_json::Value {
    // Full per-profile version matrix (mirrors Electron get-hardware-profile).
    // The Tauri port only sent python/torch, so the overview showed '—' for
    // Triton/Sage/Sparge/Flash and bare keys for kernel wheels.
    struct Prof { python: &'static str, torch: &'static str, triton: Option<&'static str>, sage: Option<&'static str>, sparge: Option<&'static str>, flash: Option<&'static str>, kernels: &'static [&'static str] }
    let gpus = detect_gpus();
    let vram_mb = gpus.as_array().and_then(|a| a.first()).and_then(|g| g.get("vramMB")).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
    let gpu = get_gpu_info_sync();
    return hardware_profile_detail(
        gpu.get("vendor").and_then(|v| v.as_str()).unwrap_or("UNKNOWN"),
        gpu.get("name").and_then(|v| v.as_str()).unwrap_or(""),
        vram_mb,
    );
}

/// Pure profile matrix behind get_hardware_profile (no hardware probes —
/// unit-testable). INTEL_XPU maps to a CPU-only, kernel-free INTEL_CPU row:
/// neither upstream nor this launcher ships an XPU backend, so the overview
/// must not promise CUDA wheels (the old `_` fallthrough did). Install,
/// launch and smoke paths are untouched — Intel boxes keep working exactly
/// as before (CPU torch), only the labels are honest now.
pub(crate) fn hardware_profile_detail(vendor: &str, name: &str, vram_mb: f64) -> serde_json::Value {
    struct Prof { python: &'static str, torch: &'static str, triton: Option<&'static str>, sage: Option<&'static str>, sparge: Option<&'static str>, flash: Option<&'static str>, kernels: &'static [&'static str] }
    let vram_gb = vram_mb / 1024.0;
    let key = kernel_profile_key(vendor, name);
    let (profile_str, prof) = match key.as_str() {
        "GTX_10" => ("GTX_10", Prof { python: "3.10.9", torch: "2.7.1 CU12.8", triton: None, sage: None, sparge: None, flash: None, kernels: &[] }),
        "RTX_20" => ("RTX_20", Prof { python: "3.11.14", torch: "2.10.0 CU13", triton: Some("latest"), sage: Some("1.0.6"), sparge: None, flash: Some("2.8.3"), kernels: &["nunchaku_cu13", "gguf"] }),
        "RTX_30" => ("RTX_30", Prof { python: "3.11.14", torch: "2.10.0 CU13", triton: Some("latest"), sage: Some("2.2.0"), sparge: Some("0.1.0"), flash: Some("2.8.3"), kernels: &["nunchaku_cu13", "gguf"] }),
        "RTX_40" => ("RTX_40", Prof { python: "3.11.14", torch: "2.10.0 CU13", triton: Some("latest"), sage: Some("2.2.0"), sparge: Some("0.1.0"), flash: Some("2.8.3"), kernels: &["nunchaku_cu13", "gguf"] }),
        "RTX_50" => ("RTX_50", Prof { python: "3.11.14", torch: "2.10.0 CU13", triton: Some("latest"), sage: Some("2.2.0"), sparge: Some("0.1.0"), flash: Some("2.8.3"), kernels: &["nunchaku_cu13", "light2xv", "gguf"] }),
        "MPS" => ("MPS", Prof { python: "3.11.14", torch: "MPS", triton: None, sage: None, sparge: None, flash: None, kernels: &[] }),
        k if k.starts_with("AMD") => ("AMD", Prof { python: "3.11.14", torch: "ROCm 7.15", triton: None, sage: None, sparge: None, flash: None, kernels: &[] }),
        // Intel → CPU torch, no kernels: XPU acceleration is not possible
        // (no upstream XPU backend exists). Display key is INTEL_CPU.
        "INTEL_XPU" => ("INTEL_CPU", Prof { python: "3.11.14", torch: "CPU", triton: None, sage: None, sparge: None, flash: None, kernels: &[] }),
        _ => (key.as_str(), Prof { python: "3.11.14", torch: "2.10.0 CU13", triton: Some("latest"), sage: Some("2.2.0"), sparge: Some("0.1.0"), flash: Some("2.8.3"), kernels: &["nunchaku_cu13", "gguf"] }),
    };
    // Package chips with versions (Electron order + emoji).
    let mut packages: Vec<String> = Vec::new();
    packages.push(format!("🐍 Python {}", prof.python));
    packages.push(format!("🔥 PyTorch {}", prof.torch));
    if let Some(t) = prof.triton { packages.push(format!("⚡ Triton ({t})")); }
    if let Some(s) = prof.sage { packages.push(format!("🌀 Sage Attn {s}")); }
    if let Some(s) = prof.sparge { packages.push(format!("🌊 Sparge Attn {s}")); }
    if let Some(f) = prof.flash { packages.push(format!("💥 Flash Attn {f}")); }
    packages.push("📋 50+ reqs (diffusers, gradio, opencv, moviepy…)".into());
    let kernel_labels: Vec<String> = prof.kernels.iter().map(|k| kernel_display(k).0.to_string()).collect();
    let detail = serde_json::json!({
        "profile": profile_str,
        "python": prof.python,
        "torch": prof.torch,
        "triton": prof.triton.map(comp_label).unwrap_or("—".into()),
        "sage": prof.sage.map(comp_label).unwrap_or("—".into()),
        "sparge": prof.sparge.map(comp_label).unwrap_or("—".into()),
        "flash": prof.flash.map(comp_label).unwrap_or("—".into()),
        "kernels": kernel_labels,
    });
    let pnum = if vram_gb >= 24.0 { 1 } else if vram_gb >= 12.0 { 4 } else { 5 };
    serde_json::json!({
        "profile": profile_str,
        "vramGb": vram_gb,
        "profileNum": pnum,
        "detail": detail,
        "packages": packages,
        "kernels": prof.kernels.iter().map(|k| { let (label, dist) = kernel_display(k); serde_json::json!({"label": label, "dist": dist}) }).collect::<Vec<_>>(),
        "kernelsRaw": prof.kernels
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
        if let Ok(wmi_out) = probe_command("POWERSHELL", "powershell").args(["-NoProfile","-Command","Get-CimInstance Win32_VideoController | Select-Object Name,AdapterRAM | ForEach-Object { $_.Name + '|' + $_.AdapterRAM }"]).output() {
            if wmi_out.status.success() {
                let wmi_s = String::from_utf8_lossy(&wmi_out.stdout).trim().to_string();
                for ln in wmi_s.lines() {
                    if let Some((n, r)) = ln.split_once('|') {
                        let name = n.trim();
                        if name.is_empty() || name.to_lowercase().contains("nvidia") { continue; }
                        let lower = name.to_lowercase();
                        if lower.contains("intel") || lower.contains("amd") || lower.contains("radeon") || lower.contains("arc") {
                            let raw_mb = r.trim().parse::<u64>().unwrap_or(0);
                            // Same 4GB-cap guard as detection (R9700 metrics tile
                            // showed a bogus 4095 MB before this).
                            let vram_mb = adapter_ram_known_mb(raw_mb).map(|mb| mb as i64).unwrap_or(0);
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
        let out = probe_command("NVIDIA_SMI", "nvidia-smi").args(["--query-gpu=memory.free,memory.used,memory.total,utilization.gpu", "--format=csv,noheader,nounits"]).output();
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
                // iGPU fallback: cached once, not every 2s (was 400ms powershell in hot loop).
                // Name-keyed tiles are WMI primaries (AMD/Intel-only box) — never append a dupe.
                if gpus_arr.len() == 1 && gpus_arr[0].get("name").is_none() {
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
    // AMD/Intel-only box (no nvidia-smi output): primary tile from the cached WMI probe.
    // Total prefers the 64-bit registry VRAM (exact on 32GB cards) over AdapterRAM.
    if result["gpus"].as_array().is_none_or(|a| a.is_empty()) {
        if let Some(igpu) = get_cached_igpu() {
            if !igpu.is_null() {
                let name = igpu.get("name").and_then(|v| v.as_str()).unwrap_or("GPU").to_string();
                let total = wmi_dedicated_vram_mb(&name)
                    .map(|mb| if mb >= 1024 { format!("{} GB", (mb as f64 / 1024.0).round() as i64) } else { format!("{mb} MB") })
                    .unwrap_or_else(|| igpu.get("vram").and_then(|v| v.as_str()).unwrap_or("—").to_string());
                result["gpus"] = serde_json::json!([{"index": 0, "gpu": null, "vram": null, "vramFree": total.clone(), "vramUsed": "—", "vramTotal": total, "name": name}]);
            }
        }
    }
    // cache for throttle
    if let Ok(mut g) = METRICS_CACHE.get_or_init(|| Mutex::new(None)).lock() { *g = Some((std::time::Instant::now(), result.clone())); }
    result
}


/// Simulated AMD-box integration test (Windows): fakes nvidia-smi (absent)
/// and powershell (canned R9700 WMI + registry answers) via the WGP_PROBE_*
/// hook, then drives the REAL detection → plan → torch-URL chain.
/// Pattern: `set "ARGS=%*"` is parse-safe for | $ { }; dispatch by substring.
#[cfg(all(test, windows))]
mod amd_sim_tests {
    use super::*;
    use std::sync::Mutex;
    static SIM_LOCK: Mutex<()> = Mutex::new(());
    fn write_fakes(dir: &std::path::Path) {
        std::fs::write(dir.join("nvidia-smi.cmd"), "@echo off\r\nexit /b 1\r\n").unwrap();
        let ps = "@echo off\r\nif \"%~1\"==\"--\" goto dispatch\r\nexit /b 1\r\n:dispatch\r\nif \"%~3\"==\"-Command\" goto powershell\r\nexit /b 1\r\n:powershell\r\nset \"Q=%~4\"\r\nif not \"%Q:Win32_VideoController=%\"==\"%Q%\" goto wmi\r\nif not \"%Q:HardwareInformation=%\"==\"%Q%\" goto reg\r\nexit /b 1\r\n:wmi\r\necho AMD Radeon AI PRO R9700^|0^|32.0.11029.1008\r\necho AMD Radeon Graphics^|0^|32.0.11029.1008\r\nexit /b 0\r\n:reg\r\nif defined WGP_SIM_NO_REG exit /b 1\r\necho AMD Radeon AI PRO R9700^|34359738368\r\nexit /b 0\r\n";
        std::fs::write(dir.join("powershell.cmd"), ps).unwrap();
    }
    #[test]
    fn simulated_r9700_end_to_end() {
        let _guard = SIM_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("wgp-sim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_fakes(&dir);
        let old_path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{};{old_path}", dir.display()));
        std::env::set_var("WGP_PROBE_NVIDIA_SMI", dir.join("nvidia-smi.cmd").to_string_lossy().to_string());
        std::env::set_var("WGP_PROBE_POWERSHELL", dir.join("powershell.cmd").to_string_lossy().to_string());
        std::env::remove_var("WGP_SIM_NO_REG");

        // detect → profile → install plan → torch URLs (AdapterRAM is 0: the
        // 32768 MB MUST come from the simulated registry QWORD).
        let gpus = detect_gpus();
        let g = &gpus.as_array().unwrap()[0];
        assert_eq!(g.get("vendor").and_then(|v| v.as_str()), Some("AMD"));
        assert_eq!(g.get("name").and_then(|v| v.as_str()), Some("AMD Radeon AI PRO R9700"));
        assert_eq!(g.get("vramMB").and_then(|v| v.as_f64()), Some(32768.0));

        let info = get_gpu_info_sync();
        assert_eq!(info.get("vendor").and_then(|v| v.as_str()), Some("AMD"));
        assert_eq!(info.get("vramMB").and_then(|v| v.as_str()), Some("32768 MiB"));
        // Driver version now flows through detection (preflight gate).
        assert_eq!(info.get("driverVersion").and_then(|v| v.as_str()), Some("32.0.11029.1008"));
        // dGPU + iGPU: both listed, dGPU first (selection unchanged).
        let all = wmi_all_gpus();
        assert_eq!(all.len(), 2);
        assert!(all[0].0.contains("R9700") && all[1].0.contains("Graphics"));

        let plan = build_install_plan(&info);
        assert_eq!(plan.get("profile").and_then(|v| v.as_str()), Some("AMD_GFX1201"));
        assert_eq!(plan.get("cuda").and_then(|v| v.as_str()), Some("ROCm 7.15 (TheRock)"));

        // Exact-pinned 7.15 primary (verified working), staging float fallback.
        let (primary, staging) = crate::install::amd_therock_torch_cmds("AMD_GFX1201", "AMD Radeon AI PRO R9700").unwrap();
        assert!(primary.contains("whl-multi-arch"), "got {primary}");
        assert!(primary.contains("torch==2.12.0+rocm7.15.0a20260728"), "got {primary}");
        assert!(primary.contains("amd-torch-device-gfx1201==2.12.0+rocm7.15.0a20260728"), "got {primary}");
        assert!(!primary.contains('['), "bracket-free for setup.py splice: {primary}");
        assert!(staging.contains("/v2-staging/gfx120X-all/"), "got {staging}");

        // registry-absent hardware (the 0.5.1 reporter box: PRO driver
        // exposes no parseable QWORD): the known-card table still resolves
        // 32768 — honest unknown only for truly unknown names.
        std::env::set_var("WGP_SIM_NO_REG", "1");
        let gpus2 = detect_gpus();
        let g2 = &gpus2.as_array().unwrap()[0];
        assert_eq!(g2.get("vendor").and_then(|v| v.as_str()), Some("AMD"));
        assert_eq!(g2.get("name").and_then(|v| v.as_str()), Some("AMD Radeon AI PRO R9700"));
        assert_eq!(g2.get("vramMB").and_then(|v| v.as_f64()), Some(32768.0));

        std::env::remove_var("WGP_SIM_NO_REG");
        std::env::remove_var("WGP_PROBE_NVIDIA_SMI");
        std::env::remove_var("WGP_PROBE_POWERSHELL");
        std::env::set_var("PATH", old_path);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod adapter_ram_tests {
    use super::{adapter_ram_known_mb, distinctive_tokens};
    #[test]
    fn cap_values_are_unknown() {
        // The R9700 report: 32GB card, AdapterRAM capped → was shown as 4GB.
        assert_eq!(adapter_ram_known_mb(0), None);
        assert_eq!(adapter_ram_known_mb(0xFFFF_FFFF), None);
        assert_eq!(adapter_ram_known_mb(4294967295), None);
        // Genuine small readings stay unknown for tiering honesty.
        assert_eq!(adapter_ram_known_mb(512 * 1024 * 1024), None);
        // Real sub-4GB readings pass through (3GB card, 2GB floor).
        assert_eq!(adapter_ram_known_mb(3221225472), Some(3072.0));
        assert_eq!(adapter_ram_known_mb(2147483648), Some(2048.0));
    }
    #[test]
    fn tokens_pick_model_numbers() {
        assert!(distinctive_tokens("AMD Radeon AI PRO R9700").contains(&"R9700".to_string()));
        assert!(distinctive_tokens("AMD Radeon RX 9070 XT").contains(&"9070".to_string()));
        // Generic iGPU names yield nothing (can never false-match).
        assert!(distinctive_tokens("AMD Radeon(TM) Graphics").is_empty());
    }
}
#[cfg(test)]
mod intel_cpu_tests {
    use super::{build_install_plan, hardware_profile_detail, kernel_profile_key};
    #[test]
    fn intel_stays_cpu_honest() {
        // Keys stable (setup.py never sees them; install/launch behavior unchanged).
        assert_eq!(kernel_profile_key("INTEL", "Intel UHD Graphics 770"), "INTEL_XPU");
        assert_eq!(kernel_profile_key("INTEL", "Intel Arc A770 Graphics"), "INTEL_XPU");
        for n in ["Intel UHD Graphics 770", "Intel Arc A770 Graphics"] {
            // Install plan: CPU payload, no XPU promise anywhere.
            let plan = build_install_plan(&serde_json::json!({"vendor":"INTEL","name":n,"vramMB":"128 MiB","driverVersion":""}));
            assert_eq!(plan["cuda"], serde_json::json!("CPU"));
            assert_eq!(plan["torch"], serde_json::json!("PyTorch (CPU)"));
            assert_eq!(plan["profile"], serde_json::json!("INTEL_XPU"));
            // Overview: INTEL_CPU row, kernel-free (old fallthrough promised CUDA wheels).
            let d = hardware_profile_detail("INTEL", n, 0.0);
            assert_eq!(d["profile"], serde_json::json!("INTEL_CPU"));
            assert_eq!(d["kernelsRaw"].as_array().unwrap().len(), 0);
            assert!(d["detail"]["torch"].as_str().unwrap().contains("CPU"));
            assert!(!d["packages"].as_array().unwrap().iter().any(|p| p.as_str().unwrap().contains("CUDA")));
        }
    }
}

//! Packages, memory profiles, auto-tune, Deepy, LLM engines, notifier, settings.
use crate::base::*;
use crate::{
    hw::{get_gpu_info_sync, kernel_profile_key},
    status::{get_active_env, resolve_env_python},
};
use std::path::PathBuf;
use tauri::Emitter;

#[tauri::command]
pub async fn check_package_updates(
    app: tauri::AppHandle,
    versions: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let _ = versions;
    let Some(py) = env_python_bin() else {
        return Ok(serde_json::json!([]));
    };
    use tauri_plugin_shell::process::CommandEvent;
    use tauri_plugin_shell::ShellExt;
    let (mut rx, _) = app
        .shell()
        .command(&py)
        .args(["-m", "pip", "list", "--outdated", "--format=json"])
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut out = String::new();
    while let Some(ev) = rx.recv().await {
        match ev {
            CommandEvent::Stdout(b) => out.push_str(&String::from_utf8_lossy(&b)),
            CommandEvent::Stderr(b) => out.push_str(&String::from_utf8_lossy(&b)),
            _ => {}
        }
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&out) {
        if let Some(arr) = v.as_array() {
            let dist_to_key = |d: &str| -> String {
                match d.to_lowercase().replace('-', "_").as_str() {
                    "triton_windows" => "triton".into(),
                    "opencv_python" => "opencv".into(),
                    "huggingface_hub" => "huggingface_hub".into(),
                    "spas_sage_attn" => "sageattention".into(),
                    "flash_attn" => "flash_attn".into(),
                    other => other.into(),
                }
            };
            let res: Vec<serde_json::Value> = arr.iter().map(|e| {
            let dist = e.get("name").and_then(|v| v.as_str()).unwrap_or("");
            serde_json::json!({"name": dist_to_key(dist), "dist": dist, "installed": e.get("version").cloned().unwrap_or(serde_json::Value::Null), "latest": e.get("latest_version").cloned().unwrap_or(serde_json::Value::Null)})
        }).collect();
            return Ok(serde_json::Value::Array(res));
        }
    }
    Ok(serde_json::json!([]))
}
/// Required version specifier for a distribution as pinned in the Wan2GP
/// repo's requirements.txt (`hf_xet>=1.5.2` -> `Some(">=1.5.2")`, bare
/// `tqdm` -> `Some("")`). Unlike `parse_requirement_pins` (exact `==` only)
/// this keeps any single PEP 440 operator so cards can verdict `>=` floors.
/// Skips comments, options, URLs, and marker-only mismatches; name match is
/// case-insensitive with `-`/`_` equivalent. Pure + unit-tested.
pub(crate) fn required_spec_in_requirements(text: &str, dist: &str) -> Option<String> {
    let want = dist.to_ascii_lowercase().replace('_', "-");
    for raw_line in text.lines() {
        let mut line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('-') {
            continue;
        }
        if let Some(i) = line.find('#') {
            line = line[..i].trim();
        }
        if let Some(i) = line.find(';') {
            line = line[..i].trim();
        }
        if line.is_empty() || line.contains("://") {
            continue;
        }
        let cut = line.find(|c: char| "<>=!~".contains(c) || c.is_whitespace());
        let (mut name, spec) = match cut {
            Some(i) => (line[..i].trim(), line[i..].trim()),
            None => (line, ""),
        };
        if let Some(b) = name.find('[') {
            name = name[..b].trim();
        }
        if name.to_ascii_lowercase().replace('_', "-") != want {
            continue;
        }
        if spec.is_empty() {
            return Some(String::new());
        }
        return Some(spec.split_whitespace().next().unwrap_or("").to_string());
    }
    None
}
#[tauri::command]
pub fn check_package(pkg: String) -> serde_json::Value {
    // Real probe: importlib version from the active env (aliases map import names to dist names).
    let dist = match pkg.as_str() {
        "triton" => "triton-windows",
        "spas_sage_attn" => "spas-sage-attn",
        "huggingface_hub" => "huggingface-hub",
        "opencv" | "opencv-python" => "opencv-python",
        other => other,
    };
    // Live upstream pin from the repo's requirements.txt (e.g. hf_xet>=1.5.2
    // -> ">=1.5.2") so cards can show installed-vs-required. Null when the
    // repo or pin is missing — the card degrades to installed-only.
    let required: serde_json::Value =
        std::fs::read_to_string(get_repo_dir().join("requirements.txt"))
            .ok()
            .and_then(|text| required_spec_in_requirements(&text, dist))
            .map(serde_json::Value::from)
            .unwrap_or(serde_json::Value::Null);
    let py = env_python_bin();
    if let Some(p) = py {
        if p.exists() {
            let code = format!("import importlib.metadata as m; print(m.version({dist:?}))");
            if let Ok(o) = silent_command(&p).args(["-c", &code]).output() {
                if o.status.success() {
                    let v = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if !v.is_empty() {
                        return serde_json::json!({"name": pkg, "installed": true, "version": v, "required": required});
                    }
                }
            }
        }
    }
    serde_json::json!({"name": pkg, "installed": false, "version": null, "required": required})
}
#[tauri::command]
pub fn memory_profile_read() -> serde_json::Value {
    // read wgp_config.json memory profile — ponytail: return current profile or default
    let p = get_repo_dir().join("wgp_config.json");
    if let Ok(s) = std::fs::read_to_string(&p) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
            // int8_kernels replaced the legacy enable_int8_kernels upstream
            // (v13.13 migration deletes the old key on launch). Prefer the
            // new key; map a lingering legacy value for display so
            // pre-update configs still read correctly.
            let int8 = v.get("int8_kernels").cloned().unwrap_or_else(|| {
                match v.get("enable_int8_kernels").and_then(|x| x.as_i64()) {
                    Some(0) => serde_json::json!("disabled"),
                    _ => serde_json::json!("auto"),
                }
            });
            return serde_json::json!({"ok": true, "settings": {
                "video_profile": v.get("video_profile").cloned().unwrap_or(serde_json::json!(4)),
                "image_profile": v.get("image_profile").cloned().unwrap_or(serde_json::json!(4)),
                "audio_profile": v.get("audio_profile").cloned().unwrap_or(serde_json::json!(4)),
                "vram_safety_coefficient": v.get("vram_safety_coefficient").cloned().unwrap_or(serde_json::json!(0.8)),
                "vae_config": v.get("vae_config").cloned().unwrap_or(serde_json::json!(0)),
                "transformer_quantization": v.get("transformer_quantization").cloned().unwrap_or(serde_json::json!("int8")),
                "int8_kernels": int8,
                "kernel_precision": v.get("kernel_precision").cloned().unwrap_or(serde_json::json!("fast"))
            }});
        }
    }
    serde_json::json!({"ok": true, "settings": {"video_profile": 4, "image_profile": 4, "audio_profile": 4, "vram_safety_coefficient": 0.8, "vae_config": 0, "transformer_quantization": "int8", "int8_kernels": "auto", "kernel_precision": "fast"}})
}
#[tauri::command]
pub fn auto_tune_detect() -> serde_json::Value {
    // real hardware detect — mirrors services/auto-tune.js detect() but sync via nvidia-smi
    let gpu = get_gpu_info_sync();
    let vendor = gpu
        .get("vendor")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_uppercase();
    let name = gpu
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let vram_str = gpu.get("vramMB").and_then(|v| v.as_str()).unwrap_or("0");
    let vram_mb: f64 = vram_str
        .split_whitespace()
        .next()
        .unwrap_or("0")
        .parse()
        .unwrap_or(0.0);
    let vram_gb = (vram_mb / 1024.0).round() as i64;
    let cuda_available = vendor == "NVIDIA" && vram_mb > 0.0 && !name.is_empty();
    // AMD (TheRock/ROCm): no CUDA, but a named Radeon is a usable GPU —
    // surface it instead of "—" so the installer plans the ROCm path.
    // (VRAM comes from the 64-bit registry probe in hw.rs; AdapterRAM cap
    // values read as unknown, never as a fake 4GB figure.)
    let gpu_available = cuda_available || (vendor == "AMD" && !name.is_empty());
    // RAM via powershell fallback
    let ram_gb = {
        #[cfg(windows)]
        {
            silent_command("powershell").args(["-NoProfile","-Command","[math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory/1GB,1)"]).output()
                .ok().and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().ok()).unwrap_or(32.0)
        }
        #[cfg(not(windows))]
        {
            32.0
        }
    };
    let cpu_count = std::thread::available_parallelism().map_or(8, std::num::NonZero::get) as i64;
    // AMD with known (64-bit registry) VRAM tiers like NVIDIA; unknown stays none.
    let vram_tier = if cuda_available || (vendor == "AMD" && vram_gb > 0) {
        if vram_gb >= 24 {
            "high"
        } else if vram_gb >= 12 {
            "low"
        } else {
            "tight"
        }
    } else {
        "none"
    };
    let ram_tier = if ram_gb >= 63.5 {
        "high"
    } else if ram_gb >= 31.5 {
        "low"
    } else {
        "very_low"
    };
    serde_json::json!({
        "cuda_available": cuda_available,
        "gpu_available": gpu_available,
        "gpu_name": if gpu_available { name.clone() } else { "—".into() },
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
pub fn auto_tune_recommend(
    hw: Option<serde_json::Value>,
    opts: Option<serde_json::Value>,
) -> serde_json::Value {
    let hw = hw.unwrap_or(serde_json::json!({"vram_tier":"low","ram_tier":"low","gpu_vram_gb":10}));
    let vram_tier = hw
        .get("vram_tier")
        .and_then(|v| v.as_str())
        .unwrap_or("low");
    let ram_tier = hw.get("ram_tier").and_then(|v| v.as_str()).unwrap_or("low");
    let vram_gb = hw
        .get("gpu_vram_gb")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(10.0);
    let failsafe = opts
        .as_ref()
        .and_then(|o| o.get("failsafe"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    // No CUDA at all → explicit conservative fallback, clearly labeled (never silent P5).
    if !failsafe && hw.get("cuda_available").and_then(|v| v.as_bool()) == Some(false) {
        return serde_json::json!({
            "video_profile": 4.5, "image_profile": 4.5, "audio_profile": 4.5,
            "vram_safety_coefficient": 0.70, "vae_config": 0, "transformer_quantization": "int8", "int8_kernels": "auto", "kernel_precision": "fast",
            "_recommendation_label": "Auto-tune unavailable on this hardware",
            "_recommendation_reason": "No CUDA-capable GPU detected. Conservative profile applied — generation may be limited.",
            "packages": ["torch","triton","sageattention"],
            "kernels": ["nunchaku","gguf"]
        });
    }
    // 7-profile matrix incl. fractional P3.5/P4.5 (mirrors auto-tune.js PROFILE_MATRIX).
    let (profile, coeff): (f64, f64) = if failsafe {
        (5.0, 0.6)
    } else {
        let p = match (vram_tier, ram_tier) {
            ("high", "high") => 1.0,
            ("high", "low") => 3.0,
            ("high", "very_low") => 3.5,
            ("low", "high") => 2.0,
            ("low", "low") => 4.0,
            ("low", "very_low") => 5.0,
            ("tight", "high") => 4.0,
            ("tight", "low") => 4.5,
            _ => 5.0,
        };
        let c = if vram_tier == "tight" || vram_tier == "none" {
            0.7
        } else {
            0.8
        };
        (p, c)
    };
    // Fast-LM-decoder rule: ≥12GB VRAM needs int-profile 1/3 for audio, else inherit.
    let audio = if vram_gb >= 12.0 && ![1.0, 3.0].contains(&profile) {
        3.0
    } else {
        profile
    };
    let label = if failsafe {
        "Failsafe · P5 (maximum compatibility)".to_string()
    } else {
        match profile.to_string().as_str() {
            "1" => "HighRAM · HighVRAM",
            "2" => "HighRAM · LowVRAM",
            "3" => "LowRAM · HighVRAM",
            "3.5" => "VeryLowRAM · HighVRAM",
            "4" => "LowRAM · LowVRAM",
            "4.5" => "LowRAM · LowVRAM+",
            _ => "VerylowRAM · LowVRAM",
        }
        .to_string()
    };
    serde_json::json!({
        "video_profile": profile, "image_profile": profile, "audio_profile": audio,
        "vram_safety_coefficient": coeff, "vae_config": 0, "transformer_quantization": "int8", "int8_kernels": "auto", "kernel_precision": "fast",
        "_recommendation_label": label,
        "_recommendation_reason": "Auto-tuned for your hardware",
        "packages": ["torch","triton","sageattention"],
        "kernels": ["nunchaku","gguf"]
    })
}

// ── Phase 2-5: remaining 65 handlers as thin stubs (real logic behind shell/fs plugins) ──
/// AMD package gate for the install path: CUDA / bitsandbytes / vanilla
/// PyPI triton / vanilla spas_sage_attn / PyPI sdist flash-attn break the
/// TheRock env, so they are refused on AMD profiles with a pointer to
/// docs/AMD-INSTALLATION.md. Vanilla `triton` maps to `triton-windows`.
/// Non-AMD profiles pass through untouched (pure + unit-testable core).
pub(crate) fn amd_package_gate_result(profile: &str, pkg: &str) -> Result<String, String> {
    const GUIDE: &str = "docs/AMD-INSTALLATION.md";
    if !profile.starts_with("AMD") {
        return Ok(pkg.to_string());
    }
    let low = pkg.to_lowercase();
    // Dist name without any version pin.
    let cut = low.find(|c| "<>=!~; [".contains(c));
    let name = match cut {
        Some(i) => low[..i].trim(),
        None => low.trim(),
    };
    // Vanilla PyPI triton → triton-windows (keep any version pin).
    if name == "triton" {
        let pin = pkg[name.len()..].to_string();
        return Ok(format!("triton-windows{pin}"));
    }
    let blocked = name == "bitsandbytes"
        || name == "spas-sage-attn"
        || name == "spas_sage_attn"
        || name == "sageattention"
        || name == "flash-attn"
        || name == "flash_attn"
        || name.contains("cuda")
        || name.contains("nvidia");
    if blocked {
        return Err(format!(
"refused on AMD (ROCm/TheRock env): '{pkg}' would break torch — use the {GUIDE} guide recipe instead"
));
    }
    Ok(pkg.to_string())
}
fn amd_package_gate(pkg: &str) -> Result<String, String> {
    let gpu = get_gpu_info_sync();
    let profile = kernel_profile_key(
        gpu.get("vendor").and_then(|v| v.as_str()).unwrap_or(""),
        gpu.get("name").and_then(|v| v.as_str()).unwrap_or(""),
    );
    amd_package_gate_result(&profile, pkg)
}
#[tauri::command]
pub async fn upgrade_package(
    app: tauri::AppHandle,
    pkg: String,
) -> Result<serde_json::Value, String> {
    pip_spec_ok(&pkg).map_err(|e| format!("blocked: {e}"))?;
    // AMD guard (same as install_package): refuse ROCm-breaking dists on
    // AMD profiles; vanilla `triton` maps to `triton-windows`.
    let pkg = amd_package_gate(&pkg)?;
    let Some(py) = env_python_bin() else {
        return Err("python not found".into());
    };
    let py_s = py.to_string_lossy().to_string();
    let emit = |m: &str| {
        let _ = app.emit("launch-log", m.to_string());
    };
    if !run_logged(
        &app,
        &py_s,
        &["-m", "pip", "install", "--upgrade", &pkg],
        None,
        emit,
    )
    .await
    {
        return Err(format!("pip upgrade {pkg} failed — see console output"));
    }
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command]
pub async fn install_package(
    app: tauri::AppHandle,
    pkg: String,
) -> Result<serde_json::Value, String> {
    pip_spec_ok(&pkg).map_err(|e| format!("blocked: {e}"))?;
    // AMD guard: CUDA / bitsandbytes / vanilla PyPI triton / vanilla
    // spas_sage_attn / PyPI sdist flash-attn break the TheRock env — refuse
    // with a pointer to the guide recipe. Vanilla `triton` maps to
    // `triton-windows`. NVIDIA/Intel/CPU paths are identical to before.
    let pkg = amd_package_gate(&pkg)?;
    let Some(py) = env_python_bin() else {
        return Err("python not found".into());
    };
    let py_s = py.to_string_lossy().to_string();
    let emit = |m: &str| {
        let _ = app.emit("launch-log", m.to_string());
    };
    if !run_logged(&app, &py_s, &["-m", "pip", "install", &pkg], None, emit).await {
        return Err(format!("pip install {pkg} failed — see console output"));
    }
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[cfg(test)]
mod required_spec_tests {
    use super::required_spec_in_requirements;
    #[test]
    fn finds_ge_floor_and_bare_and_exact() {
        let text = "# comment\nhf_xet>=1.5.2\ntqdm\nmmgp==3.8.0\n";
        assert_eq!(
            required_spec_in_requirements(text, "hf_xet").as_deref(),
            Some(">=1.5.2")
        );
        assert_eq!(
            required_spec_in_requirements(text, "tqdm").as_deref(),
            Some("")
        );
        assert_eq!(
            required_spec_in_requirements(text, "mmgp").as_deref(),
            Some("==3.8.0")
        );
    }
    #[test]
    fn name_match_ignores_case_underscore_and_extras() {
        let text = "HuggingFace_Hub[hf_xet]>=0.36.2\n";
        assert_eq!(
            required_spec_in_requirements(text, "huggingface-hub").as_deref(),
            Some(">=0.36.2")
        );
    }
    #[test]
    fn skips_urls_options_markers_and_missing() {
        let text = "-r other.txt\ninsightface @ https://example.com/x.whl\nrembg==2.0.65; python_version < \"3.11\"\n";
        assert_eq!(
            required_spec_in_requirements(text, "rembg").as_deref(),
            Some("==2.0.65")
        );
        assert_eq!(required_spec_in_requirements(text, "insightface"), None);
        assert_eq!(required_spec_in_requirements(text, "nope"), None);
    }
}
#[cfg(test)]
mod amd_package_gate_tests {
    use super::amd_package_gate_result;
    #[test]
    fn amd_refuses_cuda_bitsandbytes_vanilla_sage_flash() {
        // Vanilla `triton` maps to triton-windows instead of refusing.
        assert_eq!(
            amd_package_gate_result("AMD_GFX1201", "triton").unwrap(),
            "triton-windows"
        );
        assert_eq!(
            amd_package_gate_result("AMD_GFX1201", "triton==3.4.0").unwrap(),
            "triton-windows==3.4.0"
        );
        for bad in [
            "bitsandbytes",
            "spas_sage_attn",
            "spas-sage-attn",
            "sageattention",
            "flash-attn",
            "flash_attn",
            "nvidia-cuda-runtime-cu12",
        ] {
            let err = amd_package_gate_result("AMD_GFX1201", bad).unwrap_err();
            assert!(
                err.contains("docs/AMD-INSTALLATION.md"),
                "missing guide pointer: {err}"
            );
        }
        // NVIDIA behavior identical: everything passes through.
        assert_eq!(
            amd_package_gate_result("RTX_40", "triton").unwrap(),
            "triton"
        );
        assert_eq!(
            amd_package_gate_result("RTX_40", "bitsandbytes").unwrap(),
            "bitsandbytes"
        );
        assert_eq!(
            amd_package_gate_result("INTEL_XPU", "flash-attn").unwrap(),
            "flash-attn"
        );
    }
}
#[cfg(test)]
mod apprise_argv_tests {
    use super::{apprise_argv, notifier_normalize};
    #[test]
    fn prefers_console_script_falls_back_to_module() {
        // Binary beside the interpreter wins (the #35 fix: `-m` may lack __main__).
        let dir = std::env::temp_dir().join(format!(
            "wgp-apprise-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        #[cfg(windows)]
        let bin_name = "apprise.exe";
        #[cfg(not(windows))]
        let bin_name = "apprise";
        let py = dir.join("python.exe");
        std::fs::write(&py, b"").unwrap();
        // No binary yet → module fallback.
        let (cmd, args) = apprise_argv(&py, "T", "B", "discord://x");
        assert_eq!(cmd, py);
        assert_eq!(args[..2], vec!["-m".to_string(), "apprise".to_string()]);
        assert_eq!(args.last().unwrap(), "discord://x");
        // Binary appears → direct invocation, no `-m`.
        std::fs::write(dir.join(bin_name), b"").unwrap();
        let (cmd, args) = apprise_argv(&py, "T", "B", "discord://x");
        assert_eq!(cmd, dir.join(bin_name));
        assert_eq!(
            args,
            vec!["-t", "T", "-b", "B", "discord://x"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn native_managed_defaults_off_and_survives_normalize() {
        // Absent marker reads as legacy mode (old configs keep working).
        let off = notifier_normalize(&serde_json::json!({"enabled": true}));
        assert_eq!(off.get("nativeManaged").and_then(|v| v.as_bool()), Some(false));
        // Present marker survives a round-trip (native save owns it).
        let on = notifier_normalize(&serde_json::json!({"nativeManaged": true}));
        assert_eq!(on.get("nativeManaged").and_then(|v| v.as_bool()), Some(true));
    }
}
#[tauri::command]
pub async fn uninstall_package(
    app: tauri::AppHandle,
    pkg: String,
) -> Result<serde_json::Value, String> {
    pip_spec_ok(&pkg).map_err(|e| format!("blocked: {e}"))?;
    let Some(py) = env_python_bin() else {
        return Err("python not found".into());
    };
    let py_s = py.to_string_lossy().to_string();
    let emit = |m: &str| {
        let _ = app.emit("launch-log", m.to_string());
    };
    if !run_logged(
        &app,
        &py_s,
        &["-m", "pip", "uninstall", "-y", &pkg],
        None,
        emit,
    )
    .await
    {
        return Err(format!("pip uninstall {pkg} failed — see console output"));
    }
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command]
pub async fn restore_requirements(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let repo = get_repo_dir();
    let Some(py) = env_python_bin() else {
        return Err("python not found".into());
    };
    let py_s = py.to_string_lossy().to_string();
    let emit = |m: &str| {
        let _ = app.emit("launch-log", m.to_string());
    };
    if !run_logged(
        &app,
        &py_s,
        &["-m", "pip", "install", "-r", "requirements.txt"],
        Some(&repo),
        emit,
    )
    .await
    {
        return Err("requirements restore failed — see console output".into());
    }
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command]
pub fn llm_engines_list() -> serde_json::Value {
    // ponytail: probe cliOnPath + pipInstalled like Electron services/llm-engines.js
    let env = get_active_env();
    let py = env_python_bin();
    let check_cli = |cli: &str| -> bool {
        #[cfg(windows)]
        {
            silent_command("where")
                .arg(cli)
                .output()
                .is_ok_and(|o| o.status.success())
        }
        #[cfg(not(windows))]
        {
            silent_command("which")
                .arg(cli)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }
    };
    // Reuse the get_status version scan (same refresh already ran it — cache is warm)
    // instead of spawning a second cold venv python just for pip show.
    let cached_sdk = crate::base::LAST_STATUS
        .get()
        .and_then(|m| m.lock().ok())
        .and_then(|g| {
            g.as_ref()
                .and_then(|(_, _, v)| v.get("versions"))
                .and_then(|vs| vs.get("claude-agent-sdk"))
                .and_then(|x| x.as_str())
                .map(|s| !s.is_empty())
        })
        .unwrap_or(false);
    let check_pip = |pkg: &str| -> bool {
        if pkg == "claude-agent-sdk" && cached_sdk {
            return true;
        }
        if let Some(p) = &py {
            if p.exists() {
                return silent_command(p)
                    .args(["-m", "pip", "show", pkg])
                    .output()
                    .is_ok_and(|o| o.status.success());
            }
        }
        false
    };
    let engines = vec![
        serde_json::json!({"id":"claude-code","label":"Claude Code","desc":"Anthropic Claude Code CLI + Python bridge","cli":"claude","cliOnPath": check_cli("claude"), "pipPackage":"claude_agent_sdk","pipInstalled": check_pip("claude-agent-sdk"), "install":{"mode":"pip","spec":"claude-agent-sdk==0.1.66"}, "external": false, "auth":{"docsUrl":"https://code.claude.com/docs/en/authentication"}, "notes":"Pinned to 0.1.66 - upstream's exact bridge (summarized thinking); a newer SDK can clobber Wan2GP MCP deps."}),
        serde_json::json!({"id":"codex","label":"OpenAI Codex","desc":"OpenAI Codex CLI (npm)","cli":"codex","cliOnPath": check_cli("codex"), "pipPackage":null,"pipInstalled":null,"install":{"mode":"npm","spec":"@openai/codex","global":true}, "external": true, "authHint":"Sign in via a Deepy request in Wan2GP (secure link shown in chat)."}),
        serde_json::json!({"id":"opencode","label":"OpenCode","desc":"Universal-provider agent","cli":"opencode","cliOnPath": check_cli("opencode"), "pipPackage":null,"pipInstalled":null,"install":{"mode":"npm","spec":"opencode-ai","global":true}, "external": true, "serve":{"cmd":"opencode","args":["serve","--hostname","127.0.0.1","--port","4096"]}, "serverUrl":"http://127.0.0.1:4096", "serverRunning": std::net::TcpStream::connect("127.0.0.1:4096").is_ok(), "authHint":"In the OpenCode UI run /connect, pick a provider, follow auth. Wan2GP auto-starts opencode serve."}),
    ];
    serde_json::json!({"ok": true, "engines": engines, "hasActiveEnv": !env.is_null()})
}
// One-click engine installer: claude-code via env pip (pinned), codex/opencode via npm -g.
#[tauri::command]
pub fn llm_engine_install(engine: String) -> serde_json::Value {
    let spec = match engine.as_str() {
        "claude-code" => "claude-agent-sdk==0.1.66",
        "codex" => "@openai/codex",
        "opencode" => "opencode-ai",
        _ => return serde_json::json!({"ok": false, "success": false, "error": "Unknown engine"}),
    };
    let res = match engine.as_str() {
        "claude-code" => match env_python_bin() {
            None => {
                return serde_json::json!({"ok": false, "success": false, "error": "No active Python environment"})
            }
            Some(py) => silent_command(&py)
                .args(["-m", "pip", "install", spec])
                .output(),
        },
        _ => term_tool("npm").args(["install", "-g", spec]).output(),
    };
    match res {
        Ok(o) if o.status.success() => {
            serde_json::json!({"ok": true, "success": true, "spec": spec})
        }
        Ok(o) => {
            serde_json::json!({"ok": false, "success": false, "error": format!("install failed: {}", String::from_utf8_lossy(&o.stderr).trim())})
        }
        Err(e) => serde_json::json!({"ok": false, "success": false, "error": e.to_string()}),
    }
}
#[tauri::command]
pub fn llm_engine_uninstall(engine: String) -> serde_json::Value {
    let res = match engine.as_str() {
        "claude-code" => match env_python_bin() {
            None => {
                return serde_json::json!({"ok": false, "success": false, "error": "No active Python environment"})
            }
            Some(py) => silent_command(&py)
                .args(["-m", "pip", "uninstall", "-y", "claude-agent-sdk"])
                .output(),
        },
        "codex" => term_tool("npm")
            .args(["uninstall", "-g", "@openai/codex"])
            .output(),
        "opencode" => term_tool("npm")
            .args(["uninstall", "-g", "opencode-ai"])
            .output(),
        _ => return serde_json::json!({"ok": false, "success": false, "error": "Unknown engine"}),
    };
    match res {
        Ok(o) if o.status.success() => serde_json::json!({"ok": true, "success": true}),
        Ok(o) => {
            serde_json::json!({"ok": false, "success": false, "error": format!("remove failed: {}", String::from_utf8_lossy(&o.stderr).trim())})
        }
        Err(e) => serde_json::json!({"ok": false, "success": false, "error": e.to_string()}),
    }
}
// OpenCode server lifecycle (Wan2GP auto-starts it too; this is the manual toggle).
// PID-tracked so Start/Stop is honest; other engines have no servable process.
static OPENCODE_PID: std::sync::OnceLock<std::sync::Mutex<Option<u32>>> =
    std::sync::OnceLock::new();
// Kill the OpenCode server this launcher spawned (if any). Shared by the
// serve toggle and the app-close cleanup.
// Orphan fallback: OPENCODE_PID is memory-only and the child is
// mem::forget detached, so a restart loses the PID while the server keeps
// listening on :4096. After the PID kill (or when no PID was stored),
// sweep local port 4096 and kill ONLY node/opencode owners — never
// foreign processes. Stays SYNC; returns true if anything was killed.
/// Fast synchronous kill for app-close (see shutdown_cleanup): tracked PID
/// only, no port-scan PowerShell. Milliseconds; the :4096 orphan sweep
/// stays on the toggle/Stop path.
pub(crate) fn stop_opencode_fast() -> bool {
    let pid = OPENCODE_PID
        .get()
        .and_then(|m| m.lock().ok())
        .and_then(|mut g| g.take());
    if let Some(pid) = pid {
        #[cfg(windows)]
        {
            let _ = silent_command("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .output();
        }
        #[cfg(not(windows))]
        {
            let _ = silent_command("kill").arg(pid.to_string()).output();
        }
        return true;
    }
    false
}
pub(crate) fn stop_opencode_server() -> bool {
    let mut killed_any = false;
    let pid = OPENCODE_PID
        .get()
        .and_then(|m| m.lock().ok())
        .and_then(|mut g| g.take());
    if let Some(pid) = pid {
        // ponytail: /T kills the tree — the tracked PID is now the cmd /C wrapper, not the server itself
        #[cfg(windows)]
        {
            let _ = silent_command("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .output();
        }
        #[cfg(not(windows))]
        {
            let _ = silent_command("kill").arg(pid.to_string()).output();
        }
        killed_any = true;
    }
    // Port-sweep fallback for the :4096 orphan (restart lost the PID).
    // Windows: ONE Get-NetTCPConnection call, owner resolved in-PS.
    // Non-Windows: lsof -ti tcp:4096. Owner gate (node/opencode,
    // case-insensitive) is enforced in Rust before any kill.
    #[cfg(windows)]
    {
        let ps = "Get-NetTCPConnection -LocalPort 4096 -State Listen | ForEach-Object { $p = Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue; if ($p) { $p.Id.ToString() + '|' + $p.ProcessName } }";
        match silent_command("powershell")
            .args(["-NoProfile", "-Command", ps])
            .output()
        {
            Ok(o) if o.status.success() => {
                for line in String::from_utf8_lossy(&o.stdout).lines() {
                    let mut parts = line.splitn(2, '|');
                    if let (Some(pid_s), Some(name)) = (parts.next(), parts.next()) {
                        let owner = name.trim().to_lowercase();
                        if !(owner.contains("node") || owner.contains("opencode")) {
                            continue; // never kill foreign processes
                        }
                        if let Ok(pid) = pid_s.trim().parse::<u32>() {
                            if pid == 0 || pid == std::process::id() {
                                continue;
                            }
                            crate::base::push_log(
                                &format!(
                                    "[stop] opencode :4096: killing {name} PID {pid}\n",
                                    name = name.trim(),
                                ),
                                "launch",
                            );
                            let _ = silent_command("taskkill")
                                .args(["/F", "/T", "/PID", &pid.to_string()])
                                .output();
                            killed_any = true;
                        }
                    }
                }
            }
            // Exit 1 + "No matching" = nothing listening (the CLEAN case).
            Ok(o) => {
                let code = o.status.code().unwrap_or(-1);
                let err: String = String::from_utf8_lossy(&o.stderr)
                    .chars()
                    .take(200)
                    .collect();
                if !(code == 1 && err.contains("No matching")) {
                    crate::base::push_log(
                        &format!("[stop] opencode port scan failed (exit {code}): {err}\n"),
                        "launch",
                    );
                }
            }
            Err(e) => crate::base::push_log(
                &format!("[stop] opencode port scan spawn failed: {e}\n"),
                "launch",
            ),
        }
    }
    #[cfg(not(windows))]
    {
        // lsof lists PIDs; owner gate via ps so foreign listeners survive.
        let pids: Vec<u32> = silent_command("lsof")
            .args(["-ti", "tcp:4096"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .filter_map(|l| l.trim().parse::<u32>().ok())
                    .collect()
            })
            .unwrap_or_default();
        for pid in pids {
            if pid == 0 || pid == std::process::id() {
                continue;
            }
            let owner = silent_command("ps")
                .args(["-p", &pid.to_string(), "-o", "comm="])
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_lowercase())
                .unwrap_or_default();
            if !(owner.contains("node") || owner.contains("opencode")) {
                continue; // never kill foreign processes
            }
            crate::base::push_log(
                &format!("[stop] opencode :4096: killing {owner} PID {pid}\n"),
                "launch",
            );
            let _ = silent_command("kill").arg(pid.to_string()).output();
            killed_any = true;
        }
    }
    killed_any
}
#[tauri::command]
pub fn llm_engine_serve(engine: String, action: String) -> serde_json::Value {
    if engine != "opencode" {
        return serde_json::json!({"ok": false, "success": false, "error": "Only OpenCode has a local server"});
    }
    if action == "stop" {
        crate::features::stop_opencode_server();
        return serde_json::json!({"ok": true, "success": true, "running": false});
    }
    // start: already up (ours or Wan2GP's) → report running instead of double-spawning
    if std::net::TcpStream::connect("127.0.0.1:4096").is_ok() {
        return serde_json::json!({"ok": true, "success": true, "running": true, "url": "http://127.0.0.1:4096"});
    }
    match term_tool("opencode")
        .args(["serve", "--hostname", "127.0.0.1", "--port", "4096"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => {
            if let Ok(mut g) = OPENCODE_PID
                .get_or_init(|| std::sync::Mutex::new(None))
                .lock()
            {
                *g = Some(child.id());
            }
            std::mem::forget(child); // detach: lives beyond this command
            serde_json::json!({"ok": true, "success": true, "running": true, "url": "http://127.0.0.1:4096"})
        }
        Err(e) => {
            serde_json::json!({"ok": false, "success": false, "error": format!("Could not start opencode (is it installed?): {e}")})
        }
    }
}
#[tauri::command]
pub fn llm_engine_auth(engine: String) -> serde_json::Value {
    // Auth itself is interactive (browser OAuth / `auth login`) — the UI opens the
    // guide directly; this just hands out the canonical docs URL per engine.
    let docs = match engine.as_str() {
        "claude-code" => "https://code.claude.com/docs/en/authentication",
        "codex" => "https://developers.openai.com/codex/cli/",
        "opencode" => "https://opencode.ai/docs",
        _ => return serde_json::json!({"ok": false, "error": "Unknown engine"}),
    };
    serde_json::json!({"ok": true, "docsUrl": docs})
}
#[tauri::command]
pub fn deepy_status() -> serde_json::Value {
    let p = get_repo_dir().join("wgp_config.json");
    if !p.exists() {
        return serde_json::json!({"ok": true, "available": false, "reason": "wgp_config.json not found — install Wan2GP first."});
    }
    let Ok(s) = std::fs::read_to_string(&p) else {
        return serde_json::json!({"ok": false, "error": "cannot read wgp_config.json"});
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else {
        return serde_json::json!({"ok": false, "error": "wgp_config.json corrupted"});
    };
    let enabled = v
        .get("deepy_enabled")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    let dtype = v
        .get("deepy_type")
        .and_then(|x| x.as_str())
        .unwrap_or("zero");
    let mode = if enabled == 0 {
        "disabled"
    } else if dtype == "prime" {
        "prime"
    } else {
        "zero"
    };
    let enh = v
        .get("enhancer_enabled")
        .and_then(serde_json::Value::as_i64);
    let le = v.get("llm_engines");
    let cur_engine = le
        .and_then(|x| x.get("deepy"))
        .and_then(|x| x.as_str())
        .map(std::string::ToString::to_string)
        .unwrap_or_default();
    let prompt_enh = le
        .and_then(|x| x.get("prompt_enhancer"))
        .and_then(|x| x.as_str())
        .map(std::string::ToString::to_string);
    let engines: Vec<String> = le
        .and_then(|x| x.get("profiles"))
        .and_then(|x| x.as_object())
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    // Sessions section (upstream shared/deepy/config.py keys) — normalized
    // with upstream defaults so the launcher panel can pre-select.
    let session_mode =
        normalize_session_mode(v.get("deepy_multi_session").and_then(|x| x.as_str()));
    let reset_mode = v
        .get("deepy_session_reset_mode")
        .and_then(|x| x.as_str())
        .map(normalize_session_reset_mode)
        .unwrap_or_else(|| "new_session".into());
    let gallery_mode = v
        .get("deepy_session_gallery_media_mode")
        .and_then(|x| x.as_str())
        .map(normalize_session_gallery_mode)
        .unwrap_or_else(|| "link".into());
    // Qwen LLM quantization backend (upstream `prompt_enhancer_quantization`:
    // quanto_int8/gguf/gguf_q3/gguf_q2/gguf_ptq1) so the panel can pre-select.
    let quant = v
        .get("prompt_enhancer_quantization")
        .and_then(|x| x.as_str())
        .map(std::string::ToString::to_string);
    // Prompt-enhancement UI (upstream `enhancer_mode`: 0 = Automatic dropdown,
    // 1 = on-demand Enhance Prompt button) so the panel can pre-select.
    let enhancer_mode = v.get("enhancer_mode").and_then(serde_json::Value::as_i64);
    serde_json::json!({"ok": true, "available": true, "mode": mode, "deepyEnabled": enabled!=0, "deepyType": dtype, "currentEngine": if cur_engine.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(cur_engine) }, "promptEnhancer": prompt_enh, "enhancerEnabled": enh, "engines": engines, "promptEnhancerQuantization": quant, "sessionMode": session_mode, "sessionResetMode": reset_mode, "sessionGalleryMediaMode": gallery_mode, "enhancerMode": enhancer_mode})
}
/// Upstream `normalize_deepy_session_mode` (shared/deepy/config.py):
/// disabled/selectable/dedicated, anything else falls back to disabled.
fn normalize_session_mode(value: Option<&str>) -> String {
    match value.map(|s| s.trim().to_lowercase()).as_deref() {
        Some("disabled" | "selectable" | "dedicated") => value
            .map(|s| s.trim().to_lowercase())
            .unwrap_or_else(|| "disabled".into()),
        _ => "disabled".into(),
    }
}
/// Upstream `normalize_deepy_session_reset_mode`: reset_session or new_session.
fn normalize_session_reset_mode(value: &str) -> String {
    if value.trim().to_lowercase() == "reset_session" {
        "reset_session".into()
    } else {
        "new_session".into()
    }
}
/// Upstream `normalize_deepy_session_gallery_media_mode`: copy or link.
fn normalize_session_gallery_mode(value: &str) -> String {
    if value.trim().to_lowercase() == "copy" {
        "copy".into()
    } else {
        "link".into()
    }
}
/// Resolve a session pref: an explicitly provided valid value wins, then the
/// existing config value, then the launcher default. `None` (field absent)
/// always preserves existing config so an engine-only Apply never clobbers
/// the Sessions section.
fn resolve_session_pref(
    provided: Option<&str>,
    existing: Option<&str>,
    normalize: fn(&str) -> String,
    upstream_default: &str,
    launcher_default: Option<&str>,
) -> String {
    if let Some(p) = provided {
        let n = normalize(p);
        // A provided value that normalizes away from itself is invalid —
        // fall through to existing/default instead of writing garbage.
        if p.trim().to_lowercase() == n || (n == upstream_default && p.trim().is_empty()) {
            return n;
        }
    }
    if let Some(e) = existing {
        let n = normalize(e);
        if e.trim().to_lowercase() == n {
            return n;
        }
    }
    launcher_default.unwrap_or(upstream_default).into()
}
/// Stored Prime profile id (`llm_engines.deepy`: opencode / claude / codex /
/// qwen38_27b / qwen35_*) → `deepy_set` UI engine id (opencode / claude-code /
/// codex / local-qwen38). The auto-config enhancer-fix path reads the stored
/// profile but must call `deepy_set` with the UI id, or Prime starts fail
/// outright with "Prime requires an engine".
pub(crate) fn prime_profile_to_ui_id(profile: &str) -> &str {
    match profile.trim().to_lowercase().as_str() {
        "opencode" => "opencode",
        "claude" | "claude-code" => "claude-code",
        "codex" => "codex",
        s if s.contains("27b") || s.contains("qwen38") => "local-qwen38",
        // qwen35_* local profiles are Zero engines, not Prime ones — fall
        // back to the default remote engine rather than failing the start.
        _ => "opencode",
    }
}
/// Upstream Qwen LLM quantization backend (`prompt_enhancer_quantization`):
/// valid values per local engine (plugins/configuration/plugin.py
/// `prompt_enhancer_quantization_ui_state`): Qwen3.8 (id 5) takes the four
/// GGUF backends including Bonsai PTQ1 (`gguf_ptq1`, ~10GB VRAM, needs
/// GGUF kernels 1.0.23+); Qwen3.5 4B/9B (ids 3/4) take Quanto Int8 or
/// plain GGUF Q4. Engine-inappropriate values normalize to the engine
/// default (mirrors upstream); `None` preserves existing config.
/// Pure + unit-tested.
pub(crate) fn normalize_qwen_quant<'a>(quant: Option<&'a str>, enhancer: Option<i64>) -> Option<&'a str> {
    let q = quant.map(str::trim).filter(|s| !s.is_empty())?;
    match enhancer {
        Some(5) => match q {
            "gguf" | "gguf_q3" | "gguf_q2" | "gguf_ptq1" => Some(q),
            _ => Some("gguf"),
        },
        Some(3) | Some(4) => match q {
            "quanto_int8" | "gguf" => Some(q),
            _ => Some("quanto_int8"),
        },
        _ => None,
    }
}
#[tauri::command]
pub fn deepy_set(
    mode: String,
    engine: Option<String>,
    enhancer: Option<serde_json::Value>,
    sessions: Option<serde_json::Value>,
    quant: Option<String>,
    enhancer_mode: Option<serde_json::Value>,
) -> serde_json::Value {
    eprintln!("[deepy_set] mode={mode} engine={engine:?} enhancer={enhancer:?} quant={quant:?} enhancer_mode={enhancer_mode:?}");
    let m = mode.trim().to_lowercase();
    if !["disabled", "zero", "prime"].contains(&m.as_str()) {
        return serde_json::json!({"ok": false, "error": format!("Unknown Deepy mode: {}", mode)});
    }
    if m == "prime"
        && !engine
            .as_deref()
            .is_some_and(|s| ["opencode", "claude-code", "codex", "local-qwen38"].contains(&s))
    {
        return serde_json::json!({"ok": false, "error": "Prime requires an engine (OpenCode / Claude Code / Codex / local Qwen3.8 27B)."});
    }
    let p = get_repo_dir().join("wgp_config.json");
    if !p.exists() {
        return serde_json::json!({"ok": false, "error": "wgp_config.json not found — install Wan2GP first."});
    }
    let Ok(s) = std::fs::read_to_string(&p) else {
        return serde_json::json!({"ok": false, "error": "cannot read wgp_config.json"});
    };
    let mut v: serde_json::Value = match serde_json::from_str(&s) {
        Ok(x) => x,
        Err(e) => {
            return serde_json::json!({"ok": false, "error": format!("wgp_config.json corrupted: {}", e)})
        }
    };
    let bak = p.with_file_name("wgp_config.json.deepy-bak");
    let _ = std::fs::copy(&p, &bak);
    // F11: timestamped snapshot alongside the legacy single backup.
    let _ = snapshot_wgp_config();
    let (enabled, dtype) = match m.as_str() {
        "disabled" => (0, "zero"),
        "prime" => (1, "prime"),
        _ => (1, "zero"),
    };
    v["deepy_enabled"] = serde_json::json!(enabled);
    v["deepy_type"] = serde_json::json!(dtype);
    // Sessions section: explicit choice wins, otherwise keep the existing
    // config value, otherwise the launcher default (selectable workspace —
    // one shared outputs folder; upstream default is disabled).
    let sess = sessions.as_ref();
    let sess_str = |key: &str| sess.and_then(|s| s.get(key)).and_then(|x| x.as_str());
    let cfg_str = |key: &str| v.get(key).and_then(|x| x.as_str());
    let multi = resolve_session_pref(
        sess_str("multi_session"),
        cfg_str("deepy_multi_session"),
        |s| normalize_session_mode(Some(s)),
        "disabled",
        Some("selectable"),
    );
    let reset = resolve_session_pref(
        sess_str("reset_mode"),
        cfg_str("deepy_session_reset_mode"),
        normalize_session_reset_mode,
        "new_session",
        None,
    );
    let gallery = resolve_session_pref(
        sess_str("gallery_media_mode"),
        cfg_str("deepy_session_gallery_media_mode"),
        normalize_session_gallery_mode,
        "link",
        None,
    );
    // enhancer id — JS sends number (3) or null, handle both string/number.
    // Enforce valid mode↔id pairs like Electron's resolveEnhancerId: Zero only
    // runs on Qwen (3/4/5) — a Llama id (1/2) with Zero is the tokenizer-crash
    // combo, so fall back to the mode default instead of writing it. Disabled
    // runs the enhancer standalone, so any local model (1-5) is valid there.
    let raw_id: Option<i64> = enhancer.as_ref().and_then(|v| {
        if let Some(n) = v.as_i64() {
            Some(n)
        } else if let Some(s) = v.as_str() {
            s.parse::<i64>().ok()
        } else {
            None
        }
    });
    let enh_id: Option<i64> = match m.as_str() {
        "prime" => None,
        "zero" => Some(match raw_id {
            Some(3) | Some(4) | Some(5) => raw_id.unwrap(),
            _ => 3,
        }),
        _ => Some(match raw_id {
            Some(1) | Some(2) | Some(3) | Some(4) | Some(5) => raw_id.unwrap(),
            _ => 1,
        }),
    };
    if m != "prime" {
        if let Some(id) = enh_id {
            v["enhancer_enabled"] = serde_json::json!(id);
        }
    }
    // Qwen LLM quantization (upstream "Qwen LLM Quantization" dropdown):
    // applies only with a local Qwen engine — Disabled/Zero on 3/4/5, or
    // Prime on local Qwen3.8 (id 5). Engine-inappropriate values normalize to
    // the engine default; absent quant preserves existing config (e.g. the
    // auto-config fix path must not clobber a chosen Bonsai backend).
    let quant_enhancer: Option<i64> = match m.as_str() {
        "zero" | "disabled" => enh_id.filter(|id| [3, 4, 5].contains(id)),
        "prime" if engine.as_deref() == Some("local-qwen38") => Some(5),
        _ => None,
    };
    if let Some(q) = normalize_qwen_quant(quant.as_deref(), quant_enhancer) {
        v["prompt_enhancer_quantization"] = serde_json::json!(q);
        if q == "gguf_ptq1" {
            // Bonsai companion: INT8 KV cache halves cache VRAM — what makes
            // Prime viable at ~10GB (GGUF 1.0.23+ carries the kernels).
            // (Prompt enhancement mode is handled below from the panel choice.)
            v["deepy_kv_cache_quantization"] = serde_json::json!("int8");
        }
    }
    // Prompt enhancement UI (upstream `enhancer_mode`: 0 = Automatic dropdown
    // on every generation form, 1 = on-demand Enhance Prompt button).
    // Explicit 0/1 wins; otherwise the existing config value sticks; a missing
    // key defaults to 1 (button). Deepy tool templates carry no flags of their
    // own, so they follow it; per-model/template prompt_enhancer flags stay ""
    // as shipped. Applies in every mode — the enhancer (and its button) runs
    // standalone when Deepy is disabled.
    let raw_emode: Option<i64> = enhancer_mode.as_ref().and_then(|v| {
        if let Some(n) = v.as_i64() {
            Some(n)
        } else if let Some(s) = v.as_str() {
            s.parse::<i64>().ok()
        } else {
            None
        }
    });
    let emode = match raw_emode {
        Some(0) | Some(1) => raw_emode.unwrap(),
        _ => v.get("enhancer_mode").and_then(serde_json::Value::as_i64).filter(|n| *n == 0 || *n == 1).unwrap_or(1),
    };
    v["enhancer_mode"] = serde_json::json!(emode);
    // llm_engines deepy
    let eng_map = |id: &str| match id {
        "opencode" => "opencode",
        "claude-code" => "claude",
        "codex" => "codex",
        _ => "opencode",
    };
    let exe_map = |id: &str| match id {
        "opencode" => "opencode",
        "claude-code" => "claude",
        "codex" => "codex",
        _ => "opencode",
    };
    let enh_to_engine = |id: i64| match id {
        1 => "local_florence_llama32",
        2 => "local_florence_llamajoy",
        3 => "qwen35_4b",
        4 => "qwen35_9b",
        5 => "qwen38_27b",
        _ => "qwen35_4b",
    };
    if v.get("llm_engines").is_none() {
        v["llm_engines"] = serde_json::json!({});
    }
    if m == "prime" {
        let eid = engine.clone().unwrap_or_else(|| "opencode".into());
        if eid == "local-qwen38" {
            // Local Prime (b71026f): runs on Qwen3.8 VL 27B — upstream requires the
            // 27B model plus context >= 32000 and Summarize compaction, and the
            // Configuration UI auto-raises both, so mirror that here.
            v["enhancer_enabled"] = serde_json::json!(5);
            v["llm_engines"]["deepy"] = serde_json::json!("qwen38_27b");
            v["llm_engines"]["prompt_enhancer"] = serde_json::json!("same_as_deepy");
            if v.get("deepy_context_tokens")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0)
                < 32000
            {
                v["deepy_context_tokens"] = serde_json::json!(32000);
            }
            v["deepy_compaction_type"] = serde_json::json!("summarize");
            v["deepy_repetition_penalty"] = serde_json::json!(true);
        } else {
            let profile = eng_map(&eid);
            let exe = exe_map(&eid);
            v["llm_engines"]["deepy"] = serde_json::json!(profile);
            v["llm_engines"]["prompt_enhancer"] = serde_json::json!("same_as_deepy");
            if v["llm_engines"]["profiles"].is_null() {
                v["llm_engines"]["profiles"] = serde_json::json!({});
            }
            if v["llm_engines"]["profiles"][profile].is_null() {
                v["llm_engines"]["profiles"][profile] = serde_json::json!({});
            }
            v["llm_engines"]["profiles"][profile]["executable"] = serde_json::json!(exe);
            if profile == "opencode" {
                v["llm_engines"]["profiles"]["opencode"]["base_url"] =
                    serde_json::json!("http://127.0.0.1:4096");
            }
        }
        // Full Prime preset (mirrors shared/deepy/config.py defaults + guidance).
        for (k, val) in [
            ("deepy_prime_custom_system_prompt", serde_json::json!("When several models can satisfy the request, prefer the highest-quality base or full model unless the user explicitly prioritizes speed or names another model.")),
            ("deepy_prime_mcp_servers", serde_json::json!({})),
            ("deepy_mcp_auto_discover_paths", serde_json::json!(false)),
            ("deepy_allow_read_file_system", serde_json::json!(false)),
            ("deepy_file_system_paths", serde_json::json!([])),
            ("deepy_read_everywhere", serde_json::json!(false)),
            ("deepy_auto_cancel_queue_tasks", serde_json::json!(true)),
            ("deepy_separate_requests_with_empty_line", serde_json::json!(true)),
            // v12.72 sessions feature — resolved above (explicit choice >
            // existing config > launcher default) so pre-existing configs
            // can't skew-missing when wgp.py expects the keys.
            ("deepy_session_reset_mode", serde_json::json!(reset)),
            ("deepy_session_gallery_media_mode", serde_json::json!(gallery)),
            ("deepy_multi_session", serde_json::json!(multi)),
        ] { v[k] = val; }
    } else {
        let eid = enh_id.unwrap_or(1);
        let eng_str = enh_to_engine(eid);
        v["llm_engines"]["deepy"] = serde_json::json!(eng_str);
        v["llm_engines"]["prompt_enhancer"] = serde_json::json!("same_as_deepy");
        if m == "zero" {
            // Full Zero preset (mirrors shared/deepy/config.py defaults + tool picks).
            for (k, val) in [
                ("deepy_vram_mode", serde_json::json!("unload")),
                ("deepy_context_tokens", serde_json::json!(16386)),
                ("deepy_kv_cache_quantization", serde_json::json!("auto")),
                ("deepy_repetition_penalty", serde_json::json!(true)),
                ("deepy_compaction_type", serde_json::json!("discard")),
                (
                    "deepy_tool_gen_image",
                    serde_json::json!("Krea 2 Turbo (8 Steps)"),
                ),
                ("deepy_tool_edit_image", serde_json::json!("Flux Klein 9B")),
                (
                    "deepy_tool_gen_video",
                    serde_json::json!("LTX-2 2.5 Distilled"),
                ),
                (
                    "deepy_tool_gen_video_with_speech",
                    serde_json::json!("LTX-2.5 Distilled With Sound"),
                ),
                (
                    "deepy_tool_gen_song",
                    serde_json::json!("ACE-Step 1.5 Turbo LM 1.7B"),
                ),
                (
                    "deepy_tool_gen_speech_from_description",
                    serde_json::json!("Qwen3 1.7B"),
                ),
                (
                    "deepy_tool_gen_speech_from_sample",
                    serde_json::json!("Index TTS 2"),
                ),
                ("deepy_zero_custom_system_prompt", serde_json::json!("")),
                ("deepy_auto_cancel_queue_tasks", serde_json::json!(true)),
                (
                    "deepy_separate_requests_with_empty_line",
                    serde_json::json!(true),
                ),
                // v12.72 sessions feature — same resolution as Prime preset.
                ("deepy_session_reset_mode", serde_json::json!(reset)),
                (
                    "deepy_session_gallery_media_mode",
                    serde_json::json!(gallery),
                ),
                ("deepy_multi_session", serde_json::json!(multi)),
            ] {
                v[k] = val;
            }
        }
    }
    if atomic_write(&p, &serde_json::to_string_pretty(&v).unwrap_or_default()).is_err() {
        return serde_json::json!({"ok": false, "error": "failed to write wgp_config.json"});
    }
    let prime_label = match engine.as_deref().unwrap_or("opencode") {
        "local-qwen38" => "Qwen3.8 VL 27B (local)".into(),
        other => other.to_string(),
    };
    let msg = if m == "prime" {
        format!(
            "Deepy Prime set to {prime_label}. Launch Wan2GP and click \"Ask Deepy\"."
        )
    } else if m == "zero" {
        "Deepy Zero enabled (local model). Launch Wan2GP and click \"Ask Deepy\".".into()
    } else {
        "Deepy disabled. Prompt-enhancer settings saved — relaunch Wan2GP to apply.".into()
    };
    serde_json::json!({"ok": true, "mode": m, "enhancerId": enh_id, "backup": bak.to_string_lossy().to_string(), "message": msg})
}
#[tauri::command]
pub fn deepy_activate(engine: String) -> serde_json::Value {
    deepy_set("prime".into(), Some(engine), None, None, None, None)
}
// Auto-start via the per-user Run key (no admin needed). Returns success, like the UI checks.
#[tauri::command]
pub fn set_auto_start(enabled: bool) -> serde_json::Value {
    #[cfg(windows)]
    {
        let exe = std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
        let ok = if enabled && !exe.is_empty() {
            silent_command("reg")
                .args([
                    "add",
                    key,
                    "/v",
                    "Wan2GPDesktop",
                    "/t",
                    "REG_SZ",
                    "/d",
                    &format!("\"{exe}\""),
                    "/f",
                ])
                .output()
                .is_ok_and(|o| o.status.success())
        } else {
            silent_command("reg")
                .args(["delete", key, "/v", "Wan2GPDesktop", "/f"])
                .output()
                .is_ok_and(|o| o.status.success())
        };
        serde_json::json!({"ok": ok, "success": ok, "enabled": enabled})
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        return serde_json::json!({"ok": false, "success": false, "error": "auto-start is Windows-only"});
    }
}
#[tauri::command]
/// Fail-closed gate for the new upstream string enums (upstream raises
/// ValueError on unknown selections, so garbage must never reach
/// wgp_config.json). Other memory keys are validated by their own panels
/// or are numeric upstream choices — this gate covers only the string
/// enums it names. Pure + unit-tested.
pub(crate) fn valid_memory_override(key: &str, val: &serde_json::Value) -> bool {
    let s = val.as_str().unwrap_or("");
    match key {
        // shared/kernels/int8_backend.py CHOICES
        "int8_kernels" => ["auto", "disabled", "triton", "kitchen"].contains(&s),
        // shared/kernels/kernel_policy.py CHOICES ("strict"/"fast")
        "kernel_precision" => ["fast", "strict"].contains(&s),
        _ => true,
    }
}
#[tauri::command]
pub fn memory_profile_apply(settings: serde_json::Value) -> serde_json::Value {
    // mirrors Electron memory_profile:apply — writes to wgp_config.json and returns applied keys
    let p = get_repo_dir().join("wgp_config.json");
    let Ok(s) = std::fs::read_to_string(&p) else {
        return serde_json::json!({"ok": false, "success": false, "error": "wgp_config.json not found"});
    };
    let mut cfg: serde_json::Value = match serde_json::from_str(&s) {
        Ok(v) => v,
        Err(e) => {
            return serde_json::json!({"ok": false, "success": false, "error": format!("corrupted: {}", e)})
        }
    };
    let mut applied: Vec<String> = Vec::new();
    for key in [
        "video_profile",
        "image_profile",
        "audio_profile",
        "vram_safety_coefficient",
        "vae_config",
        "transformer_quantization",
        "int8_kernels",
        "kernel_precision",
    ] {
        if let Some(val) = settings.get(key) {
            if !valid_memory_override(key, val) {
                return serde_json::json!({"ok": false, "success": false, "error": format!("{key}: invalid value")});
            }
            cfg[key] = val.clone();
            applied.push(key.to_string());
        }
    }
    // Drop the deleted legacy key when the new one is written so upstream
    // never sees a stale enable_int8_kernels (its own migration deletes it
    // on next launch anyway; this just avoids the confusion sooner).
    if settings.get("int8_kernels").is_some() {
        if let Some(m) = cfg.as_object_mut() {
            m.remove("enable_int8_kernels");
        }
    }
    if applied.is_empty() {
        return serde_json::json!({"ok": true, "success": true, "applied": applied, "unchanged": true});
    }
    // F11: snapshot before overwriting so every Apply is restorable.
    let snapshot = snapshot_wgp_config();
    if atomic_write(&p, &serde_json::to_string_pretty(&cfg).unwrap_or_default()).is_err() {
        return serde_json::json!({"ok": false, "success": false, "error": "write failed"});
    }
    serde_json::json!({"ok": true, "success": true, "applied": applied, "snapshot": snapshot})
}
// ── Queue Notifier (Apprise) ──
// Config lives in desktop-config.json under "notifier". Events are classified
// from the launch-log stream (port of services/queue-notifier.js) and delivered
// via the env's apprise console script (binary first, `python -m` fallback).
// Spawns a thread per delivery so log streaming never blocks.
static NOTIF_LAST_PCT: std::sync::OnceLock<std::sync::Mutex<Option<u8>>> =
    std::sync::OnceLock::new();
static NOTIF_LAST_FIRE: std::sync::OnceLock<
    std::sync::Mutex<Option<(std::time::Instant, String)>>,
> = std::sync::OnceLock::new();
pub(crate) fn notifier_normalize(cfg: &serde_json::Value) -> serde_json::Value {
    let step = cfg
        .get("progressStep")
        .and_then(|v| v.as_u64())
        .unwrap_or(25)
        .clamp(1, 100);
    serde_json::json!({
        "enabled": cfg.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false),
        "url": cfg.get("url").and_then(|v| v.as_str()).unwrap_or("").trim(),
        "notifyOnComplete": cfg.get("notifyOnComplete").and_then(|v| v.as_bool()).unwrap_or(true),
        "notifyOnFail": cfg.get("notifyOnFail").and_then(|v| v.as_bool()).unwrap_or(true),
        "notifyOnProgress": cfg.get("notifyOnProgress").and_then(|v| v.as_bool()).unwrap_or(false),
        "progressStep": step,
        // Set by the native-notifications assistant once Wan2GP itself sends
        // events: the log-driven launcher sender stays off (no double pings).
        "nativeManaged": cfg.get("nativeManaged").and_then(|v| v.as_bool()).unwrap_or(false)
    })
}
fn notifier_saved() -> serde_json::Value {
    notifier_normalize(
        load_config_value()
            .get("notifier")
            .unwrap_or(&serde_json::Value::Null),
    )
}
fn env_python_bin() -> Option<PathBuf> {
    // Single resolution point for package ops: uv/venv (Scripts\ or bin/)
    // and conda (python at the env root) alike.
    let env = get_active_env();
    let raw = env.get("path")?.as_str()?;
    resolve_env_python(&get_repo_dir(), raw)
}
/// Build the apprise invocation for the active env.
/// Prefers the `pip install apprise` console script (Scripts\apprise.exe on
/// Windows, bin/apprise elsewhere — the only entry upstream's pinned
/// apprise==1.12.0 ships: `console_scripts apprise = apprise.cli:main`, no
/// __main__.py) and falls back to `python -m apprise` for distributions that
/// provide it. Issue #35: `-m` failed with
/// "No module named apprise.__main__" while `import apprise` succeeded, so
/// delivery must not assume `-m` works. Pure over an explicit path so it is
/// unit-testable.
pub(crate) fn apprise_argv(py: &PathBuf, title: &str, body: &str, url: &str) -> (PathBuf, Vec<String>) {
    #[cfg(windows)]
    let bin = py.parent().map(|d| d.join("apprise.exe"));
    #[cfg(not(windows))]
    let bin = py.parent().map(|d| d.join("apprise"));
    if let Some(b) = bin.filter(|b| b.is_file()) {
        (
            b,
            vec![
                "-t".to_string(),
                title.to_string(),
                "-b".to_string(),
                body.to_string(),
                url.to_string(),
            ],
        )
    } else {
        (
            py.clone(),
            vec![
                "-m".to_string(),
                "apprise".to_string(),
                "-t".to_string(),
                title.to_string(),
                "-b".to_string(),
                body.to_string(),
                url.to_string(),
            ],
        )
    }
}
fn apprise_send(url: &str, title: &str, body: &str) -> Result<(), String> {
    let py = env_python_bin().ok_or_else(|| "No active Python environment".to_string())?;
    let (cmd, args) = apprise_argv(&py, title, body, url);
    let out = silent_command(&cmd)
        .args(&args)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "apprise failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}
pub(crate) fn notifier_fire(kind: &str, text: &str) {
    let cfg = notifier_saved();
    // Native Wan2GP notifications active → stay silent (no double pings).
    if cfg
        .get("nativeManaged")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return;
    }
    if !cfg
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return;
    }
    let url = cfg
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if url.is_empty() {
        return;
    }
    let subscribed = match kind {
        "complete" => cfg
            .get("notifyOnComplete")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        "fail" => cfg
            .get("notifyOnFail")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        "progress" => cfg
            .get("notifyOnProgress")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        _ => false,
    };
    if !subscribed {
        return;
    }
    // 10s cooldown per kind (log bursts shouldn't spam phones)
    if let Some(m) = NOTIF_LAST_FIRE.get() {
        if let Ok(g) = m.lock() {
            if let Some((t, k)) = g.as_ref() {
                if k == kind && t.elapsed() < std::time::Duration::from_secs(10) {
                    return;
                }
            }
        }
    }
    if let Ok(mut g) = NOTIF_LAST_FIRE
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
    {
        *g = Some((std::time::Instant::now(), kind.to_string()));
    }
    let msg = match kind {
        "complete" => "Wan2GP: \u{2705} generation finished".to_string(),
        "fail" => format!(
            "Wan2GP: \u{274C} generation failed\n{}",
            text.chars().take(280).collect::<String>()
        ),
        _ => format!("Wan2GP: {text}"),
    };
    std::thread::spawn(move || {
        let _ = apprise_send(&url, "Wan2GP", &msg);
    });
}
const DONE_MARKERS: &[&str] = &[
    "task completed",
    "generation complete",
    "finished",
    "saved to",
    "output saved",
    "done in",
    "job finished",
    "completed successfully",
    "\u{2713}",
    "\u{2714}",
];
const FAIL_MARKERS: &[&str] = &[
    "traceback",
    "exception",
    "cuda out of memory",
    "out of memory",
    "assertionerror",
    "runtimeerror",
    "failed",
    "error",
];
const FAIL_SKIP: &[&str] = &[
    "0 error",
    "no error",
    "error_queue",
    "errors: 0",
    "0 errors",
];
// Classify one launch-log line; progress gated on progressStep multiples.
pub(crate) fn notifier_scan_line(line: &str) {
    // progress: last NN% in line (tqdm prints 50%|...)
    let mut pct: Option<u8> = None;
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let mut j = i;
            while j > 0 && bytes[j - 1].is_ascii_digit() {
                j -= 1;
            }
            if j < i {
                if let Ok(n) = line[j..i].parse::<u8>() {
                    if n <= 100 {
                        pct = Some(n);
                    }
                }
            }
        }
        i += 1;
    }
    if let Some(p) = pct {
        if p < 100 {
            let cfg = notifier_saved();
            if cfg
                .get("notifyOnProgress")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                let step = cfg
                    .get("progressStep")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(25)
                    .max(1);
                let last = NOTIF_LAST_PCT
                    .get()
                    .and_then(|m| m.lock().ok())
                    .and_then(|g| *g)
                    .unwrap_or(0);
                if p / (step as u8) > last / (step as u8) {
                    if let Ok(mut g) = NOTIF_LAST_PCT
                        .get_or_init(|| std::sync::Mutex::new(None))
                        .lock()
                    {
                        *g = Some(p);
                    }
                    notifier_fire("progress", &format!("{p}% done"));
                }
            }
            return;
        }
    }
    let low = line.to_lowercase();
    if FAIL_MARKERS.iter().any(|m| low.contains(m)) && !FAIL_SKIP.iter().any(|s| low.contains(s)) {
        if let Ok(mut g) = NOTIF_LAST_PCT
            .get_or_init(|| std::sync::Mutex::new(None))
            .lock()
        {
            *g = None;
        }
        notifier_fire("fail", line.trim());
        return;
    }
    if DONE_MARKERS.iter().any(|m| low.contains(m)) {
        if let Ok(mut g) = NOTIF_LAST_PCT
            .get_or_init(|| std::sync::Mutex::new(None))
            .lock()
        {
            *g = None;
        }
        notifier_fire("complete", line.trim());
    }
}
#[tauri::command]
pub fn notifier_config() -> serde_json::Value {
    serde_json::json!({"ok": true, "config": notifier_saved()})
}
#[tauri::command]
pub fn notifier_set(cfg: serde_json::Value) -> serde_json::Value {
    let mut clean = notifier_normalize(&cfg);
    // The nativeManaged marker is owned by the native assistant — a legacy
    // save must never clobber it (that would silently re-arm double pings).
    let stored_managed = notifier_saved()
        .get("nativeManaged")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if let Some(m) = clean.as_object_mut() {
        m.insert("nativeManaged".into(), serde_json::json!(stored_managed));
    }
    if clean
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        && clean
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .is_empty()
    {
        return serde_json::json!({"ok": false, "error": "A delivery URL is required when notifications are enabled (Apprise URL, e.g. discord://, tgram://)"});
    }
    // Refuse re-enabling the legacy sender while native events are on —
    // that would ping every destination twice. Turn the native events off
    // in the Notifications section above first.
    if clean
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        && stored_managed
    {
        return serde_json::json!({"ok": false, "error": "Native Wan2GP notifications are active — turn them off above before enabling the launcher sender, or you will get every notification twice."});
    }
    let mut full = load_config_value();
    if let Some(m) = full.as_object_mut() {
        m.insert("notifier".into(), clean.clone());
    }
    match crate::config::config_save(full) {
        Ok(_) => serde_json::json!({"ok": true, "config": clean}),
        Err(e) => serde_json::json!({"ok": false, "error": e}),
    }
}
#[tauri::command]
pub fn notifier_test(cfg: Option<serde_json::Value>) -> serde_json::Value {
    let use_cfg = cfg
        .map(|c| notifier_normalize(&c))
        .unwrap_or_else(notifier_saved);
    let url = use_cfg.get("url").and_then(|v| v.as_str()).unwrap_or("");
    if url.is_empty() {
        return serde_json::json!({"ok": false, "error": "No Apprise URL set"});
    }
    match apprise_send(
        url,
        "Wan2GP",
        "Wan2GP test notification \u{2705} — your queue notifier works.",
    ) {
        Ok(()) => serde_json::json!({"ok": true}),
        Err(e) => serde_json::json!({"ok": false, "error": e}),
    }
}
#[tauri::command]
pub fn set_theme_follow_system(enabled: bool) -> serde_json::Value {
    let mut full = load_config_value();
    if let Some(m) = full.as_object_mut() {
        m.insert("themeFollowSystem".into(), serde_json::json!(enabled));
    }
    match crate::config::config_save(full) {
        Ok(_) => serde_json::json!({"ok": true}),
        Err(e) => serde_json::json!({"ok": false, "error": e}),
    }
}
#[tauri::command]
pub fn set_notifications_enabled(enabled: bool) -> serde_json::Value {
    let _ = enabled;
    serde_json::json!({"ok": true})
}

#[cfg(test)]
mod deepy_roundtrip_tests {
    use super::*;
    fn read_cfg() -> serde_json::Value {
        serde_json::from_str(
            &std::fs::read_to_string(get_repo_dir().join("wgp_config.json")).unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn deepy_set_writes_coherent_config() {
        let p = get_repo_dir().join("wgp_config.json");
        if !p.exists() {
            return;
        } // no Wan2GP install on CI — nothing to verify
        let original = std::fs::read(&p).unwrap();
        // zero + Qwen 9B + GGUF quant
        let r = deepy_set(
            "zero".into(),
            None,
            Some(serde_json::json!(4)),
            Some(serde_json::json!({
                "multi_session": "dedicated",
                "reset_mode": "reset_session",
                "gallery_media_mode": "copy",
            })),
            Some("gguf".into()),
            None,
        );
        assert!(
            r.get("ok").and_then(|v| v.as_bool()).unwrap(),
            "zero apply failed: {r}"
        );
        let c = read_cfg();
        assert_eq!(c["deepy_enabled"], 1);
        assert_eq!(c["deepy_type"], "zero");
        assert_eq!(c["enhancer_enabled"], 4);
        assert_eq!(c["llm_engines"]["deepy"], "qwen35_9b");
        assert_eq!(c["deepy_vram_mode"], "unload");
        assert_eq!(c["deepy_context_tokens"], 16386);
        assert_eq!(c["deepy_tool_gen_image"], "Krea 2 Turbo (8 Steps)");
        // Deepy enable with no explicit choice defaults to the button (1).
        assert_eq!(c["enhancer_mode"], 1);
        // explicit session prefs stick
        assert_eq!(c["deepy_session_reset_mode"], "reset_session");
        assert_eq!(c["deepy_session_gallery_media_mode"], "copy");
        assert_eq!(c["deepy_multi_session"], "dedicated");
        // explicit quant sticks (engine-appropriate)
        assert_eq!(c["prompt_enhancer_quantization"], "gguf");
        // Bonsai PTQ1 on a Qwen3.5 engine normalizes to its default
        let r = deepy_set(
            "zero".into(),
            None,
            Some(serde_json::json!(4)),
            None,
            Some("gguf_ptq1".into()),
            None,
        );
        assert!(r.get("ok").and_then(|v| v.as_bool()).unwrap());
        assert_eq!(read_cfg()["prompt_enhancer_quantization"], "quanto_int8");
        // explicit Automatic (0) sticks; a later None apply preserves it
        let r = deepy_set(
            "zero".into(),
            None,
            Some(serde_json::json!(4)),
            None,
            None,
            Some(serde_json::json!(0)),
        );
        assert!(r.get("ok").and_then(|v| v.as_bool()).unwrap());
        assert_eq!(read_cfg()["enhancer_mode"], 0);
        let r = deepy_set("zero".into(), None, Some(serde_json::json!(4)), None, None, None);
        assert!(r.get("ok").and_then(|v| v.as_bool()).unwrap());
        assert_eq!(read_cfg()["enhancer_mode"], 0);
        // zero + Llama id must fall back to 3 (tokenizer-crash combo)
        let r = deepy_set("zero".into(), None, Some(serde_json::json!(1)), None, None, None);
        assert!(r.get("ok").and_then(|v| v.as_bool()).unwrap());
        let c = read_cfg();
        assert_eq!(c["enhancer_enabled"], 3);
        assert_eq!(c["llm_engines"]["deepy"], "qwen35_4b");
        // prime + codex
        let r = deepy_set("prime".into(), Some("codex".into()), None, None, None, None);
        assert!(r.get("ok").and_then(|v| v.as_bool()).unwrap());
        let c = read_cfg();
        assert_eq!(c["deepy_type"], "prime");
        assert_eq!(c["llm_engines"]["deepy"], "codex");
        assert_eq!(c["llm_engines"]["profiles"]["codex"]["executable"], "codex");
        assert!(c.get("deepy_prime_mcp_servers").is_some());
        // sessions=None preserves the existing session prefs
        assert_eq!(c["deepy_session_reset_mode"], "reset_session");
        assert_eq!(c["deepy_session_gallery_media_mode"], "copy");
        assert_eq!(c["deepy_multi_session"], "dedicated");
        // prime + local Qwen3.8 + Bonsai PTQ1 sticks
        let r = deepy_set(
            "prime".into(),
            Some("local-qwen38".into()),
            None,
            None,
            Some("gguf_ptq1".into()),
            Some(serde_json::json!(1)),
        );
        assert!(r.get("ok").and_then(|v| v.as_bool()).unwrap());
        let c = read_cfg();
        assert_eq!(c["enhancer_enabled"], 5);
        assert_eq!(c["llm_engines"]["deepy"], "qwen38_27b");
        assert_eq!(c["prompt_enhancer_quantization"], "gguf_ptq1");
        // Bonsai companions ride along: button kept + INT8 KV cache.
        assert_eq!(c["enhancer_mode"], 1);
        assert_eq!(c["deepy_kv_cache_quantization"], "int8");
        // disabled + Qwen: standalone enhancer — engine, quant and an
        // explicit Automatic choice all stick
        let r = deepy_set(
            "disabled".into(),
            None,
            Some(serde_json::json!(3)),
            None,
            Some("gguf".into()),
            Some(serde_json::json!(0)),
        );
        assert!(r.get("ok").and_then(|v| v.as_bool()).unwrap());
        let c = read_cfg();
        assert_eq!(c["deepy_enabled"], 0);
        assert_eq!(c["enhancer_enabled"], 3);
        assert_eq!(c["llm_engines"]["deepy"], "qwen35_4b");
        assert_eq!(c["prompt_enhancer_quantization"], "gguf");
        assert_eq!(c["enhancer_mode"], 0);
        // disabled + Florence (explicit button)
        let r = deepy_set(
            "disabled".into(),
            None,
            Some(serde_json::json!(2)),
            None,
            None,
            Some(serde_json::json!(1)),
        );
        assert!(r.get("ok").and_then(|v| v.as_bool()).unwrap());
        let c = read_cfg();
        assert_eq!(c["deepy_enabled"], 0);
        assert_eq!(c["enhancer_enabled"], 2);
        assert_eq!(c["llm_engines"]["deepy"], "local_florence_llamajoy");
        // disabled leaves the prompt-enhancement UI choice untouched
        assert_eq!(c["enhancer_mode"], 1);
        // restore byte-identical
        std::fs::write(&p, &original).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), original);
    }
}

#[cfg(test)]
mod autotune_matrix_tests {
    use super::*;
    fn rec(vram_tier: &str, ram_tier: &str, vram_gb: f64, failsafe: bool) -> serde_json::Value {
        auto_tune_recommend(
            Some(
                serde_json::json!({"vram_tier": vram_tier, "ram_tier": ram_tier, "gpu_vram_gb": vram_gb, "cuda_available": true}),
            ),
            Some(serde_json::json!({"failsafe": failsafe})),
        )
    }
    #[test]
    fn matrix_matches_reference() {
        // (vram, ram) -> (video, audio)
        let cases = [
            (("high", "high"), (1.0, 1.0)),
            (("high", "low"), (3.0, 3.0)),
            (("high", "very_low"), (3.5, 3.0)), // P3+ with audio fast-decoder rule
            (("low", "high"), (2.0, 3.0)),
            (("low", "low"), (4.0, 3.0)),
            (("low", "very_low"), (5.0, 3.0)),
            (("tight", "high"), (4.0, 4.0)),
            (("tight", "low"), (4.5, 4.5)),
            (("tight", "very_low"), (5.0, 5.0)),
        ];
        for ((vt, rt), (v, a)) in cases {
            let r = rec(vt, rt, if vt == "tight" { 8.0 } else { 16.0 }, false);
            assert_eq!(r["video_profile"].as_f64().unwrap(), v, "{vt}/{rt} video");
            assert_eq!(r["audio_profile"].as_f64().unwrap(), a, "{vt}/{rt} audio");
            assert_eq!(r["vae_config"], 0);
            assert_eq!(r["transformer_quantization"], "int8");
            // Upstream v13.13 kernel settings (legacy enable_int8_kernels is gone).
            assert_eq!(r["int8_kernels"], "auto");
            assert_eq!(r["kernel_precision"], "fast");
            assert!(r.get("enable_int8_kernels").is_none());
        }
        // failsafe forces P5 + 0.60
        let r = rec("high", "high", 48.0, true);
        assert_eq!(r["video_profile"], 5.0);
        assert_eq!(r["vram_safety_coefficient"], 0.60);
        // no CUDA → labeled fallback, not silent P4
        let r = auto_tune_recommend(
            Some(
                serde_json::json!({"vram_tier": "none", "ram_tier": "low", "gpu_vram_gb": 0, "cuda_available": false}),
            ),
            None,
        );
        assert_eq!(r["video_profile"], 4.5);
        assert!(r["_recommendation_label"]
            .as_str()
            .unwrap()
            .contains("unavailable"));
    }
}
#[cfg(test)]
mod kernel_setting_tests {
    use super::{normalize_qwen_quant, valid_memory_override};
    #[test]
    fn qwen_quant_normalizes_per_engine() {
        // Qwen3.8 (id 5): four GGUF backends incl. Bonsai PTQ1.
        for good in ["gguf", "gguf_q3", "gguf_q2", "gguf_ptq1"] {
            assert_eq!(normalize_qwen_quant(Some(good), Some(5)), Some(good));
        }
        // Wrong-engine values fall back to the engine default, never garbage.
        assert_eq!(normalize_qwen_quant(Some("quanto_int8"), Some(5)), Some("gguf"));
        assert_eq!(normalize_qwen_quant(Some("gguf_ptq1"), Some(4)), Some("quanto_int8"));
        assert_eq!(normalize_qwen_quant(Some("gguf"), Some(3)), Some("gguf"));
        // No engine / no quant preserves existing config.
        assert_eq!(normalize_qwen_quant(Some("gguf_ptq1"), None), None);
        assert_eq!(normalize_qwen_quant(None, Some(5)), None);
        assert_eq!(normalize_qwen_quant(Some(""), Some(5)), None);
    }
    #[test]
    fn int8_backends_match_upstream_choices() {
        // shared/kernels/int8_backend.py CHOICES.
        for good in ["auto", "disabled", "triton", "kitchen"] {
            assert!(
                valid_memory_override("int8_kernels", &serde_json::json!(good)),
                "{good}"
            );
        }
        for bad in ["", "enabled", "1", "pytorch", "AUTO"] {
            assert!(
                !valid_memory_override("int8_kernels", &serde_json::json!(bad)),
                "{bad}"
            );
        }
        // Legacy numeric key must not validate as the new enum.
        assert!(!valid_memory_override("int8_kernels", &serde_json::json!(1)));
    }
    #[test]
    fn kernel_precision_match_upstream_choices() {
        // shared/kernels/kernel_policy.py CHOICES ("strict"/"fast").
        assert!(valid_memory_override("kernel_precision", &serde_json::json!("fast")));
        assert!(valid_memory_override("kernel_precision", &serde_json::json!("strict")));
        assert!(!valid_memory_override("kernel_precision", &serde_json::json!("preserve")));
        assert!(!valid_memory_override("kernel_precision", &serde_json::json!("")));
    }
}

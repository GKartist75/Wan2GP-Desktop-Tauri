//! P0 Troubleshooting panel backend (upstream docs/TROUBLESHOOTING.md).
//! Six small read-only-first commands behind Manage → Troubleshooting:
//! failsafe apply, CUDA smoke test, port status/fix, debug bundle,
//! Triton import test + cache clear (with optional SDPA fallback).
use std::path::PathBuf;
use crate::base::*;
use crate::{hw::{get_gpu_info_sync, kernel_profile_key}, status::get_active_env};

/// Resolve the active env's interpreter (same shape as launch.rs).
fn active_python() -> Option<PathBuf> {
    let env = get_active_env();
    let raw = env.get("path")?.as_str()?;
    let repo = get_repo_dir();
    let base = if std::path::Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        repo.join(raw.trim_start_matches(".\\").trim_start_matches("./"))
    };
    let cand = if cfg!(windows) {
        base.join("Scripts\\python.exe")
    } else {
        base.join("bin/python3")
    };
    if cand.exists() {
        Some(cand)
    } else {
        None
    }
}

/// Quote-aware split (same as launch.rs) + merge helper: drop existing
/// `--flag [value]` occurrences, then append the canonical replacements.
fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    let mut started = false;
    for ch in s.chars() {
        match ch {
            '"' => {
                in_q = !in_q;
                started = true;
            }
            c if c.is_whitespace() && !in_q => {
                if started {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            c => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(cur);
    }
    out
}

/// Replace whole flags: `--attention X`, `--teacache X`, `--profile X` take a
/// value; `--fp16` is bare. Everything else passes through untouched.
fn merge_launch_flags(existing: &str, want: &[(&str, Option<&str>)]) -> String {
    let mut out: Vec<String> = Vec::new();
    let cur = split_args(existing);
    let mut i = 0;
    while i < cur.len() {
        let hit = want.iter().find(|(f, _)| *f == cur[i]);
        match hit {
            Some((_, takes_val)) => {
                i += 1;
                if takes_val.is_some() && i < cur.len() {
                    i += 1;
                }
            }
            None => {
                out.push(cur[i].clone());
                i += 1;
            }
        }
    }
    for (f, v) in want {
        out.push((*f).to_string());
        if let Some(val) = v {
            out.push((*val).to_string());
        }
    }
    out.join(" ")
}

/// Emergency failsafe (upstream "Absolute minimum setup"):
/// `wgp.py --attention sdpa --profile 4 --teacache 0 --fp16`
/// plus P5 profiles in wgp_config.json. Backs up wgp_config first.
#[tauri::command]
pub fn troubleshoot_failsafe_apply() -> Result<serde_json::Value, String> {
    let repo = get_repo_dir();
    if !repo.join("wgp.py").exists() {
        return Err("Wan2GP not installed — run Install first".into());
    }
    // 1) wgp_config.json → P5 profiles (backup first, same stamp style as reset_wgp_config).
    let cfg_path = repo.join("wgp_config.json");
    let mut backup: Option<String> = None;
    if cfg_path.exists() {
        let raw = std::fs::read_to_string(&cfg_path).map_err(|e| e.to_string())?;
        let mut v: serde_json::Value =
            serde_json::from_str(&raw).map_err(|_| "wgp_config.json is not valid JSON — use Reset wgp_config first".to_string())?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let bak = repo.join(format!("wgp_config.bak-{stamp}.json"));
        std::fs::copy(&cfg_path, &bak).map_err(|e| e.to_string())?;
        backup = Some(bak.to_string_lossy().to_string());
        if let Some(m) = v.as_object_mut() {
            for k in ["video_profile", "image_profile", "audio_profile"] {
                m.insert(k.to_string(), serde_json::json!(5));
            }
            m.insert("vram_safety_coefficient".to_string(), serde_json::json!(0.6));
        }
        let eol = if raw.contains("\r\n") { "\r\n" } else { "\n" };
        let s = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
        atomic_write(&cfg_path, &s.replace('\n', eol)).map_err(|e| e.to_string())?;
    }
    // 2) desktop-config launchArgs → canonical minimal flags (dedupe first).
    let mut dc = load_config_value();
    let prev = dc.get("launchArgs").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let merged = merge_launch_flags(
        &prev,
        &[
            ("--attention", Some("sdpa")),
            ("--profile", Some("4")),
            ("--teacache", Some("0")),
            ("--fp16", None),
        ],
    );
    dc["launchArgs"] = serde_json::json!(merged);
    let dp = get_config_file();
    atomic_write(
        &dp,
        &serde_json::to_string_pretty(&dc).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "ok": true, "success": true,
        "backup": backup,
        "launchArgs": merged,
        "profiles": {"video_profile": 5, "image_profile": 5, "audio_profile": 5, "vram_safety_coefficient": 0.6},
    }))
}

/// `python -c "import torch; …"` smoke test (upstream emergency fallback).
#[tauri::command]
pub fn troubleshoot_cuda_check() -> serde_json::Value {
    let Some(py) = active_python() else {
        return serde_json::json!({"ok": false, "error": "No Python environment installed — run Install first"});
    };
    let probe = "import torch, json; print(json.dumps({\"torch\": torch.__version__, \"cuda\": torch.version.cuda, \"available\": torch.cuda.is_available(), \"devices\": torch.cuda.device_count(), \"name\": (torch.cuda.get_device_name(0) if torch.cuda.is_available() and torch.cuda.device_count()>0 else None)}))";
    match silent_command(&py).args(["-c", probe]).output() {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            match serde_json::from_str::<serde_json::Value>(&s) {
                Ok(mut v) => {
                    v["ok"] = serde_json::json!(true);
                    v["python"] = serde_json::json!(py.to_string_lossy().to_string());
                    v
                }
                Err(_) => serde_json::json!({"ok": false, "error": "torch probe returned unparseable output", "raw": s}),
            }
        }
        Ok(o) => serde_json::json!({"ok": false, "error": "torch import failed (install incomplete?)", "stderr": String::from_utf8_lossy(&o.stderr).chars().take(500).collect::<String>()}),
        Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
    }
}

/// Deep GPU check: import-torch is not proof the stack runs (0.5.2 AMD
/// imported fine, died at the first int8 GEMM). Runs the compute probe —
/// both HSA modes on AMD (records the winner for launch, like install
/// does), single pass elsewhere. Can take a minute on cold HIP init.
#[tauri::command]
pub fn troubleshoot_gpu_compute() -> serde_json::Value {
    let Some(py) = active_python() else {
        return serde_json::json!({"ok": false, "error": "No Python environment installed — run Install first"});
    };
    let gpu = get_gpu_info_sync();
    let vendor = gpu.get("vendor").and_then(|v| v.as_str()).unwrap_or("");
    let name = gpu.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let profile = kernel_profile_key(vendor, name);
    if !profile.starts_with("AMD") {
        return match crate::amd::run_compute_probe(&py, None) {
            Ok(p) => verify_kernels(&py, p.torch, p.device, "native", false, None),
            Err(e) => serde_json::json!({"ok": false, "error": e}),
        };
    }
    let repo = get_repo_dir();
    let mut modes: Vec<(&str, Option<String>)> = Vec::new();
    if let Some(v) = crate::amd::profile_hsa_version(&repo, &profile) { modes.push(("override", Some(v))); }
    modes.push(("native", None));
    let mut detail = serde_json::Map::new();
    for (label, hsa) in &modes {
        match crate::amd::run_compute_probe(&py, hsa.as_deref()) {
            Ok(p) => {
                let choice = match hsa { Some(v) => crate::amd::HsaChoice::Override(v.clone()), None => crate::amd::HsaChoice::Native };
                crate::amd::write_hsa_choice(&repo, &choice);
                detail.insert(label.to_string(), serde_json::json!({"ok": true, "torch": p.torch, "device": p.device}));
                return verify_kernels(&py, p.torch, p.device, label, true, Some(serde_json::Value::Object(detail)));
            }
            Err(e) => { detail.insert(label.to_string(), serde_json::json!({"ok": false, "error": e})); }
        }
    }
    serde_json::json!({"ok": false, "error": "compute probe failed in both HSA modes — attach Copy diagnostics", "detail": detail})
}

/// Kernel import step shared by both Verify paths: compute passed, now
/// prove the wheels actually import (version-present ≠ loadable — AV
/// quarantine and wrong-torch ABIs break imports). Installed-but-broken
/// dists fail the check; missing dists stay neutral (presence is the
/// version scan's job).
fn verify_kernels(py: &std::path::Path, torch: String, device: String, mode: &str, recorded: bool, detail: Option<serde_json::Value>) -> serde_json::Value {
    let mut base = serde_json::json!({"torch": torch, "device": device, "mode": mode, "recorded": recorded});
    if let Some(d) = detail { base["detail"] = d; }
    match crate::amd::run_kernel_probe(py) {
        Err(e) => {
            base["ok"] = serde_json::json!(true);
            base["kernels"] = serde_json::Value::Null;
            base["kernel_warning"] = serde_json::json!(format!("kernel probe did not run ({e}) — compute passed"));
            base
        }
        Ok(map) => {
            let fails = crate::amd::kernel_probe_failures(&map);
            base["kernels"] = serde_json::Value::Object(map);
            if fails.is_empty() {
                base["ok"] = serde_json::json!(true);
            } else {
                base["ok"] = serde_json::json!(false);
                base["error"] = serde_json::json!(format!("GPU computes, but these wheels won't import: {}. Re-run Sync/Repair, check antivirus quarantine.", fails.join("; ")));
            }
            base
        }
    }
}

/// Is the configured server port already listening? If so, who owns it?
#[tauri::command]
pub fn troubleshoot_port_status() -> serde_json::Value {
    let port = load_config_value()
        .get("serverPort")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(7860);
    let listening = std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok();
    if !listening {
        return serde_json::json!({"ok": true, "port": port, "inUse": false});
    }
    // Owner lookup (Windows): Get-NetTCPConnection → OwningProcess → process name.
    let mut owner: serde_json::Value = serde_json::json!(null);
    #[cfg(windows)]
    {
        let ps = format!("Get-NetTCPConnection -LocalPort {port} -State Listen | Select-Object -First 1 -ExpandProperty OwningProcess");
        if let Ok(o) = silent_command("powershell").args(["-NoProfile", "-Command", &ps]).output() {
            if o.status.success() {
                let pid_s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if let Ok(pid) = pid_s.parse::<u32>() {
                    let ps2 = format!("(Get-Process -Id {pid} -ErrorAction SilentlyContinue).ProcessName");
                    let name = silent_command("powershell")
                        .args(["-NoProfile", "-Command", &ps2])
                        .output()
                        .ok()
                        .filter(|o| o.status.success())
                        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_default();
                    // Command line tells us if it's OUR wgp.py or something foreign.
                    let ps3 = format!("(Get-CimInstance Win32_Process -Filter \"ProcessId={pid}\" | Select-Object -ExpandProperty CommandLine)");
                    let cmd = silent_command("powershell")
                        .args(["-NoProfile", "-Command", &ps3])
                        .output()
                        .ok()
                        .filter(|o| o.status.success())
                        .map(|o| String::from_utf8_lossy(&o.stdout).trim().chars().take(300).collect::<String>())
                        .unwrap_or_default();
                    let ours = cmd.to_lowercase().contains("wgp.py");
                    owner = serde_json::json!({"pid": pid, "name": name, "cmd": cmd, "ours": ours});
                }
            }
        }
    }
    serde_json::json!({"ok": true, "port": port, "inUse": true, "owner": owner})
}

/// Fix a busy port: `kill` (our python owner only) or `bump` (serverPort+1 …+9 scan).
#[tauri::command]
pub fn troubleshoot_port_fix(action: Option<String>) -> Result<serde_json::Value, String> {
    let a = action.unwrap_or("bump".into());
    let mut dc = load_config_value();
    let port = dc.get("serverPort").and_then(serde_json::Value::as_u64).unwrap_or(7860);
    if a == "kill" {
        let st = troubleshoot_port_status();
        let pid = st.get("owner").and_then(|o| o.get("pid")).and_then(serde_json::Value::as_u64);
        let name = st.get("owner").and_then(|o| o.get("name")).and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
        let ours = st.get("owner").and_then(|o| o.get("ours")).and_then(serde_json::Value::as_bool).unwrap_or(false);
        let Some(pid) = pid else {
            return Err("Port owner unknown — use 'next free port' instead".into());
        };
        // Safety: only kill python processes (Gradio always runs on Python).
        // Our own wgp.py is ideal; foreign python gets killed too only with
        // explicit user click on this button (never blanket taskkill).
        if !(name.contains("python") || ours) {
            return Err(format!("Port {port} is owned by '{name}' (pid {pid}) — not a Python server. Change the port instead."));
        }
        #[cfg(windows)]
        let _ = silent_command("taskkill").args(["/pid", &pid.to_string(), "/f", "/t"]).output();
        #[cfg(not(windows))]
        let _ = silent_command("kill").arg("-9").arg(pid.to_string()).output();
        std::thread::sleep(std::time::Duration::from_millis(800));
        let free = std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_err();
        return Ok(serde_json::json!({"ok": true, "action": "kill", "port": port, "pid": pid, "freed": free}));
    }
    // bump: first free port in +1..=+9
    for p in (port + 1)..=(port + 9) {
        if std::net::TcpStream::connect(format!("127.0.0.1:{p}")).is_err() {
            dc["serverPort"] = serde_json::json!(p);
            let dp = get_config_file();
            atomic_write(&dp, &serde_json::to_string_pretty(&dc).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            return Ok(serde_json::json!({"ok": true, "action": "bump", "from": port, "port": p}));
        }
    }
    Err(format!("No free port in {}–{} — kill the owner instead", port + 1, port + 9))
}

/// Debug bundle markdown for Discord/GitHub (upstream "Before asking for help").
#[tauri::command]
pub fn troubleshoot_debug_bundle() -> serde_json::Value {
    let gpu = get_gpu_info_sync();
    let dc = load_config_value();
    let repo = get_repo_dir();
    let ver = env!("CARGO_PKG_VERSION");
    // torch/python via the same probe as cuda_check (cheap, cached nowhere — fine on click).
    let cuda = troubleshoot_cuda_check();
    let torch = cuda.get("torch").and_then(|v| v.as_str()).unwrap_or("?");
    let cuda_v = cuda.get("cuda").and_then(|v| v.as_str()).unwrap_or("?");
    let cuda_ok = cuda.get("available").and_then(serde_json::Value::as_bool).unwrap_or(false);
    let py_ver = silent_command(
        active_python()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or("python".into()),
    )
    .args(["--version"])
    .output()
    .ok()
    .map(|o| format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)).trim().to_string())
    .filter(|s| !s.is_empty())
    .unwrap_or("?".into());
    // wgp_config relevant keys (tokens never live there — safe to dump).
    let wgp: serde_json::Value = std::fs::read_to_string(repo.join("wgp_config.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!(null));
    let pick = |k: &str| wgp.get(k).cloned().unwrap_or(serde_json::json!(null));
    // Last error-ish log lines (ring buffer, no tqdm spam by construction).
    let tail: Vec<String> = LOG_HISTORY
        .get()
        .and_then(|m| m.lock().ok())
        .map(|g| {
            g.iter()
                .rev()
                .filter(|l| {
                    let t = l.to_lowercase();
                    t.contains("error") || t.contains("traceback") || t.contains("out of memory") || t.contains("keyerror") || t.contains("failed")
                })
                .take(15)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect()
        })
        .unwrap_or_default();
    // AMD runtime evidence (cheap importlib query — no torch import —
    // plus the probed HSA choice and live process env). Empty on NVIDIA.
    let amd_line = if gpu.get("vendor").and_then(|v| v.as_str()).unwrap_or("") == "AMD" {
        let vers = active_python()
            .and_then(|py| silent_command(&py).args(["-c", "import importlib.metadata as m\ndef v(p):\n try: return m.version(p)\n except Exception: return '?'\nprint(v('numpy') + '|' + v('optimum-quanto'))"]).output().ok())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or("?|?".into());
        let (numpy, quanto) = vers.split_once('|').unwrap_or(("?", "?"));
        let choice = match crate::amd::read_hsa_choice(&repo) {
            Some(crate::amd::HsaChoice::Native) => "native".to_string(),
            Some(crate::amd::HsaChoice::Override(v)) => format!("override {v}"),
            None => "unprobed".to_string(),
        };
        format!("\n**AMD** hsa_choice={choice} HSA_OVERRIDE={hsa} MIOPEN_FIND_MODE={mio} numpy={numpy} quanto={quanto}",
            hsa = std::env::var("HSA_OVERRIDE_GFX_VERSION").unwrap_or("(unset)".into()),
            mio = std::env::var("MIOPEN_FIND_MODE").unwrap_or("(unset)".into()))
    } else { String::new() };
    let md = format!(
        "**Launcher** v{ver} ({os}/{arch})\n**GPU** {gpu} ({vendor}, {vram} MB VRAM)\n**Python** {py}\n**Torch** {torch} + CUDA {cuda_v} (cuda_available={cuda_ok}){amd}\n**Launch** port={port} server={server} share={share} gpu={gpudev} args=`{args}`\n**Profiles** video={vp} image={ip} audio={ap} quant={q}\n**Log tail**\n```\n{tail}\n```",
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        gpu = gpu.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
        vendor = gpu.get("vendor").and_then(|v| v.as_str()).unwrap_or("?"),
        vram = gpu.get("vramMB").and_then(|v| v.as_str()).unwrap_or("?"),
        py = py_ver,
        port = dc.get("serverPort").and_then(serde_json::Value::as_u64).unwrap_or(7860),
        server = dc.get("serverName").and_then(|v| v.as_str()).unwrap_or("localhost"),
        share = dc.get("share").and_then(serde_json::Value::as_bool).unwrap_or(false),
        gpudev = dc.get("gpuDevice").and_then(|v| v.as_str()).unwrap_or("auto"),
        args = dc.get("launchArgs").and_then(|v| v.as_str()).unwrap_or(""),
        vp = pick("video_profile"),
        ip = pick("image_profile"),
        ap = pick("audio_profile"),
        q = pick("transformer_quantization"),
        amd = amd_line,
        tail = if tail.is_empty() { "(no errors in session log)".to_string() } else { tail.join("\n") },
    );
    serde_json::json!({"ok": true, "markdown": md})
}

/// Upstream Sage diagnostic step 1: can the env import Triton at all?
#[tauri::command]
pub fn troubleshoot_triton_test() -> serde_json::Value {
    let Some(py) = active_python() else {
        return serde_json::json!({"ok": false, "error": "No Python environment installed — run Install first"});
    };
    match silent_command(&py).args(["-c", "import triton; print(triton.__version__)"]).output() {
        Ok(o) if o.status.success() => serde_json::json!({"ok": true, "version": String::from_utf8_lossy(&o.stdout).trim().to_string()}),
        Ok(o) => serde_json::json!({"ok": false, "error": "triton import failed — attention modes needing Triton (sage/flash/compile/int8 kernels) won't work; fall back to SDPA", "stderr": String::from_utf8_lossy(&o.stderr).chars().take(500).collect::<String>()}),
        Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
    }
}

/// Upstream Sage diagnostic step 2: clear `%USERPROFILE%\.triton` (rename, not
/// delete — locked files can't block a rename-away). Optional SDPA fallback
/// merges `--attention sdpa` into launchArgs so the next boot works.
#[tauri::command]
pub fn troubleshoot_triton_clear(fallback_sdpa: Option<bool>) -> Result<serde_json::Value, String> {
    let cache = home_dir().join(".triton");
    let cleared;
    let mut backup: Option<String> = None;
    if cache.exists() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let bak = home_dir().join(format!(".triton.bak-{stamp}"));
        match std::fs::rename(&cache, &bak) {
            Ok(()) => {
                cleared = true;
                backup = Some(bak.to_string_lossy().to_string());
            }
            Err(_) => {
                // Rename failed (locked?) — best-effort remove instead.
                if std::fs::remove_dir_all(&cache).is_ok() {
                    cleared = true;
                } else {
                    return Err("Could not clear .triton cache (files locked by a running Python?) — stop Wan2GP and retry".into());
                }
            }
        }
    } else {
        cleared = true; // nothing there = nothing stale
    }
    let mut launch_args: Option<String> = None;
    if fallback_sdpa == Some(true) {
        let mut dc = load_config_value();
        let prev = dc.get("launchArgs").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let merged = merge_launch_flags(&prev, &[("--attention", Some("sdpa"))]);
        dc["launchArgs"] = serde_json::json!(merged.clone());
        let dp = get_config_file();
        atomic_write(&dp, &serde_json::to_string_pretty(&dc).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        launch_args = Some(merged);
    }
    Ok(serde_json::json!({"ok": true, "success": true, "cleared": cleared, "backup": backup, "launchArgs": launch_args}))
}

#[cfg(test)]
mod troubleshoot_tests {
    use super::*;
    #[test]
    fn merge_flags_dedupes() {
        let m = merge_launch_flags("--profile 3 --attention sage2 --verbose 2", &[("--attention", Some("sdpa")), ("--fp16", None)]);
        assert!(m.contains("--attention sdpa") && !m.contains("sage2") && m.contains("--verbose 2") && m.contains("--fp16"));
        let m2 = merge_launch_flags("", &[("--attention", Some("sdpa")), ("--profile", Some("4")), ("--teacache", Some("0")), ("--fp16", None)]);
        assert_eq!(m2, "--attention sdpa --profile 4 --teacache 0 --fp16");
    }
}

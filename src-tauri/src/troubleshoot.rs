//! P0 Troubleshooting panel backend (upstream docs/TROUBLESHOOTING.md).
//! Read-only-first commands behind Manage → Troubleshooting: failsafe apply,
//! CUDA smoke test, port status/fix, debug bundle, Triton import test + cache
//! clear (optional SDPA fallback), upstream's per-GPU known-good recipe, the
//! v17 OOM remedies, and the v17 Windows VRAM diagnostics.
use crate::base::*;
use crate::{
    hw::{get_gpu_info_sync, kernel_profile_key},
    status::get_active_env,
};
use std::path::PathBuf;

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

/// Replace whole flags: `--attention X`, `--profile X` take a value; `--fp16`
/// is bare. Everything else passes through untouched.
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
/// `wgp.py --attention sdpa --profile 4 --fp16`
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
        let mut v: serde_json::Value = serde_json::from_str(&raw).map_err(|_| {
            "wgp_config.json is not valid JSON — use Reset wgp_config first".to_string()
        })?;
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
            m.insert(
                "vram_safety_coefficient".to_string(),
                serde_json::json!(0.6),
            );
        }
        let eol = if raw.contains("\r\n") { "\r\n" } else { "\n" };
        let s = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
        atomic_write(&cfg_path, &s.replace('\n', eol)).map_err(|e| e.to_string())?;
    }
    // 2) desktop-config launchArgs → canonical minimal flags (dedupe first).
    let mut dc = load_config_value();
    let prev = dc
        .get("launchArgs")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let merged = merge_launch_flags(
        &prev,
        &[
            ("--attention", Some("sdpa")),
            ("--profile", Some("4")),
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
    let probe = r#"import json, torch
d = {"torch": torch.__version__, "cuda": torch.version.cuda, "available": torch.cuda.is_available(), "devices": torch.cuda.device_count(), "name": (torch.cuda.get_device_name(0) if torch.cuda.is_available() and torch.cuda.device_count() > 0 else None), "arch": torch.cuda.get_arch_list()}
try:
    d["cc"] = list(torch.cuda.get_device_capability(0)) if torch.cuda.is_available() and torch.cuda.device_count() > 0 else None
except Exception:
    d["cc"] = None
try:
    d["mps"] = bool(torch.backends.mps.is_available())
except Exception:
    d["mps"] = False
print(json.dumps(d))"#;
    match silent_command(&py).args(["-c", probe]).output() {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            match serde_json::from_str::<serde_json::Value>(&s) {
                Ok(mut v) => {
                    v["ok"] = serde_json::json!(true);
                    v["python"] = serde_json::json!(py.to_string_lossy().to_string());
                    v
                }
                Err(_) => {
                    serde_json::json!({"ok": false, "error": "torch probe returned unparseable output", "raw": s})
                }
            }
        }
        Ok(o) => {
            serde_json::json!({"ok": false, "error": "torch import failed (install incomplete?)", "stderr": String::from_utf8_lossy(&o.stderr).chars().take(500).collect::<String>()})
        }
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
            // Apple Silicon: MPS has no kernel wheels to import, and the
            // CUDA-only kernel probe would report every wheel as broken.
            Ok(p) if p.backend == "mps" => serde_json::json!({
                "ok": true,
                "torch": p.torch,
                "device": p.device,
                "mode": "mps",
                "recorded": false
            }),
            Ok(p) => {
                // Same arch gate as install: a wheel without SASS for this
                // card computes nothing (launcher #84 class).
                if let Some(cc) = p.cc {
                    if !p.arch.is_empty() && !crate::hw::arch_list_supports_cc(&p.arch, cc) {
                        return serde_json::json!({
                            "ok": false,
                            "torch": p.torch,
                            "device": p.device,
                            "mode": "native",
                            "error": format!(
                                "torch {} has no CUDA kernels for {}: arch list is [{}]. Every kernel launch fails with \"no kernel image is available\". Install a torch build that includes {}, then repair the environment.",
                                p.torch,
                                crate::hw::cc_sm_tag(cc),
                                p.arch.join(" "),
                                crate::hw::cc_sm_tag(cc)
                            )
                        });
                    }
                }
                verify_kernels(&py, p.torch, p.device, "native", false, None)
            }
            Err(e) => serde_json::json!({"ok": false, "error": e}),
        };
    }
    let repo = get_repo_dir();
    let mut modes: Vec<(&str, Option<String>)> = Vec::new();
    if let Some(v) = crate::amd::profile_hsa_version(&repo, &profile) {
        modes.push(("override", Some(v)));
    }
    modes.push(("native", None));
    let mut detail = serde_json::Map::new();
    for (label, hsa) in &modes {
        match crate::amd::run_compute_probe(&py, hsa.as_deref()) {
            Ok(p) => {
                let choice = match hsa {
                    Some(v) => crate::amd::HsaChoice::Override(v.clone()),
                    None => crate::amd::HsaChoice::Native,
                };
                crate::amd::write_hsa_choice(&repo, &choice);
                detail.insert(
                    label.to_string(),
                    serde_json::json!({"ok": true, "torch": p.torch, "device": p.device}),
                );
                return verify_kernels(
                    &py,
                    p.torch,
                    p.device,
                    label,
                    true,
                    Some(serde_json::Value::Object(detail)),
                );
            }
            Err(e) => {
                detail.insert(
                    label.to_string(),
                    serde_json::json!({"ok": false, "error": e}),
                );
            }
        }
    }
    serde_json::json!({"ok": false, "error": "compute probe failed in both HSA modes — attach Copy diagnostics", "detail": detail})
}

/// Kernel import step shared by both Verify paths: compute passed, now
/// prove the wheels actually import (version-present ≠ loadable — AV
/// quarantine and wrong-torch ABIs break imports). Installed-but-broken
/// dists fail the check; missing dists stay neutral (presence is the
/// version scan's job).
fn verify_kernels(
    py: &std::path::Path,
    torch: String,
    device: String,
    mode: &str,
    recorded: bool,
    detail: Option<serde_json::Value>,
) -> serde_json::Value {
    let mut base =
        serde_json::json!({"torch": torch, "device": device, "mode": mode, "recorded": recorded});
    if let Some(d) = detail {
        base["detail"] = d;
    }
    match crate::amd::run_kernel_probe(py) {
        Err(e) => {
            base["ok"] = serde_json::json!(true);
            base["kernels"] = serde_json::Value::Null;
            base["kernel_warning"] =
                serde_json::json!(format!("kernel probe did not run ({e}) — compute passed"));
            base
        }
        Ok(map) => {
            let mut fails = crate::amd::kernel_probe_failures(&map);
            // Sage3 is gated, never synced — a stray install must not fail
            // Verify; sage3_note() reports it at the right level instead.
            fails.retain(|f| !f.starts_with("sageattn3"));
            // Effective floor (setup_config may pin higher than the const) so a
            // future upstream bump raises the warning threshold too.
            let floor = std::fs::read_to_string(get_repo_dir().join("setup_config.json"))
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .map(|c| crate::hw::effective_gguf_floor(&c))
                .unwrap_or_else(|| crate::hw::GGUF_FLOOR.to_string());
            let stale = crate::amd::kernel_probe_stale(&map, &floor);
            let sage_note = crate::amd::sage3_note(&map, crate::amd::is_blackwell_gpu(&device));
            base["kernels"] = serde_json::Value::Object(map);
            if fails.is_empty() {
                base["ok"] = serde_json::json!(true);
            } else {
                base["ok"] = serde_json::json!(false);
                base["error"] = serde_json::json!(format!("GPU computes, but these wheels won't import: {}. Re-run Sync/Repair, check antivirus quarantine.", fails.join("; ")));
            }
            // Stale-but-importable wheels (GGUF < floor: #2274) warn even
            // when everything imports — slowness, not breakage.
            if !stale.is_empty() {
                base["kernel_warning"] = serde_json::json!(stale.join(" "));
            }
            if let Some((level, msg)) = sage_note {
                base["sage3_note"] = serde_json::json!({"level": level, "msg": msg});
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
    if let Some(o) = crate::platform::current().port_owner(port) {
        owner = serde_json::json!({"pid": o.pid, "name": o.name, "cmd": o.cmd, "ours": o.ours});
    }
    serde_json::json!({"ok": true, "port": port, "inUse": true, "owner": owner})
}

/// Fix a busy port: `kill` (our python owner only) or `bump` (serverPort+1 …+9 scan).
#[tauri::command]
pub fn troubleshoot_port_fix(action: Option<String>) -> Result<serde_json::Value, String> {
    let a = action.unwrap_or("bump".into());
    let mut dc = load_config_value();
    let port = dc
        .get("serverPort")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(7860);
    if a == "kill" {
        let st = troubleshoot_port_status();
        let pid = st
            .get("owner")
            .and_then(|o| o.get("pid"))
            .and_then(serde_json::Value::as_u64);
        let name = st
            .get("owner")
            .and_then(|o| o.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();
        let ours = st
            .get("owner")
            .and_then(|o| o.get("ours"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let Some(pid) = pid else {
            return Err("Port owner unknown — use 'next free port' instead".into());
        };
        // Safety: only kill python processes (Gradio always runs on Python).
        // Our own wgp.py is ideal; foreign python gets killed too only with
        // explicit user click on this button (never blanket taskkill).
        if !(name.contains("python") || ours) {
            return Err(format!("Port {port} is owned by '{name}' (pid {pid}) — not a Python server. Change the port instead."));
        }
        crate::platform::current().kill_pid(pid as u32);
        std::thread::sleep(std::time::Duration::from_millis(800));
        let free = std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_err();
        return Ok(
            serde_json::json!({"ok": true, "action": "kill", "port": port, "pid": pid, "freed": free}),
        );
    }
    // bump: first free port in +1..=+9
    for p in (port + 1)..=(port + 9) {
        if std::net::TcpStream::connect(format!("127.0.0.1:{p}")).is_err() {
            dc["serverPort"] = serde_json::json!(p);
            let dp = get_config_file();
            atomic_write(
                &dp,
                &serde_json::to_string_pretty(&dc).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            return Ok(serde_json::json!({"ok": true, "action": "bump", "from": port, "port": p}));
        }
    }
    Err(format!(
        "No free port in {}–{} — kill the owner instead",
        port + 1,
        port + 9
    ))
}

/// Windows long paths opt-in state (read-only). Used by the one-click fix.
#[tauri::command]
pub fn troubleshoot_long_paths_status() -> serde_json::Value {
    #[cfg(not(windows))]
    {
        serde_json::json!({"enabled": true, "note": "Windows only"})
    }
    #[cfg(windows)]
    {
        let out = silent_command("reg")
            .args([
                "query",
                "HKLM\\SYSTEM\\CurrentControlSet\\Control\\FileSystem",
                "/v",
                "LongPathsEnabled",
            ])
            .output();
        match out {
            Ok(o) if o.status.success() => {
                let text = String::from_utf8_lossy(&o.stdout);
                match crate::install::long_paths_state(&text) {
                    Some(enabled) => serde_json::json!({"enabled": enabled}),
                    None => serde_json::json!({"enabled": false}),
                }
            }
            _ => serde_json::json!({"enabled": false}),
        }
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn long_paths_currently_enabled() -> bool {
    #[cfg(not(windows))]
    {
        true
    }
    #[cfg(windows)]
    {
        silent_command("reg")
            .args([
                "query",
                "HKLM\\SYSTEM\\CurrentControlSet\\Control\\FileSystem",
                "/v",
                "LongPathsEnabled",
            ])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| crate::install::long_paths_state(&String::from_utf8_lossy(&o.stdout)))
            .unwrap_or(false)
    }
}

/// One-click Windows long paths fix. Only ever writes LongPathsEnabled=1.
#[tauri::command]
pub fn troubleshoot_long_paths_enable() -> Result<serde_json::Value, String> {
    #[cfg(not(windows))]
    {
        Err("Windows only".into())
    }
    #[cfg(windows)]
    {
        if long_paths_currently_enabled() {
            return Ok(serde_json::json!({"ok": true, "already": true}));
        }
        let _ = silent_command("reg")
            .args([
                "add",
                "HKLM\\SYSTEM\\CurrentControlSet\\Control\\FileSystem",
                "/v",
                "LongPathsEnabled",
                "/t",
                "REG_DWORD",
                "/d",
                "1",
                "/f",
            ])
            .output();
        if long_paths_currently_enabled() {
            return Ok(serde_json::json!({"ok": true, "elevated": false, "reboot": true}));
        }
        let _ = silent_command("powershell")
.args([
"-NoProfile",
"-Command",
"Start-Process reg -ArgumentList 'add HKLM\\SYSTEM\\CurrentControlSet\\Control\\FileSystem /v LongPathsEnabled /t REG_DWORD /d 1 /f' -Verb RunAs -Wait",
])
.output();
        if long_paths_currently_enabled() {
            return Ok(serde_json::json!({"ok": true, "elevated": true, "reboot": true}));
        }
        Err("Could not enable long paths (admin declined or blocked). Enable manually: Settings > System > For developers > Long paths, or run the reg add command as admin, then reboot.".into())
    }
}

/// Debug bundle markdown for Discord/GitHub (upstream "Before asking for help").
#[tauri::command(async)]
pub fn troubleshoot_debug_bundle() -> serde_json::Value {
    let gpu = get_gpu_info_sync();
    let dc = load_config_value();
    let repo = get_repo_dir();
    let ver = env!("CARGO_PKG_VERSION");
    // torch/python via the same probe as cuda_check (cheap, cached nowhere — fine on click).
    let cuda = troubleshoot_cuda_check();
    let torch = cuda.get("torch").and_then(|v| v.as_str()).unwrap_or("?");
    let cuda_v = cuda.get("cuda").and_then(|v| v.as_str()).unwrap_or("?");
    let cuda_ok = cuda
        .get("available")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    // macOS has no CUDA at all: reporting "CUDA ? / cuda_available=false" as
    // the only accelerator line reads like a broken install. Name the backend
    // that actually exists, and include the arch list on CUDA so a
    // "no kernel image" report can be judged from the bundle alone.
    let mps_ok = cuda
        .get("mps")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let arch_list = cuda
        .get("arch")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|e| e.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    let accel_line = if cuda_ok {
        format!(
            "CUDA {cuda_v} (cuda_available=true{})",
            if arch_list.is_empty() {
                String::new()
            } else {
                format!(", arch=[{arch_list}]")
            }
        )
    } else if mps_ok {
        "MPS (mps_available=true, cuda_available=false)".to_string()
    } else {
        format!("CPU only (cuda_available=false, mps_available={mps_ok})")
    };
    let py_ver = silent_command(
        active_python()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or("python".into()),
    )
    .args(["--version"])
    .output()
    .ok()
    .map(|o| {
        format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        )
        .trim()
        .to_string()
    })
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
        .lock()
        .map(|g| {
            g.iter()
                .rev()
                .filter(|l| {
                    let t = l.to_lowercase();
                    t.contains("error")
                        || t.contains("traceback")
                        || t.contains("out of memory")
                        || t.contains("keyerror")
                        || t.contains("failed")
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
    } else {
        String::new()
    };
    let v17_line = format!(
        "\n**v17 RAM/VRAM** mmgp={mmgp} allocator={alloc} head_split={hs} smart_pinning={pin} reserved_pct={res} preload v/i/a={pv}/{pi}/{pa}\n**Kernels** attention={att} int8={i8} precision={prec} compile={comp}",
        mmgp = crate::status::pyproject_version(
            &std::fs::read_to_string(repo.join("mmgp").join("pyproject.toml")).unwrap_or_default(),
        )
        .unwrap_or_else(|| "(not vendored)".into()),
        alloc = pick("vram_allocator"),
        hs = pick("attention_head_split"),
        pin = pick("smart_memory_pinning"),
        res = pick("perc_reserved_mem_max"),
        pv = pick("video_preload_mode"),
        pi = pick("image_preload_mode"),
        pa = pick("audio_preload_mode"),
        att = pick("attention_mode"),
        i8 = pick("int8_kernels"),
        prec = pick("kernel_precision"),
        comp = pick("compile"),
);
    // Which physical card the picker resolves to, and the exact flag WanGP
    // receives. This is the first thing to check on a multi-GPU OOM report.
    let gpu_pin_line = {
        let pick = crate::hw::saved_gpu_pick();
        let pin = crate::hw::resolve_gpu_pick(&pick, &crate::hw::nv_cards());
        format!(
            "\n**GPU pin** saved={pick} -> arg={arg} card={resolved} order={order} launcher CUDA_VISIBLE_DEVICES={visible}{note}",
            arg = pin.arg.as_deref().unwrap_or("(none: WanGP default)"),
            resolved = pin
                .card
                .as_ref()
                .map(|c| format!("{} ({} MiB, uuid {})", c.name, c.vram_mb, c.uuid))
                .unwrap_or_else(|| "none".into()),
            order = std::env::var("CUDA_DEVICE_ORDER").unwrap_or("(unset)".into()),
            visible = std::env::var("CUDA_VISIBLE_DEVICES").unwrap_or("(unset)".into()),
            note = pin.note.map(|n| format!(" ({n})")).unwrap_or_default(),
        )
    };
    // Identity of the code that actually runs: the checkout being reported on,
    // the vendored mmgp (pip cannot tell you), and the launcher UI's own GPU
    // mode — three questions every SeedVC/HEVC/codec report currently needs.
    let upstream_line = format!(
        "\n**Upstream** {} | mmgp {} | launcher UI {}",
        crate::install::short_rev(&repo, "HEAD"),
        crate::status::vendored_mmgp_label(&repo).unwrap_or_else(|| "(none vendored)".into()),
        crate::launcher_gpu_summary(),
    );
    let md = format!(
        "**Launcher** v{ver} ({os}/{arch}){upstream}\n**GPU** {gpu} ({vendor}, {vram} MB VRAM){gpu_pin_line}\n**Python** {py}\n**Torch** {torch} + {accel}{amd}\n**Launch** port={port} server={server} share={share} gpu={gpudev} verbose={verbose} args=`{args}`\n**Profiles** video={vp} image={ip} audio={ap} quant={q}{v17}\n**Log tail**\n```\n{tail}\n```",
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        upstream = upstream_line,
        accel = accel_line,
        gpu = gpu.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
        vendor = gpu.get("vendor").and_then(|v| v.as_str()).unwrap_or("?"),
        vram = gpu.get("vramMB").and_then(|v| v.as_str()).unwrap_or("?"),
        py = py_ver,
        port = dc.get("serverPort").and_then(serde_json::Value::as_u64).unwrap_or(7860),
        server = dc.get("serverName").and_then(|v| v.as_str()).unwrap_or("localhost"),
        share = dc.get("share").and_then(serde_json::Value::as_bool).unwrap_or(false),
        gpudev = dc.get("gpuDevice").and_then(|v| v.as_str()).unwrap_or("auto"),
        verbose = dc.get("verboseLogging").and_then(serde_json::Value::as_bool).unwrap_or(false),
        args = dc.get("launchArgs").and_then(|v| v.as_str()).unwrap_or(""),
        vp = pick("video_profile"),
        ip = pick("image_profile"),
        ap = pick("audio_profile"),
        q = pick("transformer_quantization"),
        v17 = v17_line,
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
    match silent_command(&py)
        .args(["-c", "import triton; print(triton.__version__)"])
        .output()
    {
        Ok(o) if o.status.success() => {
            serde_json::json!({"ok": true, "version": String::from_utf8_lossy(&o.stdout).trim().to_string()})
        }
        Ok(o) => {
            serde_json::json!({"ok": false, "error": "triton import failed — attention modes needing Triton (sage/flash/compile/int8 kernels) won't work; fall back to SDPA", "stderr": String::from_utf8_lossy(&o.stderr).chars().take(500).collect::<String>()})
        }
        Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
    }
}

/// Upstream Sage diagnostic step 2: clear the Triton JIT cache (rename, not
/// delete — locked files can't block a rename-away). Two homes are cleared:
/// the historical default `%USERPROFILE%\.triton` AND the launcher-pinned
/// `<data>\triton-cache` (`TRITON_CACHE_DIR`, whichever exists). Optional
/// SDPA fallback merges `--attention sdpa` into launchArgs so the next boot
/// works.
/// Rename-or-remove both Triton JIT cache homes (legacy `~\.triton` first,
/// then the launcher-pinned dir when present). Shared by Clear and Repair.
/// Returns the legacy backup path (the panel's known field). Errors when
/// files are locked — never partial success.
fn clear_triton_caches() -> Result<Option<String>, String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Rename-or-remove, shared by both cache homes. Ok(backup-path) on
    // rename (the cache can be restored manually), Ok(None) when removed
    // in place, Err when files are locked — never partial success.
    let clear_dir = |dir: &std::path::Path| -> std::result::Result<Option<String>, ()> {
        let name = dir
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("triton-cache");
        let bak = dir
            .parent()
            .unwrap_or(&home_dir())
            .join(format!("{name}.bak-{stamp}"));
        match std::fs::rename(dir, &bak) {
            Ok(()) => {
                let p = bak.to_string_lossy().to_string();
                Ok(Some(p))
            }
            Err(_) => {
                // Rename failed (locked by AV?) — best-effort remove instead.
                if std::fs::remove_dir_all(dir).is_ok() {
                    Ok(None)
                } else {
                    Err(())
                }
            }
        }
    };
    // Target list: legacy default first (reported as `backup`, the panel's
    // known field), then the pinned dir. Backups land beside each cache,
    // never inside it.
    let legacy = home_dir().join(".triton");
    let pinned = crate::launch::triton_cache_dir_for(&get_data_dir());
    let mut targets = vec![legacy.clone()];
    if pinned.exists() {
        targets.push(pinned);
    }
    let mut backup: Option<String> = None;
    for (i, dir) in targets.iter().enumerate() {
        if !dir.is_dir() {
            continue;
        }
        match clear_dir(dir) {
            Ok(b) => {
                if i == 0 {
                    backup = b;
                }
                crate::base::push_log(
                    &format!("[i] cleared Triton cache: {}\n", dir.to_string_lossy()),
                    "setup",
                );
            }
            Err(()) => {
                return Err(
                    "Could not clear the Triton cache (files locked by a running Python?) — stop Wan2GP and retry".into(),
                );
            }
        }
    }
    Ok(backup)
}

#[tauri::command]
pub fn troubleshoot_triton_clear(fallback_sdpa: Option<bool>) -> Result<serde_json::Value, String> {
    // `cleared` is always true: clear_triton_caches errors instead of
    // reporting partial success, and absent caches mean nothing stale.
    let backup = clear_triton_caches()?;
    let mut launch_args: Option<String> = None;
    if fallback_sdpa == Some(true) {
        let mut dc = load_config_value();
        let prev = dc
            .get("launchArgs")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let merged = merge_launch_flags(&prev, &[("--attention", Some("sdpa"))]);
        dc["launchArgs"] = serde_json::json!(merged.clone());
        let dp = get_config_file();
        atomic_write(
            &dp,
            &serde_json::to_string_pretty(&dc).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        launch_args = Some(merged);
    }
    Ok(
        serde_json::json!({"ok": true, "success": true, "cleared": true, "backup": backup, "launchArgs": launch_args}),
    )
}

/// Which installed Triton dist to repair, from `pip show` output: the
/// `triton-windows` build when present, else plain `triton`. (dist, version).
/// Floating to latest is deliberately NOT offered — a major drift would
/// break upstream parity. Pure + unit-tested.
pub(crate) fn parse_triton_pip_show(out: &str) -> Option<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    let mut name: Option<String> = None;
    for line in out.lines() {
        let t = line.trim();
        if let Some(n) = t.strip_prefix("Name:") {
            name = Some(n.trim().to_string());
        } else if let Some(v) = t.strip_prefix("Version:") {
            if let Some(n) = name.take() {
                found.push((n, v.trim().to_string()));
            }
        }
    }
    found
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("triton-windows"))
        .or_else(|| found.iter().find(|(n, _)| n.eq_ignore_ascii_case("triton")))
        .cloned()
}

/// Exact reinstall spec for the detected dist (`triton-windows==3.6.0.post26`).
/// `--force-reinstall --no-deps`: Triton's own files are replaced, torch and
/// everything else stay untouched. Also builds rollback specs
/// (`triton-windows==3.6.0.post24`). Pure + unit-tested.
pub(crate) fn triton_reinstall_spec(dist: &str, version: &str) -> String {
    format!("{dist}=={version}")
}

/// The exact argv the repair runs (minus the interpreter): pip show for
/// detection. Pinned so the console echo and the spawn can never diverge.
/// Pure + unit-tested.
pub(crate) fn triton_pip_show_argv() -> Vec<&'static str> {
    vec!["-m", "pip", "show", "triton-windows", "triton"]
}

/// The exact argv the repair runs (minus the interpreter): force-reinstall
/// without touching anything else. Pinned so flags can't silently change.
/// Pure + unit-tested.
pub(crate) fn triton_pip_reinstall_argv(spec: &str) -> Vec<String> {
    [
        "-m",
        "pip",
        "install",
        "--force-reinstall",
        "--no-deps",
        spec,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Render the full command line for the console echo
/// (`<python> -m pip install …`). Pure + unit-tested.
pub(crate) fn render_py_command(py: &str, argv: &[String]) -> String {
    format!("[>] {py} {}", argv.join(" "))
}

/// A repair version must be a single pip token — never empty, never
/// whitespace (argv, not shell, so this is hygiene, not injection defense;
/// pip itself rejects anything it can't resolve). Pure + unit-tested.
pub(crate) fn valid_repair_version(v: &str) -> bool {
    !v.is_empty() && v.split_whitespace().count() == 1
}

/// Repair a damaged Triton install: force-reinstall the EXACT installed
/// build (or the requested older one — the Roll back button), clear both
/// JIT cache homes, verify the import. For corrupted installs
/// (quarantined or half-written files) — it does not change antivirus
/// verdicts, which key on the (identical) rebuilt bytes. Rolling back does
/// not pin: the next setup run floats forward again on upstream's `-U`
/// range spec.
#[tauri::command]
pub async fn troubleshoot_triton_repair(
    app: tauri::AppHandle,
    version: Option<String>,
) -> Result<serde_json::Value, String> {
    // pip reinstall can run for minutes; never block the invoking thread.
    tauri::async_runtime::spawn_blocking(move || troubleshoot_triton_repair_blocking(app, version))
        .await
        .map_err(|e| e.to_string())?
}

fn troubleshoot_triton_repair_blocking(
    app: tauri::AppHandle,
    version: Option<String>,
) -> Result<serde_json::Value, String> {
    use tauri::Emitter;
    // push_log alone only reaches history — the live console needs the event
    // (that is why the first version of this echo never displayed).
    let echo = |m: &str| {
        crate::base::push_log(m, "setup");
        let _ = app.emit("launch-log", m.to_string());
    };
    let Some(py) = active_python() else {
        return Err("No Python environment installed — run Install first".into());
    };
    let py_s = py.to_string_lossy().to_string();
    let show_argv: Vec<String> = triton_pip_show_argv()
        .iter()
        .map(|s| s.to_string())
        .collect();
    echo(&format!("{}\n", render_py_command(&py_s, &show_argv)));
    let show = silent_command(&py_s)
        .args(&show_argv)
        .output()
        .map_err(|e| format!("pip show failed: {e}"))?;
    let show_text = String::from_utf8_lossy(&show.stdout).to_string();
    let Some((dist, detected)) = parse_triton_pip_show(&show_text) else {
        return Err(
            "No Triton distribution installed in the active env — run Install first".into(),
        );
    };
    // Explicit older build (Roll back button) or the detected one. Either
    // way an exact == pin — never floating.
    let want = version.unwrap_or_default();
    let want = if want.trim().is_empty() {
        detected.clone()
    } else {
        if !valid_repair_version(&want) {
            return Err(
                "Bad Triton version — single version token only (e.g. 3.6.0.post24)".into(),
            );
        }
        want.trim().to_string()
    };
    let spec = triton_reinstall_spec(&dist, &want);
    let inst_argv = triton_pip_reinstall_argv(&spec);
    echo(&format!("{}\n", render_py_command(&py_s, &inst_argv)));
    let inst = silent_command(&py_s)
        .args(&inst_argv)
        .output()
        .map_err(|e| format!("pip reinstall failed to start: {e}"))?;
    if !inst.status.success() {
        return Err(format!(
            "pip reinstall failed: {}",
            String::from_utf8_lossy(&inst.stderr)
                .chars()
                .take(500)
                .collect::<String>()
        ));
    }
    let backup = clear_triton_caches()?;
    let check_argv = vec![
        "-c".to_string(),
        "import triton; print(triton.__version__)".to_string(),
    ];
    echo(&format!("{}\n", render_py_command(&py_s, &check_argv)));
    let imp = silent_command(&py_s).args(&check_argv).output();
    let import_version = match imp {
        Ok(o) if o.status.success() => Some(String::from_utf8_lossy(&o.stdout).trim().to_string()),
        _ => None,
    };
    echo(&format!(
        "[✓] Triton repaired: {spec}, cache cleared, import {}\n",
        import_version.as_deref().unwrap_or("FAILED")
    ));
    Ok(serde_json::json!({
        "ok": true, "success": true,
        "dist": dist, "version": want, "previous": detected,
        "cleared": true, "backup": backup,
        "importVersion": import_version,
    }))
}

/// MSVC C++ toolchain status for the Triton JIT compiler pin (see
/// launch.rs `MsvcPin`). `{ok, supported, installed, cc?}` — the panel
/// shows the pin state on open and offers the installer when missing.
#[tauri::command]
pub fn troubleshoot_msvc_status() -> serde_json::Value {
    if cfg!(not(windows)) {
        return serde_json::json!({"ok": true, "supported": false});
    }
    match crate::launch::msvc_toolchain_cc() {
        Some(cc) => {
            serde_json::json!({"ok": true, "supported": true, "installed": true, "cc": cc})
        }
        None => serde_json::json!({"ok": true, "supported": true, "installed": false}),
    }
}

/// Official Microsoft VS 2022 Build Tools bootstrapper (evergreen link) and
/// the filename it is staged as under %TEMP%. Nothing compiler-shaped is
/// bundled with the launcher — MSVC cannot be redistributed that way and any
/// third-party compiler we shipped would be unsigned (the exact pattern the
/// pin exists to escape) — so the offer is always download-at-click-time
/// from Microsoft, correctly licensed and Microsoft-signed.
pub(crate) const VS_BUILDTOOLS_URL: &str = "https://aka.ms/vs/17/release/vs_BuildTools.exe";
pub(crate) const VS_BUILDTOOLS_EXE: &str = "wgp-vs_BuildTools.exe";

/// Bootstrapper args: the C++ build-tools workload plus recommended (which
/// pulls the Windows SDK our INCLUDE/LIB discovery requires). `--passive`
/// shows progress without prompts.
pub(crate) fn vs_buildtools_args() -> Vec<&'static str> {
    vec![
        "--passive",
        "--add",
        "Microsoft.VisualStudio.Workload.VCTools",
        "--includeRecommended",
    ]
}

/// PowerShell one-liner launching the staged bootstrapper elevated (UAC
/// prompt). Fire-and-forget BY DESIGN: a C++ workload install runs 10+
/// minutes and the installer has its own progress UI — awaiting it would
/// wedge the invoke. powershell exits 0 once the elevated child starts (a
/// UAC decline fails the command instead). Pure + unit-tested.
pub(crate) fn vs_buildtools_ps_launch(exe: &str) -> String {
    format!(
        "Start-Process '{exe}' -ArgumentList '{args}' -Verb RunAs",
        args = vs_buildtools_args().join(" ")
    )
}

/// Opt-in MSVC Build Tools install behind the Troubleshooting button.
/// Downloads Microsoft's official bootstrapper, stages it under %TEMP%,
/// and launches it elevated. Returns as soon as the installer starts; the
/// Microsoft installer runs its own UI, and the next app Launch
/// auto-detects cl.exe (or the panel Recheck does). A failed download errors
/// out before anything executes — never a partial state.
#[tauri::command]
pub async fn troubleshoot_msvc_install(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    use tauri::Emitter;
    if cfg!(not(windows)) {
        return Err("Windows only".into());
    }
    if let Some(cc) = crate::launch::msvc_toolchain_cc() {
        return Ok(serde_json::json!({"ok": true, "already": true, "cc": cc}));
    }
    let emit = |m: &str| {
        crate::base::push_log(m, "setup");
        let _ = app.emit("launch-log", m.to_string());
    };
    emit("[*] Downloading Microsoft VS 2022 Build Tools bootstrapper…\n");
    let bytes = reqwest::Client::new()
        .get(VS_BUILDTOOLS_URL)
        .header("User-Agent", "wan2gp-tauri")
        .send()
        .await
        .map_err(|e| format!("bootstrapper download failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("bootstrapper download failed: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("bootstrapper download failed: {e}"))?;
    // Captive-portal guard: the real bootstrapper is several MB; an HTML
    // login page is kilobytes. Never execute a stub download.
    if bytes.len() < 1_000_000 {
        return Err(format!(
            "bootstrapper download too small ({} bytes) — captive portal or blocked host? Use the manual winget command in the User Guide instead.",
            bytes.len()
        ));
    }
    let dest = std::env::temp_dir().join(VS_BUILDTOOLS_EXE);
    std::fs::write(&dest, &bytes).map_err(|e| format!("could not stage bootstrapper: {e}"))?;
    emit(&format!(
        "[*] Bootstrapper staged ({} MB) — launching the Microsoft installer elevated (UAC prompt)…\n",
        bytes.len() / 1_000_000
    ));
    // Blocking spawn is fine here: powershell returns once the elevated
    // child starts (or the user declines UAC) — the 10-minute install itself
    // runs detached in its own UI.
    let ps = vs_buildtools_ps_launch(&dest.to_string_lossy());
    let out = silent_command("powershell")
        .args(["-NoProfile", "-Command", &ps])
        .output()
        .map_err(|e| format!("could not start the installer: {e}"))?;
    if !out.status.success() {
        return Err("The installer did not start (admin declined the UAC prompt, or elevation is blocked). Re-run and accept, or install manually via the User Guide command.".into());
    }
    emit("[*] Microsoft installer running elevated — complete it (C++ workload, a few GB), then press Recheck. The next Launch auto-detects cl.exe.\n");
    Ok(serde_json::json!({"ok": true, "launched": true, "exe": dest.to_string_lossy()}))
}

/// Upstream's known-good recipe per GPU class (docs/TROUBLESHOOTING.md →
/// Performance Issues → GPU-Specific Optimizations).
///
/// Only returns keys verified to exist in a live v17 `wgp_config.json`.
/// Tea Cache and fp16 have NO config key upstream — they are per-generation
/// choices made inside WanGP — so they come back as a note rather than being
/// written as settings WanGP would silently ignore. None for AMD/Intel/CPU:
/// upstream ships no CLI recipe for those, and the launcher's own AMD path
/// owns them, so we fail closed instead of inventing one.
/// Pure + unit-tested.
pub(crate) fn known_good_recipe(profile_key: &str) -> Option<serde_json::Value> {
    // v18: upstream `compile` is a 3-state string (disabled / cudagraph /
    // transformer), not a boolean — a boolean matches neither engine and
    // silently engages nothing. "On" now means cudagraph (transformer is
    // deprecated upstream); the CLI --compile flag still forces transformer.
    let (profile, compile, note) = match profile_key {
        "GTX_10" => (
            4.0,
            "disabled",
            "Tea Cache 1.5, in WanGP's generation settings",
        ),
        "RTX_20" => (
            4.0,
            "disabled",
            "Needs SageAttention 1.0.6 (Sync GPU Wheels); Tea Cache 1.5 in WanGP",
        ),
        "RTX_30" | "RTX_40" => (
            3.0,
            "cudagraph",
            "Needs SageAttention 2.2.0 post6+ (Sync GPU Wheels) for the fastest CUDA-Graph kernels; Tea Cache 2.0 in WanGP",
        ),
        "RTX_50" => (
            4.0,
            "disabled",
            "Upstream pairs this with fp16 — set it per generation in WanGP",
        ),
        _ => return None,
    };
    // One definition of "which attention does this card want" — the same one
    // Auto-Tune recommends from, so the two can never disagree.
    let attention = crate::hw::attention_for_profile(profile_key)?;
    Some(serde_json::json!({
        "settings": {"attention_mode": attention, "video_profile": profile, "compile": compile},
        "note": note,
    }))
}

/// Which GPU class this machine is, plus upstream's recipe for it. Read-only:
/// applying the recipe is the dashboard's Apply Overrides, so the same
/// rec/saved contract applies and a value tuned elsewhere is never clobbered.
#[tauri::command]
pub fn troubleshoot_known_good() -> serde_json::Value {
    let gpu = get_gpu_info_sync();
    let key = kernel_profile_key(
        gpu.get("vendor").and_then(|v| v.as_str()).unwrap_or(""),
        gpu.get("name").and_then(|v| v.as_str()).unwrap_or(""),
    );
    match known_good_recipe(&key) {
        Some(r) => serde_json::json!({"ok": true, "profileKey": key, "recipe": r}),
        None => serde_json::json!({
            "ok": true, "profileKey": key, "recipe": serde_json::Value::Null,
            "reason": "Upstream publishes no known-good command line for this GPU class — keep what Performance Settings recommends."
        }),
    }
}

/// One-click remedies for the two memory failures upstream documents, mapped
/// onto config keys that actually exist. `head_split_medium` is upstream's own
/// numbers (H3 1920x1088 / 362 frames: about 2 GB less VRAM for up to about
/// 3% slower steps). `lower_reserved_ram` pairs a smaller pinning share with
/// Smart Memory Pinning, which upstream recommends for "your PC runs short of
/// RAM". P5 stays on the existing failsafe button. Pure + unit-tested.
pub(crate) fn oom_remedy_settings(action: &str) -> Option<serde_json::Value> {
    match action {
        "head_split_low" => Some(serde_json::json!({"attention_head_split": 1})),
        "head_split_medium" => Some(serde_json::json!({"attention_head_split": 2})),
        "head_split_high" => Some(serde_json::json!({"attention_head_split": 3})),
        "lower_reserved_ram" => Some(serde_json::json!({
            "perc_reserved_mem_max": 25,
            "smart_memory_pinning": true,
        })),
        _ => None,
    }
}

/// Apply an OOM remedy through the SAME gated path as Performance Settings, so
/// it snapshots, validates and can never write a value WanGP would ignore.
#[tauri::command]
pub fn troubleshoot_oom_remedy(action: Option<String>) -> serde_json::Value {
    let a = action.unwrap_or_default();
    // `--preload 0` is a launch flag, not a wgp_config value: upstream's
    // "CUDA out of memory during model loading" fix. Stage it into Extra
    // Launch Args through the same read-modify-atomic-write path the failsafe
    // uses, so it applies to the next launch and is visible in Manage → Launch
    // (and removable there).
    if a == "no_preload" {
        let mut dc = load_config_value();
        let prev = dc
            .get("launchArgs")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let merged = merge_launch_flags(&prev, &[("--preload", Some("0"))]);
        dc["launchArgs"] = serde_json::json!(merged);
        let dp = get_config_file();
        if let Err(e) = atomic_write(
            &dp,
            &serde_json::to_string_pretty(&dc).unwrap_or_else(|_| "{}".into()),
        ) {
            return serde_json::json!({"ok": false, "error": e.to_string()});
        }
        return serde_json::json!({"ok": true, "success": true, "remedy": a, "launchArgs": merged});
    }
    let Some(settings) = oom_remedy_settings(&a) else {
        return serde_json::json!({"ok": false, "error": format!("unknown remedy '{a}'")});
    };
    let r = crate::features::memory_profile_apply(settings);
    let mut out = r.clone();
    out["remedy"] = serde_json::Value::String(a);
    out
}

/// v17 Windows VRAM diagnostics, shipped in the checkout as
/// `scripts/gpumem.cmd` (per-process VRAM, sorted) and `scripts/gputrim.cmd`
/// (ask Windows to trim idle allocations, then re-report). `gputrim.cmd` shells
/// out to bare `python`, so the active env's Scripts directory goes first on
/// PATH — otherwise it resolves to some other interpreter, or none. Trim is
/// user-initiated on purpose: it briefly applies memory pressure and the
/// screen can flash.
#[tauri::command]
pub fn troubleshoot_vram_diag(action: Option<String>) -> serde_json::Value {
    let repo = get_repo_dir();
    let script = match action.as_deref().unwrap_or("list") {
        "list" => "gpumem.cmd",
        "trim" => "gputrim.cmd",
        other => {
            return serde_json::json!({"ok": false, "error": format!("unknown action '{other}'")})
        }
    };
    let path = repo.join("scripts").join(script);
    if !path.exists() {
        return serde_json::json!({
            "ok": false,
            "error": format!("{script} not found — update Wan2GP to a v17 or newer checkout"),
        });
    }
    let mut cmd = silent_command("cmd");
    cmd.arg("/C").arg(&path).current_dir(&repo);
    if let Some(py) = active_python() {
        if let Some(scripts) = py.parent() {
            let old = std::env::var("PATH").unwrap_or_default();
            cmd.env("PATH", format!("{};{}", scripts.display(), old));
        }
    }
    match cmd.output() {
        Ok(o) => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            serde_json::json!({
                "ok": o.status.success(),
                "script": script,
                "exit": o.status.code(),
                "output": text.trim(),
            })
        }
        Err(e) => serde_json::json!({"ok": false, "script": script, "error": e.to_string()}),
    }
}

// ── Upstream flag audit ───────────────────────────────────────────────
// `wgp.py --help` costs ~35 s here because argparse is reached after torch is
// imported, so the flag list is learned ONCE per upstream commit in the
// background and cached; launches from then on get an instant verdict, and the
// Diagnostics button can force a synchronous refresh. This is what catches the
// next upstream flag removal without shipping a launcher update: an unknown
// flag makes argparse exit before the UI appears.

static FLAG_PROBE_RUNNING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

const FLAG_CACHE_FILE: &str = "upstream-flags.json";

/// Long options in argparse's help text: lines that begin with an option
/// (`--gpu DEVICE`, `-h, --help`) contribute their long form; values and
/// wrapped description lines do not. Pure.
pub(crate) fn flags_from_help(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        if !t.starts_with('-') {
            continue;
        }
        // `-h, --help  show …` → the first `--` token on the line.
        let Some(name) = t
            .split(|c: char| c.is_whitespace() || c == ',')
            .find(|tok| tok.starts_with("--"))
        else {
            continue;
        };
        let name = name.split('=').next().unwrap_or(name);
        if name.len() > 2
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            out.push(name.to_string());
        }
    }
    out.sort();
    out.dedup();
    out
}

fn flag_cache_path() -> PathBuf {
    get_data_dir().join(FLAG_CACHE_FILE)
}

/// `(head, flags)` from the cache, `None` when absent/unreadable.
pub(crate) fn read_flag_cache() -> Option<(String, Vec<String>)> {
    let s = std::fs::read_to_string(flag_cache_path()).ok()?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    let head = v.get("head")?.as_str()?.to_string();
    let flags = v
        .get("flags")?
        .as_array()?
        .iter()
        .filter_map(|x| x.as_str().map(str::to_string))
        .collect::<Vec<_>>();
    (!flags.is_empty()).then_some((head, flags))
}

fn write_flag_cache(head: &str, flags: &[String]) {
    let path = flag_cache_path();
    let body = serde_json::json!({ "head": head, "flags": flags }).to_string();
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// Run `wgp.py --help` and parse the option list. `None` when the probe fails
/// or returns something that is clearly not argparse's help.
fn probe_help_flags(py: &std::path::Path, repo: &std::path::Path) -> Option<Vec<String>> {
    let out = silent_command(py.as_os_str())
        .args(["-s", "wgp.py", "--help"])
        .current_dir(repo)
        .output()
        .ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let flags = flags_from_help(&text);
    // A real WanGP help lists dozens of options; a short list means the
    // interpreter printed a traceback instead (bad env, broken checkout).
    (flags.len() >= 10).then_some(flags)
}

/// Resolve Python + HEAD and learn the flag set in a detached thread. Cheap to
/// call on every launch: it no-ops when a probe is already running.
pub(crate) fn learn_upstream_flags_async(repo: PathBuf) {
    use std::sync::atomic::Ordering;
    if FLAG_PROBE_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        let head = crate::install::short_rev(&repo, "HEAD");
        if head == "?" {
            FLAG_PROBE_RUNNING.store(false, Ordering::SeqCst);
            return;
        }
        if let Some(flags) = active_python().and_then(|py| probe_help_flags(&py, &repo)) {
            write_flag_cache(&head, &flags);
        }
        FLAG_PROBE_RUNNING.store(false, Ordering::SeqCst);
    });
}

/// Flags a launch is about to pass, in the order they appear, values dropped.
/// `--flag=value` keeps just `--flag`. Pure.
pub(crate) fn launch_flags(args: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for a in args {
        if !a.starts_with("--") {
            continue;
        }
        let f = a.split('=').next().unwrap_or(a).to_string();
        if !out.contains(&f) {
            out.push(f);
        }
    }
    out
}

/// Warning lines for launch args this checkout's argparse does not list.
/// Empty when everything is known or the flag set has not been learned yet
/// (a stale cache starts the background probe). Never blocks a launch.
pub fn audit_launch_args(repo: &std::path::Path, args: &[String]) -> Vec<String> {
    let want = launch_flags(args);
    if want.is_empty() {
        return Vec::new();
    }
    let head = crate::install::short_rev(repo, "HEAD");
    let Some((cached_head, flags)) = read_flag_cache() else {
        learn_upstream_flags_async(repo.to_path_buf());
        return Vec::new();
    };
    if cached_head != head {
        learn_upstream_flags_async(repo.to_path_buf());
        return Vec::new();
    }
    let unknown: Vec<&String> = want
        .iter()
        // argparse accepts unambiguous prefixes (`--prof`), so a prefix of a
        // real flag is not a false claim about this version.
        .filter(|f| !flags.contains(f) && !flags.iter().any(|known| known.starts_with(f.as_str())))
        .collect();
    if unknown.is_empty() {
        Vec::new()
    } else {
        vec![format!(
            "[!] Launch arg(s) this Wan2GP version ({head}) does not list: {} — upstream renamed or removed them, so Wan2GP may refuse to start. Check Manage → Launch → Extra Launch Args.\n",
            unknown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ")
        )]
    }
}

/// Diagnostics: force/read the flag audit for the saved Extra Launch Args.
#[tauri::command]
pub fn troubleshoot_flag_audit(force: Option<bool>) -> serde_json::Value {
    let repo = get_repo_dir();
    let head = crate::install::short_rev(&repo, "HEAD");
    if head == "?" {
        return serde_json::json!({"ok": false, "error": "not a git checkout — cannot tell which WanGP version this is"});
    }
    let mut cached = read_flag_cache();
    let mut fresh = cached.as_ref().is_some_and(|(h, _)| h == &head);
    if force == Some(true) || !fresh {
        let Some(py) = active_python() else {
            return serde_json::json!({"ok": false, "error": "no Python environment installed — run Install first"});
        };
        let Some(flags) = probe_help_flags(&py, &repo) else {
            return serde_json::json!({"ok": false, "error": "wgp.py --help printed no option list (broken checkout or Python env?)"});
        };
        write_flag_cache(&head, &flags);
        cached = Some((head.clone(), flags));
        fresh = true;
    }
    let (_, flags) = cached.unwrap_or_default();
    let dc = load_config_value();
    let saved = dc
        .get("launchArgs")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let want = launch_flags(&split_args(&saved));
    let unknown: Vec<String> = want
        .iter()
        .filter(|f| !flags.contains(f) && !flags.iter().any(|known| known.starts_with(f.as_str())))
        .cloned()
        .collect();
    serde_json::json!({
        "ok": true,
        "head": head,
        "learned": fresh,
        "known": flags.len(),
        "checked": want,
        "unknown": unknown,
    })
}

// ── Model-file integrity ──────────────────────────────────────────────
// The failure this exists for: a partially downloaded or HTML-error-page
// `.gguf` makes WanGP die deep inside the prompt enhancer with
// `ValueError: GGUF magic invalid` (upstream #2549), and a 0-byte
// `.safetensors` looks to the model manager like an installed model. Both are
// invisible from the launcher UI today, and both are one file-read away from
// being obvious.

/// Model extensions worth opening. Anything else (json, txt, images) is skipped.
fn is_model_file(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.ends_with(".gguf")
        || n.ends_with(".safetensors")
        || n.ends_with(".sft")
        || n.ends_with(".part")
        || n.ends_with(".incomplete")
        || n.ends_with(".download")
}

/// Why a model file cannot be used, if it cannot. Pure — the tests are the
/// specification of what the scanner reports.
pub(crate) fn classify_model_file(name: &str, head: &[u8], size: u64) -> Option<&'static str> {
    if !is_model_file(name) {
        return None;
    }
    if size == 0 {
        return Some("empty file (0 bytes)");
    }
    if head.starts_with(b"<!DOCTYPE") || head.starts_with(b"<html") || head.starts_with(b"<?xml") {
        return Some("HTML/XML page saved instead of a model");
    }
    let n = name.to_ascii_lowercase();
    if n.ends_with(".gguf") {
        return (head.len() < 4 || &head[..4] != b"GGUF")
            .then_some("missing GGUF magic (not a GGUF file)");
    }
    if n.ends_with(".safetensors") || n.ends_with(".sft") {
        if head.len() < 8 {
            return Some("shorter than a safetensors header");
        }
        let mut n8 = [0u8; 8];
        n8.copy_from_slice(&head[..8]);
        let hdr = u64::from_le_bytes(n8);
        // Real headers are a few KB to a few MB; 100 MB is already nonsense.
        if hdr == 0 || hdr > 100 * 1024 * 1024 {
            return Some("implausible safetensors header length");
        }
        if size < 8 + hdr {
            return Some("truncated safetensors (header runs past the end of the file)");
        }
        return None;
    }
    if n.ends_with(".part") || n.ends_with(".incomplete") || n.ends_with(".download") {
        return Some("unfinished download left on disk");
    }
    None
}

/// Model roots from `wgp_config.json` (`checkpoints_paths` / `checkpointsPaths`
/// / `ckpt_dir`, each a string or a list), falling back to `<repo>/ckpts`.
fn model_roots(repo: &std::path::Path) -> Vec<PathBuf> {
    let wgp: serde_json::Value = std::fs::read_to_string(repo.join("wgp_config.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null);
    let mut out: Vec<PathBuf> = Vec::new();
    for key in ["checkpoints_paths", "checkpointsPaths", "ckpt_dir"] {
        let Some(v) = wgp.get(key) else { continue };
        let items: Vec<&serde_json::Value> = match v {
            serde_json::Value::Array(a) => a.iter().collect(),
            other => vec![other],
        };
        for item in items {
            let Some(s) = item.as_str().map(str::trim).filter(|s| !s.is_empty()) else {
                continue;
            };
            let p = PathBuf::from(s);
            let p = if p.is_absolute() { p } else { repo.join(p) };
            if !out.contains(&p) {
                out.push(p);
            }
        }
        // Wan2GP writes one of these spellings; a stale leftover from an older
        // version must not add a second root.
        if !out.is_empty() {
            break;
        }
    }
    if out.is_empty() {
        out.push(repo.join("ckpts"));
    }
    out
}

/// Read the first bytes of a file plus its length; `None` when unreadable.
fn head_and_len(path: &std::path::Path) -> Option<(Vec<u8>, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = [0u8; 16];
    let n = f.read(&mut buf).ok()?;
    Some((buf[..n].to_vec(), meta.len()))
}

/// One entry per problem file, capped so a pathological folder cannot flood
/// the response.
fn scan_model_roots(roots: &[PathBuf]) -> (usize, Vec<serde_json::Value>, bool) {
    const MAX_ENTRIES: usize = 20000;
    const MAX_BAD: usize = 50;
    const MAX_DEPTH: usize = 6;
    const MAX_SECS: u64 = 20;
    let started = std::time::Instant::now();
    let mut queue: Vec<(PathBuf, usize)> = roots
        .iter()
        .filter(|p| p.exists())
        .map(|p| (p.clone(), 0))
        .collect();
    let mut scanned = 0usize;
    let mut bad: Vec<serde_json::Value> = Vec::new();
    let mut seen = 0usize;
    while let Some((dir, depth)) = queue.pop() {
        if seen >= MAX_ENTRIES || started.elapsed().as_secs() > MAX_SECS {
            return (scanned, bad, true);
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen >= MAX_ENTRIES || started.elapsed().as_secs() > MAX_SECS {
                return (scanned, bad, true);
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.is_dir() {
                if depth < MAX_DEPTH && !name.starts_with('.') {
                    queue.push((path, depth + 1));
                }
                continue;
            }
            if !is_model_file(&name) {
                continue;
            }
            scanned += 1;
            let Some((head, len)) = head_and_len(&path) else {
                continue;
            };
            if let Some(reason) = classify_model_file(&name, &head, len) {
                if bad.len() < MAX_BAD {
                    bad.push(serde_json::json!({
                        "path": path.to_string_lossy(),
                        "name": name,
                        "reason": reason,
                        "size": len,
                    }));
                }
            }
        }
    }
    (scanned, bad, false)
}

/// Diagnostics: are the model files on disk actually usable?
#[tauri::command]
pub fn troubleshoot_model_integrity() -> serde_json::Value {
    let repo = get_repo_dir();
    let roots = model_roots(&repo);
    let started = std::time::Instant::now();
    let (scanned, bad, truncated) = scan_model_roots(&roots);
    serde_json::json!({
        "ok": !roots.iter().all(|p| !p.exists()),
        "roots": roots.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>(),
        "scanned": scanned,
        "bad": bad,
        "truncated": truncated,
        "tookMs": started.elapsed().as_millis() as u64,
    })
}

// ── YuE2 startup crash: a bad `audio_prompt_type` inside output metadata ──
//
// WanGP stores every output's settings *inside the media file* (a `json` RIFF
// chunk for WAV, an ID3 `TXXX:WanGP` frame for MP3, a Vorbis comment for FLAC).
// At startup it walks the gallery and runs the model handler's `fix_settings`
// on each entry, so a single stale value aborts the UI before it paints:
// upstream's YuE2 migration raises
// `ValueError: Choose one of the five YuE2 composition sources.`
// when `audio_prompt_type` is not one of the five sources (upstream #2517,
// launcher #82). The launcher can find that file without a Python runtime —
// read the metadata, compare the value, offer to move the file out of the
// gallery — which is exactly what the reporter had to do by hand.

/// Sources upstream's `resolve_source()` accepts, verbatim (case-sensitive).
const YUE2_SOURCES: [&str; 5] = ["0", "1", "12", "A", "A2"];

fn is_audio_file(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.ends_with(".wav") || n.ends_with(".mp3") || n.ends_with(".flac")
}

/// `audio_prompt_type` from an embedded settings blob. Handles the JSON
/// spellings WanGP has written (`"A"`, `12`); unquoted values stop at the
/// first non-alphanumeric byte. Pure.
pub(crate) fn audio_prompt_type_in(text: &str) -> Option<String> {
    const KEY: &str = "\"audio_prompt_type\"";
    let rest = text.get(text.find(KEY)? + KEY.len()..)?.trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    if let Some(after) = rest.strip_prefix('"') {
        let end = after.find('"')?;
        return Some(after[..end].to_string());
    }
    let end = rest
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(rest.len());
    (end > 0).then(|| rest[..end].to_string())
}

/// A value upstream would reject. Empty is *not* rejected upstream — it falls
/// back to "0"/"1" — so only non-empty unknown values count.
pub(crate) fn yue2_source_invalid(value: &str) -> bool {
    let v = value.trim();
    !v.is_empty() && !YUE2_SOURCES.contains(&v)
}

/// Payload of a WAV `json` chunk. Walks chunk headers only, so a 4 GB WAV
/// costs a handful of seeks. `None` for non-RIFF files, missing chunks and
/// corrupt sizes (never a panic: a user file is untrusted input).
pub(crate) fn wav_json_chunk(path: &std::path::Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let mut hdr = [0u8; 12];
    f.read_exact(&mut hdr).ok()?;
    if &hdr[..4] != b"RIFF" || &hdr[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = 12u64;
    while pos + 8 <= len {
        f.seek(SeekFrom::Start(pos)).ok()?;
        let mut ch = [0u8; 8];
        f.read_exact(&mut ch).ok()?;
        let size = u32::from_le_bytes([ch[4], ch[5], ch[6], ch[7]]) as u64;
        let body = pos + 8;
        if body.saturating_add(size) > len {
            return None;
        }
        if &ch[..4] == b"json" {
            if size > 32 * 1024 * 1024 {
                return None;
            }
            let mut buf = vec![0u8; size as usize];
            f.seek(SeekFrom::Start(body)).ok()?;
            f.read_exact(&mut buf).ok()?;
            return Some(String::from_utf8_lossy(&buf).to_string());
        }
        pos = body + size + (size & 1);
    }
    None
}

/// Settings text embedded in an audio file. MP3/FLAC keep their tags in the
/// first metadata block, so a bounded read is enough; anything without the key
/// is skipped without a second read.
fn embedded_settings_text(path: &std::path::Path, name: &str) -> Option<String> {
    use std::io::Read;
    if name.to_ascii_lowercase().ends_with(".wav") {
        return wav_json_chunk(path).filter(|s| s.contains("audio_prompt_type"));
    }
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; 512 * 1024];
    let got = f.read(&mut buf).ok()?;
    buf.truncate(got);
    let text = String::from_utf8_lossy(&buf).to_string();
    text.contains("audio_prompt_type").then_some(text)
}

/// Output folders the gallery reads: configured save paths first, then the
/// repo defaults.
fn yue2_output_roots(repo: &std::path::Path) -> Vec<PathBuf> {
    let cfg: serde_json::Value = std::fs::read_to_string(repo.join("wgp_config.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null);
    crate::deepy_web::outputs_candidates(&cfg, repo)
        .into_iter()
        .map(PathBuf::from)
        .collect()
}

/// Workspace manifest root: `workspaces_dir` from `wgp_config.json`, relative
/// to the checkout when not absolute, else `./workspaces`.
fn yue2_workspace_dir(repo: &std::path::Path) -> PathBuf {
    let cfg: serde_json::Value = std::fs::read_to_string(repo.join("wgp_config.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null);
    match cfg
        .get("workspaces_dir")
        .and_then(|x| x.as_str())
        .filter(|s| !s.trim().is_empty())
    {
        Some(raw) => {
            let p = std::path::Path::new(raw.trim());
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                repo.join(p)
            }
        }
        None => repo.join("workspaces"),
    }
}

/// Audio files the saved galleries point at. Workspaces store absolute paths
/// (dedicated folders, imports outside the output roots), so scanning only
/// the output folders can miss the file that aborts startup. Virtual media
/// paths (`file.wav|start_frame=…`) are reduced to their source file.
fn yue2_gallery_paths(repo: &std::path::Path) -> Vec<PathBuf> {
    let root = yue2_workspace_dir(repo);
    let mut out: Vec<PathBuf> = Vec::new();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for dir in [root.clone(), root.join("archives")] {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(record) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            let Some(gallery) = record.get("gallery").and_then(|g| g.as_object()) else {
                continue;
            };
            for (key, value) in gallery {
                if !key.ends_with("_file_list") {
                    continue;
                }
                let Some(items) = value.as_array() else {
                    continue;
                };
                for item in items {
                    let raw = item
                        .as_str()
                        .or_else(|| item.get("path").and_then(|p| p.as_str()));
                    let Some(raw) = raw else {
                        continue;
                    };
                    let source = raw.split('|').next().unwrap_or(raw).trim();
                    if source.is_empty() || !is_audio_file(source) {
                        continue;
                    }
                    let p = PathBuf::from(source);
                    if seen.insert(p.clone()) {
                        out.push(p);
                    }
                }
            }
        }
    }
    out
}

/// Read one audio file and append a report entry when upstream would reject it.
fn inspect_yue2_audio(
    path: &std::path::Path,
    name: &str,
    size: u64,
    bad: &mut Vec<serde_json::Value>,
) {
    let Some(text) = embedded_settings_text(path, name) else {
        return;
    };
    let Some(value) = audio_prompt_type_in(&text) else {
        return;
    };
    if !yue2_source_invalid(&value) {
        return;
    }
    if bad.len() < MAX_BAD {
        bad.push(serde_json::json!({
            "path": path.to_string_lossy(),
            "name": name,
            "value": value,
            "size": size,
        }));
    }
}

const MAX_ENTRIES: usize = 20000;
const MAX_BAD: usize = 50;
const MAX_DEPTH: usize = 6;
const MAX_SECS: u64 = 20;

/// One entry per output whose embedded `audio_prompt_type` upstream's YuE2
/// migration would reject. Capped like the model scanner.
fn scan_yue2_metadata(
    roots: &[PathBuf],
    referenced: &[PathBuf],
) -> (usize, Vec<serde_json::Value>, bool) {
    let started = std::time::Instant::now();
    let mut scanned = 0usize;
    let mut bad: Vec<serde_json::Value> = Vec::new();
    let mut seen_files: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

    // Saved-gallery references first: they may live outside every output root.
    for path in referenced {
        if started.elapsed().as_secs() > MAX_SECS {
            return (scanned, bad, true);
        }
        let Ok(meta) = std::fs::metadata(path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if !is_audio_file(&name) {
            continue;
        }
        if !seen_files.insert(path.clone()) {
            continue;
        }
        scanned += 1;
        inspect_yue2_audio(path, &name, meta.len(), &mut bad);
    }

    let mut queue: Vec<(PathBuf, usize)> = roots
        .iter()
        .filter(|p| p.exists())
        .map(|p| (p.clone(), 0))
        .collect();
    let mut seen = 0usize;
    while let Some((dir, depth)) = queue.pop() {
        if seen >= MAX_ENTRIES || started.elapsed().as_secs() > MAX_SECS {
            return (scanned, bad, true);
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen >= MAX_ENTRIES || started.elapsed().as_secs() > MAX_SECS {
                return (scanned, bad, true);
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.is_dir() {
                if depth < MAX_DEPTH && !name.starts_with('.') && name != "yue2-quarantine" {
                    queue.push((path, depth + 1));
                }
                continue;
            }
            if !is_audio_file(&name) {
                continue;
            }
            if !seen_files.insert(path.clone()) {
                continue;
            }
            scanned += 1;
            inspect_yue2_audio(&path, &name, meta.len(), &mut bad);
        }
    }
    (scanned, bad, false)
}

/// Diagnostics body, with the checkout passed in so the tests can drive it.
pub(crate) fn yue2_report(repo: &std::path::Path) -> serde_json::Value {
    let roots = yue2_output_roots(repo);
    let referenced = yue2_gallery_paths(repo);
    let started = std::time::Instant::now();
    let (scanned, bad, truncated) = scan_yue2_metadata(&roots, &referenced);
    serde_json::json!({
        "ok": !roots.iter().all(|p| !p.exists()) || !referenced.is_empty(),
        "roots": roots.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>(),
        "referenced": referenced.len(),
        "scanned": scanned,
        "bad": bad,
        "truncated": truncated,
        "tookMs": started.elapsed().as_millis() as u64,
    })
}

/// Quarantine body: rescan, then move what the scan found.
pub(crate) fn yue2_quarantine(repo: &std::path::Path) -> serde_json::Value {
    let roots = yue2_output_roots(repo);
    let referenced = yue2_gallery_paths(repo);
    let (_, bad, _) = scan_yue2_metadata(&roots, &referenced);
    let (dirs, moved, failed) = quarantine_bad(&bad);
    serde_json::json!({
        "ok": failed.is_empty() && !moved.is_empty(),
        "error": failed.first().map(|f| f["error"].clone()),
        "dirs": dirs,
        "moved": moved,
        "failed": failed,
    })
}

/// Diagnostics: does any generated audio carry settings WanGP cannot migrate?
/// Read-only — nothing is written, moved or deleted.
#[tauri::command]
pub fn troubleshoot_yue2_metadata() -> serde_json::Value {
    yue2_report(&get_repo_dir())
}

/// Move `bad` (as returned by the scan) into `<home>/yue2-quarantine/`.
/// Returns the quarantine folder plus what moved and what could not.
pub(crate) fn quarantine_bad(
    bad: &[serde_json::Value],
) -> (Vec<String>, Vec<serde_json::Value>, Vec<serde_json::Value>) {
    let mut dirs: Vec<String> = Vec::new();
    let mut moved = Vec::new();
    let mut failed = Vec::new();
    for item in bad {
        let from = PathBuf::from(item["path"].as_str().unwrap_or_default());
        let name = match from.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        // Beside the file it came from: a custom save_path is not the repo's
        // `outputs`, and a rename stays on one volume this way.
        let Some(parent) = from.parent() else {
            continue;
        };
        let dir = parent.join("yue2-quarantine");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            failed.push(serde_json::json!({
                "name": name,
                "error": format!("cannot create {}: {e}", dir.display()),
            }));
            continue;
        }
        let dir_s = dir.to_string_lossy().to_string();
        if !dirs.contains(&dir_s) {
            dirs.push(dir_s);
        }
        let mut target = dir.join(&name);
        let mut i = 1;
        while target.exists() {
            let stem = from
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("output");
            let ext = from.extension().and_then(|s| s.to_str()).unwrap_or("");
            let candidate = if ext.is_empty() {
                format!("{stem}-{i}")
            } else {
                format!("{stem}-{i}.{ext}")
            };
            target = dir.join(candidate);
            i += 1;
        }
        match std::fs::rename(&from, &target) {
            Ok(_) => moved.push(serde_json::json!({
                "from": from.to_string_lossy(),
                "to": target.to_string_lossy(),
                "name": name,
                "value": item["value"],
            })),
            Err(e) => failed.push(serde_json::json!({
                "name": name,
                "error": e.to_string(),
            })),
        }
    }
    (dirs, moved, failed)
}

/// Move the offending outputs out of the gallery so WanGP can start. Never
/// deletes: the files keep their bytes and their names (numeric suffix on
/// collision), and moving them back restores the gallery.
#[tauri::command]
pub fn troubleshoot_yue2_quarantine() -> serde_json::Value {
    yue2_quarantine(&get_repo_dir())
}

#[cfg(test)]
mod troubleshoot_tests {
    use super::*;
    #[test]
    fn triton_repair_runs_pinned_commands_shown_in_console() {
        // The console echo and the spawn share these builders, so what the
        // user reads is exactly what runs — including the safety flags.
        assert_eq!(
            triton_pip_show_argv(),
            vec!["-m", "pip", "show", "triton-windows", "triton"]
        );
        assert_eq!(
            triton_pip_reinstall_argv("triton-windows==3.6.0.post24"),
            [
                "-m",
                "pip",
                "install",
                "--force-reinstall",
                "--no-deps",
                "triton-windows==3.6.0.post24"
            ]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
        );
        assert_eq!(
            render_py_command(
                r"C:\Wan2GP\env_uv\Scripts\python.exe",
                &triton_pip_reinstall_argv("triton-windows==3.6.0.post24")
            ),
            r"[>] C:\Wan2GP\env_uv\Scripts\python.exe -m pip install --force-reinstall --no-deps triton-windows==3.6.0.post24"
        );
    }

    #[test]
    fn triton_rollback_spec_and_version_gate() {
        // Rollback builds the same exact-pin spec for the older build.
        assert_eq!(
            triton_reinstall_spec("triton-windows", "3.6.0.post24"),
            "triton-windows==3.6.0.post24"
        );
        // Single tokens pass; empty/whitespace fail closed.
        assert!(valid_repair_version("3.6.0.post24"));
        assert!(!valid_repair_version(""));
        assert!(!valid_repair_version("3.6.0 post24"));
    }

    #[test]
    fn triton_repair_prefers_windows_dist_and_pins_exact() {
        let both = "Name: triton-windows\nVersion: 3.6.0.post26\nSummary: Triton\n---\nName: triton\nVersion: 3.6.0\nSummary: Triton\n";
        assert_eq!(
            parse_triton_pip_show(both),
            Some(("triton-windows".to_string(), "3.6.0.post26".to_string()))
        );
        // Plain triton alone still repairs (never None when Triton exists).
        let plain = "Name: triton\nVersion: 3.6.0\nLocation: x\n";
        assert_eq!(
            parse_triton_pip_show(plain),
            Some(("triton".to_string(), "3.6.0".to_string()))
        );
        // Nothing Triton-shaped → None (caller errors "not installed").
        assert_eq!(
            parse_triton_pip_show("Name: torch\nVersion: 2.10.0\n"),
            None
        );
        assert_eq!(parse_triton_pip_show(""), None);
        // The spec pins the detected build exactly — floating to latest
        // would break upstream parity.
        assert_eq!(
            triton_reinstall_spec("triton-windows", "3.6.0.post26"),
            "triton-windows==3.6.0.post26"
        );
    }
    #[test]
    fn vs_buildtools_offer_is_official_and_complete() {
        // Evergreen Microsoft short link, exact bootstrapper binary.
        assert!(
            VS_BUILDTOOLS_URL.starts_with("https://aka.ms/vs/"),
            "{VS_BUILDTOOLS_URL}"
        );
        assert!(VS_BUILDTOOLS_URL.ends_with("vs_BuildTools.exe"));
        let args = vs_buildtools_args();
        assert!(args.contains(&"--passive"), "{args:?}");
        assert!(args.contains(&"Microsoft.VisualStudio.Workload.VCTools"));
        assert!(
            args.contains(&"--includeRecommended"),
            "without the SDK our INCLUDE/LIB discovery finds nothing: {args:?}"
        );
    }
    #[test]
    fn vs_buildtools_launch_is_elevated_fire_and_forget() {
        let ps = vs_buildtools_ps_launch(r"C:\Temp\wgp-vs_BuildTools.exe");
        assert!(
            ps.contains("-Verb RunAs"),
            "UAC elevation is the whole point"
        );
        assert!(ps.contains("Microsoft.VisualStudio.Workload.VCTools"));
        assert!(ps.contains(r"C:\Temp\wgp-vs_BuildTools.exe"));
        assert!(
            !ps.contains("-Wait"),
            "a 10-minute -Wait would wedge the invoke; the installer has its own UI"
        );
    }
    #[test]
    fn known_good_recipe_covers_every_nvidia_class() {
        // Attention + profile + compile, exactly as documented.
        let gtx = known_good_recipe("GTX_10").unwrap();
        assert_eq!(gtx["settings"]["attention_mode"], "sdpa");
        assert_eq!(gtx["settings"]["video_profile"], 4.0);
        assert_eq!(gtx["settings"]["compile"], "disabled");

        let r20 = known_good_recipe("RTX_20").unwrap();
        assert_eq!(r20["settings"]["attention_mode"], "sage");

        for k in ["RTX_30", "RTX_40"] {
            let r = known_good_recipe(k).unwrap();
            assert_eq!(r["settings"]["attention_mode"], "sage2");
            assert_eq!(r["settings"]["video_profile"], 3.0);
            assert_eq!(r["settings"]["compile"], "cudagraph");
        }

        let r50 = known_good_recipe("RTX_50").unwrap();
        assert_eq!(r50["settings"]["attention_mode"], "sage2");
        assert_eq!(r50["settings"]["video_profile"], 4.0);
    }

    /// No upstream recipe for these — failing closed beats inventing settings.
    /// Auto-Tune's recommendation and the Troubleshooting recipe must agree —
    /// they read the same definition, so this pins both against one table.
    #[test]
    fn recipe_and_autotune_agree_on_attention() {
        for (key, want) in [
            ("GTX_10", "sdpa"),
            ("RTX_20", "sage"),
            ("RTX_30", "sage2"),
            ("RTX_40", "sage2"),
            ("RTX_50", "sage2"),
        ] {
            assert_eq!(crate::hw::attention_for_profile(key), Some(want), "{key}");
            let r = known_good_recipe(key).unwrap();
            assert_eq!(r["settings"]["attention_mode"], want, "recipe for {key}");
        }
        // No Sage build upstream on the ROCm / CPU paths.
        for k in ["AMD_GFX1201", "AMD_GFX110X", "INTEL_XPU", "CPU", ""] {
            assert_eq!(crate::hw::attention_for_profile(k), None, "{k}");
        }
    }

    #[test]
    fn known_good_recipe_refuses_unknown_and_non_nvidia() {
        for k in [
            "AMD_GFX1201",
            "AMD_GFX110X",
            "INTEL_XPU",
            "CPU",
            "",
            "RTX_99",
        ] {
            assert!(known_good_recipe(k).is_none(), "{k} must have no recipe");
        }
    }

    /// Every key a recipe writes must be appliable, or Apply would report
    /// success and drop it (the allowlist bug).
    #[test]
    fn recipe_keys_are_appliable() {
        for key in ["GTX_10", "RTX_20", "RTX_30", "RTX_40", "RTX_50"] {
            let r = known_good_recipe(key).unwrap();
            let s = r["settings"].as_object().unwrap();
            for k in s.keys() {
                assert!(
                    crate::features::MEMORY_OVERRIDE_KEYS.contains(&k.as_str()),
                    "{key}: {k} is not in the Apply allowlist"
                );
            }
        }
    }

    #[test]
    fn oom_remedies_are_writable() {
        for action in [
            "head_split_low",
            "head_split_medium",
            "head_split_high",
            "lower_reserved_ram",
        ] {
            let s = oom_remedy_settings(action).unwrap();
            for k in s.as_object().unwrap().keys() {
                assert!(
                    crate::features::MEMORY_OVERRIDE_KEYS.contains(&k.as_str()),
                    "{action}: {k} not appliable"
                );
            }
        }
        assert!(oom_remedy_settings("nonsense").is_none());
    }

    #[test]
    fn merge_flags_dedupes() {
        let m = merge_launch_flags(
            "--profile 3 --attention sage2 --verbose 2",
            &[("--attention", Some("sdpa")), ("--fp16", None)],
        );
        assert!(
            m.contains("--attention sdpa")
                && !m.contains("sage2")
                && m.contains("--verbose 2")
                && m.contains("--fp16")
        );
        let m2 = merge_launch_flags(
            "",
            &[
                ("--attention", Some("sdpa")),
                ("--profile", Some("4")),
                ("--fp16", None),
            ],
        );
        assert_eq!(m2, "--attention sdpa --profile 4 --fp16");
    }

    /// argparse help: options are indented, values follow on the same line, and
    /// wrapped description lines must not become "flags".
    #[test]
    fn flags_from_help_keeps_only_long_options() {
        let help = "usage: wgp.py [-h] [--gpu DEVICE]\n\n\
options:\n\
  -h, --help            show this help message and exit\n\
  --gpu DEVICE          Run on specific GPU device\n\
  --preload NUMBER      Preload N MB\n\
                        of the model (wrapped description)\n\
  --no-prevent-power-throttling\n\
  --server-port PORT, --server_port PORT\n\
  --gpu DEVICE\n";
        let flags = flags_from_help(help);
        assert_eq!(
            flags,
            vec![
                "--gpu".to_string(),
                "--help".to_string(),
                "--no-prevent-power-throttling".to_string(),
                "--preload".to_string(),
                "--server-port".to_string(),
            ]
        );
        // A traceback (no options) must not look like a learned flag set.
        assert!(flags_from_help("Traceback (most recent call last):\nValueError").is_empty());
    }

    #[test]
    fn launch_flags_drops_values_and_dedupes() {
        let args: Vec<String> = [
            "wgp.py",
            "--server-port",
            "7861",
            "--gpu",
            "cuda:0",
            "--attention=sdpa",
            "--advanced",
            "--advanced",
            "value-not-a-flag",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            launch_flags(&args),
            vec!["--server-port", "--gpu", "--attention", "--advanced"]
        );
    }

    #[test]
    fn classify_model_file_catches_the_reported_failure_modes() {
        let gguf_ok = b"GGUFxxxx";
        assert_eq!(classify_model_file("m.gguf", gguf_ok, 1000), None);
        // Upstream #2549: the prompt enhancer dies with "GGUF magic invalid".
        assert_eq!(
            classify_model_file("mtp.gguf", b"NOPEzzzz", 1000),
            Some("missing GGUF magic (not a GGUF file)")
        );
        // A 404/HTML page saved under a model name.
        assert_eq!(
            classify_model_file("m.safetensors", b"<!DOCTYPE html>", 512),
            Some("HTML/XML page saved instead of a model")
        );
        assert_eq!(
            classify_model_file("m.gguf", b"", 0),
            Some("empty file (0 bytes)")
        );
        // Valid safetensors: 8-byte LE header length, then the JSON header.
        let mut st = Vec::new();
        st.extend_from_slice(&16u64.to_le_bytes());
        st.extend_from_slice(b"{\"a\":1}         ");
        assert_eq!(classify_model_file("m.safetensors", &st, 24), None);
        // Header claims more than the file holds → truncated download.
        let mut st = Vec::new();
        st.extend_from_slice(&1024u64.to_le_bytes());
        st.extend_from_slice(b"{\"a\":1}");
        assert_eq!(
            classify_model_file("m.safetensors", &st, 16),
            Some("truncated safetensors (header runs past the end of the file)")
        );
        assert_eq!(
            classify_model_file("m.part", b"GGUF", 1000),
            Some("unfinished download left on disk")
        );
        // Non-model files are never flagged even when the bytes look odd.
        assert_eq!(classify_model_file("notes.txt", b"<html>", 10), None);
    }

    #[test]
    fn scan_reports_bad_files_and_skips_good_ones() {
        let dir = std::env::temp_dir().join(format!(
            "wan2gp-integrity-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("good.gguf"), b"GGUF0000000000000").unwrap();
        std::fs::write(dir.join("sub/bad.gguf"), b"not a gguf at all").unwrap();
        std::fs::write(dir.join("notes.txt"), b"<html>").unwrap();
        let (scanned, bad, truncated) = scan_model_roots(std::slice::from_ref(&dir));
        assert_eq!(scanned, 2, "only model files are counted");
        assert!(!truncated);
        assert_eq!(bad.len(), 1);
        assert!(bad[0]["name"].as_str().unwrap().ends_with("bad.gguf"));
        assert!(bad[0]["reason"].as_str().unwrap().contains("GGUF magic"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn model_roots_reads_wgp_config_paths() {
        let repo = temp_dir("roots");
        // One absolute folder (whatever the platform's volume looks like) and
        // one checkout-relative path, exactly as Wan2GP writes them. A
        // hardcoded `D:\...` is relative on macOS/Linux and would be joined to
        // the repo instead of being kept.
        let external = temp_dir("roots-external");
        let cfg = serde_json::json!({
            "checkpoints_paths": [external.to_string_lossy(), "ckpts"],
            "ckpt_dir": "ignored"
        });
        std::fs::write(repo.join("wgp_config.json"), cfg.to_string()).unwrap();
        let roots = model_roots(&repo);
        assert_eq!(roots.len(), 2, "duplicates collapsed, ckpt_dir unused");
        assert_eq!(roots[0], external);
        assert_eq!(roots[1], repo.join("ckpts"));
        let _ = std::fs::remove_dir_all(&external);
        // No config at all → the checkout's own ckpts folder.
        let _ = std::fs::remove_file(repo.join("wgp_config.json"));
        assert_eq!(model_roots(&repo), vec![repo.join("ckpts")]);
        let _ = std::fs::remove_dir_all(&repo);
    }

    // ── YuE2 metadata scan (launcher #82 = upstream #2517) ──

    /// Minimal RIFF/WAVE with a `fmt ` chunk, a `data` chunk and `payload`
    /// stored in WanGP's `json` chunk (`write_wav_text_chunk`).
    fn wav_with_json(payload: &[u8]) -> Vec<u8> {
        let mut body = b"WAVE".to_vec();
        for (id, data) in [
            (b"fmt ", vec![0u8; 16]),
            (b"data", vec![0u8; 8]),
            (b"json", payload.to_vec()),
        ] {
            body.extend_from_slice(id);
            body.extend_from_slice(&(data.len() as u32).to_le_bytes());
            body.extend_from_slice(&data);
            if data.len() % 2 == 1 {
                body.push(0);
            }
        }
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "wan2gp-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn yue2_source_value_is_read_and_validated_like_upstream() {
        assert_eq!(
            audio_prompt_type_in(r#"{"audio_prompt_type": "AV"}"#).as_deref(),
            Some("AV")
        );
        assert_eq!(
            audio_prompt_type_in(r#"{"audio_model":"yue2","audio_prompt_type":"A2"}"#).as_deref(),
            Some("A2")
        );
        // Old versions wrote the source as a number.
        assert_eq!(
            audio_prompt_type_in(r#"{"audio_prompt_type": 12}"#).as_deref(),
            Some("12")
        );
        // Nothing to read: no key, or no value after it.
        assert_eq!(audio_prompt_type_in(r#"{"audio_model":"yue2"}"#), None);
        assert_eq!(audio_prompt_type_in(r#"{"audio_prompt_type": }"#), None);
        // The five upstream sources pass; anything else is what aborts startup.
        for ok in YUE2_SOURCES {
            assert!(!yue2_source_invalid(ok), "{ok}");
        }
        for bad in ["AV", "av", "A1", "continue", "2"] {
            assert!(yue2_source_invalid(bad), "{bad}");
        }
        // Empty falls back to "0"/"1" upstream, so it is not a blocker.
        assert!(!yue2_source_invalid(""));
        assert!(!yue2_source_invalid("  "));
    }

    #[test]
    fn wav_json_chunk_reads_only_the_chunk_and_survives_corruption() {
        let dir = temp_dir("wav-json");
        let good = dir.join("a.wav");
        std::fs::write(&good, wav_with_json(br#"{"audio_prompt_type":"AV"}"#)).unwrap();
        assert_eq!(
            wav_json_chunk(&good).as_deref(),
            Some(r#"{"audio_prompt_type":"AV"}"#)
        );
        // A WAV without the chunk, a non-RIFF file and a truncated chunk all
        // return None instead of panicking on user data.
        let plain = dir.join("plain.wav");
        std::fs::write(&plain, wav_with_json(b"")).unwrap();
        assert_eq!(wav_json_chunk(&plain).as_deref(), Some(""));
        let not_wav = dir.join("b.wav");
        std::fs::write(&not_wav, b"ID3 not a riff file").unwrap();
        assert_eq!(wav_json_chunk(&not_wav), None);
        let corrupt = dir.join("c.wav");
        let mut bytes = wav_with_json(br#"{"audio_prompt_type":"AV"}"#);
        // Inflate the `data` chunk size past EOF: the walk must stop, not read.
        bytes[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
        std::fs::write(&corrupt, bytes).unwrap();
        assert_eq!(wav_json_chunk(&corrupt), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn yue2_scan_finds_the_one_bad_output_and_quarantines_it() {
        let root = temp_dir("yue2-scan");
        std::fs::write(
            root.join("good.wav"),
            wav_with_json(br#"{"audio_prompt_type":"A"}"#),
        )
        .unwrap();
        std::fs::write(
            root.join("bad.wav"),
            wav_with_json(br#"{"audio_prompt_type":"AV"}"#),
        )
        .unwrap();
        // Not audio → never opened.
        std::fs::write(root.join("preview.png"), b"\x89PNG").unwrap();
        let (scanned, bad, truncated) = scan_yue2_metadata(std::slice::from_ref(&root), &[]);
        assert_eq!(scanned, 2, "only audio files are read");
        assert!(!truncated);
        assert_eq!(bad.len(), 1, "{bad:?}");
        assert_eq!(bad[0]["value"], "AV");
        assert!(bad[0]["name"].as_str().unwrap().ends_with("bad.wav"));

        let (dirs, moved, failed) = quarantine_bad(&bad);
        assert!(failed.is_empty(), "{failed:?}");
        assert_eq!(moved.len(), 1);
        assert_eq!(
            dirs,
            vec![root.join("yue2-quarantine").to_string_lossy().to_string()]
        );
        let dir = PathBuf::from(&dirs[0]);
        assert!(!root.join("bad.wav").exists(), "moved out of the gallery");
        assert!(dir.join("bad.wav").exists(), "bytes kept, not deleted");
        assert!(root.join("good.wav").exists(), "only the offender moves");

        // The same name again lands beside the first one instead of overwriting.
        std::fs::write(
            root.join("bad.wav"),
            wav_with_json(br#"{"audio_prompt_type":"AV"}"#),
        )
        .unwrap();
        let (_, bad2, _) = scan_yue2_metadata(std::slice::from_ref(&root), &[]);
        assert_eq!(bad2.len(), 1, "the quarantined copy is out of the scan");
        let (_, moved2, failed2) = quarantine_bad(&bad2);
        assert!(failed2.is_empty(), "{failed2:?}");
        assert_eq!(moved2.len(), 1);
        assert!(dir.join("bad-1.wav").exists(), "{moved2:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn yue2_report_follows_workspace_gallery_references() {
        let repo = temp_dir("yue2-workspace");
        // The offender is outside every configured output root: only the saved
        // gallery points at it. This is the case the output-only scan missed.
        let media = repo.join("elsewhere");
        std::fs::create_dir_all(&media).unwrap();
        let bad = media.join("song.wav");
        std::fs::write(&bad, wav_with_json(br#"{"audio_prompt_type":"AV"}"#)).unwrap();
        std::fs::write(
            repo.join("wgp_config.json"),
            r#"{"workspaces_dir":"ws-custom"}"#,
        )
        .unwrap();
        let workspaces = repo.join("ws-custom");
        std::fs::create_dir_all(&workspaces).unwrap();
        std::fs::write(
            workspaces.join("Default.json"),
            serde_json::json!({
                "gallery": {
                    // Virtual media paths carry a `|start_frame=…` suffix.
                    "audio_file_list": [format!("{}|start_frame=0", bad.to_string_lossy())],
                }
            })
            .to_string(),
        )
        .unwrap();

        let r = yue2_report(&repo);
        assert_eq!(r["referenced"], 1, "{r}");
        assert_eq!(r["scanned"], 1, "{r}");
        assert_eq!(r["bad"][0]["name"], "song.wav", "{r}");
        assert_eq!(r["bad"][0]["value"], "AV");
        assert!(r["bad"][0]["path"].as_str().unwrap().ends_with("song.wav"));

        let q = yue2_quarantine(&repo);
        assert_eq!(q["ok"], true, "{q}");
        assert!(!bad.exists());
        assert!(media.join("yue2-quarantine").join("song.wav").exists());
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn yue2_report_reads_the_configured_save_path() {
        let repo = temp_dir("yue2-repo");
        // A custom save_path outside `outputs/` is where WanGP writes, so that
        // is where the offender lives — the repo defaults must not hide it.
        std::fs::write(
            repo.join("wgp_config.json"),
            format!(
                r#"{{"save_path":"{}"}}"#,
                repo.join("my-out").to_string_lossy().replace('\\', "\\\\")
            ),
        )
        .unwrap();
        std::fs::create_dir_all(repo.join("my-out").join("deep")).unwrap();
        std::fs::write(
            repo.join("my-out").join("deep").join("song.wav"),
            wav_with_json(br#"{"audio_prompt_type":"AV"}"#),
        )
        .unwrap();
        let r = yue2_report(&repo);
        assert_eq!(r["ok"], true);
        assert_eq!(r["scanned"], 1);
        assert_eq!(r["bad"][0]["name"], "song.wav");
        // Nothing written by a read-only report.
        assert!(repo.join("my-out").join("deep").join("song.wav").exists());

        let q = yue2_quarantine(&repo);
        assert_eq!(q["ok"], true, "{q}");
        assert_eq!(q["moved"].as_array().unwrap().len(), 1);
        assert!(repo
            .join("my-out")
            .join("deep")
            .join("yue2-quarantine")
            .join("song.wav")
            .exists());
        assert_eq!(yue2_report(&repo)["bad"].as_array().unwrap().len(), 0);
        // Second pass finds nothing to move and says so.
        assert_eq!(yue2_quarantine(&repo)["ok"], false);
        let _ = std::fs::remove_dir_all(&repo);
    }
}

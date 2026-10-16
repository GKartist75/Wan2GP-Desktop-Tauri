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
            Ok(p) => verify_kernels(&py, p.torch, p.device, "native", false, None),
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
            let stale = crate::amd::kernel_probe_stale(&map);
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
    #[cfg(windows)]
    {
        let ps = format!("Get-NetTCPConnection -LocalPort {port} -State Listen | Select-Object -First 1 -ExpandProperty OwningProcess");
        if let Ok(o) = silent_command("powershell")
            .args(["-NoProfile", "-Command", &ps])
            .output()
        {
            if o.status.success() {
                let pid_s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if let Ok(pid) = pid_s.parse::<u32>() {
                    let ps2 = format!(
                        "(Get-Process -Id {pid} -ErrorAction SilentlyContinue).ProcessName"
                    );
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
                        .map(|o| {
                            String::from_utf8_lossy(&o.stdout)
                                .trim()
                                .chars()
                                .take(300)
                                .collect::<String>()
                        })
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
        #[cfg(windows)]
        let _ = silent_command("taskkill")
            .args(["/pid", &pid.to_string(), "/f", "/t"])
            .output();
        #[cfg(not(windows))]
        let _ = silent_command("kill")
            .arg("-9")
            .arg(pid.to_string())
            .output();
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
        return serde_json::json!({"enabled": true, "note": "Windows only"});
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

fn long_paths_currently_enabled() -> bool {
    #[cfg(not(windows))]
    {
        return true;
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
        return Err("Windows only".into());
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
    let cuda_ok = cuda
        .get("available")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
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
        .get()
        .and_then(|m| m.lock().ok())
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
        "\n**v17 RAM/VRAM** mmgp={mmgp} allocator={alloc} head_split={hs} smart_pinning={pin} read_ahead={ra} reserved_pct={res} preload v/i/a={pv}/{pi}/{pa}\n**Kernels** attention={att} int8={i8} precision={prec} compile={comp}",
        mmgp = crate::status::pyproject_version(
            &std::fs::read_to_string(repo.join("mmgp").join("pyproject.toml")).unwrap_or_default(),
        )
        .unwrap_or_else(|| "(not vendored)".into()),
        alloc = pick("vram_allocator"),
        hs = pick("attention_head_split"),
        pin = pick("smart_memory_pinning"),
        ra = pick("read_ahead"),
        res = pick("perc_reserved_mem_max"),
        pv = pick("video_preload_mode"),
        pi = pick("image_preload_mode"),
        pa = pick("audio_preload_mode"),
        att = pick("attention_mode"),
        i8 = pick("int8_kernels"),
        prec = pick("kernel_precision"),
        comp = pick("compile"),
);
let md = format!(
        "**Launcher** v{ver} ({os}/{arch})\n**GPU** {gpu} ({vendor}, {vram} MB VRAM)\n**Python** {py}\n**Torch** {torch} + CUDA {cuda_v} (cuda_available={cuda_ok}){amd}\n**Launch** port={port} server={server} share={share} gpu={gpudev} verbose={verbose} args=`{args}`\n**Profiles** video={vp} image={ip} audio={ap} quant={q}{v17}\n**Log tail**\n```\n{tail}\n```",
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
                let _ = crate::base::push_log(
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
    ["-m", "pip", "install", "--force-reinstall", "--no-deps", spec]
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
pub fn troubleshoot_triton_repair(
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
    let _ = echo(&format!("{}\n", render_py_command(&py_s, &show_argv)));
    let show = silent_command(&py_s)
        .args(&show_argv)
        .output()
        .map_err(|e| format!("pip show failed: {e}"))?;
    let show_text = String::from_utf8_lossy(&show.stdout).to_string();
    let Some((dist, detected)) = parse_triton_pip_show(&show_text) else {
        return Err("No Triton distribution installed in the active env — run Install first".into());
    };
    // Explicit older build (Roll back button) or the detected one. Either
    // way an exact == pin — never floating.
    let want = version.unwrap_or_default();
    let want = if want.trim().is_empty() {
        detected.clone()
    } else {
        if !valid_repair_version(&want) {
            return Err("Bad Triton version — single version token only (e.g. 3.6.0.post24)".into());
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
            String::from_utf8_lossy(&inst.stderr).chars().take(500).collect::<String>()
        ));
    }
    let backup = clear_triton_caches()?;
    let check_argv = vec!["-c".to_string(), "import triton; print(triton.__version__)".to_string()];
    echo(&format!("{}\n", render_py_command(&py_s, &check_argv)));
    let imp = silent_command(&py_s)
        .args(&check_argv)
        .output();
    let import_version = match imp {
        Ok(o) if o.status.success() => Some(
            String::from_utf8_lossy(&o.stdout).trim().to_string(),
        ),
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
pub async fn troubleshoot_msvc_install(
    app: tauri::AppHandle,
) -> Result<serde_json::Value, String> {
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
        "head_split_medium" => Some(serde_json::json!({"attention_head_split": 2})),
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
        other => return serde_json::json!({"ok": false, "error": format!("unknown action '{other}'")}),
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
            vec!["-m", "pip", "install", "--force-reinstall", "--no-deps", "triton-windows==3.6.0.post24"]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            render_py_command(r"C:\Wan2GP\env_uv\Scripts\python.exe", &triton_pip_reinstall_argv("triton-windows==3.6.0.post24")),
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
    fn triton_repair_prefers_windows_dist_and_pins_exact() {        let both = "Name: triton-windows\nVersion: 3.6.0.post26\nSummary: Triton\n---\nName: triton\nVersion: 3.6.0\nSummary: Triton\n";
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
        assert_eq!(parse_triton_pip_show("Name: torch\nVersion: 2.10.0\n"), None);
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
        assert!(ps.contains("-Verb RunAs"), "UAC elevation is the whole point");
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
        for k in ["AMD_GFX1201", "AMD_GFX110X", "INTEL_XPU", "CPU", "", "RTX_99"] {
            assert!(known_good_recipe(k).is_none(), "{k} must have no recipe");
        }
    }

    /// Every key a recipe writes must be appliable, or Apply would report
    /// success and drop it (the allowlist bug).
    #[test]
    fn recipe_keys_are_appliable_and_valid() {
        for key in ["GTX_10", "RTX_20", "RTX_30", "RTX_40", "RTX_50"] {
            let r = known_good_recipe(key).unwrap();
            let s = r["settings"].as_object().unwrap();
            for (k, v) in s {
                assert!(
                    crate::features::MEMORY_OVERRIDE_KEYS.contains(&k.as_str()),
                    "{key}: {k} is not in the Apply allowlist"
                );
                assert!(
                    crate::features::valid_memory_override(k, v),
                    "{key}: {k}={v} would be rejected"
                );
            }
        }
    }

    #[test]
    fn oom_remedies_are_valid_and_writable() {
        for action in ["head_split_medium", "lower_reserved_ram"] {
            let s = oom_remedy_settings(action).unwrap();
            for (k, v) in s.as_object().unwrap() {
                assert!(
                    crate::features::MEMORY_OVERRIDE_KEYS.contains(&k.as_str()),
                    "{action}: {k} not appliable"
                );
                assert!(
                    crate::features::valid_memory_override(k, v),
                    "{action}: {k}={v} rejected"
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
                ("--teacache", Some("0")),
                ("--fp16", None),
            ],
        );
        assert_eq!(m2, "--attention sdpa --profile 4 --teacache 0 --fp16");
    }
}

//! Wan2GP server lifecycle (launch/stop/browser modes).
use tauri::Emitter;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use crate::base::*;
use crate::{hw::get_gpu_info_sync, status::get_active_env};

// Quote-aware split for Extra Launch Args (keeps "--teacache \"a b\"" together).
fn split_launch_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    let mut started = false;
    for ch in s.chars() {
        match ch {
            '"' => { in_q = !in_q; started = true; }
            c if c.is_whitespace() && !in_q => {
                if started { out.push(std::mem::take(&mut cur)); started = false; }
            }
            c => { cur.push(c); started = true; }
        }
    }
    if started { out.push(cur); }
    out
}
#[tauri::command]
pub async fn launch(app: tauri::AppHandle, mode: Option<String>) -> Result<serde_json::Value, String> {
    let mode = mode.unwrap_or("browser".into());
    let repo = get_repo_dir();
    if !repo.join("wgp.py").exists() { return Err("Wan2GP not installed — run Install first".into()); }
    let cfg = load_config_value();
    let port = cfg.get("serverPort").and_then(serde_json::Value::as_u64).unwrap_or(7860);
    // ponytail: if server already listening on :port (desktop→browser switch), reuse it — don't spawn second python on same port (Gradio OSError)
    if std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
        let url = format!("http://localhost:{port}");
        let m = format!("[*] Wan2GP already running on :{port} — opening {url}\n"); crate::base::push_log(&m, "launch"); let _ = app.emit("launch-log", m);
        return Ok(serde_json::json!({"ok": true, "port": port, "mode": mode, "url": url, "fresh": false}));
    }
    mutating_try("launch")?;
    let share = cfg.get("share").and_then(serde_json::Value::as_bool).unwrap_or(false);
    let gpu_device = cfg.get("gpuDevice").and_then(|v| v.as_str()).unwrap_or("auto").trim().to_string();
    let launcher_gpu = cfg.get("launcherGpu").and_then(|v| v.as_str()).unwrap_or("auto").to_string();
    // build args — gpuDevice -> --gpu (mirrors Electron buildCommonLaunchArgs)
    let server_name = cfg.get("serverName").and_then(|v| v.as_str()).unwrap_or("localhost").to_string();
    let mut args = vec!["wgp.py".to_string(), "--server-port".into(), port.to_string(), "--server-name".into(), server_name.clone(), "--advanced".into(), "--multiple-images".into()];
    if share { args.push("--share".into()); }
    if gpu_device != "auto" && gpu_device.starts_with("cuda:") && !args.contains(&"--gpu".to_string()) {
        args.push("--gpu".into()); args.push(gpu_device.clone());
    }
    // Extra Launch Args from Manage tab (quote-aware split, appended last so they win).
    if let Some(extra) = cfg.get("launchArgs").and_then(|v| v.as_str()) {
        let add = split_launch_args(extra);
        if !add.is_empty() { args.extend(add); }
    }
    let emit = |msg: &str| { crate::base::push_log(msg, "launch"); let _ = app.emit("launch-log", msg.to_string()); };
    emit(&format!("[*] Launching Wan2GP ({mode}) on :{port}…\n"));
    // GPU assignment log (mirrors Electron 9945990)
    {
        let hw = get_gpu_info_sync();
        let hw_name = hw.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let hw_vendor = hw.get("vendor").and_then(|v| v.as_str()).unwrap_or("?");
        let hw_vram = hw.get("vramMB").and_then(|v| v.as_str()).unwrap_or("0");
        let gpu_count = silent_command("nvidia-smi").args(["--query-gpu=index","--format=csv,noheader"]).output().ok().map_or("?".into(), |o| if o.status.success() { String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.trim().is_empty()).count().to_string() + " NVIDIA" } else { "?".into() });
        let gen_label = if gpu_device=="auto" { format!("auto ({hw_name} )") } else { gpu_device.clone() };
        emit(&format!("[*] GPU assignment — Launcher UI: {launcher_gpu} | Generation: {gen_label} | HW: {hw_name} ({hw_vendor}, {hw_vram}) | Detected: {gpu_count}\n"));
    }
    emit(&format!("[*] Args: {}\n", args.join(" ")));
    // HF_TOKEN / config env (mirrors Electron launchCfg)
    let launch_cfg = load_config_value();
    let hf_token = launch_cfg.get("hfToken").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let claude_key = launch_cfg.get("claudeApiKey").and_then(|v| v.as_str()).unwrap_or("").to_string();
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
    emit(&format!("[*] Python: {py}\n"));
    emit(&format!("[*] Port: {port}\n"));
    use tauri_plugin_shell::ShellExt;
    // ponytail: PYTHONUNBUFFERED for streaming logs (tqdm), plus HF_TOKEN/claude key
    let mut cmd = app.shell().command(&py);
    cmd = cmd.args(&args).current_dir(&repo);
    // shell plugin env() — if not available, fallback to std env (child inherits)
    // tauri-plugin-shell 2.x supports .env() — use it when available
    #[allow(unused_mut)]
    let mut cmd = cmd;
    // set env via std::env for child inheritance as fallback
    // (shell plugin also inherits process env, so set temporarily)
    if !hf_token.is_empty() { std::env::set_var("HF_TOKEN", &hf_token); }
    if !claude_key.is_empty() { std::env::set_var("ANTHROPIC_API_KEY", &claude_key); }
    // GGUF CUDA kernel knobs from Manage → GGUF CUDA Kernel (docs/INSTALLATION.md parity).
    // std::env persists in OUR process across launches, so always reconcile:
    // set what's configured, REMOVE stale leftovers.
    {
        let g = load_config_value().get("ggufEnv").cloned().unwrap_or(serde_json::Value::Null);
        let enabled = g.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
        if !enabled {
            std::env::set_var("WGP_GGUF_LLAMACPP_CUDA", "0");
        } else {
            std::env::remove_var("WGP_GGUF_LLAMACPP_CUDA");
            match g.get("matmulMode").and_then(|v| v.as_str()).unwrap_or("auto") {
                "fast" | "low_vram" => std::env::set_var("WGP_GGUF_LLAMACPP_CUDA_MATMUL_MODE", g["matmulMode"].as_str().unwrap()),
                _ => std::env::remove_var("WGP_GGUF_LLAMACPP_CUDA_MATMUL_MODE"),
            }
            if g.get("streamK").and_then(|v| v.as_bool()) == Some(false) { std::env::set_var("WGP_GGUF_LLAMACPP_CUDA_STREAM_K", "0"); }
            else { std::env::remove_var("WGP_GGUF_LLAMACPP_CUDA_STREAM_K"); }
            if g.get("bf16Fp16").and_then(|v| v.as_bool()) == Some(true) { std::env::set_var("WGP_GGUF_LLAMACPP_CUDA_BF16_FP16", "1"); }
            else { std::env::remove_var("WGP_GGUF_LLAMACPP_CUDA_BF16_FP16"); }
        }
        emit(&format!("[i] GGUF env: CUDA={} MATMUL={} STREAM_K={} BF16_FP16={}\n",
            std::env::var("WGP_GGUF_LLAMACPP_CUDA").unwrap_or("1".into()),
            std::env::var("WGP_GGUF_LLAMACPP_CUDA_MATMUL_MODE").unwrap_or("auto".into()),
            std::env::var("WGP_GGUF_LLAMACPP_CUDA_STREAM_K").unwrap_or("1".into()),
            std::env::var("WGP_GGUF_LLAMACPP_CUDA_BF16_FP16").unwrap_or("0".into())));
    }
    std::env::set_var("PYTHONUNBUFFERED", "1");
    std::env::set_var("PYTHONUTF8", "1");
    std::env::set_var("PYTHONIOENCODING", "utf-8");
    let (rx, child) = cmd.spawn().map_err(|e| { mutating_done(); emit(&format!("[LAUNCH ERROR] spawn failed: {e}\n")); e.to_string() })?;
    emit(&format!("[*] Spawned PID {}\n", child.pid()));
    if let Ok(m) = WANGP_PID.get_or_init(|| Mutex::new(None)).lock() { drop(m); }
    *WANGP_PID.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(child.pid());
    // stream logs in background
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        use tauri_plugin_shell::process::CommandEvent;
        let mut rx = rx;
        while let Some(ev) = rx.recv().await {
            match ev { CommandEvent::Stdout(b) => { let s = String::from_utf8_lossy(&b).to_string(); crate::base::push_log(&s, "launch"); let _ = app2.emit("launch-log", s); }, CommandEvent::Stderr(b) => { let s = String::from_utf8_lossy(&b).to_string(); crate::base::push_log(&s, "launch"); let _ = app2.emit("launch-log", s); }, CommandEvent::Terminated(s) => { let _ = app2.emit("wangp-exit", serde_json::json!({"code": s.code})); break; }, _ => {} }
        }
    });
    // wait for port in background (don't hold mutating — launch is done, server boots async)
    let host = "127.0.0.1".to_string();
    let app3 = app.clone();
    std::thread::spawn(move || {
        for _ in 0..60 { std::thread::sleep(std::time::Duration::from_secs(3)); if std::net::TcpStream::connect(format!("{host}:{port}")).is_ok() { let m = format!("[✓] Wan2GP ready on http://localhost:{port}\n"); crate::base::push_log(&m, "launch"); let _ = app3.emit("launch-log", m); break; } }
    });
    mutating_done();
    let url = format!("http://localhost:{port}");
    Ok(serde_json::json!({"ok": true, "port": port, "mode": mode, "url": url, "fresh": true}))
}
#[tauri::command]
pub fn stop_wangp(app: tauri::AppHandle) -> serde_json::Value {
    // robust stop: kill stored PID + any PID listening on :7861 (handles shim/child split + stale port)
    let mut killed: Vec<u32> = Vec::new();
    if let Some(pid) = WANGP_PID.get().and_then(|m| m.lock().ok()).and_then(|g| *g) {
        killed.push(pid);
        #[cfg(windows)] { let _ = silent_command("taskkill").args(["/pid", &pid.to_string(), "/f", "/t"]).output(); }
        #[cfg(not(windows))] { let _ = silent_command("kill").arg("-9").arg(pid.to_string()).output(); }
    }
    // find actual LISTENING PID on :7861 via netstat (handles uv shim -> cpython child)
    #[cfg(windows)]
    {
        if let Ok(out) = silent_command("cmd").args(["/C", "netstat -ano | findstr LISTENING | findstr :7861"]).output() {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout);
                for line in s.lines() {
                    if let Some(pid_str) = line.split_whitespace().last() {
                        if let Ok(pid) = pid_str.parse::<u32>() {
                            if !killed.contains(&pid) {
                                let _ = silent_command("taskkill").args(["/pid", &pid.to_string(), "/f", "/t"]).output();
                                killed.push(pid);
                            }
                        }
                    }
                }
            }
        }
        // fallback: kill any python wgp.py
        let _ = silent_command("taskkill").args(["/F", "/IM", "python.exe", "/T"]).output();
    }
    if let Some(m) = WANGP_PID.get() { *m.lock().unwrap() = None; }
    let _ = app.emit("wangp-exit", serde_json::json!({"stopped": true, "killed": killed}));
    // wait a bit for port to free (FIN_WAIT -> TIME_WAIT -> CLOSED)
    std::thread::sleep(std::time::Duration::from_millis(800));
    serde_json::json!({"ok": true, "killed": killed})
}

// ── misc stubs to unblock frontend (return safe defaults) ──
#[tauri::command] pub fn open_external(url: Option<String>) { let _=url; }
#[tauri::command] pub fn detect_browsers() -> serde_json::Value {
    // mirrors Electron WELL_KNOWN_BROWSERS with win env expansion
    let cfg = load_config_value(); let def = cfg.get("defaultBrowser").and_then(|v| v.as_str()).unwrap_or("system").to_string();
    let expand = |p: &str| {
        let mut s = p.to_string();
        for (k,v) in std::env::vars() { s = s.replace(&format!("%{k}%"), &v); s = s.replace(&format!("%{}%", k.to_lowercase()), &v); }
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
#[tauri::command] pub fn launch_browser(app: tauri::AppHandle, url: Option<String>) -> serde_json::Value {
    use tauri_plugin_opener::OpenerExt;
    let u = url.unwrap_or_else(|| "http://localhost:7861".into());
    if !(u.starts_with("http://") || u.starts_with("https://")) {
        return serde_json::json!({"ok": false, "success": false, "error": "invalid url"});
    }
    let chosen = load_config_value().get("defaultBrowser").and_then(|v| v.as_str()).unwrap_or("system").to_string();
    // "system" (or anything unresolved) → OS default via opener.
    let exe = if chosen == "system" { None } else { find_browser_exe(&chosen) };
    match exe {
        None => match app.opener().open_url(u, None::<String>) {
            Ok(()) => serde_json::json!({"ok": true, "success": true, "via": "system"}),
            Err(e) => serde_json::json!({"ok": false, "success": false, "error": e.to_string()}),
        },
        Some(path) => match silent_command(&path).arg(&u).spawn() {
            Ok(_) => serde_json::json!({"ok": true, "success": true, "via": chosen}),
            Err(e) => serde_json::json!({"ok": false, "success": false, "error": e.to_string()}),
        },
    }
}
// Resolve a known browser id to its exe (same candidates as detect_browsers).
fn find_browser_exe(id: &str) -> Option<String> {
    let cands: &[&str] = match id {
        "chrome" => &["%ProgramFiles%\\Google\\Chrome\\Application\\chrome.exe", "%ProgramFiles(x86)%\\Google\\Chrome\\Application\\chrome.exe", "%LocalAppData%\\Google\\Chrome\\Application\\chrome.exe"],
        "edge" => &["%ProgramFiles%\\Microsoft\\Edge\\Application\\msedge.exe", "%ProgramFiles(x86)%\\Microsoft\\Edge\\Application\\msedge.exe"],
        "firefox" => &["%ProgramFiles%\\Mozilla Firefox\\firefox.exe", "%ProgramFiles(x86)%\\Mozilla Firefox\\firefox.exe"],
        "brave" => &["%LocalAppData%\\BraveSoftware\\Brave-Browser\\Application\\brave.exe"],
        "opera" => &["%LocalAppData%\\Programs\\Opera\\launcher.exe"],
        "vivaldi" => &["%LocalAppData%\\Vivaldi\\Application\\vivaldi.exe"],
        _ => return None,
    };
    for c in cands {
        let mut s = c.to_string();
        for (k, v) in std::env::vars() {
            s = s.replace(&format!("%{k}%"), &v);
            s = s.replace(&format!("%{}%", k.to_lowercase()), &v);
        }
        if let Ok(pf86) = std::env::var("ProgramFiles(x86)") { s = s.replace("%ProgramFiles(x86)%", &pf86); }
        if std::path::Path::new(&s).exists() { return Some(s); }
    }
    None
}
#[tauri::command] pub fn launch_browser_no_gpu(url: Option<String>) -> serde_json::Value {
    // No-GPU browser frees VRAM for generation (mirrors Electron's chrome flags).
    let u = url.unwrap_or_else(|| "http://localhost:7861".into());
    if !(u.starts_with("http://") || u.starts_with("https://")) {
        return serde_json::json!({"ok": false, "success": false, "error": "invalid url"});
    }
    let chosen = load_config_value().get("defaultBrowser").and_then(|v| v.as_str()).unwrap_or("system").to_string();
    // Prefer Chrome, else the chosen browser, else whatever opener gives (GPU on).
    let exe = find_browser_exe("chrome")
        .or_else(|| if chosen != "system" { find_browser_exe(&chosen) } else { None });
    let Some(path) = exe else {
        return serde_json::json!({"ok": false, "success": false, "error": "No Chromium browser found for no-GPU launch"});
    };
    let args = ["--disable-gpu", "--disable-gpu-compositing", "--disable-accelerated-2d-canvas", "--disable-accelerated-video-decode", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-webgpu"];
    match silent_command(&path).args(args).arg(&u).spawn() {
        Ok(_) => serde_json::json!({"ok": true, "success": true}),
        Err(e) => serde_json::json!({"ok": false, "success": false, "error": e.to_string()}),
    }
}
#[tauri::command] pub fn chrome_available() -> bool {
    // ponytail: where chrome only checks PATH, but Chrome is at Program Files — check there like detect_browsers does
    for p in ["C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe", "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe"] {
        if std::path::Path::new(p).exists() { return true; }
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        if std::path::Path::new(&format!("{local}\\Google\\Chrome\\Application\\chrome.exe")).exists() { return true; }
    }
    silent_command("where").arg("chrome").output().is_ok_and(|o| o.status.success())
}

#[tauri::command] pub async fn launch_webview(app: tauri::AppHandle) -> Result<serde_json::Value, String> { launch(app, Some("app".into())).await }
#[tauri::command] pub async fn popout_webview(app: tauri::AppHandle, url: Option<String>) -> Result<serde_json::Value, String> {
    let res = launch(app.clone(), Some("browser".into())).await?;
    let u = url.or_else(|| res.get("url").and_then(|v| v.as_str()).map(std::string::ToString::to_string)).unwrap_or_else(|| "http://localhost:7861".into());
    use tauri_plugin_opener::OpenerExt; let _ = app.opener().open_url(u, None::<String>);
    Ok(res)
}

#[cfg(test)]
mod launch_args_tests {
    use super::*;
    #[test]
    fn split_launch_args_quotes() {
        assert_eq!(split_launch_args("--profile 4 --attention sage2"), vec!["--profile", "4", "--attention", "sage2"]);
        assert_eq!(split_launch_args("--teacache \"a b\" --verbose 2"), vec!["--teacache", "a b", "--verbose", "2"]);
        assert_eq!(split_launch_args(""), Vec::<String>::new());
    }
}

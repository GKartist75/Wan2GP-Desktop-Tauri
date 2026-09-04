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
static TERMINAL_TITLE: std::sync::OnceLock<std::sync::Mutex<Option<String>>> = std::sync::OnceLock::new();
pub(crate) fn terminal_title() -> Option<String> {
    TERMINAL_TITLE.get().and_then(|m| m.lock().ok()).and_then(|g| g.clone())
}
// External-terminal mode (run.bat style): generate a script that runs wgp.py
// with the same args/env, open it in a VISIBLE console window, wait for the
// server, open the browser. Not a streamed child — the user owns the window.
fn launch_in_terminal(app: tauri::AppHandle, repo: &PathBuf, py: &str, args: &[String], port: u64, _cfg: &serde_json::Value, hf_token: String, claude_key: String) -> Result<serde_json::Value, String> {
    let emit = |msg: &str| { crate::base::push_log(msg, "launch"); let _ = app.emit("launch-log", msg.to_string()); };
    let title = format!("Wan2GP-Launcher-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0));
    if let Ok(mut g) = TERMINAL_TITLE.get_or_init(|| std::sync::Mutex::new(None)).lock() { *g = Some(title.clone()); }
    // env lines for the script (tokens + GGUF knobs, same as hidden launch)
    let mut env_lines: Vec<String> = vec![
        "set PYTHONIOENCODING=utf-8".into(), "set PYTHONUTF8=1".into(), "set PYTHONUNBUFFERED=1".into(),
        "set TQDM_MININTERVAL=0".into(), "set TQDM_MINITERS=1".into(), "set NO_PROXY=localhost,127.0.0.1,::1".into(),
    ];
    if !hf_token.is_empty() { env_lines.push(format!("set HF_TOKEN={hf_token}")); }
    if !claude_key.is_empty() { env_lines.push(format!("set ANTHROPIC_API_KEY={claude_key}"));
    }
    for k in ["WGP_GGUF_LLAMACPP_CUDA", "WGP_GGUF_LLAMACPP_CUDA_MATMUL_MODE", "WGP_GGUF_LLAMACPP_CUDA_STREAM_K", "WGP_GGUF_LLAMACPP_CUDA_BF16_FP16"] {
        if let Ok(val) = std::env::var(k) { env_lines.push(format!("set {k}={val}")); }
    }
    let arg_str = args.iter().map(|a| if a.contains(' ') { format!("\"{}\"", a.replace('%', "%%")) } else { a.replace('%', "%%") }).collect::<Vec<_>>().join(" ");
    #[cfg(windows)] {
        let script = std::env::temp_dir().join("wan2gp-terminal.bat");
        let url = format!("http://localhost:{port}");
        let full = format!("@echo off\r\ntitle {title}\r\ncd /d \"{repo}\"\r\n{envs}\r\necho [Wan2GP Desktop Launcher] Starting on port {port}...\r\nstart /b \"\" cmd /c \"\"{py}\" -u wgp.py {arg_str}\" 2>&1\r\necho Waiting for server on port {port}...\r\nset RC=0\r\n:waitloop\r\ntimeout /t 2 /nobreak >nul\r\nset /a RC+=1\r\nif %RC% gtr 60 (echo Server failed to start. Check console. ^& pause ^& exit /b 1)\r\npowershell -Command \"try{{$(Invoke-WebRequest -Uri http://127.0.0.1:{port}/config -TimeoutSec 2 -UseBasicParsing).StatusCode -eq 200;exit 0}}catch{{exit 1}}\" >nul 2>&1 && goto ready\r\ngoto waitloop\r\n:ready\r\necho Wan2GP is ready! Opening browser...\r\nstart {url}\r\necho [Wan2GP] Server running. Close this window to stop it.\r\npause >nul\r\n",
            repo = repo.display(), envs = env_lines.join("\r\n"));
        std::fs::write(&script, full).map_err(|e| { mutating_done(); e.to_string() })?;
        emit(&format!("[*] Starting Wan2GP in external terminal…\n"));
        // visible window: wt.exe preferred, else cmd /K (NOT silent — user must see it)
        let has_wt = std::process::Command::new("where").arg("wt.exe").output().is_ok_and(|o| o.status.success());
        let spawned = if has_wt {
            std::process::Command::new("wt.exe").args(["-w", "-1", "new-tab", "--title", &title, "cmd.exe", "/K", &script.to_string_lossy().to_string()]).spawn()
        } else {
            std::process::Command::new("cmd.exe").args(["/C", "start", &title, "cmd", "/K", &script.to_string_lossy().to_string()]).spawn()
        };
        if let Err(e) = spawned { mutating_done(); return Err(format!("Could not open terminal: {e}")); }
        mutating_done();
        return Ok(serde_json::json!({"ok": true, "port": port, "mode": "terminal", "url": url, "fresh": true}));
    }
    #[cfg(not(windows))] {
        let _ = (cfg, hf_token, claude_key);
        mutating_done();
        return Err("External terminal mode is Windows-only in this build".into());
    }
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
    // Bootstrap shim (Electron parity): lies isatty()=True so tqdm +
    // huggingface_hub bars render even though stdout is piped, not a tty.
    // Fresh temp file per launch (never repo-local): %TEMP% cleaners or a
    // stale copy used to break launches with cryptic errors until restart.
    // ponytail: Electron's z-image VAE monkeypatch deliberately NOT ported — crash fix, separate issue.
    let boot = {
        let tmp = std::env::temp_dir();
        for stale in std::fs::read_dir(&tmp).into_iter().flatten().flatten() {
            let n = stale.file_name().to_string_lossy().to_string();
            if n.starts_with("wan2gp-bootstrap-") && n.ends_with(".py") { let _ = std::fs::remove_file(stale.path()); }
        }
        tmp.join(format!("wan2gp-bootstrap-{}-{}.py", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)))
    };
    let _ = std::fs::write(&boot, r#"import os, sys, runpy
os.environ['PYTHONUNBUFFERED'] = '1'
os.environ['TQDM_MININTERVAL'] = '0'
os.environ['TQDM_MINITERS'] = '1'
os.environ['HF_HUB_DISABLE_PROGRESS_BARS'] = '0'
os.environ['HF_HUB_ENABLE_HF_TRANSFER'] = '0'
os.environ.setdefault('TERM', 'xterm-256color')
class _Tty:
    def __init__(self, inner): self._inner = inner
    def isatty(self): return True
    def fileno(self):
        try: return self._inner.fileno()
        except OSError: raise
    def __getattr__(self, n): return getattr(self._inner, n)
sys.stdout = _Tty(sys.stdout)
sys.stderr = _Tty(sys.stderr)
sys.__stdout__ = sys.stdout
sys.__stderr__ = sys.stderr
print('[bootstrap] active', flush=True)
sys.argv = sys.argv[1:]
d = os.path.dirname(os.path.abspath(sys.argv[0]))
if d not in sys.path: sys.path.insert(0, d)
runpy.run_path(sys.argv[0], run_name='__main__')
"#);
    args.insert(0, boot.to_string_lossy().to_string()); // py <boot> wgp.py … (target = argv[1])
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
    if !hf_token.is_empty() { std::env::set_var("HF_TOKEN", &hf_token); std::env::set_var("HUGGINGFACE_HUB_TOKEN", &hf_token); }
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
    // Live progress bars (mirrors Electron terminal env): redraw every iteration.
    std::env::set_var("TQDM_MININTERVAL", "0");
    std::env::set_var("TQDM_MINITERS", "1");
    // Bars on for piped output; classic hf_hub path (hf_transfer has its own non-tqdm progress).
    std::env::set_var("HF_HUB_DISABLE_PROGRESS_BARS", "0");
    std::env::set_var("HF_HUB_ENABLE_HF_TRANSFER", "0");
    std::env::set_var("NO_PROXY", "localhost,127.0.0.1,::1");
    // External-terminal mode: visible console window running wgp.py (run.bat style).
    // Not a child we stream — the user owns the window; Stop also kills by title.
    if mode == "terminal" {
        return launch_in_terminal(app, &repo, &py, &args, port, &cfg, hf_token.clone(), claude_key.clone());
    }
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
    // Scoped stop: ONLY our Wan2GP processes — the tracked child plus any python
    // running OUR repo's wgp.py (uv-shim/child split, detached terminal mode).
    // ponytail: the old `taskkill /F /IM python.exe` blanket-killed every Python
    // on the machine (user scripts, other apps) — never again.
    let repo = get_repo_dir();
    let repo_s = repo.to_string_lossy().replace('/', "\\").to_lowercase();
    let mut killed: Vec<u32> = Vec::new();
    let mut kill_pid = |pid: u32| {
        if pid == 0 || killed.contains(&pid) { return; }
        #[cfg(windows)] { let _ = silent_command("taskkill").args(["/pid", &pid.to_string(), "/f", "/t"]).output(); }
        #[cfg(not(windows))] { let _ = silent_command("kill").arg("-9").arg(pid.to_string()).output(); }
        killed.push(pid);
    };
    if let Some(pid) = WANGP_PID.get().and_then(|m| m.lock().ok()).and_then(|g| *g) {
        kill_pid(pid);
    }
    // our wgp.py by command line (covers shim→child + terminal children, any port)
    #[cfg(windows)]
    {
        let ps = "Get-CimInstance Win32_Process -Filter \"Name='python.exe'\" | Where-Object { $_.CommandLine -like '*wgp.py*' } | ForEach-Object { $_.ProcessId + '|' + $_.CommandLine }";
        if let Ok(out) = silent_command("powershell").args(["-NoProfile", "-Command", ps]).output() {
            if out.status.success() {
                for line in String::from_utf8_lossy(&out.stdout).lines() {
                    let mut parts = line.splitn(2, '|');
                    if let (Some(pid_s), Some(cmd)) = (parts.next(), parts.next()) {
                        if let Ok(pid) = pid_s.trim().parse::<u32>() {
                            if cmd.to_lowercase().replace('/', "\\").contains(&repo_s) { kill_pid(pid); }
                        }
                    }
                }
            }
        }
        // our external-terminal window (unique timestamped title)
        if let Some(t) = crate::launch::terminal_title() {
            let _ = silent_command("taskkill").args(["/F", "/FI", &format!("WINDOWTITLE eq {t}*")]).output();
        }
    }
    #[cfg(not(windows))]
    {
        let pat = repo.join("wgp.py").to_string_lossy().to_string();
        if let Ok(out) = silent_command("pgrep").args(["-f", &pat]).output() {
            if out.status.success() {
                for line in String::from_utf8_lossy(&out.stdout).lines() {
                    if let Ok(pid) = line.trim().parse::<u32>() { kill_pid(pid); }
                }
            }
        }
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

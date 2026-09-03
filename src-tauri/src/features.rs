//! Packages, memory profiles, auto-tune, Deepy, LLM engines, notifier, settings.
use tauri::Emitter;
use std::path::PathBuf;
use crate::base::*;
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;
use crate::{hw::get_gpu_info_sync, status::get_active_env};

#[tauri::command]
pub async fn check_package_updates(app: tauri::AppHandle, versions: Option<serde_json::Value>) -> Result<serde_json::Value,String> {
    let _ = versions;
    let env = get_active_env();
    let raw = env.get("path").and_then(|p| p.as_str()).unwrap_or("");
    let r = raw.trim_start_matches(['.', '\\', '/']);
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
pub fn check_package(pkg: String) -> serde_json::Value {
    // Real probe: importlib version from the active env (aliases map import names to dist names).
    let dist = match pkg.as_str() {
        "triton" => "triton-windows",
        "spas_sage_attn" => "spas-sage-attn",
        "huggingface_hub" => "huggingface-hub",
        "opencv" | "opencv-python" => "opencv-python",
        other => other,
    };
    let env = get_active_env();
    let py = env.get("path").and_then(|p| p.as_str()).map(|raw| {
        let base = if std::path::Path::new(raw).is_absolute() { PathBuf::from(raw) } else { get_repo_dir().join(raw.trim_start_matches(".\\").trim_start_matches("./")) };
        if cfg!(windows) { base.join("Scripts\\python.exe") } else { base.join("bin/python") }
    });
    if let Some(p) = py {
        if p.exists() {
            let code = format!("import importlib.metadata as m; print(m.version({dist:?}))");
            if let Ok(o) = silent_command(&p).args(["-c", &code]).output() {
                if o.status.success() {
                    let v = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if !v.is_empty() {
                        return serde_json::json!({"name": pkg, "installed": true, "version": v});
                    }
                }
            }
        }
    }
    serde_json::json!({"name": pkg, "installed": false, "version": null})
}
#[tauri::command]
pub fn memory_profile_read() -> serde_json::Value {
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
pub fn auto_tune_detect() -> serde_json::Value {
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
            silent_command("powershell").args(["-NoProfile","-Command","[math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory/1GB,1)"]).output()
                .ok().and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().ok()).unwrap_or(32.0)
        }
        #[cfg(not(windows))] { 32.0 }
    };
    let cpu_count = std::thread::available_parallelism().map_or(8, std::num::NonZero::get) as i64;
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
pub fn auto_tune_recommend(hw: Option<serde_json::Value>, opts: Option<serde_json::Value>) -> serde_json::Value {
    let hw = hw.unwrap_or(serde_json::json!({"vram_tier":"low","ram_tier":"low","gpu_vram_gb":10}));
    let vram_tier = hw.get("vram_tier").and_then(|v| v.as_str()).unwrap_or("low");
    let ram_tier = hw.get("ram_tier").and_then(|v| v.as_str()).unwrap_or("low");
    let vram_gb = hw.get("gpu_vram_gb").and_then(serde_json::Value::as_f64).unwrap_or(10.0);
    let failsafe = opts.as_ref().and_then(|o| o.get("failsafe")).and_then(serde_json::Value::as_bool).unwrap_or(false);
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
pub async fn upgrade_package(app: tauri::AppHandle, pkg: String) -> Result<serde_json::Value,String> {
    let env = get_active_env(); let raw = env.get("path").and_then(|p| p.as_str()).unwrap_or(""); let base = if std::path::Path::new(raw).is_absolute() { PathBuf::from(raw) } else { get_repo_dir().join(raw.trim_start_matches(".\\").trim_start_matches("./")) };
    let py = if cfg!(windows) { base.join("Scripts\\python.exe") } else { base.join("bin/python") };
    if !py.exists() { return Err("python not found".into()); }
    let (mut rx, _) = app.shell().command(&py).args(["-m","pip","install","--upgrade", &pkg]).spawn().map_err(|e| e.to_string())?;
    while let Some(ev) = rx.recv().await { match ev { CommandEvent::Stdout(b)|CommandEvent::Stderr(b) => { let _ = app.emit("launch-log", String::from_utf8_lossy(&b).to_string()); }, _=>{} } }
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command]
pub async fn install_package(app: tauri::AppHandle, pkg: String) -> Result<serde_json::Value,String> {
    let env = get_active_env(); let raw = env.get("path").and_then(|p| p.as_str()).unwrap_or(""); let base = if std::path::Path::new(raw).is_absolute() { PathBuf::from(raw) } else { get_repo_dir().join(raw.trim_start_matches(".\\").trim_start_matches("./")) };
    let py = if cfg!(windows) { base.join("Scripts\\python.exe") } else { base.join("bin/python") };
    if !py.exists() { return Err("python not found".into()); }
    let (mut rx, _) = app.shell().command(&py).args(["-m","pip","install", &pkg]).spawn().map_err(|e| e.to_string())?;
    while let Some(ev) = rx.recv().await { match ev { CommandEvent::Stdout(b)|CommandEvent::Stderr(b) => { let _ = app.emit("launch-log", String::from_utf8_lossy(&b).to_string()); }, _=>{} } }
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command]
pub async fn uninstall_package(app: tauri::AppHandle, pkg: String) -> Result<serde_json::Value,String> {
    let env = get_active_env(); let raw = env.get("path").and_then(|p| p.as_str()).unwrap_or(""); let base = if std::path::Path::new(raw).is_absolute() { PathBuf::from(raw) } else { get_repo_dir().join(raw.trim_start_matches(".\\").trim_start_matches("./")) };
    let py = if cfg!(windows) { base.join("Scripts\\python.exe") } else { base.join("bin/python") };
    if !py.exists() { return Err("python not found".into()); }
    let (mut rx, _) = app.shell().command(&py).args(["-m","pip","uninstall","-y", &pkg]).spawn().map_err(|e| e.to_string())?;
    while let Some(ev) = rx.recv().await { match ev { CommandEvent::Stdout(b)|CommandEvent::Stderr(b) => { let _ = app.emit("launch-log", String::from_utf8_lossy(&b).to_string()); }, _=>{} } }
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command]
pub async fn restore_requirements(app: tauri::AppHandle) -> Result<serde_json::Value,String> {
    let repo = get_repo_dir(); let env = get_active_env(); let raw = env.get("path").and_then(|p| p.as_str()).unwrap_or(""); let base = if std::path::Path::new(raw).is_absolute() { PathBuf::from(raw) } else { repo.join(raw.trim_start_matches(".\\").trim_start_matches("./")) };
    let py = if cfg!(windows) { base.join("Scripts\\python.exe") } else { base.join("bin/python") };
    if !py.exists() { return Err("python not found".into()); }
    let (mut rx, _) = app.shell().command(&py).args(["-m","pip","install","-r", "requirements.txt"]).current_dir(&repo).spawn().map_err(|e| e.to_string())?;
    while let Some(ev) = rx.recv().await { match ev { CommandEvent::Stdout(b)|CommandEvent::Stderr(b) => { let _ = app.emit("launch-log", String::from_utf8_lossy(&b).to_string()); }, _=>{} } }
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command] pub fn llm_engines_list() -> serde_json::Value {
    // ponytail: probe cliOnPath + pipInstalled like Electron services/llm-engines.js
    let env = get_active_env();
    let py = env.get("path").and_then(|p| p.as_str()).map(|raw| {
        let base = if std::path::Path::new(raw).is_absolute() { PathBuf::from(raw) } else { get_repo_dir().join(raw.trim_start_matches(".\\").trim_start_matches("./")) };
        if cfg!(windows) { base.join("Scripts\\python.exe") } else { base.join("bin/python") }
    });
    let check_cli = |cli: &str| -> bool {
        #[cfg(windows)] { silent_command("where").arg(cli).output().is_ok_and(|o| o.status.success()) }
        #[cfg(not(windows))] { silent_command("which").arg(cli).output().map(|o| o.status.success()).unwrap_or(false) }
    };
    // Reuse the get_status version scan (same refresh already ran it — cache is warm)
    // instead of spawning a second cold venv python just for pip show.
    let cached_sdk = crate::base::LAST_STATUS.get()
        .and_then(|m| m.lock().ok())
        .and_then(|g| g.as_ref()
            .and_then(|(_, _, v)| v.get("versions"))
            .and_then(|vs| vs.get("claude-agent-sdk"))
            .and_then(|x| x.as_str())
            .map(|s| !s.is_empty()))
        .unwrap_or(false);
    let check_pip = |pkg: &str| -> bool {
        if pkg == "claude-agent-sdk" && cached_sdk { return true; }
        if let Some(p) = &py { if p.exists() { return silent_command(p).args(["-m","pip","show",pkg]).output().is_ok_and(|o| o.status.success()); } }
        false
    };
    let engines = vec![
        serde_json::json!({"id":"claude-code","label":"Claude Code","desc":"Anthropic Claude Code CLI + Python bridge","cli":"claude","cliOnPath": check_cli("claude"), "pipPackage":"claude_agent_sdk","pipInstalled": check_pip("claude-agent-sdk"), "external": false}),
        serde_json::json!({"id":"codex","label":"OpenAI Codex","desc":"OpenAI Codex CLI (npm)","cli":"codex","cliOnPath": check_cli("codex"), "pipPackage":null,"pipInstalled":null,"external": true}),
        serde_json::json!({"id":"opencode","label":"OpenCode","desc":"Universal-provider agent","cli":"opencode","cliOnPath": check_cli("opencode"), "pipPackage":null,"pipInstalled":null,"external": true, "serverUrl":"http://127.0.0.1:4096"}),
    ];
    serde_json::json!({"ok": true, "engines": engines, "hasActiveEnv": !env.is_null()})
}
// One-click engine installer: claude-code via env pip (pinned), codex/opencode via npm -g.
#[tauri::command] pub fn llm_engine_install(engine: String) -> serde_json::Value {
    let spec = match engine.as_str() { "claude-code" => "claude-agent-sdk==0.1.40", "codex" => "@openai/codex", "opencode" => "opencode-ai", _ => return serde_json::json!({"ok": false, "success": false, "error": "Unknown engine"}) };
    let res = match engine.as_str() {
        "claude-code" => match env_python_bin() {
            None => return serde_json::json!({"ok": false, "success": false, "error": "No active Python environment"}),
            Some(py) => silent_command(&py).args(["-m", "pip", "install", spec]).output(),
        },
        _ => silent_command("npm").args(["install", "-g", spec]).output(),
    };
    match res {
        Ok(o) if o.status.success() => serde_json::json!({"ok": true, "success": true, "spec": spec}),
        Ok(o) => serde_json::json!({"ok": false, "success": false, "error": format!("install failed: {}", String::from_utf8_lossy(&o.stderr).trim())}),
        Err(e) => serde_json::json!({"ok": false, "success": false, "error": e.to_string()}),
    }
}
// OpenCode server lifecycle (Wan2GP auto-starts it too; this is the manual toggle).
// PID-tracked so Start/Stop is honest; other engines have no servable process.
static OPENCODE_PID: std::sync::OnceLock<std::sync::Mutex<Option<u32>>> = std::sync::OnceLock::new();
#[tauri::command] pub fn llm_engine_serve(engine: String, action: String) -> serde_json::Value {
    if engine != "opencode" {
        return serde_json::json!({"ok": false, "success": false, "error": "Only OpenCode has a local server"});
    }
    if action == "stop" {
        let pid = OPENCODE_PID.get().and_then(|m| m.lock().ok()).and_then(|mut g| g.take());
        if let Some(pid) = pid {
            #[cfg(windows)] { let _ = silent_command("taskkill").args(["/F", "/PID", &pid.to_string()]).output(); }
            #[cfg(not(windows))] { let _ = silent_command("kill").arg(pid.to_string()).output(); }
        }
        return serde_json::json!({"ok": true, "success": true, "running": false});
    }
    // start: already up (ours or Wan2GP's) → report running instead of double-spawning
    if std::net::TcpStream::connect("127.0.0.1:4096").is_ok() {
        return serde_json::json!({"ok": true, "success": true, "running": true, "url": "http://127.0.0.1:4096"});
    }
    match silent_command("opencode").args(["serve", "--hostname", "127.0.0.1", "--port", "4096"]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn() {
        Ok(child) => {
            if let Ok(mut g) = OPENCODE_PID.get_or_init(|| std::sync::Mutex::new(None)).lock() { *g = Some(child.id()); }
            std::mem::forget(child); // detach: lives beyond this command
            serde_json::json!({"ok": true, "success": true, "running": true, "url": "http://127.0.0.1:4096"})
        }
        Err(e) => serde_json::json!({"ok": false, "success": false, "error": format!("Could not start opencode (is it installed?): {e}")}),
    }
}
#[tauri::command] pub fn llm_engine_auth(engine: String) -> serde_json::Value {
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
#[tauri::command] pub fn deepy_status() -> serde_json::Value {
    let p = get_repo_dir().join("wgp_config.json");
    if !p.exists() { return serde_json::json!({"ok": true, "available": false, "reason": "wgp_config.json not found — install Wan2GP first."}); }
    let Ok(s) = std::fs::read_to_string(&p) else { return serde_json::json!({"ok": false, "error": "cannot read wgp_config.json"}); };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else { return serde_json::json!({"ok": false, "error": "wgp_config.json corrupted"}); };
    let enabled = v.get("deepy_enabled").and_then(serde_json::Value::as_i64).unwrap_or(0);
    let dtype = v.get("deepy_type").and_then(|x| x.as_str()).unwrap_or("zero");
    let mode = if enabled==0 { "disabled" } else if dtype=="prime" { "prime" } else { "zero" };
    let enh = v.get("enhancer_enabled").and_then(serde_json::Value::as_i64);
    let le = v.get("llm_engines");
    let cur_engine = le.and_then(|x| x.get("deepy")).and_then(|x| x.as_str()).map(std::string::ToString::to_string).unwrap_or_default();
    let prompt_enh = le.and_then(|x| x.get("prompt_enhancer")).and_then(|x| x.as_str()).map(std::string::ToString::to_string);
    let engines: Vec<String> = le.and_then(|x| x.get("profiles")).and_then(|x| x.as_object()).map(|o| o.keys().cloned().collect()).unwrap_or_default();
    serde_json::json!({"ok": true, "available": true, "mode": mode, "deepyEnabled": enabled!=0, "deepyType": dtype, "currentEngine": if cur_engine.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(cur_engine) }, "promptEnhancer": prompt_enh, "enhancerEnabled": enh, "engines": engines})
}
#[tauri::command] pub fn deepy_set(mode: String, engine: Option<String>, enhancer: Option<serde_json::Value>) -> serde_json::Value {
    eprintln!("[deepy_set] mode={mode} engine={engine:?} enhancer={enhancer:?}");
    let m = mode.trim().to_lowercase();
    if !["disabled","zero","prime"].contains(&m.as_str()) { return serde_json::json!({"ok": false, "error": format!("Unknown Deepy mode: {}", mode)}); }
    if m=="prime" && !engine.as_deref().is_some_and(|s| ["opencode","claude-code","codex"].contains(&s)) {
        return serde_json::json!({"ok": false, "error": "Prime requires an engine (OpenCode / Claude Code / Codex)."});
    }
    let p = get_repo_dir().join("wgp_config.json");
    if !p.exists() { return serde_json::json!({"ok": false, "error": "wgp_config.json not found — install Wan2GP first."}); }
    let Ok(s) = std::fs::read_to_string(&p) else { return serde_json::json!({"ok": false, "error": "cannot read wgp_config.json"}); };
    let mut v: serde_json::Value = match serde_json::from_str(&s) { Ok(x)=>x, Err(e)=> return serde_json::json!({"ok": false, "error": format!("wgp_config.json corrupted: {}", e)}) };
    let bak = p.with_file_name("wgp_config.json.deepy-bak"); let _ = std::fs::copy(&p, &bak);
    let (enabled, dtype) = match m.as_str() { "disabled"=> (0,"zero"), "prime"=> (1,"prime"), _=> (1,"zero") };
    v["deepy_enabled"] = serde_json::json!(enabled); v["deepy_type"] = serde_json::json!(dtype);
    // enhancer id — JS sends number (3) or null, handle both string/number
    let enh_id: Option<i64> = enhancer.as_ref().and_then(|v| {
        if let Some(n) = v.as_i64() { Some(n) }
        else if let Some(s) = v.as_str() { s.parse::<i64>().ok() }
        else { None }
    }).or({
        match m.as_str() { "prime"=> None, "zero"=> Some(3), _=> Some(1) }
    });
    if m!="prime" { if let Some(id)=enh_id { v["enhancer_enabled"] = serde_json::json!(id); } }
    // llm_engines deepy
    let eng_map = |id: &str| match id { "opencode"=> "opencode", "claude-code"=> "claude", "codex"=> "codex", _=> "opencode" };
    let exe_map = |id: &str| match id { "opencode"=> "opencode", "claude-code"=> "claude", "codex"=> "codex", _=> "opencode" };
    let enh_to_engine = |id:i64| match id { 1=>"local_florence_llama32", 2=>"local_florence_llamajoy", 3=>"qwen35_4b", 4=>"qwen35_9b", 5=>"qwen38_27b", _=>"qwen35_4b" };
    if v.get("llm_engines").is_none() { v["llm_engines"] = serde_json::json!({}); }
    if m=="prime" {
        let eid = engine.clone().unwrap_or_else(|| "opencode".into());
        let profile = eng_map(&eid); let exe = exe_map(&eid);
        v["llm_engines"]["deepy"] = serde_json::json!(profile);
        v["llm_engines"]["prompt_enhancer"] = serde_json::json!("same_as_deepy");
        if v["llm_engines"]["profiles"].is_null() { v["llm_engines"]["profiles"] = serde_json::json!({}); }
        if v["llm_engines"]["profiles"][profile].is_null() { v["llm_engines"]["profiles"][profile] = serde_json::json!({}); }
        v["llm_engines"]["profiles"][profile]["executable"] = serde_json::json!(exe);
        if profile=="opencode" { v["llm_engines"]["profiles"]["opencode"]["base_url"] = serde_json::json!("http://127.0.0.1:4096"); }
    } else {
        let eid = enh_id.unwrap_or(1);
        let eng_str = enh_to_engine(eid);
        v["llm_engines"]["deepy"] = serde_json::json!(eng_str);
        v["llm_engines"]["prompt_enhancer"] = serde_json::json!("same_as_deepy");
    }
    if atomic_write(&p, &serde_json::to_string_pretty(&v).unwrap_or_default()).is_err() {
        return serde_json::json!({"ok": false, "error": "failed to write wgp_config.json"});
    }
    let msg = if m=="prime" { format!("Deepy Prime set to {}", engine.clone().unwrap_or_else(|| "opencode".into())) } else if m=="zero" { "Deepy Zero enabled (local model)".into() } else { "Deepy disabled".into() };
    serde_json::json!({"ok": true, "mode": m, "enhancerId": enh_id, "backup": bak.to_string_lossy().to_string(), "message": msg + ". Launch Wan2GP and click \"Ask Deepy\"."})
}
#[tauri::command] pub fn deepy_activate(engine: String) -> serde_json::Value { deepy_set("prime".into(), Some(engine), None) }
// Auto-start via the per-user Run key (no admin needed). Returns success, like the UI checks.
#[tauri::command] pub fn set_auto_start(enabled: bool) -> serde_json::Value {
    #[cfg(windows)] {
        let exe = std::env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
        let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
        let ok = if enabled && !exe.is_empty() {
            silent_command("reg").args(["add", key, "/v", "Wan2GPDesktop", "/t", "REG_SZ", "/d", &format!("\"{exe}\""), "/f"]).output().is_ok_and(|o| o.status.success())
        } else {
            silent_command("reg").args(["delete", key, "/v", "Wan2GPDesktop", "/f"]).output().is_ok_and(|o| o.status.success())
        };
        return serde_json::json!({"ok": ok, "success": ok, "enabled": enabled});
    }
    #[cfg(not(windows))] {
        let _ = enabled;
        return serde_json::json!({"ok": false, "success": false, "error": "auto-start is Windows-only"});
    }
}
#[tauri::command] pub fn memory_profile_apply(settings: serde_json::Value) -> serde_json::Value {
    // mirrors Electron memory_profile:apply — writes to wgp_config.json and returns applied keys
    let p = get_repo_dir().join("wgp_config.json");
    let Ok(s) = std::fs::read_to_string(&p) else { return serde_json::json!({"ok": false, "success": false, "error": "wgp_config.json not found"}); };
    let mut cfg: serde_json::Value = match serde_json::from_str(&s) { Ok(v)=>v, Err(e)=> return serde_json::json!({"ok": false, "success": false, "error": format!("corrupted: {}", e)}) };
    let mut applied: Vec<String> = Vec::new();
    for key in ["video_profile","image_profile","audio_profile","vram_safety_coefficient","vae_config","transformer_quantization"] {
        if let Some(val) = settings.get(key) { cfg[key] = val.clone(); applied.push(key.to_string()); }
    }
    if applied.is_empty() { return serde_json::json!({"ok": true, "success": true, "applied": applied, "unchanged": true}); }
    if atomic_write(&p, &serde_json::to_string_pretty(&cfg).unwrap_or_default()).is_err() {
        return serde_json::json!({"ok": false, "success": false, "error": "write failed"});
    }
    serde_json::json!({"ok": true, "success": true, "applied": applied})
}
// ── Queue Notifier (Apprise) ──
// Config lives in desktop-config.json under "notifier". Events are classified
// from the launch-log stream (port of services/queue-notifier.js) and delivered
// via the env's apprise (`python -m apprise`). Spawns a thread per delivery so
// log streaming never blocks.
static NOTIF_LAST_PCT: std::sync::OnceLock<std::sync::Mutex<Option<u8>>> = std::sync::OnceLock::new();
static NOTIF_LAST_FIRE: std::sync::OnceLock<std::sync::Mutex<Option<(std::time::Instant, String)>>> = std::sync::OnceLock::new();
pub(crate) fn notifier_normalize(cfg: &serde_json::Value) -> serde_json::Value {
    let step = cfg.get("progressStep").and_then(|v| v.as_u64()).unwrap_or(25).clamp(1, 100);
    serde_json::json!({
        "enabled": cfg.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false),
        "url": cfg.get("url").and_then(|v| v.as_str()).unwrap_or("").trim(),
        "notifyOnComplete": cfg.get("notifyOnComplete").and_then(|v| v.as_bool()).unwrap_or(true),
        "notifyOnFail": cfg.get("notifyOnFail").and_then(|v| v.as_bool()).unwrap_or(true),
        "notifyOnProgress": cfg.get("notifyOnProgress").and_then(|v| v.as_bool()).unwrap_or(false),
        "progressStep": step
    })
}
fn notifier_saved() -> serde_json::Value {
    notifier_normalize(load_config_value().get("notifier").unwrap_or(&serde_json::Value::Null))
}
fn env_python_bin() -> Option<PathBuf> {
    let env = get_active_env();
    let raw = env.get("path")?.as_str()?;
    let base = if std::path::Path::new(raw).is_absolute() { PathBuf::from(raw) } else { get_repo_dir().join(raw.trim_start_matches(".\\").trim_start_matches("./")) };
    let py = if cfg!(windows) { base.join("Scripts\\python.exe") } else { base.join("bin/python") };
    py.exists().then_some(py)
}
fn apprise_send(url: &str, title: &str, body: &str) -> Result<(), String> {
    let py = env_python_bin().ok_or_else(|| "No active Python environment".to_string())?;
    let out = silent_command(&py).args(["-m", "apprise", "-t", title, "-b", body, url]).output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else {
        Err(format!("apprise failed: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}
pub(crate) fn notifier_fire(kind: &str, text: &str) {
    let cfg = notifier_saved();
    if !cfg.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false) { return; }
    let url = cfg.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if url.is_empty() { return; }
    let subscribed = match kind {
        "complete" => cfg.get("notifyOnComplete").and_then(|v| v.as_bool()).unwrap_or(true),
        "fail" => cfg.get("notifyOnFail").and_then(|v| v.as_bool()).unwrap_or(true),
        "progress" => cfg.get("notifyOnProgress").and_then(|v| v.as_bool()).unwrap_or(false),
        _ => false,
    };
    if !subscribed { return; }
    // 10s cooldown per kind (log bursts shouldn't spam phones)
    if let Some(m) = NOTIF_LAST_FIRE.get() {
        if let Ok(g) = m.lock() {
            if let Some((t, k)) = g.as_ref() {
                if k == kind && t.elapsed() < std::time::Duration::from_secs(10) { return; }
            }
        }
    }
    if let Ok(mut g) = NOTIF_LAST_FIRE.get_or_init(|| std::sync::Mutex::new(None)).lock() {
        *g = Some((std::time::Instant::now(), kind.to_string()));
    }
    let msg = match kind {
        "complete" => "Wan2GP: \u{2705} generation finished".to_string(),
        "fail" => format!("Wan2GP: \u{274C} generation failed\n{}", text.chars().take(280).collect::<String>()),
        _ => format!("Wan2GP: {text}"),
    };
    std::thread::spawn(move || { let _ = apprise_send(&url, "Wan2GP", &msg); });
}
const DONE_MARKERS: &[&str] = &["task completed", "generation complete", "finished", "saved to", "output saved", "done in", "job finished", "completed successfully", "\u{2713}", "\u{2714}"];
const FAIL_MARKERS: &[&str] = &["traceback", "exception", "cuda out of memory", "out of memory", "assertionerror", "runtimeerror", "failed", "error"];
const FAIL_SKIP: &[&str] = &["0 error", "no error", "error_queue", "errors: 0", "0 errors"];
// Classify one launch-log line; progress gated on progressStep multiples.
pub(crate) fn notifier_scan_line(line: &str) {
    // progress: last NN% in line (tqdm prints 50%|...)
    let mut pct: Option<u8> = None;
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let mut j = i;
            while j > 0 && bytes[j - 1].is_ascii_digit() { j -= 1; }
            if j < i {
                if let Ok(n) = line[j..i].parse::<u8>() {
                    if n <= 100 { pct = Some(n); }
                }
            }
        }
        i += 1;
    }
    if let Some(p) = pct {
        if p < 100 {
            let cfg = notifier_saved();
            if cfg.get("notifyOnProgress").and_then(|v| v.as_bool()).unwrap_or(false) {
                let step = cfg.get("progressStep").and_then(|v| v.as_u64()).unwrap_or(25).max(1);
                let last = NOTIF_LAST_PCT.get().and_then(|m| m.lock().ok()).and_then(|g| *g).unwrap_or(0);
                if p / (step as u8) > last / (step as u8) {
                    if let Ok(mut g) = NOTIF_LAST_PCT.get_or_init(|| std::sync::Mutex::new(None)).lock() { *g = Some(p); }
                    notifier_fire("progress", &format!("{p}% done"));
                }
            }
            return;
        }
    }
    let low = line.to_lowercase();
    if FAIL_MARKERS.iter().any(|m| low.contains(m)) && !FAIL_SKIP.iter().any(|s| low.contains(s)) {
        if let Ok(mut g) = NOTIF_LAST_PCT.get_or_init(|| std::sync::Mutex::new(None)).lock() { *g = None; }
        notifier_fire("fail", line.trim());
        return;
    }
    if DONE_MARKERS.iter().any(|m| low.contains(m)) {
        if let Ok(mut g) = NOTIF_LAST_PCT.get_or_init(|| std::sync::Mutex::new(None)).lock() { *g = None; }
        notifier_fire("complete", line.trim());
    }
}
#[tauri::command] pub fn notifier_config() -> serde_json::Value {
    serde_json::json!({"ok": true, "config": notifier_saved()})
}
#[tauri::command] pub fn notifier_set(cfg: serde_json::Value) -> serde_json::Value {
    let clean = notifier_normalize(&cfg);
    if clean.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false)
        && clean.get("url").and_then(|v| v.as_str()).unwrap_or("").is_empty() {
        return serde_json::json!({"ok": false, "error": "A delivery URL is required when notifications are enabled (Apprise URL, e.g. discord://, tgram://)"});
    }
    let mut full = load_config_value();
    if let Some(m) = full.as_object_mut() { m.insert("notifier".into(), clean.clone()); }
    match crate::config::config_save(full) {
        Ok(_) => serde_json::json!({"ok": true, "config": clean}),
        Err(e) => serde_json::json!({"ok": false, "error": e}),
    }
}
#[tauri::command] pub fn notifier_test(cfg: Option<serde_json::Value>) -> serde_json::Value {
    let use_cfg = cfg.map(|c| notifier_normalize(&c)).unwrap_or_else(notifier_saved);
    let url = use_cfg.get("url").and_then(|v| v.as_str()).unwrap_or("");
    if url.is_empty() { return serde_json::json!({"ok": false, "error": "No Apprise URL set"}); }
    match apprise_send(url, "Wan2GP", "Wan2GP test notification \u{2705} — your queue notifier works.") {
        Ok(()) => serde_json::json!({"ok": true}),
        Err(e) => serde_json::json!({"ok": false, "error": e}),
    }
}
// ── Pulsebar: always-on-top mini progress window ──
// Progress % is sniffed from the launch stream (same lines the notifier scans).
// The window (pulsebar.html) polls pulsebar_state; toggle creates/destroys it.
static PULSE: std::sync::OnceLock<std::sync::Mutex<(Option<u8>, String)>> = std::sync::OnceLock::new();
pub(crate) fn pulse_sniff(line: &str) {
    // last NN% wins (tqdm prints 50%|...)
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut pct: Option<u8> = None;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let mut j = i;
            while j > 0 && bytes[j - 1].is_ascii_digit() { j -= 1; }
            if j < i {
                if let Ok(n) = line[j..i].parse::<u8>() { if n <= 100 { pct = Some(n); } }
            }
        }
        i += 1;
    }
    let m = PULSE.get_or_init(|| std::sync::Mutex::new((None, String::new())));
    if let Ok(mut g) = m.lock() {
        if let Some(p) = pct {
            *g = (Some(p), format!("Generating… {p}%"));
        } else {
            let low = line.to_lowercase();
            if DONE_MARKERS.iter().any(|mk| low.contains(mk)) { *g = (Some(100), "Done ✓".to_string()); }
            else if FAIL_MARKERS.iter().any(|mk| low.contains(mk)) && !FAIL_SKIP.iter().any(|s| low.contains(s)) {
                *g = (None, "Failed ✗".to_string());
            }
        }
    }
}
#[tauri::command] pub fn pulsebar_state() -> serde_json::Value {
    let (pct, label) = PULSE.get().and_then(|m| m.lock().ok()).map(|g| g.clone()).unwrap_or((None, String::new()));
    serde_json::json!({"pct": pct, "label": label})
}
#[tauri::command] pub fn pulsebar_show(app: tauri::AppHandle) -> serde_json::Value {
    use tauri::Manager;
    if let Some(w) = app.get_webview_window("pulsebar") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        return serde_json::json!({"ok": true, "existed": true});
    }
    match tauri::WebviewWindowBuilder::new(&app, "pulsebar", tauri::WebviewUrl::App("pulsebar.html".into()))
        .title("Wan2GP Progress")
        .inner_size(340.0, 64.0)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .build() {
        Ok(_) => serde_json::json!({"ok": true}),
        Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
    }
}
#[tauri::command] pub fn pulsebar_hide(app: tauri::AppHandle) -> serde_json::Value {
    use tauri::Manager;
    if let Some(w) = app.get_webview_window("pulsebar") { let _ = w.close(); }
    serde_json::json!({"ok": true})
}
#[tauri::command] pub fn set_theme_follow_system(enabled: bool) -> serde_json::Value {
    let mut full = load_config_value();
    if let Some(m) = full.as_object_mut() { m.insert("themeFollowSystem".into(), serde_json::json!(enabled)); }
    match crate::config::config_save(full) {
        Ok(_) => serde_json::json!({"ok": true}),
        Err(e) => serde_json::json!({"ok": false, "error": e}),
    }
}
#[tauri::command] pub fn set_notifications_enabled(enabled: bool) -> serde_json::Value { let _=enabled; serde_json::json!({"ok": true}) }

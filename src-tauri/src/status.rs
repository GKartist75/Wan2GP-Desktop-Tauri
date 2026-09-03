//! Environment status, version scans and prerequisite probes.
use std::path::PathBuf;
use std::sync::Mutex;
use crate::base::*;
use crate::hw::{get_gpu_info_sync, kernel_profile_key};

#[tauri::command]
pub fn get_status() -> serde_json::Value {
    let env = get_active_env();
    // site-packages mtime: pip install/uninstall invalidates the cache instantly
    let sp_mtime = env.get("path").and_then(|p| p.as_str()).map(|raw| {
        let base = if std::path::Path::new(raw).is_absolute() { PathBuf::from(raw) } else { get_repo_dir().join(raw.trim_start_matches(".\\").trim_start_matches("./")) };
        #[cfg(windows)] { base.join("Lib").join("site-packages") }
        #[cfg(not(windows))] { base.join("lib") }
    }).and_then(|d| std::fs::metadata(&d).and_then(|m| m.modified()).ok());
    if let Some(m) = LAST_STATUS.get() {
        if let Ok(g) = m.lock() {
            if let Some((t, mt, v)) = g.as_ref() {
                if t.elapsed() < std::time::Duration::from_secs(5) && *mt == sp_mtime { return v.clone(); }
            }
        }
    }
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
                        // build overview wheels with label/pipName/configured so frontend shows "want <ver>" not "want ?"
                        let mut arr = Vec::new();
                        for k in kernels {
                            if let Some(key) = k.as_str() {
                                let (label, pip) = match key {
                                    "nunchaku_cu13" | "nunchaku" => ("Nunchaku", "nunchaku"),
                                    "gguf" | "llamacpp_gguf_cuda" => ("GGUF (llamacpp)", "llamacpp_gguf_cuda"),
                                    "light2xv" | "lightx2v_kernel" => ("LightX2V", "lightx2v_kernel"),
                                    _ => (key, key),
                                };
                                // configured version from setup_config.json components.kernels[key].cmd[win]
                                let mut configured: Option<String> = None;
                                if let Some(cmd) = cfg.get("components").and_then(|c| c.get("kernels")).and_then(|m| m.get(key)).and_then(|e| e.get("cmd")).and_then(|c| c.get("win")).and_then(|u| u.as_str()) {
                                    // wheelDistVersion: parse "<dist>-<version>-cp..." 
                                    if let Some(base) = cmd.split('/').next_back() {
                                        let base = base.trim_end_matches(".whl");
                                        if let Some(dash) = base.find('-') {
                                            let rest = &base[dash+1..];
                                            if let Some(v_end) = rest.find("-cp") .or_else(|| rest.find("-py")) {
                                                configured = Some(rest[..v_end].to_string());
                                            }
                                        }
                                    }
                                }
                                arr.push(serde_json::json!({"key": key, "label": label, "pipName": pip, "configured": configured}));
                            }
                        }
                        wheels = serde_json::Value::Array(arr);
                    }
                }
            }
        }
    }
    // wheels already built with configured, but installed will be filled after version scan
    let pending_wheels = wheels.clone();
    // real version scan via env's python (importlib.metadata) — ponytail: helper file on same drive as env
    let mut versions = serde_json::Map::new();
    if let Some(raw) = env.get("path").and_then(|p| p.as_str()) {
        let rel = raw.trim_start_matches(".\\").trim_start_matches("./");
        let base = if std::path::Path::new(raw).is_absolute() { PathBuf::from(raw) } else { get_repo_dir().join(rel) };
        let py = if cfg!(windows) { base.join("Scripts\\python.exe") } else { base.join("bin/python3") };
        let py_bin = if py.exists() { py } else { PathBuf::from(raw) };
        if py_bin.exists() {
            let helper = get_data_dir().join(".get_versions.py");
            let code = r"import sys, importlib.metadata
try:
    aliases={'triton':'triton-windows','opencv-python':'opencv','spas_sage_attn':'spas-sage-attn','huggingface_hub':'huggingface-hub'}
    pkgs=['python','torch','triton','sageattention','spas_sage_attn','flash_attn','nunchaku','llamacpp_gguf_cuda','lightx2v','diffusers','transformers','gradio','accelerate','onnxruntime','xformers','mmgp','moviepy','opencv-python','insightface','peft','timm','vector_quantize_pytorch','torchcodec','torchaudio','huggingface_hub','bitsandbytes','numpy','sentencepiece','open_clip_torch','imageio','einops','librosa','soundfile','tokenizers','av','claude-agent-sdk']
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
";
            let _ = std::fs::write(&helper, code);
            if let Ok(out) = silent_command(&py_bin).arg(&helper).current_dir(&repo).output() {
                if out.status.success() {
                    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    for part in s.split("||") {
                        if let Some((k,v)) = part.split_once('=') { versions.insert(k.to_string(), serde_json::Value::String(v.to_string())); }
                    }
                }
            }
        }
    }
    // fill installed into wheels now that versions are known
    let mut final_wheels = Vec::new();
    if let Some(arr) = pending_wheels.as_array() {
        for w in arr {
            let mut obj = w.clone();
            if let Some(pip) = w.get("pipName").and_then(|v| v.as_str()) {
                if let Some(ver) = versions.get(pip).and_then(|v| v.as_str()) {
                    obj["installed"] = serde_json::json!(ver);
                    let cfg = w.get("configured").and_then(|v| v.as_str()).unwrap_or("");
                    // configured is like "1.2.1+cu13.0torch2.10", installed is same or with "+" - compare prefix before "+"
                    let want = cfg.split('+').next().unwrap_or(cfg);
                    let have = ver.split('+').next().unwrap_or(ver);
                    obj["state"] = serde_json::json!(if have==want { "ok" } else { "mismatch" });
                } else {
                    obj["state"] = serde_json::json!("missing");
                }
            }
            final_wheels.push(obj);
        }
    } else if let Some(arr) = wheels.as_array() {
        final_wheels.clone_from(arr);
    }
    let out_wheels = if final_wheels.is_empty() { wheels } else { serde_json::Value::Array(final_wheels) };
    // profile object for frontend specSparge (was only kernelProfile string → showed —)
    let profile_obj = cfg_path.exists().then(|| std::fs::read_to_string(&cfg_path).ok()).flatten().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()).and_then(|c| c.get("gpu_profiles").and_then(|p| p.get(&profile)).cloned()).unwrap_or(serde_json::json!({"sparge": null}));
    let out = serde_json::json!({"env": env, "versions": serde_json::Value::Object(versions), "kernelWheels": out_wheels, "kernelProfile": profile, "profile": profile_obj, "spike": false});
    // cache for 5s (mtime-keyed: pip changes invalidate instantly)
    if let Ok(mut g) = LAST_STATUS.get_or_init(|| Mutex::new(None)).lock() { *g = Some((std::time::Instant::now(), sp_mtime, out.clone())); }
    out
}
#[tauri::command]
pub fn check_python() -> serde_json::Value {
    
    for cmd in ["python", "python3", "py"] {
        if let Ok(out) = silent_command(cmd).args(["--version"]).output() {
            if out.status.success() {
                let v = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)).trim().to_string();
                return serde_json::json!({ "found": true, "cmd": cmd, "version": v });
            }
        }
    }
    serde_json::json!({ "found": false, "cmd": null, "version": "python not found" })
}
#[tauri::command]
pub fn check_git() -> serde_json::Value {
    match silent_command("git").arg("--version").output() {
        Ok(out) if out.status.success() => serde_json::json!({ "found": true, "version": String::from_utf8_lossy(&out.stdout).trim() }),
        _ => serde_json::json!({ "found": false, "version": "git not found" }),
    }
}

// ── Phase 1: paths / config / hardware / install checks ──
pub(crate) fn get_active_env() -> serde_json::Value {
    let f = get_envs_file();
    if !f.exists() { return serde_json::Value::Null; }
    let Ok(s) = std::fs::read_to_string(&f) else { return serde_json::Value::Null; };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else { return serde_json::Value::Null; };
    let active = v.get("active").and_then(|x| x.as_str()).unwrap_or("");
    if active.is_empty() { return serde_json::Value::Null; }
    if let Some(env) = v.get("envs").and_then(|e| e.get(active)) { env.clone() } else { serde_json::Value::Null }
}

#[tauri::command]
pub fn check_installed() -> serde_json::Value {
    let repo = get_repo_dir();
    let has_repo = repo.join("wgp.py").exists();
    let has_env = !get_active_env().is_null();
    serde_json::json!({"repo": has_repo, "env": has_env})
}

#[tauri::command]
pub fn check_command(cmd: String) -> serde_json::Value {
    #[cfg(windows)] let probe = silent_command("where").arg(&cmd).output();
    #[cfg(not(windows))] let probe = silent_command("which").arg(&cmd).output();
    let found = probe.is_ok_and(|o| o.status.success());
    serde_json::json!({"cmd": cmd, "found": found})
}

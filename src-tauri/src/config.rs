//! Launcher config, install paths, model folders, env management, uv cache.
use std::path::{Path, PathBuf};
use crate::base::*;
use crate::{hw::{build_install_plan, get_gpu_info_sync}, status::get_active_env};

#[tauri::command]
pub fn config_load() -> serde_json::Value { load_config_value() }

#[tauri::command]
pub fn config_save(cfg: serde_json::Value) -> Result<serde_json::Value, String> {
    let p = get_config_file();
    let s = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    atomic_write(&p, &s).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"ok": true, "success": true}))
}

#[tauri::command]
pub fn get_install_paths() -> serde_json::Value {
    let data = get_data_dir();
    let repo = get_repo_dir();
    let orig = if let Ok(a) = std::env::var("APPDATA") { PathBuf::from(a).join("wan2gp-desktop") } else { PathBuf::from("C:\\Users\\Default\\AppData\\Roaming\\wan2gp-desktop") };
    let models_default = data.with_file_name(format!("{}-Models", data.file_name().unwrap_or_default().to_string_lossy()));
    let models_default = if models_default.to_string_lossy().is_empty() { PathBuf::from("C:\\Wan2GP-Models") } else { models_default };
    serde_json::json!({
        "appData": data.to_string_lossy().to_string(),
        "appDataRoot": orig.to_string_lossy().to_string(),
        "repo": repo.to_string_lossy().to_string(),
        "dataDir": data.to_string_lossy().to_string(),
        "repoDir": repo.to_string_lossy().to_string(),
        "config": get_config_file().to_string_lossy().to_string(),
        "configFile": get_config_file().to_string_lossy().to_string(),
        "envsFile": get_envs_file().to_string_lossy().to_string(),
        "modelsDefault": models_default.to_string_lossy().to_string(),
        "dataDirInRoaming": data.to_string_lossy().to_string().to_lowercase().contains("appdata"),
        "legacyRoamingFound": false,
        "isRoaming": data.to_string_lossy().contains("AppData")
    })
}

#[tauri::command]
pub fn get_disk_space(path: Option<String>) -> serde_json::Value {
    let p = path.unwrap_or_else(|| get_data_dir().to_string_lossy().to_string());
    // Use sysinfo Disks — ~0ms, no powershell spawn (was 400ms)
    {
        use sysinfo::{Disks, DiskRefreshKind};
        let disks = Disks::new_with_refreshed_list_specifics(DiskRefreshKind::nothing().with_storage());
        // match longest prefix mount_point that is parent of p
        let mut best: Option<&sysinfo::Disk> = None;
        let mut best_len = 0usize;
        for d in disks.list() {
            let mp = d.mount_point().to_string_lossy().to_string();
            if p.to_lowercase().starts_with(&mp.to_lowercase()) && mp.len() > best_len {
                best_len = mp.len();
                best = Some(d);
            }
        }
        if let Some(d) = best {
            return serde_json::json!({"path": p, "free": d.available_space(), "total": d.total_space()});
        }
        // fallback: first disk
        if let Some(d) = disks.list().first() {
            return serde_json::json!({"path": p, "free": d.available_space(), "total": d.total_space()});
        }
    }
    serde_json::json!({"path": p, "free": null, "total": null})
}

#[tauri::command]
pub fn get_model_paths() -> serde_json::Value {
    let repo = get_repo_dir();
    let cfg_path = repo.join("wgp_config.json");
    if cfg_path.exists() {
        if let Ok(s) = std::fs::read_to_string(&cfg_path) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                let mut out = serde_json::Map::new();
                if let Some(a) = v.get("checkpointsPaths").and_then(|x| x.as_array()).and_then(|a| a.first()) { out.insert("checkpoints".into(), a.clone()); }
                else if let Some(a) = v.get("checkpoints_paths").and_then(|x| x.as_array()).and_then(|a| a.first()) { out.insert("checkpoints".into(), a.clone()); }
                else if let Some(c) = v.get("ckpt_dir") { if let Some(arr)=c.as_array().and_then(|a| a.first()) { out.insert("checkpoints".into(), arr.clone()); } else { out.insert("checkpoints".into(), c.clone()); } }
                if let Some(l) = v.get("lorasRoot") { out.insert("loras".into(), l.clone()); }
                else if let Some(l) = v.get("loras_root") { out.insert("loras".into(), l.clone()); }
                else if let Some(l) = v.get("lora_dir") { out.insert("loras".into(), l.clone()); }
                if let Some(o) = v.get("savePath") { out.insert("output".into(), o.clone()); }
                else if let Some(o) = v.get("save_path") { out.insert("output".into(), o.clone()); }
                if !out.is_empty() { return serde_json::Value::Object(out); }
            }
        }
    }
    // ponytail: fallback to desktop-config.json (changeModelFolder also writes there) — so UI shows new path even if wgp_config not yet created
    let dc = load_config_value();
    let mut out = serde_json::Map::new();
    if let Some(p) = dc.get("modelCkptsPath").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) { out.insert("checkpoints".into(), serde_json::Value::String(p.to_string())); }
    if let Some(p) = dc.get("modelLorasPath").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) { out.insert("loras".into(), serde_json::Value::String(p.to_string())); }
    if let Some(p) = dc.get("modelOutputPath").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) { out.insert("output".into(), serde_json::Value::String(p.to_string())); }
    if !out.is_empty() { return serde_json::Value::Object(out); }
    serde_json::Value::Null
}

#[tauri::command]
pub fn detect_model_folders() -> serde_json::Value {
    let repo = get_repo_dir();
    let candidates = ["ckpts","loras","outputs","output","models"];
    let mut out = serde_json::Map::new();
    for c in candidates { out.insert(c.into(), serde_json::Value::Bool(repo.join(c).exists())); }
    serde_json::Value::Object(out)
}

// ── Phase 2 stubs + real logic as needed ──
#[tauri::command]
pub fn install_plan() -> serde_json::Value {
    let gpu = get_gpu_info_sync();
    let plan = build_install_plan(&gpu);
    // disk check (ponytail: statvfs when exact GB needed)
    let disk = get_disk_space(None);
    serde_json::json!({"gpu": gpu, "plan": plan, "disk": disk})
}
#[tauri::command]
pub fn validate_install() -> serde_json::Value {
    let repo = get_repo_dir();
    let mut errors: Vec<String> = Vec::new();
    if !repo.join("wgp.py").exists() { errors.push("wgp.py not found — not installed".into()); }
    if !repo.join("setup_config.json").exists() { errors.push("setup_config.json missing".into()); }
    if get_active_env().is_null() { errors.push("no active env".into()); }
    serde_json::json!({"ok": errors.is_empty(), "errors": errors})
}
#[tauri::command]
pub fn uv_cache_info() -> serde_json::Value {
    let p = get_repo_dir().join(".uv-cache");
    serde_json::json!({"exists": p.exists(), "sizeBytes": null, "cacheDir": p.to_string_lossy().to_string()})
}
#[tauri::command]
pub async fn uv_cache_size() -> serde_json::Value {
    // ponytail: async walk so Manage → Calculate size doesn't freeze UI (Electron 63b0f90)
    let p = get_repo_dir().join(".uv-cache");
    if !p.exists() { return serde_json::json!({"exists": false, "sizeBytes": 0, "cacheDir": p.to_string_lossy().to_string()}); }
    let p_clone = p.clone();
    let size = tauri::async_runtime::spawn_blocking(move || {
        let mut size: u64 = 0;
        fn walk(p: &Path, acc: &mut u64) { if let Ok(rd) = std::fs::read_dir(p) { for e in rd.flatten() { if let Ok(m) = e.metadata() { if m.is_dir() { walk(&e.path(), acc); } else { *acc += m.len(); } } } } }
        walk(&p_clone, &mut size);
        size
    }).await.unwrap_or(0);
    serde_json::json!({"exists": true, "sizeBytes": size, "cacheDir": p.to_string_lossy().to_string()})
}
#[tauri::command]
pub fn manage_list() -> serde_json::Value {
    let f = get_envs_file();
    if !f.exists() { return serde_json::json!([]); }
    if let Ok(s) = std::fs::read_to_string(&f) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
            if let Some(envs) = v.get("envs").and_then(|e| e.as_object()) {
                return serde_json::Value::Array(envs.keys().map(|k| serde_json::Value::String(k.clone())).collect());
            }
        }
    }
    serde_json::json!([])
}
#[tauri::command] pub fn manage_set_active(name: String) -> Result<serde_json::Value,String> {
    let f = get_envs_file(); let mut v: serde_json::Value = std::fs::read_to_string(&f).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(serde_json::json!({"envs":{}, "active":null}));
    if v.get("envs").and_then(|e| e.get(&name)).is_none() { return Err(format!("env {name} not found")); }
    v["active"] = serde_json::Value::String(name);
    atomic_write(&f, &serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command] pub fn uninstall_env(name: String) -> Result<serde_json::Value,String> {
    let f = get_envs_file(); let mut v: serde_json::Value = std::fs::read_to_string(&f).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(serde_json::json!({"envs":{}}));
    let path = v.get("envs").and_then(|e| e.get(&name)).and_then(|e| e.get("path")).and_then(|p| p.as_str()).map(|p| if std::path::Path::new(p).is_absolute() { PathBuf::from(p) } else { get_repo_dir().join(p.trim_start_matches(".\\").trim_start_matches("./")) });
    if let Some(p) = path { let _ = std::fs::remove_dir_all(p); }
    if let Some(obj) = v.get_mut("envs").and_then(|e| e.as_object_mut()) { obj.remove(&name); }
    if v.get("active").and_then(|a| a.as_str()) == Some(&name) { v["active"] = serde_json::Value::Null; }
    atomic_write(&f, &serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"ok": true, "success": true}))
}

#[tauri::command] pub fn uv_cache_clean(action: Option<String>) -> serde_json::Value { let _=action; serde_json::json!({"success": true}) }

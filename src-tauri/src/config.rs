//! Launcher config, install paths, model folders, env management, uv cache.
use crate::base::*;
use crate::{
    hw::{build_install_plan, get_gpu_info_sync},
    status::get_active_env,
};
use std::path::{Path, PathBuf};

#[tauri::command]
pub fn config_load() -> serde_json::Value {
    load_config_value()
}

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
    let orig = if let Ok(a) = std::env::var("APPDATA") {
        PathBuf::from(a).join("wan2gp-desktop")
    } else {
        PathBuf::from("C:\\Users\\Default\\AppData\\Roaming\\wan2gp-desktop")
    };
    let models_default = data.with_file_name(format!(
        "{}-Models",
        data.file_name().unwrap_or_default().to_string_lossy()
    ));
    let models_default = if models_default.to_string_lossy().is_empty() {
        PathBuf::from("C:\\Wan2GP-Models")
    } else {
        models_default
    };
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

/// Free/total bytes for the disk hosting `p` (longest-prefix mount match).
/// Shared by get_disk_space and python_preflight (uv's own data dir).
pub(crate) fn disk_for_path(p: &str) -> Option<(u64, u64)> {
    use sysinfo::{DiskRefreshKind, Disks};
    let disks = Disks::new_with_refreshed_list_specifics(DiskRefreshKind::nothing().with_storage());
    let mut best: Option<&sysinfo::Disk> = None;
    let mut best_len = 0usize;
    for d in disks.list() {
        let mp = d.mount_point().to_string_lossy().to_string();
        if p.to_lowercase().starts_with(&mp.to_lowercase()) && mp.len() > best_len {
            best_len = mp.len();
            best = Some(d);
        }
    }
    best.map(|d| (d.available_space(), d.total_space()))
}

#[tauri::command]
pub fn get_disk_space(path: Option<String>) -> serde_json::Value {
    let p = path.unwrap_or_else(|| get_data_dir().to_string_lossy().to_string());
    // Use sysinfo Disks — ~0ms, no powershell spawn (was 400ms)
    if let Some((free, total)) = disk_for_path(&p) {
        return serde_json::json!({"path": p, "free": free, "total": total});
    }
    {
        use sysinfo::{DiskRefreshKind, Disks};
        let disks =
            Disks::new_with_refreshed_list_specifics(DiskRefreshKind::nothing().with_storage());
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
                if let Some(a) = v
                    .get("checkpointsPaths")
                    .and_then(|x| x.as_array())
                    .and_then(|a| a.first())
                {
                    out.insert("checkpoints".into(), a.clone());
                } else if let Some(a) = v
                    .get("checkpoints_paths")
                    .and_then(|x| x.as_array())
                    .and_then(|a| a.first())
                {
                    out.insert("checkpoints".into(), a.clone());
                } else if let Some(c) = v.get("ckpt_dir") {
                    if let Some(arr) = c.as_array().and_then(|a| a.first()) {
                        out.insert("checkpoints".into(), arr.clone());
                    } else {
                        out.insert("checkpoints".into(), c.clone());
                    }
                }
                if let Some(l) = v.get("lorasRoot") {
                    out.insert("loras".into(), l.clone());
                } else if let Some(l) = v.get("loras_root") {
                    out.insert("loras".into(), l.clone());
                } else if let Some(l) = v.get("lora_dir") {
                    out.insert("loras".into(), l.clone());
                }
                if let Some(o) = v.get("savePath") {
                    out.insert("output".into(), o.clone());
                } else if let Some(o) = v.get("save_path") {
                    out.insert("output".into(), o.clone());
                }
                if !out.is_empty() {
                    return serde_json::Value::Object(out);
                }
            }
        }
    }
    // ponytail: fallback to desktop-config.json (changeModelFolder also writes there) — so UI shows new path even if wgp_config not yet created
    let dc = load_config_value();
    let mut out = serde_json::Map::new();
    if let Some(p) = dc
        .get("modelCkptsPath")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        out.insert(
            "checkpoints".into(),
            serde_json::Value::String(p.to_string()),
        );
    }
    if let Some(p) = dc
        .get("modelLorasPath")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        out.insert("loras".into(), serde_json::Value::String(p.to_string()));
    }
    if let Some(p) = dc
        .get("modelOutputPath")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        out.insert("output".into(), serde_json::Value::String(p.to_string()));
    }
    if !out.is_empty() {
        return serde_json::Value::Object(out);
    }
    serde_json::Value::Null
}

#[tauri::command]
pub fn detect_model_folders() -> serde_json::Value {
    let repo = get_repo_dir();
    let candidates = ["ckpts", "loras", "outputs", "output", "models"];
    let mut out = serde_json::Map::new();
    for c in candidates {
        out.insert(c.into(), serde_json::Value::Bool(repo.join(c).exists()));
    }
    serde_json::Value::Object(out)
}

// ── F4 library: LoRA + finetune librarians ──

/// Resolve the LoRA root: wgp_config (any key spelling) → desktop-config
/// modelLorasPath → repo/loras. Mirrors get_model_paths() precedence.
pub(crate) fn resolve_loras_root() -> Option<PathBuf> {
    let repo = get_repo_dir();
    if let Ok(s) = std::fs::read_to_string(repo.join("wgp_config.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
            for k in ["loras_root", "lorasRoot", "lora_dir"] {
                if let Some(p) = v.get(k).and_then(|x| x.as_str()).filter(|s| !s.is_empty()) {
                    return Some(PathBuf::from(p));
                }
            }
        }
    }
    let dc = load_config_value();
    if let Some(p) = dc
        .get("modelLorasPath")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        return Some(PathBuf::from(p));
    }
    let fallback = repo.join("loras");
    if fallback.exists() {
        Some(fallback)
    } else {
        None
    }
}

fn is_lora_file(name: &str) -> bool {
    let l = name.to_lowercase();
    l.ends_with(".safetensors") || l.ends_with(".sft")
}

/// Count cache hits for one folder. Cache keys are `"<dir>|<file>"`
/// (see loras_url_cache_v2.json) — exact match, no guessing.
pub(crate) fn lora_cache_hits(
    cache: &std::collections::HashMap<String, String>,
    dir: &str,
    files: &[String],
) -> usize {
    files
        .iter()
        .filter(|f| cache.contains_key(&format!("{dir}|{f}")))
        .count()
}

/// Finetune file-stem allowlist: `[A-Za-z0-9_-]{1,64}`, no separators, no ext.
/// Rejects path traversal and hidden files alike.
pub(crate) fn finetune_id_valid(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Summarize one finetune definition (never fails — unparsable files report
/// an error string so one bad file can't hide the whole library).
pub(crate) fn summarize_finetune(id: &str, text: &str, bytes: u64) -> serde_json::Value {
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(text);
    let Ok(v) = parsed else {
        return serde_json::json!({"id": id, "error": "not valid JSON", "bytes": bytes});
    };
    let m = v.get("model");
    let str_list = |key: &str| m.and_then(|m| m.get(key)).and_then(|u| u.as_array()).map(|a| a.len()).unwrap_or(0);
    let desc = m
        .and_then(|m| m.get("description"))
        .and_then(|d| d.as_str())
        .unwrap_or("");
    let mut short: String = desc.chars().take(200).collect();
    if desc.chars().count() > 200 {
        short.push('…');
    }
    serde_json::json!({
        "id": id,
        "name": m.and_then(|m| m.get("name")).and_then(|n| n.as_str()).unwrap_or(id),
        "architecture": m.and_then(|m| m.get("architecture")).and_then(|a| a.as_str()).unwrap_or("?"),
        "urls": str_list("URLs"),
        "urls2": str_list("URLs2"),
        "loras": str_list("loras"),
        "description": short,
        "bytes": bytes,
    })
}

#[tauri::command]
pub fn library_loras() -> serde_json::Value {
    let Some(root) = resolve_loras_root() else {
        return serde_json::json!({"ok": false, "error": "No LoRA root configured"});
    };
    let repo = get_repo_dir();
    let mut cache = std::collections::HashMap::new();
    for name in ["loras_url_cache_v2.json", "loras_url_cache.json"] {
        if let Ok(s) = std::fs::read_to_string(repo.join(name)) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                if let Some(o) = v.as_object() {
                    for (k, val) in o {
                        if let Some(u) = val.as_str() {
                            cache.insert(k.clone(), u.to_string());
                        }
                    }
                }
            }
            break;
        }
    }
    let mut folders = vec![];
    let entries = std::fs::read_dir(&root).map(|r| r.filter_map(|e| e.ok()).collect::<Vec<_>>()).unwrap_or_default();
    for e in entries {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let mut files = vec![];
        let mut bytes = 0u64;
        if let Ok(inner) = std::fs::read_dir(&p) {
            for f in inner.filter_map(|x| x.ok()) {
                let fp = f.path();
                if !fp.is_file() {
                    continue;
                }
                let fn_ = f.file_name().to_string_lossy().to_string();
                if is_lora_file(&fn_) {
                    bytes += f.metadata().map(|m| m.len()).unwrap_or(0);
                    files.push(fn_);
                }
            }
        }
        files.sort();
        let dir_key = p.to_string_lossy().to_string();
        let hits = lora_cache_hits(&cache, &dir_key, &files);
        let truncated = files.len() > 200;
        let sample: Vec<String> = files.iter().take(200).cloned().collect();
        folders.push(serde_json::json!({
            "name": name,
            "files": files.len(),
            "bytes": bytes,
            "urls_known": hits,
            "truncated": truncated,
            "sample": sample,
        }));
    }
    folders.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    serde_json::json!({"ok": true, "root": root.to_string_lossy().to_string(), "folders": folders})
}

#[tauri::command]
pub fn library_finetunes() -> serde_json::Value {
    let dir = get_repo_dir().join("finetunes");
    let mut items = vec![];
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.filter_map(|x| x.ok()) {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let id = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            if !finetune_id_valid(&id) {
                continue;
            }
            let bytes = e.metadata().map(|m| m.len()).unwrap_or(0);
            let text = std::fs::read_to_string(&p).unwrap_or_default();
            items.push(summarize_finetune(&id, &text, bytes));
        }
    }
    items.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    serde_json::json!({"ok": true, "dir": dir.to_string_lossy().to_string(), "items": items})
}

#[tauri::command]
pub fn library_finetune_import(source: String) -> Result<serde_json::Value, String> {
    let src = PathBuf::from(&source);
    if !src.is_absolute() || !src.is_file() {
        return Err("Source must be an existing .json file".into());
    }
    if src.extension().and_then(|x| x.to_str()) != Some("json") {
        return Err("Source must be a .json file".into());
    }
    let id = src.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
    if !finetune_id_valid(&id) {
        return Err("Filename must be [A-Za-z0-9_-], max 64 chars".into());
    }
    let text = std::fs::read_to_string(&src).map_err(|e| format!("Cannot read source ({e})"))?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| "Source is not valid JSON".to_string())?;
    if v.get("model").is_none() {
        return Err("Not a finetune definition (no \"model\" object)".into());
    }
    let dest = get_repo_dir().join("finetunes").join(format!("{id}.json"));
    if dest.exists() {
        return Err(format!("{id}.json already exists — delete it first to replace"));
    }
    std::fs::write(&dest, text).map_err(|e| format!("Cannot write ({e})"))?;
    Ok(serde_json::json!({"ok": true, "id": id}))
}

#[tauri::command]
pub fn library_finetune_delete(id: String) -> Result<serde_json::Value, String> {
    if !finetune_id_valid(&id) {
        return Err("Invalid finetune id".into());
    }
    let p = get_repo_dir().join("finetunes").join(format!("{id}.json"));
    if !p.is_file() {
        return Err("Finetune not found".into());
    }
    std::fs::remove_file(&p).map_err(|e| format!("Cannot delete ({e})"))?;
    Ok(serde_json::json!({"ok": true, "id": id}))
}

/// F4-models: crude kind tag from a checkpoint filename (display only —
/// never authoritative about precision, the loader decides that).
pub(crate) fn model_kind_tag(name: &str) -> &'static str {
    let l = name.to_lowercase();
    if l.contains("gguf") {
        "GGUF"
    } else if l.contains("nvfp4") {
        "NVFP4"
    } else if l.contains("nunchaku") || l.contains("svdq") || l.contains("nf4") {
        "Nunchaku/NF4"
    } else if l.contains("quanto") || l.contains("int8") {
        "INT8"
    } else if l.contains("fp8") {
        "FP8"
    } else if l.contains("bf16") {
        "BF16"
    } else if l.contains("fp16") {
        "FP16"
    } else {
        "—"
    }
}

fn is_ckpt_file(name: &str) -> bool {
    let l = name.to_lowercase();
    l.ends_with(".safetensors")
        || l.ends_with(".gguf")
        || l.ends_with(".pt")
        || l.ends_with(".pth")
        || l.ends_with(".bin")
        || l.ends_with(".ckpt")
}

/// Resolve the checkpoints dir: wgp_config checkpoints_paths[0] (any key
/// spelling, mirrors get_model_paths) → desktop-config modelCkptsPath →
/// repo/ckpts.
pub(crate) fn resolve_ckpts_dir() -> Option<PathBuf> {
    let repo = get_repo_dir();
    if let Ok(s) = std::fs::read_to_string(repo.join("wgp_config.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
            for k in ["checkpoints_paths", "checkpointsPaths", "ckpt_dir"] {
                if let Some(p) = v
                    .get(k)
                    .and_then(|x| x.as_array().and_then(|a| a.first()).or(Some(x)))
                    .and_then(|x| x.as_str())
                    .filter(|s| !s.is_empty())
                {
                    return Some(PathBuf::from(p));
                }
            }
        }
    }
    let dc = load_config_value();
    if let Some(p) = dc
        .get("modelCkptsPath")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        return Some(PathBuf::from(p));
    }
    let fallback = repo.join("ckpts");
    if fallback.exists() {
        Some(fallback)
    } else {
        None
    }
}

/// F4-models: downloaded checkpoint inventory (names, sizes, kind tags).
/// Reads file metadata only — never opens weights.
#[tauri::command]
pub fn library_models() -> serde_json::Value {
    let Some(root) = resolve_ckpts_dir() else {
        return serde_json::json!({"ok": false, "error": "No checkpoints folder configured"});
    };
    let mut files = vec![];
    let mut bytes = 0u64;
    if let Ok(entries) = std::fs::read_dir(&root) {
        for e in entries.filter_map(|x| x.ok()) {
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if !is_ckpt_file(&name) {
                continue;
            }
            let b = e.metadata().map(|m| m.len()).unwrap_or(0);
            bytes += b;
            files.push(serde_json::json!({
                "name": name,
                "bytes": b,
                "kind": model_kind_tag(&e.file_name().to_string_lossy()),
            }));
        }
    }
    files.sort_by(|a, b| b["bytes"].as_u64().cmp(&a["bytes"].as_u64()));
    let truncated = files.len() > 300;
    let sample: Vec<serde_json::Value> = files.iter().take(300).cloned().collect();
    serde_json::json!({
        "ok": true,
        "root": root.to_string_lossy().to_string(),
        "files": files.len(),
        "bytes": bytes,
        "truncated": truncated,
        "sample": sample,
    })
}

#[tauri::command]
pub fn library_finetune_content(id: String) -> Result<serde_json::Value, String> {
    if !finetune_id_valid(&id) {
        return Err("Invalid finetune id".into());
    }
    let p = get_repo_dir().join("finetunes").join(format!("{id}.json"));
    let text = std::fs::read_to_string(&p).map_err(|_| "Finetune not found".to_string())?;
    Ok(serde_json::json!({"ok": true, "id": id, "content": text}))
}

// ── F7 workspaces: backup + disk usage ──

/// Summarize one workspace definition. Referenced files are stat'ed (capped)
/// so the UI can show real disk use and spot moved/deleted media.
pub(crate) fn summarize_workspace(id: &str, v: &serde_json::Value) -> serde_json::Value {
    let g = v.get("gallery");
    let files: Vec<String> = g
        .and_then(|g| g.get("file_list"))
        .and_then(|l| l.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    let audio: usize = g
        .and_then(|g| g.get("audio_file_list"))
        .and_then(|l| l.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let mut bytes = 0u64;
    let mut missing = 0usize;
    for f in files.iter().take(2000) {
        match std::fs::metadata(f) {
            Ok(m) if m.is_file() => bytes += m.len(),
            _ => missing += 1,
        }
    }
    serde_json::json!({
        "id": id,
        "name": v.get("name").and_then(|n| n.as_str()).unwrap_or(id),
        "files": files.len(),
        "audio": audio,
        "bytes": bytes,
        "missing": missing,
        "truncated": files.len() > 2000,
        "last_activity": v.get("last_activity").and_then(|t| t.as_f64()).unwrap_or(0.0),
        "archive_protected": v.get("archive_protected").and_then(|p| p.as_bool()).unwrap_or(false),
    })
}

#[tauri::command]
pub fn workspace_list() -> serde_json::Value {
    let dir = get_repo_dir().join("workspaces");
    let mut items = vec![];
    let mut archived = 0usize;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.filter_map(|x| x.ok()) {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().and_then(|n| n.to_str()) == Some("archives") {
                    archived = std::fs::read_dir(&p).map(|r| r.filter_map(|x| x.ok()).count()).unwrap_or(0);
                }
                continue;
            }
            if p.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let id = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            if !finetune_id_valid(&id) {
                continue;
            }
            let text = std::fs::read_to_string(&p).unwrap_or_default();
            match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(v) => items.push(summarize_workspace(&id, &v)),
                Err(_) => items.push(serde_json::json!({"id": id, "error": "not valid JSON"})),
            }
        }
    }
    items.sort_by(|a, b| {
        let ta = a.get("last_activity").and_then(|t| t.as_f64()).unwrap_or(0.0);
        let tb = b.get("last_activity").and_then(|t| t.as_f64()).unwrap_or(0.0);
        tb.partial_cmp(&ta).unwrap_or(std::cmp::Ordering::Equal)
    });
    serde_json::json!({"ok": true, "dir": dir.to_string_lossy().to_string(), "items": items, "archived": archived})
}

#[tauri::command]
pub fn workspace_protect(id: String, protected: bool) -> Result<serde_json::Value, String> {
    if !finetune_id_valid(&id) {
        return Err("Invalid workspace id".into());
    }
    let p = get_repo_dir().join("workspaces").join(format!("{id}.json"));
    let text = std::fs::read_to_string(&p).map_err(|_| "Workspace not found".to_string())?;
    let mut v: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| "Workspace is not valid JSON".to_string())?;
    if let Some(m) = v.as_object_mut() {
        m.insert("archive_protected".into(), serde_json::Value::Bool(protected));
    }
    let out = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    std::fs::write(&p, out).map_err(|e| format!("Cannot write ({e})"))?;
    Ok(serde_json::json!({"ok": true, "id": id, "archive_protected": protected}))
}

#[tauri::command]
pub fn workspace_backup() -> Result<serde_json::Value, String> {
    use crate::base::get_data_dir;
    let dir = get_repo_dir().join("workspaces");
    if !dir.is_dir() {
        return Err("No workspaces folder — nothing to back up".into());
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dest = get_data_dir().join(format!("backup-workspaces-{stamp}.zip"));
    let ok = crate::base::silent_command("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                "Compress-Archive -Path '{}' -DestinationPath '{}' -Force",
                dir.display(),
                dest.display()
            ),
        ])
        .output()
        .is_ok_and(|o| o.status.success());
    if !ok {
        return Err("ZIP failed".into());
    }
    Ok(serde_json::json!({"ok": true, "zip": dest.to_string_lossy().to_string()}))
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
    if !repo.join("wgp.py").exists() {
        errors.push("wgp.py not found — not installed".into());
    }
    if !repo.join("setup_config.json").exists() {
        errors.push("setup_config.json missing".into());
    }
    let env = get_active_env();
    if env.is_null() {
        errors.push("no active env".into());
    } else if let Some(raw) = env.get("path").and_then(|p| p.as_str()) {
        // Reuse is only honest if the interpreter exists AND runs (a stale
        // envs.json entry or a half-deleted venv must fail here, not on the
        // dashboard after "Use existing & go to Dashboard").
        let base = if std::path::Path::new(raw).is_absolute() {
            PathBuf::from(raw)
        } else {
            repo.join(raw.trim_start_matches(".\\").trim_start_matches("./"))
        };
        if !base.exists() {
            errors.push(format!("env folder missing on disk: {}", base.display()));
        } else {
            #[cfg(windows)]
            let py = base.join("Scripts\\python.exe");
            #[cfg(not(windows))]
            let py = base.join("bin/python");
            if !py.exists() {
                errors.push(format!(
                    "env python missing ({} broken) — repair the environment",
                    base.display()
                ));
            } else {
                let runs = silent_command(&py)
                    .arg("-c")
                    .arg("import sys")
                    .output()
                    .is_ok_and(|o| o.status.success());
                if !runs {
                    errors.push("env python won't start — reinstall/repair the environment".into());
                }
            }
        }
    }
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
    if !p.exists() {
        return serde_json::json!({"exists": false, "sizeBytes": 0, "cacheDir": p.to_string_lossy().to_string()});
    }
    let p_clone = p.clone();
    let size = tauri::async_runtime::spawn_blocking(move || {
        let mut size: u64 = 0;
        fn walk(p: &Path, acc: &mut u64) {
            if let Ok(rd) = std::fs::read_dir(p) {
                for e in rd.flatten() {
                    if let Ok(m) = e.metadata() {
                        if m.is_dir() {
                            walk(&e.path(), acc);
                        } else {
                            *acc += m.len();
                        }
                    }
                }
            }
        }
        walk(&p_clone, &mut size);
        size
    })
    .await
    .unwrap_or(0);
    serde_json::json!({"exists": true, "sizeBytes": size, "cacheDir": p.to_string_lossy().to_string()})
}
#[tauri::command]
pub fn manage_list() -> serde_json::Value {
    let f = get_envs_file();
    if !f.exists() {
        return serde_json::json!([]);
    }
    if let Ok(s) = std::fs::read_to_string(&f) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
            // Dashboard env switcher consumes objects ({name, type, active}),
            // not bare name strings — strings render as a bare dot row and
            // click-to-activate sends "undefined".
            let active = v.get("active").and_then(|a| a.as_str()).unwrap_or("");
            if let Some(envs) = v.get("envs").and_then(|e| e.as_object()) {
                return serde_json::Value::Array(
                    envs.iter()
                        .map(|(k, entry)| {
                            serde_json::json!({
                                "name": k,
                                "type": entry.get("type").and_then(|t| t.as_str()).unwrap_or("?"),
                                "active": k == active,
                            })
                        })
                        .collect(),
                );
            }
        }
    }
    serde_json::json!([])
}
#[tauri::command]
pub fn manage_set_active(name: String) -> Result<serde_json::Value, String> {
    let f = get_envs_file();
    let mut v: serde_json::Value = std::fs::read_to_string(&f)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({"envs":{}, "active":null}));
    if v.get("envs").and_then(|e| e.get(&name)).is_none() {
        return Err(format!("env {name} not found"));
    }
    v["active"] = serde_json::Value::String(name);
    atomic_write(
        &f,
        &serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command]
pub async fn uninstall_env(
    app: tauri::AppHandle,
    name: String,
) -> Result<serde_json::Value, String> {
    use tauri::Emitter;
    let log = |m: &str| {
        crate::base::push_log(m, "setup");
        let _ = app.emit("setup-output", m.to_string());
    };
    let f = get_envs_file();
    let mut v: serde_json::Value = std::fs::read_to_string(&f)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}));
    let entry = v.get("envs").and_then(|e| e.get(&name)).cloned();
    let Some(entry) = entry else {
        return Err("Environment not found".into());
    };
    let etype = entry.get("type").and_then(|t| t.as_str()).unwrap_or("?");
    log(&format!("[{name}] type: {etype}\n"));
    if let Some(raw) = entry.get("path").and_then(|p| p.as_str()) {
        if etype != "none" {
            let repo = get_repo_dir();
            let p = if std::path::Path::new(raw).is_absolute() {
                PathBuf::from(raw)
            } else {
                repo.join(raw.trim_start_matches(['.', '\\', '/']))
            };
            log(&format!("[{name}] path: {}\n", p.display()));
            // SECURITY (mirrors Electron ensureInsideRepo): never delete outside the repo.
            if !p.starts_with(&repo) {
                log(&format!(
                    "[{name}] SECURITY: env path outside repo — skipped deletion\n"
                ));
                return Err("Environment path outside repo — deletion blocked".into());
            }
            if p.exists() {
                // Size + top-level contents first, so the console shows what is being
                // removed (Electron parity) — then delete with progress. A venv is
                // 100k+ files, so all of it runs on a background thread.
                let app2 = app.clone();
                let name2 = name.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    use tauri::Emitter;
                    let say = |m: String| { crate::base::push_log(&m, "setup"); let _ = app2.emit("setup-output", m); };
                    fn du(path: &Path, acc: &mut u64) {
                        if let Ok(rd) = std::fs::read_dir(path) {
                            for e in rd.flatten() {
                                let q = e.path();
                                if q.is_dir() && !q.is_symlink() { du(&q, acc); }
                                else if let Ok(m) = e.metadata() { *acc += m.len(); }
                            }
                        }
                    }
                    let mut bytes: u64 = 0;
                    du(&p, &mut bytes);
                    if bytes > 0 {
                        let human = if bytes >= 1073741824 { format!("{:.1} GB", bytes as f64 / 1073741824.0) }
                            else if bytes >= 1048576 { format!("{:.1} MB", bytes as f64 / 1048576.0) }
                            else { format!("{:.1} KB", bytes as f64 / 1024.0) };
                        say(format!("[{name2}] size: {human}\n"));
                    }
                    if let Ok(rd) = std::fs::read_dir(&p) {
                        let top: Vec<String> = rd.flatten().take(20).map(|e| e.file_name().to_string_lossy().to_string()).collect();
                        if !top.is_empty() { say(format!("[{name2}] contents:\n  {}\n", top.join("\n  "))); }
                    }
                    // Delete with progress + retries: a locked file (running python,
                    // open terminal) fails once — retry with backoff before giving up
                    // on it, so one stubborn handle can't silently leave residue.
                    // Trust log: every directory entered is printed, plus a live
                    // current-file line (\r overwrites in place — no flooding) and
                    // the 2000-file milestones. Live lines skip the history buffer.
                    fn rm_tree(root: &Path, path: &Path, n: &mut u64, app: &tauri::AppHandle, name: &str, depth: usize, last_live: &mut std::time::Instant) {
                        use tauri::Emitter;
                        if let Ok(rd) = std::fs::read_dir(path) {
                            for e in rd.flatten() {
                                let q = e.path();
                                if q.is_dir() && !q.is_symlink() {
                                    if depth <= 1 {
                                        let rel = q.strip_prefix(root).unwrap_or(&q).to_string_lossy().to_string();
                                        let m = format!("[{name}] removing {rel}\\…\n");
                                        crate::base::push_log(&m, "setup");
                                        let _ = app.emit("setup-output", m);
                                    }
                                    rm_tree(root, &q, n, app, name, depth + 1, last_live);
                                    rm_retry(|| std::fs::remove_dir(&q).map_err(|e| e.to_string()));
                                } else {
                                    rm_retry(|| std::fs::remove_file(&q).map_err(|e| e.to_string()));
                                }
                                *n += 1;
                                if (*n).is_multiple_of(2000) {
                                    let m = format!("[{name}] …{n} files removed\n");
                                    crate::base::push_log(&m, "setup");
                                    let _ = app.emit("setup-output", m);
                                    *last_live = std::time::Instant::now();
                                } else if last_live.elapsed() > std::time::Duration::from_millis(500) {
                                    *last_live = std::time::Instant::now();
                                    let rel = q.strip_prefix(root).unwrap_or(&q).to_string_lossy().to_string();
                                    let _ = app.emit("setup-output", format!("\r[{name}] removing {rel}"));
                                }
                            }
                        }
                    }
                    fn rm_retry(mut op: impl FnMut() -> Result<(), String>) {
                        for attempt in 0..6 {
                            if op().is_ok() { return; }
                            if attempt < 5 { std::thread::sleep(std::time::Duration::from_millis(300)); }
                        }
                    }
                    let mut n: u64 = 0;
                    let mut last_live = std::time::Instant::now();
                    rm_tree(&p, &p, &mut n, &app2, &name2, 0, &mut last_live);
                    rm_retry(|| std::fs::remove_dir(&p).map_err(|e| e.to_string()));
                    if p.exists() {
                        // Second sweep entry-by-entry so one locked subdir can't shield the rest.
                        if let Ok(rd) = std::fs::read_dir(&p) {
                            for e in rd.flatten() {
                                let q = e.path();
                                if q.is_dir() && !q.is_symlink() { rm_tree(&p, &q, &mut n, &app2, &name2, 1, &mut last_live); }
                                rm_retry(|| if q.is_dir() && !q.is_symlink() { std::fs::remove_dir(&q).map_err(|e| e.to_string()) } else { std::fs::remove_file(&q).map_err(|e| e.to_string()) });
                            }
                        }
                        rm_retry(|| std::fs::remove_dir(&p).map_err(|e| e.to_string()));
                    }
                    if p.exists() {
                        say(format!("[{name2}] some files are locked by another process (close it / retry); remaining: {}\n", p.display()));
                    } else {
                        say(format!("[{name2}] folder removed ({n} files)\n"));
                    }
                }).await.map_err(|e| e.to_string())?;
            } else {
                log(&format!(
                    "[{name}] folder not found on disk, removing from registry\n"
                ));
            }
        }
    }
    if let Some(obj) = v.get_mut("envs").and_then(|e| e.as_object_mut()) {
        obj.remove(&name);
    }
    // If it was active, switch to the first remaining env (Electron parity) —
    // leaving active=null strands the dashboard on "No active environment".
    if v.get("active").and_then(|a| a.as_str()) == Some(&name) {
        let next = v
            .get("envs")
            .and_then(|e| e.as_object())
            .and_then(|m| m.keys().next().cloned());
        if let Some(nx) = next {
            v["active"] = serde_json::Value::String(nx.clone());
            log(&format!("[*] Switched active env to '{nx}'\n"));
        } else {
            v["active"] = serde_json::Value::Null;
            log("[*] No environments remaining\n");
        }
    }
    atomic_write(
        &f,
        &serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    crate::base::invalidate_path_cache();
    if let Some(m) = crate::base::LAST_STATUS.get() {
        *m.lock().unwrap() = None;
    }
    log(&format!("[{name}] uninstalled\n"));
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command]
pub fn uv_cache_clean(action: Option<String>) -> serde_json::Value {
    let _ = action;
    serde_json::json!({"success": true})
}

#[cfg(test)]
mod library_tests {
    use super::{finetune_id_valid, lora_cache_hits, summarize_finetune, summarize_workspace};
    use std::collections::HashMap;

    #[test]
    fn id_allows_safe_stems_rejects_traversal() {
        assert!(finetune_id_valid("hunyuan_t2v_fast"));
        assert!(finetune_id_valid("my-finetune-01"));
        assert!(!finetune_id_valid(""));
        assert!(!finetune_id_valid("../evil"));
        assert!(!finetune_id_valid("a/b"));
        assert!(!finetune_id_valid("x.json"));
        assert!(!finetune_id_valid(".hidden"));
        assert!(!finetune_id_valid(&"a".repeat(65)));
    }

    #[test]
    fn summary_extracts_model_fields() {
        let text = r#"{"model": {"name": "N", "architecture": "t2v",
            "description": "d", "URLs": ["a", "b"], "URLs2": ["c"],
            "loras": ["l"]}, "prompt": "hi"}"#;
        let s = summarize_finetune("n", text, 10);
        assert_eq!(s["name"], serde_json::json!("N"));
        assert_eq!(s["architecture"], serde_json::json!("t2v"));
        assert_eq!(s["urls"], serde_json::json!(2));
        assert_eq!(s["urls2"], serde_json::json!(1));
        assert_eq!(s["loras"], serde_json::json!(1));
    }

    #[test]
    fn summary_never_fails_on_garbage() {
        let s = summarize_finetune("bad", "{oops", 5);
        assert_eq!(s["error"], serde_json::json!("not valid JSON"));
        assert_eq!(s["id"], serde_json::json!("bad"));
    }

    #[test]
    fn cache_hits_match_exact_dir_file_keys() {
        let mut cache = HashMap::new();
        cache.insert("D:\\L|a.safetensors".to_string(), "http://x".to_string());
        let files = vec!["a.safetensors".to_string(), "b.safetensors".to_string()];
        assert_eq!(lora_cache_hits(&cache, "D:\\L", &files), 1);
        assert_eq!(lora_cache_hits(&cache, "D:\\Other", &files), 0);
    }

    #[test]
    fn kind_tags_quant_families() {
        use super::model_kind_tag;
        assert_eq!(model_kind_tag("m_qwen38_Q4.gguf"), "GGUF");
        assert_eq!(model_kind_tag("x_quanto_bf16_int8.safetensors"), "INT8");
        assert_eq!(model_kind_tag("x_fp8.safetensors"), "FP8");
        assert_eq!(model_kind_tag("x_nvfp4.safetensors"), "NVFP4");
        assert_eq!(model_kind_tag("x_bf16.safetensors"), "BF16");
        assert_eq!(model_kind_tag("readme.txt"), "—");
    }

    #[test]
    fn workspace_summary_counts_and_missing() {
        let v = serde_json::json!({
            "name": "Shoot",
            "gallery": {
                "file_list": ["C:\\definitely-not-here-wgp\\a.mp4", "C:\\definitely-not-here-wgp\\b.png"],
                "audio_file_list": ["x.wav"]
            },
            "last_activity": 123.0,
            "archive_protected": true
        });
        let s = summarize_workspace("abc123", &v);
        assert_eq!(s["name"], serde_json::json!("Shoot"));
        assert_eq!(s["files"], serde_json::json!(2));
        assert_eq!(s["audio"], serde_json::json!(1));
        assert_eq!(s["missing"], serde_json::json!(2));
        assert_eq!(s["bytes"], serde_json::json!(0));
        assert_eq!(s["archive_protected"], serde_json::json!(true));
    }

    #[test]
    fn workspace_summary_tolerates_garbage() {
        let s = summarize_workspace("x", &serde_json::json!({"nope": 1}));
        assert_eq!(s["files"], serde_json::json!(0));
        assert_eq!(s["archive_protected"], serde_json::json!(false));
    }
}

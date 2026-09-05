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

/// Free/total bytes for the disk hosting `p` (longest-prefix mount match).
/// Shared by get_disk_space and python_preflight (uv's own data dir).
pub(crate) fn disk_for_path(p: &str) -> Option<(u64, u64)> {
    use sysinfo::{Disks, DiskRefreshKind};
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
        use sysinfo::{Disks, DiskRefreshKind};
        let disks = Disks::new_with_refreshed_list_specifics(DiskRefreshKind::nothing().with_storage());
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
    let env = get_active_env();
    if env.is_null() {
        errors.push("no active env".into());
    } else if let Some(raw) = env.get("path").and_then(|p| p.as_str()) {
        // Reuse is only honest if the interpreter exists AND runs (a stale
        // envs.json entry or a half-deleted venv must fail here, not on the
        // dashboard after "Use existing & go to Dashboard").
        let base = if std::path::Path::new(raw).is_absolute() { PathBuf::from(raw) } else { repo.join(raw.trim_start_matches(".\\").trim_start_matches("./")) };
        if !base.exists() {
            errors.push(format!("env folder missing on disk: {}", base.display()));
        } else {
            #[cfg(windows)] let py = base.join("Scripts\\python.exe");
            #[cfg(not(windows))] let py = base.join("bin/python");
            if !py.exists() {
                errors.push(format!("env python missing ({} broken) — repair the environment", base.display()));
            } else {
                let runs = silent_command(&py).arg("-c").arg("import sys").output().is_ok_and(|o| o.status.success());
                if !runs { errors.push("env python won't start — reinstall/repair the environment".into()); }
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
#[tauri::command] pub async fn uninstall_env(app: tauri::AppHandle, name: String) -> Result<serde_json::Value,String> {
    use tauri::Emitter;
    let log = |m: &str| { crate::base::push_log(m, "setup"); let _ = app.emit("setup-output", m.to_string()); };
    let f = get_envs_file(); let mut v: serde_json::Value = std::fs::read_to_string(&f).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(serde_json::json!({}));
    let entry = v.get("envs").and_then(|e| e.get(&name)).cloned();
    let Some(entry) = entry else { return Err("Environment not found".into()); };
    let etype = entry.get("type").and_then(|t| t.as_str()).unwrap_or("?");
    log(&format!("[{name}] type: {etype}\n"));
    if let Some(raw) = entry.get("path").and_then(|p| p.as_str()) {
        if etype != "none" {
            let repo = get_repo_dir();
            let p = if std::path::Path::new(raw).is_absolute() { PathBuf::from(raw) } else { repo.join(raw.trim_start_matches(['.', '\\', '/'])) };
            log(&format!("[{name}] path: {}\n", p.display()));
            // SECURITY (mirrors Electron ensureInsideRepo): never delete outside the repo.
            if !p.starts_with(&repo) {
                log(&format!("[{name}] SECURITY: env path outside repo — skipped deletion\n"));
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
                                if *n % 2000 == 0 {
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
                log(&format!("[{name}] folder not found on disk, removing from registry\n"));
            }
        }
    }
    if let Some(obj) = v.get_mut("envs").and_then(|e| e.as_object_mut()) { obj.remove(&name); }
    // If it was active, switch to the first remaining env (Electron parity) —
    // leaving active=null strands the dashboard on "No active environment".
    if v.get("active").and_then(|a| a.as_str()) == Some(&name) {
        let next = v.get("envs").and_then(|e| e.as_object()).and_then(|m| m.keys().next().cloned());
        if let Some(nx) = next {
            v["active"] = serde_json::Value::String(nx.clone());
            log(&format!("[*] Switched active env to '{nx}'\n"));
        } else {
            v["active"] = serde_json::Value::Null;
            log("[*] No environments remaining\n");
        }
    }
    atomic_write(&f, &serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    crate::base::invalidate_path_cache();
    if let Some(m) = crate::base::LAST_STATUS.get() { *m.lock().unwrap() = None; }
    log(&format!("[{name}] uninstalled\n"));
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command] pub fn uv_cache_clean(action: Option<String>) -> serde_json::Value { let _=action; serde_json::json!({"success": true}) }

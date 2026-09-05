//! Folders, dialogs, data-dir management, reports, shortcuts, view shims.
use std::path::{Path, PathBuf};
use crate::base::*;
use crate::{hw::get_gpu_info_sync, status::get_active_env};

#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("Hello, {name}! You've been greeted from Rust!")
}

// Explorer windows this launcher opened (plus data/models roots) are closed
// again when the launcher exits — no orphaned windows left behind.
static OPENED_FOLDERS: std::sync::OnceLock<std::sync::Mutex<Vec<String>>> = std::sync::OnceLock::new();
#[tauri::command]
pub fn open_folder(path: String, app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    if let Ok(mut g) = OPENED_FOLDERS.get_or_init(|| std::sync::Mutex::new(Vec::new())).lock() {
        if !g.contains(&path) { g.push(path.clone()); }
    }
    app.opener().open_path(&path, None::<&str>).map_err(|e| e.to_string()).or_else(|_| silent_command("explorer").arg(&path).spawn().map(|_| ()).map_err(|e| e.to_string()))
}
#[tauri::command]
pub async fn select_folder(app: tauri::AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    // blocking_pick_folder is sync; pick_folder is async — try both
    if let Some(p) = app.dialog().file().blocking_pick_folder() { return Some(p.to_string()); }
    None
}
#[tauri::command]
pub async fn confirm_dialog(app: tauri::AppHandle, opts: Option<serde_json::Value>) -> serde_json::Value {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    let title = opts.as_ref().and_then(|o| o.get("title").and_then(|v| v.as_str())).unwrap_or("Confirm");
    let msg = opts.as_ref().and_then(|o| o.get("message").and_then(|v| v.as_str())).unwrap_or("Are you sure?");
    let detail = opts.as_ref().and_then(|o| o.get("detail").and_then(|v| v.as_str())).unwrap_or("");
    let full = if detail.is_empty() { msg.to_string() } else { format!("{msg}\n\n{detail}") };
    let confirmed = app.dialog().message(&full).title(title).kind(MessageDialogKind::Info).blocking_show();
    serde_json::json!({"response": i32::from(!confirmed)})
}
// Settings repair — port of services/settings-repair.js (Electron).
// Part 1: clamp dropdown values in models/_settings.json + every *_settings.json
// (stale values make Gradio reject the whole form on save). Part 2: fix model
// paths nested inside the repo (issue #18). Backs files up as *.bak-repair.
// Response shape matches what the Manage-tab handler renders.
#[tauri::command]
pub fn repair_settings() -> serde_json::Value {
    const CLAMPS: &[(&str, &[i64])] = &[
        ("apg_switch", &[0, 1]),
        ("cfg_star_switch", &[0, 1]),
        ("multi_images_gen_type", &[0, 1]),
    ];
    let repo = get_repo_dir();
    let mut files = Vec::new();
    let models = repo.join("models");
    if models.join("_settings.json").exists() { files.push(models.join("_settings.json")); }
    for dir in [&models, &repo.join("settings")] {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with("_settings.json") && n != "_settings.json") {
                    files.push(p);
                }
            }
        }
    }
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() { walk(&p, out); }
                else if p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with("_settings.json")) { out.push(p); }
            }
        }
    }
    walk(&repo.join("finetunes"), &mut files);
    let mut results = Vec::new();
    let mut problems = Vec::new();
    let mut fixed_total = 0;
    for f in &files {
        let raw = match std::fs::read_to_string(f) { Ok(s) => s, Err(e) => { problems.push(serde_json::json!({"file": f.display().to_string(), "error": e.to_string()})); continue; } };
        let stripped = raw.strip_prefix('\u{FEFF}').unwrap_or(&raw);
        let mut obj: serde_json::Value = match serde_json::from_str(stripped) {
            Ok(v) => v,
            Err(_) => { problems.push(serde_json::json!({"file": f.display().to_string(), "error": "invalid-json"})); continue; }
        };
        let map = match obj.as_object_mut() { Some(m) => m, None => { problems.push(serde_json::json!({"file": f.display().to_string(), "error": "invalid-shape"})); continue; } };
        let mut changed = 0;
        for (key, allowed) in CLAMPS {
            if let Some(v) = map.get_mut(*key) {
                if let Some(arr) = v.as_array_mut() {
                    for entry in arr.iter_mut() {
                        if let Some(ev) = entry.get("value").and_then(|x| x.as_i64()) {
                            if !allowed.contains(&ev) { entry["value"] = serde_json::json!(allowed[0]); changed += 1; }
                        }
                    }
                    continue;
                }
                if let Some(n) = v.as_i64() {
                    if !allowed.contains(&n) { *v = serde_json::json!(allowed[0]); changed += 1; }
                }
            }
        }
        if changed == 0 { continue; }
        let bak = PathBuf::from(format!("{}.bak-repair", f.display()));
        if !bak.exists() { let _ = std::fs::copy(f, &bak); }
        let eol = if raw.contains("\r\n") { "\r\n" } else { "\n" };
        match serde_json::to_string_pretty(&obj) {
            Ok(s) => {
                if std::fs::write(f, s.replace('\n', eol)).is_ok() {
                    fixed_total += changed;
                    results.push(serde_json::json!({"file": f.display().to_string(), "fixed": changed, "backup": bak.display().to_string()}));
                } else { problems.push(serde_json::json!({"file": f.display().to_string(), "error": "write failed"})); }
            }
            Err(_) => problems.push(serde_json::json!({"file": f.display().to_string(), "error": "serialize failed"})),
        }
    }
    // Part 2: nested model paths (issue #18)
    let mut replacements = Vec::new();
    let cfg_path = repo.join("wgp_config.json");
    if let Ok(raw) = std::fs::read_to_string(&cfg_path) {
        if let Ok(mut cfg) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(map) = cfg.as_object_mut() {
                let home = get_data_dir();
                let nested_root = repo.join("Wan2GP");
                let is_nested = |p: &str| -> bool {
                    if p.is_empty() { return false; }
                    let abs = if Path::new(p).is_absolute() { PathBuf::from(p) } else { repo.join(p) };
                    let (a, r) = (abs.to_string_lossy().to_lowercase(), nested_root.to_string_lossy().to_lowercase());
                    a == r || a.starts_with(&(r.clone() + "\\")) || a.starts_with(&(r + "/"))
                };
                let ck_def = home.join("ckpt").to_string_lossy().to_string();
                let lo_def = home.join("lora").to_string_lossy().to_string();
                let out_def = home.join("outputs").to_string_lossy().to_string();
                if let Some(arr) = map.get_mut("checkpoints_paths").and_then(|v| v.as_array_mut()) {
                    for p in arr.iter_mut() {
                        if let Some(s) = p.as_str() {
                            if is_nested(s) { replacements.push(serde_json::json!({"key": "checkpoints_paths", "from": s, "to": ck_def})); *p = serde_json::json!(ck_def); }
                        }
                    }
                }
                for (key, def) in [("loras_root", &lo_def), ("save_path", &out_def), ("image_save_path", &out_def), ("audio_save_path", &out_def)] {
                    if let Some(s) = map.get(key).and_then(|v| v.as_str()) {
                        if is_nested(s) { replacements.push(serde_json::json!({"key": key, "from": s, "to": def})); map.insert(key.to_string(), serde_json::json!(def)); }
                    }
                }
                if !replacements.is_empty() {
                    let bak = PathBuf::from(format!("{}.bak-repair", cfg_path.display()));
                    if !bak.exists() { let _ = std::fs::copy(&cfg_path, &bak); }
                    let eol = if raw.contains("\r\n") { "\r\n" } else { "\n" };
                    if let Ok(s) = serde_json::to_string_pretty(&cfg) { let _ = std::fs::write(&cfg_path, s.replace('\n', eol)); }
                }
            }
        }
    }
    serde_json::json!({
        "success": true,
        "fixed": fixed_total,
        "scanned": files.len(),
        "results": results,
        "problems": problems,
        "modelPaths": {"fixed": !replacements.is_empty(), "replacements": replacements}
    })
}
#[tauri::command] pub fn set_data_dir(dir: String) -> Result<serde_json::Value,String> {
    // ponytail: reject file paths pasted as folder (e.g. Temp\orca-paste-*.png)
    let p = PathBuf::from(dir.trim());
    if looks_like_file_path(&p.to_string_lossy()) {
        return Err("Please select a folder, not a file".into());
    }
    // Reject a bare drive root (X:\ or X:) — the UI auto-resolves those to
    // <root>\Wan2GP before calling, so this is defense-in-depth (Electron parity).
    {
        let s = p.to_string_lossy().trim().replace('/', "\\");
        let t = s.trim_end_matches('\\');
        let is_root = t.len() == 2 && t.as_bytes()[1] == b':' && t.as_bytes()[0].is_ascii_alphabetic();
        if is_root {
            return Err(format!("drive-root: pick a folder such as {}\\Wan2GP, not a bare drive root", t));
        }
    }
    let ov = data_dir_override_file(); atomic_write(&ov, p.to_string_lossy().as_ref()).map_err(|e| e.to_string())?; invalidate_path_cache();
    // User deliberately pointed elsewhere — drop the last-known-install marker
    // so first-run doesn't nag about the abandoned location.
    let marker = home_dir().join(".wan2gp-tauri-installed");
    if std::fs::read_to_string(&marker).ok().is_some_and(|s| s.trim() != p.to_string_lossy().trim()) { let _ = std::fs::remove_file(&marker); }
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command] pub fn reset_data_dir() -> serde_json::Value { let _=std::fs::remove_file(data_dir_override_file()); let _=std::fs::remove_file(home_dir().join(".wan2gp-tauri-installed")); invalidate_path_cache(); serde_json::json!({"ok": true}) }
#[tauri::command] pub fn migrate_to_preferred(choices: Option<serde_json::Value>) -> serde_json::Value { let _=choices; serde_json::json!({"ok": true}) }
/// Shared cross-device move used by move_folder and reinstall (model
/// relocation before wipe). Emits migration-progress 0-100 on the slow path.
pub(crate) async fn move_path_inner(app: &tauri::AppHandle, s: &Path, d: &Path) -> Result<serde_json::Value, String> {
    use tauri::Emitter;
    if !s.exists() { return Err("Source folder not found".into()); }
    // Fast path: same-volume rename (instant, no progress needed).
    if std::fs::rename(s, d).is_ok() && !s.exists() {
        return Ok(serde_json::json!({"ok": true, "success": true, "mode": "rename"}));
    }
    // Slow path: cross-device copy with live progress, then tolerant remove +
    // verify. Previously this was a silent blocking copy — the migration modal's
    // progress bar never moved and locked files left a silent half-state.
    if !s.is_dir() {
        if let Some(par) = d.parent() { std::fs::create_dir_all(par).map_err(|e| e.to_string())?; }
        std::fs::copy(s, d).map_err(|e| e.to_string())?;
        let (a, b) = (std::fs::metadata(s).map(|m| m.len()).unwrap_or(0), std::fs::metadata(d).map(|m| m.len()).unwrap_or(1));
        if a != b { return Err("Copy verification failed (size mismatch)".into()); }
        std::fs::remove_file(s).map_err(|e| e.to_string())?;
        return Ok(serde_json::json!({"ok": true, "success": true, "mode": "copy"}));
    }
    fn count(path: &Path, acc: &mut u64) {
        if let Ok(rd) = std::fs::read_dir(path) {
            for e in rd.flatten() {
                *acc += 1;
                let q = e.path();
                if q.is_dir() && !q.is_symlink() { count(&q, acc); }
            }
        }
    }
    fn copy_tree(src: &Path, dst: &Path, done: &mut u64, total: u64, app: &tauri::AppHandle) -> Result<(), String> {
        use tauri::Emitter;
        std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
        let entries = std::fs::read_dir(src).map_err(|e| e.to_string())?;
        for e in entries.flatten() {
            let q = e.path(); let t = dst.join(e.file_name());
            if q.is_dir() && !q.is_symlink() { copy_tree(&q, &t, done, total, app)?; }
            else { std::fs::copy(&q, &t).map_err(|e| e.to_string())?; }
            *done += 1;
            if *done % 100 == 0 && total > 0 {
                let _ = app.emit("migration-progress", (*done * 100 / total).min(100) as i64);
            }
        }
        Ok(())
    }
    fn rm_retry(op: impl Fn() -> std::io::Result<()>) {
        for attempt in 0..6 {
            if op().is_ok() { return; }
            if attempt < 5 { std::thread::sleep(std::time::Duration::from_millis(400)); }
        }
    }
    fn rm_tree(path: &Path) {
        if let Ok(rd) = std::fs::read_dir(path) {
            for e in rd.flatten() {
                let q = e.path();
                if q.is_dir() && !q.is_symlink() { rm_tree(&q); rm_retry(|| std::fs::remove_dir(&q)); }
                else { rm_retry(|| std::fs::remove_file(&q)); }
            }
        }
    }
    let (s2, d2, app2) = (s.to_path_buf(), d.to_path_buf(), app.clone());
    let r = tauri::async_runtime::spawn_blocking(move || -> Result<(u64, u64), String> {
        use tauri::Emitter;
        let mut total: u64 = 0;
        count(&s2, &mut total);
        let mut done: u64 = 0;
        copy_tree(&s2, &d2, &mut done, total, &app2)?;
        let _ = app2.emit("migration-progress", 100);
        // Verify: every source file landed at dst (compare counts).
        let (mut a, mut b) = (0u64, 0u64);
        count(&s2, &mut a); count(&d2, &mut b);
        Ok((a.min(b), a))
    }).await.map_err(|e| e.to_string())?;
    let (copied, total) = r.map_err(|e| e.to_string())?;
    if copied < total {
        return Err(format!("Only {copied}/{total} entries copied to {} — disk full or unreadable files? Source left untouched.", d.display()));
    }
    // Remove the source, tolerating locked files, then verify.
    let s3 = s.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || { rm_tree(&s3); rm_retry(|| std::fs::remove_dir(&s3)); }).await.map_err(|e| e.to_string())?;
    if s.exists() {
        return Err(format!("Files moved to {}, but the old folder couldn't be fully removed (files locked by a running program?) — close anything using {} and delete it manually. Your data is safe at the new location.", d.display(), s.display()));
    }
    Ok(serde_json::json!({"ok": true, "success": true, "mode": "copy", "entries": total}))
}
#[tauri::command] pub async fn move_folder(app: tauri::AppHandle, src: String, dst: String) -> Result<serde_json::Value,String> {
    move_path_inner(&app, &PathBuf::from(&src), &PathBuf::from(&dst)).await
}
/// Folder size with top-level breakdown — backs the reinstall backup dialog
/// ("Wan2GP can be big when models live inside the repo").
#[tauri::command] pub async fn folder_size(path: String) -> Result<serde_json::Value,String> {
    let p = PathBuf::from(&path);
    if !p.exists() { return Err("Folder not found".into()); }
    let out = tauri::async_runtime::spawn_blocking(move || {
        fn du(path: &Path, acc: &mut u64) {
            if let Ok(rd) = std::fs::read_dir(path) {
                for e in rd.flatten() {
                    let q = e.path();
                    if q.is_dir() && !q.is_symlink() { du(&q, acc); }
                    else if let Ok(m) = e.metadata() { *acc += m.len(); }
                }
            }
        }
        let mut total: u64 = 0;
        let mut entries: Vec<serde_json::Value> = Vec::new();
        if p.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&p) {
                for e in rd.flatten() {
                    let q = e.path();
                    let is_dir = q.is_dir() && !q.is_symlink();
                    let mut b: u64 = 0;
                    if is_dir { du(&q, &mut b); }
                    else { b = e.metadata().map(|m| m.len()).unwrap_or(0); }
                    total += b;
                    entries.push(serde_json::json!({"name": e.file_name().to_string_lossy().to_string(), "bytes": b, "isDir": is_dir}));
                }
            }
        } else {
            total = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
        }
        entries.sort_by(|a, b| b.get("bytes").and_then(|x| x.as_u64()).unwrap_or(0).cmp(&a.get("bytes").and_then(|x| x.as_u64()).unwrap_or(0)));
        (total, entries)
    }).await.map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"ok": true, "success": true, "path": path, "bytes": out.0, "entries": out.1}))
}
#[tauri::command] pub fn write_wgp_config(cfg: serde_json::Value) -> Result<serde_json::Value, String> {
    // ponytail: reject file-as-folder (Temp\orca-paste-*.png was pasted as folder)
    for key in ["checkpoints_paths","checkpointsPaths","ckpt_dir","loras_root","lorasRoot","lora_dir","save_path","savePath"] {
        if let Some(v) = cfg.get(key).and_then(|x| if x.is_array() { x.as_array().and_then(|a| a.first()) } else { Some(x) }).and_then(|x| x.as_str()) {
            if looks_like_file_path(v) {
                return Err(format!("Please select a folder, not a file for {key}: {v}"));
            }
        }
    }
    let repo = get_repo_dir();
    let p = repo.join("wgp_config.json");
    let mut cur: serde_json::Value = if p.exists() { std::fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(serde_json::json!({})) } else { serde_json::json!({}) };
    if let Some(obj) = cfg.as_object() {
        for (k,v) in obj {
            // ponytail: don't write camelCase directly — only canonical snake_case
            if k=="checkpointsPaths" || k=="lorasRoot" || k=="savePath" { continue; }
            cur[k] = v.clone();
        }
    } else if let Some(patch) = cfg.get("patch") { if let Some(o) = patch.as_object() { for (k,v) in o { if k=="checkpointsPaths"||k=="lorasRoot"||k=="savePath"{continue;} cur[k]=v.clone(); } } }
    if let Some(v) = cfg.get("checkpointsPaths") { cur["checkpoints_paths"] = v.clone(); }
    if let Some(v) = cfg.get("lorasRoot") { cur["loras_root"] = v.clone(); }
    if let Some(v) = cfg.get("savePath") { cur["save_path"] = v.clone(); cur["image_save_path"] = v.clone(); cur["audio_save_path"] = v.clone(); }
    // clean legacy camel/ckpt_dir leftovers from earlier builds
    if let Some(m) = cur.as_object_mut() { m.remove("checkpointsPaths"); m.remove("ckpt_dir"); m.remove("lora_dir"); m.remove("lorasRoot"); m.remove("savePath"); }
    let s = serde_json::to_string_pretty(&cur).map_err(|e| e.to_string())?;
    atomic_write(&p, &s).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command] pub fn report_issue() -> serde_json::Value {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or("report".into(), |d| d.as_secs().to_string());
    let bundle = get_data_dir().join(format!("report-{stamp}"));
    let _ = std::fs::create_dir_all(&bundle);
    let ver = env!("CARGO_PKG_VERSION");
    let gpu = get_gpu_info_sync();
    let mut lines = vec![
        format!("Wan2GP Tauri {} ", ver),
        format!("GPU: {} ({} {} MB)", gpu.get("name").and_then(|v| v.as_str()).unwrap_or("?"), gpu.get("vendor").and_then(|v| v.as_str()).unwrap_or("?"), gpu.get("vramMB").and_then(|v| v.as_str()).unwrap_or("0")),
        format!("OS: {} {}", std::env::consts::OS, std::env::consts::ARCH),
    ];
    if let Ok(s)=std::fs::read_to_string(get_data_dir().join("boot.log")) { lines.push("\n── boot.log ──".into()); lines.extend(s.lines().take(25).map(std::string::ToString::to_string)); }
    let _ = std::fs::write(bundle.join("system-info.txt"), lines.join("\n"));
    let eq = get_repo_dir().join("error_queue.zip");
    let had = eq.exists();
    if had { let _ = std::fs::copy(&eq, bundle.join("error_queue.zip")); }
    let zip = get_data_dir().join(format!("report-{stamp}.zip"));
    let zip_ok = silent_command("powershell").args(["-NoProfile","-Command", &format!("Compress-Archive -Path '{}\\*' -DestinationPath '{}' -Force", bundle.display(), zip.display())]).output().is_ok_and(|o| o.status.success());
    let zip_path = if zip_ok { zip.to_string_lossy().to_string() } else { String::new() };
    let open_path = if zip_ok { zip.to_string_lossy().to_string() } else { bundle.to_string_lossy().to_string() };
    #[cfg(windows)] { let _ = silent_command("explorer").arg(&open_path).spawn(); }
    serde_json::json!({"ok": true, "success": true, "logLines": 0, "zipPath": zip_path, "bundleDir": bundle.to_string_lossy().to_string(), "hadErrorQueue": had})
}
#[tauri::command] pub fn create_desktop_shortcut() -> serde_json::Value {
    // ponytail: Windows .lnk via WScript.Shell — mirrors Electron main.js:3923 (uses active env python)
    let env = get_active_env();
    if env.is_null() { return serde_json::json!({"ok": false, "error": "No active environment"}); }
    let repo = get_repo_dir();
    if !repo.join("wgp.py").exists() { return serde_json::json!({"ok": false, "error": "Wan2GP repo not found"}); }
    let raw = env.get("path").and_then(|p| p.as_str()).unwrap_or("");
    let base = if std::path::Path::new(raw).is_absolute() { PathBuf::from(raw) } else { repo.join(raw.trim_start_matches(".\\").trim_start_matches("./")) };
    let py = if cfg!(windows) { base.join("Scripts\\python.exe") } else { base.join("bin/python") };
    if !py.exists() { return serde_json::json!({"ok": false, "error": "Python not found"}); }
    let desktop = std::env::var("USERPROFILE").map_or(PathBuf::from("C:\\Users\\Public\\Desktop"), |p| PathBuf::from(p).join("Desktop"));
    let lnk = desktop.join("Wan2GP Tauri.lnk");
    let ps = format!("$s=New-Object -ComObject WScript.Shell; $l=$s.CreateShortcut('{}'); $l.TargetPath='{}'; $l.Arguments='wgp.py'; $l.WorkingDirectory='{}'; $l.Description='Wan2GP Tauri'; $l.Save()", lnk.display(), py.display(), repo.display());
    let ok = silent_command("powershell").args(["-NoProfile","-Command", &ps]).output().is_ok_and(|o| o.status.success());
    if ok { serde_json::json!({"ok": true, "success": true, "path": lnk.to_string_lossy().to_string()}) } else { serde_json::json!({"ok": false, "error": "Failed to create shortcut"}) }
}
#[tauri::command] pub fn create_browser_view(url: Option<String>, opts: Option<serde_json::Value>) -> serde_json::Value { let _=(url, opts); serde_json::json!({"ok": true}) }
#[tauri::command] pub fn destroy_browser_view() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] pub fn get_log_history() -> serde_json::Value {
    let lines = crate::base::LOG_HISTORY.get()
        .and_then(|m| m.lock().ok())
        .map(|g| g.clone())
        .unwrap_or_default();
    serde_json::Value::Array(lines.into_iter().map(|d| serde_json::json!({"data": d})).collect())
}
#[tauri::command] pub fn open_task_manager() -> Result<serde_json::Value,String> {
    #[cfg(windows)] { std::process::Command::new("taskmgr.exe").spawn().map_err(|e| e.to_string())?; }
    #[cfg(not(windows))] { silent_command("gnome-system-monitor").spawn().map_err(|e| e.to_string())?; }
    Ok(serde_json::json!({"ok": true, "success": true}))
}
// ui_mode persists across renderer reloads so a crash can restore the session.
// get_crash_recovery_info reports pending:true only when the saved mode says a
// server view was open AND the port still answers (stale mode files don't trigger).
#[tauri::command] pub fn get_crash_recovery_info() -> serde_json::Value {
    let mf = get_data_dir().join("ui_mode.json");
    let mode = std::fs::read_to_string(&mf).ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("mode").and_then(|m| m.as_str()).map(|s| s.to_string()))
        .unwrap_or_default();
    if mode != "app" && mode != "browser" {
        return serde_json::json!({"pending": false});
    }
    let port = load_config_value().get("serverPort").and_then(serde_json::Value::as_u64).unwrap_or(7861);
    let url = format!("http://localhost:{port}");
    let running = std::net::TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        std::time::Duration::from_millis(400)).is_ok();
    serde_json::json!({"pending": running, "mode": mode, "url": url, "serverRunning": running})
}
#[tauri::command] pub fn hide_browser_view() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] pub fn detach_browser_view() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] pub fn reattach_browser_view() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] pub fn create_term_view() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] pub fn destroy_term_view() -> serde_json::Value { serde_json::json!({"ok": true}) }
#[tauri::command] pub fn bv_navigate(action: String) -> serde_json::Value { let _=action; serde_json::json!({"ok": true}) }
#[tauri::command] pub fn bv_set_zoom(factor: f64) -> serde_json::Value { let _=factor; serde_json::json!({"ok": true}) }
#[tauri::command] pub fn bv_set_dock(dock: String) -> serde_json::Value { let _=dock; serde_json::json!({"ok": true}) }
#[tauri::command] pub fn is_data_dir_roaming() -> bool { false } // ponytail: Tauri uses isolated .wan2gp-tauri-data-dir + C:\Wan2GP — never roaming, hide pre-v3.0 warning (#05cbdb3)
#[tauri::command] pub fn migrate_choose() -> serde_json::Value {
    let ip = crate::config::get_install_paths();
    let mp = crate::config::get_model_paths();
    let data_dir = ip.get("dataDir").and_then(|v| v.as_str()).unwrap_or("").to_string();
    serde_json::json!({
        "dataDir": data_dir,
        "legacy": data_dir,
        "fromRoaming": false,
        "ckpts": mp.get("checkpoints").and_then(|v| v.as_str()).unwrap_or(""),
        "loras": mp.get("loras").and_then(|v| v.as_str()).unwrap_or(""),
        "output": mp.get("output").and_then(|v| v.as_str()).unwrap_or(""),
        "modelsDefault": ip.get("modelsDefault").and_then(|v| v.as_str()).unwrap_or("")
    })
}
#[tauri::command] pub fn notifier_ensure() -> serde_json::Value {
    // Make sure `apprise` is importable in the active env (needed for delivery).
    let probe = (|| {
        let env = get_active_env();
        let raw = env.get("path")?.as_str()?;
        let base = if std::path::Path::new(raw).is_absolute() { PathBuf::from(raw) } else { get_repo_dir().join(raw.trim_start_matches(".\\").trim_start_matches("./")) };
        let py = if cfg!(windows) { base.join("Scripts\\python.exe") } else { base.join("bin/python") };
        if !py.exists() { return None; }
        let has = silent_command(&py).args(["-c", "import apprise"]).output().is_ok_and(|o| o.status.success());
        Some((py, has))
    })();
    let Some((py, has)) = probe else {
        return serde_json::json!({"ok": false, "error": "No active Python environment"});
    };
    if has { return serde_json::json!({"ok": true, "already": true}); }
    match silent_command(&py).args(["-m", "pip", "install", "apprise"]).output() {
        Ok(o) if o.status.success() => serde_json::json!({"ok": true, "already": false}),
        Ok(o) => serde_json::json!({"ok": false, "error": format!("pip install apprise failed: {}", String::from_utf8_lossy(&o.stderr).trim())}),
        Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
    }
}
#[tauri::command] pub fn ui_mode_set(mode: Option<String>) -> serde_json::Value {
    let mf = get_data_dir().join("ui_mode.json");
    let _ = std::fs::write(&mf, serde_json::json!({"mode": mode}).to_string());
    serde_json::json!({"ok": true})
}
#[allow(dead_code)]
#[tauri::command] pub fn on_system_theme_change() -> serde_json::Value { serde_json::json!(null) }
// App-close cleanup: stop the Wan2GP server, stop our OpenCode server, close
// Explorer windows showing our data/model folders. Runs synchronously inside
// CloseRequested so everything is dead before the process exits.
pub(crate) fn shutdown_cleanup(app: &tauri::AppHandle) {
    let _ = crate::launch::stop_wangp(app.clone());
    crate::features::stop_opencode_server();
    #[cfg(windows)] {
        let mut roots: Vec<String> = vec![get_data_dir().to_string_lossy().to_string()];
        if let Ok(cfg) = std::fs::read_to_string(get_config_file()) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&cfg) {
                for k in ["modelCkptsPath", "modelLorasPath", "modelOutputPath"] {
                    if let Some(p) = v.get(k).and_then(|x| x.as_str()) {
                        if !p.is_empty() { roots.push(p.to_string()); }
                    }
                }
            }
        }
        if let Some(m) = OPENED_FOLDERS.get().and_then(|x| x.lock().ok()) {
            for p in m.iter() { if !roots.contains(p) { roots.push(p.clone()); } }
        }
        for root in &roots {
            let esc = root.replace('\'', "''");
            let ps = format!("$sh=New-Object -ComObject Shell.Application; foreach($w in @($sh.Windows())){{ try {{ $p=$w.Document.Folder.Self.Path; if($p -like '{esc}*'){{$w.Quit()}} }} catch {{}} }}");
            let _ = silent_command("powershell").args(["-NoProfile", "-Command", &ps]).output();
        }
    }
}

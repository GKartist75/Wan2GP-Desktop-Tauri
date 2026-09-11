//! Folders, dialogs, data-dir management, reports, shortcuts, view shims.
use crate::base::*;
use crate::{hw::get_gpu_info_sync, status::get_active_env};
use std::path::{Path, PathBuf};

#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("Hello, {name}! You've been greeted from Rust!")
}

#[tauri::command]
pub fn open_folder(path: String, app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    // Deliberately NOT tracked for close-on-exit: Explorer windows (even ones
    // we opened) and browsers are always left alone — exit cleanup stops only
    // our server processes (see shutdown_cleanup).
    app.opener()
        .open_path(&path, None::<&str>)
        .map_err(|e| e.to_string())
        .or_else(|_| {
            silent_command("explorer")
                .arg(&path)
                .spawn()
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
}
#[tauri::command]
pub async fn select_folder(app: tauri::AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    // blocking_pick_folder is sync; pick_folder is async — try both
    if let Some(p) = app.dialog().file().blocking_pick_folder() {
        return Some(p.to_string());
    }
    None
}
#[tauri::command]
pub async fn confirm_dialog(
    app: tauri::AppHandle,
    opts: Option<serde_json::Value>,
) -> serde_json::Value {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    let title = opts
        .as_ref()
        .and_then(|o| o.get("title").and_then(|v| v.as_str()))
        .unwrap_or("Confirm");
    let msg = opts
        .as_ref()
        .and_then(|o| o.get("message").and_then(|v| v.as_str()))
        .unwrap_or("Are you sure?");
    let detail = opts
        .as_ref()
        .and_then(|o| o.get("detail").and_then(|v| v.as_str()))
        .unwrap_or("");
    let full = if detail.is_empty() {
        msg.to_string()
    } else {
        format!("{msg}\n\n{detail}")
    };
    let confirmed = app
        .dialog()
        .message(&full)
        .title(title)
        .kind(MessageDialogKind::Info)
        .blocking_show();
    serde_json::json!({"response": i32::from(!confirmed)})
}
/// Reset a broken/outdated wgp_config.json: back it up, delete the original
/// so Wan2GP regenerates full defaults on next launch. Used when wgp.py dies
/// with `KeyError: '<key>'` — the file exists but misses keys the installed
/// wgp.py requires (partial write after a failed install, or an ancient
/// config after an update). Never edits values, so no silent misconfiguration.
#[tauri::command]
pub fn reset_wgp_config() -> Result<serde_json::Value, String> {
    let repo = get_repo_dir();
    let cfg = repo.join("wgp_config.json");
    if !cfg.exists() {
        return Err("wgp_config.json not found — nothing to reset".into());
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let bak = repo.join(format!("wgp_config.bak-{stamp}.json"));
    std::fs::copy(&cfg, &bak).map_err(|e| e.to_string())?;
    std::fs::remove_file(&cfg).map_err(|e| e.to_string())?;
    Ok(
        serde_json::json!({"ok": true, "success": true, "backup": bak.to_string_lossy().to_string()}),
    )
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
    if models.join("_settings.json").exists() {
        files.push(models.join("_settings.json"));
    }
    for dir in [&models, &repo.join("settings")] {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with("_settings.json") && n != "_settings.json")
                {
                    files.push(p);
                }
            }
        }
    }
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with("_settings.json"))
                {
                    out.push(p);
                }
            }
        }
    }
    walk(&repo.join("finetunes"), &mut files);
    let mut results = Vec::new();
    let mut problems = Vec::new();
    let mut fixed_total = 0;
    for f in &files {
        let raw = match std::fs::read_to_string(f) {
            Ok(s) => s,
            Err(e) => {
                problems.push(
                    serde_json::json!({"file": f.display().to_string(), "error": e.to_string()}),
                );
                continue;
            }
        };
        let stripped = raw.strip_prefix('\u{FEFF}').unwrap_or(&raw);
        let mut obj: serde_json::Value = match serde_json::from_str(stripped) {
            Ok(v) => v,
            Err(_) => {
                problems.push(
                    serde_json::json!({"file": f.display().to_string(), "error": "invalid-json"}),
                );
                continue;
            }
        };
        let map = match obj.as_object_mut() {
            Some(m) => m,
            None => {
                problems.push(
                    serde_json::json!({"file": f.display().to_string(), "error": "invalid-shape"}),
                );
                continue;
            }
        };
        let mut changed = 0;
        for (key, allowed) in CLAMPS {
            if let Some(v) = map.get_mut(*key) {
                if let Some(arr) = v.as_array_mut() {
                    for entry in arr.iter_mut() {
                        if let Some(ev) = entry.get("value").and_then(|x| x.as_i64()) {
                            if !allowed.contains(&ev) {
                                entry["value"] = serde_json::json!(allowed[0]);
                                changed += 1;
                            }
                        }
                    }
                    continue;
                }
                if let Some(n) = v.as_i64() {
                    if !allowed.contains(&n) {
                        *v = serde_json::json!(allowed[0]);
                        changed += 1;
                    }
                }
            }
        }
        if changed == 0 {
            continue;
        }
        let bak = PathBuf::from(format!("{}.bak-repair", f.display()));
        if !bak.exists() {
            let _ = std::fs::copy(f, &bak);
        }
        let eol = if raw.contains("\r\n") { "\r\n" } else { "\n" };
        match serde_json::to_string_pretty(&obj) {
            Ok(s) => {
                if std::fs::write(f, s.replace('\n', eol)).is_ok() {
                    fixed_total += changed;
                    results.push(serde_json::json!({"file": f.display().to_string(), "fixed": changed, "backup": bak.display().to_string()}));
                } else {
                    problems.push(serde_json::json!({"file": f.display().to_string(), "error": "write failed"}));
                }
            }
            Err(_) => problems.push(
                serde_json::json!({"file": f.display().to_string(), "error": "serialize failed"}),
            ),
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
                    if p.is_empty() {
                        return false;
                    }
                    let abs = if Path::new(p).is_absolute() {
                        PathBuf::from(p)
                    } else {
                        repo.join(p)
                    };
                    let (a, r) = (
                        abs.to_string_lossy().to_lowercase(),
                        nested_root.to_string_lossy().to_lowercase(),
                    );
                    a == r || a.starts_with(&(r.clone() + "\\")) || a.starts_with(&(r + "/"))
                };
                let ck_def = home.join("ckpt").to_string_lossy().to_string();
                let lo_def = home.join("lora").to_string_lossy().to_string();
                let out_def = home.join("outputs").to_string_lossy().to_string();
                if let Some(arr) = map
                    .get_mut("checkpoints_paths")
                    .and_then(|v| v.as_array_mut())
                {
                    for p in arr.iter_mut() {
                        if let Some(s) = p.as_str() {
                            if is_nested(s) {
                                replacements.push(serde_json::json!({"key": "checkpoints_paths", "from": s, "to": ck_def}));
                                *p = serde_json::json!(ck_def);
                            }
                        }
                    }
                }
                for (key, def) in [
                    ("loras_root", &lo_def),
                    ("save_path", &out_def),
                    ("image_save_path", &out_def),
                    ("audio_save_path", &out_def),
                ] {
                    if let Some(s) = map.get(key).and_then(|v| v.as_str()) {
                        if is_nested(s) {
                            replacements
                                .push(serde_json::json!({"key": key, "from": s, "to": def}));
                            map.insert(key.to_string(), serde_json::json!(def));
                        }
                    }
                }
                if !replacements.is_empty() {
                    let bak = PathBuf::from(format!("{}.bak-repair", cfg_path.display()));
                    if !bak.exists() {
                        let _ = std::fs::copy(&cfg_path, &bak);
                    }
                    let eol = if raw.contains("\r\n") { "\r\n" } else { "\n" };
                    if let Ok(s) = serde_json::to_string_pretty(&cfg) {
                        let _ = std::fs::write(&cfg_path, s.replace('\n', eol));
                    }
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
#[tauri::command]
pub fn set_data_dir(dir: String) -> Result<serde_json::Value, String> {
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
        let is_root =
            t.len() == 2 && t.as_bytes()[1] == b':' && t.as_bytes()[0].is_ascii_alphabetic();
        if is_root {
            return Err(format!(
                "drive-root: pick a folder such as {}\\Wan2GP, not a bare drive root",
                t
            ));
        }
    }
    let ov = data_dir_override_file();
    atomic_write(&ov, p.to_string_lossy().as_ref()).map_err(|e| e.to_string())?;
    invalidate_path_cache();
    // User deliberately pointed elsewhere — drop the last-known-install marker
    // so first-run doesn't nag about the abandoned location.
    let marker = home_dir().join(".wan2gp-tauri-installed");
    if std::fs::read_to_string(&marker)
        .ok()
        .is_some_and(|s| s.trim() != p.to_string_lossy().trim())
    {
        let _ = std::fs::remove_file(&marker);
    }
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command]
pub fn reset_data_dir() -> serde_json::Value {
    let _ = std::fs::remove_file(data_dir_override_file());
    let _ = std::fs::remove_file(home_dir().join(".wan2gp-tauri-installed"));
    invalidate_path_cache();
    serde_json::json!({"ok": true})
}
#[tauri::command]
pub fn migrate_to_preferred(choices: Option<serde_json::Value>) -> serde_json::Value {
    let _ = choices;
    serde_json::json!({"ok": true})
}
/// Shared cross-device move used by move_folder and reinstall (model
/// relocation before wipe). Emits migration-progress 0-100 on the slow path.
pub(crate) async fn move_path_inner(
    app: &tauri::AppHandle,
    s: &Path,
    d: &Path,
) -> Result<serde_json::Value, String> {
    if !s.exists() {
        return Err("Source folder not found".into());
    }
    // Fast path: same-volume rename (instant, no progress needed).
    if std::fs::rename(s, d).is_ok() && !s.exists() {
        return Ok(serde_json::json!({"ok": true, "success": true, "mode": "rename"}));
    }
    // Slow path: cross-device copy with live progress, then tolerant remove +
    // verify. Previously this was a silent blocking copy — the migration modal's
    // progress bar never moved and locked files left a silent half-state.
    if !s.is_dir() {
        if let Some(par) = d.parent() {
            std::fs::create_dir_all(par).map_err(|e| e.to_string())?;
        }
        std::fs::copy(s, d).map_err(|e| e.to_string())?;
        let (a, b) = (
            std::fs::metadata(s).map(|m| m.len()).unwrap_or(0),
            std::fs::metadata(d).map(|m| m.len()).unwrap_or(1),
        );
        if a != b {
            return Err("Copy verification failed (size mismatch)".into());
        }
        std::fs::remove_file(s).map_err(|e| e.to_string())?;
        return Ok(serde_json::json!({"ok": true, "success": true, "mode": "copy"}));
    }
    fn count(path: &Path, acc: &mut u64) {
        if let Ok(rd) = std::fs::read_dir(path) {
            for e in rd.flatten() {
                *acc += 1;
                let q = e.path();
                if q.is_dir() && !q.is_symlink() {
                    count(&q, acc);
                }
            }
        }
    }
    fn copy_tree(
        src: &Path,
        dst: &Path,
        done: &mut u64,
        total: u64,
        app: &tauri::AppHandle,
    ) -> Result<(), String> {
        use tauri::Emitter;
        std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
        let entries = std::fs::read_dir(src).map_err(|e| e.to_string())?;
        for e in entries.flatten() {
            let q = e.path();
            let t = dst.join(e.file_name());
            if q.is_dir() && !q.is_symlink() {
                copy_tree(&q, &t, done, total, app)?;
            } else {
                std::fs::copy(&q, &t).map_err(|e| e.to_string())?;
            }
            *done += 1;
            if (*done).is_multiple_of(100) && total > 0 {
                let _ = app.emit("migration-progress", (*done * 100 / total).min(100) as i64);
            }
        }
        Ok(())
    }
    fn rm_retry(op: impl Fn() -> std::io::Result<()>) {
        for attempt in 0..6 {
            if op().is_ok() {
                return;
            }
            if attempt < 5 {
                std::thread::sleep(std::time::Duration::from_millis(400));
            }
        }
    }
    fn rm_tree(path: &Path) {
        if let Ok(rd) = std::fs::read_dir(path) {
            for e in rd.flatten() {
                let q = e.path();
                if q.is_dir() && !q.is_symlink() {
                    rm_tree(&q);
                    rm_retry(|| std::fs::remove_dir(&q));
                } else {
                    rm_retry(|| std::fs::remove_file(&q));
                }
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
        count(&s2, &mut a);
        count(&d2, &mut b);
        Ok((a.min(b), a))
    })
    .await
    .map_err(|e| e.to_string())?;
    let (copied, total) = r.map_err(|e| e.to_string())?;
    if copied < total {
        return Err(format!("Only {copied}/{total} entries copied to {} — disk full or unreadable files? Source left untouched.", d.display()));
    }
    // Remove the source, tolerating locked files, then verify.
    let s3 = s.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        rm_tree(&s3);
        rm_retry(|| std::fs::remove_dir(&s3));
    })
    .await
    .map_err(|e| e.to_string())?;
    if s.exists() {
        return Err(format!("Files moved to {}, but the old folder couldn't be fully removed (files locked by a running program?) — close anything using {} and delete it manually. Your data is safe at the new location.", d.display(), s.display()));
    }
    Ok(serde_json::json!({"ok": true, "success": true, "mode": "copy", "entries": total}))
}
#[tauri::command]
pub async fn move_folder(
    app: tauri::AppHandle,
    src: String,
    dst: String,
) -> Result<serde_json::Value, String> {
    move_path_inner(&app, &PathBuf::from(&src), &PathBuf::from(&dst)).await
}
/// Move a just-downloaded gallery file out of ~/Downloads via a native
/// Save-As dialog (filename prefilled, location/folder chosen by the user).
/// The download click itself is untouchable (cross-origin Gradio iframe +
/// zero-UI WebView2 completion), so this runs from the arrival prompt's
/// Save-As button. `name` must be a bare filename — any path components
/// are stripped. `dir` optionally seeds the dialog at the last-used folder.
#[tauri::command]
pub fn save_downloaded_file(
    app: tauri::AppHandle,
    name: String,
    dir: Option<String>,
) -> Result<serde_json::Value, String> {
    use tauri_plugin_dialog::DialogExt;
    let safe: PathBuf = Path::new(&name)
        .file_name()
        .map(PathBuf::from)
        .filter(|f| !f.as_os_str().is_empty())
        .ok_or_else(|| "bad filename".to_string())?;
    let src = home_dir().join("Downloads").join(&safe);
    if !src.is_file() {
        return Err("file is no longer in Downloads (moved or deleted?)".into());
    }
    let mut dlg = app
        .dialog()
        .file()
        .set_file_name(safe.to_string_lossy().as_ref());
    if let Some(d) = dir.filter(|d| !d.trim().is_empty()) {
        let dp = PathBuf::from(&d);
        if dp.is_dir() {
            dlg = dlg.set_directory(dp);
        }
    }
    let dst = match dlg.blocking_save_file() {
        Some(p) => p.into_path().map_err(|e| e.to_string())?,
        None => return Ok(serde_json::json!({"ok": true, "cancelled": true})),
    };
    if dst == src {
        return Ok(
            serde_json::json!({"ok": true, "path": dst.to_string_lossy().to_string(), "unchanged": true}),
        );
    }
    if let Some(par) = dst.parent() {
        std::fs::create_dir_all(par).map_err(|e| e.to_string())?;
    }
    // Same volume = atomic rename; across drives fall back to copy + verify + remove.
    if std::fs::rename(&src, &dst).is_err() {
        std::fs::copy(&src, &dst).map_err(|e| e.to_string())?;
        let (a, b) = (
            std::fs::metadata(&src).map(|m| m.len()).unwrap_or(0),
            std::fs::metadata(&dst).map(|m| m.len()).unwrap_or(1),
        );
        if a != b {
            let _ = std::fs::remove_file(&dst);
            return Err(
                "copy verification failed (size mismatch) — original kept in Downloads".into(),
            );
        }
        std::fs::remove_file(&src).map_err(|e| e.to_string())?;
    }
    Ok(serde_json::json!({"ok": true, "path": dst.to_string_lossy().to_string()}))
}
/// True when `file_name` looks like a Wan2GP-issued download, so other
/// apps saving into ~/Downloads while the Desktop view happens to be open
/// never trigger the Save / Save As prompt:
/// - settings/queue/preset bundles (.zip/.json/.lset) always prompt —
///   export names carry a timestamp but preset/finetune names are free-form;
/// - gallery media must carry Wan2GP's timestamp (`-YYYY-MM-DD-HHhMMmSSs`)
///   or `_seedNNNN` marker (generated outputs, e.g.
///   `2026-08-22-17h51m35s_seed494753204_...jpg`).
/// Windows collision copies (`name (1).ext`) are unwrapped for matching;
/// the real on-disk name is still what gets reported.
fn is_wangp_download(file_name: &str) -> bool {
    const EXT_OK: &[&str] = &[
        "png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff", "jfif", "mp4", "mov", "avi",
        "mkv", "webm", "m4v", "mpeg", "mpg", "ogv", "wav", "mp3", "ogg", "flac", "zip", "json",
        "lset", "srt", "vtt", "txt",
    ];
    const SKIP: &[&str] = &[
        "tmp",
        "temp",
        "crdownload",
        "part",
        "partial",
        "download",
        "opdownload",
        "lock",
        "bak",
        "lnk",
    ];
    if file_name.starts_with('.') || file_name.starts_with("~$") {
        return false;
    }
    let ext = Path::new(file_name)
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_lowercase();
    if SKIP.contains(&ext.as_str()) || !EXT_OK.contains(&ext.as_str()) {
        return false;
    }
    if ext == "zip" || ext == "json" || ext == "lset" {
        return true;
    }
    let stem = file_name
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(file_name);
    let stem = strip_collision_suffix(stem);
    has_wangp_stamp(stem) || has_wangp_seed(stem)
}

/// Strip a trailing Windows ` (N)` collision suffix: `name (1)` -> `name`.
fn strip_collision_suffix(stem: &str) -> &str {
    if !stem.ends_with(')') {
        return stem;
    }
    if let Some(open) = stem.rfind(" (") {
        let inner = &stem[open + 2..stem.len() - 1];
        if !inner.is_empty() && inner.bytes().all(|b| b.is_ascii_digit()) {
            return &stem[..open];
        }
    }
    stem
}

/// Scan for `-YYYY-MM-DD-HHhMMmSSs` (e.g. `-2026-08-22-17h51m35s`).
fn has_wangp_stamp(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 22 {
        return false;
    }
    for i in 0..=(b.len() - 22) {
        if b[i] == b'-'
            && b[i + 1..i + 5].iter().all(|c| c.is_ascii_digit())
            && b[i + 5] == b'-'
            && b[i + 6..i + 8].iter().all(|c| c.is_ascii_digit())
            && b[i + 8] == b'-'
            && b[i + 9..i + 11].iter().all(|c| c.is_ascii_digit())
            && b[i + 11] == b'-'
            && b[i + 12..i + 14].iter().all(|c| c.is_ascii_digit())
            && b[i + 14] == b'h'
            && b[i + 15..i + 17].iter().all(|c| c.is_ascii_digit())
            && b[i + 17] == b'm'
            && b[i + 18..i + 20].iter().all(|c| c.is_ascii_digit())
            && b[i + 20] == b's'
        {
            return true;
        }
    }
    false
}

/// `_seed` immediately followed by a digit (e.g. `_seed494753204`).
fn has_wangp_seed(s: &str) -> bool {
    let low = s.to_lowercase();
    let mut rest = low.as_str();
    while let Some(i) = rest.find("_seed") {
        if rest
            .as_bytes()
            .get(i + 5)
            .is_some_and(|c| c.is_ascii_digit())
        {
            return true;
        }
        rest = &rest[i + 5..];
    }
    false
}
/// Wan2GP-issued files in ~/Downloads newer than `since_ms` (epoch millis).
/// WebView2 completes iframe downloads with zero UI, so saves look
/// broken while files pile up silently. The frontend polls this while the
/// Desktop view is open and pops a Save / Save As prompt per arrival.
/// Shape-gated by is_wangp_download() so other apps' downloads never
/// prompt. Best-effort: a relocated Downloads folder outside the profile
/// won't be seen.
#[tauri::command]
pub fn downloads_since(since_ms: i64) -> serde_json::Value {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(home_dir().join("Downloads")) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if !is_wangp_download(&name) {
                continue;
            }
            let ms = e
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            if ms > since_ms {
                out.push(serde_json::json!({"name": e.file_name().to_string_lossy().to_string(), "ms": ms}));
            }
        }
    }
    out.sort_by_key(|v| v.get("ms").and_then(|x| x.as_i64()).unwrap_or(0));
    serde_json::json!({"ok": true, "files": out})
}
/// Folder size with top-level breakdown — backs the reinstall backup dialog
/// ("Wan2GP can be big when models live inside the repo").
#[tauri::command]
pub async fn folder_size(path: String) -> Result<serde_json::Value, String> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err("Folder not found".into());
    }
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
    Ok(
        serde_json::json!({"ok": true, "success": true, "path": path, "bytes": out.0, "entries": out.1}),
    )
}
#[tauri::command]
pub fn write_wgp_config(cfg: serde_json::Value) -> Result<serde_json::Value, String> {
    // ponytail: reject file-as-folder (Temp\orca-paste-*.png was pasted as folder)
    for key in [
        "checkpoints_paths",
        "checkpointsPaths",
        "ckpt_dir",
        "loras_root",
        "lorasRoot",
        "lora_dir",
        "save_path",
        "savePath",
    ] {
        if let Some(v) = cfg
            .get(key)
            .and_then(|x| {
                if x.is_array() {
                    x.as_array().and_then(|a| a.first())
                } else {
                    Some(x)
                }
            })
            .and_then(|x| x.as_str())
        {
            if looks_like_file_path(v) {
                return Err(format!("Please select a folder, not a file for {key}: {v}"));
            }
        }
    }
    let repo = get_repo_dir();
    let p = repo.join("wgp_config.json");
    let mut cur: serde_json::Value = if p.exists() {
        std::fs::read_to_string(&p)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
    if let Some(obj) = cfg.as_object() {
        for (k, v) in obj {
            // ponytail: don't write camelCase directly — only canonical snake_case
            if k == "checkpointsPaths" || k == "lorasRoot" || k == "savePath" {
                continue;
            }
            cur[k] = v.clone();
        }
    } else if let Some(patch) = cfg.get("patch") {
        if let Some(o) = patch.as_object() {
            for (k, v) in o {
                if k == "checkpointsPaths" || k == "lorasRoot" || k == "savePath" {
                    continue;
                }
                cur[k] = v.clone();
            }
        }
    }
    if let Some(v) = cfg.get("checkpointsPaths") {
        cur["checkpoints_paths"] = v.clone();
    }
    if let Some(v) = cfg.get("lorasRoot") {
        cur["loras_root"] = v.clone();
    }
    if let Some(v) = cfg.get("savePath") {
        cur["save_path"] = v.clone();
        cur["image_save_path"] = v.clone();
        cur["audio_save_path"] = v.clone();
    }
    // clean legacy camel/ckpt_dir leftovers from earlier builds
    if let Some(m) = cur.as_object_mut() {
        m.remove("checkpointsPaths");
        m.remove("ckpt_dir");
        m.remove("lora_dir");
        m.remove("lorasRoot");
        m.remove("savePath");
    }
    let s = serde_json::to_string_pretty(&cur).map_err(|e| e.to_string())?;
    atomic_write(&p, &s).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command]
pub fn report_issue() -> serde_json::Value {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or("report".into(), |d| d.as_secs().to_string());
    let bundle = get_data_dir().join(format!("report-{stamp}"));
    let _ = std::fs::create_dir_all(&bundle);
    let ver = env!("CARGO_PKG_VERSION");
    let gpu = get_gpu_info_sync();
    let mut lines = vec![
        format!("Wan2GP Tauri {} ", ver),
        format!(
            "GPU: {} ({} {} MB)",
            gpu.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
            gpu.get("vendor").and_then(|v| v.as_str()).unwrap_or("?"),
            gpu.get("vramMB").and_then(|v| v.as_str()).unwrap_or("0")
        ),
        format!("OS: {} {}", std::env::consts::OS, std::env::consts::ARCH),
    ];
    if let Ok(s) = std::fs::read_to_string(get_data_dir().join("boot.log")) {
        lines.push("\n── boot.log ──".into());
        lines.extend(s.lines().take(25).map(std::string::ToString::to_string));
    }
    let _ = std::fs::write(bundle.join("system-info.txt"), lines.join("\n"));
    let eq = get_repo_dir().join("error_queue.zip");
    let had = eq.exists();
    if had {
        let _ = std::fs::copy(&eq, bundle.join("error_queue.zip"));
    }
    let zip = get_data_dir().join(format!("report-{stamp}.zip"));
    let zip_ok = silent_command("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                "Compress-Archive -Path '{}\\*' -DestinationPath '{}' -Force",
                bundle.display(),
                zip.display()
            ),
        ])
        .output()
        .is_ok_and(|o| o.status.success());
    let zip_path = if zip_ok {
        zip.to_string_lossy().to_string()
    } else {
        String::new()
    };
    let open_path = if zip_ok {
        zip.to_string_lossy().to_string()
    } else {
        bundle.to_string_lossy().to_string()
    };
    #[cfg(windows)]
    {
        let _ = silent_command("explorer").arg(&open_path).spawn();
    }
    serde_json::json!({"ok": true, "success": true, "logLines": 0, "zipPath": zip_path, "bundleDir": bundle.to_string_lossy().to_string(), "hadErrorQueue": had})
}
#[tauri::command]
pub fn create_desktop_shortcut() -> serde_json::Value {
    // ponytail: Windows .lnk via WScript.Shell — mirrors Electron main.js:3923 (uses active env python)
    let env = get_active_env();
    if env.is_null() {
        return serde_json::json!({"ok": false, "error": "No active environment"});
    }
    let repo = get_repo_dir();
    if !repo.join("wgp.py").exists() {
        return serde_json::json!({"ok": false, "error": "Wan2GP repo not found"});
    }
    let raw = env.get("path").and_then(|p| p.as_str()).unwrap_or("");
    let base = if std::path::Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        repo.join(raw.trim_start_matches(".\\").trim_start_matches("./"))
    };
    let py = if cfg!(windows) {
        base.join("Scripts\\python.exe")
    } else {
        base.join("bin/python")
    };
    if !py.exists() {
        return serde_json::json!({"ok": false, "error": "Python not found"});
    }
    let desktop = std::env::var("USERPROFILE")
        .map_or(PathBuf::from("C:\\Users\\Public\\Desktop"), |p| {
            PathBuf::from(p).join("Desktop")
        });
    let lnk = desktop.join("Wan2GP Tauri.lnk");
    let ps = format!("$s=New-Object -ComObject WScript.Shell; $l=$s.CreateShortcut('{}'); $l.TargetPath='{}'; $l.Arguments='wgp.py'; $l.WorkingDirectory='{}'; $l.Description='Wan2GP Tauri'; $l.Save()", lnk.display(), py.display(), repo.display());
    let ok = silent_command("powershell")
        .args(["-NoProfile", "-Command", &ps])
        .output()
        .is_ok_and(|o| o.status.success());
    if ok {
        serde_json::json!({"ok": true, "success": true, "path": lnk.to_string_lossy().to_string()})
    } else {
        serde_json::json!({"ok": false, "error": "Failed to create shortcut"})
    }
}
// ── Desktop embed: iframe vs native child webview ──
// Manage → Launch → "Desktop embed" persists `embedMode` in desktop-config.json:
// - "iframe" (default): Gradio renders in an <iframe> in the main window.
//   Zero native code; downloads land silently in ~/Downloads (polled).
// - "native" (experimental): Gradio renders in a real child Webview with an
//   on_download handler — downloads emit `download-started` / `download-finished`
//   events with the exact URL + path, so no polling and no filename shape-gate.
//   NOTE: a native child composites ABOVE the DOM (like Electron's BrowserView),
//   so the floating terminal / Manage panel must hide or shrink it (see app.js).
const GRADIO_VIEW_LABEL: &str = "wan2gp-view";

/// Staging area for native-embed downloads: bytes land here invisibly, then
/// save_staged_download pops the native Save-As dialog (browser-with-ask
/// behaviour). A dialog *before* the bytes would deadlock WebView2, hence
/// this order. Cancelled/orphaned files older than 7 days are swept on view
/// creation.
fn staging_dir() -> PathBuf {
    let d = get_data_dir().join(".pending-downloads");
    let _ = std::fs::create_dir_all(&d);
    d
}
/// Sweep staged downloads older than 7 days (cancelled dialogs, orphans).
fn sweep_staging() {
    let now = std::time::SystemTime::now();
    if let Ok(rd) = std::fs::read_dir(staging_dir()) {
        for e in rd.flatten() {
            let stale = e
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| now.duration_since(t).ok())
                .map(|d| d.as_secs() > 7 * 86400)
                .unwrap_or(false);
            if stale {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}
/// Unique sibling path: `name.ext`, `name (1).ext`, … (Windows-style).
fn unique_in(dir: &Path, fname: &str) -> PathBuf {
    let _ = std::fs::create_dir_all(dir);
    let p = dir.join(fname);
    if !p.exists() {
        return p;
    }
    let (stem, ext) = match fname.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (fname.to_string(), String::new()),
    };
    for n in 1..10000u32 {
        let q = dir.join(format!("{stem} ({n}){ext}"));
        if !q.exists() {
            return q;
        }
    }
    dir.join(format!(
        "{stem}-{}.{ext}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ))
}
/// Move src → dst (atomic rename, copy+verify+remove across drives).
fn move_file_verified(src: &Path, dst: &Path) -> Result<(), String> {
    if std::fs::rename(src, dst).is_err() {
        std::fs::copy(src, dst).map_err(|e| e.to_string())?;
        let (a, b) = (
            std::fs::metadata(src).map(|m| m.len()).unwrap_or(0),
            std::fs::metadata(dst).map(|m| m.len()).unwrap_or(1),
        );
        if a != b {
            let _ = std::fs::remove_file(dst);
            return Err("copy verification failed (size mismatch) — staged copy kept".into());
        }
        std::fs::remove_file(src).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn embed_mode(opts: Option<&serde_json::Value>) -> String {
    // Explicit frontend request wins (lets the UI detect a failed native
    // stand-up); otherwise fall back to the persisted Manage → Launch setting.
    if let Some(m) = opts
        .as_ref()
        .and_then(|o| o.get("mode"))
        .and_then(|v| v.as_str())
    {
        if m == "native" || m == "iframe" {
            return m.to_string();
        }
    }
    crate::base::load_config_value()
        .get("embedMode")
        .and_then(|v| v.as_str())
        .unwrap_or("native")
        .to_string()
}

#[tauri::command]
pub async fn create_browser_view(
    app: tauri::AppHandle,
    url: Option<String>,
    opts: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    use tauri::{Emitter, Manager};
    // iframe mode: the frontend builds the <iframe>; nothing to do natively.
    if embed_mode(opts.as_ref()) != "native" {
        let _ = opts;
        let _ = url;
        return Ok(serde_json::json!({"ok": true, "mode": "iframe"}));
    }
    // Native path — MUST be async: creating a WebView2 inside a sync command
    // deadlocks on Windows (tauri-apps/wry#583).
    let u = url.unwrap_or_else(|| "http://localhost:7861".to_string());
    let (x, y, w, h) = opts
        .as_ref()
        .and_then(|o| o.get("rect"))
        .map(|r| {
            (
                r.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0),
                r.get("y").and_then(|v| v.as_f64()).unwrap_or(44.0),
                r.get("w").and_then(|v| v.as_f64()).unwrap_or(1280.0),
                r.get("h").and_then(|v| v.as_f64()).unwrap_or(700.0),
            )
        })
        .unwrap_or((0.0, 44.0, 1280.0, 700.0));
    // Never stack two Gradio renderers (each holds 200–600MB) — close stale first.
    if let Some(v) = app.get_webview(GRADIO_VIEW_LABEL) {
        let _ = v.close();
    }
    sweep_staging();
    let window = app
        .get_window("main")
        .ok_or_else(|| "main window not found".to_string())?;
    let builder = tauri::webview::WebviewBuilder::new(
        GRADIO_VIEW_LABEL,
        tauri::WebviewUrl::External(u.parse().map_err(|e| format!("bad embed url: {e}"))?),
    )
    // Let WebView2/Gradio handle HTML5 drag & drop natively on Windows.
    // Without this, Tauri's file-drop handler swallows drops before the
    // page sees them (main window already opts out via
    // dragDropEnabled:false; the child must opt out per-view).
    .disable_drag_drop_handler()
    // Diagnostic: prove the Gradio page actually loads in the child (start vs
    // finish). If generation misbehaves, these lines tell load-failure apart
    // from app-failure.
    .on_page_load(|webview, payload| {
        use tauri::webview::PageLoadEvent;
        let _ = webview.emit("gradio-page-load", serde_json::json!({
            "url": payload.url().to_string(),
            "started": matches!(payload.event(), PageLoadEvent::Started) }));
    })
    .on_download(|webview, event| {
        use tauri::webview::DownloadEvent;
        match event {
            DownloadEvent::Requested { url, destination } => {
                // Stage invisibly; the native Save-As dialog pops on finish
                // (a blocking dialog HERE would deadlock WebView2).
                // Filename: prefer WebView2's suggestion (it honors the
                // server's Content-Disposition, i.e. the gallery's rendered
                // name — URL paths are often hashes). Fall back to URL
                // parsing, then a generic name.
                let dl = home_dir().join("Downloads");
                let suggested = if destination != &dl { destination.file_name().map(|n| n.to_string_lossy().to_string()) } else { None };
                let suggested = suggested.filter(|n| !n.is_empty());
                let from_url = url.path_segments().and_then(|mut s| s.next_back())
                    .filter(|s| !s.is_empty()).map(|s| s.to_string());
                let fname = suggested.or(from_url).unwrap_or_else(|| "wan2gp-download".into());
                let safe: PathBuf = Path::new(&fname).file_name()
                    .map(PathBuf::from).unwrap_or_else(|| PathBuf::from("wan2gp-download"));
                let staged = unique_in(&staging_dir(), &safe.to_string_lossy());
                let staged_name = staged.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| fname.clone());
                *destination = staged;
                let _ = webview.emit("download-started", serde_json::json!({"url": url.as_str(), "name": staged_name}));
                true
            }
            DownloadEvent::Finished { url, path, success } => {
                let _ = webview.emit("download-finished", serde_json::json!({
                    "url": url.as_str(),
                    "path": path.as_ref().map(|p| p.to_string_lossy().to_string()),
                    "name": path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()),
                    "success": success }));
                true
            }
            _ => true,
        }
    });
    let view = window
        .add_child(
            builder,
            tauri::LogicalPosition::new(x, y),
            tauri::LogicalSize::new(w, h),
        )
        .map_err(|e| e.to_string())?;
    let _ = view.show();
    Ok(serde_json::json!({"ok": true, "mode": "native"}))
}
// Native-view lifecycle. All gracefully no-op in iframe mode (no child exists).
// A native child composites ABOVE the DOM, so hide = fully hidden (there is no
// "keep visible under the terminal" like the iframe has) — docked terminal
// instead shrinks the child via bv_sync_bounds (see app.js syncTermEmbedPadding).
#[tauri::command]
pub fn destroy_browser_view(app: tauri::AppHandle) -> serde_json::Value {
    use tauri::Manager;
    if let Some(v) = app.get_webview(GRADIO_VIEW_LABEL) {
        let _ = v.close();
    }
    serde_json::json!({"ok": true})
}
#[tauri::command]
pub fn get_log_history() -> serde_json::Value {
    let lines = crate::base::LOG_HISTORY
        .get()
        .and_then(|m| m.lock().ok())
        .map(|g| g.clone())
        .unwrap_or_default();
    serde_json::Value::Array(
        lines
            .into_iter()
            .map(|d| serde_json::json!({"data": d}))
            .collect(),
    )
}
#[tauri::command]
pub fn open_task_manager() -> Result<serde_json::Value, String> {
    #[cfg(windows)]
    {
        std::process::Command::new("taskmgr.exe")
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(windows))]
    {
        silent_command("gnome-system-monitor")
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(serde_json::json!({"ok": true, "success": true}))
}
// ui_mode persists across renderer reloads so a crash can restore the session.
// get_crash_recovery_info reports pending:true only when the saved mode says a
// server view was open AND the port still answers (stale mode files don't trigger).
#[tauri::command]
pub fn get_crash_recovery_info() -> serde_json::Value {
    let mf = get_data_dir().join("ui_mode.json");
    let mode = std::fs::read_to_string(&mf)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            v.get("mode")
                .and_then(|m| m.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_default();
    if mode != "app" && mode != "browser" {
        return serde_json::json!({"pending": false});
    }
    let port = load_config_value()
        .get("serverPort")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(7861);
    let url = format!("http://localhost:{port}");
    let running = std::net::TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        std::time::Duration::from_millis(400),
    )
    .is_ok();
    serde_json::json!({"pending": running, "mode": mode, "url": url, "serverRunning": running})
}
#[tauri::command]
pub fn hide_browser_view(app: tauri::AppHandle) -> serde_json::Value {
    use tauri::Manager;
    if let Some(v) = app.get_webview(GRADIO_VIEW_LABEL) {
        let _ = v.hide();
    }
    serde_json::json!({"ok": true})
}
#[tauri::command]
pub fn detach_browser_view(app: tauri::AppHandle) -> serde_json::Value {
    use tauri::Manager;
    if let Some(v) = app.get_webview(GRADIO_VIEW_LABEL) {
        let _ = v.hide();
    }
    serde_json::json!({"ok": true})
}
#[tauri::command]
pub fn reattach_browser_view(app: tauri::AppHandle) -> serde_json::Value {
    use tauri::Manager;
    if let Some(v) = app.get_webview(GRADIO_VIEW_LABEL) {
        let _ = v.show();
    }
    serde_json::json!({"ok": true})
}
// ── Separate console window (floating terminal that works over NATIVE) ──
// A native child composites above all DOM, so a floating DOM console can never
// overlay Gradio — this OS-level window can (normal top-level z-order).
// term.html + term.js were built for exactly this (log history + live events +
// dock/close/export buttons); only the opener + missing bridges were stubs.
const TERM_LABEL: &str = "term";

#[tauri::command]
pub async fn create_term_view(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    // MUST be async: window creation inside a sync command deadlocks WebView2.
    if let Some(w) = app.get_window(TERM_LABEL) {
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(serde_json::json!({"ok": true, "open": true, "existing": true}));
    }
    let w = tauri::WebviewWindowBuilder::new(
        &app,
        TERM_LABEL,
        tauri::WebviewUrl::App("term.html".into()),
    )
    .title("Wan2GP Console")
    .inner_size(760.0, 520.0)
    .always_on_top(true)
    .focused(true)
    .build()
    .map_err(|e| e.to_string())?;
    let _ = w.show();
    Ok(serde_json::json!({"ok": true, "open": true}))
}
#[tauri::command]
pub fn destroy_term_view(app: tauri::AppHandle) -> serde_json::Value {
    use tauri::Manager;
    // No-op when no term window exists (iframe flows never open one).
    if let Some(w) = app.get_window(TERM_LABEL) {
        let _ = w.close();
    }
    serde_json::json!({"ok": true})
}
/// Toggle for the console button in native+floating mode (backend owns truth:
/// the window can also be closed via its X button, which the frontend can't see).
#[tauri::command]
pub async fn toggle_term_window(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    if app.get_window(TERM_LABEL).is_some() {
        destroy_term_view(app);
        return Ok(serde_json::json!({"ok": true, "open": false}));
    }
    create_term_view(app).await
}
/// Dock switch pressed INSIDE the term window: route to the main window (it
/// owns the DOM terminal + dock state). Main listens for `term-set-dock`.
#[tauri::command]
pub fn term_set_dock(app: tauri::AppHandle, dock: String) -> serde_json::Value {
    use tauri::{Emitter, Manager};
    if let Some(m) = app.get_window("main") {
        let _ = m.emit("term-set-dock", dock);
    }
    serde_json::json!({"ok": true})
}
/// Export the console buffer (term window button) via native Save dialog.
#[tauri::command]
pub fn export_logs(
    app: tauri::AppHandle,
    text: Option<String>,
) -> Result<serde_json::Value, String> {
    use tauri_plugin_dialog::DialogExt;
    let t = text.unwrap_or_default();
    if t.trim().is_empty() {
        return Err("nothing to export".into());
    }
    let dst = match app
        .dialog()
        .file()
        .set_file_name("wan2gp-console.log")
        .blocking_save_file()
    {
        Some(p) => p.into_path().map_err(|e| e.to_string())?,
        None => return Ok(serde_json::json!({"ok": true, "cancelled": true})),
    };
    std::fs::write(&dst, t).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"ok": true, "path": dst.to_string_lossy().to_string()}))
}
#[tauri::command]
pub fn bv_navigate(app: tauri::AppHandle, action: String) -> serde_json::Value {
    use tauri::Manager;
    // The UI only exposes Reload (no Back/Forward buttons — Tauri's Webview
    // has no history API, and history.back() can't run cross-origin anyway).
    if action == "reload" {
        if let Some(v) = app.get_webview(GRADIO_VIEW_LABEL) {
            let _ = v.reload();
        }
    }
    serde_json::json!({"ok": true})
}
#[tauri::command]
pub fn bv_set_zoom(app: tauri::AppHandle, factor: f64) -> serde_json::Value {
    use tauri::Manager;
    // Compositor zoom (IsZoomControlEnabled) — replaces the iframe's CSS-zoom hack.
    if let Some(v) = app.get_webview(GRADIO_VIEW_LABEL) {
        let _ = v.set_zoom(factor.clamp(0.25, 2.0));
    }
    serde_json::json!({"ok": true})
}
/// Reposition/resize the native child to the measured DOM rect (CSS px =
/// logical px). Called after unhide, on window resize (debounced in JS),
/// and when the docked terminal opens/closes (view shrinks side-by-side).
/// No-op in iframe mode.
#[tauri::command]
pub fn bv_sync_bounds(app: tauri::AppHandle, x: f64, y: f64, w: f64, h: f64) -> serde_json::Value {
    use tauri::Manager;
    if let Some(v) = app.get_webview(GRADIO_VIEW_LABEL) {
        if w > 10.0 && h > 10.0 {
            let _ = v.set_position(tauri::LogicalPosition::new(x, y));
            let _ = v.set_size(tauri::LogicalSize::new(w, h));
        }
    }
    serde_json::json!({"ok": true})
}
/// Legacy dock hook (Electron parity): the frontend now measures the real
/// terminal rect and calls bv_sync_bounds instead. Kept so existing callers
/// don't break; just re-asserts the native view is visible.
#[tauri::command]
pub fn bv_set_dock(app: tauri::AppHandle, dock: String) -> serde_json::Value {
    use tauri::Manager;
    let _ = dock;
    if let Some(v) = app.get_webview(GRADIO_VIEW_LABEL) {
        let _ = v.show();
    }
    serde_json::json!({"ok": true})
}
/// Finish a native-embed download, browser-with-ask style: the file sits in
/// staging (see on_download). Pop the native Save-As dialog prefilled with its
/// name and move it there. Cancel → keep it in ~/Downloads (unique name),
/// mirroring a browser's default location. `path` MUST live inside staging —
/// anything else is rejected (no arbitrary file-move primitive).
#[tauri::command]
pub fn save_staged_download(
    app: tauri::AppHandle,
    path: String,
    dir: Option<String>,
) -> Result<serde_json::Value, String> {
    use tauri_plugin_dialog::DialogExt;
    let canon_stage = staging_dir()
        .canonicalize()
        .unwrap_or_else(|_| staging_dir());
    let canon_src = PathBuf::from(&path)
        .canonicalize()
        .map_err(|_| "download is no longer available (moved or deleted?)".to_string())?;
    if !canon_src.starts_with(&canon_stage) {
        return Err("refusing to move a file outside the download staging area".into());
    }
    if !canon_src.is_file() {
        return Err("download is no longer available (moved or deleted?)".into());
    }
    let fname = canon_src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "wan2gp-download".into());
    let mut dlg = app.dialog().file().set_file_name(&fname);
    if let Some(d) = dir.filter(|d| !d.trim().is_empty()) {
        let dp = PathBuf::from(&d);
        if dp.is_dir() {
            dlg = dlg.set_directory(dp);
        }
    }
    let dst = match dlg.blocking_save_file() {
        Some(p) => p.into_path().map_err(|e| e.to_string())?,
        None => {
            let dst = unique_in(&home_dir().join("Downloads"), &fname);
            move_file_verified(&canon_src, &dst)?;
            return Ok(serde_json::json!({"ok": true, "cancelled": true,
                "path": dst.to_string_lossy(), "name": fname}));
        }
    };
    if dst == canon_src {
        return Ok(serde_json::json!({"ok": true, "unchanged": true,
            "path": dst.to_string_lossy().to_string()}));
    }
    if let Some(par) = dst.parent() {
        std::fs::create_dir_all(par).map_err(|e| e.to_string())?;
    }
    move_file_verified(&canon_src, &dst)?;
    Ok(serde_json::json!({"ok": true, "path": dst.to_string_lossy().to_string()}))
}
/// Mirror a main-window console line to the backend bus: stored in history
/// (later term windows load it) + emitted to all windows (the separate term
/// window appends it). Backend-echoed lines are excluded by the caller
/// (appendLog forward=false) so nothing arrives twice. Keeps floating,
/// docked and dashboard consoles identical.
#[tauri::command]
pub fn mirror_console(app: tauri::AppHandle, text: String) -> serde_json::Value {
    use tauri::Emitter;
    if !text.trim().is_empty() {
        crate::base::push_log(&text, "launch");
        let _ = app.emit("console-mirror", text);
    }
    serde_json::json!({"ok": true})
}
/// console: sums working-set of every msedgewebview2.exe plus the launcher
/// itself. sysinfo process scan (~10ms), no powershell spawn.
#[tauri::command]
pub fn webview_memory() -> serde_json::Value {
    use sysinfo::{ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let self_pid = std::process::id();
    let mut rows = Vec::new();
    let (mut wv_mb, mut wv_n, mut self_mb) = (0u64, 0u32, 0u64);
    for (pid, p) in sys.processes() {
        if pid.as_u32() == self_pid {
            self_mb = p.memory() / 1048576;
        }
        if p.name()
            .to_string_lossy()
            .to_lowercase()
            .contains("msedgewebview2")
        {
            let mb = p.memory() / 1048576;
            wv_mb += mb;
            wv_n += 1;
            rows.push(serde_json::json!({"pid": pid.as_u32(), "mb": mb}));
        }
    }
    rows.sort_by(|a, b| {
        b.get("mb")
            .and_then(|x| x.as_u64())
            .unwrap_or(0)
            .cmp(&a.get("mb").and_then(|x| x.as_u64()).unwrap_or(0))
    });
    rows.truncate(8);
    serde_json::json!({"ok": true, "webviewMb": wv_mb, "webviewProcs": wv_n, "launcherMb": self_mb, "top": rows})
}
#[tauri::command]
pub fn is_data_dir_roaming() -> bool {
    false
} // ponytail: Tauri uses isolated .wan2gp-tauri-data-dir + C:\Wan2GP — never roaming, hide pre-v3.0 warning (#05cbdb3)
#[tauri::command]
pub fn migrate_choose() -> serde_json::Value {
    let ip = crate::config::get_install_paths();
    let mp = crate::config::get_model_paths();
    let data_dir = ip
        .get("dataDir")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
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
#[tauri::command]
pub fn notifier_ensure() -> serde_json::Value {
    // Make sure `apprise` is importable in the active env (needed for delivery).
    let probe = (|| {
        let env = get_active_env();
        let raw = env.get("path")?.as_str()?;
        let base = if std::path::Path::new(raw).is_absolute() {
            PathBuf::from(raw)
        } else {
            get_repo_dir().join(raw.trim_start_matches(".\\").trim_start_matches("./"))
        };
        let py = if cfg!(windows) {
            base.join("Scripts\\python.exe")
        } else {
            base.join("bin/python")
        };
        if !py.exists() {
            return None;
        }
        let has = silent_command(&py)
            .args(["-c", "import apprise"])
            .output()
            .is_ok_and(|o| o.status.success());
        Some((py, has))
    })();
    let Some((py, has)) = probe else {
        return serde_json::json!({"ok": false, "error": "No active Python environment"});
    };
    if has {
        return serde_json::json!({"ok": true, "already": true});
    }
    match silent_command(&py)
        .args(["-m", "pip", "install", "apprise"])
        .output()
    {
        Ok(o) if o.status.success() => serde_json::json!({"ok": true, "already": false}),
        Ok(o) => {
            serde_json::json!({"ok": false, "error": format!("pip install apprise failed: {}", String::from_utf8_lossy(&o.stderr).trim())})
        }
        Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
    }
}
#[tauri::command]
pub fn ui_mode_set(mode: Option<String>) -> serde_json::Value {
    let mf = get_data_dir().join("ui_mode.json");
    let _ = std::fs::write(&mf, serde_json::json!({"mode": mode}).to_string());
    serde_json::json!({"ok": true})
}
#[allow(dead_code)]
#[tauri::command]
pub fn on_system_theme_change() -> serde_json::Value {
    serde_json::json!(null)
}
// App-close cleanup: stop the Wan2GP server and our OpenCode server only.
// Explorer and browser windows are NEVER touched here — even ones the
// launcher opened. Runs synchronously inside CloseRequested so our processes
// are dead before the app exits.
pub(crate) fn shutdown_cleanup(app: &tauri::AppHandle) {
    // NB: must be the SYNC blocking variant. stop_wangp() is async and
    // its Future would be dropped unpolled here (CloseRequested is sync),
    // silently skipping the kill and orphaning the server every close.
    let _ = crate::launch::stop_wangp_blocking(app.clone());
    crate::features::stop_opencode_server();
}

//! Wan2GP versions/changelog and the in-app updater.
use tauri::Emitter;
use crate::base::*;

#[tauri::command]
pub fn get_desktop_version() -> String { env!("CARGO_PKG_VERSION").to_string() }
#[tauri::command]
pub fn get_wangp_local_version() -> serde_json::Value {
    let repo = get_repo_dir();
    if !repo.join(".git").exists() { return serde_json::Value::Null; }
    
    let hash = silent_command("git").args(["rev-parse","HEAD"]).current_dir(&repo).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let date = silent_command("git").args(["log","-1","--format=%cI"]).current_dir(&repo).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    if hash.is_empty() { serde_json::Value::Null } else { serde_json::json!({"hash": hash, "date": date}) }
}
#[tauri::command]
pub fn get_desktop_git_info() -> serde_json::Value { get_wangp_local_version() }

// Shared GitHub client: native HTTP (no curl/powershell spawns), token auth,
// ETag caching so repeated checks don't burn rate limit.
static GH_ETAG: std::sync::OnceLock<std::sync::Mutex<Option<String>>> = std::sync::OnceLock::new();
static GH_CACHED_COMMITS: std::sync::OnceLock<std::sync::Mutex<Option<serde_json::Value>>> = std::sync::OnceLock::new();
fn gh_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("wan2gp-tauri")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}
fn gh_token() -> String {
    load_config_value().get("githubToken").and_then(|v| v.as_str()).unwrap_or("").trim().to_string()
}
#[tauri::command]
pub async fn get_wangp_upstream_info() -> serde_json::Value {
    let token = gh_token();
    let mut req = gh_client().get("https://api.github.com/repos/deepbeepmeep/Wan2GP/commits?per_page=10&sha=main")
        .header("Accept", "application/vnd.github.v3+json");
    if !token.is_empty() { req = req.header("Authorization", format!("token {token}")); }
    if let Some(m) = GH_ETAG.get().and_then(|x| x.lock().ok()).and_then(|g| g.clone()) {
        req = req.header("If-None-Match", m);
    }
    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => return serde_json::json!({"error": format!("Could not fetch updates — offline or GitHub unreachable ({e}). Add a GitHub token in Manage settings for a higher rate limit.")}),
    };
    if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
        if let Some(cached) = GH_CACHED_COMMITS.get().and_then(|m| m.lock().ok()).and_then(|g| g.clone()) {
            return cached;
        }
    }
    if !resp.status().is_success() {
        return serde_json::json!({"error": "Could not fetch updates — GitHub API rate limited or offline. Add a GitHub token in Manage settings."});
    }
    if let Some(etag) = resp.headers().get("etag").and_then(|v| v.to_str().ok()) {
        let etag = etag.to_string();
        if let Ok(mut g) = GH_ETAG.get_or_init(|| std::sync::Mutex::new(None)).lock() { *g = Some(etag); }
    }
    let v: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => return serde_json::json!({"error": "Could not parse GitHub response."}),
    };
    if let Some(arr) = v.as_array() {
        let commits: Vec<serde_json::Value> = arr.iter().map(|c| {
            let sha = c.get("sha").and_then(|s| s.as_str()).unwrap_or("");
            let commit = c.get("commit");
            let date = commit.and_then(|x| x.get("author")).and_then(|x| x.get("date")).and_then(|d| d.as_str()).unwrap_or("");
            let msg = commit.and_then(|x| x.get("message")).and_then(|m| m.as_str()).unwrap_or("").split('\n').next().unwrap_or("").to_string();
            let author = commit.and_then(|x| x.get("author")).and_then(|x| x.get("name")).and_then(|n| n.as_str()).unwrap_or("").to_string();
            serde_json::json!({"hash": sha, "date": date, "message": msg, "author": author})
        }).collect();
        if !commits.is_empty() {
            let out = serde_json::json!({"commits": commits});
            if let Ok(mut g) = GH_CACHED_COMMITS.get_or_init(|| std::sync::Mutex::new(None)).lock() { *g = Some(out.clone()); }
            return out;
        }
    }
    serde_json::json!({"error": "Could not fetch updates — GitHub API rate limited or offline. Add a GitHub token in Manage settings."})
}
#[tauri::command]
pub async fn get_wangp_version() -> serde_json::Value {
    // Mirrors Electron: parse the WanGP version from the upstream README.
    let url = "https://raw.githubusercontent.com/deepbeepmeep/Wan2GP/main/README.md";
    match gh_client().get(url).send().await {
        Err(_) => serde_json::Value::Null,
        Ok(r) => match r.text().await {
            Err(_) => serde_json::Value::Null,
            Ok(body) => {
                let lower = body.to_lowercase();
                let mut out: Option<String> = None;
                let mut i = 0;
                while let Some(pos) = lower[i..].find("wangp") {
                    let j = i + pos + 5;
                    let rest = body[j..].trim_start_matches(|c: char| c == ' ' || c == '\t' || c == '-');
                    let rest = rest.strip_prefix('v').unwrap_or(rest);
                    let mut ver = String::new();
                    for ch in rest.chars() {
                        if ch.is_ascii_digit() || ch == '.' { ver.push(ch); } else { break; }
                    }
                    let dots = ver.chars().filter(|&c| c == '.').count();
                    if (1..=2).contains(&dots) && !ver.is_empty() {
                        out = Some(ver.trim_matches('.').to_string());
                        break;
                    }
                    i = j;
                    if i >= lower.len() { break; }
                }
                out.map(serde_json::Value::String).unwrap_or(serde_json::Value::Null)
            }
        },
    }
}
// ── Legacy Electron launcher removal ──
// Finds the old Electron-based launcher (any DisplayName containing "wan2gp"
// that isn't this Tauri build) and runs its uninstaller silently. Only the app
// dir goes — Wan2GP repo, models, LoRAs, outputs and settings live outside it
// (and this build already follows the Electron data-dir pointer), so all data
// is kept.
// ── In-app updater (tauri-plugin-updater) ──
// Manual-only flow matching the frontend banner state machine: check emits
// checking → available{autoDownload:false} | up-to-date; Full Download runs
// download_update (downloading{percent} → downloaded); Install & Restart runs
// install_update (passive NSIS swap, app exits). Update metadata comes from
// latest.json on the public GitHub releases page.
static PENDING_UPDATE: std::sync::OnceLock<std::sync::Mutex<Option<tauri_plugin_updater::Update>>> = std::sync::OnceLock::new();
static DOWNLOADED_UPDATE: std::sync::OnceLock<std::sync::Mutex<Option<(String, Vec<u8>)>>> = std::sync::OnceLock::new();
pub(crate) fn pending_lock() -> &'static std::sync::Mutex<Option<tauri_plugin_updater::Update>> {
    PENDING_UPDATE.get_or_init(|| std::sync::Mutex::new(None))
}
pub(crate) fn downloaded_lock() -> &'static std::sync::Mutex<Option<(String, Vec<u8>)>> {
    DOWNLOADED_UPDATE.get_or_init(|| std::sync::Mutex::new(None))
}
#[tauri::command] pub async fn check_update(app: tauri::AppHandle, opts: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
    use tauri_plugin_updater::UpdaterExt;
    let _ = opts;
    let _ = app.emit("update-status", serde_json::json!({"status": "checking"}));
    let builder = app.updater_builder();
    match builder.build().map_err(|e| e.to_string())?.check().await {
        Err(e) => {
            let _ = app.emit("update-status", serde_json::json!({"status": "error", "message": e.to_string()}));
            Ok(serde_json::json!({"update": null, "error": e.to_string()}))
        }
        Ok(None) => {
            let _ = app.emit("update-status", serde_json::json!({"status": "up-to-date"}));
            Ok(serde_json::json!({"update": null}))
        }
        Ok(Some(u)) => {
            let ver = u.version.clone();
            if let Ok(mut g) = pending_lock().lock() { *g = Some(u); }
            // autoDownload:false — user explicitly presses Full Download (manual-only updates)
            let _ = app.emit("update-status", serde_json::json!({"status": "available", "version": ver, "autoDownload": false}));
            Ok(serde_json::json!({"update": {"version": ver}}))
        }
    }
}
#[tauri::command] pub async fn download_update(app: tauri::AppHandle, opts: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
    let _ = opts;
    // Reuse the checked update; re-check if the user jumped straight here.
    let has_pending = pending_lock().lock().map(|g| g.is_some()).unwrap_or(false);
    if !has_pending {
        check_update(app.clone(), None).await?;
    }
    let app2 = app.clone();
    let mut last_pct: i64 = -1;
    // Take the Update out of the stash for the download (put back on failure).
    let upd = pending_lock().lock().ok().and_then(|mut g| g.take());
    let Some(upd) = upd else {
        let _ = app.emit("update-status", serde_json::json!({"status": "error", "message": "No update available — check first"}));
        return Ok(serde_json::json!({"ok": false, "error": "no pending update"}));
    };
    let ver = upd.version.clone();
    match upd.download(|got, total| {
        if let Some(t) = total {
            if t > 0 {
                let pct = (got as u64 * 100 / t) as i64;
                if pct != last_pct {
                    last_pct = pct;
                    let _ = app2.emit("update-status", serde_json::json!({"status": "downloading", "percent": pct}));
                }
            }
        }
    }, || {}).await {
        Err(e) => {
            if let Ok(mut g) = pending_lock().lock() { *g = Some(upd); }
            let _ = app.emit("update-status", serde_json::json!({"status": "error", "message": e.to_string()}));
            Ok(serde_json::json!({"ok": false, "error": e.to_string()}))
        }
        Ok(bytes) => {
            if let Ok(mut g) = pending_lock().lock() { *g = Some(upd); }
            if let Ok(mut g) = downloaded_lock().lock() { *g = Some((ver.clone(), bytes)); }
            let _ = app.emit("update-status", serde_json::json!({"status": "downloaded", "version": ver}));
            Ok(serde_json::json!({"ok": true}))
        }
    }
}
#[tauri::command] pub fn install_update(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let dl = downloaded_lock().lock().ok().and_then(|mut g| g.take());
    let Some((ver, bytes)) = dl else {
        let _ = app.emit("update-status", serde_json::json!({"status": "error", "message": "Nothing downloaded — download first"}));
        return Ok(serde_json::json!({"ok": false, "error": "nothing downloaded"}));
    };
    let upd = pending_lock().lock().ok().and_then(|mut g| g.take());
    let Some(upd) = upd else { return Err("Update metadata lost — check and download again".into()); };
    // Windows: launches the passive NSIS installer and exits this process.
    upd.install(&bytes).map_err(|e| e.to_string())?;
    let _ = ver;
    Ok(serde_json::json!({"ok": true}))
}

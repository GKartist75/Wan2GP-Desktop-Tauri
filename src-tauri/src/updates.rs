//! Wan2GP versions/changelog and the in-app updater.
use crate::base::*;
use tauri::Emitter;

#[tauri::command]
pub fn get_desktop_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
#[tauri::command]
pub fn get_wangp_local_version() -> serde_json::Value {
    let repo = get_repo_dir();
    if !repo.join(".git").exists() {
        return serde_json::Value::Null;
    }

    let hash = silent_command("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&repo)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let date = silent_command("git")
        .args(["log", "-1", "--format=%cI"])
        .current_dir(&repo)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if hash.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::json!({"hash": hash, "date": date, "version": local_wangp_version(&repo)})
    }
}

/// The checkout's own version, read from `wgp.py`'s `WanGP_version` — the same
/// string upstream renders in its own window, so the dashboard and the running
/// Wan2GP can never disagree. None when `wgp.py` is missing or unparseable;
/// the caller then falls back to showing the commit alone.
fn local_wangp_version(repo: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(repo.join("wgp.py"))
        .ok()
        .and_then(|body| parse_wangp_source_version(&body))
}
#[tauri::command]
pub fn get_desktop_git_info() -> serde_json::Value {
    get_wangp_local_version()
}

// Shared GitHub client: native HTTP (no curl/powershell spawns), token auth,
// ETag caching so repeated checks don't burn rate limit.
static GH_ETAG: std::sync::OnceLock<std::sync::Mutex<Option<String>>> = std::sync::OnceLock::new();
static GH_CACHED_COMMITS: std::sync::OnceLock<std::sync::Mutex<Option<serde_json::Value>>> =
    std::sync::OnceLock::new();
fn gh_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("wan2gp-tauri")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}
fn gh_token() -> String {
    load_config_value()
        .get("githubToken")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}
#[tauri::command]
pub async fn get_wangp_upstream_info() -> serde_json::Value {
    let token = gh_token();
    let mut req = gh_client()
        .get("https://api.github.com/repos/deepbeepmeep/Wan2GP/commits?per_page=10&sha=main")
        .header("Accept", "application/vnd.github.v3+json");
    if !token.is_empty() {
        req = req.header("Authorization", format!("token {token}"));
    }
    if let Some(m) = GH_ETAG
        .get()
        .and_then(|x| x.lock().ok())
        .and_then(|g| g.clone())
    {
        req = req.header("If-None-Match", m);
    }
    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            return serde_json::json!({"error": format!("Could not fetch updates — offline or GitHub unreachable ({e}). Add a GitHub token in Manage settings for a higher rate limit.")})
        }
    };
    if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
        if let Some(cached) = GH_CACHED_COMMITS
            .get()
            .and_then(|m| m.lock().ok())
            .and_then(|g| g.clone())
        {
            return cached;
        }
    }
    if !resp.status().is_success() {
        return serde_json::json!({"error": "Could not fetch updates — GitHub API rate limited or offline. Add a GitHub token in Manage settings."});
    }
    if let Some(etag) = resp.headers().get("etag").and_then(|v| v.to_str().ok()) {
        let etag = etag.to_string();
        if let Ok(mut g) = GH_ETAG.get_or_init(|| std::sync::Mutex::new(None)).lock() {
            *g = Some(etag);
        }
    }
    let v: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => return serde_json::json!({"error": "Could not parse GitHub response."}),
    };
    if let Some(arr) = v.as_array() {
        let commits: Vec<serde_json::Value> = arr
            .iter()
            .map(|c| {
                let sha = c.get("sha").and_then(|s| s.as_str()).unwrap_or("");
                let commit = c.get("commit");
                let date = commit
                    .and_then(|x| x.get("author"))
                    .and_then(|x| x.get("date"))
                    .and_then(|d| d.as_str())
                    .unwrap_or("");
                let msg = commit
                    .and_then(|x| x.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("")
                    .split('\n')
                    .next()
                    .unwrap_or("")
                    .to_string();
                let author = commit
                    .and_then(|x| x.get("author"))
                    .and_then(|x| x.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string();
                serde_json::json!({"hash": sha, "date": date, "message": msg, "author": author})
            })
            .collect();
        if !commits.is_empty() {
            let out = serde_json::json!({"commits": commits});
            if let Ok(mut g) = GH_CACHED_COMMITS
                .get_or_init(|| std::sync::Mutex::new(None))
                .lock()
            {
                *g = Some(out.clone());
            }
            return out;
        }
    }
    serde_json::json!({"error": "Could not fetch updates — GitHub API rate limited or offline. Add a GitHub token in Manage settings."})
}
/// A git hash we are willing to hand to `merge-base --is-ancestor`: 4–40 hex
/// chars. Anything else is treated as unknown rather than passed to git.
fn valid_commit_hash(h: &str) -> bool {
    !(h.len() < 4 || h.len() > 40 || !h.chars().all(|c| c.is_ascii_hexdigit()))
}
#[tauri::command]
pub fn wangp_contains_commit(hash: String) -> serde_json::Value {
    // Containment probe for the dashboard "update available" dot and the Sync
    // behind-warning: pure hash equality (`tip != HEAD`) never clears once the
    // checkout carries a local merge commit — every launcher `git pull` that
    // can't fast-forward manufactures one, so HEAD permanently differs from
    // the upstream tip even when it CONTAINS it. `merge-base --is-ancestor`
    // answers the real question ("is upstream already merged here?").
    // Returns {contained: true/false/null}; null = unknown (bad hash, shallow
    // clone without the ancestors, git failure) so callers keep the legacy
    // equality behavior instead of hiding a real update.
    let h = hash.trim().to_lowercase();
    if !valid_commit_hash(&h) {
        return serde_json::json!({"contained": null});
    }
    let repo = get_repo_dir();
    if !repo.join(".git").exists() {
        return serde_json::json!({"contained": null});
    }
    match silent_command("git")
        .args(["merge-base", "--is-ancestor", &h, "HEAD"])
        .current_dir(&repo)
        .output()
    {
        Ok(o) if o.status.success() => serde_json::json!({"contained": true}),
        Ok(o) if o.status.code() == Some(1) => serde_json::json!({"contained": false}),
        _ => serde_json::json!({"contained": null}),
    }
}
#[tauri::command]
pub async fn get_wangp_version() -> serde_json::Value {
    // Upstream's own `WanGP_version`, the same field the local read uses. This
    // replaced a README scrape whose parser returned None for every current
    // upstream README (its first "WanGP" mentions carry no version token), so
    // the dashboard version label stayed permanently blank — a checkout four
    // majors behind upstream looked identical to a current one.
    let url = "https://raw.githubusercontent.com/deepbeepmeep/Wan2GP/main/wgp.py";
    match gh_client().get(url).send().await {
        Err(_) => serde_json::Value::Null,
        Ok(r) => match r.text().await {
            Err(_) => serde_json::Value::Null,
            Ok(body) => parse_wangp_source_version(&body)
                      .map(serde_json::Value::String)
                      .unwrap_or(serde_json::Value::Null)
      },
    }
}
/// Upstream's single source of truth for a checkout's version: the
/// `WanGP_version = "17.10"` assignment at the top of `wgp.py`. Anchored on the
/// assignment (not a bare mention) so `target_mmgp_version = "3.8.0"` and the
/// f-string interpolations further down the file can never be mistaken for it.
/// Pure — unit-tested without a network or a checkout.
fn parse_wangp_source_version(body: &str) -> Option<String> {
    for line in body.lines() {
        let Some(rest) = line.trim().strip_prefix("WanGP_version") else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let v = rest
            .trim()
            .trim_end_matches([';', ','])
            .trim()
            .trim_matches(|c: char| c == '"' || c == '\'')
            .trim();
        if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit() || c == '.') {
            return Some(v.to_string());
        }
    }
    None
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
static PENDING_UPDATE: std::sync::OnceLock<std::sync::Mutex<Option<tauri_plugin_updater::Update>>> =
    std::sync::OnceLock::new();
static DOWNLOADED_UPDATE: std::sync::OnceLock<std::sync::Mutex<Option<(String, Vec<u8>)>>> =
    std::sync::OnceLock::new();
pub(crate) fn pending_lock() -> &'static std::sync::Mutex<Option<tauri_plugin_updater::Update>> {
    PENDING_UPDATE.get_or_init(|| std::sync::Mutex::new(None))
}
pub(crate) fn downloaded_lock() -> &'static std::sync::Mutex<Option<(String, Vec<u8>)>> {
    DOWNLOADED_UPDATE.get_or_init(|| std::sync::Mutex::new(None))
}
#[tauri::command]
pub async fn check_update(
    app: tauri::AppHandle,
    opts: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    use tauri_plugin_updater::UpdaterExt;
    let _ = opts;
    let _ = app.emit("update-status", serde_json::json!({"status": "checking"}));
    let builder = app.updater_builder();
    match builder.build().map_err(|e| e.to_string())?.check().await {
        Err(e) => {
            let _ = app.emit(
                "update-status",
                serde_json::json!({"status": "error", "message": e.to_string()}),
            );
            Ok(serde_json::json!({"update": null, "error": e.to_string()}))
        }
        Ok(None) => {
            let _ = app.emit("update-status", serde_json::json!({"status": "up-to-date"}));
            Ok(serde_json::json!({"update": null}))
        }
        Ok(Some(u)) => {
            let ver = u.version.clone();
            if let Ok(mut g) = pending_lock().lock() {
                *g = Some(u);
            }
            // autoDownload:false — user explicitly presses Full Download (manual-only updates)
            let _ = app.emit(
                "update-status",
                serde_json::json!({"status": "available", "version": ver, "autoDownload": false}),
            );
            Ok(serde_json::json!({"update": {"version": ver}}))
        }
    }
}
#[tauri::command]
pub async fn download_update(
    app: tauri::AppHandle,
    opts: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
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
        let _ = app.emit(
            "update-status",
            serde_json::json!({"status": "error", "message": "No update available — check first"}),
        );
        return Ok(serde_json::json!({"ok": false, "error": "no pending update"}));
    };
    let ver = upd.version.clone();
    match upd
        .download(
            |got, total| {
                if let Some(t) = total {
                    if t > 0 {
                        let pct = (got as u64 * 100 / t) as i64;
                        if pct != last_pct {
                            last_pct = pct;
                            let _ = app2.emit(
                                "update-status",
                                serde_json::json!({"status": "downloading", "percent": pct}),
                            );
                        }
                    }
                }
            },
            || {},
        )
        .await
    {
        Err(e) => {
            if let Ok(mut g) = pending_lock().lock() {
                *g = Some(upd);
            }
            let _ = app.emit(
                "update-status",
                serde_json::json!({"status": "error", "message": e.to_string()}),
            );
            Ok(serde_json::json!({"ok": false, "error": e.to_string()}))
        }
        Ok(bytes) => {
            if let Ok(mut g) = pending_lock().lock() {
                *g = Some(upd);
            }
            if let Ok(mut g) = downloaded_lock().lock() {
                *g = Some((ver.clone(), bytes));
            }
            let _ = app.emit(
                "update-status",
                serde_json::json!({"status": "downloaded", "version": ver}),
            );
            Ok(serde_json::json!({"ok": true}))
        }
    }
}
#[tauri::command]
pub fn install_update(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let dl = downloaded_lock().lock().ok().and_then(|mut g| g.take());
    let Some((ver, bytes)) = dl else {
        let _ = app.emit("update-status", serde_json::json!({"status": "error", "message": "Nothing downloaded — download first"}));
        return Ok(serde_json::json!({"ok": false, "error": "nothing downloaded"}));
    };
    let upd = pending_lock().lock().ok().and_then(|mut g| g.take());
    let Some(upd) = upd else {
        return Err("Update metadata lost — check and download again".into());
    };
    // Windows: launches the passive NSIS installer and exits this process.
    upd.install(&bytes).map_err(|e| e.to_string())?;
    let _ = ver;
    Ok(serde_json::json!({"ok": true}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_hash_must_be_4_to_40_hex_chars() {
        assert!(valid_commit_hash("a1b2c3d"));
        assert!(valid_commit_hash(&"a".repeat(40)));
        // Abbreviations are legal git inputs.
        assert!(valid_commit_hash("a1b2"));
        assert!(!valid_commit_hash("abc"));
        assert!(!valid_commit_hash(&"a".repeat(41)));
        assert!(!valid_commit_hash("a1b2c3z"));
        assert!(!valid_commit_hash(""));
    }

    #[test]
    fn source_version_reads_the_wangp_version_assignment() {
        assert_eq!(
            parse_wangp_source_version("import os\nWanGP_version = \"17.10\"\n"),
            Some("17.10".to_string())
        );
        assert_eq!(
            parse_wangp_source_version("WanGP_version = \"13.0\""),
            Some("13.0".to_string())
        );
    }

    #[test]
    fn source_version_tolerates_spacing_and_single_quotes() {
        assert_eq!(
            parse_wangp_source_version("    WanGP_version='2.0'"),
            Some("2.0".to_string())
        );
        assert_eq!(
            parse_wangp_source_version("WanGP_version =\"1.2.3\","),
            Some("1.2.3".to_string())
        );
    }

    #[test]
    fn source_version_ignores_the_mmgp_pin_and_interpolations() {
        // Both lines sit near the assignment in a real wgp.py. Neither is the
        // checkout version: `target_mmgp_version` is a different name, and the
        // f-strings only interpolate the variable.
        let body = "target_mmgp_version = \"3.8.0\"\n\
                    WanGP_version = \"17.10\"\n\
                    return f\"WanGP v{WanGP_version} by DeepBeepMeep\"\n";
        assert_eq!(parse_wangp_source_version(body), Some("17.10".to_string()));
    }

    #[test]
    fn source_version_is_none_without_a_well_formed_assignment() {
        assert_eq!(parse_wangp_source_version(""), None);
        assert_eq!(parse_wangp_source_version("WanGP is great"), None);
        // A bare mention (no `=`) is not an assignment.
        assert_eq!(parse_wangp_source_version("WanGP_version"), None);
        // A non-numeric value is not a version.
        assert_eq!(parse_wangp_source_version("WanGP_version = \"dev\""), None);
    }
}

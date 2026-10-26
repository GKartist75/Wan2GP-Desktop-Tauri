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
        serde_json::json!({"hash": hash, "date": date})
    }
}
#[tauri::command]
pub fn get_desktop_git_info() -> serde_json::Value {
    get_wangp_local_version()
}

// Shared GitHub client: native HTTP (no curl/powershell spawns), token auth,
// ETag caching so repeated checks don't burn rate limit.
static GH_ETAG: std::sync::LazyLock<std::sync::Mutex<Option<String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(None));
static GH_CACHED_COMMITS: std::sync::LazyLock<std::sync::Mutex<Option<serde_json::Value>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(None));
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
    if let Some(m) = GH_ETAG.lock().ok().and_then(|g| g.clone()) {
        req = req.header("If-None-Match", m);
    }
    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            return serde_json::json!({"error": format!("Could not fetch updates — offline or GitHub unreachable ({e}). Add a GitHub token in Manage settings for a higher rate limit.")})
        }
    };
    if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
        if let Some(cached) = GH_CACHED_COMMITS.lock().ok().and_then(|g| g.clone()) {
            return cached;
        }
    }
    if !resp.status().is_success() {
        return serde_json::json!({"error": "Could not fetch updates — GitHub API rate limited or offline. Add a GitHub token in Manage settings."});
    }
    if let Some(etag) = resp.headers().get("etag").and_then(|v| v.to_str().ok()) {
        let etag = etag.to_string();
        if let Ok(mut g) = GH_ETAG.lock() {
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
            if let Ok(mut g) = GH_CACHED_COMMITS.lock() {
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
    // Mirrors Electron: parse the WanGP version from the upstream README.
    let url = "https://raw.githubusercontent.com/deepbeepmeep/Wan2GP/main/README.md";
    match gh_client().get(url).send().await {
        Err(_) => serde_json::Value::Null,
        Ok(r) => match r.text().await {
            Err(_) => serde_json::Value::Null,
            Ok(body) => parse_wangp_version(&body)
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null),
        },
    }
}
/// Scrape the WanGP version out of the upstream README, mirroring the Electron
/// launcher: find the first "wangp" whose following token starts with 1–2 dots
/// of digits. Pure — split out so the scan is unit-testable without a network.
fn parse_wangp_version(body: &str) -> Option<String> {
    let lower = body.to_lowercase();
    let mut out: Option<String> = None;
    let mut i = 0;
    while let Some(pos) = lower[i..].find("wangp") {
        let j = i + pos + 5;
        let rest = body[j..].trim_start_matches([' ', '\t', '-']);
        let rest = rest.strip_prefix('v').unwrap_or(rest);
        let mut ver = String::new();
        for ch in rest.chars() {
            if ch.is_ascii_digit() || ch == '.' {
                ver.push(ch);
            } else {
                break;
            }
        }
        let dots = ver.chars().filter(|&c| c == '.').count();
        if (1..=2).contains(&dots) && !ver.is_empty() {
            out = Some(ver.trim_matches('.').to_string());
            break;
        }
        i = j;
        if i >= lower.len() {
            break;
        }
    }
    out
}
// ── Legacy Electron launcher removal ──
// Finds the old Electron-based launcher (any DisplayName containing "wan2gp"
// that isn't this Tauri build) and runs its uninstaller silently. Only the app
// dir goes — Wan2GP repo, models, LoRAs, outputs and settings live outside it
// (and this build already follows the Electron data-dir pointer), so all data
// is kept.
// ── How this copy was installed, and what "update" can do about it ──
//
// The plugin can only swap a *packaged* app: NSIS/MSI on Windows, an AppImage
// or a .deb/.rpm on Linux, a .app bundle on macOS. A cargo build has no bundle
// to patch — on Linux the plugin would take the running executable as the
// AppImage and rename the build away — so it is never offered there. That path
// asks GitHub for the newest tag instead and tells the user how this copy
// updates (see `report_unmanaged`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LauncherInstall {
    AppImage,
    Deb,
    Rpm,
    WindowsBundle,
    MacApp,
    Source,
}

impl LauncherInstall {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::AppImage => "appimage",
            Self::Deb => "deb",
            Self::Rpm => "rpm",
            Self::WindowsBundle => "windows",
            Self::MacApp => "macapp",
            Self::Source => "source",
        }
    }
}

/// Map the bundler's compile-time bundle type (plus the AppImage runtime's
/// `APPIMAGE`) to the install kind. `None` with no AppImage means a plain
/// binary — a cargo build, a portable extraction, or `--appimage-extract-and-run`.
pub(crate) fn install_kind(bundle: Option<&str>, appimage: Option<&str>) -> LauncherInstall {
    match bundle.map(|b| b.trim().to_ascii_lowercase()).as_deref() {
        Some("appimage") => LauncherInstall::AppImage,
        Some("deb") => LauncherInstall::Deb,
        Some("rpm") => LauncherInstall::Rpm,
        Some("nsis") | Some("msi") => LauncherInstall::WindowsBundle,
        Some("app") => LauncherInstall::MacApp,
        _ => {
            if appimage.map(|p| !p.trim().is_empty()).unwrap_or(false) {
                LauncherInstall::AppImage
            } else {
                LauncherInstall::Source
            }
        }
    }
}

/// This process's install kind, from the bundler's compile-time marker and the
/// AppImage runtime environment.
pub(crate) fn launcher_install_kind() -> LauncherInstall {
    let bundle = tauri::utils::platform::bundle_type().map(|b| match b {
        tauri::utils::config::BundleType::Deb => "deb",
        tauri::utils::config::BundleType::Rpm => "rpm",
        tauri::utils::config::BundleType::AppImage => "appimage",
        tauri::utils::config::BundleType::Nsis => "nsis",
        tauri::utils::config::BundleType::Msi => "msi",
        tauri::utils::config::BundleType::App => "app",
        _ => "",
    });
    let appimage = std::env::var("APPIMAGE").ok();
    install_kind(bundle, appimage.as_deref())
}

/// The release asset this install kind should download, if the release carries
/// one. Preference matters: a release also holds `.AppImage.tar.gz` and
/// `.app.tar.gz` updater payloads, which are NOT what a user installs.
pub(crate) fn pick_release_asset(
    kind: LauncherInstall,
    assets: &[(String, String)],
) -> Option<(String, String)> {
    let ext = match kind {
        LauncherInstall::AppImage => ".AppImage",
        LauncherInstall::Deb => ".deb",
        LauncherInstall::Rpm => ".rpm",
        LauncherInstall::MacApp => ".dmg",
        LauncherInstall::WindowsBundle => "-setup.exe",
        LauncherInstall::Source => return None,
    };
    let prefer: &[&str] = match kind {
        LauncherInstall::AppImage | LauncherInstall::Deb | LauncherInstall::Rpm => {
            &["x86_64", "amd64"]
        }
        LauncherInstall::MacApp => &["aarch64", "arm64"],
        LauncherInstall::WindowsBundle => &["x64", "x86_64"],
        LauncherInstall::Source => &[],
    };
    let mut fallback: Option<&(String, String)> = None;
    let mut preferred: Option<&(String, String)> = None;
    for asset in assets {
        let name = asset.0.as_str();
        // Updater payloads share the suffix family; never offer them as a download.
        if name.ends_with(".tar.gz") || name.ends_with(".sig") || name.ends_with(".json") {
            continue;
        }
        if !name.ends_with(ext) {
            continue;
        }
        if fallback.is_none() {
            fallback = Some(asset);
        }
        if prefer.iter().any(|p| name.contains(p)) {
            preferred = Some(asset);
            break;
        }
    }
    preferred.or(fallback).map(|a| (a.0.clone(), a.1.clone()))
}

/// The command a user runs after downloading, for the kinds where the download
/// itself does not finish the job. `None` means "no shell step".
pub(crate) fn install_command(kind: LauncherInstall, asset: Option<&str>) -> Option<String> {
    let name = asset?;
    match kind {
        LauncherInstall::Deb => Some(format!("sudo apt install ./{name}")),
        LauncherInstall::Rpm => Some(format!("sudo rpm -Uvh {name}")),
        _ => None,
    }
}

// ── Newest launcher release, straight from the public GitHub API ──
//
// This is the honest fallback: source builds have no bundle to patch, and a
// release that carries no asset for this platform/arch makes the plugin fail
// with `TargetsNotFound` — neither is something the user can act on. Same
// client, token and ETag cache as the Wan2GP commit feed, so the 5-hourly poll
// stays off the rate limit.
const LAUNCHER_RELEASE_API: &str =
    "https://api.github.com/repos/GKartist75/Wan2GP-Desktop-Tauri/releases/latest";
static GH_RELEASE_ETAG: std::sync::LazyLock<std::sync::Mutex<Option<String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(None));
static GH_CACHED_RELEASE: std::sync::LazyLock<std::sync::Mutex<Option<serde_json::Value>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(None));

/// Shape the GitHub `releases/latest` payload for this install kind. Pure over
/// the response plus the kind, so the asset choice is unit-testable.
pub(crate) fn payload_for(kind: LauncherInstall, release: &serde_json::Value) -> serde_json::Value {
    let current = env!("CARGO_PKG_VERSION");
    let tag = release
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let version = tag.trim_start_matches('v').trim().to_string();
    let assets: Vec<(String, String)> = release
        .get("assets")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| {
                    Some((
                        a.get("name")?.as_str()?.to_string(),
                        a.get("browser_download_url")?.as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    let asset = pick_release_asset(kind, &assets);
    // Release notes are shown as PLAIN TEXT only — never parsed as markup.
    let notes: String = release
        .get("body")
        .and_then(|b| b.as_str())
        .unwrap_or_default()
        .chars()
        .take(4000)
        .collect();
    serde_json::json!({
        "currentVersion": current,
        "version": version,
        "available": !version.is_empty() && crate::hw::version_gt(&version, current),
        "htmlUrl": release.get("html_url").cloned().unwrap_or(serde_json::Value::Null),
        "notes": notes,
        "installKind": kind.as_str(),
        "assetName": asset.as_ref().map(|a| a.0.clone()),
        "assetUrl": asset.as_ref().map(|a| a.1.clone()),
        "command": install_command(kind, asset.as_ref().map(|a| a.0.as_str())),
    })
}

#[tauri::command]
pub async fn launcher_release_info() -> serde_json::Value {
    release_info().await
}

pub(crate) async fn release_info() -> serde_json::Value {
    let token = gh_token();
    let mut req = gh_client()
        .get(LAUNCHER_RELEASE_API)
        .header("Accept", "application/vnd.github.v3+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if !token.is_empty() {
        req = req.header("Authorization", format!("token {token}"));
    }
    if let Some(m) = GH_RELEASE_ETAG.lock().ok().and_then(|g| g.clone()) {
        req = req.header("If-None-Match", m);
    }
    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            return serde_json::json!({"error": format!("Could not reach GitHub — offline or blocked ({e}).")})
        }
    };
    if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
        if let Some(cached) = GH_CACHED_RELEASE.lock().ok().and_then(|g| g.clone()) {
            return cached;
        }
    }
    if !resp.status().is_success() {
        return serde_json::json!({"error": "Could not fetch the launcher release — GitHub API rate limited or offline. Add a GitHub token in Manage settings."});
    }
    if let Some(etag) = resp.headers().get("etag").and_then(|v| v.to_str().ok()) {
        let etag = etag.to_string();
        if let Ok(mut g) = GH_RELEASE_ETAG.lock() {
            *g = Some(etag);
        }
    }
    let raw: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => {
            return serde_json::json!({"error": "Could not parse the GitHub release response."})
        }
    };
    let out = payload_for(launcher_install_kind(), &raw);
    if let Ok(mut g) = GH_CACHED_RELEASE.lock() {
        *g = Some(out.clone());
    }
    out
}

/// No plugin install is possible here. Report the newest release and how this
/// copy updates: `unmanaged` when something newer exists, `up-to-date` when it
/// does not, `error` only when GitHub cannot be asked either.
async fn report_unmanaged(app: &tauri::AppHandle, why: Option<String>) -> serde_json::Value {
    let info = release_info().await;
    if let Some(err) = info.get("error").and_then(|v| v.as_str()) {
        let msg = match why {
            Some(w) => format!("{w} — {err}"),
            None => err.to_string(),
        };
        let _ = app.emit(
            "update-status",
            serde_json::json!({"status": "error", "message": msg}),
        );
        return serde_json::json!({"update": null, "error": msg});
    }
    if info
        .get("available")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        let url = info
            .get("assetUrl")
            .and_then(|v| v.as_str())
            .or_else(|| info.get("htmlUrl").and_then(|v| v.as_str()))
            .unwrap_or_default()
            .to_string();
        let _ = app.emit(
            "update-status",
            serde_json::json!({
                "status": "unmanaged",
                "version": info["version"],
                "installKind": info["installKind"],
                "assetName": info["assetName"],
                "url": url,
                "htmlUrl": info["htmlUrl"],
                "command": info["command"],
                "notes": info["notes"],
            }),
        );
        return serde_json::json!({
            "update": null,
            "available": {"version": info["version"], "installKind": info["installKind"], "url": url},
        });
    }
    let _ = app.emit("update-status", serde_json::json!({"status": "up-to-date"}));
    serde_json::json!({"update": null})
}

// ── In-app updater (tauri-plugin-updater) ──
// Manual-only flow matching the frontend banner state machine: check emits
// checking → available{autoDownload:false} | up-to-date; Full Download runs
// download_update (downloading{percent} → downloaded); Install & Restart runs
// install_update (passive NSIS swap, app exits). Update metadata comes from
// latest.json on the public GitHub releases page.
type PendingUpdate = std::sync::Mutex<Option<tauri_plugin_updater::Update>>;
type DownloadedUpdate = std::sync::Mutex<Option<(String, Vec<u8>)>>;
static PENDING_UPDATE: std::sync::LazyLock<PendingUpdate> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(None));
static DOWNLOADED_UPDATE: std::sync::LazyLock<DownloadedUpdate> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(None));
pub(crate) fn pending_lock() -> &'static PendingUpdate {
    &PENDING_UPDATE
}
pub(crate) fn downloaded_lock() -> &'static DownloadedUpdate {
    &DOWNLOADED_UPDATE
}
#[tauri::command]
pub async fn check_update(
    app: tauri::AppHandle,
    opts: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    use tauri_plugin_updater::UpdaterExt;
    let _ = opts;
    let kind = launcher_install_kind();
    let _ = app.emit("update-status", serde_json::json!({"status": "checking"}));
    // A source build has no packaged bundle for the plugin to patch: on Linux it
    // would treat the running executable as the AppImage and move the build
    // aside. Never hand it that path — ask GitHub and explain instead.
    if kind == LauncherInstall::Source {
        return Ok(report_unmanaged(&app, None).await);
    }
    let builder = match app.updater_builder().build() {
        Ok(b) => b,
        Err(e) => return Ok(report_unmanaged(&app, Some(e.to_string())).await),
    };
    match builder.check().await {
        Err(e) => {
            // `TargetsNotFound` is the normal state of a platform whose asset the
            // release does not carry yet — telling the user "error" is useless, so
            // report the release and the download path instead.
            Ok(report_unmanaged(&app, Some(e.to_string())).await)
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
                        let pct =
                            (got as u64).saturating_mul(100).checked_div(t).unwrap_or(0) as i64;
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
    // Belt and braces next to the check-time gate: the plugin's AppImage branch
    // renames whatever `APPIMAGE` (or the running executable) points at, which
    // for a cargo build is the build itself.
    if launcher_install_kind() == LauncherInstall::Source {
        let msg = "This launcher runs from a build directory — update it with git pull and a rebuild (README → Updating the launcher).";
        let _ = app.emit(
            "update-status",
            serde_json::json!({"status": "error", "message": msg}),
        );
        return Ok(serde_json::json!({"ok": false, "error": msg}));
    }
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
    // Linux/macOS: swaps the file in place and returns — the new version only
    // runs after a relaunch, so say so instead of leaving a stale process.
    upd.install(&bytes).map_err(|e| e.to_string())?;
    let restart_required = !cfg!(windows);
    if restart_required {
        let _ = app.emit(
            "update-status",
            serde_json::json!({"status": "installed", "version": ver}),
        );
    }
    Ok(serde_json::json!({"ok": true, "restartRequired": restart_required}))
}

/// Restart into the version that `install_update` just wrote. Unix only needs
/// this because the plugin cannot restart the process it replaced.
#[tauri::command]
pub fn relaunch_app(app: tauri::AppHandle) {
    app.restart();
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
    fn version_scrape_reads_the_token_immediately_after_wangp() {
        assert_eq!(
            parse_wangp_version("WanGP v1.3.5 released."),
            Some("1.3.5".to_string())
        );
        assert_eq!(
            parse_wangp_version("upstream WanGP 2.10 is current"),
            Some("2.10".to_string())
        );
    }

    #[test]
    fn version_scrape_tolerates_separators_and_case() {
        assert_eq!(parse_wangp_version("WANGP - 2.0"), Some("2.0".to_string()));
    }

    #[test]
    fn version_scrape_skips_prose_mentions_of_wangp() {
        // "WanGP is great" has no version token; the scan must move on to the
        // next occurrence rather than returning junk or giving up.
        assert_eq!(
            parse_wangp_version("WanGP is great. See WanGP v1.2.3 for details."),
            Some("1.2.3".to_string())
        );
    }

    #[test]
    fn version_scrape_needs_a_dotted_version_right_after_the_name() {
        // A single component carries no dot, so the scanner does not accept it.
        assert_eq!(parse_wangp_version("wanGP v3"), None);
        // "version" is consumed as the token, so its digits never count.
        assert_eq!(
            parse_wangp_version("# Wan2GP\n\nWanGP version v1.3.5 released."),
            None
        );
        // Three dots is not a version this scanner accepts.
        assert_eq!(parse_wangp_version("WanGP 1.2.3.4"), None);
    }

    #[test]
    fn version_scrape_returns_none_when_no_version_is_present() {
        assert_eq!(parse_wangp_version(""), None);
        assert_eq!(parse_wangp_version("WanGP is great"), None);
        // A trailing "wangp" with nothing after it must not panic.
        assert_eq!(parse_wangp_version("WanGP"), None);
    }

    fn assets(names: &[&str]) -> Vec<(String, String)> {
        names
            .iter()
            .map(|n| (n.to_string(), format!("https://example.test/{n}")))
            .collect()
    }

    #[test]
    fn install_kind_follows_the_bundler_marker() {
        assert_eq!(
            install_kind(Some("nsis"), None),
            LauncherInstall::WindowsBundle
        );
        assert_eq!(
            install_kind(Some("msi"), None),
            LauncherInstall::WindowsBundle
        );
        assert_eq!(
            install_kind(Some("appimage"), None),
            LauncherInstall::AppImage
        );
        assert_eq!(install_kind(Some("deb"), None), LauncherInstall::Deb);
        assert_eq!(install_kind(Some("rpm"), None), LauncherInstall::Rpm);
        assert_eq!(install_kind(Some("app"), None), LauncherInstall::MacApp);
        // Case and stray whitespace come from build scripts, not users.
        assert_eq!(
            install_kind(Some(" NSIS "), None),
            LauncherInstall::WindowsBundle
        );
    }

    #[test]
    fn install_kind_treats_a_bare_binary_as_source() {
        assert_eq!(install_kind(None, None), LauncherInstall::Source);
        assert_eq!(install_kind(None, Some("  ")), LauncherInstall::Source);
        // `--appimage-extract-and-run` keeps APPIMAGE set while the binary lives
        // in an extracted tree; the plugin still needs the AppImage file, and
        // APPIMAGE names it, so this stays an AppImage install.
        assert_eq!(
            install_kind(None, Some("/home/u/Wan2GP.AppImage")),
            LauncherInstall::AppImage
        );
    }

    #[test]
    fn asset_pick_never_offers_an_updater_payload() {
        // The release also carries `.AppImage.tar.gz` and `.app.tar.gz`; those are
        // updater payloads, not something a user installs.
        let a = assets(&[
            "Wan2GP.Desktop.Launcher_0.11.0_amd64.AppImage.tar.gz",
            "Wan2GP.Desktop.Launcher_0.11.0_amd64.AppImage",
            "latest.json",
            "Wan2GP.Desktop.Launcher_0.11.0_amd64.deb",
        ]);
        assert_eq!(
            pick_release_asset(LauncherInstall::AppImage, &a).map(|x| x.0),
            Some("Wan2GP.Desktop.Launcher_0.11.0_amd64.AppImage".to_string())
        );
        assert_eq!(
            pick_release_asset(LauncherInstall::Deb, &a).map(|x| x.0),
            Some("Wan2GP.Desktop.Launcher_0.11.0_amd64.deb".to_string())
        );
        // A kind with no matching asset reports nothing rather than the wrong file.
        assert_eq!(pick_release_asset(LauncherInstall::MacApp, &a), None);
        assert_eq!(pick_release_asset(LauncherInstall::Source, &a), None);
    }

    #[test]
    fn asset_pick_prefers_the_host_architecture() {
        let a = assets(&[
            "wan2gp_0.11.0_aarch64.dmg",
            "wan2gp_0.11.0_x64.dmg",
            "wan2gp_0.11.0_x64-setup.exe",
        ]);
        assert_eq!(
            pick_release_asset(LauncherInstall::MacApp, &a).map(|x| x.0),
            Some("wan2gp_0.11.0_aarch64.dmg".to_string())
        );
        assert_eq!(
            pick_release_asset(LauncherInstall::WindowsBundle, &a).map(|x| x.0),
            Some("wan2gp_0.11.0_x64-setup.exe".to_string())
        );
        // No arch match still yields the right file type.
        let b = assets(&["wan2gp_0.11.0_x86_64.rpm"]);
        assert_eq!(
            pick_release_asset(LauncherInstall::Rpm, &b).map(|x| x.0),
            Some("wan2gp_0.11.0_x86_64.rpm".to_string())
        );
    }

    #[test]
    fn only_deb_and_rpm_need_a_shell_step() {
        assert_eq!(
            install_command(LauncherInstall::Deb, Some("x.deb")),
            Some("sudo apt install ./x.deb".to_string())
        );
        assert_eq!(
            install_command(LauncherInstall::Rpm, Some("x.rpm")),
            Some("sudo rpm -Uvh x.rpm".to_string())
        );
        // AppImage and .dmg finish with a file move, and a source build has
        // nothing to download at all.
        assert_eq!(
            install_command(LauncherInstall::AppImage, Some("x.AppImage")),
            None
        );
        assert_eq!(
            install_command(LauncherInstall::MacApp, Some("x.dmg")),
            None
        );
        assert_eq!(install_command(LauncherInstall::Deb, None), None);
    }

    #[test]
    fn payload_compares_against_the_running_version() {
        let release = serde_json::json!({
            "tag_name": "v999.0.0",
            "html_url": "https://example.test/releases/v999.0.0",
            "body": "notes",
            "assets": [
                {"name": "wan2gp_999.0.0_amd64.deb", "browser_download_url": "https://example.test/w.deb"}
            ]
        });
        let out = payload_for(LauncherInstall::Deb, &release);
        assert_eq!(out["version"], "999.0.0");
        assert_eq!(out["available"], true);
        assert_eq!(out["installKind"], "deb");
        assert_eq!(out["assetName"], "wan2gp_999.0.0_amd64.deb");
        assert_eq!(
            out["command"],
            "sudo apt install ./wan2gp_999.0.0_amd64.deb"
        );
        // An older or equal tag is not an update.
        let old = serde_json::json!({"tag_name": "v0.0.1"});
        assert_eq!(
            payload_for(LauncherInstall::Source, &old)["available"],
            false
        );
        // Missing fields degrade to empty/None instead of panicking.
        let empty = serde_json::json!({});
        let out = payload_for(LauncherInstall::AppImage, &empty);
        assert_eq!(out["available"], false);
        assert_eq!(out["assetUrl"], serde_json::Value::Null);
    }
}

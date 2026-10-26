//! Wan2GP plugin management (list / install / enable).
//! Mirrors the in-app plugin manager's data model without reimplementing it:
//! - catalog: <repo>/plugins.json (remote registry: name/author/version/url)
//! - installed: <repo>/plugins/<id>/ (+ plugin_info.json for name/version)
//! - enabled: `enabled_plugins` in <repo>/wgp_config.json (+ SYSTEM_PLUGINS always on)
use crate::base::*;
use std::path::{Path, PathBuf};
use tauri::Emitter;

const SYSTEM_PLUGINS: &[&str] = &[
    "video_mask_creator",
    "guides",
    "configuration",
    "plugin_manager",
    "about",
];
// bundled with the Wan2GP repo itself — not uninstallable (like upstream: uninstallable=false).
const BUNDLED_PLUGINS: &[&str] = &[
    "downloads",
    "media_flow",
    "models_manager",
    "motion_designer",
    "sample",
];

// The launcher's pack/recipe UI is intentionally thin: Finetune Manager owns
// recipe editing, the registry, CivitAI, imports and missing-file downloads.
// Install a pinned, known-good tag by default; never float master.
const COMPANION_PLUGIN_ID: &str = "Wan2GP-finetune-manager";
const COMPANION_PLUGIN_URL: &str = "https://github.com/GKartist75/Wan2GP-finetune-manager.git";
const COMPANION_PLUGIN_REF: &str = "v3.7.0";

// repo dir name from a git URL (mirrors shared/utils/plugins.py plugin_id_from_url).
fn plugin_id_from_url(url: &str) -> String {
    let mut t = url.trim().to_string();
    if let Some(i) = t.find("github.com:") {
        t = format!("https://github.com/{}", &t[i + "github.com:".len()..]);
    }
    t = t.trim_end_matches('/').to_string();
    if t.to_lowercase().ends_with(".git") {
        t.truncate(t.len() - 4);
    }
    t.rsplit('/').next().unwrap_or("").trim().to_string()
}

fn read_wgp_config(repo: &Path) -> serde_json::Value {
    let p = repo.join("wgp_config.json");
    if p.exists() {
        if let Ok(s) = std::fs::read_to_string(&p) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                return v;
            }
        }
    }
    serde_json::json!({})
}

fn str_list(v: &serde_json::Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|e| e.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn read_local_catalog(repo: &Path) -> std::collections::HashMap<String, serde_json::Value> {
    let mut map = std::collections::HashMap::new();
    if let Ok(s) = std::fs::read_to_string(repo.join("plugins_local.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
            let arr = if v.is_array() {
                v.as_array().cloned().unwrap_or_default()
            } else {
                vec![v]
            };
            for e in arr {
                let url = e.get("url").and_then(|v| v.as_str()).unwrap_or("");
                let id = plugin_id_from_url(url);
                if !id.is_empty() {
                    map.insert(id, e);
                }
            }
        }
    }
    map
}

fn str_field(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}

#[tauri::command(async)]
pub fn plugins_list() -> serde_json::Value {
    let repo = get_repo_dir();
    if !repo.join("wgp.py").exists() {
        return serde_json::json!({"ok": false, "error": "Wan2GP not installed"});
    }
    let catalog: Vec<serde_json::Value> = std::fs::read_to_string(repo.join("plugins.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let cfg = read_wgp_config(&repo);
    let enabled = str_list(&cfg, "enabled_plugins");
    // Wan2GP's own refreshed library (written by its plugin manager AND by us —
    // same file, same schema): fresher metadata wins over the shipped plugins.json.
    let local_cat = read_local_catalog(&repo);
    // local scan: every plugins/<dir> with plugin.py or plugin_info.json
    let mut local: std::collections::HashMap<String, (String, String, String, String)> =
        Default::default(); // id -> (name, version, date, author)
    if let Ok(rd) = std::fs::read_dir(repo.join("plugins")) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            let id = e.file_name().to_string_lossy().to_string();
            if id.starts_with('.') || id == "__pycache__" {
                continue;
            }
            if !p.join("plugin.py").exists() && !p.join("plugin_info.json").exists() {
                continue;
            }
            let (mut name, mut ver, mut date, mut author) =
                (id.clone(), String::new(), String::new(), String::new());
            if let Ok(s) = std::fs::read_to_string(p.join("plugin_info.json")) {
                if let Ok(info) = serde_json::from_str::<serde_json::Value>(&s) {
                    if let Some(n) = info.get("name").and_then(|v| v.as_str()) {
                        name = n.to_string();
                    }
                    if let Some(v) = info.get("version").and_then(|v| v.as_str()) {
                        ver = v.to_string();
                    }
                    if let Some(v) = info.get("date").and_then(|v| v.as_str()) {
                        date = v.to_string();
                    }
                    if let Some(v) = info.get("author").and_then(|v| v.as_str()) {
                        author = v.to_string();
                    }
                }
            }
            local.insert(id, (name, ver, date, author));
        }
    }
    // merge: catalog first (id from url), then local-only dirs
    let mut out: Vec<serde_json::Value> = Vec::new();
    let mut seen: std::collections::HashSet<String> = Default::default();
    for c in &catalog {
        let url = c
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let id = plugin_id_from_url(&url);
        if id.is_empty() {
            continue;
        }
        seen.insert(id.clone());
        let (mut name, mut ver, mut date, mut lauthor_local) =
            local.get(&id).cloned().unwrap_or_default();
        // refreshed library entry wins for display metadata (mirrors upstream merge)
        let (lname, lauthor, lver, ldesc, ldate) = local_cat
            .get(&id)
            .map(|e| {
                (
                    str_field(e, "name"),
                    str_field(e, "author"),
                    str_field(e, "version"),
                    str_field(e, "description"),
                    str_field(e, "date"),
                )
            })
            .unwrap_or_default();
        if !lname.is_empty() {
            name = lname;
        }
        if !lver.is_empty() {
            ver = lver;
        }
        if !ldate.is_empty() {
            date = ldate;
        }
        if lauthor_local.is_empty() {
            lauthor_local = lauthor;
        }
        let author = if !lauthor_local.is_empty() {
            lauthor_local
        } else {
            str_field(c, "author")
        };
        let desc = if !ldesc.is_empty() {
            ldesc
        } else {
            str_field(c, "description")
        };
        if date.is_empty() {
            date = str_field(c, "date");
        }
        out.push(serde_json::json!({
            "id": id,
            "name": if name.is_empty() { c.get("name").and_then(|v| v.as_str()).unwrap_or(&id).to_string() } else { name },
            "author": author,
            "version": if ver.is_empty() { str_field(c, "version") } else { ver },
            "description": desc,
            "date": date,
            "url": url,
            "installed": local.contains_key(&id),
            "enabled": SYSTEM_PLUGINS.contains(&id.as_str()) || enabled.contains(&id),
            "system": SYSTEM_PLUGINS.contains(&id.as_str()),
            "companion": id == COMPANION_PLUGIN_ID,
            "locked": id == COMPANION_PLUGIN_ID,
            "group": if SYSTEM_PLUGINS.contains(&id.as_str()) || BUNDLED_PLUGINS.contains(&id.as_str()) { "system" } else { "community" },
        }));
    }
    let mut local_only: Vec<String> = local
        .keys()
        .filter(|id| !seen.contains(*id))
        .cloned()
        .collect();
    // refreshed-library entries unknown to the shipped catalog (community additions)
    for id in local_cat.keys() {
        if !seen.contains(id) && !local.contains_key(id) {
            local_only.push(id.clone());
            seen.insert(id.clone());
        }
    }
    local_only.sort();
    for id in local_only {
        let (mut name, mut ver, mut date, mut author) = local.get(&id).cloned().unwrap_or((
            id.clone(),
            String::new(),
            String::new(),
            String::new(),
        ));
        let mut url = String::new();
        if let Some(e) = local_cat.get(&id) {
            if !str_field(e, "name").is_empty() {
                name = str_field(e, "name");
            }
            if !str_field(e, "version").is_empty() {
                ver = str_field(e, "version");
            }
            if !str_field(e, "date").is_empty() {
                date = str_field(e, "date");
            }
            if !str_field(e, "author").is_empty() {
                author = str_field(e, "author");
            }
            url = str_field(e, "url");
        }
        out.push(serde_json::json!({
            "id": id, "name": name, "author": author, "version": ver, "description": "", "date": date, "url": url,
            "installed": local.contains_key(&id),
            "enabled": SYSTEM_PLUGINS.contains(&id.as_str()) || enabled.contains(&id),
            "system": SYSTEM_PLUGINS.contains(&id.as_str()),
            "companion": id == COMPANION_PLUGIN_ID,
            "locked": id == COMPANION_PLUGIN_ID,
            "group": if SYSTEM_PLUGINS.contains(&id.as_str()) || BUNDLED_PLUGINS.contains(&id.as_str()) { "system" } else { "community" },
        }));
    }
    serde_json::json!({"ok": true, "plugins": out})
}

fn env_python() -> Option<PathBuf> {
    let env = crate::status::get_active_env();
    let raw = env.get("path")?.as_str()?;
    let r = raw.trim_start_matches(['.', '\\', '/']);
    let base = if std::path::Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        get_repo_dir().join(r)
    };
    let py = if cfg!(windows) {
        base.join("Scripts\\python.exe")
    } else {
        base.join("bin/python")
    };
    py.exists().then_some(py)
}

fn scrub_config_lists(repo: &Path, id: &str) -> Result<(), String> {
    let p = repo.join("wgp_config.json");
    let mut cfg = read_wgp_config(repo);
    for key in [
        "enabled_plugins",
        "installed_remote_plugins",
        "pending_plugin_deletions",
    ] {
        let mut list = str_list(&cfg, key);
        list.retain(|x| x != id);
        cfg[key] =
            serde_json::Value::Array(list.into_iter().map(serde_json::Value::String).collect());
    }
    atomic_write(&p, &serde_json::to_string_pretty(&cfg).unwrap_or_default())
        .map_err(|e| e.to_string())
}

// guard-free core: clone (+requirements) + record + enable. plugin_install wraps
// it with the mutating guard; ensure_favorites calls it in a loop (best-effort).
fn enable_plugin_in_config(repo: &Path, id: &str) -> Result<(), String> {
    // record + auto-enable in wgp_config.json (mirrors _finish_install_from_url)
    let p = repo.join("wgp_config.json");
    let mut cfg = read_wgp_config(repo);
    for key in ["installed_remote_plugins", "enabled_plugins"] {
        let mut list = str_list(&cfg, key);
        if !list.iter().any(|x| x == id) {
            list.push(id.to_string());
        }
        cfg[key] =
            serde_json::Value::Array(list.into_iter().map(serde_json::Value::String).collect());
    }
    atomic_write(&p, &serde_json::to_string_pretty(&cfg).unwrap_or_default())
        .map_err(|e| e.to_string())
}

async fn install_plugin_inner_at(
    app: &tauri::AppHandle,
    url: &str,
    git_ref: Option<&str>,
) -> Result<String, String> {
    // Plaintext git clone is a MITM code-execution path (plugins load into
    // Wan2GP). scp-style github.com: URLs ride over SSH and stay allowed.
    if url.starts_with("http://") {
        return Err("refusing plaintext http:// plugin URL - use https://".into());
    }
    let id = plugin_id_from_url(url);
    if !valid_plugin_id(&id) {
        return Err("Could not derive a usable plugin id from that URL".into());
    }
    let repo = get_repo_dir();
    let log = |m: &str| {
        crate::base::push_log(m, "launch");
        let _ = app.emit("launch-log", m.to_string());
    };
    let target = repo.join("plugins").join(&id);
    if !target.exists() {
        log(&format!("[*] Cloning plugin {id}...\n"));
        std::fs::create_dir_all(repo.join("plugins")).map_err(|e| e.to_string())?;
        let target_s = target.to_string_lossy().to_string();
        let mut args = vec!["clone", "--depth", "1"];
        if let Some(r) = git_ref {
            args.push("--branch");
            args.push(r);
        }
        args.push(url);
        args.push(target_s.as_str());
        if !run_logged(app, "git", &args, None, &log).await || !target.exists() {
            // A failed clone can leave a partial target; do not let it block the
            // retry by making the next run think a plugin is already installed.
            if target.exists() {
                let _ = std::fs::remove_dir_all(&target);
            }
            return Err("git clone failed - check console output".into());
        }
        install_requirements(app, &repo, &id, &target, &log).await;
    } else {
        log(&format!(
            "[*] Plugin {id} already installed - enabling...\n"
        ));
    }
    enable_plugin_in_config(&repo, &id)?;
    Ok(id)
}

async fn install_plugin_inner(app: &tauri::AppHandle, url: &str) -> Result<String, String> {
    install_plugin_inner_at(app, url, None).await
}

async fn install_requirements(
    app: &tauri::AppHandle,
    repo: &Path,
    id: &str,
    target: &Path,
    log: &impl Fn(&str),
) {
    use tauri_plugin_shell::process::CommandEvent;
    let req = target.join("requirements.txt");
    if !req.exists() {
        return;
    }
    match env_python() {
        Some(py) => {
            log(&format!("[*] Installing {id} requirements…\n"));
            match crate::base::shell_command(app, &py.to_string_lossy())
                .args(["-m", "pip", "install", "-r", &req.to_string_lossy()])
                .current_dir(repo)
                .spawn()
            {
                Ok((mut rx2, _)) => {
                    while let Some(ev) = rx2.recv().await {
                        match ev {
                            CommandEvent::Stdout(b) | CommandEvent::Stderr(b) => {
                                log(&String::from_utf8_lossy(&b))
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => log(&format!("[!] requirements install spawn failed: {e}\n")),
            }
        }
        None => log(&format!(
            "[!] No active env python — skipped requirements for {id}\n"
        )),
    }
}

fn companion_target(repo: &Path) -> PathBuf {
    repo.join("plugins").join(COMPANION_PLUGIN_ID)
}

fn companion_version(repo: &Path) -> Option<String> {
    let p = companion_target(repo).join("plugin_info.json");
    let s = std::fs::read_to_string(p).ok()?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    v.get("version")
        .and_then(|x| x.as_str())
        .map(str::to_string)
}

fn companion_installed(repo: &Path) -> bool {
    let target = companion_target(repo);
    target.join("plugin.py").is_file() && target.join("plugin_info.json").is_file()
}

fn companion_enabled(repo: &Path) -> bool {
    let cfg = read_wgp_config(repo);
    str_list(&cfg, "enabled_plugins")
        .iter()
        .any(|x| x == COMPANION_PLUGIN_ID)
}

fn companion_opt_out() -> bool {
    // Opt-in by default: the Finetune Manager is not installed automatically on
    // fresh installs, after Repair or at startup. Uncheck the Manage → Plugins
    // toggle to let the launcher seed/repair it again.
    load_config_value()
        .get("companionPluginOptOut")
        .and_then(|x| x.as_bool())
        .unwrap_or(true)
}

/// Persist `companionPluginOptOut` (true = the startup seeder leaves the
/// plugin alone). Shared by the UI toggle and uninstall.
fn set_companion_opt_out(enabled: bool) -> Result<(), String> {
    let p = get_config_file();
    let mut cfg = load_config_value();
    if !cfg.is_object() {
        cfg = serde_json::json!({});
    }
    cfg["companionPluginOptOut"] = serde_json::Value::Bool(enabled);
    atomic_write(&p, &serde_json::to_string_pretty(&cfg).unwrap_or_default())
        .map_err(|e| e.to_string())
}

pub fn companion_plugin_status_value() -> serde_json::Value {
    let repo = get_repo_dir();
    let target = companion_target(&repo);
    let installed = companion_installed(&repo);
    serde_json::json!({
        "id": COMPANION_PLUGIN_ID,
        "url": COMPANION_PLUGIN_URL,
        "pinnedRef": COMPANION_PLUGIN_REF,
        "installed": installed,
        "enabled": installed && companion_enabled(&repo),
        "version": companion_version(&repo),
        "path": target.to_string_lossy(),
        "optOut": companion_opt_out(),
    })
}

#[tauri::command]
pub fn companion_plugin_status() -> serde_json::Value {
    companion_plugin_status_value()
}

#[tauri::command]
pub fn companion_plugin_set_opt_out(enabled: bool) -> Result<serde_json::Value, String> {
    set_companion_opt_out(enabled)?;
    Ok(companion_plugin_status_value())
}

async fn install_companion_inner(app: &tauri::AppHandle) -> Result<String, String> {
    let repo = get_repo_dir();
    if !repo.join("wgp.py").exists() && !repo.join("wgp_config.json").exists() {
        return Err("Wan2GP is not installed yet".into());
    }
    let target = companion_target(&repo);
    if target.exists() {
        if companion_installed(&repo) {
            enable_plugin_in_config(&repo, COMPANION_PLUGIN_ID)?;
            return Ok(COMPANION_PLUGIN_ID.to_string());
        }
        // A failed/partial clone is safe to discard only when it has no local
        // config. config.json holds the registry token, so never delete that.
        if target.join("config.json").exists() {
            return Err(format!(
                "{} is incomplete and contains config.json - back it up, remove the folder, then retry",
                target.display()
            ));
        }
        std::fs::remove_dir_all(&target)
            .map_err(|e| format!("could not clear incomplete companion checkout: {e}"))?;
    }
    let id = install_plugin_inner_at(app, COMPANION_PLUGIN_URL, Some(COMPANION_PLUGIN_REF)).await?;
    if !companion_installed(&repo) {
        let cloned = companion_target(&repo);
        if !cloned.join("config.json").exists() {
            let _ = std::fs::remove_dir_all(&cloned);
        }
        return Err("Finetune Manager cloned but plugin.py/plugin_info.json is missing".into());
    }
    Ok(id)
}

/// Default companion install for fresh installs, reinstall/repair and existing
/// installs that predate this feature. Best-effort: never fails setup.
pub async fn ensure_companion_plugin(app: tauri::AppHandle, force: bool) {
    if !force && companion_opt_out() {
        return;
    }
    let repo = get_repo_dir();
    if !repo.join("wgp.py").exists() && !repo.join("wgp_config.json").exists() {
        return;
    }
    let log = |m: &str| {
        crate::base::push_log(m, "setup");
        let _ = app.emit("setup-output", m.to_string());
    };
    if companion_installed(&repo) {
        if !companion_enabled(&repo) {
            match enable_plugin_in_config(&repo, COMPANION_PLUGIN_ID) {
                Ok(()) => log("[*] Finetune Manager enabled.\n"),
                Err(e) => log(&format!("[!] Could not enable Finetune Manager: {e}\n")),
            }
        }
        return;
    }
    log("[*] Installing Finetune Manager (pinned companion plugin)...\n");
    match install_companion_inner(&app).await {
        Ok(id) => log(&format!("[OK] Companion plugin {id} ready.\n")),
        Err(e) => log(&format!(
            "[!] Finetune Manager was not installed: {e}. You can retry from Manage > Plugins.\n"
        )),
    }
}

#[tauri::command]
pub async fn companion_plugin_install(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    mutating_try("companion-plugin-install")?;
    let r = install_companion_inner(&app).await;
    mutating_done();
    let id = r?;
    let log = |m: &str| {
        crate::base::push_log(m, "launch");
        let _ = app.emit("launch-log", m.to_string());
    };
    log(&format!(
        "[OK] Finetune Manager {id} installed and enabled - restart Wan2GP to load it.\n"
    ));
    Ok(serde_json::json!({"ok": true, "id": id, "status": companion_plugin_status_value()}))
}

// Favourites (desktop-config.json `favoritePlugins: [urls]`) auto-install.
// Called at the end of fresh Install; best-effort per URL, never fails setup.
// ponytail: no mutating guard — runs INSIDE install()'s guard; concurrent
// plugin_install calls are still serialized by their own guard.
pub async fn ensure_favorite_plugins(app: tauri::AppHandle) {
    let mut favs: Vec<String> = Vec::new();
    let user: Vec<String> = load_config_value()
        .get("favoritePlugins")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|e| e.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    for u in user {
        if !favs.contains(&u) {
            favs.push(u);
        }
    }
    let log = |m: &str| {
        crate::base::push_log(m, "setup");
        let _ = app.emit("setup-output", m.to_string());
    };
    log(&format!(
        "[*] Installing {} favourite plugin(s)…\n",
        favs.len()
    ));
    for url in favs {
        match install_plugin_inner(&app, &url).await {
            Ok(id) => log(&format!("[✓] Favourite plugin {id} ready.\n")),
            Err(e) => log(&format!("[!] Favourite {url} failed: {e}\n")),
        }
    }
}

fn check_update_inner(id: &str) -> (bool, u64, Option<String>) {
    let Ok(target) = plugin_dir(id) else {
        return (false, 0, Some("bad plugin id".into()));
    };
    if !target.join(".git").exists() {
        return (false, 0, Some("not a git checkout".into()));
    }
    let fetch = silent_command("git")
        .args([
            "-C",
            &target.to_string_lossy(),
            "fetch",
            "origin",
            "--quiet",
        ])
        .output();
    if fetch.is_err() || !fetch.unwrap().status.success() {
        return (false, 0, Some("fetch failed (offline?)".into()));
    }
    let behind = silent_command("git")
        .args([
            "-C",
            &target.to_string_lossy(),
            "rev-list",
            "--count",
            "HEAD..@{u}",
        ])
        .output();
    match behind {
        Ok(o) => {
            let n: u64 = String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse()
                .unwrap_or(0);
            (n > 0, n, None)
        }
        Err(e) => (false, 0, Some(e.to_string())),
    }
}

/// A plugin id is ONE directory name under `<repo>/plugins`. Letters, digits,
/// `_`, `-` and inner dots only: separators, `..`, a leading/trailing dot
/// (`__pycache__`, hidden dirs) or a drive prefix must never reach a
/// remove_dir_all/clone target.
fn valid_plugin_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 100
        && !id.starts_with('.')
        && !id.ends_with('.')
        && !id.contains("..")
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// `<repo>/plugins/<id>` after validation — the only way callers should build
/// a path from a user-supplied plugin id.
fn plugin_dir(id: &str) -> Result<std::path::PathBuf, String> {
    if !valid_plugin_id(id) {
        return Err("Bad plugin id".into());
    }
    let root = crate::base::get_repo_dir().join("plugins");
    let target = root.join(id);
    if target.parent() != Some(root.as_path()) {
        return Err("Bad plugin id".into());
    }
    Ok(target)
}

#[tauri::command]
pub async fn plugin_check_update(id: Option<String>) -> Result<serde_json::Value, String> {
    let id = id.unwrap_or_default().trim().to_string();
    if !valid_plugin_id(&id) {
        return Err("Bad plugin id".into());
    }
    let (update, behind, error) = check_update_inner(&id);
    Ok(serde_json::json!({"ok": true, "update": update, "behind": behind, "error": error}))
}

#[tauri::command]
pub async fn plugin_check_updates(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let mut out = Vec::new();
    let mut avail = 0u32;
    if let Ok(rd) = std::fs::read_dir(get_repo_dir().join("plugins")) {
        let mut ids: Vec<String> = rd
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                let id = e.file_name().to_string_lossy().to_string();
                (p.is_dir() && p.join(".git").exists() && !SYSTEM_PLUGINS.contains(&id.as_str()))
                    .then_some(id)
            })
            .collect();
        ids.sort();
        let log = |m: &str| {
            crate::base::push_log(m, "launch");
            let _ = app.emit("launch-log", m.to_string());
        };
        log(&format!(
            "[*] Checking plugin updates ({} installed)…\n",
            ids.len()
        ));
        for id in ids {
            let (update, behind, _) = check_update_inner(&id);
            if update {
                avail += 1;
            }
            out.push(serde_json::json!({"id": id, "update": update, "behind": behind}));
        }
        log(&format!(
            "[*] Plugin update check done: {avail} update(s) available.\n"
        ));
    }
    Ok(serde_json::json!({"ok": true, "updates": out, "updates_available": avail}))
}

#[tauri::command]
pub async fn plugin_update(
    app: tauri::AppHandle,
    id: Option<String>,
) -> Result<serde_json::Value, String> {
    let id = id.unwrap_or_default().trim().to_string();
    if !valid_plugin_id(&id) {
        return Err("Bad plugin id".into());
    }
    let repo = get_repo_dir();
    let target = plugin_dir(&id)?;
    if !target.join(".git").exists() {
        return Err("Not a git checkout — cannot update".into());
    }
    mutating_try("plugin-update")?;
    let log = |m: &str| {
        crate::base::push_log(m, "launch");
        let _ = app.emit("launch-log", m.to_string());
    };
    log(&format!("[*] Updating plugin {id}…\n"));
    let target_s = target.to_string_lossy().to_string();
    if !run_logged(
        &app,
        "git",
        &["-C", target_s.as_str(), "pull", "--ff-only"],
        None,
        &log,
    )
    .await
    {
        mutating_done();
        return Err(format!(
            "Plugin {id} update failed (git pull) — see console output"
        ));
    }
    install_requirements(&app, &repo, &id, &target, &log).await;
    mutating_done();
    log(&format!(
        "[✓] Plugin {id} updated — restart Wan2GP to load it.\n"
    ));
    Ok(serde_json::json!({"ok": true, "id": id}))
}

#[tauri::command]
pub fn plugin_uninstall(id: Option<String>) -> Result<serde_json::Value, String> {
    let id = id.unwrap_or_default().trim().to_string();
    if !valid_plugin_id(&id) {
        return Err("Bad plugin id".into());
    }
    if SYSTEM_PLUGINS.contains(&id.as_str()) || BUNDLED_PLUGINS.contains(&id.as_str()) {
        return Err("System/bundled plugins cannot be uninstalled".into());
    }
    let repo = get_repo_dir();
    let target = plugin_dir(&id)?;
    if !target.exists() {
        return Err("Not installed".into());
    }
    // files may be locked (Wan2GP running) — mirror upstream pending_plugin_deletions.
    let is_companion = id == COMPANION_PLUGIN_ID;
    match std::fs::remove_dir_all(&target) {
        Ok(()) => {
            scrub_config_lists(&repo, &id)?;
            if is_companion {
                // Otherwise the startup seeder silently reinstalls what the
                // user just removed (0.10.19 fix).
                let _ = set_companion_opt_out(true);
            }
        }
        Err(e) => {
            let p = repo.join("wgp_config.json");
            let mut cfg = read_wgp_config(&repo);
            let mut list = str_list(&cfg, "pending_plugin_deletions");
            if !list.contains(&id) {
                list.push(id.clone());
            }
            cfg["pending_plugin_deletions"] =
                serde_json::Value::Array(list.into_iter().map(serde_json::Value::String).collect());
            atomic_write(&p, &serde_json::to_string_pretty(&cfg).unwrap_or_default())
                .map_err(|e| e.to_string())?;
            if is_companion {
                let _ = set_companion_opt_out(true);
            }
            return Ok(
                serde_json::json!({"ok": true, "id": id, "pending": true, "hint": format!("Files locked ({e}) — will delete on next Wan2GP start. Disabled now.")}),
            );
        }
    }
    // disabling takes effect next launch even when files are gone now
    let p = repo.join("wgp_config.json");
    let mut cfg = read_wgp_config(&repo);
    for key in ["enabled_plugins", "installed_remote_plugins"] {
        let mut list = str_list(&cfg, key);
        list.retain(|x| x != &id);
        cfg[key] =
            serde_json::Value::Array(list.into_iter().map(serde_json::Value::String).collect());
    }
    atomic_write(&p, &serde_json::to_string_pretty(&cfg).unwrap_or_default())
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"ok": true, "id": id}))
}
#[tauri::command]
pub async fn plugin_install(
    app: tauri::AppHandle,
    url: Option<String>,
) -> Result<serde_json::Value, String> {
    let url = url.unwrap_or_default().trim().to_string();
    // http:// is a MITM code-execution path (cloned code loads into Wan2GP).
    if !(url.starts_with("https://") || url.contains("github.com:")) {
        return Err("Give a plugin git URL (https://github.com/…/…)".into());
    }
    if !get_repo_dir().join("wgp.py").exists() {
        return Err("Wan2GP not installed — run Install first".into());
    }
    mutating_try("plugin-install")?;
    let r = install_plugin_inner(&app, &url).await;
    mutating_done();
    let id = r?;
    let log = |m: &str| {
        crate::base::push_log(m, "launch");
        let _ = app.emit("launch-log", m.to_string());
    };
    log(&format!(
        "[✓] Plugin {id} installed and enabled — restart Wan2GP to load it.\n"
    ));
    Ok(serde_json::json!({"ok": true, "id": id}))
}
fn split_github_repo(url: &str) -> Option<(String, String)> {
    let mut t = url.trim().to_string();
    if t.is_empty() {
        return None;
    }
    if let Some(i) = t.find('?') {
        t.truncate(i);
    }
    if let Some(i) = t.find('#') {
        t.truncate(i);
    }
    if t.starts_with("git@github.com:") {
        t = format!("https://github.com/{}", &t["git@github.com:".len()..]);
    }
    t = t.trim_end_matches('/').to_string();
    let low = t.to_lowercase();
    let idx = low.find("github.com/")?;
    let tail = &t[idx + "github.com/".len()..];
    let parts: Vec<&str> = tail.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() < 2 {
        return None;
    }
    let mut repo = parts[1].to_string();
    if repo.to_lowercase().ends_with(".git") {
        repo.truncate(repo.len() - 4);
    }
    if parts[0].is_empty() || repo.is_empty() {
        return None;
    }
    Some((parts[0].to_string(), repo))
}

#[derive(PartialOrd, PartialEq, Debug)]
enum VTok {
    Num(u64),
    Str(String),
}
fn version_tokens(v: &str) -> Vec<VTok> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut cur_num = true;
    let flush = |cur: &mut String, cur_num: bool, out: &mut Vec<VTok>| {
        if cur.is_empty() {
            return;
        }
        if cur_num {
            out.push(VTok::Num(cur.parse().unwrap_or(0)));
        } else {
            out.push(VTok::Str(cur.to_lowercase()));
        }
    };
    for ch in v.chars() {
        if ch.is_ascii_alphanumeric() {
            let is_num = ch.is_ascii_digit();
            if !cur.is_empty() && is_num != cur_num {
                flush(&mut cur, cur_num, &mut out);
                cur_num = is_num;
            } else if cur.is_empty() {
                cur_num = is_num;
            }
            cur.push(ch);
        } else if !cur.is_empty() {
            flush(&mut cur, cur_num, &mut out);
            cur.clear();
        }
    }
    flush(&mut cur, cur_num, &mut out);
    out
}
// port of compare_release_metadata: date first (ISO strings sort chronologically), then version.
fn release_newer(remote: &serde_json::Value, local: &serde_json::Value) -> bool {
    let (rd, ld) = (str_field(remote, "date"), str_field(local, "date"));
    if !rd.is_empty() || !ld.is_empty() {
        return rd > ld;
    }
    let (rv, lv) = (
        version_tokens(&str_field(remote, "version")),
        version_tokens(&str_field(local, "version")),
    );
    let n = rv.len().max(lv.len());
    for i in 0..n {
        // ponytail: missing part = 0 (upstream filler) — but VTok has no default; treat missing as Num(0)
        let l = rv.get(i);
        let r = lv.get(i);
        let ord = match (l, r) {
            (Some(a), Some(b)) => a.partial_cmp(b),
            (Some(_), None) => Some(std::cmp::Ordering::Greater),
            (None, Some(_)) => Some(std::cmp::Ordering::Less),
            (None, None) => Some(std::cmp::Ordering::Equal),
        };
        match ord {
            Some(std::cmp::Ordering::Greater) => return true,
            Some(std::cmp::Ordering::Less) => return false,
            _ => {}
        }
    }
    // upstream filler (0,0): trailing numeric zeros don't count — "1.0" vs "1.0.0" equal here only if all compared equal
    false
}

#[tauri::command]
pub async fn plugin_refresh_catalog(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let repo = get_repo_dir();
    if !repo.join("wgp.py").exists() {
        return Err("Wan2GP not installed — run Install first".into());
    }
    mutating_try("plugin-refresh")?;
    let log = |m: &str| {
        crate::base::push_log(m, "launch");
        let _ = app.emit("launch-log", m.to_string());
    };
    // targets: shipped catalog urls + refreshed-library urls + installed git remotes
    let mut targets: std::collections::HashMap<String, String> = Default::default(); // id -> url
    if let Ok(s) = std::fs::read_to_string(repo.join("plugins.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
            if let Some(arr) = v.as_array() {
                for e in arr {
                    if let Some(u) = e.get("url").and_then(|x| x.as_str()) {
                        let id = plugin_id_from_url(u);
                        if !id.is_empty() {
                            targets.entry(id).or_insert_with(|| u.to_string());
                        }
                    }
                }
            }
        }
    }
    for (id, e) in read_local_catalog(&repo) {
        let u = str_field(&e, "url");
        if !u.is_empty() {
            targets.entry(id).or_insert(u);
        }
    }
    if let Ok(rd) = std::fs::read_dir(repo.join("plugins")) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_dir() || !p.join(".git").exists() {
                continue;
            }
            let id = e.file_name().to_string_lossy().to_string();
            if let Ok(out) = silent_command("git")
                .args([
                    "-C",
                    &p.to_string_lossy(),
                    "config",
                    "--get",
                    "remote.origin.url",
                ])
                .output()
            {
                if out.status.success() {
                    let u = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !u.is_empty() {
                        targets.entry(id).or_insert(u);
                    }
                }
            }
        }
    }
    log(&format!(
        "[*] Refreshing plugin library ({} source(s))…\n",
        targets.len()
    ));
    let client = reqwest::Client::builder()
        .user_agent("wan2gp-tauri")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let mut local_map: std::collections::HashMap<String, serde_json::Value> =
        read_local_catalog(&repo);
    let (mut checked, mut updated, mut avail) = (0u32, 0u32, 0u32);
    let mut ids: Vec<String> = targets.keys().cloned().collect();
    ids.sort();
    for id in ids {
        let url = &targets[&id];
        let Some((owner, name)) = split_github_repo(url) else {
            continue;
        };
        checked += 1;
        let raw = format!("https://github.com/{owner}/{name}/raw/HEAD/plugin_info.json");
        let meta: serde_json::Value = match client.get(&raw).send().await {
            Ok(r) if r.status().is_success() => match r.json().await {
                Ok(v) => v,
                Err(_) => continue,
            },
            _ => continue,
        };
        if !meta.is_object() {
            continue;
        }
        let mut entry = serde_json::json!({
            "name": str_field(&meta, "name"), "author": str_field(&meta, "author"),
            "version": str_field(&meta, "version"), "description": str_field(&meta, "description"),
            "type": meta.get("type").cloned().unwrap_or(serde_json::json!(["app"])),
            "date": str_field(&meta, "date"), "wan2gp_version": str_field(&meta, "wan2gp_version"),
            "url": url,
        });
        entry["last_check"] = serde_json::Value::String(chrono_now());
        // updates_available: remote newer than the INSTALLED copy (upstream semantics)
        let plug = repo.join("plugins").join(&id);
        if plug.join("plugin_info.json").exists() {
            if let Ok(s) = std::fs::read_to_string(plug.join("plugin_info.json")) {
                if let Ok(cur) = serde_json::from_str::<serde_json::Value>(&s) {
                    if release_newer(&entry, &cur) {
                        avail += 1;
                    }
                }
            }
        } else if let Some(old) = local_map.get(&id) {
            if release_newer(&entry, old) {
                avail += 1;
            }
        }
        local_map.insert(id, entry);
        updated += 1;
    }
    if updated > 0 {
        let mut arr: Vec<serde_json::Value> = local_map.into_values().collect();
        arr.sort_by_key(|a| str_field(a, "name").to_lowercase());
        atomic_write(
            &repo.join("plugins_local.json"),
            &serde_json::to_string_pretty(&serde_json::Value::Array(arr)).unwrap_or_default(),
        )
        .map_err(|e| {
            mutating_done();
            e.to_string()
        })?;
    }
    mutating_done();
    log(&format!("[✓] Library refreshed: {checked} checked, {updated} updated, {avail} update(s) available.\n"));
    Ok(
        serde_json::json!({"ok": true, "checked": checked, "updated": updated, "updates_available": avail}),
    )
}

fn chrono_now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

#[cfg(test)]
mod companion_tests {
    use super::*;

    #[test]
    fn companion_pin_is_a_release_tag_not_a_branch() {
        assert!(COMPANION_PLUGIN_REF.starts_with('v'));
        assert_ne!(COMPANION_PLUGIN_REF, "master");
    }

    #[test]
    fn installed_requires_both_plugin_files() {
        let repo = tempfile::tempdir().unwrap();
        let target = companion_target(repo.path());
        std::fs::create_dir_all(&target).unwrap();
        assert!(!companion_installed(repo.path()));
        std::fs::write(target.join("plugin.py"), "x").unwrap();
        assert!(!companion_installed(repo.path()));
        std::fs::write(target.join("plugin_info.json"), r#"{"version":"1.2.3"}"#).unwrap();
        assert!(companion_installed(repo.path()));
        assert_eq!(companion_version(repo.path()).as_deref(), Some("1.2.3"));
    }

    #[test]
    fn uninstall_opts_out_so_the_seeder_leaves_it_alone() {
        // Uninstalling from the UI used to be undone by the next startup seed,
        // which saw "not installed" and cloned it back (0.10.19 fix).
        let _data = crate::base::TestDataDir::new();
        let repo = get_repo_dir();
        std::fs::create_dir_all(repo.join("plugins")).unwrap();
        std::fs::write(repo.join("wgp.py"), "").unwrap();
        std::fs::write(repo.join("wgp_config.json"), "{}").unwrap();
        let target = companion_target(&repo);
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("plugin.py"), "x").unwrap();
        std::fs::write(target.join("plugin_info.json"), r#"{"version":"3.7.0"}"#).unwrap();
        enable_plugin_in_config(&repo, COMPANION_PLUGIN_ID).unwrap();
        // Default is now opt-in (the user asked for the companion NOT to install
        // automatically): the launcher considers it opted out until enabled.
        assert!(companion_opt_out());

        // Installed: the row is marked managed (locked checkbox, no delete).
        let list = plugins_list();
        let rows = list.get("plugins").and_then(|v| v.as_array()).unwrap();
        let row = rows
            .iter()
            .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(COMPANION_PLUGIN_ID))
            .expect("companion row listed");
        assert_eq!(row.get("companion").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(row.get("locked").and_then(|v| v.as_bool()), Some(true));

        let r = plugin_uninstall(Some(COMPANION_PLUGIN_ID.to_string())).unwrap();
        assert_eq!(r.get("ok").and_then(|v| v.as_bool()), Some(true));
        assert!(!target.exists());
        assert!(companion_opt_out(), "uninstall must stop the re-seed");
        // Status reflects it, and the list row carries the managed flags.
        let st = companion_plugin_status_value();
        assert_eq!(st.get("optOut").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(st.get("installed").and_then(|v| v.as_bool()), Some(false));
        reset_dir_caches();
    }

    #[test]
    fn plugin_ids_cannot_escape_the_plugins_dir() {
        for bad in [
            "..", ".", "../evil", "a/b", "a\\b", "", "..\\..", ".hidden", "trail.", "C:evil",
        ] {
            assert!(!valid_plugin_id(bad), "{bad:?} must be rejected");
        }
        assert!(!valid_plugin_id(&"x".repeat(101)));
        for good in [
            "Wan2GP-finetune-manager",
            "sd_perturbation",
            "my.plugin",
            "StatusPro",
        ] {
            assert!(valid_plugin_id(good), "{good:?} must be accepted");
        }
        let _data = crate::base::TestDataDir::new();
        let repo = get_repo_dir();
        assert!(plugin_dir("..").is_err());
        assert!(plugin_dir("../..").is_err());
        let ok = plugin_dir("StatusPro").unwrap();
        assert_eq!(ok, repo.join("plugins").join("StatusPro"));
        reset_dir_caches();
    }

    #[test]
    fn uninstall_refuses_a_traversing_id() {
        // ".." used to pass the old slash-only check and remove_dir_all'd the
        // whole Wan2GP repo.
        let _data = crate::base::TestDataDir::new();
        let repo = get_repo_dir();
        std::fs::create_dir_all(repo.join("plugins")).unwrap();
        std::fs::write(repo.join("wgp.py"), "").unwrap();
        std::fs::write(repo.join("wgp_config.json"), "{}").unwrap();
        let err = plugin_uninstall(Some("..".to_string())).unwrap_err();
        assert!(err.contains("Bad plugin id"), "got {err}");
        assert!(repo.join("wgp.py").exists());
        assert!(repo.join("plugins").exists());
        reset_dir_caches();
    }

    #[test]
    fn enabling_records_and_enables_the_companion_once() {
        let repo = tempfile::tempdir().unwrap();
        std::fs::write(repo.path().join("wgp_config.json"), "{}").unwrap();
        enable_plugin_in_config(repo.path(), COMPANION_PLUGIN_ID).unwrap();
        enable_plugin_in_config(repo.path(), COMPANION_PLUGIN_ID).unwrap();
        let cfg = read_wgp_config(repo.path());
        let enabled = str_list(&cfg, "enabled_plugins");
        let installed = str_list(&cfg, "installed_remote_plugins");
        assert_eq!(
            enabled.iter().filter(|x| *x == COMPANION_PLUGIN_ID).count(),
            1
        );
        assert_eq!(
            installed
                .iter()
                .filter(|x| *x == COMPANION_PLUGIN_ID)
                .count(),
            1
        );
    }
}

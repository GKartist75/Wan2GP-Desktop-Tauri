//! Wan2GP plugin management (list / install / enable).
//! Mirrors the in-app plugin manager's data model without reimplementing it:
//! - catalog: <repo>/plugins.json (remote registry: name/author/version/url)
//! - installed: <repo>/plugins/<id>/ (+ plugin_info.json for name/version)
//! - enabled: `enabled_plugins` in <repo>/wgp_config.json (+ SYSTEM_PLUGINS always on)
use tauri::Emitter;
use std::path::PathBuf;
use crate::base::*;

const SYSTEM_PLUGINS: &[&str] = &["video_mask_creator", "guides", "configuration", "plugin_manager", "about"];
// bundled with the Wan2GP repo itself — not uninstallable (like upstream: uninstallable=false).
const BUNDLED_PLUGINS: &[&str] = &["downloads", "media_flow", "models_manager", "motion_designer", "sample"];
// Status Pro is a default plugin: installed on fresh setup, kept enabled, not uninstallable.
const STATUS_PRO_ID: &str = "wan2gp-status-pro";
const STATUS_PRO_URL: &str = "https://github.com/totideyouover2026-max/wan2gp-status-pro";

// repo dir name from a git URL (mirrors shared/utils/plugins.py plugin_id_from_url).
fn plugin_id_from_url(url: &str) -> String {
    let mut t = url.trim().to_string();
    if let Some(i) = t.find("github.com:") { t = format!("https://github.com/{}", &t[i + "github.com:".len()..]); }
    t = t.trim_end_matches('/').to_string();
    if t.to_lowercase().ends_with(".git") { t.truncate(t.len() - 4); }
    t.rsplit('/').next().unwrap_or("").trim().to_string()
}

fn read_wgp_config(repo: &PathBuf) -> serde_json::Value {
    let p = repo.join("wgp_config.json");
    if p.exists() {
        if let Ok(s) = std::fs::read_to_string(&p) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) { return v; }
        }
    }
    serde_json::json!({})
}

fn str_list(v: &serde_json::Value, key: &str) -> Vec<String> {
    v.get(key).and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|e| e.as_str().map(|s| s.to_string())).collect()).unwrap_or_default()
}

fn read_local_catalog(repo: &PathBuf) -> std::collections::HashMap<String, serde_json::Value> {
    let mut map = std::collections::HashMap::new();
    if let Ok(s) = std::fs::read_to_string(repo.join("plugins_local.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
            let arr = if v.is_array() { v.as_array().cloned().unwrap_or_default() } else { vec![v] };
            for e in arr {
                let url = e.get("url").and_then(|v| v.as_str()).unwrap_or("");
                let id = plugin_id_from_url(url);
                if !id.is_empty() { map.insert(id, e); }
            }
        }
    }
    map
}

fn str_field(v: &serde_json::Value, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

#[tauri::command]
pub fn plugins_list() -> serde_json::Value {
    let repo = get_repo_dir();
    if !repo.join("wgp.py").exists() { return serde_json::json!({"ok": false, "error": "Wan2GP not installed"}); }
    let catalog: Vec<serde_json::Value> = std::fs::read_to_string(repo.join("plugins.json"))
        .ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let cfg = read_wgp_config(&repo);
    let enabled = str_list(&cfg, "enabled_plugins");
    // Wan2GP's own refreshed library (written by its plugin manager AND by us —
    // same file, same schema): fresher metadata wins over the shipped plugins.json.
    let local_cat = read_local_catalog(&repo);
    // local scan: every plugins/<dir> with plugin.py or plugin_info.json
    let mut local: std::collections::HashMap<String, (String, String, String, String)> = Default::default(); // id -> (name, version, date, author)
    if let Ok(rd) = std::fs::read_dir(repo.join("plugins")) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_dir() { continue; }
            let id = e.file_name().to_string_lossy().to_string();
            if id.starts_with('.') || id == "__pycache__" { continue; }
            if !p.join("plugin.py").exists() && !p.join("plugin_info.json").exists() { continue; }
            let (mut name, mut ver, mut date, mut author) = (id.clone(), String::new(), String::new(), String::new());
            if let Ok(s) = std::fs::read_to_string(p.join("plugin_info.json")) {
                if let Ok(info) = serde_json::from_str::<serde_json::Value>(&s) {
                    if let Some(n) = info.get("name").and_then(|v| v.as_str()) { name = n.to_string(); }
                    if let Some(v) = info.get("version").and_then(|v| v.as_str()) { ver = v.to_string(); }
                    if let Some(v) = info.get("date").and_then(|v| v.as_str()) { date = v.to_string(); }
                    if let Some(v) = info.get("author").and_then(|v| v.as_str()) { author = v.to_string(); }
                }
            }
            local.insert(id, (name, ver, date, author));
        }
    }
    // merge: catalog first (id from url), then local-only dirs
    let mut out: Vec<serde_json::Value> = Vec::new();
    let mut seen: std::collections::HashSet<String> = Default::default();
    for c in &catalog {
        let url = c.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let id = plugin_id_from_url(&url);
        if id.is_empty() { continue; }
        seen.insert(id.clone());
        let (mut name, mut ver, mut date, mut lauthor_local) = local.get(&id).cloned().unwrap_or_default();
        // refreshed library entry wins for display metadata (mirrors upstream merge)
        let (lname, lauthor, lver, ldesc, ldate) = local_cat.get(&id)
            .map(|e| (str_field(e, "name"), str_field(e, "author"), str_field(e, "version"), str_field(e, "description"), str_field(e, "date")))
            .unwrap_or_default();
        if !lname.is_empty() { name = lname; }
        if !lver.is_empty() { ver = lver; }
        if !ldate.is_empty() { date = ldate; }
        if lauthor_local.is_empty() { lauthor_local = lauthor; }
        let author = if !lauthor_local.is_empty() { lauthor_local } else { str_field(c, "author") };
        let desc = if !ldesc.is_empty() { ldesc } else { str_field(c, "description") };
        if date.is_empty() { date = str_field(c, "date"); }
        out.push(serde_json::json!({
            "id": id,
            "name": if name.is_empty() { c.get("name").and_then(|v| v.as_str()).unwrap_or(&id).to_string() } else { name },
            "author": author,
            "version": if ver.is_empty() { str_field(c, "version") } else { ver },
            "description": desc,
            "date": date,
            "url": url,
            "installed": local.contains_key(&id),
            "enabled": id == STATUS_PRO_ID || SYSTEM_PLUGINS.contains(&id.as_str()) || enabled.contains(&id),
            "system": SYSTEM_PLUGINS.contains(&id.as_str()),
            "locked": id == STATUS_PRO_ID,
            "group": if SYSTEM_PLUGINS.contains(&id.as_str()) || BUNDLED_PLUGINS.contains(&id.as_str()) { "system" } else { "community" },
        }));
    }
    let mut local_only: Vec<String> = local.keys().filter(|id| !seen.contains(*id)).cloned().collect();
    // refreshed-library entries unknown to the shipped catalog (community additions)
    for id in local_cat.keys() {
        if !seen.contains(id) && !local.contains_key(id) { local_only.push(id.clone()); seen.insert(id.clone()); }
    }
    local_only.sort();
    for id in local_only {
        let (mut name, mut ver, mut date, mut author) = local.get(&id).cloned().unwrap_or((id.clone(), String::new(), String::new(), String::new()));
        let mut url = String::new();
        if let Some(e) = local_cat.get(&id) {
            if !str_field(e, "name").is_empty() { name = str_field(e, "name"); }
            if !str_field(e, "version").is_empty() { ver = str_field(e, "version"); }
            if !str_field(e, "date").is_empty() { date = str_field(e, "date"); }
            if !str_field(e, "author").is_empty() { author = str_field(e, "author"); }
            url = str_field(e, "url");
        }
        out.push(serde_json::json!({
            "id": id, "name": name, "author": author, "version": ver, "description": "", "date": date, "url": url,
            "installed": local.contains_key(&id),
            "enabled": id == STATUS_PRO_ID || SYSTEM_PLUGINS.contains(&id.as_str()) || enabled.contains(&id),
            "system": SYSTEM_PLUGINS.contains(&id.as_str()),
            "locked": id == STATUS_PRO_ID,
            "group": if SYSTEM_PLUGINS.contains(&id.as_str()) || BUNDLED_PLUGINS.contains(&id.as_str()) { "system" } else { "community" },
        }));
    }
    serde_json::json!({"ok": true, "plugins": out})
}

fn env_python() -> Option<PathBuf> {
    let env = crate::status::get_active_env();
    let raw = env.get("path")?.as_str()?;
    let r = raw.trim_start_matches(['.', '\\', '/']);
    let base = if std::path::Path::new(raw).is_absolute() { PathBuf::from(raw) } else { get_repo_dir().join(r) };
    let py = if cfg!(windows) { base.join("Scripts\\python.exe") } else { base.join("bin/python") };
    py.exists().then_some(py)
}

fn scrub_config_lists(repo: &PathBuf, id: &str) -> Result<(), String> {
    let p = repo.join("wgp_config.json");
    let mut cfg = read_wgp_config(repo);
    for key in ["enabled_plugins", "installed_remote_plugins", "pending_plugin_deletions"] {
        let mut list = str_list(&cfg, key);
        list.retain(|x| x != id);
        cfg[key] = serde_json::Value::Array(list.into_iter().map(serde_json::Value::String).collect());
    }
    atomic_write(&p, &serde_json::to_string_pretty(&cfg).unwrap_or_default()).map_err(|e| e.to_string())
}

// guard-free core: clone (+requirements) + record + enable. plugin_install wraps
// it with the mutating guard; ensure_favorites calls it in a loop (best-effort).
async fn install_plugin_inner(app: &tauri::AppHandle, url: &str) -> Result<String, String> {
    use tauri_plugin_shell::ShellExt;
    use tauri_plugin_shell::process::CommandEvent;
    let id = plugin_id_from_url(url);
    if id.is_empty() { return Err("Could not derive a plugin id from that URL".into()); }
    let repo = get_repo_dir();
    let log = |m: &str| { crate::base::push_log(m, "launch"); let _ = app.emit("launch-log", m.to_string()); };
    let target = repo.join("plugins").join(&id);
    if !target.exists() {
        log(&format!("[*] Cloning plugin {id}…\n"));
        std::fs::create_dir_all(repo.join("plugins")).map_err(|e| e.to_string())?;
        let (mut rx, _) = app.shell().command("git").args(["clone", "--depth", "1", url, &target.to_string_lossy()]).spawn().map_err(|e| e.to_string())?;
        while let Some(ev) = rx.recv().await {
            match ev {
                CommandEvent::Stdout(b) | CommandEvent::Stderr(b) => log(&String::from_utf8_lossy(&b)),
                _ => {}
            }
        }
        if !target.exists() { return Err("git clone failed — check console output".into()); }
        install_requirements(app, &repo, &id, &target, &log).await;
    } else {
        log(&format!("[*] Plugin {id} already installed — enabling…\n"));
    }
    // record + auto-enable in wgp_config.json (mirrors _finish_install_from_url)
    let p = repo.join("wgp_config.json");
    let mut cfg = read_wgp_config(&repo);
    for key in ["installed_remote_plugins", "enabled_plugins"] {
        let mut list = str_list(&cfg, key);
        if !list.contains(&id) { list.push(id.clone()); }
        cfg[key] = serde_json::Value::Array(list.into_iter().map(serde_json::Value::String).collect());
    }
    atomic_write(&p, &serde_json::to_string_pretty(&cfg).unwrap_or_default()).map_err(|e| e.to_string())?;
    Ok(id)
}

async fn install_requirements(app: &tauri::AppHandle, repo: &PathBuf, id: &str, target: &PathBuf, log: &impl Fn(&str)) {
    use tauri_plugin_shell::ShellExt;
    use tauri_plugin_shell::process::CommandEvent;
    let req = target.join("requirements.txt");
    if !req.exists() { return; }
    match env_python() {
        Some(py) => {
            log(&format!("[*] Installing {id} requirements…\n"));
            match app.shell().command(&py).args(["-m", "pip", "install", "-r", &req.to_string_lossy()]).current_dir(repo).spawn() {
                Ok((mut rx2, _)) => {
                    while let Some(ev) = rx2.recv().await {
                        match ev {
                            CommandEvent::Stdout(b) | CommandEvent::Stderr(b) => log(&String::from_utf8_lossy(&b)),
                            _ => {}
                        }
                    }
                }
                Err(e) => log(&format!("[!] requirements install spawn failed: {e}\n")),
            }
        }
        None => log(&format!("[!] No active env python — skipped requirements for {id}\n")),
    }
}

// Favourites (desktop-config.json `favoritePlugins: [urls]`) auto-install.
// Called at the end of fresh Install; best-effort per URL, never fails setup.
// ponytail: no mutating guard — runs INSIDE install()'s guard; concurrent
// plugin_install calls are still serialized by their own guard.
pub async fn ensure_favorite_plugins(app: tauri::AppHandle) {
    let mut favs: Vec<String> = vec![STATUS_PRO_URL.to_string()];
    let user: Vec<String> = load_config_value().get("favoritePlugins").and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|e| e.as_str().map(|s| s.trim().to_string())).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    for u in user { if !favs.contains(&u) { favs.push(u); } }
    let log = |m: &str| { crate::base::push_log(m, "setup"); let _ = app.emit("setup-output", m.to_string()); };
    log(&format!("[*] Installing {} favourite plugin(s)…\n", favs.len()));
    for url in favs {
        match install_plugin_inner(&app, &url).await {
            Ok(id) => log(&format!("[✓] Favourite plugin {id} ready.\n")),
            Err(e) => log(&format!("[!] Favourite {url} failed: {e}\n")),
        }
    }
}

fn check_update_inner(id: &str) -> (bool, u64, Option<String>) {
    let target = get_repo_dir().join("plugins").join(id);
    if !target.join(".git").exists() { return (false, 0, Some("not a git checkout".into())); }
    let fetch = silent_command("git").args(["-C", &target.to_string_lossy(), "fetch", "origin", "--quiet"]).output();
    if fetch.is_err() || !fetch.unwrap().status.success() { return (false, 0, Some("fetch failed (offline?)".into())); }
    let behind = silent_command("git").args(["-C", &target.to_string_lossy(), "rev-list", "--count", "HEAD..@{u}"]).output();
    match behind {
        Ok(o) => { let n: u64 = String::from_utf8_lossy(&o.stdout).trim().parse().unwrap_or(0); (n > 0, n, None) }
        Err(e) => (false, 0, Some(e.to_string())),
    }
}

fn valid_plugin_id(id: &str) -> bool { !id.is_empty() && !id.contains('/') && !id.contains('\\') }

#[tauri::command]
pub async fn plugin_check_update(id: Option<String>) -> Result<serde_json::Value, String> {
    let id = id.unwrap_or_default().trim().to_string();
    if !valid_plugin_id(&id) { return Err("Bad plugin id".into()); }
    let (update, behind, error) = check_update_inner(&id);
    Ok(serde_json::json!({"ok": true, "update": update, "behind": behind, "error": error}))
}

#[tauri::command]
pub async fn plugin_check_updates(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let mut out = Vec::new();
    let mut avail = 0u32;
    if let Ok(rd) = std::fs::read_dir(get_repo_dir().join("plugins")) {
        let mut ids: Vec<String> = rd.flatten().filter_map(|e| {
            let p = e.path();
            let id = e.file_name().to_string_lossy().to_string();
            (p.is_dir() && p.join(".git").exists() && !SYSTEM_PLUGINS.contains(&id.as_str())).then_some(id)
        }).collect();
        ids.sort();
        let log = |m: &str| { crate::base::push_log(m, "launch"); let _ = app.emit("launch-log", m.to_string()); };
        log(&format!("[*] Checking plugin updates ({} installed)…\n", ids.len()));
        for id in ids {
            let (update, behind, _) = check_update_inner(&id);
            if update { avail += 1; }
            out.push(serde_json::json!({"id": id, "update": update, "behind": behind}));
        }
        log(&format!("[*] Plugin update check done: {avail} update(s) available.\n"));
    }
    Ok(serde_json::json!({"ok": true, "updates": out, "updates_available": avail}))
}

#[tauri::command]
pub async fn plugin_update(app: tauri::AppHandle, id: Option<String>) -> Result<serde_json::Value, String> {
    use tauri_plugin_shell::ShellExt;
    use tauri_plugin_shell::process::CommandEvent;
    let id = id.unwrap_or_default().trim().to_string();
    if !valid_plugin_id(&id) { return Err("Bad plugin id".into()); }
    let repo = get_repo_dir();
    let target = repo.join("plugins").join(&id);
    if !target.join(".git").exists() { return Err("Not a git checkout — cannot update".into()); }
    mutating_try("plugin-update")?;
    let log = |m: &str| { crate::base::push_log(m, "launch"); let _ = app.emit("launch-log", m.to_string()); };
    log(&format!("[*] Updating plugin {id}…\n"));
    let (mut rx, _) = app.shell().command("git").args(["-C", &target.to_string_lossy(), "pull", "--ff-only"]).spawn().map_err(|e| { mutating_done(); e.to_string() })?;
    while let Some(ev) = rx.recv().await {
        match ev {
            CommandEvent::Stdout(b) | CommandEvent::Stderr(b) => log(&String::from_utf8_lossy(&b)),
            _ => {}
        }
    }
    install_requirements(&app, &repo, &id, &target, &log).await;
    mutating_done();
    log(&format!("[✓] Plugin {id} updated — restart Wan2GP to load it.\n"));
    Ok(serde_json::json!({"ok": true, "id": id}))
}

#[tauri::command]
pub fn plugin_uninstall(id: Option<String>) -> Result<serde_json::Value, String> {
    let id = id.unwrap_or_default().trim().to_string();
    if !valid_plugin_id(&id) { return Err("Bad plugin id".into()); }
    if SYSTEM_PLUGINS.contains(&id.as_str()) || BUNDLED_PLUGINS.contains(&id.as_str()) {
        return Err("System/bundled plugins cannot be uninstalled".into());
    }
    let repo = get_repo_dir();
    let target = repo.join("plugins").join(&id);
    if !target.exists() { return Err("Not installed".into()); }
    // files may be locked (Wan2GP running) — mirror upstream pending_plugin_deletions.
    match std::fs::remove_dir_all(&target) {
        Ok(()) => { scrub_config_lists(&repo, &id)?; }
        Err(e) => {
            let p = repo.join("wgp_config.json");
            let mut cfg = read_wgp_config(&repo);
            let mut list = str_list(&cfg, "pending_plugin_deletions");
            if !list.contains(&id) { list.push(id.clone()); }
            cfg["pending_plugin_deletions"] = serde_json::Value::Array(list.into_iter().map(serde_json::Value::String).collect());
            atomic_write(&p, &serde_json::to_string_pretty(&cfg).unwrap_or_default()).map_err(|e| e.to_string())?;
            return Ok(serde_json::json!({"ok": true, "id": id, "pending": true, "hint": format!("Files locked ({e}) — will delete on next Wan2GP start. Disabled now.")}));
        }
    }
    // disabling takes effect next launch even when files are gone now
    let p = repo.join("wgp_config.json");
    let mut cfg = read_wgp_config(&repo);
    for key in ["enabled_plugins", "installed_remote_plugins"] {
        let mut list = str_list(&cfg, key);
        list.retain(|x| x != &id);
        cfg[key] = serde_json::Value::Array(list.into_iter().map(serde_json::Value::String).collect());
    }
    atomic_write(&p, &serde_json::to_string_pretty(&cfg).unwrap_or_default()).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"ok": true, "id": id}))
}
#[tauri::command]
pub async fn plugin_install(app: tauri::AppHandle, url: Option<String>) -> Result<serde_json::Value, String> {
    let url = url.unwrap_or_default().trim().to_string();
    if !(url.starts_with("https://") || url.starts_with("http://") || url.contains("github.com:")) {
        return Err("Give a plugin git URL (https://github.com/…/…)".into());
    }
    if !get_repo_dir().join("wgp.py").exists() { return Err("Wan2GP not installed — run Install first".into()); }
    mutating_try("plugin-install")?;
    let r = install_plugin_inner(&app, &url).await;
    mutating_done();
    let id = r?;
    let log = |m: &str| { crate::base::push_log(m, "launch"); let _ = app.emit("launch-log", m.to_string()); };
    log(&format!("[✓] Plugin {id} installed and enabled — restart Wan2GP to load it.\n"));
    Ok(serde_json::json!({"ok": true, "id": id}))
}
fn split_github_repo(url: &str) -> Option<(String, String)> {
    let mut t = url.trim().to_string();
    if t.is_empty() { return None; }
    if let Some(i) = t.find('?') { t.truncate(i); }
    if let Some(i) = t.find('#') { t.truncate(i); }
    if t.starts_with("git@github.com:") { t = format!("https://github.com/{}", &t["git@github.com:".len()..]); }
    t = t.trim_end_matches('/').to_string();
    let low = t.to_lowercase();
    let idx = low.find("github.com/")?;
    let tail = &t[idx + "github.com/".len()..];
    let parts: Vec<&str> = tail.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() < 2 { return None; }
    let mut repo = parts[1].to_string();
    if repo.to_lowercase().ends_with(".git") { repo.truncate(repo.len() - 4); }
    if parts[0].is_empty() || repo.is_empty() { return None; }
    Some((parts[0].to_string(), repo))
}

#[derive(PartialOrd, PartialEq, Debug)]
enum VTok { Num(u64), Str(String) }
fn version_tokens(v: &str) -> Vec<VTok> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut cur_num = true;
    let flush = |cur: &mut String, cur_num: bool, out: &mut Vec<VTok>| {
        if cur.is_empty() { return; }
        if cur_num { out.push(VTok::Num(cur.parse().unwrap_or(0))); }
        else { out.push(VTok::Str(cur.to_lowercase())); }
    };
    for ch in v.chars() {
        if ch.is_ascii_alphanumeric() {
            let is_num = ch.is_ascii_digit();
            if !cur.is_empty() && is_num != cur_num { flush(&mut cur, cur_num, &mut out); cur_num = is_num; }
            else if cur.is_empty() { cur_num = is_num; }
            cur.push(ch);
        } else if !cur.is_empty() { flush(&mut cur, cur_num, &mut out); cur.clear(); }
    }
    flush(&mut cur, cur_num, &mut out);
    out
}
// port of compare_release_metadata: date first (ISO strings sort chronologically), then version.
fn release_newer(remote: &serde_json::Value, local: &serde_json::Value) -> bool {
    let (rd, ld) = (str_field(remote, "date"), str_field(local, "date"));
    if !rd.is_empty() || !ld.is_empty() { return rd > ld; }
    let (rv, lv) = (version_tokens(&str_field(remote, "version")), version_tokens(&str_field(local, "version")));
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
    if !repo.join("wgp.py").exists() { return Err("Wan2GP not installed — run Install first".into()); }
    mutating_try("plugin-refresh")?;
    let log = |m: &str| { crate::base::push_log(m, "launch"); let _ = app.emit("launch-log", m.to_string()); };
    // targets: shipped catalog urls + refreshed-library urls + installed git remotes
    let mut targets: std::collections::HashMap<String, String> = Default::default(); // id -> url
    if let Ok(s) = std::fs::read_to_string(repo.join("plugins.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
            if let Some(arr) = v.as_array() {
                for e in arr {
                    if let Some(u) = e.get("url").and_then(|x| x.as_str()) {
                        let id = plugin_id_from_url(u);
                        if !id.is_empty() { targets.entry(id).or_insert_with(|| u.to_string()); }
                    }
                }
            }
        }
    }
    for (id, e) in read_local_catalog(&repo) {
        let u = str_field(&e, "url");
        if !u.is_empty() { targets.entry(id).or_insert(u); }
    }
    if let Ok(rd) = std::fs::read_dir(repo.join("plugins")) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_dir() || !p.join(".git").exists() { continue; }
            let id = e.file_name().to_string_lossy().to_string();
            if let Ok(out) = silent_command("git").args(["-C", &p.to_string_lossy(), "config", "--get", "remote.origin.url"]).output() {
                if out.status.success() {
                    let u = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !u.is_empty() { targets.entry(id).or_insert(u); }
                }
            }
        }
    }
    log(&format!("[*] Refreshing plugin library ({} source(s))…\n", targets.len()));
    let client = reqwest::Client::builder().user_agent("wan2gp-tauri").timeout(std::time::Duration::from_secs(10)).build().unwrap_or_else(|_| reqwest::Client::new());
    let mut local_map: std::collections::HashMap<String, serde_json::Value> = read_local_catalog(&repo);
    let (mut checked, mut updated, mut avail) = (0u32, 0u32, 0u32);
    let mut ids: Vec<String> = targets.keys().cloned().collect();
    ids.sort();
    for id in ids {
        let url = &targets[&id];
        let Some((owner, name)) = split_github_repo(url) else { continue; };
        checked += 1;
        let raw = format!("https://github.com/{owner}/{name}/raw/HEAD/plugin_info.json");
        let meta: serde_json::Value = match client.get(&raw).send().await {
            Ok(r) if r.status().is_success() => match r.json().await { Ok(v) => v, Err(_) => continue },
            _ => continue,
        };
        if !meta.is_object() { continue; }
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
                    if release_newer(&entry, &cur) { avail += 1; }
                }
            }
        } else if let Some(old) = local_map.get(&id) {
            if release_newer(&entry, old) { avail += 1; }
        }
        local_map.insert(id, entry);
        updated += 1;
    }
    if updated > 0 {
        let mut arr: Vec<serde_json::Value> = local_map.into_values().collect();
        arr.sort_by(|a, b| str_field(a, "name").to_lowercase().cmp(&str_field(b, "name").to_lowercase()));
        atomic_write(&repo.join("plugins_local.json"), &serde_json::to_string_pretty(&serde_json::Value::Array(arr)).unwrap_or_default()).map_err(|e| { mutating_done(); e.to_string() })?;
    }
    mutating_done();
    log(&format!("[✓] Library refreshed: {checked} checked, {updated} updated, {avail} update(s) available.\n"));
    Ok(serde_json::json!({"ok": true, "checked": checked, "updated": updated, "updates_available": avail}))
}

fn chrono_now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

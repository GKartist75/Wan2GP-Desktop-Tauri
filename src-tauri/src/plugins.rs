//! Plugin recovery for a Wan2GP that will not start.
//!
//! Installing, updating, enabling and uninstalling plugins belongs to WanGP's
//! built-in Plugin Manager. The launcher keeps exactly one recovery action:
//! remove community plugins from `enabled_plugins` in `wgp_config.json`.
//! Plugin folders and `installed_remote_plugins` are left untouched, so WanGP
//! can re-enable them after it starts.

use crate::base::{atomic_write, get_repo_dir};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const INSTALLED_REMOTE_PLUGINS_KEY: &str = "installed_remote_plugins";

fn string_list(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|e| e.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Repo directory name from a plugin URL, matching WanGP's plugin ids.
fn plugin_id_from_url(url: &str) -> Option<String> {
    let mut t = url.trim().to_string();
    if let Some(i) = t.find("github.com:") {
        t = format!("https://github.com/{}", &t[i + "github.com:".len()..]);
    }
    t = t.trim_end_matches('/').to_string();
    if t.to_lowercase().ends_with(".git") {
        t.truncate(t.len() - 4);
    }
    let id = t.rsplit('/').next().unwrap_or("").trim();
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

/// Community plugin ids, derived from the sources WanGP already maintains:
/// the remote-install record, the shipped catalog, and git clone directories.
/// System and bundled plugins have none of those markers.
fn third_party_ids(repo: &Path, cfg: &Value) -> HashSet<String> {
    let mut ids: HashSet<String> = string_list(cfg, INSTALLED_REMOTE_PLUGINS_KEY)
        .into_iter()
        .filter(|id| !id.is_empty())
        .collect();

    if let Ok(s) = std::fs::read_to_string(repo.join("plugins.json")) {
        if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(&s) {
            for entry in arr {
                if let Some(id) = entry.get("id").and_then(Value::as_str) {
                    if !id.is_empty() {
                        ids.insert(id.to_string());
                    }
                    continue;
                }
                if let Some(url) = entry.get("url").and_then(Value::as_str) {
                    if let Some(id) = plugin_id_from_url(url) {
                        ids.insert(id);
                    }
                }
            }
        }
    }

    if let Ok(rd) = std::fs::read_dir(repo.join("plugins")) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() && p.join(".git").exists() {
                if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                    ids.insert(name.to_string());
                }
            }
        }
    }
    ids
}

fn config_path(repo: &Path) -> PathBuf {
    repo.join("wgp_config.json")
}

/// Disable every community plugin while keeping built-ins enabled.
/// Reads and writes only `enabled_plugins`; returns the ids that were removed.
fn disable_third_party_in(repo: &Path) -> Result<Value, String> {
    let path = config_path(repo);
    let text = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "{}".to_string(),
        Err(e) => return Err(format!("could not read wgp_config.json: {e}")),
    };
    let mut cfg: Value = serde_json::from_str(&text)
        .map_err(|e| format!("wgp_config.json is not valid JSON: {e}"))?;
    if !cfg.is_object() {
        return Err("wgp_config.json is not a JSON object".into());
    }

    let enabled = string_list(&cfg, "enabled_plugins");
    if enabled.is_empty() {
        return Ok(json!({"ok": true, "disabled": [], "disabled_count": 0, "kept_count": 0}));
    }

    let third_party = third_party_ids(repo, &cfg);
    let (kept, disabled): (Vec<String>, Vec<String>) = enabled
        .into_iter()
        .partition(|id| !third_party.contains(id));
    if disabled.is_empty() {
        return Ok(json!({
            "ok": true,
            "disabled": [],
            "disabled_count": 0,
            "kept_count": kept.len()
        }));
    }

    let kept_count = kept.len();
    cfg["enabled_plugins"] = json!(kept);
    let pretty = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    atomic_write(&path, &pretty).map_err(|e| format!("could not write wgp_config.json: {e}"))?;

    Ok(json!({
        "ok": true,
        "disabled": disabled,
        "disabled_count": disabled.len(),
        "kept_count": kept_count
    }))
}

/// Disable third-party plugins without deleting their folders.
#[tauri::command]
pub fn plugins_disable_third_party() -> Result<Value, String> {
    let repo = get_repo_dir();
    if !repo.join("wgp.py").exists() {
        return Err("Wan2GP not installed".into());
    }
    disable_third_party_in(&repo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_cfg(repo: &Path, cfg: &Value) {
        fs::write(
            repo.join("wgp_config.json"),
            serde_json::to_string_pretty(cfg).unwrap(),
        )
        .unwrap();
    }

    fn enabled(repo: &Path) -> Vec<String> {
        let cfg: Value =
            serde_json::from_str(&fs::read_to_string(repo.join("wgp_config.json")).unwrap())
                .unwrap();
        string_list(&cfg, "enabled_plugins")
    }

    #[test]
    fn disables_remote_catalog_and_cloned_plugins_only() {
        let repo = tempfile::tempdir().unwrap();
        fs::create_dir_all(repo.path().join("plugins/ManualClone/.git")).unwrap();
        fs::write(
            repo.path().join("plugins.json"),
            r#"[{"url":"https://github.com/someone/CatalogOnly.git"},{"url":"https://github.com/someone/Ignored","id":"ExplicitId"}]"#,
        )
        .unwrap();
        write_cfg(
            repo.path(),
            &json!({
                "enabled_plugins": [
                    "guides", "downloads", "StatusPro", "CatalogOnly", "ExplicitId",
                    "ManualClone", "LocalUnknown"
                ],
                "installed_remote_plugins": ["StatusPro"]
            }),
        );

        let r = disable_third_party_in(repo.path()).unwrap();
        assert_eq!(r["disabled_count"], 4);
        assert_eq!(
            enabled(repo.path()),
            vec!["guides", "downloads", "LocalUnknown"]
        );
        assert!(
            repo.path().join("plugins/ManualClone").exists(),
            "plugin folders must stay on disk"
        );
    }

    #[test]
    fn keeps_a_builtin_only_config_untouched() {
        let repo = tempfile::tempdir().unwrap();
        write_cfg(
            repo.path(),
            &json!({"enabled_plugins": ["guides", "configuration", "downloads"]}),
        );
        let before = fs::read(repo.path().join("wgp_config.json")).unwrap();

        let r = disable_third_party_in(repo.path()).unwrap();
        assert_eq!(r["disabled_count"], 0);
        assert_eq!(
            fs::read(repo.path().join("wgp_config.json")).unwrap(),
            before
        );
    }

    #[test]
    fn refuses_invalid_json_instead_of_overwriting_it() {
        let repo = tempfile::tempdir().unwrap();
        fs::write(repo.path().join("wgp_config.json"), "{oops").unwrap();
        let err = disable_third_party_in(repo.path()).unwrap_err();
        assert!(err.contains("not valid JSON"), "got {err}");
        assert_eq!(
            fs::read_to_string(repo.path().join("wgp_config.json")).unwrap(),
            "{oops"
        );
    }
}

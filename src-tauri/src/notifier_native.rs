//! Native Wan2GP notifications assistant (issue #35 follow-up).
//!
//! Upstream Wan2GP (12.643+) notifies natively: Apprise destinations live in
//! `wgp_config.json` (`shared/notifications/`) and delivery is in-process via
//! `import apprise` — no CLI involved. The launcher therefore helps the user
//! set *that* up instead of duplicating it:
//!   - status/load/save/test all drive upstream's own functions
//!     (`prepare_config_update`, `send_notification`) through the env python,
//!     so key names, secure-store handling and validation never drift here.
//!   - the log-driven launcher sender (features.rs) stays as the fallback for
//!     older upstream checkouts without `shared.notifications`, and is
//!     auto-disabled (desktop-config `notifier.nativeManaged`) once native
//!     events are configured — never double pings.

use crate::base::{get_repo_dir, load_config_value, silent_command, snapshot_wgp_config};
use crate::status::{get_active_env, resolve_env_python};
use std::io::Write;
use std::path::PathBuf;
use std::process::Stdio;

fn env_python() -> Option<PathBuf> {
    get_active_env()
        .get("path")
        .and_then(|p| p.as_str())
        .and_then(|raw| resolve_env_python(&get_repo_dir(), raw))
}

fn no_env() -> serde_json::Value {
    serde_json::json!({"ok": false, "error": "No active Python environment"})
}

const STATUS_SNIPPET: &str = r#"import json, sys
out = {"supported": False}
try:
    import shared.notifications.settings as S
    from shared.notifications import secure_store
except ImportError:
    print(json.dumps(out))
    sys.exit(0)
try:
    cfg = json.load(open(sys.argv[1], encoding="utf-8"))
except FileNotFoundError:
    out.update({"supported": True, "wgpConfig": False})
    print(json.dumps(out))
    sys.exit(0)
except Exception as e:
    out.update({"supported": True, "wgpConfig": False, "configError": "wgp_config.json unreadable (%s)" % type(e).__name__})
    print(json.dumps(out))
    sys.exit(0)
try:
    urls = S.configured_urls(cfg)
    keyring_error = ""
except Exception as e:
    urls, keyring_error = [], "%s: %s" % (type(e).__name__, e)
if not keyring_error:
    keyring_error = secure_store.availability_error()
try:
    from importlib.metadata import version as _dist_version
    apprise_version = _dist_version("apprise")
except Exception:
    apprise_version = ""
try:
    from importlib.metadata import version as _dist_version2
    keyring_version = _dist_version2("keyring")
except Exception:
    keyring_version = ""
import os as _os
_exe_dir = _os.path.dirname(__import__("sys").executable)
apprise_binary = _os.path.isfile(_os.path.join(_exe_dir, "apprise.exe" if _os.name == "nt" else "apprise"))
out.update({
    "supported": True,
    "wgpConfig": True,
    "secure": bool(cfg.get(S.SECURE_STORAGE_KEY, False)),
    "credentialSet": bool(str(cfg.get(S.CREDENTIAL_ID_KEY, "")).strip()),
    "urlsCount": len(urls),
    "onGeneration": bool(cfg.get(S.NOTIFY_GENERATION_KEY, False)),
    "onQueueComplete": bool(cfg.get(S.NOTIFY_QUEUE_COMPLETE_KEY, False)),
    "onQueueInterrupted": bool(cfg.get(S.NOTIFY_QUEUE_INTERRUPTED_KEY, False)),
    "keyringError": keyring_error,
    "appriseVersion": apprise_version,
    "appriseBinary": apprise_binary,
    "keyringVersion": keyring_version,
})
print(json.dumps(out))"#;

const SAVE_SNIPPET: &str = r#"import json, sys
req = json.load(sys.stdin)
try:
    from shared.notifications.settings import prepare_config_update
except ImportError:
    print(json.dumps({"ok": False, "error": "This Wan2GP version has no native notifications (shared.notifications missing)."}))
    sys.exit(0)
try:
    cfg = json.load(open(sys.argv[1], encoding="utf-8"))
except FileNotFoundError:
    print(json.dumps({"ok": False, "error": "wgp_config.json not found — install Wan2GP first."}))
    sys.exit(0)
except Exception as e:
    print(json.dumps({"ok": False, "error": "wgp_config.json unreadable (%s)." % type(e).__name__}))
    sys.exit(0)
try:
    update = prepare_config_update(cfg, req.get("urls", ""), req.get("secure", True), req.get("onGeneration", False), req.get("onQueueComplete", False), req.get("onQueueInterrupted", False))
except Exception as e:
    print(json.dumps({"ok": False, "error": "%s: %s" % (type(e).__name__, e)}))
    sys.exit(0)
cfg.update(update)
try:
    json.dump(cfg, open(sys.argv[1], "w", encoding="utf-8"), indent=2)
except Exception as e:
    print(json.dumps({"ok": False, "error": "write failed (%s)." % type(e).__name__}))
    sys.exit(0)
print(json.dumps({"ok": True, "secure": bool(update.get("notification_apprise_urls_secure", False)), "credentialSet": bool(str(update.get("notification_apprise_credential_id", "")).strip())}))"#;

const TEST_SNIPPET: &str = r#"import json, sys
req = json.load(sys.stdin)
try:
    from shared.notifications.service import send_notification
except ImportError:
    print(json.dumps({"ok": False, "error": "This Wan2GP version has no native notifications (shared.notifications missing)."}))
    sys.exit(0)
result = send_notification({"notification_apprise_urls": req.get("urls", ""), "notification_apprise_urls_secure": False}, "WanGP test notification", "WanGP remote notifications are configured correctly.")
print(json.dumps({"ok": bool(result.get("sent")), "error": result.get("error", ""), "destinations": result.get("destinations", 0), "warning": result.get("warning", "")}))"#;

const LOAD_SNIPPET: &str = r#"import json, sys
try:
    import shared.notifications.settings as S
except ImportError:
    print(json.dumps({"ok": False, "error": "This Wan2GP version has no native notifications."}))
    sys.exit(0)
try:
    cfg = json.load(open(sys.argv[1], encoding="utf-8"))
    urls = S.configured_urls(cfg)
except FileNotFoundError:
    print(json.dumps({"ok": False, "error": "wgp_config.json not found — install Wan2GP first."}))
    sys.exit(0)
except Exception as e:
    print(json.dumps({"ok": False, "error": "%s: %s" % (type(e).__name__, e)}))
    sys.exit(0)
print(json.dumps({"ok": True, "urlsText": S.apprise_urls_text(urls)}))"#;

/// Run a snippet with the repo as cwd (needed for `import shared.…`),
/// optional JSON on stdin; parse the single JSON line on stdout.
fn run_snippet(py: &PathBuf, snippet: &str, stdin_json: Option<&str>) -> Result<serde_json::Value, String> {
    let repo = get_repo_dir();
    let cfg_path = repo.join("wgp_config.json");
    let mut child = silent_command(py)
        .args(["-c", snippet])
        .arg(&cfg_path)
        .current_dir(&repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Some(payload) = stdin_json {
        child
            .stdin
            .as_mut()
            .ok_or_else(|| "cannot open child stdin".to_string())?
            .write_all(payload.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            "helper failed".to_string()
        } else {
            err
        });
    }
    serde_json::from_str::<serde_json::Value>(String::from_utf8_lossy(&out.stdout).trim())
        .map_err(|_| "helper returned invalid JSON".to_string())
}

#[tauri::command]
pub fn notifier_native_status() -> serde_json::Value {
    let Some(py) = env_python() else {
        return no_env();
    };
    match run_snippet(&py, STATUS_SNIPPET, None) {
        Ok(v) => {
            let mut r = serde_json::json!({"ok": true});
            if let Some(m) = r.as_object_mut() {
                if let Some(o) = v.as_object() {
                    for (k, val) in o {
                        m.insert(k.clone(), val.clone());
                    }
                }
            }
            r
        }
        Err(e) => serde_json::json!({"ok": false, "error": e}),
    }
}

#[tauri::command]
pub fn notifier_native_load() -> serde_json::Value {
    let Some(py) = env_python() else {
        return no_env();
    };
    match run_snippet(&py, LOAD_SNIPPET, None) {
        Ok(v) => v,
        Err(e) => serde_json::json!({"ok": false, "error": e}),
    }
}

#[tauri::command]
pub fn notifier_native_test(cfg: serde_json::Value) -> serde_json::Value {
    let urls = cfg.get("urls").and_then(|v| v.as_str()).unwrap_or("");
    if urls.trim().is_empty() {
        return serde_json::json!({"ok": false, "error": "No Apprise destinations entered"});
    }
    let Some(py) = env_python() else {
        return no_env();
    };
    let payload = serde_json::json!({"urls": urls}).to_string();
    match run_snippet(&py, TEST_SNIPPET, Some(&payload)) {
        Ok(v) => v,
        Err(e) => serde_json::json!({"ok": false, "error": e}),
    }
}

#[tauri::command]
pub fn notifier_native_save(cfg: serde_json::Value) -> serde_json::Value {
    let urls = cfg.get("urls").and_then(|v| v.as_str()).unwrap_or("");
    let secure = cfg.get("secure").and_then(|v| v.as_bool()).unwrap_or(true);
    let on_generation = cfg.get("onGeneration").and_then(|v| v.as_bool()).unwrap_or(false);
    let on_complete = cfg.get("onQueueComplete").and_then(|v| v.as_bool()).unwrap_or(false);
    let on_interrupted = cfg
        .get("onQueueInterrupted")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let Some(py) = env_python() else {
        return no_env();
    };
    // Backup-first: same convention as every other wgp_config.json write.
    let snapshot = snapshot_wgp_config();
    let payload = serde_json::json!({
        "urls": urls,
        "secure": secure,
        "onGeneration": on_generation,
        "onQueueComplete": on_complete,
        "onQueueInterrupted": on_interrupted,
    })
    .to_string();
    let saved = match run_snippet(&py, SAVE_SNIPPET, Some(&payload)) {
        Ok(v) => v,
        Err(e) => return serde_json::json!({"ok": false, "error": e}),
    };
    if saved.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        return saved;
    }
    // Native events on → the log-driven launcher sender must stay off
    // (user decision: drop our sender once native is configured).
    let native_managed = on_generation || on_complete || on_interrupted;
    let mut full = load_config_value();
    if let Some(m) = full.as_object_mut() {
        let cur = m.get("notifier").cloned().unwrap_or(serde_json::Value::Null);
        let mut clean = crate::features::notifier_normalize(&cur);
        if let Some(c) = clean.as_object_mut() {
            c.insert("nativeManaged".into(), serde_json::json!(native_managed));
            if native_managed {
                c.insert("enabled".into(), serde_json::json!(false));
            }
        }
        m.insert("notifier".into(), clean);
    }
    match crate::config::config_save(full) {
        Ok(_) => {
            let mut r = saved;
            if let Some(m) = r.as_object_mut() {
                m.insert("nativeManaged".into(), serde_json::json!(native_managed));
                if let Some(s) = snapshot {
                    m.insert("snapshot".into(), serde_json::json!(s));
                }
            }
            r
        }
        Err(e) => serde_json::json!({"ok": false, "error": e}),
    }
}

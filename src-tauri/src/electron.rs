//! Legacy Electron launcher detection and removal.
use std::path::PathBuf;
use crate::base::*;

#[tauri::command] pub fn detect_electron() -> serde_json::Value {
    #[cfg(not(windows))] { return serde_json::json!({"found": false}); }
    #[cfg(windows)] {
        let mut best: Option<serde_json::Value> = None;
        // 1) Add/Remove Programs registry scan (per-user + machine hives)
        for hive in ["HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall", "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall", "HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall"] {
            let Ok(o) = std::process::Command::new("reg").args(["query", hive, "/s"]).output() else { continue };
            if !o.status.success() { continue; }
            let s = String::from_utf8_lossy(&o.stdout);
            let (mut name, mut ver, mut un, mut quiet, mut loc) = (String::new(), String::new(), String::new(), String::new(), String::new());
            let flush = |name: &mut String, ver: &mut String, un: &mut String, quiet: &mut String, loc: &mut String, best: &mut Option<serde_json::Value>| {
                let n = name.to_lowercase();
                if n.contains("wan2gp") && !n.contains("tauri")
                    && !un.to_lowercase().contains("tauri") && !loc.to_lowercase().contains("tauri") {
                    *best = Some(serde_json::json!({"found": true, "name": name.trim(), "version": ver.trim(),
                        "uninstallString": un.trim(), "quietUninstall": quiet.trim(), "installLocation": loc.trim()}));
                }
                name.clear(); ver.clear(); un.clear(); quiet.clear(); loc.clear();
            };
            for line in s.lines() {
                if line.trim_start().starts_with("HKEY_") { flush(&mut name, &mut ver, &mut un, &mut quiet, &mut loc, &mut best); continue; }
                if let Some((k, v)) = reg_val(line) {
                    match k.as_str() {
                        "DisplayName" => name = v, "DisplayVersion" => ver = v,
                        "UninstallString" => un = v, "QuietUninstallString" => quiet = v,
                        "InstallLocation" => loc = v, _ => {}
                    }
                }
            }
            flush(&mut name, &mut ver, &mut un, &mut quiet, &mut loc, &mut best);
            if best.is_some() { return best.unwrap(); }
        }
        // 2) filesystem fallback: per-user Programs dir (Electron Builder default)
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let progs = PathBuf::from(local).join("Programs");
            if let Ok(rd) = std::fs::read_dir(&progs) {
                for e in rd.flatten() {
                    let p = e.path();
                    let f = p.file_name().and_then(|x| x.to_str()).unwrap_or("").to_string();
                    let fl = f.to_lowercase();
                    if !p.is_dir() || !fl.contains("wan2gp") || fl.contains("tauri") { continue; }
                    let has_un = std::fs::read_dir(&p).map(|r| r.flatten().any(|x| x.file_name().to_string_lossy().to_lowercase().starts_with("uninstall") && x.path().extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("exe")))).unwrap_or(false);
                    if has_un { return serde_json::json!({"found": true, "name": f, "version": "", "uninstallString": "", "quietUninstall": "", "installLocation": p.to_string_lossy()}); }
                }
            }
        }
        best.unwrap_or(serde_json::json!({"found": false}))
    }
}
#[tauri::command] pub async fn uninstall_electron() -> Result<serde_json::Value, String> {
    let det = detect_electron();
    if !det.get("found").and_then(|v| v.as_bool()).unwrap_or(false) {
        return Ok(serde_json::json!({"ok": false, "error": "Legacy Electron launcher not found"}));
    }
    let loc = det.get("installLocation").and_then(|v| v.as_str()).unwrap_or("").to_string();
    // best-effort: kill running copies from that dir (not the uninstallers, never us — we live elsewhere)
    if !loc.is_empty() {
        if let Ok(rd) = std::fs::read_dir(&loc) {
            for e in rd.flatten() {
                let p = e.path();
                let is_exe = p.extension().and_then(|x| x.to_str()).is_some_and(|x| x.eq_ignore_ascii_case("exe"));
                let f = p.file_name().and_then(|x| x.to_str()).unwrap_or("");
                if is_exe && !f.to_lowercase().starts_with("uninstall") {
                    let _ = std::process::Command::new("taskkill").args(["/F", "/IM", f]).output();
                }
            }
        }
    }
    let mut cmdline = det.get("quietUninstall").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if cmdline.is_empty() { cmdline = det.get("uninstallString").and_then(|v| v.as_str()).unwrap_or("").to_string(); }
    if cmdline.is_empty() {
        // last resort: Uninstall*.exe in the install dir (Electron Builder layout)
        if !loc.is_empty() {
            if let Ok(rd) = std::fs::read_dir(&loc) {
                for e in rd.flatten() {
                    let p = e.path();
                    let f = p.file_name().and_then(|x| x.to_str()).unwrap_or("");
                    if f.to_lowercase().starts_with("uninstall") && p.extension().and_then(|x| x.to_str()).is_some_and(|x| x.eq_ignore_ascii_case("exe")) {
                        cmdline = format!("\"{}\"", p.to_string_lossy()); break;
                    }
                }
            }
        }
    }
    if cmdline.is_empty() { return Ok(serde_json::json!({"ok": false, "error": "No uninstaller registered"})); }
    let (exe, mut args) = split_cmdline(&cmdline);
    if !args.iter().any(|a| a.eq_ignore_ascii_case("/S")) { args.push("/S".into()); } // NSIS silent flag
    let out = std::process::Command::new(&exe).args(&args).output().map_err(|e| e.to_string())?;
    std::thread::sleep(std::time::Duration::from_secs(2));
    let gone = !detect_electron().get("found").and_then(|v| v.as_bool()).unwrap_or(false);
    Ok(serde_json::json!({"ok": out.status.success() || gone, "removed": gone, "exit": out.status.code()}))
}

#![allow(dead_code, unused_imports)]

use crate::platform::unix;
use crate::platform::{Capability, Listener, Platform, PortOwner, ProcessInfo};
use std::path::PathBuf;

pub struct MacosPlatform;

impl Platform for MacosPlatform {
    fn os_name(&self) -> &'static str {
        "macos"
    }

    fn set_auto_start(&self, enabled: bool, exe: &str) -> serde_json::Value {
        if exe.is_empty() {
            return serde_json::json!({"ok": false, "success": false, "error": "executable path unavailable"});
        }
        let home = std::env::var("HOME").unwrap_or_default();
        if home.is_empty() {
            return serde_json::json!({"ok": false, "success": false, "error": "HOME not set"});
        }
        let agents_dir = std::path::PathBuf::from(&home).join("Library/LaunchAgents");
        let plist = agents_dir.join("com.wan2gp.desktop.plist");
        if enabled {
            if std::fs::create_dir_all(&agents_dir).is_err() {
                return serde_json::json!({"ok": false, "success": false, "error": "failed to create LaunchAgents dir"});
            }
            let content = format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.wan2gp.desktop</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
</dict>
</plist>
"#,
                exe = exe
            );
            if std::fs::write(&plist, content).is_ok() {
                serde_json::json!({"ok": true, "success": true, "enabled": true})
            } else {
                serde_json::json!({"ok": false, "success": false, "error": "failed to write LaunchAgent"})
            }
        } else {
            let _ = std::fs::remove_file(&plist);
            serde_json::json!({"ok": true, "success": true, "enabled": false})
        }
    }

    fn open_path(&self, path: &str) -> Result<(), String> {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn process_signature(&self, pid: u32) -> Option<(String, String)> {
        if pid == 0 {
            return None;
        }
        let cmd = crate::base::silent_command("ps")
            .args(["-p", &pid.to_string(), "-o", "command="])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())?;
        let exe = crate::base::silent_command("ps")
            .args(["-p", &pid.to_string(), "-o", "comm="])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        Some((exe, cmd))
    }

    fn port_owner(&self, port: u64) -> Option<PortOwner> {
        unix::port_owner(port, false, |pid| self.process_signature(pid))
    }

    fn port_listeners(&self, ports: &[u64]) -> Result<Vec<Listener>, String> {
        Ok(self
            .tcp_listeners()?
            .into_iter()
            .filter(|l| ports.contains(&l.port))
            .collect())
    }

    fn tcp_listeners(&self) -> Result<Vec<Listener>, String> {
        // macOS has no iproute2: lsof is the only listener source, and it ships
        // with the OS.
        unix::tcp_listeners(false)
    }

    fn scan_processes(&self, marker: &str) -> Result<Vec<ProcessInfo>, String> {
        unix::scan_processes(marker)
    }

    /// macOS: `/usr/bin` for the system copies, Homebrew (both prefixes — Apple
    /// Silicon uses /opt/homebrew, Intel /usr/local) and the python.org
    /// framework layout. A GUI-launched app inherits launchd's minimal PATH, so
    /// these absolute probes are what make Homebrew installs visible.
    fn tool_candidates(&self, tool: &str, home: &str) -> Vec<PathBuf> {
        let p = |s: String| PathBuf::from(s);
        match tool {
            "git" => vec![
                p("/usr/bin/git".into()),
                p("/opt/homebrew/bin/git".into()),
                p("/usr/local/bin/git".into()),
            ],
            "conda" => vec![
                p(format!("{home}/miniconda3/bin/conda")),
                p(format!("{home}/anaconda3/bin/conda")),
                p(format!("{home}/opt/miniconda3/bin/conda")),
                p(format!("{home}/opt/anaconda3/bin/conda")),
                p("/opt/homebrew/bin/conda".into()),
                p("/usr/local/bin/conda".into()),
            ],
            "python" => vec![
                p("/opt/homebrew/bin/python3".into()),
                p("/usr/local/bin/python3".into()),
                p("/usr/bin/python3".into()),
                p(format!("{home}/Library/Python/3.12/bin/python3")),
                p("/Library/Frameworks/Python.framework/Versions/3.11/bin/python3".into()),
                p("/Library/Frameworks/Python.framework/Versions/3.10/bin/python3".into()),
                p("/Library/Frameworks/Python.framework/Versions/3.12/bin/python3".into()),
            ],
            "uv" => vec![
                p(format!("{home}/.local/bin/uv")),
                p("/opt/homebrew/bin/uv".into()),
                p("/usr/local/bin/uv".into()),
                p(format!("{home}/.cargo/bin/uv")),
            ],
            _ => vec![],
        }
    }

    fn tool_on_path(&self, tool: &str) -> bool {
        unix::which_path(tool).is_some()
    }

    fn lookup_on_path(&self, tool: &str) -> Option<String> {
        unix::which_path(tool)
    }

    fn kill_pid(&self, pid: u32) {
        unix::kill_pid(pid);
    }

    fn detect_gpu(&self) -> serde_json::Value {
        #[cfg(target_os = "macos")]
        {
            crate::hw::macos_gpu_fallback_info()
        }
        #[cfg(not(target_os = "macos"))]
        {
            serde_json::json!({"vendor":"unknown","name":"","vramMB":"0","driverVersion":"","raw":"not implemented"})
        }
    }

    fn supports(&self, capability: Capability) -> bool {
        matches!(capability, Capability::AutoStart)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A GUI-launched app gets launchd's minimal PATH, so Homebrew (and any
    /// other non-/usr/bin install) must be found through these absolute
    /// probes or the launcher reports "not installed" for a working tool.
    #[test]
    fn candidate_table_covers_homebrew_and_python_org() {
        let home = "/Users/u";
        for tool in ["git", "uv"] {
            assert!(
                MacosPlatform
                    .tool_candidates(tool, home)
                    .contains(&PathBuf::from(format!("/opt/homebrew/bin/{tool}"))),
                "{tool}: Apple Silicon Homebrew prefix missing"
            );
        }
        let py = MacosPlatform.tool_candidates("python", home);
        assert!(py.contains(&PathBuf::from("/usr/bin/python3")));
        assert!(py.iter().any(|p| p
            .to_string_lossy()
            .contains("/Library/Frameworks/Python.framework/Versions/3.11/bin/python3")));
        for tool in ["git", "conda", "python", "uv"] {
            for c in MacosPlatform.tool_candidates(tool, home) {
                // `is_absolute` uses the host's rules; these tables are POSIX.
                assert!(
                    c.to_string_lossy().starts_with('/'),
                    "{tool}: {} is not rooted",
                    c.display()
                );
            }
        }
    }
}

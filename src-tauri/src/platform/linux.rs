#![allow(dead_code, unused_imports)]

use crate::platform::unix;
use crate::platform::{Capability, Listener, Platform, PortOwner, ProcessInfo};
use std::path::PathBuf;

pub struct LinuxPlatform;

impl Platform for LinuxPlatform {
    fn os_name(&self) -> &'static str {
        "linux"
    }

    fn set_auto_start(&self, enabled: bool, exe: &str) -> serde_json::Value {
        if exe.is_empty() {
            return serde_json::json!({"ok": false, "success": false, "error": "executable path unavailable"});
        }
        let home = std::env::var("HOME").unwrap_or_default();
        if home.is_empty() {
            return serde_json::json!({"ok": false, "success": false, "error": "HOME not set"});
        }
        let autostart_dir = std::path::PathBuf::from(&home).join(".config/autostart");
        let file = autostart_dir.join("wan2gp-desktop.desktop");
        if enabled {
            if std::fs::create_dir_all(&autostart_dir).is_err() {
                return serde_json::json!({"ok": false, "success": false, "error": "failed to create autostart dir"});
            }
            let content = format!(
                "[Desktop Entry]\nType=Application\nName=Wan2GP Desktop Launcher\nExec=\"{exe}\"\nX-GNOME-Autostart-enabled=true\n",
                exe = exe
            );
            if std::fs::write(&file, content).is_ok() {
                serde_json::json!({"ok": true, "success": true, "enabled": true})
            } else {
                serde_json::json!({"ok": false, "success": false, "error": "failed to write autostart entry"})
            }
        } else {
            let _ = std::fs::remove_file(&file);
            serde_json::json!({"ok": true, "success": true, "enabled": false})
        }
    }

    fn create_desktop_shortcut(
        &self,
        repo: &std::path::Path,
        _py: &std::path::Path,
    ) -> serde_json::Value {
        let home = std::env::var("HOME").unwrap_or_default();
        if home.is_empty() {
            return serde_json::json!({"ok": false, "error": "HOME not set"});
        }
        let exe = std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        if exe.is_empty() {
            return serde_json::json!({"ok": false, "error": "executable path unavailable"});
        }
        let desktop_dir = std::path::PathBuf::from(&home).join("Desktop");
        let apps_dir = std::path::PathBuf::from(&home).join(".local/share/applications");
        let _ = std::fs::create_dir_all(&apps_dir);
        let _ = std::fs::create_dir_all(&desktop_dir);
        let content = format!(
            "[Desktop Entry]\nType=Application\nName=Wan2GP Desktop Launcher\nExec=\"{exe}\"\nPath={repo}\nIcon=applications-graphics\nTerminal=false\nCategories=Graphics;AudioVideo;\n",
            exe = exe,
            repo = repo.display()
        );
        let mut ok = true;
        for dir in [&apps_dir, &desktop_dir] {
            let path = dir.join("wan2gp-desktop-launcher.desktop");
            if std::fs::write(&path, &content).is_err() {
                ok = false;
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
            }
        }
        if ok {
            serde_json::json!({"ok": true, "success": true, "path": apps_dir.join("wan2gp-desktop-launcher.desktop").to_string_lossy().to_string()})
        } else {
            serde_json::json!({"ok": false, "error": "Failed to write one or more .desktop files"})
        }
    }

    fn open_path(&self, path: &str) -> Result<(), String> {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn process_signature(&self, pid: u32) -> Option<(String, String)> {
        if pid == 0 {
            return None;
        }
        let cmd = std::fs::read_to_string(format!("/proc/{pid}/cmdline"))
            .ok()
            .map(|s| s.replace('\0', " ").trim().to_string())
            .filter(|s| !s.is_empty())?;
        let exe = std::fs::read_link(format!("/proc/{pid}/exe"))
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        Some((exe, cmd))
    }

    fn port_owner(&self, port: u64) -> Option<PortOwner> {
        unix::port_owner(port, true, |pid| self.process_signature(pid))
    }

    fn kill_pid(&self, pid: u32) {
        unix::kill_pid(pid);
    }

    fn port_listeners(&self, ports: &[u64]) -> Result<Vec<Listener>, String> {
        Ok(self
            .tcp_listeners()?
            .into_iter()
            .filter(|l| ports.contains(&l.port))
            .collect())
    }

    fn tcp_listeners(&self) -> Result<Vec<Listener>, String> {
        unix::tcp_listeners(true)
    }

    fn scan_processes(&self, marker: &str) -> Result<Vec<ProcessInfo>, String> {
        unix::scan_processes(marker)
    }

    /// Linux installs live in `/usr`, `/usr/local`, the per-user dot dirs and
    /// (Fedora) `/snap`. Pure table: nothing here touches the filesystem.
    fn tool_candidates(&self, tool: &str, home: &str) -> Vec<PathBuf> {
        let p = |s: String| PathBuf::from(s);
        match tool {
            "git" => vec![
                p("/usr/bin/git".into()),
                p("/usr/local/bin/git".into()),
                p("/snap/bin/git".into()),
            ],
            "conda" => vec![
                p(format!("{home}/miniconda3/bin/conda")),
                p(format!("{home}/anaconda3/bin/conda")),
                p(format!("{home}/.conda/bin/conda")),
                p("/opt/miniconda3/bin/conda".into()),
                p("/opt/anaconda3/bin/conda".into()),
            ],
            "python" => vec![
                p("/usr/bin/python3".into()),
                p("/usr/local/bin/python3".into()),
            ],
            // Git's and Astral's installers both use ~/.local/bin; cargo's is
            // the fallback for `cargo install uv`.
            "uv" => vec![
                p(format!("{home}/.local/bin/uv")),
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

    fn detect_gpu(&self) -> serde_json::Value {
        #[cfg(target_os = "linux")]
        {
            crate::hw::linux_gpu_fallback_info()
        }
        #[cfg(not(target_os = "linux"))]
        {
            serde_json::json!({"vendor":"unknown","name":"","vramMB":"0","driverVersion":"","raw":"not implemented"})
        }
    }

    fn supports(&self, capability: Capability) -> bool {
        matches!(
            capability,
            Capability::KernelWheels | Capability::DesktopShortcut | Capability::AutoStart
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_table_covers_stock_linux_layouts() {
        let home = "/home/u";
        let git = LinuxPlatform.tool_candidates("git", home);
        assert!(git.contains(&PathBuf::from("/usr/bin/git")));
        assert!(git.contains(&PathBuf::from("/usr/local/bin/git")));
        let uv = LinuxPlatform.tool_candidates("uv", home);
        assert!(uv.contains(&PathBuf::from("/home/u/.local/bin/uv")));
        // Only spawnable images: no .bat shims, no bare names. `is_absolute`
        // is Windows-semantics on this host, hence the leading-slash check.
        for tool in ["git", "conda", "python", "uv"] {
            for c in LinuxPlatform.tool_candidates(tool, home) {
                assert!(
                    c.to_string_lossy().starts_with('/'),
                    "{tool}: {} is not rooted",
                    c.display()
                );
                assert_ne!(c.extension().and_then(|e| e.to_str()), Some("bat"));
            }
        }
    }
}

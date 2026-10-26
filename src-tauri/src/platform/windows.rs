#![allow(dead_code, unused_imports)]

use crate::platform::{Capability, Listener, Platform, PortOwner, ProcessInfo};
use serde_json::Value;
use std::path::PathBuf;

pub struct WindowsPlatform;

impl Platform for WindowsPlatform {
    fn os_name(&self) -> &'static str {
        "windows"
    }

    fn set_auto_start(&self, enabled: bool, exe: &str) -> serde_json::Value {
        let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
        let ok = if enabled && !exe.is_empty() {
            crate::base::silent_command("reg")
                .args([
                    "add",
                    key,
                    "/v",
                    "Wan2GPDesktop",
                    "/t",
                    "REG_SZ",
                    "/d",
                    &format!("\"{exe}\""),
                    "/f",
                ])
                .output()
                .is_ok_and(|o| o.status.success())
        } else {
            crate::base::silent_command("reg")
                .args(["delete", key, "/v", "Wan2GPDesktop", "/f"])
                .output()
                .is_ok_and(|o| o.status.success())
        };
        serde_json::json!({"ok": ok, "success": ok, "enabled": enabled})
    }

    fn create_desktop_shortcut(
        &self,
        repo: &std::path::Path,
        py: &std::path::Path,
    ) -> serde_json::Value {
        let desktop = std::env::var("USERPROFILE")
            .map(|p| std::path::PathBuf::from(p).join("Desktop"))
            .unwrap_or_else(|_| std::path::PathBuf::from("C:\\Users\\Public\\Desktop"));
        let lnk = desktop.join("Wan2GP Tauri.lnk");
        let ps = format!(
            "$s=New-Object -ComObject WScript.Shell; $l=$s.CreateShortcut('{}'); $l.TargetPath='{}'; $l.Arguments='wgp.py'; $l.WorkingDirectory='{}'; $l.Description='Wan2GP Tauri'; $l.Save()",
            lnk.display(),
            py.display(),
            repo.display()
        );
        let ok = crate::base::silent_command("powershell")
            .args(["-NoProfile", "-Command", &ps])
            .output()
            .is_ok_and(|o| o.status.success());
        if ok {
            serde_json::json!({"ok": true, "success": true, "path": lnk.to_string_lossy().to_string()})
        } else {
            serde_json::json!({"ok": false, "error": "Failed to create shortcut"})
        }
    }

    fn open_path(&self, path: &str) -> Result<(), String> {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn process_signature(&self, pid: u32) -> Option<(String, String)> {
        if pid == 0 {
            return None;
        }
        let ps = format!(
            "Get-CimInstance Win32_Process -Filter \"ProcessId='{pid}'\" | ForEach-Object {{ $_.ExecutablePath + '|' + $_.CommandLine }}"
        );
        let o = crate::base::silent_command("powershell")
            .args(["-NoProfile", "-Command", &ps])
            .output()
            .ok()?;
        if !o.status.success() {
            return None;
        }
        let line = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if line.is_empty() {
            return None;
        }
        let mut parts = line.splitn(2, '|');
        match (parts.next(), parts.next()) {
            (Some(exe), Some(cmd)) if !cmd.trim().is_empty() => {
                Some((exe.to_string(), cmd.to_string()))
            }
            _ => None,
        }
    }

    fn port_owner(&self, port: u64) -> Option<PortOwner> {
        let ps = format!(
            "Get-NetTCPConnection -LocalPort {port} -State Listen | Select-Object -First 1 -ExpandProperty OwningProcess"
        );
        let o = crate::base::silent_command("powershell")
            .args(["-NoProfile", "-Command", &ps])
            .output()
            .ok()?;
        if !o.status.success() {
            return None;
        }
        let pid_s = String::from_utf8_lossy(&o.stdout).trim().to_string();
        let pid: u32 = pid_s.parse().ok()?;
        let ps2 = format!("(Get-Process -Id {pid} -ErrorAction SilentlyContinue).ProcessName");
        let name = crate::base::silent_command("powershell")
            .args(["-NoProfile", "-Command", &ps2])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_default();
        let ps3 = format!(
            "(Get-CimInstance Win32_Process -Filter \"ProcessId={pid}\" | Select-Object -ExpandProperty CommandLine)"
        );
        let cmd = crate::base::silent_command("powershell")
            .args(["-NoProfile", "-Command", &ps3])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .chars()
                    .take(300)
                    .collect::<String>()
            })
            .unwrap_or_default();
        let ours = cmd.to_lowercase().contains("wgp.py");
        Some(PortOwner {
            pid,
            name,
            cmd,
            ours,
        })
    }

    fn kill_pid(&self, pid: u32) {
        let _ = crate::base::silent_command("taskkill")
            .args(["/pid", &pid.to_string(), "/f", "/t"])
            .output();
    }

    fn open_task_manager(&self) -> Result<Value, String> {
        // ponytail: the ONE spawn that deliberately keeps a visible console —
        // silent_command would hide the window the user asked to see.
        std::process::Command::new("taskmgr.exe")
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({"ok": true, "success": true}))
    }

    fn port_listeners(&self, ports: &[u64]) -> Result<Vec<Listener>, String> {
        let list = ports
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(",");
        // Get-NetTCPConnection joins the socket to the owning process in one
        // call; Get-Process supplies the image name for the log line.
        let ps = format!("Get-NetTCPConnection -LocalPort {list} -State Listen | ForEach-Object {{ $p = Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue; if ($p) {{ $p.Id.ToString() + '|' + $p.ProcessName + '|' + $_.LocalPort }} }}");
        let o = crate::base::silent_command("powershell")
            .args(["-NoProfile", "-Command", &ps])
            .output()
            .map_err(|e| format!("port scan ({list}) spawn failed: {e}"))?;
        if !o.status.success() {
            let code = o.status.code().unwrap_or(-1);
            let err: String = String::from_utf8_lossy(&o.stderr)
                .chars()
                .take(200)
                .collect();
            // Exit 1 + "No matching" = no listener on these ports (the CLEAN
            // case), not a failure.
            if code == 1 && err.contains("No matching") {
                return Ok(Vec::new());
            }
            return Err(format!("port scan ({list}) failed (exit {code}): {err}"));
        }
        Ok(parse_listener_rows(&String::from_utf8_lossy(&o.stdout)))
    }

    fn tcp_listeners(&self) -> Result<Vec<Listener>, String> {
        let ps = "Get-NetTCPConnection -State Listen | ForEach-Object { $p = Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue; if ($p) { $p.Id.ToString() + '|' + $p.ProcessName + '|' + $_.LocalPort } }";
        let o = crate::base::silent_command("powershell")
            .args(["-NoProfile", "-Command", ps])
            .output()
            .map_err(|e| format!("listener scan spawn failed: {e}"))?;
        if !o.status.success() {
            let code = o.status.code().unwrap_or(-1);
            let err: String = String::from_utf8_lossy(&o.stderr)
                .chars()
                .take(200)
                .collect();
            if code == 1 && err.contains("No matching") {
                return Ok(Vec::new());
            }
            return Err(format!("listener scan failed (exit {code}): {err}"));
        }
        Ok(parse_listener_rows(&String::from_utf8_lossy(&o.stdout)))
    }

    fn scan_processes(&self, marker: &str) -> Result<Vec<ProcessInfo>, String> {
        // The marker is interpolated into a single-quoted PowerShell string, so
        // a quote would break out of it. Callers pass fixed literals; refuse
        // anything else instead of building a malformed query.
        if marker.is_empty() || marker.contains('\'') || marker.contains('"') {
            return Err(format!("unsupported process marker: {marker:?}"));
        }
        // python.exe OR pythonw.exe: windowed interpreters have no console but
        // run the server just the same.
        let ps = format!("Get-CimInstance Win32_Process -Filter \"Name='python.exe' OR Name='pythonw.exe'\" | Where-Object {{ $_.CommandLine -like '*{marker}*' }} | ForEach-Object {{ $_.ProcessId + '|' + $_.ExecutablePath + '|' + $_.CommandLine }}");
        let o = crate::base::silent_command("powershell")
            .args(["-NoProfile", "-Command", &ps])
            .output()
            .map_err(|e| format!("process scan spawn failed: {e}"))?;
        if !o.status.success() {
            let code = o.status.code().unwrap_or(-1);
            let err: String = String::from_utf8_lossy(&o.stderr)
                .chars()
                .take(300)
                .collect();
            return Err(format!("process scan failed (exit {code}): {err}"));
        }
        Ok(parse_process_rows(&String::from_utf8_lossy(&o.stdout)))
    }

    fn tool_candidates(&self, tool: &str, home: &str) -> Vec<PathBuf> {
        // Only directly-spawnable images (never .bat — CreateProcess can't run
        // those without cmd /C).
        let p = |s: String| PathBuf::from(s);
        match tool {
            "git" => vec![
                p("C:\\Program Files\\Git\\bin\\git.exe".into()),
                p("C:\\Program Files (x86)\\Git\\bin\\git.exe".into()),
            ],
            // Scripts\conda.exe first: directly executable (condabin\conda.bat
            // is not a valid process image). ProgramData covers machine-wide
            // installs, which stock Miniconda/Anaconda offer and which used to
            // be invisible (a second 3 GB copy got installed instead).
            "conda" => vec![
                p(format!("{home}\\Miniconda3\\Scripts\\conda.exe")),
                p(format!("{home}\\Anaconda3\\Scripts\\conda.exe")),
                p(format!("{home}\\miniconda3\\Scripts\\conda.exe")),
                p("C:\\ProgramData\\Anaconda3\\Scripts\\conda.exe".into()),
                p("C:\\ProgramData\\Miniconda3\\Scripts\\conda.exe".into()),
                p("C:\\ProgramData\\miniconda3\\Scripts\\conda.exe".into()),
                p("C:\\Anaconda3\\Scripts\\conda.exe".into()),
                p("C:\\Miniconda3\\Scripts\\conda.exe".into()),
            ],
            // py launcher (system-placed; python.org and the new PyManager keep
            // different copies — C:\Windows\py.exe is absent on the latter).
            "py" => vec![
                p("C:\\Windows\\py.exe".into()),
                p(format!(
                    "{home}\\AppData\\Local\\Programs\\Python\\Launcher\\py.exe"
                )),
                p("C:\\Program Files\\PyManager\\py.exe".into()),
            ],
            "python" => vec![
                p(format!(
                    "{home}\\AppData\\Local\\Programs\\Python\\Python311\\python.exe"
                )),
                p(format!(
                    "{home}\\AppData\\Local\\Programs\\Python\\Python310\\python.exe"
                )),
                p(format!(
                    "{home}\\AppData\\Local\\Programs\\Python\\Python312\\python.exe"
                )),
                p(format!(
                    "{home}\\AppData\\Local\\Programs\\Python\\Python313\\python.exe"
                )),
                p("C:\\Python311\\python.exe".into()),
                p("C:\\Python310\\python.exe".into()),
                p("C:\\Python312\\python.exe".into()),
                p("C:\\Program Files\\Python311\\python.exe".into()),
                p("C:\\Program Files\\Python310\\python.exe".into()),
                p("C:\\Program Files\\Python312\\python.exe".into()),
            ],
            // Astral-script and cargo defaults: on PATH for new processes but
            // invisible to a running launcher until a registry refresh — the
            // exact "not found here, already installed there" split.
            "uv" => vec![
                p(format!("{home}\\.local\\bin\\uv.exe")),
                p(format!("{home}\\.cargo\\bin\\uv.exe")),
            ],
            _ => vec![],
        }
    }

    fn tool_on_path(&self, tool: &str) -> bool {
        crate::base::tool_usable(tool)
    }

    fn lookup_on_path(&self, tool: &str) -> Option<String> {
        where_hits(tool)
            .into_iter()
            .find(|p| !p.to_string_lossy().to_lowercase().contains("windowsapps"))
            .map(|p| p.to_string_lossy().to_string())
    }

    fn detect_gpu(&self) -> serde_json::Value {
        #[cfg(target_os = "windows")]
        {
            crate::hw::windows_gpu_fallback_info()
        }
        #[cfg(not(target_os = "windows"))]
        {
            serde_json::json!({"vendor":"unknown","name":"","vramMB":"0","driverVersion":"","raw":"not implemented"})
        }
    }

    fn supports(&self, capability: Capability) -> bool {
        match capability {
            Capability::TaskManager
            | Capability::DesktopShortcut
            | Capability::Dlss
            | Capability::KernelWheels
            | Capability::TritonRepair
            | Capability::AutoStart => true,
        }
    }
}

/// `pid|name|port` rows (one `|`-joined line per listener).
fn parse_listener_rows(stdout: &str) -> Vec<Listener> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut it = line.splitn(3, '|');
            let pid = it.next()?.trim().parse::<u32>().ok()?;
            let name = it.next()?.trim().to_string();
            let port = it.next()?.trim().parse::<u64>().ok()?;
            (pid != 0 && port != 0).then_some(Listener { pid, name, port })
        })
        .collect()
}

/// `pid|exe|cmdline` rows. The cmdline may itself contain `|`, so only the
/// first two separators split; a row with an empty command line is dropped
/// (unverifiable owner, never a kill candidate).
fn parse_process_rows(stdout: &str) -> Vec<ProcessInfo> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut it = line.splitn(3, '|');
            let pid = it.next()?.trim().parse::<u32>().ok()?;
            let exe = it.next()?.trim().to_string();
            let cmd = it.next()?.trim().to_string();
            (pid != 0 && !cmd.is_empty()).then_some(ProcessInfo { pid, exe, cmd })
        })
        .collect()
}

/// `where <exe>` hits, in PATH order. Used to find real installs that are not
/// yet visible to this process's (stale) environment.
pub(crate) fn where_hits(exe: &str) -> Vec<PathBuf> {
    crate::base::silent_command("where")
        .arg(exe)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listener_rows_parse_and_reject_junk() {
        let out = "4242|python3.12|7860\n5|node|7861\nnotapid|node|7862\n77|node|nope\n";
        assert_eq!(
            parse_listener_rows(out),
            vec![
                Listener {
                    pid: 4242,
                    name: "python3.12".into(),
                    port: 7860
                },
                Listener {
                    pid: 5,
                    name: "node".into(),
                    port: 7861
                },
            ]
        );
    }

    #[test]
    fn process_rows_keep_pipes_inside_the_command_line() {
        let out = "4242|C:\\envs\\py311\\python.exe|python.exe wgp.py --server | tee log\n9||\n";
        assert_eq!(
            parse_process_rows(out),
            vec![ProcessInfo {
                pid: 4242,
                exe: "C:\\envs\\py311\\python.exe".into(),
                cmd: "python.exe wgp.py --server | tee log".into(),
            }]
        );
    }

    #[test]
    fn scan_marker_refuses_shell_metacharacters() {
        // Validation rejects before any process is spawned: cross-platform.
        assert!(WindowsPlatform.scan_processes("wgp'py").is_err());
        assert!(WindowsPlatform.scan_processes("").is_err());
        // The accepted path actually runs the PowerShell probe.
        #[cfg(windows)]
        assert!(WindowsPlatform.scan_processes("wgp.py").is_ok());
    }

    #[test]
    fn candidate_tables_cover_stock_layouts() {
        let home = "C:\\Users\\t";
        let git = WindowsPlatform.tool_candidates("git", home);
        assert!(git.contains(&PathBuf::from("C:\\Program Files\\Git\\bin\\git.exe")));
        let conda = WindowsPlatform.tool_candidates("conda", home);
        assert!(conda.iter().any(|p| p
            .to_string_lossy()
            .contains("Miniconda3\\Scripts\\conda.exe")));
        // .bat is never offered as a spawn target (not a process image).
        assert!(!conda
            .iter()
            .any(|p| p.extension().is_some_and(|e| e == "bat")));
        assert_eq!(
            WindowsPlatform.tool_candidates("py", home),
            vec![
                PathBuf::from("C:\\Windows\\py.exe"),
                PathBuf::from("C:\\Users\\t\\AppData\\Local\\Programs\\Python\\Launcher\\py.exe"),
                PathBuf::from("C:\\Program Files\\PyManager\\py.exe"),
            ]
        );
        assert!(WindowsPlatform
            .tool_candidates("conda", home)
            .iter()
            .any(|p| p.to_string_lossy() == "C:\\ProgramData\\Miniconda3\\Scripts\\conda.exe"));
        // Profile minors must not shadow each other: 3.11 stays first so a
        // box with several interpreters keeps the RTX/CPU default unless the
        // profile pin says otherwise (the installer provisions the exact pin).
        let python = WindowsPlatform.tool_candidates("python", home);
        let pos = |needle: &str| {
            python
                .iter()
                .position(|p| p.to_string_lossy().ends_with(needle))
                .unwrap_or_else(|| panic!("missing {needle}"))
        };
        assert!(pos("Python311\\python.exe") < pos("Python313\\python.exe"));
        assert!(python
            .iter()
            .any(|p| p.to_string_lossy() == "C:\\Program Files\\Python312\\python.exe"));
        assert!(WindowsPlatform.tool_candidates("nope", home).is_empty());
    }
}

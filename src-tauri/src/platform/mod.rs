#![allow(dead_code, unused_imports)]

//! Platform adapters.
//!
//! OS-specific behavior lives here. Shared modules should call `current()`
//! instead of using `cfg(target_os = "...")` directly.

pub mod linux;
pub mod macos;
pub mod unix;
pub mod windows;

use serde_json::Value;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    TaskManager,
    DesktopShortcut,
    Dlss,
    KernelWheels,
    TritonRepair,
    AutoStart,
    /// One-click prerequisite install (winget/choco/MSI download). Linux and
    /// macOS get the package-manager command instead of a button.
    PrereqInstall,
}

pub trait Platform {
    fn os_name(&self) -> &'static str;

    fn supports(&self, _capability: Capability) -> bool {
        false
    }

    fn detect_gpu(&self) -> Value {
        serde_json::json!({"vendor":"unknown","name":"","vramMB":"0","driverVersion":"","raw":"not implemented"})
    }

    fn detect_prereqs(&self) -> Value {
        serde_json::json!({"ok": false, "error": "not implemented"})
    }

    fn process_signature(&self, _pid: u32) -> Option<(String, String)> {
        None
    }

    fn port_owner(&self, _port: u64) -> Option<PortOwner> {
        None
    }

    fn kill_pid(&self, _pid: u32) {}

    fn open_path(&self, _path: &str) -> Result<(), String> {
        Err("not implemented".into())
    }

    fn create_desktop_shortcut(&self, _repo: &std::path::Path, _py: &std::path::Path) -> Value {
        serde_json::json!({"ok": false, "error": "not supported on this platform"})
    }

    fn open_task_manager(&self) -> Result<Value, String> {
        Err("not supported on this platform".into())
    }

    fn set_auto_start(&self, _enabled: bool, _exe: &str) -> Value {
        serde_json::json!({"ok": false, "success": false, "error": "not supported on this platform"})
    }

    /// Every TCP socket in LISTEN state owned by `ports`. Errors are returned
    /// instead of swallowed: a silently empty scan is how orphaned servers
    /// survived earlier versions of the stop path.
    fn port_listeners(&self, _ports: &[u64]) -> Result<Vec<Listener>, String> {
        Ok(Vec::new())
    }

    /// Every TCP socket in LISTEN state, any port, with its owner. Used by the
    /// stop sweep to catch a server that was started on a non-standard port.
    fn tcp_listeners(&self) -> Result<Vec<Listener>, String> {
        Ok(Vec::new())
    }

    /// Processes whose command line contains `marker`, as (pid, exe, cmdline).
    fn scan_processes(&self, _marker: &str) -> Result<Vec<ProcessInfo>, String> {
        Ok(Vec::new())
    }

    /// Directly spawnable image paths for a prerequisite tool, in the order
    /// they should be probed. Pure tables — no filesystem access.
    fn tool_candidates(&self, _tool: &str, _home: &str) -> Vec<PathBuf> {
        Vec::new()
    }

    /// Is `tool` runnable from PATH for this launcher's (possibly stale)
    /// environment? Windows additionally filters the Store stub.
    fn tool_on_path(&self, _tool: &str) -> bool {
        false
    }

    /// Absolute PATH hit for `tool`, for display and for spawning.
    fn lookup_on_path(&self, _tool: &str) -> Option<String> {
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listener {
    pub pid: u32,
    pub name: String,
    pub port: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub exe: String,
    pub cmd: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortOwner {
    pub pid: u32,
    pub name: String,
    pub cmd: String,
    pub ours: bool,
}

pub fn current() -> Box<dyn Platform> {
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsPlatform)
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxPlatform)
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacosPlatform)
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        Box::new(linux::LinuxPlatform)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_platform_has_a_name() {
        assert!(!current().os_name().is_empty());
    }

    #[test]
    fn capability_checks_are_boolean() {
        let p = current();
        let _ = p.supports(Capability::TaskManager);
        let _ = p.supports(Capability::DesktopShortcut);
    }

    /// Every adapter compiles on every host, so the capability maps can be
    /// checked here against what each impl actually does. Keep these in step
    /// with `src/w2gp.js`.
    #[test]
    fn adapter_capability_maps_match_their_impls() {
        let w = windows::WindowsPlatform;
        assert!(w.supports(Capability::TaskManager));
        assert!(w.supports(Capability::DesktopShortcut));
        assert!(w.supports(Capability::Dlss));
        assert!(w.supports(Capability::KernelWheels));
        assert!(w.supports(Capability::TritonRepair));
        assert!(w.supports(Capability::AutoStart));
        assert!(w.supports(Capability::PrereqInstall));

        let l = linux::LinuxPlatform;
        assert!(l.supports(Capability::KernelWheels));
        assert!(l.supports(Capability::DesktopShortcut));
        assert!(l.supports(Capability::AutoStart));
        assert!(!l.supports(Capability::TaskManager));
        assert!(!l.supports(Capability::Dlss));
        assert!(!l.supports(Capability::TritonRepair));
        assert!(!l.supports(Capability::PrereqInstall));

        let m = macos::MacosPlatform;
        assert!(m.supports(Capability::AutoStart));
        assert!(!m.supports(Capability::DesktopShortcut));
        assert!(!m.supports(Capability::KernelWheels));
        assert!(!m.supports(Capability::Dlss));
        assert!(!m.supports(Capability::TritonRepair));
        assert!(!m.supports(Capability::TaskManager));
        assert!(!m.supports(Capability::PrereqInstall));
    }

    /// macOS has no shortcut writer; it must answer with an explicit refusal
    /// instead of silently reporting success.
    #[test]
    fn macos_desktop_shortcut_refuses() {
        let out = macos::MacosPlatform
            .create_desktop_shortcut(std::path::Path::new("repo"), std::path::Path::new("py"));
        assert_eq!(out["ok"], serde_json::json!(false));
        assert!(out["error"].as_str().is_some_and(|e| !e.is_empty()));
    }
}

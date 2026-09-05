//! Shared state, path helpers, config I/O and tiny utils.

#![allow(clippy::too_many_lines, clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::cast_possible_wrap, clippy::cast_lossless, clippy::missing_panics_doc, clippy::needless_pass_by_value, clippy::items_after_statements, clippy::match_same_arms, clippy::similar_names, clippy::many_single_char_names, clippy::case_sensitive_file_extension_comparisons, clippy::unreadable_literal, clippy::redundant_closure, clippy::uninlined_format_args, clippy::redundant_clone)]
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
// shell/dialog/fs plugins wired for install/launch streaming — ponytail: std::process covers probes without them
pub(crate) static MUTATING: OnceLock<Mutex<Option<String>>> = OnceLock::new();
pub(crate) static LAST_STATUS: OnceLock<Mutex<Option<(std::time::Instant, Option<std::time::SystemTime>, serde_json::Value)>>> = OnceLock::new(); // status cache: 5s floor + site-packages mtime — pip installs invalidate instantly, dashboard paints from cache
pub(crate) static CACHED_DATA_DIR: OnceLock<Mutex<(PathBuf, std::time::Instant)>> = OnceLock::new();
pub(crate) static CACHED_REPO_DIR: OnceLock<Mutex<(PathBuf, std::time::Instant)>> = OnceLock::new();
pub(crate) static CACHED_IGPU: OnceLock<Mutex<Option<serde_json::Value>>> = OnceLock::new();
pub(crate) static METRICS_CACHE: OnceLock<Mutex<Option<(std::time::Instant, serde_json::Value)>>> = OnceLock::new();
pub(crate) static SYSINFO_CACHE: OnceLock<Mutex<sysinfo::System>> = OnceLock::new();
// Ring buffer of recent server/install log lines backs get_log_history (term window prefill).
// Spawn helper: CREATE_NO_WINDOW on Windows so probes (nvidia-smi, python,
// powershell, reg, where…) never flash a conhost window. The shell plugin
// already hides its own spawns; this covers every std::process call site.
// Keep a visible console ONLY for taskmgr (user asked to see it).
#[cfg(windows)]
pub(crate) fn silent_command(prog: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut c = std::process::Command::new(prog);
    c.creation_flags(0x08000000);
    c
}
#[cfg(not(windows))]
pub(crate) fn silent_command(prog: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    std::process::Command::new(prog)
}
// Console tools (npm, opencode, codex) ship as .cmd shims on Windows, which
// CreateProcess can't launch directly — route through cmd /C like a terminal.
// (ponytail: bare Command::new("npm") fails with "program not found".)
#[cfg(windows)]
pub(crate) fn term_tool(tool: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut c = std::process::Command::new("cmd");
    c.args(["/C", tool]);
    c.creation_flags(0x08000000);
    c
}
#[cfg(not(windows))]
pub(crate) fn term_tool(tool: &str) -> std::process::Command {
    std::process::Command::new(tool)
}
pub(crate) static LOG_HISTORY: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
pub(crate) fn push_log(text: &str, source: &str) {
    if text.is_empty() { return; }
    let m = LOG_HISTORY.get_or_init(|| Mutex::new(Vec::new()));
    if let Ok(mut g) = m.lock() {
        for line in text.replace("\r\n", "\n").replace('\r', "\n").split('\n') {
            let t = line.trim();
            if t.is_empty() { continue; }
            // launch stream doubles as the notifier event source
            // (scan before the tqdm skip — progress lives in those fragments)
            if source == "launch" { crate::features::notifier_scan_line(t); }
            // skip tqdm progress spam (\r-rewritten fragments) — history is for real log lines
            if t.contains("it/s") || t.contains("s/it") { continue; }
            g.push(line.to_string());
        }
        let excess = g.len().saturating_sub(2000);
        if excess > 0 { g.drain(..excess); }
    }
}
pub(crate) fn invalidate_path_cache() {
    if let Some(m) = CACHED_DATA_DIR.get() { let _ = m.lock().map(|mut g| g.1 = std::time::Instant::now().checked_sub(std::time::Duration::from_hours(1)).unwrap()); }
    if let Some(m) = CACHED_REPO_DIR.get() { let _ = m.lock().map(|mut g| g.1 = std::time::Instant::now().checked_sub(std::time::Duration::from_hours(1)).unwrap()); }
}
pub(crate) fn mutating_try(name: &str) -> Result<(), String> {
    let m = MUTATING.get_or_init(|| Mutex::new(None));
    let mut g = m.lock().unwrap();
    if let Some(cur) = g.as_ref() { return Err(format!("Another operation already running ({cur}). Wait for it to finish.")); }
    *g = Some(name.to_string()); Ok(())
}
pub(crate) fn mutating_done() { if let Some(m) = MUTATING.get() { *m.lock().unwrap() = None; } }

// ── helpers: paths (mirrors main.js getDataDir/getRepoDir) ──
pub(crate) fn home_dir() -> PathBuf {
    if let Ok(h) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        PathBuf::from(h)
    } else {
        PathBuf::from(".")
    }
}
pub(crate) fn appdata_dir() -> PathBuf {
    if let Ok(a) = std::env::var("APPDATA") { PathBuf::from(a) }
    else if let Ok(h) = std::env::var("HOME") { PathBuf::from(h).join(".config") }
    else { PathBuf::from(".") }
}
#[allow(dead_code)]
pub(crate) fn local_appdata_dir() -> PathBuf {
    if let Ok(l) = std::env::var("LOCALAPPDATA") { PathBuf::from(l) }
    else { appdata_dir() }
}
pub(crate) fn data_dir_override_file() -> PathBuf { home_dir().join(".wan2gp-tauri-data-dir") }
#[allow(dead_code)]
pub(crate) fn data_dir_override_file_electron() -> PathBuf { home_dir().join(".wan2gp-desktop-data-dir") }

pub(crate) fn get_data_dir_uncached() -> PathBuf {
    let ov = data_dir_override_file();
    let ov_e = data_dir_override_file_electron();
    for pth in [ov.clone(), ov_e.clone()] {
        if pth.exists() {
            if let Ok(s) = std::fs::read_to_string(&pth) {
                let d = s.trim().to_string();
                if !d.is_empty() {
                    let p = PathBuf::from(&d);
                    if p.is_absolute() && p.exists() { return p; }
                    if !p.exists() {
                        // Fresh pick (e.g. D:\Wan2GP auto-resolved from D:\) doesn't
                        // exist yet — honor it while its parent drive is alive
                        // (install creates it). Fall back only when the parent is
                        // gone too (disconnected drive / changed letter).
                        if p.parent().is_some_and(|par| par.exists()) { return p; }
                        let legacy = std::path::Path::new(&d).join("wgp.py");
                        let nested = PathBuf::from(&d).join("Wan2GP").join("wgp.py");
                        if legacy.exists() || nested.exists() { return PathBuf::from(d); }
                    }
                }
            }
        }
    }
    default_data_dir()
}
pub(crate) fn get_data_dir() -> PathBuf {
    let cache = CACHED_DATA_DIR.get_or_init(|| Mutex::new((PathBuf::new(), std::time::Instant::now().checked_sub(std::time::Duration::from_hours(1)).unwrap())));
    if let Ok(g) = cache.lock() {
        if g.1.elapsed() < std::time::Duration::from_secs(5) && !g.0.as_os_str().is_empty() { return g.0.clone(); }
    }
    let v = get_data_dir_uncached();
    if let Ok(mut g) = cache.lock() { *g = (v.clone(), std::time::Instant::now()); }
    v
}
pub(crate) fn default_data_dir() -> PathBuf {
    // Check existing installs on drives CDEFG (Windows)
    #[cfg(windows)]
    {
        for d in ["C:\\Wan2GP", "D:\\Wan2GP", "E:\\Wan2GP", "F:\\Wan2GP", "G:\\Wan2GP"] {
            let p = PathBuf::from(d);
            if p.join("wgp.py").exists() || p.join("Wan2GP").join("wgp.py").exists() { return p; }
        }
        let legacy = appdata_dir().join("wan2gp-desktop").join("Wan2GP");
        if legacy.join("wgp.py").exists() || legacy.join("Wan2GP").join("wgp.py").exists() { return legacy; }
        // Prefer root of current drive if writable
        let root = PathBuf::from(std::env::var("SystemDrive").unwrap_or("C:".into()) + "\\Wan2GP");
        if dir_is_writable(&root) || dir_is_writable(root.parent().unwrap_or(Path::new("C:\\"))) { return root; }
        // install-dir adjacent
        if let Ok(exe) = std::env::current_exe() {
            if let Some(base) = exe.parent() {
                let cand = base.join("Wan2GP");
                if dir_is_writable(base) { return cand; }
            }
        }
        legacy
    }
    #[cfg(not(windows))]
    {
        return home_dir().join("Wan2GP");
    }
}
pub(crate) fn dir_is_writable(p: &Path) -> bool {
    // try mkdir + probe file (same as main.js dirIsWritable)
    let target = p.to_path_buf();
    if std::fs::create_dir_all(&target).is_err() { return false; }
    let probe = target.join(format!(".writetest-{}", std::process::id()));
    match std::fs::write(&probe, b"1") { Ok(()) => { let _ = std::fs::remove_file(&probe); true }, Err(_) => false }
}
pub(crate) fn get_repo_dir_uncached() -> PathBuf {
    let base = get_data_dir();
    let nested = base.join("Wan2GP");
    if nested.join("wgp.py").exists() { return nested; }
    base
}
pub(crate) fn get_repo_dir() -> PathBuf {
    let cache = CACHED_REPO_DIR.get_or_init(|| Mutex::new((PathBuf::new(), std::time::Instant::now().checked_sub(std::time::Duration::from_hours(1)).unwrap())));
    if let Ok(g) = cache.lock() {
        if g.1.elapsed() < std::time::Duration::from_secs(5) && !g.0.as_os_str().is_empty() { return g.0.clone(); }
    }
    let v = get_repo_dir_uncached();
    if let Ok(mut g) = cache.lock() { *g = (v.clone(), std::time::Instant::now()); }
    v
}
pub(crate) fn get_config_file() -> PathBuf { get_data_dir().join("desktop-config.json") }
pub(crate) fn get_envs_file() -> PathBuf { get_repo_dir().join("envs.json") }
// One file-as-folder guard (pasted Temp pngs chosen as folders) shared by all scrubbers.
pub(crate) fn looks_like_file_path(s: &str) -> bool {
    let low = s.to_lowercase();
    low.contains("orca-paste")
        || [".png", ".jpg", ".jpeg", ".webp", ".bmp", ".gif"].iter().any(|e| low.ends_with(e))
        || (low.contains("\\temp\\") && low.contains(".png"))
}

pub(crate) fn load_config_value() -> serde_json::Value {
    // ponytail: clean wgp_config.json file-as-folder on every launch (before desktop-config return)
    {
        let wp = get_repo_dir().join("wgp_config.json");
        if wp.exists() {
            if let Ok(s)=std::fs::read_to_string(&wp) {
                if let Ok(mut v)=serde_json::from_str::<serde_json::Value>(&s) {
                    let mut d=false;
                    for k in ["checkpoints_paths","checkpointsPaths","ckpt_dir","loras_root","lorasRoot","lora_dir","save_path","savePath"] {
                        if let Some(p)=v.get(k).and_then(|x| if x.is_array(){x.as_array().and_then(|a|a.first())}else{Some(x)}).and_then(|x| x.as_str()) {
                            if looks_like_file_path(p) {
                                if let Some(m)=v.as_object_mut(){ m.remove(k); d=true; }
                            }
                        }
                    }
                    if d { let _= atomic_write(&wp, &serde_json::to_string_pretty(&v).unwrap_or(s)); }
                }
            }
        }
    }
    let f = get_config_file();
    if f.exists() {
        if let Ok(s) = std::fs::read_to_string(&f) {
            if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&s) {
                let mut dirty=false;
                for k in ["modelCkptsPath","modelLorasPath","modelOutputPath","modelCkpts","modelLoras"] {
                    if let Some(p)=v.get(k).and_then(|x| x.as_str()) {
                        if looks_like_file_path(p) {
                            v.as_object_mut().map(|m| m.remove(k));
                            dirty=true;
                        }
                    }
                }
                if dirty { let _= atomic_write(&f, &serde_json::to_string_pretty(&v).unwrap_or(s)); }
                return v;
            }
        }
    }
    serde_json::json!({
        "githubToken": "", "hfToken": "", "claudeApiKey": "", "theme": "dark",
        "serverPort": 7861, "serverName": "localhost", "defaultBrowser": "system", // ponytail: 7861 for side-by-side with Electron 7860

        "termDockDefault": "bottom", "electronGpu": true, "launcherGpu": "auto", "sageSafe": true, "share": false,
        "autoUpdateEnabled": true, "ggufEnv": { "enabled": true, "matmulMode": "auto", "streamK": true, "bf16Fp16": false }
    })
}
pub(crate) fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() { std::fs::create_dir_all(dir)?; }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub(crate) fn reg_val(line: &str) -> Option<(String, String)> {
    let t = line.trim();
    if t.is_empty() || t.starts_with("HKEY_") { return None; }
    for typ in ["REG_SZ", "REG_EXPAND_SZ"] {
        if let Some(i) = t.find(typ) {
            return Some((t[..i].trim().to_string(), t[i + typ.len()..].trim().to_string()));
        }
    }
    None
}
pub(crate) fn split_cmdline(s: &str) -> (String, Vec<String>) {
    let t = s.trim();
    if t.starts_with('"') {
        if let Some(end) = t[1..].find('"') {
            let exe = t[1..1 + end].to_string();
            let args = t[1 + end + 1..].split_whitespace().map(|x| x.to_string()).collect();
            return (exe, args);
        }
    }
    let mut it = t.split_whitespace();
    (it.next().unwrap_or("").to_string(), it.map(|x| x.to_string()).collect())
}

pub(crate) static WANGP_PID: OnceLock<Mutex<Option<u32>>> = OnceLock::new();

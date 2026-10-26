//! Wan2GP server lifecycle (launch/stop/browser modes).
use crate::base::*;
use crate::{
    hw::{
        get_gpu_info_sync, kernel_profile_key, nv_cards, resolve_gpu_pick, version_gt,
        wmi_all_gpus, wmi_gpu_fallback, wmi_virtual_adapters, GpuPin,
    },
    status::{get_active_env, resolve_env_python},
};
use std::path::{Path, PathBuf};
use tauri::Emitter;

// Quote-aware split for Extra Launch Args (keeps "--config \"a b\"" together).
pub(crate) fn split_launch_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    let mut started = false;
    for ch in s.chars() {
        match ch {
            '"' => {
                in_q = !in_q;
                started = true;
            }
            c if c.is_whitespace() && !in_q => {
                if started {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            c => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(cur);
    }
    out
}

/// Flags current WanGP no longer defines. `wgp.py` parses with argparse, so a
/// stale token makes it exit with "unrecognized arguments" before the server
/// binds — and the launch looks like a hang. They are dropped on the way in
/// and reported, so upgrades from older presets keep working.
const REMOVED_LAUNCH_FLAGS: &[&str] = &["--teacache"];

/// Split `tokens` into (kept, removed) for [`REMOVED_LAUNCH_FLAGS`]. A removed
/// flag takes one following non-flag token with it (`--teacache 2.0`), so the
/// value can't remain as a stray positional. `--flag=value` is removed whole.
/// Pure + unit-tested.
pub(crate) fn strip_removed_launch_flags(tokens: Vec<String>) -> (Vec<String>, Vec<String>) {
    let mut kept = Vec::with_capacity(tokens.len());
    let mut removed = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let t = &tokens[i];
        let flag = t.split('=').next().unwrap_or(t);
        if REMOVED_LAUNCH_FLAGS.contains(&flag) {
            removed.push(t.clone());
            if !t.contains('=') {
                if let Some(next) = tokens.get(i + 1) {
                    if !next.starts_with('-') {
                        removed.push(next.clone());
                        i += 1;
                    }
                }
            }
            i += 1;
            continue;
        }
        kept.push(t.clone());
        i += 1;
    }
    (kept, removed)
}

/// Drop removed flags from the persisted Extra Launch Args (best effort).
/// The read-path filter below already protects the launch; this keeps the
/// Manage field honest so the user can see and edit what actually runs.
pub(crate) fn scrub_removed_launch_flags() {
    let dc = load_config_value();
    let raw = dc
        .get("launchArgs")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if raw.trim().is_empty() {
        return;
    }
    let (kept, removed) = strip_removed_launch_flags(split_launch_args(&raw));
    if removed.is_empty() {
        return;
    }
    let mut next = dc.clone();
    next["launchArgs"] = serde_json::json!(kept.join(" "));
    if let Ok(s) = serde_json::to_string_pretty(&next) {
        let _ = atomic_write(&get_config_file(), &s);
    }
    push_log(
        &format!(
            "[!] Removed launch flag(s) this WanGP version no longer accepts: {}\n",
            removed.join(" ")
        ),
        "launch",
    );
}
/// Shared wgp.py base args (Slice `deepy-web`: common builder for the main
/// server and the standalone Deepy Web process — no fork, verbatim upstream
/// flags only). `--config` is intentionally omitted: children spawn with
/// `current_dir = repo`, so upstream resolves its config folder default.
/// `--deepy-sessions-dir` is passed explicitly (verified in
/// `shared/cli_args.py`: `default: ./deepy_sessions`).
pub(crate) struct WgpLaunchBase {
    pub port: u64,
    pub server_name: String,
    pub sessions_dir: String,
    /// Troubleshooting → Verbose logging: appends `--verbose 2` so the
    /// launcher's console captures upstream's detail instead of guessing at
    /// a problem from the summary lines.
    pub verbose: bool,
}
pub(crate) fn build_wgp_args(base: &WgpLaunchBase) -> Vec<String> {
    let mut args = vec![
        "wgp.py".to_string(),
        "--server-port".to_string(),
        base.port.to_string(),
        "--server-name".to_string(),
        base.server_name.clone(),
        "--deepy-sessions-dir".to_string(),
        base.sessions_dir.clone(),
    ];
    if base.verbose {
        args.push("--verbose".to_string());
        args.push("2".to_string());
    }
    args
}
static TERMINAL_TITLE: std::sync::LazyLock<std::sync::Mutex<Option<String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(None));
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn terminal_title() -> Option<String> {
    TERMINAL_TITLE.lock().ok().and_then(|g| g.clone())
}
// AMD ROCm PATH prepend tracking (retired): older launcher versions
// prepended the ROCm SDK bin dirs to PATH in this process. Nothing prepends
// anymore (upstream needs no launch-time SDK env), but the recording stays
// so the non-AMD reconcile below can still strip prepends left by those
// versions. HSA override handling is untouched.
static AMD_PATH_PREPEND: std::sync::LazyLock<std::sync::Mutex<Option<String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(None));
/// Keys this launcher sets for the AMD ROCm session env. Meaningless off-AMD:
/// on any non-AMD launch they are removed unconditionally (never set-if-absent
/// there), each removal logged at `[i]` level. HSA override keys are NOT here —
/// HSA handling stays exactly as-is. Pure data + unit-tested.
pub(crate) const NON_AMD_STALE_ENV_KEYS: &[&str] = &[
    "ROCM_HOME",
    "CC",
    "CXX",
    "DISTUTILS_USE_SDK",
    "FLASH_ATTENTION_TRITON_AMD_ENABLE",
    "TORCH_ROCM_AOTRITON_ENABLE_EXPERIMENTAL",
    "MIOPEN_FIND_MODE",
    "HIP_VISIBLE_DEVICES",
];
/// Strip our own recorded PATH prepend: remove the exact `prepended` prefix
/// (`add` in `format!("{add};{old}")`) when present, case-insensitively.
/// Anything else (user PATH, foreign entries) passes through untouched.
/// Pure + unit-tested.
pub(crate) fn strip_own_path_prepend(current: &str, prepended: &str) -> String {
    if prepended.is_empty() || current.is_empty() {
        return current.to_string();
    }
    let mut rest = current;
    loop {
        if rest.eq_ignore_ascii_case(prepended) {
            return String::new();
        }
        if rest.len() > prepended.len()
            && rest[..prepended.len()].eq_ignore_ascii_case(prepended)
            && rest[prepended.len()..].starts_with(';')
        {
            rest = &rest[prepended.len() + 1..];
        } else {
            break;
        }
    }
    rest.to_string()
}
/// Reconcile stale AMD session env on a non-AMD launch: remove every
/// NON_AMD_STALE_ENV_KEYS var unconditionally (they are meaningless off-AMD;
/// a ROCm-prepended PATH and `CC=clang-cl` break later Intel/CPU/NVIDIA pip
/// builds in this same launcher process) and strip our recorded PATH prepend.
/// Each removal logged at `[i]` level. HSA override handling is NOT touched.
/// The Intel path gains zero new env behavior beyond this reconcile.
pub(crate) fn reconcile_non_amd_session_env(mut emit: impl FnMut(&str)) {
    for k in NON_AMD_STALE_ENV_KEYS {
        if std::env::var(k).is_ok() {
            std::env::remove_var(k);
            emit(&format!(
                "[i] removed stale AMD session env {k} (non-AMD launch)\n"
            ));
        }
    }
    let recorded = AMD_PATH_PREPEND
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_default();
    if !recorded.is_empty() {
        let old = std::env::var("PATH").unwrap_or_default();
        let stripped = strip_own_path_prepend(&old, &recorded);
        if stripped != old {
            std::env::set_var("PATH", &stripped);
            emit(&format!(
                "[i] removed stale AMD ROCm PATH prepend {recorded}\n"
            ));
        }
    }
}
/// First plausible absolute-dir line of `rocm-sdk path --root` stdout.
/// None for empty/relative output or dirs that don't exist (never invent
/// paths — the caller falls back to dir-guessing). Pure + unit-tested.
pub(crate) fn parse_rocm_sdk_root(out: &str) -> Option<PathBuf> {
    out.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .find(|l| {
            let p = Path::new(l);
            p.is_absolute() || (l.len() > 3 && l.as_bytes()[1] == b':')
        })
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}
/// Virtual-GPU HIP pin decision (issue #15 follow-up): exactly one
/// discrete AMD GPU next to an iGPU (APU) or ignored virtual display
/// adapter(s) → pin HIP to the discrete card's index so HIP can't land on
/// the iGPU/virtual shim. APUs enumerate first in HIP order, so the
/// discrete card sits at index = apu count. Multi-discrete boxes (the right
/// index would be a guess) and single-card boxes (unambiguous) do nothing.
/// Pure + unit-tested.
pub(crate) fn hip_pin_value(
    discrete_amd: usize,
    apus: usize,
    virtual_adapters: usize,
) -> Option<String> {
    if discrete_amd != 1 {
        return None;
    }
    if apus > 0 {
        // Discrete card sits after the APU(s) in HIP order.
        return Some(apus.to_string());
    }
    if virtual_adapters > 0 {
        // Virtual shims are not HIP devices, so the discrete card is 0.
        return Some("0".to_string());
    }
    None
}
/// MIOpen mode for AMD launch from the Manage-backed `amdEnv` config key
/// (`amdEnv.miopenDisabled`, default false — bound in Manage → AMD ROCm).
/// Some(_) → set-if-absent as today;
/// None → leave MIOPEN_FIND_MODE fully unset. Pure core + unit-tested.
pub(crate) fn miopen_find_mode_value(miopen_disabled: bool) -> Option<&'static str> {
    if miopen_disabled {
        None
    } else {
        Some("FAST")
    }
}
/// Config read behind the MIOpen toggle (missing key → false → FAST).
fn miopen_find_mode() -> Option<&'static str> {
    let disabled = load_config_value()
        .get("amdEnv")
        .and_then(|a| a.get("miopenDisabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    miopen_find_mode_value(disabled)
}
// Server command for the external-terminal .bat: `"<py>" -u <args…>`.
// args[0] is the bootstrap shim (`py <boot> wgp.py …`) — never hardcode a
// script name here: duplicating `wgp.py` makes argparse die with
// "unrecognized arguments: <boot> wgp.py" (all terminal modes).
fn terminal_server_command(py: &str, args: &[String]) -> String {
    let joined = args
        .iter()
        .map(|a| {
            if a.contains(' ') {
                format!("\"{}\"", a.replace('%', "%%"))
            } else {
                a.replace('%', "%%")
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!("\"{py}\" -u {joined}")
}
// External-terminal mode (run.bat style): generate a script that runs wgp.py
// with the same args/env, open it in a VISIBLE console window, wait for the
// server, open the browser. Not a streamed child — the user owns the window.
// Nine wires (same env/args as hidden launch, plus the no-GPU open mode) —
// kept flat to mirror the launch() call-site locals; allowed by policy.
#[allow(clippy::too_many_arguments)]
#[cfg_attr(not(windows), allow(unused_variables))]
fn launch_in_terminal(
    app: tauri::AppHandle,
    repo: &Path,
    py: &str,
    args: &[String],
    port: u64,
    _cfg: &serde_json::Value,
    hf_token: String,
    claude_key: String,
    no_gpu: bool,
) -> Result<serde_json::Value, String> {
    let emit = |msg: &str| {
        crate::base::push_log(msg, "launch");
        let _ = app.emit("launch-log", msg.to_string());
    };
    let title = format!(
        "Wan2GP-Launcher-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    if let Ok(mut g) = TERMINAL_TITLE.lock() {
        *g = Some(title.clone());
    }
    // Non-secret env lines for the script (GGUF knobs, same as hidden launch).
    let mut env_lines: Vec<String> = vec![
        "set PYTHONIOENCODING=utf-8".into(),
        "set PYTHONUTF8=1".into(),
        "set PYTHONUNBUFFERED=1".into(),
        "set TQDM_MININTERVAL=0".into(),
        "set TQDM_MINITERS=1".into(),
        "set NO_PROXY=localhost,127.0.0.1,::1".into(),
    ];
    // Tokens are NOT written into the .bat: a crash left the file (and the
    // secrets) in %TEMP% indefinitely. They go into a separate file that the
    // script reads and deletes before starting the server; every path below
    // removes it on failure and stop/startup sweeps anything left behind.
    let mut secret_env: Vec<String> = Vec::new();
    if !hf_token.is_empty() {
        secret_env.push(format!("HF_TOKEN={}", hf_token.replace(['\r', '\n'], "")));
    }
    if !claude_key.is_empty() {
        secret_env.push(format!(
            "ANTHROPIC_API_KEY={}",
            claude_key.replace(['\r', '\n'], "")
        ));
    }
    for k in [
        "WGP_GGUF_LLAMACPP_CUDA",
        "WGP_GGUF_LLAMACPP_CUDA_MATMUL_MODE",
        "LLAMACPP_GGUF_SHORT_BATCH",
    ] {
        if let Ok(val) = std::env::var(k) {
            env_lines.push(format!("set {k}={val}"));
        }
    }
    // Quoted server command (`"<py>" -u <boot> wgp.py …`); the template
    // wraps it in `cmd /c "…"`.
    let server_cmd = terminal_server_command(py, args);
    #[cfg(windows)]
    {
        // Unique per launch: the old fixed wan2gp-terminal.bat raced when
        // two launches overlapped (second overwrote the first's script).
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let script = std::env::temp_dir().join(format!("wan2gp-terminal-{millis}.bat"));
        let env_script = std::env::temp_dir().join(format!("wan2gp-env-{millis}.txt"));
        let env_preamble = if secret_env.is_empty() {
            String::new()
        } else {
            if let Err(e) = std::fs::write(&env_script, secret_env.join("\r\n") + "\r\n") {
                mutating_done();
                return Err(format!("Could not stage terminal environment: {e}"));
            }
            format!(
                "for /f \"usebackq delims=\" %%A in (\"{env}\") do set \"%%A\"\r\ndel \"{env}\" >nul 2>&1\r\n",
                env = env_script.display()
            )
        };
        let url = format!("http://localhost:{port}");
        // No-GPU variant: open the user's selected default browser with GPU
        // acceleration disabled so the browser frees VRAM for generation.
        // Falls back through Chrome → any Chromium → Firefox (plain, noted)
        // → OS default. The console window itself is unaffected.
        let open_line = if no_gpu {
            match resolve_no_gpu_browser() {
                Some((exe, id)) if is_chromium_browser(&id) => format!(
                    "start \"\" \"{exe}\" {flags} {url}",
                    flags = NO_GPU_CHROME_FLAGS.join(" ")
                ),
                Some((exe, _)) => {
                    emit("[!] Default browser is Firefox — it has no command-line GPU switch, opening it normally (no VRAM savings).\n");
                    format!("start \"\" \"{exe}\" {url}")
                }
                None => {
                    emit("[!] No supported browser found — external terminal will open the default browser instead (GPU on).\n");
                    format!("start {url}")
                }
            }
        } else {
            format!("start {url}")
        };
        let full = format!("@echo off\r\ntitle {title}\r\ncd /d \"{repo}\"\r\n{env_preamble}{envs}\r\necho [Wan2GP Desktop Launcher] Starting on port {port}...\r\necho [Wan2GP] Running: {server_cmd}\r\nstart /b \"\" cmd /c \"{server_cmd}\" 2>&1\r\necho Waiting for server on port {port}...\r\nset RC=0\r\n:waitloop\r\nrem Start-Sleep via PowerShell, not timeout /t: a GNU timeout from Git/MSYS on PATH mangles the flags. PowerShell is already required below.\r\npowershell -NoProfile -Command \"Start-Sleep -Seconds 2\" >nul 2>&1\r\nset /a RC+=1\r\nif %RC% gtr 60 (echo Server failed to start. Check console. ^& pause ^& exit /b 1)\r\npowershell -Command \"try{{$(Invoke-WebRequest -Uri http://127.0.0.1:{port}/config -TimeoutSec 2 -UseBasicParsing).StatusCode -eq 200;exit 0}}catch{{exit 1}}\" >nul 2>&1 && goto ready\r\ngoto waitloop\r\n:ready\r\necho Wan2GP is ready! Opening browser...\r\n{open_line}\r\necho [Wan2GP] Server running. Close this window to stop it.\r\npause >nul\r\n",
                repo = repo.display(), envs = env_lines.join("\r\n"));
        if let Err(e) = std::fs::write(&script, full) {
            let _ = std::fs::remove_file(&env_script);
            mutating_done();
            return Err(e.to_string());
        };
        emit("[*] Starting Wan2GP in external terminal…\n");
        // visible window: wt.exe preferred, else cmd /K (NOT silent — user must see it)
        let has_wt = std::process::Command::new("where")
            .arg("wt.exe")
            .output()
            .is_ok_and(|o| o.status.success());
        let spawned = if has_wt {
            std::process::Command::new("wt.exe")
                .args([
                    "-w",
                    "-1",
                    "new-tab",
                    "--title",
                    &title,
                    "cmd.exe",
                    "/K",
                    script.to_string_lossy().as_ref(),
                ])
                .spawn()
        } else {
            // `start` only treats the first arg as a window title when it is
            // quoted — an unquoted title is executed as a program ("Windows
            // cannot find 'Wan2GP-Launcher-…'"). Pass an empty quoted title;
            // the .bat already runs `title {title}` and wt.exe keeps --title.
            std::process::Command::new("cmd.exe")
                .args([
                    "/C",
                    "start",
                    "",
                    "cmd",
                    "/K",
                    script.to_string_lossy().as_ref(),
                ])
                .spawn()
        };
        if let Err(e) = spawned {
            let _ = std::fs::remove_file(&script);
            let _ = std::fs::remove_file(&env_script);
            mutating_done();
            return Err(format!("Could not open terminal: {e}"));
        }
        mutating_done();
        Ok(
            serde_json::json!({"ok": true, "port": port, "mode": if no_gpu { "terminal-nogpu" } else { "terminal" }, "url": url, "fresh": true}),
        )
    }
    #[cfg(not(windows))]
    {
        let _ = no_gpu;
        mutating_done();
        Err("External terminal mode is Windows-only in this build".into())
    }
}
#[tauri::command]
pub async fn launch(
    app: tauri::AppHandle,
    mode: Option<String>,
) -> Result<serde_json::Value, String> {
    let mode = mode.unwrap_or("browser".into());
    let repo = get_repo_dir();
    if !repo.join("wgp.py").exists() {
        return Err("Wan2GP not installed — run Install first".into());
    }
    let cfg = load_config_value();
    let port = cfg
        .get("serverPort")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(7860);
    // ponytail: if server already listening on :port (desktop→browser switch), reuse it — don't spawn second python on same port (Gradio OSError)
    if std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
        let url = format!("http://localhost:{port}");
        let m = format!("[*] Wan2GP already running on :{port} — opening {url}\n");
        crate::base::push_log(&m, "launch");
        let _ = app.emit("launch-log", m);
        return Ok(
            serde_json::json!({"ok": true, "port": port, "mode": mode, "url": url, "fresh": false}),
        );
    }
    mutating_try("launch")?;
    let share = cfg
        .get("share")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let gpu_device = cfg
        .get("gpuDevice")
        .and_then(|v| v.as_str())
        .unwrap_or("auto")
        .trim()
        .to_string();
    let launcher_gpu = cfg
        .get("launcherGpu")
        .and_then(|v| v.as_str())
        .unwrap_or("auto")
        .to_string();
    // ── GPU pin ──────────────────────────────────────────────────────────
    // A saved pick becomes `--gpu cuda:<nvidia-smi index>` — the same switch a
    // manual device choice inside WanGP produces. With the flag set, upstream
    // points CUDA_VISIBLE_DEVICES at that card AND calls
    // torch.set_default_device(), which is the path the launcher was missing;
    // without it WanGP lets torch decide, and on a multi-GPU box its device-0
    // code paths and its device_count() loops can disagree (a bare CUDA OOM
    // while VRAM reads free). "auto" is a real decision (the biggest card),
    // not "let WanGP decide".
    let nv_cards_now = nv_cards();
    let extra_gpu_arg = cfg
        .get("launchArgs")
        .and_then(|v| v.as_str())
        .map(|raw| {
            split_launch_args(raw)
                .iter()
                .any(|a| a == "--gpu" || a.starts_with("--gpu="))
        })
        .unwrap_or(false);
    let pin: GpuPin = if extra_gpu_arg {
        GpuPin {
            arg: None,
            card: None,
            note: Some(
                "Extra Launch Args contains --gpu — that flag wins and the GPU Device picker is paused for this launch"
                    .to_string(),
            ),
        }
    } else {
        resolve_gpu_pick(&gpu_device, &nv_cards_now)
    };
    // build args — gpuDevice -> explicit pin (mirrors Electron buildCommonLaunchArgs)
    let server_name = cfg
        .get("serverName")
        .and_then(|v| v.as_str())
        .unwrap_or("localhost")
        .to_string();
    let mut args = vec![
        "wgp.py".to_string(),
        "--server-port".into(),
        port.to_string(),
        "--server-name".into(),
        server_name.clone(),
        "--advanced".into(),
        "--multiple-images".into(),
    ];
    if cfg
        .get("verboseLogging")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        args.push("--verbose".into());
        args.push("2".into());
    }
    if share {
        args.push("--share".into());
    }
    // Pin exactly like a manual device pick: `--gpu cuda:<nvidia-smi index>`.
    if let Some(arg) = pin.arg.as_deref() {
        if !args.iter().any(|a| a == "--gpu") {
            args.push("--gpu".into());
            args.push(arg.to_string());
        }
    }
    // No nvidia-smi: fall back to a saved legacy ordinal, if any.
    if !extra_gpu_arg
        && nv_cards_now.is_empty()
        && gpu_device != "auto"
        && gpu_device.starts_with("cuda:")
        && !args.contains(&"--gpu".to_string())
    {
        args.push("--gpu".into());
        args.push(gpu_device.clone());
    }
    // Extra Launch Args from Manage tab (quote-aware split, appended last so they win).
    if let Some(extra) = cfg.get("launchArgs").and_then(|v| v.as_str()) {
        let (add, removed) = strip_removed_launch_flags(split_launch_args(extra));
        if !removed.is_empty() {
            crate::base::push_log(
                &format!(
                    "[!] Ignored launch flag(s) this WanGP version no longer accepts: {}\n",
                    removed.join(" ")
                ),
                "launch",
            );
            scrub_removed_launch_flags();
        }
        if !add.is_empty() {
            args.extend(add);
        }
    }
    // Phone-access toggle (dashboard Launch card): upstream `--listen` verbatim
    // ("Server accessible on local network" — Gradio binds off-localhost).
    // Deduped: a manual Extra-args entry already covers it.
    let main_lan = cfg
        .get("mainLan")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if main_lan && !args.iter().any(|a| a == "--listen") {
        args.push("--listen".into());
    }
    let emit = |msg: &str| {
        crate::base::push_log(msg, "launch");
        let _ = app.emit("launch-log", msg.to_string());
    };
    emit(&format!("[*] Launching Wan2GP ({mode}) on :{port}…\n"));
    // Prepare the child env. `--gpu` is upstream's own switch: it writes
    // CUDA_VISIBLE_DEVICES itself (shared/default_device.py) and, because the
    // flag is non-empty, wgp.py calls torch.set_default_device() after model
    // load. A CUDA_VISIBLE_DEVICES inherited from the launcher's own env would
    // renumber the ordinals under that flag, so clear it and let the flag own
    // the choice.
    {
        if std::env::var("CUDA_VISIBLE_DEVICES").is_ok() {
            std::env::remove_var("CUDA_VISIBLE_DEVICES");
            emit("[i] Cleared an inherited CUDA_VISIBLE_DEVICES so --gpu ordinals match nvidia-smi\n");
        }
        if !nv_cards_now.is_empty() {
            // Without this, CUDA orders devices FASTEST_FIRST and cuda:N is not
            // the card nvidia-smi (and the picker) calls N.
            std::env::set_var("CUDA_DEVICE_ORDER", "PCI_BUS_ID");
        }
        if let Some(note) = pin.note.as_deref() {
            emit(&format!("[!] GPU: {note}\n"));
        }
        // Diagnostics: one line that turns the next OOM report into a
        // five-second answer and proves the pin actually took effect.
        let dev_list = if nv_cards_now.is_empty() {
            "(no nvidia-smi)".to_string()
        } else {
            nv_cards_now
                .iter()
                .map(|c| {
                    format!(
                        "#{} {} ({} MiB total, {} free)",
                        c.index, c.name, c.vram_mb, c.free_mb
                    )
                })
                .collect::<Vec<_>>()
                .join(" | ")
        };
        emit(&format!(
            "[i] GPU diagnostics — CUDA_VISIBLE_DEVICES={} CUDA_DEVICE_ORDER={} resolved_uuid={}{} | devices: {}\n",
            std::env::var("CUDA_VISIBLE_DEVICES").unwrap_or("(unset)".into()),
            std::env::var("CUDA_DEVICE_ORDER").unwrap_or("(unset)".into()),
            pin.card.as_ref().map(|c| c.uuid.as_str()).unwrap_or("(none)"),
            pin.arg.as_deref().map(|a| format!(" arg={a}")).unwrap_or_default(),
            dev_list,
        ));
    }
    // GPU assignment log (mirrors Electron 9945990)
    {
        let hw = get_gpu_info_sync();
        let hw_name = hw.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let hw_vendor = hw.get("vendor").and_then(|v| v.as_str()).unwrap_or("?");
        let hw_vram = hw.get("vramMB").and_then(|v| v.as_str()).unwrap_or("0");
        let gpu_count = if nv_cards_now.is_empty() {
            // AMD/Intel-only box: nvidia-smi absent — WMI name instead of "?".
            wmi_gpu_fallback()
                .map(|(_, v, _, _)| format!("1 {v}"))
                .unwrap_or("?".into())
        } else {
            format!("{} NVIDIA", nv_cards_now.len())
        };
        let gen_label = match pin.arg.as_deref() {
            Some(arg) => match pin.card.as_ref() {
                Some(c) => format!("{arg} ({})", c.name),
                None => arg.to_string(),
            },
            None if extra_gpu_arg => "--gpu from Extra Launch Args".to_string(),
            None => "WanGP default (all GPUs visible)".to_string(),
        };
        emit(&format!("[*] GPU assignment — Launcher UI: {launcher_gpu} | Generation: {gen_label} (saved: {gpu_device}) | HW: {hw_name} ({hw_vendor}, {hw_vram}) | Detected: {gpu_count}\n"));
    }
    emit(&format!("[*] Args: {}\n", args.join(" ")));
    // HF_TOKEN / config env (mirrors Electron launchCfg)
    let launch_cfg = load_config_value();
    let hf_token = launch_cfg
        .get("hfToken")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let claude_key = launch_cfg
        .get("claudeApiKey")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    // Bootstrap shim (Electron parity): lies isatty()=True so tqdm +
    // huggingface_hub bars render even though stdout is piped, not a tty.
    // Fresh temp file per launch (never repo-local): %TEMP% cleaners or a
    // stale copy used to break launches with cryptic errors until restart.
    // ponytail: Electron's z-image VAE monkeypatch deliberately NOT ported — crash fix, separate issue.
    let boot = {
        let tmp = std::env::temp_dir();
        for stale in std::fs::read_dir(&tmp).into_iter().flatten().flatten() {
            let n = stale.file_name().to_string_lossy().to_string();
            if n.starts_with("wan2gp-bootstrap-") {
                // Issue #36: legacy loose files + current subdirs.
                if stale.path().is_dir() {
                    let _ = std::fs::remove_dir_all(stale.path());
                } else if n.ends_with(".py") {
                    let _ = std::fs::remove_file(stale.path());
                }
            }
        }
        // Issue #36: isolated subdir so %TEMP% itself is never sys.path[0].
        // A stray %TEMP%\grp.py used to shadow stdlib (tarfile -> import grp).
        let dir = tmp.join(format!(
            "wan2gp-bootstrap-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        ));
        let _ = std::fs::create_dir_all(&dir);
        dir.join("boot.py")
    };
    let _ = std::fs::write(
        &boot,
        r#"import os, sys, runpy
os.environ['PYTHONUNBUFFERED'] = '1'
os.environ['TQDM_MININTERVAL'] = '0'
os.environ['TQDM_MINITERS'] = '1'
os.environ['HF_HUB_DISABLE_PROGRESS_BARS'] = '0'
os.environ['HF_HUB_ENABLE_HF_TRANSFER'] = '0'
os.environ.setdefault('TERM', 'xterm-256color')
class _Tty:
    def __init__(self, inner): self._inner = inner
    def isatty(self): return True
    def fileno(self):
        try: return self._inner.fileno()
        except OSError: raise
    def __getattr__(self, n): return getattr(self._inner, n)
sys.stdout = _Tty(sys.stdout)
sys.stderr = _Tty(sys.stderr)
sys.__stdout__ = sys.stdout
sys.__stderr__ = sys.stderr
print('[bootstrap] active', flush=True)
# Issue #36: drop our own dir from sys.path so nothing beside boot.py can
# shadow stdlib; %TEMP% itself is never on sys.path (isolated subdir).
_boot_dir = os.path.dirname(os.path.abspath(__file__))
sys.path = [p for p in sys.path if os.path.abspath(p or '.') != _boot_dir]
sys.argv = sys.argv[1:]
d = os.path.dirname(os.path.abspath(sys.argv[0]))
if d not in sys.path: sys.path.insert(0, d)
runpy.run_path(sys.argv[0], run_name='__main__')
"#,
    );
    args.insert(0, boot.to_string_lossy().to_string()); // py <boot> wgp.py … (target = argv[1])
                                                        // resolve python for active env (uv/venv: Scripts\ or bin/; conda:
                                                        // python at the env root — resolve_env_python knows both layouts).
    let env = get_active_env();
    let py = if let Some(raw) = env.get("path").and_then(|p| p.as_str()) {
        let rel = raw
            .trim_start_matches(".\\")
            .trim_start_matches("./")
            .trim_start_matches(".\\")
            .trim_start_matches("./");
        let base = if Path::new(raw).is_absolute() {
            PathBuf::from(raw)
        } else {
            get_repo_dir().join(rel)
        };
        let legacy = if cfg!(windows) {
            base.join("Scripts\\python.exe")
        } else {
            base.join("bin/python3")
        };
        resolve_env_python(&get_repo_dir(), raw)
            .unwrap_or(legacy)
            .to_string_lossy()
            .to_string()
    } else {
        "python".to_string()
    };
    // Pre-flight: the interpreter must exist, run, AND import torch.
    // (Screenshot: after the failed install there was no env, launch fell back
    // to system `python` and died with `ModuleNotFoundError: No module named
    // 'torch'.) Refuse here with directions instead of a traceback there.
    {
        let torch_ok = silent_command(&py)
            .args(["-c", "import torch; print(torch.__version__)"])
            .output()
            .is_ok_and(|o| o.status.success());
        if !torch_ok {
            mutating_done();
            let reason = if env.is_null() {
                "no Python environment is installed"
            } else {
                "the environment's Python can't import torch (install incomplete or env broken)"
            };
            emit(&format!("[!] Launch blocked: {reason} [{py}].\n"));
            return Err(format!("Cannot launch: {reason}. Finish the install first (installer re-opens automatically), or repair the environment — launching now would crash on `import torch`."));
        }
    }
    emit(&format!("[*] Python: {py}\n"));
    emit(&format!("[*] Port: {port}\n"));
    // ponytail: PYTHONUNBUFFERED for streaming logs (tqdm), plus HF_TOKEN/claude key
    let mut cmd = crate::base::shell_command(&app, &py);
    cmd = cmd.args(&args).current_dir(&repo);
    // shell plugin env() — if not available, fallback to std env (child inherits)
    // tauri-plugin-shell 2.x supports .env() — use it when available
    #[allow(unused_mut)]
    let mut cmd = cmd;
    // set env via std::env for child inheritance as fallback
    // (shell plugin also inherits process env, so set temporarily)
    if !hf_token.is_empty() {
        std::env::set_var("HF_TOKEN", &hf_token);
        std::env::set_var("HUGGINGFACE_HUB_TOKEN", &hf_token);
    }
    if let Some(k) = launch_cfg.get("civitaiApiKey").and_then(|v| v.as_str()) {
        if !k.is_empty() {
            std::env::set_var("CIVITAI_API_KEY", k);
        }
    }
    if let Some(k) = launch_cfg.get("civitaiRedApiKey").and_then(|v| v.as_str()) {
        if !k.is_empty() {
            std::env::set_var("CIVITAI_RED_API_KEY", k);
        }
    }
    if !claude_key.is_empty() {
        std::env::set_var("ANTHROPIC_API_KEY", &claude_key);
    }
    // GGUF CUDA kernel knobs from Settings → GGUF CUDA Kernel. Verified against
    // the 1.0.25 wheel's llamacpp_gguf_cuda/__init__.py: it reads
    // WGP_GGUF_LLAMACPP_CUDA_MATMUL_MODE (fast/mmq/packed/low_vram -> packed
    // MMVQ/MMQ; materialized/dense/cublas -> cuBLAS on a dequant copy) and
    // LLAMACPP_GGUF_SHORT_BATCH (auto/native/mma). The pre-1.0.25 Stream-K and
    // BF16->FP16 env vars were removed upstream and are no longer set.
    // std::env persists in OUR process across launches, so always reconcile:
    // set what's configured, REMOVE stale leftovers.
    {
        let g = load_config_value()
            .get("ggufEnv")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let enabled = g.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
        if !enabled {
            std::env::set_var("WGP_GGUF_LLAMACPP_CUDA", "0");
        } else {
            std::env::remove_var("WGP_GGUF_LLAMACPP_CUDA");
            let matmul = g
                .get("matmulMode")
                .and_then(|v| v.as_str())
                .unwrap_or("auto");
            match matmul {
                "fast" | "low_vram" | "materialized" => {
                    std::env::set_var("WGP_GGUF_LLAMACPP_CUDA_MATMUL_MODE", matmul)
                }
                _ => std::env::remove_var("WGP_GGUF_LLAMACPP_CUDA_MATMUL_MODE"),
            }
            // Successor to the removed Stream-K knob (wheel 1.0.25): the
            // Q4_K/PTQ1_0 tensor-core path for 2-8 row batches. "auto" (and
            // anything unknown) leaves the wheel's own per-shape default.
            let short = g
                .get("shortBatch")
                .and_then(|v| v.as_str())
                .unwrap_or("auto");
            match short {
                "native" | "mma" => std::env::set_var("LLAMACPP_GGUF_SHORT_BATCH", short),
                _ => std::env::remove_var("LLAMACPP_GGUF_SHORT_BATCH"),
            }
            // Retire the dead pre-1.0.25 knobs so a stale process env can't
            // leak them into a launch that no longer honours them.
            std::env::remove_var("WGP_GGUF_LLAMACPP_CUDA_STREAM_K");
            std::env::remove_var("WGP_GGUF_LLAMACPP_CUDA_BF16_FP16");
        }
        emit(&format!(
            "[i] GGUF env: CUDA={} MATMUL={} SHORT_BATCH={}\n",
            std::env::var("WGP_GGUF_LLAMACPP_CUDA").unwrap_or("1".into()),
            std::env::var("WGP_GGUF_LLAMACPP_CUDA_MATMUL_MODE").unwrap_or("auto".into()),
            std::env::var("LLAMACPP_GGUF_SHORT_BATCH").unwrap_or("auto".into())
        ));
    }
    // AMD GPU profile env from setup_config.json (e.g. HSA_OVERRIDE_GFX_VERSION).
    // Upstream removed per-arch env overrides in the ROCm 10 refresh (the
    // unified AMD profile carries no `env`, and multi-arch wheels resolve
    // the gfx target themselves), so on current checkouts this only ever
    // reconciles stale leftovers away (same pattern as the GGUF knobs
    // above). Values still come verbatim from upstream's file when present
    // on old checkouts — never invented here.
    // `is_amd_launch` escapes the block: the vendor-independent JIT pins
    // below (Triton cache dir, MSVC compiler) need it without a second
    // WMI probe.
    let is_amd_launch = {
        let gpu = get_gpu_info_sync();
        let profile = kernel_profile_key(
            gpu.get("vendor").and_then(|v| v.as_str()).unwrap_or(""),
            gpu.get("name").and_then(|v| v.as_str()).unwrap_or(""),
        );
        let env_map = std::fs::read_to_string(repo.join("setup_config.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|c| {
                c.get("gpu_profiles")
                    .and_then(|p| p.get(&profile))
                    .and_then(|pr| pr.get("env"))
                    .cloned()
            })
            .unwrap_or(serde_json::Value::Null);
        // Keys this launcher owns: anything upstream ever puts under a
        // profile's `env` today is just the HSA override — remove it when
        // the active profile doesn't declare it (e.g. after switching GPUs).
        // Empirically probed choice wins over the static file: the working
        // R9700 config sets no override at all, and blindly forcing 12.0.1
        // is suspect #1 in the 0.5.2 quanto crash. Written by install/verify.
        const MANAGED_AMD_ENV: &[&str] = &["HSA_OVERRIDE_GFX_VERSION"];
        // The probed choice is per-GPU evidence — only honor it while an
        // AMD card is actually present; a stale file after a GPU switch
        // must never set HSA vars on another vendor.
        if profile.starts_with("AMD") {
            match crate::amd::read_hsa_choice(&repo) {
                Some(crate::amd::HsaChoice::Native) => {
                    for key in MANAGED_AMD_ENV {
                        std::env::remove_var(key);
                    }
                    emit("[i] HSA override off (compute probe passed native on this GPU)\n");
                }
                Some(crate::amd::HsaChoice::Override(v)) => {
                    std::env::set_var("HSA_OVERRIDE_GFX_VERSION", &v);
                    emit(&format!(
                        "[i] HSA override {v} (compute probe winner on this GPU)\n"
                    ));
                }
                // No probed choice (legacy install): static setup_config behavior.
                None => {
                    if let Some(obj) = env_map.as_object() {
                        for (k, val) in obj {
                            if let Some(s) = val.as_str() {
                                std::env::set_var(k, s);
                                emit(&format!("[i] GPU profile env: {k}={s}\n"));
                            }
                        }
                    }
                }
            }
        } else {
            let _ = std::fs::remove_file(crate::amd::hsa_choice_path(&repo));
        }
        for key in MANAGED_AMD_ENV {
            if env_map.get(*key).and_then(|v| v.as_str()).is_none() {
                std::env::remove_var(key);
            }
        }
        profile.starts_with("AMD")
    };
    /// ROCm SDK dir for the AMD launch env. Prefers the `rocm-sdk` CLI
    /// shipped inside the TheRock `rocm` dist (`rocm-sdk path --root` —
    /// the `rocm-sdk` binary on PATH first, then `<env python> -m rocm_sdk`,
    /// since the exact installed entry point varies by `rocm` release),
    /// falling back to dir-guessing (`<env>/Lib/site-packages/rocm`)
    /// when the CLI is absent or fails. Never invents paths; None when
    /// everything misses. AMD-gated by the caller; NVIDIA/Intel/CPU paths
    /// never call it.
    pub(crate) fn amd_rocm_sdk_dir() -> Option<PathBuf> {
        // CLI first (warn-only by design: any failure falls through to
        // dir-guessing below, never fails launch).
        if let Ok(o) = silent_command("rocm-sdk").args(["path", "--root"]).output() {
            if o.status.success() {
                if let Some(d) = parse_rocm_sdk_root(&String::from_utf8_lossy(&o.stdout)) {
                    return Some(d);
                }
            }
        }
        let env = get_active_env();
        let raw = env.get("path")?.as_str()?;
        if let Some(py) = resolve_env_python(&crate::base::get_repo_dir(), raw) {
            if let Ok(o) = silent_command(&py)
                .args(["-m", "rocm_sdk", "path", "--root"])
                .output()
            {
                if o.status.success() {
                    if let Some(d) = parse_rocm_sdk_root(&String::from_utf8_lossy(&o.stdout)) {
                        return Some(d);
                    }
                }
            }
        }
        let base = if Path::new(raw).is_absolute() {
            PathBuf::from(raw)
        } else {
            crate::base::get_repo_dir().join(
                raw.trim_start_matches(".\\")
                    .trim_start_matches("./")
                    .trim_start_matches(".\\")
                    .trim_start_matches("./"),
            )
        };
        // Windows venv/uv/conda layout first, then POSIX layouts.
        let direct = [
            base.join("Lib").join("site-packages").join("rocm"),
            base.join("lib").join("site-packages").join("rocm"),
        ];
        for d in direct {
            if d.is_dir() {
                return Some(d);
            }
        }
        // POSIX versioned lib dir: <env>/lib/python3*/site-packages/rocm.
        if let Ok(rd) = std::fs::read_dir(base.join("lib")) {
            for e in rd.flatten() {
                let cand = e.path().join("site-packages").join("rocm");
                if cand.is_dir() {
                    return Some(cand);
                }
            }
        }
        None
    }
    // AMD ROCm session env (doc-leading: upstream docs/AMD-INSTALLATION.md
    // post-refresh — NO compiler/SDK env at launch). Only set-if-absent
    // attention-runtime knobs remain; everything else is diagnostics.
    // Stale values from older launcher versions are NOT harmless on any
    // launch: `CC=clang-cl`, `CXX=clang-cl`, `DISTUTILS_USE_SDK=1` and a
    // ROCm-prepended PATH break later Intel/CPU/NVIDIA pip builds in this
    // same launcher process — so the else branch below reconciles them
    // away (HSA handling above stays exactly as-is).
    {
        let gpu = get_gpu_info_sync();
        let profile = kernel_profile_key(
            gpu.get("vendor").and_then(|v| v.as_str()).unwrap_or(""),
            gpu.get("name").and_then(|v| v.as_str()).unwrap_or(""),
        );
        if profile.starts_with("AMD") {
            // wgp.py sets TORCH_ROCM_AOTRITON_ENABLE_EXPERIMENTAL itself
            // (setdefault); the launcher keeps a set-if-absent copy so
            // explicit user overrides always win. FLASH_ATTENTION_TRITON_AMD
            // is build-time only upstream (aiter submodule build) and is
            // NOT set at launch; stale values are reconciled away off-AMD.
            {
                let (k, v) = ("TORCH_ROCM_AOTRITON_ENABLE_EXPERIMENTAL", "1");
                if std::env::var(k).is_err() {
                    std::env::set_var(k, v);
                    emit(&format!("[i] GPU profile env: {k}={v}\n"));
                }
            }
            // MIOpen toggle (issue #15 follow-up): Manage-backed
            // `amdEnv.miopenDisabled` (default false, Manage → AMD ROCm).
            // When true,
            // MIOPEN_FIND_MODE is NOT set at all and the log says so;
            // when false, the current FAST set-if-absent stays.
            match miopen_find_mode() {
                Some(v) => {
                    if std::env::var("MIOPEN_FIND_MODE").is_err() {
                        std::env::set_var("MIOPEN_FIND_MODE", v);
                        emit(&format!("[i] GPU profile env: MIOPEN_FIND_MODE={v}\n"));
                    }
                }
                None => {
                    // A previous launch in this app session may have set it
                    // process-wide; children inherit the parent env, so an
                    // empty arm left MIOpen enabled despite the toggle.
                    std::env::remove_var("MIOPEN_FIND_MODE");
                    emit("[i] MIOpen disabled by user setting\n");
                }
            }
            // Virtual-GPU HIP pin (strict conditions only): exactly one
            // discrete AMD GPU PLUS ignored virtual display adapter(s)
            // (e.g. a Meta Virtual Monitor next to one discrete card) →
            // set-if-absent HIP_VISIBLE_DEVICES=0 so HIP can't land on the
            // virtual shim. Multi-AMD boxes (the index would be a guess)
            // and no-virtual-adapter boxes do nothing.
            {
                let amd_adapters: Vec<_> = wmi_all_gpus()
                    .into_iter()
                    .filter(|(_, v, _, _)| v == "AMD")
                    .collect();
                let discrete = amd_adapters
                    .iter()
                    .filter(|(name, _, _, _)| !crate::hw::is_amd_apu(name))
                    .count();
                let apus = amd_adapters
                    .iter()
                    .filter(|(name, _, _, _)| crate::hw::is_amd_apu(name))
                    .count();
                let virtuals = wmi_virtual_adapters().len();
                if let Some(v) = hip_pin_value(discrete, apus, virtuals) {
                    if std::env::var("HIP_VISIBLE_DEVICES").is_err() {
                        std::env::set_var("HIP_VISIBLE_DEVICES", &v);
                        emit(&format!(
                            "[i] GPU profile env: HIP_VISIBLE_DEVICES={v} (one discrete AMD GPU + {} APU / {} virtual adapter(s) — pinning HIP to the discrete card)\n",
                            apus, virtuals
                        ));
                    }
                }
            }
            // Full AMD launch env: derive the ROCm SDK dir from the
            // installed env (the `rocm` package dir, e.g.
            // `<env>/Lib/site-packages/rocm`). Never invent paths: when
            // the dir is absent, log and continue without failing launch.
            // ROCm SDK diagnostics only (upstream b8b18f8): NO launch-time
            // env is set here anymore — triton-windows resolves the SDK
            // from the active env itself, and ROCM_HOME / PATH prepends /
            // CC / CXX / DISTUTILS_USE_SDK belong to the retired pre-refresh
            // guide (setting CC=clang-cl process-wide used to break later
            // non-AMD pip builds). The non-AMD reconcile below still strips
            // stale values left by older launcher versions. We only log.
            match amd_rocm_sdk_dir() {
                Some(sdk) => emit(&format!(
                    "[i] ROCm SDK detected at {} (no env vars set — triton resolves it from the env)\n",
                    sdk.to_string_lossy()
                )),
                None => emit("[i] ROCm SDK dir not found in the active env — launching anyway (no ROCm env vars required).\n"),
            }
        } else {
            // Non-AMD launch in a process that previously ran AMD: strip
            // the stale ROCm/compiler session env (unconditional — these
            // keys are meaningless off-AMD) and our recorded PATH prepend.
            // HSA handling above is untouched; the Intel path gains zero
            // new env behavior beyond this reconcile.
            reconcile_non_amd_session_env(&emit);
        }
    }
    // Triton JIT cache pin (see triton_cache_dir_for): set-if-absent so an
    // explicit user override stays authoritative. The folder is created here
    // (warn-only — creation failure leaves Triton's default location); the
    // first launch after this change recompiles the old ~/.triton cache once.
    if std::env::var("TRITON_CACHE_DIR").is_ok() {
        emit("[i] TRITON_CACHE_DIR already set outside the launcher — leaving it untouched\n");
    } else {
        let dir = triton_cache_dir_for(&get_data_dir());
        match std::fs::create_dir_all(&dir) {
            Ok(()) => {
                std::env::set_var("TRITON_CACHE_DIR", &dir);
                emit(&format!(
                    "[i] Triton JIT cache pinned to {} (one stable folder to add to your antivirus exclusions — Triton's compiled helpers there are false-positive targets)\n",
                    dir.to_string_lossy()
                ));
            }
            Err(e) => emit(&format!(
                "[i] could not create the pinned Triton cache folder ({e}) — Triton keeps its default cache location\n"
            )),
        }
    }
    // MSVC compiler pin for Triton JIT (see MsvcPin): NVIDIA/Intel/CPU only.
    // Without it every stub build goes through the wheel-bundled unsigned
    // TinyCC (Bitdefender ATD blocked python.exe → tcc.exe mid-generation);
    // a signed cl.exe dissolves the "unsigned spawns unsigned compiler"
    // pattern. AMD launches instead remove our pin — TheRock's clang-cl
    // must win there, and CC env would shadow it.
    apply_msvc_cc_pin(is_amd_launch, emit);
    std::env::set_var("PYTHONUNBUFFERED", "1");
    std::env::set_var("PYTHONUTF8", "1");
    std::env::set_var("PYTHONIOENCODING", "utf-8");
    // Live progress bars (mirrors Electron terminal env): redraw every iteration.
    std::env::set_var("TQDM_MININTERVAL", "0");
    std::env::set_var("TQDM_MINITERS", "1");
    // Bars on for piped output; classic hf_hub path (hf_transfer has its own non-tqdm progress).
    std::env::set_var("HF_HUB_DISABLE_PROGRESS_BARS", "0");
    std::env::set_var("HF_HUB_ENABLE_HF_TRANSFER", "0");
    std::env::set_var("NO_PROXY", "localhost,127.0.0.1,::1");
    // External-terminal mode: visible console window running wgp.py (run.bat style).
    // Not a child we stream — the user owns the window; Stop also kills by title.
    // "terminal-nogpu" runs the same visible console but opens no-GPU Chrome
    // from the script instead of the OS default browser.
    if mode == "terminal" || mode == "terminal-nogpu" {
        return launch_in_terminal(
            app,
            &repo,
            &py,
            &args,
            port,
            &cfg,
            hf_token.clone(),
            claude_key.clone(),
            mode == "terminal-nogpu",
        );
    }
    // `shell_command()` snapshots the environment when the Command is built
    // (env_clear + child_env), but every launch pin the launcher needs is a
    // set_var() *after* that line: HF/Civitai/Claude keys from config,
    // TRITON_CACHE_DIR, PYTHONUNBUFFERED/PYTHONUTF8, TQDM_* and the
    // WGP_GGUF_* knobs. Windows inherits the live process env at spawn, so
    // it never noticed; Linux/macOS kept the stale snapshot and silently
    // dropped all of them (0.10.27 added the env_clear). Re-snapshot here.
    #[cfg(not(windows))]
    let cmd = cmd.env_clear().envs(crate::base::child_env());
    let (rx, child) = cmd.spawn().map_err(|e| {
        mutating_done();
        emit(&format!("[LAUNCH ERROR] spawn failed: {e}\n"));
        e.to_string()
    })?;
    emit(&format!("[*] Spawned PID {}\n", child.pid()));
    if let Ok(m) = WANGP_PID.lock() {
        drop(m);
    }
    *WANGP_PID.lock().unwrap() = Some(child.pid());
    // stream logs in background
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        use tauri_plugin_shell::process::CommandEvent;
        let mut rx = rx;
        while let Some(ev) = rx.recv().await {
            match ev {
                CommandEvent::Stdout(b) => {
                    let s = crate::base::tag_child_lines(&String::from_utf8_lossy(&b));
                    crate::base::push_log(&s, "launch");
                    let _ = app2.emit("launch-log", s);
                }
                CommandEvent::Stderr(b) => {
                    let s = crate::base::tag_child_lines(&String::from_utf8_lossy(&b));
                    crate::base::push_log(&s, "launch");
                    let _ = app2.emit("launch-log", s);
                }
                CommandEvent::Terminated(s) => {
                    let _ = app2.emit("wangp-exit", serde_json::json!({"code": s.code}));
                    break;
                }
                _ => {}
            }
        }
    });
    // wait for port in background (don't hold mutating — launch is done, server boots async)
    let host = "127.0.0.1".to_string();
    let app3 = app.clone();
    std::thread::spawn(move || {
        for _ in 0..60 {
            std::thread::sleep(std::time::Duration::from_secs(3));
            if std::net::TcpStream::connect(format!("{host}:{port}")).is_ok() {
                let m = format!("[✓] Wan2GP ready on http://localhost:{port}\n");
                crate::base::push_log(&m, "launch");
                let _ = app3.emit("launch-log", m);
                break;
            }
        }
    });
    mutating_done();
    let url = format!("http://localhost:{port}");
    Ok(serde_json::json!({"ok": true, "port": port, "mode": mode, "url": url, "fresh": true}))
}
/// Blocking worker (see async wrapper below): WMI/port scans + kill verifies
/// take seconds — running them on Tauri's invoke pool starves concurrent
/// commands (metrics, toasts) and the UI visibly stalls.
#[tauri::command]
pub async fn stop_wangp(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || stop_wangp_blocking(app))
        .await
        .map_err(|e| e.to_string())
}
/// Fast synchronous kill for app-close (see shutdown_cleanup): tracked
/// child PID + external-terminal window only — no PowerShell scans, no
/// sleeps, milliseconds. The full verified sweep stays on the Stop button
/// path (plus close-time best-effort background work).
/// Kill one PID (/f /t tree-kill on Windows, -9 elsewhere). Skips 0,
/// our own PID and already-killed PIDs. File-level so both the Stop
/// sweep and the close sweep share it (was an identical closure).
fn kill_pid(pid: u32, killed: &mut Vec<u32>) {
    if pid == 0 || pid == std::process::id() || killed.contains(&pid) {
        return;
    }
    crate::platform::current().kill_pid(pid);
    killed.push(pid);
}
/// Wan2GP ports: configured serverPort + the 7860/7861 defaults. Shared
/// by the Stop sweep and the close sweep so they can never disagree.
fn wan2gp_ports() -> Vec<u64> {
    let cfg = load_config_value();
    let sport = cfg
        .get("serverPort")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(7860);
    let mut sports = vec![sport];
    for p in [7860u64, 7861u64] {
        if !sports.contains(&p) {
            sports.push(p);
        }
    }
    // Deepy Web standalone (deepyPort, default serverPort+1) must die on app
    // close too — otherwise it outlives the launcher as an orphan holding
    // its port (seen live on :7862 while only the main ports were swept).
    if let Some(dport) = crate::deepy_web::deepy_sweep_port(
        sport,
        cfg.get("deepyPort").and_then(serde_json::Value::as_u64),
    ) {
        if !sports.contains(&dport) {
            sports.push(dport);
        }
    }
    sports
}
/// Repo prefix for origin checks (`c:\wan2gp\` lowercase). Shared by
/// the Stop matcher and the close sweep so they can never disagree.
fn wangp_repo_prefix() -> String {
    format!(
        "{}\\",
        get_repo_dir()
            .to_string_lossy()
            .replace('/', "\\")
            .to_lowercase()
    )
}
/// True only for processes OUR launcher started: the cmdline must name
/// wgp.py AND carry one of three repo signals — repo path in argv,
/// interpreter under the repo, or our bootstrap filename. Anything else
/// (user scripts, other software's Python) is foreign and must survive.
/// Pure so it stays unit-tested.
fn is_ours_process(exe: &str, cmd: &str, repo_pre: &str) -> bool {
    if !cmd.to_lowercase().contains("wgp.py") {
        return false;
    }
    let cl = cmd.to_lowercase();
    cl.contains(repo_pre)                      // interpreter or script path under repo
|| exe.to_lowercase().replace('/', "\\").starts_with(repo_pre) // env python, relative argv
|| cl.contains("wan2gp-bootstrap-") // our launcher bootstrap (any interpreter)
}
/// Origin signature of one PID: (interpreter path, full command line).
/// None when the process is gone or unreadable — callers must treat
/// None as FOREIGN (fail closed, never kill unverifiable processes).
fn process_signature(pid: u32) -> Option<(String, String)> {
    if pid == 0 || pid == std::process::id() {
        return None;
    }
    crate::platform::current().process_signature(pid)
}
/// Close-path guard: never ourselves, never PID 0. Origin is checked
/// separately via is_ours_process — this is only the sanity gate.
/// Pure so it stays unit-tested.
fn close_sweep_should_kill(pid: u32) -> bool {
    pid != 0 && pid != std::process::id()
}
/// Final close sweep: re-check every listener on the Wan2GP ports and
/// kill it ONLY when it is provably ours — tracked child PID, or a
/// cmdline carrying our repo signals (is_ours_process). Orphans from
/// crashed instances die here; foreign processes (user scripts, other
/// software's Python) are spared and logged as such. Unverifiable PIDs
/// fail closed (spared, loudly). Runs last in shutdown_cleanup_full.
pub(crate) fn close_port_sweep() -> Vec<u32> {
    let sports = wan2gp_ports();
    let repo_pre = wangp_repo_prefix();
    let tracked = WANGP_PID.lock().ok().and_then(|g| *g);
    // One scan: (pid, process name, port) for EVERY listener on our
    // ports — no owner pre-filter; origin is decided per PID below.
    let listeners = match crate::platform::current().port_listeners(&sports) {
        Ok(v) => v,
        Err(e) => {
            crate::base::push_log(&format!("[stop] CLOSE sweep scan failed: {e}\n"), "launch");
            Vec::new()
        }
    };
    let mut killed: Vec<u32> = Vec::new();
    for l in listeners {
        let (pid, name, port) = (l.pid, l.name, l.port);
        if !close_sweep_should_kill(pid) || killed.contains(&pid) {
            continue;
        }
        let ours = tracked == Some(pid)
            || process_signature(pid)
                .map(|(exe, cmd)| is_ours_process(&exe, &cmd, &repo_pre))
                .unwrap_or(false);
        if ours {
            crate::base::push_log(
                &format!(
                    "[stop] CLOSE sweep: killing {} PID {pid} on Wan2GP port {port} (verified ours)\n",
                    if name.is_empty() { "?" } else { &name }
                ),
                "launch",
            );
            kill_pid(pid, &mut killed);
        } else {
            // Foreign or unverifiable — spared by policy, but said out
            // loud so a surviving OUR server can never hide here silently.
            crate::base::push_log(
                &format!(
                    "[stop] CLOSE sweep: leaving {} PID {pid} on port {port} (not ours — spared)\n",
                    if name.is_empty() { "?" } else { &name }
                ),
                "launch",
            );
        }
    }
    killed
}
/// Is a WanGP still alive that could rewrite `wgp_config.json` under us?
///
/// Two cheap signals, neither can hang: the PID this launcher spawned (with
/// `process_signature` so a recycled PID fails closed) and the configured
/// server port, which also catches a WanGP the user started themselves.
/// Used by `memory_profile_apply` — WanGP keeps its own copy of the config in
/// memory and rewrites the file on any settings change inside its UI, so
/// whichever side persists last wins.
pub(crate) fn wangp_running() -> bool {
    if let Some(pid) = WANGP_PID.lock().ok().and_then(|g| *g) {
        if pid != 0 && pid != std::process::id() && process_signature(pid).is_some() {
            return true;
        }
    }
    let port = load_config_value()
        .get("serverPort")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(7860);
    use std::net::ToSocketAddrs;
    let timeout = std::time::Duration::from_millis(300);
    match format!("127.0.0.1:{port}").to_socket_addrs() {
        Ok(mut addrs) => addrs
            .next()
            .is_some_and(|a| std::net::TcpStream::connect_timeout(&a, timeout).is_ok()),
        Err(_) => false,
    }
}

pub(crate) fn stop_wangp_fast() {
    if let Some(pid) = WANGP_PID.lock().ok().and_then(|g| *g) {
        if pid != 0 && pid != std::process::id() {
            crate::platform::current().kill_pid(pid);
        }
    }
    #[cfg(windows)]
    if let Some(t) = terminal_title() {
        let _ = silent_command("taskkill")
            .args(["/F", "/FI", &format!("WINDOWTITLE eq {t}*")])
            .output();
    }
}
/// Delete the per-launch scripts this launcher owns: `wan2gp-terminal-*.bat`
/// and the short-lived `wan2gp-env-*.txt` secret staging files. Called on
/// stop and at startup, so a crash cannot leave tokens in %TEMP%.
pub(crate) fn sweep_terminal_temp_files() {
    let tmp = std::env::temp_dir();
    let Ok(rd) = std::fs::read_dir(&tmp) else {
        return;
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let ours = (name.starts_with("wan2gp-terminal-") && name.ends_with(".bat"))
            || (name.starts_with("wan2gp-env-") && name.ends_with(".txt"));
        if ours && entry.path().is_file() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

pub(crate) fn stop_wangp_blocking(app: tauri::AppHandle) -> serde_json::Value {
    // Scoped stop: ONLY our Wan2GP processes — the tracked child plus any python
    // running OUR repo's wgp.py (uv-shim/child split, detached terminal mode).
    // ponytail: the old `taskkill /F /IM python.exe` blanket-killed every Python
    // on the machine (user scripts, other apps) — never again.
    // Matching uses THREE independent repo signals because no single one covers
    // every spawn shape: (1) repo path in the command line (cwd-relative argv
    // still shows the env interpreter path under the repo), (2) interpreter
    // ExecutablePath under the repo, (3) our `wan2gp-bootstrap-<pid>-<ms>.py`
    // filename — the ONLY signal for uv-managed interpreters outside the repo
    // (proven orphan: uv python in AppData + relative `wgp.py`, unkillable by
    // the old repo-path-only filter).
    let repo_pre = wangp_repo_prefix();
    let mut killed: Vec<u32> = Vec::new();
    // One scan pass: every process whose command line mentions wgp.py. The
    // adapter owns the query (WMI/PowerShell on Windows, pgrep on Unix) and
    // origin filtering happens in Rust (exact, case-insensitive). The marker
    // is the FILE NAME, not the absolute path, so a server started with a
    // relative `wgp.py` argv (cwd = repo) is still found. Failures are LOUD:
    // a silently-empty scan is exactly how orphans used to survive Stop.
    let scan = || -> Vec<crate::platform::ProcessInfo> {
        match crate::platform::current().scan_processes("wgp.py") {
            Ok(v) => v,
            Err(e) => {
                crate::base::push_log(&format!("[stop] process scan failed: {e}\n"), "launch");
                Vec::new()
            }
        }
    };
    // Origin matcher shared with the close sweep (is_ours_process) —
    // one definition, one behaviour, unit-tested at the file level.
    let is_ours = |exe: &str, cmd: &str| -> bool { is_ours_process(exe, cmd, &repo_pre) };
    if let Some(pid) = WANGP_PID.lock().ok().and_then(|g| *g) {
        kill_pid(pid, &mut killed);
    }
    let mut found = 0usize;
    // Cache the first WMI pass: reused below by the custom-port sweep so
    // the extra python-listener probe stays the ONE extra PS call.
    let scan_first = scan();
    found += scan_first.len();
    for p in &scan_first {
        if is_ours(&p.exe, &p.cmd) {
            kill_pid(p.pid, &mut killed);
        }
    }
    // our external-terminal window (unique timestamped title)
    #[cfg(windows)]
    if let Some(t) = crate::launch::terminal_title() {
        let _ = silent_command("taskkill")
            .args(["/F", "/FI", &format!("WINDOWTITLE eq {t}*")])
            .output();
    }
    // Best-effort cleanup of per-launch terminal scripts and staged env files.
    sweep_terminal_temp_files();
    // Ground truth: whatever LISTENS on a Wan2GP port dies too. Catches every
    // spawn shape (workers, renamed interpreters, stale launchers) — launch
    // itself treats port-in-use as "ours" (reuses instead of spawning), so
    // stop must treat it the same way. Restricted to python* owners: Gradio
    // always runs on Python, and we never kill foreign processes.
    // Ports: configured serverPort + the 7860/7861 defaults. A launcher killed
    // for a rebuild can't clean up, so the next instance (possibly with a
    // changed port) must still catch the previous session's listener —
    // proven orphan class 2026-09-10 (PID on :7861 survived every Stop).
    let sports = wan2gp_ports();
    // Live progress: the sweep below takes seconds (PowerShell spawns) —
    // without these the console sits dead and Stop feels frozen.
    let say = |m: &str| {
        crate::base::push_log(m, "launch");
        let _ = app.emit("launch-log", m.to_string());
    };
    say("[stop] scanning Wan2GP processes…\n");
    // Pure scan (no kill): (pid, port) of python listeners on ANY of `ports`.
    // One adapter call for all ports (was one PowerShell spawn per port — the
    // stall). Used by the sweep below and by the alive check, so survivors are
    // reported, not hidden.
    let port_listeners = |ports: &[u64]| -> Vec<(u32, u64)> {
        let listeners = match crate::platform::current().port_listeners(ports) {
            Ok(v) => v,
            Err(e) => {
                crate::base::push_log(&format!("[stop] port scan failed: {e}\n"), "launch");
                Vec::new()
            }
        };
        // Restricted to python* owners: Gradio always runs on Python, and we
        // never kill whatever else happens to hold a Wan2GP port.
        listeners
            .into_iter()
            .filter(|l| l.name.to_lowercase().contains("python"))
            .map(|l| (l.pid, l.port))
            .collect()
    };
    say("[stop] checking Wan2GP ports…\n");
    for (pid, port) in port_listeners(&sports) {
        crate::base::push_log(
            &format!("[stop] port {port}: killing python PID {pid}\n"),
            "launch",
        );
        kill_pid(pid, &mut killed);
    }
    // Custom-port evasion: every TCP listener on the box, any LocalPort. Kept
    // only when it is provably ours — python-owned on a known Wan2GP port, or
    // the cmdline from the first scan carries our repo signals. Kill scope is
    // NOT widened: kill_pid only, same as every other sweep.
    let any_listeners: Vec<crate::platform::Listener> =
        match crate::platform::current().tcp_listeners() {
            Ok(v) => v,
            Err(e) => {
                crate::base::push_log(&format!("[stop] listener scan failed: {e}\n"), "launch");
                Vec::new()
            }
        };
    for l in any_listeners {
        if killed.contains(&l.pid) {
            continue;
        }
        if sports.contains(&l.port) {
            // Already covered by the sports sweep above; a live entry here
            // means the earlier kill missed — retry loudly.
            if l.name.to_lowercase().contains("python") {
                crate::base::push_log(
                    &format!("[stop] port {}: killing python PID {}\n", l.port, l.pid),
                    "launch",
                );
                kill_pid(l.pid, &mut killed);
            }
        } else if let Some(p) = scan_first.iter().find(|p| p.pid == l.pid) {
            // Custom-port evasion: python listener off the known ports that is
            // still OURS by cmdline signals.
            if is_ours(&p.exe, &p.cmd) {
                crate::base::push_log(
                    &format!(
                        "[stop] custom port {}: killing ours PID {}\n",
                        l.port, l.pid
                    ),
                    "launch",
                );
                kill_pid(l.pid, &mut killed);
            }
        }
    }
    if let Ok(mut g) = WANGP_PID.lock() {
        *g = None;
    }
    // Verify: re-scan after the dust settles, kill stragglers once, report
    // who's still alive instead of claiming success with orphans around.
    std::thread::sleep(std::time::Duration::from_millis(1200));
    for p in scan() {
        if is_ours(&p.exe, &p.cmd) && !killed.contains(&p.pid) {
            kill_pid(p.pid, &mut killed);
        }
    }
    // Stragglers on ANY Wan2GP port (same orphan class as above).
    for (pid, port) in port_listeners(&sports) {
        if !killed.contains(&pid) {
            crate::base::push_log(
                &format!("[stop] port {port}: killing straggler PID {pid}\n"),
                "launch",
            );
            kill_pid(pid, &mut killed);
        }
    }
    say("[stop] verifying…\n");
    std::thread::sleep(std::time::Duration::from_millis(600));
    let mut alive: Vec<u32> = scan()
        .into_iter()
        .filter(|p| is_ours(&p.exe, &p.cmd))
        .map(|p| p.pid)
        .collect();
    for (pid, _port) in port_listeners(&sports) {
        // Still listening = still alive, even if we already tried to kill
        // it (failed kill must stay visible, never be hidden).
        if !alive.contains(&pid) {
            alive.push(pid);
        }
    }
    let _ = app.emit(
        "wangp-exit",
        serde_json::json!({"stopped": true, "killed": killed}),
    );
    if !alive.is_empty() {
        crate::base::push_log(&format!("[!] stop_wangp: {} process(es) survived Kill ({} checked): {:?} — kill them manually or restart.
", alive.len(), found, alive), "launch");
    }
    serde_json::json!({"ok": true, "killed": killed, "alive": alive})
}

/// Stop every server this launcher owns: Wan2GP (+children, verified) and
/// the OpenCode server (if we spawned it). One dashboard button, no
/// leftovers. Reuses stop_wangp so behavior can never diverge from it.
#[tauri::command]
pub async fn stop_all_servers(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    // Same pool-starvation fix as stop_wangp: off the invoke pool.
    tauri::async_runtime::spawn_blocking(move || {
        let wangp = stop_wangp_blocking(app);
        let opencode = crate::features::stop_opencode_server();
        serde_json::json!({"ok": true, "wangp": wangp, "opencode_stopped": opencode})
    })
    .await
    .map_err(|e| e.to_string())
}

// ── misc helpers ──
/// Expand Windows %VAR% placeholders case-insensitively. The old code only
/// replaced exact-case and lowercase forms, but Windows env names come back
/// UPPERCASE (LOCALAPPDATA) — so a literal `%LocalAppData%` never matched and
/// every LocalAppData-only browser (Brave, Opera, Vivaldi) was permanently
/// reported "not installed" and couldn't be selected.
fn expand_win_env(s: &str) -> String {
    let mut out = s.to_string();
    let vars: Vec<(String, String)> = std::env::vars().collect();
    for (k, v) in &vars {
        let needle = format!("%{k}%").to_uppercase();
        loop {
            let up = out.to_uppercase();
            match up.find(&needle) {
                Some(pos) => out.replace_range(pos..pos + needle.len(), v),
                None => break,
            }
        }
    }
    out
}
/// Install-location candidates per browser, per OS. The table used to be
/// Windows-only, so on Linux/macOS the No-GPU probe never found a browser and
/// the launch buttons stayed disabled even with Chrome or Firefox installed.
/// Windows keeps `%Var%` entries (expanded by `expand_win_env`); Linux lists
/// distro/snap paths, macOS the `.app` bundle executables.
fn browser_candidates(id: &str) -> &'static [&'static str] {
    if cfg!(windows) {
        match id {
            "chrome" => &[
                "%ProgramFiles%\\Google\\Chrome\\Application\\chrome.exe",
                "%ProgramFiles(x86)%\\Google\\Chrome\\Application\\chrome.exe",
                "%LocalAppData%\\Google\\Chrome\\Application\\chrome.exe",
            ],
            "edge" => &[
                "%ProgramFiles%\\Microsoft\\Edge\\Application\\msedge.exe",
                "%ProgramFiles(x86)%\\Microsoft\\Edge\\Application\\msedge.exe",
            ],
            "firefox" => &[
                "%ProgramFiles%\\Mozilla Firefox\\firefox.exe",
                "%ProgramFiles(x86)%\\Mozilla Firefox\\firefox.exe",
                "%LocalAppData%\\Mozilla Firefox\\firefox.exe",
            ],
            "brave" => &[
                "%LocalAppData%\\BraveSoftware\\Brave-Browser\\Application\\brave.exe",
                "%ProgramFiles%\\BraveSoftware\\Brave-Browser\\Application\\brave.exe",
                "%ProgramFiles(x86)%\\BraveSoftware\\Brave-Browser\\Application\\brave.exe",
            ],
            "opera" => &[
                "%LocalAppData%\\Programs\\Opera\\launcher.exe",
                "%ProgramFiles%\\Opera\\launcher.exe",
                "%ProgramFiles(x86)%\\Opera\\launcher.exe",
            ],
            "vivaldi" => &[
                "%LocalAppData%\\Vivaldi\\Application\\vivaldi.exe",
                "%ProgramFiles%\\Vivaldi\\Application\\vivaldi.exe",
                "%ProgramFiles(x86)%\\Vivaldi\\Application\\vivaldi.exe",
            ],
            _ => &[],
        }
    } else if cfg!(target_os = "macos") {
        match id {
            "chrome" => &["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"],
            "edge" => &["/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"],
            "firefox" => &["/Applications/Firefox.app/Contents/MacOS/firefox"],
            "brave" => &["/Applications/Brave Browser.app/Contents/MacOS/Brave Browser"],
            "opera" => &[
                "/Applications/Opera.app/Contents/MacOS/Opera",
                "/Applications/Opera GX.app/Contents/MacOS/Opera",
            ],
            "vivaldi" => &["/Applications/Vivaldi.app/Contents/MacOS/Vivaldi"],
            _ => &[],
        }
    } else {
        match id {
            "chrome" => &[
                "/usr/bin/google-chrome",
                "/usr/bin/google-chrome-stable",
                "/usr/bin/chromium",
                "/usr/bin/chromium-browser",
                "/snap/bin/chromium",
            ],
            "edge" => &[
                "/usr/bin/microsoft-edge",
                "/usr/bin/microsoft-edge-stable",
                "/opt/microsoft/msedge/msedge",
            ],
            "firefox" => &[
                "/usr/bin/firefox",
                "/snap/bin/firefox",
                "/usr/lib/firefox/firefox",
            ],
            "brave" => &["/usr/bin/brave-browser", "/snap/bin/brave"],
            "opera" => &["/usr/bin/opera", "/usr/bin/opera-stable"],
            "vivaldi" => &["/usr/bin/vivaldi", "/usr/bin/vivaldi-stable"],
            _ => &[],
        }
    }
}
#[tauri::command]
pub fn detect_browsers() -> serde_json::Value {
    // mirrors Electron WELL_KNOWN_BROWSERS with win env expansion
    let cfg = load_config_value();
    let def = cfg
        .get("defaultBrowser")
        .and_then(|v| v.as_str())
        .unwrap_or("system")
        .to_string();
    let browsers = vec![
        ("chrome", "Google Chrome"),
        ("edge", "Microsoft Edge"),
        ("firefox", "Firefox"),
        ("brave", "Brave"),
        ("opera", "Opera"),
        ("vivaldi", "Vivaldi"),
    ];
    let mut out = Vec::new();
    for (id, name) in browsers {
        let mut path: Option<String> = None;
        for cand in browser_candidates(id) {
            let ep = expand_win_env(cand);
            if std::path::Path::new(&ep).exists() {
                path = Some(ep);
                break;
            }
        }
        out.push(
            serde_json::json!({"id": id, "name": name, "installed": path.is_some(), "path": path}),
        );
    }
    serde_json::json!({"browsers": out, "defaultBrowser": def})
}
#[tauri::command]
pub fn launch_browser(app: tauri::AppHandle, url: Option<String>) -> serde_json::Value {
    use tauri_plugin_opener::OpenerExt;
    let u = url.unwrap_or_else(|| "http://localhost:7861".into());
    if !(u.starts_with("http://") || u.starts_with("https://")) {
        return serde_json::json!({"ok": false, "success": false, "error": "invalid url"});
    }
    let chosen = load_config_value()
        .get("defaultBrowser")
        .and_then(|v| v.as_str())
        .unwrap_or("system")
        .to_string();
    // "system" (or anything unresolved) → OS default via opener.
    let exe = if chosen == "system" {
        None
    } else {
        find_browser_exe(&chosen)
    };
    // A stale selection (browser uninstalled after being picked) used to fall
    // back to the system default with zero explanation — "it didn't use it".
    if chosen != "system" && exe.is_none() {
        let m = format!("[!] Default browser '{chosen}' not found — opened with the system default instead. Reinstall it or pick another in Manage → Default Browser.\n");
        crate::base::push_log(&m, "launch");
        let _ = app.emit("launch-log", m);
    }
    match exe {
        None => match app.opener().open_url(u, None::<String>) {
            Ok(()) => serde_json::json!({"ok": true, "success": true, "via": "system"}),
            Err(e) => serde_json::json!({"ok": false, "success": false, "error": e.to_string()}),
        },
        Some(path) => {
            use tauri::Emitter;
            let m = format!("[*] Opening with {chosen}: {path}\n");
            crate::base::push_log(&m, "launch");
            let _ = app.emit("launch-log", m);
            match silent_command(&path).arg(&u).spawn() {
                Ok(_) => serde_json::json!({"ok": true, "success": true, "via": chosen}),
                Err(e) => {
                    serde_json::json!({"ok": false, "success": false, "error": e.to_string()})
                }
            }
        }
    }
}
// Resolve a known browser id to its exe (same candidates as detect_browsers).
fn find_browser_exe(id: &str) -> Option<String> {
    for c in browser_candidates(id) {
        let s = expand_win_env(c);
        if std::path::Path::new(&s).exists() {
            return Some(s);
        }
    }
    None
}
// Flags that run Chrome with GPU acceleration disabled so the browser frees
// VRAM for generation. Shared by launch_browser_no_gpu and the
// terminal-nogpu script (keep the two in sync).
const NO_GPU_CHROME_FLAGS: [&str; 7] = [
    "--disable-gpu",
    "--disable-gpu-compositing",
    "--disable-accelerated-2d-canvas",
    "--disable-accelerated-video-decode",
    "--use-angle=swiftshader",
    "--enable-unsafe-swiftshader",
    "--disable-webgpu",
];
/// Chromium-family ids share Chrome's --disable-gpu flag surface (Opera's
/// launcher.exe forwards flags to opera.exe).
fn is_chromium_browser(id: &str) -> bool {
    matches!(id, "chrome" | "edge" | "brave" | "opera" | "vivaldi")
}
/// Which browser a no-GPU launch should use: the user's selected default
/// first, then Chrome, then any installed Chromium, then Firefox (plain
/// launch — Firefox offers no CLI GPU kill switch). None when no known
/// browser is installed at all.
fn resolve_no_gpu_browser() -> Option<(String, String)> {
    let chosen = load_config_value()
        .get("defaultBrowser")
        .and_then(|v| v.as_str())
        .unwrap_or("system")
        .to_string();
    if chosen != "system" {
        if let Some(exe) = find_browser_exe(&chosen) {
            return Some((exe, chosen));
        }
        // Stale selection (uninstalled after being picked) falls through to
        // the fallbacks below instead of failing.
    }
    for id in ["chrome", "edge", "brave", "opera", "vivaldi", "firefox"] {
        if let Some(exe) = find_browser_exe(id) {
            return Some((exe, id.into()));
        }
    }
    None
}
#[tauri::command]
pub fn launch_browser_no_gpu(url: Option<String>) -> serde_json::Value {
    // No-GPU browser frees VRAM for generation (mirrors Electron's chrome flags).
    let u = url.unwrap_or_else(|| "http://localhost:7861".into());
    if !(u.starts_with("http://") || u.starts_with("https://")) {
        return serde_json::json!({"ok": false, "success": false, "error": "invalid url"});
    }
    // User's selected default first, then Chrome, then any Chromium/Firefox.
    let Some((path, id)) = resolve_no_gpu_browser() else {
        return serde_json::json!({"ok": false, "success": false, "error": "No supported browser found for no-GPU launch"});
    };
    if !is_chromium_browser(&id) {
        // Firefox has no CLI GPU kill switch (MOZ_WEBRENDER=0 is a no-op
        // upstream) — launch it plainly and say so instead of pretending.
        return match silent_command(&path).arg(&u).spawn() {
            Ok(_) => {
                serde_json::json!({"ok": true, "success": true, "via": id, "gpu_disabled": false, "note": "Firefox has no command-line GPU switch — launched normally, no VRAM savings."})
            }
            Err(e) => serde_json::json!({"ok": false, "success": false, "error": e.to_string()}),
        };
    }
    let args = NO_GPU_CHROME_FLAGS;
    match silent_command(&path).args(args).arg(&u).spawn() {
        Ok(_) => serde_json::json!({"ok": true, "success": true, "via": id}),
        Err(e) => serde_json::json!({"ok": false, "success": false, "error": e.to_string()}),
    }
}
#[tauri::command]
pub fn chrome_available() -> bool {
    // ponytail: where chrome only checks PATH, but Chrome is at Program Files — check there like detect_browsers does
    for p in [
        "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
        "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
    ] {
        if std::path::Path::new(p).exists() {
            return true;
        }
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        if std::path::Path::new(&format!("{local}\\Google\\Chrome\\Application\\chrome.exe"))
            .exists()
        {
            return true;
        }
    }
    silent_command("where")
        .arg("chrome")
        .output()
        .is_ok_and(|o| o.status.success())
}
#[tauri::command]
pub fn no_gpu_available() -> bool {
    // Any browser resolve_no_gpu_browser can open (Chromium with flags,
    // Firefox plainly) — gates the no-GPU launch buttons.
    resolve_no_gpu_browser().is_some()
}

#[tauri::command]
pub async fn launch_webview(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    launch(app, Some("app".into())).await
}
/// Pure path derivation for the launcher-pinned Triton JIT cache:
/// `<data dir>\triton-cache` (e.g. `C:\Wan2GP\triton-cache`). Triton's
/// compiled helpers (`cuda_utils`, `__triton_launcher`) are unsigned .pyd
/// files with a fresh hash every build — heuristic antivirus engines
/// (e.g. Bitdefender `Gen:Variant.Ulise`) block them mid-compile, which can
/// kill generation at the first kernel launch. Pinning the cache dir gives
/// every AV exclusion ONE known path instead of `%TEMP%` /
/// `%USERPROFILE%\.triton` roulette. Pure + unit-tested.
pub(crate) const TRITON_CACHE_SUBDIR: &str = "triton-cache";
pub(crate) fn triton_cache_dir_for(data_dir: &Path) -> PathBuf {
    data_dir.join(TRITON_CACHE_SUBDIR)
}

/// MSVC compiler pin for Triton JIT stub builds.
///
/// Background: the spawned server process carries no VsDevCmd env (no CC,
/// no INCLUDE/LIB), so Triton's `get_cc()` falls through to the
/// wheel-bundled unsigned TinyCC (`triton/runtime/tcc/tcc.exe`) — and
/// Bitdefender's behavioral engine blocked `python.exe → tcc.exe`
/// mid-generation ("unsigned spawns unsigned compiler"). Pointing CC at the
/// installed MSVC toolchain (see apply_msvc_cc_pin) dissolves that pattern:
/// a Microsoft-signed `cl.exe` building into the pinned cache dir.
///
/// NVIDIA-ONLY by design: Triton's `get_cc()` checks `CC` before TheRock's
/// own `clang-cl`, so a CC pin on AMD would shadow the ROCm compiler that
/// HIP headers require. AMD launches remove our pin instead (leaving a
/// genuine user toolchain untouched); Intel/CPU launches pin harmlessly —
/// they never JIT, and `cl.exe` is the native toolchain for any pip
/// from-source build there too.
/// Pure decision logic + hermetic discovery below; only the thin
/// `*_live` wrappers touch the real Program Files. Pure + unit-tested.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MsvcPin {
    pub cc: String,
    pub include: String,
    pub lib: String,
}

/// Current process values of the three pinned keys. All-None is the common
/// fresh-process case.
pub(crate) struct MsvcCurrent {
    pub cc: Option<String>,
    pub include: Option<String>,
    pub lib: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MsvcDecision {
    /// Set all three from fresh discovery.
    Pin,
    /// At least one key is user-owned (VsDevCmd shell, custom toolchain) —
    /// a coherent set that must never be half-overridden.
    KeepUser { keys: Vec<&'static str> },
    /// AMD launch: remove exactly the keys still holding our recorded
    /// values (a user override on any key survives).
    Unpin { cc: bool, include: bool, lib: bool },
}

pub(crate) fn decide_msvc_pin(
    is_amd: bool,
    cur: &MsvcCurrent,
    recorded: Option<&MsvcPin>,
) -> MsvcDecision {
    let matches_recorded =
        |cur_val: &Option<String>, rec_val: &str| -> bool { cur_val.as_deref() == Some(rec_val) };
    if is_amd {
        let (mut cc, mut include, mut lib) = (false, false, false);
        if let Some(r) = recorded {
            cc = matches_recorded(&cur.cc, &r.cc);
            include = matches_recorded(&cur.include, &r.include);
            lib = matches_recorded(&cur.lib, &r.lib);
        }
        return MsvcDecision::Unpin { cc, include, lib };
    }
    let user_owned = |cur_val: &Option<String>, rec_val: Option<&str>| -> bool {
        match cur_val {
            None => false,
            Some(v) => Some(v.as_str()) != rec_val,
        }
    };
    let mut keys = Vec::new();
    if user_owned(&cur.cc, recorded.map(|p| p.cc.as_str())) {
        keys.push("CC");
    }
    if user_owned(&cur.include, recorded.map(|p| p.include.as_str())) {
        keys.push("INCLUDE");
    }
    if user_owned(&cur.lib, recorded.map(|p| p.lib.as_str())) {
        keys.push("LIB");
    }
    if keys.is_empty() {
        MsvcDecision::Pin
    } else {
        MsvcDecision::KeepUser { keys }
    }
}

/// First non-empty line of `vswhere -property installationPath` output.
/// Pure + unit-tested.
pub(crate) fn parse_vswhere_root(out: &str) -> Option<PathBuf> {
    out.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(PathBuf::from)
}

/// Newest dotted-version dir name (`14.44.35207` > `14.39.35128`,
/// `10.0.22621.0` > `10.0.19041.0`) via the shared numeric compare.
/// A non-version name only wins an otherwise-empty listing — and then the
/// existence gate in `probe_toolchain_at` rejects it anyway (no cl.exe
/// under a junk dir). Pure + unit-tested.
pub(crate) fn pick_newest_versioned(names: &[String]) -> Option<String> {
    let mut best: Option<&String> = None;
    for n in names {
        let better = match best {
            None => true,
            Some(b) => version_gt(n, b),
        };
        if better {
            best = Some(n);
        }
    }
    best.cloned()
}

/// (cl.exe, INCLUDE dirs, LIB dirs) for a VS root + toolset version +
/// Windows Kits root + kit version. Pure path construction + unit-tested.
pub(crate) fn msvc_toolchain_paths(
    vs_root: &Path,
    msvc_ver: &str,
    kits_root: &Path,
    kit_ver: &str,
) -> (PathBuf, Vec<PathBuf>, Vec<PathBuf>) {
    let msvc = vs_root.join("VC").join("Tools").join("MSVC").join(msvc_ver);
    let cc = msvc.join("bin").join("Hostx64").join("x64").join("cl.exe");
    let kit_inc = kits_root.join("Include").join(kit_ver);
    let kit_lib = kits_root.join("Lib").join(kit_ver);
    let includes = vec![
        msvc.join("include"),
        kit_inc.join("ucrt"),
        kit_inc.join("um"),
        kit_inc.join("shared"),
    ];
    let libs = vec![
        msvc.join("lib").join("x64"),
        kit_lib.join("ucrt").join("x64"),
        kit_lib.join("um").join("x64"),
    ];
    (cc, includes, libs)
}

fn newest_subdir_name(dir: &Path) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;
    let names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    pick_newest_versioned(&names)
}

fn probe_toolchain_at(vs_root: &Path, kits_root: &Path) -> Option<MsvcPin> {
    let msvc_ver = newest_subdir_name(&vs_root.join("VC").join("Tools").join("MSVC"))?;
    let kit_ver = newest_subdir_name(&kits_root.join("Include"))?;
    let (cc, includes, libs) = msvc_toolchain_paths(vs_root, &msvc_ver, kits_root, &kit_ver);
    // Existence gate: cl.exe plus every INCLUDE/LIB dir must be real —
    // a half-installed toolchain (MSVC without the SDK) must fall through
    // to the next candidate, never half-pin.
    if !cc.is_file() {
        return None;
    }
    if includes.iter().chain(libs.iter()).any(|d| !d.is_dir()) {
        return None;
    }
    let join = |ds: &[PathBuf]| {
        ds.iter()
            .map(|d| d.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(";")
    };
    Some(MsvcPin {
        cc: cc.to_string_lossy().into_owned(),
        include: join(&includes),
        lib: join(&libs),
    })
}

/// Hermetic core of discovery: first (vs_root, kits_root) pair Wins.
/// Tested against fake trees; the live wrapper below supplies real roots.
pub(crate) fn discover_msvc_pin_in(
    vs_roots: &[PathBuf],
    kits_roots: &[PathBuf],
) -> Option<MsvcPin> {
    for vs in vs_roots {
        for kits in kits_roots {
            if let Some(pin) = probe_toolchain_at(vs, kits) {
                return Some(pin);
            }
        }
    }
    None
}

fn vswhere_install_root() -> Option<PathBuf> {
    let pf86 = std::env::var("ProgramFiles(x86)").ok()?;
    let vswhere = PathBuf::from(pf86)
        .join("Microsoft Visual Studio")
        .join("Installer")
        .join("vswhere.exe");
    if !vswhere.is_file() {
        return None;
    }
    let out = std::process::Command::new(&vswhere)
        .args([
            "-latest",
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-property",
            "installationPath",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_vswhere_root(&String::from_utf8_lossy(&out.stdout))
}

fn default_vs_roots() -> Vec<PathBuf> {
    let mut bases = Vec::new();
    for key in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Ok(pf) = std::env::var(key) {
            bases.push(pf);
        }
    }
    if bases.is_empty() {
        bases.push(r"C:\Program Files".to_string());
    }
    let mut roots = Vec::new();
    for pf in &bases {
        for year in ["2022", "2019"] {
            for ed in ["Enterprise", "Professional", "Community", "BuildTools"] {
                roots.push(
                    PathBuf::from(pf)
                        .join("Microsoft Visual Studio")
                        .join(year)
                        .join(ed),
                );
            }
        }
    }
    roots
}

fn default_kits_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(pf86) = std::env::var("ProgramFiles(x86)") {
        roots.push(PathBuf::from(pf86).join("Windows Kits").join("10"));
    }
    let fallback = PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10");
    if !roots.contains(&fallback) {
        roots.push(fallback);
    }
    roots
}

fn discover_msvc_pin_live() -> Option<MsvcPin> {
    let mut vs = Vec::new();
    if let Some(r) = vswhere_install_root() {
        vs.push(r);
    }
    vs.extend(default_vs_roots());
    discover_msvc_pin_in(&vs, &default_kits_roots())
}

/// Guidance logged when no MSVC toolchain exists on the box. Triton still
/// works (bundled TinyCC fallback — same AV exposure as before, nothing
/// breaks); the pin just can't apply. Points at the free fix. The text is
/// a const so the pointer it gives is unit-tested, not left to rot.
pub(crate) const NO_MSVC_TOOLCHAIN_HINT: &str = "[i] no MSVC toolchain found — Triton keeps its bundled TinyCC fallback for JIT stub builds (unsigned: more AV-prone). Install VS 2022 Build Tools with the C++ workload (free) and relaunch to get the signed-compiler pin — one-line command in the User Guide (Antivirus section).\n";

static MSVC_PIN_RECORDED: std::sync::LazyLock<std::sync::Mutex<Option<MsvcPin>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(None));

fn recorded_pin() -> Option<MsvcPin> {
    MSVC_PIN_RECORDED.lock().ok().and_then(|g| g.clone())
}

fn set_recorded_pin(pin: Option<MsvcPin>) {
    if let Ok(mut g) = MSVC_PIN_RECORDED.lock() {
        *g = pin;
    }
}

/// Apply the MSVC decision for this launch, honoring the user's Triton
/// compiler choice (Settings → Desktop, `tritonCompiler`):
/// - TinyCC mode: upstream default, no pin ever — just scrub ours.
/// - AMD launch: remove our pin so TheRock's clang-cl discovery (which `CC`
///   would shadow) stays authoritative, whatever the mode.
/// - Forced MSVC: pin, or warn loudly when no toolchain exists.
/// - Auto (default): pin when no user toolchain is present.
///
/// Warn-only throughout — discovery failure just keeps Triton's TinyCC
/// fallback. Takes effect on the NEXT launch (launch-time env).
pub(crate) fn apply_msvc_cc_pin(is_amd: bool, emit: impl Fn(&str)) {
    let cur = MsvcCurrent {
        cc: std::env::var("CC").ok(),
        include: std::env::var("INCLUDE").ok(),
        lib: std::env::var("LIB").ok(),
    };
    let recorded = recorded_pin();
    let mode = triton_compiler_mode();
    // Explicit TinyCC reuses the AMD cleanup: our MSVC values must not be
    // in this env either.
    if mode == TritonCompilerMode::TinyCc {
        let removed = remove_recorded_pin(&cur, recorded.as_ref());
        if removed.is_empty() {
            emit("[i] Triton JIT compiler: TinyCC (upstream default) per user setting — no MSVC pin\n");
        } else {
            emit(&format!(
                "[i] Triton JIT compiler: TinyCC per user setting — removed launcher-pinned {} (MSVC pin off)\n",
                removed.join(", ")
            ));
        }
        return;
    }
    if is_amd {
        let removed = remove_recorded_pin(&cur, recorded.as_ref());
        if !removed.is_empty() {
            emit(&format!(
                "[i] removed launcher-pinned MSVC compiler env ({}) for the AMD launch — TheRock clang-cl discovery stays authoritative\n",
                removed.join(", ")
            ));
        }
        return;
    }
    if mode == TritonCompilerMode::Msvc {
        match discover_msvc_pin_live() {
            Some(pin) => {
                std::env::set_var("CC", &pin.cc);
                std::env::set_var("INCLUDE", &pin.include);
                std::env::set_var("LIB", &pin.lib);
                set_recorded_pin(Some(pin.clone()));
                emit(&format!(
                    "[i] Triton JIT compiler pinned to signed MSVC ({}) — stub builds no longer use the bundled unsigned TinyCC\n",
                    pin.cc
                ));
            }
            None => emit(MSVC_FORCED_MISSING_HINT),
        }
        return;
    }
    match decide_msvc_pin(false, &cur, recorded.as_ref()) {
        // Unpin is unreachable off-AMD (decide only unpins for AMD).
        MsvcDecision::Unpin { .. } => {}
        MsvcDecision::KeepUser { keys } => {
            emit(&format!(
                "[i] CC/INCLUDE/LIB already set outside the launcher ({}) — leaving the user toolchain untouched (Triton keeps its bundled TinyCC fallback)\n",
                keys.join(", ")
            ));
        }
        MsvcDecision::Pin => match discover_msvc_pin_live() {
            Some(pin) => {
                std::env::set_var("CC", &pin.cc);
                std::env::set_var("INCLUDE", &pin.include);
                std::env::set_var("LIB", &pin.lib);
                set_recorded_pin(Some(pin.clone()));
                emit(&format!(
                    "[i] Triton JIT compiler pinned to signed MSVC ({}) — stub builds no longer use the bundled unsigned TinyCC\n",
                    pin.cc
                ));
            }
            None => emit(NO_MSVC_TOOLCHAIN_HINT),
        },
    }
}

/// Current MSVC `cl.exe` path via live discovery, if any. Thin accessor for
/// the Troubleshooting status command — the discovery itself is unit-tested.
pub(crate) fn msvc_toolchain_cc() -> Option<String> {
    discover_msvc_pin_live().map(|p| p.cc)
}

/// User-facing Triton JIT compiler choice (Settings → Desktop →
/// `tritonCompiler` in desktop-config.json). Out-of-box (unset) is pure
/// upstream TinyCC — the MSVC pin is a workaround and stays opt-in via
/// explicit "auto" (MSVC when installed) or "msvc" (forced).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TritonCompilerMode {
    Auto,
    Msvc,
    TinyCc,
}

/// Normalize free-form input to a mode: "msvc" / "tinycc" (or "tcc") win,
/// everything else (including "" and "auto") is Auto. Pure + unit-tested.
pub(crate) fn triton_compiler_normalize(s: &str) -> TritonCompilerMode {
    match s.trim().to_lowercase().as_str() {
        "msvc" => TritonCompilerMode::Msvc,
        "tinycc" | "tcc" => TritonCompilerMode::TinyCc,
        _ => TritonCompilerMode::Auto,
    }
}

/// Persisted choice. UNSET means pure upstream (TinyCC) — the cl.exe pin is
/// a workaround and must be opt-in, never default-on. An explicit "auto"
/// value (chosen in the menu) is the smart mode: MSVC when installed.
pub(crate) fn triton_compiler_mode() -> TritonCompilerMode {
    let cfg = load_config_value();
    let s = cfg.get("tritonCompiler").and_then(|v| v.as_str());
    match s {
        None => TritonCompilerMode::TinyCc,
        Some(v) => triton_compiler_normalize(v),
    }
}

/// Persist the Triton JIT compiler choice. Takes effect on the NEXT launch
/// (the pin is launch-time env) — unlike the window mode there is nothing
/// live to apply. Mirrors set_window_mode's shape ({ok, mode}).
#[tauri::command]
pub fn set_triton_compiler(mode: String) -> serde_json::Value {
    let s = match triton_compiler_normalize(&mode) {
        TritonCompilerMode::Msvc => "msvc",
        TritonCompilerMode::TinyCc => "tinycc",
        TritonCompilerMode::Auto => "auto",
    };
    let mut full = load_config_value();
    if let Some(m) = full.as_object_mut() {
        m.insert("tritonCompiler".into(), serde_json::json!(s));
    }
    if let Err(e) = crate::config::config_save(full) {
        return serde_json::json!({"ok": false, "error": e});
    }
    serde_json::json!({"ok": true, "mode": s})
}

/// Warned when tritonCompiler=msvc finds no toolchain: the fallback stays
/// TinyCC, and the message must say so plus where the fix lives. Const so
/// the pointer is unit-tested.
pub(crate) const MSVC_FORCED_MISSING_HINT: &str = "[i] tritonCompiler=msvc but no MSVC toolchain found — TinyCC fallback active. Install Build Tools (Manage → Troubleshooting) or switch the Triton compiler back to Auto.\n";

/// Remove exactly the keys still holding our recorded values. Shared by the
/// AMD cleanup and the explicit-TinyCC mode — both mean "our MSVC values
/// must not be in this env". Clears the recording when it removes anything.
pub(crate) fn remove_recorded_pin(
    cur: &MsvcCurrent,
    recorded: Option<&MsvcPin>,
) -> Vec<&'static str> {
    let mut removed = Vec::new();
    if let Some(r) = recorded {
        if cur.cc.as_deref() == Some(r.cc.as_str()) {
            std::env::remove_var("CC");
            removed.push("CC");
        }
        if cur.include.as_deref() == Some(r.include.as_str()) {
            std::env::remove_var("INCLUDE");
            removed.push("INCLUDE");
        }
        if cur.lib.as_deref() == Some(r.lib.as_str()) {
            std::env::remove_var("LIB");
            removed.push("LIB");
        }
    }
    if !removed.is_empty() {
        set_recorded_pin(None);
    }
    removed
}

#[cfg(test)]
mod launch_args_tests {
    use super::*;
    #[test]
    fn origin_matcher_keeps_only_launcher_processes() {
        let pre = "c:\\wan2gp\\";
        // Ours: interpreter or script under the repo.
        assert!(is_ours_process(
            "C:\\Wan2GP\\env_uv\\Scripts\\python.exe",
            "C:\\Wan2GP\\env_uv\\Scripts\\python.exe wgp.py --port 7861",
            pre
        ));
        // Ours: bootstrap marker (uv-managed interpreter outside repo).
        assert!(is_ours_process(
            "C:\\Users\\x\\AppData\\uv\\python.exe",
            "python.exe C:\\Temp\\wan2gp-bootstrap-9-3.py wgp.py",
            pre
        ));
        // Foreign: other software's Python, no repo signals.
        assert!(!is_ours_process(
            "C:\\Other\\soft\\python.exe",
            "C:\\Other\\soft\\python.exe analyzer.py --port 7861",
            pre
        ));
        // Foreign even WITH wgp.py in argv when it is not under our repo
        // and has no bootstrap marker (e.g. someone else's checkout).
        assert!(!is_ours_process(
            "D:\\Stuff\\python.exe",
            "D:\\Stuff\\python.exe D:\\Stuff\\wgp.py",
            pre
        ));
        // No wgp.py at all: never ours.
        assert!(!is_ours_process(
            "C:\\Wan2GP\\env_uv\\Scripts\\python.exe",
            "python.exe -m pip list",
            pre
        ));
    }
    #[test]
    fn triton_cache_pin_targets_one_stable_folder() {
        use super::triton_cache_dir_for;
        let d = triton_cache_dir_for(Path::new(r"C:\Wan2GP"));
        assert_eq!(d, Path::new(r"C:\Wan2GP").join("triton-cache"));
        // Non-default data dir (D: pick) resolves inside it, never HOME.
        let d2 = triton_cache_dir_for(Path::new(r"D:\Tools\Wan2GP"));
        assert_eq!(d2, Path::new(r"D:\Tools\Wan2GP").join("triton-cache"));
        assert!(!d2.to_string_lossy().contains("AppData\\Local\\Temp"));
    }
    #[test]
    fn close_sweep_guard_skips_zero_and_self() {
        // The final close sweep kills ANY owner on Wan2GP ports — the
        // only two PIDs it must never touch are 0 and ourselves.
        assert!(!close_sweep_should_kill(0));
        assert!(!close_sweep_should_kill(std::process::id()));
        assert!(close_sweep_should_kill(123456));
    }
    #[test]
    fn terminal_command_runs_bootstrap_first_no_dup_script() {
        // Regression: the .bat once ran `py -u wgp.py <boot> wgp.py …`, and
        // wgp.py's argparse rejected `<boot> wgp.py` as unrecognized args.
        let args = vec![
            "C:\\Temp\\wan2gp-bootstrap-1-2.py".to_string(),
            "wgp.py".to_string(),
            "--server-port".to_string(),
            "7861".to_string(),
        ];
        let cmd = terminal_server_command("C:\\Wan2GP\\env_uv\\Scripts\\python.exe", &args);
        assert_eq!(
cmd,
"\"C:\\Wan2GP\\env_uv\\Scripts\\python.exe\" -u C:\\Temp\\wan2gp-bootstrap-1-2.py wgp.py --server-port 7861"
);
        assert_eq!(
            cmd.matches("wgp.py").count(),
            1,
            "script name must appear exactly once"
        );
    }
    #[test]
    fn split_launch_args_quotes() {
        assert_eq!(
            split_launch_args("--profile 4 --attention sage2"),
            vec!["--profile", "4", "--attention", "sage2"]
        );
        assert_eq!(
            split_launch_args("--config \"a b\" --verbose 2"),
            vec!["--config", "a b", "--verbose", "2"]
        );
        assert_eq!(split_launch_args(""), Vec::<String>::new());
    }
    #[test]
    fn removed_launch_flags_are_dropped_with_their_value() {
        // Stale preset from an older build: argparse would reject the token.
        let (kept, removed) = strip_removed_launch_flags(split_launch_args(
            "--compile --attention sage2 --profile 3 --teacache 2.0",
        ));
        assert_eq!(
            kept,
            vec!["--compile", "--attention", "sage2", "--profile", "3"]
        );
        assert_eq!(removed, vec!["--teacache", "2.0"]);
        // `--flag=value` goes whole; a bare flag at the end can't eat the next arg.
        let (kept, removed) =
            strip_removed_launch_flags(split_launch_args("--teacache=1.5 --fp16"));
        assert_eq!(kept, vec!["--fp16"]);
        assert_eq!(removed, vec!["--teacache=1.5"]);
        let (kept, _) = strip_removed_launch_flags(split_launch_args("--teacache"));
        assert!(kept.is_empty());
        // A value that looks like a flag is never swallowed.
        let (kept, removed) =
            strip_removed_launch_flags(split_launch_args("--teacache --profile 4"));
        assert_eq!(kept, vec!["--profile", "4"]);
        assert_eq!(removed, vec!["--teacache"]);
        // Nothing else is touched.
        let (kept, removed) =
            strip_removed_launch_flags(split_launch_args("--attention sdpa --fp16"));
        assert_eq!(kept, vec!["--attention", "sdpa", "--fp16"]);
        assert!(removed.is_empty());
    }
    #[test]
    fn path_strip_removes_only_recorded_prepend() {
        // Exact recorded prefix (case-insensitive) is stripped; user PATH survives.
        assert_eq!(
            strip_own_path_prepend("C:\\rocm\\bin;C:\\x", "C:\\rocm\\bin"),
            "C:\\x"
        );
        assert_eq!(
            strip_own_path_prepend("c:\\ROCM\\bin;C:\\x", "C:\\rocm\\bin"),
            "C:\\x"
        );
        // Multi-dir prepend (llvm bin + sdk bin joined by ';') strips as one block.
        assert_eq!(strip_own_path_prepend("A;B;C:\\x", "A;B"), "C:\\x");
        // Not our prepend (middle of PATH, partial match) → untouched.
        assert_eq!(
            strip_own_path_prepend("C:\\x;C:\\rocm\\bin", "C:\\rocm\\bin"),
            "C:\\x;C:\\rocm\\bin"
        );
        assert_eq!(
            strip_own_path_prepend("C:\\rocm\\bin-extra;C:\\x", "C:\\rocm\\bin"),
            "C:\\rocm\\bin-extra;C:\\x"
        );
        // Empty inputs are no-ops.
        assert_eq!(strip_own_path_prepend("C:\\x", ""), "C:\\x");
        assert_eq!(strip_own_path_prepend("", "C:\\rocm\\bin"), "");
    }
    #[test]
    fn non_amd_reconcile_clears_stale_vars() {
        // Stale AMD compiler/ROCm vars from an earlier AMD launch in this
        // process must be gone after the non-AMD reconcile; HSA keys are NOT
        // this function's business (handled separately above).
        let saved_path = std::env::var("PATH").unwrap_or_default();
        let saved: Vec<(String, Option<String>)> = NON_AMD_STALE_ENV_KEYS
            .iter()
            .map(|k| (k.to_string(), std::env::var(k).ok()))
            .collect();
        for k in NON_AMD_STALE_ENV_KEYS {
            std::env::set_var(k, "stale");
        }
        std::env::set_var("HSA_OVERRIDE_GFX_VERSION", "11.0.0");
        let mut logged: Vec<String> = Vec::new();
        reconcile_non_amd_session_env(|m: &str| logged.push(m.to_string()));
        for k in NON_AMD_STALE_ENV_KEYS {
            assert!(std::env::var(k).is_err(), "{k} must be removed");
        }
        // HSA override untouched by this reconcile.
        assert_eq!(
            std::env::var("HSA_OVERRIDE_GFX_VERSION").as_deref(),
            Ok("11.0.0")
        );
        // One [i] line per removed key.
        assert_eq!(logged.len(), NON_AMD_STALE_ENV_KEYS.len());
        assert!(logged.iter().all(|l| l.starts_with("[i]")));
        // Restore.
        std::env::remove_var("HSA_OVERRIDE_GFX_VERSION");
        for (k, v) in saved {
            match v {
                Some(val) => std::env::set_var(&k, val),
                None => std::env::remove_var(&k),
            }
        }
        std::env::set_var("PATH", saved_path);
    }
    #[test]
    fn hip_pin_strict_conditions_only() {
        // Reporter shape: one discrete AMD card + virtual monitor → pin to 0.
        assert_eq!(hip_pin_value(1, 0, 1), Some("0".to_string()));
        assert_eq!(hip_pin_value(1, 0, 3), Some("0".to_string()));
        // iGPU (APU) + discrete: discrete sits after the APU in HIP order.
        assert_eq!(hip_pin_value(1, 1, 0), Some("1".to_string()));
        assert_eq!(hip_pin_value(1, 2, 0), Some("2".to_string()));
        // Multi-discrete (guess), no discrete, single card (unambiguous) →
        // do nothing.
        assert_eq!(hip_pin_value(2, 1, 0), None);
        assert_eq!(hip_pin_value(0, 1, 0), None);
        assert_eq!(hip_pin_value(1, 0, 0), None);
        assert_eq!(hip_pin_value(0, 0, 0), None);
    }
    #[test]
    fn miopen_disabled_unsets() {
        // Default (false) keeps the FAST set-if-absent; true leaves the var
        // fully unset (never an empty string — MIOpen reads absence).
        assert_eq!(miopen_find_mode_value(false), Some("FAST"));
        assert_eq!(miopen_find_mode_value(true), None);
    }
    #[test]
    fn rocm_sdk_root_parses() {
        // First plausible absolute-dir line wins; junk/relative → None.
        let d = std::env::temp_dir().join(format!("wgp-rocm-probe-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        let ds = d.to_string_lossy().to_string();
        assert_eq!(parse_rocm_sdk_root(&format!("{ds}\n")), Some(d.clone()));
        assert_eq!(
            parse_rocm_sdk_root(&format!("rocm-sdk 1.2.3\n{ds}\n")),
            Some(d.clone())
        );
        assert_eq!(parse_rocm_sdk_root(""), None);
        assert_eq!(parse_rocm_sdk_root("not-a-path\nrelative/dir\n"), None);
        // Points nowhere on disk → None (never invent paths).
        assert_eq!(
            parse_rocm_sdk_root("C:\\definitely-not-here-wgp\\rocm\n"),
            None
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(all(test, windows))]
mod msvc_pin_tests {
    use super::{
        decide_msvc_pin, discover_msvc_pin_in, msvc_toolchain_paths, parse_vswhere_root,
        pick_newest_versioned, remove_recorded_pin, triton_compiler_normalize, MsvcCurrent,
        MsvcDecision, MsvcPin, TritonCompilerMode, MSVC_FORCED_MISSING_HINT,
        NO_MSVC_TOOLCHAIN_HINT,
    };

    fn rec_pin() -> MsvcPin {
        MsvcPin {
            cc: r"C:\VS\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64\cl.exe".to_string(),
            include: r"C:\VS\inc".to_string(),
            lib: r"C:\VS\lib".to_string(),
        }
    }

    fn cur_of(r: &MsvcPin) -> MsvcCurrent {
        MsvcCurrent {
            cc: Some(r.cc.clone()),
            include: Some(r.include.clone()),
            lib: Some(r.lib.clone()),
        }
    }

    #[test]
    fn compiler_mode_normalizes_user_input() {
        use TritonCompilerMode::*;
        assert_eq!(triton_compiler_normalize("msvc"), Msvc);
        assert_eq!(triton_compiler_normalize("MSVC"), Msvc);
        assert_eq!(triton_compiler_normalize("tinycc"), TinyCc);
        assert_eq!(triton_compiler_normalize(" tcc "), TinyCc);
        for auto in ["", "auto", "AUTO", "clang", "gcc"] {
            assert_eq!(triton_compiler_normalize(auto), Auto, "{auto}");
        }
    }

    #[test]
    fn forced_msvc_hint_names_the_way_out() {
        assert!(MSVC_FORCED_MISSING_HINT.contains("TinyCC fallback"));
        assert!(MSVC_FORCED_MISSING_HINT.contains("Troubleshooting"));
    }

    #[test]
    fn recorded_pin_removal_spares_user_keys() {
        // Our CC/INCLUDE go, a user LIB survives — then everything restored.
        let saved: Vec<(String, Option<String>)> = ["CC", "INCLUDE", "LIB"]
            .iter()
            .map(|k| (k.to_string(), std::env::var(k).ok()))
            .collect();
        let r = rec_pin();
        std::env::set_var("CC", &r.cc);
        std::env::set_var("INCLUDE", &r.include);
        std::env::set_var("LIB", r"C:\user\lib");
        let cur = MsvcCurrent {
            cc: std::env::var("CC").ok(),
            include: std::env::var("INCLUDE").ok(),
            lib: std::env::var("LIB").ok(),
        };
        let removed = remove_recorded_pin(&cur, Some(&r));
        assert_eq!(removed, vec!["CC", "INCLUDE"]);
        assert_eq!(std::env::var("LIB").as_deref(), Ok(r"C:\user\lib"));
        for (k, v) in saved {
            match v {
                Some(val) => std::env::set_var(&k, val),
                None => std::env::remove_var(&k),
            }
        }
    }

    #[test]
    fn no_msvc_hint_points_at_the_free_fix() {
        // The whole point of the None branch is telling a MSVC-less user how
        // to get the pin: name the fallback truthfully and point at Build Tools.
        assert!(NO_MSVC_TOOLCHAIN_HINT.contains("TinyCC fallback"));
        assert!(NO_MSVC_TOOLCHAIN_HINT.contains("Build Tools"));
        assert!(NO_MSVC_TOOLCHAIN_HINT.contains("User Guide"));
    }

    #[test]
    fn vswhere_parse_takes_first_non_empty_line() {
        assert_eq!(
            parse_vswhere_root("C:\\VS\\2022\\Community\r\n"),
            Some(std::path::PathBuf::from("C:\\VS\\2022\\Community"))
        );
        assert_eq!(
            parse_vswhere_root("\n  \nC:\\VS\\2022\\BuildTools\nC:\\VS\\2019\\Community\n"),
            Some(std::path::PathBuf::from("C:\\VS\\2022\\BuildTools"))
        );
        assert_eq!(parse_vswhere_root(""), None);
        assert_eq!(parse_vswhere_root("   \n\t\n"), None);
    }

    #[test]
    fn newest_version_picks_numeric_max() {
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        // MSVC toolsets.
        assert_eq!(
            pick_newest_versioned(&v(&["14.39.35128", "14.44.35207"])),
            Some("14.44.35207".to_string())
        );
        // Windows Kits.
        assert_eq!(
            pick_newest_versioned(&v(&["10.0.19041.0", "10.0.22621.0"])),
            Some("10.0.22621.0".to_string())
        );
        // A non-version name never beats a real version (it parses as 0).
        assert_eq!(
            pick_newest_versioned(&v(&["Current", "10.0.19041.0"])),
            Some("10.0.19041.0".to_string())
        );
        assert_eq!(pick_newest_versioned(&[]), None);
    }

    #[test]
    fn toolchain_paths_point_at_hostx64_cl_and_sdk_layout() {
        let (cc, inc, lib) = msvc_toolchain_paths(
            std::path::Path::new(r"C:\VS"),
            "14.44.35207",
            std::path::Path::new(r"C:\Kits\10"),
            "10.0.22621.0",
        );
        assert_eq!(
            cc,
            std::path::PathBuf::from(r"C:\VS\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64\cl.exe")
        );
        assert!(inc.iter().any(|d| d.ends_with(r"14.44.35207\include")));
        assert!(inc.iter().any(|d| d.ends_with(r"10.0.22621.0\ucrt")));
        assert!(inc.iter().any(|d| d.ends_with(r"10.0.22621.0\um")));
        assert!(lib.iter().any(|d| d.ends_with(r"14.44.35207\lib\x64")));
        assert!(lib.iter().any(|d| d.ends_with(r"10.0.22621.0\ucrt\x64")));
    }

    /// Build a fake (VS root, Kits root) pair under `base`.
    fn fake_tree(
        base: &std::path::Path,
        msvc_vers: &[&str],
        kit_vers: &[&str],
        with_cl: bool,
    ) -> (std::path::PathBuf, std::path::PathBuf) {
        let vs = base.join("VS");
        for v in msvc_vers {
            let m = vs.join("VC").join("Tools").join("MSVC").join(v);
            std::fs::create_dir_all(m.join("include")).unwrap();
            std::fs::create_dir_all(m.join("lib").join("x64")).unwrap();
            if with_cl {
                let cldir = m.join("bin").join("Hostx64").join("x64");
                std::fs::create_dir_all(&cldir).unwrap();
                std::fs::File::create(cldir.join("cl.exe")).unwrap();
            }
        }
        let kits = base.join("Kits");
        for k in kit_vers {
            for d in ["ucrt", "um", "shared"] {
                std::fs::create_dir_all(kits.join("Include").join(k).join(d)).unwrap();
            }
            for d in ["ucrt", "um"] {
                std::fs::create_dir_all(kits.join("Lib").join(k).join(d).join("x64")).unwrap();
            }
        }
        (vs, kits)
    }

    #[test]
    fn discovery_finds_newest_pair_in_fake_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let (vs, kits) = fake_tree(
            tmp.path(),
            &["14.39.35128", "14.44.35207"],
            &["10.0.19041.0", "10.0.22621.0"],
            true,
        );
        let pin = discover_msvc_pin_in(std::slice::from_ref(&vs), std::slice::from_ref(&kits))
            .expect("fake toolchain must be found");
        assert!(pin.cc.contains("14.44.35207"), "{}", pin.cc);
        assert!(pin.cc.ends_with("cl.exe"), "{}", pin.cc);
        assert!(pin.include.contains("10.0.22621.0"), "{}", pin.include);
        assert!(pin.lib.contains("10.0.22621.0"), "{}", pin.lib);
    }

    #[test]
    fn discovery_rejects_half_installed_toolchains() {
        let tmp = tempfile::tempdir().unwrap();
        // MSVC without cl.exe → None.
        let (vs_nocl, kits) = fake_tree(
            &tmp.path().join("a"),
            &["14.44.35207"],
            &["10.0.22621.0"],
            false,
        );
        assert!(
            discover_msvc_pin_in(std::slice::from_ref(&vs_nocl), std::slice::from_ref(&kits))
                .is_none()
        );
        // MSVC without any SDK → None (cl.exe alone can't compile Python.h).
        let (vs, _) = fake_tree(&tmp.path().join("b"), &["14.44.35207"], &[], true);
        let empty_kits = tmp.path().join("b-empty-kits");
        std::fs::create_dir_all(&empty_kits).unwrap();
        assert!(
            discover_msvc_pin_in(std::slice::from_ref(&vs), std::slice::from_ref(&empty_kits))
                .is_none()
        );
    }

    #[test]
    fn decision_pins_on_clean_env_and_repin_is_idempotent() {
        let clean = MsvcCurrent {
            cc: None,
            include: None,
            lib: None,
        };
        assert_eq!(decide_msvc_pin(false, &clean, None), MsvcDecision::Pin);
        // Second launch in the same process still holding our values → Pin
        // again (harmless overwrite), never KeepUser against ourselves.
        let r = rec_pin();
        assert_eq!(
            decide_msvc_pin(false, &cur_of(&r), Some(&r)),
            MsvcDecision::Pin
        );
    }

    #[test]
    fn decision_never_half_overrides_a_user_toolchain() {
        // VsDevCmd-style env, unknown to us → keep all of it.
        let user = MsvcCurrent {
            cc: Some(r"C:\Other\cl.exe".to_string()),
            include: None,
            lib: None,
        };
        assert_eq!(
            decide_msvc_pin(false, &user, None),
            MsvcDecision::KeepUser { keys: vec!["CC"] }
        );
        // User overrode CC after our pin, INCLUDE/LIB still ours → only CC
        // is reported; nothing is touched.
        let r = rec_pin();
        let mixed = MsvcCurrent {
            cc: Some(r"C:\Other\cl.exe".to_string()),
            include: Some(r.include.clone()),
            lib: Some(r.lib.clone()),
        };
        assert_eq!(
            decide_msvc_pin(false, &mixed, Some(&r)),
            MsvcDecision::KeepUser { keys: vec!["CC"] }
        );
    }

    #[test]
    fn decision_amd_unpins_recorded_keys_only() {
        let r = rec_pin();
        // Full match → remove all three so TheRock clang-cl discovery wins
        // (CC env would shadow it in Triton's get_cc).
        assert_eq!(
            decide_msvc_pin(true, &cur_of(&r), Some(&r)),
            MsvcDecision::Unpin {
                cc: true,
                include: true,
                lib: true
            }
        );
        // User CC override survives the AMD cleanup; ours still go.
        let mixed = MsvcCurrent {
            cc: Some(r"C:\Other\cl.exe".to_string()),
            include: Some(r.include.clone()),
            lib: Some(r.lib.clone()),
        };
        assert_eq!(
            decide_msvc_pin(true, &mixed, Some(&r)),
            MsvcDecision::Unpin {
                cc: false,
                include: true,
                lib: true
            }
        );
        // Nothing recorded → nothing to remove.
        let clean = MsvcCurrent {
            cc: None,
            include: None,
            lib: None,
        };
        assert_eq!(
            decide_msvc_pin(true, &clean, None),
            MsvcDecision::Unpin {
                cc: false,
                include: false,
                lib: false
            }
        );
    }
}

/// F6: validate headless-batch inputs. Pure: returns the argv tail
/// (`-u wgp.py --process …`) or a user-facing error.
pub(crate) fn batch_argv(
    queue: &str,
    output_dir: Option<&str>,
    dry_run: bool,
) -> Result<Vec<String>, String> {
    let q = std::path::Path::new(queue);
    if !q.is_absolute() {
        return Err("Queue path must be absolute".into());
    }
    if !q.is_file() {
        return Err("Queue file not found".into());
    }
    match q
        .extension()
        .and_then(|x| x.to_str())
        .map(|x| x.to_lowercase())
        .as_deref()
    {
        Some("zip") | Some("json") => {}
        _ => return Err("Queue must be a .zip (saved queue) or .json (settings)".into()),
    }
    let mut args = vec![
        "-u".to_string(),
        "wgp.py".to_string(),
        "--process".to_string(),
        queue.to_string(),
    ];
    if dry_run {
        args.push("--dry-run".to_string());
    }
    if let Some(d) = output_dir.map(str::trim).filter(|d| !d.is_empty()) {
        args.push("--output-dir".to_string());
        args.push(d.to_string());
    }
    Ok(args)
}

fn batch_python() -> Result<String, String> {
    let env = get_active_env();
    let raw = env.get("path").and_then(|p| p.as_str()).ok_or_else(|| {
        "No Python environment is installed — finish or repair the install first".to_string()
    })?;
    let rel = raw.trim_start_matches(".\\").trim_start_matches("./");
    let base = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        get_repo_dir().join(rel)
    };
    let legacy = if cfg!(windows) {
        base.join("Scripts\\python.exe")
    } else {
        base.join("bin/python3")
    };
    let py = resolve_env_python(&get_repo_dir(), raw)
        .unwrap_or(legacy)
        .to_string_lossy()
        .to_string();
    let torch_ok = silent_command(&py)
        .args(["-c", "import torch"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !torch_ok {
        return Err(
            "The environment's Python can't import torch — repair the environment first".into(),
        );
    }
    Ok(py)
}

/// F6: run a saved queue headless (`wgp.py --process …`), streaming lines to
/// the console log bus. Returns the process exit code (0 = all tasks done,
/// 1 = error, 130 = interrupted).
#[tauri::command]
pub async fn batch_process(
    app: tauri::AppHandle,
    queue: String,
    output_dir: Option<String>,
    dry_run: bool,
) -> Result<serde_json::Value, String> {
    use tauri::Emitter;
    let repo = get_repo_dir();
    if !repo.join("wgp.py").exists() {
        return Err("Wan2GP not installed — run Install first".into());
    }
    let argv = batch_argv(&queue, output_dir.as_deref(), dry_run)?;
    let py = batch_python()?;
    mutating_try("batch")?;
    let app2 = app.clone();
    let emit = |msg: &str| {
        crate::base::push_log(msg, "launch");
        let _ = app.emit("launch-log", msg.to_string());
    };
    emit(&format!(
        "[*] Batch {} {queue}{}\n",
        if dry_run { "validating" } else { "processing" },
        output_dir
            .as_deref()
            .filter(|d| !d.trim().is_empty())
            .map(|d| format!(" → {d}"))
            .unwrap_or_default()
    ));
    let code = tauri::async_runtime::spawn_blocking(move || {
        let mut cmd = std::process::Command::new(&py);
        crate::base::sanitize(&mut cmd);
        let mut child = cmd
            .args(&argv)
            .current_dir(&repo)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Could not start batch ({e})"))?;
        if let Some(out) = child.stdout.take() {
            for line in std::io::BufRead::lines(std::io::BufReader::new(out)).map_while(Result::ok)
            {
                let msg = format!("[batch] {line}\n");
                crate::base::push_log(&msg, "launch");
                let _ = app2.emit("launch-log", msg);
            }
        }
        let mut err_tail = String::new();
        if let Some(err) = child.stderr.take() {
            use std::io::Read;
            let mut buf = String::new();
            if std::io::BufReader::new(err)
                .read_to_string(&mut buf)
                .is_ok()
            {
                let t = buf.trim();
                if t.len() > 2000 {
                    err_tail = format!("…{}", &t[t.len() - 2000..]);
                } else {
                    err_tail = t.to_string();
                }
            }
        }
        let code = child.wait().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
        if code != 0 && !err_tail.is_empty() {
            let msg = format!("[batch:stderr] {err_tail}\n");
            crate::base::push_log(&msg, "launch");
            let _ = app2.emit("launch-log", msg);
        }
        Ok::<i32, String>(code)
    })
    .await
    .map_err(|e| format!("Batch task failed ({e})"))??;
    mutating_done();
    emit(&format!(
        "[*] Batch finished with exit code {code} (0 = done, 1 = error, 130 = interrupted)\n"
    ));
    Ok(serde_json::json!({"ok": code == 0, "code": code}))
}

#[cfg(test)]
mod batch_tests {
    use super::batch_argv;

    fn fixture(ext: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("wgp-batch-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        let p = d.join(format!("queue.{ext}"));
        let _ = std::fs::write(&p, b"{}");
        p
    }

    #[test]
    fn accepts_zip_and_json_shapes_argv() {
        let z = fixture("zip");
        let argv = batch_argv(z.to_str().unwrap(), Some("C:\\out"), false).unwrap();
        assert_eq!(
            argv,
            vec![
                "-u",
                "wgp.py",
                "--process",
                z.to_str().unwrap(),
                "--output-dir",
                "C:\\out"
            ]
        );
        let j = fixture("json");
        let argv = batch_argv(j.to_str().unwrap(), None, true).unwrap();
        assert!(argv.contains(&"--dry-run".to_string()));
        assert!(!argv.iter().any(|a| a == "--output-dir"));
    }

    #[test]
    fn rejects_relative_missing_and_wrong_extension() {
        assert!(batch_argv("relative/queue.zip", None, false).is_err());
        #[cfg(windows)]
        let exe = "C:\\Windows\\System32\\notepad.exe";
        #[cfg(not(windows))]
        let exe = "/bin/sh";
        assert!(batch_argv(exe, None, false).unwrap_err().contains(".zip"));
        assert!(batch_argv(env!("CARGO_MANIFEST_DIR"), None, false).is_err());
    }
}

#[cfg(test)]
mod browser_candidate_tests {
    use super::browser_candidates;
    #[test]
    fn candidates_match_the_host_os() {
        let chrome = browser_candidates("chrome");
        let firefox = browser_candidates("firefox");
        assert!(!chrome.is_empty() && !firefox.is_empty());
        if cfg!(windows) {
            assert!(chrome[0].ends_with("chrome.exe"), "{chrome:?}");
        } else if cfg!(target_os = "macos") {
            assert!(chrome[0].contains(".app/Contents/MacOS/"), "{chrome:?}");
        } else {
            // The No-GPU probe never found a browser on Linux while the table
            // was Windows-only, so the launch buttons stayed disabled.
            assert!(chrome.contains(&"/usr/bin/google-chrome"), "{chrome:?}");
            assert!(chrome.contains(&"/usr/bin/chromium"), "{chrome:?}");
            assert!(firefox.contains(&"/usr/bin/firefox"), "{firefox:?}");
        }
    }
    #[test]
    fn unknown_browser_has_no_candidates() {
        assert!(browser_candidates("netscape").is_empty());
    }
}

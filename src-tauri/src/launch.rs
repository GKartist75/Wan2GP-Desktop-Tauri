//! Wan2GP server lifecycle (launch/stop/browser modes).
use crate::base::*;
use crate::{
    hw::{
        get_gpu_info_sync, kernel_profile_key, probe_command, wmi_all_gpus, wmi_gpu_fallback,
        wmi_virtual_adapters,
    },
    status::{get_active_env, resolve_env_python},
};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::Emitter;

// Quote-aware split for Extra Launch Args (keeps "--teacache \"a b\"" together).
fn split_launch_args(s: &str) -> Vec<String> {
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
static TERMINAL_TITLE: std::sync::OnceLock<std::sync::Mutex<Option<String>>> =
    std::sync::OnceLock::new();
pub(crate) fn terminal_title() -> Option<String> {
    TERMINAL_TITLE
        .get()
        .and_then(|m| m.lock().ok())
        .and_then(|g| g.clone())
}
// AMD ROCm PATH prepend tracking: the AMD launch env prepends the ROCm SDK
// bin dirs to PATH in this process. On a later non-AMD launch in the same
// process that prepend is stale (and the compiler vars below are worse), so
// the non-AMD reconcile strips exactly what was prepended. Recorded once per
// prepend; HSA override handling is untouched.
static AMD_PATH_PREPEND: std::sync::OnceLock<std::sync::Mutex<Option<String>>> =
    std::sync::OnceLock::new();
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
        .get()
        .and_then(|m| m.lock().ok())
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
/// discrete AMD GPU PLUS ignored virtual display adapter(s) (e.g. a Meta
/// Virtual Monitor next to one discrete card) → pin HIP to device 0.
/// Multi-AMD boxes (the right index would be a guess) and
/// no-virtual-adapter boxes → None (do nothing). Pure + unit-tested.
pub(crate) fn hip_pin_value(discrete_amd: usize, virtual_adapters: usize) -> Option<&'static str> {
    if discrete_amd == 1 && virtual_adapters > 0 {
        Some("0")
    } else {
        None
    }
}
/// MIOpen mode for AMD launch from the Manage-backed `amdEnv` config key
/// (`amdEnv.miopenDisabled`, default false — backend-only for now, the
/// frontend can bind the key later). Some(_) → set-if-absent as today;
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
// External-terminal mode (run.bat style): generate a script that runs wgp.py
// with the same args/env, open it in a VISIBLE console window, wait for the
// server, open the browser. Not a streamed child — the user owns the window.
fn launch_in_terminal(
    app: tauri::AppHandle,
    repo: &PathBuf,
    py: &str,
    args: &[String],
    port: u64,
    _cfg: &serde_json::Value,
    hf_token: String,
    claude_key: String,
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
    if let Ok(mut g) = TERMINAL_TITLE
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
    {
        *g = Some(title.clone());
    }
    // env lines for the script (tokens + GGUF knobs, same as hidden launch)
    let mut env_lines: Vec<String> = vec![
        "set PYTHONIOENCODING=utf-8".into(),
        "set PYTHONUTF8=1".into(),
        "set PYTHONUNBUFFERED=1".into(),
        "set TQDM_MININTERVAL=0".into(),
        "set TQDM_MINITERS=1".into(),
        "set NO_PROXY=localhost,127.0.0.1,::1".into(),
    ];
    if !hf_token.is_empty() {
        env_lines.push(format!("set HF_TOKEN={hf_token}"));
    }
    if !claude_key.is_empty() {
        env_lines.push(format!("set ANTHROPIC_API_KEY={claude_key}"));
    }
    for k in [
        "WGP_GGUF_LLAMACPP_CUDA",
        "WGP_GGUF_LLAMACPP_CUDA_MATMUL_MODE",
        "WGP_GGUF_LLAMACPP_CUDA_STREAM_K",
        "WGP_GGUF_LLAMACPP_CUDA_BF16_FP16",
    ] {
        if let Ok(val) = std::env::var(k) {
            env_lines.push(format!("set {k}={val}"));
        }
    }
    let arg_str = args
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
    #[cfg(windows)]
    {
        // Unique per launch: the old fixed wan2gp-terminal.bat raced when
        // two launches overlapped (second overwrote the first's script).
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let script = std::env::temp_dir().join(format!("wan2gp-terminal-{millis}.bat"));
        let url = format!("http://localhost:{port}");
        let full = format!("@echo off\r\ntitle {title}\r\ncd /d \"{repo}\"\r\n{envs}\r\necho [Wan2GP Desktop Launcher] Starting on port {port}...\r\nstart /b \"\" cmd /c \"\"{py}\" -u wgp.py {arg_str}\" 2>&1\r\necho Waiting for server on port {port}...\r\nset RC=0\r\n:waitloop\r\ntimeout /t 2 /nobreak >nul\r\nset /a RC+=1\r\nif %RC% gtr 60 (echo Server failed to start. Check console. ^& pause ^& exit /b 1)\r\npowershell -Command \"try{{$(Invoke-WebRequest -Uri http://127.0.0.1:{port}/config -TimeoutSec 2 -UseBasicParsing).StatusCode -eq 200;exit 0}}catch{{exit 1}}\" >nul 2>&1 && goto ready\r\ngoto waitloop\r\n:ready\r\necho Wan2GP is ready! Opening browser...\r\nstart {url}\r\necho [Wan2GP] Server running. Close this window to stop it.\r\npause >nul\r\n",
            repo = repo.display(), envs = env_lines.join("\r\n"));
        std::fs::write(&script, full).map_err(|e| {
            mutating_done();
            e.to_string()
        })?;
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
            std::process::Command::new("cmd.exe")
                .args([
                    "/C",
                    "start",
                    &title,
                    "cmd",
                    "/K",
                    script.to_string_lossy().as_ref(),
                ])
                .spawn()
        };
        if let Err(e) = spawned {
            mutating_done();
            return Err(format!("Could not open terminal: {e}"));
        }
        mutating_done();
        Ok(
            serde_json::json!({"ok": true, "port": port, "mode": "terminal", "url": url, "fresh": true}),
        )
    }
    #[cfg(not(windows))]
    {
        let _ = (cfg, hf_token, claude_key);
        mutating_done();
        return Err("External terminal mode is Windows-only in this build".into());
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
    // build args — gpuDevice -> --gpu (mirrors Electron buildCommonLaunchArgs)
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
    if share {
        args.push("--share".into());
    }
    if gpu_device != "auto"
        && gpu_device.starts_with("cuda:")
        && !args.contains(&"--gpu".to_string())
    {
        args.push("--gpu".into());
        args.push(gpu_device.clone());
    }
    // Extra Launch Args from Manage tab (quote-aware split, appended last so they win).
    if let Some(extra) = cfg.get("launchArgs").and_then(|v| v.as_str()) {
        let add = split_launch_args(extra);
        if !add.is_empty() {
            args.extend(add);
        }
    }
    let emit = |msg: &str| {
        crate::base::push_log(msg, "launch");
        let _ = app.emit("launch-log", msg.to_string());
    };
    emit(&format!("[*] Launching Wan2GP ({mode}) on :{port}…\n"));
    // GPU assignment log (mirrors Electron 9945990)
    {
        let hw = get_gpu_info_sync();
        let hw_name = hw.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let hw_vendor = hw.get("vendor").and_then(|v| v.as_str()).unwrap_or("?");
        let hw_vram = hw.get("vramMB").and_then(|v| v.as_str()).unwrap_or("0");
        let gpu_count = probe_command("NVIDIA_SMI", "nvidia-smi")
            .args(["--query-gpu=index", "--format=csv,noheader"])
            .output()
            .ok()
            .and_then(|o| {
                o.status
                    .success()
                    .then(|| {
                        String::from_utf8_lossy(&o.stdout)
                            .lines()
                            .filter(|l| !l.trim().is_empty())
                            .count()
                    })
                    .filter(|&c| c > 0)
            })
            .map(|c| format!("{c} NVIDIA"))
            // AMD/Intel-only box: nvidia-smi absent — WMI name instead of "?".
            .or_else(|| wmi_gpu_fallback().map(|(_, v, _, _)| format!("1 {v}")))
            .unwrap_or("?".into());
        let gen_label = if gpu_device == "auto" {
            format!("auto ({hw_name} )")
        } else {
            gpu_device.clone()
        };
        emit(&format!("[*] GPU assignment — Launcher UI: {launcher_gpu} | Generation: {gen_label} | HW: {hw_name} ({hw_vendor}, {hw_vram}) | Detected: {gpu_count}\n"));
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
            if n.starts_with("wan2gp-bootstrap-") && n.ends_with(".py") {
                let _ = std::fs::remove_file(stale.path());
            }
        }
        tmp.join(format!(
            "wan2gp-bootstrap-{}-{}.py",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        ))
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
    use tauri_plugin_shell::ShellExt;
    // ponytail: PYTHONUNBUFFERED for streaming logs (tqdm), plus HF_TOKEN/claude key
    let mut cmd = app.shell().command(&py);
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
    if !claude_key.is_empty() {
        std::env::set_var("ANTHROPIC_API_KEY", &claude_key);
    }
    // GGUF CUDA kernel knobs from Manage → GGUF CUDA Kernel (docs/INSTALLATION.md parity).
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
            match g
                .get("matmulMode")
                .and_then(|v| v.as_str())
                .unwrap_or("auto")
            {
                "fast" | "low_vram" => std::env::set_var(
                    "WGP_GGUF_LLAMACPP_CUDA_MATMUL_MODE",
                    g["matmulMode"].as_str().unwrap(),
                ),
                _ => std::env::remove_var("WGP_GGUF_LLAMACPP_CUDA_MATMUL_MODE"),
            }
            if g.get("streamK").and_then(|v| v.as_bool()) == Some(false) {
                std::env::set_var("WGP_GGUF_LLAMACPP_CUDA_STREAM_K", "0");
            } else {
                std::env::remove_var("WGP_GGUF_LLAMACPP_CUDA_STREAM_K");
            }
            if g.get("bf16Fp16").and_then(|v| v.as_bool()) == Some(true) {
                std::env::set_var("WGP_GGUF_LLAMACPP_CUDA_BF16_FP16", "1");
            } else {
                std::env::remove_var("WGP_GGUF_LLAMACPP_CUDA_BF16_FP16");
            }
        }
        emit(&format!(
            "[i] GGUF env: CUDA={} MATMUL={} STREAM_K={} BF16_FP16={}\n",
            std::env::var("WGP_GGUF_LLAMACPP_CUDA").unwrap_or("1".into()),
            std::env::var("WGP_GGUF_LLAMACPP_CUDA_MATMUL_MODE").unwrap_or("auto".into()),
            std::env::var("WGP_GGUF_LLAMACPP_CUDA_STREAM_K").unwrap_or("1".into()),
            std::env::var("WGP_GGUF_LLAMACPP_CUDA_BF16_FP16").unwrap_or("0".into())
        ));
    }
    // AMD GPU profile env from setup_config.json (e.g. HSA_OVERRIDE_GFX_VERSION).
    // Neither setup.py nor wgp.py exports these today — verified: only
    // setup_config.json references HSA_OVERRIDE — yet the per-arch values
    // (11.0.0/11.5.1/12.0.1) exist precisely so TheRock wheels detect the
    // right gfx target. Set what's configured, remove stale leftovers
    // (same reconcile pattern as the GGUF knobs above). Values come
    // verbatim from upstream's file — never invented here.
    {
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
    }
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
    // AMD ROCm session env (doc-leading: docs/AMD-INSTALLATION.md "Running Wan2GP").
    // Set-if-absent so explicit user overrides always win; logged like the HSA
    // override above. On a NON-AMD launch the stale values are NOT harmless:
    // `CC=clang-cl`, `CXX=clang-cl`, `DISTUTILS_USE_SDK=1` and a
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
            for (k, v) in [
                ("FLASH_ATTENTION_TRITON_AMD_ENABLE", "TRUE"),
                ("TORCH_ROCM_AOTRITON_ENABLE_EXPERIMENTAL", "1"),
            ] {
                if std::env::var(k).is_err() {
                    std::env::set_var(k, v);
                    emit(&format!("[i] GPU profile env: {k}={v}\n"));
                }
            }
            // MIOpen toggle (issue #15 follow-up): Manage-backed
            // `amdEnv.miopenDisabled` (default false, backend-only for now
            // — the frontend can bind the key later). When true,
            // MIOPEN_FIND_MODE is NOT set at all and the log says so;
            // when false, the current FAST set-if-absent stays.
            match miopen_find_mode() {
                Some(v) => {
                    if std::env::var("MIOPEN_FIND_MODE").is_err() {
                        std::env::set_var("MIOPEN_FIND_MODE", v);
                        emit(&format!("[i] GPU profile env: MIOPEN_FIND_MODE={v}\n"));
                    }
                }
                None => emit("[i] MIOpen disabled by user setting\n"),
            }
            // Virtual-GPU HIP pin (strict conditions only): exactly one
            // discrete AMD GPU PLUS ignored virtual display adapter(s)
            // (e.g. a Meta Virtual Monitor next to one discrete card) →
            // set-if-absent HIP_VISIBLE_DEVICES=0 so HIP can't land on the
            // virtual shim. Multi-AMD boxes (the index would be a guess)
            // and no-virtual-adapter boxes do nothing.
            {
                let discrete = wmi_all_gpus()
                    .iter()
                    .filter(|(_, v, _, _)| v == "AMD")
                    .count();
                let virtuals = wmi_virtual_adapters().len();
                if let Some(v) = hip_pin_value(discrete, virtuals) {
                    if std::env::var("HIP_VISIBLE_DEVICES").is_err() {
                        std::env::set_var("HIP_VISIBLE_DEVICES", v);
                        emit("[i] GPU profile env: HIP_VISIBLE_DEVICES=0 (one AMD GPU + virtual display adapter — pinning HIP to the discrete card)\n");
                    }
                }
            }
            // Full AMD launch env: derive the ROCm SDK dir from the
            // installed env (the `rocm` package dir, e.g.
            // `<env>/Lib/site-packages/rocm`). Never invent paths: when
            // the dir is absent, log and continue without failing launch.
            match amd_rocm_sdk_dir() {
                    Some(sdk) => {
                        let sdk_s = sdk.to_string_lossy().to_string();
                        if std::env::var("ROCM_HOME").is_err() {
                            std::env::set_var("ROCM_HOME", &sdk_s);
                            emit(&format!("[i] GPU profile env: ROCM_HOME={sdk_s}\n"));
                        }
                        let llvm_bin = sdk.join("lib").join("llvm").join("bin");
                        let sdk_bin = sdk.join("bin");
                        let mut prepend: Vec<String> = Vec::new();
                        if llvm_bin.is_dir() {
                            prepend.push(llvm_bin.to_string_lossy().to_string());
                        }
                        if sdk_bin.is_dir() {
                            prepend.push(sdk_bin.to_string_lossy().to_string());
                        }
                            if !prepend.is_empty() {
                                let old = std::env::var("PATH").unwrap_or_default();
                                let add = prepend.join(";");
                                if !old.split(';').any(|p| p.eq_ignore_ascii_case(&add)) {
                                    std::env::set_var("PATH", format!("{add};{old}"));
                                    if let Ok(mut g) = AMD_PATH_PREPEND
                                        .get_or_init(|| std::sync::Mutex::new(None))
                                        .lock()
                                    {
                                        *g = Some(add.clone());
                                    }
                                    emit(&format!("[i] GPU profile env: PATH prepend {add}\n"));
                                }
                            }
                        for (k, v) in [
                            ("CC", "clang-cl"),
                            ("CXX", "clang-cl"),
                            ("DISTUTILS_USE_SDK", "1"),
                        ] {
                            if std::env::var(k).is_err() {
                                std::env::set_var(k, v);
                                emit(&format!("[i] GPU profile env: {k}={v}\n"));
                            }
                        }
                    }
                        None => emit("[!] ROCm SDK dir not found (no rocm package in the active env) — launching without ROCM_HOME/CC/CXX.\n"),
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
    if mode == "terminal" {
        return launch_in_terminal(
            app,
            &repo,
            &py,
            &args,
            port,
            &cfg,
            hf_token.clone(),
            claude_key.clone(),
        );
    }
    let (rx, child) = cmd.spawn().map_err(|e| {
        mutating_done();
        emit(&format!("[LAUNCH ERROR] spawn failed: {e}\n"));
        e.to_string()
    })?;
    emit(&format!("[*] Spawned PID {}\n", child.pid()));
    if let Ok(m) = WANGP_PID.get_or_init(|| Mutex::new(None)).lock() {
        drop(m);
    }
    *WANGP_PID.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(child.pid());
    // stream logs in background
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        use tauri_plugin_shell::process::CommandEvent;
        let mut rx = rx;
        while let Some(ev) = rx.recv().await {
            match ev {
                CommandEvent::Stdout(b) => {
                    let s = String::from_utf8_lossy(&b).to_string();
                    crate::base::push_log(&s, "launch");
                    let _ = app2.emit("launch-log", s);
                }
                CommandEvent::Stderr(b) => {
                    let s = String::from_utf8_lossy(&b).to_string();
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
    let repo = get_repo_dir();
    let repo_s = repo.to_string_lossy().replace('/', "\\").to_lowercase();
    let repo_pre = format!("{repo_s}\\");
    let mut killed: Vec<u32> = Vec::new();
    // Plain closure (no captures): killed is passed in so later reads don't
    // fight the borrow checker.
    let kill_pid = |pid: u32, killed: &mut Vec<u32>| {
        if pid == 0 || pid == std::process::id() || killed.contains(&pid) {
            return;
        }
        #[cfg(windows)]
        {
            let _ = silent_command("taskkill")
                .args(["/pid", &pid.to_string(), "/f", "/t"])
                .output();
        }
        #[cfg(not(windows))]
        {
            let _ = silent_command("kill")
                .arg("-9")
                .arg(pid.to_string())
                .output();
        }
        killed.push(pid);
    };
    // One scan pass: all python.exe with wgp.py in the command line.
    // Returns (pid, exe_path, cmdline); filtering happens in Rust (exact,
    // case-insensitive — no PowerShell quoting pitfalls). Failures are LOUD:
    // a silently-empty scan is exactly how orphans used to survive Stop.
    let scan = || -> Vec<(u32, String, String)> {
        let mut out = Vec::new();
        #[cfg(windows)]
        {
            let ps = "Get-CimInstance Win32_Process -Filter \"Name='python.exe'\" | Where-Object { $_.CommandLine -like '*wgp.py*' } | ForEach-Object { $_.ProcessId + '|' + $_.ExecutablePath + '|' + $_.CommandLine }";
            match silent_command("powershell")
                .args(["-NoProfile", "-Command", ps])
                .output()
            {
                Ok(o) if o.status.success() => {
                    for line in String::from_utf8_lossy(&o.stdout).lines() {
                        let mut parts = line.splitn(3, '|');
                        if let (Some(pid_s), Some(exe), Some(cmd)) =
                            (parts.next(), parts.next(), parts.next())
                        {
                            if let Ok(pid) = pid_s.trim().parse::<u32>() {
                                out.push((pid, exe.to_string(), cmd.to_string()));
                            }
                        }
                    }
                }
                Ok(o) => crate::base::push_log(
                    &format!(
                        "[stop] WMI scan failed (exit {}): {}\n",
                        o.status.code().unwrap_or(-1),
                        String::from_utf8_lossy(&o.stderr)
                            .chars()
                            .take(300)
                            .collect::<String>()
                    ),
                    "launch",
                ),
                Err(e) => {
                    crate::base::push_log(&format!("[stop] WMI scan spawn failed: {e}\n"), "launch")
                }
            }
        }
        #[cfg(not(windows))]
        {
            let pat = repo.join("wgp.py").to_string_lossy().to_string();
            if let Ok(o) = silent_command("pgrep").args(["-af", &pat]).output() {
                if o.status.success() {
                    for line in String::from_utf8_lossy(&o.stdout).lines() {
                        let mut parts = line.splitn(2, ' ');
                        if let (Some(pid_s), Some(cmd)) = (parts.next(), parts.next()) {
                            if let Ok(pid) = pid_s.trim().parse::<u32>() {
                                out.push((pid, String::new(), cmd.to_string()));
                            }
                        }
                    }
                }
            }
        }
        out
    };
    let is_ours = |exe: &str, cmd: &str| -> bool {
        if !cmd.to_lowercase().contains("wgp.py") {
            return false;
        }
        let cl = cmd.to_lowercase();
        cl.contains(&repo_pre)                      // interpreter or script path under repo
            || exe.to_lowercase().replace('/', "\\").starts_with(&repo_pre) // env python, relative argv
            || cl.contains("wan2gp-bootstrap-") // our launcher bootstrap (any interpreter)
    };
    if let Some(pid) = WANGP_PID.get().and_then(|m| m.lock().ok()).and_then(|g| *g) {
        kill_pid(pid, &mut killed);
    }
    let mut found = 0usize;
    // Cache the first WMI pass: reused below by the custom-port sweep so
    // the extra python-listener probe stays the ONE extra PS call.
    let scan_first = scan();
    found += scan_first.len();
    for (pid, exe, cmd) in &scan_first {
        if is_ours(exe, cmd) {
            kill_pid(*pid, &mut killed);
        }
    }
    // our external-terminal window (unique timestamped title)
    #[cfg(windows)]
    if let Some(t) = crate::launch::terminal_title() {
        let _ = silent_command("taskkill")
            .args(["/F", "/FI", &format!("WINDOWTITLE eq {t}*")])
            .output();
    }
    // Best-effort cleanup of per-launch terminal scripts (unique
    // wan2gp-terminal-<millis>.bat files). Files only, ignore errors.
    {
        let tmp = std::env::temp_dir();
        if let Ok(rd) = std::fs::read_dir(&tmp) {
            for entry in rd.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("wan2gp-terminal-")
                    && name.ends_with(".bat")
                    && entry.path().is_file()
                {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
    // Ground truth: whatever LISTENS on a Wan2GP port dies too. Catches every
    // spawn shape (workers, renamed interpreters, stale launchers) — launch
    // itself treats port-in-use as "ours" (reuses instead of spawning), so
    // stop must treat it the same way. Restricted to python* owners: Gradio
    // always runs on Python, and we never kill foreign processes.
    // Ports: configured serverPort + the 7860/7861 defaults. A launcher killed
    // for a rebuild can't clean up, so the next instance (possibly with a
    // changed port) must still catch the previous session's listener —
    // proven orphan class 2026-09-10 (PID on :7861 survived every Stop).
    let sport = load_config_value()
        .get("serverPort")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(7860);
    let mut sports = vec![sport];
    for p in [7860u64, 7861u64] {
        if !sports.contains(&p) {
            sports.push(p);
        }
    }
    // Live progress: the sweep below takes seconds (PowerShell spawns) —
    // without these the console sits dead and Stop feels frozen.
    let say = |m: &str| {
        crate::base::push_log(m, "launch");
        let _ = app.emit("launch-log", m.to_string());
    };
    say("[stop] scanning Wan2GP processes…\n");
    // Pure scan (no kill): (pid, port) of python listeners on ANY of `ports`.
    // ONE PowerShell call for all ports (was one per port — the stall).
    // Used by the sweep below and by the alive check, so survivors are
    // reported, not hidden.
    let port_listeners = |ports: &[u64]| -> Vec<(u32, u64)> {
        let mut out = Vec::new();
        #[cfg(windows)]
        {
            let list = ports
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let ps = format!("Get-NetTCPConnection -LocalPort {list} -State Listen | ForEach-Object {{ $p = Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue; if ($p) {{ $p.Id.ToString() + '|' + $p.ProcessName + '|' + $_.LocalPort }} }}");
            match silent_command("powershell")
                .args(["-NoProfile", "-Command", &ps])
                .output()
            {
                Ok(o) if o.status.success() => {
                    for line in String::from_utf8_lossy(&o.stdout).lines() {
                        let mut parts = line.splitn(3, '|');
                        if let (Some(pid_s), Some(name), Some(port_s)) =
                            (parts.next(), parts.next(), parts.next())
                        {
                            if name.trim().to_lowercase().contains("python") {
                                if let (Ok(pid), Ok(port)) =
                                    (pid_s.trim().parse::<u32>(), port_s.trim().parse::<u64>())
                                {
                                    out.push((pid, port));
                                }
                            }
                        }
                    }
                }
                // Exit 1 + "No matching" = no listeners on these ports (the
                // CLEAN case) — not an error, stay silent so Stop reads clean.
                Ok(o) => {
                    let code = o.status.code().unwrap_or(-1);
                    let err: String = String::from_utf8_lossy(&o.stderr)
                        .chars()
                        .take(200)
                        .collect();
                    if !(code == 1 && err.contains("No matching")) {
                        crate::base::push_log(
                            &format!("[stop] port scan ({list}) failed (exit {code}): {err}\n"),
                            "launch",
                        );
                    }
                }
                Err(e) => crate::base::push_log(
                    &format!("[stop] port scan ({list}) spawn failed: {e}\n"),
                    "launch",
                ),
            }
        }
        #[cfg(not(windows))]
        {
            for port in ports {
                if let Ok(o) = silent_command("lsof")
                    .args(["-ti", &format!("tcp:{port}")])
                    .output()
                {
                    if o.status.success() {
                        for line in String::from_utf8_lossy(&o.stdout).lines() {
                            if let Ok(pid) = line.trim().parse::<u32>() {
                                out.push((pid, *port));
                            }
                        }
                    }
                }
            }
        }
        out
    };
    say("[stop] checking Wan2GP ports…\n");
    for (pid, port) in port_listeners(&sports) {
        crate::base::push_log(
            &format!("[stop] port {port}: killing python PID {pid}\n"),
            "launch",
        );
        kill_pid(pid, &mut killed);
    }
    // Custom-port evasion: ONE extra PS call listing ALL python-owned
    // listeners (any LocalPort). Rust keeps only PIDs whose WMI cmdline
    // has our signals (via the cached first scan + is_ours) or whose port
    // is in `sports`. Kill scope is NOT widened — kill_pid only, same as
    // every other sweep. Non-Windows: skipped (no cheap equivalent).
    #[cfg(windows)]
    {
        let ps_any = "Get-NetTCPConnection -State Listen | ForEach-Object { $p = Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue; if ($p -and $p.ProcessName -like 'python*') { $p.Id.ToString() + '|' + $_.LocalPort } }";
        let any_listeners: Vec<(u32, u64)> = match silent_command("powershell")
            .args(["-NoProfile", "-Command", ps_any])
            .output()
        {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|line| {
                    let mut parts = line.splitn(2, '|');
                    match (parts.next(), parts.next()) {
                        (Some(pid_s), Some(port_s)) => {
                            match (pid_s.trim().parse::<u32>(), port_s.trim().parse::<u64>()) {
                                (Ok(pid), Ok(port)) => Some((pid, port)),
                                _ => None,
                            }
                        }
                        _ => None,
                    }
                })
                .collect(),
            Ok(o) => {
                let code = o.status.code().unwrap_or(-1);
                let err: String = String::from_utf8_lossy(&o.stderr)
                    .chars()
                    .take(200)
                    .collect();
                if !(code == 1 && err.contains("No matching")) {
                    crate::base::push_log(
                        &format!("[stop] python-listener scan failed (exit {code}): {err}\n"),
                        "launch",
                    );
                }
                Vec::new()
            }
            Err(e) => {
                crate::base::push_log(
                    &format!("[stop] python-listener scan spawn failed: {e}\n"),
                    "launch",
                );
                Vec::new()
            }
        };
        for (pid, port) in any_listeners {
            if killed.contains(&pid) {
                continue;
            }
            if sports.contains(&port) {
                // Already covered by the sports sweep above; a live entry
                // here means the earlier kill missed — retry loudly.
                crate::base::push_log(
                    &format!("[stop] port {port}: killing python PID {pid}\n"),
                    "launch",
                );
                kill_pid(pid, &mut killed);
            } else if let Some((_, exe, cmd)) = scan_first.iter().find(|(p, _, _)| *p == pid) {
                // Custom-port evasion: python listener off the known ports
                // that is still OURS by cmdline signals.
                if is_ours(exe, cmd) {
                    crate::base::push_log(
                        &format!("[stop] custom port {port}: killing ours PID {pid}\n"),
                        "launch",
                    );
                    kill_pid(pid, &mut killed);
                }
            }
        }
    }
    if let Some(m) = WANGP_PID.get() {
        *m.lock().unwrap() = None;
    }
    // Verify: re-scan after the dust settles, kill stragglers once, report
    // who's still alive instead of claiming success with orphans around.
    std::thread::sleep(std::time::Duration::from_millis(1200));
    for (pid, exe, cmd) in scan() {
        if is_ours(&exe, &cmd) && !killed.contains(&pid) {
            kill_pid(pid, &mut killed);
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
        .filter(|(_, exe, cmd)| is_ours(exe, cmd))
        .map(|(pid, _, _)| pid)
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

// ── misc stubs to unblock frontend (return safe defaults) ──
#[tauri::command]
pub fn open_external(url: Option<String>) {
    let _ = url;
}
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
/// Install-location candidates per browser (user-level LocalAppData first,
/// then system-wide Program Files — Brave/Opera/Vivaldi all support those).
fn browser_candidates(id: &str) -> &[&str] {
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
#[tauri::command]
pub fn launch_browser_no_gpu(url: Option<String>) -> serde_json::Value {
    // No-GPU browser frees VRAM for generation (mirrors Electron's chrome flags).
    let u = url.unwrap_or_else(|| "http://localhost:7861".into());
    if !(u.starts_with("http://") || u.starts_with("https://")) {
        return serde_json::json!({"ok": false, "success": false, "error": "invalid url"});
    }
    let chosen = load_config_value()
        .get("defaultBrowser")
        .and_then(|v| v.as_str())
        .unwrap_or("system")
        .to_string();
    // Prefer Chrome, else the chosen browser, else whatever opener gives (GPU on).
    let exe = find_browser_exe("chrome").or_else(|| {
        if chosen != "system" {
            find_browser_exe(&chosen)
        } else {
            None
        }
    });
    let Some(path) = exe else {
        return serde_json::json!({"ok": false, "success": false, "error": "No Chromium browser found for no-GPU launch"});
    };
    let args = [
        "--disable-gpu",
        "--disable-gpu-compositing",
        "--disable-accelerated-2d-canvas",
        "--disable-accelerated-video-decode",
        "--use-angle=swiftshader",
        "--enable-unsafe-swiftshader",
        "--disable-webgpu",
    ];
    match silent_command(&path).args(args).arg(&u).spawn() {
        Ok(_) => serde_json::json!({"ok": true, "success": true}),
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
pub async fn launch_webview(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    launch(app, Some("app".into())).await
}
#[tauri::command]
pub async fn popout_webview(
    app: tauri::AppHandle,
    url: Option<String>,
) -> Result<serde_json::Value, String> {
    let res = launch(app.clone(), Some("browser".into())).await?;
    let u = url
        .or_else(|| {
            res.get("url")
                .and_then(|v| v.as_str())
                .map(std::string::ToString::to_string)
        })
        .unwrap_or_else(|| "http://localhost:7861".into());
    use tauri_plugin_opener::OpenerExt;
    let _ = app.opener().open_url(u, None::<String>);
    Ok(res)
}

#[cfg(test)]
mod launch_args_tests {
    use super::*;
    #[test]
    fn split_launch_args_quotes() {
        assert_eq!(
            split_launch_args("--profile 4 --attention sage2"),
            vec!["--profile", "4", "--attention", "sage2"]
        );
        assert_eq!(
            split_launch_args("--teacache \"a b\" --verbose 2"),
            vec!["--teacache", "a b", "--verbose", "2"]
        );
        assert_eq!(split_launch_args(""), Vec::<String>::new());
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
        // Reporter shape: one discrete AMD card + virtual monitor → pin.
        assert_eq!(hip_pin_value(1, 1), Some("0"));
        assert_eq!(hip_pin_value(1, 3), Some("0"));
        // Multi-AMD (the right index would be a guess), no AMD card, and
        // no-virtual-adapter boxes → do nothing.
        assert_eq!(hip_pin_value(2, 1), None);
        assert_eq!(hip_pin_value(0, 1), None);
        assert_eq!(hip_pin_value(1, 0), None);
        assert_eq!(hip_pin_value(0, 0), None);
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

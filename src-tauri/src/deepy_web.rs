//! Deepy Web standalone lifecycle (Slice 0 Core: Same-PC + Phone-LAN HTTP).
//!
//! Upstream `wgp.py` owns `--deepy-server` / `--listen` / `--auth`; this module only
//! composes them verbatim on a dedicated `deepyPort` (default `serverPort+1`).
//! Auth password travels ONLY via child env `WANGP_AUTH_PASSWORD` — never CLI,
//! never logs, never desktop-config. HTTPS certs and Tailscale are later slices — out of scope.
//!
//! Contracts: never render `0.0.0.0`; stop is scoped to `deepyPort` only
//! (the main Gradio server on `serverPort` is never touched); `wgp_config.json`
//! writes go exclusively through `features::deepy_set` (keeps `.deepy-bak`
//! backups + the literal-exe-name rule).

use crate::base::*;
use crate::status::{get_active_env, resolve_env_python};
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::Emitter;

/// True while a `deepy_web_start` boot wait is in flight. Rejects concurrent
/// starts (double-click / retry-while-loading) which would otherwise race two
/// model loads onto one port and double VRAM use.
static DEEPY_STARTING: AtomicBool = AtomicBool::new(false);

/// RAII reset for `DEEPY_STARTING` — every exit path after it is set clears it.
struct ClearDeepyStarting;
impl Drop for ClearDeepyStarting {
    fn drop(&mut self) {
        DEEPY_STARTING.store(false, Ordering::SeqCst);
    }
}

/// Default Deepy Web port: main `serverPort + 1` (e.g. 7861 for 7860).
pub(crate) fn default_deepy_port(server_port: u64) -> u64 {
    server_port.saturating_add(1)
}

/// Validate a Deepy Web port: range 1–65535 and `deepyPort != serverPort`.
pub(crate) fn validate_deepy_port(deepy_port: u64, server_port: u64) -> Result<u64, String> {
    if deepy_port == 0 || deepy_port > 65535 {
        return Err(format!(
            "Deepy Web port {deepy_port} is out of range — pick 1–65535."
        ));
    }
    if deepy_port == server_port {
        return Err(format!(
            "Deepy Web port must differ from the main server port ({server_port})."
        ));
    }
    Ok(deepy_port)
}

/// Parse user-entered port text (rejects non-numeric) then validates.
/// Exercised by unit tests; wired to the Advanced port field in the frontend slice.
#[allow(dead_code)]
pub(crate) fn parse_deepy_port(raw: &str, server_port: u64) -> Result<u64, String> {
    let trimmed = raw.trim();
    match trimmed.parse::<u64>() {
        Ok(n) => validate_deepy_port(n, server_port),
        Err(_) => Err(format!(
            "Deepy Web port \"{trimmed}\" is not a number — pick 1–65535."
        )),
    }
}

/// Same-PC URL for a Deepy Web port.
pub(crate) fn same_pc_url(deepy_port: u64) -> String {
    format!("http://localhost:{deepy_port}")
}

/// True for an IPv4 address usable as a Phone-LAN host: not loopback,
/// not `0.0.0.0`, not multicast.
fn is_usable_lan_ip(s: &str) -> bool {
    match s.trim().parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => !v4.is_loopback() && !v4.is_unspecified() && !v4.is_multicast(),
        _ => false,
    }
}

/// Pick the LAN IPv4 for the Phone URL: first usable non-link-local *real*
/// LAN address (a Tailscale 100.64/10 address is not same-Wi-Fi reachable,
/// so it loses to plain LAN), else first usable non-link-local, else first
/// usable. Never returns loopback or `0.0.0.0`.
pub(crate) fn select_lan_ip(candidates: &[String]) -> Option<String> {
    let usable: Vec<&String> = candidates.iter().filter(|c| is_usable_lan_ip(c)).collect();
    usable
        .iter()
        .find(|c| !c.trim().starts_with("169.254.") && lan_ip_kind(c) == "lan")
        .or_else(|| usable.iter().find(|c| !c.trim().starts_with("169.254.")))
        .or_else(|| usable.first())
        .map(|c| c.trim().to_string())
}

/// First usable Tailscale (100.64.0.0/10) address — the off-LAN host for
/// the External URL row. `None` when Tailscale is not active.
pub(crate) fn select_external_ip(candidates: &[String]) -> Option<String> {
    ordered_lan_ips(candidates)
        .into_iter()
        .filter(|ip| !ip.starts_with("169.254."))
        .find(|ip| lan_ip_kind(ip) == "tailscale")
}

/// Kind label for the multi-IP panel: Tailscale CGNAT (100.64.0.0/10) vs plain LAN.
pub(crate) fn lan_ip_kind(ip: &str) -> &'static str {
    match ip.trim().parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(v4)) => {
            let o = v4.octets();
            if o[0] == 100 && (o[1] & 0xc0) == 0x40 {
                "tailscale"
            } else {
                "lan"
            }
        }
        _ => "lan",
    }
}

/// All usable addresses, ordered: non-link-local first (stable), then
/// link-local; loopback/unspecified/multicast dropped, deduped.
pub(crate) fn ordered_lan_ips(candidates: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut usable: Vec<String> = candidates
        .iter()
        .map(|c| c.trim().to_string())
        .filter(|c| is_usable_lan_ip(c) && seen.insert(c.clone()))
        .collect();
    usable.sort_by_key(|c| c.starts_with("169.254."));
    usable
}

/// (ip, url, kind) rows for the multi-IP panel on `deepy_port`.
pub(crate) fn phone_url_list(
    candidates: &[String],
    deepy_port: u64,
) -> Vec<(String, String, String)> {
    ordered_lan_ips(candidates)
        .into_iter()
        .filter(|ip| !ip.starts_with("169.254."))
        .filter_map(|ip| {
            phone_url(&ip, deepy_port).map(|url| {
                let kind = lan_ip_kind(&ip).to_string();
                (ip, url, kind)
            })
        })
        .collect()
}

/// Phone URL from a LAN IP, or `None` when there is no usable address —
/// the caller renders an unavailable+guidance state instead. Never emits
/// `0.0.0.0`.
pub(crate) fn phone_url(lan_ip: &str, deepy_port: u64) -> Option<String> {
    if is_usable_lan_ip(lan_ip) {
        Some(format!("http://{}:{deepy_port}", lan_ip.trim()))
    } else {
        None
    }
}

/// Upstream flags composed verbatim: Same-PC is
/// `wgp.py --deepy-server --server-port N`; LAN appends `--listen`.
/// Unit-tested contract spec; `deepy_web_start` composes the same flags through
/// the shared `launch::build_wgp_args` builder (plus `--deepy-sessions-dir`).
#[allow(dead_code)]
pub(crate) fn build_deepy_args(deepy_port: u64, lan: bool) -> Vec<String> {
    let mut args = vec![
        "wgp.py".to_string(),
        "--deepy-server".to_string(),
        "--server-port".to_string(),
        deepy_port.to_string(),
    ];
    if lan {
        args.push("--listen".to_string());
    }
    args
}

/// Child-env key for the Deepy Web auth password. The ONLY transport:
/// never a CLI arg, never logged, never persisted.
pub(crate) const AUTH_ENV_KEY: &str = "WANGP_AUTH_PASSWORD";

pub(crate) fn auth_env_key() -> &'static str {
    AUTH_ENV_KEY
}

/// Auth-aware arg composer: Same-PC/LAN base plus verbatim upstream `--auth`
/// when enabled. The password itself is NEVER placed in argv (see `AUTH_ENV_KEY`).
#[allow(dead_code)]
pub(crate) fn build_deepy_args_auth(deepy_port: u64, lan: bool, auth_enabled: bool) -> Vec<String> {
    let mut args = build_deepy_args(deepy_port, lan);
    if auth_enabled {
        args.push("--auth".to_string());
    }
    args
}

/// True when any argv entry contains the secret (must always be false for
/// the auth password — the RED contract).
pub(crate) fn args_contain_secret(args: &[String], secret: &str) -> bool {
    if secret.is_empty() {
        return false;
    }
    args.iter().any(|a| a.contains(secret))
}

/// Generate a one-time auth password (32 hex chars from time+pid+counter
/// hashed with SHA-256 — no new dependencies). Shown once, never stored.
pub(crate) fn generate_auth_password() -> String {
    use sha2::{Digest, Sha256};
    static CTR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = CTR.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let seed = format!(
        "{}-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
        n,
        (&n as *const u64) as usize
    );
    let mut h = Sha256::new();
    h.update(seed.as_bytes());
    let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    hex[..32].to_string()
}

/// Resolve the password for an auth mode: `generate` mints one,
/// `fixed` reuses the user secret (empty -> None), anything else -> None.
pub(crate) fn auth_password_for_mode(mode: &str, fixed: Option<&str>) -> Option<String> {
    match mode {
        "generate" => Some(generate_auth_password()),
        "fixed" => fixed.filter(|v| !v.is_empty()).map(|v| v.to_string()),
        _ => None,
    }
}

/// True when a log line would leak the secret (non-empty secret contained).
#[allow(dead_code)]
pub(crate) fn log_line_leaks_secret(line: &str, secret: &str) -> bool {
    !secret.is_empty() && line.contains(secret)
}

/// Scrub a secret from a log line before emitting/persisting.
#[allow(dead_code)]
pub(crate) fn scrub_secret(line: &str, secret: &str) -> String {
    if secret.is_empty() {
        return line.to_string();
    }
    line.replace(secret, "[redacted]")
}

/// Normalize a desktop-config `deepyAuthMode` value to off|generate|fixed.
pub(crate) fn normalize_auth_mode(raw: Option<&str>) -> String {
    match raw {
        Some("generate") => "generate".to_string(),
        Some("fixed") => "fixed".to_string(),
        _ => "off".to_string(),
    }
}

/// Port-scoped stop predicate: a process is ours to stop only when its
/// command line names `wgp.py` with `--deepy-server` bound to `deepy_port`.
/// The main Gradio server (no `--deepy-server`, or another port) never matches.
/// Slice 2 HTTPS - cert validation + verbatim upstream flag composer.
pub(crate) fn validate_cert_pair(cert_path: &str, key_path: &str) -> Result<(), String> {
    if cert_path.trim().is_empty() {
        return Err("Certificate (.pem) path is required for LAN HTTPS - bring a .pem or Create-LAN-cert first.".to_string());
    }
    if key_path.trim().is_empty() {
        return Err(
            "Key (.key) path is required for LAN HTTPS - bring a .key or Create-LAN-cert first."
                .to_string(),
        );
    }
    for (label, path) in [
        ("Certificate (.pem)", cert_path.trim()),
        ("Key (.key)", key_path.trim()),
    ] {
        let pp = std::path::Path::new(path);
        if !pp.is_file() {
            return Err(format!("{} not found or unreadable: {}", label, path));
        }
        if std::fs::File::open(pp).is_err() {
            return Err(format!(
                "{} not readable (check permissions): {}",
                label, path
            ));
        }
    }
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn build_https_args(
    deepy_port: u64,
    lan: bool,
    cert_path: &str,
    key_path: &str,
    https_port: Option<u64>,
) -> Vec<String> {
    let mut args = build_deepy_args(deepy_port, lan);
    args.push("--ssl-certfile".to_string());
    args.push(cert_path.to_string());
    args.push("--ssl-keyfile".to_string());
    args.push(key_path.to_string());
    if let Some(hp) = https_port {
        args.push("--https-port".to_string());
        args.push(hp.to_string());
    }
    args
}

#[allow(dead_code)]
pub(crate) fn https_url(host: &str, https_port: u64) -> String {
    format!("https://{}:{}", host.trim(), https_port)
}

/// Upstream `--public-url` (WanGP 13.11+, reverse-proxy support): an exact
/// HTTP(S) origin — scheme + host + optional port, no path/query/fragment/
/// credentials. Pure port of upstream `parse_public_url` (no pydantic here);
/// returns the normalized origin (no trailing slash) or a fail-closed error.
pub(crate) fn validate_public_url(raw: &str) -> Result<String, String> {
    let v = raw.trim();
    let err = || {
        Err("--public-url must be an HTTP(S) origin, e.g. https://wangp.example.com, without a path, query, fragment or credentials.".to_string())
    };
    let after_scheme = if let Some(rest) = v.strip_prefix("https://") {
        rest
    } else if let Some(rest) = v.strip_prefix("http://") {
        rest
    } else {
        return err();
    };
    if after_scheme.is_empty() {
        return err();
    }
    if v.chars()
        .any(|c| c.is_whitespace() || (c as u32) < 32 || "?#\\*".contains(c) || c == '@')
    {
        return err();
    }
    // Authority is up to the first '/'; anything beyond a single trailing
    // slash is a path and is rejected.
    let (authority, rest) = match after_scheme.find('/') {
        Some(i) => (&after_scheme[..i], &after_scheme[i..]),
        None => (after_scheme, ""),
    };
    if !(rest.is_empty() || rest == "/") || authority.is_empty() {
        return err();
    }
    // Optional :port must be numeric 1-65535 (mirror upstream url.port check).
    if let Some(colon) = authority.rfind(':') {
        let host_part = &authority[..colon];
        let port_part = &authority[colon + 1..];
        if host_part.is_empty() {
            return err();
        }
        match port_part.parse::<u64>() {
            Ok(p) if (1..=65535).contains(&p) => {}
            _ => return err(),
        }
    }
    let scheme = if v.starts_with("https://") {
        "https"
    } else {
        "http"
    };
    Ok(format!("{scheme}://{authority}"))
}

/// Verbatim upstream `--public-url` composer for the start-arg chain.
#[allow(dead_code)]
pub(crate) fn build_public_url_args(mut args: Vec<String>, public_url: &str) -> Vec<String> {
    args.push("--public-url".to_string());
    args.push(public_url.to_string());
    args
}

pub(crate) fn should_stop_deepy_process(cmdline: &str, deepy_port: u64) -> bool {
    let cl = cmdline.to_lowercase();
    cl.contains("wgp.py") && cl.contains("--deepy-server") && cl.contains(&deepy_port.to_string())
}

/// Upstream flag drift: surface an actionable hint instead of fake-success
/// when the child dies on an unrecognized argument.
pub(crate) fn flag_drift_hint(stderr: &str) -> Option<String> {
    if stderr.to_lowercase().contains("unrecognized argument") {
        Some(
            "Upstream wgp.py rejected a launch flag (expected --deepy-server / --listen / --auth / --public-url). \
             Update Wan2GP to a version with Deepy Web support, then retry."
                .to_string(),
        )
    } else {
        None
    }
}

/// Non-blocking Gradio-clash notice: the main server stays on `serverPort`
/// while Deepy Web runs beside it on `deepyPort`.
pub(crate) fn clash_notice(main_bound: bool, deepy_free: bool) -> Option<String> {
    if main_bound && deepy_free {
        Some(
            "The main Wan2GP server keeps running on its port — Deepy Web starts \
             as a second process alongside it (e.g. 7860 + 7861). Finish in one \
             view, stop Deepy Web, then resume in the other: the two do not live-sync."
                .to_string(),
        )
    } else {
        None
    }
}

/// Auto-config decision (pure, unit-tested). Disabled → Zero + Qwen3.5-4B;
/// stale enhancer 1/2 → fix to 3; Prime-local without 27B weights → BLOCK
/// (fail-closed: never silently rewrite Prime to Zero — the starter owns
/// truth about where weights live, and a wrong guess would clobber the
/// user's explicit Prime choice on every boot); anything healthy stays
/// untouched.
#[derive(Debug, PartialEq)]
pub(crate) enum AutoPlan {
    Keep,
    ZeroPlusQwen4B,
    FixEnhancerTo3,
    BlockMissing27B,
}

pub(crate) fn auto_config_plan(
    deepy_enabled: i64,
    deepy_type: &str,
    current_engine: &str,
    enhancer: Option<i64>,
    engine_path_exists: bool,
) -> AutoPlan {
    if deepy_enabled == 0 {
        return AutoPlan::ZeroPlusQwen4B;
    }
    let local_prime = deepy_type == "prime"
        && (current_engine.contains("qwen") || current_engine.contains("local"));
    if local_prime && !engine_path_exists {
        return AutoPlan::BlockMissing27B;
    }
    if matches!(enhancer, Some(1) | Some(2)) {
        return AutoPlan::FixEnhancerTo3;
    }
    AutoPlan::Keep
}

/// Upstream Qwen3.8 27B text-GGUF checkpoints the local Prime engine can run
/// on (shared/prompt_enhancer/assets.py: `QWEN38_27B_TEXT_GGUF_*`). Any one of
/// them counts as "weights present" — the Q4_K_M / IQ3_S / IQ2_M quant menu.
const QWEN38_27B_WEIGHT_FILES: &[&str] = &[
    "Qwen3.8-27B-Uncensored-Q4_K_M.gguf",
    "Qwen3.8-27B-Uncensored-noMTP-IQ3_S.gguf",
    "Qwen3.8-27B-Uncensored-IQ2_M.gguf",
];
/// Upstream assets folder holding the 27B weights
/// (shared/prompt_enhancer/assets.py: `assets_dir_name`).
const QWEN38_27B_ASSETS_DIR: &str = "Qwen3_8_27B_Uncensored";

/// Checkpoint roots to probe for the 27B folder: `wgp_config.json`
/// `checkpoints_paths` (absolute entries as-is; relative ones resolved against
/// the repo root, mirroring upstream `files_locator` which runs with the repo
/// as CWD; `"."` entries skipped), falling back to `<repo>/ckpts` when the
/// key is missing. Pure over (config, repo) so it stays unit-tested.
pub(crate) fn qwen27b_search_roots(
    cfg: &serde_json::Value,
    repo: &std::path::Path,
) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Some(arr) = cfg.get("checkpoints_paths").and_then(|v| v.as_array()) {
        for entry in arr {
            if let Some(s) = entry
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty() && *s != ".")
            {
                let p = std::path::Path::new(s);
                out.push(if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    repo.join(p)
                });
            }
        }
    }
    if out.is_empty() {
        out.push(repo.join("ckpts"));
    }
    out
}

/// True when an explicit `llm_engines` profile entry for a 27B-style engine
/// points at an existing path (hand-configured setups). Legacy branch of the
/// weights probe — the launcher itself never writes such entries.
fn profile_points_at_existing_path(v: &serde_json::Value) -> bool {
    let profiles = v
        .get("llm_engines")
        .and_then(|l| l.get("profiles"))
        .and_then(|p| p.as_object());
    let Some(profiles) = profiles else {
        return false;
    };
    profiles.iter().any(|(name, entry)| {
        let n = name.to_lowercase();
        if !(n.contains("27b") || n.contains("qwen38")) {
            return false;
        }
        let mut paths = Vec::new();
        if let Some(p) = entry.get("path").and_then(|x| x.as_str()) {
            paths.push(p.to_string());
        }
        if let Some(obj) = entry.as_object() {
            for (_, val) in obj {
                if let Some(s) = val.as_str() {
                    if s.len() > 3 {
                        paths.push(s.to_string());
                    }
                }
            }
        }
        paths.iter().any(|p| std::path::Path::new(p).exists())
    })
}

/// True when the Qwen3.8 27B weights are on disk: either an explicit profile
/// path (legacy, see above) or — the layout upstream actually uses — the
/// `Qwen3_8_27B_Uncensored` assets folder under a checkpoint root holding one
/// of the known text GGUF checkpoints. A bare folder without weights does NOT
/// count. Pure over (config, repo); the only impurity is the fs probe.
pub(crate) fn qwen27b_weights_present(cfg: &serde_json::Value, repo: &std::path::Path) -> bool {
    if profile_points_at_existing_path(cfg) {
        return true;
    }
    qwen27b_search_roots(cfg, repo).iter().any(|root| {
        let dir = root.join(QWEN38_27B_ASSETS_DIR);
        QWEN38_27B_WEIGHT_FILES
            .iter()
            .any(|f| dir.join(f).is_file())
    })
}

/// Read desktop-config ports: `(server_port, deepy_port_or_validation_error)`.
fn resolve_ports(deepy_override: Option<u64>) -> (u64, Result<u64, String>) {
    let cfg = load_config_value();
    let server_port = cfg
        .get("serverPort")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(7860);
    let deepy = deepy_override
        .or_else(|| cfg.get("deepyPort").and_then(serde_json::Value::as_u64))
        .unwrap_or_else(|| default_deepy_port(server_port));
    (server_port, validate_deepy_port(deepy, server_port))
}

/// True when nothing is listening on `127.0.0.1:port`.
fn port_is_free(port: u64) -> bool {
    use std::time::Duration;
    let addr = format!("127.0.0.1:{port}");
    match addr.parse::<std::net::SocketAddr>() {
        Ok(sa) => std::net::TcpStream::connect_timeout(&sa, Duration::from_millis(500)).is_err(),
        Err(_) => false,
    }
}

/// Enumerate non-loopback LAN IPv4s: `local-ipaddress` crate first,
/// `ipconfig` parse as fallback. Never yields `0.0.0.0`.
fn lan_candidates() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Ok(nics) = local_ip_address::list_afinet_netifas() {
        for (_, ip) in nics {
            if let IpAddr::V4(v4) = ip {
                let s = v4.to_string();
                if is_usable_lan_ip(&s) && !out.contains(&s) {
                    out.push(s);
                }
            }
        }
    }
    if out.is_empty() {
        if let Ok(o) = silent_command("ipconfig").output() {
            if o.status.success() {
                for line in String::from_utf8_lossy(&o.stdout).lines() {
                    if line.contains("IPv4") {
                        if let Some(token) = line.split(':').next_back() {
                            let ip = token.trim().to_string();
                            if is_usable_lan_ip(&ip) && !out.contains(&ip) {
                                out.push(ip);
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

fn resolve_lan_ip() -> Option<String> {
    select_lan_ip(&lan_candidates())
}

/// Resolve the interpreter for the active env (mirrors `launch`).
fn resolve_py() -> String {
    let repo = get_repo_dir();
    let env = get_active_env();
    if let Some(raw) = env.get("path").and_then(|p| p.as_str()) {
        if let Some(py) = resolve_env_python(&repo, raw) {
            return py.to_string_lossy().to_string();
        }
    }
    "python".to_string()
}

static DEEPY_PID: std::sync::OnceLock<Mutex<Option<u32>>> = std::sync::OnceLock::new();
static DEEPY_MODE: std::sync::OnceLock<Mutex<Option<String>>> = std::sync::OnceLock::new();
static DEEPY_AUTH: std::sync::OnceLock<Mutex<Option<String>>> = std::sync::OnceLock::new();

fn remember_deepy_auth(mode: Option<String>) {
    *DEEPY_AUTH
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = mode;
}

fn last_deepy_auth() -> Option<String> {
    DEEPY_AUTH
        .get()
        .and_then(|m| m.lock().ok())
        .and_then(|g| g.clone())
}

fn clear_deepy_auth() {
    *DEEPY_AUTH
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = None;
}

/// Drop all in-memory Deepy tracking (spawned PID + auth mode) — used when a
/// boot is abandoned so later status polls fall back to the port probe.
fn clear_deepy_tracking() {
    if let Some(m) = DEEPY_PID.get() {
        *m.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
    clear_deepy_auth();
}

fn remember_deepy(child_pid: u32, mode: &str) {
    *DEEPY_PID
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(child_pid);
    *DEEPY_MODE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(mode.to_string());
}

fn last_deepy_mode() -> String {
    DEEPY_MODE
        .get()
        .and_then(|m| m.lock().ok())
        .and_then(|g| g.clone())
        .unwrap_or_else(|| "same-pc".to_string())
}

/// Apply the Slice 0 auto-config contract through `deepy_set` (backup +
/// literal-exe-name rules live there). Returns the plan label.
fn ensure_deepy_config_for_web() -> Result<String, String> {
    let repo = get_repo_dir();
    let p = repo.join("wgp_config.json");
    let s = std::fs::read_to_string(&p)
        .map_err(|_| "wgp_config.json not found — install Wan2GP first.".to_string())?;
    let v: serde_json::Value =
        serde_json::from_str(&s).map_err(|_| "wgp_config.json is corrupted.".to_string())?;
    let enabled = v
        .get("deepy_enabled")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    let dtype = v
        .get("deepy_type")
        .and_then(|x| x.as_str())
        .unwrap_or("zero")
        .to_string();
    let engine = v
        .get("llm_engines")
        .and_then(|l| l.get("deepy"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let enh = v
        .get("enhancer_enabled")
        .and_then(serde_json::Value::as_i64);
    let plan = auto_config_plan(
        enabled,
        &dtype,
        &engine.to_lowercase(),
        enh,
        qwen27b_weights_present(&v, &repo),
    );
    let mut label = match plan {
        AutoPlan::Keep => Ok("kept".to_string()),
        AutoPlan::ZeroPlusQwen4B => {
            let r = crate::features::deepy_set(
                "zero".to_string(),
                None,
                Some(serde_json::json!(3)),
                None,
            );
            if r.get("ok").and_then(|x| x.as_bool()) == Some(true) {
                Ok("zero+qwen35-4b".to_string())
            } else {
                Err(r
                    .get("error")
                    .and_then(|x| x.as_str())
                    .unwrap_or("auto-configure failed")
                    .to_string())
            }
        }
        // Fail-closed: local Prime without visible 27B weights must NEVER
        // be silently rewritten to Zero (that clobbers an explicit user
        // choice on every boot). Block the start with an
        // actionable error instead — the file stays exactly as the user
        // left it.
        AutoPlan::BlockMissing27B => Err("Deepy Prime (local Qwen 27B) is configured but no 27B weights were found — refusing to downgrade you to Zero. Install the Qwen3.8 VL 27B model, switch to a remote Prime engine (OpenCode/Claude/Codex), or press Apply on Deepy Zero.".to_string()),
        AutoPlan::FixEnhancerTo3 => {
            // deepy_set validates Prime engines by UI id (opencode /
            // claude-code / codex / local-qwen38) but the stored config
            // carries profile ids (opencode / claude / codex / qwen38_27b /
            // qwen35_*). Map profile → UI id first, or Prime starts here
            // fail outright with "Prime requires an engine".
            let eng_arg = if dtype == "prime" {
                Some(crate::features::prime_profile_to_ui_id(&engine).to_string())
            } else {
                None
            };
            let r = crate::features::deepy_set(
                dtype.clone(),
                eng_arg,
                Some(serde_json::json!(3)),
                None,
            );
            if r.get("ok").and_then(|x| x.as_bool()) == Some(true) {
                Ok("enhancer-3".to_string())
            } else {
                Err(r
                    .get("error")
                    .and_then(|x| x.as_str())
                    .unwrap_or("enhancer fix failed")
                    .to_string())
            }
        }
    }?;
    // Standard sessions: multisessions with selectable workspace (one shared
    // outputs folder), so media generated in Deepy Web carries over to Gradio
    // through session resume. `disabled` (upstream default) means a temporary
    // conversation invisible across processes. An explicit choice is respected
    // — only disabled/missing is upgraded.
    // Re-read: the plan handling above may have rewritten the file already.
    let ps = std::fs::read_to_string(&p)
        .map_err(|_| "wgp_config.json not found — install Wan2GP first.".to_string())?;
    let mut vv: serde_json::Value =
        serde_json::from_str(&ps).map_err(|_| "wgp_config.json is corrupted.".to_string())?;
    if needs_session_upgrade(&vv) {
        let bak = p.with_file_name("wgp_config.json.deepy-bak");
        let _ = std::fs::copy(&p, &bak);
        vv["deepy_multi_session"] = serde_json::json!("selectable");
        atomic_write(&p, &serde_json::to_string_pretty(&vv).unwrap_or_default())
            .map_err(|_| "failed to write wgp_config.json".to_string())?;
        label.push_str("+sessions");
    }
    Ok(label)
}

fn deepy_urls(deepy_port: u64) -> serde_json::Value {
    let same_pc = same_pc_url(deepy_port);
    let cands = lan_candidates();
    let lan_rows: Vec<serde_json::Value> = phone_url_list(&cands, deepy_port)
        .into_iter()
        .map(|(ip, url, kind)| serde_json::json!({"ip": ip, "url": url, "kind": kind}))
        .collect();
    let mut urls = match resolve_lan_ip().and_then(|ip| phone_url(&ip, deepy_port)) {
        Some(phone) => serde_json::json!({
            "samePc": same_pc,
            "phone": phone,
            "phoneUnavailable": false,
            "lanIps": lan_rows,
        }),
        None => serde_json::json!({
            "samePc": same_pc,
            "phone": serde_json::Value::Null,
            "phoneUnavailable": true,
            "phoneGuidance": "No LAN adapter found — connect to Wi-Fi/Ethernet to enable the Phone URL.",
            "lanIps": lan_rows,
        }),
    };
    // External row: Tailscale IPv4 URL for off-LAN access (null when
    // Tailscale is not active — the frontend hides the row).
    match select_external_ip(&cands).and_then(|ip| phone_url(&ip, deepy_port).map(|url| (ip, url)))
    {
        Some((ip, url)) => {
            urls["external"] = serde_json::json!(url);
            urls["externalIp"] = serde_json::json!(ip);
            urls["externalUnavailable"] = serde_json::json!(false);
        }
        None => {
            urls["external"] = serde_json::Value::Null;
            urls["externalIp"] = serde_json::Value::Null;
            urls["externalUnavailable"] = serde_json::json!(true);
        }
    }
    urls
}

#[tauri::command]
pub async fn deepy_web_preflight(mode: Option<String>) -> Result<serde_json::Value, String> {
    let lan = mode.as_deref() == Some("lan");
    let repo = get_repo_dir();
    let installed = repo.join("wgp.py").exists();
    let py = resolve_py();
    let torch = silent_command(&py)
        .args(["-c", "import torch"])
        .output()
        .is_ok_and(|o| o.status.success());
    let (server_port, deepy_res) = resolve_ports(None);
    let mut errors: Vec<String> = Vec::new();
    if !installed {
        errors.push("Wan2GP not installed — run Install first.".to_string());
    }
    if installed && !torch {
        errors.push(
            "The environment's Python can't import torch — finish or repair the install first."
                .to_string(),
        );
    }
    let deepy_port = match deepy_res {
        Ok(p) => p,
        Err(e) => {
            errors.push(e);
            default_deepy_port(server_port)
        }
    };
    let deepy_free = port_is_free(deepy_port);
    if !deepy_free {
        errors.push(format!(
            "Port {deepy_port} is already in use — pick another Deepy Web port or stop the occupying process."
        ));
    }
    let main_bound = !port_is_free(server_port);
    let urls = deepy_urls(deepy_port);
    Ok(serde_json::json!({
        "ok": errors.is_empty(),
        "installed": installed,
        "torch": torch,
        "portFree": deepy_free,
        "serverPort": server_port,
        "deepyPort": deepy_port,
        "mode": if lan { "lan" } else { "same-pc" },
        "clashNotice": clash_notice(main_bound, deepy_free),
        "urls": urls,
        "errors": errors,
    }))
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn deepy_web_start(
    app: tauri::AppHandle,
    mode: Option<String>,
    deepy_port: Option<u64>,
    auth_mode: Option<String>,
    auth_fixed: Option<String>,
    https_enabled: Option<bool>,
    https_cert: Option<String>,
    https_key: Option<String>,
    https_port: Option<u64>,
    public_url: Option<String>,
) -> Result<serde_json::Value, String> {
    let lan = match mode.as_deref() {
        Some("lan") => true,
        Some("same-pc") => false,
        _ => load_config_value()
            .get("deepyListen")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
    };
    let mode_label = if lan { "lan" } else { "same-pc" };
    let cfg_auth = load_config_value();
    let resolved_auth = normalize_auth_mode(
        auth_mode
            .as_deref()
            .or_else(|| cfg_auth.get("deepyAuthMode").and_then(|v| v.as_str())),
    );
    let auth_secret: Option<String> = auth_password_for_mode(&resolved_auth, auth_fixed.as_deref());
    if resolved_auth == "fixed" && auth_secret.is_none() {
        return Ok(serde_json::json!({"ok": false, "error": "Fixed auth needs a password."}));
    }
    let auth_enabled = auth_secret.is_some();
    // Slice 2 HTTPS: explicit opt-in only; cert/key resolve from args or
    // persisted desktop-config `deepyCertPath`/`deepyKeyPath`. Missing or
    // unreadable pairs block the start fail-closed (never warn-and-continue).
    let cfg_https = load_config_value();
    let https_on = https_enabled.unwrap_or(false);
    let https_cert_path = https_cert
        .filter(|v| !v.trim().is_empty())
        .or_else(|| {
            cfg_https
                .get("deepyCertPath")
                .and_then(|v| v.as_str())
                .filter(|v| !v.trim().is_empty())
                .map(|v| v.to_string())
        })
        .unwrap_or_default();
    let https_key_path = https_key
        .filter(|v| !v.trim().is_empty())
        .or_else(|| {
            cfg_https
                .get("deepyKeyPath")
                .and_then(|v| v.as_str())
                .filter(|v| !v.trim().is_empty())
                .map(|v| v.to_string())
        })
        .unwrap_or_default();
    if https_on {
        if let Err(e) = validate_cert_pair(&https_cert_path, &https_key_path) {
            return Ok(serde_json::json!({"ok": false, "error": e}));
        }
        if let Some(hp) = https_port {
            if hp == 0 || hp > 65535 {
                return Ok(
                    serde_json::json!({"ok": false, "error": format!("HTTPS port {hp} is out of range — pick 1–65535.")}),
                );
            }
        }
    }
    // Reverse-proxy origin (upstream 13.11+ `--public-url`): explicit opt-in
    // only; resolved from the start call or persisted `deepyPublicUrl`.
    // Upstream rejects `--public-url` + `--https-port` together (proxy-managed
    // HTTPS vs WanGP redirect are exclusive) — fail closed here as well.
    let public_url_raw = public_url
        .filter(|v| !v.trim().is_empty())
        .or_else(|| {
            cfg_https
                .get("deepyPublicUrl")
                .and_then(|v| v.as_str())
                .filter(|v| !v.trim().is_empty())
                .map(|v| v.to_string())
        })
        .unwrap_or_default();
    let public_url_norm = if public_url_raw.trim().is_empty() {
        None
    } else {
        match validate_public_url(&public_url_raw) {
            Ok(u) => Some(u),
            Err(e) => return Ok(serde_json::json!({"ok": false, "error": e})),
        }
    };
    if public_url_norm.is_some() && https_on && https_port.is_some() {
        return Ok(
            serde_json::json!({"ok": false, "error": "Use --public-url for proxy-managed HTTPS or the HTTPS redirect port, not both (upstream rejects the combination)."}),
        );
    }
    let repo = get_repo_dir();
    if !repo.join("wgp.py").exists() {
        return Ok(
            serde_json::json!({"ok": false, "error": "Wan2GP not installed — run Install first."}),
        );
    }
    let (_server_port, deepy_res) = resolve_ports(deepy_port);
    let port = match deepy_res {
        Ok(p) => p,
        Err(e) => return Ok(serde_json::json!({"ok": false, "error": e})),
    };
    // Serialize boots: a double-click or retry-while-loading must not spawn a
    // second model load onto the same port (double VRAM, port race).
    if DEEPY_STARTING.swap(true, Ordering::SeqCst) {
        return Ok(
            serde_json::json!({"ok": false, "error": "Deepy Web is already starting — wait for the current boot to finish."}),
        );
    }
    let _clear_starting = ClearDeepyStarting;
    // Pre-start cleanup: stop OUR strays on the target port (orphaned boots,
    // earlier runs incl. other launcher instances) so Start never fails
    // against our own leftovers. Manually-run servers are spared — only the
    // bootstrap-shim signature is swept.
    let mut pre_killed = false;
    for (pid, _) in listeners_on_port(port) {
        if process_cmdline(pid).is_some_and(|cmd| {
            should_stop_deepy_process(&cmd, port) && is_launcher_deepy_cmdline(&cmd)
        }) {
            kill_pid_deepy(pid);
            pre_killed = true;
        }
    }
    for pid in launcher_deepy_pids() {
        if process_cmdline(pid).is_some_and(|cmd| {
            should_stop_deepy_process(&cmd, port) && is_launcher_deepy_cmdline(&cmd)
        }) {
            kill_pid_deepy(pid);
            pre_killed = true;
        }
    }
    if pre_killed {
        let m = format!("[*] Stopped leftover Deepy Web process(es) on :{port} before starting…\n");
        push_log(&m, "launch");
        let _ = app.emit("launch-log", m);
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    if !port_is_free(port) {
        return Ok(
            serde_json::json!({"ok": false, "error": format!("Port {port} is already in use by another program — stop it or pick another Deepy Web port.")}),
        );
    }
    let autoconf = match ensure_deepy_config_for_web() {
        Ok(label) => label,
        Err(e) => return Ok(serde_json::json!({"ok": false, "error": e})),
    };
    // Sessions dir: multisession dedicated dir, ensured before spawn.
    let sessions_dir = repo.join("deepy_sessions");
    if let Err(e) = std::fs::create_dir_all(&sessions_dir) {
        return Ok(
            serde_json::json!({"ok": false, "error": format!("Cannot create sessions dir: {e}")}),
        );
    }
    // Shared arg ground with `launch` (verbatim upstream flags; no fork).
    let base = crate::launch::WgpLaunchBase {
        port,
        server_name: "localhost".to_string(),
        sessions_dir: sessions_dir.to_string_lossy().to_string(),
    };
    let mut args = crate::launch::build_wgp_args(&base);
    args.insert(1, "--deepy-server".to_string());
    if lan {
        args.push("--listen".to_string());
    }
    if auth_enabled {
        args.push("--auth".to_string());
    }
    if https_on {
        args.push("--ssl-certfile".to_string());
        args.push(https_cert_path.clone());
        args.push("--ssl-keyfile".to_string());
        args.push(https_key_path.clone());
        if let Some(hp) = https_port {
            args.push("--https-port".to_string());
            args.push(hp.to_string());
        }
    }
    if let Some(ref pu) = public_url_norm {
        args = build_public_url_args(args, pu);
    }
    debug_assert!(
        auth_secret
            .as_deref()
            .map(|pw| !args_contain_secret(&args, pw))
            .unwrap_or(true),
        "auth password must never appear in argv"
    );
    let emit = |msg: &str| {
        crate::base::push_log(msg, "launch");
        let _ = app.emit("launch-log", msg.to_string());
    };
    emit(&format!(
        "[Deepy] Starting Deepy Web ({mode_label}) on :{port} (auto-config: {autoconf})…\n"
    ));
    if lan {
        emit("[Deepy] Phone-LAN mode: Windows may show a firewall prompt — allow it on private networks. No firewall rules are created silently: https://github.com/deepbeepmeep/Wan2GP\n");
    }
    mutating_try("deepy-web-start")?;
    let py = resolve_py();
    let boot = std::env::temp_dir().join(format!(
        "wan2gp-deepy-bootstrap-{}-{}.py",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    if std::fs::write(
        &boot,
        r#"import os, sys, runpy
os.environ['PYTHONUNBUFFERED'] = '1'
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
sys.argv = sys.argv[1:]
d = os.path.dirname(os.path.abspath(sys.argv[0]))
if d not in sys.path: sys.path.insert(0, d)
runpy.run_path(sys.argv[0], run_name='__main__')
"#,
    )
    .is_err()
    {
        mutating_done();
        return Ok(serde_json::json!({"ok": false, "error": "Cannot write bootstrap shim."}));
    }
    args.insert(0, boot.to_string_lossy().to_string());
    for (k, v) in [
        ("PYTHONUNBUFFERED", "1"),
        ("PYTHONUTF8", "1"),
        ("PYTHONIOENCODING", "utf-8"),
        ("NO_PROXY", "localhost,127.0.0.1,::1"),
    ] {
        std::env::set_var(k, v);
    }
    let cfg = load_config_value();
    if let Some(t) = cfg.get("hfToken").and_then(|x| x.as_str()) {
        if !t.is_empty() {
            std::env::set_var("HF_TOKEN", t);
        }
    }
    use tauri_plugin_shell::ShellExt;
    let shell_cmd = app.shell().command(&py).args(&args).current_dir(&repo);
    let shell_cmd = match auth_secret.clone() {
        Some(pw) => shell_cmd.env(auth_env_key(), pw),
        None => shell_cmd,
    };
    let (rx, child) = shell_cmd.spawn().map_err(|e| {
        mutating_done();
        emit(&format!("[Deepy] ERROR: spawn failed: {e}\n"));
        e.to_string()
    })?;
    emit(&format!(
        "[Deepy] Deepy Web spawned (PID {})\n",
        child.pid()
    ));
    remember_deepy(child.pid(), mode_label);
    remember_deepy_auth(if auth_enabled {
        Some(resolved_auth.clone())
    } else {
        None
    });
    // Forward logs + collect early stderr for flag-drift diagnosis.
    let deepy_exit_port = port;
    let app2 = app.clone();
    let stderr_buf = std::sync::Arc::new(Mutex::new(String::new()));
    let stderr_buf2 = stderr_buf.clone();
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
                    let _ = app2.emit("launch-log", s.clone());
                    if let Ok(mut g) = stderr_buf2.lock() {
                        if g.len() < 4000 {
                            g.push_str(&s);
                        }
                    }
                }
                CommandEvent::Terminated(s) => {
                    let _ = app2.emit("deepy-exit", serde_json::json!({"source": "deepy", "code": s.code, "port": deepy_exit_port}));
                    break;
                }
                _ => {}
            }
        }
    });
    // Setup done — release the global mutation guard before the boot wait so
    // a slow model load never blocks installs/updates. Concurrent boots are
    // covered by DEEPY_STARTING instead.
    mutating_done();
    // Bounded boot wait: a cold boot loads a multi-GB Qwen model before the
    // port opens, so allow up to 5 minutes with progress in the launch log.
    // (The old 12s timeout orphaned slow boots: the card flipped to Stopped
    // while the process kept loading and bound the port minutes later.)
    let child_pid = child.pid();
    let mut opened = false;
    for waited in 1..=300u32 {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        if !port_is_free(port) {
            opened = true;
            break;
        }
        if waited % 30 == 0 {
            emit(&format!(
                "[Deepy] Deepy Web still booting ({waited}s) — loading the Deepy model, this can take minutes on a cold start…\n"
            ));
        }
    }
    if opened {
        let m = format!("[Deepy] ✓ Deepy Web ready on {}\n", same_pc_url(port));
        crate::base::push_log(&m, "launch");
        let _ = app.emit("launch-log", m);
        Ok(serde_json::json!({
            "ok": true, "running": true, "port": port,
            "mode": mode_label, "urls": deepy_urls(port),
            "autoconf": autoconf, "fresh": true,
            "authEnabled": auth_enabled,
            "authMode": resolved_auth,
            "authPassword": if resolved_auth == "generate" { auth_secret.clone().unwrap_or_default() } else { String::new() },
            "authExpiry": if auth_enabled { "Credentials expire within 24h / on restart." } else { "" },
            "authHttpWarning": if auth_enabled && lan { "Auth over plain HTTP sends the password unencrypted." } else { "" },
            "rateLimitNote": if auth_enabled { "Too many wrong tries throttles logins." } else { "" },
            "httpsEnabled": https_on,
            "httpsCertPath": if https_on { serde_json::Value::String(https_cert_path.clone()) } else { serde_json::Value::Null },
            "httpsKeyPath": if https_on { serde_json::Value::String(https_key_path.clone()) } else { serde_json::Value::Null },
            "httpsPort": if https_on { https_port.map(serde_json::Value::from).unwrap_or(serde_json::Value::Null) } else { serde_json::Value::Null },
            "publicUrl": public_url_norm.clone().map(serde_json::Value::from).unwrap_or(serde_json::Value::Null),
        }))
    } else {
        // Fail-closed: stop what we spawned (whole process tree) plus a
        // port-scoped sweep, and clear tracking — no slow boot may survive to
        // bind the port later behind the card's back.
        kill_pid_deepy(child_pid);
        for (pid, _) in listeners_on_port(port) {
            if process_cmdline(pid).is_some_and(|cmd| should_stop_deepy_process(&cmd, port)) {
                kill_pid_deepy(pid);
            }
        }
        clear_deepy_tracking();
        emit(&format!(
            "[Deepy] ✗ Deepy Web did not open :{port} within 5 minutes — spawned process stopped.\n"
        ));
        let early = stderr_buf.lock().map(|g| g.clone()).unwrap_or_default();
        let hint = flag_drift_hint(&early).unwrap_or_else(|| {
            "Deepy Web did not open its port — check Manage → logs for the traceback, then retry.".to_string()
        });
        Ok(
            serde_json::json!({"ok": false, "error": format!("Deepy Web failed to start on :{port}."), "hint": hint}),
        )
    }
}

/// PIDs listening on exactly `port` (deepy-scoped scan — never the main port).
fn listeners_on_port(port: u64) -> Vec<(u32, String)> {
    let mut out: Vec<(u32, String)> = Vec::new();
    #[cfg(windows)]
    {
        let ps = format!("Get-NetTCPConnection -LocalPort {port} -State Listen | ForEach-Object {{ $p = Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue; if ($p) {{ $p.Id.ToString() + '|' + $p.ProcessName }} }}");
        if let Ok(o) = silent_command("powershell")
            .args(["-NoProfile", "-Command", &ps])
            .output()
        {
            if o.status.success() {
                for line in String::from_utf8_lossy(&o.stdout).lines() {
                    let mut parts = line.splitn(2, '|');
                    if let (Some(pid_s), Some(name)) = (parts.next(), parts.next()) {
                        if let Ok(pid) = pid_s.trim().parse::<u32>() {
                            out.push((pid, name.trim().to_string()));
                        }
                    }
                }
            }
        }
    }
    #[cfg(not(windows))]
    {
        if let Ok(o) = silent_command("lsof")
            .args(["-ti", &format!("tcp:{port}")])
            .output()
        {
            if o.status.success() {
                for line in String::from_utf8_lossy(&o.stdout).lines() {
                    if let Ok(pid) = line.trim().parse::<u32>() {
                        out.push((pid, String::new()));
                    }
                }
            }
        }
    }
    out
}

fn process_cmdline(pid: u32) -> Option<String> {
    if pid == 0 || pid == std::process::id() {
        return None;
    }
    #[cfg(windows)]
    {
        let ps = format!("Get-CimInstance Win32_Process -Filter \"ProcessId='{pid}'\" | ForEach-Object {{ $_.CommandLine }}");
        let o = silent_command("powershell")
            .args(["-NoProfile", "-Command", &ps])
            .output()
            .ok()?;
        if !o.status.success() {
            return None;
        }
        let line = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if line.is_empty() {
            None
        } else {
            Some(line)
        }
    }
    #[cfg(not(windows))]
    {
        std::fs::read_to_string(format!("/proc/{pid}/cmdline"))
            .ok()
            .map(|s| s.replace('\0', " ").trim().to_string())
            .filter(|s| !s.is_empty())
    }
}

/// True when a cmdline belongs to a Deepy Web server spawned by THIS launcher:
/// our bootstrap shim (`wan2gp-deepy-bootstrap-*`) plus upstream
/// `--deepy-server`. A manually-run `python wgp.py --deepy-server` has no
/// shim and is NEVER matched — stop/start sweeps leave foreign processes alone.
pub(crate) fn is_launcher_deepy_cmdline(cmdline: &str) -> bool {
    let cl = cmdline.to_lowercase();
    cl.contains("wan2gp-deepy-bootstrap-") && cl.contains("--deepy-server")
}

/// PIDs of launcher-spawned Deepy Web processes on ANY port (orphan sweep).
/// Port-scoped `listeners_on_port` cannot see strays left on a shifted port,
/// so Stop and pre-start cleanup use this signature sweep instead.
fn launcher_deepy_pids() -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    #[cfg(windows)]
    {
        let ps = "Get-CimInstance Win32_Process -Filter \"CommandLine LIKE '%wan2gp-deepy-bootstrap-%' AND CommandLine LIKE '%--deepy-server%'\" | ForEach-Object { $_.ProcessId }";
        if let Ok(o) = silent_command("powershell")
            .args(["-NoProfile", "-Command", ps])
            .output()
        {
            if o.status.success() {
                for line in String::from_utf8_lossy(&o.stdout).lines() {
                    if let Ok(pid) = line.trim().parse::<u32>() {
                        if pid != 0 && pid != std::process::id() {
                            out.push(pid);
                        }
                    }
                }
            }
        }
    }
    #[cfg(not(windows))]
    {
        if let Ok(o) = silent_command("pgrep")
            .args(["-f", "wan2gp-deepy-bootstrap-"])
            .output()
        {
            if o.status.success() {
                for line in String::from_utf8_lossy(&o.stdout).lines() {
                    if let Ok(pid) = line.trim().parse::<u32>() {
                        if pid != 0
                            && pid != std::process::id()
                            && std::fs::read_to_string(format!("/proc/{pid}/cmdline"))
                                .map(|s| is_launcher_deepy_cmdline(&s.replace('\0', " ")))
                                .unwrap_or(false)
                        {
                            out.push(pid);
                        }
                    }
                }
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// True when the sessions standard still needs enforcing: missing,
/// upstream `disabled`, or our own legacy boolean `false`. `dedicated`,
/// `selectable` (explicit choice) and other truthy values (upstream
/// normalizes those to selectable) are left alone. Pure so it stays tested.
pub(crate) fn needs_session_upgrade(v: &serde_json::Value) -> bool {
    match v.get("deepy_multi_session") {
        None => true,
        Some(serde_json::Value::String(s)) => s == "disabled",
        Some(serde_json::Value::Bool(b)) => !b,
        Some(serde_json::Value::Number(n)) => n.as_i64().is_some_and(|i| i == 0),
        _ => false,
    }
}

/// Candidate outputs folders in priority order: configured save paths first
/// (relative ones resolved against the repo), then repo `outputs`/`output`.
/// Pure so it stays unit-tested; the opener command takes the first existing one.
pub(crate) fn outputs_candidates(cfg: &serde_json::Value, repo: &std::path::Path) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for key in ["save_path", "image_save_path", "audio_save_path"] {
        if let Some(s) = cfg
            .get(key)
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let p = std::path::Path::new(s);
            let abs = if p.is_absolute() {
                p.to_path_buf()
            } else {
                repo.join(p)
            };
            let s = abs.to_string_lossy().to_string();
            if !out.contains(&s) {
                out.push(s);
            }
        }
    }
    for d in ["outputs", "output"] {
        let s = repo.join(d).to_string_lossy().to_string();
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

/// Fast-path kill for app close: the tracked child only (whole tree), no
/// scans. The close worker's port sweep catches anything else.
pub(crate) fn stop_deepy_fast() {
    if let Some(pid) = DEEPY_PID.get().and_then(|m| m.lock().ok()).and_then(|g| *g) {
        kill_pid_deepy(pid);
    }
}

/// Deepy port to include in the app-close port sweep: configured `deepyPort`
/// or the `serverPort+1` default. None when unset/invalid or clashing with
/// the main port (already swept as a Wan2GP port). Pure so it stays tested.
pub(crate) fn deepy_sweep_port(server_port: u64, deepy_cfg: Option<u64>) -> Option<u64> {
    let p = deepy_cfg.unwrap_or_else(|| server_port.saturating_add(1));
    if p == 0 || p > 65535 || p == server_port {
        None
    } else {
        Some(p)
    }
}

fn kill_pid_deepy(pid: u32) {
    if pid == 0 || pid == std::process::id() {
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
}

/// Open the shared Wan2GP outputs folder in Explorer (first existing
/// candidate). Reports the tried path when nothing exists.
#[tauri::command]
pub async fn deepy_web_open_outputs(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    use tauri_plugin_opener::OpenerExt;
    let repo = get_repo_dir();
    let cfg: serde_json::Value = std::fs::read_to_string(repo.join("wgp_config.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null);
    let cands = outputs_candidates(&cfg, &repo);
    let dir = cands
        .iter()
        .find(|p| std::path::Path::new(p).is_dir())
        .or(cands.first());
    let Some(dir) = dir else {
        return Ok(
            serde_json::json!({"ok": false, "error": "No outputs folder configured — generate something first."}),
        );
    };
    match app.opener().open_path(dir, None::<&str>) {
        Ok(()) => Ok(serde_json::json!({"ok": true, "path": dir})),
        Err(e) => Ok(serde_json::json!({"ok": false, "error": e.to_string(), "path": dir})),
    }
}

#[tauri::command]
pub async fn deepy_web_stop(deepy_port: Option<u64>) -> Result<serde_json::Value, String> {
    let mut killed: Vec<u32> = Vec::new();
    // Port-scoped kill when a port is known: tracked child + listeners whose
    // cmdline matches (this also covers a manually-run server on the port).
    if let Some(port) = deepy_port {
        if let Some(pid) = DEEPY_PID.get().and_then(|m| m.lock().ok()).and_then(|g| *g) {
            if process_cmdline(pid).is_some_and(|cmd| should_stop_deepy_process(&cmd, port)) {
                kill_pid_deepy(pid);
                killed.push(pid);
            }
        }
        for (pid, _) in listeners_on_port(port) {
            if killed.contains(&pid) {
                continue;
            }
            if process_cmdline(pid).is_some_and(|cmd| should_stop_deepy_process(&cmd, port)) {
                kill_pid_deepy(pid);
                killed.push(pid);
            }
        }
    }
    // Signature sweep (any port): launcher-spawned strays the port scope
    // cannot see — orphans on a shifted port after a serverPort bump, boots
    // that outlived their card, other launcher instances. Manually-run
    // servers carry no shim signature and are spared.
    for pid in launcher_deepy_pids() {
        if killed.contains(&pid) {
            continue;
        }
        kill_pid_deepy(pid);
        killed.push(pid);
    }
    killed.sort_unstable();
    killed.dedup();
    std::thread::sleep(std::time::Duration::from_secs(1));
    // With a port: stopped = port free (a foreign occupant keeps it busy and
    // is correctly reported as not-stopped). Without: no strays left.
    let stopped = deepy_port
        .map(port_is_free)
        .unwrap_or_else(|| launcher_deepy_pids().is_empty());
    if stopped || !killed.is_empty() {
        clear_deepy_tracking();
    }
    Ok(serde_json::json!({"ok": stopped, "stopped": stopped, "port": deepy_port, "killed": killed}))
}

#[tauri::command]
pub async fn deepy_web_status(deepy_port: Option<u64>) -> Result<serde_json::Value, String> {
    let (server_port, deepy_res) = resolve_ports(deepy_port);
    let port = deepy_res.unwrap_or_else(|_| default_deepy_port(server_port));
    let running = !port_is_free(port);
    let cfg = load_config_value();
    let cfg_auth = normalize_auth_mode(cfg.get("deepyAuthMode").and_then(|v| v.as_str()));
    let live_auth = if running {
        last_deepy_auth().unwrap_or(cfg_auth.clone())
    } else {
        cfg_auth.clone()
    };
    let auth_on = running && last_deepy_auth().is_some();
    Ok(serde_json::json!({
        "ok": true,
        "running": running,
        "port": port,
        "mode": if running { serde_json::Value::String(last_deepy_mode()) } else { serde_json::Value::Null },
        "authEnabled": auth_on,
        "authMode": live_auth,
        "urls": deepy_urls(port),
    }))
}

/// Persist bring-own / created cert paths to launcher desktop-config
/// (`deepyCertPath` / `deepyKeyPath`). Passwords are never stored — paths only.
fn persist_cert_paths(cert_path: &str, key_path: &str) {
    let mut cfg = load_config_value();
    if let Some(m) = cfg.as_object_mut() {
        m.insert(
            "deepyCertPath".to_string(),
            serde_json::Value::String(cert_path.to_string()),
        );
        m.insert(
            "deepyKeyPath".to_string(),
            serde_json::Value::String(key_path.to_string()),
        );
        let p = get_config_file();
        if let Ok(s) = serde_json::to_string_pretty(&cfg) {
            let _ = atomic_write(&p, &s);
        }
    }
}

/// Slice 2 LAN HTTPS certificate manager.
///
/// - `bring`: validate user-supplied `.pem` / `.key` readability, persist paths.
/// - `create`: shell `mkcert` (must already be on PATH) into the launcher data
///   dir. Never runs `mkcert -install` or any silent CA trust — the phone
///   CA-install guide is the explicit next step.
#[tauri::command]
pub async fn deepy_web_cert(
    action: Option<String>,
    cert_path: Option<String>,
    key_path: Option<String>,
) -> Result<serde_json::Value, String> {
    match action.as_deref().unwrap_or("bring") {
        "bring" => {
            let cert = cert_path.unwrap_or_default();
            let key = key_path.unwrap_or_default();
            if let Err(e) = validate_cert_pair(&cert, &key) {
                return Ok(serde_json::json!({"ok": false, "error": e}));
            }
            persist_cert_paths(cert.trim(), key.trim());
            Ok(serde_json::json!({"ok": true, "certPath": cert.trim(), "keyPath": key.trim()}))
        }
        "create" => {
            let mkcert_ok = silent_command("mkcert")
                .arg("--version")
                .output()
                .is_ok_and(|o| o.status.success());
            if !mkcert_ok {
                return Ok(
                    serde_json::json!({"ok": false, "error": "mkcert not found on PATH — follow the card's 'Install mkcert on this PC' guide (Advanced), then retry. No certificates were created."}),
                );
            }
            let dir = get_data_dir().join("deepy-certs");
            if let Err(e) = std::fs::create_dir_all(&dir) {
                return Ok(
                    serde_json::json!({"ok": false, "error": format!("Cannot create cert dir: {e}")}),
                );
            }
            let pem = dir.join("lan-cert.pem");
            let key = dir.join("lan-cert-key.pem");
            let mut hosts: Vec<String> = vec![
                "localhost".to_string(),
                "127.0.0.1".to_string(),
                "::1".to_string(),
            ];
            for ip in lan_candidates() {
                if !hosts.contains(&ip) {
                    hosts.push(ip);
                }
            }
            let status = silent_command("mkcert")
                .arg("-cert-file")
                .arg(&pem)
                .arg("-key-file")
                .arg(&key)
                .args(&hosts)
                .output();
            match status {
                Ok(o) if o.status.success() => {
                    let c = pem.to_string_lossy().to_string();
                    let k = key.to_string_lossy().to_string();
                    persist_cert_paths(&c, &k);
                    Ok(
                        serde_json::json!({"ok": true, "certPath": c, "keyPath": k, "guide": "Install the mkcert CA on your phone before opening the HTTPS URL (card CA guide)."}),
                    )
                }
                Ok(o) => {
                    let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
                    Ok(
                        serde_json::json!({"ok": false, "error": format!("mkcert failed: {}", if err.is_empty() { "unknown error" } else { &err })}),
                    )
                }
                Err(e) => Ok(
                    serde_json::json!({"ok": false, "error": format!("mkcert spawn failed: {e}")}),
                ),
            }
        }
        other => Ok(
            serde_json::json!({"ok": false, "error": format!("Unknown cert action '{other}' — use bring|create.")}),
        ),
    }
}

/// Parse `tailscale status --json` into a tailnet URL.
/// Returns `Some("https://<host>.<tailnet>.ts.net")` only when logged in
/// (`BackendState == "Running"`, Self online) with a `HostName` and a
/// `MagicDNSSuffix`; absent / not-logged-in / malformed yields `None`
/// (the caller renders guidance with no URL instead).
pub(crate) fn parse_tailscale_json(raw: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    if v.get("BackendState").and_then(|s| s.as_str()) != Some("Running") {
        return None;
    }
    let this = v.get("Self")?;
    if this.get("Online").and_then(|o| o.as_bool()) == Some(false) {
        return None;
    }
    let host = this.get("HostName").and_then(|h| h.as_str())?.trim();
    let suffix = v
        .get("MagicDNSSuffix")
        .and_then(|s| s.as_str())
        .map(str::trim)
        .unwrap_or("");
    if host.is_empty() || suffix.is_empty() {
        return None;
    }
    Some(format!("https://{host}.{suffix}"))
}

/// Run `tailscale status --json` with a bounded wait. `None` on spawn
/// failure, non-zero exit (e.g. not logged in), timeout, or unreadable output.
fn tailscale_status_json() -> Option<String> {
    use std::io::Read;
    use std::time::{Duration, Instant};
    let mut child = silent_command("tailscale")
        .args(["status", "--json"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let mut s = String::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = out.read_to_string(&mut s);
                }
                return Some(s);
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => return None,
        }
    }
}

/// Slice 3 Tailscale detect: binary probe → `tailscale status` (short timeout)
/// → tailnet URL only on success. Absent / not-logged-in yields
/// `{present:false}` / `{loggedIn:false}` with no URL, plus guidance.
/// Router VPN stays docs-only (frontend text, no automation) by design.
#[tauri::command]
pub async fn deepy_web_tailscale() -> Result<serde_json::Value, String> {
    let present = silent_command("tailscale")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !present {
        return Ok(serde_json::json!({
            "ok": true, "present": false, "loggedIn": false,
            "tailnetUrl": serde_json::Value::Null,
            "guidance": "Tailscale not found — install it from https://tailscale.com/download, sign in on this PC and your phone, then retry.",
        }));
    }
    match tailscale_status_json().and_then(|s| parse_tailscale_json(&s)) {
        Some(url) => Ok(serde_json::json!({
            "ok": true, "present": true, "loggedIn": true, "tailnetUrl": url,
        })),
        None => Ok(serde_json::json!({
            "ok": true, "present": true, "loggedIn": false,
            "tailnetUrl": serde_json::Value::Null,
            "guidance": "Tailscale is installed but not logged in (or status timed out) — sign in, approve the device at login.tailscale.com if asked, then retry.",
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn red_default_port_is_server_port_plus_one() {
        assert_eq!(default_deepy_port(7860), 7861);
    }

    #[test]
    fn red_rejects_zero_port() {
        assert!(validate_deepy_port(0, 7860).is_err());
    }

    #[test]
    fn red_rejects_huge_port() {
        assert!(validate_deepy_port(99999, 7860).is_err());
    }

    #[test]
    fn red_rejects_non_numeric_text() {
        assert!(parse_deepy_port("abc", 7860).is_err());
    }

    #[test]
    fn red_rejects_port_equal_to_server_port() {
        assert!(validate_deepy_port(7860, 7860).is_err());
    }

    #[test]
    fn red_same_pc_url_uses_localhost() {
        assert_eq!(same_pc_url(7861), "http://localhost:7861");
    }

    #[test]
    fn red_phone_url_uses_real_lan_ip() {
        assert_eq!(
            phone_url("192.168.1.20", 7861).as_deref(),
            Some("http://192.168.1.20:7861")
        );
    }

    #[test]
    fn red_phone_url_never_emits_unspecified() {
        assert_eq!(phone_url("0.0.0.0", 7861), None);
    }

    #[test]
    fn red_loopback_excluded_from_lan_pick() {
        let cands = vec!["127.0.0.1".to_string(), "192.168.1.20".to_string()];
        assert_eq!(select_lan_ip(&cands).as_deref(), Some("192.168.1.20"));
    }

    #[test]
    fn red_no_lan_adapter_yields_none_for_guidance_state() {
        let cands = vec!["127.0.0.1".to_string(), "0.0.0.0".to_string()];
        assert_eq!(select_lan_ip(&cands), None);
    }

    #[test]
    fn tri_phone_prefers_real_lan_over_tailscale() {
        // Tailscale enumerates first on the reporter's PC — the Phone URL
        // must still be the same-Wi-Fi address.
        let cands = vec!["100.112.0.220".to_string(), "192.168.1.55".to_string()];
        assert_eq!(select_lan_ip(&cands).as_deref(), Some("192.168.1.55"));
        // Tailscale-only: Phone falls back to it (better than nothing).
        let cands = vec!["100.112.0.220".to_string()];
        assert_eq!(select_lan_ip(&cands).as_deref(), Some("100.112.0.220"));
    }

    #[test]
    fn tri_external_picks_tailscale_ip() {
        let cands = vec!["192.168.1.55".to_string(), "100.112.0.220".to_string()];
        assert_eq!(select_external_ip(&cands).as_deref(), Some("100.112.0.220"));
        let cands = vec!["192.168.1.55".to_string(), "127.0.0.1".to_string()];
        assert_eq!(select_external_ip(&cands), None);
    }

    // GREEN: arg composition — verbatim upstream flags.
    #[test]
    fn green_same_pc_args_exclude_listen() {
        let a = build_deepy_args(7861, false);
        assert!(a.contains(&"--deepy-server".to_string()));
        assert!(a.contains(&"--server-port".to_string()));
        assert!(a.contains(&"7861".to_string()));
        assert!(!a.contains(&"--listen".to_string()));
    }

    #[test]
    fn green_lan_args_append_listen() {
        let a = build_deepy_args(7861, true);
        assert!(a.contains(&"--listen".to_string()));
        assert_eq!(a.first().map(String::as_str), Some("wgp.py"));
    }

    // TRIANGULATE: stop scoping, clash notice, flag drift, auto-config.
    #[test]
    fn tri_stop_matches_only_deepy_process_on_port() {
        let deepy =
            "C:\\Wan2GP\\env\\python.exe wan2gp-bootstrap-1.py wgp.py --deepy-server --server-port 7861";
        assert!(should_stop_deepy_process(deepy, 7861));
        let main = "C:\\Wan2GP\\env\\python.exe wgp.py --server-port 7860 --advanced";
        assert!(!should_stop_deepy_process(main, 7861));
        let other_port = "C:\\Wan2GP\\env\\python.exe wgp.py --deepy-server --server-port 7862";
        assert!(!should_stop_deepy_process(other_port, 7861));
        assert!(!should_stop_deepy_process("notepad.exe", 7861));
    }

    #[test]
    fn tri_launcher_sig_matches_only_shim_spawns() {
        let shim = "C:\\Wan2GP\\env\\python.exe C:\\Users\\x\\AppData\\Local\\Temp\\wan2gp-deepy-bootstrap-18184-1.py wgp.py --deepy-server --server-port 7862 --server-name localhost";
        assert!(is_launcher_deepy_cmdline(shim));
        // Manual run: same flags, no shim — sweeps must spare it.
        let manual = "C:\\Wan2GP\\env\\python.exe wgp.py --deepy-server --server-port 7862";
        assert!(!is_launcher_deepy_cmdline(manual));
        // The launcher itself must never match.
        assert!(!is_launcher_deepy_cmdline(
            "D:\\dev\\target\\release\\wan2gp-desktop-launcher-tauri.exe"
        ));
        // Case-insensitive (cmdline is lowercased before matching).
        assert!(is_launcher_deepy_cmdline(
            "WAN2GP-DEEPY-BOOTSTRAP-9.PY WGP.PY --DEEPY-SERVER"
        ));
    }

    #[test]
    fn tri_deepy_sweep_port_covers_standalone() {
        assert_eq!(deepy_sweep_port(7860, None), Some(7861));
        assert_eq!(deepy_sweep_port(7861, None), Some(7862));
        assert_eq!(deepy_sweep_port(7860, Some(7862)), Some(7862));
        assert_eq!(deepy_sweep_port(7860, Some(7860)), None);
        assert_eq!(deepy_sweep_port(7860, Some(0)), None);
        assert_eq!(deepy_sweep_port(7860, Some(99999)), None);
    }

    #[test]
    fn tri_session_upgrade_only_when_disabled() {
        let missing = serde_json::json!({});
        assert!(needs_session_upgrade(&missing));
        assert!(needs_session_upgrade(
            &serde_json::json!({"deepy_multi_session": "disabled"})
        ));
        assert!(needs_session_upgrade(
            &serde_json::json!({"deepy_multi_session": false})
        ));
        assert!(needs_session_upgrade(
            &serde_json::json!({"deepy_multi_session": 0})
        ));
        assert!(!needs_session_upgrade(
            &serde_json::json!({"deepy_multi_session": "dedicated"})
        ));
        assert!(!needs_session_upgrade(
            &serde_json::json!({"deepy_multi_session": "selectable"})
        ));
        assert!(!needs_session_upgrade(
            &serde_json::json!({"deepy_multi_session": true})
        ));
    }

    #[test]
    fn tri_outputs_candidates_prefer_configured() {
        let repo = std::path::Path::new("C:/Wan2GP");
        let cfg = serde_json::json!({
            "save_path": "C:/Wan2GP-Models/outputs",
            "image_save_path": "",
            "audio_save_path": "C:/Wan2GP-Models/outputs",
        });
        let c = outputs_candidates(&cfg, repo);
        assert_eq!(
            c.first().map(String::as_str),
            Some("C:/Wan2GP-Models/outputs")
        );
        assert!(c.iter().any(|p| p.ends_with("outputs")));
        let rel = serde_json::json!({"save_path": "my-out"});
        let c2 = outputs_candidates(&rel, repo);
        assert!(c2
            .first()
            .map(String::as_str)
            .unwrap_or("")
            .contains("my-out"));
    }

    #[test]
    fn tri_clash_notice_only_when_side_by_side() {
        assert!(clash_notice(true, true).is_some());
        assert_eq!(clash_notice(false, true), None);
        assert_eq!(clash_notice(true, false), None);
    }

    #[test]
    fn tri_flag_drift_surfaces_hint() {
        let err = "wgp.py: error: unrecognized arguments: --deepy-server";
        assert!(flag_drift_hint(err).is_some());
        assert_eq!(flag_drift_hint("[bootstrap] active"), None);
    }

    #[test]
    fn tri_auto_config_disabled_goes_zero_qwen4b() {
        assert_eq!(
            auto_config_plan(0, "zero", "", Some(1), false),
            AutoPlan::ZeroPlusQwen4B
        );
    }

    #[test]
    fn tri_auto_config_fixes_stale_enhancer() {
        assert_eq!(
            auto_config_plan(1, "zero", "", Some(2), true),
            AutoPlan::FixEnhancerTo3
        );
    }

    #[test]
    fn tri_auto_config_prime_local_without_27b_blocks() {
        // Gerard's exact failure mode: Prime+local data saved (e.g. after the
        // old JS override or a profile-id/enhancer mismatch), weights absent.
        for profile in ["local-qwen38", "qwen38_27b", "qwen38"] {
            assert_eq!(
                auto_config_plan(1, "prime", profile, Some(5), false),
                AutoPlan::BlockMissing27B,
                "profile {profile} must block, not rewrite"
            );
        }
    }

    #[test]
    fn tri_auto_config_healthy_prime_is_kept() {
        assert_eq!(
            auto_config_plan(1, "prime", "opencode", Some(3), true),
            AutoPlan::Keep
        );
    }

    #[test]
    fn tri_qwen27b_roots_read_checkpoints_paths() {
        let repo = std::path::Path::new("C:/Wan2GP");
        // Gerard's real config: absolute ckpts root + "." entry (skipped).
        let cfg = serde_json::json!({"checkpoints_paths": ["C:\\Wan2GP-Models\\ckpts", "."]});
        let roots = qwen27b_search_roots(&cfg, repo);
        assert_eq!(roots.len(), 1);
        assert!(roots[0].ends_with("ckpts"));
        // Missing key falls back to <repo>/ckpts; relative entries resolve
        // against the repo (upstream files_locator runs with repo as CWD).
        let fallback = qwen27b_search_roots(&serde_json::json!({}), repo);
        assert_eq!(fallback, vec![repo.join("ckpts")]);
        let rel = qwen27b_search_roots(&serde_json::json!({"checkpoints_paths": ["ckpts"]}), repo);
        assert_eq!(rel, vec![repo.join("ckpts")]);
    }

    // Weights probe: temp-tree fixtures (no dependency on the dev machine).
    fn qwen_fixture(with_weight: bool, bare_dir_only: bool) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("fixture dir");
        let dir = tmp.path().join("ckpts").join("Qwen3_8_27B_Uncensored");
        std::fs::create_dir_all(&dir).expect("fixture assets dir");
        if with_weight && !bare_dir_only {
            std::fs::write(dir.join("Qwen3.8-27B-Uncensored-Q4_K_M.gguf"), b"x")
                .expect("fixture weight");
        }
        tmp
    }
    fn qwen_cfg_for(tmp: &tempfile::TempDir) -> serde_json::Value {
        serde_json::json!({"checkpoints_paths": [tmp.path().join("ckpts").to_string_lossy().to_string()]})
    }

    #[test]
    fn tri_qwen27b_weights_detected_per_quant() {
        for file in [
            "Qwen3.8-27B-Uncensored-Q4_K_M.gguf",
            "Qwen3.8-27B-Uncensored-noMTP-IQ3_S.gguf",
            "Qwen3.8-27B-Uncensored-IQ2_M.gguf",
        ] {
            let tmp = tempfile::tempdir().expect("fixture dir");
            let dir = tmp.path().join("ckpts").join("Qwen3_8_27B_Uncensored");
            std::fs::create_dir_all(&dir).expect("fixture assets dir");
            std::fs::write(dir.join(file), b"x").expect("fixture weight");
            let repo = tmp.path().join("repo");
            assert!(
                qwen27b_weights_present(&qwen_cfg_for(&tmp), &repo),
                "{file} must count as weights present"
            );
        }
    }

    #[test]
    fn tri_qwen27b_bare_dir_does_not_count() {
        let tmp = qwen_fixture(false, true);
        let repo = tmp.path().join("repo");
        assert!(!qwen27b_weights_present(&qwen_cfg_for(&tmp), &repo));
    }

    #[test]
    fn tri_qwen27b_missing_dir_blocks() {
        let tmp = tempfile::tempdir().expect("fixture dir");
        let repo = tmp.path().join("repo");
        let cfg = qwen_cfg_for(&tmp); // ckpts/ exists? no — nothing created
        assert!(!qwen27b_weights_present(&cfg, &repo));
        assert_eq!(
            auto_config_plan(1, "prime", "qwen38_27b", Some(5), false),
            AutoPlan::BlockMissing27B
        );
    }

    #[test]
    fn tri_prime_profile_to_ui_id_stays_in_sync() {
        // The JS override maps stored profiles through the same table — every
        // local-style id must resolve to the local UI id, never opencode.
        for p in ["qwen38_27b", "qwen38", "27b-local"] {
            assert_eq!(crate::features::prime_profile_to_ui_id(p), "local-qwen38");
        }
    }

    // Slice 1 Auth RED — env-only password (must FAIL before GREEN).
    #[test]
    fn auth_red_flag_appended_when_enabled() {
        let a = build_deepy_args_auth(7861, false, true);
        assert!(a.contains(&"--auth".to_string()));
    }

    #[test]
    fn auth_red_no_flag_when_disabled() {
        let a = build_deepy_args_auth(7861, false, false);
        assert!(!a.contains(&"--auth".to_string()));
    }

    #[test]
    fn auth_red_password_only_in_env_never_argv() {
        let pw = "s3cr3t-test-pw-auth-red";
        let args = build_deepy_args_auth(7861, true, true);
        assert_eq!(auth_env_key(), "WANGP_AUTH_PASSWORD");
        assert!(!args_contain_secret(&args, pw));
    }

    #[test]
    fn auth_red_generate_vs_fixed_modes() {
        let gen = auth_password_for_mode("generate", None);
        assert!(gen.as_deref().map(|v| !v.is_empty()).unwrap_or(false));
        let fixed = auth_password_for_mode("fixed", Some("my-fixed-secret"));
        assert_eq!(fixed.as_deref(), Some("my-fixed-secret"));
        assert_eq!(auth_password_for_mode("off", None), None);
    }

    #[test]
    fn auth_red_log_scrub_never_contains_secret() {
        let pw = "s3cr3t-log-red";
        let line = format!("starting deepy with password {pw}");
        assert!(log_line_leaks_secret(&line, pw));
        let clean = scrub_secret(&line, pw);
        assert_eq!(clean, "starting deepy with password [redacted]");
        assert!(!log_line_leaks_secret(&clean, pw));
    }

    // Multi-IP panel RED — every usable address listed with LAN/Tailscale kind.
    #[test]
    fn ips_red_tailscale_kind_detected() {
        assert_eq!(lan_ip_kind("100.112.0.176"), "tailscale");
        assert_eq!(lan_ip_kind("100.64.0.1"), "tailscale");
        assert_eq!(lan_ip_kind("192.168.1.55"), "lan");
        assert_eq!(lan_ip_kind("10.0.0.5"), "lan");
    }

    #[test]
    fn ips_red_ordered_non_linklocal_first_deduped() {
        let cands = vec![
            "169.254.10.5".to_string(),
            "192.168.1.55".to_string(),
            "100.112.0.176".to_string(),
            "192.168.1.55".to_string(),
            "127.0.0.1".to_string(),
            "0.0.0.0".to_string(),
        ];
        assert_eq!(
            ordered_lan_ips(&cands),
            vec![
                "192.168.1.55".to_string(),
                "100.112.0.176".to_string(),
                "169.254.10.5".to_string(),
            ]
        );
    }

    #[test]
    fn ips_red_panel_list_skips_linklocal() {
        let rows = phone_url_list(
            &[
                "192.168.1.55".to_string(),
                "169.254.41.106".to_string(),
                "100.112.0.176".to_string(),
            ],
            7862,
        );
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|(ip, _, _)| !ip.starts_with("169.254.")));
    }

    #[test]
    fn ips_red_phone_url_list_pairs_ip_url_kind() {
        let rows = phone_url_list(
            &["192.168.1.55".to_string(), "100.112.0.176".to_string()],
            7861,
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "192.168.1.55");
        assert_eq!(rows[0].1, "http://192.168.1.55:7861");
        assert_eq!(rows[0].2, "lan");
        assert_eq!(rows[1].2, "tailscale");
    }

    // Slice 2 HTTPS RED — cert validation (must FAIL before GREEN).
    #[test]
    fn https_red_missing_cert_blocked() {
        let r = validate_cert_pair("", "");
        assert!(r.is_err());
    }

    #[test]
    fn https_red_unreadable_key_blocked() {
        let r = validate_cert_pair("C:/certs/wangp.pem", "C:/certs/missing-key.pem");
        assert!(r.is_err());
    }

    #[test]
    fn https_red_https_args_use_verbatim_upstream_flags() {
        let a = build_https_args(
            7861,
            true,
            "C:/certs/wangp.pem",
            "C:/certs/wangp-key.pem",
            None,
        );
        assert!(a.contains(&"--ssl-certfile".to_string()));
        assert!(a.contains(&"--ssl-keyfile".to_string()));
    }

    #[test]
    fn https_red_https_port_redirects() {
        let a = build_https_args(
            7860,
            true,
            "C:/certs/wangp.pem",
            "C:/certs/wangp-key.pem",
            Some(7861),
        );
        assert!(a.contains(&"--https-port".to_string()));
        assert!(a.contains(&"7861".to_string()));
    }

    #[test]
    fn public_url_accepts_origins() {
        assert_eq!(
            validate_public_url("https://wangp.example.com").as_deref(),
            Ok("https://wangp.example.com")
        );
        assert_eq!(
            validate_public_url("https://wangp.example.com:8443").as_deref(),
            Ok("https://wangp.example.com:8443")
        );
        assert_eq!(
            validate_public_url("https://abc123xyz-7860.proxy.runpod.net").as_deref(),
            Ok("https://abc123xyz-7860.proxy.runpod.net")
        );
        // Trailing slash is normalized away (upstream accepts it).
        assert_eq!(
            validate_public_url("https://wangp.example.com/").as_deref(),
            Ok("https://wangp.example.com")
        );
        assert_eq!(
            validate_public_url("  http://127.0.0.1:7861  ").as_deref(),
            Ok("http://127.0.0.1:7861")
        );
    }

    #[test]
    fn public_url_rejects_non_origins() {
        for bad in [
            "wangp.example.com",
            "ftp://wangp.example.com",
            "https://wangp.example.com/deepy/",
            "https://wangp.example.com/deepy",
            "https://wangp.example.com?x=1",
            "https://wangp.example.com#frag",
            "https://user:pass@wangp.example.com",
            "https://wangp.example.com:99999",
            "https://wangp.example.com:abc",
            "https://",
            "",
            "https://wangp.example.com/a b",
            "https://wangp.example.com\\evil",
        ] {
            assert!(validate_public_url(bad).is_err(), "must reject: {bad:?}");
        }
    }

    #[test]
    fn public_url_args_compose_verbatim() {
        let a = build_public_url_args(vec!["wgp.py".to_string()], "https://wangp.example.com");
        assert_eq!(
            a,
            vec![
                "wgp.py".to_string(),
                "--public-url".to_string(),
                "https://wangp.example.com".to_string()
            ]
        );
    }

    // Slice 3 Tailscale RED — status-parse (must FAIL before GREEN).
    #[test]
    fn tailscale_red_valid_json_yields_tailnet_url() {
        let json = serde_json::json!({
            "BackendState": "Running",
            "MagicDNSSuffix": "tail-abc123.ts.net",
            "Self": {"HostName": "mylaptop", "Online": true},
        })
        .to_string();
        assert_eq!(
            parse_tailscale_json(&json).as_deref(),
            Some("https://mylaptop.tail-abc123.ts.net")
        );
    }

    #[test]
    fn tailscale_red_not_logged_in_yields_none() {
        let json = serde_json::json!({
            "BackendState": "NoState",
            "MagicDNSSuffix": "tail-abc123.ts.net",
            "Self": {"HostName": "mylaptop", "Online": false},
        })
        .to_string();
        assert_eq!(parse_tailscale_json(&json), None);
    }

    #[test]
    fn tailscale_red_missing_fields_yields_none() {
        let json = serde_json::json!({"BackendState": "Running"}).to_string();
        assert_eq!(parse_tailscale_json(&json), None);
    }

    #[test]
    fn tailscale_red_garbage_yields_none() {
        assert_eq!(parse_tailscale_json("not json at all"), None);
        assert_eq!(parse_tailscale_json(""), None);
    }
}

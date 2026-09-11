//! AMD/ROCm install hardening: GPU compute probe + HSA-override selection.
//!
//! `import torch` is not proof the stack runs — 0.5.2 imported fine and
//! died at the first int8 GEMM (`hipErrorInvalidValue` in quanto's
//! `qbytes_mm`). This probe runs a real workload instead: a bf16 GEMM
//! (rocBLAS path) plus the exact pattern that crashed (int8 weights ×
//! fp32 scales broadcast). Device-agnostic (`cuda` == HIP on ROCm).
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Real-GPU workload probe. Prints one JSON line on stdout; ~10-60s cold
/// (HIP init dominates). 256-wide keeps it fast on weak GPUs while still
/// exercising the kernels that failed on gfx1201.
pub(crate) const COMPUTE_PROBE: &str = r#"import json, torch
out = {"torch": torch.__version__, "cuda_available": torch.cuda.is_available(), "device": None, "gemm": None, "int8_broadcast": None}
if torch.cuda.is_available():
    out["device"] = torch.cuda.get_device_name(0)
    x = torch.randn(256, 256, device="cuda", dtype=torch.bfloat16)
    y = torch.randn(256, 256, device="cuda", dtype=torch.bfloat16)
    out["gemm"] = float((x @ y).sum().item())
    w = torch.randint(-128, 127, (256, 256), device="cuda", dtype=torch.int8)
    s = torch.rand(256, 1, device="cuda", dtype=torch.float32)
    out["int8_broadcast"] = float(((s * w).to(torch.bfloat16)).sum().item())
print(json.dumps(out))"#;

pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_secs(300);

/// Empirically chosen HSA handling, persisted per install (see
/// HSA_CHOICE_FILE). The working R9700 config sets no override at all;
/// blindly forcing 12.0.1 may mistarget kernels — so probe both and keep
/// the winner instead of guessing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HsaChoice {
    Native,
    Override(String),
}

pub(crate) const HSA_CHOICE_FILE: &str = ".amd-hsa-choice";

pub(crate) fn hsa_choice_path(repo: &Path) -> PathBuf {
    repo.join(HSA_CHOICE_FILE)
}

pub(crate) fn read_hsa_choice(repo: &Path) -> Option<HsaChoice> {
    let raw = std::fs::read_to_string(hsa_choice_path(repo)).ok()?;
    let s = raw.trim();
    if s.eq_ignore_ascii_case("native") {
        return Some(HsaChoice::Native);
    }
    if let Some(ver) = s.strip_prefix("override ").map(str::trim) {
        if !ver.is_empty() {
            return Some(HsaChoice::Override(ver.to_string()));
        }
    }
    None
}

pub(crate) fn write_hsa_choice(repo: &Path, choice: &HsaChoice) {
    let s = match choice {
        HsaChoice::Native => "native".to_string(),
        HsaChoice::Override(v) => format!("override {v}"),
    };
    let _ = std::fs::write(hsa_choice_path(repo), s);
}

/// The HSA override `setup_config.json` declares for a profile (mirrors
/// launch.rs; e.g. 12.0.1 for AMD_GFX1201). None when undeclared.
pub(crate) fn profile_hsa_version(repo: &Path, profile: &str) -> Option<String> {
    std::fs::read_to_string(repo.join("setup_config.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|c| {
            c.get("gpu_profiles")?
                .get(profile)?
                .get("env")?
                .get("HSA_OVERRIDE_GFX_VERSION")?
                .as_str()
                .map(str::to_string)
        })
}

#[derive(Debug, Clone)]
pub(crate) struct ComputeProbe {
    pub torch: String,
    pub device: String,
}

/// Run the compute probe with the given HSA handling (Some = forced
/// override, None = native). Saves/restores any pre-existing process value
/// so probes never leak into the launcher session.
pub(crate) fn run_compute_probe(py: &Path, hsa: Option<&str>) -> Result<ComputeProbe, String> {
    // Save + apply HSA for the child only; restored below on every path.
    let saved = std::env::var("HSA_OVERRIDE_GFX_VERSION").ok();
    match hsa {
        Some(v) => std::env::set_var("HSA_OVERRIDE_GFX_VERSION", v),
        None => std::env::remove_var("HSA_OVERRIDE_GFX_VERSION"),
    }
    let res = run_compute_probe_inner(py);
    match saved {
        Some(v) => std::env::set_var("HSA_OVERRIDE_GFX_VERSION", v),
        None => std::env::remove_var("HSA_OVERRIDE_GFX_VERSION"),
    }
    res
}

fn run_compute_probe_inner(py: &Path) -> Result<ComputeProbe, String> {
    let out = spawn_bounded(py, COMPUTE_PROBE, PROBE_TIMEOUT)?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr_tail = tail(&String::from_utf8_lossy(&out.stderr), 800);
    if out.status.success() {
        if let Some(probe) = parse_probe_json(&stdout) {
            return Ok(probe);
        }
        return Err(format!(
            "probe exited 0 but printed no result JSON — stdout tail: {}",
            tail(&stdout, 300)
        ));
    }
    Err(classify_probe_failure(&stderr_tail))
}

/// Spawn `py -c <script>` with captured output and a hard timeout (see
/// PROBE_TIMEOUT rationale above). Shared by the compute and kernel
/// probes so TDR-hang protection can't drift between them.
fn spawn_bounded(
    py: &Path,
    script: &str,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    use std::process::Stdio;
    let mut child = std::process::Command::new(py)
        .args(["-c", script])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("probe spawn failed ({e})"))?;
    let start = Instant::now();
    loop {
        match child
            .try_wait()
            .map_err(|e| format!("probe wait failed ({e})"))?
        {
            Some(_) => break,
            None => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("probe timed out after {}s — possible display-driver hang (TDR). Reboot, update the GPU driver, then Verify again.", timeout.as_secs()));
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
    child
        .wait_with_output()
        .map_err(|e| format!("probe output failed ({e})"))
}

/// Last `n` chars of s (log-tail helper for bounded error text).
fn tail(s: &str, n: usize) -> String {
    s.chars()
        .rev()
        .take(n)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}

/// Kernel import health (Maestro pattern): a dist can be installed yet
/// unimportable (AV-quarantined DLL, wrong-torch ABI) — version scans
/// miss that, generation crashes on it. Primary module per dist is
/// hardcoded (stable upstream names); unknown dists fall back to the
/// first top_level entry. Extra top_level modules are attempted with
/// failures reported as warnings only. No GPU init — but cold torch
/// import still costs, so Verify-click only, never background ticks.
/// Prints one JSON line: {dist: {version, import, extra}}.
/// `import` is "ok" | "missing" | "<Mod>: <Err>".
pub(crate) const KERNEL_IMPORT_PROBE: &str = r#"import importlib, importlib.metadata as md, json
PRIMARY = {"torch": "torch", "triton": "triton", "sageattention": "sageattention", "flash-attn": "flash_attn", "nunchaku": "nunchaku", "lightx2v-kernel": "lightx2v_kernel", "optimum-quanto": "optimum.quanto"}
DISTS = ["torch", "triton", "sageattention", "sageattn3", "flash-attn", "nunchaku", "lightx2v-kernel", "optimum-quanto", "llamacpp-gguf-cuda"]
out = {}
for d in DISTS:
    try: ver = md.version(d)
    except Exception: ver = None
    if ver is None:
        out[d] = {"version": None, "import": "missing", "extra": ""}; continue
    try: tops = (md.distribution(d).read_text("top_level.txt") or "").split()
    except Exception: tops = []
    prim = PRIMARY.get(d, tops[0] if tops else d)
    try:
        importlib.import_module(prim)
        status, warns = "ok", []
    except Exception as e:
        status, warns = f"{prim}: {type(e).__name__}", []
    for t in tops:
        if t == prim: continue
        try: importlib.import_module(t)
        except Exception as e: warns.append(f"{t}: {type(e).__name__}")
    out[d] = {"version": ver, "import": status, "extra": "; ".join(warns)}
try:
    import torch
    out["quanto_qbytes_mm"] = hasattr(torch.ops.quanto, "qbytes_mm")
except Exception:
    out["quanto_qbytes_mm"] = False
try:
    from sageattention import _qattn_sm89; out["sage2_symbol"] = True
except Exception:
    out["sage2_symbol"] = False
print(json.dumps(out))"#;

pub(crate) const KERNEL_PROBE_TIMEOUT: Duration = Duration::from_secs(180);

/// Run the kernel import probe. Ok(map) whenever the script itself ran —
/// per-dist breakage is DATA (import != ok), not a probe failure.
pub(crate) fn run_kernel_probe(
    py: &Path,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let out = spawn_bounded(py, KERNEL_IMPORT_PROBE, KERNEL_PROBE_TIMEOUT)?;
    if !out.status.success() {
        let tail_s = tail(&String::from_utf8_lossy(&out.stderr), 500);
        return Err(format!("kernel probe failed to run: {tail_s}"));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    parse_kernel_json(&stdout).ok_or_else(|| "kernel probe printed no result JSON".to_string())
}

/// Last `{...}` line → per-dist map. Pure (tested). A dist counts as
/// broken only when installed AND its primary import failed; "missing"
/// is neutral (presence is the version scan's job).
pub(crate) fn parse_kernel_json(
    stdout: &str,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with('{'))?;
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    v.as_object().cloned()
}

/// Installed-but-unimportable dists: `dist: detail`. Empty = all healthy
/// (missing dists ignored). Pure (tested).
pub(crate) fn kernel_probe_failures(
    map: &serde_json::Map<String, serde_json::Value>,
) -> Vec<String> {
    let mut out = Vec::new();
    for (dist, v) in map {
        if dist == "sage2_symbol" || dist == "quanto_qbytes_mm" {
            continue;
        }
        let status = v.get("import").and_then(|s| s.as_str()).unwrap_or("");
        if status != "ok" && status != "missing" && !status.is_empty() {
            let ver = v.get("version").and_then(|s| s.as_str()).unwrap_or("?");
            out.push(format!("{dist} {ver}: {status}"));
        }
    }
    out
}

/// Known-stale kernel versions in a kernel-probe map: GGUF builds older
/// than the 1.0.21 floor look healthy (import ok) while silently disabling
/// the SM120 async path — 6 tok/s Deepy decode instead of 37 (#2274).
/// Triton floors are deliberately NOT flagged: RTX_20 boxes want <3.3 per
/// upstream docs, so "old" triton is profile-relative, not stale.
/// Pure + unit-tested.
pub(crate) fn kernel_probe_stale(map: &serde_json::Map<String, serde_json::Value>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(v) = map
        .get("llamacpp-gguf-cuda")
        .and_then(|d| d.get("version"))
        .and_then(|s| s.as_str())
    {
        if crate::hw::version_gt(crate::hw::GGUF_FLOOR, v) {
            out.push(format!("llamacpp-gguf-cuda {v} predates the {} SM120 kernels — Deepy decode falls back to slow PyTorch SDPA. Re-run Sync kernels.", crate::hw::GGUF_FLOOR));
        }
    }
    out
}

/// Blackwell-or-newer GPU by display name (nvidia-smi / torch device
/// name). Sage3's kernel is Blackwell-only — proved by isolated probe
/// (branch feature/sage3-isolated-probe): installs+imports on py3.13 but
/// `RuntimeError: only supports Blackwell GPUs or newer` on a 3080.
/// SKU digits only: old "Quadro RTX 5000" (Turing) contains "RTX 50",
/// and "RTX 5000 Ada" is Ada — ADA always excludes. Pure + unit-tested.
pub(crate) fn is_blackwell_gpu(name: &str) -> bool {
    let g = name.to_uppercase();
    if g.contains("ADA") {
        return false;
    }
    [
        "5090",
        "5080",
        "5070",
        "5060",
        "5050",
        "PRO 6000",
        "PRO 5000",
        "PRO 4000",
        "B100",
        "B200",
        "GB100",
        "BLACKWELL",
    ]
    .iter()
    .any(|t| g.contains(t))
}

/// Sage3 gate verdict for a kernel-probe map (#2280). Sage3 is never
/// synced (Blackwell-only AND py>=3.12 wheels; managed envs are
/// py3.10/3.11) — so this explains instead of installing:
/// - stray install + runnable GPU → info it exists;
/// - stray install + older GPU → warn it is inert, safe to remove;
/// - nothing installed + Blackwell → info why sync skipped it;
/// - nothing installed + older GPU → silent (common case, no noise).
/// Returns (level, message). Pure + unit-tested.
pub(crate) fn sage3_note(
    map: &serde_json::Map<String, serde_json::Value>,
    blackwell: bool,
) -> Option<(&'static str, String)> {
    let (installed, ver, import) = match map.get("sageattn3") {
        Some(v) => {
            let imp = v.get("import").and_then(|s| s.as_str()).unwrap_or("");
            if imp == "missing" {
                (false, None, imp)
            } else {
                (true, v.get("version").and_then(|s| s.as_str()), imp)
            }
        }
        None => (false, None, "missing"),
    };
    match (installed, blackwell) {
        (false, false) => None,
        (false, true) => Some(("info", "SageAttention 3 needs Python ≥ 3.12 (this env is 3.10/3.11) — Sage 2.2.0 stays synced; nothing to fix.".into())),
        (true, true) if import == "ok" => Some(("info", format!("sageattn3 {} present on a Blackwell GPU — usable if upstream enables it.", ver.unwrap_or("?")))),
        (true, false) if import == "ok" => Some(("warn", format!("sageattn3 {} is installed but Blackwell-only — inert on this GPU; safe to uninstall.", ver.unwrap_or("?")))),
        (true, _) => Some(("warn", format!("sageattn3 {} won't import ({import}) — needs triton plus a Blackwell GPU; harmless, safe to remove.", ver.unwrap_or("?")))),
    }
}

/// Last `{...}` line of probe stdout → structured result. Pure (tested).
pub(crate) fn parse_probe_json(stdout: &str) -> Option<ComputeProbe> {
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with('{'))?;
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v.get("cuda_available").and_then(|b| b.as_bool()) != Some(true) {
        return None;
    }
    Some(ComputeProbe {
        torch: v
            .get("torch")
            .and_then(|t| t.as_str())
            .unwrap_or("?")
            .to_string(),
        device: v
            .get("device")
            .and_then(|d| d.as_str())
            .unwrap_or("?")
            .to_string(),
    })
}

/// Map probe stderr to a human-actionable error. Pure (tested).
pub(crate) fn classify_probe_failure(stderr_tail: &str) -> String {
    let low = stderr_tail.to_lowercase();
    if low.contains("modulenotfounderror") && low.contains("torch")
        || low.contains("no module named torch")
    {
        return "torch won't import in the new env (install incomplete?) — rebuild the environment.".into();
    }
    for sig in [
        "hiperror",
        "acceleratorerror",
        "cuda error",
        "miopen",
        "hipblas",
        "rocblas",
    ] {
        if low.contains(sig) {
            return format!("GPU kernel failure on this torch build ({sig}): {stderr_tail}");
        }
    }
    if low.contains("out of memory") || low.contains("hip out of memory") {
        return "probe ran out of GPU memory (close GPU apps / browsers and retry).".into();
    }
    format!("compute probe failed: {stderr_tail}")
}

#[cfg(test)]
mod probe_tests {
    use super::{
        classify_probe_failure, is_blackwell_gpu, kernel_probe_failures, kernel_probe_stale,
        parse_kernel_json, parse_probe_json, read_hsa_choice, sage3_note, write_hsa_choice,
        HsaChoice, COMPUTE_PROBE, KERNEL_IMPORT_PROBE,
    };
    #[test]
    fn probe_script_is_valid_python() {
        // No torch on CI hosts — syntax-check only (same pattern as the
        // setup.py override patch test).
        let tmp = std::env::temp_dir().join(format!("wgp-probe-syntax-{}.py", std::process::id()));
        std::fs::write(&tmp, COMPUTE_PROBE).unwrap();
        let arg = tmp.to_string_lossy().to_string();
        let ok = std::process::Command::new("python")
            .args([
                "-c",
                "import ast,sys; ast.parse(open(sys.argv[1]).read())",
                &arg,
            ])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(true);
        let _ = std::fs::remove_file(&tmp);
        assert!(ok, "COMPUTE_PROBE is not valid Python");
    }
    #[test]
    fn parses_probe_json() {
        let good = "torch warning: blah\n{\"torch\": \"2.12.0+rocm7.15\", \"cuda_available\": true, \"device\": \"AMD Radeon AI PRO R9700\", \"gemm\": 1.5, \"int8_broadcast\": 2.5}\n";
        let p = parse_probe_json(good).expect("should parse");
        assert_eq!(p.torch, "2.12.0+rocm7.15");
        assert_eq!(p.device, "AMD Radeon AI PRO R9700");
        // cuda unavailable → None (honest fail, not a pass).
        assert!(parse_probe_json("{\"torch\": \"x\", \"cuda_available\": false}").is_none());
        assert!(parse_probe_json("no json here").is_none());
    }
    #[test]
    fn classifies_failures() {
        let hip = "torch.AcceleratorError: CUDA error: invalid argument (hipErrorInvalidValue)";
        assert!(classify_probe_failure(hip).contains("GPU kernel failure"));
        assert!(
            classify_probe_failure("ModuleNotFoundError: No module named 'torch'")
                .contains("won't import")
        );
        assert!(
            classify_probe_failure("torch.cuda.OutOfMemoryError: out of memory")
                .contains("out of GPU memory")
        );
        assert!(classify_probe_failure("weird new error").contains("compute probe failed"));
    }
    #[test]
    fn kernel_probe_script_is_valid_python() {
        // Same pattern as the compute probe: syntax-check only, no torch.
        let tmp = std::env::temp_dir().join(format!("wgp-kprobe-syntax-{}.py", std::process::id()));
        std::fs::write(&tmp, KERNEL_IMPORT_PROBE).unwrap();
        let arg = tmp.to_string_lossy().to_string();
        let ok = std::process::Command::new("python")
            .args([
                "-c",
                "import ast,sys; ast.parse(open(sys.argv[1]).read())",
                &arg,
            ])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(true);
        let _ = std::fs::remove_file(&tmp);
        assert!(ok, "KERNEL_IMPORT_PROBE is not valid Python");
    }
    #[test]
    fn kernel_results_classify() {
        // Broken flash (AV-quarantined DLL shape) + missing lightx2v +
        // healthy sage: only flash fails; missing is neutral.
        let raw = r#"{"torch": {"version": "2.10.0+cu130", "import": "ok", "extra": ""}, "flash_attn": {"version": "2.8.3", "import": "flash_attn: DLL load failed", "extra": ""}, "lightx2v-kernel": {"version": null, "import": "missing", "extra": ""}, "sageattention": {"version": "2.2.0", "import": "ok", "extra": ""}, "sage2_symbol": true, "quanto_qbytes_mm": true}"#;
        let map = parse_kernel_json(raw).expect("should parse");
        let fails = kernel_probe_failures(&map);
        assert_eq!(fails.len(), 1);
        assert!(
            fails[0].contains("flash_attn") && fails[0].contains("DLL load failed"),
            "got {fails:?}"
        );
        // All healthy → empty (symbol/op flags never fail).
        let raw_ok = r#"{"torch": {"version": "x", "import": "ok", "extra": ""}, "sage2_symbol": false, "quanto_qbytes_mm": false}"#;
        assert!(kernel_probe_failures(&parse_kernel_json(raw_ok).unwrap()).is_empty());
        assert!(parse_kernel_json("no json").is_none());
    }
    #[test]
    fn hsa_choice_round_trip() {
        let repo = std::env::temp_dir().join(format!("wgp-hsa-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&repo);
        assert_eq!(read_hsa_choice(&repo), None);
        write_hsa_choice(&repo, &HsaChoice::Native);
        assert_eq!(read_hsa_choice(&repo), Some(HsaChoice::Native));
        write_hsa_choice(&repo, &HsaChoice::Override("12.0.1".into()));
        assert_eq!(
            read_hsa_choice(&repo),
            Some(HsaChoice::Override("12.0.1".into()))
        );
        std::fs::write(repo.join(super::HSA_CHOICE_FILE), "garbage!!").unwrap();
        assert_eq!(read_hsa_choice(&repo), None);
        let _ = std::fs::remove_dir_all(&repo);
    }
    #[test]
    fn stale_gguf_flagged_floor_passes() {
        // #2274: 1.0.2 imports fine but silently disables the SM120 async
        // path — Verify must say so instead of reporting healthy.
        let raw_old =
            r#"{"llamacpp-gguf-cuda": {"version": "1.0.2", "import": "ok", "extra": ""}}"#;
        let stale = kernel_probe_stale(&parse_kernel_json(raw_old).unwrap());
        assert_eq!(stale.len(), 1);
        assert!(
            stale[0].contains("1.0.2") && stale[0].contains("Sync kernels"),
            "got {stale:?}"
        );
        let raw_floor =
            r#"{"llamacpp-gguf-cuda": {"version": "1.0.21", "import": "ok", "extra": ""}}"#;
        assert!(kernel_probe_stale(&parse_kernel_json(raw_floor).unwrap()).is_empty());
        let raw_missing = r#"{"torch": {"version": "x", "import": "ok", "extra": ""}}"#;
        assert!(kernel_probe_stale(&parse_kernel_json(raw_missing).unwrap()).is_empty());
    }
    #[test]
    fn blackwell_names() {
        // Proved Blackwell (isolated probe ran on a 3080 → refused).
        for n in [
            "NVIDIA GeForce RTX 5090",
            "NVIDIA GeForce RTX 5070 Laptop GPU",
            "NVIDIA RTX PRO 6000 Blackwell",
            "NVIDIA B200",
        ] {
            assert!(is_blackwell_gpu(n), "{n}");
        }
        // Older / other-arch must stay silent: note the Turing-Quadro
        // and Ada traps (both contain Blackwell-looking tokens).
        for n in [
            "NVIDIA GeForce RTX 3080",
            "NVIDIA GeForce RTX 4090",
            "NVIDIA RTX 5000 Ada Generation",
            "Quadro RTX 5000",
            "NVIDIA H100",
            "AMD Radeon RX 7900 XTX",
            "",
        ] {
            assert!(!is_blackwell_gpu(n), "{n}");
        }
    }
    #[test]
    fn sage3_gate_levels() {
        // #2280: explain, never install. Common case stays silent.
        let missing =
            parse_kernel_json(r#"{"torch": {"version": "x", "import": "ok", "extra": ""}}"#)
                .unwrap();
        assert!(sage3_note(&missing, false).is_none());
        let (lvl, msg) = sage3_note(&missing, true).expect("blackwell wants an explanation");
        assert_eq!(lvl, "info");
        assert!(msg.contains("3.12") && msg.contains("2.2.0"), "got {msg}");
        // Stray install on a runnable GPU: info. On an older GPU: warn.
        let ok = parse_kernel_json(
            r#"{"sageattn3": {"version": "1.0.0", "import": "ok", "extra": ""}}"#,
        )
        .unwrap();
        assert_eq!(sage3_note(&ok, true).unwrap().0, "info");
        let (lvl, msg) = sage3_note(&ok, false).unwrap();
        assert_eq!(lvl, "warn");
        assert!(msg.contains("inert"), "got {msg}");
        // Broken stray install: warn with the import detail.
        let broken = parse_kernel_json(r#"{"sageattn3": {"version": "1.0.0", "import": "sageattn3: ModuleNotFoundError", "extra": ""}}"#).unwrap();
        let (lvl, msg) = sage3_note(&broken, false).unwrap();
        assert_eq!(lvl, "warn");
        assert!(msg.contains("won't import"), "got {msg}");
    }
}

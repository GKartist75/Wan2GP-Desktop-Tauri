//! Packages, memory profiles, auto-tune, Deepy, LLM engines, notifier, settings.
use crate::base::*;
use crate::{
    hw::{get_gpu_info_sync, kernel_profile_key},
    status::{get_active_env, resolve_env_python},
};
use std::path::PathBuf;
use tauri::Emitter;

/// Packages the launcher manages outside PyPI and therefore must never be
/// offered as single-package upgrades (issue #54 follow-up).
///
/// Two sources, neither visible in `requirements.txt`/`constraints.txt`:
/// - `gpu-profile`: torch / torchaudio / torchvision / triton(-windows) come
///   from the pytorch CUDA/ROCm index pinned by the GPU profile. A bare
///   `pip install --upgrade torch` resolves to the PyPI CPU-only wheel and
///   silently breaks CUDA (`cuda.is_available() == False`).
/// - `kernel-wheel`: flash-attn / sageattention / spas-sage-attn / nunchaku /
///   llamacpp-gguf-cuda / lightx2v-kernel are `setup_config.json`
///   `components.kernels[*].cmd.win` wheels (e.g. deepbeepmeep kernels).
///   PyPI has no Windows wheels for flash-attn, so the upgrade resolves to
///   the sdist and fails in the isolated build env (`No module named
///   'torch'`); `2.8.3.post1` vs `2.8.3` is packaging-only anyway.
///
/// Name match is case-insensitive with `-`/`_` equivalent. A `+cuXXX` /
/// `+rocm` / `+cuda` local version also pins any dist as `gpu-profile`.
/// Pure + unit-tested.
pub(crate) fn managed_pin_source(dist: &str, installed: &str) -> Option<&'static str> {
    let norm = dist.to_ascii_lowercase().replace('_', "-");
    let name = norm.as_str();
    // GPU-profile-indexed dists (pytorch CUDA/ROCm index, not PyPI).
    if name == "torch" || name == "torchaudio" || name == "torchvision" {
        return Some("gpu-profile");
    }
    if name == "triton" || name == "triton-windows" {
        return Some("gpu-profile");
    }
    // setup_config.json kernel wheels.
    if name == "flash-attn"
        || name == "sageattention"
        || name == "spas-sage-attn"
        || name == "nunchaku"
        || name == "llamacpp-gguf-cuda"
        || name == "lightx2v-kernel"
    {
        return Some("kernel-wheel");
    }
    // Local CUDA/ROCm build tag (e.g. torch 2.10.0+cu130): managed even
    // under an unexpected dist name.
    let local = installed.to_ascii_lowercase();
    if local.contains("+cu") || local.contains("+rocm") || local.contains("+cuda") {
        return Some("gpu-profile");
    }
    None
}

/// Dist name without any version pin (`flash-attn==2.8.3` -> `flash-attn`).
fn dist_name_only(pkg: &str) -> &str {
    let cut = pkg.find(|c| "<>=!~; [".contains(c));
    match cut {
        Some(i) => pkg[..i].trim(),
        None => pkg.trim(),
    }
}
/// Why a single-package upgrade must be refused (`None` = allowed).
/// Managed dists (GPU-profile / kernel wheels) are always refused;
/// requirements-capped dists (`==` pins, ceilings) are refused when the
/// tested set is known. Floors, bare names, and unknown dists pass.
/// `force` bypass lives in the command itself — this stays pure. Pure.
pub(crate) fn refuse_upgrade_reason(pkg: &str, req_text: Option<&str>) -> Option<String> {
    if let Some(src) = managed_pin_source(dist_name_only(pkg), "") {
        let hint = match src {
            "gpu-profile" => "reinstall the GPU profile / PyTorch CUDA build instead",
            _ => "use Sync Kernels / reinstall instead",
        };
        return Some(format!(
            "refused: '{pkg}' is {src}-managed ({hint}) — single-package upgrade would break the env"
        ));
    }
    if let Some(text) = req_text {
        if requirements_caps_dist(text, dist_name_only(pkg), None) {
            return Some(format!(
                "refused: '{pkg}' is pinned by requirements.txt — single-package upgrade would deviate from the tested set; use restore"
            ));
        }
    }
    None
}

/// Numeric dotted-version equality via the shared `version_gt` comparator
/// (non-numeric tails ignored, missing components zero). Pure.
fn version_eq(a: &str, b: &str) -> bool {
    !crate::hw::version_gt(a, b) && !crate::hw::version_gt(b, a)
}

/// Numeric prefix match for `==1.4.*` and `~=` (`"1.4.7"` matches `"1.4"`).
/// Pure.
fn version_prefix_match(version: &str, prefix: &str) -> bool {
    fn nums(s: &str) -> Vec<u64> {
        s.split('.')
            .map(|p| {
                p.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0)
            })
            .collect()
    }
    let (v, p) = (nums(version), nums(prefix));
    if v.len() < p.len() {
        return false;
    }
    v[..p.len()] == p[..]
}

/// Whether pip's `latest` satisfies one PEP 440 clause (`==0.36.0`,
/// `>=1.1.1`, `~=1.4.2`, `!=2.0`, …). Unparseable clauses fail open
/// (satisfied) so an unknown operator never hides an update. Pure.
fn spec_clause_satisfied(clause: &str, latest: &str) -> bool {
    let c = clause.trim();
    if c.is_empty() {
        return true;
    }
    // Operators longest-first so `>=` wins over `>`, `===` over `==`.
    let (op, ver) = ["===", "==", "!=", "~=", "<=", ">=", "<", ">"]
        .iter()
        .find_map(|op| c.strip_prefix(op).map(|v| (*op, v.trim())))
        .unwrap_or(("", c));
    // Wildcard prefix (`==1.4.*`).
    if op == "==" && ver.ends_with(".*") {
        return version_prefix_match(latest, ver.trim_end_matches(".*"));
    }
    match op {
        "" => true, // bare (shouldn't reach here) — fail open
        "==" | "===" => version_eq(latest, ver),
        "!=" => !version_eq(latest, ver),
        ">=" => version_eq(latest, ver) || crate::hw::version_gt(latest, ver),
        ">" => crate::hw::version_gt(latest, ver),
        "<=" => version_eq(latest, ver) || crate::hw::version_gt(ver, latest),
        "<" => crate::hw::version_gt(ver, latest),
        "~=" => {
            // Compatible release: `~=1.4.2` means `>=1.4.2, ==1.4.*`.
            let mut parts: Vec<&str> = ver.split('.').collect();
            if parts.len() < 2 {
                return version_eq(latest, ver)
                    || crate::hw::version_gt(latest, ver);
            }
            parts.pop();
            let prefix = parts.join(".");
            (version_eq(latest, ver) || crate::hw::version_gt(latest, ver))
                && version_prefix_match(latest, &prefix)
        }
        _ => true,
    }
}

/// Applicable requirement spec for a dist, honoring environment markers.
/// Unlike `required_spec_in_requirements` (first match wins, markers
/// ignored), marker-gated lines only apply when `py` is known and the
/// marker holds; with unknown `py` they are skipped (fail-open: an
/// unjudgeable line must not hide updates). Pure.
fn applicable_spec_in_requirements(
    text: &str,
    dist: &str,
    py: Option<(u32, u32)>,
) -> Option<String> {
    let want = dist.to_ascii_lowercase().replace('_', "-");
    for raw_line in text.lines() {
        let mut line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('-') {
            continue;
        }
        if line.contains("://") {
            continue;
        }
        if let Some(i) = line.find('#') {
            line = line[..i].trim();
        }
        let (req, marker) = match line.find(';') {
            Some(i) => (line[..i].trim(), Some(line[i + 1..].trim())),
            None => (line, None),
        };
        if let Some(m) = marker {
            match py {
                Some(v) => {
                    if !crate::install::pin_marker_applies(m, v) {
                        continue;
                    }
                }
                None => continue,
            }
        }
        if req.is_empty() {
            continue;
        }
        let cut = req.find(|c: char| "<>=!~".contains(c) || c.is_whitespace());
        let (mut name, spec) = match cut {
            Some(i) => (req[..i].trim(), req[i..].trim()),
            None => (req, ""),
        };
        if let Some(b) = name.find('[') {
            name = name[..b].trim();
        }
        if name.to_ascii_lowercase().replace('_', "-") != want {
            continue;
        }
        if spec.is_empty() {
            return Some(String::new());
        }
        return Some(spec.split_whitespace().next().unwrap_or("").to_string());
    }
    None
}

/// Requirements pin verdict: `Some(spec)` when pip's `latest` would violate
/// the tested `requirements.txt` spec for this dist (exact `==` pins,
/// ceilings) — the "update" must not be offered. `None` when upgradable
/// (floors satisfied, bare names), missing, or unjudgeable. Pure.
pub(crate) fn requirements_pin_for(
    text: &str,
    dist: &str,
    latest: &str,
    py: Option<(u32, u32)>,
) -> Option<String> {
    let spec = applicable_spec_in_requirements(text, dist, py)?;
    if spec.is_empty() {
        return None;
    }
    let latest = latest.trim();
    if latest.is_empty() {
        return None;
    }
    let ok = spec
        .split(',')
        .all(|c| spec_clause_satisfied(c.trim(), latest));
    if ok {
        None
    } else {
        Some(spec)
    }
}

/// Whether the requirements spec caps a dist at all (any `==`/`!=`/`<`/
/// `<=`/`~=` clause). Used by `upgrade_package`, which doesn't know pip's
/// `latest` — a capped dist refuses every `--upgrade`. Floors (`>=`, `>`)
/// and bare names are not caps. Pure.
pub(crate) fn requirements_caps_dist(
    text: &str,
    dist: &str,
    py: Option<(u32, u32)>,
) -> bool {
    let Some(spec) = applicable_spec_in_requirements(text, dist, py) else {
        return false;
    };
    spec.split(',').any(|c| {
        let c = c.trim();
        let op = ["===", "==", "!=", "~=", "<=", ">=", "<", ">"]
            .iter()
            .find(|op| c.starts_with(*op));
        // Floors (`>=`, `>`), bare fragments, and unknown operators are not
        // caps; everything else (`==`, `!=`, `<=`, `<`, `~=`) is.
        match op.map(|s| *s) {
            Some("===") | Some("==") | Some("!=") | Some("~=") | Some("<=")
            | Some("<") => true,
            _ => false,
        }
    })
}

/// Active env's interpreter major.minor from the warm status cache (the
/// dashboard's version scan already probed it). None when unknown — callers
/// fail open on marker-gated lines rather than spawning a probe per check.
fn cached_env_python_version() -> Option<(u32, u32)> {
    let g = crate::base::LAST_STATUS.get()?.lock().ok()?;
    let v = g.as_ref()?.2.get("versions")?.get("python")?.as_str()?;
    let mut it = v.split('.').map(|p| {
        p.chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse::<u32>()
            .unwrap_or(0)
    });
    Some((it.next()?, it.next()?))
}

#[tauri::command]
pub async fn check_package_updates(
    app: tauri::AppHandle,
    versions: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let _ = versions;
    let Some(py) = env_python_bin() else {
        return Ok(serde_json::json!([]));
    };
    use tauri_plugin_shell::process::CommandEvent;
    use tauri_plugin_shell::ShellExt;
    let (mut rx, _) = app
        .shell()
        .command(&py)
        .args(["-m", "pip", "list", "--outdated", "--format=json"])
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut out = String::new();
    while let Some(ev) = rx.recv().await {
        match ev {
            CommandEvent::Stdout(b) => out.push_str(&String::from_utf8_lossy(&b)),
            CommandEvent::Stderr(b) => out.push_str(&String::from_utf8_lossy(&b)),
            _ => {}
        }
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&out) {
        if let Some(arr) = v.as_array() {
            let dist_to_key = |d: &str| -> String {
                match d.to_lowercase().replace('-', "_").as_str() {
                    "triton_windows" => "triton".into(),
                    "opencv_python" => "opencv".into(),
                    "huggingface_hub" => "huggingface_hub".into(),
                    "spas_sage_attn" => "sageattention".into(),
                    "flash_attn" => "flash_attn".into(),
                    other => other.into(),
                }
            };
            // Tested-set gate (#54 follow-up): a pip `latest` that violates
            // requirements.txt must not be offered. Missing file degrades to
            // the managed-dist gate only.
            let req_text =
                std::fs::read_to_string(get_repo_dir().join("requirements.txt")).ok();
            let py = cached_env_python_version();
            let res: Vec<serde_json::Value> = arr.iter().map(|e| {
            let dist = e.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let installed = e.get("version").and_then(|v| v.as_str()).unwrap_or("");
            let latest = e.get("latest_version").and_then(|v| v.as_str()).unwrap_or("");
            // Managed dists (GPU-profile / kernel wheels) always win: they
            // are pinned even when requirements.txt is silent about them
            // (torch / flash_attn are GPU-profile-managed, not PyPI).
            let pin = managed_pin_source(dist, installed);
            let (pinned, pin_source, pin_spec) = match pin {
                Some(src) => (true, src.to_string(), String::new()),
                None => match req_text.as_deref().and_then(|t| requirements_pin_for(t, dist, latest, py)) {
                    Some(spec) => (true, "requirements".to_string(), spec),
                    None => (false, String::new(), String::new()),
                },
            };
            serde_json::json!({"name": dist_to_key(dist), "dist": dist, "installed": e.get("version").cloned().unwrap_or(serde_json::Value::Null), "latest": e.get("latest_version").cloned().unwrap_or(serde_json::Value::Null),
                "pinned": pinned, "pinSource": pin_source, "pinSpec": pin_spec})
        }).collect();
            return Ok(serde_json::Value::Array(res));
        }
    }
    Ok(serde_json::json!([]))
}
/// Required version specifier for a distribution as pinned in the Wan2GP
/// repo's requirements.txt (`hf_xet>=1.5.2` -> `Some(">=1.5.2")`, bare
/// `tqdm` -> `Some("")`). Unlike `parse_requirement_pins` (exact `==` only)
/// this keeps any single PEP 440 operator so cards can verdict `>=` floors.
/// Skips comments, options, URLs, and marker-only mismatches; name match is
/// case-insensitive with `-`/`_` equivalent. Pure + unit-tested.
pub(crate) fn required_spec_in_requirements(text: &str, dist: &str) -> Option<String> {
    let want = dist.to_ascii_lowercase().replace('_', "-");
    for raw_line in text.lines() {
        let mut line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('-') {
            continue;
        }
        if let Some(i) = line.find('#') {
            line = line[..i].trim();
        }
        if let Some(i) = line.find(';') {
            line = line[..i].trim();
        }
        if line.is_empty() || line.contains("://") {
            continue;
        }
        let cut = line.find(|c: char| "<>=!~".contains(c) || c.is_whitespace());
        let (mut name, spec) = match cut {
            Some(i) => (line[..i].trim(), line[i..].trim()),
            None => (line, ""),
        };
        if let Some(b) = name.find('[') {
            name = name[..b].trim();
        }
        if name.to_ascii_lowercase().replace('_', "-") != want {
            continue;
        }
        if spec.is_empty() {
            return Some(String::new());
        }
        return Some(spec.split_whitespace().next().unwrap_or("").to_string());
    }
    None
}
#[tauri::command]
pub fn check_package(pkg: String) -> serde_json::Value {
    // Real probe: importlib version from the active env (aliases map import names to dist names).
    let dist = match pkg.as_str() {
        "triton" => "triton-windows",
        "spas_sage_attn" => "spas-sage-attn",
        "huggingface_hub" => "huggingface-hub",
        "opencv" | "opencv-python" => "opencv-python",
        other => other,
    };
    // Live upstream pin from the repo's requirements.txt (e.g. hf_xet>=1.5.2
    // -> ">=1.5.2") so cards can show installed-vs-required. Null when the
    // repo or pin is missing — the card degrades to installed-only.
    let required: serde_json::Value =
        std::fs::read_to_string(get_repo_dir().join("requirements.txt"))
            .ok()
            .and_then(|text| required_spec_in_requirements(&text, dist))
            .map(serde_json::Value::from)
            .unwrap_or(serde_json::Value::Null);
    let py = env_python_bin();
    if let Some(p) = py {
        if p.exists() {
            let code = format!("import importlib.metadata as m; print(m.version({dist:?}))");
            if let Ok(o) = silent_command(&p).args(["-c", &code]).output() {
                if o.status.success() {
                    let v = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if !v.is_empty() {
                        return serde_json::json!({"name": pkg, "installed": true, "version": v, "required": required});
                    }
                }
            }
        }
    }
    serde_json::json!({"name": pkg, "installed": false, "version": null, "required": required})
}
#[tauri::command]
pub fn memory_profile_read() -> serde_json::Value {
    // Read back EXACTLY the keys the panel owns, and only the ones actually
    // on disk. This used to be a hand-written list that predated the eight v17
    // keys: they saved perfectly and then rendered "saved: —" forever, because
    // this function never returned them. Same failure mode as the Apply
    // allowlist — a literal list that nobody extends.
    //
    // It also used to fabricate a default (4 / 0.8 / "int8"…) for any missing
    // key, which painted "saved: 4" on a config that had no such key. Absent
    // now reads as absent; `memProfileLoad` seeds empty dropdowns from what is
    // really there.
    let empty = serde_json::json!({"ok": true, "settings": {}});
    let p = get_repo_dir().join("wgp_config.json");
    let Ok(s) = std::fs::read_to_string(&p) else {
        return empty;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else {
        return empty;
    };
    let mut settings = serde_json::Map::new();
    for key in MEMORY_OVERRIDE_KEYS {
        // int8_kernels replaced the legacy enable_int8_kernels upstream (its
        // v13.13 migration deletes the old key on launch). Prefer the new key;
        // map a lingering legacy value so pre-update configs still read right.
        if *key == "int8_kernels" && v.get("int8_kernels").is_none() {
            let legacy = match v.get("enable_int8_kernels").and_then(|x| x.as_i64()) {
                Some(0) => serde_json::json!("disabled"),
                _ => serde_json::json!("auto"),
            };
            settings.insert((*key).to_string(), legacy);
            continue;
        }
        if let Some(val) = v.get(*key) {
            settings.insert((*key).to_string(), val.clone());
        }
    }
    serde_json::json!({"ok": true, "settings": settings})
}
/// Parse the RAM probe into GB. The probe emits integer BYTES (culture-proof:
/// no decimal separator for locales to mangle); legacy decimal-GB strings
/// ("79,8" pl-PL / "79.8") are also accepted. Returns None on garbage
/// (empty, non-numeric, multi-separator, non-positive, non-finite) so the
/// caller falls back conservatively instead of tiering on a lie.
/// Pure + unit-tested.
pub(crate) fn ram_gb_from_probe(raw: &str) -> Option<f64> {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    // Legacy decimal GB: exactly one dot/comma separator ("79,8" / "79.8").
    if s.contains('.') || s.contains(',') {
        if s.matches(['.', ',']).count() != 1 {
            return None;
        }
        let norm: String = s
            .chars()
            .map(|c| if c == ',' { '.' } else { c })
            .collect();
        let gb: f64 = norm.parse().ok()?;
        if !gb.is_finite() || gb <= 0.0 {
            return None;
        }
        return Some(gb);
    }
    // Integer bytes; sub-KB plain numbers are impossible as byte counts
    // from the real probe, so treat them as corrupt output (None) rather
    // than as GB — misreading "512" as 512GB tiered high-RAM and picked
    // P1, a too-large model. Callers fall back conservatively instead.
    if !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: f64 = s.parse().ok()?;
    if !n.is_finite() || n <= 0.0 {
        return None;
    }
    if n < 1024.0 {
        return None;
    }
    Some(n / GB)
}
#[tauri::command]
pub fn auto_tune_detect() -> serde_json::Value {
    // real hardware detect — mirrors services/auto-tune.js detect() but sync via nvidia-smi
    let gpu = get_gpu_info_sync();
    let vendor = gpu
        .get("vendor")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_uppercase();
    let name = gpu
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let vram_str = gpu.get("vramMB").and_then(|v| v.as_str()).unwrap_or("0");
    let vram_mb: f64 = vram_str
        .split_whitespace()
        .next()
        .unwrap_or("0")
        .parse()
        .unwrap_or(0.0);
    let vram_gb = (vram_mb / 1024.0).floor() as i64;
    let cuda_available = vendor == "NVIDIA" && vram_mb > 0.0 && !name.is_empty();
    // AMD (TheRock/ROCm): no CUDA, but a named Radeon is a usable GPU —
    // surface it instead of "—" so the installer plans the ROCm path.
    // (VRAM comes from the 64-bit registry probe in hw.rs; AdapterRAM cap
    // values read as unknown, never as a fake 4GB figure.)
    let gpu_available = cuda_available || (vendor == "AMD" && !name.is_empty());
    // RAM via powershell (locale-proof): the probe returns integer BYTES —
    // culture-proof by construction — and ram_gb_from_probe() also accepts
    // the legacy decimal-GB strings ("79,8" pl-PL / "79.8"). The old probe
    // (`Round(bytes/1GB,1)`) printed `79,8` under pl-PL, f64::parse rejected
    // the comma, and the silent 32GB fallback mistiered high-RAM machines.
    // Failure now falls back to 16.0 (upstream setup.py's own default),
    // which tiers very_low — fail-closed toward P5, never toward P4.
    let ram_gb = {
        #[cfg(windows)]
        {
            silent_command("powershell").args(["-NoProfile","-Command","(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory"]).output()
                .ok().and_then(|o| ram_gb_from_probe(&String::from_utf8_lossy(&o.stdout))).unwrap_or(16.0)
        }
        #[cfg(not(windows))]
        {
            16.0
        }
    };
    let cpu_count = std::thread::available_parallelism().map_or(8, std::num::NonZero::get) as i64;
    // AMD with known (64-bit registry) VRAM tiers like NVIDIA; unknown stays none.
    let vram_tier = if cuda_available || (vendor == "AMD" && vram_gb > 0) {
        if vram_gb >= 24 {
            "high"
        } else if vram_gb >= 12 {
            "low"
        } else {
            "tight"
        }
    } else {
        "none"
    };
    let ram_tier = if ram_gb >= 63.5 {
        "high"
    } else if ram_gb >= 31.5 {
        "low"
    } else {
        "very_low"
    };
    serde_json::json!({
        "cuda_available": cuda_available,
        "gpu_available": gpu_available,
        "gpu_name": if gpu_available { name.clone() } else { "—".into() },
        "gpu_vram_gb": vram_gb,
        "ram_gb": ram_gb,
        "cpu_count": cpu_count,
        "vram_tier": vram_tier,
        "ram_tier": ram_tier,
        "vendor": vendor,
        "driver": gpu.get("driverVersion").cloned().unwrap_or(serde_json::Value::Null)
    })
}
#[tauri::command]
pub fn auto_tune_recommend(
    hw: Option<serde_json::Value>,
    opts: Option<serde_json::Value>,
) -> serde_json::Value {
    let hw = hw.unwrap_or(serde_json::json!({"vram_tier":"low","ram_tier":"low","gpu_vram_gb":10}));
    let vram_tier = hw
        .get("vram_tier")
        .and_then(|v| v.as_str())
        .unwrap_or("low");
    let ram_tier = hw.get("ram_tier").and_then(|v| v.as_str()).unwrap_or("low");
    let vram_gb = hw
        .get("gpu_vram_gb")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(10.0);
    let failsafe = opts
        .as_ref()
        .and_then(|o| o.get("failsafe"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    // No usable GPU at all → explicit conservative fallback, clearly
    // labeled (never silent P5). AMD cards report cuda_available=false but
    // gpu_available=true with real VRAM (ROCm/TheRock) — tiering them as
    // "unavailable" forced every healthy Radeon (incl. 9060 XT) to P4.5.
    let cuda_off = hw.get("cuda_available").and_then(|v| v.as_bool()) == Some(false);
    let vendor_is_amd = hw
        .get("vendor")
        .and_then(|v| v.as_str())
        .map(|v| v.to_uppercase() == "AMD")
        .unwrap_or(false);
    let amd_usable = hw
        .get("gpu_available")
        .and_then(|v| v.as_bool())
        .unwrap_or(vendor_is_amd)
        && vram_gb > 0.0;
    if !failsafe && cuda_off && !amd_usable {
        return serde_json::json!({
            "video_profile": 4.5, "image_profile": 4.5, "audio_profile": 4.5,
            "vram_safety_coefficient": 0.70, "vae_config": 0, "transformer_quantization": "int8", "int8_kernels": "auto", "kernel_precision": "fast",
            "_recommendation_label": "Auto-tune unavailable on this hardware",
            "_recommendation_reason": "No CUDA-capable GPU detected. Conservative profile applied — generation may be limited.",
            "packages": ["torch","triton","sageattention"],
            "kernels": ["nunchaku","gguf"]
        });
    }
    // 7-profile matrix incl. fractional P3.5/P4.5 (mirrors auto-tune.js PROFILE_MATRIX).
    let (profile, coeff): (f64, f64) = if failsafe {
        (5.0, 0.6)
    } else {
        let p = match (vram_tier, ram_tier) {
            ("high", "high") => 1.0,
            ("high", "low") => 3.0,
            ("high", "very_low") => 3.5,
            ("low", "high") => 2.0,
            ("low", "low") => 4.0,
            ("low", "very_low") => 5.0,
            ("tight", "high") => 4.0,
            // v17 + MMGP v4: Profile 4 is up to 50% lower peak VRAM and
            // 11 GB now does 1080p H3, so the tight tier no longer has to
            // fall back to P4+/P5 once the v17 levers below are on.
            ("tight", "low") => 4.0,
            _ => 4.0,
        };
        let c = if vram_tier == "tight" || vram_tier == "none" {
            0.7
        } else {
            0.8
        };
        (p, c)
    };
    // Fast-LM-decoder rule: ≥12GB VRAM needs int-profile 1/3 for audio, else inherit.
    // Upstream v17: "Profile 3+ is recommended for audio models (their default)" —
// audio models fit whole in VRAM, where the language model many of them
// include runs much faster and can use the CUDA Graph or vLLM engines.
//
// Do NOT gate this on VRAM the way the video/image tiers are gated: those are
// calibrated against 14B video models, and audio models are an order of
// magnitude smaller, so a card that earns P4 for video comfortably holds one
// whole. Gating on vram_gb (>= 12) therefore sent a 10 GB RTX 3080 — where
// upstream's own default applies — to P4 for audio and cost it the fast path.
// The real safety valve is the failsafe switch and a genuinely tiny card: when
// video itself is already at P5 there is nothing left to spare, so audio
// follows it down. P1 and P3 are left alone — they already load whole and
// keep Reserved RAM. (Was 3.0, gated on vram_gb >= 12.)
let audio = if [2.0, 4.0, 4.5].contains(&profile) {
    3.5
} else {
    profile
};
    let label = if failsafe {
        "Failsafe · P5 (maximum compatibility)".to_string()
    } else {
        match profile.to_string().as_str() {
            "1" => "HighRAM · HighVRAM",
            "2" => "HighRAM · LowVRAM",
            "3" => "LowRAM · HighVRAM",
            "3.5" => "VeryLowRAM · HighVRAM (upstream: recommended for audio)",
            "4" => "LowRAM · LowVRAM (upstream: recommended)",
            "4.5" => "LowRAM · LowVRAM+",
            _ => "VerylowRAM · LowVRAM",
        }
        .to_string()
    };
    // v17 (MMGP v4) levers. All of them are CUDA-only in practice:
    // shared/cuda_memory.apply_startup_settings returns early when
    // torch.version.hip is set or CUDA is unavailable, so writing them on
    // AMD/Intel/CPU would persist a setting that silently does nothing.
    let mut extra = serde_json::Map::new();
    if hw.get("cuda_available").and_then(|v| v.as_bool()) == Some(true) {
        extra.insert("vram_allocator".into(), serde_json::json!("vmm_spill"));
        // Head Split Medium (2) buys ~20% VRAM for <=10% slower steps
        // (docs/CLI.md, README v17) — the right trade exactly where the
        // tight tier used to need a lower profile instead. Only engages at
        // >= 8192 tokens upstream, so it costs nothing on short clips.
        extra.insert(
            "attention_head_split".into(),
            serde_json::json!(if vram_tier == "tight" { 2 } else { 0 }),
        );
        // Upstream v17.01: "first make sure you use Sage2/2+ Attention as quite a
    // few optimizations depends on it" — so the attention mode is part of the
    // recommendation, not just a troubleshooting dropdown. NVIDIA only; the
    // AMD/ROCm path has no Sage build and defaults to sdpa upstream.
    if hw.get("cuda_available").and_then(|v| v.as_bool()) == Some(true) {
        let key = crate::hw::kernel_profile_key(
            hw.get("vendor").and_then(|v| v.as_str()).unwrap_or(""),
            hw.get("gpu_name").and_then(|v| v.as_str()).unwrap_or(""),
        );
        if let Some(a) = crate::hw::attention_for_profile(&key) {
            extra.insert("attention_mode".into(), serde_json::json!(a));
        }
    }
    extra.insert("read_ahead".into(), serde_json::json!(cfg!(windows)));
        extra.insert("smart_memory_pinning".into(), serde_json::json!(true));
        // WanGP's own RAM/VRAM advice: "For image models, use Profile 4 or 5
        // with a Dynamic or Manual VRAM Preload ... Video steps are usually
        // long enough to hide the transfers." Audio rides P3+, which loads
        // each model whole and hides the control entirely.
        extra.insert("video_preload_mode".into(), serde_json::json!("default"));
        extra.insert("image_preload_mode".into(), serde_json::json!("dynamic"));
        extra.insert("audio_preload_mode".into(), serde_json::json!("default"));
        // 0 = Auto (upstream: 40% on Windows, 60% on Linux — v17.10 / ec9566a
                  // lowered Linux from 80%, so a Linux Auto now pins less than v17.00 did).
        extra.insert("perc_reserved_mem_max".into(), serde_json::json!(0));
    }
    let mut out = serde_json::json!({
        "video_profile": profile, "image_profile": profile, "audio_profile": audio,
        "vram_safety_coefficient": coeff, "vae_config": 0, "transformer_quantization": "int8", "int8_kernels": "auto", "kernel_precision": "fast",
        "_recommendation_label": label,
        "_recommendation_reason": "Auto-tuned for your hardware",
        "packages": ["torch","triton","sageattention"],
        "kernels": ["nunchaku","gguf"]
    });
    if let Some(obj) = out.as_object_mut() {
        for (k, v) in extra {
            obj.insert(k, v);
        }
    }
    out
}

// ── Phase 2-5: remaining 65 handlers as thin stubs (real logic behind shell/fs plugins) ──
/// AMD package gate for the install path: CUDA / bitsandbytes /
/// CUDA-built SageAttention 2 / vanilla spas_sage_attn / PyPI sdist
/// flash-attn break the TheRock env, so they are refused on AMD profiles
/// with a pointer to docs/AMD-INSTALLATION.md. Vanilla `triton` maps to
/// `triton-windows`. PyPI `sageattention` 1.x is ALLOWED: it is upstream's
/// AMD SageAttention 1 stack (`sage: v1`, pure PyPI, needs triton-windows).
/// Non-AMD profiles pass through untouched (pure + unit-testable core).
pub(crate) fn amd_package_gate_result(profile: &str, pkg: &str) -> Result<String, String> {
    const GUIDE: &str = "docs/AMD-INSTALLATION.md";
    if !profile.starts_with("AMD") {
        return Ok(pkg.to_string());
    }
    let low = pkg.to_lowercase();
    // Dist name without any version pin.
    let cut = low.find(|c| "<>=!~; [".contains(c));
    let name = match cut {
        Some(i) => low[..i].trim(),
        None => low.trim(),
    };
    // Vanilla PyPI triton → triton-windows (keep any version pin).
    if name == "triton" {
        let pin = pkg[name.len()..].to_string();
        return Ok(format!("triton-windows{pin}"));
    }
    // Upstream AMD SageAttention 1 stack: pure-PyPI sageattention 1.x
    // (e.g. `sageattention==1.0.6`). CUDA-built wheels (2.x, `+cu…`
    // build tags, woct0rdho URLs) stay refused below.
    if name == "sageattention" {
        let spec = low.clone();
        let is_cuda_build = spec.contains("+cu")
            || spec.contains("cu12")
            || spec.contains("cu13")
            || spec.contains("woct0rdho");
        let is_v2 = spec.contains("==2.") || spec.contains(">=2.") || spec.contains(">2.");
        if !is_cuda_build && !is_v2 {
            return Ok(pkg.to_string());
        }
    }
    let blocked = name == "bitsandbytes"
        || name == "spas-sage-attn"
        || name == "spas_sage_attn"
        || name == "sageattention"
        // Wheel-filename form (`sageattention-<ver>+cu…-…whl`, e.g. pasted
        // from the install guide): CUDA builds only — no ROCm sage wheel
        // ships under this name.
        || (name.starts_with("sageattention-")
            && (name.contains("+cu")
                || name.contains("cu12")
                || name.contains("cu13")
                || name.contains("woct0rdho")))
        || name == "flash-attn"
        || name == "flash_attn"
        || name.contains("cuda")
        || name.contains("nvidia");
    if blocked {
        return Err(format!(
"refused on AMD (ROCm/TheRock env): '{pkg}' would break torch — use the {GUIDE} guide recipe instead"
));
    }
    Ok(pkg.to_string())
}
fn amd_package_gate(pkg: &str) -> Result<String, String> {
    let gpu = get_gpu_info_sync();
    let profile = kernel_profile_key(
        gpu.get("vendor").and_then(|v| v.as_str()).unwrap_or(""),
        gpu.get("name").and_then(|v| v.as_str()).unwrap_or(""),
    );
    amd_package_gate_result(&profile, pkg)
}
#[tauri::command]
pub async fn upgrade_package(
    app: tauri::AppHandle,
    pkg: String,
    force: Option<bool>,
) -> Result<serde_json::Value, String> {
    pip_spec_ok(&pkg).map_err(|e| format!("blocked: {e}"))?;
    // Pin guards (issue #54): managed dists and requirements-capped dists
    // refuse a bare `--upgrade`. `force` (explicit pinned-chip override in
    // the UI, after a confirm naming the recovery path) skips both guards;
    // the spec + AMD guards below always apply, even forced.
    if !force.unwrap_or(false) {
        let req_text =
            std::fs::read_to_string(get_repo_dir().join("requirements.txt")).ok();
        if let Some(reason) = refuse_upgrade_reason(&pkg, req_text.as_deref()) {
            return Err(reason);
        }
    }
    // AMD guard (same as install_package): refuse ROCm-breaking dists on
    // AMD profiles; vanilla `triton` maps to `triton-windows`.
    let pkg = amd_package_gate(&pkg)?;
    let Some(py) = env_python_bin() else {
        return Err("python not found".into());
    };
    let py_s = py.to_string_lossy().to_string();
    let emit = |m: &str| {
        let _ = app.emit("launch-log", m.to_string());
    };
    if !run_logged(
        &app,
        &py_s,
        &["-m", "pip", "install", "--upgrade", &pkg],
        None,
        emit,
    )
    .await
    {
        return Err(format!("pip upgrade {pkg} failed — see console output"));
    }
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command]
pub async fn install_package(
    app: tauri::AppHandle,
    pkg: String,
) -> Result<serde_json::Value, String> {
    pip_spec_ok(&pkg).map_err(|e| format!("blocked: {e}"))?;
    // AMD guard: CUDA / bitsandbytes / vanilla PyPI triton / vanilla
    // spas_sage_attn / PyPI sdist flash-attn break the TheRock env — refuse
    // with a pointer to the guide recipe. Vanilla `triton` maps to
    // `triton-windows`. NVIDIA/Intel/CPU paths are identical to before.
    let pkg = amd_package_gate(&pkg)?;
    let Some(py) = env_python_bin() else {
        return Err("python not found".into());
    };
    let py_s = py.to_string_lossy().to_string();
    let emit = |m: &str| {
        let _ = app.emit("launch-log", m.to_string());
    };
    if !run_logged(&app, &py_s, &["-m", "pip", "install", &pkg], None, emit).await {
        return Err(format!("pip install {pkg} failed — see console output"));
    }
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[cfg(test)]
mod required_spec_tests {
    use super::required_spec_in_requirements;
    #[test]
    fn finds_ge_floor_and_bare_and_exact() {
        let text = "# comment\nhf_xet>=1.5.2\ntqdm\nmmgp==3.8.0\n";
        assert_eq!(
            required_spec_in_requirements(text, "hf_xet").as_deref(),
            Some(">=1.5.2")
        );
        assert_eq!(
            required_spec_in_requirements(text, "tqdm").as_deref(),
            Some("")
        );
        assert_eq!(
            required_spec_in_requirements(text, "mmgp").as_deref(),
            Some("==3.8.0")
        );
    }
    #[test]
    fn name_match_ignores_case_underscore_and_extras() {
        let text = "HuggingFace_Hub[hf_xet]>=0.36.2\n";
        assert_eq!(
            required_spec_in_requirements(text, "huggingface-hub").as_deref(),
            Some(">=0.36.2")
        );
    }
    #[test]
    fn skips_urls_options_markers_and_missing() {
        let text = "-r other.txt\ninsightface @ https://example.com/x.whl\nrembg==2.0.65; python_version < \"3.11\"\n";
        assert_eq!(
            required_spec_in_requirements(text, "rembg").as_deref(),
            Some("==2.0.65")
        );
        assert_eq!(required_spec_in_requirements(text, "insightface"), None);
        assert_eq!(required_spec_in_requirements(text, "nope"), None);
    }
}
#[cfg(test)]
mod amd_package_gate_tests {
    use super::amd_package_gate_result;
    #[test]
    fn amd_refuses_cuda_bitsandbytes_vanilla_sage_flash() {
        // Vanilla `triton` maps to triton-windows instead of refusing.
        assert_eq!(
            amd_package_gate_result("AMD_GFX1201", "triton").unwrap(),
            "triton-windows"
        );
        assert_eq!(
            amd_package_gate_result("AMD_GFX1201", "triton==3.4.0").unwrap(),
            "triton-windows==3.4.0"
        );
        // Upstream AMD SageAttention 1 stack: pure-PyPI 1.x passes through.
        assert_eq!(
            amd_package_gate_result("AMD_GFX1201", "sageattention==1.0.6").unwrap(),
            "sageattention==1.0.6"
        );
        assert_eq!(
            amd_package_gate_result("AMD", "sageattention").unwrap(),
            "sageattention"
        );
        for bad in [
            "bitsandbytes",
            "spas_sage_attn",
            "spas-sage-attn",
            // CUDA-built SageAttention 2 stays refused (2.x / +cu tags).
            "sageattention==2.2.0",
            "sageattention-2.2.0+cu130torch2.9.0andhigher.post4",
            "flash-attn",
            "flash_attn",
            "nvidia-cuda-runtime-cu12",
        ] {
            let err = amd_package_gate_result("AMD_GFX1201", bad).unwrap_err();
            assert!(
                err.contains("docs/AMD-INSTALLATION.md"),
                "missing guide pointer: {err}"
            );
        }
        // NVIDIA behavior identical: everything passes through.
        assert_eq!(
            amd_package_gate_result("RTX_40", "triton").unwrap(),
            "triton"
        );
        assert_eq!(
            amd_package_gate_result("RTX_40", "bitsandbytes").unwrap(),
            "bitsandbytes"
        );
        assert_eq!(
            amd_package_gate_result("INTEL_XPU", "flash-attn").unwrap(),
            "flash-attn"
        );
    }
}
#[cfg(test)]
mod managed_pin_source_tests {
    use super::{dist_name_only, managed_pin_source};
    #[test]
    fn issue54_torch_flash_are_managed() {
        // The exact #54 report: torch 2.10.0+cu130 and flash_attn 2.8.3.
        assert_eq!(
            managed_pin_source("torch", "2.10.0+cu130"),
            Some("gpu-profile")
        );
        assert_eq!(managed_pin_source("flash_attn", "2.8.3"), Some("kernel-wheel"));
        assert_eq!(managed_pin_source("flash-attn", "2.8.3"), Some("kernel-wheel"));
    }
    #[test]
    fn covers_profile_and_kernel_families() {
        for (dist, src) in [
            ("torchaudio", "gpu-profile"),
            ("torchvision", "gpu-profile"),
            ("triton", "gpu-profile"),
            ("triton-windows", "gpu-profile"),
            ("sageattention", "kernel-wheel"),
            ("spas-sage-attn", "kernel-wheel"),
            ("spas_sage_attn", "kernel-wheel"),
            ("nunchaku", "kernel-wheel"),
            ("llamacpp_gguf_cuda", "kernel-wheel"),
            ("lightx2v_kernel", "kernel-wheel"),
        ] {
            assert_eq!(managed_pin_source(dist, "1.0"), Some(src), "{dist}");
        }
        // Case / underscore-insensitive; version pins strip to the dist name.
        assert_eq!(managed_pin_source("Torch", "2.10.0"), Some("gpu-profile"));
        assert_eq!(managed_pin_source("FLASH_ATTN", "2.8.3"), Some("kernel-wheel"));
        assert_eq!(dist_name_only("flash-attn==2.8.3"), "flash-attn");
        assert_eq!(dist_name_only("torch>=2.10"), "torch");
        // Local CUDA/ROCm build tags pin even unknown names.
        assert_eq!(
            managed_pin_source("something", "1.0+cu130"),
            Some("gpu-profile")
        );
        assert_eq!(
            managed_pin_source("something", "1.0+rocm7.14"),
            Some("gpu-profile")
        );
        // Plain PyPI packages stay upgradable.
        assert_eq!(managed_pin_source("diffusers", "0.35.0"), None);
        assert_eq!(managed_pin_source("transformers", "4.50.0"), None);
        assert_eq!(managed_pin_source("huggingface-hub", "0.30.0"), None);
    }
}
#[cfg(test)]
mod requirements_pin_tests {
    use super::{refuse_upgrade_reason, requirements_caps_dist, requirements_pin_for};
    const REQS: &str = "# Core AI stack\ndiffusers==0.36.0\ntransformers==4.54.0 #4.53.1\ntokenizers>=0.20.3\nnumpy==2.1.2\nhuggingface_hub[hf_xet]\nrembg[gpu]==2.0.65; platform_system != \"Darwin\"\n";
    #[test]
    fn exact_pins_block_newer_latest() {
        // The live #54 follow-up rows: tested == pins vs pip latest.
        assert_eq!(
            requirements_pin_for(REQS, "diffusers", "0.40.0", None),
            Some("==0.36.0".into())
        );
        assert_eq!(
            requirements_pin_for(REQS, "transformers", "5.17.0", None),
            Some("==4.54.0".into())
        );
        assert_eq!(
            requirements_pin_for(REQS, "numpy", "2.4.6", None),
            Some("==2.1.2".into())
        );
        // Already at the pin: no update offered, nothing to block.
        assert_eq!(requirements_pin_for(REQS, "diffusers", "0.36.0", None), None);
    }
    #[test]
    fn floors_bare_and_missing_stay_upgradable() {
        assert_eq!(requirements_pin_for(REQS, "tokenizers", "0.23.2", None), None);
        // Upstream leaves huggingface_hub uncapped: 2.0 stays offered.
        assert_eq!(
            requirements_pin_for(REQS, "huggingface-hub", "2.0.0", None),
            None
        );
        assert_eq!(requirements_pin_for(REQS, "not-listed", "9.9.9", None), None);
        assert_eq!(requirements_pin_for(REQS, "diffusers", "", None), None);
    }
    #[test]
    fn markers_evaluated_when_interpreter_known() {
        // Unknown interpreter: unjudgeable marker line fails open.
        assert_eq!(requirements_pin_for(REQS, "rembg", "2.0.66", None), None);
        // Windows + py3.11: `platform_system != "Darwin"` holds → capped.
        assert_eq!(
            requirements_pin_for(REQS, "rembg", "2.0.66", Some((3, 11))),
            Some("==2.0.65".into())
        );
        // A python_version-gated line applies per interpreter.
        let gated = "pkg==1.0; python_version >= \"3.10\"\n";
        assert_eq!(
            requirements_pin_for(gated, "pkg", "1.1", Some((3, 11))),
            Some("==1.0".into())
        );
        assert_eq!(requirements_pin_for(gated, "pkg", "1.1", Some((3, 9))), None);
    }
    #[test]
    fn clause_semantics_cover_ranges_and_compat() {
        let ceil = "pkg<2\n";
        assert_eq!(
            requirements_pin_for(ceil, "pkg", "2.0.0", None),
            Some("<2".into())
        );
        assert_eq!(requirements_pin_for(ceil, "pkg", "1.9", None), None);
        let compat = "pkg~=1.4.2\n";
        assert_eq!(requirements_pin_for(compat, "pkg", "1.4.7", None), None);
        assert_eq!(
            requirements_pin_for(compat, "pkg", "1.5.0", None),
            Some("~=1.4.2".into())
        );
        let wild = "pkg==1.4.*\n";
        assert_eq!(requirements_pin_for(wild, "pkg", "1.4.7", None), None);
        assert_eq!(
            requirements_pin_for(wild, "pkg", "1.5.0", None),
            Some("==1.4.*".into())
        );
    }
    #[test]
    fn caps_dist_drives_upgrade_refusal() {
        assert!(requirements_caps_dist(REQS, "diffusers", None));
        assert!(requirements_caps_dist(REQS, "numpy", None));
        assert!(requirements_caps_dist("pkg<2\n", "pkg", None));
        assert!(requirements_caps_dist("pkg~=1.4.2\n", "pkg", None));
        // Floors, bare names, and missing entries never refuse.
        assert!(!requirements_caps_dist(REQS, "tokenizers", None));
        assert!(!requirements_caps_dist(REQS, "huggingface-hub", None));
        assert!(!requirements_caps_dist(REQS, "not-listed", None));
        assert!(!requirements_caps_dist("pkg>1\n", "pkg", None));
    }
    #[test]
    fn refuse_reason_points_at_recovery_path() {
        // Managed dists refuse even without a requirements file; torch
        // recovery is reinstall (restore can't fix it).
        let r = refuse_upgrade_reason("torch", None).unwrap();
        assert!(r.contains("gpu-profile") && r.contains("reinstall"), "{r}");
        let r = refuse_upgrade_reason("flash_attn==2.8.3", None).unwrap();
        assert!(r.contains("kernel-wheel"), "{r}");
        // Capped dists refuse with a restore pointer; floors pass.
        let r = refuse_upgrade_reason("diffusers", Some(REQS)).unwrap();
        assert!(r.contains("requirements.txt") && r.contains("restore"), "{r}");
        assert_eq!(refuse_upgrade_reason("tokenizers", Some(REQS)), None);
        assert_eq!(refuse_upgrade_reason("not-listed", Some(REQS)), None);
        // No file degrades to the managed guard only.
        assert_eq!(refuse_upgrade_reason("diffusers", None), None);
    }
}
#[cfg(test)]
mod apprise_argv_tests {
    use super::{apprise_argv, notifier_normalize};
    #[test]
    fn prefers_console_script_falls_back_to_module() {
        // Binary beside the interpreter wins (the #35 fix: `-m` may lack __main__).
        let dir = std::env::temp_dir().join(format!(
            "wgp-apprise-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        #[cfg(windows)]
        let bin_name = "apprise.exe";
        #[cfg(not(windows))]
        let bin_name = "apprise";
        let py = dir.join("python.exe");
        std::fs::write(&py, b"").unwrap();
        // No binary yet → module fallback.
        let (cmd, args) = apprise_argv(&py, "T", "B", "discord://x");
        assert_eq!(cmd, py);
        assert_eq!(args[..2], vec!["-m".to_string(), "apprise".to_string()]);
        assert_eq!(args.last().unwrap(), "discord://x");
        // Binary appears → direct invocation, no `-m`.
        std::fs::write(dir.join(bin_name), b"").unwrap();
        let (cmd, args) = apprise_argv(&py, "T", "B", "discord://x");
        assert_eq!(cmd, dir.join(bin_name));
        assert_eq!(
            args,
            vec!["-t", "T", "-b", "B", "discord://x"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn native_managed_defaults_off_and_survives_normalize() {
        // Absent marker reads as legacy mode (old configs keep working).
        let off = notifier_normalize(&serde_json::json!({"enabled": true}));
        assert_eq!(off.get("nativeManaged").and_then(|v| v.as_bool()), Some(false));
        // Present marker survives a round-trip (native save owns it).
        let on = notifier_normalize(&serde_json::json!({"nativeManaged": true}));
        assert_eq!(on.get("nativeManaged").and_then(|v| v.as_bool()), Some(true));
    }
}
#[tauri::command]
pub async fn uninstall_package(
    app: tauri::AppHandle,
    pkg: String,
) -> Result<serde_json::Value, String> {
    pip_spec_ok(&pkg).map_err(|e| format!("blocked: {e}"))?;
    let Some(py) = env_python_bin() else {
        return Err("python not found".into());
    };
    let py_s = py.to_string_lossy().to_string();
    let emit = |m: &str| {
        let _ = app.emit("launch-log", m.to_string());
    };
    if !run_logged(
        &app,
        &py_s,
        &["-m", "pip", "uninstall", "-y", &pkg],
        None,
        emit,
    )
    .await
    {
        return Err(format!("pip uninstall {pkg} failed — see console output"));
    }
    Ok(serde_json::json!({"ok": true, "success": true}))
}
#[tauri::command]
pub async fn restore_requirements(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let repo = get_repo_dir();
    let Some(py) = env_python_bin() else {
        return Err("python not found".into());
    };
    let py_s = py.to_string_lossy().to_string();
    let emit = |m: &str| {
        let _ = app.emit("launch-log", m.to_string());
    };
    let (ok, out) = run_logged_capture(
        &app,
        &py_s,
        &["-m", "pip", "install", "-r", "requirements.txt"],
        Some(&repo),
        emit,
    )
    .await;
    if !ok {
        return Err("requirements restore failed — see console output".into());
    }
    // Say what landed. pip states it only in its own `Successfully installed`
    // line, which scrolls past the console viewer's tail on a full
    // requirements run — so a 1 MB download and a no-op used to read alike.
    let installed: Vec<String> = crate::base::pip_installed_from_output(&out);
    Ok(serde_json::json!({"ok": true, "success": true, "installed": installed}))
}
#[tauri::command]
pub fn llm_engines_list() -> serde_json::Value {
    // ponytail: probe cliOnPath + pipInstalled like Electron services/llm-engines.js
    let env = get_active_env();
    let py = env_python_bin();
    let check_cli = |cli: &str| -> bool {
        #[cfg(windows)]
        {
            silent_command("where")
                .arg(cli)
                .output()
                .is_ok_and(|o| o.status.success())
        }
        #[cfg(not(windows))]
        {
            silent_command("which")
                .arg(cli)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }
    };
    // Reuse the get_status version scan (same refresh already ran it — cache is warm)
    // instead of spawning a second cold venv python just for pip show.
    let cached_sdk = crate::base::LAST_STATUS
        .get()
        .and_then(|m| m.lock().ok())
        .and_then(|g| {
            g.as_ref()
                .and_then(|(_, _, v)| v.get("versions"))
                .and_then(|vs| vs.get("claude-agent-sdk"))
                .and_then(|x| x.as_str())
                .map(|s| !s.is_empty())
        })
        .unwrap_or(false);
    let check_pip = |pkg: &str| -> bool {
        if pkg == "claude-agent-sdk" && cached_sdk {
            return true;
        }
        if let Some(p) = &py {
            if p.exists() {
                return silent_command(p)
                    .args(["-m", "pip", "show", pkg])
                    .output()
                    .is_ok_and(|o| o.status.success());
            }
        }
        false
    };
    let engines = vec![
        serde_json::json!({"id":"claude-code","label":"Claude Code","desc":"Anthropic Claude Code CLI + Python bridge","cli":"claude","cliOnPath": check_cli("claude"), "pipPackage":"claude_agent_sdk","pipInstalled": check_pip("claude-agent-sdk"), "install":{"mode":"pip","spec":"claude-agent-sdk==0.1.66"}, "external": false, "auth":{"docsUrl":"https://code.claude.com/docs/en/authentication"}, "notes":"Pinned to 0.1.66 - upstream's exact bridge (summarized thinking); a newer SDK can clobber Wan2GP MCP deps."}),
        serde_json::json!({"id":"codex","label":"OpenAI Codex","desc":"OpenAI Codex CLI (npm)","cli":"codex","cliOnPath": check_cli("codex"), "pipPackage":null,"pipInstalled":null,"install":{"mode":"npm","spec":"@openai/codex","global":true}, "external": true, "authHint":"Sign in via a Deepy request in Wan2GP (secure link shown in chat)."}),
        serde_json::json!({"id":"opencode","label":"OpenCode","desc":"Universal-provider agent","cli":"opencode","cliOnPath": check_cli("opencode"), "pipPackage":null,"pipInstalled":null,"install":{"mode":"npm","spec":"opencode-ai","global":true}, "external": true, "serve":{"cmd":"opencode","args":["serve","--hostname","127.0.0.1","--port","4096"]}, "serverUrl":"http://127.0.0.1:4096", "serverRunning": std::net::TcpStream::connect("127.0.0.1:4096").is_ok(), "authHint":"In the OpenCode UI run /connect, pick a provider, follow auth. Wan2GP auto-starts opencode serve."}),
    ];
    serde_json::json!({"ok": true, "engines": engines, "hasActiveEnv": !env.is_null()})
}
// One-click engine installer: claude-code via env pip (pinned), codex/opencode via npm -g.
#[tauri::command]
pub fn llm_engine_install(engine: String) -> serde_json::Value {
    let spec = match engine.as_str() {
        "claude-code" => "claude-agent-sdk==0.1.66",
        "codex" => "@openai/codex",
        "opencode" => "opencode-ai",
        _ => return serde_json::json!({"ok": false, "success": false, "error": "Unknown engine"}),
    };
    let res = match engine.as_str() {
        "claude-code" => match env_python_bin() {
            None => {
                return serde_json::json!({"ok": false, "success": false, "error": "No active Python environment"})
            }
            Some(py) => silent_command(&py)
                .args(["-m", "pip", "install", spec])
                .output(),
        },
        _ => term_tool("npm").args(["install", "-g", spec]).output(),
    };
    match res {
        Ok(o) if o.status.success() => {
            serde_json::json!({"ok": true, "success": true, "spec": spec})
        }
        Ok(o) => {
            serde_json::json!({"ok": false, "success": false, "error": format!("install failed: {}", String::from_utf8_lossy(&o.stderr).trim())})
        }
        Err(e) => serde_json::json!({"ok": false, "success": false, "error": e.to_string()}),
    }
}
#[tauri::command]
pub fn llm_engine_uninstall(engine: String) -> serde_json::Value {
    let res = match engine.as_str() {
        "claude-code" => match env_python_bin() {
            None => {
                return serde_json::json!({"ok": false, "success": false, "error": "No active Python environment"})
            }
            Some(py) => silent_command(&py)
                .args(["-m", "pip", "uninstall", "-y", "claude-agent-sdk"])
                .output(),
        },
        "codex" => term_tool("npm")
            .args(["uninstall", "-g", "@openai/codex"])
            .output(),
        "opencode" => term_tool("npm")
            .args(["uninstall", "-g", "opencode-ai"])
            .output(),
        _ => return serde_json::json!({"ok": false, "success": false, "error": "Unknown engine"}),
    };
    match res {
        Ok(o) if o.status.success() => serde_json::json!({"ok": true, "success": true}),
        Ok(o) => {
            serde_json::json!({"ok": false, "success": false, "error": format!("remove failed: {}", String::from_utf8_lossy(&o.stderr).trim())})
        }
        Err(e) => serde_json::json!({"ok": false, "success": false, "error": e.to_string()}),
    }
}
// OpenCode server lifecycle (Wan2GP auto-starts it too; this is the manual toggle).
// PID-tracked so Start/Stop is honest; other engines have no servable process.
static OPENCODE_PID: std::sync::OnceLock<std::sync::Mutex<Option<u32>>> =
    std::sync::OnceLock::new();
// Kill the OpenCode server this launcher spawned (if any). Shared by the
// serve toggle and the app-close cleanup.
// Orphan fallback: OPENCODE_PID is memory-only and the child is
// mem::forget detached, so a restart loses the PID while the server keeps
// listening on :4096. After the PID kill (or when no PID was stored),
// sweep local port 4096 and kill ONLY node/opencode owners — never
// foreign processes. Stays SYNC; returns true if anything was killed.
/// Fast synchronous kill for app-close (see shutdown_cleanup): tracked PID
/// only, no port-scan PowerShell. Milliseconds; the :4096 orphan sweep
/// stays on the toggle/Stop path.
pub(crate) fn stop_opencode_fast() -> bool {
    let pid = OPENCODE_PID
        .get()
        .and_then(|m| m.lock().ok())
        .and_then(|mut g| g.take());
    if let Some(pid) = pid {
        #[cfg(windows)]
        {
            let _ = silent_command("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .output();
        }
        #[cfg(not(windows))]
        {
            let _ = silent_command("kill").arg(pid.to_string()).output();
        }
        return true;
    }
    false
}
pub(crate) fn stop_opencode_server() -> bool {
    let mut killed_any = false;
    let pid = OPENCODE_PID
        .get()
        .and_then(|m| m.lock().ok())
        .and_then(|mut g| g.take());
    if let Some(pid) = pid {
        // ponytail: /T kills the tree — the tracked PID is now the cmd /C wrapper, not the server itself
        #[cfg(windows)]
        {
            let _ = silent_command("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .output();
        }
        #[cfg(not(windows))]
        {
            let _ = silent_command("kill").arg(pid.to_string()).output();
        }
        killed_any = true;
    }
    // Port-sweep fallback for the :4096 orphan (restart lost the PID).
    // Windows: ONE Get-NetTCPConnection call, owner resolved in-PS.
    // Non-Windows: lsof -ti tcp:4096. Owner gate (node/opencode,
    // case-insensitive) is enforced in Rust before any kill.
    #[cfg(windows)]
    {
        let ps = "Get-NetTCPConnection -LocalPort 4096 -State Listen | ForEach-Object { $p = Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue; if ($p) { $p.Id.ToString() + '|' + $p.ProcessName } }";
        match silent_command("powershell")
            .args(["-NoProfile", "-Command", ps])
            .output()
        {
            Ok(o) if o.status.success() => {
                for line in String::from_utf8_lossy(&o.stdout).lines() {
                    let mut parts = line.splitn(2, '|');
                    if let (Some(pid_s), Some(name)) = (parts.next(), parts.next()) {
                        let owner = name.trim().to_lowercase();
                        if !(owner.contains("node") || owner.contains("opencode")) {
                            continue; // never kill foreign processes
                        }
                        if let Ok(pid) = pid_s.trim().parse::<u32>() {
                            if pid == 0 || pid == std::process::id() {
                                continue;
                            }
                            crate::base::push_log(
                                &format!(
                                    "[stop] opencode :4096: killing {name} PID {pid}\n",
                                    name = name.trim(),
                                ),
                                "launch",
                            );
                            let _ = silent_command("taskkill")
                                .args(["/F", "/T", "/PID", &pid.to_string()])
                                .output();
                            killed_any = true;
                        }
                    }
                }
            }
            // Exit 1 + "No matching" = nothing listening (the CLEAN case).
            Ok(o) => {
                let code = o.status.code().unwrap_or(-1);
                let err: String = String::from_utf8_lossy(&o.stderr)
                    .chars()
                    .take(200)
                    .collect();
                if !(code == 1 && err.contains("No matching")) {
                    crate::base::push_log(
                        &format!("[stop] opencode port scan failed (exit {code}): {err}\n"),
                        "launch",
                    );
                }
            }
            Err(e) => crate::base::push_log(
                &format!("[stop] opencode port scan spawn failed: {e}\n"),
                "launch",
            ),
        }
    }
    #[cfg(not(windows))]
    {
        // lsof lists PIDs; owner gate via ps so foreign listeners survive.
        let pids: Vec<u32> = silent_command("lsof")
            .args(["-ti", "tcp:4096"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .filter_map(|l| l.trim().parse::<u32>().ok())
                    .collect()
            })
            .unwrap_or_default();
        for pid in pids {
            if pid == 0 || pid == std::process::id() {
                continue;
            }
            let owner = silent_command("ps")
                .args(["-p", &pid.to_string(), "-o", "comm="])
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_lowercase())
                .unwrap_or_default();
            if !(owner.contains("node") || owner.contains("opencode")) {
                continue; // never kill foreign processes
            }
            crate::base::push_log(
                &format!("[stop] opencode :4096: killing {owner} PID {pid}\n"),
                "launch",
            );
            let _ = silent_command("kill").arg(pid.to_string()).output();
            killed_any = true;
        }
    }
    killed_any
}
#[tauri::command]
pub fn llm_engine_serve(engine: String, action: String) -> serde_json::Value {
    if engine != "opencode" {
        return serde_json::json!({"ok": false, "success": false, "error": "Only OpenCode has a local server"});
    }
    if action == "stop" {
        crate::features::stop_opencode_server();
        return serde_json::json!({"ok": true, "success": true, "running": false});
    }
    // start: already up (ours or Wan2GP's) → report running instead of double-spawning
    if std::net::TcpStream::connect("127.0.0.1:4096").is_ok() {
        return serde_json::json!({"ok": true, "success": true, "running": true, "url": "http://127.0.0.1:4096"});
    }
    match term_tool("opencode")
        .args(["serve", "--hostname", "127.0.0.1", "--port", "4096"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => {
            if let Ok(mut g) = OPENCODE_PID
                .get_or_init(|| std::sync::Mutex::new(None))
                .lock()
            {
                *g = Some(child.id());
            }
            std::mem::forget(child); // detach: lives beyond this command
            serde_json::json!({"ok": true, "success": true, "running": true, "url": "http://127.0.0.1:4096"})
        }
        Err(e) => {
            serde_json::json!({"ok": false, "success": false, "error": format!("Could not start opencode (is it installed?): {e}")})
        }
    }
}
#[tauri::command]
pub fn llm_engine_auth(engine: String) -> serde_json::Value {
    // Auth itself is interactive (browser OAuth / `auth login`) — the UI opens the
    // guide directly; this just hands out the canonical docs URL per engine.
    let docs = match engine.as_str() {
        "claude-code" => "https://code.claude.com/docs/en/authentication",
        "codex" => "https://developers.openai.com/codex/cli/",
        "opencode" => "https://opencode.ai/docs",
        _ => return serde_json::json!({"ok": false, "error": "Unknown engine"}),
    };
    serde_json::json!({"ok": true, "docsUrl": docs})
}
#[tauri::command]
pub fn deepy_status() -> serde_json::Value {
    let p = get_repo_dir().join("wgp_config.json");
    if !p.exists() {
        return serde_json::json!({"ok": true, "available": false, "reason": "wgp_config.json not found — install Wan2GP first."});
    }
    let Ok(s) = std::fs::read_to_string(&p) else {
        return serde_json::json!({"ok": false, "error": "cannot read wgp_config.json"});
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else {
        return serde_json::json!({"ok": false, "error": "wgp_config.json corrupted"});
    };
    let enabled = v
        .get("deepy_enabled")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    let dtype = v
        .get("deepy_type")
        .and_then(|x| x.as_str())
        .unwrap_or("zero");
    let mode = if enabled == 0 {
        "disabled"
    } else if dtype == "prime" {
        "prime"
    } else {
        "zero"
    };
    let enh = v
        .get("enhancer_enabled")
        .and_then(serde_json::Value::as_i64);
    let le = v.get("llm_engines");
    let cur_engine = le
        .and_then(|x| x.get("deepy"))
        .and_then(|x| x.as_str())
        .map(std::string::ToString::to_string)
        .unwrap_or_default();
    let prompt_enh = le
        .and_then(|x| x.get("prompt_enhancer"))
        .and_then(|x| x.as_str())
        .map(std::string::ToString::to_string);
    let engines: Vec<String> = le
        .and_then(|x| x.get("profiles"))
        .and_then(|x| x.as_object())
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    // Sessions section (upstream shared/deepy/config.py keys) — normalized
    // with upstream defaults so the launcher panel can pre-select.
    let session_mode =
        normalize_session_mode(v.get("deepy_multi_session").and_then(|x| x.as_str()));
    let reset_mode = v
        .get("deepy_session_reset_mode")
        .and_then(|x| x.as_str())
        .map(normalize_session_reset_mode)
        .unwrap_or_else(|| "new_session".into());
    let gallery_mode = v
        .get("deepy_session_gallery_media_mode")
        .and_then(|x| x.as_str())
        .map(normalize_session_gallery_mode)
        .unwrap_or_else(|| "link".into());
    // Qwen LLM quantization backend (upstream `prompt_enhancer_quantization`:
    // quanto_int8/gguf/gguf_q3/gguf_q2/gguf_ptq1) so the panel can pre-select.
    let quant = v
        .get("prompt_enhancer_quantization")
        .and_then(|x| x.as_str())
        .map(std::string::ToString::to_string);
    // Prompt-enhancement UI (upstream `enhancer_mode`: 0 = Manual Button +
    // Automatic on Generation, 1 = Manual Button Only) so the panel can pre-select.
    let enhancer_mode = v.get("enhancer_mode").and_then(serde_json::Value::as_i64);
    serde_json::json!({"ok": true, "available": true, "mode": mode, "deepyEnabled": enabled!=0, "deepyType": dtype, "currentEngine": if cur_engine.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(cur_engine) }, "promptEnhancer": prompt_enh, "enhancerEnabled": enh, "engines": engines, "promptEnhancerQuantization": quant, "sessionMode": session_mode, "sessionResetMode": reset_mode, "sessionGalleryMediaMode": gallery_mode, "enhancerMode": enhancer_mode})
}
/// Upstream `normalize_deepy_session_mode` (shared/deepy/config.py):
/// disabled/selectable/dedicated, anything else falls back to disabled.
fn normalize_session_mode(value: Option<&str>) -> String {
    match value.map(|s| s.trim().to_lowercase()).as_deref() {
        Some("disabled" | "selectable" | "dedicated") => value
            .map(|s| s.trim().to_lowercase())
            .unwrap_or_else(|| "disabled".into()),
        _ => "disabled".into(),
    }
}
/// Upstream `normalize_deepy_session_reset_mode`: reset_session or new_session.
fn normalize_session_reset_mode(value: &str) -> String {
    if value.trim().to_lowercase() == "reset_session" {
        "reset_session".into()
    } else {
        "new_session".into()
    }
}
/// Upstream `normalize_deepy_session_gallery_media_mode`: copy or link.
fn normalize_session_gallery_mode(value: &str) -> String {
    if value.trim().to_lowercase() == "copy" {
        "copy".into()
    } else {
        "link".into()
    }
}
/// Resolve a session pref: an explicitly provided valid value wins, then the
/// existing config value, then the launcher default. `None` (field absent)
/// always preserves existing config so an engine-only Apply never clobbers
/// the Sessions section.
fn resolve_session_pref(
    provided: Option<&str>,
    existing: Option<&str>,
    normalize: fn(&str) -> String,
    upstream_default: &str,
    launcher_default: Option<&str>,
) -> String {
    if let Some(p) = provided {
        let n = normalize(p);
        // A provided value that normalizes away from itself is invalid —
        // fall through to existing/default instead of writing garbage.
        if p.trim().to_lowercase() == n || (n == upstream_default && p.trim().is_empty()) {
            return n;
        }
    }
    if let Some(e) = existing {
        let n = normalize(e);
        if e.trim().to_lowercase() == n {
            return n;
        }
    }
    launcher_default.unwrap_or(upstream_default).into()
}
/// Stored Prime profile id (`llm_engines.deepy`: opencode / claude / codex /
/// qwen38_27b / qwen38_9b / qwen35_*) → `deepy_set` UI engine id (opencode / claude-code /
/// codex / local-qwen38). The auto-config enhancer-fix path reads the stored
/// profile but must call `deepy_set` with the UI id, or Prime starts fail
/// outright with "Prime requires an engine".
pub(crate) fn prime_profile_to_ui_id(profile: &str) -> &str {
    match profile.trim().to_lowercase().as_str() {
        "opencode" => "opencode",
        "claude" | "claude-code" => "claude-code",
        "codex" => "codex",
        s if s.contains("27b") || s.contains("qwen38") => "local-qwen38",
        // qwen35_* local profiles are Zero engines, not Prime ones — fall
        // back to the default remote engine rather than failing the start.
        _ => "opencode",
    }
}
/// Upstream Qwen LLM quantization backend (`prompt_enhancer_quantization`):
/// valid values per local engine (shared/prompt_enhancer/qwen35_vl.py
/// `get_qwen35_quantization`): Qwen3.8 27B (id 5) takes the four GGUF
/// backends including Bonsai PTQ1 (`gguf_ptq1`, ~10GB VRAM, needs GGUF
/// kernels 1.0.25+); Qwen3.8 9B (id 6) takes GGUF Q4 (`gguf`) or Q8
/// (`gguf_q8`, closest to full precision, ~11GB VRAM); Qwen3.5 4B/9B
/// (ids 3/4) take Quanto Int8 or plain GGUF Q4. Engine-inappropriate values
/// normalize to the engine default (mirrors upstream); `None` preserves
/// existing config.
/// Pure + unit-tested.
pub(crate) fn normalize_qwen_quant<'a>(quant: Option<&'a str>, enhancer: Option<i64>) -> Option<&'a str> {
    let q = quant.map(str::trim).filter(|s| !s.is_empty())?;
    match enhancer {
        Some(5) => match q {
            "gguf" | "gguf_q3" | "gguf_q2" | "gguf_ptq1" => Some(q),
            _ => Some("gguf"),
        },
        Some(6) => match q {
            "gguf" | "gguf_q8" => Some(q),
            _ => Some("gguf"),
        },
        Some(3) | Some(4) => match q {
            "quanto_int8" | "gguf" => Some(q),
            _ => Some("quanto_int8"),
        },
        _ => None,
    }
}
#[tauri::command]
pub fn deepy_set(
    mode: String,
    engine: Option<String>,
    enhancer: Option<serde_json::Value>,
    sessions: Option<serde_json::Value>,
    quant: Option<String>,
    enhancer_mode: Option<serde_json::Value>,
) -> serde_json::Value {
    eprintln!("[deepy_set] mode={mode} engine={engine:?} enhancer={enhancer:?} quant={quant:?} enhancer_mode={enhancer_mode:?}");
    let m = mode.trim().to_lowercase();
    if !["disabled", "zero", "prime"].contains(&m.as_str()) {
        return serde_json::json!({"ok": false, "error": format!("Unknown Deepy mode: {}", mode)});
    }
    if m == "prime"
        && !engine
            .as_deref()
            .is_some_and(|s| ["opencode", "claude-code", "codex", "local-qwen38"].contains(&s))
    {
        return serde_json::json!({"ok": false, "error": "Prime requires an engine (OpenCode / Claude Code / Codex / local Qwen3.8 9B/27B)."});
    }
    let p = get_repo_dir().join("wgp_config.json");
    if !p.exists() {
        return serde_json::json!({"ok": false, "error": "wgp_config.json not found — install Wan2GP first."});
    }
    let Ok(s) = std::fs::read_to_string(&p) else {
        return serde_json::json!({"ok": false, "error": "cannot read wgp_config.json"});
    };
    let mut v: serde_json::Value = match serde_json::from_str(&s) {
        Ok(x) => x,
        Err(e) => {
            return serde_json::json!({"ok": false, "error": format!("wgp_config.json corrupted: {}", e)})
        }
    };
    let bak = p.with_file_name("wgp_config.json.deepy-bak");
    let _ = std::fs::copy(&p, &bak);
    // F11: timestamped snapshot alongside the legacy single backup.
    let _ = snapshot_wgp_config();
    let (enabled, dtype) = match m.as_str() {
        "disabled" => (0, "zero"),
        "prime" => (1, "prime"),
        _ => (1, "zero"),
    };
    v["deepy_enabled"] = serde_json::json!(enabled);
    v["deepy_type"] = serde_json::json!(dtype);
    // Sessions section: explicit choice wins, otherwise keep the existing
    // config value, otherwise the launcher default (selectable workspace —
    // one shared outputs folder; upstream default is disabled).
    let sess = sessions.as_ref();
    let sess_str = |key: &str| sess.and_then(|s| s.get(key)).and_then(|x| x.as_str());
    let cfg_str = |key: &str| v.get(key).and_then(|x| x.as_str());
    let multi = resolve_session_pref(
        sess_str("multi_session"),
        cfg_str("deepy_multi_session"),
        |s| normalize_session_mode(Some(s)),
        "disabled",
        Some("selectable"),
    );
    let reset = resolve_session_pref(
        sess_str("reset_mode"),
        cfg_str("deepy_session_reset_mode"),
        normalize_session_reset_mode,
        "new_session",
        None,
    );
    let gallery = resolve_session_pref(
        sess_str("gallery_media_mode"),
        cfg_str("deepy_session_gallery_media_mode"),
        normalize_session_gallery_mode,
        "link",
        None,
    );
    // enhancer id — JS sends number (3) or null, handle both string/number.
    // Enforce valid mode↔id pairs like Electron's resolveEnhancerId: Zero only
    // runs on Qwen (3/4/5) — a Llama id (1/2) with Zero is the tokenizer-crash
    // combo, so fall back to the mode default instead of writing it. Disabled
    // runs the enhancer standalone, so any local model (1-5) is valid there.
    let raw_id: Option<i64> = enhancer.as_ref().and_then(|v| {
        if let Some(n) = v.as_i64() {
            Some(n)
        } else if let Some(s) = v.as_str() {
            s.parse::<i64>().ok()
        } else {
            None
        }
    });
    let enh_id: Option<i64> = match m.as_str() {
        "prime" => None,
        "zero" => Some(match raw_id {
            Some(3) | Some(4) | Some(5) | Some(6) => raw_id.unwrap(),
            _ => 3,
        }),
        _ => Some(match raw_id {
            Some(1) | Some(2) | Some(3) | Some(4) | Some(5) | Some(6) => raw_id.unwrap(),
            _ => 1,
        }),
    };
    if m != "prime" {
        if let Some(id) = enh_id {
            v["enhancer_enabled"] = serde_json::json!(id);
        }
    }
    // Qwen LLM quantization (upstream "Qwen LLM Quantization" dropdown):
    // applies only with a local Qwen engine — Disabled/Zero on 3/4/5/6, or
    // Prime on local Qwen3.8 (id 5 = 27B, id 6 = 9B). Engine-inappropriate
    // values normalize to the engine default; absent quant preserves existing
    // config (e.g. the auto-config fix path must not clobber a chosen Bonsai
    // backend).
    // For local Prime the variant sticks: an explicit panel pick (27B id 5 /
    // 9B id 6 from the Prime-local variant selector), else an explicit
    // gguf_q8 quant or a stored enhancer 6 / qwen38_9b profile, otherwise 27B.
    let requested_56: Option<i64> = raw_id.filter(|id| *id == 5 || *id == 6);
    // (Zero/Disabled keep using raw_id via enh_id below; this is Prime-only.)
    let prime_local_variant: Option<i64> = if m == "prime" && engine.as_deref() == Some("local-qwen38") {
        if requested_56.is_some() {
            requested_56
        } else {
            let stored_6 = v.get("enhancer_enabled").and_then(|x| x.as_i64()) == Some(6);
            let stored_9b = v.get("llm_engines").and_then(|l| l.get("deepy")).and_then(|x| x.as_str()) == Some("qwen38_9b");
            let wants_q8 = quant.as_deref().is_some_and(|q| q.trim() == "gguf_q8");
            if wants_q8 || stored_6 || stored_9b { Some(6) } else { Some(5) }
        }
    } else {
        None
    };
    let quant_enhancer: Option<i64> = match m.as_str() {
        "zero" | "disabled" => enh_id.filter(|id| [3, 4, 5, 6].contains(id)),
        "prime" => prime_local_variant,
        _ => None,
    };
    if let Some(q) = normalize_qwen_quant(quant.as_deref(), quant_enhancer) {
        v["prompt_enhancer_quantization"] = serde_json::json!(q);
        if q == "gguf_ptq1" {
            // Bonsai companion: INT8 KV cache halves cache VRAM — what makes
            // Prime viable at ~10GB (GGUF 1.0.25+ carries the kernels).
            // (Prompt enhancement mode is handled below from the panel choice.)
            v["deepy_kv_cache_quantization"] = serde_json::json!("int8");
        }
    }
    // Prompt enhancement UI (upstream `enhancer_mode`: 0 = Manual Button +
    // Automatic on Generation, 1 = Manual Button Only; the button stays in
    // both modes).
    // Explicit 0/1 wins; otherwise the existing config value sticks; a missing
    // key defaults to 1 (button). Deepy tool templates carry no flags of their
    // own, so they follow it; per-model/template prompt_enhancer flags stay ""
    // as shipped. Applies in every mode — the enhancer (and its button) runs
    // standalone when Deepy is disabled.
    let raw_emode: Option<i64> = enhancer_mode.as_ref().and_then(|v| {
        if let Some(n) = v.as_i64() {
            Some(n)
        } else if let Some(s) = v.as_str() {
            s.parse::<i64>().ok()
        } else {
            None
        }
    });
    let emode = match raw_emode {
        Some(0) | Some(1) => raw_emode.unwrap(),
        _ => v.get("enhancer_mode").and_then(serde_json::Value::as_i64).filter(|n| *n == 0 || *n == 1).unwrap_or(1),
    };
    v["enhancer_mode"] = serde_json::json!(emode);
    // llm_engines deepy
    let eng_map = |id: &str| match id {
        "opencode" => "opencode",
        "claude-code" => "claude",
        "codex" => "codex",
        _ => "opencode",
    };
    let exe_map = |id: &str| match id {
        "opencode" => "opencode",
        "claude-code" => "claude",
        "codex" => "codex",
        _ => "opencode",
    };
    let enh_to_engine = |id: i64| match id {
        1 => "local_florence_llama32",
        2 => "local_florence_llamajoy",
        3 => "qwen35_4b",
        4 => "qwen35_9b",
        5 => "qwen38_27b",
        6 => "qwen38_9b",
        _ => "qwen35_4b",
    };
    if v.get("llm_engines").is_none() {
        v["llm_engines"] = serde_json::json!({});
    }
    if m == "prime" {
        let eid = engine.clone().unwrap_or_else(|| "opencode".into());
        if eid == "local-qwen38" {
            // Local Prime: Qwen3.8 VL 27B (id 5) or 9B Heretic (id 6, ~6.5GB
            // Q4 / ~11GB Q8) — upstream supports both since v13.1315.
            // Either needs context >= 32000 and Summarize compaction, and the
            // Configuration UI auto-raises both, so mirror that here.
            let variant = prime_local_variant.unwrap_or(5);
            let profile = if variant == 6 { "qwen38_9b" } else { "qwen38_27b" };
            v["enhancer_enabled"] = serde_json::json!(variant);
            v["llm_engines"]["deepy"] = serde_json::json!(profile);
            v["llm_engines"]["prompt_enhancer"] = serde_json::json!("same_as_deepy");
            if v.get("deepy_context_tokens")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0)
                < 32000
            {
                v["deepy_context_tokens"] = serde_json::json!(32000);
            }
            v["deepy_compaction_type"] = serde_json::json!("summarize");
            v["deepy_repetition_penalty"] = serde_json::json!(true);
        } else {
            let profile = eng_map(&eid);
            let exe = exe_map(&eid);
            v["llm_engines"]["deepy"] = serde_json::json!(profile);
            v["llm_engines"]["prompt_enhancer"] = serde_json::json!("same_as_deepy");
            if v["llm_engines"]["profiles"].is_null() {
                v["llm_engines"]["profiles"] = serde_json::json!({});
            }
            if v["llm_engines"]["profiles"][profile].is_null() {
                v["llm_engines"]["profiles"][profile] = serde_json::json!({});
            }
            v["llm_engines"]["profiles"][profile]["executable"] = serde_json::json!(exe);
            if profile == "opencode" {
                v["llm_engines"]["profiles"]["opencode"]["base_url"] =
                    serde_json::json!("http://127.0.0.1:4096");
            }
        }
        // Full Prime preset (mirrors shared/deepy/config.py defaults + guidance).
        for (k, val) in [
            ("deepy_prime_custom_system_prompt", serde_json::json!("When several models can satisfy the request, prefer the highest-quality base or full model unless the user explicitly prioritizes speed or names another model.")),
            ("deepy_prime_mcp_servers", serde_json::json!({})),
            ("deepy_mcp_auto_discover_paths", serde_json::json!(false)),
            ("deepy_allow_read_file_system", serde_json::json!(false)),
            ("deepy_file_system_paths", serde_json::json!([])),
            ("deepy_read_everywhere", serde_json::json!(false)),
            ("deepy_auto_cancel_queue_tasks", serde_json::json!(true)),
            ("deepy_separate_requests_with_empty_line", serde_json::json!(true)),
            // v12.72 sessions feature — resolved above (explicit choice >
            // existing config > launcher default) so pre-existing configs
            // can't skew-missing when wgp.py expects the keys.
            ("deepy_session_reset_mode", serde_json::json!(reset)),
            ("deepy_session_gallery_media_mode", serde_json::json!(gallery)),
            ("deepy_multi_session", serde_json::json!(multi)),
        ] { v[k] = val; }
    } else {
        let eid = enh_id.unwrap_or(1);
        let eng_str = enh_to_engine(eid);
        v["llm_engines"]["deepy"] = serde_json::json!(eng_str);
        v["llm_engines"]["prompt_enhancer"] = serde_json::json!("same_as_deepy");
        if m == "zero" {
            // Full Zero preset (mirrors shared/deepy/config.py defaults + tool picks).
            for (k, val) in [
                ("deepy_vram_mode", serde_json::json!("unload")),
                ("deepy_context_tokens", serde_json::json!(16386)),
                ("deepy_kv_cache_quantization", serde_json::json!("auto")),
                ("deepy_repetition_penalty", serde_json::json!(true)),
                ("deepy_compaction_type", serde_json::json!("discard")),
                (
                    "deepy_tool_gen_image",
                    serde_json::json!("Krea 2 Turbo (8 Steps)"),
                ),
                ("deepy_tool_edit_image", serde_json::json!("Flux Klein 9B")),
                (
                    "deepy_tool_gen_video",
                    serde_json::json!("LTX-2 2.5 Distilled"),
                ),
                (
                    "deepy_tool_gen_video_with_speech",
                    serde_json::json!("LTX-2.5 Distilled With Sound"),
                ),
                (
                    "deepy_tool_gen_song",
                    serde_json::json!("ACE-Step 1.5 Turbo LM 1.7B"),
                ),
                (
                    "deepy_tool_gen_speech_from_description",
                    serde_json::json!("Qwen3 1.7B"),
                ),
                (
                    "deepy_tool_gen_speech_from_sample",
                    serde_json::json!("Index TTS 2"),
                ),
                ("deepy_zero_custom_system_prompt", serde_json::json!("")),
                ("deepy_auto_cancel_queue_tasks", serde_json::json!(true)),
                (
                    "deepy_separate_requests_with_empty_line",
                    serde_json::json!(true),
                ),
                // v12.72 sessions feature — same resolution as Prime preset.
                ("deepy_session_reset_mode", serde_json::json!(reset)),
                (
                    "deepy_session_gallery_media_mode",
                    serde_json::json!(gallery),
                ),
                ("deepy_multi_session", serde_json::json!(multi)),
            ] {
                v[k] = val;
            }
        }
    }
    if atomic_write(&p, &serde_json::to_string_pretty(&v).unwrap_or_default()).is_err() {
        return serde_json::json!({"ok": false, "error": "failed to write wgp_config.json"});
    }
    let prime_label = match engine.as_deref().unwrap_or("opencode") {
        "local-qwen38" => "Qwen3.8 VL 9B/27B (local)".into(),
        other => other.to_string(),
    };
    let msg = if m == "prime" {
        format!(
            "Deepy Prime set to {prime_label}. Launch Wan2GP and click \"Ask Deepy\"."
        )
    } else if m == "zero" {
        "Deepy Zero enabled (local model). Launch Wan2GP and click \"Ask Deepy\".".into()
    } else {
        "Deepy disabled. Prompt-enhancer settings saved — relaunch Wan2GP to apply.".into()
    };
    serde_json::json!({"ok": true, "mode": m, "enhancerId": enh_id, "backup": bak.to_string_lossy().to_string(), "message": msg})
}
#[tauri::command]
pub fn deepy_activate(engine: String) -> serde_json::Value {
    deepy_set("prime".into(), Some(engine), None, None, None, None)
}
// Auto-start via the per-user Run key (no admin needed). Returns success, like the UI checks.
#[tauri::command]
pub fn set_auto_start(enabled: bool) -> serde_json::Value {
    #[cfg(windows)]
    {
        let exe = std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
        let ok = if enabled && !exe.is_empty() {
            silent_command("reg")
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
            silent_command("reg")
                .args(["delete", key, "/v", "Wan2GPDesktop", "/f"])
                .output()
                .is_ok_and(|o| o.status.success())
        };
        serde_json::json!({"ok": ok, "success": ok, "enabled": enabled})
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        return serde_json::json!({"ok": false, "success": false, "error": "auto-start is Windows-only"});
    }
}
#[tauri::command]
/// Fail-closed gate for the new upstream string enums (upstream raises
/// ValueError on unknown selections, so garbage must never reach
/// wgp_config.json). Other memory keys are validated by their own panels
/// or are numeric upstream choices — this gate covers only the string
/// enums it names. Pure + unit-tested.
pub(crate) fn valid_memory_override(key: &str, val: &serde_json::Value) -> bool {
    let s = val.as_str().unwrap_or("");
    match key {
        // shared/kernels/int8_backend.py CHOICES
        "int8_kernels" => ["auto", "disabled", "triton", "kitchen"].contains(&s),
        // shared/kernels/kernel_policy.py CHOICES ("strict"/"fast")
        "kernel_precision" => ["fast", "strict"].contains(&s),
        // shared/cuda_memory.py VRAM_ALLOCATOR_CHOICES
        "vram_allocator" => ["default", "vmm", "vmm_spill"].contains(&s),
        // shared/attention_kit.py HEAD_SPLIT_CHOICES: int 0..3
        "attention_head_split" => matches!(val.as_u64(), Some(0..=3)),
        // bool knobs added with the MMGP v4 runtime
        "read_ahead" | "smart_memory_pinning" => val.is_boolean(),
        // wgp.py preload_mode() — "default" | "dynamic" | "manual"
        "video_preload_mode" | "image_preload_mode" | "audio_preload_mode" => {
            ["default", "dynamic", "manual"].contains(&s)
        }
        "video_preload_in_VRAM" | "image_preload_in_VRAM" | "audio_preload_in_VRAM" => {
            matches!(val.as_i64(), Some(n) if (0..=200_000).contains(&n))
        }
        // wgp.py: perc_reserved_mem_max is a PERCENT as of v17 (0 = automatic)
        "perc_reserved_mem_max" => matches!(val.as_f64(), Some(n) if (0.0..=100.0).contains(&n)),
        // wgp.py attention_modes_installed + check_attn
        "attention_mode" => ["sdpa", "flash", "sage", "sage2", "sage3", "radial"].contains(&s),
        "compile" => val.is_boolean(),
        // wgp.py queue table: "pastel" renders per-row hues, anything else
        // renders the theme-following alternating grey rows. Fail-closed to
        // the two known names so garbage never reaches wgp_config.json.
        "queue_color_scheme" => ["pastel", "grey"].contains(&s),
        _ => true,
    }
}
/// Keys the Performance Settings panel owns in `wgp_config.json`. Single
/// source of truth for the Apply allowlist (a key missing here is silently
/// dropped, not validated) and for install-time seeding.
pub(crate) const MEMORY_OVERRIDE_KEYS: &[&str] = &[
    "video_profile",
    "image_profile",
    "audio_profile",
    "vram_safety_coefficient",
    "vae_config",
    "transformer_quantization",
    "int8_kernels",
    "kernel_precision",
    "queue_color_scheme",
    // v17 (MMGP v4) RAM/VRAM controls.
    "vram_allocator",
    "attention_head_split",
    "read_ahead",
    "smart_memory_pinning",
    "video_preload_mode",
    "image_preload_mode",
    "audio_preload_mode",
    "perc_reserved_mem_max",
    // Troubleshooting "known-good recipe" writes these two; both verified to
    // exist in a live v17 wgp_config.json.
    "attention_mode",
    "compile",
];

/// Merge a recommendation into a `wgp_config` document for a FRESH install,
/// so the first launch already runs with the hardware-aware values instead of
/// upstream's generic defaults.
///
/// Setdefault-only, and it is called from `install()` alone: a key already on
/// disk belongs to the user or to WanGP and is never touched, which is what
/// keeps "the launcher auto-recommends" from becoming "the launcher
/// overwrites". Keys the recommendation does not carry (every CUDA-only lever
/// on AMD/Intel/CPU) and keys that fail `valid_memory_override` are skipped.
pub(crate) fn seed_memory_defaults(
    cfg: &mut serde_json::Value,
    rec: &serde_json::Value,
) -> Vec<String> {
    let Some(map) = cfg.as_object_mut() else {
        return Vec::new();
    };
    let mut seeded = Vec::new();
    for key in MEMORY_OVERRIDE_KEYS {
        let Some(v) = rec.get(*key) else { continue };
        if !valid_memory_override(key, v) || map.contains_key(*key) {
            continue;
        }
        map.insert((*key).to_string(), v.clone());
        seeded.push((*key).to_string());
    }
    seeded
}

#[tauri::command]
pub fn memory_profile_apply(settings: serde_json::Value) -> serde_json::Value {
    // mirrors Electron memory_profile:apply — writes to wgp_config.json and returns applied keys
    let p = get_repo_dir().join("wgp_config.json");
    let Ok(s) = std::fs::read_to_string(&p) else {
        return serde_json::json!({"ok": false, "success": false, "error": "wgp_config.json not found"});
    };
    let mut cfg: serde_json::Value = match serde_json::from_str(&s) {
        Ok(v) => v,
        Err(e) => {
            return serde_json::json!({"ok": false, "success": false, "error": format!("corrupted: {}", e)})
        }
    };
    // WanGP keeps its own copy of wgp_config.json in memory and rewrites the
    // file on any settings change inside its UI. Writing while it runs is a
    // lost update in whichever direction the user did not expect — the panel
    // would report success and the value would silently revert. Refuse and
    // name the fix; we own start/stop, so stopping first is one click.
    if crate::launch::wangp_running() {
        return serde_json::json!({
            "ok": false,
            "success": false,
            "error": "WanGP is running — stop it first, then apply. It rewrites wgp_config.json from its own copy and would overwrite these values.",
        });
    }
    let mut applied: Vec<String> = Vec::new();
    for key in MEMORY_OVERRIDE_KEYS {
        if let Some(val) = settings.get(*key) {
            if !valid_memory_override(key, val) {
                return serde_json::json!({"ok": false, "success": false, "error": format!("{key}: invalid value")});
            }
            cfg[key] = val.clone();
            applied.push(key.to_string());
        }
    }
    // Drop the deleted legacy key when the new one is written so upstream
    // never sees a stale enable_int8_kernels (its own migration deletes it
    // on next launch anyway; this just avoids the confusion sooner).
    if settings.get("int8_kernels").is_some() {
        if let Some(m) = cfg.as_object_mut() {
            m.remove("enable_int8_kernels");
        }
    }
    if applied.is_empty() {
        return serde_json::json!({"ok": true, "success": true, "applied": applied, "unchanged": true});
    }
    // F11: snapshot before overwriting so every Apply is restorable.
    let snapshot = snapshot_wgp_config();
    if atomic_write(&p, &serde_json::to_string_pretty(&cfg).unwrap_or_default()).is_err() {
        return serde_json::json!({"ok": false, "success": false, "error": "write failed"});
    }
    serde_json::json!({"ok": true, "success": true, "applied": applied, "snapshot": snapshot})
}
// ── Queue Notifier (Apprise) ──
// Config lives in desktop-config.json under "notifier". Events are classified
// from the launch-log stream (port of services/queue-notifier.js) and delivered
// via the env's apprise console script (binary first, `python -m` fallback).
// Spawns a thread per delivery so log streaming never blocks.
static NOTIF_LAST_PCT: std::sync::OnceLock<std::sync::Mutex<Option<u8>>> =
    std::sync::OnceLock::new();
static NOTIF_LAST_FIRE: std::sync::OnceLock<
    std::sync::Mutex<Option<(std::time::Instant, String)>>,
> = std::sync::OnceLock::new();
pub(crate) fn notifier_normalize(cfg: &serde_json::Value) -> serde_json::Value {
    let step = cfg
        .get("progressStep")
        .and_then(|v| v.as_u64())
        .unwrap_or(25)
        .clamp(1, 100);
    serde_json::json!({
        "enabled": cfg.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false),
        "url": cfg.get("url").and_then(|v| v.as_str()).unwrap_or("").trim(),
        "notifyOnComplete": cfg.get("notifyOnComplete").and_then(|v| v.as_bool()).unwrap_or(true),
        "notifyOnFail": cfg.get("notifyOnFail").and_then(|v| v.as_bool()).unwrap_or(true),
        "notifyOnProgress": cfg.get("notifyOnProgress").and_then(|v| v.as_bool()).unwrap_or(false),
        "progressStep": step,
        // Set by the native-notifications assistant once Wan2GP itself sends
        // events: the log-driven launcher sender stays off (no double pings).
        "nativeManaged": cfg.get("nativeManaged").and_then(|v| v.as_bool()).unwrap_or(false)
    })
}
fn notifier_saved() -> serde_json::Value {
    notifier_normalize(
        load_config_value()
            .get("notifier")
            .unwrap_or(&serde_json::Value::Null),
    )
}
fn env_python_bin() -> Option<PathBuf> {
    // Single resolution point for package ops: uv/venv (Scripts\ or bin/)
    // and conda (python at the env root) alike.
    let env = get_active_env();
    let raw = env.get("path")?.as_str()?;
    resolve_env_python(&get_repo_dir(), raw)
}
/// Build the apprise invocation for the active env.
/// Prefers the `pip install apprise` console script (Scripts\apprise.exe on
/// Windows, bin/apprise elsewhere — the only entry upstream's pinned
/// apprise==1.12.0 ships: `console_scripts apprise = apprise.cli:main`, no
/// __main__.py) and falls back to `python -m apprise` for distributions that
/// provide it. Issue #35: `-m` failed with
/// "No module named apprise.__main__" while `import apprise` succeeded, so
/// delivery must not assume `-m` works. Pure over an explicit path so it is
/// unit-testable.
pub(crate) fn apprise_argv(py: &PathBuf, title: &str, body: &str, url: &str) -> (PathBuf, Vec<String>) {
    #[cfg(windows)]
    let bin = py.parent().map(|d| d.join("apprise.exe"));
    #[cfg(not(windows))]
    let bin = py.parent().map(|d| d.join("apprise"));
    if let Some(b) = bin.filter(|b| b.is_file()) {
        (
            b,
            vec![
                "-t".to_string(),
                title.to_string(),
                "-b".to_string(),
                body.to_string(),
                url.to_string(),
            ],
        )
    } else {
        (
            py.clone(),
            vec![
                "-m".to_string(),
                "apprise".to_string(),
                "-t".to_string(),
                title.to_string(),
                "-b".to_string(),
                body.to_string(),
                url.to_string(),
            ],
        )
    }
}
fn apprise_send(url: &str, title: &str, body: &str) -> Result<(), String> {
    let py = env_python_bin().ok_or_else(|| "No active Python environment".to_string())?;
    let (cmd, args) = apprise_argv(&py, title, body, url);
    let out = silent_command(&cmd)
        .args(&args)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "apprise failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}
pub(crate) fn notifier_fire(kind: &str, text: &str) {
    let cfg = notifier_saved();
    // Native Wan2GP notifications active → stay silent (no double pings).
    if cfg
        .get("nativeManaged")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return;
    }
    if !cfg
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return;
    }
    let url = cfg
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if url.is_empty() {
        return;
    }
    let subscribed = match kind {
        "complete" => cfg
            .get("notifyOnComplete")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        "fail" => cfg
            .get("notifyOnFail")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        "progress" => cfg
            .get("notifyOnProgress")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        _ => false,
    };
    if !subscribed {
        return;
    }
    // 10s cooldown per kind (log bursts shouldn't spam phones)
    if let Some(m) = NOTIF_LAST_FIRE.get() {
        if let Ok(g) = m.lock() {
            if let Some((t, k)) = g.as_ref() {
                if k == kind && t.elapsed() < std::time::Duration::from_secs(10) {
                    return;
                }
            }
        }
    }
    if let Ok(mut g) = NOTIF_LAST_FIRE
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
    {
        *g = Some((std::time::Instant::now(), kind.to_string()));
    }
    let msg = match kind {
        "complete" => "Wan2GP: \u{2705} generation finished".to_string(),
        "fail" => format!(
            "Wan2GP: \u{274C} generation failed\n{}",
            text.chars().take(280).collect::<String>()
        ),
        _ => format!("Wan2GP: {text}"),
    };
    std::thread::spawn(move || {
        let _ = apprise_send(&url, "Wan2GP", &msg);
    });
}
const DONE_MARKERS: &[&str] = &[
    "task completed",
    "generation complete",
    "finished",
    "saved to",
    "output saved",
    "done in",
    "job finished",
    "completed successfully",
    "\u{2713}",
    "\u{2714}",
];
const FAIL_MARKERS: &[&str] = &[
    "traceback",
    "exception",
    "cuda out of memory",
    "out of memory",
    "assertionerror",
    "runtimeerror",
    "failed",
    "error",
];
const FAIL_SKIP: &[&str] = &[
    "0 error",
    "no error",
    "error_queue",
    "errors: 0",
    "0 errors",
];
// Classify one launch-log line; progress gated on progressStep multiples.
pub(crate) fn notifier_scan_line(line: &str) {
    // progress: last NN% in line (tqdm prints 50%|...)
    let mut pct: Option<u8> = None;
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let mut j = i;
            while j > 0 && bytes[j - 1].is_ascii_digit() {
                j -= 1;
            }
            if j < i {
                if let Ok(n) = line[j..i].parse::<u8>() {
                    if n <= 100 {
                        pct = Some(n);
                    }
                }
            }
        }
        i += 1;
    }
    if let Some(p) = pct {
        if p < 100 {
            let cfg = notifier_saved();
            if cfg
                .get("notifyOnProgress")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                let step = cfg
                    .get("progressStep")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(25)
                    .max(1);
                let last = NOTIF_LAST_PCT
                    .get()
                    .and_then(|m| m.lock().ok())
                    .and_then(|g| *g)
                    .unwrap_or(0);
                if p / (step as u8) > last / (step as u8) {
                    if let Ok(mut g) = NOTIF_LAST_PCT
                        .get_or_init(|| std::sync::Mutex::new(None))
                        .lock()
                    {
                        *g = Some(p);
                    }
                    notifier_fire("progress", &format!("{p}% done"));
                }
            }
            return;
        }
    }
    let low = line.to_lowercase();
    if FAIL_MARKERS.iter().any(|m| low.contains(m)) && !FAIL_SKIP.iter().any(|s| low.contains(s)) {
        if let Ok(mut g) = NOTIF_LAST_PCT
            .get_or_init(|| std::sync::Mutex::new(None))
            .lock()
        {
            *g = None;
        }
        notifier_fire("fail", line.trim());
        return;
    }
    if DONE_MARKERS.iter().any(|m| low.contains(m)) {
        if let Ok(mut g) = NOTIF_LAST_PCT
            .get_or_init(|| std::sync::Mutex::new(None))
            .lock()
        {
            *g = None;
        }
        notifier_fire("complete", line.trim());
    }
}
#[tauri::command]
pub fn notifier_config() -> serde_json::Value {
    serde_json::json!({"ok": true, "config": notifier_saved()})
}
#[tauri::command]
pub fn notifier_set(cfg: serde_json::Value) -> serde_json::Value {
    let mut clean = notifier_normalize(&cfg);
    // The nativeManaged marker is owned by the native assistant — a legacy
    // save must never clobber it (that would silently re-arm double pings).
    let stored_managed = notifier_saved()
        .get("nativeManaged")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if let Some(m) = clean.as_object_mut() {
        m.insert("nativeManaged".into(), serde_json::json!(stored_managed));
    }
    if clean
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        && clean
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .is_empty()
    {
        return serde_json::json!({"ok": false, "error": "A delivery URL is required when notifications are enabled (Apprise URL, e.g. discord://, tgram://)"});
    }
    // Refuse re-enabling the legacy sender while native events are on —
    // that would ping every destination twice. Turn the native events off
    // in the Notifications section above first.
    if clean
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        && stored_managed
    {
        return serde_json::json!({"ok": false, "error": "Native Wan2GP notifications are active — turn them off above before enabling the launcher sender, or you will get every notification twice."});
    }
    let mut full = load_config_value();
    if let Some(m) = full.as_object_mut() {
        m.insert("notifier".into(), clean.clone());
    }
    match crate::config::config_save(full) {
        Ok(_) => serde_json::json!({"ok": true, "config": clean}),
        Err(e) => serde_json::json!({"ok": false, "error": e}),
    }
}
#[tauri::command]
pub fn notifier_test(cfg: Option<serde_json::Value>) -> serde_json::Value {
    let use_cfg = cfg
        .map(|c| notifier_normalize(&c))
        .unwrap_or_else(notifier_saved);
    let url = use_cfg.get("url").and_then(|v| v.as_str()).unwrap_or("");
    if url.is_empty() {
        return serde_json::json!({"ok": false, "error": "No Apprise URL set"});
    }
    match apprise_send(
        url,
        "Wan2GP",
        "Wan2GP test notification \u{2705} — your queue notifier works.",
    ) {
        Ok(()) => serde_json::json!({"ok": true}),
        Err(e) => serde_json::json!({"ok": false, "error": e}),
    }
}
#[tauri::command]
pub fn set_theme_follow_system(enabled: bool) -> serde_json::Value {
    let mut full = load_config_value();
    if let Some(m) = full.as_object_mut() {
        m.insert("themeFollowSystem".into(), serde_json::json!(enabled));
    }
    match crate::config::config_save(full) {
        Ok(_) => serde_json::json!({"ok": true}),
        Err(e) => serde_json::json!({"ok": false, "error": e}),
    }
}
#[tauri::command]
pub fn set_notifications_enabled(enabled: bool) -> serde_json::Value {
    let _ = enabled;
    serde_json::json!({"ok": true})
}

#[cfg(test)]
mod deepy_roundtrip_tests {
    use super::*;
    /// Minimal stand-in for a shipped Wan2GP install. Seeded without
    /// `enhancer_mode` so the first apply exercises the documented default.
    const SEED_CFG: &str = r#"{"llm_engines":{},"deepy_multi_session":"selectable"}"#;
    #[test]
    fn deepy_set_writes_coherent_config() {
        let td = TestDataDir::new();
        td.seed_wgp_config(SEED_CFG);
        let read_cfg = || td.read_wgp_config();
        // zero + Qwen 9B + GGUF quant
        let r = deepy_set(
            "zero".into(),
            None,
            Some(serde_json::json!(4)),
            Some(serde_json::json!({
                "multi_session": "dedicated",
                "reset_mode": "reset_session",
                "gallery_media_mode": "copy",
            })),
            Some("gguf".into()),
            None,
        );
        assert!(
            r.get("ok").and_then(|v| v.as_bool()).unwrap(),
            "zero apply failed: {r}"
        );
        let c = read_cfg();
        assert_eq!(c["deepy_enabled"], 1);
        assert_eq!(c["deepy_type"], "zero");
        assert_eq!(c["enhancer_enabled"], 4);
        assert_eq!(c["llm_engines"]["deepy"], "qwen35_9b");
        assert_eq!(c["deepy_vram_mode"], "unload");
        assert_eq!(c["deepy_context_tokens"], 16386);
        assert_eq!(c["deepy_tool_gen_image"], "Krea 2 Turbo (8 Steps)");
        // Seed had no enhancer_mode and none was passed, so the documented
        // button-only default (1) applies.
        assert_eq!(c["enhancer_mode"], 1);
        // explicit session prefs stick
        assert_eq!(c["deepy_session_reset_mode"], "reset_session");
        assert_eq!(c["deepy_session_gallery_media_mode"], "copy");
        assert_eq!(c["deepy_multi_session"], "dedicated");
        // explicit quant sticks (engine-appropriate)
        assert_eq!(c["prompt_enhancer_quantization"], "gguf");
        // Bonsai PTQ1 on a Qwen3.5 engine normalizes to its default
        let r = deepy_set(
            "zero".into(),
            None,
            Some(serde_json::json!(4)),
            None,
            Some("gguf_ptq1".into()),
            None,
        );
        assert!(r.get("ok").and_then(|v| v.as_bool()).unwrap());
        assert_eq!(read_cfg()["prompt_enhancer_quantization"], "quanto_int8");
        // explicit Automatic (0) sticks; a later None apply preserves it
        let r = deepy_set(
            "zero".into(),
            None,
            Some(serde_json::json!(4)),
            None,
            None,
            Some(serde_json::json!(0)),
        );
        assert!(r.get("ok").and_then(|v| v.as_bool()).unwrap());
        assert_eq!(read_cfg()["enhancer_mode"], 0);
        let r = deepy_set("zero".into(), None, Some(serde_json::json!(4)), None, None, None);
        assert!(r.get("ok").and_then(|v| v.as_bool()).unwrap());
        assert_eq!(read_cfg()["enhancer_mode"], 0);
        // zero + Llama id must fall back to 3 (tokenizer-crash combo)
        let r = deepy_set("zero".into(), None, Some(serde_json::json!(1)), None, None, None);
        assert!(r.get("ok").and_then(|v| v.as_bool()).unwrap());
        let c = read_cfg();
        assert_eq!(c["enhancer_enabled"], 3);
        assert_eq!(c["llm_engines"]["deepy"], "qwen35_4b");
        // prime + codex
        let r = deepy_set("prime".into(), Some("codex".into()), None, None, None, None);
        assert!(r.get("ok").and_then(|v| v.as_bool()).unwrap());
        let c = read_cfg();
        assert_eq!(c["deepy_type"], "prime");
        assert_eq!(c["llm_engines"]["deepy"], "codex");
        assert_eq!(c["llm_engines"]["profiles"]["codex"]["executable"], "codex");
        assert!(c.get("deepy_prime_mcp_servers").is_some());
        // sessions=None preserves the existing session prefs
        assert_eq!(c["deepy_session_reset_mode"], "reset_session");
        assert_eq!(c["deepy_session_gallery_media_mode"], "copy");
        assert_eq!(c["deepy_multi_session"], "dedicated");
        // prime + local Qwen3.8 + Bonsai PTQ1 sticks
        let r = deepy_set(
            "prime".into(),
            Some("local-qwen38".into()),
            None,
            None,
            Some("gguf_ptq1".into()),
            Some(serde_json::json!(1)),
        );
        assert!(r.get("ok").and_then(|v| v.as_bool()).unwrap());
        let c = read_cfg();
        assert_eq!(c["enhancer_enabled"], 5);
        assert_eq!(c["llm_engines"]["deepy"], "qwen38_27b");
        assert_eq!(c["prompt_enhancer_quantization"], "gguf_ptq1");
        // Bonsai companions ride along: button kept + INT8 KV cache.
        assert_eq!(c["enhancer_mode"], 1);
        assert_eq!(c["deepy_kv_cache_quantization"], "int8");
        // prime + local Qwen3.8 9B (explicit variant pick + Q8 quant sticks)
        let r = deepy_set(
            "prime".into(),
            Some("local-qwen38".into()),
            Some(serde_json::json!(6)),
            None,
            Some("gguf_q8".into()),
            Some(serde_json::json!(1)),
        );
        assert!(r.get("ok").and_then(|v| v.as_bool()).unwrap());
        let c = read_cfg();
        assert_eq!(c["enhancer_enabled"], 6);
        assert_eq!(c["llm_engines"]["deepy"], "qwen38_9b");
        assert_eq!(c["prompt_enhancer_quantization"], "gguf_q8");
        // disabled + Qwen: standalone enhancer — engine, quant and an
        // explicit Automatic choice all stick
        let r = deepy_set(
            "disabled".into(),
            None,
            Some(serde_json::json!(3)),
            None,
            Some("gguf".into()),
            Some(serde_json::json!(0)),
        );
        assert!(r.get("ok").and_then(|v| v.as_bool()).unwrap());
        let c = read_cfg();
        assert_eq!(c["deepy_enabled"], 0);
        assert_eq!(c["enhancer_enabled"], 3);
        assert_eq!(c["llm_engines"]["deepy"], "qwen35_4b");
        assert_eq!(c["prompt_enhancer_quantization"], "gguf");
        assert_eq!(c["enhancer_mode"], 0);
        // disabled + Florence (explicit button)
        let r = deepy_set(
            "disabled".into(),
            None,
            Some(serde_json::json!(2)),
            None,
            None,
            Some(serde_json::json!(1)),
        );
        assert!(r.get("ok").and_then(|v| v.as_bool()).unwrap());
        let c = read_cfg();
        assert_eq!(c["deepy_enabled"], 0);
        assert_eq!(c["enhancer_enabled"], 2);
        assert_eq!(c["llm_engines"]["deepy"], "local_florence_llamajoy");
        // disabled leaves the prompt-enhancement UI choice untouched
        assert_eq!(c["enhancer_mode"], 1);
    }

    /// `enhancer_mode` is sticky by design (see `deepy_set`): when the caller
    /// passes no choice, the config's existing 0/1 survives instead of being
    /// reset. The round-trip above only ever saw a fresh default, so it could
    /// not catch a regression here.
    ///
    /// One `TestDataDir` per test — it serializes on a process-wide lock, so a
    /// second one on the same thread would deadlock.
    #[test]
    fn enhancer_mode_sticks_when_no_choice_is_passed() {
        let td = TestDataDir::new();
        td.seed_wgp_config(&SEED_CFG.replace("{}", "{},\"enhancer_mode\":0"));
        let r = deepy_set(
            "zero".into(),
            None,
            Some(serde_json::json!(4)),
            None,
            None,
            None,
        );
        assert!(r.get("ok").and_then(|v| v.as_bool()).unwrap(), "{r}");
        assert_eq!(td.read_wgp_config()["enhancer_mode"], 0);
    }

    #[test]
    fn an_out_of_range_stored_enhancer_mode_falls_back_to_the_default() {
        let td = TestDataDir::new();
        td.seed_wgp_config(&SEED_CFG.replace("{}", "{},\"enhancer_mode\":7"));
        let r = deepy_set(
            "zero".into(),
            None,
            Some(serde_json::json!(4)),
            None,
            None,
            None,
        );
        assert!(r.get("ok").and_then(|v| v.as_bool()).unwrap(), "{r}");
        assert_eq!(td.read_wgp_config()["enhancer_mode"], 1);
    }

    /// `deepy_set` refuses unknown modes and writes nothing.
    #[test]
    fn deepy_set_rejects_unknown_mode_without_touching_config() {
        let td = TestDataDir::new();
        let p = td.seed_wgp_config(SEED_CFG);
        let before = std::fs::read(&p).unwrap();
        let r = deepy_set("bogus".into(), None, None, None, None, None);
        assert!(!r.get("ok").and_then(|v| v.as_bool()).unwrap(), "{r}");
        assert_eq!(std::fs::read(&p).unwrap(), before);
    }
}

#[cfg(test)]
mod ram_probe_tests {
    use super::ram_gb_from_probe;
    #[test]
    fn integer_bytes_are_culture_proof() {
        // 80 GiB and 32 GiB as the new probe emits them (no separator to
        // mangle); surrounding whitespace tolerated.
        assert_eq!(ram_gb_from_probe("85899345920"), Some(80.0));
        assert_eq!(ram_gb_from_probe("  34359738368\n"), Some(32.0));
    }
    #[test]
    fn legacy_decimal_gb_accepted_both_separators() {
        // The pl-PL "79,8" that used to fall back to 32.0, and "79.8".
        assert_eq!(ram_gb_from_probe("79,8"), Some(79.8));
        assert_eq!(ram_gb_from_probe("79.8"), Some(79.8));
    }
    #[test]
    fn garbage_rejected() {
        for bad in ["", "   ", "abc", "12.3.4", "1,2,3", "12,34.56", "-5", "0", "-79,8", "NaN", "inf"] {
            assert_eq!(ram_gb_from_probe(bad), None, "{bad:?}");
        }
    }
    #[test]
    fn tiny_plain_numbers_rejected_not_gigabytes() {
        // Truncated/error output ("512", "32") must not tier as 512GB/32GB
        // (that picked P1/P4, too-large models) — None fails closed to the
        // conservative fallback instead.
        for bad in ["512", "32", "64", "8", "1023"] {
            assert_eq!(ram_gb_from_probe(bad), None, "{bad:?}");
        }
    }
}
#[cfg(test)]
mod autotune_matrix_tests {
    use super::*;
    fn rec(vram_tier: &str, ram_tier: &str, vram_gb: f64, failsafe: bool) -> serde_json::Value {
        auto_tune_recommend(
            Some(
                serde_json::json!({"vram_tier": vram_tier, "ram_tier": ram_tier, "gpu_vram_gb": vram_gb, "cuda_available": true}),
            ),
            Some(serde_json::json!({"failsafe": failsafe})),
        )
    }
    #[test]
    fn matrix_matches_reference() {
        // (vram, ram) -> (video, audio)
        let cases = [
            (("high", "high"), (1.0, 1.0)),
            (("high", "low"), (3.0, 3.0)),
            (("high", "very_low"), (3.5, 3.5)), // P3+ is the audio default upstream
            (("low", "high"), (2.0, 3.5)),
            (("low", "low"), (4.0, 3.5)),
            // Video already at P5 → nothing left to spare; audio follows down.
            (("low", "very_low"), (5.0, 5.0)),
            (("tight", "high"), (4.0, 3.5)),
            // v17 + MMGP v4: the tight tier rides P4 with the levers on, and
            // audio still gets P3+ — audio models fit whole where a 14B video
            // model would not.
            (("tight", "low"), (4.0, 3.5)),
            (("tight", "very_low"), (4.0, 3.5)),
        ];
        for ((vt, rt), (v, a)) in cases {
            let r = rec(vt, rt, if vt == "tight" { 8.0 } else { 16.0 }, false);
            assert_eq!(r["video_profile"].as_f64().unwrap(), v, "{vt}/{rt} video");
            assert_eq!(r["audio_profile"].as_f64().unwrap(), a, "{vt}/{rt} audio");
            assert_eq!(r["vae_config"], 0);
            assert_eq!(r["transformer_quantization"], "int8");
            // Upstream v13.13 kernel settings (legacy enable_int8_kernels is gone).
            assert_eq!(r["int8_kernels"], "auto");
            assert_eq!(r["kernel_precision"], "fast");
            assert!(r.get("enable_int8_kernels").is_none());
        }
        // failsafe forces P5 + 0.60
        let r = rec("high", "high", 48.0, true);
        assert_eq!(r["video_profile"], 5.0);
        assert_eq!(r["vram_safety_coefficient"], 0.60);
        // no usable GPU → labeled fallback, not silent P4
        let r = auto_tune_recommend(
            Some(
                serde_json::json!({"vram_tier": "none", "ram_tier": "low", "gpu_vram_gb": 0, "cuda_available": false}),
            ),
            None,
        );
        assert_eq!(r["video_profile"], 4.5);
        assert!(r["_recommendation_label"]
            .as_str()
            .unwrap()
            .contains("unavailable"));
    }
    /// v17 levers are CUDA-only: shared/cuda_memory.apply_startup_settings
    /// early-returns on HIP and on !cuda_available, so AMD/Intel must not
    /// have them persisted as if they did something.
    #[test]
    fn v17_levers_are_cuda_only() {
        let cuda = auto_tune_recommend(
            Some(serde_json::json!({
                "vram_tier": "tight", "ram_tier": "low", "gpu_vram_gb": 10.0,
                "cuda_available": true, "vendor": "NVIDIA",
            })),
            None,
        );
        assert_eq!(cuda["vram_allocator"], "vmm_spill");
        assert_eq!(cuda["attention_head_split"], 2, "tight VRAM buys head split");
        assert_eq!(cuda["image_preload_mode"], "dynamic");
        assert_eq!(cuda["smart_memory_pinning"], true);
        // Plenty of VRAM: head split off, everything else unchanged.
        let roomy = auto_tune_recommend(
            Some(serde_json::json!({
                "vram_tier": "high", "ram_tier": "high", "gpu_vram_gb": 24.0,
                "cuda_available": true, "vendor": "NVIDIA",
            })),
            None,
        );
        assert_eq!(roomy["attention_head_split"], 0);

        let amd = auto_tune_recommend(
            Some(serde_json::json!({
                "vram_tier": "low", "ram_tier": "low", "gpu_vram_gb": 16.0,
                "cuda_available": false, "gpu_available": true, "vendor": "AMD",
            })),
            None,
        );
        for k in [
            "vram_allocator",
            "attention_head_split",
            "read_ahead",
            "smart_memory_pinning",
            "image_preload_mode",
        ] {
            assert!(amd.get(k).is_none(), "AMD must not be given {k}");
        }

        // No usable GPU: the labeled fallback path must stay free of them too.
        let none = auto_tune_recommend(
            Some(serde_json::json!({
                "vram_tier": "none", "ram_tier": "low", "gpu_vram_gb": 0,
                "cuda_available": false, "gpu_available": false,
            })),
            None,
        );
        for k in ["vram_allocator", "attention_head_split", "image_preload_mode"] {
            assert!(none.get(k).is_none(), "CPU-only must not be given {k}");
        }
    }

    /// Install-time seeding must fill gaps and never overwrite: an existing
    /// value belongs to the user or to WanGP.
    #[test]
    fn seed_memory_defaults_fills_only_absent_keys() {
        let mut cfg = serde_json::json!({"video_profile": 5, "clear_file_list": 5});
        let rec = serde_json::json!({
            "video_profile": 4.0,
            "image_profile": 4.0,
            "vram_allocator": "vmm_spill",
            "attention_head_split": 2,
            "smart_memory_pinning": true,
            "read_ahead": true,
            "video_preload_mode": "default",
            "image_preload_mode": "dynamic",
            "audio_preload_mode": "default",
            "perc_reserved_mem_max": 0,
        });
        let seeded = seed_memory_defaults(&mut cfg, &rec);
        // The user's P5 survives; everything absent is filled.
        assert_eq!(cfg["video_profile"], 5);
        assert_eq!(cfg["image_profile"], 4.0);
        assert_eq!(cfg["vram_allocator"], "vmm_spill");
        assert_eq!(cfg["attention_head_split"], 2);
        assert_eq!(cfg["image_preload_mode"], "dynamic");
        assert_eq!(cfg["perc_reserved_mem_max"], 0);
        assert_eq!(cfg["clear_file_list"], 5, "unrelated keys untouched");
        assert!(!seeded.contains(&"video_profile".to_string()));
        assert!(seeded.contains(&"vram_allocator".to_string()));

        // Second run is a no-op: seeding twice must not re-report or change.
        let again = seed_memory_defaults(&mut cfg, &rec);
        assert!(again.is_empty(), "idempotent: {again:?}");
    }

    #[test]
    fn seed_memory_defaults_skips_absent_and_invalid() {
        let mut cfg = serde_json::json!({});
        // AMD/CPU recommendation carries none of the CUDA-only levers.
        let amd = auto_tune_recommend(
            Some(serde_json::json!({
                "vram_tier": "low", "ram_tier": "low", "gpu_vram_gb": 16.0,
                "cuda_available": false, "gpu_available": true, "vendor": "AMD",
            })),
            None,
        );
        let seeded = seed_memory_defaults(&mut cfg, &amd);
        assert!(cfg.get("vram_allocator").is_none());
        assert!(cfg.get("attention_head_split").is_none());
        assert!(seeded.contains(&"video_profile".to_string()));

        // Fail-closed: a bad value never reaches the file even if a
        // recommendation somehow carried it.
        let mut cfg2 = serde_json::json!({});
        let bad = serde_json::json!({"vram_allocator": "turbo"});
        assert!(seed_memory_defaults(&mut cfg2, &bad).is_empty());
        assert!(cfg2.get("vram_allocator").is_none());
    }

    /// The Apply allowlist must carry every v17 key — a key missing from
    /// MEMORY_OVERRIDE_KEYS is validated and then silently dropped.
    #[test]
    fn apply_allowlist_covers_every_v17_key() {
        for key in [
            "vram_allocator",
            "attention_head_split",
            "read_ahead",
            "smart_memory_pinning",
            "video_preload_mode",
            "image_preload_mode",
            "audio_preload_mode",
            "perc_reserved_mem_max",
        ] {
            assert!(MEMORY_OVERRIDE_KEYS.contains(&key), "{key} not appliable");
        }
    }

    /// The other half of the allowlist contract: what Apply writes, Read must
    /// return. `memory_profile_read` used a hand-written list that predated the
    /// v17 keys — they saved and then displayed "saved: —" forever.
    #[test]
    fn memory_profile_read_returns_written_keys_and_invents_none() {
        let td = TestDataDir::new();
        td.seed_wgp_config(
            r#"{"video_profile":4,"vram_allocator":"vmm_spill","attention_head_split":2,"read_ahead":true,"smart_memory_pinning":true,"video_preload_mode":"default","image_preload_mode":"dynamic","audio_preload_mode":"default","perc_reserved_mem_max":0,"enable_int8_kernels":0,"unrelated":"x"}"#,
        );
        let settings = memory_profile_read()
            .get("settings")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        // Everything on disk comes back, v17 keys included.
        for (k, v) in [
            ("video_profile", serde_json::json!(4)),
            ("vram_allocator", serde_json::json!("vmm_spill")),
            ("attention_head_split", serde_json::json!(2)),
            ("read_ahead", serde_json::json!(true)),
            ("smart_memory_pinning", serde_json::json!(true)),
            ("video_preload_mode", serde_json::json!("default")),
            ("image_preload_mode", serde_json::json!("dynamic")),
            ("audio_preload_mode", serde_json::json!("default")),
            ("perc_reserved_mem_max", serde_json::json!(0)),
            // legacy int8 toggle still maps to the replacement key
            ("int8_kernels", serde_json::json!("disabled")),
        ] {
            assert_eq!(settings.get(k), Some(&v), "{k} missing from read-back");
        }
        // And nothing is invented for keys that were never written — the old
        // code painted "saved: 4" on a config with no video_profile at all.
        for k in ["vae_config", "queue_color_scheme", "kernel_precision"] {
            assert!(settings.get(k).is_none(), "{k} was never written");
        }
        assert!(
            settings.get("unrelated").is_none(),
            "unrelated keys must not leak into the panel"
        );
    }

    #[test]
    fn amd_usable_gpus_tier_normally() {
        // Healthy Radeon (cuda off, gpu on, real VRAM) must NOT take the
        // no-CUDA fallback — 9060 XT 16GB + 32GB RAM tiers like NVIDIA.
        let r = auto_tune_recommend(
            Some(serde_json::json!({
                "vram_tier": "low", "ram_tier": "low", "gpu_vram_gb": 16,
                "cuda_available": false, "gpu_available": true, "vendor": "AMD",
            })),
            None,
        );
        assert_eq!(r["video_profile"], 4.0);
        assert!(!r["_recommendation_label"]
            .as_str()
            .unwrap()
            .contains("unavailable"));
        // AMD with unknown/zero VRAM still falls back.
        let r = auto_tune_recommend(
            Some(serde_json::json!({
                "vram_tier": "none", "ram_tier": "low", "gpu_vram_gb": 0,
                "cuda_available": false, "gpu_available": false, "vendor": "AMD",
            })),
            None,
        );
        assert_eq!(r["video_profile"], 4.5);
    }
}
#[cfg(test)]
mod kernel_setting_tests {
    use super::{normalize_qwen_quant, valid_memory_override};
    #[test]
    fn qwen_quant_normalizes_per_engine() {
        // Qwen3.8 27B (id 5): four GGUF backends incl. Bonsai PTQ1.
        for good in ["gguf", "gguf_q3", "gguf_q2", "gguf_ptq1"] {
            assert_eq!(normalize_qwen_quant(Some(good), Some(5)), Some(good));
        }
        // Qwen3.8 9B Heretic (id 6): GGUF Q4 + Q8 only.
        for good in ["gguf", "gguf_q8"] {
            assert_eq!(normalize_qwen_quant(Some(good), Some(6)), Some(good));
        }
        // Wrong-engine values fall back to the engine default, never garbage.
        assert_eq!(normalize_qwen_quant(Some("quanto_int8"), Some(5)), Some("gguf"));
        assert_eq!(normalize_qwen_quant(Some("gguf_ptq1"), Some(6)), Some("gguf"));
        assert_eq!(normalize_qwen_quant(Some("gguf_q8"), Some(5)), Some("gguf"));
        assert_eq!(normalize_qwen_quant(Some("gguf_ptq1"), Some(4)), Some("quanto_int8"));
        assert_eq!(normalize_qwen_quant(Some("gguf"), Some(3)), Some("gguf"));
        // No engine / no quant preserves existing config.
        assert_eq!(normalize_qwen_quant(Some("gguf_ptq1"), None), None);
        assert_eq!(normalize_qwen_quant(None, Some(5)), None);
        assert_eq!(normalize_qwen_quant(Some(""), Some(5)), None);
    }
    #[test]
    fn int8_backends_match_upstream_choices() {
        // shared/kernels/int8_backend.py CHOICES.
        for good in ["auto", "disabled", "triton", "kitchen"] {
            assert!(
                valid_memory_override("int8_kernels", &serde_json::json!(good)),
                "{good}"
            );
        }
        for bad in ["", "enabled", "1", "pytorch", "AUTO"] {
            assert!(
                !valid_memory_override("int8_kernels", &serde_json::json!(bad)),
                "{bad}"
            );
        }
        // Legacy numeric key must not validate as the new enum.
        assert!(!valid_memory_override("int8_kernels", &serde_json::json!(1)));
    }
    #[test]
    fn kernel_precision_match_upstream_choices() {
        // shared/kernels/kernel_policy.py CHOICES ("strict"/"fast").
        assert!(valid_memory_override("kernel_precision", &serde_json::json!("fast")));
        assert!(valid_memory_override("kernel_precision", &serde_json::json!("strict")));
        assert!(!valid_memory_override("kernel_precision", &serde_json::json!("preserve")));
        assert!(!valid_memory_override("kernel_precision", &serde_json::json!("")));
    }
    #[test]
    fn queue_color_scheme_allows_only_known_names() {
        // wgp.py: "pastel" renders per-row hues, anything else the
        // theme-following grey rows. Fail-closed to the two known names.
        assert!(valid_memory_override("queue_color_scheme", &serde_json::json!("pastel")));
        assert!(valid_memory_override("queue_color_scheme", &serde_json::json!("grey")));
        for bad in ["", "rainbow", "dark", "GREY"] {
            assert!(
                !valid_memory_override("queue_color_scheme", &serde_json::json!(bad)),
                "{bad:?}"
            );
        }
        assert!(!valid_memory_override("queue_color_scheme", &serde_json::json!(1)));
    }
}

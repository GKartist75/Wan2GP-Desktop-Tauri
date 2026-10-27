//! Environment status, version scans and prerequisite probes.
use crate::base::*;
use crate::hw::{get_gpu_info_sync, kernel_profile_key};
use std::path::PathBuf;

#[tauri::command]
pub fn get_status() -> serde_json::Value {
    let env = get_active_env();
    // site-packages mtime: pip install/uninstall invalidates the cache instantly
    let sp_mtime = env
        .get("path")
        .and_then(|p| p.as_str())
        .map(|raw| {
            let base = if std::path::Path::new(raw).is_absolute() {
                PathBuf::from(raw)
            } else {
                get_repo_dir().join(raw.trim_start_matches(".\\").trim_start_matches("./"))
            };
            #[cfg(windows)]
            {
                base.join("Lib").join("site-packages")
            }
            #[cfg(not(windows))]
            {
                base.join("lib")
            }
        })
        .and_then(|d| std::fs::metadata(&d).and_then(|m| m.modified()).ok());
    if let Ok(g) = LAST_STATUS.lock() {
        if let Some((t, mt, v)) = g.as_ref() {
            if t.elapsed() < std::time::Duration::from_secs(5) && *mt == sp_mtime {
                return v.clone();
            }
        }
    }
    if env.is_null() {
        return serde_json::json!({"error":"No active environment","spike":true});
    }
    // try to read kernel wheels from setup_config.json if present
    let repo = get_repo_dir();
    let cfg_path = repo.join("setup_config.json");
    let mut wheels = serde_json::json!([]);
    let mut profile = String::new();
    if cfg_path.exists() {
        if let Ok(s) = std::fs::read_to_string(&cfg_path) {
            if let Ok(cfg) = serde_json::from_str::<serde_json::Value>(&s) {
                let gpu = get_gpu_info_sync();
                let vendor = gpu
                    .get("vendor")
                    .and_then(|v| v.as_str())
                    .unwrap_or("UNKNOWN");
                let name = gpu.get("name").and_then(|v| v.as_str()).unwrap_or("");
                profile = kernel_profile_key(vendor, name);
                // Launcher AMD sub-profiles (AMD_GFX1200/1201/...) alias to
                // upstream's unified AMD key (ROCm 10 refresh) when the exact
                // key is absent — mirrors forced_setup_profile + the JS
                // resolveKernelWheels fallback so the overview agrees with
                // what actually gets installed.
                let lookup = if cfg
                    .get("gpu_profiles")
                    .and_then(|p| p.get(&profile))
                    .is_none()
                    && profile.starts_with("AMD")
                    && cfg.get("gpu_profiles").and_then(|p| p.get("AMD")).is_some()
                {
                    "AMD".to_string()
                } else {
                    profile.clone()
                };
                if let Some(prof) = cfg.get("gpu_profiles").and_then(|p| p.get(&lookup)) {
                    if let Some(kernels) = prof.get("kernels").and_then(|k| k.as_array()) {
                        // build overview wheels with label/pipName/configured so frontend shows "want <ver>" not "want ?"
                        let mut arr = Vec::new();
                        for k in kernels {
                            if let Some(key) = k.as_str() {
                                let (label, pip) = match key {
                                    "nunchaku_cu13" | "nunchaku" => ("Nunchaku", "nunchaku"),
                                    "gguf" | "gguf_cu128" | "gguf_rocm10"
                                    | "llamacpp_gguf_cuda" => {
                                        ("GGUF (llamacpp)", "llamacpp_gguf_cuda")
                                    }
                                    "light2xv" | "lightx2v_kernel" => {
                                        ("LightX2V", "lightx2v_kernel")
                                    }
                                    "wgpkernels" | "wgpkernels_cu128" | "wgpkernels_rocm10" => {
                                        ("WGPKernels", "wgpkernels")
                                    }
                                    _ => (key, key),
                                };
                                // Configured version from setup_config.json
                                // components.kernels[key].cmd[win]. GGUF ships
                                // one build per (torch, python) and profiles
                                // list the cu130 default (`gguf`), so resolve
                                // the profile's own pair to the component it
                                // actually installs (AMD → `gguf_rocm10`,
                                // GTX_10 → `gguf_cu128`) — otherwise the
                                // overview compares against a foreign build tag
                                // and shows a permanent phantom mismatch.
                                let comp_key = if key == "gguf" {
                                    let t =
                                        prof.get("torch").and_then(|v| v.as_str()).unwrap_or("");
                                    let p =
                                        prof.get("python").and_then(|v| v.as_str()).unwrap_or("");
                                    match crate::hw::gguf_component_for(t, p) {
                                        // Non-RDNA4 AMD maps to the ROCm wheel, but
                                        // Sync skips it (gfx1200/1201 only) — claim
                                        // no configured build rather than a phantom
                                        // mismatch that can never clear.
                                        Some("gguf_rocm10")
                                            if !["AMD_GFX1200", "AMD_GFX1201"]
                                                .contains(&profile.as_str()) =>
                                        {
                                            ""
                                        }
                                        other => other.unwrap_or("gguf"),
                                    }
                                } else {
                                    key
                                };
                                let mut configured: Option<String> = None;
                                if let Some(cmd) = cfg
                                    .get("components")
                                    .and_then(|c| c.get("kernels"))
                                    .and_then(|m| m.get(comp_key))
                                    .and_then(|e| e.get("cmd"))
                                    .and_then(|c| c.get("win"))
                                    .and_then(|u| u.as_str())
                                {
                                    let cmd = crate::hw::apply_gguf_override_cfg(cmd, &cfg);
                                    // wheelDistVersion: parse "<dist>-<version>-cp..."
                                    if let Some(base) = cmd.as_str().split('/').next_back() {
                                        // NOTE: newer upstream URLs percent-encode the build tag
                                        // (`%2B` for `+`, e.g. gguf-v1.0.21 links) while importlib
                                        // reports a literal `+` — decode before comparing or every
                                        // wheel shows a phantom version mismatch.
                                        let base = base
                                            .trim_end_matches(".whl")
                                            .replace("%2B", "+")
                                            .replace("%2b", "+");
                                        if let Some(dash) = base.find('-') {
                                            let rest = &base[dash + 1..];
                                            if let Some(v_end) =
                                                rest.find("-cp").or_else(|| rest.find("-py"))
                                            {
                                                configured = Some(rest[..v_end].to_string());
                                            }
                                        }
                                    }
                                }
                                arr.push(serde_json::json!({"key": key, "label": label, "pipName": pip, "configured": configured}));
                            }
                        }
                        wheels = serde_json::Value::Array(arr);
                    }
                }
            }
        }
    }
    // wheels already built with configured, but installed will be filled after version scan
    let pending_wheels = wheels.clone();
    // real version scan via env's python (importlib.metadata) — ponytail: helper file on same drive as env
    let mut versions = serde_json::Map::new();
    if let Some(raw) = env.get("path").and_then(|p| p.as_str()) {
        let rel = raw.trim_start_matches(".\\").trim_start_matches("./");
        let base = if std::path::Path::new(raw).is_absolute() {
            PathBuf::from(raw)
        } else {
            get_repo_dir().join(rel)
        };
        let py = if cfg!(windows) {
            base.join("Scripts\\python.exe")
        } else {
            base.join("bin/python3")
        };
        let py_bin = if py.exists() { py } else { PathBuf::from(raw) };
        if py_bin.exists() {
            let helper = get_data_dir().join(".get_versions.py");
            // Triton's distribution is `triton-windows` on Windows and plain
            // `triton` everywhere else (torch pulls it in on Linux/macOS, see
            // upstream docs/INSTALLATION.md#linux). Probing only the Windows
            // name silently dropped the row — the panel read "Triton —" on a
            // host with 3.6.0 installed. Try the host's own name first, then
            // the other, so a stray cross-name install still shows up.
            let order = triton_dist_order();
            let triton_order = format!("['{}','{}']", order[0], order[1]);
            let code = r#"import sys, importlib.metadata
try:
    aliases={'spas_sage_attn':'spas-sage-attn','huggingface_hub':'huggingface-hub'}
    triton_order=@TRITON_ORDER@
    def ver(*names):
        for n in names:
            try:
                return importlib.metadata.version(n)
            except Exception:
                pass
        raise LookupError(names[0])
    pkgs=['python','torch','triton','wgpkernels','sageattention','spas_sage_attn','flash_attn','nunchaku','llamacpp_gguf_cuda','lightx2v_kernel','diffusers','transformers','gradio','accelerate','onnxruntime','onnxruntime-gpu','xformers','mmgp','moviepy','opencv-python','insightface','peft','timm','vector_quantize_pytorch','torchcodec','torchaudio','huggingface_hub','bitsandbytes','numpy','sentencepiece','open_clip_torch','imageio','einops','librosa','soundfile','tokenizers','av','claude-agent-sdk','xatlas','fast-simplification','scikit-image','resvg-py','chumpy','smplfitter','comfy-kitchen']
    r=[]
    for p in pkgs:
        try:
            if p=='python': r.append(f'python={sys.version.split()[0]}')
            elif p=='triton': r.append(f'triton={ver(*triton_order)}')
            elif p in aliases: r.append(f'{p}={importlib.metadata.version(aliases[p])}')
            else: r.append(f'{p}={importlib.metadata.version(p)}')
        except: pass
    print('||'.join(r))
except Exception as e:
    print(f'error:{e}')
"#;
            let code = code.replace("@TRITON_ORDER@", &triton_order);
            let _ = std::fs::write(&helper, code);
            if let Ok(out) = silent_command(&py_bin)
                .arg(&helper)
                .current_dir(&repo)
                .output()
            {
                if out.status.success() {
                    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    for part in s.split("||") {
                        if let Some((k, v)) = part.split_once('=') {
                            versions
                                .insert(k.to_string(), serde_json::Value::String(v.to_string()));
                        }
                    }
                    // onnxruntime probe mapping (issue #15 follow-up): the AMD
                    // install log puts down `onnxruntime-gpu`, which must satisfy
                    // the Manage `onnxruntime` row — show its version there. Never
                    // swaps the installed package (runtime effect unverified).
                    apply_onnxruntime_alias(&mut versions);
                }
            }
        }
    }
    apply_vendored_mmgp(&mut versions, &repo);
    // fill installed into wheels now that versions are known
    let mut final_wheels = Vec::new();
    if let Some(arr) = pending_wheels.as_array() {
        for w in arr {
            let mut obj = w.clone();
            if let Some(pip) = w.get("pipName").and_then(|v| v.as_str()) {
                if let Some(ver) = versions.get(pip).and_then(|v| v.as_str()) {
                    obj["installed"] = serde_json::json!(ver);
                    let cfg = w.get("configured").and_then(|v| v.as_str()).unwrap_or("");
                    // configured is like "1.2.1+cu13.0torch2.10", installed is same or with "+" - compare prefix before "+"
                    let want = cfg.split('+').next().unwrap_or(cfg);
                    let have = ver.split('+').next().unwrap_or(ver);
                    obj["state"] = serde_json::json!(if have == want { "ok" } else { "mismatch" });
                } else {
                    obj["state"] = serde_json::json!("missing");
                }
            }
            final_wheels.push(obj);
        }
    } else if let Some(arr) = wheels.as_array() {
        final_wheels.clone_from(arr);
    }
    let out_wheels = if final_wheels.is_empty() {
        wheels
    } else {
        serde_json::Value::Array(final_wheels)
    };
    // profile object for frontend specSparge (was only kernelProfile string → showed —)
    let mut profile_obj = cfg_path
        .exists()
        .then(|| std::fs::read_to_string(&cfg_path).ok())
        .flatten()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|c| {
            // AMD sub-profiles (AMD_GFX1200/…) alias to upstream's unified
            // `AMD` key — without this every Sparge/Flash/Triton/Sage tag reads
            // "—" on an AMD box (same aliasing as the wheels list above).
            let key = if profile.starts_with("AMD")
                && c.get("gpu_profiles")
                    .and_then(|p| p.get(&profile))
                    .is_none()
            {
                "AMD"
            } else {
                profile.as_str()
            };
            c.get("gpu_profiles").and_then(|p| p.get(key)).cloned()
        })
        .unwrap_or(serde_json::json!({"sparge": null}));
    // Panels print the *expected* version beside the installed one; map the raw
    // upstream tags (v010_cu13, v210 …) to plain versions so no row ever reads
    // "v010_cu13". `status.profile` is only consumed for the Sparge Attn row.
    if let Some(o) = profile_obj.as_object_mut() {
        for key in ["sparge", "flash", "triton", "sage"] {
            if let Some(tag) = o.get(key).and_then(|v| v.as_str()) {
                let short = crate::hw::comp_short(tag);
                o.insert(key.to_string(), serde_json::json!(short));
            }
        }
    }
    let out = serde_json::json!({"env": env, "versions": serde_json::Value::Object(versions), "kernelWheels": out_wheels, "kernelProfile": profile, "profile": profile_obj, "spike": false});
    // cache for 5s (mtime-keyed: pip changes invalidate instantly)
    if let Ok(mut g) = LAST_STATUS.lock() {
        *g = Some((std::time::Instant::now(), sp_mtime, out.clone()));
    }
    out
}
// ── Phase 1: paths / config / hardware / install checks ──
/// Map an installed `onnxruntime-gpu` onto the `onnxruntime` version row
/// (issue #15 follow-up): the probe queries both dist names; when only
/// mmgp stopped shipping as a pip dist in upstream v17 (09a7c6c8): it is the
/// in-repo `mmgp/` folder now, and requirements.txt dropped `mmgp==3.8.2`.
/// Overwrite whatever the pip probe reported with the vendored version, so
/// the dashboard shows the code that actually runs — never a stale 3.8.2 left
/// in site-packages (wgp.py shadows it, the probe would not) — and drop the
/// key entirely on a pre-v17 checkout with no `mmgp/` folder.
pub(crate) fn apply_vendored_mmgp(
    versions: &mut serde_json::Map<String, serde_json::Value>,
    repo: &std::path::Path,
) {
    match vendored_mmgp_label(repo) {
        Some(label) => {
            versions.insert("mmgp".into(), serde_json::Value::String(label));
        }
        None => {
            versions.remove("mmgp");
        }
    }
}

/// `4.4.1 (alloc 3f2a91)` for the in-repo `mmgp/` folder, `None` on a pre-v17
/// checkout that has no vendored mmgp. The allocator id is folded in because
/// upstream rebuilds the prebuilt allocators without bumping the version, so
/// the version alone cannot identify the binary that produced a crash.
pub(crate) fn vendored_mmgp_label(repo: &std::path::Path) -> Option<String> {
    let toml =
        std::fs::read_to_string(repo.join("mmgp").join("pyproject.toml")).unwrap_or_default();
    let v = pyproject_version(&toml)?;
    // v17.01 rebuilt the allocator (mmgp/allocator/vmm_alloc.cpp +59/-11
    // and a new DLL) WITHOUT bumping pyproject.toml, so "4.0.0" alone
    // cannot tell you which allocator you are running. The DLL is ~94 KB,
    // so a content hash of the binaries costs less than any probe — append it so two
    // v17.17 (6479db3) adds a SECOND prebuilt library for the RAM
    // allocator and rebuilds the VRAM one under the same version, so
    // allocator_build_id folds both in.
    // builds of the same version stay distinguishable in bug reports.
    Some(
        match allocator_build_id(&repo.join("mmgp").join("allocator")) {
            Some(id) => format!("{v} (alloc {id})"),
            None => v,
        },
    )
}

/// Short build id of the vendored prebuilt allocators: FNV-1a over the
/// platform's binaries (the VRAM one, and the RAM allocator v17.17 added
/// beside it), first 6 hex. Size is folded in so a truncated or absent
/// file is obvious rather than looking like a valid build. None when the
/// checkout predates the vendored allocator (or is not Windows/Linux).
/// Pure + unit-tested.
pub(crate) fn allocator_build_id(dir: &std::path::Path) -> Option<String> {
    let (vram, ram) = if cfg!(windows) {
        ("vmm_alloc_win_amd64.dll", "ram_alloc_win_amd64.dll")
    } else if cfg!(target_os = "macos") {
        return None;
    } else {
        ("vmm_alloc_linux_x86_64.so", "ram_alloc_linux_x86_64.so")
    };
    let vram_bytes = std::fs::read(dir.join(vram)).ok()?;
    if vram_bytes.is_empty() {
        return None;
    }
    // The RAM library is absent on a pre-v17.17 checkout: hashing what exists
    // then reproduces the single-binary id those trees always showed.
    let ram_bytes = std::fs::read(dir.join(ram)).unwrap_or_default();
    let mut h: u64 = 0xcbf29ce484222325;
    let mut size = 0usize;
    for bytes in [&vram_bytes, &ram_bytes] {
        size += bytes.len();
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    Some(format!("{:x}/{}k", h & 0xffffff, size / 1024))
}

#[cfg(test)]
mod allocator_build_tests {
    use super::allocator_build_id;
    /// The allocator fingerprint must distinguish two builds that share a
    /// version — v17.01 shipped a new DLL without bumping pyproject.toml — and
    /// must never invent one for a missing or empty binary.
    #[test]
    fn allocator_build_id_separates_builds_and_refuses_nothing() {
        if cfg!(target_os = "macos") {
            return;
        }
        let dir = std::env::temp_dir().join(format!("wan2gp-alloc-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let name = if cfg!(windows) {
            "vmm_alloc_win_amd64.dll"
        } else {
            "vmm_alloc_linux_x86_64.so"
        };
        // No binary at all → None (pre-allocator checkout).
        assert_eq!(allocator_build_id(&dir), None);
        // Empty file is not a build.
        std::fs::write(dir.join(name), b"").unwrap();
        assert_eq!(allocator_build_id(&dir), None);
        // Same content → same id; different content → different id.
        std::fs::write(dir.join(name), b"build-one").unwrap();
        let a = allocator_build_id(&dir).unwrap();
        assert_eq!(
            a,
            allocator_build_id(&dir).unwrap(),
            "fingerprint is stable"
        );
        std::fs::write(dir.join(name), b"build-two!").unwrap();
        let b = allocator_build_id(&dir).unwrap();
        assert_ne!(a, b, "two builds sharing a version must not collide");
        assert!(a.ends_with("k"), "size folded into the id: {a}");
        // v17.17: a second prebuilt library sits beside the VRAM one, so the
        // same VRAM build must not fingerprint the same with and without it.
        std::fs::write(
            dir.join(name.replace("vmm_alloc", "ram_alloc")),
            b"ram-build",
        )
        .unwrap();
        let with_ram = allocator_build_id(&dir).unwrap();
        assert_ne!(
            b, with_ram,
            "the RAM allocator binary must be part of the id"
        );
        assert!(
            with_ram.ends_with("k"),
            "both sizes folded into the id: {with_ram}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// `version = "4.0.0"` from a pyproject.toml body. First `version =` line
/// wins; `requires-python` and dependency entries never match. Pure +
/// unit-tested.
pub(crate) fn pyproject_version(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("version"))
        .find_map(|l| l.split_once('='))
        .map(|(_, v)| v.trim().trim_matches(|c| c == '"' || c == '\'').to_string())
        .filter(|v| !v.is_empty())
}

/// Dist names Triton can live under, host's own name first: Windows ships the
/// `triton-windows` fork, Linux and macOS get plain `triton` pulled in by
/// torch (upstream docs/INSTALLATION.md#linux). The panel probes them in this
/// order and keeps the second as a fallback, so the row never reads "—" just
/// because the other spelling is what got installed.
pub(crate) fn triton_dist_order() -> [&'static str; 2] {
    if cfg!(windows) {
        ["triton-windows", "triton"]
    } else {
        ["triton", "triton-windows"]
    }
}

/// the -gpu dist is installed, its version satisfies the row. Never
/// overwrites a real `onnxruntime` version, and never touches the
/// installed package. Pure + unit-tested.
pub(crate) fn apply_onnxruntime_alias(versions: &mut serde_json::Map<String, serde_json::Value>) {
    if versions.get("onnxruntime").is_none() {
        if let Some(v) = versions.get("onnxruntime-gpu").cloned() {
            versions.insert("onnxruntime".into(), v);
        }
    }
}

/// Does the registered env still exist on disk (interpreter present)?
/// Users sometimes delete the env folder by hand — envs.json then points
/// at nothing and the dashboard shows a phantom healthy env with a working
/// Launch button (0.5.2 report: env_uv deleted, launcher said all OK).
/// Empty path = system/"none" env, no folder to check → always alive.
/// Pure + unit-tested.
pub(crate) fn env_entry_alive(repo: &std::path::Path, entry: &serde_json::Value) -> bool {
    let path = entry.get("path").and_then(|p| p.as_str()).unwrap_or("");
    if path.is_empty() {
        return true;
    }
    let base = if std::path::Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        repo.join(path.trim_start_matches(".\\").trim_start_matches("./"))
    };
    if !base.is_dir() {
        return false;
    }
    // Windows: Scripts\python.exe (uv/venv) or root python.exe (conda).
    // Elsewhere: bin/python (uv) or bin/python3 (venv) or root (conda).
    #[cfg(windows)]
    {
        base.join("Scripts\\python.exe").exists() || base.join("python.exe").exists()
    }
    #[cfg(not(windows))]
    {
        base.join("bin/python").exists()
            || base.join("bin/python3").exists()
            || base.join("python").exists()
    }
}

/// Resolve the interpreter for a registered env dir (relative or absolute).
/// Layouts differ per env type — uv/venv keep it under Scripts\ (Windows)
/// or bin/ (unix), conda keeps python.exe at the env ROOT. Returns the
/// first candidate that exists, or None (callers fall back to their legacy
/// default so error messages stay identical). Pure + unit-tested.
/// (0.5.2 report: conda env healthy but launch blocked — resolution only
/// knew Scripts\, so it tried to execute the env folder itself.)
pub(crate) fn resolve_env_python(repo: &std::path::Path, raw: &str) -> Option<PathBuf> {
    if raw.is_empty() {
        return None;
    }
    let base = if std::path::Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        repo.join(raw.trim_start_matches(".\\").trim_start_matches("./"))
    };
    #[cfg(windows)]
    let cands = [base.join("Scripts\\python.exe"), base.join("python.exe")];
    #[cfg(not(windows))]
    let cands = [
        base.join("bin/python"),
        base.join("bin/python3"),
        base.join("python"),
    ];
    cands.into_iter().find(|p| p.is_file())
}

/// Interpreter of the env registered as active in envs.json, resolved
/// type-agnostically (uv/venv: `Scripts\`/`bin/`; conda: the env ROOT).
/// None when no env is active or the folder/interpreter is gone. Every
/// package/wheel path must use this instead of re-deriving `Scripts\
/// python.exe`: that layout silently broke every conda env.
pub(crate) fn active_env_python() -> Option<PathBuf> {
    let env = get_active_env();
    let raw = env.get("path")?.as_str()?;
    if raw.is_empty() {
        return None;
    }
    resolve_env_python(&get_repo_dir(), raw)
}

pub(crate) fn get_active_env() -> serde_json::Value {
    let f = get_envs_file();
    if !f.exists() {
        return serde_json::Value::Null;
    }
    let Ok(s) = std::fs::read_to_string(&f) else {
        return serde_json::Value::Null;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else {
        return serde_json::Value::Null;
    };
    let active = v.get("active").and_then(|x| x.as_str()).unwrap_or("");
    if active.is_empty() {
        return serde_json::Value::Null;
    }
    if let Some(env) = v.get("envs").and_then(|e| e.get(active)) {
        // A hand-deleted env folder must read as "no env" (installer
        // prompt), never as a healthy active env with a live Launch button.
        if !env_entry_alive(&get_repo_dir(), env) {
            return serde_json::Value::Null;
        }
        // env entries carry type/path only — inject the map key as `name`
        // (dashboard, unlink/restore and logs all key off status.env.name).
        let mut e = env.clone();
        if let Some(m) = e.as_object_mut() {
            m.insert("name".into(), serde_json::json!(active));
        }
        e
    } else {
        serde_json::Value::Null
    }
}

#[tauri::command]
pub fn check_installed() -> serde_json::Value {
    let repo = get_repo_dir();
    let has_repo = repo.join("wgp.py").exists();
    let has_env = !get_active_env().is_null();
    // Previous install location now missing (external drive disconnected or
    // drive letter changed)? Report it so first-run can say so explicitly.
    let missing_prev = std::fs::read_to_string(home_dir().join(".wan2gp-tauri-installed"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|p| !p.is_empty())
        .filter(|p| !std::path::Path::new(p).join("wgp.py").exists())
        .filter(|_| !has_repo);
    serde_json::json!({"repo": has_repo, "env": has_env, "missingPrevious": missing_prev})
}

#[tauri::command]
pub fn check_command(cmd: String) -> serde_json::Value {
    // Same verdict as the installer card (install::probe_tool on Windows):
    // the gate used to call tool_found while the card used probe_tool, so a
    // python.org install without a PATH alias looped on the same card.
    let found = crate::install::tool_ready(&cmd);
    serde_json::json!({"cmd": cmd, "found": found})
}

#[cfg(test)]
mod triton_dist_tests {
    use super::triton_dist_order;

    /// Windows must ask for `triton-windows` first (plain `triton` there is a
    /// shadowing mistake) and Unix for `triton`; both names are always probed
    /// so a cross-installed build still shows up instead of a blank row.
    #[test]
    fn host_name_first_with_the_other_as_fallback() {
        let [first, second] = triton_dist_order();
        let (want_first, want_second) = if cfg!(windows) {
            ("triton-windows", "triton")
        } else {
            ("triton", "triton-windows")
        };
        assert_eq!(first, want_first);
        assert_eq!(second, want_second);
    }
}

#[cfg(test)]
mod env_alive_tests {
    use super::{env_entry_alive, resolve_env_python};
    fn tmp_repo(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("wgp-env-alive-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    #[cfg(windows)]
    fn plant_python(base: &std::path::Path) {
        std::fs::create_dir_all(base.join("Scripts")).unwrap();
        std::fs::write(base.join("Scripts\\python.exe"), b"fake").unwrap();
    }
    #[cfg(not(windows))]
    fn plant_python(base: &std::path::Path) {
        std::fs::create_dir_all(base.join("bin")).unwrap();
        std::fs::write(base.join("bin/python"), b"fake").unwrap();
    }
    #[test]
    fn deleted_env_reads_dead() {
        let repo = tmp_repo("dead");
        // 0.5.2 report: env_uv deleted by hand, registry still lists it.
        let entry = serde_json::json!({"type": "uv", "path": "./env_uv"});
        assert!(!env_entry_alive(&repo, &entry));
        // Empty dir without interpreter is also dead.
        std::fs::create_dir_all(repo.join("env_uv")).unwrap();
        assert!(!env_entry_alive(&repo, &entry));
        // Interpreter present → alive (relative + absolute forms).
        plant_python(&repo.join("env_uv"));
        assert!(env_entry_alive(&repo, &entry));
        let abs = serde_json::json!({"type": "uv", "path": repo.join("env_uv").to_string_lossy()});
        assert!(env_entry_alive(&repo, &abs));
        // System env (no folder) stays alive.
        assert!(env_entry_alive(
            &repo,
            &serde_json::json!({"type": "none", "path": ""})
        ));
        let _ = std::fs::remove_dir_all(&repo);
    }
    #[test]
    fn resolver_covers_all_env_layouts() {
        let repo = tmp_repo("resolve");
        // uv/venv layout: Scripts\python.exe (Windows) / bin/python (unix).
        std::fs::create_dir_all(repo.join("env_uv")).unwrap();
        plant_python(&repo.join("env_uv"));
        assert!(resolve_env_python(&repo, "./env_uv").is_some());
        // conda layout: python.exe at the env root, no Scripts dir.
        let conda = repo.join("env_conda");
        std::fs::create_dir_all(&conda).unwrap();
        #[cfg(windows)]
        {
            std::fs::write(conda.join("python.exe"), b"fake").unwrap();
        }
        #[cfg(not(windows))]
        {
            std::fs::write(conda.join("python"), b"fake").unwrap();
        }
        let found = resolve_env_python(&repo, ".\\env_conda").expect("conda root interpreter");
        #[cfg(windows)]
        {
            assert_eq!(found, conda.join("python.exe"));
        }
        #[cfg(not(windows))]
        {
            assert_eq!(found, conda.join("python"));
        }
        // Scripts wins when both exist (uv/venv never have a root python).
        plant_python(&conda);
        let both = resolve_env_python(&repo, "./env_conda").unwrap();
        #[cfg(windows)]
        {
            assert_eq!(both, conda.join("Scripts\\python.exe"));
        }
        #[cfg(not(windows))]
        {
            assert_eq!(both, conda.join("bin/python"));
        }
        // Missing dir, empty dir, empty path → None (callers keep legacy fallback).
        assert!(resolve_env_python(&repo, "./env_gone").is_none());
        std::fs::create_dir_all(repo.join("env_empty")).unwrap();
        assert!(resolve_env_python(&repo, "./env_empty").is_none());
        assert!(resolve_env_python(&repo, "").is_none());
        // Absolute raw path form.
        assert!(resolve_env_python(&repo, conda.to_string_lossy().as_ref()).is_some());
        let _ = std::fs::remove_dir_all(&repo);
    }
}
#[cfg(test)]
mod onnx_alias_tests {
    use super::apply_onnxruntime_alias;
    fn map(pairs: &[(&str, &str)]) -> serde_json::Map<String, serde_json::Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), serde_json::Value::String(v.to_string())))
            .collect()
    }
    #[test]
    fn onnxruntime_gpu_satisfies_row() {
        // Reporter shape: install log put down onnxruntime-gpu, the
        // Manage `onnxruntime` row showed —. Now it shows the gpu version.
        let mut v = map(&[("onnxruntime-gpu", "1.22.0")]);
        apply_onnxruntime_alias(&mut v);
        assert_eq!(
            v.get("onnxruntime").and_then(|x| x.as_str()),
            Some("1.22.0")
        );
        // The -gpu key itself is kept (probe transparency).
        assert_eq!(
            v.get("onnxruntime-gpu").and_then(|x| x.as_str()),
            Some("1.22.0")
        );
        // A real onnxruntime version is never overwritten.
        let mut both = map(&[("onnxruntime", "1.20.0"), ("onnxruntime-gpu", "1.22.0")]);
        apply_onnxruntime_alias(&mut both);
        assert_eq!(
            both.get("onnxruntime").and_then(|x| x.as_str()),
            Some("1.20.0")
        );
        // Neither installed → row stays missing.
        let mut none = map(&[("torch", "2.12.0")]);
        apply_onnxruntime_alias(&mut none);
        assert!(none.get("onnxruntime").is_none());
    }
}

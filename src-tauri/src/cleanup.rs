//! Disk cleanup — sizes and deletion for folders the launcher and Wan2GP
//! rebuild on demand.
//!
//! The renderer never sends a path: it sends keys, and the paths are recomputed
//! here, canonicalised, then rejected if they are a drive root or contain the
//! repo, data dir, home or temp root. Nothing outside this list can be deleted
//! through the command, so a stale junction in `%TEMP%\gradio` cannot redirect a
//! delete into the model store.
//!
//! Wan2GP must be stopped: a Gradio upload or a Triton JIT artifact is in use
//! while it runs, and a half-deleted cache turns into a launch failure.

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize)]
pub struct CleanupTarget {
    pub key: &'static str,
    pub label: &'static str,
    pub path: String,
    pub exists: bool,
    pub bytes: u64,
    pub files: u64,
    pub risky: bool,
    pub warning: &'static str,
}

struct Spec {
    key: &'static str,
    label: &'static str,
    path: PathBuf,
    risky: bool,
    warning: &'static str,
}

fn pip_cache_dir() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::base::home_dir().join("AppData").join("Local"));
    let p = local.join("pip").join("cache");
    p.exists().then_some(p)
}

/// `$HF_HOME` if set, else the hub default. Only `hub/` is listed: `token` and
/// `xet/` sit beside it and deleting the root would log the user out.
fn hf_hub_dir() -> PathBuf {
    std::env::var_os("HF_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::base::home_dir().join(".cache").join("huggingface"))
        .join("hub")
}

fn specs() -> Vec<Spec> {
    let repo = crate::base::get_repo_dir();
    let data = crate::base::get_data_dir();
    let gradio = std::env::var_os("GRADIO_TEMP_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("gradio"));

    let mut v = vec![
        Spec {
            key: "gradio",
            label: "Gradio temp",
            path: gradio,
            risky: false,
            warning: "Uploads and previews Gradio copies while Wan2GP runs. A page still showing one stops until it is reloaded, and a queued task needs its upload — keep the age above your longest queue.",
        },
        Spec {
            key: "uv",
            label: "uv wheel cache",
            path: repo.join(".uv-cache"),
            risky: false,
            warning: "Wheels for your Python environment. The next update re-downloads them, so the first update after cleaning is slower.",
        },
        Spec {
            key: "wheels",
            label: "Launcher wheel cache",
            path: crate::wheels::cache_dir(),
            risky: false,
            warning: "Pinned kernel wheels. The launcher re-downloads and SHA-256 verifies them when they are missing.",
        },
        Spec {
            key: "triton",
            label: "Triton JIT cache",
            path: data.join("triton-cache"),
            risky: false,
            warning: "Compiled Triton kernels. They rebuild on the next run — the first generation is slower.",
        },
    ];
    if let Some(p) = pip_cache_dir() {
        v.push(Spec {
            key: "pip",
            label: "pip cache",
            path: p,
            risky: false,
            warning: "pip's download cache, shared by every Python project on this machine. Installed packages stay in your environment; anything reinstalled is fetched again.",
        });
    }
    v.push(Spec {
        key: "hf",
        label: "HuggingFace hub cache",
        path: hf_hub_dir(),
        risky: true,
        warning: "Model weights fetched through huggingface_hub live ONLY here (Deepy LLM engines, Kandinsky, some VAE/ONNX files). Deleting them means downloading them again — tens of GB. Models in your Wan2GP model folder (ckpts) are NOT affected, and your HF login token is kept.",
    });
    v
}

fn walk(p: &Path, bytes: &mut u64, files: &mut u64, depth: u32) {
    if depth > 64 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(p) else { return };
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            walk(&e.path(), bytes, files, depth + 1);
        } else if ft.is_file() {
            if let Ok(md) = e.metadata() {
                *bytes += md.len();
                *files += 1;
            }
        }
    }
}

fn scan_one(s: &Spec, with_sizes: bool) -> CleanupTarget {
    let exists = s.path.exists();
    let (mut bytes, mut files) = (0u64, 0u64);
    if exists && with_sizes {
        walk(&s.path, &mut bytes, &mut files, 0);
    }
    CleanupTarget {
        key: s.key,
        label: s.label,
        path: s.path.to_string_lossy().to_string(),
        exists,
        bytes,
        files,
        risky: s.risky,
        warning: s.warning,
    }
}

fn scan_blocking(with_sizes: bool) -> Vec<CleanupTarget> {
    specs().iter().map(|s| scan_one(s, with_sizes)).collect()
}

/// Reject a delete target that is a root, or an ancestor of anything protected.
/// `canon` must already be canonicalised.
pub(crate) fn guard(canon: &Path, protected: &[PathBuf]) -> Result<(), String> {
    if canon.parent().is_none() || canon.components().count() <= 1 {
        return Err(format!(
            "refusing to delete {} — that is a drive/root folder",
            canon.display()
        ));
    }
    for p in protected {
        let Ok(pc) = p.canonicalize() else { continue };
        if pc == canon {
            return Err(format!(
                "refusing to delete {} — it is a protected folder",
                canon.display()
            ));
        }
        if pc.starts_with(canon) {
            return Err(format!(
                "refusing to delete {} — it contains {}",
                canon.display(),
                pc.display()
            ));
        }
    }
    Ok(())
}

fn protected_paths() -> Vec<PathBuf> {
    vec![
        crate::base::get_repo_dir(),
        crate::base::get_data_dir(),
        crate::base::home_dir(),
        std::env::temp_dir(),
    ]
}

/// Empty a folder but keep it: Gradio, uv and Triton all recreate their layout,
/// and an existing folder keeps the "not installed" probes quiet.
fn clear_children(p: &Path) -> (u64, u64, Vec<String>) {
    let (mut freed, mut removed, mut errors) = (0u64, 0u64, Vec::new());
    let Ok(rd) = std::fs::read_dir(p) else {
        errors.push(format!("cannot read {}", p.display()));
        return (freed, removed, errors);
    };
    for e in rd.flatten() {
        let path = e.path();
        let Ok(ft) = e.file_type() else { continue };
        let (mut bytes, mut files) = (0u64, 0u64);
        if ft.is_dir() {
            walk(&path, &mut bytes, &mut files, 0);
        } else if ft.is_file() {
            if let Ok(md) = e.metadata() {
                bytes = md.len();
                files = 1;
            }
        }
        let res = if ft.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        match res {
            Ok(()) => {
                freed += bytes;
                removed += files;
            }
            // In use / permission denied: report and keep going so one locked
            // file does not abort the other folders.
            Err(err) => errors.push(format!("{} — {err}", path.display())),
        }
    }
    (freed, removed, errors)
}

#[tauri::command]
pub async fn cleanup_scan(with_sizes: bool) -> Result<serde_json::Value, String> {
    let targets = tauri::async_runtime::spawn_blocking(move || scan_blocking(with_sizes))
        .await
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "ok": true, "targets": targets }))
}

#[tauri::command]
pub async fn cleanup_clean(keys: Vec<String>) -> Result<serde_json::Value, String> {
    if keys.is_empty() {
        return Err("no folders selected".into());
    }
    if crate::launch::wangp_running() {
        return Err("Wan2GP is running — stop it before deleting caches it is using.".into());
    }
    // Shares the "one destructive op at a time" lock with installs and updates:
    // deleting the wheel cache mid-install would break the running uv call.
    crate::base::mutating_try("disk cleanup")?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let specs = specs();
        let protected = protected_paths();
        let mut freed = 0u64;
        let mut removed = 0u64;
        let mut errors: Vec<String> = Vec::new();
        let mut cleared: Vec<&'static str> = Vec::new();
        for key in &keys {
            let Some(s) = specs.iter().find(|s| s.key == key.as_str()) else {
                errors.push(format!("unknown folder '{key}' — refresh the list"));
                continue;
            };
            if !s.path.exists() {
                continue;
            }
            match s.path.canonicalize().map_err(|e| e.to_string()) {
                Err(err) => errors.push(format!("{} — {err}", s.path.display())),
                Ok(canon) => {
                    if let Err(err) = guard(&canon, &protected) {
                        errors.push(err);
                        continue;
                    }
                    let (f, r, e) = clear_children(&s.path);
                    freed += f;
                    removed += r;
                    errors.extend(e);
                    cleared.push(s.key);
                    crate::base::push_log(
                        &format!(
                            "[*] Disk cleanup: emptied {} ({})",
                            s.label,
                            s.path.display()
                        ),
                        "launcher",
                    );
                }
            }
        }
        (freed, removed, errors, cleared)
    })
    .await;
    crate::base::mutating_done();
    let (freed, removed, errors, cleared) = result.map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "ok": errors.is_empty(),
        "freedBytes": freed,
        "removedFiles": removed,
        "cleared": cleared,
        "errors": errors,
        "targets": scan_blocking(false),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> PathBuf {
        std::env::temp_dir()
    }

    #[test]
    fn guard_rejects_a_protected_folder_and_its_ancestors() {
        let root = temp_root().canonicalize().unwrap();
        let leaf = root.join(format!("omp-cleanup-guard-{}", std::process::id()));
        std::fs::create_dir_all(&leaf).unwrap();
        let leaf_c = leaf.canonicalize().unwrap();
        // candidate == protected folder
        assert!(guard(&root, std::slice::from_ref(&root)).is_err());
        // candidate contains the protected folder (deleting it would take the
        // temp root with it)
        assert!(guard(&root, std::slice::from_ref(&leaf_c)).is_err());
        // a real leaf with no protected folder inside is allowed
        assert!(guard(&leaf_c, std::slice::from_ref(&root)).is_ok());
        let _ = std::fs::remove_dir(&leaf);
    }

    #[test]
    fn guard_rejects_a_drive_root() {
        let root = Path::new("C:\\");
        let canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        if canon.parent().is_none() {
            assert!(guard(&canon, &[]).is_err());
        }
    }

    #[test]
    fn clear_children_empties_but_keeps_the_folder() {
        let dir = temp_root().join(format!("omp-cleanup-clear-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.bin"), b"0123456789").unwrap();
        std::fs::write(dir.join("sub").join("b.bin"), b"abcde").unwrap();

        let (freed, removed, errors) = clear_children(&dir);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(freed, 15);
        assert_eq!(removed, 2);
        assert!(dir.is_dir(), "the target folder itself must survive");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

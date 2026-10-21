//! Verified kernel-wheel fetches — the checksum gate for *Update GPU Wheels*.
//!
//! Every kernel wheel the launcher installs comes from a GitHub release asset
//! (`deepbeepmeep/kernels`, `deepbeepmeep/WGPKernels`, `nunchaku-ai/nunchaku`,
//! `woct0rdho/SageAttention`, …). Those tags are **not immutable** the way PyPI
//! files are: an asset can be replaced under the same tag, and a stale/renamed
//! build can be served by a mirror. pip installed them over TLS alone, which
//! catches nothing.
//!
//! So before pip runs, the launcher downloads the wheel itself, hashes it, and
//! refuses to install on a mismatch. Two expectation sources, in order:
//!
//! 1. [`PINNED_WHEELS`] — the exact assets the launcher ships today, pinned by
//!    filename. This is trust-on-first-use: if upstream re-uploads different
//!    bytes under the same version, the install fails loudly instead of
//!    silently swapping the wheel under the user.
//! 2. GitHub's own published `sha256` digest for the asset (the API's `digest`
//!    field). This covers versions the launcher has not pinned yet — an
//!    upstream version bump verifies without a launcher release. It shares a
//!    trust root with the download, so it mainly catches truncation, a
//!    wrong-asset swap, and a tampered mirror.
//!
//! If neither is available the wheel is installed anyway (matching the Git and
//! Python installers' "no pinned checksum — trusting TLS" precedent) but the
//! computed hash is logged so it can be pinned later.
//!
//! Every outcome prints the evidence: the sha256 the launcher computed and
//! whose digest it matched (the publisher's, named from the URL) — a cache hit
//! included, which re-hashes the file it is about to install. "Verified" is
//! therefore checkable in the console instead of something to take on faith.
//!
//! Verified wheels are kept in `<data dir>\wheel-cache` — next to the Triton
//! JIT cache, outside the Wan2GP checkout — so a re-sync re-hashes a local file
//! instead of re-downloading 150 MB.

use sha2::Digest;
use std::path::{Path, PathBuf};

/// Folder under the launcher data dir holding verified wheels.
pub(crate) const WHEEL_CACHE_SUBDIR: &str = "wheel-cache";

/// `<data dir>\wheel-cache` — siblings the `triton-cache` pin in `launch.rs`.
pub(crate) fn wheel_cache_dir_for(data_dir: &Path) -> PathBuf {
    data_dir.join(WHEEL_CACHE_SUBDIR)
}

/// The launcher's own verified-wheel cache.
pub(crate) fn cache_dir() -> PathBuf {
    wheel_cache_dir_for(&crate::base::get_data_dir())
}

/// SHA-256 of the wheel assets the launcher installs today, keyed by the asset
/// filename as decoded from the URL. Hashes are GitHub's published digests for
/// the exact assets the pinned versions resolve to — see the module docs for
/// why they are pinned here rather than only fetched.
pub(crate) const PINNED_WHEELS: &[(&str, &str)] = &[
    // GGUF llama.cpp kernels (upstream gguf-v1.0.28; the HIP one is RDNA4 opt-in).
    (
        "llamacpp_gguf_cuda-1.0.28+torch210cu130py311-cp311-cp311-win_amd64.whl",
        "551eb355c144e653f4416c55de61ac50e7380d5c48336a774f271404832d6148",
    ),
    (
        "llamacpp_gguf_cuda-1.0.28+torch271cu128py310-cp310-cp310-win_amd64.whl",
        "0c74f83820d461035690cbfae8849e1568866b906304f3857906aecab0b09989",
    ),
    (
        "llamacpp_gguf_cuda-1.0.28+torch213rocm100py312-cp312-cp312-win_amd64.whl",
        "acd46b919c89e16ec99205e93b6ed3991a54cbce1d89819ef0562e5d8fb9cc84",
    ),
    // WGPKernels 1.3.0 (NVIDIA cu130/cu128, AMD RDNA4 rocm100).
    (
        "wgpkernels-1.3.0+cu130-py3-none-win_amd64.whl",
        "dd384bb41a3ee4c8b3c9bb530df43c1ee68c9a74a5b39cc60f0fe4462d41a86a",
    ),
    (
        "wgpkernels-1.3.0+cu128-py3-none-win_amd64.whl",
        "8ddaea79c039a3d1cfeb2c8852d189a926a6ae2f366f9b274bea76ab82661f8a",
    ),
    (
        "wgpkernels-1.3.0+rocm100-py3-none-win_amd64.whl",
        "b1f8b54e1d028c250d24a0211e1dca3c938f7dd07246c98d4ae8e17c6e17ccde",
    ),
    // Nunchaku (upstream torch2.7 build and the cu13.0/torch2.10 build).
    (
        "nunchaku-1.2.1+cu13.0torch2.10-cp311-cp311-win_amd64.whl",
        "a31fbb48650f9a5c0ed48e447f34e64e471318ff3466f73c9167a09bede28771",
    ),
    (
        "nunchaku-1.2.0+torch2.7-cp310-cp310-win_amd64.whl",
        "9eb877dad499bbe7eae3bc0e98b7f65b96bcaa81771ea9657f3a576cb0847d0f",
    ),
    // LightX2V kernels.
    (
        "lightx2v_kernel-0.0.2+torch2.10.0-cp311-abi3-win_amd64.whl",
        "1b2a722b0dfac79962b64eabb99fe7295defeab37f5b002d95f2b8b3aa1cc210",
    ),
    // SageAttention — the launcher's safe build (post6 cu130) and upstream's
    // pinned PyTorch-2.7.1 builds (cp310/cp311).
    (
        "sageattention-2.2.0+cu130torch2.10.0andhigher.post6-cp310-abi3-win_amd64.whl",
        "1635283f5c01ec3cda58a784d0d7eabbcaffaf9511d1b263db4750e1ed7958bb",
    ),
    (
        "sageattention-2.2.0+cu128torch2.7.1-cp311-cp311-win_amd64.whl",
        "92b2ef73b6b5d6cf8d23914c9c408204264c49ac6e264676d5f5a70d293f430a",
    ),
    (
        "sageattention-2.2.0+cu128torch2.7.1-cp310-cp310-win_amd64.whl",
        "68015b797baef5c3b4db4f645052299d6f0174476f1b9f12973a86f7341569fa",
    ),
    // SpargeAttention.
    (
        "spas_sage_attn-0.1.0+cu130torch2.9.0andhigher.post4-cp39-abi3-win_amd64.whl",
        "2b34a5abe45c0ea51872f78dc1d0b407893ac5ecab3bf71425a73e769e47f27a",
    ),
    (
        "spas_sage_attn-0.1.0+cu128torch2.7.1.post3-cp39-abi3-win_amd64.whl",
        "425afea0544f948f5236199f1e23f13e821a5d13cf761035ab9723b5de25e802",
    ),
    // FlashAttention-2 Windows wheels (CUDA 2.8.3, ROCm 2.8.4).
    (
        "flash_attn-2.8.3-cp311-cp311-win_amd64.whl",
        "de0e6713775ace8d0b74d5b04ac8bff2767362a2d86f81e2b7a6e6b8dd924e52",
    ),
    (
        "flash_attn-2.8.4-py3-none-win_amd64.whl",
        "90dda5975e87e53a9e3ce8f8761c410212e5a7e01a7bc1e9a8d8a601531445bc",
    ),
];

/// Percent-decode a URL path segment (`%2B` → `+`, which local-version wheels
/// need; every other escape is decoded too).
pub(crate) fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The wheel filename a pip spec names, when that spec is an HTTP(S) `.whl`.
/// Query/fragment are dropped; the name is percent-decoded (`+` in a
/// local-version wheel arrives as `%2B`).
pub(crate) fn wheel_filename(url: &str) -> Option<String> {
    let trimmed = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let path = trimmed.split(['?', '#']).next().unwrap_or("");
    let last = path.rsplit('/').next()?;
    if last.is_empty() {
        return None;
    }
    let name = percent_decode(last);
    name.to_ascii_lowercase().ends_with(".whl").then_some(name)
}

/// True for a pip argv token that is a downloadable wheel URL.
pub(crate) fn is_wheel_url(token: &str) -> bool {
    wheel_filename(token).is_some()
}

/// Launcher-pinned hash for a wheel URL, when the exact asset is known.
pub(crate) fn pinned_sha(url: &str) -> Option<&'static str> {
    let name = wheel_filename(url)?;
    PINNED_WHEELS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(&name))
        .map(|(_, h)| *h)
}

/// `(owner, repo, tag, asset)` for a GitHub release-asset URL.
pub(crate) fn github_release_asset(url: &str) -> Option<(String, String, String, String)> {
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))?;
    let path = rest.split(['?', '#']).next().unwrap_or("");
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 6 || parts[2] != "releases" || parts[3] != "download" || parts[0].is_empty() {
        return None;
    }
    Some((
        parts[0].to_string(),
        parts[1].to_string(),
        percent_decode(parts[4]),
        percent_decode(parts[5]),
    ))
}

/// Bare lowercase hex from a GitHub `digest` field (`sha256:<hex>`), rejecting
/// other algorithms and malformed values.
pub(crate) fn hex_from_digest(digest: &str) -> Option<String> {
    let hex = digest
        .strip_prefix("sha256:")
        .or_else(|| digest.strip_prefix("SHA256:"))?
        .trim();
    (hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| hex.to_ascii_lowercase())
}

/// GitHub's published digest for the release asset a URL points at.
/// Network failure, a missing asset or a missing digest are all `None`.
pub(crate) async fn github_asset_digest(url: &str) -> Option<String> {
    let (owner, repo, tag, asset) = github_release_asset(url)?;
    let api = format!("https://api.github.com/repos/{owner}/{repo}/releases/tags/{tag}");
    let client = reqwest::Client::builder()
        .user_agent("wan2gp-tauri")
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let v: serde_json::Value = client.get(api).send().await.ok()?.json().await.ok()?;
    v.get("assets")?
        .as_array()?
        .iter()
        .find(|a| a.get("name").and_then(|n| n.as_str()) == Some(asset.as_str()))
        .and_then(|a| a.get("digest").and_then(|d| d.as_str()))
        .and_then(hex_from_digest)
}

/// SHA-256 of a file, as bare lowercase hex.
pub(crate) fn sha256_file(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = sha2::Sha256::new();
    std::io::copy(&mut file, &mut hasher).ok()?;
    Some(format!("{:x}", hasher.finalize()))
}

/// Where a verified wheel's hash came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WheelTrust {
    /// Matched a launcher pin — the strongest guarantee (TOFU).
    Pinned,
    /// Matched GitHub's published digest for this exact asset.
    PublishedDigest,
    /// Neither was available; TLS only, hash logged.
    Unverified,
}

impl WheelTrust {
    /// Whose sha256 these bytes were matched against, and how the launcher
    /// knows it. The publisher is named from the URL because the gates differ:
    /// most kernel wheels are deepbeepmeep's (`deepbeepmeep/kernels`,
    /// `deepbeepmeep/WGPKernels`), while Nunchaku and SageAttention ship from
    /// their own repos — a bare "verified" would hide whose digest it was.
    fn matched(self, url: &str) -> String {
        let (who, tag) = github_release_asset(url)
            .map(|(owner, repo, tag, _)| (format!("{owner}/{repo}"), tag))
            .unwrap_or_else(|| ("the publisher".to_string(), String::new()));
        match self {
            WheelTrust::Pinned => {
                let release = if tag.is_empty() {
                    String::new()
                } else {
                    format!(" for release {tag}")
                };
                format!("the sha256 {who} published{release} (launcher pin)")
            }
            WheelTrust::PublishedDigest => {
                let release = if tag.is_empty() {
                    String::new()
                } else {
                    format!(" (release {tag})")
                };
                format!("the sha256 GitHub publishes for {who}{release}")
            }
            WheelTrust::Unverified => "nothing published to compare against".to_string(),
        }
    }

    /// The evidence line every verified wheel prints under its name: the hash
    /// itself, then whose sha256 it matched — so "verified" is never something
    /// the user has to take on faith.
    fn evidence(self, hash: &str, url: &str) -> String {
        format!("    sha256 {hash} — matches {}\n", self.matched(url))
    }
}

/// Stream a URL to `dest`, hashing as it goes; returns the SHA-256 hex.
/// Writes to `<dest>.part` first so an interrupted download never leaves a
/// file that looks installable.
async fn download_to<E: Fn(&str)>(url: &str, dest: &Path, emit: &E) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent("wan2gp-tauri")
        .timeout(std::time::Duration::from_secs(1800))
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;
    let mut resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("download failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("download failed: HTTP {}", resp.status()));
    }
    let total = resp.content_length();
    let part = dest.with_file_name(format!(
        "{}.part",
        dest.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    let mut file = std::fs::File::create(&part)
        .map_err(|e| format!("cannot write {}: {e}", part.display()))?;
    let mut hasher = sha2::Sha256::new();
    let mut got: u64 = 0;
    let mut next_mark: u64 = 0;
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("download interrupted after {got} bytes: {e}"))?
    {
        hasher.update(&chunk);
        std::io::Write::write_all(&mut file, &chunk)
            .map_err(|e| format!("cannot write {}: {e}", part.display()))?;
        got += chunk.len() as u64;
        if got >= next_mark {
            next_mark = got + 32 * 1024 * 1024;
            let msg = match total {
                Some(t) if t > 0 => {
                    format!("    … {}/{} MB\n", got / (1024 * 1024), t / (1024 * 1024))
                }
                _ => format!("    … {} MB\n", got / (1024 * 1024)),
            };
            emit(&msg);
        }
    }
    drop(file);
    let hex = format!("{:x}", hasher.finalize());
    std::fs::rename(&part, dest).map_err(|e| format!("cannot finalize {}: {e}", dest.display()))?;
    Ok(hex)
}

/// Download (or reuse from the cache) the wheel named by `url`, verify it, and
/// return the local path pip should install. `Err` means **do not install** —
/// either the checksum did not match or the download failed.
pub(crate) async fn fetch_verified<E: Fn(&str)>(
    url: &str,
    cache: &Path,
    emit: &E,
) -> Result<(PathBuf, WheelTrust), String> {
    let name = wheel_filename(url).ok_or_else(|| format!("not a wheel URL: {url}"))?;
    let dest = cache.join(&name);

    let mut trust = WheelTrust::Pinned;
    let expected = match pinned_sha(url) {
        Some(h) => Some(h.to_ascii_lowercase()),
        None => match github_asset_digest(url).await {
            Some(h) => {
                trust = WheelTrust::PublishedDigest;
                Some(h)
            }
            None => {
                trust = WheelTrust::Unverified;
                None
            }
        },
    };

    if dest.exists() {
        match expected.as_deref() {
            Some(want) => {
                // Re-hash the cached file and print what it actually is: a
                // cache hit still states the hash and whose digest it matches.
                let got = sha256_file(&dest);
                if got.as_deref() == Some(want) {
                    let got = got.unwrap_or_else(|| want.to_string());
                    emit(&format!(
                        "[*] wheel cache hit ({name})\n{}",
                        trust.evidence(&got, url)
                    ));
                    return Ok((dest, trust));
                }
                emit(&format!(
                    "[!] cached {name} no longer matches its checksum — re-downloading\n    expected {want} ({})\n",
                    trust.matched(url)
                ));
            }
            // Nothing to check it against: reusing is as good as re-fetching.
            None => {
                emit(&format!(
                    "[*] wheel cache hit ({name}) — no published checksum to verify against\n    sha256 {} — {}\n",
                    sha256_file(&dest).unwrap_or_else(|| "unreadable".to_string()),
                    trust.matched(url)
                ));
                return Ok((dest, trust));
            }
        }
    }

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    emit(&format!("[*] downloading {name} (verifying sha256)…\n"));
    let got = download_to(url, &dest, emit).await?;

    match expected.as_deref() {
        Some(want) if got != want => {
            let _ = std::fs::remove_file(&dest);
            Err(format!(
                "checksum mismatch for {name} — refusing to install\n    expected {want} ({})\n    got      {got}\n\
                 The release asset changed, or the download was tampered with or corrupted.\n\
                 Update Wan2GP and re-run Update GPU Wheels; if it still fails, check the asset on GitHub.",
                trust.matched(url)
            ))
        }
        Some(_) => {
            emit(&format!("[✓] {name}\n{}", trust.evidence(&got, url)));
            Ok((dest, trust))
        }
        None => {
            emit(&format!(
                "[!] {name}: no published checksum found (none on GitHub, or the API was unreachable) — installing on TLS alone\n    sha256 {got}\n"
            ));
            Ok((dest, trust))
        }
    }
}

/// Rewrite a pip argv so every wheel-URL token points at a locally verified
/// file. Non-URL tokens (flags, PyPI specs) pass through untouched.
pub(crate) async fn verify_argv_wheels<E: Fn(&str)>(
    argv: Vec<String>,
    cache: &Path,
    emit: &E,
) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(argv.len());
    for token in argv {
        if is_wheel_url(&token) {
            let (path, _) = fetch_verified(&token, cache, emit).await?;
            out.push(path.to_string_lossy().into_owned());
        } else {
            out.push(token);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decode_handles_local_version_plus() {
        assert_eq!(percent_decode("a%2Bb"), "a+b");
        assert_eq!(percent_decode("plain.whl"), "plain.whl");
        // A stray `%` that is not an escape is kept verbatim.
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn wheel_filename_decodes_and_rejects_non_wheels() {
        assert_eq!(
            wheel_filename("https://github.com/deepbeepmeep/kernels/releases/download/gguf-v1.0.28/llamacpp_gguf_cuda-1.0.28%2Btorch210cu130py311-cp311-cp311-win_amd64.whl").as_deref(),
            Some("llamacpp_gguf_cuda-1.0.28+torch210cu130py311-cp311-cp311-win_amd64.whl")
        );
        assert_eq!(
            wheel_filename("https://x/y/w.whl?token=1#frag").as_deref(),
            Some("w.whl")
        );
        assert!(wheel_filename("sageattention==1.0.6").is_none());
        assert!(wheel_filename("https://x/nope.tar.gz").is_none());
    }

    #[test]
    fn is_wheel_url_only_for_http_wheel_tokens() {
        assert!(is_wheel_url("https://host/a/b/c.whl"));
        assert!(is_wheel_url("http://host/c.whl"));
        assert!(!is_wheel_url("--no-deps"));
        assert!(!is_wheel_url("https://host/c.tar.gz"));
        assert!(!is_wheel_url("C:\\tmp\\c.whl"));
    }

    #[test]
    fn pinned_sha_matches_known_assets_only() {
        let gguf = "https://github.com/deepbeepmeep/kernels/releases/download/gguf-v1.0.28/llamacpp_gguf_cuda-1.0.28%2Btorch210cu130py311-cp311-cp311-win_amd64.whl";
        assert_eq!(
            pinned_sha(gguf),
            Some("551eb355c144e653f4416c55de61ac50e7380d5c48336a774f271404832d6148")
        );
        // A version bump is unknown here on purpose — the GitHub digest covers it.
        let newer = "https://github.com/deepbeepmeep/kernels/releases/download/gguf-v1.0.99/llamacpp_gguf_cuda-1.0.99%2Btorch210cu130py311-cp311-cp311-win_amd64.whl";
        assert!(pinned_sha(newer).is_none());
        assert!(pinned_sha("sageattention==1.0.6").is_none());
    }

    #[test]
    fn pin_table_is_well_formed() {
        for (name, hash) in PINNED_WHEELS {
            assert!(name.ends_with(".whl"), "not a wheel filename: {name}");
            assert_eq!(hash.len(), 64, "not a sha256: {name}");
            assert!(
                hash.chars().all(|c| c.is_ascii_hexdigit()),
                "not hex: {name}"
            );
            assert_eq!(*hash, hash.to_ascii_lowercase(), "not lowercase: {name}");
        }
        for (i, (name, _)) in PINNED_WHEELS.iter().enumerate() {
            assert!(
                !PINNED_WHEELS[i + 1..].iter().any(|(n, _)| n == name),
                "duplicate pin for {name}"
            );
        }
    }

    #[test]
    fn evidence_line_prints_the_hash_and_whose_digest_it_matched() {
        const GGUF: &str = "https://github.com/deepbeepmeep/kernels/releases/download/gguf-v1.0.28/llamacpp_gguf_cuda-1.0.28%2Btorch210cu130py311-cp311-cp311-win_amd64.whl";
        let h = pinned_sha(GGUF).unwrap();
        let line = WheelTrust::Pinned.evidence(h, GGUF);
        assert!(line.contains(h), "the hash itself must be printed: {line}");
        assert!(
            line.contains("deepbeepmeep/kernels"),
            "whose digest it matched must be named: {line}"
        );
        assert!(
            line.contains("gguf-v1.0.28"),
            "the release tag pins it down: {line}"
        );
        assert!(line.contains("matches"), "{line}");

        // A wheel from someone else's repo must credit *them*.
        const NUNCHAKU: &str = "https://github.com/nunchaku-ai/nunchaku/releases/download/v1.2.1/nunchaku-1.2.1+cu13.0torch2.10-cp311-cp311-win_amd64.whl";
        let line = WheelTrust::PublishedDigest.evidence(&"ab".repeat(32), NUNCHAKU);
        assert!(line.contains("nunchaku-ai/nunchaku"), "{line}");
        assert!(line.contains("v1.2.1"), "{line}");
        assert!(
            !line.contains("deepbeepmeep"),
            "must not credit the wrong publisher: {line}"
        );
    }

    #[test]
    fn an_unverified_wheel_never_reads_as_a_match() {
        let line = WheelTrust::Unverified.matched("https://example.com/pkg-1.0.whl");
        assert!(line.contains("nothing published"), "{line}");
        assert!(!line.contains("matches"), "{line}");
    }

    #[test]
    fn github_release_asset_parses_release_urls() {
        assert_eq!(
            github_release_asset("https://github.com/nunchaku-ai/nunchaku/releases/download/v1.2.1/nunchaku-1.2.1+cu13.0torch2.10-cp311-cp311-win_amd64.whl"),
            Some((
                "nunchaku-ai".into(),
                "nunchaku".into(),
                "v1.2.1".into(),
                "nunchaku-1.2.1+cu13.0torch2.10-cp311-cp311-win_amd64.whl".into()
            ))
        );
        // Archive links are not release assets.
        assert!(github_release_asset("https://github.com/owner/repo/archive/v1.zip").is_none());
        assert!(github_release_asset("https://pypi.org/simple/x/").is_none());
    }

    #[test]
    fn hex_from_digest_accepts_only_sha256_hex() {
        let h = "a".repeat(64);
        assert_eq!(hex_from_digest(&format!("sha256:{h}")), Some(h.clone()));
        assert_eq!(
            hex_from_digest(&format!("sha256:{}", h.to_uppercase())),
            Some(h.clone())
        );
        assert!(hex_from_digest("sha512:abc").is_none());
        assert!(hex_from_digest("sha256:tooshort").is_none());
        assert!(hex_from_digest(&"z".repeat(64)).is_none());
        assert!(hex_from_digest("").is_none());
    }

    #[test]
    fn sha256_file_matches_known_content() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("abc.bin");
        std::fs::write(&f, b"abc").unwrap();
        assert_eq!(
            sha256_file(&f).as_deref(),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
        assert!(sha256_file(&dir.path().join("missing.bin")).is_none());
    }

    #[test]
    fn wheel_cache_dir_sits_next_to_the_triton_cache() {
        let d = wheel_cache_dir_for(Path::new(r"C:\Wan2GP"));
        assert_eq!(d, Path::new(r"C:\Wan2GP\wheel-cache"));
    }

    /// Network test, ignored by default: proves the real download → hash →
    /// verify → cache-hit path end to end against the smallest pinned wheel.
    /// Run it (with every other ignored test) via
    /// `powershell -ExecutionPolicy Bypass -File scripts\live-tests.ps1`,
    /// or directly with
    /// `cargo test --lib wheels::tests::live_download_verifies_and_caches -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_download_verifies_and_caches() {
        const URL: &str = "https://github.com/deepbeepmeep/kernels/releases/download/Light2xv/lightx2v_kernel-0.0.2%2Btorch2.10.0-cp311-abi3-win_amd64.whl";
        assert!(
            pinned_sha(URL).is_some(),
            "expected a pin for the live test wheel"
        );
        let dir = tempfile::tempdir().unwrap();
        let msgs = std::sync::Mutex::new(Vec::new());
        let emit = |s: &str| msgs.lock().unwrap().push(s.to_string());

        let (path, trust) =
            tauri::async_runtime::block_on(fetch_verified(URL, dir.path(), &emit)).unwrap();
        assert_eq!(trust, WheelTrust::Pinned);
        assert_eq!(
            sha256_file(&path).as_deref(),
            pinned_sha(URL),
            "downloaded bytes must match the pin"
        );
        // The console must carry the evidence, not just the word "verified".
        let want = pinned_sha(URL).unwrap();
        assert!(
            msgs.lock()
                .unwrap()
                .iter()
                .any(|m| m.contains(want) && m.contains("deepbeepmeep/kernels")),
            "the verified line must print the hash and whose digest it matched"
        );

        // Second call reuses the verified file instead of re-downloading.
        let (again, _) =
            tauri::async_runtime::block_on(fetch_verified(URL, dir.path(), &emit)).unwrap();
        assert_eq!(path, again);
        assert!(msgs
            .lock()
            .unwrap()
            .iter()
            .any(|m| m.contains("wheel cache hit")));

        // A corrupted/tampered cache entry must be noticed and replaced with
        // the verified bytes instead of being installed as-is.
        std::fs::write(&again, b"not a wheel").unwrap();
        let (fixed, _) =
            tauri::async_runtime::block_on(fetch_verified(URL, dir.path(), &emit)).unwrap();
        assert_eq!(sha256_file(&fixed).as_deref(), pinned_sha(URL));
        assert!(msgs
            .lock()
            .unwrap()
            .iter()
            .any(|m| m.contains("no longer matches its checksum")));

        for m in msgs.lock().unwrap().iter() {
            print!("{m}");
        }
    }

    /// Coherence guard: every wheel the launcher can generate for itself must
    /// be pinned, so a version bump cannot silently downgrade the gate to
    /// "whatever GitHub says". Failing here means "add the new digest to
    /// PINNED_WHEELS" (or consciously decide the digest fallback is enough).
    #[test]
    fn every_wheel_the_launcher_generates_is_pinned() {
        for kind in ["cu130", "cu128", "rocm100"] {
            let u = crate::hw::wgpkernels_wheel_url(kind, "win_amd64").unwrap();
            assert!(pinned_sha(&u).is_some(), "unpinned WGPKernels wheel: {u}");
        }
        // The GGUF floor rewrites a stale CUDA wheel to the pinned build
        // (py311/cu130 and py310/cu128), and leaves the HIP wheel alone.
        // Cmds keep their `--no-deps` prefix, so split them like Sync does.
        let stale_cu130 = "https://github.com/deepbeepmeep/kernels/releases/download/gguf-v1.0.26/llamacpp_gguf_cuda-1.0.26%2Btorch210cu130py311-cp311-cp311-win_amd64.whl";
        let stale_cu128 = "https://github.com/deepbeepmeep/kernels/releases/download/gguf-v1.0.26/llamacpp_gguf_cuda-1.0.26%2Btorch271cu128py310-cp310-cp310-win_amd64.whl";
        let mut checked = 0;
        for cmd in [
            stale_cu130.to_string(),
            stale_cu128.to_string(),
            crate::hw::GGUF_1028_WIN_PY312_HIP.to_string(),
            crate::install::SAGE_SAFE_WIN_URL.to_string(),
        ] {
            let floored = crate::hw::apply_gguf_override_with(&cmd, crate::hw::GGUF_FLOOR);
            for token in crate::install::pip_install_argv(&floored) {
                if is_wheel_url(&token) {
                    assert!(pinned_sha(&token).is_some(), "unpinned wheel: {token}");
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 4, "expected four GGUF/sage wheel URLs to check");
    }
}

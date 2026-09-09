# Changelog — Wan2GP Desktop Launcher (Tauri)

All notable changes. Dates are release dates; `Unreleased` tracks `master`.

## [Unreleased]

- No-restart prerequisites: git/conda/python/py resolve to absolute known locations (plus the owned uv copy), so freshly installed tools work instantly — no PATH refresh, no launcher restart; gates and every spawn site use the same resolver

- Launcher-owned uv: the installer provisions `uv` into its own `<dataDir>\.tools` via the official standalone installer (`UV_INSTALL_DIR`, keeps the install receipt so `self update` works) and prefers it for every later run — PATH copies belonging to other apps (0.5.3 report: Hermes agent's bundled uv, self-update hit its file lock) are fallback only, never self-updated; offline last resort is a plain copy (works, but receipt-less until a proper install replaces it)

## [0.5.3] — 2026-09-08

- Pre-install detection gate: fail fast on missing git / Microsoft Basic Display Adapter before any download, warn on pre-2024 AMD drivers, multiple AMD GPUs (dGPU+iGPU order check), unreadable VRAM, and missing Defender exclusions; stale markers and CUDA-era configs are reported instead of tripping the install
- Rebuilds now name antivirus quarantine explicitly when the damage has the missing-DLL shape

- AMD hardening: install now runs a real GPU compute probe after setup (bf16 GEMM + the quanto int8 pattern that crashed 0.5.2 on gfx1201) in both HSA modes and records the winner for launch, instead of trusting `import torch`; double failure auto-reseats torch to the staging float and probes again, honest error if everything fails (no more false "Installation complete")
- System → Troubleshooting gains **Verify GPU compute** (same probe on demand — use after AV quarantine restores, driver updates, or hand-deleted envs) and the debug bundle now carries the AMD evidence line (HSA choice, live HSA/MIOPEN env, numpy + quanto versions)
- Deleted env folders no longer show as a healthy active env: the registry entry validates the folder + interpreter and reads as missing (installer prompt) when gone
- No-env states (repo without working env) now offer Fresh repo alongside Adopt: same backup-modal wipe-and-restore flow as the healthy-state trio, for corrupted repo code Adopt can't repair
- Verify GPU compute now also proves the wheels import (version-present is not loadable — AV quarantine and wrong-torch ABIs break imports while versions look fine); installed-but-broken dists fail the check with the culprit named

## [0.5.2] — 2026-09-07

- AMD install actually installs ROCm now: upstream `setup.py --auto` re-detects the GPU itself via `wmic.exe` (removed on current Windows 11 → `Unknown` → `RTX_40` → full CUDA stack, so the 0.5.1 TheRock torch patch never ran) and reads VRAM via `nvidia-smi` only (→ 8 GB default). The launcher now patches the cloned `setup.py` to honor its own verdict (`WAN2GP_TAURI_GPU_PROFILE`/`WAN2GP_TAURI_VRAM_GB`, allowlisted to real `setup_config.json` keys, logged, idempotent, refuses to write on upstream drift) — NVIDIA routing is unchanged, and its detection is more correct than upstream's (bare `"50"` substring calls a GTX 1050 `RTX_50` there)
- AMD VRAM: known-card table fallback (`R9700` → 32 GB, 9070/7900/7800/… families, Radeon PRO W-series) when WMI `AdapterRAM` and the registry both miss — the reporter's card now tiers as 32 GB (Profile 1) instead of defaulting to 8 GB; stale CUDA-era `wgp_config.json` (`sage` attention on an AMD box) is removed before `setup.py` so it regenerates correctly

## [0.5.1] — 2026-09-07

- AMD: exact-pinned ROCm 7.15 torch stack (torch 2.12.0 / torchvision 0.27.0 / torchaudio 2.11.0 `+rocm7.15.0a20260728` + per-target device packs from `whl-multi-arch` — pip-verified closure for all four profiles, confirmed working on RDNA 4; staging float on retry), replacing the stale `/v2/` float (torch 2.10 + ROCm 7.13) and the `ROCm 6.5` / `PyTorch 2.7.0` labels (now factual: `ROCm 7.15` / `PyTorch 2.12`)
- AMD VRAM: 32-bit `AdapterRAM` cap values (0 / 0xFFFFFFFF ≈ 4095 MB) now read as `(VRAM unknown)` instead of a fake 4 GB figure (the R9700 report); registry probe also reads AMD PRO drivers' `qwMemorySize` with model-number token fallback so 32 GB resolves
- AMD `numpy==1.26.4` pin now applies only when torch isn't a 7.15 build (the 7.15 stack resolves with numpy 2.x — downgrading under it risked breaking torch)

## [0.5.0] — 2026-09-07

- AMD installer pipeline (docs-led per upstream `docs/AMD-INSTALLATION.md`): WMI detection fallback (was nvidia-smi-only → UNKNOWN/CPU), R9700 → `AMD_GFX1201`, 64-bit registry VRAM (32 GB cards tier correctly), per-family TheRock nightlies patched into setup.py's torch step (release → staging retry), `numpy==1.26.4` pin, ROCm session env + HSA override at launch, dedicated `AMD_GFX103X` RDNA 2 key. NVIDIA paths untouched; simulated-R9700 integration test
- Maintainer has no AMD hardware — AMD users: please report issues with the install-log `[hw]` line + `torch.cuda.is_available()`/device name + `Win32_VideoController` Name/DriverVersion (see README AMD note)
- Intel honesty: no XPU backend exists upstream, so the launcher stops promising it — iGPU/Arc both install CPU torch exactly as before (nothing working changes), while the overview shows an honest kernel-free `INTEL_CPU` row instead of uninstalled CUDA wheels, and Arc gets an explicit not-possible note

## [0.4.6] — 2026-09-06

- GPU Kernel Wheels card moved above Active Environment — sync state visible without scrolling (same IDs, renderer untouched)
- LightX2V no longer reports "not installed (want 0.0.2+torch2.10.0)" right after sync: status scan queried dist `lightx2v` but the wheel installs `lightx2v_kernel` (installer was always correct, only the dashboard lied)
- OpenCV row populates again (scan aliased `opencv-python→opencv`, a dist that doesn't exist, and the frontend read the wrong versions key; Check-Updates now targets PyPI `opencv-python`)
- Full RTX 20/30/40/50 detection audit vs upstream setup.py/setup_config.json: Nunchaku/GGUF/Sage/Sparge/Flash/Triton all correct; hw kernel keys aligned to upstream (`nunchaku_cu13`, `light2xv`); x050→RTX_50 matches upstream, GTX 16xx→GTX_10 stays an intentional launcher divergence

## [0.4.5] — 2026-09-06

- Installs that finish: retries resume cleanly via completion marker (verify-in-seconds instead of re-download, broken envs wiped first), plus one automatic setup.py retry on transient network failure
- Embedded view fixes: Gradio fills the window (no more half-screen collapse), zoom slider scrolls and survives view rebuilds, full Permissions-Policy (downloads, autoplay, camera/mic), toast on every gallery save (WebView2 downloads complete silently)
- No more false "Chrome not installed" flash (repeated-negative probe); Stop actually stops (bootstrap/exe-path matcher, port-listener kill, verify + survivors report) plus an always-visible Stop All button (Wan2GP + OpenCode), now the single stop control
- Gallery downloads: arrival toast is clickable and opens a native Save-As picker (WebView2 completes saves silently); embed Permissions-Policy covers autoplay/camera/mic/PiP/share
- Upstream v12.72 parity: Deepy session keys in presets, deepy_sessions/ in reinstall backup+restore, %2B-safe wheel parsing, GGUF 1.0.21 installed per docs while setup_config lags (auto-follow after)
- AMD profile env (HSA_OVERRIDE_GFX_VERSION) exported at launch — declared in setup_config.json but consumed by nothing

## [0.4.4] — 2026-09-06

- Install retries resume cleanly: completion marker verifies finished installs in seconds, failed envs are wiped first (setup.py can never resume into them), and setup.py auto-retries once on transient network failure
- Upstream Wan2GP v12.72 parity: Deepy session keys in presets, deepy_sessions/ in reinstall backup+restore, %2B-safe wheel version parsing for the coming GGUF 1.0.21 flip
- Embedded Gradio view can no longer collapse to a 150px half-screen (flex base rules + measured pixel fit)

## [0.4.3] — 2026-09-06

- Hardened backend: pip spec guard (blocks --flags/-r/-e/http tarballs), exit-code propagation everywhere (no more false success), https-only plugin installs
- Honest prerequisites: winget Python exact-pin check, Store-shim filter, git live-resolve + SHA-pinned installers, working Python fallback (3.11.9 — 3.11.14 ships no binary), uv fallback Bypass fix
- Drift fixes: Intel XPU profile (no NVIDIA wheels on Arc), pip preview uses the real validator, SpargeAttention dedup
- Frontend: baseline CSP + trimmed capabilities, DLSS consent as a real popup (shared modal convention, checkbox instead of typed I ACCEPT), KeyError crash recovery (backup + reset + relaunch)
- Fail fast when the install drive has <10 GB free (mirrors the model-drive gate instead of dying of ENOSPC mid-setup)

## [0.4.2] — 2026-09-06

- Self-repairing toolchain: a corrupt uv (not just outdated) is reinstalled automatically with one retry; failure diagnostics include the manual reinstall command
- Prerequisites survive the real world: official-installer fallbacks when winget is missing or fails (git/Python/uv/Miniconda), smarter probes (conda paths, `py` launcher)

## [0.4.1] — 2026-09-06

- Prerequisites that finish the job: one-click installs for git/uv/Python 3.11/Miniconda (Python/Conda buttons led nowhere before), PATH re-read from the registry so installs usually continue without a restart, and a `py -3.11` shim for venv mode when no launcher exists (Electron has all four installers; its Python is now 3.11.14)
- Python fallback that counts: preflight finds before downloading (a manually installed exact Python is accepted — setup.py's `uv venv` reuses the same discovery, so the env gets built from it), exact-version verify everywhere, and failure messages that list what was actually found plus the three fixes
- Resolved install stack renders again (was fed the wrong backend shape, so it never appeared) — now with GPU/CUDA/Python/uv/profile, free disk, and target verdict

## [0.4.0] — 2026-09-05

No more silent failures: every step of install, reuse, migrate and launch is checked, reported honestly, and recoverable.

**Install tells the truth**
- `setup.py` exit code propagated with actionable hints — a dead install can never report `Installation complete!` again; failure offers Retry + Copy diagnostics
- Success gated on a post-install smoke test (`import torch` + CUDA visible on NVIDIA), not just exit code 0
- Task list tracks what `setup.py` actually emits (`[*] Install <Component>` headers + uv package lines) — phases no longer stick on PENDING mid-install
- Exact Python pin preflight (`uv self update` → provision → run-verify → force-reinstall if corrupt, downloads forced on, uv data-drive space checked); pin read from upstream `setup_config.json` so the next bump can't re-open the hole
- Missing-tool checks (`git`/`uv`/`python`/`conda`) actually trigger now

**Reuse, migrate & clean, safely**
- Target-folder triage: empty / healthy / broken-env / repo-no-env / Pinokio / foreign, with Fresh / Install-repair / Choose-empty-folder choices instead of blind merges
- "Use existing" validates first (python exists + runs) and offers repair on failure
- Reinstall always asks first: backup dialog with folder sizes, optional plugins/settings backup (restored automatically), per-model Move-to… rows
- `move_folder` has live progress, locked-file tolerance and post-verify; model-drive disk gates (warn <50 GB, block <10 GB per drive)
- Pinokio trees detected and refused (install/repair/wipe/uninstall) — one-click "reuse its models in a fresh install" instead; reusing a Pinokio install directly is not supported
- Drive roots auto-resolve (`J:\` → `J:\Wan2GP`); picked folders stick even before they exist (fixed fallback to `C:\Wan2GP`); missing previous install (disconnected drive) warns instead of a blank first run
- Keep/Update/Skip choices only appear for healthy installs; uninstall and post-uninstall return to the installer, never an empty dashboard
- Manage → Updates has "🧭 Run Setup again"; uninstall is an explicit Keep my models / Delete everything (sizes shown, AGREE to confirm) / Cancel modal
- Migration modal: explicit Move & restart vs Just switch to it (no second popup, drive roots auto-resolve)
- Live download panel during install: per-file rows with sizes, animated bars and installed versions parsed from uv output
- Active Environment has "reinstall": full env recreate (venv, Python, torch, kernels, smoke test) — "restore" only re-pips requirements

**Launch & everyday polish**
- Launch pre-flights `import torch` and refuses with directions instead of a traceback; exit codes render as numbers; Launch buttons need repo + active env
- Default browser honored: Brave/Opera/Vivaldi detected (fixed `%LocalAppData%` expansion + Program Files candidates); fallback is logged, successful launches log the browser used
- Installer overview shows full per-profile versions (Triton/Sage/Sparge/Flash, kernel labels) like Electron
- Plugin updates highlighted (amber row + bold badge); env unlink narrates deletions (per-directory lines + live current file); unlink/restore hidden without a repo; `[LAUNCH ERROR] undefined` fixed everywhere

## [0.3.1] — 2026-09-05

- DLSS5 panel always shows all 8 runtime files with package version + expected per-file SHA-256 (green ✓ when present, red — not installed when missing); backend owns the pinned manifest so labels can't go stale

## [0.3.0] — 2026-09-05

- DLSS5 status now counts `host/nvngx.dll` (8 files, was 7) and README tracks workers v1.1.3 (upstream `33eb156`)
- Launcher update check runs once shortly after boot (5h poll alone left fresh releases unknown for hours)

## [0.2.1] — 2026-09-04

- Live console download bars: bootstrap shim actually wired into launch (isatty patch, HF progress env, `HUGGINGFACE_HUB_TOKEN` mirror, per-launch temp file) — model/LoRA downloads stream tqdm bars

## [0.2.0] — 2026-09-04

- **🧩 Plugin Manager** (Manage → Plugins tab): list/enable/install/update/uninstall Wan2GP plugins, search + sort, per-plugin update checks, catalog refresh from GitHub, ★ favourites auto-installed on fresh setup. **Status Pro** ships as a default plugin — installed and locked-on, still uninstallable
- **DLSS5 installer** (Dashboard card): runs Wan2GP's own `scripts/install_dlss5.ps1` with live per-component checklist (download → SHA-256 ✓ → installed) in the console; strict `I ACCEPT` consent modal, Force (backup + replace) option. Requires Windows 11 + RTX 30+ (Neural Rendering) / RTX 40+ (Frame Generation)
- **Deepy catch-up** (upstream b71026f): local **Qwen3.8 VL 27B** Prime engine (auto-raises 32k context + Summarize + repetition penalty), `repetition_penalty` in Zero preset, Prime/MCP copy
- **LLM engines**: Claude bridge pin 0.1.40 → **0.1.66** (upstream mandate), npm installs routed through `cmd /C` (fixes "program not found" on `.cmd` shims), serve button shows real running state (port-4096 probe), per-engine npm labels
- **Auto-Tune**: **Int8 Kernels** default-on (experimental, ~10% faster with INT8 checkpoints, needs Triton) in recommendation + adjuster + writer
- **Window**: launcher opens maximized
- **Stop**: scoped to our Wan2GP processes only (tracked child + our repo's `wgp.py`) — no longer blanket-kills every Python on the machine

## [0.1.3]

- Renamed to Wan2GP Desktop Launcher Tauri (product, binary, installer)
- Topbar cleanup (port of Electron): refresh button dropped, reload moved after Console, red stop button, title no longer overlaps metrics/buttons

## [0.1.2]

- Env unlink/restore as compact buttons with state-driven visibility; env name now resolved from backend (fixes vanishing buttons)
- Unlink deletes with live console progress instead of freezing the app
- Release script fixes (version filter, ASCII-only for PS 5.1)

## [0.1.1] — current spike build

First Tauri feature-complete build (port of the Electron launcher):

- **Shell** — Rust backend in 10 modules, system WebView2, ~3 MB installer, no console flashes (`CREATE_NO_WINDOW` on every probe)
- **Dashboard** — batched IPC, single-wave panel paint, live sparklines, kernel wheels, env management (unlink/restore), per-package ↑ upgrades with dist-name mapping
- **Launch** — Desktop embed (console-first boot, hide/show keeps session), Browser (waits for ready, honors chosen browser + no-GPU Chrome), External Terminal (visible `.bat` console), Extra Launch Args applied and echoed
- **Auto-Tune** — full 7-profile matrix incl. P3.5/P4.5, fast-LM audio rule, no-CUDA fallback, saved-vs-rec tags, validated writer
- **Deepy** — mode/engine/enhancer pairs enforced in backend, Zero + Prime presets match upstream, live round-trip test
- **Updates** — one-click Tauri updater (signed) + `scripts/release-tauri.ps1`; Wan2GP core update via git pull
- **Notifier** — Apprise engine (Telegram/Discord/…), log-driven complete/fail/progress events, test + auto-install
- **Settings repair** — dropdown clamps + nested-path fix with `.bak-repair` backups, matching the UI shape
- **Migration** — legacy Electron detection + silent removal (data kept), self-uninstall with keep-models prompt, full cleanup on launcher close (server, helper PIDs, Explorer windows)
- **Docs** — README rewritten for Tauri, live screenshot

# Changelog — Wan2GP Desktop Launcher (Tauri)

All notable changes. Dates are release dates; `Unreleased` tracks `master`.

## [0.10.24] — 2026-10-25

- **Shared Image and 3D output folders.** The launcher now manages `image_save_path` and `mesh_save_path` separately (Manage → General), alongside the existing Output folder, so WanGP's four output keys (`save_path`, `image_save_path`, `audio_save_path`, `mesh_save_path`) can each point where you want.
- **Q4 KV cache setting.** New "Deepy KV Cache Quantization" dropdown (Auto / INT8 / Q4) in the Deepy tab, written as `deepy_kv_cache_quantization`. Auto-Tune recommends Q4 on tight VRAM (saves ~24% vs INT8).
- **Civitai API keys.** New settings for Civitai (civitai.com) and Civitai.red API keys, passed as `CIVITAI_API_KEY` / `CIVITAI_RED_API_KEY` env vars.
- **Open Deepy in browser.** One-click button next to Start Deepy Web that starts Deepy (if needed) and opens its URL in the system browser.
- **Wheel pins bumped** to WGPKernels 1.4.2 and GGUF kernels 1.0.29 (SHA-256 verified).
- **Deepy crash workaround note:** "PTQ1 decode inputs must share a CUDA device" is a WanGP-side GGUF kernel issue; try disabling CUDA graphs or using INT8 KV cache.

## [Unreleased]

- **Cross-platform foundation: Linux x86_64 and macOS Apple Silicon.** The
  launcher is now one code base with platform adapters
  (`src-tauri/src/platform/{windows,linux,macos}.rs` + shared `unix.rs`) instead
  of `cfg(windows)` branches scattered through `install.rs`, `launch.rs`,
  `system.rs`, `hw.rs` and `features.rs`. GPU detection, prereq discovery,
  process listing/kill, port owner, path opener, desktop shortcut, auto-start and
  the task-manager hand-off all go through one `Platform` trait. Windows
  behaviour is unchanged; the full suite still runs green on Windows.

- **Capability gating instead of per-tab platform checks.** `index.html` marks a
  control with `data-requires="capability"`; `src/services/platform-gate.js`
  applies the platform's capability map and toggles `.cap-off`. Unknown or empty
  capability names fail closed, so a typo hides a control instead of crashing it.
  The JS map is pinned to the Rust adapters by `tests/platform-gate.test.js`,
  which parses the `supports(Capability::…)` assertions out of
  `platform/mod.rs` — the two can no longer drift. DLSS, kernel wheels, the
  Triton/MSVC toolchain and the Task Manager button disappear on the platforms
  that cannot honour them; `w2gp.platform` comparisons in the tabs are gone.

- **macOS reports real unified memory.** `vramMB` was hard-coded to
  `8192 MiB`, so every Mac looked like a base M1 in the tier and Auto-Tune math.
  It now reads `sysctl -n hw.memsize` (8 GB only as a fallback when sysctl
  refuses).

- **`tool_usable` has a non-Windows arm.** The Windows adapter called a
  Windows-only helper while every adapter compiles on every target, so a Linux
  release build would have failed to link `crate::base::tool_usable`. It now
  tests with `sh -c 'command -v "$1"'` elsewhere.

- **First-run platform bugs.** Linux `.desktop` entries quoted `Exec=` (install
  paths with spaces broke the shortcut); the macOS LaunchAgent label was
  `com.wangp.desktop` and is now `com.wan2gp.desktop`.

- **Multi-platform release workflow.** `.github/workflows/release.yml` builds
  NSIS+MSI, AppImage+deb and app+dmg from one tag and attaches them to a single
  draft release. macOS disables updater artifacts (an unsigned `.app` cannot
  self-update), so `latest.json` only advertises platforms where the update can
  actually apply. CI gains Linux/macOS compile, test, clippy and fmt jobs.

- **Bundles exist.** `build-artifacts.yml` (dispatch-only, no signing key) built
  the first real Unix artifacts on the project's CI: AppImage 85.0 MB (ELF64
  x86-64), `.deb` 7.25 MB (`Depends:` was duplicated by the explicit override,
  now left to Tauri), macOS `.dmg` 5.96 MB (UDIF). Windows keeps NSIS/MSI. The
  release workflow attaches all three to one draft release once
  `TAURI_SIGNING_PRIVATE_KEY` exists as a repo secret.

- **Honest support label.** Windows is *supported*; Linux and macOS are *ported*
  — the code compiles and bundles on real runners, and is unit-tested, but no
  install → launch → generate cycle has run there. See `docs/PLATFORMS.md`.

- **GPU selection actually reaches Wan2GP.** The picker used to pass
  `--gpu cuda:N` only for an explicit pick, so *Auto* launched with no flag at
  all — the one path that skips upstream's `torch.set_default_device()`. Auto is
  now a real decision (the card with the most VRAM), every NVIDIA launch pins
  `--gpu cuda:<nvidia-smi index>`, and the launcher sets
  `CUDA_DEVICE_ORDER=PCI_BUS_ID` so that index is not reordered to
  `FASTEST_FIRST`. Cards are stored by UUID, so a re-slotted or added GPU does
  not move the selection. *All GPUs visible* is the new no-flag escape hatch,
  an Extra Launch Args `--gpu` pauses the picker, and a saved card that is gone
  falls back to Auto with a warning instead of hiding every GPU.

- FIX: GPU selection heuristic updated to prioritize available free VRAM over total VRAM in `best_nv_card`. This resolves `CUDA error: out of memory` on systems with multiple GPUs when using "Let Wan2GP decide" auto selection.
- **Auto-Tune and the hardware profile follow the pinned card.** On a
  multi-GPU box the profile tier, VRAM figures, install plan and the topbar's
  VRAM tile described nvidia-smi's first card (or summed every card) instead of
  the one generation runs on. All of them now resolve the same pick.

- **Launcher GPU setting is no longer a no-op.** It wrote
  `WEBVIEW2_ADDITIONAL_BROWSER_ARGS`, a variable that does not exist; the
  documented `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` is set instead (appended,
  never replacing existing flags), so *Integrated* / *Disabled* can really free
  VRAM on the card generation uses. `--force_low_power_gpu` is broken on
  WebView2 v145+, so *Integrated* now uses the reliable `--disable-gpu`
  (SwiftShader) path. The troubleshoot bundle gained a **GPU pin** line and an
  OOM remedy that pins generation to one card.

- **AMD HIP pin no longer skips iGPU + dGPU boxes.** `wmi_all_gpus` counted
  APUs as discrete cards, so a Radeon iGPU + dGPU reported two discrete
  adapters and the pin was skipped. APUs are now detected by name/model number
  and excluded from the count; the discrete card is pinned at its HIP index
  (APUs enumerate first). A startup diagnostic line reports per-device VRAM,
  the resolved UUID, and the live `CUDA_VISIBLE_DEVICES` / `CUDA_DEVICE_ORDER`.

## [0.10.22] — 2026-10-23

- **Prerequisites panel on the installer screen.** Git, uv, Python and Conda
  are listed with a live status dot, the detected version/path, and per-tool
  **Install** / **Reinstall**, **Check** and **Version** buttons plus **Check
  all**. The panel is a read-only report (`prerequisite_report`) until a button
  is pressed; Reinstall asks for confirmation, wipes the launcher-owned uv copy
  for a clean install, and forces the winget/installer path for the others.

- **Prerequisite installs now work on bare machines and never loop.** The
  missing-tool gate and the installer card share one readiness probe, so a
  python.org install that keeps `py.exe` off `PATH` is detected instead of
  re-showing the same card forever; the card gains **Recheck** and the backend
  re-probes for a few seconds after an install (no launcher restart).

- **venv installs follow the GPU profile's Python.** GTX 10 (`3.10`) and AMD
  (`3.12`) venvs used to fail because the launcher hardcoded `py -3.11` in the
  gate, the shim, and the Python installer; the profile minor is now used
  everywhere (upstream builds the venv with `py -<minor>`). The shim strips the
  profile selector and delegates to the uv-provisioned interpreter, and setup.py
  itself is driven by a runnable interpreter instead of a bare `python` alias
  that may only be the Microsoft Store stub.

- **One-click Python matches the hardware.** The prerequisite installs
  `Python.Python.3.10/3.11/3.12` and the pinned official fallback for that same
  minor (new SHA-256 pins for 3.10.11 and 3.12.10), instead of always 3.11.

- **Conda installs are detected and spawnable.** System-wide
  `C:\ProgramData\Anaconda3` / `Miniconda3` and other roots are found (no second
  3 GB install), `condabin\conda.bat` is resolved to `Scripts\conda.exe` (a .bat
  is not a process image — this was the "setup.py failed to start" path), and a
  fresh conda env can be driven by conda's own base Python when no system Python
  is on `PATH`.

- **Preflight is check-only again.** The "Python preflight" on the first-run
  splash and the diagnostics bundle no longer download/install uv behind the
  user's back; it reads an existing owned/copy/PATH uv and reports what it sees.

- **uv is installed once.** A launcher-owned standalone install is preferred,
  an existing PATH uv is copied (not re-downloaded), and the copy is marked so
  later actions stop retrying the network installer. The uv card no longer says
  "via PowerShell" when winget runs first.

## [0.10.21] — 2026-10-22

- **Manage → System gains a disk cache cleanup panel.** One list, measured on
  disk, of the folders the launcher and Wan2GP rebuild on demand — Gradio temp
  (`%TEMP%\gradio`), the `uv` wheel cache, the launcher wheel cache, the Triton
  JIT cache, pip's download cache and the HuggingFace hub cache. Tick what to
  free, press **Clean selected**, and the folders stay while their contents go
  (with a byte/file summary and per-item errors for anything locked). The
  HuggingFace row is flagged in red and ticked off by default: weights fetched
  through `huggingface_hub` live only in `hub/` and are re-downloaded, while
  models in the Wan2GP model folder and the HF login token are untouched. The
  old uv-only *Calculate size / Purge unused / Remove cache* trio is gone — it
  was wired to a stub command that reported success without deleting anything,
  and the `uv` row here really does empty `.uv-cache`. The backend refuses to
  run while Wan2GP is up and re-checks every path against an allow-list plus a
  root/ancestor guard before deleting.

## [0.10.20] — 2026-10-22

- **The Finetune Manager is opt-in.** The companion plugin is no longer
  installed automatically on fresh installs, after Repair or at startup: the
  automatic install defaults to **off** (`companionPluginOptOut` defaults to
  `true`), the *Manage → Plugins → Finetune Manager* checkbox arrives ticked,
  and the plugin is installed only when you press **Install / repair** (or
  untick the box). Existing installs keep their plugin, uninstalling still
  turns the automatic install off, and nothing about the pack UI is
  advertised when the plugin is absent.

- **AMD ROCm install no longer guesses the device target.** GPU tiers now follow
  the real architecture per name (RDNA 2/3/3.5/4 plus the APUs: 680M/660M/610M,
  Rogue Ally Z1, Strix Point/Halo and Phoenix/Hawk Point 780M), the fallback
  picks the same target upstream's table gives, and Settings → AMD ROCm carries a
  **GPU device target** (leave empty to auto-detect via `clinfo`/the GPU name, or
  pin e.g. `gfx1036,gfx1201`). When Windows reports only a generic adapter name
  and no target/clinfo is available, the install preflight now fails with an
  actionable message instead of installing a stack the GPU cannot run.

- **Multi-GPU and driverless detection hardened.** nvidia-smi multi-GPU queries
  resolve the largest card instead of mashing rows into one blob, the WMI
  fallback prefers a discrete adapter over an iGPU, and a driverless box
  surfaces the Microsoft Basic Display Adapter so the fatal preflight can refuse
  the install.

- **Console Logs server is token-gated and host-checked.** The opt-in log
  viewer on `serverPort + 2` now requires a per-start token (the Logs page shows
  the URL carrying it) and refuses `Host` headers that are neither localhost nor
  an IP literal (DNS-rebinding defence).

- **Finetune Manager uninstall is honoured.** Uninstalling the companion from
  *Manage → Plugins → Finetune Manager* now turns off the automatic install, and
  the companion row in the plugin list is locked to that section.

- **Post-install profile chip reads the backend resolution.** The Install tab's
  *Profile:* chip now renders the backend's resolved tier (RTX_50 / AMD_GFX1201 /
  INTEL_XPU / CPU…) via one labeler instead of a divergent regex copy.

- **Metrics and compute probes fixed.** The topbar CPU tile always publishes the
  current reading (a real 0% was dropped, so the tile went stale), the WMI
  registry VRAM lookup is memoized per display name, and the AMD compute probe
  runs the real int8 GEMM (`qbytes_mm`) when optimum-quanto is present, failing
  on the HIP signature instead of only the simulated broadcast.

- **WanGP v18.21 parity (`9d25c601`) — guidance only.** The two upstream
  commits since `dee66357` are kernel / model-internals work plus the version
  bump, so the launcher itself needs nothing: `wgp.py` adds no CLI argument,
  `setup_config.json` and the requirements files are untouched (GGUF `1.0.28`,
  WGPKernels `1.3.0`, the Triton and torch pins still match), `settings_version`
  stays `2.81` and the `int8_kernels` / `fp8_kernels` choices are unchanged. The
  guide records what did change for users: **FlashVSR+ Turbo** is now the
  FlashVSR extension's default model (BF16 or INT8 ConvRot transformer, cache
  windows 1–3, 16 frames per step on memory profile 0), MiniMax **H3 PDD now
  accepts base-model LoRAs**, the FlashVSR decoder labels were renamed, and
  WanGP's Triton kernels are gated by compute capability (BF16 ≥ 8.0,
  FP16/FP32 ≥ 7.5) instead of an allowlist. The model inventory stays at 276
  (`defaults/` gained no file) and all 38 Guide-tab picks still resolve against
  `9d25c601`.

- **Upstream-docs audit closed three gaps.** All 25 files in upstream `docs/` were
  re-read at `9d25c601` (byte-identical to `dee66357` — 18.21 changed no doc, only
  the root README) and the guide's references were completed: the audio processor
  inventory was missing `remove_vocals` (Remove Vocals, `audio_edit`, the
  `audio_background_removal` handler), the guide never mentioned *Generation
  Preview*'s two Tiny VAE modes and the decoder residency they cost, and its
  upstream-docs list named 17 of 25 files. No launcher code touches any of these
  names — the guide and the README table are the only mirrors.

## [0.10.19] — 2026-10-21

- **Auto-Tune follows upstream's new VRAM coefficient.** Upstream `06106f69`
  raised `vram_safety_coefficient` from `0.80` to `0.85` — the CLI default and
  mmgp's own signature ("max VRAM that should be allocated to preloaded
  models"). The panel kept writing the old 0.80 on every 12 GB+ card, so 5% of
  the VRAM was held back from the models for no reason. Non-tight tiers now
  write **0.85**; tight (under 12 GB) keeps the launcher's own 0.70 valve and
  failsafe 0.60. A test pins all three.

- **Manual VRAM Preload now has an amount.** `video/image/audio_preload_in_VRAM`
  are validated by `memory_profile_apply` but were missing from the Apply
  allowlist and had no control, so choosing **Manual** wrote the mode and left
  the megabytes at whatever was on disk — normally nothing, i.e. a preload of
  0 MB. Three *VRAM Preload (MB)* fields now sit beside the video, image and
  audio preload choices (0–200000, integers, 0 = none), are seeded from disk
  and shown as `saved: 8000 MB`. The image field carries upstream's own
  measurement: ~6000 MB brought Flux dev 1024² P4 steps within ~20% of P1, and
  8000 MB or more comes close to P1.

- **Language Models CPU Compute is settable in the launcher.** v18.20's
  `llm_cpu_compute` (`wgp.py:4183`, `Configuration > Performance > Language
  Models CPU Compute`) decides whether the CPU processes the layers a local
  language model streams from RAM — Deepy and the prompt enhancer answer about
  twice as fast when it does. The Performance Settings panel now carries
  **Auto / On / Off**; it is written only when you pick one, so upstream's
  `Auto` default applies otherwise, and 2-bit / ternary weights (IQ2, Bonsai)
  keep streaming to the GPU either way.

- **P0 is now the preferred profile under 32 GB of RAM.** v18.20's Profile 0
  (*More Speed with Less RAM*) loads models part by part exactly like P4 but
  keeps nothing in Reserved RAM — the running model borrows it and hands it on
  at the next switch. Both `<32 GB` matrix cells reserved RAM that is not there
  for a whole 14B model, so Auto-Tune now writes **P0** for 12–23 GB and under
  12 GB cards with less than 32 GB of RAM (P5 remains the **Prefer failsafe**
  net), and audio follows P0 instead of jumping to P3. The panel also names
  its pick above the matrix — "Preferred here: video P0 · image P0 · audio P0
  — P0 · LowRAM (upstream: experimental …)" — instead of leaving the table to
  be decoded, and the rec:/saved: chips now read P3+/P4+ rather than 3.5/4.5.

- **The Library can delete checkpoints.** Each row of *Downloaded models* now
  has a **Delete** button: it names the file and its size in the confirmation,
  then calls `library_model_delete`, which accepts only `name.ext` or
  `adult-pack/name.ext` with a model extension and re-checks that the canonical
  path is under the checkpoints folder — so the button can never remove
  anything else. The inventory payload gained `rel` for that (a name alone is
  ambiguous between the root and `adult-pack/`), and the compact table gained a
  fixed-width action column beside Size.

- **The launcher no longer contains a model-pack manager.** The Pack tab,
  the 6.9k-line `adult_pack` backend and its 29 Tauri commands, the pinned
  catalog (`adult-pack-manifest.json`), the sources/verify/refresh scripts,
  the `pack:*` npm scripts and the pack test suite are all gone. Pack recipes,
  registry, CivitAI search and downloads are owned by the **Finetune Manager**
  companion plugin, which the launcher installs from a pinned release and can
  repair or opt out of under Manage → Plugins. The CSP no longer allows the
  HuggingFace/CivitAI preview origins. The Library still *reads* files already
  on disk under `adult-pack/` so earlier downloads remain visible; it does not
  manage them.

- **Upstream v18.20 parity (`dee66357`, 2026-10-21): GGUF 1.0.28, WGPKernels
  1.3.0 — and Turing is back in.** The GGUF floor moves to `1.0.28` — the same
  three wheels (cu130/py311, cu128/py310, rocm10/py312) plus the RTX 20
  `PTQ1_0`/Bonsai decode win, where each weight block is decoded once for all
  tokens of a step — so `1.0.27` and everything older now flags stale and
  re-syncs. WGPKernels **1.3.0** supports RTX 20XX and newer, and upstream
  listed `wgpkernels` in the `RTX_20` profile: the launcher's kernel gate,
  profile row and the button's tooltip/refusal text follow, so a 2080 gets the
  INT8 button it was previously told it could not have. Both changes are
  code-only upstream, so the wheels are version-only bumps — but every pin is
  re-hashed: the 16 SHA-256 launcher pins now cover GGUF 1.0.28
  (cu130/cu128/rocm10) and WGPKernels 1.3.0 (cu130/cu128/rocm100), taken from
  GitHub's published release digests.
- **The Wan2GP guide is reconciled to v18.20.** 276 models — including
  `fibo2_dev`, `fibo2_edit_turbo` and `minimax_h3_fasth3_omni` (+ its NVFP4
  variant) — the retired *Language Models Decoder Engine* setting (vLLM is
  replaced by the in-tree `lmgp` rewrite; CUDA Graphs on every profile), the
  `0.85` `vram_safety_coefficient` default, and the Linux DLSS 5
  frame-generation worker with its WSL caveat.
- **Memory profile P0 is selectable.** Upstream's `profile 0` commit adds a
  profile that transfers models part by part like P4 but keeps none of them in
  Reserved RAM for good — each model's parts live there only while it runs, and
  *Dynamic* preload fills the VRAM a generation leaves free. `docs/CLI.md` now
  documents `--profile 0-5` (default 4); the Performance Settings
  video/image/audio dropdowns and the launch-builder profile patch offer P0,
  the arguments help reads 0-5, and Auto-Tune still recommends P1–P5 only.
- **The guide's code notes point at files that exist.** The generation-settings,
  Deepy, install-plan and audio-profile notes cited six renderer services
  deleted in the Rust port (`memory-profile.js`, `auto-tune.js`,
  `deepy-config.js`, `llm-engines.js`, `install-plan.js`, `kernel-resolver.js`);
  they now name `src-tauri/src/features.rs`, `hw.rs` and `wheels.rs`, and the
  profile note lists the real `0-5` domain from `docs/CLI.md`. The "What's new"
  line no longer claims an upstream changelog entry that was never written: it
  cites the changelog's real latest entry (26 Aug 2026) and the four commits
  since the previous pin. The Guide tab's curated picks gain the three v18.20
  models (`minimax_h3_fasth3_omni`, `fibo2_dev`, `fibo2_edit_turbo`) — all
  38 picks re-verified against upstream `defaults/` — and the user guide no
  longer advertises a TeaCache dropdown (the flag is gone; step skipping is a
  WanGP generation setting).

## [0.10.18] — 2026-10-20

- **Finetune JSON now matches Wan2GP's documented shape.** Pack and CivitAI
  recipes put licence/source notes in `model.infos` instead of a root `infos`
  that the model's information button never reads, and copy the catalog's
  `loraMultipliers` into `model.loras_multipliers` so always-loaded LoRAs keep
  their weights. The manager validator now accepts `URLs` as a single reusable
  recipe id, relative local checkpoint paths and bare model filenames, and it
  refuses mismatched `loras`/`loras_multipliers` lists. A reused recipe reports
  one source instead of zero.
- **DLSS 5 status, compatibility and updates now all follow
  DeepBeepMeep/Wan2GP.** Status and SHA verification read the package table in
  `<repo>/scripts/install_dlss5.ps1`, and *Compare with upstream* compares those
  local pins with the same script on `deepbeepmeep/Wan2GP` main — component
  release feeds are no longer treated as update sources. A newer pin means
  "update Wan2GP, then run Install / update DLSS 5"; the launcher never
  downloads a build DeepBeepMeep/Wan2GP has not pinned. A successful install
  now runs the file verification immediately, and a failed installer exit is
  reported as a failure even when an older complete install left every file in
  place, so verified rows turn green without waiting for a manual check. The
  compatibility check also mirrors current upstream's workstation-aware GPU
  tier, so RTX PRO / Ada / RTX Ax000 cards no longer show a false "upstream
  would block this" mismatch.
- **Custom Wan2GP resolutions are editable in Manage → General.** A small
  table editor reads and writes `<repo>/resolutions.json` next to `wgp.py` in
  Wan2GP's own `[[label, "WxH"], ...]` shape. Every row is validated before the
  file is replaced (two strings, positive `WIDTHxHEIGHT`, no duplicate labels or
  sizes), the write is atomic, and the editor is only enabled once `wgp.py`
  exists. **Load example** fills the eight Krea 2 / MiniMax H3 1080p and 1 MP
  sizes; saving an empty table removes all custom entries. Wan2GP caches the
  list at Python start, so restart it after saving.
- **CivitAI LoRAs now install as LoRAs.** Catalog entries can declare
  `"kind": "lora"` (with an API-type fallback for hand-written entries), and the
  downloader files them into `<loras_root>/<collection>/adult-pack/` instead of
  `ckpts/adult-pack/`. Checkpoints still go to `ckpts/adult-pack/` with a
  local-path finetune; LoRAs deliberately get no finetune JSON because Wan2GP
  loads them from the collection folder.
- **A CivitAI token is optional again.** Public releases download without an
  account; a token is attached only when one is stored or typed. Version
  resolution and download URLs use it when present, an HTTP 401/403 explains
  that a token is needed (or that the stored one was rejected), and the token
  help now links straight to the API-key page on both `civitai.com` and
  `civitai.red`.
- **CivitAI installs are recorded, not guessed.** Each download stores its real
  file paths, kind and byte count (`civitaiInstalled`). The manifest reports
  `installed`, `installedKind` and `installedBytes`, so the footer, free-space
  check and "Remove selected" agree with what is actually on disk. A selected
  CivitAI item is no longer re-downloaded or counted as pending, and
  "Fetch missing only" now includes CivitAI items. If the launcher has no token
  of its own, it reuses the `civitai_api_key` from the Wan2GP Finetune Manager
  plugin's `config.json` read-only, so the key is entered once.
- **CivitAI downloads only fetch weights.** A version's auxiliary entries
  (configs, VAEs, training data, sample archives) are skipped instead of being
  filed as models. When a release publishes HIGH and LOW weights as two files,
  the LOW half is written to the recipe's `URLs2` (the second Wan 2.2 phase)
  instead of leaving both in `URLs`. A failed removal now reports the locked
  file and keeps the install record, rather than claiming the item is gone.
- **The Pack tab has a clear order.** A prominent in-development/adult-content
  disclaimer comes first, then the shared Downloads, Find models, Finetune
  recipes and LoRA files tools, with the adult pack gate and catalogue below
  them. CivitAI's token row explains that public releases need no key, and the
  old "token required" checkbox gate is gone.
- **The Library now shows provenance.** Checkpoints are labelled
  `DeepBeepMeep` (a file named in the shipped `defaults/*.json`), `Pack` (in
  `adult-pack/`), or `Yours`; LoRA families report separate Pack and Yours
  counts/sizes (including the nested `adult-pack/` folder); finetunes report
  `Pack`, `Pack · CivitAI`, `Overrides DeepBeepMeep`, or `Yours`. The pack's own
  LoRA inventory uses the same labels, including `Yours (added via pack)` and
  `Pack (unlisted)` so a hand-placed file is not mistaken for a curated one.
  The checkpoint table gives Checkpoint/Origin/Kind/Size real widths, so long
  filenames wrap normally instead of one character per line and Size no longer
  takes the whole table.
- **Adult Pack gains two H3 models.** MiniMax H3 Singularity is available in a
  pruned int8 build (20.9 GB, off by default) alongside the existing w4a8
  finetune, and the 131 MB MiniMax H3 Realism People LoRA (T2V/I2V/R2V, trigger
  `r34l1sm`) joins the H3 group. MysticXXX MMH3 V4 was already in the catalog.

## [0.10.17] — 2026-10-20

- **CivitAI is now a master switch, off by default.** `civitai.red` is the one
  adult host the launcher talks to, and security suites and ISPs routinely block
  it as an adult site. While the switch is off nothing contacts it: the account
  tier is hidden, both source pickers fall back to HuggingFace, and every
  CivitAI command (search, browse, inspect, examples, token check, download)
  refuses locally — no socket, no partial request. Direct downloads of
  user-added CivitAI models are covered too. Turn it on in the pack's own
  settings; the 18+ gate stays independent. HuggingFace browsing, updates and
  catalog fetches are unaffected.
- **CivitAI search and browse work again.** The API rejects comma-separated
  types, as well as the names `LoRA`, `Model` and `GGUF`; it wants repeated
  `types=` parameters with its own spelling (`LORA`, `Checkpoint`, `DoRA`,
  `LoCon`). Both paths now share that mapping, so "Trending" no longer answers
  HTTP 400.
- **Download counts tell the truth.** The footer used to count every selected
  item as a download, even when the LoRA was already on disk or a checkpoint's
  weights were already in `adult-pack/`. It now separates "N to download" from
  "M already on disk", sizes only the pending bytes, and the free-space check
  uses the real LoRA and checkpoint folders. The manifest reports
  `weightsPresent` per checkpoint so the panel does not guess.

## [0.10.16] — 2026-10-20

- **Pack LoRAs now land where Wan2GP actually looks.** Wan2GP lists
  `<loras_root>/<collection>/**` — `wan/`, `wan_i2v/`, `minimax_h3/`, `ltx2/`,
  `qwen/`… — and never scans a sibling `adult-pack/` folder. 0.10.15 therefore
  downloaded the pack's LoRAs into `loras\adult-pack\<family>\`, where they were
  invisible in Wan2GP's LoRA tab. They are now fetched into
  `loras\<collection>\adult-pack\`, mapped per architecture exactly as each
  family handler's `get_lora_dir()` does upstream (Wan 2.2 i2v and t2v share
  `wan`, deliberately). The root follows Wan2GP's own precedence — `--loras`
  from Launch arguments, then `wgp_config.json`'s `loras_root`, then the desktop
  setting — and a `--lora-config` override is honoured per collection, so the
  pack cannot fill a folder Wan2GP never reads. Files already downloaded by
  0.10.15 are moved into place once, automatically, when the Pack tab opens —
  nothing is re-downloaded.
- **Pack checkpoints get their own folder.** Recipes now ask Wan2GP for
  `<checkpoint root>\adult-pack\<file>` using upstream's `URL|subfolder` form,
  which its own downloader and locator both understand. Nothing else on the
  drive changes: `checkpoints_paths` is untouched, the pack simply stops
  mixing its weights with your own or DeepBeepMeep's downloads. Files
  prefetched by 0.10.15 are moved in once, and the pack's own recipes are
  re-pointed, so nothing is downloaded twice. Which drive this lands on still
  follows `checkpoints_paths[0]` — that is also where Wan2GP downloads any
  model, so only moving that root would move the whole library.

## [0.10.15] — 2026-10-19

- **Opt-in Adult (NSFW) model pack — Manage → 📦 Pack.** A new tab in Manage, deliberately
  **not** part of Setup: installing Wan2GP must never carry an opt-in adult-content decision.
  Off by default, and nothing unlocks until the 18+ confirmation is ticked. Two tiers, because
  they behave very differently:
  - **LoRA packs (~100 files, 4 groups: Wan 2.2 / MiniMax H3 / LTX 2.3 / image).**
    Fetched during install with resumable GETs into
    `loras\adult-pack\<family>\`, SHA-256 verified, each file individually
    retriable. HIGH/LOW halves are pinned together — ticking one ticks its
    partner, because Wan 2.2 loads both in the same job.
  - **Checkpoint finetunes (14 recipes).** The launcher writes
    `finetunes/adultpack_*.json` with the remote URLs and lets Wan2GP's own
    downloader fetch weights on first use; each row has an optional *prefetch*
    box for people who want it on disk now.
  - **CivitAI token tier** for author-exclusive releases (DaSiWa TastySin v8,
    Wan 2.2 NSFW Lightning): the version is resolved live at install time (a
    pinned id would be stale the moment the author publishes), fetched with the
    user's own token, and recorded as a local-path recipe. The token lives in
    its own file, never in `desktop-config.json`, and the launcher never
    re-hosts anything.

  Pack content is **data, not code**: `scripts/adult-pack-sources.json` lists the
  repos, `scripts/verify-adult-pack.mjs` resolves them against the HuggingFace
  tree API and pins URL + SHA-256 + size into `src/adult-pack-manifest.json`, and
  the Rust module embeds that manifest. Adding, updating or removing a model is
  a JSON edit plus one script run — no Rust, no UI, no rebuild of the catalog.
  The verifier caches listings (12h) and backs off on HTTP 429 so a full run
  stays polite, and `--deep` cross-checks every URL against the CDN.

  Install-time behaviour is gone with the move: the tab owns the lifecycle — **Download
  selected**, **Write finetune recipes**, **Fetch missing only**, **Remove selected…** and a
  live progress line plus console-style log. The panel shows free-space headroom next to the
  running total, and the selection is remembered between visits.

  Coverage is pinned against Wan2GP v18.17 / the 270-id inventory; a recipe whose
  architecture is missing from the installed repo greys out instead of writing a
  finetune that could never load.

- **The pack now shows you what a model looks like, how old it is, and whether it
  has moved on.** Three gaps in the first cut, all found by looking at the panel
  rather than by reading the code:
  - **Examples.** Most HF mirrors in this catalog publish **no media at all** — a
    55-file repo with zero images, and 0 of 34 repos embed anything in their model
    card. Rows therefore had nothing to show. The verifier now looks each entry up
    on CivitAI (where these creators actually publish their renders), matches it by
    name, and **bakes the example images into the manifest** — so a row shows
    pictures instantly and offline, with no network call per row. Matching is
    bidirectional and conservative: a word glued together on one side
    (`HandjobBlowjobCombo` vs "Combo Handjob Blowjob") still matches, but a name
    that merely *contains* ours ("Lusty Argonian Maid" vs `Lusty_Krea-2`) never
    does — wrong examples are worse than none. Everything unmatched gets a
    **Find examples on CivitAI** button and a search link, which shows candidate
    renders with their names so the user can judge. A dead card preview (one repo's
    model card pointed at `images/undefined_image (1).png`, never committed) is now
    dropped instead of rendering as a broken tile.
  - **Version and release date on every row.** Every entry carries a version
    token from its file name (or the repo's publish date) plus a release date —
    the CivitAI version's `createdAt` where a confident match exists, the HF repo's
    `createdAt` otherwise. Dimension and format markers (`_e320`, `alpha128`,
    `q4_k_m`) are deliberately *not* versions.
  - **Update detection is exact, and applicable.** **Check for updates** lists
    every catalog repo and compares each file's current SHA-256 against the pinned
    one — an update is reported only when the bytes really changed, not because a
    date moved. **Apply & download** then re-points the local hash in
    `adult-pack-pins.json` and downloads the new file, verified against the new
    hash; without that override the download would be discarded for failing the
    stale pin. Rows distinguish *downloaded*, *update available* (you hold outdated
    bytes, with `2.8 GB → 3.0 GB` shown), *newer upstream* (you are current) and
    *new version*, and headline chips answer "what do I already have".
  - Search and trending results carry their example images, version and release
    date too — a title alone tells you nothing about what a model produces.

- **A finetune recipe can be created from the Pack panel without hand-writing JSON.**
  The finetune manager could edit, link-check and delete recipes, but making a new one
  meant writing `finetunes/<id>.json` by hand from upstream's `FINETUNES.md` — the one
  part of the flow that required knowing the format. **＋ New finetune.json** now collects
  it: id, name, base architecture (the picker is built from the manifest of the build you
  have installed, so a recipe that could never load cannot be written), description, and
  the source URLs one per line. The three WanGP fields worth having from the UI are
  optional — **Second phase** (`URLs2`, for HIGH/LOW pairs such as Wan 2.2), **Model
  notes** (`infos`) and **Prompt help** (`prompt_infos`), both Markdown — and are omitted
  from the file when left empty, so a minimal t2v recipe stays minimal. The write goes
  through the same `adult_pack_finetune_write` validation as every other path (name,
  architecture, non-empty usable URLs, never a shipped `defaults/` model), and an id that
  already exists is refused with *open it below and press Edit* rather than silently
  replacing a definition. The guide documents the form and links upstream's
  `docs/FINETUNES.md` for the full field list.

- **Audit pass: confirmations, build metadata, and a warning-free backend.**
  - Pack deletes (single row, bulk selection, saved recipe) now ask through the
    launcher's native `askConfirm`; `window.confirm` is already replaced at boot
    by an async plugin shim the ACL denies, so those three safety prompts were
    being skipped.
  - `Cargo.toml` declares the crate's real MIT license and `rust-version = 1.95`
    (the actual floor imposed by `sysinfo` 0.39 / edition 2024); a `LICENSE`
    file now exists and `package.json` carries the same SPDX id.
  - Process-wide caches use `LazyLock` with their initializer declared in place
    (no `get_or_init` scattered through accessors); dead `PREV_CPU` tracking is
    gone. `cargo fmt --check` and `cargo clippy --all-targets` are both clean.
  - README build prerequisites/asset names and the test counts were stale; the
    guide now points at the current 272-model inventory.

- **The Pack can now add models Wan2GP does not ship, and manage them like anything else.** The
  tab shipped as a fixed catalog with a read-only recipe list, so every path a user actually needs
  for their own models was missing. Three of them, all load-bearing:
  - **Add by link.** Finding a model meant matching it against a keyword, so a name matching no
    search term was unreachable. Pasting a HuggingFace repo (`owner/name` or its URL), a CivitAI
    model URL or a bare CivitAI id now resolves **server-side** (`adult_pack_resolve_url` /
    `parse_source_url`) into the same inspect → pick file → Add flow, and the renderer never
    parses a URL itself. A multi-version CivitAI model gets a version picker, and the detected file
    kind (LoRA vs checkpoint) is an editable select — a wrong guess can never file a model into the
    wrong folder.
  - **The architecture list, and the family, come from the build.** The picker was built from a
    hardcoded map, so it offered architectures this build cannot load. It now prefers what the
    build actually ships (`defaults/*.json` ∪ the curated map, via `adult_pack_arches`, with the
    map as fallback), and a recipe naming an unknown architecture is refused at write time instead
    of written and then unloadable-forever. The family folder (`wan` / `h3` / `ltx` / `image` /
    `misc`) is derived server-side from that architecture (`family_for_arch`) instead of trusted
    from the request. And a user-added LoRA was **invisible**: additions were filed under a
    `custom` group that no group list contained, so the row was counted and never drawn — the
    manifest now synthesizes it.
  - **A manager, not a list.** Recipes open in the same form that creates them (**Form view**),
    export to a shareable `.json`, and import back through the validated
    `library_finetune_import` path. The LoRAs folder gets its own inventory (`Refresh files`) that
    lists every weight on disk — catalog-known or not — and deletes one without a trip to Explorer,
    confined to the LoRAs root (canonicalized, `..` refused) and always confirmed. **Check for
    updates** now also chains the repos behind your own additions, and reports the ones it could
    not reach instead of silently skipping them.
  `tests/adult-pack-add-and-manage.test.js` pins the six paths its header names.

- **Six ways the Pack could silently do the wrong thing.** A review pass over the panel where the
  visible result disagreed with what actually happened:
  - **"Apply updates" only wrote pins.** It called the guarded download wrapper from inside another
    guarded action, and the busy guard is re-entrant-hostile: the nested call returned immediately,
    so the new pin was recorded and nothing was ever fetched. It now calls the unwrapped fetch
    worker, and the test fails if a `doFetch` call reappears there.
  - **A HIGH/LOW half is pinned as its own file.** A `#low` id resolves its parent recipe and uses
    the parent's `#low` pin key, so the second half verifies against its own hash and size rather
    than the first half's.
  - **A user-added file keeps a safe basename.** The stored name is the URL's basename — checked
    again by `safe_file_name` — never the repo path.
  - **CivitAI downloads use the file's real name and the hash CivitAI published.** The endpoint is
    not a file name, and CivitAI's `sizeKB` is only a fallback for entries with no digest.
  - **The source-page action cannot open a non-https scheme** — the raw page string never reaches
    `openExternal` unvalidated.
  - **The finetune manager stops listing a pack recipe twice** by stripping the `adultpack_` prefix
    while de-duplicating; user additions are found by fetch, recipe-write and remove through one
    `find_merged` helper, so adding one is not a second-class path.

- **The Pack's downloads are a queue you can watch, not a wall of log lines.** *Download selected*
  fetched sequentially and reported per file into the console, so "what is left?" meant reading
  backwards through a scrolling log, and a batch of seven files looked identical on file 2 and file
  7. The Downloads section now grows a small queue panel — one row per ticked item with its own
  state (`queued` → downloading `1.2 GB / 3.0 GB` → `verifying…` → ✓, or **✗ failed**, or
  **– not attempted**) under an `n / total done` header — and the progress line above the log gains
  the same `[3/7]` position. The backend stamps every progress event with `queueIndex` /
  `queueTotal` through one `QueueSink` wrapper — deliberately not `index` / `total`, because
  `total` is already the byte count of the file being downloaded — and each item emits an explicit
  `start` phase so its row appears before the first byte arrives. Rows are keyed by the *selected*
  item, so a HIGH/LOW recipe's two files fold into the one row you ticked, and only an in-flight
  batch is folded into: CivitAI downloads and the update-check probe share the same event channel
  and could otherwise rewrite a finished run's rows. `Fetch missing only`, *Apply &
  download* and every other fetch path drive the same queue, and a batch from an earlier visit is
  cleared on open instead of left looking current. `tests/adult-pack-actions.test.js` pins the
  position fields, the `start` events, the row keying and the in-flight gate.

- **An installed, hash-matching DLSS 5 component is green again — an upstream update is news, not a
  fault.** The 0.10.14 audit made a row's SHA a real check, but it also tinted a correct row amber
  whenever the read-only probe found a newer build upstream, so a fully installed panel still read
  as "something is wrong" — and for two components that is permanent, by design: upstream's own
  `docs/DLSS5.md` keeps `renodx-dlss5 4.70` and `dlssnr 310.8.SF-v2` pinned to the versions it
  tested ("newer development builds are announced in the official RenoDX Discord"), while DLSS
  Super Resolution / Frame Generation are NVIDIA-signed builds WanGP has not validated (`310.9.16`
  is live upstream; the installer pins the tested `310.8.0` / `310.7.0`). Green is now local truth
  only: the dot follows *verified* / *matches what WanGP pins*, amber stays for a file that exists
  but was never hashed, red for missing or wrong bytes. The `↻ upstream … — update Wan2GP` note
  keeps its wording but is a muted span beside the verdict rather than a colour on it, and it
  appears **only after you press Check for updates** — the startup TTL probe still refreshes the
  data silently so the button answers instantly, it just no longer decorates a healthy panel on
  launch.

- **DLSS 5's Force setting is remembered across restarts.** It is not a per-run toggle: upstream's
  installer keeps matching files and, when *different* files are already present, changes nothing
  until you rerun `install_dlss5.bat -Force` (which backs up every file it replaces) — so losing
  the tick on the next launch silently changed what an install would do. It now persists as
  `dlss5.force` in the launcher config and is restored on load. The install modal's consent tick
  (*I accept the copyright, licensing, and security risks above*) is deliberately **not**
  remembered, because consent is per-install.

- **The Pack's model list is workable in bulk now.** A hundred checkboxes you have to tick one at a
  time is not a selection UI. A filter bar narrows the catalog by text, by *selected only* and by
  *not downloaded*, and **Select shown** / **Deselect shown** act on exactly the filtered rows — so
  "download everything I don't have yet" (filter → *Select shown* → *Download selected*) is two
  clicks. The panel's tooling also sits in four flat sections — *Find models*, *Downloads*,
  *Finetune recipes*, *LoRA files on disk* — rather than one long scroll, and the CivitAI source is
  labelled *civitai.red*, the host the adult catalog actually publishes on (`civitai.com` became the
  SFW library in April 2026) — the same token works on both.

- **A DLSS 5 verdict now survives a restart.** *Check compatibility* hashes a couple hundred MB, so
  its answer was good for one session only: the panel came back every launch with every row reading
  *expected*, and the one thing it had learned — that these bytes are still the bytes it hashed —
  was discarded. Comparisons are now cached in `dlss5-verified.json` in the data dir and a new
  read-only `dlss5_cached_state` paints them on open, so the ticks are there before you ask. A
  cached row is revalidated against the pin, the file's size **and** its mtime first: a re-download,
  an installer run or a hand-copied file drops straight back to *expected* rather than showing a
  stale tick. The upstream update probe's last answer is cached alongside it, but the
  `↻ upstream` note still waits for **Check for updates** — a cached answer is data, not a paint.

- **Launch credentials no longer sit in a batch file on disk.** Starting Wan2GP in a terminal wrote
  `HF_TOKEN` / `ANTHROPIC_API_KEY` into `%TEMP%\wan2gp-terminal-*.bat` as literal `set …` lines, so
  a crash — or a shared machine — left your HuggingFace and Anthropic tokens in the temp directory
  in plain text. They are now staged in a separate `wan2gp-env-<millis>.txt` that the script reads
  with `for /f` and deletes *before* the server starts, and `sweep_terminal_temp_files()` clears
  both `wan2gp-terminal-*.bat` and `wan2gp-env-*.txt` on stop and at startup, so an interrupted run
  leaves neither behind.

- **The last five blocking commands moved off the invoke thread, and the backend shed its dead
  surface.** `python_preflight` (spawns uv plus a Python probe), `auto_tune_detect` (`nvidia-smi`
  and the registry), `llm_engines_list` (up to three cold `pip show` calls), `workspace_backup`
  (`Compress-Archive`, which can run for minutes) and `troubleshoot_triton_repair` (a pip reinstall)
  all ran synchronously on the IPC thread, so the window could not repaint while they worked; each
  now wraps a `_blocking` twin in `spawn_blocking` and the UI stays live. In the same pass six
  commands with no surviving caller were deleted — `greet`, `check_python`, `check_git`,
  `open_external`, `popout_webview`, `preflight_check` — with their registrations and their dead
  frontend listeners (`onSetupProfile`, `onBvNavState`, `onTermDockChanged`, `onOpenMigration`,
  `onSystemThemeChange`, `onBvCrashRecovered`), and the dependencies only they needed left with
  them: `tauri-plugin-fs`, `base64` and `image` (plus the `windows` crate), and the unused
  `updater:default` ACL entry. The exe is smaller, the event bus no longer advertises events nothing
  emits, and nothing in `src/` or `src-tauri/` still references a removed path.

- **Frontend hardening, and a dev-harness path that could not escape its root.** The Library panel
  assumed every backend list it received was an array, so one malformed payload threw and took the
  whole table with it — `folders`, `items` and `sample` are now guarded (an absent list renders as
  the empty table it is). The guide, safety and Deepy-checks panels dropped their private `esc()`
  copies for the one shared `window.escHtml`, and the harness stub now answers with the real payload
  shapes (`library_models` → `{root, files, bytes, truncated, sample}`, etc.) so those guards are
  actually exercised instead of passing by luck. The dev server's traversal guard used
  `file.startsWith(ROOT)`, which also accepts a sibling directory (`src2/…` beside `src/`); it now
  compares `path.relative(ROOT, file)` and rejects `..` and absolute results. First-open hardware
  probing also retried unconditionally after 2s — it now retries only when the first probe came back
  empty, so a slow-but-successful probe is not run twice.

- **The release script can no longer sign and publish the wrong artifact, and CI now runs the
  suites.** The v0.10.14 flow fell back to "the newest `setup.exe`" when the versioned one was
  missing — exactly how a stale installer gets published under a new version. `scripts/release-tauri.ps1`
  now requires a versioned `*-setup.exe` **and** its `.sig` **and** a versioned `.msi`, runs
  `npm test`, `cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` as a
  hard preflight that fails the run, refuses a detached HEAD, uncommitted tracked changes, or a
  version that is not strictly greater than `package.json`'s, bumps all five version files together
  (`package.json`, `package-lock.json`, `tauri.conf.json`, `Cargo.toml`, `Cargo.lock`), refuses if an
  existing local or remote tag does not point at HEAD before pushing, publishes as a **draft** and
  only then un-drafts it, and refuses outright if the release already exists. `scripts/live-tests.ps1` now
  fails when its filter matches **zero** tests — `cargo test` exits 0 with none, which used to read
  as a pass — and a new `.github/workflows/ci.yml` runs `npm test`, `cargo test`, clippy and
  `fmt --check` on `windows-latest` for every push to `master` and every PR, with the Rust build
  cached.

## [0.10.14] — 2026-10-19

- **The metrics row can no longer clip VRAM off the end of the topbar.** The row is a flex item, and a tight bar made it shrink its own container and hide its overflow — which cut the **last** metric, so the VRAM graph (the one you watch while a generation runs) disappeared behind a half-drawn *VRA* exactly when it was useful. It now drops whole, lower-priority readings in tiers instead: first the CPU/GPU/RAM sparklines, then RAM and GPU2, then CPU/GPU, then the VRAM graph, and only then the cluster itself. Nothing is ever cut through a label. The embedded Wan2GP view costs ~330px more (*Back to Dashboard*, *Console*, the zoom slider), so it runs the same ladder one rung higher behind a `wv-active` class on the dashboard; VRAM keeps its graph longest in both views. The thresholds are measured in a browser at the widths where each rung was still clipping (the bar wants ~1440px with every sparkline, ~1550px with a second GPU), and `tests/topbar-metrics-fit.test.js` pins the ids, the protected class and the rung order.

- **uv is provisioned without ever editing your PATH, and `uv self update` only runs when uv's own receipt names our copy.** uv's standalone installer ships `"modify_path": true`, so every install run appended its install dir to the **user's** persistent PATH; where that dir was a throwaway `%TEMP%\.tmpXXXX\.tools` prefix the litter accumulated — one box carried **104 dead PATH entries** (6565 chars of PATH) and **1.65 GB** of duplicate `uv.exe` copies in `%TEMP%`. Both installer runs now pass `UV_NO_MODIFY_PATH=1` (the tools-dir install and the step-3b repair path), and self-update is gated on uv's **own** install receipt (`%LOCALAPPDATA%\uv\uv-receipt.json` → `install_prefix`) naming our `.tools` dir. The `.launcher-uv` marker alone is not enough: a marker left by a run whose receipt points at `%TEMP%` makes uv refuse with *"…the standalone installer was used to install uv to <other dir>. Are multiple copies of uv installed?"* — noise before every reinstall, and it was the reason for that error in the console. New `scripts/cleanup-uv-path.ps1` clears the historical litter on boxes that already have it (dry run by default; backs up the raw `HKCU\Environment\Path` value and preserves its `REG_EXPAND_SZ` kind; `-DeleteTempDirs` also frees the orphaned trees; writes via `reg.exe` because the .NET registry write is refused on hardened boxes).
- **The DLSS 5 card can now tell you whether a newer component exists.** The panel printed the pinned versions and checked that the files were present, but nothing answered "is a newer one out?" — the versions come from upstream's `scripts/install_dlss5.ps1`, whose package table hardcodes six version-pinned URLs with their SHA-256s, so a newer DLSS / ReShade / RenoDX build can only become installable by updating Wan2GP. The new **Check for updates** button (read-only) asks the same sources that script pins — `RankFTW/rhi-repo` for DLSS Super Resolution, Frame Generation, DLSSNR and RenoDX, `DeepBeepMeep/dlss5-visual-enhancer` for the workers, and reshade.me's own download links for ReShade — and reports per component: *up to date with the pinned build*, *newer release upstream — update Wan2GP to pick it up*, *a different build upstream* for a tag the launcher cannot order against the pin (`310.8.SF-v2` → `310.8.SF-v3` is reported as differing, never as newer), and *not checked* when a source is unreachable. The upstream tag is shown beside the version — that is what tells a same-base flavor (`dlssnr-310.8.Lecram`) apart from a real version bump — and an `-rc` upstream is labelled a release candidate, since upstream pins stable builds. It downloads nothing, installs nothing and never moves a pin. The compared-against versions are read out of the existing `DLSS5_FILES` manifest, so the pins keep exactly one home, and a unit test fails if a probe row ever loses its manifest counterpart.
- **DLSS 5 probe detail no longer reads "exit exit code: 2".** `ExitStatus`'s own `Display` already renders `exit code: 2`, so the extra `exit ` prefix is gone and a failed frame-generation probe reports its exit status once.
- **Detect no longer blanks a Performance Settings dropdown it has no opinion on.** The Auto-Tune *recommend* pass re-seeds the dropdowns from Detect's payload and wrote `""` for every key that payload did not mention — and `ram_allocator` is deliberately not one of its members, because upstream already defaults to `mmgp`. The panel auto-runs Detect on the first Auto-Tune open of each session, right after `memProfileLoad` seeds the dropdowns from `wgp_config.json`, so Detect's clear always landed last: **RAM Allocator** sat on *— unset —* with `saved: MMGP Optimized…` beside it, which reads as "the launcher forgot my setting" even though the key was on disk and unreachable-by-Apply the whole time (an empty dropdown means "leave the key alone", so nothing was ever lost either). Every manual **Detect** and every **Failsafe** toggle repeated it, and it also silently reverted a pick made in that dropdown. The recommend pass now overwrites only the keys the recommendation actually carries — the backend omits the ones it has no opinion on (`features.rs` merges `extra` only when applicable), so `undefined` is the signal — which also stops the blanking of `attention_mode` / `compile` / `vram_allocator` / `compute_cache` / the preload modes on hardware where upstream recommends nothing. `tests/mem-seed.test.js` covers both directions.
- **A verified wheel now shows its SHA-256 and whose digest it matched.** The gate already refused a mismatched wheel, but on success it printed only `sha256 pinned, verified` / `sha256 (GitHub digest) verified` — the hash the launcher computed was never shown, so "verified" was a claim you had to take on faith, and it did not say *whose* checksum it was. Every path now prints the evidence under the wheel name: `sha256 <64 hex> — matches the sha256 deepbeepmeep/WGPKernels published for release v1.1.2 (launcher pin)`, or `… — matches the sha256 GitHub publishes for nunchaku-ai/nunchaku (release v1.2.1)` when the launcher has no pin and the digest came live from the release API. The publisher is named from the wheel's own URL, because not every kernel wheel is deepbeepmeep's — Nunchaku and SageAttention ship from their own repos — so the line always credits the repo the hash actually came from. A **cache hit re-hashes the file it is about to install** and prints the same evidence rather than skipping quietly, a cached file that no longer matches says so with the expected hash before re-downloading, a mismatch failure names the source beside both hashes, and a wheel with nothing published to compare against says that too (`sha256 <hash> — nothing published to compare against`) instead of implying a check happened. The two non-wheel gates follow: the Git for Windows and Python installers print `[✓] sha256 <hash> — matches <source>` as well, where they used to verify silently.
- **The launcher's own plugins lead the plugin list.** The community list is alphabetical, so *Finetune Manager · by GKartist75* sat behind File Gallery and H3 Director — the author's own entry buried by name in the author's own plugin manager. Plugins authored by the launcher (matched on the entry's `author` **or** its repo URL, case-insensitively, because a catalog entry carries `author` while a locally cloned plugin may only carry the URL) now sort directly under the **COMMUNITY PLUGINS** header, ahead of the alphabetical run and ahead of whichever column sort is active — *Name*, *Latest* or *Created by* — since "on top" should not depend on which sort you last clicked. The group boundary is untouched: community still leads system, so one of ours in the system set could not jump above the community group. A small 📌 marker (hover: *"GKartist75 is the launcher's author — this plugin is kept at the top"*) sits on the pinned row so the order does not read as a sorting bug. `tests/plugins-order.test.js` covers the pin, the untouched remainder, the case-insensitive author/URL match, the group boundary and all three sorts.
- **The Update button no longer prints its own duplicate summary.** `update()` streams every human-readable line through `launch-log` into the dashboard console — the `[✓] Wan2GP update complete — now at upstream <sha>; N personal file(s) left untouched` line, the `[=] Already at upstream … — nothing to update` no-op line, the requirements reinstall line and its pin diff, the `[✓] dependencies match requirements.txt (N pinned)` verdict, the failure line, the drift line, and each compat note. The dashboard click handler restated four of those ("`[*] Wan2GP update complete`", "`[*] requirements.txt changed — pinned packages reinstalled`", the drift line, the compat line), so one update printed each fact twice in a row (visible as a duplicated tail on every 0.10.13 update). The handler now only does what a log line cannot — the drift banner and its toast, the compat toast, and a dashboard refresh.
- **The DLSS 5 card's per-file SHA is now a real check, not an existence badge.** `dlss5_status` only stats the eight files, so every installed row printed `✓ SHA <pin>…` — a tick and a hash beside a file whose bytes were never compared with it (the wheel rows on the other panels print a hash the launcher computed *and* matched, which made this the one claim you had to take on faith). **Check compatibility** now hashes every file and compares it against the same `DLSS5_FILES` manifest the rows print, through a new read-only `dlss5_verify_files` command: each row becomes *✓ SHA … matches what Wan2GP pins*, *⚠ SHA … expected — this file hashes to …* (both digests named), *missing*, or *could not be read*. A row that has not been checked says **expected — press Check compatibility** instead of claiming a match, and the installer's own verified line still counts as a comparison while an install is running. Hashing the set streams a couple hundred MB, so it runs on the blocking pool and only behind that button — never on a plain panel open — and a fresh status read clears stale verdicts. `tests/dlss5-verify.test.js` pins the wording rules (no tick without a comparison, digests truncated) and the Rust classifier stays pure and tested (`dlss_file_state`), including that verification is read-only and leaves the files untouched.
- **A Rust panic now leaves a crash log instead of a silent process death.** The release profile is `panic = "abort"`, so any unexpected panic killed the process with no window, no toast and no log — and the console history is in memory, so it died with it and left nothing to report. `run()` now installs a panic hook (the last code that runs before the abort) that appends `[<timestamp>] panic at <file>:<line>:<col>: <message>` to **`<data folder>\launcher-crash.log`**, then chains to the default hook so a dev run still sees stderr. A crash left by a previous session is surfaced **once** in the console on the next launch (*"the previous launcher session ended with a Rust panic — full log: …"*) and the file is rotated to `launcher-crash.previous.log`, so a stale file cannot make every later launch read as a fresh crash while the details stay on disk for a bug report. `base::panic_log_tests` covers the record format, append-not-truncate, and report-once-then-rotate.
- **"Safe SageAttention build" is a real choice again, not a rule that only knew one historic pin.** The toggle recognized exactly one URL — upstream's `…torch2.9.0andhigher.post4` pin — so the moment upstream moved to the fixed `post6` the setting became a switch that changed nothing, while the card still promised it would act on the next sync. The rule is now the wheel *family*: with the toggle on, any `woct0rdho/SageAttention` Windows wheel in the safe build's `cu130torch2.10` line that differs from the launcher's known-good build is substituted, and the console names what upstream pinned and what was installed instead (with a pointer to uncheck the toggle); a different torch line, a PyPI spec (Turing's `sageattention==1.0.6`) or the toggle off leave upstream's pick exactly as published. Both paths — the Sync-time override and the post-install pass — now go through one pure predicate, `sage_needs_safe_swap`. **Today this is deliberately quiet**: the live `setup_config.json` pins `v220_cu13` to the byte-identical `post6` wheel, so nothing installs differently until upstream ships another build in that family — and the card's hint now says exactly that instead of promising a swap that no longer happens. Tests cover the historic post4, a future post7, the safe build itself, another torch line, a PyPI spec, and toggle-off.
- **The heavy, user-initiated commands no longer run on the thread that asked for them.** Tauri executes a command on the invoking thread unless it is declared `async`, so the longest one-shot jobs — `repair_settings` (four config walks), `report_issue`, `troubleshoot_debug_bundle`, `detect_electron`, `library_loras`, `library_finetunes`, `workspace_list`, and first-open `plugins_list` — could hold up the UI while they worked. All eight are now `#[tauri::command(async)]`: an attribute-only change, the bodies are untouched and the results come back the same way. The frequently-polled `get_status` / `get_system_metrics` stay synchronous on purpose — they are already cache-throttled and their timing is worth more than the move — and a comment at the handler list records the policy.
- **Renderer errors after boot now say something instead of nothing.** `window.onerror` / `unhandledrejection` painted only the hidden `#splashError` element, which explains a blank *startup* screen — but once boot is past the splash is gone, so a handler throwing or a promise rejecting surfaced as "nothing happened", with the trace in an element nobody sees. Both channels now also append one line to the console (`[!] renderer error: <first line>`), de-duplicated so an error thrown every frame cannot flood the log, and tolerant of `appendLog` not being loaded yet. `tests/renderer-errors.test.js` runs the real helper in a vm for the splash write, the single console line, the de-duplication and the missing-`appendLog` path.
- **Dead code, and the build warnings it caused, are gone.** `base::reset_dir_caches` (test-only — it warned "never used" in every release build), `parse_requirement_pins` and `format_pin_line` (test-only wrappers, now `#[cfg(test)]` with a note naming what production uses instead: `parse_requirement_pins_for` and `pin_drift_entry`), and the unreachable `on_system_theme_change` command (never registered, never called — the real path is the `system-theme-changed` event) are cleared. `cargo build --release` is warning-free again.
- **Live tests have a runner: `scripts/live-tests.ps1`.** The default `cargo test` has to pass offline, so the one test that proves wheel verification against a real server — `wheels::tests::live_download_verifies_and_caches`, which downloads the smallest pinned wheel and walks download → sha256 → pin match → console evidence → cache hit — is `#[ignore]`d, and `cargo test -- --ignored` is easy to forget *and* easy to get subtly wrong (the filter goes before the `--`, and the evidence lines only appear with `--nocapture`). The script wraps it with the right argument order and runs one test at a time so the evidence is readable; it is the thing to run before a release, or whenever a pin should be re-proved against the server.
- **The kernel-sync console lines are now pure, tested formatters — and a test that had silently never run is running again.** The lines a *Sync* prints (the `setup_config.json @ <sha>` header, `already at <version>, skipped`, the triton no-downgrade notice and the completion summary) were loose `format!`s inside the sync loop, so the wording a user reads after every Sync — and quotes in a bug report — had no test behind it. They are four small pure helpers now, and `install::sync_transcript_tests` pins them against a real all-up-to-date run captured from the console (RTX 3080 · `env_uv` · upstream `fdc63bdf`): the four `already at … skipped` rows, the triton line, and `0 synced, 4 already current, 0 failed`. The sage row of that transcript is the visible proof that the safe-build toggle is doing nothing **on purpose** — upstream's pin and the known-good build are the same `post6` wheel, so nothing is substituted and the kernel is reported current rather than reinstalled for no reason. While writing it, `wangp_git_tests::changed_path_count_ignores_blank_lines` was found to be missing its `#[test]` attribute — compiled but never executed since the upstream-v17 parity commit (`8bdab92`), with only a `never used` build warning as a hint — so that assertion is live again.

- **Confirmations actually ask again — and can say no.** `tauri-plugin-dialog` 2.7.x injects an init script that replaces `window.confirm` / `window.alert` with *async* shims invoking `plugin:dialog|confirm` / `plugin:dialog|message`; the plugin registers only `open`/`save`/`message` and this app grants no `dialog:` permission, so the invoke was rejected (`Command plugin:dialog|confirm not allowed by ACL`, which the renderer-error channel added above surfaced in the console). The async half was the real damage: `confirm()` returned a Promise, `if (!confirm(…))` was therefore always false, and **every confirmation in the app fell through** — *Restore GPU Wheels*, *WGPKernels (INT8)*, *Uninstall environment*, *Recreate environment*, *Start install*, *Roll back*, *Repair files*, *Delete finetune*, *Restore config*, *Remove engine* and both *requirements reinstall* prompts ran without asking, while all twelve `alert()` calls were silently swallowed. Every one of the 26 call sites now goes through two helpers that use the launcher's own `confirm_dialog` command (native dialog, no ACL): `askConfirm(message, title)` resolves a real boolean (anything but `ok` — including a bridge failure — refuses) and `notifyUser(message, title)` is a fire-and-forget notice that cannot throw from a `catch` block. `tests/native-dialogs.test.js` fails if a bare `confirm(`/`alert(` ever returns, exercises the `ok`/`cancel`/`undefined`/`''`/throwing paths, and asserts the helpers load before the tabs that use them.

- **The topbar status cluster is a 2 × 2 grid, so the controls beside it fit again.** *Running*, *INT8 · WGPKernels*, *FP8 · Triton* and *Deepy Web* ran as one inline row beside the title; at four items wide it ate the bar and squeezed the title and the right-hand controls on narrower windows. The four now sit in a compact grid — *Running* / *Deepy Web* on the first row, the two backend chips spanning the second — which is **203px instead of 364px**, 44% narrower, and leaves the original 44px bar height untouched (the grid is 25px, with 9px clearance above and below). Measured in the browser engine: the inline row started clipping at **720px** of window width, the grid holds down to **580px**. Hidden items are `display:none` and therefore create no cell at all, so a stopped server or an unreported backend collapses the cluster instead of leaving a hole; the chips keep their own dots and their green/amber/grey states. `tests/backend-chips.test.js` now pins the 2 × 2 structure and the row order.

## [0.10.13] — 2026-10-18

- **AMD GGUF (HIP) installs in the launcher's own env again.** Upstream replaced the HIP wheel's `torch210rocm714py311` build with `torch213rocm100py312` (gguf-v1.0.27) — a ROCm 10 / torch 2.13 / Python 3.12 build that records and checks the exact ROCm torch build at import. The launcher had no `gguf_rocm10` handling at all, so *↻ Update GPU Wheels* resolved the cu130 wheel on AMD, and the GGUF floor's anti-swap guard no longer recognised the new HIP tag. Sync now maps `gguf` → `gguf_cu128`/`gguf_rocm10` from the live env (upstream `setup.py`'s `(torch, python)` table, RDNA4-gated), the Active Environment overview compares against that same component, and the separate ROCm 7.14 environment is **retired** — the **AMD setup (exp)** recipe and its button are gone, because the HIP wheel belongs in this env. Upstream's own HIP validation is still pending, so the button stays labelled experimental.
- **GGUF kernels 1.0.27 (parity floor).** Upstream's 1.0.27 wheel reworks the llama.cpp row loop into a single launch — one CUDA-graph kernel node and one memset node instead of a per-row launch, with per-shape first-use tuning — for **2–10% faster decode** (5–19% once a bias is added) and **lower VRAM**. Everything below 1.0.27 now flags stale and re-syncs; the three components are version-only bumps (`gguf` cu130/py311, `gguf_cu128` cu128/py310, `gguf_rocm10` rocm10/py312). HIP gets none of the row-loop win yet — upstream's `linear_chunked_supported` is still False on ROCm.
- **Model inventory reconciled to 272** (v18.19 / `22d1f261`): **MOSS-TTS v1.5** from OpenMOSS in two sizes — 8B and Local 4B — with up to three cloned voices, dialogue with adjustable pauses and optional script writing (Local 4B outputs 48 kHz stereo), plus the named **Krea 2 Edit v2** ids (`krea2_turbo_general_edit_v2`, `krea2_turbo_identity_edit_v2`) now spelled out in the guide.
- **Settings follow v18.19's renames.** `read_ahead` is retired upstream — the read-ahead copier is now chosen automatically — so the *Read Ahead (Windows)* toggle is gone, the launcher no longer writes or reads the key back, and a legacy value already in `wgp_config.json` is simply ignored. The per-task `override_profile` / `override_attention` / `override_compile` trio collapsed into one `overrides` string (`name=value, …`); the launcher never wrote those keys, so this is a docs/settings-reference fix (the H3 Veda defaults now ship `overrides: "attention_mode='veda'"`). The guide's 118-key list is exact again — `num_inference_steps2`, `video_guide3` and `audio_guide3` were missing, and `model_type`/`settings_version`/`keep_intermediate_sliding_windows` are `SETTINGS.md` labels, not stored keys.
- **Topbar kernel pills get status lights.** The `INT8 · <backend>` / `FP8 · <backend>` chips beside *Running* now carry a dot like the Running LED: **green** when WanGP is using an accelerated backend (WGPKernels / Comfy Kitchen / Triton / CUDA FP8 tensor cores), **amber** when it fell back to PyTorch — a valid state, but it shouldn't glow like a fast path — and grey until the boot log reports. Mixed FP8 lines (`Triton, PyTorch for scaled FP8 checkpoints`) count as a fallback.
- **Kernel wheels are SHA-256-verified before pip sees them.** GitHub release tags are not immutable — an asset can be re-uploaded under the same version — and pip was handed a bare URL over TLS. *Update GPU Wheels* (Sync, the post-install override pass, and the WGPKernels/HIP-GGUF buttons) now downloads the wheel, hashes it, and installs the **local file** only once it matches either a launcher **pin** for that exact asset — 16 of them: GGUF 1.0.27 (cu130/cu128/rocm10), WGPKernels 1.1.2 (cu130/cu128/rocm100), Nunchaku 1.2.1 + 1.2.0, LightX2V 0.0.2, SageAttention 2.2.0 (safe build + upstream cu128/cp310+cp311), SpargeAttn post3/post4, FlashAttention 2.8.3/2.8.4 — or, for a version the launcher has not pinned yet, **GitHub's published `sha256` digest** for that asset. So an upstream version bump still verifies without a launcher release, and a re-uploaded or truncated wheel fails loudly: both hashes are printed and that component installs nothing. When neither source is available the install proceeds on TLS and logs the hash it saw (the same stance the Git/Python installers take). Verified wheels are cached in **`<data folder>\wheel-cache`** beside `triton-cache`, so a re-sync re-hashes a local file instead of re-downloading up to 250 MB; the folder is a disposable cache. A unit test keeps the pin table in step with the URLs the launcher generates, so a version bump cannot silently downgrade the gate.

- **Agent API (MCP) parity — `2.1` by default, plus the web-UI and OAuth flags.** The *Agent API (MCP server)* helper (Manage → Launch) only knew `--mcp-api-version 2`/`1`. Upstream's choice list is `1 | 2 | 2.1` with **2.1 the default**, so the dropdown now defaults to 2.1 (`2` and `1` stay for older clients); the panel gained the **`--mcp-webui` / `--no-mcp-webui`** autostart override — which switches the generated command from a standalone `--mcp` server to a `--listen --mcp-webui` web launch, because that flag overrides *Configuration → Agents → MCP autostart* for the normal web UI — and the **OAuth** fields (`--mcp-auth`, `--mcp-auth-url`, `--mcp-auth-password`). The hint under the command now names the real connect URL and warns when a non-loopback host is exposed without OAuth, or when non-loopback OAuth still needs `--mcp-auth-url`. The builder moved into a pure, unit-tested `services/mcp-command.js`.
- **Sync / install can no longer hand a wheel to the wrong backend.** Four AMD holes closed: *Update GPU Wheels* used to fall back to the **cu130** WGPKernels wheel — and the CUDA **gguf** wheel — whenever the live torch probe failed or reported a legacy/unknown build, so a ROCm box could be handed a wheel that cannot import; it now skips loudly (NVIDIA profiles still imply cu130). The HIP-GGUF button pip-installed its fixed `cp312` / torch 2.13 / ROCm 10 wheel into **whatever AMD env was active**; it now checks the env's torch build and Python first and refuses with a pointer to *Update GPU Wheels* instead. The Active Environment overview applies the same RDNA4 gate Sync uses for `gguf_rocm10`, so a non-RDNA4 AMD box no longer shows a phantom "want" that can never clear, and it reads the shared `AMD` profile for the Sparge/Flash/Triton/Sage rows instead of "—". `%2B` build-tag decoding is case-insensitive, and a stray `torch212` clause in the GGUF anti-swap guard (it could exempt a future CUDA wheel) is gone.
- **Sync covers every profile's SageAttention, and a failed install leaves no env behind.** The SageAttention step was hardcoded to RTX 30–50; it now follows the profile's `sage` tag — RTX 20 and AMD `v1` are a pure-PyPI `sageattention==1.0.6` (a spec, not a URL), still resolved through the shared `AMD` profile. The conda "no system Python" early return restores `PATH` and clears the `WAN2GP_TAURI_*` / `PYTHONUNBUFFERED` env it sets, and `install` validates the env type up front instead of letting an unknown string skip the smoke test while still writing the completion marker.

## [0.10.12] — 2026-10-17

- **Upstream v18.18 parity (fde3c7e + wave) — the Bitdefender/Triton fix.** Upstream ships a **prebuilt CUDA helper** in-repo for `triton-windows 3.7.0.post26`–`3.7.1.post27`, checksum-verified and already compiled (CUDA: Windows/Python 3.11; HIP: Python 3.12 + rocm-sdk 7.14, experimental) — no JIT helper compile, no TinyCC/MSVC step, no antivirus exclusion needed on those stacks. The launcher mirrors it:
  - **Triton pins** – the recommended line is now Triton 3.7 Windows (`>=3.7.0.post26,<=3.7.1.post27`) for RTX 30–50 (hardware overview, dashboard spec rows, User Guide; the raw key rename is honored — `v37` = 3.7, old `v37` 3.6 spec lives on as `v36`), and the AMD installer pins `v38`'s exact range instead of floating `>=3.7,<3.8`. Triton 3.6 stays supported for existing installs (the cache pin, Repair, Roll back to post24 and the opt-in MSVC pin remain as fallbacks for un-helpered builds — the User Guide says exactly where each still applies).
  - **GGUF kernels 1.0.25 → 1.0.26** – docs-prescribed floor raised; stale pins swap to the 1.0.26 wheels (cu130/cu128/HIP — HIP is a version-only bump). 1.0.26 adds packed rows crossing quantization blocks, fixing MiniMax H3 image-reference projections; everything below 1.0.26 now flags stale and re-syncs.
  - **Model inventory reconciled**: the guide's catalog was 32 defaults behind the live upstream state (the v18.00–v18.18 waves) — now 270 and complete again, with the new **3D / world reconstruction** section (Hunyuan3D 2.1, TRELLIS.2, TripoSG/TripoSplat, PartCrafter, WorldMirror 2.0, MapAnything, Depth Anything 3, MoGe-2, GeoSAM2, AnimateAnyMesh), **Kandinsky 6** (Pro 30B / Lite 3B, synced soundtrack), Wan 2.2 **Prism** and camera control (Fun Control-Camera, Uni3C), H3 **Veda sparse** + **ControlNet-Union** families, LTX-2.5 **Alpha Gen**/**Layout to Render**, **MOSS Image 5B**, **Ming Image 0.1 Design(+Layer)**, Qwen Image 2.1 Turbo and the Krea 2 v2 edit presets.

- **Prebuilt-helper integrity check on the dashboard.** The helper binaries (`shared/triton_runtime/*.pyd`) are unsigned binaries — prime antivirus-quarantine targets, and a stripped one silently falls back to the JIT path with no other symptom. The Active Environment panel gains a **Prebuilt Helper** row that verifies presence and hash (anchored to each manifest's own `binary_sha256` — zero drift when upstream revises the builds) of both helper manifest+binary pairs and the `__init__.py` engager on every dashboard refresh: green "intact", red "missing — AV quarantine suspected" or "modified — reinstall Wan2GP", with per-file detail on hover.
- **uv self-update noise removed.** "Ensuring Python" fired `uv self update` unconditionally; on copy-fallback binaries (no install receipt) that always errors with "Self-update is only available for uv binaries installed via the standalone installation scripts…" — confusing boot noise before every reinstall. The attempt now runs only on receipted (markered) launcher-owned installs; outdated no-receipt copies still have the official-installer recovery path, and a foreign PATH uv is never touched.
- **Update GPU Wheels now syncs Triton.** Upstream tags Triton per profile outside the `kernels[]` list (`gpu_profiles[profile].triton` → `components.triton[v37|v38|…]`), so the sync loop never included it — a fully updated install stayed on Triton 3.6 and the prebuilt CUDA helper never engaged on existing launches. Sync now resolves the profile's Triton tag through setup_config (AMD sub-profiles land on the shared `AMD` `v38` tag), strips the spec's shell quoting, and reinstalls with `--upgrade`; the no-downgrade guard keeps newer working builds untouched, and an already-satisfied range re-resolves without downloading. Untagged profiles (GTX 10, CPU/MPS) stay out of the sync list. Fresh installs were already correct — this closes the upgrade path only.
- **Repair/Roll back show their commands.** The Troubleshooting Triton actions echo the exact pip command lines (`[>] <python> -m pip …`) to the console before running them — built from the same pinned argv the spawn uses, so shown and run can never diverge.

## [0.10.11] — 2026-10-17

- **Roll back Triton to post24** (Manage → Troubleshooting): the Repair command takes an optional version; the new button pins `triton-windows==3.6.0.post24` (previous known-good 3.6.x) with the same reinstall-and-clear-and-verify flow. Warns that a later setup floats forward again on upstream's `-U` range spec.
- **AV guidance: try post24 as a diagnostic.** Some engines react per-build, so a flagged post26 may get a different verdict than post24 on the same box — the User Guide now says so and asks affected users to report which build their AV accepts, with the detection name. Not a cure: identical patterns get identical verdicts.

## [0.10.10] — 2026-10-16

- **Repair Triton wheel** (Manage → Troubleshooting → Repair wheel): force-reinstalls the exact installed Triton build (`--force-reinstall --no-deps`, torch and everything else untouched — deliberately not floating to latest, which would break upstream parity), clears both JIT cache homes (renamed to backup), then verifies the import and reports the version. For damaged installs (quarantined or half-written files) — stated plainly in the UI, it fixes corrupt files, not antivirus verdicts.
- **TCC vs MSVC, stated plainly.** Out-of-box stays pure upstream TinyCC. The signed-MSVC pin (Auto/MSVC choice, one-click Build Tools installer, stable cache dir) is a *temporary workaround* for antivirus blocks on Triton's unsigned JIT output — it retires when upstream removes the runtime-JIT need. Documented as temporary in the User Guide so nobody mistakes it for the permanent direction.

## [0.10.9] — 2026-10-16

- **Window mode setting** (issue #63, Settings → Desktop → Window mode): the
  launcher had been hard-coded to open maximized since v0.2.0 and nothing
  could change that — `windowed | maximized | fullscreen` is now a persisted
  setting in `desktop-config.json` (`desktopWindowMode`, default maximized so
  existing behavior is unchanged). Applies immediately on change and on every
  launch: the main window now ships hidden and pre-maximized, setup applies
  the saved mode invisibly then shows it — no maximized→windowed flash on
  either path. Fullscreen is true OS fullscreen (Windows chrome hidden; the
  top bar keeps Settings reachable for switching back).


### Triton JIT hardening (Bitdefender python.exe-to-tcc.exe block)

- **Pinned Triton JIT cache** at `<data folder>\triton-cache` (`TRITON_CACHE_DIR`,
  set-if-absent): Triton's compiled helpers (`cuda_utils`, `__triton_launcher`)
  are unsigned with a fresh hash per build, so heuristic AV quarantines them
  mid-compile. One stable folder replaces `%TEMP%`/`~\.triton` roulette as the
  single AV-exclusion path. **Manage → Troubleshooting → Clear Triton cache**
  now clears both homes (rename-to-backup preserved).
- **Pinned Triton JIT compiler to signed MSVC cl.exe** (NVIDIA/Intel/CPU
  launches; AMD excluded so TheRock clang-cl stays authoritative): without a
  VS shell in its environment Triton falls back to the wheel-bundled unsigned
  TinyCC, whose execution Bitdefender's behavioral engine blocks outright,
  failing every sage/sage2 generation. CC/INCLUDE/LIB come from the newest VS
  plus Win10 SDK via vswhere, all-or-nothing against a user toolchain (a
  VsDevCmd shell always wins), recorded per-process so AMD launches remove
  exactly what was pinned. Warn-only throughout; the console names the pinned
  compiler on every launch.
- **No-MSVC boxes stay fully working** via the TinyCC fallback (same AV
  exposure as before — the pin just can't apply). The console names the
  missing piece and the User Guide carries the one-line Build Tools install;
  the hint text itself is unit-tested so the pointer can't rot.
- **One-click MSVC Build Tools offer** (Manage → Troubleshooting): status line
  shows whether `cl.exe` is present; otherwise a guided button downloads
  Microsoft's official bootstrapper at click-time (nothing compiler-shaped is
  bundled — MSVC can't be redistributed that way, and any third-party compiler
  we shipped would be unsigned anyway) and runs it elevated with the C++
  workload. Returns as soon as the installer starts; Recheck/next Launch
  picks up the pin. Windows-only row, confirm dialog states size/admin/time.
- **Triton compiler choice** (Settings → Desktop, `tritonCompiler` in
  desktop-config.json): out-of-box is pure upstream TinyCC (the MSVC pin is
  a workaround and stays opt-in); explicit `auto` pins MSVC when installed,
  `msvc` forces it (warns when missing), `tinycc` keeps upstream with the pin
  scrubbed. Takes effect on next launch; AMD launches still remove the pin
  whatever the mode.
- **Performance Settings dropdowns reflect disk.** The panel seeded dropdowns
  from saved settings only when one field was empty, so after any Detect run
  keys Detect never recommends (RAM allocator above all) sat on "unset" while
  `saved:` told the truth. Each still-unset dropdown now seeds per-field —
  Detect results and user picks always win, `rec:` tags only ever come from
  Detect. (The setting itself was never lost: `wgp_config.json` holds it and
  WanGP boots with it.)
- **INT8/FP8 live backends moved beside Running.** The Active Environment rows
  sat on "—" whenever the server wasn't running, reading as a forgotten
  setting. The live chips are now topbar pills shown only while a backend is
  known — no server, no pills.

## [0.10.8] — 2026-10-16

### Upstream WGPKernels 1.1.2 (`17307e5`, 2026-10-16)

WGPKernels left manual-install land: v1.1.2 is listed in `setup_config.json`
(RTX 30/40/50 + AMD profiles), so **↻ Update GPU Wheels** installs it —
re-run Sync after a WanGP update, since upstream's *Update* only refreshes
code and requirements.

- **v1.1.1 URLs are dead (404)** — the button, sync and overview now target
  v1.1.2 (`cu130` / `cu128` / new `rocm100`).
- **AMD needs no separate env for WGPKernels anymore.** The `rocm100` wheel
  (ROCm 10, Win+Linux) loads in the installer's torch 2.13+rocm10 env and
  covers RDNA4 gfx1200 (RX 9060/9060 XT) *and* gfx1201 — the retired
  Windows-only `rocm714` wheel did gfx1201 only. Only HIP GGUF keeps its
  torch-2.10 separate env (the **AMD setup (exp)** recipe is now HIP-only).
- Sync mirrors `setup.py`'s per-stack remap (`wgpkernels` / `_cu128` /
  `_rocm10`, RDNA4-gated) and resolves AMD sub-profiles through the unified
  `AMD` key, with a no-re-download skip guard.
- INT8 wording follows upstream's new labels: WGPKernels "recommended,
  precise", Kitchen "fastest, least precise".

## [0.10.7] — 2026-10-16

### Upstream v18.11 parity (`5290e89` → `a827cf2`, 2026-10-15/16)

WanGP v18.11 adds Flux 3 Klein and Fish Audio Drama 3, an mmgp CPU-expert
rework, split WGPKernels releases and new kernel enums.

- **WGPKernels is installable (optional).** Upstream's `int8_kernels = auto`
  now ranks WGPKernels > Kitchen > Triton > PyTorch, but the wheel is manual
  install only (absent from `requirements.txt` / `setup_config.json`). A new
  **GPU Kernel Wheels → WGPKernels (INT8)** button installs v1.1.1 from its own
  repo (`deepbeepmeep/WGPKernels`, moved in `a827cf2`), auto-picking the wheel
  from the active env's torch build (`cu130` / `cu128`, or the experimental
  `rocm714` build on RX 9070/R9700). Shown for RTX 30/40/50 (compute 8.0+) and
  gfx1201; the installed version appears in Active Environment.
- **`int8_kernels` gains `wgpkernels`;** new **`fp8_kernels`** control
  (`auto` / `pytorch` / `triton` / `cuda`) for FP8 checkpoints. Auto-Tune
  recommends `auto` for both and validates the full upstream enum.
- **Audio profile default is P3 again.** Upstream made P3 "recommended for
  audio" (P4 + Dynamic Preload) and reserved P3+ for RAM-starved cards, so
  Auto-Tune now sends audio to P3 instead of P3.5 on video/image profiles
  2/4/4.5. Profile labels explain the new P1 = P2+Dynamic and P3 = P4+Dynamic
  semantics.
- **GGUF controls match wheel 1.0.25.** Verified against the installed wheel's
  `__init__.py`: the pre-1.0.25 Stream-K and BF16→FP16 env vars are gone, so
  those switches are retired and a **Short-batch mode** control
  (`LLAMACPP_GGUF_SHORT_BATCH`, auto/native/mma) takes their place. Matmul mode
  now also offers `materialized` (cuBLAS dense).
- **Guide covers the new models** — Flux 3 Klein 5.6B (image) and Fish Audio
  Drama 3 (speech) display names, with a Flux 3 starter in the image goal.
- **Text Encoder Quant** (`bf16` / `int8`) joins Performance Settings and
  Auto-Tune — recommends `bf16` on high-RAM boxes (better quality) and `int8`
  elsewhere (less RAM), matching Configuration → Text Encoder Quantization.
- **AMD experimental kernels are explained, not a dead end.** The `rocm714`
  WGPKernels wheel (and the `torch210rocm714` HIP GGUF wheel) require PyTorch
  2.10.0+rocm7.14.0 and — per upstream — a *separate* AMD environment; they do
  not load in the installer's torch 2.13+rocm10 env. A new **AMD setup (exp)**
  button (RX 9070/9070 XT/R9700, gfx1201) copies the exact setup commands, and
  the install paths now return that guidance instead of a generic failure.
  9060 XT (gfx1200) and RDNA 1–3.5 remain unsupported for these wheels.
- **AMD recipe gains `rocm-sdk init`.** Community guidance (DELUXA) recommends
  initializing the ROCm SDK for Veda sparse attention; upstream's main-env doc
  says it isn't needed there, so the copied recipe marks it as the optional
  step 3 of the *separate* 7.14 env with a skip-if-unrecognized note.
- **Active INT8/FP8 backend chips.** The dashboard's Active Environment card
  gains live `INT8 backend` / `FP8 backend` rows fed by WanGP's boot lines
  (`[INT8] Backend: …`), so the selected kernel is visible without digging the
  console (Discord question); cleared on server exit, survives refreshes.
- **Backend quality note.** Performance Settings + guide state that backends
  differ slightly at identical seed and Auto means fastest *without quality
  degradation* — garbage output calls for Triton/PyTorch explicitly.

### Hardware-overview pins re-checked against upstream (2026-10-16)

The installer's *Detected Hardware* matrix and the guides are generated from
WanGP's own `setup_config.json` / `requirements.txt`; a parity pass found and
corrected four stale entries.

- **RTX 20 no longer claims FlashAttention.** Upstream's `RTX_20` profile
  declares no flash wheel (Turing keeps only Triton `v33`), so the overview and
  guides show `—` instead of `Flash Attn 2.8.3`.
- **AMD shows FlashAttention 2.8.4 (ROCm/Triton).** Upstream's `AMD` profile
  gained `flash: v284_rocm` (the prebuilt `flash_attn-2.8.4-py3-none-win_amd64`
  wheel) — the installer already installed it, but the overview and guides said
  `—`.
- **Triton pins are stated, not "latest":** `3.2.x` (RTX 20), `3.6.x`
  (RTX 30–50), `3.7.x` (AMD), matching upstream `components.triton` labels.
- **bitsandbytes is an optional, manual wheel.** It is absent from upstream's
  `requirements.txt`, `setup_config.json`, `setup.py` and plugin requirements,
  but `docs/INSTALLATION.md` documents it as an optional component
  (`bitsandbytes==0.49.2`). The guides keep that pin, now labelled optional, and
  the dashboard's one-click **+** installs exactly `0.49.2` instead of whatever
  pip resolves today.
- **VAE control renamed to upstream's "VAE Tiling".** The Auto-Tune memory
  dropdown mirrored `vae_config` but used the old "VAE Config" label; it now
  matches `plugins/configuration/plugin.py` (0 auto / 16GB+ / 8GB+ / 6GB+
  presets).
- **Sparge Attn spec row shows a version, not a raw tag.** The dashboard read
  `status.profile.sparge` straight from `setup_config.json`, printing `v010_cu13`
  on RTX 30–50 — and, when a profile has no Sparge, the GPU profile name
  (`RTX_40`). The tag is now mapped to its version (`0.1.0`) and the misleading
  profile-name fallback is gone.
- The installer's requirement chip now reads `100+ reqs` (upstream ships 104
  active pins).

## [0.10.6] — 2026-10-12

### Stop no longer kills your own OpenCode/Zed on :4096

Stop (serve toggle, WanGP Stop, app close) swept port 4096 and killed any
`node`/`opencode` listener — including the user's own agent, which also
defaults to 4096. A process-name gate cannot tell ours from yours.

- **Ownership is by recorded PID now.** Start persists the spawned server
  PID to `.opencode.pid` (memory first, file second, consumed on Stop).
- **Kill iff the recorded PID is the :4096 listener** (plus an owner-name
  guard against PID reuse). Anything else on the port is logged with its
  PID for manual cleanup and spared — the blind sweep is gone.
- App-close fast path keeps its no-scan guarantee (recorded PID + name
  guard only).

### Upstream v18.02 parity (`46005e5` → `ad07926`, 2026-10-11/12)

WanGP v18 ("Distortion of Space and Time", now `18.02`) is a major release:
rewritten LTX/Wan/Hunyuan VAEs, H3 Veda Sparse attention, Kandinsky 6 video,
Prism i2v+audio, Fun/Uni3C camera control, CUDA Graphs, a Blackwell Compute
Cache, an 11-model 3D stack, workspace folders and expanded MCP support.

- **Health probe covers the 3D stack.** `requirements.txt` adds `xatlas`,
  `fast-simplification`, `scikit-image`, `resvg-py`, `chumpy` and `smplfitter`
  — the dashboard version scan now reports them instead of staying green with
  broken 3D generation.
- **3D stack visible in Active Environment.** The six probes above now render
  as panel rows (with Check Updates coverage) instead of living only in the
  status payload; Compute Cache Folder is labeled RTX 50 only.
- **Reinstall keeps more, re-downloads less.** `user_skills/` (v18 Deepy
  skills) joins the reinstall backup + restore set; `ComputeCache/` (up to
  2 GB of compiled Blackwell kernels) survives the wipe so a reinstall does
  not force a per-model re-optimize.
- **Veda documented, not foot-gunned.** H3 Veda Sparse is a per-task
  `override_attention` set by the model's own defaults — the launcher's
  global Attention Mode intentionally offers no Veda option (writing it
  globally risks an invalid config); the label map and hint now say so.
- **Model-drive warn gate raised 50 → 80 GB** (block stays 10 GB): v18 adds a
  30B video model plus GBs of 3D weights.
- **Guide starters for v18:** Kandinsky 6 Lite + Prism picks, a camera-move
  goal (Fun 1.3B / Uni3C 14B), a 3D-asset goal (TRELLIS.2 / Hunyuan3D /
  WorldMirror — the renderer now omits steps/guidance where they don't
  apply), a Veda pruned-FL2VA pick, and verified display names for all four
  Veda variants.
- **`ad07926` (18.02) needs nothing:** VAE Triton kernels migrate
  `libdevice.*` → `tl.*` with tuner bumps, SeedVC component pin rolls,
  Deepy workspace-save fix, Gradio polish — all upstream-runtime, no
  install/config surface. `settings_version` stays 2.80.
- **Compute Cache wired end-to-end** (keys verified against upstream
  `shared/cuda_memory.py`: `compute_cache` bool + `compute_cache_path`,
  default `ComputeCache`). Performance Settings gains both controls with
  rec/saved tags; Detect recommends On for RTX 50 (Blackwell) only; Apply
  validates through the same allowlist as every other knob. Deepy Goes 3D
  needs no launcher setting — it runs inside WanGP once the 3D models above
  are present.
- **Sage post4 retired (deepbeepmeep 2026-10).** post4-class builds degrade
  speed under CUDA Graphs (triton fallback); post6 is the fixed build and
  the current upstream pin (the `post7` release carries only a ROCm wheel,
  and the fixed Linux builds are not published yet — the launcher follows
  `setup_config.json` live, so they apply automatically once upstream ships
  them). "Restore GPU Wheels" no longer downgrades post6 → post4, the
  missing-entry fallback is post6, and all toggle labels now say
  setup_config-as-published instead of promising post4.
- **`compile` is a 3-state setting in v18, not a boolean** (`disabled` /
  `cudagraph` / `transformer`; the CLI `--compile` flag forces the
  deprecated transformer path). Launcher-written booleans matched no engine
  and silently did nothing. Performance Settings gains a Compile Mode
  selector, Detect recommends CUDA Graph on RTX 30/40, the known-good
  recipe prescribes it there too, and the flag table documents the
  deprecation.

## [0.10.5] — 2026-10-09

### The update notice stopped moving your buttons

Checking for a Desktop-Launcher update put a green strip into the dashboard,
between the topbar and the cards. Appearing pushed everything below it down by
its own height — so a click that started on one button landed on the button
that had just slid underneath. That is the "I pressed the wrong button"
report, and making the strip smaller never fixes it: anything sitting in that
slot can move the buttons. So the strip is gone. Nothing occupies that space
any more.

**The state lives on the control you would press anyway.** "Check Desktop
Updates" becomes `Update available — v0.10.5`, then
`Downloading v0.10.5 — 42%`, then `v0.10.5 ready — click to install`, keeping
the pulsing dot and green ring it already had. Pressing it does the pending
thing — download, then install — and asks GitHub only when nothing is pending.
Manage → Updates follows the same rule and its status line names the action.

**The announcement is one toast, once.**
`UPDATE · v0.10.5 available — click to download`, in the platform's usual
bottom-centre spot, clickable straight into the action, and it does not repeat
on the 5-hourly background poll. A check that finds nothing announces nothing:
no strip, no dot, no flash — the console keeps its
`Launcher v0.10.5 is up to date.` line, and failures keep using the plain toast.

- Removed the strip, its markup, the lane it reserved under the topbar, and the
  CSS only it used (`.update-float`, `.update-actions`, `.update-progress`,
  `.progress-*`). `.dash-body` is back to a plain `14px` padding.
- The SageAttention and AppData model warnings still use `.update-banner` and
  are untouched: they are actionable warnings rather than an update
  announcement, and they never shifted the button you were reaching for.

## [0.10.4] — 2026-10-08

### Every console message gets its own line again

This is the long-standing "newlines don't always work" complaint, and it was
never a CSS problem — `.term-body` has always set `white-space: pre-wrap` and
the browser honours it (verified: three `\n` in the text produce three line
boxes). The text simply had no newlines in it.

`appendLog` implements *stream* semantics: a chunk with no `\n` continues the
current line, because a child process's stdout arrives in arbitrary chunks. The
launcher's own messages are complete lines, and 68 of them were passed with no
terminator — only 2 ended in `\n`. So every launcher message concatenated into
one endless `lastLine` and the console showed a single run-on row:

```
[*] Wan2GP install found — loading dashboard…[*] Launcher v0.9.2 ready — …
```

- **`appendLog` now terminates launcher-side lines.** Keyed off the `forward`
  flag that was already there: the only `forward === false` callers are the two
  stream listeners in `init-tab.js` (`setup-output`, `launch-log`), and they
  keep the continuation behaviour so long child lines are never torn in half.
  `\r` progress overwrites still work — covered by tests.
- **The separate console window had the same fault.** `console-mirror` carries
  the same un-terminated launcher messages, so `term.js` now adds the newline
  on arrival. This is why floating read as *less* readable than docked.

### The console is resizable in every dock

The console panel had exactly one resize handle and it was wired to the bottom
and top docks only. Dock it left or right and the drag did nothing — the handle
was there, but `mousedown` bailed out before touching the panel, so there was no
way to trade GUI width for log width. Now every dock has a draggable edge on the
side that faces Wan2GP: drag up/down on bottom and top, sideways on left and
right, and any edge when the console floats.

- **Four edges instead of one.** `index.html` ships a handle per edge
  (`data-edge` / `data-axis`); `style.css` shows only the live ones for the
  current dock. They sit inside the panel bounds — `.floating-term` clips
  overflow, so the old negative-offset trick would have been clipped away.
  Floating is draggable on all four of its edges, so the floating panel is as
  resizable as any dock.
- **A drag moves the GUI too.** The old handler called only
  `syncTermEmbedPadding`, which is a no-op under a native child webview (it
  composites above the DOM and ignores CSS padding). Both syncs now run per
  frame, so the embedded view really does shrink as the console grows.
- **The cap is now the whole area below the topbar** (was 60%, then 90%).
  Dragging a docked console all the way up covers the GUI completely — the
  ask was to spend more time in the console than in Wan2GP. Only the topbar
  (44px) is never covered, so the dashboard icons stay reachable, and the
  console's own ✕ sits in its header, so a full-size console is never a trap.
  Minimum is 80px tall / 240px wide.
- **The separate console window (native + floating) opens big.** It was a fixed
  760x520 peephole — the unreadable case that made floating unpopular in the
  first place. It now opens at ~72% of the primary monitor in logical pixels
  (floor 760x520, cap 1800x1300 so it never opens off-screen), and is
  explicitly resizable and maximizable so the OS border and the maximize box
  are the "make it max" control there.
- **A dragged size sticks.** `setFtDock` ran `ft.style.cssText = ""` on every
  dock change, which threw the size away — that is why a resize appeared not to
  save. It now re-applies the size stored for the dock being switched to, and
  each dock keeps its own (`termSizes` in `desktop-config.json`), so bottom, top,
  left and right each remember how far you dragged them, across relaunches.
- **Floating is anchored before its first drag.** `.dock-floating` positions the
  panel from the right, so a drag on its west or north edge had no fixed edge to
  grow from and the panel slid away from the cursor. The first such drag
  converts the anchor to explicit `left`/`top` in place, keeping the position.
  - **The dashboard Console card resizes too.** Its top edge is now a drag handle:
  drag up for more log, down for less. The card is `flex:1` in a scrolling
  column, so a drag pins both the flex basis and the height — `flex-basis:0`
  beats an inline height in a column flex container, and without pinning the
  basis the drag would have had no visible effect. Bounded to the column (so the
  cards above stay reachable) with a 120px floor, re-clamped when the window
  resizes, and saved as `dashTermHeight`.
- **A `⤢ Full` button gives the Console the whole dashboard** (double-clicking
  the drag edge does the same). This is the browser-mode case: the launcher
  runs as a plain web page with no other chrome, so the Console *is* the view —
  and a drag can only ever move it inside its own column, next to cards nobody
  opens. Full mode hides the other columns and the other cards and lets the
  Console fill `.dash-body`. It parks the dragged height rather than losing it,
  dragging the edge drops back out of it, and it is deliberately **not**
  persisted: reopening into a dashboard with no visible GUI would read as a
  broken launch.

### The floating console no longer shows a shorter log than the docked one

Opening the console in its own window seeded itself from the backend's
`LOG_HISTORY`. That ring buffer is raw text — `base::push_log` splits `\r` into
separate lines and drops tqdm fragments — so the replay was **not** the log the
docked console shows: shorter, differently collapsed, and drifting further apart
with every dock switch. It now asks the main window, which owns the rendered
buffer, so the two consoles are the same log by construction.

- `term_console_seed` / `request_console_seed` (`src-tauri/src/system.rs`) route
  term → main → term. The request is deliberately **pull**, not push: a push at
  `create_term_view` time races the webview's listener registration and the
  console would start empty with no error anywhere.
- `term.js` replaces its buffer with the seed (replace, not append — the window
  is recreated on every dock switch, so appending would duplicate the log), and
  keeps `getLogHistory` only as a fallback behind a flag, for a build where the
  seed never arrives.
- `LOG_HISTORY`'s cap rose from 2000 to 5000 to match `MAX_LOG`, so the logs
  web page and the fallback agree with the console. tqdm fragments stay
  excluded there: they are `\r` rewrites, and replaying them as static lines
  would be noise, not history.

In the iframe renderer "floating" is the same DOM panel as any dock, so it was
already complete — the loss was specific to the separate native window.

Resizing never touches the log path: the docked panel renders from `logBuffer`
on every frame, and a drag only changes the panel's box — verified by pushing
`\r`, `\r\n` and tqdm lines through `appendLog`, dragging all four edges, and
confirming the rendered text is byte-identical before and after.

### Upstream v17.17 parity (`6479db3`, 2026-10-07)

Six upstream commits after v17.10 — `f02a382` ("H3 VAE optims") and four
iterations of "added RAM allocator" plus a "fix" — 57 files, read upstream as
**WanGP v17.17**.

**No install-stack movement.** `requirements.txt`, `setup.py`,
`setup_config.json`, `docs/INSTALLATION.md` and `docs/AMD-INSTALLATION.md` are
untouched in this range, so no torch, Triton, Sage, Flash, Nunchaku, LightX2V
or GGUF pin moves. The new RAM allocator accepts PyTorch 2.6 to 2.15, which
holds the managed `torch==2.10.0`, and its only new import, `psutil`, is
already an upstream requirement.

- **New `ram_allocator` setting, mirrored in Performance Settings**
  (`shared/cuda_memory.py` `RAM_ALLOCATOR_KEY`, `plugins/configuration/
  plugin.py`, `--ram-allocator` on the CLI). `mmgp` (upstream's default) hands
  the RAM of freed CPU tensors back to the system when the queue is done, a
  model is released or RAM runs short, keeping up to 2 GB for reuse between
  generations of the same model; `default` is PyTorch's own allocator, which
  keeps that RAM for the rest of the session. Added the control beside the VRAM
  one, its labels and `saved:` chip, and both backend gates — without them the
  key validated and was then silently dropped by the Apply allowlist, the one
  failure mode that panel has. No recommendation: upstream already defaults to
  `mmgp`, so there is nothing hardware-aware to calibrate and nothing to seed
  into a fresh `wgp_config.json`.
- **The allocator fingerprint now covers both prebuilt libraries.** v17.17
  ships `ram_alloc_win_amd64.dll` / `ram_alloc_linux_x86_64.so` beside the VRAM
  one and rebuilds the VRAM library (`vmm_alloc.cpp` +29/-3) under the same
  `mmgp` `4.0.0`, so `mmgp 4.0.0 (alloc ...)` could no longer tell a v17.10
  tree from a v17.17 one. `allocator_build_id` hashes what exists, which keeps
  the id a pre-v17.17 checkout always showed.
- **Dynamic image preload is not written outside Profiles 2, 4 and 5.**
  `wgp.py:4216` gates *Dynamic* to `mmgp_profile in (2, 4, 5)` (`init_pipe`
  does `int(profile)`, so 4.5 counts as 4). Auto-Tune recommended
  `image_preload_mode: dynamic` on every profile, so on P1 it wrote a
  setting upstream silently ignores. It now writes `default` there; the
  panel note, the dropdown tooltip and the guide say the same thing.
- **README: the Linux auto of Reserved RAM was still documented as 80%.** The
  tooltip and the panel were corrected in v0.10.3; the settings table was not.
  It now says 60% (upstream v17.10 / `ec9566a`).
- Left alone on purpose: SageAttention 2's staged path widened to sm86 and
  masked attentions, so the RTX 30/40/50 Sage 2.2.0 recommendation stands;
  Attention Head Split now counts all batch items' query tokens against its
  8192 threshold, so the tooltip's "8192+ tokens" still holds; and the
  `uint8_guides` RAM savings (H3 VAE, control videos, the `locals()` cycle fix)
  change no key and no pin.
  
  ### Documentation pass — README split by audience
  
  No code change. The README was carrying three audiences at once (decide +
  install, operate, contribute) and compromised on all three, which is why it had
  grown to 6,305 words. It is now the decide-and-install tier at 4,039 words,
  with mechanics moved to the docs that already had clean TOCs.
  
  - **What's New is now a release table** (1,092 → 198 words). Eight
    hand-maintained prose bullets became six rows; the full entries stay in the
    CHANGELOG, which is where a reader looks for them anyway.
  - **Auto-Tune** (1,261 → 394) keeps the profile matrix, the two honest caveats
    and the ownership rule; the per-setting v17 mechanics moved to
    `docs/USER-GUIDE.md`, whose `Manage → Auto-Tune` section was a three-line stub.
  - **GPU kernels** (848 → 188) keeps only the decision the launcher makes. The
    wheel table, per-GPU sets, driver requirements and AMD/Intel notes moved to a
    new `docs/USER-GUIDE.md` section with NVIDIA and AMD side by side. The README
    now deliberately restates no version numbers — the in-app card is
    authoritative — so the pins cannot go stale in prose again.
  - **Phone & remote access** was one 288-word paragraph. Split into the three
    real paths (A Gradio server / B Deepy Web standalone / C console logs) it is
    longer and readable, which is the right trade.
  - **Troubleshooting** turned from seven prose bullets into a button table — the
    reader arriving there is already stuck and is scanning for the fix.
  - **Facts that were wrong**: the test counts said 234 backend / 27 frontend and
    are now 256 / 37 (verified by running both suites); "10 infographics" claimed
    for a guide that has no images; the download size contradicted itself at 3 MB
    and 4 MB; `docs/OVERVIEW.md` still said GGUF 1.0.21 in three places. Where a
    version was genuinely ambiguous (`install.rs` carries both 1.0.14 and 1.0.25)
    the docs now name the floor instead of a number.
  - **New Documentation section** links all four docs from one place, including
    `docs/OVERVIEW.md`, which until now was reachable from nothing.
  - **Deleted** `todo.md`, `TODO-GUIDE-FEATURES.md` and `docs/DEEPY-WEB-DESIGN.md`
    — finished work with every checkbox ticked and no inbound links; the four
    openspec references to the last one now point at `openspec/changes/deepy-web/`
    instead. `docs/UPSTREAM-PATCH-auth-referrer.md` keeps its content but opens
    with a banner saying it documents a reverted patch.
  
  Verified across all five docs: zero broken anchors, zero missing file links,
  zero unlinked sections.

## [0.10.3] — 2026-10-06

Three report-driven fixes: the Auto-Tune panel's overlapping chips, a Restore
that never said what it installed, and the wheel-sync skip guard that had never
once fired.

### Sync GPU Wheels: the skip guard never fired on a real wheel URL

The 0.10.1 fix that stops a no-op Sync re-downloading 281.9 MB (nunchaku 111.7
+ GGUF 153.5 + Sage 16.7) compared `base_version(installed)` with the version
parsed out of the spec — and the parse **always returned None** for a real
upstream wheel, because it split the whole spec token on `-`. A wheel URL
carries hyphens in the host, the owner and the tag
(`github.com/nunchaku-ai/…/gguf-v1.0.25/…`), so the "version" it found was
`ai/nunchaku/releases/download/v1.2.1/nunchaku`. No version, no skip, and the
guard fell through to installing every wheel on every run — reported honestly
as `3 synced, 0 already current`. The old unit test passed a bare filename
(`https://x/pkg-1.0.whl`), which has no hyphen before the name, so it never
caught this.

- `kernel_spec_version_full` parses the **file name** out of the URL first
  (`%2B` decoded to `+`) and keeps the local build tag.
- `kernel_version_matches` compares the exact version, falling back to the base
  version only when the spec carries no tag — so the base-only comparison can no
  longer swallow a real change.
- That fallback was a second bug: with base versions only, turning the Sage safe
  toggle off in Manage → Settings (post6 → post4) compared `2.2.0` with
  `2.2.0` and skipped, so the setting silently did nothing. Same base version,
  different wheel — now it reinstalls.
- The superseded base-only `kernel_spec_version` is gone; its test moved to the
  new function with a real hyphenated URL added.

Four new tests, including the three RTX_30 wheels as setup_config spells them
against what `pip show` actually prints for them (verified on a live
`env_uv`: `1.2.1+cu13.0torch2.10`, `1.0.25+torch210cu130py311`,
`2.2.0+cu130torch2.10.0andhigher.post6`).

### Restore names what it installed, and stops printing a line twice

- **Restore says what landed.** `restore_requirements` ran
  `pip install -r requirements.txt`, streamed it to the console, and returned a
  bare ok — the panel reported "Requirements restored." whether pip had just
  installed three wheels or had changed nothing at all. pip's only statement of
  the outcome is its own `Successfully installed …` line, and that scrolls past
  the console viewer's tail on a full requirements run (it was not even visible
  in the log that prompted this). The command now also captures stdout,
  `pip_installed_from_output` reads the distribution list, and both Restore
  entry points print it: `Restored: insightface-0.7.3, chumpy-0.71,
  smplfitter-0.2.10`, or `Requirements already matched — nothing installed.`
- **The moved-aside-files note appeared twice** per update — once from the
  backend, once again from the Dashboard handler. The frontend copy is gone;
  the backend line (`[i] N file(s) kept in .launcher-update-backup/ …`) is the
  single one.

### Auto-Tune panel: the rec:/saved: chips stop painting over the next field

The Performance Settings grid is two columns of dropdowns, each followed by a
`rec:` and a `saved:` chip. Both chips are `white-space: nowrap`, and the grid
item defaulted to `min-width: auto`, so a long recommendation ("SageAttention 2
/ 2+ (recommended — v17 gains depend on it)", "MMGP Optimized, with RAM
spilling (default — falls back to shared GPU memory)") pushed past its column
and printed straight over the neighbouring field. Widening the allocator label
in 0.10.2 made it visible.

- `.mem-field` gets `min-width: 0`, `.mem-hint` wraps, and each chip is capped
  at its column with an ellipsis.
- Nothing is lost: the chip's `title` carries the full text (set next to
  `textContent` in `memProfilePopulate`), so a clipped value stays readable on
  hover.

## [0.10.2] — 2026-10-06

Theme persistence, no-op update honesty, and upstream v17.10 parity.

### Theme: a manual light pick survives a relaunch

The main window shipped `data-theme="dark"` hardcoded in `index.html`, and only
the dark branches of the four theme paths ever wrote that attribute, so a user
who picked light came back to dark on the next launch. Each path also decided
the theme for itself instead of asking one question.

- **One resolver, four callers.** `resolveTheme(cfg, prefersDark)` in
  `theme-tab.js` is the single source of truth: the persisted choice, or the OS
  preference while follow-system is on. Startup (`init-tab.js`), the manual
  toggle, the OS-change listener and the settings toggle (`term-tab.js`) all go
  through it, and `applyTheme` normalises to dark/light before touching
  `data-theme` — so the light branch now actually *removes* the attribute.
- **A manual pick outranks follow-system.** `toggleTheme` clears
  `themeFollowSystem` and unticks the settings switch. Leaving the flag on meant
  the next launch — and every later OS theme change — reverted to the system
  theme and threw the choice away.
- **The follow-system toggle now applies in both directions.** Turning it off
  falls back to the persisted choice instead of leaving the system theme on
  screen until the next relaunch.
- **The floating terminal follows.** `term.js` reads `w2gp.theme` — the key
  `applyTheme` writes, alongside accent and the scales — instead of `theme`,
  which the preload never wrote, so the mirror was stuck on its last value.

### Update: a second Update press says so instead of re-doing everything

Pressing **Update Wan2GP** on a checkout that already *is* upstream printed a
wall of work-sounding output — "66 upstream file(s) differ", the hand-changed
warning, `HEAD is now at <same sha>` — for a tree that had nothing to land. Run
it twice and the two logs are indistinguishable, which reads as a repo that
refuses to update rather than one that is current.

- **No-op updates short-circuit.** `changed_file_count` now distinguishes
  "0 files differ" from "the git probe failed" (`Some(0)` vs `None`); on
  `Some(0)` with no hand-edited tracked files the update emits one
  `[=] Already at upstream <sha> — nothing to update.` line, runs the
  dependency drift probe (still useful) and returns `updated: false`. A probe
  failure keeps the old unconditional path — fail-open, never skip an update
  because git hiccuped. Hand-edited files with an already-current checkout
  still get the no-stash reset, and say so.
- **The launcher's own backup folder stopped counting as your files.** Git
  reports `.launcher-update-backup/` as one `??` entry, so it was tallied as a
  "personal file" (inflating the count) and never mentioned again. The update
  summary and **Verify** now exclude it, report how many files are parked there,
  and `verify_wangp_files` returns `updateBackup`. The Dashboard and Manage
  buttons also say "Already up to date" instead of "Update finished".

Nothing was lost in the reported run: the six files were untracked local copies
of upstream's new LTX-2.5 VFX files, the guard moved them aside exactly as
designed, and the checkout landed on `ec9566a6` with everything tracked.

### Upstream v17.10 parity (`09a7c6c8` → `ec9566a`, 2026-10-06)

Four upstream commits landed after v17.00: `1074cc3` + `0e58385` ("fixes") and
`c2b4743` + `ec9566a` ("LTX VFX + mem allocator fixes", `WanGP_version =
17.10`). 90 files, almost all runtime and model code.

**No install-stack movement.** `requirements.txt`, `setup.py`,
`setup_config.json`, `docs/INSTALLATION.md` and `docs/AMD-INSTALLATION.md` are
untouched in this range, so no torch, Triton, Sage, Flash, Nunchaku, LightX2V
or GGUF pin moves and the launcher-compat line correctly stays silent. No AMD
work either.

- **Reserved RAM auto is 60% on Linux now, not 80%**
  (`mmgp/offload.py` `_get_perc_reserved_mem_max`; Windows stays 40%). The
  Reserved RAM tooltip said 80%; the panel now matches upstream, and its input
  caps at 80 like WanGP's own slider.
- **RAM spilling no longer stops at the RAM floor.** The allocator now takes
  `vmm_configure(..., 3 if spill else 0)`: pinned system RAM first, then the
  CUDA driver's own allocation (shared GPU memory on Windows). Only `vmm`
  without spilling raises an out-of-memory error, and a new `driver_spilled`
  stat reports the fallback. The allocator labels and tooltip say so.
- **Other programs keep their VRAM.** The MMGP allocator now takes VRAM another
  program is not using only when a generation would otherwise run out, instead
  of budgeting for it up front (documented in `docs/CLI.md` and
  `mmgp/README.md`); the tooltip carries the wording.
- **New `rgba_video_output` output setting** (`png_zip` | `prores_4444`) with
  `prores_ks` / `yuva444p10le` / 16-bit alpha, plus `shared/utils/rgba_video.py`
  and imageio `macro_block_size: 1` so mattes stay pixel-aligned with their
  source. The launcher leaves the key alone — it is WanGP's own Outputs
  setting, not a RAM/VRAM one — so nothing here can clobber it.
- **Two new LTX-2.5 VFX presets** (`ltx2_25_22B_alpha_gen`,
  `ltx2_25_22B_layout_to_render`) reach the Guide tab as a new "Transparent
  video (alpha) / VFX" goal, alongside `alpha2`. The 2.5 base preset's preload
  LoRA moved to `ltx-2.5-22b-ic-lora-sdr-to-hdr-scene-emb.safetensors`.
- **Attention Head Split numbers refreshed** for H3 with Sol (~4 GB at Medium)
  and the VDN models (~2.5 GB) — the tooltips already carried them.
- TinyVAE `decoders.json` is gone; handlers declare `tiny_vae_architecture`
  and the preview family list widened. No launcher surface.

## [0.10.1] — 2026-10-05

### Upstream v17 parity (`b8b18f81` → `09a7c6c8`)

Re-audited every commit deepbeepmeep landed since the AMD/ROCm refresh. The
install stack is untouched upstream — `docs/INSTALLATION.md`,
`docs/AMD-INSTALLATION.md`, `setup.py` and `setup_config.json` did not change —
so no torch, Triton, Sage, Flash, Nunchaku, LightX2V or GGUF pin moves. What
v17 changed is `requirements.txt` (mmgp leaves pip), the `wgp_config.json`
schema (v1.25), and the MMGP v4 runtime.

- **The update log stopped lying about its own size.** `rev-list --count
  HEAD..@{u}` is meaningless on our `--depth 1` clone: git records a shallow
  boundary at the old HEAD, so the first fetch walks the whole upstream
  history back through it and the range counts every ancestor re-downloaded.
  A fresh install's first update reported **1828 incoming changes for a
  4-commit delta** (the "incoming" commits were dated 2025-10 → 2026-10). Now
  a tree diff between HEAD and FETCH_HEAD, which needs no history at all.
- **The launcher-compat line speaks only when there is something to do.** It
  fired on every update with the same text ("gguf 1.0.25 (floor 1.0.25) —
  re-run Sync GPU Wheels") even when upstream already equalled the floor and
  the wheel was installed. Silent when nothing is pending.
- **mmgp is reported from where it actually runs.** v17 vendors MMGP v4 into
  the repo and drops `mmgp==3.8.2` from requirements.txt, but `pip install -r`
  never removes a dist — the old wheel sat in site-packages while `import mmgp`
  loaded the in-repo copy. The dashboard reported `mmgp 3.8.2` for v4 code. The
  version now comes from `mmgp/pyproject.toml`, and an update that unpins mmgp
  removes the stale install and says so.
- **Sync GPU Wheels stops re-downloading what you already have.** The sync
  loop appended a blind `--upgrade`, so every wheel was re-fetched on every
  run: a no-op sync pulled **281.9 MB** (Nunchaku 111.7 + GGUF 153.5 + Sage
  16.7) for wheels already installed at exactly those versions. Sync now
  compares the wanted version against what pip reports and skips, reusing the
  existing Triton no-downgrade guard's shape. Unknown components and failed
  probes still install. Sync also closes with a
  *synced / already current / failed* summary instead of trailing off on a
  download line.

### Getting the most out of v17 on any hardware

The README asks every user to turn on five MMGP v4 settings by hand. The
launcher now calibrates them, per hardware, and still lets the user override —
in the launcher or inside WanGP.

- **Fail-closed config gate.** `valid_memory_override` now rejects anything
  outside `vram_allocator` (default/vmm/vmm_spill), `attention_head_split`
  (0–3), `read_ahead`, `smart_memory_pinning`, the per-output preload modes,
  the preload amounts and `perc_reserved_mem_max` (0–100 percent as of v17).
  The Apply allowlist was also missing every one of these keys, so Apply
  reported success while dropping them; both now share one key list.
- **The tight-VRAM tier no longer falls back to the safety net.** MMGP v4
  makes Profile 4 up to 50% cheaper in VRAM and does 1080p H3 on 11 GB, so a
  card under 12 GB rides **P4 plus Attention Head Split** instead of P4+/P5.
  "Prefer failsafe" still forces P5.
- **Audio defaults to Profile 3+** — upstream's own default for audio models
  ("they are usually small enough to fit entirely in VRAM, where the language
  model many of them include runs much faster"). Not gated on the VRAM tiers:
  those are calibrated against 14B video models and audio models are an order
  of magnitude smaller, so a 10 GB card earns P4 for video and still gets P3+
  for audio. Audio follows video down to P5 when video is at the failsafe net.
- **Per-vendor gating.** `shared/cuda_memory.apply_startup_settings` returns
  early on HIP and when CUDA is unavailable, so the allocator and Dynamic
  Preload cannot work on AMD/Intel/CPU. Detect leaves those keys unset there
  rather than persisting settings that silently do nothing.
- **Eight new Performance Settings controls**, mirroring WanGP's own
  Configuration → RAM/VRAM Management panel one-for-one: VRAM Allocator,
  Attention Head Split, Read Ahead, Smart Memory Pinning, VRAM Preload for
  Video/Image/Audio, and Reserved RAM for Pinning. Same rec/saved contract as
  every other knob — Detect proposes, Apply writes, and a value changed inside
  WanGP shows as `saved` rather than being clobbered. Profile dropdowns now
  carry upstream's v17 descriptions.
- **Fresh installs start calibrated.** Install seeds the recommended keys into
  `wgp_config.json` with setdefault semantics only — it runs on fresh install
  alone, never on update, and never overwrites a key already on disk.

- **The app could come up black after adding the Attention Mode control.** The
  edit nested a new field inside the VRAM Allocator's `mem-field` and left the
  outer one unclosed, so every control after it nested one level deeper and
  the grid container closed early — no console error, no clue which line. Two
  structural guards now make it loud instead: `index.html` `<div>` tags must
  balance, and every id the Auto-Tune panel reads must exist exactly once (a
  duplicate id silently repaints one control while writing tags to another).
- **Auto-Tune now recommends an attention mode.** Upstream v17.01 says plainly
  that *"quite a few optimizations depends on"* Sage2/2+, so the attention mode
  belongs with the memory profiles rather than only in Troubleshooting: RTX
  30/40/50 → `sage2`, RTX 20 → `sage`, GTX 10 → `sdpa`, and nothing on
  AMD/Intel where there is no Sage build upstream. Still a recommendation —
  Detect proposes, Apply writes, and a mode you already chose stays yours.
  Auto-Tune and the known-good recipe now read one definition of it, so they
  cannot disagree.
- **The dashboard distinguishes MMGP allocator builds.** v17.01 rebuilt
  `mmgp/allocator/vmm_alloc.cpp` and its prebuilt DLL **without** bumping
  `mmgp/pyproject.toml`, so two different allocators both reported `4.0.0`. The
  version now carries a short content hash of the platform binary
  (`4.0.0 (alloc 1a2b3c/94k)`), so a bug report says which allocator was
  actually loaded.
  
### Troubleshooting gets the v17 and GPU-class remedies

Upstream's `docs/TROUBLESHOOTING.md` is mostly copy-paste command lines. These
turn the parts that map onto a real control into buttons, keeping the existing
rule that Detect proposes and Apply writes.

- **Known-good settings for this GPU.** Upstream hands out four per-class
  command lines (GTX 10XX `sdpa`/P4, RTX 20XX `sage`/P4, RTX 30–40XX
  `compile`/`sage2`/P3, RTX 50XX `sage2`/P4). The launcher now reads the same
  recipe for the detected card and shows it before writing anything. Only keys
  verified to exist in a live v17 `wgp_config.json` are written — Tea Cache and
  fp16 have no config key upstream, so they come back as a note pointing at
  WanGP instead of a setting that would silently do nothing. AMD/Intel/CPU get
  no recipe: upstream publishes none, and the launcher's AMD path owns them.
- **Out-of-memory remedies as one click.** Attention Head Split → Medium
  (upstream's figure: ~2 GB less VRAM for ≤3% slower steps on H3 1920×1088 /
  362 frames) and Lower Reserved RAM with Smart Memory Pinning on. Both go
  through the same fail-closed gate and snapshot as Performance Settings.
- **Windows VRAM diagnostics.** `scripts/gpumem.cmd` (per-process VRAM, which
  WDDM hides from `nvidia-smi`) and `scripts/gputrim.cmd` (ask Windows to trim
  idle allocations, behind a confirmation — it briefly applies memory pressure).
  `gputrim.cmd` shells out to bare `python`, so the active env's Scripts
  directory goes first on PATH.
- **Verbose logging** — a checkbox that launches WanGP with `--verbose 2`, on
  both the main server and the standalone Deepy Web child, so bug reports carry
  upstream's own diagnostics.
- **Debug bundle knows about v17**: mmgp version, allocator, head split, smart
  pinning, read-ahead, reserved-RAM percent, the three preload modes, plus the
  attention mode and compile flag.

### Fixes found while wiring the above

Two of these are the same bug twice: a hardcoded key list that nobody extends.
The panel owns its keys in one place now, and both directions derive from it.

- **Apply silently dropped every v17 key.** The write path had an allowlist
  that predated the MMGP v4 settings: Apply reported success, listed the keys,
  and wrote none of them.
- **The panel could not show what it had saved.** `memory_profile_read` had
  the same defect on the read side, so a value that saved correctly still
  rendered as `saved: —`; it also invented a default for any key that was
  never written, painting `saved: 4` on configs that had no such key. It now
  returns exactly the panel-owned keys that are on disk, and nothing else.
- **`saved:` tags went stale after Apply.** The tags were captured when the
  panel opened, so a value that genuinely changed sat there showing the old
  one — audio read `rec: 3.5` beside `saved: 4`. Apply now re-reads from disk
  before it reports.
- **Apply Overrides refused nothing while WanGP was running.** WanGP keeps its
  own copy of `wgp_config.json` and rewrites the file on any settings change
  inside its UI, so writing then is a lost update in whichever direction the
  user did not expect — the panel reported success and the value silently
  reverted. Apply, the OOM remedies and the known-good loader now refuse with a
  message naming the fix. Detection uses the PID the launcher spawned plus the
  configured port, so a WanGP started by hand is caught too.
- **Audio defaulted to the video profile instead of P3+.** Upstream's default
  for audio is Profile 3+; Auto-Tune gated that on VRAM calibrated for 14B
  video models, so a 10 GB card that earns P4 for video lost the CUDA Graph /
  vLLM fast path for its audio models. Audio now keys off the video profile.
- **Install died when uv was not installed system-wide.** setup.py builds a uv
  environment with `uv venv …` through `subprocess.run(shell=True)`, which
  resolves `uv` on `PATH` — not from the absolute path the launcher used to
  provision Python. The launcher deliberately keeps its own uv private in
  `<dataDir>/.tools` so it never collides with another app's copy, but nothing
  put that directory on the child's `PATH`. With uv unlinked the install failed
  at step 1/3 with *"'uv' is not recognized as an internal or external
  command"* → `CalledProcessError`. The tools directory is now prepended for
  the setup child (and logged, so it is visible next time) and the existing
  `PATH` save/restore puts it back afterwards.
- **Hermetic tests could read the real install.** `get_data_dir()` /
  `get_repo_dir()` cached for 5 s, and `TestDataDir` only serializes the tests
  that hold its guard — a sibling test resolving a directory without it could
  repopulate that cache with another test's deleted tempdir or with the real
  `C:\Wan2GP`, which is how the Deepy round-trip test intermittently saw the
  real `enhancer_mode` instead of its seed's default. Both now bypass the cache
  under `cfg(test)`; 8 consecutive full runs, no failures.

## [0.10.0] — 2026-10-04

- **Renderer split.** `app.js` went from 10,545 lines to 1,469: 35 `*-tab.js`
  files now hold one panel each (installer, Deepy, Deepy Web, LLM engines, theme,
  plugins, settings, dashboard, troubleshooting, DLSS5, migration, terminal,
  auto-tune, and more), all loaded after `app.js` in `index.html`. Every move is
  a pure cut-and-paste — no logic changed. What stays in `app.js` is the shell:
  the global log buffer, overlay/card helpers, and the small launch-button
  sections that share too much state to split cleanly.
- **First test suite.** `cargo test --lib` (234 tests) and `npm test` (27 tests)
  both run from a clean checkout with no extra dependencies — the frontend suite
  uses Node's built-in `node:test`.
- **Tests can no longer touch your install.** A `WAN2GP_DATA_DIR` seam redirects
  `get_repo_dir()` at a tempdir, so running the suite leaves
  `C:\Wan2GP\wgp_config.json` byte-identical. Previously the Deepy round-trip
  test rewrote it for real and skipped its restore whenever an assertion failed.
- **DOM harness.** `npm run harness` serves `src/` on localhost with the Tauri
  IPC stubbed, so the real renderer boots in a browser and panels can be driven
  and screenshotted without a build.
- **Load-order guards.** Splitting made script order a real contract: a file
  cannot reference a symbol a *later* file defines. `tests/load-order.test.js`
  scans every loaded script and fails on a bare-argument read of a later symbol —
  the class of bug that breaks boot silently. Three such bugs were caught during
  the split and fixed (theme toggles, terminal buttons, `closeSettings`).
- **Removed 11 dead service modules** (~2,200 lines) left over from the Electron
  port — never loaded by `index.html` and never `require`d, their logic already
  owned by the Rust backend or by `app.js`. `tests/service-graph.test.js` fails
  if a new one appears. Stale comments pointing at them now point at the real
  owners.
- **Queue Row Colors moved** from Auto-tune to Settings → General → Appearance:
  it is a theme preference, and the hardware scan has no opinion on it. It
  previously showed a permanently-empty `rec:` badge.
- **Hardware chips format properly.** The Auto-tune card rendered
  `RAM 31.763145446777344 GB` because it concatenated the raw probe float;
  GB values now round to one decimal and missing values show `—` instead of
  `NaN`.
- New Rust coverage for `log_server.rs` (log tail parsing/clamping) and
  `updates.rs` (README version scrape, commit-hash validation), both extracted as
  pure functions to be testable.

## [0.9.2] — 2026-10-03

- AMD upstream parity (`b8b18f8` audit): RX 9060 / 9060 XT map to `gfx1200`
  (Navi 44), not `gfx1201` — fallback torch targets, `known_vram_mb`
  (XT 16GB / plain 8GB), and `kernel-resolver.js` fixed; no launch-time
  `ROCM_HOME` / `PATH` / `CC` / `CXX` / `DISTUTILS_USE_SDK` anymore
  (triton-windows resolves the SDK from the env); AMD `attention_mode`
  default is now `sdpa` (upstream profile value, migrates stale
  `auto`/`sage`/`sage2`); torch recipe reads the cloned
  `setup_config.json` `rocm10` template (`{device}` spliced per-box).
- Fail-closed memory tiering: RAM probe rejects truncated output (no more
  `512` → 512GB → P1), probe failure falls back to 16GB (`very_low`, P5
  direction — was silent 32GB → P4); VRAM tiers use `floor` (11.5GB stays
  `tight`); overview `profileNum` follows upstream RAM+VRAM pid thresholds
  (unknown RAM never shows P1); healthy AMD cards tier through Auto-Tune
  instead of forced P4.5; `kernel-resolver.js` NVIDIA tokens synced to
  spaced generation tokens (3050/4050 no longer → Blackwell).

## [0.9.1] — 2026-10-01

- Opt-in console log viewer on its own port (Dubon request): new
  **C · Console logs** section in Manage → Phone & remote access serves the
  same read-only console tail (errors, completions) as a web page on
  `serverPort+2` by default — keep `:7860` generating in one tab while
  watching logs in another, including from a phone on the same Wi-Fi.
  Off by default; flip Logs page on (Same-PC) or add Phone-LAN + Apply
  for LAN access. No new dependencies (plain std HTTP), trusted-LAN only,
  never port-forward to the internet.

## [0.9.0] — 2026-10-01

- AMD ROCm 10 refresh (upstream `b8b18f8` / PR #2397): installer follows the
  unified `AMD` profile — Python 3.12, pinned stable torch 2.13.0+rocm10
  (`stable.repo.amd.com/whl-next`, per-box `device-gfxXXX` targets from
  `clinfo` with a profile-based fallback), nightly whl-next fallback,
  `triton-windows>=3.7,<3.8`, SageAttention 1.0.6 (package gate allows
  PyPI 1.x, still refuses CUDA-built 2.x wheels), SDPA default. Old
  `rocm65` / per-family-nightly envs must be recreated, not upgraded.
  `HSA_OVERRIDE_GFX_VERSION` is no longer set (upstream dropped per-arch
  overrides; stale values are removed), and `FLASH_ATTENTION_TRITON_AMD_ENABLE`
  is no longer set at launch (build-time only upstream). HIP GGUF messaging
  now states the separate torch 2.10 env requirement.
- RAM probe is locale-proof: total memory is read as integer bytes
  (pl-PL `79,8`-style decimal strings no longer fall back to 32 GB).

## [0.8.9] — 2026-09-30

- Active Environment updates are now pin-aware (issue #54): GPU-profile
  dists (torch / torchaudio / torchvision / triton) and `setup_config.json`
  kernel wheels (flash-attn, sage, sparge, nunchaku, GGUF, lightx2v) show a
  `pinned` chip instead of an upgrade arrow — a bare PyPI upgrade pulled the
  CPU-only torch wheel or the uninstallable flash-attn sdist and broke the
  env. `requirements.txt`-capped packages (`==` pins, ceilings, marker-aware)
  are pinned the same way; floors and bare names stay upgradable. Both
  `upgrade_package` paths refuse pinned dists with a recovery pointer.
- Pinned chips are clickable: an explicit **Yes / Cancel** dialog (no
  OK/Cancel trap) names the risk and the way back per source — reinstall for
  torch (restore cannot fix it), Sync Kernels / reinstall for wheels, restore
  for requirements pins — and only then runs a forced upgrade.
- Panel hint corrected: restore reinstalls the tested `requirements.txt`; a
  broken torch needs reinstall.

## [0.8.8] — 2026-09-27

- Quieter updates: the best-effort `merge --abort` cleanup after a hard
  reset no longer prints a scary `fatal: There is no merge to abort` line
  when there is nothing to abort (Update + Repair).

## [0.8.7] — 2026-09-27

- Update policy is now stash-free: latest upstream always wins — fetch, loud
  warning, hard reset. Hand-changed tracked files (and any unfinished merge
  state) are overwritten, never stashed; untracked files still never touched.
- Verify / Repair recovers conflicted checkouts: unmerged files are detected
  up front and reset to latest upstream (previously died in the stash step).
- DLSS panel follows the upstream merge: when Wan2GP already tiers
  workstation cards natively, Check says so and the fix button disables
  itself instead of offering a redundant patch.

## [0.8.6] — 2026-09-27

- Workstation GPU fix, hardened from the first real PRO 5000 report: the
  patcher refused on CRLF checkouts (Windows autocrlf) — matching now
  normalizes endings and preserves the file's style. A refusal logs the
  installed `_gpu_series` body to the console for paste-into-report
  diagnostics, and an upstream-fixed runtime is detected (nothing to patch).
- Check compatibility: compact frame-gen probe summary (`5x max`, runtime
  version) in the HAGS row instead of the raw JSON blob — full probe detail
  stays in the console.

## [0.8.5] — 2026-09-27

- DLSS5 card: new **Check compatibility** button — per-mode verdicts (ready /
  not-installed / blocked) with GPU tier, installed files, HAGS, and the
  native frame-gen probe, mirrored to the console; results render as labeled
  rows. New opt-in **Apply workstation GPU fix** button patches Wan2GP's
  GeForce-only `_gpu_series()` so RTX PRO / Ada / RTX Ax000 / L40 / Hopper
  cards (e.g. RTX PRO 5000, RTX A6000) pass DLSS gating — original backed up
  (`*.launcher-bak`), one-click revert, fail-closed on upstream drift.
  ⚠️ Workstation path is **untested on real hardware** (tiers
  logic-simulated only: PRO 5000→50, 5000 Ada→40, A6000→30; no PRO/A-series
  card available) — please confirm on workstation silicon before relying on it.

## [0.8.4] — 2026-09-27

- Upstream Wan2GP `v13.14` (Sep 27, community release): GGUF kernel floor
  1.0.23 → **1.0.25** (INT8-verified speculative decoding for Q4_K / Bonsai
  PTQ1_0, SM120 async path). Sync / override / overview target the 1.0.25
  builds (`gguf` cu130/py311 + `gguf_cu128` cu128/py310); 1.0.23/1.0.24 now
  flag stale. HIP opt-in bumped to `1.0.25+torch210rocm714`.
- Local Deepy Prime accepts **Qwen3.8 9B Heretic** as well as 27B: weights
  probe covers both asset dirs (fail-closed only when neither is present),
  Prime + local shows ONE combined model+quantization list (27B:
  Q4/Q3/Q2/Bonsai PTQ1; 9B: Q4 ~6.5GB / Q8 ~11GB), Zero/Disabled offer the 9B
  model (id 6) with matching `qwen38_9b` engine mapping. The Prime panel
  groups Local (Qwen3.8 radio + its model/quant list beneath it) and Remote
  (Codex/Claude/OpenCode radios + LLM Engines setup card, hidden while a
  local engine is picked), mirroring Zero's model-then-quantization order.
- Claude Code bridge pin `0.1.40` → `0.1.66` (per upstream `REMOTE_LLMS.md`).
- Fix perpetual "update available": update badge + Sync behind-warning now
  check containment (`merge-base --is-ancestor`) instead of hash equality,
  so a local merge commit no longer reads as behind forever; Update tries
  `git pull --ff-only` first and only merges when diverged.
- Console: consecutive tqdm progress prints (e.g. Wan2GP's `Generating: 0%|…`
  LLM meter, one `\n`-terminated row per refresh) collapse into a single
  in-place row instead of flooding the log — 0% → 100% ticks on one line in
  every console view (dashboard, floating, separate window).
- Memory panel: new Queue Row Colors dropdown (Pastel rainbow / Theme grey)
  writing upstream's `queue_color_scheme` — grey follows dark/light instead
  of the hardcoded pastel rows.

## [0.8.3] — 2026-09-25

- Upstream Wan2GP `5533384` (Sep 24, H3 VAE fix): GGUF kernel floor
  1.0.22 → 1.0.23 (short-batch projection fusion + SM120 async path).
  Sync / post-install override / overview now target the 1.0.23 builds and
  understand the `gguf` (cu130/py311) + `gguf_cu128` (cu128/py310) split —
  py310 envs get the cu128 build (mirrors `setup.py`'s torch/py remap),
  1.0.22 installs now flag stale. HIP `1.0.22+torch210rocm714` opt-in
  unchanged (upstream did not bump it).
- Fix Sync failure on GGUF 1.0.23 (fixes #44): upstream `setup_config.json` cmds carry a
  `--no-deps` prefix (since `a56122a`) which the launcher passed to pip as
  ONE argument (`no such option: --no-deps https://…`). Kernel cmds are now
  split into separate pip argv items; override wheels install with
  `--no-deps` exactly like `setup.py`. Kernel Sync/Restore/HIP failure
  toasts also print the real backend error instead of "undefined".
- Collapsed GPU Kernel Wheels card now signals pending wheel updates: a green
  "● update" badge on the header one-liner (the Update button itself is hidden
  while collapsed) plus a green ring around ↻ Update GPU Wheels when expanded.
- Desktop view follows the launcher theme (fixes #43): the embed URL now
  carries Gradio's `?__theme=dark|light` (verified against the gradio 5.29
  frontend bundle; Gradio's own toggle still wins afterwards). Covers iframe
  + native renderers.
- Upstream-proofing (Sync keeps working after deepbeepmeep changes):
  GGUF floor follows `setup_config.json` forward and stale pins swap to
  upstream's own fresh pin (a future 1.0.24 flows through with no code
  change); unknown profile kernels skip LOUDLY in the console instead of
  silently under-installing; `setup_config.json` shape is validated
  warn-only at Sync and after every Wan2GP update (new `compat` field +
  toast); update flow logs a launcher-compat line (setup_config gguf pin
  vs floor). `%2B`-encoded wheel versions now parse to clean versions.

## [0.8.2] — 2026-09-23

- Phone & remote access panel (left column): new **LAN (`--listen`) toggle**
  for the main Gradio server, persisted as `mainLan` and appended verbatim
  on launch (deduped against manual Extra args; wins over the bind-address
  setting exactly like upstream `wgp.py`, which forces `0.0.0.0`). Flipping
  it while running offers a confirm-then-restart in the same Desktop/Browser
  mode (fail-closed if the port stays bound). The card shows This-PC plus
  Phone **Gradio `/` and `/deepy/`** URLs with Open/Copy/QR — both views
  share the live conversation, galleries, progress and queue — and probes
  the LAN IP itself so the status stays honest even for manually-started
  servers. Merged with the Deepy Web card into one panel with labeled
  **A · Gradio server** / **B · Deepy Web standalone** sections
- Collapsible left info panels (chevron per card, state remembered across
  restarts): collapsed headers keep their key actions live (enhancement
  mode + Apply, Deepy Disabled/Zero/Prime + Apply, LAN toggle + Deepy
  Start/Stop), while Kernel Wheels and Active Environment collapse clean;
  the pip strip is now a real panel too. Single-env installs no longer
  show the dead environment-switcher row (renders only with 2+ envs)
- Console **Clear** button (dashboard, docked, and pop-out consoles):
  wipes local buffers plus the backend ring buffer and broadcasts so all
  consoles stay identical
- Control taxonomy styling (all themes, no per-theme values): inset
  type-in fields with accent focus, link-style address buttons, accent
  radio-pill selection, themed native dropdowns/inputs, focus rings
- Fixes: Deepy Web port field can be cleared back to **auto** (server
  port + 1) again — empty + Save deletes the override; `manage_list` now
  returns `{name, type, active}` objects so the env switcher renders names
  and click-to-activate works (was a bare bullet row); "Fixed secret"
  relabeled **Fixed passphrase** (matches upstream wording)
- Guides: new proxy-debugging checklist (RunPod/Nginx/Cloudflare stalls —
  `gradio_config.root`, queue stream, events 101 vs poll fallback, 403)
- Manage tabs reorganized: Repair Settings → System troubleshooting,
  GGUF CUDA Kernel + AMD ROCm → Launch (next-launch hardware prefs),
  Xet Storage → System updates cluster; System port tool renamed
  **Port conflict**; the stray Extra-args hint sits under its own field.
  General is now purely tokens/browser/appearance/desktop/notifications
- Polish: topbar realigned to one 26px sight-line (uniform icons, no
  baseline gaps); collapsed headers keep actions on a single right-aligned
  row with uniform sizing; panel interiors share one section rhythm, a
  `.form-row` pattern, and a vertical pip card

## [0.8.1] — 2026-09-22

- AMD experimental HIP wheel (upstream Sep 22 `docs/INSTALLATION.md`):
  Sync-kernels-only opt-in for RX 9070 / R9700 (gfx1201) — new
  **HIP GGUF (exp)** button in GPU Kernel Wheels (AMD profiles only)
  installs `llamacpp_gguf_cuda-1.0.22+torch210rocm714` with
  `--no-deps --force-reinstall` (same dist name as the CUDA wheel, torch
  untouched — the HIP wheel rejects other torch builds at import; needs
  torch 2.10.0+rocm7.14.0, validation pending). The main install stays on
  the TheRock 7.15 stack and now logs the HIP alternative on gfx1201
  installs; GGUF floor overrides (Rust + JS) are HIP-aware and never swap
  HIP↔CUDA; installer labels name both stacks
- Deepy Web start transparency + control: every start logs the exact
  `[Deepy] cmd:` line to the console (password stays env-only, never
  logged); new **Extra args** field in Step 4 (Manage-style free text,
  persisted, fail-closed validation — card-managed flags and `--share`
  refused with guidance); mode radios spell out `--listen`
  (Same-PC = no `--listen`, Phone-LAN = adds `--listen`, per upstream:
  reachable from anywhere on the LAN and by extension the VPN); boot wait
  fails fast when the child dies instead of holding "already starting"
  for 5 minutes
- Deepy checks note the AMD HIP paged-attention SDPA fallback
  (`hip_paged_sdpa`) as expected/slower, not an error

## [0.8.0] — 2026-09-21

- Launch hardening (#36): the Python bootstrap shim now lives in an
  isolated `%TEMP%\wan2gp-bootstrap-<pid>-<ms>\boot.py` subdir (Deepy:
  `wan2gp-deepy-bootstrap-*`) instead of loose in `%TEMP%`, and scrubs its
  own dir from `sys.path` — a stray `%TEMP%\grp.py` could previously shadow
  stdlib (`tarfile → import grp`) and kill startup with
  `IndexError: list index out of range`. Verified with a dummy `grp.py`
  against the real env (old layout crashes, new layout imports torch fine)
- Notifications are now native-first (#35): Manage → Notifications sets up
  Wan2GP's own Apprise destinations in `wgp_config.json` (OS credential
  store supported, test button, ntfy.sh quick-start), Install also covers
  `keyring`; the status line shows `apprise`/`keyring` versions. Saving with
  any native event on switches the launcher's log-driven sender off (no
  double pings); older Wan2GP checkouts fall back to it (now via the
  `apprise.exe` console script — `python -m apprise` never worked with the
  pinned `apprise==1.12.0`, which ships no `__main__.py`)

## [0.7.5] — 2026-09-21

- Guide topbar tab (was a Manage tab): goal-based model recommender (13
  goals, 23 curated picks with starters + copy-full-model-name for toolbar
  search), 6 prompt templates with line modes, offline `[/...]`
  window-command checker, VACE pre-flight checklist, post-processing
  at-a-glance; opens as its own side panel next to Manage
- Manage → Library: LoRA family inventory (files/size/known URLs),
  downloaded checkpoint inventory with kind tags, finetune
  list/import/export/delete (validated, never overwrites), workspace
  sizes with missing-file flags, archive-lock toggle, definitions backup
- Manage → Launch: Balanced/Low VRAM/Max perf/Emergency presets plus a
  token-aware flag builder (attention/profile/teacache/compile/fp16),
  Agent-API (MCP) command/URL builder, headless queue runner
  (`--process` with console streaming + exit codes)
- Manage → System: issue bundle gains torch/CUDA + triton/sage probes,
  staged launch args and redacted `wgp_config.json`; config snapshots on
  every Apply (newest 5, undoable restore) + upstream changelog viewer;
  Deepy engine checks, `--public-url` validator, TLS flag staging
- Side panels (Manage/Guide) shrink the native view beside them instead
  of detaching to black; panels widened to 600px; Library tables use one
  readable size with fixed checkpoint columns; Manage opens instantly
  (probes deferred past paint, plugins list lazy)
- Docs: new `docs/WAN2GP-GUIDE.md` (goal flowchart, 7 guided paths, all
  232 models / 116 settings / full processor + 44-flag inventory, 10
  Mermaid infographics) linked from README + USER-GUIDE

## [0.7.3] — 2026-09-21

- Upstream Wan2GP v13.13 (Sep 20-21): GGUF kernel floor 1.0.21 → 1.0.22
  (docs prescription over `setup_config.json` lag, verified Sep 2026;
  required for Deepy Prime Bonsai PTQ1) in Rust + JS resolvers, UI strings,
  stale-wheel diagnostics and tests; 1.0.21 now counts as stale
- Kernel Sync warns when the local checkout is behind `origin/main`
  (15s best-effort check, silent offline) — sync follows the checkout,
  so stale clones installed stale wheels with no hint before
- Deepy Prime weights check accepts the Bonsai 2 PTQ1 checkpoint
  (`Ternary-Bonsai-2-27B-Abliterated-PTQ1_0.gguf`, upstream
  `QWEN38_27B_TEXT_GGUF_PTQ1_FILENAME`) — Bonsai-only installs no longer
  misreport missing 27B weights
- VAE Config dropdown relabeled to upstream presets (auto / 16GB+ / 8GB+ /
  6GB+); auto-tune still recommends Auto, comments updated for Qwen 2.1
  tiling (1024/512/256, 25% overlap, Auto thresholds 16000/8000 MiB)
- Install plan notes: Comfy Kitchen via `requirements.txt` (same R580+
  driver, +10% H3/LTX2.x), Qwen 2.1 first-use download sizes, JIT
  on-demand pre/post checkpoints, first-H3-INT8 ConvRot VAE fetch;
  USER-GUIDE gains sync-behind warning, download notes and an
  `AUTHENTICATION.md` pointer. Plugin catalog needs no change (reads the
  checkout's `plugins.json`, so H3 Director appears after update)
- Update pin diff is environment-marker aware: `; python_version …`
  branches are evaluated against the active env's interpreter (probed,
  fallback to unfiltered), duplicate dist names resolve last-wins —
  ends the phantom `onnxruntime-gpu 1.25.0.dev… -> 1.22.0` line when old
  and new files carry both conditional branches; dep recheck filters
  inapplicable branches the same way
- Memory panel tracks upstream v13.13 kernel settings: the deleted legacy
  `enable_int8_kernels` numeric toggle is replaced by `int8_kernels`
  (`auto`/`kitchen`/`triton`/`disabled`) and `kernel_precision`
  (`fast`/`strict`) dropdowns, with fail-closed validation matching
  upstream CHOICES; apply removes the stale legacy key, read maps a
  lingering legacy value for display, auto-tune seeds upstream defaults
- Deepy panel gains a Qwen LLM Quantization selector (upstream
  `prompt_enhancer_quantization`): GGUF Q4 / IQ3_S / Q2 / Bonsai PTQ1 for
  Qwen3.8, Quanto Int8 / GGUF Q4 for Qwen3.5 — shown only with a local
  Qwen engine, engine-inappropriate values normalize to the engine
  default, absent choice preserves existing config; choosing Bonsai
  also sets INT8 KV cache, and any Deepy apply sets Automatic prompting
  (the one global switch upstream offers; template flags untouched)
- Startup drift sync: new `dep_check` command reuses the post-update pin
  comparison so the dashboard reconciles the drift banner with the live
  env once at startup (stale banner clears, real drift names itself)
- Post-install override sync: after successful `setup.py`, install swaps
  the two wheels it can never produce itself (GGUF floor, sage safe
  build) so fresh installs don't land stale; warn-only, honors the
  sageSafe setting, `setup.py` untouched
- Readability: small hint/micro text across all panels +1px (0.5→0.5625,
  0.5625→0.625, 0.6→0.6625, 0.625→0.6875, 0.6875/0.7→0.75rem, incl.
  inline HTML sizes); body 0.75rem and the terminal's own slider scale
  untouched

## [0.7.2] — 2026-09-18

- Deepy Web and Wan2GP exits are now independent events: the backend emits
  `deepy-exit {source, code, port}` instead of reusing `wangp-exit`, so
  stopping one server no longer tears down the other's view; new Stop-Deepy-only
  button (keeps Wan2GP running) and the topbar LED follows the server, not the view
- Upstream Wan2GP 13.11 reverse-proxy support: Deepy Web card gains an Advanced ·
  Reverse proxy field (`--public-url` exact origin, validated fail-closed,
  persisted to config, rejected together with the HTTPS redirect port —
  proxy-managed HTTPS vs WanGP redirect are exclusive upstream too)
- Xet Storage card is version-aware: shows installed vs the live
  `requirements.txt` pin (e.g. `installed 1.6.0 (requires >=1.5.2)`), turns red
  with an Update action when below the floor, and Install/Update always resolves
  the live pin instead of a hardcoded floor
- Housekeeping: retired the stale `reference/` snapshot (20 files, diverged
  pre-Tauri copy) and the checked-in `.vscode` config

## [0.7.0] — 2026-09-16

- Appearance: 5 full themes (Mono default + Blue Sky + Orca + Cyber + Matrix) —
  every dot click-to-apply, double-click to edit 3 colors (accent/background/text)
  with live preview, Save theme / Set as default / Reset to shipped colors;
  palette-cycles from the topbar, Text size (85–130%) + Terminal size (85–150%)
  sliders in Manage → Appearance (persisted, console window follows)
- Dashboard info text raised to readable minimums (spec rows, hints, metrics,
  micro-labels — nothing below 0.625rem except table micro-notes)
- Save-As dialogs show real file-type filters (issue #29: ZIP/JSON no longer
  `All Files`, extension preserved on rename) in both embed paths + log export
- Wan2GP update ends with a one-line verdict (upstream commit + local-edit count
  - untouched personal files) so users can tell at a glance the checkout is
  100% original git
- Deepy Web URLs are click-to-open in the real browser (login must happen in a
  tab — embedded views 403); QR captions/tooltips say what works where
  (same-Wi-Fi vs off-LAN Tailscale/VPN); LAN HTTPS tucked under Advanced
- Deepy Web Assistant selector (Zero/Prime per start, applied via deepy_set);
  local-Prime-without-27B no longer silently downgrades to Zero (fail-closed
  error instead) and the enhancer-fix maps stored profile ids to UI ids
- Standing rule: Wan2GP originals never modified — the retired `web.py`
  Referrer-Policy patch was reverted to pristine upstream (Chrome login fixed
  upstream in 38d4a64 instead); companion login-fix plugin removed entirely

## [0.7.1] — 2026-09-17

- Deepy Web no longer overwrites a saved Deepy Prime engine with OpenCode: a
  Prime per-start override now keeps the saved engine (Qwen3.8 27B local,
  Claude, Codex), falls back to the first installed engine only when nothing
  is saved, and blocks with an actionable error when the saved engine is not
  installed — instead of booting into `opencode serve` FileNotFoundError
- Local Prime 27B weights probe mirrors upstream: checks the
  `Qwen3_8_27B_Uncensored` assets folder under `checkpoints_paths` for a
  known text GGUF (Q4_K_M / IQ3_S / IQ2_M) instead of only scanning
  `llm_engines.profiles` paths the launcher never writes — installed 27B
  weights are now recognized at Deepy Web start

## [0.7.4] — 2026-09-21

- Prompt enhancement is its own dashboard card (below Active Environment, above Deepy) with an Enhancement UI picker — **Enhance Prompt button** (new default) or **Automatic dropdown** — effective with or without Deepy; both Apply buttons share one coherent `wgp_config.json` write and enable/disable together
- Local model selector unlocked for Deepy Disabled: all five models (Florence 2 + Qwen3.5 4B/9B + Qwen3.8 27B) selectable with the Qwen quantization picker included — the enhancer runs standalone without the assistant (Qwen was grayed out before)
- Fresh Deepy applies default to the Enhance Prompt button (`enhancer_mode=1`) instead of forcing Automatic; an existing choice is preserved, and the toggle now applies in every mode (Disabled included)
- Disabled apply message no longer tells you to click "Ask Deepy"

## [0.6.9] — 2026-09-15

- Deepy panel gains a Sessions section (mirrors Ask Deepy → Settings → Sessions): multisessions mode (Disabled / selectable / dedicated workspace), reset behavior, gallery media (keep links vs copy into session) — saved via Apply into `wgp_config.json`; the launcher default is now the selectable shared workspace (one outputs folder) instead of dedicated
- Deepy Web Addresses gains an External row: the Tailscale IPv4 URL (e.g. `http://100.112.0.220:7862`) with Copy + QR for off-LAN access — shown only while Tailscale is active, and the per-IP list no longer duplicates it; address rows share one grid so labels/URLs/buttons align in columns
- Phone URL now prefers the real LAN address (it picked the Tailscale IP when that enumerated first); Tailscale-only setups still fall back to it
- Deepy and Deepy Web actions leave `[Deepy]` traces in the Console (start/stop/preflight/cert/apply); backend lifecycle lines retagged `[Deepy]`
- Passphrase generator minimum is 4 characters (was 8)
- Deepy Web no longer outlives the launcher: deepyPort joins the app-close sweep and fast shutdown cleanup; Outputs-folder opener command added

## [0.6.8] — 2026-09-15

- Dependency drift after update is now unmissable (issue #23): a toast plus a persistent dashboard banner with a working Restore-now action, instead of a single log line that was easy to miss
- Drift banner pinned above the Console with hover explanations; it auto-hides on a clean update and when the Active Environment restore succeeds (no more stale Restore/Dismiss)
- Every dashboard button (150/150) now has a hover tooltip naming what it does plus scope/consequence
- Uninstall Wan2GP streams live per-file removal progress to the Console (was silent until done)
- GPU Kernel Wheels card gains Restore GPU Wheels next to ↻ Update GPU Wheels: Update installs upstream's wanted set plus launcher fixes (safe Sage build, GGUF 1.0.21 floor), Restore reinstalls deepbeepmeep's pure setup_config set
- Background launcher update check leaves a Console trace when up to date (`[*] Launcher vX is up to date`), instead of only a 3s banner flash
- Yellow first-boot bar is refcounted across all launch paths: a failed side-click during a pending boot no longer hides it (no more stuck "Starting…" with no bar)

## [0.6.7] — 2026-09-14

- Update no longer dies on untracked files colliding with incoming upstream adds (0.6.6 report: a local `shared/gradio/import_files.pyi` blocked the pull): byte-identical strays are removed so the merge lands them tracked, differing ones move to `.launcher-update-backup/` — nothing deleted, pull error now names this cause

## [0.6.6] — 2026-09-14

- Manage → AMD ROCm section with MIOpen-disable toggle (binds the `amdEnv.miopenDisabled` backend key; unsets `MIOPEN_FIND_MODE` at AMD launch when on)
- Updater v2: fetch-first with incoming-change count, dirty trees auto-stash (`launcher-update-autostash`) around the pull with recoverable pop, step-named errors (no more merge-abort dead end / `Update failed: undefined`)
- Dashboard Verify / Repair Wan2GP files: read-only drift report plus tracked-only repair (recoverable stash + `reset --hard FETCH_HEAD`, never `git clean` — settings/models/envs untouched, Pinokio refused)
- Roll back Wan2GP update: every update/repair/clone records the upstream commit (`wangpCommit`); one-click return with dirty-tree and hash-shape guards, shallow-clone safe
- Reinstall preserves `desktop-config.json` (tokens, launchArgs, prefs) via the backup set with copy/aside semantics; skipped only on explicit wipe-without-backup
- Prerequisite probes unified: astral/cargo uv homes in the candidate tables plus PATH refresh before probing (no more “UV not found” vs “already installed” loop)
- Update shows the full requirements check: per-pin `✓`/`✗` lines plus `a -> b` / `+` / `-` pin diff in the log and result
- Console tags upstream child lines `[wan2gp]` (launch + setup streams; progress/profile parsers still read raw text)
- XSS hardening: http(s)-only external-link/iframe guards plus ~40 `innerHTML` sites converted to DOM building (tsStatus now text-only)
- Menu reorder: Verify/Repair + Roll back grouped after Create Desktop Shortcut; metrics keep sampling while Wan2GP runs embedded (was frozen when `dashBody` hid)
- Status Pro self-healing retired: no longer auto-uninstalled on existing installs, never auto-installed — plugins install only from user-starred favourites

## [0.6.5] — 2026-09-13

- mmgp 3.8.0 fallout fixes: no-`wt.exe` terminal launch passes an empty `start` title (a quoted title was executed as a program — "Windows cannot find 'Wan2GP-Launcher-…'"), the quick pip box redirects `-r`/`-e`/flag/`requirements.txt` input to the restore button instead of installing a literal filename, Update auto-reinstalls `requirements.txt` when the pull changes it (result carries `requirements`/`depCheck`/`drift`, dashboard narrates reinstall/failure), a post-update one-probe `importlib.metadata` recheck warns on version drift with a restore pointer (never fails the update), Status Pro is uninstalled by default (temporarily incompatible with the current Deepy update — removed from existing installs, reinstall manually once fixed); Install-button feedback (Working…/Installing… locks, Measuring… log line, backend wipe-phase lines, no double-press); env panel gains MMGP/XFormers/Torchaudio/MoviePy rows; first-launch yellow bar self-repairs (repainted from server state on every dashboard refresh)

## [0.6.4] — 2026-09-13

- Ordered app-close: first X starts session shutdown and the window stays; tracked PIDs plus terminal window die synchronously (milliseconds), a worker then runs the full verified sweep (WMI plus port scans catch old/orphaned sessions) and closes the window when done; second X or a 30s watchdog forces the exit. A final close-port sweep re-checks Wan2GP-port listeners and kills only provably-ours processes (tracked child or repo-signal cmdline; foreign/unverifiable PIDs are spared and logged).
- Launch modes: Desktop hero button plus a 2x2 grid — Browser, Browser No-GPU, Terminal, Terminal No-GPU. No-GPU opens the selected default browser with GPU acceleration disabled (Chrome, any Chromium, Firefox, OS default) so the browser frees VRAM; a hint appears when no supported browser is found.
- Terminal launch fix: the external-terminal server command now carries the bootstrap shim instead of a hardcoded script name (all terminal modes died with unrecognized-arguments).
- Native downloads traced per-download: Requested assigns [dl #id], Finished reaps it (unpaired finishes mint a fresh id and say so), so a staged path reused after Save-As never collides; both events also go to the backend log bus.
- Manage Debug: verbose embed-bounds logging toggle (off by default; takes effect immediately, no restart).
- AMD driver label fix: the 32.x store branch reads as current (Adrenalin 24.x-26.x+); pre-2024 drivers still warn (issue #15).

## [0.6.3] — 2026-09-12

- AMD easy-mode installer fixes (issue #15 follow-up): per-family `rocm[devel]` doc-recipe torch float (`/v2/<fam>/`, v2-staging fallback), full ROCm launch env derived from the installed env (`ROCM_HOME`, LLVM/bin PATH prepend, `CC`/`CXX=clang-cl`, `DISTUTILS_USE_SDK=1` — set-if-absent, never invented), `attention_mode` defaults to `"auto"` after AMD setup when missing/empty **or holding setup.py's bogus `'sage'`/`'sage2'` default** (upstream picks it with `if "20" in profile_key`, which matches `"AMD_GFX1201"` via the "1201" substring — every fresh AMD config was born `sage` with no sage backend installed; deliberate values like `sdpa`/`flash` stay untouched), package installs refuse CUDA/bitsandbytes/vanilla triton/vanilla spas_sage_attn/sdist flash-attn on AMD with a docs/AMD-INSTALLATION.md pointer (vanilla `triton` maps to `triton-windows`; Manage hides those add buttons on AMD), and AMD verify logs torch + HIP versions via `collect_env`
- Intel/AMD pipeline separation + wheels-only triton-windows on AMD: non-AMD launches now reconcile stale AMD session env (`ROCM_HOME`/`CC`/`CXX`/`DISTUTILS_USE_SDK`/ROCm flags removed unconditionally with `[i]` logs, own ROCm PATH prepend stripped — compiler vars no longer leak into later Intel/CPU/NVIDIA pip builds in the same process; HSA handling unchanged, Intel path gains zero new env behavior), and AMD installs add `triton-windows` from the official PyPI wheel after setup (vanilla `triton` uninstalled first on the namespace conflict; warn-only — SDPA still works per docs/AMD-INSTALLATION.md). OFFICIAL WHEELS ONLY throughout: anything without an official wheel stays refused by the AMD package gate, no build-from-source logic anywhere
- AMD attention default also repairs setup.py's `'sage'` artifact (first bullet) instead of missing/empty only; the `upgrade_package` path runs the same AMD package gate as installs (no bypass via upgrades; vanilla `triton` upgrades map to `triton-windows`); test executable stamped `0.6.3-amdtest1` (version files only — reverted for the 0.6.3 release)
- AMD follow-up batch (issue #15): stale env dirs missing `pyvenv.cfg` are detected before reuse (removed with a naming log line; locked residue fails with an actionable message instead of setup.py exit 106, and a 106 now maps to repair directions rather than the generic error), TheRock torch stays a single setup_config-spliced command (the splice is one `{pip}` argv string with no shell chaining, and setup.py drives it via `uv pip` whose resolver differs from plain pip — pinned by test), ROCm SDK discovery prefers the `rocm-sdk path --root` CLI (`rocm-sdk` binary, then `<env python> -m rocm_sdk`) with dir-guessing fallback plus warn-only `rocm-sdk init` / `test`-tail capture after AMD setup, an installed `onnxruntime-gpu` now satisfies the Manage `onnxruntime` row (display mapping only — the installed package is untouched), exactly-one-AMD-GPU-plus-virtual-adapter boxes pin `HIP_VISIBLE_DEVICES=0` set-if-absent at AMD launch (multi-AMD and no-virtual-adapter boxes do nothing; the key also reconciles away on non-AMD launch), and a Manage-backed `amdEnv.miopenDisabled` key (default false, backend-only for now so the frontend can bind it later) leaves `MIOPEN_FIND_MODE` fully unset when true with an `[i] MIOpen disabled by user setting` log line (Manage checkbox binds it as of Unreleased)

## [0.6.2] — 2026-09-11

- Windows long paths handled end to end: preflight warns when `LongPathsEnabled` is off, Troubleshooting offers a one-click enable (permission confirm, UAC elevation, reboot reminder), and Install gates on it before any download with an enable-then-reboot choice
- Gallery no longer wipes every generation: `write_wgp_config` seeds `clear_file_list=5` when absent (missing key used to resolve to 0), so results keep the last generations by default
- Installer stops asking twice: backup-choice stash reconciled against the live radio pick at dispatch, verdict refresh preserves radio picks on same-mode refreshes, foreign-folder fallthrough asks once via confirm instead of silently bouncing

## [0.6.1] — 2026-09-11

- Native-view drag & drop fixed: the child webview opts out of Tauri file-drop interception (`disable_drag_drop_handler`) so drops reach Gradio
- Closing the launcher now actually stops servers: `shutdown_cleanup` runs the sync blocking stop instead of dropping an async Future, so no orphaned Wan2GP/OpenCode processes survive app close
- Stop hardening: OpenCode :4096 port-sweep fallback across restarts, custom-port python-listener sweep, unique per-launch terminal scripts + temp cleanup

## [0.6.0] — 2026-09-11

- Desktop embed goes native (experimental, now default): Gradio renders in a real child Webview instead of an iframe — switchable anytime via the topbar `Renderer: native/iframe` dropdown (locked while a session runs; dashboard + Manage mirror it). Native brings browser-grade downloads (staged bytes → native Save-As dialog with the gallery's real filename, cancel keeps Downloads), compositor zoom, exact bounds sync below the topbar, and `download-started/finished` events instead of Downloads-folder polling
- Floating console is its own window in native mode: the child composites above all DOM so overlay is impossible — the separate always-on-top console window floats over visible Gradio, with history + live logs, Follow/search/export, and dock buttons that switch back to side-by-side docks; iframe mode keeps the DOM overlay
- One console stream everywhere: main-side lines mirror through the backend bus (history + live), so floating/docked/dashboard consoles read identically with no duplicates
- Stop no longer stalls the UI: async off the invoke pool, one PowerShell call for all ports, live `[stop] …` progress, self-disabling button — and the ground-truth sweep covers the configured port plus 7860/7861, so rebuild orphans from killed launchers die too (survivors stay reported, never hidden)
- View-transition mutex (no more dashboard-with-topbar mixed states from fast Back-and-forth), Stop tears down view state immediately (stale Back-to-Desktop gone), server-exit close stays silent (no more phantom "still running"), stale-renderer guard rebuilds hidden-alive views after a mode switch, transition-safe terminal guards, `Measure WebView2 memory` one-shot in Manage → Launch
- AMD install no longer depends on patching upstream `setup.py` (#15: RX 9070 XT got a silent 20-minute CUDA install after both source patches refused on drift — `Unknown` → `RTX_40`, 8 GB VRAM default). The launcher now drives `setup.py` through a hook module that imports it, applies the launcher verdict (profile key validated against the cloned `setup_config.json`, launcher VRAM, conda-direct-pip) and calls `do_install_auto()` directly — upstream `setup.py` is never modified. Fail-closed throughout: a failed TheRock entry patch or hook staging aborts before any download, a profile mismatch in the streamed output kills the child immediately, and hook exit-2 maps to a report-it hint instead of a blind retry
- Hook corrections from 0.6.0 testing: fresh-uv installs drive setup via `uv run --with setuptools python …` — the hook now replaces only the `setup.py …` tail, keeping the interpreter prefix (first build passed the bare hook path to uv itself → exit 2); RDNA 2 (`AMD_GFX103X`, no upstream key) aliases to the compatible `AMD_GFX110X` entry instead of risking `Unknown` → `RTX_40` on Win11, while Intel/CPU keeps setup.py's own detection

## [0.5.3] — 2026-09-09

- Healthy-state trio routed through Install too: Update / Reinstall-fresh / Use-existing as radios above the button; backup modal is collect-only, one adaptive confirm per run, Install is the sole launcher everywhere
- Install button can no longer resurrect mid-install (Browse during a fresh install re-trips the verdict — guarded)
- New user guide: docs/USER-GUIDE.md covers every screen, tab and button
- Conda pip routing fixed: `conda run` re-quotes args and corrupts URL-encoded wheel URLs (SpargeAttn %2B → Invalid build number) — setup.py's conda install template is patched to drive pip with the env interpreter directly (venv shape, own marker, drift-refusing)

- Conda ToS gate handled: Anaconda's 2024+ Terms-of-Service enforcement refuses non-interactive channel ops — the installer accepts pkgs/main, pkgs/r, pkgs/msys2 once (transparently logged, persists in conda config) before setup.py runs

- Fresh conda installs work: env_conda doesn't exist yet when setup.py runs (it creates it in step 1/3), so `conda run -p` died with EnvironmentLocationNotFound — setup.py is now driven by system python on fresh installs (existing envs still run through `conda run`), with conda's own bin dir scoped onto PATH so its `conda create` is found; honest error when no system Python exists

- Viewer is now browser-parity for media flow: drag & drop into Gradio dropzones works (Tauri's native file-drop interception disabled via `dragDropEnabled: false` — it was swallowing drops before Gradio saw them)
- Gallery downloads are no longer silent: each fresh arrival pops a Save / Save As… prompt — Save keeps it in Downloads, Save As… opens the native dialog (reopens at the last-used folder)
- Conda env parity: interpreter resolution now knows conda's layout (python.exe at the env root, no Scripts dir) — launch, package install/upgrade/uninstall, requirements restore, update checks and the install smoke test all share one resolver, so conda works the same as uv/venv instead of blocking launch with "can't import torch" on a healthy env (0.5.2 report: RTX 4080 SUPER, env_conda)
- No-env setup is now a checklist: repo-without-env states show tick-one-of-two radios (Install / repair environment vs Fresh repo) and the big Install button starts the checked choice — no more competing action buttons under a dead Install
- Empty Active Environment card offers Run Setup directly after unlinking the last env, instead of a dead end
- Preflight antivirus warning is AMD-only (observed risk: quarantined nightly wheels); NVIDIA/Intel stay on the reactive missing-DLL hint
- No-restart prerequisites: git/conda/python/py resolve to absolute known locations (plus the owned uv copy), so freshly installed tools work instantly — no PATH refresh, no launcher restart; gates and every spawn site use the same resolver
- Launcher-owned uv: the installer provisions `uv` into its own `<dataDir>\.tools` via the official standalone installer (`UV_INSTALL_DIR`, keeps the install receipt so `self update` works) and prefers it for every later run — PATH copies belonging to other apps (0.5.3 report: Hermes agent's bundled uv, self-update hit its file lock) are fallback only, never self-updated; offline last resort is a plain copy (works, but receipt-less until a proper install replaces it)

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

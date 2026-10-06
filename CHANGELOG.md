# Changelog — Wan2GP Desktop Launcher (Tauri)

All notable changes. Dates are release dates; `Unreleased` tracks `master`.

## Unreleased

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

# Cross-Platform Launcher - Todo

Legend:
- `[x]` done and verified by running it here (Windows natively, Linux under
  WSL2 + Xvfb — no GPU, no display server, software GL).
- `[x]*` written and type-checked for the platform (the `cfg` arms were made
  live on this Windows host with `%TEMP%\opencode\prep-unixcheck.ps1`, then
  clippy'd with `--all-targets -D warnings`), but never run on that OS.
  Treat as "ported", not "working".
- `[ ]` not done.

Current suite: 419 Rust tests (+1 ignored), 212 JS tests, clippy + fmt green on
Windows; the same suite green on Ubuntu 26.04 (WSL: 400 Rust — the difference is
`cfg(windows)`-only tests), clippy + fmt green there too. Both Unix flavors
clippy-clean with `--all-targets`. The suite is host-independent since the
`*_on(…, windows)` split (see Phase 7).

## Phase 0 - Baseline and safety
- [x] Add CI job: Windows
- [x]* Add CI job: Ubuntu (executed — run 40281177754, see Phase 4)
- [x]* Add CI job: macOS (configured, never executed)
- [x] Add frontend capability map (`src/services/platform-gate.js`)
- [x] Add feature matrix to README and `docs/PLATFORMS.md`
- [x] Keep Windows build unchanged (behaviour identical; suite green)
- [ ] Tag current Windows release as `v0.10.25-windows-stable`

## Phase 1 - Platform adapter skeleton
- [x] Create `src-tauri/src/platform/mod.rs`
- [x] Create `src-tauri/src/platform/windows.rs`
- [x] Create `src-tauri/src/platform/linux.rs`
- [x] Create `src-tauri/src/platform/macos.rs`
- [x] Create shared `src-tauri/src/platform/unix.rs` (lsof/ps/pgrep helpers)
- [x] Add `mod platform;` in `lib.rs`
- [x] Add trait + factory (`current()`)
- [x] Move Windows logic into the Windows adapter (GPU, process signature, port owner, kill, open path, shortcut, auto-start)
- [x] Move Unix logic into the Linux adapter (same set)
- [x] Add macOS adapter (system_profiler, ps/lsof, open, kill, LaunchAgent)
- [x] Move the Windows-only branches that belong on the trait: install prerequisites, task manager, launch close sweep. Browser launch was already portable (`tauri-plugin-opener`).

## Phase 2 - Migrate subsystems
- [x] Migrate `hw.rs` GPU detection
- [x] Migrate `launch.rs` process signature
- [x] Migrate `launch.rs` port scan/close sweep
- [x] Migrate `troubleshoot.rs` port status
- [x] Migrate `troubleshoot.rs` port fix
- [x] Migrate `system.rs` open path
- [x] Migrate `system.rs` desktop shortcut
- [x] Migrate `system.rs` task manager
- [x] Migrate `install.rs` prerequisite detection
- [x] Migrate `features.rs` auto-start
- [x] Remove the cfg blocks the migrations replaced
- [x] Run clippy/fmt/tests after each migration (final state green)

Remaining `cfg(windows)` pairs are install-flow internals, not adapter
territory: shell-script strings, wheel/channel choices and path layouts live in
`install.rs` (79), `features.rs` (14), `status.rs` (14), `hw.rs` (11),
`base.rs` (8), `launch.rs` (6), `troubleshoot.rs` (6), `system.rs` (2). Both
arms of every pair compile; the Unix arm is what the flavor check exercises.

## Phase 3 - Linux
- [x]* Document Linux runtime dependencies
- [x]* Implement Linux NVIDIA detection (shared `nvidia-smi` probe)
- [x]* Implement Linux AMD detection (`lspci` + `/sys/class/drm` for VRAM)
- [x]* Implement Linux Intel fallback
- [x]* Implement Linux prerequisite detection (`which` + known paths)
- [x]* Implement Linux `.desktop` entry (quoted `Exec=`; spaces in the install path)
- [x]* Implement Linux auto-start entry (`~/.config/autostart`)
- [x]* Update docs for Linux (`docs/PLATFORMS.md`)
- [x] Gate unsupported Linux UI (DLSS card, Task Manager button, Triton/MSVC row)
- [x] Configure Linux bundle targets (`appimage`, `deb`); built AppImage 85.0 MB + deb 7.25 MB in CI
- [x]* Compile for `x86_64-unknown-linux-gnu` on a Linux host (`build-artifacts.yml`, run 40279609160, ubuntu-22.04)
- [x]* Launch the Linux build by hand and exercise the wizard: window (WSLg
      native window and Xvfb both), full UI, prerequisites detected (git 2.53,
      uv 0.13.5 from `~/Wan2GP/.tools`, Python 3.14), CPU/RAM probe, path
      rendering, install-location verdict. WSL2 passes the host GPU through
      (`/usr/lib/wsl/lib/nvidia-smi`), so detection was real: RTX 3080, 10240 MB,
      driver 617.42 → CUDA 13 (cu130) → profile RTX 30
- [x]* Install both bundles by hand: AppImage (needs FUSE or
      `--appimage-extract-and-run`) and `.deb` (`dpkg -i`, `/usr/bin` binary, menu
      entry) both start on Ubuntu 26.04
- [x] Kernel-wheel sync on Linux resolves each component through the host's
      `cmd.linux` entry (`triton>=3.6,<3.7`, `…-linux_x86_64.whl`, `{py}` →
      `cp311`) — verified against the live `setup_config.json` on WSL2 Ubuntu
      26.04, pip accepting all five resolved commands; it used to install
      Windows wheels and ask PyPI for `triton-windows`
- [ ] Implement Linux install flow end to end (clone + env + torch never run)
- [ ] Implement Linux update flow (never run)
- [ ] Verify launch/stop on Linux (needs a real env, so it follows the install)
- [ ] Verify close sweep on Linux
- [ ] Test on Ubuntu with NVIDIA
- [ ] Test on Ubuntu/Fedora with AMD

## Phase 4 - macOS (Apple Silicon only)
- [x]* Implement Apple Silicon detection via `system_profiler`
- [x]* Return vendor `APPLE` and profile `MPS`
- [x] Implement RAM reporting (cross-platform `sysinfo::total_memory`, no per-OS code needed)
- [x]* Report unified memory as VRAM (`sysctl -n hw.memsize`, 8 GB fallback) — was hard-coded 8192 MiB
- [x]* Implement macOS prerequisite detection (`which` + known paths)
- [x]* Replace `/proc` signature logic with macOS `ps`/`lsof`
- [x]* Use `lsof` for port owner
- [x]* Implement macOS LaunchAgent auto-start (label `com.wan2gp.desktop`)
- [x]* Show Apple MPS in the installer UI (`installer-tab.js` maps `MPS` to "Apple MPS")
- [x]* Update docs for macOS
- [x] Gate unsupported macOS UI (DLSS, kernel wheels, Task Manager, MSVC row, desktop shortcut)
- [x] Configure macOS bundle targets (`app`, `dmg`, minimumSystemVersion 11.0); built dmg 5.96 MB in CI
- [x]* Compile for `aarch64-apple-darwin` on a macOS host (`build-artifacts.yml`, run 40279609160, macos-latest)
- [ ] Implement macOS install flow (MPS profile, never run)
- [ ] Implement macOS update flow (never run)
- [ ] Verify launch/stop on macOS
- [ ] Verify close sweep on macOS
- [ ] Test on real Apple Silicon Mac

## Phase 5 - System integration
- [x]* Linux `.desktop` entry
- [x]* Linux optional auto-start
- [x]* Linux notifications (via upstream Python)
- [x]* macOS optional LaunchAgent auto-start
- [x]* macOS notifications (via upstream Python)
- [x] Decide desktop shortcut strategy for Linux/macOS (Windows `.lnk`, Linux `.desktop`, macOS none)
- [x]* Port owners: `ss -ltnp` first, `lsof -Fpcn` fallback (`platform/unix.rs`, unit-tested parsers)

## Phase 6 - Updater and release
- [x] Define Linux update strategy (AppImage self-update; `.deb` is manual, `docs/PLATFORMS.md`)
- [x] Decide macOS updater question: **off** until an Apple identity exists — an unsigned `.app` cannot be replaced safely, so the workflow sets `createUpdaterArtifacts: false` there
- [x] Document every update lane: `docs/UPDATING.md` (six install kinds, asset
      names, per-platform steps, in-app state machine) + a README pointer
- [x] winget + Homebrew Cask definitions in `packaging/`, submitted by
      `.github/workflows/package-managers.yml` — called from `release.yml`
      (a `release: published` event from `GITHUB_TOKEN` starts no workflow), and
      gated on their own tokens so a missing secret skips instead of failing
- [ ] Publish the manifests (first submission is manual: winget-pkgs review, and
      `GKartist75/homebrew-tap` must exist) — needs `WINGET_TOKEN`/`HOMEBREW_TOKEN`
- [ ] Configure Apple signing/notarization (needs Developer ID credentials; deferred in `plan.md`)
- [x]* Build all platforms in one release workflow (`.github/workflows/release.yml`)
- [x] Upload all artifacts to one GitHub Release — v0.10.26 (run 40367092578) is the first
      release built by the workflow: AppImage/deb/msi/nsis/dmg + `.sig` + `latest.json`
- [x] Update changelog with platform support/limitations

## Phase 7 - Testing and validation
- [x] Add unit tests for detection parsing
- [x] Add unit tests for profile resolution
- [x] Add unit tests for adapter capability maps and unsupported behavior (`platform::tests`)
- [x] Add integration tests with tempdirs
- [x] Add integration tests with fake commands (`WGP_PROBE_*`)
- [x] Add integration tests with fake hardware probes (simulated R9700, dual NVIDIA)
- [x] Add frontend gating tests (`tests/platform-gate.test.js`: DOM toggling, fail-closed names, script order, and a pin against `platform/mod.rs`)
- [x] Run full CI on Windows (locally)
- [x] Type-check both Unix flavors on this host (clippy `--all-targets -D warnings`)
- [x] Make the pins testable on any host: `cmd_for_platform_on`,
      `triton_component_spec_on`, `post_install_override_urls_on`,
      `sage_safe_url_on`, `sage_needs_safe_swap_on`,
      `hardware_profile_detail_with_ram_on` — the seven Windows-only assertions
      that would have failed the Ubuntu job now pin the table they mean and
      assert the other host's answer beside it
- [x] Document known limitations
- [x]* Run full CI on Ubuntu (run 40281177754: JS 201 + Rust 399 tests, clippy, fmt green)
- [x]* Run full CI on macOS (run 40281177754: same jobs, green)
- [x]* Manual smoke test: Linux (WSL2 + Xvfb, wizard + bundles only — see Phase 3)
- [x] Manual smoke test: Linux NVIDIA (WSL2 + real RTX 3080 passthrough: install to completion, WGPKernels/nunchaku/GGUF loaded, `Running on local URL: http://localhost:7861`).
  Re-run on the shipped **0.10.28 `.deb`** (Ubuntu 26.04, WSLg): `env_uv` 9.1 GB, `torch 2.10.0+cu130 cuda=True`, SageAttention 2.2.0 wheel, smoke test passed, ZImageTurbo + Qwen3 + VAE downloaded (11 GB), WGPKernels INT8 autotune ran (660x1024x3840 → 651 µs), `/` and `/deepy/` both 200. SpargeAttn/flash-attn skipped: no `nvcc` on the box (the CUDA toolkit row reads *optional* for exactly this reason).
- [ ] Manual smoke test: Linux AMD
- [ ] Manual smoke test: macOS Apple Silicon

## Session 2026-10-26 — Desktop embed on Linux, console docking

- [x] Explain the half-window Desktop view: `add_child` on GTK puts the child
      webview in the window's layout box, so the launcher page is squeezed to
      the top half (window 1032px → launcher webview 493px, child 492px) — the
      "black band" was the launcher renderer's unused lower half, and the
      console docked to the bottom of *that* half.
- [x] Add `Capability::NativeEmbed` (Windows true, Linux/macOS false) + JS
      `nativeEmbed` in the pinned capability map.
- [x] `embed_mode()` returns `iframe` wherever native cannot composite, so a
      saved `embedMode: native` cannot reproduce the split.
- [x] Renderer pickers (topbar + Manage → Launch) show `iframe` and disable the
      `native` option ("(Windows only)") on those hosts.
- [x] Console header wraps: 340px side docks used to push the last dock buttons
      (`Floating`, `✕`) past the right edge, making a right-docked console
      impossible to switch back.
- [x] Verified on WSL2/Ubuntu 26.04 (real RTX 3080, WSLg) by driving the app
      with xdotool and measuring getBoundingClientRect from inside the page:
      iframe renderer, webview = full window (1908×999), embed 1908×955 under
      the 44px topbar, and dock bottom/left/top/right/floating each shrinking or
      restoring the embed (200px / 340px strips) with no overlap.
- [x] Environment panel: Triton showed "—" on Linux because the version scan
      only asked for `triton-windows`. Upstream INSTALLATION.md#linux: torch
      installs the matching plain `triton` (3.3 with torch 2.7, 3.6 with 2.10).
      Confirmed installed here (3.6.0) and now displayed; the probe asks for the
      host's dist name first with the other as fallback.
- [ ] Re-run the same dock cycle on Windows with the native renderer (the child
      webview ignores CSS padding; `__syncNativeBounds` subtracts the dock strip
      from the measured rect — unchanged this session, unverified today).

## Session 2026-10-26 (2) — update lanes, packaging, host-independent tests

- [x] Launcher update UI verified live on WSL2/Ubuntu 26.04 for the `source`
      kind: with the harness copy at 0.10.24 and the release at 0.10.25 the
      dashboard control reads **v0.10.25 — Releases** (`has-update` ring), no
      toast, no plugin error, and **Manage → Updates** reads *v0.10.25 available
      — this copy runs from a build directory — git pull, then rebuild*; the
      copy-command button correctly stays absent (a `source` copy has no
      command). The `.deb` lane's copy button is covered by
      `tests/launcher-update.test.js` (`commandFor`, `statusLine`) rather than
      the screen — Tauri's CSP blocks in-page `eval`, so a synthetic deb render
      could not be injected into the running app.
- [x] Rust suite green on both hosts after the portability split (419 Windows /
      400 Ubuntu), clippy + `cargo fmt --check` green on both.

## Known gaps
- The Linux AppImage built on `ubuntu-22.04` bundles that image's WebKitGTK and
  aborts at startup on hosts with a recent Mesa (`Could not create default EGL
  display: EGL_BAD_PARAMETER`, reproduced on WSL2/Ubuntu 26.04). The `.deb` uses
  the host WebKitGTK and starts fine there. Bundles moved to `ubuntu-24.04`.
- GitHub's release secrets were stale: `TAURI_SIGNING_PRIVATE_KEY` did not match
  the password, so the first v0.10.26 run failed both signed legs
  (`failed to decode secret key: incorrect updater private key password`).
  Re-uploaded from `~/.tauri/wan2gp-desktop.key`.
- Linux runs (WSL2, Ubuntu 26.04, real RTX 3080): app starts, wizard reads the
  machine, install completes (Torch 2.10.0+cu130, SageAttention wheel, Triton
  JIT, WGPKernels, nunchaku, GGUF), WanGP launches and serves Gradio on 7861
  with CUDA kernels active. Still never run on Linux: stop/close sweep edge
  cases, AMD detection (no AMD hardware here) and the updater. macOS has never
  been executed at all — nothing is known beyond "it compiles".
- SpargeAttn and flash-attn are skipped on Linux/macOS without a CUDA toolkit
  (upstream builds both from source there; Windows gets wheels). The launcher
  detects a native `bin/nvcc`, exports `CUDA_HOME`, and ignores toolkits under
  `/mnt/c` — `nvcc.exe` via WSL interop cannot build Linux extensions.
- AMD on Linux has never been installed: the TheRock torch path, its `triton`
  handling (now Windows-gated) and the AMD package gate's Linux arm are
  unverified there.
- Upstream's `run.sh` restarts WanGP when the process exits with code 42 (its
  in-app "Restart" button). The launcher has no such loop, so that button just
  stops the app and Start must be pressed again — Windows behavior too, not a
  cross-platform gap.
- NVIDIA on Linux without `nvidia-smi` (nouveau) reports `unknown`, so the
  installer picks a CPU profile.
- A host with neither `ss` nor `lsof` cannot attribute ports; the stop sweep
  reports an explicit error instead of an empty list (no `fuser` fallback).
- `bundle.targets: "all"` produced NSIS/MSI locally and AppImage/deb/dmg in CI;
  the `.dmg` is unsigned and the `dist/` bundles are update-incapable (no
  updater artifact).
- macOS: no code signature, no notarization, no updater artifact.
- The release workflow has never run, and the repository has **no secrets**, so
  `release.yml` would fail its Windows/Linux legs until
  `TAURI_SIGNING_PRIVATE_KEY` + `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` are added.

## Upstream sync

- [x] Audit WanGP `9905f0d` (18.83 / mmgp 4.4, mapped RAM) against the launcher:
      no CLI flag, no `wgp_config.json` key, no `requirements.txt` pin changed;
      mmgp is vendored so the version bump needs no pip step; every Auto-Tune
      choice list still matches upstream; `mmgp` version is read generically.
- [x] Prove the new mapping on this machine (env `C:\Wan2GP\env_uv`): a 39 MB
      table maps into reserved RAM, rows and gathers match the CPU copy, and a
      missing CUDA context degrades with a printed `[MMGP]` line.
- [ ] Re-audit `valid_memory_override` (features.rs) whenever upstream adds a
      `wgp_config.json` key — upstream `docs/CHANGELOG.md` stops at v12.643, so
      the Python code is the only contract source.
- [ ] Surface the enhancer's mapping outcome in the UI if users report the
      `[MMGP] Not enough reserved RAM left` line being missed (it already lands
      in the log verbatim).

## Session 2026-10-26 — v0.10.26 published

- [x] Version bump 0.10.25 → 0.10.26 (`package.json`, `tauri.conf.json`,
      `Cargo.toml`), commit `58c8481`, tag `v0.10.26`.
- [x] Release workflow ran for the first time (run 40367092578). Both signed
      legs failed: repo `TAURI_SIGNING_PRIVATE_KEY` did not match its password
      ("failed to decode secret key: incorrect updater private key password").
      Re-uploaded both secrets from `~/.tauri/wan2gp-desktop.key`/`.pwd`, rerun
      green: AppImage/deb/msi/nsis/dmg + `.sig` + `latest.json` + `SHA256SUMS.txt`.
- [x] Linux install bug verified fixed end-to-end: with the child environment the
      launcher now spawns, `uv install → uv python install 3.11.14 → verify →
      venv → uv pip install six` all succeed (previously the spawned python
      aborted on `PYTHONHOME` from the AppImage and the launcher reported
      "uv could not provision Python 3.11.14").
- [x] Found that runner-built AppImages never start: WebKitGTK 2.44/2.48 from
      ubuntu-22.04/24.04 aborts with `Could not create default EGL display:
      EGL_BAD_PARAMETER` (WSLg and Xvfb; every documented workaround env var
      still aborts). WebKitGTK 2.52 (ubuntu 26.04) warns and falls back.
      Replaced the 24.04 AppImage with the locally built 26.04 one (glibc ≥ 2.43),
      re-signed, patched `latest.json`, and re-tested by downloading the asset
      from the release: window up, probe ran, env clean, no EGL abort.
- [x] Windows NSIS verified from the release asset: silent install to
      `D:\w2gp-test`, launches, dashboard renders 0.10.26 with live hardware
      rows, console reports "Launcher v0.10.26 ready".
- [x] macOS bundle inspected (arm64 Mach-O, `Info.plist` 0.10.26, min 11.0);
      NOT executed — no Apple Silicon hardware. `[ ] Manual smoke test: macOS`.
- [x] `dist/` mirrors the release (13 files + `SHA256SUMS.txt`) and
      `dist/README.txt` records what was and was not run.

## Session 2026-10-26 (later) — the launch environment was silently dropped on Linux/macOS

- [x] Root cause of "the log says the Triton cache is pinned, but Triton keeps
      filling `~/.triton/cache`" (and of tokens configured in Settings never
      reaching WanGP): 0.10.27 made every spawn go through `shell_command`,
      which clears the child env and snapshots `child_env()` **when the
      `Command` is built** (line 811 of `launch.rs`). Every launch pin is a
      `set_var()` *after* that line — HF / Civitai / Claude keys from config,
      `TRITON_CACHE_DIR`, `PYTHONUNBUFFERED` / `PYTHONUTF8` /
      `PYTHONIOENCODING`, `TQDM_MININTERVAL` / `TQDM_MINITERS`, the
      `WGP_GGUF_*` knobs and the MSVC `CC`/`CXX` pin — so the stale snapshot
      won. Windows was unaffected: the plugin inherits the live process env at
      spawn, which is why nothing showed on the only OS tested at the time.
- [x] Proof, 0.10.28 on Ubuntu 26.04 + RTX 3080 (WSLg): launched with
      `HF_TOKEN=probe-abc123` exported by the shell → the child's
      `/proc/<pid>/environ` had the probe (it existed before the snapshot) and
      **none** of `TRITON_CACHE_DIR`, `PYTHONUNBUFFERED`, `PYTHONUTF8`,
      `PYTHONIOENCODING`, `TQDM_*`, `WGP_GGUF_*` (all set after it).
- [x] Fix (0.10.29): re-snapshot the live env immediately before `spawn()` on
      non-Windows targets. Windows keeps the unchanged path (one `cfg`), and
      `shell_command` already applied the same clear/filter, so the re-apply is
      idempotent (`env_clear().envs(child_env())`).
- [x] Verified on the shipped **0.10.29 `.deb`** (not a local build): child env
      now carries `TRITON_CACHE_DIR=/home/gerard/Wan2GP/triton-cache`,
      `PYTHONUNBUFFERED=1`, `PYTHONUTF8=1`, `PYTHONIOENCODING=utf-8`,
      `TQDM_MININTERVAL=0`, `TQDM_MINITERS=1`. Triton honours the pin: a probe
      kernel run under that var wrote 3 entries into the target dir.
- [x] Release 0.10.29 published with all assets (NSIS/MSI, `.deb`, AppImage +
      sig, `.dmg`/`.app.tar.gz`, `latest.json`); Windows `cargo check` clean.

## Session 2026-10-27 — 0.10.29 `.deb` and AppImage smoke test on WSL Ubuntu 26.04

- [x] `.deb` from the release (`sha256 673495c0…`; the binary inside is
      byte-identical to the installed `/usr/bin/wan2gp-desktop-launcher-tauri`,
      `8b032c47…`): renders on Xvfb `:98` (`GDK_BACKEND=x11`, no EGL at all) and
      on WSLg through XWayland. Console: *Launcher v0.10.29 ready*, hardware
      rows (i9-12900K / RTX 3080 / 19 GB / 10240 MB), env_uv / torch
      2.10.0+cu130 / CUDA 130 / Triton 3.6.0.
- [x] 0.10.29 launch-env fix re-confirmed on the `.deb`: **Launch Wan2GP in
      Desktop** → child `env_uv/bin/python …/boot.py wgp.py …` (34 vars) has
      `TRITON_CACHE_DIR=/home/gerard/Wan2GP/triton-cache`, `PYTHONUNBUFFERED=1`,
      `PYTHONUTF8=1`, `PYTHONIOENCODING=utf-8`, `TQDM_MININTERVAL=0`,
      `TQDM_MINITERS=1`; console logs *Triton JIT cache pinned to …*.
- [x] **The published 0.10.29 AppImage is broken** (`sha256 29125a73…`,
      82,590,200 bytes — the ubuntu-24.04 CI build, asset timestamp 21:33 UTC):
      the window opens blank (2.7–4 KB solid dark screenshot) with
      `Could not create default EGL display: EGL_BAD_PARAMETER. Aborting...`,
      identically under Xvfb and under WSLg (Wayland and XWayland). It is the
      WebKitGTK 2.48 abort, exactly the case the release/CI matrix change to
      `ubuntu-26.04` for the AppImage is meant to fix — that change is still
      uncommitted, so the 0.10.29 release did not benefit from it.
- [x] Rebuilt the AppImage locally on Ubuntu 26.04 (`npx tauri build --bundles
      appimage --config '{"bundle":{"createUpdaterArtifacts":false}}'`,
      86,501,880 bytes, `sha256 4d63b921…`, bundles `libwebkit2gtk-4.1.so.0`
      94,782,624 bytes vs the host 2.52.6 at 94,499,504): renders on Xvfb and on
      WSLg, no EGL abort (only `DRI3` warnings / software fallback), and
      **Launch** produces the same pinned child env (42 vars, AppImage bundle
      vars included). So the CI split fixes it; no 26.04-built AppImage is
      published yet.
- [x] `ubuntu-26.04` is a real runner image (`actions/runner-images`
      `images/ubuntu/Ubuntu2604-Readme.md`), and both Linux legs of the matrix
      are coherent: `.deb` stays on 24.04, AppImage on 26.04, same
      `x86_64-unknown-linux-gnu` target, one `latest.json` merge.
- [x] The v0.10.29 release has **no `SHA256SUMS.txt`** (11 assets, none of them
      the checksum file) while `README.md` line 46 tells users it is there.
      0.10.26's file was made by hand; nothing in `.github/workflows` generates
      it. → Fixed: a `checksums` job in `release.yml` now generates and uploads
      it; 0.10.29 carries the first one (12 assets, `sha256sum -c` passes).
- [x] Publish a 26.04-built AppImage for 0.10.29 (or ship 0.10.30 with the
      workflow change) — needs the updater key to re-sign and patch
      `latest.json`. → Done: rebuilt on Ubuntu 26.04, signed with the same key,
      re-uploaded to `v0.10.29` (86,501,880 bytes, `4d63b921…`),
      `latest.json` patched, verified end-to-end (`sha256sum -c` + updater
      signature + the served AppImage renders and launches on WSL 26.04).

## Session 2026-10-27 — published .deb and AppImage verified on WSL Ubuntu 26.04, then a clean first-run `.deb`

- [x] Published AppImage, fresh download (not the home copy): `sha256sum -c
      SHA256SUMS.txt` → OK for AppImage/sig/latest.json; the `.sig` asset equals
      the `latest.json` signature for both `linux-x86_64` entries (timestamp
      1793084681 = the re-sign). `.sig` files are base64(minisign), so compare the
      file content itself, not `base64 -w0` of it.
- [x] Render checks: Xvfb route was silent until the PLATFORMS route-B stack
      (`dbus-run-session` + three WEBKIT/LIBGL vars) — without it the window maps
      but never paints (1-color root screenshot, no ready line; app alive, no
      crash). With it: 902-color wizard screenshot, portal noise only. WSLg
      route A opened a real window (mapped 1908x999 at 6 s).
- [x] AppImage full user flow: wizard → fresh clone to `~/wan2gpappimage`
      (separate from the `.deb` install at `~/Wan2GP`; model paths remain the
      shared `~/Wan2GP-Models`) → torch 2.10.0+cu130 `cuda=True` → all kernels →
      config seeded. Verified on disk (venv python, wgp_config.json ckpts row).
- [x] Cross-bundle state: `.deb` launcher then launched the **AppImage** install
      (`/home/gerard/wan2gpappimage/env_uv/bin/python`) — by design:
      `~/.wan2gp-tauri-installed` and `.wan2gp-tauri-data-dir` are per-user and
      shared; `install.rs` writes the marker on install, `status.rs check_installed`
      follows it (its `missingPrevious` branch = the AppImage's "Previous install
      not found" message). Last install wins; delete the install dir and both
      bundles lose Launch until re-pointed. Documented nowhere in user docs —
      worth a README sentence when editing releases again.
- [x] Published `.deb` fresh download: `sha256sum -c` OK, inner binary
      byte-identical to `/usr/bin/…` (`8b032c47…`), `dpkg -V wan2-gp-desktop-launcher-tauri`
      clean. Package name `wan2-gp-desktop-launcher-tauri`, binary name without the
      dash — don't confuse the two when scripting dpkg.
- [x] Clean first-run `.deb` on 26.04 (the untested flow): wiped both installs
      (11 GB each), markers and the state dir (backup tar: `Temp/opencode/wsl-test/`)
      → wizard from zero (no, "unknown files" warning) → default `~/Wan2GP`
      → uv re-downloaded all wheels (cache did NOT serve them — true cold install)
      → smoke test `cuda=True` → complete. Launch: child
      `/home/gerard/Wan2GP/env_uv/bin/python`, 36 vars, all six pins,
      `TRITON_CACHE_DIR=/home/gerard/Wan2GP/triton-cache` (`/proc`-verified).
- [x] The `_POSIX_C_SOURCE redefined` TinyCC prattle reappeared exactly once on
      the fresh triton cache → README Linux runtime note added (warnings never
      fatal; only `error:` lines matter).
- [x] Two PLATFORMS.md fixes from evidence: route A gets a private
      `XDG_RUNTIME_DIR=/tmp/xdg` (WSLg's runtime-dir is mode 0777 and dbus warns
      "can be written by others" per start — observed, then gone with /tmp/xdg);
      WSL note that `GStreamer element appsink not found` is normal in WSL
      (WebKitGTK web-video backend missing; only the embedded `<video>` previews
      are affected; `gstreamer1.0-plugins-base gstreamer1.0-libav` to enable).
- [x] Shutdown: launcher closed → child killed by recorded PID (port 7861 free,
      no strays). Final state kept: one fresh `.deb` install at `~/Wan2GP` +
      empty `~/Wan2GP-Models` skeleton + uv caches; wsl-test scripts live in
      `Temp/opencode/wsl-test/` (git-ignored temp).


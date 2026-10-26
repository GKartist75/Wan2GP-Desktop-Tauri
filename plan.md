# Cross-Platform Wan2GP Desktop Launcher — Master Plan

## Objective
Make the existing Windows-only Tauri launcher run on:
- Linux x86_64 (NVIDIA, AMD ROCm, CPU fallback)
- macOS Apple Silicon (MPS/CPU only)

While keeping Windows behavior unchanged, one repository, one main branch, and synchronized releases.

---

## Guiding principles
1. One repo, one main branch — no platform forks.
2. Shared core, platform adapters.
3. Additive changes first — no invasive rewrites.
4. Capability gating — unsupported features are hidden, not broken.
5. Small PRs — one subsystem at a time.
6. CI on every platform.
7. Real-hardware validation before claiming support.

---

## Scope

### In scope (MVP)
- Linux build/run
- Linux NVIDIA detection
- Linux AMD ROCm detection
- Linux venv/uv install/update
- Linux launch/stop/update
- macOS Apple Silicon build/run
- macOS Apple Silicon detection
- macOS MPS profile
- macOS venv/uv install/update
- macOS launch/stop/update
- basic UI gating for both

### Out of scope (MVP)
- Intel Macs
- ARM Linux
- DLSS on macOS
- perfect desktop integration
- full updater polish on Linux/macOS
- every distro/GPU combination

---

## Architecture target

### New platform layer
- `src-tauri/src/platform/mod.rs`
- `src-tauri/src/platform/windows.rs`
- `src-tauri/src/platform/linux.rs`
- `src-tauri/src/platform/macos.rs`

### Adapter responsibilities
- GPU detection
- prerequisite detection/installation
- process signature
- port owner lookup
- kill PID
- open path
- desktop shortcut
- task manager
- auto-start
- capability flags

### Shared modules remain
- install orchestration
- launch orchestration
- config
- status
- updates
- frontend state

Rule: business logic must not contain `cfg(target_os)`.

---

## Phase 0 — Baseline and safety
Tasks:
- Tag current release as `v0.10.25-windows-stable`
- Add CI jobs for Windows, Ubuntu, macOS
- Add frontend capability map
- Add feature matrix to docs
- Keep Windows build unchanged

Acceptance:
- CI runs on all three OSes
- Windows behavior unchanged
- Feature matrix documented

---

## Phase 1 — Platform adapter skeleton (no behavior change)
Tasks:
- Add `platform/mod.rs` with trait + factory
- Add Windows adapter by moving existing Windows logic
- Add Linux adapter by moving existing Unix/Linux logic
- Add macOS adapter as stub or Linux-like where safe
- Do not migrate call sites yet

Acceptance:
- Compiles on Windows
- No behavior change
- Tests pass

---

## Phase 2 — Migrate subsystems to adapters
Migration order:
1. `hw.rs` GPU detection
2. `launch.rs` process signature + port scan
3. `troubleshoot.rs` port status/fix
4. `system.rs` open path / shortcut / task manager
5. `install.rs` prerequisite detection
6. `features.rs` auto-start

For each subsystem:
- Move OS cfg logic into adapter
- Replace cfg branches with adapter calls
- Run tests and clippy
- Verify Windows behavior unchanged

Acceptance:
- Migrated modules have no OS cfg blocks
- All tests pass
- Clippy/fmt pass

---

## Phase 3 — Linux port

### 3.1 Build and packaging
- Configure Tauri Linux targets: AppImage, `.deb`, optional `.rpm`
- Document WebKitGTK/runtime dependencies

### 3.2 Hardware detection
- NVIDIA via `nvidia-smi`
- AMD via `lspci` / `clinfo`
- Intel via `lspci`
- VRAM via nvidia-smi or `/sys/class/drm`
- RAM via sysinfo

### 3.3 Prerequisites
- Detect Git, Python, uv, Conda
- Distro-aware guidance
- Prefer venv/uv
- Keep Conda optional

### 3.4 Install/update
- Use Linux command entries from `setup_config.json`
- Run upstream `setup.py`
- Handle Linux kernel wheels
- Update via git pull + requirements

### 3.5 Launch/stop
- Reuse `/proc` and `lsof`
- Verify close sweep
- No Windows-only process calls

### 3.6 UI gating
Hide on Linux:
- Task manager
- MSVC repair
- Windows Terminal integration
- desktop shortcut (or defer)
- unsupported DLSS features unless using upstream Linux worker

Linux acceptance:
- Build succeeds
- NVIDIA and AMD detected
- install succeeds
- launch/stop works
- update works
- generate one image/video
- no orphaned processes
- docs updated

---

## Phase 4 — macOS port (Apple Silicon only)

### 4.1 Build and packaging
- Configure macOS bundle: `.app`, `.dmg`
- Document signing/notarization later

### 4.2 Hardware detection
- Detect Apple Silicon via `system_profiler SPDisplaysDataType`
- Vendor `APPLE`
- Profile `MPS`
- Chip name
- Unified memory / RAM default
- Sensible VRAM default (documented)

### 4.3 Prerequisites
- Detect Git, Python, uv
- Prefer venv/uv
- Keep Conda experimental/hidden initially
- macOS paths for tools

### 4.4 Install/update
- MPS profile:
  - Python 3.11
  - `torch torchvision torchaudio`
  - requirements only
- No Triton/Sage/Flash/kernels
- Handle macOS RAM/VRAM defaults
- Update via git pull + requirements

### 4.5 Launch/stop
- Replace `/proc` signature logic with macOS `ps`/`lsof`
- Use `lsof` for port owner
- Kill by PID
- Verify stop/close

### 4.6 UI gating
Hide on macOS:
- DLSS
- GPU kernel wheels
- VRAM diagnostics/trim
- MSVC/Triton repair
- task manager
- desktop shortcut (or defer)
- NVIDIA/AMD-specific controls

Show clearly:
- “Apple Silicon / MPS (CPU+Metal, slower)”

macOS acceptance:
- Build succeeds
- Apple Silicon detected
- install succeeds
- launch/stop works
- update works
- generate one image/video
- no orphaned processes
- docs updated

---

## Phase 5 — System integration

### Linux
- `.desktop` entry
- optional auto-start via systemd user service
- notifications

### macOS
- optional LaunchAgent auto-start
- notifications
- defer desktop shortcut or implement later

Acceptance:
- Apps launch normally
- Clear integration status documented

---

## Phase 6 — Updater and release
- Linux: AppImage self-update or documented manual/package-manager update
- macOS: signed/notarized updater artifacts
- Windows: unchanged
- One GitHub Release with all artifacts
- Changelog notes platform support and limitations

Acceptance:
- Synchronized multi-platform release
- Clear update instructions

---

## Phase 7 — Testing and validation

### Level 1 — Unit tests
- detection parsing
- profile resolution
- adapter unsupported behavior
- frontend gating

### Level 2 — Integration tests
- tempdir repo/env
- fake git/python/uv/pip
- fake hardware probes
- fake lsof/kill/ps

### Level 3 — CI
- Windows
- Ubuntu
- macOS

### Level 4 — Real hardware
- Windows NVIDIA
- Linux NVIDIA
- Linux AMD
- macOS Apple Silicon

Acceptance:
- Successful end-to-end run documented on each supported platform
- Known limitations listed

---

## Refactor rules to prevent drift
1. Business logic must not contain `cfg(target_os)`.
2. OS code lives only in `platform/*`.
3. Shared modules call adapter methods.
4. Unsupported features return explicit “unsupported” results.
5. Capability flags drive frontend gating.
6. CI must build all platforms before merge.
7. One repo, one main branch.

---

## Risks and mitigations
| Risk | Mitigation |
|---|---|
| Linux distro differences | Target Ubuntu/Fedora first |
| AMD ROCm complexity | Add AMD after NVIDIA on Linux |
| macOS `/proc` absence | Use `ps`/`lsof` |
| macOS unified memory ambiguity | Document VRAM default |
| Conda on macOS | Prefer venv/uv |
| Large diff risk | Small PRs, one subsystem at a time |
| Platform drift | One repo, CI matrix, capability flags |

---

## Recommended implementation order
1. Phase 0 — baseline/CI
2. Phase 1 — adapter skeleton
3. Phase 2 — migrate subsystems
4. Phase 3 — Linux
5. Phase 4 — macOS
6. Phase 5 — system integration
7. Phase 6 — updater/release
8. Phase 7 — validation

---

## First concrete coding slice
Do this first, before any platform behavior changes:
1. Add Linux/macOS CI build jobs.
2. Create `platform/` module with Windows adapter only.
3. Add Linux/macOS adapter stubs.
4. Add capability flags to frontend.
5. Keep Windows behavior unchanged.

This is the safest foundation for everything that follows.
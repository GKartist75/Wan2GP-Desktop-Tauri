# Platform support — Windows, Linux, macOS

One repository, one code base, three builds. This page states what each
platform gets, how the platform split is implemented, and — per feature —
whether it has ever been run on that OS.

Status vocabulary, used strictly:

| Mark | Meaning |
| --- | --- |
| **Working** | Built, run and exercised by hand on that OS on this branch. |
| **Ported** | Compiles *and bundles* for that target on a real machine of that OS (CI), and is unit-tested, but the app has never been executed there. |
| **Hidden** | Deliberately not offered: the UI control is removed and the backend capability reports `false`. |

Today only Windows is **Working**. Linux and macOS are **Ported**: the crate
compiles and the bundles build on `ubuntu-22.04` (x86_64) and `macos-latest`
(arm64) runners, and `dist/` holds real artifacts. That distinction is the whole
point of this document — a green bundle build proves the code compiles and the
packaging works; it does not prove `uv` resolved a wheel or that a port was
freed.

## Feature matrix

Backend truth lives in `Platform::supports` (`src-tauri/src/platform/{windows,linux,macos}.rs`).
The renderer reads the same map through `src/services/platform-gate.js`, and
`tests/platform-gate.test.js` parses the Rust assertions so the two cannot drift.

| Capability | Key | Windows | Linux | macOS |
| --- | --- | --- | --- | --- |
| Install / update WanGP, launch, stop, logs | — | Working | Ported | Ported |
| GPU detection | — | Working (nvidia-smi / WMI) | Ported (`nvidia-smi`, `lspci` + `/sys/class/drm`) | Ported (`system_profiler` + `hw.memsize`) |
| Embedded desktop view | — | Working | Ported | Ported |
| Port owner lookup | — | Working (`netstat`/`tasklist`) | Ported (`ss`, then `lsof`) | Ported (`lsof`) |
| Desktop shortcut | `desktopShortcut` | Working | Ported (`~/.local/share/applications/*.desktop`) | Hidden |
| Auto-start on login | `autoStart` | Working (registry) | Ported (`~/.config/autostart/*.desktop`) | Ported (LaunchAgent) |
| GPU kernel wheels | `kernelWheels` | Working | Ported | Hidden |
| Triton repair / MSVC toolchain | `tritonRepair` | Working | Hidden | Hidden |
| Windows Task Manager button | `taskManager` | Working | Hidden | Hidden |
| DLSS 5 runtime | `dlss` | Working | Hidden | Hidden |
| Self-update (`tauri-plugin-updater`) | — | Working (NSIS/MSI) | AppImage only, untested | Disabled |

`autoStart` is the only capability every platform claims; DLSS, kernel wheels and
the MSVC/Triton toolchain are all CUDA/NVIDIA-specific, and macOS has neither.

## What each build produces

| Platform | Artifacts | Updater artifact |
| --- | --- | --- |
| Windows x64 | NSIS `setup.exe`, MSI (`release.yml` bundles `nsis,msi`; `dist/windows-x64` holds the NSIS one) | yes (minisign) |
| Linux x64 | AppImage, `.deb` | AppImage only |
| macOS arm64 | `.app`, `.dmg` | no |

`.github/workflows/build-artifacts.yml` produces the same bundles without a tag,
a release or a signing key (dispatch it, download the run's artifacts). The
Linux and macOS files in `dist/` came from it; the Windows installer there comes
from `release.yml` (or a local `npm run tauri -- build`), because the artifact
workflow builds the two Unix targets only. The release workflow
(`.github/workflows/release.yml`) builds all three from one tag and attaches them
to a single draft release. macOS sets
`createUpdaterArtifacts: false` for the build, because replacing an unsigned
`.app` is not a path we can support without an Apple developer identity — the
in-app updater is only offered where it can actually work.

Updater signing needs `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` in the repository secrets (the local
Windows release script uses the same key).

## Installing on Linux

Requires a desktop session with WebKitGTK 4.1 (Ubuntu 22.04+/24.04, Fedora 40+,
Arch). No Python, CUDA toolkit or `pip` needed beforehand — the launcher fetches
what it needs, exactly like on Windows.

```bash
# Debian/Ubuntu — the AppImage needs no packages, the .deb pulls these in
sudo apt install ./Wan2GP*_amd64.deb
# or
chmod +x Wan2GP*.AppImage && ./Wan2GP*.AppImage
```

GPU support:

- **NVIDIA** — `nvidia-smi` must be on `PATH` (the proprietary driver package
  puts it in `/usr/bin`). The launcher reads VRAM from it and picks the CUDA
  profile.
- **AMD** — detected through `lspci` plus `/sys/class/drm/*/device/mem_info_vram_total`;
  ROCm wheels are installed for RDNA3/RDNA4 parts.
- **No `nvidia-smi`** (nouveau, container, headless) — detection returns
  `unknown` and the installer falls back to the CPU profile. That is a
  deliberate conservative choice: an NVK/nouveau box cannot use CUDA wheels.
- **Port owner lookup prefers `ss`** (iproute2, installed by default on Ubuntu,
  Fedora and Arch) and falls back to `lsof`. If neither exists the sweep reports
  that it cannot see sockets instead of silently sparing an orphan.

## Installing on macOS

Apple Silicon only (Intel Macs are out of scope). macOS 11+.

- GPU work runs on **MPS**; there are no CUDA or ROCm paths, so the DLSS card,
  the GPU kernel-wheels card and the Triton/MSVC toolchain are hidden.
- The launcher reports **unified memory** as VRAM (`sysctl -n hw.memsize`), since
  the GPU addresses the same pool as the CPU. Falls back to 8 GB if `sysctl`
  cannot be read.
- `pip`/torch installs are plain PyPI builds; there is no Apple-signed wheel step.
- The `.dmg` is **unsigned and not notarized**. First launch needs
  right-click → Open (or `xattr -dr com.apple.quarantine /Applications/Wan2GP*.app`).
  Gatekeeper will keep warning until a Developer ID is configured.
- Conda is experimental; venv/uv are the supported env types.

## How the platform split works

Three layers, no forks:

1. **Adapters** — `src-tauri/src/platform/{windows,linux,macos}.rs` implement one
   trait (`Platform`): GPU probe, process listing/kill, port owner, path opener,
   shortcut, auto-start, task manager, `supports(Capability)`. `platform::current()`
   picks one at compile time; `src-tauri/src/platform/unix.rs` holds the shared
   `ss`/`lsof`/`ps`/`pgrep` code for Linux and macOS.
2. **Capabilities** — `Capability::{TaskManager, DesktopShortcut, Dlss,
   KernelWheels, TritonRepair, AutoStart}`. A platform either supports one or it
   does not; nothing is "partially" supported at runtime.
3. **Declarative UI gate** — `src/services/platform-gate.js` maps platform →
   capability → visible controls. `index.html` marks a control with
   `data-requires="capability"`; the gate toggles `.cap-off`. Unknown or empty
   capability names fail closed (hidden), so a typo makes a control disappear
   instead of crashing it. A control can require several capabilities by
   space-separating them (ANDed).

Adding a platform-visible feature therefore costs three lines: a `Capability`
variant, one `supports` arm per adapter, and `data-requires` in the markup.

## Known gaps

Ordered by what breaks first for a new Linux/macOS user.

1. **Nothing on Linux or macOS has been executed.** No install, launch, stop,
   update, GPU probe or port lookup has run on real hardware. First-run bugs are
   expected.
2. **Apple code signing** is not configured: unsigned `.dmg`, Gatekeeper warning,
   self-update disabled.
3. **Linux self-update only works for the AppImage.** `.deb` users must download
   the new build.
4. **A host with neither `ss` nor `lsof`** cannot attribute or sweep ports; the
   stop path surfaces an explicit error rather than an empty list. No `fuser`
   fallback.
5. **NVIDIA on Linux without `nvidia-smi`** is `unknown` → CPU profile.
6. **AMD ROCm on Linux** is ported and unit-tested but has never run; the HIP
   wheel path is the same one that recently changed on Windows.
7. **The bundle build is the only proof so far.** `build-artifacts.yml` compiled
   and packaged both targets on real runners (that also cleared the AWS-LC /
   `rustls` link question a Windows cross-check could never answer), but no
   0.10.x build has been started on either OS by a human.
8. **The artifacts in `dist/` carry no updater signature** (the artifact workflow
   forces `createUpdaterArtifacts: false`), so they cannot self-update. Only
   `release.yml` with `TAURI_SIGNING_PRIVATE_KEY` produces update-capable
   bundles.

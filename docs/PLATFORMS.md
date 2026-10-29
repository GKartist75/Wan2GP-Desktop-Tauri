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

Windows is **Working**. Linux is **Working for launch** — the AppImage and the
`.deb` were installed and started by hand on Ubuntu 26.04 (2026-10-26): window,
full UI, prerequisite probe (git 2.53, uv 0.13.5, Python 3.14), WebKit processes
up. What has *not* run on Linux is the rest of the cycle: WanGP install, GPU
wheel install, launching WanGP itself, stop, self-update. macOS is **Ported**:
the crate compiles and the bundle builds on `macos-latest`, no human has opened
it. That distinction is the whole point of this document — a green bundle build
proves the code compiles and the packaging works; it does not prove `uv`
resolved a wheel or that a port was freed.

## Feature matrix

Backend truth lives in `Platform::supports` (`src-tauri/src/platform/{windows,linux,macos}.rs`).
The renderer reads the same map through `src/services/platform-gate.js`, and
`tests/platform-gate.test.js` parses the Rust assertions so the two cannot drift.

| Capability | Key | Windows | Linux | macOS |
| --- | --- | --- | --- | --- |
| Install / update WanGP, launch, stop, logs | — | Working | Ported | Ported |
| GPU detection | — | Working (nvidia-smi / WMI) | Ported (`nvidia-smi`, `lspci` + `/sys/class/drm`) | Ported (`system_profiler` + `hw.memsize`) |
| Embedded desktop view | — | Working | Ported | Ported |
| Desktop renderer | `nativeEmbed` | Working (WebView2 child webview) | Forced to `iframe` (GTK cannot overlay a child webview) | Forced to `iframe` (WKWebView cannot overlay a child webview) |
| Port owner lookup | — | Working (`netstat`/`tasklist`) | Ported (`ss`, then `lsof`) | Ported (`lsof`) |
| Desktop shortcut | `desktopShortcut` | Working | Ported (`~/.local/share/applications/*.desktop`) | Hidden |
| Auto-start on login | `autoStart` | Working (registry) | Ported (`~/.config/autostart/*.desktop`) | Ported (LaunchAgent) |
| GPU kernel wheels | `kernelWheels` | Working | Ported | Hidden |
| Triton repair / MSVC toolchain | `tritonRepair` | Working | Hidden | Hidden |
| Windows Task Manager button | `taskManager` | Working | Hidden | Hidden |
| DLSS 5 runtime | `dlss` | Working | Hidden | Hidden |
| Self-update (`tauri-plugin-updater`) | — | Working (NSIS/MSI) | AppImage only, untested; `.deb`/`.rpm` get the install command | Disabled — `.dmg` is download-and-drag |

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

The launcher knows which of six install kinds it is running as (`appimage`,
`deb`, `rpm`, `windows`, `macapp`, `source`) and answers accordingly — an in-app
download+install on Windows, an in-app swap on a signed AppImage, and otherwise
the download link or the exact command to run. `docs/UPDATING.md` is the
per-platform page, including the winget and Homebrew Cask lanes that
`.github/workflows/package-managers.yml` submits on a published release.

## Installing on Linux

Two bundles; both were installed and started on Ubuntu 26.04. They are built on
different hosts on purpose: the AppImage bundles the WebKitGTK of whoever builds
it, so it is built on the newest Ubuntu here (26.04, WebKitGTK 2.52), while the
`.deb` links the distro's WebKitGTK and is built on `ubuntu-24.04`.

**AppImage + EGL.** WebKitGTK 2.44/2.48 (ubuntu-22.04/24.04 runners) does not
just warn when it cannot create an EGL display, it aborts:

```
Could not create default EGL display: EGL_BAD_PARAMETER. Aborting...
```

Reproduced with runner-built AppImages on WSLg and under Xvfb, and no
environment variable avoids it (`WEBKIT_DISABLE_DMABUF_RENDERER`,
`WEBKIT_DISABLE_COMPOSITING_MODE`, `GDK_GL=disable`, `EGL_PLATFORM`,
`LIBGL_ALWAYS_SOFTWARE`, `GALLIUM_DRIVER=llvmpipe` all still abort). WebKitGTK
2.52 falls back to software rendering, which is why the shipped AppImage starts
even on a host with no usable GPU. The cost is the build host's glibc floor:
this AppImage needs **glibc ≥ 2.43** (Ubuntu 26.04+). On 22.04/24.04 use the
`.deb`.

| Form | Command | Notes |
| --- | --- | --- |
| AppImage | `chmod +x Wan2GP*.AppImage && ./Wan2GP*.AppImage` | self-contained (bundles WebKitGTK and its GTK deps). **Needs FUSE** — `libfuse2t64` on Ubuntu 24.04+/26.04, `libfuse2` before that. Without it: `./Wan2GP*.AppImage --appimage-extract-and-run`, or run `squashfs-root/AppRun` from an extracted bundle (running the inner binary directly fails: WebKit's helper processes are resolved relative to the AppRun environment) |
| `.deb` | `sudo apt install ./Wan2GP*_amd64.deb` | package `wan2-gp-desktop-launcher-tauri`, installs `/usr/bin/wan2gp-desktop-launcher-tauri` and a menu entry; `Depends: libwebkit2gtk-4.1-0, libgtk-3-0` |

Both need a desktop session with GTK 3 + WebKitGTK 4.1 (Ubuntu 22.04+, Debian 12+,
Fedora 40+, Arch). The `.deb` is the smaller, system-integrated path; the AppImage
is the only Linux form that can self-update (see Known gaps).

**Arch-based distros (Arch, CachyOS, Omarchy, EndeavourOS).** Use the AppImage;
the `.deb` is Debian packaging and `dpkg` will not install it. Arch ships glibc
2.44 (2026-09), above this AppImage's 2.43 floor, so nothing needs rebuilding.
Packages the launcher leans on:

```bash
sudo pacman -S fuse2 xdg-utils git   # fuse2 mounts the AppImage, xdg-utils opens folders
```

`nvidia-smi` comes from `nvidia-utils`; AMD cards are read from `/sys/class/drm`,
no extra package. Python on PATH is not required — uv provisions 3.11 itself.
Never run on Arch here, so treat this as the documented path, not a tested one.

On a Hyprland/wlroots session such as Omarchy, three things differ:

- *Start at login* writes `~/.config/autostart/wan2gp-desktop.desktop`, and
  Hyprland does not read XDG autostart. Use
  `exec-once = wan2gp-desktop-launcher-tauri` in the Hyprland config instead.
- *Create desktop shortcut* writes `~/.local/share/applications/wan2gp-desktop-launcher.desktop`.
  Hyprland has no desktop icons — start it from a keybind or a launcher
  (walker/wofi/rofi); the entry still shows up there.
- If the window opens blank on NVIDIA + Wayland, run it with
  `WEBKIT_DISABLE_DMABUF_RENDERER=1` (WebKitGTK's DMABUF path on wlroots).

The **prereq chip** next to each tool is copyable and carries the Debian
command; its tooltip lists the Arch (pacman) and Fedora (dnf) equivalents,
since the launcher cannot tell the distro apart from the browser. **Python
reads `not found - optional`**: `uv` provisions the pinned 3.11 the installer
uses (`uv python find`), so a system Python is only needed by the legacy
CPU/INTEL_XPU path.

**Prerequisites are mostly not one-click on Linux.** Those buttons are
winget/choco/MSI, i.e. Windows-only, so on Linux the Prerequisites card replaces
each *Install* button with the command to run (`sudo apt install git`, …); click
it to copy. On macOS the same slot shows `xcode-select --install` / `brew install
python`. `git` and Python come from the distro (`sudo apt install git
python3`) — or uv fetches the Python build the environment needs. Conda is
optional. `uv` is the exception and **keeps its Install button**: Astral's
`install.sh` is user-scope (no sudo), so the launcher runs it and puts the copy in
`~/Wan2GP/.tools` (`UV_NO_MODIFY_PATH=1`, so your PATH is never touched). That is
also why uv does not have to be installed before pressing *Install*.

**SpargeAttn and flash-attn need a CUDA toolkit on Linux/macOS.** Upstream
`docs/INSTALLATION.md` names CUDA **13.1** (RTX 20XX-50XX) or **12.8**
(GTX 10XX) as a prerequisite: those two components are compiled from source here
(`git+https://github.com/woct0rdho/SpargeAttn`, `flash-attn==2.8.3.post1` sdist)
while Windows gets prebuilt wheels. The Prerequisites card carries a **CUDA
toolkit** row on Linux NVIDIA hosts (detected version, marked optional), and the
wizard's overview marks the two components "skipped — needs the CUDA toolkit
(nvcc)" when it is absent — everything else (Torch, SageAttention wheel, Triton,
WGPKernels, nunchaku, GGUF) still installs. To add them later, install the
toolkit (NVIDIA runfile; the distro package is usually too old), then Manage →
Reinstall. With a toolkit present the launcher exports `CUDA_HOME` and puts its
`bin` plus the environment's `bin` on `PATH`, because SpargeAttn's build shells
out to `nvcc` and `python`. A Windows toolkit mounted under `/mnt/c` is ignored:
`nvcc.exe` through WSL interop cannot build Linux extensions.

**glibc.** The Linux wheels are built for glibc 2.34+ (WGPKernels) and 2.28+
(SageAttention, GGUF). On an older distro WanGP's own kernel probe falls back to
Triton/PyTorch INT8, so nothing breaks — INT8 is just slower.

Where things land:

- `~/Wan2GP` — the WanGP repo and its `envs/` (changeable in the wizard)
- `~/Wan2GP-Models` — the proposed model root (`ckpts`, `loras`, `outputs`)
- `~/Wan2GP/desktop-config.json` — launcher settings; `envs.json` sits in the repo
- `~/.local/share/applications/*.desktop` — the shortcut the launcher creates;
  `~/.config/autostart/*.desktop` — auto-start

Uninstall: `sudo apt remove wan2-gp-desktop-launcher-tauri` (or delete the
AppImage), then delete `~/Wan2GP` and `~/Wan2GP-Models` if the env and the models
should go too. The launcher writes nothing else; `uv`, `pip` and WanGP keep their
own caches under `~/.cache` (Manage → System → disk cleanup lists them).

> WSL2 needs three environment variables or WebKitGTK aborts with `Could not
> create default EGL display: EGL_BAD_PARAMETER` and the window stays blank:
> WSLg offers no `/dev/dri` (only `/dev/dxg`), so there is no GBM device for
> WebKit's default EGL path. With `LIBGL_ALWAYS_SOFTWARE=1`,
> `WEBKIT_DISABLE_DMABUF_RENDERER=1` and `WEBKIT_DISABLE_COMPOSITING_MODE=1` the
> launcher runs as a normal Windows window (verified on Ubuntu 26.04, WSL 2.7,
> WSLg 1.0.73). Not a launcher bug.
>
> Mesa on 26.04 still tries the d3d12/ZINK path first and logs `MESA: error:
> ZINK: failed to choose pdev` + `egl: failed to create dri2 screen`; the
> process then survives without ever mapping a window. Adding
> `MESA_LOADER_DRIVER_OVERRIDE=llvmpipe GALLIUM_DRIVER=llvmpipe GDK_BACKEND=x11`
> gives a real window (verified with the 0.10.28 `.deb` on the same box; the
> full installer screen renders, RTX 3080 detected through WSL's CUDA
> passthrough).
>
> WSL also exposes the host's NVIDIA driver (`/usr/lib/wsl/lib/nvidia-smi`,
> `libcuda.so`), so on an NVIDIA machine GPU detection, VRAM, profile choice and
> CUDA execution are real — only the UI is software-rendered.

### Testing the Linux build from a Windows machine

WSL2 runs the real Linux binary (same GTK/WebKit stack, same code paths, and on
an NVIDIA host the same GPU through the driver's CUDA passthrough — `/dev/dri`
does not exist there, but `/usr/lib/wsl/lib/libcuda.so` does). It is what the
Linux verification above was done with.

```bash
# one-time inside the distro
sudo apt install -y xvfb xdotool imagemagick x11vnc dbus-x11 \
  libwebkit2gtk-4.1-0 libgtk-3-0 curl git
```

Build from the repo as usual (cargo needs the Linux target deps: `libwebkit2gtk-4.1-dev`,
`libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `patchelf`), then:

**Route A — WSLg, native window (what you want for clicking around):**

```bash
export DISPLAY=:0 GDK_BACKEND=x11 XDG_RUNTIME_DIR=/tmp/xdg
export LIBGL_ALWAYS_SOFTWARE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 WEBKIT_DISABLE_COMPOSITING_MODE=1
mkdir -p "$XDG_RUNTIME_DIR"; chmod 700 "$XDG_RUNTIME_DIR"
dbus-run-session -- ./src-tauri/target/debug/wan2gp-desktop-launcher-tauri
```

The window is an XWayland client but appears like any other Windows window.
(The private `XDG_RUNTIME_DIR` keeps dbus quiet: WSLg's `/mnt/wslg/runtime-dir`
is mode 0777 and dbus complains `can be written by others` on every start.)

**Route B — Xvfb (headless, scriptable):**

```bash
export DISPLAY=:99 WEBKIT_DISABLE_DMABUF_RENDERER=1 WEBKIT_DISABLE_COMPOSITING_MODE=1 LIBGL_ALWAYS_SOFTWARE=1 GDK_BACKEND=x11 XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$XDG_RUNTIME_DIR"; chmod 700 "$XDG_RUNTIME_DIR"
Xvfb :99 -screen 0 1400x900x24 &
dbus-run-session -- ./src-tauri/target/debug/wan2gp-desktop-launcher-tauri
```

For a live, clickable window point a VNC viewer at `localhost:5900` (WSL2
forwards it) after:

```bash
env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET XDG_SESSION_TYPE=x11 \
  x11vnc -display :99 -localhost -nopw -forever -shared -bg
```

The `env -u` matters: WSLg exports `WAYLAND_DISPLAY`, and x11vnc then refuses to
start ("Wayland sessions are only supported via -rawfb"). For scripted checks use
`xdotool` (click at coordinates) and `import -window root shot.png`
(screenshot). Without `dbus-run-session` the WebKit portal warnings flood the log
and the window may not map.

`GStreamer element appsink not found. Please install it.` is normal under WSL:
WebKitGTK has no web-media backend there (no base GStreamer plugins installed) and
prints this once, then keeps working — the launcher UI needs none of it. Only the
embedded gradio view's HTML `<video>` previews are affected; install
`gstreamer1.0-plugins-base gstreamer1.0-libav` if you want those inside the app.
Same page in a normal browser plays as usual. Verified on WSL Ubuntu 26.04 with
the published AppImage and the `.deb`.

Simulate a GPU without hardware: `WGP_PROBE_NVIDIA_SMI=/tmp/fake-nvidia-smi`
replaces the `nvidia-smi` probe with your script (executed directly on
Linux/macOS, via `cmd /C` on Windows), so the wizard resolves the CUDA profile,
VRAM and the driver preflight. Two calls are made:

```bash
#!/usr/bin/env bash
case "$*" in
  *index,uuid*) echo "0,GPU-11111111-2222-3333-4444-555555555555,NVIDIA GeForce RTX 4080,16376,15000,550.54" ;;
  *)            echo "NVIDIA GeForce RTX 4080, 16376 MiB, 550.54" ;;
esac
```

That renders `GPU NVIDIA GeForce RTX 4080`, `CUDA build CUDA 13 (cu130)`,
`Profile RTX 40` and the "driver 550.54 < R580" warning — the whole Linux
detection → profile → preflight chain, with no NVIDIA hardware.

Building straight out of `/mnt/d` works but is slow; a copy on ext4 (tar-pipe
the repo to `~/w2gp`) rebuilds in seconds.

Unit tests: `npm test` (renderer) and `cargo test` (Rust) run unmodified inside
the distro. Bundles: `cargo tauri build` then install the `.deb`
(`sudo apt install ./…_amd64.deb`) or run the AppImage — both start under Xvfb.

What WSL still cannot answer: AMD (needs an AMD card — Linux detection there
uses `lspci` and `/sys/class/drm`), a genuine desktop session (WSLg has no seat,
no display manager, no `/dev/dri`, so GTK integration like the menu entry and
`.desktop` handling is only half-tested), and the updater. A real Ubuntu desktop
or a live USB is the answer for those.

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

Apple Silicon only (Intel Macs are out of scope). macOS 11+ (the bundle's
`minimumSystemVersion`).

1. Open the `.dmg` and drag **Wan2GP Desktop Launcher Tauri** into *Applications*.
2. It is unsigned and not notarized: first launch needs right-click → **Open**, or
   `xattr -dr com.apple.quarantine "/Applications/Wan2GP Desktop Launcher Tauri.app"`.
   Gatekeeper keeps warning until a Developer ID is configured.
3. There is **no in-app updater on macOS** (an unsigned `.app` cannot replace
   itself): download the new `.dmg`.

Prerequisites: `git` from Xcode command line tools (`xcode-select --install`) or
Homebrew; Python comes from uv when the system one is unsuitable. The one-click
*Install* buttons are Windows-only here too; `uv` is the exception — the
installer can put its own copy in `~/Wan2GP/.tools`.

Where things land: `~/Wan2GP` (repo + `envs/`), `~/Wan2GP-Models` (model root:
`ckpts`, `loras`, `outputs`), `~/Wan2GP/desktop-config.json`, and
`~/Library/LaunchAgents/com.wan2gp.desktop.plist` when auto-start is on.

Uninstall: drag the app to the Bin; if auto-start was enabled,
`launchctl bootout gui/$UID/com.wan2gp.desktop` and delete that plist; then remove
`~/Wan2GP` and `~/Wan2GP-Models`.

- GPU work runs on **MPS**; there are no CUDA or ROCm paths, so the DLSS card,
  the GPU kernel-wheels card and the Triton/MSVC toolchain are hidden.
- **MPS failures are upstream bugs, not install problems.** Manage →
  Troubleshooting shows an *Apple Silicon (MPS) — known upstream bugs* card on
  macOS (hidden elsewhere) mapping the console line to its upstream issue:
  `cudaHostRegister` / `L2_cache_size` / `/proc/self/smaps` / Metal command
  buffer / no-error black frames → #2554, #2552, #2545, #2014, #2496. The
  startup-crash triage recognises the same signatures and says so instead of
  offering a settings retry; *Report an issue…* carries the accelerator line
  (`MPS (mps_available=true, cuda_available=false)`), not `CUDA ?`.
- The GPU probes are backend-aware: *Verify GPU compute* runs a float32 GEMM on
  MPS and reports `mode mps` (it used to run the CUDA kernel probe, which
  reported every wheel as missing). The NVIDIA arch gate — a wheel whose
  `get_arch_list()` cannot run this card — is NVIDIA-only: ROCm reports gfx
  tags in the same field and Apple Silicon has no kernel wheels.
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

1. **Nothing has run on macOS.** Ubuntu 26.04 ran the AppImage (WSLg and Xvfb)
   and the `.deb`: window, prerequisite probe, the installer's uv → Python
   3.11.14 → venv → pip sequence under the exact child environment, and a full
   NVIDIA install to a running WanGP. Still unexecuted anywhere on Unix: GPU
   wheel install, self-update, port sweep, shortcut and auto-start creation.
   The macOS bundle is only known to be well formed (arm64 Mach-O, `Info.plist`
   0.10.26, minimum 11.0). First-run bugs are expected — a first-run path bug
   was already found this way (hard-coded `\\` joins produced
   `~/Wan2GP-Models\ckpts`).
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
7. **Runner-built AppImages do not start.** `release.yml` on ubuntu-24.04
   produces an AppImage whose bundled WebKitGTK 2.48 aborts without an EGL
   display (see *Installing on Linux*); the 0.10.26 asset is therefore a
   local ubuntu-26.04 build with a glibc ≥ 2.43 floor. A portable AppImage needs
   either a 26.04 runner image or a WebKit that falls back instead of aborting.
8. **`dist/` mirrors the release, so it *is* signed** — the assets come from
   `release.yml` with `TAURI_SIGNING_PRIVATE_KEY`, and `latest.json` points at
   the same bytes. If the repo secrets are ever rotated, they must match
   `~/.tauri/wan2gp-desktop.key` (the pubkey in `tauri.conf.json`).

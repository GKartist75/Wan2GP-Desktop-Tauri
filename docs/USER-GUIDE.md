# Wan2GP Desktop Launcher — User Guide

Every screen, tab and button, and what it does. The launcher has three screens
(**Splash → Dashboard → Installer**) plus the **Manage** panel and the embedded
**Viewer**.

## Contents

- [Dashboard](#dashboard)
- [Installer (Setup)](#installer-setup)
- [Viewer (Desktop embed)](#viewer-desktop-embed)
- [Manage → General](#manage--general)
- [Manage → Launch](#manage--launch)
- [Manage → System](#manage--system)
- [Manage → Plugins](#manage--plugins)
- [Manage → Auto-Tune](#manage--auto-tune)
- [Console & logs](#console--logs)
- [Typical workflows](#typical-workflows)

---

## Dashboard

The home screen. Top to bottom:

**Top bar**

| Button | What it does |
|---|---|
| ← Back to Dashboard | Leaves the Viewer, back to this screen |
| ⊞ Console | Toggles the floating terminal console |
| ⟳ (reload icon) | Reloads the Wan2GP view |
| ⏹ Stop All | Stops Wan2GP + OpenCode servers (also kills the port listener) |
| Task Manager | Opens Windows Task Manager |
| ☀/☾ | Toggles dark / light theme |
| ⚙ (gear) | Opens the Manage panel |

**Banners** (appear only when relevant):

- **Update available** — *Full Download* (complete ~93 MB package, always works, fixes corrupted installs), *Install & Restart* (incremental update), ✕ dismiss.
- **Sync Kernels** — your GPU kernel wheels are out of sync; *Sync Kernels* reinstalls them, *Dismiss* hides it.
- **Models location warning** — your data sits inside the roaming AppData profile; *Migrate to new location* moves it out.

**System card** — CPU, RAM, GPU, VRAM, GPU profile badge. Read-only health summary.

**Paths & Model Folders card** — where everything lives:

- *Wan2GP install location* — repo + Python env + settings. Folder icon opens it in Explorer, pencil icon moves it.
- *Checkpoints / LoRAs / Output* — model and output folders, each with open + change buttons.
- Keep models OUT of AppData/Roaming (tens–hundreds of GB, roams with your profile).

**GPU Kernel Wheels card** — per-GPU wheels (torch, triton, sage/flash attention, nunchaku/GGUF) with installed versions. *↻ Sync* reinstalls them for the current GPU.

**Active Environment card** — the selected Python env (`uv`, `venv` or `conda`):

| Button | What it does |
|---|---|
| unlink | Removes the env from the launcher (asks first) |
| restore | Re-installs all packages from `requirements.txt` into the existing env (asks first) |
| reinstall | Recreates the whole env from scratch — fresh Python, PyTorch, packages, kernels. Models, plugins and settings are kept (asks first, takes a while) |
| 🧭 Run Setup | Shown when no env is active — opens the installer for a fresh setup |
| ↻ Check Updates | Checks PyPI for package updates for this env |

**Deepy card** — the WanGP assistant. *LLM Engines* lists backends (Claude Code CLI,
OpenCode, …) with *↻ Refresh* to re-check status; pick one and press *Apply*.

**DLSS 5 card** — optional NVIDIA upsampler runtimes. *Install DLSS 5…* opens a
confirmation (Cancel / Install).

**pip install row** — install any extra PyPI package into the active env, with a
copy button for the equivalent command.

**Launch buttons** — start Wan2GP:

| Button | Mode |
|---|---|
| App/Desktop (green) | Embedded Viewer inside the launcher |
| Browser | Opens your browser when the server is ready |
| Browser No-GPU | Launches Chrome with GPU disabled to free VRAM for generation |
| Terminal | Real Windows Terminal / cmd window |

**Action row** — *Wan2GP Updates* (upstream version management), *Check updates*
(launcher version), *Desktop shortcut*, *Auto-Tune shortcut*.

**Console card** — live launcher + install/launch log with follow toggle.

---

## Installer (Setup)

Opened on first run, via 🧭 Run Setup, or Manage → Run Setup again.

1. **Prerequisites card** (only if something is missing) — Git / Python / uv /
   Miniconda. *Download & Install* fetches it silently, *How to install manually*
   opens the vendor page.
2. **Environment** — pick `uv` (fast, recommended), `venv` (bundled with Python)
   or `conda` (Anaconda/Miniconda). All three end up identical: same pinned Python,
   same torch stack, same smoke test.
3. **Install location + model folders** — repo/env/settings folder plus
   Checkpoints, LoRAs and Output folders, each with Browse / reset-to-default.
   Bare drive roots auto-resolve to `<root>\Wan2GP`.
4. **Install location check** — triage of what's already in the folder:
   - *Empty folder* → nothing to decide, just Install.
   - *Repo without a working env* → tick one of two radios (**Install / repair
     environment**, keeps models & settings — pre-ticked; or **Fresh repo**,
     wipes code and keeps models via the backup flow), then press Install.
   - *Healthy install* → Keep / Update / Reinstall-fresh trio.
   - *Pinokio tree* → Install is blocked; *Reuse its models in a fresh install*
     points your model folders at the Pinokio library, then pick an empty folder.
   - *Foreign files* → Install anyway (not recommended) or Choose empty folder.
5. **Install button** (below the checks) — pops an are-you-sure dialog (choice,
   env type, location, ~5–20 min + several GB). Cancel stops everything.
6. Below: **hardware summary**, **GPU Profile Overview** (Python, Torch/CUDA,
   Triton, attention backends, kernel wheels), **expected packages**, **live
   download rows**, **resolved install stack** with *Validate installation* and
   *Copy diagnostics* (hardware + paths + checks + log tail for Discord/GitHub).

---

## Viewer (Desktop embed)

Wan2GP itself, embedded as a tab. Behaves like the browser version:

- Drag & drop image/video/audio files from Explorer straight into Gradio inputs
  (reference images, etc.).
- New gallery arrivals pop a **Save / Save As…** prompt — Save keeps the file in
  Downloads, Save As… opens the native dialog (reopens at the last-used folder).
- Reload, zoom 25–200%, hide/show keeps the session alive.

---

## Manage → General

- **GitHub Token** — optional, avoids API rate limits for updates/changelog (Save / Clear).
- **HuggingFace Token** — optional, needed for gated models; passed as `HF_TOKEN` on launch.
- **Claude / Anthropic API Key** — for Deepy/Claude paths.
- **Default Browser** — which browser Browser-mode opens.
- **Floating Terminal Default** — where the console docks (bottom/left/top/right/minimised).
- **Desktop** — desktop shortcut + start options.
- **Queue Notifier** — optional ping (via Apprise) when generations finish.
- **Xet Storage (hf_xet)** — fast HuggingFace downloads toggle.
- **GGUF CUDA Kernel** — llama.cpp CUDA offload switch.
- **Repair Settings** — resets launcher settings to defaults.

## Manage → Launch

- **Share Link** — Gradio public tunnel (only if localhost isn't reachable; your server becomes internet-visible).
- **Extra Launch Args** — appended to `wgp.py` verbatim.
- **Server Port** — default 7860.
- **GPU Device** — `auto` or a pinned `cuda:N` for generation.
- **Launcher GPU** — which GPU the launcher UI itself prefers.
- **SageAttention wheel** — which Sage build to use.
- **Bind Address** — `localhost` (matches Gradio's self-check) vs strict `127.0.0.1` for proxy/VPN setups.

## Manage → System

- **Wan2GP Desktop Launcher** — launcher version + update state.
- **Legacy Electron Launcher** — one-click removal of the old launcher (data kept).
- **Updates** — manual-only, version-aware launcher updates.
- **Wan2GP (DeepBeepMeep)** — upstream version + update controls.
- **Setup** — re-opens the installer (fresh / repair / migrate).
- **🛟 Troubleshooting** — **Verify GPU compute** (proves torch + each kernel wheel
  actually *import*, naming the broken dist — run after AV restores, driver
  updates, or env surgery), GPU report, emergency fallback torch probe.
- **Emergency Failsafe** — last-resort recovery when nothing launches.
- **Server Port** — port conflict detection / override.
- **Debug Bundle** — copies full diagnostics (hardware, paths, checks, log tail).
- **Triton / SageAttention** — attention-backend controls.
- **uv Wheel Cache** — cache location / cleanup.

## Manage → Plugins

- **Installed & Available** — plugin list with enable/disable/install/remove.
- **Install from URL** — sideload a plugin from a link.

## Manage → Auto-Tune

⚡ **Performance Auto-Tune** — one click: detects your GPU and applies the right
profile (precision, attention backend, memory knobs). Re-run after GPU/driver changes.

---

## Console & logs

- Dashboard **Console card** and the installer console stream everything.
- **Export** saves the log to a file; **Copy diagnostics** (installer stack +
  System tab) puts the full report on the clipboard for Discord/GitHub.
- The floating console docks bottom/left/top/right, floating, or minimised
  (Manage → General → Floating Terminal Default).

---

## Typical workflows

- **First install:** Dashboard → 🧭 Run Setup (or auto-opened) → pick env type →
  accept folders → Install → confirm → Launch (App).
- **Broken env, keep models:** Active Environment → *reinstall* (or installer →
  tick repair → Install → confirm).
- **Wan2GP update:** Dashboard → Wan2GP Updates (manual, version-aware).
- **Something's wrong:** System → Verify GPU compute, then Copy diagnostics /
  Debug Bundle and paste it in Discord/GitHub.
- **Moving house:** Paths card pencil icons (or installer Migrate) — models,
  plugins and settings survive.

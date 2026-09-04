# Changelog — Wan2GP Desktop Launcher (Tauri)

All notable changes. Dates are release dates; `Unreleased` tracks `master`.

## [Unreleased]

- Launcher update check runs once shortly after boot (5h poll alone left fresh releases unknown for hours)

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

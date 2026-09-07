# Tauri → Electron port — overview

Branch: `docs/electron-port-overview` · Source: `master` @ `b1dd879` (v0.4.6).
App: **Wan2GP Desktop Launcher Tauri** (`com.wangp.desktop-tauri`), ~13.8k lines
total — Rust backend ~5.5k lines (12 modules), frontend ~7.3k lines (vanilla JS,
no framework, no bundler).

## 1. Latest Tauri changes (what the port must include)

Tags stop at `v0.4.3`; `master` is three versions ahead. Port from `master`,
not from any tag.

### v0.4.6 (current) — kernel-status honesty
- Kernel Wheels card moved above Active Environment (IDs unchanged, renderer untouched).
- LightX2V status fix: scan queried dist `lightx2v`, wheel installs
  `lightx2v_kernel` (`install.rs` was right, dashboard lied).
- OpenCV row fix: scan aliased `opencv-python→opencv` (nonexistent dist);
  Check-Updates now targets PyPI `opencv-python`.
- RTX 20/30/40/50 audit vs upstream `setup.py`/`setup_config.json`
  (`hw.rs`): kernel keys aligned to upstream (`nunchaku_cu13`, `light2xv`);
  x050→RTX_50 matches upstream; GTX 16xx→GTX_10 stays an intentional divergence.

### v0.4.5 — embed + stop + upstream parity
- Embed: half-screen collapse fixed (flex base + measured pixel fit,
  `__fitBrowserView`), scrollable zoom slider surviving view rebuilds, full
  Permissions-Policy (downloads/autoplay/camera/mic/PiP/share), toast on every
  gallery save (WebView2 completes downloads silently → Downloads watcher +
  `downloads_since`/`save_downloaded_file` + clickable Save-As toast).
- Chrome probe: repeated negatives required (cold-spawn hiccup flashed
  "not installed"); negatives logged to console.
- Stop actually stops: bootstrap/exe-path process matcher, port-listener kill,
  verify+survivors report. Single always-visible Stop All button
  (Wan2GP + OpenCode); user-initiated stops log as "stopped", never crash.
- Upstream v12.72 parity: Deepy session keys in presets, `deepy_sessions/` in
  backup+restore, `%2B`-safe wheel parsing, **GGUF 1.0.21 override** (docs lead
  upstream `setup_config`, auto-follows when upstream flips), AMD
  `HSA_OVERRIDE_GFX_VERSION` exported at launch (declared upstream, consumed
  by nothing).

### v0.4.4 / v0.4.3 — install honesty + hardening
- Install retry-resume: `.wan2gp-install-ok` completion marker (verify in
  seconds, wipe broken envs first — `setup.py` never resumes into half-built
  envs), one auto-retry of `setup.py` on transient network failure, disk gate
  (<10 GB free aborts before downloading), KeyError crash recovery
  (backup-reset-relaunch).
- Backend hardening: pip-spec guard (blocks `--flags`/`-r`/`-e`/URLs),
  exit-code propagation everywhere, https-only plugin installs, exact Python
  pin preflight (3.11.14) with corrupt-`uv` self-repair, winget-independent
  fallbacks (official installers), Store-shim filter, Intel XPU profile,
  baseline CSP + trimmed `capabilities/default.json`.

## 2. Architecture (what maps to what)

| Tauri today | Electron equivalent | Notes |
|---|---|---|
| `src-tauri/src/*.rs` (12 modules) | `main/` Node modules, ~1:1 file mapping | All logic is portable: child processes, fs, registry reads, HTTP. No Rust-only deps except `sysinfo` → `os` + `systeminformation` pkg |
| `tauri-plugin-shell` | `child_process.spawn/execFile` (`windowsHide: true` = `CREATE_NO_WINDOW`) | Audit every spawn for hidden-console parity |
| `tauri-plugin-opener` | `shell.openPath` / `shell.openExternal` | |
| `tauri-plugin-dialog` | `dialog.showOpenDialog` / `showMessageBox` | `confirm_dialog` + `select_folder` + Save-As picker |
| `tauri-plugin-fs` | `fs` | |
| `tauri-plugin-updater` + `updates.rs` | `electron-updater` + `latest.yml` | Replace minisign `.sig` flow; needs cert or unsigned-NSIS strategy; `check_update`/`download_update`/`install_update` events already mirror it |
| Tauri BrowserViews (`create_browser_view`, `bv_*`, `create_term_view` …) | **No-ops — delete.** Backend stubs return `{ok:true}`; real embed is a plain `<iframe>` built by `w2gp.js` | Biggest simplification: Electron doesn't need any of it |
| `WEBVIEW2_ADDITIONAL_BROWSER_ARGS` GPU pref (`lib.rs`) | `app.commandLine.appendSwitch` before ready | `launcherGpu` (auto/integrated/dedicated/disabled) + legacy `electronGpu` key — same config file |
| `shutdown_cleanup` on window close | `app.on('before-quit')` | Kill tracked child, helper PIDs, Explorer windows |
| `scripts/release-tauri.ps1` | `electron-builder` config + release script | NSIS passive mode, versioned portable exe, GitHub public release (never draft) |
| CSP in `tauri.conf.json` | `session.webRequest` headers + `webPreferences` | `frame-src/connect-src localhost:*` for the Gradio iframe |

## 3. Frontend — mostly portable

- `src/services/*.js` (14 files) are **pure logic, port as-is** (explicitly no-Electron by design).
- `src/app.js` (4.5k lines) is portable except 3 `window.__TAURI__` checks
  (banner hide, term-dock default, browser-view display) — trivial.
- `src/w2gp.js` (177 lines) is the **only file to rewrite**: it bridges
  `window.w2gp.*` → `__TAURI__.core.invoke` + `listen()`. Electron version maps
  the same method names → `ipcRenderer.invoke` / `ipcRenderer.on`
  (via `contextBridge` in preload). Keep every method name stable — `app.js`
  calls ~120 commands through it and must not change.
- `src/term.html` + `src/term.js`: floating-terminal overlay; rewire to
  Electron preload (`contextBridge`), same `\r`/`\n` accumulation logic.
- `src/index.html`/`style.css`: portable (drop Tauri script tags).

## 4. IPC surface (frozen contract)

**~120 commands** registered in `src-tauri/src/lib.rs` — re-expose all under
the same names via `ipcMain.handle`. By module: `system` (folders, dialogs,
migration, browser/launcher prefs, noop view stubs), `hw` (gpu/gpus/hardware/
profile/metrics), `status` (python/git/installed/command checks), `config`
(load/save, paths, disk, models, install-plan, env link/unlink, uv-cache),
`updates` (versions, upstream info, updater trio), `launch` (launch/stop/stop-all,
browsers, external, webview), `features` (packages, Deepy, LLM engines, memory
profiles, auto-tune, notifier, theme), `install` (install/reinstall/uninstall,
sync-kernels, update, DLSS5, classify-target, preflight, prereqs), `plugins`
(list/install/update/uninstall/catalog), `electron` (legacy detect/uninstall).

**7 events** backend→frontend (`emit` counts): `launch-log` (21 sites),
`setup-output` (14), `update-status` (9), `setup-phase` (4),
`migration-progress` (2), `wangp-exit` (2), `install-progress`/`dlss5-progress`
(via progress channels). Map to `webContents.send`; `w2gp.js` `on*` handlers
already abstract them.

Watch out: `electron.rs` (legacy Electron detect + silent NSIS uninstall,
registry + `%LocalAppData%/Programs` scan) ports verbatim to Node — and on
first Electron run it will find the *Tauri-era* Electron install, so keep the
"data kept, only launcher removed" behavior and the Add/Remove Programs
wording accurate in reverse.

## 5. Suggested port order

1. Scaffold (`electron-builder`, main/preload, `ipcMain.handle` stubs returning
   current-backend-shaped fixtures) + rewritten `w2gp.js` preload bridge —
   dashboard renders against stubs first.
2. `base`/`config`/`hw`/`status` (read-only, easily verified).
3. `launch` + process-lifecycle (stop matcher, port-listener kill, cleanup).
4. `install` (largest file, 1.5k lines) + `features`/`plugins` + DLSS5/SHA manifest.
5. `updates` (`electron-updater`), Downloads watcher + Save-As, GPU switches,
   release script, icons/screenshots.
6. Parity pass: v0.4.6 kernel fixes, GGUF/HSA overrides with their
   auto-follow conditions, `%2B` parsing, `deepy_sessions/` backup.

## 6. Risks / gotchas

- Console-window flashes: every probe needs `windowsHide: true` (Tauri got
  this via `CREATE_NO_WINDOW` on each call — easy to miss one in Node).
- Stop-matching semantics (`cc8a0a8`, `8158cdd`) must be preserved exactly or
  Stop regresses to killing all Pythons / nothing.
- WebView2 silent-download watcher → Electron `session.on('will-download')`;
  keep the clickable-toast + native Save-As flow.
- Gradio iframe needs the full `allow=` Permissions-Policy + no `sandbox`
  (see `w2gp.js` comment block) — copy verbatim.
- Trust rule carries over: upstream `setup.py`/`setup_config.json` own the
  profile→package mapping; launcher mirrors, with only the two documented
  overrides (GGUF 1.0.21, HSA export).
- Uncommitted tree noise: `src-tauri/Cargo.toml` shows modified (CRLF) with
  empty diff — normalize or revert before branching real work.

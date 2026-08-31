# wan2gp-tauri-spike — Isolated Tauri Evaluation

> **This folder is 100% standalone. It has zero impact on `../wan2gp-desktop` (`dev`/`main`). Delete it to abort the experiment.**

Scaffolded with `create-tauri-app --template vanilla` on 2026-08-31. Identifier `com.wangp.desktop-tauri` to avoid colliding with Electron `com.wangp.desktop`.

## What this proves (in order)
1. **Frontend reuse** — your `wan2gp-desktop/renderer/` (HTML/CSS/JS) runs in WebView2 unchanged (vs GPUI which requires rewrite).
2. **IPC replacement** — `ipcMain.handle` → `#[tauri::command]` + `invoke()` (see `src-tauri/src/lib.rs` `detect_gpu` + `src/main.js`).
3. **System access** — Rust `std::process::Command` replaces `child_process.spawn/execSync` (nvidia-smi / python / git) without Node.
4. **Embedded Wan2GP** — `WebView2` child webview for `http://localhost:7860` is possible via `Window::add_child` (not yet wired — see next steps).

## Run
```bash
cd E:/DEVELOPMENT/wan2gp-tauri-spike
npm run tauri dev      # desktop dev (requires Rust 1.77.2+, WebView2 on Win)
npm run tauri build    # produces NSIS installer in src-tauri/target/release/bundle/
```

## What's wired in this spike
- `greet` (template) + `detect_gpu` (real nvidia-smi probe, mirrors `main.js` `getGpuInfo()`)
- Frontend calls `invoke("detect_gpu")` and renders result — proves async command + error handling without `preload.js`/`contextBridge`.

## Next steps to validate full migration (do not do in wan2gp-desktop repo)
- `cargo add tauri-plugin-shell tauri-plugin-dialog tauri-plugin-updater tauri-plugin-opener`
- Add `shell:allow-spawn` etc. to `src-tauri/capabilities/default.json`
- Prototype one real handler: `install`, `launch` (spawn python), `launch-log` streaming via `CommandEvent`
- Test embedded Wan2GP: `WebviewWindow` or `add_child` for `localhost:7860`

## Reference copy
- `reference/renderer/` — copied `wan2gp-desktop/renderer/` for diffing (not used at runtime). Compare how little `app.js` would change (mostly `window.w2gp.*` → `invoke()`).

## Delete
```bash
rm -rf E:/DEVELOPMENT/wan2gp-tauri-spike
# or: git worktree remove ../wan2gp-tauri-spike  (if you used worktree)
```

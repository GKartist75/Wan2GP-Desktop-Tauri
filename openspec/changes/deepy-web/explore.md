# Explore — deepy-web (Deepy Web full remote)

## Goal

Implement Deepy Web one-button full remote (Core + Auth + LAN HTTPS + Tailscale) per `docs/DEEPY-WEB-DESIGN.md` on branch `feature/deepy-web`.

## 1. How wgp.py is launched today

- Backend: `src-tauri/src/launch.rs` `launch(app, mode)` — builds `py <boot-shim> wgp.py --server-port <serverPort> --server-name <serverName> --advanced --multiple-images [--share] [--gpu cuda:N] + launchArgs`.
- `serverPort` comes from launcher desktop-config (`load_config_value()`, default 7860); `serverName` default `localhost`. Port-in-use check is `TcpStream::connect(127.0.0.1:port)` → reuse, else spawn via `app.shell().command(py)` with bootstrap shim (fake tty for tqdm), cwd=repo dir, env HF_TOKEN/claude key.
- No `--deepy-server`, `--listen`, `--deepy-sessions-dir`, `--auth` flags emitted today. Extra flags only via Manage-tab `launchArgs` free text.
- Stop path: `stop_wangp` / `stop_all_servers` (taskkill by repo signal + port scan). Deepy Web standalone process will need same lifecycle (separate port, separate kill filter).
- Frontend trigger: `src/app.js` (~L4653/4683/4711) `window.w2gp.launch()` via `src/w2gp.js`; port input at `portInput` ↔ `cfg.serverPort`.

## 2. How wgp_config.json deepy fields work

- Writer today: Rust `src-tauri/src/features.rs` `deepy_set(mode, engine, enhancer)` + legacy pure helper `src/services/deepy-config.js` `setDeepy()` (same logic, JS-testable).
- Fields: `deepy_enabled` (0/1), `deepy_type` (`zero`|`prime`), `enhancer_enabled` (1–5 for disabled/zero — Florence 1/2, Qwen 3/4/5; untouched for remote Prime, forced 5 for local-qwen38 Prime), `enhancer_mode` (0 Automatic dropdown / 1 Enhance Prompt button, default 1, applies in every mode), `prompt_enhancer_quantization` (Qwen backends, Disabled/Zero on 3/4/5 + local-qwen38 Prime), `llm_engines.deepy` + `prompt_enhancer:same_as_deepy` + `profiles.<x>.executable` (literal exe name, never absolute path) + `base_url` for opencode.
- Presets mirrored from upstream `shared/deepy/config.py`: Zero (`deepy_vram_mode`, `context_tokens=16386`, kv `auto`, compaction `discard`, tool picks) and Prime (`prime_custom_system_prompt`, mcp, fs-access, `session_reset_mode=new_session`, `gallery_media_mode=link`, `multi_session=false`). Backup `wgp_config.json.deepy-bak` before every write.
- Reader: `deepy_status()` returns `{mode, deepyEnabled, deepyType, currentEngine, promptEnhancer, enhancerEnabled, engines, promptEnhancerQuantization, sessionMode, sessionResetMode, sessionGalleryMediaMode, enhancerMode}`; UI `refreshDeepy()` renders mode radios + enhancer + engine dots from `llm_engines_list`, plus the quant selector, session dropdowns and the standalone Prompt enhancement card (shared `applyDeepy` write).
- Gap: no `deepyPort`, `deepyAuth`, cert, tailscale, listen fields exist yet — new keys must be launcher-owned (desktop-config, not wgp_config) unless upstream adds them.

## 3. Where to add Deepy Web dashboard card + backend commands

- UI: new card `📱 Deepy Web` — best placement adjacent to `#deepyPrimeCard` in `src/index.html` (~L696-739), reusing `.deepy-*` CSS in `src/style.css` (~L497-518). Elements per design: status, Start/Stop/QR buttons, Same-PC URL + Phone URL + Copy/QR, radio Same-PC/Phone-LAN, Advanced (Port, Auth, Sessions, Engine, Voice). QR modal + Add-to-Home-Screen hint. New `refreshDeepyWeb()` sibling to `refreshDeepy()`, wired in `src/app.js` + `src/w2gp.js` invoke wrappers.
- Backend (`src-tauri/src/`): new module e.g. `deepy_web.rs` (or extend `features.rs`+`launch.rs`) with commands: `deepy_web_preflight`, `deepy_web_start`, `deepy_web_stop`, `deepy_web_status`, `deepy_web_qr` (or QR purely frontend via lib), `deepy_web_cert`, `deepy_web_tailscale`. Register in `src-tauri/src/lib.rs` `invoke_handler!`.
- Config/init reuse: preflight + start must call existing `deepy_set` logic (auto Zero+Qwen3.5-4B, enhancer 1/2→3 fallback, Prime-local 27B check, `deepy_sessions` dir ensure per `install.rs:3026/3144`, `--config` + `--deepy-sessions-dir` passthrough). Standalone = separate process on `deepyPort`, so finish→stop→resume handoff warning needed (design Notes).

## 4. Port strategy, LAN IP, firewall, auth, HTTPS, tailscale

- Port: `deepyPort = serverPort + 1` default (7861 when main is 7860); preflight checks free via TcpStream; Gradio clash warning when both processes side-by-side. Persist override in desktop-config; validate 1-65535, ≠ serverPort.
- Launch args: Same-PC `wgp.py --deepy-server --server-port N`; LAN appends `--listen`. Reuse common builder in `launch.rs` (extend `args` vec, don't fork).
- LAN IP: enumerate non-loopback IPv4 (Rust `local-ipaddress` crate or `ipconfig` parse — check Cargo deps first); never display `0.0.0.0`; show `http://<lan-ip>:<deepyPort>`.
- Firewall: Windows Defender prompt is automatic on first `--listen` bind; add hint text + optional `netsh advfirewall` doc link (no silent rule creation without consent).
- Auth v1.1: `--auth` toggle; password Generate (random) vs Fixed via `WANGP_AUTH_PASSWORD` env (never CLI arg, never logged); 24h expiry banner; rate-limit note; never send password over plain HTTP (warn on LAN-HTTP + auth).
- LAN HTTPS v1.2: bring-own `.pem/.key` + Create-LAN-cert via `mkcert`; phone CA-install guide; mic requires trusted HTTPS — gate voice features on HTTPS.
- Tailscale v1.3: detect (`tailscale status` / exe probe) → install link → show `https://<host>.<tailnet>.ts.net` URL; router-VPN docs-only (firmware variance, UPnP/CGNAT unreliable). Public NAT rule: auth + trusted HTTPS, forward HTTPS-only.

## Suggested slice order (for spec/tasks)

1. Core: preflight + auto-config + start/stop/status + card + URLs (Same-PC + LAN `--listen`).
2. Auth: password generate/fixed + expiry banner + HTTP guard.
3. LAN HTTPS: cert bring/create + HTTPS start + CA guide.
4. Tailscale: detect/install + tailnet URL + router-VPN docs.

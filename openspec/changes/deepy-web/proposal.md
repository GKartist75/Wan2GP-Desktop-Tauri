# Proposal — deepy-web (Deepy Web full remote)

## Status

- Change: `deepy-web`
- Branch: `feature/deepy-web`
- Choice: Full remote (Core + Auth v1.1 + LAN HTTPS v1.2 + Tailscale v1.3)
- Pre-proposal handoff: confirmed — no further product discovery in this phase
- Sources: `docs/DEEPY-WEB-DESIGN.md` + `openspec/changes/deepy-web/explore.md`
- Execution: auto · Store: openspec · Delivery: ask-on-risk · Budget: 400 changed lines

## Problem statement

Deepy Web (the phone-friendly standalone chat UI served by upstream `wgp.py --deepy-server`) is powerful but unreachable for non-technical users: starting it today requires hand-editing config, picking engines, fixing enhancer values, choosing session dirs, and typing CLI flags (`--deepy-server`, `--listen`, `--auth`). There is no one-button path from the launcher dashboard to a working Same-PC URL, let alone a phone-on-LAN URL with QR, auth, HTTPS (mic), or off-LAN (Tailscale) access. Users get stuck, misconfigure `wgp_config.json`, clash ports with the main Gradio server, or expose an unauthenticated HTTP listener on LAN without understanding the risk.

## Users

- **Primary:** desktop launcher users who want to chat with Deepy from their phone (same Wi-Fi) without touching CLI flags.
- **Secondary:** users who need off-LAN access (Tailscale) or voice input (mic → requires trusted HTTPS).
- **Moments:** Dashboard → `📱 Deepy Web` card → Start → scan QR → chat on phone; Add-to-Home-Screen (iOS Share > Add, Android Install).
- **Non-users:** upstream CLI-only operators; router-VPN tweakers (docs-only, not a launcher flow).

## Product outcome

One click: preflight → auto install/initiate → start phone-friendly Deepy Web. After the change:

- Dashboard card `📱 Deepy Web` shows status, Start/Stop/QR, Same-PC URL + Phone URL with Copy/QR, radio Same-PC-only / Phone-LAN, Advanced (Port, Auth, Sessions, Engine, Voice).
- Same-PC works out of the box; Phone-LAN shows a real LAN IPv4 URL + QR (never `0.0.0.0`) with a firewall hint.
- Auth protects remote access (generated or fixed password, 24h-expiry banner, HTTP guard).
- Mic/voice is gated on trusted LAN HTTPS (bring-own cert or mkcert-created cert + phone CA-install guide).
- Off-LAN works over Tailscale tailnet URL with detect/install guidance; router VPN stays docs-only.
- Standalone-vs-Gradio `/deepy/` confusion is handled with a finish → stop → resume handoff warning.

## Intent

Implement the full remote per `docs/DEEPY-WEB-DESIGN.md` on branch `feature/deepy-web`, reusing existing launch/config/session machinery and adding only the launcher-owned surface (dashboard card + `deepy_web_*` commands + desktop-config keys).

## Scope slices

### Slice 0 — Core (one-button Same-PC + Phone-LAN HTTP)

- `deepy_web_preflight`: installed + torch check, port free (default `deepyPort = serverPort + 1`, e.g. 7861), Gradio-clash warning.
- Auto install/initiate when Disabled: Zero + Qwen3.5 4B + backup; enhancer 1/2 → 3 fix; Prime-local needs 27B else fallback; sessions = multisession dedicated + file copy; reuse `--config` + `--deepy-sessions-dir`.
- `deepy_web_start` / `stop` / `status`: Same-PC `wgp.py --deepy-server --server-port N`; LAN appends `--listen`. Separate process on `deepyPort` with same lifecycle pattern as `stop_wangp` (port-scoped kill filter).
- Card UI adjacent to `#deepyPrimeCard` (`src/index.html` ~L696-739, `.deepy-*` CSS `src/style.css` ~L497-518), `refreshDeepyWeb()` sibling to `refreshDeepy()`, invoke wrappers in `src/w2gp.js`.
- URLs: Same-PC `http://localhost:<deepyPort>` (or `serverName`); Phone `http://<lan-ip>:<deepyPort>`; Copy + QR modal + Add-to-Home-Screen hint; handoff warning (standalone ≠ Gradio `/deepy/` live sync).

### Slice 1 — Auth v1.1

- `--auth` toggle; password modes Generate (random) vs Fixed via `WANGP_AUTH_PASSWORD` env only.
- 24h-expiry banner; rate-limit note; never send password over plain HTTP (warn on LAN-HTTP + auth).

### Slice 2 — LAN HTTPS v1.2

- Bring-own `.pem`/`.key` + Create-LAN-cert (mkcert) + phone CA-install guide.
- HTTPS start path; mic/voice features gated on trusted HTTPS.

### Slice 3 — Tailscale v1.3

- Detect (`tailscale status` / exe probe) → install link → show `https://<host>.<tailnet>.ts.net` URL.
- Router VPN docs-only. Public NAT rule surfaced in UI/docs: auth + trusted HTTPS, forward HTTPS-only.

## Non-goals

- No public NAT / port auto-forward (no UPnP/NAT-PMP automation; CGNAT makes it unreliable).
- No silent firewall changes (no unattended `netsh advfirewall` rules; hint text + doc link + OS prompt only).
- No silent cert changes (no unattended CA install / mkcert trust; explicit user action + guide only).
- No upstream flag renames or forks (use `--deepy-server` / `--listen` / `--auth` as upstream defines them).
- No router-VPN automation (firmwares differ → docs-only).
- No live sync between standalone Deepy Web process and Gradio `/deepy/` view (handoff warning instead).

## Constraints

- Upstream owns `wgp.py` flags: `--deepy-server`, `--listen`, `--auth` (launcher only composes them; common arg builder in `launch.rs`, no fork).
- Launcher owns desktop-config keys: `deepyPort`, `deepyAuth`, cert paths, tailscale/listen prefs (new keys live in desktop-config, NOT in `wgp_config.json`, unless upstream adds them).
- Password via env only: `WANGP_AUTH_PASSWORD`; never as CLI arg, never logged, never over plain HTTP without warning.
- Never display or advertise `0.0.0.0`; enumerate non-loopback LAN IPv4 for the Phone URL.
- Mic requires trusted HTTPS; gate voice features accordingly.
- Port validation: 1–65535, `deepyPort ≠ serverPort`; preflight via `TcpStream::connect` check.
- `profiles.<x>.executable` stays a literal exe name (never absolute path); backup `wgp_config.json.deepy-bak` before every config write (existing `deepy_set` contract).
- Review budget: 400 changed lines; delivery `ask-on-risk` (pause and ask on budget/scope risk, do not invent chain/exception).

## Affected areas

- Frontend: `src/index.html` (new card), `src/style.css` (`.deepy-*` reuse), `src/app.js` (`refreshDeepyWeb()`, Start/Stop/QR wiring), `src/w2gp.js` (invoke wrappers).
- Backend (`src-tauri/src/`): new `deepy_web.rs` (preferred) or extend `features.rs` + `launch.rs`; commands `deepy_web_preflight/start/stop/status/qr/cert/tailscale`; register in `lib.rs` `invoke_handler!`; reuse `deepy_set`, `install.rs` sessions ensure, `launch.rs` arg builder + env pattern.
- Config: launcher desktop-config (new keys); `wgp_config.json` writes only through existing `deepy_set` semantics.
- Docs: CA-install guide, Tailscale install link, router-VPN docs-only note, firewall hint.

## Risks

- **Review-budget overrun (400 lines):** full remote (4 slices + card + backend + guides) likely exceeds one PR → mitigation: spec/tasks slice per section above; `ask-on-risk` gate before delivery (ask, don't chain/invent exception).
- **Upstream flag drift:** if upstream renames `--deepy-server/--listen/--auth`, launcher breaks → mitigation: no rename/fork; pin + document expected upstream behavior; preflight surfaces launch errors.
- **LAN IP enumeration:** crate availability (`local-ipaddress`) vs `ipconfig` parsing variance → mitigation: check Cargo deps first; fallback gracefully; never show `0.0.0.0`.
- **Auth over HTTP footgun:** user enables LAN + auth but stays on HTTP → mitigation: blocking-style warning banner; password never transmitted without HTTPS warning.
- **Cert/CA friction:** mkcert + phone CA install is multi-step and OS-specific → mitigation: bring-own path + step guide; gate mic on HTTPS so failure is legible.
- **Tailscale absent:** binary missing / not logged in → mitigation: detect → install link → tailnet URL only when `tailscale status` succeeds.
- **Two-process confusion:** standalone Deepy Web vs main Gradio server side-by-side (7860+7861) → mitigation: Gradio-clash warning + finish→stop→resume handoff copy.

## Rollback

- Feature is additive (new card + new commands + new desktop-config keys). Rollback = revert `feature/deepy-web` branch / PR; existing `launch.rs`, `features.rs deepy_set`, and `wgp_config.json` paths unchanged.
- Config writes keep `.deepy-bak` backups; stop path kills only the `deepyPort`-scoped process.

## Success criteria

- One click from Disabled state yields working Same-PC Deepy Web URL.
- Phone-LAN radio yields scannable QR with a real LAN IPv4 URL (no `0.0.0.0`), firewall hint shown on first `--listen`.
- Auth on → password via env only, expiry banner visible, HTTP warning shown when applicable.
- LAN HTTPS on → trusted-HTTPS URL, CA guide reachable, mic gated until HTTPS.
- Tailscale on → tailnet URL shown when detected; install guidance otherwise; router VPN stays docs-only.
- Full slice set stays within review-budget process via `ask-on-risk` (no silent chain, no silent exception).

## Proposal question round

Not run — pre-proposal handoff confirmed Full remote on `feature/deepy-web`; per instructions, no user interview in this phase.

# Design — deepy-web (Deepy Web full remote)

## Overview

Add one-button phone-friendly Deepy Web to the launcher dashboard per
`docs/DEEPY-WEB-DESIGN.md`, `proposal.md`, and `specs/deepy-web/spec.md`.
Upstream `wgp.py` owns `--deepy-server` / `--listen` / `--auth`; the launcher
only composes them. New surface is launcher-owned: one dashboard card, one new
backend module, launcher desktop-config keys. No fork of `launch.rs` arg logic,
no writes to `wgp_config.json` outside existing `deepy_set` semantics, no
silent firewall/CA changes.

## Decisions

### 1. New module `src-tauri/src/deepy_web.rs` (vs extending `features.rs`/`launch.rs`)

- **Decision:** new file `src-tauri/src/deepy_web.rs` + `mod deepy_web;` in `lib.rs`.
- **Rationale:** `features.rs` owns Deepy config presets (`deepy_set`); `launch.rs`
  owns main Gradio lifecycle (`launch`, `stop_wangp`). Deepy Web is a second
  concurrent lifecycle (own port, own process, own preflight/HTTPS/Tailscale).
  A dedicated module keeps the kill-filter and port scoping auditable and keeps
  the diff out of two already-large files.
- **Reuse, not fork:** `deepy_web.rs` calls into existing helpers — `features::deepy_set`
  logic for auto-config, `install.rs` sessions-dir ensure, and a shared arg-builder
  extracted from `launch.rs` (see §4). No duplicated spawn/shim/env code; the common
  builder lives in `launch.rs` and is imported (`pub(crate) fn`).

### 2. Commands + `lib.rs` registration

New `#[tauri::command]`s in `deepy_web.rs`, registered in `lib.rs`
`invoke_handler!` alongside existing entries:

| Command | Inputs | Returns |
| --- | --- | --- |
| `deepy_web_preflight` | `{ deepyPort?, mode }` | `{ installed, torch, portFree, clashNotice?, errors[] }` |
| `deepy_web_start` | `{ mode: same-pc\|lan, deepyPort, auth: {enabled, passwordMode: generate\|fixed}, https: {enabled, certPath?, keyPath?} }` | `{ running, port, mode, urls }` or error |
| `deepy_web_stop` | `{ deepyPort }` | `{ stopped }` |
| `deepy_web_status` | `{ deepyPort }` | `{ running, port, mode, urls }` |
| `deepy_web_cert` | `{ action: bring\|create, certPath?, keyPath? }` | `{ certPath, keyPath }` or error |
| `deepy_web_tailscale` | `{}` | `{ present, loggedIn, tailnetUrl? }` |

- QR is **frontend-only** (no `deepy_web_qr` backend command): encode the already-known
  URL string in JS (see §7). Keeps binary surface minimal.
- `deepy_web_status` is pollable by `refreshDeepyWeb()`; `start` returns the same shape
  so the card renders without a second round-trip.

### 3. Desktop-config keys (launcher-owned, NOT `wgp_config.json`)

```jsonc
{
  "deepyPort": 7861,          // default serverPort+1, override persisted
  "deepyListen": false,       // false = Same-PC-only, true = Phone-LAN
  "deepyAuthMode": "off",     // off | generate | fixed
  "deepyCertPath": "",        // bring-own .pem
  "deepyKeyPath": "",         // bring-own .key
  "deepyTailscale": false     // show tailnet section
}
```

- Password is **never** stored here (env-only, §6). Cert/key store paths only.
- `wgp_config.json` writes go exclusively through existing `deepy_set` semantics
  (Zero + Qwen3.5-4B auto-config, enhancer 1/2→3 fix, Prime-local 27B check/fallback,
  `.deepy-bak` backup, literal-exe-name rule).

### 4. `launch.rs` common arg-builder reuse

- Extract `build_wgp_args(base: &WgpLaunchBase) -> Vec<String>` in `launch.rs`
  (`pub(crate)`), covering python shim, `--config` + `--deepy-sessions-dir`
  passthrough, `--server-port N`, shared env (`HF_TOKEN`, claude key).
- `deepy_web_start` composes on top:
  - Same-PC: `wgp.py --deepy-server --server-port <deepyPort>` (+ shared args).
  - LAN: append `--listen`.
  - Auth on: append `--auth`; password via `WANGP_AUTH_PASSWORD` env on the child
    (never CLI arg, never logged).
  - HTTPS: append upstream HTTPS/cert flags as defined by upstream (passthrough;
    launcher does not invent flag names).
- Spawning reuses the `launch.rs` pattern: `app.shell().command(py)` + bootstrap
  shim (fake tty for tqdm), cwd=repo dir.

### 5. Port check via `TcpStream`

- Preflight + `status`: `TcpStream::connect(127.0.0.1:<deepyPort>)` with short timeout.
  Connect-ok ⇒ occupied (`portFree=false` + guidance); refused ⇒ free.
- Validation before start: 1–65535, integer, `deepyPort != serverPort`; reject with
  field-specific error otherwise.
- Gradio-clash notice: if main `serverPort` is bound AND `deepyPort` is free, return
  non-blocking `clashNotice` (two processes side-by-side, e.g. 7860+7861).

### 6. LAN IP: `local-ipaddress` crate vs `ipconfig` fallback

- **Primary:** add `local-ipaddress` crate; call `local_ip()` / enumerate, filter
  non-loopback IPv4. Small, no subprocess, testable.
- **Fallback:** if crate returns loopback/none, run `ipconfig` parse for
  `IPv4 Address` lines (existing Windows-only target), else Phone URL renders
  "unavailable + guidance".
- **Invariant:** never display or advertise `0.0.0.0`. Same-PC URL uses
  `http://localhost:<deepyPort>` (or `serverName`); Phone URL uses
  `http://<lan-ip>:<deepyPort>`.

### 7. Stop scoping by `deepyPort`

- `deepy_web_stop({deepyPort})` reuses the `stop_wangp` / `stop_all_servers` pattern
  (repo-signal + port scan) but filters kills to processes bound to `deepyPort` only.
- MUST NOT kill the main Gradio server on `serverPort`. Port is a required arg so a
  stale-config stop cannot widen the filter.

### 8. Frontend: card placement + `refreshDeepyWeb()` + `w2gp.js` + QR lib

- **Placement:** new `📱 Deepy Web` card adjacent to `#deepyPrimeCard` in
  `src/index.html` (~L696-739); reuse `.deepy-*` classes in `src/style.css`
  (~L497-518) plus minimal `.deepyweb-*` additions.
- **Logic:** `refreshDeepyWeb()` in `src/app.js`, sibling to `refreshDeepy()`:
  calls `deepy_web_status`, renders status/buttons/URLs/radio/Advanced
  (Port, Auth, Sessions, Engine, Voice), firewall hint, 24h-expiry banner,
  HTTP-auth guard, HTTPS mic gate, handoff warning, CA-guide + Tailscale entries.
- **`src/w2gp.js` wrappers:** `deepyWebPreflight/start/stop/status/cert/tailscale()`
  thin `invoke()` wrappers mirroring `window.w2gp.launch()` style.
- **QR lib choice (frontend-only):** vendored minimal QR encoder (e.g. `qrcodejs`-style
  single-file, no build step — matches vanilla-JS frontend) rendering the selected
  URL into the existing modal pattern; Copy buttons via `navigator.clipboard` with
  fallback. No backend QR command, no new npm dependency chain. Add-to-Home-Screen
  hint is static copy (iOS Share > Add, Android Install).

### 9. Cert via mkcert bring/create

- **Bring:** `deepy_web_cert({action:"bring", certPath, keyPath})` validates presence +
  readability, persists paths to desktop-config, returns them for the HTTPS start.
- **Create:** `action:"create"` shells `mkcert` (must already be on PATH; launcher does
  not install it silently) to generate a LAN-usable pair into the launcher data dir,
  persists paths, returns them. No unattended `mkcert -install` / CA trust — explicit
  user click + phone CA-install guide step.
- **Gate:** missing/unreadable cert blocks HTTPS start with field-specific error. Mic/
  voice blocked on plain HTTP with "requires trusted LAN HTTPS + CA install" message.
- **Transport (upstream DEEPY.md):** pass `--ssl-certfile` / `--ssl-keyfile` verbatim
  (`WANGP_SSL_CERT` / `WANGP_SSL_KEY` env fallback, CLI wins); with `--https-port` the
  HTTP port only redirects, never serves cleartext; bad pairs fail startup closed.

### 10. Tailscale detect

- `deepy_web_tailscale()`: probe `tailscale` binary on PATH → run `tailscale status`
  (short timeout) → parse host/tailnet → return
  `https://<host>.<tailnet>.ts.net` URL only on success.
- Absent/not-logged-in ⇒ `{ present:false }` or `{ loggedIn:false }` + install/login
  link; card MUST NOT show a tailnet URL. Router VPN stays docs-only text (no buttons).

### 11. Error surfacing for upstream flag drift

- Launcher pins expected upstream flags (`--deepy-server`, `--listen`, `--auth`) and
  treats spawn/stderr failures as first-class: `deepy_web_start` captures early child
  stderr (bounded wait) and returns `{ error, hint }` (e.g. "unrecognized argument:
  check upstream wgp.py version") instead of fake-success. `preflight`/`status` never
  mask launch errors; no flag renames or forks in the launcher.

### 12. No silent firewall/CA changes

- Firewall: first `--listen` bind triggers the OS prompt naturally; card shows hint
  text + doc link. No unattended `netsh advfirewall` rules.
- CA/certs: no unattended CA install, no silent trust. Every step is explicit-click +
  guide. This is also a spec requirement (`No silent security-sensitive changes`).

## Data flow

```
Card (index.html) → app.js refreshDeepyWeb() → w2gp.js invoke wrappers
  → deepy_web_preflight → {installed, torch, portFree, clashNotice}
  → deepy_web_start {mode, port, auth(env), https} → launch.rs builder → wgp.py child
  → deepy_web_status poll → URLs (localhost + lan-ip) → QR modal (frontend-only)
  → deepy_web_stop {deepyPort} → port-scoped kill
  → deepy_web_cert / deepy_web_tailscale → desktop-config keys + guides
```

## File changes

| File | Change |
| --- | --- |
| `src-tauri/src/deepy_web.rs` | NEW: 6 commands + preflight/port/LAN-IP/stop-scope logic |
| `src-tauri/src/launch.rs` | Extract `pub(crate) build_wgp_args`; no behavior change to `launch` |
| `src-tauri/src/lib.rs` | `mod deepy_web;` + 6 command registrations |
| `src-tauri/Cargo.toml` | Add `local-ipaddress` dependency |
| `src/index.html` | New card adjacent to `#deepyPrimeCard` + QR modal + guide entries |
| `src/style.css` | Reuse `.deepy-*`, add minimal `.deepyweb-*` |
| `src/app.js` | `refreshDeepyWeb()` + Start/Stop/QR/copy/radio/Advanced wiring |
| `src/w2gp.js` | 6 thin invoke wrappers |
| Desktop-config | 6 new keys (§3); password never persisted |

## Contracts

- `deepyPort` default `serverPort+1`; validate 1–65535, `≠ serverPort`.
- `WANGP_AUTH_PASSWORD` via child env only; never CLI/log/config.
- Never render `0.0.0.0`; Phone URL degrades to unavailable+guidance.
- Stop filter scoped to `deepyPort`; main server untouched.
- `wgp_config.json` writes keep `.deepy-bak` + literal-exe-name rule.
- Upstream flags used verbatim; drift surfaces as explicit launch error.

## Tests

- `cargo test --manifest-path src-tauri/Cargo.toml`: port validation (0/99999/nonnumeric/
  equal-to-serverPort rejected; default `serverPort+1`), URL builder (loopback excluded,
  `0.0.0.0` never emitted), stop-filter scoping (unit on filter predicate), auth env
  (password absent from argv/logs), cert validation (missing/unreadable blocks).
- `cargo clippy` + `cargo check` per repo quality gates. Frontend: manual card flows
  (Same-PC start, LAN+QR, auth banner, HTTPS gate, Tailscale absent/present).

## Rollout

- Additive only; rollback = revert `feature/deepy-web`. Slices per spec: Core →
  Auth → LAN HTTPS → Tailscale. Review budget 400 lines with `ask-on-risk` gate —
  pause and ask on overrun; no invented chain/exception.

## Tradeoffs

| # | Option A (chosen) | Option B (rejected) | Why |
| --- | --- | --- | --- |
| 1 | New `deepy_web.rs` module | Extend `features.rs` + `launch.rs` in place | Second concurrent lifecycle (own port/process/preflight/HTTPS/Tailscale) deserves isolation; keeps kill-filter auditable; avoids bloating two large files. Cost: one more module + imports. |
| 2 | Extract common arg-builder in `launch.rs` | Fork spawn logic into `deepy_web.rs` | Fork drifts (shim/env/cwd diverge); shared builder keeps `--config`/`--deepy-sessions-dir`/env consistent. Cost: small refactor of `launch.rs` touched by both paths. |
| 3 | `local-ipaddress` crate primary, `ipconfig` fallback | `ipconfig` parse only | Crate is cheaper, testable, no subprocess encoding issues; fallback covers crate gaps on odd adapters. Cost: one new dependency. |
| 4 | `TcpStream::connect` port check (existing pattern) | Bind-then-release or `netstat` parse | Matches current `launch.rs` behavior, zero new deps, no TOCTOU worse than alternatives. Cost: same inherent check-then-spawn race (accepted; start failure surfaces explicitly). |
| 5 | QR frontend-only (vendored encoder) | Backend `deepy_web_qr` returning PNG | URL is already client-side; backend QR adds command + image pipeline for no benefit. Cost: vendored JS must be reviewed once. |
| 6 | mkcert bring/create, explicit-click only | Auto-install CA / silent trust | Silent CA changes are a security violation and a spec non-goal; explicit flow is legible and matches "no silent security-sensitive changes". Cost: more user steps (guide mitigates). |
| 7 | Password via `WANGP_AUTH_PASSWORD` env only | CLI arg or desktop-config storage | CLI leaks via process list/logs; config storage persists secrets. Env-only is the only acceptable transport. Cost: generated passwords must be shown once + 24h-expiry banner. |
| 8 | Tailscale detect/probe, docs-only router VPN | Automate port-forward (UPnP/NAT-PMP) | CGNAT + firmware variance make automation unreliable and risky; Tailscale is the supported off-LAN path. Cost: users without Tailscale get guidance, not magic. |
| 9 | Surface upstream flag drift as explicit errors | Pin/fork flag handling in launcher | Launcher must not rename/fork upstream flags; explicit stderr surfacing keeps breakage diagnosable. Cost: UX depends on upstream error text quality. |
| 10 | Stop scoped by required `deepyPort` arg | Global "stop all" reuse | Global stop would kill the main Gradio server; port-scoped filter is the only safe semantic. Cost: callers must thread the port through. |

## Risks

- 400-line budget vs 4-slice scope → `ask-on-risk` pause, slice per PR section.
- Upstream flag rename → mitigated by §11 explicit error surfacing.
- `local-ipaddress` missing odd adapter → `ipconfig` fallback + graceful unavailable state.
- LAN-HTTP + auth footgun → blocking-style warning banner.
- mkcert/CA friction → bring-own path + guide; mic gated on HTTPS.
- Two-process confusion (7860+7861) → clash notice + finish→stop→resume handoff copy.

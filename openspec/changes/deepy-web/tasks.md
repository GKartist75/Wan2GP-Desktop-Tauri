# Tasks — deepy-web (Deepy Web full remote)

## Context

- Change: `deepy-web` on branch `feature/deepy-web`
- Sources: `openspec/changes/deepy-web/explore.md`, `proposal.md`, `specs/deepy-web/spec.md`, `design.md`, `openspec/config.yaml`
- Execution: auto · Store: openspec · Delivery: ask-on-risk · Budget: 400 changed lines
- Testing: `cargo test --manifest-path src-tauri/Cargo.toml` (strict TDD: RED → GREEN → TRIANGULATE → REFACTOR); frontend vanilla-JS verified manually
- Slice order: Core → Auth → LAN HTTPS → Tailscale

## Review Workload Forecast

| Field | Value |
| ------- | ------- |
| Estimated changed lines | ~900–1200 (additions + deletions) |
| 400-line budget risk | High |
| Chained PRs recommended | Yes |
| Suggested split | PR 1 Core backend → PR 2 Core frontend card → PR 3 Auth → PR 4 LAN HTTPS + Tailscale |
| Delivery strategy | ask-on-risk |
| Chain strategy | pending |

```text
Decision needed before apply: Yes
Chained PRs recommended: Yes
Chain strategy: pending
400-line budget risk: High
```

Forecast rationale: new backend module `src-tauri/src/deepy_web.rs` (6 commands + preflight/port/LAN-IP/stop-scope, ~300 lines) + `launch.rs` builder extract + `lib.rs` + `Cargo.toml` dep + frontend card (`index.html`, `style.css`, `app.js` `refreshDeepyWeb()`, `w2gp.js` wrappers, vendored QR, ~300–400 lines) + unit tests (~150 lines) + Auth/HTTPS/Tailscale UI copy and guides. Four slices cannot fit one 400-line PR; do NOT auto-chain — pause for delivery decision per ask-on-risk.

## Slice 0 — Core (Same-PC + Phone-LAN HTTP)

### Core RED — port validation + URL builder failing tests

- [x] Define Core RED tests for port validation and URL building in `src-tauri/src/deepy_web.rs` (new `#[cfg(test)]` module: default `serverPort+1`, reject 0/99999/non-numeric/`deepyPort == serverPort`, loopback excluded, never emit `0.0.0.0`, unavailable+guidance state). Files: `src-tauri/src/deepy_web.rs`. Acceptance: `cargo test --manifest-path src-tauri/Cargo.toml` compiles and new tests FAIL (module stubs missing). Test: `cargo test --manifest-path src-tauri/Cargo.toml deepy_web`. <!-- sdd-owner: implementation -->

### Core GREEN — preflight/start/stop/status backend + registration

- [x] Implement Core GREEN backend: new `src-tauri/src/deepy_web.rs` (`deepy_web_preflight`, `deepy_web_start` Same-PC `--deepy-server --server-port <deepyPort>` / LAN appends `--listen`, `deepy_web_stop` port-scoped, `deepy_web_status`), extract `pub(crate) build_wgp_args` in `src-tauri/src/launch.rs` (no behavior change to `launch`), register `mod deepy_web` + commands in `src-tauri/src/lib.rs`, add `local-ipaddress` to `src-tauri/Cargo.toml`, desktop-config keys `deepyPort`/`deepyListen`. Files: `src-tauri/src/deepy_web.rs`, `src-tauri/src/launch.rs`, `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml`. Acceptance: RED tests go GREEN; Same-PC args exclude `--listen`, LAN args include it; stop filter requires `deepyPort`. Test: `cargo test --manifest-path src-tauri/Cargo.toml` + frontend manual N/A. <!-- sdd-owner: implementation -->

### Core TRIANGULATE — clash notice, auto-config reuse, stop scoping, flag-drift errors

- [x] Harden Core backend triangulation in `src-tauri/src/deepy_web.rs`: Gradio-clash notice (main `serverPort` bound + `deepyPort` free), auto-config reuse via existing `features::deepy_set` (Disabled→Zero+Qwen3.5-4B, enhancer 1/2→3, Prime-local 27B check/fallback, `.deepy-bak` backup, literal-exe-name rule), `install.rs` sessions-dir ensure + `--config`/`--deepy-sessions-dir` passthrough, port-scoped kill predicate unit test, upstream flag-drift surfaces `{error,hint}` from early child stderr. Files: `src-tauri/src/deepy_web.rs`, `src-tauri/src/features.rs` (call only, no semantic change). Acceptance: clash/auto-config/stop-scope/flag-drift unit tests pass; main server on `serverPort` untouched by stop. Test: `cargo test --manifest-path src-tauri/Cargo.toml`. <!-- sdd-owner: implementation -->

### Core REFACTOR + frontend card — dashboard UI, URLs, QR, hints

- [x] Build Core frontend card and refactor backend for clippy: `📱 Deepy Web` card adjacent to `#deepyPrimeCard` in `src/index.html` (~L696-739) + QR modal + Add-to-Home-Screen hint, reuse `.deepy-*` plus minimal `.deepyweb-*` in `src/style.css` (~L497-518), `refreshDeepyWeb()` + Start/Stop/QR/copy/radio/Advanced wiring in `src/app.js`, thin `deepyWebPreflight/start/stop/status()` invoke wrappers in `src/w2gp.js`, frontend-only vendored QR encoder (no backend QR command, no npm chain), Same-PC `http://localhost:<deepyPort>` + Phone `http://<lan-ip>:<deepyPort>` + Copy/QR, firewall hint + doc link (no `netsh`), handoff warning (finish→stop→resume). Files: `src/index.html`, `src/style.css`, `src/app.js`, `src/w2gp.js`, `src-tauri/src/deepy_web.rs` (refactor only). Acceptance: card renders status/buttons/URLs/radio/Advanced; QR encodes selected URL; Phone URL never `0.0.0.0`; `cargo clippy` + `cargo check` clean. Test: `cargo test --manifest-path src-tauri/Cargo.toml`; frontend manual: Same-PC start, LAN+QR, stop leaves main server running. <!-- sdd-owner: implementation -->

## Slice 1 — Auth v1.1

### Auth RED — env-only password failing tests

- [x] Define Auth RED tests in `src-tauri/src/deepy_web.rs`: `--auth` appends flag, password present only in child env `WANGP_AUTH_PASSWORD`, absent from argv/logs/config (generate vs fixed modes). Files: `src-tauri/src/deepy_web.rs`. Acceptance: new auth tests FAIL before implementation. Test: `cargo test --manifest-path src-tauri/Cargo.toml deepy_web_auth`. <!-- sdd-owner: implementation -->

### Auth GREEN — start with auth + expiry banner + HTTP guard UI

- [x] Implement Auth GREEN: `deepy_web_start` auth input `{enabled, passwordMode: generate|fixed}` passing `WANGP_AUTH_PASSWORD` via child env only, desktop-config `deepyAuthMode` (`off|generate|fixed`, password never persisted), card 24h-expiry banner + regenerate/restart action + rate-limit note, blocking-style LAN-HTTP+auth warning in `src/app.js`/`src/index.html`. Files: `src-tauri/src/deepy_web.rs`, `src/app.js`, `src/index.html`, `src/w2gp.js`. Acceptance: argv/logs contain no secret; banner + HTTP guard render when Auth on without HTTPS. Test: `cargo test --manifest-path src-tauri/Cargo.toml`; frontend manual: generate/fixed start, banner visible, LAN-HTTP warning shown. <!-- sdd-owner: implementation -->

## Slice 2 — LAN HTTPS v1.2

### HTTPS RED — cert validation failing tests

- [x] Define HTTPS RED tests in `src-tauri/src/deepy_web.rs`: missing/unreadable `.pem`/`.key` blocks HTTPS start with field-specific error; valid paths pass. Files: `src-tauri/src/deepy_web.rs`. Acceptance: new cert tests FAIL before implementation. Test: `cargo test --manifest-path src-tauri/Cargo.toml deepy_web_cert`. <!-- sdd-owner: implementation -->

### HTTPS GREEN — bring/create cert + HTTPS start + mic gate + CA guide

- [x] Implement HTTPS GREEN: `deepy_web_cert({action: bring|create})` (bring validates readability + persists `deepyCertPath`/`deepyKeyPath`; create shells PATH `mkcert` into launcher data dir, no silent `mkcert -install`/CA trust), HTTPS start passthrough of upstream cert flags, phone CA-install guide entry in card, mic/voice gate (`requires trusted LAN HTTPS + CA install` on plain HTTP) in `src/app.js`/`src/index.html`, `deepyWebCert()` wrapper in `src/w2gp.js`. Files: `src-tauri/src/deepy_web.rs`, `src/app.js`, `src/index.html`, `src/w2gp.js`. Acceptance: HTTPS serves trusted URL with valid cert; mic blocked on HTTP, allowed on trusted HTTPS; no silent CA changes. Test: `cargo test --manifest-path src-tauri/Cargo.toml`; frontend manual: bring-own, create, CA guide reachable, mic gate both states. <!-- sdd-owner: implementation -->
  - Implemented 2026-09-14: `deepy_web_cert` command (bring/create) + registration in `lib.rs`; `deepy_web_start` extended with `https_enabled/cert/key/port` (fail-closed validation, `--ssl-certfile/--ssl-keyfile/--https-port` passthrough, config fallback); card LAN-HTTPS section + CA guide + `deepyWebMicGate`; `refreshDeepyWeb()` mic gate + prefs; `deepyWebStartFlow` + `deepyWebCertFlow`. Gates: 32/32 `deepy_web` tests pass, clippy/check clean for `deepy_web.rs`, `cargo fmt` clean, `node --check` clean for `w2gp.js`+`app.js`.

## Slice 3 — Tailscale v1.3

### Tailscale RED — status-parse failing tests

- [x] Define Tailscale RED tests in `src-tauri/src/deepy_web.rs`: parse `tailscale status` host/tailnet into `https://<host>.<tailnet>.ts.net`; absent/not-logged-in yields `{present:false}`/`{loggedIn:false}` with no URL. Files: `src-tauri/src/deepy_web.rs`. Acceptance: new tailscale tests FAIL before implementation. Test: `cargo test --manifest-path src-tauri/Cargo.toml deepy_web_tailscale`. <!-- sdd-owner: implementation -->
  - Implemented 2026-09-14 as `parse_tailscale_json` over `tailscale status --json` (`BackendState`/`Self.HostName`/`MagicDNSSuffix`); RED confirmed (E0425, fn missing), then GREEN. 4 tests pass.

### Tailscale GREEN — detect + tailnet URL + docs-only router VPN + NAT guard

- [x] Implement Tailscale GREEN: `deepy_web_tailscale()` (binary probe → `tailscale status` short timeout → tailnet URL only on success), desktop-config `deepyTailscale`, card section with install/login link vs Copy/QR tailnet URL, router-VPN docs-only text (no automate buttons), public-NAT rule copy (auth + trusted HTTPS, HTTPS-only forward) wherever off-LAN discussed, `deepyWebTailscale()` wrapper in `src/w2gp.js`. Files: `src-tauri/src/deepy_web.rs`, `src/app.js`, `src/index.html`, `src/w2gp.js`. Acceptance: URL shown only on success; otherwise guidance with no URL; router VPN has no execute buttons. Test: `cargo test --manifest-path src-tauri/Cargo.toml`; frontend manual: absent/present Tailscale states. <!-- sdd-owner: implementation -->
  - Implemented 2026-09-14: `deepy_web_tailscale` command (probe → 8s-bounded `--json` status → URL or `{present,loggedIn}` + guidance) + `lib.rs` registration; card live-status block + Copy/QR + NAT rule + router-VPN docs-only details; `refreshDeepyWebTailscale()` render. `deepyTailscale` config key deemed unnecessary (live probe each refresh, no persistence). Gates: 36/36 `deepy_web` tests pass, clippy clean for `deepy_web.rs`, `cargo fmt` clean, `node --check` clean.

## Verification

## Follow-ups (pre-existing, out of slice scope)

- `src/w2gp.js` pi-lens flags on lines 205/298/450/470/485 (`window.open` fallbacks, `innerHTML` iframe embed) were pre-existing (byte-identical in HEAD) — fixed 2026-09-14 per user request with a shared `safeHttpUrl()` guard (http(s)-only, caller text passed through unchanged) + DOM-built iframe (same style/`allow` tokens). Verified: `node --check` clean, 11/11 validator cases pass. No behavior change for legit URLs.
- Vendored `src/qrcodegen.js` (Nayuki, MIT) intentionally keeps upstream `==` style — must stay byte-faithful to source; excluded from style gates.

## Verification

- [x] Run full quality gates and manual pass: `cargo test --manifest-path src-tauri/Cargo.toml`, `cargo clippy --manifest-path src-tauri/Cargo.toml`, `cargo check --manifest-path src-tauri/Cargo.toml`, `cargo fmt --check`, plus frontend manual (Same-PC start, LAN+QR, auth banner, HTTPS gate, Tailscale absent/present, stop scoping). Files: all touched files above. Acceptance: all gates green; manual checklist recorded in PR body. Test: commands listed above. <!-- sdd-owner: implementation -->
  - Automated gates 2026-09-14: `cargo test` 121/121 pass; `clippy` 0 errors, 12 warnings all pre-existing (plugins/electron/base/updates/system/amd/hw — none in `deepy_web.rs`); `cargo check` 0 errors; `cargo fmt --check` clean; `node --check` clean (`w2gp.js`, `app.js`).
    - Frontend manual (needs running launcher — PENDING): Same-PC start, LAN+QR, auth banner, HTTPS bring/create + mic gate both states, Tailscale absent/present, stop leaves main server running. Record results in PR body.

    ## Post-verification refinements (from live testing, same files)

    - Auth controls lock while running (radios/field/Generate/Show disabled + "apply at start only" hint).
    - Fixed password stays visible in the info panel after start (with Copy/QR); input field is cleared.
    - Auth modes reduced to Off + Fixed; passphrase generator fills the Fixed field; saved `generate` prefs migrate to `fixed`.
    - Mode radio + cert UI moved under Advanced (mode, port, HTTPS); added PC-side mkcert install guide; fixed stale "coming soon" HTTPS line.
    - `mkcert`-missing error now points at the in-card guide.
    - Remember-password (30 days, `deepySavedPassword`/`deepyPasswordExpiry` in desktop-config, reuse-when-empty, Forget button, expired-key cleanup). Plaintext on this PC — labeled in UI.
    - LAN-login 403 diagnosed (read-only) in upstream `C:/Wan2GP/shared/authentication/web.py`: per-process CSRF secret → reload the login page after any server restart. Upstream throttles (4 free, +30s, 10 min from 20th, reset on success) but never invalidates; launcher cannot force reset-after-N (attempts invisible upstream).

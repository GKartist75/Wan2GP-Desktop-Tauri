# Deepy Web Specification

## Purpose

One-button phone-friendly Deepy Web from the launcher dashboard: preflight, auto-configure, start/stop/status on a dedicated port, Same-PC and Phone-LAN URLs with QR, Auth v1.1, LAN HTTPS v1.2 (mic gate), and Tailscale v1.3 off-LAN. Upstream `wgp.py` owns `--deepy-server` / `--listen` / `--auth` flags; the launcher only composes them.

## Requirements

### Requirement: Dashboard card

The system MUST provide a `📱 Deepy Web` dashboard card adjacent to the Deepy Prime card showing status, Start / Stop / QR controls, Same-PC URL + Phone URL with Copy/QR actions, a Same-PC-only / Phone-LAN radio, and Advanced (Port, Auth, Sessions, Engine, Voice) controls.

#### Scenario: Card renders with status and controls

- GIVEN the launcher dashboard is open
- WHEN the Deepy Web card section renders
- THEN status text, Start, Stop, and QR buttons are visible alongside Same-PC and Phone URL fields with Copy/QR actions, the mode radio, and Advanced controls

#### Scenario: Card actions invoke backend commands

- GIVEN the Deepy Web card is visible
- WHEN the user clicks Start, Stop, or QR
- THEN the frontend invokes `deepy_web_start`, `deepy_web_stop`, or the QR flow respectively and refreshes status via `deepy_web_status`

### Requirement: Preflight check

The system MUST provide a `deepy_web_preflight` check that verifies install presence, torch availability, that `deepyPort` is free, and that surfaces a Gradio-clash warning when the main server and Deepy Web would run side-by-side.

#### Scenario: Preflight passes on healthy machine

- GIVEN Wan2GP is installed with torch and `deepyPort` is free
- WHEN `deepy_web_preflight` runs
- THEN it returns success with installed=true, torch=true, portFree=true

#### Scenario: Preflight reports blocked port

- GIVEN `deepyPort` is already bound
- WHEN `deepy_web_preflight` runs
- THEN it returns portFree=false with guidance to change the port or stop the occupying process

#### Scenario: Gradio clash warning

- GIVEN the main Gradio server is running on `serverPort`
- WHEN preflight runs for a Deepy Web start on `deepyPort`
- THEN it returns a non-blocking clash notice that two processes will run side-by-side (e.g. 7860 + 7861)

### Requirement: Port default and validation

The system MUST default `deepyPort` to `serverPort + 1` (e.g. 7861 when main is 7860), persist overrides in launcher desktop-config (NOT in `wgp_config.json`), validate range 1–65535, and reject `deepyPort == serverPort`.

#### Scenario: Default port derives from server port

- GIVEN `serverPort` is 7860 with no `deepyPort` override
- WHEN the Deepy Web port is resolved
- THEN it resolves to 7861

#### Scenario: Invalid port rejected

- GIVEN a user enters port 0, 99999, non-numeric text, or a value equal to `serverPort`
- WHEN validation runs
- THEN the value is rejected with an explanatory error and the start is blocked

### Requirement: Auto install and initiate

The system MUST, when Deepy is Disabled at Start time, auto-configure Zero + Qwen3.5 4B, fix enhancer values 1/2 to 3, require Prime-local 27B presence otherwise fall back, ensure a multisession dedicated sessions directory with file copy, and reuse `--config` + `--deepy-sessions-dir` passthrough using existing `deepy_set` semantics.

#### Scenario: Disabled state auto-configures Zero

- GIVEN `deepy_enabled` is 0
- WHEN the user starts Deepy Web
- THEN the system configures Zero + Qwen3.5 4B before launching, without requiring manual config edits

#### Scenario: Enhancer auto-fix

- GIVEN `enhancer_enabled` is 1 or 2
- WHEN auto-configure runs
- THEN it sets the enhancer to 3

#### Scenario: Prime-local without 27B falls back

- GIVEN Prime-local is selected but no 27B model is present
- WHEN auto-configure runs
- THEN it falls back to the Zero path instead of launching a broken Prime-local configuration

### Requirement: wgp_config backup and executable rule

The system MUST back up `wgp_config.json` to `wgp_config.json.deepy-bak` before every Deepy config write, and MUST keep `profiles.<x>.executable` as a literal exe name (never an absolute path).

#### Scenario: Backup written before config change

- GIVEN a Deepy config write is about to occur
- WHEN the write executes
- THEN `wgp_config.json.deepy-bak` contains the pre-write contents

### Requirement: Start Same-PC

The system MUST start Same-PC Deepy Web as `wgp.py --deepy-server --server-port <deepyPort>` (plus shared `--config` / `--deepy-sessions-dir` args) via the common launch-args builder without forking it.

#### Scenario: Same-PC launch args

- GIVEN mode is Same-PC-only with `deepyPort` 7861
- WHEN `deepy_web_start` runs
- THEN the spawned process args include `--deepy-server --server-port 7861` and MUST NOT include `--listen`

### Requirement: Start Phone-LAN

The system MUST start Phone-LAN Deepy Web by appending `--listen` to the Same-PC args, display a firewall hint on first `--listen` bind, and MUST NOT create silent firewall rules.

#### Scenario: LAN launch appends listen with firewall hint

- GIVEN mode is Phone-LAN
- WHEN `deepy_web_start` runs
- THEN args include `--deepy-server --server-port <deepyPort> --listen` and the UI shows a firewall hint (OS prompt + doc link, no unattended `netsh` rule)

### Requirement: Stop with port-scoped process kill

The system MUST stop only the Deepy Web process scoped to `deepyPort`, reusing the `stop_wangp` / `stop_all_servers` lifecycle pattern with a port-scoped kill filter, and MUST NOT kill the main Gradio server on `serverPort`.

#### Scenario: Stop kills only Deepy Web process

- GIVEN the main server runs on 7860 and Deepy Web runs on 7861
- WHEN `deepy_web_stop` runs for `deepyPort` 7861
- THEN the 7861 process terminates and the 7860 process keeps running

### Requirement: Status query

The system MUST provide `deepy_web_status` reporting running state, bound port, mode (same-pc / lan), and URLs in use.

#### Scenario: Status reflects running instance

- GIVEN Deepy Web is running on 7861 in LAN mode
- WHEN `deepy_web_status` is queried
- THEN it returns running=true, port=7861, mode=lan, with matching Same-PC and Phone URLs

#### Scenario: Status reflects stopped state

- GIVEN no Deepy Web process is bound to `deepyPort`
- WHEN `deepy_web_status` is queried
- THEN it returns running=false

### Requirement: URLs with never-0.0.0.0 rule

The system MUST show a Same-PC URL (`http://localhost:<deepyPort>` or `serverName`) and a Phone URL built from an enumerated non-loopback LAN IPv4 address (`http://<lan-ip>:<deepyPort>`), and MUST NEVER display or advertise `0.0.0.0`.

#### Scenario: Phone URL uses real LAN IP

- GIVEN the host LAN IP is 192.168.1.20 and `deepyPort` is 7861
- WHEN URLs render
- THEN the Phone URL is `http://192.168.1.20:7861` and no URL contains `0.0.0.0`

#### Scenario: No LAN adapter degrades gracefully

- GIVEN no non-loopback IPv4 is enumerable
- WHEN URLs render
- THEN the Phone URL shows an unavailable state with guidance instead of `0.0.0.0`

### Requirement: QR, copy, and home-screen hint

The system MUST provide Copy buttons for both URLs, a QR modal encoding the selected URL, and an Add-to-Home-Screen hint (iOS Share > Add, Android Install).

#### Scenario: QR encodes phone URL

- GIVEN Phone-LAN mode with Phone URL `http://192.168.1.20:7861`
- WHEN the user opens QR for the Phone URL
- THEN the QR modal encodes exactly that URL and the home-screen hint is visible

### Requirement: Standalone versus Gradio handoff warning

The system MUST show a finish → stop → resume handoff warning because the standalone Deepy Web process does not live-sync with the Gradio `/deepy/` view.

#### Scenario: Handoff warning visible

- GIVEN Deepy Web standalone is running
- WHEN the card renders
- THEN a warning states that finishing in standalone requires stop + resume to appear in Gradio `/deepy/`

### Requirement: Auth toggle with env-only password

The system MUST gate remote access behind an `--auth` toggle whose password is supplied ONLY via the `WANGP_AUTH_PASSWORD` environment variable (Generate-random vs Fixed), and MUST NEVER pass the password as a CLI arg, log it, or persist it in desktop-config / `wgp_config.json`.

#### Scenario: Auth start uses env password

- GIVEN Auth is enabled with a generated password
- WHEN `deepy_web_start` runs
- THEN the child process environment includes `WANGP_AUTH_PASSWORD`, args include `--auth`, and no arg or log line contains the password value

#### Scenario: Fixed password path

- GIVEN Auth is set to Fixed with a user-supplied secret
- WHEN the start runs
- THEN the same env-only transport applies and the secret is never written to config files

### Requirement: Auth expiry banner and rate-limit note

The system MUST show a 24h-expiry banner whenever Auth is enabled (restart invalidates auth) and surface a rate-limit note.

#### Scenario: Expiry banner on auth session

- GIVEN an authenticated Deepy Web session is running
- WHEN the card renders
- THEN a banner states credentials expire within 24h / on restart with a regenerate-or-restart action

### Requirement: Auth HTTP guard

The system MUST warn whenever Auth is combined with plain HTTP (especially LAN-HTTP), and MUST NEVER transmit or display the password over plain HTTP without that warning.

#### Scenario: LAN-HTTP with auth warns

- GIVEN Phone-LAN mode uses `http://<lan-ip>:<deepyPort>` with Auth on and HTTPS off
- WHEN the card/status renders
- THEN a blocking-style warning states the password would travel over unencrypted HTTP and recommends enabling LAN HTTPS or Same-PC-only

### Requirement: LAN HTTPS bring-own certificate

The system MUST support a bring-own certificate path accepting user-supplied `.pem` / `.key` files for the Deepy Web HTTPS start, validating presence and readability before launch.

#### Scenario: Bring-own cert starts HTTPS

- GIVEN valid user-supplied `.pem` and `.key` paths
- WHEN HTTPS start runs
- THEN Deepy Web serves a trusted-HTTPS URL using those files

#### Scenario: Missing cert blocks HTTPS start

- GIVEN a cert path is missing or unreadable
- WHEN HTTPS start runs
- THEN the start is blocked with a field-specific error

#### Scenario: Upstream flag and env transport

- GIVEN an HTTPS start with a valid cert/key pair
- WHEN the launcher spawns the Deepy Web process
- THEN it passes `--ssl-certfile <pem> --ssl-keyfile <key>` verbatim (`WANGP_SSL_CERT` / `WANGP_SSL_KEY` remain documented env fallbacks, command-line paths take precedence) and a missing/mismatched/unreadable pair stops startup fail-closed per upstream — never warn-and-continue

#### Scenario: Redirect port semantics

- GIVEN `--https-port <P>` is configured alongside the HTTP `deepyPort`
- WHEN Deepy Web serves HTTPS on `P`
- THEN the HTTP port only redirects to HTTPS and MUST NOT serve a second unencrypted application (per upstream DEEPY.md)

### Requirement: LAN HTTPS create certificate

The system MUST support a Create-LAN-cert path via mkcert that generates a LAN-usable certificate only via explicit user action, with no unattended CA install or silent trust changes.

#### Scenario: Explicit cert creation

- GIVEN the user clicks Create-LAN-cert
- WHEN generation completes
- THEN the new cert paths are stored as launcher-owned desktop-config keys and the UI offers the phone CA-install guide as the next step

### Requirement: Phone CA install guide

The system MUST provide a phone CA-install guide reachable from the card whenever LAN HTTPS is configured or required (especially for mic).

#### Scenario: Guide reachable from card

- GIVEN LAN HTTPS is enabled or mic is requested without HTTPS
- WHEN the user clicks the CA-guide entry point
- THEN step-by-step phone CA-install instructions are displayed

### Requirement: Microphone gate on trusted HTTPS

The system MUST gate microphone / voice features on trusted HTTPS and disable or block them with an explanatory message on plain HTTP.

#### Scenario: Mic blocked on HTTP

- GIVEN Deepy Web is served over plain HTTP
- WHEN the user attempts voice input
- THEN the action is blocked with a message requiring trusted LAN HTTPS + CA install

#### Scenario: Mic allowed on trusted HTTPS

- GIVEN Deepy Web is served over trusted LAN HTTPS
- WHEN the user attempts voice input
- THEN voice input is permitted

### Requirement: Tailscale detect, install guidance, tailnet URL

The system MUST detect Tailscale presence (binary probe / `tailscale status`), show an install link when absent or not logged in, and show the `https://<host>.<tailnet>.ts.net` tailnet URL only when `tailscale status` succeeds.

#### Scenario: Tailnet URL shown when Tailscale active

- GIVEN `tailscale status` succeeds with host and tailnet names
- WHEN the Tailscale section renders
- THEN it shows the `https://<host>.<tailnet>.ts.net` URL with Copy/QR actions

#### Scenario: Install guidance when Tailscale absent

- GIVEN the Tailscale binary is missing or not logged in
- WHEN the Tailscale section renders
- THEN it shows install / login guidance and MUST NOT show a tailnet URL

### Requirement: Router VPN docs-only

The system MUST treat router VPN as docs-only guidance (no automation, no port-forward automation, no UPnP/NAT-PMP), citing firmware variance and UPnP/CGNAT unreliability.

#### Scenario: Router VPN renders docs only

- GIVEN the user opens router-VPN help
- WHEN the content renders
- THEN it contains documentation text and offers no execute/automate buttons for router or NAT changes

### Requirement: Public NAT guard rule

The system MUST surface the public-NAT rule wherever off-LAN access is discussed: public exposure requires auth plus trusted HTTPS, forwarding HTTPS-only.

#### Scenario: Public NAT warning visible

- GIVEN Tailscale or any off-LAN path is shown
- WHEN the surrounding guidance renders
- THEN it states public NAT requires auth + trusted HTTPS with HTTPS-only forwarding

### Requirement: No silent security-sensitive changes

The system MUST NOT create firewall rules, install CAs, or trust certificates silently; every such change requires explicit user action plus guide text.

#### Scenario: First listen bind prompts rather than silences

- GIVEN the first `--listen` bind triggers the OS firewall prompt
- WHEN the flow completes
- THEN the launcher has created no unattended firewall rule and the hint + doc link were shown

# Deepy Web — One-Button Design (saved 2026-09-13)

## Goal

One click: install/initiate/start phone-friendly Deepy Web. No CLI flags for user.

## UX — Dashboard card `📱 Deepy Web`

- Status, [Start Deepy Web] [Stop] [QR], Same-PC URL + Phone URL [Copy][QR]
- Radio: Same-PC only / Phone-LAN. Advanced: Port, Auth, Sessions, Engine, Voice.
- QR modal + Add to Home Screen (iOS Share>Add, Android Install).

## Flow

1. Preflight: installed+torch, port free (default deepyPort=serverPort+1), Gradio clash warning.
2. Install/Initiate: if Disabled → auto Zero+Qwen3.5 4B + backup; fix enhancer 1/2→3; Prime-local needs 27B else fallback; ensure sessions=multisession dedicated + copy files; reuse --config + --deepy-sessions-dir.
3. Start: Same-PC `wgp.py --deepy-server --server-port N`; LAN `+ --listen`. Show LAN IP (never 0.0.0.0), firewall hint.

## Remote (deepbeepmeep feedback)

- v1.1 Password: --auth toggle, Generate vs Fixed via WANGP_AUTH_PASSWORD, 24h expiry banner, rate-limit note.
- v1.2 LAN HTTPS: Bring .pem/.key + Create LAN cert (mkcert) + phone CA install guide. Mic needs trusted HTTPS.
- v1.3 Tailscale over router VPN: detect/install tailscale, show tailnet URL. Router VPN docs-only (firmwares differ, UPnP/CGNAT unreliable).
- Never expose password over HTTP; public NAT = auth + trusted HTTPS, forward HTTPS-only.

## Notes

- Gradio+Web /deepy/ syncs live; standalone is separate process → finish→stop→resume handoff warning.
- Edge: restart invalidates auth; two processes side-by-side on 7860+7861.

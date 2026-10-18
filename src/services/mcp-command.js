/**
 * mcp-command.js — pure builder for the Agent API (MCP server) launch command.
 *
 * Mirrors upstream `shared/cli_args.py` + `shared/authentication/__init__.py`:
 *   --mcp --mcp-transport --mcp-host --mcp-port --mcp-api-version
 *   --mcp-webui/--no-mcp-webui --mcp-auth --mcp-auth-url --mcp-auth-password
 *
 * `--mcp-webui` is a web-UI autostart override (upstream help: "Override
 * Configuration > Agents MCP autostart for this launch"), not an MCP-only
 * flag — so selecting it switches the command from a standalone `--mcp`
 * server to a `--listen --mcp-webui` web launch.
 *
 * No DOM, no Tauri. Loaded as a plain <script> in the renderer and require()d
 * from node --test (src/services/package.json scopes this dir to CommonJS).
 * All top-level names are mcp-prefixed so they cannot collide with a sibling
 * service's globals (e.g. launch-args.js already defines `quoteVal`).
 */

'use strict'

/** API versions upstream accepts (`--mcp-api-version` choices); 2.1 is the default. */
const MCP_API_VERSIONS = ['2.1', '2', '1']

/** Transports upstream accepts (`--mcp-transport`). */
const MCP_TRANSPORTS = ['stdio', 'streamable-http', 'sse']

/** Web-UI autostart overrides (`--mcp-webui` is a tri-state BooleanOptionalAction). */
const MCP_WEBUI_MODES = ['', '--mcp-webui', '--no-mcp-webui']

/** Hosts that never leave the machine — no OAuth or TLS required. */
function mcpIsLoopback(host) {
  const h = String(host || '').trim().toLowerCase().replace(/^\[|\]$/g, '')
  return h === 'localhost' || h === '127.0.0.1' || h === '::1'
}

/** A host that binds every interface (not a connectable address). */
function mcpIsWildcard(host) {
  return /^(0\.0\.0\.0|::)$/.test(String(host || '').trim())
}

/** Quote a value only when it contains whitespace; strip inner quotes. */
function mcpQuote(v) {
  v = String(v)
  return /\s/.test(v) ? '"' + v.replace(/"/g, '') + '"' : v
}

/** Clamp every field into a known-good shape. */
function mcpNormalize(opts) {
  opts = opts || {}
  const transport = MCP_TRANSPORTS.indexOf(String(opts.transport)) >= 0 ? String(opts.transport) : 'stdio'
  const webui = MCP_WEBUI_MODES.indexOf(String(opts.webui == null ? '' : opts.webui)) >= 0 ? String(opts.webui || '') : ''
  return {
    transport: transport,
    webui: webui,
    host: String(opts.host || '').trim() || '127.0.0.1',
    port: String(opts.port == null ? '' : opts.port).trim() || '7866',
    api: String(opts.api || '2.1'),
    auth: opts.auth === true,
    authUrl: String(opts.authUrl || '').trim(),
    authPassword: String(opts.authPassword || '').trim()
  }
}

/** The OAuth flags, shared by the MCP-only and web-UI modes. */
function mcpAuthToks(c) {
  if (!c.auth) return []
  const toks = ['--mcp-auth']
  if (c.authUrl) toks.push('--mcp-auth-url', c.authUrl)
  if (c.authPassword) toks.push('--mcp-auth-password', mcpQuote(c.authPassword))
  return toks
}

/**
 * Build the copy-paste launch command.
 *
 * @param {object} opts
 * @param {string} [opts.transport]  'stdio' | 'streamable-http' | 'sse' (MCP-only mode)
 * @param {string} [opts.host]       bind host for non-stdio transports
 * @param {string|number} [opts.port]
 * @param {string} [opts.api]        '2.1' | '2' | '1'
 * @param {string} [opts.webui]      '' (standalone --mcp) | '--mcp-webui' | '--no-mcp-webui'
 * @param {boolean} [opts.auth]      add --mcp-auth (OAuth)
 * @param {string} [opts.authUrl]    public origin for --mcp-auth-url
 * @param {string} [opts.authPassword] fixed passphrase (blank = upstream generates one)
 * @returns {string}
 */
function mcpCommand(opts) {
  const c = mcpNormalize(opts)

  // Web-UI modes: the flag controls the normal web launch, so no --mcp/transport.
  if (c.webui === '--no-mcp-webui') return ['python', 'wgp.py', '--listen', '--no-mcp-webui'].join(' ')
  if (c.webui === '--mcp-webui') {
    return ['python', 'wgp.py', '--listen', '--mcp-webui']
      .concat(['--mcp-host', c.host, '--mcp-port', c.port])
      .concat(['--mcp-api-version', c.api])
      .concat(mcpAuthToks(c))
      .join(' ')
  }

  const toks = ['python', 'wgp.py', '--mcp']
  if (c.transport !== 'stdio') toks.push('--mcp-transport', c.transport, '--mcp-host', c.host, '--mcp-port', c.port)
  if (c.api) toks.push('--mcp-api-version', c.api)
  return toks.concat(mcpAuthToks(c)).join(' ')
}

/**
 * Human hint shown under the command: where clients connect and what is still
 * missing for a safe network exposure.
 *
 * @param {object} opts see mcpCommand
 * @returns {string}
 */
function mcpHint(opts) {
  const c = mcpNormalize(opts)

  if (c.webui === '--no-mcp-webui') return 'Web UI only — MCP autostart suppressed for this launch.'
  if (c.webui === '--mcp-webui') {
    const base = 'Web UI with MCP autostart — --listen exposes both on the LAN; protect the web UI with --auth + --ssl-certfile and MCP with OAuth.'
    return c.auth ? base : base + ' MCP OAuth is off.'
  }

  if (c.transport === 'stdio') return 'Local clients spawn this command — no URL, no OAuth.'

  const path = c.transport === 'sse' ? '/sse' : '/mcp'
  // A wildcard bind isn't connectable — prefer the public origin for the URL.
  let connectHost = c.host
  if (mcpIsWildcard(c.host)) connectHost = c.authUrl ? null : 'localhost'
  const scheme = /^https:\/\//i.test(c.authUrl) ? 'https' : 'http'
  const url = connectHost === null
    ? c.authUrl.replace(/\/+$/, '') + path
    : scheme + '://' + connectHost + ':' + c.port + path

  if (mcpIsLoopback(c.host) && !c.auth) return 'Connect clients to ' + url + ' — loopback only; add OAuth + HTTPS to expose it.'
  if (mcpIsLoopback(c.host) && c.auth) return 'Connect clients to ' + url + ' — OAuth on; approve the client on the Authorize MCP Access page.'
  if (!c.auth) return 'WARNING: ' + url + ' is reachable off this PC without --mcp-auth. Keep unauthenticated MCP on localhost or a trusted VPN.'
  if (!c.authUrl) return 'Connect clients to ' + url + ' — non-loopback OAuth needs --mcp-auth-url set to the public HTTPS origin.'
  return 'Connect OAuth clients to ' + url + ' — the certificate must cover that hostname and be trusted by the client.'
}

if (typeof module !== 'undefined' && module.exports) {
  module.exports = {
    mcpCommand,
    mcpHint,
    MCP_API_VERSIONS,
    MCP_TRANSPORTS,
    MCP_WEBUI_MODES,
    mcpIsLoopback,
    mcpIsWildcard
  }
}

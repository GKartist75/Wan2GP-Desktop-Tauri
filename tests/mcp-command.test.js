/**
 * mcp-command.test.js — the Agent API panel builds a correct wgp.py command.
 *
 * Parity with upstream `shared/cli_args.py` + `shared/authentication/__init__.py`:
 * API v2.1 is the default, `--mcp-webui` is a web-UI autostart override (so it
 * switches to a `--listen` launch instead of a standalone `--mcp` server), and
 * the OAuth flags exist (`--mcp-auth`, `--mcp-auth-url`, `--mcp-auth-password`).
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')

const { mcpCommand, mcpHint, MCP_API_VERSIONS, MCP_WEBUI_MODES } = require('../src/services/mcp-command.js')

const SRC = path.join(__dirname, '..', 'src')
const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
const builder = fs.readFileSync(path.join(SRC, 'launch-builder.js'), 'utf8')

test('2.1 is the default API version and the offered choices', () => {
  assert.deepEqual(MCP_API_VERSIONS, ['2.1', '2', '1'])
  assert.equal(mcpCommand({}), 'python wgp.py --mcp --mcp-api-version 2.1')
  assert.equal(mcpCommand({ api: '2' }), 'python wgp.py --mcp --mcp-api-version 2')
  assert.equal(mcpCommand({ api: '1' }), 'python wgp.py --mcp --mcp-api-version 1')
})

test('stdio (default) carries no host/port/transport', () => {
  const cmd = mcpCommand({ transport: 'stdio', host: '127.0.0.1', port: '7866' })
  assert.equal(cmd, 'python wgp.py --mcp --mcp-api-version 2.1')
  assert.ok(!cmd.includes('--mcp-host'))
  assert.ok(!cmd.includes('--mcp-transport'))
})

test('http transports add transport + host + port', () => {
  assert.equal(
    mcpCommand({ transport: 'streamable-http', host: '0.0.0.0', port: '7866' }),
    'python wgp.py --mcp --mcp-transport streamable-http --mcp-host 0.0.0.0 --mcp-port 7866 --mcp-api-version 2.1',
  )
  assert.ok(mcpCommand({ transport: 'sse' }).includes('--mcp-transport sse'))
})

test('an unknown transport or webui value falls back to the safe default', () => {
  assert.ok(mcpCommand({ transport: 'carrier-pigeon' }).startsWith('python wgp.py --mcp --mcp-api-version'))
  assert.ok(!mcpCommand({ webui: '--bogus' }).includes('--bogus'))
})

test('--mcp-webui switches to a web-UI launch (no --mcp, no transport)', () => {
  const cmd = mcpCommand({ webui: '--mcp-webui', host: '127.0.0.1', port: '7866' })
  assert.equal(
    cmd,
    'python wgp.py --listen --mcp-webui --mcp-host 127.0.0.1 --mcp-port 7866 --mcp-api-version 2.1',
  )
  assert.ok(!/(^| )--mcp( |$)/.test(cmd), 'must not include a bare --mcp in web-UI mode')
  assert.ok(!cmd.includes('--mcp-transport'))
})

test('--no-mcp-webui is a plain web launch', () => {
  assert.equal(mcpCommand({ webui: '--no-mcp-webui' }), 'python wgp.py --listen --no-mcp-webui')
})

test('OAuth flags are emitted only when enabled', () => {
  assert.ok(!mcpCommand({ transport: 'streamable-http' }).includes('--mcp-auth'))
  assert.equal(
    mcpCommand({ transport: 'streamable-http', auth: true, authUrl: 'https://wangp.example.com:7866' }),
    'python wgp.py --mcp --mcp-transport streamable-http --mcp-host 127.0.0.1 --mcp-port 7866 --mcp-api-version 2.1 --mcp-auth --mcp-auth-url https://wangp.example.com:7866',
  )
  // Blank passphrase = upstream generates/prints one at startup.
  assert.ok(!mcpCommand({ auth: true }).includes('--mcp-auth-password'))
})

test('a passphrase with spaces is quoted', () => {
  const cmd = mcpCommand({ auth: true, authPassword: 'my long passphrase' })
  assert.ok(cmd.endsWith('--mcp-auth --mcp-auth-password "my long passphrase"'), cmd)
})

test('the hint names the connect URL and warns about unsafe exposure', () => {
  assert.match(mcpHint({ transport: 'stdio' }), /no URL, no OAuth/)
  assert.match(mcpHint({ transport: 'streamable-http' }), /http:\/\/127\.0\.0\.1:7866\/mcp/)
  assert.match(mcpHint({ transport: 'sse' }), /\/sse/)
  assert.match(mcpHint({ transport: 'streamable-http', host: '0.0.0.0' }), /WARNING/)
  assert.match(
    mcpHint({ transport: 'streamable-http', host: '0.0.0.0', auth: true, authUrl: 'https://w.example.com:7866' }),
    /https:\/\/w\.example\.com:7866\/mcp/,
  )
  assert.match(
    mcpHint({ transport: 'streamable-http', host: '0.0.0.0', auth: true }),
    /--mcp-auth-url/,
  )
  assert.match(mcpHint({ webui: '--mcp-webui' }), /web UI with MCP autostart/i)
})

test('the panel HTML exposes the new controls and the 2.1 option', () => {
  for (const id of ['mcpWebui', 'mcpAuth', 'mcpAuthUrl', 'mcpAuthPassword', 'mcpApi']) {
    assert.ok(html.includes(`id="${id}"`), `index.html is missing #${id}`)
  }
  assert.match(html, /<option value="2\.1">/)
  assert.ok(html.includes('--mcp-webui') && html.includes('--no-mcp-webui'))
  // The service must load before the panel that calls it.
  const order = [...html.matchAll(/<script[^>]*src="([^"]+)"/g)].map((m) => m[1])
  assert.ok(
    order.indexOf('services/mcp-command.js') < order.indexOf('launch-builder.js'),
    'mcp-command.js must load before launch-builder.js',
  )
})

test('launch-builder delegates to the service and wires every control', () => {
  assert.ok(builder.includes('typeof mcpCommand'), 'launch-builder must call the service')
  for (const id of ['mcpTransport', 'mcpHost', 'mcpPort', 'mcpApi', 'mcpWebui', 'mcpAuth', 'mcpAuthUrl', 'mcpAuthPassword']) {
    assert.ok(builder.includes(`'${id}'`), `launch-builder does not wire #${id}`)
  }
  // The old in-file builder (API v2 default) must be gone, not shadowed.
  assert.ok(!/function mcpCommand\s*\(/.test(builder), 'stale in-file mcpCommand left in launch-builder')
})

test('MCP_WEBUI_MODES covers the tri-state flag', () => {
  assert.deepEqual(MCP_WEBUI_MODES, ['', '--mcp-webui', '--no-mcp-webui'])
})

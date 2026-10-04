// harness/serve.js — serves src/ over localhost with the Tauri IPC stubbed in.
// Test-only: lets a plain browser boot the real renderer so the UI can be
// driven and inspected. Never shipped; lives under tests/.
//
//   node tests/harness/serve.js [port]
//
// Injects tests/harness/tauri-stub.js as the first script of index.html, so
// window.__TAURI__ exists before w2gp.js runs. Everything else is served
// byte-for-byte from disk.
'use strict'

const http = require('node:http')
const fs = require('node:fs')
const path = require('node:path')

const ROOT = path.join(__dirname, '..', '..', 'src')
const STUB = fs.readFileSync(path.join(__dirname, 'tauri-stub.js'), 'utf8')
const PORT = Number(process.argv[2] || 4173)

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.svg': 'image/svg+xml',
  '.ico': 'image/x-icon',
}

const server = http.createServer((req, res) => {
  const url = decodeURIComponent((req.url || '/').split('?')[0])

  // The stub is injected by path, not inlined, so its own syntax errors are
  // visible in the console instead of being swallowed into the page.
  if (url === '/__harness/tauri-stub.js') {
    res.writeHead(200, {
      'content-type': TYPES['.js'],
      'cache-control': 'no-store',
    })
    res.end(STUB)
    return
  }

  const rel = url === '/' ? 'index.html' : url.replace(/^\/+/, '')
  const file = path.join(ROOT, rel)

  // Stay inside src/ — the harness serves the renderer, nothing else.
  if (!file.startsWith(ROOT)) {
    res.writeHead(403).end('forbidden')
    return
  }
  if (!fs.existsSync(file) || fs.statSync(file).isDirectory()) {
    res.writeHead(404).end('not found: ' + rel)
    return
  }
  let body = fs.readFileSync(file)
  const type = TYPES[path.extname(file).toLowerCase()] || 'application/octet-stream'
  if (rel === 'index.html') {
    // The stub must be first: w2gp.js captures window.__TAURI__.core at load.
    body = Buffer.from(
      body.toString('utf8').replace(/<head(\s*>)/i, '<head$1\n<script src="/__harness/tauri-stub.js"></script>')
    )
  }
  res.writeHead(200, { 'content-type': type, 'cache-control': 'no-store' })
  res.end(body)
})

server.listen(PORT, '127.0.0.1', () => {
  console.log('harness serving ' + ROOT + ' on http://127.0.0.1:' + PORT)
})

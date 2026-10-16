/**
 * triton-compiler.test.js — the Triton compiler choice is a real setting.
 *
 * Settings → Desktop → "Triton compiler" (auto | msvc | tinycc), persisted
 * as tritonCompiler in desktop-config.json, honoured on the next launch.
 * Structural guards: the select exists with all three options, the bridge
 * maps the command, and the tab wires change + open-reflect (default auto).
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')

const SRC = path.join(__dirname, '..', 'src')
const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
const bridge = fs.readFileSync(path.join(SRC, 'w2gp.js'), 'utf8')
const tab = fs.readFileSync(path.join(SRC, 'term-tab.js'), 'utf8')

test('the Desktop section offers all three compiler choices', () => {
  assert.match(html, /id="tritonCompilerSelect"/)
  for (const v of ['auto', 'msvc', 'tinycc']) {
    assert.match(html, new RegExp(`<option value="${v}">`), `option ${v} missing`)
  }
  assert.match(html, /Applies on next WanGP launch/, 'must say when it takes effect')
})

test('the w2gp bridge maps the setter', () => {
  assert.match(bridge, /setTritonCompiler:\s*\(m\)\s*=>\s*call\("set_triton_compiler", \{ mode: m \}\)/)
})

test('the tab persists the pick and reflects it on open', () => {
  assert.match(tab, /\$\("tritonCompilerSelect"\)\?\.addEventListener\("change"/)
  assert.match(tab, /setTritonCompiler\(el\.value\)/)
  assert.match(tab, /applies on next WanGP launch/i, 'the toast must not promise live apply')
  assert.match(tab, /cfg\.tritonCompiler \|\| "tinycc"/, 'unset means pure upstream')
})

/**
 * backend-chips.test.js — INT8/FP8 live chips sit beside Running, gated by values.
 *
 * After the move out of Active Environment (rows stuck on "—" read as a
 * forgotten setting): _paintBackendChips targets the topbar pills and the
 * container shows only while a backend is known — no server, no pills.
 * Behaviour runs in a vm (theme-persist.test.js style); placement is pinned
 * structurally.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')

const SRC = path.join(__dirname, '..', 'src')
const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
const tab = fs.readFileSync(path.join(SRC, 'resetui-tab.js'), 'utf8')

function extractFn(name) {
  const m = tab.match(
    new RegExp(`(?:async\\s+)?function\\s+${name}\\s*\\([^)]*\\)\\s*\\{[\\s\\S]*?\\n\\}`),
  )
  assert.ok(m, `${name} not found`)
  return m[0]
}

function harness() {
  const els = new Map()
  const el = (id) => {
    if (!els.has(id)) els.set(id, { textContent: '', title: '', style: {} })
    return els.get(id)
  }
  const ctx = { $: (id) => el(id), __els: els, _int8Backend: undefined, _fp8Backend: undefined }
  ctx.window = ctx
  vm.createContext(ctx)
  vm.runInContext(extractFn('_paintBackendChips') + '\nthis.paint = _paintBackendChips;', ctx, {
    timeout: 5000,
  })
  return ctx
}

test('known backends paint the pills and reveal the container', () => {
  const h = harness()
  h.window._int8Backend = 'WGPKernels'
  h.window._fp8Backend = 'Triton'
  h.paint()
  assert.equal(h.__els.get('ledInt8').textContent, 'INT8 · WGPKernels')
  assert.equal(h.__els.get('ledFp8').textContent, 'FP8 · Triton')
  assert.equal(h.__els.get('backendLeds').style.display, 'inline-flex')
})

test('no known backend hides the container (never a dangling dash)', () => {
  const h = harness()
  h.paint()
  assert.equal(h.__els.get('backendLeds').style.display, 'none')
  // One-sided knowledge still shows (the other pill waits, not dashes).
  h.window._int8Backend = 'WGPKernels'
  h.paint()
  assert.equal(h.__els.get('backendLeds').style.display, 'inline-flex')
  assert.equal(h.__els.get('ledFp8').textContent, 'FP8 · …')
})

test('chips live beside Running, not in Active Environment', () => {
  for (const id of ['backendLeds', 'ledInt8', 'ledFp8']) {
    assert.match(html, new RegExp(`id="${id}"`), `${id} missing from index.html`)
  }
  assert.doesNotMatch(html, /specInt8Backend/, 'env rows removed — one home for live state')
  assert.doesNotMatch(html, /specFp8Backend/, 'env rows removed — one home for live state')
  assert.match(tab, /\$\("backendLeds"\)/, 'paint must toggle the container')
  assert.match(tab, /onLaunchLog/, 'the launch-log subscription still feeds the chips')
})

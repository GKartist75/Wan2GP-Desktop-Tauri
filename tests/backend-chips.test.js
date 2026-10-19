/**
 * backend-chips.test.js — INT8/FP8 live chips sit beside Running, gated by values.
 *
 * After the move out of Active Environment (rows stuck on "—" read as a
 * forgotten setting): _paintBackendChips targets the topbar pills and the
 * container shows only while a backend is known — no server, no pills.
 * Each pill carries a status dot like the Running LED: green for an
 * accelerated backend, amber for the PyTorch fallback, grey before the log
 * reports. Behaviour runs in a vm (theme-persist.test.js style); placement
 * is pinned structurally.
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
    if (!els.has(id)) els.set(id, { textContent: '', title: '', style: {}, className: '' })
    return els.get(id)
  }
  const ctx = { $: (id) => el(id), __els: els, _int8Backend: undefined, _fp8Backend: undefined }
  ctx.window = ctx
  vm.createContext(ctx)
  vm.runInContext(
    extractFn('_backendChipDot') +
      '\n' +
      extractFn('_paintBackendChips') +
      '\nthis.paint = _paintBackendChips;',
    ctx,
    { timeout: 5000 },
  )
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
  // Accelerated backends get the Running LED's green dot.
  assert.equal(h.__els.get('ledInt8Dot').className, 'led-dot led-running')
  assert.equal(h.__els.get('ledFp8Dot').className, 'led-dot led-running')
})

test('PyTorch fallback is amber, not green', () => {
  const h = harness()
  h.window._int8Backend = 'PyTorch'
  h.window._fp8Backend = 'Triton, PyTorch for scaled FP8 checkpoints'
  h.paint()
  // Still a real report → pill shown, but one path fell back.
  assert.equal(h.__els.get('backendLeds').style.display, 'inline-flex')
  assert.equal(h.__els.get('ledInt8Dot').className, 'led-dot led-warn')
  assert.equal(h.__els.get('ledFp8Dot').className, 'led-dot led-warn')
})

test('no known backend hides the container (never a dangling dash)', () => {
  const h = harness()
  h.paint()
  assert.equal(h.__els.get('backendLeds').style.display, 'none')
  assert.equal(h.__els.get('ledInt8Dot').className, 'led-dot led-idle')
  // One-sided knowledge still shows (the other pill waits, not dashes).
  h.window._int8Backend = 'WGPKernels'
  h.paint()
  assert.equal(h.__els.get('backendLeds').style.display, 'inline-flex')
  assert.equal(h.__els.get('ledFp8').textContent, 'FP8 · …')
  assert.equal(h.__els.get('ledFp8Dot').className, 'led-dot led-idle')
})

test('chips live beside Running, not in Active Environment', () => {
  for (const id of ['backendLeds', 'ledInt8', 'ledFp8']) {
    assert.match(html, new RegExp(`id="${id}"`), `${id} missing from index.html`)
  }
  // The dot is the chip's first child; the text span keeps the id so the
  // paint path and the assertions above read the same element.
  for (const kind of ['Int8', 'Fp8']) {
    assert.match(
      html,
      new RegExp(
        `<span class="backend-chip"><span class="led-dot" id="led${kind}Dot"></span><span id="led${kind}">`,
      ),
      `led${kind} dot must precede the text span`,
    )
  }
  assert.doesNotMatch(html, /specInt8Backend/, 'env rows removed — one home for live state')
  assert.doesNotMatch(html, /specFp8Backend/, 'env rows removed — one home for live state')
  assert.match(tab, /\$\("backendLeds"\)/, 'paint must toggle the container')
  assert.match(tab, /onLaunchLog/, 'the launch-log subscription still feeds the chips')
})

test('the four status items sit in one 2×2 cluster, not an inline row', () => {
  // Inline, the cluster was Running · INT8 · FP8 · Deepy Web wide, and pushed
  // the topbar controls off the bar. It is a grid now: Running / Deepy Web on
  // the first row, the chip pair spanning the second.
  const grid = html.match(
    /<span id="statusGrid"[^>]*>([\s\S]*?)\r?\n\s*<\/span>\r?\n\s*<\/div>/,
  )
  assert.ok(grid, 'statusGrid wrapper missing from index.html')
  const inner = grid[1]
  assert.match(inner, /id="runningLed"/)
  assert.match(inner, /id="deepyWebLed"/)
  assert.match(inner, /id="backendLeds"/)
  // Order inside the grid: the two process LEDs first, then the chip pair.
  assert.ok(
    inner.indexOf('id="runningLed"') < inner.indexOf('id="deepyWebLed"'),
    'Running precedes Deepy Web in the first row',
  )
  assert.ok(
    inner.indexOf('id="deepyWebLed"') < inner.indexOf('id="backendLeds"'),
    'the chip pair is the second row',
  )
  // The cluster must not be a bare inline run of four spans any more.
  assert.doesNotMatch(
    html,
    /id="backendLeds"[\s\S]{0,80}id="deepyWebLed"/,
    'chips must not sit between the two process LEDs',
  )
})

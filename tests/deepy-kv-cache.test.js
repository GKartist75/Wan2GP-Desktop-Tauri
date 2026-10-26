/**
 * deepy-kv-cache.test.js — the Deepy KV cache control carries upstream's full
 * value set and round-trips a saved config.
 *
 * Upstream `deepy_kv_cache_quantization` (shared/deepy/config.py) is "" for
 * BF16, auto, int8, or one of six body presets; the four mixed ones live in the
 * config as "body:exact" with the exact overlay defaulting to s1r1024
 * (shared/llm_engines/lmgp/layers/kv_formats.py BODY_PRESETS / EXACT_PRESETS).
 * The panel's dropdowns must offer exactly that set — a missing option means a
 * config written by upstream can no longer be shown or changed here — and
 * splitting a saved value must not invent an overlay for BF16/INT8.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')

const SRC = path.join(__dirname, '..', 'src')
const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
const tab = fs.readFileSync(path.join(SRC, 'deepy-tab.js'), 'utf8')

// Upstream's lists, verbatim (kv_formats.py: BODY_PRESETS / EXACT_PRESETS).
const BODIES = ['int8', 'q6', 'int8_q4', 'q6_q4', 'q4_1', 'q4']
const MIXED = ['int8_q4', 'q6', 'q6_q4', 'q4_1', 'q4']
const EXACTS = ['s1r1024', 's1r256', 's0r1024', 's0r0']

function options(id) {
  const block = html.match(new RegExp(`<select id="${id}"[\\s\\S]*?</select>`))
  assert.ok(block, `#${id} exists`)
  return [...block[0].matchAll(/<option value="([^"]*)"/g)].map((m) => m[1])
}

function extractFn(name) {
  const m = tab.match(new RegExp(`function\\s+${name}\\s*\\([^)]*\\)\\s*\\{[\\s\\S]*?\\n\\}`))
  assert.ok(m, `${name} is a top-level function`)
  return m[0]
}

const ctx = vm.createContext({})
vm.runInContext(
  [
    tab.match(/const KV_MIXED = \[[^\]]*\];/)[0],
    extractFn('kvSplit'),
    extractFn('kvJoin'),
  ].join('\n'),
  ctx,
)

test('the body dropdown offers upstream values: BF16, auto, INT8, six presets', () => {
  assert.deepStrictEqual(options('deepyKvCacheSelect').sort(), ['', 'auto', ...BODIES].sort())
})

test('the overlay dropdown offers exactly upstream’s four exact presets', () => {
  assert.deepStrictEqual(options('deepyKvCacheExactSelect').sort(), EXACTS.slice().sort())
})

test('a mixed body always writes an overlay, defaulting to s1r1024', () => {
  for (const body of MIXED) {
    assert.strictEqual(ctx.kvJoin(body, ''), `${body}:s1r1024`)
    assert.strictEqual(ctx.kvJoin(body, 's0r0'), `${body}:s0r0`)
  }
})

test('BF16, auto and INT8 write no overlay', () => {
  for (const body of ['', 'auto', 'int8']) {
    assert.strictEqual(ctx.kvJoin(body, 's0r0'), body)
  }
})

test('a saved value splits into its dropdowns without inventing an overlay', () => {
  assert.deepStrictEqual(
    { ...ctx.kvSplit('int8_q4:s1r1024') },
    { body: 'int8_q4', exact: 's1r1024' },
  )
  assert.deepStrictEqual({ ...ctx.kvSplit('q4:s0r0') }, { body: 'q4', exact: 's0r0' })
  // Bare bodies and the non-mixed values fall back to the upstream default
  // overlay, which is what a later write would join anyway.
  assert.deepStrictEqual({ ...ctx.kvSplit('q6') }, { body: 'q6', exact: 's1r1024' })
  assert.deepStrictEqual({ ...ctx.kvSplit('int8') }, { body: 'int8', exact: 's1r1024' })
  // BF16 is the empty string, and it must survive as BF16 — not "auto".
  assert.deepStrictEqual({ ...ctx.kvSplit('') }, { body: '', exact: 's1r1024' })
  assert.deepStrictEqual({ ...ctx.kvSplit(null) }, { body: 'auto', exact: 's1r1024' })
  // Upstream writes lower case; case is normalised rather than dropped.
  assert.strictEqual(ctx.kvSplit('Q4_1:S1R256').body, 'q4_1')
  assert.strictEqual(ctx.kvSplit('Q4_1:S1R256').exact, 's1r256')
})

test('every body option round-trips through join → split', () => {
  for (const body of options('deepyKvCacheSelect')) {
    for (const exact of EXACTS) {
      const written = ctx.kvJoin(body, exact)
      assert.strictEqual(ctx.kvSplit(written).body, body)
    }
  }
})

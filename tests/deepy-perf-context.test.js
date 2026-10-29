/**
 * deepy-perf-context.test.js — the Deepy performance/context controls mirror
 * upstream's Configuration → Deepy values and keep context/compaction a valid
 * pair.
 *
 * Upstream shared/deepy/config.py: `deepy_context_tokens` clamps to
 * [20480, 256000]; Summarize needs >= 32000 and Summarize with Thinking
 * >= 48000; `deepy_compaction_type` accepts discard/summarize and the legacy
 * `summarize_thinking`; `deepy_vram_mode` accepts unload/unload_on_request/
 * always_loaded. The panel must offer exactly those values and raise the
 * context when the user picks a compaction mode that needs more.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')

const SRC = path.join(__dirname, '..', 'src')
const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
const tab = fs.readFileSync(path.join(SRC, 'deepy-tab.js'), 'utf8')

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
    tab.match(/const DEEPY_CONTEXT_MIN = \d+;/)[0],
    tab.match(/const DEEPY_CONTEXT_MAX = \d+;/)[0],
    tab.match(/const DEEPY_COMPACTION_MIN = \{[\s\S]*?\};/)[0],
    extractFn('deepyCompactionChoice'),
    extractFn('deepyClampContext'),
  ].join('\n'),
  ctx,
)

test('VRAM mode offers exactly upstream’s three values', () => {
  assert.deepStrictEqual(options('deepyVramModeSelect').sort(), [
    'always_loaded',
    'unload',
    'unload_on_request',
  ])
})

test('compaction offers discard, summarize and summarize-with-thinking', () => {
  assert.deepStrictEqual(options('deepyCompactionSelect'), [
    'discard',
    'summarize',
    'thinking',
  ])
})

test('Prime model preferences offer upstream’s speed and size choices', () => {
  assert.deepStrictEqual(options('deepyModelSpeedSelect'), ['fast', 'standard', 'any'])
  assert.deepStrictEqual(options('deepyModelSizeSelect'), ['smaller', 'larger', 'any'])
})

test('the legacy summarize_thinking type maps to Summarize with Thinking', () => {
  assert.strictEqual(ctx.deepyCompactionChoice('summarize_thinking', false), 'thinking')
  assert.strictEqual(ctx.deepyCompactionChoice('summarize', true), 'thinking')
  assert.strictEqual(ctx.deepyCompactionChoice('summarize', false), 'summarize')
  assert.strictEqual(ctx.deepyCompactionChoice('discard', true), 'discard')
  assert.strictEqual(ctx.deepyCompactionChoice(null, null), 'discard')
})

test('context clamps to upstream’s range and to the compaction minimum', () => {
  assert.strictEqual(ctx.deepyClampContext(1234, 'discard'), 20480)
  assert.strictEqual(ctx.deepyClampContext(20480, 'summarize'), 32000)
  assert.strictEqual(ctx.deepyClampContext(32000, 'thinking'), 48000)
  assert.strictEqual(ctx.deepyClampContext(48000, 'thinking'), 48000)
  assert.strictEqual(ctx.deepyClampContext(300000, 'discard'), 256000)
  assert.strictEqual(ctx.deepyClampContext('64000', 'discard'), 64000)
  assert.strictEqual(ctx.deepyClampContext('nonsense', 'discard'), 20480)
})

test('the panel writes every performance key and both Prime preferences', () => {
  for (const key of [
    'deepy_vram_mode',
    'deepy_context_tokens',
    'deepy_compaction_type',
    'deepy_compaction_thinking',
    'deepy_repetition_penalty',
    'deepy_model_speed',
    'deepy_model_size',
  ]) {
    assert.ok(tab.includes(key), `${key} is written by the panel`)
  }
})

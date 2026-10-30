/**
 * deepy-speculative.test.js — the Deepy speculative-decoding rows carry
 * upstream's full method set and round-trip a saved config.
 *
 * Upstream `prompt_enhancer_speculative_decoding`
 * (shared/prompt_enhancer/config.py) is a {"method", "tokens"} dict for the
 * panel's writes; plain ints are the legacy encoding upstream still reads
 * (0 = disabled, 1 = MTP at 2 draft tokens, 2 = Auto, 3/4 = MTP at 3/4 —
 * anything else truthy maps to 1). Methods: auto/disabled always offerable;
 * MTP needs enhancer id 4/5/6 (Qwen3.5-9B / Qwen3.8-9B / Qwen3.8-27B); the
 * block-draft methods DSpark/DFlash2 need the 27B (id 5) on a GGUF
 * quantization. Methods that drop out of scope fall back to Auto, like
 * upstream's ui_state. Ways this can break and what pins it: a renamed
 * upstream key (markup + write-trait test), a legacy shape shifting meaning
 * (specSplit tables), a method gating rule moving (specMethods table), and
 * a canonical write shape drifting from upstream's dict (specJoin tables).
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')

const SRC = path.join(__dirname, '..', 'src')
const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
const tab = fs.readFileSync(path.join(SRC, 'deepy-tab.js'), 'utf8')

function extractConst(name) {
  const m = tab.match(new RegExp(`const ${name} = [\\s\\S]*?;`))
  assert.ok(m, `const ${name} exists`)
  return m[0]
}

function extractFn(name) {
  const m = tab.match(new RegExp(`function\\s+${name}\\s*\\([^)]*\\)\\s*\\{[\\s\\S]*?\\n\\}`))
  assert.ok(m, `${name} is a top-level function`)
  return m[0]
}

const ctx = vm.createContext({})
vm.runInContext(
  [
    extractConst('SPEC_METHOD_LABELS'),
    extractConst('SPEC_MAX_TOKENS'),
    extractConst('SPEC_DEFAULT_TOKENS'),
    extractFn('specSplit'),
    extractFn('specJoin'),
    extractFn('specMethods'),
    extractFn('specMaxTokens'),
  ].join('\n'),
  ctx,
)

const LEGACY = [
  // [input, expected] — upstream normalize + split semantics.
  [2, { method: 'auto', tokens: null }],
  [0, { method: 'disabled', tokens: null }],
  [1, { method: 'mtp', tokens: 2 }],
  [3, { method: 'mtp', tokens: 3 }],
  [4, { method: 'mtp', tokens: 4 }],
  [5, { method: 'mtp', tokens: 2 }], // truthy non-choice -> 1 -> MTP at 2
  [-1, { method: 'mtp', tokens: 2 }], // truthy counts as 1
  [2.5, { method: 'auto', tokens: null }], // upstream int() truncates to Auto
  [true, { method: 'mtp', tokens: 2 }],
  [false, { method: 'disabled', tokens: null }],
  [null, { method: 'auto', tokens: null }],
  // Strings follow upstream's separate string branch: only "3"/"4" keep
  // their draft count, and unrecognised text lands on 0 -> disabled.
  ['auto', { method: 'auto', tokens: null }],
  ['2', { method: 'auto', tokens: null }],
  ['3', { method: 'mtp', tokens: 3 }],
  ['4', { method: 'mtp', tokens: 4 }],
  ['1', { method: 'mtp', tokens: 2 }],
  ['5', { method: 'disabled', tokens: null }],
  ['-1', { method: 'disabled', tokens: null }],
  ['true', { method: 'mtp', tokens: 2 }],
  ['false', { method: 'disabled', tokens: null }],
  ['', { method: 'disabled', tokens: null }],
  ['dspark', { method: 'dspark', tokens: 7 }],
  ['dflash2', { method: 'dflash2', tokens: 5 }],
  ['garbage', { method: 'disabled', tokens: null }], // upstream maps unknown strings to 0
]

test('specSplit reads every upstream stored shape', () => {
  // Spreads bridge the vm realm: objects made inside the context carry a
  // different Array/Object prototype, which deepStrictEqual rejects.
  for (const [input, expected] of LEGACY) {
    assert.deepStrictEqual({ ...ctx.specSplit(input) }, expected, `specSplit(${JSON.stringify(input)})`)
  }
})

test('specSplit keeps canonical dicts and clamps their tokens', () => {
  assert.deepStrictEqual({ ...ctx.specSplit({ method: 'mtp', tokens: 3 }) }, { method: 'mtp', tokens: 3 })
  assert.deepStrictEqual({ ...ctx.specSplit({ method: 'auto', tokens: 7 }) }, { method: 'auto', tokens: null })
  assert.deepStrictEqual({ ...ctx.specSplit({ method: 'disabled', tokens: 2 }) }, { method: 'disabled', tokens: null })
  // Upstream raises on out-of-range tokens; the panel clamps to the default.
  assert.deepStrictEqual({ ...ctx.specSplit({ method: 'mtp', tokens: 99 }) }, { method: 'mtp', tokens: 2 })
  assert.deepStrictEqual({ ...ctx.specSplit({ method: 'dspark' }) }, { method: 'dspark', tokens: 7 })
})

test('specJoin writes upstream canonical dicts', () => {
  assert.deepStrictEqual({ ...ctx.specJoin('auto', 5) }, { method: 'auto', tokens: null })
  assert.deepStrictEqual({ ...ctx.specJoin('disabled', null) }, { method: 'disabled', tokens: null })
  assert.deepStrictEqual({ ...ctx.specJoin('mtp', null) }, { method: 'mtp', tokens: 2 })
  assert.deepStrictEqual({ ...ctx.specJoin('mtp', '3') }, { method: 'mtp', tokens: 3 })
  assert.deepStrictEqual({ ...ctx.specJoin('dspark', 99) }, { method: 'dspark', tokens: 7 })
  assert.deepStrictEqual({ ...ctx.specJoin('dflash2', 7) }, { method: 'dflash2', tokens: 7 })
  assert.deepStrictEqual({ ...ctx.specJoin('dflash2') }, { method: 'dflash2', tokens: 5 })
  assert.deepStrictEqual({ ...ctx.specJoin('kludge') }, { method: 'auto', tokens: null })
})

test('method choices follow upstream gating', () => {
  assert.deepStrictEqual([...ctx.specMethods('5:gguf')], ['auto', 'disabled', 'mtp', 'dspark', 'dflash2'])
  assert.deepStrictEqual([...ctx.specMethods('5:gguf_q3')], ['auto', 'disabled', 'mtp', 'dspark', 'dflash2'])
  assert.deepStrictEqual([...ctx.specMethods('5:gguf_ptq1')], ['auto', 'disabled', 'mtp', 'dspark', 'dflash2'])
  assert.deepStrictEqual([...ctx.specMethods('6:gguf')], ['auto', 'disabled', 'mtp'])
  assert.deepStrictEqual([...ctx.specMethods('6:gguf_q8')], ['auto', 'disabled', 'mtp'])
  // 4B (id 3) and Qwen3.5-9B (id 4) keep MTP only where upstream allows it.
  assert.deepStrictEqual([...ctx.specMethods('3:gguf')], ['auto', 'disabled'])
  assert.deepStrictEqual([...ctx.specMethods('4:gguf')], ['auto', 'disabled', 'mtp'])
})

test('draft-token ceiling follows the drafter, not just the method', () => {
  // Upstream caps the Bonsai drafter at 5 (block_draft_spec(bonsai=True)).
  assert.strictEqual(ctx.specMaxTokens('dflash2', '5:gguf_ptq1'), 5)
  assert.strictEqual(ctx.specMaxTokens('dflash2', '5:gguf'), 7)
  assert.strictEqual(ctx.specMaxTokens('dspark', '5:gguf_ptq1'), 7)
  assert.strictEqual(ctx.specMaxTokens('mtp', '6:gguf'), 8)
  // Auto and Disabled have no token control at all.
  assert.strictEqual(ctx.specMaxTokens('auto', '5:gguf'), 0)
  assert.strictEqual(ctx.specMaxTokens('disabled', '5:gguf'), 0)
})

test('the panel markup carries the row and the upstream key', () => {
  assert.ok(/id="deepySpecWrap"/.test(html), '#deepySpecWrap exists')
  assert.ok(/id="deepySpecSelect"/.test(html), '#deepySpecSelect exists')
  assert.ok(/id="deepySpecTokensSelect"/.test(html), '#deepySpecTokensSelect exists')
  assert.ok(/id="deepySpecHint"/.test(html), '#deepySpecHint exists')
  assert.ok(html.includes('prompt_enhancer_speculative_decoding'), 'the upstream key is named')
  // Hidden with the Prime-local block it serves (inside deepyPrimeBackendWrap).
  const wrap = html.match(/<div id="deepyPrimeBackendWrap"[\s\S]*?<\/div>\s*<\/div>/)
  assert.ok(wrap && /id="deepySpecWrap"/.test(wrap[0]), 'spec row nests inside the Prime-local block')
})

test('the write path uses the immediate-write channel like the KV row', () => {
  assert.ok(
    /deepySpecWrite[\s\S]*?writeWgpConfig\(\{\s*prompt_enhancer_speculative_decoding: value/.test(tab),
    'deepySpecWrite writes via writeWgpConfig',
  )
  assert.ok(/dataset\.specBound/.test(tab), 'bindings are guarded against double-binding')
})

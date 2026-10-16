/**
 * mem-seed.test.js — Performance Settings dropdowns must reflect disk.
 *
 * Reported bug: the RAM Allocator dropdown sat on "— unset —" while its
 * `saved:` tag correctly showed "MMGP Optimized…" — the panel looked like it
 * forgot the setting (the backend never did: wgp_config.json holds it and
 * WanGP boots with it). Root cause: memProfileLoad seeded dropdowns from
 * disk only when memVideoProfile was empty, a stale-DOM proxy for "no Detect
 * ran yet". Once any Detect populated that one field, keys Detect never
 * recommends (ram_allocator above all) stayed unset forever.
 *
 * Fix under test: memProfilePopulate's "saved-seed" mode paints each
 * still-empty dropdown from disk per-field — never clobbering Detect
 * results or user picks, never touching rec: tags — and memProfileLoad
 * calls it unconditionally.
 *
 * Like theme-persist.test.js, the real bodies run in a vm against fakes.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')

const SRC = path.join(__dirname, '..', 'src')
const tab = fs.readFileSync(path.join(SRC, 'autotune-tab.js'), 'utf8')

/** pull one top-level `function name(...) {...}` body (column-0 close) */
function extractFn(name) {
  const m = tab.match(
    new RegExp(`(?:async\\s+)?function\\s+${name}\\s*\\([^)]*\\)\\s*\\{[\\s\\S]*?\\n\\}`),
  )
  assert.ok(m, `${name} not found`)
  return m[0]
}

/** pull one top-level `const NAME = {...}` (balanced braces — MEM_FIELDS nests) */
function extractConst(name) {
  const start = tab.indexOf(`const ${name} =`)
  assert.ok(start >= 0, `${name} not found`)
  const open = tab.indexOf('{', start)
  let depth = 0
  for (let i = open; i < tab.length; i++) {
    if (tab[i] === '{') depth++
    if (tab[i] === '}') {
      depth--
      if (depth === 0) return tab.slice(start, i + 1)
    }
  }
  assert.fail(`unbalanced ${name}`)
}

const LABELS = [
  'TEXT_ENCODER_LABELS',
  'KERNEL_PRECISION_LABELS',
  'QUEUE_COLOR_LABELS',
  'VRAM_ALLOCATOR_LABELS',
  'RAM_ALLOCATOR_LABELS',
  'HEAD_SPLIT_LABELS',
  'ATTENTION_MODE_LABELS',
  'PRELOAD_MODE_LABELS',
  'COMPILE_MODE_LABELS',
]

/** fake DOM: every id resolves to {value, textContent, title} unless omitted */
function harness(present = null) {
  const els = new Map()
  const el = (id) => {
    if (!els.has(id)) els.set(id, { value: '', textContent: '', title: '' })
    return els.get(id)
  }
  const ctx = {
    $: (id) => (present && !present.includes(id) ? null : el(id)),
    __els: els,
  }
  ctx.window = ctx
  vm.createContext(ctx)
  vm.runInContext(
    [
      extractConst('MEM_FIELDS'),
      ...LABELS.map(extractConst),
      extractFn('fmtVal'),
      extractFn('setTag'),
      extractFn('memProfilePopulate'),
      'this.MEM_FIELDS = MEM_FIELDS;',
      'this.populate = memProfilePopulate;',
    ].join('\n'),
    ctx,
    { timeout: 5000 },
  )
  // Pre-create every field element so tests can set/seed values directly;
  // the `present` filter still governs what the code under test sees via $.
  for (const f of Object.values(ctx.MEM_FIELDS)) {
    for (const id of Object.values(f)) {
      if (typeof id === 'string' && id && !els.has(id)) {
        els.set(id, { value: '', textContent: '', title: '' })
      }
    }
  }
  return ctx
}

const SAVED = {
  video_profile: '3',
  ram_allocator: 'mmgp',
  vram_allocator: 'vmm_spill',
}

test('saved-seed fills only empty dropdowns (the reported bug)', () => {
  const h = harness()
  // Stale Detect state: video holds a recommendation, RAM was never recommended.
  h.__els.get(h.MEM_FIELDS.video_profile.sel).value = '4'
  h.populate(SAVED, { mode: 'saved' })
  h.populate(SAVED, { mode: 'saved-seed' })
  const F = h.MEM_FIELDS
  assert.equal(h.__els.get(F.ram_allocator.sel).value, 'mmgp', 'RAM dropdown must show the disk value')
  assert.equal(h.__els.get(F.vram_allocator.sel).value, 'vmm_spill')
  assert.equal(
    h.__els.get(F.video_profile.sel).value,
    '4',
    'a Detect-populated dropdown must not be clobbered by disk',
  )
})

test('saved tags show disk while rec tags stay untouched by the seed', () => {
  const h = harness()
  h.populate(SAVED, { mode: 'saved' })
  h.populate(SAVED, { mode: 'saved-seed' })
  const F = h.MEM_FIELDS
  const savedTag = h.__els.get(F.ram_allocator.saved).textContent
  assert.match(savedTag, /MMGP Optimized/, 'saved: tag mirrors disk')
  assert.equal(h.__els.get(F.ram_allocator.rec).textContent, '', 'seed must never paint rec: tags')
  assert.equal(h.__els.get(F.video_profile.rec).textContent, '', 'rec: tags only ever come from Detect')
})

test('user picks and absent keys survive the seed', () => {
  const h = harness()
  const F = h.MEM_FIELDS
  h.__els.get(F.ram_allocator.sel).value = 'default' // user chose PyTorch
  h.populate({ ram_allocator: 'mmgp' }, { mode: 'saved-seed' })
  assert.equal(h.__els.get(F.ram_allocator.sel).value, 'default', 'explicit user pick wins over disk')
  // A key missing from saved settings seeds nothing and clears nothing.
  h.__els.get(F.vram_allocator.sel).value = ''
  h.populate({ ram_allocator: 'mmgp' }, { mode: 'saved-seed' })
  assert.equal(h.__els.get(F.vram_allocator.sel).value, '')
})

test('missing elements do not throw', () => {
  const h = harness(['memRamAllocator']) // sparse DOM: most ids absent
  assert.doesNotThrow(() => {
    h.populate(SAVED, { mode: 'saved' })
    h.populate(SAVED, { mode: 'saved-seed' })
  })
  assert.equal(h.__els.get('memRamAllocator').value, 'mmgp')
})

test('vae_config falls back to AUTO when disk has no key', () => {
  const h = harness()
  h.populate({}, { mode: 'saved-seed' })
  const sel = h.MEM_FIELDS.vae_config.sel
  assert.equal(h.__els.get(sel).value, '0', 'unset vae_config means AUTO upstream')
})

test('memProfileLoad seeds from disk unconditionally (no video-profile gate)', () => {
  // Structural guard on the exact lines that caused the report: seeding used
  // to run in recommend mode behind `first.value === ""`, which also
  // mislabelled saved values as rec: tags on fresh panels.
  assert.match(tab, /memProfilePopulate\(res\.settings, \{ mode: "saved-seed" \}\)/)
  assert.doesNotMatch(
    tab,
    /memProfilePopulate\(res\.settings, \{ mode: "recommend" \}\)/,
    'saved settings must never flow through the recommend path again',
  )
})

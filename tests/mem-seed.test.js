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
 * Second door to the same report: the "recommend" mode. It re-seeds the
 * dropdowns from the Detect payload, and the old branch wrote "" for every
 * key the payload did not mention — so the first-open auto-detect (and every
 * manual Detect / failsafe toggle) re-blanked the dropdown the seed had just
 * filled: "— unset —" beside "saved: MMGP Optimized…" again, plus a silent
 * revert of a manual pick. The backend omits keys it has no opinion on rather
 * than sending null (features.rs: extra is merged only when applicable), so
 * undefined is the signal: recommend mode now only overwrites keys the
 * recommendation actually carries.
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
      extractFn('fmtProfile'),
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

test('profile chips read P-labels, matching the dropdowns', () => {
  const h = harness()
  h.populate(
    { video_profile: '0', image_profile: '4.5', audio_profile: '3.5' },
    { mode: 'saved' },
  )
  const F = h.MEM_FIELDS
  assert.equal(h.__els.get(F.video_profile.saved).textContent, 'saved: P0')
  assert.equal(h.__els.get(F.image_profile.saved).textContent, 'saved: P4+')
  assert.equal(h.__els.get(F.audio_profile.saved).textContent, 'saved: P3+')
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

// ── recommend mode: never blank a key the recommendation does not carry ──
// ram_allocator is the reported one (upstream already defaults to mmgp, so
// Detect has no opinion on it); the v17 levers are absent on AMD/Intel for
// the same reason and must not wipe an on-disk value either.

test('recommend keeps a key the recommendation does not carry', () => {
  const h = harness()
  const F = h.MEM_FIELDS
  h.populate(SAVED, { mode: 'saved-seed' }) // disk: ram_allocator = mmgp
  h.populate({ video_profile: '2', vram_allocator: 'vmm' }, { mode: 'recommend' })
  assert.equal(
    h.__els.get(F.ram_allocator.sel).value,
    'mmgp',
    'Detect has no opinion on ram_allocator — the value on disk must survive',
  )
  assert.equal(
    h.__els.get(F.video_profile.sel).value,
    '2',
    'a key the recommendation carries is still re-seeded',
  )
  assert.equal(h.__els.get(F.vram_allocator.sel).value, 'vmm')
  assert.equal(
    h.__els.get(F.ram_allocator.rec).textContent,
    'rec: —',
    'and Detect still reports that it has no recommendation for it',
  )
})

test('recommend does not revert a manual pick for an uncarried key', () => {
  const h = harness()
  const F = h.MEM_FIELDS
  h.__els.get(F.ram_allocator.sel).value = 'default' // user picked PyTorch allocator
  h.populate({ video_profile: '2' }, { mode: 'recommend' })
  assert.equal(
    h.__els.get(F.ram_allocator.sel).value,
    'default',
    'Detect must not snap a manual pick back to "— unset —"',
  )
})

test('recommend still clears a carried key whose value is empty', () => {
  const h = harness()
  const F = h.MEM_FIELDS
  h.__els.get(F.attention_mode.sel).value = 'sage2'
  h.populate({ attention_mode: '' }, { mode: 'recommend' })
  assert.equal(
    h.__els.get(F.attention_mode.sel).value,
    '',
    'a key Detect does carry, with no value, still means unset',
  )
})

test('recommend carries vae_config 0 through as AUTO, not as empty', () => {
  const h = harness()
  h.populate({ vae_config: 0 }, { mode: 'recommend' })
  assert.equal(h.__els.get(h.MEM_FIELDS.vae_config.sel).value, '0')
})

test('recommend only overwrites what the payload carries (structural)', () => {
  // Guards the exact condition: reverting it to `if (sel)` re-introduces the
  // blanked-dropdown report.
  assert.match(tab, /if \(sel && v !== undefined\)/)
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

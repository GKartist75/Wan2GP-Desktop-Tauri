/**
 * adult-pack-refresh.test.js — the freshness check must be trustworthy.
 *
 * `npm run pack:check` is the thing that tells a human (or CI) whether the
 * committed catalog still matches what the sources serve. A check that is
 * subtly wrong is worse than none:
 *   - if it ignores `generatedAt` wrongly, it reports drift on every run and
 *     gets muted within a week
 *   - if it compares too much, it re-fires forever
 *   - if it compares too little, real model swaps go unnoticed
 *   - if its summary is unreadable, nobody reviews the PR it opens
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')
const { pathToFileURL } = require('node:url')

const ROOT = path.join(__dirname, '..')
const SRC = path.join(ROOT, 'src')

// One shared promise, awaited by every test. A dynamic import needs a file://
// URL on Windows, and module-level `mod` would race the test order: a later
// test can read it before the first one assigns it.
const modPromise = import(pathToFileURL(path.join(ROOT, 'scripts', 'refresh-adult-pack.mjs')).href)

test('the refresh script is importable without running the verifier', async () => {
  // Importing must not kick off a network run: that would make the test suite
  // slow, rate-limited and non-deterministic.
  const mod = await modPromise
  assert.equal(typeof mod.comparable, 'function')
  assert.equal(typeof mod.index, 'function')
  assert.equal(typeof mod.diffManifest, 'function')
  assert.equal(typeof mod.formatSummary, 'function')
})

const fixture = (over = {}) => ({
  generatedAt: '2026-01-01T00:00:00.000Z',
  loras: [
    { id: 'a', name: 'A', repo: 'r/a', sha256: 'a'.repeat(64), bytes: 100 },
    { id: 'b', name: 'B', repo: 'r/b', sha256: 'b'.repeat(64), bytes: 200 },
  ],
  finetunes: [{ id: 'f1', name: 'F1', repo: 'r/f', sha256: 'c'.repeat(64), bytes: 300 }],
  ...over,
})

test('a new generatedAt alone is NOT drift', async () => {
  const mod = await modPromise
  const a = fixture()
  const b = fixture({ generatedAt: '2026-06-06T06:06:06.000Z' })
  assert.equal(mod.comparable(a), mod.comparable(b), 'the timestamp must be excluded')
  assert.equal(mod.diffManifest(a, b).stale, false)
  assert.equal(mod.diffManifest(a, b).total, 0)
})

test('a real change IS drift', async () => {
  const mod = await modPromise
  const a = fixture()
  const b = fixture()
  b.loras[0].sha256 = 'd'.repeat(64)
  b.generatedAt = '2026-06-06T06:06:06.000Z'
  const d = mod.diffManifest(a, b)
  assert.equal(d.stale, true)
  assert.deepEqual(d.changed, ['loras:a'])
})

test('entries are keyed per bucket, so a lora and a recipe can share an id', async () => {
  const mod = await modPromise
  const m = fixture()
  m.loras.push({ id: 'f1', name: 'Lora called f1', repo: 'r/x', sha256: 'e'.repeat(64), bytes: 1 })
  const idx = mod.index(m)
  assert.equal(idx.size, 4, '3 entries + the colliding one')
  assert.ok(idx.has('loras:f1') && idx.has('finetunes:f1'))
})

test('HIGH/LOW halves count as one entry of their combined size', async () => {
  const mod = await modPromise
  const m = { generatedAt: 'x', loras: [], finetunes: [] }
  m.finetunes.push({ id: 'h', name: 'H+L', repo: 'r', sha256: 'f'.repeat(64), bytes: 10, bytesLow: 5 })
  assert.equal(mod.index(m).get('finetunes:h').bytes, 15)
  // ...and moving only the low half is still a detected change.
  const other = JSON.parse(JSON.stringify(m))
  other.finetunes[0].bytesLow = 6
  assert.deepEqual(mod.diffManifest(m, other).changed, ['finetunes:h'])
})

test('added, removed and changed are reported separately', async () => {
  const mod = await modPromise
  const before = fixture()
  const after = {
    generatedAt: 'later',
    loras: [
      { id: 'a', name: 'A', repo: 'r/a', sha256: 'a'.repeat(64), bytes: 150 }, // changed size
      { id: 'c', name: 'C', repo: 'r/c', sha256: 'c'.repeat(64), bytes: 50 }, // added
    ],
    finetunes: [{ id: 'f1', name: 'F1', repo: 'r/f', sha256: 'c'.repeat(64), bytes: 300 }],
  }
  const d = mod.diffManifest(before, after)
  assert.deepEqual(d.added, ['loras:c'])
  assert.deepEqual(d.removed, ['loras:b'])
  assert.deepEqual(d.changed, ['loras:a'])
  assert.equal(d.changedBytes, 50)
})

test('a repo change counts, so a mirrored file moving is not silent', async () => {
  const mod = await modPromise
  const before = fixture()
  const after = fixture()
  after.loras[0].repo = 'r/relocated'
  assert.deepEqual(mod.diffManifest(before, after).changed, ['loras:a'])
})

test('the summary is readable: it names models, not just counts', async () => {
  const mod = await modPromise
  const before = fixture()
  before.loras[0].bytes = 100 * 1024 * 1024
  const after = JSON.parse(JSON.stringify(before))
  after.generatedAt = 'later'
  after.loras[0].sha256 = '9'.repeat(64)
  after.loras[0].bytes = 150 * 1024 * 1024 // a real-size change, so MB reads sensibly
  after.loras.push({ id: 'z', name: 'Brand New', repo: 'r/z', sha256: '7'.repeat(64), bytes: 7 })
  after.loras = after.loras.filter((l) => l.id !== 'b')
  const d = mod.diffManifest(before, after)
  d.before = mod.index(before)
  d.after = mod.index(after)
  const text = mod.formatSummary(d)
  assert.match(text, /3 change\(s\)/)
  assert.match(text, /added: 1/)
  assert.match(text, /removed: 1/)
  assert.match(text, /changed hash or size: 1/)
  assert.match(text, /Brand New/, 'the added model must be named')
  assert.match(text, /sha a{12} -> 9{12}/, 'the hash change must be shown')
  assert.match(text, /size 100\.0 MB -> 150\.0 MB/, 'the size change must be shown')
  assert.match(text, /\+50\.0 MB/, 'the size delta must be shown')
})

test('a long list is truncated rather than dumping the whole catalog', async () => {
  const mod = await modPromise
  const before = { generatedAt: 'x', loras: [], finetunes: [] }
  const after = { generatedAt: 'y', loras: [], finetunes: [] }
  for (let i = 0; i < 40; i++) {
    after.loras.push({ id: `n${i}`, name: `New ${i}`, repo: 'r', sha256: '1'.repeat(64), bytes: 1 })
  }
  const d = mod.diffManifest(before, after)
  d.before = mod.index(before)
  d.after = mod.index(after)
  const text = mod.formatSummary(d, 12)
  assert.match(text, /… and 28 more/)
  assert.ok(!text.includes('New 39'), 'the tail must be summarised, not printed')
})

test('an unchanged catalog says so in one line', async () => {
  const mod = await modPromise
  const a = fixture()
  const d = mod.diffManifest(a, fixture({ generatedAt: 'later' }))
  d.before = mod.index(a)
  d.after = mod.index(fixture({ generatedAt: 'later' }))
  assert.match(mod.formatSummary(d), /up to date/)
})

test('the npm scripts and the real manifest line up', async () => {
  const mod = await modPromise
  const pkg = JSON.parse(fs.readFileSync(path.join(ROOT, 'package.json'), 'utf8'))
  for (const s of ['pack:check', 'pack:refresh', 'pack:verify', 'pack:report']) {
    assert.ok(pkg.scripts[s], `package.json is missing the ${s} script`)
  }
  // pack:check must not silently write: --check is what CI runs.
  assert.match(pkg.scripts['pack:check'], /refresh-adult-pack\.mjs --check/)
  assert.match(pkg.scripts['pack:refresh'], /refresh-adult-pack\.mjs --refresh/)

  // And the manifest the check compares must be the one the app embeds.
  const rust = fs.readFileSync(path.join(ROOT, 'src-tauri', 'src', 'adult_pack.rs'), 'utf8')
  assert.match(rust, /include_str!\("\.\.\/\.\.\/src\/adult-pack-manifest\.json"\)/)
  assert.ok(fs.existsSync(path.join(SRC, 'adult-pack-manifest.json')))
})
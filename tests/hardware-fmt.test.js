/**
 * hardware-fmt.test.js — the Auto-tune Hardware chip formatters.
 *
 * The chips used to concatenate the raw probe value (`ram_gb + " GB"`), which
 * rendered 31.763145446777344 GB on a real machine. fmtGb is the contract now:
 * at most one decimal, no trailing ".0", and never NaN/undefined.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')

const SRC = path.join(__dirname, '..', 'src')

// load fmtGb out of the file without booting the whole renderer
function loadFmtGb() {
  const file = fs
    .readdirSync(SRC)
    .filter((f) => f.endsWith('.js'))
    .find((f) => /function fmtGb/.test(fs.readFileSync(path.join(SRC, f), 'utf8')))
  assert.ok(file, 'fmtGb not found in any src/*.js')
  const text = fs.readFileSync(path.join(SRC, file), 'utf8')
  const m = text.match(/function fmtGb[\s\S]*?\n}/)
  assert.ok(m, 'could not extract fmtGb')
  const box = {}
  vm.createContext(box)
  vm.runInContext(m[0] + '\nthis.fmtGb = fmtGb;', box, { timeout: 5000 })
  return box.fmtGb
}

const fmtGb = loadFmtGb()

test('rounds a raw probe float to one decimal', () => {
  // The exact value a real RTX 3080 / 32GB box produced.
  assert.equal(fmtGb(31.763145446777344), '31.8')
  assert.equal(fmtGb(10.0), '10')
  assert.equal(fmtGb(9.9999), '10')
})

test('keeps one decimal when there is a fraction', () => {
  assert.equal(fmtGb(7.25), '7.3')
  assert.equal(fmtGb(15.04), '15')
  assert.equal(fmtGb(24), '24')
})

test('never renders NaN, undefined or zero-sized cards', () => {
  assert.equal(fmtGb(null), '—')
  assert.equal(fmtGb(undefined), '—')
  assert.equal(fmtGb(NaN), '—')
  assert.equal(fmtGb('abc'), '—')
  assert.equal(fmtGb(0), '—')
  assert.equal(fmtGb(-3), '—')
})

test('accepts numeric strings from JSON probes', () => {
  assert.equal(fmtGb('31.763145446777344'), '31.8')
  assert.equal(fmtGb('10'), '10')
})
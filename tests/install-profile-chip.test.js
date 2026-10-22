/**
 * install-profile-chip.test.js — the post-install "Profile:" chip.
 *
 * The chip used to re-derive the GPU class with its own regexes, which
 * disagreed with the backend: Intel Arc read "STANDARD", every AMD card read a
 * bare "AMD", and a generic adapter name ("AMD Radeon(TM) Graphics") matched
 * none of the branches at all. It now renders the backend's profile key
 * through profileChipLabel.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')

const SRC = path.join(__dirname, '..', 'src', 'installer-tab.js')

function loadLabel() {
  const text = fs.readFileSync(SRC, 'utf8')
  const m = text.match(/function profileChipLabel[\s\S]*?\n}/)
  assert.ok(m, 'could not extract profileChipLabel')
  const box = {}
  vm.createContext(box)
  vm.runInContext(m[0] + '\nthis.profileChipLabel = profileChipLabel;', box, {
    timeout: 5000,
  })
  return box.profileChipLabel
}

const label = loadLabel()

test('nvidia tiers keep their familiar names', () => {
  assert.equal(label('RTX_50'), 'RTX 50')
  assert.equal(label('RTX_40'), 'RTX 40')
  assert.equal(label('GTX_10'), 'GTX 10/16')
})

test('amd shows the device target', () => {
  assert.equal(label('AMD_GFX1201'), 'AMD gfx1201')
  assert.equal(label('AMD_GFX110X'), 'AMD gfx110x')
  assert.equal(label('AMD_GFX103X'), 'AMD gfx103x')
})

test('intel and cpu are honest about the CPU torch', () => {
  assert.equal(label('INTEL_XPU'), 'Intel (CPU torch)')
  assert.equal(label('CPU'), 'CPU')
  assert.equal(label('MPS'), 'Apple MPS')
})

test('empty/unknown never prints a fake tier', () => {
  assert.equal(label(''), '')
  assert.equal(label('unknown'), '')
  assert.equal(label(null), '')
})

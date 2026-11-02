/**
 * deepy-transcription.test.js — Prime's advanced-transcription control carries
 * upstream's model set and writes its own keys.
 *
 * Upstream `deepy_advanced_transcription` (shared/deepy/config.py) accepts
 * exactly `nemotron_transcribe` and `nemotron_transcribe_q4`; anything else,
 * including a missing key, normalizes to `disabled` ("Use Whisper Only").
 * The panel must offer those three values, show the terms field only for a
 * Nemotron model, and write `deepy_advanced_transcription` /
 * `deepy_transcription_terms` without touching Whisper's quick-transcript
 * path.
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
  [extractConst('DEEPY_TRANSCRIPTION_MODELS'), extractFn('deepyTranscriptionValue')].join('\n'),
  ctx,
)

test('the model dropdown offers upstream’s three choices', () => {
  assert.deepStrictEqual(options('deepyAdvancedTranscriptionSelect'), [
    'disabled',
    'nemotron_transcribe',
    'nemotron_transcribe_q4',
  ])
})

test('unknown or missing saved models show as Whisper Only', () => {
  assert.strictEqual(ctx.deepyTranscriptionValue('nemotron_transcribe'), 'nemotron_transcribe')
  assert.strictEqual(ctx.deepyTranscriptionValue('nemotron_transcribe_q4'), 'nemotron_transcribe_q4')
  assert.strictEqual(ctx.deepyTranscriptionValue('disabled'), 'disabled')
  assert.strictEqual(ctx.deepyTranscriptionValue('bogus'), 'disabled')
  assert.strictEqual(ctx.deepyTranscriptionValue(''), 'disabled')
  assert.strictEqual(ctx.deepyTranscriptionValue(null), 'disabled')
  assert.strictEqual(ctx.deepyTranscriptionValue(2), 'disabled')
})

test('the panel writes both upstream transcription keys', () => {
  assert.match(tab, /deepy_advanced_transcription/)
  assert.match(tab, /deepy_transcription_terms/)
  // The terms block exists and the model picker controls its visibility.
  assert.match(html, /id="deepyTranscriptionTermsWrap"/)
  assert.match(html, /id="deepyTranscriptionTermsInput"/)
  assert.match(
    tab,
    /txTermsWrap\.style\.display\s*=\s*\n?\s*txSel && txSel\.value !== "disabled"/,
  )
})

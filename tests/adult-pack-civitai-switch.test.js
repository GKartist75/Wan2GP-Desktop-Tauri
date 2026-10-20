/**
 * adult-pack-civitai-switch.test.js — CivitAI is opt-in.
 *
 * `civitai.red` is the only adult host the launcher talks to, and security
 * suites commonly block it as an adult site. The switch must therefore:
 *   1. default to off and persist per install,
 *   2. refuse every CivitAI command (and direct CivitAI file downloads) before
 *      opening a socket,
 *   3. hide the CivitAI UI and fall back to HuggingFace when off.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')

const ROOT = path.join(__dirname, '..')
const SRC = path.join(ROOT, 'src')
const rust = fs.readFileSync(path.join(ROOT, 'src-tauri', 'src', 'adult_pack.rs'), 'utf8')
const lib = fs.readFileSync(path.join(ROOT, 'src-tauri', 'src', 'lib.rs'), 'utf8')
const bridge = fs.readFileSync(path.join(SRC, 'w2gp.js'), 'utf8')
const renderer = fs.readFileSync(path.join(SRC, 'adult-pack-tab.js'), 'utf8')
const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')

test('CivitAI defaults to off and is exposed in the manifest', () => {
  const enabled = (rust.split('fn civitai_enabled()')[1] || '').split('\n}')[0]
  assert.ok(enabled.includes('unwrap_or(false)'), 'CivitAI must default to off')
  assert.ok(rust.includes('fn require_civitai()'), 'missing the refusal helper')
  assert.ok(rust.includes('pub fn adult_pack_set_civitai(enabled: bool)'), 'missing the setter command')
  assert.ok(lib.includes('adult_pack::adult_pack_set_civitai'), 'setter not registered in lib.rs')
  assert.ok(bridge.includes('adultPackSetCivitai'), 'setter missing from the w2gp bridge')
  assert.ok(bridge.includes('adult_pack_set_civitai'), 'bridge must call the setter command')
  assert.ok(rust.includes('"civitaiEnabled": civitai_enabled()'), 'manifest must expose the setting')
})

test('no CivitAI command opens a socket while the switch is off', () => {
  for (const fn of [
    'adult_pack_civitai_verify',
    'adult_pack_civitai_download',
    'adult_pack_civitai_gallery',
    'adult_pack_browse',
    'adult_pack_search',
    'adult_pack_inspect',
  ]) {
    const body = (rust.split(`pub async fn ${fn}`)[1] || '').split('\n#[tauri')[0]
    assert.ok(body.includes('require_civitai()?'), `${fn} must refuse while CivitAI is off`)
  }
  // Direct downloads of user-added CivitAI models follow the same switch.
  assert.ok(rust.includes('fn is_civitai_url('), 'missing the CivitAI URL detector')
  assert.ok(rust.includes('!civitai_enabled() && is_civitai_url(&url)'), 'fetch must honour the switch')
})

test('the UI hides CivitAI and falls back to HuggingFace when off', () => {
  assert.ok(html.includes('id="apCivEnabled"'), 'missing the CivitAI toggle')
  assert.ok(html.includes('id="apCivSection"'), 'missing the CivitAI section')
  assert.ok(renderer.includes('adultPackSetCivitai('), 'renderer must save the toggle')
  assert.ok(renderer.includes('function applyCivitaiVisibility()'), 'renderer must hide the section when off')
  const planBody = (renderer.split('function plan()')[1] || '').split('\n  }')[0]
  assert.ok(planBody.includes('civOn()'), 'a selection left from before must not count while off')
})

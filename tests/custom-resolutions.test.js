/**
 * custom-resolutions.test.js — the launcher edits Wan2GP's own
 * `<repo>/resolutions.json` without becoming a second source of defaults.
 *
 * The contract:
 *   1. the backend only ever resolves `resolutions.json` next to `wgp.py`;
 *   2. every row is validated before the existing file is replaced;
 *   3. the bridge and Manage → General card are wired;
 *   4. opening Manage reloads the file, so hand edits are never silently lost.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')

const ROOT = path.join(__dirname, '..')
const SRC = path.join(ROOT, 'src')
const rust = fs.readFileSync(
  path.join(ROOT, 'src-tauri', 'src', 'resolutions.rs'),
  'utf8',
)
const lib = fs.readFileSync(path.join(ROOT, 'src-tauri', 'src', 'lib.rs'), 'utf8')
const bridge = fs.readFileSync(path.join(SRC, 'w2gp.js'), 'utf8')
const settings = fs.readFileSync(path.join(SRC, 'settings-tab.js'), 'utf8')
const term = fs.readFileSync(path.join(SRC, 'term-tab.js'), 'utf8')
const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
const css = fs.readFileSync(path.join(SRC, 'style.css'), 'utf8')

test('the writer can only target the repo resolutions.json', () => {
  const fn = (rust.split('pub fn custom_resolutions_write')[1] || '').split(
    '\n#[cfg(test)]',
  )[0]
  assert.ok(fn.includes('repo.join("resolutions.json")'), 'wrong destination')
  assert.ok(!/custom_resolutions_write\([^)]*path/i.test(fn), 'no caller-supplied path')
  assert.ok(fn.includes('atomic_write'), 'a half-written JSON file must be impossible')
  assert.ok(fn.includes('wgp.py'), 'must refuse before Wan2GP is installed')
  assert.ok(
    rust.includes('get_repo_dir().join("resolutions.json")'),
    'the read path must match the write path',
  )
})

test('rows are validated before the existing file is touched', () => {
  assert.ok(rust.includes("split_once('x')"), 'must parse WIDTHxHEIGHT')
  assert.ok(rust.includes("starts_with('0')"), 'Wan2GP rejects leading zeroes')
  assert.ok(rust.includes('duplicate label'), 'duplicate labels must be refused')
  assert.ok(
    rust.includes('duplicate resolution'),
    'Wan2GP dedupes by size, so a duplicate size must be refused',
  )
  assert.ok(rust.includes('65535'), 'must cap absurd dimensions')
  assert.ok(rust.includes('exactly a label and a size'), 'must require two elements')
  assert.ok(
    rust.includes('fn round_trips_to_the_repo_root'),
    'missing the write/read round-trip test',
  )
  assert.ok(
    rust.includes('fn refuses_to_write_before_wan2gp_is_installed'),
    'missing the pre-install guard test',
  )
})

test('both commands are registered and exposed to the renderer', () => {
  assert.ok(lib.includes('mod resolutions;'), 'module not declared')
  assert.ok(lib.includes('resolutions::custom_resolutions_read'), 'read not registered')
  assert.ok(lib.includes('resolutions::custom_resolutions_write'), 'write not registered')
  assert.ok(bridge.includes('custom_resolutions_read'), 'read missing from the bridge')
  assert.ok(bridge.includes('custom_resolutions_write'), 'write missing from the bridge')
})

test('Manage → General has a custom resolutions editor', () => {
  for (const id of [
    'resolutionsList',
    'resolutionsAddBtn',
    'resolutionsExampleBtn',
    'resolutionsReloadBtn',
    'resolutionsSaveBtn',
    'resolutionsStatus',
    'resolutionsPath',
  ]) {
    assert.ok(html.includes(`id="${id}"`), `missing #${id}`)
  }
  assert.ok(html.includes('Custom Resolutions'), 'missing the card heading')
  assert.ok(
    html.includes('next to <code>wgp.py</code>'),
    'the card must say where the file lives',
  )
  assert.ok(css.includes('.resolution-row'), 'missing the row layout')
  assert.ok(
    settings.includes('window.w2gp.customResolutionsRead()'),
    'the editor must load the saved list',
  )
  assert.ok(
    settings.includes('window.w2gp.customResolutionsWrite(list)'),
    'Save must call the validated writer',
  )
  assert.ok(
    settings.includes('duplicate label'),
    'the UI should catch duplicate labels before writing',
  )
  assert.ok(
    settings.includes('Remove all custom resolutions?'),
    'saving an empty list must be confirmed',
  )
  assert.ok(
    term.includes('loadCustomResolutions()'),
    'opening Manage must refresh the editor',
  )
})

test('the Load example button carries the requested Krea 2 / H3 sizes', () => {
  assert.ok(settings.includes('const RESOLUTION_EXAMPLE'), 'missing the example list')
  for (const pair of [
    ['1600x1200 (4:3, 1080p)', '1600x1200'],
    ['1200x1600 (3:4, 1080p)', '1200x1600'],
    ['1376x768 (16:9, 1MP) - Krea 2 (Official)', '1376x768'],
    ['768x1376 (9:16, 1MP) - Krea 2 (Official)', '768x1376'],
    ['1344x768 (16:9, 1MP) - MM H3 (Official)', '1344x768'],
    ['768x1344 (9:16, 1MP) - MM H3 (Official)', '768x1344'],
    ['1184x896 (4:3, 1MP) - Krea 2 (Official)', '1184x896'],
    ['896x1184 (3:4, 1MP) - Krea 2 (Official)', '896x1184'],
  ]) {
    assert.ok(
      settings.includes(`["${pair[0]}", "${pair[1]}"]`),
      `missing ${pair[0]}`,
    )
  }
})

/**
 * launcher-update.test.js — the "how does this copy update?" copy.
 *
 * The kinds live in Rust (`LauncherInstall::as_str`) because the backend picks
 * the download asset; the words live here. This suite parses updates.rs so a new
 * kind cannot ship with a missing hint, and pins the two behaviours users see:
 * a source build never tells them to "install the installer", and a release
 * without an asset URL still lands on the releases page.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')

const ROOT = path.join(__dirname, '..')
const lu = require('../src/services/launcher-update.js')
const UPDATES_RS = fs.readFileSync(
  path.join(ROOT, 'src-tauri', 'src', 'updates.rs'),
  'utf8',
)
const HTML = fs.readFileSync(path.join(ROOT, 'src', 'index.html'), 'utf8')
const AUTOUPDATE = fs.readFileSync(
  path.join(ROOT, 'src', 'autoupdate-tab.js'),
  'utf8',
)

/** `Self::AppImage => "appimage",` — the serialized install kinds. */
function rustKinds() {
  const out = []
  const re = /Self::(\w+)\s*=>\s*"([a-z]+)"/g
  let m
  while ((m = re.exec(UPDATES_RS))) out.push(m[2])
  return out
}

test('every install kind in Rust has a hint and a label', () => {
  const kinds = rustKinds()
  assert.ok(kinds.length >= 6, 'did not parse install kinds: ' + kinds.join(','))
  for (const kind of kinds) {
    assert.ok(lu.HINTS[kind], 'missing hint for ' + kind)
    assert.ok(lu.KIND_LABEL[kind], 'missing label for ' + kind)
  }
  // ...and no JS-only kind can linger after a Rust rename.
  assert.deepStrictEqual(Object.keys(lu.HINTS).sort(), kinds.slice().sort())
})

test('a source build is told to rebuild, never to run an installer', () => {
  assert.match(lu.hint('source'), /git pull/)
  assert.doesNotMatch(lu.hint('source'), /installer|\.dmg|\.deb/)
  assert.strictEqual(lu.actionText('source'), 'Releases')
  assert.strictEqual(lu.actionText('deb'), 'Download')
  // Unknown kinds fail closed to the source wording.
  assert.strictEqual(lu.hint('plan9'), lu.HINTS.source)
  assert.strictEqual(lu.kindLabel('plan9'), lu.KIND_LABEL.source)
})

test('statusLine names the version, the step and the artifact', () => {
  const line = lu.statusLine({
    version: '0.11.0',
    installKind: 'deb',
    assetName: 'wan2gp_0.11.0_amd64.deb',
    command: 'sudo apt install ./wan2gp_0.11.0_amd64.deb',
  })
  assert.match(line, /^v0\.11\.0 available — /)
  assert.match(line, /download the \.deb/)
  assert.match(line, /wan2gp_0\.11\.0_amd64\.deb/)
  assert.strictEqual(lu.statusLine({}), '')
  assert.strictEqual(lu.statusLine(null), '')
})

test('downloadUrl prefers the asset, then the release page', () => {
  assert.strictEqual(
    lu.downloadUrl({ url: 'https://a/w.deb', htmlUrl: 'https://a/tag' }),
    'https://a/w.deb',
  )
  assert.strictEqual(
    lu.downloadUrl({ htmlUrl: 'https://a/tag' }),
    'https://a/tag',
  )
  assert.strictEqual(lu.downloadUrl(null), lu.RELEASES_URL)
  assert.strictEqual(lu.downloadUrl({}), lu.RELEASES_URL)
})

test('commandFor only returns a shell step when the backend sent one', () => {
  assert.strictEqual(
    lu.commandFor({ command: 'sudo apt install ./x.deb' }),
    'sudo apt install ./x.deb',
  )
  assert.strictEqual(lu.commandFor({ installKind: 'appimage' }), '')
  assert.strictEqual(lu.commandFor(null), '')
})

test('the renderer loads the module before app.js and wires the new statuses', () => {
  assert.ok(HTML.includes('services/launcher-update.js'))
  assert.ok(
    HTML.indexOf('services/launcher-update.js') < HTML.indexOf('src="app.js"'),
    'launcher-update.js must load before app.js',
  )
  // Both new backend states are handled, and the copy button is created by id.
  assert.match(AUTOUPDATE, /case "unmanaged":/)
  assert.match(AUTOUPDATE, /case "installed":/)
  assert.match(AUTOUPDATE, /manageUpdateCopyCmdBtn/)
})

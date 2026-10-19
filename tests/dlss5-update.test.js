/**
 * dlss5-update.test.js — the DLSS 5 upstream-release probe is wired end to end.
 *
 * Upstream's scripts/install_dlss5.ps1 owns every component version (hardcoded
 * URLs + SHA-256s), so a newer DLSS / ReShade / RenoDX build can only arrive
 * through a Wan2GP update. The probe answers "is a newer version available?"
 * read-only: it asks the sources that script pins, and never downloads,
 * installs or re-pins anything.
 *
 * Structural guards: button, result box, bridge, command registration, the
 * pins' single home, and the "update Wan2GP" wording.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')

const ROOT = path.join(__dirname, '..')
const SRC = path.join(ROOT, 'src')
const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
const bridge = fs.readFileSync(path.join(SRC, 'w2gp.js'), 'utf8')
const tab = fs.readFileSync(path.join(SRC, 'dlss5-tab.js'), 'utf8')
const rust = fs.readFileSync(path.join(ROOT, 'src-tauri', 'src', 'install.rs'), 'utf8')
const lib = fs.readFileSync(path.join(ROOT, 'src-tauri', 'src', 'lib.rs'), 'utf8')
const dlTab = fs.readFileSync(path.join(SRC, 'downloads-tab.js'), 'utf8')

test('the DLSS 5 card carries the update-probe button and its own result box', () => {
  assert.match(html, /id="dlss5UpdateBtn"/)
  assert.match(html, /Check for updates/)
  assert.match(html, /id="dlss5UpdateResult"/)
  assert.match(
    html,
    /Update Wan2GP<\/em> away/,
    'the card must say where a newer build actually comes from',
  )
})

test('the w2gp bridge maps the probe command', () => {
  assert.match(bridge, /dlss5UpdateCheck:\s*\(\)\s*=>\s*call\("dlss5_update_check"\)/)
})

test('the backend command exists and is registered', () => {
  assert.match(rust, /pub async fn dlss5_update_check\(\)/)
  assert.match(lib, /install::dlss5_update_check,/)
})

test('the probe reads the sources upstream pins (and reshade.me for ReShade)', () => {
  assert.match(rust, /RankFTW\/rhi-repo/)
  assert.match(rust, /DeepBeepMeep\/dlss5-visual-enhancer/)
  assert.match(rust, /reshade\.me/)
})

test('the pins have one home: the probe derives them from DLSS5_FILES', () => {
  assert.match(
    rust,
    /fn dlss_pinned_version[\s\S]{0,700}DLSS5_FILES/,
    'the pins must be read out of the shipped manifest, not copied',
  )
  assert.match(
    rust,
    /fn every_probe_row_maps_to_a_manifest_pin/,
    'a pkg-id typo would report "not checked" forever',
  )
})

test('a newer upstream release is offered as a Wan2GP update, never self-installed', () => {
  assert.match(tab, /dlss5UpdateCheck\(\)/, 'must go through the bridge')
  assert.match(tab, /update Wan2GP to pick it up/)
  assert.match(tab, /never downloads a build upstream has not pinned/)
  assert.doesNotMatch(tab, /installDlss5\(/, 'the probe must not trigger an install')
})

test('an unreachable upstream is reported honestly, not as "current"', () => {
  assert.match(tab, /Couldn't reach the upstream sources/)
  assert.match(tab, /upstream unreachable/)
  assert.match(tab, /nothing was checked or changed/)
})

test('upstream-update notes appear only after the user asks for them', () => {
  // A healthy, installed panel must not decorate itself with "update Wan2GP" on
  // launch: that is what made correct rows look wrong. The startup probe still
  // refreshes the data, it just does not paint it.
  assert.match(tab, /_dlss5UpdateAsked = false/, 'the session starts with notes off')
  assert.match(
    tab,
    /_dlss5UpdateAsked \? _dlss5Update\[f\.pkg\] : undefined/,
    'the row is handed no update verdict until it is asked for',
  )
  const handler = tab.slice(tab.indexOf('$("dlss5UpdateBtn")'))
  assert.match(handler, /_dlss5UpdateAsked = true/, 'the button is what turns them on')
  const auto = tab.slice(tab.indexOf('async function maybeRefreshDlss5Updates()'))
  const autoBody = auto.slice(0, auto.indexOf('async function verifyDlss5Files()'))
  assert.doesNotMatch(
    autoBody,
    /_dlss5UpdateAsked/,
    'the automatic TTL probe never decorates the panel',
  )
})

test('the Force setting is remembered, the consent tick is not', () => {
  // Force changes what an install does (replace vs skip), so losing it on the
  // next launch silently changes behaviour. Consent is the opposite: it must be
  // given again every time.
  assert.match(dlTab, /async function loadDlss5Prefs\(\)/, 'the setting must be read back')
  assert.match(dlTab, /async function saveDlss5Prefs\(/, 'and written on change')
  assert.match(dlTab, /cfg\.dlss5 = \{ \.\.\.\(cfg\.dlss5 \|\| \{\}\), force: !!force \}/)
  assert.match(dlTab, /loadDlss5Prefs\(\);/, 'it is restored on load, not only on click')
  assert.doesNotMatch(
    dlTab,
    /dlss5AcceptChk[\s\S]{0,200}configSave/,
    'the consent tick must never be persisted',
  )
})

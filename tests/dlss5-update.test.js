/**
 * dlss5-update.test.js — the DLSS 5 upstream-release probe is wired end to end.
 *
 * Wan2GP's scripts/install_dlss5.ps1 owns every component version (hardcoded
 * URLs + SHA-256s), and the launcher reads those pins from the local checkout.
 * The probe answers "is a newer version available upstream?" read-only: it
 * downloads, installs and re-pins nothing. Updating Wan2GP replaces the script,
 * never the installed files, so a reinstall is still required.
 *
 * Structural guards: button, result box, bridge, command registration, live
 * script parsing, fallback pins, and honest wording.
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

test('the DLSS 5 card carries the upstream-probe button and its own result box', () => {
  assert.match(html, /id="dlss5UpdateBtn"/)
  assert.match(html, /Compare with upstream/)
  assert.match(html, /id="dlss5UpdateResult"/)
  assert.match(
    html,
    /Updating Wan2GP replaces that script, not the files already in/,
    'the card must say what a Wan2GP update does and does not update',
  )
  assert.match(
    html,
    /run <em>Install \/ update DLSS 5<\/em> afterwards to apply new pins/,
    'the card must name the step that actually replaces files',
  )
})

test('the w2gp bridge maps the probe command', () => {
  assert.match(bridge, /dlss5UpdateCheck:\s*\(\)\s*=>\s*call\("dlss5_update_check"\)/)
})

test('the backend command exists and is registered', () => {
  assert.match(rust, /pub async fn dlss5_update_check\(\)/)
  assert.match(lib, /install::dlss5_update_check,/)
})

test('the probe reads DeepBeepMeep/Wan2GP’s installer script, not component feeds', () => {
  assert.match(
    rust,
    /raw\.githubusercontent\.com\/deepbeepmeep\/WanGP\/main\/scripts\/install_dlss5\.ps1/,
    'the update source is DeepBeepMeep/Wan2GP main',
  );
  assert.match(rust, /deepbeepmeep\/WanGP/i, 'the UI and logs must name the upstream');
  assert.doesNotMatch(
    rust,
    /RankFTW\/rhi-repo/,
    'component release tags are not an update source',
  );
  assert.doesNotMatch(rust, /reshade\.me/, 'reshade.me is not an update source');
});

test('the pins are read from Wan2GP’s installer script, with a fallback', () => {
  assert.match(
    rust,
    /fn dlss5_parse_script_pins/,
    'upstream version pins must be parsed from the live script',
  );
  assert.match(rust, /scripts\/install_dlss5\.ps1/, 'that is the file the installer runs');
  assert.match(rust, /fn dlss5_pins_for_repo/, 'the script is preferred per checkout');
  assert.match(
    rust,
    /fn dlss5_static_pins/,
    'a missing/unreadable script still needs verifiable fallback pins',
  );
  assert.match(rust, /"pinsSource": pins_source/, 'the panel must be told which pins were used');
  assert.match(
    rust,
    /fn every_update_row_maps_to_a_manifest_pin/,
    'a pkg-id typo would report "not checked" forever',
  );
});

test('a newer DeepBeepMeep/Wan2GP pin is informational, never self-installed', () => {
  assert.match(tab, /dlss5UpdateCheck\(\)/, 'must go through the bridge');
  assert.match(tab, /DeepBeepMeep\/Wan2GP pins a newer build/, 'the honest state of an update');
  assert.match(
    tab,
    /run Install \/ update DLSS 5 after updating Wan2GP|update Wan2GP, then run Install \/ update DLSS 5/,
    'the probe must say the installer still has to run',
  );
  assert.match(tab, /never downloads a build DeepBeepMeep\/Wan2GP has not pinned/);
  assert.doesNotMatch(tab, /installDlss5\(/, 'the probe must not trigger an install');
  assert.doesNotMatch(
    tab,
    /update Wan2GP alone will install it/,
    'updating Wan2GP alone does not update DLSS 5',
  );
});

test('an unreachable upstream is reported honestly, not as "current"', () => {
  assert.match(tab, /Couldn't read DeepBeepMeep\/Wan2GP's installer script/)
  assert.match(tab, /upstream check — unreachable|couldn't read deepbeepmeep\/Wan2GP/)
  assert.match(tab, /nothing was checked or changed/)
})

test('upstream-update notes appear only after the user asks for them', () => {
  // A healthy, installed panel must not decorate itself with an upstream note
  // on launch: that is what made correct rows look wrong. The startup probe
  // still refreshes the data, it just does not paint it.
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

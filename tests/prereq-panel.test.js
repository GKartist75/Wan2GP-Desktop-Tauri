/**
 * prereq-panel.test.js — installer's Prerequisites panel wiring.
 *
 * Each tool needs install/reinstall, a re-check and an explicit version
 * readout; the panel must never provision anything just by rendering, and a
 * Reinstall must be confirmed before it replaces the current copy.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')

const ROOT = path.join(__dirname, '..')
const HTML = fs.readFileSync(path.join(ROOT, 'src', 'index.html'), 'utf8')
const TAB = fs.readFileSync(path.join(ROOT, 'src', 'installer-tab.js'), 'utf8')
const W2GP = fs.readFileSync(path.join(ROOT, 'src', 'w2gp.js'), 'utf8')

test('panel container and check-all control exist on the installer screen', () => {
  assert.match(HTML, /id="prereqToolsRows"/)
  assert.match(HTML, /id="prereqToolsRefresh"/)
  // Four rows are rendered from the tool table, one per prerequisite.
  const list = TAB.match(/const PREREQ_TOOLS = \[[\s\S]*?\]/)
  assert.ok(list, 'tool table missing')
  for (const tool of ['git', 'uv', 'python', 'conda']) {
    assert.ok(
      list[0].includes(`tool: "${tool}"`),
      `panel has no row spec for ${tool}`,
    )
  }
})

test('every row offers install, check and version actions', () => {
  assert.match(TAB, /mk\("install", "Install"\)/)
  assert.match(TAB, /mk\("check", "Check"\)/)
  assert.match(TAB, /mk\("version", "Version"\)/)
})

test('reinstall is confirmed and forces the backend install', () => {
  const fn = TAB.match(/async function prereqInstall\([\s\S]*?\n\}/)
  assert.ok(fn, 'prereqInstall missing')
  assert.match(fn[0], /askConfirm\(/)
  assert.match(fn[0], /installPrerequisite\(spec\.tool, force\)/)
  assert.match(fn[0], /btn\.dataset\.force === "1"/)
  // Two winget runs at once would fight over the same machine state.
  assert.match(fn[0], /_prereqInstalling/)
  assert.match(fn[0], /data-act="install"/)
})

test('version action reports version and path, and refuses when missing', () => {
  const fn = TAB.match(/\[data-act="version"\]'\)\.onclick = \(\) => \{[\s\S]*?\n  \};/)
  assert.ok(fn, 'version handler missing')
  assert.match(fn[0], /r\.version/)
  assert.match(fn[0], /r\.path/)
  assert.match(fn[0], /is not installed/)
})

test('check refreshes the read-only report (no installs)', () => {
  const fn = TAB.match(/async function refreshPrereqTools\([\s\S]*?\n\}/)
  assert.ok(fn, 'refreshPrereqTools missing')
  assert.match(fn[0], /prerequisiteReport\(\)/)
  assert.ok(
    !/installPrerequisite/.test(fn[0]),
    'refresh must not trigger installs',
  )
})

test('panel refreshes on installer show', () => {
  assert.match(TAB, /show\("installer"\);\s*\n\s*refreshPrereqTools\(\)/)
})

test('bindings forward the force flag', () => {
  assert.match(W2GP, /prerequisiteReport: \(\) => call\("prerequisite_report"\)/)
  assert.match(W2GP, /installPrerequisite: \(tool, force\) =>/)
  assert.match(W2GP, /force: !!force/)
})

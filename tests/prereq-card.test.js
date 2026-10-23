/**
 * prereq-card.test.js — missing-tool card regressions.
 *
 * The gate and the installer card used different readiness probes, so a
 * python.org install (py.exe only, no PATH alias) was "missing" to the gate
 * and "installed" to the card: Download -> success -> same card, forever.
 * The card also had no way to re-probe without a launcher restart, and its
 * venv copy claimed Python was "bundled" when it is not.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')

const ROOT = path.join(__dirname, '..')
const HTML = fs.readFileSync(path.join(ROOT, 'src', 'index.html'), 'utf8')
const TAB = fs.readFileSync(path.join(ROOT, 'src', 'installer-tab.js'), 'utf8')

test('card offers Recheck and it resumes the install', () => {
  assert.match(HTML, /id="prereqRecheckBtn"/)
  const block = TAB.match(
    /recheckBtn\.onclick = async function[\s\S]*?\n      \};/,
  )
  assert.ok(block, 'recheck handler missing')
  assert.match(block[0], /await commandReady\(tool\)/)
  assert.match(block[0], /startInstall\(\)/)
})

test('gate and card share one readiness helper', () => {
  for (const cmd of ['git', 'python', 'uv', 'conda']) {
    assert.ok(
      TAB.includes(`commandReady("${cmd}")`),
      `gate does not use commandReady for ${cmd}`,
    )
  }
  assert.ok(!/const hasCmd/.test(TAB), 'old private gate helper still present')
  // check_command returns {cmd, found}; an object alone is always truthy.
  assert.match(TAB, /r && r\.found/)
})

test('venv is not described as bundled with Python', () => {
  assert.ok(!/bundled with Python/i.test(HTML))
  const venv = HTML.match(/<button[^>]*data-env="venv"[\s\S]*?<\/button>/i)
  assert.ok(venv, 'venv button missing')
  assert.match(venv[0], /profile Python/i)
})

test('uv copy no longer promises a PowerShell-only install', () => {
  assert.ok(!/install it via PowerShell/i.test(TAB))
})

test('a not-yet-visible install points at Recheck, not restart-only', () => {
  assert.match(TAB, /Press Recheck/)
})

test('recheck hides the card before resuming the install', () => {
  const block = TAB.match(
    /recheckBtn\.onclick = async function[\s\S]*?\n      \};/,
  )
  assert.ok(block, 'recheck handler missing')
  const hide = block[0].indexOf('classList.add("hidden")')
  const resume = block[0].indexOf('startInstall()')
  assert.ok(hide >= 0 && resume > hide, 'card must hide before resuming')
})

test('download handler trusts the backend ready flag and falls back to Recheck', () => {
  const block = TAB.match(
    /prereqDownloadBtn"\)\.onclick = async function[\s\S]*?\n    \};/,
  )
  assert.ok(block, 'download handler missing')
  // ready:true means the tool is already visible -> continue automatically.
  assert.match(block[0], /r && r\.success/)
  assert.match(block[0], /r\.ready/)
  const ready = block[0].indexOf('r.ready')
  const resume = block[0].indexOf('startInstall()')
  assert.ok(resume > ready, 'auto-continue must be behind the ready flag')
  // ready:false keeps the card and points at Recheck.
  assert.match(block[0], /Press Recheck/)
})

test('python gate copy follows the profile instead of hardcoding 3.11', () => {
  assert.ok(
    !/Python 3\.11 is required/i.test(TAB),
    'legacy hardcoded 3.11 gate copy returned',
  )
  assert.match(TAB, /Python 3\.10–3\.12 is required for venv installs/)
})

test('manual button opens the vendor download page', () => {
  const block = TAB.match(
    /prereqManualBtn"\)\.onclick = \(\) => \{[\s\S]*?\n    \};/,
  )
  assert.ok(block, 'manual handler missing')
  assert.match(block[0], /openExternal\(url\)/)
})

test('uv is described as auto-installed, not a separate install', () => {
  assert.ok(!/requires install/i.test(HTML))
  const uv = HTML.match(/<button[^>]*data-env="uv"[\s\S]*?<\/button>/i)
  assert.ok(uv, 'uv button missing')
  assert.match(uv[0], /auto-installed/i)
})

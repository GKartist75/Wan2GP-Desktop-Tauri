/**
 * msvc-install.test.js — the Troubleshooting MSVC offer is wired end to end.
 *
 * Structural guards (theme-persist.test.js style): the button/row/status ids
 * exist in index.html, the w2gp bridge maps both commands, and the tab script
 * wires both listeners plus the off-Windows row hide. Behavioural coverage of
 * the backend (official URL, workload args, elevated fire-and-forget) lives in
 * troubleshoot.rs's own unit tests — this file only pins the renderer side.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')

const SRC = path.join(__dirname, '..', 'src')
const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
const bridge = fs.readFileSync(path.join(SRC, 'w2gp.js'), 'utf8')
const tab = fs.readFileSync(path.join(SRC, 'troubleshoot-tab.js'), 'utf8')

test('the Triton section carries the MSVC row, buttons and status', () => {
  for (const id of ['tsMsvcRow', 'tsMsvcInstallBtn', 'tsMsvcRecheckBtn', 'tsMsvcStatus']) {
    assert.match(html, new RegExp(`id="${id}"`), `${id} missing from index.html`)
  }
  assert.match(html, /Install MSVC Build Tools/, 'the button must name what it installs')
})

test('the w2gp bridge maps both MSVC commands', () => {
  assert.match(bridge, /tsMsvcStatus:\s*\(\)\s*=>\s*call\("troubleshoot_msvc_status"\)/)
  assert.match(bridge, /tsMsvcInstall:\s*\(\)\s*=>\s*call\("troubleshoot_msvc_install"\)/)
})

test('the tab wires install + recheck and hides the row off-Windows', () => {
  assert.match(tab, /\$\("tsMsvcInstallBtn"\)\?\.addEventListener\("click"/)
  assert.match(tab, /\$\("tsMsvcRecheckBtn"\)\?\.addEventListener\("click"/)
  assert.match(tab, /tsMsvcStatus\(\)/, 'install flow must re-check status first (already-installed short-circuit)')
  assert.match(tab, /confirmDialog\(\{/, 'a multi-GB admin install needs an explicit confirm')
  assert.match(tab, /finish it, then press Recheck/, 'fire-and-forget must tell the user what happens next')
  assert.match(tab, /platform !== "win32"/, 'no MSVC outside Windows')
  assert.match(tab, /tsMsvcRow/, 'the hide targets the row')
})

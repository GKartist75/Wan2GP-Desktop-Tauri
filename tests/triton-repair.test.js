/**
 * triton-repair.test.js — the Troubleshooting Triton repair is wired end to end.
 *
 * Repair = force-reinstall the exact installed build (--no-deps, torch
 * untouched) + clear both JIT cache homes + verify the import. For damaged
 * installs, not AV verdicts. Structural guards: button, bridge, confirm flow.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')

const SRC = path.join(__dirname, '..', 'src')
const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
const bridge = fs.readFileSync(path.join(SRC, 'w2gp.js'), 'utf8')
const tab = fs.readFileSync(path.join(SRC, 'troubleshoot-tab.js'), 'utf8')

test('the Triton section carries the repair button', () => {
  assert.match(html, /id="tsTritonRepairBtn"/)
  assert.match(html, /Repair wheel/)
})

test('the w2gp bridge maps the repair command with optional version', () => {
  assert.match(bridge, /tsTritonRepair:\s*\(v\)\s*=>\s*call\("troubleshoot_triton_repair", \{ version: v \?\? null \}\)/)
})

test('the rollback button pins the previous known-good build', () => {
  assert.match(html, /id="tsTritonRollbackBtn"/)
  assert.match(tab, /TRITON_ROLLBACK_VERSION = "3\.6\.0\.post24"/, 'one UI default to update when the line moves')
  assert.match(tab, /tsTritonRepair\(TRITON_ROLLBACK_VERSION\)/)
  assert.match(tab, /floats forward again/, 'must warn the rollback does not pin')
})
test('the tab confirms, invokes, and reports import state', () => {
  assert.match(tab, /\$\("tsTritonRepairBtn"\)\?\.addEventListener\("click"/)
  assert.match(tab, /confirmDialog\(\{/, 'a minute-long env reinstall needs an explicit confirm')
  assert.match(tab, /tsTritonRepair\(\)/, 'repair must go through the bridge')
  assert.match(tab, /importVersion/, 'the import check result must reach the status line')
})

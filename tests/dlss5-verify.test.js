/**
 * dlss5-verify.test.js — the DLSS 5 SHA column has to be earned.
 *
 * The card printed `✓ SHA 6EB209E764F3…` for every file the backend could stat,
 * but `dlss5_status` only checks existence: the ✓ and the hash beside it were an
 * expectation dressed up as a verification — while the wheel rows on other panels
 * print a hash the launcher actually computed and matched.
 *
 * Fix under test: a row states what is true. Installed-but-unhashed says
 * "expected" and points at the button; a tick needs a real comparison — the
 * installer's own verified line, or Check compatibility, which hashes every file
 * against DLSS5_FILES (dlss5_verify_files). A mismatch names both digests.
 *
 * Like mem-seed.test.js, the real body runs in a vm; the rest are structural
 * guards that the bridge, the command and its registration exist.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')

const ROOT = path.join(__dirname, '..')
const SRC = path.join(ROOT, 'src')
const tab = fs.readFileSync(path.join(SRC, 'dlss5-tab.js'), 'utf8')
const bridge = fs.readFileSync(path.join(SRC, 'w2gp.js'), 'utf8')
const rust = fs.readFileSync(path.join(ROOT, 'src-tauri', 'src', 'install.rs'), 'utf8')
const lib = fs.readFileSync(path.join(ROOT, 'src-tauri', 'src', 'lib.rs'), 'utf8')

/** pull one top-level `function name(...) {...}` body (column-0 close) */
function extractFn(name) {
  const m = tab.match(
    new RegExp(`(?:async\\s+)?function\\s+${name}\\s*\\([^)]*\\)\\s*\\{[\\s\\S]*?\\n\\}`),
  )
  assert.ok(m, `${name} not found`)
  return m[0]
}

const ctx = vm.createContext({})
vm.runInContext(`${extractFn('dlss5RowVerdict')}; globalThis.verdict = dlss5RowVerdict`, ctx)
const verdict = ctx.verdict

const SHA = '6EB209E764F3' + '0'.repeat(52) // 64 hex chars, like a real pin
const FILE = { id: 'dlss/dlls/nvngx_dlss.dll', version: 'DLSS Super Resolution 310.8.0', sha: SHA }

test('an installed file that was never hashed claims no match', () => {
  const v = verdict({ ...FILE, installed: true }, undefined, undefined)
  assert.equal(v.tone, 'warn', 'unproven is not the same as fine')
  assert.match(v.tail, /expected/, 'it must say the hash is an expectation')
  assert.doesNotMatch(v.tail, /✓/, 'no tick without a comparison')
  assert.match(v.tail, /Check compatibility/, 'and it must say how to settle it')
})

test('only a real hash comparison earns the tick', () => {
  const ok = verdict({ ...FILE, installed: true }, undefined, { state: 'match', got: SHA })
  assert.equal(ok.tone, 'ok')
  assert.match(ok.tail, /✓ SHA /)
  assert.match(ok.tail, /matches what Wan2GP pins/)

  // Mid-install, the installer's own verified line is a real comparison too.
  const live = verdict({ ...FILE, installed: true }, 'verified', undefined)
  assert.equal(live.tone, 'ok')
  assert.match(live.tail, /verified by the installer/)
})

test('a mismatch names what was expected and what the bytes hash to', () => {
  const got = 'A'.repeat(64)
  const v = verdict({ ...FILE, installed: true }, undefined, { state: 'mismatch', got })
  assert.equal(v.tone, 'bad')
  assert.match(v.tail, /expected/)
  assert.match(v.tail, new RegExp(SHA.slice(0, 12)), 'the pin it expected')
  assert.match(v.tail, /A{12}/, 'and the digest it found')
})

test('missing, unreadable and never-installed are all honest failures', () => {
  for (const [state, re] of [
    ['missing', /file missing/],
    ['unreadable', /could not be read/],
  ]) {
    const v = verdict({ ...FILE, installed: true }, undefined, { state, got: '' })
    assert.equal(v.tone, 'bad', state)
    assert.match(v.tail, re)
  }
  const absent = verdict({ ...FILE, installed: false }, undefined, undefined)
  assert.equal(absent.tone, 'bad')
  assert.match(absent.tail, /not installed/)
})

test('digests are truncated, never shown as a full 64-char wall', () => {
  const v = verdict({ ...FILE, installed: true }, undefined, { state: 'match', got: SHA })
  assert.match(v.tail, new RegExp(`${SHA.slice(0, 12)}…`))
  assert.doesNotMatch(v.tail, new RegExp(SHA), 'the full pin must not be printed')
})

test('the verifier is wired: bridge, command, registration, and a caller', () => {
  assert.match(bridge, /dlss5VerifyFiles:\s*\(\)\s*=>\s*call\("dlss5_verify_files"\)/)
  assert.match(rust, /pub async fn dlss5_verify_files\(\)/)
  assert.match(lib, /install::dlss5_verify_files,/)
  assert.match(
    tab,
    /dlss5CheckBtn[\s\S]{0,12000}dlss5VerifyFiles\(\)/,
    'Check compatibility is the button that pays for the hashing',
  )
})

test('the verifier compares against the shipped manifest, not a copy of it', () => {
  assert.match(
    rust,
    /fn dlss5_verify_files_blocking[\s\S]{0,900}DLSS5_FILES\.iter\(\)/,
    'one home for the pins',
  )
  assert.match(rust, /fn dlss_file_state\(/, 'the verdict classifier is pure and tested')
  assert.match(rust, /fn state_classifies_existence_and_hash\(/)
  assert.match(rust, /fn verify_reports_mismatch_and_missing_without_touching_files\(/)
})

test('a fresh status read invalidates stale verdicts', () => {
  assert.match(
    tab,
    /async function refreshDlss5\(\)[\s\S]{0,1200}_dlss5Verify = \{\}/,
    'verdicts describe the bytes as they were; a new file set makes them stale',
  )
})

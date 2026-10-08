/**
 * console-newlines.test.js — each launcher message must land on its OWN line.
 *
 * The bug, from a user screenshot of #termBody:
 *   "[*] Wan2GP install found — loading dashboard…[*] Launcher v0.9.2 ready —
 *    …[*] Hardware: ? · ? RAM · ? (?)"
 * Three messages, one line.
 *
 * `appendLog` implements STREAM semantics — a chunk with no newline continues
 * the current line — which is right for a child process's stdout. But the
 * launcher's own messages are complete lines, and 68 of them were passed with
 * no terminator (only 2 ended in \n), so they concatenated into one endless
 * `lastLine`. Not a CSS problem: `.term-body` already sets `white-space:
 * pre-wrap` and the browser honours it (verified — three `\n` produce three
 * line boxes). The text simply had no newlines in it.
 *
 * The fix keys off the existing `forward` flag: the ONLY `forward === false`
 * callers are the two stream listeners in init-tab.js (setup-output,
 * launch-log), so they keep continuation semantics and everything else
 * terminates the line it just wrote.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')

const SRC = path.join(__dirname, '..', 'src')
const appJs = fs.readFileSync(path.join(SRC, 'app.js'), 'utf8')
const termJs = fs.readFileSync(path.join(SRC, 'term.js'), 'utf8')
const initTab = fs.readFileSync(path.join(SRC, 'init-tab.js'), 'utf8')
const css = fs.readFileSync(path.join(SRC, 'style.css'), 'utf8')

function extract(file, name) {
  const m = file.match(
    new RegExp(`(?:async\\s+)?function\\s+${name}\\s*\\([^)]*\\)\\s*\\{[\\s\\S]*?\\n\\}`),
  )
  assert.ok(m, `${name} not found in ${file}`)
  return m[0]
}

/** Run the real appendLog against a plain buffer. */
function harness() {
  const ctx = {
    console,
    window: { w2gp: { mirrorConsole() {} } },
    requestAnimationFrame() {},
  }
  ctx.window.window = ctx.window
  vm.createContext(ctx)
  vm.runInContext(
    [
      'const logBuffer = [];',
      'const MAX_LOG = 5000;',
      'let lastLine = ""; let _carriageReturn = false;',
      'function scheduleTerminalRender() {}',
      extract(appJs, 'progressKey'),
      extract(appJs, 'appendLog'),
      'this.lines = () => logBuffer.slice();',
      'this.pending = () => lastLine;',
      'this.reset = () => { logBuffer.length = 0; lastLine = ""; _carriageReturn = false; };',
    ].join('\n'),
    ctx,
    { timeout: 5000 },
  )
  return ctx
}

test('consecutive launcher messages each get their own line', () => {
  // Exactly the three messages from the reported screenshot.
  const h = harness()
  h.appendLog('[*] Wan2GP install found — loading dashboard…')
  h.appendLog('[*] Launcher v0.9.2 ready — dashboard live.')
  h.appendLog('[*] Hardware: RTX 4070 · 32 GB RAM')
  assert.deepEqual(
    h.lines(),
    [
      '[*] Wan2GP install found — loading dashboard…',
      '[*] Launcher v0.9.2 ready — dashboard live.',
      '[*] Hardware: RTX 4070 · 32 GB RAM',
    ],
    'three messages must be three lines, not one run-on row',
  )
  assert.equal(h.pending(), '', 'nothing may be left hanging as an unfinished line')
})

test('a launcher message containing newlines still splits', () => {
  const h = harness()
  h.appendLog('[*] first\n[*] second\n')
  assert.deepEqual(h.lines(), ['[*] first', '[*] second'])
})

test('process output still continues a partial line across chunks', () => {
  // The reason the fix cannot simply append "\n" to everything: a child's
  // stdout arrives in arbitrary chunks, and splitting on every one would tear
  // long lines in half. `forward === false` is how those callers identify
  // themselves, and it must keep the old continuation behaviour.
  const h = harness()
  h.appendLog('Downloading ', false)
  h.appendLog('model.safetensors', false)
  h.appendLog('\n', false)
  assert.deepEqual(h.lines(), ['Downloading model.safetensors'])
})

test('carriage-return progress still overwrites in place', () => {
  const h = harness()
  h.appendLog('50%|####| 5/10 [00:05<00:05, 1.0it/s]\r', false)
  h.appendLog('90%|####| 9/10 [00:09<00:01, 1.0it/s]\n', false)
  assert.deepEqual(h.lines(), ['90%|####| 9/10 [00:09<00:01, 1.0it/s]'])
})

test('both stream listeners still pass forward === false', () => {
  // The fix is keyed off this flag, so a stream caller that lost its `false`
  // would start getting line-terminating behaviour and tear its output in half.
  assert.match(
    initTab,
    /onSetupOutput\([\s\S]{0,200}?,\s*\n\s*false,\s*\n\s*\)/,
    'setup-output must stay a stream (false = chunk, do not terminate)',
  )
  assert.match(
    initTab,
    /onLaunchLog\([\s\S]{0,300}?appendLog\(clean, false\)/,
    'launch-log must stay a stream',
  )
  assert.doesNotMatch(
    appJs,
    /appendLog\(\s*[^;]{0,200}?,\s*false\s*\)/,
    'app.js calls are launcher messages, never stream chunks',
  )
})

test('the mirrored copy in the separate console window terminates its lines too', () => {
  // console-mirror carries the same un-terminated launcher messages, so the
  // floating console glued them together exactly like the dashboard did.
  assert.match(
    termJs,
    /onConsoleMirror\([\s\S]{0,300}appendToBuf\(strip\(String\(t \?\? ""\)\) \+ "\\n"\)/,
    'term.js must terminate mirrored launcher lines',
  )
})

test('the console body already honours newlines — do not "fix" it in CSS', () => {
  // Worth pinning: the symptom looks like a CSS problem and is not.
  const rules = [...css.matchAll(/\.term-body\s*\{[^}]*\}/g)].map((m) => m[0])
  assert.ok(rules.length, '.term-body rule must exist')
  assert.ok(
    rules.some((r) => /white-space:\s*pre-wrap/.test(r)),
    'newlines must render as newlines',
  )
})
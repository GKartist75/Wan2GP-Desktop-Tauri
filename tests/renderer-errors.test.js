/**
 * renderer-errors.test.js — a renderer error must reach the console, once.
 *
 * window.onerror / unhandledrejection painted #splashError and nothing else. That
 * is enough to explain a blank startup screen, but once boot is past the splash is
 * gone — so a failure after boot (a handler throwing, a rejected promise) looked
 * like "nothing happened", with the trace in a hidden element nobody sees.
 *
 * Fix under test: the same handler also writes a line to the console, de-duplicated
 * so an error thrown every frame can't flood the log, and it degrades quietly when
 * appendLog isn't there yet (theme-tab.js loads before app.js on some paths).
 *
 * The real body runs in a vm, like theme-persist.test.js does for this file.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')

const SRC = path.join(__dirname, '..', 'src')
const themeTab = fs.readFileSync(path.join(SRC, 'theme-tab.js'), 'utf8')

/** pull one top-level `function name(...) {...}` body out of a renderer file */
function extract(name) {
  const m = themeTab.match(
    new RegExp(`(?:async\\s+)?function\\s+${name}\\s*\\([^)]*\\)\\s*\\{[\\s\\S]*?\\n\\}`),
  )
  assert.ok(m, `${name} not found in theme-tab.js`)
  return m[0]
}

/** fake $/appendLog: the splash element and a log sink */
function harness({ withAppendLog = true } = {}) {
  const logs = []
  const el = {
    textContent: '',
    hidden: true,
    classList: { remove: () => { el.hidden = false } },
  }
  const sandbox = vm.createContext({
    $: () => el,
    _errSeen: new Set(),
    __logs: logs,
    __el: el,
  })
  if (withAppendLog) sandbox.appendLog = (t) => logs.push(t)
  vm.runInContext(`${extract('reportRendererError')}; globalThis.report = reportRendererError`, sandbox)
  return { report: sandbox.report, el, logs }
}

test('an error is shown on the splash AND written to the console', () => {
  const { report, el, logs } = harness()
  report('renderer error', { stack: 'boom\n  at thing (a.js:1)' })
  assert.equal(el.hidden, false, 'the splash still explains a blank screen')
  assert.match(el.textContent, /at thing/, 'the full stack stays on the splash')
  assert.equal(logs.length, 1, 'and it lands in the console history')
  assert.match(logs[0], /\[!\] renderer error: /)
  assert.match(logs[0], /^\[!\] renderer error: boom$/, 'one line, not a wall of stack')
})

test('the same failure logged every frame is written once', () => {
  const { report, el, logs } = harness()
  const err = new Error('same')
  for (let i = 0; i < 5; i++) report('unhandled promise rejection', err)
  assert.equal(logs.length, 1, 'a repeating error must not flood the console')
  assert.equal(el.hidden, false, 'the splash still reflects the latest one')
  // A different failure still gets through.
  report('unhandled promise rejection', new Error('other'))
  assert.equal(logs.length, 2)
})

test('a missing or unhelpful detail still reports something', () => {
  const { report, logs } = harness()
  report('renderer error', undefined)
  assert.match(logs[0], /unknown error/)
})

test('it degrades quietly when appendLog is not loaded yet', () => {
  const { report, el } = harness({ withAppendLog: false })
  assert.doesNotThrow(() => report('renderer error', new Error('early')))
  assert.equal(el.hidden, false, 'the splash path still works')
})

test('both error channels are actually wired to it', () => {
  assert.match(themeTab, /addEventListener\("error"[\s\S]{0,200}reportRendererError\(/)
  assert.match(
    themeTab,
    /addEventListener\("unhandledrejection"[\s\S]{0,200}reportRendererError\(/,
  )
})

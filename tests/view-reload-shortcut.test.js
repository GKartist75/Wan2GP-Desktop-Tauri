/**
 * view-reload-shortcut.test.js — Ctrl+R / F5 must reload the embedded Wan2GP
 * page, and only there.
 *
 * Reported symptom: a video playing in the desktop view while a queued ACE
 * audio job finishes behind it leaves the Gradio page unresponsive, and the
 * only way back was the topbar reload button. The keyboard recovery must go
 * through the same code path as that button (reloadWan2gpView), must never
 * fire while the launcher's own screens are up, and must not disturb the
 * neighbouring view shortcuts (Escape / Ctrl+W).
 *
 * Same approach as console-resize.test.js: the renderer has no module loader,
 * so the real shortcuts-tab.js runs in a vm against a fake document.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')

const SRC = path.join(__dirname, '..', 'src')
const shortcutsJs = fs.readFileSync(path.join(SRC, 'shortcuts-tab.js'), 'utf8')

/** Boot shortcuts-tab.js with the globals it reads at load time. */
function boot({ dashDisplay = 'flex' } = {}) {
  const calls = []
  const keyListeners = []
  const dashBody = { style: { display: dashDisplay } }

  const el = (id) => ({
    id,
    style: {},
    textContent: '',
    classList: { contains: () => false, remove() {}, add() {} },
    addEventListener() {},
    setAttribute() {},
    appendChild() {},
    remove() {},
  })

  const document = {
    documentElement: { getAttribute: () => 'dark' },
    body: { appendChild() {} },
    addEventListener(type, fn) {
      if (type === 'keydown') keyListeners.push(fn)
    },
    createElement: () => el('created'),
    querySelector: () => null,
    querySelectorAll: () => [],
    getElementById: (id) => (id === 'dashBody' ? dashBody : el(id)),
  }

  const context = {
    console,
    setTimeout,
    clearTimeout,
    document,
    window: { w2gp: { checkUpdate: async () => ({}) } },
    $: (id) => (id === 'dashBody' ? dashBody : el(id)),
    closeSettings: () => calls.push('closeSettings'),
    closeWebview: () => calls.push('closeWebview'),
    toggleFloatingTerm: () => calls.push('toggleFloatingTerm'),
    reloadWan2gpView: () => calls.push('reload'),
  }
  context.globalThis = context
  vm.createContext(context)
  vm.runInContext(shortcutsJs, context, { filename: 'shortcuts-tab.js' })
  assert.equal(keyListeners.length, 1, 'exactly one keydown listener')

  const press = (key, mods = {}) => {
    let prevented = false
    keyListeners[0]({
      key,
      target: { tagName: 'BODY' },
      preventDefault() {
        prevented = true
      },
      ...mods,
    })
    return prevented
  }

  return { calls, press, dashBody }
}

test('Ctrl+R and F5 reload the open view', () => {
  const { calls, press } = boot({ dashDisplay: 'none' })
  assert.equal(press('r', { ctrlKey: true }), true, 'Ctrl+R is handled')
  assert.equal(press('R', { ctrlKey: true }), true, 'Ctrl+Shift+R is handled')
  assert.equal(press('F5'), true, 'F5 is handled')
  assert.deepEqual(calls, ['reload', 'reload', 'reload'])
})

test('the shortcut leaves the launcher screens alone', () => {
  const { calls, press } = boot({ dashDisplay: 'flex' })
  assert.equal(press('r', { ctrlKey: true }), false, 'not swallowed on the dashboard')
  assert.equal(press('F5'), false, 'not swallowed on the dashboard')
  assert.deepEqual(calls, [], 'nothing reloaded')
})

test('view shortcuts keep their order and behaviour', () => {
  const { calls, press } = boot({ dashDisplay: 'none' })
  press('Escape')
  press('w', { ctrlKey: true })
  assert.deepEqual(calls, ['closeWebview', 'closeWebview'])
})

test('the topbar button and the keyboard share one reload path', () => {
  const appJs = fs.readFileSync(path.join(SRC, 'app.js'), 'utf8')
  assert.match(
    appJs,
    /function reloadWan2gpView\(\)[\s\S]{0,400}wvReloadBtn"\)\.addEventListener\("click", reloadWan2gpView\)/,
    'the reload button must call reloadWan2gpView',
  )
  assert.match(
    shortcutsJs,
    /typeof reloadWan2gpView === "function"/,
    'the keyboard path must reuse it, not re-implement the reload',
  )
})

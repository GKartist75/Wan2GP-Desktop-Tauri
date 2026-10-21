/**
 * native-dialogs.test.js — confirmations must actually be able to say "no".
 *
 * tauri-plugin-dialog 2.7.x registers an init script (init-iife.js) that
 * replaces window.confirm / window.alert with *async* shims invoking
 * `plugin:dialog|confirm` and `plugin:dialog|message`. Two things are wrong
 * with that in this app:
 *
 *   1. `plugin:dialog|confirm` is not a command the plugin registers (its
 *      handler list is open/save/message), and our capability grants no
 *      `dialog:` permission at all — so the invoke rejects with
 *      "Command plugin:dialog|confirm not allowed by ACL", which surfaced in
 *      the console as an unhandled promise rejection.
 *   2. The shim is async, so `confirm()` returns a Promise. `if (!confirm(…))`
 *      is then `if (!Promise)` — always false — so every guard fell through and
 *      the destructive action ran without asking. (A WGPKernels reinstall ran
 *      with no confirmation at all, which is how this was found.)
 *
 * The launcher therefore uses its own `askConfirm` / `notifyUser` helpers,
 * which call the Rust `confirm_dialog` command (native, ACL-free).
 *
 * These tests fail if anyone reintroduces a bare confirm/alert, or if the
 * helpers stop going through confirm_dialog.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')

const SRC = path.join(__dirname, '..', 'src')
const appJs = fs.readFileSync(path.join(SRC, 'app.js'), 'utf8')

/** every renderer file, in load order, as [name, source] */
const files = fs
  .readdirSync(SRC)
  .filter((f) => f.endsWith('.js'))
  .map((f) => [f, fs.readFileSync(path.join(SRC, f), 'utf8')])

/** strip comments and string literals so prose about confirm() can't trip the scan */
function codeOnly(src) {
  return src
    .replace(/\/\*[\s\S]*?\*\//g, ' ')
    .replace(/(^|[^:\\])\/\/[^\n]*/g, '$1 ')
    .replace(/'(?:[^'\\\n]|\\.)*'/g, "''")
    .replace(/"(?:[^"\\\n]|\\.)*"/g, '""')
    .replace(/`(?:[^`\\]|\\.)*`/g, '``')
}

test('no renderer file calls the plugin-shimmed confirm() or alert()', () => {
  const offenders = []
  for (const [name, src] of files) {
    const code = codeOnly(src)
    // `askConfirm(` / `notifyUser(` are ours; a bare call is the shim.
    const bare = [
      ...code.matchAll(/(?<![\w.$])(confirm|alert)\s*\(/g),
      ...code.matchAll(/window\.(confirm|alert)\s*\(/g),
    ]
    for (const m of bare) offenders.push(`${name}: ${m[0].trim()}`)
  }
  assert.deepStrictEqual(
    offenders,
    [],
    'window.confirm/window.alert are replaced by an async shim that the ACL ' +
      'denies and whose truthiness silently skips the guard — use askConfirm() ' +
      'or notifyUser(). Offenders:\n  ' + offenders.join('\n  '),
  )
})

/** run the two helpers against a fake w2gp bridge, returning what they called */
function runHelpers(bridge) {
  const start = appJs.indexOf('async function askConfirm')
  assert.ok(start > -1, 'askConfirm not found in app.js')
  const end = appJs.indexOf('\n}', appJs.indexOf('function notifyUser'))
  assert.ok(end > start, 'notifyUser not found in app.js')
  const body = appJs.slice(start, end + 2)

  const sandbox = vm.createContext({ window: { w2gp: bridge }, Promise })
  vm.runInContext(body, sandbox)
  return sandbox
}

test('askConfirm returns true only for an ok answer', async () => {
  for (const [reply, expected] of [
    ['ok', true],
    ['cancel', false],
    [undefined, false],
    [null, false],
    ['', false],
    // A raw bridge failure must not read as consent.
    ['error', false],
  ]) {
    const calls = []
    const sb = runHelpers({
      confirmDialog: async (opts) => {
        calls.push(opts)
        return reply
      },
    })
    assert.strictEqual(await sb.askConfirm('Delete it?'), expected, `reply=${reply}`)
    assert.strictEqual(calls.length, 1)
    assert.strictEqual(calls[0].message, 'Delete it?')
    assert.ok(calls[0].title, 'a title is always supplied')
  }
})

test('askConfirm survives a throwing bridge by refusing', async () => {
  const sb = runHelpers({
    confirmDialog: async () => {
      throw new Error('boom')
    },
  })
  // Refusing (rather than throwing into the click handler) is the safe answer:
  // the caller's `if (!(await askConfirm(…))) return` then just stops.
  await assert.rejects(() => sb.askConfirm('Delete it?'), /boom/)
})

test('notifyUser never rejects, even when the bridge throws', async () => {
  const calls = []
  const sb = runHelpers({
    confirmDialog: async (opts) => {
      calls.push(opts)
      throw new Error('boom')
    },
  })
  assert.strictEqual(sb.notifyUser('Install failed'), undefined)
  // let the swallowed rejection settle; an unhandled one would fail the run
  await new Promise((r) => setImmediate(r))
  assert.strictEqual(calls.length, 1)
  assert.strictEqual(calls[0].message, 'Install failed')
  assert.strictEqual(calls[0].title, 'Notice')
})

test('the helpers are defined before the tab files that use them', () => {
  // app.js is loaded at index.html:2180, before every *-tab.js. A helper moved
  // into a later file would be undefined at click time.
  const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
  const order = [...html.matchAll(/<script src="([^"]+)"/g)].map((m) => m[1])
  const appAt = order.indexOf('app.js')
  assert.ok(appAt > -1, 'app.js is loaded')
  const users = files
    .filter(
      ([name, src]) => name !== 'app.js' && /\b(askConfirm|notifyUser)\s*\(/.test(src),
    )
    .map(([name]) => name)
  assert.ok(users.length > 0, 'tab files use the helpers')
  for (const name of users) {
    assert.ok(
      order.indexOf(name) > appAt,
      `${name} uses the helpers but loads before app.js`,
    )
  }
})

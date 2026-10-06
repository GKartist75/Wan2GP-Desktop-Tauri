/**
 * theme-persist.test.js — the theme must survive a relaunch.
 *
 * index.html ships a hardcoded `data-theme="dark"`, so a persisted light theme
 * only survives startup if the boot path ACTIVELY applies the stored value.
 * It used to apply it only when the value was "dark" (`else if (cfg.theme ===
 * "dark")`), which meant light worked until the next launch and then silently
 * reverted. These tests pin the resolved theme, the manual toggle's write, and
 * the boot path that consumes both.
 *
 * The renderer has no module loader, so the real bodies are extracted from
 * src/theme-tab.js and run in a vm — exactly like services.test.js does for
 * the inline normalizePipSpec copy. Text comparison would not catch a
 * behaviour change; running the code does.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')

const SRC = path.join(__dirname, '..', 'src')
const themeTab = fs.readFileSync(path.join(SRC, 'theme-tab.js'), 'utf8')

/** pull one top-level `function name(...) {...}` body out of a renderer file */
function extract(file, name) {
  const m = file.match(
    new RegExp(`(?:async\\s+)?function\\s+${name}\\s*\\([^)]*\\)\\s*\\{[\\s\\S]*?\\n\\}`),
  )
  assert.ok(m, `${name} not found in ${file}`)
  return m[0]
}

/**
 * Run resolveTheme + applyTheme against a fake document, so the assertions are
 * about the attribute the user actually sees.
 */
function harness({ cfg, prefersDark = false } = {}) {
  const attrs = new Map([['data-theme', 'dark']]) // index.html's hardcoded default
  const ls = new Map()
  const ctx = {
    matchMedia: () => ({ matches: prefersDark }),
    document: {
      documentElement: {
        getAttribute: (k) => (attrs.has(k) ? attrs.get(k) : null),
        setAttribute: (k, v) => attrs.set(k, v),
        removeAttribute: (k) => attrs.delete(k),
      },
      querySelectorAll: () => [],
    },
    localStorage: {
      setItem: (k, v) => ls.set(k, String(v)),
      getItem: (k) => (ls.has(k) ? ls.get(k) : null),
    },
    ls, // exposed on the context so the vm can hand it back to the test
    attrs,
  }
  ctx.window = ctx
  vm.createContext(ctx)
  vm.runInContext(
    extract(themeTab, 'resolveTheme') +
      '\n' +
      extract(themeTab, 'systemPrefersDark') +
      '\n' +
      extract(themeTab, 'applyTheme') +
      '\nthis.resolveTheme = resolveTheme; this.systemPrefersDark = systemPrefersDark;' +
      '\nthis.applyTheme = applyTheme; this.boot = (c) => applyTheme(resolveTheme(c, systemPrefersDark()));' +
      '\nthis.attr = () => (attrs.has("data-theme") ? attrs.get("data-theme") : "light");',
    ctx,
    { timeout: 5000 },
  )
  return Object.assign(ctx, { cfg })
}

test('resolveTheme honours the persisted choice when follow-system is off', () => {
  const { resolveTheme } = harness()
  assert.equal(resolveTheme({ theme: 'light' }, true), 'light', 'OS dark must not override a manual light pick')
  assert.equal(resolveTheme({ theme: 'dark' }, false), 'dark', 'OS light must not override a manual dark pick')
  assert.equal(resolveTheme({}, true), 'dark', 'a fresh profile keeps the shipped dark default')
  assert.equal(resolveTheme(null, false), 'dark')
})

test('resolveTheme follows the OS only while follow-system is on', () => {
  const { resolveTheme } = harness()
  const on = { theme: 'light', themeFollowSystem: true }
  assert.equal(resolveTheme(on, true), 'dark')
  assert.equal(resolveTheme(on, false), 'light')
})

test('boot applies the stored light theme over index.html default dark', () => {
  const h = harness({ cfg: { theme: 'light' } })
  h.boot(h.cfg)
  assert.equal(h.attr(), 'light', 'relaunch reverted to dark — the reported bug')
  assert.equal(h.ls.get('w2gp.theme'), 'light', 'term window mirror must be written too')
})

test('boot keeps dark for a dark profile and for a fresh install', () => {
  const dark = harness({ cfg: { theme: 'dark' } })
  dark.boot(dark.cfg)
  assert.equal(dark.attr(), 'dark')

  const fresh = harness({ cfg: {} })
  fresh.boot(fresh.cfg)
  assert.equal(fresh.attr(), 'dark')
})

test('boot follows the OS when the profile asks it to', () => {
  const h = harness({ cfg: { theme: 'light', themeFollowSystem: true }, prefersDark: false })
  h.boot(h.cfg)
  assert.equal(h.attr(), 'light')
})

test('a manual toggle persists its pick and clears follow-system', async () => {
  // The regression had a second face: toggleTheme wrote cfg.theme but left
  // themeFollowSystem on, so the next launch followed the OS and undid the
  // choice the user just made.
  const saved = []
  const checkbox = { checked: true }
  const cfg = { theme: 'dark', themeFollowSystem: true }
  const ctx = {
    window: {
      w2gp: {
        configLoad: async () => cfg,
        configSave: async (c) => saved.push(JSON.parse(JSON.stringify(c))),
      },
    },
    $: (id) => (id === 'followSystemThemeToggle' ? checkbox : null),
    matchMedia: () => ({ matches: true }),
    document: {
      documentElement: {
        getAttribute: () => 'dark',
        setAttribute: () => {},
        removeAttribute: () => {},
      },
      querySelectorAll: () => [],
    },
    localStorage: { setItem: () => {} },
  }
  ctx.window.document = ctx.document
  vm.createContext(ctx)
  vm.runInContext(
    extract(themeTab, 'resolveTheme') +
      '\n' +
      extract(themeTab, 'systemPrefersDark') +
      '\n' +
      extract(themeTab, 'applyTheme') +
      '\n' +
      extract(themeTab, 'toggleTheme') +
      '\nthis.toggleTheme = toggleTheme;',
    ctx,
    { timeout: 5000 },
  )

  await ctx.toggleTheme()
  assert.equal(cfg.theme, 'light', 'toggled away from the dark theme it is on')
  assert.equal(cfg.themeFollowSystem, false, 'manual pick must outrank follow-system')
  assert.equal(saved.length, 1, 'the pick must reach desktop-config.json')
  assert.equal(saved[0].theme, 'light')
  assert.equal(checkbox.checked, false, 'settings checkbox must not keep claiming to follow the OS')

  await ctx.toggleTheme()
  assert.equal(cfg.theme, 'dark', 'second toggle flips back')
  assert.equal(saved[1].theme, 'dark')
})

test('the boot path applies the resolved theme on every launch', () => {
  // Structural guard on the exact line that regressed: the theme was applied
  // only in the "dark" branch, leaving light to index.html's hardcoded dark.
  const init = fs.readFileSync(path.join(SRC, 'init-tab.js'), 'utf8')
  assert.match(
    init,
    /applyTheme\(resolveTheme\(cfg, systemPrefersDark\(\)\)\)/,
    'init-tab.js must apply the resolved theme unconditionally',
  )
  assert.doesNotMatch(
    init,
    /else if \(cfg\.theme === "dark"\)/,
    'the dark-only branch is what made light revert on relaunch',
  )
})

test('the floating terminal reads the mirror the main window writes', () => {
  const term = fs.readFileSync(path.join(SRC, 'term.js'), 'utf8')
  assert.match(themeTab, /localStorage\.setItem\("w2gp\.theme"/, 'applyTheme must mirror the theme for the term window')
  assert.match(term, /localStorage\.getItem\("w2gp\.theme"\)/, 'term.js must read the key applyTheme writes')
  assert.doesNotMatch(term, /getItem\("theme"\)/, 'nothing ever wrote a bare "theme" key — the term window was always dark')
})

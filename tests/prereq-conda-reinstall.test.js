/**
 * prereq-conda-reinstall.test.js — the Conda row must not offer a Reinstall.
 *
 * Miniconda's silent installer refuses ANY non-empty prefix (verified: rc 2,
 * "Directory … is not empty"), so re-downloading cannot replace a live conda
 * base. The row therefore reads "Installed" and stays disabled once conda is
 * ready, while git/uv/python keep their Reinstall (their installers upgrade in
 * place). Runs the real installer-tab.js in a vm against a fake DOM, like
 * view-reload-shortcut.test.js does for the shortcuts.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')

const SRC = path.join(__dirname, '..', 'src')
const GATE = fs.readFileSync(path.join(SRC, 'services', 'platform-gate.js'), 'utf8')
const TAB = fs.readFileSync(path.join(SRC, 'installer-tab.js'), 'utf8')

/** Minimal element with just the surface installer-tab.js touches. */
function makeEl(tag) {
  const el = {
    tagName: String(tag || 'div').toUpperCase(),
    className: '',
    textContent: '',
    title: '',
    disabled: false,
    dataset: {},
    style: {},
    onclick: null,
    children: [],
    parent: null,
    classList: { add() {}, remove() {}, contains: () => false, toggle() {} },
    addEventListener() {},
    setAttribute() {},
    appendChild(c) {
      c.parent = el
      el.children.push(c)
      return c
    },
    append(...cs) {
      cs.forEach((c) => el.appendChild(c))
    },
    remove() {
      if (el.parent) el.parent.children = el.parent.children.filter((x) => x !== el)
    },
    querySelector(sel) {
      for (const c of el.children) {
        if (matches(c, sel)) return c
        const deep = c.querySelector(sel)
        if (deep) return deep
      }
      return null
    },
    querySelectorAll(sel) {
      const out = []
      const walk = (n) => {
        for (const c of n.children) {
          if (matches(c, sel)) out.push(c)
          walk(c)
        }
      }
      walk(el)
      return out
    },
  }
  return el
}

/** `.cls`, `[data-x="v"]`, `.cls[data-x="v"]`, `tag.cls` — no combinator. */
function matches(el, sel) {
  let rest = sel.trim()
  const tag = rest.match(/^[a-zA-Z]+/)
  if (tag) {
    if (el.tagName !== tag[0].toUpperCase()) return false
    rest = rest.slice(tag[0].length)
  }
  for (const m of rest.matchAll(/\.([\w-]+)|\[([\w-]+)(?:="([^"]*)")?\]/g)) {
    if (m[1]) {
      if (!el.className.split(/\s+/).includes(m[1])) return false
    } else if (m[2]) {
      const key = m[2].startsWith('data-')
        ? m[2].slice(5).replace(/-(\w)/g, (_, c) => c.toUpperCase())
        : m[2]
      const has = m[2].startsWith('data-') ? el.dataset[key] : el[m[2]]
      if (m[3] === undefined ? has === undefined : String(has) !== m[3]) return false
    }
  }
  return true
}

/** Boot installer-tab.js with fake DOM + a stubbed report. */
function boot(tools) {
  const byId = new Map()
  const logs = []
  const document = {
    createElement: (t) => makeEl(t),
    getElementById: (id) => byId.get(id) || null,
    querySelector: () => null,
    querySelectorAll: () => [],
    addEventListener() {},
    body: makeEl('body'),
    documentElement: makeEl('html'),
  }
  const $ = (id) => {
    if (!byId.has(id)) {
      const el = makeEl('div')
      el.id = id
      byId.set(id, el)
    }
    return byId.get(id)
  }
  const ctx = {
    console,
    document,
    navigator: { platform: 'Win32', userAgentData: { platform: 'Windows' } },
    $,
    appendLog: (m) => logs.push(String(m)),
    showToast() {},
    errText: (e) => String((e && e.message) || e),
    askConfirm: async () => true,
    setTimeout,
    clearTimeout,
    setInterval,
    clearInterval,
    require,
    module: undefined,
    window: null,
  }
  ctx.window = ctx
  ctx.self = ctx
  ctx.w2gp = {
    prerequisiteReport: async () => ({ tools }),
    checkCommand: async () => ({ found: true }),
    installPrerequisite: async () => ({ success: false, error: 'no bridge' }),
  }
  vm.createContext(ctx)
  vm.runInContext(GATE, ctx, { filename: 'platform-gate.js' })
  vm.runInContext(TAB, ctx, { filename: 'installer-tab.js' })
  ctx.logs = logs
  return ctx
}

const report = (conda) => [
  { tool: 'git', ready: true, version: '2.55.0' },
  { tool: 'uv', ready: true, version: '0.9.1' },
  { tool: 'python', ready: true, version: '3.11.9' },
  { tool: 'conda', ready: conda.ready, version: conda.version, path: conda.path },
]

const rowButton = (ctx, tool) => {
  const row = ctx.$('prereqToolsRows').querySelector('.pt-row[data-tool="' + tool + '"]')
  assert.ok(row, `no ${tool} row`)
  return row.querySelector('[data-act="install"]')
}

test('ready conda shows a disabled "Installed", not a Reinstall', async () => {
  const ctx = boot(report({ ready: true, version: '26.9.4', path: 'C:\\Users\\u\\Miniconda3' }))
  await ctx.refreshPrereqTools()
  const conda = rowButton(ctx, 'conda')
  assert.strictEqual(conda.textContent, 'Installed')
  assert.strictEqual(conda.disabled, true)
  assert.strictEqual(conda.dataset.force, '')
  assert.match(conda.title, /conda update -n base conda/)
})

test('the other tools keep Reinstall (their installers upgrade in place)', async () => {
  const ctx = boot(report({ ready: true, version: '26.9.4', path: 'C:\\Users\\u\\Miniconda3' }))
  await ctx.refreshPrereqTools()
  for (const tool of ['git', 'uv', 'python']) {
    const b = rowButton(ctx, tool)
    assert.strictEqual(b.textContent, 'Reinstall', tool)
    assert.strictEqual(b.disabled, false, tool)
    assert.strictEqual(b.dataset.force, '1', tool)
  }
})

test('missing conda offers Install again (no stuck "Installed" marker)', async () => {
  const ctx = boot(report({ ready: true, version: '26.9.4', path: 'C:\\Users\\u\\Miniconda3' }))
  await ctx.refreshPrereqTools()
  ctx.w2gp.prerequisiteReport = async () => ({
    tools: report({ ready: false }),
  })
  await ctx.refreshPrereqTools()
  const conda = rowButton(ctx, 'conda')
  assert.strictEqual(conda.textContent, 'Install')
  assert.strictEqual(conda.disabled, false)
  assert.strictEqual(conda.dataset.force, '')
})

test('a failed install keeps the whole backend message in the console', async () => {
  // The toast truncates; the conda "already installed" remedy is long and is
  // the only actionable thing on that path, so it must reach the log intact.
  const remedy =
    'Miniconda is already installed (C:\\Users\\u\\Miniconda3\\Scripts\\conda.exe). Update it instead: `conda update -n base conda` in an admin terminal — or run the Miniconda uninstaller, then press Install again.'
  const ctx = boot(report({ ready: true, version: '26.9.4', path: 'C:\\Users\\u\\Miniconda3' }))
  await ctx.refreshPrereqTools()
  ctx.w2gp.installPrerequisite = async () => ({ error: remedy })
  await rowButton(ctx, 'python').onclick()
  const hit = ctx.logs.find((l) => l.includes('conda update -n base conda'))
  assert.ok(hit, 'backend error text missing from the console log: ' + JSON.stringify(ctx.logs))
  assert.match(hit, /^\[!\] Python: /)
})

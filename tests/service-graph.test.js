/**
 * service-graph.test.js — structural guard for the renderer bundle.
 *
 * The renderer loads plain <script> tags and has no module loader, so a file
 * under src/services/ does nothing unless BOTH things are true: index.html
 * lists it, and the file assigns itself to `window`. This test pins that
 * invariant so the bundle cannot silently grow dead weight again.
 *
 * Every file under src/services/ is now either loaded by index.html or listed in
 * FOSSILS below with a reason. Eleven Electron-port fossils were removed once the
 * IPC audit proved the Rust backend (and app.js) own that logic; the allowlist is
 * kept so the NEXT accidental orphan is a test failure rather than silent weight.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')

const SRC = path.join(__dirname, '..', 'src')

/**
 * Never executed, but deliberately kept: normalize-pip-spec is the unit-test
 * twin of the load-bearing inline copy at app.js:7252. The renderer has no
 * module loader, so the inline copy is what runs; this file is what gets tested,
 * and the two bodies must stay identical.
 */
const FOSSILS = new Set(['normalize-pip-spec.js'])

const scriptSources = () => {
  const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
  return [...html.matchAll(/<script[^>]*src="([^"]+)"/g)].map((m) => m[1])
}

const serviceFiles = () =>
  fs
    .readdirSync(path.join(SRC, 'services'))
    .filter((f) => f.endsWith('.js'))
    .sort()

/**
 * Run a renderer script in a browser-shaped sandbox and report the globals it
 * leaves behind. Services have no DOM dependency, so they execute fully; the
 * sandbox aliases window/self/globalThis the way a browser does.
 */
function globalsDefinedBy(src) {
  const sandbox = { console }
  sandbox.window = sandbox
  sandbox.self = sandbox
  sandbox.globalThis = sandbox
  vm.createContext(sandbox)
  vm.runInContext(fs.readFileSync(path.join(SRC, src), 'utf8'), sandbox, { timeout: 5000 })
  const scaffolding = new Set(['console', 'window', 'self', 'globalThis'])
  return Object.keys(sandbox).filter((k) => !scaffolding.has(k))
}

test('every service is either loaded by index.html or a declared fossil', () => {
  const loaded = new Set(scriptSources())
  const orphans = serviceFiles().filter((f) => !loaded.has(`services/${f}`))
  const undeclared = orphans.filter((f) => !FOSSILS.has(f))
  assert.deepEqual(
    undeclared,
    [],
    `new orphan service(s): ${undeclared.join(', ')} — load it in index.html ` +
      `(and give it a window.* export) or add it to FOSSILS with a reason`
  )
})

test('the declared fossil list matches what is actually orphaned', () => {
  // Catches the reverse drift: a fossil that later gets wired in must be
  // removed from the list, so the allowlist cannot rot into a blanket excuse.
  const loaded = new Set(scriptSources())
  const orphans = new Set(serviceFiles().filter((f) => !loaded.has(`services/${f}`)))
  const stale = [...FOSSILS].filter((f) => !orphans.has(f)).sort()
  assert.deepEqual(stale, [], `FOSSILS lists files that are now loaded: ${stale.join(', ')}`)
})

test('a loaded service must define at least one global to be reachable', () => {
  // A <script> tag alone is not enough — without a global the module parses,
  // defines its functions and vanishes. Globals can arrive three ways, so this
  // evaluates each service rather than pattern-matching: a `window.X =` export,
  // a UMD wrapper that falls back to `root.X =` (escape.js), or a bare top-level
  // declaration, which in a classic script IS a global.
  const inert = scriptSources()
    .filter((s) => s.startsWith('services/'))
    .filter((s) => globalsDefinedBy(s).length === 0)
  assert.deepEqual(inert, [], `loaded but inert (defines no global): ${inert.join(', ')}`)
})

test('the renderer has no module loader, so no script may call require()', () => {
  // If this ever starts failing, the bundle grew a real bundler step and the
  // CommonJS fossils above can be imported instead of deleted.
  const loaded = scriptSources().filter((s) => !s.startsWith('services/'))
  const users = loaded.filter((s) => /require\(/.test(fs.readFileSync(path.join(SRC, s), 'utf8')))
  assert.deepEqual(users, [], `renderer scripts must not require(): ${users.join(', ')}`)
})

test('app.js never passes an extracted tab symbol by value at top level', () => {
  // The one real ordering hazard in this codebase: app.js loads BEFORE the
  // *-tab.js files, so app.js cannot reference their functions at load time.
  // Calling one inside a callback is fine; passing one as a bare argument
  // (`addEventListener("click", toggleTheme)`) evaluates the reference
  // immediately and throws ReferenceError. That bug shipped once already.
  const tabFiles = fs
    .readdirSync(SRC)
    .filter((f) => f.endsWith('-tab.js'))
  const names = new Set()
  for (const f of tabFiles) {
    const src = fs.readFileSync(path.join(SRC, f), 'utf8')
    for (const m of src.matchAll(/^(?:async\s+)?function\s+(\w+)/gm)) names.add(m[1])
    for (const m of src.matchAll(/^(?:const|let|var)\s+(\w+)\s*=/gm)) names.add(m[1])
  }
  assert.ok(names.size > 0, 'expected to find symbols in the extracted tabs')

  const app = fs.readFileSync(path.join(SRC, 'app.js'), 'utf8').split('\n')
  const hazards = []
  let depth = 0
  app.forEach((line, i) => {
    if (depth === 0) {
      for (const n of names) {
        // bare reference: name present, not a call (no `(` right after)
        const re = new RegExp('(^|[^\\w.$])' + n + '(?![\\w$(])')
        if (re.test(line)) hazards.push(`app.js:${i + 1} passes ${n} — ${line.trim().slice(0, 70)}`)
      }
    }
    for (const ch of line) {
      if (ch === '{') depth++
      else if (ch === '}') depth--
    }
  })
  assert.deepEqual(hazards, [], 'load-time references into a later script:\n' + hazards.join('\n'))
})

test('every renderer script referenced by index.html exists on disk', () => {
  // A stale <script src> fails silently in a Tauri build — the file 404s, the
  // script never runs, and the feature it owned just disappears from the UI
  // with nothing in the console. This is the exact failure the llm-engines-tab
  // extraction could have shipped.
  const missing = scriptSources()
    .filter((s) => !s.startsWith('__harness/'))
    .filter((s) => !fs.existsSync(path.join(SRC, s)))
  assert.deepEqual(missing, [], `index.html references missing scripts: ${missing.join(', ')}`)
})

test('extracted tabs load after app.js (they read app.js globals)', () => {
  // Extracting code out of app.js made load order a real contract: $, showToast
  // and getLLMEngines are all defined by app.js. All scripts are `defer`, so
  // document order IS execution order.
  const order = scriptSources()
  const app = order.indexOf('app.js')
  assert.ok(app !== -1, 'app.js must be loaded')
  const tabs = [
  'llm-engines-tab.js',
  'deepy-tab.js',
  'deepy-web-tab.js',
  'installer-tab.js',
  'plugins-tab.js',
  'theme-tab.js',
  'init-tab.js',
  'dashboard-tab.js',
  'env-tab.js',
  'kernelsync-tab.js',
  'migration-tab.js',
  'dlss5-tab.js',
  'settings-tab.js',
  'troubleshoot-tab.js',
]
  for (const tab of tabs) {
    const i = order.indexOf(tab)
    assert.ok(i !== -1, `${tab} must be loaded`)
    assert.ok(i > app, `${tab} must come after app.js (got ${i} vs ${app})`)
  }
})
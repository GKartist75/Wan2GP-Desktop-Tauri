/**
 * load-order.test.js — guards the contract that splitting app.js into
 * *-tab.js files created.
 *
 * All renderer scripts are `defer`, so document order in index.html IS
 * execution order. That means one script cannot reference a symbol defined by a
 * script that loads later. Calling one inside a callback is fine (deferred);
 * passing one as a bare argument — `addEventListener("click", toggleTheme)` —
 * evaluates the reference immediately and throws ReferenceError. That shipped
 * twice during the split, both times only visible in the browser harness.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')

const SRC = path.join(__dirname, '..', 'src')

// An unclosed <div> nests everything after it inside that element and closes
// the grid container early: the panel renders but the app comes up black, with
// no console error pointing at the real line. That happened twice while adding
// the v17 controls, so the invariant is now a test rather than a memory.
// Only counts real <div> tags — <!doctype>, <div/> and text are excluded, and
// <script>/<style> bodies are stripped first so JS comparison operators inside
// them cannot be mistaken for markup.
test('index.html div tags balance', () => {
  const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
  const markup = html
    .replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi, '')
    .replace(/<style\b[^>]*>[\s\S]*?<\/style>/gi, '')
    .replace(/<!--[\s\S]*?-->/g, '')
  const opens = (markup.match(/<div\b(?![^>]*\/>)[^>]*>/gi) || []).length
  const closes = (markup.match(/<\/div\s*>/gi) || []).length
  assert.equal(
    opens,
    closes,
    `index.html has ${opens} <div> but ${closes} </div> — the DOM will nest and the app can render black`,
  )
})

test('every id the Auto-Tune panel reads exists exactly once', () => {
  const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
  // A duplicated id silently breaks $(id) lookups: the panel would repaint the
  // first one while the tags it writes to belong to the second.
  const ids = [...html.matchAll(/\sid="([^"]+)"/g)].map((m) => m[1])
  const dupes = ids.filter((id, i) => ids.indexOf(id) !== i)
  assert.deepEqual([...new Set(dupes)], [], `duplicate ids in index.html`)
  // The controls the v17 work added must all still be present — a rename in
  // one file without the other is exactly the silent failure above.
  for (const id of [
    'memAttentionMode',
    'memVramAllocator',
    'memRamAllocator',
    'memHeadSplit',
    'memReadAhead',
    'memSmartPinning',
    'memVideoPreload',
    'memImagePreload',
    'memAudioPreload',
    'memReservedPct',
  ]) {
    assert.ok(ids.includes(id), `index.html is missing #${id}`)
  }
})
const loadedScripts = () => {
  const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
  return [...html.matchAll(/<script[^>]*src="([^"]+)"/g)]
    .map((m) => m[1])
    .filter((s) => s.endsWith('.js') && !s.startsWith('__harness/'))
}

/** symbol -> index of the first script that declares it */
function definitionIndex(order) {
  const defines = new Map()
  order.forEach((s, idx) => {
    const file = path.join(SRC, s)
    if (!fs.existsSync(file)) return
    const src = fs.readFileSync(file, 'utf8')
    const add = (n) => {
      if (!defines.has(n)) defines.set(n, idx)
    }
    for (const m of src.matchAll(/^(?:async\s+)?function\s+(\w+)/gm)) add(m[1])
    for (const m of src.matchAll(/^(?:const|let|var)\s+(\w+)\s*=/gm)) add(m[1])
  })
  return defines
}

/** top-level line indexes, with comments and string literals blanked out */
function topLevelLines(source) {
  const lines = source.split('\n').map((l) =>
    l
      .replace(/\/\*[\s\S]*?\*\//g, ' ')
      .replace(/\/\/.*$/, ' ')
  )
  const out = new Set()
  let depth = 0
  lines.forEach((line, idx) => {
    if (depth === 0) out.add(idx)
    for (const ch of line) {
      if (ch === '{') depth++
      else if (ch === '}') depth--
    }
  })
  return out
}

/**
 * A load-time hazard is a symbol used as a BARE ARGUMENT at top level:
 *   addEventListener("click", toggleTheme)     reads it now -> ReferenceError
 * versus a call inside a callback, which is deferred and therefore safe:
 *   addEventListener("click", () => toggleTheme())
 */
function bareArgumentUse(line, name) {
  const esc = name.replace(/\$/g, '\\$')
  const leading = new RegExp('(,|\\()\\s*' + esc + '\\s*$')
  const trailing = new RegExp('(^|[^\\w.$])' + esc + '\\s*[,)]')
  return leading.test(line) && trailing.test(line)
}

test('no script passes a later script symbol by value at top level', () => {
  const order = loadedScripts()
  const defines = definitionIndex(order)
  assert.ok(defines.size > 0, 'expected to find declarations')

  const hazards = []
  order.forEach((s, self) => {
    const file = path.join(SRC, s)
    if (!fs.existsSync(file)) return
    const source = fs.readFileSync(file, 'utf8')
    const lines = source.split('\n')
    for (const i of topLevelLines(source)) {
      for (const [name, owner] of defines) {
        if (owner >= self) continue
        if (bareArgumentUse(lines[i], name)) {
          hazards.push(
            s + ':' + (i + 1) + ' reads ' + name +
              ' (defined later in ' + order[owner] + ') — ' + lines[i].trim().slice(0, 60)
          )
        }
      }
    }
  })
  assert.deepEqual(hazards, [], 'load-time bare references to a later script:\n' + hazards.join('\n'))
})

test('every script referenced by index.html exists on disk', () => {
  // A stale <script src> 404s in a Tauri build, the script silently never runs,
  // and the feature it owned just disappears with an empty console.
  const missing = loadedScripts().filter((s) => !fs.existsSync(path.join(SRC, s)))
  assert.deepEqual(missing, [], `index.html references missing scripts: ${missing.join(', ')}`)
})

test('every extracted tab is loaded, and after app.js', () => {
  const order = loadedScripts()
  const app = order.indexOf('app.js')
  assert.ok(app !== -1, 'app.js must be loaded')
  const onDisk = fs.readdirSync(SRC).filter((f) => f.endsWith('-tab.js'))
  assert.ok(onDisk.length > 0, 'expected extracted tab files')
  for (const tab of onDisk) {
    const i = order.indexOf(tab)
    assert.ok(i !== -1, `${tab} exists but is not loaded by index.html`)
    assert.ok(i > app, `${tab} must load after app.js`)
  }
})

/**
 * console-resize.test.js — the console panel must be draggable in every dock.
 *
 * The shipped console had ONE handle, wired to bottom/top only: left/right
 * docks silently ignored the drag, so a user who docks the console sideways
 * could not trade GUI width for log width at all. These tests pin the per-edge
 * geometry, the drag maths for each dock, the clamp, and the persistence that
 * keeps a dragged size across a dock switch and a relaunch.
 *
 * Same approach as theme-persist.test.js: the renderer has no module loader,
 * so the real function bodies are lifted out of src/app.js and run in a vm
 * against a fake element. Running the code catches behaviour changes; string
 * matching would not.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')

const SRC = path.join(__dirname, '..', 'src')
const appJs = fs.readFileSync(path.join(SRC, 'app.js'), 'utf8')
const termTab = fs.readFileSync(path.join(SRC, 'term-tab.js'), 'utf8')
const indexHtml = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
const css = fs.readFileSync(path.join(SRC, 'style.css'), 'utf8')

/** pull one top-level `function name(...) {...}` body out of a renderer file */
function extract(file, name) {
  const m = file.match(
    new RegExp(`(?:async\\s+)?function\\s+${name}\\s*\\([^)]*\\)\\s*\\{[\\s\\S]*?\\n\\}`),
  )
  assert.ok(m, `${name} not found in ${file}`)
  return m[0]
}

/**
 * Run the real resize code against a stand-in console element.
 *
 * The fake panel reports whatever width/height its inline style says, which is
 * what the browser does for a dock sized by CSS alone — so a handle that never
 * writes the style leaves the measured size at the CSS default, exactly like the
 * live "the drag did nothing" bug.
 */
function harness({ dock = 'bottom', cfg = {}, win = { innerWidth: 1200, innerHeight: 900 } } = {}) {
  const saved = []
  const state = { w: 0, h: 0 }
  const numeric = (v, fallback) => (v ? parseInt(v, 10) : fallback)
  const ft = {
    className: 'floating-term dock-' + dock,
    dataset: {},
    style: {},
    get offsetWidth() {
      const side = this.className.includes('dock-left') || this.className.includes('dock-right')
      state.w = side ? numeric(this.style.width, 340) : win.innerWidth
      return state.w
    },
    get offsetHeight() {
      const side = this.className.includes('dock-left') || this.className.includes('dock-right')
      state.h = side ? win.innerHeight : numeric(this.style.height, 200)
      return state.h
    },
    getBoundingClientRect() {
      // Measure first so a caller that captured a rect before any drag still
      // sees the CSS default rather than a stale zero.
      return { left: 0, top: 0, right: this.offsetWidth, bottom: this.offsetHeight, width: this.offsetWidth, height: this.offsetHeight }
    },
  }
  const handle = { dataset: { edge: 'n', axis: 'v' }, setPointerCapture() {} }
  const pressed = []
  const doc = {
    readyState: 'complete',
    body: {
      classList: {
        add: (c) => pressed.push('add:' + c),
        remove: (c) => pressed.push('rm:' + c),
      },
    },
    querySelectorAll: () => [],
  }
  const ctx = {
    console,
    window: {
      innerWidth: win.innerWidth,
      innerHeight: win.innerHeight,
      w2gp: {
        configLoad: async () => cfg,
        configSave: async (c) => saved.push(JSON.parse(JSON.stringify(c))),
      },
      addEventListener() {},
    },
    document: doc,
    requestAnimationFrame: (fn) => fn(),
    $: (id) => (id === 'floatingTerminal' ? ft : null),
    currentDock: () => dock,
    syncTermEmbedPadding() {},
    syncNativeBoundsAdjusted() {},
    handle,
  }
  ctx.window.document = doc
  vm.createContext(ctx)
  vm.runInContext(
    [
      'const FT_MIN = { v: 80, h: 240 };',
      'const FT_TOPBAR = 44;',
      'const FT_DEFAULT = { bottom: 200, top: 200, left: 340, right: 340 };',
      'let _ftSizes = null; let _resize = null; let _ftSyncRaf = 0;',
      extract(appJs, 'ftClamp'),
      extract(appJs, 'ftApplySize'),
      extract(appJs, 'anchorFloatingTerm'),
      extract(appJs, 'ftRememberSize'),
      extract(appJs, 'ftScheduleSync'),
      extract(appJs, 'ftStartResize'),
      extract(appJs, '_resizeMove'),
      extract(appJs, '_resizeEnd'),
      'this.api = { ftStartResize, _resizeMove, _resizeEnd, ftApplySize, anchorFloatingTerm };',
      // Mirrors the boot block in app.js: read termSizes, then size the dock.
      'this.boot = (c) => { if (c && c.termSizes && typeof c.termSizes === "object") _ftSizes = c.termSizes; ftApplySize(currentDock()); };',
    ].join('\n'),
    ctx,
    { timeout: 5000 },
  )
  return Object.assign(ctx, { ft, handle, saved, pressed })
}

/** press the given edge, drag by (dx, dy), release */
function drag(h, { edge, axis, dx, dy }) {
  h.handle.dataset.edge = edge
  h.handle.dataset.axis = axis
  h.ft.getBoundingClientRect()
  h.api.ftStartResize({
    button: 0,
    pointerId: 1,
    clientX: 500,
    clientY: 500,
    currentTarget: h.handle,
    preventDefault() {},
  })
  h.api._resizeMove({ clientX: 500 + dx, clientY: 500 + dy })
  h.api._resizeEnd()
}

const SIDE = (dock) => dock === 'left' || dock === 'right'
const sizeOf = (h, dock) => (SIDE(dock) ? h.ft.offsetWidth : h.ft.offsetHeight)

test('every dock exposes a draggable edge on the side facing Wan2GP', () => {
  const edges = [...indexHtml.matchAll(/data-edge="([nsew])"\s+data-axis="([hv])"/g)].map((m) => m[1])
  assert.deepEqual(edges, ['n', 's', 'w', 'e'], 'index.html must ship all four edges')

  for (const [dock, live] of Object.entries({ bottom: 'n', top: 's', left: 'e', right: 'w' })) {
    assert.match(
      css,
      new RegExp(`\\.floating-term\\.dock-${dock} \\.resize-${live}\\b`),
      `dock-${dock} must offer its ${live} edge`,
    )
  }
  assert.match(css, /\.floating-term\.dock-floating \.resize-n/, 'floating keeps all four edges')
})

test('dragging the edge grows the console on every dock', () => {
  // The bug: only bottom/top were wired, so a sideways-docked console ignored
  // the drag completely — no way to trade GUI width for log width.
  const moves = {
    bottom: { edge: 'n', axis: 'v', dx: 0, dy: -120 },
    top: { edge: 's', axis: 'v', dx: 0, dy: 120 },
    left: { edge: 'e', axis: 'h', dx: 200, dy: 0 },
    right: { edge: 'w', axis: 'h', dx: -200, dy: 0 },
  }
  for (const [dock, move] of Object.entries(moves)) {
    const h = harness({ dock })
    const before = sizeOf(h, dock)
    drag(h, move)
    const after = sizeOf(h, dock)
    assert.ok(
      after > before,
      `dock-${dock} did not grow: ${before} -> ${after} (style "${h.ft.style.width || h.ft.style.height}")`,
    )
  }
})

test('a drag is bounded so the console always leaves the GUI something', () => {
  const h = harness({ dock: 'bottom', win: { innerWidth: 1200, innerHeight: 900 } })
  drag(h, { edge: 'n', axis: 'v', dx: 0, dy: -5000 })
  // Full size means everything below the topbar (44px) — the topbar is the one
  // thing a drag never covers, and the console's own ✕ stays reachable at it.
  assert.equal(h.ft.offsetHeight, 900 - 44, 'a drag must be able to take the whole area below the topbar')

  drag(h, { edge: 'n', axis: 'v', dx: 0, dy: 5000 })
  assert.equal(h.ft.offsetHeight, 80, 'must not collapse below the header + search rows')

  // Sideways has no topbar to respect — it may take the full width.
  const wide = harness({ dock: 'left', win: { innerWidth: 1200, innerHeight: 900 } })
  drag(wide, { edge: 'e', axis: 'h', dx: 5000, dy: 0 })
  assert.equal(wide.ft.offsetWidth, 1200)
})

test('a dragged size is remembered per dock and survives a relaunch', async () => {
  const h = harness({ dock: 'bottom' })
  drag(h, { edge: 'n', axis: 'v', dx: 0, dy: -260 })
  const dragged = h.ft.offsetHeight
  await new Promise((r) => setImmediate(r))
  assert.equal(h.saved.length, 1, 'the size must reach desktop-config.json')
  assert.deepEqual(h.saved[0].termSizes.bottom, { h: dragged })

  const boot = harness({ dock: 'bottom', cfg: { termSizes: { bottom: { h: dragged } } } })
  boot.boot({ termSizes: { bottom: { h: dragged } } })
  assert.equal(boot.ft.style.height, dragged + 'px', 'the relaunched panel must come back the size the user dragged')
  boot.api.ftApplySize('left')
  assert.equal(boot.ft.style.width, '340px', 'a dock with no saved size falls back to its default')
})

test('switching docks re-applies the saved size instead of snapping back', () => {
  // setFtDock used to run `ft.style.cssText = ""`, which threw the dragged size
  // away on every dock change — the reason a resize appeared not to stick.
  assert.match(
    extract(termTab, 'setFtDock'),
    /ftApplySize\(dock\)/,
    'setFtDock must restore the size for the dock it switches to',
  )
})

test('the console marks the window while dragging and clears it on release', () => {
  const h = harness({ dock: 'bottom' })
  drag(h, { edge: 'n', axis: 'v', dx: 0, dy: 0 })
  assert.deepEqual(h.pressed, ['add:ft-resizing', 'rm:ft-resizing'])
})

test('a floating console is anchored before the first west/north drag', () => {
  // .dock-floating positions the panel from the right, so a west-edge drag had
  // no fixed edge to grow from: the panel would slide away from the cursor.
  const h = harness({ dock: 'floating' })
  h.ft.style.left = '398px'
  h.api.anchorFloatingTerm(h.ft)
  assert.equal(h.ft.dataset.ftAnchored, '1')
  assert.equal(h.ft.style.right, 'auto', 'the right anchor must go, or left and right fight')
  assert.equal(h.ft.style.left, '0px', 'anchor conversion keeps the on-screen position')
})

/**
 * The dashboard Console card, same drag on its top edge. Its parent column is
 * what bounds it, so the fake column carries a clientHeight.
 */
function dashHarness({ colH = 464 } = {}) {
  const saved = []
  const classes = new Set()
  const card = {
    style: {},
    // A pinned card measures its inline height, like the real one.
    get offsetHeight() {
      return this.style.height ? parseInt(this.style.height, 10) : 200;
    },
    parentElement: { clientHeight: colH },
  }
  const body = {
    classList: {
      toggle: (c, on) => (on ? classes.add(c) : classes.delete(c)),
      contains: (c) => classes.has(c),
    },
  }
  const btn = {
    textContent: '',
    attrs: {},
    active: false,
    classList: { toggle: (c, on) => (btn.active = !!on) },
    setAttribute: (k, v) => (btn.attrs[k] = v),
  }
  const handle = {}
  const pressed = []
  const doc = {
    readyState: 'complete',
    body: {
      classList: {
        add: (c) => pressed.push('add:' + c),
        remove: (c) => pressed.push('rm:' + c),
      },
    },
  }
  const ctx = {
    console,
    window: {
      innerWidth: 1200,
      innerHeight: 900,
      w2gp: {
        configLoad: async () => ({}),
        configSave: async (c) => saved.push(JSON.parse(JSON.stringify(c))),
      },
      addEventListener() {},
    },
    document: doc,
    $: (id) =>
      id === 'dashTermCard' ? card : id === 'dashBody' ? body : id === 'dashTermMaxBtn' ? btn : null,
    ftScheduleSync() {},
    handle,
  }
  ctx.window.document = doc
  vm.createContext(ctx)
  vm.runInContext(
    [
      'const DASH_TERM_MIN = 120;',
      'let _dashTermPinned = 0; let _dashTermSaved = 0; let _dashResize = null;',
      extract(appJs, 'applyDashTermHeight'),
      extract(appJs, 'dashTermClamp'),
      extract(appJs, 'dashTermSetMax'),
      extract(appJs, 'startDashTermResize'),
      extract(appJs, 'dashTermResizeMove'),
      extract(appJs, 'dashTermResizeEnd'),
      'this.api = { applyDashTermHeight, dashTermClamp, dashTermSetMax, startDashTermResize, dashTermResizeMove, dashTermResizeEnd };',
    ].join('\n'),
    ctx,
    { timeout: 5000 },
  )
  return Object.assign(ctx, { card, body, btn, handle, saved, pressed, classes })
}

function dashDrag(h, dy) {
  h.api.startDashTermResize({ button: 0, pointerId: 1, clientX: 300, clientY: 500, preventDefault() {} })
  h.api.dashTermResizeMove({ clientY: 500 + dy })
  h.api.dashTermResizeEnd()
}

test('the dashboard console card has a draggable top edge', () => {
  assert.match(indexHtml, /id="dashTermResize"/, 'index.html must ship the dashboard drag edge')
  assert.match(css, /\.dash-term-resize\s*\{[^}]*cursor:\s*ns-resize/, 'the edge must advertise the vertical resize cursor')
})

test('dragging the dashboard edge up gives the console more of the column', () => {
  const h = dashHarness()
  dashDrag(h, -160)
  // flex-basis:0 from `flex:1` beats an inline height in a column flex
  // container, so a dragged size only sticks if the basis is pinned too.
  assert.equal(h.card.style.flex, '0 0 auto', 'the card must stop being flex:1 or the height is ignored')
  assert.equal(h.card.style.height, '360px')
})

test('the dashboard console is bounded by its column and a 120px floor', () => {
  const h = dashHarness({ colH: 464 })
  dashDrag(h, -5000)
  assert.equal(h.card.style.height, '384px', 'must leave the cards above it reachable')
  dashDrag(h, 5000)
  assert.equal(h.card.style.height, '120px')
})

test('clearing the pinned height hands the card back to the column', () => {
  const h = dashHarness()
  dashDrag(h, -160)
  h.api.applyDashTermHeight(0)
  assert.equal(h.card.style.flex, '', 'flex:1 must come back')
  assert.equal(h.card.style.height, '')
})

test('the dashboard console size is persisted', async () => {
  const h = dashHarness()
  dashDrag(h, -160)
  await new Promise((r) => setImmediate(r))
  assert.equal(h.saved.length, 1)
  assert.equal(h.saved[0].dashTermHeight, 360)
  assert.deepEqual(h.pressed, ['add:ft-resizing', 'rm:ft-resizing'])
})

test('full-console mode hands the console the whole dashboard', () => {
  // In browser mode the launcher IS the web page and the console is the view —
  // a drag inside its column can never fill the window, so the card would stay
  // boxed in beside cards nobody opens.
  const h = dashHarness()
  h.api.dashTermSetMax(true)
  assert.ok(h.body.classList.contains('dash-console-max'))
  assert.equal(h.card.style.height, '', 'the pinned height must not fight the max layout')
  assert.equal(h.card.style.flex, '', 'and neither must the pinned flex basis')
  assert.equal(h.btn.textContent, '⤢ Exit'.replace('⤢ Exit', '⤡ Exit'))
  assert.equal(h.btn.attrs['aria-pressed'], 'true')

  h.api.dashTermSetMax(false)
  assert.ok(!h.body.classList.contains('dash-console-max'))
  assert.equal(h.btn.textContent, '⤢ Full')
})

test('leaving full-console mode restores the dragged size', () => {
  const h = dashHarness()
  dashDrag(h, -160) // → 360
  h.api.dashTermSetMax(true)
  assert.equal(h.card.style.height, '', 'max mode owns the layout')
  h.api.dashTermSetMax(false)
  assert.equal(h.card.style.height, '360px', 'the drag must survive a round trip through max')
  assert.equal(h.card.style.flex, '0 0 auto')
})

test('dragging while full-console is on drops back to the normal layout', () => {
  // A pinned pixel height and the max layout cannot both hold; the drag is the
  // user saying "give me the dashboard back".
  const h = dashHarness()
  h.api.dashTermSetMax(true)
  h.api.startDashTermResize({ button: 0, pointerId: 1, clientX: 300, clientY: 500, preventDefault() {} })
  assert.ok(!h.body.classList.contains('dash-console-max'))
})

test('full-console mode is not persisted across launches', () => {
  // Reopening into a dashboard with no visible GUI reads as a broken launch.
  assert.doesNotMatch(
    extract(appJs, 'dashTermSetMax'),
    /configSave/,
    'the max toggle must stay a per-session choice',
  )
  assert.match(appJs, /dashTermMaxBtn"\)\?\.addEventListener\("click"/, 'the button must toggle it')
  assert.match(appJs, /dashTermResize"\)\?\.addEventListener\("dblclick"/, 'the edge must double-click to it too')
})
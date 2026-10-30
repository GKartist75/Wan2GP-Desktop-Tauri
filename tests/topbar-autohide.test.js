/**
 * topbar-autohide.test.js — the topbar can be pinned or reveal on hover.
 *
 * Auto-hide is a layout mode: the bar leaves the dashboard flex row, the
 * reveal strip (a real DOM element above the iframe/native child) brings it
 * back, and switching to pinned drops both classes. These tests run the real
 * bodies from src/theme-tab.js against a fake dashboard, so a class-name or
 * state-machine regression fails here instead of only in the UI.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')

const SRC = path.join(__dirname, '..', 'src')
const themeTab = fs.readFileSync(path.join(SRC, 'theme-tab.js'), 'utf8')

function extract(name) {
  const m = themeTab.match(
    new RegExp(`(?:async\\s+)?function\\s+${name}\\s*\\([^)]*\\)\\s*\\{[\\s\\S]*?\\n\\}`),
  )
  assert.ok(m, `${name} not found in theme-tab.js`)
  return m[0]
}

function classList() {
  const set = new Set()
  return {
    add: (c) => set.add(c),
    remove: (c) => set.delete(c),
    contains: (c) => set.has(c),
    toggle: (c, on) => {
      if (on === undefined) {
        if (set.has(c)) set.delete(c)
        else set.add(c)
      } else if (on) set.add(c)
      else set.delete(c)
      return set.has(c)
    },
  }
}

function harness({ manualTimers = false } = {}) {
  const els = new Map()
  const el = (id) => {
    if (!els.has(id)) {
      els.set(id, {
        id,
        value: '',
        textContent: '',
        title: '',
        classList: classList(),
        setAttribute(k, v) {
          this[k] = v
        },
        blur() {
          this.blurred = true
        },
      })
    }
    return els.get(id)
  }
  const topbar = el('topbar')
  topbar.matches = () => false
  topbar.contains = (node) => !!node && node.parent === topbar
  topbar.offsetHeight = 44
  const doc = {
    activeElement: null,
    querySelector: (sel) => (sel === '#dashboard .topbar' ? topbar : null),
  }
  const ls = new Map()
  const timers = []
  let nextTimer = 1
  const ctx = {
    $: (id) => el(id),
    document: doc,
    _appearMem: { accent: 'mono', ui: 100, term: 100, topbar: 'pinned', custom: {} },
    _appearStore: () => {
      ls.set('w2gp.topbarMode', ctx._appearMem.topbar)
    },
    _syncTopbarBounds: () => {},
    clearTimeout: (id) => {
      const i = timers.findIndex((t) => t.id === id)
      if (i >= 0) timers.splice(i, 1)
    },
    setTimeout: (fn, ms) => {
      if (!manualTimers) {
        fn()
        return 0
      }
      const id = nextTimer++
      timers.push({ id, fn, ms })
      return id
    },
    localStorage: {
      setItem: (k, v) => ls.set(k, String(v)),
      getItem: (k) => (ls.has(k) ? ls.get(k) : null),
    },
    ls,
  }
  ctx.window = ctx
  vm.createContext(ctx)
  vm.runInContext(
    'let _topbarHideTimer = null; let _topbarIdleTimer = null; let _topbarCursorPoll = null;' +
      '\n' +
      extract('_topbarDash') +
      '\n' +
      extract('_topbarAutoHidden') +
      '\n' +
      extract('_stopTopbarCursorPoll') +
      '\n' +
      extract('_startTopbarCursorPoll') +
      '\n' +
      extract('revealTopbar') +
      '\n' +
      extract('hideTopbar') +
      '\n' +
      extract('scheduleTopbarHide') +
      '\n' +
      extract('_armTopbarIdle') +
      '\n' +
      extract('_syncTopbarPin') +
      '\n' +
      extract('applyTopbarMode') +
      '\nthis.apply = applyTopbarMode; this.reveal = revealTopbar; this.hide = hideTopbar; this.schedule = scheduleTopbarHide; this.armIdle = _armTopbarIdle;',
    ctx,
    { timeout: 5000 },
  )
  return { ctx, els, ls, timers, topbar, doc }
}

test('pinned is the default and keeps the topbar in flow', () => {
  const { ctx, els, ls } = harness()
  ctx.apply('pinned')
  const dash = els.get('dashboard')
  assert.equal(dash.classList.contains('topbar-autohide'), false)
  assert.equal(dash.classList.contains('topbar-revealed'), false)
  assert.equal(els.get('topbarModeSelect').value, 'pinned')
  assert.equal(ls.get('w2gp.topbarMode'), 'pinned')
})

test('hover mode takes the bar out of flow and reveals it from the strip', () => {
  const { ctx, els } = harness()
  ctx.apply('hover')
  const dash = els.get('dashboard')
  assert.equal(dash.classList.contains('topbar-autohide'), true)
  assert.equal(dash.classList.contains('topbar-revealed'), false)
  assert.equal(els.get('topbarModeSelect').value, 'hover')

  ctx.reveal()
  assert.equal(dash.classList.contains('topbar-revealed'), true)
  ctx.hide()
  assert.equal(dash.classList.contains('topbar-revealed'), false)
})

test('switching back to pinned clears both auto-hide classes', () => {
  const { ctx, els } = harness()
  ctx.apply('hover')
  ctx.reveal()
  assert.equal(els.get('dashboard').classList.contains('topbar-revealed'), true)
  ctx.apply('pinned')
  const dash = els.get('dashboard')
  assert.equal(dash.classList.contains('topbar-autohide'), false)
  assert.equal(dash.classList.contains('topbar-revealed'), false)
})

test('an unknown persisted value falls back to pinned', () => {
  const { ctx, els } = harness()
  ctx.apply('bogus')
  assert.equal(ctx._appearMem.topbar, 'pinned')
  assert.equal(els.get('dashboard').classList.contains('topbar-autohide'), false)
})

test('the bar pin reflects and toggles the persisted mode', () => {
  const { ctx, els } = harness()
  ctx.apply('pinned')
  const pin = els.get('topbarPinBtn')
  assert.equal(pin.classList.contains('is-pinned'), true)
  assert.equal(pin['aria-pressed'], 'true')
  assert.match(pin.title, /pinned/i)

  ctx.apply('hover')
  assert.equal(pin.classList.contains('is-pinned'), false)
  assert.equal(pin['aria-pressed'], 'false')
  assert.match(pin.title, /auto-hides/i)
})

test('mouse-out hides even when a clicked button still owns focus', () => {
  const { ctx, els, topbar, doc } = harness()
  ctx.apply('hover')
  ctx.reveal()
  const btn = {
    tagName: 'BUTTON',
    parent: topbar,
    blur() {
      this.blurred = true
    },
  }
  doc.activeElement = btn
  ctx.schedule()
  assert.equal(els.get('dashboard').classList.contains('topbar-revealed'), false)
  assert.equal(btn.blurred, true, 'hidden bar must drop stale button focus')

  // An open native select must survive the same timer.
  ctx.reveal()
  const select = { tagName: 'SELECT', parent: topbar }
  doc.activeElement = select
  ctx.schedule()
  assert.equal(els.get('dashboard').classList.contains('topbar-revealed'), true)
})

test('idle watchdog hides a pointer-revealed bar with no mouseleave', () => {
  const { ctx, els, timers } = harness({ manualTimers: true })
  ctx.apply('hover')
  ctx.reveal()
  ctx.armIdle(1800)
  assert.equal(timers.length, 1)
  timers.pop().fn()
  assert.equal(els.get('dashboard').classList.contains('topbar-revealed'), false)
})

test('idle watchdog keeps the bar while a select owns focus', () => {
  const { ctx, els, timers, topbar, doc } = harness({ manualTimers: true })
  ctx.apply('hover')
  ctx.reveal()
  doc.activeElement = { tagName: 'SELECT', parent: topbar }
  ctx.armIdle(1800)
  timers.pop().fn()
  assert.equal(els.get('dashboard').classList.contains('topbar-revealed'), true)
  assert.equal(timers.length, 1)
  assert.equal(timers[0].ms, 800, 're-checks instead of hiding under an open dropdown')
})

test('the reveal strip sits above the embedded view in auto-hide mode', () => {
  const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
  const css = fs.readFileSync(path.join(SRC, 'style.css'), 'utf8')
  assert.match(html, /id="topbarRevealZone"/)
  assert.match(
    css,
    /#dashboard\.topbar-autohide \.topbar-reveal-zone\s*\{[^}]*z-index:\s*59/s,
    'strip must outrank the iframe/native child but sit below the revealed topbar',
  )
  assert.match(
    css,
    /#dashboard\.topbar-autohide \.topbar\s*\{[^}]*z-index:\s*60/s,
    'revealed bar must cover the strip',
  )
  assert.match(
    css,
    /#dashboard\.topbar-autohide \.topbar\s*\{[^}]*transform:\s*translateY\(-100%\)/s,
    'auto-hide must move the bar offscreen',
  )
})

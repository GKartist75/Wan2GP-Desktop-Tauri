/**
 * signature-themes.test.js — the 4 shipped signature theme palettes are wired
 * end to end: dots in the picker, fixed token blocks in style.css, and the
 * accent application + persistence path accepts them.
 *
 * Keys are noir / phosphor / parchment / retina (the accent slot names
 * "cyber" and "matrix" already belong to editable 3-color themes).
 * noir / phosphor / retina are dark-only — the JS must switch a light shell
 * to dark on first pick so the palette can never silently no-op.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')

const SRC = path.join(__dirname, '..', 'src')
const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
const css = fs.readFileSync(path.join(SRC, 'style.css'), 'utf8')
const theme = fs.readFileSync(path.join(SRC, 'theme-tab.js'), 'utf8')

const FAME_KEYS = ['noir', 'phosphor', 'parchment', 'retina']
const DARK_ONLY_KEYS = ['noir', 'phosphor', 'retina']

test('the theme picker carries a dot for each signature theme', () => {
  for (const k of FAME_KEYS)
    assert.match(
      html,
      new RegExp(`data-dot="${k}"`),
      `missing picker dot for ${k}`,
    )
})

test('each signature palette is a full token block in style.css', () => {
  const anchors = {
    noir: [
      ':root[data-theme="dark"][data-accent="noir"]',
      '--accent: #ff2bd6',
      '--canvas: #0d0a14',
      '--signal-yellow: #ffe14d',
    ],
    phosphor: [
      ':root[data-theme="dark"][data-accent="phosphor"]',
      '--accent: #22ff66',
      '--canvas: #000000',
      '--signal-yellow: #eaff00',
    ],
    parchment: [
      ':root[data-accent="parchment"]',
      '--accent: #8a5a1e',
      '--canvas: #f4ead6',
      '--signal-red: #8a2f22',
    ],
    retina: [
      ':root[data-theme="dark"][data-accent="retina"]',
      '--accent: #ff71ce',
      '--canvas: #120b24',
      '--signal-blue: #01cdfe',
    ],
  }
  for (const [k, regexes] of Object.entries(anchors)) {
    for (const re of regexes)
      assert.match(css, new RegExp(re.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')), `${k}: missing "${re}"`)
  }
})

test('dark-only palettes are scoped under [data-theme="dark"]', () => {
  // a standalone block would repaint light shells with a dark palette
  for (const k of DARK_ONLY_KEYS)
    assert.doesNotMatch(
      css,
      new RegExp(`^:root\\[data-accent="${k}"\\]\\s*\\{`, 'm'),
      `${k} must not ship a light-mode token block`,
    )
  for (const k of DARK_ONLY_KEYS)
    assert.match(
      css,
      new RegExp(`:root\\[data-theme="dark"\\]\\[data-accent="${k}"\\]`),
      `${k} must ship a dark-scoped token block`,
    )
  // parchment is mode-agnostic: standalone selector, no dark-scoped block
  assert.match(css, /:root\[data-accent="parchment"\]/)
})

test('the JS accepts signature keys, maps labels, and switches light to dark', () => {
  assert.match(theme, /const FAME_THEMES = \["noir", "phosphor", "parchment", "retina"\]/)
  assert.match(theme, /const FAME_DARK_ONLY = \["noir", "phosphor", "retina"\]/)
  assert.match(theme, /APPEAR_LABELS\.noir = "Cyberpunk"/)
  assert.match(theme, /APPEAR_LABELS\.phosphor = "Phosphor"/)
  assert.match(theme, /APPEAR_LABELS\.parchment = "Parchment"/)
  assert.match(theme, /APPEAR_LABELS\.retina = "Retina Sunset"/)
  assert.match(theme, /FAME_DARK_ONLY\.includes\(fame\)/, 'dark-only enforcement exists')
  assert.match(theme, /applyTheme\("dark"\)/, 'the visual switch reuses applyTheme')
  assert.match(
    theme,
    /\[\.\.\.APPEAR_ACCENTS, \.\.\.FAME_THEMES\]/,
    'restored profiles accept signature keys, not just the 5 editable slots',
  )
  assert.match(
    theme,
    /\[\.\.\.APPEAR_THEMES, \.\.\.FAME_THEMES\]/,
    'the palette cycle button covers every theme',
  )
})

test('signature themes persist through the same themeAccent key', () => {
  assert.match(theme, /cfg\.themeAccent = _appearMem\.accent/, 'persistence key unchanged')
  assert.match(theme, /FAME_DOT\b/, 'picker dots get signatures hardcoded accent colors')
  // not editable via the 3-color editor
  assert.match(theme, /function _isEditableSlot\(name\) \{\s*\n\s*return APPEAR_THEMES\.includes\(name\);?\s*\n\s*\}/)
})

test('the appearance hint documents the shipped palettes', () => {
  assert.match(html, /signature palettes|dark-only/i)
})

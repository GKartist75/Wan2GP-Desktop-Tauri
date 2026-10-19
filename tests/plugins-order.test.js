/**
 * plugins-order.test.js — the launcher's own plugins stay on top.
 *
 * Reported: in Manage → Plugins, "Finetune Manager · by GKartist75" sat in the
 * middle of the alphabetical community run, behind File Gallery — the launcher
 * author's own entry, buried by name in the launcher's own plugin manager.
 * Order is now group (community above system, unchanged) → our own plugins →
 * the chosen column and direction, matched on the entry's author or its repo
 * URL, case-insensitively (a catalog entry carries `author`; a locally cloned
 * plugin may only carry the URL).
 *
 * A 📌 badge marks the pinned row so the order does not read as a sort bug.
 * The pin holds for the default Name sort and for Latest / Created by, since
 * the ask was simply "my own plugins on top".
 *
 * Like mem-seed.test.js, the real function bodies run in a vm against fakes.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')

const tab = fs.readFileSync(path.join(__dirname, '..', 'src', 'plugins-tab.js'), 'utf8')

/** pull one top-level `function name(...) {...}` body (column-0 close) */
function extractFn(name) {
  const m = tab.match(
    new RegExp(`(?:async\\s+)?function\\s+${name}\\s*\\([^)]*\\)\\s*\\{[\\s\\S]*?\\n\\}`),
  )
  assert.ok(m, `${name} not found`)
  return m[0]
}

/** pull one top-level `const NAME = ...` line */
function extractLine(prefix) {
  const m = tab.match(new RegExp(`^${prefix}[^\\n]*$`, 'm'))
  assert.ok(m, `${prefix} not found`)
  return m[0]
}

const ctx = {}
vm.createContext(ctx)
vm.runInContext(
  [
    extractLine('const OWN_PLUGIN_HANDLE'),
    extractFn('isOwnPlugin'),
    extractFn('pluginCompare'),
    'this.isOwnPlugin = isOwnPlugin;',
    'this.pluginCompare = pluginCompare;',
  ].join('\n'),
  ctx,
  { timeout: 5000 },
)
const { isOwnPlugin, pluginCompare } = ctx

/** order exactly the way renderPlugins does */
const order = (arr, key = 'name', dir = 1) =>
  [...arr].sort((a, b) => pluginCompare(a, b, key, dir)).map((p) => p.name)

const CATALOG = [
  {
    name: 'File Gallery',
    author: 'Tophness and Neires',
    url: 'https://github.com/Tophness/file_gallery',
    group: 'community',
  },
  {
    name: 'Finetune Manager',
    author: 'GKartist75',
    url: 'https://github.com/GKArtist75/finetune_manager',
    group: 'community',
  },
  {
    name: 'H3 Director',
    author: 'The-ShadowWalker',
    url: 'https://github.com/The-ShadowWalker/h3',
    group: 'community',
  },
  {
    name: 'LoRA Manager',
    author: 'Tophness',
    url: 'https://github.com/Tophness/lora',
    group: 'community',
  },
  {
    name: 'Downloads',
    author: 'deepbeepmeep',
    url: '',
    group: 'system',
    system: true,
  },
]

test('our own plugin leads its group', () => {
  assert.deepEqual(order(CATALOG), [
    'Finetune Manager',
    'File Gallery',
    'H3 Director',
    'LoRA Manager',
    'Downloads',
  ])
})

test('the rest of the list keeps its alphabetical order', () => {
  const rest = order(CATALOG).filter((n) => n !== 'Finetune Manager' && n !== 'Downloads')
  assert.deepEqual(rest, ['File Gallery', 'H3 Director', 'LoRA Manager'])
})

test('the match is the author or the repo URL, case-insensitively', () => {
  assert.ok(isOwnPlugin({ author: 'GKArtist' }))
  assert.ok(isOwnPlugin({ author: 'gkartist75' }))
  assert.ok(isOwnPlugin({ author: 'someone', url: 'https://github.com/GKArtisT75/thing' }))
  assert.ok(!isOwnPlugin({ author: 'GK Fan', url: 'https://github.com/gkfan/thing' }))
  assert.ok(!isOwnPlugin({ author: 'deepbeepmeep' }))
  assert.ok(!isOwnPlugin({}))
  assert.ok(!isOwnPlugin(null))
})

test('a system plugin is never lifted above the community group', () => {
  const rows = [
    { name: 'About', author: 'GKartist75', group: 'system', system: true },
    { name: 'File Gallery', author: 'Tophness and Neires', group: 'community' },
  ]
  assert.deepEqual(
    order(rows),
    ['File Gallery', 'About'],
    'group comes before the own-plugin pin',
  )
})

test('the pin survives the column sorts the user can click', () => {
  const rows = [
    { name: 'File Gallery', author: 'Tophness', date: '2026-01-01', group: 'community' },
    {
      name: 'Finetune Manager',
      author: 'GKartist75',
      date: '2024-01-01',
      group: 'community',
    },
    { name: 'H3 Director', author: 'The-ShadowWalker', date: '2026-05-01', group: 'community' },
  ]
  assert.equal(order(rows, 'date', -1)[0], 'Finetune Manager', 'Latest still leads with ours')
  assert.equal(order(rows, 'author', 1)[0], 'Finetune Manager', 'Created by still leads with ours')
  assert.deepEqual(order(rows, 'date', -1).slice(1), ['H3 Director', 'File Gallery'])
  assert.deepEqual(order(rows, 'author', 1).slice(1), ['H3 Director', 'File Gallery'])
})

test('descending name flips the run under the pin, not the pin', () => {
  assert.deepEqual(order(CATALOG, 'name', -1).slice(0, 4), [
    'Finetune Manager',
    'LoRA Manager',
    'H3 Director',
    'File Gallery',
  ])
})

test('renderPlugins routes through pluginCompare and marks the pinned row', () => {
  const body = tab.slice(tab.indexOf('function renderPlugins'))
  assert.match(body, /sort\(\(a, b\) => pluginCompare\(a, b, key, dir\)\)/)
  assert.match(body, /plugin-own-badge/)
  const style = fs.readFileSync(path.join(__dirname, '..', 'src', 'style.css'), 'utf8')
  assert.match(style, /\.plugin-own-badge\s*\{/)
})

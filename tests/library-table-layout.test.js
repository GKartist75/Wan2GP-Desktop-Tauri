/**
 * library-table-layout.test.js — the Library checkpoint table lives in a
 * 600px side panel with four data columns plus a delete action. The generic `.args-table td:last-child
 * { width: 100% }` rule used to hand the whole table to Size, leaving the
 * Checkpoint and Origin columns at zero width; long filenames then wrapped one
 * character per line. Keep explicit column widths so that cannot regress.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')

const ROOT = path.join(__dirname, '..')
const css = fs.readFileSync(path.join(ROOT, 'src', 'style.css'), 'utf8')
const renderer = fs.readFileSync(path.join(ROOT, 'src', 'library-tab.js'), 'utf8')

function declarations(selector) {
  const at = css.indexOf(selector)
  assert.ok(at >= 0, `missing CSS rule: ${selector}`)
  const open = css.indexOf('{', at)
  const close = css.indexOf('}', open)
  assert.ok(open >= 0 && close > open, `unterminated CSS rule: ${selector}`)
  return css.slice(open + 1, close)
}

test('the checkpoint table gives Size a real width instead of 100%', () => {
  const compact = declarations('table.args-table.lib-table.compact {')
  assert.match(compact, /table-layout:\s*fixed/, 'fixed layout keeps data columns stable')
  assert.match(compact, /width:\s*100%/, 'the table still fills the panel')

  const size = declarations('table.args-table.lib-table.compact td:nth-child(4)')
  assert.match(
    size,
    /width:\s*82px/,
    'the last column must not inherit the generic 100% width',
  )

  const origin = declarations('table.args-table.lib-table.compact td:nth-child(2)')
  assert.match(origin, /width:\s*108px/, 'Origin needs enough room for DeepBeepMeep')
  const kind = declarations('table.args-table.lib-table.compact td:nth-child(3)')
  assert.match(kind, /width:\s*64px/, 'Kind is a short tag')
  const action = declarations('table.args-table.lib-table.compact td:nth-child(5)')
  assert.match(
    action,
    /width:\s*64px/,
    'the Delete column needs its own width, not the generic last-child 100%',
  )
})

test('long checkpoint names wrap in the remaining space, not per character', () => {
  const name = declarations('table.args-table.lib-table.compact td:first-child')
  assert.match(name, /width:\s*auto/, 'the name column absorbs the remaining width')
  assert.match(name, /overflow-wrap:\s*anywhere/, 'long filenames must break when needed')
  assert.ok(
    !name.includes('word-break: break-all'),
    'break-all is what turned the zero-width column into one character per line',
  )
})

test('the Library renderer still builds the compact checkpoint table', () => {
  assert.ok(
    renderer.includes('class="args-table lib-table compact"'),
    'the checkpoint table must keep the compact class',
  )
  assert.ok(
    renderer.includes('data-lib-model-del='),
    'each checkpoint row must carry its delete target',
  )
  assert.ok(
    renderer.includes('libraryModelDelete('),
    'the Delete button must call the backend command',
  )
  for (const heading of ['Checkpoint', 'Origin', 'Kind', 'Size']) {
    assert.ok(
      renderer.includes(`<strong>${heading}</strong>`),
      `missing the ${heading} column`,
    )
  }
})

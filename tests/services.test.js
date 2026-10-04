/**
 * services.test.js — coverage for the small pure services that had none.
 * All are CommonJS with no I/O, no Tauri and no DOM.
 */
const test = require('node:test')
const assert = require('node:assert')

const { normalizePipSpec } = require('../src/services/normalize-pip-spec.js')
const escHtml = require('../src/services/escape.js')
const { validatePublicUrl } = require('../src/services/net-validate.js')

test('normalizePipSpec in app.js matches the tested service body', () => {
  // The renderer has no module loader, so app.js carries its own inline copy
  // of this function — that copy is what runs. If the two drift, these tests
  // pass while the app behaves differently, which is exactly the failure mode
  // that made services/ go stale in the first place.
  const fs = require('node:fs')
  const path = require('node:path')
  const src = path.join(__dirname, '..', 'src')
  const vm = require('node:vm')
  // Extract each function's source and run it, rather than comparing text —
  // the two files differ in quote style and semicolons, and only behaviour
  // matters.
  const load = (file) => {
    const text = fs.readFileSync(file, 'utf8')
    const m = text.match(/function normalizePipSpec\s*\([^)]*\)\s*\{[\s\S]*?\n\}/)
    assert.ok(m, `normalizePipSpec not found in ${file}`)
    const box = {}
    vm.createContext(box)
    vm.runInContext(m[0] + '\nthis.fn = normalizePipSpec;', box, { timeout: 5000 })
    return box.fn
  }
  const inline = load(path.join(src, 'app.js'))
  const service = load(path.join(src, 'services', 'normalize-pip-spec.js'))
  const corpus = [
    'pip install claude-agent-sdk==0.1.66',
    'pip3 install foo',
    'python -m pip install foo',
    'py -m pip install foo',
    'PIP INSTALL foo',
    'pip install foo --upgrade',
    'pip install foo --no-deps -q',
    'foo==1.2.3',
    'foo',
    '  foo  ',
    '',
    'pip install',
    'pip install foo bar',
  ]
  for (const input of corpus) {
    assert.equal(
      inline(input),
      service(input),
      `normalizePipSpec diverges on ${JSON.stringify(input)}`
    )
  }
})

test('normalizePipSpec strips a pasted pip invocation', () => {
  assert.equal(normalizePipSpec('pip install claude-agent-sdk==0.1.66'), 'claude-agent-sdk==0.1.66')
  assert.equal(normalizePipSpec('pip3 install foo'), 'foo')
  assert.equal(normalizePipSpec('python -m pip install foo'), 'foo')
  assert.equal(normalizePipSpec('py -m pip install foo'), 'foo')
})

test('normalizePipSpec strips pip flags but keeps the spec', () => {
  assert.equal(normalizePipSpec('pip install foo --upgrade'), 'foo')
  assert.equal(normalizePipSpec('foo'), 'foo')
  assert.equal(normalizePipSpec(''), '')
  assert.equal(normalizePipSpec(null), '')
})

test('escHtml escapes every markup-significant character', () => {
  assert.equal(escHtml('<script>alert(1)</script>'),
    '&lt;script&gt;alert(1)&lt;/script&gt;')
  assert.equal(escHtml('a & b'), 'a &amp; b')
  assert.equal(escHtml(`"quoted"`), '&quot;quoted&quot;')
  assert.equal(escHtml("it's"), 'it&#39;s')
})

test('escHtml coerces non-strings and never emits a raw path', () => {
  assert.equal(escHtml(null), '')
  assert.equal(escHtml(undefined), '')
  assert.equal(escHtml(0), '0')
  assert.equal(escHtml(false), 'false')
  assert.equal(escHtml('<img src=x onerror=1>'),
    '&lt;img src=x onerror=1&gt;')
})

test('validatePublicUrl accepts a bare http(s) origin', () => {
  assert.deepEqual(validatePublicUrl('https://example.com'), {
    ok: true,
    normalized: 'https://example.com',
  })
  assert.deepEqual(validatePublicUrl('http://example.com:8080'), {
    ok: true,
    normalized: 'http://example.com:8080',
  })
  assert.equal(validatePublicUrl('https://example.com/').normalized, 'https://example.com')
})

test('validatePublicUrl rejects anything that is not a bare origin', () => {
  for (const bad of ['', '   ', 'ftp://example.com', 'example.com',
    'https://user:pw@example.com', 'https://example.com/deepy/',
    'https://example.com?a=1', 'https://example.com#x', 'https://exa mple.com']) {
    assert.equal(validatePublicUrl(bad).ok, false, `${bad} should be rejected`)
  }
})


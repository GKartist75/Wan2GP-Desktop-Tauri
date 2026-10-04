/**
 * pip-spec.test.js — the accept/reject table for the pip safety validator.
 * This is a security boundary: the Rust side (features.rs pip_spec_ok) makes
 * the same decision, so a divergence here means the preview and the install
 * handler disagree about what is safe.
 */
const test = require('node:test')
const assert = require('node:assert')
const { assertSafePipSpec } = require('../src/services/pip-spec.js')

const ok = (spec) => assert.equal(assertSafePipSpec(spec).ok, true, `${spec} should be accepted`)
const no = (spec, reason) => {
  const r = assertSafePipSpec(spec)
  assert.equal(r.ok, false, `${spec} should be rejected`)
  if (reason !== undefined) assert.equal(r.reason, reason)
}

test('accepts bare distribution names', () => {
  ok('claude-agent-sdk')
  ok('nunchaku')
  ok('lightx2v_kernel')
  ok('a')
})

test('accepts every PEP 440 comparison operator', () => {
  ok('claude-agent-sdk==0.1.66')
  ok('torch>=2.10')
  ok('torch<=2.9')
  ok('torch~=2.9.0')
  ok('torch!=2.8.0')
  ok('torch>2.9')
  ok('torch<3.0')
})

test('accepts direct https wheel installs from Wan2GP docs', () => {
  const u = 'https://download.pytorch.org/whl/gguf/llamacpp_gguf_cuda-1.0.25-cp311-cp311-win_amd64.whl'
  assert.deepEqual(assertSafePipSpec(u), { ok: true, name: u })
  ok('https://download.pytorch.org/whl/lightx2v/sdist.tar.gz')
})

test('rejects empty and non-string input', () => {
  no('', 'empty')
  no(null, 'empty')
  no(undefined, 'empty')
  no(42, 'empty')
})

test('rejects shell metacharacters', () => {
  for (const bad of [
    'foo;rm -rf /',
    'foo&calc',
    'foo|tee out',
    'foo$(id)',
    'foo`id`',
    "foo'bar",
    'foo"bar',
    'foo{bar}',
    'foo(bar)',
    'foo bar',
    'foo\nbar',
    'foo\tbar',
  ]) {
    assert.equal(assertSafePipSpec(bad).reason, 'unsafe-characters', bad)
  }
})

test('rejects pip options the user must not smuggle through', () => {
  // Each is rejected, though by a different guard: option-shaped tokens fail the
  // name check, whitespace fails the metacharacter check, and a URL smuggled in
  // behind an option fails the slash check.
  no('--index-url=https://evil.example.com', 'slash-not-allowed')
  no('-rrequirements.txt', 'bad-name')
  no('foo==1.0 --no-deps', 'unsafe-characters')
  no('--trusted-host', 'bad-name')
})

test('rejects path separators and option-shaped names', () => {
  assert.equal(assertSafePipSpec('foo/bar').reason, 'slash-not-allowed')
  assert.equal(assertSafePipSpec('..\\foo').reason, 'malformed')
  assert.equal(assertSafePipSpec('-foo').reason, 'bad-name')
  assert.equal(assertSafePipSpec('1foo').reason, 'bad-name')
})

test('rejects an operator with no version', () => {
  assert.equal(assertSafePipSpec('torch>=').reason, 'op-without-version')
})

test('rejects non-https and non-wheel URLs', () => {
  // http:// never reaches the URL branch (that branch is https-only), so it is
  // caught by the slash guard first.
  assert.equal(assertSafePipSpec('http://evil.example.com/x.whl').reason, 'slash-not-allowed')
  assert.equal(assertSafePipSpec('https://evil.example.com/x.exe').reason, 'bad-url')
  // A second query parameter needs '&', which the metacharacter guard rejects.
  assert.equal(
    assertSafePipSpec('https://evil.example.com/x.whl?a=1&b=2').reason,
    'unsafe-characters'
  )
})
/**
 * backend-parse.test.js — coverage for services/parse-backend.js.
 * Mirrors the real boot lines WanGP prints (with child-tag prefixes).
 */
const test = require('node:test')
const assert = require('node:assert')

const parseBackendLine = require('../src/services/parse-backend.js')

test('parses INT8 backend with setting suffix', () => {
  assert.deepStrictEqual(
    parseBackendLine('[wan2gp] [INT8] Backend: WGPKernels (setting: auto).'),
    { kind: 'INT8', name: 'WGPKernels' },
  )
})

test('parses FP8 backend with multi-word names', () => {
  assert.deepStrictEqual(
    parseBackendLine('[FP8] Backend: Comfy Kitchen CUDA (setting: auto).'),
    { kind: 'FP8', name: 'Comfy Kitchen CUDA' },
  )
  assert.deepStrictEqual(
    parseBackendLine('[FP8] Backend: CUDA FP8 Tensor Cores (setting: auto; extra).'),
    { kind: 'FP8', name: 'CUDA FP8 Tensor Cores' },
  )
})

test('rejects non-report lines and empty names', () => {
  assert.strictEqual(parseBackendLine('[INT8] Backend: (setting: auto).'), null)
  assert.strictEqual(parseBackendLine('[RAM] MMGP RAM Allocator: ok'), null)
  assert.strictEqual(parseBackendLine(''), null)
  assert.strictEqual(parseBackendLine(null), null)
  assert.strictEqual(parseBackendLine('[int8] backend: wgpkernels (setting: auto)'), null)
})

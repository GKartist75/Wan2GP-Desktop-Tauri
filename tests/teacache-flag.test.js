/**
 * teacache-flag.test.js — current WanGP removed the --teacache launch flag.
 *
 * shared/cli_args.py uses argparse.parse_args(), and its parser no longer
 * defines --teacache. Passing it makes wgp.py exit with
 * "unrecognized arguments: --teacache". Step skipping is now a generation
 * setting: skip_steps_cache_type + skip_steps_multiplier under
 * Advanced Mode -> Steps Skipping.
 *
 * The launcher must not stage the removed flag in a preset, patch it from a
 * control, or write it from the emergency failsafe. Explanatory text is fine.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')

const ROOT = path.join(__dirname, '..')
const read = (p) => fs.readFileSync(path.join(ROOT, p), 'utf8')

test('launch presets do not emit the removed --teacache flag', () => {
  const presets = read('src/services/launch-args.js')
  const block = presets.match(/const LAUNCH_PRESETS = \{[\s\S]*?\n\}/)
  assert.ok(block, 'LAUNCH_PRESETS block missing')
  assert.doesNotMatch(block[0], /--teacache/)
  assert.match(block[0], /emergency:\s*'--attention sdpa --profile 4 --fp16'/)
  assert.match(block[0], /lowvram:\s*'--attention sdpa --profile 5 --fp16'/)
  assert.match(block[0], /maxperf:\s*'--compile --attention sage2 --profile 3'/)
})

test('the launch builder has no TeaCache control or patch', () => {
  const builder = read('src/launch-builder.js')
  assert.doesNotMatch(builder, /lbTeacache|--teacache/)
  const html = read('src/index.html')
  assert.doesNotMatch(html, /id="lbTeacache"/)
  assert.doesNotMatch(html, /data-launch-preset="[^"]+"[^>]*teacache/i)
  assert.match(html, /Advanced Mode → Steps Skipping/)
})

test('the emergency failsafe and launch splitter do not use --teacache', () => {
  for (const file of ['src-tauri/src/troubleshoot.rs', 'src-tauri/src/launch.rs']) {
    assert.doesNotMatch(read(file), /--teacache/, `${file} still mentions the removed flag`)
  }
})

test('the guide explains where step skipping moved', () => {
  const guide = read('docs/WAN2GP-GUIDE.md')
  assert.match(guide, /TeaCache is no longer a launch flag/)
  assert.match(guide, /skip_steps_cache_type/)
  assert.match(guide, /skip_steps_multiplier/)
})

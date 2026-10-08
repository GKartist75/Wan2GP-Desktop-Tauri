/**
 * console-seed.test.js — the floating console must show the SAME log as the
 * docked one, on first open and after every dock switch.
 *
 * It used to seed itself from the backend's LOG_HISTORY. That ring buffer is
 * raw text: `base::push_log` splits `\r` into separate lines and drops tqdm
 * fragments, so the replay was shorter and differently collapsed than the
 * buffer the docked console renders from — which reads as "floating lost my
 * logs", and got further apart with each dock switch. The main window owns the
 * rendered buffer, so the term window now asks the owner for it.
 *
 * The seed crosses a window boundary (term → Rust → main → Rust → term), so
 * these pin the wiring on both sides plus the one thing that can silently lose
 * it: the request must come FROM the term window, not be pushed at creation
 * time — a push can land before term.js has registered its listener.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')

const SRC = path.join(__dirname, '..', 'src')
const appJs = fs.readFileSync(path.join(SRC, 'app.js'), 'utf8')
const termJs = fs.readFileSync(path.join(SRC, 'term.js'), 'utf8')
const w2gp = fs.readFileSync(path.join(SRC, 'w2gp.js'), 'utf8')
const systemRs = fs.readFileSync(path.join(__dirname, '..', 'src-tauri', 'src', 'system.rs'), 'utf8')
const libRs = fs.readFileSync(path.join(__dirname, '..', 'src-tauri', 'src', 'lib.rs'), 'utf8')

test('the term window asks for the seed instead of only replaying the ring buffer', () => {
  assert.match(termJs, /requestConsoleSeed\(\)/, 'the term window must request the log it should show')
  assert.match(termJs, /onTermConsoleSeed\(/, 'and must listen for the answer')
  // The fallback stays, but it can no longer win a race: it is behind a flag.
  assert.match(termJs, /if \(_seeded\) return;/, 'the history fallback must not overwrite a seed that already arrived')
})

test('the seed handler replaces the buffer, not appends to it', () => {
  // Docked → floating → docked → floating re-opens the window each time; an
  // append would duplicate the whole log on every switch.
  assert.match(termJs, /buf = \(Array\.isArray\(lines\) \? lines : \[\]\)/, 'the seed must replace buf, not extend it')
  assert.doesNotMatch(termJs, /for \(const entry of entries\)[\s\S]{0,200}appendToBuf[\s\S]{0,400}_seeded = true/, 'the replay path must not set _seeded')
})

test('the main window answers with its rendered buffer', () => {
  assert.match(appJs, /window\._getLogAll = \(\) => logBuffer\.slice\(\)/, 'the owner must expose its rendered buffer')
  assert.match(
    appJs,
    /onTermRequestSeed\([\s\S]{0,200}termConsoleSeed\(window\._getLogAll\(\)\)/,
    'a seed request must be answered with that buffer',
  )
})

test('the request is routed through the main window, not pushed at creation', () => {
  // A push at create_term_view time races term.js's listener registration: the
  // webview loads asynchronously, so the emit can arrive before the listener
  // exists and the console starts empty with no error anywhere.
  assert.match(systemRs, /fn request_console_seed[\s\S]{0,200}emit_to\("main", "term-request-seed"/, 'the term window must trigger the request')
  assert.match(systemRs, /fn term_console_seed[\s\S]{0,300}emit_to\(TERM_LABEL, "term-console-seed"/, 'the answer must go only to the console window')
  assert.doesNotMatch(systemRs, /fn create_term_view[\s\S]{0,1200}emit_to\(TERM_LABEL, "term-console-seed"/, 'create_term_view must not push a seed')
})

test('both commands are registered', () => {
  assert.match(libRs, /system::request_console_seed,/, 'request_console_seed must be registered')
  assert.match(libRs, /system::term_console_seed,/, 'term_console_seed must be registered')
  for (const bridge of ['requestConsoleSeed', 'termConsoleSeed', 'onTermRequestSeed', 'onTermConsoleSeed']) {
    assert.ok(w2gp.includes(bridge), `w2gp.js must expose ${bridge}`)
  }
})
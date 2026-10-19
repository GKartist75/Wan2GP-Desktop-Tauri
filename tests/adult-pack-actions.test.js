/**
 * adult-pack-actions.test.js — the wiring that acts on a row: fetch, update,
 * add and delete.
 *
 * These are the paths where a silent no-op looks like success, so each guard
 * is pinned here as well as in the Rust behavioural tests:
 *   1. "Apply updates" must actually download (no nested busy guard)
 *   2. a downloaded HIGH/LOW half must verify against its own pin
 *   3. user-added entries are resolved by fetch / write / remove, like the pack
 *   4. adding a checkpoint ticks the checkpoint selection, not the LoRA one
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')

const ROOT = path.join(__dirname, '..')
const SRC = path.join(ROOT, 'src')
const rust = fs.readFileSync(path.join(ROOT, 'src-tauri', 'src', 'adult_pack.rs'), 'utf8')
const renderer = fs.readFileSync(path.join(SRC, 'adult-pack-tab.js'), 'utf8')
const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')

test('apply-updates downloads without re-entering the busy guard', () => {
  // `withBusy` is re-entrant-hostile: a nested call returns immediately while
  // the outer one holds the flag. Fetching has to go through the unwrapped
  // worker or "Apply updates" only writes pins.
  const apply = renderer.slice(
    renderer.indexOf('async function applyUpdates()'),
    renderer.indexOf('async function fetchNow('),
  )
  assert.ok(apply.includes('fetchNow(ids)'), 'apply must call the unwrapped fetch')
  assert.ok(!apply.includes('await doFetch('), 'apply must not call the guarded doFetch')

  const worker = renderer.slice(renderer.indexOf('async function fetchNow('))
  const body = worker.slice(0, worker.indexOf('async function doFetch('))
  assert.ok(!body.includes('withBusy('), 'the worker must not take the busy guard')
  assert.ok(body.includes('adultPackFetch(ids)'), 'the worker does the real fetch')

  const doFetch = worker.slice(worker.indexOf('async function doFetch('))
  assert.ok(
    /withBusy\(label \+ "…", \(\) => fetchNow\(ids\)\)/.test(doFetch),
    'the public doFetch wraps the worker in the guard',
  )
})

test('user additions are resolved alongside the bundled catalog', () => {
  for (const cmd of ['adult_pack_fetch', 'adult_pack_write_finetunes', 'adult_pack_remove']) {
    const fn = (rust.split(`pub async fn ${cmd}`)[1] || rust.split(`pub fn ${cmd}`)[1] || '').split(
      '\n#[tauri',
    )[0]
    assert.ok(fn.includes('find_merged('), `${cmd} must see user-added entries too`)
  }
  assert.ok(rust.includes('fn find_merged<'), 'the merge helper must exist')
  assert.ok(rust.includes('fn drop_custom_entry('), 'removing an entry must also forget it')
  assert.ok(
    renderer.includes('res.kind === "lora" ? sel.loras[res.id] : sel.fts[res.id]') ||
      /res\.kind === "lora"[\s\S]{0,80}sel\.loras\[res\.id\][\s\S]{0,80}sel\.fts\[res\.id\]/.test(renderer),
    'adding a checkpoint must tick the finetune selection',
  )
})

test('a HIGH/LOW half is fetched and pinned as its own file', () => {
  assert.ok(rust.includes('fn finetune_target('), 'the #low id shape must be decoded')
  const fetchFn = (rust.split('pub async fn adult_pack_fetch')[1] || '').split('\n#[tauri')[0]
  assert.ok(fetchFn.includes('find_merged(m, &custom, "finetunes", parent)'), 'low half resolves its parent')
  assert.ok(fetchFn.includes('effective_pin('), 'fetch must honor a local pin override')
  assert.ok(/format!\("\{parent\}#low"\)/.test(fetchFn), 'the low half has its own pin key')
})

test('a user-added file keeps a safe basename, not the repo path', () => {
  const fn = (rust.split('pub fn adult_pack_add')[1] || '').split('\npub ')[0]
  assert.ok(fn.includes('basename(&repo_path)'), 'the catalog path must be reduced to a file name')
  assert.ok(fn.includes('safe_file_name(&file)'), 'the stored name is still checked')
})

test('CivitAI downloads use the real file name and published hash', () => {
  assert.ok(rust.includes('fn civitai_file_name('), 'the endpoint is not a file name')
  assert.ok(rust.includes('fn civitai_sha('), 'the published SHA-256 must be used')
  const fn = (rust.split('pub async fn adult_pack_civitai_download')[1] || '').split('\n#[tauri')[0]
  assert.ok(fn.includes('civitai_file_name(f, dl)'), 'name comes from the file object')
  assert.ok(fn.includes('civitai_sha(f)'), 'verification uses the file hash')
  assert.ok(fn.includes('local_paths'), 'recipes record where the bytes landed')
})

test('the source-page action cannot open a non-https scheme', () => {
  const act = renderer.slice(
    renderer.indexOf('const page = item.filePage'),
    renderer.indexOf('const btn = rowEl("ap-act-btn"'),
  )
  assert.ok(!/openExternal\(page\)/.test(act), 'the raw page string must not go to openExternal')
  assert.ok(act.includes('httpsUrl(page)'), 'the scheme must be checked first')
})

test('the finetune manager hides the adultpack_ prefix when de-duplicating', () => {
  assert.ok(
    rust.includes('strip_prefix("adultpack_")'),
    'the directory scan must recognize pack recipe file names',
  )
})

test('a download batch is drawn as a queue, not inferred from the log', () => {
  // Backend: every event carries its batch position, and per-item work starts
  // with an explicit `start` so a row appears before the first byte arrives.
  assert.ok(rust.includes('struct QueueSink'), 'the position stamping lives in one place')
  const sink = (rust.split('struct QueueSink')[1] || '').split('\nfn http_client')[0]
  assert.ok(sink.includes('"queueIndex"'), 'the index must not be called `index`')
  assert.ok(
    sink.includes('"queueTotal"'),
    'the batch size must not reuse `total`, which is the byte count',
  )
  const fetchFn = (rust.split('pub async fn adult_pack_fetch')[1] || '').split('\n#[tauri')[0]
  assert.ok(fetchFn.includes('let total = ids.len()'), 'the batch size is captured before consumption')
  assert.ok(
    /for \(index, id\) in ids\.into_iter\(\)\.enumerate\(\)/.test(fetchFn),
    'each id must know its position',
  )
  assert.ok(
    (fetchFn.match(/phase": "start"/g) || []).length === 2,
    'both the LoRA and checkpoint paths announce the item they start',
  )

  // Renderer: the queue is fed by the same events, and a row is one ticked item
  // (a HIGH/LOW recipe emits twice but must not become two rows).
  for (const fn of ['function startQueue(', 'function updateQueue(', 'function finishQueue(', 'function renderQueue(']) {
    assert.ok(renderer.includes(fn), `missing ${fn}`)
  }
  const upd = (renderer.split('function updateQueue(')[1] || '').split('\n  /**')[0]
  assert.ok(upd.includes('queue.byId.get(ev.id)'), 'rows are keyed by the selected item')
  assert.ok(upd.includes('queue.byId.set(ev.id, row)'), 'an unannounced id still gets a row')
  assert.ok(
    upd.includes('queue.active'),
    'a finished batch must not absorb CivitAI or update-check events',
  )
  const fin = (renderer.split('function finishQueue(')[1] || '').split('\n  function renderQueue')[0]
  assert.ok(fin.includes('"skipped"'), 'an item that never started is not called a failure')
  assert.ok(fin.includes('"failed"'), 'an interrupted item is')
  assert.ok(
    html.includes('id="apQueue"'),
    'the queue needs a host in the Downloads section',
  )
  assert.ok(
    renderer.includes('updateQueue(ev)'),
    'progress events must reach the queue, not only the status line',
  )
  // The progress line must not re-read the batch size as a byte total.
  assert.ok(
    renderer.includes('ev.queueIndex') && renderer.includes('ev.queueTotal'),
    'the status line shows [i/n] from the queue fields',
  )
})

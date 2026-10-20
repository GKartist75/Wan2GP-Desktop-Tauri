/**
 * adult-pack.test.js — locks the contract of the opt-in adult model pack.
 *
 * The pack is data, not code: scripts/adult-pack-sources.json says which repos
 * belong in it, scripts/verify-adult-pack.mjs resolves and pins them into
 * src/adult-pack-manifest.json, and the Rust module embeds that manifest. These
 * tests guard the seams where a hand-edit would silently ship something broken:
 *
 *   1. schema + resolvability of every manifest entry (URL, size, sha, safe name)
 *   2. HIGH/LOW halves are never selected apart (Wan2GP loads both in one job)
 *   3. no model data is hardcoded in the renderer (the panel must stay generic)
 *   4. the panel is wired into the install flow and gated behind 18+
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')

const ROOT = path.join(__dirname, '..')
const SRC = path.join(ROOT, 'src')
const MANIFEST = path.join(SRC, 'adult-pack-manifest.json')
const SOURCES = path.join(ROOT, 'scripts', 'adult-pack-sources.json')

const manifest = JSON.parse(fs.readFileSync(MANIFEST, 'utf8'))
const sources = JSON.parse(fs.readFileSync(SOURCES, 'utf8'))

// ── manifest shape ─────────────────────────────────────────────────────────

test('manifest has the tiers the installer renders', () => {
  assert.ok(manifest.manifestVersion >= 1, 'manifestVersion must be set')
  assert.ok(manifest.groups.length >= 4, 'expected a group per model family')
  assert.ok(manifest.loras.length > 50, `expected a populated lora tier, got ${manifest.loras.length}`)
  assert.ok(manifest.finetunes.length >= 8, `expected a checkpoint tier, got ${manifest.finetunes.length}`)
  assert.ok(manifest.legal.length >= 2, 'legal notes must ship with the pack')
  assert.ok(manifest.totals && manifest.totals.loraDefaultBytes > 0, 'totals must be precomputed')
})

test('every lora entry is fetchable and safely named', () => {
  for (const l of manifest.loras) {
    assert.ok(l.id, 'entry needs an id')
    assert.ok(l.url && l.url.startsWith('https://'), `${l.id}: must be an https URL`)
    assert.ok(l.bytes > 0, `${l.id}: needs a real byte size`)
    assert.ok(l.sha256 && /^[0-9a-f]{64}$/.test(l.sha256), `${l.id}: needs a sha256`)
    assert.ok(l.group, `${l.id}: needs a group`)
    assert.ok(l.family, `${l.id}: needs a family`)
    assert.ok(l.wan2gpIds && l.wan2gpIds.length, `${l.id}: needs a Wan2GP architecture id`)
    // The launcher writes these into loras/<collection>/adult-pack/ — a path or a
    // Win32-reserved character in the name would escape or fail the write.
    assert.ok(!/[\\/]/.test(l.file), `${l.id}: file must be a bare name, got ${l.file}`)
    assert.ok(!/[<>:"|?*]/.test(l.file), `${l.id}: file has reserved characters`)
    assert.ok(!l.file.endsWith('.') && !l.file.endsWith(' '), `${l.id}: trailing dot/space`)
    assert.ok(l.file.length <= 180, `${l.id}: file name too long`)
  }
})

test('every finetune entry maps to a real Wan2GP architecture', () => {
  for (const f of manifest.finetunes) {
    assert.ok(f.id, 'entry needs an id')
    assert.ok(f.architecture, `${f.id}: architecture is what Wan2GP loads`)
    assert.ok(f.name, `${f.id}: needs a display name`)
    assert.ok(Array.isArray(f.urls) && f.urls.length, `${f.id}: needs URLs`)
    for (const u of f.urls) assert.ok(u.startsWith('https://'), `${f.id}: ${u}`)
    assert.ok(f.bytes > 0, `${f.id}: needs a byte size`)
    assert.ok(f.sha256 && /^[0-9a-f]{64}$/.test(f.sha256), `${f.id}: needs a sha256`)
    // HIGH/LOW pairs carry the second phase in URLs2.
    if (f.urls2) {
      assert.ok(f.urls2.every((u) => u.startsWith('https://')), `${f.id}: bad URLs2`)
      // Sized separately: summing the halves would make each file fail its own
      // size check when prefetched (both would expect the pair's total).
      assert.ok(f.bytesLow > 0, `${f.id}: URLs2 present but bytesLow missing`)
      assert.ok(f.sha256Low && /^[0-9a-f]{64}$/.test(f.sha256Low), `${f.id}: URLs2 present but sha256Low missing`)
    }
  }
})

test('civitai tier is token-gated, resolvable and never pre-selected', () => {
  for (const c of manifest.civitai) {
    assert.ok(c.id && c.name, 'entry needs id + name')
    assert.ok(c.licenseNote, `${c.id}: token-gated items must carry a licence note`)
    assert.strictEqual(c.defaultSelected, false, `${c.id}: must never be selected by default`)
    // The runtime resolves today's release by search — a pinned id would go
    // stale the moment the author publishes again.
    assert.ok(c.query, `${c.id}: needs a CivitAI search query`)
    assert.ok(c.baseModelFilter, `${c.id}: must filter by base model (wrong family = useless file)`)
  }
})

// ── HIGH/LOW pairing ────────────────────────────────────────────────────────

// Wan 2.2 loads the HIGH and LOW halves in the same job, so shipping one
// without the other produces a silently broken recipe. This is the one rule
// the verifier had to be taught twice.
test('selected HIGH/LOW halves always ship together', () => {
  const bySource = new Map()
  for (const l of manifest.loras) {
    if (!l.pair) continue
    const key = `${l.sourceId}::${l.file
      .toLowerCase()
      .replace(/high(_?noise)?|low(_?noise)?/g, '~')
      .replace(/[-_]h([-_]|$)/g, '-~-')
      .replace(/[-_]l([-_]|$)/g, '-~-')
      .replace(/[^a-z0-9~]+/g, '')}`
    if (!bySource.has(key)) bySource.set(key, [])
    bySource.get(key).push(l)
  }
  let pairs = 0
  for (const [, group] of bySource) {
    if (group.length < 2) continue
    pairs++
    const kinds = new Set(group.map((g) => g.pair))
    assert.ok(kinds.has('high') && kinds.has('low'), `unbalanced pair: ${group.map((g) => g.file)}`)
    const selected = group.filter((g) => g.defaultSelected).length
    assert.ok(
      selected === 0 || selected === group.length,
      `half-selected pair: ${group.map((g) => `${g.file}=${g.defaultSelected}`).join(', ')}`,
    )
  }
  assert.ok(pairs > 5, `expected several HIGH/LOW pairs, found ${pairs}`)
})

// ── data, not code ──────────────────────────────────────────────────────────

// The whole maintenance story is "edit JSON, ship nothing else". If a model
// name ever lands in the renderer, that promise is broken.
test('renderer holds no model data — every row comes from the manifest', () => {
  const js = fs.readFileSync(path.join(SRC, 'adult-pack-tab.js'), 'utf8')
  for (const l of manifest.loras.slice(0, 40)) {
    const name = l.file.replace(/\.(safetensors|gguf)$/i, '')
    if (name.length < 8) continue
    assert.ok(!js.includes(name), `adult-pack-tab.js hardcodes "${name}" — move it to the manifest`)
  }
  assert.ok(!/https:\/\/(huggingface|civitai)/.test(js), 'no hardcoded source URLs in the renderer')
  // The panel must read every list through the manifest payload.
  assert.ok(js.includes('pack.loras'), 'loras must be read from the manifest')
  assert.ok(js.includes('pack.finetunes'), 'finetunes must be read from the manifest')
})

test('sources file drives the manifest (no orphan entries)', () => {
  const sourceIds = new Set((sources.loras || []).map((s) => s.id))
  for (const l of manifest.loras) {
    assert.ok(sourceIds.has(l.sourceId), `${l.id}: sourceId ${l.sourceId} is not in the sources file`)
  }
  const ftIds = new Set((sources.finetunes || []).map((s) => s.id))
  for (const f of manifest.finetunes) {
    assert.ok(ftIds.has(f.id), `${f.id}: finetune id missing from sources`)
  }
})

// ── wiring ──────────────────────────────────────────────────────────────────

test('pack lives in its own Manage tab, never in the installer', () => {
  const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
  assert.ok(html.includes('data-tab="pack"'), 'Model Pack tab missing')
  assert.ok(
    html.includes('data-tab="pack"') && html.includes('adult-pack-tab.js'),
    'panel script not loaded',
  )
  // The whole point of moving it out of Setup: a fresh install must never carry
  // an opt-in adult-content decision.
  const installer = html.slice(html.indexOf('id="installer"'), html.indexOf('id="installer"') + 20000)
  assert.ok(!installer.includes('id="apEnabled"'), 'the pack must NOT live on the installer screen')
  assert.ok(!installer.includes('adult-pack-tab.js'), 'installer must not pull the pack in')

  const inst = fs.readFileSync(path.join(SRC, 'installer-tab.js'), 'utf8')
  assert.ok(!inst.includes('adultPack'), 'installer flow must not reference the pack at all')

  // Load order: the panel script must come after app.js (it uses errText).
  const appAt = html.indexOf('src="app.js"')
  const packAt = html.indexOf('src="adult-pack-tab.js"')
  assert.ok(packAt > appAt, 'adult-pack-tab.js must load after app.js')
})

test('the 18+ gate gates every row, and the tab can act without the installer', () => {
  const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
  assert.ok(html.includes('id="apAge"'), '18+ confirmation checkbox missing')
  assert.ok(html.includes('id="apEnabled"'), 'parent checkbox missing')
  const js = fs.readFileSync(path.join(SRC, 'adult-pack-tab.js'), 'utf8')
  // Rows stay hidden until BOTH the parent tick and the age confirmation exist.
  assert.ok(/const active = \(\) => enabled\(\) && ageOk\(\)/.test(js), 'no combined enable+age gate')
  assert.ok(/if \(!active\(\)\)[\s\S]{0,200}display = "none"/.test(js), 'rows must hide without the gate')
  // On-demand management, not install-time: download / recipes / remove exist.
  for (const fn of ['adultPackFetch', 'adultPackWriteFinetunes', 'adultPackRemove']) {
    assert.ok(js.includes(fn), `tab must be able to call ${fn}`)
  }
  assert.ok(html.includes('id="apDownloadBtn"') && html.includes('id="apRemoveBtn"'), 'tab actions missing')
})

test('rust module exposes the pack commands and embeds the manifest', () => {
  const rs = fs.readFileSync(path.join(ROOT, 'src-tauri', 'src', 'adult_pack.rs'), 'utf8')
  assert.ok(rs.includes('include_str!("../../src/adult-pack-manifest.json")'), 'manifest must be embedded')
  for (const cmd of [
    'adult_pack_manifest',
    'adult_pack_save',
    'adult_pack_fetch',
    'adult_pack_write_finetunes',
    'adult_pack_civitai_verify',
    'adult_pack_civitai_download',
    'adult_pack_status',
    'adult_pack_remove',
  ]) {
    assert.ok(rs.includes(cmd), `missing command ${cmd}`)
    const lib = fs.readFileSync(path.join(ROOT, 'src-tauri', 'src', 'lib.rs'), 'utf8')
    assert.ok(lib.includes(`adult_pack::${cmd}`), `${cmd} not registered in lib.rs`)
  }
})
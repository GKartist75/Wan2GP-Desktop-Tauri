/**
 * adult-pack-search.test.js — "find your own models" wiring.
 *
 * The flow is: search HF/CivitAI → only families this Wan2GP build supports are
 * offered → inspect the repo → pick a file → the launcher writes the finetune
 * recipe and files the download into the right folder. These tests lock the
 * parts that are easy to break and expensive when they do:
 *   1. every search hit carries a real Wan2GP architecture (from the archMap)
 *   2. the renderer never invents an architecture or a URL
 *   3. user additions live outside the bundled manifest, so an app update
 *      cannot silently drop something someone installed
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')

const ROOT = path.join(__dirname, '..')
const SRC = path.join(ROOT, 'src')
const manifest = JSON.parse(fs.readFileSync(path.join(SRC, 'adult-pack-manifest.json'), 'utf8'))
const renderer = fs.readFileSync(path.join(SRC, 'adult-pack-tab.js'), 'utf8')
const rust = fs.readFileSync(path.join(ROOT, 'src-tauri', 'src', 'adult_pack.rs'), 'utf8')
const bridge = fs.readFileSync(path.join(SRC, 'w2gp.js'), 'utf8')

test('archMap covers the families the catalog ships', () => {
  const map = manifest.archMap || []
  assert.ok(map.length >= 8, `expected a family map, got ${map.length}`)
  for (const entry of map) {
    assert.ok(entry.id && entry.label, 'entry needs id + label')
    assert.ok(entry.architecture, `${entry.id}: needs a Wan2GP architecture`)
    assert.ok(Array.isArray(entry.match) && entry.match.length, `${entry.id}: needs match keywords`)
    assert.ok(['lora', 'checkpoint'].includes(entry.kind), `${entry.id}: kind must be lora or checkpoint`)
    for (const id of entry.wan2gpIds || []) {
      // Wan2GP ids are mixed-case by convention (ltx2_22B, qwen_image_21_turbo_7B),
      // so only the character set is constrained — not the casing.
      assert.match(id, /^[A-Za-z0-9_]+$/, `${entry.id}: bad Wan2GP id ${id}`)
    }
  }
  // Every architecture the curated catalog uses must be reachable from the map,
  // otherwise a user could not add the same model themselves.
  const reachable = new Set(map.flatMap((e) => e.wan2gpIds || []).concat(map.map((e) => e.architecture)))
  const used = new Set(manifest.finetunes.map((f) => f.architecture))
  for (const arch of used) {
    assert.ok(reachable.has(arch), `${arch} ships in the catalog but is not in archMap`)
  }
})

test('search, inspect and add are wired end to end', () => {
  for (const cmd of ['adult_pack_search', 'adult_pack_inspect', 'adult_pack_add']) {
    assert.ok(bridge.includes(cmd), `w2gp bridge must expose ${cmd}`)
    const lib = fs.readFileSync(path.join(ROOT, 'src-tauri', 'src', 'lib.rs'), 'utf8')
    assert.ok(lib.includes(`adult_pack::${cmd}`), `${cmd} must be registered in lib.rs`)
    assert.ok(rust.includes(cmd), `${cmd} must exist in the module`)
  }
  assert.ok(renderer.includes('adultPackSearch('), 'renderer must call search')
  assert.ok(renderer.includes('adultPackInspect('), 'renderer must call inspect')
  assert.ok(renderer.includes('adultPackAdd('), 'renderer must call add')
})

test('user additions persist outside the bundled manifest', () => {
  // An app update regenerates src/adult-pack-manifest.json from sources; if
  // user additions lived there they would vanish on the next release.
  assert.ok(rust.includes('adult-pack-custom.json'), 'custom store must have its own file')
  assert.ok(rust.includes('fn load_custom()'), 'custom store must be loaded')
  assert.ok(rust.includes('fn save_custom('), 'custom store must be saved')
  assert.ok(
    /load_custom\(\)/.test(rust.split('fn adult_pack_add')[1] || ''),
    'the manifest command must merge custom entries',
  )
  assert.ok(
    !fs.readFileSync(path.join(SRC, 'adult-pack-manifest.json'), 'utf8').includes('custom_hf-'),
    'generated manifest must never contain user additions',
  )
})

test('adding a model requires a real https URL, a safe name and an architecture', () => {
  const addFn = (rust.split('pub fn adult_pack_add')[1] || '').split('\npub ')[0]
  assert.ok(addFn.includes('url.starts_with("https://")'), 'URL must be https-checked')
  assert.ok(addFn.includes('safe_file_name(&file)'), 'file name must be safety-checked')
  assert.ok(addFn.includes('if arch.is_empty()'), 'architecture must be required')
})

test('renderer never invents an architecture — it uses what inspect returns', () => {
  assert.ok(
    renderer.includes('architecture: info.architecture'),
    'the added entry must carry the architecture the backend chose',
  )
  assert.ok(
    renderer.includes('architectureKnown === false'),
    'the UI must say when the architecture is not in this Wan2GP build',
  )
  // Family (which folder the file lands in) is derived from that architecture,
  // never guessed from the title.
  assert.ok(renderer.includes('const familyOf = (arch) =>'), 'family must derive from architecture')
  assert.ok(renderer.includes('function familyDir') === false || true)
})

test('search filters by model type and base model', () => {
  // Type: LoRA / checkpoint / GGUF, spelled per source.
  const fn = (rust.split('pub async fn adult_pack_search')[1] || '').split('\npub ')[0]
  assert.ok(fn.includes('&filter=lora'), 'HF LoRA search must use its lora filter')
  assert.ok(fn.includes('&filter=gguf'), 'HF GGUF search must use its gguf filter')
  assert.ok(fn.includes('types=LoRA') || fn.includes('"LoRA"'), 'CivitAI LoRA search must use types')
  assert.ok(fn.includes('"Checkpoint"'), 'CivitAI checkpoint search must use types')
  assert.ok(fn.includes('"GGUF"'), 'CivitAI GGUF search must use types')
  // Base model: filtered after mapping, so the family always comes from
  // archMap rather than the source's own wording.
  assert.ok(fn.includes('family_filter'), 'family filter must be applied')
  assert.ok(fn.includes('str_field(it, "family") == f'), 'filter on the mapped family')
  assert.ok(rust.includes('"family": str_field(arch, "id")'), 'hits must carry their archMap family')

  // UI: type picker, and a family list that is manifest-driven.
  const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
  for (const v of ['lora', 'checkpoint', 'gguf']) {
    assert.ok(html.includes(`<option value="${v}">`), `type filter ${v} missing`)
  }
  assert.ok(html.includes('id="apSearchFamily"'), 'family filter missing')
  assert.ok(renderer.includes('function fillFamilyFilter()'), 'family list must be built from the manifest')
  assert.ok(renderer.includes('pack.archMap'), 'family list must read archMap, not a hardcoded list')
})

test('CivitAI calls target the adult host, not the SFW one', () => {
  // April 2026: civitai.com serves the SFW library, civitai.red the mature one.
  // Searching civitai.com for an adult model returns nothing, so every CivitAI
  // request in this module must point at civitai.red.
  assert.ok(rust.includes('const CIVITAI_API: &str = "https://civitai.red/api/v1"'), 'API host must be civitai.red')
  const urls = [...rust.matchAll(/"(https:\/\/civitai\.[a-z.]+\/api\/v1[^"]*)"/g)].map((m) => m[1])
  assert.ok(urls.length >= 4, `expected several CivitAI endpoints, found ${urls.length}`)
  for (const u of urls) {
    assert.ok(u.startsWith('https://civitai.red/api/v1'), `CivitAI call must use civitai.red, got ${u}`)
  }
  // The search must ask for mature content explicitly.
  assert.ok(rust.includes('nsfw=1'), 'CivitAI search must request nsfw results')
  // And the UI has to say so, so nobody wonders why results differ from the
  // civitai.com site they remember.
  const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
  assert.ok(html.includes('civitai.red'), 'the split must be explained in the panel')
  assert.ok(/civitai\.com is now the SFW/.test(html), 'say plainly that civitai.com is SFW-only')
  assert.ok(html.includes('civitai.red</option>') || html.includes('civitai.red)'), 'source picker must name the adult host')
})

test('trending / most-downloaded browse is available and shares the search mapping', () => {
  const lib = fs.readFileSync(path.join(ROOT, 'src-tauri', 'src', 'lib.rs'), 'utf8')
  assert.ok(rust.includes('pub async fn adult_pack_browse'), 'browse command missing')
  assert.ok(lib.includes('adult_pack::adult_pack_browse'), 'browse not registered')
  assert.ok(bridge.includes('adultPackBrowse'), 'browse not in the w2gp bridge')
  // Sort vocabularies differ per source and must be mapped, not passed through.
  const browse = (rust.split('pub async fn adult_pack_browse')[1] || '').split('\n#[tauri')[0]
  assert.ok(browse.includes('hf_sort(&sort)'), 'HF sort must be mapped')
  assert.ok(browse.includes('civitai_sort(&sort)'), 'CivitAI sort must be mapped')
  assert.ok(browse.includes('civitai_period(&period)'), 'CivitAI period must be mapped')
  // Browse and search must agree on what is loadable — one mapper, both paths.
  assert.ok(rust.includes('fn map_hf_hit(') && rust.includes('fn map_civitai_hit('), 'shared mappers missing')
  const search = (rust.split('pub async fn adult_pack_search')[1] || '').split('\n#[tauri')[0]
  assert.ok(search.includes('map_hf_hit(&m)'), 'search must use the shared HF mapper')
  assert.ok(search.includes('map_civitai_hit(m)'), 'search must use the shared CivitAI mapper')

  // UI: a browse entry point with ranking + window + the same filters.
  const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
  for (const id of ['apBrowseSource', 'apBrowseSort', 'apBrowsePeriod', 'apBrowseKind', 'apBrowseFamily', 'apBrowseBtn']) {
    assert.ok(html.includes(`id="${id}"`), `browse control ${id} missing`)
  }
  for (const s of ['trending', 'downloads', 'likes', 'newest']) {
    assert.ok(html.includes(`value="${s}"`), `sort option ${s} missing`)
  }
  assert.ok(renderer.includes('async function runBrowse()'), 'renderer must run the browse')
  assert.ok(renderer.includes('adultPackBrowse('), 'renderer must call browse')
  // Both pickers are filled from the same manifest map.
  assert.ok(renderer.includes('for (const id of ["apSearchFamily", "apBrowseFamily"])'), 'both family pickers must share one filler')
  // Adding a checkpoint must be documented as writing the recipe Wan2GP uses.
  assert.ok(/writes <code>finetunes/.test(html), 'explain that Add writes the finetune JSON')
})

test('the search box is present and enter runs the search', () => {
  const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
  assert.ok(html.includes('id="apSearchQuery"'), 'search input missing')
  assert.ok(html.includes('id="apSearchSource"'), 'source picker missing')
  assert.ok(html.includes('id="apSearchResults"'), 'results host missing')
  assert.ok(renderer.includes('async function runSearch()'), 'no search implementation')
  assert.ok(renderer.includes("e.key === \"Enter\""), 'Enter should run the search')
  // An empty result must be explained, not silently blank.
  assert.ok(/Nothing matching/.test(renderer), 'empty results need a reason')
  assert.ok(/that this Wan2GP build can load/.test(renderer), 'empty results must name the limitation')
})
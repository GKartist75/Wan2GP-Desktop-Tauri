/**
 * adult-pack-add-and-manage.test.js — the paths that turn the pack into a
 * general finetune creator + manager for models Wan2GP itself does not ship:
 *
 *   1. Add by link reaches a model whose name matches no search keyword
 *   2. the architecture list is the build's own defaults, not only archMap
 *   3. a user-added LoRA gets a group, so it is actually visible
 *   4. recipes can be edited in a form, exported and imported
 *   5. LoRA files on disk can be listed and deleted (confined to the root)
 *   6. an existing recipe can be edited without the "id is taken" trap
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')

const ROOT = path.join(__dirname, '..')
const SRC = path.join(ROOT, 'src')
const rust = fs.readFileSync(path.join(ROOT, 'src-tauri', 'src', 'adult_pack.rs'), 'utf8')
const libRs = fs.readFileSync(
  path.join(ROOT, 'src-tauri', 'src', 'lib.rs'),
  'utf8',
)
const bridge = fs.readFileSync(path.join(SRC, 'w2gp.js'), 'utf8')
const renderer = fs.readFileSync(path.join(SRC, 'adult-pack-tab.js'), 'utf8')
const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')

test('a model can be added by pasting a link, not only by keyword search', () => {
  for (const id of ['apUrlInput', 'apUrlBtn', 'apUrlArch', 'apUrlResults']) {
    assert.ok(html.includes(`id="${id}"`), `the Add-by-link row needs ${id}`)
  }
  assert.ok(renderer.includes('async function addByUrl()'), 'the flow must exist')
  assert.ok(
    renderer.includes('adultPackResolveUrl(raw)'),
    'the pasted text must go through the backend resolver',
  )
  // It reuses inspect, and results render in their own host, not the search box.
  assert.ok(
    renderer.includes('{ host: results, status }'),
    'Add-by-link must render into its own results host',
  )
  // Resolution is backend-owned: the renderer never parses the URL itself.
  assert.ok(!renderer.includes('.match(/huggingface/'), 'the renderer must not parse URLs')
  assert.ok(rust.includes('fn parse_source_url('), 'the resolver lives in Rust')
})

test('the architecture list is what the build ships, with archMap as fallback', () => {
  assert.ok(rust.includes('pub fn adult_pack_arches('), 'the build list command must exist')
  assert.ok(rust.includes('fn family_for_arch('), 'the family mapping must be backend-owned')
  const fn = (rust.split('pub fn adult_pack_arches')[1] || '').split('\n#[tauri')[0]
  assert.ok(fn.includes('installed_model_ids()'), 'installed architectures come first')
  assert.ok(fn.includes('archMap'), 'the curated map is the fallback')
  assert.ok(!fn.includes('str_field(&entry, "family")'), 'the family is derived, never trusted')
  // The form prefers the build list and keeps the old manifest path as fallback.
  const opts = renderer.slice(renderer.indexOf('function archOptions()'))
  assert.ok(opts.includes('arches && arches.length'), 'the form must prefer the build list')
  assert.ok(opts.includes('pack && pack.archMap'), 'the manifest is the fallback')
})

test('adding an entry derives the folder from the architecture server-side', () => {
  const fn = (rust.split('pub fn adult_pack_add')[1] || '').split('\npub ')[0]
  assert.ok(fn.includes('family_for_arch(&arch)'), 'the backend derives the family')
  assert.ok(!fn.includes('str_field(&entry, "family")'), 'a client family value must be ignored')
  assert.ok(fn.includes('repo_field'), 'the source repo id is recorded for later update checks')
})

test('a user-added LoRA is given a group so it is not invisible', () => {
  // A row filed under a group that does not exist is counted but never drawn.
  assert.ok(
    rust.includes('"id": "custom"'),
    'the manifest must synthesize the group additions are filed under',
  )
  const manifestFn = (rust.split('pub fn adult_pack_manifest')[1] || '').split('\n#[tauri')[0]
  assert.ok(manifestFn.includes('groups.push('), 'the group must be appended when needed')
  assert.ok(
    manifestFn.includes('"group": "custom"') === false,
    'the manifest reads the group back out of the entries, not the other way around',
  )
})

test('a recipe is validated against the architectures the build ships', () => {
  assert.ok(
    rust.includes('is not an architecture this Wan2GP build ships'),
    'an unknown architecture must be refused at write time',
  )
  const validate = (rust.split('fn validate_finetune_doc')[1] || '').split('\nfn ')[0]
  assert.ok(validate.includes('installed_model_ids()'), 'the check uses the installed set')
  assert.ok(
    validate.includes('!installed.is_empty()'),
    'an uninitialised repo (empty list) must still allow the write',
  )
})

test('a CivitAI model can be inspected and added at a chosen version', () => {
  const sig = rust.slice(rust.indexOf('pub async fn adult_pack_inspect'))
  assert.ok(sig.slice(0, 400).includes('version: Option<String>'), 'inspect takes a version')
  assert.ok(rust.includes('"versions": versions_meta'), 'the response lists every version')
  assert.ok(
    renderer.includes('adultPackInspect(item.source, item.id, item.architecture, version)'),
    'the renderer passes the chosen version through',
  )
  assert.ok(
    renderer.includes('openAddFlow(item, { ...opts, version: vs.value, host, status })'),
    'switching version re-inspects in place',
  )
  assert.ok(
    bridge.includes('call("adult_pack_inspect", { source, id, architecture, version })'),
    'the bridge must forward the version',
  )
})

test('the file kind can be overridden before adding', () => {
  // A wrong name/size guess must not force a LoRA to be filed as a checkpoint.
  assert.ok(renderer.includes('ap-kind-sel'), 'the kind must be a control, not a label')
  assert.ok(renderer.includes('kind: kindSel.value'), 'the chosen kind must be what is sent')
  assert.ok(!renderer.includes('const familyOf'), 'the renderer must not guess a family')
})

test('existing recipes can be edited in a form, exported and imported', () => {
  assert.ok(renderer.includes('function openRecipeForm('), 'one form serves create and edit')
  assert.ok(renderer.includes('Form view'), 'an existing recipe must offer the form')
  assert.ok(
    renderer.includes('openRecipeForm({ host, id: ft.id, doc: parsed })'),
    'Form view must open the form seeded with the recipe',
  )
  assert.ok(renderer.includes('editId ? "Save changes"'), 'an edit must say it saves, not creates')
  // An edit must not trip the "id is taken" guard meant for new recipes.
  assert.ok(
    /if \(!editId\) \{[\s\S]{0,400}const taken = new Set\(\)/.test(renderer),
    'the duplicate-id guard must be skipped when editing',
  )
  assert.ok(renderer.includes('function downloadRecipe('), 'export must exist')
  assert.ok(renderer.includes('adultPackFinetuneWrite('), 'edits save through the same command')
  assert.ok(html.includes('id="apFmImportInput"'), 'import needs a path input')
  assert.ok(html.includes('id="apFmImportBtn"'), 'import needs a button')
  assert.ok(renderer.includes('libraryFinetuneImport(src)'), 'import reuses the validated path')
})

test('LoRA files on disk are listed and can be deleted, confined to the root', () => {
  assert.ok(rust.includes('pub fn adult_pack_lora_inventory('), 'the inventory command must exist')
  assert.ok(rust.includes('pub fn adult_pack_lora_delete('), 'deletion must exist')
  const del = (rust.split('pub fn adult_pack_lora_delete')[1] || '').split('\n#[tauri')[0]
  assert.ok(del.includes('canonicalize'), 'the target must be canonicalised before removal')
  assert.ok(del.includes('starts_with(&canon_root)'), 'deletion must stay inside the LoRAs root')
  assert.ok(del.includes('contains("..")'), 'a climbing path must be refused up front')

  assert.ok(html.includes('id="apLoraInvList"'), 'the inventory needs a host')
  assert.ok(html.includes('id="apLoraInvRefreshBtn"'), 'the inventory needs a refresh')
  assert.ok(renderer.includes('async function loadLoraInventory()'), 'the renderer must load it')
  assert.ok(renderer.includes('adultPackLoraInventory()'), 'it reads through the command')
  assert.ok(renderer.includes('adultPackLoraDelete(f.rel)'), 'a row must be deletable')
  assert.ok(
    renderer.includes('not just a catalog row'),
    'deleting weights must be confirmed in plain words',
  )
})

test('user-added entries are checked for updates like the catalog', () => {
  const updates = (rust.split('pub async fn adult_pack_updates')[1] || '').split('\n#[tauri')[0]
  assert.ok(
    updates.includes('arr(&custom, "loras")') && updates.includes('arr(&custom, "finetunes")'),
    'custom repos must be listed for the check too',
  )
  // A repo that could not be reached is reported, not silently dropped.
  assert.ok(
    renderer.includes('could not be reached'),
    'the update check must surface unreachable repos',
  )
})

test('every new command is registered and reachable from the bridge', () => {
  for (const cmd of [
    'adult_pack_arches',
    'adult_pack_resolve_url',
    'adult_pack_lora_inventory',
    'adult_pack_lora_delete',
  ]) {
    assert.ok(libRs.includes(`adult_pack::${cmd}`), `${cmd} must be registered in the invoke handler`)
  }
  for (const m of [
    'adultPackArches',
    'adultPackResolveUrl',
    'adultPackLoraInventory',
    'adultPackLoraDelete',
  ]) {
    assert.ok(bridge.includes(m), `${m} must be exposed on the bridge`)
  }
})

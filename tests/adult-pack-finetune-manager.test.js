/**
 * adult-pack-finetune-manager.test.js — managing the recipes, not just writing
 * them.
 *
 * WanGP parses `finetunes/*.json` at startup, so a bad edit produces a model
 * that silently never appears. These tests lock the guard rails around that:
 *   1. every write is validated (model + name + architecture + non-empty URLs)
 *   2. a recipe can never overwrite an upstream base model in defaults/
 *   3. the manager shows all three origins (pack / user-added / hand-written),
 *      because "what will Wan2GP offer me" is the question the user is asking
 *   4. link checking exists, because a pinned catalog rots silently otherwise
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
const bridge = fs.readFileSync(path.join(SRC, 'w2gp.js'), 'utf8')

test('every catalog row has its own actions, not just the bulk buttons', () => {
  // Bulk buttons act on a selection; a single model's own actions (write its
  // recipe, fetch it, copy its URL, delete it) must live on its row.
  assert.ok(renderer.includes('function actionsEl(item, kind)'), 'rows need an action bar')
  assert.ok(renderer.includes('ap-act-btn'), 'the action bar needs a trigger button')
  assert.ok(renderer.includes('ap-act-bar'), 'the action bar needs a container')

  // All three row kinds render it.
  for (const call of ['actionsEl(l, "lora")', 'actionsEl(f, "finetune")', 'actionsEl(c, "civitai")']) {
    assert.ok(renderer.includes(call), `missing ${call}`)
  }

  // The checkpoint-specific actions.
  for (const label of ['Make finetune JSON', 'Rewrite finetune JSON', 'Fetch weights now', 'Open recipe']) {
    assert.ok(renderer.includes(label), `checkpoint row is missing "${label}"`)
  }
  // Deletion and the copy helpers exist on every kind.
  for (const label of ['Delete…', 'Copy URL', 'Copy file name', 'Open source page']) {
    assert.ok(renderer.includes(label), `row actions missing "${label}"`)
  }
  // Delete must (a) be the danger action, (b) target exactly the clicked row,
  // (c) be confirmed by name. Asserted separately: one wide regex spanning the
  // whole callback is brittle and says nothing about which half broke.
  const delBtn = renderer.slice(
    renderer.indexOf('actionBtn("Delete…'),
    renderer.indexOf('const btn = rowEl("ap-act-btn"'),
  )
  assert.ok(delBtn.includes('adultPackRemove([item.id])'), 'per-row delete must target just that id')
  assert.ok(/\n\s*true\),\s*\n\s*\);/.test(delBtn), 'delete must be the danger action')
  assert.ok(
    renderer.includes('window.confirm(`Delete ${item.name}'),
    'per-row delete must ask first, naming the model',
  )
  assert.ok(renderer.includes('Files you added yourself are never touched.'), 'the confirm must state the limit')

  // A LoRA must not offer a finetune JSON it cannot have: Wan2GP loads LoRAs
  // from loras\, so the row says so instead of offering a button that lies.
  const loraPart = renderer.slice(
    renderer.indexOf('const isLora = kind === "lora"'),
    renderer.indexOf('function renderLoraRows'),
  )
  assert.ok(
    !loraPart.slice(0, loraPart.indexOf('isLora ? "Download now"')).includes('Make finetune JSON'),
    'LoRA rows must not offer a finetune JSON',
  )
  assert.ok(loraPart.includes('loras\\\\'), 'the LoRA row must explain why')

  // CivitAI rows fetch through the user's token, not through the bulk path.
  assert.ok(renderer.includes('adultPackCivitaiDownload(item.id, token)'), 'civitai row must fetch with the token')
})

test('finetune manager commands exist and are registered', () => {
  const lib = fs.readFileSync(path.join(ROOT, 'src-tauri', 'src', 'lib.rs'), 'utf8')
  // [rust command, camelCase bridge name]
  const pairs = [
    ['adult_pack_finetune_list', 'adultPackFinetuneList'],
    ['adult_pack_finetune_read', 'adultPackFinetuneRead'],
    ['adult_pack_finetune_write', 'adultPackFinetuneWrite'],
    ['adult_pack_finetune_check', 'adultPackFinetuneCheck'],
  ]
  for (const [cmd, camel] of pairs) {
    assert.ok(rust.includes(cmd), `${cmd} missing from the module`)
    assert.ok(lib.includes(`adult_pack::${cmd}`), `${cmd} not registered`)
    assert.ok(bridge.includes(camel), `${camel} missing from the w2gp bridge`)
  }
})

test('writes are validated before they reach finetunes/', () => {
  const fn = (rust.split('fn validate_finetune_doc')[1] || '').split('\nfn ')[0]
  assert.ok(fn.includes('get("model")'), 'must require a model section')
  assert.ok(fn.includes('name.is_empty()'), 'must require a name')
  assert.ok(fn.includes('arch.is_empty()'), 'must require an architecture')
  assert.ok(fn.includes('urls.is_empty()'), 'must require at least one URL')
  assert.ok(/starts_with\("https?:\/\/"\)/.test(fn), 'URLs must be http(s) or a local path')
})

test('a recipe can never overwrite an upstream base model', () => {
  const fn = (rust.split('pub fn adult_pack_finetune_write')[1] || '').split('\n#[tauri')[0]
  assert.ok(fn.includes('defaults'), 'must check defaults/ before writing')
  assert.ok(/finetune_id_valid\(&id\)/.test(fn), 'the id itself must be validated')
  // ids are used as file names — traversal must be impossible
  assert.ok(
    /id\.chars\(\)\.all\(\|c\| c\.is_ascii_alphanumeric\(\) \|\| c == '-' \|\| c == '_'\)/.test(rust),
    'finetune ids must be filename-safe',
  )
})

test('the manager lists all three origins, not just the pack', () => {
  const fn = (rust.split('pub fn adult_pack_finetune_list')[1] || '').split('\nfn ')[0]
  assert.ok(fn.includes('arr(manifest(), "finetunes")'), 'pack recipes')
  assert.ok(fn.includes('arr(&custom, "finetunes")'), 'user-added recipes')
  assert.ok(fn.includes('read_dir(&dir)'), 'hand-written recipes in the folder')
  assert.ok(fn.includes('hasLocalWeights'), 'must report whether weights are already on disk')
  // The UI must be able to reach all of it.
  assert.ok(renderer.includes('async function loadFinetunes()'), 'renderer must load the list')
  assert.ok(renderer.includes('adultPackFinetuneList()'), 'renderer must call the list command')
  assert.ok(html.includes('id="apFmList"'), 'manager host missing from the panel')
})

test('recipes can be edited, saved, copied, link-checked and deleted', () => {
  for (const fn of ['adultPackFinetuneRead', 'adultPackFinetuneWrite', 'adultPackFinetuneCheck']) {
    assert.ok(renderer.includes(fn), `editor must be able to call ${fn}`)
  }
  assert.ok(renderer.includes('navigator.clipboard.writeText'), 'recipes should be copyable to share')
  assert.ok(renderer.includes('Check links'), 'link checking must be reachable from the UI')
  // Deleting a recipe must not silently delete gigabytes of weights.
  assert.ok(
    /Downloaded weights are left on disk/.test(renderer),
    'the delete confirmation must say weights are kept',
  )
  assert.ok(renderer.includes('className = "ap-fm-text"'), 'a real JSON editor, not a summary')
})
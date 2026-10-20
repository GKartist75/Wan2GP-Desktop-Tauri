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
  // Per-row delete must ask first, naming the model. It goes through the
  // native dialog bridge: window.confirm is replaced by an async plugin shim
  // the ACL denies, so its truthiness silently skips the guard.
  assert.ok(
    renderer.includes('askConfirm(`Delete ${item.name}'),
    'per-row delete must ask first, naming the model',
  )
  assert.ok(renderer.includes('Nothing outside the pack folders is touched.'), 'the confirm must state the limit')

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

  // CivitAI rows fetch through the (optional) token, not through the bulk path.
  assert.ok(
    renderer.includes('adultPackCivitaiDownload(item.id, token || null)'),
    'civitai row must fetch even without a token',
  )
  assert.ok(
    !renderer.includes('Paste a CivitAI token first.'),
    'a public CivitAI release must not require a token',
  )
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
  // Filename safety is behavioral: the Rust `a_recipe_can_be_created_...`
  // test drives "../escape" through the real command and expects a refusal,
  // so rustfmt can reflow the predicate without breaking this suite.
})

test('the manager lists all three origins, not just the pack', () => {
  const fn = (rust.split('pub fn adult_pack_finetune_list')[1] || '').split('\nfn ')[0]
  assert.ok(fn.includes('arr(manifest(), "finetunes")'), 'pack recipes')
  assert.ok(fn.includes('arr(&custom, "finetunes")'), 'user-added recipes')
  assert.ok(fn.includes('read_dir(&dir)'), 'hand-written recipes in the folder')
  assert.ok(fn.includes('finetune_summary'), 'scanned recipes must use the shared summary')
  assert.ok(
    rust.includes('"hasLocalWeights": local_files > 0'),
    'must report whether weights are already on disk',
  )
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

test('a recipe can be created from the panel without hand-writing JSON', () => {
  assert.ok(html.includes('id="apFmNewBtn"'), 'the panel needs a create button')
  assert.ok(html.includes('id="apFmNewHost"'), 'the form needs a host next to the list')
  assert.ok(html.includes('＋ New finetune.json'), 'the create button must be visible as an action')
  assert.ok(renderer.includes('function openNewFinetuneEditor()'), 'the form must exist')
  assert.ok(
    renderer.includes('$("apFmNewBtn").addEventListener("click", openNewFinetuneEditor)'),
    'the button must open the form',
  )

  const fn = renderer.slice(
    renderer.indexOf('function archOptions()'),
    renderer.indexOf('async function loadFinetunes()'),
  )
  // The architecture picker prefers the list this build actually ships (from
  // adult_pack_arches) and falls back to the pack manifest: a hand-typed id
  // would write a recipe Wan2GP cannot map to any base model.
  assert.ok(fn.includes('arches && arches.length'), 'architectures must come from the build list first')
  assert.ok(fn.includes('pack && pack.archMap'), 'the manifest is the fallback source')
  // manifest archMap ids are family filters; the recipe needs Wan2GP's own
  // architecture id, or the written file is one Wan2GP can never offer.
  assert.ok(fn.includes('e.architecture || (e.wan2gpIds || [])[0]'), 'must write a Wan2GP architecture id')
  assert.ok(fn.includes('seenArch.has(e.value)'), 'one option per architecture')
  assert.ok(!fn.includes('o.value = e.id'), 'must not write the launcher family key as architecture')
  assert.ok(/finetunes\/<id>\.json/.test(fn), 'the form must say where the file lands')
  // A second (LOW) half, model notes and prompt help are real WanGP fields —
  // optional, so a plain t2v finetune still writes the minimal shape.
  assert.ok(fn.includes('model.URLs2 = second'), 'second-phase URLs are optional')
  assert.ok(fn.includes('model.URLs2 = reuseSecond'), 'a second-phase recipe id is optional')
  assert.ok(fn.includes('reuse2.value'), 'the form must round-trip a reused second phase')
  assert.ok(fn.includes('model.infos = notes'), 'model notes are optional')
  assert.ok(fn.includes('model.prompt_infos = promptHelp'), 'prompt help is optional')
  assert.ok(
    fn.includes('if (!primary.length && !reuseId)'),
    'a recipe with neither source URLs nor a reused recipe must be refused',
  )
  // Creating over an existing recipe would silently lose the old definition.
  // The check must cover the file stem too: a pack recipe is listed as
  // `wan_rapid_aio` but lives in `adultpack_wan_rapid_aio.json`.
  assert.ok(fn.includes('const taken = new Set()'), 'existing ids must be collected')
  assert.ok(fn.includes('taken.add(f.id)'), 'manifest ids count')
  assert.ok(fn.includes('.replace(/\\.json$/i, "")'), 'file stems count')
  assert.ok(fn.includes('if (taken.has(fid))'), 'an existing id must be refused')
  assert.ok(!fn.includes('fmItems.some((f) => f.id === fid)'), 'id-only comparison misses pack files')
  assert.ok(fn.includes('if (!fmLoaded) await loadFinetunes()'), 'the guard must read finetunes/ first')
  // The backend does the real validation; the form must still go through it.
  assert.ok(fn.includes('adultPackFinetuneWrite('), 'the form must write through the command')
  assert.ok(fn.includes('JSON.stringify(doc, null, 2)'), 'the file is the API — write it pretty')
  assert.ok(fn.includes('loadFinetunes()'), 'the list must refresh after a create')
})
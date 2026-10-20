/**
 * adult-pack-civitai-switch.test.js — CivitAI is opt-in.
 *
 * `civitai.red` is the only adult host the launcher talks to, and security
 * suites commonly block it as an adult site. The switch must therefore:
 *   1. default to off and persist per install,
 *   2. refuse every CivitAI command (and direct CivitAI file downloads) before
 *      opening a socket,
 *   3. hide the CivitAI UI and fall back to HuggingFace when off.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')

const ROOT = path.join(__dirname, '..')
const SRC = path.join(ROOT, 'src')
const rust = fs.readFileSync(path.join(ROOT, 'src-tauri', 'src', 'adult_pack.rs'), 'utf8')
const lib = fs.readFileSync(path.join(ROOT, 'src-tauri', 'src', 'lib.rs'), 'utf8')
const bridge = fs.readFileSync(path.join(SRC, 'w2gp.js'), 'utf8')
const renderer = fs.readFileSync(path.join(SRC, 'adult-pack-tab.js'), 'utf8')
const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
const config = fs.readFileSync(path.join(ROOT, 'src-tauri', 'src', 'config.rs'), 'utf8')

test('CivitAI defaults to off and is exposed in the manifest', () => {
  const enabled = (rust.split('fn civitai_enabled()')[1] || '').split('\n}')[0]
  assert.ok(enabled.includes('unwrap_or(false)'), 'CivitAI must default to off')
  assert.ok(rust.includes('fn require_civitai()'), 'missing the refusal helper')
  assert.ok(rust.includes('pub fn adult_pack_set_civitai(enabled: bool)'), 'missing the setter command')
  assert.ok(lib.includes('adult_pack::adult_pack_set_civitai'), 'setter not registered in lib.rs')
  assert.ok(bridge.includes('adultPackSetCivitai'), 'setter missing from the w2gp bridge')
  assert.ok(bridge.includes('adult_pack_set_civitai'), 'bridge must call the setter command')
  assert.ok(rust.includes('"civitaiEnabled": civitai_enabled()'), 'manifest must expose the setting')
})

test('no CivitAI command opens a socket while the switch is off', () => {
  for (const fn of [
    'adult_pack_civitai_verify',
    'adult_pack_civitai_download',
    'adult_pack_civitai_gallery',
    'adult_pack_browse',
    'adult_pack_search',
    'adult_pack_inspect',
  ]) {
    const body = (rust.split(`pub async fn ${fn}`)[1] || '').split('\n#[tauri')[0]
    assert.ok(body.includes('require_civitai()?'), `${fn} must refuse while CivitAI is off`)
  }
  // Direct downloads of user-added CivitAI models follow the same switch.
  assert.ok(rust.includes('fn is_civitai_url('), 'missing the CivitAI URL detector')
  assert.ok(rust.includes('!civitai_enabled() && is_civitai_url(&url)'), 'fetch must honour the switch')
})

test('the UI hides CivitAI and falls back to HuggingFace when off', () => {
  assert.ok(html.includes('id="apCivEnabled"'), 'missing the CivitAI toggle')
  assert.ok(html.includes('id="apCivSection"'), 'missing the CivitAI section')
  assert.ok(renderer.includes('adultPackSetCivitai('), 'renderer must save the toggle')
  assert.ok(renderer.includes('function applyCivitaiVisibility()'), 'renderer must hide the section when off')
  const planBody = (renderer.split('function plan()')[1] || '').split('\n  }')[0]
  assert.ok(planBody.includes('civOn()'), 'a selection left from before must not count while off')
})

test('CivitAI LoRAs are filed as LoRAs, not checkpoints', () => {
  const manifest = JSON.parse(fs.readFileSync(path.join(SRC, 'adult-pack-manifest.json'), 'utf8'))
  for (const id of ['civ_h3_nsfw_unlocked', 'civ_h3_dr34ml4y']) {
    const item = (manifest.civitai || []).find((x) => x.id === id)
    assert.ok(item, `missing ${id}`)
    assert.strictEqual(item.kind, 'lora', `${id} must declare kind=lora`)
  }
  // The download path must consult the declared/API kind, and put LoRAs into
  // the collection folder Wan2GP scans instead of the checkpoints folder.
  assert.ok(rust.includes('let is_lora = declared == "lora"'), 'missing kind detection')
  assert.ok(rust.includes('lora_collection_dir(&collection).join("adult-pack")'), 'LoRA destination missing')
  assert.ok(rust.includes('let finetune = if is_lora {'), 'LoRAs must not get a finetune recipe')
})

test('a CivitAI token is optional and only attached once', () => {
  assert.ok(rust.includes('fn civitai_auth('), 'missing optional auth helper')
  assert.ok(rust.includes('fn civitai_download_url('), 'missing download-token helper')
  assert.ok(
    rust.includes('tok.is_some()') && rust.includes('Public releases download without one'),
    '401 errors must explain that public releases need no token',
  )
  const download = (rust.split('fn civitai_download_url(')[1] || '').split('\n}')[0]
  assert.ok(download.includes('!url.contains("token=")'), 'must not append the token twice')
  assert.ok(download.includes('urlencode(t)'), 'the token must be URL-encoded')
})

test('installed CivitAI items are not offered again and can still be removed', () => {
  const planBody = (renderer.split('function plan()')[1] || '').split('\n  }')[0]
  assert.ok(planBody.includes('civSelIds.filter((id) => !cm.get(id)?.installed)'))
  assert.ok(planBody.includes('civSelIds.length'), 'selection count must include installed items')
  assert.ok(renderer.includes('p.civSelIds'), 'remove must include installed CivitAI items')
  assert.ok(rust.includes('"civitaiInstalled"'), 'downloads must be recorded for accurate installed counts')
  assert.ok(rust.includes('for f in files {'), 'removal must delete the recorded files')
  assert.ok(rust.includes('"failed": failed'), 'a locked file must be reported, not silently kept')
  assert.ok(renderer.includes('r.failed'), 'the UI must show which file could not be removed')
})

test('the bulk fetch path uses the optional token for CivitAI files', () => {
  const body = (rust.split('pub async fn adult_pack_fetch')[1] || '').split('\n#[tauri')[0]
  assert.ok(body.includes('civitai_download_url'), 'fetch must append a stored token to CivitAI URLs')
  assert.ok(body.includes('is_civitai_url(&url)'), 'only CivitAI URLs get the token')
  // The Finetune Manager plugin already stores a CivitAI key; read it instead
  // of making the user paste the same key twice.
  assert.ok(rust.includes('Wan2GP-finetune-manager'), 'missing plugin config lookup')
  assert.ok(rust.includes('civitai_api_key'), 'the plugin key field must be read')
  assert.ok(rust.includes('civitai_auth(client.get(&url), civ_token.as_deref())'), 'inspect must authenticate')
})

test('the library distinguishes curated pack recipes from your additions', () => {
  assert.ok(config.includes('adult-pack-custom.json'), 'must read the custom list for provenance')
  assert.ok(config.includes('Yours (CivitAI)'), 'CivitAI additions must not be labelled as curated')
  assert.ok(config.includes('Yours (added via pack)'), 'pack additions must not be labelled as curated')
  assert.ok(config.includes('"Pack · CivitAI"'))
  assert.ok(config.includes('adult-pack'), 'checkpoints in adult-pack must be labelled Pack')
})

test('the LoRA inventory labels pack and user files', () => {
  assert.ok(rust.includes('"Pack · CivitAI"'), 'CivitAI LoRAs need their own origin')
  assert.ok(rust.includes('"Pack (unlisted)"'), 'a file in adult-pack/ but not in the catalog is still packish')
  assert.ok(rust.includes('"Yours (added via pack)"'), 'custom additions must be distinguished')
  assert.ok(renderer.includes('f.origin'), 'the inventory must show the origin instead of a bare known/unknown tag')
})

test('the pack table is ordered as a workflow and starts with the disclaimer', () => {
  assert.ok(html.includes('class="ap-disclaimer"'), 'missing the in-development/adult disclaimer')
  assert.ok(html.includes('The Pack Manager is in development.'))
  const order = ['⬇ Downloads', '🔎 Find models', '🧩 Finetune recipes', '🗂 LoRA files on disk', 'Adult (NSFW) model pack']
  const positions = order.map((label) => html.indexOf(label))
  for (let i = 0; i < positions.length; i++) {
    assert.ok(positions[i] >= 0, `missing section: ${order[i]}`)
    if (i) assert.ok(positions[i - 1] < positions[i], `${order[i - 1]} must come before ${order[i]}`)
  }
  assert.ok(html.includes('data-civ-key="com"') && html.includes('data-civ-key="red"'), 'missing both API-key links')
  assert.ok(html.includes('civitai.com → Account → API Keys'))
  assert.ok(html.includes('civitai.red → Account → API Keys'))
  assert.ok(renderer.includes('data-civ-key'), 'the API-key links must be wired to open externally')
})

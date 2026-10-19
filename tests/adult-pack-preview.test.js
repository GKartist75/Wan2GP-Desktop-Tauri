/**
 * adult-pack-preview.test.js — the Model Pack rows show more than a filename.
 *
 * Each row carries a preview image, a source link and a verified hash, and all
 * three come from remote manifest data. That makes this file a security-shaped
 * test as much as a content one:
 *   1. every URL the panel renders is https and points at the verified source
 *   2. previews are pinned to the exact commit whose bytes we hashed, so a
 *      picture can never advertise a different build than the file
 *   3. the CSP grants exactly one extra image origin (huggingface.co) — not
 *      `https:` or `*`, which would let any injected content pull anything
 *   4. the renderer never navigates the app webview with raw anchors
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')

const ROOT = path.join(__dirname, '..')
const SRC = path.join(ROOT, 'src')
const manifest = JSON.parse(fs.readFileSync(path.join(SRC, 'adult-pack-manifest.json'), 'utf8'))
const renderer = fs.readFileSync(path.join(SRC, 'adult-pack-tab.js'), 'utf8')

test('every lora entry carries provenance and a valid file page', () => {
  for (const l of manifest.loras) {
    assert.ok(l.repo, `${l.id}: needs a repo for provenance`)
    assert.ok(
      l.filePage && l.filePage.startsWith('https://huggingface.co/'),
      `${l.id}: needs an https file page`,
    )
    assert.ok(l.repoPage && l.repoPage.startsWith('https://'), `${l.id}: needs an https repo page`)
    assert.ok(l.filePage.includes(l.repo), `${l.id}: file page must live under its own repo`)
  }
})

test('finetune recipes carry provenance too', () => {
  for (const f of manifest.finetunes) {
    assert.ok(f.repo, `${f.id}: needs a repo for provenance`)
    assert.ok(f.filePage && f.filePage.startsWith('https://'), `${f.id}: needs an https file page`)
    assert.ok(f.sha256 && /^[0-9a-f]{64}$/.test(f.sha256), `${f.id}: needs the verified hash`)
  }
})

test('previews are https, commit-pinned and correctly encoded', () => {
  const withPreview = manifest.loras.filter((l) => l.preview)
  assert.ok(withPreview.length > 10, `expected the catalog to resolve previews, got ${withPreview.length}`)
  for (const l of withPreview) {
    assert.ok(l.preview.startsWith('https://huggingface.co/'), `${l.id}: preview must be https`)
    // Pinned to the commit we hashed — an unpinned /resolve/main/ could show a
    // different build than the bytes the manifest promises.
    assert.ok(
      /\/resolve\/[0-9a-f]{40}\//.test(l.preview),
      `${l.id}: preview must be pinned to a commit sha, got ${l.preview}`,
    )
    // %20 (a space in the filename) is correct; %25 is the double-encode bug
    // that turned "Screenshot 2026-01-29.png" into "Screenshot%25202026...".
    assert.ok(!l.preview.includes('%25'), `${l.id}: preview looks double-encoded`)
    assert.ok(l.preview.includes(l.repo), `${l.id}: preview must come from its own repo`)
  }
})

// A bare "https:" scheme (not "https://host") or a wildcard would allow any
// origin — exactly the lazy fix these CSP assertions exist to prevent.
const bareScheme = (list) => /https:(?!\/\/)/.test(list) || /(^|\s)\*/.test(list)
const csp = () =>
  JSON.parse(fs.readFileSync(path.join(ROOT, 'src-tauri', 'tauri.conf.json'), 'utf8')).app.security.csp

test('CSP allows the preview origin and nothing broader', () => {
  const conf = JSON.parse(fs.readFileSync(path.join(ROOT, 'src-tauri', 'tauri.conf.json'), 'utf8'))
  const csp = conf.app.security.csp
  const imgSrc = /img-src ([^;]*)/.exec(csp)[1]
  assert.ok(imgSrc.includes('https://huggingface.co'), 'previews need the huggingface origin')
  // A bare "https:" scheme (not "https://host") or a wildcard would allow any
  // origin — exactly the lazy fix this test exists to prevent.
  const rest = imgSrc.replace(/https:\/\/[\w.-]+/g, ' ')
  assert.ok(!bareScheme(rest), `img-src must stay host-scoped, got: ${imgSrc}`)
})

test('rows can show what a model looks like even when its repo has no media', () => {
  // Most HF mirrors in this catalog publish zero example files, so a row cannot
  // depend on repo media alone. Two independent paths must exist:
  //   1. examples baked into the manifest, matched from CivitAI by name
  //   2. a live CivitAI search for the row, for everything else
  const withCiv = [...manifest.loras, ...manifest.finetunes].filter((x) => (x.examples || []).length)
  assert.ok(withCiv.length > 0, 'expected some entries matched to CivitAI examples')
  for (const it of withCiv) {
    assert.ok(it.civitaiPage, `${it.id}: examples shown without a link back to the source`)
    for (const ex of it.examples) {
      assert.ok(ex.url.startsWith('https://'), `${it.id}: example must be https`)
      assert.ok(['image', 'video'].includes(ex.kind), `${it.id}: example needs a media kind`)
    }
  }
  assert.ok(renderer.includes('Find examples on CivitAI'), 'rows need a fallback path to examples')
  assert.ok(renderer.includes('adultPackSearch("civitai"'), 'the fallback must search CivitAI')

  // Every row also gets a plain search link, whether or not it matched.
  for (const it of [...manifest.loras, ...manifest.finetunes]) {
    assert.ok(it.civitaiSearch, `${it.id}: needs a CivitAI search link`)
  }

  // The gallery must not depend on a network round-trip when the data is baked.
  assert.ok(
    /const load = async \(\) => \{[\s\S]{0,600}?paintBaked\(\);[\s\S]{0,600}?(adultPackGallery|kind === "baked")/.test(
      renderer,
    ),
    'the example drawer must paint the baked examples before asking the network',
  )
})

test('a row shows its version, release date and download state', () => {
  const html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8')
  const bridge = fs.readFileSync(path.join(SRC, 'w2gp.js'), 'utf8')
  const lib = fs.readFileSync(path.join(ROOT, 'src-tauri', 'src', 'lib.rs'), 'utf8')
  const rust = fs.readFileSync(path.join(ROOT, 'src-tauri', 'src', 'adult_pack.rs'), 'utf8')

  for (const it of [...manifest.loras, ...manifest.finetunes]) {
    assert.ok(it.version, `${it.id}: needs a version`)
    assert.ok(it.releasedAt, `${it.id}: needs a release date`)
  }
  assert.ok(renderer.includes('function versionEl(item)'), 'rows must show version + date')
  assert.ok(renderer.includes('function stateEl(item)'), 'rows must show download state')
  assert.ok(renderer.includes('apChips'), 'the panel needs headline counters')
  assert.ok(html.includes('id="apChips"'), 'the counters need a host element')

  // Update detection must be exact (hash comparison), not date-based, and it
  // must have a way to actually apply what it finds.
  assert.ok(rust.includes('pub async fn adult_pack_updates'), 'update check command missing')
  assert.ok(rust.includes('pub fn adult_pack_apply_updates'), 'apply-updates command missing')
  assert.ok(lib.includes('adult_pack::adult_pack_updates'), 'update check not registered')
  assert.ok(lib.includes('adult_pack::adult_pack_apply_updates'), 'apply not registered')
  assert.ok(bridge.includes('adultPackUpdates') && bridge.includes('adultPackApplyUpdates'), 'bridge')
  assert.ok(rust.includes('?blobs=true'), 'the check must compare real file hashes')
  // Downloads have to honour an applied pin, or an update can never verify.
  assert.ok(/let pins = load_pins\(\);/.test(rust), 'fetch must load pin overrides')
  assert.ok(/effective_pin\(&id,/.test(rust), 'fetch must verify against the effective pin')
  for (const id of ['apUpdCheckBtn', 'apUpdApplyBtn', 'apUpdStatus', 'apUpdList']) {
    assert.ok(html.includes(`id="${id}"`), `updates panel needs ${id}`)
  }
})

test('renderer renders previews, checks the scheme, and links out externally', () => {
  assert.ok(/const httpsUrl = \(u\)/.test(renderer), 'no scheme guard for remote URLs')
  assert.ok(renderer.startsWith('//') || renderer.includes('(function'), 'sanity: renderer parses')
  assert.ok(renderer.includes('window.w2gp.openExternal(url)'), 'links must go through openExternal')
  assert.ok(renderer.includes('function thumbOf(item)'), 'rows need one thumbnail helper')
  assert.ok(renderer.includes('thumbOf(l)'), 'lora rows must render their preview')
  assert.ok(renderer.includes('thumbOf(f)'), 'finetune rows must render their preview')
  // Most HF mirrors publish no media at all, so a row falls back to the example
  // renders matched from CivitAI instead of showing an empty placeholder.
  assert.ok(
    /function thumbOf\(item\)[\s\S]{0,160}item\.examples/.test(renderer),
    'a row without a repo preview must fall back to its CivitAI examples',
  )
  assert.ok(renderer.includes('detailsEl('), 'rows must be able to show full details')
  assert.ok(renderer.includes('img.loading = "lazy"'), 'previews must load lazily')
  assert.ok(renderer.includes('"error"'), 'a missing preview must degrade to a placeholder')
})

test('live galleries are wired for both sources, and stay scheme-checked', () => {
  const bridge = fs.readFileSync(path.join(SRC, 'w2gp.js'), 'utf8')
  for (const cmd of ['adult_pack_gallery', 'adult_pack_civitai_gallery']) {
    assert.ok(bridge.includes(cmd), `w2gp bridge must expose ${cmd}`)
    const lib = fs.readFileSync(path.join(ROOT, 'src-tauri', 'src', 'lib.rs'), 'utf8')
    assert.ok(lib.includes(`adult_pack::${cmd}`), `${cmd} must be registered in lib.rs`)
  }
  // Video samples are the point: an image-only preview skips every .mp4 a Wan,
  // H3 or LTX repo publishes as its example reel.
  assert.ok(renderer.includes("kind === \"video\"") || renderer.includes('createElement("video")'), 'galleries must support video')
  assert.ok(renderer.includes('adultPackGallery('), 'renderer must fetch HF galleries')
  assert.ok(renderer.includes('adultPackCivitaiGallery('), 'renderer must fetch CivitAI examples')
  // Every media URL still passes the https guard.
  assert.ok(renderer.includes('function mediaEl('), 'media helper missing')
  const mediaSrc = /media-src ([^;]*)/.exec(csp())[1]
  assert.ok(mediaSrc.includes('https://huggingface.co'), 'video needs the huggingface media origin')
  assert.ok(!bareScheme(mediaSrc.replace(/https:\/\/[\w.-]+/g, ' ')), `media-src must stay host-scoped, got: ${mediaSrc}`)
  // CivitAI example images come from their own CDN.
  assert.ok(/img-src[^;]*image\.civitai\.com/.test(csp()), 'CivitAI image CDN must be allowed')
})

test('every finetune name carries a Wan2GP precision token', () => {
  // Wan2GP reads the checkpoint NAME to decide how to load it. Two cases, per
  // the upstream docs:
  //   * safetensors quantized weights MUST carry quanto_bf16_int8 — otherwise
  //     an int8 file is read as bf16 and fails to load;
  //   * .gguf files are driven by their extension and the level visible in the
  //     name (Q4_K_M etc.), with the matching GGUF kernel auto-selected.
  // This is load-bearing, not cosmetic.
  for (const f of manifest.finetunes) {
    assert.ok(f.precisionToken, `${f.id}: no precision token derived (${f.name})`)
    const lower = f.name.toLowerCase()
    const isGguf = /\.gguf$/i.test(f.file || '')
    if (isGguf) {
      assert.ok(
        String(f.precisionToken).startsWith('gguf_'),
        `${f.id}: .gguf file must get a gguf token`,
      )
      assert.ok(
        /^gguf_(q\d+(?:_[a-z0-9]+)*|iq\d+_[a-z0-9]+)$/.test(f.precisionToken),
        `${f.id}: malformed gguf token ${f.precisionToken}`,
      )
      const level = f.precisionToken.slice(5)
      assert.ok(lower.includes(level), `${f.id}: level ${level} not visible in "${f.name}"`)
    } else if (String(f.precisionToken) === 'quanto_bf16_int8') {
      assert.ok(
        lower.includes('quanto_bf16_int8'),
        `${f.id}: quantized safetensors must carry quanto_bf16_int8, got "${f.name}"`,
      )
    } else {
      // bf16 / fp16 / fp8 — the precision itself is the token.
      assert.ok(lower.includes(String(f.precisionToken)), `${f.id}: token missing from "${f.name}"`)
    }
  }
})

test('details drawer exposes hash and source, not just a filename', () => {
  for (const label of ['SHA-256', 'Trigger', 'Licence', 'Wan2GP', 'Source']) {
    assert.ok(renderer.includes(`add("${label}"`) || renderer.includes(`add('${label}'`), `details missing ${label}`)
  }
})
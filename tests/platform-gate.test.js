/**
 * platform-gate.test.js — the JS capability map and the declarative UI gate.
 *
 * Two things can silently rot here:
 *  1. `capabilitiesFor` drifting from the Rust adapters it mirrors. The pin
 *     below parses the assertions in src-tauri/src/platform/mod.rs, so a new
 *     capability in Rust fails this suite until the JS map learns it.
 *  2. index.html advertising a control whose `data-requires` name is a typo.
 *     Unknown names fail closed (hidden), which is safe but invisible — the
 *     index.html test asserts they are all real.
 */
const test = require('node:test')
const assert = require('node:assert')
const fs = require('node:fs')
const path = require('node:path')

const ROOT = path.join(__dirname, '..')
const gate = require('../src/services/platform-gate.js')
const HTML = fs.readFileSync(path.join(ROOT, 'src', 'index.html'), 'utf8')
const CSS = fs.readFileSync(path.join(ROOT, 'src', 'style.css'), 'utf8')
const RUST = fs.readFileSync(
  path.join(ROOT, 'src-tauri', 'src', 'platform', 'mod.rs'),
  'utf8',
)

/** Minimal stand-in for the elements `applyPlatformGating` walks. */
function fakeRoot(specs) {
  const els = specs.map((s) => {
    const classes = new Set(s.classes || [])
    return {
      id: s.id || '',
      getAttribute: (name) => (name === 'data-requires' ? s.requires : null),
      classList: {
        toggle(c, on) {
          if (on) classes.add(c)
          else classes.delete(c)
        },
        contains: (c) => classes.has(c),
      },
    }
  })
  return { querySelectorAll: (sel) => (sel === '[data-requires]' ? els : []), els }
}

const UI = [
  { id: 'dlss5Card', requires: 'dlss' },
  { id: 'kernelWheelsCard', requires: 'kernelWheels' },
  { id: 'taskMgrBtn', requires: 'taskManager' },
  { id: 'desktopShortcutBtn', requires: 'desktopShortcut' },
  { id: 'tsMsvcRow', requires: 'tritonRepair' },
]

test('capabilitiesFor matches the platform each control needs', () => {
  assert.deepStrictEqual(gate.capabilitiesFor('win32'), {
    taskManager: true,
    desktopShortcut: true,
    dlss: true,
    kernelWheels: true,
    tritonRepair: true,
    autoStart: true,
  })
  assert.deepStrictEqual(gate.capabilitiesFor('linux'), {
    taskManager: false,
    desktopShortcut: true,
    dlss: false,
    kernelWheels: true,
    tritonRepair: false,
    autoStart: true,
  })
  assert.deepStrictEqual(gate.capabilitiesFor('darwin'), {
    taskManager: false,
    desktopShortcut: false,
    dlss: false,
    kernelWheels: false,
    tritonRepair: false,
    autoStart: true,
  })
})

test('platformFromNavigator normalizes every webview spelling', () => {
  assert.strictEqual(gate.platformFromNavigator({ platform: 'Win32' }), 'win32')
  assert.strictEqual(gate.platformFromNavigator({ platform: 'MacIntel' }), 'darwin')
  assert.strictEqual(gate.platformFromNavigator({ platform: 'Linux x86_64' }), 'linux')
  assert.strictEqual(
    gate.platformFromNavigator({ userAgentData: { platform: 'macOS' }, platform: 'Linux' }),
    'darwin',
  )
  // Unknown/absent falls back to Linux: that is the only family where the
  // Windows-only and macOS-only controls are both hidden.
  assert.strictEqual(gate.platformFromNavigator({}), 'linux')
  assert.strictEqual(gate.platformFromNavigator(), 'linux')
})

test('Windows shows every gated control', () => {
  const root = fakeRoot(UI)
  const out = gate.applyPlatformGating(root, gate.capabilitiesFor('win32'))
  assert.strictEqual(out.length, UI.length)
  for (const el of root.els) assert.strictEqual(el.classList.contains('cap-off'), false)
})

test('macOS hides DLSS, kernel wheels, task manager, shortcut and MSVC', () => {
  const root = fakeRoot(UI)
  gate.applyPlatformGating(root, gate.capabilitiesFor('darwin'))
  for (const el of root.els) {
    assert.strictEqual(
      el.classList.contains('cap-off'),
      true,
      `${el.id} must be hidden on macOS`,
    )
  }
})

test('Linux hides only the Windows-only controls', () => {
  const root = fakeRoot(UI)
  gate.applyPlatformGating(root, gate.capabilitiesFor('linux'))
  const hidden = root.els.filter((e) => e.classList.contains('cap-off')).map((e) => e.id)
  assert.deepStrictEqual(hidden, ['dlss5Card', 'taskMgrBtn', 'tsMsvcRow'])
})

test('requirements are ANDed and ungating is reversible', () => {
  const root = fakeRoot([{ id: 'both', requires: 'dlss kernelWheels' }])
  const el = root.els[0]
  gate.applyPlatformGating(root, gate.capabilitiesFor('linux')) // kernelWheels only
  assert.strictEqual(el.classList.contains('cap-off'), true)
  gate.applyPlatformGating(root, gate.capabilitiesFor('win32'))
  assert.strictEqual(el.classList.contains('cap-off'), false)
  gate.applyPlatformGating(root, gate.capabilitiesFor('darwin'))
  assert.strictEqual(el.classList.contains('cap-off'), true)
})

test('unknown or empty requirements fail closed', () => {
  const root = fakeRoot([
    { id: 'typo', requires: 'dlsss' },
    { id: 'empty', requires: '' },
    { id: 'blank', requires: '   ' },
  ])
  const out = gate.applyPlatformGating(root, gate.capabilitiesFor('win32'))
  assert.deepStrictEqual(
    out.map((o) => o.enabled),
    [false, false, false],
  )
  for (const el of root.els) assert.ok(el.classList.contains('cap-off'))
})

test('applyPlatformGating tolerates a missing root', () => {
  assert.deepStrictEqual(gate.applyPlatformGating(null, {}), [])
  assert.deepStrictEqual(gate.applyPlatformGating({}, {}), [])
})

test('index.html gates exactly the controls the map knows', () => {
  const known = new Set(gate.CAPABILITIES)
  const attrs = [...HTML.matchAll(/data-requires="([^"]*)"/g)].map((m) => m[1])
  assert.ok(attrs.length >= UI.length, 'no data-requires attributes found')
  for (const attr of attrs) {
    for (const token of attr.split(/\s+/).filter(Boolean)) {
      assert.ok(known.has(token), `unknown capability "${token}" in index.html`)
    }
  }
  for (const { id, requires } of UI) {
    const tag = HTML.match(new RegExp(`<[^>]*id="${id}"[^>]*>`))
    assert.ok(tag, `#${id} missing from index.html`)
    assert.match(tag[0], new RegExp(`data-requires="${requires}"`), `#${id} not gated`)
  }
})

test('the gate script loads before the bridge that applies it', () => {
  const gateAt = HTML.indexOf('src="services/platform-gate.js"')
  const bridgeAt = HTML.indexOf('src="w2gp.js"')
  assert.ok(gateAt > 0, 'platform-gate.js is not loaded')
  assert.ok(bridgeAt > 0, 'w2gp.js is not loaded')
  assert.ok(gateAt < bridgeAt, 'platform-gate.js must load before w2gp.js')
  assert.match(CSS, /\.cap-off\s*\{[^}]*display:\s*none/, '.cap-off is not styled')
})

/** `assert!(w.supports(Capability::Dlss))` per adapter variable. */
function rustCapabilities(src, varName) {
  const re = new RegExp(
    `assert!\\(\\s*(!?)\\s*${varName}\\.supports\\(Capability::(\\w+)\\)`,
    'g',
  )
  const out = {}
  for (const m of src.matchAll(re)) out[m[2]] = m[1] !== '!'
  return out
}

test('the JS map is pinned to the Rust adapters', () => {
  const names = {
    TaskManager: 'taskManager',
    DesktopShortcut: 'desktopShortcut',
    Dlss: 'dlss',
    KernelWheels: 'kernelWheels',
    TritonRepair: 'tritonRepair',
    AutoStart: 'autoStart',
  }
  const adapters = [
    ['w', 'win32'],
    ['l', 'linux'],
    ['m', 'darwin'],
  ]
  for (const [varName, platform] of adapters) {
    const rust = rustCapabilities(RUST, varName)
    const js = gate.capabilitiesFor(platform)
    const seen = Object.keys(rust)
    assert.strictEqual(
      seen.length,
      Object.keys(names).length,
      `platform/mod.rs must assert every capability for "${varName}" (saw ${seen.join(', ')})`,
    )
    for (const [cap, key] of Object.entries(names)) {
      assert.strictEqual(
        rust[cap],
        js[key],
        `${platform}: ${key} is ${js[key]} in JS but ${rust[cap]} in Rust`,
      )
    }
  }
})

test('no renderer tab still branches on the platform string', () => {
  const dir = path.join(ROOT, 'src')
  for (const file of fs.readdirSync(dir)) {
    if (!file.endsWith('.js') || file === 'platform-gate.js') continue
    const src = fs.readFileSync(path.join(dir, file), 'utf8')
    assert.ok(
      !/navigator\.platform/.test(src),
      `${file} reads navigator.platform directly — use platformGate`,
    )
    assert.ok(
      !/w2gp\.platform\s*[!=]==?/.test(src) && !/!\s*w2gp\.platform/.test(src),
      `${file} compares w2gp.platform — gate the element with data-requires`,
    )
  }
})

test('pathJoin follows the host separator', () => {
  const win = { platform: 'Win32' }
  const lin = { platform: 'Linux x86_64' }
  const mac = { platform: 'MacIntel' }
  assert.strictEqual(gate.pathJoin('C:\\Wan2GP', 'ckpts', win), 'C:\\Wan2GP\\ckpts')
  assert.strictEqual(gate.pathJoin('C:', 'Wan2GP', win), 'C:\\Wan2GP')
  assert.strictEqual(gate.pathJoin('C:\\', 'Wan2GP', win), 'C:\\Wan2GP')
  assert.strictEqual(
    gate.pathJoin('/home/x/Wan2GP', 'ckpts', lin),
    '/home/x/Wan2GP/ckpts',
  )
  assert.strictEqual(
    gate.pathJoin('/home/x/Wan2GP-Models/', 'loras', lin),
    '/home/x/Wan2GP-Models/loras',
  )
  assert.strictEqual(
    gate.pathJoin('/Users/x/Wan2GP', 'outputs', mac),
    '/Users/x/Wan2GP/outputs',
  )
})

test('no renderer file joins paths with a hard-coded backslash', () => {
  const dir = path.join(ROOT, 'src')
  for (const file of fs.readdirSync(dir)) {
    if (!file.endsWith('.js') || file === 'platform-gate.js') continue
    const src = fs.readFileSync(path.join(dir, file), 'utf8')
    assert.ok(
      !/\+\s*"\\\\"\s*\+\s*[a-zA-Z_$]/.test(src),
      `${file} concatenates "\\\\" to build a path — use platformGate.pathJoin`,
    )
  }
})

test('no renderer file hard-codes a path separator variable', () => {
  const dir = path.join(ROOT, 'src')
  for (const file of fs.readdirSync(dir)) {
    if (!file.endsWith('.js') || file === 'platform-gate.js') continue
    const src = fs.readFileSync(path.join(dir, file), 'utf8')
    assert.ok(
      !/=\s*"\\\\"\s*;/.test(src),
      `${file} assigns "\\\\" as a separator — use platformGate.pathJoin`,
    )
  }
})

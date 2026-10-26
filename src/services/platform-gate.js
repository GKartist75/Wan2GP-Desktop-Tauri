/**
 * platform-gate.js — one platform → capability map plus declarative UI gating.
 *
 * The Rust side has the same idea in `src-tauri/src/platform/` (`Platform::
 * supports`). That mapping drives what the *backend* will do; this one drives
 * what the *frontend* shows, and `tests/platform-gate.test.js` pins this file
 * against the Rust asserts so the two cannot drift apart silently.
 *
 * Gating is declarative: an element carries `data-requires="dlss"` (or several
 * space-separated capabilities, ANDed) and `applyPlatformGating` adds
 * `cap-off` when any requirement is unsupported. Unknown capability names fail
 * CLOSED — a typo hides a control instead of exposing a button that always
 * errors.
 *
 * UMD wrapper: the renderer gets a `platformGate` global (loaded via a plain
 * <script> before w2gp.js), node --test gets it via require().
 *
 * @module platform-gate
 */
(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory()
  else root.platformGate = factory()
})(typeof self !== 'undefined' ? self : this, function () {
  /** Every capability the launcher can gate on, in one place. */
  const CAPABILITIES = [
    'taskManager',
    'desktopShortcut',
    'dlss',
    'kernelWheels',
    'tritonRepair',
    'autoStart',
    'prereqInstall',
    'nativeEmbed',
  ]

  /**
   * Normalize the host platform to the names the Rust adapters use.
   * `navigator.userAgentData.platform` is preferred where available (it is not
   * spoofed into a generic value the way UA strings can be); `navigator.platform`
   * is the fallback, including in the Tauri webview where it reads "Win32"/
   * "MacIntel"/"Linux x86_64".
   *
   * @param {{platform?: string, userAgentData?: {platform?: string}}} [nav]
   * @returns {'win32'|'darwin'|'linux'}
   */
  function platformFromNavigator(nav) {
    const raw =
      (nav && nav.userAgentData && nav.userAgentData.platform) ||
      (nav && nav.platform) ||
      ''
    const s = String(raw).toLowerCase()
    if (s.indexOf('win') !== -1) return 'win32'
    if (s.indexOf('mac') !== -1) return 'darwin'
    return 'linux'
  }

  /**
   * Capability map. MUST stay identical to the `supports` impls in
   * `platform/{windows,linux,macos}.rs` — the unit test compares them.
   *
   * @param {'win32'|'darwin'|'linux'} platform
   */
  function capabilitiesFor(platform) {
    const win = platform === 'win32'
    const mac = platform === 'darwin'
    return {
      // taskmgr.exe only: Linux has no standard monitor, and the launcher
      // refuses rather than spawning one the user may not have installed.
      taskManager: win,
      // Windows .lnk, Linux .desktop, macOS deliberately none.
      desktopShortcut: !mac,
      // NVAPI/DLSS runtime is Windows-only.
      dlss: win,
      // CUDA/ROCm wheels: not applicable on Apple Silicon (MPS).
      kernelWheels: !mac,
      // MSVC/Triton repair is Windows tooling.
      tritonRepair: win,
      // XDG autostart / LaunchAgent / Run key on all three.
      autoStart: true,
      // winget/choco/MSI only. Linux and macOS get the package-manager
      // command in place of the button.
      prereqInstall: win,
      // WebView2 composites a child webview over the page. GTK/WKWebView put
      // it in the window's box instead (the launcher page gets squeezed into
      // the other half), so the native Desktop renderer is Windows-only.
      nativeEmbed: win,
    }
  }

  /**
   * Does the current host support `capability`?
   *
   * @param {string} capability one of CAPABILITIES
   * @param {{platform?: string, userAgentData?: {platform?: string}}} [nav]
   * @returns {boolean}
   */
  function has(capability, nav) {
    const src = nav || (typeof navigator !== 'undefined' ? navigator : null)
    return capabilitiesFor(platformFromNavigator(src))[capability] === true
  }

  /**
   * The command a user runs when one-click install is not available, per tool.
   *
   * The Prerequisites card replaces its Install button with this string on
   * Linux and macOS (winget/choco/MSI are Windows-only), so the card tells the
   * user what to type instead of offering a button that cannot work.
   *
   * @param {string} tool git|uv|python|conda|cuda
   * @param {'win32'|'darwin'|'linux'} [platform]
   * @returns {string} empty for an unknown tool
   */
  function prereqCommand(tool, platform) {
    const p = platform || platformFromNavigator(
      typeof navigator !== 'undefined' ? navigator : null,
    )
    const mac = p === 'darwin'
    const table = {
      git: mac ? 'xcode-select --install' : 'sudo apt install git',
      uv: 'curl -LsSf https://astral.sh/uv/install.sh | sh',
      python: mac ? 'brew install python' : 'sudo apt install python3',
      conda: 'Miniconda installer from repo.anaconda.com',
      // Upstream INSTALLATION.md: CUDA 13.1 for RTX 20XX-50XX, 12.8 for
      // GTX 10XX. Not a distro package (too old there), so point at NVIDIA's
      // runfile archive rather than `apt install nvidia-cuda-toolkit`.
      cuda: mac ? '' : 'NVIDIA CUDA toolkit (13.1 / 12.8) — developer.nvidia.com/cuda-downloads',
    }
    return table[tool] || ''
  }

  /**
   * Can the launcher run an installer for `tool` on this host?
   *
   * One-click install is winget/choco/MSI, i.e. Windows-only — with one
   * exception: uv ships a user-scope installer (`curl … | sh`, no sudo) that
   * the launcher runs into its own `<dataDir>/.tools`, so that row keeps its
   * button off Windows too. `git`, `python` and `conda` need root or a GUI
   * installer and keep showing the command to type.
   *
   * MUST agree with `prereq_one_click_on` in `src-tauri/src/install.rs` (the
   * backend rejects a tool the frontend offers, and vice versa).
   *
   * @param {string} tool git|uv|python|conda|cuda
   * @param {{platform?: string, userAgentData?: {platform?: string}}} [nav]
   * @returns {boolean}
   */
  function prereqInstallable(tool, nav) {
    // The CUDA toolkit is a vendor runfile download, never a package-manager
    // install — that row always answers with the URL.
    if (tool === 'cuda') return false
    return tool === 'uv' || has('prereqInstall', nav)
  }

  /**
   * Join two path fragments with the host's separator.
   *
   * The renderer has no `node:path`, and every caller used to concatenate
   * "\\" — which on Linux/macOS produced `~/Wan2GP-Models\ckpts`, a single
   * file name containing a backslash instead of a subdirectory. Windows keeps
   * "\\" because that is what its own dialogs show; everything else gets "/".
   * A trailing separator on `a` is collapsed, so a bare drive root works:
   * `pathJoin("D:", "Wan2GP")` → `D:\Wan2GP`.
   *
   * @param {string} a base path
   * @param {string} b child name
   * @param {{platform?: string, userAgentData?: {platform?: string}}} [nav]
   * @returns {string}
   */
  function pathJoin(a, b, nav) {
    const src = nav || (typeof navigator !== 'undefined' ? navigator : null)
    const sep = platformFromNavigator(src) === 'win32' ? '\\' : '/'
    return String(a || '').replace(/[\\/]+$/, '') + sep + String(b || '')
  }

  /**
   * Apply `data-requires` gating under `root`.
   *
   * @param {{querySelectorAll?: Function}} root document or any element
   * @param {Record<string, boolean>} capabilities from `capabilitiesFor`
   * @returns {Array<{id: string, requires: string[], enabled: boolean}>}
   */
  function applyPlatformGating(root, capabilities) {
    const out = []
    if (!root || typeof root.querySelectorAll !== 'function') return out
    const caps = capabilities || {}
    root.querySelectorAll('[data-requires]').forEach(function (el) {
      const requires = String(el.getAttribute('data-requires') || '')
        .split(/\s+/)
        .filter(Boolean)
      const enabled =
        requires.length > 0 &&
        requires.every(function (c) {
          return caps[c] === true
        })
      el.classList.toggle('cap-off', !enabled)
      out.push({ id: el.id || '', requires: requires, enabled: enabled })
    })
    return out
  }

  return {
    CAPABILITIES: CAPABILITIES,
    platformFromNavigator: platformFromNavigator,
    pathJoin: pathJoin,
    has: has,
    prereqCommand: prereqCommand,
    prereqInstallable: prereqInstallable,
    capabilitiesFor: capabilitiesFor,
    applyPlatformGating: applyPlatformGating,
  }
})

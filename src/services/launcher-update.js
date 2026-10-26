/**
 * launcher-update.js — the copy for "how does THIS copy of the launcher update?"
 *
 * The in-app updater can only swap a packaged app (NSIS/MSI, AppImage, .deb/
 * .rpm, .app). Everything else — a cargo build, a portable extraction, a release
 * that carries no asset for this platform yet — has to be told what to do
 * instead. The backend decides the *kind* (`LauncherInstall` in updates.rs) and
 * the shell command; this module owns the words, so the two cannot drift into
 * "download the installer" on a Linux box.
 *
 * UMD wrapper: renderer gets `launcherUpdate`, node --test gets require().
 *
 * @module launcher-update
 */
(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory()
  else root.launcherUpdate = factory()
})(typeof self !== 'undefined' ? self : this, function () {
  /**
   * What the user does after the download lands, per install kind. Keys MUST
   * match `LauncherInstall::as_str` in updates.rs — the test parses that file.
   */
  const HINTS = {
    appimage: 'download the new .AppImage and replace this file, then chmod +x it',
    deb: 'download the .deb, then install it with the command below',
    rpm: 'download the .rpm, then install it with the command below',
    macapp: 'download the .dmg and drag the app into Applications',
    windows: 'download the installer and run it',
    source: 'this copy runs from a build directory — git pull, then rebuild',
  }

  /** Short name for the artifact this install consumes. */
  const KIND_LABEL = {
    appimage: 'AppImage',
    deb: '.deb',
    rpm: '.rpm',
    macapp: '.dmg',
    windows: 'installer',
    source: 'source',
  }

  /** Releases page — the fallback when the API gave us no asset URL. */
  const RELEASES_URL =
    'https://github.com/GKartist75/Wan2GP-Desktop-Tauri/releases/latest'

  /** Unknown kinds fail closed to the source wording, never to "installer". */
  function hint(kind) {
    return HINTS[kind] || HINTS.source
  }

  function kindLabel(kind) {
    return KIND_LABEL[kind] || KIND_LABEL.source
  }

  /** Verb on the update control: a source build links out, the rest download. */
  function actionText(kind) {
    return kind === 'source' ? 'Releases' : 'Download'
  }

  /**
   * One line for the Manage status area. `release` is the backend payload:
   * `{version, installKind, assetName, command}`.
   */
  function statusLine(release) {
    if (!release || !release.version) return ''
    const kind = release.installKind
    const artifact = release.assetName ? ` (${release.assetName})` : ''
    return `v${release.version} available — ${hint(kind)}${artifact}`
  }

  /** Where the download button goes. */
  function downloadUrl(release) {
    if (!release) return RELEASES_URL
    return release.url || release.htmlUrl || RELEASES_URL
  }

  /** The shell step to copy, if this kind needs one. */
  function commandFor(release) {
    if (!release || !release.command) return ''
    return String(release.command)
  }

  return {
    HINTS,
    KIND_LABEL,
    RELEASES_URL,
    hint,
    kindLabel,
    actionText,
    statusLine,
    downloadUrl,
    commandFor,
  }
})

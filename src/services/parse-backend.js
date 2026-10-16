/**
 * parse-backend.js — parse WanGP's boot backend reports.
 *
 * WanGP prints its selected kernel backends at boot, e.g.
 * `[INT8] Backend: WGPKernels (setting: auto).` The launcher mirrors those
 * lines into the Active Environment rows so users see which backend is
 * actually running without digging the console.
 *
 * UMD wrapper: renderer gets a `parseBackendLine` global (loaded via a plain
 * <script> before resetui-tab.js), node --test gets it via require().
 *
 * @module parse-backend
 */
(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory()
  else root.parseBackendLine = factory()
})(typeof self !== 'undefined' ? self : this, function () {
  /**
   * Parse one log line for an INT8/FP8 backend report.
   * @param {*} line raw log line (child-tag prefixes tolerated)
   * @returns {{kind: 'INT8'|'FP8', name: string} | null}
   */
  function parseBackendLine(line) {
    const s = line == null ? '' : String(line)
    const m = s.match(/\[(INT8|FP8)\]\s*Backend:\s*([^()]+?)\s*\(setting:/)
    if (!m) return null
    const name = m[2].trim().replace(/\.+$/, '')
    if (!name) return null
    return { kind: m[1], name }
  }
  return parseBackendLine
})

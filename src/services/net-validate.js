/**
 * net-validate.js — pure validators for F8 network & TLS flags.
 *
 * Mirrors upstream shared/authentication/__init__.py::parse_public_url:
 * scheme http/https, hostname required, no credentials, path '' or '/',
 * no whitespace/control chars, no ? # \ *, port 1-65535 when present.
 * No Tauri, no Node, no DOM. Copy to .cjs for node checks.
 */

'use strict'

/**
 * @param {string} value user input
 * @returns {{ok:boolean, normalized?:string, error?:string}}
 */
function validatePublicUrl(value) {
  const v = String(value == null ? '' : value).trim()
  if (!v) return { ok: false, error: 'Empty — leave blank for direct use' }
  if (/\s/.test(v) || /[#?\\*]/.test(v)) {
    return { ok: false, error: 'No spaces, ?, #, \\ or * allowed' }
  }
  let u
  try {
    u = new URL(v)
  } catch (e) {
    return { ok: false, error: 'Not a valid URL' }
  }
  if (u.protocol !== 'http:' && u.protocol !== 'https:') {
    return { ok: false, error: 'Scheme must be http or https' }
  }
  if (!u.hostname) return { ok: false, error: 'Hostname required' }
  if (u.username || u.password) return { ok: false, error: 'No credentials in URL' }
  if (u.pathname !== '/' && u.pathname !== '') {
    return { ok: false, error: 'Bare origin only — no path (no /deepy/)' }
  }
  if (u.search || u.hash) return { ok: false, error: 'No query or fragment' }
  if (u.port) {
    const p = Number(u.port)
    if (!Number.isInteger(p) || p < 1 || p > 65535) {
      return { ok: false, error: 'Port must be 1-65535' }
    }
  }
  const normalized = u.protocol + '//' + u.host
  return { ok: true, normalized }
}

if (typeof module !== 'undefined' && module.exports) {
  module.exports = { validatePublicUrl }
}

/**
 * launch-args.js — pure, side-effect-free helpers for the Launch args builder (F2).
 *
 * Token-aware edit of a wgp.py arg string: set/replace `--flag value` pairs or
 * boolean `--flag`s without duplicating tokens and without touching unknown
 * user args. Quoting: values with whitespace are wrapped in double quotes.
 *
 * No Tauri, no Node, no DOM. Unit-testable by copying to .cjs (see gcheck).
 */

'use strict'

/** Split an arg string on whitespace (no quote-awareness needed for our flags). */
function splitArgs(s) {
  return String(s == null ? '' : s).split(/\s+/).filter(Boolean)
}

/** Quote a value if it contains whitespace. */
function quoteVal(v) {
  v = String(v)
  return /\s/.test(v) ? '"' + v.replace(/"/g, '') + '"' : v
}

/**
 * Set one flag in a token list.
 * @param {string[]} toks token list (mutated copy expected — pass a fresh array)
 * @param {string} flag e.g. "--profile"
 * @param {string|null} value null/'' removes the flag; otherwise sets `--flag value`
 *   (boolean flags: pass value TRUE and takesValue=false)
 * @param {boolean} takesValue
 * @returns {string[]} the same array
 */
function setFlag(toks, flag, value, takesValue) {
  const idx = toks.indexOf(flag)
  const hasValue = value !== null && value !== undefined && String(value) !== ''
  if (idx === -1) {
    if (!hasValue) return toks
    toks.push(flag)
    if (takesValue !== false && hasValue) toks.push(quoteVal(value))
    return toks
  }
  // Remove existing occurrence + its value (if it takes one and next token isn't a flag).
  let end = idx + 1
  if (takesValue !== false && end < toks.length && !toks[end].startsWith('--')) end += 1
  toks.splice(idx, end - idx)
  if (!hasValue) return toks
  toks.splice(idx, 0, flag)
  if (takesValue !== false) toks.splice(idx + 1, 0, quoteVal(value))
  return toks
}

/**
 * Apply a patch object { flag: value } over an arg string.
 * Boolean flags use takesValue=false entries in BOOL_FLAGS.
 */
const BOOL_FLAGS = new Set(['--compile', '--fp16', '--bf16', '--listen', '--share', '--advanced', '--check-loras'])

function applyPatch(argStr, patch) {
  const toks = splitArgs(argStr)
  for (const flag of Object.keys(patch)) {
    setFlag(toks, flag, patch[flag], !BOOL_FLAGS.has(flag))
  }
  return toks.join(' ')
}

/** Named presets (F2). Values mirror docs/WAN2GP-GUIDE.md + upstream TROUBLESHOOTING. */
const LAUNCH_PRESETS = {
  balanced: '--attention sage2 --profile 4',
  lowvram: '--attention sdpa --profile 5 --fp16 --teacache 1.5',
  maxperf: '--compile --attention sage2 --profile 3 --teacache 2.0',
  emergency: '--attention sdpa --profile 4 --teacache 0 --fp16'
}

if (typeof module !== 'undefined' && module.exports) {
  module.exports = { splitArgs, setFlag, applyPatch, BOOL_FLAGS, LAUNCH_PRESETS }
}

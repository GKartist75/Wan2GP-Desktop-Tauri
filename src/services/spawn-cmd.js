/**
 * services/spawn-cmd.js — spawn a command on any platform WITHOUT the classic
 * Windows breakage modes:
 *
 *   1. "'C:\Program' is not recognized"  — a path WITH A SPACE (e.g. the default
 *      `C:\Program Files\nodejs\npm.cmd`) passed unquoted into `cmd /c` gets split
 *      on the space, so cmd tries to run `C:\Program`.
 *   2. "spawn EINVAL"  — a `.cmd`/`.bat` batch file spawned DIRECTLY (shell:false)
 *      is rejected by Windows CreateProcess, which throws EINVAL. Batch files MUST
 *      run through cmd.exe (a shell).
 *
 * The single correct rule:
 *   - If the resolved bin ends in `.cmd`/`.bat`  → run via `shell: true` with the
 *     bin QUOTED so the spaced path stays one token. (Windows npm/opencode shims.)
 *   - Otherwise (`.exe`, or a POSIX binary/script) → spawn DIRECTLY (shell:false);
 *     Node passes argv[0] whole, so spaces are inherently safe and there is no
 *     shell to misinterpret metacharacters.
 *
 * Args are assumed to be static/safe (validated callers: npm package name regex,
 * fixed serve flags). They are joined for the shell case; user-derived args
 * are escaped (cmd.exe double-quote rules) with a loud warning so a future
 * caller can't silently introduce injection.
 *
 * @param {string} bin   resolved executable (from resolveCmd, may contain spaces)
 * @param {string[]} args argv (excluding argv[0])
 * @param {object} [opts] extra child_process.spawn options (cwd, env, timeout…)
 * @returns {import('child_process').ChildProcess}
 */
function isBatchFile(bin) {
  return /\.(cmd|bat)$/i.test(bin)
}

// Single safe token: word chars plus the punctuation fixed flags/paths need.
// NOTE: this class also matches leading dashes, so option-like tokens get a
// dedicated check below — quoting neutralizes shell metacharacters but can
// never neutralize a flag (`"--flag"` is still a flag to the callee).
const SAFE_ARG_RE = /^[\w.:/-]+$/

// cmd.exe quoting: wrap in double quotes, double any embedded quotes.
function escapeArg(a) {
  return `"${String(a).replace(/"/g, '""')}"`
}

// Classify one arg: null = static-safe, 'chars' = shell-risky (escaped),
// 'flag' = option-like (warned; must be a conscious caller choice).
function argRisk(a) {
  const s = String(a)
  if (!SAFE_ARG_RE.test(s)) return 'chars'
  if (s.startsWith('-')) return 'flag'
  return null
}

function spawnCmd(bin, args = [], opts = {}) {
  const { spawn } = require('child_process')
  const risky = args.map((a) => ({ arg: a, risk: argRisk(a) })).filter((r) => r.risk)
  if (risky.length) console.warn('[spawn-cmd] non-static args:', risky)
  if (isBatchFile(bin)) {
    // Quote the bin so "C:\Program Files\..." is one token; /s makes cmd honor it.
    // Safe args join verbatim; shell-risky args are cmd.exe-quoted (fail-closed).
    const joined = args.map((a) => (argRisk(a) === 'chars' ? escapeArg(a) : a)).join(' ')
    const command = `"${bin}" ${joined}`
    return spawn(command, [], { ...opts, shell: true })
  }
  return spawn(bin, args, { ...opts, shell: false })
}

module.exports = { spawnCmd, isBatchFile, escapeArg, SAFE_ARG_RE, argRisk }

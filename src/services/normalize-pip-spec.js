/**
 * normalize-pip-spec.js — accept either a bare pip spec
 * (claude-agent-sdk==0.1.40) or a full command pasted by a user
 * (pip install claude-agent-sdk==0.1.40 / pip3 install foo /
 * python -m pip install ...) and strip the leading pip invocation plus any
 * pip flags so the preview and the real install handler see the same spec.
 * Pure + offline-testable. UX normalization only — the backend re-validates
 * the result (services/pip-spec.js + Rust port), so nothing here is a
 * security boundary.
 */
function normalizePipSpec(raw) {
  let s = (raw || '').trim()
  const m = s.match(/^(?:py(?:thon)?\s+-m\s+)?pip3?\s+install\s+/i)
  if (m) s = s.slice(m[0].length).trim()
  // Strip pip flags (`pip install foo --upgrade` → `foo`).
  s = s.split(/\s+/).filter((t) => !t.startsWith('-')).join(' ')
  return s
}

module.exports = { normalizePipSpec }

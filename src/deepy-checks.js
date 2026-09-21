/**
 * deepy-checks.js — wiring for System → Deepy engine checks + Network & TLS (F8).
 *
 * Engine presence reuses llm_engines_list (same source as the Dashboard card);
 * the Claude bridge pin is checked via check_package. Public-URL validation is
 * offline (services/net-validate.js); staging writes into Extra Launch Args
 * through the Launch builder hook (or directly into the input as fallback).
 * All lookups guarded.
 */
(function () {
  'use strict'

  function $(id) { return document.getElementById(id) }

  function esc(s) {
    return String(s == null ? '' : s)
      .replace(/&/g, '&amp;').replace(/</g, '&lt;')
      .replace(/>/g, '&gt;').replace(/"/g, '&quot;')
  }

  function stageFlags(patchObj, status, okMsg) {
    if (typeof window.w2gpLaunchPatch === 'function') {
      window.w2gpLaunchPatch(patchObj)
    } else if ($('launchArgsInput')) {
      // Fallback: append-only when the builder hook is unavailable.
      const cur = $('launchArgsInput').value || ''
      const add = Object.keys(patchObj)
        .filter((f) => patchObj[f] !== null && patchObj[f] !== undefined && String(patchObj[f]) !== '')
        .map((f) => (patchObj[f] === true ? f : f + ' ' + patchObj[f]))
        .join(' ')
      $('launchArgsInput').value = (cur + ' ' + add).trim()
    }
    if (status) status.textContent = okMsg + ' — press Save in Manage → Launch'
  }

  async function runEngineChecks() {
    const box = $('deepyChecksBox')
    const status = $('deepyChecksStatus')
    if (!box) return
    box.innerHTML = '<p class="token-hint">Checking… (spawns probes, takes a few seconds)</p>'
    const rows = []
    try {
      const data = await window.w2gp.llmEnginesList()
      const engines = (data && data.engines) || []
      const find = (id) => engines.find((e) => e.id === id)
      const bin = (e) => {
        if (!e) return { ok: false, note: 'not reported' }
        if (e.cliOnPath) return { ok: true, note: 'binary on PATH' }
        if (e.pipInstalled) return { ok: true, note: 'pip package installed' }
        if (e.external) return { ok: false, note: 'external — install, then it auto-detects' }
        return { ok: false, note: 'missing' }
      }
      const oc = bin(find('opencode'))
      rows.push({ name: 'OpenCode binary', ok: oc.ok, note: oc.note + ' — auto-starts `opencode serve`, /connect for providers' })
      const cl = bin(find('claude-code') || find('claude'))
      rows.push({ name: 'Claude Code binary', ok: cl.ok, note: cl.note + ' — then `claude auth login`' })
      const cx = bin(find('codex'))
      rows.push({ name: 'Codex binary', ok: cx.ok, note: cx.note + ' — browser sign-in on first Deepy request' })
    } catch (e) {
      rows.push({ name: 'Engine binaries', ok: false, note: 'probe failed: ' + ((e && e.message) || e) })
    }
    try {
      const sdk = await window.w2gp.checkPackage('claude-agent-sdk')
      const pinned = sdk && sdk.installed && String(sdk.version || '').trim() === '0.1.66'
      rows.push({
        name: 'claude-agent-sdk==0.1.66',
        ok: !!pinned,
        note: (sdk && sdk.installed) ? ('installed ' + sdk.version + (pinned ? ' (pinned ✓)' : ' — want exactly 0.1.66, reinstall')) : 'not installed (needed for Claude Code)'
      })
    } catch (e) {
      rows.push({ name: 'claude-agent-sdk==0.1.66', ok: false, note: 'probe failed' })
    }
    box.innerHTML = '<table class="args-table">' +
      rows.map((r) => (
        '<tr><td>' + (r.ok ? '✓' : '✗') + '</td><td><strong>' + esc(r.name) + '</strong><br><span class="token-hint">' + esc(r.note) + '</span></td></tr>'
      )).join('') + '</table>'
    if (status) status.textContent = 'Remote engines need Deepy Prime; local Qwen needs nothing here'
  }

  function init() {
    if (!$('deepyChecksBox')) return
    $('deepyChecksRunBtn')?.addEventListener('click', runEngineChecks)
    $('netUrlValidateBtn')?.addEventListener('click', () => {
      const status = $('netStatus')
      const input = ($('netPublicUrlInput')?.value || '').trim()
      if (typeof validatePublicUrl !== 'function') return
      if (!input) { if (status) status.textContent = 'Empty — leave blank for direct Same-PC / Phone-LAN use'; return }
      const r = validatePublicUrl(input)
      if (!r.ok) { if (status) status.textContent = 'Invalid: ' + r.error; return }
      if ($('netPublicUrlInput')) $('netPublicUrlInput').value = r.normalized
      stageFlags({ '--public-url': r.normalized }, status, 'Staged --public-url ' + r.normalized)
    })
    $('netTlsStageBtn')?.addEventListener('click', () => {
      const status = $('netStatus')
      const cert = ($('netCertInput')?.value || '').trim()
      const key = ($('netKeyInput')?.value || '').trim()
      const port = ($('netHttpsPortInput')?.value || '').trim()
      if (!cert || !key) { if (status) status.textContent = 'Certificate + key paths required (or use a reverse proxy)'; return }
      const patch = { '--ssl-certfile': cert, '--ssl-keyfile': key }
      if (port) patch['--https-port'] = port
      stageFlags(patch, status, 'Staged TLS flags')
    })
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init)
  } else {
    init()
  }
})()

/**
 * launch-builder.js — wiring for Manage → Launch presets & flag builder (F2).
 *
 * Preset buttons replace the Extra Launch Args field with a known-good string;
 * per-flag controls patch single flags token-aware (services/launch-args.js).
 * Saving still goes through the existing Save button flow (desktop-config.json).
 * Loaded after app.js; all lookups guarded.
 */
(function () {
  'use strict'

  function $(id) { return document.getElementById(id) }

  function current() { return ($('launchArgsInput') && $('launchArgsInput').value) || '' }
  function set(v) { if ($('launchArgsInput')) $('launchArgsInput').value = v }

  function patch(patchObj) {
    if (typeof applyPatch === 'function') set(applyPatch(current(), patchObj))
  }

  function init() {
    if (!$('launchArgsInput')) return
    document.querySelectorAll('[data-launch-preset]').forEach((btn) => {
      btn.addEventListener('click', () => {
        const name = btn.getAttribute('data-launch-preset')
        const presets = (typeof LAUNCH_PRESETS !== 'undefined') ? LAUNCH_PRESETS : {}
        if (presets[name] !== undefined) {
          set(presets[name])
          const st = $('launchBuilderStatus')
          if (st) st.textContent = 'Preset "' + name + '" staged — press Save'
        }
      })
    })
    $('lbAttention')?.addEventListener('change', (e) => patch({ '--attention': e.target.value || null }))
    $('lbProfile')?.addEventListener('change', (e) => patch({ '--profile': e.target.value || null }))
    $('lbTeacache')?.addEventListener('change', (e) => patch({ '--teacache': e.target.value || null }))
    $('lbCompile')?.addEventListener('change', (e) => patch({ '--compile': e.target.checked ? true : null }))
    $('lbFp16')?.addEventListener('change', (e) => patch({ '--fp16': e.target.checked ? true : null }))
    initMcpHelper()
  }

  function mcpCommand() {
    const t = ($('mcpTransport') && $('mcpTransport').value) || 'stdio'
    const host = ($('mcpHost') && $('mcpHost').value.trim()) || '127.0.0.1'
    const port = ($('mcpPort') && $('mcpPort').value) || '7866'
    const api = ($('mcpApi') && $('mcpApi').value) || '2'
    if (t === 'stdio') return 'python wgp.py --mcp --mcp-api-version ' + api
    return 'python wgp.py --mcp --mcp-transport ' + t + ' --mcp-host ' + host + ' --mcp-port ' + port + ' --mcp-api-version ' + api
  }

  function initMcpHelper() {
    if (!$('mcpCommand')) return
    const refresh = () => {
      $('mcpCommand').value = mcpCommand()
      const t = ($('mcpTransport') && $('mcpTransport').value) || 'stdio'
      const hint = $('mcpUrlHint')
      if (hint) {
        if (t === 'stdio') {
          hint.textContent = 'Local clients spawn this command — no URL, no OAuth.'
        } else {
          const host = ($('mcpHost') && $('mcpHost').value.trim()) || '127.0.0.1'
          const port = ($('mcpPort') && $('mcpPort').value) || '7866'
          const path = (t === 'sse') ? '/sse' : '/mcp'
          hint.textContent = 'Connect clients to http://' + host + ':' + port + path + ' — localhost only unless --mcp-auth + HTTPS.'
        }
      }
    }
    ;['mcpTransport', 'mcpHost', 'mcpPort', 'mcpApi'].forEach((id) => {
      $(id)?.addEventListener('change', refresh)
      $(id)?.addEventListener('input', refresh)
    })
    $('mcpCopyBtn')?.addEventListener('click', () => {
      const cmd = $('mcpCommand').value
      const done = (ok) => { if ($('mcpUrlHint')) $('mcpUrlHint').textContent = ok ? 'Command copied' : 'Copy failed — select manually' }
      if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(cmd).then(() => done(true), () => done(false))
      } else { done(false) }
    })
    refresh()
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init)
  } else {
    init()
  }
})()

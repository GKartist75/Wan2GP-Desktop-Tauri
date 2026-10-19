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

  // Hook for other cards (F8 network staging) to patch launch args.
  try { window.w2gpLaunchPatch = patch } catch (e) {}

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

  function mcpOpts() {
    return {
      transport: ($('mcpTransport') && $('mcpTransport').value) || 'stdio',
      host: ($('mcpHost') && $('mcpHost').value) || '127.0.0.1',
      port: ($('mcpPort') && $('mcpPort').value) || '7866',
      api: ($('mcpApi') && $('mcpApi').value) || '2.1',
      webui: ($('mcpWebui') && $('mcpWebui').value) || '',
      auth: $('mcpAuth')?.checked === true,
      authUrl: ($('mcpAuthUrl') && $('mcpAuthUrl').value) || '',
      authPassword: ($('mcpAuthPassword') && $('mcpAuthPassword').value) || ''
    }
  }

  function initMcpHelper() {
    if (!$('mcpCommand')) return
    // mcpCommand/mcpHint come from services/mcp-command.js (pure, unit-tested).
    const build = (typeof mcpCommand === 'function') ? mcpCommand : () => 'python wgp.py --mcp'
    const hintFor = (typeof mcpHint === 'function') ? mcpHint : () => ''
    const refresh = () => {
      const opts = mcpOpts()
      // --mcp-transport only applies to a standalone --mcp server.
      const tSel = $('mcpTransport')
      if (tSel) tSel.disabled = opts.webui !== ''
      $('mcpCommand').value = build(opts)
      const hint = $('mcpUrlHint')
      if (hint) hint.textContent = hintFor(opts)
    }
    ;['mcpTransport', 'mcpHost', 'mcpPort', 'mcpApi', 'mcpWebui', 'mcpAuth', 'mcpAuthUrl', 'mcpAuthPassword'].forEach((id) => {
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
    initBatchRunner()
  }

  function initBatchRunner() {
    if (!$('batchRunBtn')) return
    $('batchRunBtn').addEventListener('click', async () => {
      const status = $('batchStatus')
      const btn = $('batchRunBtn')
      const queue = ($('batchQueueInput')?.value || '').trim().replace(/^"|"$/g, '')
      const out = ($('batchOutputInput')?.value || '').trim().replace(/^"|"$/g, '')
      const dry = $('batchDryRun')?.checked === true
      if (!queue) { if (status) status.textContent = 'Pick a saved queue (.zip) or settings (.json) first'; return }
      btn.disabled = true
      if (status) status.textContent = dry ? 'Validating…' : 'Running — watch the console…'
      try {
        const r = await window.w2gp.batchProcess(queue, out || null, dry)
        if (status) status.textContent = (r && r.ok) ? 'Done — exit 0' : ('Finished with exit ' + ((r && r.code) ?? '?'))
      } catch (e) {
        if (status) status.textContent = 'Failed: ' + ((e && e.message) || e)
      } finally {
        btn.disabled = false
      }
    })
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init)
  } else {
    init()
  }
})()

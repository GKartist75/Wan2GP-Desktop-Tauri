/**
 * safety-tab.js — wiring for System → Config backups & changelog (F11).
 *
 * Every launcher Apply (memory profile, Deepy, Manage writes) snapshots
 * wgp_config.json first (newest 5 kept). Restore re-snapshots first, so a
 * restore is itself undoable. All lookups guarded.
 */
(function () {
  'use strict'

  function $(id) { return document.getElementById(id) }

  function esc(s) {
    return String(s == null ? '' : s)
      .replace(/&/g, '&amp;').replace(/</g, '&lt;')
      .replace(/>/g, '&gt;').replace(/"/g, '&quot;')
  }

  async function refreshBackups() {
    const box = $('cfgBackupsBox')
    const status = $('cfgBackupsStatus')
    if (!box) return
    box.innerHTML = '<p class="token-hint">Loading…</p>'
    try {
      const r = await window.w2gp.configBackupsList()
      if (!r || r.ok === false || !r.items.length) {
        box.innerHTML = '<p class="token-hint">No backups yet — they appear here after the first Apply.</p>'
        if (status) status.textContent = ''
        return
      }
      box.innerHTML = '<table class="args-table">' +
        r.items.map((it) => (
          '<tr><td><code>' + esc(it.name) + '</code></td>' +
          '<td><button class="btn btn-ghost small" data-cfg-restore="' + esc(it.name) + '" title="Restore this backup (current config is snapshotted first)">Restore</button></td></tr>'
        )).join('') + '</table>'
      box.querySelectorAll('[data-cfg-restore]').forEach((btn) => {
        btn.addEventListener('click', async () => {
          const name = btn.getAttribute('data-cfg-restore')
          if (!window.confirm('Restore "' + name + '"? Current config is snapshotted first, so this is undoable. WanGP picks it up on next launch.')) return
          try {
            await window.w2gp.configBackupRestore(name)
            if (status) status.textContent = 'Restored ' + name
            refreshBackups()
          } catch (e) {
            if (status) status.textContent = 'Restore failed: ' + ((e && e.message) || e)
          }
        })
      })
      if (status) status.textContent = r.items.length + ' backup(s), newest first'
    } catch (e) {
      box.innerHTML = '<p class="token-hint">Failed: ' + esc((e && e.message) || e) + '</p>'
    }
  }

  function init() {
    if (!$('cfgBackupsBox')) return
    $('cfgBackupsRefreshBtn')?.addEventListener('click', refreshBackups)
    $('cfgChangelogBtn')?.addEventListener('click', async () => {
      const box = $('cfgChangelogBox')
      const status = $('cfgBackupsStatus')
      if (!box) return
      box.textContent = 'Loading…'
      try {
        const r = await window.w2gp.upstreamChangelog()
        box.textContent = (r && r.ok !== false) ? r.lines : ((r && r.error) || 'Failed')
        if (status && r && r.ok !== false) status.textContent = 'Upstream changelog (first 150 lines)'
      } catch (e) {
        box.textContent = 'Failed: ' + ((e && e.message) || e)
      }
    })
    refreshBackups()
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init)
  } else {
    init()
  }
})()

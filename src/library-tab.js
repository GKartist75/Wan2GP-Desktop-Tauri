/**
 * library-tab.js — wiring for Manage → Library (F4 LoRA + finetune librarians).
 *
 * Read-only lists via library_loras / library_finetunes; import copies a .json
 * into finetunes/ (backend validates name + content), delete removes it, export
 * downloads the JSON through a Blob. All lookups guarded; safe standalone.
 */
(function () {
  'use strict'

  function $(id) { return document.getElementById(id) }

  function esc(s) {
    return String(s == null ? '' : s)
      .replace(/&/g, '&amp;').replace(/</g, '&lt;')
      .replace(/>/g, '&gt;').replace(/"/g, '&quot;')
  }

  function fmtBytes(n) {
    n = Number(n) || 0
    if (n >= 1073741824) return (n / 1073741824).toFixed(1) + ' GB'
    if (n >= 1048576) return Math.round(n / 1048576) + ' MB'
    if (n >= 1024) return Math.round(n / 1024) + ' KB'
    return n + ' B'
  }

  async function refreshLoras() {
    const box = $('libLorasBox')
    const status = $('libLorasStatus')
    if (!box) return
    box.innerHTML = '<p class="token-hint">Loading…</p>'
    try {
      const r = await window.w2gp.libraryLoras()
      if (!r || r.ok === false) {
        box.innerHTML = '<p class="token-hint">' + esc((r && r.error) || 'No LoRA root found') + '</p>'
        return
      }
      if (!r.folders.length) {
        box.innerHTML = '<p class="token-hint">Empty — root: <code>' + esc(r.root) + '</code></p>'
        return
      }
      box.innerHTML = '<table class="args-table">' +
        '<tr><td><strong>Family</strong></td><td><strong>Files</strong></td><td><strong>Size</strong></td><td><strong>URLs known</strong></td></tr>' +
        r.folders.map((f) => (
          '<tr><td><code>' + esc(f.name) + '</code>' +
          (f.truncated ? ' <span class="token-hint">(first 200 shown in WanGP)</span>' : '') +
          '</td><td>' + esc(f.files) + '</td><td>' + esc(fmtBytes(f.bytes)) + '</td>' +
          '<td>' + esc(f.urls_known) + ' / ' + esc(f.files) + '</td></tr>'
        )).join('') + '</table>' +
        '<p class="token-hint">Root: <code>' + esc(r.root) + '</code> — URLs known come from <code>loras_url_cache_v2.json</code> (share it to save friends re-hunting LoRAs).</p>'
      if (status) status.textContent = r.folders.length + ' families'
    } catch (e) {
      box.innerHTML = '<p class="token-hint">Failed: ' + esc((e && e.message) || e) + '</p>'
    }
  }

  async function refreshFinetunes() {
    const box = $('libFinetunesBox')
    const status = $('libFinetunesStatus')
    if (!box) return
    box.innerHTML = '<p class="token-hint">Loading…</p>'
    try {
      const r = await window.w2gp.libraryFinetunes()
      if (!r || r.ok === false) {
        box.innerHTML = '<p class="token-hint">Failed to list finetunes.</p>'
        return
      }
      if (!r.items.length) {
        box.innerHTML = '<p class="token-hint">None yet — put shared <code>*.json</code> definitions in <code>' + esc(r.dir) + '</code> or import one below. Never edit <code>defaults/</code>.</p>'
        return
      }
      box.innerHTML = '<table class="args-table">' +
        '<tr><td><strong>Id</strong></td><td><strong>Base</strong></td><td><strong>URLs / LoRAs</strong></td><td><strong></strong></td></tr>' +
        r.items.map((it) => (
          '<tr><td><code>' + esc(it.id) + '</code><br><span class="token-hint">' + esc(it.name || '') + '</span>' +
          (it.error ? '<br><span class="token-hint">' + esc(it.error) + '</span>' : '') +
          '</td><td><code>' + esc(it.architecture || '?') + '</code></td>' +
          '<td>' + esc(it.urls || 0) + (it.urls2 ? '+' + esc(it.urls2) : '') + ' / ' + esc(it.loras || 0) + '</td>' +
          '<td><button class="btn btn-ghost small" data-lib-export="' + esc(it.id) + '" title="Download this finetune JSON to share">Export</button> ' +
          '<button class="btn btn-ghost small" data-lib-delete="' + esc(it.id) + '" title="Delete this finetune definition (weights untouched)">Delete</button></td></tr>'
        )).join('') + '</table>'
      box.querySelectorAll('[data-lib-export]').forEach((btn) => {
        btn.addEventListener('click', async () => {
          const id = btn.getAttribute('data-lib-export')
          try {
            const r2 = await window.w2gp.libraryFinetuneContent(id)
            const blob = new Blob([r2.content], { type: 'application/json' })
            const a = document.createElement('a')
            a.href = URL.createObjectURL(blob)
            a.download = id + '.json'
            document.body.appendChild(a)
            a.click()
            setTimeout(() => { URL.revokeObjectURL(a.href); a.remove() }, 1000)
          } catch (e) {
            if (status) status.textContent = 'Export failed: ' + ((e && e.message) || e)
          }
        })
      })
      box.querySelectorAll('[data-lib-delete]').forEach((btn) => {
        btn.addEventListener('click', async () => {
          const id = btn.getAttribute('data-lib-delete')
          if (!window.confirm('Delete finetune "' + id + '"? The definition is removed; downloaded weights stay.')) return
          try {
            await window.w2gp.libraryFinetuneDelete(id)
            refreshFinetunes()
          } catch (e) {
            if (status) status.textContent = 'Delete failed: ' + ((e && e.message) || e)
          }
        })
      })
      if (status) status.textContent = r.items.length + ' finetunes — Refresh Model List in WanGP (or restart) to apply changes'
    } catch (e) {
      box.innerHTML = '<p class="token-hint">Failed: ' + esc((e && e.message) || e) + '</p>'
    }
  }

  function init() {
    if (!$('libLorasBox')) return
    $('libRefreshBtn')?.addEventListener('click', async () => { await refreshLoras(); await refreshFinetunes(); await refreshWorkspaces() })
    $('libImportBtn')?.addEventListener('click', async () => {
      const status = $('libFinetunesStatus')
      const src = ($('libImportInput')?.value || '').trim().replace(/^"|"$/g, '')
      if (!src) { if (status) status.textContent = 'Paste the full path to a .json file first (Explorer: Shift + right-click → Copy as path)'; return }
      try {
        const r = await window.w2gp.libraryFinetuneImport(src)
        if ($('libImportInput')) $('libImportInput').value = ''
        if (status) status.textContent = 'Imported "' + r.id + '" — Refresh Model List in WanGP to use it'
        refreshFinetunes()
      } catch (e) {
        if (status) status.textContent = 'Import failed: ' + ((e && e.message) || e)
      }
    })
    refreshLoras()
    refreshFinetunes()
    refreshWorkspaces()
    initWsBackupOnce()
  }

  function fmtDate(ts) {
    const t = Number(ts) || 0
    if (!t) return '—'
    const d = new Date(t * 1000)
    return d.toLocaleDateString() + ' ' + d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
  }

  async function refreshWorkspaces() {
    const box = $('wsBox')
    const status = $('wsStatus')
    if (!box) return
    box.innerHTML = '<p class="token-hint">Loading…</p>'
    try {
      const r = await window.w2gp.workspaceList()
      if (!r || r.ok === false) {
        box.innerHTML = '<p class="token-hint">No workspaces folder yet.</p>'
        return
      }
      if (!r.items.length) {
        box.innerHTML = '<p class="token-hint">No workspaces.</p>'
        return
      }
      box.innerHTML = '<table class="args-table">' +
        '<tr><td><strong>Workspace</strong></td><td><strong>Media</strong></td><td><strong>Size</strong></td><td><strong>Last activity</strong></td><td><strong></strong></td></tr>' +
        r.items.map((it) => (
          '<tr><td><code>' + esc(it.name || it.id) + '</code>' +
          (it.error ? '<br><span class="token-hint">' + esc(it.error) + '</span>' : '') +
          (it.missing ? '<br><span class="token-hint">' + esc(it.missing) + ' file(s) moved/missing</span>' : '') +
          (it.truncated ? '<br><span class="token-hint">first 2000 files counted</span>' : '') +
          '</td><td>' + esc(it.files || 0) + ' + ' + esc(it.audio || 0) + ' audio</td>' +
          '<td>' + esc(fmtBytes(it.bytes)) + '</td>' +
          '<td>' + esc(fmtDate(it.last_activity)) + '</td>' +
          '<td><button class="btn btn-ghost small" data-ws-lock="' + esc(it.id) + '" data-locked="' + (it.archive_protected ? '1' : '0') + '" title="Toggle auto-archive protection">' +
          (it.archive_protected ? 'Locked' : 'Lock') + '</button></td></tr>'
        )).join('') + '</table>' +
        (r.archived ? '<p class="token-hint">' + esc(r.archived) + ' archived workspace(s) hidden at startup (media files kept).</p>' : '')
      box.querySelectorAll('[data-ws-lock]').forEach((btn) => {
        btn.addEventListener('click', async () => {
          const id = btn.getAttribute('data-ws-lock')
          const to = btn.getAttribute('data-locked') !== '1'
          try {
            await window.w2gp.workspaceProtect(id, to)
            refreshWorkspaces()
          } catch (e) {
            if (status) status.textContent = 'Lock failed: ' + ((e && e.message) || e)
          }
        })
      })
      if (status) status.textContent = r.items.length + ' workspace(s)'
    } catch (e) {
      box.innerHTML = '<p class="token-hint">Failed: ' + esc((e && e.message) || e) + '</p>'
    }
  }

  function initWsBackupOnce() {
    $('wsBackupBtn')?.addEventListener('click', async () => {
      const status = $('wsStatus')
      if (status) status.textContent = 'Zipping…'
      try {
        const r = await window.w2gp.workspaceBackup()
        if (status) status.textContent = 'Saved ' + ((r && r.zip) || 'backup')
      } catch (e) {
        if (status) status.textContent = 'Backup failed: ' + ((e && e.message) || e)
      }
    })
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init)
  } else {
    init()
  }
})()

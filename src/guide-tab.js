/**
 * guide-tab.js — wiring for the Guide panel's prompt tools and checklists.
 *
 * Pure DOM; model recommendations live in WanGP's own selector, not here.
 * Copy uses navigator.clipboard with a legacy fallback; no backend calls.
 * Loaded after app.js; safe to run standalone (all lookups guarded).
 */
(function () {
  'use strict'

  function $(id) { return document.getElementById(id) }

  // Single source of truth (services/escape.js, loaded before this file).
  const esc = window.escHtml

  function copyText(text, statusEl, okMsg) {
    function done(ok) {
      if (statusEl) statusEl.textContent = ok ? okMsg : 'Copy failed — select and copy manually'
    }
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(text).then(() => done(true), () => done(false))
      return
    }
    try {
      const ta = document.createElement('textarea')
      ta.value = text
      document.body.appendChild(ta)
      ta.select()
      done(document.execCommand('copy'))
      document.body.removeChild(ta)
    } catch (e) { done(false) }
  }

  function init() {
    initPromptTools()
  }

  function initPromptTools() {
    const tsel = $('promptTemplateSelect')
    const templates = (typeof PROMPT_TEMPLATES !== 'undefined') ? PROMPT_TEMPLATES : []
    if (tsel) {
      templates.forEach((t) => {
        const opt = document.createElement('option')
        opt.value = t.id
        opt.textContent = t.label
        tsel.appendChild(opt)
      })
      tsel.addEventListener('change', () => {
        const t = templates.find((x) => x.id === tsel.value)
        const mode = $('promptTemplateMode')
        if (mode) mode.textContent = t ? ('Line mode in WanGP: ' + t.mode) : ''
      })
    }
    $('promptCopyBtn')?.addEventListener('click', () => {
      const t = templates.find((x) => x.id === (tsel && tsel.value))
      const status = $('promptCheckStatus')
      if (!t) { if (status) status.textContent = 'Pick a template first'; return }
      copyText(t.text, status, 'Template copied — paste into WanGP prompt box')
    })
    $('promptCheckBtn')?.addEventListener('click', () => {
      const box = $('promptCheckResults')
      const status = $('promptCheckStatus')
      const text = $('promptCheckInput')?.value || ''
      if (typeof validateWindowCommands !== 'function') return
      const issues = validateWindowCommands(text)
      if (!issues.length) {
        if (status) status.textContent = text.includes('[/') ? 'Clean — no bad commands' : 'No [/...] commands found — nothing to check'
        if (box) box.innerHTML = ''
        return
      }
      if (status) status.textContent = issues.length + ' issue(s)'
      if (box) {
        box.innerHTML = '<table class="args-table">' +
          issues.map((it) => '<tr><td>line ' + it.line + '</td><td>' + esc(it.message) + '</td></tr>').join('') +
          '</table>'
      }
    })
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init)
  } else {
    init()
  }
})()

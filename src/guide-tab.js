/**
 * guide-tab.js — wiring for Manage → Guide (F1 model recommender).
 *
 * Pure DOM: renders goal picker results from GUIDE_GOALS (services/guide-catalog.js).
 * Copy uses navigator.clipboard with a legacy fallback; no backend calls.
 * Loaded after app.js; safe to run standalone (all lookups guarded).
 */
(function () {
  'use strict'

  function $(id) { return document.getElementById(id) }

  function esc(s) {
    return String(s == null ? '' : s)
      .replace(/&/g, '&amp;').replace(/</g, '&lt;')
      .replace(/>/g, '&gt;').replace(/"/g, '&quot;')
  }

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

  function renderGoal(goalId) {
    const box = $('guideResults')
    const status = $('guideStatus')
    if (!box) return
    const goals = (typeof GUIDE_GOALS !== 'undefined') ? GUIDE_GOALS : []
    const goal = goals.find((g) => g.id === goalId)
    if (!goal) {
      box.innerHTML = '<p class="token-hint">Pick a goal above to get model picks.</p>'
      return
    }
    box.innerHTML = goal.picks.map((p, i) => (
      '<div class="settings-section" style="margin-top:8px">' +
        '<h3><code>' + esc(p.model) + '</code></h3>' +
        '<p class="token-hint">' + esc(p.why) + '</p>' +
        '<p class="token-hint">Starters: ' +
          (p.frames ? esc(p.frames) + ' frames · ' : '') +
          esc(p.steps) + ' steps · guidance ' + esc(p.guidance) + '</p>' +
        '<p class="token-hint">' + esc(p.tip) + '</p>' +
        '<div class="token-field" style="margin-top:6px">' +
          '<button class="btn btn-ghost small" data-guide-copy="' + i + '" title="Copy model id to clipboard">Copy model id</button>' +
        '</div>' +
      '</div>'
    )).join('')
    box.querySelectorAll('[data-guide-copy]').forEach((btn) => {
      btn.addEventListener('click', () => {
        const pick = goal.picks[parseInt(btn.getAttribute('data-guide-copy'), 10)]
        if (pick) copyText(pick.model, status, 'Model id copied — paste it in WanGP toolbar search')
      })
    })
    if (status) status.textContent = ''
  }

  function init() {
    const sel = $('guideGoalSelect')
    if (!sel) return
    const goals = (typeof GUIDE_GOALS !== 'undefined') ? GUIDE_GOALS : []
    goals.forEach((g) => {
      const opt = document.createElement('option')
      opt.value = g.id
      opt.textContent = g.label
      sel.appendChild(opt)
    })
    sel.addEventListener('change', () => renderGoal(sel.value))
    renderGoal(sel.value)
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

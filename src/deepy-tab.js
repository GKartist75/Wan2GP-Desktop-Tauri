// deepy-tab.js — Deepy (WanGP assistant) settings panel.
//
// Extracted from app.js as a pure move: $, showToast, getLLMEngines and
// window.w2gp are all globals, so no imports are needed. Loaded AFTER
// app.js in index.html; every call site is a deferred callback.

// Last-rendered "enhancerId|savedQuant" key — options rebuild only when the
// engine context changes so syncApply validation never resets a choice.
let _deepyQuantCtx = "";
// Render the quant selector for a local Qwen engine id (3/4/5/6), or hide it
// (quant untouched) for remote engines / Florence / Disabled. primeBackend
// ("id:quant") swaps the whole block for ONE combined Prime-local list, so
// every possible backend is explicitly visible — no hidden dependency
// between a variant dropdown and a quant dropdown.
function renderDeepyQuant(enhancerId, savedQuant, primeBackend) {
  const wrap = $("deepyQuantWrap");
  const sel = $("deepyQuantSelect");
  const hint = $("deepyQuantHint");
  const beWrap = $("deepyPrimeBackendWrap");
  const beSel = $("deepyPrimeBackendSelect");
  const beHint = $("deepyPrimeBackendHint");
  const stdWrap = $("deepyQuantStdWrap");
  if (!wrap || !sel) return;
  // Prime + local lives in its own block (below the engine pick, above LLM
  // Engines) — the shared Zero/Disabled block stays hidden for it.
  const isPrimeLocal =
    typeof primeBackend === "string" &&
    primeBackend.includes(":") &&
    !!beSel;
  if (isPrimeLocal) {
    wrap.style.display = "none";
    if (beWrap) beWrap.style.display = "block";
    if ([...beSel.options].some((o) => o.value === primeBackend))
      beSel.value = primeBackend;
    if (beHint)
      beHint.textContent =
        primeBackend === "5:gguf_ptq1"
          ? "Bonsai PTQ1 runs Prime on ~10 GB VRAM. Weights download on first WanGP launch. Apply also sets INT8 KV cache."
          : primeBackend === "6:gguf"
            ? "Heretic Q4 runs Prime on ~6.5 GB VRAM — the fit for 8–12 GB GPUs. Weights download on first WanGP launch."
            : primeBackend === "6:gguf_q8"
              ? "Heretic Q8 (~11 GB VRAM) stays closest to full precision. Weights download on first WanGP launch."
              : "Weights download on first WanGP launch. Needs GGUF kernels 1.0.25 (Sync Kernels).";
    return;
  }
  if (beWrap) beWrap.style.display = "none";
  const choices = (enhancerId && DEEPY_QUANT_CHOICES[enhancerId]) || null;
  if (!choices) {
    wrap.style.display = "none";
    _deepyQuantCtx = "";
    return;
  }
  wrap.style.display = "block";
  if (stdWrap) stdWrap.style.display = "block";
  const key = enhancerId + "|" + (savedQuant || "");
  if (key !== _deepyQuantCtx) {
    _deepyQuantCtx = key;
    sel.textContent = "";
    for (const c of choices) {
      const o = document.createElement("option");
      o.value = c.id;
      o.textContent = c.label;
      sel.append(o);
    }
    sel.value = choices.some((c) => c.id === savedQuant)
      ? savedQuant
      : DEEPY_QUANT_DEFAULT[enhancerId];
  }
  sel.dataset.enh = String(enhancerId);
  if (hint)
    updateDeepyQuantHint(sel.value, enhancerId);
  sel.onchange = () => updateDeepyQuantHint(sel.value, enhancerId);
}
// Hint under the quant selector; Bonsai notes its companion defaults.
function updateDeepyQuantHint(value, enhancerId) {
  const hint = $("deepyQuantHint");
  if (!hint) return;
  hint.textContent =
    value === "gguf_ptq1"
      ? "Bonsai PTQ1 runs Prime on ~10 GB VRAM (Sync Kernels for 1.0.25+). Weights download on first WanGP launch. Apply also sets INT8 KV cache."
      : Number(enhancerId) === 6
        ? "Heretic Q4 runs Prime on ~6.5 GB VRAM, Q8 (~11 GB) stays closest to full precision. Weights download on first WanGP launch."
        : Number(enhancerId) === 5
        ? "GGUF Q4 is highest quality; Q3/Q2 trade quality for VRAM. Weights download on first WanGP launch."
        : "Quanto Int8 preserves quality; GGUF Q4 uses less memory when kernels are installed.";
}

async function refreshDeepy() {
  const opts = $("deepyPrimeOnly");
  // Helper: the checked Prime engine radio across the Local/Remote groups.
  const deepyEngineChecked = () =>
    (opts && opts.querySelector("input[name=deepyEngine]:checked")) || {};
  const statusMsg = $("deepyStatusMsg");
  const applyBtn = $("deepyApplyBtn");
  const promptApplyBtn = $("deepyPromptApplyBtn");
  const promptStatusMsg = $("deepyPromptStatusMsg");
  const docsLink = $("deepyDocsLink");
  const primeOnly = $("deepyPrimeOnly");
  const enhancerWrap = $("deepyEnhancerWrap");
  const enhancerOpts = $("deepyEnhancerOptions");
  const enhancerHint = $("deepyEnhancerHint");
  const modeRadios = document.querySelectorAll("input[name=deepyMode]");
  if (!applyBtn) return;

  let status = { available: false };
  let engines = [];
  try {
    const s = await window.w2gp.deepyStatus();
    if (s && s.ok) status = s;
  } catch {}
  try {
    const d = await getLLMEngines();
    engines = (d && d.engines) || [];
  } catch {}

  const ready = (id) => {
    // ponytail: local model lives in Wan2GP — it validates the 9B/27B requirement + downloads on first use, nothing for the launcher to probe
    if (id === "local-qwen38") return true;
    const e = engines.find((x) => x.id === id);
    if (!e) return false;
    if (id === "claude-code") return !!(e.cliOnPath || e.claudeApiKeySet);
    return !!e.cliOnPath;
  };

  const currentProfile = status.currentEngine;
  const profileToUi = {
    opencode: "opencode",
    claude: "claude-code",
    codex: "codex",
    qwen38_27b: "local-qwen38",
    qwen38_9b: "local-qwen38",
  };
  const currentUi = profileToUi[currentProfile] || null;
  const currentMode = status.mode || "disabled";
  const currentEnhancer =
    typeof status.enhancerEnabled === "number" ? status.enhancerEnabled : null;
  // Saved quant backend (upstream prompt_enhancer_quantization) for preselect.
  const savedQuant =
    typeof status.promptEnhancerQuantization === "string"
      ? status.promptEnhancerQuantization
      : null;
  // Sessions section — pre-select from config (backend normalizes; missing
  // keys fall back to upstream defaults). Launcher default for a fresh
  // config is the selectable shared workspace.
  const validSessionMode = ["disabled", "selectable", "dedicated"];
  const curSessionMode = validSessionMode.includes(status.sessionMode)
    ? status.sessionMode
    : "selectable";
  const curResetMode =
    status.sessionResetMode === "reset_session"
      ? "reset_session"
      : "new_session";
  const curGalleryMode =
    status.sessionGalleryMediaMode === "copy" ? "copy" : "link";
  if ($("deepySessionMode")) $("deepySessionMode").value = curSessionMode;
  if ($("deepySessionReset")) $("deepySessionReset").value = curResetMode;
  if ($("deepySessionGallery")) $("deepySessionGallery").value = curGalleryMode;
  // Prompt-enhancement UI — pre-select from config; missing key defaults to
  // Manual Button Only (1).
  if ($("deepyEnhancerMode"))
    $("deepyEnhancerMode").value = status.enhancerMode === 0 ? "0" : "1";
  // Default engine for Prime is OpenCode (universal providers / external, free).
  // Preserve an already-configured engine; otherwise fall back to OpenCode.
  const selectedEngine = currentUi || "opencode";

  // Pre-select the current Deepy mode (Disabled / Zero / Prime).
  modeRadios.forEach((r) => {
    r.checked = r.value === currentMode;
  });
  primeOnly.style.display = currentMode === "prime" ? "block" : "none";

  // Local-model (Prompt Enhancer) selector: shown for Disabled/Zero only.
  // Rendered from the SELECTED mode (not just persisted), so switching modes
  // immediately re-renders the local-model choices. Selection is transient —
  // only persisted when Apply is pressed.
  const renderEnhancer = (mode, preselectId) => {
    const visible = mode === "disabled" || mode === "zero";
    enhancerWrap.style.display = visible ? "block" : "none";
    if (!visible) return;
    const forThisMode = (o) => o.modes.includes(mode);
    // Pre-select: caller-supplied id if valid, else persisted id, else first
    // valid option for this mode.
    const validForMode = DEEPY_PANEL_ENHANCERS.filter(forThisMode);
    const chosen =
      validForMode.find((o) => o.id === preselectId) ||
      validForMode.find((o) => o.id === currentEnhancer) ||
      validForMode[0];
    const sub =
      mode === "zero"
        ? "Deepy Zero runs locally — pick the Qwen model Wan2GP will use."
        : "Prompt enhancement runs without Deepy too — pick any local model.";
    if (enhancerHint) enhancerHint.textContent = sub;
    enhancerOpts.textContent = "";
    for (const o of DEEPY_PANEL_ENHANCERS) {
      const enabled = forThisMode(o);
      const lab = document.createElement("label");
      lab.className = enabled
        ? "deepy-enhancer-opt"
        : "deepy-enhancer-opt deepy-enhancer-opt-disabled";
      const radio = document.createElement("input");
      radio.type = "radio";
      radio.name = "deepyEnhancer";
      radio.value = o.id;
      if (o.id === chosen.id) radio.checked = true;
      if (!enabled) radio.disabled = true;
      const name = document.createElement("span");
      name.className = "deepy-enhancer-label";
      name.textContent = o.label;
      lab.append(radio, "\n  ", name);
      if (!enabled) {
        const note = document.createElement("span");
        note.className = "deepy-enhancer-note";
        note.textContent =
          " — only for " + (o.modes[0] === "zero" ? "Deepy Zero" : "Disabled");
        lab.append(note);
      }
      lab.append("\n");
      enhancerOpts.append(lab);
    }
  };
  renderEnhancer(currentMode, currentEnhancer);

  const engLocal = $("deepyEngineLocal");
  const engRemote = $("deepyEngineRemote");
  if (engLocal) engLocal.textContent = "";
  if (engRemote) engRemote.textContent = "";
  for (const en of DEEPY_PANEL_ENGINES) {
    const isReady = ready(en.id);
    const lab = document.createElement("label");
    lab.className = "deepy-engine-opt";
    const radio = document.createElement("input");
    radio.type = "radio";
    radio.name = "deepyEngine";
    radio.value = en.id;
    if (en.id === selectedEngine) radio.checked = true;
    const dot = document.createElement("span");
    dot.className = isReady ? "dot-ok" : "dot-bad";
    dot.textContent = isReady ? "●" : "○";
    const name = document.createElement("span");
    name.className = "deepy-engine-label";
    name.textContent = en.label;
    const cost = document.createElement("span");
    cost.className = "deepy-engine-cost";
    cost.textContent = en.paid ? "paid" : "free";
    lab.append(
      radio,
      "\n          ",
      dot,
      "\n          ",
      name,
      "\n          ",
      cost,
      "\n        ",
    );
    // Local Qwen3.8 groups with its model+quantization picker; the remote
    // CLIs group with the LLM Engines setup card below them.
    const host =
      en.id === "local-qwen38"
        ? engLocal || opts
        : engRemote || opts;
    host.append(lab);
  }

  statusMsg.textContent = "";
  if (status.available) {
    const label =
      {
        disabled: "Disabled",
        zero: "Deepy Zero (local model)",
        prime: "Deepy Prime",
      }[currentMode] || currentMode;
    const st = document.createElement("strong");
    st.textContent = label;
    statusMsg.append(
      "Currently: ",
      st,
      currentMode === "prime" && currentProfile
        ? " — engine: " + currentProfile
        : "",
    );
  } else {
    const s = document.createElement("span");
    s.style.color = "#FBBF24";
    s.textContent =
      status.reason || "Wan2GP config not found — install Wan2GP first.";
    statusMsg.append(s);
  }

  const syncApply = () => {
    const mode =
      (document.querySelector("input[name=deepyMode]:checked") || {}).value ||
      "disabled";
    primeOnly.style.display = mode === "prime" ? "block" : "none";
    enhancerWrap.style.display =
      mode === "disabled" || mode === "zero" ? "block" : "none";
    let ok = true;
    let title = "";
    if (mode === "prime") {
      const eng = (opts.querySelector("input[name=deepyEngine]:checked") || {})
        .value;
      if (!eng) {
        ok = false;
        title = "Pick an engine for Deepy Prime";
      } else if (!ready(eng)) {
        ok = false;
        title = "Install / enable this engine first (see LLM Engines above)";
      }
    } else if (mode === "disabled" || mode === "zero") {
      const enh = (
        enhancerOpts.querySelector("input[name=deepyEnhancer]:checked") || {}
      ).value;
      // ponytail: Tauri — don't block Apply if enhancer not yet rendered; Rust defaults to 3 (Qwen 4B) for Zero
      if (!enh && !window.__TAURI__) {
        ok = false;
        title = "Pick a local model (Prompt Enhancer)";
      }
    }
    // Qwen quant selector follows the local engine: Disabled/Zero + Qwen 3/4/5/6,
    // or Prime + local Qwen3.8 via ONE combined model+quantization list (all
    // six backends explicitly visible). Hidden otherwise (quant untouched).
    const savedEnh = Number.isInteger(currentEnhancer) ? currentEnhancer : null;
    const isPrimeLocalPick =
      mode === "prime" && deepyEngineChecked().value === "local-qwen38";
    const backendPick = isPrimeLocalPick
      ? primeBackendFor(savedQuant, savedEnh)
      : null;
    // Variant id anchoring the combined list (5/6) so the shared choice
    // tables stay valid even though the per-variant lists are bypassed.
    const beVariant = backendPick ? parseInt(backendPick.split(":")[0], 10) : NaN;
    const qEnhRaw =
      mode === "zero" || mode === "disabled"
        ? parseInt(
            (enhancerOpts.querySelector("input[name=deepyEnhancer]:checked") || {})
              .value,
            10,
          )
        : isPrimeLocalPick
          ? beVariant
          : NaN;
    renderDeepyQuant(
      [3, 4, 5, 6].includes(qEnhRaw) ? qEnhRaw : null,
      savedQuant,
      backendPick,
    );
    // LLM Engines setup belongs to the remote group — hidden while a local
    // engine is picked so each group only shows its own follow-ups.
    const llmCard = $("llmEnginesCard");
    if (llmCard)
      llmCard.style.display =
        mode === "prime" && !isPrimeLocalPick ? "" : "none";
    // Both Apply buttons (Deepy card + Prompt enhancement card) share one
    // coherent config write, so they enable/disable together.
    for (const b of [applyBtn, promptApplyBtn]) {
      if (!b) continue;
      b.disabled = !ok;
      b.title = title || "Set Deepy to " + mode;
    }
  };
  modeRadios.forEach((r) =>
    r.addEventListener("change", () => {
      // Switching the mode immediately re-renders the local-model selector for
      // the newly-selected mode (transient — not persisted until Apply).
      const m =
        (document.querySelector("input[name=deepyMode]:checked") || {}).value ||
        "disabled";
      renderEnhancer(m);
      syncApply();
    }),
  );
  opts
    .querySelectorAll("input[name=deepyEngine]")
    .forEach((r) => r.addEventListener("change", syncApply));
  enhancerOpts
    .querySelectorAll("input[name=deepyEnhancer]")
    .forEach((r) => r.addEventListener("change", syncApply));
  // Dropdowns have no validation of their own, but changing one is a pending
  // change — enable both Apply buttons.
  for (const id of [
    "deepySessionMode",
    "deepySessionReset",
    "deepySessionGallery",
    "deepyEnhancerMode",
    "deepyQuantSelect",
  ]) {
    $(id)?.addEventListener("change", syncApply);
  }
  // Combined Prime-local backend picker has its own handler: the pick must
  // land in _primeBackend BEFORE syncApply re-renders, or the generic path
  // would paint the saved pick back over it.
  $("deepyPrimeBackendSelect")?.addEventListener("change", (e) => {
    const v = String((e.target || {}).value || "");
    _primeBackend = /^(5:(gguf|gguf_q3|gguf_q2|gguf_ptq1)|6:(gguf|gguf_q8))$/.test(v)
      ? v
      : null;
    syncApply();
  });
  syncApply();

  // One shared write for both cards: reads the whole panel state and applies
  // it coherently. Each Apply button reports into its own card's message line.
  const applyDeepy = async (btn, msgEl) => {
    const mode =
      (document.querySelector("input[name=deepyMode]:checked") || {}).value ||
      "disabled";
    const eng = deepyEngineChecked().value;
    const enh = (
      enhancerOpts.querySelector("input[name=deepyEnhancer]:checked") || {}
    ).value;
    btn.disabled = true;
    btn.textContent = "applying...";
    const sessions = {
      multi_session: ($("deepySessionMode") || {}).value || "selectable",
      reset_mode: ($("deepySessionReset") || {}).value || "new_session",
      gallery_media_mode: ($("deepySessionGallery") || {}).value || "link",
    };
    // Quant + variant: Prime + local Qwen3.8 reads the single combined
    // backend picker ("id:quant"); Zero/Disabled read the local-model radios
    // + quant selector. Hidden controls mean preserve whatever WanGP has.
    const quantWrap = $("deepyQuantWrap");
    const quantSel = $("deepyQuantSelect");
    const beSel = $("deepyPrimeBackendSelect");
    const beWrap = $("deepyPrimeBackendWrap");
    const primeBackend =
      mode === "prime" &&
      eng === "local-qwen38" &&
      beWrap &&
      beWrap.style.display !== "none" &&
      beSel &&
      /^(5:(gguf|gguf_q3|gguf_q2|gguf_ptq1)|6:(gguf|gguf_q8))$/.test(beSel.value || "")
        ? String(beSel.value)
        : null;
    const quant = primeBackend
      ? primeBackend.split(":")[1]
      : quantWrap && quantWrap.style.display !== "none" && quantSel && quantSel.value
        ? quantSel.value
        : null;
    // Prime + local writes the picked variant (27B id 5 / 9B id 6), never a
    // stale Zero-mode radio hiding in the hidden enhancer block.
    const enhForWrite =
      primeBackend != null
        ? parseInt(primeBackend.split(":")[0], 10)
        : enh
          ? parseInt(enh, 10)
          : null;
    // Prompt-enhancement UI: "1" = Manual Button Only, "0" = Manual Button +
    // Automatic on Generation (the button stays in both modes).
    const enhancerMode = ($("deepyEnhancerMode") || {}).value || "1";
    const r = await window.w2gp.deepySet(
      mode,
      eng,
      enhForWrite,
      sessions,
      quant,
      enhancerMode,
    );
    btn.textContent = "Apply";
    // No pending changes left — park both buttons until the next edit.
    for (const b of [applyBtn, promptApplyBtn]) if (b) b.disabled = true;
    if (r && r.ok) {
      const s = document.createElement("span");
      s.style.color = "#4ADE80";
      s.textContent = "✓ " + (r.message || "Deepy updated");
      msgEl.textContent = "";
      msgEl.append(s);
      showToast("✓ " + (r.message || "Deepy updated"));
      appendLog(
        "[Deepy] ✓ " +
          (r.message || "Deepy mode: " + mode) +
          " (sessions: " +
          sessions.multi_session +
          "/" +
          sessions.reset_mode +
          "/" +
          sessions.gallery_media_mode +
          (quant ? "; quant: " + quant : "") +
          "; prompt: " +
          (enhancerMode === "0" ? "automatic" : "button") +
          ")",
      );
    } else {
      const s = document.createElement("span");
      s.style.color = "#F87171";
      s.textContent = "✗ " + ((r && r.error) || "update failed");
      msgEl.textContent = "";
      msgEl.append(s);
      showToast("✗ " + (r && r.error ? r.error : "update failed"));
      appendLog(
        "[Deepy] ✗ update failed: " + ((r && r.error) || "update failed"),
      );
      for (const b of [applyBtn, promptApplyBtn]) if (b) b.disabled = false;
    }
  };
  applyBtn.onclick = () => applyDeepy(applyBtn, statusMsg);
  if (promptApplyBtn)
    promptApplyBtn.onclick = () =>
      applyDeepy(promptApplyBtn, promptStatusMsg || statusMsg);
  if (docsLink)
    docsLink.onclick = async (ev) => {
      ev.preventDefault();
      await window.w2gp.openExternal(
        "https://github.com/deepbeepmeep/Wan2GP/blob/main/docs/DEEPY.md",
      );
    };
}

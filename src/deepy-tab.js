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
          : primeBackend === "5:gguf"
            ? "GGUF Q4 is the highest-quality 27B and usually wants a 24 GB GPU. Weights download on first WanGP launch. Needs GGUF kernels 1.0.29 (Sync Kernels)."
            : primeBackend === "5:gguf_q3"
              ? "IQ3_S is the balanced 27B choice and usually fits a 16 GB GPU. Weights download on first WanGP launch. Needs GGUF kernels 1.0.29 (Sync Kernels)."
              : primeBackend === "5:gguf_q2"
                ? "Q2 is the leanest 27B — 16 GB is enough and leaves room to grow the context window, at the largest quality loss. Weights download on first WanGP launch. Needs GGUF kernels 1.0.29 (Sync Kernels)."
                : primeBackend === "6:gguf"
                  ? "Heretic Q4 runs Prime on ~6.5 GB VRAM — the fit for 8–12 GB GPUs. Weights download on first WanGP launch."
                  : primeBackend === "6:gguf_q8"
                    ? "Heretic Q8 (~11 GB VRAM) stays closest to full precision. Weights download on first WanGP launch."
                    : "Weights download on first WanGP launch. Needs GGUF kernels 1.0.29 (Sync Kernels).";
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
      ? "Bonsai PTQ1 runs Prime on ~10 GB VRAM (Sync Kernels for 1.0.29+). Weights download on first WanGP launch. Apply also sets INT8 KV cache."
      : Number(enhancerId) === 6
        ? "Heretic Q4 runs Prime on ~6.5 GB VRAM, Q8 (~11 GB) stays closest to full precision. Weights download on first WanGP launch."
        : Number(enhancerId) === 5
        ? "GGUF Q4 is highest quality; Q3/Q2 trade quality for VRAM. Weights download on first WanGP launch."
        : "Quanto Int8 preserves quality; GGUF Q4 uses less memory when kernels are installed.";
}

// Current speculative value for this session: the row is re-rendered on
// every syncApply from the saved key, so each successful write must land
// here first — a later field change would otherwise repaint the value from
// tab-open state over the user's mid-session pick.
let _specSaved = null;

// Draft-token ceiling for a method on a backend pick. Stored values keep
// upstream's 1..7 range, but the Bonsai PTQ1 drafter only takes 5
// (block_draft_spec(bonsai=True)["drafts"]), so the row must not offer more.
function specMaxTokens(method, backendPick) {
  const max = SPEC_MAX_TOKENS[method] || 0;
  if (!max) return 0;
  const [id, quant] = String(backendPick || "").split(":");
  if (method === "dflash2" && Number(id) === 5 && quant === "gguf_ptq1")
    return Math.min(max, 5);
  return max;
}

// Speculative Decoding row — Prime-local only (`#deepySpecWrap` lives inside
// the backend block in index.html). Rebuilds options per backend pick like
// the quant selector rebuilds per engine context; preselects the saved key
// normalized upstream-style. A change writes the canonical dict immediately
// (the KV cache row's write pattern), so nothing waits on Apply.
function renderDeepySpec(backendPick, savedSpec) {
  const wrap = $("deepySpecWrap");
  const sel = $("deepySpecSelect");
  const tok = $("deepySpecTokensSelect");
  if (!wrap || !sel) return;
  if (!backendPick) {
    wrap.style.display = "none";
    return;
  }
  wrap.style.display = "block";
  const methods = specMethods(backendPick);
  const saved = specSplit(savedSpec);
  // A method the backend no longer qualifies for falls back to Auto,
  // exactly like upstream's ui_state.
  const method = methods.includes(saved.method) ? saved.method : "auto";
  sel.textContent = "";
  for (const m of methods) {
    const o = document.createElement("option");
    o.value = m;
    o.textContent =
      SPEC_METHOD_LABELS[m] + " [+" + SPEC_VRAM_COSTS[m] + " VRAM]";
    sel.append(o);
  }
  sel.value = method;
  const max = specMaxTokens(method, backendPick);
  if (tok) {
    tok.textContent = "";
    if (max) {
      tok.style.display = "";
      for (let n = 1; n <= max; n++) {
        const o = document.createElement("option");
        o.value = String(n);
        o.textContent = n === 1 ? "1 draft token" : n + " draft tokens";
        tok.append(o);
      }
      tok.value = String(
        Math.min(max, Math.max(1, saved.tokens || SPEC_DEFAULT_TOKENS[method])),
      );
    } else {
      tok.style.display = "none";
    }
  }
  updateDeepySpecHint();
}
function updateDeepySpecHint() {
  const sel = $("deepySpecSelect");
  const hint = $("deepySpecHint");
  if (!sel || !hint) return;
  hint.textContent =
    sel.value === "auto"
      ? "Auto turns MTP on only when it pays: the model still fits in VRAM beside the drafts, or part of the model already streams from RAM (MTP then predicts several tokens per pass). It stays off when MTP would push a fitting model partly out, and on GPUs with 8 GB or less. For non-GGUF Qwen3.5 9B it needs 12 GB+. The console reports the choice."
      : sel.value === "disabled"
        ? "No draft model — plain autoregressive decoding (0 GiB extra)."
        : sel.value === "mtp"
          ? "MTP drafts tokens ahead; start at 2 (the upstream default). About +0.5-1 GiB VRAM near 32k context."
          : "Needs the 27B on a GGUF quantization and Triton with FlashAttention 2 in the env (Sync Kernels). About +4-5 GiB VRAM near 32k context.";
}
async function deepySpecWrite() {
  const sel = $("deepySpecSelect");
  const tok = $("deepySpecTokensSelect");
  if (!sel) return;
  updateDeepySpecHint();
  const value = specJoin(
    sel.value,
    tok && tok.style.display !== "none" && tok.value ? tok.value : null,
  );
  try {
    await window.w2gp.writeWgpConfig({
      prompt_enhancer_speculative_decoding: value,
    });
    _specSaved = value;
    showToast(
      "✓ Speculative decoding: " +
        SPEC_METHOD_LABELS[value.method] +
        (value.tokens != null ? " (" + value.tokens + " draft tokens)" : ""),
    );
  } catch (err) {
    showToast("✗ " + (err?.message || err));
  }
}

// Upstream `deepy_kv_cache_quantization` (shared/deepy/config.py) is "" for
// BF16, auto, int8, or a body preset; the mixed bodies carry an exact-overlay
// preset joined as "body:exact", defaulting to s1r1024
// (shared/llm_engines/lmgp/layers/kv_formats.py BODY_PRESETS / EXACT_PRESETS).
// Auto resolves to int8 only while GGUF kernels >= 1.0.13 are installed.
const KV_MIXED = ["int8_q4", "q6", "q6_q4", "q4_1", "q4"];
function kvSplit(raw) {
  const [body, exact] = String(raw == null ? "auto" : raw)
    .trim()
    .toLowerCase()
    .split(":");
  return { body, exact: exact || "s1r1024" };
}
function kvJoin(body, exact) {
  if (!KV_MIXED.includes(body)) return body;
  return body + ":" + (exact || "s1r1024");
}

// Upstream `deepy_context_tokens` / `deepy_compaction_type` /
// `deepy_compaction_thinking` (shared/deepy/config.py). The context window is
// clamped to [20480, 256000]; Summarize needs >= 32K and Summarize with
// Thinking >= 48K, so the panel raises the context when that pair is picked.
// `summarize_thinking` is the legacy type encoding, still read by upstream.
const DEEPY_CONTEXT_MIN = 20480;
const DEEPY_CONTEXT_MAX = 256000;
const DEEPY_COMPACTION_MIN = {
  discard: DEEPY_CONTEXT_MIN,
  summarize: 32000,
  thinking: 48000,
};
function deepyCompactionChoice(type, thinking) {
  const t = String(type == null ? "" : type).trim().toLowerCase();
  if (t === "summarize_thinking" || (t === "summarize" && thinking === true))
    return "thinking";
  return t === "summarize" ? "summarize" : "discard";
}
function deepyClampContext(value, choice) {
  const min = DEEPY_COMPACTION_MIN[choice] || DEEPY_CONTEXT_MIN;
  const n = Math.trunc(Number(value));
  const v = Number.isFinite(n) ? n : DEEPY_CONTEXT_MIN;
  return Math.min(DEEPY_CONTEXT_MAX, Math.max(min, v));
}

// Upstream `prompt_enhancer_speculative_decoding`
// (shared/prompt_enhancer/config.py): Deepy's own LLM drafts tokens ahead.
// The canonical stored value is a {"method", "tokens"} dict; plain ints are
// the legacy encoding upstream still reads (0 = disabled, 1 = MTP at 2 draft
// tokens, 2 = Auto, 3/4 = MTP at 3/4). Auto turns MTP on only while it
// speeds Deepy up — the model, or the part of it that would stream from RAM
// anyway, still fits in VRAM alongside the drafts (MTP_AUTO_SLACK 512 MB).
const SPEC_METHOD_LABELS = {
  auto: "Auto",
  disabled: "Disabled",
  mtp: "MTP",
  dspark: "DSpark",
  dflash2: "DFlash2",
};
const SPEC_MAX_TOKENS = { mtp: 8, dspark: 7, dflash2: 7 };
const SPEC_DEFAULT_TOKENS = { mtp: 2, dspark: 7, dflash2: 5 };
// Approximate extra residency next to ~32k context (upstream UI costs).
const SPEC_VRAM_COSTS = {
  auto: "0 to ~1 GiB",
  disabled: "0 GiB",
  mtp: "~0.5-1 GiB",
  dspark: "~4-5 GiB",
  dflash2: "~4-5 GiB",
};

// Normalize any stored shape to {method, tokens} like upstream's normalize +
// split: dicts pass through with tokens clamped to the method's 1..max (a
// non-throwing stand-in for upstream's ValueError), auto/disabled drop their
// tokens, and the legacy int/string encodings map over. Absent = Auto.
function specSplit(raw) {
  if (raw && typeof raw === "object" && !Array.isArray(raw)) {
    const method = String(raw.method || "auto").toLowerCase();
    const max = SPEC_MAX_TOKENS[method];
    let tokens = raw.tokens == null ? null : Number(raw.tokens);
    if (max) {
      if (tokens == null || !Number.isInteger(tokens) || tokens < 1 || tokens > max)
        tokens = SPEC_DEFAULT_TOKENS[method];
    } else {
      tokens = null;
    }
    return { method, tokens };
  }
  if (raw == null) return { method: "auto", tokens: null };
  if (typeof raw === "string") {
    // Upstream's string branch verbatim: block methods pass through, digit
    // strings only keep 3/4 as their own draft count, the truthy words count
    // as 1, and anything else (0/false/no/off/other digits/garbage) is
    // disabled.
    const text = raw.trim().toLowerCase();
    if (text === "auto" || text === "2") return { method: "auto", tokens: null };
    if (text === "dspark" || text === "dflash2")
      return { method: text, tokens: SPEC_DEFAULT_TOKENS[text] };
    if (text === "3" || text === "4") return { method: "mtp", tokens: Number(text) };
    if (text === "1" || text === "true" || text === "yes" || text === "on")
      return { method: "mtp", tokens: 2 };
    return { method: "disabled", tokens: null };
  }
  // Upstream's numeric branch: Auto and the draft counts 2-4 keep their
  // mode, 0 is disabled, and any other truthy number means 1 -> MTP at 2.
  const n = Math.trunc(Number(raw));
  if (n === 2) return { method: "auto", tokens: null };
  if (n === 3 || n === 4) return { method: "mtp", tokens: n };
  return raw ? { method: "mtp", tokens: 2 } : { method: "disabled", tokens: null };
}

// Canonical write value (upstream speculative_decoding_config): unknown
// methods fall back to Auto, token counts outside 1..max to the method
// default, auto/disabled never carry tokens.
function specJoin(method, tokens) {
  const m = String(method || "auto").toLowerCase();
  if (!SPEC_METHOD_LABELS[m]) return { method: "auto", tokens: null };
  const max = SPEC_MAX_TOKENS[m];
  if (!max) return { method: m, tokens: null };
  let t = Number(tokens);
  if (!Number.isInteger(t) || t < 1 || t > max) t = SPEC_DEFAULT_TOKENS[m];
  return { method: m, tokens: t };
}

// Method choices for a Prime-local backend pick ("5:gguf_ptq1"). Upstream
// speculative_decoding_ui_state gating: MTP needs enhancer id 4/5/6; the
// block-draft methods need the 27B (id 5) on a GGUF quantization — upstream
// additionally requires Triton + FlashAttention 2 at runtime, which the hint
// says instead of the launcher probing the env.
function specMethods(backendPick) {
  const methods = ["auto", "disabled"];
  const [id, quant] = String(backendPick).split(":");
  const n = Number(id);
  if ([4, 5, 6].includes(n)) methods.push("mtp");
  if (
    n === 5 &&
    ["gguf", "gguf_q3", "gguf_q2", "gguf_ptq1"].includes(String(quant))
  ) {
    methods.push("dspark", "dflash2");
  }
  return methods;
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
  // Saved speculative decoding (upstream prompt_enhancer_speculative_decoding)
  // — raw, dict or legacy int; the renderer normalizes like upstream.
  _specSaved = status.promptEnhancerSpeculativeDecoding ?? null;
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
    renderDeepyPerf(
      mode,
      mode === "prime" && deepyEngineChecked().value === "local-qwen38",
    );
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
    renderDeepySpec(backendPick, _specSaved);
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

  // Speculative-decoding rows write immediately on change (the KV cache
  // row's pattern); bind once across refreshes like the KV buttons.
  const specBind = $("deepySpecSelect");
  if (specBind && !specBind.dataset.specBound) {
    specBind.dataset.specBound = "1";
    specBind.addEventListener("change", deepySpecWrite);
    $("deepySpecTokensSelect")?.addEventListener("change", deepySpecWrite);
  }

  // Performance & context + Prime model preferences. Both write immediately
  // (one atomic config write) and are bound once across refreshes. Context and
  // compaction are clamped as a pair so WanGP never loads an invalid combo.
  function renderDeepyPerf(mode, primeLocal) {
    const wrap = $("deepyPerfWrap");
    if (wrap) wrap.style.display = mode === "zero" || (mode === "prime" && primeLocal) ? "block" : "none";
    const prefs = $("deepyPrimePrefsWrap");
    if (prefs) prefs.style.display = mode === "prime" ? "block" : "none";
    const speed = $("deepyModelSpeedSelect");
    const size = $("deepyModelSizeSelect");
    if (speed)
      speed.value = ["fast", "standard", "any"].includes(status.modelSpeed)
        ? status.modelSpeed
        : "fast";
    if (size)
      size.value = ["smaller", "larger", "any"].includes(status.modelSize)
        ? status.modelSize
        : "smaller";
    if (!wrap || wrap.style.display === "none") return;
    const vram = $("deepyVramModeSelect");
    const ctx = $("deepyContextInput");
    const comp = $("deepyCompactionSelect");
    const rep = $("deepyRepPenaltySelect");
    const choice = deepyCompactionChoice(
      status.compactionType,
      status.compactionThinking,
    );
    if (vram)
      vram.value = ["unload", "unload_on_request", "always_loaded"].includes(
        status.vramMode,
      )
        ? status.vramMode
        : "unload";
    if (comp) {
      for (const o of comp.options)
        if (o.value === "discard") o.disabled = mode === "prime";
      comp.value = mode === "prime" && choice === "discard" ? "summarize" : choice;
    }
    if (ctx) ctx.value = deepyClampContext(status.contextTokens, choice);
    if (rep) rep.value = status.repetitionPenalty === false ? "false" : "true";
    const hint = $("deepyPerfHint");
    if (hint)
      hint.textContent =
        mode === "prime"
          ? "Local Prime requires Summarize compaction and at least 32K context — the launcher raises both when needed. Always loaded is fastest; Unload ASAP frees VRAM for media models."
          : "Unload ASAP frees VRAM for media models; Always loaded is fastest when you switch to Deepy often. Use 20K context for short chats and Summarize (32K+) for long sessions.";
  }

  const perfVram = $("deepyVramModeSelect");
  if (perfVram && !perfVram.dataset.perfBound) {
    perfVram.dataset.perfBound = "1";
    const perfWrite = async () => {
      const comp = $("deepyCompactionSelect");
      const choice = comp ? comp.value : "discard";
      const ctxEl = $("deepyContextInput");
      const ctx = deepyClampContext(ctxEl && ctxEl.value, choice);
      if (ctxEl && String(ctxEl.value) !== String(ctx))
        ctxEl.value = ctx;
      const rep = $("deepyRepPenaltySelect");
      const patch = {
        deepy_vram_mode: perfVram.value,
        deepy_context_tokens: ctx,
        deepy_compaction_type: choice === "discard" ? "discard" : "summarize",
        deepy_compaction_thinking: choice === "thinking",
        deepy_repetition_penalty: !rep || rep.value !== "false",
      };
      try {
        await window.w2gp.writeWgpConfig(patch);
        Object.assign(status, {
          vramMode: patch.deepy_vram_mode,
          contextTokens: ctx,
          compactionType: patch.deepy_compaction_type,
          compactionThinking: patch.deepy_compaction_thinking,
          repetitionPenalty: patch.deepy_repetition_penalty,
        });
        showToast(
          "✓ Deepy: " +
            (patch.deepy_vram_mode === "always_loaded"
              ? "always loaded"
              : patch.deepy_vram_mode === "unload_on_request"
                ? "unload on request"
                : "unload ASAP") +
            ", " +
            ctx +
            " context tokens, " +
            (choice === "thinking"
              ? "Summarize with Thinking"
              : choice === "summarize"
                ? "Summarize"
                : "Discard oldest"),
        );
      } catch (err) {
        showToast("✗ " + (err?.message || err));
      }
    };
    for (const el of [
      perfVram,
      $("deepyContextInput"),
      $("deepyCompactionSelect"),
      $("deepyRepPenaltySelect"),
    ])
      if (el) el.addEventListener("change", perfWrite);
  }

  const prefSpeed = $("deepyModelSpeedSelect");
  const prefSize = $("deepyModelSizeSelect");
  if (prefSpeed && !prefSpeed.dataset.prefBound) {
    prefSpeed.dataset.prefBound = "1";
    const prefWrite = async () => {
      const patch = {
        deepy_model_speed: prefSpeed.value,
        deepy_model_size: prefSize ? prefSize.value : "smaller",
      };
      try {
        await window.w2gp.writeWgpConfig(patch);
        Object.assign(status, {
          modelSpeed: patch.deepy_model_speed,
          modelSize: patch.deepy_model_size,
        });
        showToast("✓ Deepy model preferences saved");
      } catch (err) {
        showToast("✗ " + (err?.message || err));
      }
    };
    prefSpeed.addEventListener("change", prefWrite);
    if (prefSize) prefSize.addEventListener("change", prefWrite);
  }

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

// Deepy KV Cache Quantization — written directly to wgp_config.json. Upstream
// `deepy_kv_cache_quantization` (shared/deepy/config.py) is a body preset,
// plus an exact-overlay preset for the mixed layouts joined as "body:exact"
// (shared/llm_engines/lmgp/layers/kv_formats.py BODY_PRESETS / EXACT_PRESETS).
const kvSel = $("deepyKvCacheSelect");
const kvExact = $("deepyKvCacheExactSelect");
const kvSync = () => {
  if (!kvExact) return;
  kvExact.style.display =
    kvSel && KV_MIXED.includes(kvSel.value) ? "" : "none";
};
const kvPopulate = (raw) => {
  if (!kvSel) return;
  const { body, exact } = kvSplit(raw);
  kvSel.value = [...kvSel.options].some((o) => o.value === body)
    ? body
    : "auto";
  if (kvExact && [...kvExact.options].some((o) => o.value === exact))
    kvExact.value = exact;
  kvSync();
};
kvPopulate(status.kvCacheQuantization);
if (kvSel && !kvSel.dataset.kvBound) {
  kvSel.dataset.kvBound = "1";
  const kvWrite = async () => {
    const v = kvJoin(kvSel.value, kvExact && kvExact.value);
    try {
      await window.w2gp.writeWgpConfig({ deepy_kv_cache_quantization: v });
      showToast("✓ Deepy KV cache: " + (v === "" ? "BF16 (disabled)" : v));
    } catch (err) {
      showToast("✗ " + (err?.message || err));
    }
  };
  kvSel.addEventListener("change", () => {
    kvSync();
    kvWrite();
  });
  if (kvExact) kvExact.addEventListener("change", kvWrite);
}
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

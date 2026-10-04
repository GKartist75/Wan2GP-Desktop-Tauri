// pipinstall-tab.js — Quick pip install (guided pip spec entry).
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

// ── Quick pip install ──
// Accept either a bare spec (claude-agent-sdk==0.1.66) or a full command
// (pip install claude-agent-sdk==0.1.66) pasted by the user — strip any leading
// pip invocation so both the preview and the real install behave identically.
// (Renderer is a plain browser script — no require — so this is inlined; the
// Node-side mirror lives in services/normalize-pip-spec.js for unit tests.)
function normalizePipSpec(raw) {
  let s = (raw || "").trim();
  const m = s.match(/^(?:py(?:thon)?\s+-m\s+)?pip3?\s+install\s+/i);
  if (m) s = s.slice(m[0].length).trim();
  // Strip pip flags (`pip install foo --upgrade` → `foo`). UX only — the
  // backend re-validates. Must match services/normalize-pip-spec.js.
  s = s
    .split(/\s+/)
    .filter((t) => !t.startsWith("-"))
    .join(" ");
  return s;
}
$("pipInstallBtn").addEventListener("click", async () => {
  const input = $("pipInput");
  // Quick-box trap: users paste upstream's `pip install -r requirements.txt`
  // here. normalizePipSpec strips `-r`, leaving the literal file name as a
  // package spec. Catch file/flag input BEFORE normalizing and redirect to
  // the restore button instead of sending `pip install requirements.txt`.
  const rawPip = input?.value || "";
  const lowPip = rawPip.toLowerCase();
  const pipTokens = lowPip.split(/\s+/).filter(Boolean);
  if (
    /(^|\s)pip\s+install\s+-r/i.test(rawPip) ||
    pipTokens.some((t) => t === "-r" || t === "-e" || t === "-c") ||
    pipTokens.some((t) => t.startsWith("-")) ||
    lowPip.includes("requirements.txt") ||
    /--[a-z0-9]/i.test(rawPip)
  ) {
    showToast(
      "That box installs single packages only (e.g. mmgp==3.8.0). For requirements.txt use the restore button.",
    );
    if (input) input.disabled = false;
    $("pipInstallBtn").disabled = false;
    $("pipInstallBtn").textContent = "pip install";
    return;
  }
  const pkg = normalizePipSpec(input?.value);
  if (!pkg) return;
  input.disabled = true;
  $("pipInstallBtn").disabled = true;
  $("pipInstallBtn").textContent = "installing...";
  const r = await window.w2gp.installPackage(pkg);
  input.disabled = false;
  $("pipInstallBtn").disabled = false;
  $("pipInstallBtn").textContent = "pip install";
  if (r && r.success) {
    input.value = "";
    showToast("✓ " + pkg + " installed");
    refreshDashboard();
  } else {
    showToast("✗ " + (r && r.error ? r.error : "install failed"));
  }
});
$("pipInput").addEventListener("keydown", (e) => {
  if (e.key === "Enter") $("pipInstallBtn").click();
});

// Live, copyable preview of the exact command the Advanced box will run.
// Mirrors the launcher's guard: a valid spec shows `pip install <spec>`; an
// invalid one shows the reason it would be blocked (no misleading command).
function updatePipCmdPreview() {
  const input = $("pipInput");
  const preview = $("pipCmdPreview");
  const text = $("pipCmdText");
  if (!input || !preview || !text) return;
  const spec = normalizePipSpec(input.value);
  if (!spec) {
    preview.style.display = "none";
    return;
  }
  // Single source of truth: the same validator the backend enforces
  // (services/pip-spec.js, exposed as window.PipSpec by the script tag in
  // index.html). No inline copy — the old one wrongly blocked `<>` (valid
  // PEP 440 operators), so `foo>=1.0` previewed as blocked but installed fine.
  const check = window.PipSpec
    ? window.PipSpec.assertSafePipSpec(spec)
    : { ok: false, reason: "validator missing" };
  if (check.ok) {
    preview.style.display = "flex";
    preview.classList.remove("pip-cmd-bad");
    text.textContent = "pip install " + spec + "   (runs in the active env)";
  } else {
    preview.style.display = "flex";
    preview.classList.add("pip-cmd-bad");
    text.textContent = "✗ Blocked: " + (check.reason || "invalid spec");
  }
}
$("pipInput").addEventListener("input", updatePipCmdPreview);
$("pipCmdCopy")?.addEventListener("click", async () => {
  const t = $("pipCmdText")?.textContent || "";
  if (!t.startsWith("pip install")) return;
  try {
    await navigator.clipboard.writeText(t.split("   (")[0]);
    $("pipCmdCopy").textContent = "copied!";
    setTimeout(() => {
      $("pipCmdCopy").textContent = "copy";
    }, 1200);
  } catch {}
});
// Clear the preview after a successful install so it doesn't linger.
const _pipInstallOrig = $("pipInstallBtn");
if (_pipInstallOrig) {
  _pipInstallOrig.addEventListener("click", () => {
    setTimeout(updatePipCmdPreview, 50);
  });
}



// Deepy Prime activation panel: pick a ready engine, write it into
// wgp_config.json so the next Wan2GP launch boots with Deepy Prime enabled.
const DEEPY_PANEL_ENGINES = [
  { id: "opencode", label: "OpenCode", paid: false },
  { id: "claude-code", label: "Claude Code", paid: true },
  { id: "codex", label: "OpenAI Codex", paid: true },
  // ponytail: b71026f — local Prime runs on Qwen3.8 VL 9B/27B (needs the 9B or 27B model + GGUF 1.0.25; backend auto-raises 32k context + Summarize)
  { id: "local-qwen38", label: "Qwen3.8 VL 9B/27B (local)", paid: false },
];

// Local-model (Prompt Enhancer) choices shown in the Deepy panel when Deepy is
// Disabled or Zero.
// modes: which Deepy modes the option is valid for. All options are rendered in
// the UI (the non-applicable ones are shown disabled with an annotation), so
// the user sees the full set of possible local models.
const DEEPY_PANEL_ENHANCERS = [
  { id: 1, label: "Florence 2 + Llama 3.2 3B (local)", modes: ["disabled", "zero"] },
  { id: 2, label: "Florence 2 + Llama Joy 8B (local)", modes: ["disabled", "zero"] },
  {
    id: 3,
    label: "Qwen3.5 VL Abliterated 4B (local, recommended)",
    modes: ["disabled", "zero"],
  },
  { id: 4, label: "Qwen3.5 VL Abliterated 9B (local)", modes: ["disabled", "zero"] },
  { id: 5, label: "Qwen3.8 VL Uncensored 27B (local)", modes: ["disabled", "zero"] },
  { id: 6, label: "Qwen3.8 VL Uncensored 9B (local, Prime-ready ~6.5GB)", modes: ["disabled", "zero"] },
];

// Qwen LLM Quantization choices per local engine id — mirrors upstream's
// "Qwen LLM Quantization" dropdown (shared/prompt_enhancer/qwen35_vl.py
// get_qwen35_quantization). 27B Bonsai PTQ1 (gguf_ptq1) is the ~10 GB VRAM
// checkpoint; 9B Heretic offers GGUF Q4 / Q8; Qwen3.5 offers Quanto Int8 or GGUF Q4.
const DEEPY_QUANT_CHOICES = {
  5: [
    { id: "gguf", label: "GGUF Q4 (default, highest quality)" },
    { id: "gguf_q3", label: "GGUF IQ3_S (middle, 16 GB VRAM)" },
    { id: "gguf_q2", label: "GGUF Q2 (lowest memory)" },
    { id: "gguf_ptq1", label: "Bonsai PTQ1 (~10 GB VRAM, needs kernels 1.0.25+)" },
  ],
  6: [
    { id: "gguf", label: "GGUF Q4 (~6.5 GB VRAM, default)" },
    { id: "gguf_q8", label: "GGUF Q8 (~11 GB VRAM, closest to full precision)" },
  ],
  4: [
    { id: "quanto_int8", label: "Quanto Int8 (recommended, better quality)" },
    { id: "gguf", label: "GGUF Q4 (less VRAM, needs kernels)" },
  ],
  3: [
    { id: "quanto_int8", label: "Quanto Int8 (recommended, better quality)" },
    { id: "gguf", label: "GGUF Q4 (less VRAM, needs kernels)" },
  ],
};
const DEEPY_QUANT_DEFAULT = { 6: "gguf", 5: "gguf", 4: "quanto_int8", 3: "quanto_int8" };
// Prime-local backend pick ("<enhancerId>:<quant>", e.g. "6:gguf_q8"). Null =
// derive from the saved config (gguf_q8 or saved enhancer 6 → 9B Q4, Bonsai /
// saved enhancer 5 → 27B entry, else 27B Q4); a manual pick sticks until the
// saved values change (e.g. after Apply).
let _primeBackend = null;
let _primeBackendCtx = "";
function primeBackendFor(savedQuant, savedEnh) {
  const key = (savedQuant || "") + "|" + (savedEnh ?? "");
  const valid = (v) => ["5:gguf", "5:gguf_q3", "5:gguf_q2", "5:gguf_ptq1", "6:gguf", "6:gguf_q8"].includes(v);
  if (_primeBackend === null || _primeBackendCtx !== key || !valid(_primeBackend)) {
    if (savedQuant === "gguf_q8") _primeBackend = "6:gguf_q8";
    else if (savedQuant === "gguf_q3") _primeBackend = "5:gguf_q3";
    else if (savedQuant === "gguf_q2") _primeBackend = "5:gguf_q2";
    else if (savedQuant === "gguf_ptq1") _primeBackend = "5:gguf_ptq1";
    else if (Number(savedEnh) === 6) _primeBackend = "6:gguf";
    else _primeBackend = "5:gguf";
    _primeBackendCtx = key;
  }
  return _primeBackend;
}

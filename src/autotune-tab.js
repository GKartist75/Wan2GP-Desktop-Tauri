// autotune-tab.js — Performance Settings: Detect seeds dropdowns + rec/saved tags.
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

// ── Performance Settings (unified: Detect seeds dropdowns + rec tags; user overrides; saved tags from disk) ──
function memProfileCollect() {
  // Only include fields the user actually set (non-empty) — unset = leave existing config.
  const s = {};
  const vp = $("memVideoProfile").value;
  const ip = $("memImageProfile").value;
  const ap = $("memAudioProfile").value;
  const co = $("memCoeff").value;
  const ve = $("memVae").value;
  const q = $("memQuant").value;
  const i8k = $("memInt8Kernels") ? $("memInt8Kernels").value : "";
  const kp = $("memKernelPrecision") ? $("memKernelPrecision").value : "";
  if (i8k) s.int8_kernels = i8k;
  if (kp) s.kernel_precision = kp;
  if (vp) s.video_profile = Number(vp);
  if (ip) s.image_profile = Number(ip);
  if (ap) s.audio_profile = Number(ap);
  if (co) {
    const n = Number(co);
    if (!(n > 0 && n <= 1)) {
      setMemStatus("VRAM Safety Coeff must be between 0.1 and 1", true);
      return null;
    }
    s.vram_safety_coefficient = n;
  }
  if (ve !== "") s.vae_config = Number(ve);
  if (q) s.transformer_quantization = q;
  return s;
}

function setMemStatus(msg, isError) {
  const el = $("memProfileStatus");
  if (!el) return;
  el.textContent = msg || "";
  el.style.color = isError ? "var(--signal-red)" : "var(--text-secondary)";
}

// Field metadata: maps the dropdown id to its rec/saved tag ids and a formatter.
const MEM_FIELDS = {
  video_profile: {
    sel: "memVideoProfile",
    rec: "recVideoProfile",
    saved: "savedVideoProfile",
  },
  image_profile: {
    sel: "memImageProfile",
    rec: "recImageProfile",
    saved: "savedImageProfile",
  },
  audio_profile: {
    sel: "memAudioProfile",
    rec: "recAudioProfile",
    saved: "savedAudioProfile",
  },
  vram_safety_coefficient: {
    sel: "memCoeff",
    rec: "recCoeff",
    saved: "savedCoeff",
  },
  vae_config: { sel: "memVae", rec: "recVae", saved: "savedVae" },
  transformer_quantization: {
    sel: "memQuant",
    rec: "recQuant",
    saved: "savedQuant",
  },
  int8_kernels: {
    sel: "memInt8Kernels",
    rec: "recInt8Kernels",
    saved: "savedInt8Kernels",
  },
  kernel_precision: {
    sel: "memKernelPrecision",
    rec: "recKernelPrecision",
    saved: "savedKernelPrecision",
  },
};
const INT8_KERNEL_LABELS = {
  auto: "Auto (default)",
  kitchen: "Comfy Kitchen",
  triton: "Triton",
  disabled: "Disabled (PyTorch)",
};
const KERNEL_PRECISION_LABELS = {
  fast: "Approximate (default)",
  strict: "Preserve precision",
};
const QUEUE_COLOR_LABELS = {
  pastel: "Pastel rainbow (default)",
  grey: "Theme grey",
};
function fmtVal(key, v) {
  if (v == null || v === "") return "—";
  if (key === "vae_config") return v + (Number(v) === 0 ? " (AUTO)" : "");
  if (key === "int8_kernels")
    return INT8_KERNEL_LABELS[v] || String(v);
  if (key === "kernel_precision")
    return KERNEL_PRECISION_LABELS[v] || String(v);
  if (key === "queue_color_scheme")
    return QUEUE_COLOR_LABELS[v] || String(v);
  return String(v);
}

function memProfilePopulate(settings, opts = {}) {
  if (!settings) return;
  // opts.mode: 'recommend' fills the dropdown + rec tags; 'saved' fills rec tags from detect AND saved tags from disk.
  for (const key of Object.keys(MEM_FIELDS)) {
    const f = MEM_FIELDS[key];
    const v = settings[key];
    if (opts.mode === "recommend") {
      // Seed the dropdown with the recommended value (user can override).
      const sel = $(f.sel);
      if (sel)
        sel.value =
          v != null && v !== "" ? String(v) : key === "vae_config" ? "0" : "";
      const rec = $(f.rec);
      if (rec) rec.textContent = "rec: " + fmtVal(key, v);
    } else if (opts.mode === "saved") {
      // Show what's currently written to disk (preferred/saved).
      const saved = $(f.saved);
      if (saved) saved.textContent = "saved: " + fmtVal(key, v);
    }
  }
}

// Feed the manual Adjuster from an Auto-Tune detection result: set the dropdown
// defaults to the recommended values AND show the rec tags. The user can then
// override any dropdown before pressing Apply.
function memProfileFromRecommendation(rec) {
  if (!rec) return;
  memProfilePopulate(
    {
      video_profile: rec.video_profile,
      image_profile: rec.image_profile,
      audio_profile: rec.audio_profile,
      vram_safety_coefficient: rec.vram_safety_coefficient,
      vae_config: rec.vae_config == null ? 0 : rec.vae_config, // AUTO unless Detect set a fixed value
      transformer_quantization: rec.transformer_quantization,
      // Upstream v13.13 string enums (legacy numeric enable_int8_kernels
      // mapped defensively — the backend no longer sends it).
      int8_kernels:
        rec.int8_kernels ||
        (rec.enable_int8_kernels === 0 ? "disabled" : "auto"),
      kernel_precision: rec.kernel_precision || "fast",
    },
    { mode: "recommend" },
  );
}

async function memProfileLoad() {
  try {
    const res = await window.w2gp.memoryProfileRead();
    if (res && res.ok) {
      // Show the currently-saved (preferred) values from disk.
      memProfilePopulate(res.settings, { mode: "saved" });
      // If a detection already populated the dropdowns, leave them; otherwise
      // seed the dropdowns from the saved config too so the panel isn't empty.
      const first = $("memVideoProfile");
      if (first && first.value === "")
        memProfilePopulate(res.settings, { mode: "recommend" });
    } else
      setMemStatus(
        (res && res.error) || "Failed to read memory settings",
        true,
      );
  } catch (e) {
    setMemStatus(e.message, true);
  }
}

$("memProfileApplyBtn")?.addEventListener("click", async () => {
  const btn = $("memProfileApplyBtn");
  const s = memProfileCollect();
  if (!s) return;
  if (Object.keys(s).length === 0) {
    setMemStatus("Set at least one field before applying.", true);
    return;
  }
  btn.disabled = true;
  btn.textContent = "Applying…";
  setMemStatus("");
  try {
    const res = await window.w2gp.memoryProfileApply(s);
    if (res && res.ok)
      setMemStatus(
        "✓ Applied: " +
          res.applied.join(", ") +
          " — restart Wan2GP to take effect.",
        false,
      );
    else setMemStatus("✗ " + ((res && res.error) || "apply failed"), true);
  } catch (e) {
    setMemStatus("✗ " + e.message, true);
  } finally {
    btn.disabled = false;
    btn.textContent = "Apply Overrides";
  }
});

// (memProfileLoad is called from switchSettingsTab — every entry path.)

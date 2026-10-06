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
  const am = $("memAttentionMode") ? $("memAttentionMode").value : "";
  // v17 (MMGP v4) — CUDA-only upstream, so Detect never recommends these on
  // AMD/Intel; a manual pick here is still honoured. One control per setting,
  // mirroring WanGP's Configuration > RAM/VRAM Management panel.
  const va = $("memVramAllocator") ? $("memVramAllocator").value : "";
  const hs = $("memHeadSplit") ? $("memHeadSplit").value : "";
  const ra = $("memReadAhead") ? $("memReadAhead").value : "";
  const sp = $("memSmartPinning") ? $("memSmartPinning").value : "";
  const vpm = $("memVideoPreload") ? $("memVideoPreload").value : "";
  const ipm = $("memImagePreload") ? $("memImagePreload").value : "";
  const apm = $("memAudioPreload") ? $("memAudioPreload").value : "";
  const prm = $("memReservedPct") ? $("memReservedPct").value : "";
  if (i8k) s.int8_kernels = i8k;
  if (kp) s.kernel_precision = kp;
  if (am) s.attention_mode = am;
  if (va) s.vram_allocator = va;
  if (hs !== "") s.attention_head_split = Number(hs);
  if (ra) s.read_ahead = ra === "true";
  if (sp) s.smart_memory_pinning = sp === "true";
  if (vpm) s.video_preload_mode = vpm;
  if (ipm) s.image_preload_mode = ipm;
  if (apm) s.audio_preload_mode = apm;
  if (prm !== "") {
    const pct = Number(prm);
    if (!(pct >= 0 && pct <= 100)) {
      setMemStatus("Reserved RAM must be 0 (auto) to 100 percent", true);
      return null;
    }
    s.perc_reserved_mem_max = pct;
  }
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
  // v17 (MMGP v4) — same rec/saved contract as every other knob: Detect
  // recommends, Apply writes, a value changed inside WanGP shows as saved.
  attention_mode: {
    sel: "memAttentionMode",
    rec: "recAttentionMode",
    saved: "savedAttentionMode",
  },
  vram_allocator: {
    sel: "memVramAllocator",
    rec: "recVramAllocator",
    saved: "savedVramAllocator",
  },
  attention_head_split: {
    sel: "memHeadSplit",
    rec: "recHeadSplit",
    saved: "savedHeadSplit",
  },
  read_ahead: {
    sel: "memReadAhead",
    rec: "recReadAhead",
    saved: "savedReadAhead",
  },
  smart_memory_pinning: {
    sel: "memSmartPinning",
    rec: "recSmartPinning",
    saved: "savedSmartPinning",
  },
  video_preload_mode: {
    sel: "memVideoPreload",
    rec: "recVideoPreload",
    saved: "savedVideoPreload",
  },
  image_preload_mode: {
    sel: "memImagePreload",
    rec: "recImagePreload",
    saved: "savedImagePreload",
  },
  audio_preload_mode: {
    sel: "memAudioPreload",
    rec: "recAudioPreload",
    saved: "savedAudioPreload",
  },
  perc_reserved_mem_max: {
    sel: "memReservedPct",
    rec: "recReservedPct",
    saved: "savedReservedPct",
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
// v17 (MMGP v4) labels — wording follows upstream's own
// Configuration > RAM/VRAM Management panel and the README v17 section, so
// this panel and WanGP describe the same setting the same way.
const VRAM_ALLOCATOR_LABELS = {
  vmm_spill: "MMGP Optimized, with RAM spilling (default — falls back to shared GPU memory)",
  vmm: "MMGP Optimized, out-of-memory when VRAM is full",
  default: "PyTorch allocator",
};
const HEAD_SPLIT_LABELS = {
  0: "Off (default)",
  1: "Low — saves some VRAM",
  2: "Medium — good balance, ~10% slower",
  3: "High — saves the most VRAM",
};
const ATTENTION_MODE_LABELS = {
  sage2: "SageAttention 2 / 2+ (recommended — v17 gains depend on it)",
  sage: "SageAttention 1 (Turing only)",
  flash: "FlashAttention (quality first, less VRAM)",
  sdpa: "SDPA (PyTorch default, always available)",
};
const PRELOAD_MODE_LABELS = {
  default: "Default — the profile's own choice",
  dynamic: "Dynamic — fill the VRAM each generation leaves free",
  manual: "Manual — use the VRAM Preload amount",
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
  if (key === "vram_allocator")
    return VRAM_ALLOCATOR_LABELS[v] || String(v);
  if (key === "attention_mode") return ATTENTION_MODE_LABELS[v] || String(v);
  if (key === "attention_head_split")
    return HEAD_SPLIT_LABELS[Number(v)] || String(v);
  if (key.endsWith("_preload_mode"))
    return PRELOAD_MODE_LABELS[v] || String(v);
  if (key === "read_ahead" || key === "smart_memory_pinning")
    return v === true ? "On" : v === false ? "Off" : String(v);
  if (key === "perc_reserved_mem_max")
    return Number(v) === 0 ? "Auto" : Number(v) + "%";
  return String(v);
}

// A rec:/saved: chip that outgrew its column is ellipsised by the stylesheet,
// so the full text rides along in the title: nothing is lost, nothing paints
// over the neighbouring field.
function setTag(el, text) {
  el.textContent = text;
  el.title = text;
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
      if (rec) setTag(rec, "rec: " + fmtVal(key, v));
    } else if (opts.mode === "saved") {
      // Show what's currently written to disk (preferred/saved).
      const saved = $(f.saved);
      if (saved) setTag(saved, "saved: " + fmtVal(key, v));
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
        // v17.10 (ec9566a): "quite a few optimizations depends on" Sage2/2+, so the
        // attention mode is part of the recommendation. Absent on AMD/Intel
        // (no Sage build upstream there), leaving it "— unset —".
        attention_mode: rec.attention_mode,
        // v17 levers. Absent on AMD/Intel/CPU (upstream's allocator
        // early-returns there), which leaves those dropdowns on "— unset —".
      vram_allocator: rec.vram_allocator,
      attention_head_split: rec.attention_head_split,
      read_ahead: rec.read_ahead,
      smart_memory_pinning: rec.smart_memory_pinning,
      video_preload_mode: rec.video_preload_mode,
      image_preload_mode: rec.image_preload_mode,
      audio_preload_mode: rec.audio_preload_mode,
      perc_reserved_mem_max: rec.perc_reserved_mem_max,
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
    if (res && res.ok) {
      // Re-read from disk so the `saved:` tags show what Apply just wrote.
      // Without this they keep whatever the panel was loaded with, and a value
      // that genuinely changed (audio P4 → P3+) sits there showing `rec: 3.5`
      // beside a stale `saved: 4` until the panel is reopened.
      await memProfileLoad();
      setMemStatus(
        "✓ Applied: " +
          res.applied.join(", ") +
          " — restart Wan2GP to take effect.",
        false,
      );
    }
    else setMemStatus("✗ " + ((res && res.error) || "apply failed"), true);
  } catch (e) {
    setMemStatus("✗ " + e.message, true);
  } finally {
    btn.disabled = false;
    btn.textContent = "Apply Overrides";
  }
});

// (memProfileLoad is called from switchSettingsTab — every entry path.)

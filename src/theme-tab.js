// theme-tab.js — theme presets, appearance editor and accent application.
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

// ── Theme ──
function applyTheme(theme) {
  const html = document.documentElement;
  document.querySelectorAll(".theme-toggle").forEach((btn) => {
    const sun = btn.querySelector(".sun-icon");
    const moon = btn.querySelector(".moon-icon");
    if (theme === "dark") {
      if (sun) sun.style.display = "none";
      if (moon) moon.style.display = "";
    } else {
      if (sun) sun.style.display = "";
      if (moon) moon.style.display = "none";
    }
  });
  if (theme === "dark") html.setAttribute("data-theme", "dark");
  else html.removeAttribute("data-theme");
}

async function toggleTheme() {
  const cfg = await window.w2gp.configLoad();
  const next = cfg.theme === "dark" ? "light" : "dark";
  cfg.theme = next;
  await window.w2gp.configSave(cfg);
  applyTheme(next);
}

// ── Appearance: theme color + UI/terminal text scale. Exactly 5 themes:
// Mono (original, default) + 4 editable slots prefilled with Sky / Orca /
// Cyber / Matrix. Every slot is edited from 3 base colors (accent /
// background / text) — the full palette is auto-derived. Persisted in
// desktop-config.json as themeAccent (+customThemes)/uiScale/termScale;
// mirrored to localStorage so the term window (no config channel) can follow.
const APPEAR_THEMES = ["mono", "sky", "orca", "cyber", "matrix"];
const APPEAR_ACCENTS = [...APPEAR_THEMES];
// Shipped 3-color bases (used by the editor + Reset). Mono is the original
// launcher look; the other four are prefilled customs the user owns.
const APPEAR_FACTORY = {
  mono: { accent: "#666666", bg: "#242424", text: "#E8E6E1" },
  sky: { accent: "#357fc4", bg: "#152b40", text: "#e6f2fc" },
  orca: { accent: "#7dd3fc", bg: "#0b1220", text: "#e2e8f0" },
  cyber: { accent: "#22d3ee", bg: "#0a0a14", text: "#fef08a" },
  matrix: { accent: "#4ade80", bg: "#03170b", text: "#bbf7d0" },
};
const APPEAR_LABELS = {
  mono: "Mono",
  sky: "Blue Sky",
  orca: "Orca",
  cyber: "Cyber",
  matrix: "Matrix",
};
const _appearMem = { accent: "mono", ui: 100, term: 100, custom: {} };
function _themeDef(name) {
  const over = (_appearMem.custom || {})[name];
  if (_validCustomDef(over)) return over;
  return APPEAR_FACTORY[name] || null;
}
function _isEditableSlot(name) {
  return APPEAR_THEMES.includes(name);
}
// --- color math: derive a full dashboard palette from 3 base colors ---
function _hexRgb(hex) {
  const m = /^#?([0-9a-f]{6})$/i.exec((hex || "").trim());
  if (!m) return null;
  const n = parseInt(m[1], 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}
function _rgbHex(r, g, b) {
  const c = (v) =>
    Math.max(0, Math.min(255, Math.round(v)))
      .toString(16)
      .padStart(2, "0");
  return "#" + c(r) + c(g) + c(b);
}
function _mix(hexA, hexB, t) {
  const a = _hexRgb(hexA),
    b = _hexRgb(hexB);
  if (!a || !b) return hexA;
  return _rgbHex(
    a[0] + (b[0] - a[0]) * t,
    a[1] + (b[1] - a[1]) * t,
    a[2] + (b[2] - a[2]) * t,
  );
}
function _lum(hex) {
  const c = _hexRgb(hex);
  if (!c) return 0.5;
  const f = (v) => {
    v /= 255;
    return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2]);
}
// Build light + dark variable sets from {accent, bg, text}. Dark mode uses
// bg as the surface anchor; light mode keeps white surfaces with tinted
// canvas/borders so text stays readable.
function _deriveCustomPalette(def) {
  const accent = _hexRgb(def.accent) ? def.accent : "#4bd4a1";
  const bg = _hexRgb(def.bg) ? def.bg : "#142d26";
  const text = _hexRgb(def.text) ? def.text : "#e0f8eb";
  const darkBg = _lum(bg) < 0.4;
  const hoverShift = darkBg ? 0.12 : -0.1;
  const shift = (hex, amt) =>
    _mix(hex, _lum(hex) < 0.5 ? "#ffffff" : "#000000", Math.abs(amt));
  const accentHover = shift(accent, hoverShift);
  const light = {
    "--canvas": _mix("#ffffff", accent, 0.06),
    "--surface": "#ffffff",
    "--surface-hover": _mix("#ffffff", accent, 0.12),
    "--border": _mix("#d8d8d8", accent, 0.35),
    "--border-hover": accent,
    "--text-primary": _mix("#1a1a1a", accent, 0.25),
    "--text-secondary": _mix("#555555", accent, 0.35),
    "--text-tertiary": _mix("#999999", accent, 0.35),
    "--accent": accent,
    "--accent-hover": accentHover,
    "--accent-dim": _mix(accent, "#ffffff", 0.45),
    "--bg-secondary": _mix("#ffffff", accent, 0.1),
    "--bg-tertiary": _mix("#ffffff", accent, 0.16),
  };
  const surface = darkBg ? bg : _mix(bg, "#000000", 0.55);
  const canvas = _mix(surface, "#000000", 0.35);
  const dark = {
    "--canvas": canvas,
    "--surface": surface,
    "--surface-hover": _mix(surface, accent, 0.22),
    "--border": _mix(surface, accent, 0.38),
    "--border-hover": accent,
    "--text-primary": _lum(text) > 0.4 ? text : _mix(text, "#ffffff", 0.6),
    "--text-secondary": _mix(text, surface, 0.3),
    "--text-tertiary": _mix(text, surface, 0.55),
    "--accent": _lum(accent) > 0.25 ? accent : _mix(accent, "#ffffff", 0.35),
    "--accent-hover": shift(accent, 0.12),
    "--accent-dim": _mix(accent, surface, 0.45),
    "--bg-secondary": _mix(surface, accent, 0.18),
    "--bg-tertiary": _mix(surface, "#000000", 0.3),
  };
  return { light, dark };
}
function _applyCustomVars(pal) {
  const root = document.documentElement;
  const isDark =
    root.getAttribute("data-theme") === "dark" ||
    (!root.hasAttribute("data-theme") &&
      matchMedia("(prefers-color-scheme: dark)").matches);
  const vars = isDark ? pal.dark : pal.light;
  for (const [k, v] of Object.entries(vars)) root.style.setProperty(k, v);
}
function _clearCustomVars() {
  const root = document.documentElement;
  for (const k of [
    "--canvas",
    "--surface",
    "--surface-hover",
    "--border",
    "--border-hover",
    "--text-primary",
    "--text-secondary",
    "--text-tertiary",
    "--accent",
    "--accent-hover",
    "--accent-dim",
    "--bg-secondary",
    "--bg-tertiary",
  ])
    root.style.removeProperty(k);
}
function _validCustomDef(d) {
  return d && _hexRgb(d.accent) && _hexRgb(d.bg) && _hexRgb(d.text);
}
function _appearStore() {
  try {
    localStorage.setItem("w2gp.accent", _appearMem.accent);
    localStorage.setItem("w2gp.uiScale", String(_appearMem.ui));
    localStorage.setItem("w2gp.termScale", String(_appearMem.term));
    // Prune retired slots on every store so old profiles self-clean.
    const pruned = {};
    for (const t of APPEAR_THEMES)
      if (_validCustomDef((_appearMem.custom || {})[t]))
        pruned[t] = _appearMem.custom[t];
    _appearMem.custom = pruned;
    localStorage.setItem("w2gp.customThemes", JSON.stringify(pruned));
    // Derived vars for the term window (it has no config channel).
    // Mono renders from CSS alone; every other theme ships derived vars.
    const def = _themeDef(_appearMem.accent);
    if (_validCustomDef(def) && _appearMem.accent !== "mono") {
      localStorage.setItem(
        "w2gp.appearVars",
        JSON.stringify(_deriveCustomPalette(def)),
      );
    } else {
      localStorage.removeItem("w2gp.appearVars");
    }
  } catch {}
}
function _paintThemeDots() {
  // Single theme row: every dot shows its live accent color.
  document.querySelectorAll("#themeRow .accent-dot").forEach((d) => {
    const n = d.dataset.dot;
    const def = _themeDef(n);
    const over = (_appearMem.custom || {})[n];
    d.style.background =
      def && _hexRgb(def.accent) ? def.accent : "transparent";
    d.title =
      _displayName(n) +
      (over ? " (edited)" : "") +
      " — click to apply, double-click to edit";
    const on = n === _appearMem.accent;
    d.classList.toggle("active", on);
    d.setAttribute("aria-checked", on ? "true" : "false");
  });
}
function _resetSlot(slot) {
  // Every theme reverts to its shipped colors (drop the override).
  // Mono is the startup default but otherwise a theme like any other.
  const next = Object.assign({}, _appearMem.custom);
  delete next[slot];
  _appearMem.custom = next;
  delete _previewCustomFromEditor._keep;
  if ($("customEditor")) $("customEditor").style.display = "none";
  if (_appearMem.accent === slot) applyAccent(slot);
  else _paintThemeDots();
  persistAppear();
  return _displayName(slot) + " reset to shipped colors";
}
function _displayName(name) {
  return APPEAR_LABELS[name] || name;
}
function applyAccent(accent) {
  const a = APPEAR_ACCENTS.includes(accent) ? accent : "mono";
  _appearMem.accent = a;
  const html = document.documentElement;
  const def = _themeDef(a);
  html.removeAttribute("data-accent");
  _clearCustomVars();
  // Mono renders from the base CSS; every other theme applies its derived
  // palette (factory base or user override — _themeDef resolves both).
  if (a !== "mono" && _validCustomDef(def))
    _applyCustomVars(_deriveCustomPalette(def));
  // Sky keeps its legacy stylesheet fallback only while unedited; an
  // edited Sky runs on derived vars like every other theme.
  if (a === "sky" && !(_appearMem.custom || {})[a])
    html.setAttribute("data-accent", "sky");
  _paintThemeDots();
  const pal = $("appearPaletteBtn");
  if (pal)
    pal.title =
      "Theme: " +
      _displayName(a) +
      " (click for next: Mono → Sky → Orca → Cyber → Matrix)";
  _appearStore();
}
function applyUiScale(pct) {
  const p = Math.min(130, Math.max(85, Math.round(pct / 5) * 5));
  _appearMem.ui = p;
  document.documentElement.style.setProperty("--ui-scale", p / 100);
  const inp = $("uiScaleInput");
  if (inp) inp.value = String(p);
  const val = $("uiScaleVal");
  if (val) val.textContent = p + "%";
  _appearStore();
}
function applyTermScale(pct) {
  const p = Math.min(150, Math.max(85, Math.round(pct / 5) * 5));
  _appearMem.term = p;
  document.documentElement.style.setProperty("--term-scale", p / 100);
  const inp = $("termScaleInput");
  if (inp) inp.value = String(p);
  const val = $("termScaleVal");
  if (val) val.textContent = p + "%";
  _appearStore();
}
async function persistAppear() {
  try {
    // Prune retired slots before saving so desktop-config.json self-cleans.
    const pruned = {};
    for (const t of APPEAR_THEMES)
      if (_validCustomDef((_appearMem.custom || {})[t]))
        pruned[t] = _appearMem.custom[t];
    _appearMem.custom = pruned;
    const cfg = await window.w2gp.configLoad().catch(() => ({}));
    cfg.themeAccent = _appearMem.accent;
    cfg.customThemes = pruned;
    cfg.uiScale = _appearMem.ui;
    cfg.termScale = _appearMem.term;
    await window.w2gp.configSave(cfg).catch(() => {});
  } catch {}
}
function loadAppear(cfg) {
  const c = cfg || {};
  let mem = null;
  try {
    mem = {
      accent: localStorage.getItem("w2gp.accent"),
      ui: parseInt(localStorage.getItem("w2gp.uiScale") || "", 10),
      term: parseInt(localStorage.getItem("w2gp.termScale") || "", 10),
      custom: JSON.parse(localStorage.getItem("w2gp.customThemes") || "null"),
    };
  } catch {
    mem = null;
  }
  const stored =
    c.customThemes && typeof c.customThemes === "object"
      ? c.customThemes
      : null;
  const local =
    mem && mem.custom && typeof mem.custom === "object" ? mem.custom : null;
  // Keep only overrides for the 5 live themes; wipe retired slots
  // (preset1-3, custom1-5, classic/emerald/…) from old profiles.
  const merged = Object.assign({}, local || {}, stored || {});
  const kept = {};
  for (const t of APPEAR_THEMES)
    if (_validCustomDef(merged[t])) kept[t] = merged[t];
  _appearMem.custom = kept;
  let acc = c.themeAccent || (mem && mem.accent) || "mono";
  if (!APPEAR_ACCENTS.includes(acc)) acc = "mono";
  _paintThemeDots();
  applyAccent(acc);
  applyUiScale(c.uiScale || (mem && mem.ui) || 100);
  applyTermScale(c.termScale || (mem && mem.term) || 100);
}
// --- theme editor: click = apply, double-click = edit ---
let _customEditSlot = "mono";
function _hexOk(v) {
  return /^#[0-9a-f]{6}$/i.test((v || "").trim());
}
function _syncHexPair(colorEl, hexEl) {
  if (!colorEl || !hexEl) return;
  colorEl.addEventListener("input", () => {
    hexEl.value = colorEl.value;
    _previewCustomFromEditor();
  });
  hexEl.addEventListener("input", () => {
    if (_hexOk(hexEl.value)) {
      colorEl.value = hexEl.value.trim().toLowerCase();
      _previewCustomFromEditor();
    }
  });
}
function _readEditorDef() {
  const g = (c, h, fb) => {
    const v = ($(h) || {}).value || ($(c) || {}).value || fb;
    return _hexOk(v) ? v.trim().toLowerCase() : fb;
  };
  return {
    accent: g("customAccentInput", "customAccentHex", "#4bd4a1"),
    bg: g("customBgInput", "customBgHex", "#142d26"),
    text: g("customTextInput", "customTextHex", "#e0f8eb"),
  };
}
function _fillEditor(def) {
  const set = (c, h, v) => {
    if ($(c)) $(c).value = v;
    if ($(h)) $(h).value = v;
  };
  set("customAccentInput", "customAccentHex", def.accent);
  set("customBgInput", "customBgHex", def.bg);
  set("customTextInput", "customTextHex", def.text);
}
function _openCustomEditor(slot) {
  if (!_isEditableSlot(slot)) return;
  _customEditSlot = slot;
  const cur = _themeDef(slot) || {
    accent: "#4bd4a1",
    bg: "#142d26",
    text: "#e0f8eb",
  };
  _fillEditor(cur);
  if ($("customEditorLabel"))
    $("customEditorLabel").textContent = "Editing " + _displayName(slot);
  if ($("customEditor")) $("customEditor").style.display = "";
}
function _previewCustomFromEditor() {
  // Live preview: temporarily apply the editor colors without persisting.
  const def = _readEditorDef();
  if (!_validCustomDef(def)) return;
  const root = document.documentElement;
  const keepAccent = _appearMem.accent;
  _clearCustomVars();
  root.removeAttribute("data-accent");
  _applyCustomVars(_deriveCustomPalette(def));
  // Restore the committed theme on the next applyAccent; until Save the
  // preview stays visible so the user sees the result while picking.
  _previewCustomFromEditor._keep = keepAccent;
}
function initAppearControls() {
  if (window.__appearControlsReady) return;
  window.__appearControlsReady = true;
  document.querySelectorAll("#themeRow .accent-dot").forEach((d) => {
    d.addEventListener("click", () => {
      if ($("customEditor")) $("customEditor").style.display = "none";
      // Drop any unsaved editor preview before applying the dot.
      delete _previewCustomFromEditor._keep;
      applyAccent(d.dataset.dot);
      persistAppear();
    });
    d.addEventListener("dblclick", () => _openCustomEditor(d.dataset.dot));
  });
  _syncHexPair($("customAccentInput"), $("customAccentHex"));
  _syncHexPair($("customBgInput"), $("customBgHex"));
  _syncHexPair($("customTextInput"), $("customTextHex"));
  $("customSaveBtn")?.addEventListener("click", () => {
    const def = _readEditorDef();
    if (!_validCustomDef(def)) {
      showToast("Pick 3 valid hex colors first");
      return;
    }
    _appearMem.custom = Object.assign({}, _appearMem.custom, {
      [_customEditSlot]: def,
    });
    delete _previewCustomFromEditor._keep;
    if ($("customEditor")) $("customEditor").style.display = "none";
    applyAccent(_customEditSlot);
    persistAppear();
    showToast(_displayName(_customEditSlot) + " saved (" + def.accent + ")");
  });
  $("customDefaultBtn")?.addEventListener("click", () => {
    const def = _readEditorDef();
    if (_validCustomDef(def)) {
      _appearMem.custom = Object.assign({}, _appearMem.custom, {
        [_customEditSlot]: def,
      });
    }
    delete _previewCustomFromEditor._keep;
    if ($("customEditor")) $("customEditor").style.display = "none";
    applyAccent(_customEditSlot);
    persistAppear();
    showToast(_displayName(_customEditSlot) + " is now the default theme");
  });
  $("customResetBtn")?.addEventListener("click", () => {
    showToast(_resetSlot(_customEditSlot));
  });
  $("uiScaleInput")?.addEventListener("input", (e) => {
    applyUiScale(parseInt(e.target.value, 10) || 100);
    persistAppear();
  });
  $("termScaleInput")?.addEventListener("input", (e) => {
    applyTermScale(parseInt(e.target.value, 10) || 100);
    persistAppear();
  });
  $("appearPaletteBtn")?.addEventListener("click", () => {
    if ($("customEditor")) $("customEditor").style.display = "none";
    const i = APPEAR_THEMES.indexOf(_appearMem.accent);
    applyAccent(APPEAR_THEMES[(i + 1) % APPEAR_THEMES.length]);
    persistAppear();
    showToast("Theme: " + _displayName(_appearMem.accent));
  });
}

let prevPhaseId = null;

// Show renderer errors on splash so blank-screen root cause is visible
window.addEventListener("error", (e) => {
  const el = $("splashError");
  if (el) {
    el.textContent = e.error?.stack || e.message || String(e);
    el.classList.remove("hidden");
  }
});
window.addEventListener("unhandledrejection", (e) => {
  const el = $("splashError");
  if (el) {
    el.textContent = e.reason?.stack || String(e.reason);
    el.classList.remove("hidden");
  }
});

// Theme toggle buttons — moved here from app.js: the handler is a value read at
// load time, so it had to live in the same file as toggleTheme (a later-loading
// script cannot be referenced from app.js top level).
document
  .querySelectorAll(".theme-toggle")
  .forEach((btn) => btn.addEventListener("click", toggleTheme));


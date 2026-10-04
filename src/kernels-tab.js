// kernels-tab.js — GPU Kernel Wheels (profile-driven subsection of Active Environment).
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

// ── GPU Kernel Wheels (profile-driven, subsection of Active Environment) ──
// Renders the wheels resolved from setup_config.json for the active GPU:
// each row shows ✓ (current) / ⚠ (installed, mismatch) / ✗ (not installed).
// Shows the EXACT configured version (e.g. nunchaku 0.3.1) and an "update
// available" hint when the installed wheel is older than the profile declares.
// GTX 10/16, AMD, Apple profiles carry no kernels → the subsection hides.
function renderKernelWheels(wheels, kernelProfile, _osKey) {
  const card = $("kernelWheelsSubsection");
  const box = $("kernelWheels");
  const tag = $("kernelProfileTag");
  if (!card || !box) return;
  // Experimental HIP GGUF opt-in: AMD only, gfx1201 primary target.
  // Other AMD profiles see it too (upstream validation pending, warning on click).
  try {
    const hipBtn = $("installHipGgufBtn");
    if (hipBtn) {
      const p = String(kernelProfile || "");
      hipBtn.style.display = p.indexOf("AMD") === 0 ? "" : "none";
      if (p.indexOf("AMD") === 0 && p !== "AMD_GFX1201") {
        hipBtn.title =
          "Experimental AMD-only: GGUF 1.0.25 torch210rocm714 HIP wheel (upstream targets gfx1201 RX 9070/R9700 — installing on " +
          p +
          " is unvalidated). Needs a separate torch 2.10.0+rocm7.14.0 env — it does not load in the installer's torch 2.13+rocm10 env.";
      } else if (hipBtn.title && hipBtn.title.indexOf("does not load") === -1) {
        hipBtn.title =
          "Experimental AMD-only: GGUF 1.0.25 torch210rocm714 HIP wheel (gfx1201 RX 9070/R9700). Needs a separate torch 2.10.0+rocm7.14.0 env — it does not load in the installer's torch 2.13+rocm10 env.";
      }
    }
  } catch {}
  const list = Array.isArray(wheels) ? wheels : [];
  if (!list.length) {    // Distinguish "no GPU profile" (genuinely nothing to show) from a data
    // error so the user isn't left staring at a blank section.
    if (
      kernelProfile === null ||
      kernelProfile === undefined ||
      kernelProfile === "unknown"
    ) {
      box.innerHTML =
        '<div class="kw-empty">No GPU kernel profile detected — wheels are managed automatically for this GPU.</div>';
    } else {
      box.innerHTML =
        '<div class="kw-empty">This GPU profile has no dedicated kernel wheels.</div>';
    }
    card.style.display = ""; // keep the card; show the friendly note
    if (tag) tag.textContent = kernelProfile || "—";
    try {
      document
        .getElementById("kernelWheelsCard")
        ?.classList.remove("wheels-update");
    } catch {}
    return;
  }
  card.style.display = "";
  if (tag && kernelProfile) tag.textContent = kernelProfile;
  box.innerHTML = "";
  // Pending-update flag for the (possibly collapsed) card header: any
  // versioned wheel that isn't "ok" lights the header badge + rings the
  // Update button green. Bare-string entries carry no version info, so they
  // never flag (can't prove an update exists).
  let needsUpdate = false;
  list.forEach((w) => {
    // ponytail: Tauri spike returns string array; Electron returns objects — handle both
    let unversioned = false;
    if (typeof w === "string") {
      unversioned = true;
      w = { key: w, label: w, pipName: w, state: "missing" };
    }
    const row = document.createElement("div");
    row.className = "spec-row";
    const dot = document.createElement("span");
    dot.className = "spec-dot";
    const state =
      w.state ||
      (w.installed
        ? w.installed === w.configured
          ? "ok"
          : "mismatch"
        : "missing");
    const cls =
      state === "ok" ? "installed" : state === "mismatch" ? "error" : "";
    if (cls) dot.classList.add(cls);
    if (!unversioned && state !== "ok") needsUpdate = true;
    const label = document.createElement("span");
    label.className = "spec-label";
    label.textContent = w.label;
    const val = document.createElement("span");
    val.className = "spec-value";
    if (state === "ok") {
      val.textContent = w.installed;
    } else if (state === "mismatch") {
      val.textContent = w.installed;
      // "update available": installed wheel is older than the profile declares.
      const badge = document.createElement("span");
      badge.className = "kw-update";
      badge.textContent = ` ↑ ${w.configured}`;
      val.appendChild(badge);
    } else {
      val.textContent = `not installed (want ${w.configured || "?"})`;
    }
    row.appendChild(label);
    row.appendChild(dot);
    row.appendChild(val);
    box.appendChild(row);
  });
  try {
    document
      .getElementById("kernelWheelsCard")
      ?.classList.toggle("wheels-update", needsUpdate);
  } catch {}
}

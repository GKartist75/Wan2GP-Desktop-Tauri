// dlss5-tab.js — DLSS5 optional runtime (upstream scripts/install_dlss5.ps1).
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

// ── DLSS5 optional runtime (upstream scripts/install_dlss5.ps1) ──
async function refreshDlss5() {
  const msg = $("dlss5StatusMsg"),
    btn = $("dlss5InstallBtn");
  if (!msg || !btn) return;
  let s = null;
  try {
    s = await window.w2gp.dlss5Status();
  } catch (e) {
    msg.textContent = "✗ " + e.message;
    btn.disabled = true;
    return;
  }
  if (!s || !s.ok) {
    msg.textContent = (s && s.error) || "Wan2GP not installed";
    btn.disabled = true;
    return;
  }
  btn.disabled = false;
  msg.textContent = s.complete
    ? `✓ DLSS 5 installed (${s.present}/${s.total} files).`
    : s.installed
      ? `Partial DLSS 5 install (${s.present}/${s.total} files) — reinstall, or tick Force to replace.`
      : "DLSS 5 not installed — optional NVIDIA upsampler runtime.";
  _dlss5Rows = Array.isArray(s.files) ? s.files : [];
  _dlss5State = {};
  renderDlss5Progress();
}
// ── DLSS5 compatibility check (Test button): mirrors upstream
// unavailable_reason() so workstation cards can see the false-block.
// Read-only: GPU tier, installed files, HAGS, dlssg --probe. Results render
// in-card and mirror to the console; never installs or modifies files.
$("dlss5CheckBtn")?.addEventListener("click", async () => {
  const btn = $("dlss5CheckBtn"),
    box = $("dlss5CheckResult");
  if (!btn || !box) return;
  btn.disabled = true;
  const orig = btn.textContent;
  btn.textContent = "Checking…";
  box.style.display = "block";
  box.style.color = "";
  box.textContent = "Checking DLSS 5 compatibility…";
  appendLog("[*] DLSS 5 compatibility check — progress below…");
  try {
    const r = await window.w2gp.dlss5Check();
    if (!r || !r.ok) throw new Error((r && r.error) || "check failed");
    const fmt = (v) =>
      v && v.available
        ? "✓ available"
        : v && v.state === "not-installed"
          ? "○ compatible — runtime not installed, run Install DLSS 5"
          : `✗ ${(v && v.reason) || "unavailable"}`;
    // Dot color follows the three verdict states: green ready, amber
    // install-to-use, red blocked.
    const dot = (v) =>
      v && v.available
        ? true
        : v && v.state === "not-installed"
          ? "warn"
          : false;
    const probeBits = [];
    if (r.dlssgProbe) {
      probeBits.push(r.dlssgProbe.available ? "✓" : "✗");
      if (r.dlssgProbe.maxFrames != null)
        probeBits.push(`${r.dlssgProbe.maxFrames}x max`);
      if (r.dlssgProbe.runtimeVersion)
        probeBits.push(r.dlssgProbe.runtimeVersion);
    }
    const hagsText =
      `${r.hags === true ? "on" : r.hags === false ? "off" : "unknown"}` +
      (probeBits.length ? ` · probe ${probeBits.join(", ")}` : "");
    const lines = [
      `GPU: ${r.gpuName || "unknown"}${r.profile ? ` (${r.profile})` : ""}`,
      `Series seen by upstream: ${r.upstreamSeries || 0} · launcher tier: ${r.fixedSeries || 0}`,
      `Neural Rendering: ${fmt(r.nr)} (upstream says: ${fmt(r.nrUpstream)})`,
      `Frame Generation: ${fmt(r.fg)} (upstream says: ${fmt(r.fgUpstream)})`,
      `Files: ${r.files.present}/${r.files.total}` +
        (r.files.nrMissing.length || r.files.fgMissing.length
          ? ` — missing: ${[...r.files.nrMissing, ...r.files.fgMissing].join(", ")}`
          : ""),
      `HAGS: ${hagsText}`,
    ];
    if (r.dlssgProbe && r.dlssgProbe.raw)
      lines.push(`Probe detail: ${r.dlssgProbe.raw}`);
    if (r.workstationMismatch)
      lines.push(
        r.runtimePatch && r.runtimePatch.patched
          ? "Workstation fix: applied — restart Wan2GP and Wan2GP will tier this card correctly."
          : r.runtimePatch && r.runtimePatch.upstreamFixed
            ? "Wan2GP now tiers this card natively (upstream merged the fix) — no patch needed."
            : "Note: this card is capable, but upstream Wan2GP only matches “GeForce RTX” names — use “Apply workstation GPU fix” below, then restart Wan2GP.",
      );
    box.textContent = "";
    box.style.color = "";
    const checkRow = (label, text, ok) => {
      const row = document.createElement("div");
      row.className = "spec-row";
      const lab = document.createElement("span");
      lab.className = "spec-label";
      lab.textContent = label;
      const icon = document.createElement("span");
      if (ok === true) icon.className = "dot-ok";
      else if (ok === "warn") icon.style.color = "#FBBF24";
      else if (ok === false) icon.style.color = "#F87171";
      icon.textContent = ok == null ? "" : "●";
      const val = document.createElement("span");
      val.className = "spec-value";
      val.textContent = text;
      if (ok == null) row.append(lab, val);
      else row.append(lab, icon, val);
      box.append(row);
    };
    checkRow(
      "GPU",
      `${r.gpuName || "unknown"}${r.profile ? ` (${r.profile})` : ""}`,
    );
    checkRow(
      "Series",
      `upstream ${r.upstreamSeries || 0} · launcher ${r.fixedSeries || 0}`,
      !r.workstationMismatch,
    );
    checkRow(
      "Neural Rendering",
      fmt(r.nr) + (fmt(r.nr) !== fmt(r.nrUpstream) ? ` · upstream: ${fmt(r.nrUpstream)}` : ""),
      dot(r.nr),
    );
    checkRow(
      "Frame Generation",
      fmt(r.fg) + (fmt(r.fg) !== fmt(r.fgUpstream) ? ` · upstream: ${fmt(r.fgUpstream)}` : ""),
      dot(r.fg),
    );
    checkRow(
      "Files",
      `${r.files.present}/${r.files.total}` +
        (r.files.nrMissing.length || r.files.fgMissing.length
          ? ` — missing: ${[...r.files.nrMissing, ...r.files.fgMissing].join(", ")}`
          : ""),
      r.files.present === r.files.total,
    );
    checkRow("HAGS", hagsText, r.hags !== false);
    if (r.workstationMismatch) {
      const note = document.createElement("div");
      note.className = "pip-advanced-hint";
      note.textContent =
        r.runtimePatch && r.runtimePatch.patched
          ? "Workstation fix: applied — restart Wan2GP and Wan2GP will tier this card correctly. (Untested on real workstation hardware.)"
          : r.runtimePatch && r.runtimePatch.upstreamFixed
            ? "Wan2GP now tiers this card natively (upstream merged the fix) — no patch needed. Update Wan2GP if this still blocks."
            : "This card is capable, but upstream Wan2GP only matches “GeForce RTX” names — use “Apply workstation GPU fix” below, then restart Wan2GP. (Fix untested on real workstation hardware.)";
      box.append(note);
    }
    appendLog("[DLSS5 check]\n" + lines.join("\n"));
    const ready = r.nr.available || r.fg.available;
    const pending =
      !ready &&
      (r.nr.state === "not-installed" || r.fg.state === "not-installed");
    showToast(
      ready
        ? "✓ DLSS 5 check done — see card + console"
        : pending
          ? "○ DLSS 5 compatible — runtime not installed yet"
          : "✗ DLSS 5 unavailable — see card + console",
    );
    syncDlss5FixBtn(r);
  } catch (e) {
    box.textContent = "✗ " + errText(e);
    appendLog("[!] DLSS 5 check failed: " + errText(e));
    showToast("✗ DLSS 5 check failed: " + errText(e));
  } finally {
    btn.disabled = false;
    btn.textContent = orig;
  }
});
// ── DLSS5 workstation-GPU fix (opt-in panel button): patches Wan2GP's own
// postprocessing/dlss5/runtime.py GPU check so RTX PRO / Ada / Ax000 cards
// pass DLSS gating. Backend backs up the original and refuses on upstream
// drift; the button flips to Revert while patched.
let _dlss5FixPatched = false;
function syncDlss5FixBtn(r) {
  const b = $("dlss5FixBtn");
  if (!b) return;
  const p = (r && r.runtimePatch) || {};
  _dlss5FixPatched = !!p.patched;
  // Always visible: the patch is a no-op for GeForce names (it only fires
  // when no GeForce matched), so applying it on a consumer card is harmless
  // and lets anyone test the apply/revert round-trip. Only disabled when
  // Wan2GP's runtime.py isn't there to patch.
  b.style.display = "";
  if (p.patched) {
    b.textContent = "Revert workstation GPU fix";
    b.disabled = false;
    b.title =
      "Restore upstream postprocessing/dlss5/runtime.py from the launcher backup";
  } else if (p.upstreamFixed) {
    b.textContent = "Apply workstation GPU fix";
    b.disabled = true;
    b.title =
      "Upstream Wan2GP already tiers workstation cards — nothing to apply (update Wan2GP if DLSS still blocks)";
  } else {
    b.textContent = "Apply workstation GPU fix";
    b.disabled = !p.found;
    b.title = p.found
      ? r && r.workstationMismatch
        ? "Patch Wan2GP's GPU check so this workstation card passes DLSS gating — backed up, reversible"
        : "Patch Wan2GP's GPU check for workstation cards (RTX PRO, Ada, RTX Ax000) — no-op on this GeForce card, backed up, reversible"
      : "Wan2GP's postprocessing/dlss5/runtime.py not found — install/update Wan2GP first";
  }
}
$("dlss5FixBtn")?.addEventListener("click", async () => {
  const btn = $("dlss5FixBtn");
  if (!btn) return;
  const reverting = _dlss5FixPatched;
  const choice = await window.w2gp.confirmDialog({
    title: reverting ? "Revert workstation GPU fix?" : "Apply workstation GPU fix?",
    message: reverting
      ? "Restore upstream postprocessing/dlss5/runtime.py from the launcher backup?"
      : "Patch Wan2GP's DLSS GPU check for workstation cards?",
    detail: reverting
      ? "Restores the original upstream file. Restart Wan2GP afterwards to pick it up."
      : "Only touches Wan2GP's GPU-tier check (RTX PRO / Ada / RTX Ax000 / L40 / Hopper map to their series). The original is backed up next to it (*.launcher-bak) and upstream updates may overwrite the patch. Not yet verified on real workstation hardware. Stop Wan2GP first, then restart it after.",
  });
  if (choice !== "ok") return;
  btn.disabled = true;
  const orig = btn.textContent;
  btn.textContent = reverting ? "Reverting…" : "Applying…";
  appendLog(
    reverting
      ? "[*] Reverting DLSS workstation-GPU fix — progress below…"
      : "[*] Applying DLSS workstation-GPU fix — progress below…",
  );
  try {
    const res = reverting
      ? await window.w2gp.dlss5RevertWorkstationFix()
      : await window.w2gp.dlss5ApplyWorkstationFix();
    if (res && res.ok) {
      showToast(
        res.upstream
          ? "✓ Wan2GP already tiers workstation cards — nothing to patch"
          : res.already
            ? "✓ Workstation fix already applied"
            : reverting
              ? "✓ Workstation fix reverted — restart Wan2GP"
              : "✓ Workstation fix applied — restart Wan2GP, then Check compatibility",
      );
      appendLog(
        res.upstream
          ? "[*] Upstream runtime.py already handles workstation cards — no patch needed."
          : res.already
            ? "[*] Workstation fix already present — nothing changed."
            : reverting
              ? "[*] Workstation fix reverted — restart Wan2GP."
              : "[*] Workstation fix applied — restart Wan2GP, then Check compatibility.",
      );
    } else showToast("✗ " + ((res && res.error) || "fix failed"));
  } catch (e) {
    showToast("✗ " + errText(e));
    appendLog("[!] Workstation fix failed: " + errText(e));
  } finally {
    btn.disabled = false;
    btn.textContent = orig;
    try {
      const r = await window.w2gp.dlss5Check();
      if (r && r.ok) syncDlss5FixBtn(r);
    } catch {}
  }
});
$("dlss5InstallBtn")?.addEventListener("click", () => {
  $("dlss5AcceptChk").checked = false;
  $("dlss5ConfirmBtn").disabled = true;
  $("dlss5Modal").classList.remove("hidden");
  $("dlss5AcceptChk").focus();
});
$("dlss5AcceptChk")?.addEventListener("change", (e) => {
  $("dlss5ConfirmBtn").disabled = !e.target.checked;
});
$("dlss5CancelBtn")?.addEventListener("click", () => {
  $("dlss5Modal").classList.add("hidden");
});
// ── DLSS5 file overview: one always-visible row per installed file (path +
// version + expected SHA from the backend manifest) with installed /
// not-installed state. Live install events only override the phase mid-install;
// refreshDlss5 re-seeds backend truth after.
// ponytail: the script owns integrity — rows mirror its Downloading /
// verified / Installed lines. True byte-% isn't in the script output, so the
// downloading state is honest (no fake progress bar).
let _dlss5Rows = [],
  _dlss5State = {},
  _dlss5LastPkg = null,
  _dlss5Done = false;
function renderDlss5Progress() {
  const box = $("dlss5Progress");
  if (!box) return;
  if (!_dlss5Rows.length && !_dlss5Done) {
    box.innerHTML = "";
    box.style.display = "none";
    return;
  }
  box.style.display = "block";
  box.textContent = "";
  for (const f of _dlss5Rows) {
    const ph = _dlss5State[f.id];
    const sha = String(f.sha || "");
    const row = document.createElement("div");
    row.className = "spec-row";
    const lab = document.createElement("span");
    lab.className = "spec-label";
    const val = document.createElement("span");
    val.className = "spec-value";
    if (ph === "downloading") {
      const dots = document.createElement("span");
      dots.textContent = "…";
      lab.append(dots, " " + f.id);
      val.textContent = f.version + " · downloading…";
    } else {
      const ok = ph === "verified" || (!ph && f.installed);
      const icon = document.createElement("span");
      if (ok) icon.className = "dot-ok";
      else icon.style.color = "#F87171";
      icon.textContent = "●";
      lab.append(icon, " " + f.id);
      val.textContent =
        f.version +
        " · " +
        (ok ? "✓ SHA " : "SHA ") +
        sha.slice(0, 12) +
        "… " +
        (ok ? "" : "— not installed");
    }
    row.append(lab, val);
    box.append(row);
  }
  if (_dlss5Done) {
    const d = document.createElement("div");
    d.className = "pip-advanced-hint";
    d.style.color = "#4ADE80";
    d.textContent = "✓ DLSS 5 components installed — restart Wan2GP.";
    box.append(d);
  }
}

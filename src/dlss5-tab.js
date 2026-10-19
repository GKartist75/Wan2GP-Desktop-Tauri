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
  _dlss5Verify = {};
  _dlss5Update = {};
  renderDlss5Progress();
  // Paint the last real hash comparison and upstream probe (validated by the
  // backend against the current pin/size/mtime) so a restart does not make
  // verified files look unproven again.
  try {
    const c = await window.w2gp.dlss5CachedState();
    if (c && c.ok) {
      for (const row of c.rows || []) if (row && row.id) _dlss5Verify[row.id] = row;
      const u = c.update || {};
      for (const row of u.rows || []) if (row && row.id) _dlss5Update[row.id] = row;
      _dlss5UpdateCheckedAt = Number(u.checkedAt) || 0;
      renderDlss5Progress();
      maybeRefreshDlss5Updates();
    }
  } catch {}
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
    if (r.hags !== true) {
      // HAGS lives in HKLM (HwSchMode: 2 = on, 1 = off) — the launcher only
      // reads it. Point at the manual toggle: writing it needs admin + reboot.
      const hagsNote = document.createElement("div");
      hagsNote.className = "pip-advanced-hint";
      hagsNote.textContent =
        r.hags === false
          ? "HAGS is OFF — Frame Generation needs it on: Windows Settings → System → Display → Graphics → Hardware-accelerated GPU scheduling → On, then reboot. (Changing it requires administrator rights.)"
          : "HAGS state is unknown — the registry value couldn't be read (on some drivers the key only appears after HAGS was toggled once). Check it at Windows Settings → System → Display → Graphics → Hardware-accelerated GPU scheduling; turning it on needs admin rights + a reboot.";
      box.append(hagsNote);
    }
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
    // The file rows above print a per-file SHA from Wan2GP's manifest, but the
    // backend's "installed" is only an existence check — so answer the question
    // for real while we are already asking: hash every file and compare. Runs
    // only here (and behind an explicit button), never on a plain panel open,
    // because it streams a couple hundred MB.
    try {
      if (!_dlss5Rows.length) await refreshDlss5();
      const v = await verifyDlss5Files();
      if (v && v.ok) {
        const bad = (v.rows || []).filter(
          (x) => x.state === "mismatch" || x.state === "unreadable",
        );
        appendLog(
          `[DLSS5 files] ${v.match}/${v.total} SHA verified · ${v.missing} missing · ${v.mismatch} mismatched` +
            (bad.length
              ? "\n" +
                bad
                  .map(
                    (b) =>
                      `  ${b.id}: expected ${b.expected.slice(0, 12)}… got ${String(b.got).slice(0, 12)}…`,
                  )
                  .join("\n")
              : ""),
        );
        if (bad.length)
          showToast(
            `⚠ DLSS 5: ${bad.length} file(s) don't match the pinned SHA — see console`,
          );
      } else if (v && v.error) {
        appendLog("[!] DLSS 5 file verification unavailable: " + v.error);
      }
    } catch (e) {
      appendLog("[!] DLSS 5 file verification failed: " + errText(e));
    }
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
// ── DLSS5 file overview: one always-visible row per file (version + the SHA
// DLSS5_FILES pins) with its real state. The backend's "installed" is a file
// existence check only, so a row must never imply the SHA matched unless it
// actually did: either the installer verified it live, or the user ran Check
// compatibility, which hashes every file (dlss5_verify_files). Everything else
// says "expected".
// ponytail: the script owns integrity — rows mirror its Downloading /
// verified / Installed lines. True byte-% isn't in the script output, so the
// downloading state is honest (no fake progress bar).
let _dlss5Rows = [],
  _dlss5State = {},
  _dlss5Verify = {},
  _dlss5Update = {},
  _dlss5UpdateCheckedAt = 0,
  // Upstream-update notes are opt-in per session: they appear only after the
  // user presses Check for updates, never from the startup/cached probe.
  _dlss5UpdateAsked = false,
  _dlss5UpdateBusy = false,
  _dlss5LastPkg = null,
  _dlss5Done = false;

// Re-ask upstream at most once per TTL: a fresh launcher should show an
// available update without hammering GitHub, and an offline probe never
// overwrites the last good answer.
const DLSS5_UPDATE_TTL_MS = 6 * 60 * 60 * 1000;

function applyDlss5UpdateRows(r) {
  const rows = Array.isArray(r && r.rows) ? r.rows : [];
  _dlss5Update = {};
  for (const row of rows) if (row && row.id) _dlss5Update[row.id] = row;
  _dlss5UpdateCheckedAt = Number(r && r.checkedAt) || Math.floor(Date.now() / 1000);
  renderDlss5Progress();
}

async function maybeRefreshDlss5Updates() {
  if (_dlss5UpdateBusy) return;
  const age = _dlss5UpdateCheckedAt
    ? Date.now() - _dlss5UpdateCheckedAt * 1000
    : Infinity;
  if (age < DLSS5_UPDATE_TTL_MS) return;
  _dlss5UpdateBusy = true;
  try {
    const r = await window.w2gp.dlss5UpdateCheck();
    if (r && r.ok && r.online) applyDlss5UpdateRows(r);
  } catch {
    // Read-only probe: leave the cached verdicts alone.
  } finally {
    _dlss5UpdateBusy = false;
  }
}

// Hash every runtime file, remember the comparison, and repaint. Used by the
// compatibility button and once after an install so the next restart starts
// green.
async function verifyDlss5Files() {
  const v = await window.w2gp.dlss5VerifyFiles();
  if (v && v.ok) {
    const byId = {};
    for (const row of v.rows || []) if (row && row.id) byId[row.id] = row;
    _dlss5Verify = byId;
    renderDlss5Progress();
  }
  return v;
}
// Verdict for one file row: tone + trailing text. Pure, so the wording — and
// the rule that only a real hash comparison earns the ✓ — is tested without a
// DOM or a couple hundred MB of DLLs.
function dlss5RowVerdict(f, phase, verdict, update) {
  const sha = String((f && f.sha) || "");
  const short = sha.slice(0, 12) + (sha.length > 12 ? "…" : "");
  const got = String((verdict && verdict.got) || "");
  const gotShort = got.slice(0, 12) + (got.length > 12 ? "…" : "");
  // A newer upstream build is news about Wan2GP's pins, not a fault in this
  // file: something that verified (or hash-matches the pin) stays green, and
  // the update is named as a separate muted note. Amber is for a file that is
  // present but unproven, red for missing or wrong bytes. Only a strictly
  // newer build is an "update"; a same-base flavor (`differs`, e.g. DLSSNR
  // SF-v2) is reported by the probe, not here.
  const note =
    update && update.state === "newer" && update.latest
      ? ` · ↻ upstream ${update.latest} — update Wan2GP`
      : "";
  if (phase === "verified")
    return {
      tone: "ok",
      tail: "✓ SHA " + short + " verified by the installer",
      note,
    };
  const st = verdict && verdict.state;
  if (st === "match")
    return {
      tone: "ok",
      tail: "✓ SHA " + short + " matches what Wan2GP pins",
      note,
    };
  if (st === "mismatch")
    return {
      tone: "bad",
      tail: "⚠ SHA " + short + " expected — this file hashes to " + gotShort,
    };
  if (st === "unreadable")
    return {
      tone: "bad",
      tail: "⚠ SHA " + short + " expected — the file could not be read",
    };
  if (st === "missing")
    return { tone: "bad", tail: "SHA " + short + " expected — file missing" };
  if (f && f.installed)
    return {
      tone: "warn",
      tail:
        "SHA " + short + " expected — press Check compatibility to verify",
      note,
    };
  return { tone: "bad", tail: "SHA " + short + " expected — not installed" };
}
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
      const v = dlss5RowVerdict(
        f,
        ph,
        _dlss5Verify[f.id],
        // Notes are opt-in: an available upstream update says nothing about
        // whether the file on disk is right, so it must not decorate the row
        // until the user asks for it.
        _dlss5UpdateAsked ? _dlss5Update[f.pkg] : undefined,
      );
      const icon = document.createElement("span");
      // Green = installed and correct, amber = present but unproven, red =
      // missing or wrong bytes. An available upstream update is a separate
      // muted note, so a correct file never looks like a fault.
      if (v.tone === "ok") icon.className = "dot-ok";
      else icon.style.color = v.tone === "bad" ? "#F87171" : "#FBBF24";
      icon.textContent = "●";
      lab.append(icon, " " + f.id);
      val.textContent = f.version + " · " + v.tail;
      if (v.note) {
        const noteEl = document.createElement("span");
        noteEl.className = "dlss5-note";
        noteEl.textContent = v.note;
        val.append(noteEl);
      }
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

// ── DLSS 5 upstream-release probe (read-only) ─────────────────────────────
// Answers "how would I even know a newer DLSS is out?" without leaving the
// panel. Upstream's scripts/install_dlss5.ps1 owns the component versions —
// its package table is version-pinned with hardcoded URLs and SHA-256s — so a
// newer build only becomes installable through a Wan2GP update. This asks the
// same sources that script pins whether a newer release exists: it downloads
// nothing, installs nothing, and never moves a pin.
const DLSS5_UPDATE_STATES = {
  current: { color: null, text: "up to date with the pinned build" },
  newer: {
    color: "#FBBF24",
    text: "newer release upstream — update Wan2GP to pick it up",
  },
  differs: {
    color: "#FBBF24",
    text: "a different build upstream — check its release notes",
  },
  older: { color: null, text: "pinned build is newer than upstream's newest tag" },
  unknown: { color: "", text: "not checked" },
};
$("dlss5UpdateBtn")?.addEventListener("click", async () => {
  const btn = $("dlss5UpdateBtn"),
    box = $("dlss5UpdateResult");
  if (!btn || !box) return;
  btn.disabled = true;
  const orig = btn.textContent;
  btn.textContent = "Checking…";
  box.style.display = "block";
  box.textContent = "";
  appendLog("[*] DLSS 5 update check — asking the sources upstream's installer pins…");
  try {
    const r = await window.w2gp.dlss5UpdateCheck();
    if (!r || !r.ok) throw new Error((r && r.error) || "check failed");
    const rows = Array.isArray(r.rows) ? r.rows : [];
    // Repaint the file rows too: a newer upstream build is named on the rows
    // it affects. Only an explicit check turns the notes on — the startup
    // probe refreshes the data without decorating a healthy panel.
    if (r.online) {
      _dlss5UpdateAsked = true;
      applyDlss5UpdateRows(r);
    }
    const consoleLines = [];
    for (const row of rows) {
      const st = DLSS5_UPDATE_STATES[row.state] || DLSS5_UPDATE_STATES.unknown;
      const line = document.createElement("div");
      line.className = "spec-row";
      const lab = document.createElement("span");
      lab.className = "spec-label";
      const icon = document.createElement("span");
      // Three verdict colors, same convention as the compatibility rows:
      // green nothing-to-do, amber look-at-this, grey not checked.
      if (st.color === null) icon.className = "dot-ok";
      else icon.style.color = st.color;
      icon.textContent = "●";
      lab.append(icon, " " + row.id);
      const val = document.createElement("span");
      val.className = "spec-value";
      // The tag rides along for anything the launcher isn't already on: for
      // "differs" it is the whole point (a same-base flavor like
      // dlssnr-310.8.Lecram vs the pinned 310.8.SF-v2), and an -rc upstream
      // is flagged so "newer release" can't read as "Wan2GP will install it".
      const rc = /-rc\d*$/i.test(row.latest || "") ? " · release candidate" : "";
      val.textContent =
        `pinned ${row.pinned || "?"}` +
        (row.latest && row.latest !== row.pinned ? ` · upstream ${row.latest}` : "") +
        (row.state !== "current" && row.tag ? ` (${row.tag})` : "") +
        rc +
        ` — ${st.text}`;
      line.append(lab, val);
      box.append(line);
      consoleLines.push(
        `${row.id}: pinned ${row.pinned || "?"} · upstream ${row.latest || "?"} [${row.state}] ${row.tag} (${row.source})`,
      );
    }
    const news = rows.filter((x) => x.state === "newer");
    const note = document.createElement("div");
    note.className = "pip-advanced-hint";
    if (!r.online) {
      note.textContent =
        "Couldn't reach the upstream sources (offline, or GitHub rate limited) — nothing was checked or changed. A GitHub token in Manage settings raises the rate limit.";
    } else if (news.length) {
      note.style.color = "#FBBF24";
      note.textContent =
        `Newer upstream release${news.length > 1 ? "s" : ""}: ` +
        news.map((x) => `${x.id} ${x.latest}`).join(", ") +
        " — install it by updating Wan2GP (this panel never downloads a build upstream has not pinned).";
    } else {
      note.textContent =
        "Nothing newer upstream: these are the builds Wan2GP's own installer pins today.";
    }
    box.append(note);
    appendLog("[DLSS5 update check]\n" + consoleLines.join("\n"));
    showToast(
      !r.online
        ? "✗ DLSS 5 update check — upstream unreachable"
        : news.length
          ? `↻ ${news.length} newer DLSS 5 component${news.length > 1 ? "s" : ""} upstream — update Wan2GP`
          : "✓ DLSS 5 components match upstream's pins",
    );
  } catch (e) {
    box.textContent = "✗ " + errText(e);
    appendLog("[!] DLSS 5 update check failed: " + errText(e));
    showToast("✗ DLSS 5 update check failed: " + errText(e));
  } finally {
    btn.disabled = false;
    btn.textContent = orig;
  }
});

// autoupdate-tab.js — Auto-Update (check/download/install).
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

// ── Auto-Update ──
//
// An update is a state of a control, not a page event — so nothing is added to
// the dashboard layout. A panel up there can only push the buttons under the
// user's cursor, which is exactly how a click once landed on the wrong control.
// The state lives on the "Check Desktop Updates" controls (pulsing dot, green
// ring, and a label that names the version), and the announcement is a single
// clickable toast.

// What pressing an update control should do right now: "" = run a check,
// "download" = fetch the update, "install" = restart into it.
let updateAction = "";
let updateVersion = "";
// One announcement per moment per session: the toast is an event, not a
// fixture, so repeating it on every background poll would be nagging.
let announcedAvailable = false;
let announcedReady = false;

/**
 * What pressing an update control does: finish the pending update when one is
 * already known, otherwise run a check (shift-click = local/offline check).
 */
function runUpdateAction(e) {
  if (updateAction === "install") window.w2gp.installUpdate();
  else if (updateAction === "download") window.w2gp.downloadUpdate();
  else window.w2gp.checkUpdate(e && e.shiftKey ? { local: true } : undefined);
}

/** The button's own text node — its icon and update dot are left alone. */
function updateLabelNode() {
  const btn = $("updateCheckBtn");
  if (!btn) return null;
  for (let i = btn.childNodes.length - 1; i >= 0; i--) {
    const n = btn.childNodes[i];
    if (n.nodeType === 3 && n.textContent.trim()) return n;
  }
  return null;
}

/** Say the state on the control itself: "Update available — v0.10.5". */
function setUpdateLabel(text) {
  const node = updateLabelNode();
  if (!node) return;
  const btn = $("updateCheckBtn");
  if (btn.dataset.labelBase === undefined)
    btn.dataset.labelBase = node.textContent.trim();
  node.textContent = " " + (text || btn.dataset.labelBase);
}

// Reflect Desktop-Launcher update availability on the update controls
// themselves (persistent dot + green border), so users who turned off
// launch-time checking still see there is an update available.
function setDesktopUpdateIndicator(on) {
  for (const id of ["updateCheckBtn", "manageUpdateDesktopBtn"]) {
    const btn = $(id);
    if (!btn) continue;
    btn.classList.toggle("has-update", !!on);
    const dot = btn.querySelector(".update-dot");
    if (on && !dot) {
      const d = document.createElement("span");
      d.className = "update-dot";
      btn.appendChild(d);
    } else if (!on && dot) {
      dot.remove();
    }
  }
}

window.w2gp.onUpdateStatus((status) => {
  // Mirror to Manage → Updates tab if present
  const mS = $("manageUpdateDesktopStatus");
  const mBtn = $("manageUpdateDesktopBtn");
  if (mS && mBtn) {
    if (status.status === "checking") mS.textContent = "Checking...";
    else if (status.status === "available")
      mS.textContent =
        "v" +
        status.version +
        (status.autoDownload === false
            ? " available — press this button to download"
            : " available — downloading automatically");
    else if (status.status === "downloading")
      mS.textContent = "Downloading " + (status.percent || 0) + "%";
      else if (status.status === "downloaded")
        mS.textContent =
        "v" +
        status.version +
        " ready — press this button to install & restart";
    else if (status.status === "up-to-date") mS.textContent = "Up to date ✓";
    else if (status.status === "error")
      mS.textContent = "Error: " + (status.message || "");
  }
  switch (status.status) {
    case "checking":
      // A background check (30s after launch, then every 5h) must not touch
      // the dashboard at all — nothing to announce until something is found.
      break;
      case "available":
      updateVersion = status.version;
        updateAction = status.autoDownload === false ? "download" : "";
      setDesktopUpdateIndicator(true);
      setUpdateLabel(`Update available — v${status.version}`);
        if (!announcedAvailable) {
        announcedAvailable = true;
        if (status.autoDownload === false)
            showToast(
              `UPDATE · v${status.version} available — click to download`,
              () => window.w2gp.downloadUpdate(),
              "update",
            );
        else
            showToast(
              `UPDATE · v${status.version} — downloading…`,
              null,
              "update",
              6000,
            );
        }
        break;
      case "downloading":
        setUpdateLabel(
        `Downloading v${updateVersion} — ${status.percent || 0}%`,
        );
      break;
      case "downloaded":
        updateVersion = status.version;
      updateAction = "install";
      setDesktopUpdateIndicator(true);
      setUpdateLabel(`v${status.version} ready — click to install`);
      if (!announcedReady) {
          announcedReady = true;
          showToast(
            `UPDATE · v${status.version} ready — click to install and restart`,
            () => window.w2gp.installUpdate(),
          "update",
          );
      }
      break;
    case "up-to-date":
      updateAction = "";
      setDesktopUpdateIndicator(false);
      setUpdateLabel("");
      // Console trace so the background boot check (30s + every 5h) leaves
      // evidence; there is nothing to announce and no-update noise.
      try {
          const vv = $("appVersionTag")?.textContent?.trim();
          appendLog("[*] Launcher " + (vv ? vv + " " : "") + "is up to date.");
      } catch {}
      break;
    case "error":
      updateAction = "";
      setDesktopUpdateIndicator(false);
      setUpdateLabel("");
        showToast(
          (status.message || "").includes("401") ||
            (status.message || "").includes("403") ||
            (status.message || "").includes("authentication")
            ? "GitHub rate limited — add a token in Manage settings"
            : "Update check failed: " + (status.message || "unknown"),
      );
      break;
    }
});

// ════════════════════════════════════════════
//  Auto-Tune
// ════════════════════════════════════════════

let _autotuneHardware = null;
let _autotuneRecommendation = null;
let _autotuneAutoDetectDone = false; // D3: auto-run Detect once per session on first tab open

/**
 * GB for a chip: at most one decimal, never a trailing ".0". Probes return raw
 * f64 (31.763145446777344), which reads as noise next to a rounded VRAM figure.
 * Non-numeric / missing input renders as "—", never "NaN" or "undefined".
 */
function fmtGb(v) {
  const n = Number(v);
  if (!Number.isFinite(n) || n <= 0) return "—";
  const r = Math.round(n * 10) / 10;
  return String(Number.isInteger(r) ? r : r.toFixed(1));
}

/** Render hardware info into the card. */
function renderAutoTuneHardware(hw) {
  const el = $("autotuneHardwareInfo");
  if (!hw) {
    el.innerHTML =
      '<p class="token-hint" style="margin:0">Click <strong>Detect</strong> to scan your system.</p>';
    return;
  }
  if (!hw.cuda_available) {
    el.innerHTML =
      '<p class="token-hint" style="margin:0;color:var(--text-secondary)">No NVIDIA GPU detected.</p>';
    return;
  }

  const badges = [];
  if (hw.supports_fp8)
    badges.push(
      '<span class="env-type-tag" style="background:#2D4A2E;color:#8BC48B">FP8</span>',
    );
  if (hw.supports_nvfp4)
    badges.push(
      '<span class="env-type-tag" style="background:#2D3A5E;color:#8AB4F8">NVFP4</span>',
    );
  if (hw.supports_flash)
    badges.push(
      '<span class="env-type-tag" style="background:#3A2D4E;color:#C58AF8">Flash</span>',
    );
  if (hw.supports_sage)
    badges.push(
      '<span class="env-type-tag" style="background:#2D4A3E;color:#8AF8C5">Sage</span>',
    );
  if (hw.supports_triton)
    badges.push(
      '<span class="env-type-tag" style="background:#4A3D2E;color:#F8C58A">Triton</span>',
    );

  el.textContent = "";
  const wrap = document.createElement("div");
  wrap.className = "hw-compact";
  const chip = (labelText, valueText) => {
    const c = document.createElement("span");
    c.className = "hw-chip";
    const l = document.createElement("span");
    l.className = "hw-chip-label";
    l.textContent = labelText;
    c.append(l, valueText);
    return c;
  };
  wrap.append(
    chip("GPU", hw.gpu_name),
    // One decimal at most: ram_gb is a raw f64 (31.763145446777344 GB), and
    // gpu_vram_gb is only rounded by luck of the probe, not by contract.
    chip("VRAM", fmtGb(hw.gpu_vram_gb) + " GB"),
    chip("RAM", fmtGb(hw.ram_gb) + " GB"),
      chip("CUDA", hw.cuda_version || "—"),
    chip("Cap", hw.gpu_capability || "—"),
  );
  const brow = document.createElement("div");
  brow.style.cssText = "display:flex;gap:4px;flex-wrap:wrap;margin-top:4px";
  const badge = (bg, fg, text) => {
    const s = document.createElement("span");
    s.className = "env-type-tag";
    s.style.background = bg;
    s.style.color = fg;
    s.textContent = text;
    return s;
  };
  if (hw.supports_fp8) brow.append(badge("#2D4A2E", "#8BC48B", "FP8"));
  if (hw.supports_nvfp4) brow.append(badge("#2D3A5E", "#8AB4F8", "NVFP4"));
  if (hw.supports_flash) brow.append(badge("#3A2D4E", "#C58AF8", "Flash"));
  if (hw.supports_sage) brow.append(badge("#2D3A3E", "#8AF8C5", "Sage"));
  if (hw.supports_triton) brow.append(badge("#4A3D2E", "#F8C58A", "Triton"));
  el.append(wrap, brow);
}

// escHtml now comes from services/escape.js (loaded before app.js) so the
// module's escaping logic is shared with the node --test suite.

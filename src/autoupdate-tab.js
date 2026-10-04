// autoupdate-tab.js — Auto-Update (check/download/install).
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

// ── Auto-Update ──

// Reflect Desktop-Launcher update availability on the dashboard "Check Desktop
// Updates" action button itself (persistent dot + green border), so users who
// turned off launch-time checking still see there is an update available — not
// only in the transient top banner.
function setDesktopUpdateIndicator(on) {
  for (const id of ["updateCheckBtn", "manageUpdateDesktopBtn"]) {
    const btn = $(id);
    if (!btn) continue;
    if (on) {
      btn.classList.add("has-update");
      if (!btn.querySelector(".update-dot")) {
        const dot = document.createElement("span");
        dot.className = "update-dot";
        btn.appendChild(dot);
      }
    } else {
      btn.classList.remove("has-update");
      btn.querySelector(".update-dot")?.remove();
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
        " available — click Full Download or Quick Update on Dashboard banner";
    else if (status.status === "downloading")
      mS.textContent = "Downloading " + (status.percent || 0) + "%";
    else if (status.status === "downloaded")
      mS.textContent =
        "v" +
        status.version +
        " downloaded — Install & Restart on Dashboard banner";
    else if (status.status === "up-to-date") mS.textContent = "Up to date ✓";
    else if (status.status === "error")
      mS.textContent = "Error: " + (status.message || "");
  }
  switch (status.status) {
    case "checking":
      setDesktopUpdateIndicator(false);
      $("updateText").textContent = "Checking for updates...";
      $("updateBanner").classList.remove("hidden");
      $("updateDownloadBtn").classList.add("hidden");
      $("updateInstallBtn").classList.add("hidden");
      $("updateActions").classList.remove("hidden");
      $("updateProgress").classList.add("hidden");
      $("updateDismissBtn").classList.add("hidden");
      break;
    case "available":
      setDesktopUpdateIndicator(true);
      if (status.autoDownload === false) {
        // Auto-updates disabled: don't auto-download — offer the manual
        // Download button instead.
        $("updateText").textContent = `v${status.version} available`;
        $("updateDownloadBtn").classList.remove("hidden");
        $("updateInstallBtn").classList.add("hidden");
        $("updateActions").classList.remove("hidden");
        $("updateProgress").classList.add("hidden");
        $("updateBanner").classList.remove("hidden");
        $("updateDismissBtn").classList.add("hidden");
      } else {
        $("updateText").textContent = `v${status.version} — downloading...`;
        $("updateDownloadBtn").classList.add("hidden");
        $("updateInstallBtn").classList.add("hidden");
        $("updateActions").classList.add("hidden");
        $("updateProgress").classList.remove("hidden");
        $("progressFill").style.width = "0%";
        $("progressText").textContent = "0%";
        $("updateBanner").classList.remove("hidden");
        $("updateDismissBtn").classList.add("hidden");
      }
      break;
    case "up-to-date":
      setDesktopUpdateIndicator(false);
      $("updateText").textContent = "Up to date ✓";
      // Console trace so the background boot check (30s + every 5h) leaves
      // evidence instead of a 3s banner flash that is easy to miss.
      try {
        const vv = $("appVersionTag")?.textContent?.trim();
        appendLog("[*] Launcher " + (vv ? vv + " " : "") + "is up to date.");
      } catch {}
      $("updateDownloadBtn").classList.add("hidden");
      $("updateActions").classList.remove("hidden");
      $("updateProgress").classList.add("hidden");
      $("updateBanner").classList.remove("hidden");
      $("updateDismissBtn").classList.remove("hidden");
      setTimeout(() => $("updateBanner").classList.add("hidden"), 3000);
      break;
    case "downloading":
      $("updateText").textContent = "Downloading...";
      $("updateDownloadBtn").classList.add("hidden");
      $("updateInstallBtn").classList.add("hidden");
      $("updateActions").classList.add("hidden");
      $("updateProgress").classList.remove("hidden");
      $("progressFill").style.width = status.percent + "%";
      $("progressText").textContent = status.percent + "%";
      $("updateBanner").classList.remove("hidden");
      $("updateDismissBtn").classList.add("hidden");
      break;
    case "downloaded":
      setDesktopUpdateIndicator(false);
      $("updateText").textContent =
        `v${status.version} downloaded — ready to install`;
      $("updateDownloadBtn").classList.add("hidden");
      $("updateInstallBtn").classList.remove("hidden");
      $("updateActions").classList.remove("hidden");
      $("updateProgress").classList.add("hidden");
      $("updateBanner").classList.remove("hidden");
      $("updateDismissBtn").classList.remove("hidden");
      break;
    case "error":
      setDesktopUpdateIndicator(false);
      $("updateText").textContent =
        (status.message || "").includes("401") ||
        (status.message || "").includes("403") ||
        (status.message || "").includes("authentication")
          ? "GitHub rate limited — add token in Manage settings"
          : `Update error: ${status.message}`;
      $("updateDownloadBtn").classList.add("hidden");
      $("updateInstallBtn").classList.add("hidden");
      $("updateActions").classList.add("hidden");
      $("updateProgress").classList.add("hidden");
      $("updateBanner").classList.remove("hidden");
      $("updateDismissBtn").classList.remove("hidden");
      setTimeout(() => $("updateBanner").classList.add("hidden"), 8000);
      break;
  }
});

// ════════════════════════════════════════════
//  Auto-Tune
// ════════════════════════════════════════════

let _autotuneHardware = null;
let _autotuneRecommendation = null;
let _autotuneAutoDetectDone = false; // D3: auto-run Detect once per session on first tab open

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
    chip("VRAM", hw.gpu_vram_gb + " GB"),
    chip("RAM", hw.ram_gb + " GB"),
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

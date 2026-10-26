// settings-tab.js — Settings overlay: tab switching and General controls.
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

// ── Settings ──
$("settingsBackBtn").addEventListener("click", closeSettings);
$("browserRefreshBtn")?.addEventListener("click", loadBrowserList);
document.querySelectorAll('input[name="termDock"]').forEach((r) => {
  r.addEventListener("change", async () => {
    if (!r.checked) return;
    const cfg = await window.w2gp.configLoad();
    cfg.termDockDefault = r.value;
    await window.w2gp.configSave(cfg);
    appendLog(`[*] Floating terminal default set to: ${r.value}`);
  });
});

// F12 is built-in DevTools shortcut. The IPC handler in main.js is kept
// (it opens the BrowserView DevTools when embedded), just no UI button needed.

$("tokenSaveBtn")?.addEventListener("click", async () => {
  const token = $("githubTokenInput")?.value;
  if (!token) return;
  const cfg = await window.w2gp.configLoad();
  cfg.githubToken = token;
  await window.w2gp.configSave(cfg);
  showToast("GitHub token saved");
});
$("tokenClearBtn")?.addEventListener("click", async () => {
  const cfg = await window.w2gp.configLoad();
  cfg.githubToken = null;
  await window.w2gp.configSave(cfg);
  if ($("githubTokenInput")) $("githubTokenInput").value = "";
  showToast("GitHub token cleared");
});
$("tokenDocsLink")?.addEventListener("click", (e) => {
  e.preventDefault();
  window.w2gp.openExternal("https://github.com/settings/tokens");
});
$("hfTokenSaveBtn")?.addEventListener("click", async () => {
  const token = $("hfTokenInput")?.value;
  if (!token) return;
  const cfg = await window.w2gp.configLoad();
  cfg.hfToken = token;
  await window.w2gp.configSave(cfg);
  showToast("HuggingFace token saved");
});
$("hfTokenClearBtn")?.addEventListener("click", async () => {
  const cfg = await window.w2gp.configLoad();
  cfg.hfToken = null;
  await window.w2gp.configSave(cfg);
  if ($("hfTokenInput")) $("hfTokenInput").value = "";
  showToast("HuggingFace token cleared");
});

function makeTokenHandlers(inputId, saveBtnId, clearBtnId, cfgKey, label) {
  $(saveBtnId)?.addEventListener("click", async () => {
    const token = $(inputId)?.value;
    if (!token) return;
    const cfg = await window.w2gp.configLoad();
    cfg[cfgKey] = token;
    await window.w2gp.configSave(cfg);
    showToast(label + " saved");
  });
  $(clearBtnId)?.addEventListener("click", async () => {
    const cfg = await window.w2gp.configLoad();
    cfg[cfgKey] = null;
    await window.w2gp.configSave(cfg);
    if ($(inputId)) $(inputId).value = "";
    showToast(label + " cleared");
  });
}
makeTokenHandlers("civitaiApiKeyInput", "civitaiApiKeySaveBtn", "civitaiApiKeyClearBtn", "civitaiApiKey", "Civitai API key");
makeTokenHandlers("civitaiRedApiKeyInput", "civitaiRedApiKeySaveBtn", "civitaiRedApiKeyClearBtn", "civitaiRedApiKey", "Civitai.red API key");
$("claudeApiKeySaveBtn")?.addEventListener("click", async () => {
  const token = $("claudeApiKeyInput")?.value;
  if (!token) return;
  const cfg = await window.w2gp.configLoad();
  cfg.claudeApiKey = token;
  await window.w2gp.configSave(cfg);
  showToast(
    "Claude API key saved — it will be used for Claude Code on next launch",
  );
  if (typeof refreshLLMEngines === "function") refreshLLMEngines();
});
$("claudeApiKeyClearBtn")?.addEventListener("click", async () => {
  const cfg = await window.w2gp.configLoad();
  cfg.claudeApiKey = null;
  await window.w2gp.configSave(cfg);
  if ($("claudeApiKeyInput")) $("claudeApiKeyInput").value = "";
  showToast("Claude API key cleared");
  if (typeof refreshLLMEngines === "function") refreshLLMEngines();
});
$("launchArgsSaveBtn")?.addEventListener("click", async () => {
  const args = $("launchArgsInput")?.value || "";
  const cfg = await window.w2gp.configLoad();
  cfg.launchArgs = args.trim();
  await window.w2gp.configSave(cfg);
  showToast("Extra launch args saved");
});
$("ggufSaveBtn")?.addEventListener("click", async () => {
  const cfg = await window.w2gp.configLoad();
  cfg.ggufEnv = {
    enabled: $("ggufEnabled")?.checked !== false,
    matmulMode: $("ggufMatmulMode")?.value || "auto",
    shortBatch: $("ggufShortBatch")?.value || "auto",
  };
  await window.w2gp.configSave(cfg);
  showToast("GGUF CUDA kernel settings saved — applies on next launch");
});
$("amdSaveBtn")?.addEventListener("click", async () => {
  const cfg = await window.w2gp.configLoad();
  cfg.amdEnv = {
    miopenDisabled: $("amdMiopenDisabled")?.checked === true,
    deviceTarget: ($("amdDeviceTarget")?.value || "").trim(),
  };
  await window.w2gp.configSave(cfg);
  showToast("AMD settings saved — applies on next launch");
});
$("portSaveBtn")?.addEventListener("click", async () => {
  const val = parseInt($("portInput")?.value) || 7861;
  if (val < 1024 || val > 65535) {
    showToast("Port must be between 1024 and 65535");
    return;
  }
  const cfg = await window.w2gp.configLoad();
  cfg.serverPort = val;
  await window.w2gp.configSave(cfg);
  showToast("Server port set to " + val);
});
// GPU device picker (multi-GPU machines) — populate dropdown + save selection
async function loadGpuDeviceOptions(current) {
  const sel = $("gpuDeviceSelect");
  if (!sel) return;
  // Rebuild the detected rows (keep the two fixed options) so a hot-plugged
  // eGPU, or a card that went away, never leaves a stale entry behind.
  while (sel.options.length > 2) sel.remove(2);
  try {
    const gpus = await window.w2gp.detectGpus();
    gpus.forEach((g) => {
      // Saved by UUID (nvidia-smi indices reshuffle when a card is added,
      // removed or re-slotted) and resolved to `--gpu cuda:N` at launch.
      const v = g.uuid ? "uuid:" + g.uuid : "cuda:" + g.index;
      if (Array.from(sel.options).some((o) => o.value === v)) return;
      const opt = document.createElement("option");
      opt.value = v;
      const vram = g.vramMB ? Math.round(g.vramMB / 1024) + " GB" : "VRAM n/a";
      opt.textContent = g.name + " (" + vram + ")";
      sel.appendChild(opt);
    });
  } catch {
    /* detection failed — the fixed options stay usable */
  }
  const saved = current || "auto";
  // A saved legacy "cuda:N" (or a card that is not installed right now) must
  // still render, or the dropdown silently claims "Auto" while the config says
  // otherwise.
  if (!Array.from(sel.options).some((o) => o.value === saved)) {
    const opt = document.createElement("option");
    opt.value = saved;
    opt.textContent = saved + " (saved — not detected right now)";
    sel.appendChild(opt);
  }
  sel.value = saved;
}
$("gpuDeviceSaveBtn")?.addEventListener("click", async () => {
  const selEl = $("gpuDeviceSelect");
  const val = selEl?.value || "auto";
  const cfg = await window.w2gp.configLoad();
  cfg.gpuDevice = val;
  await window.w2gp.configSave(cfg);
  const label = selEl?.selectedOptions?.[0]?.textContent || val;
  showToast(
    val === "auto"
      ? "GPU device set to Auto — the card with the most VRAM (applies on next launch)"
      : val === "all"
        ? "All GPUs will stay visible (applies on next launch)"
        : "GPU device set to " + label + " (applies on next launch)",
  );
  // Auto-Tune sizes profiles from the pinned card's VRAM; tell it to re-scan
  // instead of leaving a recommendation made for the other GPU.
  document.dispatchEvent(
    new CustomEvent("gpu-device-changed", { detail: { value: val } }),
  );
});
$("launcherGpuSaveBtn")?.addEventListener("click", async () => {
  const val = $("launcherGpuSelect")?.value || "auto";
  const cfg = await window.w2gp.configLoad();
  cfg.launcherGpu = val;
  cfg.electronGpu = val !== "disabled";
  await window.w2gp.configSave(cfg);
  showToast(
    val === "auto"
      ? "Launcher GPU set to Auto (restart to apply)"
      : "Launcher GPU set to " + val + " (restart to apply)",
  );
});
$("sageSafeSaveBtn")?.addEventListener("click", async () => {
  const val = $("sageSafeSelect")?.value || "safe";
  const cfg = await window.w2gp.configLoad();
  cfg.sageSafe = val !== "upstream";
  await window.w2gp.configSave(cfg);
  showToast(
    val === "safe"
      ? "Sage: Safe post6 (applies on next sync/install)"
      : "Sage: Upstream pin as published (applies on next sync/install)",
  );
});
// Bind Address picker — mirror of gpuDevice picker
$("serverNameSaveBtn")?.addEventListener("click", async () => {
  const val = $("serverNameSelect")?.value || "localhost";
  const cfg = await window.w2gp.configLoad();
  cfg.serverName = val;
  await window.w2gp.configSave(cfg);
  showToast("Bind address set to " + val + " (applies on next launch)");
});
// (Dashboard row switch retired — single permanent topbar switch + Manage.)
// Permanent topbar switch: usable while stopped (save + applies on launch).
// Locked while a Desktop session runs (also enforced via disabled).
$("embedModeTop")?.addEventListener("change", async () => {
  // Locked while a Desktop session runs (also enforced via disabled).
  if (appRunning) {
    showToast("Stop the Wan2GP server to switch renderer");
    syncEmbedSwitchLocks();
    return;
  }
  const val = $("embedModeTop")?.value === "native" ? "native" : "iframe";
  try {
    const cfg = await window.w2gp.configLoad();
    const prev = window.w2gp.effectiveEmbedMode(cfg);
    if (val === prev) return;
    cfg.embedMode = val;
    await window.w2gp.configSave(cfg);
    const mg = $("embedModeSelect");
    if (mg) {
      mg.value = val;
      window.w2gp.gateRendererSelect(mg);
    }
    appendLog(
      `[*] Renderer set to ${val} (was ${prev}) — applies on Desktop launch`,
    );
    showToast("Renderer: " + val + " (applies on Desktop launch)");
  } catch (e) {
    showToast("✗ " + errText(e));
  }
});
// A hidden-but-alive view keeps the OLD renderer, so saving a change while
// the Desktop view (or its server) is up offers a one-click relaunch.
$("embedModeSaveBtn")?.addEventListener("click", async () => {
  if (appRunning) {
    showToast("Stop the Wan2GP server to switch renderer");
    syncEmbedSwitchLocks();
    return;
  }
  const val = $("embedModeSelect")?.value === "native" ? "native" : "iframe";
  const cfg = await window.w2gp.configLoad();
  const prev = window.w2gp.effectiveEmbedMode(cfg);
  cfg.embedMode = val;
  await window.w2gp.configSave(cfg);
  if (val === prev) {
    showToast("Desktop embed already " + val);
    return;
  }
  appendLog(`[*] Renderer set to ${val} (was ${prev})`);
  if ((serverMode === "app" && currentUrl) || appRunning) {
    const choice = await window.w2gp.confirmDialog({
      title: "Relaunch Desktop view?",
      message: `Embed mode saved: ${val}. The Desktop view still runs on the old (${prev}) renderer.`,
      detail:
        "OK = destroy + reopen the Desktop view now (Gradio session restarts). Cancel = keep the old view; the new mode applies on your next manual launch.",
    });
    if (choice === "ok") {
      relaunchDesktopView();
      return;
    }
  }
  showToast("Desktop embed: " + val + " (applies on next Desktop launch)");
});
// One-shot WebView2/RAM footprint — run once per embed mode to compare.
$("webviewMemBtn")?.addEventListener("click", async () => {
  const st = $("webviewMemStatus");
  if (st) st.textContent = "Measuring…";
  try {
    const r = await window.w2gp.webviewMemory();
    const line =
      r && r.ok
        ? `WebView2: ${r.webviewMb} MB across ${r.webviewProcs} processes · Launcher: ${r.launcherMb} MB`
        : "Measurement failed";
    if (st)
      st.textContent =
        line +
        (r && r.top && r.top.length
          ? " — biggest: " +
            r.top
              .slice(0, 3)
              .map((t) => "PID " + t.pid + " " + t.mb + "MB")
              .join(", ")
          : "");
    appendLog("[mem] " + line);
    if (r && r.top)
      for (const t of r.top) appendLog(`[mem]   PID ${t.pid}: ${t.mb} MB`);
  } catch (e) {
    if (st) st.textContent = "✗ " + errText(e);
  }
});
$("cliDocsLink")?.addEventListener("click", (e) => {
  e.preventDefault();
  window.w2gp.openExternal(
    "https://github.com/deepbeepmeep/Wan2GP/blob/main/docs/CLI.md",
  );
});

// ── Manage → General: custom Wan2GP resolutions ──
// Wan2GP reads `resolutions.json` from the folder containing wgp.py. The
// launcher only edits that one file; the backend validates every row and
// refuses to write before Wan2GP is installed.
const RESOLUTION_EXAMPLE = [
  ["1600x1200 (4:3, 1080p)", "1600x1200"],
  ["1200x1600 (3:4, 1080p)", "1200x1600"],
  ["1376x768 (16:9, 1MP) - Krea 2 (Official)", "1376x768"],
  ["768x1376 (9:16, 1MP) - Krea 2 (Official)", "768x1376"],
  ["1344x768 (16:9, 1MP) - MM H3 (Official)", "1344x768"],
  ["768x1344 (9:16, 1MP) - MM H3 (Official)", "768x1344"],
  ["1184x896 (4:3, 1MP) - Krea 2 (Official)", "1184x896"],
  ["896x1184 (3:4, 1MP) - Krea 2 (Official)", "896x1184"],
];
let _resolutionsInstalled = true;

function setResolutionsStatus(text, kind) {
  const st = $("resolutionsStatus");
  if (!st) return;
  st.textContent = text || "";
  st.style.color =
    kind === "err"
      ? "var(--signal-red, #9f2f2d)"
      : kind === "ok"
        ? "var(--text-secondary)"
        : "";
}

function resolutionRow(label = "", value = "") {
  const row = document.createElement("div");
  row.className = "resolution-row";
  const l = document.createElement("input");
  l.type = "text";
  l.className = "input res-label";
  l.placeholder = "Label — e.g. 1600x1200 (4:3, 1080p)";
  l.value = label;
  l.maxLength = 200;
  l.spellcheck = false;
  const v = document.createElement("input");
  v.type = "text";
  v.className = "input res-value";
  v.placeholder = "1600x1200";
  v.value = value;
  v.maxLength = 16;
  v.spellcheck = false;
  const del = document.createElement("button");
  del.type = "button";
  del.className = "btn btn-ghost small res-del";
  del.textContent = "×";
  del.title = "Remove this resolution";
  del.setAttribute("aria-label", "Remove resolution");
  del.addEventListener("click", () => row.remove());
  row.append(l, v, del);
  return row;
}

function addResolutionRow(label, value) {
  const host = $("resolutionsList");
  if (!host) return null;
  const row = resolutionRow(label, value);
  host.appendChild(row);
  return row;
}

function collectResolutionRows() {
  const host = $("resolutionsList");
  if (!host) return [];
  const out = [];
  host.querySelectorAll(".resolution-row").forEach((row) => {
    const label = (row.querySelector(".res-label")?.value || "").trim();
    const value = (row.querySelector(".res-value")?.value || "").trim();
    if (!label && !value) return;
    out.push([label, value]);
  });
  return out;
}

async function loadCustomResolutions() {
  const host = $("resolutionsList");
  if (!host) return;
  const pathEl = $("resolutionsPath");
  const saveBtn = $("resolutionsSaveBtn");
  host.innerHTML = "";
  setResolutionsStatus("Loading…");
  try {
    const r = await window.w2gp.customResolutionsRead();
    _resolutionsInstalled = !(r && r.installed === false);
    if (saveBtn) saveBtn.disabled = !_resolutionsInstalled;
    if (pathEl) pathEl.textContent = r && r.path ? "File: " + r.path : "";
    if (r && r.ok === false) {
      setResolutionsStatus(
        "⚠ " +
          (r.error || "Could not read resolutions.json") +
          (r.exists ? " — correcting it and pressing Save replaces the file." : ""),
        "err",
      );
      return;
    }
    const list = (r && r.resolutions) || [];
    for (const pair of list) addResolutionRow(pair[0] || "", pair[1] || "");
    if (!_resolutionsInstalled) {
      setResolutionsStatus(
        "Wan2GP is not installed yet — this editor becomes available after install.",
        "err",
      );
    } else if (!list.length) {
      setResolutionsStatus(
        "No custom resolutions yet. Add a row or load the example.",
      );
    } else {
      setResolutionsStatus(
        `${list.length} custom resolution(s) saved. Restart Wan2GP to see them.`,
        "ok",
      );
    }
  } catch (e) {
    setResolutionsStatus(
      "✗ " + (typeof errText === "function" ? errText(e) : e?.message || e),
      "err",
    );
  }
}

async function saveCustomResolutions() {
  if (!_resolutionsInstalled) {
    setResolutionsStatus("Install Wan2GP first — there is no wgp.py to write next to.", "err");
    return;
  }
  const list = collectResolutionRows();
  if (!list.length) {
    const choice = await window.w2gp.confirmDialog({
      title: "Remove all custom resolutions?",
      message: "The editor is empty — save an empty list?",
      detail:
        "This writes [] to resolutions.json. Wan2GP's built-in resolutions are never removed; only custom entries disappear.",
    });
    if (choice !== "ok") {
      setResolutionsStatus("Nothing saved.");
      return;
    }
  }
  const labels = new Set();
  const sizes = new Set();
  for (let i = 0; i < list.length; i++) {
    const label = list[i][0];
    const value = list[i][1].toLowerCase();
    if (!label) {
      setResolutionsStatus(`Row ${i + 1}: add a label.`, "err");
      return;
    }
    if (!/^[1-9]\d*x[1-9]\d*$/.test(value)) {
      setResolutionsStatus(
        `Row ${i + 1}: "${list[i][1]}" is not WIDTHxHEIGHT (for example 1600x1200).`,
        "err",
      );
      return;
    }
    const [w, h] = value.split("x").map((n) => parseInt(n, 10));
    if (w > 65535 || h > 65535) {
      setResolutionsStatus(`Row ${i + 1}: width and height must be at most 65535.`, "err");
      return;
    }
    if (labels.has(label.toLowerCase())) {
      setResolutionsStatus(`Row ${i + 1}: duplicate label "${label}".`, "err");
      return;
    }
    if (sizes.has(value)) {
      setResolutionsStatus(
        `Row ${i + 1}: duplicate resolution "${value}" — Wan2GP keeps only the first.`,
        "err",
      );
      return;
    }
    labels.add(label.toLowerCase());
    sizes.add(value);
    list[i][1] = value;
  }
  const btn = $("resolutionsSaveBtn");
  if (btn) btn.disabled = true;
  setResolutionsStatus(list.length ? "Saving…" : "Removing all custom resolutions…");
  try {
    const r = await window.w2gp.customResolutionsWrite(list);
    if (r && r.ok) {
      if ($("resolutionsPath") && r.path) {
        $("resolutionsPath").textContent = "File: " + r.path;
      }
      const n = r.count == null ? list.length : r.count;
      setResolutionsStatus(
        n
          ? `✓ Saved ${n} resolution(s). Restart Wan2GP to see them.`
          : "✓ Removed all custom resolutions. Restart Wan2GP to apply.",
        "ok",
      );
      showToast(n ? `Saved ${n} custom resolutions` : "Custom resolutions cleared");
    } else {
      setResolutionsStatus("✗ " + ((r && r.error) || "Save failed"), "err");
    }
  } catch (e) {
    setResolutionsStatus(
      "✗ " + (typeof errText === "function" ? errText(e) : e?.message || e),
      "err",
    );
  } finally {
    if (btn) btn.disabled = !_resolutionsInstalled;
  }
}

$("resolutionsAddBtn")?.addEventListener("click", () => {
  const row = addResolutionRow("", "");
  row?.querySelector(".res-label")?.focus();
});
$("resolutionsReloadBtn")?.addEventListener("click", () => loadCustomResolutions());
$("resolutionsSaveBtn")?.addEventListener("click", () => saveCustomResolutions());
$("resolutionsExampleBtn")?.addEventListener("click", async () => {
  const host = $("resolutionsList");
  if (!host) return;
  const populated = Array.from(host.querySelectorAll(".resolution-row")).some(
    (row) =>
      (row.querySelector(".res-label")?.value || "").trim() ||
      (row.querySelector(".res-value")?.value || "").trim(),
  );
  if (populated) {
    const choice = await window.w2gp.confirmDialog({
      title: "Replace the editor contents?",
      message: "Load the eight example resolutions?",
      detail:
        "This only fills the editor. Nothing is written until you press Save to resolutions.json.",
    });
    if (choice !== "ok") return;
  }
  host.innerHTML = "";
  for (const [label, value] of RESOLUTION_EXAMPLE) addResolutionRow(label, value);
  setResolutionsStatus("Example loaded — review the rows, then Save to resolutions.json.");
});

// ── Manage → General: Electron legacy section + closeSettings ──
// Moved here from term-tab.js: these belong to the settings overlay, and
// settings-tab.js needs closeSettings at load time (it wires the back button).
// ponytail: one-shot detect per Manage open — registry read, no polling
async function refreshElectronSection() {
  const sec = $("electronSection");
  if (!sec) return;
  sec.style.display = "none";
  let det = null;
  try {
    det = await window.w2gp.detectElectron();
  } catch {}
  if (!det || !det.found) return;
  sec.style.display = "";
  const st = $("electronStatus");
  if (st)
    st.textContent =
      "Found: " +
      (det.name || "Electron launcher") +
      (det.version ? " v" + det.version : "") +
      (det.installLocation ? " — " + det.installLocation : "");
}
$("removeElectronBtn")?.addEventListener("click", async function () {
  const choice = await window.w2gp.confirmDialog({
    title: "Remove Electron launcher?",
    message: "Uninstall the legacy Electron launcher?",
    detail:
      "Only the old launcher app is removed. Your Wan2GP install, models, LoRAs, outputs and settings are kept and carry over automatically.",
  });
  if (choice !== "ok") return;
  this.disabled = true;
  const orig = this.textContent;
  this.textContent = "Removing… (see console)";
  appendLog("[*] Removing legacy Electron launcher — progress below…");
  try {
    const r = await window.w2gp.uninstallElectron();
    if (r && r.ok) {
      showToast(
        r.removed
          ? "✓ Electron launcher removed — data kept"
          : "✓ Uninstaller ran (verify in Add/Remove Programs)",
      );
      refreshElectronSection();
    } else {
      showToast("✗ " + ((r && r.error) || "removal failed"));
    }
  } catch (e) {
    showToast("✗ " + e.message);
  } finally {
    this.disabled = false;
    this.textContent = orig;
  }
});
function closeSettings() {
  $("settingsPanel").classList.remove("open");
  var guideOpen = $("guidePanel") && $("guidePanel").classList.contains("open");
  // Only hide the overlay when the Guide panel isn't open either.
  if (!guideOpen) {
    $("settingsOverlay").classList.remove("visible");
  }
  // Restore full viewer bounds when leaving Manage in webview mode
  // (skipped while the Guide panel stays open — it keeps its trim).
  if ($("dashBody").style.display === "none" && !guideOpen) {
    $("settingsOverlay").classList.remove("opaque");
    // Don't reattach over an open terminal — restore the correct view state instead.
    if (_ftVisible) showTerminal();
    else {
      try {
        reshowNativeView();
      } catch (e) {}
    }
  }
}


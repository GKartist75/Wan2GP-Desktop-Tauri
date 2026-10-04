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
    streamK: $("ggufStreamK")?.checked !== false,
    bf16Fp16: $("ggufBf16Fp16")?.checked === true,
  };
  await window.w2gp.configSave(cfg);
  showToast("GGUF CUDA kernel settings saved — applies on next launch");
});
$("amdSaveBtn")?.addEventListener("click", async () => {
  const cfg = await window.w2gp.configLoad();
  cfg.amdEnv = {
    miopenDisabled: $("amdMiopenDisabled")?.checked === true,
  };
  await window.w2gp.configSave(cfg);
  showToast("AMD settings saved — applies on next launch");
});
$("portSaveBtn")?.addEventListener("click", async () => {
  const val = parseInt($("portInput")?.value) || 7860;
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
  try {
    const gpus = await window.w2gp.detectGpus();
    // Keep "Auto" first, then one option per detected GPU
    const existing = Array.from(sel.options).map((o) => o.value);
    gpus.forEach((g) => {
      const v = "cuda:" + g.index;
      if (!existing.includes(v)) {
        const opt = document.createElement("option");
        opt.value = v;
        opt.textContent =
          g.name +
          " (" +
          (g.vramMB ? g.vramMB + " MB" : "VRAM n/a") +
          ") — " +
          v;
        sel.appendChild(opt);
      }
    });
    sel.value = current && /^cuda:\d+$/.test(current) ? current : "auto";
  } catch {
    sel.value = "auto";
  }
}
$("gpuDeviceSaveBtn")?.addEventListener("click", async () => {
  const val = $("gpuDeviceSelect")?.value || "auto";
  const cfg = await window.w2gp.configLoad();
  cfg.gpuDevice = val;
  await window.w2gp.configSave(cfg);
  showToast(
    val === "auto"
      ? "GPU device set to Auto"
      : "GPU device set to " + val + " (applies on next launch)",
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
      : "Sage: Upstream post4 (100% original, applies on next sync/install)",
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
  const val = $("embedModeTop")?.value === "iframe" ? "iframe" : "native";
  try {
    const cfg = await window.w2gp.configLoad();
    const prev = cfg.embedMode === "iframe" ? "iframe" : "native";
    if (val === prev) return;
    cfg.embedMode = val;
    await window.w2gp.configSave(cfg);
    const mg = $("embedModeSelect");
    if (mg) mg.value = val;
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
  const prev = cfg.embedMode === "iframe" ? "iframe" : "native";
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


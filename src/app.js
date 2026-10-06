// ── Global Log Buffer ──
const logBuffer = [];
// ponytail: expose for overlay pre-fill (createBrowserView races launch-log)
window._logBuffer = logBuffer;
window._getLogTail = () =>
  logBuffer.slice(-40).join("\n") + (lastLine ? "\n" + lastLine : "");
const MAX_LOG = 5000;
let lastLine = "";
let _carriageReturn = false; // next text part replaces lastLine instead of appending (tqdm progress bars)
let _renderScheduled = false;
// Coalesce terminal rewrites onto one animation frame: a pip/log flood can emit
// dozens of chunks per second, and each one used to rebuild up to 3 full
// 5k-line textContent blobs synchronously. Now renders at most once per frame.
function scheduleTerminalRender() {
  if (_renderScheduled) return;
  _renderScheduled = true;
  requestAnimationFrame(() => {
    _renderScheduled = false;
    renderTerminals();
  });
}
// tqdm-style progress prints (one full line per refresh, \n-terminated — e.g.
// Wan2GP's `[wan2gp] Generating: 0%| | 0/1 [00:05<?, ?it/s, …]` LLM meter)
// would flood the console with dozens of near-identical rows. Consecutive
// updates sharing the same key (everything before the NN%) collapse into ONE
// row: each new update replaces the previous instead of appending, so the
// row visibly ticks 0% → 100% in place. \r-style bars (pip) already coalesce
// via _carriageReturn below; this covers the \n-printing kind. Returns the
// key, or null when the line is not a progress update.
function progressKey(line) {
  const m = /^(.*?)(\d+)%\|.*\|\s*\d+\/\d+\s*\[.*(?:it\/s|s\/it)/.exec(line);
  // trimEnd: tqdm pads the number field (`Generating:  0%` vs
  // `Generating: 100%`) — padding must not split one bar into two rows.
  return m ? m[1].trimEnd() : null;
}
// Main console entry. `forward=false` for backend-echoed lines (the backend
// already emits those to every window — forwarding would duplicate them in
// the separate term window). Everything else mirrors to the backend bus so
// floating/docked/dashboard consoles stay identical (history + live).
function appendLog(text, forward) {
  if (!text) return;
  if (!text) return;
  // Normalize Windows \r\n to \n first (avoids \r clearing lastLine before \n pushes it)
  const parts = text.replace(/\r\n/g, "\n").split(/(\r|\n)/);
  for (const part of parts) {
    if (part === "\r") {
      // \r = go to start of line — next text OVERWRITES lastLine, doesn't append.
      // The render shows lastLine as the in-progress line, so progress bars stay visible.
      _carriageReturn = true;
    } else if (part === "\n") {
      if (lastLine.trim()) {
        const k = progressKey(lastLine);
        const prev = logBuffer.length
          ? logBuffer[logBuffer.length - 1]
          : null;
        if (k && prev && progressKey(prev) === k)
          logBuffer[logBuffer.length - 1] = lastLine.trim();
        else logBuffer.push(lastLine.trim());
      }
      lastLine = "";
      _carriageReturn = false;
    } else if (part !== "") {
      // NB: skip empty split fragments (chunk ending in \r yields a trailing "").
      // Treating "" as text would wipe lastLine AND disarm _carriageReturn,
      // so \r-terminated progress could never display (stuck pre-0% state).
      if (_carriageReturn) {
        lastLine = part;
        _carriageReturn = false;
      } else {
        lastLine += part;
      }
    }
  }
  if (logBuffer.length > MAX_LOG)
    logBuffer.splice(0, logBuffer.length - MAX_LOG);
  scheduleTerminalRender();
  if (forward !== false) {
    try {
      window.w2gp.mirrorConsole(text);
    } catch {}
  }
}

const termFollow = { termBody: true, ftTermBody: true, installTermBody: true };
const termDirty = {};

const termText = {};
function renderTerminals() {
  // Include the in-progress (carriage-return-updated) line so progress bars are visible
  // before a newline arrives. When lastLine is empty we show buffer only.
  const text = logBuffer.join("\n") + (lastLine ? "\n" + lastLine : "");
  ["termBody", "ftTermBody", "installTermBody"].forEach((id) => {
    const el = document.getElementById(id);
    if (!el) return;
    // Skip offscreen consoles (webview mode hides the dashboard console, the
    // floating overlay hides the DOM console) — writing 5k lines to a hidden
    // element is pure waste. They are flagged dirty and flushed on next show
    // (showTerminal/toggleFloatingTerm call renderTerminals() explicitly).
    if (el.offsetParent === null) {
      termDirty[id] = true;
      return;
    }
    termDirty[id] = false;
    // Dirty-check: skip the textContent write when the text hasn't changed.
    if (termText[id] !== text) {
      termText[id] = text;
      el.textContent = text;
    }
    if (termFollow[id])
      setTimeout(() => {
        el.scrollTop = el.scrollHeight;
      }, 10);
  });
}

// Wipe all local console views + backend history. Local buffers clear
// immediately; the backend call clears LOG_HISTORY and broadcasts
// `console-cleared` so the separate term window wipes its own buffer too.
function clearConsole() {
  logBuffer.length = 0;
  lastLine = "";
  _carriageReturn = false;
  for (const k of Object.keys(termText)) delete termText[k];
  for (const k of Object.keys(termDirty)) delete termDirty[k];
  try {
    const s = $("logSearch");
    if (s && s.value) {
      s.value = "";
      _lastFilter = "";
    }
  } catch {}
  renderTerminals();
  try {
    window.w2gp.clearLogHistory().catch(() => {});
  } catch {}
}
// A Clear issued from another window (separate term window): wipe local
// buffers without re-broadcasting. Registered in init alongside the other
// console listeners.
function onRemoteConsoleCleared() {
  logBuffer.length = 0;
  lastLine = "";
  _carriageReturn = false;
  for (const k of Object.keys(termText)) delete termText[k];
  for (const k of Object.keys(termDirty)) delete termDirty[k];
  renderTerminals();
}

function setupScrollUnfollow(bodyId, btnId) {
  const body = document.getElementById(bodyId);
  const btn = btnId ? document.getElementById(btnId) : null;
  if (!body) return;
  body.addEventListener("scroll", () => {
    const atBottom =
      body.scrollHeight - body.scrollTop - body.clientHeight < 30;
    if (!atBottom && termFollow[bodyId]) {
      termFollow[bodyId] = false;
      if (btn) {
        btn.classList.remove("active");
        const ft = btn.querySelector(".follow-text");
        if (ft) ft.textContent = "Follow";
      }
    } else if (atBottom && !termFollow[bodyId]) {
      termFollow[bodyId] = true;
      if (btn) {
        btn.classList.add("active");
        const ft = btn.querySelector(".follow-text");
        if (ft) ft.textContent = "Follow";
      }
    }
  });
}

const $ = (id) => document.getElementById(id);
// ── Collapsible left-column info cards ──
// A chevron per card header; the collapsed set persists in localStorage (pure
// UI preference, no backend/config churn). Runs at script eval (deferred, so
// the DOM is parsed) to restore state before first paint-ish.
const _collapsedCards = (() => {
  try {
    const v = JSON.parse(localStorage.getItem("w2gp.collapsedCards") || "[]");
    return new Set(Array.isArray(v) ? v : []);
  } catch {
    return new Set();
  }
})();
function saveCollapsedCards() {
  try {
    localStorage.setItem(
      "w2gp.collapsedCards",
      JSON.stringify([..._collapsedCards]),
    );
  } catch {}
}
function initCardCollapse() {
  const wire = (container, labelEl, key) => {
    if (!container || !labelEl || !key) return;
    if (labelEl.querySelector(":scope > .card-collapse-btn")) return;
    const btn = document.createElement("button");
    btn.className = "card-collapse-btn";
    const chev = document.createElement("span");
    chev.className = "chev";
    chev.textContent = "▾";
    btn.appendChild(chev);
    // Key actions (marked data-keep-visible) relocate into the header while
    // collapsed so they stay usable; restored to their exact spots on expand.
    // Same nodes move (listeners/state preserved).
    const kept = [...container.querySelectorAll("[data-keep-visible]")].map(
      (el) => ({ el, parent: el.parentElement, next: el.nextSibling }),
    );
    let keptWrap = null;
    const setCollapsed = (on) => {
      container.classList.toggle("collapsed", on);
      btn.title = on ? "Expand this panel" : "Collapse this panel";
      if (on) {
        if (kept.length && !keptWrap) {
          keptWrap = document.createElement("span");
          keptWrap.className = "collapse-kept";
          kept.forEach(({ el }) => keptWrap.appendChild(el));
          labelEl.insertBefore(keptWrap, btn);
        }
        _collapsedCards.add(key);
      } else {
        if (keptWrap) {
          kept.forEach(({ el, parent, next }) => {
            if (parent && next && next.parentElement === parent)
              parent.insertBefore(el, next);
            else if (parent) parent.appendChild(el);
          });
          keptWrap.remove();
          keptWrap = null;
        }
        _collapsedCards.delete(key);
      }
      saveCollapsedCards();
    };
    btn.setAttribute("aria-label", "Collapse/expand panel");
    btn.addEventListener("click", () =>
      setCollapsed(!container.classList.contains("collapsed")),
    );
    labelEl.appendChild(btn);
    if (_collapsedCards.has(key)) setCollapsed(true);
    else btn.title = "Collapse this panel";
  };
  document
    .querySelectorAll(".col-left .card[id] > .card-header")
    .forEach((header) => wire(header.parentElement, header, header.parentElement && header.parentElement.id));
}
try {
  initCardCollapse();
} catch {}
// Tauri invoke() rejects with the raw backend string, not an Error object —
// reading e.message would print "undefined". Normalizes both shapes.
function errText(e) {
  return (e && e.message) || (e == null ? "unknown error" : e);
}
function show(id) {
  document
    .querySelectorAll(".screen")
    .forEach((s) => s.classList.remove("active"));
  $(id).classList.add("active");
  // Flush console output buffered while the terminal was offscreen (e.g. the
  // post-install show('dashboard') would otherwise leave the Console card
  // empty until the next log line arrives).
  if (Object.values(termDirty).some(Boolean)) renderTerminals();
}
function breakPath(p) {
  if (!p) return p;
  const zwsp = String.fromCharCode(0x200b);
  const bs = String.fromCharCode(0x5c);
  const s = String(p);
  return s
    .split(bs)
    .join(bs + zwsp)
    .split("/")
    .join("/" + zwsp);
}
// One file-as-folder guard (pasted Temp pngs chosen as folders) shared by pickers.
function isFilePickedAsFolder(dir) {
  return (
    /\.(png|jpg|jpeg|webp|bmp|gif)$/i.test(dir) ||
    String(dir).toLowerCase().includes("orca-paste")
  );
}


// ── Guide panel (topbar tab — same overlay/BrowserView rules as Manage) ──
function openGuide() {
  closeSettings();
  if (!$("guidePanel")) return;
  $("guidePanel").classList.add("open");
  $("settingsOverlay").classList.add("visible");
  // Side-by-side like Manage: keep Wan2GP visible beside the panel instead of
  // detaching it to black. Translucent overlay stays click-to-close.
  if ($("dashBody").style.display === "none") {
    try {
      reshowNativeView();
    } catch (e) {}
  }
}
function closeGuide() {
  if ($("guidePanel")) $("guidePanel").classList.remove("open");
  var manageOpen =
    $("settingsPanel") && $("settingsPanel").classList.contains("open");
  // Only hide the overlay when the Manage panel isn't open either.
  if (!manageOpen) {
    $("settingsOverlay").classList.remove("visible");
  }
  if ($("dashBody").style.display === "none" && !manageOpen) {
    $("settingsOverlay").classList.remove("opaque");
    if (_ftVisible) showTerminal();
    else {
      try {
        reshowNativeView();
      } catch (e) {}
    }
  }
}
$("guideBtn")?.addEventListener("click", () => {
  if ($("guidePanel") && $("guidePanel").classList.contains("open")) closeGuide();
  else openGuide();
});
$("guideBackBtn")?.addEventListener("click", closeGuide);
// Window resize while a side panel is open: the native child keeps absolute
// bounds, so re-trim it to the new viewport (debounced, panel-open only).
var __sideResizeT = null;
window.addEventListener("resize", () => {
  try {
    if (sidePanelWidth() <= 0) return;
    if (__sideResizeT) clearTimeout(__sideResizeT);
    __sideResizeT = setTimeout(() => {
      try {
        syncNativeBoundsAdjusted();
      } catch (e) {}
    }, 150);
  } catch (e) {}
});





// ── Hardware ──
async function loadHardware() {
  const s = await window.w2gp.detectHardware();
  $("specCpu").textContent = s.cpu || "—";
  $("specRam").textContent = s.ram || "—";
  $("specGpu").textContent = s.gpu || "—";
  $("specVram").textContent = s.vram || "—";
  return s;
}





// ── Task List ──
const taskMap = {};
document.querySelectorAll(".task").forEach((t) => {
  taskMap[t.dataset.id] = t;
});
function taskStart(id) {
  const t = taskMap[id];
  if (!t) return;
  t.className = "task active";
  t.querySelector(".task-icon").textContent = "○";
  t.querySelector(".task-status").textContent = "running";
}
function taskComplete(id, failed) {
  const t = taskMap[id];
  if (!t) return;
  t.className = failed ? "task fail" : "task done";
  t.querySelector(".task-icon").textContent = failed ? "✕" : "✓";
  t.querySelector(".task-status").textContent = failed ? "failed" : "done";
}
function resetTasks() {
  Object.values(taskMap).forEach((t) => {
    t.className = "task pending";
    t.querySelector(".task-icon").textContent = "○";
    t.querySelector(".task-status").textContent = "pending";
  });
}





// ── Model-path warning ──
// Shows a dashboard banner when the configured checkpoints/LoRAs/output paths
// resolve under the roaming AppData profile (a bad place for huge model files).
async function checkModelsPathWarning() {
  const banner = $("modelsWarnBanner");
  if (!banner) return;
  // ponytail: Tauri uses isolated C:\Wan2GP — hide roaming warning (05cbdb3)
  if (window.__TAURI__) {
    banner.classList.add("hidden");
    return;
  }
  if (banner.dataset.dismissed === "1") {
    banner.classList.add("hidden");
    return;
  }
  try {
    const [paths, ip] = await Promise.all([
      window.w2gp.getModelPaths(),
      window.w2gp.getInstallPaths(),
    ]);
    if (!paths || !ip) {
      banner.classList.add("hidden");
      return;
    }
    const appDataRoot = (ip.appDataRoot || "")
      .toLowerCase()
      .replace(/\\/g, "/");
    const bad =
      appDataRoot &&
      [paths.checkpoints, paths.loras, paths.output]
        .filter(Boolean)
        .some((p) =>
          (p || "").toLowerCase().replace(/\\/g, "/").startsWith(appDataRoot),
        );
    banner.classList.toggle("hidden", !bad);
    // Top warning banner: show its "Migrate to new location" button when a legacy
    // roaming data dir exists — this is the in-launcher entry point the user wants.
    const migrateBtn = $("modelsWarnMigrateBtn");
    if (migrateBtn)
      migrateBtn.classList.toggle("hidden", !ip.legacyRoamingFound);
  } catch {
    banner.classList.add("hidden");
  }
}
$("modelsWarnMigrateBtn")?.addEventListener("click", () =>
  openMigrationModal(),
);
$("modelsWarnDismissBtn")?.addEventListener("click", () => {
  const b = $("modelsWarnBanner");
  if (b) {
    b.classList.add("hidden");
    b.dataset.dismissed = "1";
  }
});

// ── SageAttention broken-wheel banner ──
// RTX 40/50 users who updated the launcher but haven't yet run Kernel sync are
// still on the upstream `cu130torch2.9.0andhigher` SageAttention wheel, whose fp8
// PV kernel corrupts the CUDA context under torch 2.10 (false OOM / stalling).
// The launcher's setSageAttentionSafe() swaps it for the stable cu128 build on
// install / update / Kernel sync. Until they sync, show a top banner telling
// them to click Sync Kernels. Only RTX 40/50 are affected (RTX 30 routes to the
// safe Triton fp16 kernel, RTX 20/older use Sage v1 — neither needs this).
const SAGE_BROKEN = /cu130torch2\.9\.0andhigher/;
function checkSageSyncBanner(status) {
  const banner = $("sageSyncBanner");
  if (!banner) return;
  if (banner.dataset.dismissed === "1") {
    banner.classList.add("hidden");
    return;
  }
  try {
    const profile = status?.kernelProfile;
    const sage =
      status?.versions?.sageattention || status?.versions?.spas_sage_attn || "";
    const affected = profile === "RTX_40" || profile === "RTX_50";
    const brokenWheel = SAGE_BROKEN.test(sage);
    const show = !!(affected && brokenWheel);
    banner.classList.toggle("hidden", !show);
  } catch {
    banner.classList.add("hidden");
  }
}
$("sageSyncBtn")?.addEventListener("click", async () => {
  const banner = $("sageSyncBanner");
  const btn = $("sageSyncBtn");
  if (btn) {
    btn.disabled = true;
    btn.textContent = "Syncing…";
  }
  try {
    const r = await window.w2gp.syncKernels();
    if (r && r.success) {
      if (banner) {
        banner.classList.add("hidden");
        banner.dataset.dismissed = "1";
      }
      refreshDashboard();
    } else if (btn) {
      btn.disabled = false;
      btn.textContent = "Sync Kernels";
    }
  } catch (e) {
    if (btn) {
      btn.disabled = false;
      btn.textContent = "Sync Kernels";
    }
    alert("Kernel sync failed: " + (e?.message || e));
  }
});
$("sageSyncDismissBtn")?.addEventListener("click", () => {
  const b = $("sageSyncBanner");
  if (b) {
    b.classList.add("hidden");
    b.dataset.dismissed = "1";
  }
});



// ── GPU Profile Overview (mirrors setup_config.json gpu_profiles) ──
// Renders the resolved profile's python/torch/attention-kernel matrix in BOTH
// the installer and the dashboard from a single `detail` object, so the two
// views can never disagree. `ids` maps each field to a DOM element id.
function renderProfileOverview(detail, ids) {
  const box = $(ids.box);
  if (!box) return;
  if (!detail) {
    box.style.display = "none";
    return;
  }
  box.style.display = "";
  if (ids.profile) {
    const t = $(ids.profile);
    if (t) t.textContent = (detail.profile || "").replace(/_/g, " ");
  }
  const set = (id, val) => {
    const el = $(id);
    if (el) el.textContent = val || "—";
  };
  set(ids.python, detail.python);
  set(ids.torch, detail.torch);
  set(ids.triton, detail.triton);
  set(ids.sage, detail.sage);
  set(ids.sparge, detail.sparge);
  set(ids.flash, detail.flash);
  set(
    ids.kernels,
    detail.kernels && detail.kernels.length ? detail.kernels.join(", ") : "—",
  );
}







// ── Launch buttons: disabled + hint when Wan2GP is not installed ──
function setLaunchButtonsInstalled(installed) {
  _launchInstalled = !!installed; // client-side installed state (drives paintLaunchInfo)
  [
    "browserBtn",
    "browserNoGpuBtn",
    "termBtn",
    "termNoGpuBtn",
    "appBtn",
  ].forEach((id) => {
    const b = $(id);
    if (b) b.disabled = !installed;
  });
  const hint = $("notInstalledHint");
  if (hint) hint.style.display = installed ? "none" : "block";
}

// ── Yellow first-boot bar is shared by all 5 launch paths — refcount it so a
// failed side-click during another pending boot can't hide it out from
// under the real boot (stranded "Starting…" with no bar). Function
// declarations hoist, so later handlers can call these.
let __launchBarRefs = 0;
function showLaunchInfo() {
  __launchBarRefs += 1;
  $("launchInfo")?.classList.remove("hidden");
}
function hideLaunchInfo() {
  __launchBarRefs = Math.max(0, __launchBarRefs - 1);
  if (__launchBarRefs === 0) $("launchInfo")?.classList.add("hidden");
}
// ── Launch in Browser (uses the user's chosen default browser) ──
// Opens the browser once the server is up (immediately if already running,
// otherwise when the backend reports ready — never a dead URL first).
async function openBrowserView(url, noGpu) {
  if (noGpu) {
    const r = await window.w2gp.launchBrowserNoGpu(url);
    if (!r || !r.success)
      throw new Error((r && r.error) || "no-GPU launch failed");
    appendLog(
      `[*] Launched in browser with GPU disabled${r.via ? ` (${r.via})` : ""}.`,
    );
    if (r.note) appendLog(`[!] ${r.note}`);
  } else {
    await window.w2gp.launchBrowser(url);
  }
  browserRunning = true;
  serverMode = "browser";
  window.w2gp.uiModeSet("browser"); // crash recovery: remember browser mode
  showBrowserRunningUI();
  $("browserBtn").textContent = "Open Browser";
  if (noGpu) {
    $("browserNoGpuBtn").textContent = "Open No-GPU";
    $("browserBtn").style.display = "none";
    // Terminal re-open would silently re-enable GPU acceleration — hide
    // both until Stop (same reason the re-open path pins launchBrowserNoGpu).
    $("termBtn").style.display = "none";
    $("termNoGpuBtn").style.display = "none";
  } else {
    $("browserNoGpuBtn").style.display = "none";
    $("termNoGpuBtn").style.display = "none";
  }
  hideLaunchInfo();
}
$("browserBtn").addEventListener("click", async () => {
  // Already running in browser mode → just re-open the URL (don't re-spawn the server).
  if (browserRunning && currentUrl) {
    await window.w2gp.launchBrowser(currentUrl);
    return;
  }
  const btn = $("browserBtn");
  btn.disabled = true;
  btn.textContent = "Starting...";
  showLaunchInfo();
  appendLog("[*] Starting Wan2GP — watch the console below…\n");
  try {
    const result = await window.w2gp.launch();
    currentUrl = result.url;
    if (!result.fresh) {
      await openBrowserView(result.url, false);
      return;
    }
    _pendingOpen = { kind: "browser", url: result.url, noGpu: false };
    btn.textContent = "Starting… (see console)";
    armPendingTimeout();
  } catch (e) {
    appendLog(`[LAUNCH ERROR] ${errText(e)}`);
    hideLaunchInfo();
    btn.disabled = false;
    if (!browserRunning) btn.textContent = "Browser";
  }
});

// ── Launch in Browser with GPU disabled (start-chrome-no-gpu script) ──
$("browserNoGpuBtn").addEventListener("click", async () => {
  // Already running → re-open with the SAME no-GPU path (previously this
  // fell back to launchBrowser, silently re-enabling GPU acceleration).
  if (browserRunning && currentUrl) {
    await window.w2gp.launchBrowserNoGpu(currentUrl);
    return;
  }
  const btn = $("browserNoGpuBtn");
  btn.disabled = true;
  btn.textContent = "Starting...";
  showLaunchInfo();
  try {
    const result = await window.w2gp.launch();
    currentUrl = result.url;
    if (!result.fresh) {
      await openBrowserView(result.url, true);
      return;
    }
    _pendingOpen = { kind: "browser", url: result.url, noGpu: true };
    btn.textContent = "Starting… (see console)";
    armPendingTimeout();
  } catch (e) {
    appendLog(`[LAUNCH ERROR] ${errText(e)}`);
    hideLaunchInfo();
    btn.disabled = false;
    if (!browserRunning) btn.textContent = "Browser No-GPU";
  }
});

// ── Launch in a real terminal (run.bat style: server runs in a cmd window) ──
$("termBtn").addEventListener("click", async () => {
  if (browserRunning && currentUrl) {
    await window.w2gp.launchBrowser(currentUrl);
    return;
  }
  const btn = $("termBtn");
  btn.disabled = true;
  btn.textContent = "Starting...";
  showLaunchInfo();
  try {
    const result = await window.w2gp.launch("terminal");
    currentUrl = result.url;
    // The generated .bat opens localhost itself (mirrors the desktop shortcut), so we don't double-open.
    browserRunning = true;
    serverMode = "browser"; // UI treatment identical to browser mode (running + Stop + re-open)
    window.w2gp.uiModeSet("browser"); // crash recovery: remember browser mode
    showBrowserRunningUI();
    btn.textContent = "Open Browser";
    $("browserBtn").style.display = "none";
    $("browserNoGpuBtn").style.display = "none";
    $("termNoGpuBtn").style.display = "none";
    hideLaunchInfo();
  } catch (e) {
    appendLog(`[LAUNCH ERROR] ${errText(e)}`);
    hideLaunchInfo();
  } finally {
    $("termBtn").disabled = false;
    if (!browserRunning) $("termBtn").textContent = "Terminal";
  }
});

// ── Launch in a real terminal + No-GPU browser (visible console window running
// wgp.py like run.bat; the script opens the selected default browser with GPU
// acceleration disabled to free VRAM for generation) ──
$("termNoGpuBtn").addEventListener("click", async () => {
  // Already running → re-open with the SAME no-GPU path (never silently
  // re-enable GPU acceleration).
  if (browserRunning && currentUrl) {
    await window.w2gp.launchBrowserNoGpu(currentUrl);
    return;
  }
  const btn = $("termNoGpuBtn");
  btn.disabled = true;
  btn.textContent = "Starting...";
  showLaunchInfo();
  try {
    const result = await window.w2gp.launch("terminal-nogpu");
    currentUrl = result.url;
    // The generated .bat opens the no-GPU browser itself when ready, so we don't double-open.
    browserRunning = true;
    serverMode = "browser"; // UI treatment identical to browser mode (running + Stop + re-open)
    window.w2gp.uiModeSet("browser"); // crash recovery: remember browser mode
    showBrowserRunningUI();
    btn.textContent = "Open No-GPU";
    $("browserBtn").style.display = "none";
    $("browserNoGpuBtn").style.display = "none";
    $("termBtn").style.display = "none";
    hideLaunchInfo();
  } catch (e) {
    appendLog(`[LAUNCH ERROR] ${errText(e)}`);
    hideLaunchInfo();
  } finally {
    $("termNoGpuBtn").disabled = false;
    if (!browserRunning) $("termNoGpuBtn").textContent = "Terminal No-GPU";
  }
});

let currentUrl = null;
// Tracks which launcher path started the server so we can reset the right UI on exit.
let serverMode = null; // 'app' | 'browser' | null
let browserRunning = false; // browser-mode server currently up (button acts as re-open)
let appRunning = false; // desktop-mode (BrowserView) server currently up (button acts as "Back to…")
// Client-side "Wan2GP installed" state (mirrors setLaunchButtonsInstalled).
// Drives paintLaunchInfo: the bar must never show when not installed.
let _launchInstalled = false;




// ── BrowserView navigation / zoom (relayed via main process) ──
// Back/Forward can't work on a cross-origin iframe (Gradio history is
// unreachable) — only Reload is offered. Zoom is real: CSS zoom on the iframe
// embed, compositor zoom (bv_set_zoom) on the native child embed.
$("wvReloadBtn").addEventListener("click", () => {
  if (window.w2gp.isNativeEmbed && window.w2gp.isNativeEmbed()) {
    window.w2gp.bvNavigate("reload");
    return;
  }
  const f = document.querySelector("#tauri-browser-view iframe");
  if (f) f.src = f.src;
});
let _zoomDebounce = null;
$("zoomSlider").addEventListener("input", () => {
  const pct = parseInt($("zoomSlider").value);
  $("zoomLabel").textContent = pct + "%";
  clearTimeout(_zoomDebounce);
  _zoomDebounce = setTimeout(() => {
    if (window.w2gp.isNativeEmbed && window.w2gp.isNativeEmbed()) {
      window.w2gp.bvSetZoom(pct / 100);
      return;
    }
    const f = document.querySelector("#tauri-browser-view iframe");
    if (f) f.style.zoom = pct / 100;
  }, 120);
});



// ── Running LED (Wan2GP) + Deepy Web LED ──
// Wan2GP LED: green while the main server runs, red briefly on stop, hidden
// otherwise. Deepy Web LED: persistent — green while the Deepy Web process
// runs, red while it is stopped (mirrors the Deepy Web card status, polled
// every 15s + refreshed on start/stop). Both LEDs are independent.
function updateLed(state) {
  const led = $("runningLed");
  const dot = $("ledDot");
  const txt = $("ledText");
  if (!led || !dot || !txt) return;
  led.style.display = "inline-flex";
  if (state === "running") {
    dot.className = "led-dot led-running";
    txt.textContent = "Running";
  } else {
    dot.className = "led-dot led-stopped";
    txt.textContent = "Stopped";
  }
}
function updateDeepyWebLed(running) {
  const led = $("deepyWebLed");
  const dot = $("deepyWebLedDot");
  const txt = $("deepyWebLedText");
  if (!led || !dot || !txt) return;
  led.style.display = "inline-flex";
  if (running) {
    dot.className = "led-dot led-running";
    txt.textContent = "Deepy Web";
    led.title = "Deepy Web is running";
  } else {
    dot.className = "led-dot led-stopped";
    txt.textContent = "Deepy Web";
    led.title = "Deepy Web is stopped";
  }
}

// ── Browser-mode running UI (server runs in user's browser; dashboard stays visible) ──
function showBrowserRunningUI() {
  updateLed("running");
}
function hideBrowserRunningUI() {
  $("runningLed").style.display = "none";
}
// Restore the dashboard launch buttons to their default (pre-launch) state.
function resetBrowserLaunchUI() {
  browserRunning = false;
  serverMode = null;
  window.w2gp.uiModeSet(null);
  $("browserBtn").textContent = "Browser";
  $("browserBtn").style.display = "";
  $("browserBtn").disabled = false;
  $("browserNoGpuBtn").textContent = "Browser No-GPU";
  $("browserNoGpuBtn").style.display = "";
  $("browserNoGpuBtn").disabled = false;
  $("termBtn").textContent = "Terminal";
  $("termBtn").style.display = "";
  $("termBtn").disabled = false;
  $("termNoGpuBtn").textContent = "Terminal No-GPU";
  $("termNoGpuBtn").style.display = "";
  $("termNoGpuBtn").disabled = false;
}

// ── Stop Wan2GP button ──
// _expectServerExit marks the exit event our own Stop is about to cause: a
// taskkill victim exits with code 1, which must read as "stopped by user",
// not as a crash (and must never trigger KeyError-crash recovery).
let _expectServerExit = false;
let _expectServerExitTimer = null;
// Shared stop-result handling (single + stop-all buttons): loud on survivors,
// honest counts otherwise. `r` is either stop_wangp's or stop_all_servers'
// {wangp} payload. Returns true when fully stopped.
function noteStopResult(r) {
  const w = (r && r.wangp) || r || {};
  const alive = w.alive || [];
  if (alive.length) {
    // Backend killed what it could but processes survived — stay loud instead
    // of showing a dead-stopped UI over a live server.
    appendLog(
      `[!] ${alive.length} Wan2GP process(es) survived Stop (PID ${alive.join(", ")}). Kill them in Task Manager or restart the PC, then press Stop again.`,
    );
    showToast(`✗ Server still running (PID ${alive.join(", ")}) — see console`);
    return false;
  }
  const killed = w.killed || [];
  if (killed.length) appendLog(`[*] Stopped (${killed.length} process(es)).`);
  else appendLog("[*] Stop requested — no Wan2GP processes were running.");
  return true;
}
// (Retired: the single always-visible #stopAllBtn below replaces the old
// contextual per-view stop button — one button, no show/hide churn.)
// ── Stop ALL servers (always-visible dashboard button) ──
// Wan2GP (+children, verified) and the OpenCode server in one click.
// Never hidden — stopping an already-quiet machine is a harmless no-op.
$("stopAllBtn").addEventListener("click", async () => {
  const btn = $("stopAllBtn");
  appendLog("[*] Stopping all servers (Wan2GP + OpenCode)...");
  // Disabled + label while the multi-second sweep runs (async backend keeps
  // the rest of the UI live; this just prevents double-Stop).
  if (btn) {
    btn.disabled = true;
    btn.title = "Stopping…";
  }
  _expectServerExit = true;
  if (_expectServerExitTimer) clearTimeout(_expectServerExitTimer);
  _expectServerExitTimer = setTimeout(() => {
    _expectServerExit = false;
    _expectServerExitTimer = null;
  }, 10000);
  let clean = false;
  try {
    const r = await window.w2gp.stopAllServers();
    if (r && r.opencode_stopped) appendLog("[*] OpenCode server stopped.");
    clean = noteStopResult(r);
    showToast(
      clean ? "✓ All servers stopped" : "✗ Wan2GP still running — see console",
    );
  } catch (e) {
    appendLog("[!] Stop-all failed: " + errText(e));
    showToast("✗ " + errText(e));
  }
  updateLed("stopped");
  updateFtStatus("stopped");
  if (btn) {
    btn.disabled = false;
    btn.title = "Stop all servers (Wan2GP + OpenCode)";
  }
  // Server is gone: tear down the Desktop view state NOW (don't wait for the
  // exit event) so "Back to Wan2GP in Desktop" can never point at a dead
  // server and no dead page lingers. Survivors (clean=false) keep their view.
  // The reset is UNCONDITIONAL and serialized on the view mutex: a concurrent
  // open/recovery/exit flow must never interleave into dashboard+controls.
  if (clean) {
    await awaitViewFree(20000);
    window.__viewBusy = true;
    try {
      const hadView =
        serverMode === "app" ||
        !$("webviewContainer").classList.contains("hidden");
      appRunning = false;
      _termWinOpen = false;
      try {
        await window.w2gp.destroyBrowserView();
      } catch {}
      serverMode = null;
      try {
        $("webviewContainer").classList.add("hidden");
        $("dashBody").style.display = "";
      } catch {}
      hideWebviewUI();
      $("runningLed").style.display = "none";
      hideNativeHiddenNote();
      try {
        window.w2gp.uiModeSet(null);
      } catch {}
      setAppLaunchLabel();
      appendLog(
        hadView
          ? "[*] Desktop view closed (server stopped)."
          : "[*] Servers stopped (already on dashboard).",
      );
    } finally {
      window.__viewBusy = false;
    }
  }
});



// ── Floating Terminal (Desktop/webview mode only) ──
function updateFtStatus(state) {
  const st = $("ftServerStatus");
  const dot = $("ftStatusDot");
  const txt = $("ftStatusText");
  if (!st || !dot || !txt) return;
  st.style.display = "";
  if (state === "running") {
    dot.className = "ft-status-dot running";
    txt.textContent = "Running";
  } else {
    dot.className = "ft-status-dot stopped";
    txt.textContent = "Stopped";
  }
}

// ── Dependency-drift banner (issue #23): post-update drift is warn-only
// backend-side, so the dashboard makes it unmissable here — toast for
// attention plus a persistent banner with a working Restore action.
// All text via textContent (XSS-safe). Banner stays until Restore
// succeeds or the user dismisses it.
function showDriftBanner(drift) {
  const b = $("driftBanner");
  const t = $("driftBannerText");
  if (!b || !t) return;
  // Defensive: an empty list must hide, never render buttons with no text.
  if (!Array.isArray(drift) || !drift.length) {
    b.style.display = "none";
    return;
  }
  const list = drift.slice(0, 5).join(", ");
  const more = drift.length > 5 ? " (+" + (drift.length - 5) + " more)" : "";
  t.textContent =
    "[!] Dependency drift: " + list + more + " — generation may crash. ";
  b.style.display = "";
}
function hideDriftBanner() {
  const b = $("driftBanner");
  if (b) b.style.display = "none";
}
$("driftRestoreBtn")?.addEventListener("click", async () => {
  if (
    !confirm(
      "Reinstall all packages from requirements.txt? This will restore pinned versions.",
    )
  )
    return;
  const btn = $("driftRestoreBtn");
  if (btn) btn.disabled = true;
  appendLog("[*] Restoring packages from requirements.txt...");
  try {
    const rr = await window.w2gp.restoreRequirements();
    if (rr && rr.success) {
      // Name what landed. pip's own summary scrolls off the console tail on
      // a full requirements run, so without this "restored" covered both a
      // no-op and a real download.
      const got = rr.installed;
      appendLog(
        Array.isArray(got) && got.length
          ? "[*] Restored: " + got.join(", ")
          : "[*] Requirements already matched — nothing installed.",
      );
      hideDriftBanner();
      setTimeout(refreshDashboard, 2000);
    } else showToast("✗ Restore failed: " + ((rr && rr.error) || "unknown"));
  } catch (e) {
    showToast("✗ " + errText(e));
  }
  if (btn) btn.disabled = false;
});
$("driftDismissBtn")?.addEventListener("click", hideDriftBanner);










// ── Dashboard console follow ──
$("dashTermFollowBtn").addEventListener("click", () => {
  termFollow.termBody = !termFollow.termBody;
  const b = $("dashTermFollowBtn");
  b.classList.toggle("active");
  const ft = b.querySelector(".follow-text");
  if (ft) ft.textContent = termFollow.termBody ? "Follow" : "Paused";
  if (termFollow.termBody) {
    const e = $("termBody");
    if (e) setTimeout(() => (e.scrollTop = e.scrollHeight), 10);
  }
});
$("dashTermClearBtn")?.addEventListener("click", clearConsole);
$("ftClearBtn")?.addEventListener("click", clearConsole);
$("installFollowBtn").addEventListener("click", () => {
  termFollow.installTermBody = !termFollow.installTermBody;
  const b = $("installFollowBtn");
  b.classList.toggle("active");
  const ft = b.querySelector(".follow-text");
  if (ft) ft.textContent = termFollow.installTermBody ? "Follow" : "Paused";
  if (termFollow.installTermBody) {
    const e = $("installTermBody");
    if (e) setTimeout(() => (e.scrollTop = e.scrollHeight), 10);
  }
});

// ── Floating terminal: search, export, resize ──
let _lastFilter = "";
$("logSearch")?.addEventListener("input", () => {
  const q = ($("logSearch")?.value || "").toLowerCase();
  if (q === _lastFilter) return;
  _lastFilter = q;
  const ft = $("ftTermBody");
  if (ft)
    ft.textContent = (
      q ? logBuffer.filter((l) => l.toLowerCase().includes(q)) : logBuffer
    ).join("\n");
});
$("logExportBtn")?.addEventListener("click", () => {
  const blob = new Blob([logBuffer.join("\n")], { type: "text/plain" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = "wan2gp-console.log";
  a.click();
  URL.revokeObjectURL(a.href);
});
// Resize handle
let _resize = null;
$("ftResize").addEventListener("mousedown", (e) => {
  e.preventDefault();
  const ft = $("floatingTerminal");
  if (
    !ft.classList.contains("dock-bottom") &&
    !ft.classList.contains("dock-top")
  )
    return;
  _resize = {
    startY: e.clientY,
    startH: ft.offsetHeight,
    dock: ft.classList.contains("dock-top") ? "top" : "bottom",
  };
  document.addEventListener("mousemove", _resizeMove);
  document.addEventListener("mouseup", _resizeEnd);
});
function _resizeMove(e) {
  if (!_resize) return;
  const dh = e.clientY - _resize.startY;
  let h = _resize.dock === "top" ? _resize.startH + dh : _resize.startH - dh;
  h = Math.max(80, Math.min(h, window.innerHeight * 0.6));
  $("floatingTerminal").style.height = h + "px";
  syncTermEmbedPadding();
}
function _resizeEnd() {
  _resize = null;
  document.removeEventListener("mousemove", _resizeMove);
  document.removeEventListener("mouseup", _resizeEnd);
}







// ── Auto-Tune: Detect ──
$("autotuneDetectBtn").addEventListener("click", async () => {
  const btn = $("autotuneDetectBtn");
  const status = $("autotuneStatus");
  btn.disabled = true;
  btn.textContent = "\u27b3 Scanning\u2026";
  status.classList.add("hidden");

  try {
    // Detect + recommend only — nothing is written until Apply is clicked.
    const hw = await window.w2gp.autoTuneDetect();
    _autotuneHardware = hw;
    const rec = await window.w2gp.autoTuneRecommend(hw, {
      failsafe: $("autotuneFailsafeChk").checked,
    });
    _autotuneRecommendation = rec;
    // Feed the manual VRAM/RAM Adjuster so the user can review/edit before Apply.
    memProfileFromRecommendation(rec);

    renderAutoTuneHardware(_autotuneHardware);

    status.className = "";
    status.style.background = "var(--bg-tertiary)";
    status.style.fontSize = "0.7rem";
    status.style.color = "var(--text-secondary)";
    status.innerHTML =
      "\u2139\ufe0f Detection complete. Review the recommendation below, then <strong>Apply</strong> to write settings (Wan2GP must be restarted for them to take effect).";
  } catch (e) {
    status.className = "";
    status.style.background = "#3A1E1E";
    status.textContent =
      "\u274c Detection failed: " + ((e && e.message) || String(e));
  } finally {
    btn.disabled = false;
    btn.textContent = "\u27b3 Detect";
  }
});



// ── Appearance: queue row colors ──
// Lives with the other theme controls, not in Auto-tune: it is a purely visual
// preference and the hardware scan has no opinion on it. Still written through
// memory_profile_apply because that is the one command that writes wgp_config
// memory keys (backend validates the value fail-closed to pastel|grey).
async function queueColorsLoad() {
  const sel = $("queueColorsSelect");
  const status = $("queueColorsStatus");
  if (!sel) return;
  try {
    const res = await window.w2gp.memoryProfileRead();
    const v = res && res.ok && res.settings && res.settings.queue_color_scheme;
    sel.value = v === "grey" ? "grey" : "pastel";
  } catch {
    if (status) status.textContent = "could not read";
  }
}
$("queueColorsSelect")?.addEventListener("change", async (e) => {
  const sel = e.currentTarget;
  const status = $("queueColorsStatus");
  const val = sel.value;
  sel.disabled = true;
  if (status) status.textContent = "saving…";
  try {
    const r = await window.w2gp.memoryProfileApply({ queue_color_scheme: val });
    if (status) {
      status.textContent =
        r && r.success ? "saved ✓" : "✗ " + ((r && r.error) || "save failed");
    }
    if (!(r && r.success)) sel.value = "pastel";
  } catch (err) {
    if (status) status.textContent = "✗ " + errText(err);
    sel.value = "pastel";
  } finally {
    sel.disabled = false;
  }
});

// ── Auto-Tune: failsafe toggle → re-render recommendation live ──
$("autotuneFailsafeChk").addEventListener("change", async () => {
  const status = $("autotuneStatus");
  if (!_autotuneHardware) {
    // Nothing detected yet — tell the user Detect will honor it.
    status.className = "";
    status.style.background = "var(--bg-tertiary)";
    status.textContent = "";
    {
      const on = $("autotuneFailsafeChk").checked;
      const lead = document.createElement("span");
      lead.textContent = on
        ? "⚠️ Failsafe enabled — run "
        : "Failsafe off — run ";
      const st = document.createElement("strong");
      st.textContent = "Detect";
      const tail = document.createElement("span");
      tail.textContent = on ? " to see the P5 recommendation." : " when ready.";
      status.append(lead, st, tail);
    }
    return;
  }
  try {
    const rec = await window.w2gp.autoTuneRecommend(_autotuneHardware, {
      failsafe: $("autotuneFailsafeChk").checked,
    });
    _autotuneRecommendation = rec;
    // Re-seed the editable Adjuster fields with the (P5) recommendation.
    memProfileFromRecommendation(rec);
    status.className = "";
    status.style.background = "var(--bg-tertiary)";
    status.textContent = $("autotuneFailsafeChk").checked
      ? "⚠️ Failsafe mode active — P5 (maximum compatibility) selected. Apply to write it."
      : "ℹ️ Failsafe mode off — standard matrix recommendation restored.";
  } catch (e) {
    status.className = "";
    status.style.background = "#3A1E1E";
    status.textContent =
      "❌ Failsafe toggle failed: " + ((e && e.message) || String(e));
  }
});



// ── Silent settings auto-scan (D1) — runs once at dashboard load ──
async function silentSettingsRepair() {
  try {
    const r = await window.w2gp.repairSettings();
    if (!r || !r.success) {
      if (r && r.error) appendLog("[i] Settings auto-scan skipped: " + r.error);
      return;
    }
    const modelFixed =
      r.modelPaths && r.modelPaths.fixed && r.modelPaths.replacements.length;
    if (r.fixed > 0 || modelFixed) {
      if (r.fixed > 0) {
        appendLog(
          `[✓] Auto-repaired ${r.fixed} out-of-range setting value(s) (${r.scanned} file(s) scanned).`,
        );
        showToast("✓ Auto-repaired " + r.fixed + " setting value(s)");
      }
      if (modelFixed) {
        appendLog(
          `[✓] Fixed ${r.modelPaths.replacements.length} nested model path(s) in wgp_config.json (issue #18 class).`,
        );
        r.modelPaths.replacements.forEach((x) =>
          appendLog("[✓]   " + x.key + ": " + x.from + " → " + x.to),
        );
        if (r.fixed === 0) showToast("✓ Fixed nested model paths");
      }
    }
    // Quiet when nothing was wrong — auto-scan must never nag.
  } catch {}
}

// ── uv Wheel Cache (Manage → General) ──
function fmtBytes(n) {
  if (!n && n !== 0) return "—";
  if (!n) return "0 B";
  const u = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(n) / Math.log(1024));
  return (n / 1024 ** i).toFixed(i ? 1 : 0) + " " + u[i];
}
// Cheap: only reports presence on Manage-panel open (no directory walk).
async function refreshUvCacheInfo() {
  const statusEl = $("uvCacheStatus");
  if (!statusEl) return;
  try {
    const info = await window.w2gp.uvCacheInfo();
    if (info && info.exists) {
      statusEl.textContent = `Cache present at ${info.cacheDir} — size on demand.`;
    } else {
      statusEl.textContent =
        "No cache folder present (fresh install or already removed).";
    }
  } catch {
    statusEl.textContent = "Could not read cache info.";
  }
}
// On-demand: computes the byte count only when the user asks.
async function showUvCacheSize() {
  const statusEl = $("uvCacheStatus");
  if (!statusEl) return;
  statusEl.textContent = "Calculating size…";
  try {
    const info = await window.w2gp.uvCacheSize();
    if (info && info.exists) {
      statusEl.textContent = `Cache size: ${fmtBytes(info.sizeBytes)} at ${info.cacheDir}`;
    } else {
      statusEl.textContent = "No cache folder present.";
    }
  } catch {
    statusEl.textContent = "Could not read cache size.";
  }
}
$("uvCacheSizeBtn")?.addEventListener("click", showUvCacheSize);
$("uvCachePurgeBtn")?.addEventListener("click", async function () {
  this.disabled = true;
  const resEl = $("uvCacheResult");
  if (resEl) resEl.textContent = "Purging unused wheels…";
  try {
    const r = await window.w2gp.uvCacheClean("prune");
    if (resEl)
      resEl.textContent =
        r && r.success
          ? "Purge done — see log for details."
          : "Purge skipped — " + ((r && r.error) || "unknown error");
  } catch (e) {
    if (resEl) resEl.textContent = "Error: " + e;
  }
  this.disabled = false;
  refreshUvCacheInfo();
});
$("uvCacheRemoveBtn")?.addEventListener("click", async function () {
  if (
    !confirm(
      "Remove the entire uv wheel cache? Next Wan2GP update will re-download everything (one-time).",
    )
  )
    return;
  this.disabled = true;
  const resEl = $("uvCacheResult");
  if (resEl) resEl.textContent = "Removing cache…";
  try {
    const r = await window.w2gp.uvCacheClean("remove");
    if (resEl)
      resEl.textContent =
        r && r.success
          ? r.removed
            ? "Cache removed."
            : "No cache to remove."
          : "Remove failed — " + ((r && r.error) || "unknown error");
  } catch (e) {
    if (resEl) resEl.textContent = "Error: " + e;
  }
  this.disabled = false;
  refreshUvCacheInfo();
});

// ── Repair Settings (Manage → General) — fixes "Value: N is not in the list of choices" ──

$("repairSettingsBtn")?.addEventListener("click", async function () {
  this.disabled = true;
  this.textContent = "Scanning...";
  appendLog("[*] Scanning settings files for out-of-range values...");
  try {
    const r = await window.w2gp.repairSettings();
    if (r && r.success) {
      if (r.fixed > 0) {
        appendLog(
          `[✓] Repaired ${r.fixed} out-of-range value(s) across ${r.scanned} settings file(s).`,
        );
        r.results
          .filter((x) => x.fixed)
          .forEach((x) =>
            appendLog(
              "[✓]   " +
                x.file +
                " — " +
                x.fixed +
                " fixed (backup: " +
                x.backup +
                ")",
            ),
          );
        showToast("✓ Settings repaired (" + r.fixed + " values)");
      } else {
        appendLog(
          `[i] No problems found — scanned ${r.scanned} settings file(s).`,
        );
        showToast("✓ Settings OK — nothing to repair");
      }
      if (r.problems && r.problems.length) {
        appendLog("[!] Could not read some files (skipped):");
        r.problems.forEach((p) =>
          appendLog("[!]   " + p.file + " — " + p.error),
        );
      }
      if (
        r.modelPaths &&
        r.modelPaths.fixed &&
        r.modelPaths.replacements.length
      ) {
        appendLog(
          `[✓] Fixed ${r.modelPaths.replacements.length} nested model path(s) in wgp_config.json:`,
        );
        r.modelPaths.replacements.forEach((x) =>
          appendLog("[✓]   " + x.key + ": " + x.from + " → " + x.to),
        );
        showToast("✓ Model paths repaired");
      }
    } else {
      appendLog("[!] " + ((r && r.error) || "Repair failed"));
      showToast("✗ " + ((r && r.error) || "Repair failed"));
    }
  } catch (e) {
    appendLog("[!] Repair error: " + e.message);
    showToast("✗ " + e.message);
  } finally {
    this.disabled = false;
    this.textContent = "Scan & Repair Settings";
  }
});

// ── Report an issue (Manage → About) — bundles diagnostics + prefills GitHub issue ──
$("reportIssueBtn")?.addEventListener("click", async function () {
  this.disabled = true;
  this.textContent = "Bundling diagnostics...";
  appendLog("[*] Gathering diagnostics...");
  try {
    const r = await window.w2gp.reportIssue();
    if (r && r.success) {
      appendLog(
        "[✓] Diagnostic bundle created (" +
          r.logLines +
          " log lines" +
          (r.hadErrorQueue ? ", crash diagnostics included" : "") +
          ").",
      );
      appendLog("[✓] Bundle: " + (r.zipPath || r.bundleDir));
      appendLog(
        "[i] A GitHub issue has been opened pre-filled with your system info — attach the bundle zip to it.",
      );
      showToast("✓ Diagnostics bundled — issue opened");
    } else {
      appendLog("[!] " + ((r && r.error) || "Failed to create diagnostics"));
      showToast("✗ " + ((r && r.error) || "Failed to create diagnostics"));
    }
  } catch (e) {
    appendLog("[!] Report-issue error: " + e.message);
    showToast("✗ " + e.message);
  } finally {
    this.disabled = false;
    this.textContent = "🐞 Report an issue…";
  }
});

// ── Manage → Updates tab — proxies to same IPCs as Dashboard ──
$("manageUpdateWan2gpBtn")?.addEventListener("click", async function () {
  const s = $("manageUpdateWan2gpStatus");
  this.disabled = true;
  this.textContent = "Updating...";
  if (s) s.textContent = "Updating Wan2GP (this can take a few minutes)...";
  try {
    const r = await window.w2gp.update();
    if (s)
      s.textContent = !r
        ? "✗ Update failed"
        : r.updated === false
          ? "✓ Already up to date — nothing to update"
          : "✓ Update finished — check Dashboard log";
  } catch (e) {
    if (s) s.textContent = "✗ " + (e.message || e);
  } finally {
    this.disabled = false;
    this.textContent = "↻ Update Wan2GP";
  }
});
$("manageUpdateDesktopBtn")?.addEventListener("click", (e) => {
  const s = $("manageUpdateDesktopStatus");
  if (s) s.textContent = "Checking...";
  window.w2gp.checkUpdate(e.shiftKey ? { local: true } : undefined);
  setTimeout(() => {
    if (s && !s.textContent.includes("✓"))
      s.textContent = "Check sent — see banner on Dashboard";
  }, 1500);
});





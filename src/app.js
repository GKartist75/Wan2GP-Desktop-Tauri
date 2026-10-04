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



// ── Periodic Wan2GP update check ──
// Re-polls the upstream commit list every 30 min while the app is open so an
// update released mid-session still flags the green dot + changelog without a
// manual refresh. Silent re-check (no loading flash); the GitHub cache in
// main.js keeps this off the rate-limit radar. Skips while the dashboard is
// hidden (user is in the webview / embedded browser).
const WANGP_POLL_MS = 30 * 60 * 1000;
const DESKTOP_POLL_MS = 5 * 60 * 60 * 1000;
// Deepy Web self-healing poll (15s): the card otherwise only refreshes on
// user actions, so a slow boot that binds the port late — or a process
// started/stopped outside the card — leaves Start/Stop lying until the next
// click. Light status only (no Tailscale probe); skipped while a start flow
// owns the card.
function startDeepyWebPolling() {
  if (window.__deepyWebPollTimer) clearInterval(window.__deepyWebPollTimer);
  const poll = () => {
    if (document.hidden) return;
    const dash = $("dashBody");
    if (dash && dash.style.display === "none") return;
    if (!$("deepyWebCard")) return;
    if (window.__deepyWebStarting) return; // start flow owns the card
    if (window.__deepyWebPollBusy) return;
    window.__deepyWebPollBusy = true;
    refreshDeepyWeb(true)
      .catch(() => {})
      .finally(() => {
        window.__deepyWebPollBusy = false;
      });
  };
  window.__deepyWebPollTimer = setInterval(poll, 15000);
}
function startWangpPolling() {
  if (window.__wangpPollTimer) clearInterval(window.__wangpPollTimer);
  const poll = () => {
    const dash = $("dashBody");
    if (dash && dash.style.display === "none") return;
    loadWangpChangelog(false);
  };
  poll(); // immediate tick on (re)start
  window.__wangpPollTimer = setInterval(poll, WANGP_POLL_MS);
  // Same visibility pause/resume as startMetricsPolling.
  if (!window.__wangpVisBound) {
    window.__wangpVisBound = () => {
      if (document.hidden) {
        if (window.__wangpPollTimer) {
          clearInterval(window.__wangpPollTimer);
          window.__wangpPollTimer = null;
        }
      } else if (!window.__wangpPollTimer) {
        startWangpPolling();
      }
    };
    document.addEventListener("visibilitychange", window.__wangpVisBound);
  }
}
function startDesktopPolling() {
  if (window.__desktopPollTimer) clearInterval(window.__desktopPollTimer);
  const poll = () => {
    const dash = $("dashBody");
    if (dash && dash.style.display === "none") return;
    try {
      window.w2gp.checkUpdate();
    } catch {}
  };
  window.__desktopPollTimer = setInterval(poll, DESKTOP_POLL_MS);
  // One early check shortly after boot — the 5h interval alone means a fresh
  // release sits unknown for hours (seen with v0.1.3). Slightly delayed (not
  // immediate) so backend/network are up and the boot sequence stays
  // undisturbed; this timer is the only boot-time self-check.
  if (!window.__desktopBootCheckDone) {
    window.__desktopBootCheckDone = true;
    setTimeout(poll, 8000);
  }
  if (!window.__desktopVisBound) {
    window.__desktopVisBound = () => {
      if (document.hidden) {
        if (window.__desktopPollTimer) {
          clearInterval(window.__desktopPollTimer);
          window.__desktopPollTimer = null;
        }
      } else if (!window.__desktopPollTimer) {
        startDesktopPolling();
      }
    };
    document.addEventListener("visibilitychange", window.__desktopVisBound);
  }
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

// ── Download Save / Save-As prompt ──
// WebView2 completes iframe downloads with zero UI (no shelf, toast or
// dialog), so saves look broken while files pile up in Downloads.
// While the Desktop view is open, poll Downloads for Wan2GP-issued files
// and pop a browser-like prompt per arrival: [Save] keeps it in
// Downloads, [Save As…] opens the native dialog and moves it (dialog
// reopens at the last-used folder). Covers gallery media, settings
// .zip/.json, queue .zip, finetune exports, right-click saves — all share
// the same anchor-download path. Other apps' files never prompt (shape
// gate mirrors the backend). Baseline resets whenever the view is
// hidden so old files never announce themselves.
let _dlWatchTimer = null;
const _dlWatchSeen = new Set();
let _dlWatchBaseline = 0;
// Wan2GP shapes only: bundles (.zip/.json/.lset) always prompt, media must
// carry Wan2GP's timestamp (-YYYY-MM-DD-HHhMMmSSs) or _seed marker.
// Skip in-progress/partial artifacts and OS noise.
const DL_BUNDLE_RE = /\.(zip|json|lset)$/i;
const DL_STAMP_RE = /[-_]\d{4}-\d{2}-\d{2}-\d{2}h\d{2}m\d{2}s/i;
const DL_SEED_RE = /_seed\d/i;
const DL_SKIP_RE =
  /\.(tmp|temp|crdownload|part|partial|download|opdownload|lock|bak|lnk)$/i;
function isWangpDownload(name) {
  if (
    !name ||
    name.startsWith(".") ||
    name.startsWith("~$") ||
    DL_SKIP_RE.test(name)
  )
    return false;
  if (DL_BUNDLE_RE.test(name)) return true;
  const stem = name.replace(/ \(\d+\)(?=\.[^.]+$)/, "");
  return DL_STAMP_RE.test(stem) || DL_SEED_RE.test(stem);
}
function dlWatchViewOpen() {
  const host = $("webviewContainer");
  return !!(
    host &&
    !host.classList.contains("hidden") &&
    document.getElementById("tauri-browser-view")
  );
}
function startDownloadsWatch() {
  if (_dlWatchTimer) return;
  _dlWatchSeen.clear();
  _dlWatchBaseline = Date.now();
  // Native child embed: downloads arrive as exact `download-finished` events
  // (no polling, no shape-gate — every event IS a Wan2GP download). Register once.
  if (!startDownloadsWatch._nativeWired) {
    startDownloadsWatch._nativeWired = true;
    // Native child, browser-with-ask flow: toast while bytes land in staging,
    // then the native Save-As dialog pops on finish (cancel keeps Downloads).
    try {
      window.w2gp.onDownloadStarted((p) => {
        if (p && p.name)
          showToast(
            "⬇ Downloading: " + p.name + (p.dlId ? " (#" + p.dlId + ")" : ""),
          );
      });
    } catch {}
    try {
      window.w2gp.onDownloadFinished((p) => {
        // Log FIRST, dedup second: a suppressed duplicate must leave a
        // trace (#14 one-shot was this silent return — the staged path
        // gets reused once Save-As moves the file away). Keyed on the
        // backend dlId nonce, never on the reusable path.
        if (!p || !p.path) {
          appendLog("[dl] finish event with no path — dropped.");
          return;
        }
        const tag = p.dlId ? " (#" + p.dlId + ")" : "";
        if (p.success === false) {
          appendLog(
            "[dl] download FAILED" +
              tag +
              ": " +
              (p.name || p.url || "unknown"),
          );
          showToast("✗ Download failed: " + (p.name || p.url || "unknown"));
          return;
        }
        appendLog("[*] Download finished" + tag + ": " + (p.name || p.path));
        const key = "dl#" + (p.dlId ?? "staged|" + p.path);
        if (_dlWatchSeen.has(key)) {
          appendLog("[dl] duplicate finish suppressed: " + key);
          return;
        }
        _dlWatchSeen.add(key);
        finishNativeDownload(p);
      });
    } catch {}
    try {
      window.w2gp.onGradioPageLoad((p) => {
        if (!p) return;
        appendLog(
          `[embed] Gradio page ${p.started ? "started" : "finished"}: ${p.url || ""}`,
        );
      });
    } catch {}
  }
  _dlWatchTimer = setInterval(async () => {
    try {
      // Native mode is event-driven — keep the iframe baseline fresh so
      // switching back to iframe never replays old files.
      if (window.w2gp.isNativeEmbed && window.w2gp.isNativeEmbed()) {
        _dlWatchBaseline = Date.now();
        return;
      }
      if (!dlWatchViewOpen()) {
        _dlWatchSeen.clear();
        _dlWatchBaseline = Date.now();
        return;
      }
      const files = await window.w2gp.downloadsSince(_dlWatchBaseline);
      for (const f of files || []) {
        const key = (f.name || "") + "|" + (f.ms || 0);
        // NB: no logging on the duplicate path — downloadsSince
        // re-reports every file newer than baseline on EVERY poll, so a
        // log here would spam every 4s. New arrivals log below.
        if (!f.name || _dlWatchSeen.has(key)) continue;
        _dlWatchSeen.add(key);
        if (!isWangpDownload(f.name)) continue;
        appendLog("[*] Download detected: " + f.name);
        showDownloadPrompt(f.name);
      }
    } catch {}
  }, 4000);
}

// Browser-with-ask finish for native downloads: staged file → native Save-As
// dialog (remembers last folder). Cancel keeps it in Downloads. The iframe
// path keeps using showDownloadPrompt (name-only poll).
async function finishNativeDownload(p) {
  try {
    let lastDir = null;
    try {
      lastDir = localStorage.getItem("w2gp.saveAsDir");
    } catch {}
    const r = await window.w2gp.saveStagedDownload(p.path, lastDir);
    if (r && (r.ok || r.success) && r.path) {
      try {
        const slash = Math.max(
          r.path.lastIndexOf("/"),
          r.path.lastIndexOf("\\"),
        );
        if (slash > 0)
          localStorage.setItem("w2gp.saveAsDir", r.path.slice(0, slash));
      } catch {}
      if (r.cancelled) showToast("Kept in Downloads: " + (r.name || ""));
      else {
        showToast("✓ Saved to: " + r.path);
        appendLog("[*] Saved to: " + r.path);
      }
    } else showToast("✗ Save failed: " + ((r && r.error) || "unknown"));
  } catch (e) {
    showToast("✗ " + errText(e));
  }
}
// Browser-like arrival prompt: Save (keep in Downloads) vs Save As… (move
// via native dialog). Remembers the last Save-As folder for the session.
function showDownloadPrompt(fname) {
  const t = document.createElement("div");
  t.setAttribute("role", "status");
  t.setAttribute("aria-live", "polite");
  t.style.cssText =
    "position:fixed;bottom:20px;left:50%;transform:translateX(-50%);background:#333;color:#e8e6e1;padding:8px 12px;border-radius:6px;font-size:13px;z-index:9999;font-family:Geist Mono,monospace;display:flex;gap:8px;align-items:center;max-width:90vw";
  const label = document.createElement("span");
  label.textContent = "⬇ " + fname;
  label.style.cssText =
    "overflow:hidden;text-overflow:ellipsis;white-space:nowrap;max-width:46vw";
  const mkBtn = (text, title) => {
    const b = document.createElement("button");
    b.textContent = text;
    b.title = title;
    b.style.cssText =
      "background:#4a4a4a;color:#fff;border:1px solid #666;border-radius:4px;padding:3px 10px;font-size:12px;cursor:pointer;font-family:inherit";
    return b;
  };
  const saveBtn = mkBtn("Save", "Keep it in your Downloads folder");
  const saveAsBtn = mkBtn("Save As…", "Choose where to save it");
  t.append(label, saveBtn, saveAsBtn);
  document.body.appendChild(t);
  let gone = false;
  const dismiss = () => {
    if (gone) return;
    gone = true;
    t.style.opacity = "0";
    t.style.transition = "opacity 0.3s";
    setTimeout(() => t.remove(), 400);
  };
  saveBtn.addEventListener("click", () => {
    dismiss();
    showToast("✓ Saved to Downloads: " + fname);
  });
  saveAsBtn.addEventListener("click", async () => {
    dismiss();
    try {
      let lastDir = null;
      try {
        lastDir = localStorage.getItem("w2gp.saveAsDir");
      } catch {}
      const r = await window.w2gp.saveDownloadedFile(fname, lastDir);
      if (r && r.cancelled) {
        showToast("Kept in Downloads: " + fname);
        return;
      }
      if (r && (r.ok || r.success) && r.path) {
        try {
          const slash = Math.max(
            r.path.lastIndexOf("/"),
            r.path.lastIndexOf("\\"),
          );
          if (slash > 0)
            localStorage.setItem("w2gp.saveAsDir", r.path.slice(0, slash));
        } catch {}
        showToast("✓ Saved to: " + r.path);
      } else showToast("✗ Save failed: " + ((r && r.error) || "unknown"));
    } catch (e) {
      showToast("✗ " + errText(e));
    }
  });
  setTimeout(dismiss, 30000);
}

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

// ── Reset UI when server exits (manual stop or crash) ──
// Separate term-window wiring (native floating): X-close sync + dock routing.
try {
  window.w2gp.onTermClosed(() => {
    _termWinOpen = false;
    try {
      if (currentDock() === "floating") _ftVisible = false;
    } catch {}
  });
} catch {}
try {
  window.w2gp.onTermSetDock((d) => {
    try {
      window.__dockTerminal(d);
    } catch {}
  });
} catch {}
window.w2gp.onAppClosing(() => {
  // Ordered close: the window stays alive while the backend stops
  // sessions first — tell the user why close takes a few seconds.
  appendLog("[*] Closing — stopping Wan2GP sessions first…");
  try {
    showToast("Stopping sessions before exit…");
  } catch {}
});
window.w2gp.onWangpExit(async (c) => {
  if (c && typeof c === "object" && c.source === "deepy") return;
  // Payload shapes: {code: n|null} on process end, {stopped:true} on manual stop.
  // (Was interpolating the whole object → "exited (code [object Object])".)
  const code =
    c && typeof c === "object" ? (c.code ?? (c.stopped ? 0 : "?")) : c;
  const manualStop = _expectServerExit;
  _expectServerExit = false;
  if (_expectServerExitTimer) {
    clearTimeout(_expectServerExitTimer);
    _expectServerExitTimer = null;
  }
  if (manualStop && code !== 0) appendLog("[*] Wan2GP server stopped.");
  else
    appendLog(
      `${code === 0 ? "[*]" : "[!]"} Wan2GP process exited (code ${code})`,
    );
  const exitMode = serverMode; // capture before teardown below nulls it
  _pendingOpen = null; // boot failed/went away — don't open anything later
  // Server is really gone: drop the (now stale) embed entirely so the next
  // open rebuilds it instead of showing a dead page.
  window.w2gp.destroyBrowserView().catch(() => {});
  if (serverMode === "app") {
    if (!$("webviewContainer").classList.contains("hidden")) {
      // closeWebview skips when a transition is in flight — wait for it
      // first so a real exit can never strand visible view controls.
      await awaitViewFree(20000);
      closeWebview(true);
    }
  } else if (serverMode === "browser") {
    hideBrowserRunningUI();
    resetBrowserLaunchUI();
  }
  appRunning = false;
  setAppLaunchLabel();
  updateLed("stopped");
  updateFtStatus("stopped");
  try { refreshMainLan(); } catch {}
  // Config-skew recovery: wgp.py died with KeyError on a settings key
  // (partial write after a failed install, or an ancient config after an
  // update). Crashes only — never for a manual Stop, which also exits
  // non-zero when taskkill does the killing.
  if (
    !manualStop &&
    code !== 0 &&
    code !== "?" &&
    !window._configCrashOffered
  ) {
    try {
      const tail =
        typeof window._getLogTail === "function" ? window._getLogTail() : "";
      const m = tail.match(/KeyError:\s*'([^']+)'/);
      if (m && /wgp\.py/.test(tail)) {
        window._configCrashOffered = true;
        offerConfigReset(m[1], exitMode);
      }
    } catch {}
  }
});
window.w2gp.onDeepyExit(async (c) => {
    const code = c && typeof c === "object" ? (c.code ?? "?") : c;
  appendLog(`[*] Deepy Web stopped (code ${code}).`);
  try { refreshDeepyWeb(); } catch {}
  try { refreshMainLan(); } catch {}
  try { updateDeepyWebLed(false); } catch {}
});

async function offerConfigReset(missingKey, mode) {
  appendLog(
    `[!] Wan2GP crashed: settings file is missing '${missingKey}' (outdated or partial wgp_config.json).`,
  );
  showToast(`✗ Settings missing '${missingKey}' — reset offered`);
  const choice = await window.w2gp.confirmDialog({
    title: "Settings file outdated?",
    message: `Wan2GP crashed because wgp_config.json is missing '${missingKey}'.`,
    detail:
      "Back it up and reset to defaults? Wan2GP regenerates the full file on next launch (models stay where they are).",
  });
  window._configCrashOffered = false;
  if (choice !== "ok") return;
  try {
    const r = await window.w2gp.resetWgpConfig();
    if (r && (r.success || r.ok)) {
      appendLog(
        "[*] Settings backed up to " +
          (r.backup || "wgp_config.bak-*.json") +
          " — relaunching with fresh defaults…",
      );
      showToast("✓ Settings reset — relaunching");
      setTimeout(() => {
        // Reuse the dashboard buttons' full logic (validation, boot flow).
        const b = mode === "browser" ? $("browserBtn") : $("appBtn");
        if (b && !b.disabled) b.click();
        else showToast("Press Launch to start Wan2GP with fresh settings");
      }, 800);
    } else showToast("✗ Reset failed: " + ((r && r.error) || "unknown"));
  } catch (e) {
    showToast("✗ " + errText(e));
  }
}

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
      appendLog("[*] Requirements restored.");
      hideDriftBanner();
      setTimeout(refreshDashboard, 2000);
    } else showToast("✗ Restore failed: " + ((rr && rr.error) || "unknown"));
  } catch (e) {
    showToast("✗ " + errText(e));
  }
  if (btn) btn.disabled = false;
});
$("driftDismissBtn")?.addEventListener("click", hideDriftBanner);


// ── Quick pip install ──
// Accept either a bare spec (claude-agent-sdk==0.1.66) or a full command
// (pip install claude-agent-sdk==0.1.66) pasted by the user — strip any leading
// pip invocation so both the preview and the real install behave identically.
// (Renderer is a plain browser script — no require — so this is inlined; the
// Node-side mirror lives in services/normalize-pip-spec.js for unit tests.)
function normalizePipSpec(raw) {
  let s = (raw || "").trim();
  const m = s.match(/^(?:py(?:thon)?\s+-m\s+)?pip3?\s+install\s+/i);
  if (m) s = s.slice(m[0].length).trim();
  // Strip pip flags (`pip install foo --upgrade` → `foo`). UX only — the
  // backend re-validates. Must match services/normalize-pip-spec.js.
  s = s
    .split(/\s+/)
    .filter((t) => !t.startsWith("-"))
    .join(" ");
  return s;
}
$("pipInstallBtn").addEventListener("click", async () => {
  const input = $("pipInput");
  // Quick-box trap: users paste upstream's `pip install -r requirements.txt`
  // here. normalizePipSpec strips `-r`, leaving the literal file name as a
  // package spec. Catch file/flag input BEFORE normalizing and redirect to
  // the restore button instead of sending `pip install requirements.txt`.
  const rawPip = input?.value || "";
  const lowPip = rawPip.toLowerCase();
  const pipTokens = lowPip.split(/\s+/).filter(Boolean);
  if (
    /(^|\s)pip\s+install\s+-r/i.test(rawPip) ||
    pipTokens.some((t) => t === "-r" || t === "-e" || t === "-c") ||
    pipTokens.some((t) => t.startsWith("-")) ||
    lowPip.includes("requirements.txt") ||
    /--[a-z0-9]/i.test(rawPip)
  ) {
    showToast(
      "That box installs single packages only (e.g. mmgp==3.8.0). For requirements.txt use the restore button.",
    );
    if (input) input.disabled = false;
    $("pipInstallBtn").disabled = false;
    $("pipInstallBtn").textContent = "pip install";
    return;
  }
  const pkg = normalizePipSpec(input?.value);
  if (!pkg) return;
  input.disabled = true;
  $("pipInstallBtn").disabled = true;
  $("pipInstallBtn").textContent = "installing...";
  const r = await window.w2gp.installPackage(pkg);
  input.disabled = false;
  $("pipInstallBtn").disabled = false;
  $("pipInstallBtn").textContent = "pip install";
  if (r && r.success) {
    input.value = "";
    showToast("✓ " + pkg + " installed");
    refreshDashboard();
  } else {
    showToast("✗ " + (r && r.error ? r.error : "install failed"));
  }
});
$("pipInput").addEventListener("keydown", (e) => {
  if (e.key === "Enter") $("pipInstallBtn").click();
});

// Live, copyable preview of the exact command the Advanced box will run.
// Mirrors the launcher's guard: a valid spec shows `pip install <spec>`; an
// invalid one shows the reason it would be blocked (no misleading command).
function updatePipCmdPreview() {
  const input = $("pipInput");
  const preview = $("pipCmdPreview");
  const text = $("pipCmdText");
  if (!input || !preview || !text) return;
  const spec = normalizePipSpec(input.value);
  if (!spec) {
    preview.style.display = "none";
    return;
  }
  // Single source of truth: the same validator the backend enforces
  // (services/pip-spec.js, exposed as window.PipSpec by the script tag in
  // index.html). No inline copy — the old one wrongly blocked `<>` (valid
  // PEP 440 operators), so `foo>=1.0` previewed as blocked but installed fine.
  const check = window.PipSpec
    ? window.PipSpec.assertSafePipSpec(spec)
    : { ok: false, reason: "validator missing" };
  if (check.ok) {
    preview.style.display = "flex";
    preview.classList.remove("pip-cmd-bad");
    text.textContent = "pip install " + spec + "   (runs in the active env)";
  } else {
    preview.style.display = "flex";
    preview.classList.add("pip-cmd-bad");
    text.textContent = "✗ Blocked: " + (check.reason || "invalid spec");
  }
}
$("pipInput").addEventListener("input", updatePipCmdPreview);
$("pipCmdCopy")?.addEventListener("click", async () => {
  const t = $("pipCmdText")?.textContent || "";
  if (!t.startsWith("pip install")) return;
  try {
    await navigator.clipboard.writeText(t.split("   (")[0]);
    $("pipCmdCopy").textContent = "copied!";
    setTimeout(() => {
      $("pipCmdCopy").textContent = "copy";
    }, 1200);
  } catch {}
});
// Clear the preview after a successful install so it doesn't linger.
const _pipInstallOrig = $("pipInstallBtn");
if (_pipInstallOrig) {
  _pipInstallOrig.addEventListener("click", () => {
    setTimeout(updatePipCmdPreview, 50);
  });
}



// Deepy Prime activation panel: pick a ready engine, write it into
// wgp_config.json so the next Wan2GP launch boots with Deepy Prime enabled.
const DEEPY_PANEL_ENGINES = [
  { id: "opencode", label: "OpenCode", paid: false },
  { id: "claude-code", label: "Claude Code", paid: true },
  { id: "codex", label: "OpenAI Codex", paid: true },
  // ponytail: b71026f — local Prime runs on Qwen3.8 VL 9B/27B (needs the 9B or 27B model + GGUF 1.0.25; backend auto-raises 32k context + Summarize)
  { id: "local-qwen38", label: "Qwen3.8 VL 9B/27B (local)", paid: false },
];

// Local-model (Prompt Enhancer) choices shown in the Deepy panel when Deepy is
// Disabled or Zero.
// modes: which Deepy modes the option is valid for. All options are rendered in
// the UI (the non-applicable ones are shown disabled with an annotation), so
// the user sees the full set of possible local models.
const DEEPY_PANEL_ENHANCERS = [
  { id: 1, label: "Florence 2 + Llama 3.2 3B (local)", modes: ["disabled", "zero"] },
  { id: 2, label: "Florence 2 + Llama Joy 8B (local)", modes: ["disabled", "zero"] },
  {
    id: 3,
    label: "Qwen3.5 VL Abliterated 4B (local, recommended)",
    modes: ["disabled", "zero"],
  },
  { id: 4, label: "Qwen3.5 VL Abliterated 9B (local)", modes: ["disabled", "zero"] },
  { id: 5, label: "Qwen3.8 VL Uncensored 27B (local)", modes: ["disabled", "zero"] },
  { id: 6, label: "Qwen3.8 VL Uncensored 9B (local, Prime-ready ~6.5GB)", modes: ["disabled", "zero"] },
];

// Qwen LLM Quantization choices per local engine id — mirrors upstream's
// "Qwen LLM Quantization" dropdown (shared/prompt_enhancer/qwen35_vl.py
// get_qwen35_quantization). 27B Bonsai PTQ1 (gguf_ptq1) is the ~10 GB VRAM
// checkpoint; 9B Heretic offers GGUF Q4 / Q8; Qwen3.5 offers Quanto Int8 or GGUF Q4.
const DEEPY_QUANT_CHOICES = {
  5: [
    { id: "gguf", label: "GGUF Q4 (default, highest quality)" },
    { id: "gguf_q3", label: "GGUF IQ3_S (middle, 16 GB VRAM)" },
    { id: "gguf_q2", label: "GGUF Q2 (lowest memory)" },
    { id: "gguf_ptq1", label: "Bonsai PTQ1 (~10 GB VRAM, needs kernels 1.0.25+)" },
  ],
  6: [
    { id: "gguf", label: "GGUF Q4 (~6.5 GB VRAM, default)" },
    { id: "gguf_q8", label: "GGUF Q8 (~11 GB VRAM, closest to full precision)" },
  ],
  4: [
    { id: "quanto_int8", label: "Quanto Int8 (recommended, better quality)" },
    { id: "gguf", label: "GGUF Q4 (less VRAM, needs kernels)" },
  ],
  3: [
    { id: "quanto_int8", label: "Quanto Int8 (recommended, better quality)" },
    { id: "gguf", label: "GGUF Q4 (less VRAM, needs kernels)" },
  ],
};
const DEEPY_QUANT_DEFAULT = { 6: "gguf", 5: "gguf", 4: "quanto_int8", 3: "quanto_int8" };
// Prime-local backend pick ("<enhancerId>:<quant>", e.g. "6:gguf_q8"). Null =
// derive from the saved config (gguf_q8 or saved enhancer 6 → 9B Q4, Bonsai /
// saved enhancer 5 → 27B entry, else 27B Q4); a manual pick sticks until the
// saved values change (e.g. after Apply).
let _primeBackend = null;
let _primeBackendCtx = "";
function primeBackendFor(savedQuant, savedEnh) {
  const key = (savedQuant || "") + "|" + (savedEnh ?? "");
  const valid = (v) => ["5:gguf", "5:gguf_q3", "5:gguf_q2", "5:gguf_ptq1", "6:gguf", "6:gguf_q8"].includes(v);
  if (_primeBackend === null || _primeBackendCtx !== key || !valid(_primeBackend)) {
    if (savedQuant === "gguf_q8") _primeBackend = "6:gguf_q8";
    else if (savedQuant === "gguf_q3") _primeBackend = "5:gguf_q3";
    else if (savedQuant === "gguf_q2") _primeBackend = "5:gguf_q2";
    else if (savedQuant === "gguf_ptq1") _primeBackend = "5:gguf_ptq1";
    else if (Number(savedEnh) === 6) _primeBackend = "6:gguf";
    else _primeBackend = "5:gguf";
    _primeBackendCtx = key;
  }
  return _primeBackend;
}



// ── C · Console log viewer (own port) ──
let _logServer = { running: false, port: 0, lan: false, samePc: null, phone: null };
let _logServerBusy = false;
async function refreshLogServer() {
  if (!$("logServerCard") || _logServerBusy) return;
  _logServerBusy = true;
  try {
    const [cfg, st] = await Promise.all([
      window.w2gp.configLoad().catch(() => ({})),
      window.w2gp.logServerStatus().catch(() => null),
    ]);
    const running = !!(st && st.running);
    const port = (st && st.port) || (cfg && cfg.logPort) || ((cfg && cfg.serverPort) || 7860) + 2;
    const lan = !!(st && st.lan) || !!(cfg && cfg.logLan);
    const samePc = (st && st.samePc) || ("http://localhost:" + port);
    const phone = (st && st.phone) || null;
    _logServer = { running, port, lan, samePc: running ? samePc : samePc, phone: running ? phone : phone };
    const tgl = $("logServerToggle");
    if (tgl) tgl.checked = running;
    const lanTgl = $("logServerLanToggle");
    if (lanTgl) lanTgl.checked = lan;
    const portInput = $("logServerPortInput");
    if (portInput && !portInput.value) portInput.value = String(port);
    const statusEl = $("logServerStatus");
    if (statusEl) {
      statusEl.textContent = "";
      const dot = document.createElement("span");
      if (running && lan && phone) {
        dot.style.color = "#4ADE80";
        dot.textContent = "● Live on :" + port + " — ";
        statusEl.append(dot, "open a URL below (same Wi-Fi). Read-only tail, refreshes every 2s.");
      } else if (running) {
        dot.style.color = "#4ADE80";
        dot.textContent = "● Running (this PC only) :" + port + " — ";
        statusEl.append(dot, "flip Phone-LAN + Apply to expose it to Wi-Fi.");
      } else {
        statusEl.append("○ Off — flip Logs page on to serve the console on its own port.");
      }
    }
    const setUrlBtn = (el, url, fallbackText) => {
      if (!el) return;
      const has = running && typeof url === "string" && url.length > 0;
      el.textContent = has ? url : fallbackText || "—";
      el.disabled = !has;
    };
    setUrlBtn($("logServerSamePcOpen"), samePc);
    setUrlBtn($("logServerPhoneOpen"), phone, lan ? "start with Phone-LAN on" : "needs Phone-LAN");
  } finally {
    _logServerBusy = false;
  }
}
function startLogServerPolling() {
  if (window.__logServerPollTimer) clearInterval(window.__logServerPollTimer);
  const poll = () => {
    if (document.hidden) return;
    if (!$("logServerCard")) return;
    refreshLogServer().catch(() => {});
  };
  window.__logServerPollTimer = setInterval(poll, 15000);
}
$("logServerToggle")?.addEventListener("change", async (ev) => {
  const on = ev.target.checked;
  try {
    if (on) {
      const lan = !!($("logServerLanToggle") || {}).checked;
      const raw = String(($("logServerPortInput") || {}).value || "").trim();
      const port = raw ? Number(raw) : null;
      const st = await window.w2gp.logServerStart(port, lan);
      appendLog("[*] Log viewer ON :" + (st && st.port) + ((st && st.lan) ? " (LAN)" : " (this PC only)"));
    } else {
      await window.w2gp.logServerStop();
      appendLog("[*] Log viewer OFF.");
    }
  } catch (e) {
    showToast("✗ Log viewer failed: " + errText(e));
    appendLog("[!] Log viewer failed: " + errText(e));
  }
  refreshLogServer().catch(() => {});
});
$("logServerSave")?.addEventListener("click", async () => {
  try {
    const lan = !!($("logServerLanToggle") || {}).checked;
    const raw = String(($("logServerPortInput") || {}).value || "").trim();
    const port = raw ? Number(raw) : null;
    if (_logServer.running) {
      const st = await window.w2gp.logServerStart(port, lan);
      appendLog("[*] Log viewer re-bound :" + (st && st.port));
      showToast("✓ Log viewer on :" + (st && st.port));
    } else {
      const cfg = await window.w2gp.configLoad().catch(() => ({}));
      if (cfg && typeof cfg === "object") {
        if (port) cfg.logPort = port;
        cfg.logLan = lan;
        await window.w2gp.configSave(cfg).catch(() => {});
      }
      showToast("✓ Log viewer settings saved (flip on to start)");
    }
  } catch (e) {
    showToast("✗ Log viewer failed: " + errText(e));
  }
  refreshLogServer().catch(() => {});
});
$("logServerSamePcOpen")?.addEventListener("click", () => {
  if (!_logServer.running || !_logServer.samePc) { showToast("Log viewer is off."); return; }
  deepyWebOpenUrl(_logServer.samePc);
});
$("logServerSamePcCopy")?.addEventListener("click", () => deepyWebCopyUrl(_logServer.samePc));
$("logServerPhoneOpen")?.addEventListener("click", () => {
  if (!_logServer.phone) { showToast("No Phone URL — enable Phone-LAN + Apply."); return; }
  deepyWebOpenUrl(_logServer.phone);
});
$("logServerPhoneCopy")?.addEventListener("click", () => {
  if (!_logServer.phone) { showToast("No Phone URL — enable Phone-LAN + Apply."); return; }
  deepyWebCopyUrl(_logServer.phone);
});
$("logServerPhoneQr")?.addEventListener("click", () => {
  if (!_logServer.phone) { showToast("No Phone URL — enable Phone-LAN + Apply."); return; }
  deepyWebQrUrl(_logServer.phone, "Scan from your phone camera (same Wi-Fi) — read-only console log tail");
});

// ── Installer live downloads (per-file rows from uv output) ──
const _installDl = new Map();
let _installDlDone = 0,
  _installDlPaint = 0,
  _installDlQueued = false;
function installProgressReset() {
  _installDl.clear();
  _installDlDone = 0;
  _installDlPaint = 0;
  _installDlQueued = false;
  const box = $("installProgress");
  if (box) box.style.display = "none";
  const list = $("installProgressList");
  if (list) list.innerHTML = "";
  const cnt = $("installProgressCount");
  if (cnt) cnt.textContent = "";
}
function installProgressOnEvent(d) {
  if (!d || !d.phase) return;
  if (d.phase === "downloading" && d.pkg) {
    const r = _installDl.get(d.pkg) || {
      size: "",
      state: "downloading",
      version: "",
    };
    if (!r.size && d.size) r.size = d.size;
    r.state = "downloading";
    _installDl.set(d.pkg, r);
  } else if (d.phase === "package-installed" && d.pkg) {
    const r = _installDl.get(d.pkg) || {
      size: "",
      state: "installed",
      version: "",
    };
    r.state = "installed";
    if (d.version) r.version = d.version;
    _installDl.set(d.pkg, r);
    _installDlDone++;
  } else if (d.phase === "resolved") {
    // count shown via map size; nothing to store
  } else return;
  // Throttle paints: full innerHTML re-render restarts the bar animation.
  const now = Date.now();
  if (now - _installDlPaint < 500) {
    if (!_installDlQueued) {
      _installDlQueued = true;
      setTimeout(() => {
        _installDlQueued = false;
        installProgressPaint();
      }, 500);
    }
    return;
  }
  installProgressPaint();
}
function installProgressPaint() {
  _installDlPaint = Date.now();
  const box = $("installProgress"),
    list = $("installProgressList");
  if (!box || !list || !_installDl.size) return;
  box.style.display = "";
  const names = [..._installDl.keys()].slice(-40);
  list.textContent = "";
  for (const n of names) {
    const r = _installDl.get(n);
    const row = document.createElement("div");
    row.className = "iprog-row";
    const top = document.createElement("div");
    top.className = "iprog-top";
    const nm = document.createElement("span");
    nm.className = "iprog-name";
    nm.title = n;
    nm.textContent = n;
    const st = document.createElement("span");
    if (r.state === "installed") {
      st.className = "iprog-state installed";
      st.textContent = "✓ " + (r.version || "done");
    } else {
      st.className = "iprog-state downloading";
      st.textContent = "⬇ " + (r.size || "…");
    }
    top.append(nm, st);
    const bar = document.createElement("div");
    bar.className = "iprog-bar" + (r.state === "installed" ? " done" : "");
    bar.append(document.createElement("div"));
    row.append(top, bar);
    list.append(row);
  }
  const cnt = $("installProgressCount");
  if (cnt)
    cnt.textContent = _installDlDone + "/" + _installDl.size + " installed";
}

function dlss5OnEvent(d) {
  if (!d || !d.phase) return;
  if (d.phase === "downloading" && d.pkg && d.pkg !== "other") {
    for (const f of _dlss5Rows)
      if (f.pkg === d.pkg) _dlss5State[f.id] = "downloading";
    _dlss5LastPkg = d.pkg;
  } else if (d.phase === "verified" && _dlss5LastPkg) {
    for (const f of _dlss5Rows)
      if (f.pkg === _dlss5LastPkg) _dlss5State[f.id] = "verified";
  } else if ((d.phase === "installed" || d.phase === "present") && d.path) {
    _dlss5State[String(d.path).replace(/\\/g, "/")] = "verified";
  } else if (d.phase === "done") {
    _dlss5Done = true;
  } else return;
  renderDlss5Progress();
}
$("dlss5ConfirmBtn")?.addEventListener("click", async () => {
  _dlss5LastPkg = null;
  _dlss5State = {};
  _dlss5Done = false;
  renderDlss5Progress();
  $("dlss5Modal").classList.add("hidden");
  const force = !!$("dlss5ForceChk")?.checked;
  const btn = $("dlss5InstallBtn");
  btn.disabled = true;
  const orig = btn.textContent;
  btn.textContent = "Installing…";
  appendLog("[*] Installing DLSS 5 runtime — progress below…");
  try {
    const r = await window.w2gp.installDlss5(force);
    if (r && r.ok)
      showToast(
        r.complete
          ? "✓ DLSS 5 installed — restart Wan2GP"
          : "⏳ " + (r.hint || "Partial install — see console"),
      );
    else showToast("✗ " + ((r && r.error) || "install failed"));
  } catch (e) {
    showToast("✗ " + e.message);
  } finally {
    btn.disabled = false;
    btn.textContent = orig;
    refreshDlss5();
  }
});
for (const id of ["dlss5DocsLink", "dlss5DocsLink2"]) {
  const a = $(id);
  if (a)
    a.onclick = async (ev) => {
      ev.preventDefault();
      await window.w2gp.openExternal(
        "https://github.com/deepbeepmeep/Wan2GP/blob/main/docs/DLSS5.md",
      );
    };
}

$("llmEnginesRefresh")?.addEventListener("click", () => {
  _llmEnginesPromise = null;
  refreshLLMEngines();
});

$("desktopShortcutBtn").addEventListener("click", async function () {
  this.disabled = true;
  this.textContent = "Creating...";
  const r = await window.w2gp.createDesktopShortcut();
  this.disabled = false;
  this.textContent = "Create Desktop Shortcut";
  if (r && r.success) {
    showToast("✓ Shortcut created on desktop: Launch Wan2GP.bat");
  } else {
    showToast("✗ " + (r && r.error ? r.error : "Failed to create shortcut"));
  }
});


// Dock buttons (always visible)
document.querySelectorAll(".dock-btn").forEach((btn) => {
  btn.addEventListener("click", () => setFtDock(btn.dataset.dock));
});
// Events coming from the floating-terminal overlay (its own BrowserView, used for 'floating' dock)
window.w2gp.onTermDockChanged((dock) => {
  const ft = $("floatingTerminal");
  ft.className =
    "floating-term dock-" +
    dock +
    (ft.classList.contains("hidden") ? " hidden" : "");
  if (dock !== "floating") ft.style.cssText = "";
  document
    .querySelectorAll(".dock-btn")
    .forEach((b) => b.classList.toggle("active", b.dataset.dock === dock));
  window.w2gp.bvSetDock(dock);
  if (_ftVisible) showTerminal();
});
window.w2gp.onTermClosed(() => {
  _ftVisible = false;
  hideTerminal();
});
// Floating drag for dock-floating mode
let _fdrag = null;
$("floatingTerminal").addEventListener("mousedown", (e) => {
  if (!$("floatingTerminal").classList.contains("dock-floating")) return;
  if (e.target.closest(".term-btn-small, .dock-menu")) return;
  const r = $("floatingTerminal").getBoundingClientRect();
  _fdrag = {
    dx: e.clientX - r.left,
    dy: e.clientY - r.top,
    w: r.width,
    h: r.height,
  };
  document.addEventListener("mousemove", _fdragMove);
  document.addEventListener("mouseup", _fdragEnd);
});
function _fdragMove(e) {
  if (!_fdrag) return;
  const p = $("floatingTerminal");
  let x = e.clientX - _fdrag.dx,
    y = e.clientY - _fdrag.dy;
  x = Math.max(0, Math.min(x, window.innerWidth - _fdrag.w));
  y = Math.max(0, Math.min(y, window.innerHeight - 30));
  p.style.left = x + "px";
  p.style.top = y + "px";
  p.style.right = "auto";
  p.style.bottom = "auto";
}
function _fdragEnd() {
  _fdrag = null;
  document.removeEventListener("mousemove", _fdragMove);
  document.removeEventListener("mouseup", _fdragEnd);
}
// Follow toggle
$("ftFollowBtn").addEventListener("click", () => {
  termFollow.ftTermBody = !termFollow.ftTermBody;
  const b = $("ftFollowBtn");
  b.classList.toggle("active");
  const ft = b.querySelector(".follow-text");
  if (ft) ft.textContent = termFollow.ftTermBody ? "Follow" : "Paused";
  if (termFollow.ftTermBody) {
    const e = $("ftTermBody");
    if (e) setTimeout(() => (e.scrollTop = e.scrollHeight), 10);
  }
});
// Keyboard shortcut: Ctrl+` toggles floating terminal
// NOTE: the real shortcut lives in the Keyboard-shortcuts handler below
// (Ctrl+` / Escape / Ctrl+W). This duplicate copy fired on the SAME keypress,
// calling toggleFloatingTerm() twice — open then instantly close — so the
// shortcut looked dead in webview mode. Removed to avoid the double toggle.

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

// ── Keyboard shortcuts ──
document.addEventListener("keydown", (e) => {
  if (["INPUT", "TEXTAREA", "SELECT"].includes(e.target.tagName)) return;
  // Escape closes the Manage or Guide panel first — either can be open in
  // webview mode too, where the webview Escape branch below would otherwise fire instead.
  if (e.key === "Escape" && $("settingsPanel").classList.contains("open")) {
    closeSettings();
    return;
  }
  if (
    e.key === "Escape" &&
    $("guidePanel") &&
    $("guidePanel").classList.contains("open") &&
    typeof closeGuide === "function"
  ) {
    closeGuide();
    return;
  }
  // Ctrl+` toggles floating terminal
  if (e.ctrlKey && e.key === "`") {
    e.preventDefault();
    toggleFloatingTerm();
    return;
  }
  // Escape closes the webview/BrowserView
  if (e.key === "Escape" && $("dashBody").style.display === "none") {
    closeWebview();
    return;
  }
  // Ctrl+W closes the webview/BrowserView
  if (
    e.ctrlKey &&
    (e.key === "w" || e.key === "W") &&
    $("dashBody").style.display === "none"
  ) {
    e.preventDefault();
    closeWebview();
  }
});

function showToast(msg, onClick) {
  const t = document.createElement("div");
  t.textContent = msg;
  t.setAttribute("role", "status");
  t.setAttribute("aria-live", "polite");
  t.style.cssText =
    "position:fixed;bottom:20px;left:50%;transform:translateX(-50%);background:#333;color:#e8e6e1;padding:8px 16px;border-radius:6px;font-size:13px;z-index:9999;font-family:Geist Mono,monospace;transition:opacity 0.3s;max-width:90vw;text-align:center";
  document.body.appendChild(t);
  let gone = false;
  const dismiss = () => {
    if (gone) return;
    gone = true;
    t.style.opacity = "0";
    setTimeout(() => t.remove(), 400);
  };
  if (typeof onClick === "function") {
    t.style.cursor = "pointer";
    t.title = "Click to choose where to save it";
    t.addEventListener("click", () => {
      const f = onClick;
      dismiss();
      try {
        f();
      } catch {}
    });
    setTimeout(dismiss, 12000);
  } else {
    setTimeout(dismiss, 2500);
  }
}

$("updateCheckBtn").addEventListener("click", (e) => {
  window.w2gp.checkUpdate(e.shiftKey ? { local: true } : undefined);
});
$("updateDownloadBtn").addEventListener("click", () => {
  window.w2gp.downloadUpdate();
});
$("updateInstallBtn").addEventListener("click", () => {
  window.w2gp.installUpdate();
});
$("updateDismissBtn").addEventListener("click", () => {
  $("updateBanner").classList.add("hidden");
  // Keep the persistent button indicator — the user dismissed the banner, not
  // the fact that an update is still available. It clears on Download/Install.
});





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
      s.textContent = r
        ? "✓ Update finished — check Dashboard log"
        : "✗ Update failed";
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





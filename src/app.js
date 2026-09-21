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
      if (lastLine.trim()) logBuffer.push(lastLine.trim());
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

// ── Floating Terminal state/helpers (hoisted so the launch handler can use them) ──
let _ftVisible = false;
// A BrowserView always composites above DOM, so the terminal (plain DOM) can't sit on top of
// Wan2GP. Strategy: docked (bottom/top/left/right) → shrink the view, DOM console sits beside
// Wan2GP (side-by-side); floating → console is its OWN window (movable to another monitor) and
// Wan2GP is detached so the main window isn't left showing a grey Wan2GP panel.
// Separate-window state (native floating): backend owns truth (X-close invisible
// to us — synced via the term-closed event).
let _termWinOpen = false;
// Open the console as its own OS window (native floating): floats above
// everything incl. the native child, so Gradio stays visible. Keeps the
// child shown (re-syncs bounds) and clears any hidden-view note.
function openNativeTermWindow() {
  hideNativeHiddenNote();
  // Surface failures LOUDLY: a silent catch here looks exactly like
  // "floating does nothing" with no way to tell why.
  window.w2gp
    .createTermView()
    .then((r) => {
      _termWinOpen = true;
      appendLog(
        r && r.existing
          ? "[*] Console window focused"
          : "[*] Console opened in its own window (native floating mode)",
      );
    })
    .catch((e) => {
      _termWinOpen = false;
      _ftVisible = false;
      appendLog("[!] Console window failed to open: " + errText(e));
      showToast("✗ Console window failed: " + errText(e));
      showNativeHiddenNote();
    });
  _termWinOpen = true;
  _ftVisible = true;
  // DOM panel stands down (the window owns the console); mark floating state
  // so toggles/recovery route back here instead of the DOM path.
  try {
    const ft = $("floatingTerminal");
    if (ft) ft.className = "floating-term dock-floating hidden";
    document
      .querySelectorAll(".dock-btn")
      .forEach((b) =>
        b.classList.toggle("active", b.dataset.dock === "floating"),
      );
  } catch {}
  try {
    reshowNativeView();
  } catch {}
  try {
    renderTerminals();
  } catch {}
  syncTermEmbedPadding();
}
function currentDock() {
  const ft = $("floatingTerminal");
  for (const d of ["bottom", "top", "left", "right", "floating"]) {
    if (ft.classList.contains("dock-" + d)) return d;
  }
  return "bottom";
}
// Show the console for the current dock. In Tauri the console is ALWAYS the
// DOM terminal (all docks incl. floating overlay) — there is no separate native
// terminal window like Electron's TermView, so no special-casing by dock.
let _termBusy = false;
function showTerminal() {
  if (_termBusy) {
    appendLog("[i] showTerminal skipped (_termBusy)");
    return;
  }
  _termBusy = true;
  try {
    // Native + floating FIRST: separate OS window (floats above everything,
    // Gradio stays visible). Skips the DOM terminal entirely — showing it would
    // leave a dead panel behind once the window owns the console.
    if (
      currentDock() === "floating" &&
      window.w2gp.isNativeEmbed &&
      window.w2gp.isNativeEmbed()
    ) {
      openNativeTermWindow();
      return;
    }
    window.w2gp.destroyTermView();
    $("floatingTerminal").classList.remove("hidden");
    _ftVisible = true;
    renderTerminals();
    window.w2gp.reattachBrowserView();
    // ponytail: guard — bvSetDock/showTerminal recursion caused open/close loop (floating vs dock)
    const dock = currentDock();
    if (dock !== "floating") window.w2gp.bvSetDock(dock);
    window.w2gp.hideBrowserView("term");
    syncTermEmbedPadding();
    // Native child composites above the DOM: hideBrowserView('term') hid it —
    // for docked (side-by-side) mode re-show it shrunk beside the console.
    // Floating keeps it hidden (with an explanatory note, not black void).
    try {
      if (window.w2gp.isNativeEmbed && window.w2gp.isNativeEmbed()) {
        if (dock === "floating") showNativeHiddenNote();
        else reshowNativeView();
      }
    } catch {}
    // Synchronous (not timed) guard: both terminal functions run to completion
    // within one task (all backend calls are fire-and-forget), so same-stack
    // recursion is still blocked while sequential calls across async gaps —
    // e.g. term-window X-close → hideTerminal, then dock-back → showTerminal —
    // are never dropped. The old 50ms timeout ate those and wedged docking.
  } finally {
    _termBusy = false;
  }
}
function hideTerminal() {
  if (_termBusy) return;
  _termBusy = true;
  // Separate window (if any) always closes with the console.
  _termWinOpen = false;
  try {
    $("floatingTerminal").classList.add("hidden");
    _ftVisible = false;
    syncTermEmbedPadding();
    hideNativeHiddenNote();
    window.w2gp.destroyTermView();
    window.w2gp.reattachBrowserView();
    // Native child: restore full bounds now the console is gone (with settle).
    try {
      reshowNativeView();
    } catch {}
    // Same synchronous guard as showTerminal (see above): never time-based.
  } finally {
    _termBusy = false;
  }
}
async function toggleFloatingTerm() {
  if (_termBusy) return;
  if ($("dashBody").style.display === "none") {
    // Native + floating console lives in its own OS window (backend owns
    // truth — the DOM panel stays hidden, so classList can't drive this).
    if (
      window.w2gp.isNativeEmbed &&
      window.w2gp.isNativeEmbed() &&
      currentDock() === "floating"
    ) {
      try {
        const r = await window.w2gp.toggleTermWindow().catch(() => null);
        _termWinOpen = !!(r && r.open);
        _ftVisible = _termWinOpen;
      } catch {}
      return;
    }
    // DOM is truth — the flag desyncs if a toggle ever gets swallowed.
    if ($("floatingTerminal").classList.contains("hidden")) {
      renderTerminals();
      showTerminal();
    } else {
      hideTerminal();
    }
  }
}
function closeFloatingTerm() {
  hideTerminal();
}
// Dock switch pressed inside the separate term window: close it, then dock
// the DOM console in the main window (same end state as the main dock buttons).
window.__dockTerminal = async (dock) => {
  appendLog("[*] Docking console: " + dock + " (from separate window)");
  showToast("Docking console: " + dock);
  if (dock === "floating") {
    try {
      await window.w2gp.createTermView();
    } catch (e) {
      appendLog("[!] dock/floating reopen failed: " + errText(e));
    }
    return;
  }
  try {
    await window.w2gp.destroyTermView().catch(() => {});
  } catch (e) {
    appendLog("[!] dock/destroy failed: " + errText(e));
  }
  appendLog("[i] dock step: window closed");
  _termWinOpen = false;
  _ftVisible = false;
  try {
    const cfg = await window.w2gp.configLoad().catch(() => ({}));
    if (cfg && typeof cfg === "object") {
      cfg.termDockDefault = dock;
      window.w2gp.configSave(cfg).catch(() => {});
    }
  } catch (e) {
    appendLog("[!] dock/config failed: " + errText(e));
  }
  appendLog("[i] dock step: config saved");
  try {
    setFtDock(dock);
  } catch (e) {
    appendLog("[!] dock/setFtDock failed: " + errText(e));
  }
  appendLog(
    "[i] dock step: setFtDock done, dashHidden=" +
      ($("dashBody").style.display === "none"),
  );
  if ($("dashBody").style.display === "none") showTerminal();
  appendLog("[i] dock step: showTerminal returned");
};
// Apply a dock position to the floating terminal (className + IPC), without toggling visibility.
// When the console is open this also switches the rendering mode (DOM vs overlay) as needed.
function setFtDock(dock) {
  if (_termBusy) return;
  const ft = $("floatingTerminal");
  const wasVisible = !ft.classList.contains("hidden") && _ftVisible;
  ft.className =
    "floating-term dock-" +
    dock +
    (ft.classList.contains("hidden") ? " hidden" : "");
  if (dock !== "floating") ft.style.cssText = "";
  document
    .querySelectorAll(".dock-btn")
    .forEach((b) => b.classList.toggle("active", b.dataset.dock === dock));
  if (dock !== "floating") window.w2gp.bvSetDock(dock);
  // ponytail: don't re-enter showTerminal from here if we just changed dock — toggling dock while open re-shrinks view without loop
  if (wasVisible && $("dashBody").style.display === "none") {
    const _native = window.w2gp.isNativeEmbed && window.w2gp.isNativeEmbed();
    // Switching dock TO floating while open: open the separate window (the
    // hole that left a hidden child + black view). Leaving floating: close it.
    if (_native && dock === "floating") {
      openNativeTermWindow();
    } else {
      if (_native) {
        try {
          window.w2gp.destroyTermView().catch(() => {});
        } catch {}
        _termWinOpen = false;
      }
      // only re-flow BrowserView, don't re-create terminal DOM in a loop
      window.w2gp.hideBrowserView("term");
      if (_native) reshowNativeView();
    }
  }
  syncTermEmbedPadding();
}
// ponytail: Tauri has no native BrowserView — bvSetDock is a stub no-op, so a
// docked console would overlay and cover part of the Wan2GP iframe. Shrink the
// embed instead via container padding matching the console's real size.
function syncTermEmbedPadding() {
  const wc = $("webviewContainer");
  if (!wc) return;
  const ft = $("floatingTerminal");
  const dock = currentDock();
  const visible =
    $("dashBody").style.display === "none" &&
    !ft.classList.contains("hidden") &&
    dock !== "floating";
  wc.style.paddingBottom =
    visible && dock === "bottom" ? ft.offsetHeight + "px" : "";
  wc.style.paddingTop = visible && dock === "top" ? ft.offsetHeight + "px" : "";
  wc.style.paddingLeft =
    visible && dock === "left" ? ft.offsetWidth + "px" : "";
  wc.style.paddingRight =
    visible && dock === "right" ? ft.offsetWidth + "px" : "";
}
// Native-child placeholder: a native child composites above the DOM, so the
// free-floating console can't overlay Gradio — the child is hidden instead.
// Show an explanatory note (not black void) with the way back.
function showNativeHiddenNote() {
  try {
    const wc = $("webviewContainer");
    if (!wc) return;
    hideNativeHiddenNote();
    const d = document.createElement("div");
    d.id = "nativeHiddenNote";
    d.style.cssText =
      "flex:1;display:flex;align-items:center;justify-content:center;color:#888;font-size:13px;font-family:Geist Mono,monospace;text-align:center;padding:20px;line-height:1.8";
    d.textContent =
      "Desktop view hidden while the floating console is open. Dock the console (Bottom / Left / Top / Right) to see both side-by-side — or switch Renderer back to iframe for overlay.";
    wc.appendChild(d);
  } catch {}
}
function hideNativeHiddenNote() {
  try {
    document.getElementById("nativeHiddenNote")?.remove();
  } catch {}
}
// Native-child twin of syncTermEmbedPadding: the child ignores CSS padding
// (it composites above the DOM), so subtract the open docked console's strip
// from the measured container rect and push real bounds to Rust. Floating /
// hidden console → full container rect.
function syncNativeBoundsAdjusted() {
  try {
    if (!window.w2gp.isNativeEmbed || !window.w2gp.isNativeEmbed()) return;
    const wc = $("webviewContainer");
    if (!wc || wc.classList.contains("hidden")) return;
    const r = wc.getBoundingClientRect();
    if (!r.width || !r.height) return;
    let { left: x, top: y, width: w, height: h } = r;
    const ft = $("floatingTerminal");
    const dock = currentDock();
    if (
      ft &&
      !ft.classList.contains("hidden") &&
      $("dashBody").style.display === "none"
    ) {
      if (dock === "bottom") h = Math.max(0, h - ft.offsetHeight);
      else if (dock === "top") {
        const t = ft.offsetHeight;
        y += t;
        h = Math.max(0, h - t);
      } else if (dock === "left") {
        const l = ft.offsetWidth;
        x += l;
        w = Math.max(0, w - l);
      } else if (dock === "right") w = Math.max(0, w - ft.offsetWidth);
    }
    // Side panels (Manage/Guide) dock right: shrink the native child beside
    // them instead of detaching it, so Wan2GP stays visible instead of black.
    // offsetWidth is layout width (unaffected by the slide transform), and 0
    // when closed — every caller (terminal, reshow, resize) inherits this.
    try {
      w = Math.max(0, w - sidePanelWidth());
    } catch (e) {}
    // Same topbar clamp as __syncNativeBounds: the child must never cover metrics.
    try {
      const tb = document.querySelector(".topbar");
      if (tb) {
        const minY = tb.getBoundingClientRect().bottom;
        if (y < minY) {
          h -= minY - y;
          y = minY;
        }
      }
    } catch {}
    hideNativeHiddenNote();
    // Bounds spam quieting: transitions fire this up to 3× per flip — log
    // only on actual change, and only when Manage → Debug enables it.
    const _bk = `${Math.round(x)}.${Math.round(y)}.${Math.round(w)}.${Math.round(h)}`;
    if (window.__lastNativeBounds !== _bk) {
      window.__lastNativeBounds = _bk;
      if (window.__debugBounds) {
        appendLog(
          `[embed] native bounds x=${Math.round(x)} y=${Math.round(y)} w=${Math.round(w)} h=${Math.round(h)}`,
        );
      }
    }
    if (w > 10 && h > 10) {
      try {
        window.w2gp.bvSyncBoundsRect({ x, y, w, h });
      } catch {}
    }
  } catch {}
}
// Width of the currently open right-docked side panel (Manage/Guide), else 0.
function sidePanelWidth() {
  var w = 0;
  try {
    var sp = $("settingsPanel");
    if (sp && sp.classList.contains("open")) w = Math.max(w, sp.offsetWidth || 0);
    var gp = $("guidePanel");
    if (gp && gp.classList.contains("open")) w = Math.max(w, gp.offsetWidth || 0);
  } catch (e) {}
  return w;
}
// Re-show the native child with settle re-syncs: measuring right after unhide
// can catch a stale rect, leaving the child at the wrong size with Gradio
// controls cut off. Syncs now (forced layout), after paint, and after settle.
// No-op unless native embed is active.
function reshowNativeView() {
  try {
    if (!window.w2gp.isNativeEmbed || !window.w2gp.isNativeEmbed()) return;
    window.w2gp
      .reattachBrowserView()
      .then(() => {
        const once = () => {
          try {
            syncNativeBoundsAdjusted();
          } catch {}
        };
        once();
        try {
          requestAnimationFrame(once);
        } catch {}
        setTimeout(once, 300);
      })
      .catch(() => {});
  } catch {}
}
// Settings toggle handlers registered once (avoids memory leak from repeated onchange reassignment).
let _settingsTogglesReady = false;
function initSettingsToggles() {
  if (_settingsTogglesReady) return;
  _settingsTogglesReady = true;

  $("autoStartToggle")?.addEventListener("change", async () => {
    const el = $("autoStartToggle");
    const r = await window.w2gp.setAutoStart(el.checked);
    if (r && r.success)
      showToast(
        el.checked ? "Will start with Windows" : "Removed from startup",
      );
    else showToast("✗ " + (r && r.error ? r.error : "Failed"));
  });
  $("followSystemThemeToggle")?.addEventListener("change", async () => {
    const el = $("followSystemThemeToggle");
    await window.w2gp.setThemeFollowSystem(el.checked);
    if (el.checked)
      applyTheme(
        matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light",
      );
    showToast(
      el.checked ? "Theme will follow system" : "Manual theme control restored",
    );
  });
  $("notificationsToggle")?.addEventListener("change", async () => {
    const el = $("notificationsToggle");
    await window.w2gp.setNotificationsEnabled(el.checked);
    showToast(el.checked ? "Notifications enabled" : "Notifications disabled");
  });
  $("autoUpdateToggle")?.addEventListener("change", async () => {
    const el = $("autoUpdateToggle");
    const c = await window.w2gp.configLoad();
    c.autoUpdateEnabled = el.checked;
    await window.w2gp.configSave(c);
    showToast(
      el.checked
        ? "Update check on launch enabled"
        : 'Update check on launch disabled — updates only via "Check for updates"',
    );
  });
  $("debugBoundsChk")?.addEventListener("change", async () => {
    const el = $("debugBoundsChk");
    const c = await window.w2gp.configLoad();
    c.debugBounds = el.checked;
    await window.w2gp.configSave(c);
    window.__debugBounds = el.checked === true;
    showToast(
      el.checked
        ? "Embed-bounds logging on — reposition the view to see lines"
        : "Embed-bounds logging off",
    );
  });
  $("shareToggle")?.addEventListener("change", async () => {
    const el = $("shareToggle");
    const c = await window.w2gp.configLoad();
    c.share = el.checked;
    await window.w2gp.configSave(c);
    showToast(
      el.checked
        ? "Share link enabled — Gradio will create a public tunnel on next launch"
        : "Share link disabled",
    );
  });

  // ── Queue Notifier ──
  const notifStatus = (msg, isErr) => {
    const el = $("notifStatus");
    if (!el) return;
    el.textContent = msg || "";
    el.style.color = isErr ? "var(--signal-red)" : "var(--signal-green)";
  };
  const notifCollect = () => ({
    enabled: $("notifEnabled")?.checked || false,
    notifyOnComplete: $("notifOnComplete")?.checked || false,
    notifyOnFail: $("notifOnFail")?.checked || false,
    notifyOnProgress: $("notifOnProgress")?.checked || false,
    progressStep: parseInt($("notifProgressStep")?.value || "25", 10) || 25,
    url: ($("notifUrl")?.value || "").trim(),
  });
  const notifApplyDom = (cfg) => {
    if (!$("notifEnabled")) return;
    $("notifEnabled").checked = !!cfg.enabled;
    $("notifOnComplete").checked = cfg.notifyOnComplete !== false;
    $("notifOnFail").checked = cfg.notifyOnFail !== false;
    $("notifOnProgress").checked = !!cfg.notifyOnProgress;
    $("notifProgressStep").value = cfg.progressStep || 25;
    $("notifUrl").value = cfg.url || "";
  };
  window.w2gp
    .notifierConfig()
    .then((r) => {
      if (r && r.ok) notifApplyDom(r.config);
    })
    .catch(() => {});
  $("notifSaveBtn")?.addEventListener("click", async () => {
    const r = await window.w2gp.notifierSet(notifCollect());
    if (r && r.ok) {
      notifStatus("✓ Saved", false);
      if (r.config.enabled && r.config.url)
        window.w2gp.notifierEnsure().catch(() => {});
    } else notifStatus("✗ " + ((r && r.error) || "save failed"), true);
  });
  $("notifTestBtn")?.addEventListener("click", async () => {
    const r = await window.w2gp.notifierTest(notifCollect());
    if (r && r.ok) notifStatus("✓ Test sent", false);
    else notifStatus("✗ " + ((r && r.error) || "test failed"), true);
  });
  $("notifEnsureBtn")?.addEventListener("click", async () => {
    notifStatus("Installing Apprise…", false);
    const r = await window.w2gp.notifierEnsure();
    if (r && r.ok)
      notifStatus(
        r.already ? "Apprise already present" : "✓ Apprise installed",
        false,
      );
    else notifStatus("✗ " + ((r && r.error) || "install failed"), true);
  });
  $("notifAppriseLink")?.addEventListener("click", (e) => {
    e.preventDefault();
    window.w2gp.openExternal("https://github.com/caronc/apprise");
  });

  // ── Native Wan2GP notifications (wgp_config.json via shared/notifications) ──
  const nativeStatus = (msg, isErr) => {
    const el = $("nativeNotifStatus");
    if (!el) return;
    el.textContent = msg || "";
    el.style.color = isErr ? "var(--signal-red)" : "var(--signal-green)";
  };
  const nativeCollect = () => ({
    urls: ($("nativeNotifUrls")?.value || "").trim(),
    secure: $("nativeNotifSecure")?.checked !== false,
    onGeneration: $("nativeNotifOnGeneration")?.checked || false,
    onQueueComplete: $("nativeNotifOnQueueComplete")?.checked || false,
    onQueueInterrupted: $("nativeNotifOnQueueInterrupted")?.checked || false,
  });
  const nativeRefresh = async () => {
    let st;
    try {
      st = await window.w2gp.notifierNativeStatus();
    } catch {
      return;
    }
    if (!st || !st.ok || !st.supported) {
      // Older Wan2GP without shared/notifications → legacy sender UI.
      if ($("nativeNotifBlock")) $("nativeNotifBlock").style.display = "none";
      if ($("notifLegacyBlock")) $("notifLegacyBlock").style.display = "";
      if (st && !st.ok) nativeStatus("✗ " + (st.error || "status failed"), true);
      return;
    }
    if ($("notifLegacyBlock")) $("notifLegacyBlock").style.display = "none";
    if ($("nativeNotifSecure")) $("nativeNotifSecure").checked = st.secure !== false;
    if ($("nativeNotifOnGeneration")) $("nativeNotifOnGeneration").checked = !!st.onGeneration;
    if ($("nativeNotifOnQueueComplete")) $("nativeNotifOnQueueComplete").checked = !!st.onQueueComplete;
    if ($("nativeNotifOnQueueInterrupted")) $("nativeNotifOnQueueInterrupted").checked = !!st.onQueueInterrupted;
    try {
      const ld = await window.w2gp.notifierNativeLoad();
      if (ld && ld.ok && $("nativeNotifUrls")) $("nativeNotifUrls").value = ld.urlsText || "";
    } catch {}
    const bits = [];
    if (st.appriseVersion) bits.push(`apprise ${st.appriseVersion}${st.appriseBinary ? "" : " (no CLI)"}`);
    else bits.push("apprise missing");
    if (st.keyringVersion) bits.push(`keyring ${st.keyringVersion}`);
    else bits.push("keyring missing");
    bits.push(st.urlsCount ? `${st.urlsCount} destination(s)` : "no destinations");
    if (st.secure) bits.push(st.credentialSet ? "credential store" : "secure, nothing stored yet");
    if (st.keyringError && st.secure) bits.push("keyring: " + st.keyringError);
    if (st.onGeneration || st.onQueueComplete || st.onQueueInterrupted)
      bits.push("native active — launcher sender off");
    const pkgsMissing = !st.appriseVersion || !st.keyringVersion;
    nativeStatus(bits.join(" · "), !!(pkgsMissing || (st.keyringError && st.secure)));
  };
  nativeRefresh().catch(() => {});
  $("nativeNotifSaveBtn")?.addEventListener("click", async () => {
    const r = await window.w2gp.notifierNativeSave(nativeCollect());
    if (r && r.ok) {
      nativeStatus(
        r.nativeManaged
          ? "✓ Saved — native notifications active, launcher sender off"
          : "✓ Saved (no events selected — nothing will notify)",
        false,
      );
      if (r.nativeManaged && $("notifEnabled")) $("notifEnabled").checked = false;
    } else nativeStatus("✗ " + ((r && r.error) || "save failed"), true);
  });
  $("nativeNotifTestBtn")?.addEventListener("click", async () => {
    const r = await window.w2gp.notifierNativeTest(nativeCollect());
    if (r && r.ok) {
      const n = r.destinations || 0;
      nativeStatus(`✓ Test sent to ${n} destination(s)` + (r.warning ? " — " + r.warning : ""), false);
    } else nativeStatus("✗ " + ((r && r.error) || "test failed"), true);
  });
  $("nativeNotifEnsureBtn")?.addEventListener("click", async () => {
    nativeStatus("Installing Apprise + keyring…", false);
    const r = await window.w2gp.notifierEnsure();
    if (r && r.ok)
      nativeStatus(
        (r.already ? "Apprise already present" : "✓ Apprise installed") +
          (r.keyringAlready === false ? " + ✓ keyring installed" : r.keyringAlready ? " + keyring present" : ""),
        false,
      );
    else nativeStatus("✗ " + ((r && r.error) || "install failed"), true);
    nativeRefresh().catch(() => {});
  });
}

function openSettings() {
  // Mutual exclusion with the Guide panel — only one side panel at a time.
  if ($("guidePanel") && $("guidePanel").classList.contains("open")) {
    $("guidePanel").classList.remove("open");
  }
  initSettingsToggles();
  $("settingsPanel").classList.add("open");
  $("settingsOverlay").classList.add("visible");
  // Side-by-side: keep Wan2GP visible beside the panel (the bounds sync above
  // auto-trims the open panel width) instead of detaching it to black. The
  // translucent overlay stays click-to-close; no opaque blackout.
  if ($("dashBody").style.display === "none") {
    try {
      reshowNativeView();
    } catch (e) {}
  }
  window.w2gp.configLoad().then((cfg) => {
    if ($("launchArgsInput")) $("launchArgsInput").value = cfg.launchArgs || "";
    if ($("portInput")) $("portInput").value = cfg.serverPort || 7860;
    if ($("githubTokenInput"))
      $("githubTokenInput").value = cfg.githubToken || "";
    if ($("hfTokenInput")) $("hfTokenInput").value = cfg.hfToken || "";
    if ($("claudeApiKeyInput"))
      $("claudeApiKeyInput").value = cfg.claudeApiKey || "";
    // Floating terminal default dock
    const td = cfg.termDockDefault || "bottom";
    document.querySelectorAll('input[name="termDock"]').forEach((r) => {
      r.checked = r.value === td;
    });
    // Sync toggle states from config (handlers already registered via initSettingsToggles)
    const autoStart = $("autoStartToggle");
    if (autoStart) autoStart.checked = cfg.autoStart === true;
    const followTheme = $("followSystemThemeToggle");
    if (followTheme) followTheme.checked = cfg.themeFollowSystem === true;
    const notifications = $("notificationsToggle");
    if (notifications)
      notifications.checked = cfg.notificationsEnabled !== false;
    const autoUpdate = $("autoUpdateToggle");
    if (autoUpdate) autoUpdate.checked = cfg.autoUpdateEnabled !== false;
    const share = $("shareToggle");
    if (share) share.checked = cfg.share === true;
    // Appearance controls reflect the live-applied prefs (loadAppear ran at boot).
    _paintThemeDots();
    if ($("uiScaleInput")) $("uiScaleInput").value = String(_appearMem.ui);
    if ($("uiScaleVal")) $("uiScaleVal").textContent = _appearMem.ui + "%";
    if ($("termScaleInput"))
      $("termScaleInput").value = String(_appearMem.term);
    if ($("termScaleVal"))
      $("termScaleVal").textContent = _appearMem.term + "%";
    // GGUF CUDA kernel controls
    const g = cfg.ggufEnv || {
      enabled: true,
      matmulMode: "auto",
      streamK: true,
      bf16Fp16: false,
    };
    if ($("ggufEnabled")) $("ggufEnabled").checked = g.enabled !== false;
    if ($("ggufMatmulMode")) $("ggufMatmulMode").value = g.matmulMode || "auto";
    if ($("ggufStreamK")) $("ggufStreamK").checked = g.streamK !== false;
    if ($("ggufBf16Fp16")) $("ggufBf16Fp16").checked = g.bf16Fp16 === true;
    // AMD ROCm controls (backend: amdEnv.miopenDisabled, default false)
    const a = cfg.amdEnv || { miopenDisabled: false };
    if ($("amdMiopenDisabled"))
      $("amdMiopenDisabled").checked = a.miopenDisabled === true;
    // GPU device picker: fill the dropdown from the main process, keep current choice
    loadGpuDeviceOptions(cfg.gpuDevice || "auto");
    // Launcher GPU picker — auto | integrated | dedicated | disabled
    const lg = $("launcherGpuSelect");
    if (lg)
      lg.value =
        cfg.launcherGpu || (cfg.electronGpu === false ? "disabled" : "auto");
    const ss = $("sageSafeSelect");
    if (ss) ss.value = cfg.sageSafe === false ? "upstream" : "safe"; // ponytail: 1348e5b — default safe
    // Bind Address picker: reflect saved choice (default localhost)
    const sn = $("serverNameSelect");
    if (sn)
      sn.value = cfg.serverName === "127.0.0.1" ? "127.0.0.1" : "localhost";
    // Desktop embed picker: native (default) vs iframe child webview
    const em = $("embedModeSelect");
    if (em) em.value = cfg.embedMode === "iframe" ? "iframe" : "native";
    // Debug flags (cached on window for hot paths — no async in the log call)
    window.__debugBounds = cfg.debugBounds === true;
    const db = $("debugBoundsChk");
    if (db) db.checked = cfg.debugBounds === true;
  });
  // Heavy probes deferred past paint: the panel opens instantly (like Guide),
  // then fills in without janking the slide transition. The plugins list
  // additionally loads lazily on its tab (30s stale guard).
  setTimeout(() => {
    try {
      loadBrowserList();
    } catch (e) {}
    try {
      updateXetStatus();
    } catch (e) {}
    try {
      refreshUvCacheInfo();
    } catch (e) {}
    try {
      refreshElectronSection();
    } catch (e) {}
  }, 60);
  refreshPluginsLazy();
}
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
// ── Plugins (Wan2GP plugin manager parity: enable + install/update/uninstall + favourites) ──
let _pluginFavs = [];
let _pluginData = [];
let _pluginUpdates = {};
let _pluginLoadedAt = 0;
// Biggest Manage DOM write — load once, then only when stale or explicitly
// refreshed, so opening Manage never waits on the plugin disk walk.
function refreshPluginsLazy() {
  try {
    if (_pluginData.length && Date.now() - _pluginLoadedAt < 30000) return;
  } catch (e) {}
  refreshPlugins();
}
let _pluginQuery = "";
let _pluginSort = { key: "name", dir: 1 };
$("pluginSearchInput")?.addEventListener("input", (e) => {
  _pluginQuery = e.target.value || "";
  renderPlugins();
});
document.querySelectorAll(".plugin-sort").forEach((b) => {
  b.addEventListener("click", () => {
    const k = b.dataset.sort;
    if (_pluginSort.key === k) _pluginSort.dir *= -1;
    else _pluginSort = { key: k, dir: k === "date" ? -1 : 1 };
    renderPlugins();
  });
});
async function refreshPlugins() {
  const list = $("pluginList");
  if (!list) return;
  list.innerHTML = '<p class="token-hint">Loading…</p>';
  let r;
  try {
    r = await window.w2gp.pluginsList();
  } catch (e) {
    list.textContent = "";
    {
      const p = document.createElement("p");
      p.className = "token-hint";
      p.textContent = "✗ " + ((e && e.message) || String(e));
      list.append(p);
    }
    return;
  }
  if (!r || !r.ok) {
    list.textContent = "";
    {
      const p = document.createElement("p");
      p.className = "token-hint";
      p.textContent = (r && r.error) || "Failed to load";
      list.append(p);
    }
    return;
  }
  try {
    const cfg = await window.w2gp.configLoad();
    _pluginFavs = cfg.favoritePlugins || [];
  } catch {
    _pluginFavs = [];
  }
  _pluginData = r.plugins || [];
  _pluginLoadedAt = Date.now();
  renderPlugins();
}
function renderPlugins() {
  const list = $("pluginList");
  if (!list) return;
  const q = _pluginQuery.trim().toLowerCase();
  let arr = _pluginData.filter(
    (p) =>
      !q ||
      (
        (p.name || "") +
        " " +
        (p.author || "") +
        " " +
        p.id +
        " " +
        (p.description || "")
      )
        .toLowerCase()
        .includes(q),
  );
  const { key, dir } = _pluginSort;
  const grp = (p) => (p.group === "system" ? 1 : 0);
  arr = [...arr].sort(
    (a, b) =>
      grp(a) - grp(b) ||
      (() => {
        const x = (a[key] || "").toLowerCase(),
          y = (b[key] || "").toLowerCase();
        return x < y ? -dir : x > y ? dir : 0;
      })(),
  );
  document.querySelectorAll(".plugin-sort").forEach((b) => {
    const active = b.dataset.sort === _pluginSort.key;
    b.classList.toggle("active", active);
    b.textContent =
      b.textContent.replace(/ [▲▼]/, "") +
      (active ? (_pluginSort.dir === 1 ? " ▲" : " ▼") : "");
  });
  list.innerHTML = "";
  let lastGroup = "";
  for (const p of arr) {
    const g = p.group === "system" ? "system" : "community";
    if (g !== lastGroup) {
      lastGroup = g;
      const h = document.createElement("div");
      h.className = "plugin-group";
      h.textContent =
        g === "system" ? "System plugins (deepbeepmeep)" : "Community plugins";
      list.appendChild(h);
    }
    const row = document.createElement("div");
    row.className = "browser-row";
    const label = document.createElement("label");
    label.className = "browser-opt";
    const cb = document.createElement("input");
    cb.type = "checkbox";
    cb.checked = !!p.enabled;
    cb.dataset.pluginId = p.id;
    // ponytail: enabling a non-cloned plugin is a no-op (Wan2GP skips missing dirs) —
    // lock the box until installed so Save can't promise what isn't there.
    if (p.system || p.locked) {
      cb.checked = true;
      cb.disabled = true;
      if (p.locked)
        label.title =
          (p.description ? p.description + " — " : "") +
          "Default plugin, always enabled.";
    }
    if (!p.installed) cb.disabled = true;
    label.appendChild(cb);
    const badgeBase =
      (p.system ? "system" : p.installed ? "installed" : "available") +
      (p.version ? " v" + p.version : "");
    const badges = [badgeBase]
      .concat(p.author ? ["by " + p.author] : [])
      .join(" · ");
    label.appendChild(document.createTextNode(" " + p.name + " (" + badges));
    // Available update: amber row wash + bold badge (was a grey text fragment).
    const updCount = _pluginUpdates[p.id];
    if (updCount) {
      row.classList.add("plugin-has-update");
      label.appendChild(document.createTextNode(" · "));
      const ub = document.createElement("span");
      ub.className = "plugin-update-badge";
      ub.textContent = "⇪ " + updCount + " update(s)";
      label.appendChild(ub);
    }
    label.appendChild(document.createTextNode(")"));
    if (p.description) label.title = p.description;
    row.appendChild(label);
    // ★ favourite (auto-installed on fresh setup) — needs a URL to reinstall from
    if (p.url) {
      const fav = document.createElement("button");
      fav.className = "btn btn-ghost small";
      fav.textContent = _pluginFavs.includes(p.url) ? "★" : "☆";
      fav.title = "Favourite — auto-install on fresh setup";
      fav.addEventListener("click", async () => {
        const cfg = await window.w2gp.configLoad();
        let favs = cfg.favoritePlugins || [];
        favs = favs.includes(p.url)
          ? favs.filter((u) => u !== p.url)
          : [...favs, p.url];
        cfg.favoritePlugins = favs;
        await window.w2gp.configSave(cfg);
        _pluginFavs = favs;
        fav.textContent = favs.includes(p.url) ? "★" : "☆";
        showToast(
          favs.includes(p.url)
            ? "★ Favourited — will auto-install on setup"
            : "☆ Unfavourited",
        );
      });
      row.appendChild(fav);
    }
    // ⬇ install (catalog entries not yet cloned)
    if (!p.installed && p.url) {
      const ins = document.createElement("button");
      ins.className = "btn btn-primary small";
      ins.textContent = "Install";
      ins.addEventListener("click", async () => {
        ins.disabled = true;
        ins.textContent = "Installing…";
        appendLog("[*] Installing plugin " + p.name + " — progress below…");
        try {
          const r = await window.w2gp.pluginInstall(p.url);
          if (r && r.ok)
            showToast(
              "✓ " +
                p.name +
                " installed & enabled — restart Wan2GP to load it",
            );
          else showToast("✗ " + ((r && r.error) || "install failed"));
        } catch (e) {
          showToast("✗ " + e.message);
        } finally {
          refreshPlugins();
        }
      });
      row.appendChild(ins);
    }
    if (p.installed && !p.system && p.url) {
      const up = document.createElement("button");
      up.className = "btn btn-ghost small";
      up.textContent = "↻";
      up.title = "Check for updates";
      up.style.marginLeft = "4px";
      up.addEventListener("click", async () => {
        up.disabled = true;
        const orig = up.textContent;
        up.textContent = "…";
        try {
          const c = await window.w2gp.pluginCheckUpdate(p.id);
          if (c && c.update) {
            up.textContent = "⇪";
            appendLog(
              "[*] Updating plugin " +
                p.id +
                " (" +
                c.behind +
                " behind) — progress below…",
            );
            const u = await window.w2gp.pluginUpdate(p.id);
            if (u && u.ok)
              showToast("✓ " + p.name + " updated — restart Wan2GP to load it");
            else showToast("✗ " + ((u && u.error) || "update failed"));
          } else {
            showToast(
              "✓ " +
                p.name +
                " is up to date" +
                (c && c.error ? " (" + c.error + ")" : ""),
            );
          }
        } catch (e) {
          showToast("✗ " + e.message);
        } finally {
          up.disabled = false;
          up.textContent = orig;
        }
      });
      row.appendChild(up);
      // 🗑 uninstall (not system/bundled)
      if (
        ![
          "downloads",
          "media_flow",
          "models_manager",
          "motion_designer",
          "sample",
        ].includes(p.id)
      ) {
        const del = document.createElement("button");
        del.className = "btn btn-ghost small";
        del.textContent = "🗑";
        del.title = "Uninstall plugin";
        del.style.marginLeft = "4px";
        del.addEventListener("click", async () => {
          const choice = await window.w2gp.confirmDialog({
            title: "Uninstall " + p.name + "?",
            message: "Remove the plugin folder and disable it?",
          });
          if (choice !== "ok") return;
          del.disabled = true;
          try {
            const u = await window.w2gp.pluginUninstall(p.id);
            if (u && u.ok)
              showToast(
                u.pending
                  ? "⏳ " + (u.hint || "Locked — deleted on next start")
                  : "✓ " + p.name + " uninstalled",
              );
            else showToast("✗ " + ((u && u.error) || "uninstall failed"));
          } catch (e) {
            showToast("✗ " + e.message);
          } finally {
            refreshPlugins();
          }
        });
        row.appendChild(del);
      }
    }
    list.appendChild(row);
  }
  if (!arr.length)
    list.innerHTML = '<p class="token-hint">No plugins match.</p>';
}
$("pluginCheckUpdatesBtn")?.addEventListener("click", async () => {
  const btn = $("pluginCheckUpdatesBtn"),
    st = $("pluginRefreshStatus");
  const orig = btn.textContent;
  btn.disabled = true;
  btn.textContent = "Checking…";
  if (st)
    st.textContent = "⏳ Fetching plugin remotes — progress in the console…";
  appendLog("[*] Checking all plugins for updates — progress below…");
  try {
    const r = await window.w2gp.pluginCheckUpdates();
    if (r && r.ok) {
      _pluginUpdates = {};
      for (const u of r.updates || [])
        if (u.update) _pluginUpdates[u.id] = u.behind;
      renderPlugins();
      if (st)
        st.textContent = r.updates_available
          ? "⇪ " +
            r.updates_available +
            " update(s) available — use ↻ per plugin"
          : "✓ All plugins up to date";
      showToast(
        r.updates_available
          ? "⇪ " + r.updates_available + " update(s) available"
          : "✓ All plugins up to date",
      );
    } else showToast("✗ " + ((r && r.error) || "check failed"));
  } catch (e) {
    showToast("✗ " + e.message);
    if (st) st.textContent = "✗ " + e.message;
  } finally {
    btn.disabled = false;
    btn.textContent = orig;
  }
});
$("pluginRefreshBtn")?.addEventListener("click", async () => {
  const btn = $("pluginRefreshBtn"),
    st = $("pluginRefreshStatus");
  const orig = btn.textContent;
  btn.disabled = true;
  btn.textContent = "Refreshing…";
  if (st) st.textContent = "⏳ Contacting GitHub — progress in the console…";
  appendLog("[*] Refreshing plugin library from GitHub — progress below…");
  try {
    const r = await window.w2gp.pluginRefreshCatalog();
    if (r && r.ok) {
      if (st)
        st.textContent =
          "✓ " +
          r.checked +
          " checked, " +
          r.updated +
          " updated" +
          (r.updates_available
            ? ", " +
              r.updates_available +
              " update(s) available — use ↻ per plugin"
            : "");
      showToast("✓ Library refreshed");
    } else showToast("✗ " + ((r && r.error) || "refresh failed"));
  } catch (e) {
    showToast("✗ " + e.message);
    if (st) st.textContent = "✗ " + e.message;
  } finally {
    btn.disabled = false;
    btn.textContent = orig;
    refreshPlugins();
  }
});
$("pluginSaveBtn")?.addEventListener("click", async () => {
  const btn = $("pluginSaveBtn"),
    st = $("pluginSaveStatus");
  const orig = btn.textContent;
  btn.disabled = true;
  btn.textContent = "Saving…";
  const ids = [
    ...document.querySelectorAll('#pluginList input[type="checkbox"]'),
  ]
    .filter((c) => c.checked)
    .map((c) => c.dataset.pluginId);
  try {
    await window.w2gp.writeWgpConfig({ enabled_plugins: ids });
    if (st) st.textContent = "✓ Saved — takes effect on next Wan2GP launch.";
    showToast("✓ Plugins saved");
  } catch (e) {
    if (st) st.textContent = "✗ " + e.message;
    showToast("✗ " + e.message);
  } finally {
    btn.disabled = false;
    btn.textContent = orig;
  }
});
$("pluginInstallBtn")?.addEventListener("click", async () => {
  const input = $("pluginUrlInput");
  const st = $("pluginSaveStatus");
  const url = (input?.value || "").trim();
  if (!url) {
    showToast("Paste a plugin git URL first");
    return;
  }
  const btn = $("pluginInstallBtn");
  const orig = btn.textContent;
  btn.disabled = true;
  btn.textContent = "Installing…";
  if (st) st.textContent = "⏳ Installing — progress in the console…";
  appendLog("[*] Installing plugin from " + url + " — progress below…");
  try {
    const r = await window.w2gp.pluginInstall(url);
    if (r && r.ok) {
      showToast("✓ Plugin installed & enabled — restart Wan2GP to load it");
      if (input) input.value = "";
      if (st) st.textContent = "✓ Installed " + r.id;
    } else {
      showToast("✗ " + ((r && r.error) || "install failed"));
      if (st) st.textContent = "✗ " + ((r && r.error) || "install failed");
    }
  } catch (e) {
    showToast("✗ " + e.message);
    if (st) st.textContent = "✗ " + e.message;
  } finally {
    btn.disabled = false;
    btn.textContent = orig;
    refreshPlugins();
  }
});
// Populate the Manage "Default Browser" list from the main process.
async function loadBrowserList() {
  const list = $("browserList");
  if (!list) return;
  list.innerHTML =
    '<div class="browser-row"><label class="browser-opt"><input type="radio" name="defaultBrowser" value="system" checked> System default</label></div>';
  try {
    const { browsers, defaultBrowser } = await window.w2gp.detectBrowsers();
    for (const b of browsers) {
      const row = document.createElement("div");
      row.className = "browser-row";
      const label = document.createElement("label");
      label.className = "browser-opt";
      const radio = document.createElement("input");
      radio.type = "radio";
      radio.name = "defaultBrowser";
      radio.value = b.id;
      radio.disabled = !b.installed;
      if (b.id === defaultBrowser) radio.checked = true;
      label.appendChild(radio);
      label.appendChild(
        document.createTextNode(
          " " + b.name + (b.installed ? "" : " (not installed)"),
        ),
      );
      row.appendChild(label);
      list.appendChild(row);
    }
    list.querySelectorAll('input[name="defaultBrowser"]').forEach((r) => {
      r.addEventListener("change", async () => {
        if (!r.checked) return;
        const cfg = await window.w2gp.configLoad();
        cfg.defaultBrowser = r.value;
        await window.w2gp.configSave(cfg);
        appendLog(`[*] Default browser set to: ${r.value}`);
      });
    });
  } catch (e) {
    appendLog(`[!] Browser detection failed: ${errText(e)}`);
  }
}
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

// ── Init ──
document.addEventListener("DOMContentLoaded", async () => {
  try {
    // ponytail: keep splash visible while loading — covers WebView2 + python scan (was showing empty dashboard)
    $("splashStatus").textContent = "Loading...";
    setDesktopUpdateIndicator(false);
    const [installed, cfgPreload] = await Promise.all([
      window.w2gp.checkInstalled(),
      window.w2gp.configLoad().catch(() => ({})),
    ]);
    await checkCrashRecovery();

    window.w2gp.getDesktopVersion().then((v) => {
      if (!v) return;
      document.title = "Wan2GP Desktop Launcher v" + v;
      var verEl = $("settingsVersionNum");
      if (verEl) verEl.textContent = v;
      var appVerEl = $("appVersionTag");
      if (appVerEl) appVerEl.textContent = "v" + v;
    });
    setupScrollUnfollow("termBody", "dashTermFollowBtn");
    setupScrollUnfollow("installTermBody", "installFollowBtn");

    window.w2gp.onSetupOutput((t) =>
      appendLog(
        t.replace(/\x1b\[[0-9;?]*[a-zA-Z]/g, "").replace(/\x08/g, ""),
        false,
      ),
    );
    window.w2gp.onDlss5Progress(dlss5OnEvent);
    window.w2gp.onInstallProgress(installProgressOnEvent);

    window.w2gp.onLaunchLog((t) => {
      const clean = t
        .replace(/\x1b\[[0-9;?]*[a-zA-Z]/g, "")
        .replace(/\x08/g, "");
      appendLog(clean, false);
      // Console-first launch: stay on the dashboard while starting, open the
      // destination the moment the backend reports ready.
      if (_pendingOpen && /Wan2GP ready/.test(clean)) {
        const p = _pendingOpen;
        _pendingOpen = null;
        if (p.kind === "desktop") openDesktopView(p.url, true);
        else
          openBrowserView(p.url, p.noGpu).catch((e) =>
            appendLog(`[LAUNCH ERROR] ${errText(e)}`),
          );
      }
    });
    window.w2gp.onSetupPhase((p) => {
      if (p.done) {
        if (prevPhaseId && prevPhaseId !== p.id) taskComplete(prevPhaseId);
        taskComplete(p.id);
        prevPhaseId = null;
      } else {
        if (prevPhaseId && prevPhaseId !== p.id) taskComplete(prevPhaseId);
        taskStart(p.id);
        appendLog("[*] " + p.label);
        prevPhaseId = p.id;
      }
    });
    window.w2gp.onSetupProfile((p) => {
      $("installProfile").textContent = p;
      $("installProfileRow").style.display = "flex";
    });

    const cfg =
      cfgPreload || (await window.w2gp.configLoad().catch(() => ({})));
    if (cfg.themeFollowSystem)
      applyTheme(
        matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light",
      );
    else if (cfg.theme === "dark") applyTheme("dark");
    loadAppear(cfg);
    initAppearControls(); // top-bar palette + A± need no settings panel
    // System theme follow (real matchMedia — backend only persists the preference).
    // The native onSystemThemeChange event never fires in Tauri; this is the mechanism.
    if (!window.__themeFollowBound) {
      window.__themeFollowBound = true;
      matchMedia("(prefers-color-scheme: dark)").addEventListener(
        "change",
        async () => {
          try {
            const c = await window.w2gp.configLoad();
            if (c.themeFollowSystem)
              applyTheme(
                matchMedia("(prefers-color-scheme: dark)").matches
                  ? "dark"
                  : "light",
              );
          } catch {}
        },
      );
    }

    // Embedded-Wan2GP view crashed and was auto-reloaded by the main process.
    window.w2gp.onBvCrashRecovered(() =>
      showToast("Wan2GP view reloaded after a crash"),
    );

    // hardware probe — fire-and-forget, fills specs when ready (no await)
    loadHardware()
      .then((s) => {
        if (s)
          appendLog(
            `[*] Hardware: ${s.cpu || "?"} · ${s.ram || "?"} RAM · ${s.gpu || "?"} (${s.vram || "?"})`,
          );
      })
      .catch((e) => {
        console.warn("[hw] detectHardware failed", e);
      });
    setTimeout(() => loadHardware().catch(() => {}), 2000);

    window.w2gp
      .getDesktopVersion()
      .then((v) => {
        if (v)
          appendLog(
            `[*] Launcher v${v} ready — dashboard live. Metrics polling every 3s; update checks in background.`,
          );
      })
      .catch(() => {});

    if (installed.repo && installed.env) {
      // ponytail: fast paint — show dashboard instantly (~300ms), fill versions in background
      appendLog("[*] Wan2GP install found — loading dashboard…");
      show("dashboard");
      refreshDashboard()
        .then(() => {
          const env = $("envName")?.textContent?.trim() || "—";
          const torch = $("specTorch")?.textContent?.trim() || "—";
          appendLog(`[*] Environment ready: ${env} · torch ${torch}`);
          // One-time startup drift sync: the banner otherwise reflects only
          // the last update's verdict — re-verify against the live env so a
          // stale banner clears (or real drift names itself) unprompted.
          window.w2gp
            .depCheck()
            .then((d) => {
              if (d && Array.isArray(d.drift) && d.drift.length) {
                appendLog(
                  "[!] dependency drift: " + d.drift.join(", ") + " — use restore",
                );
                showDriftBanner(d.drift);
              } else hideDriftBanner();
            })
            .catch(() => {});
        })
        .catch(() => {});
      startMetricsPolling();
      startDownloadsWatch();
      // Periodic Wan2GP update re-check while the app is open (30 min) + Desktop (5h).
      // Launch-time check alone misses updates released mid-session; the
      // renderer-side timers re-poll and re-flag the green dot + changelog.
      startWangpPolling();
      startDeepyWebPolling();
      startDesktopPolling();
      // (Wan2GP polls immediately at boot; Desktop does its early check
      // 8s after boot inside startDesktopPolling.)
      // D1: silent settings auto-scan (issue #7 class) — out-of-range dropdown
      // values make Wan2GP reject the whole settings form on save; repair them
      // in the background so the user never hits the "can't save" wall. Writes
      // only when a fix is actually found (console log + toast otherwise quiet).
      silentSettingsRepair();
    } else {
      $("splashStatus").textContent = "First-time setup...";
      appendLog(
        "[*] First run — no Wan2GP install detected. Complete the installer below to set up.",
      );
      // External drive disconnected or letter changed (e.g. J:\WanGPApp was there
      // and now isn't)? Say so explicitly instead of a blank "first run".
      if (installed && installed.missingPrevious) {
        appendLog(
          "[!] Previous install not found: " + installed.missingPrevious,
        );
        appendLog(
          "[!] If that is an external drive, reconnect it (check the drive letter) and restart the launcher — or install fresh / pick the new location below.",
        );
        $("installSubtitle").textContent =
          "Previous install at " +
          installed.missingPrevious +
          " is missing — reconnect the drive, or set up again below.";
        try {
          showToast(
            "⚠ Previous install folder missing — reconnect the drive or reinstall",
          );
        } catch {}
      }
      const hw = await window.w2gp.detectHardware();
      $("installCpu").textContent = hw.cpu || "—";
      $("installRam").textContent = hw.ram || "—";
      $("installGpu").textContent = hw.gpu || "—";
      $("installVram").textContent = hw.vram || "—";
      loadPaths();
      try {
        const mf = await window.w2gp.detectModelFolders();
        if (mf.checkpointsPaths && mf.checkpointsPaths.length) {
          _modelCkpts = mf.checkpointsPaths[0];
          $("installCkptsPath").textContent = _modelCkpts;
        }
        if (mf.lorasRoot) {
          _modelLoras = mf.lorasRoot;
          $("installLorasPath").textContent = _modelLoras;
        }
      } catch {}
      show("installer");
      if (!(installed && installed.missingPrevious))
        $("installSubtitle").textContent =
          "Select environment type, then click Install";
      // Target-folder triage: ATFGriff's J:\\WanGPApp wasn't empty (Pinokio? previous
      // attempt?) and we merged blindly over it. Show what's there first.
      refreshTargetVerdict().catch(() => {});
      refreshModelDiskGates().catch(() => {});
      $("installStartBtn").classList.remove("hidden");
      $("envTypeSelect").classList.remove("disabled");
      document
        .querySelectorAll(".env-type-btn")
        .forEach((b) => (b.disabled = false));
      // Show expected packages for this hardware
      window.w2gp.getHardwareProfile().then((hp) => {
        if (!hp) return;
        var list = $("installPkgsList");
        var header = $("installPkgsProfile");
        if (list && hp.packages && hp.packages.length) {
          if (header)
            header.textContent = "(" + hp.profile.replace(/_/g, " ") + ")";
          list.textContent = "";
          for (const p of hp.packages) {
            const s = document.createElement("span");
            s.className = "ipkg-item";
            s.textContent = p;
            list.append(s);
          }
          $("installPkgs").style.display = "";
        }
        // Distinct kernel-wheels group (so the wheels are clearly visible pre-install)
        var klist = $("installKernelsList");
        var kheader = $("installKernelsProfile");
        if (klist && hp.kernels && hp.kernels.length) {
          if (kheader)
            kheader.textContent = "(" + hp.profile.replace(/_/g, " ") + ")";
          klist.textContent = "";
          for (const k of hp.kernels) {
            const row = document.createElement("div");
            row.className = "ikernel-item";
            const lab = document.createElement("span");
            lab.className = "ikernel-label";
            lab.textContent = k.label;
            const dist = document.createElement("span");
            dist.className = "ikernel-dist";
            dist.textContent = k.dist;
            row.append(lab, dist);
            klist.append(row);
          }
          $("installKernels").style.display = "";
        }
        // GPU Profile Overview — installer only (different screen; the dashboard
        // consolidates detected versions + kernel wheels into the env_uv card).
        renderProfileOverview(hp.detail, {
          box: "installProfileOverview",
          profile: "ipoProfile",
          python: "ipoPython",
          torch: "ipoTorch",
          triton: "ipoTriton",
          sage: "ipoSage",
          sparge: "ipoSparge",
          flash: "ipoFlash",
          kernels: "ipoKernels",
        });
      });
      // Pre-flight resolved stack: GPU/CUDA/driver/disk gates + exact Python pin.
      // (Tauri install_plan shape: { plan: {gpuName,vendor,cuda,torch,driverWarning,profile}, disk: {free,total} }.)
      window.w2gp
        .installPlan()
        .then((r) => {
          if (!r || !r.plan) return;
          const grid = $("installStackGrid");
          const warn = $("installStackWarn");
          const stack = $("installStack");
          if (!grid) return;
          const p = r.plan;
          const freeBytes = r.disk && r.disk.free != null ? r.disk.free : null;
          const freeGb =
            freeBytes == null ? "?" : (freeBytes / 1073741824).toFixed(1);
          const rows = [
            ["GPU", p.gpuName || p.vendor],
            ["CUDA build", p.cuda],
            ["PyTorch", p.torch],
            ["Profile", (p.profile || "").replace(/_/g, " ")],
            ["Free disk", freeGb + " GB"],
          ];
          const renderRows = () => {
            grid.textContent = "";
            for (const row of rows) {
              const d = document.createElement("div");
              d.className = "istack-row"; // rebased: keep master naming (branch had `row`, identical DOM-safe code)
              const k = document.createElement("span");
              k.className = "istack-k";
              k.textContent = row[0];
              const v = document.createElement("span");
              v.className = "istack-v";
              v.textContent = row[1];
              d.append(k, v);
              grid.append(d);
            }
          };
          renderRows();
          // Exact Python pin setup.py will demand via `uv venv --python X`
          // (pythonPreflight is check-only — the download happens on Install).
          window.w2gp
            .pythonPreflight()
            .then((pf) => {
              if (!pf || !pf.wanted) return;
              const uvTag = pf.uvVersion
                ? " (" + pf.uvVersion.split(" ").slice(0, 2).join(" ") + ")"
                : "";
              const state = pf.uvVersion
                ? pf.path && pf.runs
                  ? "✓ " + pf.wanted + " ready"
                  : pf.path
                    ? "⚠ " +
                      pf.wanted +
                      " corrupted — auto-reinstall on Install"
                    : "⬇ " + pf.wanted + " — auto-download on Install"
                : "✗ uv not found";
              rows.push(["Python" + uvTag, state]);
              renderRows();
              if (pf.hint && warn)
                warn.innerHTML +=
                  '<div class="istack-hint">' + escHtml(pf.hint) + "</div>";
            })
            .catch(() => {});
          const warns = [];
          if (p.driverWarning) warns.push(p.driverWarning);
          if (freeBytes != null && freeBytes < 10 * 1073741824)
            warns.push(
              "Only " +
                freeGb +
                " GB free — 50+ GB recommended (models are tens–hundreds of GB).",
            );
          if (warn) {
            warn.textContent = "";
            for (const w of warns) {
              const d = document.createElement("div");
              d.className = "istack-w";
              d.textContent = "⚠ " + w;
              warn.append(d);
            }
            if (
              freeBytes != null &&
              freeBytes >= 10 * 1073741824 &&
              freeBytes < 50 * 1073741824
            ) {
              const d = document.createElement("div");
              d.className = "istack-hint";
              d.textContent =
                freeGb +
                " GB free is tight — models alone can exceed 50 GB. A non-system drive is recommended.";
              warn.append(d);
            }
          }
          stack.style.display = "";
          // Hard block only when install can't succeed (cu130 driver too old, or ~no disk).
          const startBtn = $("installStartBtn");
          const hardBlocked =
            /R580/.test(p.driverWarning || "") ||
            (freeBytes != null && freeBytes < 10 * 1073741824);
          if (startBtn && hardBlocked) {
            startBtn.disabled = true;
            startBtn.title = "Resolve the warnings above before installing";
            startBtn.textContent = "Install blocked — see warnings";
          }
        })
        .catch(() => {});
    }
  } catch (e) {
    const el = $("splashError");
    if (el) {
      el.textContent = e.stack || String(e);
      el.classList.remove("hidden");
    }
    $("splashStatus").textContent = "Startup error";
  }
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

// ── Live topbar metrics (CPU/GPU/RAM/VRAM sparklines) ──
const _sparkHistory = {
  cpu: [],
  gpu: [],
  gpu2: [],
  ram: [],
  vram: [],
  vram2: [],
};
const _sparkMax = 60; // samples kept (~2 min at 2s)

function drawSpark(id, data, color) {
  const c = $(id);
  if (!c) return;
  const ctx = c.getContext("2d");
  const w = c.width,
    h = c.height;
  ctx.clearRect(0, 0, w, h);
  if (data.length < 2) return;
  const max = 100;
  ctx.beginPath();
  data.forEach((v, i) => {
    const x = (i / (data.length - 1)) * w;
    const y = h - (Math.max(0, Math.min(max, v)) / max) * h;
    if (i === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  });
  ctx.strokeStyle = color;
  ctx.lineWidth = 1.25;
  ctx.stroke();
  // fill under curve
  ctx.lineTo(w, h);
  ctx.lineTo(0, h);
  ctx.closePath();
  ctx.fillStyle = color + "22";
  ctx.fill();
}

function pushMetric(key, val) {
  const arr = _sparkHistory[key];
  arr.push(val == null ? 0 : val);
  if (arr.length > _sparkMax) arr.shift();
}

function startMetricsPolling() {
  const tick = async () => {
    // Skip sampling only when the window itself is hidden/minimized.
    // The topbar metrics stay visible in embed/webview mode while dashBody
    // is hidden — gating on dashBody froze them whenever Wan2GP runs
    // embedded. (visibilitychange already pauses the timer when hidden.)
    if (document.hidden) return;
    let m;
    try {
      m = await window.w2gp.getSystemMetrics();
    } catch {
      return;
    }
    if (!m) return;
    if (m.ramFree) {
      const el = $("specRamFree");
      if (el) el.textContent = "(" + m.ramFree + " free)";
    }
    if (m.vramFree) {
      const el = $("specVramFree");
      if (el) el.textContent = "(" + m.vramFree + " free)";
    }
    pushMetric("cpu", m.cpu);
    pushMetric("gpu", m.gpu);
    pushMetric("ram", m.ram);
    pushMetric("vram", m.vram);
    if ($("valCpu"))
      $("valCpu").textContent = m.cpu == null ? "—" : m.cpu + "%";
    if ($("valGpu"))
      $("valGpu").textContent = m.gpu == null ? "—" : m.gpu + "%";
    if ($("valRam"))
      $("valRam").textContent =
        m.ramUsed == null ? "—" : m.ramUsed + "/" + m.ramTotal;
    if ($("valVram"))
      $("valVram").textContent = m.vramUsed
        ? m.vramUsed + "/" + m.vramTotal
        : "—";
    drawSpark("sparkCpu", _sparkHistory.cpu, "#4ADE80");
    drawSpark("sparkGpu", _sparkHistory.gpu, "#60A5FA");
    drawSpark("sparkRam", _sparkHistory.ram, "#FBBF24");
    drawSpark("sparkVram", _sparkHistory.vram, "#F472B6");
    // 2nd GPU (iGPU or dual dGPU) — ponytail: flicker fix — only toggle display when state actually changes
    const hasGpu2 = m.gpus && m.gpus.length > 1 && m.gpus[1] != null;
    const g2 = hasGpu2
      ? m.gpus[1]
      : m.gpu2 == null
        ? null
        : {
            gpu: m.gpu2,
            vram: m.vram2,
            vramUsed: m.vramUsed2,
            vramTotal: m.vramTotal2,
          };
    const mg2 = $("metricGpu2"),
      mv2 = $("metricVram2");
    const shouldShow = !!(g2 && g2.gpu != null);
    const isShown = mg2 && mg2.style.display !== "none";
    if (shouldShow) {
      pushMetric("gpu2", g2.gpu);
      pushMetric("vram2", g2.vram);
      if ($("valGpu2")) $("valGpu2").textContent = g2.gpu + "%";
      if ($("valVram2"))
        $("valVram2").textContent = g2.vramUsed
          ? g2.vramUsed + "/" + g2.vramTotal
          : "—";
      drawSpark("sparkGpu2", _sparkHistory.gpu2, "#A78BFA");
      drawSpark("sparkVram2", _sparkHistory.vram2, "#FB7185");
      if (!isShown) {
        if (mg2) mg2.style.display = "";
        if (mv2) mv2.style.display = "";
      }
      if (mg2)
        mg2.title =
          "GPU 2" + (m.gpus[1] ? " — " + (m.gpus[1].vramTotal || "") : "");
      if (mv2)
        mv2.title =
          "VRAM 2 " + (g2.vramUsed || "") + "/" + (g2.vramTotal || "");
    } else if (isShown) {
      if (mg2) mg2.style.display = "none";
      if (mv2) mv2.style.display = "none";
    }
  };
  if (window.__metricsTimer) clearInterval(window.__metricsTimer);
  window.__metricsTick = tick;
  tick();
  window.__metricsTimer = setInterval(tick, 3000);
  // Pause the 2s nvidia-smi sampling while the window is hidden/minimized;
  // resume with an immediate tick on visibility.
  if (!window.__metricsVisBound) {
    window.__metricsVisBound = () => {
      if (document.hidden) {
        if (window.__metricsTimer) {
          clearInterval(window.__metricsTimer);
          window.__metricsTimer = null;
        }
      } else if (!window.__metricsTimer) {
        startMetricsPolling();
      }
    };
    document.addEventListener("visibilitychange", window.__metricsVisBound);
  }
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

// ── Installer ──
let selectedEnvType = "uv";
// Checklist verdict: when the install folder holds a repo without a working env
// (repo_no_env / ours_broken_env), the choice lives in the #targetChoiceList
// radios and the big Install button dispatches it (see startInstall).
let _targetChoiceMode = null;
// True while an install is actually running (set in doInstall, cleared on
// every exit) — verdict refreshes must never resurrect Install mid-install
// (e.g. Browse clicked during a fresh install re-trips repo_no_env).
let _installRunning = false;
// Stashed Fresh-repo backup choice (collect-only modal). The wipe launches
// solely from the big Install button — never from inside the backup dialog.
let _freshBackupChoice = null;
// Snapshot of the live radio pick backing _freshBackupChoice. Compared at
// dispatch time so touching a radio after the modal only re-collects when
// the pick actually moved on (Loop 3 fix).
let _freshBackupPick = null;
// Verdict mode last rendered by refreshTargetVerdict (null = none yet).
// Guards the radio force-check defaults so refreshes preserve user picks.
let _verdictModeShown = null;
// Latest classifyTarget verdict (null when hidden/failed). Lets the
// fallthrough dispatch ask once for foreign folders instead of bouncing.
let _lastVerdict = null;

document.querySelectorAll(".env-type-btn").forEach((btn) => {
  btn.addEventListener("click", () => {
    document
      .querySelectorAll(".env-type-btn")
      .forEach((b) => b.classList.remove("selected"));
    btn.classList.add("selected");
    selectedEnvType = btn.dataset.env;
  });
});

$("installStartBtn").addEventListener("click", startInstall);
// NOTE: no radio change listener voids _freshBackupChoice here. Stash
// staleness is reconciled at dispatch time (snapshot vs live pick), so
// touching a radio after the backup modal must not force a reopen.
// NOTE: the healthy-state trio and the no-env checklist are radios now —
// all launching goes through the big Install button (see startInstall).
// The backup modal below is collect-only; doInstall('reinstall') wipes the
// repo (trash, not delete), reinstalls, and merges the backup back.

$("validateInstallBtn")?.addEventListener("click", async () => {
  const btn = $("validateInstallBtn");
  const warn = $("installStackWarn");
  btn.disabled = true;
  btn.textContent = "Validating…";
  if (warn) warn.textContent = "";
  try {
    const r = await window.w2gp.validateInstall();
    if (r && r.ok) {
      const line = `✓ torch ${r.torch} · CUDA available: ${r.cudaAvailable} (${r.cudaVer})`;
      if (warn) {
        const d = document.createElement("div");
        d.className = "istack-ok";
        d.textContent = "⚡ " + line;
        warn.append(d);
      }
      btn.textContent = "Validated ✓";
    } else {
      if (warn) {
        const d = document.createElement("div");
        d.className = "istack-w";
        d.textContent = "✗ " + ((r && r.error) || "validation failed");
        warn.append(d);
      }
      btn.textContent = "Validate failed";
    }
  } catch (e) {
    if (warn) {
      const d = document.createElement("div");
      d.className = "istack-w";
      d.textContent = "✗ " + ((e && e.message) || String(e));
      warn.append(d);
    }
    btn.textContent = "Validate failed";
  }
});

// When the user picks a bare drive root (e.g. D:), we DON'T apply it (installing
// on a root fails). Instead we show a cross + message on the Install button.
// Cleared as soon as a valid folder is chosen.
let _pendingRoot = null;

function reflectRootBlock(rootPath) {
  const set = (id, val) => {
    const e = $(id);
    if (e) {
      e.textContent = breakPath(val) || "—";
      e.title = val || "";
    }
  };
  set("installAppDataPath", rootPath);
  const startBtn = $("installStartBtn");
  const rootWarn = $("installRootWarn");
  if (startBtn) {
    startBtn.disabled = true;
    startBtn.title = "Choose a folder, not a drive root.";
  }
  if (rootWarn) {
    rootWarn.textContent =
      "⚠ Install location is a drive root (" +
      rootPath +
      "). Pick a folder using Browse.";
    rootWarn.classList.remove("hidden");
  }
}

$("browseAppDataPath")?.addEventListener("click", async () => {
  const folder = await window.w2gp.selectFolder();
  if (!folder) return;
  // Bare drive root: don't block — show it resolved to <root>\Wan2GP and use
  // that (backend rejects raw roots too, defense in depth).
  if (isDriveRoot(folder)) {
    const suggested = pathJoin(folder, "Wan2GP");
    appendLog(
      "[*] Drive root selected (" +
        folder +
        ") — using " +
        suggested +
        " instead.",
    );
    showToast("Using " + suggested);
    try {
      await window.w2gp.setDataDir(suggested);
    } catch (e) {
      appendLog("[!] Could not set install folder: " + ((e && e.message) || e));
      _pendingRoot = suggested;
      reflectRootBlock(suggested);
      return;
    }
    _pendingRoot = null;
    loadPaths();
    return;
  }
  _pendingRoot = null;
  try {
    await window.w2gp.setDataDir(folder);
  } catch (e) {
    if (/drive-root/.test((e && e.message) || String(e))) {
      _pendingRoot = folder;
      reflectRootBlock(folder);
      return;
    }
    throw e;
  }
  loadPaths();
});

$("clearAppDataPath")?.addEventListener("click", async () => {
  await window.w2gp.resetDataDir();
  loadPaths(true);
});

let _modelCkpts = "",
  _modelLoras = "",
  _modelOutput = "";

function setModelPath(type, folder) {
  const elMap = {
    ckpts: "installCkptsPath",
    loras: "installLorasPath",
    output: "installOutputPath",
  };
  const clearMap = {
    ckpts: "clearCkptsPath",
    loras: "clearLorasPath",
    output: "clearOutputPath",
  };
  const el = $(elMap[type]);
  const clearBtn = $(clearMap[type]);
  if (!el) return;
  if (folder) {
    el.textContent = folder;
    el.style.color = "";
    if (clearBtn) clearBtn.style.display = "";
    if (type === "ckpts") _modelCkpts = folder;
    else if (type === "loras") _modelLoras = folder;
    else _modelOutput = folder;
  } else {
    el.textContent = "(default)";
    el.style.color = "var(--text-tertiary)";
    if (clearBtn) clearBtn.style.display = "none";
    if (type === "ckpts") _modelCkpts = "";
    else if (type === "loras") _modelLoras = "";
    else _modelOutput = "";
  }
  // Model drive changed → re-gate disk space (installer screen only).
  try {
    if ($("installer") && $("installer").classList.contains("active"))
      refreshModelDiskGates().catch(() => {});
  } catch {}
}

async function browseModelFolder(type) {
  const folder = await window.w2gp.selectFolder();
  if (!folder) return;
  setModelPath(type, folder);
  // Persist BOTH: the user-facing choice (desktop-config.json, for the UI) AND
  // the file Wan2GP actually reads (wgp_config.json). Previously only the former
  // was written, so the Settings slider was cosmetic and downloads ignored it
  // (issue #74, "Model folders" always reverted to C:\Wan2GP-Models on refresh).
  if (type === "ckpts")
    await window.w2gp.writeWgpConfig({ checkpointsPaths: [folder, "."] });
  else if (type === "loras")
    await window.w2gp.writeWgpConfig({ lorasRoot: folder });
  else await window.w2gp.writeWgpConfig({ savePath: folder });
  const cfg = await window.w2gp.configLoad();
  if (type === "ckpts") cfg.modelCkptsPath = folder;
  else if (type === "loras") cfg.modelLorasPath = folder;
  else cfg.modelOutputPath = folder;
  await window.w2gp.configSave(cfg);
}

$("browseCkptsPath")?.addEventListener("click", () =>
  browseModelFolder("ckpts"),
);
$("browseLorasPath")?.addEventListener("click", () =>
  browseModelFolder("loras"),
);
$("clearCkptsPath")?.addEventListener("click", async () => {
  const p = await window.w2gp.getInstallPaths();
  const def = p?.modelsDefault
    ? pathJoin(p.modelsDefault, "ckpts")
    : "(default)";
  setModelPath("ckpts", "");
  const el = $("installCkptsPath");
  if (el) {
    el.textContent = def;
    el.style.color = "var(--text-tertiary)";
  }
  // Reset the real config too, so the UI and Wan2GP stay in sync (issue #74).
  await window.w2gp.writeWgpConfig({ checkpointsPaths: [def, "."] });
  const cfg = await window.w2gp.configLoad();
  delete cfg.modelCkptsPath;
  await window.w2gp.configSave(cfg);
});
$("clearLorasPath")?.addEventListener("click", async () => {
  const p = await window.w2gp.getInstallPaths();
  const def = p?.modelsDefault
    ? pathJoin(p.modelsDefault, "loras")
    : "(default)";
  setModelPath("loras", "");
  const el = $("installLorasPath");
  if (el) {
    el.textContent = def;
    el.style.color = "var(--text-tertiary)";
  }
  await window.w2gp.writeWgpConfig({ lorasRoot: def });
  const cfg = await window.w2gp.configLoad();
  delete cfg.modelLorasPath;
  await window.w2gp.configSave(cfg);
});
$("browseOutputPath")?.addEventListener("click", () =>
  browseModelFolder("output"),
);
$("clearOutputPath")?.addEventListener("click", async () => {
  const p = await window.w2gp.getInstallPaths();
  const def = p?.modelsDefault
    ? pathJoin(p.modelsDefault, "outputs")
    : "(default)";
  setModelPath("output", "");
  const el = $("installOutputPath");
  if (el) {
    el.textContent = def;
    el.style.color = "var(--text-tertiary)";
  }
  await window.w2gp.writeWgpConfig({ savePath: def });
  const cfg = await window.w2gp.configLoad();
  delete cfg.modelOutputPath;
  await window.w2gp.configSave(cfg);
});

async function startInstall() {
  if (_installRunning) return;
  const _restoreStartBtn = () => {
    const _b = $("installStartBtn");
    if (_b) {
      _b.disabled = false;
      _b.textContent = "Install";
    }
  };
  const _sb0 = $("installStartBtn");
  if (_sb0) {
    _sb0.disabled = true;
    _sb0.textContent = "Working…";
  }
  // Helper to show prereq help card
  function showPrereqHelp(title, text, url, tool) {
    $("prereqHelp").classList.remove("hidden");
    $("prereqTitle").textContent = title;
    $("prereqText").textContent = text;
    $("prereqDownloadBtn").onclick = async function () {
      this.disabled = true;
      this.textContent = "Installing...";
      appendLog("[*] Installing " + tool + "...");
      var r;
      try {
        r = await window.w2gp.installPrerequisite(tool);
      } catch (e) {
        r = { error: (e && e.message) || String(e) };
      } // never leave the button frozen
      this.disabled = false;
      this.textContent = "Download & Install";
      if (r && r.success) {
        if (r.ready) {
          // Tool is on PATH already (registry refresh) — continue automatically.
          $("prereqHelp").classList.add("hidden");
          showToast("✓ " + tool + " installed — continuing…");
          startInstall();
        } else
          showToast("✓ " + tool + " installed. Please restart the launcher.");
      } else showToast("✗ Install failed: " + (r?.error || "unknown"));
    };
    $("prereqManualBtn").onclick = () => {
      window.w2gp.openExternal(url);
    };
    $("installStartBtn").classList.remove("hidden");
    $("envTypeSelect").classList.remove("disabled");
    document
      .querySelectorAll(".env-type-btn")
      .forEach((b) => (b.disabled = false));
  }

  // Check prerequisites (check_command returns {cmd, found} — an object is
  // always truthy, so compare .found; previously a missing tool sailed through
  // and failed 10 minutes into setup.py instead of showing the help card).
  const hasCmd = async (c) => {
    try {
      const r = await window.w2gp.checkCommand(c);
      return !!(r && r.found);
    } catch {
      return false;
    }
  };
  var hasGit = await hasCmd("git");
  if (!hasGit) {
    appendLog("[!] Git not found — showing install help");
    showPrereqHelp(
      "Git not found",
      "Git is required to clone the Wan2GP repository. Click Download to install it silently, or use the manual button.",
      "https://git-scm.com/downloads",
      "git",
    );
    _restoreStartBtn();
    return;
  }
  if (selectedEnvType === "venv") {
    var hasPy = await hasCmd("python");
    if (!hasPy) {
      appendLog("[!] Python not found — showing install help");
      showPrereqHelp(
        "Python not found",
        "Python 3.10 or 3.11 is required for venv installs. Click Download to install Python 3.11 silently, or select uv/conda above.",
        "https://www.python.org/downloads/",
        "python",
      );
      _restoreStartBtn();
      return;
    }
  }
  if (selectedEnvType === "uv") {
    var hasUv = await hasCmd("uv");
    if (!hasUv) {
      appendLog("[!] uv not found — showing install help");
      showPrereqHelp(
        "uv not found",
        "uv is required for uv installs. Click Download to install it via PowerShell, or select venv/conda above.",
        "https://docs.astral.sh/uv/#installation",
        "uv",
      );
      _restoreStartBtn();
      return;
    }
  }
  if (selectedEnvType === "conda") {
    var hasConda = await hasCmd("conda");
    if (!hasConda) {
      appendLog("[!] Conda not found — showing install help");
      showPrereqHelp(
        "Conda not found",
        "Miniconda is required for conda installs. Click Download to install it silently, or select venv/uv above.",
        "https://docs.anaconda.com/miniconda/",
        "conda",
      );
      _restoreStartBtn();
      return;
    }
  }
  // Fresh-repo: collect the backup choice FIRST (modal never launches —
  // the wipe starts solely from the big Install button). Stored, then the
  // user presses Install again for the final are-you-sure + launch.
  // Applies to the no-env checklist AND the healthy-state trio.
  const pickedRadio = (name) =>
    (document.querySelector('input[name="' + name + '"]:checked') || {})
      .value || null;
  const liveFreshPick = () =>
    _targetChoiceMode === "repair-or-fresh"
      ? pickedRadio("targetChoice")
      : _targetChoiceMode === "reinstall-trio"
        ? pickedRadio("reinstallChoice")
        : null;
  const collectFreshBackup = async () => {
    appendLog("[*] Measuring existing installation folders…");
    const choice = await showReinstallBackupModal().catch(() => null);
    if (!choice) return false;
    _freshBackupChoice = choice;
    _freshBackupPick = { mode: _targetChoiceMode, value: liveFreshPick() };
    showToast("Backup choice saved — press Install to start");
    return true;
  };
  const freshPicked =
    (_targetChoiceMode === "repair-or-fresh" &&
      pickedRadio("targetChoice") === "fresh") ||
    (_targetChoiceMode === "reinstall-trio" &&
      pickedRadio("reinstallChoice") === "fresh");
  if (freshPicked && !_freshBackupChoice) {
    await collectFreshBackup();
    _restoreStartBtn();
    return;
  }
  // Are-you-sure gate: the Install button sits below the checks, and
  // nothing starts without explicit confirmation (fresh-repo wipes code).
  let choiceNote = "";
  let wipeWarn = "";
  if (_targetChoiceMode === "repair-or-fresh") {
    const checked = document.querySelector(
      'input[name="targetChoice"]:checked',
    );
    const isFresh = (checked && checked.value) === "fresh";
    choiceNote = isFresh
      ? "Fresh repo (wipe code, keep models)" +
        (_freshBackupChoice
          ? _freshBackupChoice.skip
            ? " — no backup"
            : " — with backup"
          : "")
      : "Install / repair environment (keeps models & settings)";
    if (isFresh && _freshBackupChoice && _freshBackupChoice.skip)
      wipeWarn =
        "\n⚠ WILL WIPE code, plugins, finetunes, settings and any models inside the folder.";
  } else if (_targetChoiceMode === "reinstall-trio") {
    const v = pickedRadio("reinstallChoice");
    choiceNote =
      v === "fresh"
        ? "Reinstall (fresh)" +
          (_freshBackupChoice
            ? _freshBackupChoice.skip
              ? " — no backup"
              : " — with backup"
            : "")
        : v === "skip"
          ? "Use existing (health-check)"
          : "Update & keep files";
    if (v === "fresh" && _freshBackupChoice && _freshBackupChoice.skip)
      wipeWarn =
        "\n⚠ WILL WIPE code, plugins, finetunes, settings and any models inside the folder.";
  }
  let locNote = "";
  try {
    const paths = await window.w2gp.getInstallPaths().catch(() => null);
    if (paths && (paths.repo || paths.dataDir))
      locNote = "\nLocation: " + (paths.repo || paths.dataDir);
  } catch {}
  if (
    !window.confirm(
      "Start the Wan2GP install now?" +
        (choiceNote ? "\nChoice: " + choiceNote : "") +
        "\nEnvironment: " +
        selectedEnvType +
        locNote +
        wipeWarn +
        "\n\nThis downloads several GB and takes 5–20 minutes.",
    )
  ) {
    _restoreStartBtn();
    return;
  }
  // Windows long paths gate (issue #15): enabling only takes effect
  // after a reboot, so offer it BEFORE any download — and stop here
  // when enabled, telling the user to reboot and re-run Install.
  // Covers every pipeline (AMD/Intel/NVIDIA share this entry point).
  try {
    const lp = await window.w2gp.tsLongPathsStatus().catch(() => null);
    if (lp && !lp.enabled) {
      const lpChoice = await window.w2gp.confirmDialog({
        title: "Enable Windows long paths?",
        message:
          "Long paths are OFF - deep ML package trees can fail mid-install past 260 chars. Enable now (needs admin approval)? You must reboot BEFORE installing for it to take effect.",
      });
      if (lpChoice === "ok" || lpChoice === 0) {
        try {
          const lr = await window.w2gp.tsLongPathsEnable();
          if (lr && (lr.ok || lr.already)) {
            appendLog(
              "[*] Long paths enabled - reboot Windows now, then run Install again.",
            );
            showToast("Long paths enabled - reboot, then Install again");
          } else {
            appendLog(
              "[!] Long paths enable failed: " +
                ((lr && lr.error) || "unknown") +
                " - see Manage → Troubleshooting.",
            );
          }
        } catch (e) {
          appendLog("[!] Long paths enable failed: " + errText(e));
        }
        _restoreStartBtn();
        return;
      }
      appendLog(
        "[!] Continuing without Windows long paths - mid-install failures past 260 chars are possible.",
      );
    }
  } catch {}
  // Checklist + trio dispatch: the big Install button is the ONLY launcher.
  // Fresh wipes consume the stashed backup choice (collected earlier) — the
  // wipe warning already lives in the single CONFIRM above, so dispatch
  // launches directly with no second dialog.
  const launchFresh = async () => {
    const live = liveFreshPick();
    const match =
      _freshBackupChoice &&
      _freshBackupPick &&
      _freshBackupPick.mode === _targetChoiceMode &&
      _freshBackupPick.value === live;
    if (!match) {
      // Live pick moved on (or no stash survived triage) — re-collect the
      // backup choice instead of launching stale, then wait for Press 2.
      _freshBackupChoice = null;
      _freshBackupPick = { mode: _targetChoiceMode, value: live };
      appendLog("[*] Measuring existing installation folders…");
      const choice = await showReinstallBackupModal().catch(() => null);
      if (!choice) {
        _restoreStartBtn();
        return;
      }
      _freshBackupChoice = choice;
      _freshBackupPick = { mode: _targetChoiceMode, value: liveFreshPick() };
      showToast("Backup choice saved — press Install to start");
      _restoreStartBtn();
      return;
    }
    const stored = _freshBackupChoice;
    _freshBackupChoice = null;
    _freshBackupPick = null;
    resetTasks();
    if (stored && stored.skip) {
      doInstall(null, "reinstall", { backup: false });
      return;
    }
    doInstall(null, "reinstall", stored);
    return;
  };
  if (_targetChoiceMode === "repair-or-fresh") {
    const checked = document.querySelector(
      'input[name="targetChoice"]:checked',
    );
    if ((checked && checked.value) === "fresh") {
      await launchFresh();
      return;
    }
    _freshBackupChoice = null;
    _freshBackupPick = null;
    resetTasks();
    doInstall(null, "update");
    return;
  }
  if (_targetChoiceMode === "reinstall-trio") {
    const v = pickedRadio("reinstallChoice");
    if (v === "fresh") {
      await launchFresh();
      return;
    }
    _freshBackupChoice = null;
    _freshBackupPick = null;
    resetTasks();
    doInstall(null, v === "skip" ? "skip" : "update");
    return;
  }
  show("installer");
  resetTasks();
  $("envTypeSelect").classList.add("disabled");
  document
    .querySelectorAll(".env-type-btn")
    .forEach((b) => (b.disabled = true));
  $("installStartBtn").classList.add("hidden");
  $("installSubtitle").textContent = "Setting up Wan2GP...";
  const installed = await window.w2gp.checkInstalled();
  if (installed.repo) {
    if (_lastVerdict === "foreign") {
      // Foreign folder with no checklist/trio: ask once whether to merge
      // upstream over the unknown files instead of bouncing to a re-render.
      const foreignChoice = await window.w2gp
        .confirmDialog({
          title: "Install into this folder anyway?",
          message:
            "Unknown files live here - upstream Wan2GP merges over them (empty folder is safer). Proceed?",
        })
        .catch(() => null);
      if (foreignChoice === "ok" || foreignChoice === 0) {
        doInstall(installed);
        return;
      }
      showToast("Cancelled - pick an empty folder with Browse to install");
      return;
    }
    // The verdict card owns the choices (healthy → Keep/Update/Skip trio,
    // broken → adopt/repair, pinokio → models reuse) so stale buttons can
    // never offer Keep/Skip for a folder that holds no install.
    await refreshTargetVerdict().catch(() => null);
    return;
  }
  doInstall(installed);
}

// Reinstall backup dialog: folder size + breakdown, backup checkbox,
// per-model Move-to rows with Browse. Resolves {backup, moveModels} |
// {skip:true} | null (cancel). Model destinations must be OUTSIDE the wiped folder.
function showReinstallBackupModal() {
  return new Promise((resolve) => {
    const modal = $("backupModal");
    if (!modal) {
      resolve({ backup: true, moveModels: [] });
      return;
    }
    const done = (v) => {
      modal.classList.add("hidden");
      resolve(v);
    };
    $("backupCloseBtn").onclick = () => done(null);
    $("backupCancelBtn").onclick = () => done(null);
    $("backupSkipBtn").onclick = () => done({ skip: true });
    const sumEl = $("backupSizeSummary"),
      bdEl = $("backupBreakdown");
    const secEl = $("backupModelsSection"),
      rowsEl = $("backupModelsRows");
    sumEl.textContent = "calculating…";
    bdEl.innerHTML = "";
    secEl.style.display = "none";
    rowsEl.innerHTML = "";
    $("backupIncludeCheckbox").checked = true;
    modal.classList.remove("hidden");
    // Gather: repo path, size breakdown, model locations. This async tail
    // runs in a fail-closed IIFE (never an async executor): any unexpected
    // throw cancels via done(null) instead of hanging the installer on a
    // never-settling promise. Callers already treat null as cancel.
    // (Tail keeps executor-level indent so the diff stays reviewable.)
    (async () => {
      const paths = await window.w2gp.getInstallPaths().catch(() => null);
      const repo = (paths && paths.repo) || "";
      let size = null;
      try {
        size = await window.w2gp.folderSize(repo);
      } catch (e) {
        size = { error: e.message };
      }
      if (!size || size.error) {
        sumEl.textContent =
          "size unavailable (" + ((size && size.error) || "unknown") + ")";
      } else {
        sumEl.textContent = fmtBytes(size.bytes) + " total";
        bdEl.textContent = "";
        for (const e of (size.entries || []).slice(0, 8)) {
          const d = document.createElement("div");
          d.className = "istack-row";
          const k = document.createElement("span");
          k.className = "istack-k";
          k.textContent = e.name;
          const v = document.createElement("span");
          v.className = "istack-v";
          v.textContent = fmtBytes(e.bytes);
          d.append(k, v);
          bdEl.append(d);
        }
      }
      const entryBytes = {};
      for (const e of (size && size.entries) || [])
        entryBytes[e.name.toLowerCase()] = e.bytes;
      // Which model folders live INSIDE the wiped repo?
      const mp = await window.w2gp.getModelPaths().catch(() => null);
      const norm = (p) => (p || "").replace(/\//g, "\\");
      const abs = (p) =>
        /^[A-Za-z]:\\/.test(p || "") || /\\\\/.test(p || "")
          ? norm(p)
          : norm(repo + "\\" + (p || ""));
      const inside = (p) => {
        const a = abs(p).toLowerCase();
        return a.startsWith(repo.toLowerCase().replace(/\\+$/, "") + "\\");
      };
      const found = [];
      const repoNorm = repo.toLowerCase().replace(/\\+$/, "");
      const push = (type, label, from, trusted) => {
        if (!from) return;
        const a = abs(from);
        if (a.toLowerCase() === repoNorm) return; // '.' == the repo itself — never offer to move it
        if (
          !inside(from) ||
          found.some((f) => f.from.toLowerCase() === a.toLowerCase())
        )
          return;
        found.push({ type, label, from: a, trusted: !!trusted });
      };
      if (mp) {
        push("ckpts", "Checkpoints", mp.checkpoints, true);
        push("loras", "LoRAs", mp.loras, true);
        push("output", "Output", mp.output, true);
      }
      // Default subdirs count too (ckpts/, loras/, outputs/ under the repo).
      for (const [sub, type, label] of [
        ["ckpts", "ckpts", "Checkpoints"],
        ["loras", "loras", "LoRAs"],
        ["outputs", "output", "Output"],
        ["output", "output", "Output"],
      ]) {
        push(type, label, repo + "\\" + sub, false);
      }
      // Untrusted (default-subdir) rows need a size entry proving they exist;
      // config-listed rows are trusted as-is.
      const rows = found.filter((f) => {
        if (f.trusted) return true;
        const base = f.from.split("\\").pop().toLowerCase();
        return Object.hasOwn(entryBytes, base);
      });
      const dsts = {};
      $("backupGoBtn").onclick = () => {
        const moveModels = [];
        rows.forEach((r, i) => {
          if (dsts[i])
            moveModels.push({ type: r.type, from: r.from, to: dsts[i] });
        });
        done({ backup: $("backupIncludeCheckbox").checked, moveModels });
      };
      if (!rows.length) return;
      secEl.style.display = "";
      rowsEl.innerHTML = "";
      rows.forEach((r, i) => {
        const base = r.from.split("\\").pop().toLowerCase();
        const div = document.createElement("div");
        div.className = "migrate-row";
        const lab = document.createElement("label");
        lab.textContent = r.label + " (" + fmtBytes(entryBytes[base]) + ")";
        const path = document.createElement("div");
        path.className = "migrate-path";
        const inp = document.createElement("input");
        inp.type = "text";
        inp.id = "backupDst" + i;
        inp.readOnly = true;
        inp.placeholder = "stays — will be deleted";
        const btn = document.createElement("button");
        btn.className = "btn btn-ghost small";
        btn.id = "backupBrowse" + i;
        btn.textContent = "Move to…";
        path.append(inp, btn);
        const hint = document.createElement("div");
        hint.className = "istack-hint";
        hint.textContent = r.from;
        div.append(lab, path, hint);
        rowsEl.appendChild(div);
        $("backupBrowse" + i).onclick = async () => {
          const dir = await window.w2gp.selectFolder().catch(() => null);
          if (!dir) return;
          if (isDriveRoot(dir)) {
            alert("Pick a folder, not a drive root.");
            return;
          }
          if (
            dir
              .toLowerCase()
              .startsWith(repo.toLowerCase().replace(/\\+$/, "") + "\\")
          ) {
            alert(
              "Destination must be OUTSIDE the wiped folder — it would be deleted too.",
            );
            return;
          }
          dsts[i] = dir;
          $("backupDst" + i).value = dir;
          $("backupDst" + i).title = dir;
        };
      });
    })().catch(() => done(null));
  });
}

async function doInstall(_installed, mode, opts) {
  $("reinstallChoice").classList.add("hidden");
  // Checklist verdict consumed — hide it so it can't be re-dispatched mid-install.
  _targetChoiceMode = null;
  _verdictModeShown = null;
  _installRunning = true;
  if ($("targetChoiceList")) $("targetChoiceList").style.display = "none";
  installProgressReset();
  if (mode === "skip") {
    // Reuse must earn it: a stale envs.json or half-deleted venv used to sail
    // through to a broken dashboard. Validate first, offer repair on failure.
    appendLog("[*] Checking existing install health before reuse…");
    let v = null;
    try {
      v = await window.w2gp.validateInstall();
    } catch (e) {
      v = { ok: false, errors: [e.message || String(e)] };
    }
    if (v && v.ok) {
      appendLog("[*] Existing install healthy — reusing.");
      _installRunning = false;
      show("dashboard");
      refreshDashboard();
      return;
    }
    appendLog(
      "[!] Existing install failed checks: " +
        ((v && v.errors && v.errors.join("; ")) || "unknown"),
    );
    $("installSubtitle").textContent =
      "Existing install needs repair — see issues above";
    _installRunning = false;
    try {
      await refreshTargetVerdict();
    } catch {}
    showToast(
      "✗ Existing install failed health checks — repair instead of reusing",
    );
    return;
  }
  const _diBtn = $("installStartBtn");
  if (_diBtn) {
    _diBtn.disabled = true;
    _diBtn.textContent = "Installing…";
  }
  document
    .querySelectorAll(".env-type-btn")
    .forEach((b) => (b.disabled = true));
  $("installSubtitle").textContent = "Starting installer — progress below…";
  let skipClone = false;
  if (mode === "reinstall") {
    $("installSubtitle").textContent = "Removing existing installation...";
    appendLog(
      "[*] Removing existing Wan2GP installation (large folders can take several minutes — wait for the next line)…",
    );
    const ok = await window.w2gp.reinstall(opts || null);
    if (ok && ok.movedModels && ok.movedModels.length) {
      appendLog("[*] Models relocated: " + ok.movedModels.join("; "));
      // Adopt the new locations so wgp_config.json points at them post-install.
      for (const m of (opts && opts.moveModels) || []) {
        if (m.type === "ckpts") _modelCkpts = m.to;
        else if (m.type === "loras") _modelLoras = m.to;
        else if (m.type === "output") _modelOutput = m.to;
      }
    }
    if (!ok) {
      appendLog(
        "[!] Reinstall aborted — the existing installation could not be removed (files likely locked by a running process or a terminal open in the folder).",
      );
      appendLog(
        "[!] Close any terminal/Explorer window open in the Wan2GP folder, then retry.",
      );
      showToast("✗ Could not remove existing installation");
      $("installSubtitle").textContent = "Setup Wan2GP";
      $("envTypeSelect").classList.remove("disabled");
      document
        .querySelectorAll(".env-type-btn")
        .forEach((b) => (b.disabled = false));
      $("installStartBtn").classList.remove("hidden");
      const _rbBtn = $("installStartBtn");
      if (_rbBtn) {
        _rbBtn.disabled = false;
        _rbBtn.textContent = "Install";
      }
      _installRunning = false;
      return;
    }
  } else if (mode === "update") {
    $("installSubtitle").textContent = "Update instead of fresh install...";
    skipClone = true;
  } else {
    // Fresh install (startInstall passes no mode): clone the repo normally —
    // previously this branch treated fresh installs as updates, showing
    // "Update instead of fresh install..." and marking the clone task done
    // before it had even run.
    skipClone = false;
  }
  if (skipClone) {
    taskComplete("clone");
    prevPhaseId = "clone";
  } else {
    taskStart("clone");
    prevPhaseId = "clone";
    appendLog("[*] Cloning Wan2GP repository...");
  }
  try {
    appendLog(
      "[*] Installing Wan2GP (environment: " + selectedEnvType + ")...",
    );
    await window.w2gp.install(selectedEnvType);
    try {
      const gpu = await window.w2gp.detectGpu();
      const hw = await window.w2gp.detectHardware();
      const name = (gpu.name || hw.gpu || "").toUpperCase();
      const vendor = gpu.vendor || "";
      let profile = "STANDARD";
      if (vendor === "APPLE") profile = "MPS";
      else if (name.match(/RTX 50|50\d0/)) profile = "RTX 50";
      else if (name.match(/RTX 40|40\d0/)) profile = "RTX 40";
      else if (name.match(/RTX 30|30\d0/)) profile = "RTX 30";
      else if (name.match(/RTX 20|20\d0/)) profile = "RTX 20";
      else if (name.includes("GTX") || name.includes("10")) profile = "GTX 10";
      else if (vendor === "AMD") profile = "AMD";
      $("installProfile").textContent = profile;
      $("installProfileRow").style.display = "flex";
    } catch {}
    try {
      // Fresh reinstall wiped the repo — merge the .reinstall-backup back first
      // (custom plugins/finetunes/old settings), then apply model paths on top.
      if (mode === "reinstall") {
        try {
          const rb = await window.w2gp.restoreBackup();
          if (rb && rb.restored && rb.restored.length)
            appendLog("[*] Restored from backup: " + rb.restored.join(", "));
        } catch (e) {
          appendLog(
            "[!] Backup restore failed: " +
              e.message +
              " — files remain in .reinstall-backup",
          );
        }
      }
      const modelCfg = {};
      if (_modelCkpts) modelCfg.checkpointsPaths = [_modelCkpts, "."];
      if (_modelLoras) modelCfg.lorasRoot = _modelLoras;
      if (_modelOutput) modelCfg.savePath = _modelOutput;
      await window.w2gp.writeWgpConfig(modelCfg);
      appendLog(
        `[*] wgp_config.json updated: ckpts=${_modelCkpts || "(default)"}, loras=${_modelLoras || "(default)"}`,
      );
    } catch (e) {
      appendLog(`[!] Failed to write model config: ${errText(e)}`);
    }
    taskComplete("done");
    $("installSubtitle").textContent = "Wan2GP is ready!";
    appendLog("[*] Installation complete!");
    _installRunning = false;
    const vb = $("validateInstallBtn");
    if (vb) {
      vb.style.display = "";
      vb.disabled = false;
      vb.textContent = "Validate installation";
    }
    setTimeout(() => {
      show("dashboard");
      refreshDashboard();
      startMetricsPolling();
    }, 1200);
  } catch (e) {
    // Honest failure: fail any still-running task, offer Retry + diagnostics.
    // (Previously only 'done' was marked and the Install button stayed hidden.)
    taskComplete("done", true);
    document.querySelectorAll(".task.active").forEach((t) => {
      t.className = "task fail";
      const ic = t.querySelector(".task-icon");
      if (ic) ic.textContent = "✕";
      const st = t.querySelector(".task-status");
      if (st) st.textContent = "failed";
    });
    $("installSubtitle").textContent =
      "Installation failed — see console output above";
    appendLog(`[ERROR] ${(e && e.message) || e}`);
    const sb = $("installStartBtn");
    if (sb) {
      sb.classList.remove("hidden");
      sb.disabled = false;
      sb.textContent = "Retry install";
    }
    const cdb = $("copyDiagnosticsBtn");
    if (cdb) {
      cdb.style.display = "";
      cdb.onclick = copyDiagnostics;
    }
    _installRunning = false;
    showToast("✗ Install failed — fix the issue above, then Retry");
  }
}

// Model-drive disk gates: checkpoints/LoRAs/outputs may live on other drives
// (or the same one) — the app-drive check in the install stack doesn't cover
// them, and a model library eats tens–hundreds of GB. Warn <50 GB, block <10.
function driveRootOf(p) {
  const m = /^([A-Za-z]:\\)/.exec(p || "");
  return m ? m[1].toUpperCase() : p || "";
}
let _gatesRun = 0;
async function refreshModelDiskGates() {
  const box = $("modelDiskGates");
  if (!box) return;
  const my = ++_gatesRun; // superseded runs abort before painting
  const targets = [
    ["Checkpoints", _modelCkpts],
    ["LoRAs", _modelLoras],
    ["Output", _modelOutput],
  ].filter((t) => t[1] && !isDriveRoot(t[1]));
  if (!targets.length) {
    if (my === _gatesRun) {
      box.innerHTML = "";
      window._modelDriveBlocked = false;
    }
    return;
  }
  const seen = new Set();
  const frag = document.createDocumentFragment();
  let blocked = false;
  for (const [label, p] of targets) {
    const root = driveRootOf(p);
    if (seen.has(root)) continue;
    seen.add(root);
    let d = null;
    try {
      d = await window.w2gp.getDiskSpace(p);
    } catch {
      continue;
    }
    if (my !== _gatesRun) return;
    if (!d || d.free == null || d.total == null) continue;
    const gb = d.free / 1073741824;
    if (gb < 10) {
      blocked = true;
      const d = document.createElement("div");
      d.className = "istack-w";
      d.textContent =
        "⛔ " +
        label +
        " drive " +
        root +
        " has only " +
        gb.toFixed(1) +
        " GB free — a model library needs tens of GB. Pick a roomier drive.";
      frag.append(d);
    } else if (gb < 50) {
      const d = document.createElement("div");
      d.className = "istack-hint";
      d.textContent =
        "⚠ " +
        label +
        " drive " +
        root +
        ": " +
        gb.toFixed(1) +
        " GB free — tight for a model library.";
      frag.append(d);
    }
  }
  if (my !== _gatesRun) return;
  box.textContent = "";
  box.append(frag);
  window._modelDriveBlocked = blocked;
  const startBtn = $("installStartBtn");
  if (blocked && startBtn) {
    startBtn.disabled = true;
    startBtn.title = "Free space on the model drive(s) before installing";
    startBtn.textContent = "Install blocked — model drive full";
  }
}

// Copy a diagnostics bundle (hardware + paths + python preflight + log tail)
// for Discord/GitHub reports — ATFGriff-class issues arrive without this.
async function copyDiagnostics() {
  let info = "";
  try {
    const parts = await Promise.all([
      window.w2gp.detectHardware().catch(() => null),
      window.w2gp.getInstallPaths().catch(() => null),
      window.w2gp.pythonPreflight().catch(() => null),
      window.w2gp.classifyTarget().catch(() => null),
    ]);
    info =
      "Hardware: " +
      JSON.stringify(parts[0]) +
      "\nPaths: " +
      JSON.stringify(parts[1]) +
      "\nPython: " +
      JSON.stringify(parts[2]) +
      "\nTarget: " +
      JSON.stringify(parts[3]) +
      "\n\n";
  } catch {}
  const tail =
    typeof window._getLogTail === "function" ? window._getLogTail() : "";
  try {
    await navigator.clipboard.writeText(info + tail);
    showToast("✓ Diagnostics copied — paste it in Discord/GitHub");
  } catch {
    showToast("✗ Copy failed — select the console text manually");
  }
}

// Target-folder triage UI: what is already in the install location?
// Verdicts from classify_target: empty | ours_healthy | ours_broken_env |
// repo_no_env | pinokio | foreign.
async function refreshTargetVerdict() {
  const box = $("targetVerdict"),
    body = $("targetVerdictBody");
  const choiceList = $("targetChoiceList"),
    browse = $("targetBrowseBtn"),
    useModels = $("targetUseModelsBtn");
  if (!box || !body) return;
  let t = null;
  try {
    t = await window.w2gp.classifyTarget();
  } catch {
    _lastVerdict = null;
    box.style.display = "none";
    return null;
  }
  if (!t || !t.verdict) {
    _lastVerdict = null;
    box.style.display = "none";
    return null;
  }
  const v = t.verdict;
  _lastVerdict = v;
  const newMode =
    v === "ours_healthy"
      ? "reinstall-trio"
      : v === "repo_no_env" || v === "ours_broken_env"
        ? "repair-or-fresh"
        : null;
  // Only force-check the default radio when the verdict MODE changed — a
  // same-mode refresh (Browse re-triage) leaves the user's pick alone.
  const verdictModeChanged = newMode !== _verdictModeShown;
  _verdictModeShown = newMode;
  if (choiceList) choiceList.style.display = "none";
  if (browse) browse.style.display = "none";
  if (useModels) useModels.style.display = "none";
  _targetChoiceMode = null;
  const startBtn = $("installStartBtn");
  if (v === "empty") {
    box.style.display = "none";
    // No install here → no Keep / reuse choices either.
    $("reinstallChoice")?.classList.add("hidden");
    // Restore the label if a previous verdict changed it (pinokio/foreign only —
    // never touch driver/disk hard blocks, the installPlan block owns those).
    if (
      startBtn &&
      (startBtn.textContent.startsWith("Install anyway") ||
        startBtn.textContent.startsWith("Install blocked — Pinokio"))
    ) {
      startBtn.textContent = "Install";
      startBtn.disabled = false;
      startBtn.title = "";
    }
    return t;
  }
  box.style.display = "";
  const envNames = (t.envs && Object.keys(t.envs).join(", ")) || "";
  if (v === "ours_healthy") {
    body.textContent = "";
    {
      const d = document.createElement("div");
      d.className = "istack-ok";
      d.textContent = "✓ " + (t.hint || "");
      body.append(d);
    }
    // Reuse path: the choice lives in the trio radios, the big Install
    // button dispatches it — same contract as the no-env checklist.
    $("reinstallChoice")?.classList.remove("hidden");
    const trio = document.querySelector(
      'input[name="reinstallChoice"][value="update"]',
    );
    if (verdictModeChanged && trio) trio.checked = true;
    _targetChoiceMode = "reinstall-trio";
    if (startBtn && !_installRunning) {
      startBtn.classList.remove("hidden");
      if (!startBtn.disabled) {
        startBtn.textContent = "Install";
        startBtn.title = "";
      }
    }
    $("installSubtitle").textContent = "Wan2GP is already installed.";
  } else if (v === "repo_no_env" || v === "ours_broken_env") {
    // Checklist mode: the choice lives in the radios, the big Install button
    // dispatches it (see startInstall) — no competing action buttons.
    // Adopt-cover note: the generic Keep/Update/Skip trio doesn't apply.
    $("reinstallChoice")?.classList.add("hidden");
    body.textContent = "";
    {
      const d = document.createElement("div");
      d.className = "istack-w";
      d.textContent = "⚠ " + (t.hint || "");
      body.append(d);
    }
    if (envNames) {
      const d = document.createElement("div");
      d.className = "istack-hint";
      d.textContent =
        "Env folders found: " +
        envNames +
        " — repair recreates the broken one, keeps models & settings.";
      body.append(d);
    }
    {
      const d = document.createElement("div");
      d.className = "istack-hint";
      d.textContent = "Tick your choice below, then press Install.";
      body.append(d);
    }
    if (choiceList) {
      choiceList.style.display = "";
      const repair = choiceList.querySelector('input[value="repair"]');
      if (verdictModeChanged && repair) repair.checked = true;
    }
    _targetChoiceMode = "repair-or-fresh";
    // Re-arm the big button (the first Install pass hid it to show this card).
    // Never override a hard block owned elsewhere (disk gates, pinokio, roots),
    // and never resurrect it while an install is running.
    if (startBtn && !_installRunning) {
      startBtn.classList.remove("hidden");
      if (!startBtn.disabled) {
        startBtn.textContent = "Install";
        startBtn.title = "";
      }
    }
    $("installSubtitle").textContent =
      "Wan2GP repo found — environment missing or broken.";
  } else {
    // pinokio | foreign
    const isPinokio = v === "pinokio";
    const icon = isPinokio ? "🧩" : "⚠";
    let detail =
      "Folder: " + (t.repo || "") + " (" + (t.entryCount || 0) + " entries";
    if (t.hasConfig) detail += ", has wgp_config.json";
    if (t.modelDirs && t.modelDirs.length)
      detail += ", model dirs: " + t.modelDirs.join(", ");
    detail += ").";
    // For Pinokio libraries, show what we found + sizes (proves reuse is worth it).
    if (isPinokio && t.modelDirs && t.modelDirs.length) {
      try {
        const sz = await window.w2gp.folderSize(t.repo).catch(() => null);
        const byName = {};
        for (const e of (sz && sz.entries) || [])
          byName[e.name.toLowerCase()] = e.bytes;
        const lines = t.modelDirs.map((d) => {
          const b = byName[d.toLowerCase()];
          return d + (b == null ? "" : " (" + fmtBytes(b) + ")");
        });
        detail += " Reusable library: " + lines.join(", ") + ".";
      } catch {}
    }
    body.textContent = "";
    {
      const d = document.createElement("div");
      d.className = "istack-w";
      d.textContent = icon + " " + (t.hint || "");
      body.append(d);
    }
    {
      const d = document.createElement("div");
      d.className = "istack-hint";
      d.textContent =
        detail +
        (isPinokio
          ? ""
          : " Installing here merges upstream over unknown files — an empty folder is safer.");
      body.append(d);
    }
    if (browse) {
      browse.style.display = "";
      browse.onclick = () => {
        $("browseAppDataPath")?.click();
      };
    }
    // Foreign content → no Keep / reuse choices either.
    $("reinstallChoice")?.classList.add("hidden");
    if (isPinokio) {
      // Hard stop: backend install/reinstall/uninstall refuse Pinokio trees too.
      if (startBtn) {
        startBtn.disabled = true;
        startBtn.title =
          "Pick an empty folder first — installing into a Pinokio tree would corrupt it.";
        startBtn.textContent = "Install blocked — Pinokio folder";
      }
      $("reinstallChoice")?.classList.add("hidden");
      if (useModels && t.modelDirs && t.modelDirs.length) {
        useModels.style.display = "";
        useModels.onclick = () => {
          usePinokioModels(t);
        };
      }
      $("installSubtitle").textContent =
        "Pinokio-managed Wan2GP found — reuse its models in a fresh install below.";
    } else if (startBtn && !startBtn.disabled) {
      startBtn.textContent = "Install anyway (folder not empty)";
    }
    if (v === "foreign")
      $("installSubtitle").textContent =
        "This folder holds unknown files — install into an empty folder, or wipe it first.";
  }
  return t;
}

// Pinokio reuse: point OUR model folders at the Pinokio library (no re-downloads,
// Pinokio keeps working untouched), then let the user pick an empty install folder.
// NOTE: writes desktop-config only — wgp_config.json here belongs to Pinokio.
async function usePinokioModels(t) {
  const repo = (t && t.repo) || "";
  const sep = "\\";
  const pick = async (type, sub) => {
    const p = repo.replace(/\\+$/, "") + sep + sub;
    setModelPath(type, p);
    try {
      const cfg = await window.w2gp.configLoad();
      if (type === "ckpts") cfg.modelCkptsPath = p;
      else if (type === "loras") cfg.modelLorasPath = p;
      else cfg.modelOutputPath = p;
      await window.w2gp.configSave(cfg);
    } catch {}
  };
  const dirs = (t && t.modelDirs) || [];
  // Map upstream subdir names to our folder types.
  for (const d of dirs) {
    const low = d.toLowerCase();
    if (low === "ckpts" || low === "checkpoints") await pick("ckpts", d);
    else if (low === "loras") await pick("loras", d);
    else if (low === "outputs" || low === "output") await pick("output", d);
  }
  appendLog(
    "[*] Model folders now point at the Pinokio library — its install stays untouched.",
  );
  showToast("✓ Reusing Pinokio models — now pick an empty install folder");
  try {
    await refreshModelDiskGates();
  } catch {}
  $("browseAppDataPath")?.click();
}

// (Re)open the installer screen in a fresh state — used by Manage → Run Setup
// again and after uninstall. Never lands on the dashboard without an install.
async function openInstallerFresh(subtitle) {
  resetTasks();
  installProgressReset();
  show("installer");
  $("installSubtitle").textContent =
    subtitle || "Select environment type, then click Install";
  const sb = $("installStartBtn");
  if (sb) {
    sb.classList.remove("hidden");
    sb.disabled = false;
    sb.textContent = "Install";
  }
  $("envTypeSelect")?.classList.remove("disabled");
  document
    .querySelectorAll(".env-type-btn")
    .forEach((b) => (b.disabled = false));
  await loadPaths().catch(() => null);
  try {
    await refreshTargetVerdict().catch(() => null);
  } catch {}
  try {
    await refreshModelDiskGates().catch(() => null);
  } catch {}
}

$("manageRunSetupBtn")?.addEventListener("click", async () => {
  closeSettings();
  appendLog(
    "[*] Opening Setup — pick fresh install, repair, reuse or migrate.",
  );
  await openInstallerFresh();
});

$("settingsOverlay").addEventListener("click", () => {
  closeSettings();
  // closeGuide is defined below the overlay wiring — guard for load order.
  if (typeof closeGuide === "function") closeGuide();
});

// ── Dashboard ──
let _dashRefreshing = false,
  _dashPending = false;
async function refreshDashboard() {
  if (_dashRefreshing) {
    _dashPending = true;
    return;
  }
  _dashRefreshing = true;
  try {
    // status / checkInstalled / manageList are independent — run them in one
    // batch instead of 3 sequential IPC round-trips (~2-6ms saved each, more
    // when the machine is under load from a running install).
    const [status, instRes, envs] = await Promise.all([
      window.w2gp.getStatus(),
      window.w2gp.checkInstalled().catch(() => null),
      window.w2gp.manageList().catch(() => []),
    ]);
    // Dashboard renderer switch: reflect the saved embedMode (same source as
    // Manage → Launch and the topbar quick-switch).
    try {
      const _cfg = await window.w2gp.configLoad().catch(() => ({}));
      const _et = $("embedModeTop");
      if (_et)
        _et.value = _cfg && _cfg.embedMode === "iframe" ? "iframe" : "native";
    } catch {}
    // Launch buttons only make sense when Wan2GP is actually installed
    try {
      // Launch buttons need a repo AND an active env — repo alone (failed
      // install, wiped env) used to launch system `python` into a torch traceback.
      setLaunchButtonsInstalled(
        !!(instRes && instRes.repo && status.env && !status.error),
      );
    } catch {}
    // Show a visible error note if the status call failed (so the panel is never
    // silently blank — this is exactly the blank-dashboard bug we hit before).
    const errNote = $("envDetailError");
    if (errNote) errNote.style.display = status.error ? "" : "none";
    if (status.error || !status.env) {
      if (errNote)
        errNote.textContent =
          "Could not read environment status: " +
          (status.error || "no active environment");
      $("envName").textContent = "No active environment";
      window._activeEnvName = "";
      window._activeEnvType = "";
      window._hasActiveEnv = false;
      $("envNameHint")?.classList.remove("hidden");
      document
        .querySelectorAll(".pkg-install-btn, .spec-latest, .spec-update-btn")
        .forEach((el) => {
          el.remove();
        });
      [
        "specPython",
        "specTorch",
        "specCuda",
        "specTriton",
        "specSage",
        "specFlash",
        "specDiffusers",
        "specTransformers",
        "specGradio",
        "specAccelerate",
        "specOnnx",
        "specOpencv",
        "specPeft",
        "specHfhub",
        "specBits",
        "specNumpy",
        "specTokenizers",
        "specMmgp",
        "specXformers",
        "specTorchaudio",
        "specMoviepy",
        "specSparge",
      ].forEach((id) => {
        const el = $(id);
        if (el) el.textContent = "—";
      });
      [
        "dotPython",
        "dotTorch",
        "dotCuda",
        "dotTriton",
        "dotSage",
        "dotFlash",
        "dotDiffusers",
        "dotTransformers",
        "dotGradio",
        "dotAccelerate",
        "dotOnnx",
        "dotOpencv",
        "dotPeft",
        "dotHfhub",
        "dotBits",
        "dotNumpy",
        "dotTokenizers",
        "dotMmgp",
        "dotXformers",
        "dotTorchaudio",
        "dotMoviepy",
      ].forEach((id) => {
        const el = $(id);
        if (el) el.classList.remove("installed");
      });
      // Kernel wheels section is independent — keep it rendered from whatever we got.
      renderKernelWheels(
        status.kernelWheels,
        status.kernelProfile,
        status.osKey,
      );
      const spargeEl = $("specSparge");
      if (spargeEl) spargeEl.textContent = "—";
    } else {
      $("envName").textContent = status.env.name;
      $("envType").textContent = status.env.type;
      window._activeEnvName = status.env.name || "";
      window._activeEnvType = status.env.type || "uv";
      window._hasActiveEnv = true;
      $("envNameHint")?.classList.add("hidden");
      // Clear old update/install buttons before re-creating
      document
        .querySelectorAll(".spec-latest, .spec-update-btn, .pkg-install-btn")
        .forEach((el) => {
          el.remove();
        });

      // AMD guard: CUDA / bitsandbytes / vanilla PyPI triton / vanilla
      // spas_sage_attn / PyPI sdist flash-attn break the TheRock env —
      // hide the one-click add button on AMD profiles with a tooltip
      // pointing at the guide recipe (backend refuses them too).
      // NVIDIA behavior is identical to before.
      var isAmdProfile =
        typeof status.kernelProfile === "string" &&
        status.kernelProfile.indexOf("AMD") === 0;
      var amdBlockedPkgs = [
        "bitsandbytes",
        "triton",
        "spas_sage_attn",
        "flash-attn",
      ];
      function setSpec(specId, dotId, val, pkgName) {
        const el = $(specId);
        if (el) el.textContent = val || "—";
        const dot = $(dotId);
        if (dot) {
          if (val) {
            dot.classList.remove("has-update", "error", "installing");
            dot.classList.add("installed");
          } else dot.classList.remove("installed");
        }
        // Show install button if package is missing and we know its pip name
        if (!val && pkgName && el) {
          // AMD: no one-click button for dists that break the TheRock
          // env (backend refuses them too) — tooltip notes the guide.
          if (isAmdProfile && amdBlockedPkgs.indexOf(pkgName) !== -1) {
            var parent0 = el.closest(".spec-row");
            if (parent0) {
              var old0 = parent0.querySelector(".pkg-install-btn");
              if (old0) old0.remove();
            }
            el.title =
              "Not available on AMD — see the docs/AMD-INSTALLATION.md guide recipe";
            return;
          }
          var parent = el.closest(".spec-row");
          if (parent) {
            var oldBtn = parent.querySelector(".pkg-install-btn");
            if (oldBtn) oldBtn.remove();
            var btn = document.createElement("button");
            btn.className = "pkg-install-btn";
            btn.textContent = "+";
            btn.title = "Install " + pkgName;
            btn.addEventListener("click", async function (ev) {
              ev.stopPropagation();
              this.disabled = true;
              this.textContent = "...";
              var res = await window.w2gp.installPackage(pkgName);
              if (res && res.success) {
                this.textContent = "✓";
                this.classList.add("done");
                setTimeout(refreshDashboard, 2000);
              } else {
                this.textContent = "+";
                this.disabled = false;
                showToast(
                  "✗ Install failed: " +
                    (res && res.error ? res.error : "unknown"),
                );
              }
            });
            el.after(btn);
          }
        }
      }
      // If the version query itself failed, show the reason in the note but keep
      // the wheels/paths sections alive (they're independent of the version scan).
      if (status.versions && status.versions.error) {
        const errNote = $("envDetailError");
        if (errNote) {
          errNote.style.display = "";
          errNote.textContent = "Package scan failed: " + status.versions.error;
        }
      }
      setSpec("specPython", "dotPython", status.versions?.python);
      setSpec("specTorch", "dotTorch", status.versions?.torch);
      const m = (status.versions?.torch || "").match(/cu(\d+)/);
      setSpec("specCuda", "dotCuda", m ? `CUDA ${m[1]}` : null);
      setSpec("specTriton", "dotTriton", status.versions?.triton, "triton");
      setSpec(
        "specSage",
        "dotSage",
        status.versions?.sageattention || status.versions?.spas_sage_attn,
        "spas_sage_attn",
      );
      setSpec(
        "specFlash",
        "dotFlash",
        status.versions?.flash_attn,
        "flash-attn",
      );
      setSpec("specDiffusers", "dotDiffusers", status.versions?.diffusers);
      setSpec(
        "specTransformers",
        "dotTransformers",
        status.versions?.transformers,
      );
      setSpec("specGradio", "dotGradio", status.versions?.gradio);
      setSpec("specAccelerate", "dotAccelerate", status.versions?.accelerate);
      setSpec("specOnnx", "dotOnnx", status.versions?.onnxruntime);
      setSpec("specOpencv", "dotOpencv", status.versions?.["opencv-python"]);
      setSpec("specPeft", "dotPeft", status.versions?.peft);
      setSpec("specHfhub", "dotHfhub", status.versions?.huggingface_hub);
      setSpec(
        "specBits",
        "dotBits",
        status.versions?.bitsandbytes,
        "bitsandbytes",
      );
      setSpec("specNumpy", "dotNumpy", status.versions?.numpy);
      setSpec("specTokenizers", "dotTokenizers", status.versions?.tokenizers);
      setSpec("specMmgp", "dotMmgp", status.versions?.mmgp);
      setSpec("specXformers", "dotXformers", status.versions?.xformers);
      setSpec("specTorchaudio", "dotTorchaudio", status.versions?.torchaudio);
      setSpec("specMoviepy", "dotMoviepy", status.versions?.moviepy);

      // ── GPU Kernel Wheels (profile-driven) ──
      renderKernelWheels(
        status.kernelWheels,
        status.kernelProfile,
        status.osKey,
      );
      // Sparge Attn comes from the expected GPU profile (not a detected version),
      // so it's surfaced here to avoid a separate duplicate "GPU Profile Overview".
      const spargeEl = $("specSparge");
      // ponytail: show installed 0.1.0 if present, else expected v010_cu13 profile tag
      if (spargeEl)
        spargeEl.textContent =
          status.versions?.spas_sage_attn ||
          status.versions?.sparge ||
          (status.profile && status.profile.sparge) ||
          status.kernelProfile ||
          "—";
    }
    // ponytail: batch DOM swap to avoid flicker — build fragment then single replace
    const list = $("envList");
    const frag = document.createDocumentFragment();
    envs.forEach((e) => {
      const div = document.createElement("div");
      div.className = "env-list-item" + (e.active ? " active" : "");
      {
        const dot = document.createElement("span");
        dot.className = "env-dot";
        const nm = document.createElement("span");
        nm.className = "env-list-name";
        nm.textContent = e.name;
        const ty = document.createElement("span");
        ty.style.cssText = "font-size:0.65rem;color:#666;flex-shrink:0";
        ty.textContent = e.type;
        div.append(dot, nm, ty);
      }
      if (!e.active) {
        div.setAttribute("role", "button");
        div.tabIndex = 0;
        const activate = async () => {
          await window.w2gp.manageSetActive(e.name);
          refreshDashboard();
        };
        div.addEventListener("click", activate);
        div.addEventListener("keydown", (ev) => {
          if (ev.key === "Enter" || ev.key === " ") {
            ev.preventDefault();
            activate();
          }
        });
      }
      frag.appendChild(div);
    });
    list.innerHTML = "";
    list.appendChild(frag);
    loadWangpChangelog();
    loadPaths();
    loadModelPaths();
    document.querySelectorAll(".env-detail .spec-row").forEach((r) => {
      r.classList.remove("has-update", "up-to-date");
    });
    $("checkPkgUpdatesBtn").textContent = "↻ Check Updates";
    $("checkPkgUpdatesBtn").disabled = false;
    refreshEnvUnlink(!!(instRes && instRes.repo));
    // Warn if model checkpoints/LoRAs still live in a roaming AppData profile.
    checkModelsPathWarning();
    // Warn RTX 40/50 users still on the broken fp8 SageAttention wheel to sync.
    checkSageSyncBanner(status);
    // Refresh the guided LLM engine cards (Deepy Prime setup).
    refreshLLMEngines().catch(() => {});
    // Refresh the Deepy Prime activation panel.
    refreshDeepy().catch(() => {});
    // Refresh the Deepy Web standalone card (Same-PC + Phone-LAN HTTP).
    refreshDeepyWeb().catch(() => {});
    // Refresh the DLSS5 optional-runtime status.
    refreshDlss5().catch(() => {});
    // Enable/disable no-GPU button based on Chrome availability. A single
    // negative is never trusted for failure UI: a cold first spawn (AV hooks,
    // process-creation stalls) can fail once and would flash "not installed"
    // for a second. Re-probe immediately — probes are synchronous file checks
    // plus `where`, so this costs milliseconds. Only a repeated negative
    // disables the button and shows the hint. IPC errors leave UI untouched.
    (async () => {
      const probe = async () => {
        try {
          if (window.w2gp.noGpuAvailable)
            return await window.w2gp.noGpuAvailable();
          return await window.w2gp.chromeAvailable();
        } catch {
          return null;
        }
      };
      let available = await probe();
      if (available === false) available = await probe();
      if (available === null) return;
      // Flake guard: a single negative probe (common at cold start) must not
      // flash "no browser installed" — show only after 2 consecutive misses.
      window._noGpuMissCount =
        available === false ? (window._noGpuMissCount || 0) + 1 : 0;
      const noBrowser = available === false && window._noGpuMissCount >= 2;
      if (noBrowser)
        appendLog(
          "[!] No-GPU browser probe: none found twice (No-GPU launches disabled)",
        );
      else if (available === true && window._noGpuWasMissing)
        appendLog(
          "[*] No-GPU browser probe: found on re-probe (first probe flaked)",
        );
      window._noGpuWasMissing = available === false;
      for (const id of ["browserNoGpuBtn", "termNoGpuBtn"]) {
        const btn = $(id);
        if (btn) btn.disabled = noBrowser;
      }
      const hint = $("noGpuHint");
      if (hint) hint.style.display = noBrowser ? "block" : "none";
    })();
    // Self-healing first-launch info bar: repaint from STATE on every
    // dashboard refresh so any runtime path that left it hidden (fresh load
    // never shows it; exit/stop paths don't restore it) is corrected.
    // Instant feedback still comes from the explicit show/hide calls at
    // launch-click/error/ready sites — this only corrects drift.
    paintLaunchInfo();
  } finally {
    _dashRefreshing = false;
    if (_dashPending) {
      _dashPending = false;
      setTimeout(refreshDashboard, 80);
    }
  }
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
  const list = Array.isArray(wheels) ? wheels : [];
  if (!list.length) {
    // Distinguish "no GPU profile" (genuinely nothing to show) from a data
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
    return;
  }
  card.style.display = "";
  if (tag && kernelProfile) tag.textContent = kernelProfile;
  box.innerHTML = "";
  list.forEach((w) => {
    // ponytail: Tauri spike returns string array; Electron returns objects — handle both
    if (typeof w === "string")
      w = { key: w, label: w, pipName: w, state: "missing" };
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

// ── Env unlink button visibility ──
// Shown only when a repo is present AND an env is known-active: with no
// install the buttons used to stay clickable and fail (or "clean" a stale
// registry entry with no context).
function refreshEnvUnlink(hasRepo) {
  var btn = $("envUnlinkBtn");
  var restoreBtn = $("envRestoreBtn");
  var reinstallBtn = $("envReinstallBtn");
  var setupBtn = $("envSetupBtn");
  var hideAll = () => {
    if (btn) btn.style.display = "none";
    if (restoreBtn) restoreBtn.style.display = "none";
    if (reinstallBtn) reinstallBtn.style.display = "none";
    if (setupBtn) setupBtn.style.display = "none";
  };
  if (hasRepo === false) {
    hideAll();
    return;
  }
  // State-driven: shown whenever an env is known-active, hidden otherwise.
  var hasEnv = window._hasActiveEnv === true;
  var name = (hasEnv && window._activeEnvName) || "";
  if (btn) {
    if (name && name !== "—" && name !== "No active environment") {
      if (setupBtn) setupBtn.style.display = "none";
      btn.style.display = "";
      if (restoreBtn) restoreBtn.style.display = "";
      if (reinstallBtn) reinstallBtn.style.display = "";
      btn.onclick = async () => {
        if (!confirm('Uninstall environment "' + name + '"?')) return;
        btn.disabled = true;
        btn.textContent = "...";
        appendLog("[*] Uninstalling environment " + name + "...");
        try {
          var r = await window.w2gp.uninstallEnv(name);
          if (r && r.success) {
            appendLog("[*] Environment " + name + " uninstalled.");
            await refreshDashboard();
            if (window._hasActiveEnv === false)
              appendLog(
                '[*] No environments remaining — click "🧭 Run Setup" in the Active Environment card to install a fresh one.',
              );
          } else showToast((r && r.error) || "Failed");
        } catch (e) {
          showToast(errText(e));
        }
        btn.disabled = false;
        btn.textContent = "unlink";
      };
    } else {
      hideAll();
      // No active env (e.g. just unlinked the last one): offer the
      // installer directly — same destination as Manage → Run Setup.
      if (setupBtn) {
        setupBtn.style.display = "";
        setupBtn.onclick = () => {
          openInstallerFresh();
        };
      }
    }
  }
  // Restore button handler
  if (restoreBtn) {
    restoreBtn.onclick = async () => {
      if (
        !confirm(
          "Reinstall all packages from requirements.txt? This will restore pinned versions.",
        )
      )
        return;
      restoreBtn.disabled = true;
      restoreBtn.textContent = "...";
      appendLog("[*] Restoring packages from requirements.txt...");
      try {
        var r = await window.w2gp.restoreRequirements();
        if (r && r.success) {
          appendLog("[*] Requirements restored.");
          hideDriftBanner();
          setTimeout(refreshDashboard, 2000);
        } else showToast((r && r.error) || "Failed");
      } catch (e) {
        showToast(errText(e));
      }
      restoreBtn.disabled = false;
      restoreBtn.textContent = "restore";
    };
  }
  // Full reinstall: delete the env and run the whole setup again (fresh
  // venv, pinned Python, PyTorch+CUDA, requirements, kernels, smoke test).
  // Restore only re-pips requirements into the existing venv — this fixes
  // broken interpreters, wrong torch builds and corrupt venvs. Models,
  // plugins and settings are untouched (they live outside the env folder).
  if (reinstallBtn) {
    reinstallBtn.onclick = async () => {
      const envType = window._activeEnvType || "uv";
      if (
        !window.confirm(
          'Recreate the "' +
            name +
            '" environment from scratch?\n\nFresh venv, Python, PyTorch + CUDA, packages and kernels (takes a while — watch the console). Models, plugins and settings are kept.',
        )
      )
        return;
      reinstallBtn.disabled = true;
      reinstallBtn.textContent = "working…";
      appendLog(
        "[*] Reinstalling environment " +
          name +
          " from scratch (" +
          envType +
          ") — progress below…",
      );
      try {
        const u = await window.w2gp.uninstallEnv(name);
        if (!u || !u.success)
          throw new Error((u && u.error) || "env removal failed");
        appendLog("[*] Old env removed — running full setup…");
        const r = await window.w2gp.install(envType);
        if (r && (r.success || r.ok)) {
          appendLog("[*] Environment reinstalled.");
          showToast("✓ Environment reinstalled");
        } else
          showToast(
            "✗ " + ((r && r.error) || "reinstall failed — see console"),
          );
      } catch (e) {
        appendLog("[!] Reinstall failed: " + errText(e));
        showToast("✗ " + errText(e));
      }
      reinstallBtn.disabled = false;
      reinstallBtn.textContent = "reinstall";
      refreshDashboard();
    };
  }
}

const _labelToKey = {
  Python: "python",
  Torch: "torch",
  CUDA: "cuda",
  Triton: "triton",
  "Sage Attn": "sageattention",
  "Flash Attn": "flash_attn",
  Diffusers: "diffusers",
  Transformers: "transformers",
  Gradio: "gradio",
  Accelerate: "accelerate",
  onnxruntime: "onnxruntime",
  OpenCV: "opencv-python",
  PEFT: "peft",
  hf_hub: "huggingface_hub",
  bitsandbytes: "bitsandbytes",
  NumPy: "numpy",
  Tokenizers: "tokenizers",
};

$("checkPkgUpdatesBtn").addEventListener("click", async function () {
  this.textContent = "Checking...";
  this.classList.add("check-updates-loading");
  this.disabled = true;
  const versions = {};
  document.querySelectorAll(".env-detail .spec-row").forEach((row) => {
    const labelEl = row.querySelector(".spec-label");
    const valEl = row.querySelector(".spec-value");
    if (!labelEl || !valEl) return;
    const label = labelEl.textContent.trim();
    const key = _labelToKey[label];
    if (!key) return;
    const val = valEl.textContent.trim();
    if (val && val !== "—") versions[key] = val;
  });
  if (Object.keys(versions).length === 0) {
    this.textContent = "↻ Check Updates";
    this.classList.remove("check-updates-loading");
    this.disabled = false;
    return;
  }
  var results = await window.w2gp.checkPackageUpdates(versions);
  this.textContent = "↻ Check Updates";
  this.classList.remove("check-updates-loading");
  this.disabled = false;
  if (!results || !results.length) {
    showToast("No update info available");
    return;
  }
  let updateCount = 0;
  results.forEach((r) => {
    let row = document.querySelector(
      '.env-detail .spec-row[data-pkg="' + r.name + '"]',
    );
    if (!row) {
      const revMap = {};
      for (const k in _labelToKey) revMap[_labelToKey[k]] = k;
      const label = revMap[r.name];
      if (!label) return;
      const rows = document.querySelectorAll(".env-detail .spec-row");
      for (let i = 0; i < rows.length; i++) {
        if (
          rows[i].querySelector(".spec-label") &&
          rows[i].querySelector(".spec-label").textContent.trim() === label
        ) {
          row = rows[i];
          row.setAttribute("data-pkg", r.name);
          break;
        }
      }
    }
    if (!row) return;
    const valEl = row.querySelector(".spec-value");
    if (!valEl) return;
    const oldLatest = row.querySelector(".spec-latest");
    if (oldLatest) oldLatest.remove();
    const oldBtn = row.querySelector(".spec-update-btn");
    if (oldBtn) oldBtn.remove();
    if (!r.latest) return;
    const latestSpan = document.createElement("span");
    latestSpan.className = "spec-latest";
    latestSpan.textContent = "→ " + r.latest;
    valEl.after(latestSpan);
    if (r.installed && r.installed !== r.latest) {
      row.classList.add("has-update");
      row.classList.remove("up-to-date");
      updateCount++;
      const dot = row.querySelector(".spec-dot");
      if (dot) {
        dot.classList.remove("installed", "error", "installing");
        dot.classList.add("has-update");
      }
      const upBtn = document.createElement("button");
      upBtn.className = "spec-update-btn";
      upBtn.textContent = "↑";
      upBtn.title = "Upgrade " + r.name + " to " + r.latest;
      upBtn.addEventListener("click", async function (ev) {
        ev.stopPropagation();
        this.disabled = true;
        this.textContent = "...";
        if (dot) {
          dot.classList.remove("has-update", "installed", "error");
          dot.classList.add("installing");
        }
        var res = await window.w2gp.upgradePackage(r.dist || r.name);
        if (res && res.success) {
          this.textContent = "✓";
          this.classList.add("done");
          if (dot) {
            dot.classList.remove("installing", "has-update", "error");
            dot.classList.add("installed");
          }
          showToast("✓ " + r.name + " upgraded to " + r.latest);
        } else {
          this.textContent = "↑";
          this.disabled = false;
          if (dot) {
            dot.classList.remove("installing", "has-update", "installed");
            dot.classList.add("error");
          }
          showToast(
            "✗ Upgrade failed: " +
              (res && res.error ? res.error : "unknown error"),
          );
        }
      });
      latestSpan.after(upBtn);
    } else {
      row.classList.add("up-to-date");
      row.classList.remove("has-update");
      // Clear stale dot state (e.g. 'error' from a failed upgrade) when the
      // check now reports the package is installed & current.
      const dot = row.querySelector(".spec-dot");
      if (dot) {
        dot.classList.remove("installing", "has-update", "error");
        dot.classList.add("installed");
      }
    }
  });
  showToast(
    updateCount > 0
      ? updateCount + " updates available"
      : "All packages up to date",
  );
});

// ── GPU Kernel Wheels: Sync button ──
// Reinstalls every kernel wheel the active GPU's profile declares. Streams to
// the Console; refreshes the dashboard when done so versions update live.
$("syncKernelsBtn")?.addEventListener("click", async function () {
  if (this.disabled) return;
  this.disabled = true;
  this.textContent = "Updating…";
  try {
    const r = await window.w2gp.syncKernels();
    if (r && r.success) showToast("✓ GPU wheels updated");
    else showToast("✗ Update failed: " + (r && r.error ? r.error : "unknown"));
  } catch (e) {
    showToast("✗ Update failed: " + e.message);
  } finally {
    this.disabled = false;
    this.textContent = "↻ Update GPU Wheels";
    setTimeout(refreshDashboard, 1500);
  }
});
$("restoreKernelsBtn")?.addEventListener("click", async function () {
  if (this.disabled) return;
  if (
    !confirm(
      "Reinstall deepbeepmeep's original wheels? This downgrades launcher overrides (sage safe build → post4, GGUF floor off).",
    )
  )
    return;
  this.disabled = true;
  this.textContent = "Restoring…";
  try {
    const r = await window.w2gp.restoreKernels();
    if (r && r.success) showToast("✓ GPU wheels restored to upstream set");
    else showToast("✗ Restore failed: " + (r && r.error ? r.error : "unknown"));
  } catch (e) {
    showToast("✗ Restore failed: " + e.message);
  } finally {
    this.disabled = false;
    this.textContent = "Restore GPU Wheels";
    setTimeout(refreshDashboard, 1500);
  }
});

async function loadModelPaths() {
  const paths = await window.w2gp.getModelPaths();
  $("dashCkptPath").textContent = breakPath(paths?.checkpoints) || "(default)";
  $("dashCkptPath").title = paths?.checkpoints || "";
  $("dashLoraPath").textContent = breakPath(paths?.loras) || "(default)";
  $("dashLoraPath").title = paths?.loras || "";
  $("dashOutputPath").textContent = breakPath(paths?.output) || "(default)";
  $("dashOutputPath").title = paths?.output || "";
}

// When changing a model folder via the pencil, ask whether to physically MOVE
// the existing files (so nothing is re-downloaded) or just point Wan2GP at the
// new (empty) location. Then write wgp_config.json accordingly.
async function changeModelFolder(type, key, _cfgKey, singular) {
  const dir = await window.w2gp.selectFolder();
  if (!dir) return;
  // ponytail: reject file-as-folder (orca-paste Temp png)
  if (isFilePickedAsFolder(dir)) {
    alert("Please select a folder, not a file:\n" + dir);
    return;
  }
  const cur = await window.w2gp
    .getModelPaths()
    .then(
      (p) =>
        ({ ckpts: p?.checkpoints, loras: p?.loras, output: p?.output })[type],
    );
  if (cur && cur.toLowerCase() === dir.toLowerCase()) {
    window.w2gp.openFolder(dir);
    return;
  }
  const choice = await window.w2gp.confirmDialog({
    title: "Move " + singular + "?",
    message: "Change " + singular + " folder to:\n  " + dir,
    detail: cur
      ? "Do you want to MOVE the existing files from the old location into the new folder, or just point Wan2GP at the new (empty) folder?\n\nOld: " +
        cur
      : "Point Wan2GP at the new folder?",
    buttons: cur
      ? ["Move existing files", "Just point (no move)", "Cancel"]
      : ["OK", "Cancel"],
    defaultId: cur ? 0 : 0,
    cancelId: cur ? 2 : 1,
  });
  if (choice === "cancel") {
    window.w2gp.openFolder(dir);
    return;
  }
  if (choice === "move" && cur) {
    const r = await window.w2gp.moveFolder(cur, dir);
    if (!r || !r.ok) {
      alert("Could not move files:\n" + ((r && r.error) || "unknown"));
    }
  }
  // Write the real config (what Wan2GP reads) so the change takes effect next launch.
  const patch = {};
  patch[key] = type === "ckpts" ? [dir, "."] : dir;
  await window.w2gp.writeWgpConfig(patch);
  const cfg = await window.w2gp.configLoad();
  if (type === "ckpts") cfg.modelCkptsPath = dir;
  else if (type === "loras") cfg.modelLorasPath = dir;
  else cfg.modelOutputPath = dir;
  await window.w2gp.configSave(cfg);
  await loadModelPaths();
  showToast("✓ " + singular + " folder updated — restart Wan2GP to apply");
}

$("dashBrowseCkpt").addEventListener("click", () =>
  changeModelFolder("ckpts", "checkpointsPaths", "checkpoints", "Checkpoints"),
);
$("dashBrowseLora").addEventListener("click", () =>
  changeModelFolder("loras", "lorasRoot", "loras", "LoRAs"),
);
$("dashBrowseOutput").addEventListener("click", () =>
  changeModelFolder("output", "savePath", "output", "Output"),
);

$("desktopRepoLink").addEventListener("click", (e) => {
  e.preventDefault();
  window.w2gp.openExternal(
    "https://github.com/GKartist75/Wan2GP-Desktop-Tauri",
  );
});
$("discussionsLink").addEventListener("click", (e) => {
  e.preventDefault();
  window.w2gp.openExternal(
    "https://github.com/GKartist75/Wan2GP-Desktop-Tauri/discussions",
  );
});
$("ytLink").addEventListener("click", (e) => {
  e.preventDefault();
  window.w2gp.openExternal("https://www.youtube.com/@GK-Artist");
});

async function loadPaths(skipModelPaths) {
  const p = await window.w2gp.getInstallPaths();
  if (!p) return;
  const set = (id, val) => {
    const e = $(id);
    if (e) {
      e.textContent = breakPath(val) || "—";
      e.title = val || "";
    }
  };
  set("pathAppData", p.repo);
  set("installAppDataPath", p.appData);
  // Guard: if the chosen install location is a bare drive root (e.g. D:\),
  // the install is invalid — disable the Install button and warn the user.
  const rootBad = isDriveRoot(p.appData);
  const startBtn = $("installStartBtn");
  const rootWarn = $("installRootWarn");
  if (rootBad) {
    if (startBtn) {
      startBtn.disabled = true;
      startBtn.title = "Choose a folder, not a drive root.";
    }
    if (rootWarn) {
      rootWarn.textContent =
        "⚠ Install location is a drive root (" +
        p.appData +
        "). Pick a folder using Browse.";
      rootWarn.classList.remove("hidden");
    }
  } else {
    if (startBtn) {
      startBtn.disabled = false;
      startBtn.title = "";
    }
    if (rootWarn) rootWarn.classList.add("hidden");
  }
  // The top warning banner already owns the in-launcher "Migrate to new location"
  // button (shown when legacyRoamingFound), so keep this dashboard card button
  // hidden in that case to avoid two migration buttons. It only appears as a
  // manual re-trigger when there is no legacy roaming dir to migrate.
  const wrap = $("moveToPreferredWrap");
  if (wrap) {
    // ponytail: Tauri isolated — never show roaming migrate-warn
    if (window.__TAURI__) {
      wrap.classList.add("hidden");
    } else if (p.legacyRoamingFound) {
      wrap.classList.add("hidden");
    } else {
      wrap.classList.remove("hidden");
      const cp = $("currentDataDirPath");
      if (cp) cp.textContent = p.appData;
    }
  }
  window.w2gp.getDiskSpace().then((d) => {
    if (!d) return;
    var freeGb = (d.free / 1073741824).toFixed(1);
    $("pathFreeSpace").textContent = freeGb + " GB free";
  });
  if (!skipModelPaths) {
    // Show the model folders the user actually chose. Precedence: a previously
    // saved custom choice (desktop-config.json modelCkptsPath/…) wins; otherwise
    // the dedicated default (C:\\Wan2GP-Models). We used to ALWAYS overwrite with
    // the default here, which is why any custom path silently reverted to
    // C:\\Wan2GP-Models on every refresh (issue #74).
    const md = p.modelsDefault || p.appData;
    let saved = {};
    try {
      saved = (await window.w2gp.configLoad()) || {};
    } catch {}
    const savedCkpts = saved.modelCkptsPath;
    const savedLoras = saved.modelLorasPath;
    const savedOutput = saved.modelOutputPath;
    if (_modelCkpts || savedCkpts)
      setModelPath("ckpts", _modelCkpts || savedCkpts);
    else setModelPath("ckpts", pathJoin(md, "ckpts"));
    if (_modelLoras || savedLoras)
      setModelPath("loras", _modelLoras || savedLoras);
    else setModelPath("loras", pathJoin(md, "loras"));
    if (_modelOutput || savedOutput)
      setModelPath("output", _modelOutput || savedOutput);
    else setModelPath("output", pathJoin(md, "outputs"));
  }
  // Re-triage the target folder when the installer screen is showing
  // (Browse / reset changes the location — verdict must follow).
  try {
    if ($("installer") && $("installer").classList.contains("active")) {
      refreshTargetVerdict().catch(() => {});
      refreshModelDiskGates().catch(() => {});
    }
  } catch {}
}
// Tiny path join that tolerates both separators in the renderer (no node path).
function pathJoin(a, b) {
  return (a || "").replace(/[\\/]+$/, "") + "\\" + b;
}
// True when the path is a bare drive root, e.g. "D:" or "D:\" (but not "D:\Wan2GP").
function isDriveRoot(p) {
  if (!p) return false;
  const norm = (p || "").replace(/[\\/]+$/, "");
  return /^[A-Za-z]:$/.test(norm);
}

$("openAppDataBtn")?.addEventListener("click", () => {
  window.w2gp.getInstallPaths().then((p) => {
    if (p) window.w2gp.openFolder(p.repo);
  });
});
// Move the entire Wan2GP install (no reinstall) — reuse the migration modal
// pre-filled with the current location as the source.
$("changeAppDataBtn")?.addEventListener("click", () => openMigrationModal());

// Per-folder "open" buttons (folder icon) for the three model paths.
$("openCkptBtn")?.addEventListener("click", () =>
  window.w2gp
    .getModelPaths()
    .then((p) => p?.checkpoints && window.w2gp.openFolder(p.checkpoints)),
);
$("openLoraBtn")?.addEventListener("click", () =>
  window.w2gp
    .getModelPaths()
    .then((p) => p?.loras && window.w2gp.openFolder(p.loras)),
);
$("openOutputBtn")?.addEventListener("click", () =>
  window.w2gp
    .getModelPaths()
    .then((p) => p?.output && window.w2gp.openFolder(p.output)),
);

$("moveToPreferredBtn")?.addEventListener("click", () => openMigrationModal());

// ── Migration folder-chooser modal ──
// Opens a dialog pre-filled with our recommended targets (data dir + checkpoints
// + LoRAs + output). The user can override any of them, then "Move & restart"
// calls migrate-to-preferred with the chosen paths. After the move, main rewrites
// wgp_config.json model paths and relaunches.
let _migBusy = false;
async function openMigrationModal() {
  if (_migBusy) return;
  let prefs;
  try {
    prefs = await window.w2gp.migrateChoose();
  } catch {
    prefs = null;
  }
  if (!prefs) {
    alert("Could not determine migration targets.");
    return;
  }
  window._migPrefs = prefs;
  $("migDataDir").value = prefs.dataDir || "";
  $("migDataDir").title = prefs.dataDir || "";
  $("migCkpts").value = prefs.ckpts || "";
  $("migCkpts").title = prefs.ckpts || "";
  $("migLoras").value = prefs.loras || "";
  $("migLoras").title = prefs.loras || "";
  $("migOutput").value = prefs.output || "";
  $("migOutput").title = prefs.output || "";
  // Context-aware copy: the modal is reused both for the first migration out of
  // a roaming AppData profile AND for later re-location of an already-migrated
  // install (e.g. C:\Wan2GP → D:\Wan2GP). Don't claim "AppData" when it isn't.
  const roaming = !!prefs.fromRoaming;
  const cur = prefs.legacy || "";
  const title = $("migrationTitle");
  const sub = $("migrationSub");
  if (title)
    title.textContent = roaming
      ? "Move Wan2GP out of AppData"
      : "Move Wan2GP to a new location";
  if (sub) {
    sub.textContent = roaming
      ? "Your Wan2GP data currently lives in your roaming AppData profile. Move it to a dedicated, fast drive — AppData is meant for small settings, not multi-GB model checkpoints (it can slow logins, trigger antivirus locks, and bloat your profile). Our recommended locations are pre-filled — change any of them if you like."
      : "Your Wan2GP is currently at " +
        cur +
        ". Move it to a different drive or folder — your repo, venv, settings, and model folders travel with it. The recommended location is pre-filled — change it if you like.";
  }
  // Reset to idle state (in case a previous attempt left the progress UI showing).
  _migBusy = false;
  const btn = $("migrationMoveBtn");
  if (btn) {
    btn.disabled = false;
    btn.textContent = "Move & restart";
  }
  const prog = $("migrationProgress");
  if (prog) {
    prog.classList.add("hidden");
    const f = $("migrationProgressFill");
    if (f) f.style.width = "0%";
  }
  $("migrationModal").classList.remove("hidden");
}
$("migrationCloseBtn")?.addEventListener("click", () =>
  $("migrationModal").classList.add("hidden"),
);
$("migrationCancelBtn")?.addEventListener("click", () =>
  $("migrationModal").classList.add("hidden"),
);
// Browse buttons inside the modal pick a folder for the matching field.
document.querySelectorAll("#migrationModal [data-browse]").forEach((btn) => {
  btn.addEventListener("click", async () => {
    const key = btn.getAttribute("data-browse");
    const field = {
      dataDir: "migDataDir",
      ckpts: "migCkpts",
      loras: "migLoras",
      output: "migOutput",
    }[key];
    try {
      const picked = await window.w2gp.selectFolder();
      if (picked) {
        $(field).value = picked;
        $(field).title = picked;
      }
    } catch {}
  });
});
async function runMigration(doMove) {
  if (_migBusy) return;
  // The two modal buttons own the choice now (Move vs Just-switch) — no
  // second popup. Same folder ⇒ nothing to move regardless of button.
  const newDataDir = $("migDataDir").value.trim();
  // ponytail: reject file-as-folder
  if (isFilePickedAsFolder(newDataDir)) {
    alert("Please select a folder, not a file:\n" + newDataDir);
    resetMigrationUI();
    return;
  }
  const oldDataDir =
    (window._migPrefs &&
      (window._migPrefs.dataDir || window._migPrefs.legacy)) ||
    (await window.w2gp.getInstallPaths().catch(() => null))?.dataDir ||
    "";
  if (
    newDataDir &&
    oldDataDir &&
    newDataDir.toLowerCase() === oldDataDir.toLowerCase()
  )
    doMove = false;
  _migBusy = true;
  const moveBtn = $("migrationMoveBtn"),
    pointBtn = $("migrationPointBtn");
  if (moveBtn) moveBtn.disabled = true;
  if (pointBtn) pointBtn.disabled = true;
  const btn = (doMove ? moveBtn : pointBtn) || moveBtn;
  btn.textContent = doMove ? "Moving…" : "Switching…";
  const prog = $("migrationProgress");
  if (prog) {
    prog.classList.remove("hidden");
    setMigrationProgress(0);
  }
  const choices = {
    dataDir: newDataDir,
    ckpts: $("migCkpts").value,
    loras: $("migLoras").value,
    output: $("migOutput").value,
  };
  if (!choices.dataDir) {
    alert("Choose a Wan2GP data folder.");
    resetMigrationUI();
    return;
  }
  // Bare drive root ⇒ resolve to <root>\Wan2GP like the installer Browse does.
  if (isDriveRoot(choices.dataDir)) {
    choices.dataDir = pathJoin(choices.dataDir, "Wan2GP");
    $("migDataDir").value = choices.dataDir;
    appendLog(
      "[*] Drive root selected — using " + choices.dataDir + " instead.",
    );
  }
  try {
    // ponytail: 4 modes for Wan2GP folder — existing/new × Move vs Just point
    if (
      doMove &&
      oldDataDir &&
      newDataDir.toLowerCase() !== oldDataDir.toLowerCase()
    ) {
      const r = await window.w2gp.moveFolder(oldDataDir, newDataDir);
      if (!r || (!r.ok && !r.success)) {
        alert(
          "Could not move files:\n" +
            ((r && r.error) || "unknown") +
            "\n\nClose any Wan2GP windows/terminals and try again.",
        );
        resetMigrationUI();
        return;
      }
    }
    const r2 = await window.w2gp.setDataDir(newDataDir);
    if (!r2 || (!r2.ok && !r2.success)) {
      alert(
        "Could not switch data folder:\n" + ((r2 && r2.error) || "unknown"),
      );
      resetMigrationUI();
      return;
    }
    // persist model folder overrides if changed (no move, just point — like changeModelFolder "Just point")
    const ck = $("migCkpts").value.trim(),
      lo = $("migLoras").value.trim(),
      out = $("migOutput").value.trim();
    const patch = {};
    if (ck && ck !== (window._migPrefs?.ckpts || ""))
      patch.checkpointsPaths = [ck, "."];
    if (lo && lo !== (window._migPrefs?.loras || "")) patch.lorasRoot = lo;
    if (out && out !== (window._migPrefs?.output || "")) patch.savePath = out;
    if (Object.keys(patch).length) await window.w2gp.writeWgpConfig(patch);
    btn.textContent = "Restarting…";
    setTimeout(() => location.reload(), 900);
  } catch (e) {
    alert("Migration failed: " + errText(e));
    resetMigrationUI();
  }
}
$("migrationMoveBtn")?.addEventListener("click", () => runMigration(true));
$("migrationPointBtn")?.addEventListener("click", () => runMigration(false));
// Show live copy progress (only the slow cross-volume/copy-fallback path emits
// this — the common instant rename path finishes before any paint).
function setMigrationProgress(pct) {
  const fill = $("migrationProgressFill");
  if (fill) fill.style.width = pct + "%";
  const txt = $("migrationProgressText");
  if (txt) txt.textContent = "Moving… " + pct + "%";
}
window.w2gp.onMigrationProgress?.(setMigrationProgress);
// Restore the modal to its idle state (re-enable button, hide progress).
function resetMigrationUI() {
  _migBusy = false;
  const btn = $("migrationMoveBtn");
  if (btn) {
    btn.disabled = false;
    btn.textContent = "Move & restart";
  }
  const pt = $("migrationPointBtn");
  if (pt) {
    pt.disabled = false;
    pt.textContent = "Just switch to it";
  }
  const prog = $("migrationProgress");
  if (prog) {
    prog.classList.add("hidden");
    setMigrationProgress(0);
  }
}
// Startup prompt (main process) asks the renderer to open this modal.
window.w2gp.onOpenMigration?.(() => openMigrationModal());

// Re-entrancy guard: periodic + manual checks share one flight; a slow GitHub
// response can't stack overlapping fetches.
let _wangpCheckBusy = false;
// ponytail: upstream fetch spawns curl/powershell to GitHub (~1-5s) — cache 5 min
// instead of re-hitting the network on every refreshDashboard.
let _upstreamAt = 0,
  _upstreamData = null;
// ponytail: llmEnginesList spawns 3x where + a cold venv python per call, and
// refreshLLMEngines + refreshDeepy each called it per refresh — one shared flight.
let _llmEnginesPromise = null;
function getLLMEngines() {
  if (!_llmEnginesPromise)
    _llmEnginesPromise = window.w2gp
      .llmEnginesList()
      .catch(() => ({ engines: [] }));
  return _llmEnginesPromise;
}
async function loadWangpChangelog(showLoading) {
  const localEl = $("localCommit");
  const listEl = $("updatesList");
  const verEl = $("wangpVersion");
  if (!listEl) return;
  if (_wangpCheckBusy) return;
  _wangpCheckBusy = true;
  try {
    if (showLoading)
      listEl.innerHTML =
        '<div class="changelog-loading">Checking for updates...</div>';

    const local = await window.w2gp.getWangpLocalVersion();
    if (local && localEl)
      localEl.textContent = local.hash ? local.hash.substring(0, 7) : "";

    window.w2gp.getWangpVersion().then((v) => {
      if (v && verEl) verEl.textContent = v;
    });

    const upstream =
      Date.now() - _upstreamAt < 5 * 60 * 1000 && _upstreamData
        ? _upstreamData
        : await window.w2gp.getWangpUpstreamInfo().then((u) => {
            if (u && u.commits) {
              _upstreamAt = Date.now();
              _upstreamData = u;
            }
            return u;
          });
    if (!upstream || !upstream.commits) {
      // A transient upstream failure on the silent periodic poll must not
      // clobber a previously rendered changelog — show the error only on an
      // explicit user check.
      if (showLoading)
        listEl.innerHTML =
          '<div class="changelog-error">Could not fetch updates</div>';
      // Clear any stale green dot from a previous check — don't leave it dangling
      const updateBtn = $("updateBtn");
      if (updateBtn) {
        updateBtn.classList.remove("has-update");
        updateBtn.querySelector(".update-dot")?.remove();
      }
      return;
    }

    const updateBtn = $("updateBtn");
    const hasUpdate = local && upstream.commits[0]?.hash !== local.hash;
    if (hasUpdate) {
      updateBtn?.classList.add("has-update");
      if (!updateBtn?.querySelector(".update-dot")) {
        const dot = document.createElement("span");
        dot.className = "update-dot";
        updateBtn.appendChild(dot);
      }
    } else {
      updateBtn?.classList.remove("has-update");
      updateBtn?.querySelector(".update-dot")?.remove();
    }

    listEl.textContent = "";
    for (const c of upstream.commits) {
      const item = document.createElement("div");
      item.className = "cl-item";
      const dt = document.createElement("span");
      dt.className = "cl-date";
      dt.textContent = fmtDate(c.date);
      const msg = document.createElement("span");
      msg.className = "cl-msg";
      msg.textContent = c.message;
      const au = document.createElement("span");
      au.className = "cl-author";
      au.textContent = c.author;
      item.append(dt, msg, au);
      listEl.append(item);
    }
  } finally {
    _wangpCheckBusy = false;
  }
}

function fmtDate(s) {
  if (!s) return "";
  const d = new Date(s);
  const days = (Date.now() - d) / 864e5;
  if (days < 1) return "today";
  if (days < 2) return "yesterday";
  return days < 7
    ? `${Math.floor(days)}d ago`
    : d.toLocaleDateString("en-US", { month: "short", day: "numeric" });
}

document.addEventListener("DOMContentLoaded", () => {
  $("wangpCheckLink")?.addEventListener("click", (e) => {
    e.preventDefault();
    loadWangpChangelog(true);
  });
  $("changelogLink")?.addEventListener("click", (e) => {
    e.preventDefault();
    window.w2gp.openExternal(
      "https://github.com/deepbeepmeep/Wan2GP/blob/main/docs/CHANGELOG.md",
    );
  });
  $("hfModelsLink")?.addEventListener("click", (e) => {
    e.preventDefault();
    window.w2gp.openExternal("https://huggingface.co/DeepBeepMeep");
  });
});

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
// ── First-launch info bar (#launchInfo): shown ONLY while starting/launching ──
// Explicit show/hide calls at launch-click/error/ready sites give instant
// feedback; this repaint only corrects drift (a runtime path that strands it
// hidden mid-start). It never shows outside the starting window — not on
// fresh page load, not after Stop — and never when Wan2GP is not installed.
// Runs on the refresh path, so every dashboard refresh repaints it from STATE.
function paintLaunchInfo() {
  try {
    const bar = $("launchInfo");
    if (!bar) return;
    if (!_launchInstalled) {
      bar.classList.add("hidden");
      return;
    }
    const starting = [
      "browserBtn",
      "browserNoGpuBtn",
      "termBtn",
      "termNoGpuBtn",
      "appBtn",
    ].some((id) => {
      const b = $(id);
      return !!b && b.disabled && /Starting/.test(b.textContent || "");
    });
    if (starting) {
      bar.classList.remove("hidden");
      return;
    }
    // Outside the starting window the bar stays hidden (fresh load,
    // ready, stopped) — it informs the launch wait, nothing else.
    bar.classList.add("hidden");
  } catch {}
}

// ── Launch in App (BrowserView — renders Gradio reliably on Electron 40; intercepts
//     /manifest.json to dodge gradio#11553 blank-page bug) ──
// Opens the Desktop embed for an already-running server (called immediately
// when the server was already up, or when the backend reports ready).
let _pendingOpen = null;
// Safety: if ready never arrives (crashed silencer), un-wedge after 3 min.
function armPendingTimeout() {
  setTimeout(() => {
    if (_pendingOpen) {
      _pendingOpen = null;
      appendLog(
        "[!] Wan2GP did not report ready — check the console above for errors.",
      );
      hideLaunchInfo();
      $("appBtn").disabled = false;
      setAppLaunchLabel();
      ["browserBtn", "browserNoGpuBtn", "termNoGpuBtn"].forEach((id) => {
        const b = $(id);
        if (b) b.disabled = false;
      });
      if (!browserRunning) $("browserBtn").textContent = "Browser";
    }
  }, 180000);
}
// Bounded wait for the view-transition mutex. openDesktopView, closeWebview,
// checkCrashRecovery, the Stop-all teardown and the server-exit path all
// mutate the same four UI elements (dashBody, webviewContainer, wvControls,
// serverMode) — never interleave them or the UI strands mixed states
// (dashboard visible WITH Desktop topbar controls, #14 follow-up).
async function awaitViewFree(timeoutMs) {
  const t0 = Date.now();
  while (window.__viewBusy && Date.now() - t0 < (timeoutMs || 20000)) {
    await new Promise((r) => setTimeout(r, 100));
  }
  return !window.__viewBusy;
}
async function openDesktopView(url, fresh) {
  // Transition mutex with closeWebview: double-clicks / fast Back-and-forth
  // used to interleave the two async flows and strand mixed states
  // (dashboard visible WITH Desktop topbar controls).
  if (window.__viewBusy) {
    appendLog("[i] View transition in progress — ignored");
    return;
  }
  window.__viewBusy = true;
  $("appBtn").disabled = true;
  $("appBtn").textContent = "Opening...";
  try {
    // Stale-renderer guard: the mode was switched while the view sat
    // hidden-but-alive → destroy it and fall through to a fresh create below.
    // (Just re-showing would keep the OLD renderer forever.)
    try {
      const _cfg = await window.w2gp.configLoad().catch(() => ({}));
      const _want = _cfg && _cfg.embedMode === "iframe" ? "iframe" : "native";
      const _have =
        window.w2gp.isNativeEmbed && window.w2gp.isNativeEmbed()
          ? "native"
          : document.getElementById("tauri-browser-view")
            ? "iframe"
            : null;
      if (!fresh && _have && _have !== _want) {
        appendLog(
          `[*] Embed mode changed (${_have} → ${_want}) — recreating Desktop view…`,
        );
        try {
          await window.w2gp.destroyBrowserView();
        } catch {}
        fresh = true;
      }
    } catch {}
    // Hidden-but-alive view (Back to Dashboard keeps it) → just re-show it.
    // No relaunch, no port traffic, Gradio session untouched. Native child:
    // re-show the (still-alive) child instead of recreating it.
    const _nativeAlive =
      !fresh && window.w2gp.isNativeEmbed && window.w2gp.isNativeEmbed();
    if (
      (!fresh && document.getElementById("tauri-browser-view")) ||
      _nativeAlive
    ) {
      const _iv = document.getElementById("tauri-browser-view");
      if (_iv) _iv.style.display = "flex";
      if (_nativeAlive) reshowNativeView();
      $("dashBody").style.display = "none";
      $("webviewContainer").classList.remove("hidden");
      hideLaunchInfo();
      showWebviewUI();
      updateLed("running");
      updateFtStatus("running");
      serverMode = "app";
      appRunning = true;
      setAppLaunchLabel();
      window.w2gp.uiModeSet("app");
      if (browserRunning) resetBrowserLaunchUI();
      return;
    }
    const created = await window.w2gp.createBrowserView(url, {
      reload: !!fresh,
    });
    if (!created || (created.error && !created.fallback))
      throw new Error(
        created && created.error ? created.error : "failed to create embed",
      );
    noteEmbedFallback(created);
    appendLog(
      `[*] Desktop view: ${(created && created.mode) || "iframe"} renderer — ${url}`,
    );
    $("dashBody").style.display = "none";
    $("webviewContainer").classList.remove("hidden");
    hideLaunchInfo();
    showWebviewUI();
    updateLed("running");
    updateFtStatus("running");
    serverMode = "app";
    appRunning = true;
    setAppLaunchLabel();
    window.w2gp.uiModeSet("app"); // crash recovery: remember we are in Desktop mode
    if (browserRunning) resetBrowserLaunchUI();
    // Open the floating terminal per the saved default dock (or stay minimised)
    // ponytail: Tauri desktop embed is iframe, not native BrowserView — don't auto-cover it with the console
    const cfg = await window.w2gp.configLoad();
    const dock = window.__TAURI__
      ? "minimised"
      : cfg.termDockDefault || "bottom";
    if (dock === "minimised") {
      if (!$("floatingTerminal").classList.contains("hidden"))
        closeFloatingTerm();
    } else {
      if ($("floatingTerminal").classList.contains("hidden"))
        toggleFloatingTerm();
      setFtDock(dock);
    }
  } catch (e) {
    // Never leave the dashboard hidden behind a blank embed
    $("dashBody").style.display = "";
    $("webviewContainer").classList.add("hidden");
    hideWebviewUI();
    $("runningLed").style.display = "none";
    appendLog(`[LAUNCH ERROR] ${errText(e)}`);
  } finally {
    $("appBtn").disabled = false;
    setAppLaunchLabel();
    window.__viewBusy = false;
  }
}
$("appBtn").addEventListener("click", async () => {
  // Server already up behind the dashboard → just open the view.
  if (appRunning && currentUrl) {
    openDesktopView(currentUrl, false);
    return;
  }
  $("appBtn").disabled = true;
  $("appBtn").textContent = "Starting...";
  showLaunchInfo();
  appendLog("[*] Starting Wan2GP — watch the console below…\n");
  try {
    const result = await window.w2gp.launchWebview();
    currentUrl = result.url;
    if (!result.fresh) {
      openDesktopView(result.url, false);
      return;
    }
    // Fresh boot: stay on the dashboard console until the backend reports ready.
    _pendingOpen = { kind: "desktop", url: result.url };
    $("appBtn").textContent = "Starting… (see console)";
    armPendingTimeout();
  } catch (e) {
    hideLaunchInfo();
    appendLog(`[LAUNCH ERROR] ${errText(e)}`);
    $("appBtn").disabled = false;
    setAppLaunchLabel();
  }
});

// Native was requested but the backend couldn't stand up the child webview
// (it fell back to iframe) — say so loudly instead of silently running old.
function noteEmbedFallback(created) {
  if (created && created.fallback) {
    const why = created.error ? " — " + created.error : "";
    appendLog(
      "[!] Native embed failed, fell back to iframe" +
        why +
        " (see console / F12)",
    );
    showToast("✗ Native embed failed — running iframe" + why);
  }
}
// Destroy + recreate the Desktop view (picks up a new embedMode).
// The Gradio session restarts — that's the point: a live renderer can't
// change modes, and Back-to-Dashboard only hides it (keeps it alive).
async function relaunchDesktopView() {
  if (!currentUrl) {
    showToast("Nothing to relaunch — launch Wan2GP in Desktop first");
    return;
  }
  appendLog("[*] Relaunching Desktop view…");
  try {
    await window.w2gp.destroyBrowserView();
  } catch {}
  appRunning = false;
  await openDesktopView(currentUrl, true);
}
// Reflect whether the Wan2GP desktop (BrowserView) server is still up behind the
// dashboard: while it is, the launch button reads "Back to Wan2GP in Desktop".
function setAppLaunchLabel() {
  $("appBtn").textContent = appRunning
    ? "Back to Wan2GP in Desktop"
    : "Launch Wan2GP in Desktop";
  syncEmbedSwitchLocks();
}
// Renderer switches are usable only while NO Desktop session is active: while
// one runs, every switch shows the active viewer locked (stop the server +
// back to dashboard to change it). One hook — setAppLaunchLabel runs on
// every open/close/stop/exit transition.
function syncEmbedSwitchLocks() {
  const locked = !!appRunning;
  const tip = locked
    ? "Active renderer (locked — stop the Wan2GP server to switch)"
    : "Desktop renderer — applies on launch";
  for (const id of ["embedModeSelect", "embedModeTop"]) {
    const el = $(id);
    if (!el) continue;
    el.disabled = locked;
    el.title = tip;
  }
}

function showWebviewUI() {
  $("wvControls").style.display = "flex";
  // Topbar renderer switch (permanent, always visible): shows the active
  // renderer (gray iframe / green native). Locked while a session runs.
  try {
    const native = window.w2gp.isNativeEmbed && window.w2gp.isNativeEmbed();
    const q = $("embedModeTop");
    if (q) {
      q.value = native ? "native" : "iframe";
      q.classList.toggle("native", !!native);
    }
  } catch {}
}

function hideWebviewUI() {
  $("wvControls").style.display = "none";
  }

async function closeWebview(silent) {
  if (window.__viewBusy) {
    appendLog("[i] View transition in progress — ignored");
    return;
  }
  window.__viewBusy = true;
  try {
    // Like Electron's BrowserView hide: the iframe STAYS ALIVE but hidden, so
    // flipping back is instant (no relaunch, no port conflict, Gradio state kept).
    // Only a real server stop destroys it (see onWangpExit).
    if (!$("floatingTerminal").classList.contains("hidden"))
      closeFloatingTerm();
    await window.w2gp.hideBrowserView();
    $("webviewContainer").classList.add("hidden");
    $("dashBody").style.display = "";
    hideWebviewUI();
    // LED follows the server, not the view: Back-to-Dashboard keeps it visible.
    if (appRunning) updateLed("running");
    else $("runningLed").style.display = "none";
    serverMode = null; // webview UI is gone; a later server exit must not re-close it
    window.w2gp.uiModeSet(null);
    // Server is still running behind the dashboard → the launch button becomes "Back to…"
    setAppLaunchLabel();
    // Silent when invoked from the server-exit path. On a manual Back press,
    // only claim a live server when we actually believe one is behind us —
    // after a stop/exit nothing runs and the old message lied (shot-2 class).
    if (!silent)
      appendLog(
        appRunning
          ? "[*] Back on dashboard. Server still running — flip back anytime."
          : "[*] Back on dashboard.",
      );
  } finally {
    window.__viewBusy = false;
  }
}

$("backToDashboardBtn").addEventListener("click", () => closeWebview());

// ── Crash recovery: put the UI back where it was after a renderer crash ──
// The main process auto-reloads the launcher renderer when it dies (usually a
// GPU/display-driver hiccup during generation). On this fresh load we ask what
// happened: if a crash just occurred and the Wan2GP server is still running,
// re-open the embedded view (Desktop mode) or re-arm the browser-mode UI
// instead of stranding the user on the bare dashboard.
async function checkCrashRecovery() {
  let info = null;
  try {
    info = await window.w2gp.getCrashRecoveryInfo();
  } catch {
    return;
  }
  if (!info || !info.pending) return;
  appendLog(
    info.serverRunning
      ? `[i] Launcher UI recovered after a crash (${info.gpuProcessDied ? "GPU/display-driver hiccup" : "renderer crash"}). The Wan2GP server is still running.`
      : "[i] Launcher UI recovered after a crash. The Wan2GP server is not running.",
  );
  if (info.serverRunning && info.mode === "app" && info.url) {
    // Same UI state as open/close/Stop — hold the transition mutex so a
    // Stop pressed mid-recovery can't interleave into a mixed state.
    window.__viewBusy = true;
    try {
      // Force a reload: after a renderer crash the embedded Gradio page may be in a
      // bad state, so re-open from a fresh load rather than re-attaching a live session.
      const created = await window.w2gp.createBrowserView(info.url, {
        reload: true,
      });
      if (!created || (created.error && !created.fallback))
        throw new Error(
          created && created.error
            ? created.error
            : "failed to re-create embed",
        );
      noteEmbedFallback(created);
      // createBrowserView takes seconds — a Stop may have landed meanwhile.
      // Re-probe: never open a view onto a just-stopped server.
      try {
        const fresh = await window.w2gp
          .getCrashRecoveryInfo()
          .catch(() => null);
        if (!fresh || !fresh.pending || !fresh.serverRunning) {
          appendLog(
            "[i] Server went away during recovery — staying on dashboard.",
          );
          try {
            await window.w2gp.destroyBrowserView();
          } catch {}
          window.__viewBusy = false;
          return;
        }
      } catch {}
      appendLog(
        `[*] Desktop view recovered: ${(created && created.mode) || "iframe"} renderer — ${info.url}`,
      );
      $("dashBody").style.display = "none";
      $("webviewContainer").classList.remove("hidden");
      showWebviewUI();
      updateLed("running");
      updateFtStatus("running");
      serverMode = "app";
      appRunning = true;
      setAppLaunchLabel();
      window.w2gp.uiModeSet("app");
      // Restore the floating console per the saved default dock, exactly like
      // the normal Desktop launch does.
      const cfg = await window.w2gp.configLoad();
      const dock = cfg.termDockDefault || "bottom";
      if (dock === "minimised") {
        if (!$("floatingTerminal").classList.contains("hidden"))
          closeFloatingTerm();
      } else {
        if ($("floatingTerminal").classList.contains("hidden"))
          toggleFloatingTerm();
        setFtDock(dock);
      }
      showToast("Launcher UI recovered — Wan2GP re-opened");
      window.__viewBusy = false;
    } catch (e) {
      appendLog(
        "[!] Could not re-open the embedded view after the crash: " + e.message,
      );
      $("dashBody").style.display = "";
      $("webviewContainer").classList.add("hidden");
      hideWebviewUI();
      $("runningLed").style.display = "none";
      serverMode = null;
      try {
        await window.w2gp.destroyBrowserView();
      } catch {}
      window.__viewBusy = false;
    }
  } else if (info.serverRunning && info.mode === "browser") {
    browserRunning = true;
    serverMode = "browser";
    showBrowserRunningUI();
    $("browserBtn").textContent = "Open Browser";
    appendLog("[i] Browser-mode launch restored — server running.");
  } else {
    // Dashboard (or no server): detach any leftover BrowserView so it can't
    // composite above the dashboard after the crash.
    try {
      await window.w2gp.destroyBrowserView();
    } catch {}
  }
}

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
// ── Event Wiring: Dashboard ──
$("updateBtn").addEventListener("click", async () => {
  $("updateBtn").disabled = true;
  $("updateBtn").textContent = "Working...";
  try {
    const r = await window.w2gp.update();
    appendLog("[*] Wan2GP update complete");
    if (r && r.requirements === "reinstalled") {
      appendLog("[*] requirements.txt changed — pinned packages reinstalled");
      const pd = r && r.pinDiff;
      if (Array.isArray(pd) && pd.length) appendLog("    " + pd.join("\n    "));
    } else if (r && r.requirements === "failed")
      appendLog(
        "[!] requirements reinstall failed — see the console output above; the git pull itself stays applied.",
      );
    if (
      r &&
      r.depCheck === "drift" &&
      Array.isArray(r.drift) &&
      r.drift.length
    ) {
      appendLog(
        "[!] dependency drift: " + r.drift.join(", ") + " — use restore",
      );
      showToast(
        "[!] Dependency drift — packages missing or outdated. See the red banner.",
      );
      showDriftBanner(r.drift);
    } else {
      // Clean update (or no drift info) clears any stale banner from an
      // earlier drift run — otherwise Restore/Dismiss linger confusingly.
      hideDriftBanner();
    }
    refreshDashboard();
  } catch (e) {
    appendLog("[!] Update failed: " + errText(e));
    alert("Update: " + errText(e));
  }
  $("updateBtn").disabled = false;
  $("updateBtn").textContent = "↻ Update Wan2GP (DeepBeepMeep)";
});
$("repairFilesBtn")?.addEventListener("click", async () => {
  const btn = $("repairFilesBtn");
  if (btn) btn.disabled = true;
  try {
    const v = await window.w2gp.verifyWangpFiles().catch((e) => ({
      error: errText(e),
    }));
    if (v && v.error) {
      showToast("✗ Verify failed: " + v.error);
    } else if (!v || v.clean) {
      const untracked =
        v && v.untracked
          ? " (" + v.untracked + " untracked user file(s) left alone)"
          : "";
      showToast("✓ Wan2GP files match upstream.");
      appendLog("[*] Verify: tracked files clean" + untracked + ".");
    } else {
      const dirty = v.dirty || [];
      const total = v.dirtyTotal || dirty.length;
      const names = dirty
        .slice(0, 10)
        .map((d) => (d.path || "?") + " [" + (d.kind || "?") + "]")
        .join("\n");
      const more =
        total > dirty.length ? "\n…+" + (total - dirty.length) + " more" : "";
      const ok = confirm(
        total +
          " tracked Wan2GP file(s) differ from upstream:\n" +
          names +
          more +
          "\n\nRepair restores them (your edits are stashed recoverably; settings/models untouched). Continue?",
      );
      if (ok) {
        appendLog("[*] Repairing Wan2GP files…");
        const r = await window.w2gp.repairWangpFiles();
        if (r && (r.ok || r.repaired)) {
          showToast("✓ Wan2GP files repaired — restart Wan2GP to run them.");
          appendLog(
            "[*] Repair done" +
              (r.stashed ? " (prior edits stashed, recoverable via git)" : "") +
              ".",
          );
        } else showToast("✗ Repair failed: " + ((r && r.error) || "unknown"));
        refreshDashboard();
      }
    }
  } catch (e) {
    showToast("✗ " + errText(e));
  }
  if (btn) btn.disabled = false;
});
$("rollbackBtn")?.addEventListener("click", async () => {
  const btn = $("rollbackBtn");
  if (btn) btn.disabled = true;
  try {
    const v = await window.w2gp.verifyWangpFiles().catch((e) => ({
      error: errText(e),
    }));
    if (v && v.error) {
      showToast("✗ Rollback check failed: " + v.error);
    } else {
      const pin = (v && v.pin) || null;
      const head = (v && v.head) || null;
      if (!pin || !pin.hash) {
        showToast("No recorded Wan2GP update yet — update once first.");
      } else if (head && pin.hash.slice(0, head.length) === head) {
        showToast("✓ Already at the recorded update (" + head + ").");
        appendLog("[*] Rollback: already at " + head + ", nothing to do.");
      } else if (v && !v.clean) {
        showToast(
          "Tracked files differ — Verify/Repair (or stash) first, then roll back.",
        );
      } else {
        const when = pin.date ? " (" + pin.date + ")" : "";
        const ok = confirm(
          "Roll back Wan2GP to the recorded update?\n" +
            pin.hash.slice(0, 8) +
            when +
            " → HEAD is " +
            (head || "unknown") +
            "\n\nUntracked files (settings/models) untouched. Continue?",
        );
        if (ok) {
          appendLog("[*] Rolling back Wan2GP…");
          const r = await window.w2gp.rollbackWangp();
          if (r && (r.ok || r.rolledBack)) {
            showToast(
              "✓ Rolled back to " +
                ((r && r.commit) || "recorded update") +
                " — restart Wan2GP.",
            );
            appendLog(
              "[*] Rolled back to " +
                ((r && r.commit) || "recorded update") +
                ".",
            );
          } else
            showToast("✗ Rollback failed: " + ((r && r.error) || "unknown"));
          refreshDashboard();
        }
      }
    }
  } catch (e) {
    showToast("✗ " + errText(e));
  }
  if (btn) btn.disabled = false;
});
document
  .querySelectorAll(".theme-toggle")
  .forEach((btn) => btn.addEventListener("click", toggleTheme));

function switchSettingsTab(tabName) {
  document.querySelectorAll(".settings-tab").forEach((t) => {
    t.classList.remove("active");
  });
  document.querySelectorAll(".settings-tab-content").forEach((c) => {
    c.classList.remove("active");
  });
  var tab = document.querySelector('.settings-tab[data-tab="' + tabName + '"]');
  if (tab) tab.classList.add("active");
  var tabContent = document.querySelector(
    '.settings-tab-content[data-tab="' + tabName + '"]',
  );
  if (tabContent) tabContent.classList.add("active");

  // Plugins tab: fill on demand (openSettings no longer preloads the walk).
  if (tabName === "plugins") {
    setTimeout(() => {
      try {
        refreshPluginsLazy();
      } catch (e) {}
    }, 30);
  }

  // Auto-Tune: check if Wan2GP is installed — disable if not
  if (tabName === "autotune") {
    checkAutoTuneInstalled();
    // Saved tags + dropdown seeding — runs on EVERY entry (tab click or
    // dashboard shortcut), not just physical clicks.
    setTimeout(() => {
      try {
        memProfileLoad();
      } catch {}
    }, 120);
  }
}

async function checkAutoTuneInstalled() {
  const installed = await window.w2gp.checkInstalled();
  const notInstalledEl = $("autotuneNotInstalled");
  const contentEl = $("autotuneContent");
  if (!notInstalledEl || !contentEl) return;
  if (installed.repo) {
    notInstalledEl.classList.add("hidden");
    contentEl.classList.remove("hidden");
    // D3: first visit to the tab — auto-run detection so the panel shows a live
    // recommendation instead of an empty "Run detection first" state. Only once
    // per session; a failed detect leaves the button enabled for a manual retry.
    if (!_autotuneHardware && !_autotuneAutoDetectDone) {
      _autotuneAutoDetectDone = true;
      setTimeout(() => $("autotuneDetectBtn")?.click(), 150);
    }
  } else {
    notInstalledEl.classList.remove("hidden");
    contentEl.classList.add("hidden");
  }
}

document.querySelectorAll(".settings-tab").forEach((tab) => {
  tab.addEventListener("click", () => {
    switchSettingsTab(tab.dataset.tab);
  });
});
$("settingsBtn").addEventListener("click", () => {
  openSettings();
});
$("autoTuneDashBtn").addEventListener("click", () => {
  openSettings();
  switchSettingsTab("autotune");
});
// Windows-only UI: hide the Task Manager button on other platforms.
if (window.w2gp && window.w2gp.platform !== "win32") {
  const taskMgrBtn = $("taskMgrBtn");
  if (taskMgrBtn) taskMgrBtn.style.display = "none";
}
$("taskMgrBtn").addEventListener("click", () => {
  window.w2gp.openTaskManager();
});

// ── Quick pip install ──
// Accept either a bare spec (claude-agent-sdk==0.1.40) or a full command
// (pip install claude-agent-sdk==0.1.40) pasted by the user — strip any leading
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

// ── Guided LLM engine setup (Deepy Prime) ──
// Renders ONE generic card per catalog engine (services/llm-engines.js). The
// card shows live ✓/✗ status for the CLI and/or pip bridge, plus a one-click
// installer (pip for Claude Code, npm for Codex/OpenCode) and, for engines with
// a server (OpenCode), a Start/Stop server toggle. New engines = one data line
// in services/llm-engines.js — no UI branch.
async function refreshLLMEngines() {
  const list = $("llmEnginesList");
  if (!list) return;
  let data;
  try {
    data = await getLLMEngines();
  } catch {
    data = { engines: [] };
  }
  const engines = (data && data.engines) || [];
  if (!engines.length) {
    list.innerHTML =
      '<div class="spec-row"><span class="spec-value">No LLM engines available — reload the Dashboard or check the logs.</span></div>';
    return;
  }
  list.textContent = "";
  const specDot = (on) => {
    const s = document.createElement("span");
    s.className = on ? "spec-dot dot-ok" : "spec-dot dot-bad";
    return s;
  };
  const engineBtn = (cls, label, engineId, extra) => {
    const b = document.createElement("button");
    b.className = "pip-install-btn " + cls;
    b.dataset.engine = engineId;
    b.textContent = label;
    if (extra) for (const k of Object.keys(extra)) b.dataset[k] = extra[k];
    return b;
  };
  const hintDiv = (text, color) => {
    const d = document.createElement("div");
    d.className = "pip-advanced-hint";
    if (color) d.style.color = color;
    d.textContent = text;
    return d;
  };
  for (const e of engines) {
    const card = document.createElement("div");
    card.className = "llm-engine-card";
    const head = document.createElement("div");
    head.className = "llm-engine-head";
    const title = document.createElement("span");
    title.className = "llm-engine-title";
    title.textContent = e.label;
    head.append(title);
    if (e.install && e.install.mode === "pip") {
      const done = e.pipInstalled;
      head.append(
        engineBtn(
          "llm-install-btn",
          (done ? "Reinstall " : "Install ") + e.install.spec,
          e.id,
        ),
      );
      if (done) head.append(engineBtn("llm-remove-btn", "Remove", e.id));
    } else if (e.install && e.install.mode === "npm") {
      const done = e.cliOnPath;
      head.append(
        engineBtn(
          "llm-install-btn",
          (done ? "Reinstall via npm (" : "Install via npm (") +
            e.install.spec +
            ")",
          e.id,
        ),
      );
      if (done) head.append(engineBtn("llm-remove-btn", "Remove", e.id));
    } else if (e.external) {
      const s = document.createElement("span");
      s.className = "spec-value llm-external-hint";
      s.textContent = "External — install via terminal, then it auto-detects.";
      head.append(s);
    }
    card.append(head);
    const specs = document.createElement("div");
    specs.className = "env-specs";
    const specRow = (labelText, dotOn, valueText) => {
      const d = document.createElement("div");
      d.className = "spec-row";
      const lab = document.createElement("span");
      lab.className = "spec-label";
      lab.textContent = labelText;
      const val = document.createElement("span");
      val.className = "spec-value";
      val.textContent = valueText;
      d.append(lab, specDot(dotOn), val);
      return d;
    };
    if (e.cli)
      specs.append(
        specRow(
          e.cli + " CLI",
          e.cliOnPath,
          e.cliOnPath ? "on PATH" : "not found",
        ),
      );
    if (e.pipPackage)
      specs.append(
        specRow(
          e.pipPackage,
          e.pipInstalled,
          e.pipInstalled ? "installed" : "missing",
        ),
      );
    if (e.serverUrl) {
      const d = document.createElement("div");
      d.className = "spec-row";
      const lab = document.createElement("span");
      lab.className = "spec-label";
      lab.textContent = "Server";
      const val = document.createElement("span");
      val.className = "spec-value";
      val.textContent = e.serverUrl;
      d.append(lab, val);
      specs.append(d);
    }
    card.append(specs);
    if (e.serve) {
      const row = document.createElement("div");
      row.className = "llm-serve-row";
      row.append(
        engineBtn(
          "llm-serve-btn",
          e.serverRunning ? "Stop server" : "Start server",
          e.id,
        ),
      );
      card.append(row);
    }
    if (e.auth) {
      // Open the official Claude Code authentication guide (the user asked for a
      // how-to page, not a silent terminal launch that blocks on Max/Pro).
      const row = document.createElement("div");
      row.className = "llm-serve-row";
      row.append(
        engineBtn("llm-auth-btn", "How to sign in", e.id, {
          authDocs: e.auth.docsUrl || "",
        }),
      );
      card.append(row);
    }
    card.append(hintDiv(e.desc));
    if (e.auth) card.append(hintDiv(e.auth.help));
    if (e.claudeApiKeySet)
      card.append(
        hintDiv(
          "✓ Anthropic API key active — Claude Code will use it instead of a Max/Pro login (needs API credits in the Console; billed per use).",
          "#4ADE80",
        ),
      );
    if (e.notes) card.append(hintDiv(e.notes));
    list.append(card);
  }
  list.querySelectorAll(".llm-install-btn").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const id = btn.dataset.engine;
      btn.disabled = true;
      btn.textContent = "installing...";
      const r = await window.w2gp.llmEngineInstall(id);
      if (r && r.success) {
        _llmEnginesPromise = null;
        showToast("✓ engine installed");
        refreshLLMEngines();
      } else {
        btn.disabled = false;
        showToast("✗ " + (r && r.error ? r.error : "install failed"));
      }
    });
  });
  list.querySelectorAll(".llm-remove-btn").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const id = btn.dataset.engine;
      if (!confirm("Remove " + id + " from this machine?")) return;
      btn.disabled = true;
      btn.textContent = "removing...";
      const r = await window.w2gp.llmEngineUninstall(id);
      if (r && r.success) {
        _llmEnginesPromise = null;
        showToast("✓ engine removed");
        refreshLLMEngines();
      } else {
        btn.disabled = false;
        btn.textContent = "Remove";
        showToast("✗ " + (r && r.error ? r.error : "remove failed"));
      }
    });
  });
  list.querySelectorAll(".llm-serve-btn").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const id = btn.dataset.engine;
      const starting = btn.textContent.trim().startsWith("Start");
      btn.disabled = true;
      const r = await window.w2gp.llmEngineServe(
        id,
        starting ? "start" : "stop",
      );
      btn.disabled = false;
      if (r && r.success) {
        btn.textContent = starting ? "Stop server" : "Start server";
        showToast(
          starting ? "✓ " + id + " server started" : "✓ server stopped",
        );
      } else {
        showToast("✗ " + (r && r.error ? r.error : "server action failed"));
      }
    });
  });
  list.querySelectorAll(".llm-auth-btn").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const url = btn.dataset.authDocs;
      if (url) {
        await window.w2gp.openExternal(url);
        showToast("Opened Claude Code authentication guide");
      } else {
        showToast("No sign-in guide configured for this engine");
      }
    });
  });
}

// Deepy Prime activation panel: pick a ready engine, write it into
// wgp_config.json so the next Wan2GP launch boots with Deepy Prime enabled.
const DEEPY_PANEL_ENGINES = [
  { id: "opencode", label: "OpenCode", paid: false },
  { id: "claude-code", label: "Claude Code", paid: true },
  { id: "codex", label: "OpenAI Codex", paid: true },
  // ponytail: b71026f — local Prime runs on Qwen3.8 VL 27B (needs the 27B model + GGUF 1.0.22; backend auto-raises 32k context + Summarize)
  { id: "local-qwen38", label: "Qwen3.8 VL 27B (local)", paid: false },
];

// Local-model (Prompt Enhancer) choices shown in the Deepy panel when Deepy is
// Disabled or Zero. Mirrors services/deepy-config.js DEEPY_ENHANCER_OPTIONS.
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
];

// Qwen LLM Quantization choices per local engine id — mirrors upstream's
// "Qwen LLM Quantization" dropdown (plugins/configuration/plugin.py
// QWEN38_QUANTIZATION_CHOICES). Bonsai PTQ1 (gguf_ptq1) is the ~10 GB VRAM
// checkpoint for Qwen3.8; Qwen3.5 offers Quanto Int8 or GGUF Q4.
const DEEPY_QUANT_CHOICES = {
  5: [
    { id: "gguf", label: "GGUF Q4 (default, highest quality)" },
    { id: "gguf_q3", label: "GGUF IQ3_S (middle, 16 GB VRAM)" },
    { id: "gguf_q2", label: "GGUF Q2 (lowest memory)" },
    { id: "gguf_ptq1", label: "Bonsai PTQ1 (~10 GB VRAM, needs kernels 1.0.22+)" },
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
const DEEPY_QUANT_DEFAULT = { 5: "gguf", 4: "quanto_int8", 3: "quanto_int8" };
// Last-rendered "enhancerId|savedQuant" key — options rebuild only when the
// engine context changes so syncApply validation never resets a choice.
let _deepyQuantCtx = "";
// Render the quant selector for a local Qwen engine id (3/4/5), or hide it
// (quant untouched) for remote engines / Florence / Disabled.
function renderDeepyQuant(enhancerId, savedQuant) {
  const wrap = $("deepyQuantWrap");
  const sel = $("deepyQuantSelect");
  const hint = $("deepyQuantHint");
  if (!wrap || !sel) return;
  const choices = (enhancerId && DEEPY_QUANT_CHOICES[enhancerId]) || null;
  if (!choices) {
    wrap.style.display = "none";
    _deepyQuantCtx = "";
    return;
  }
  const key = enhancerId + "|" + (savedQuant || "");
  wrap.style.display = "block";
  if (key !== _deepyQuantCtx) {
    _deepyQuantCtx = key;
    sel.textContent = "";
    for (const c of choices) {
      const o = document.createElement("option");
      o.value = c.id;
      o.textContent = c.label;
      sel.append(o);
    }
    sel.value = choices.some((c) => c.id === savedQuant)
      ? savedQuant
      : DEEPY_QUANT_DEFAULT[enhancerId];
  }
  sel.dataset.enh = String(enhancerId);
  if (hint)
    updateDeepyQuantHint(sel.value, enhancerId);
  sel.onchange = () => updateDeepyQuantHint(sel.value, enhancerId);
}
// Hint under the quant selector; Bonsai notes its companion defaults.
function updateDeepyQuantHint(value, enhancerId) {
  const hint = $("deepyQuantHint");
  if (!hint) return;
  hint.textContent =
    value === "gguf_ptq1"
      ? "Bonsai PTQ1 runs Prime on ~10 GB VRAM (Sync Kernels for 1.0.22+). Weights download on first WanGP launch. Apply also sets INT8 KV cache."
      : Number(enhancerId) === 5
        ? "GGUF Q4 is highest quality; Q3/Q2 trade quality for VRAM. Weights download on first WanGP launch."
        : "Quanto Int8 preserves quality; GGUF Q4 uses less memory when kernels are installed.";
}

async function refreshDeepy() {
  const opts = $("deepyEngineOptions");
  const statusMsg = $("deepyStatusMsg");
  const applyBtn = $("deepyApplyBtn");
  const promptApplyBtn = $("deepyPromptApplyBtn");
  const promptStatusMsg = $("deepyPromptStatusMsg");
  const docsLink = $("deepyDocsLink");
  const primeOnly = $("deepyPrimeOnly");
  const enhancerWrap = $("deepyEnhancerWrap");
  const enhancerOpts = $("deepyEnhancerOptions");
  const enhancerHint = $("deepyEnhancerHint");
  const modeRadios = document.querySelectorAll("input[name=deepyMode]");
  if (!applyBtn) return;

  let status = { available: false };
  let engines = [];
  try {
    const s = await window.w2gp.deepyStatus();
    if (s && s.ok) status = s;
  } catch {}
  try {
    const d = await getLLMEngines();
    engines = (d && d.engines) || [];
  } catch {}

  const ready = (id) => {
    // ponytail: local model lives in Wan2GP — it validates the 27B requirement + downloads on first use, nothing for the launcher to probe
    if (id === "local-qwen38") return true;
    const e = engines.find((x) => x.id === id);
    if (!e) return false;
    if (id === "claude-code") return !!(e.cliOnPath || e.claudeApiKeySet);
    return !!e.cliOnPath;
  };

  const currentProfile = status.currentEngine;
  const profileToUi = {
    opencode: "opencode",
    claude: "claude-code",
    codex: "codex",
    qwen38_27b: "local-qwen38",
  };
  const currentUi = profileToUi[currentProfile] || null;
  const currentMode = status.mode || "disabled";
  const currentEnhancer =
    typeof status.enhancerEnabled === "number" ? status.enhancerEnabled : null;
  // Saved quant backend (upstream prompt_enhancer_quantization) for preselect.
  const savedQuant =
    typeof status.promptEnhancerQuantization === "string"
      ? status.promptEnhancerQuantization
      : null;
  // Sessions section — pre-select from config (backend normalizes; missing
  // keys fall back to upstream defaults). Launcher default for a fresh
  // config is the selectable shared workspace.
  const validSessionMode = ["disabled", "selectable", "dedicated"];
  const curSessionMode = validSessionMode.includes(status.sessionMode)
    ? status.sessionMode
    : "selectable";
  const curResetMode =
    status.sessionResetMode === "reset_session"
      ? "reset_session"
      : "new_session";
  const curGalleryMode =
    status.sessionGalleryMediaMode === "copy" ? "copy" : "link";
  if ($("deepySessionMode")) $("deepySessionMode").value = curSessionMode;
  if ($("deepySessionReset")) $("deepySessionReset").value = curResetMode;
  if ($("deepySessionGallery")) $("deepySessionGallery").value = curGalleryMode;
  // Prompt-enhancement UI — pre-select from config; missing key defaults to
  // the Enhance Prompt button (1).
  if ($("deepyEnhancerMode"))
    $("deepyEnhancerMode").value = status.enhancerMode === 0 ? "0" : "1";
  // Default engine for Prime is OpenCode (universal providers / external, free).
  // Preserve an already-configured engine; otherwise fall back to OpenCode.
  const selectedEngine = currentUi || "opencode";

  // Pre-select the current Deepy mode (Disabled / Zero / Prime).
  modeRadios.forEach((r) => {
    r.checked = r.value === currentMode;
  });
  primeOnly.style.display = currentMode === "prime" ? "block" : "none";

  // Local-model (Prompt Enhancer) selector: shown for Disabled/Zero only.
  // Rendered from the SELECTED mode (not just persisted), so switching modes
  // immediately re-renders the local-model choices. Selection is transient —
  // only persisted when Apply is pressed.
  const renderEnhancer = (mode, preselectId) => {
    const visible = mode === "disabled" || mode === "zero";
    enhancerWrap.style.display = visible ? "block" : "none";
    if (!visible) return;
    const forThisMode = (o) => o.modes.includes(mode);
    // Pre-select: caller-supplied id if valid, else persisted id, else first
    // valid option for this mode.
    const validForMode = DEEPY_PANEL_ENHANCERS.filter(forThisMode);
    const chosen =
      validForMode.find((o) => o.id === preselectId) ||
      validForMode.find((o) => o.id === currentEnhancer) ||
      validForMode[0];
    const sub =
      mode === "zero"
        ? "Deepy Zero runs locally — pick the Qwen model Wan2GP will use."
        : "Prompt enhancement runs without Deepy too — pick any local model.";
    if (enhancerHint) enhancerHint.textContent = sub;
    enhancerOpts.textContent = "";
    for (const o of DEEPY_PANEL_ENHANCERS) {
      const enabled = forThisMode(o);
      const lab = document.createElement("label");
      lab.className = enabled
        ? "deepy-enhancer-opt"
        : "deepy-enhancer-opt deepy-enhancer-opt-disabled";
      const radio = document.createElement("input");
      radio.type = "radio";
      radio.name = "deepyEnhancer";
      radio.value = o.id;
      if (o.id === chosen.id) radio.checked = true;
      if (!enabled) radio.disabled = true;
      const name = document.createElement("span");
      name.className = "deepy-enhancer-label";
      name.textContent = o.label;
      lab.append(radio, "\n  ", name);
      if (!enabled) {
        const note = document.createElement("span");
        note.className = "deepy-enhancer-note";
        note.textContent =
          " — only for " + (o.modes[0] === "zero" ? "Deepy Zero" : "Disabled");
        lab.append(note);
      }
      lab.append("\n");
      enhancerOpts.append(lab);
    }
  };
  renderEnhancer(currentMode, currentEnhancer);

  opts.textContent = "";
  for (const en of DEEPY_PANEL_ENGINES) {
    const isReady = ready(en.id);
    const lab = document.createElement("label");
    lab.className = "deepy-engine-opt";
    const radio = document.createElement("input");
    radio.type = "radio";
    radio.name = "deepyEngine";
    radio.value = en.id;
    if (en.id === selectedEngine) radio.checked = true;
    const dot = document.createElement("span");
    dot.className = isReady ? "dot-ok" : "dot-bad";
    dot.textContent = isReady ? "●" : "○";
    const name = document.createElement("span");
    name.className = "deepy-engine-label";
    name.textContent = en.label;
    const cost = document.createElement("span");
    cost.className = "deepy-engine-cost";
    cost.textContent = en.paid ? "paid" : "free";
    lab.append(
      radio,
      "\n          ",
      dot,
      "\n          ",
      name,
      "\n          ",
      cost,
      "\n        ",
    );
    opts.append(lab);
  }

  statusMsg.textContent = "";
  if (status.available) {
    const label =
      {
        disabled: "Disabled",
        zero: "Deepy Zero (local model)",
        prime: "Deepy Prime",
      }[currentMode] || currentMode;
    const st = document.createElement("strong");
    st.textContent = label;
    statusMsg.append(
      "Currently: ",
      st,
      currentMode === "prime" && currentProfile
        ? " — engine: " + currentProfile
        : "",
    );
  } else {
    const s = document.createElement("span");
    s.style.color = "#FBBF24";
    s.textContent =
      status.reason || "Wan2GP config not found — install Wan2GP first.";
    statusMsg.append(s);
  }

  const syncApply = () => {
    const mode =
      (document.querySelector("input[name=deepyMode]:checked") || {}).value ||
      "disabled";
    primeOnly.style.display = mode === "prime" ? "block" : "none";
    enhancerWrap.style.display =
      mode === "disabled" || mode === "zero" ? "block" : "none";
    let ok = true;
    let title = "";
    if (mode === "prime") {
      const eng = (opts.querySelector("input[name=deepyEngine]:checked") || {})
        .value;
      if (!eng) {
        ok = false;
        title = "Pick an engine for Deepy Prime";
      } else if (!ready(eng)) {
        ok = false;
        title = "Install / enable this engine first (see LLM Engines above)";
      }
    } else if (mode === "disabled" || mode === "zero") {
      const enh = (
        enhancerOpts.querySelector("input[name=deepyEnhancer]:checked") || {}
      ).value;
      // ponytail: Tauri — don't block Apply if enhancer not yet rendered; Rust defaults to 3 (Qwen 4B) for Zero
      if (!enh && !window.__TAURI__) {
        ok = false;
        title = "Pick a local model (Prompt Enhancer)";
      }
    }
    // Qwen quant selector follows the local engine: Disabled/Zero + Qwen 3/4/5,
    // or Prime + local Qwen3.8 (id 5). Hidden otherwise (quant untouched).
    const qEnhRaw =
      mode === "zero" || mode === "disabled"
        ? parseInt(
            (enhancerOpts.querySelector("input[name=deepyEnhancer]:checked") || {})
              .value,
            10,
          )
        : mode === "prime" &&
            ((opts.querySelector("input[name=deepyEngine]:checked") || {}).value ===
              "local-qwen38")
          ? 5
          : NaN;
    renderDeepyQuant([3, 4, 5].includes(qEnhRaw) ? qEnhRaw : null, savedQuant);
    // Both Apply buttons (Deepy card + Prompt enhancement card) share one
    // coherent config write, so they enable/disable together.
    for (const b of [applyBtn, promptApplyBtn]) {
      if (!b) continue;
      b.disabled = !ok;
      b.title = title || "Set Deepy to " + mode;
    }
  };
  modeRadios.forEach((r) =>
    r.addEventListener("change", () => {
      // Switching the mode immediately re-renders the local-model selector for
      // the newly-selected mode (transient — not persisted until Apply).
      const m =
        (document.querySelector("input[name=deepyMode]:checked") || {}).value ||
        "disabled";
      renderEnhancer(m);
      syncApply();
    }),
  );
  opts
    .querySelectorAll("input[name=deepyEngine]")
    .forEach((r) => r.addEventListener("change", syncApply));
  enhancerOpts
    .querySelectorAll("input[name=deepyEnhancer]")
    .forEach((r) => r.addEventListener("change", syncApply));
  // Dropdowns have no validation of their own, but changing one is a pending
  // change — enable both Apply buttons.
  for (const id of [
    "deepySessionMode",
    "deepySessionReset",
    "deepySessionGallery",
    "deepyEnhancerMode",
    "deepyQuantSelect",
  ]) {
    $(id)?.addEventListener("change", syncApply);
  }
  syncApply();

  // One shared write for both cards: reads the whole panel state and applies
  // it coherently. Each Apply button reports into its own card's message line.
  const applyDeepy = async (btn, msgEl) => {
    const mode =
      (document.querySelector("input[name=deepyMode]:checked") || {}).value ||
      "disabled";
    const eng = (opts.querySelector("input[name=deepyEngine]:checked") || {})
      .value;
    const enh = (
      enhancerOpts.querySelector("input[name=deepyEnhancer]:checked") || {}
    ).value;
    btn.disabled = true;
    btn.textContent = "applying...";
    const sessions = {
      multi_session: ($("deepySessionMode") || {}).value || "selectable",
      reset_mode: ($("deepySessionReset") || {}).value || "new_session",
      gallery_media_mode: ($("deepySessionGallery") || {}).value || "link",
    };
    // Quant only when its selector is visible (local Qwen engine) — hidden
    // means preserve whatever WanGP already has.
    const quantWrap = $("deepyQuantWrap");
    const quantSel = $("deepyQuantSelect");
    const quant =
      quantWrap && quantWrap.style.display !== "none" && quantSel && quantSel.value
        ? quantSel.value
        : null;
    // Prompt-enhancement UI: "1" = Enhance Prompt button, "0" = Automatic.
    const enhancerMode = ($("deepyEnhancerMode") || {}).value || "1";
    const r = await window.w2gp.deepySet(
      mode,
      eng,
      enh ? parseInt(enh, 10) : null,
      sessions,
      quant,
      enhancerMode,
    );
    btn.textContent = "Apply";
    // No pending changes left — park both buttons until the next edit.
    for (const b of [applyBtn, promptApplyBtn]) if (b) b.disabled = true;
    if (r && r.ok) {
      const s = document.createElement("span");
      s.style.color = "#4ADE80";
      s.textContent = "✓ " + (r.message || "Deepy updated");
      msgEl.textContent = "";
      msgEl.append(s);
      showToast("✓ " + (r.message || "Deepy updated"));
      appendLog(
        "[Deepy] ✓ " +
          (r.message || "Deepy mode: " + mode) +
          " (sessions: " +
          sessions.multi_session +
          "/" +
          sessions.reset_mode +
          "/" +
          sessions.gallery_media_mode +
          (quant ? "; quant: " + quant : "") +
          "; prompt: " +
          (enhancerMode === "0" ? "automatic" : "button") +
          ")",
      );
    } else {
      const s = document.createElement("span");
      s.style.color = "#F87171";
      s.textContent = "✗ " + ((r && r.error) || "update failed");
      msgEl.textContent = "";
      msgEl.append(s);
      showToast("✗ " + (r && r.error ? r.error : "update failed"));
      appendLog(
        "[Deepy] ✗ update failed: " + ((r && r.error) || "update failed"),
      );
      for (const b of [applyBtn, promptApplyBtn]) if (b) b.disabled = false;
    }
  };
  applyBtn.onclick = () => applyDeepy(applyBtn, statusMsg);
  if (promptApplyBtn)
    promptApplyBtn.onclick = () =>
      applyDeepy(promptApplyBtn, promptStatusMsg || statusMsg);
  if (docsLink)
    docsLink.onclick = async (ev) => {
      ev.preventDefault();
      await window.w2gp.openExternal(
        "https://github.com/deepbeepmeep/Wan2GP/blob/main/docs/DEEPY.md",
      );
    };
}
// ── Deepy Web standalone (openspec `deepy-web` Slice 0: Same-PC + Phone-LAN HTTP) ──
let _deepyWeb = { running: false, port: null, urls: null };
let _deepyWebPrefsLoaded = false;
function deepyWebMode() {
  return (
    (document.querySelector("input[name=deepyWebMode]:checked") || {}).value ||
    "same-pc"
  );
}
// Assistant level for the Deepy Web process: "zero" | "prime" | null
// (null = follow the saved Deepy panel config). Persisted to desktop-config
// so the choice survives reloads; applied via deepy_set at Start time.
function deepyWebAssistant() {
  const v = (
    document.querySelector("input[name=deepyWebAssistant]:checked") || {}
  ).value;
  return v === "zero" || v === "prime" ? v : null;
}
function deepyWebAssistantHintSync() {
  const hint = $("deepyWebAssistantHint");
  if (!hint) return;
  const a = deepyWebAssistant();
  hint.textContent =
    a === "prime"
      ? "Prime for this start — uses your saved Prime engine (or the first installed one). Applied when you press Start (also saved for next time)."
      : a === "zero"
        ? "Zero for this start — fast, local Qwen model. Applied when you press Start (also saved for next time)."
        : "Uses the saved Deepy config. Pick Zero or Prime for this start — applied when you press Start (also saved for next time).";
}

// Stored Prime profile id (wgp_config llm_engines.deepy) -> Deepy panel UI id.
// Must match the panel's profileToUi in refreshDeepy + Rust prime_profile_to_ui_id.
function deepyWebProfileToUi(profile) {
  return (
    {
      opencode: "opencode",
      claude: "claude-code",
      codex: "codex",
      qwen38_27b: "local-qwen38",
    }[profile] || null
  );
}

// Resolve which Prime engine a Deepy Web start should boot: the SAVED Prime
// engine when one is configured (never silently switched to OpenCode), else
// the panel default (OpenCode) when installed, else any installed remote,
// else local Qwen3.8 27B (the boot verifies weights fail-closed). Returns
// { engine } or { error } — the caller blocks the start on error instead of
// writing a broken config.
async function resolveDeepyWebPrimeEngine() {
  let status = {};
  try {
    const s = await window.w2gp.deepyStatus();
    if (s && s.ok) status = s;
  } catch {}
  let engines = [];
  try {
    const d = await getLLMEngines();
    engines = (d && d.engines) || [];
  } catch {}
  const engineReady = (id) => {
    // Local weights live in Wan2GP — installedness is verified at boot
    // (fail-closed), nothing for the launcher to probe here.
    if (id === "local-qwen38") return true;
    const e = engines.find((x) => x.id === id);
    if (!e) return false;
    if (id === "claude-code") return !!(e.cliOnPath || e.claudeApiKeySet);
    return !!e.cliOnPath;
  };
  const savedUi = deepyWebProfileToUi(status.currentEngine);
  if (status.mode === "prime" && savedUi) {
    if (engineReady(savedUi)) return { engine: savedUi };
    return {
      error:
        "Saved Deepy Prime engine (" +
        status.currentEngine +
        ") is not installed — pick an installed engine in the Deepy panel above and press Apply first.",
    };
  }
  if (engineReady("opencode")) return { engine: "opencode" };
  for (const id of ["claude-code", "codex"]) {
    if (engineReady(id)) return { engine: id };
  }
  // Last resort: local Prime — deepy_web_start verifies the 27B weights and
  // blocks with an actionable error when they are missing.
  return { engine: "local-qwen38" };
}
function deepyWebPortArg() {
  const raw = ($("deepyWebPortInput") || {}).value;
  const n = parseInt(String(raw == null ? "" : raw).trim(), 10);
  return Number.isFinite(n) && n >= 1 && n <= 65535 ? n : null;
}
function deepyWebAuthMode() {
  return (
    (document.querySelector("input[name=deepyWebAuth]:checked") || {}).value ||
    "off"
  );
}
function deepyWebAuthFixed() {
  const v = ($("deepyWebAuthFixed") || {}).value;
  return typeof v === "string" && v ? v : "";
}
const DEEPY_SAVED_DAYS = 30;
function deepyWebSavedSecret(cfg) {
  const s = (cfg && cfg.deepySavedPassword) || "";
  const exp = parseInt((cfg && cfg.deepyPasswordExpiry) || "0", 10);
  if (!s || !Number.isFinite(exp) || exp <= Date.now()) return null;
  return { secret: s, until: new Date(exp) };
}
function renderDeepyWebSavedNote(saved, expired) {
  const note = $("deepyWebSavedNote");
  if (!note) return;
  if (saved) {
    note.style.display = "";
    note.textContent =
      "Remembered password valid until " +
      saved.until.toLocaleString() +
      " — leave the field empty to reuse it, or Forget to delete it.";
  } else if (expired) {
    note.style.display = "";
    note.textContent = "Saved password expired — type a new one or generate.";
  } else {
    note.style.display = "none";
    note.textContent = "";
  }
}
function deepyWebHttpsOn() {
  return (
    ((document.querySelector("input[name=deepyWebHttps]:checked") || {})
      .value || "off") === "on"
  );
}
function deepyWebHttpsPaths() {
  const c = ($("deepyWebCertInput") || {}).value;
  const k = ($("deepyWebKeyInput") || {}).value;
  return {
    cert: typeof c === "string" ? c.trim() : "",
    key: typeof k === "string" ? k.trim() : "",
  };
}
function deepyWebPublicUrl() {
  const v = ($("deepyWebPublicUrlInput") || {}).value;
  return typeof v === "string" ? v.trim() : "";
}
async function deepyWebCertFlow(action) {
  const p = deepyWebHttpsPaths();
  try {
    const r = await window.w2gp.deepyWebCert(action, p.cert, p.key);
    if (r && r.ok) {
      if (r.certPath && $("deepyWebCertInput"))
        $("deepyWebCertInput").value = r.certPath;
      if (r.keyPath && $("deepyWebKeyInput"))
        $("deepyWebKeyInput").value = r.keyPath;
      try {
        const cfg = await window.w2gp.configLoad().catch(() => ({}));
        if (cfg && typeof cfg === "object") {
          cfg.deepyCertPath = r.certPath || p.cert;
          cfg.deepyKeyPath = r.keyPath || p.key;
          await window.w2gp.configSave(cfg).catch(() => {});
        }
      } catch {}
      const on = document.querySelector(
        'input[name=deepyWebHttps][value="on"]',
      );
      if (on) on.checked = true;
      showToast("✓ HTTPS cert ready" + (r.guide ? " — " + r.guide : ""));
      appendLog("[Deepy] ✓ HTTPS cert ready (" + action + ")");
    } else {
      showToast("✗ Cert: " + ((r && r.error) || "failed"));
      appendLog("[Deepy] ✗ cert failed: " + ((r && r.error) || "failed"));
    }
  } catch (e) {
    showToast("✗ " + errText(e));
    appendLog("[Deepy] ✗ cert error: " + errText(e));
  }
  refreshDeepyWeb();
}
async function refreshDeepyWeb(light = false) {
  const statusEl = $("deepyWebStatus");
  if (!statusEl) return;
  const samePcEl = $("deepyWebSamePcOpen");
  const phoneEl = $("deepyWebPhoneOpen");
  const phoneHint = $("deepyWebPhoneHint");
  const startBtn = $("deepyWebStartBtn");
  const stopBtn = $("deepyWebStopBtn");
  if (!_deepyWebPrefsLoaded) {
    _deepyWebPrefsLoaded = true;
    try {
      const cfg = await window.w2gp.configLoad().catch(() => ({}));
      if (cfg && typeof cfg === "object") {
        if (cfg.deepyListen === true) {
          const lan = document.querySelector(
            'input[name=deepyWebMode][value="lan"]',
          );
          if (lan) lan.checked = true;
        }
        const _authPref =
          (cfg && cfg.deepyAuthMode) === "generate"
            ? "fixed"
            : (cfg && cfg.deepyAuthMode) || "off";
        const authRadio = document.querySelector(
          'input[name=deepyWebAuth][value="' + _authPref + '"]',
        );
        if (authRadio) authRadio.checked = true;
        if ($("deepyWebCertInput") && cfg.deepyCertPath)
          $("deepyWebCertInput").value = cfg.deepyCertPath;
        if ($("deepyWebKeyInput") && cfg.deepyKeyPath)
          $("deepyWebKeyInput").value = cfg.deepyKeyPath;
        if ($("deepyWebPublicUrlInput") && typeof cfg.deepyPublicUrl === "string" && !$("deepyWebPublicUrlInput").value)
          $("deepyWebPublicUrlInput").value = cfg.deepyPublicUrl;
        if (cfg.deepyCertPath || cfg.deepyKeyPath) {
          const httpsOn = document.querySelector(
            'input[name=deepyWebHttps][value="on"]',
          );
          if (httpsOn) httpsOn.checked = true;
        }
        renderDeepyWebSavedNote(
          deepyWebSavedSecret(cfg),
          !!cfg.deepySavedPassword,
        );
        if (cfg.deepySavedPassword && !deepyWebSavedSecret(cfg)) {
          try {
            const cfg2 = await window.w2gp.configLoad().catch(() => null);
            if (cfg2 && typeof cfg2 === "object") {
              delete cfg2.deepySavedPassword;
              delete cfg2.deepyPasswordExpiry;
              await window.w2gp.configSave(cfg2).catch(() => {});
            }
          } catch {}
        }
        if ($("deepyWebPortInput") && !$("deepyWebPortInput").value) {
          const p = parseInt(cfg.deepyPort, 10);
          if (Number.isFinite(p) && p >= 1 && p <= 65535)
            $("deepyWebPortInput").value = String(p);
        }
        // Assistant pre-select (persisted per-start choice, no default).
        if (
          cfg.deepyWebAssistant === "zero" ||
          cfg.deepyWebAssistant === "prime"
        ) {
          const ar = document.querySelector(
            'input[name=deepyWebAssistant][value="' +
              cfg.deepyWebAssistant +
              '"]',
          );
          if (ar) ar.checked = true;
        }
        deepyWebAssistantHintSync();
      }
    } catch {}
  }
  let s = null;
  try {
    s = await window.w2gp.deepyWebStatus(deepyWebPortArg());
  } catch (e) {
    statusEl.textContent = "✗ " + errText(e);
    return;
  }
  if (!s || !s.ok) {
    statusEl.textContent = "✗ " + ((s && s.error) || "Deepy Web status failed");
    return;
  }
  _deepyWeb = {
    running: !!s.running,
    port: s.port,
    urls: s.urls || null,
  };
  // Topbar Deepy Web LED mirrors the card: green = process up, red = down.
  updateDeepyWebLed(!!s.running);
  const urls = s.urls || {};
  statusEl.textContent = "";
  if (s.running) {
    const dot = document.createElement("span");
    dot.style.color = "#4ADE80";
    dot.textContent = "● Running";
    statusEl.append(
      dot,
      " on :" +
        s.port +
        " — open the Same-PC URL here or scan the Phone URL from your phone.",
    );
  } else {
    const b = document.createElement("strong");
    b.textContent = "Start Deepy Web";
    statusEl.append("○ Stopped — pick a mode above, then ", b, ".");
    if (urls.clashNotice) statusEl.append(" " + urls.clashNotice);
  }
  const _setUrlBtn = (el, url, fallbackText) => {
    if (!el) return;
    const has = typeof url === "string" && url.length > 0;
    el.textContent = has ? url : fallbackText || "—";
    el.disabled = !has;
  };
  _setUrlBtn(samePcEl, urls.samePc);
  _setUrlBtn(phoneEl, urls.phone, "unavailable — see hint below");
  // External row: Tailscale IPv4 URL for off-LAN access. Hidden when
  // Tailscale is not active (backend sends external: null).
  const extRow = $("deepyWebExternalRow");
  const extEl = $("deepyWebExternalOpen");
  const extHint = $("deepyWebExternalHint");
  if (extEl) _setUrlBtn(extEl, urls.external);
  if (extRow) extRow.style.display = urls.external ? "" : "none";
  if (extHint) {
    if (urls.external) {
      extHint.style.display = "";
      extHint.textContent =
        deepyWebMode() === "lan"
          ? "External via Tailscale IPv4 " +
            (urls.externalIp || "") +
            " — reachable from outside your LAN (Tailscale on both ends). Phone-LAN mode serves it."
          : "External URL is shown for convenience — start in Phone-LAN mode to serve it.";
    } else {
      extHint.style.display = "none";
      extHint.textContent = "";
    }
  }
  if (phoneHint) {
    if (urls.phone) {
      phoneHint.textContent =
        deepyWebMode() === "lan"
          ? "Phone-LAN mode uses --listen: Windows may show a firewall prompt — allow it on private networks. No firewall rules are created silently."
          : "Phone URL is shown for convenience — start in Phone-LAN mode to serve it.";
    } else {
      phoneHint.textContent =
        urls.phoneGuidance ||
        "No LAN adapter found — connect to Wi-Fi/Ethernet to enable the Phone URL.";
    }
  }
  if (startBtn) startBtn.disabled = !!s.running;
  if (stopBtn) stopBtn.disabled = false; // always clickable: kills orphans even when the card thinks Stopped.
  renderDeepyWebIpList(urls);
  try {
    const banner = $("deepyWebAuthBanner");
    const warn = $("deepyWebAuthWarning");
    const pwBox = $("deepyWebAuthPassword");
    const liveMode = (s && (s.authMode || s.auth_mode)) || deepyWebAuthMode();
    const liveOn =
      !!(s && (s.authEnabled || s.auth_enabled)) ||
      (!!s.running && liveMode !== "off");
    const lanNow = deepyWebMode() === "lan";
    if (banner) {
      if (liveOn) {
        banner.style.display = "";
        banner.textContent =
          "Auth on (" +
          liveMode +
          "): credentials expire within 24h / on restart — restart to rotate. " +
          "After every restart, reload the phone page before signing in (old pages get rejected). " +
          "Rate limits: first 4 tries free, then 30s wait growing +30s per failure up to 450s; from the 20th failure one try per 10 min; success resets the counter.";
      } else {
        banner.style.display = "none";
        banner.textContent = "";
      }
    }
    if (warn) {
      if (lanNow && liveMode !== "off") {
        warn.style.display = "";
        warn.textContent =
          "Blocking: LAN + Auth over plain HTTP sends the password unencrypted — enable LAN HTTPS or use Same-PC-only.";
      } else {
        warn.style.display = "none";
        warn.textContent = "";
      }
    }
    const authHint = $("deepyWebAuthHint");
    const authCtrls = [
      ...document.querySelectorAll("input[name=deepyWebAuth]"),
      $("deepyWebAuthFixed"),
      $("deepyWebAuthLen"),
      $("deepyWebAuthGen"),
      $("deepyWebAuthShow"),
      $("deepyWebAuthRemember"),
      $("deepyWebAuthForget"),
    ];
    authCtrls.forEach((el) => {
      if (el) el.disabled = !!s.running;
    });
    if (authHint) {
      authHint.textContent = s.running
        ? "Stop Deepy Web to change auth — settings apply at start only."
        : "Password via child env only. Without Remember, restart invalidates it. A remembered password is stored on this PC in plain text. Tip: 3–4 plain words you can type on the phone.";
    }
    if (pwBox && !s.running) {
      pwBox.style.display = "none";
      pwBox.textContent = "";
    }
    try {
      const mic = $("deepyWebMicGate");
      const httpsHint = $("deepyWebHttpsHint");
      const httpsOnPref = deepyWebHttpsOn();
      if (mic) {
        if (httpsOnPref && s.running) {
          mic.style.display = "";
          mic.textContent =
            "Trusted LAN HTTPS on — voice input allowed. Phones need the CA installed (guide above).";
        } else {
          mic.style.display = "";
          mic.textContent =
            "Mic/voice requires trusted LAN HTTPS + CA install — blocked on plain HTTP. Turn LAN HTTPS on above.";
        }
      }
      if (httpsHint && s.running && httpsOnPref) {
        httpsHint.textContent =
          "HTTPS on: this port serves HTTPS only — open the https:// URL (exact address the cert covers). Plain http:// on this port will not load.";
      }
    } catch {}
  } catch {}
  if (!light) refreshDeepyWebTailscale().catch(() => {});
}
async function refreshDeepyWebTailscale() {
  const statusEl = $("deepyWebTailscaleStatus");
  const row = $("deepyWebTailscaleRow");
  const urlEl = $("deepyWebTailnetOpen");
  if (!statusEl) return;
  let t = null;
  try {
    t = await window.w2gp.deepyWebTailscale();
  } catch (e) {
    statusEl.textContent = "Tailscale: check failed — " + errText(e);
    return;
  }
  if (!t || !t.ok) {
    statusEl.textContent = "Tailscale: " + ((t && t.error) || "check failed");
    if (row) row.style.display = "none";
    return;
  }
  const url = t.tailnetUrl || t.tailnet_url || null;
  _deepyWeb.tailnetUrl = url;
  if (url) {
    statusEl.textContent =
      "● Tailscale on — off-LAN URL below (works from anywhere).";
    if (row) row.style.display = "";
    if (urlEl) {
      urlEl.textContent = url;
      urlEl.disabled = false;
    }
  } else {
    statusEl.textContent =
      "○ " +
      (t.guidance || "Tailscale not active — see the setup guide below.");
    if (row) row.style.display = "none";
  }
}
async function deepyWebStartFlow() {
  const statusEl = $("deepyWebStatus");
  const startBtn = $("deepyWebStartBtn");
  const mode = deepyWebMode();
  const port = deepyWebPortArg();
  const authMode = deepyWebAuthMode();
  let authFixed = deepyWebAuthFixed();
  const rememberPw = ($("deepyWebAuthRemember") || {}).checked === true;
  if (authMode === "fixed" && !authFixed) {
    try {
      const cfg0 = await window.w2gp.configLoad().catch(() => ({}));
      const saved0 = deepyWebSavedSecret(cfg0);
      if (saved0) authFixed = saved0.secret;
    } catch {}
  }
  const httpsPaths = deepyWebHttpsPaths();
  const httpsOn = deepyWebHttpsOn();
  const publicUrl = deepyWebPublicUrl();
  const assistant = deepyWebAssistant();
  if (httpsOn && (!httpsPaths.cert || !httpsPaths.key)) {
    showToast("HTTPS needs both .pem and .key — use Bring or Create first.");
    return;
  }
  if (authMode === "fixed" && !authFixed) {
    showToast(
      "Fixed auth needs a password — type one or press Generate passphrase.",
    );
    return;
  }
  try {
    const cfg = await window.w2gp.configLoad().catch(() => ({}));
    if (cfg && typeof cfg === "object") {
      cfg.deepyListen = mode === "lan";
      if (port) cfg.deepyPort = port;
      if (assistant) cfg.deepyWebAssistant = assistant;
      else delete cfg.deepyWebAssistant;
      cfg.deepyAuthMode = authMode;
      cfg.deepyCertPath = httpsPaths.cert;
      cfg.deepyKeyPath = httpsPaths.key;
      if (publicUrl) cfg.deepyPublicUrl = publicUrl;
      else delete cfg.deepyPublicUrl;
      if (authMode === "fixed" && rememberPw && authFixed) {
        cfg.deepySavedPassword = authFixed;
        cfg.deepyPasswordExpiry = Date.now() + DEEPY_SAVED_DAYS * 864e5;
      }
      await window.w2gp.configSave(cfg).catch(() => {});
      if (authMode === "fixed" && rememberPw && authFixed) {
        renderDeepyWebSavedNote(
          {
            secret: authFixed,
            until: new Date(cfg.deepyPasswordExpiry),
          },
          false,
        );
      }
    }
  } catch {}
  if (statusEl) statusEl.textContent = "Starting Deepy Web…";
  if (startBtn) startBtn.disabled = true;
  appendLog(
    "[Deepy] Starting Deepy Web (" +
      mode +
      ") on :" +
      (port || "auto") +
      " auth=" +
      authMode +
      (httpsOn ? " https=on" : "") +
      (publicUrl ? " public-url=" + publicUrl : "") +
      "…",
  );
  try {
    const pre = await window.w2gp.deepyWebPreflight(mode);
    if (!pre || !pre.ok) {
      const errs =
        (pre && pre.errors && pre.errors.join(" ")) ||
        ((pre && pre.error) ?? "preflight failed");
      if (statusEl) {
        statusEl.textContent = "";
        const e = document.createElement("span");
        e.style.color = "#F87171";
        e.textContent = "✗ " + errs;
        statusEl.append(e);
      }
      showToast("✗ Deepy Web preflight: " + errs);
      appendLog("[Deepy] ✗ preflight failed: " + errs);
      refreshDeepyWeb();
      return;
    }
    if (pre.clashNotice && statusEl) statusEl.textContent = pre.clashNotice;
    window.__deepyWebStarting = true; // light poll stands down until boot resolves
    // Assistant override (Zero/Prime radio): applied to the saved config BEFORE
    // the backend reads it, so this Deepy Web process boots the chosen level.
    // Unchecked = follow the Deepy panel config untouched. Prime preserves the
    // SAVED Prime engine (never forced to OpenCode) — see resolveDeepyWebPrimeEngine.
    if (assistant === "zero" || assistant === "prime") {
      try {
        let primeEngine = null;
        if (assistant === "prime") {
          const resolved = await resolveDeepyWebPrimeEngine();
          if (resolved.error) throw new Error(resolved.error);
          primeEngine = resolved.engine;
        }
        const ar = await window.w2gp.deepySet(
          assistant,
          primeEngine,
          null,
          null,
        );
        if (ar && ar.ok) {
          appendLog(
            "[Deepy] Assistant for this start: " +
              assistant +
              (primeEngine ? " (" + primeEngine + ")" : "") +
              " (saved). ",
          );
        } else {
          throw new Error((ar && ar.error) || "assistant apply failed");
        }
      } catch (e) {
        window.__deepyWebStarting = false;
        showToast("✗ Assistant: " + errText(e));
        appendLog("[Deepy] ✗ assistant apply failed: " + errText(e));
        refreshDeepyWeb();
        return;
      }
    }
    const r = await window.w2gp.deepyWebStart(mode, port, authMode, authFixed, {
      enabled: httpsOn,
      cert: httpsPaths.cert,
      key: httpsPaths.key,
    }, publicUrl);
    if (r && r.ok) {
      showToast("✓ Deepy Web running on :" + r.port);
      const extUrl = (r.urls && r.urls.external) || "";
      appendLog(
        "[Deepy] ✓ Deepy Web running on :" +
          r.port +
          (extUrl ? " — external " + extUrl : ""),
      );
      try {
        const pwBox = $("deepyWebAuthPassword");
        const fixedInput = $("deepyWebAuthFixed");
        const typedFixed = authMode === "fixed" ? authFixed : "";
        if (fixedInput) fixedInput.value = "";
        const once =
          (r && (r.authPassword || r.auth_password)) || typedFixed || "";
        const onceLabel =
          authMode === "fixed" ? "Auth password" : "Generated password";
        if (pwBox) {
          if (once) {
            pwBox.style.display = "";
            pwBox.textContent = "";
            const span = document.createElement("span");
            span.textContent =
              onceLabel +
              " (copy now, restart invalidates): " +
              once +
              " — QR holds URL + password in one scan.";
            const qrBtn = document.createElement("button");
            qrBtn.className = "btn btn-ghost small";
            qrBtn.textContent = "QR";
            qrBtn.title =
              "One QR with URL + password (scan once, paste each where needed)";
            qrBtn.style.marginLeft = "8px";
            const qrUrl =
              (r && r.urls && (r.urls.phone || r.urls.samePc)) ||
              (_deepyWeb.urls || {}).phone ||
              (_deepyWeb.urls || {}).samePc ||
              "";
            const qrText = qrUrl
              ? qrUrl + String.fromCharCode(10) + "Password: " + once
              : once;
            qrBtn.addEventListener("click", () => deepyWebQrUrl(qrText));
            const copyBtn = document.createElement("button");
            copyBtn.className = "btn btn-ghost small";
            copyBtn.textContent = "Copy";
            copyBtn.title = "Copy password to clipboard";
            copyBtn.style.marginLeft = "8px";
            copyBtn.addEventListener("click", () => deepyWebCopyUrl(once));
            pwBox.append(span, copyBtn, qrBtn);
            try {
              navigator.clipboard.writeText(once).catch(() => {});
            } catch {}
          } else {
            pwBox.style.display = "none";
            pwBox.textContent = "";
          }
        }
      } catch {}
    } else {
      const msg =
        ((r && r.error) || "start failed") +
        (r && r.hint ? " — " + r.hint : "");
      if (statusEl) {
        statusEl.textContent = "";
        const e = document.createElement("span");
        e.style.color = "#F87171";
        e.textContent = "✗ " + msg;
        statusEl.append(e);
      }
      showToast("✗ " + msg);
      appendLog("[Deepy] ✗ start failed: " + msg);
    }
  } catch (e) {
    if (statusEl) statusEl.textContent = "✗ " + errText(e);
    showToast("✗ " + errText(e));
    appendLog("[Deepy] ✗ start error: " + errText(e));
  }
  window.__deepyWebStarting = false;
  refreshDeepyWeb();
}
async function deepyWebStopFlow() {
  // Null port is intentional: the backend still sweeps launcher-spawned
  // strays on any port, so orphans die even when no port is known.
  const port = _deepyWeb.port || deepyWebPortArg() || null;
  try {
    const r = await window.w2gp.deepyWebStop(port);
    const killed = (r && r.killed) || [];
    if (r && (r.ok || r.stopped || killed.length)) {
      const stopMsg = killed.length
        ? `■ Deepy Web stopped (${killed.length} process${killed.length === 1 ? "" : "es"})`
        : "■ Deepy Web stopped";
      showToast(stopMsg);
      appendLog("[Deepy] " + stopMsg);
      try {
        const pwBox = $("deepyWebAuthPassword");
        if (pwBox) {
          pwBox.style.display = "none";
          pwBox.textContent = "";
        }
      } catch {}
    } else {
      showToast("✗ Stop failed: " + ((r && r.error) || "unknown"));
      appendLog("[Deepy] ✗ stop failed: " + ((r && r.error) || "unknown"));
    }
  } catch (e) {
    showToast("✗ " + errText(e));
    appendLog("[Deepy] ✗ stop error: " + errText(e));
  }
  refreshDeepyWeb();
}
// Open a Deepy Web URL in the OS default browser. Upstream rejects logins
// from embedded/iframe views (403 "Cross-origin request rejected"), and
// Chrome additionally sends `Origin: null` from the login page — so the
// login step always happens in a real browser tab. Launcher files only;
// Wan2GP originals are never touched.
function deepyWebOpenUrl(url) {
  if (!url) {
    showToast("No URL yet — start Deepy Web first.");
    return;
  }
  window.w2gp.openExternal(url).catch((e) => showToast("✗ " + errText(e)));
}
function deepyWebOpen(which) {
  const urls = _deepyWeb.urls || {};
  deepyWebOpenUrl(
    which === "phone"
      ? urls.phone
      : which === "external"
        ? urls.external
        : which === "tailnet"
          ? _deepyWeb.tailnetUrl
          : urls.samePc,
  );
}
function deepyWebCopyUrl(text) {
  if (!text) {
    showToast("No URL to copy yet — start Deepy Web first.");
    return;
  }
  navigator.clipboard
    .writeText(text)
    .then(() => showToast("✓ Copied " + text))
    .catch((e) => showToast("✗ Copy failed: " + errText(e)));
}
function deepyWebCopy(which) {
  const urls = _deepyWeb.urls || {};
  deepyWebCopyUrl(
    which === "phone"
      ? urls.phone
      : which === "external"
        ? urls.external
        : which === "tailnet"
          ? _deepyWeb.tailnetUrl
          : urls.samePc,
  );
}
function deepyWebQr(which) {
  const urls = _deepyWeb.urls || {};
  const captions = {
    phone:
      "Scan from your phone camera (same Wi-Fi) — opens Deepy Web in your phone browser",
    external:
      "Scan from anywhere — works off-LAN over Tailscale/VPN in your phone browser",
    tailnet:
      "Scan from anywhere — works off-LAN over Tailscale in your phone browser",
    "same-pc": "Same-PC URL — open in this PC's browser",
  };
  deepyWebQrUrl(
    which === "phone"
      ? urls.phone
      : which === "external"
        ? urls.external
        : which === "tailnet"
          ? _deepyWeb.tailnetUrl
          : urls.samePc,
    captions[which] || undefined,
  );
}
function deepyWebQrUrl(text, caption) {
  if (!text) {
    showToast("No URL to encode yet — start Deepy Web first.");
    return;
  }
  const canvas = $("deepyWebQrCanvas");
  const cap = $("deepyWebQrCaption");
  const modal = $("deepyWebQrModal");
  if (!canvas || !modal) return;
  try {
    const qr = qrcodegen.QrCode.encodeText(text, qrcodegen.QrCode.Ecc.MEDIUM);
    const border = 4;
    const n = qr.size + border * 2;
    const px = Math.max(1, Math.floor(232 / n));
    canvas.width = n * px;
    canvas.height = n * px;
    const ctx = canvas.getContext("2d");
    ctx.fillStyle = "#ffffff";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.fillStyle = "#000000";
    for (let y = 0; y < qr.size; y++)
      for (let x = 0; x < qr.size; x++)
        if (qr.getModule(x, y))
          ctx.fillRect((x + border) * px, (y + border) * px, px, px);
    if (cap) cap.textContent = caption || text;
    modal.classList.remove("hidden");
  } catch (e) {
    showToast("✗ QR failed: " + errText(e));
  }
}
function renderDeepyWebIpList(urls) {
  const box = $("deepyWebIpList");
  if (!box) return;
  box.textContent = "";
  const rows = (urls && urls.lanIps) || [];
  // The External row already shows the Tailscale URL — listing it again
  // here duplicates it, so skip tailscale rows while External is shown.
  const hideTailscale = !!(urls && urls.external);
  for (const r of rows) {
    if (!r || !r.url) continue;
    const isTs = r.kind === "tailscale";
    if (isTs && hideTailscale) continue;
    const row = document.createElement("div");
    row.className = "deepyweb-url-row";
    const label = document.createElement("span");
    label.className = "deepy-sub-label";
    // Short kind label only (the URL itself carries the IP) so the shared
    // grid column stays aligned across all address rows.
    label.textContent = isTs ? "Tailscale" : "LAN";
    label.title = r.ip || "";
    const open = document.createElement("button");
    open.className = "deepyweb-url deepyweb-open";
    open.textContent = r.url;
    open.title = isTs
      ? "Works off-LAN over Tailscale/VPN — open from anywhere"
      : "Same Wi-Fi only — scan from your phone camera";
    open.addEventListener("click", () => deepyWebOpenUrl(r.url));
    const copy = document.createElement("button");
    copy.className = "btn btn-ghost small";
    copy.textContent = "Copy";
    copy.title = "Copy " + r.url;
    copy.addEventListener("click", () => deepyWebCopyUrl(r.url));
    const qr = document.createElement("button");
    qr.className = "btn btn-ghost small";
    qr.textContent = "QR";
    qr.title = isTs
      ? "Show " + r.url + " as QR — scan from anywhere"
      : "Show " + r.url + " as QR — scan from your phone (same Wi-Fi)";
    qr.addEventListener("click", () =>
      deepyWebQrUrl(
        r.url,
        isTs
          ? "Scan from anywhere — works off-LAN over Tailscale in your phone browser"
          : "Scan from your phone camera (same Wi-Fi) — opens Deepy Web in your phone browser",
      ),
    );
    row.append(label, open, copy, qr);
    box.append(row);
  }
}
$("deepyWebSamePcOpen")?.addEventListener("click", () =>
  deepyWebOpen("same-pc"),
);
$("deepyWebPhoneOpen")?.addEventListener("click", () => deepyWebOpen("phone"));
$("deepyWebExternalOpen")?.addEventListener("click", () =>
  deepyWebOpen("external"),
);
$("deepyWebTailnetOpen")?.addEventListener("click", () =>
  deepyWebOpen("tailnet"),
);
document.querySelectorAll("input[name=deepyWebAssistant]").forEach((r) =>
  r.addEventListener("change", () => {
    deepyWebAssistantHintSync();
    // Persist the per-start choice immediately (applied via deepy_set
    // at Start time too) so a reload keeps the selection.
    window.w2gp
      .configLoad()
      .then((cfg) => {
        if (cfg && typeof cfg === "object") {
          const a = deepyWebAssistant();
          if (a) cfg.deepyWebAssistant = a;
          else delete cfg.deepyWebAssistant;
          return window.w2gp.configSave(cfg);
        }
      })
      .catch(() => {});
  }),
);
$("deepyWebStartBtn")?.addEventListener("click", deepyWebStartFlow);
$("deepyWebStopBtn")?.addEventListener("click", deepyWebStopFlow);
$("stopDeepyBtn")?.addEventListener("click", deepyWebStopFlow);
$("deepyWebOutputsBtn")?.addEventListener("click", async () => {
  try {
    const r = await window.w2gp.deepyWebOpenOutputs().catch(() => null);
    if (r && r.ok) showToast("✓ Outputs folder: " + (r.path || ""));
    else showToast("✗ Outputs folder: " + ((r && r.error) || "failed"));
  } catch (e) {
    showToast("✗ " + errText(e));
  }
});
$("deepyWebSamePcCopy")?.addEventListener("click", () =>
  deepyWebCopy("same-pc"),
);
$("deepyWebPhoneCopy")?.addEventListener("click", () => deepyWebCopy("phone"));
$("deepyWebExternalCopy")?.addEventListener("click", () =>
  deepyWebCopy("external"),
);
$("deepyWebExternalQr")?.addEventListener("click", () =>
  deepyWebQr("external"),
);
$("deepyWebSamePcQr")?.addEventListener("click", () => deepyWebQr("same-pc"));
$("deepyWebPhoneQr")?.addEventListener("click", () => deepyWebQr("phone"));
function deepyWebGenPassphrase(len) {
  const n = Math.min(64, Math.max(4, parseInt(len, 10) || 16));
  const chars = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
  const rnd = new Uint32Array(n);
  crypto.getRandomValues(rnd);
  let out = "";
  for (let i = 0; i < n; i++) out += chars[rnd[i] % chars.length];
  return out;
}
$("deepyWebAuthGen")?.addEventListener("click", () => {
  const inp = $("deepyWebAuthFixed");
  if (!inp) return;
  inp.value = deepyWebGenPassphrase(($("deepyWebAuthLen") || {}).value);
  const fixed = document.querySelector(
    'input[name=deepyWebAuth][value="fixed"]',
  );
  if (fixed) fixed.checked = true;
  showToast(
    "Passphrase generated — it is never stored. Copy it or press Show to read it.",
  );
});
$("deepyWebAuthShow")?.addEventListener("click", () => {
  const inp = $("deepyWebAuthFixed");
  const btn = $("deepyWebAuthShow");
  if (!inp) return;
  const show = inp.type === "password";
  inp.type = show ? "text" : "password";
  if (btn) btn.textContent = show ? "Hide" : "Show";
});
$("deepyWebAuthForget")?.addEventListener("click", async () => {
  try {
    const cfg = await window.w2gp.configLoad().catch(() => ({}));
    if (cfg && typeof cfg === "object") {
      delete cfg.deepySavedPassword;
      delete cfg.deepyPasswordExpiry;
      await window.w2gp.configSave(cfg).catch(() => {});
    }
  } catch (e) {
    showToast("✗ " + errText(e));
  }
  renderDeepyWebSavedNote(null, false);
  showToast("Saved Deepy Web password forgotten.");
});
$("deepyWebTailscaleLink")?.addEventListener("click", async (ev) => {
  ev.preventDefault();
  await window.w2gp.openExternal("https://tailscale.com/download");
});
$("deepyWebQrClose")?.addEventListener("click", () => {
  $("deepyWebQrModal")?.classList.add("hidden");
});
document
  .querySelectorAll("input[name=deepyWebMode]")
  .forEach((r) => r.addEventListener("change", refreshDeepyWeb));
$("deepyWebPortSave")?.addEventListener("click", async () => {
  const port = deepyWebPortArg();
  if (!port) {
    showToast("Port must be 1–65535 (or empty for auto).");
    return;
  }
  try {
    const cfg = await window.w2gp.configLoad().catch(() => ({}));
    if (cfg && typeof cfg === "object") {
      cfg.deepyPort = port;
      await window.w2gp.configSave(cfg);
    }
    showToast("Deepy Web port set to " + port);
  } catch (e) {
    showToast("✗ " + errText(e));
  }
  refreshDeepyWeb();
});
$("deepyWebCertBring")?.addEventListener("click", () =>
  deepyWebCertFlow("bring"),
);
$("deepyWebCertCreate")?.addEventListener("click", () =>
  deepyWebCertFlow("create"),
);
document
  .querySelectorAll("input[name=deepyWebHttps]")
  .forEach((r) => r.addEventListener("change", refreshDeepyWeb));
$("deepyWebTailnetCopy")?.addEventListener("click", () =>
  deepyWebCopy("tailnet"),
);
$("deepyWebTailnetQr")?.addEventListener("click", () => deepyWebQr("tailnet"));
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

// ── Floating Terminal events ──
$("ftToggleBtn")?.addEventListener("click", toggleFloatingTerm);
$("ftCloseBtn")?.addEventListener("click", closeFloatingTerm);
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

// ── Performance Settings (unified: Detect seeds dropdowns + rec tags; user overrides; saved tags from disk) ──
function memProfileCollect() {
  // Only include fields the user actually set (non-empty) — unset = leave existing config.
  const s = {};
  const vp = $("memVideoProfile").value;
  const ip = $("memImageProfile").value;
  const ap = $("memAudioProfile").value;
  const co = $("memCoeff").value;
  const ve = $("memVae").value;
  const q = $("memQuant").value;
  const i8k = $("memInt8Kernels") ? $("memInt8Kernels").value : "";
  const kp = $("memKernelPrecision") ? $("memKernelPrecision").value : "";
  if (i8k) s.int8_kernels = i8k;
  if (kp) s.kernel_precision = kp;
  if (vp) s.video_profile = Number(vp);
  if (ip) s.image_profile = Number(ip);
  if (ap) s.audio_profile = Number(ap);
  if (co) {
    const n = Number(co);
    if (!(n > 0 && n <= 1)) {
      setMemStatus("VRAM Safety Coeff must be between 0.1 and 1", true);
      return null;
    }
    s.vram_safety_coefficient = n;
  }
  if (ve !== "") s.vae_config = Number(ve);
  if (q) s.transformer_quantization = q;
  return s;
}

function setMemStatus(msg, isError) {
  const el = $("memProfileStatus");
  if (!el) return;
  el.textContent = msg || "";
  el.style.color = isError ? "var(--signal-red)" : "var(--text-secondary)";
}

// Field metadata: maps the dropdown id to its rec/saved tag ids and a formatter.
const MEM_FIELDS = {
  video_profile: {
    sel: "memVideoProfile",
    rec: "recVideoProfile",
    saved: "savedVideoProfile",
  },
  image_profile: {
    sel: "memImageProfile",
    rec: "recImageProfile",
    saved: "savedImageProfile",
  },
  audio_profile: {
    sel: "memAudioProfile",
    rec: "recAudioProfile",
    saved: "savedAudioProfile",
  },
  vram_safety_coefficient: {
    sel: "memCoeff",
    rec: "recCoeff",
    saved: "savedCoeff",
  },
  vae_config: { sel: "memVae", rec: "recVae", saved: "savedVae" },
  transformer_quantization: {
    sel: "memQuant",
    rec: "recQuant",
    saved: "savedQuant",
  },
  int8_kernels: {
    sel: "memInt8Kernels",
    rec: "recInt8Kernels",
    saved: "savedInt8Kernels",
  },
  kernel_precision: {
    sel: "memKernelPrecision",
    rec: "recKernelPrecision",
    saved: "savedKernelPrecision",
  },
};
const INT8_KERNEL_LABELS = {
  auto: "Auto (default)",
  kitchen: "Comfy Kitchen",
  triton: "Triton",
  disabled: "Disabled (PyTorch)",
};
const KERNEL_PRECISION_LABELS = {
  fast: "Approximate (default)",
  strict: "Preserve precision",
};
function fmtVal(key, v) {
  if (v == null || v === "") return "—";
  if (key === "vae_config") return v + (Number(v) === 0 ? " (AUTO)" : "");
  if (key === "int8_kernels")
    return INT8_KERNEL_LABELS[v] || String(v);
  if (key === "kernel_precision")
    return KERNEL_PRECISION_LABELS[v] || String(v);
  return String(v);
}

function memProfilePopulate(settings, opts = {}) {
  if (!settings) return;
  // opts.mode: 'recommend' fills the dropdown + rec tags; 'saved' fills rec tags from detect AND saved tags from disk.
  for (const key of Object.keys(MEM_FIELDS)) {
    const f = MEM_FIELDS[key];
    const v = settings[key];
    if (opts.mode === "recommend") {
      // Seed the dropdown with the recommended value (user can override).
      const sel = $(f.sel);
      if (sel)
        sel.value =
          v != null && v !== "" ? String(v) : key === "vae_config" ? "0" : "";
      const rec = $(f.rec);
      if (rec) rec.textContent = "rec: " + fmtVal(key, v);
    } else if (opts.mode === "saved") {
      // Show what's currently written to disk (preferred/saved).
      const saved = $(f.saved);
      if (saved) saved.textContent = "saved: " + fmtVal(key, v);
    }
  }
}

// Feed the manual Adjuster from an Auto-Tune detection result: set the dropdown
// defaults to the recommended values AND show the rec tags. The user can then
// override any dropdown before pressing Apply.
function memProfileFromRecommendation(rec) {
  if (!rec) return;
  memProfilePopulate(
    {
      video_profile: rec.video_profile,
      image_profile: rec.image_profile,
      audio_profile: rec.audio_profile,
      vram_safety_coefficient: rec.vram_safety_coefficient,
      vae_config: rec.vae_config == null ? 0 : rec.vae_config, // AUTO unless Detect set a fixed value
      transformer_quantization: rec.transformer_quantization,
      // Upstream v13.13 string enums (legacy numeric enable_int8_kernels
      // mapped defensively — the backend no longer sends it).
      int8_kernels:
        rec.int8_kernels ||
        (rec.enable_int8_kernels === 0 ? "disabled" : "auto"),
      kernel_precision: rec.kernel_precision || "fast",
    },
    { mode: "recommend" },
  );
}

async function memProfileLoad() {
  try {
    const res = await window.w2gp.memoryProfileRead();
    if (res && res.ok) {
      // Show the currently-saved (preferred) values from disk.
      memProfilePopulate(res.settings, { mode: "saved" });
      // If a detection already populated the dropdowns, leave them; otherwise
      // seed the dropdowns from the saved config too so the panel isn't empty.
      const first = $("memVideoProfile");
      if (first && first.value === "")
        memProfilePopulate(res.settings, { mode: "recommend" });
    } else
      setMemStatus(
        (res && res.error) || "Failed to read memory settings",
        true,
      );
  } catch (e) {
    setMemStatus(e.message, true);
  }
}

$("memProfileApplyBtn")?.addEventListener("click", async () => {
  const btn = $("memProfileApplyBtn");
  const s = memProfileCollect();
  if (!s) return;
  if (Object.keys(s).length === 0) {
    setMemStatus("Set at least one field before applying.", true);
    return;
  }
  btn.disabled = true;
  btn.textContent = "Applying…";
  setMemStatus("");
  try {
    const res = await window.w2gp.memoryProfileApply(s);
    if (res && res.ok)
      setMemStatus(
        "✓ Applied: " +
          res.applied.join(", ") +
          " — restart Wan2GP to take effect.",
        false,
      );
    else setMemStatus("✗ " + ((res && res.error) || "apply failed"), true);
  } catch (e) {
    setMemStatus("✗ " + e.message, true);
  } finally {
    btn.disabled = false;
    btn.textContent = "Apply Overrides";
  }
});

// (memProfileLoad is called from switchSettingsTab — every entry path.)

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

// ── Xet Storage (hf_xet) ──
// Minimal PEP 440 subset for dotted numeric releases (1.6.0 vs >=1.5.2).
// True when the requirement is empty/unparseable (display only, no verdict).
function versionSatisfies(installed, required) {
  if (!installed || !required) return true;
  const m = String(required).match(
    /^(==|>=|<=|~=|!=|>|<)\s*([0-9][0-9A-Za-z.\-_]*)/,
  );
  if (!m) return true;
  const num = (v) =>
    String(v).split(".").map((p) => {
      const n = parseInt(p, 10);
      return Number.isFinite(n) ? n : 0;
    });
  const a = num(installed);
  const b = num(m[2]);
  let cmp = 0;
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const x = a[i] || 0;
    const y = b[i] || 0;
    if (x !== y) {
      cmp = x < y ? -1 : 1;
      break;
    }
  }
  switch (m[1]) {
    case "==":
      return cmp === 0;
    case "!=":
      return cmp !== 0;
    case ">=":
    case "~=":
      return cmp >= 0;
    case "<=":
      return cmp <= 0;
    case ">":
      return cmp > 0;
    case "<":
      return cmp < 0;
    default:
      return true;
  }
}
async function updateXetStatus() {
  const btn = $("xetInstallBtn");
  const status = $("xetStatus");
  if (!btn || !status) return;
  try {
    const r = await window.w2gp.checkPackage("hf_xet");
    const ver = (r && r.version) || "";
    const req = (r && r.required) || "";
    const reqNote = req ? " (requires " + req + ")" : "";
    if (r && r.installed) {
      if (versionSatisfies(ver, req)) {
        status.textContent =
          "installed" + (ver ? " " + ver : "") + reqNote;
        status.style.color = "var(--signal-green)";
        btn.textContent = "Uninstall hf_xet";
      } else {
        status.textContent =
          "installed " + ver + " — outdated" + reqNote;
        status.style.color = "var(--signal-red)";
        btn.textContent = "Update hf_xet";
      }
    } else {
      status.textContent = "not installed" + reqNote;
      status.style.color = "var(--text-tertiary)";
      btn.textContent = "Install hf_xet";
    }
  } catch {
    status.textContent = "error checking";
    status.style.color = "var(--signal-red)";
  }
}

$("xetInstallBtn")?.addEventListener("click", async function () {
  this.disabled = true;
  const status = $("xetStatus");
  if (status) status.textContent = "working...";
  try {
    let r;
    if (this.textContent.startsWith("Uninstall")) {
      r = await window.w2gp.uninstallPackage("hf_xet");
    } else {
      // Install/Update to the live upstream pin from requirements.txt
      // (e.g. hf_xet>=1.5.2) — never a hardcoded floor.
      let spec = "hf_xet";
      try {
        const c = await window.w2gp.checkPackage("hf_xet");
        if (c && c.required) spec = "hf_xet" + c.required;
      } catch {}
      r = await window.w2gp.installPackage(spec);
    }
    if (r && r.success) {
      updateXetStatus();
      showToast(
        r.success
          ? "hf_xet " +
              (this.textContent.startsWith("Uninstall")
                ? "uninstalled"
                : "installed")
          : "Failed",
      );
    } else {
      if (status) {
        status.textContent = "failed";
        status.style.color = "var(--signal-red)";
      }
      showToast("✗ " + (r && r.error ? r.error : "Failed"));
    }
  } catch (e) {
    if (status) {
      status.textContent = "error";
      status.style.color = "var(--signal-red)";
    }
    showToast("✗ " + e.message);
  } finally {
    this.disabled = false;
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

// ── Uninstall Wan2GP (Manage → General → danger section) ──
// Uninstall via explicit 3-choice modal (the old native OK/Cancel confused:
// Cancel sounded like abort but meant "delete everything").
async function openUninstallModal() {
  const modal = $("uninstallModal");
  if (!modal) return null;
  const rows = $("uninstallModelsRows");
  rows.innerHTML = '<span class="istack-hint">checking model folders…</span>';
  modal.classList.remove("hidden");
  // Fill model folders + sizes so the choice is informed.
  try {
    const mp = await window.w2gp.getModelPaths().catch(() => null);
    const items = [
      ["Checkpoints", mp && mp.checkpoints],
      ["LoRAs", mp && mp.loras],
      ["Output", mp && mp.output],
    ].filter(([, p]) => p && p !== ".");
    if (items.length) {
      rows.innerHTML = "";
      for (const [label, p] of items) {
        let sizeTxt = "";
        try {
          const sz = await window.w2gp.folderSize(p).catch(() => null);
          if (sz && sz.bytes != null) sizeTxt = " (" + fmtBytes(sz.bytes) + ")";
        } catch {}
        const div = document.createElement("div");
        div.className = "istack-row";
        const k = document.createElement("span");
        k.className = "istack-k";
        k.textContent = label;
        const v = document.createElement("span");
        v.className = "istack-v";
        v.textContent = p + sizeTxt;
        div.append(k, v);
        rows.appendChild(div);
      }
    } else {
      rows.innerHTML =
        '<span class="istack-hint">No separate model folders configured.</span>';
    }
  } catch {}
  return new Promise((resolve) => {
    const done = (v) => {
      modal.classList.add("hidden");
      resolve(v);
    };
    const agreeRow = $("uninstallAgreeRow"),
      agreeInput = $("uninstallAgreeInput"),
      delBtn = $("uninstallDeleteBtn");
    // Reset the AGREE gate on every open.
    if (agreeRow) agreeRow.style.display = "none";
    if (agreeInput) agreeInput.value = "";
    if (delBtn) {
      delBtn.disabled = false;
      delBtn.textContent = "Delete everything";
    }
    $("uninstallCloseBtn").onclick = () => done(null);
    $("uninstallCancelBtn").onclick = () => done(null);
    $("uninstallKeepBtn").onclick = () => done({ keepModels: true });
    delBtn.onclick = () => {
      // Two-step: first click reveals the gate, second (with AGREE) deletes.
      if (agreeRow && agreeRow.style.display === "none") {
        agreeRow.style.display = "";
        delBtn.disabled = true;
        delBtn.textContent = "Type AGREE above";
        agreeInput?.focus();
        return;
      }
      if ((agreeInput?.value || "").trim() === "AGREE")
        done({ keepModels: false });
    };
    if (agreeInput)
      agreeInput.oninput = () => {
        const ok = agreeInput.value.trim() === "AGREE";
        delBtn.disabled = !ok;
        delBtn.textContent = ok ? "Confirm delete" : "Type AGREE above";
      };
  });
}

$("uninstallBtn")?.addEventListener("click", async function () {
  const choice = await openUninstallModal().catch(() => null);
  if (!choice) {
    appendLog("[*] Uninstall cancelled.");
    return;
  }
  this.disabled = true;
  this.textContent = "Uninstalling...";
  appendLog(
    "[*] Uninstalling Wan2GP" +
      (choice.keepModels ? " (keeping models)…" : " (deleting everything)…"),
  );
  try {
    const r = await window.w2gp.uninstall(choice);
    if (r && r.cancelled) {
      appendLog("[*] Uninstall cancelled.");
    } else if (r && r.success) {
      appendLog("[✓] Wan2GP uninstalled.");
      if (r.keptFiles && r.keptPaths && r.keptPaths.length) {
        appendLog("[i] Kept your files (checkpoints, LoRAs, output):");
        r.keptPaths.forEach((p) => appendLog("[i]   " + p));
        appendLog("[i] Reinstalling will reuse them automatically.");
      }
      if (r.leftoverFolder) {
        appendLog(
          "[i] The empty folder could not be deleted (locked by a process open in it):",
        );
        appendLog("[i]   " + r.leftoverFolder);
        appendLog(
          "[i] Close any terminal/Explorer window open in it and delete it manually.",
        );
      }
      showToast(
        "✓ Wan2GP uninstalled" +
          (r.keptFiles ? " (files kept)" : "") +
          (r.leftoverFolder ? " (empty folder left)" : ""),
      );
      setLaunchButtonsInstalled(false);
      // Nothing installed → back to the installer, not the dashboard.
      await openInstallerFresh("Wan2GP removed — install fresh below.");
    } else {
      appendLog("[!] Uninstall failed: " + ((r && r.error) || "unknown"));
      showToast("✗ " + ((r && r.error) || "Uninstall failed"));
    }
  } catch (e) {
    appendLog("[!] Uninstall error: " + errText(e));
    showToast("✗ " + errText(e));
  } finally {
    this.disabled = false;
    this.textContent = "Uninstall Wan2GP…";
  }
});

// ── 🛟 Troubleshooting (P0 — upstream TROUBLESHOOTING.md) ──
function tsStatus(id, text) {
  const el = $(id);
  if (el) el.textContent = text;
}
async function tsRefreshLaunchArgs() {
  try {
    const cfg = await window.w2gp.configLoad();
    if ($("launchArgsInput")) $("launchArgsInput").value = cfg.launchArgs || "";
    if ($("portInput")) $("portInput").value = cfg.serverPort || 7860;
  } catch {}
}
$("tsUpstreamDocsLink")?.addEventListener("click", async (ev) => {
  ev.preventDefault();
  await window.w2gp.openExternal(
    "https://github.com/deepbeepmeep/Wan2GP/blob/main/docs/TROUBLESHOOTING.md",
  );
});
$("tsInstallDocsLink")?.addEventListener("click", async (ev) => {
  ev.preventDefault();
  await window.w2gp.openExternal(
    "https://github.com/deepbeepmeep/Wan2GP/blob/main/docs/INSTALLATION.md",
  );
});
$("tsFailsafeBtn")?.addEventListener("click", async function () {
  this.disabled = true;
  tsStatus("tsFailsafeStatus", "Applying…");
  try {
    const r = await window.w2gp.tsFailsafeApply();
    appendLog(
      "[✓] Failsafe applied: " +
        (r.launchArgs || "") +
        (r.backup
          ? " (backup: " + r.backup + ")"
          : " (no wgp_config.json yet)"),
    );
    tsStatus("tsFailsafeStatus", "✓ Failsafe applied — relaunch Wan2GP.");
    showToast("✓ Failsafe applied — relaunch Wan2GP");
    tsRefreshLaunchArgs();
  } catch (e) {
    tsStatus("tsFailsafeStatus", "✗ " + errText(e));
    showToast("✗ " + errText(e));
  }
  this.disabled = false;
});
$("tsCudaBtn")?.addEventListener("click", async function () {
  this.disabled = true;
  tsStatus("tsFailsafeStatus", "Probing torch…");
  try {
    const r = await window.w2gp.tsCudaCheck();
    if (r && r.ok) {
      const msg =
        "torch " +
        r.torch +
        " + CUDA " +
        (r.cuda || "?") +
        " — cuda_available=" +
        r.available +
        " (" +
        (r.devices || 0) +
        " device(s)" +
        (r.name ? ": " + r.name : "") +
        ")";
      appendLog("[✓] CUDA check: " + msg);
      tsStatus("tsFailsafeStatus", "✓ " + msg);
    } else {
      tsStatus("tsFailsafeStatus", "✗ " + ((r && r.error) || "probe failed"));
      appendLog(
        "[!] CUDA check failed: " + ((r && (r.stderr || r.error)) || "unknown"),
      );
    }
  } catch (e) {
    tsStatus("tsFailsafeStatus", "✗ " + errText(e));
  }
  this.disabled = false;
});
$("tsComputeBtn")?.addEventListener("click", async function () {
  this.disabled = true;
  tsStatus(
    "tsFailsafeStatus",
    "Running GPU compute (import + kernels, ~1 min on cold HIP)…",
  );
  try {
    const r = await window.w2gp.tsGpuCompute();
    const kernelLines = (r) => {
      const k = (r && r.kernels) || {};
      return Object.keys(k)
        .filter((d) => d !== "sage2_symbol" && d !== "quanto_qbytes_mm")
        .map((d) => {
          const e = k[d] || {};
          const st = e.import || "?";
          return (
            (st === "ok" || st === "missing" ? "✓ " : "✗ ") +
            d +
            " " +
            (e.version || "") +
            (st !== "ok" && st !== "missing" ? " — " + st : "")
          );
        });
    };
    if (r && r.ok) {
      const msg =
        "GPU compute OK: torch " +
        (r.torch || "?") +
        " on " +
        (r.device || "?") +
        " (mode " +
        (r.mode || "?") +
        (r.recorded ? ", recorded for launch" : "") +
        ")";
      appendLog("[✓] " + msg);
      kernelLines(r).forEach((l) => appendLog("    " + l));
      if (r.kernel_warning) appendLog("[i] " + r.kernel_warning);
      tsStatus("tsFailsafeStatus", "✓ " + msg);
      showToast("✓ GPU compute passed");
    } else {
      const det = r && r.detail ? " " + JSON.stringify(r.detail) : "";
      tsStatus("tsFailsafeStatus", "✗ " + ((r && r.error) || "probe failed"));
      const kl = kernelLines(r).filter((l) => l.startsWith("✗"));
      appendLog(
        "[!] GPU compute failed: " + ((r && r.error) || "unknown") + det,
      );
      kl.forEach((l) => appendLog("    " + l));
    }
  } catch (e) {
    tsStatus("tsFailsafeStatus", "✗ " + errText(e));
  }
  this.disabled = false;
});
$("tsPortCheckBtn")?.addEventListener("click", async () => {
  tsStatus("tsPortStatus", "Checking…");
  try {
    const r = await window.w2gp.tsPortStatus();
    if (!r.inUse) tsStatus("tsPortStatus", "✓ Port " + r.port + " is free.");
    else if (r.owner && r.owner.pid)
      tsStatus(
        "tsPortStatus",
        "⚠ Port " +
          r.port +
          " busy — " +
          (r.owner.name || "unknown") +
          " (pid " +
          r.owner.pid +
          ")" +
          (r.owner.ours ? " — looks like Wan2GP" : ""),
      );
    else
      tsStatus("tsPortStatus", "⚠ Port " + r.port + " busy — owner unknown.");
  } catch (e) {
    tsStatus("tsPortStatus", "✗ " + errText(e));
  }
});
$("tsPortKillBtn")?.addEventListener("click", async function () {
  const choice = await window.w2gp.confirmDialog({
    title: "Kill port owner?",
    message:
      "Kill the Python process listening on the server port? Only Python owners are touched — anything else is refused.",
  });
  if (choice !== "ok" && choice !== 0) return;
  this.disabled = true;
  try {
    const r = await window.w2gp.tsPortFix("kill");
    tsStatus(
      "tsPortStatus",
      r.freed
        ? "✓ Port " + r.port + " freed (pid " + r.pid + ")."
        : "⚠ Kill sent but port " +
            r.port +
            " still busy — use next free port.",
    );
    appendLog("[*] Port fix (kill): " + JSON.stringify(r));
  } catch (e) {
    tsStatus("tsPortStatus", "✗ " + errText(e));
    showToast("✗ " + errText(e));
  }
  this.disabled = false;
});
$("tsPortBumpBtn")?.addEventListener("click", async function () {
  this.disabled = true;
  try {
    const r = await window.w2gp.tsPortFix("bump");
    tsStatus(
      "tsPortStatus",
      "✓ Moved " + r.from + " → " + r.port + " — relaunch Wan2GP.",
    );
    showToast("Server port set to " + r.port);
    appendLog("[✓] Port bumped " + r.from + " → " + r.port);
    tsRefreshLaunchArgs();
  } catch (e) {
    tsStatus("tsPortStatus", "✗ " + errText(e));
    showToast("✗ " + errText(e));
  }
  this.disabled = false;
});
$("tsLongPathsBtn")?.addEventListener("click", async function () {
  this.disabled = true;
  try {
    const st = await window.w2gp.tsLongPathsStatus();
    if (st && st.enabled) {
      tsStatus("tsLongPathsStatus", "Already enabled.");
      appendLog("[*] Windows long paths already enabled.");
      return;
    }
    const choice = await window.w2gp.confirmDialog({
      title: "Enable Windows long paths?",
      message:
        "Sets HKLM...LongPathsEnabled=1. Needs admin approval (UAC prompt) and a reboot afterwards. Proceed?",
    });
    if (choice !== "ok" && choice !== 0) {
      tsStatus("tsLongPathsStatus", "Cancelled.");
      return;
    }
    tsStatus("tsLongPathsStatus", "Enabling...");
    const r = await window.w2gp.tsLongPathsEnable();
    if (r && r.already) {
      tsStatus("tsLongPathsStatus", "Already enabled.");
      appendLog("[*] Windows long paths already enabled.");
    } else {
      tsStatus("tsLongPathsStatus", "Enabled - reboot Windows to apply.");
      showToast("Long paths enabled - reboot Windows to apply");
      appendLog(
        "[+] Windows long paths enabled" +
          (r && r.elevated ? " (elevated)" : "") +
          " - reboot Windows to apply.",
      );
      console.log("[ts] long paths enabled", r);
    }
  } catch (e) {
    tsStatus("tsLongPathsStatus", "Error: " + errText(e));
    showToast("Error: " + errText(e));
  } finally {
    this.disabled = false;
  }
});
$("tsDebugCopyBtn")?.addEventListener("click", async function () {
  this.disabled = true;
  tsStatus("tsDebugStatus", "Gathering…");
  try {
    const r = await window.w2gp.tsDebugBundle();
    const md = (r && r.markdown) || "";
    try {
      await navigator.clipboard.writeText(md);
    } catch {
      const ta = document.createElement("textarea");
      ta.value = md;
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      ta.remove();
    }
    tsStatus("tsDebugStatus", "✓ Copied — paste into Discord / GitHub.");
    showToast("✓ Debug info copied to clipboard");
  } catch (e) {
    tsStatus("tsDebugStatus", "✗ " + errText(e));
  }
  this.disabled = false;
});
$("tsTritonTestBtn")?.addEventListener("click", async function () {
  this.disabled = true;
  tsStatus("tsTritonStatus", "Testing import…");
  try {
    const r = await window.w2gp.tsTritonTest();
    tsStatus(
      "tsTritonStatus",
      r.ok
        ? "✓ Triton " + escHtml(r.version || "?") + " importable."
        : "✗ " + escHtml(r.error || "import failed"),
    );
    if (!r.ok)
      appendLog("[!] Triton test: " + (r.stderr || r.error || "failed"));
  } catch (e) {
    tsStatus("tsTritonStatus", "✗ " + escHtml(errText(e)));
  }
  this.disabled = false;
});
async function tsTritonClear(fallback) {
  tsStatus("tsTritonStatus", "Clearing…");
  try {
    const r = await window.w2gp.tsTritonClear(fallback);
    tsStatus(
      "tsTritonStatus",
      "✓ Cache cleared" +
        (r.backup ? " (backup kept)" : " (was already empty)") +
        (fallback ? " — SDPA fallback set, relaunch." : "."),
    );
    appendLog(
      "[✓] Triton cache cleared" +
        (r.backup ? " → " + r.backup : "") +
        (fallback ? " + SDPA fallback" : ""),
    );
    if (fallback) tsRefreshLaunchArgs();
  } catch (e) {
    tsStatus("tsTritonStatus", "✗ " + escHtml(errText(e)));
    showToast("✗ " + errText(e));
  }
}
$("tsTritonClearBtn")?.addEventListener("click", () => tsTritonClear(false));
$("tsTritonSdpaBtn")?.addEventListener("click", () => tsTritonClear(true));

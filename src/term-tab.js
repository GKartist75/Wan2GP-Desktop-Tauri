// term-tab.js — Floating Terminal state/helpers (the launch handler drives these).
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

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

// Terminal show/close buttons — moved here from app.js: toggleFloatingTerm and
// closeFloatingTerm are values read at load time, so they must be wired from
// the same file that defines them.
$("ftToggleBtn")?.addEventListener("click", toggleFloatingTerm);
$("ftCloseBtn")?.addEventListener("click", closeFloatingTerm);


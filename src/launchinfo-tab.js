// launchinfo-tab.js — First-launch info bar (#launchInfo).
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

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
  try {
    refreshMainLan();
  } catch {}
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

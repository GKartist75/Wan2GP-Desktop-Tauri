// crash-tab.js — renderer-crash recovery.
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

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

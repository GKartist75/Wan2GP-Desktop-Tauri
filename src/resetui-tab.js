// resetui-tab.js — Reset UI when server exits (manual stop or crash).
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

// ── Live INT8/FP8 backend chips (topbar, beside Running) ──
// WanGP prints its selected kernel backends at boot (`[INT8] Backend: X
// (setting: auto)`). This subscriber mirrors those lines into two pills
// beside the Running LED. Visibility is value-gated: the pills exist only
// while a backend is known (server booted and reported) — no server, no
// pills, so there is never a "—" row to mistake for a forgotten setting.
// Each pill carries a status dot like the Running LED: green = an
// accelerated backend is in use, amber = WanGP fell back to plain PyTorch
// (upstream's last choice — allowed, but it should not glow like a fast
// path), grey = not reported yet.
// Latest line wins; a server exit clears both (and hides the pills).
function _backendChipDot(val) {
  const v = String(val == null ? "" : val).trim();
  if (!v) return "led-idle";
  // Both INT8 and FP8 report PyTorch as the fallback; a mixed FP8 line
  // ("Triton, PyTorch for scaled FP8 checkpoints") means one path fell back.
  return /\bpytorch\b/i.test(v) ? "led-warn" : "led-running";
}
function _paintBackendChips() {
  try {
    const pairs = [
      ["ledInt8", "ledInt8Dot", "INT8", window._int8Backend],
      ["ledFp8", "ledFp8Dot", "FP8", window._fp8Backend],
    ];
    for (const [id, dotId, label, val] of pairs) {
      const el = $(id);
      if (el) {
        el.textContent = label + " · " + (val || "…");
        el.title = val
          ? label + " backend WanGP selected at boot: " + val
          : label + " backend not reported yet";
      }
      const dot = $(dotId);
      if (dot) dot.className = ("led-dot " + _backendChipDot(val)).trim();
    }
    const box = $("backendLeds");
    if (box)
      box.style.display =
        window._int8Backend || window._fp8Backend ? "inline-flex" : "none";
  } catch {}
}
try {
  window.w2gp.onLaunchLog((t) => {
    try {
      const s = String(t == null ? "" : t);
      let changed = false;
      // One event may carry several lines; scan each for backend reports.
      // parseBackendLine is the shared service (global via services/
      // parse-backend.js); fall back to the inline pattern if it failed to
      // load so a missing script never blinds the chips.
      const parse =
        typeof parseBackendLine === "function"
          ? parseBackendLine
          : (l) => {
              const m = String(l).match(
                /\[(INT8|FP8)\]\s*Backend:\s*([^()]+?)\s*\(setting:/,
              );
              if (!m || !m[2].trim()) return null;
              return { kind: m[1], name: m[2].trim() };
            };
      for (const line of s.split("\n")) {
        const hit = parse(line);
        if (!hit) continue;
        if (hit.kind === "INT8") window._int8Backend = hit.name;
        else window._fp8Backend = hit.name;
        changed = true;
      }
      if (changed) _paintBackendChips();
    } catch {}
  });
} catch {}
function _clearBackendChips() {
  window._int8Backend = null;
  window._fp8Backend = null;
  _paintBackendChips();
}
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
  _clearBackendChips();
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
  let offeredKeyError = false;
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
        offeredKeyError = true;
        offerConfigReset(m[1], exitMode);
      }
    } catch {}
  }
  // Startup-crash triage (upstream #2517 class): a non-zero exit before the
  // server ever answered is a failure on the way up — a settings entry WanGP
  // cannot migrate, a launch flag this version dropped, a bad model file —
  // and "process exited (code 1)" names none of them. The one click that
  // separates "my settings" from "my install" is relaunching against a
  // throwaway settings folder (--config): it never touches wgp_config.json,
  // so a successful retry proves the settings are the cause and the user can
  // then hand them over for repair instead of reinstalling.
  const launchedAt = window.__wan2gpLaunchAt || 0;
  const uptime = launchedAt ? Date.now() - launchedAt : 0;
  if (
    !offeredKeyError &&
    !manualStop &&
    code !== 0 &&
    code !== "?" &&
    launchedAt > 0 &&
    uptime < 180000 &&
    !window.__wan2gpServed &&
    !window._startupCrashOffered
  ) {
    window._startupCrashOffered = true;
    let lastErr = "";
    let tail = "";
    try {
      tail = typeof window._getLogTail === "function" ? window._getLogTail() : "";
      lastErr = (tail.match(/^.*(?:Traceback|Error|Exception|FATAL)[^\n]*$/gm) || [])
        .slice(-3)
        .join("\n");
    } catch {}
    appendLog(
      `[!] Wan2GP did not start: process exited ${Math.round(uptime / 1000)}s after launch, before the server answered.`,
    );
    showToast("✗ Wan2GP did not start — retry option offered");
    const clickLaunch = () => {
      const btn =
        exitMode === "app"
          ? $("appBtn")
          : exitMode === "browser"
            ? $("browserBtn")
            : null;
      if (btn) {
        // A crashed Wan2GP exists on disk, so the launch button is only
        // disabled by a stale install probe or the prior attempt — enable it
        // or the retry click is swallowed.
        btn.disabled = false;
        btn.click();
        return true;
      }
      return false;
    };
    try {
      // One cause is identifiable without guesswork: upstream's YuE2 migration
      // rejects an `audio_prompt_type` embedded in a generated file, and a
      // temporary settings folder cannot hide it (the value is inside the
      // media). Scan and, if found, offer the real remedy instead of a retry
      // that is guaranteed to fail the same way (launcher #82).
      const yue2Signature =
        /Choose one of the five YuE2 composition sources/i.test(tail) ||
        (/yue2/i.test(tail) && /migrate_source/.test(tail));
      if (yue2Signature && typeof window.w2gpYue2Triage === "function") {
        if (
          await window.w2gpYue2Triage(() => {
            if (!clickLaunch())
              appendLog(
                "[!] Quarantined — start Wan2GP again with a launch button; the offending file is out of the gallery.",
              );
          })
        ) {
          return;
        }
        appendLog(
          "[i] YuE2 crash signature seen, but no output file carries a bad source — continuing with the generic triage.",
        );
      }
    } catch (e) {
      appendLog("[!] YuE2 triage failed: " + errText(e));
    }
    try {
      const choice = await window.w2gp.confirmDialog({
        title: "Wan2GP crashed on startup",
        message: `Wan2GP exited with code ${code} after ${Math.round(uptime / 1000)}s, before its UI came up. That is a startup failure, not a generation error — usually a settings entry Wan2GP cannot migrate, a launch flag this version dropped, or an unreadable model file.`,
        detail:
          (lastErr ? "Last error lines:\n" + lastErr + "\n\n" : "") +
          "Retry once with a temporary, empty settings folder (--config <temp>)? Your saved wgp_config.json and models are not touched: if Wan2GP starts that way, the settings are the cause and can be repaired from Manage → Troubleshooting; if it fails the same way, the cause is not wgp_config.json — it is the install, or a generated file Wan2GP reads at startup (an output with metadata it cannot migrate, which the same Troubleshooting tab scans).",
      });
      if (choice === "ok") {
        window.__safeConfigNextLaunch = true;
        appendLog(
          "[*] Retrying with a temporary settings folder (--config) — saved settings stay untouched.",
        );
        if (!clickLaunch())
          appendLog(
            "[!] Use a launch button to retry — it will start with the temporary settings folder.",
          );
      } else {
        appendLog(
          "[i] Startup triage dismissed. Manage → Troubleshooting has the debug bundle if you want to report it.",
        );
      }
    } catch (e) {
      appendLog("[!] Startup triage failed: " + errText(e));
    }
  }
});
// Startup-crash triage state: "Wan2GP ready on …" is proof the server came
// up, so a later non-zero exit is a runtime failure (never triaged as a
// startup crash) and the next launch gets a fresh offer.
try {
  window.w2gp.onLaunchLog((line) => {
    const s = typeof line === "string" ? line : "";
    if (s.indexOf("Wan2GP ready on") === -1) return;
    window.__wan2gpServed = true;
    window._startupCrashOffered = false;
    window._configCrashOffered = false;
  });
} catch {}
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

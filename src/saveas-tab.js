// saveas-tab.js — Download Save / Save-As prompt.
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

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

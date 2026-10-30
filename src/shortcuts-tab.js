// shortcuts-tab.js — Keyboard shortcuts.
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

// ── Keyboard shortcuts ──
document.addEventListener("keydown", (e) => {
  if (["INPUT", "TEXTAREA", "SELECT"].includes(e.target.tagName)) return;
  // Escape closes Manage first — it can be open in webview mode too, where the
  // webview Escape branch below would otherwise fire instead.
  if (e.key === "Escape" && $("settingsPanel").classList.contains("open")) {
    closeSettings();
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
  // Ctrl+R / F5 reloads the embedded Wan2GP page — the recovery path when the
  // Gradio view hangs (reported: video playing full screen while a queued ACE
  // audio job finishes behind it). Scoped to the open view so the launcher's
  // own screens keep the keys.
  if (
    $("dashBody").style.display === "none" &&
    ((e.ctrlKey && (e.key === "r" || e.key === "R")) || e.key === "F5")
  ) {
    e.preventDefault();
    if (typeof reloadWan2gpView === "function") reloadWan2gpView();
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

function showToast(msg, onClick, variant, ms) {
  const t = document.createElement("div");
  t.textContent = msg;
  t.setAttribute("role", "status");
  t.setAttribute("aria-live", "polite");
  // "update" is the one variant that carries an action worth clicking into, so
  // it takes the green signal language instead of the neutral grey.
  const skin =
    variant === "update"
      ? "background:#12351f;color:#e8f7ec;border:1px solid #4ade80;"
      : "background:#333;color:#e8e6e1;";
  t.style.cssText =
    "position:fixed;bottom:20px;left:50%;transform:translateX(-50%);" +
    skin +
    "padding:8px 16px;border-radius:6px;font-size:13px;z-index:9999;font-family:Geist Mono,monospace;transition:opacity 0.3s;max-width:90vw;text-align:center";
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
    setTimeout(dismiss, ms || 2500);
  }
}

$("updateCheckBtn").addEventListener("click", (e) => {
  // The tile carries the state: it downloads or installs when an update is
  // already known, and only asks GitHub when there is nothing pending.
  if (typeof runUpdateAction === "function") runUpdateAction(e);
  else window.w2gp.checkUpdate(e.shiftKey ? { local: true } : undefined);
});

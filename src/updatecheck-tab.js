// updatecheck-tab.js — Periodic Wan2GP background update check.
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

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

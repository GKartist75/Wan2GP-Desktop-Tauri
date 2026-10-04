// logviewer-tab.js — Console log viewer (own port).
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

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

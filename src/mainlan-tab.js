// mainlan-tab.js — Phone access: --listen on the MAIN server.
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

// ── Phone access: --listen on the MAIN server (Launch card) ──
// Upstream model: one Gradio process serves / plus the synchronized /deepy/
// mobile app (shared chat, galleries, active work) — the phone needs no
// second port. The toggle persists `mainLan`; launch() appends --listen
// verbatim. The bind address is fixed at startup, so flipping while the
// server runs restarts it in the same mode.
let _mainLan = {
  lanOn: false,
  serving: false,
  servingLan: false,
  port: 7860,
  samePc: "",
  phone: null,
  phoneDeepy: null,
};
let _mainLanBusy = false;
async function refreshMainLan() {
  if (!$("mainLanCard") || _mainLanBusy) return;
  _mainLanBusy = true;
  try {
    const [cfg, u] = await Promise.all([
      window.w2gp.configLoad().catch(() => ({})),
      window.w2gp.mainLanUrls().catch(() => null),
    ]);
    const lanOn = !!(cfg && cfg.mainLan);
    const port = (u && u.port) || (cfg && cfg.serverPort) || 7860;
    const samePc = (u && u.samePc) || "http://localhost:" + port;
    const phone = (u && u.phone) || null;
    const phoneDeepy = (u && u.phoneDeepy) || null;
    const serving = !!(u && u.serving);
    const servingLan = !!(u && u.servingLan);
    _mainLan = { lanOn, serving, servingLan, port, samePc, phone, phoneDeepy };
    const tgl = $("mainLanToggle");
    if (tgl) tgl.checked = lanOn;
    const statusEl = $("mainLanStatus");
    if (statusEl) {
      statusEl.textContent = "";
      const dot = document.createElement("span");
      if (serving && servingLan) {
        dot.style.color = "#4ADE80";
        dot.textContent = "● Live on LAN :" + port + " — ";
        statusEl.append(
          dot,
          "open a Phone URL or scan the QR (same Wi-Fi). Gradio and /deepy/ share the live conversation, galleries, progress and queue.",
        );
      } else if (serving && lanOn) {
        dot.style.color = "#FBBF24";
        dot.textContent = "● ON but this boot predates the toggle — ";
        statusEl.append(
          dot,
          "flip the toggle off and on again to restart onto the LAN.",
        );
      } else if (serving) {
        dot.style.color = "#4ADE80";
        dot.textContent = "● Running (this PC only) — ";
        statusEl.append(dot, "flip LAN on to expose it to your Wi-Fi.");
      } else {
        statusEl.append(
          "○ Server stopped — flip LAN on and it applies on the next launch.",
        );
      }
    }
    const setUrlBtn = (el, url, fallbackText) => {
      if (!el) return;
      const has = typeof url === "string" && url.length > 0;
      el.textContent = has ? url : fallbackText || "—";
      el.disabled = !has;
    };
    setUrlBtn($("mainLanSamePcOpen"), samePc);
    setUrlBtn($("mainLanPhoneOpen"), phone, "unavailable — no LAN adapter");
    setUrlBtn(
      $("mainLanPhoneDeepyOpen"),
      phoneDeepy,
      "unavailable — no LAN adapter",
    );
    const hint = $("mainLanHint");
    if (hint) {
      hint.textContent = phone
        ? "Gradio + /deepy/ together: start a request on one device, follow, pause, stop or inspect it on the other — accepted work continues with every browser closed, reopen to recover. If the page can't connect, allow the port through the PC firewall. Trusted home Wi-Fi is fine as-is; shared networks need a password + HTTPS first (main-server auth is a follow-up). Never port-forward plain HTTP to the internet."
        : "No LAN adapter found — connect to Wi-Fi/Ethernet to enable the Phone URLs.";
    }
  } finally {
    _mainLanBusy = false;
  }
}
function startMainLanPolling() {
  if (window.__mainLanPollTimer) clearInterval(window.__mainLanPollTimer);
  const poll = () => {
    if (document.hidden) return;
    const dash = $("dashBody");
    if (dash && dash.style.display === "none") return;
    if (!$("mainLanCard")) return;
    if (window.__mainLanRestarting) return;
    refreshMainLan().catch(() => {});
  };
  poll(); // immediate tick on (re)start
  window.__mainLanPollTimer = setInterval(poll, 15000);
}
async function mainLanRestartFlow(mode, on) {
  if (window.__mainLanRestarting) return;
  window.__mainLanRestarting = true;
  try {
    _expectServerExit = true;
    if (_expectServerExitTimer) clearTimeout(_expectServerExitTimer);
    _expectServerExitTimer = setTimeout(() => {
      _expectServerExit = false;
      _expectServerExitTimer = null;
    }, 10000);
    appendLog(
      "[*] Restarting Wan2GP (Phone access " + (on ? "ON" : "OFF") + ")…",
    );
    try {
      await window.w2gp.stopWangp();
    } catch (e) {
      showToast("✗ Stop failed: " + errText(e));
      appendLog("[!] Restart aborted — stop failed: " + errText(e));
      return;
    }
    // Wait for the port to actually release: launching while it is still
    // bound would take the "already running" reuse path on the OLD flags.
    let free = false;
    for (let i = 0; i < 30; i++) {
      try {
        const st = await window.w2gp.tsPortStatus();
        if (st && st.inUse === false) {
          free = true;
          break;
        }
      } catch {}
      await new Promise((r) => setTimeout(r, 1000));
    }
    if (!free) {
      showToast("✗ Port still busy — press Stop All, then launch manually");
      appendLog(
        "[!] Restart aborted — server port still bound after stop. Press Stop All and launch manually.",
      );
      return;
    }
    (mode === "browser" ? $("browserBtn") : $("appBtn")).click();
  } finally {
    window.__mainLanRestarting = false;
    refreshMainLan().catch(() => {});
  }
}
$("mainLanToggle")?.addEventListener("change", async (ev) => {
  const on = ev.target.checked;
  try {
    const cfg = await window.w2gp.configLoad().catch(() => ({}));
    if (cfg && typeof cfg === "object") {
      cfg.mainLan = on;
      await window.w2gp.configSave(cfg).catch(() => {});
    }
  } catch {}
  appendLog(
    "[*] Phone access " + (on ? "ON (--listen)" : "OFF") + " saved.",
  );
  if (serverMode) {
    let choice = "cancel";
    try {
      choice = await window.w2gp.confirmDialog({
        title: on
          ? "Restart Wan2GP with Phone access?"
          : "Restart Wan2GP off the LAN?",
        message: on
          ? "Wan2GP is running. Restart now so phones on your Wi-Fi can reach it?"
          : "Wan2GP is running. Restart now to take it off the network?",
        detail:
          "Restarts in the same mode (Desktop / Browser). Terminal-launched servers come back as plain Browser. The bind address can only change at startup — that is why a restart is needed.",
      });
    } catch {}
    if (choice !== "ok") {
      refreshMainLan().catch(() => {});
      return;
    }
    mainLanRestartFlow(serverMode, on);
  } else {
    showToast(
      on ? "✓ Phone access applies on next launch" : "Phone access off",
    );
    refreshMainLan().catch(() => {});
  }
});
$("mainLanSamePcOpen")?.addEventListener("click", () => {
  if (!_mainLan.samePc) {
    showToast("No URL yet.");
    return;
  }
  deepyWebOpenUrl(_mainLan.samePc);
});
$("mainLanSamePcCopy")?.addEventListener("click", () =>
  deepyWebCopyUrl(_mainLan.samePc),
);
$("mainLanPhoneOpen")?.addEventListener("click", () => {
  if (!_mainLan.phone) {
    showToast("No Phone URL yet — no LAN adapter found.");
    return;
  }
  deepyWebOpenUrl(_mainLan.phone);
});
$("mainLanPhoneCopy")?.addEventListener("click", () => {
  if (!_mainLan.phone) {
    showToast("No Phone URL yet — no LAN adapter found.");
    return;
  }
  deepyWebCopyUrl(_mainLan.phone);
});
$("mainLanPhoneDeepyOpen")?.addEventListener("click", () => {
  if (!_mainLan.phoneDeepy) {
    showToast("No Phone URL yet — no LAN adapter found.");
    return;
  }
  deepyWebOpenUrl(_mainLan.phoneDeepy);
});
$("mainLanPhoneDeepyCopy")?.addEventListener("click", () => {
  if (!_mainLan.phoneDeepy) {
    showToast("No Phone URL yet — no LAN adapter found.");
    return;
  }
  deepyWebCopyUrl(_mainLan.phoneDeepy);
});
$("mainLanPhoneQr")?.addEventListener("click", () => {
  if (!_mainLan.phoneDeepy) {
    showToast("No Phone URL yet — no LAN adapter found.");
    return;
  }
  deepyWebQrUrl(
    _mainLan.phoneDeepy,
    "Scan from your phone camera (same Wi-Fi) — opens the synchronized /deepy/ mobile app",
  );
});

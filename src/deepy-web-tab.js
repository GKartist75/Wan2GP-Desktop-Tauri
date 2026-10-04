// deepy-web-tab.js — Deepy Web standalone card (Same-PC + Phone-LAN HTTP).
//
// Extracted from app.js as a pure move: $, showToast and window.w2gp are all
// globals, so no imports are needed. Loaded AFTER app.js in index.html.

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
      qwen38_9b: "local-qwen38",
    }[profile] || null
  );
}

// Resolve which Prime engine a Deepy Web start should boot: the SAVED Prime
// engine when one is configured (never silently switched to OpenCode), else
// the panel default (OpenCode) when installed, else any installed remote,
// else local Qwen3.8 9B/27B (the boot verifies weights fail-closed). Returns
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
  // Last resort: local Prime — deepy_web_start verifies the 9B/27B weights and
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
function deepyWebExtraArgs() {
  const v = ($("deepyWebExtraArgsInput") || {}).value;
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
        if ($("deepyWebExtraArgsInput") && typeof cfg.deepyExtraArgs === "string" && !$("deepyWebExtraArgsInput").value)
          $("deepyWebExtraArgsInput").value = cfg.deepyExtraArgs;
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
          ? "Phone-LAN mode uses --listen: the server becomes reachable from anywhere on your local network (and by extension your VPN). Windows may show a firewall prompt — allow it on private networks. No firewall rules are created silently."
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
  const extraArgs = deepyWebExtraArgs();
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
      if (extraArgs) cfg.deepyExtraArgs = extraArgs;
      else delete cfg.deepyExtraArgs;
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
    }, publicUrl, extraArgs);
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
  const raw = String(($("deepyWebPortInput") || {}).value == null ? "" : $("deepyWebPortInput").value).trim();
  const port = deepyWebPortArg();
  if (raw !== "" && !port) {
    showToast("Port must be 1–65535 (or empty for auto).");
    return;
  }
  try {
    const cfg = await window.w2gp.configLoad().catch(() => ({}));
    if (cfg && typeof cfg === "object") {
      if (port) {
        cfg.deepyPort = port;
        showToast("Deepy Web port set to " + port);
      } else {
        // Empty = reset to auto (server port + 1): drop the saved override
        // so resolve_ports falls back to the default again.
        delete cfg.deepyPort;
        try {
          $("deepyWebPortInput").value = "";
        } catch {}
        showToast("Deepy Web port reset to auto (server port + 1)");
      }
      await window.w2gp.configSave(cfg);
    }
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

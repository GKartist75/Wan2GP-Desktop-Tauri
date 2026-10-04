// metrics-tab.js — Live topbar metrics: CPU/GPU/RAM/VRAM sparklines.
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

// ── Live topbar metrics (CPU/GPU/RAM/VRAM sparklines) ──
const _sparkHistory = {
  cpu: [],
  gpu: [],
  gpu2: [],
  ram: [],
  vram: [],
  vram2: [],
};
const _sparkMax = 60; // samples kept (~2 min at 2s)

function drawSpark(id, data, color) {
  const c = $(id);
  if (!c) return;
  const ctx = c.getContext("2d");
  const w = c.width,
    h = c.height;
  ctx.clearRect(0, 0, w, h);
  if (data.length < 2) return;
  const max = 100;
  ctx.beginPath();
  data.forEach((v, i) => {
    const x = (i / (data.length - 1)) * w;
    const y = h - (Math.max(0, Math.min(max, v)) / max) * h;
    if (i === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  });
  ctx.strokeStyle = color;
  ctx.lineWidth = 1.25;
  ctx.stroke();
  // fill under curve
  ctx.lineTo(w, h);
  ctx.lineTo(0, h);
  ctx.closePath();
  ctx.fillStyle = color + "22";
  ctx.fill();
}

function pushMetric(key, val) {
  const arr = _sparkHistory[key];
  arr.push(val == null ? 0 : val);
  if (arr.length > _sparkMax) arr.shift();
}

function startMetricsPolling() {
  const tick = async () => {
    // Skip sampling only when the window itself is hidden/minimized.
    // The topbar metrics stay visible in embed/webview mode while dashBody
    // is hidden — gating on dashBody froze them whenever Wan2GP runs
    // embedded. (visibilitychange already pauses the timer when hidden.)
    if (document.hidden) return;
    let m;
    try {
      m = await window.w2gp.getSystemMetrics();
    } catch {
      return;
    }
    if (!m) return;
    if (m.ramFree) {
      const el = $("specRamFree");
      if (el) el.textContent = "(" + m.ramFree + " free)";
    }
    if (m.vramFree) {
      const el = $("specVramFree");
      if (el) el.textContent = "(" + m.vramFree + " free)";
    }
    pushMetric("cpu", m.cpu);
    pushMetric("gpu", m.gpu);
    pushMetric("ram", m.ram);
    pushMetric("vram", m.vram);
    if ($("valCpu"))
      $("valCpu").textContent = m.cpu == null ? "—" : m.cpu + "%";
    if ($("valGpu"))
      $("valGpu").textContent = m.gpu == null ? "—" : m.gpu + "%";
    if ($("valRam"))
      $("valRam").textContent =
        m.ramUsed == null ? "—" : m.ramUsed + "/" + m.ramTotal;
    if ($("valVram"))
      $("valVram").textContent = m.vramUsed
        ? m.vramUsed + "/" + m.vramTotal
        : "—";
    drawSpark("sparkCpu", _sparkHistory.cpu, "#4ADE80");
    drawSpark("sparkGpu", _sparkHistory.gpu, "#60A5FA");
    drawSpark("sparkRam", _sparkHistory.ram, "#FBBF24");
    drawSpark("sparkVram", _sparkHistory.vram, "#F472B6");
    // 2nd GPU (iGPU or dual dGPU) — ponytail: flicker fix — only toggle display when state actually changes
    const hasGpu2 = m.gpus && m.gpus.length > 1 && m.gpus[1] != null;
    const g2 = hasGpu2
      ? m.gpus[1]
      : m.gpu2 == null
        ? null
        : {
            gpu: m.gpu2,
            vram: m.vram2,
            vramUsed: m.vramUsed2,
            vramTotal: m.vramTotal2,
          };
    const mg2 = $("metricGpu2"),
      mv2 = $("metricVram2");
    const shouldShow = !!(g2 && g2.gpu != null);
    const isShown = mg2 && mg2.style.display !== "none";
    if (shouldShow) {
      pushMetric("gpu2", g2.gpu);
      pushMetric("vram2", g2.vram);
      if ($("valGpu2")) $("valGpu2").textContent = g2.gpu + "%";
      if ($("valVram2"))
        $("valVram2").textContent = g2.vramUsed
          ? g2.vramUsed + "/" + g2.vramTotal
          : "—";
      drawSpark("sparkGpu2", _sparkHistory.gpu2, "#A78BFA");
      drawSpark("sparkVram2", _sparkHistory.vram2, "#FB7185");
      if (!isShown) {
        if (mg2) mg2.style.display = "";
        if (mv2) mv2.style.display = "";
      }
      if (mg2)
        mg2.title =
          "GPU 2" + (m.gpus[1] ? " — " + (m.gpus[1].vramTotal || "") : "");
      if (mv2)
        mv2.title =
          "VRAM 2 " + (g2.vramUsed || "") + "/" + (g2.vramTotal || "");
    } else if (isShown) {
      if (mg2) mg2.style.display = "none";
      if (mv2) mv2.style.display = "none";
    }
  };
  if (window.__metricsTimer) clearInterval(window.__metricsTimer);
  window.__metricsTick = tick;
  tick();
  window.__metricsTimer = setInterval(tick, 3000);
  // Pause the 2s nvidia-smi sampling while the window is hidden/minimized;
  // resume with an immediate tick on visibility.
  if (!window.__metricsVisBound) {
    window.__metricsVisBound = () => {
      if (document.hidden) {
        if (window.__metricsTimer) {
          clearInterval(window.__metricsTimer);
          window.__metricsTimer = null;
        }
      } else if (!window.__metricsTimer) {
        startMetricsPolling();
      }
    };
    document.addEventListener("visibilitychange", window.__metricsVisBound);
  }
}

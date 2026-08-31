import { w2gp } from "./w2gp.js";
const { invoke } = window.__TAURI__.core;

let greetInputEl, greetMsgEl;
async function greet() { greetMsgEl.textContent = await invoke("greet", { name: greetInputEl.value }); }
function renderJson(el, obj) { el.textContent = typeof obj === "string" ? obj : JSON.stringify(obj, null, 2); }

window.addEventListener("DOMContentLoaded", () => {
  greetInputEl = document.querySelector("#greet-input");
  greetMsgEl = document.querySelector("#greet-msg");
  document.querySelector("#greet-form")?.addEventListener("submit", (e) => { e.preventDefault(); greet(); });

  const gpuOut = document.querySelector("#gpu-out");
  async function probe(cmd, label) {
    gpuOut.textContent = label;
    try { renderJson(gpuOut, await invoke(cmd)); } catch (e) { gpuOut.textContent = String(e); }
  }
  document.querySelector("#btn-gpu")?.addEventListener("click", () => probe("detect_gpu", "probing nvidia-smi…"));
  document.querySelector("#btn-gpus")?.addEventListener("click", () => probe("detect_gpus", "listing GPUs…"));
  document.querySelector("#btn-hw")?.addEventListener("click", () => probe("detect_hardware", "detecting hardware…"));
  document.querySelector("#btn-python")?.addEventListener("click", () => probe("check_python", "probing python…"));
  document.querySelector("#btn-git")?.addEventListener("click", () => probe("check_git", "probing git…"));
  document.querySelector("#btn-status")?.addEventListener("click", () => probe("get_status", "fetching status…"));
  document.querySelector("#btn-installed")?.addEventListener("click", () => probe("check_installed", "checking install…"));
  document.querySelector("#btn-paths")?.addEventListener("click", () => probe("get_install_paths", "fetching paths…"));
  document.querySelector("#btn-disks")?.addEventListener("click", () => probe("get_disk_space", "disk space…"));
  document.querySelector("#btn-config")?.addEventListener("click", async () => { gpuOut.textContent = "loading config…"; try { const c = await w2gp.configLoad(); renderJson(gpuOut, c); } catch (e) { gpuOut.textContent = String(e); } });
  document.querySelector("#btn-w2gp")?.addEventListener("click", async () => {
    gpuOut.textContent = "probing w2gp shim (101 handlers)…";
    const results = {};
    const tests = [
      ["check_installed", w2gp.checkInstalled], ["detect_gpus", w2gp.detectGpus], ["detect_hardware", w2gp.detectHardware],
      ["get_install_paths", w2gp.getInstallPaths], ["config_load", w2gp.configLoad], ["manage_list", w2gp.manageList],
      ["get_desktop_version", w2gp.getDesktopVersion], ["uv_cache_info", w2gp.uvCacheInfo], ["get_model_paths", w2gp.getModelPaths],
      ["deepy_status", w2gp.deepyStatus], ["memory_profile_read", w2gp.memoryProfileRead], ["auto_tune_detect", w2gp.autoTuneDetect],
    ];
    for (const [name, fn] of tests) {
      try { const v = await fn(); results[name] = typeof v === "string" ? v.slice(0,120) : Array.isArray(v) ? `array[${v.length}]` : v === null ? null : typeof v === "object" ? Object.keys(v).slice(0,5) : String(v).slice(0,120); }
      catch (e) { results[name] = "ERR:" + String(e).slice(0,80); }
    }
    renderJson(gpuOut, results);
  });

  // iframe embed
  const wanFrame = document.querySelector("#wan-frame");
  const wanPlaceholder = document.querySelector("#wan-placeholder");
  const wanUrl = document.querySelector("#wan-url");
  document.querySelector("#btn-load-wan")?.addEventListener("click", () => {
    const url = wanUrl.value.trim() || "http://localhost:7860";
    wanFrame.src = url; wanFrame.style.display = "block";
    if (wanPlaceholder) wanPlaceholder.style.display = "none";
  });
  document.querySelector("#btn-open-external")?.addEventListener("click", async () => {
    const url = wanUrl.value.trim() || "http://localhost:7860";
    try { await invoke("plugin:opener|open_url", { url }); } catch { window.open(url, "_blank"); }
  });
});

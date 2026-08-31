const { invoke } = window.__TAURI__.core;

let greetInputEl;
let greetMsgEl;

async function greet() {
  greetMsgEl.textContent = await invoke("greet", { name: greetInputEl.value });
}

function renderJson(el, obj) {
  el.textContent = typeof obj === "string" ? obj : JSON.stringify(obj, null, 2);
}

window.addEventListener("DOMContentLoaded", () => {
  greetInputEl = document.querySelector("#greet-input");
  greetMsgEl = document.querySelector("#greet-msg");
  document.querySelector("#greet-form").addEventListener("submit", (e) => {
    e.preventDefault();
    greet();
  });

  const gpuOut = document.querySelector("#gpu-out");
  async function probe(cmd, label) {
    gpuOut.textContent = label;
    try { renderJson(gpuOut, await invoke(cmd)); } catch (e) { gpuOut.textContent = String(e); }
  }
  document.querySelector("#btn-gpu").addEventListener("click", () => probe("detect_gpu", "probing nvidia-smi…"));
  document.querySelector("#btn-python")?.addEventListener("click", () => probe("check_python", "probing python…"));
  document.querySelector("#btn-git")?.addEventListener("click", () => probe("check_git", "probing git…"));
  document.querySelector("#btn-status").addEventListener("click", () => probe("get_status", "fetching status…"));

  // Embedded Wan2GP — iframe proves BrowserView replacement without native add_child
  const wanFrame = document.querySelector("#wan-frame");
  const wanPlaceholder = document.querySelector("#wan-placeholder");
  const wanUrl = document.querySelector("#wan-url");
  document.querySelector("#btn-load-wan")?.addEventListener("click", () => {
    const url = wanUrl.value.trim() || "http://localhost:7860";
    wanFrame.src = url;
    wanFrame.style.display = "block";
    if (wanPlaceholder) wanPlaceholder.style.display = "none";
  });
  document.querySelector("#btn-open-external")?.addEventListener("click", async () => {
    const url = wanUrl.value.trim() || "http://localhost:7860";
    try { await invoke("plugin:opener|open_url", { url }); } catch { window.open(url, "_blank"); }
  });
});

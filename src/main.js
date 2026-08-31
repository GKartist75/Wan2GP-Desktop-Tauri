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
  document.querySelector("#btn-gpu").addEventListener("click", async () => {
    gpuOut.textContent = "probing nvidia-smi…";
    try {
      const res = await invoke("detect_gpu");
      renderJson(gpuOut, res);
    } catch (e) {
      gpuOut.textContent = String(e);
    }
  });
  document.querySelector("#btn-status").addEventListener("click", async () => {
    gpuOut.textContent = "fetching status…";
    try {
      const res = await invoke("get_status");
      renderJson(gpuOut, res);
    } catch (e) {
      gpuOut.textContent = String(e);
    }
  });
});

// kernelsync-tab.js — GPU Kernel Wheels: Sync button and wheel update flow.
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

// ── GPU Kernel Wheels: Sync button ──
// Reinstalls every kernel wheel the active GPU's profile declares. Streams to
// the Console; refreshes the dashboard when done so versions update live.
$("syncKernelsBtn")?.addEventListener("click", async function () {
  if (this.disabled) return;
  this.disabled = true;
  this.textContent = "Updating…";
  try {
    const r = await window.w2gp.syncKernels();
    if (r && r.success) showToast("✓ GPU wheels updated");
    else showToast("✗ Update failed: " + (r && r.error ? r.error : "unknown"));
  } catch (e) {
    showToast("✗ Update failed: " + errText(e));
  } finally {
    this.disabled = false;
    this.textContent = "↻ Update GPU Wheels";
    setTimeout(refreshDashboard, 1500);
  }
});
$("restoreKernelsBtn")?.addEventListener("click", async function () {
  if (this.disabled) return;
  if (
    !(await askConfirm(
      "Reinstall setup_config wheels exactly as published? This drops launcher overrides (GGUF floor off; sage installs the published pin).",
    ))
  )
    return;
  this.disabled = true;
  this.textContent = "Restoring…";
  try {
    const r = await window.w2gp.restoreKernels();
    if (r && r.success) showToast("✓ GPU wheels restored to upstream set");
    else showToast("✗ Restore failed: " + (r && r.error ? r.error : "unknown"));
  } catch (e) {
    showToast("✗ Restore failed: " + errText(e));
  } finally {
    this.disabled = false;
    this.textContent = "Restore GPU Wheels";
    setTimeout(refreshDashboard, 1500);
  }
});
// Experimental AMD HIP GGUF wheel (RDNA4 opt-in / reinstall path).
// Same dist name as the CUDA GGUF wheel; installs in THIS env's torch
// 2.13+rocm10 stack (Update GPU Wheels installs it too).
$("installHipGgufBtn")?.addEventListener("click", async function () {
  if (this.disabled) return;
  if (
    !(await askConfirm(
      "Install experimental HIP GGUF 1.0.27 (torch213rocm100py312) for AMD RDNA4 (RX 9060/9070, gfx1200/gfx1201)?\n\nInstalls in this env's torch 2.13+rocm10 stack - the wheel checks the ROCm torch build at import. Update GPU Wheels installs the same wheel automatically. Replaces the CUDA GGUF wheel. Validation pending; paged-attention SDPA fallback expected.",
    ))
  )
    return;
  this.disabled = true;
  this.textContent = "Installing…";
  try {
    const r = await window.w2gp.installHipGguf();
    if (r && r.success) showToast("✓ HIP GGUF wheel installed");
    else showToast("✗ HIP install failed: " + (r && r.error ? r.error : "unknown"));
  } catch (e) {
    showToast("✗ HIP install failed: " + errText(e));
  } finally {
    this.disabled = false;
    this.textContent = "HIP GGUF (exp)";
    setTimeout(refreshDashboard, 1500);
  }
});
// Optional WGPKernels INT8 wheel (deepbeepmeep/WGPKernels v1.1.2, in
// setup_config since upstream 17307e5 but still an explicit opt-in here).
// The backend picks the wheel from the active env's torch build
// (cu130/cu128, or rocm100 on an AMD ROCm 10 env) and refuses
// GPUs below compute 8.0 / outside RDNA4.
$("installWgpkernelsBtn")?.addEventListener("click", async function () {
  if (this.disabled) return;
  if (
    !(await askConfirm(
      "Install the WGPKernels 1.1.2 wheel for INT8 fast kernels?\n\nOptional upstream wheel (also installed by Update GPU Wheels). It matches your torch build automatically: cu130/cu128 for NVIDIA RTX 30/40/50, or the experimental rocm100 build for AMD RDNA4 (RX 9060/9070 series). WanGP's int8_kernels = Auto will prefer it once present.",
    ))
  )
    return;
  this.disabled = true;
  this.textContent = "Installing…";
  try {
    const r = await window.w2gp.installWgpkernels();
    if (r && r.success)
      showToast("✓ WGPKernels " + (r.version || "") + " installed (" + (r.torchKind || "") + ")");
    else showToast("✗ WGPKernels install failed: " + (r && r.error ? r.error : "unknown"));
  } catch (e) {
    showToast("✗ WGPKernels install failed: " + errText(e));
  } finally {
    this.disabled = false;
    this.textContent = "WGPKernels (INT8)";
    setTimeout(refreshDashboard, 1500);
  }
});

async function loadModelPaths() {
  const paths = await window.w2gp.getModelPaths();
  $("dashCkptPath").textContent = breakPath(paths?.checkpoints) || "(default)";
  $("dashCkptPath").title = paths?.checkpoints || "";
  $("dashLoraPath").textContent = breakPath(paths?.loras) || "(default)";
  $("dashLoraPath").title = paths?.loras || "";
  $("dashOutputPath").textContent = breakPath(paths?.output) || "(default)";
  $("dashOutputPath").title = paths?.output || "";
}

// When changing a model folder via the pencil, ask whether to physically MOVE
// the existing files (so nothing is re-downloaded) or just point Wan2GP at the
// new (empty) location. Then write wgp_config.json accordingly.
async function changeModelFolder(type, key, _cfgKey, singular) {
  const dir = await window.w2gp.selectFolder();
  if (!dir) return;
  // ponytail: reject file-as-folder (orca-paste Temp png)
  if (isFilePickedAsFolder(dir)) {
    notifyUser("Please select a folder, not a file:\n" + dir);
    return;
  }
  const cur = await window.w2gp
    .getModelPaths()
    .then(
      (p) =>
        ({ ckpts: p?.checkpoints, loras: p?.loras, output: p?.output })[type],
    );
  if (cur && cur.toLowerCase() === dir.toLowerCase()) {
    window.w2gp.openFolder(dir);
    return;
  }
  const choice = await window.w2gp.confirmDialog({
    title: "Move " + singular + "?",
    message: "Change " + singular + " folder to:\n  " + dir,
    detail: cur
      ? "Do you want to MOVE the existing files from the old location into the new folder, or just point Wan2GP at the new (empty) folder?\n\nOld: " +
        cur
      : "Point Wan2GP at the new folder?",
    buttons: cur
      ? ["Move existing files", "Just point (no move)", "Cancel"]
      : ["OK", "Cancel"],
    defaultId: cur ? 0 : 0,
    cancelId: cur ? 2 : 1,
  });
  if (choice === "cancel") {
    window.w2gp.openFolder(dir);
    return;
  }
  if (choice === "move" && cur) {
    const r = await window.w2gp.moveFolder(cur, dir);
    if (!r || !r.ok) {
      notifyUser("Could not move files:\n" + ((r && r.error) || "unknown"));
    }
  }
  // Write the real config (what Wan2GP reads) so the change takes effect next launch.
  const patch = {};
  patch[key] = type === "ckpts" ? [dir, "."] : dir;
  await window.w2gp.writeWgpConfig(patch);
  const cfg = await window.w2gp.configLoad();
  if (type === "ckpts") cfg.modelCkptsPath = dir;
  else if (type === "loras") cfg.modelLorasPath = dir;
  else cfg.modelOutputPath = dir;
  await window.w2gp.configSave(cfg);
  await loadModelPaths();
  showToast("✓ " + singular + " folder updated — restart Wan2GP to apply");
}

$("dashBrowseCkpt").addEventListener("click", () =>
  changeModelFolder("ckpts", "checkpointsPaths", "checkpoints", "Checkpoints"),
);
$("dashBrowseLora").addEventListener("click", () =>
  changeModelFolder("loras", "lorasRoot", "loras", "LoRAs"),
);
$("dashBrowseOutput").addEventListener("click", () =>
  changeModelFolder("output", "savePath", "output", "Output"),
);

$("desktopRepoLink").addEventListener("click", (e) => {
  e.preventDefault();
  window.w2gp.openExternal(
    "https://github.com/GKartist75/Wan2GP-Desktop-Tauri",
  );
});
$("discussionsLink").addEventListener("click", (e) => {
  e.preventDefault();
  window.w2gp.openExternal(
    "https://github.com/GKartist75/Wan2GP-Desktop-Tauri/discussions",
  );
});
$("ytLink").addEventListener("click", (e) => {
  e.preventDefault();
  window.w2gp.openExternal("https://www.youtube.com/@GK-Artist");
});

async function loadPaths(skipModelPaths) {
  const p = await window.w2gp.getInstallPaths();
  if (!p) return;
  const set = (id, val) => {
    const e = $(id);
    if (e) {
      e.textContent = breakPath(val) || "—";
      e.title = val || "";
    }
  };
  set("pathAppData", p.repo);
  set("installAppDataPath", p.appData);
  // Guard: if the chosen install location is a bare drive root (e.g. D:\),
  // the install is invalid — disable the Install button and warn the user.
  const rootBad = isDriveRoot(p.appData);
  const startBtn = $("installStartBtn");
  const rootWarn = $("installRootWarn");
  if (rootBad) {
    if (startBtn) {
      startBtn.disabled = true;
      startBtn.title = "Choose a folder, not a drive root.";
    }
    if (rootWarn) {
      rootWarn.textContent =
        "⚠ Install location is a drive root (" +
        p.appData +
        "). Pick a folder using Browse.";
      rootWarn.classList.remove("hidden");
    }
  } else {
    if (startBtn) {
      startBtn.disabled = false;
      startBtn.title = "";
    }
    if (rootWarn) rootWarn.classList.add("hidden");
  }
  // The top warning banner already owns the in-launcher "Migrate to new location"
  // button (shown when legacyRoamingFound), so keep this dashboard card button
  // hidden in that case to avoid two migration buttons. It only appears as a
  // manual re-trigger when there is no legacy roaming dir to migrate.
  const wrap = $("moveToPreferredWrap");
  if (wrap) {
    // ponytail: Tauri isolated — never show roaming migrate-warn
    if (window.__TAURI__) {
      wrap.classList.add("hidden");
    } else if (p.legacyRoamingFound) {
      wrap.classList.add("hidden");
    } else {
      wrap.classList.remove("hidden");
      const cp = $("currentDataDirPath");
      if (cp) cp.textContent = p.appData;
    }
  }
  window.w2gp.getDiskSpace().then((d) => {
    if (!d) return;
    var freeGb = (d.free / 1073741824).toFixed(1);
    $("pathFreeSpace").textContent = freeGb + " GB free";
  });
  if (!skipModelPaths) {
    // Show the model folders the user actually chose. Precedence: a previously
    // saved custom choice (desktop-config.json modelCkptsPath/…) wins; otherwise
    // the dedicated default (C:\\Wan2GP-Models). We used to ALWAYS overwrite with
    // the default here, which is why any custom path silently reverted to
    // C:\\Wan2GP-Models on every refresh (issue #74).
    const md = p.modelsDefault || p.appData;
    let saved = {};
    try {
      saved = (await window.w2gp.configLoad()) || {};
    } catch {}
    const savedCkpts = saved.modelCkptsPath;
    const savedLoras = saved.modelLorasPath;
    const savedOutput = saved.modelOutputPath;
    if (_modelCkpts || savedCkpts)
      setModelPath("ckpts", _modelCkpts || savedCkpts);
    else setModelPath("ckpts", pathJoin(md, "ckpts"));
    if (_modelLoras || savedLoras)
      setModelPath("loras", _modelLoras || savedLoras);
    else setModelPath("loras", pathJoin(md, "loras"));
    if (_modelOutput || savedOutput)
      setModelPath("output", _modelOutput || savedOutput);
    else setModelPath("output", pathJoin(md, "outputs"));
  }
  // Re-triage the target folder when the installer screen is showing
  // (Browse / reset changes the location — verdict must follow).
  try {
    if ($("installer") && $("installer").classList.contains("active")) {
      refreshTargetVerdict().catch(() => {});
      refreshModelDiskGates().catch(() => {});
    }
  } catch {}
}
// Tiny path join that tolerates both separators in the renderer (no node path).
function pathJoin(a, b) {
  return (a || "").replace(/[\\/]+$/, "") + "\\" + b;
}
// True when the path is a bare drive root, e.g. "D:" or "D:\" (but not "D:\Wan2GP").
function isDriveRoot(p) {
  if (!p) return false;
  const norm = (p || "").replace(/[\\/]+$/, "");
  return /^[A-Za-z]:$/.test(norm);
}

$("openAppDataBtn")?.addEventListener("click", () => {
  window.w2gp.getInstallPaths().then((p) => {
    if (p) window.w2gp.openFolder(p.repo);
  });
});
// Move the entire Wan2GP install (no reinstall) — reuse the migration modal
// pre-filled with the current location as the source.
$("changeAppDataBtn")?.addEventListener("click", () => openMigrationModal());

// Per-folder "open" buttons (folder icon) for the three model paths.
$("openCkptBtn")?.addEventListener("click", () =>
  window.w2gp
    .getModelPaths()
    .then((p) => p?.checkpoints && window.w2gp.openFolder(p.checkpoints)),
);
$("openLoraBtn")?.addEventListener("click", () =>
  window.w2gp
    .getModelPaths()
    .then((p) => p?.loras && window.w2gp.openFolder(p.loras)),
);
$("openOutputBtn")?.addEventListener("click", () =>
  window.w2gp
    .getModelPaths()
    .then((p) => p?.output && window.w2gp.openFolder(p.output)),
);

$("moveToPreferredBtn")?.addEventListener("click", () => openMigrationModal());

// installer-tab.js — the Wan2GP install flow (Model download / Env selection).
//
// Extracted from app.js as a pure move. $ and window.w2gp are globals defined by
// app.js, and the one top-level statement here registers a listener (resolved
// at click time), so loading this file after app.js is safe.

// ── Installer ──
let selectedEnvType = "uv";
// Checklist verdict: when the install folder holds a repo without a working env
// (repo_no_env / ours_broken_env), the choice lives in the #targetChoiceList
// radios and the big Install button dispatches it (see startInstall).
let _targetChoiceMode = null;
// True while an install is actually running (set in doInstall, cleared on
// every exit) — verdict refreshes must never resurrect Install mid-install
// (e.g. Browse clicked during a fresh install re-trips repo_no_env).
let _installRunning = false;
// Stashed Fresh-repo backup choice (collect-only modal). The wipe launches
// solely from the big Install button — never from inside the backup dialog.
let _freshBackupChoice = null;
// Snapshot of the live radio pick backing _freshBackupChoice. Compared at
// dispatch time so touching a radio after the modal only re-collects when
// the pick actually moved on (Loop 3 fix).
let _freshBackupPick = null;
// Verdict mode last rendered by refreshTargetVerdict (null = none yet).
// Guards the radio force-check defaults so refreshes preserve user picks.
let _verdictModeShown = null;
// Latest classifyTarget verdict (null when hidden/failed). Lets the
// fallthrough dispatch ask once for foreign folders instead of bouncing.
let _lastVerdict = null;

document.querySelectorAll(".env-type-btn").forEach((btn) => {
  btn.addEventListener("click", () => {
    document
      .querySelectorAll(".env-type-btn")
      .forEach((b) => b.classList.remove("selected"));
    btn.classList.add("selected");
    selectedEnvType = btn.dataset.env;
  });
});

$("installStartBtn").addEventListener("click", startInstall);
// NOTE: no radio change listener voids _freshBackupChoice here. Stash
// staleness is reconciled at dispatch time (snapshot vs live pick), so
// touching a radio after the backup modal must not force a reopen.
// NOTE: the healthy-state trio and the no-env checklist are radios now —
// all launching goes through the big Install button (see startInstall).
// The backup modal below is collect-only; doInstall('reinstall') wipes the
// repo (trash, not delete), reinstalls, and merges the backup back.

$("validateInstallBtn")?.addEventListener("click", async () => {
  const btn = $("validateInstallBtn");
  const warn = $("installStackWarn");
  btn.disabled = true;
  btn.textContent = "Validating…";
  if (warn) warn.textContent = "";
  try {
    const r = await window.w2gp.validateInstall();
    if (r && r.ok) {
      const line = `✓ torch ${r.torch} · CUDA available: ${r.cudaAvailable} (${r.cudaVer})`;
      if (warn) {
        const d = document.createElement("div");
        d.className = "istack-ok";
        d.textContent = "⚡ " + line;
        warn.append(d);
      }
      btn.textContent = "Validated ✓";
    } else {
      if (warn) {
        const d = document.createElement("div");
        d.className = "istack-w";
        d.textContent = "✗ " + ((r && r.error) || "validation failed");
        warn.append(d);
      }
      btn.textContent = "Validate failed";
    }
  } catch (e) {
    if (warn) {
      const d = document.createElement("div");
      d.className = "istack-w";
      d.textContent = "✗ " + ((e && e.message) || String(e));
      warn.append(d);
    }
    btn.textContent = "Validate failed";
  }
});

// When the user picks a bare drive root (e.g. D:), we DON'T apply it (installing
// on a root fails). Instead we show a cross + message on the Install button.
// Cleared as soon as a valid folder is chosen.
let _pendingRoot = null;

function reflectRootBlock(rootPath) {
  const set = (id, val) => {
    const e = $(id);
    if (e) {
      e.textContent = breakPath(val) || "—";
      e.title = val || "";
    }
  };
  set("installAppDataPath", rootPath);
  const startBtn = $("installStartBtn");
  const rootWarn = $("installRootWarn");
  if (startBtn) {
    startBtn.disabled = true;
    startBtn.title = "Choose a folder, not a drive root.";
  }
  if (rootWarn) {
    rootWarn.textContent =
      "⚠ Install location is a drive root (" +
      rootPath +
      "). Pick a folder using Browse.";
    rootWarn.classList.remove("hidden");
  }
}

$("browseAppDataPath")?.addEventListener("click", async () => {
  const folder = await window.w2gp.selectFolder();
  if (!folder) return;
  // Bare drive root: don't block — show it resolved to <root>\Wan2GP and use
  // that (backend rejects raw roots too, defense in depth).
  if (isDriveRoot(folder)) {
    const suggested = pathJoin(folder, "Wan2GP");
    appendLog(
      "[*] Drive root selected (" +
        folder +
        ") — using " +
        suggested +
        " instead.",
    );
    showToast("Using " + suggested);
    try {
      await window.w2gp.setDataDir(suggested);
    } catch (e) {
      appendLog("[!] Could not set install folder: " + ((e && e.message) || e));
      _pendingRoot = suggested;
      reflectRootBlock(suggested);
      return;
    }
    _pendingRoot = null;
    loadPaths();
    return;
  }
  _pendingRoot = null;
  try {
    await window.w2gp.setDataDir(folder);
  } catch (e) {
    if (/drive-root/.test((e && e.message) || String(e))) {
      _pendingRoot = folder;
      reflectRootBlock(folder);
      return;
    }
    throw e;
  }
  loadPaths();
});

$("clearAppDataPath")?.addEventListener("click", async () => {
  await window.w2gp.resetDataDir();
  loadPaths(true);
});

let _modelCkpts = "",
  _modelLoras = "",
  _modelOutput = "";

function setModelPath(type, folder) {
  const elMap = {
    ckpts: "installCkptsPath",
    loras: "installLorasPath",
    output: "installOutputPath",
  };
  const clearMap = {
    ckpts: "clearCkptsPath",
    loras: "clearLorasPath",
    output: "clearOutputPath",
  };
  const el = $(elMap[type]);
  const clearBtn = $(clearMap[type]);
  if (!el) return;
  if (folder) {
    el.textContent = folder;
    el.style.color = "";
    if (clearBtn) clearBtn.style.display = "";
    if (type === "ckpts") _modelCkpts = folder;
    else if (type === "loras") _modelLoras = folder;
    else _modelOutput = folder;
  } else {
    el.textContent = "(default)";
    el.style.color = "var(--text-tertiary)";
    if (clearBtn) clearBtn.style.display = "none";
    if (type === "ckpts") _modelCkpts = "";
    else if (type === "loras") _modelLoras = "";
    else _modelOutput = "";
  }
  // Model drive changed → re-gate disk space (installer screen only).
  try {
    if ($("installer") && $("installer").classList.contains("active"))
      refreshModelDiskGates().catch(() => {});
  } catch {}
}

async function browseModelFolder(type) {
  const folder = await window.w2gp.selectFolder();
  if (!folder) return;
  setModelPath(type, folder);
  // Persist BOTH: the user-facing choice (desktop-config.json, for the UI) AND
  // the file Wan2GP actually reads (wgp_config.json). Previously only the former
  // was written, so the Settings slider was cosmetic and downloads ignored it
  // (issue #74, "Model folders" always reverted to C:\Wan2GP-Models on refresh).
  if (type === "ckpts")
    await window.w2gp.writeWgpConfig({ checkpointsPaths: [folder, "."] });
  else if (type === "loras")
    await window.w2gp.writeWgpConfig({ lorasRoot: folder });
  else await window.w2gp.writeWgpConfig({ savePath: folder });
  const cfg = await window.w2gp.configLoad();
  if (type === "ckpts") cfg.modelCkptsPath = folder;
  else if (type === "loras") cfg.modelLorasPath = folder;
  else cfg.modelOutputPath = folder;
  await window.w2gp.configSave(cfg);
}

$("browseCkptsPath")?.addEventListener("click", () =>
  browseModelFolder("ckpts"),
);
$("browseLorasPath")?.addEventListener("click", () =>
  browseModelFolder("loras"),
);
$("clearCkptsPath")?.addEventListener("click", async () => {
  const p = await window.w2gp.getInstallPaths();
  const def = p?.modelsDefault
    ? pathJoin(p.modelsDefault, "ckpts")
    : "(default)";
  setModelPath("ckpts", "");
  const el = $("installCkptsPath");
  if (el) {
    el.textContent = def;
    el.style.color = "var(--text-tertiary)";
  }
  // Reset the real config too, so the UI and Wan2GP stay in sync (issue #74).
  await window.w2gp.writeWgpConfig({ checkpointsPaths: [def, "."] });
  const cfg = await window.w2gp.configLoad();
  delete cfg.modelCkptsPath;
  await window.w2gp.configSave(cfg);
});
$("clearLorasPath")?.addEventListener("click", async () => {
  const p = await window.w2gp.getInstallPaths();
  const def = p?.modelsDefault
    ? pathJoin(p.modelsDefault, "loras")
    : "(default)";
  setModelPath("loras", "");
  const el = $("installLorasPath");
  if (el) {
    el.textContent = def;
    el.style.color = "var(--text-tertiary)";
  }
  await window.w2gp.writeWgpConfig({ lorasRoot: def });
  const cfg = await window.w2gp.configLoad();
  delete cfg.modelLorasPath;
  await window.w2gp.configSave(cfg);
});
$("browseOutputPath")?.addEventListener("click", () =>
  browseModelFolder("output"),
);
$("clearOutputPath")?.addEventListener("click", async () => {
  const p = await window.w2gp.getInstallPaths();
  const def = p?.modelsDefault
    ? pathJoin(p.modelsDefault, "outputs")
    : "(default)";
  setModelPath("output", "");
  const el = $("installOutputPath");
  if (el) {
    el.textContent = def;
    el.style.color = "var(--text-tertiary)";
  }
  await window.w2gp.writeWgpConfig({ savePath: def });
  const cfg = await window.w2gp.configLoad();
  delete cfg.modelOutputPath;
  await window.w2gp.configSave(cfg);
});

async function startInstall() {
  if (_installRunning) return;
  const _restoreStartBtn = () => {
    const _b = $("installStartBtn");
    if (_b) {
      _b.disabled = false;
      _b.textContent = "Install";
    }
  };
  const _sb0 = $("installStartBtn");
  if (_sb0) {
    _sb0.disabled = true;
    _sb0.textContent = "Working…";
  }
  // Helper to show prereq help card
  function showPrereqHelp(title, text, url, tool) {
    $("prereqHelp").classList.remove("hidden");
    $("prereqTitle").textContent = title;
    $("prereqText").textContent = text;
    $("prereqDownloadBtn").onclick = async function () {
      this.disabled = true;
      this.textContent = "Installing...";
      appendLog("[*] Installing " + tool + "...");
      var r;
      try {
        r = await window.w2gp.installPrerequisite(tool);
      } catch (e) {
        r = { error: (e && e.message) || String(e) };
      } // never leave the button frozen
      this.disabled = false;
      this.textContent = "Download & Install";
      if (r && r.success) {
        if (r.ready) {
          // Tool is on PATH already (registry refresh) — continue automatically.
          $("prereqHelp").classList.add("hidden");
          showToast("✓ " + tool + " installed — continuing…");
          startInstall();
        } else
          showToast("✓ " + tool + " installed. Please restart the launcher.");
      } else showToast("✗ Install failed: " + (r?.error || "unknown"));
    };
    $("prereqManualBtn").onclick = () => {
      window.w2gp.openExternal(url);
    };
    $("installStartBtn").classList.remove("hidden");
    $("envTypeSelect").classList.remove("disabled");
    document
      .querySelectorAll(".env-type-btn")
      .forEach((b) => (b.disabled = false));
  }

  // Check prerequisites (check_command returns {cmd, found} — an object is
  // always truthy, so compare .found; previously a missing tool sailed through
  // and failed 10 minutes into setup.py instead of showing the help card).
  const hasCmd = async (c) => {
    try {
      const r = await window.w2gp.checkCommand(c);
      return !!(r && r.found);
    } catch {
      return false;
    }
  };
  var hasGit = await hasCmd("git");
  if (!hasGit) {
    appendLog("[!] Git not found — showing install help");
    showPrereqHelp(
      "Git not found",
      "Git is required to clone the Wan2GP repository. Click Download to install it silently, or use the manual button.",
      "https://git-scm.com/downloads",
      "git",
    );
    _restoreStartBtn();
    return;
  }
  if (selectedEnvType === "venv") {
    var hasPy = await hasCmd("python");
    if (!hasPy) {
      appendLog("[!] Python not found — showing install help");
      showPrereqHelp(
        "Python not found",
        "Python 3.10 or 3.11 is required for venv installs. Click Download to install Python 3.11 silently, or select uv/conda above.",
        "https://www.python.org/downloads/",
        "python",
      );
      _restoreStartBtn();
      return;
    }
  }
  if (selectedEnvType === "uv") {
    var hasUv = await hasCmd("uv");
    if (!hasUv) {
      appendLog("[!] uv not found — showing install help");
      showPrereqHelp(
        "uv not found",
        "uv is required for uv installs. Click Download to install it via PowerShell, or select venv/conda above.",
        "https://docs.astral.sh/uv/#installation",
        "uv",
      );
      _restoreStartBtn();
      return;
    }
  }
  if (selectedEnvType === "conda") {
    var hasConda = await hasCmd("conda");
    if (!hasConda) {
      appendLog("[!] Conda not found — showing install help");
      showPrereqHelp(
        "Conda not found",
        "Miniconda is required for conda installs. Click Download to install it silently, or select venv/uv above.",
        "https://docs.anaconda.com/miniconda/",
        "conda",
      );
      _restoreStartBtn();
      return;
    }
  }
  // Fresh-repo: collect the backup choice FIRST (modal never launches —
  // the wipe starts solely from the big Install button). Stored, then the
  // user presses Install again for the final are-you-sure + launch.
  // Applies to the no-env checklist AND the healthy-state trio.
  const pickedRadio = (name) =>
    (document.querySelector('input[name="' + name + '"]:checked') || {})
      .value || null;
  const liveFreshPick = () =>
    _targetChoiceMode === "repair-or-fresh"
      ? pickedRadio("targetChoice")
      : _targetChoiceMode === "reinstall-trio"
        ? pickedRadio("reinstallChoice")
        : null;
  const collectFreshBackup = async () => {
    appendLog("[*] Measuring existing installation folders…");
    const choice = await showReinstallBackupModal().catch(() => null);
    if (!choice) return false;
    _freshBackupChoice = choice;
    _freshBackupPick = { mode: _targetChoiceMode, value: liveFreshPick() };
    showToast("Backup choice saved — press Install to start");
    return true;
  };
  const freshPicked =
    (_targetChoiceMode === "repair-or-fresh" &&
      pickedRadio("targetChoice") === "fresh") ||
    (_targetChoiceMode === "reinstall-trio" &&
      pickedRadio("reinstallChoice") === "fresh");
  if (freshPicked && !_freshBackupChoice) {
    await collectFreshBackup();
    _restoreStartBtn();
    return;
  }
  // Are-you-sure gate: the Install button sits below the checks, and
  // nothing starts without explicit confirmation (fresh-repo wipes code).
  let choiceNote = "";
  let wipeWarn = "";
  if (_targetChoiceMode === "repair-or-fresh") {
    const checked = document.querySelector(
      'input[name="targetChoice"]:checked',
    );
    const isFresh = (checked && checked.value) === "fresh";
    choiceNote = isFresh
      ? "Fresh repo (wipe code, keep models)" +
        (_freshBackupChoice
          ? _freshBackupChoice.skip
            ? " — no backup"
            : " — with backup"
          : "")
      : "Install / repair environment (keeps models & settings)";
    if (isFresh && _freshBackupChoice && _freshBackupChoice.skip)
      wipeWarn =
        "\n⚠ WILL WIPE code, plugins, finetunes, settings and any models inside the folder.";
  } else if (_targetChoiceMode === "reinstall-trio") {
    const v = pickedRadio("reinstallChoice");
    choiceNote =
      v === "fresh"
        ? "Reinstall (fresh)" +
          (_freshBackupChoice
            ? _freshBackupChoice.skip
              ? " — no backup"
              : " — with backup"
            : "")
        : v === "skip"
          ? "Use existing (health-check)"
          : "Update & keep files";
    if (v === "fresh" && _freshBackupChoice && _freshBackupChoice.skip)
      wipeWarn =
        "\n⚠ WILL WIPE code, plugins, finetunes, settings and any models inside the folder.";
  }
  let locNote = "";
  try {
    const paths = await window.w2gp.getInstallPaths().catch(() => null);
    if (paths && (paths.repo || paths.dataDir))
      locNote = "\nLocation: " + (paths.repo || paths.dataDir);
  } catch {}
  if (
    !window.confirm(
      "Start the Wan2GP install now?" +
        (choiceNote ? "\nChoice: " + choiceNote : "") +
        "\nEnvironment: " +
        selectedEnvType +
        locNote +
        wipeWarn +
        "\n\nThis downloads several GB and takes 5–20 minutes.",
    )
  ) {
    _restoreStartBtn();
    return;
  }
  // Windows long paths gate (issue #15): enabling only takes effect
  // after a reboot, so offer it BEFORE any download — and stop here
  // when enabled, telling the user to reboot and re-run Install.
  // Covers every pipeline (AMD/Intel/NVIDIA share this entry point).
  try {
    const lp = await window.w2gp.tsLongPathsStatus().catch(() => null);
    if (lp && !lp.enabled) {
      const lpChoice = await window.w2gp.confirmDialog({
        title: "Enable Windows long paths?",
        message:
          "Long paths are OFF - deep ML package trees can fail mid-install past 260 chars. Enable now (needs admin approval)? You must reboot BEFORE installing for it to take effect.",
      });
      if (lpChoice === "ok" || lpChoice === 0) {
        try {
          const lr = await window.w2gp.tsLongPathsEnable();
          if (lr && (lr.ok || lr.already)) {
            appendLog(
              "[*] Long paths enabled - reboot Windows now, then run Install again.",
            );
            showToast("Long paths enabled - reboot, then Install again");
          } else {
            appendLog(
              "[!] Long paths enable failed: " +
                ((lr && lr.error) || "unknown") +
                " - see Manage → Troubleshooting.",
            );
          }
        } catch (e) {
          appendLog("[!] Long paths enable failed: " + errText(e));
        }
        _restoreStartBtn();
        return;
      }
      appendLog(
        "[!] Continuing without Windows long paths - mid-install failures past 260 chars are possible.",
      );
    }
  } catch {}
  // Checklist + trio dispatch: the big Install button is the ONLY launcher.
  // Fresh wipes consume the stashed backup choice (collected earlier) — the
  // wipe warning already lives in the single CONFIRM above, so dispatch
  // launches directly with no second dialog.
  const launchFresh = async () => {
    const live = liveFreshPick();
    const match =
      _freshBackupChoice &&
      _freshBackupPick &&
      _freshBackupPick.mode === _targetChoiceMode &&
      _freshBackupPick.value === live;
    if (!match) {
      // Live pick moved on (or no stash survived triage) — re-collect the
      // backup choice instead of launching stale, then wait for Press 2.
      _freshBackupChoice = null;
      _freshBackupPick = { mode: _targetChoiceMode, value: live };
      appendLog("[*] Measuring existing installation folders…");
      const choice = await showReinstallBackupModal().catch(() => null);
      if (!choice) {
        _restoreStartBtn();
        return;
      }
      _freshBackupChoice = choice;
      _freshBackupPick = { mode: _targetChoiceMode, value: liveFreshPick() };
      showToast("Backup choice saved — press Install to start");
      _restoreStartBtn();
      return;
    }
    const stored = _freshBackupChoice;
    _freshBackupChoice = null;
    _freshBackupPick = null;
    resetTasks();
    if (stored && stored.skip) {
      doInstall(null, "reinstall", { backup: false });
      return;
    }
    doInstall(null, "reinstall", stored);
    return;
  };
  if (_targetChoiceMode === "repair-or-fresh") {
    const checked = document.querySelector(
      'input[name="targetChoice"]:checked',
    );
    if ((checked && checked.value) === "fresh") {
      await launchFresh();
      return;
    }
    _freshBackupChoice = null;
    _freshBackupPick = null;
    resetTasks();
    doInstall(null, "update");
    return;
  }
  if (_targetChoiceMode === "reinstall-trio") {
    const v = pickedRadio("reinstallChoice");
    if (v === "fresh") {
      await launchFresh();
      return;
    }
    _freshBackupChoice = null;
    _freshBackupPick = null;
    resetTasks();
    doInstall(null, v === "skip" ? "skip" : "update");
    return;
  }
  show("installer");
  resetTasks();
  $("envTypeSelect").classList.add("disabled");
  document
    .querySelectorAll(".env-type-btn")
    .forEach((b) => (b.disabled = true));
  $("installStartBtn").classList.add("hidden");
  $("installSubtitle").textContent = "Setting up Wan2GP...";
  const installed = await window.w2gp.checkInstalled();
  if (installed.repo) {
    if (_lastVerdict === "foreign") {
      // Foreign folder with no checklist/trio: ask once whether to merge
      // upstream over the unknown files instead of bouncing to a re-render.
      const foreignChoice = await window.w2gp
        .confirmDialog({
          title: "Install into this folder anyway?",
          message:
            "Unknown files live here - upstream Wan2GP merges over them (empty folder is safer). Proceed?",
        })
        .catch(() => null);
      if (foreignChoice === "ok" || foreignChoice === 0) {
        doInstall(installed);
        return;
      }
      showToast("Cancelled - pick an empty folder with Browse to install");
      return;
    }
    // The verdict card owns the choices (healthy → Keep/Update/Skip trio,
    // broken → adopt/repair, pinokio → models reuse) so stale buttons can
    // never offer Keep/Skip for a folder that holds no install.
    await refreshTargetVerdict().catch(() => null);
    return;
  }
  doInstall(installed);
}

// Reinstall backup dialog: folder size + breakdown, backup checkbox,
// per-model Move-to rows with Browse. Resolves {backup, moveModels} |
// {skip:true} | null (cancel). Model destinations must be OUTSIDE the wiped folder.
function showReinstallBackupModal() {
  return new Promise((resolve) => {
    const modal = $("backupModal");
    if (!modal) {
      resolve({ backup: true, moveModels: [] });
      return;
    }
    const done = (v) => {
      modal.classList.add("hidden");
      resolve(v);
    };
    $("backupCloseBtn").onclick = () => done(null);
    $("backupCancelBtn").onclick = () => done(null);
    $("backupSkipBtn").onclick = () => done({ skip: true });
    const sumEl = $("backupSizeSummary"),
      bdEl = $("backupBreakdown");
    const secEl = $("backupModelsSection"),
      rowsEl = $("backupModelsRows");
    sumEl.textContent = "calculating…";
    bdEl.innerHTML = "";
    secEl.style.display = "none";
    rowsEl.innerHTML = "";
    $("backupIncludeCheckbox").checked = true;
    modal.classList.remove("hidden");
    // Gather: repo path, size breakdown, model locations. This async tail
    // runs in a fail-closed IIFE (never an async executor): any unexpected
    // throw cancels via done(null) instead of hanging the installer on a
    // never-settling promise. Callers already treat null as cancel.
    // (Tail keeps executor-level indent so the diff stays reviewable.)
    (async () => {
      const paths = await window.w2gp.getInstallPaths().catch(() => null);
      const repo = (paths && paths.repo) || "";
      let size = null;
      try {
        size = await window.w2gp.folderSize(repo);
      } catch (e) {
        size = { error: e.message };
      }
      if (!size || size.error) {
        sumEl.textContent =
          "size unavailable (" + ((size && size.error) || "unknown") + ")";
      } else {
        sumEl.textContent = fmtBytes(size.bytes) + " total";
        bdEl.textContent = "";
        for (const e of (size.entries || []).slice(0, 8)) {
          const d = document.createElement("div");
          d.className = "istack-row";
          const k = document.createElement("span");
          k.className = "istack-k";
          k.textContent = e.name;
          const v = document.createElement("span");
          v.className = "istack-v";
          v.textContent = fmtBytes(e.bytes);
          d.append(k, v);
          bdEl.append(d);
        }
      }
      const entryBytes = {};
      for (const e of (size && size.entries) || [])
        entryBytes[e.name.toLowerCase()] = e.bytes;
      // Which model folders live INSIDE the wiped repo?
      const mp = await window.w2gp.getModelPaths().catch(() => null);
      const norm = (p) => (p || "").replace(/\//g, "\\");
      const abs = (p) =>
        /^[A-Za-z]:\\/.test(p || "") || /\\\\/.test(p || "")
          ? norm(p)
          : norm(repo + "\\" + (p || ""));
      const inside = (p) => {
        const a = abs(p).toLowerCase();
        return a.startsWith(repo.toLowerCase().replace(/\\+$/, "") + "\\");
      };
      const found = [];
      const repoNorm = repo.toLowerCase().replace(/\\+$/, "");
      const push = (type, label, from, trusted) => {
        if (!from) return;
        const a = abs(from);
        if (a.toLowerCase() === repoNorm) return; // '.' == the repo itself — never offer to move it
        if (
          !inside(from) ||
          found.some((f) => f.from.toLowerCase() === a.toLowerCase())
        )
          return;
        found.push({ type, label, from: a, trusted: !!trusted });
      };
      if (mp) {
        push("ckpts", "Checkpoints", mp.checkpoints, true);
        push("loras", "LoRAs", mp.loras, true);
        push("output", "Output", mp.output, true);
      }
      // Default subdirs count too (ckpts/, loras/, outputs/ under the repo).
      for (const [sub, type, label] of [
        ["ckpts", "ckpts", "Checkpoints"],
        ["loras", "loras", "LoRAs"],
        ["outputs", "output", "Output"],
        ["output", "output", "Output"],
      ]) {
        push(type, label, repo + "\\" + sub, false);
      }
      // Untrusted (default-subdir) rows need a size entry proving they exist;
      // config-listed rows are trusted as-is.
      const rows = found.filter((f) => {
        if (f.trusted) return true;
        const base = f.from.split("\\").pop().toLowerCase();
        return Object.hasOwn(entryBytes, base);
      });
      const dsts = {};
      $("backupGoBtn").onclick = () => {
        const moveModels = [];
        rows.forEach((r, i) => {
          if (dsts[i])
            moveModels.push({ type: r.type, from: r.from, to: dsts[i] });
        });
        done({ backup: $("backupIncludeCheckbox").checked, moveModels });
      };
      if (!rows.length) return;
      secEl.style.display = "";
      rowsEl.innerHTML = "";
      rows.forEach((r, i) => {
        const base = r.from.split("\\").pop().toLowerCase();
        const div = document.createElement("div");
        div.className = "migrate-row";
        const lab = document.createElement("label");
        lab.textContent = r.label + " (" + fmtBytes(entryBytes[base]) + ")";
        const path = document.createElement("div");
        path.className = "migrate-path";
        const inp = document.createElement("input");
        inp.type = "text";
        inp.id = "backupDst" + i;
        inp.readOnly = true;
        inp.placeholder = "stays — will be deleted";
        const btn = document.createElement("button");
        btn.className = "btn btn-ghost small";
        btn.id = "backupBrowse" + i;
        btn.textContent = "Move to…";
        path.append(inp, btn);
        const hint = document.createElement("div");
        hint.className = "istack-hint";
        hint.textContent = r.from;
        div.append(lab, path, hint);
        rowsEl.appendChild(div);
        $("backupBrowse" + i).onclick = async () => {
          const dir = await window.w2gp.selectFolder().catch(() => null);
          if (!dir) return;
          if (isDriveRoot(dir)) {
            alert("Pick a folder, not a drive root.");
            return;
          }
          if (
            dir
              .toLowerCase()
              .startsWith(repo.toLowerCase().replace(/\\+$/, "") + "\\")
          ) {
            alert(
              "Destination must be OUTSIDE the wiped folder — it would be deleted too.",
            );
            return;
          }
          dsts[i] = dir;
          $("backupDst" + i).value = dir;
          $("backupDst" + i).title = dir;
        };
      });
    })().catch(() => done(null));
  });
}

async function doInstall(_installed, mode, opts) {
  $("reinstallChoice").classList.add("hidden");
  // Checklist verdict consumed — hide it so it can't be re-dispatched mid-install.
  _targetChoiceMode = null;
  _verdictModeShown = null;
  _installRunning = true;
  if ($("targetChoiceList")) $("targetChoiceList").style.display = "none";
  installProgressReset();
  if (mode === "skip") {
    // Reuse must earn it: a stale envs.json or half-deleted venv used to sail
    // through to a broken dashboard. Validate first, offer repair on failure.
    appendLog("[*] Checking existing install health before reuse…");
    let v = null;
    try {
      v = await window.w2gp.validateInstall();
    } catch (e) {
      v = { ok: false, errors: [e.message || String(e)] };
    }
    if (v && v.ok) {
      appendLog("[*] Existing install healthy — reusing.");
      _installRunning = false;
      show("dashboard");
      refreshDashboard();
      return;
    }
    appendLog(
      "[!] Existing install failed checks: " +
        ((v && v.errors && v.errors.join("; ")) || "unknown"),
    );
    $("installSubtitle").textContent =
      "Existing install needs repair — see issues above";
    _installRunning = false;
    try {
      await refreshTargetVerdict();
    } catch {}
    showToast(
      "✗ Existing install failed health checks — repair instead of reusing",
    );
    return;
  }
  const _diBtn = $("installStartBtn");
  if (_diBtn) {
    _diBtn.disabled = true;
    _diBtn.textContent = "Installing…";
  }
  document
    .querySelectorAll(".env-type-btn")
    .forEach((b) => (b.disabled = true));
  $("installSubtitle").textContent = "Starting installer — progress below…";
  let skipClone = false;
  if (mode === "reinstall") {
    $("installSubtitle").textContent = "Removing existing installation...";
    appendLog(
      "[*] Removing existing Wan2GP installation (large folders can take several minutes — wait for the next line)…",
    );
    const ok = await window.w2gp.reinstall(opts || null);
    if (ok && ok.movedModels && ok.movedModels.length) {
      appendLog("[*] Models relocated: " + ok.movedModels.join("; "));
      // Adopt the new locations so wgp_config.json points at them post-install.
      for (const m of (opts && opts.moveModels) || []) {
        if (m.type === "ckpts") _modelCkpts = m.to;
        else if (m.type === "loras") _modelLoras = m.to;
        else if (m.type === "output") _modelOutput = m.to;
      }
    }
    if (!ok) {
      appendLog(
        "[!] Reinstall aborted — the existing installation could not be removed (files likely locked by a running process or a terminal open in the folder).",
      );
      appendLog(
        "[!] Close any terminal/Explorer window open in the Wan2GP folder, then retry.",
      );
      showToast("✗ Could not remove existing installation");
      $("installSubtitle").textContent = "Setup Wan2GP";
      $("envTypeSelect").classList.remove("disabled");
      document
        .querySelectorAll(".env-type-btn")
        .forEach((b) => (b.disabled = false));
      $("installStartBtn").classList.remove("hidden");
      const _rbBtn = $("installStartBtn");
      if (_rbBtn) {
        _rbBtn.disabled = false;
        _rbBtn.textContent = "Install";
      }
      _installRunning = false;
      return;
    }
  } else if (mode === "update") {
    $("installSubtitle").textContent = "Update instead of fresh install...";
    skipClone = true;
  } else {
    // Fresh install (startInstall passes no mode): clone the repo normally —
    // previously this branch treated fresh installs as updates, showing
    // "Update instead of fresh install..." and marking the clone task done
    // before it had even run.
    skipClone = false;
  }
  if (skipClone) {
    taskComplete("clone");
    prevPhaseId = "clone";
  } else {
    taskStart("clone");
    prevPhaseId = "clone";
    appendLog("[*] Cloning Wan2GP repository...");
  }
  try {
    appendLog(
      "[*] Installing Wan2GP (environment: " + selectedEnvType + ")...",
    );
    await window.w2gp.install(selectedEnvType);
    try {
      const gpu = await window.w2gp.detectGpu();
      const hw = await window.w2gp.detectHardware();
      const name = (gpu.name || hw.gpu || "").toUpperCase();
      const vendor = gpu.vendor || "";
      let profile = "STANDARD";
      if (vendor === "APPLE") profile = "MPS";
      else if (name.match(/RTX 50|50\d0/)) profile = "RTX 50";
      else if (name.match(/RTX 40|40\d0/)) profile = "RTX 40";
      else if (name.match(/RTX 30|30\d0/)) profile = "RTX 30";
      else if (name.match(/RTX 20|20\d0/)) profile = "RTX 20";
      else if (name.includes("GTX") || name.includes("10")) profile = "GTX 10";
      else if (vendor === "AMD") profile = "AMD";
      $("installProfile").textContent = profile;
      $("installProfileRow").style.display = "flex";
    } catch {}
    try {
      // Fresh reinstall wiped the repo — merge the .reinstall-backup back first
      // (custom plugins/finetunes/old settings), then apply model paths on top.
      if (mode === "reinstall") {
        try {
          const rb = await window.w2gp.restoreBackup();
          if (rb && rb.restored && rb.restored.length)
            appendLog("[*] Restored from backup: " + rb.restored.join(", "));
        } catch (e) {
          appendLog(
            "[!] Backup restore failed: " +
              e.message +
              " — files remain in .reinstall-backup",
          );
        }
      }
      const modelCfg = {};
      if (_modelCkpts) modelCfg.checkpointsPaths = [_modelCkpts, "."];
      if (_modelLoras) modelCfg.lorasRoot = _modelLoras;
      if (_modelOutput) modelCfg.savePath = _modelOutput;
      await window.w2gp.writeWgpConfig(modelCfg);
      appendLog(
        `[*] wgp_config.json updated: ckpts=${_modelCkpts || "(default)"}, loras=${_modelLoras || "(default)"}`,
      );
    } catch (e) {
      appendLog(`[!] Failed to write model config: ${errText(e)}`);
    }
    taskComplete("done");
    $("installSubtitle").textContent = "Wan2GP is ready!";
    appendLog("[*] Installation complete!");
    _installRunning = false;
    const vb = $("validateInstallBtn");
    if (vb) {
      vb.style.display = "";
      vb.disabled = false;
      vb.textContent = "Validate installation";
    }
    setTimeout(() => {
      show("dashboard");
      refreshDashboard();
      startMetricsPolling();
    }, 1200);
  } catch (e) {
    // Honest failure: fail any still-running task, offer Retry + diagnostics.
    // (Previously only 'done' was marked and the Install button stayed hidden.)
    taskComplete("done", true);
    document.querySelectorAll(".task.active").forEach((t) => {
      t.className = "task fail";
      const ic = t.querySelector(".task-icon");
      if (ic) ic.textContent = "✕";
      const st = t.querySelector(".task-status");
      if (st) st.textContent = "failed";
    });
    $("installSubtitle").textContent =
      "Installation failed — see console output above";
    appendLog(`[ERROR] ${(e && e.message) || e}`);
    const sb = $("installStartBtn");
    if (sb) {
      sb.classList.remove("hidden");
      sb.disabled = false;
      sb.textContent = "Retry install";
    }
    const cdb = $("copyDiagnosticsBtn");
    if (cdb) {
      cdb.style.display = "";
      cdb.onclick = copyDiagnostics;
    }
    _installRunning = false;
    showToast("✗ Install failed — fix the issue above, then Retry");
  }
}

// Model-drive disk gates: checkpoints/LoRAs/outputs may live on other drives
// (or the same one) — the app-drive check in the install stack doesn't cover
// them, and a model library eats tens–hundreds of GB. Warn <50 GB, block <10.
function driveRootOf(p) {
  const m = /^([A-Za-z]:\\)/.exec(p || "");
  return m ? m[1].toUpperCase() : p || "";
}
let _gatesRun = 0;
async function refreshModelDiskGates() {
  const box = $("modelDiskGates");
  if (!box) return;
  const my = ++_gatesRun; // superseded runs abort before painting
  const targets = [
    ["Checkpoints", _modelCkpts],
    ["LoRAs", _modelLoras],
    ["Output", _modelOutput],
  ].filter((t) => t[1] && !isDriveRoot(t[1]));
  if (!targets.length) {
    if (my === _gatesRun) {
      box.innerHTML = "";
      window._modelDriveBlocked = false;
    }
    return;
  }
  const seen = new Set();
  const frag = document.createDocumentFragment();
  let blocked = false;
  for (const [label, p] of targets) {
    const root = driveRootOf(p);
    if (seen.has(root)) continue;
    seen.add(root);
    let d = null;
    try {
      d = await window.w2gp.getDiskSpace(p);
    } catch {
      continue;
    }
    if (my !== _gatesRun) return;
    if (!d || d.free == null || d.total == null) continue;
    const gb = d.free / 1073741824;
    if (gb < 10) {
      blocked = true;
      const d = document.createElement("div");
      d.className = "istack-w";
      d.textContent =
        "⛔ " +
        label +
        " drive " +
        root +
        " has only " +
        gb.toFixed(1) +
        " GB free — a model library needs tens of GB. Pick a roomier drive.";
      frag.append(d);
    } else if (gb < 50) {
      const d = document.createElement("div");
      d.className = "istack-hint";
      d.textContent =
        "⚠ " +
        label +
        " drive " +
        root +
        ": " +
        gb.toFixed(1) +
        " GB free — tight for a model library.";
      frag.append(d);
    }
  }
  if (my !== _gatesRun) return;
  box.textContent = "";
  box.append(frag);
  window._modelDriveBlocked = blocked;
  const startBtn = $("installStartBtn");
  if (blocked && startBtn) {
    startBtn.disabled = true;
    startBtn.title = "Free space on the model drive(s) before installing";
    startBtn.textContent = "Install blocked — model drive full";
  }
}

// Copy a diagnostics bundle (hardware + paths + python preflight + log tail)
// for Discord/GitHub reports — ATFGriff-class issues arrive without this.
async function copyDiagnostics() {
  let info = "";
  try {
    const parts = await Promise.all([
      window.w2gp.detectHardware().catch(() => null),
      window.w2gp.getInstallPaths().catch(() => null),
      window.w2gp.pythonPreflight().catch(() => null),
      window.w2gp.classifyTarget().catch(() => null),
    ]);
    info =
      "Hardware: " +
      JSON.stringify(parts[0]) +
      "\nPaths: " +
      JSON.stringify(parts[1]) +
      "\nPython: " +
      JSON.stringify(parts[2]) +
      "\nTarget: " +
      JSON.stringify(parts[3]) +
      "\n\n";
  } catch {}
  const tail =
    typeof window._getLogTail === "function" ? window._getLogTail() : "";
  try {
    await navigator.clipboard.writeText(info + tail);
    showToast("✓ Diagnostics copied — paste it in Discord/GitHub");
  } catch {
    showToast("✗ Copy failed — select the console text manually");
  }
}

// Target-folder triage UI: what is already in the install location?
// Verdicts from classify_target: empty | ours_healthy | ours_broken_env |
// repo_no_env | pinokio | foreign.
async function refreshTargetVerdict() {
  const box = $("targetVerdict"),
    body = $("targetVerdictBody");
  const choiceList = $("targetChoiceList"),
    browse = $("targetBrowseBtn"),
    useModels = $("targetUseModelsBtn");
  if (!box || !body) return;
  let t = null;
  try {
    t = await window.w2gp.classifyTarget();
  } catch {
    _lastVerdict = null;
    box.style.display = "none";
    return null;
  }
  if (!t || !t.verdict) {
    _lastVerdict = null;
    box.style.display = "none";
    return null;
  }
  const v = t.verdict;
  _lastVerdict = v;
  const newMode =
    v === "ours_healthy"
      ? "reinstall-trio"
      : v === "repo_no_env" || v === "ours_broken_env"
        ? "repair-or-fresh"
        : null;
  // Only force-check the default radio when the verdict MODE changed — a
  // same-mode refresh (Browse re-triage) leaves the user's pick alone.
  const verdictModeChanged = newMode !== _verdictModeShown;
  _verdictModeShown = newMode;
  if (choiceList) choiceList.style.display = "none";
  if (browse) browse.style.display = "none";
  if (useModels) useModels.style.display = "none";
  _targetChoiceMode = null;
  const startBtn = $("installStartBtn");
  if (v === "empty") {
    box.style.display = "none";
    // No install here → no Keep / reuse choices either.
    $("reinstallChoice")?.classList.add("hidden");
    // Restore the label if a previous verdict changed it (pinokio/foreign only —
    // never touch driver/disk hard blocks, the installPlan block owns those).
    if (
      startBtn &&
      (startBtn.textContent.startsWith("Install anyway") ||
        startBtn.textContent.startsWith("Install blocked — Pinokio"))
    ) {
      startBtn.textContent = "Install";
      startBtn.disabled = false;
      startBtn.title = "";
    }
    return t;
  }
  box.style.display = "";
  const envNames = (t.envs && Object.keys(t.envs).join(", ")) || "";
  if (v === "ours_healthy") {
    body.textContent = "";
    {
      const d = document.createElement("div");
      d.className = "istack-ok";
      d.textContent = "✓ " + (t.hint || "");
      body.append(d);
    }
    // Reuse path: the choice lives in the trio radios, the big Install
    // button dispatches it — same contract as the no-env checklist.
    $("reinstallChoice")?.classList.remove("hidden");
    const trio = document.querySelector(
      'input[name="reinstallChoice"][value="update"]',
    );
    if (verdictModeChanged && trio) trio.checked = true;
    _targetChoiceMode = "reinstall-trio";
    if (startBtn && !_installRunning) {
      startBtn.classList.remove("hidden");
      if (!startBtn.disabled) {
        startBtn.textContent = "Install";
        startBtn.title = "";
      }
    }
    $("installSubtitle").textContent = "Wan2GP is already installed.";
  } else if (v === "repo_no_env" || v === "ours_broken_env") {
    // Checklist mode: the choice lives in the radios, the big Install button
    // dispatches it (see startInstall) — no competing action buttons.
    // Adopt-cover note: the generic Keep/Update/Skip trio doesn't apply.
    $("reinstallChoice")?.classList.add("hidden");
    body.textContent = "";
    {
      const d = document.createElement("div");
      d.className = "istack-w";
      d.textContent = "⚠ " + (t.hint || "");
      body.append(d);
    }
    if (envNames) {
      const d = document.createElement("div");
      d.className = "istack-hint";
      d.textContent =
        "Env folders found: " +
        envNames +
        " — repair recreates the broken one, keeps models & settings.";
      body.append(d);
    }
    {
      const d = document.createElement("div");
      d.className = "istack-hint";
      d.textContent = "Tick your choice below, then press Install.";
      body.append(d);
    }
    if (choiceList) {
      choiceList.style.display = "";
      const repair = choiceList.querySelector('input[value="repair"]');
      if (verdictModeChanged && repair) repair.checked = true;
    }
    _targetChoiceMode = "repair-or-fresh";
    // Re-arm the big button (the first Install pass hid it to show this card).
    // Never override a hard block owned elsewhere (disk gates, pinokio, roots),
    // and never resurrect it while an install is running.
    if (startBtn && !_installRunning) {
      startBtn.classList.remove("hidden");
      if (!startBtn.disabled) {
        startBtn.textContent = "Install";
        startBtn.title = "";
      }
    }
    $("installSubtitle").textContent =
      "Wan2GP repo found — environment missing or broken.";
  } else {
    // pinokio | foreign
    const isPinokio = v === "pinokio";
    const icon = isPinokio ? "🧩" : "⚠";
    let detail =
      "Folder: " + (t.repo || "") + " (" + (t.entryCount || 0) + " entries";
    if (t.hasConfig) detail += ", has wgp_config.json";
    if (t.modelDirs && t.modelDirs.length)
      detail += ", model dirs: " + t.modelDirs.join(", ");
    detail += ").";
    // For Pinokio libraries, show what we found + sizes (proves reuse is worth it).
    if (isPinokio && t.modelDirs && t.modelDirs.length) {
      try {
        const sz = await window.w2gp.folderSize(t.repo).catch(() => null);
        const byName = {};
        for (const e of (sz && sz.entries) || [])
          byName[e.name.toLowerCase()] = e.bytes;
        const lines = t.modelDirs.map((d) => {
          const b = byName[d.toLowerCase()];
          return d + (b == null ? "" : " (" + fmtBytes(b) + ")");
        });
        detail += " Reusable library: " + lines.join(", ") + ".";
      } catch {}
    }
    body.textContent = "";
    {
      const d = document.createElement("div");
      d.className = "istack-w";
      d.textContent = icon + " " + (t.hint || "");
      body.append(d);
    }
    {
      const d = document.createElement("div");
      d.className = "istack-hint";
      d.textContent =
        detail +
        (isPinokio
          ? ""
          : " Installing here merges upstream over unknown files — an empty folder is safer.");
      body.append(d);
    }
    if (browse) {
      browse.style.display = "";
      browse.onclick = () => {
        $("browseAppDataPath")?.click();
      };
    }
    // Foreign content → no Keep / reuse choices either.
    $("reinstallChoice")?.classList.add("hidden");
    if (isPinokio) {
      // Hard stop: backend install/reinstall/uninstall refuse Pinokio trees too.
      if (startBtn) {
        startBtn.disabled = true;
        startBtn.title =
          "Pick an empty folder first — installing into a Pinokio tree would corrupt it.";
        startBtn.textContent = "Install blocked — Pinokio folder";
      }
      $("reinstallChoice")?.classList.add("hidden");
      if (useModels && t.modelDirs && t.modelDirs.length) {
        useModels.style.display = "";
        useModels.onclick = () => {
          usePinokioModels(t);
        };
      }
      $("installSubtitle").textContent =
        "Pinokio-managed Wan2GP found — reuse its models in a fresh install below.";
    } else if (startBtn && !startBtn.disabled) {
      startBtn.textContent = "Install anyway (folder not empty)";
    }
    if (v === "foreign")
      $("installSubtitle").textContent =
        "This folder holds unknown files — install into an empty folder, or wipe it first.";
  }
  return t;
}

// Pinokio reuse: point OUR model folders at the Pinokio library (no re-downloads,
// Pinokio keeps working untouched), then let the user pick an empty install folder.
// NOTE: writes desktop-config only — wgp_config.json here belongs to Pinokio.
async function usePinokioModels(t) {
  const repo = (t && t.repo) || "";
  const sep = "\\";
  const pick = async (type, sub) => {
    const p = repo.replace(/\\+$/, "") + sep + sub;
    setModelPath(type, p);
    try {
      const cfg = await window.w2gp.configLoad();
      if (type === "ckpts") cfg.modelCkptsPath = p;
      else if (type === "loras") cfg.modelLorasPath = p;
      else cfg.modelOutputPath = p;
      await window.w2gp.configSave(cfg);
    } catch {}
  };
  const dirs = (t && t.modelDirs) || [];
  // Map upstream subdir names to our folder types.
  for (const d of dirs) {
    const low = d.toLowerCase();
    if (low === "ckpts" || low === "checkpoints") await pick("ckpts", d);
    else if (low === "loras") await pick("loras", d);
    else if (low === "outputs" || low === "output") await pick("output", d);
  }
  appendLog(
    "[*] Model folders now point at the Pinokio library — its install stays untouched.",
  );
  showToast("✓ Reusing Pinokio models — now pick an empty install folder");
  try {
    await refreshModelDiskGates();
  } catch {}
  $("browseAppDataPath")?.click();
}

// (Re)open the installer screen in a fresh state — used by Manage → Run Setup
// again and after uninstall. Never lands on the dashboard without an install.
async function openInstallerFresh(subtitle) {
  resetTasks();
  installProgressReset();
  show("installer");
  $("installSubtitle").textContent =
    subtitle || "Select environment type, then click Install";
  const sb = $("installStartBtn");
  if (sb) {
    sb.classList.remove("hidden");
    sb.disabled = false;
    sb.textContent = "Install";
  }
  $("envTypeSelect")?.classList.remove("disabled");
  document
    .querySelectorAll(".env-type-btn")
    .forEach((b) => (b.disabled = false));
  await loadPaths().catch(() => null);
  try {
    await refreshTargetVerdict().catch(() => null);
  } catch {}
  try {
    await refreshModelDiskGates().catch(() => null);
  } catch {}
}

$("manageRunSetupBtn")?.addEventListener("click", async () => {
  closeSettings();
  appendLog(
    "[*] Opening Setup — pick fresh install, repair, reuse or migrate.",
  );
  await openInstallerFresh();
});

$("settingsOverlay").addEventListener("click", () => {
  closeSettings();
  // closeGuide is defined below the overlay wiring — guard for load order.
  if (typeof closeGuide === "function") closeGuide();
});

// migration-tab.js — Migration folder-chooser modal.
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

// ── Migration folder-chooser modal ──
// Opens a dialog pre-filled with our recommended targets (data dir + checkpoints
// + LoRAs + output). The user can override any of them, then "Move & restart"
// calls migrate-to-preferred with the chosen paths. After the move, main rewrites
// wgp_config.json model paths and relaunches.
let _migBusy = false;
async function openMigrationModal() {
  if (_migBusy) return;
  let prefs;
  try {
    prefs = await window.w2gp.migrateChoose();
  } catch {
    prefs = null;
  }
  if (!prefs) {
    alert("Could not determine migration targets.");
    return;
  }
  window._migPrefs = prefs;
  $("migDataDir").value = prefs.dataDir || "";
  $("migDataDir").title = prefs.dataDir || "";
  $("migCkpts").value = prefs.ckpts || "";
  $("migCkpts").title = prefs.ckpts || "";
  $("migLoras").value = prefs.loras || "";
  $("migLoras").title = prefs.loras || "";
  $("migOutput").value = prefs.output || "";
  $("migOutput").title = prefs.output || "";
  // Context-aware copy: the modal is reused both for the first migration out of
  // a roaming AppData profile AND for later re-location of an already-migrated
  // install (e.g. C:\Wan2GP → D:\Wan2GP). Don't claim "AppData" when it isn't.
  const roaming = !!prefs.fromRoaming;
  const cur = prefs.legacy || "";
  const title = $("migrationTitle");
  const sub = $("migrationSub");
  if (title)
    title.textContent = roaming
      ? "Move Wan2GP out of AppData"
      : "Move Wan2GP to a new location";
  if (sub) {
    sub.textContent = roaming
      ? "Your Wan2GP data currently lives in your roaming AppData profile. Move it to a dedicated, fast drive — AppData is meant for small settings, not multi-GB model checkpoints (it can slow logins, trigger antivirus locks, and bloat your profile). Our recommended locations are pre-filled — change any of them if you like."
      : "Your Wan2GP is currently at " +
        cur +
        ". Move it to a different drive or folder — your repo, venv, settings, and model folders travel with it. The recommended location is pre-filled — change it if you like.";
  }
  // Reset to idle state (in case a previous attempt left the progress UI showing).
  _migBusy = false;
  const btn = $("migrationMoveBtn");
  if (btn) {
    btn.disabled = false;
    btn.textContent = "Move & restart";
  }
  const prog = $("migrationProgress");
  if (prog) {
    prog.classList.add("hidden");
    const f = $("migrationProgressFill");
    if (f) f.style.width = "0%";
  }
  $("migrationModal").classList.remove("hidden");
}
$("migrationCloseBtn")?.addEventListener("click", () =>
  $("migrationModal").classList.add("hidden"),
);
$("migrationCancelBtn")?.addEventListener("click", () =>
  $("migrationModal").classList.add("hidden"),
);
// Browse buttons inside the modal pick a folder for the matching field.
document.querySelectorAll("#migrationModal [data-browse]").forEach((btn) => {
  btn.addEventListener("click", async () => {
    const key = btn.getAttribute("data-browse");
    const field = {
      dataDir: "migDataDir",
      ckpts: "migCkpts",
      loras: "migLoras",
      output: "migOutput",
    }[key];
    try {
      const picked = await window.w2gp.selectFolder();
      if (picked) {
        $(field).value = picked;
        $(field).title = picked;
      }
    } catch {}
  });
});
async function runMigration(doMove) {
  if (_migBusy) return;
  // The two modal buttons own the choice now (Move vs Just-switch) — no
  // second popup. Same folder ⇒ nothing to move regardless of button.
  const newDataDir = $("migDataDir").value.trim();
  // ponytail: reject file-as-folder
  if (isFilePickedAsFolder(newDataDir)) {
    alert("Please select a folder, not a file:\n" + newDataDir);
    resetMigrationUI();
    return;
  }
  const oldDataDir =
    (window._migPrefs &&
      (window._migPrefs.dataDir || window._migPrefs.legacy)) ||
    (await window.w2gp.getInstallPaths().catch(() => null))?.dataDir ||
    "";
  if (
    newDataDir &&
    oldDataDir &&
    newDataDir.toLowerCase() === oldDataDir.toLowerCase()
  )
    doMove = false;
  _migBusy = true;
  const moveBtn = $("migrationMoveBtn"),
    pointBtn = $("migrationPointBtn");
  if (moveBtn) moveBtn.disabled = true;
  if (pointBtn) pointBtn.disabled = true;
  const btn = (doMove ? moveBtn : pointBtn) || moveBtn;
  btn.textContent = doMove ? "Moving…" : "Switching…";
  const prog = $("migrationProgress");
  if (prog) {
    prog.classList.remove("hidden");
    setMigrationProgress(0);
  }
  const choices = {
    dataDir: newDataDir,
    ckpts: $("migCkpts").value,
    loras: $("migLoras").value,
    output: $("migOutput").value,
  };
  if (!choices.dataDir) {
    alert("Choose a Wan2GP data folder.");
    resetMigrationUI();
    return;
  }
  // Bare drive root ⇒ resolve to <root>\Wan2GP like the installer Browse does.
  if (isDriveRoot(choices.dataDir)) {
    choices.dataDir = pathJoin(choices.dataDir, "Wan2GP");
    $("migDataDir").value = choices.dataDir;
    appendLog(
      "[*] Drive root selected — using " + choices.dataDir + " instead.",
    );
  }
  try {
    // ponytail: 4 modes for Wan2GP folder — existing/new × Move vs Just point
    if (
      doMove &&
      oldDataDir &&
      newDataDir.toLowerCase() !== oldDataDir.toLowerCase()
    ) {
      const r = await window.w2gp.moveFolder(oldDataDir, newDataDir);
      if (!r || (!r.ok && !r.success)) {
        alert(
          "Could not move files:\n" +
            ((r && r.error) || "unknown") +
            "\n\nClose any Wan2GP windows/terminals and try again.",
        );
        resetMigrationUI();
        return;
      }
    }
    const r2 = await window.w2gp.setDataDir(newDataDir);
    if (!r2 || (!r2.ok && !r2.success)) {
      alert(
        "Could not switch data folder:\n" + ((r2 && r2.error) || "unknown"),
      );
      resetMigrationUI();
      return;
    }
    // persist model folder overrides if changed (no move, just point — like changeModelFolder "Just point")
    const ck = $("migCkpts").value.trim(),
      lo = $("migLoras").value.trim(),
      out = $("migOutput").value.trim();
    const patch = {};
    if (ck && ck !== (window._migPrefs?.ckpts || ""))
      patch.checkpointsPaths = [ck, "."];
    if (lo && lo !== (window._migPrefs?.loras || "")) patch.lorasRoot = lo;
    if (out && out !== (window._migPrefs?.output || "")) patch.savePath = out;
    if (Object.keys(patch).length) await window.w2gp.writeWgpConfig(patch);
    btn.textContent = "Restarting…";
    setTimeout(() => location.reload(), 900);
  } catch (e) {
    alert("Migration failed: " + errText(e));
    resetMigrationUI();
  }
}
$("migrationMoveBtn")?.addEventListener("click", () => runMigration(true));
$("migrationPointBtn")?.addEventListener("click", () => runMigration(false));
// Show live copy progress (only the slow cross-volume/copy-fallback path emits
// this — the common instant rename path finishes before any paint).
function setMigrationProgress(pct) {
  const fill = $("migrationProgressFill");
  if (fill) fill.style.width = pct + "%";
  const txt = $("migrationProgressText");
  if (txt) txt.textContent = "Moving… " + pct + "%";
}
window.w2gp.onMigrationProgress?.(setMigrationProgress);
// Restore the modal to its idle state (re-enable button, hide progress).
function resetMigrationUI() {
  _migBusy = false;
  const btn = $("migrationMoveBtn");
  if (btn) {
    btn.disabled = false;
    btn.textContent = "Move & restart";
  }
  const pt = $("migrationPointBtn");
  if (pt) {
    pt.disabled = false;
    pt.textContent = "Just switch to it";
  }
  const prog = $("migrationProgress");
  if (prog) {
    prog.classList.add("hidden");
    setMigrationProgress(0);
  }
}
// Startup prompt (main process) asks the renderer to open this modal.
window.w2gp.onOpenMigration?.(() => openMigrationModal());

// Re-entrancy guard: periodic + manual checks share one flight; a slow GitHub
// response can't stack overlapping fetches.
let _wangpCheckBusy = false;
// ponytail: upstream fetch spawns curl/powershell to GitHub (~1-5s) — cache 5 min
// instead of re-hitting the network on every refreshDashboard.
let _upstreamAt = 0,
  _upstreamData = null;
// ponytail: llmEnginesList spawns 3x where + a cold venv python per call, and
// refreshLLMEngines + refreshDeepy each called it per refresh — one shared flight.
let _llmEnginesPromise = null;
function getLLMEngines() {
  if (!_llmEnginesPromise)
    _llmEnginesPromise = window.w2gp
      .llmEnginesList()
      .catch(() => ({ engines: [] }));
  return _llmEnginesPromise;
}
async function loadWangpChangelog(showLoading) {
  const localEl = $("localCommit");
  const listEl = $("updatesList");
  const verEl = $("wangpVersion");
  if (!listEl) return;
  if (_wangpCheckBusy) return;
  _wangpCheckBusy = true;
  try {
    if (showLoading)
      listEl.innerHTML =
        '<div class="changelog-loading">Checking for updates...</div>';

    const local = await window.w2gp.getWangpLocalVersion();
    if (local && localEl)
      localEl.textContent = local.hash ? local.hash.substring(0, 7) : "";

    window.w2gp.getWangpVersion().then((v) => {
      if (v && verEl) verEl.textContent = v;
    });

    const upstream =
      Date.now() - _upstreamAt < 5 * 60 * 1000 && _upstreamData
        ? _upstreamData
        : await window.w2gp.getWangpUpstreamInfo().then((u) => {
            if (u && u.commits) {
              _upstreamAt = Date.now();
              _upstreamData = u;
            }
            return u;
          });
    if (!upstream || !upstream.commits) {
      // A transient upstream failure on the silent periodic poll must not
      // clobber a previously rendered changelog — show the error only on an
      // explicit user check.
      if (showLoading)
        listEl.innerHTML =
          '<div class="changelog-error">Could not fetch updates</div>';
      // Clear any stale green dot from a previous check — don't leave it dangling
      const updateBtn = $("updateBtn");
      if (updateBtn) {
        updateBtn.classList.remove("has-update");
        updateBtn.querySelector(".update-dot")?.remove();
      }
      return;
    }

    const updateBtn = $("updateBtn");
    // Hash equality alone never clears: any local merge commit (every
    // non-fast-forward `git pull` manufactures one) differs from the tip
    // forever. When the hashes differ, ask git whether the tip is already
    // merged here before lighting the dot.
    const tip = upstream.commits[0]?.hash;
    let hasUpdate = !!(local && tip && tip !== local.hash);
    if (hasUpdate) {
      try {
        const c = await window.w2gp.wangpContainsCommit(tip);
        if (c && c.contained === true) hasUpdate = false;
      } catch {}
    }
    if (hasUpdate) {
      updateBtn?.classList.add("has-update");
      if (!updateBtn?.querySelector(".update-dot")) {
        const dot = document.createElement("span");
        dot.className = "update-dot";
        updateBtn.appendChild(dot);
      }
    } else {
      updateBtn?.classList.remove("has-update");
      updateBtn?.querySelector(".update-dot")?.remove();
    }

    listEl.textContent = "";
    for (const c of upstream.commits) {
      const item = document.createElement("div");
      item.className = "cl-item";
      const dt = document.createElement("span");
      dt.className = "cl-date";
      dt.textContent = fmtDate(c.date);
      const msg = document.createElement("span");
      msg.className = "cl-msg";
      msg.textContent = c.message;
      const au = document.createElement("span");
      au.className = "cl-author";
      au.textContent = c.author;
      item.append(dt, msg, au);
      listEl.append(item);
    }
  } finally {
    _wangpCheckBusy = false;
  }
}

function fmtDate(s) {
  if (!s) return "";
  const d = new Date(s);
  const days = (Date.now() - d) / 864e5;
  if (days < 1) return "today";
  if (days < 2) return "yesterday";
  return days < 7
    ? `${Math.floor(days)}d ago`
    : d.toLocaleDateString("en-US", { month: "short", day: "numeric" });
}

document.addEventListener("DOMContentLoaded", () => {
  $("wangpCheckLink")?.addEventListener("click", (e) => {
    e.preventDefault();
    loadWangpChangelog(true);
  });
  $("changelogLink")?.addEventListener("click", (e) => {
    e.preventDefault();
    window.w2gp.openExternal(
      "https://github.com/deepbeepmeep/Wan2GP/blob/main/docs/CHANGELOG.md",
    );
  });
  $("hfModelsLink")?.addEventListener("click", (e) => {
    e.preventDefault();
    window.w2gp.openExternal("https://huggingface.co/DeepBeepMeep");
  });
});

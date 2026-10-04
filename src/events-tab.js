// events-tab.js — Dashboard event wiring.
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

// ── Event Wiring: Dashboard ──
$("updateBtn").addEventListener("click", async () => {
  $("updateBtn").disabled = true;
  $("updateBtn").textContent = "Working...";
  try {
    const r = await window.w2gp.update();
    appendLog("[*] Wan2GP update complete");
    if (r && r.requirements === "reinstalled") {
      appendLog("[*] requirements.txt changed — pinned packages reinstalled");
      const pd = r && r.pinDiff;
      if (Array.isArray(pd) && pd.length) appendLog("    " + pd.join("\n    "));
    } else if (r && r.requirements === "failed")
      appendLog(
        "[!] requirements reinstall failed — see the console output above; the git pull itself stays applied.",
      );
    if (
      r &&
      r.depCheck === "drift" &&
      Array.isArray(r.drift) &&
      r.drift.length
    ) {
      appendLog(
        "[!] dependency drift: " + r.drift.join(", ") + " — use restore",
      );
      showToast(
        "[!] Dependency drift — packages missing or outdated. See the red banner.",
      );
      showDriftBanner(r.drift);
    } else {
      // Clean update (or no drift info) clears any stale banner from an
      // earlier drift run — otherwise Restore/Dismiss linger confusingly.
      hideDriftBanner();
    }
    // Launcher-compat verify pass (backend): non-empty means the update
    // pulled a setup_config shape this launcher doesn't fully understand.
    if (r && Array.isArray(r.compat) && r.compat.length) {
      appendLog("[!] launcher compat: " + r.compat.join(" | "));
      showToast("[!] Upstream changed setup — see Console, then Sync GPU Wheels");
    }
    refreshDashboard();
  } catch (e) {
    appendLog("[!] Update failed: " + errText(e));
    alert("Update: " + errText(e));
  }
  $("updateBtn").disabled = false;
  $("updateBtn").textContent = "↻ Update Wan2GP (DeepBeepMeep)";
});
$("repairFilesBtn")?.addEventListener("click", async () => {
  const btn = $("repairFilesBtn");
  if (btn) btn.disabled = true;
  try {
    const v = await window.w2gp.verifyWangpFiles().catch((e) => ({
      error: errText(e),
    }));
    if (v && v.error) {
      showToast("✗ Verify failed: " + v.error);
    } else if (!v || v.clean) {
      const untracked =
        v && v.untracked
          ? " (" + v.untracked + " untracked user file(s) left alone)"
          : "";
      showToast("✓ Wan2GP files match upstream.");
      appendLog("[*] Verify: tracked files clean" + untracked + ".");
    } else {
      const dirty = v.dirty || [];
      const total = v.dirtyTotal || dirty.length;
      const names = dirty
        .slice(0, 10)
        .map((d) => (d.path || "?") + " [" + (d.kind || "?") + "]")
        .join("\n");
      const more =
        total > dirty.length ? "\n…+" + (total - dirty.length) + " more" : "";
      const ok = confirm(
        total +
          " tracked Wan2GP file(s) differ from upstream:\n" +
          names +
          more +
          "\n\nRepair restores them (your edits are stashed recoverably; settings/models untouched). Continue?",
      );
      if (ok) {
        appendLog("[*] Repairing Wan2GP files…");
        const r = await window.w2gp.repairWangpFiles();
        if (r && (r.ok || r.repaired)) {
          showToast("✓ Wan2GP files repaired — restart Wan2GP to run them.");
          appendLog(
            "[*] Repair done" +
              (r.stashed ? " (prior edits stashed, recoverable via git)" : "") +
              ".",
          );
        } else showToast("✗ Repair failed: " + ((r && r.error) || "unknown"));
        refreshDashboard();
      }
    }
  } catch (e) {
    showToast("✗ " + errText(e));
  }
  if (btn) btn.disabled = false;
});
$("rollbackBtn")?.addEventListener("click", async () => {
  const btn = $("rollbackBtn");
  if (btn) btn.disabled = true;
  try {
    const v = await window.w2gp.verifyWangpFiles().catch((e) => ({
      error: errText(e),
    }));
    if (v && v.error) {
      showToast("✗ Rollback check failed: " + v.error);
    } else {
      const pin = (v && v.pin) || null;
      const head = (v && v.head) || null;
      if (!pin || !pin.hash) {
        showToast("No recorded Wan2GP update yet — update once first.");
      } else if (head && pin.hash.slice(0, head.length) === head) {
        showToast("✓ Already at the recorded update (" + head + ").");
        appendLog("[*] Rollback: already at " + head + ", nothing to do.");
      } else if (v && !v.clean) {
        showToast(
          "Tracked files differ — Verify/Repair (or stash) first, then roll back.",
        );
      } else {
        const when = pin.date ? " (" + pin.date + ")" : "";
        const ok = confirm(
          "Roll back Wan2GP to the recorded update?\n" +
            pin.hash.slice(0, 8) +
            when +
            " → HEAD is " +
            (head || "unknown") +
            "\n\nUntracked files (settings/models) untouched. Continue?",
        );
        if (ok) {
          appendLog("[*] Rolling back Wan2GP…");
          const r = await window.w2gp.rollbackWangp();
          if (r && (r.ok || r.rolledBack)) {
            showToast(
              "✓ Rolled back to " +
                ((r && r.commit) || "recorded update") +
                " — restart Wan2GP.",
            );
            appendLog(
              "[*] Rolled back to " +
                ((r && r.commit) || "recorded update") +
                ".",
            );
          } else
            showToast("✗ Rollback failed: " + ((r && r.error) || "unknown"));
          refreshDashboard();
        }
      }
    }
  } catch (e) {
    showToast("✗ " + errText(e));
  }
  if (btn) btn.disabled = false;
});


function switchSettingsTab(tabName) {
  document.querySelectorAll(".settings-tab").forEach((t) => {
    t.classList.remove("active");
  });
  document.querySelectorAll(".settings-tab-content").forEach((c) => {
    c.classList.remove("active");
  });
  var tab = document.querySelector('.settings-tab[data-tab="' + tabName + '"]');
  if (tab) tab.classList.add("active");
  var tabContent = document.querySelector(
    '.settings-tab-content[data-tab="' + tabName + '"]',
  );
  if (tabContent) tabContent.classList.add("active");

  // Plugins tab: fill on demand (openSettings no longer preloads the walk).
  if (tabName === "plugins") {
    setTimeout(() => {
      try {
        refreshPluginsLazy();
      } catch (e) {}
    }, 30);
  }

  // Appearance tab: queue row colors live here (visual pref, not auto-tuned).
  if (tabName === "general") {
    setTimeout(() => {
      try {
        queueColorsLoad();
      } catch {}
    }, 120);
  }

  // Auto-Tune: check if Wan2GP is installed — disable if not
  if (tabName === "autotune") {
    checkAutoTuneInstalled();
    // Saved tags + dropdown seeding — runs on EVERY entry (tab click or
    // dashboard shortcut), not just physical clicks.
    setTimeout(() => {
      try {
        memProfileLoad();
      } catch {}
    }, 120);
  }
}

async function checkAutoTuneInstalled() {
  const installed = await window.w2gp.checkInstalled();
  const notInstalledEl = $("autotuneNotInstalled");
  const contentEl = $("autotuneContent");
  if (!notInstalledEl || !contentEl) return;
  if (installed.repo) {
    notInstalledEl.classList.add("hidden");
    contentEl.classList.remove("hidden");
    // D3: first visit to the tab — auto-run detection so the panel shows a live
    // recommendation instead of an empty "Run detection first" state. Only once
    // per session; a failed detect leaves the button enabled for a manual retry.
    if (!_autotuneHardware && !_autotuneAutoDetectDone) {
      _autotuneAutoDetectDone = true;
      setTimeout(() => $("autotuneDetectBtn")?.click(), 150);
    }
  } else {
    notInstalledEl.classList.remove("hidden");
    contentEl.classList.add("hidden");
  }
}

document.querySelectorAll(".settings-tab").forEach((tab) => {
  tab.addEventListener("click", () => {
    switchSettingsTab(tab.dataset.tab);
  });
});
$("settingsBtn").addEventListener("click", () => {
  openSettings();
});
$("autoTuneDashBtn").addEventListener("click", () => {
  openSettings();
  switchSettingsTab("autotune");
});
// Windows-only UI: hide the Task Manager button on other platforms.
if (window.w2gp && window.w2gp.platform !== "win32") {
  const taskMgrBtn = $("taskMgrBtn");
  if (taskMgrBtn) taskMgrBtn.style.display = "none";
}
$("taskMgrBtn").addEventListener("click", () => {
  window.w2gp.openTaskManager();
});

// uninstall-tab.js — Uninstall Wan2GP (Manage → General → danger section).
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

// ── Uninstall Wan2GP (Manage → General → danger section) ──
// Uninstall via explicit 3-choice modal (the old native OK/Cancel confused:
// Cancel sounded like abort but meant "delete everything").
async function openUninstallModal() {
  const modal = $("uninstallModal");
  if (!modal) return null;
  const rows = $("uninstallModelsRows");
  rows.innerHTML = '<span class="istack-hint">checking model folders…</span>';
  modal.classList.remove("hidden");
  // Fill model folders + sizes so the choice is informed.
  try {
    const mp = await window.w2gp.getModelPaths().catch(() => null);
    const items = [
      ["Checkpoints", mp && mp.checkpoints],
      ["LoRAs", mp && mp.loras],
      ["Output", mp && mp.output],
    ].filter(([, p]) => p && p !== ".");
    if (items.length) {
      rows.innerHTML = "";
      for (const [label, p] of items) {
        let sizeTxt = "";
        try {
          const sz = await window.w2gp.folderSize(p).catch(() => null);
          if (sz && sz.bytes != null) sizeTxt = " (" + fmtBytes(sz.bytes) + ")";
        } catch {}
        const div = document.createElement("div");
        div.className = "istack-row";
        const k = document.createElement("span");
        k.className = "istack-k";
        k.textContent = label;
        const v = document.createElement("span");
        v.className = "istack-v";
        v.textContent = p + sizeTxt;
        div.append(k, v);
        rows.appendChild(div);
      }
    } else {
      rows.innerHTML =
        '<span class="istack-hint">No separate model folders configured.</span>';
    }
  } catch {}
  return new Promise((resolve) => {
    const done = (v) => {
      modal.classList.add("hidden");
      resolve(v);
    };
    const agreeRow = $("uninstallAgreeRow"),
      agreeInput = $("uninstallAgreeInput"),
      delBtn = $("uninstallDeleteBtn");
    // Reset the AGREE gate on every open.
    if (agreeRow) agreeRow.style.display = "none";
    if (agreeInput) agreeInput.value = "";
    if (delBtn) {
      delBtn.disabled = false;
      delBtn.textContent = "Delete everything";
    }
    $("uninstallCloseBtn").onclick = () => done(null);
    $("uninstallCancelBtn").onclick = () => done(null);
    $("uninstallKeepBtn").onclick = () => done({ keepModels: true });
    delBtn.onclick = () => {
      // Two-step: first click reveals the gate, second (with AGREE) deletes.
      if (agreeRow && agreeRow.style.display === "none") {
        agreeRow.style.display = "";
        delBtn.disabled = true;
        delBtn.textContent = "Type AGREE above";
        agreeInput?.focus();
        return;
      }
      if ((agreeInput?.value || "").trim() === "AGREE")
        done({ keepModels: false });
    };
    if (agreeInput)
      agreeInput.oninput = () => {
        const ok = agreeInput.value.trim() === "AGREE";
        delBtn.disabled = !ok;
        delBtn.textContent = ok ? "Confirm delete" : "Type AGREE above";
      };
  });
}

$("uninstallBtn")?.addEventListener("click", async function () {
  const choice = await openUninstallModal().catch(() => null);
  if (!choice) {
    appendLog("[*] Uninstall cancelled.");
    return;
  }
  this.disabled = true;
  this.textContent = "Uninstalling...";
  appendLog(
    "[*] Uninstalling Wan2GP" +
      (choice.keepModels ? " (keeping models)…" : " (deleting everything)…"),
  );
  try {
    const r = await window.w2gp.uninstall(choice);
    if (r && r.cancelled) {
      appendLog("[*] Uninstall cancelled.");
    } else if (r && r.success) {
      appendLog("[✓] Wan2GP uninstalled.");
      if (r.keptFiles && r.keptPaths && r.keptPaths.length) {
        appendLog("[i] Kept your files (checkpoints, LoRAs, output):");
        r.keptPaths.forEach((p) => appendLog("[i]   " + p));
        appendLog("[i] Reinstalling will reuse them automatically.");
      }
      if (r.leftoverFolder) {
        appendLog(
          "[i] The empty folder could not be deleted (locked by a process open in it):",
        );
        appendLog("[i]   " + r.leftoverFolder);
        appendLog(
          "[i] Close any terminal/Explorer window open in it and delete it manually.",
        );
      }
      showToast(
        "✓ Wan2GP uninstalled" +
          (r.keptFiles ? " (files kept)" : "") +
          (r.leftoverFolder ? " (empty folder left)" : ""),
      );
      setLaunchButtonsInstalled(false);
      // Nothing installed → back to the installer, not the dashboard.
      await openInstallerFresh("Wan2GP removed — install fresh below.");
    } else {
      appendLog("[!] Uninstall failed: " + ((r && r.error) || "unknown"));
      showToast("✗ " + ((r && r.error) || "Uninstall failed"));
    }
  } catch (e) {
    appendLog("[!] Uninstall error: " + errText(e));
    showToast("✗ " + errText(e));
  } finally {
    this.disabled = false;
    this.textContent = "Uninstall Wan2GP…";
  }
});

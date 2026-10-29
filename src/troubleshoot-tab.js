// troubleshoot-tab.js — Troubleshooting (upstream TROUBLESHOOTING.md).
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

// ── 🛟 Troubleshooting (P0 — upstream TROUBLESHOOTING.md) ──
function tsStatus(id, text) {
  const el = $(id);
  if (el) el.textContent = text;
}
async function tsRefreshLaunchArgs() {
  try {
    const cfg = await window.w2gp.configLoad();
    if ($("launchArgsInput")) $("launchArgsInput").value = cfg.launchArgs || "";
    if ($("portInput")) $("portInput").value = cfg.serverPort || 7861;
  } catch {}
}
$("tsUpstreamDocsLink")?.addEventListener("click", async (ev) => {
  ev.preventDefault();
  await window.w2gp.openExternal(
    "https://github.com/deepbeepmeep/Wan2GP/blob/main/docs/TROUBLESHOOTING.md",
  );
});
$("tsInstallDocsLink")?.addEventListener("click", async (ev) => {
  ev.preventDefault();
  await window.w2gp.openExternal(
    "https://github.com/deepbeepmeep/Wan2GP/blob/main/docs/INSTALLATION.md",
  );
});
$("tsFailsafeBtn")?.addEventListener("click", async function () {
  this.disabled = true;
  tsStatus("tsFailsafeStatus", "Applying…");
  try {
    const r = await window.w2gp.tsFailsafeApply();
    appendLog(
      "[✓] Failsafe applied: " +
        (r.launchArgs || "") +
        (r.backup
          ? " (backup: " + r.backup + ")"
          : " (no wgp_config.json yet)"),
    );
    tsStatus("tsFailsafeStatus", "✓ Failsafe applied — relaunch Wan2GP.");
    showToast("✓ Failsafe applied — relaunch Wan2GP");
    tsRefreshLaunchArgs();
  } catch (e) {
    tsStatus("tsFailsafeStatus", "✗ " + errText(e));
    showToast("✗ " + errText(e));
  }
  this.disabled = false;
});
$("tsCudaBtn")?.addEventListener("click", async function () {
  this.disabled = true;
  tsStatus("tsFailsafeStatus", "Probing torch…");
  try {
    const r = await window.w2gp.tsCudaCheck();
    if (r && r.ok) {
      const accelerator = r.available
        ? "CUDA " +
          (r.cuda || "?") +
          " — cuda_available=true (" +
          (r.devices || 0) +
          " device(s)" +
          (r.name ? ": " + r.name : "") +
          ")"
        : r.mps
          ? "MPS — mps_available=true, cuda_available=false (no CUDA on Apple Silicon)"
          : "no accelerator — cuda_available=false, mps_available=false (CPU only)";
      const msg = "torch " + r.torch + " + " + accelerator;
      appendLog("[✓] GPU check: " + msg);
      tsStatus("tsFailsafeStatus", "✓ " + msg);
    } else {
      tsStatus("tsFailsafeStatus", "✗ " + ((r && r.error) || "probe failed"));
      appendLog(
        "[!] CUDA check failed: " + ((r && (r.stderr || r.error)) || "unknown"),
      );
    }
  } catch (e) {
    tsStatus("tsFailsafeStatus", "✗ " + errText(e));
  }
  this.disabled = false;
});
$("tsComputeBtn")?.addEventListener("click", async function () {
  this.disabled = true;
  tsStatus(
    "tsFailsafeStatus",
    "Running GPU compute (import + kernels — cold start can take a minute)…",
  );
  try {
    const r = await window.w2gp.tsGpuCompute();
    const kernelLines = (r) => {
      const k = (r && r.kernels) || {};
      return Object.keys(k)
        .filter((d) => d !== "sage2_symbol" && d !== "quanto_qbytes_mm")
        .map((d) => {
          const e = k[d] || {};
          const st = e.import || "?";
          return (
            (st === "ok" || st === "missing" ? "✓ " : "✗ ") +
            d +
            " " +
            (e.version || "") +
            (st !== "ok" && st !== "missing" ? " — " + st : "")
          );
        });
    };
    if (r && r.ok) {
      const msg =
        "GPU compute OK: torch " +
        (r.torch || "?") +
        " on " +
        (r.device || "?") +
        " (mode " +
        (r.mode || "?") +
        (r.recorded ? ", recorded for launch" : "") +
        ")";
      appendLog("[✓] " + msg);
      kernelLines(r).forEach((l) => appendLog("    " + l));
      if (r.kernel_warning) appendLog("[i] " + r.kernel_warning);
      tsStatus("tsFailsafeStatus", "✓ " + msg);
      showToast("✓ GPU compute passed");
    } else {
      const det = r && r.detail ? " " + JSON.stringify(r.detail) : "";
      tsStatus("tsFailsafeStatus", "✗ " + ((r && r.error) || "probe failed"));
      const kl = kernelLines(r).filter((l) => l.startsWith("✗"));
      appendLog(
        "[!] GPU compute failed: " + ((r && r.error) || "unknown") + det,
      );
      kl.forEach((l) => appendLog("    " + l));
    }
  } catch (e) {
    tsStatus("tsFailsafeStatus", "✗ " + errText(e));
  }
  this.disabled = false;
});
$("tsPortCheckBtn")?.addEventListener("click", async () => {
  tsStatus("tsPortStatus", "Checking…");
  try {
    const r = await window.w2gp.tsPortStatus();
    if (!r.inUse) tsStatus("tsPortStatus", "✓ Port " + r.port + " is free.");
    else if (r.owner && r.owner.pid)
      tsStatus(
        "tsPortStatus",
        "⚠ Port " +
          r.port +
          " busy — " +
          (r.owner.name || "unknown") +
          " (pid " +
          r.owner.pid +
          ")" +
          (r.owner.ours ? " — looks like Wan2GP" : ""),
      );
    else
      tsStatus("tsPortStatus", "⚠ Port " + r.port + " busy — owner unknown.");
  } catch (e) {
    tsStatus("tsPortStatus", "✗ " + errText(e));
  }
});
$("tsPortKillBtn")?.addEventListener("click", async function () {
  const choice = await window.w2gp.confirmDialog({
    title: "Kill port owner?",
    message:
      "Kill the Python process listening on the server port? Only Python owners are touched — anything else is refused.",
  });
  if (choice !== "ok" && choice !== 0) return;
  this.disabled = true;
  try {
    const r = await window.w2gp.tsPortFix("kill");
    tsStatus(
      "tsPortStatus",
      r.freed
        ? "✓ Port " + r.port + " freed (pid " + r.pid + ")."
        : "⚠ Kill sent but port " +
            r.port +
            " still busy — use next free port.",
    );
    appendLog("[*] Port fix (kill): " + JSON.stringify(r));
  } catch (e) {
    tsStatus("tsPortStatus", "✗ " + errText(e));
    showToast("✗ " + errText(e));
  }
  this.disabled = false;
});
$("tsPortBumpBtn")?.addEventListener("click", async function () {
  this.disabled = true;
  try {
    const r = await window.w2gp.tsPortFix("bump");
    tsStatus(
      "tsPortStatus",
      "✓ Moved " + r.from + " → " + r.port + " — relaunch Wan2GP.",
    );
    showToast("Server port set to " + r.port);
    appendLog("[✓] Port bumped " + r.from + " → " + r.port);
    tsRefreshLaunchArgs();
  } catch (e) {
    tsStatus("tsPortStatus", "✗ " + errText(e));
    showToast("✗ " + errText(e));
  }
  this.disabled = false;
});
$("tsLongPathsBtn")?.addEventListener("click", async function () {
  this.disabled = true;
  try {
    const st = await window.w2gp.tsLongPathsStatus();
    if (st && st.enabled) {
      tsStatus("tsLongPathsStatus", "Already enabled.");
      appendLog("[*] Windows long paths already enabled.");
      return;
    }
    const choice = await window.w2gp.confirmDialog({
      title: "Enable Windows long paths?",
      message:
        "Sets HKLM...LongPathsEnabled=1. Needs admin approval (UAC prompt) and a reboot afterwards. Proceed?",
    });
    if (choice !== "ok" && choice !== 0) {
      tsStatus("tsLongPathsStatus", "Cancelled.");
      return;
    }
    tsStatus("tsLongPathsStatus", "Enabling...");
    const r = await window.w2gp.tsLongPathsEnable();
    if (r && r.already) {
      tsStatus("tsLongPathsStatus", "Already enabled.");
      appendLog("[*] Windows long paths already enabled.");
    } else {
      tsStatus("tsLongPathsStatus", "Enabled - reboot Windows to apply.");
      showToast("Long paths enabled - reboot Windows to apply");
      appendLog(
        "[+] Windows long paths enabled" +
          (r && r.elevated ? " (elevated)" : "") +
          " - reboot Windows to apply.",
      );
      console.log("[ts] long paths enabled", r);
    }
  } catch (e) {
    tsStatus("tsLongPathsStatus", "Error: " + errText(e));
    showToast("Error: " + errText(e));
  } finally {
    this.disabled = false;
  }
});
$("tsDebugCopyBtn")?.addEventListener("click", async function () {
  this.disabled = true;
  tsStatus("tsDebugStatus", "Gathering…");
  try {
    const r = await window.w2gp.tsDebugBundle();
    const md = (r && r.markdown) || "";
    try {
      await navigator.clipboard.writeText(md);
    } catch {
      const ta = document.createElement("textarea");
      ta.value = md;
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      ta.remove();
    }
    tsStatus("tsDebugStatus", "✓ Copied — paste into Discord / GitHub.");
    showToast("✓ Debug info copied to clipboard");
  } catch (e) {
    tsStatus("tsDebugStatus", "✗ " + errText(e));
  }
  this.disabled = false;
});
$("tsTritonTestBtn")?.addEventListener("click", async function () {
  this.disabled = true;
  tsStatus("tsTritonStatus", "Testing import…");
  try {
    const r = await window.w2gp.tsTritonTest();
    tsStatus(
      "tsTritonStatus",
      r.ok
        ? "✓ Triton " + escHtml(r.version || "?") + " importable."
        : "✗ " + escHtml(r.error || "import failed"),
    );
    if (!r.ok)
      appendLog("[!] Triton test: " + (r.stderr || r.error || "failed"));
  } catch (e) {
    tsStatus("tsTritonStatus", "✗ " + escHtml(errText(e)));
  }
  this.disabled = false;
});
async function tsTritonClear(fallback) {
  tsStatus("tsTritonStatus", "Clearing…");
  try {
    const r = await window.w2gp.tsTritonClear(fallback);
    tsStatus(
      "tsTritonStatus",
      "✓ Cache cleared" +
        (r.backup ? " (backup kept)" : " (was already empty)") +
        (fallback ? " — SDPA fallback set, relaunch." : "."),
    );
    appendLog(
      "[✓] Triton cache cleared" +
        (r.backup ? " → " + r.backup : "") +
        (fallback ? " + SDPA fallback" : ""),
    );
    if (fallback) tsRefreshLaunchArgs();
  } catch (e) {
    tsStatus("tsTritonStatus", "✗ " + escHtml(errText(e)));
    showToast("✗ " + errText(e));
  }
}
$("tsTritonClearBtn")?.addEventListener("click", () => tsTritonClear(false));
$("tsTritonSdpaBtn")?.addEventListener("click", () => tsTritonClear(true));
// Previous known-good 3.6.x for the Roll back button. UI default only —
// the backend takes any single-token version, so when the line moves this
// is the one place to update (button label + title + call below).
const TRITON_ROLLBACK_VERSION = "3.6.0.post24";
$("tsTritonRollbackBtn")?.addEventListener("click", async function () {
  if (this.disabled) return;
  this.disabled = true;
  try {
    const choice = await window.w2gp.confirmDialog({
      title: "Roll back Triton to 3.6.0.post24?",
      message: "Reinstalls the previous known-good Triton build and clears the JIT cache.",
      detail:
        "Same reinstall-and-clear as Repair, pinned to triton-windows 3.6.0.post24 instead of the installed build (torch untouched, import verified after). A later setup run floats forward again on upstream's -U range spec — use Repair to preserve whatever is installed then. First generations recompile (possible one-off antivirus toast), then stay warm. Proceed?",
    });
    if (choice !== "ok" && choice !== 0) {
      tsStatus("tsTritonStatus", "Cancelled.");
      return;
    }
    tsStatus("tsTritonStatus", "Rolling back Triton…");
    const r = await window.w2gp.tsTritonRepair(TRITON_ROLLBACK_VERSION);
    if (r && r.importVersion) {
      tsStatus(
        "tsTritonStatus",
        "✓ Rolled back " + r.dist + " " + (r.previous || "?") + " → " + r.version + " (import " + r.importVersion + ").",
      );
      appendLog("[✓] Triton rolled back: " + r.dist + " " + (r.previous || "?") + " → " + r.version);
    } else {
      tsStatus(
        "tsTritonStatus",
        "✓ Reinstalled " + ((r && r.dist) || "triton") + " but the import check failed — see console.",
      );
      appendLog("[!] Triton rollback installed but import failed — see console output");
    }
    showToast("Triton rollback finished — see status");
  } catch (e) {
    tsStatus("tsTritonStatus", "✗ " + errText(e));
    showToast("✗ " + errText(e));
  } finally {
    this.disabled = false;
  }
});
// Repair a damaged Triton install (quarantined/half-written files): exact
// build force-reinstalled, both caches cleared, import verified. Repair, not
// AV strategy — identical bytes mean identical verdicts.
$("tsTritonRepairBtn")?.addEventListener("click", async function () {
  if (this.disabled) return;
  this.disabled = true;
  try {
    const choice = await window.w2gp.confirmDialog({
      title: "Repair Triton wheel?",
      message: "Force-reinstalls the exact installed Triton build and clears the JIT cache.",
      detail:
        "Runs pip install --force-reinstall --no-deps on the installed triton build (torch and everything else untouched — only Triton's own files are replaced), clears both JIT cache homes (renamed to backup), then verifies the import. Takes a minute or two. Use when Triton itself is damaged; it does not change antivirus verdicts. Proceed?",
    });
    if (choice !== "ok" && choice !== 0) {
      tsStatus("tsTritonStatus", "Cancelled.");
      return;
    }
    tsStatus("tsTritonStatus", "Reinstalling Triton…");
    const r = await window.w2gp.tsTritonRepair();
    if (r && r.importVersion) {
      tsStatus("tsTritonStatus", "✓ Repaired " + r.dist + " " + r.version + " (import " + r.importVersion + ").");
      appendLog("[✓] Triton repaired: " + r.dist + " " + r.version);
    } else {
      tsStatus(
        "tsTritonStatus",
        "✓ Reinstalled " + ((r && r.dist) || "triton") + " but the import check failed — see console.",
      );
      appendLog("[!] Triton reinstalled but import failed — see console output");
    }
    showToast("Triton repair finished — see status");
  } catch (e) {
    tsStatus("tsTritonStatus", "✗ " + errText(e));
    showToast("✗ " + errText(e));
  } finally {
    this.disabled = false;
  }
});

// ── MSVC Build Tools (signed compiler for Triton JIT) ──
// Status on open: found → pin applies on next launch; missing → TinyCC
// fallback with the installer one click away. Off-Windows the row hides
// (no MSVC there, and no TinyCC problem either).
async function tsMsvcRefresh() {
  try {
    const r = await window.w2gp.tsMsvcStatus();
    if (!r || !r.supported) return;
    tsStatus(
      "tsMsvcStatus",
      r.installed
        ? "✓ MSVC cl.exe present — the signed-compiler pin applies on next launch."
        : "MSVC C++ toolchain not found — Triton builds through its bundled TinyCC fallback.",
    );
  } catch {}
}
$("tsMsvcRecheckBtn")?.addEventListener("click", () => tsMsvcRefresh());
$("tsMsvcInstallBtn")?.addEventListener("click", async function () {
  if (this.disabled) return;
  this.disabled = true;
  try {
    const st = await window.w2gp.tsMsvcStatus();
    if (st && st.installed) {
      tsStatus("tsMsvcStatus", "Already installed" + (st.cc ? " (" + st.cc + ")" : "."));
      appendLog("[*] MSVC toolchain already present.");
      return;
    }
    const choice = await window.w2gp.confirmDialog({
      title: "Install MSVC Build Tools?",
      message: "Downloads Microsoft's official VS 2022 Build Tools bootstrapper and runs it elevated.",
      detail:
        "One-time, a few GB downloaded by the Microsoft installer, needs admin approval (UAC prompt) and can take 10+ minutes — the installer shows its own progress. Afterwards the launcher pins Triton JIT builds to the signed MSVC compiler automatically (relaunch or Recheck to confirm). Without it, Triton builds through its bundled unsigned TinyCC, which antivirus behavioral engines may block mid-generation. Proceed?",
    });
    if (choice !== "ok" && choice !== 0) {
      tsStatus("tsMsvcStatus", "Cancelled.");
      return;
    }
    tsStatus("tsMsvcStatus", "Downloading bootstrapper…");
    const r = await window.w2gp.tsMsvcInstall();
    if (r && r.already) {
      tsStatus("tsMsvcStatus", "Already installed.");
      appendLog("[*] MSVC toolchain already present.");
    } else {
      tsStatus("tsMsvcStatus", "Installer running elevated — finish it, then press Recheck.");
      appendLog(
        "[*] MSVC Build Tools installer launched elevated — complete it, then Recheck (next Launch auto-detects cl.exe).",
      );
      showToast("Finish the Microsoft installer, then press Recheck");
    }
  } catch (e) {
    tsStatus("tsMsvcStatus", "✗ " + errText(e));
    showToast("✗ " + errText(e));
  } finally {
    this.disabled = false;
  }
});
try {
  // Gated by data-requires="tritonRepair" in index.html (platform-gate.js);
  // the refresh only makes sense where the toolchain exists.
  if (window.w2gp.capabilities.tritonRepair) {
    tsMsvcRefresh();
  }
} catch {}

// ── v17 RAM/VRAM (upstream TROUBLESHOOTING.md → Memory Issues, v17 additions) ──
// Every write goes through memoryProfileApply: fail-closed gate + snapshot,
// so a remedy can never land a value WanGP would ignore.
async function tsApplyRemedy(action, label) {
  tsStatus("tsOomStatus", "Applying " + label + "…");
  try {
    const r = await window.w2gp.tsOomRemedy(action);
    if (r && r.ok) {
      // Flag-shaped remedies (`--preload 0`) come back as launchArgs, not as
      // wgp_config keys: show what was staged and refresh the Launch field.
      if (typeof r.launchArgs === "string") {
        tsStatus(
          "tsOomStatus",
          "✓ " + label + " — Extra Launch Args: " + (r.launchArgs || "(empty)") + ". Takes effect on the next launch.",
        );
        appendLog("[✓] " + label + " → launch args: " + r.launchArgs);
        try {
          tsRefreshLaunchArgs();
        } catch {}
        showToast("✓ " + label);
        return;
      }
      const keys = (r.applied || []).join(", ") || "(nothing to change)";
      tsStatus("tsOomStatus", "✓ " + label + " — wrote " + keys + ". Relaunch WanGP if the setting needs a restart.");
      appendLog("[✓] " + label + " → " + keys);
      showToast("✓ " + label);
    } else {
      tsStatus("tsOomStatus", "✗ " + ((r && r.error) || "failed"));
    }
  } catch (e) {
    tsStatus("tsOomStatus", "✗ " + escHtml(errText(e)));
  }
}
$("tsHeadSplitBtn")?.addEventListener("click", () =>
  tsApplyRemedy("head_split_medium", "Attention Head Split → Medium"),
);
$("tsLowerRamBtn")?.addEventListener("click", () =>
  tsApplyRemedy("lower_reserved_ram", "Reserved RAM → 25% with Smart Pinning on"),
);
// Multi-GPU OOM: pin generation to one card. With several GPUs visible,
// WanGP's device-0 paths and its device_count() loops can disagree, which
// surfaces as a bare CUDA out of memory while VRAM reads free.
$("tsPinGpuBtn")?.addEventListener("click", async () => {
  tsStatus("tsOomStatus", "Looking for GPUs…");
  try {
    const detected = (await window.w2gp.detectGpus()) || [];
    if (detected.length < 2) {
      tsStatus(
        "tsOomStatus",
        "Only one GPU detected — nothing to pin. Use the remedies beside this button.",
      );
      return;
    }
    const cfg = await window.w2gp.configLoad();
    // Respect an explicit card the user already chose; only fall back to the
    // biggest one when the picker is on Auto / All.
    const saved = (cfg.gpuDevice || "auto").trim();
    const chosen = saved.startsWith("uuid:")
      ? detected.find((g) => g.uuid && saved === "uuid:" + g.uuid)
      : null;
    const best =
      chosen ||
      detected.slice().sort((a, b) => (b.vramMB || 0) - (a.vramMB || 0))[0];
    cfg.gpuDevice = best.uuid ? "uuid:" + best.uuid : "cuda:" + best.index;
    await window.w2gp.configSave(cfg);
    const gb = best.vramMB ? " (" + Math.round(best.vramMB / 1024) + " GB)" : "";
    tsStatus(
      "tsOomStatus",
      "✓ Generation pinned to " + best.name + gb + " — relaunch WanGP.",
    );
    appendLog("[✓] GPU pinned to " + best.name);
    showToast("✓ Pinned to " + best.name);
    document.dispatchEvent(
      new CustomEvent("gpu-device-changed", { detail: { value: cfg.gpuDevice } }),
    );
  } catch (e) {
    tsStatus("tsOomStatus", "✗ " + escHtml(errText(e)));
  }
});
$("tsFailsafeP5Btn")?.addEventListener("click", async () => {
  this.disabled = true;
  tsStatus("tsOomStatus", "Applying failsafe…");
  try {
    const r = await window.w2gp.tsFailsafeApply();
    tsStatus("tsOomStatus", "✓ P5 failsafe applied" + (r.backup ? " (backup: " + r.backup + ")" : "") + " — relaunch WanGP.");
    appendLog("[✓] Failsafe P5 applied" + (r.backup ? " → " + r.backup : ""));
    showToast("✓ Failsafe P5 applied");
  } catch (e) {
    tsStatus("tsOomStatus", "✗ " + escHtml(errText(e)));
  }
  this.disabled = false;
});

// Known-good recipe: report first (read-only), load only on the second click.
async function tsFetchKnownGood() {
  try {
    return await window.w2gp.tsKnownGood();
  } catch (e) {
    tsStatus("tsKnownGoodStatus", "✗ " + errText(e));
    return null;
  }
}
$("tsKnownGoodBtn")?.addEventListener("click", async function () {
  this.disabled = true;
  tsStatus("tsKnownGoodStatus", "Reading upstream's guide…");
  try {
    const r = await tsFetchKnownGood();
    if (!r) return;
    if (!r.recipe) {
      tsStatus("tsKnownGoodStatus", escHtml(r.reason || "No upstream recipe for this GPU class."));
      return;
    }
    const s = r.recipe.settings;
    tsStatus(
      "tsKnownGoodStatus",
      "For " + escHtml(r.profileKey) + ": attention " + escHtml(String(s.attention_mode)) +
        ", video profile " + escHtml(String(s.video_profile)) +
        ", compile " + (s.compile === "cudagraph" ? "CUDA Graph" : s.compile === "transformer" || s.compile === true ? "PyTorch" : "off") + ". " + escHtml(r.recipe.note || ""),
    );
  } finally {
    this.disabled = false;
  }
});
$("tsKnownGoodApplyBtn")?.addEventListener("click", async function () {
  this.disabled = true;
  try {
    const r = await tsFetchKnownGood();
    if (!r) return;
    if (!r.recipe) {
      tsStatus("tsKnownGoodStatus", escHtml(r.reason || "No upstream recipe for this GPU class."));
      return;
    }
    const applied = await window.w2gp.memoryProfileApply(r.recipe.settings);
    if (applied && applied.ok) {
      tsStatus("tsKnownGoodStatus", "✓ Wrote " + (applied.applied || []).join(", ") + " — review the rec/saved tags in Performance Settings, where you can still change anything.");
      appendLog("[✓] Known-good recipe for " + r.profileKey + " → " + (applied.applied || []).join(", "));
      showToast("✓ Known-good settings written");
    } else {
      tsStatus("tsKnownGoodStatus", "✗ " + ((applied && applied.error) || "apply failed"));
    }
  } catch (e) {
    tsStatus("tsKnownGoodStatus", "✗ " + escHtml(errText(e)));
  } finally {
    this.disabled = false;
  }
});

// VRAM diagnostics — upstream ships these two scripts in the checkout.
async function tsVramDiag(action, label) {
  tsStatus("tsVramStatus", "Running " + label + "…");
  try {
    const r = await window.w2gp.tsVramDiag(action);
    if (r && r.ok) {
      tsStatus("tsVramStatus", "✓ " + (r.output || "(no output)"));
      appendLog("[✓] " + label + "\n" + (r.output || ""));
    } else {
      tsStatus("tsVramStatus", "✗ " + ((r && (r.error || r.output)) || "failed"));
    }
  } catch (e) {
    tsStatus("tsVramStatus", "✗ " + escHtml(errText(e)));
  }
}
$("tsVramListBtn")?.addEventListener("click", () => tsVramDiag("list", "gpumem.cmd"));
$("tsVramTrimBtn")?.addEventListener("click", async () => {
  const choice = await window.w2gp.confirmDialog({
    title: "Trim idle VRAM?",
    message:
      "Applies memory pressure briefly to encourage Windows to move idle GPU allocations to system RAM. The screen can flash, and GPU apps can stall for a moment. Continue?",
  });
  if (choice !== "ok" && choice !== 0) {
    tsStatus("tsVramStatus", "Cancelled.");
    return;
  }
  tsVramDiag("trim", "gputrim.cmd");
});

// Verbose logging — `--verbose 2` on the next launch, both the main server
// and the standalone Deepy Web child.
$("tsVerboseChk")?.addEventListener("change", async function () {
  const on = this.checked;
  try {
    const cfg = await window.w2gp.configLoad();
    cfg.verboseLogging = on;
    await window.w2gp.configSave(cfg);
    tsStatus("tsOomStatus", "");
    showToast(on ? "Verbose logging on — next launch is detailed" : "Verbose logging off");
    appendLog("[*] verboseLogging = " + on + " (applies on next launch)");
  } catch (e) {
    this.checked = !on;
    showToast("✗ " + errText(e));
  }
});
(async function tsLoadVerbose() {
  try {
    const cfg = await window.w2gp.configLoad();
    if ($("tsVerboseChk")) $("tsVerboseChk").checked = cfg.verboseLogging === true;
  } catch {}
})();

// ── Head Split levels + no-preload (upstream's memory guide) ──
$("tsHeadSplitLowBtn")?.addEventListener("click", () =>
  tsApplyRemedy("head_split_low", "Attention Head Split → Low"),
);
$("tsHeadSplitHighBtn")?.addEventListener("click", () =>
  tsApplyRemedy("head_split_high", "Attention Head Split → High"),
);
$("tsNoPreloadBtn")?.addEventListener("click", () =>
  tsApplyRemedy("no_preload", "--preload 0 in Extra Launch Args"),
);

// ── Model-file integrity (upstream #2549 class: a bad download that only
// surfaces as a deep ValueError inside WanGP) ──
$("tsModelCheckBtn")?.addEventListener("click", async function () {
  if (this.disabled) return;
  this.disabled = true;
  const box = $("tsModelBox");
  tsStatus("tsModelStatus", "Reading model headers…");
  if (box) {
    box.style.display = "none";
    box.textContent = "";
  }
  try {
    const r = await window.w2gp.tsModelIntegrity();
    if (!r || !r.ok) {
      tsStatus(
        "tsModelStatus",
        "✗ " +
          ((r && r.error) ||
            "no model folder found — set one in Wan2GP's Configuration"),
      );
      return;
    }
    const bad = Array.isArray(r.bad) ? r.bad : [];
    const scanned = r.scanned || 0;
    if (bad.length === 0) {
      tsStatus(
        "tsModelStatus",
        "✓ " +
          scanned +
          " model file(s) checked in " +
          (r.tookMs || 0) +
          " ms — headers all valid" +
          (r.truncated ? " (scan capped: very large folder)" : ""),
      );
      appendLog("[✓] Model integrity: " + scanned + " file(s) OK");
      return;
    }
    tsStatus(
      "tsModelStatus",
      "⚠ " + bad.length + " unusable file(s) of " + scanned + " checked",
    );
    appendLog("[!] Model integrity: " + bad.length + " bad file(s)");
    if (box) {
      box.style.display = "block";
      box.textContent = bad
        .map(
          (b) =>
            "• " +
            b.name +
            " — " +
            b.reason +
            " (" +
            Math.round((b.size || 0) / 1024) +
            " KB)\n  " +
            b.path,
        )
        .join("\n");
    }
    showToast("⚠ " + bad.length + " model file(s) look broken — see Manage → Troubleshooting");
  } catch (e) {
    tsStatus("tsModelStatus", "✗ " + escHtml(errText(e)));
  } finally {
    this.disabled = false;
  }
});

// ── YuE2 startup crash (launcher #82 = upstream #2517): one stale
// `audio_prompt_type` embedded in an output file makes WanGP raise
// `ValueError: Choose one of the five YuE2 composition sources.` while it
// migrates the gallery, before the UI paints. The scan is read-only; the fix
// moves the offender into `yue2-quarantine/` so nothing is destroyed.
function yue2Render(r) {
  const box = $("tsYue2Box");
  const bad = r && Array.isArray(r.bad) ? r.bad : [];
  if (box) {
    box.style.display = bad.length ? "block" : "none";
    box.textContent = bad
      .map(
        (b) =>
          "• " +
          b.name +
          ' — audio_prompt_type "' +
          b.value +
          '" (' +
          Math.round((b.size || 0) / 1024) +
          " KB)\n  " +
          b.path,
      )
      .join("\n");
  }
  if ($("tsYue2FixBtn")) $("tsYue2FixBtn").style.display = bad.length ? "" : "none";
  return bad;
}

async function yue2Scan() {
  const r = await window.w2gp.tsYue2Metadata();
  if (!r || !r.ok) return null;
  const bad = yue2Render(r);
  tsStatus(
    "tsYue2Status",
    bad.length
      ? "⚠ " + bad.length + " file(s) carry a source WanGP rejects, of " + (r.scanned || 0) + " audio file(s) read"
      : "✓ " + (r.scanned || 0) + " audio file(s) read in " + (r.tookMs || 0) + " ms — no bad YuE2 source" + (r.truncated ? " (scan capped: very large folder)" : ""),
  );
  return { ...r, bad };
}

$("tsYue2CheckBtn")?.addEventListener("click", async function () {
  if (this.disabled) return;
  this.disabled = true;
  tsStatus("tsYue2Status", "Reading output metadata…");
  try {
    const r = await yue2Scan();
    if (!r) {
      tsStatus("tsYue2Status", "✗ no output folder found — set one in Wan2GP's Configuration");
      return;
    }
    appendLog(
      r.bad.length
        ? "[!] YuE2 metadata: " + r.bad.length + " bad output(s) of " + (r.scanned || 0) + " scanned"
        : "[✓] YuE2 metadata: " + (r.scanned || 0) + " output(s) OK",
    );
  } catch (e) {
    tsStatus("tsYue2Status", "✗ " + escHtml(errText(e)));
  } finally {
    this.disabled = false;
  }
});

async function yue2Quarantine() {
  const r = await window.w2gp.tsYue2Quarantine();
  const moved = Array.isArray(r && r.moved) ? r.moved : [];
  const failed = Array.isArray(r && r.failed) ? r.failed : [];
  if (moved.length === 0) {
    showToast(
      failed.length
        ? "✗ " + (r.error || "could not move the files")
        : "Nothing left to move — rescan",
    );
    return null;
  }
  appendLog(
    "[✓] Moved " +
      moved.length +
      " file(s) to " +
      (r.dirs || []).join(", ") +
      "\n" +
      moved.map((m) => "    " + m.name).join("\n"),
  );
  if (failed.length)
    appendLog(
      "[!] " +
        failed.length +
        " file(s) stayed in place: " +
        failed.map((f) => f.name + " (" + f.error + ")").join(", "),
    );
  return r;
}

$("tsYue2FixBtn")?.addEventListener("click", async function () {
  if (this.disabled) return;
  const choice = await window.w2gp.confirmDialog({
    title: "Quarantine the offending outputs?",
    message:
      "Moves the listed files into yue2-quarantine/ next to your outputs so WanGP can start. Nothing is deleted — move them back to restore them once upstream fixes the migration.",
  });
  if (choice !== "ok" && choice !== 0) {
    tsStatus("tsYue2Status", "Cancelled.");
    return;
  }
  this.disabled = true;
  try {
    const r = await yue2Quarantine();
    if (r) {
      const moved = Array.isArray(r.moved) ? r.moved : [];
      showToast("✓ Quarantined " + moved.length + " file(s)");
      await yue2Scan();
      tsStatus(
        "tsYue2Status",
        "✓ moved " + moved.length + " file(s) to yue2-quarantine/ — launch Wan2GP again",
      );
    }
  } catch (e) {
    tsStatus("tsYue2Status", "✗ " + escHtml(errText(e)));
  } finally {
    this.disabled = false;
  }
});

// Startup triage hook: called when Wan2GP died before serving. Returns true
// when this specific cause was found and handled, so the caller skips its
// generic "retry with a temporary settings folder" dialog — a throwaway
// settings folder cannot help, the value lives inside the media file.
window.w2gpYue2Triage = async function (relaunch) {
  const r = await yue2Scan();
  if (!r || r.bad.length === 0) return false;
  const names = r.bad.slice(0, 5).map((b) => "• " + b.name + ' ("' + b.value + '")');
  if (r.bad.length > 5) names.push("… and " + (r.bad.length - 5) + " more");
  const choice = await window.w2gp.confirmDialog({
    title: "WanGP cannot start: a YuE2 source in your output files",
    message:
      "WanGP exited while migrating the settings stored inside your generated audio, and " +
      r.bad.length +
      " file(s) in your output folder carry an audio_prompt_type this version rejects. That is upstream bug #2517, not a launcher or install problem.",
    detail:
      names.join("\n") +
      "\n\nQuarantine moves them into yue2-quarantine/ next to your outputs (nothing is deleted) and restarts Wan2GP. Move the files back any time.",
  });
  if (choice !== "ok" && choice !== 0) {
    appendLog("[i] YuE2 triage dismissed — the files are still in your output folder.");
    return true;
  }
  const q = await yue2Quarantine();
  if (q) {
    const moved = Array.isArray(q.moved) ? q.moved : [];
    showToast("✓ Quarantined " + moved.length + " file(s)");
    if (typeof relaunch === "function") relaunch();
  }
  return true;
};

// ── Extra Launch Args vs `wgp.py --help` for the installed commit ──
$("tsFlagAuditBtn")?.addEventListener("click", async function () {
  if (this.disabled) return;
  this.disabled = true;
  tsStatus("tsFlagAuditStatus", "Comparing with wgp.py --help (first run per commit takes ~40 s)…");
  try {
    const r = await window.w2gp.tsFlagAudit(false);
    if (!r || !r.ok) {
      tsStatus("tsFlagAuditStatus", "✗ " + ((r && r.error) || "audit failed"));
      return;
    }
    const unknown = Array.isArray(r.unknown) ? r.unknown : [];
    if (unknown.length === 0) {
      tsStatus(
        "tsFlagAuditStatus",
        "✓ " +
          (r.checked || []).length +
          " launch arg(s) known to WanGP " +
          (r.head || "") +
          " (" +
          (r.known || 0) +
          " options listed)",
      );
      appendLog("[✓] Launch args match WanGP " + (r.head || "") + " (wgp.py --help)");
    } else {
      tsStatus(
        "tsFlagAuditStatus",
        "⚠ Unknown to WanGP " + (r.head || "") + ": " + unknown.join(" "),
      );
      appendLog(
        "[!] Launch arg(s) not listed by this WanGP version: " + unknown.join(" ") +
          " — remove them in Manage → Launch → Extra Launch Args or Wan2GP will refuse to start.",
      );
      showToast("⚠ Unknown launch args — see Troubleshooting");
    }
  } catch (e) {
    tsStatus("tsFlagAuditStatus", "✗ " + escHtml(errText(e)));
  } finally {
    this.disabled = false;
  }
});

// ── Preview decoders (the HEVC gallery regression's permanent sensor) ──
// Runs in the same WebView2 process with the same browser arguments as the
// Wan2GP window, so "Audio only + black frame" reports become one glance:
// HEVC "no" here means the platform decoder is gone (SwiftShader mode).
async function tsShowPreviewCodecs() {
  const el = $("tsCodecStatus");
  if (!el) return;
  const v = document.createElement("video");
  const probe = (t) => {
    try {
      return v.canPlayType(t) === "" ? "no" : "yes";
    } catch {
      return "?";
    }
  };
  const hevc = probe('video/mp4; codecs="hev1.1.6.L93.B0"') === "yes" ||
    probe('video/mp4; codecs="hvc1"') === "yes";
  const rows = [
    "H.264 " + probe('video/mp4; codecs="avc1.42E01E"'),
    "HEVC " + (hevc ? "yes" : "no"),
    "AV1 " + probe('video/mp4; codecs="av01.0.05M.08"'),
    "VP9 " + probe('video/webm; codecs="vp9"'),
  ];
  let mode = "";
  try {
    const cfg = await window.w2gp.configLoad();
    mode = cfg.launcherGpu || "auto";
  } catch {}
  el.textContent =
    "Preview decoders in this window: " +
    rows.join(" · ") +
    " — launcher GPU: " +
    mode +
    (hevc
      ? ""
      : " (HEVC previews will play sound over a black picture; switch Launcher GPU away from Disabled)") +
    ".";
}
tsShowPreviewCodecs();

// ── macOS (MPS) known-issue card ──
// The v17 Apple Silicon blockers are upstream bugs: no launcher control fixes
// them, so the card stays hidden off macOS and links out to the tracker.
(function tsMacosCard() {
  const platform =
    (document.body && document.body.dataset && document.body.dataset.platform) ||
    "";
  if (platform !== "darwin" && platform !== "macos") return;
  const card = $("macosMpsCard");
  if (!card) return;
  card.style.display = "";
  card.addEventListener("click", async (ev) => {
    const a = ev.target && ev.target.closest ? ev.target.closest("a[href^='https://']") : null;
    if (!a) return;
    ev.preventDefault();
    await window.w2gp.openExternal(a.href);
  });
})();

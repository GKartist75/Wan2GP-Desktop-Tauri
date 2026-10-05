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
    if ($("portInput")) $("portInput").value = cfg.serverPort || 7860;
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
      const msg =
        "torch " +
        r.torch +
        " + CUDA " +
        (r.cuda || "?") +
        " — cuda_available=" +
        r.available +
        " (" +
        (r.devices || 0) +
        " device(s)" +
        (r.name ? ": " + r.name : "") +
        ")";
      appendLog("[✓] CUDA check: " + msg);
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
    "Running GPU compute (import + kernels, ~1 min on cold HIP)…",
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

// ── v17 RAM/VRAM (upstream TROUBLESHOOTING.md → Memory Issues, v17 additions) ──
// Every write goes through memoryProfileApply: fail-closed gate + snapshot,
// so a remedy can never land a value WanGP would ignore.
async function tsApplyRemedy(action, label) {
  tsStatus("tsOomStatus", "Applying " + label + "…");
  try {
    const r = await window.w2gp.tsOomRemedy(action);
    if (r && r.ok) {
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
        ", compile " + (s.compile ? "on" : "off") + ". " + escHtml(r.recipe.note || ""),
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

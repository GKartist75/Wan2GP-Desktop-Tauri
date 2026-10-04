// env-tab.js — Active Environment card: env unlink/reinstall + kernel wheels.
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

// ── Env unlink button visibility ──
// Shown only when a repo is present AND an env is known-active: with no
// install the buttons used to stay clickable and fail (or "clean" a stale
// registry entry with no context).
function refreshEnvUnlink(hasRepo) {
  var btn = $("envUnlinkBtn");
  var restoreBtn = $("envRestoreBtn");
  var reinstallBtn = $("envReinstallBtn");
  var setupBtn = $("envSetupBtn");
  var hideAll = () => {
    if (btn) btn.style.display = "none";
    if (restoreBtn) restoreBtn.style.display = "none";
    if (reinstallBtn) reinstallBtn.style.display = "none";
    if (setupBtn) setupBtn.style.display = "none";
  };
  if (hasRepo === false) {
    hideAll();
    return;
  }
  // State-driven: shown whenever an env is known-active, hidden otherwise.
  var hasEnv = window._hasActiveEnv === true;
  var name = (hasEnv && window._activeEnvName) || "";
  if (btn) {
    if (name && name !== "—" && name !== "No active environment") {
      if (setupBtn) setupBtn.style.display = "none";
      btn.style.display = "";
      if (restoreBtn) restoreBtn.style.display = "";
      if (reinstallBtn) reinstallBtn.style.display = "";
      btn.onclick = async () => {
        if (!confirm('Uninstall environment "' + name + '"?')) return;
        btn.disabled = true;
        btn.textContent = "...";
        appendLog("[*] Uninstalling environment " + name + "...");
        try {
          var r = await window.w2gp.uninstallEnv(name);
          if (r && r.success) {
            appendLog("[*] Environment " + name + " uninstalled.");
            await refreshDashboard();
            if (window._hasActiveEnv === false)
              appendLog(
                '[*] No environments remaining — click "🧭 Run Setup" in the Active Environment card to install a fresh one.',
              );
          } else showToast((r && r.error) || "Failed");
        } catch (e) {
          showToast(errText(e));
        }
        btn.disabled = false;
        btn.textContent = "unlink";
      };
    } else {
      hideAll();
      // No active env (e.g. just unlinked the last one): offer the
      // installer directly — same destination as Manage → Run Setup.
      if (setupBtn) {
        setupBtn.style.display = "";
        setupBtn.onclick = () => {
          openInstallerFresh();
        };
      }
    }
  }
  // Restore button handler
  if (restoreBtn) {
    restoreBtn.onclick = async () => {
      if (
        !confirm(
          "Reinstall all packages from requirements.txt? This will restore pinned versions.",
        )
      )
        return;
      restoreBtn.disabled = true;
      restoreBtn.textContent = "...";
      appendLog("[*] Restoring packages from requirements.txt...");
      try {
        var r = await window.w2gp.restoreRequirements();
        if (r && r.success) {
          appendLog("[*] Requirements restored.");
          hideDriftBanner();
          setTimeout(refreshDashboard, 2000);
        } else showToast((r && r.error) || "Failed");
      } catch (e) {
        showToast(errText(e));
      }
      restoreBtn.disabled = false;
      restoreBtn.textContent = "restore";
    };
  }
  // Full reinstall: delete the env and run the whole setup again (fresh
  // venv, pinned Python, PyTorch+CUDA, requirements, kernels, smoke test).
  // Restore only re-pips requirements into the existing venv — this fixes
  // broken interpreters, wrong torch builds and corrupt venvs. Models,
  // plugins and settings are untouched (they live outside the env folder).
  if (reinstallBtn) {
    reinstallBtn.onclick = async () => {
      const envType = window._activeEnvType || "uv";
      if (
        !window.confirm(
          'Recreate the "' +
            name +
            '" environment from scratch?\n\nFresh venv, Python, PyTorch + CUDA, packages and kernels (takes a while — watch the console). Models, plugins and settings are kept.',
        )
      )
        return;
      reinstallBtn.disabled = true;
      reinstallBtn.textContent = "working…";
      appendLog(
        "[*] Reinstalling environment " +
          name +
          " from scratch (" +
          envType +
          ") — progress below…",
      );
      try {
        const u = await window.w2gp.uninstallEnv(name);
        if (!u || !u.success)
          throw new Error((u && u.error) || "env removal failed");
        appendLog("[*] Old env removed — running full setup…");
        const r = await window.w2gp.install(envType);
        if (r && (r.success || r.ok)) {
          appendLog("[*] Environment reinstalled.");
          showToast("✓ Environment reinstalled");
        } else
          showToast(
            "✗ " + ((r && r.error) || "reinstall failed — see console"),
          );
      } catch (e) {
        appendLog("[!] Reinstall failed: " + errText(e));
        showToast("✗ " + errText(e));
      }
      reinstallBtn.disabled = false;
      reinstallBtn.textContent = "reinstall";
      refreshDashboard();
    };
  }
}

const _labelToKey = {
  Python: "python",
  Torch: "torch",
  CUDA: "cuda",
  Triton: "triton",
  "Sage Attn": "sageattention",
  "Flash Attn": "flash_attn",
  Diffusers: "diffusers",
  Transformers: "transformers",
  Gradio: "gradio",
  Accelerate: "accelerate",
  onnxruntime: "onnxruntime",
  OpenCV: "opencv-python",
  PEFT: "peft",
  hf_hub: "huggingface_hub",
  bitsandbytes: "bitsandbytes",
  NumPy: "numpy",
  Tokenizers: "tokenizers",
};

// Pinned-upgrade override dialog (explicit Yes/Cancel — no OK/Cancel trap).
// openPinOverride resolves true on "Yes, upgrade", false on Cancel/✕/
// backdrop click. Buttons are wired once here; text via textContent only
// (package names come from the local pip scan — never HTML).
let _pinOverrideResolve = null;
function openPinOverride(title, body, recovery) {
  const modal = $("pinOverrideModal");
  if (!modal) return Promise.resolve(false);
  $("pinOverrideTitle").textContent = title || "Upgrade pinned package?";
  $("pinOverrideBody").textContent = body || "";
  $("pinOverrideRecovery").textContent = recovery || "";
  modal.classList.remove("hidden");
  return new Promise((resolve) => {
    _pinOverrideResolve = resolve;
  });
}
function closePinOverride(result) {
  $("pinOverrideModal")?.classList.add("hidden");
  if (_pinOverrideResolve) {
    const r = _pinOverrideResolve;
    _pinOverrideResolve = null;
    r(result);
  }
}
$("pinOverrideCloseBtn")?.addEventListener("click", () =>
  closePinOverride(false),
);
$("pinOverrideCancelBtn")?.addEventListener("click", () =>
  closePinOverride(false),
);
$("pinOverrideYesBtn")?.addEventListener("click", () =>
  closePinOverride(true),
);
$("pinOverrideModal")?.addEventListener("click", (ev) => {
  if (ev.target && ev.target.id === "pinOverrideModal")
    closePinOverride(false);
});

$("checkPkgUpdatesBtn").addEventListener("click", async function () {
  this.textContent = "Checking...";
  this.classList.add("check-updates-loading");
  this.disabled = true;
  const versions = {};
  document.querySelectorAll(".env-detail .spec-row").forEach((row) => {
    const labelEl = row.querySelector(".spec-label");
    const valEl = row.querySelector(".spec-value");
    if (!labelEl || !valEl) return;
    const label = labelEl.textContent.trim();
    const key = _labelToKey[label];
    if (!key) return;
    const val = valEl.textContent.trim();
    if (val && val !== "—") versions[key] = val;
  });
  if (Object.keys(versions).length === 0) {
    this.textContent = "↻ Check Updates";
    this.classList.remove("check-updates-loading");
    this.disabled = false;
    return;
  }
  var results = await window.w2gp.checkPackageUpdates(versions);
  this.textContent = "↻ Check Updates";
  this.classList.remove("check-updates-loading");
  this.disabled = false;
  if (!results || !results.length) {
    showToast("No update info available");
    return;
  }
  let updateCount = 0;
  results.forEach((r) => {
    let row = document.querySelector(
      '.env-detail .spec-row[data-pkg="' + r.name + '"]',
    );
    if (!row) {
      const revMap = {};
      for (const k in _labelToKey) revMap[_labelToKey[k]] = k;
      const label = revMap[r.name];
      if (!label) return;
      const rows = document.querySelectorAll(".env-detail .spec-row");
      for (let i = 0; i < rows.length; i++) {
        if (
          rows[i].querySelector(".spec-label") &&
          rows[i].querySelector(".spec-label").textContent.trim() === label
        ) {
          row = rows[i];
          row.setAttribute("data-pkg", r.name);
          break;
        }
      }
    }
    if (!row) return;
    const valEl = row.querySelector(".spec-value");
    if (!valEl) return;
    const oldLatest = row.querySelector(".spec-latest");
    if (oldLatest) oldLatest.remove();
    const oldBtn = row.querySelector(".spec-update-btn");
    if (oldBtn) oldBtn.remove();
    const oldPin = row.querySelector(".spec-pinned");
    if (oldPin) oldPin.remove();
    if (!r.latest) return;
    const latestSpan = document.createElement("span");
    latestSpan.className = "spec-latest";
    latestSpan.textContent = "→ " + r.latest;
    valEl.after(latestSpan);
    if (r.pinned) {
      // Issue #54: GPU-profile / kernel-wheel dists (torch, flash_attn, …)
      // are launcher-managed — never offer the ↑ arrow, show pinned chip.
      row.classList.add("up-to-date");
      row.classList.remove("has-update");
      const pin = document.createElement("span");
      pin.className = "spec-pinned";
      pin.textContent = "pinned";
      pin.title =
        r.pinSource === "gpu-profile"
          ? "GPU-profile managed (PyTorch CUDA/ROCm index) — single upgrade would break CUDA; use reinstall"
          : r.pinSource === "requirements"
            ? "Pinned by requirements.txt (" +
              (r.pinSpec || "tested set") +
              ") — single upgrade would deviate from the tested set; use restore"
            : "Kernel wheel managed via setup_config.json — use Sync Kernels / reinstall";
      pin.title += " Click to upgrade anyway (override).";
      pin.style.cursor = "pointer";
      pin.addEventListener("click", async function (ev) {
        ev.stopPropagation();
        var distName = r.dist || r.name;
        var isTorch =
          r.pinSource === "gpu-profile" && /torch/i.test(distName || "");
        var recovery =
          r.pinSource === "gpu-profile"
            ? isTorch
              ? "Recovery: reinstall (restore will NOT fix torch)."
              : "Recovery: reinstall."
            : r.pinSource === "kernel-wheel"
              ? "Recovery: Sync Kernels / reinstall."
              : "Recovery: restore from requirements.txt.";
        var why =
          r.pinSource === "gpu-profile"
            ? "is GPU-profile managed (PyTorch CUDA/ROCm index)"
            : r.pinSource === "requirements"
              ? "is pinned by requirements.txt (" +
                (r.pinSpec || "tested set") +
                ")"
              : "is a setup_config.json kernel wheel";
        if (
          !(await openPinOverride(
            "Upgrade " + (r.name || distName) + " to " + r.latest + "?",
            (r.name || distName) +
              " " +
              why +
              " — upgrading to " +
              r.latest +
              " may break the env.",
            recovery,
          ))
        )
          return;
        pin.textContent = "...";
        var res;
        try {
          res = await window.w2gp.upgradePackage(distName, true);
        } catch (e) {
          res = {
            success: false,
            error: (e && e.message) || String(e || "upgrade refused"),
          };
        }
        if (res && res.success) {
          showToast(
            "✓ " +
              (r.name || distName) +
              " upgraded to " +
              r.latest +
              " (override). " +
              recovery,
          );
          setTimeout(refreshDashboard, 2000);
        } else {
          pin.textContent = "pinned";
          showToast(
            "✗ Upgrade failed: " +
              (res && res.error ? res.error : "unknown error"),
          );
        }
      });
      latestSpan.after(pin);
      const dot = row.querySelector(".spec-dot");
      if (dot) {
        dot.classList.remove("installing", "has-update", "error");
        dot.classList.add("installed");
      }
      return;
    }
    if (r.installed && r.installed !== r.latest) {
      row.classList.add("has-update");
      row.classList.remove("up-to-date");
      updateCount++;
      const dot = row.querySelector(".spec-dot");
      if (dot) {
        dot.classList.remove("installed", "error", "installing");
        dot.classList.add("has-update");
      }
      const upBtn = document.createElement("button");
      upBtn.className = "spec-update-btn";
      upBtn.textContent = "↑";
      upBtn.title = "Upgrade " + r.name + " to " + r.latest;
      upBtn.addEventListener("click", async function (ev) {
        ev.stopPropagation();
        this.disabled = true;
        this.textContent = "...";
        if (dot) {
          dot.classList.remove("has-update", "installed", "error");
          dot.classList.add("installing");
        }
        var res;
        try {
          res = await window.w2gp.upgradePackage(r.dist || r.name);
        } catch (e) {
          res = {
            success: false,
            error: (e && e.message) || String(e || "upgrade refused"),
          };
        }
        if (res && res.success) {
          this.textContent = "✓";
          this.classList.add("done");
          if (dot) {
            dot.classList.remove("installing", "has-update", "error");
            dot.classList.add("installed");
          }
          showToast("✓ " + r.name + " upgraded to " + r.latest);
        } else {
          this.textContent = "↑";
          this.disabled = false;
          if (dot) {
            dot.classList.remove("installing", "has-update", "installed");
            dot.classList.add("error");
          }
          showToast(
            "✗ Upgrade failed: " +
              (res && res.error ? res.error : "unknown error"),
          );
        }
      });
      latestSpan.after(upBtn);
    } else {
      row.classList.add("up-to-date");
      row.classList.remove("has-update");
      // Clear stale dot state (e.g. 'error' from a failed upgrade) when the
      // check now reports the package is installed & current.
      const dot = row.querySelector(".spec-dot");
      if (dot) {
        dot.classList.remove("installing", "has-update", "error");
        dot.classList.add("installed");
      }
    }
  });
  showToast(
    updateCount > 0
      ? updateCount + " updates available"
      : "All packages up to date",
  );
});

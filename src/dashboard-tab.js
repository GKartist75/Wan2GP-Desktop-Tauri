// dashboard-tab.js — dashboard render: Active Environment, specs and package chips.
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

// ── Dashboard ──
let _dashRefreshing = false,
  _dashPending = false;
async function refreshDashboard() {
  if (_dashRefreshing) {
    _dashPending = true;
    return;
  }
  _dashRefreshing = true;
  try {
    // status / checkInstalled / manageList are independent — run them in one
    // batch instead of 3 sequential IPC round-trips (~2-6ms saved each, more
    // when the machine is under load from a running install).
    const [status, instRes, envs] = await Promise.all([
      window.w2gp.getStatus(),
      window.w2gp.checkInstalled().catch(() => null),
      window.w2gp.manageList().catch(() => []),
    ]);
    // Dashboard renderer switch: reflect the saved embedMode (same source as
    // Manage → Launch and the topbar quick-switch).
    try {
      const _cfg = await window.w2gp.configLoad().catch(() => ({}));
      const _et = $("embedModeTop");
      if (_et)
        _et.value = _cfg && _cfg.embedMode === "iframe" ? "iframe" : "native";
    } catch {}
    // Launch buttons only make sense when Wan2GP is actually installed
    try {
      // Launch buttons need a repo AND an active env — repo alone (failed
      // install, wiped env) used to launch system `python` into a torch traceback.
      setLaunchButtonsInstalled(
        !!(instRes && instRes.repo && status.env && !status.error),
      );
    } catch {}
    // Show a visible error note if the status call failed (so the panel is never
    // silently blank — this is exactly the blank-dashboard bug we hit before).
    const errNote = $("envDetailError");
    if (errNote) errNote.style.display = status.error ? "" : "none";
    if (status.error || !status.env) {
      if (errNote)
        errNote.textContent =
          "Could not read environment status: " +
          (status.error || "no active environment");
      $("envName").textContent = "No active environment";
      window._activeEnvName = "";
      window._activeEnvType = "";
      window._hasActiveEnv = false;
      $("envNameHint")?.classList.remove("hidden");
      document
        .querySelectorAll(
          ".pkg-install-btn, .spec-latest, .spec-update-btn, .spec-pinned",
        )
        .forEach((el) => {
          el.remove();
        });
      [
        "specPython",
        "specTorch",
        "specCuda",
        "specTriton",
        "specWgpk",
        "specSage",
        "specFlash",
        "specDiffusers",
        "specTransformers",
        "specGradio",
        "specAccelerate",
        "specOnnx",
        "specOpencv",
        "specPeft",
        "specHfhub",
        "specBits",
        "specNumpy",
        "specTokenizers",
        "specMmgp",
        "specXformers",
        "specTorchaudio",
        "specMoviepy",
        "specSparge",
        "specXatlas",
        "specFastSimp",
        "specSciImage",
        "specResvg",
        "specChumpy",
        "specSmplfitter",
      ].forEach((id) => {
        const el = $(id);
        if (el) el.textContent = "—";
      });
      [
        "dotPython",
        "dotTorch",
        "dotCuda",
        "dotTriton",
        "dotTritonHelper",
        "dotWgpk",
        "dotSage",
        "dotFlash",
        "dotDiffusers",
        "dotTransformers",
        "dotGradio",
        "dotAccelerate",
        "dotOnnx",
        "dotOpencv",
        "dotPeft",
        "dotHfhub",
        "dotBits",
        "dotNumpy",
        "dotTokenizers",
        "dotMmgp",
        "dotXformers",
        "dotTorchaudio",
        "dotMoviepy",
        "dotXatlas",
        "dotFastSimp",
        "dotSciImage",
        "dotResvg",
        "dotChumpy",
        "dotSmplfitter",
      ].forEach((id) => {
        const el = $(id);
        if (el) el.classList.remove("installed");
      });
      // Kernel wheels section is independent — keep it rendered from whatever we got.
      // Platforms without kernel wheels (Apple Silicon) hide the card entirely.
      if (window.w2gp?.capabilities?.kernelWheels !== false) {
        renderKernelWheels(
          status.kernelWheels,
          status.kernelProfile,
          status.osKey,
        );
      } else {
        const kwCard = $("kernelWheelsSubsection");
        if (kwCard) kwCard.style.display = "none";
      }
      const spargeEl = $("specSparge");
      if (spargeEl) spargeEl.textContent = "—";
    } else {
      $("envName").textContent = status.env.name;
      $("envType").textContent = status.env.type;
      window._activeEnvName = status.env.name || "";
      window._activeEnvType = status.env.type || "uv";
      window._hasActiveEnv = true;
      $("envNameHint")?.classList.add("hidden");
      // Clear old update/install buttons before re-creating
      document
        .querySelectorAll(
          ".spec-latest, .spec-update-btn, .pkg-install-btn, .spec-pinned",
        )
        .forEach((el) => {
          el.remove();
        });

      // AMD guard: CUDA / bitsandbytes / vanilla PyPI triton / vanilla
      // spas_sage_attn / PyPI sdist flash-attn break the TheRock env —
      // hide the one-click add button on AMD profiles with a tooltip
      // pointing at the guide recipe (backend refuses them too).
      // NVIDIA behavior is identical to before.
      var isAmdProfile =
        typeof status.kernelProfile === "string" &&
        status.kernelProfile.indexOf("AMD") === 0;
      var amdBlockedPkgs = [
        "bitsandbytes",
        "triton",
        "spas_sage_attn",
        "flash-attn",
      ];
      // Upstream INSTALLATION.md documents an exact optional pin for these;
      // install that version instead of whatever pip resolves today. The AMD
      // gate still sees the bare dist name (it strips the version pin).
      var pkgPin = { bitsandbytes: "bitsandbytes==0.49.2" };
      function setSpec(specId, dotId, val, pkgName) {
        const el = $(specId);
        if (el) el.textContent = val || "—";
        const dot = $(dotId);
        if (dot) {
          if (val) {
            dot.classList.remove("has-update", "error", "installing");
            dot.classList.add("installed");
          } else dot.classList.remove("installed");
        }
        // Show install button if package is missing and we know its pip name
        if (!val && pkgName && el) {
          // AMD: no one-click button for dists that break the TheRock
          // env (backend refuses them too) — tooltip notes the guide.
          if (isAmdProfile && amdBlockedPkgs.indexOf(pkgName) !== -1) {
            var parent0 = el.closest(".spec-row");
            if (parent0) {
              var old0 = parent0.querySelector(".pkg-install-btn");
              if (old0) old0.remove();
            }
            el.title =
              "Not available on AMD — see the docs/AMD-INSTALLATION.md guide recipe";
            return;
          }
          var parent = el.closest(".spec-row");
          if (parent) {
            var oldBtn = parent.querySelector(".pkg-install-btn");
            if (oldBtn) oldBtn.remove();
            var btn = document.createElement("button");
            btn.className = "pkg-install-btn";
            btn.textContent = "+";
            var instSpec = pkgPin[pkgName] || pkgName;
            btn.title = "Install " + instSpec;
            btn.addEventListener("click", async function (ev) {
              ev.stopPropagation();
              this.disabled = true;
              this.textContent = "...";
              var res = await window.w2gp.installPackage(instSpec);
              if (res && res.success) {
                this.textContent = "✓";
                this.classList.add("done");
                setTimeout(refreshDashboard, 2000);
              } else {
                this.textContent = "+";
                this.disabled = false;
                showToast(
                  "✗ Install failed: " +
                    (res && res.error ? res.error : "unknown"),
                );
              }
            });
            el.after(btn);
          }
        }
      }
      // If the version query itself failed, show the reason in the note but keep
      // the wheels/paths sections alive (they're independent of the version scan).
      if (status.versions && status.versions.error) {
        const errNote = $("envDetailError");
        if (errNote) {
          errNote.style.display = "";
          errNote.textContent = "Package scan failed: " + status.versions.error;
        }
      }
      setSpec("specPython", "dotPython", status.versions?.python);
      setSpec("specTorch", "dotTorch", status.versions?.torch);
      const m = (status.versions?.torch || "").match(/cu(\d+)/);
      setSpec("specCuda", "dotCuda", m ? `CUDA ${m[1]}` : null);
      setSpec("specTriton", "dotTriton", status.versions?.triton, "triton");
      setSpec("specWgpk", "dotWgpk", status.versions?.wgpkernels, "wgpkernels");
      setSpec(
        "specSage",
        "dotSage",
        status.versions?.sageattention || status.versions?.spas_sage_attn,
        "spas_sage_attn",
      );
      setSpec(
        "specFlash",
        "dotFlash",
        status.versions?.flash_attn,
        "flash-attn",
      );
      setSpec("specDiffusers", "dotDiffusers", status.versions?.diffusers);
      setSpec(
        "specTransformers",
        "dotTransformers",
        status.versions?.transformers,
      );
      setSpec("specGradio", "dotGradio", status.versions?.gradio);
      setSpec("specAccelerate", "dotAccelerate", status.versions?.accelerate);
      setSpec("specOnnx", "dotOnnx", status.versions?.onnxruntime);
      setSpec("specOpencv", "dotOpencv", status.versions?.["opencv-python"]);
      setSpec("specPeft", "dotPeft", status.versions?.peft);
      setSpec("specHfhub", "dotHfhub", status.versions?.huggingface_hub);
      setSpec(
        "specBits",
        "dotBits",
        status.versions?.bitsandbytes,
        "bitsandbytes",
      );
      setSpec("specNumpy", "dotNumpy", status.versions?.numpy);
      setSpec("specTokenizers", "dotTokenizers", status.versions?.tokenizers);
      setSpec("specMmgp", "dotMmgp", status.versions?.mmgp);
      setSpec("specXformers", "dotXformers", status.versions?.xformers);
      setSpec("specTorchaudio", "dotTorchaudio", status.versions?.torchaudio);
      setSpec("specMoviepy", "dotMoviepy", status.versions?.moviepy);
      setSpec("specXatlas", "dotXatlas", status.versions?.xatlas);
      setSpec(
        "specFastSimp",
        "dotFastSimp",
        status.versions?.["fast-simplification"],
      );
      setSpec(
        "specSciImage",
        "dotSciImage",
        status.versions?.["scikit-image"],
      );
      setSpec("specResvg", "dotResvg", status.versions?.["resvg-py"]);
      setSpec("specChumpy", "dotChumpy", status.versions?.chumpy);
      setSpec("specSmplfitter", "dotSmplfitter", status.versions?.smplfitter);

      // ── GPU Kernel Wheels (profile-driven) ──
      renderKernelWheels(
        status.kernelWheels,
        status.kernelProfile,
        status.osKey,
      );
      // Sparge Attn comes from the expected GPU profile (not a detected version),
      // so it's surfaced here to avoid a separate duplicate "GPU Profile Overview".
      const spargeEl = $("specSparge");
      // Installed version wins; otherwise the profile's expected value (the
      // backend maps the raw setup_config tag, e.g. v010_cu13 → 0.1.0). Never
      // fall back to the GPU profile name — that printed "RTX_40" in this row.
      if (spargeEl)
        spargeEl.textContent =
          status.versions?.spas_sage_attn ||
          status.versions?.sparge ||
          (status.profile && status.profile.sparge) ||
          "—";
      // Live INT8/FP8 backend chips are runtime state from the launch log, not
      // version probes — repaint them so a refresh never blanks a live value.
      try {
        if (typeof _paintBackendChips === "function") _paintBackendChips();
      } catch {}
    }
    // ponytail: batch DOM swap to avoid flicker — build fragment then single replace
    const list = $("envList");
    const frag = document.createDocumentFragment();
    envs.forEach((e) => {
      const div = document.createElement("div");
      div.className = "env-list-item" + (e.active ? " active" : "");
      {
        const dot = document.createElement("span");
        dot.className = "env-dot";
        const nm = document.createElement("span");
        nm.className = "env-list-name";
        nm.textContent = e.name;
        const ty = document.createElement("span");
        ty.style.cssText = "font-size:0.65rem;color:#666;flex-shrink:0";
        ty.textContent = e.type;
        div.append(dot, nm, ty);
      }
      if (!e.active) {
        div.setAttribute("role", "button");
        div.tabIndex = 0;
        const activate = async () => {
          await window.w2gp.manageSetActive(e.name);
          refreshDashboard();
        };
        div.addEventListener("click", activate);
        div.addEventListener("keydown", (ev) => {
          if (ev.key === "Enter" || ev.key === " ") {
            ev.preventDefault();
            activate();
          }
        });
      }
      frag.appendChild(div);
    });
    list.innerHTML = "";
    // Single-env installs have nothing to switch: the Active Environment
    // card already shows it. Only render the switcher when a choice exists.
    if (Array.isArray(envs) && envs.length > 1) {
      list.appendChild(frag);
      list.style.display = "";
    } else {
      list.style.display = "none";
    }
    loadWangpChangelog();
    loadPaths();
    loadModelPaths();
    document.querySelectorAll(".env-detail .spec-row").forEach((r) => {
      r.classList.remove("has-update", "up-to-date");
    });
    $("checkPkgUpdatesBtn").textContent = "↻ Check Updates";
    $("checkPkgUpdatesBtn").disabled = false;
    refreshEnvUnlink(!!(instRes && instRes.repo));
    // Warn if model checkpoints/LoRAs still live in a roaming AppData profile.
    checkModelsPathWarning();
    // Warn RTX 40/50 users still on the broken fp8 SageAttention wheel to sync.
    checkSageSyncBanner(status);
    // Refresh the guided LLM engine cards (Deepy Prime setup).
    refreshLLMEngines().catch(() => {});
    // Refresh the Deepy Prime activation panel.
    refreshDeepy().catch(() => {});
    // Refresh the Deepy Web standalone card (Same-PC + Phone-LAN HTTP).
    refreshDeepyWeb().catch(() => {});
    // Refresh the DLSS5 optional-runtime status.
    refreshDlss5().catch(() => {});
    // Refresh the prebuilt Triton helper integrity (AV quarantine guard).
    setTritonHelper().catch(() => {});
    // Enable/disable no-GPU button based on Chrome availability. A single
    // negative is never trusted for failure UI: a cold first spawn (AV hooks,
    // process-creation stalls) can fail once and would flash "not installed"
    // for a second. Re-probe immediately — probes are synchronous file checks
    // plus `where`, so this costs milliseconds. Only a repeated negative
    // disables the button and shows the hint. IPC errors leave UI untouched.
    (async () => {
      const probe = async () => {
        try {
          if (window.w2gp.noGpuAvailable)
            return await window.w2gp.noGpuAvailable();
          return await window.w2gp.chromeAvailable();
        } catch {
          return null;
        }
      };
      let available = await probe();
      if (available === false) available = await probe();
      if (available === null) return;
      // Flake guard: a single negative probe (common at cold start) must not
      // flash "no browser installed" — show only after 2 consecutive misses.
      window._noGpuMissCount =
        available === false ? (window._noGpuMissCount || 0) + 1 : 0;
      const noBrowser = available === false && window._noGpuMissCount >= 2;
      if (noBrowser)
        appendLog(
          "[!] No-GPU browser probe: none found twice (No-GPU launches disabled)",
        );
      else if (available === true && window._noGpuWasMissing)
        appendLog(
          "[*] No-GPU browser probe: found on re-probe (first probe flaked)",
        );
      window._noGpuWasMissing = available === false;
      for (const id of ["browserNoGpuBtn", "termNoGpuBtn"]) {
        const btn = $(id);
        if (btn) btn.disabled = noBrowser;
      }
      const hint = $("noGpuHint");
      if (hint) hint.style.display = noBrowser ? "block" : "none";
    })();
    // Self-healing first-launch info bar: repaint from STATE on every
    // dashboard refresh so any runtime path that left it hidden (fresh load
    // never shows it; exit/stop paths don't restore it) is corrected.
    // Instant feedback still comes from the explicit show/hide calls at
    // launch-click/error/ready sites — this only corrects drift.
    paintLaunchInfo();
  } finally {
    _dashRefreshing = false;
    if (_dashPending) {
      _dashPending = false;
      setTimeout(refreshDashboard, 80);
    }
  }
}

// ── Prebuilt Triton helper integrity (v18.18 / fde3c7e) ──
// Upstream ships checksum-verified helper binaries in-repo
// (shared/triton_runtime/) as the Bitdefender workaround; unsigned `.pyd`
// binaries are prime AV-quarantine targets, and a stripped helper silently
// falls back to the JIT compile path. Verified on every dashboard refresh
// alongside the other environment facts — green only when presence AND
// manifests' own binary_sha256 both check out.
async function setTritonHelper() {
  const el = $("specTritonHelper"),
    dot = $("dotTritonHelper");
  if (!el) return;
  let s = null;
  try {
    s = await window.w2gp.tritonRuntimeStatus();
  } catch {}
  if (!s || s.ok === false) {
    el.textContent = "—";
    el.title = "";
    if (dot) dot.classList.remove("installed", "error", "has-update");
    return;
  }
  el.textContent = s.summary || s.state || "—";
  if (dot) {
    dot.classList.remove("installed", "error", "has-update");
    if (s.state === "intact") dot.classList.add("installed");
    else if (s.state === "missing" || s.state === "modified")
      dot.classList.add("error");
    // "absent" (pre-fde3c7e repo) stays dotless until they update.
  }
  const rows = Array.isArray(s.rows) ? s.rows : [];
  const parts = rows.map(
    (r) =>
      r.label +
      ": " +
      (r.missing
        ? "missing"
        : r.modified
          ? "modified — hash mismatch"
          : "intact" + (r.builtFrom && r.builtFrom !== "?" ? " (" + r.builtFrom + ")" : "")),
  );
  if (s.engager === false) parts.push("Engager (__init__.py): missing");
  el.title = (s.hint ? s.hint + "\n\n" : "") + parts.join("\n");
}

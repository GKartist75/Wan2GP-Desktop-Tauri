// init-tab.js — app bootstrap: overlay open/close and first-run wiring.
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

// ── Init ──
document.addEventListener("DOMContentLoaded", async () => {
  try {
    // ponytail: keep splash visible while loading — covers WebView2 + python scan (was showing empty dashboard)
    $("splashStatus").textContent = "Loading...";
    setDesktopUpdateIndicator(false);
    const [installed, cfgPreload] = await Promise.all([
      window.w2gp.checkInstalled(),
      window.w2gp.configLoad().catch(() => ({})),
    ]);
    await checkCrashRecovery();

    window.w2gp.getDesktopVersion().then((v) => {
      if (!v) return;
      document.title = "Wan2GP Desktop Launcher v" + v;
      var verEl = $("settingsVersionNum");
      if (verEl) verEl.textContent = v;
      var appVerEl = $("appVersionTag");
      if (appVerEl) appVerEl.textContent = "v" + v;
    });
    setupScrollUnfollow("termBody", "dashTermFollowBtn");
    setupScrollUnfollow("installTermBody", "installFollowBtn");

    window.w2gp.onSetupOutput((t) =>
      appendLog(
        t.replace(/\x1b\[[0-9;?]*[a-zA-Z]/g, "").replace(/\x08/g, ""),
        false,
      ),
    );
    window.w2gp.onConsoleCleared(onRemoteConsoleCleared);
    window.w2gp.onDlss5Progress(dlss5OnEvent);
    window.w2gp.onInstallProgress(installProgressOnEvent);

    window.w2gp.onLaunchLog((t) => {
      const clean = t
        .replace(/\x1b\[[0-9;?]*[a-zA-Z]/g, "")
        .replace(/\x08/g, "");
      appendLog(clean, false);
      // Console-first launch: stay on the dashboard while starting, open the
      // destination the moment the backend reports ready.
      if (_pendingOpen && /Wan2GP ready/.test(clean)) {
        const p = _pendingOpen;
        _pendingOpen = null;
        if (p.kind === "desktop") openDesktopView(p.url, true);
        else
          openBrowserView(p.url, p.noGpu).catch((e) =>
            appendLog(`[LAUNCH ERROR] ${errText(e)}`),
          );
      }
    });
    window.w2gp.onSetupPhase((p) => {
      if (p.done) {
        if (prevPhaseId && prevPhaseId !== p.id) taskComplete(prevPhaseId);
        taskComplete(p.id);
        prevPhaseId = null;
      } else {
        if (prevPhaseId && prevPhaseId !== p.id) taskComplete(prevPhaseId);
        taskStart(p.id);
        appendLog("[*] " + p.label);
        prevPhaseId = p.id;
      }
    });
    window.w2gp.onSetupProfile((p) => {
      $("installProfile").textContent = p;
      $("installProfileRow").style.display = "flex";
    });

    const cfg =
      cfgPreload || (await window.w2gp.configLoad().catch(() => ({})));
    if (cfg.themeFollowSystem)
      applyTheme(
        matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light",
      );
    else if (cfg.theme === "dark") applyTheme("dark");
    loadAppear(cfg);
    initAppearControls(); // top-bar palette + A± need no settings panel
    // System theme follow (real matchMedia — backend only persists the preference).
    // The native onSystemThemeChange event never fires in Tauri; this is the mechanism.
    if (!window.__themeFollowBound) {
      window.__themeFollowBound = true;
      matchMedia("(prefers-color-scheme: dark)").addEventListener(
        "change",
        async () => {
          try {
            const c = await window.w2gp.configLoad();
            if (c.themeFollowSystem)
              applyTheme(
                matchMedia("(prefers-color-scheme: dark)").matches
                  ? "dark"
                  : "light",
              );
          } catch {}
        },
      );
    }

    // Embedded-Wan2GP view crashed and was auto-reloaded by the main process.
    window.w2gp.onBvCrashRecovered(() =>
      showToast("Wan2GP view reloaded after a crash"),
    );

    // hardware probe — fire-and-forget, fills specs when ready (no await)
    loadHardware()
      .then((s) => {
        if (s)
          appendLog(
            `[*] Hardware: ${s.cpu || "?"} · ${s.ram || "?"} RAM · ${s.gpu || "?"} (${s.vram || "?"})`,
          );
      })
      .catch((e) => {
        console.warn("[hw] detectHardware failed", e);
      });
    setTimeout(() => loadHardware().catch(() => {}), 2000);

    window.w2gp
      .getDesktopVersion()
      .then((v) => {
        if (v)
          appendLog(
            `[*] Launcher v${v} ready — dashboard live. Metrics polling every 3s; update checks in background.`,
          );
      })
      .catch(() => {});

    if (installed.repo && installed.env) {
      // ponytail: fast paint — show dashboard instantly (~300ms), fill versions in background
      appendLog("[*] Wan2GP install found — loading dashboard…");
      show("dashboard");
      refreshDashboard()
        .then(() => {
          const env = $("envName")?.textContent?.trim() || "—";
          const torch = $("specTorch")?.textContent?.trim() || "—";
          appendLog(`[*] Environment ready: ${env} · torch ${torch}`);
          // One-time startup drift sync: the banner otherwise reflects only
          // the last update's verdict — re-verify against the live env so a
          // stale banner clears (or real drift names itself) unprompted.
          window.w2gp
            .depCheck()
            .then((d) => {
              if (d && Array.isArray(d.drift) && d.drift.length) {
                appendLog(
                  "[!] dependency drift: " + d.drift.join(", ") + " — use restore",
                );
                showDriftBanner(d.drift);
              } else hideDriftBanner();
            })
            .catch(() => {});
        })
        .catch(() => {});
      startMetricsPolling();
      startDownloadsWatch();
      // Periodic Wan2GP update re-check while the app is open (30 min) + Desktop (5h).
      // Launch-time check alone misses updates released mid-session; the
      // renderer-side timers re-poll and re-flag the green dot + changelog.
      startWangpPolling();
      startDeepyWebPolling();
      startMainLanPolling();
      startLogServerPolling();
      refreshLogServer().catch(() => {});
      // Restore an enabled log viewer across restarts (opt-in).
      try {
        const cfg = await window.w2gp.configLoad().catch(() => ({}));
        if (cfg && cfg.logServerEnabled) {
          await window.w2gp.logServerStart(cfg.logPort || null, !!cfg.logLan).catch(() => {});
          refreshLogServer().catch(() => {});
        }
      } catch {}
      startDesktopPolling();
      // (Wan2GP polls immediately at boot; Desktop does its early check
      // 8s after boot inside startDesktopPolling.)
      // D1: silent settings auto-scan (issue #7 class) — out-of-range dropdown
      // values make Wan2GP reject the whole settings form on save; repair them
      // in the background so the user never hits the "can't save" wall. Writes
      // only when a fix is actually found (console log + toast otherwise quiet).
      silentSettingsRepair();
    } else {
      $("splashStatus").textContent = "First-time setup...";
      appendLog(
        "[*] First run — no Wan2GP install detected. Complete the installer below to set up.",
      );
      // External drive disconnected or letter changed (e.g. J:\WanGPApp was there
      // and now isn't)? Say so explicitly instead of a blank "first run".
      if (installed && installed.missingPrevious) {
        appendLog(
          "[!] Previous install not found: " + installed.missingPrevious,
        );
        appendLog(
          "[!] If that is an external drive, reconnect it (check the drive letter) and restart the launcher — or install fresh / pick the new location below.",
        );
        $("installSubtitle").textContent =
          "Previous install at " +
          installed.missingPrevious +
          " is missing — reconnect the drive, or set up again below.";
        try {
          showToast(
            "⚠ Previous install folder missing — reconnect the drive or reinstall",
          );
        } catch {}
      }
      const hw = await window.w2gp.detectHardware();
      $("installCpu").textContent = hw.cpu || "—";
      $("installRam").textContent = hw.ram || "—";
      $("installGpu").textContent = hw.gpu || "—";
      $("installVram").textContent = hw.vram || "—";
      loadPaths();
      try {
        const mf = await window.w2gp.detectModelFolders();
        if (mf.checkpointsPaths && mf.checkpointsPaths.length) {
          _modelCkpts = mf.checkpointsPaths[0];
          $("installCkptsPath").textContent = _modelCkpts;
        }
        if (mf.lorasRoot) {
          _modelLoras = mf.lorasRoot;
          $("installLorasPath").textContent = _modelLoras;
        }
      } catch {}
      show("installer");
      if (!(installed && installed.missingPrevious))
        $("installSubtitle").textContent =
          "Select environment type, then click Install";
      // Target-folder triage: ATFGriff's J:\\WanGPApp wasn't empty (Pinokio? previous
      // attempt?) and we merged blindly over it. Show what's there first.
      refreshTargetVerdict().catch(() => {});
      refreshModelDiskGates().catch(() => {});
      $("installStartBtn").classList.remove("hidden");
      $("envTypeSelect").classList.remove("disabled");
      document
        .querySelectorAll(".env-type-btn")
        .forEach((b) => (b.disabled = false));
      // Show expected packages for this hardware
      window.w2gp.getHardwareProfile().then((hp) => {
        if (!hp) return;
        var list = $("installPkgsList");
        var header = $("installPkgsProfile");
        if (list && hp.packages && hp.packages.length) {
          if (header)
            header.textContent = "(" + hp.profile.replace(/_/g, " ") + ")";
          list.textContent = "";
          for (const p of hp.packages) {
            const s = document.createElement("span");
            s.className = "ipkg-item";
            s.textContent = p;
            list.append(s);
          }
          $("installPkgs").style.display = "";
        }
        // Distinct kernel-wheels group (so the wheels are clearly visible pre-install)
        var klist = $("installKernelsList");
        var kheader = $("installKernelsProfile");
        if (klist && hp.kernels && hp.kernels.length) {
          if (kheader)
            kheader.textContent = "(" + hp.profile.replace(/_/g, " ") + ")";
          klist.textContent = "";
          for (const k of hp.kernels) {
            const row = document.createElement("div");
            row.className = "ikernel-item";
            const lab = document.createElement("span");
            lab.className = "ikernel-label";
            lab.textContent = k.label;
            const dist = document.createElement("span");
            dist.className = "ikernel-dist";
            dist.textContent = k.dist;
            row.append(lab, dist);
            klist.append(row);
          }
          $("installKernels").style.display = "";
        }
        // GPU Profile Overview — installer only (different screen; the dashboard
        // consolidates detected versions + kernel wheels into the env_uv card).
        renderProfileOverview(hp.detail, {
          box: "installProfileOverview",
          profile: "ipoProfile",
          python: "ipoPython",
          torch: "ipoTorch",
          triton: "ipoTriton",
          sage: "ipoSage",
          sparge: "ipoSparge",
          flash: "ipoFlash",
          kernels: "ipoKernels",
        });
      });
      // Pre-flight resolved stack: GPU/CUDA/driver/disk gates + exact Python pin.
      // (Tauri install_plan shape: { plan: {gpuName,vendor,cuda,torch,driverWarning,profile}, disk: {free,total} }.)
      window.w2gp
        .installPlan()
        .then((r) => {
          if (!r || !r.plan) return;
          const grid = $("installStackGrid");
          const warn = $("installStackWarn");
          const stack = $("installStack");
          if (!grid) return;
          const p = r.plan;
          const freeBytes = r.disk && r.disk.free != null ? r.disk.free : null;
          const freeGb =
            freeBytes == null ? "?" : (freeBytes / 1073741824).toFixed(1);
          const rows = [
            ["GPU", p.gpuName || p.vendor],
            ["CUDA build", p.cuda],
            ["PyTorch", p.torch],
            ["Profile", (p.profile || "").replace(/_/g, " ")],
            ["Free disk", freeGb + " GB"],
          ];
          const renderRows = () => {
            grid.textContent = "";
            for (const row of rows) {
              const d = document.createElement("div");
              d.className = "istack-row"; // rebased: keep master naming (branch had `row`, identical DOM-safe code)
              const k = document.createElement("span");
              k.className = "istack-k";
              k.textContent = row[0];
              const v = document.createElement("span");
              v.className = "istack-v";
              v.textContent = row[1];
              d.append(k, v);
              grid.append(d);
            }
          };
          renderRows();
          // Exact Python pin setup.py will demand via `uv venv --python X`
          // (pythonPreflight is check-only — the download happens on Install).
          window.w2gp
            .pythonPreflight()
            .then((pf) => {
              if (!pf || !pf.wanted) return;
              const uvTag = pf.uvVersion
                ? " (" + pf.uvVersion.split(" ").slice(0, 2).join(" ") + ")"
                : "";
              const state = pf.uvVersion
                ? pf.path && pf.runs
                  ? "✓ " + pf.wanted + " ready"
                  : pf.path
                    ? "⚠ " +
                      pf.wanted +
                      " corrupted — auto-reinstall on Install"
                    : "⬇ " + pf.wanted + " — auto-download on Install"
                : "✗ uv not found";
              rows.push(["Python" + uvTag, state]);
              renderRows();
              if (pf.hint && warn)
                warn.innerHTML +=
                  '<div class="istack-hint">' + escHtml(pf.hint) + "</div>";
            })
            .catch(() => {});
          const warns = [];
          if (p.driverWarning) warns.push(p.driverWarning);
          if (freeBytes != null && freeBytes < 10 * 1073741824)
            warns.push(
              "Only " +
                freeGb +
                " GB free — 50+ GB recommended (models are tens–hundreds of GB).",
            );
          if (warn) {
            warn.textContent = "";
            for (const w of warns) {
              const d = document.createElement("div");
              d.className = "istack-w";
              d.textContent = "⚠ " + w;
              warn.append(d);
            }
            if (
              freeBytes != null &&
              freeBytes >= 10 * 1073741824 &&
              freeBytes < 50 * 1073741824
            ) {
              const d = document.createElement("div");
              d.className = "istack-hint";
              d.textContent =
                freeGb +
                " GB free is tight — models alone can exceed 50 GB. A non-system drive is recommended.";
              warn.append(d);
            }
          }
          stack.style.display = "";
          // Hard block only when install can't succeed (cu130 driver too old, or ~no disk).
          const startBtn = $("installStartBtn");
          const hardBlocked =
            /R580/.test(p.driverWarning || "") ||
            (freeBytes != null && freeBytes < 10 * 1073741824);
          if (startBtn && hardBlocked) {
            startBtn.disabled = true;
            startBtn.title = "Resolve the warnings above before installing";
            startBtn.textContent = "Install blocked — see warnings";
          }
        })
        .catch(() => {});
    }
  } catch (e) {
    const el = $("splashError");
    if (el) {
      el.textContent = e.stack || String(e);
      el.classList.remove("hidden");
    }
    $("splashStatus").textContent = "Startup error";
  }
});

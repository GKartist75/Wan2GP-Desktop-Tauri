// downloads-tab.js — Installer live downloads (per-file rows from uv output).
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

// ── Installer live downloads (per-file rows from uv output) ──
const _installDl = new Map();
let _installDlDone = 0,
  _installDlPaint = 0,
  _installDlQueued = false;
function installProgressReset() {
  _installDl.clear();
  _installDlDone = 0;
  _installDlPaint = 0;
  _installDlQueued = false;
  const box = $("installProgress");
  if (box) box.style.display = "none";
  const list = $("installProgressList");
  if (list) list.innerHTML = "";
  const cnt = $("installProgressCount");
  if (cnt) cnt.textContent = "";
}
function installProgressOnEvent(d) {
  if (!d || !d.phase) return;
  if (d.phase === "downloading" && d.pkg) {
    const r = _installDl.get(d.pkg) || {
      size: "",
      state: "downloading",
      version: "",
    };
    if (!r.size && d.size) r.size = d.size;
    r.state = "downloading";
    _installDl.set(d.pkg, r);
  } else if (d.phase === "package-installed" && d.pkg) {
    const r = _installDl.get(d.pkg) || {
      size: "",
      state: "installed",
      version: "",
    };
    r.state = "installed";
    if (d.version) r.version = d.version;
    _installDl.set(d.pkg, r);
    _installDlDone++;
  } else if (d.phase === "resolved") {
    // count shown via map size; nothing to store
  } else return;
  // Throttle paints: full innerHTML re-render restarts the bar animation.
  const now = Date.now();
  if (now - _installDlPaint < 500) {
    if (!_installDlQueued) {
      _installDlQueued = true;
      setTimeout(() => {
        _installDlQueued = false;
        installProgressPaint();
      }, 500);
    }
    return;
  }
  installProgressPaint();
}
function installProgressPaint() {
  _installDlPaint = Date.now();
  const box = $("installProgress"),
    list = $("installProgressList");
  if (!box || !list || !_installDl.size) return;
  box.style.display = "";
  const names = [..._installDl.keys()].slice(-40);
  list.textContent = "";
  for (const n of names) {
    const r = _installDl.get(n);
    const row = document.createElement("div");
    row.className = "iprog-row";
    const top = document.createElement("div");
    top.className = "iprog-top";
    const nm = document.createElement("span");
    nm.className = "iprog-name";
    nm.title = n;
    nm.textContent = n;
    const st = document.createElement("span");
    if (r.state === "installed") {
      st.className = "iprog-state installed";
      st.textContent = "✓ " + (r.version || "done");
    } else {
      st.className = "iprog-state downloading";
      st.textContent = "⬇ " + (r.size || "…");
    }
    top.append(nm, st);
    const bar = document.createElement("div");
    bar.className = "iprog-bar" + (r.state === "installed" ? " done" : "");
    bar.append(document.createElement("div"));
    row.append(top, bar);
    list.append(row);
  }
  const cnt = $("installProgressCount");
  if (cnt)
    cnt.textContent = _installDlDone + "/" + _installDl.size + " installed";
}

function dlss5OnEvent(d) {
  if (!d || !d.phase) return;
  if (d.phase === "downloading" && d.pkg && d.pkg !== "other") {
    for (const f of _dlss5Rows)
      if (f.pkg === d.pkg) _dlss5State[f.id] = "downloading";
    _dlss5LastPkg = d.pkg;
  } else if (d.phase === "verified" && _dlss5LastPkg) {
    for (const f of _dlss5Rows)
      if (f.pkg === _dlss5LastPkg) _dlss5State[f.id] = "verified";
  } else if ((d.phase === "installed" || d.phase === "present") && d.path) {
    _dlss5State[String(d.path).replace(/\\/g, "/")] = "verified";
  } else if (d.phase === "done") {
    _dlss5Done = true;
  } else return;
  renderDlss5Progress();
}
$("dlss5ConfirmBtn")?.addEventListener("click", async () => {
  _dlss5LastPkg = null;
  _dlss5State = {};
  _dlss5Done = false;
  renderDlss5Progress();
  $("dlss5Modal").classList.add("hidden");
  const force = !!$("dlss5ForceChk")?.checked;
  const btn = $("dlss5InstallBtn");
  btn.disabled = true;
  const orig = btn.textContent;
  btn.textContent = "Installing…";
  appendLog("[*] Installing DLSS 5 runtime — progress below…");
  try {
    const r = await window.w2gp.installDlss5(force);
    if (r && r.ok)
      showToast(
        r.complete
          ? "✓ DLSS 5 installed — restart Wan2GP"
          : "⏳ " + (r.hint || "Partial install — see console"),
      );
    else showToast("✗ " + ((r && r.error) || "install failed"));
  } catch (e) {
    showToast("✗ " + e.message);
  } finally {
    btn.disabled = false;
    btn.textContent = orig;
    refreshDlss5();
  }
});
for (const id of ["dlss5DocsLink", "dlss5DocsLink2"]) {
  const a = $(id);
  if (a)
    a.onclick = async (ev) => {
      ev.preventDefault();
      await window.w2gp.openExternal(
        "https://github.com/deepbeepmeep/Wan2GP/blob/main/docs/DLSS5.md",
      );
    };
}

$("llmEnginesRefresh")?.addEventListener("click", () => {
  _llmEnginesPromise = null;
  refreshLLMEngines();
});

$("desktopShortcutBtn").addEventListener("click", async function () {
  this.disabled = true;
  this.textContent = "Creating...";
  const r = await window.w2gp.createDesktopShortcut();
  this.disabled = false;
  this.textContent = "Create Desktop Shortcut";
  if (r && r.success) {
    showToast("✓ Shortcut created on desktop: Launch Wan2GP.bat");
  } else {
    showToast("✗ " + (r && r.error ? r.error : "Failed to create shortcut"));
  }
});


// Dock buttons (always visible)
document.querySelectorAll(".dock-btn").forEach((btn) => {
  btn.addEventListener("click", () => setFtDock(btn.dataset.dock));
});
// Events coming from the floating-terminal overlay (its own BrowserView, used for 'floating' dock)
window.w2gp.onTermDockChanged((dock) => {
  const ft = $("floatingTerminal");
  ft.className =
    "floating-term dock-" +
    dock +
    (ft.classList.contains("hidden") ? " hidden" : "");
  if (dock !== "floating") ft.style.cssText = "";
  document
    .querySelectorAll(".dock-btn")
    .forEach((b) => b.classList.toggle("active", b.dataset.dock === dock));
  window.w2gp.bvSetDock(dock);
  if (_ftVisible) showTerminal();
});
window.w2gp.onTermClosed(() => {
  _ftVisible = false;
  hideTerminal();
});
// Floating drag for dock-floating mode
let _fdrag = null;
$("floatingTerminal").addEventListener("mousedown", (e) => {
  if (!$("floatingTerminal").classList.contains("dock-floating")) return;
  if (e.target.closest(".term-btn-small, .dock-menu")) return;
  const r = $("floatingTerminal").getBoundingClientRect();
  _fdrag = {
    dx: e.clientX - r.left,
    dy: e.clientY - r.top,
    w: r.width,
    h: r.height,
  };
  document.addEventListener("mousemove", _fdragMove);
  document.addEventListener("mouseup", _fdragEnd);
});
function _fdragMove(e) {
  if (!_fdrag) return;
  const p = $("floatingTerminal");
  let x = e.clientX - _fdrag.dx,
    y = e.clientY - _fdrag.dy;
  x = Math.max(0, Math.min(x, window.innerWidth - _fdrag.w));
  y = Math.max(0, Math.min(y, window.innerHeight - 30));
  p.style.left = x + "px";
  p.style.top = y + "px";
  p.style.right = "auto";
  p.style.bottom = "auto";
}
function _fdragEnd() {
  _fdrag = null;
  document.removeEventListener("mousemove", _fdragMove);
  document.removeEventListener("mouseup", _fdragEnd);
}
// Follow toggle
$("ftFollowBtn").addEventListener("click", () => {
  termFollow.ftTermBody = !termFollow.ftTermBody;
  const b = $("ftFollowBtn");
  b.classList.toggle("active");
  const ft = b.querySelector(".follow-text");
  if (ft) ft.textContent = termFollow.ftTermBody ? "Follow" : "Paused";
  if (termFollow.ftTermBody) {
    const e = $("ftTermBody");
    if (e) setTimeout(() => (e.scrollTop = e.scrollHeight), 10);
  }
});
// Keyboard shortcut: Ctrl+` toggles floating terminal
// NOTE: the real shortcut lives in the Keyboard-shortcuts handler below
// (Ctrl+` / Escape / Ctrl+W). This duplicate copy fired on the SAME keypress,
// calling toggleFloatingTerm() twice — open then instantly close — so the
// shortcut looked dead in webview mode. Removed to avoid the double toggle.

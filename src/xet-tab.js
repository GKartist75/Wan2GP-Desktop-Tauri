// xet-tab.js — Xet Storage (hf_xet).
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

// ── Xet Storage (hf_xet) ──
// Minimal PEP 440 subset for dotted numeric releases (1.6.0 vs >=1.5.2).
// True when the requirement is empty/unparseable (display only, no verdict).
function versionSatisfies(installed, required) {
  if (!installed || !required) return true;
  const m = String(required).match(
    /^(==|>=|<=|~=|!=|>|<)\s*([0-9][0-9A-Za-z.\-_]*)/,
  );
  if (!m) return true;
  const num = (v) =>
    String(v).split(".").map((p) => {
      const n = parseInt(p, 10);
      return Number.isFinite(n) ? n : 0;
    });
  const a = num(installed);
  const b = num(m[2]);
  let cmp = 0;
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const x = a[i] || 0;
    const y = b[i] || 0;
    if (x !== y) {
      cmp = x < y ? -1 : 1;
      break;
    }
  }
  switch (m[1]) {
    case "==":
      return cmp === 0;
    case "!=":
      return cmp !== 0;
    case ">=":
    case "~=":
      return cmp >= 0;
    case "<=":
      return cmp <= 0;
    case ">":
      return cmp > 0;
    case "<":
      return cmp < 0;
    default:
      return true;
  }
}
async function updateXetStatus() {
  const btn = $("xetInstallBtn");
  const status = $("xetStatus");
  if (!btn || !status) return;
  try {
    const r = await window.w2gp.checkPackage("hf_xet");
    const ver = (r && r.version) || "";
    const req = (r && r.required) || "";
    const reqNote = req ? " (requires " + req + ")" : "";
    if (r && r.installed) {
      if (versionSatisfies(ver, req)) {
        status.textContent =
          "installed" + (ver ? " " + ver : "") + reqNote;
        status.style.color = "var(--signal-green)";
        btn.textContent = "Uninstall hf_xet";
      } else {
        status.textContent =
          "installed " + ver + " — outdated" + reqNote;
        status.style.color = "var(--signal-red)";
        btn.textContent = "Update hf_xet";
      }
    } else {
      status.textContent = "not installed" + reqNote;
      status.style.color = "var(--text-tertiary)";
      btn.textContent = "Install hf_xet";
    }
  } catch {
    status.textContent = "error checking";
    status.style.color = "var(--signal-red)";
  }
}

$("xetInstallBtn")?.addEventListener("click", async function () {
  this.disabled = true;
  const status = $("xetStatus");
  if (status) status.textContent = "working...";
  try {
    let r;
    if (this.textContent.startsWith("Uninstall")) {
      r = await window.w2gp.uninstallPackage("hf_xet");
    } else {
      // Install/Update to the live upstream pin from requirements.txt
      // (e.g. hf_xet>=1.5.2) — never a hardcoded floor.
      let spec = "hf_xet";
      try {
        const c = await window.w2gp.checkPackage("hf_xet");
        if (c && c.required) spec = "hf_xet" + c.required;
      } catch {}
      r = await window.w2gp.installPackage(spec);
    }
    if (r && r.success) {
      updateXetStatus();
      showToast(
        r.success
          ? "hf_xet " +
              (this.textContent.startsWith("Uninstall")
                ? "uninstalled"
                : "installed")
          : "Failed",
      );
    } else {
      if (status) {
        status.textContent = "failed";
        status.style.color = "var(--signal-red)";
      }
      showToast("✗ " + (r && r.error ? r.error : "Failed"));
    }
  } catch (e) {
    if (status) {
      status.textContent = "error";
      status.style.color = "var(--signal-red)";
    }
    showToast("✗ " + e.message);
  } finally {
    this.disabled = false;
  }
});

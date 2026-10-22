// cleanup-tab.js — Manage → System: disk cache cleanup.
//
// The rows come from the Rust allow-list (`cleanup_scan`): the renderer sends
// keys back, never paths, so a folder outside that list cannot be deleted from
// here. Sizes are measured on demand because the HuggingFace hub cache is a
// real directory walk (tens of GB); presence alone is instant.
//
// Reads app.js globals ($, fmtBytes, askConfirm, showToast) and window.w2gp.

const esc = window.escHtml;
const CLEANUP_SIZE_TTL_MS = 20000;

let cleanupTargets = [];
let cleanupTicked = new Set();
let cleanupSizedAt = 0;
let cleanupHasSizes = false;
let cleanupAutoSized = false;
let cleanupPendingSize = false;
let cleanupBusy = false;

function cleanupTickTotal() {
  return cleanupTargets
    .filter((t) => cleanupTicked.has(t.key))
    .reduce((n, t) => n + (t.bytes || 0), 0);
}

function cleanupStatusLine() {
  const st = $("cleanupStatus");
  if (!st) return;
  if (!cleanupTicked.size) {
    const gone = cleanupTargets.filter((t) => !t.exists).length;
    st.textContent = gone
      ? `Nothing ticked — ${gone} of ${cleanupTargets.length} folder(s) not present on disk.`
      : "Nothing ticked. Folders are kept; only their contents are deleted.";
    return;
  }
  const risky = cleanupTargets.filter((t) => cleanupTicked.has(t.key) && t.risky);
  st.textContent =
    `${cleanupTicked.size} folder(s) ticked · ${fmtBytes(cleanupTickTotal())}` +
    (risky.length ? " · includes model weights that will be re-downloaded" : "");
}

function renderCleanupRows() {
  const box = $("cleanupRows");
  if (!box) return;
  if (!cleanupTargets.length) {
    box.innerHTML = '<span class="token-hint">No cache folders reported.</span>';
    return;
  }
  box.innerHTML = cleanupTargets
    .map((t) => {
      const size = cleanupHasSizes
        ? `${fmtBytes(t.bytes)} · ${t.files.toLocaleString()} files`
        : "size on demand";
      const checked = cleanupTicked.has(t.key) ? " checked" : "";
      const disabled = t.exists ? "" : " disabled";
      return `
        <label class="cleanup-row${t.exists ? "" : " is-gone"}" title="${esc(t.path)}">
          <input type="checkbox" data-cleanup-key="${esc(t.key)}"${checked}${disabled}>
          <span class="cleanup-body">
            <span class="cleanup-head">
              <span class="cleanup-name">${esc(t.label)}</span>
              <span class="cleanup-size">${esc(size)}</span>
              ${t.risky ? '<span class="cleanup-risk">⚠ model weights</span>' : ""}
            </span>
            <span class="cleanup-path">${esc(t.path)}</span>
            ${t.risky ? `<span class="cleanup-warning">${esc(t.warning)}</span>` : ""}
          </span>
        </label>`;
    })
    .join("");
  cleanupStatusLine();
}

/**
 * @param {{sizes?: boolean, force?: boolean}} opts sizes defaults to true;
 *   `false` is the cheap presence-only pass the Manage panel fires on open.
 */
async function refreshCleanupRows(opts = {}) {
  const box = $("cleanupRows");
  if (!box || cleanupBusy) return;
  const sizes = opts.sizes !== false;
  if (sizes && !opts.force && Date.now() - cleanupSizedAt < CLEANUP_SIZE_TTL_MS) {
    return;
  }
  cleanupBusy = true;
  const st = $("cleanupStatus");
  if (sizes && st) st.textContent = "Measuring cache folders…";
  try {
    const r = await window.w2gp.cleanupScan(sizes);
    const fresh = (r && r.targets) || [];
    if (sizes) {
      cleanupTargets = fresh;
      cleanupHasSizes = true;
      cleanupSizedAt = Date.now();
    } else if (cleanupHasSizes) {
      // Presence-only pass: keep the byte counts already measured, take the
      // fresh `exists` (a folder may have appeared or been removed since).
      const prev = new Map(cleanupTargets.map((t) => [t.key, t]));
      cleanupTargets = fresh.map((t) => {
        const old = prev.get(t.key);
        return old ? { ...t, bytes: old.bytes, files: old.files } : t;
      });
    } else {
      cleanupTargets = fresh;
    }
    renderCleanupRows();
    // First Manage open: presence is instant, then measure once in the
    // background so the numbers are there without a click (the hub cache is a
    // tens-of-GB walk — deliberately not repeated on every open).
    if (!sizes && !cleanupAutoSized) {
      cleanupAutoSized = true;
      cleanupPendingSize = true;
    }
  } catch (e) {
    if (st) st.textContent = "Could not read cache folders: " + (e?.message || e);
  } finally {
    cleanupBusy = false;
    // Runs after the presence pass released the lock — starting it from inside
    // the try would hit the `cleanupBusy` guard and silently skip the sizes.
    if (cleanupPendingSize) {
      cleanupPendingSize = false;
      refreshCleanupRows({ sizes: true, force: true });
    }
  }
}

$("cleanupRows")?.addEventListener("change", function (ev) {
  const key = ev.target?.dataset?.cleanupKey;
  if (!key) return;
  if (ev.target.checked) cleanupTicked.add(key);
  else cleanupTicked.delete(key);
  cleanupStatusLine();
});

$("cleanupRefreshBtn")?.addEventListener("click", function () {
  refreshCleanupRows({ sizes: true, force: true });
});

$("cleanupRunBtn")?.addEventListener("click", async function () {
  const keys = [...cleanupTicked];
  if (!keys.length) {
    showToast("Tick at least one folder first");
    return;
  }
  const risky = cleanupTargets.filter((t) => cleanupTicked.has(t.key) && t.risky);
  const warning = risky.length
    ? "\n\n" +
      risky.map((t) => `⚠ ${t.label}: ${t.warning}`).join("\n\n")
    : "";
  const ok = await askConfirm(
    `Delete the contents of ${keys.length} folder(s), freeing about ${fmtBytes(cleanupTickTotal())}?` +
      warning +
      "\n\nFolders stay in place — contents are re-downloaded or rebuilt when needed.",
    "Clean caches",
  );
  if (!ok) return;

  this.disabled = true;
  const st = $("cleanupStatus");
  if (st) st.textContent = "Deleting…";
  try {
    const r = await window.w2gp.cleanupClean(keys);
    const parts = [];
    if (r && r.freedBytes != null)
      parts.push(`Freed ${fmtBytes(r.freedBytes)} (${r.removedFiles.toLocaleString()} files)`);
    if (r && r.errors && r.errors.length)
      parts.push(`${r.errors.length} item(s) could not be deleted: ${r.errors.slice(0, 3).join("; ")}`);
    cleanupTicked.clear();
    cleanupSizedAt = 0;
    // Re-measure (not just presence): the numbers on screen are the ones that
    // just changed. The summary is written after the refresh, which repaints
    // the tick line and would otherwise overwrite it.
    await refreshCleanupRows({ sizes: true, force: true });
    if (st) st.textContent = parts.join(" · ") || "Nothing to delete.";
  } catch (e) {
    if (st) st.textContent = "Cleanup failed: " + (e?.message || e);
  } finally {
    this.disabled = false;
  }
});

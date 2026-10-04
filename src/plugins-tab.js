// plugins-tab.js — Wan2GP plugin manager (enable + install/update/uninstall + favourites).
//
// Extracted from app.js as a pure move. $, showToast and window.w2gp are globals
// defined by app.js, and top-level statements here only register listeners,
// so loading this file after app.js is safe.

// ── Plugins (Wan2GP plugin manager parity: enable + install/update/uninstall + favourites) ──
let _pluginFavs = [];
let _pluginData = [];
let _pluginUpdates = {};
let _pluginLoadedAt = 0;
// Biggest Manage DOM write — load once, then only when stale or explicitly
// refreshed, so opening Manage never waits on the plugin disk walk.
function refreshPluginsLazy() {
  try {
    if (_pluginData.length && Date.now() - _pluginLoadedAt < 30000) return;
  } catch (e) {}
  refreshPlugins();
}
let _pluginQuery = "";
let _pluginSort = { key: "name", dir: 1 };
$("pluginSearchInput")?.addEventListener("input", (e) => {
  _pluginQuery = e.target.value || "";
  renderPlugins();
});
document.querySelectorAll(".plugin-sort").forEach((b) => {
  b.addEventListener("click", () => {
    const k = b.dataset.sort;
    if (_pluginSort.key === k) _pluginSort.dir *= -1;
    else _pluginSort = { key: k, dir: k === "date" ? -1 : 1 };
    renderPlugins();
  });
});
async function refreshPlugins() {
  const list = $("pluginList");
  if (!list) return;
  list.innerHTML = '<p class="token-hint">Loading…</p>';
  let r;
  try {
    r = await window.w2gp.pluginsList();
  } catch (e) {
    list.textContent = "";
    {
      const p = document.createElement("p");
      p.className = "token-hint";
      p.textContent = "✗ " + ((e && e.message) || String(e));
      list.append(p);
    }
    return;
  }
  if (!r || !r.ok) {
    list.textContent = "";
    {
      const p = document.createElement("p");
      p.className = "token-hint";
      p.textContent = (r && r.error) || "Failed to load";
      list.append(p);
    }
    return;
  }
  try {
    const cfg = await window.w2gp.configLoad();
    _pluginFavs = cfg.favoritePlugins || [];
  } catch {
    _pluginFavs = [];
  }
  _pluginData = r.plugins || [];
  _pluginLoadedAt = Date.now();
  renderPlugins();
}
function renderPlugins() {
  const list = $("pluginList");
  if (!list) return;
  const q = _pluginQuery.trim().toLowerCase();
  let arr = _pluginData.filter(
    (p) =>
      !q ||
      (
        (p.name || "") +
        " " +
        (p.author || "") +
        " " +
        p.id +
        " " +
        (p.description || "")
      )
        .toLowerCase()
        .includes(q),
  );
  const { key, dir } = _pluginSort;
  const grp = (p) => (p.group === "system" ? 1 : 0);
  arr = [...arr].sort(
    (a, b) =>
      grp(a) - grp(b) ||
      (() => {
        const x = (a[key] || "").toLowerCase(),
          y = (b[key] || "").toLowerCase();
        return x < y ? -dir : x > y ? dir : 0;
      })(),
  );
  document.querySelectorAll(".plugin-sort").forEach((b) => {
    const active = b.dataset.sort === _pluginSort.key;
    b.classList.toggle("active", active);
    b.textContent =
      b.textContent.replace(/ [▲▼]/, "") +
      (active ? (_pluginSort.dir === 1 ? " ▲" : " ▼") : "");
  });
  list.innerHTML = "";
  let lastGroup = "";
  for (const p of arr) {
    const g = p.group === "system" ? "system" : "community";
    if (g !== lastGroup) {
      lastGroup = g;
      const h = document.createElement("div");
      h.className = "plugin-group";
      h.textContent =
        g === "system" ? "System plugins (deepbeepmeep)" : "Community plugins";
      list.appendChild(h);
    }
    const row = document.createElement("div");
    row.className = "browser-row";
    const label = document.createElement("label");
    label.className = "browser-opt";
    const cb = document.createElement("input");
    cb.type = "checkbox";
    cb.checked = !!p.enabled;
    cb.dataset.pluginId = p.id;
    // ponytail: enabling a non-cloned plugin is a no-op (Wan2GP skips missing dirs) —
    // lock the box until installed so Save can't promise what isn't there.
    if (p.system || p.locked) {
      cb.checked = true;
      cb.disabled = true;
      if (p.locked)
        label.title =
          (p.description ? p.description + " — " : "") +
          "Default plugin, always enabled.";
    }
    if (!p.installed) cb.disabled = true;
    label.appendChild(cb);
    const badgeBase =
      (p.system ? "system" : p.installed ? "installed" : "available") +
      (p.version ? " v" + p.version : "");
    const badges = [badgeBase]
      .concat(p.author ? ["by " + p.author] : [])
      .join(" · ");
    label.appendChild(document.createTextNode(" " + p.name + " (" + badges));
    // Available update: amber row wash + bold badge (was a grey text fragment).
    const updCount = _pluginUpdates[p.id];
    if (updCount) {
      row.classList.add("plugin-has-update");
      label.appendChild(document.createTextNode(" · "));
      const ub = document.createElement("span");
      ub.className = "plugin-update-badge";
      ub.textContent = "⇪ " + updCount + " update(s)";
      label.appendChild(ub);
    }
    label.appendChild(document.createTextNode(")"));
    if (p.description) label.title = p.description;
    row.appendChild(label);
    // ★ favourite (auto-installed on fresh setup) — needs a URL to reinstall from
    if (p.url) {
      const fav = document.createElement("button");
      fav.className = "btn btn-ghost small";
      fav.textContent = _pluginFavs.includes(p.url) ? "★" : "☆";
      fav.title = "Favourite — auto-install on fresh setup";
      fav.addEventListener("click", async () => {
        const cfg = await window.w2gp.configLoad();
        let favs = cfg.favoritePlugins || [];
        favs = favs.includes(p.url)
          ? favs.filter((u) => u !== p.url)
          : [...favs, p.url];
        cfg.favoritePlugins = favs;
        await window.w2gp.configSave(cfg);
        _pluginFavs = favs;
        fav.textContent = favs.includes(p.url) ? "★" : "☆";
        showToast(
          favs.includes(p.url)
            ? "★ Favourited — will auto-install on setup"
            : "☆ Unfavourited",
        );
      });
      row.appendChild(fav);
    }
    // ⬇ install (catalog entries not yet cloned)
    if (!p.installed && p.url) {
      const ins = document.createElement("button");
      ins.className = "btn btn-primary small";
      ins.textContent = "Install";
      ins.addEventListener("click", async () => {
        ins.disabled = true;
        ins.textContent = "Installing…";
        appendLog("[*] Installing plugin " + p.name + " — progress below…");
        try {
          const r = await window.w2gp.pluginInstall(p.url);
          if (r && r.ok)
            showToast(
              "✓ " +
                p.name +
                " installed & enabled — restart Wan2GP to load it",
            );
          else showToast("✗ " + ((r && r.error) || "install failed"));
        } catch (e) {
          showToast("✗ " + e.message);
        } finally {
          refreshPlugins();
        }
      });
      row.appendChild(ins);
    }
    if (p.installed && !p.system && p.url) {
      const up = document.createElement("button");
      up.className = "btn btn-ghost small";
      up.textContent = "↻";
      up.title = "Check for updates";
      up.style.marginLeft = "4px";
      up.addEventListener("click", async () => {
        up.disabled = true;
        const orig = up.textContent;
        up.textContent = "…";
        try {
          const c = await window.w2gp.pluginCheckUpdate(p.id);
          if (c && c.update) {
            up.textContent = "⇪";
            appendLog(
              "[*] Updating plugin " +
                p.id +
                " (" +
                c.behind +
                " behind) — progress below…",
            );
            const u = await window.w2gp.pluginUpdate(p.id);
            if (u && u.ok)
              showToast("✓ " + p.name + " updated — restart Wan2GP to load it");
            else showToast("✗ " + ((u && u.error) || "update failed"));
          } else {
            showToast(
              "✓ " +
                p.name +
                " is up to date" +
                (c && c.error ? " (" + c.error + ")" : ""),
            );
          }
        } catch (e) {
          showToast("✗ " + e.message);
        } finally {
          up.disabled = false;
          up.textContent = orig;
        }
      });
      row.appendChild(up);
      // 🗑 uninstall (not system/bundled)
      if (
        ![
          "downloads",
          "media_flow",
          "models_manager",
          "motion_designer",
          "sample",
        ].includes(p.id)
      ) {
        const del = document.createElement("button");
        del.className = "btn btn-ghost small";
        del.textContent = "🗑";
        del.title = "Uninstall plugin";
        del.style.marginLeft = "4px";
        del.addEventListener("click", async () => {
          const choice = await window.w2gp.confirmDialog({
            title: "Uninstall " + p.name + "?",
            message: "Remove the plugin folder and disable it?",
          });
          if (choice !== "ok") return;
          del.disabled = true;
          try {
            const u = await window.w2gp.pluginUninstall(p.id);
            if (u && u.ok)
              showToast(
                u.pending
                  ? "⏳ " + (u.hint || "Locked — deleted on next start")
                  : "✓ " + p.name + " uninstalled",
              );
            else showToast("✗ " + ((u && u.error) || "uninstall failed"));
          } catch (e) {
            showToast("✗ " + e.message);
          } finally {
            refreshPlugins();
          }
        });
        row.appendChild(del);
      }
    }
    list.appendChild(row);
  }
  if (!arr.length)
    list.innerHTML = '<p class="token-hint">No plugins match.</p>';
}
$("pluginCheckUpdatesBtn")?.addEventListener("click", async () => {
  const btn = $("pluginCheckUpdatesBtn"),
    st = $("pluginRefreshStatus");
  const orig = btn.textContent;
  btn.disabled = true;
  btn.textContent = "Checking…";
  if (st)
    st.textContent = "⏳ Fetching plugin remotes — progress in the console…";
  appendLog("[*] Checking all plugins for updates — progress below…");
  try {
    const r = await window.w2gp.pluginCheckUpdates();
    if (r && r.ok) {
      _pluginUpdates = {};
      for (const u of r.updates || [])
        if (u.update) _pluginUpdates[u.id] = u.behind;
      renderPlugins();
      if (st)
        st.textContent = r.updates_available
          ? "⇪ " +
            r.updates_available +
            " update(s) available — use ↻ per plugin"
          : "✓ All plugins up to date";
      showToast(
        r.updates_available
          ? "⇪ " + r.updates_available + " update(s) available"
          : "✓ All plugins up to date",
      );
    } else showToast("✗ " + ((r && r.error) || "check failed"));
  } catch (e) {
    showToast("✗ " + e.message);
    if (st) st.textContent = "✗ " + e.message;
  } finally {
    btn.disabled = false;
    btn.textContent = orig;
  }
});
$("pluginRefreshBtn")?.addEventListener("click", async () => {
  const btn = $("pluginRefreshBtn"),
    st = $("pluginRefreshStatus");
  const orig = btn.textContent;
  btn.disabled = true;
  btn.textContent = "Refreshing…";
  if (st) st.textContent = "⏳ Contacting GitHub — progress in the console…";
  appendLog("[*] Refreshing plugin library from GitHub — progress below…");
  try {
    const r = await window.w2gp.pluginRefreshCatalog();
    if (r && r.ok) {
      if (st)
        st.textContent =
          "✓ " +
          r.checked +
          " checked, " +
          r.updated +
          " updated" +
          (r.updates_available
            ? ", " +
              r.updates_available +
              " update(s) available — use ↻ per plugin"
            : "");
      showToast("✓ Library refreshed");
    } else showToast("✗ " + ((r && r.error) || "refresh failed"));
  } catch (e) {
    showToast("✗ " + e.message);
    if (st) st.textContent = "✗ " + e.message;
  } finally {
    btn.disabled = false;
    btn.textContent = orig;
    refreshPlugins();
  }
});
$("pluginSaveBtn")?.addEventListener("click", async () => {
  const btn = $("pluginSaveBtn"),
    st = $("pluginSaveStatus");
  const orig = btn.textContent;
  btn.disabled = true;
  btn.textContent = "Saving…";
  const ids = [
    ...document.querySelectorAll('#pluginList input[type="checkbox"]'),
  ]
    .filter((c) => c.checked)
    .map((c) => c.dataset.pluginId);
  try {
    await window.w2gp.writeWgpConfig({ enabled_plugins: ids });
    if (st) st.textContent = "✓ Saved — takes effect on next Wan2GP launch.";
    showToast("✓ Plugins saved");
  } catch (e) {
    if (st) st.textContent = "✗ " + e.message;
    showToast("✗ " + e.message);
  } finally {
    btn.disabled = false;
    btn.textContent = orig;
  }
});
$("pluginInstallBtn")?.addEventListener("click", async () => {
  const input = $("pluginUrlInput");
  const st = $("pluginSaveStatus");
  const url = (input?.value || "").trim();
  if (!url) {
    showToast("Paste a plugin git URL first");
    return;
  }
  const btn = $("pluginInstallBtn");
  const orig = btn.textContent;
  btn.disabled = true;
  btn.textContent = "Installing…";
  if (st) st.textContent = "⏳ Installing — progress in the console…";
  appendLog("[*] Installing plugin from " + url + " — progress below…");
  try {
    const r = await window.w2gp.pluginInstall(url);
    if (r && r.ok) {
      showToast("✓ Plugin installed & enabled — restart Wan2GP to load it");
      if (input) input.value = "";
      if (st) st.textContent = "✓ Installed " + r.id;
    } else {
      showToast("✗ " + ((r && r.error) || "install failed"));
      if (st) st.textContent = "✗ " + ((r && r.error) || "install failed");
    }
  } catch (e) {
    showToast("✗ " + e.message);
    if (st) st.textContent = "✗ " + e.message;
  } finally {
    btn.disabled = false;
    btn.textContent = orig;
    refreshPlugins();
  }
});
// Populate the Manage "Default Browser" list from the main process.
async function loadBrowserList() {
  const list = $("browserList");
  if (!list) return;
  list.innerHTML =
    '<div class="browser-row"><label class="browser-opt"><input type="radio" name="defaultBrowser" value="system" checked> System default</label></div>';
  try {
    const { browsers, defaultBrowser } = await window.w2gp.detectBrowsers();
    for (const b of browsers) {
      const row = document.createElement("div");
      row.className = "browser-row";
      const label = document.createElement("label");
      label.className = "browser-opt";
      const radio = document.createElement("input");
      radio.type = "radio";
      radio.name = "defaultBrowser";
      radio.value = b.id;
      radio.disabled = !b.installed;
      if (b.id === defaultBrowser) radio.checked = true;
      label.appendChild(radio);
      label.appendChild(
        document.createTextNode(
          " " + b.name + (b.installed ? "" : " (not installed)"),
        ),
      );
      row.appendChild(label);
      list.appendChild(row);
    }
    list.querySelectorAll('input[name="defaultBrowser"]').forEach((r) => {
      r.addEventListener("change", async () => {
        if (!r.checked) return;
        const cfg = await window.w2gp.configLoad();
        cfg.defaultBrowser = r.value;
        await window.w2gp.configSave(cfg);
        appendLog(`[*] Default browser set to: ${r.value}`);
      });
    });
  } catch (e) {
    appendLog(`[!] Browser detection failed: ${errText(e)}`);
  }
}

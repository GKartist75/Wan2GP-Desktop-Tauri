// adult-pack-tab.js — Manage → 📦 Model Pack: the opt-in Adult (NSFW) model
// pack (installed on demand, never part of the Wan2GP install).
//
// Every row comes from src/adult-pack-manifest.json via the adult_pack_manifest
// command — model names, sizes and URLs are data, not code, so adding or
// dropping a model is a manifest edit (scripts/verify-adult-pack.mjs) with no
// change here.
//
// Two tiers, because they behave differently:
//   * LoRAs are downloaded by the launcher into loras/<collection>/adult-pack/,
//     resumable and SHA-256 verified.
//   * Checkpoints are recipes: we write finetunes/adultpack_*.json with remote
//     URLs and Wan2GP fetches the weights when you actually run that model,
//     into <checkpoints_paths[0]>/adult-pack/ (the URL|adult-pack suffix).
//     Tick "prefetch" to pull one now.
(() => {
  const $ = (id) => document.getElementById(id);
  const GB = 1024 * 1024 * 1024;

  const fmtBytes = (b) => {
    if (!b) return "0 MB";
    if (b >= GB) return (b / GB).toFixed(b / GB >= 10 ? 0 : 1) + " GB";
    if (b >= 1024 * 1024) return Math.round(b / (1024 * 1024)) + " MB";
    return Math.round(b / 1024) + " KB";
  };

  // ── state ────────────────────────────────────────────────────────────────
  let pack = null;
  let sel = { loras: {}, fts: {}, civ: {} };
  let saveTimer = null;
  let progressUnsub = null;
  let wired = false;
  let busy = false;
  // Every architecture this build can load (installed defaults/ + archMap).
  // Loaded once per init so both the recipe form and Add-by-link read one list.
  let arches = [];
  let loraInvLoaded = false;
  // Catalog filter — session-only on purpose: a remembered "selected only"
  // would hide the catalog on the next visit and look like missing data.
  let filter = { q: "", selected: false, missing: false };

  const enabled = () => !!(pack && $("apEnabled") && $("apEnabled").checked);
  const ageOk = () => !!(pack && $("apAge") && $("apAge").checked);
  const civOn = () => !!(pack && pack.civitaiEnabled);
  const active = () => enabled() && ageOk();

  function persist() {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      window.w2gp
        .adultPackSave(
          { enabled: enabled(), ageConfirmed: ageOk(), loras: sel.loras, finetunes: sel.fts, civitai: sel.civ },
          $("apToken").value.trim() || null,
        )
        .catch(() => {});
    }, 400);
  }

  const rowEl = (cls, text, title) => {
    const d = document.createElement("div");
    d.className = cls;
    if (text !== undefined) d.textContent = text;
    if (title) d.title = title;
    return d;
  };
  const sizeOf = (items) => items.reduce((a, x) => a + (x.bytes || 0), 0);
  const esc = (s) =>
    String(s == null ? "" : s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

  function checkBox(checked, onchange, title) {
    const l = document.createElement("label");
    l.className = "ap-check";
    const i = document.createElement("input");
    i.type = "checkbox";
    i.checked = !!checked;
    if (title) i.title = title;
    i.addEventListener("change", () => onchange(i.checked));
    l.appendChild(i);
    return { label: l, input: i };
  }

  // ── catalog filter ───────────────────────────────────────────────────────
  const filterOn = () => !!(filter.q || filter.selected || filter.missing);
  const hit = (values) =>
    values.some((v) => String(v || "").toLowerCase().includes(filter.q));

  function matchesFilter(item, kind) {
    if (filter.selected) {
      const on = kind === "lora" ? sel.loras[item.id] : sel.fts[item.id];
      if (!on) return false;
    }
    if (filter.missing) {
      if (kind === "lora" && item.installed) return false;
      if (kind === "finetune" && item.finetuneWritten) return false;
    }
    if (!filter.q) return true;
    return hit([item.name, item.file, item.group, item.family, item.id, item.architecture, item.description]);
  }

  function matchesCiv(c) {
    if (filter.selected) return !!sel.civ[c.id];
    return !filter.q || hit([c.name, c.description, c.id]);
  }

  const visibleLoras = () => (pack.loras || []).filter((l) => matchesFilter(l, "lora"));
  const visibleFinetunes = () =>
    (pack.finetunes || []).filter(
      (f) => f.architectureAvailable !== false && matchesFilter(f, "finetune"),
    );

  /** Toggle every row the filter currently shows: the reason to filter is
   *  usually "select this whole family", not just to look at it. */
  function selectShown(on) {
    for (const l of visibleLoras()) sel.loras[l.id] = on;
    for (const f of visibleFinetunes()) {
      sel.fts[f.id] = on;
      if (!on) sel.fts["pf:" + f.id] = false;
    }
    if (civOn()) for (const c of pack.civitai || []) if (matchesCiv(c)) sel.civ[c.id] = on;
    render();
    persist();
  }

  function clearCatalogFilter() {
    filter = { q: "", selected: false, missing: false };
    for (const id of ["apFilter", "apFilterSel", "apFilterMissing"]) {
      const el = $(id);
      if (!el) continue;
      if (el.type === "checkbox") el.checked = false;
      else el.value = "";
    }
    render();
  }

  /** Only ever render an https link out of the manifest — the preview and the
   *  file page come from remote JSON, so the scheme is checked, not assumed. */
  const httpsUrl = (u) => (typeof u === "string" && u.startsWith("https://") ? u : null);

  function linkEl(href, text, title) {
    const url = httpsUrl(href);
    if (!url) return null;
    const a = document.createElement("a");
    a.className = "ap-link";
    a.href = url;
    a.textContent = text;
    a.title = title || url;
    a.target = "_blank";
    a.rel = "noopener noreferrer";
    // Tauri's webview navigates in place for some targets; open externally.
    a.addEventListener("click", (e) => {
      e.preventDefault();
      try {
        window.w2gp.openExternal(url);
      } catch (_) {
        window.open(url, "_blank", "noopener");
      }
    });
    return a;
  }

  function thumbEl(url, alt) {
    const wrap = document.createElement("div");
    wrap.className = "ap-thumb";
    const u = httpsUrl(url);
    if (!u) {
      wrap.classList.add("ap-thumb-none");
      wrap.textContent = "—";
      return wrap;
    }
    const img = document.createElement("img");
    img.src = u;
    img.alt = alt || "";
    img.loading = "lazy";
    img.decoding = "async";
    // A missing preview must degrade to a placeholder, never a broken icon.
    img.addEventListener("error", () => {
      wrap.classList.add("ap-thumb-none");
      wrap.textContent = "—";
    });
    wrap.appendChild(img);
    if (u) wrap.title = "Preview from the model repo (pinned to the verified commit)";
    return wrap;
  }

  // ── live galleries (HuggingFace + CivitAI examples) ─────────────────────
  // The manifest carries one pinned preview per repo. This fetches the rest on
  // demand — and, crucially, the .mp4 samples that image-only previews skip.
  // Every Wan/H3/LTX repo publishes its examples as video.
  const galleryCache = new Map();

  function mediaEl(url, kind, name) {
    const u = httpsUrl(url);
    if (!u) return null;
    if (kind === "video") {
      const v = document.createElement("video");
      v.src = u;
      v.controls = true;
      v.preload = "none";
      v.muted = true;
      v.loop = true;
      v.playsInline = true;
      v.className = "ap-media-video";
      v.title = name || "sample";
      return v;
    }
    const i = document.createElement("img");
    i.src = u;
    i.alt = name || "example";
    i.loading = "lazy";
    i.decoding = "async";
    i.className = "ap-media-img";
    i.addEventListener("click", () => openLightbox(u, name));
    i.addEventListener("error", () => i.remove());
    return i;
  }

  function lightboxEl() {
    let lb = document.getElementById("apLightbox");
    if (!lb) {
      lb = rowEl("ap-lightbox");
      lb.id = "apLightbox";
      const close = rowEl("ap-lightbox-close", "✕", "Close");
      close.addEventListener("click", () => {
        lb.style.display = "none";
        lb.innerHTML = "";
      });
      lb.appendChild(close);
      document.body.appendChild(lb);
      lb.addEventListener("click", (e) => {
        if (e.target === lb) {
          lb.style.display = "none";
          lb.innerHTML = "";
        }
      });
    }
    return lb;
  }

  function openLightbox(url, name) {
    const u = httpsUrl(url);
    if (!u) return;
    const lb = lightboxEl();
    const closeBox = () => {
      lb.style.display = "none";
      lb.innerHTML = "";
    };
    lb.innerHTML = "";
    const close = rowEl("ap-lightbox-close", "✕", "Close");
    close.addEventListener("click", closeBox);
    lb.appendChild(close);
    const holder = rowEl("ap-lightbox-body");
    if (/\.(mp4|webm|mov)(\?|$)/i.test(u)) {
      holder.appendChild(mediaEl(u, "video", name));
    } else {
      const img = document.createElement("img");
      img.src = u;
      img.alt = name || "";
      holder.appendChild(img);
    }
    if (name) holder.appendChild(rowEl("ap-lightbox-cap", name));
    lb.appendChild(holder);
    lb.style.display = "flex";
    lb.onclick = (e) => {
      if (e.target === lb) closeBox();
    };
  }

  /** Build (and lazily fill) a model's example drawer.
   *
   *  Most HF mirrors publish no media at all — a 55-file repo can contain zero
   *  images — so the drawer starts from the examples baked into the manifest
   *  (matched from CivitAI by name at verification time) and only then falls
   *  back to a live fetch. When neither has anything, it offers a CivitAI search
   *  for this exact model instead of showing an empty box. */
  function galleryEl(item, kind) {
    const box = rowEl("ap-gallery");
    box.style.display = "none";
    const grid = rowEl("ap-gallery-grid");
    box.appendChild(grid);

    const baked = item.examples || [];
    let loaded = false;
    const paintBaked = () => {
      for (const e of baked) {
        const m = mediaEl(e.url, e.kind, item.name);
        if (m) grid.appendChild(m);
      }
      if (baked.length && item.civitaiPage) {
        grid.appendChild(
          linkEl(item.civitaiPage, "Examples from CivitAI" + (item.civitaiName ? `: ${item.civitaiName}` : ""), item.civitaiPage),
        );
      }
    };

    const findOnCivitai = async () => {
      const q = (item.file || item.name || "")
        .split("/")
        .pop()
        .replace(/\.(safetensors|gguf)$/i, "")
        .replace(/[-_ ]?(high|low)(\s?noise)?/gi, " ")
        .replace(/\s+/g, " ")
        .trim();
      const note = rowEl("ap-hint", "Searching CivitAI…");
      grid.appendChild(note);
      try {
        const res = await window.w2gp.adultPackSearch("civitai", q, "any", item.family || "any", 8);
        note.remove();
        if (!(res.items || []).length) {
          note.textContent = `No CivitAI model matches "${q}".`;
          return;
        }
        grid.appendChild(rowEl("ap-hint", "Possible matches on CivitAI — check the name before trusting a picture:"));
        for (const it of res.items || []) {
          const cell = rowEl("ap-civ-cell");
          for (const e of (it.examples || []).slice(0, 4)) {
            const m = mediaEl(e.url, e.kind, it.title);
            if (m) cell.appendChild(m);
          }
          const cap = rowEl("ap-civ-cap");
          cap.appendChild(linkEl(it.page, it.title, it.page));
          cap.appendChild(rowEl("ap-hint", `${it.versionName || "version ?"} · ${fmtCompact(it.downloads || 0)} downloads/month`));
          cell.appendChild(cap);
          grid.appendChild(cell);
        }
      } catch (e) {
        note.textContent = "CivitAI search failed: " + (errText(e) || e);
      }
    };

    const load = async () => {
      if (loaded) return;
      loaded = true;
      grid.innerHTML = "";
      paintBaked();
      // "baked" is a search/browse result: its examples came with the result,
      // so there is nothing to fetch and no repo to ask.
      if (kind === "baked") {
        if (!baked.length) grid.appendChild(rowEl("ap-hint", "No examples published for this model."));
        return;
      }
      try {
        const r =
          kind === "civitai"
            ? await window.w2gp.adultPackCivitaiGallery(item.id)
            : await window.w2gp.adultPackGallery(item.repo, item.repoSha || null);
        const vids = r && r.videos ? r.videos : [];
        const imgs = r && r.images ? r.images : [];
        if (r && r.pinned === false && kind !== "civitai") {
          grid.appendChild(rowEl("ap-hint", "Repo examples at the repo's latest commit (not the pinned one)."));
        }
        for (const v of vids) grid.appendChild(mediaEl(v.url, "video", v.name));
        for (const im of imgs) grid.appendChild(mediaEl(im.url, "image", im.name));
        if (!vids.length && !imgs.length && !baked.length) {
          grid.appendChild(rowEl("ap-hint", "This model publishes no example images or clips on its repo."));
        }
      } catch (e) {
        if (!baked.length) {
          grid.appendChild(rowEl("ap-hint", "Could not load repo examples: " + (errText(e) || e)));
        }
      }
      if (!baked.length) {
        const b = rowEl("btn btn-ghost small", "Find examples on CivitAI", "Search CivitAI for this model's renders");
        b.addEventListener("click", findOnCivitai);
        grid.appendChild(b);
      }
      if (item.civitaiSearch) {
        grid.appendChild(linkEl(item.civitaiSearch, "Search CivitAI", "Open this model's name in a CivitAI search"));
      }
    };

    const btn = rowEl("ap-gallery-btn", "Examples", "Show example images and sample clips");
    btn.addEventListener("click", async () => {
      const open = box.style.display === "none";
      box.style.display = open ? "" : "none";
      if (open) await load();
    });
    return { box, btn };
  }

  /** The "show more" panel: everything the manifest knows that does not fit on
   *  one line — provenance, hash, trigger words, licence, Wan2GP ids. */
  function detailsEl(item) {
    const box = rowEl("ap-details");
    const add = (label, value) => {
      if (value === undefined || value === null || value === "") return;
      const r = rowEl("ap-detail-row");
      r.appendChild(rowEl("ap-detail-k", label));
      const v = rowEl("ap-detail-v");
      if (typeof value === "string") v.textContent = value;
      else v.appendChild(value);
      r.appendChild(v);
      box.appendChild(r);
    };
    add("File", item.file);
    if (item.version) add("Version", item.version);
    if (item.releasedAt) {
      add(
        "Released",
        String(item.releasedAt).slice(0, 10) + (item.civitaiName ? ` (CivitAI ${item.civitaiName}${item.civitaiVersion ? " · " + item.civitaiVersion : ""})` : " (first published upstream)"),
      );
    }
    if (item.sourceUpdatedAt) add("Source updated", String(item.sourceUpdatedAt).slice(0, 10));
    if (item.installed) add("On disk", item.installedPath || "yes");
    else if (item.installedStale) add("On disk", `${item.installedPath || ""} (older than upstream)`);
    if (item.filePage) add("Source", linkEl(item.filePage, "Open on HuggingFace", item.filePage));
    if (item.repo) add("Repo", linkEl(item.repoPage, item.repo, item.repo));
    if (item.civitaiId && item.civitaiPage) add("CivitAI", linkEl(item.civitaiPage, item.civitaiName || "civitai", item.civitaiPage));
    if (item.sha256) add("SHA-256", (item.sha256 || "").slice(0, 24) + "… (verified)");
    if (item.trigger) add("Trigger", item.trigger);
    if (item.note || item.description) add("Notes", item.note || item.description);
    if (item.licenseNote) add("Licence", item.licenseNote);
    else if (item.license) add("Licence", item.license);
    if (item.wan2gpIds && item.wan2gpIds.length) add("Wan2GP", item.wan2gpIds.join(", "));
    return box;
  }

  const logLine = (text, cls) => {
    const box = $("apLog");
    if (!box) return;
    const d = document.createElement("div");
    d.className = "ap-log-line " + (cls || "");
    d.textContent = text;
    box.appendChild(d);
    box.scrollTop = box.scrollHeight;
  };
  const setProgress = (text) => {
    const p = $("apProgress");
    if (p) p.textContent = text;
  };

  // ── rendering ────────────────────────────────────────────────────────────
  /** A short "v1.4 · released 2025-10-24" line. A date without a version is
   *  still useful: HF repos have no per-file publish date, so this is often the
   *  only freshness signal a row can show. */
  function versionEl(item) {
    const v = item.version || "";
    const d = String(item.releasedAt || "").slice(0, 10);
    if (!v && !d) return null;
    const span = rowEl("ap-ver", v ? `v${v}` : "", v ? `Version ${v}` : "");
    if (d) {
      span.title = `Released ${d}${item.civitaiName ? " (CivitAI version date)" : " (first published upstream)"}`;
      span.appendChild(rowEl("ap-ver-date", d));
    }
    return span;
  }

  /** Installed / needs update / recipe-written state, in one badge slot. */
  function stateEl(item) {
    if (item.installed) return rowEl("ap-tag done", "downloaded", "On disk and matching the verified size");
    if (item.installedStale) return rowEl("ap-tag upd", "update available", "On disk, but upstream has newer bytes");
    if (item.overridden) return rowEl("ap-tag upd", "updated pin", "Local hash override is newer than the shipped catalog");
    if (item.finetuneWritten) return rowEl("ap-tag done", "recipe written", "finetunes/*.json is in place");
    if (item.weightsPresent) return rowEl("ap-tag done", "weights on disk", "Checkpoint files are in the checkpoints folder");
    return null;
  }

  function thumbOf(item) {
    return thumbEl(item.preview || ((item.examples || [])[0] || {}).url, item.name);
  }

  // ── per-row actions ───────────────────────────────────────────────────
  // Bulk buttons at the bottom are for bulk work. Everything a single model
  // can do to itself — write its recipe, fetch it, copy its URL, delete it —
  // belongs on its own row, so nobody has to tick 40 rows to act on one.
  function copyToClipboard(text, btn) {
    if (!text) return;
    const was = btn.textContent;
    btn.textContent = "copied";
    navigator.clipboard
      .writeText(text)
      .catch(() => {
        btn.textContent = "copy failed";
      })
      .finally(() => setTimeout(() => (btn.textContent = was), 1200));
  }

  /** One small button in the action bar. `danger` is only for deletion. */
  function actionBtn(label, title, onClick, danger) {
    const b = rowEl("ap-act" + (danger ? " danger" : ""), label, title);
    b.addEventListener("click", onClick);
    return b;
  }

  /**
   * The `⋯` menu for one row. `kind` is what the row represents, because the
   * honest action set differs: a checkpoint can become a finetune JSON, a LoRA
   * cannot (Wan2GP loads LoRAs from loras\, not from a recipe) — so instead of
   * a button that silently does nothing, a LoRA gets the copy/open actions and
   * an explanation.
   */
  function actionsEl(item, kind) {
    const box = rowEl("ap-actions-row");
    box.style.display = "none";
    const bar = rowEl("ap-act-bar");
    box.appendChild(bar);

    const done = async (fn) => {
      box.style.display = "none";
      await fn();
    };

    const isLora = kind === "lora";
    const isCiv = kind === "civitai";

    if (isCiv) {
      bar.appendChild(
        actionBtn(
          item.installed ? "Re-fetch now" : "Fetch now",
          "Resolve the current version, download it, and file it in the right folder. A token is used when present; public releases need none.",
          () =>
            done(() =>
              withBusy("Fetching from CivitAI…", async () => {
                const token = ($("apToken").value || "").trim();
                try {
                  const r = await window.w2gp.adultPackCivitaiDownload(item.id, token || null);
                  const dest = r && r.kind === "lora" ? r.path : (r && r.finetune) || "downloaded";
                  logLine(`✓ ${item.name} → ${dest}`, "ok");
                } catch (e) {
                  logLine(`✗ ${item.name}: ${errText(e)}`, "err");
                }
                await init(true);
              }),
            ),
        ),
      );
    } else {
      bar.appendChild(
        actionBtn(
          isLora ? "Download now" : "Fetch weights now",
          isLora
            ? "Download just this file (resumable, SHA-256 verified)"
            : "Prefetch this checkpoint's weights to the ckpts/adult-pack folder",
          () =>
            done(() => {
              if (!isLora) sel.fts["pf:" + item.id] = true;
              return doFetch([item.id], isLora ? "Downloading" : "Fetching");
            }),
        ),
      );
      if (!isLora) {
        bar.appendChild(
          actionBtn(
            item.finetuneWritten ? "Rewrite finetune JSON" : "Make finetune JSON",
            `Write finetunes/adultpack_${item.id}.json so Wan2GP downloads this model itself`,
            () =>
              done(() =>
                withBusy("Writing finetune recipe…", async () => {
                  const r = await window.w2gp.adultPackWriteFinetunes([item.id]);
                  const n = ((r && r.written) || []).length;
                  logLine(
                    n
                      ? `✓ ${n} recipe(s) written to ${(r && r.dir) || "finetunes\\"}`
                      : `✗ ${((r && r.skipped) || []).map((s) => s.reason).join("; ") || "nothing written"}`,
                    n ? "ok" : "err",
                  );
                  setProgress(n ? "Recipe written — Wan2GP fetches the weights on first use." : "Nothing written.");
                  await init(true);
                }),
              ),
          ),
        );
        bar.appendChild(
          actionBtn("Open recipe", "Open this recipe in the finetune manager below", () =>
            done(async () => {
              const el = $("apFmList");
              if (!el) return;
              el.scrollIntoView({ block: "center" });
              await loadFinetunes();
              const hit = fmItems.find((f) => f.id === item.id);
              if (hit) {
                openFinetuneEditor(hit, el);
                return;
              }
              // Say so rather than appearing to do nothing: the row is not in
              // the manager list, so there is nothing to open.
              logLine(`${item.name} is not in the finetune manager — write its recipe first.`, "err");
            }),
          ),
        );
      } else {
        bar.appendChild(
          rowEl(
            "ap-hint",
            "LoRAs load from loras\\, so they have no finetune JSON — pick one below as a preload, or download the file.",
          ),
        );
      }
      bar.appendChild(
        actionBtn("Add to selection", "Tick this row so the bottom buttons can act on it", () =>
          done(() => {
            if (isLora) sel.loras[item.id] = true;
            else sel.fts[item.id] = true;
            render();
            persist();
          }),
        ),
      );
    }

    const url = item.url || (item.urls && item.urls[0]) || "";
    if (url) bar.appendChild(actionBtn("Copy URL", "Copy the source URL", (e) => copyToClipboard(url, e.currentTarget)));
    bar.appendChild(
      actionBtn("Copy file name", "Copy the exact file name", (e) => copyToClipboard(item.file || "", e.currentTarget)),
    );
    const page = item.filePage || item.civitaiPage || item.repoPage || item.page;
    if (page)
      bar.appendChild(
        actionBtn("Open source page", "Open the model on HuggingFace / CivitAI", () => {
          const u = httpsUrl(page);
          if (u) window.w2gp.openExternal(u);
        }),
      );
    bar.appendChild(
      actionBtn("Delete…", "Delete this item's downloaded files (and recipe)", () =>
        done(() =>
          withBusy("Removing…", async () => {
            const what = isLora
              ? "its downloaded file"
              : isCiv
              ? "its downloaded file(s) and local recipe"
              : "its prefetched weights and its finetune recipe";
            if (!(await askConfirm(`Delete ${item.name} — ${what}?\n\nOnly this entry's files and recipe are removed; nothing outside the pack folders is touched.`, "Delete pack item"))) return;
            try {
              const r = await window.w2gp.adultPackRemove([item.id]);
              logLine(`✓ removed ${((r && r.removed) || []).length} item(s)`, "ok");
            } catch (e) {
              logLine(`✗ ${errText(e)}`, "err");
            }
            await init(true);
          }),
        ),
      true),
    );

    const btn = rowEl("ap-act-btn", "⋯", "Actions for this model: download, make finetune JSON, copy, delete");
    btn.addEventListener("click", () => {
      box.style.display = box.style.display === "none" ? "" : "none";
    });
    return { btn, box };
  }

  function renderLoraRows(host, items) {
    for (const l of items) {
      const row = rowEl("ap-row");
      row.appendChild(thumbOf(l));
      const cb = checkBox(!!sel.loras[l.id], (on) => {
        sel.loras[l.id] = on;
        // A HIGH/LOW half is useless alone — Wan2GP loads both in one job.
        if (on && l.pair) {
          for (const p of pack.loras || []) {
            if (p.sourceId === l.sourceId && p.pair && p.pair !== l.pair) sel.loras[p.id] = true;
          }
        }
        render();
        persist();
      }, l.note || "");
      row.appendChild(cb.label);
      const nm = rowEl("ap-name");
      nm.textContent = l.name;
      if (l.pair) nm.appendChild(rowEl("ap-tag " + l.pair, l.pair === "high" ? "HIGH" : "LOW"));
      const st = stateEl(l);
      if (st) nm.appendChild(st);
      const v = versionEl(l);
      if (v) nm.appendChild(v);
      row.appendChild(nm);
      row.appendChild(rowEl("ap-size", fmtBytes(l.bytes)));
      const gal = galleryEl(l, "hf");
      row.appendChild(gal.btn);
      const info = rowEl("ap-info-btn", "i", "Show details, source link and hash");
      row.appendChild(info);
      const act = actionsEl(l, "lora");
      row.appendChild(act.btn);
      host.appendChild(row);
      const det = detailsEl(l);
      det.style.display = "none";
      host.appendChild(det);
      host.appendChild(gal.box);
      host.appendChild(act.box);
      info.addEventListener("click", () => {
        det.style.display = det.style.display === "none" ? "" : "none";
      });
    }
  }

  function render() {
    if (!pack) return;
    const body = $("apBody");
    const badge = $("apBadge");
    if (badge) {
      badge.textContent = ageOk() ? "18+ confirmed" : enabled() ? "confirm 18+" : "off";
      badge.className = "ap-badge" + (ageOk() ? "" : " off");
    }
    const meta = $("apMeta");
    if (meta) {
      const va = pack.verifiedAgainst || {};
      meta.textContent = pack.generatedAt
        ? `catalog v${pack.manifestVersion} · verified ${String(pack.generatedAt).slice(0, 10)} for Wan2GP ${va.wan2gp || "?"} (${va.inventoryCount || "?"} models)`
        : "";
    }
    if (!active()) {
      if (body) body.style.display = "none";
      updateTotals();
      return;
    }
    if (body) body.style.display = "";

    const host = $("apGroups");
    host.innerHTML = "";
    const loras = visibleLoras();

    for (const g of pack.groups || []) {
      const items = loras.filter((l) => l.group === g.id);
      if (!items.length) continue;
      const on = items.filter((l) => sel.loras[l.id]).length;
      const block = rowEl("ap-block");
      const head = rowEl("ap-block-head");
      const cb = checkBox(items.length > 0 && on === items.length, (checked) => {
        for (const l of items) sel.loras[l.id] = checked;
        render();
        persist();
      }, g.sub);
      head.appendChild(cb.label);
      head.appendChild(rowEl("ap-block-title", g.title, g.sub));
      head.appendChild(rowEl("ap-size", `${on}/${items.length} · ${fmtBytes(sizeOf(items.filter((l) => sel.loras[l.id])))}`));
      block.appendChild(head);
      const list = rowEl("ap-list");
      renderLoraRows(list, items);
      block.appendChild(list);
      host.appendChild(block);
    }

    const fts = visibleFinetunes();
    const ftBlock = rowEl("ap-block");
    const ftHead = rowEl("ap-block-head");
    ftHead.appendChild(rowEl("ap-title-sm", "Checkpoint finetunes"));
    ftHead.appendChild(rowEl("ap-hint", "recipe now — weights only when you run the model, or prefetch"));
    // Bulk toggles live in the section head: with 19+ checkpoints, ticking
    // them one by one is the whole job.
    for (const [label, title, value] of [
      ["Select all", "Tick every checkpoint finetune in this list", true],
      ["Deselect all", "Untick every checkpoint finetune (prefetch flags too)", false],
    ]) {
      const b = document.createElement("button");
      b.className = "btn btn-ghost small";
      b.textContent = label;
      b.title = title;
      b.addEventListener("click", () => {
        for (const f of fts) {
          sel.fts[f.id] = value;
          if (!value) sel.fts["pf:" + f.id] = false;
        }
        render();
        persist();
      });
      ftHead.appendChild(b);
    }
    ftBlock.appendChild(ftHead);
    for (const f of fts) {
      const row = rowEl("ap-row");
      const thumb = thumbOf(f);
      row.appendChild(thumb);
      const gal = galleryEl(f, "hf");
      if (!f.preview) {
        // The manifest has no picture for most HF checkpoints, but the repo
        // may still publish samples. Make the placeholder the shortcut.
        thumb.style.cursor = "pointer";
        thumb.title = "No preview published — click to load this repo's example media";
        thumb.addEventListener("click", () => {
          if (gal.box.style.display === "none") gal.btn.click();
          gal.box.scrollIntoView({ block: "nearest" });
        });
      }
      const cb = checkBox(!!sel.fts[f.id], (on) => {
        sel.fts[f.id] = on;
        render();
        persist();
      }, f.description || "");
      row.appendChild(cb.label);
      const nm = rowEl("ap-name");
      nm.textContent = f.name;
      const st = stateEl(f);
      if (st) nm.appendChild(st);
      else if (f.finetuneWritten) nm.appendChild(rowEl("ap-tag done", "recipe written"));
      const v = versionEl(f);
      if (v) nm.appendChild(v);
      row.appendChild(nm);
      const ftBytes = (f.bytes || 0) + (f.bytesLow || 0);
      row.appendChild(rowEl("ap-size", ftBytes ? fmtBytes(ftBytes) + (f.bytesLow ? " (H+L)" : "") : "—"));
      const pf = document.createElement("label");
      pf.className = "ap-prefetch";
      const pi = document.createElement("input");
      pi.type = "checkbox";
      pi.checked = !!sel.fts["pf:" + f.id];
      pi.disabled = !sel.fts[f.id];
      pi.addEventListener("change", () => {
        sel.fts["pf:" + f.id] = pi.checked;
        persist();
      });
      pf.appendChild(pi);
      pf.appendChild(document.createTextNode("prefetch"));
      row.appendChild(pf);
      row.appendChild(gal.btn);
      const info = rowEl("ap-info-btn", "i", "Show details, source link and hash");
      row.appendChild(info);
      const act = actionsEl(f, "finetune");
      row.appendChild(act.btn);
      ftBlock.appendChild(row);
      const det = detailsEl(f);
      det.style.display = "none";
      ftBlock.appendChild(det);
      ftBlock.appendChild(gal.box);
      ftBlock.appendChild(act.box);
      info.addEventListener("click", () => {
        det.style.display = det.style.display === "none" ? "" : "none";
      });
    }
    host.appendChild(ftBlock);

    const civHost = $("apCivItems");
    civHost.innerHTML = "";
    for (const c of (pack.civitai || []).filter(matchesCiv)) {
      const row = rowEl("ap-row");
      row.appendChild(thumbEl(c.preview, c.name));
      const cb = checkBox(!!sel.civ[c.id], (on) => {
        sel.civ[c.id] = on;
        render();
        persist();
      }, c.licenseNote || c.description || "");
      row.appendChild(cb.label);
      const nm = rowEl("ap-name");
      nm.textContent = c.name;
      if (c.installed) nm.appendChild(rowEl("ap-tag done", "on disk", c.installedPath || ""));
      else nm.appendChild(rowEl("ap-tag token", "CivitAI", "Public releases need no token; gated ones use the key below."));
      row.appendChild(nm);
      row.appendChild(rowEl("ap-size", fmtBytes(c.installed ? c.installedBytes || c.bytesEstimate : c.bytesEstimate)));
      const gal = galleryEl(c, "civitai");
      row.appendChild(gal.btn);
      const info = rowEl("ap-info-btn", "i", "Show details");
      row.appendChild(info);
      const act = actionsEl(c, "civitai");
      row.appendChild(act.btn);
      civHost.appendChild(row);
      const det = detailsEl(c);
      det.style.display = "none";
      civHost.appendChild(det);
      civHost.appendChild(gal.box);
      civHost.appendChild(act.box);
      info.addEventListener("click", () => {
        det.style.display = det.style.display === "none" ? "" : "none";
      });
    }

    const legal = $("apLegal");
    if (legal) {
      legal.innerHTML = "";
      for (const line of pack.legal || []) legal.appendChild(rowEl("ap-legal-line", "⚖ " + line));
    }
    const countEl = $("apFilterCount");
    if (countEl) {
      const totalL = (pack.loras || []).length;
      const totalF = (pack.finetunes || []).filter((f) => f.architectureAvailable !== false).length;
      countEl.textContent = `${loras.length}/${totalL} LoRAs · ${fts.length}/${totalF} checkpoints`;
    }
    if (
      filterOn() &&
      !loras.length &&
      !fts.length &&
      !(pack.civitai || []).some(matchesCiv)
    ) {
      host.appendChild(
        rowEl("ap-hint", "Nothing matches this filter — Clear it to see the whole catalog."),
      );
    }
    updateTotals();
  }

  // ── plan + totals ────────────────────────────────────────────────────────
  function plan() {
    const loraIds = Object.keys(sel.loras).filter((k) => sel.loras[k]);
    const ftIds = Object.keys(sel.fts).filter((k) => !k.startsWith("pf:") && sel.fts[k]);
    const prefetchIds = ftIds.filter((k) => sel.fts["pf:" + k]);
    const civSelIds = civOn() ? Object.keys(sel.civ).filter((k) => sel.civ[k]) : [];
    const lm = new Map((pack.loras || []).map((x) => [x.id, x]));
    const fm = new Map((pack.finetunes || []).map((x) => [x.id, x]));
    const cm = new Map((pack.civitai || []).map((x) => [x.id, x]));
    const ftCost = (id) => {
      const f = fm.get(id);
      return f ? (f.bytes || 0) + (f.bytesLow || 0) : 0;
    };
    // What still costs bytes. A LoRA whose size matches the verified catalog, a
    // checkpoint whose weights are on disk, and a CivitAI item with all its
    // recorded files present are not downloads; counting them made the footer
    // claim gigabytes right after a finished fetch.
    const pendingLoras = loraIds.filter((id) => !lm.get(id)?.installed);
    const pendingPrefetch = prefetchIds.filter((id) => !fm.get(id)?.weightsPresent);
    const civIds = civSelIds.filter((id) => !cm.get(id)?.installed);
    const loraBytes = pendingLoras.reduce((a, id) => a + (lm.get(id)?.bytes || 0), 0);
    const ckptBytes = pendingPrefetch.reduce((a, id) => a + ftCost(id), 0);
    const civBytes = civIds.reduce((a, id) => a + (cm.get(id)?.bytesEstimate || 0), 0);
    // CivitAI LoRAs land beside the other pack LoRAs, not in ckpts/. Split the
    // estimate so the free-space check looks at the right drive.
    const civIsLora = (id) => (cm.get(id)?.installedKind || cm.get(id)?.kind || "checkpoint") === "lora";
    const civLoraBytes = civIds.reduce(
      (a, id) => a + (civIsLora(id) ? cm.get(id)?.bytesEstimate || 0 : 0),
      0,
    );
    const civCkptBytes = civBytes - civLoraBytes;
    return {
      active: active(),
      loraIds,
      ftIds,
      prefetchIds,
      civIds,
      civSelIds,
      pendingLoras,
      pendingPrefetch,
      // Selected and already on disk — shown so "0 to download" is never
      // mistaken for "nothing selected".
      settled:
        loraIds.length - pendingLoras.length + (prefetchIds.length - pendingPrefetch.length) +
        (civSelIds.length - civIds.length),
      loraBytes,
      ckptBytes,
      civBytes,
      civLoraBytes,
      civCkptBytes,
      nowBytes: loraBytes + ckptBytes + civBytes,
      // Items that need bytes moved. Recipe-only checkpoints are not one.
      count: pendingLoras.length + pendingPrefetch.length + civIds.length,
      selected: loraIds.length + ftIds.length + civSelIds.length,
    };
  }

  function updateTotals() {
    const p = plan();
    const t = $("apTotalText");
    if (!t) return;
    // Installed counts come from the manifest enrichment (what is on disk and
    // the right size), so these chips answer "what have I already got?".
    const chips = $("apChips");
    if (chips) {
      chips.innerHTML = "";
      const loras = (pack.loras || []);
      const got = loras.filter((l) => l.installed).length;
      const stale = loras.filter((l) => l.installedStale).length;
      const fts = (pack.finetunes || []);
      const recipes = fts.filter((f) => f.finetuneWritten).length;
      const weights = fts.filter((f) => f.weightsPresent).length;
      const pinned = loras.filter((l) => l.overridden).length;
      const civs = (pack.civitai || []);
      const civGot = civs.filter((c) => c.installed).length;
      const add = (label, value, cls, title) => {
        const c = rowEl("ap-chip " + (cls || ""), `${value} ${label}`, title || "");
        chips.appendChild(c);
      };
      add("downloaded", `${got}/${loras.length}`, got ? "ok" : "", "LoRA files on disk matching the verified size");
      if (stale) add("need update", String(stale), "warn", "Downloaded, but upstream has newer bytes");
      if (pinned) add("updated pins", String(pinned), "warn", "Local hash newer than the shipped catalog");
      add("recipes written", `${recipes}/${fts.length}`, recipes ? "ok" : "", "finetunes/*.json in place");
      add("weights on disk", `${weights}/${fts.length}`, weights ? "ok" : "", "Checkpoint files already fetched");
      if (civs.length) {
        add("CivitAI installed", `${civGot}/${civs.length}`, civGot ? "ok" : "", "CivitAI files this launcher downloaded and still finds on disk");
      }
    }
    if (!p.selected) {
      t.textContent = enabled()
        ? "Nothing selected"
        : "Nothing selected — search above, or enable the pack";
      const f = $("apFreeText");
      if (f) f.textContent = "";
      return;
    }
    const parts = [];
    if (p.count) {
      parts.push(
        `${p.count} item${p.count === 1 ? "" : "s"} to download · ${fmtBytes(p.nowBytes)}`,
      );
    } else {
      parts.push("Nothing to download");
    }
    if (p.settled) parts.push(`${p.settled} already on disk`);
    if (p.ftIds.length) {
      const n = p.ftIds.length;
      parts.push(
        `${n} checkpoint${n === 1 ? "" : "s"} as recipes (fetched only when you run ${n === 1 ? "it" : "them"})`,
      );
    }
    t.textContent = parts.join(" · ");

    const f = $("apFreeText");
    if (!f) return;
    if (!p.count) {
      f.textContent = "";
      return;
    }
    // LoRAs and prefetched checkpoints can live on different drives, so check
    // the folders this selection will actually write to and report the tightest.
    const ckptsRoot = pack.paths && (pack.paths.ckptsPack || pack.paths.ckpts);
    const targets = [
      { dir: pack.paths && pack.paths.loras, bytes: p.loraBytes + p.civLoraBytes },
      { dir: ckptsRoot, bytes: p.ckptBytes + p.civCkptBytes },
    ].filter((x) => x.dir && x.bytes > 0);
    if (!targets.length) {
      f.textContent = "";
      return;
    }
    Promise.all(targets.map((x) => window.w2gp.getDiskSpace(x.dir).catch(() => null)))
      .then((spaces) => {
        if (spaces.some((d) => !d || typeof d.free !== "number")) return;
        const remaining = spaces.map((d, i) => d.free - targets[i].bytes);
        const tightest = remaining.indexOf(Math.min(...remaining));
        f.textContent =
          `${fmtBytes(spaces[tightest].free)} free — ` +
          (remaining[tightest] < 0
            ? "⚠ not enough space for this selection"
            : `${fmtBytes(remaining[tightest])} would remain`);
      })
      .catch(() => {});
  }

  // ── actions ──────────────────────────────────────────────────────────────
  function setBusy(on) {
    busy = on;
    for (const id of ["apDownloadBtn", "apRecipesBtn", "apMissingBtn", "apRemoveBtn"]) {
      const b = $(id);
      if (b) b.disabled = on;
    }
  }

  async function withBusy(label, fn) {
    if (busy) return;
    setBusy(true);
    setProgress(label);
    try {
      await fn();
    } catch (e) {
      logLine("✗ " + (errText(e) || e), "err");
      setProgress("Failed — see above.");
    } finally {
      setBusy(false);
    }
  }

  // ── updates ─────────────────────────────────────────────────────────────
  // The catalog pins every file's SHA-256, so "is there an update?" is answered
  // exactly rather than guessed from a date: one listing per repo, compared file
  // by file. Applying an update re-points the local hash, so the next download
  // verifies the new bytes instead of rejecting them against the old pin.
  let updates = null;
  let updateSel = new Set();

  // ── download queue ───────────────────────────────────────────────────────
  // The fetch batch is sequential in the backend, but that is an implementation
  // detail the user should not have to infer from a scrolling log. One row per
  // selected item, fed by the progress events (`queueIndex`/`queueTotal`).
  let queue = null;

  /** Display name for an id, from the catalog. A `#low` suffix addresses the
   *  LOW half of a recipe, so it resolves to the recipe's own name. */
  function queueName(id) {
    const base = String(id).replace(/#low$/, "");
    const all = [...((pack && pack.loras) || []), ...((pack && pack.finetunes) || [])];
    const hit = all.find((x) => x.id === base);
    return (hit && hit.name) || base;
  }

  function startQueue(ids) {
    const order = [];
    const byId = new Map();
    for (const id of ids) {
      if (byId.has(id)) continue;
      order.push(id);
      byId.set(id, { id, name: queueName(id), status: "queued", received: 0, total: 0 });
    }
    queue = { total: order.length, order, byId, active: true };
    renderQueue();
  }

  /** Fold one progress event into the queue. The event's `id` is the selected
   *  item, so a HIGH/LOW recipe's two files land on the one row the user ticked.
   *  Only an in-flight batch is folded: CivitAI items and the update check emit
   *  on the same channel, and must not rewrite a finished run's rows. */
  function updateQueue(ev) {
    if (!queue || !queue.active || !ev || !ev.id) return;
    let row = queue.byId.get(ev.id);
    if (!row) {
      row = { id: ev.id, name: ev.name || queueName(ev.id), status: "queued", received: 0, total: 0 };
      queue.order.push(ev.id);
      queue.byId.set(ev.id, row);
      queue.total = queue.order.length;
    }
    if (ev.name) row.name = ev.name;
    if (ev.phase === "done") row.status = "done";
    else if (ev.phase === "verify") row.status = "verifying";
    else if (ev.phase === "start" || ev.phase === "download") row.status = "downloading";
    row.received = Number(ev.received) || 0;
    if (ev.total) row.total = Number(ev.total) || 0;
    renderQueue();
  }

  /** Anything never reached is reported as such; anything interrupted mid-file
   *  is a failure. The per-item reason is in the log, next to the queue. */
  function finishQueue() {
    if (!queue) return;
    for (const row of queue.byId.values()) {
      if (row.status === "done") continue;
      row.status = row.status === "queued" ? "skipped" : "failed";
    }
    queue.active = false;
    renderQueue();
  }

  function renderQueue() {
    const host = $("apQueue");
    if (!host) return;
    if (!queue || !queue.order.length) {
      host.innerHTML = "";
      return;
    }
    const rows = queue.order.map((id) => queue.byId.get(id)).filter(Boolean);
    const done = rows.filter((r) => r.status === "done").length;
    host.innerHTML = "";
    host.appendChild(
      rowEl(
        "ap-queue-head",
        `${done}/${queue.total} done`,
        "Files download one at a time; each is checked against its pinned SHA-256 before it is kept.",
      ),
    );
    for (const r of rows) {
      const row = rowEl("ap-queue-row " + r.status);
      row.appendChild(rowEl("ap-queue-name", r.name, r.id));
      let text = "queued";
      if (r.status === "done") text = fmtBytes(r.total || r.received);
      else if (r.status === "verifying") text = "verifying…";
      else if (r.status === "downloading")
        text = `${fmtBytes(r.received)}${r.total ? " / " + fmtBytes(r.total) : ""}`;
      else if (r.status === "failed") text = "✗ failed";
      else if (r.status === "skipped") text = "– not attempted";
      row.appendChild(rowEl("ap-queue-status", text));
      host.appendChild(row);
    }
  }

  async function runUpdateCheck() {
    const status = $("apUpdStatus");
    const host = $("apUpdList");
    if (!status || !host) return;
    host.innerHTML = "";
    status.textContent = "Checking every catalog repo…";
    updateSel = new Set();
    try {
      const r = await window.w2gp.adultPackUpdates();
      updates = r;
      renderUpdates();
      const changed = (r.items || []).filter((it) => it.status === "updated" || it.status === "removed");
      const stale = (r.items || []).filter((it) => it.installedStale);
      const failed = (r.failed || []).length;
      status.textContent =
        `${r.reposChecked} repo(s) checked · ${changed.length} file(s) changed upstream · ${stale.length} downloaded file(s) out of date` +
        (failed ? ` · ${failed} repo(s) could not be reached` : "");
      for (const f of r.failed || []) {
        logLine(`✗ ${f.repo || "repo"}: ${f.error || "unreachable"}`, "err");
      }
    } catch (e) {
      status.textContent = "Could not check for updates: " + (errText(e) || e);
    }
  }

  function renderUpdates() {
    const host = $("apUpdList");
    if (!host || !updates) return;
    host.innerHTML = "";
    const rows = (updates.items || []).filter((it) => it.status !== "current" || it.installedStale);
    if (!rows.length) {
      host.appendChild(rowEl("ap-hint", "Everything matches the sources — no updates pending."));
      const b = $("apUpdApplyBtn");
      if (b) b.disabled = true;
      return;
    }
    for (const it of rows) {
      const row = rowEl("ap-upd-row");
      const cb = checkBox(false, (on) => {
        if (on) updateSel.add(it.id);
        else updateSel.delete(it.id);
        const b = $("apUpdApplyBtn");
        if (b) b.disabled = updateSel.size === 0;
      }, it.path);
      cb.input.disabled = !it.liveSha;
      row.appendChild(cb.label);
      const nm = rowEl("ap-name");
      nm.textContent = it.name + (it.half === "low" ? " (low)" : "");
      // Three different situations, three different words: holding outdated
      // bytes, holding current bytes while upstream moved, and not holding the
      // file at all.
      const badge = it.installedStale
        ? ["upd", "update available", "On disk, but upstream replaced these bytes"]
        : it.status === "removed"
        ? ["warn", "removed upstream", "This file no longer exists in the source repo"]
        : it.installed
        ? ["info", "newer upstream", "Your copy matches the pinned hash; the source has since changed"]
        : ["info", "new version", "Not downloaded yet, and upstream has newer bytes"];
      nm.appendChild(
        rowEl("ap-tag " + badge[0], badge[1], badge[2] + (it.repoLastModified ? ` — repo last changed ${String(it.repoLastModified).slice(0, 10)}` : "")),
      );
      if (it.installed) nm.appendChild(rowEl("ap-tag done", "downloaded"));
      row.appendChild(nm);
      const size = rowEl("ap-size");
      size.textContent = it.installedStale
        ? `${fmtBytes(it.bytesOnDisk)} → ${fmtBytes(it.liveBytes)}`
        : fmtBytes(it.liveBytes || it.pinnedBytes);
      row.appendChild(size);
      host.appendChild(row);
    }
    const b = $("apUpdApplyBtn");
    if (b) b.disabled = true;
  }

  async function applyUpdates() {
    const ids = [...updateSel];
    if (!ids.length) return;
    await withBusy("Applying updates…", async () => {
      // Pins first: the new hash has to be in place before the bytes are
      // verified against it.
      const applied = await window.w2gp.adultPackApplyUpdates(ids);
      // Call the unwrapped worker: doFetch() would re-enter withBusy() and
      // return immediately because this outer call already holds the guard.
      await fetchNow(ids);
      const st = $("apUpdStatus");
      if (st) st.textContent = `${(applied.applied || []).length} hash pin(s) updated, ${(applied.skipped || []).length} skipped`;
      await runUpdateCheck();
    });
  }

  /** The download work, with no busy guard of its own so a caller that already
   *  holds one (Apply updates) can reuse it. */
  async function fetchNow(ids) {
    if (!ids.length) {
      logLine("Nothing selected to download.");
      return;
    }
    startQueue(ids);
    let r;
    try {
      r = await window.w2gp.adultPackFetch(ids);
    } finally {
      // Even if the whole command errors, the rows should not sit at
      // "downloading" forever.
      finishQueue();
    }
    const got = (r && r.done && r.done.length) || 0;
    logLine(`✓ ${got}/${ids.length} file(s) downloaded — ${fmtBytes((r && r.bytes) || 0)}`, "ok");
    for (const f of (r && r.failed) || []) logLine(`✗ ${f.name || f.id}: ${f.error}`, "err");
    setProgress("Download finished.");
    await init(true);
  }

  async function doFetch(ids, label) {
    if (!ids.length) {
      logLine("Nothing selected to download.");
      return;
    }
    await withBusy(label + "…", () => fetchNow(ids));
  }

  async function doRecipes() {
    const p = plan();
    if (!p.ftIds.length) {
      logLine("No checkpoints selected — tick a finetune first.");
      return;
    }
    await withBusy("Writing finetune recipes…", async () => {
      const r = await window.w2gp.adultPackWriteFinetunes(p.ftIds);
      const n = (r && r.written && r.written.length) || 0;
      logLine(`✓ ${n} recipe(s) written to ${(r && r.dir) || "finetunes\\"}`, "ok");
      for (const s of (r && r.skipped) || []) logLine(`✗ skipped ${s.id}: ${s.reason}`, "err");
      logLine("Restart Wan2GP (or Refresh Model List) to see them in the model dropdown.");
      setProgress("Recipes written.");
      await init(true);
    });
  }

  async function doCivitai() {
    const p = plan();
    if (!p.civIds.length) return;
    const token = ($("apToken").value || "").trim();
    await withBusy("Fetching CivitAI items…", async () => {
      for (const id of p.civIds) {
        setProgress(`CivitAI: ${id}…`);
        try {
          const r = await window.w2gp.adultPackCivitaiDownload(id, token || null);
          const dest = r && r.kind === "lora" ? r.path : (r && r.finetune) || "";
          logLine(
            `✓ ${(r && r.files ? r.files.length : 0)} file(s) — ${(r && r.versionName) || id} → ${dest}`,
            "ok",
          );
        } catch (e) {
          logLine(`✗ ${id}: ${errText(e)}`, "err");
        }
      }
      setProgress("CivitAI fetch finished.");
      await init(true);
    });
  }

  async function doMissing() {
    const p = plan();
    // CivitAI items are downloads too; otherwise "Fetch missing only" would
    // claim everything is on disk while a selected CivitAI row is untouched.
    if (p.civIds.length) await doCivitai();
    // Missing bytes only: selected LoRAs not matching the verified size, plus
    // prefetched checkpoints whose weights are not on disk yet.
    const missing = [...p.pendingLoras, ...p.pendingPrefetch];
    if (!missing.length) {
      if (!p.civIds.length) {
        logLine("Everything selected is already on disk (and hash-matching).");
      }
      return;
    }
    await doFetch(missing, "Downloading missing files");
  }

  async function doRemove() {
    const p = plan();
    const ids = [...p.loraIds, ...p.ftIds, ...p.civSelIds];
    if (!ids.length) {
      logLine("Nothing selected.");
      return;
    }
    if (
      !(await askConfirm(
        `Remove ${ids.length} selected pack item(s) from disk?\n\n` +
          "LoRA files are deleted, and a selected checkpoint's prefetched weights plus its " +
          "finetune recipe are deleted. Nothing outside the pack folders is touched.",
        "Remove pack items",
      ))
    )
      return;
    await withBusy("Removing…", async () => {
      const r = await window.w2gp.adultPackRemove(ids);
      const n = ((r && r.removed) || []).length;
      if (n) logLine(`✓ removed ${n} item(s)`, "ok");
      for (const f of (r && r.failed) || []) {
        logLine(`✗ ${f.file || f.id}: ${f.error}`, "err");
      }
      if ((r && r.failed && r.failed.length)) {
        logLine("Some files are still on disk — close Wan2GP or whatever is using them, then remove again.", "err");
      }
      setProgress("Removed.");
      await init(true);
    });
  }

  async function verifyToken() {
    const out = $("apVerifyResult");
    const btn = $("apVerifyBtn");
    const tok = ($("apToken").value || "").trim();
    if (!tok) {
      out.textContent = "Paste a token first.";
      return;
    }
    btn.disabled = true;
    out.textContent = "Checking…";
    try {
      const r = await window.w2gp.adultPackCivitaiVerify(tok);
      out.textContent = r && r.ok ? `✓ verified as ${r.username}` : `✗ ${(r && r.error) || "rejected"}`;
      if (r && r.ok) {
        persist();
        render();
      }
    } catch (e) {
      out.textContent = `✗ ${errText(e)}`;
    }
    btn.disabled = false;
  }

  // ── finetune manager ──────────────────────────────────────────────────
  // The pack writes recipes; this is where you actually manage them — read,
  // edit, validate, check the links, delete. WanGP parses finetunes/*.json at
  // startup, so a bad edit shows up as a model that never appears; every write
  // therefore goes through the backend's validator first.
  let fmItems = [];
  // True once finetunes/ has actually been read; the create form refuses an
  // existing id, and that guard is worthless before the list exists.
  let fmLoaded = false;

  function fmRow(ft, host) {
    const row = rowEl("ap-fm-row");
    const main = rowEl("ap-fm-main");
    main.appendChild(rowEl("ap-sr-title", ft.name || ft.id));
    const bits = [
      ft.architecture || "—",
      `${ft.urlCount || 0} source(s)`,
      ft.origin === "pack"
        ? "from pack"
        : ft.origin === "civitai"
        ? "from pack (CivitAI)"
        : ft.origin === "custom"
        ? "yours (added via pack)"
        : "yours (imported or handwritten)",
      ft.written ? "in finetunes/" : "not written",
    ];
    if (ft.hasLocalWeights) bits.push(`weights on disk (${fmtBytes(ft.localBytes || 0)})`);
    if (ft.needsToken) bits.push("CivitAI · token only if gated");
    main.appendChild(rowEl("ap-sr-meta", bits.join(" · ")));
    row.appendChild(main);

    const open = rowEl("ap-sr-add", "Edit…", "Open this recipe");
    open.addEventListener("click", () => openFinetuneEditor(ft, host));
    row.appendChild(open);
    host.appendChild(row);
    return row;
  }

  async function openFinetuneEditor(ft, host) {
    const box = rowEl("ap-fm-editor");
    box.innerHTML = "";
    host.appendChild(box);
    box.appendChild(rowEl("ap-hint", "Loading " + ft.id + "…"));
    let content;
    try {
      const r = await window.w2gp.adultPackFinetuneRead(ft.id);
      content = r.content;
    } catch (e) {
      box.innerHTML = "";
      box.appendChild(rowEl("ap-hint", "No recipe written yet — tick it and press “Write finetune recipes”, or write one here."));
      const ta = document.createElement("textarea");
      ta.className = "ap-fm-text";
      ta.value = JSON.stringify(
        { model: { name: ft.name || ft.id, architecture: ft.architecture || "i2v_2_2", description: "", URLs: [] } },
        null,
        2,
      );
      box.appendChild(ta);
      box.appendChild(fmEditorActions(ft, ta, box, host, null));
      return;
    }
    box.innerHTML = "";
    const ta = document.createElement("textarea");
    ta.className = "ap-fm-text";
    ta.value = content;
    box.appendChild(ta);
    box.appendChild(fmEditorActions(ft, ta, box, host, content));
  }

  /** Save a recipe to disk as a .json the user can share. */
  function downloadRecipe(id, text) {
    try {
      const blob = new Blob([text], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `${id}.json`;
      document.body.appendChild(a);
      a.click();
      a.remove();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
    } catch {}
  }

  function fmEditorActions(ft, ta, box, host, content) {
    const bar = rowEl("ap-fm-actions");
    const status = rowEl("ap-hint");

    const save = rowEl("ap-sr-add", "Save", "Validate and write to finetunes/");
    save.addEventListener("click", async () => {
      try {
        const r = await window.w2gp.adultPackFinetuneWrite(ft.id, ta.value);
        status.textContent = `✓ saved — ${r.name} (${r.architecture})`;
        loadFinetunes();
      } catch (e) {
        status.textContent = "✗ " + (errText(e) || e);
      }
    });

    const check = rowEl("ap-sr-add", "Check links", "HEAD each source URL");
    check.addEventListener("click", async () => {
      check.disabled = true;
      status.textContent = "Checking…";
      try {
        const r = await window.w2gp.adultPackFinetuneCheck(ft.id);
        const bad = (r.results || []).filter((x) => !x.ok);
        status.textContent = bad.length
          ? `${bad.length} of ${r.results.length} source(s) unavailable — ${bad[0].url.slice(0, 70)}`
          : `all ${r.results.length} source(s) reachable`;
      } catch (e) {
        status.textContent = "✗ " + (errText(e) || e);
      }
      check.disabled = false;
    });

    const copy = rowEl("ap-sr-add", "Copy", "Copy the JSON to the clipboard");
    copy.addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText(ta.value);
        status.textContent = "✓ copied — share it like any Wan2GP finetune JSON";
      } catch (e) {
        status.textContent = "✗ clipboard unavailable";
      }
    });

    // A recipe that parses can be edited in the same form used to create one,
    // instead of only as raw JSON.
    let parsed = null;
    try {
      parsed = content ? JSON.parse(content) : null;
    } catch {}
    if (parsed && parsed.model) {
      const form = rowEl("ap-sr-add", "Form view", "Edit with the same form as New finetune.json");
      form.addEventListener("click", () => {
        box.remove();
        openRecipeForm({ host, id: ft.id, doc: parsed });
      });
      bar.appendChild(form);
    }

    const exp = rowEl("ap-sr-add", "Export", "Download this recipe as a .json file to share or back up");
    exp.addEventListener("click", () => {
      downloadRecipe(ft.id, ta.value);
      status.textContent = `✓ saved ${ft.id}.json`;
    });
    bar.appendChild(exp);

    const del = rowEl("ap-sr-add", "Delete recipe", "Remove this JSON from finetunes/ (weights stay)");
    del.addEventListener("click", async () => {
      if (!(await askConfirm(`Delete ${ft.id}.json from finetunes/?\n\nDownloaded weights are left on disk.`, "Delete recipe"))) return;
      try {
        await window.w2gp.adultPackRemove([ft.id]);
        logLine(`✓ removed recipe ${ft.id}`, "ok");
        box.remove();
        loadFinetunes();
      } catch (e) {
        status.textContent = "✗ " + (errText(e) || e);
      }
    });

    bar.appendChild(save);
    bar.appendChild(check);
    bar.appendChild(copy);
    bar.appendChild(del);
    const wrap = rowEl("ap-fm-bar");
    wrap.appendChild(bar);
    wrap.appendChild(status);
    return wrap;
  }

  /** Read one textarea as a list of non-empty lines. */
  function listLines(el) {
    return el.value
      .split(/\r?\n/)
      .map((s) => s.trim())
      .filter(Boolean);
  }

  /** 1024x2048 -> "1024x2048 (1:2)". Wan2GP shows the label, stores the value.
   *  A ratio the author typed in parentheses is kept as written. */
  function resolutionPair(line) {
    const m = /^(\d{2,5})\s*[x×]\s*(\d{2,5})(?:\s*\(([^)]+)\))?$/i.exec(line);
    if (!m) return null;
    const w = Number(m[1]);
    const h = Number(m[2]);
    const value = `${w}x${h}`;
    const label = m[3] ? `${value} (${m[3].trim()})` : `${value} (${ratioLabel(w, h)})`;
    return [label, value];
  }

  function ratioLabel(w, h) {
    const gcd = (a, b) => (b ? gcd(b, a % b) : a);
    const g = gcd(w, h) || 1;
    return `${w / g}:${h / g}`;
  }

  /** Create a recipe from scratch without knowing the JSON shape. The backend
   *  still validates every field; this only collects them in the right place
   *  so a user-added model does not require hand-writing finetunes/<id>.json.
   *  Upstream's FINETUNES.md is the field reference. */
  /** The architecture list both the recipe form and Add-by-link use: what this
   *  build actually ships first, then the curated map. Falls back to archMap if
   *  the backend lookup failed, so the form is never empty. */
  function archOptions() {
    if (arches && arches.length) {
      return arches.map((a) => ({
        value: a.id,
        label: `${a.label || a.id}${a.installed === false ? " — not installed" : ""}`,
      }));
    }
    const out = [];
    const seen = new Set();
    for (const e of (pack && pack.archMap) || []) {
      const value = e.architecture || (e.wan2gpIds || [])[0] || e.id;
      if (!value || seen.has(value)) continue;
      seen.add(value);
      out.push({ value, label: e.label || value });
    }
    return out;
  }

  function fillArchSelect() {
    const el = $("apUrlArch");
    if (!el) return;
    const prev = el.value;
    el.innerHTML = "";
    for (const o of archOptions()) {
      const opt = document.createElement("option");
      opt.value = o.value;
      opt.textContent = o.label;
      el.appendChild(opt);
    }
    if (prev && [...el.options].some((o) => o.value === prev)) el.value = prev;
  }

  function openNewFinetuneEditor() {
    openRecipeForm({ host: $("apFmNewHost") });
  }

  /** One form for both "New finetune.json" and "Edit… → Form view". With an id
   *  and a doc it edits that recipe in place; without, it creates a new one. */
  function openRecipeForm(opts) {
    opts = opts || {};
    const host = opts.host || $("apFmNewHost");
    const editId = opts.id || "";
    const seedDoc = opts.doc || null;
    if (!host) return;
    if (!editId && host.firstChild) {
      host.firstChild.scrollIntoView({ block: "center" });
      return;
    }
    const box = rowEl("ap-fm-editor");
    box.appendChild(
      rowEl(
        "ap-hint",
        editId
          ? `Editing finetunes/${editId}.json — Save writes the change; the file id stays fixed.`
          : "Writes finetunes/<id>.json — Wan2GP offers it in the model dropdown after a refresh. Source URLs are the only required field; the other tabs carry the WanGP fields a hand-written recipe usually sets.",
      ),
    );

    const input = (placeholder) => {
      const el = document.createElement("input");
      el.className = "input";
      el.placeholder = placeholder;
      el.autocomplete = "off";
      return el;
    };
    const area = (placeholder, small) => {
      const el = document.createElement("textarea");
      el.className = "ap-fm-text" + (small ? " ap-fm-text-sm" : "");
      el.placeholder = placeholder;
      return el;
    };
    const field = (pane, label, el) => {
      const r = rowEl("ap-fm-field");
      if (el.tagName === "TEXTAREA") r.classList.add("ap-fm-field-top");
      r.appendChild(rowEl("ap-detail-k", label));
      r.appendChild(el);
      pane.appendChild(r);
    };

    // ── tab strip: six panes in one editor instead of a 20-field column ──
    const tabBar = rowEl("ap-fm-tabs");
    const paneHost = rowEl("ap-fm-panes");
    const panes = {};
    const tabBtns = [];
    const addPane = (key, label, title) => {
      const pane = rowEl("ap-fm-pane");
      pane.style.display = "none";
      panes[key] = pane;
      paneHost.appendChild(pane);
      const b = rowEl("ap-fm-tab", label, title || "");
      b.addEventListener("click", () => {
        for (const [k, p] of Object.entries(panes)) p.style.display = k === key ? "" : "none";
        for (const t of tabBtns) t.classList.toggle("active", t === b);
      });
      tabBar.appendChild(b);
      tabBtns.push(b);
    };
    for (const [key, label, title] of [
      ["basics", "Basics", "File id, name, base architecture and description"],
      ["urls", "URLs", "Checkpoint sources, second phase, text encoder, VAE and preload"],
      ["loras", "LoRAs", "Always-loaded LoRAs and their multipliers"],
      ["res", "Resolutions", "Custom resolutions and resolution categories"],
      ["help", "Help", "Markdown shown behind the model's information buttons"],
      ["adv", "Advanced", "Modules, source model, quantization, visibility, image-only"],
    ]) {
      addPane(key, label, title);
    }
    box.append(tabBar, paneHost);
    panes.basics.style.display = "";
    tabBtns[0].classList.add("active");

    const status = rowEl("ap-hint");

    // ── Basics ──
    const id = input("my_model (letters, digits, - and _)");
    if (editId) {
      id.value = editId;
      id.disabled = true;
    }
    field(panes.basics, "File id", id);
    const name = input("Name Wan2GP shows");
    field(panes.basics, "Name", name);
    const arch = document.createElement("select");
    arch.className = "input";
    // The recipe needs a Wan2GP architecture id. Prefer the list of what this
    // build actually ships; fall back to the curated map when that lookup failed.
    const seenArch = new Set();
    for (const e of archOptions()) {
      if (!e.value || seenArch.has(e.value)) continue;
      seenArch.add(e.value);
      const o = document.createElement("option");
      o.value = e.value;
      o.textContent = e.label || e.value;
      arch.appendChild(o);
    }
    field(panes.basics, "Architecture", arch);
    const desc = input("Optional description");
    field(panes.basics, "Description", desc);

    // Seeding copies a model subtree (URLs, LoRAs, resolutions, help text) so
    // a variant of an existing recipe does not have to be retyped.
    const seed = document.createElement("select");
    seed.className = "input";
    const none = document.createElement("option");
    none.value = "";
    none.textContent = "— start empty —";
    seed.appendChild(none);
    for (const ft of fmItems) {
      const o = document.createElement("option");
      o.value = ft.id;
      o.textContent = `${ft.name || ft.id}${ft.written ? "" : " (not written yet)"}`;
      seed.appendChild(o);
    }
    field(panes.basics, "Start from", seed);
    if (editId) seed.disabled = true;

    // ── URLs ──
    const urls = area("One source URL per line (https://…)");
    field(panes.urls, "Source URLs", urls);
    const reuse = input("Optional: an existing recipe id to reuse its weights (e.g. t2v_2_2)");
    field(panes.urls, "Reuse weights", reuse);
    const urls2 = area("Optional second phase, one URL per line (e.g. Wan 2.2 low-noise weights)");
    field(panes.urls, "Second phase", urls2);
    const reuse2 = input("Optional: an existing recipe id to reuse its second-phase weights");
    field(panes.urls, "Reuse second phase", reuse2);
    const te = area("Optional text encoder URLs, one per line — overrides the architecture default");
    field(panes.urls, "Text encoder", te);
    const vae = area("Optional VAE URLs, one per line — overrides the architecture default");
    field(panes.urls, "VAE", vae);
    const preload = area("Optional files to always download, one URL per line");
    field(panes.urls, "Preload", preload);

    // ── LoRAs ──
    const loras = area("One LoRA URL or local path per line", true);
    field(panes.loras, "LoRAs", loras);
    const mults = area("One per line, same order — a number, or a schedule such as 1,1.1;0", true);
    field(panes.loras, "Multipliers", mults);

    // ── Resolutions ──
    const res = area("One WxH per line, e.g. 832x480 — the ratio label is generated", true);
    field(panes.res, "Custom", res);
    const resCat = area("One condition per line, e.g. 720p, 1080p, >=720&<=1440", true);
    field(panes.res, "Categories", resCat);

    // ── Help ──
    const infos = area("Optional Markdown model notes (recommended resolutions, limitations, licence)");
    field(panes.help, "Model notes", infos);
    const promptInfos = area("Optional Markdown prompt-writing guidance");
    field(panes.help, "Prompt help", promptInfos);

    // ── Advanced ──
    const modules = area("One module per line — vace_14B, multitalk", true);
    field(panes.adv, "Modules", modules);
    const sourceModel = input("Optional: source model id the Finetune Editor starts from (defaults to architecture)");
    field(panes.adv, "Source model", sourceModel);
    const boolRow = (label, title, checked) => {
      const l = document.createElement("label");
      l.className = "ap-fm-check";
      l.title = title || "";
      const cb = document.createElement("input");
      cb.type = "checkbox";
      cb.checked = !!checked;
      l.append(cb, document.createTextNode(label));
      panes.adv.appendChild(l);
      return cb;
    };
    const autoQ = boolRow(
      "Auto-quantize when only unquantized weights are offered",
      "WanGP quantizes on the fly when the user asks for a quantized model",
      false,
    );
    const visible = boolRow(
      "Visible in the model list",
      "Uncheck to keep an override that hides a base model",
      true,
    );
    const imgOut = boolRow(
      "Image-only model",
      "Turns a video architecture into an image generator (a one-frame video)",
      false,
    );

    seed.addEventListener("change", async () => {
      if (!seed.value) return;
      status.textContent = "Loading " + seed.value + "…";
      try {
        const r = await window.w2gp.adultPackFinetuneRead(seed.value);
        const doc = JSON.parse((r && r.content) || "{}");
        fillFromModel(doc.model || {});
        status.textContent = `Loaded ${seed.value} — pick a new file id before creating.`;
      } catch (e) {
        status.textContent = "✗ could not read the recipe: " + (errText(e) || e);
      }
    });

    function fillFromModel(m) {
      const setLines = (el, v) => {
        el.value = Array.isArray(v)
          ? v
              .map((x) => (Array.isArray(x) ? x[1] || x[0] : x))
              .join("\n")
          : typeof v === "string"
            ? v
            : "";
      };
      name.value = m.name || "";
      // Round-trip the architecture even when this build does not offer it, so a
      // save never silently swaps it for the first option. The backend refuses
      // the write with a clear reason instead.
      if (m.architecture && ![...arch.options].some((o) => o.value === m.architecture)) {
        const o = document.createElement("option");
        o.value = m.architecture;
        o.textContent = `${m.architecture} (not offered by this build)`;
        arch.appendChild(o);
      }
      if (m.architecture) arch.value = m.architecture;
      desc.value = m.description || "";
      if (typeof m.URLs === "string") {
        reuse.value = m.URLs;
        urls.value = "";
      } else {
        reuse.value = "";
        setLines(urls, m.URLs);
      }
      if (typeof m.URLs2 === "string") {
        reuse2.value = m.URLs2;
        urls2.value = "";
      } else {
        reuse2.value = "";
        setLines(urls2, m.URLs2);
      }
      setLines(te, m.text_encoder_URLs);
      setLines(vae, m.VAE_URLs);
      setLines(preload, m.preload_URLs);
      setLines(loras, m.loras);
      setLines(mults, m.loras_multipliers);
      setLines(res, m.resolutions);
      setLines(resCat, m.resolutions_categories);
      setLines(modules, m.modules);
      infos.value = m.infos || "";
      promptInfos.value = m.prompt_infos || "";
      sourceModel.value = m.finetune_source_model || "";
      autoQ.checked = m.auto_quantize === true;
      visible.checked = m.visible !== false;
      imgOut.checked = m.image_outputs === true;
    }

    if (seedDoc) {
      fillFromModel(seedDoc.model || seedDoc);
      status.textContent = `Editing ${editId} — Save writes the change.`;
    }

    const bar = rowEl("ap-fm-actions");
    const save = rowEl(
      "ap-sr-add",
      editId ? "Save changes" : "Create recipe",
      editId ? "Validate and update finetunes/<id>.json" : "Validate and write finetunes/<id>.json",
    );
    save.addEventListener("click", async () => {
      const fid = id.value.trim();
      if (!/^[A-Za-z0-9_-]{1,64}$/.test(fid)) {
        status.textContent = "✗ id: letters, digits, - and _ only (max 64)";
        return;
      }
      // The list may still be loading (or the panel was never opened before
      // this click). Read finetunes/ before deciding the id is free, or an
      // existing recipe would be replaced without the Edit path ever showing.
      // An edit already owns its file, so the check does not apply.
      if (!editId) {
        if (!fmLoaded) await loadFinetunes();
        // Compare file stems, not just manifest ids: a pack recipe is listed as
        // `wan_rapid_aio` but lives in `adultpack_wan_rapid_aio.json`, and the
        // backend resolves that name too — typing it would replace the recipe.
        const taken = new Set();
        for (const f of fmItems) {
          if (f.id) taken.add(f.id);
          const stem = String(f.path || "")
            .split(/[\\/]/)
            .pop()
            .replace(/\.json$/i, "");
          if (stem) taken.add(stem);
        }
        if (taken.has(fid)) {
          status.textContent = `✗ ${fid} already exists — open it below and press Edit`;
          return;
        }
      }
      const primary = listLines(urls);
      const reuseId = reuse.value.trim();
      const second = listLines(urls2);
      const reuseSecond = reuse2.value.trim();
      if (!primary.length && !reuseId) {
        status.textContent = "✗ at least one source URL (or a recipe to reuse) is required";
        return;
      }
      if (primary.length && reuseId) {
        status.textContent = "✗ use either source URLs or a recipe id, not both";
        return;
      }
      if (second.length && reuseSecond) {
        status.textContent = "✗ use either second-phase URLs or a recipe id, not both";
        return;
      }
      const loraUrls = listLines(loras);
      const multLines = listLines(mults);
      if (multLines.length && !loraUrls.length) {
        status.textContent = "✗ multipliers need LoRAs above them";
        return;
      }
      if (multLines.length !== loraUrls.length && multLines.length) {
        status.textContent = `✗ ${loraUrls.length} LoRA(s) but ${multLines.length} multiplier(s) — they must match, in order`;
        return;
      }

      const model = {
        name: name.value.trim(),
        architecture: arch.value,
        description: desc.value.trim(),
      };
      model.URLs = reuseId || primary;
      const addList = (key, el) => {
        const v = listLines(el);
        if (v.length) model[key] = v;
      };
      addList("text_encoder_URLs", te);
      addList("VAE_URLs", vae);
      addList("preload_URLs", preload);
      addList("loras", loras);
      if (reuseSecond) model.URLs2 = reuseSecond;
      else if (second.length) model.URLs2 = second;
      const notes = infos.value.trim();
      if (notes) model.infos = notes;
      const promptHelp = promptInfos.value.trim();
      if (promptHelp) model.prompt_infos = promptHelp;
      if (multLines.length) {
        model.loras_multipliers = multLines.map((s) =>
          /^[+-]?(\d+(\.\d*)?|\.\d+)$/.test(s) ? Number(s) : s,
        );
      }
      const resLines = listLines(res);
      if (resLines.length) {
        const pairs = [];
        for (const line of resLines) {
          const pair = resolutionPair(line);
          if (!pair) {
            status.textContent = `✗ resolution “${line}” is not WxH — write it like 832x480`;
            return;
          }
          pairs.push(pair);
        }
        model.resolutions = pairs;
      }
      addList("resolutions_categories", resCat);
      addList("modules", modules);
      if (autoQ.checked) model.auto_quantize = true;
      if (!visible.checked) model.visible = false;
      if (imgOut.checked) model.image_outputs = true;
      const src = sourceModel.value.trim();
      if (src) model.finetune_source_model = src;

      const doc = { model };
      save.disabled = true;
      status.textContent = "Saving…";
      try {
        const r = await window.w2gp.adultPackFinetuneWrite(fid, JSON.stringify(doc, null, 2));
        logLine(`✓ recipe ${editId ? "updated" : "created"} — ${r.name} (${r.architecture}) → ${r.path}`, "ok");
        host.innerHTML = "";
        loadFinetunes();
      } catch (e) {
        status.textContent = "✗ " + (errText(e) || e);
        save.disabled = false;
      }
    });
    const cancel = rowEl("ap-sr-add", "Cancel", "Close without writing");
    cancel.addEventListener("click", () => {
      host.innerHTML = "";
    });
    bar.append(save, cancel);
    box.appendChild(bar);
    box.appendChild(status);
    host.appendChild(box);
    if (!editId) id.focus();
  }

  async function loadFinetunes() {
    const host = $("apFmList");
    if (!host) return;
    try {
      const r = await window.w2gp.adultPackFinetuneList();
      fmItems = (r && r.items) || [];
      fmLoaded = true;
      host.innerHTML = "";
      if (!fmItems.length) {
        host.appendChild(rowEl("ap-hint", "No recipes yet. Tick some checkpoints and press “Write finetune recipes”."));
        return;
      }
      for (const ft of fmItems) fmRow(ft, host);
      if ($("apFmStatus")) {
        const written = fmItems.filter((f) => f.written).length;
        $("apFmStatus").textContent = `${written}/${fmItems.length} written · ${fmItems.filter((f) => f.hasLocalWeights).length} with weights on disk`;
      }
    } catch (e) {
      host.innerHTML = "";
      host.appendChild(rowEl("ap-hint", "Could not read finetunes/: " + (errText(e) || e)));
    }
  }

  async function checkAllFinetuneLinks() {
    const status = $("apFmStatus");
    if (!status) return;
    let bad = 0;
    let checked = 0;
    for (const ft of fmItems.filter((f) => f.written)) {
      status.textContent = `Checking ${ft.id}…`;
      try {
        const r = await window.w2gp.adultPackFinetuneCheck(ft.id);
        const n = (r.results || []).filter((x) => !x.ok).length;
        bad += n;
        checked += (r.results || []).length;
      } catch (e) {
        bad += 1;
      }
    }
    status.textContent = bad
      ? `${bad} of ${checked} source URL(s) unavailable`
      : `all ${checked} source URL(s) reachable`;
  }

  async function runBrowse() {
    const status = $("apBrowseStatus");
    const host = $("apBrowseResults");
    host.innerHTML = "";
    status.textContent = "Loading the ranking…";
    try {
      const r = await window.w2gp.adultPackBrowse(
        $("apBrowseSource").value,
        $("apBrowseSort").value,
        $("apBrowsePeriod").value,
        $("apBrowseKind").value,
        $("apBrowseFamily").value,
        24,
      );
      const items = (r && r.items) || [];
      const sortLabel = $("apBrowseSort").options[$("apBrowseSort").selectedIndex].text;
      status.textContent = items.length
        ? `${items.length} hot model(s) — ${sortLabel.toLowerCase()} — this Wan2GP build can load them`
        : `Nothing in this ${sortLabel.toLowerCase()} list maps to a family Wan2GP supports. Try a different base model or type.`;
      for (const it of items) host.appendChild(searchRow(it, openAddFlow));
    } catch (e) {
      status.textContent = "Could not load the ranking: " + (errText(e) || e);
    }
  }

  /** Both pickers read the same manifest map — one source of truth for which
   *  families this Wan2GP build supports. */
  function fillFamilyFilters() {
    for (const id of ["apSearchFamily", "apBrowseFamily"]) {
      const sel = $(id);
      if (!sel || sel.options.length > 1) continue;
      const seen = new Set();
      for (const e of (pack && pack.archMap) || []) {
        if (seen.has(e.id)) continue;
        seen.add(e.id);
        const o = document.createElement("option");
        o.value = e.id;
        o.textContent = e.label || e.id;
        sel.appendChild(o);
      }
    }
  }

  // ── search & add your own ──────────────────────────────────────────────
  // The user finds a model on HF or CivitAI, picks the file, and the launcher
  // writes the finetune recipe and files the download where Wan2GP looks for
  // it. The architecture comes from the manifest's archMap — a model whose
  // family this Wan2GP build cannot load is never offered.
  function searchRow(item, onAdd) {
    const row = rowEl("ap-sr");
    row.appendChild(thumbEl(item.preview || ((item.examples || [])[0] || {}).url, item.title));
    const body = rowEl("ap-sr-body");
    const title = rowEl("ap-sr-title", item.title || item.id);
    if (item.repoPage || item.page) {
      const a = linkEl(item.repoPage || item.page, "open page", item.repoPage || item.page);
      if (a) title.appendChild(a);
    }
    body.appendChild(title);
    const meta = rowEl("ap-sr-meta");
    meta.textContent = [
      item.archLabel || item.architecture || "",
      item.downloads ? `${fmtCompact(item.downloads)} downloads` : "",
      item.likes ? `♥${fmtCompact(item.likes)}` : "",
      item.versionName || "",
      item.releasedAt ? String(item.releasedAt).slice(0, 10) : "",
      (item.examples || []).length ? `${item.examples.length} example${item.examples.length > 1 ? "s" : ""}` : "",
    ]
      .filter(Boolean)
      .join(" · ");
    body.appendChild(meta);
    // A search result is a title; the examples are what actually tell you what
    // the model produces, so they are one click away without leaving the list.
    if ((item.examples || []).length > 1) {
      const ex = galleryEl(item, "baked");
      row.appendChild(ex.btn);
      body.appendChild(ex.box);
    }
    row.appendChild(body);
    const btn = rowEl("ap-sr-add", "Add…", "Inspect this model's files and add it");
    btn.addEventListener("click", () => onAdd(item));
    row.appendChild(btn);
    return row;
  }

  function fmtCompact(n) {
    const v = Number(n) || 0;
    if (v >= 1e6) return (v / 1e6).toFixed(1) + "M";
    if (v >= 1e3) return (v / 1e3).toFixed(1) + "K";
    return String(v);
  }

  /** Inspect → pick a file → confirm. The file list is the honest part: a repo
   *  can hold an 8 GB checkpoint and 150 MB adapters side by side. */
  async function openAddFlow(item, opts) {
    opts = opts || {};
    const host = opts.host || $("apSearchResults");
    const status = opts.status || $("apSearchStatus");
    const version =
      opts.version != null ? String(opts.version) : item.versionId != null ? String(item.versionId) : null;
    status.textContent = `Looking inside ${item.title || item.id}…`;
    host.innerHTML = "";
    let info;
    try {
      info = await window.w2gp.adultPackInspect(item.source, item.id, item.architecture, version);
    } catch (e) {
      status.textContent = "Could not read that model: " + (errText(e) || e);
      return null;
    }
    status.textContent = "";
    const files = (info && info.files) || [];
    if (!files.length) {
      status.textContent = "No loadable files found (Wan2GP needs .safetensors or .gguf).";
      return null;
    }
    const box = rowEl("ap-sr-box");
    const head = rowEl("ap-sr-box-head");
    head.appendChild(thumbEl(info.preview, info.title));
    const hb = rowEl("ap-sr-body");
    hb.appendChild(rowEl("ap-sr-title", info.title));
    hb.appendChild(
      rowEl(
        "ap-sr-meta",
        `${files.length} file(s) · architecture ${info.architecture}` +
          (info.architectureKnown === false ? " — not in this Wan2GP build" : ""),
      ),
    );
    head.appendChild(hb);
    box.appendChild(head);
    if (info.source === "civitai") {
      box.appendChild(
        rowEl(
          "ap-hint",
          "Public CivitAI files download without a key. For gated files, store a key in the pack settings and use the pack's own fetch button — Wan2GP's auto-download cannot authenticate.",
        ),
      );
    }
    // A CivitAI model usually has several versions. The search hit pins one; this
    // lets the user switch without going back to the search.
    if (info.source === "civitai" && info.versions && info.versions.length > 1) {
      const vRow = rowEl("ap-fm-field");
      vRow.appendChild(rowEl("ap-detail-k", "Version"));
      const vs = document.createElement("select");
      vs.className = "input";
      for (const v of info.versions) {
        const o = document.createElement("option");
        o.value = String(v.id);
        o.textContent = `${v.name || "version " + v.id}${v.baseModel ? " — " + v.baseModel : ""} (${v.fileCount} file(s))`;
        vs.appendChild(o);
      }
      vs.value = String(info.versionId);
      vs.addEventListener("change", () => openAddFlow(item, { ...opts, version: vs.value, host, status }));
      vRow.appendChild(vs);
      box.appendChild(vRow);
    }
    if (info.samples && info.samples.length) {
      const strip = rowEl("ap-gallery-grid");
      for (const s of info.samples.slice(0, 4)) strip.appendChild(mediaEl(s.url, "video", s.name));
      box.appendChild(strip);
    }

    for (const f of files) {
      const r = rowEl("ap-sr-file");
      // Kind decides whether a recipe is written: a LoRA is filed as an adapter,
      // a checkpoint becomes a finetune recipe. The name+size guess is a
      // starting point, not a verdict, so it is editable.
      const kindSel = document.createElement("select");
      kindSel.className = "input ap-kind-sel";
      kindSel.title = "LoRA = filed as an adapter; Checkpoint = becomes a finetune recipe";
      for (const k of ["lora", "checkpoint"]) {
        const o = document.createElement("option");
        o.value = k;
        o.textContent = k.toUpperCase();
        kindSel.appendChild(o);
      }
      kindSel.value = f.kind === "lora" ? "lora" : "checkpoint";
      r.appendChild(kindSel);
      const nm = rowEl("ap-name");
      nm.textContent = f.file;
      if (f.pair) nm.appendChild(rowEl("ap-tag " + f.pair, f.pair.toUpperCase()));
      if (info.source === "civitai") {
        nm.appendChild(
          rowEl("ap-tag token", "CivitAI", "Used only when the release is gated; public files need no key."),
        );
      }
      r.appendChild(nm);
      r.appendChild(rowEl("ap-size", fmtBytes(f.bytes)));
      const add = rowEl("ap-sr-add", "Add", "Add this file to your own list");
      add.addEventListener("click", async () => {
        add.disabled = true;
        const entry = {
          source: info.source,
          id: info.id,
          title: info.title,
          architecture: info.architecture,
          file: f.file,
          url: f.url,
          sha256: f.sha256,
          bytes: f.bytes,
          kind: kindSel.value,
          preview: info.preview,
          repoPage: info.repoPage,
          filePage: f.url ? f.url.replace("/resolve/main/", "/blob/main/") : "",
          wan2gpIds: [info.architecture],
        };
        try {
          const res = await window.w2gp.adultPackAdd(entry);
          logLine(`✓ added ${f.file} — fetch it with “Fetch missing only” or tick it and download`, "ok");
          // A checkpoint belongs in the finetune selection; a LoRA does not.
          // Ticking the wrong bag made the download button ignore the new row.
          if (res && res.id) {
            if (res.kind === "lora") sel.loras[res.id] = true;
            else sel.fts[res.id] = true;
          }
          await init(true);
        } catch (e) {
          logLine("✗ could not add: " + (errText(e) || e), "err");
          add.disabled = false;
        }
      });
      r.appendChild(add);
      box.appendChild(r);
    }
    host.appendChild(box);
    return info;
  }

  /** Add by link: resolve whatever was pasted, then reuse the same inspect →
   *  pick-a-file flow. The one path that reaches a model whose name matches no
   *  keyword in the curated map. */
  async function addByUrl() {
    const status = $("apUrlStatus");
    const input = $("apUrlInput");
    const results = $("apUrlResults");
    const raw = (input && input.value ? input.value : "").trim();
    if (!raw) {
      if (status) status.textContent = "Paste a repo or URL first.";
      return;
    }
    const arch = $("apUrlArch") && $("apUrlArch").value;
    if (!arch) {
      if (status) status.textContent = "Pick the architecture to load it with first.";
      return;
    }
    if (results) results.innerHTML = "";
    if (status) status.textContent = "Resolving…";
    try {
      const r = await window.w2gp.adultPackResolveUrl(raw);
      const info = await openAddFlow(
        { source: r.source, id: r.id, title: r.id, architecture: arch },
        { host: results, status },
      );
      if (info && status) {
        status.textContent = `Found ${r.source === "hf" ? "repository" : "model"} ${r.id} — pick a file below.`;
      }
    } catch (e) {
      if (status) status.textContent = "✗ " + (errText(e) || e);
    }
  }

  /** Import a shared recipe by path — the Library import, offered where the
   *  recipes actually live. */
  async function importRecipe() {
    const input = $("apFmImportInput");
    const status = $("apFmStatus");
    const src = (input && input.value ? input.value : "").trim();
    if (!src) {
      if (status) status.textContent = "Paste the full path to a .json to import.";
      return;
    }
    try {
      const r = await window.w2gp.libraryFinetuneImport(src);
      logLine(`✓ imported ${(r && (r.id || r.name)) || "recipe"} into finetunes/`, "ok");
      if (input) input.value = "";
      if (status) status.textContent = "";
      await loadFinetunes();
    } catch (e) {
      if (status) status.textContent = "✗ " + (errText(e) || e);
    }
  }

  /** Every LoRA weight under the LoRAs folder, catalog-linked or not. */
  async function loadLoraInventory() {
    const host = $("apLoraInvList");
    if (!host) return;
    const status = $("apLoraInvStatus");
    host.innerHTML = "";
    host.appendChild(rowEl("ap-hint", "Scanning the LoRAs folder…"));
    try {
      const r = await window.w2gp.adultPackLoraInventory();
      host.innerHTML = "";
      if (!r || r.ok === false) {
        host.appendChild(rowEl("ap-hint", (r && r.error) || "Could not read the LoRAs folder."));
        return;
      }
      loraInvLoaded = true;
      if (status) status.textContent = `${r.count} file(s) · ${fmtBytes(r.bytes)} · ${r.root}`;
      if (!r.files.length) {
        host.appendChild(rowEl("ap-hint", "No LoRA weights found under the LoRAs folder."));
        return;
      }
      for (const f of r.files) {
        const row = rowEl("ap-fm-row");
        const main = rowEl("ap-fm-main");
        main.appendChild(rowEl("ap-sr-title", f.name));
        main.appendChild(rowEl("ap-sr-meta", `${fmtBytes(f.bytes)} · ${f.rel}`));
        row.appendChild(main);
        const origin = f.origin || (f.known ? "Pack" : "Yours");
        const packish = origin.startsWith("Pack");
        row.appendChild(
          rowEl(
            "ap-tag " + (packish ? "done" : ""),
            origin,
            packish
              ? "Shipped or fetched by this launcher's pack manager"
              : "Not from the curated pack — keep it or delete it yourself",
          ),
        );
        const del = rowEl("ap-sr-add", "Delete", "Remove this file from disk");
        del.addEventListener("click", async () => {
          if (!(await askConfirm(`Delete ${f.rel} from disk?\n\nThis removes the weights, not just a catalog row.`, "Delete LoRA file"))) return;
          try {
            await window.w2gp.adultPackLoraDelete(f.rel);
            logLine(`✓ deleted ${f.rel}`, "ok");
            loadLoraInventory();
          } catch (e) {
            if (status) status.textContent = "✗ " + (errText(e) || e);
          }
        });
        row.appendChild(del);
        host.appendChild(row);
      }
    } catch (e) {
      host.innerHTML = "";
      host.appendChild(rowEl("ap-hint", "Could not read the LoRAs folder: " + (errText(e) || e)));
    }
  }

  async function runSearch() {
    const q = ($("apSearchQuery").value || "").trim();
    const status = $("apSearchStatus");
    const host = $("apSearchResults");
    if (q.length < 2) {
      status.textContent = "Type at least two characters.";
      return;
    }
    host.innerHTML = "";
    status.textContent = "Searching…";
    try {
      const r = await window.w2gp.adultPackSearch(
        $("apSearchSource").value,
        q,
        $("apSearchKind").value,
        $("apSearchFamily").value,
        24,
      );
      const items = (r && r.items) || [];
      // selectedIndex is -1 when a value no longer exists (e.g. a family the
      // manifest no longer lists) — reading options[-1] would throw and turn a
      // filter mismatch into a crashed search.
      const labelOf = (id) => {
        const sel = $(id);
        if (!sel) return "";
        const i = sel.selectedIndex;
        return i >= 0 ? sel.options[i].text : sel.options[0] ? sel.options[0].text : "";
      };
      const kindLabel = labelOf("apSearchKind");
      const famLabel = labelOf("apSearchFamily");
      status.textContent = items.length
        ? `${items.length} match(es) — ${kindLabel}, ${famLabel} — this Wan2GP build can load them`
        : `Nothing matching “${kindLabel}” + “${famLabel}” that this Wan2GP build can load. Different family, or not published yet.`;
      for (const it of items) host.appendChild(searchRow(it, openAddFlow));
    } catch (e) {
      status.textContent = "Search failed: " + (errText(e) || e);
    }
  }

  /** The base-model list comes from the manifest's archMap, so a newly
   *  supported family shows up here without touching this file. */
  function fillFamilyFilter() {
    fillFamilyFilters();
  }

  // ── init ─────────────────────────────────────────────────────────────────
  /// The CivitAI host is never contacted while the switch is off, so the
  /// section and both source pickers follow the saved setting.
  function applyCivitaiVisibility() {
    const on = civOn();
    const cb = $("apCivEnabled");
    if (cb) cb.checked = on;
    const section = $("apCivSection");
    if (section) section.style.display = on ? "" : "none";
    for (const id of ["apSearchSource", "apBrowseSource"]) {
      const s = $(id);
      if (!s) continue;
      const opt = s.querySelector('option[value="civitai"]');
      if (opt) opt.disabled = !on;
      if (!on && s.value === "civitai") s.value = "hf";
    }
  }

  async function init(keepSelection) {
    const root = document.querySelector('.settings-tab-content[data-tab="pack"]');
    if (!root) return;
    // A batch from an earlier visit is not this visit's news; a refresh right
    // after a download is, which is why a kept selection keeps the queue.
    if (!keepSelection) {
      queue = null;
      renderQueue();
    }
    try {
      pack = await window.w2gp.adultPackManifest();
    } catch (e) {
      root.querySelector(".token-hint") &&
        (root.querySelector(".token-hint").textContent = "Could not read the pack catalog: " + errText(e));
      return;
    }
    if (!pack || !pack.loras || !pack.loras.length) {
      setProgress("This build carries no pack catalog.");
      return;
    }
    // The selectable architectures are independent of the catalog, so load them
    // before anything can need them.
    try {
      const ar = await window.w2gp.adultPackArches();
      arches = (ar && ar.items) || [];
    } catch {
      arches = [];
    }
    if (pack.hasToken && $("apToken")) $("apToken").placeholder = "CivitAI token stored — type to replace it";
    applyCivitaiVisibility();

    if (!keepSelection && !sel.hydrated) {
      const saved = pack.savedSelections || {};
      const pick = (list, bag, prefKey) => {
        const out = {};
        for (const x of list || []) {
          const was = saved[bag] ? saved[bag][x.id] : undefined;
          out[x.id] = was === undefined ? !!x.defaultSelected : !!was;
          if (prefKey) out[prefKey + x.id] = !!x.prefetchDefault && out[x.id];
        }
        return out;
      };
      sel = {
        hydrated: true,
        loras: pick(pack.loras, "loras"),
        fts: pick(pack.finetunes, "finetunes", "pf:"),
        // Token tier never starts selected: it needs consent and costs a fetch.
        civ: (saved.civitai && typeof saved.civitai === "object") ? { ...saved.civitai } : {},
      };
      if (saved.enabled) $("apEnabled").checked = true;
      if (saved.ageConfirmed || pack.ageConfirmedAt) $("apAge").checked = true;
    }

    if (!wired) {
      wired = true;
      $("apEnabled").addEventListener("change", () => {
        render();
        persist();
      });
      $("apAge").addEventListener("change", () => {
        render();
        persist();
      });
      $("apToken").addEventListener("change", () => {
        render();
        persist();
      });
      $("apCivEnabled").addEventListener("change", async () => {
        const want = $("apCivEnabled").checked;
        try {
          await window.w2gp.adultPackSetCivitai(want);
          pack.civitaiEnabled = want;
          applyCivitaiVisibility();
          setProgress(
            want
              ? "CivitAI on — requests will go to civitai.red."
              : "CivitAI off — nothing contacts civitai.red.",
          );
        } catch (e) {
          $("apCivEnabled").checked = !want;
          logLine(`✗ CivitAI setting: ${errText(e)}`, "err");
        }
      });
      $("apVerifyBtn").addEventListener("click", verifyToken);
      $("apDownloadBtn").addEventListener("click", async () => {
        const p = plan();
        // A new batch: drop the previous run's rows so the CivitAI downloads
        // that run first cannot appear against them.
        queue = null;
        renderQueue();
        await doCivitai();
        await doFetch([...p.loraIds, ...p.prefetchIds], "Downloading selected");
      });
      $("apRecipesBtn").addEventListener("click", doRecipes);
      $("apMissingBtn").addEventListener("click", doMissing);
      $("apRemoveBtn").addEventListener("click", doRemove);
      $("apRefreshBtn").addEventListener("click", () => init(true));
      $("apFilter").addEventListener("input", () => {
        filter.q = $("apFilter").value.trim().toLowerCase();
        render();
      });
      $("apFilterSel").addEventListener("change", () => {
        filter.selected = $("apFilterSel").checked;
        render();
      });
      $("apFilterMissing").addEventListener("change", () => {
        filter.missing = $("apFilterMissing").checked;
        render();
      });
      $("apFilterSelect").addEventListener("click", () => selectShown(true));
      $("apFilterUnselect").addEventListener("click", () => selectShown(false));
      $("apFilterClear").addEventListener("click", clearCatalogFilter);
      $("apFmNewBtn").addEventListener("click", openNewFinetuneEditor);
      $("apSearchBtn").addEventListener("click", runSearch);
      $("apFmRefreshBtn").addEventListener("click", loadFinetunes);
      $("apFmCheckAllBtn").addEventListener("click", checkAllFinetuneLinks);
      $("apFmImportBtn").addEventListener("click", importRecipe);
      // API-key help links. The account page is the one that shows the key on
      // both hosts; opening it externally keeps the launcher webview on the app.
      for (const b of document.querySelectorAll("[data-civ-key]")) {
        b.addEventListener("click", () => {
          const host = b.getAttribute("data-civ-key") === "red" ? "civitai.red" : "civitai.com";
          const url = `https://${host}/user/account`;
          try {
            Promise.resolve(window.w2gp.openExternal(url)).catch((e) =>
              logLine(`✗ Could not open ${host}: ${errText(e)}`, "err"),
            );
          } catch (_) {
            window.open(url, "_blank", "noopener");
          }
        });
      }
      $("apLoraInvRefreshBtn").addEventListener("click", loadLoraInventory);
      $("apUrlBtn").addEventListener("click", addByUrl);
      $("apUrlInput").addEventListener("keydown", (e) => {
        if (e.key === "Enter") addByUrl();
      });
      $("apBrowseBtn").addEventListener("click", runBrowse);
      $("apUpdCheckBtn").addEventListener("click", () => withBusy("Checking…", runUpdateCheck));
      $("apUpdApplyBtn").addEventListener("click", applyUpdates);
      $("apSearchQuery").addEventListener("keydown", (e) => {
        if (e.key === "Enter") runSearch();
      });
    }

    if (progressUnsub) {
      try {
        progressUnsub();
      } catch {}
    }
    progressUnsub = window.w2gp.onAdultPackProgress((ev) => {
      if (!ev || !ev.name) return;
      updateQueue(ev);
      if (ev.phase === "start") return; // the queue row says it all; no bytes yet
      if (ev.phase === "check") {
        setProgress(`Checking ${ev.name}… (${ev.done}/${ev.total})`);
        return;
      }
      const pct = ev.total ? Math.floor((ev.received / ev.total) * 100) : 0;
      const pos =
        ev.queueIndex != null && ev.queueTotal ? `[${ev.queueIndex}/${ev.queueTotal}] ` : "";
      setProgress(
        ev.phase === "verify"
          ? `${pos}verifying ${ev.name}…`
          : `${pos}↓ ${ev.name} — ${fmtBytes(ev.received)}${ev.total ? ` / ${fmtBytes(ev.total)} (${pct}%)` : ""}`,
      );
    });

    render();
    fillFamilyFilter();
    fillArchSelect();
    loadFinetunes();
    if (!loraInvLoaded) loadLoraInventory();
  }

  // Fill on demand: the tab is opened from Manage, so it must not cost anything
  // at boot.
  window.adultPack = { init, plan, render };
  window.adultPackRefresh = () => init(true);
  window.adultPackReset = () => {
    sel = { loras: {}, fts: {}, civ: {} };
    init(true);
  };
})();
// adult-pack-tab.js — Manage → 📦 Model Pack: the opt-in Adult (NSFW) model
// pack (installed on demand, never part of the Wan2GP install).
//
// Every row comes from src/adult-pack-manifest.json via the adult_pack_manifest
// command — model names, sizes and URLs are data, not code, so adding or
// dropping a model is a manifest edit (scripts/verify-adult-pack.mjs) with no
// change here.
//
// Two tiers, because they behave differently:
//   * LoRAs are downloaded by the launcher into loras/adult-pack/<family>/,
//     resumable and SHA-256 verified.
//   * Checkpoints are recipes: we write finetunes/adultpack_*.json with remote
//     URLs and Wan2GP fetches the weights when you actually run that model.
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

  const enabled = () => !!(pack && $("apEnabled") && $("apEnabled").checked);
  const ageOk = () => !!(pack && $("apAge") && $("apAge").checked);
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
          "Verify & fetch now",
          "Resolve the current version with your CivitAI token, download it and record a local recipe",
          () =>
            done(() =>
              withBusy("Fetching from CivitAI…", async () => {
                const token = ($("apToken").value || "").trim();
                if (!token) {
                  logLine("Paste a CivitAI token first.", "err");
                  return;
                }
                try {
                  const r = await window.w2gp.adultPackCivitaiDownload(item.id, token);
                  logLine(`✓ ${item.name} → ${(r && r.finetune) || "downloaded"}`, "ok");
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
            : "Prefetch this checkpoint's weights to the ckpts folder",
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
    if (page) bar.appendChild(actionBtn("Open source page", "Open the model on HuggingFace / CivitAI", () => window.w2gp.openExternal(page)));
    bar.appendChild(
      actionBtn("Delete…", "Delete this item's downloaded files (and recipe)", () =>
        done(() =>
          withBusy("Removing…", async () => {
            const what = isLora
              ? "its downloaded file"
              : isCiv
              ? "its local recipe"
              : "its prefetched weights and its finetune recipe";
            if (!window.confirm(`Delete ${item.name} — ${what}?\n\nFiles you added yourself are never touched.`)) return;
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
      return;
    }
    if (body) body.style.display = "";

    const host = $("apGroups");
    host.innerHTML = "";

    for (const g of pack.groups || []) {
      const items = (pack.loras || []).filter((l) => l.group === g.id);
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

    const fts = (pack.finetunes || []).filter((f) => f.architectureAvailable !== false);
    const ftBlock = rowEl("ap-block");
    const ftHead = rowEl("ap-block-head");
    ftHead.appendChild(rowEl("ap-title-sm", "Checkpoint finetunes"));
    ftHead.appendChild(rowEl("ap-hint", "recipe now — weights only when you run the model, or prefetch"));
    ftBlock.appendChild(ftHead);
    for (const f of fts) {
      const row = rowEl("ap-row");
      row.appendChild(thumbOf(f));
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
      const gal = galleryEl(f, "hf");
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
    for (const c of pack.civitai || []) {
      const row = rowEl("ap-row");
      row.appendChild(thumbEl(c.preview, c.name));
      const cb = checkBox(!!sel.civ[c.id], (on) => {
        sel.civ[c.id] = on;
        render();
        persist();
      }, c.licenseNote || c.description || "");
      cb.input.disabled = !(pack.hasToken || ($("apToken").value || "").trim());
      row.appendChild(cb.label);
      const nm = rowEl("ap-name");
      nm.textContent = c.name;
      nm.appendChild(rowEl("ap-tag token", "token"));
      row.appendChild(nm);
      row.appendChild(rowEl("ap-size", fmtBytes(c.bytesEstimate)));
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
    updateTotals();
  }

  // ── plan + totals ────────────────────────────────────────────────────────
  function plan() {
    const loraIds = Object.keys(sel.loras).filter((k) => sel.loras[k]);
    const ftIds = Object.keys(sel.fts).filter((k) => !k.startsWith("pf:") && sel.fts[k]);
    const prefetchIds = ftIds.filter((k) => sel.fts["pf:" + k]);
    const civIds = Object.keys(sel.civ).filter((k) => sel.civ[k]);
    const lm = new Map((pack.loras || []).map((x) => [x.id, x]));
    const fm = new Map((pack.finetunes || []).map((x) => [x.id, x]));
    const cm = new Map((pack.civitai || []).map((x) => [x.id, x]));
    const ftCost = (id) => {
      const f = fm.get(id);
      return f ? (f.bytes || 0) + (f.bytesLow || 0) : 0;
    };
    return {
      active: active(),
      loraIds,
      ftIds,
      prefetchIds,
      civIds,
      nowBytes:
        loraIds.reduce((a, id) => a + (lm.get(id)?.bytes || 0), 0) +
        prefetchIds.reduce((a, id) => a + ftCost(id), 0) +
        civIds.reduce((a, id) => a + (cm.get(id)?.bytesEstimate || 0), 0),
      count: loraIds.length + ftIds.length + civIds.length,
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
      const add = (label, value, cls, title) => {
        const c = rowEl("ap-chip " + (cls || ""), `${value} ${label}`, title || "");
        chips.appendChild(c);
      };
      add("downloaded", `${got}/${loras.length}`, got ? "ok" : "", "LoRA files on disk matching the verified size");
      if (stale) add("need update", String(stale), "warn", "Downloaded, but upstream has newer bytes");
      if (pinned) add("updated pins", String(pinned), "warn", "Local hash newer than the shipped catalog");
      add("recipes written", `${recipes}/${fts.length}`, recipes ? "ok" : "", "finetunes/*.json in place");
      add("weights on disk", `${weights}/${fts.length}`, weights ? "ok" : "", "Checkpoint files already fetched");
    }
    if (!p.active) {
      t.textContent = "Adult pack off";
      const f = $("apFreeText");
      if (f) f.textContent = "";
      return;
    }
    t.textContent =
      `${p.count} items selected · ${fmtBytes(p.nowBytes)} to download` +
      (p.ftIds.length
        ? ` · ${p.ftIds.length} checkpoint${p.ftIds.length > 1 ? "s" : ""} as recipes (fetched only when you run ${p.ftIds.length > 1 ? "them" : "it"})`
        : "");
    const f = $("apFreeText");
    if (f && pack.paths && pack.paths.loras) {
      window.w2gp
        .getDiskSpace(pack.paths.loras)
        .then((d) => {
          if (!d || typeof d.free !== "number") return;
          const left = d.free - p.nowBytes;
          f.textContent =
            `${fmtBytes(d.free)} free — ` +
            (left < 0 ? "⚠ not enough space for this selection" : `${fmtBytes(left)} would remain`);
        })
        .catch(() => {});
    }
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
      status.textContent = `${r.reposChecked} repo(s) checked · ${changed.length} file(s) changed upstream · ${stale.length} downloaded file(s) out of date`;
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
      await doFetch(ids, "Updating");
      const st = $("apUpdStatus");
      if (st) st.textContent = `${(applied.applied || []).length} hash pin(s) updated, ${(applied.skipped || []).length} skipped`;
      await runUpdateCheck();
    });
  }

  async function doFetch(ids, label) {
    if (!ids.length) {
      logLine("Nothing selected to download.");
      return;
    }
    await withBusy(label + "…", async () => {
      const r = await window.w2gp.adultPackFetch(ids);
      const got = (r && r.done && r.done.length) || 0;
      logLine(`✓ ${got}/${ids.length} file(s) downloaded — ${fmtBytes((r && r.bytes) || 0)}`, "ok");
      for (const f of (r && r.failed) || []) logLine(`✗ ${f.name || f.id}: ${f.error}`, "err");
      setProgress("Download finished.");
      await init(true);
    });
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
          logLine(
            `✓ ${(r && r.files ? r.files.length : 0)} file(s) — ${(r && r.versionName) || id} → ${(r && r.finetune) || ""}`,
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
    const missing = p.loraIds.filter((id) => {
      const l = (pack.loras || []).find((x) => x.id === id);
      return l && !l.installed;
    });
    if (!missing.length) {
      logLine("Everything selected is already on disk (and hash-matching).");
      return;
    }
    await doFetch(missing, "Downloading missing files");
  }

  async function doRemove() {
    const p = plan();
    const ids = [...p.loraIds, ...p.ftIds, ...p.civIds];
    if (!ids.length) {
      logLine("Nothing selected.");
      return;
    }
    if (
      !window.confirm(
        `Remove ${ids.length} selected pack item(s) from disk?\n\n` +
          "LoRA files are deleted, and a selected checkpoint's prefetched weights plus its " +
          "finetune recipe are deleted. Files you added yourself are never touched.",
      )
    )
      return;
    await withBusy("Removing…", async () => {
      const r = await window.w2gp.adultPackRemove(ids);
      const n = ((r && r.removed) || []).length;
      logLine(`✓ removed ${n} item(s)`, "ok");
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

  function fmRow(ft, host) {
    const row = rowEl("ap-fm-row");
    const main = rowEl("ap-fm-main");
    main.appendChild(rowEl("ap-sr-title", ft.name || ft.id));
    const bits = [
      ft.architecture || "—",
      `${ft.urlCount || 0} source(s)`,
      ft.origin === "pack" ? "from pack" : ft.origin === "custom" ? "yours" : "local",
      ft.written ? "in finetunes/" : "not written",
    ];
    if (ft.hasLocalWeights) bits.push(`weights on disk (${fmtBytes(ft.localBytes || 0)})`);
    if (ft.needsToken) bits.push("needs token");
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
      box.appendChild(fmEditorActions(ft, ta, box));
      return;
    }
    box.innerHTML = "";
    const ta = document.createElement("textarea");
    ta.className = "ap-fm-text";
    ta.value = content;
    box.appendChild(ta);
    box.appendChild(fmEditorActions(ft, ta, box));
  }

  function fmEditorActions(ft, ta, box) {
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

    const del = rowEl("ap-sr-add", "Delete recipe", "Remove this JSON from finetunes/ (weights stay)");
    del.addEventListener("click", async () => {
      if (!window.confirm(`Delete ${ft.id}.json from finetunes/?\n\nDownloaded weights are left on disk.`)) return;
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

  async function loadFinetunes() {
    const host = $("apFmList");
    if (!host) return;
    try {
      const r = await window.w2gp.adultPackFinetuneList();
      fmItems = (r && r.items) || [];
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
  async function openAddFlow(item) {
    const host = $("apSearchResults");
    const status = $("apSearchStatus");
    status.textContent = `Looking inside ${item.title || item.id}…`;
    host.innerHTML = "";
    let info;
    try {
      info = await window.w2gp.adultPackInspect(item.source, item.id, item.architecture);
    } catch (e) {
      status.textContent = "Could not read that model: " + (errText(e) || e);
      return;
    }
    status.textContent = "";
    const files = (info && info.files) || [];
    if (!files.length) {
      status.textContent = "No loadable files found (Wan2GP needs .safetensors or .gguf).";
      return;
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
    if (info.samples && info.samples.length) {
      const strip = rowEl("ap-gallery-grid");
      for (const s of info.samples.slice(0, 4)) strip.appendChild(mediaEl(s.url, "video", s.name));
      box.appendChild(strip);
    }

    const familyOf = (arch) =>
      arch.startsWith("minimax") ? "h3" : arch.startsWith("ltx") ? "ltx" : arch.startsWith("z_image") || arch.startsWith("krea") || arch.startsWith("qwen") || arch.startsWith("flux") || arch.startsWith("moss") ? "image" : "wan";

    for (const f of files) {
      const r = rowEl("ap-sr-file");
      const kind = rowEl("ap-tag " + (f.kind === "lora" ? "high" : "done"), f.kind.toUpperCase());
      r.appendChild(kind);
      const nm = rowEl("ap-name");
      nm.textContent = f.file;
      if (f.pair) nm.appendChild(rowEl("ap-tag " + f.pair, f.pair.toUpperCase()));
      if (f.needsToken) nm.appendChild(rowEl("ap-tag token", "token"));
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
          family: familyOf(info.architecture),
          file: f.file,
          url: f.url,
          sha256: f.sha256,
          bytes: f.bytes,
          kind: f.kind,
          preview: info.preview,
          repoPage: info.repoPage,
          filePage: f.url ? f.url.replace("/resolve/main/", "/blob/main/") : "",
          wan2gpIds: [info.architecture],
        };
        try {
          const res = await window.w2gp.adultPackAdd(entry);
          logLine(`✓ added ${f.file} — fetch it with “Fetch missing only” or tick it and download`, "ok");
          if (res && res.id) sel.loras[res.id] = true;
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
  async function init(keepSelection) {
    const root = document.querySelector('.settings-tab-content[data-tab="pack"]');
    if (!root) return;
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
    if (pack.hasToken && $("apToken")) $("apToken").placeholder = "CivitAI token stored — type to replace it";

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
      $("apToken").addEventListener("change", render);
      $("apVerifyBtn").addEventListener("click", verifyToken);
      $("apDownloadBtn").addEventListener("click", async () => {
        const p = plan();
        await doCivitai();
        await doFetch([...p.loraIds, ...p.prefetchIds], "Downloading selected");
      });
      $("apRecipesBtn").addEventListener("click", doRecipes);
      $("apMissingBtn").addEventListener("click", doMissing);
      $("apRemoveBtn").addEventListener("click", doRemove);
      $("apRefreshBtn").addEventListener("click", () => init(true));
      $("apSearchBtn").addEventListener("click", runSearch);
      $("apFmRefreshBtn").addEventListener("click", loadFinetunes);
      $("apFmCheckAllBtn").addEventListener("click", checkAllFinetuneLinks);
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
      const pct = ev.total ? Math.floor((ev.received / ev.total) * 100) : 0;
      setProgress(
        ev.phase === "verify"
          ? `verifying ${ev.name}…`
          : `↓ ${ev.name} — ${fmtBytes(ev.received)}${ev.total ? ` / ${fmtBytes(ev.total)} (${pct}%)` : ""}`,
      );
    });

    render();
    fillFamilyFilter();
    loadFinetunes();
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
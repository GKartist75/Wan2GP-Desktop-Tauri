#!/usr/bin/env node
// Adult (NSFW) model pack — manifest generator + verifier.
//
//   node scripts/verify-adult-pack.mjs            # resolve everything, write src/adult-pack-manifest.json
//   node scripts/verify-adult-pack.mjs --report   # resolve + print a report, write nothing
//   node scripts/verify-adult-pack.mjs --only=wan_set,h3_aio
//   node scripts/verify-adult-pack.mjs --deep     # also HEAD each resolved URL (slow)
//
// The sources file (scripts/adult-pack-sources.json) never contains hashes or
// sizes — it says WHICH repos/files belong in the pack. This script resolves
// each one against the HuggingFace tree API (which reports the real byte size
// and the LFS sha256 oid) and writes the pinned manifest the launcher reads.
//
// Exit codes: 0 = every required entry resolved, 1 = at least one failed.
import { readFileSync, writeFileSync, mkdirSync, statSync, renameSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..");
const SOURCES = join(HERE, "adult-pack-sources.json");
const MANIFEST = join(ROOT, "src", "adult-pack-manifest.json");

const argv = process.argv.slice(2);
const REPORT_ONLY = argv.includes("--report");
const DEEP = argv.includes("--deep");
const FORCE = argv.includes("--refresh");
const EXAMPLES = !argv.includes("--no-examples");
const MATCH_DEBUG = argv.includes("--match-debug");
const ONLY = (argv.find((a) => a.startsWith("--only=")) || "").split("=")[1] || "";
const onlySet = ONLY ? new Set(ONLY.split(",").map((s) => s.trim()).filter(Boolean)) : null;

const HF = "https://huggingface.co";
const UA = { "User-Agent": "wan2gp-launcher-adult-pack-verifier" };

// ── helpers ────────────────────────────────────────────────────────────────

/** HF tree paths can legitimately contain literal "%20" and raw UTF-8.
 *  encodeURIComponent would double-encode the "%" of a real "%20", so encode
 *  byte-wise and pass through any existing percent-escape verbatim. */
function encodeHfPath(p) {
  return p
    .split("/")
    .map((seg) =>
      seg
        .split("")
        .map((ch) => {
          if (ch === "%") return "%25";
          const c = ch.codePointAt(0);
          if (c < 0x80 && /[A-Za-z0-9\-_.!~*'()]/.test(ch)) return ch;
          return [...new TextEncoder().encode(ch)]
            .map((b) => "%" + b.toString(16).toUpperCase().padStart(2, "0"))
            .join("");
        })
        .join(""),
    )
    .join("/");
}

function resolveUrl(repo, path) {
  return `${HF}/${repo}/resolve/main/${encodeHfPath(path)}`;
}

/** Wan 2.2 HIGH/LOW halves are load-bearing: both files of a pair must ship
 *  together, so detect the half from the filename. Covers HIGH/high_noise/_H
 *  and LOW/low_noise/_L (also the NSFW-22-H-e8 / -L-e8 spelling). */
function detectPair(path) {
  const n = path.split("/").pop();
  if (/(^|[^a-z])high([^a-z]|$)/i.test(n) || /_hn?\b/i.test(n) || /[-_]H([-_]|$)/.test(n)) return "high";
  if (/(^|[^a-z])low([^a-z]|$)/i.test(n) || /_ln?\b/i.test(n) || /[-_]L([-_]|$)/.test(n)) return "low";
  return null;
}

/** Strip extension + noisy tokens for a readable UI label.
 *  Only the underscore form (`_v1.8`, `_v1_2`) is dropped — a dashed version
 *  (`rank64-v1.2`) is what actually tells two builds apart, and stripping it
 *  produced three identically named rows in the panel. */
function titleFromPath(path) {
  const base = path.split("/").pop().replace(/\.(safetensors|gguf|pt|bin|pth)$/i, "");
  return base
    .replace(/_v\d+(\.\d+)?((_|-)alpha|_a\b)$/i, "")
    .replace(/[_-]+/g, " ")
    .replace(/\s+/g, " ")
    .trim()
    .replace(/\b(\w)/g, (m) => m.toUpperCase());
}

function slugFromPath(path) {
  const base = path.split("/").pop().replace(/\.(safetensors|gguf|pt|bin|pth)$/i, "");
  return base
    .normalize("NFKD")
    .replace(/[^\w\s.-]/g, "")
    .trim()
    .replace(/[^a-zA-Z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .toLowerCase()
    .slice(0, 72);
}

/** Identity of a HIGH/LOW pair: the filename with the half marker blanked out,
 *  so `X_HIGH`/`X_LOW`, `X_high_noise`/`X_low_noise` and `X-H-e8`/`X-L-e8`
 *  collapse onto the same stem while unrelated sets stay separate. */
function pairStem(path) {
  const n = path
    .split("/")
    .pop()
    .replace(/\.(safetensors|gguf)$/i, "")
    .toLowerCase();
  return n
    .replace(/high(_?noise)?|low(_?noise)?/g, "~")
    .replace(/[-_]h([-_]|$)/g, "-~-")
    .replace(/[-_]l([-_]|$)/g, "-~-")
    .replace(/[^a-z0-9~]+/g, "");
}

// Order matters: a "v1.0" always beats a "beta"/"alpha" token, because file
// names like "..._alpha128_v1.0_bf16" train both and the real version is the
// v-token. Dimension markers (_e320, _e496) are deliberately NOT versions.
const VERSION_RES = [
  /(?:^|[._-])((?:v|ver|version)[-_.]?[0-9]+(?:[._-][0-9]+)*)/i,
  /(?:^|[._-])(beta[-_.]?[0-9]+(?:[._-][0-9]+)?)/i,
  /(?:^|[._-])(alpha[-_.]?[0-9]+(?:[._-][0-9]+)?)/i,
];

/** Human-facing version for an entry: an explicit token in the file name
 *  (v1.8, beta-0.6), else the repo's last-modified date so a row can still
 *  answer "what am I looking at". */
function versionToken(file, meta) {
  const base = String(file || "").split("/").pop().replace(/\.[a-z0-9]+$/i, "");
  for (const re of VERSION_RES) {
    const named = base.match(re);
    if (named) return named[1].replace(/[._-]/g, ".");
  }
  const date = meta && meta.lastModified ? String(meta.lastModified).slice(0, 10) : null;
  return date || null;
}

const cache = new Map();
const CACHE_DIR = join(HERE, ".adult-pack-cache");
const CACHE_TTL_MS = 12 * 60 * 60 * 1000; // 12h — enough to keep a CI run polite

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function cachePath(repo) {
  return join(CACHE_DIR, repo.replace(/[\\/]/g, "__") + ".json");
}

function readCache(repo) {
  try {
    const p = cachePath(repo);
    const st = statSync(p);
    if (!FORCE && Date.now() - st.mtimeMs > CACHE_TTL_MS) return null;
    return JSON.parse(readFileSync(p, "utf8"));
  } catch {
    return null;
  }
}

function writeCache(repo, files) {
  try {
    mkdirSync(CACHE_DIR, { recursive: true });
    writeFileSync(cachePath(repo), JSON.stringify(files));
  } catch {
    /* cache is an optimisation, never a hard failure */
  }
}

/** HF throttles unauthenticated bursts with 429 — back off instead of failing
 *  the whole catalog on one unlucky request. */
async function fetchJson(url, tries = 4) {
  for (let i = 0; i < tries; i++) {
    const r = await fetch(url, { headers: UA });
    if (r.status === 429 || r.status >= 500) {
      if (i < tries - 1) await sleep(1200 * 2 ** i);
      continue;
    }
    if (!r.ok) return { error: `HTTP ${r.status} ${url}` };
    try {
      return { json: await r.json() };
    } catch (e) {
      return { error: `bad JSON from ${url}: ${e.message}` };
    }
  }
  return { error: `HTTP 429 (rate limited) after ${tries} tries — ${url}` };
}

async function tree(repo) {
  if (cache.has(repo)) return cache.get(repo);
  const hit = readCache(repo);
  if (hit) {
    const out = { files: hit };
    cache.set(repo, out);
    return out;
  }
  const r = await fetchJson(`${HF}/api/models/${repo}/tree/main?recursive=true`);
  if (r.error) {
    const out = { error: repo.includes("/") ? r.error : r.error };
    cache.set(repo, out);
    return out;
  }
  const files = (r.json || []).filter((f) => f.type === "file");
  writeCache(repo, files);
  const out = { files };
  cache.set(repo, out);
  return out;
}

const metaCache = new Map();
/** One request per repo returns both the commit and the card preview; the
 *  result is cached alongside the listing so a re-run costs nothing. */
async function repoMeta(repo) {
  if (metaCache.has(repo)) return metaCache.get(repo);
  const hit = readCache(repo + "#meta");
  if (hit && !FORCE) {
    metaCache.set(repo, hit);
    return hit;
  }
  let out = null;
  try {
    const r = await fetchJson(HF + "/api/models/" + repo);
    if (!r.error && r.json) {
      const j = r.json;
      const c = j.cardData || {};
      const w = Array.isArray(c.widget) ? c.widget[0] : null;
      const direct = c.image_url || c.cover_image_url || (w && w.output && w.output.url);
      out = {
        sha: j.sha || null,
        lastModified: j.lastModified || null,
        // A HF repo has no per-file publish date; createdAt is the honest
        // fallback for "when did this model first appear".
        createdAt: j.createdAt || null,
        downloads: j.downloads ?? null,
        cardPreview: typeof direct === "string" ? direct : null,
      };
      writeCache(repo + "#meta", out);
    }
  } catch {
    out = null;
  }
  metaCache.set(repo, out);
  return out;
}

const PREVIEW_EXT = new RegExp("\\.(png|jpe?g|webp)$", "i");
const PREVIEW_HINT = new RegExp("(preview|image|thumbnail|sample|card|cover|thumb)", "i");

/** One preview image per repo, pinned to the verified commit so the picture can
 *  never drift away from the bytes we hashed. Prefers the model card's own
 *  widget preview, then a small obviously-named image in the tree. Video models
 *  often only ship .mp4 samples — those are skipped and the row simply renders
 *  without a thumbnail rather than showing a broken element.
 *
 *  Card previews are frequently *lies*: a model card can reference
 *  `images/undefined_image (1).png` that was never committed, which renders as a
 *  broken tile. Anything the tree does not actually contain is ignored. */
function pickPreviewPath(files, cardUrl) {
  const known = new Set(files.map((f) => f.path));
  if (cardUrl && PREVIEW_EXT.test(cardUrl.split("?")[0])) {
    const i = cardUrl.indexOf("/resolve/main/");
    if (i >= 0) {
      // The card URL is already percent-encoded; decode it back to the raw repo
      // path, otherwise encodeHfPath would turn "%20" into "%2520".
      try {
        const raw = decodeURIComponent(cardUrl.slice(i + "/resolve/main/".length));
        if (known.has(raw)) return raw;
      } catch {
        return null;
      }
    }
  }
  const images = files.filter(
    (f) => PREVIEW_EXT.test(f.path) && !f.path.startsWith(".") && !f.path.includes("undefined"),
  );
  if (!images.length) return null;
  const hinted = images.filter((f) => PREVIEW_HINT.test(f.path));
  const pool = hinted.length ? hinted : images;
  // Smallest wins: a thumbnail beats a multi-megabyte promo render.
  return pool.slice().sort((a, b) => (a.size || 0) - (b.size || 0))[0].path;
}

// Wan2GP decides how to load a checkpoint from the NAME, not from the file:
// the docs are explicit that a non-quant name carries bf16/fp16 and a
// quantized one carries quanto_bf16_int8 / quanto_fp16_int8 (lowercase
// matters), with GGUF levels written as gguf_q4_k_m etc. A published int8
// ConvRot file with no token is treated as bf16 and fails to load, so the
// token is derived from the file itself and appended when missing.
const GGUF_LEVELS = [
  "q2_k", "q2_k_s", "q3_k_m", "q3_k_s", "q4_0", "q4_1", "q4_k", "q4_k_m", "q4_k_s",
  "q5_0", "q5_k", "q5_k_m", "q5_k_s", "q6_k", "q8_0", "iq1_s", "iq2_xxs", "iq3_s",
  "iq4_xs", "iq4_nl",
];

function ggufLevel(text) {
  const n = String(text).toLowerCase();
  return GGUF_LEVELS.slice().sort((a, b) => b.length - a.length).find((l) => n.includes(l)) || null;
}

function precisionToken(file, quant) {
  const n = String(file || "").toLowerCase();
  const q = String(quant || "").toLowerCase();
  if (/\.gguf$/.test(n) || q === "gguf" || /^q\d/.test(q)) {
    // Match a KNOWN level against the file first, then the declared quant. A
    // greedy /q\d+_[a-z0-9_]+/ once produced "gguf_q5_k_m_v18_ltx23" from a
    // filename that merely had the version glued on.
    const level = ggufLevel(n) || ggufLevel(q.replace(/^gguf_?/, ""));
    return "gguf_" + (level || "gguf");
  }
  if (/(int8|convrot|w4a8|quanto|int4|nf4)/.test(n + " " + q)) return "quanto_bf16_int8";
  if (/fp8/.test(n + " " + q)) return "fp8";
  if (/bf16/.test(n + " " + q)) return "bf16";
  if (/fp16|fp32/.test(n + " " + q)) return "fp16";
  return null;
}

function wan2gpName(name, file, quant) {
  const token = precisionToken(file, quant);
  if (!token) return { name, token: null };
  const lower = name.toLowerCase();
  // Already compliant? Leave the author's wording alone: a name that already
  // says Q4_K_M / int8 / fp8 does not need the token repeated.
  if (lower.includes(token)) return { name, token };
  if (token.startsWith("gguf_") && lower.includes(token.slice(5))) return { name, token };
  return { name: name + " - " + token, token };
}

/** Cross-check the resolved entry against the live CDN (size header). */
async function deepCheck(repo, path, size) {
  try {
    const r = await fetch(resolveUrl(repo, path), { method: "HEAD", headers: UA, redirect: "follow" });
    if (!r.ok) return { ok: false, note: `HEAD ${r.status}` };
    const cdl = Number(r.headers.get("content-length") || 0);
    if (size && cdl && cdl !== size) return { ok: false, note: `size drift: tree ${size} vs HEAD ${cdl}` };
    return { ok: true };
  } catch (e) {
    return { ok: false, note: String(e.message || e) };
  }
}

// ── resolution ─────────────────────────────────────────────────────────────

function pickFiles(files, spec) {
  let out = files;
  if (spec.file) {
    out = files.filter((f) => f.path === spec.file);
  } else {
    if (spec.dir) out = out.filter((f) => f.path.startsWith(spec.dir));
    if (spec.ext) out = out.filter((f) => f.path.toLowerCase().endsWith(spec.ext.toLowerCase()));
    if (spec.fileMatch) {
      const needles = Array.isArray(spec.fileMatch) ? spec.fileMatch : [spec.fileMatch];
      out = out.filter((f) => needles.every((n) => f.path.toLowerCase().includes(String(n).toLowerCase())));
    }
    if (spec.exclude) {
      const bad = Array.isArray(spec.exclude) ? spec.exclude : [spec.exclude];
      out = out.filter((f) => !bad.some((n) => f.path.toLowerCase().includes(String(n).toLowerCase())));
    }
  }
  // Never ship README/gitattributes/preview media as an adapter or ckpt.
  out = out.filter((f) => !/(readme|\.gitattributes|\.gitignore|license)/i.test(f.path));
  if (spec.ext) out = out.filter((f) => f.path.toLowerCase().endsWith(spec.ext.toLowerCase()));
  return out.map((f) => ({
    path: f.path,
    size: f.lfs?.size ?? f.size ?? 0,
    sha256: f.lfs?.oid ?? null,
  }));
}

async function resolveAsset(spec, ctx) {
  const { repo } = spec;
  if (!repo) throw new Error("no repo");
  const t = await tree(repo);
  if (t.error) throw new Error(t.error);
  const hits = pickFiles(t.files, spec);
  if (!hits.length) {
    const sample = t.files
      .filter((f) => /\.(safetensors|gguf)$/i.test(f.path))
      .slice(0, 12)
      .map((f) => f.path);
    throw new Error(`no file matched in ${repo}${sample.length ? ` — candidates: ${sample.join(", ")}` : ""}`);
  }
  if (!spec.expand && hits.length > 1) {
    throw new Error(`ambiguous: ${hits.length} files matched in ${repo} (${hits.map((h) => h.path).slice(0, 5).join(", ")}) — set "file" or "expand": true`);
  }
  const out = [];
  for (const h of hits.sort((a, b) => a.path.localeCompare(b.path))) {
    let checked = null;
    if (DEEP) {
      checked = await deepCheck(repo, h.path, h.size);
      if (!checked.ok) throw new Error(`deep check failed ${h.path}: ${checked.note}`);
    }
    out.push({
      ...h,
      url: resolveUrl(repo, h.path),
      filePage: `${HF}/${repo}/blob/main/${encodeHfPath(h.path)}`,
      repoPage: `${HF}/${repo}`,
      repo,
      pair: spec.pair ? spec.pair : detectPair(h.path),
      title: titleFromPath(h.path),
      slug: slugFromPath(h.path),
    });
  }
  if (ctx && !ctx.meta) {
    ctx.meta = await repoMeta(repo);
    ctx.previewPath = pickPreviewPath(t.files, ctx.meta && ctx.meta.cardPreview);
  }
  const commit = ctx && ctx.meta && ctx.meta.sha;
  for (const o of out) {
    o.repoSha = commit || null;
    // No commit means no pinned preview. Showing an unpinned image could
    // display a different build than the bytes we hashed, so the row simply
    // renders without a thumbnail instead.
    o.preview = ctx && ctx.previewPath && commit
      ? HF + "/" + repo + "/resolve/" + commit + "/" + encodeHfPath(ctx.previewPath)
      : null;
  }
  return out;
}

/** Which files are default-selected inside an expanded repo.
 *  curatedMaxBytes keeps a repo's oversized extras (multi-GB merges that
 *  belong in the finetune tier) out of the default fetch even when the repo
 *  also ships real LoRAs. */
function isCurated(spec, path, bytes) {
  if (spec.select === "all") return true;
  if (spec.select === "none") return false;
  if (spec.curatedMaxBytes && bytes > spec.curatedMaxBytes) return false;
  // curatedMatch: a flat list is OR; a nested list is AND-of-OR groups, so
  // [["LT3X_V3"], ["furry"]] picks the v3 AIO or the furry variant, nothing else.
  const groups = (spec.curatedMatch || []).map((n) =>
    (Array.isArray(n) ? n : [n]).map((x) => String(x).toLowerCase()),
  );
  if (!groups.length) return true;
  const p = path.toLowerCase();
  return groups.some((g) => g.every((n) => p.includes(n)));
}

// ── CivitAI example imagery + release dates ─────────────────────────────────
// Most HF LoRA repos ship no sample media at all — a 55-file repo can contain
// zero images — which is why rows had nothing to show. The same creators
// publish their renders on CivitAI, so every catalog entry is looked up there
// and its example images are baked into the manifest. Rows then show pictures
// instantly and offline, with no per-row network call at runtime, and the
// matched version's createdAt gives a real release date that a HF repo cannot
// provide (a repo has no per-file publish date).

const CIVITAI_API = "https://civitai.red/api/v1";
const MAX_EXAMPLES = 8;
// Wrong examples are worse than none: a row showing another model's renders is
// actively misleading. So a hit must agree with our name in BOTH directions
// before its pictures are trusted; everything else gets a live search link.
const MATCH_MIN_COVER = 0.6; // min(coverage of ours, coverage of theirs)
const MATCH_MIN_SOLID = 2; // distinctive words that must actually line up

// Build vocabulary: a checkpoint and its LoRA share every one of these, so they
// can never tell two models apart.
const FORMAT_TOKENS = new Set([
  "int8", "int6", "int5", "int4", "convrot", "pruned", "fp8", "fp16", "bf16",
  "e4m3", "e5m2", "nvfp4", "nf4", "q2", "q3", "q4", "q5", "q6", "q8", "k", "m",
  "s", "gguf", "safetensors", "step", "steps", "ema", "rank", "rank16",
  "rank32", "rank64", "rank128", "alpha", "alpha128", "k3nk", "w4a8", "w4a16",
  "w8a8", "768p", "480p", "720p", "1080p", "p", "high", "low", "hn", "ln",
  "noise", "noadaln", "v",
]);

// Words that appear in nearly every adult model name, and family words that
// only say which architecture a model targets.
const GENERIC_TOKENS = new Set([
  "wan", "wanx", "ltx", "ltxv", "h3", "mm", "mmh3", "minimax", "krea", "flux",
  "qwen", "zimage", "z", "image", "img", "video", "motion", "lora", "lycoris",
  "checkpoint", "ckpt", "model", "merge", "preview", "unofficial", "official",
  "the", "and", "for", "with", "of", "to", "a", "an", "new", "best", "hd",
  "nsfw", "sfw", "adult", "general", "base", "ai", "native", "series", "i2v",
  "t2v", "flf2v", "22", "21", "14b", "ref2va", "fl2va", "fl2v", "t2i", "edit",
]);

/** CivitAI names the model, not the half or the quant, so search on the file's
 *  stem with those markers removed. Repo paths can arrive percent-encoded
 *  ("iGoon%20-%20Blink"), which would otherwise tokenise as "20blink". */
function searchLabel(path) {
  let p = String(path || "").split("/").pop();
  try {
    p = decodeURIComponent(p);
  } catch { /* keep the raw name */ }
  return p
    .replace(/\.(safetensors|gguf)$/i, "")
    .replace(/[-_ ]?(high|low)(\s?noise)?/gi, " ")
    .replace(/[-_ ]?(pruned|int8|int6|int4|convrot|bf16|fp8)/gi, " ")
    .replace(/\s+/g, " ")
    .trim();
}

/** Split on punctuation *and* camelCase: the same model is
 *  "AfterMidnight_ref2va" in a file name and "After Midnight MM H3 ref2va" on
 *  CivitAI, so "AfterMidnight" has to become two words to be comparable. */
function nameTokens(s) {
  const camel = String(s || "").replace(/([a-z0-9])([A-Z])/g, "$1 $2");
  return camel
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, " ")
    .split(" ")
    .filter((t) => t.length > 1 && !GENERIC_TOKENS.has(t) && !FORMAT_TOKENS.has(t));
}

function isVersionToken(t) {
  return /^v?\d+(\.\d+)*$/.test(t) || /^(beta|alpha|rc|preview|dev|exp)\d*$/.test(t);
}

/** Every word, including the generic and format ones — the fallback comparison
 *  for names made up entirely of those. */
function rawWords(s) {
  return String(s || "")
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, " ")
    .split(" ")
    .filter((t) => t.length > 1);
}

/** Weighted agreement between our name and a CivitAI name, in both directions.
 *
 *  An exact word match counts 1. A word the other side has glued together
 *  ("HandjobBlowjobCombo" vs "Combo Handjob Blowjob") counts 0.6. Both sides
 *  must be covered, because agreement in only one direction means a different
 *  model wearing a similar word — "Lusty Argonian Maid" must never illustrate
 *  "Lusty_Krea-2", and "Stretched Lips BJ" must not illustrate "lips-bj". */
function matchScore(ours, theirs) {
  const A = [...new Set(nameTokens(ours))];
  const B = [...new Set(nameTokens(theirs))];
  if (A.length < 2 || B.length < 2) {
    // Some names are made entirely of words we treat as generic
    // ("NSFW_Wan_14B_e15_Q4_K" vs "NSFW Wan 14B T2V"), or leave a single token
    // that cannot carry a match. Fall back to comparing the raw words.
    const rawA = rawWords(ours);
    const rawB = rawWords(theirs);
    if (!rawA.length || !rawB.length) return { score: 0, solid: 0, contained: false };
    const shared = rawA.filter((w) => rawB.includes(w));
    const score = Math.min(1, shared.length / Math.max(rawA.length, rawB.length));
    return {
      score,
      solid: shared.filter((w) => !isVersionToken(w)).length,
      contained: rawB.join("").length > rawA.join("").length
        && rawB.join("").includes(rawA.join("")),
    };
  }
  const usedB = new Set();
  let weighted = 0;
  let solid = 0;
  for (const a of A) {
    if (B.includes(a) && !usedB.has(a)) {
      usedB.add(a);
      weighted += 1;
      if (!isVersionToken(a)) solid += 1;
      continue;
    }
    for (const b of B) {
      if (usedB.has(b) || a.length < 4 || b.length < 4) continue;
      if (a.includes(b) || b.includes(a)) {
        usedB.add(b);
        weighted += 0.6;
        if (!isVersionToken(a)) solid += 1;
        break;
      }
    }
  }
  const coverA = Math.min(1, weighted / A.length);
  const coverB = Math.min(1, weighted / B.length);
  const joinedA = A.join("");
  const joinedB = B.join("");
  return {
    score: Math.min(coverA, coverB),
    coverA,
    coverB,
    solid,
    // ours sits strictly inside theirs => theirs carries words we do not have,
    // which is a different model with a similar name.
    contained: joinedB.length > joinedA.length && joinedB.includes(joinedA),
  };
}

// Family keyword -> the words that name it. A hit naming a *different* family is
// rejected: "LTX2_3_NSFW_furry_concat" must not borrow a WAN model's pictures.
const FAMILY_WORDS = {
  wan: ["wan", "wan22", "i2v", "t2v", "flf2v", "wan22"],
  h3: ["h3", "minimax", "mmh3", "ref2va", "fl2va", "fl2v", "lightx2v"],
  ltx: ["ltx", "ltx23", "ltx2"],
  image: ["qwen", "flux", "krea", "zimage", "zimage", "dev", "edit", "kontext"],
};

function familyCompatible(ours, theirs, family) {
  const t = nameTokens(theirs);
  const words = FAMILY_WORDS[family] || [];
  if (!words.length) return true;
  const names = t.some((x) => words.includes(x));
  if (names) return true;
  // No family word of ours in the hit: only acceptable when the hit does not
  // advertise a different one.
  for (const [f, ws] of Object.entries(FAMILY_WORDS)) {
    if (f === family) continue;
    if (t.some((x) => ws.includes(x))) return false;
  }
  return true;
}

/** Prefer the version that matches our own half: a HIGH half should show the
 *  HIGH example reel, not the LOW one. */
function versionScore(label, versionName) {
  const l = label.toLowerCase();
  const v = String(versionName || "").toLowerCase();
  let s = 0;
  for (const half of ["high", "low", "hn", "ln"]) {
    if (l.includes(half) && v.includes(half)) s += 2;
  }
  s += matchScore(label, versionName).solid > 0 ? 1 : 0;
  return s;
}

const civitaiCache = new Map();

async function civitaiExamples(label, family) {
  if (!label) return null;
  const key = `civitai::${family}::${label}`;
  if (civitaiCache.has(key)) return civitaiCache.get(key);
  const disk = readCache(key);
  if (disk !== null && disk !== undefined && !FORCE) {
    civitaiCache.set(key, disk);
    return disk;
  }
  let out = null;
  try {
    const r = await fetchJson(
      `${CIVITAI_API}/models?query=${encodeURIComponent(label)}&limit=8&nsfw=1`,
    );
    if (!r.error && r.json) {
      const all = r.json.items || [];
      const cands = all
        .map((m) => ({ m, sc: matchScore(label, m.name) }))
        .filter((x) => x.sc.score >= MATCH_MIN_COVER && x.sc.solid >= MATCH_MIN_SOLID
          && !x.sc.contained && familyCompatible(label, x.m.name, family))
        .sort((a, b) => b.sc.score - a.sc.score
          || (b.m.stats?.monthlyDownloadCount || 0) - (a.m.stats?.monthlyDownloadCount || 0));
      // Why a row got no picture is worth knowing when the catalog is curated.
      if (MATCH_DEBUG && !cands.length && all.length) {
        const top = all.map((m) => ({ m, sc: matchScore(label, m.name) }))
          .sort((a, b) => b.sc.score - a.sc.score)[0];
        console.log(`    miss "${label}" — best ${top.sc.score.toFixed(2)} solid=${top.sc.solid}${top.sc.contained ? " (contained)" : ""} "${top.m.name}"`);
        console.log(`        ours: [${nameTokens(label).join(" ")}]  theirs: [${nameTokens(top.m.name).join(" ")}]`);
      }
      if (cands.length) {
        const best = cands[0].m;
        const versions = (best.modelVersions || [])
          .map((v) => ({ v, s: versionScore(label, v.name) }))
          .filter((x) => (x.v.images || []).length)
          .sort((a, b) => b.s - a.s
            || String(b.v.createdAt || "").localeCompare(String(a.v.createdAt || "")));
        if (versions.length) {
          const ver = versions[0].v;
          // The list endpoint omits version dates; the detail endpoint has them,
          // and a release date is the one thing a HF mirror can never tell us.
          let releasedAt = ver.createdAt || null;
          if (!releasedAt) {
            const dk = `civitai-detail::${best.id}`;
            let detail = readCache(dk);
            if (detail === null || detail === undefined || FORCE) {
              const d = await fetchJson(`${CIVITAI_API}/models/${best.id}`);
              detail = d.error || !d.json ? null : (d.json.modelVersions || []);
              writeCache(dk, detail);
            }
            const hit = (detail || []).find((v) => String(v.id) === String(ver.id));
            releasedAt = (hit && hit.createdAt) || null;
          }
          const examples = (ver.images || []).slice(0, MAX_EXAMPLES).map((im) => ({
            url: im.url,
            // CivitAI states the media type; a guessed extension would mislabel
            // its .mp4 samples as broken images.
            kind: im.type === "video" ? "video" : "image",
          }));
          out = {
            civitaiId: best.id,
            civitaiName: best.name,
            civitaiVersion: ver.name || null,
            releasedAt,
            examples,
            matchScore: Number(cands[0].sc.score.toFixed(2)),
          };
        }
      }
    }
  } catch {
    out = null;
  }
  writeCache(key, out);
  civitaiCache.set(key, out);
  return out;
}

/** Run lookups a few at a time: 100+ sequential CivitAI hits is rude, and the
 *  API is the slowest thing in the run. */
async function pool(items, limit, worker) {
  const out = new Array(items.length);
  let next = 0;
  const runners = Array.from({ length: Math.min(limit, items.length) }, async () => {
    for (;;) {
      const i = next++;
      if (i >= items.length) return;
      out[i] = await worker(items[i], i);
    }
  });
  await Promise.all(runners);
  return out;
}

// ── main ───────────────────────────────────────────────────────────────────

const sources = JSON.parse(readFileSync(SOURCES, "utf8"));
const problems = [];
const loras = [];
const finetunes = [];
const civitai = [];
let repoMetaCache = new Map();

function want(id) {
  return !onlySet || onlySet.has(id);
}

for (const spec of sources.loras || []) {
  if (!want(spec.id)) continue;
  try {
    const ctx = {};
    const files = await resolveAsset(spec, ctx);
    if (ctx.meta) repoMetaCache.set(spec.repo, ctx.meta);
    const groupId = spec.group;
    const groupDefault = sources.groups.find((g) => g.id === groupId)?.defaultSelected !== false;
    const filesOut = files.map((f, i) => {
      const curated = isCurated(spec, f.path, f.size);
      return {
        id: files.length === 1 && !spec.expand ? spec.id : `${spec.id}-${f.slug || i}`,
        sourceId: spec.id,
        group: groupId,
        family: spec.family,
        repo: f.repo,
        repoSha: f.repoSha || null,
        // Provenance + a pinned preview so the panel can show what you are
        // about to download instead of a bare filename.
        preview: f.preview || null,
        version: versionToken(f.path, ctx && ctx.meta),
        sourceUpdatedAt: (ctx && ctx.meta && ctx.meta.lastModified) || null,
        filePage: f.filePage,
        repoPage: f.repoPage,
        name: files.length === 1 && spec.name ? spec.name : f.title,
        file: f.path,
        url: f.url,
        sha256: f.sha256,
        bytes: f.size,
        pair: f.pair,
        pairId: f.pair && files.length > 1 ? spec.id : null,
        note: spec.note || "",
        trigger: spec.trigger || "",
        license: spec.license || "hf-mirror",
        licenseNote: spec.licenseNote || "",
        wan2gpIds: spec.wan2gpIds || [],
        defaultSelected: groupDefault && curated,
      };
    });
    // HIGH/LOW halves must travel together (Wan 2.2 loads both in one job), but the
    // pairing is per pair — not per repo, or one curated pair would drag in
    // every uncurated pair the same author happened to ship.
    const pairGroups = new Map();
    for (const f of filesOut) {
      if (!f.pair) continue;
      const k = `${spec.id}::${pairStem(f.file)}`;
      if (!pairGroups.has(k)) pairGroups.set(k, []);
      pairGroups.get(k).push(f);
    }
    for (const g of pairGroups.values()) {
      if (g.length < 2) continue;
      const anySelected = g.some((f) => f.defaultSelected);
      for (const f of g) f.defaultSelected = anySelected;
    }

    // Files land flat inside loras/<collection>/adult-pack/, so the manifest's
    // `file` must be a bare name — never the repo path (a "loras/" prefix
    // would be rejected as unsafe). Disambiguate same-named files.
    const seenNames = new Map();
    for (const f of filesOut) {
      let local = f.file.split("/").pop();
      if (seenNames.has(local)) {
        const dir = f.file.split("/").slice(-2, -1)[0] || "alt";
        local = `${dir}_${local}`;
      }
      seenNames.set(local, true);
      f.file = local;
    }
    loras.push(...filesOut);
  } catch (e) {
    problems.push({ id: spec.id, kind: "lora", error: String(e.message || e) });
  }
}

for (const spec of sources.finetunes || []) {
  if (!want(spec.id)) continue;
  const ctx = {};
  try {
    const high = await resolveAsset({ ...spec, expand: false }, ctx);
    if (ctx.meta) repoMetaCache.set(spec.repo, ctx.meta);
    const named = wan2gpName(spec.name, high[0].path, spec.quant);
    const ft = {
      id: spec.id,
      family: spec.family,
      architecture: spec.architecture,
      name: named.name,
      precisionToken: named.token,
      quant: spec.quant || null,
      description: spec.description || "",
      urls: [high[0].url],
      bytes: high[0].size,
      sha256: high[0].sha256,
      file: high[0].path,
      repo: spec.repo,
      repoSha: high[0].repoSha || null,
      preview: high[0].preview || null,
      version: versionToken(high[0].path, ctx && ctx.meta),
      sourceUpdatedAt: (ctx && ctx.meta && ctx.meta.lastModified) || null,
      filePage: high[0].filePage,
      repoPage: high[0].repoPage,
      urls2: null,
      recommended: spec.recommended || null,
      preloadLoras: spec.preloadLoras || [],
      loraMultipliers: spec.loraMultipliers || null,
      wan2gpIds: spec.wan2gpIds || [],
      license: spec.license || "hf-mirror",
      licenseNote: spec.licenseNote || "",
      source: "hf",
      prefetchDefault: spec.prefetchDefault !== false,
      defaultSelected: spec.defaultSelected !== false,
      // HIGH/LOW pairs keep their halves separately addressable: `bytes` is the
      // file in `urls`, `bytesLow`/`sha256Low` the second phase. Summing them
      // would make each half fail its own size check at download time.
      bytesLow: 0,
      sha256Low: null,
    };
    if (spec.lowRepo) {
      const low = await resolveAsset({ ...spec.lowRepo, expand: false }, {});
      ft.urls2 = [low[0].url];
      ft.bytesLow = low[0].size || 0;
      ft.sha256Low = low[0].sha256;
      ft.fileLow = low[0].path;
    }
    finetunes.push(ft);
  } catch (e) {
    problems.push({ id: spec.id, kind: "finetune", error: String(e.message || e) });
  }
}

for (const spec of sources.civitai || []) {
  if (!want(spec.id)) continue;
  civitai.push({
    id: spec.id,
    family: spec.family,
    architecture: spec.architecture,
    name: spec.name,
    // Live resolution: the runtime searches CivitAI for `query` (filtered by
    // baseModelFilter) instead of pinning a version id that goes stale.
    query: spec.query || null,
    baseModelFilter: spec.baseModelFilter || null,
    civitaiModelId: spec.civitaiModelId || null,
    civitaiVersionId: spec.civitaiVersionId || null,
    bytesEstimate: spec.bytesEstimate || null,
    description: spec.description || "",
    recommended: spec.recommended || null,
    wan2gpIds: spec.wan2gpIds || [],
    licenseNote: spec.licenseNote || "",
    prefetchDefault: spec.prefetchDefault !== false,
    defaultSelected: false,
  });
}

// ── example imagery + release dates ─────────────────────────────────────────
// HF media first (pinned to a verified commit), CivitAI renders as the fill for
// the many repos that ship none. Baked into the manifest so the panel shows
// pictures without a network round-trip per row.
if (EXAMPLES) {
  const specById = new Map();
  for (const s of [...(sources.loras || []), ...(sources.finetunes || [])]) specById.set(s.id, s);

  // Looked up per MODEL, not per spec: one expanded repo can hold 50 unrelated
  // LoRAs, and each of them is published separately on CivitAI under its own
  // name. Searching the repo name once would miss all of them.
  const targets = [
    ...loras.filter((l) => !l.preview).map((l) => ({ item: l, label: searchLabel(l.file), family: l.family })),
    ...finetunes.filter((f) => !f.preview).map((f) => ({ item: f, label: searchLabel(f.file), family: f.family })),
  ];
  const seen = new Set();
  const jobs = targets.filter((t) => {
    const k = `${t.family}::${t.label.toLowerCase()}`;
    if (!t.label || seen.has(k)) return false;
    seen.add(k);
    return true;
  });

  let matched = 0;
  await pool(jobs, 3, async (job) => {
    const hit = await civitaiExamples(job.label, job.family);
    if (!hit) return;
    matched += 1;
    job.item.examples = hit.examples;
    job.item.releasedAt = hit.releasedAt;
    job.item.civitaiId = hit.civitaiId;
    job.item.civitaiName = hit.civitaiName;
    job.item.civitaiVersion = hit.civitaiVersion;
    job.item.civitaiPage = `https://civitai.red/models/${hit.civitaiId}`;
    process.stdout.write(`  · "${job.label}" → civitai ${hit.civitaiId} "${hit.civitaiName}" ${hit.examples.length} examples (score ${hit.matchScore})\n`);
  });
  console.log(`  civitai lookups: ${jobs.length} distinct names, ${matched} matched`);

  // Every row gets a way to find its renders even without a trusted match: most
  // HF mirrors ship no media at all, so CivitAI is where the examples live.
  for (const t of [...loras, ...finetunes]) {
    const q = searchLabel(t.file) || t.name;
    t.civitaiSearch = `https://civitai.red/models?query=${encodeURIComponent(q)}&nsfw=1`;
    if (!t.examples || !t.examples.length) delete t.examples;
  }

  // Whatever CivitAI could not supply falls back to the repo's publish date.
  for (const t of [...loras, ...finetunes]) {
    if (!t.releasedAt) {
      const meta = repoMetaCache.get(t.repo);
      t.releasedAt = (meta && meta.createdAt) || null;
    }
  }
}

const withExamples = [...loras, ...finetunes].filter((t) => t.examples && t.examples.length).length;
const withAnyImage = [...loras, ...finetunes].filter((t) => (t.examples && t.examples.length) || t.preview).length;
console.log(`  imagery: ${withAnyImage}/${loras.length + finetunes.length} entries have pictures (${withExamples} from CivitAI)`);

// ── report ─────────────────────────────────────────────────────────────────

const gb = (b) => `${(b / 1073741824).toFixed(2)} GB`;
const sum = (arr) => arr.reduce((a, x) => a + (x.bytes || 0), 0);
const selectedLoras = loras.filter((l) => l.defaultSelected);
const prefetch = finetunes.filter((f) => f.defaultSelected && f.prefetchDefault);

console.log(`\nAdult pack — ${loras.length} lora files, ${finetunes.length} finetunes, ${civitai.length} civitai items`);
console.log(`  loras (all)        ${gb(sum(loras))}`);
console.log(`  loras (default on) ${gb(sum(selectedLoras))}  (${selectedLoras.length} files)`);
console.log(`  prefetch ckpts     ${gb(sum(prefetch))}  (${prefetch.length} items)`);

const byGroup = new Map();
for (const l of loras) {
  const g = byGroup.get(l.group) || { n: 0, on: 0, bytes: 0, onBytes: 0 };
  g.n += 1;
  g.bytes += l.bytes || 0;
  if (l.defaultSelected) {
    g.on += 1;
    g.onBytes += l.bytes || 0;
  }
  byGroup.set(l.group, g);
}
for (const g of sources.groups) {
  const v = byGroup.get(g.id);
  console.log(
    `  · ${g.id.padEnd(6)} ${String(v?.n ?? 0).padStart(3)} files  default ${String(v?.on ?? 0).padStart(3)}  ${gb(v?.onBytes ?? 0)} / ${gb(v?.bytes ?? 0)}`,
  );
}

const pairs = loras.filter((l) => l.pair);
console.log(`  HIGH/LOW halves detected: ${pairs.length} (high ${pairs.filter((p) => p.pair === "high").length} / low ${pairs.filter((p) => p.pair === "low").length})`);
if (!ONLY) {
  console.log("\n  largest lora files:");
  for (const l of [...loras].sort((a, b) => b.bytes - a.bytes).slice(0, 18)) {
    console.log(
      `    ${gb(l.bytes).padStart(9)}  ${l.defaultSelected ? "on " : "off"}  ${l.repo}/${l.file}`,
    );
  }
}
const lone = pairs.filter((p) => !pairs.some((o) => o.sourceId === p.sourceId && o.pair !== p.pair && o.pair));
if (lone.length) console.log(`  ! ${lone.length} half without partner: ${lone.map((l) => l.file).slice(0, 5).join(", ")}`);

if (problems.length) {
  console.log(`\nFAILED (${problems.length}):`);
  for (const p of problems) console.log(`  ✗ ${p.id} [${p.kind}]: ${p.error}`);
} else {
  console.log("\nAll entries resolved.");
}

if (REPORT_ONLY) {
  console.log("\n(report mode — manifest not written)");
  process.exit(problems.length ? 1 : 0);
}

const manifest = {
  manifestVersion: sources.manifestVersion ?? 1,
  title: sources.title,
  generatedAt: new Date().toISOString(),
  verifiedAgainst: sources.verifiedAgainst,
  repoMeta: Object.fromEntries(repoMetaCache),
  groups: sources.groups,
  legal: sources.legal || [],
  // Family keyword -> Wan2GP architecture. Drives the "search and add your
  // own" flow, so a newly supported family is a sources edit, not code.
  archMap: sources.archMap || [],
  totals: {
    loraFiles: loras.length,
    loraBytes: sum(loras),
    loraDefaultFiles: selectedLoras.length,
    loraDefaultBytes: sum(selectedLoras),
    prefetchBytes: sum(prefetch),
  },
  loras,
  finetunes,
  civitai,
};
// Write atomically: a truncated manifest would silently empty the panel, and a
// killed pipe (`node verify.mjs | Select-Object -First 5`) is exactly how that
// happens on Windows. Temp file + rename, so readers see the old file or the
// new one, never a half-written one.
const tmp = MANIFEST + ".tmp";
writeFileSync(tmp, JSON.stringify(manifest, null, 2) + "\n");
renameSync(tmp, MANIFEST);
console.log(`\nWrote ${MANIFEST}`);
process.exit(problems.length ? 1 : 0);
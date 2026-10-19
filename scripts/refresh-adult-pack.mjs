/**
 * refresh-adult-pack.mjs — re-resolve the catalog and report what actually moved.
 *
 * Run by the weekly workflow, by a human before tagging a release, and by `--check`
 * in CI to assert the committed manifest still matches reality. Three modes:
 *
 *   (default)   verify, then print a human summary of the differences
 *   --check     exit 1 if the manifest is stale (used by CI)
 *   --refresh   bypass the 12h listing cache (what the weekly run wants)
 *
 * `generatedAt` is excluded from the comparison on purpose: it changes on every
 * run, so including it would report drift forever and the check would be noise
 * nobody trusts.
 *
 * The comparison logic is exported so it can be tested without running the
 * verifier — a freshness check that is never itself tested is the one thing you
 * cannot afford to be subtly wrong.
 */
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, '..');
const MANIFEST = join(ROOT, 'src', 'adult-pack-manifest.json');

const readManifest = () => JSON.parse(readFileSync(MANIFEST, 'utf8'));

/** Everything except the run timestamp. */
export function comparable(m) {
  const { generatedAt, ...rest } = m;
  return JSON.stringify(rest, null, 2);
}

function bytesOf(it) {
  return (it.bytes || 0) + (it.bytesLow || 0);
}

/** One row per catalog entry, so a diff can say *which* model changed.
 *  HIGH/LOW halves are separate rows because they are separately pinned. */
export function index(m) {
  const out = new Map();
  for (const bucket of ['loras', 'finetunes']) {
    for (const it of m[bucket] || []) {
      out.set(`${bucket}:${it.id}`, {
        name: it.name || it.file || it.id,
        repo: it.repo || '',
        sha256: it.sha256 || '',
        bytes: bytesOf(it),
      });
    }
  }
  return out;
}

const mb = (b) => `${(b / 1048576).toFixed(1)} MB`;

/** The reviewable summary: which models appeared, vanished, or changed size/hash.
 *  A regenerated manifest is thousands of lines; this is the part a human reads. */
export function diffManifest(before, after) {
  const bi = index(before);
  const ai = index(after);
  const added = [...ai.keys()].filter((k) => !bi.has(k));
  const removed = [...bi.keys()].filter((k) => !ai.has(k));
  const changed = [...ai.keys()].filter((k) => {
    if (!bi.has(k)) return false;
    const a = ai.get(k);
    const b = bi.get(k);
    return a.sha256 !== b.sha256 || a.bytes !== b.bytes || a.repo !== b.repo;
  });
  const changedBytes = changed.reduce((a, k) => a + (ai.get(k).bytes - bi.get(k).bytes), 0);
  return {
    added,
    removed,
    changed,
    changedBytes,
    stale: comparable(before) !== comparable(after),
    total: added.length + removed.length + changed.length,
  };
}

export function formatSummary(diff, limit = 12) {
  if (!diff.stale) return 'manifest is up to date — nothing changed upstream.';
  const lines = [`${diff.total} change(s):`];
  const show = (label, keys) => {
    if (!keys.length) return;
    lines.push(`  ${label}: ${keys.length}`);
    for (const k of keys.slice(0, limit)) {
      const it = diff.after?.get(k) || diff.before?.get(k);
      lines.push(`    - ${it.name}  (${it.repo})`);
    }
    if (keys.length > limit) lines.push(`    … and ${keys.length - limit} more`);
  };
  show('added', diff.added);
  show('removed', diff.removed);
  if (diff.changed.length) {
    const delta = diff.changedBytes ? ` (${diff.changedBytes > 0 ? '+' : ''}${mb(Math.abs(diff.changedBytes))})` : '';
    lines.push(`  changed hash or size: ${diff.changed.length}${delta}`);
    for (const k of diff.changed.slice(0, limit)) {
      const a = diff.after.get(k);
      const b = diff.before.get(k);
      const sizeDelta = a.bytes - b.bytes;
      lines.push(
        `    - ${a.name}\n        sha ${String(b.sha256).slice(0, 12)} -> ${String(a.sha256).slice(0, 12)}` +
          (sizeDelta
            ? `, size ${mb(b.bytes)} -> ${mb(a.bytes)} (${sizeDelta > 0 ? '+' : ''}${mb(Math.abs(sizeDelta))})`
            : ''),
      );
    }
    if (diff.changed.length > limit) lines.push(`    … and ${diff.changed.length - limit} more`);
  }
  return lines.join('\n');
}

function runVerifier(refresh) {
  const args = ['scripts/verify-adult-pack.mjs'];
  if (refresh) args.push('--refresh');
  try {
    const out = execFileSync(process.execPath, args, {
      cwd: ROOT,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'inherit'],
      maxBuffer: 64 * 1024 * 1024,
    });
    return { out, failed: false };
  } catch (e) {
    // The verifier exits 1 when an entry could not be resolved. Worth reporting
    // loudly, but the manifest it did write is still the honest state of the
    // catalog minus whatever broke.
    return { out: e.stdout || '', failed: true };
  }
}

export function main(argv = process.argv.slice(2)) {
  const checkOnly = argv.includes('--check');
  const refresh = argv.includes('--refresh');
  const base = (argv.find((a) => a.startsWith('--base=')) || '').split('=')[1] || '';

  const before = readManifest();
  const { failed } = runVerifier(refresh);
  const after = readManifest();
  const diff = diffManifest(before, after);
  diff.before = index(before);
  diff.after = index(after);

  console.log(
    `catalog: ${(after.loras || []).length} lora files, ${(after.finetunes || []).length} recipes`,
  );
  console.log(`generated: ${before.generatedAt} -> ${after.generatedAt}`);
  console.log('\n' + formatSummary(diff));

  if (base) {
    const baseOut = execFileSync('git', ['show', `${base}:src/adult-pack-manifest.json`], {
      cwd: ROOT,
      encoding: 'utf8',
      maxBuffer: 64 * 1024 * 1024,
    });
    const bi = index(JSON.parse(baseOut));
    console.log(
      `\nversus ${base}: +${[...diff.after.keys()].filter((k) => !bi.has(k)).length}` +
        ` / -${[...bi.keys()].filter((k) => !diff.after.has(k)).length}`,
    );
  }

  if (failed) {
    console.error('\nthe verifier exited non-zero: at least one entry could not be resolved.');
    console.error('see the output above; the manifest has NOT been rolled back.');
  }
  if (checkOnly && diff.stale) {
    console.error('\nFAIL: the committed manifest is stale. Run: npm run pack:refresh');
    process.exitCode = 1;
  } else if (checkOnly) {
    console.log('\nOK: manifest matches the sources.');
  }
  if (failed) process.exitCode = 1;
}

// Only run when invoked directly, so a test can import the logic above without
// kicking off a verifier run.
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
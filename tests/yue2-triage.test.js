/**
 * yue2-triage.test.js — upstream #2517 / launcher #82.
 *
 * WanGP embeds each output's settings in the media file and migrates them at
 * startup, so one stale `audio_prompt_type` ("AV" from an older version)
 * aborts the UI with `ValueError: Choose one of the five YuE2 composition
 * sources.` A throwaway settings folder cannot hide it (--config only moves
 * wgp_config.json), so the launcher scans the outputs and offers to move the
 * offender into yue2-quarantine/ instead of retrying blindly.
 *
 * Boots the real troubleshoot-tab.js in a vm: the tests drive the panel
 * buttons and the startup-triage hook, and assert what reaches the backend.
 */
const test = require("node:test");
const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const SRC = fs.readFileSync(
  path.join(__dirname, "..", "src", "troubleshoot-tab.js"),
  "utf8",
);

function makeEl() {
  return {
    textContent: "",
    style: {},
    disabled: false,
    listeners: {},
    addEventListener(type, cb) {
      this.listeners[type] = cb;
    },
  };
}

/**
 * Boot the panel. `meta` is a queue of scan responses (the panel rescans after
 * a quarantine, so the second call usually reports a clean folder).
 */
function load({ meta = [], quarantined = null, choice = "ok" } = {}) {
  const els = {};
  const ids = [
    "tsYue2CheckBtn",
    "tsYue2FixBtn",
    "tsYue2Status",
    "tsYue2Box",
  ];
  for (const id of ids) els[id] = makeEl();
  const calls = { scan: 0, quarantine: 0 };
  const log = [];
  const dialogs = [];
  const window = {
    w2gp: {
      configLoad: async () => ({}),
      tsYue2Metadata: async () => {
        calls.scan += 1;
        return meta.length > 1 ? meta.shift() : meta[meta.length - 1];
      },
      tsYue2Quarantine: async () => {
        calls.quarantine += 1;
        return quarantined || { ok: true, dir: "X", moved: [], failed: [] };
      },
      confirmDialog: async (o) => {
        dialogs.push(o);
        return choice;
      },
    },
  };
  const ctx = {
    window,
    document: { createElement: () => ({ canPlayType: () => "" }) },
    $: (id) => els[id] || null,
    appendLog: (m) => log.push(String(m)),
    showToast: () => {},
    errText: (e) => String(e),
    escHtml: (s) => String(s),
    console,
  };
  ctx.globalThis = ctx;
  vm.createContext(ctx);
  vm.runInContext(SRC, ctx, { filename: "troubleshoot-tab.js" });
  return { ctx, els, calls, log, dialogs };
}

const BAD = {
  ok: true,
  scanned: 2170,
  tookMs: 412,
  truncated: false,
  roots: ["C:\\Wan2GP\\outputs"],
  bad: [
    {
      name: "yue2_song.wav",
      path: "C:\\Wan2GP\\outputs\\yue2_song.wav",
      value: "AV",
      size: 5242880,
    },
  ],
};
const CLEAN = { ok: true, scanned: 2170, tookMs: 90, truncated: false, bad: [] };

test("the triage names the bad file and quarantines it before relaunching", async () => {
  const { ctx, calls, dialogs } = load({
    meta: [BAD],
    quarantined: {
      ok: true,
      dir: "C:\\Wan2GP\\outputs\\yue2-quarantine",
      failed: [],
      moved: [{ name: "yue2_song.wav", value: "AV" }],
    },
  });
  let relaunched = 0;
  const handled = await ctx.window.w2gpYue2Triage(() => {
    relaunched += 1;
  });
  assert.equal(handled, true, "the caller must skip the generic --config offer");
  assert.equal(calls.scan, 1, "one scan is enough to find the offender");
  assert.equal(calls.quarantine, 1);
  assert.equal(relaunched, 1, "Wan2GP is started again once the file is out");
  assert.match(dialogs[0].title, /YuE2/);
  assert.match(dialogs[0].message, /#2517/, "names the upstream bug");
  assert.match(dialogs[0].detail, /yue2_song\.wav/);
  assert.match(dialogs[0].detail, /yue2-quarantine/);
  assert.match(dialogs[0].detail, /nothing is deleted/i);
});

test("cancelling keeps the file and never relaunches", async () => {
  const { ctx, calls } = load({ meta: [BAD], choice: "cancel" });
  let relaunched = 0;
  assert.equal(
    await ctx.window.w2gpYue2Triage(() => {
      relaunched += 1;
    }),
    true,
  );
  assert.equal(calls.quarantine, 0);
  assert.equal(relaunched, 0);
});

test("a clean output folder falls through to the generic triage", async () => {
  const { ctx, calls, dialogs } = load({ meta: [CLEAN] });
  assert.equal(await ctx.window.w2gpYue2Triage(() => {}), false);
  assert.equal(calls.quarantine, 0);
  assert.equal(dialogs.length, 0, "no dialog when there is nothing to fix");
});

test("no output folder found is not reported as a clean folder", async () => {
  const { ctx, dialogs } = load({ meta: [{ ok: false, bad: [] }] });
  assert.equal(await ctx.window.w2gpYue2Triage(() => {}), false);
  assert.equal(dialogs.length, 0);
});

test("a saved settings file is labeled, not shown as an audio output", async () => {
  const { els } = load({
    meta: [
      {
        ok: true,
        scanned: 1,
        tookMs: 4,
        bad: [
          {
            name: "yue2_settings.json",
            path: "C:\\Wan2GP\\settings\\yue2_settings.json",
            value: "AV",
            size: 1234,
            kind: "settings",
          },
        ],
      },
    ],
  });
  await els.tsYue2CheckBtn.listeners.click.call(els.tsYue2CheckBtn);
  assert.match(els.tsYue2Box.textContent, /yue2_settings\.json/);
  assert.match(els.tsYue2Box.textContent, /saved settings/);
  assert.doesNotMatch(els.tsYue2Box.textContent, /1 KB/);
});

test("the panel button lists the offenders and offers the quarantine", async () => {
  const { els, calls } = load({ meta: [BAD] });
  await els.tsYue2CheckBtn.listeners.click.call(els.tsYue2CheckBtn);
  assert.equal(calls.scan, 1);
  assert.match(els.tsYue2Status.textContent, /⚠ 1 file\(s\)/);
  assert.match(els.tsYue2Status.textContent, /2170/);
  assert.equal(els.tsYue2Box.style.display, "block");
  assert.match(els.tsYue2Box.textContent, /yue2_song\.wav/);
  assert.match(els.tsYue2Box.textContent, /"AV"/);
  assert.equal(els.tsYue2FixBtn.style.display, "", "quarantine becomes available");
});

test("the quarantine button moves the files and re-reads the folder", async () => {
  const { els, calls } = load({
    meta: [CLEAN],
    quarantined: {
      ok: true,
      dir: "X",
      failed: [],
      moved: [{ name: "yue2_song.wav" }],
    },
  });
  await els.tsYue2FixBtn.listeners.click.call(els.tsYue2FixBtn);
  assert.equal(calls.quarantine, 1);
  assert.equal(calls.scan, 1, "rescan after the move");
  assert.match(els.tsYue2Status.textContent, /✓ moved 1 file\(s\)/);
  assert.equal(els.tsYue2FixBtn.style.display, "none", "nothing left to fix");
  assert.equal(els.tsYue2Box.style.display, "none");
});

test("a clean folder never shows the quarantine button", async () => {
  const { els, calls } = load({ meta: [CLEAN] });
  await els.tsYue2CheckBtn.listeners.click.call(els.tsYue2CheckBtn);
  assert.match(els.tsYue2Status.textContent, /✓ 2170/);
  assert.equal(els.tsYue2FixBtn.style.display, "none");
  assert.equal(calls.quarantine, 0);
});

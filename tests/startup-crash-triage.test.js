/**
 * startup-crash-triage.test.js — "process exited (code 1)" must not be the end
 * of the story when Wan2GP died on the way up.
 *
 * Upstream #2517 is the shape: a settings entry Wan2GP cannot migrate kills
 * the process before the UI appears, and the launcher used to show the same
 * bare exit line as a generation error. resetui-tab.js now decides whether a
 * non-zero exit is a *startup* crash and offers one retry against a throwaway
 * settings folder (--config), which separates "my settings" from "my install".
 *
 * The gate is behaviour, not markup, so these tests boot the real file in a vm
 * with the globals app.js would provide and drive its onWangpExit handler.
 */
const test = require("node:test");
const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const SRC = fs.readFileSync(
  path.join(__dirname, "..", "src", "resetui-tab.js"),
  "utf8",
);

/** Boot resetui-tab.js with stub globals; returns the captured exit handler. */
function load({
  choice = "cancel",
  tail = "",
  yue2 = null,
  platform = "win32",
  uninstallOk = true,
} = {}) {
  const log = [];
  const dialogs = [];
  const removed = [];
  const clicks = { browser: 0 };
  const browserBtn = {
    disabled: true,
    click() {
      clicks.browser += 1;
    },
  };
  const handlers = { launchLog: [] };
  const window = {
    w2gpYue2Triage: yue2 || undefined,
    w2gp: {
      onLaunchLog: (cb) => {
        handlers.launchLog.push(cb);
      },
      onTermClosed: () => {},
      onTermSetDock: () => {},
      onAppClosing: () => {},
      onDeepyExit: () => {},
      onWangpExit: (cb) => {
        handlers.exit = cb;
      },
      destroyBrowserView: () => Promise.resolve(),
      uninstallPackage: async (pkg) => {
        removed.push(pkg);
        // The Rust command rejects on a failed pip run (Err), so both shapes
        // are real and both must be reported, not thrown.
        if (uninstallOk === "throws")
          throw new Error("pip uninstall wgpkernels failed");
        return uninstallOk
          ? { ok: true, success: true }
          : { ok: false, success: false, error: "pip said no" };
      },
      confirmDialog: async (o) => {
        dialogs.push(o);
        return typeof choice === "function" ? choice(o) : choice;
      },
    },
    _getLogTail: () => tail,
  };
  const ctx = {
    window,
    document: { body: { dataset: { platform } } },
    $: (id) => (id === "browserBtn" ? browserBtn : null),
    appendLog: (m) => log.push(String(m)),
    showToast: () => {},
    setAppLaunchLabel: () => {},
    updateLed: () => {},
    updateFtStatus: () => {},
    refreshMainLan: () => {},
    hideBrowserRunningUI: () => {},
    resetBrowserLaunchUI: () => {},
    // app.js: the message, not "Error: " + message.
    errText: (e) => (e && e.message) || (e == null ? "unknown error" : String(e)),
    serverMode: "browser",
    _expectServerExit: false,
    _expectServerExitTimer: null,
    setTimeout,
    clearTimeout,
    console,
  };
  ctx.globalThis = ctx;
  vm.createContext(ctx);
  vm.runInContext(SRC, ctx, { filename: "resetui-tab.js" });
  const emitLog = (line) => handlers.launchLog.forEach((cb) => cb(line));
  return { ctx, log, dialogs, clicks, removed, exit: handlers.exit, emitLog };
}

/** Arm the state a launch arms, then fire the exit payload. */
async function crash(ctx, exit, { code = 1, agoMs = 5000, served = false } = {}) {
  ctx.window.__wan2gpLaunchAt = Date.now() - agoMs;
  ctx.window.__wan2gpServed = served;
  await exit({ code });
}

test("a non-zero exit before the server answered offers the clean-settings retry", async () => {
  const { ctx, log, dialogs, exit } = load({ choice: "ok" });
  await crash(ctx, exit);
  assert.equal(dialogs.length, 1, "exactly one triage dialog");
  assert.match(dialogs[0].title, /crashed on startup/i);
  assert.match(dialogs[0].detail, /temporary, empty settings folder/i);
  assert.ok(
    log.some((l) => /did not start/.test(l)),
    "the console names a startup failure, not a bare exit code",
  );
  assert.ok(
    log.some((l) => /temporary settings folder/.test(l)),
    "the retry is announced",
  );
  // No launch button in this sandbox: the one-shot stays armed for the next
  // launch instead of being dropped.
  assert.equal(ctx.window.__safeConfigNextLaunch, true);
});

test("a crash after the server came up is not triaged as a startup failure", async () => {
  const { ctx, dialogs, exit } = load();
  await crash(ctx, exit, { served: true });
  assert.equal(dialogs.length, 0);
});

test("a manual stop is never triaged", async () => {
  const { ctx, dialogs, exit } = load();
  ctx._expectServerExit = true;
  await crash(ctx, exit);
  assert.equal(dialogs.length, 0);
});

test("a clean exit and a late crash are left alone", async () => {
  const a = load();
  await crash(a.ctx, a.exit, { code: 0 });
  assert.equal(a.dialogs.length, 0);

  const b = load();
  // Past the startup window: a runtime crash is a generation failure.
  await crash(b.ctx, b.exit, { agoMs: 30 * 60 * 1000 });
  assert.equal(b.dialogs.length, 0);
});

test("one offer per crash streak, re-armed only by a successful boot", async () => {
  const { ctx, log, dialogs, exit } = load();
  await crash(ctx, exit);
  assert.equal(dialogs.length, 1);
  const triageLines = () =>
    log.filter((l) => /did not start|temporary settings folder/.test(l)).length;
  const first = triageLines();
  assert.ok(first > 0);
  await crash(ctx, exit, { agoMs: 4000 });
  assert.equal(dialogs.length, 1, "no second dialog for the same streak");
  assert.equal(triageLines(), first, "and no second wall of text either");
});

test("a KeyError crash keeps its specific settings-reset offer", async () => {
  const { ctx, dialogs, exit } = load({
    tail: "Traceback (most recent call last):\n  File \"wgp.py\"\nKeyError: 'video_profile'",
  });
  await crash(ctx, exit);
  assert.equal(dialogs.length, 1);
  assert.match(dialogs[0].title, /Settings file outdated/i);
  assert.match(dialogs[0].message, /video_profile/);
});

test("the ready log re-arms triage for the next launch", async () => {
  const { ctx, dialogs, exit, emitLog } = load();
  await crash(ctx, exit);
  assert.equal(dialogs.length, 1);
  // The backend prints this on every successful boot: it must clear the
  // offered flags so a later, different startup crash is triaged again.
  emitLog("[✓] Wan2GP ready on http://localhost:7860\n");
  assert.equal(ctx.window.__wan2gpServed, true);
  assert.equal(ctx.window._startupCrashOffered, false);
  assert.equal(ctx.window._configCrashOffered, false);
  // Next launch: the exit handler re-arms __wan2gpServed itself, so a crash
  // before the ready line is triaged again.
  await crash(ctx, exit, { agoMs: 3000 });
  assert.equal(dialogs.length, 2);
});

// ── YuE2 metadata crash (launcher #82 = upstream #2517) ──
// The migration reads the settings embedded in each output file, so a
// throwaway settings folder (--config) cannot help: the bad value is inside a
// media file. When the log names that cause, the triage hands over to the
// YuE2 scanner instead of offering a retry that is guaranteed to fail.
const YUE2_TAIL = [
  'Traceback (most recent call last):',
  '  File "D:\\Wan2GP\\Wan2GP\\wgp.py", line 3345, in move_files_to_cache',
  '  File "D:\\Wan2GP\\Wan2GP\\models\\TTS\\yue2\\composition.py", line 19, in resolve_source',
  "ValueError: Choose one of the five YuE2 composition sources.",
].join("\n");

test("a YuE2 metadata crash quarantines instead of offering --config", async () => {
  const seen = [];
  const { ctx, dialogs, exit, clicks, log } = load({
    tail: YUE2_TAIL,
    yue2: async (relaunch) => {
      seen.push(relaunch);
      return true;
    },
  });
  await crash(ctx, exit);
  assert.equal(seen.length, 1, "the signature reaches the YuE2 triage");
  assert.equal(dialogs.length, 0, "no generic retry dialog");
  assert.ok(
    log.some((l) => /did not start/.test(l)),
    "the console still explains the exit",
  );
  // The hook gets the launch button so the quarantine is followed by a relaunch.
  seen[0]();
  assert.equal(clicks.browser, 1);
});

test("a YuE2 signature with a clean output folder still gets the generic retry", async () => {
  const { ctx, dialogs, exit } = load({
    tail: YUE2_TAIL,
    yue2: async () => false,
  });
  await crash(ctx, exit);
  assert.equal(dialogs.length, 1);
  assert.match(dialogs[0].title, /crashed on startup/i);
});

test("an unrelated crash never scans the outputs", async () => {
  let called = 0;
  const { ctx, dialogs, exit } = load({
    tail: "RuntimeError: CUDA out of memory. Tried to allocate 2.00 GiB",
    yue2: async () => {
      called += 1;
      return true;
    },
  });
  await crash(ctx, exit);
  assert.equal(called, 0);
  assert.equal(dialogs.length, 1, "normal startup triage is untouched");
});

// ── Kernel-wheel crashes (launcher #84 class) ──
// An optional wheel built for another compute capability kills the process at
// the first kernel launch. A throwaway settings folder cannot help, so the
// triage offers the uninstall instead of --config.
const NO_KERNEL_TAIL = [
  "Traceback (most recent call last):",
  '  File "/home/mk/Wan2GP/wgp.py", line 44, in <module>',
  "RuntimeError: CUDA error: no kernel image is available for execution on the device",
].join("\n");

test("a no-kernel-image crash offers to remove WGPKernels and relaunch", async () => {
  const { ctx, dialogs, exit, removed, clicks, log } = load({
    choice: "ok",
    tail: NO_KERNEL_TAIL,
  });
  await crash(ctx, exit);
  assert.equal(dialogs.length, 1);
  assert.match(dialogs[0].title, /kernel wheel does not match/i);
  assert.match(dialogs[0].message, /wgpkernels/);
  assert.deepEqual(removed, ["wgpkernels"]);
  assert.equal(clicks.browser, 1, "removal is followed by a relaunch");
  assert.ok(log.some((l) => /Removed wgpkernels/.test(l)));
  // The empty-settings retry is the wrong tool: it must stay unarmed.
  assert.notEqual(ctx.window.__safeConfigNextLaunch, true);
});

test("a stale SpargeAttn import offers to remove spas_sage_attn", async () => {
  const { ctx, dialogs, exit, removed } = load({
    choice: "ok",
    tail:
      "importlib.metadata.PackageNotFoundError: No package metadata was found for sageattention",
  });
  await crash(ctx, exit);
  assert.equal(dialogs.length, 1);
  assert.deepEqual(removed, ["spas_sage_attn"]);
});

test("declining the removal falls back to the generic startup triage", async () => {
  const { ctx, dialogs, exit, removed } = load({
    tail: NO_KERNEL_TAIL,
    // Decline the uninstall, accept the settings retry.
    choice: (o) => (/kernel wheel/i.test(o.title) ? "cancel" : "ok"),
  });
  await crash(ctx, exit);
  assert.equal(removed.length, 0);
  assert.equal(dialogs.length, 2, "wheel offer, then the settings-retry offer");
  assert.match(dialogs[1].title, /crashed on startup/i);
  assert.equal(ctx.window.__safeConfigNextLaunch, true);
});

test("a failed uninstall is reported instead of pretending a relaunch fixed it", async () => {
  const { ctx, dialogs, exit, removed, clicks, log } = load({
    choice: "ok",
    tail: NO_KERNEL_TAIL,
    uninstallOk: false,
  });
  await crash(ctx, exit);
  assert.equal(dialogs.length, 1);
  assert.deepEqual(removed, ["wgpkernels"]);
  assert.equal(clicks.browser, 0, "no blind relaunch after a failed removal");
  assert.ok(log.some((l) => /Could not remove wgpkernels/.test(l)));
});

// ── Apple Silicon MPS blockers (upstream #2545/#2552/#2554/#2014) ──
const MPS_TAIL = [
  "Traceback (most recent call last):",
  '  File "/Users/m/Wan2GP/mmgp/pinned.py", line 55, in register',
  "AttributeError: 'NoneType' object has no attribute 'cudaHostRegister'",
].join("\n");

test("an MPS-only crash names the upstream bug and offers no repair", async () => {
  const { ctx, dialogs, exit, removed, log } = load({
    choice: "ok",
    tail: MPS_TAIL,
    platform: "darwin",
  });
  await crash(ctx, exit);
  assert.equal(dialogs.length, 1);
  assert.match(dialogs[0].title, /Apple Silicon/i);
  assert.match(dialogs[0].message, /#2554/);
  assert.match(dialogs[0].detail, /No settings reset, repair, or reinstall/);
  assert.equal(removed.length, 0);
  assert.equal(ctx.window.__safeConfigNextLaunch, undefined);
  assert.ok(log.some((l) => /Apple Silicon MPS bug/.test(l)));
});

test("the MPS signatures stay inert off macOS", async () => {
  const { ctx, dialogs, exit } = load({ tail: MPS_TAIL, platform: "win32" });
  await crash(ctx, exit);
  assert.equal(dialogs.length, 1);
  assert.match(dialogs[0].title, /crashed on startup/i);
});

test("a rejected pip uninstall is reported, not swallowed as a triage failure", async () => {
  const { ctx, dialogs, exit, removed, clicks, log } = load({
    choice: "ok",
    tail: NO_KERNEL_TAIL,
    uninstallOk: "throws",
  });
  await crash(ctx, exit);
  assert.equal(dialogs.length, 1);
  assert.deepEqual(removed, ["wgpkernels"]);
  assert.equal(clicks.browser, 0);
  assert.ok(log.some((l) => /Could not remove wgpkernels: pip uninstall wgpkernels failed/.test(l)));
  assert.ok(!log.some((l) => /triage failed/.test(l)), "a pip failure is not a triage bug");
});

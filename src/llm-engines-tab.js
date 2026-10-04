// llm-engines-tab.js — guided LLM engine cards (Deepy Prime setup).
//
// Extracted from app.js as a pure move: every symbol here is already a global
// ($, getLLMEngines, showToast, window.w2gp), so this script needs no imports.
// Loaded AFTER app.js in index.html — every call site is inside a deferred
// callback, never a top-level statement, so there is no TDZ hazard.

// ── Guided LLM engine setup (Deepy Prime) ──
// Renders ONE generic card per engine returned by the llm_engines_list Rust
// command (features.rs). The card shows live ✓/✗ status for the CLI and/or pip
// bridge, plus a one-click installer (pip for Claude Code, npm for Codex/OpenCode)
// and, for engines with a server (OpenCode), a Start/Stop server toggle. New
// engines = one entry in that Rust list — no UI branch.
async function refreshLLMEngines() {
  const list = $("llmEnginesList");
  if (!list) return;
  let data;
  try {
    data = await getLLMEngines();
  } catch {
    data = { engines: [] };
  }
  const engines = (data && data.engines) || [];
  if (!engines.length) {
    list.innerHTML =
      '<div class="spec-row"><span class="spec-value">No LLM engines available — reload the Dashboard or check the logs.</span></div>';
    return;
  }
  list.textContent = "";
  const specDot = (on) => {
    const s = document.createElement("span");
    s.className = on ? "spec-dot dot-ok" : "spec-dot dot-bad";
    return s;
  };
  const engineBtn = (cls, label, engineId, extra) => {
    const b = document.createElement("button");
    b.className = "pip-install-btn " + cls;
    b.dataset.engine = engineId;
    b.textContent = label;
    if (extra) for (const k of Object.keys(extra)) b.dataset[k] = extra[k];
    return b;
  };
  const hintDiv = (text, color) => {
    const d = document.createElement("div");
    d.className = "pip-advanced-hint";
    if (color) d.style.color = color;
    d.textContent = text;
    return d;
  };
  for (const e of engines) {
    const card = document.createElement("div");
    card.className = "llm-engine-card";
    const head = document.createElement("div");
    head.className = "llm-engine-head";
    const title = document.createElement("span");
    title.className = "llm-engine-title";
    title.textContent = e.label;
    head.append(title);
    if (e.install && e.install.mode === "pip") {
      const done = e.pipInstalled;
      head.append(
        engineBtn(
          "llm-install-btn",
          (done ? "Reinstall " : "Install ") + e.install.spec,
          e.id,
        ),
      );
      if (done) head.append(engineBtn("llm-remove-btn", "Remove", e.id));
    } else if (e.install && e.install.mode === "npm") {
      const done = e.cliOnPath;
      head.append(
        engineBtn(
          "llm-install-btn",
          (done ? "Reinstall via npm (" : "Install via npm (") +
            e.install.spec +
            ")",
          e.id,
        ),
      );
      if (done) head.append(engineBtn("llm-remove-btn", "Remove", e.id));
    } else if (e.external) {
      const s = document.createElement("span");
      s.className = "spec-value llm-external-hint";
      s.textContent = "External — install via terminal, then it auto-detects.";
      head.append(s);
    }
    card.append(head);
    const specs = document.createElement("div");
    specs.className = "env-specs";
    const specRow = (labelText, dotOn, valueText) => {
      const d = document.createElement("div");
      d.className = "spec-row";
      const lab = document.createElement("span");
      lab.className = "spec-label";
      lab.textContent = labelText;
      const val = document.createElement("span");
      val.className = "spec-value";
      val.textContent = valueText;
      d.append(lab, specDot(dotOn), val);
      return d;
    };
    if (e.cli)
      specs.append(
        specRow(
          e.cli + " CLI",
          e.cliOnPath,
          e.cliOnPath ? "on PATH" : "not found",
        ),
      );
    if (e.pipPackage)
      specs.append(
        specRow(
          e.pipPackage,
          e.pipInstalled,
          e.pipInstalled ? "installed" : "missing",
        ),
      );
    if (e.serverUrl) {
      const d = document.createElement("div");
      d.className = "spec-row";
      const lab = document.createElement("span");
      lab.className = "spec-label";
      lab.textContent = "Server";
      const val = document.createElement("span");
      val.className = "spec-value";
      val.textContent = e.serverUrl;
      d.append(lab, val);
      specs.append(d);
    }
    card.append(specs);
    if (e.serve) {
      const row = document.createElement("div");
      row.className = "llm-serve-row";
      row.append(
        engineBtn(
          "llm-serve-btn",
          e.serverRunning ? "Stop server" : "Start server",
          e.id,
        ),
      );
      card.append(row);
    }
    if (e.auth) {
      // Open the official Claude Code authentication guide (the user asked for a
      // how-to page, not a silent terminal launch that blocks on Max/Pro).
      const row = document.createElement("div");
      row.className = "llm-serve-row";
      row.append(
        engineBtn("llm-auth-btn", "How to sign in", e.id, {
          authDocs: e.auth.docsUrl || "",
        }),
      );
      card.append(row);
    }
    card.append(hintDiv(e.desc));
    if (e.auth) card.append(hintDiv(e.auth.help));
    if (e.claudeApiKeySet)
      card.append(
        hintDiv(
          "✓ Anthropic API key active — Claude Code will use it instead of a Max/Pro login (needs API credits in the Console; billed per use).",
          "#4ADE80",
        ),
      );
    if (e.notes) card.append(hintDiv(e.notes));
    list.append(card);
  }
  list.querySelectorAll(".llm-install-btn").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const id = btn.dataset.engine;
      btn.disabled = true;
      btn.textContent = "installing...";
      const r = await window.w2gp.llmEngineInstall(id);
      if (r && r.success) {
        _llmEnginesPromise = null;
        showToast("✓ engine installed");
        refreshLLMEngines();
      } else {
        btn.disabled = false;
        showToast("✗ " + (r && r.error ? r.error : "install failed"));
      }
    });
  });
  list.querySelectorAll(".llm-remove-btn").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const id = btn.dataset.engine;
      if (!confirm("Remove " + id + " from this machine?")) return;
      btn.disabled = true;
      btn.textContent = "removing...";
      const r = await window.w2gp.llmEngineUninstall(id);
      if (r && r.success) {
        _llmEnginesPromise = null;
        showToast("✓ engine removed");
        refreshLLMEngines();
      } else {
        btn.disabled = false;
        btn.textContent = "Remove";
        showToast("✗ " + (r && r.error ? r.error : "remove failed"));
      }
    });
  });
  list.querySelectorAll(".llm-serve-btn").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const id = btn.dataset.engine;
      const starting = btn.textContent.trim().startsWith("Start");
      btn.disabled = true;
      const r = await window.w2gp.llmEngineServe(
        id,
        starting ? "start" : "stop",
      );
      btn.disabled = false;
      if (r && r.success) {
        btn.textContent = starting ? "Stop server" : "Start server";
        showToast(
          starting ? "✓ " + id + " server started" : "✓ server stopped",
        );
      } else {
        showToast("✗ " + (r && r.error ? r.error : "server action failed"));
      }
    });
  });
  list.querySelectorAll(".llm-auth-btn").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const url = btn.dataset.authDocs;
      if (url) {
        await window.w2gp.openExternal(url);
        showToast("Opened Claude Code authentication guide");
      } else {
        showToast("No sign-in guide configured for this engine");
      }
    });
  });
}

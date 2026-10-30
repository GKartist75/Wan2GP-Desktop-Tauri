# WanGP Guidance — where to click in the launcher

This page is the launcher's map, not a copy of WanGP's manual. Model
inventory, generation settings, prompt syntax and per-model advice live in
WanGP's own docs and change with upstream releases; duplicating them here only
created stale advice. Follow the upstream links below.

## Launcher vs WanGP

| | Launcher | WanGP |
|---|---|---|
| Installs Python, PyTorch, kernels, wheels | Yes | — |
| Starts/stops WanGP, updates it, repairs its env | Yes | — |
| Model selector, generation settings, queue, gallery | Links to it | Owns it |
| `wgp_config.json` writes | Only the controls shown in the launcher | Owns the file and its defaults |
| Model/settings documentation | This short map | Upstream `docs/` |

Pick models **inside WanGP** with its model selector or the toolbar search.
The launcher no longer ships a model catalog, starter settings or per-model
recommendations.

## Where to pick models

1. Start WanGP from the launcher Dashboard.
2. In WanGP, use the model selector or type a model family into the toolbar
   search.
3. Read the model's own description and defaults there. They are versioned
   with the model, unlike any table the launcher could keep.

Upstream reference: [MODELS.md](https://github.com/deepbeepmeep/WanGP/blob/main/docs/MODELS.md).

## Where to pick settings

- **Hardware profiles and kernels:** launcher **Manage → Auto-Tune / Kernel
  wheels**. Auto-Tune only fills the launcher's own recommendation; Apply
  writes the values it shows.
- **Everything else:** WanGP **Configuration** (Performance, General, Deepy,
  Sessions, Plugins). Upstream owns these defaults; the launcher does not seed
  them unless you change a control that writes them.
- **Deepy:** WanGP **Configuration → Deepy**, or the launcher's Deepy card for
  the launcher-owned choices (enable/disable, Zero/Prime, engine, local Qwen
  variant). Deepy tool templates, context size, compaction, KV-cache
  quantization and VRAM mode stay on upstream's defaults until you change them
  in WanGP.
- **Step skipping:** TeaCache is no longer a launch flag. Set
  `skip_steps_cache_type` and `skip_steps_multiplier` in WanGP's **Advanced
  Mode → Steps Skipping**.

Upstream references:
[SETTINGS.md](https://github.com/deepbeepmeep/WanGP/blob/main/docs/SETTINGS.md),
[DEEPY.md](https://github.com/deepbeepmeep/WanGP/blob/main/docs/DEEPY.md).

## Launcher screens

| Screen | What lives there |
|---|---|
| Dashboard | Start/stop, update, Deepy Web, live metrics, backend chips |
| Manage | Installer, packages, kernels, settings, logs, backups, tools |
| Deepy / Deepy Web | Enable Deepy, pick engine/model variant, web UI and phone URL |
| Launch | Extra `wgp.py` arguments and named presets |

Every button is documented in [USER-GUIDE.md](USER-GUIDE.md). Prompt starters
and pre-flight notes are in the [prompt reference](USER-GUIDE.md#prompt--workflow-reference);
model choice stays in WanGP's selector.

## Upstream documentation index

| Topic | Upstream file |
|---|---|
| First start | [GETTING_STARTED.md](https://github.com/deepbeepmeep/WanGP/blob/main/docs/GETTING_STARTED.md) |
| Install and prerequisites | [INSTALLATION.md](https://github.com/deepbeepmeep/WanGP/blob/main/docs/INSTALLATION.md), [AMD-INSTALLATION.md](https://github.com/deepbeepmeep/WanGP/blob/main/docs/AMD-INSTALLATION.md) |
| Models | [MODELS.md](https://github.com/deepbeepmeep/WanGP/blob/main/docs/MODELS.md) |
| Settings | [SETTINGS.md](https://github.com/deepbeepmeep/WanGP/blob/main/docs/SETTINGS.md) |
| Prompts and sliding windows | [PROMPTS.md](https://github.com/deepbeepmeep/WanGP/blob/main/docs/PROMPTS.md) |
| VACE / edit / replace | [VACE.md](https://github.com/deepbeepmeep/WanGP/blob/main/docs/VACE.md) |
| Post-processing | [PROCESSING.md](https://github.com/deepbeepmeep/WanGP/blob/main/docs/PROCESSING.md) |
| LoRAs / finetunes | [LORAS.md](https://github.com/deepbeepmeep/WanGP/blob/main/docs/LORAS.md), [FINETUNES.md](https://github.com/deepbeepmeep/WanGP/blob/main/docs/FINETUNES.md) |
| Deepy | [DEEPY.md](https://github.com/deepbeepmeep/WanGP/blob/main/docs/DEEPY.md) |
| Remote LLMs | [REMOTE_LLMS.md](https://github.com/deepbeepmeep/WanGP/blob/main/docs/REMOTE_LLMS.md) |
| Workspaces / sessions | [WORKSPACES.md](https://github.com/deepbeepmeep/WanGP/blob/main/docs/WORKSPACES.md) |
| Plugins | [PLUGINS.md](https://github.com/deepbeepmeep/WanGP/blob/main/docs/PLUGINS.md) |
| Agent API / MCP | [API.md](https://github.com/deepbeepmeep/WanGP/blob/main/docs/API.md) |
| CLI flags | [CLI.md](https://github.com/deepbeepmeep/WanGP/blob/main/docs/CLI.md) |
| Network protection | [AUTHENTICATION.md](https://github.com/deepbeepmeep/WanGP/blob/main/docs/AUTHENTICATION.md) |
| Troubleshooting | [TROUBLESHOOTING.md](https://github.com/deepbeepmeep/WanGP/blob/main/docs/TROUBLESHOOTING.md) |

## Troubleshooting fast lane

- Read the launcher's live log before changing settings; it names the failing
  import, kernel or model.
- Stop WanGP before applying launcher settings — it rewrites
  `wgp_config.json` from its own copy.
- Use **Manage → Auto-Tune → Failsafe** if a fresh recommendation crashes.
- Use **Manage → Backups** to restore a previous `wgp_config.json`.
- If the launcher itself will not start, use its safe-mode/repair entries
  before reinstalling.

For WanGP-specific failures, use the upstream troubleshooting guide above.

## Why this file is short

The launcher used to mirror model names, steps, guidance, VRAM tables and
processor lists. Every upstream release invalidated part of that copy. Those
details now have one owner: WanGP's `defaults/*.json` and `docs/`. This page
only keeps what the launcher uniquely knows — where its own buttons are.

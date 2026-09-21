# TODO — Wan2GP Sep 20-21 (v13.13) launcher work

Scope: `wan2gp-desktop-launcher-tauri` only. No edits to the cloned Wan2GP checkout.

## A. Changes to make

### P0 — correctness
- [x] A1. GGUF floor 1.0.21 → 1.0.22 (`src/services/kernel-resolver.js`, `src-tauri/src/hw.rs:GGUF_FLOOR` + `apply_gguf_override` + tests, `amd.rs` stale probe + test, `troubleshoot.rs` + `install.rs` comments, `index.html`/`app.js` strings). Floor pass-through preserved.
- [x] A2. Latest-wheels proof (`src-tauri/src/install.rs` sync log): HEAD-behind `origin/main` warning (thread + 15s timeout, silent offline). Keep `restore_kernels` pure-upstream + triton no-downgrade.

### P1 — surface new upstream
- [x] B1. Comfy Kitchen 0.2.35: verified auto-install via `pip install -r requirements.txt` on update (`install.rs:4225-4241`); no allowlist/wheel change needed. Added R580+/H3-LTX note in `src/services/install-plan.js`.
- [x] B2. Qwen Image 2.1 guidance: first-use download note + H3 INT8 VAE note + JIT note in `install-plan.js` + `docs/USER-GUIDE.md`.
- [x] B3. `vae_config` labels (`src/services/memory-profile.js`, `src/index.html` dropdown → auto/16GB+/8GB+/6GB+; `src/services/auto-tune.js` comments updated, still recommends Auto).

### P2 — verify + notes
- [x] C1. Deepy Bonsai filename (`src-tauri/src/deepy_web.rs:QWEN38_27B_WEIGHT_FILES` + per-quant test) — added `Ternary-Bonsai-2-27B-Abliterated-PTQ1_0.gguf` per upstream `assets.py:QWEN38_27B_TEXT_GGUF_PTQ1_FILENAME`.
- [x] C2. H3 INT8 VAE note — in USER-GUIDE model-folders bullets; no install change.
- [x] C3. JIT loading — USER-GUIDE bullets; launcher never force-pre-fetches `optional_assets` (verified: no such fetch path).

### P3 — cosmetic/docs
- [x] AUTHENTICATION.md pointer in USER-GUIDE Deepy Web card; `1.0.21`→`1.0.22` strings; restore confirm text already generic (no change needed).
- [x] Plugin view: verified no change needed (catalog reads checkout's `plugins.json`; H3 Director appears after update).
- [x] CHANGELOG `Unreleased` entry.

### Out of scope (no edit)
- [x] `--public-url`/`--auth`/MCP OAuth (done: `deepy_web.rs:179,318,374,936-960`), Python/Torch/CUDA stacks, Triton/Sage/Flash/Sparge/Nunchaku/LightX2V pins, Qwen21/H3/Deepy runtime internals, matanyone/gallery cosmetics, anything inside the Wan2GP checkout.

## B. Where the launcher helps users with this wave
- [x] 1. GGUF 1.0.22 without detective work — Sync upgrades, overview ✓/⚠/✗, Restore reverts.
- [x] 2. No surprise disk usage (Qwen 2.1) — real cost stated + JIT relief noted.
- [x] 3. Driver gate before install (Comfy Kitchen R580+) instead of post-install CUDA failure.
- [x] 4. VAE preset translated (Auto/16GB+/8GB+/6GB+) — avoids forced-untiled 4K jobs.
- [x] 5. Low-VRAM Deepy answer (Bonsai PTQ1 + 1.0.22 + profile guidance).
- [x] 6. Sync vs Restore made safe + HEAD-behind warning.
- [x] 7. Auth/proxy as a form (`--auth` via env + `--public-url` validation + docs link).
- [x] 8. Sane starting points (Qwen defaults, H3 Director surfaced in-app).

Verified: `cargo check` + `cargo test --offline` green (159 passed); JS floor logic spot-checked via node (1.0.14/1.0.21→1.0.22, 1.0.22+ passthrough).

## D. Release v0.7.3 (Sep 21)
- [x] Versions 0.7.3 (Cargo.toml, tauri.conf.json, package.json, Cargo.lock), CHANGELOG + README finalized; commit `0eeed7d` → PR #33 → merged → tag `v0.7.3` pushed.
- [x] Release build + GitHub release v0.7.3 (NSIS + MSI, unsigned — no signing key in env) marked Latest; `latest.json` published without signature (in-app auto-update will report a verification error until a signed one is published; manual download works).

## C. Follow-up from live test (Sep 21)
- [x] Pin diff false positive: identical files with onnxruntime-gpu's two `python_version` branches reported `1.25.0.dev… -> 1.22.0`. Fixed with marker-aware parsing (`pin_marker_applies`, `parse_requirement_pins_for`, `env_python_version`), last-wins dedupe in `diff_requirement_pins`, same filtering in dep recheck; 3 new unit tests. Full suite green.
- [x] Memory panel kernel settings: replaced deleted legacy `enable_int8_kernels` toggle with `int8_kernels` + `kernel_precision` dropdowns (exact upstream CHOICES verified against `int8_backend.py`/`kernel_policy.py`), fail-closed apply validation, legacy-key cleanup on write + display mapping on read, auto-tune seeds `auto`/`fast`. Full suite green (161 passed).
- [x] Deepy panel quant selector: `prompt_enhancer_quantization` dropdown (Q4/IQ3_S/Q2/Bonsai PTQ1 for Qwen3.8, Quanto/GGUF for Qwen3.5 — exact upstream `QWEN38_QUANTIZATION_CHOICES`), shown only with a local Qwen engine, normalize-to-default on mismatch, preserve-on-absent; `deepy_set`/`deepy_status` extended, roundtrip + pure unit tests. Full suite green (162 passed).
- [x] Bonsai companions: choosing `gguf_ptq1` also writes `deepy_kv_cache_quantization=int8`; Deepy applies (zero or prime) write `enhancer_mode=0` (Automatic prompting — the one global switch upstream offers; template flags untouched); hint announces it; roundtrip asserts. Full suite green (164 passed). (Superseded by §E below — button is now the default, choice preserved in every mode.)

## E. Prompt-enhancement card (Sep 21, post-0.7.3)
- [x] Standalone Prompt enhancement card (below Active Environment, above Deepy): Enhancement UI picker — Enhance Prompt button (default) vs Automatic dropdown — with its own Apply; both Apply buttons share one `applyDeepy` write, enable/disable together, report into their own card; dropdown changes now enable Apply (radios only before).
- [x] Qwen unlocked for Deepy Disabled: all 5 local models selectable (was Florence-only, Qwen grayed out); quant selector shows for Disabled+Qwen; backend disabled arm accepts 1–5, quant applies for Disabled/Zero on 3/4/5; Zero's Qwen-only guard unchanged.
- [x] `enhancer_mode` default is now the button (`1`): explicit 0/1 wins, existing config preserved, missing key → 1, written in every mode (Disabled included — was forced `0` for zero/prime, preserved for disabled); Deepy Web override + auto-config paths now preserve instead of forcing Automatic.
- [x] Disabled apply message no longer says to click "Ask Deepy" (mode-specific suffixes).
- [x] Docs: README Deepy section + What's New, CHANGELOG Unreleased, USER-GUIDE Deepy/Prompt-enhancement cards. Full suite green (164 passed).
- [x] Upstream PR deepbeepmeep/Wan2GP#2355 (fork branch `prompt-enhancer-button-and-deepy-defaults` in `D:\DEVELOPMENT\WAN2GP TOOLS\Wan2GP-PR`, your `C:\Wan2GP` install untouched): always-visible Enhance Prompt button (wgp.py one-liner) + explicit `prompt_enhancer` flags in all 29 Deepy built-in templates (`TI` for image-conditioned, `T` otherwise; graceful via `normalize_choice`).
- [x] Startup drift sync: new `dep_check` command reusing the post-update pin comparison; dashboard reconciles the drift banner with the live env once at startup (stale banner clears, real drift names itself); `showDriftBanner` hardened against empty lists. Full suite green (162 passed).
- [x] Post-install override sync: after successful `setup.py`, install swaps only the GGUF/sage wheels it overrides (pure `post_install_override_urls` + warn-only runner, honors sageSafe, `setup.py` untouched) so fresh installs don't land stale. Full suite green (164 passed).

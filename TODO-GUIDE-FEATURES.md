# TODO — WanGP guidance + launcher features

Combined plan from the docs audit (`docs/WAN2GP-GUIDE.md`) and the launcher
feature overview. Branch: `feature/docs-launcher-coverage`.
No runtime code touched yet — docs only.

## A. Guidance doc (`docs/WAN2GP-GUIDE.md`)

- [x] A1. Companion guide created: goal flowchart, 7 guided paths, launcher-vs-WanGP map
- [x] A2. Complete model inventory — all 232 `defaults/*.json` ids grouped by family
- [x] A3. Complete settings reference — all 116 `models/_settings.json` keys grouped + code notes
- [x] A4. Complete processor inventory — 9 spatial / 2 temporal / 6 audio handlers + methods
- [x] A5. Complete CLI reference — all 44 flags from `cli_args.py` + auth module (deduped)
- [x] A6. New sections: install prerequisites, finetune details, DLSS5, plugins, API+MCP, network protection, changelog pointers
- [x] A7. Code-comment enrichment (`auto-tune.js`, `memory-profile.js`, `kernel-resolver.js`, `install-plan.js`, `deepy-config.js`, `llm-engines.js`, `lora_paths.py`, `attention.py`)
- [x] A8. All 24 upstream `docs/` files + root README covered (see coverage table in chat)
- [x] A9. Review pass: fix Contents anchors, proofread, confirm Mermaid renders on GitHub
- [x] A10. Link from `README.md` + `docs/USER-GUIDE.md` so users find the guide
- [ ] A11. Decide: commit guide (`git add docs/WAN2GP-GUIDE.md`) or drop branch
- [x] A12. Revert `src-tauri/Cargo.toml` line-ending noise (`git checkout -- src-tauri/Cargo.toml`)

## B. Launcher features — P0 (build first)

- [ ] F1. Model recommender — goal picker → 2–3 `model_type` + distilled default + VRAM/ckpt notes; read-only over `defaults/*.json` + `profiles/`
- [ ] F2. Launch-args builder + presets — `Manage → Launch` form for attention/profile/teacache/compile/preload/fp16/server/LoRA/folder flags; named presets + emergency `sdpa/P4/fp16`
- [ ] F3. Diagnostics bundle — one-click support ZIP (`nvidia-smi`, torch/CUDA, triton/sage, driver, disk, `boot.log`, redacted config); clear `.triton`, kill `:7860`, port picker
- [ ] F4. LoRA + finetune librarians — browse `loras_root/<family>/`, URL recovery via `loras_url_cache_v2.json`, `.lset` import/export, accelerator reminders; finetune list/import/export/backup + Refresh trigger

## C. Launcher features — P1 (build second)

- [ ] F5. Prompt starter pack + window validator — templates (text/i2v/edit/dialogue/lyrics), macro expander, `G/PG/W/PW/FG` explainer, offline `[/...]` + `L/X` validator
- [ ] F6. Headless queue runner — `Manage → Batch`: pick queue.zip/JSON → `--dry-run` → `--process` with progress + exit codes → `--output-dir`
- [ ] F7. Workspace backup + disk usage — per-workspace size, backup ZIP, `archive_protected` toggle, storage bar
- [ ] F8. Deepy setup checker + network panel — binary detection, `claude-agent-sdk==0.1.66` pin check, Refresh/catalog lifecycle, cert picker, `--public-url` validator, port/firewall check

## D. Launcher features — P2 (polish, build last)

- [ ] F9. VACE + upsampler explainers — pre-flight checklist, reference-role explainer, post tables with requirement labels
- [ ] F10. API/MCP connector helper — copy-paste launch strings, connection URL, filesystem-read warning, docs links
- [ ] F11. Settings safety — `wgp_config.json` snapshot/diff/restore on every Apply; upstream + launcher changelog viewer

## E. Non-goals

- [ ] No generation UI clone, no checkout edits, no handler reimplementation, no enhancer rewrite. Launcher configures, verifies, explains — WanGP generates.

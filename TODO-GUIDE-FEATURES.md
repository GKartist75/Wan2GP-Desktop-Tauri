# TODO — WanGP guidance + launcher features

Combined plan from the docs audit (`docs/WAN2GP-GUIDE.md`) and the launcher
feature overview. Branch: `feature/docs-launcher-coverage`.
P0 (F1–F4) implemented and committed locally — nothing pushed.

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
- [x] A11. Committed locally (guide + todo + F1–F4); push only after user tests
- [x] A12. Revert `src-tauri/Cargo.toml` line-ending noise (`git checkout -- src-tauri/Cargo.toml`)

## B. Launcher features — P0 (build first)

- [x] F1. Model recommender — Manage → Guide: goal picker → curated picks (23 ids verified in `defaults/`), starters, copy-model-id
- [x] F2. Launch-args builder + presets — Manage → Launch: Balanced/Low VRAM/Max perf/Emergency presets + token-aware attention/profile/teacache/compile/fp16 patcher, existing Save flow
- [x] F3. Diagnostics bundle — report ZIP now adds torch/CUDA + triton/sage probes, staged launchArgs, redacted `wgp_config.json`; 3 new unit tests (171 green)
- [x] F4. LoRA + finetune librarians — Manage → Library: per-family files/size/URL-known table, finetune list/import/delete/export; 5 new backend commands + 4 unit tests

## C. Launcher features — P1 (build second)

- [x] F5. Prompt starter pack + window validator — Guide → Prompt tools: 6 templates with line modes, copy button, offline `[/...]` checker (7 node cases green)
- [ ] F6. Headless queue runner — `Manage → Batch`: pick queue.zip/JSON → `--dry-run` → `--process` with progress + exit codes → `--output-dir`
- [ ] F7. Workspace backup + disk usage — per-workspace size, backup ZIP, `archive_protected` toggle, storage bar
- [ ] F8. Deepy setup checker + network panel — binary detection, `claude-agent-sdk==0.1.66` pin check, Refresh/catalog lifecycle, cert picker, `--public-url` validator, port/firewall check

## D. Launcher features — P2 (polish, build last)

- [ ] F9. VACE + upsampler explainers — pre-flight checklist, reference-role explainer, post tables with requirement labels
- [ ] F10. API/MCP connector helper — copy-paste launch strings, connection URL, filesystem-read warning, docs links
- [ ] F11. Settings safety — `wgp_config.json` snapshot/diff/restore on every Apply; upstream + launcher changelog viewer

## E. Non-goals

- [ ] No generation UI clone, no checkout edits, no handler reimplementation, no enhancer rewrite. Launcher configures, verifies, explains — WanGP generates.

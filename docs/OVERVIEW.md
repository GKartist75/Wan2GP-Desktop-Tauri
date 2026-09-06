# Wan2GP Desktop Launcher — System Overview

How the launcher, upstream Wan2GP, and the machine fit together. For the
user-facing feature list see [README.md](../README.md); for per-version
history see [CHANGELOG.md](../CHANGELOG.md).

## Architecture

```mermaid
flowchart TB
    subgraph Launcher["Tauri launcher (this repo)"]
        UI["Dashboard UI\n(src/: app.js, w2gp.js, services/)"]
        BE["Backend (src-tauri/src/)\ninstall · launch · status · features\nplugins · config · hw · updates"]
        UI <-->|invoke / events| BE
    end
    subgraph Machine["Windows machine"]
        PREREQ["Prerequisites\ngit · uv · python · conda"]
        ENV["Environments\n(base = repo folder)"]
        WAN2GP["Wan2GP (upstream git)\nwgp.py + setup.py + setup_config.json"]
        MODELS["Models & outputs\nckpts · loras · outputs"]
    end
    subgraph Upstream["deepbeepmeep/Wan2GP (origin/main)"]
        UPSTREAM_REPO["repo · setup.py\nsetup_config.json\nscripts/install_dlss5.ps1"]
    end
    BE -->|winget / official installers| PREREQ
    BE -->|git clone| UPSTREAM_REPO
    BE -->|setup.py install --env uv/venv/conda| ENV
    ENV --> WAN2GP
    WAN2GP --> MODELS
    UI -->|iframe embed\nlocalhost:7860/7861| WAN2GP
```

Trust rule: **upstream owns** the profile→package mapping (`setup.py`,
`setup_config.json`) and all model/runtime code. **The launcher owns**
detection, provisioning, honesty (exit codes, smoke tests), and lifecycle
(start/stop/update). The launcher mirrors upstream data, never overrides it —
except where upstream lags its own docs (GGUF 1.0.21) or declares data nothing
consumes (HSA override); both are documented at the call site and auto-follow
upstream flips.

## Install pipeline (fresh Install button)

```mermaid
flowchart TD
    A["classify_target()\nempty / healthy / broken / repo-only /\nPinokio / foreign"] --> B{"verdict?"}
    B -->|"not empty/clean"| STOP1["Abort with guidance\n(reuse · repair · wipe · move models)"]
    B -->|"empty"| DISK["Disk gate: <10 GB free → abort\nbefore downloading anything"]
    DISK --> CLONE["git clone --depth 1"]
    CLONE --> MARKER{"marker + torch probe?\n.wan2gp-install-ok"}
    MARKER -->|"verified"| REUSE["Return success\n(no re-download)"]
    MARKER -->|"incomplete env"| WIPE["Remove env dir\n(uv cache survives)"]
    WIPE --> UV["ensure_uv_python()\nexact pin · 3.11.14"]
    UV --> SETUP["setup.py install --env --auto\n(max 2 attempts;\nauto-retry on network blips)"]
    SETUP -->|"exit ≠ 0"| FAIL["Honest error + hint\n+ Retry button"]
    SETUP -->|"exit 0"| SMOKE["Smoke test\nimport torch · CUDA visible?"]
    SMOKE -->|"fail"| FAIL
    SMOKE -->|"pass"| DONE["Write marker\nfavourite plugins\nInstallation complete"]
```

Key properties: no step reports success it didn't earn; every failure names
the failing command plus a copy-paste fix; Retry never re-downloads a finished
install and never resumes into a half-built env.

## Kernel sync (Manage → Sync Kernels)

```mermaid
flowchart LR
    CFG["setup_config.json\n(local, else origin/main)"] --> PROF["gpu_profiles[RTX_20..50]\nkernels: nunchaku_cu13 · gguf · light2xv"]
    PROF --> URLS["components.*.cmd.win wheel URLs"]
    URLS --> SWAP["Overrides (documented)\nSage post4→post6 (safe toggle)\nGGUF 1.0.14→1.0.21 (docs lead)"]
    SWAP --> PIP["pip install --upgrade\nper kernel, failures named"]
    PIP --> OVERVIEW["Dashboard overview\nwant vs installed ✓/⚠/✗"]
```

## Update flow (launcher itself)

```mermaid
flowchart LR
    CHECK["check_update\n(updater plugin)"] --> DL["download_update\nprogress %"]
    DL --> INST["install_update\npassive NSIS swap"]
    INST --> REL["GitHub release (public)\nsetup.exe + .sig + msi + latest.json"]
```

Manual-only: check emits `available{autoDownload:false}`; nothing downloads
or installs without the user's explicit Full Download → Install & Restart.

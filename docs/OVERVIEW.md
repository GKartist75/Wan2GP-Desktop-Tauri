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
except where upstream lags its own docs (GGUF 1.0.21), declares data nothing
consumes (HSA override), or ships a template that breaks its own installs
(conda `conda run` re-quoting corrupts wheel URLs → direct-pip patch); all
are documented at the call site, drift-refusing, and auto-follow upstream flips.

Tooling rule: **never depend on PATH.** uv lives owned in `<dataDir>\.tools`
(self-updating receipted copy); env interpreters, git/conda/python resolve to
absolute known locations with PATH only as fallback. Fresh prerequisite
installs are usable instantly — no restart.

## Install pipeline (big Install button — the ONLY launcher)

User-facing version with screenshots-level detail: [USER-GUIDE.md](USER-GUIDE.md).

```mermaid
flowchart TD
    S["START: big Install button"] --> V{"classify_target()\nempty / repo-no-env /\nbroken / healthy /\nPinokio / foreign"}
    V -- "Empty" --> C0["No choice"]
    V -- "Repo, no working env" --> C1["Checklist\nRepair* / Fresh"]
    V -- "Healthy install" --> C2["Trio\nUpdate* / Fresh / Use existing"]
    V -- "Pinokio" --> BLK["BLOCKED\n(reuse models elsewhere)"]
    V -- "Foreign" --> WRN["Warning\nInstall anyway / Browse"]
    C0 --> G["Single CONFIRM\nchoice + env + location"]
    C1 --> G
    C2 --> G
    WRN --> G
    G -- "Fresh?" --> M["Backup modal\ncollect only, never launches"]
    M --> G
    G -- "Cancel" --> V
    G -- "OK" --> CL["git clone (or reuse)"]
    CL --> MK{"marker + torch probe?\n.wan2gp-install-ok"}
    MK -- "verified" --> REUSE["Return success\n(no re-download)"]
    MK -- "broken / absent" --> P{"env?"}
    P -- "uv" --> BU["owned uv (.tools)\nexact Python pin"]
    P -- "venv" --> PY["py-3.11 shim if needed"]
    P -- "conda" --> CA["ToS accept +\nsystem-python drive (fresh)\nconda run (exists)"]
    BU --> SE["setup hook (or legacy setup.py)\nprofile + VRAM + conda-pip\nvia module import"]
    PY --> SE
    CA --> SE
    SE -->|"exit != 0"| FAIL["Honest error + hint\n+ Retry button"]
    SE -->|"exit 0"| SM["Smoke test\nimport torch + CUDA\n(all env types)"]
    SM -->|"fail"| FAIL
    SM -->|"pass"| AMD{"AMD?"}
    AMD -- "yes" --> PR["Compute probe, both HSA modes\nrecord winner / reseat"]
    AMD -- "no" --> DN["Write marker\nfavourite plugins\nInstallation complete"]
    PR --> DN
```

Key properties: the big Install button is the only launcher (radios and
modals never start work); one adaptive confirm per run; no step reports
success it didn't earn; every failure names the failing command plus a
copy-paste fix; Retry never re-downloads a finished install and never resumes
into a half-built env; cancelling anywhere returns to the ticked choice.

## Verify (System → Troubleshooting)

```mermaid
flowchart LR
    T["Verify GPU compute"] --> I["import torch +\nCUDA visible?"]
    I --> K["import each wheel\n(top-level modules)"]
    K --> R["names the broken dist\nor all-green"]
```

Version-present is not loadable (AV quarantine, wrong-torch ABIs) — Verify
proves imports and names the culprit. Run after AV restores, driver updates,
or env surgery.

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

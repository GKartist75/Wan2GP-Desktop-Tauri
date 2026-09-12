# Isolated SageAttention3 probe — keeps the desktop launcher OUT.
#
# What it does:
#   1. Creates a scratch venv under D:/tmp/sage3-test (NOT the launcher-managed
#      Wan2GP clone at C:/Wan2GP, NOT env_uv/env_venv/env_conda, no uv, no conda).
#   2. Installs torch 2.10.0/cu130 for Python 3.13 + the unofficial
#      sageattn3 cp313 wheel (ussoewwin/Sage-Attention-for-Windows).
#   3. Runs an import + capability probe and writes results to results.txt.
#
# What it NEVER touches: C:/Wan2GP, D:/Wan2GP, setup_config.json, install
# markers, launcher-owned uv, conda envs, or this repo's own env.
#
# Usage (PowerShell):
#   powershell -ExecutionPolicy Bypass -File scripts/test-sage3-isolated.ps1
# Cleanup:
#   Remove-Item -Recurse D:\tmp\sage3-test
param(
  [string]$ScratchDir = "D:\tmp\sage3-test",
  [string]$WheelUrl = "https://huggingface.co/ussoewwin/Sage-Attention-for-Windows/resolve/main/sageattn3-1.0.0+cu130torch2.10.0-cp313-cp313-win_amd64.whl"
)

$ErrorActionPreference = "Stop"
$log = Join-Path $ScratchDir "results.txt"

function Log([string]$msg) {
  $line = "[$(Get-Date -Format 'HH:mm:ss')] $msg"
  Write-Host $line
  Add-Content -Path $log -Value $line
}

# --- guard rails: refuse to run inside launcher-managed locations ---
$forbidden = @("C:\Wan2GP", "D:\Wan2GP")
foreach ($f in $forbidden) {
  if ($ScratchDir.StartsWith($f, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing: ScratchDir $ScratchDir is inside launcher-managed $f"
  }
}
$repoRoot = Split-Path -Parent $PSScriptRoot
if ($ScratchDir.StartsWith($repoRoot, [StringComparison]::OrdinalIgnoreCase)) {
  throw "Refusing: ScratchDir $ScratchDir is inside the repo $repoRoot"
}

New-Item -ItemType Directory -Force -Path $ScratchDir | Out-Null
if (Test-Path $log) { Remove-Item $log }
Log "=== Sage3 isolated probe ==="
Log "scratch : $ScratchDir"
Log "wheel   : $WheelUrl"
Log "gpu     : $((nvidia-smi --query-gpu=name,driver_version --format=csv,noheader))"

$venvPy = Join-Path $ScratchDir "env\Scripts\python.exe"
if (-not (Test-Path $venvPy)) {
  Log "[*] creating venv with system Python 3.13 (py launcher, no uv/conda)..."
  py -V:3.13 -m venv (Join-Path $ScratchDir "env")
} else {
  Log "[*] reusing existing scratch venv"
}
& $venvPy --version | ForEach-Object { Log "venv python: $_" }

Log "[*] installing torch 2.10.0 cu130 (this downloads ~2.5 GB)..."
& $venvPy -m pip install --quiet "torch==2.10.0" --index-url https://download.pytorch.org/whl/cu130 2>&1 |
  ForEach-Object { Log "pip-torch: $_" }

Log "[*] installing triton + numpy (sageattn3 wheel does NOT declare triton as a dependency)..."
& $venvPy -m pip install --quiet triton-windows numpy 2>&1 |
  ForEach-Object { Log "pip-triton: $_" }

Log "[*] installing sageattn3 wheel..."
& $venvPy -m pip install --quiet $WheelUrl 2>&1 |
  ForEach-Object { Log "pip-sage3: $_" }

$probe = @'
import json, sys, traceback
out = {"python": sys.version.split()[0]}
try {
  import torch
  out["torch"] = torch.__version__
  out["cuda_available"] = torch.cuda.is_available()
  try:
    out["gpu"] = torch.cuda.get_device_name(0)
    out["capability"] = ".".join(map(str, torch.cuda.get_device_capability(0)))
  except Exception as e:
    out["cuda_device_error"] = f"{type(e).__name__}: {e}"
except Exception as e:
  out["torch_error"] = f"{type(e).__name__}: {e}"
try:
  import triton
  out["triton"] = triton.__version__
except Exception as e:
  out["triton_error"] = f"{type(e).__name__}: {e}"
try:
    import sageattn3
    out["sageattn3"] = getattr(sageattn3, "__version__", "imported-ok")
    out["sageattn3_file"] = getattr(sageattn3, "__file__", "?")
    out["sageattn3_dir"] = [n for n in dir(sageattn3) if not n.startswith("_")]
    # functional test: small Blackwell forward pass (expect FAIL on pre-Blackwell GPUs)
    try:
        import torch as _t
        _q = _t.randn(1, 8, 128, 64, dtype=_t.bfloat16, device="cuda")
        _k = _t.randn(1, 8, 128, 64, dtype=_t.bfloat16, device="cuda")
        _v = _t.randn(1, 8, 128, 64, dtype=_t.bfloat16, device="cuda")
        _o = sageattn3.sageattn3_blackwell(_q, _k, _v, is_causal=True)
        out["forward"] = f"OK shape={tuple(_o.shape)}"
    except Exception as e:
        out["forward"] = f"FAIL {type(e).__name__}: {str(e)[:300]}"
except Exception as e:
    out["sageattn3_error"] = f"{type(e).__name__}: {e}"
    out["sageattn3_trace"] = traceback.format_exc(limit=5)
print("PROBE_JSON:" + json.dumps(out, indent=1))
'@

Log "[*] running probe..."
# NOTE: $ErrorActionPreference is Stop, but python warnings on stderr arrive
# as ErrorRecords — scope it to Continue or the first NumPy warning kills us.
$prevPref = $ErrorActionPreference; $ErrorActionPreference = "Continue"
& $venvPy -c $probe 2>&1 | ForEach-Object { Log "probe: $_" }
$ErrorActionPreference = $prevPref
Log "=== done. Full log: $log ==="

<#
.SYNOPSIS
  Run the launcher's live (network) tests — the ones plain `cargo test` skips.

.DESCRIPTION
  The default suite must pass with the network unplugged, so the tests that prove
  the real thing end to end are marked `#[ignore]`:

    wheels::tests::live_download_verifies_and_caches
      Downloads the smallest pinned kernel wheel and walks the whole path:
      download -> sha256 -> pin match -> console evidence -> cache hit. It is the
      only test that shows wheel verification works against a real server, i.e.
      the only one that would catch a pin whose asset was re-uploaded.

  `cargo test -- --ignored` is easy to forget — and easy to get subtly wrong,
  since the filter goes before the `--` and the evidence lines only appear with
  `--nocapture`. This wraps it, and runs one test at a time so the evidence is
  readable.

  Run it before a release, or whenever the notes claim wheel verification still
  works. It needs the network and a few MB of disk.

.PARAMETER Filter
  Optional test-name substring, e.g. -Filter wheels.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\live-tests.ps1
  powershell -ExecutionPolicy Bypass -File scripts\live-tests.ps1 -Filter wheels
#>
[CmdletBinding()]
param(
  [string]$Filter = ''
)

# Native commands log to stderr; under 'Stop' PowerShell can turn that into a
# terminating error even when the command succeeded, so the exit code is checked
# explicitly instead.
$ErrorActionPreference = 'Continue'

$root = Split-Path -Parent $PSScriptRoot
$tauri = Join-Path $root 'src-tauri'
$cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
if (-not (Test-Path $cargo)) { $cargo = 'cargo' }
if (-not (Test-Path $tauri)) { throw "src-tauri not found next to scripts\ (looked in $tauri)" }

$cargoArgs = @('test', '--lib')
if ($Filter) { $cargoArgs += $Filter }
$cargoArgs += @('--', '--ignored', '--nocapture', '--test-threads=1')

Write-Output ("live tests (network required) — {0} {1}" -f $cargo, ($cargoArgs -join ' '))
Push-Location $tauri
try {
  & $cargo @cargoArgs
  $code = $LASTEXITCODE
} finally {
  Pop-Location
}
if ($code -ne 0) { throw "live tests failed (exit $code)" }
Write-Output ''
Write-Output 'live tests passed.'

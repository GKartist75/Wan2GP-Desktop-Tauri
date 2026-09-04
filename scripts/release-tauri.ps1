# Release the Tauri launcher: bump version, build + sign, publish GitHub release
# with updater artifacts (latest.json + signed setup.exe).
#
# Usage:  .\scripts\release-tauri.ps1 -Version 0.2.0 [-Notes "..."]
# Requires: $env:TAURI_SIGNING_PRIVATE_KEY_PATH pointing at your .key file
# (or TAURI_SIGNING_PRIVATE_KEY with its contents). Key stays out of the repo.
param([Parameter(Mandatory=$true)][string]$Version, [string]$Notes = "")

$ErrorActionPreference = "Stop"
$Root = Split-Path $PSScriptRoot -Parent
Set-Location $Root

if (-not $env:TAURI_SIGNING_PRIVATE_KEY) {
  # Content var, not _PATH: the native CLI can't resolve non-Windows paths.
  $defaultKey = Join-Path $HOME ".tauri\wan2gp-desktop.key"
  if (Test-Path $defaultKey) { $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content $defaultKey -Raw).Trim() }
  else { throw "Signing key not found. Set TAURI_SIGNING_PRIVATE_KEY first." }
}
if (-not $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) {
  $defaultPwd = Join-Path $HOME ".tauri\wan2gp-desktop.pwd"
  if (Test-Path $defaultPwd) { $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = (Get-Content $defaultPwd -Raw).Trim() }
}

# 1) Bump version (tauri.conf.json drives the updater comparison; keep Cargo in sync)
$confPath = "src-tauri\tauri.conf.json"
$conf = Get-Content $confPath -Raw | ConvertFrom-Json
$conf.version = $Version
$conf | ConvertTo-Json -Depth 10 | Set-Content $confPath
(Get-Content "src-tauri\Cargo.toml" -Raw) -replace '(?m)^version = ".*"', "version = `"$Version`"" |
  Set-Content "src-tauri\Cargo.toml" -NoNewline
git add $confPath "src-tauri\Cargo.toml"
if (git status --porcelain) { git commit -m "release: v$Version" | Out-Null }

# 2) Build (signs updater artifacts automatically via the env key)
npx tauri build
if ($LASTEXITCODE -ne 0) { throw "tauri build failed" }

# 3) Collect updater artifacts (v2 signs the installers directly: setup.exe + .sig)
$setup = Get-ChildItem "src-tauri\target\release\bundle\nsis\*-setup.exe" | Where-Object { $_.Name -like "*$Version*" } | Select-Object -First 1
if (-not $setup) { $setup = Get-ChildItem "src-tauri\target\release\bundle\nsis\*-setup.exe" | Sort-Object LastWriteTime -Descending | Select-Object -First 1 }
if (-not $setup) { throw "No setup.exe found" }
$sig = Get-Content ($setup.FullName + ".sig") -Raw
$sig = $sig.Trim()
$tag = "v$Version"
# GitHub normalizes spaces to dots in asset names on upload (seen in v0.1.3:
# "Wan2GP Desktop Launcher Tauri_...setup.exe" became "Wan2GP.Desktop.Launcher.Tauri_..."),
# so the updater URL must use the normalized name or Full Download 404s.
$assetName = $setup.Name -replace ' ', '.'
$latest = [ordered]@{
  version   = $Version
  notes     = $Notes
  pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
  platforms = [ordered]@{
    "windows-x86_64" = [ordered]@{
      signature = $sig
      url       = "https://github.com/GKartist75/Wan2GP-Desktop-Tauri/releases/download/$tag/$assetName"
    }
  }
}
$latestPath = "src-tauri\target\release\bundle\nsis\latest.json"
$latest | ConvertTo-Json -Depth 6 | Set-Content $latestPath

# 4) Publish (latest.json must be on a published release — /latest/download/ 404s on drafts)
$msi = Get-ChildItem "src-tauri\target\release\bundle\msi\*.msi" | Where-Object { $_.Name -like "*$Version*" } | Select-Object -First 1
if (-not $msi) { $msi = Get-ChildItem "src-tauri\target\release\bundle\msi\*.msi" | Sort-Object LastWriteTime -Descending | Select-Object -First 1 }
# ponytail: gh drops an empty --notes value ("flag needs an argument"), so only pass it when set.
$notesArgs = @()
if ($Notes -and $Notes.Trim()) { $notesArgs = @("--notes", $Notes) }
gh release create $tag $setup.FullName ($setup.FullName + ".sig") $msi.FullName $latestPath `
  --repo GKartist75/Wan2GP-Desktop-Tauri --title $tag @notesArgs
# ponytail: bare `git push` fails on branches without upstream tracking — push explicitly.
$branch = (git branch --show-current).Trim()
if ($branch) { git push -u origin $branch }
Write-Host "Released $tag - updater will pick it up from latest.json" -ForegroundColor Green

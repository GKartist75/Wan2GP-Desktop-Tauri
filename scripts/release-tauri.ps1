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

# 2b) Versioned portable copy next to the unversioned one (local testing).
$portable = "src-tauri\target\release\wan2gp-desktop-launcher-tauri.exe"
if (Test-Path $portable) { Copy-Item $portable "src-tauri\target\release\wan2gp-desktop-launcher-tauri-$Version.exe" -Force }

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
# No BOM, ever: the Tauri updater parses this file with serde_json, and a UTF-8
# BOM makes it fail with "error decoding response body" instead of showing the
# update. `Set-Content -Encoding UTF8` under PowerShell 5.1 writes EF BB BF —
# a v0.10.3 release shipped that way and every client's update check errored.
[System.IO.File]::WriteAllText($latestPath, ($latest | ConvertTo-Json -Depth 6), (New-Object System.Text.UTF8Encoding($false)))
if ([System.IO.File]::ReadAllBytes($latestPath)[0] -eq 0xEF) { throw "latest.json still starts with a BOM - the updater cannot parse it" }

# 4) Tag FIRST, on the commit that was actually built. `gh release create`
# makes the tag at the repo's DEFAULT BRANCH head when the tag does not exist
# yet, so releasing from a feature branch silently tags master's head instead —
# that happened for v0.10.3 (tag landed on 8bdab92, the release was ac20e95).
# Creating it here keeps the tag, the signed assets and the release on one
# commit, and pushes the branch before anything goes public.
$branch = (git branch --show-current).Trim()
if ($branch) { git push -u origin $branch }
if (-not (git tag -l $tag)) { git tag $tag HEAD }
if ((git ls-remote --tags origin "refs/tags/$tag").Trim() -eq "") { git push origin "refs/tags/$tag" }

# 5) Publish (latest.json must be on a published release — /latest/download/ 404s on drafts)
$msi = Get-ChildItem "src-tauri\target\release\bundle\msi\*.msi" | Where-Object { $_.Name -like "*$Version*" } | Select-Object -First 1
if (-not $msi) { $msi = Get-ChildItem "src-tauri\target\release\bundle\msi\*.msi" | Sort-Object LastWriteTime -Descending | Select-Object -First 1 }
# ponytail: gh drops an empty --notes value ("flag needs an argument"), so only pass it when set.
$notesArgs = @()
if ($Notes -and $Notes.Trim()) { $notesArgs = @("--notes", $Notes) }
gh release create $tag $setup.FullName ($setup.FullName + ".sig") $msi.FullName $latestPath `
  --repo GKartist75/Wan2GP-Desktop-Tauri --title $tag @notesArgs
Write-Host "Released $tag - updater will pick it up from latest.json" -ForegroundColor Green

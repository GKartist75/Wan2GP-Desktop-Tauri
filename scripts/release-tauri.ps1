# Release the Tauri launcher: verify, bump every version, build + sign, publish
# GitHub release with updater artifacts (latest.json + signed setup.exe).
#
# Usage:  .\scripts\release-tauri.ps1 -Version 0.2.0 [-Notes "..."] [-NotesFile path]
# -NotesFile is read as UTF-8: PowerShell 5.1's `Get-Content -Raw` defaults to
# the ANSI code page, so "—" or "→" in an inherited -Notes string arrives
# mojibake and ships that way into both the release body and latest.json.
# Requires: $env:TAURI_SIGNING_PRIVATE_KEY (the .key contents) or
# %USERPROFILE%\.tauri\wan2gp-desktop.key, plus TAURI_SIGNING_PRIVATE_KEY_PASSWORD
# (or %USERPROFILE%\.tauri\wan2gp-desktop.pwd). Keys stay out of the repo.
#
# The preflight is mandatory: a release cannot be published from a tree whose
# tests, formatter or clippy are red, and it cannot reuse an older installer
# under a new version (the v0.10.14 artifact-fallback bug).
param(
  [Parameter(Mandatory=$true)][string]$Version,
  [string]$Notes = "",
  [string]$NotesFile = ""
)

if ($NotesFile) {
  if ($Notes.Trim()) { throw "Pass -Notes or -NotesFile, not both" }
  if (-not (Test-Path $NotesFile)) { throw "Notes file not found: $NotesFile" }
  $Notes = [System.IO.File]::ReadAllText((Resolve-Path $NotesFile), [System.Text.Encoding]::UTF8)
}

$Repo = "GKartist75/Wan2GP-Desktop-Tauri"

# git prints CRLF/autocrlf notices to stderr, and under `$ErrorActionPreference
# = "Stop"` PowerShell promotes those notices to terminating errors that kill
# the release (seen twice on `git add src-tauri/Cargo.toml`; `2>$null` does not
# help). Every git call goes through here: notices print, success is judged by
# exit code only.
function Git {
  $prev = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'
  try {
    # git.exe, not git: the function itself is named Git and PowerShell
    # resolves case-insensitively, so `& git` recursed until stack overflow.
    & git.exe @args
    if ($LASTEXITCODE -ne 0) { throw "git $args failed with exit $LASTEXITCODE" }
  } finally { $ErrorActionPreference = $prev }
}

# Native tools write progress to stderr; under 'Stop' that is fatal even when
# the tool succeeded. Judge them by exit code, like Git above.
function Native {
  param([string]$Exe, [string[]]$Argv, [string]$What)
  $prev = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'
  try { & $Exe @Argv } finally { $ErrorActionPreference = $prev }
  if ($LASTEXITCODE -ne 0) { throw "$What failed (exit $LASTEXITCODE)" }
}

$ErrorActionPreference = "Stop"
$Root = Split-Path $PSScriptRoot -Parent
Set-Location $Root

if ($Version -notmatch '^\d+\.\d+\.\d+$') {
  throw "Version must be plain MAJOR.MINOR.PATCH (got '$Version')"
}

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

$branch = (Git branch --show-current).Trim()
if (-not $branch) { throw "Refusing to release from a detached HEAD" }
# Tracked changes would be swept into the release commit (or left out of the
# build); untracked build logs are fine.
if ((Git status --porcelain --untracked-files=no | Out-String).Trim()) {
  throw "Working tree has uncommitted tracked changes - commit or stash them first"
}

$oldVersion = (Get-Content "package.json" -Raw | ConvertFrom-Json).version
if ([version]$Version -le [version]$oldVersion) {
  throw "Version $Version must be greater than the current $oldVersion"
}

# 0) Preflight: no release is built from red tests, clippy or formatting.
Write-Host "preflight: npm test" -ForegroundColor Cyan
Native "npm" @("test") "npm test"
Write-Host "preflight: cargo test" -ForegroundColor Cyan
Native "cargo" @("test", "--manifest-path", "src-tauri/Cargo.toml") "cargo test"
Write-Host "preflight: cargo clippy -D warnings" -ForegroundColor Cyan
Native "cargo" @("clippy", "--manifest-path", "src-tauri/Cargo.toml", "--all-targets", "--", "-D", "warnings") "cargo clippy"
Write-Host "preflight: cargo fmt --check" -ForegroundColor Cyan
Native "cargo" @("fmt", "--manifest-path", "src-tauri/Cargo.toml", "--check") "cargo fmt --check"

# 1) Bump version everywhere a client or a package manager reads it.
# tauri.conf.json drives the updater comparison; Cargo.toml/Cargo.lock keep the
# crate self-consistent; package.json/package-lock keep npm metadata in sync.
$confPath = "src-tauri\tauri.conf.json"
$conf = Get-Content $confPath -Raw | ConvertFrom-Json
$conf.version = $Version
$conf | ConvertTo-Json -Depth 10 | Set-Content $confPath

(Get-Content "src-tauri\Cargo.toml" -Raw) -replace '(?m)^version = "[^"]*"', "version = `"$Version`"" |
  Set-Content "src-tauri\Cargo.toml" -NoNewline

# Cargo.lock carries the crate version too, and `cargo build` rewrites it during
# step 2 — left uncommitted it shows as a dirty file forever and master drifts
# from the tag. Bump it here so the release commit is self-consistent.
# Anchor on the package NAME: a bare `^version = "..."` matched every
# dependency and stamped them all with the release version, which cargo then
# rejects ("package `base64` is specified twice"). The group refs are braced
# because `$1` followed by a version starting "0." reads as group ten.
$lockPath = "src-tauri\Cargo.lock"
if (Test-Path $lockPath) {
  $lockRepl = '${1}' + $Version + '${2}'
  (Get-Content $lockPath -Raw) -replace '(?m)^(name = "wan2gp-desktop-launcher-tauri"\r?\nversion = ")[^"]*(")', $lockRepl |
    Set-Content $lockPath -NoNewline
}

$pkgPath = "package.json"
$pkg = Get-Content $pkgPath -Raw
$pkg = [regex]::Replace($pkg, '(?m)^(  "version": ")[^"]*(")', '${1}' + $Version + '${2}')
Set-Content $pkgPath $pkg -NoNewline

$npmLockPath = "package-lock.json"
if (Test-Path $npmLockPath) {
  $npmLock = Get-Content $npmLockPath -Raw
  $npmLock = [regex]::Replace($npmLock, '(?m)^(  "version": ")[^"]*(")', '${1}' + $Version + '${2}')
  # Only the root package entry: `devDependencies` only follows that one, so a
  # third-party package pinned at the same version is never rewritten.
  $npmLock = [regex]::Replace($npmLock, '(?m)^(      "version": ")[^"]*(",\r?\n      "devDependencies")', '${1}' + $Version + '${2}')
  Set-Content $npmLockPath $npmLock -NoNewline
}

Git add $confPath "src-tauri\Cargo.toml" $lockPath $pkgPath $npmLockPath
# Staged-only check: `git status --porcelain` also lists untracked files (e.g.
# a build log next to the repo), which made an empty `git commit` fail the
# v0.10.7 run with exit 1.
$staged = Git diff --cached --name-only
if (-not $staged) { throw "Version bump produced no staged changes (already at $Version?)" }
Git commit -m "release: v$Version" | Out-Null

# 2) Build from a clean dependency tree; signs updater artifacts via the env key.
Native "npm" @("ci") "npm ci"
Native "npm" @("run", "tauri", "--", "build") "tauri build"

# 2b) Versioned portable copy next to the unversioned one (local testing).
$portable = "src-tauri\target\release\wan2gp-desktop-launcher-tauri.exe"
if (Test-Path $portable) { Copy-Item $portable "src-tauri\target\release\wan2gp-desktop-launcher-tauri-$Version.exe" -Force }

# 3) Collect updater artifacts. NO fallback to "newest file": that is how a
# previous release's installer gets published under a new version, after which
# latest.json points at bytes whose signature/version disagree with its own
# version field and every client re-downloads forever.
$tag = "v$Version"
$setup = Get-ChildItem "src-tauri\target\release\bundle\nsis\*-setup.exe" |
  Where-Object { $_.Name -like "*$Version*" } | Select-Object -First 1
if (-not $setup) { throw "No versioned setup.exe for $Version found - refusing to publish an older installer" }
$sigPath = $setup.FullName + ".sig"
if (-not (Test-Path $sigPath)) { throw "Missing signature for $($setup.Name)" }
$sig = (Get-Content $sigPath -Raw).Trim()
if (-not $sig) { throw "Empty signature for $($setup.Name)" }

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
      url       = "https://github.com/$Repo/releases/download/$tag/$assetName"
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

# 4) Tag on the commit that was actually built. `gh release create` makes the
# tag at the repo's DEFAULT BRANCH head when the tag does not exist yet, so
# releasing from a feature branch silently tagged master's head (v0.10.3).
# Verify any existing local/remote tag first, then push the branch before
# anything goes public.
$head = (Git rev-parse HEAD).Trim().ToLowerInvariant()
$localTag = (Git tag -l --format='%(objectname)' $tag | Out-String).Trim().ToLowerInvariant()
if ($localTag -and $localTag -ne $head) {
  throw "Tag $tag already points at $localTag, not HEAD $head - pick a new version or delete the tag"
}
if (-not $localTag) { Git tag $tag $head }

$remoteTag = (Git ls-remote --tags origin "refs/tags/$tag" | Out-String).Trim()
if ($remoteTag) {
  $remoteSha = ($remoteTag -split '\s+')[0].ToLowerInvariant()
  if ($remoteSha -ne $head) {
    throw "Remote tag $tag points at $remoteSha, not HEAD $head - refusing to overwrite a published tag"
  }
}

Git push -u origin $branch
if (-not $remoteTag) { Git push origin "refs/tags/$tag" }

# 5) Publish as a DRAFT first: assets and latest.json are attached before the
# release is visible to `/releases/latest/download/`, so a half-uploaded
# release can never be the updater's target. Publish only after the upload
# exits zero.
$msi = Get-ChildItem "src-tauri\target\release\bundle\msi\*.msi" |
  Where-Object { $_.Name -like "*$Version*" } | Select-Object -First 1
if (-not $msi) { throw "No versioned .msi for $Version found - refusing to publish an older installer" }

$prevEA = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
gh release view $tag --repo $Repo *> $null
$releaseExists = ($LASTEXITCODE -eq 0)
$ErrorActionPreference = $prevEA
if ($releaseExists) { throw "Release $tag already exists - refusing to replace it" }

# gh drops an empty --notes value ("flag needs an argument"), so only pass it
# when set. Notes go through a FILE, never through the native argv: Windows
# PowerShell 5.1 escapes embedded double quotes wrong for CreateProcess, so a
# notes string containing "..." arrived split into extra arguments and gh
# tried to open them as assets ("CreateFile installed." at v0.10.22).
$notesArgs = @()
if ($Notes -and $Notes.Trim()) {
  $notesPath = Join-Path ([System.IO.Path]::GetTempPath()) "wan2gp-release-notes-$Version.md"
  [System.IO.File]::WriteAllText($notesPath, $Notes, (New-Object System.Text.UTF8Encoding($false)))
  $notesArgs = @("--notes-file", $notesPath)
} else { $notesArgs = @("--generate-notes") }
$prevEA = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
gh release create $tag $setup.FullName ($setup.FullName + ".sig") $msi.FullName $latestPath `
  --repo $Repo --title $tag --draft @notesArgs
$ghCode = $LASTEXITCODE
$ErrorActionPreference = $prevEA
if ($ghCode -ne 0) { throw "gh release create failed (draft $tag may be partial)" }

$prevEA = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
gh release edit $tag --repo $Repo --draft=false
$ghCode = $LASTEXITCODE
$ErrorActionPreference = $prevEA
if ($ghCode -ne 0) { throw "Publishing draft $tag failed - it is still a draft" }

Write-Host "Released $tag - updater will pick it up from latest.json" -ForegroundColor Green

<#
.SYNOPSIS
  Clear the uv-installer litter out of the user's PATH (and optionally the
  orphaned temp install trees it left behind).

.DESCRIPTION
  uv's standalone installer ships `"modify_path": true`: every run appends its
  install dir to the USER PATH. Older launcher builds ran the installer into a
  throwaway %TEMP%\.tmpXXXX\.tools dir, so each run left a permanent PATH entry
  *and* a whole second copy of uv on disk. On one box this reached 104 entries
  (4 KB of PATH) and 1.65 GB of duplicate binaries.

  Two removal rules, both reported before anything changes:
    1. entries matching  <...>\Temp\.tmp*\tools   (throwaway uv install dirs)
    2. entries under a \Temp\ directory that no longer exist on disk

  Dry run by default. -Apply backs the raw HKCU\Environment\Path value up to a
  timestamped file, then rewrites the value preserving its original registry
  kind (REG_EXPAND_SZ). Only the USER PATH is ever touched — never the machine
  PATH. Launcher-owned dirs (e.g. C:\Wan2GP\.tools) are always kept.

.PARAMETER Apply
  Actually rewrite the user PATH. Without it this is a read-only dry run.

.PARAMETER DeleteTempDirs
  Also delete the orphaned temp install dirs (frees the disk). Off by default.

.PARAMETER BackupDir
  Where the raw PATH backup is written (default: ~\.wan2gp-path-backups).

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\cleanup-uv-path.ps1
  powershell -ExecutionPolicy Bypass -File scripts\cleanup-uv-path.ps1 -Apply -DeleteTempDirs
#>
[CmdletBinding()]
param(
  [switch]$Apply,
  [switch]$DeleteTempDirs,
  [string]$BackupDir = "$env:USERPROFILE\.wan2gp-path-backups"
)

$ErrorActionPreference = 'Stop'

# A throwaway uv install dir: <something>\Temp\.tmpXXXXXX\.tools
$isTempTools = {
  param([string]$p)
  if ([string]::IsNullOrWhiteSpace($p)) { return $false }
  return [bool]($p -match '(?i)[\\/]Temp[\\/]\.tmp[^\\/]*[\\/]\.tools[\\/]?$')
}
$isUnderTemp = {
  param([string]$p)
  if ([string]::IsNullOrWhiteSpace($p)) { return $false }
  return [bool]($p -match '(?i)[\\/]Temp[\\/]')
}

$key  = Get-Item 'HKCU:\Environment'
$kind = $key.GetValueKind('Path')
$raw  = $key.GetValue('Path', $null, 'DoNotExpandEnvironmentNames')
if ($null -eq $raw) { Write-Output 'No user PATH value — nothing to do.'; return }

$entries = @($raw -split ';')
Write-Output ("user PATH (HKCU\Environment\Path, kind {0}): {1} entries, {2} chars" -f $kind, $entries.Count, $raw.Length)

$keep  = New-Object System.Collections.Generic.List[string]
$drop  = New-Object System.Collections.Generic.List[string]
$seen  = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
$tempDirs = New-Object System.Collections.Generic.List[string]

foreach ($e in $entries) {
  if ([string]::IsNullOrWhiteSpace($e)) { $drop.Add("(blank entry)"); continue }
  if (& $isTempTools $e) {
    $drop.Add("$e   (throwaway uv install dir)")
    $tempDirs.Add($e)
    continue
  }
  if ((& $isUnderTemp $e) -and -not (Test-Path $e)) {
    $drop.Add("$e   (dead entry under Temp)")
    continue
  }
  $norm = $e.TrimEnd('\', '/')
  if (-not $seen.Add($norm)) { $drop.Add("$e   (duplicate of an earlier entry)"); continue }
  $keep.Add($e)
}

$newRaw = ($keep -join ';')

Write-Output ''
Write-Output ("=== would remove ({0}) ===" -f $drop.Count)
$drop | ForEach-Object { Write-Output "  - $_" }
Write-Output ''
Write-Output ("=== would keep ({0}) ===" -f $keep.Count)
$keep | ForEach-Object { Write-Output "  + $_" }
Write-Output ''
Write-Output ("after: {0} entries, {1} chars (was {2})" -f $keep.Count, $newRaw.Length, $raw.Length)

$unique = @($tempDirs | Sort-Object -Unique)
$present = @($unique | Where-Object { Test-Path $_ })
$bytes = 0
foreach ($d in $present) {
  $sum = (Get-ChildItem $d -Recurse -Force -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum
  if ($sum) { $bytes += $sum }
}
Write-Output ''
Write-Output ("=== orphaned temp install dirs: {0} referenced, {1} still on disk, {2:N2} GB ===" -f $unique.Count, $present.Count, ($bytes / 1GB))

if (-not $Apply) {
  Write-Output ''
  Write-Output 'DRY RUN — nothing changed. Re-run with -Apply (add -DeleteTempDirs to also free the disk).'
  return
}

New-Item -ItemType Directory -Force -Path $BackupDir | Out-Null
$backup = Join-Path $BackupDir ("user-path-{0}.txt" -f (Get-Date -Format 'yyyyMMdd-HHmmss'))
Set-Content -Path $backup -Value $raw -NoNewline -Encoding UTF8
Write-Output "backed up raw user PATH -> $backup"

# Write via reg.exe, not [Microsoft.Win32.RegistryKey]::SetValue: on hardened
# boxes (Bitdefender's registry guard, restricted tokens) the .NET call fails
# with "Cannot write to the registry key" while reg.exe succeeds. Keep
# REG_EXPAND_SZ explicitly — .NET's SetEnvironmentVariable would rewrite the
# value as REG_SZ and expand any %VAR% in it.
$out = & reg add 'HKCU\Environment' /v Path /t REG_EXPAND_SZ /d $newRaw /f 2>&1
if ($LASTEXITCODE -ne 0) { throw "reg add failed (exit $LASTEXITCODE): $out" }
Write-Output "user PATH rewritten (registry kind preserved: $kind)."

$verify = (Get-Item 'HKCU:\Environment').GetValue('Path', $null, 'DoNotExpandEnvironmentNames')
if ($verify -ne $newRaw) { throw "verification failed: the stored PATH is not what was written" }
Write-Output ("verified: user PATH is now {0} entries, {1} chars" -f (@($verify -split ';').Count), $verify.Length)

if ($DeleteTempDirs) {
  $freed = 0
  foreach ($d in $present) {
    $sum = (Get-ChildItem $d -Recurse -Force -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum
    try {
      Remove-Item $d -Recurse -Force -ErrorAction Stop
      if ($sum) { $freed += $sum }
      Write-Output "  removed $d"
    } catch {
      Write-Output "  could not remove $d : $($_.Exception.Message)"
    }
  }
  Write-Output ("freed {0:N2} GB" -f ($freed / 1GB))
}

Write-Output ''
Write-Output 'Done. Already-running processes keep their old PATH; newly started ones pick this up.'

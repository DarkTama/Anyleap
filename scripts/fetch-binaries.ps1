<#
.SYNOPSIS
  Download a pinned scrcpy Windows release and lay its files out as Tauri
  sidecars + resources under src-tauri/binaries/.

.DESCRIPTION
  - Sidecars (target-triple suffixed): scrcpy.exe, adb.exe
  - Resource:                          scrcpy-server (the device-side jar)
  - Resources (DLLs):                  src-tauri/binaries/dll/*.dll
  Binaries are gitignored; this script repopulates them. Idempotent unless -Force.

.NOTES
  Runs on Windows PowerShell 5.1+. No Rust required.
#>
[CmdletBinding()]
param(
    [string]$Version = "4.0",
    [string]$ExpectedSha256 = "",   # optional; if empty, tries the release SHA256SUMS.txt
    [switch]$Force
)
$ErrorActionPreference = "Stop"
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

# Known-good SHA-256 hashes for pinned releases (verified at authoring time).
$KnownSha256 = @{
    "4.0" = "75DBEB5B00E6F64292F26F70900AE55CA397786BDFB0B9BBEB481A0549047457"
}
if (-not $ExpectedSha256 -and $KnownSha256.ContainsKey($Version)) {
    $ExpectedSha256 = $KnownSha256[$Version]
}

$root   = Split-Path -Parent $PSScriptRoot
$binDir = Join-Path $root "src-tauri\binaries"
$dllDir = Join-Path $binDir "dll"

# Target triple for the sidecar suffix (Tauri requires this). Default to MSVC;
# derive from rustc if available so non-default toolchains still work.
$triple = "x86_64-pc-windows-msvc"
$rustc = Get-Command rustc -ErrorAction SilentlyContinue
if ($rustc) {
    try {
        $hostLine = (& rustc -vV) | Select-String '^host:'
        if ($hostLine) { $triple = ($hostLine.ToString() -split '\s+')[1] }
    } catch { }
}

$scrcpyExe = Join-Path $binDir "scrcpy-$triple.exe"
$adbExe    = Join-Path $binDir "adb-$triple.exe"
$serverDst = Join-Path $binDir "scrcpy-server"

if (-not $Force -and (Test-Path $scrcpyExe) -and (Test-Path $adbExe) -and (Test-Path $serverDst)) {
    Write-Host "Binaries already present (use -Force to re-download). Skipping."
    exit 0
}

$asset = "scrcpy-win64-v$Version.zip"
$url   = "https://github.com/Genymobile/scrcpy/releases/download/v$Version/$asset"
$tmp   = Join-Path ([System.IO.Path]::GetTempPath()) "anyleap-scrcpy"
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
$zip = Join-Path $tmp $asset

Write-Host "Downloading $url ..."
Invoke-WebRequest -Uri $url -OutFile $zip

# --- Integrity check ---
if (-not $ExpectedSha256) {
    try {
        $sumsUrl = "https://github.com/Genymobile/scrcpy/releases/download/v$Version/SHA256SUMS.txt"
        $sums = (Invoke-WebRequest -Uri $sumsUrl -UseBasicParsing).Content
        foreach ($line in ($sums -split "`n")) {
            if ($line -match "([0-9a-fA-F]{64})\s+\*?$([regex]::Escape($asset))") {
                $ExpectedSha256 = $Matches[1]; break
            }
        }
    } catch {
        Write-Warning "Could not fetch SHA256SUMS.txt; skipping integrity check."
    }
}
$actual = (Get-FileHash -Algorithm SHA256 -Path $zip).Hash
if ($ExpectedSha256) {
    if ($actual -ne $ExpectedSha256.ToUpper()) {
        throw "SHA-256 mismatch for $asset`n  expected $($ExpectedSha256.ToUpper())`n  actual   $actual"
    }
    Write-Host "SHA-256 OK: $actual"
} else {
    Write-Warning "No expected SHA-256 available; downloaded hash = $actual (NOT verified)."
}

# --- Extract ---
$ext = Join-Path $tmp "extracted"
if (Test-Path $ext) { Remove-Item -Recurse -Force $ext }
Expand-Archive -Path $zip -DestinationPath $ext -Force
# scrcpy zips extract into a single top-level folder; fall back to flat layout.
$inner = Get-ChildItem -Path $ext -Directory | Select-Object -First 1
$srcDir = if ($inner) { $inner.FullName } else { $ext }

# --- Lay out ---
New-Item -ItemType Directory -Force -Path $binDir, $dllDir | Out-Null
Copy-Item (Join-Path $srcDir "scrcpy.exe")    $scrcpyExe -Force
Copy-Item (Join-Path $srcDir "adb.exe")       $adbExe    -Force
Copy-Item (Join-Path $srcDir "scrcpy-server") $serverDst -Force
Get-ChildItem -Path $srcDir -Filter *.dll | ForEach-Object {
    Copy-Item $_.FullName (Join-Path $dllDir $_.Name) -Force
}

Write-Host ""
Write-Host "Done. Layout under src-tauri/binaries/ :"
Get-ChildItem $binDir -Recurse -File | ForEach-Object {
    Write-Host ("  " + $_.FullName.Substring($binDir.Length + 1))
}

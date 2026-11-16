<#
.SYNOPSIS
  Silently installs AnyLeap via its NSIS installer after verifying user idleness,
  stopping AnyLeap/sidecar/WerFault processes, verifying exe change, and writing
  install-result.json next to the script.
#>
[CmdletBinding()]
param(
    [string]$Installer = '',
    [switch]$Launch,
    [string]$InstallDir = '',
    [int]$IdleThresholdSec = 120,
    [int]$PollIntervalSec = 15,
    [int]$MaxIdleWaitMinutes = 10,
    [int]$InstallerTimeoutSec = 180,
    [string]$ResultPath = '',
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

. (Join-Path $PSScriptRoot 'common.ps1')

if ([string]::IsNullOrWhiteSpace($ResultPath)) {
    $ResultPath = Join-Path $PSScriptRoot 'install-result.json'
}

function Normalize-DirPrefix {
    param([string]$Dir)
    if ([string]::IsNullOrWhiteSpace($Dir)) {
        return ''
    }
    return ([System.IO.Path]::GetFullPath($Dir).TrimEnd('\', '/') + '\').ToLowerInvariant()
}

function Resolve-AnyLeapInstallDir {
    param([string]$ExplicitDir = '')

    $candidates = New-Object System.Collections.Generic.List[string]
    if (-not [string]::IsNullOrWhiteSpace($ExplicitDir)) {
        $candidates.Add($ExplicitDir)
    }

    # Check running anyleap.exe
    try {
        $running = Get-CimInstance -ClassName Win32_Process -Filter "Name = 'anyleap.exe'" -ErrorAction SilentlyContinue
        foreach ($proc in $running) {
            if ($proc.ExecutablePath) {
                $candidates.Add((Split-Path -Path $proc.ExecutablePath -Parent))
            }
        }
    } catch {
    }

    # Check standard NSIS uninstall registry locations
    $regRoots = @(
        'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*',
        'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*',
        'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*'
    )
    foreach ($root in $regRoots) {
        try {
            $entries = Get-ItemProperty -Path $root -ErrorAction SilentlyContinue | Where-Object {
                ($_.DisplayName -and $_.DisplayName -like '*AnyLeap*') -or ($_.PSChildName -and $_.PSChildName -like '*AnyLeap*')
            }
            foreach ($entry in $entries) {
                if ($entry.InstallLocation) {
                    $candidates.Add($entry.InstallLocation)
                } elseif ($entry.UninstallString) {
                    $cleaned = $entry.UninstallString.Trim().Trim('"')
                    if (Test-Path -LiteralPath $cleaned) {
                        $candidates.Add((Split-Path -Path $cleaned -Parent))
                    }
                }
            }
        } catch {
        }
    }

    if ($env:LOCALAPPDATA) {
        $candidates.Add((Join-Path $env:LOCALAPPDATA 'AnyLeap'))
        $candidates.Add((Join-Path $env:LOCALAPPDATA 'Programs\AnyLeap'))
    }
    if ($env:ProgramFiles) {
        $candidates.Add((Join-Path $env:ProgramFiles 'AnyLeap'))
    }

    foreach ($dir in $candidates) {
        if (-not [string]::IsNullOrWhiteSpace($dir)) {
            $exe = Join-Path $dir 'anyleap.exe'
            if (Test-Path -LiteralPath $exe) {
                return [System.IO.Path]::GetFullPath($dir)
            }
        }
    }

    if (-not [string]::IsNullOrWhiteSpace($ExplicitDir)) {
        return [System.IO.Path]::GetFullPath($ExplicitDir)
    }
    if ($env:LOCALAPPDATA) {
        return [System.IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'AnyLeap'))
    }
    return ''
}

function Get-ExeMetadata {
    param([string]$ExePath)

    if ([string]::IsNullOrWhiteSpace($ExePath) -or -not (Test-Path -LiteralPath $ExePath -PathType Leaf)) {
        return [pscustomobject]@{
            Exists           = $false
            Path             = $ExePath
            LastWriteTimeUtc = $null
            Length           = $null
            FileVersion      = $null
            ProductVersion   = $null
            Sha256           = $null
        }
    }

    $item = Get-Item -LiteralPath $ExePath
    $ver = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($ExePath)
    $hash = (Get-FileHash -LiteralPath $ExePath -Algorithm SHA256).Hash

    return [pscustomobject]@{
        Exists           = $true
        Path             = $item.FullName
        LastWriteTimeUtc = $item.LastWriteTimeUtc.ToString('o')
        Length           = [int64]$item.Length
        FileVersion      = $ver.FileVersion
        ProductVersion   = $ver.ProductVersion
        Sha256           = $hash
    }
}

function Select-TargetProcessesToStop {
    param(
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][object[]]$Processes,
        [string]$InstallDir = ''
    )

    $effectiveDir = $InstallDir
    $anyleapProcs = New-Object System.Collections.Generic.List[object]
    $targetPids = New-Object 'System.Collections.Generic.HashSet[int]'

    foreach ($p in $Processes) {
        $name = if ($p.Name) { $p.Name.ToLowerInvariant() } else { '' }
        if ($name -eq 'anyleap.exe') {
            $anyleapProcs.Add($p)
            [void]$targetPids.Add([int]$p.ProcessId)
            if ([string]::IsNullOrWhiteSpace($effectiveDir) -and $p.ExecutablePath) {
                $effectiveDir = Split-Path -Path $p.ExecutablePath -Parent
            }
        }
    }

    $dirPrefix = Normalize-DirPrefix -Dir $effectiveDir
    $sidecarProcs = New-Object System.Collections.Generic.List[object]

    foreach ($p in $Processes) {
        $name = if ($p.Name) { $p.Name.ToLowerInvariant() } else { '' }
        if ($name -eq 'adb.exe' -or $name -eq 'scrcpy.exe') {
            $exePath = if ($p.ExecutablePath) { $p.ExecutablePath } else { '' }
            $matchesInstallDir = $false
            if (-not [string]::IsNullOrWhiteSpace($dirPrefix) -and -not [string]::IsNullOrWhiteSpace($exePath)) {
                $fullExe = [System.IO.Path]::GetFullPath($exePath).ToLowerInvariant()
                if ($fullExe.StartsWith($dirPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
                    $matchesInstallDir = $true
                }
            } elseif (-not [string]::IsNullOrWhiteSpace($exePath)) {
                if ($exePath -match '(?i)[\\/]AnyLeap[\\/]+(adb|scrcpy)\.exe$') {
                    $matchesInstallDir = $true
                }
            }
            if ($matchesInstallDir) {
                $sidecarProcs.Add($p)
                [void]$targetPids.Add([int]$p.ProcessId)
            }
        }
    }

    $werProcs = New-Object System.Collections.Generic.List[object]
    foreach ($p in $Processes) {
        $name = if ($p.Name) { $p.Name.ToLowerInvariant() } else { '' }
        if ($name -eq 'werfault.exe' -and $p.CommandLine) {
            if ($p.CommandLine -match '(?i)(?:^|\s)-p\s+(\d+)(?:\s|$)') {
                $targetPid = [int]$Matches[1]
                if ($targetPids.Contains($targetPid)) {
                    $werProcs.Add($p)
                }
            }
        }
    }

    # Stop WerFault first so it releases crash dump handles, then AnyLeap, then sidecars.
    $ordered = New-Object System.Collections.Generic.List[object]
    foreach ($p in $werProcs) { $ordered.Add($p) }
    foreach ($p in $anyleapProcs) { $ordered.Add($p) }
    foreach ($p in $sidecarProcs) { $ordered.Add($p) }
    return $ordered.ToArray()
}

function Test-InstallChanged {
    param(
        [Parameter(Mandatory = $true)][object]$Before,
        [Parameter(Mandatory = $true)][object]$After
    )

    if (-not $After.Exists) {
        return [pscustomobject]@{ Valid = $false; Changed = $false; Reason = 'installed_exe_missing' }
    }
    if (-not $Before.Exists) {
        return [pscustomobject]@{ Valid = $true; Changed = $true; Reason = 'fresh_install' }
    }

    $reasons = New-Object System.Collections.Generic.List[string]
    if ($After.LastWriteTimeUtc -ne $Before.LastWriteTimeUtc) {
        $reasons.Add('timestamp_changed')
    }
    if ($After.FileVersion -ne $Before.FileVersion) {
        $reasons.Add('file_version_changed')
    }
    if ($After.ProductVersion -ne $Before.ProductVersion) {
        $reasons.Add('product_version_changed')
    }
    if ($Before.Sha256 -and $After.Sha256 -and ($After.Sha256 -ne $Before.Sha256)) {
        $reasons.Add('hash_changed')
    }

    if ($reasons.Count -gt 0) {
        return [pscustomobject]@{ Valid = $true; Changed = $true; Reason = ($reasons -join ',') }
    }

    return [pscustomobject]@{ Valid = $false; Changed = $false; Reason = 'timestamp_and_version_unchanged' }
}

function Test-InstallArguments {
    param([string]$InstallerPath)
    if ([string]::IsNullOrWhiteSpace($InstallerPath)) {
        return [pscustomobject]@{ Ok = $false; Error = 'Installer parameter is required.' }
    }
    if (-not (Test-Path -LiteralPath $InstallerPath -PathType Leaf)) {
        return [pscustomobject]@{ Ok = $false; Error = "Installer file not found: $InstallerPath" }
    }
    return [pscustomobject]@{ Ok = $true; Error = $null }
}

function Invoke-InstallDryRun {
    Write-Host "=== install-anyleap.ps1 -DryRun ==="

    # 1. Argument validation
    $argBad = Test-InstallArguments -InstallerPath ''
    Assert-DryRunCondition (-not $argBad.Ok) "Rejects empty -Installer argument"
    $argMissing = Test-InstallArguments -InstallerPath 'C:\nonexistent\anyleap-setup-fake.exe'
    Assert-DryRunCondition (-not $argMissing.Ok) "Rejects missing -Installer path"
    $argValid = Test-InstallArguments -InstallerPath $PSCommandPath
    Assert-DryRunCondition ($argValid.Ok) "Accepts existing file path in argument check"

    # 2. Idle guard check (synthetic)
    $idleFail = Invoke-IdleGuard -IdleThresholdSec 120 -PollIntervalSec 15 -MaxWaitMinutes 0 `
        -GetIdleStateFn { [pscustomobject]@{ IdleSeconds = 20.0; LastInputTick = [uint32]500; NowTick = [uint32]20500 } }
    Assert-DryRunCondition ((-not $idleFail.Ok) -and $idleFail.Status -eq 'user_active') "Aborts with user_active when user is active"

    # 3. Process selection check (anyleap.exe, install-dir adb/scrcpy, WerFault -p)
    $syntheticProcs = @(
        [pscustomobject]@{ ProcessId = 101; Name = 'anyleap.exe'; ExecutablePath = 'C:\Users\Test\AppData\Local\AnyLeap\anyleap.exe'; CommandLine = '' },
        [pscustomobject]@{ ProcessId = 102; Name = 'adb.exe'; ExecutablePath = 'C:\Users\Test\AppData\Local\AnyLeap\adb.exe'; CommandLine = 'adb.exe start-server' },
        [pscustomobject]@{ ProcessId = 103; Name = 'adb.exe'; ExecutablePath = 'C:\Android\platform-tools\adb.exe'; CommandLine = 'adb.exe' },
        [pscustomobject]@{ ProcessId = 104; Name = 'scrcpy.exe'; ExecutablePath = 'C:\Users\Test\AppData\Local\AnyLeap\scrcpy.exe'; CommandLine = '' },
        [pscustomobject]@{ ProcessId = 105; Name = 'WerFault.exe'; ExecutablePath = 'C:\Windows\System32\WerFault.exe'; CommandLine = 'C:\Windows\System32\WerFault.exe -u -p 101 -s 555' },
        [pscustomobject]@{ ProcessId = 106; Name = 'WerFault.exe'; ExecutablePath = 'C:\Windows\System32\WerFault.exe'; CommandLine = 'C:\Windows\System32\WerFault.exe -u -p 999 -s 111' }
    )
    $selected = Select-TargetProcessesToStop -Processes $syntheticProcs -InstallDir 'C:\Users\Test\AppData\Local\AnyLeap'
    $selectedIds = @($selected | ForEach-Object { $_.ProcessId })
    Assert-DryRunCondition ($selectedIds.Count -eq 4) "Selects exactly 4 target processes (got $($selectedIds.Count))"
    Assert-DryRunCondition ($selectedIds[0] -eq 105) "Targets WerFault.exe for anyleap.exe PID 101 first"
    Assert-DryRunCondition (($selectedIds -contains 101) -and ($selectedIds -contains 102) -and ($selectedIds -contains 104)) "Targets anyleap.exe + install-dir adb.exe and scrcpy.exe"
    Assert-DryRunCondition ((-not ($selectedIds -contains 103)) -and (-not ($selectedIds -contains 106))) "Ignores external adb.exe and unrelated WerFault.exe"

    # 4. Install change verification check
    $before = [pscustomobject]@{ Exists = $true; LastWriteTimeUtc = '2026-11-01T00:00:00Z'; FileVersion = '1.1.0.0'; ProductVersion = '1.1.0'; Sha256 = 'AAA' }
    $same = [pscustomobject]@{ Exists = $true; LastWriteTimeUtc = '2026-11-01T00:00:00Z'; FileVersion = '1.1.0.0'; ProductVersion = '1.1.0'; Sha256 = 'AAA' }
    $newer = [pscustomobject]@{ Exists = $true; LastWriteTimeUtc = '2026-11-16T10:00:00Z'; FileVersion = '1.1.1.0'; ProductVersion = '1.1.1'; Sha256 = 'BBB' }
    $missing = [pscustomobject]@{ Exists = $false; LastWriteTimeUtc = $null; FileVersion = $null; ProductVersion = $null; Sha256 = $null }
    $freshBefore = [pscustomobject]@{ Exists = $false; LastWriteTimeUtc = $null; FileVersion = $null; ProductVersion = $null; Sha256 = $null }

    Assert-DryRunCondition (-not (Test-InstallChanged -Before $before -After $same).Valid) "Detects unchanged exe as failed verification"
    Assert-DryRunCondition ((Test-InstallChanged -Before $before -After $newer).Valid) "Detects timestamp/version change as valid install"
    Assert-DryRunCondition (-not (Test-InstallChanged -Before $before -After $missing).Valid) "Detects missing post-install exe as failure"
    Assert-DryRunCondition ((Test-InstallChanged -Before $freshBefore -After $newer).Valid) "Accepts fresh install"

    Write-Host "install-anyleap.ps1 -DryRun PASSED"
}

if ($DryRun) {
    Invoke-InstallDryRun
    exit 0
}

$sw = [System.Diagnostics.Stopwatch]::StartNew()
$startedUtc = [DateTime]::UtcNow.ToString('o')

$argCheck = Test-InstallArguments -InstallerPath $Installer
if (-not $argCheck.Ok) {
    $errResult = [pscustomobject]@{
        status       = 'error'
        timestampUtc = $startedUtc
        installer    = $Installer
        error        = $argCheck.Error
    }
    Write-JsonFile -Path $ResultPath -Data $errResult
    Write-Error $argCheck.Error
    exit 1
}

$installerFullPath = [System.IO.Path]::GetFullPath($Installer)

# 1. Idle guard
$idleRes = Invoke-IdleGuard -IdleThresholdSec $IdleThresholdSec -PollIntervalSec $PollIntervalSec -MaxWaitMinutes $MaxIdleWaitMinutes
if (-not $idleRes.Ok) {
    $sw.Stop()
    $abortResult = [pscustomobject]@{
        status       = $idleRes.Status
        timestampUtc = [DateTime]::UtcNow.ToString('o')
        installer    = $installerFullPath
        idleSeconds  = $idleRes.IdleSeconds
        waitedSec    = $idleRes.WaitedSeconds
        durationSec  = [Math]::Round($sw.Elapsed.TotalSeconds, 2)
    }
    Write-JsonFile -Path $ResultPath -Data $abortResult
    Write-Warning "Aborted: user is active (idle $($idleRes.IdleSeconds)s)."
    exit 2
}

# 2. Locate install dir and snapshot before state
$resolvedDir = Resolve-AnyLeapInstallDir -ExplicitDir $InstallDir
$exePathBefore = if ($resolvedDir) { Join-Path $resolvedDir 'anyleap.exe' } else { '' }
$beforeMeta = Get-ExeMetadata -ExePath $exePathBefore

# 3. Stop AnyLeap, stray adb/scrcpy from install dir, and targeting WerFault.exe
$allProcs = Get-CimInstance -ClassName Win32_Process | Select-Object ProcessId, Name, ExecutablePath, CommandLine
$toStop = @(Select-TargetProcessesToStop -Processes $allProcs -InstallDir $resolvedDir)
$stoppedList = New-Object System.Collections.Generic.List[object]

foreach ($proc in $toStop) {
    try {
        Stop-Process -Id ([int]$proc.ProcessId) -Force -ErrorAction Stop
        $stoppedList.Add([pscustomobject]@{
            pid  = [int]$proc.ProcessId
            name = $proc.Name
            path = $proc.ExecutablePath
        })
    } catch {
        Write-Warning "Could not stop $($proc.Name) (PID $($proc.ProcessId)): $($_.Exception.Message)"
    }
}

if ($toStop.Count -gt 0) {
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    while ([DateTime]::UtcNow -lt $deadline) {
        $stillAlive = $false
        foreach ($proc in $toStop) {
            if (Get-Process -Id ([int]$proc.ProcessId) -ErrorAction SilentlyContinue) {
                $stillAlive = $true
                break
            }
        }
        if (-not $stillAlive) { break }
        Start-Sleep -Milliseconds 250
    }
}

# 4. Run NSIS silent installer (/S)
Write-Host "Running installer silently: $installerFullPath /S"
$proc = Start-Process -FilePath $installerFullPath -ArgumentList '/S' -PassThru
$null = $proc.Handle
$exited = $proc.WaitForExit($InstallerTimeoutSec * 1000)
if (-not $exited) {
    try { $proc.Kill() } catch {}
    $sw.Stop()
    $timeoutResult = [pscustomobject]@{
        status            = 'installer_timeout'
        timestampUtc      = [DateTime]::UtcNow.ToString('o')
        installer         = $installerFullPath
        stoppedProcesses  = $stoppedList.ToArray()
        installerTimeoutS = $InstallerTimeoutSec
        durationSec       = [Math]::Round($sw.Elapsed.TotalSeconds, 2)
    }
    Write-JsonFile -Path $ResultPath -Data $timeoutResult
    Write-Error "Installer timed out after $InstallerTimeoutSec seconds."
    exit 1
}

$installerExitCode = $proc.ExitCode
if ($installerExitCode -ne 0) {
    $sw.Stop()
    $failResult = [pscustomobject]@{
        status            = 'installer_failed'
        timestampUtc      = [DateTime]::UtcNow.ToString('o')
        installer         = $installerFullPath
        installerExitCode = $installerExitCode
        stoppedProcesses  = $stoppedList.ToArray()
        durationSec       = [Math]::Round($sw.Elapsed.TotalSeconds, 2)
    }
    Write-JsonFile -Path $ResultPath -Data $failResult
    Write-Error "Installer exited with code $installerExitCode."
    exit 1
}

# Brief settle wait in case NSIS finalizes file move right at exit
Start-Sleep -Milliseconds 500
$afterDir = Resolve-AnyLeapInstallDir -ExplicitDir $InstallDir
$exePathAfter = if ($afterDir) { Join-Path $afterDir 'anyleap.exe' } else { $exePathBefore }
$afterMeta = Get-ExeMetadata -ExePath $exePathAfter
$verify = Test-InstallChanged -Before $beforeMeta -After $afterMeta

$launched = $false
$launchedPid = $null
if ($verify.Valid -and $Launch) {
    Write-Host "Launching AnyLeap: $exePathAfter"
    $appProc = Start-Process -FilePath $exePathAfter -WorkingDirectory (Split-Path -Path $exePathAfter -Parent) -PassThru
    Start-Sleep -Milliseconds 1000
    if ($appProc -and -not $appProc.HasExited) {
        $launched = $true
        $launchedPid = $appProc.Id
    } else {
        $runningAfter = Get-Process -Name 'anyleap' -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($runningAfter) {
            $launched = $true
            $launchedPid = $runningAfter.Id
        }
    }
}

$sw.Stop()
$finalStatus = if ($verify.Valid) { 'succeeded' } else { 'verify_failed' }
$result = [pscustomobject]@{
    status            = $finalStatus
    timestampUtc      = [DateTime]::UtcNow.ToString('o')
    installer         = $installerFullPath
    installerExitCode = $installerExitCode
    installDir        = $afterDir
    installedExe      = $exePathAfter
    before            = $beforeMeta
    after             = $afterMeta
    changed           = $verify.Changed
    changeReason      = $verify.Reason
    stoppedProcesses  = $stoppedList.ToArray()
    launched          = $launched
    launchedPid       = $launchedPid
    durationSec       = [Math]::Round($sw.Elapsed.TotalSeconds, 2)
}

Write-JsonFile -Path $ResultPath -Data $result
Write-Host "Install result written to $ResultPath (status=$finalStatus, reason=$($verify.Reason))"

if (-not $verify.Valid) {
    exit 1
}
exit 0

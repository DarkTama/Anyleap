<#
.SYNOPSIS
  Two-PC KVM return-cursor soak test for AnyLeap (Windows PowerShell 5.1).
  Uses SendInput RELATIVE mouse moves only (no clicks, no keyboard), enforces
  the 120s idle guard, aborts immediately on real user input, samples
  GetCursorPos at +0ms, +50ms, and +250ms after RETURN, classifies results
  (edge_ok, center, other), and writes kvm-soak-result.json.
  Moves are tagged with a dwExtraInfo magic; the script creates the
  %LOCALAPPDATA%\id.co.dak.anyleap\e2e-accept-injected flag file (removed in
  finally) so AnyLeap's hook treats them as physical. Stops early with status
  no_engage if the first 5 cycles never engage; logs kvm-soak-progress.log.
#>
[CmdletBinding()]
param(
    [int]$Cycles = 300,
    [ValidateSet('left', 'right', 'top', 'bottom')]
    [string]$SeamEdge = 'left',
    [string]$LogPath = '',
    [string]$ResultPath = '',
    [int]$IdleThresholdSec = 120,
    [int]$PollIntervalSec = 15,
    [int]$MaxIdleWaitMinutes = 10,
    [int]$EngageTimeoutMs = 3000,
    [int]$ReturnTimeoutMs = 4000,
    [int]$EdgeTolerancePx = 40,
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

. (Join-Path $PSScriptRoot 'common.ps1')

if ([string]::IsNullOrWhiteSpace($ResultPath)) {
    $ResultPath = Join-Path $PSScriptRoot 'kvm-soak-result.json'
}
if ([string]::IsNullOrWhiteSpace($LogPath) -and $env:LOCALAPPDATA) {
    $LogPath = Join-Path $env:LOCALAPPDATA 'id.co.dak.anyleap\logs\kvm-debug.log'
}

function Get-SeamDirection {
    param([Parameter(Mandatory = $true)][string]$Edge)
    switch ($Edge) {
        'left'   { return [pscustomobject]@{ OutX = -1; OutY = 0;  InX = 1;  InY = 0;  Axis = 'x' } }
        'right'  { return [pscustomobject]@{ OutX = 1;  OutY = 0;  InX = -1; InY = 0;  Axis = 'x' } }
        'top'    { return [pscustomobject]@{ OutX = 0;  OutY = -1; InX = 0;  InY = 1;  Axis = 'y' } }
        'bottom' { return [pscustomobject]@{ OutX = 0;  OutY = 1;  InX = 0;  InY = -1; Axis = 'y' } }
        default  { throw "Unsupported SeamEdge: $Edge" }
    }
}

function Get-EdgeMonitor {
    param(
        [Parameter(Mandatory = $true)][object[]]$Monitors,
        [Parameter(Mandatory = $true)][string]$Edge
    )
    if ($null -eq $Monitors -or $Monitors.Count -eq 0) {
        throw "No display monitors detected."
    }

    switch ($Edge) {
        'left'   { return ($Monitors | Sort-Object @{ Expression = { $_.Left }; Ascending = $true }, @{ Expression = { $_.IsPrimary }; Descending = $true } | Select-Object -First 1) }
        'right'  { return ($Monitors | Sort-Object @{ Expression = { $_.Right }; Descending = $true }, @{ Expression = { $_.IsPrimary }; Descending = $true } | Select-Object -First 1) }
        'top'    { return ($Monitors | Sort-Object @{ Expression = { $_.Top }; Ascending = $true }, @{ Expression = { $_.IsPrimary }; Descending = $true } | Select-Object -First 1) }
        'bottom' { return ($Monitors | Sort-Object @{ Expression = { $_.Bottom }; Descending = $true }, @{ Expression = { $_.IsPrimary }; Descending = $true } | Select-Object -First 1) }
        default  { throw "Unsupported SeamEdge: $Edge" }
    }
}

function Test-CursorClassification {
    param(
        [Parameter(Mandatory = $true)][int]$X,
        [Parameter(Mandatory = $true)][int]$Y,
        [Parameter(Mandatory = $true)][object]$Monitor,
        [Parameter(Mandatory = $true)][string]$Edge,
        [Parameter(Mandatory = $true)][int]$ExpectedAlongCoord,
        [int]$TolerancePx = 40
    )

    $edgeOk = $false
    switch ($Edge) {
        'left' {
            $edgeOk = ([Math]::Abs($X - $Monitor.Left) -le $TolerancePx) -and ([Math]::Abs($Y - $ExpectedAlongCoord) -le $TolerancePx)
        }
        'right' {
            $edgeX = $Monitor.Right - 1
            $edgeOk = ([Math]::Abs($X - $edgeX) -le $TolerancePx) -and ([Math]::Abs($Y - $ExpectedAlongCoord) -le $TolerancePx)
        }
        'top' {
            $edgeOk = ([Math]::Abs($Y - $Monitor.Top) -le $TolerancePx) -and ([Math]::Abs($X - $ExpectedAlongCoord) -le $TolerancePx)
        }
        'bottom' {
            $edgeY = $Monitor.Bottom - 1
            $edgeOk = ([Math]::Abs($Y - $edgeY) -le $TolerancePx) -and ([Math]::Abs($X - $ExpectedAlongCoord) -le $TolerancePx)
        }
        default { throw "Unsupported SeamEdge: $Edge" }
    }

    if ($edgeOk) {
        return 'edge_ok'
    }

    # Central 30% band along both axes: [0.35, 0.65] of monitor width and height.
    $centerLeft   = [double]$Monitor.Left + ([double]$Monitor.Width * 0.35)
    $centerRight  = [double]$Monitor.Left + ([double]$Monitor.Width * 0.65)
    $centerTop    = [double]$Monitor.Top  + ([double]$Monitor.Height * 0.35)
    $centerBottom = [double]$Monitor.Top  + ([double]$Monitor.Height * 0.65)

    if (([double]$X -ge $centerLeft) -and ([double]$X -le $centerRight) -and ([double]$Y -ge $centerTop) -and ([double]$Y -le $centerBottom)) {
        return 'center'
    }

    return 'other'
}

function Get-CycleClassification {
    param(
        [Parameter(Mandatory = $true)][string]$Class0,
        [Parameter(Mandatory = $true)][string]$Class50,
        [Parameter(Mandatory = $true)][string]$Class250
    )
    if ($Class0 -eq 'center' -or $Class50 -eq 'center' -or $Class250 -eq 'center') {
        return 'center'
    }
    if ($Class0 -eq 'edge_ok' -and $Class50 -eq 'edge_ok' -and $Class250 -eq 'edge_ok') {
        return 'edge_ok'
    }
    return 'other'
}

function Get-CycleScenarioName {
    param([int]$CycleIndex)
    $scenarios = @('slow', 'fast_flick', 'immediate_releap')
    return $scenarios[$CycleIndex % $scenarios.Count]
}

function Format-SanitizedLogExcerpt {
    param([Parameter(Mandatory = $true)][string]$Line)

    $tokens = New-Object System.Collections.Generic.List[string]
    if ($Line -match '\bts_us=(\d+)') {
        $tokens.Add('ts_us=' + $Matches[1])
    }
    if ($Line -match '\bevent=([A-Za-z0-9_]+)') {
        $tokens.Add('event=' + $Matches[1])
    }
    if ($Line -match '\bphase=([A-Za-z0-9_]+)') {
        $tokens.Add('phase=' + $Matches[1])
    }
    if ($Line -match '\bedge=([A-Za-z0-9_]+)') {
        $tokens.Add('edge=' + $Matches[1])
    }
    if ($Line -match '\brel_y=([0-9.\-]+)') {
        $tokens.Add('rel_y=' + $Matches[1])
    }
    if ($Line -match '\bwas_trapped=(\d+)') {
        $tokens.Add('was_trapped=' + $Matches[1])
    }
    return ($tokens -join ' ')
}

function Parse-KvmDebugEvent {
    param([Parameter(Mandatory = $true)][string]$Line)

    if ($Line -match '\bts_us=(\d+).*\bevent=ENGAGE\b.*\bphase=perform\b') {
        $tsUs = [int64]$Matches[1]
        $relY = $null
        if ($Line -match '\brel_y=([0-9.\-]+)') {
            $relY = [double]::Parse($Matches[1], [System.Globalization.CultureInfo]::InvariantCulture)
        }
        return [pscustomobject]@{
            Kind    = 'ENGAGE_PERFORM'
            TsUs    = $tsUs
            RelY    = $relY
            Excerpt = (Format-SanitizedLogExcerpt -Line $Line)
        }
    }

    if ($Line -match '\bts_us=(\d+).*\bevent=RETURN\b') {
        $tsUs = [int64]$Matches[1]
        $relY = $null
        $wasTrapped = $null
        if ($Line -match '\brel_y=([0-9.\-]+)') {
            $relY = [double]::Parse($Matches[1], [System.Globalization.CultureInfo]::InvariantCulture)
        }
        if ($Line -match '\bwas_trapped=(\d+)') {
            $wasTrapped = [int]$Matches[1]
        }
        return [pscustomobject]@{
            Kind       = 'RETURN'
            TsUs       = $tsUs
            RelY       = $relY
            WasTrapped = $wasTrapped
            Excerpt    = (Format-SanitizedLogExcerpt -Line $Line)
        }
    }

    return $null
}

function New-LogTailState {
    param([string]$Path)
    $offset = [int64]0
    if ($Path -and (Test-Path -LiteralPath $Path -PathType Leaf)) {
        $offset = (Get-Item -LiteralPath $Path).Length
    }
    return [pscustomobject]@{
        Path   = $Path
        Offset = $offset
    }
}

function Read-NewKvmLogEvents {
    param([Parameter(Mandatory = $true)][object]$LogState)

    $events = New-Object System.Collections.Generic.List[object]
    if ([string]::IsNullOrWhiteSpace($LogState.Path) -or -not (Test-Path -LiteralPath $LogState.Path -PathType Leaf)) {
        return $events.ToArray()
    }

    $fs = $null
    $sr = $null
    try {
        $fs = New-Object System.IO.FileStream($LogState.Path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::ReadWrite)
        if ($fs.Length -lt $LogState.Offset) {
            # Log rotated.
            $LogState.Offset = [int64]0
        }
        if ($fs.Length -eq $LogState.Offset) {
            return $events.ToArray()
        }
        [void]$fs.Seek($LogState.Offset, [System.IO.SeekOrigin]::Begin)
        $sr = New-Object System.IO.StreamReader($fs, [System.Text.Encoding]::UTF8, $true)
        while (-not $sr.EndOfStream) {
            $line = $sr.ReadLine()
            if ($null -ne $line) {
                $ev = Parse-KvmDebugEvent -Line $line
                if ($null -ne $ev) {
                    $events.Add($ev)
                }
            }
        }
        $LogState.Offset = $fs.Position
    } finally {
        if ($null -ne $sr) { $sr.Dispose() }
        elseif ($null -ne $fs) { $fs.Dispose() }
    }

    return $events.ToArray()
}

function New-InputTracker {
    param([uint32]$BaselineTick)
    return [pscustomobject]@{
        BaselineTick        = $BaselineTick
        LastKnownScriptTick = $BaselineTick
        Windows             = New-Object System.Collections.Generic.List[object]
    }
}

function Assert-NoRealUserInput {
    param([Parameter(Mandatory = $true)][object]$Tracker)
    $currentTick = [AnyLeapE2E.Win32]::GetLastInputTick()
    $unexpected = Test-UnexpectedUserInput `
        -CurrentLastInputTick $currentTick `
        -BaselineTick $Tracker.BaselineTick `
        -LastKnownScriptTick $Tracker.LastKnownScriptTick `
        -InjectedWindows $Tracker.Windows.ToArray() `
        -ToleranceMs 60

    if ($unexpected) {
        throw [System.OperationCanceledException]::new("Real user input detected (GetLastInputInfo tick=$currentTick).")
    }
    $Tracker.LastKnownScriptTick = $currentTick
}

function Invoke-SafeSleepMs {
    param(
        [int]$Milliseconds,
        [Parameter(Mandatory = $true)][object]$Tracker
    )
    if ($Milliseconds -le 0) {
        Assert-NoRealUserInput -Tracker $Tracker
        return
    }

    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $Milliseconds) {
        Assert-NoRealUserInput -Tracker $Tracker
        $rem = $Milliseconds - [int]$sw.ElapsedMilliseconds
        if ($rem -le 0) { break }
        $chunk = if ($rem -lt 15) { $rem } else { 15 }
        [System.Threading.Thread]::Sleep($chunk)
    }
    Assert-NoRealUserInput -Tracker $Tracker
}

function Send-TrackedRelativeMove {
    param(
        [int]$Dx,
        [int]$Dy,
        [Parameter(Mandatory = $true)][object]$Tracker
    )

    Assert-NoRealUserInput -Tracker $Tracker

    $t0 = [AnyLeapE2E.Win32]::GetTickCount()
    $sent = [AnyLeapE2E.Win32]::SendRelativeMouseMove($Dx, $Dy)
    $t1 = [AnyLeapE2E.Win32]::GetTickCount()

    if ($sent -eq 0) {
        throw "SendInput relative move ($Dx, $Dy) returned 0."
    }

    $Tracker.Windows.Add([pscustomobject]@{
        StartTick = [uint32]$t0
        EndTick   = [uint32]$t1
    })
    if ($Tracker.Windows.Count -gt 24) {
        $Tracker.Windows.RemoveAt(0)
    }

    $afterTick = [AnyLeapE2E.Win32]::GetLastInputTick()
    if (-not (Test-UnexpectedUserInput -CurrentLastInputTick $afterTick -BaselineTick $Tracker.BaselineTick -LastKnownScriptTick $Tracker.LastKnownScriptTick -InjectedWindows $Tracker.Windows.ToArray() -ToleranceMs 60)) {
        $Tracker.LastKnownScriptTick = $afterTick
    }
}

function Test-NoEngageEarlyAbort {
    # True when the first $Threshold cycles all ran without a single engage.
    param(
        [int]$CyclesCompleted,
        [int]$EngagedCycles,
        [int]$Threshold = 5
    )
    return ($CyclesCompleted -ge $Threshold -and $EngagedCycles -eq 0)
}

function Write-SoakProgress {
    param(
        [string]$Path,
        [int]$Cycle,
        [int]$Total,
        [string]$Scenario,
        [string]$Outcome
    )
    if ([string]::IsNullOrWhiteSpace($Path)) { return }
    $line = '{0} cycle={1}/{2} scenario={3} outcome={4}' -f [DateTime]::UtcNow.ToString('o'), $Cycle, $Total, $Scenario, $Outcome
    Add-Content -LiteralPath $Path -Value $line -Encoding ASCII
}

function Invoke-SoakWithFlagFile {
    # Creates the flag file, runs $Body, and always removes the flag file.
    param(
        [Parameter(Mandatory = $true)][string]$FlagPath,
        [Parameter(Mandatory = $true)][scriptblock]$Body
    )
    try {
        [void](New-AnyLeapE2EFlagFile -Path $FlagPath)
        & $Body
    } finally {
        Remove-AnyLeapE2EFlagFile -Path $FlagPath
    }
}

function Get-LiveCursorPos {
    $pt = New-Object AnyLeapE2E.POINT
    [void][AnyLeapE2E.Win32]::GetCursorPos([ref]$pt)
    return [pscustomobject]@{ X = $pt.X; Y = $pt.Y }
}

function Test-CursorHiddenOrClipped {
    param([Parameter(Mandatory = $true)][object]$BaselineClip)

    $ci = [AnyLeapE2E.Win32]::ReadCursorInfo()
    $showing = ($ci.flags -band [AnyLeapE2E.Win32]::CURSOR_SHOWING) -ne 0
    if (-not $showing) {
        return $true
    }

    $clip = New-Object AnyLeapE2E.RECT
    if ([AnyLeapE2E.Win32]::GetClipCursor([ref]$clip)) {
        if ($clip.Left -ne $BaselineClip.Left -or $clip.Top -ne $BaselineClip.Top -or $clip.Right -ne $BaselineClip.Right -or $clip.Bottom -ne $BaselineClip.Bottom) {
            return $true
        }
    }
    return $false
}

function Move-CursorNearSeam {
    param(
        [Parameter(Mandatory = $true)][object]$Monitor,
        [Parameter(Mandatory = $true)][string]$Edge,
        [Parameter(Mandatory = $true)][object]$Dir,
        [int]$TargetAlong,
        [int]$InwardOffsetPx,
        [Parameter(Mandatory = $true)][object]$Tracker
    )

    $targetX = 0
    $targetY = 0
    switch ($Edge) {
        'left'   { $targetX = $Monitor.Left + $InwardOffsetPx; $targetY = $TargetAlong }
        'right'  { $targetX = ($Monitor.Right - 1) - $InwardOffsetPx; $targetY = $TargetAlong }
        'top'    { $targetX = $TargetAlong; $targetY = $Monitor.Top + $InwardOffsetPx }
        'bottom' { $targetX = $TargetAlong; $targetY = ($Monitor.Bottom - 1) - $InwardOffsetPx }
    }

    for ($step = 0; $step -lt 120; $step++) {
        $pos = Get-LiveCursorPos
        $dx = $targetX - $pos.X
        $dy = $targetY - $pos.Y
        if ([Math]::Abs($dx) -le 6 -and [Math]::Abs($dy) -le 6) {
            break
        }
        $stepX = if ($dx -gt 40) { 40 } elseif ($dx -lt -40) { -40 } else { $dx }
        $stepY = if ($dy -gt 40) { 40 } elseif ($dy -lt -40) { -40 } else { $dy }
        Send-TrackedRelativeMove -Dx $stepX -Dy $stepY -Tracker $Tracker
        Invoke-SafeSleepMs -Milliseconds 4 -Tracker $Tracker
    }

    # Ensure cursor is at least slightly away from the seam so the leap crosses freshly.
    $cur = Get-LiveCursorPos
    if ($Dir.Axis -eq 'x') {
        $dist = if ($Edge -eq 'left') { $cur.X - $Monitor.Left } else { ($Monitor.Right - 1) - $cur.X }
        if ($dist -lt 25) {
            Send-TrackedRelativeMove -Dx ($Dir.InX * 40) -Dy 0 -Tracker $Tracker
            Invoke-SafeSleepMs -Milliseconds 10 -Tracker $Tracker
        }
    } else {
        $dist = if ($Edge -eq 'top') { $cur.Y - $Monitor.Top } else { ($Monitor.Bottom - 1) - $cur.Y }
        if ($dist -lt 25) {
            Send-TrackedRelativeMove -Dx 0 -Dy ($Dir.InY * 40) -Tracker $Tracker
            Invoke-SafeSleepMs -Milliseconds 10 -Tracker $Tracker
        }
    }
}

function Invoke-PushUntilEngage {
    param(
        [Parameter(Mandatory = $true)][object]$Dir,
        [int]$StepPx,
        [int]$StepDelayMs,
        [int]$TimeoutMs,
        [Parameter(Mandatory = $true)][object]$LogState,
        [Parameter(Mandatory = $true)][object]$BaselineClip,
        [Parameter(Mandatory = $true)][object]$Tracker
    )

    $excerpts = New-Object System.Collections.Generic.List[string]
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $engaged = $false
    $detectedBy = $null
    $engageRelY = $null

    while ($sw.ElapsedMilliseconds -lt $TimeoutMs) {
        Send-TrackedRelativeMove -Dx ($Dir.OutX * $StepPx) -Dy ($Dir.OutY * $StepPx) -Tracker $Tracker
        Invoke-SafeSleepMs -Milliseconds $StepDelayMs -Tracker $Tracker

        $events = Read-NewKvmLogEvents -LogState $LogState
        foreach ($ev in $events) {
            $excerpts.Add($ev.Excerpt)
            if ($ev.Kind -eq 'ENGAGE_PERFORM') {
                $engaged = $true
                $detectedBy = 'log'
                $engageRelY = $ev.RelY
            }
        }
        if ($engaged) { break }

        if (Test-CursorHiddenOrClipped -BaselineClip $BaselineClip) {
            $engaged = $true
            $detectedBy = 'cursor_state'
            break
        }
    }

    return [pscustomobject]@{
        Engaged    = $engaged
        DetectedBy = $detectedBy
        RelY       = $engageRelY
        Excerpts   = $excerpts.ToArray()
    }
}

function Invoke-MoveLinkedAndReturn {
    param(
        [Parameter(Mandatory = $true)][object]$Dir,
        [int]$IntoLinkedPx,
        [int]$StepPx,
        [int]$StepDelayMs,
        [int]$PauseMs,
        [int]$TimeoutMs,
        [Parameter(Mandatory = $true)][object]$LogState,
        [Parameter(Mandatory = $true)][object]$BaselineClip,
        [Parameter(Mandatory = $true)][object]$Tracker
    )

    $excerpts = New-Object System.Collections.Generic.List[string]
    $returned = $false
    $detectedBy = $null
    $returnRelY = $null

    # Move further into Linked PC away from entry edge (same direction as OutX/OutY for opposite-edge topology).
    $stepsIn = [Math]::Max(1, [int][Math]::Ceiling($IntoLinkedPx / [double]$StepPx))
    for ($s = 0; $s -lt $stepsIn; $s++) {
        Send-TrackedRelativeMove -Dx ($Dir.OutX * $StepPx) -Dy ($Dir.OutY * $StepPx) -Tracker $Tracker
        Invoke-SafeSleepMs -Milliseconds $StepDelayMs -Tracker $Tracker

        $events = Read-NewKvmLogEvents -LogState $LogState
        foreach ($ev in $events) {
            $excerpts.Add($ev.Excerpt)
            if ($ev.Kind -eq 'RETURN') {
                $returned = $true
                $detectedBy = 'log'
                $returnRelY = $ev.RelY
            }
        }
        if ($returned) { break }
    }

    if (-not $returned) {
        Invoke-SafeSleepMs -Milliseconds $PauseMs -Tracker $Tracker
        $events = Read-NewKvmLogEvents -LogState $LogState
        foreach ($ev in $events) {
            $excerpts.Add($ev.Excerpt)
            if ($ev.Kind -eq 'RETURN') {
                $returned = $true
                $detectedBy = 'log'
                $returnRelY = $ev.RelY
            }
        }
    }

    # Move back toward seam (InX/InY) until Linked PC returns the cursor.
    if (-not $returned) {
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        while ($sw.ElapsedMilliseconds -lt $TimeoutMs) {
            Send-TrackedRelativeMove -Dx ($Dir.InX * $StepPx) -Dy ($Dir.InY * $StepPx) -Tracker $Tracker
            Invoke-SafeSleepMs -Milliseconds $StepDelayMs -Tracker $Tracker

            $events = Read-NewKvmLogEvents -LogState $LogState
            foreach ($ev in $events) {
                $excerpts.Add($ev.Excerpt)
                if ($ev.Kind -eq 'RETURN') {
                    $returned = $true
                    $detectedBy = 'log'
                    $returnRelY = $ev.RelY
                }
            }
            if ($returned) { break }

            # Fallback detection when debug log is unavailable.
            if ([string]::IsNullOrWhiteSpace($LogState.Path) -or -not (Test-Path -LiteralPath $LogState.Path -PathType Leaf)) {
                if (-not (Test-CursorHiddenOrClipped -BaselineClip $BaselineClip)) {
                    $returned = $true
                    $detectedBy = 'cursor_state'
                    break
                }
            }
        }
    }

    return [pscustomobject]@{
        Returned   = $returned
        DetectedBy = $detectedBy
        RelY       = $returnRelY
        Excerpts   = $excerpts.ToArray()
    }
}

function Invoke-KvmSoakDryRun {
    Write-Host "=== kvm-soak.ps1 -DryRun ==="
    Initialize-AnyLeapWin32

    # 1. Argument & scenario validation
    Assert-DryRunCondition ($Cycles -ge 1) "Default/provided Cycles >= 1 ($Cycles)"
    Assert-DryRunCondition ((Get-CycleScenarioName 0) -eq 'slow') "Cycle 0 maps to slow"
    Assert-DryRunCondition ((Get-CycleScenarioName 1) -eq 'fast_flick') "Cycle 1 maps to fast_flick"
    Assert-DryRunCondition ((Get-CycleScenarioName 2) -eq 'immediate_releap') "Cycle 2 maps to immediate_releap"

    # 2. Idle guard synthetic test
    $idleAbort = Invoke-IdleGuard -IdleThresholdSec 120 -PollIntervalSec 15 -MaxWaitMinutes 0 `
        -GetIdleStateFn { [pscustomobject]@{ IdleSeconds = 45.0; LastInputTick = [uint32]1234; NowTick = [uint32]46234 } }
    Assert-DryRunCondition ((-not $idleAbort.Ok) -and $idleAbort.Status -eq 'user_active') "IdleGuard aborts with user_active when active"

    # 3. Edge monitor selection (multi-monitor layout)
    $mons = @(
        [pscustomobject]@{ Left = 0;     Top = 0; Right = 1920; Bottom = 1080; Width = 1920; Height = 1080; IsPrimary = $true },
        [pscustomobject]@{ Left = -1920; Top = 0; Right = 0;    Bottom = 1080; Width = 1920; Height = 1080; IsPrimary = $false }
    )
    $leftMon = Get-EdgeMonitor -Monitors $mons -Edge 'left'
    $rightMon = Get-EdgeMonitor -Monitors $mons -Edge 'right'
    Assert-DryRunCondition ($leftMon.Left -eq -1920) "Selects leftmost monitor (-1920..0) for SeamEdge=left"
    Assert-DryRunCondition ($rightMon.Right -eq 1920) "Selects rightmost monitor (0..1920) for SeamEdge=right"

    # 4. Classification math on 1920x1080 monitor
    $mon1080 = [pscustomobject]@{ Left = 0; Top = 0; Right = 1920; Bottom = 1080; Width = 1920; Height = 1080; IsPrimary = $true }
    Assert-DryRunCondition ((Test-CursorClassification -X 2 -Y 540 -Monitor $mon1080 -Edge 'left' -ExpectedAlongCoord 540 -TolerancePx 40) -eq 'edge_ok') "Classifies (2,540) at expected y=540 as edge_ok"
    Assert-DryRunCondition ((Test-CursorClassification -X 40 -Y 580 -Monitor $mon1080 -Edge 'left' -ExpectedAlongCoord 540 -TolerancePx 40) -eq 'edge_ok') "Classifies boundary (40,580) within 40px as edge_ok"
    Assert-DryRunCondition ((Test-CursorClassification -X 960 -Y 540 -Monitor $mon1080 -Edge 'left' -ExpectedAlongCoord 540 -TolerancePx 40) -eq 'center') "Classifies (960,540) as center"
    Assert-DryRunCondition ((Test-CursorClassification -X 672 -Y 378 -Monitor $mon1080 -Edge 'left' -ExpectedAlongCoord 540 -TolerancePx 40) -eq 'center') "Classifies central 30% corner (672,378) as center"
    Assert-DryRunCondition ((Test-CursorClassification -X 10 -Y 900 -Monitor $mon1080 -Edge 'left' -ExpectedAlongCoord 540 -TolerancePx 40) -eq 'other') "Classifies wrong-Y edge (10,900) as other"
    Assert-DryRunCondition ((Test-CursorClassification -X 1800 -Y 100 -Monitor $mon1080 -Edge 'left' -ExpectedAlongCoord 540 -TolerancePx 40) -eq 'other') "Classifies far corner (1800,100) as other"

    # 5. Cycle classification rollup
    Assert-DryRunCondition ((Get-CycleClassification -Class0 'edge_ok' -Class50 'edge_ok' -Class250 'edge_ok') -eq 'edge_ok') "Cycle rollup: all edge_ok -> edge_ok"
    Assert-DryRunCondition ((Get-CycleClassification -Class0 'edge_ok' -Class50 'center' -Class250 'edge_ok') -eq 'center') "Cycle rollup: any center -> center"
    Assert-DryRunCondition ((Get-CycleClassification -Class0 'edge_ok' -Class50 'other' -Class250 'edge_ok') -eq 'other') "Cycle rollup: non-center non-edge -> other"

    # 6. Debug log parsing & privacy sanitization
    $rawEngage = 'ts_us=1731700001234 event=ENGAGE phase=perform edge=left rel_y=0.5000 released_count=0 released_vks=[] tracker_before=[] send_input_us=12 hwnd=0x1234 proc="secret.exe" title="Private Title"'
    $rawReturn = 'ts_us=1731700004567 event=RETURN rel_y=0.5010 was_trapped=1 hwnd=0x1234 proc="secret.exe" title="Private Title"'
    $parsedEngage = Parse-KvmDebugEvent -Line $rawEngage
    $parsedReturn = Parse-KvmDebugEvent -Line $rawReturn
    Assert-DryRunCondition ($null -ne $parsedEngage -and $parsedEngage.Kind -eq 'ENGAGE_PERFORM' -and $parsedEngage.TsUs -eq 1731700001234) "Parses ENGAGE phase=perform log line"
    Assert-DryRunCondition ($null -ne $parsedReturn -and $parsedReturn.Kind -eq 'RETURN' -and $parsedReturn.WasTrapped -eq 1) "Parses RETURN log line"
    Assert-DryRunCondition (($parsedEngage.Excerpt -notlike '*Private Title*') -and ($parsedEngage.Excerpt -notlike '*secret.exe*') -and ($parsedEngage.Excerpt -like '*event=ENGAGE*phase=perform*')) "Sanitizes window title and process from log excerpts"

    # 5. Early-abort logic (no engage in the first 5 cycles)
    Assert-DryRunCondition (-not (Test-NoEngageEarlyAbort -CyclesCompleted 4 -EngagedCycles 0)) "Early-abort: 4 cycles without engage does not abort yet"
    Assert-DryRunCondition (Test-NoEngageEarlyAbort -CyclesCompleted 5 -EngagedCycles 0) "Early-abort: 5 cycles without engage aborts"
    Assert-DryRunCondition (-not (Test-NoEngageEarlyAbort -CyclesCompleted 9 -EngagedCycles 1)) "Early-abort: any engage disables the abort"

    # 6. Flag-file cleanup, including when the body throws
    $dryDir = Join-Path ([System.IO.Path]::GetTempPath()) ("anyleap-soak-dry-" + [guid]::NewGuid().ToString('N'))
    $dryFlag = Join-Path $dryDir 'e2e-accept-injected'
    $script:seenDuring = $false
    Invoke-SoakWithFlagFile -FlagPath $dryFlag -Body { $script:seenDuring = (Test-Path -LiteralPath $dryFlag) }
    Assert-DryRunCondition ($script:seenDuring -and -not (Test-Path -LiteralPath $dryFlag)) "Flag file exists during the run and is removed after"
    $threw = $false
    try {
        Invoke-SoakWithFlagFile -FlagPath $dryFlag -Body { throw [System.OperationCanceledException]::new('simulated abort') }
    } catch [System.OperationCanceledException] { $threw = $true }
    Assert-DryRunCondition ($threw -and -not (Test-Path -LiteralPath $dryFlag)) "Flag file is removed when the run aborts with an exception"

    # 7. Progress log
    $dryLog = Join-Path $dryDir 'kvm-soak-progress.log'
    Write-SoakProgress -Path $dryLog -Cycle 1 -Total 3 -Scenario 'slow' -Outcome 'engage_timeout'
    Write-SoakProgress -Path $dryLog -Cycle 2 -Total 3 -Scenario 'fast_flick' -Outcome 'edge_ok'
    $logLines = @(Get-Content -LiteralPath $dryLog)
    Assert-DryRunCondition ($logLines.Count -eq 2 -and $logLines[0] -like '*cycle=1/3 scenario=slow outcome=engage_timeout') "Progress log gets one line per cycle"
    Remove-Item -LiteralPath $dryDir -Recurse -Force -ErrorAction SilentlyContinue

    Write-Host "kvm-soak.ps1 -DryRun PASSED"
}

if ($DryRun) {
    Invoke-KvmSoakDryRun
    exit 0
}

if ($Cycles -lt 1) {
    throw "Cycles must be >= 1."
}

Initialize-AnyLeapWin32
$startedAtUtc = [DateTime]::UtcNow.ToString('o')
$swTotal = [System.Diagnostics.Stopwatch]::StartNew()

$monitors = [AnyLeapE2E.Win32]::GetMonitors()
$edgeMonitor = Get-EdgeMonitor -Monitors $monitors -Edge $SeamEdge
$dir = Get-SeamDirection -Edge $SeamEdge

$baselineClip = New-Object AnyLeapE2E.RECT
[void][AnyLeapE2E.Win32]::GetClipCursor([ref]$baselineClip)

# 0. Fail fast if AnyLeap is not running (nothing to leap with).
if (-not (Test-AnyLeapRunning)) {
    $swTotal.Stop()
    Write-JsonFile -Path $ResultPath -Data ([pscustomobject]@{
        status          = 'anyleap_not_running'
        startedAtUtc    = $startedAtUtc
        finishedAtUtc   = [DateTime]::UtcNow.ToString('o')
        requestedCycles = $Cycles
        completedCycles = 0
    })
    Write-Warning "AnyLeap (anyleap.exe) is not running; start it and link both PCs first."
    exit 4
}

# 1. Idle guard before touching mouse
$idleRes = Invoke-IdleGuard -IdleThresholdSec $IdleThresholdSec -PollIntervalSec $PollIntervalSec -MaxWaitMinutes $MaxIdleWaitMinutes
if (-not $idleRes.Ok) {
    $swTotal.Stop()
    $abortObj = [pscustomobject]@{
        status          = $idleRes.Status
        startedAtUtc    = $startedAtUtc
        finishedAtUtc   = [DateTime]::UtcNow.ToString('o')
        durationSec     = [Math]::Round($swTotal.Elapsed.TotalSeconds, 2)
        seamEdge        = $SeamEdge
        requestedCycles = $Cycles
        completedCycles = 0
        idleSeconds     = $idleRes.IdleSeconds
    }
    Write-JsonFile -Path $ResultPath -Data $abortObj
    Write-Warning "Aborted: user is active (idle $($idleRes.IdleSeconds)s)."
    exit 2
}

$tracker = New-InputTracker -BaselineTick $idleRes.BaselineLastInputTick
$logState = New-LogTailState -Path $LogPath
$logExists = (-not [string]::IsNullOrWhiteSpace($LogPath)) -and (Test-Path -LiteralPath $LogPath -PathType Leaf)

$counts = @{
    edge_ok        = 0
    center         = 0
    other          = 0
    engage_timeout = 0
    return_timeout = 0
}
$sampleCounts = @{
    t0ms   = @{ edge_ok = 0; center = 0; other = 0 }
    t50ms  = @{ edge_ok = 0; center = 0; other = 0 }
    t250ms = @{ edge_ok = 0; center = 0; other = 0 }
}
$scenarioStats = @{}
foreach ($sc in @('slow', 'fast_flick', 'immediate_releap')) {
    $scenarioStats[$sc] = @{
        total          = 0
        edge_ok        = 0
        center         = 0
        other          = 0
        engage_timeout = 0
        return_timeout = 0
    }
}

$progressLogPath = Join-Path (Split-Path -Parent $ResultPath) 'kvm-soak-progress.log'
Set-Content -LiteralPath $progressLogPath -Value @() -Encoding ASCII
$engagedCycles = 0
$flagPath = Get-AnyLeapE2EFlagPath

$failures = New-Object System.Collections.Generic.List[object]
$allLogExcerpts = New-Object System.Collections.Generic.List[string]
$yFractions = @(0.20, 0.35, 0.50, 0.65, 0.80)
$completedCycles = 0
$finalStatus = 'completed'
$abortReason = $null

try {
    # Opt in to tagged injected input; removed in finally even on abort/Ctrl+C.
    [void](New-AnyLeapE2EFlagFile -Path $flagPath)
    # Allow AnyLeap's hook thread (watchdog timer, 250 ms) to pick up the flag.
    Invoke-SafeSleepMs -Milliseconds 600 -Tracker $tracker
    for ($i = 0; $i -lt $Cycles; $i++) {
        $scenario = Get-CycleScenarioName -CycleIndex $i
        $scenarioStats[$scenario].total++
        $cycleExcerpts = New-Object System.Collections.Generic.List[string]

        $frac = $yFractions[$i % $yFractions.Count]
        if ($dir.Axis -eq 'x') {
            $targetAlong = [int]($edgeMonitor.Top + ($edgeMonitor.Height * $frac))
        } else {
            $targetAlong = [int]($edgeMonitor.Left + ($edgeMonitor.Width * $frac))
        }

        # Drain any stale log lines before this cycle
        $null = Read-NewKvmLogEvents -LogState $logState

        if ($scenario -eq 'immediate_releap') {
            # Warm-up leap + return, then immediate re-leap within 100-200 ms.
            Move-CursorNearSeam -Monitor $edgeMonitor -Edge $SeamEdge -Dir $dir -TargetAlong $targetAlong -InwardOffsetPx 70 -Tracker $Tracker
            $warmEngage = Invoke-PushUntilEngage -Dir $dir -StepPx 18 -StepDelayMs 5 -TimeoutMs $EngageTimeoutMs -LogState $logState -BaselineClip $baselineClip -Tracker $tracker
            foreach ($ex in $warmEngage.Excerpts) { $cycleExcerpts.Add($ex); $allLogExcerpts.Add($ex) }
            if ($warmEngage.Engaged) {
                $warmRet = Invoke-MoveLinkedAndReturn -Dir $dir -IntoLinkedPx 100 -StepPx 22 -StepDelayMs 5 -PauseMs 40 -TimeoutMs $ReturnTimeoutMs -LogState $logState -BaselineClip $baselineClip -Tracker $tracker
                foreach ($ex in $warmRet.Excerpts) { $cycleExcerpts.Add($ex); $allLogExcerpts.Add($ex) }
            }
            # Provoke immediate re-leap within 100-200 ms after RETURN.
            Invoke-SafeSleepMs -Milliseconds 130 -Tracker $tracker
            $prePos = Get-LiveCursorPos
            $expectedAlong = if ($dir.Axis -eq 'x') { $prePos.Y } else { $prePos.X }
            $eng = Invoke-PushUntilEngage -Dir $dir -StepPx 18 -StepDelayMs 4 -TimeoutMs $EngageTimeoutMs -LogState $logState -BaselineClip $baselineClip -Tracker $tracker
            foreach ($ex in $eng.Excerpts) { $cycleExcerpts.Add($ex); $allLogExcerpts.Add($ex) }
            $retIntoPx = 120; $retStepPx = 24; $retDelayMs = 4; $retPauseMs = 40
        } elseif ($scenario -eq 'fast_flick') {
            Move-CursorNearSeam -Monitor $edgeMonitor -Edge $SeamEdge -Dir $dir -TargetAlong $targetAlong -InwardOffsetPx 120 -Tracker $tracker
            $prePos = Get-LiveCursorPos
            $expectedAlong = if ($dir.Axis -eq 'x') { $prePos.Y } else { $prePos.X }
            $eng = Invoke-PushUntilEngage -Dir $dir -StepPx 32 -StepDelayMs 2 -TimeoutMs $EngageTimeoutMs -LogState $logState -BaselineClip $baselineClip -Tracker $tracker
            foreach ($ex in $eng.Excerpts) { $cycleExcerpts.Add($ex); $allLogExcerpts.Add($ex) }
            $retIntoPx = 180; $retStepPx = 32; $retDelayMs = 2; $retPauseMs = 40
        } else {
            # slow
            Move-CursorNearSeam -Monitor $edgeMonitor -Edge $SeamEdge -Dir $dir -TargetAlong $targetAlong -InwardOffsetPx 60 -Tracker $tracker
            $prePos = Get-LiveCursorPos
            $expectedAlong = if ($dir.Axis -eq 'x') { $prePos.Y } else { $prePos.X }
            $eng = Invoke-PushUntilEngage -Dir $dir -StepPx 8 -StepDelayMs 18 -TimeoutMs $EngageTimeoutMs -LogState $logState -BaselineClip $baselineClip -Tracker $tracker
            foreach ($ex in $eng.Excerpts) { $cycleExcerpts.Add($ex); $allLogExcerpts.Add($ex) }
            $retIntoPx = 140; $retStepPx = 14; $retDelayMs = 15; $retPauseMs = 100
        }

        if (-not $eng.Engaged) {
            $counts.engage_timeout++
            $scenarioStats[$scenario].engage_timeout++
            $failures.Add([pscustomobject]@{
                cycle        = $i + 1
                scenario     = $scenario
                class        = 'engage_timeout'
                timestampUtc = [DateTime]::UtcNow.ToString('o')
                expectedAlong = $expectedAlong
                samples      = $null
                logExcerpts  = $cycleExcerpts.ToArray()
            })
            $completedCycles++
            Write-SoakProgress -Path $progressLogPath -Cycle ($i + 1) -Total $Cycles -Scenario $scenario -Outcome 'engage_timeout'
            if (Test-NoEngageEarlyAbort -CyclesCompleted $completedCycles -EngagedCycles $engagedCycles) {
                $finalStatus = 'no_engage'
                $abortReason = "No leap engaged in the first $completedCycles cycles (no cursor confinement or relocation within EngageTimeoutMs=$EngageTimeoutMs). Check AnyLeap is running, linked, and the build includes the e2e-accept-injected hook."
                Write-Warning $abortReason
                break
            }
            Invoke-SafeSleepMs -Milliseconds 150 -Tracker $tracker
            continue
        }

        $engagedCycles++
        $ret = Invoke-MoveLinkedAndReturn -Dir $dir -IntoLinkedPx $retIntoPx -StepPx $retStepPx -StepDelayMs $retDelayMs -PauseMs $retPauseMs -TimeoutMs $ReturnTimeoutMs -LogState $logState -BaselineClip $baselineClip -Tracker $tracker
        foreach ($ex in $ret.Excerpts) { $cycleExcerpts.Add($ex); $allLogExcerpts.Add($ex) }

        if (-not $ret.Returned) {
            $counts.return_timeout++
            $scenarioStats[$scenario].return_timeout++
            $failures.Add([pscustomobject]@{
                cycle         = $i + 1
                scenario      = $scenario
                class         = 'return_timeout'
                timestampUtc  = [DateTime]::UtcNow.ToString('o')
                expectedAlong = $expectedAlong
                samples       = $null
                logExcerpts   = $cycleExcerpts.ToArray()
            })
            $completedCycles++
            Write-SoakProgress -Path $progressLogPath -Cycle ($i + 1) -Total $Cycles -Scenario $scenario -Outcome 'return_timeout'
            Invoke-SafeSleepMs -Milliseconds 200 -Tracker $tracker
            continue
        }

        # Use rel_y from RETURN log when available to compute exact expected Y on edge monitor.
        if ($null -ne $ret.RelY) {
            if ($dir.Axis -eq 'x') {
                $expectedAlong = [int][Math]::Round($edgeMonitor.Top + ($ret.RelY * $edgeMonitor.Height))
            } else {
                $expectedAlong = [int][Math]::Round($edgeMonitor.Left + ($ret.RelY * $edgeMonitor.Width))
            }
        }

        # Sample GetCursorPos at +0 ms, +50 ms, and +250 ms after RETURN.
        $sampleSw = [System.Diagnostics.Stopwatch]::StartNew()
        Assert-NoRealUserInput -Tracker $tracker
        $p0 = Get-LiveCursorPos

        $rem50 = 50 - [int]$sampleSw.ElapsedMilliseconds
        if ($rem50 -gt 0) { Invoke-SafeSleepMs -Milliseconds $rem50 -Tracker $tracker } else { Assert-NoRealUserInput -Tracker $tracker }
        $p50 = Get-LiveCursorPos

        $rem250 = 250 - [int]$sampleSw.ElapsedMilliseconds
        if ($rem250 -gt 0) { Invoke-SafeSleepMs -Milliseconds $rem250 -Tracker $tracker } else { Assert-NoRealUserInput -Tracker $tracker }
        $p250 = Get-LiveCursorPos

        $c0   = Test-CursorClassification -X $p0.X   -Y $p0.Y   -Monitor $edgeMonitor -Edge $SeamEdge -ExpectedAlongCoord $expectedAlong -TolerancePx $EdgeTolerancePx
        $c50  = Test-CursorClassification -X $p50.X  -Y $p50.Y  -Monitor $edgeMonitor -Edge $SeamEdge -ExpectedAlongCoord $expectedAlong -TolerancePx $EdgeTolerancePx
        $c250 = Test-CursorClassification -X $p250.X -Y $p250.Y -Monitor $edgeMonitor -Edge $SeamEdge -ExpectedAlongCoord $expectedAlong -TolerancePx $EdgeTolerancePx

        $sampleCounts.t0ms[$c0]++
        $sampleCounts.t50ms[$c50]++
        $sampleCounts.t250ms[$c250]++

        $cycleClass = Get-CycleClassification -Class0 $c0 -Class50 $c50 -Class250 $c250
        $counts[$cycleClass]++
        $scenarioStats[$scenario][$cycleClass]++

        if ($cycleClass -ne 'edge_ok') {
            $failures.Add([pscustomobject]@{
                cycle         = $i + 1
                scenario      = $scenario
                class         = $cycleClass
                timestampUtc  = [DateTime]::UtcNow.ToString('o')
                expectedAlong = $expectedAlong
                samples       = [pscustomobject]@{
                    t0ms   = [pscustomobject]@{ x = $p0.X;   y = $p0.Y;   class = $c0 }
                    t50ms  = [pscustomobject]@{ x = $p50.X;  y = $p50.Y;  class = $c50 }
                    t250ms = [pscustomobject]@{ x = $p250.X; y = $p250.Y; class = $c250 }
                }
                logExcerpts   = $cycleExcerpts.ToArray()
            })
        }

        $completedCycles++
        Write-SoakProgress -Path $progressLogPath -Cycle ($i + 1) -Total $Cycles -Scenario $scenario -Outcome $cycleClass

        # Inter-cycle cadence pause
        if ($scenario -eq 'slow') {
            Invoke-SafeSleepMs -Milliseconds 250 -Tracker $tracker
        } elseif ($scenario -eq 'fast_flick') {
            Invoke-SafeSleepMs -Milliseconds 150 -Tracker $tracker
        } else {
            Invoke-SafeSleepMs -Milliseconds 100 -Tracker $tracker
        }
    }
} catch [System.OperationCanceledException] {
    $finalStatus = 'aborted_user_input'
    $abortReason = $_.Exception.Message
    Write-Warning "Soak test aborted immediately: $abortReason"
} finally {
    Remove-AnyLeapE2EFlagFile -Path $flagPath
}

$swTotal.Stop()
$resultObj = [pscustomobject]@{
    status          = $finalStatus
    abortReason     = $abortReason
    startedAtUtc    = $startedAtUtc
    finishedAtUtc   = [DateTime]::UtcNow.ToString('o')
    durationSec     = [Math]::Round($swTotal.Elapsed.TotalSeconds, 2)
    seamEdge        = $SeamEdge
    progressLogPath = $progressLogPath
    logPath         = $LogPath
    logFound        = $logExists
    requestedCycles = $Cycles
    completedCycles = $completedCycles
    monitor         = [pscustomobject]@{
        left      = $edgeMonitor.Left
        top       = $edgeMonitor.Top
        right     = $edgeMonitor.Right
        bottom    = $edgeMonitor.Bottom
        width     = $edgeMonitor.Width
        height    = $edgeMonitor.Height
        isPrimary = $edgeMonitor.IsPrimary
    }
    counts          = [pscustomobject]$counts
    sampleCounts    = [pscustomobject]@{
        t0ms   = [pscustomobject]$sampleCounts.t0ms
        t50ms  = [pscustomobject]$sampleCounts.t50ms
        t250ms = [pscustomobject]$sampleCounts.t250ms
    }
    scenarios       = [pscustomobject]@{
        slow             = [pscustomobject]$scenarioStats['slow']
        fast_flick       = [pscustomobject]$scenarioStats['fast_flick']
        immediate_releap = [pscustomobject]$scenarioStats['immediate_releap']
    }
    failures        = $failures.ToArray()
    logExcerpts     = $allLogExcerpts.ToArray()
}

Write-JsonFile -Path $ResultPath -Data $resultObj
Write-Host ("Soak test finished (status={0}, completed={1}/{2}, edge_ok={3}, center={4}, other={5}). Result: {6}" -f `
    $finalStatus, $completedCycles, $Cycles, $counts.edge_ok, $counts.center, $counts.other, $ResultPath)

if ($finalStatus -ne 'completed') {
    exit 3
}
exit 0

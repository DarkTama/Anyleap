<#
.SYNOPSIS
  Shared helpers for AnyLeap two-PC E2E automation (Windows PowerShell 5.1).
  Dot-source this file from other scripts in scripts/e2e/, or run with -DryRun.
#>

Set-StrictMode -Version Latest

# dwExtraInfo magic stamped on every SendInput move so AnyLeap (when the opt-in
# e2e-accept-injected flag file exists) treats it as physical. Must match
# ANYLEAP_E2E_INPUT_TAG in src-tauri/src/kvm/hooks.rs (and the C# constant below).
$script:AnyLeapE2EInputTag = [uint64]4705796814379762993  # 0x414E594532454531

function Get-AnyLeapE2EFlagPath {
    if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) { return $null }
    return (Join-Path (Join-Path $env:LOCALAPPDATA 'id.co.dak.anyleap') 'e2e-accept-injected')
}

function New-AnyLeapE2EFlagFile {
    param([string]$Path = (Get-AnyLeapE2EFlagPath))
    if ([string]::IsNullOrWhiteSpace($Path)) { throw "LOCALAPPDATA is not set; cannot create e2e flag file." }
    $parent = Split-Path -Parent $Path
    if (-not (Test-Path -LiteralPath $parent)) { New-Item -ItemType Directory -Path $parent -Force | Out-Null }
    Set-Content -LiteralPath $Path -Value 'anyleap e2e soak: accept tagged injected mouse input' -Encoding ASCII
    return $Path
}

function Remove-AnyLeapE2EFlagFile {
    param([string]$Path = (Get-AnyLeapE2EFlagPath))
    if ([string]::IsNullOrWhiteSpace($Path)) { return }
    Remove-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
}

function Test-AnyLeapRunning {
    return [bool](Get-Process -Name 'anyleap' -ErrorAction SilentlyContinue)
}

function Initialize-AnyLeapWin32 {
    if (-not ('AnyLeapE2E.Win32' -as [type])) {
        $csharp = @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

namespace AnyLeapE2E {
    [StructLayout(LayoutKind.Sequential)]
    public struct POINT {
        public int X;
        public int Y;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct RECT {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct LASTINPUTINFO {
        public uint cbSize;
        public uint dwTime;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct CURSORINFO {
        public uint cbSize;
        public uint flags;
        public IntPtr hCursor;
        public POINT ptScreenPos;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct MONITORINFO {
        public uint cbSize;
        public RECT rcMonitor;
        public RECT rcWork;
        public uint dwFlags;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct MOUSEINPUT {
        public int dx;
        public int dy;
        public uint mouseData;
        public uint dwFlags;
        public uint time;
        public UIntPtr dwExtraInfo;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct KEYBDINPUT {
        public ushort wVk;
        public ushort wScan;
        public uint dwFlags;
        public uint time;
        public UIntPtr dwExtraInfo;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct HARDWAREINPUT {
        public uint uMsg;
        public ushort wParamL;
        public ushort wParamH;
    }

    [StructLayout(LayoutKind.Explicit)]
    public struct INPUTUNION {
        [FieldOffset(0)] public MOUSEINPUT mi;
        [FieldOffset(0)] public KEYBDINPUT ki;
        [FieldOffset(0)] public HARDWAREINPUT hi;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct INPUT {
        public uint type;
        public INPUTUNION U;
    }

    public class MonitorData {
        public int Left { get; set; }
        public int Top { get; set; }
        public int Right { get; set; }
        public int Bottom { get; set; }
        public int Width { get; set; }
        public int Height { get; set; }
        public bool IsPrimary { get; set; }
    }

    public static class Win32 {
        public const uint INPUT_MOUSE = 0;
        public const uint MOUSEEVENTF_MOVE = 0x0001;
        public const uint CURSOR_SHOWING = 0x00000001;
        public const uint CURSOR_SUPPRESSED = 0x00000002;
        public const uint MONITORINFOF_PRIMARY = 0x00000001;

        private delegate bool MonitorEnumProc(IntPtr hMonitor, IntPtr hdcMonitor, ref RECT lprcMonitor, IntPtr dwData);

        [DllImport("user32.dll", SetLastError = true)]
        private static extern bool GetLastInputInfo(ref LASTINPUTINFO plii);

        [DllImport("kernel32.dll")]
        public static extern uint GetTickCount();

        [DllImport("user32.dll", SetLastError = true)]
        public static extern bool GetCursorPos(out POINT lpPoint);

        [DllImport("user32.dll", SetLastError = true)]
        private static extern bool GetCursorInfo(ref CURSORINFO pci);

        [DllImport("user32.dll", SetLastError = true)]
        public static extern bool GetClipCursor(out RECT lpRect);

        [DllImport("user32.dll")]
        public static extern int GetSystemMetrics(int nIndex);

        [DllImport("user32.dll", SetLastError = true)]
        private static extern uint SendInput(uint nInputs, INPUT[] pInputs, int cbSize);

        [DllImport("user32.dll")]
        private static extern bool EnumDisplayMonitors(IntPtr hdc, IntPtr lprcClip, MonitorEnumProc lpfnEnum, IntPtr dwData);

        [DllImport("user32.dll", CharSet = CharSet.Unicode)]
        private static extern bool GetMonitorInfoW(IntPtr hMonitor, ref MONITORINFO lpmi);

        [DllImport("user32.dll")]
        private static extern bool SetProcessDpiAwarenessContext(IntPtr dpiContext);

        [DllImport("user32.dll")]
        private static extern bool SetProcessDPIAware();

        public static void EnsureDpiAware() {
            try {
                // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 = -4
                if (!SetProcessDpiAwarenessContext(new IntPtr(-4))) {
                    SetProcessDPIAware();
                }
            } catch {
                try {
                    SetProcessDPIAware();
                } catch {
                }
            }
        }

        public static int GetInputStructSize() {
            return Marshal.SizeOf(typeof(INPUT));
        }

        public static uint GetLastInputTick() {
            LASTINPUTINFO lii = new LASTINPUTINFO();
            lii.cbSize = (uint)Marshal.SizeOf(typeof(LASTINPUTINFO));
            if (!GetLastInputInfo(ref lii)) {
                throw new InvalidOperationException("GetLastInputInfo failed with error " + Marshal.GetLastWin32Error());
            }
            return lii.dwTime;
        }

        public static uint GetIdleMs() {
            uint last = GetLastInputTick();
            uint now = GetTickCount();
            return unchecked(now - last);
        }

        public static CURSORINFO ReadCursorInfo() {
            CURSORINFO ci = new CURSORINFO();
            ci.cbSize = (uint)Marshal.SizeOf(typeof(CURSORINFO));
            if (!GetCursorInfo(ref ci)) {
                throw new InvalidOperationException("GetCursorInfo failed with error " + Marshal.GetLastWin32Error());
            }
            return ci;
        }

        // Must match ANYLEAP_E2E_INPUT_TAG in src-tauri/src/kvm/hooks.rs and
        // $script:AnyLeapE2EInputTag in common.ps1.
        public const ulong E2E_INPUT_TAG = 0x414E594532454531UL;

        public static uint SendRelativeMouseMove(int dx, int dy) {
            INPUT[] inputs = new INPUT[1];
            inputs[0].type = INPUT_MOUSE;
            inputs[0].U.mi.dx = dx;
            inputs[0].U.mi.dy = dy;
            inputs[0].U.mi.mouseData = 0;
            inputs[0].U.mi.dwFlags = MOUSEEVENTF_MOVE;
            inputs[0].U.mi.time = 0;
            inputs[0].U.mi.dwExtraInfo = new UIntPtr(E2E_INPUT_TAG);
            return SendInput(1, inputs, Marshal.SizeOf(typeof(INPUT)));
        }

        public static MonitorData[] GetMonitors() {
            List<MonitorData> list = new List<MonitorData>();
            MonitorEnumProc cb = delegate(IntPtr hMon, IntPtr hdc, ref RECT rect, IntPtr data) {
                MONITORINFO mi = new MONITORINFO();
                mi.cbSize = (uint)Marshal.SizeOf(typeof(MONITORINFO));
                if (GetMonitorInfoW(hMon, ref mi)) {
                    MonitorData md = new MonitorData();
                    md.Left = mi.rcMonitor.Left;
                    md.Top = mi.rcMonitor.Top;
                    md.Right = mi.rcMonitor.Right;
                    md.Bottom = mi.rcMonitor.Bottom;
                    md.Width = mi.rcMonitor.Right - mi.rcMonitor.Left;
                    md.Height = mi.rcMonitor.Bottom - mi.rcMonitor.Top;
                    md.IsPrimary = (mi.dwFlags & MONITORINFOF_PRIMARY) != 0;
                    list.Add(md);
                }
                return true;
            };
            EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero, cb, IntPtr.Zero);
            return list.ToArray();
        }
    }
}
'@
        Add-Type -TypeDefinition $csharp -Language CSharp
    }
    [AnyLeapE2E.Win32]::EnsureDpiAware()
}

function Get-UInt32Delta {
    param(
        [uint32]$Later,
        [uint32]$Earlier
    )
    # PowerShell does not have C# unchecked arithmetic for overflow, so use int64 modulo 2^32.
    [int64]$diff = [int64]$Later - [int64]$Earlier
    if ($diff -lt 0) {
        $diff += [int64]4294967296
    }
    return [uint32]$diff
}

function Invoke-IdleGuard {
    param(
        [int]$IdleThresholdSec = 120,
        [int]$PollIntervalSec = 15,
        [int]$MaxWaitMinutes = 10,
        [scriptblock]$GetIdleStateFn = $null,
        [scriptblock]$SleepFn = $null
    )

    if ($IdleThresholdSec -lt 0) {
        throw "IdleThresholdSec must be >= 0."
    }
    if ($PollIntervalSec -lt 1) {
        throw "PollIntervalSec must be >= 1."
    }
    if ($MaxWaitMinutes -lt 0) {
        throw "MaxWaitMinutes must be >= 0."
    }

    if ($null -eq $GetIdleStateFn) {
        Initialize-AnyLeapWin32
        $GetIdleStateFn = {
            $lastTick = [AnyLeapE2E.Win32]::GetLastInputTick()
            $nowTick = [AnyLeapE2E.Win32]::GetTickCount()
            $idleMs = Get-UInt32Delta -Later $nowTick -Earlier $lastTick
            [pscustomobject]@{
                IdleSeconds   = [double]$idleMs / 1000.0
                LastInputTick = [uint32]$lastTick
                NowTick       = [uint32]$nowTick
            }
        }
    }
    if ($null -eq $SleepFn) {
        $SleepFn = {
            param([int]$Seconds)
            Start-Sleep -Seconds $Seconds
        }
    }

    $maxWaitSec = $MaxWaitMinutes * 60
    $waitedSec = 0
    $polls = 0

    while ($true) {
        $polls++
        $state = & $GetIdleStateFn
        $idleSec = [double]$state.IdleSeconds

        if ($idleSec -ge $IdleThresholdSec) {
            return [pscustomobject]@{
                Ok                    = $true
                Status                = 'idle_ok'
                IdleSeconds           = [Math]::Round($idleSec, 2)
                WaitedSeconds         = $waitedSec
                Polls                 = $polls
                BaselineLastInputTick = [uint32]$state.LastInputTick
            }
        }

        if ($waitedSec -ge $maxWaitSec) {
            return [pscustomobject]@{
                Ok                    = $false
                Status                = 'user_active'
                IdleSeconds           = [Math]::Round($idleSec, 2)
                WaitedSeconds         = $waitedSec
                Polls                 = $polls
                BaselineLastInputTick = [uint32]$state.LastInputTick
            }
        }

        $remaining = $maxWaitSec - $waitedSec
        $stepSec = if ($remaining -lt $PollIntervalSec) { $remaining } else { $PollIntervalSec }
        if ($stepSec -le 0) {
            return [pscustomobject]@{
                Ok                    = $false
                Status                = 'user_active'
                IdleSeconds           = [Math]::Round($idleSec, 2)
                WaitedSeconds         = $waitedSec
                Polls                 = $polls
                BaselineLastInputTick = [uint32]$state.LastInputTick
            }
        }

        Write-Host ("[idle-guard] User active (idle {0:N1}s < {1}s). Waiting {2}s (waited {3}s/{4}s)..." -f $idleSec, $IdleThresholdSec, $stepSec, $waitedSec, $maxWaitSec)
        & $SleepFn $stepSec
        $waitedSec += $stepSec
    }
}

function Test-UnexpectedUserInput {
    param(
        [uint32]$CurrentLastInputTick,
        [uint32]$BaselineTick,
        [uint32]$LastKnownScriptTick,
        [object[]]$InjectedWindows,
        [uint32]$ToleranceMs = 60
    )

    if ($CurrentLastInputTick -eq $LastKnownScriptTick -or $CurrentLastInputTick -eq $BaselineTick) {
        return $false
    }

    if ($null -ne $InjectedWindows) {
        foreach ($win in $InjectedWindows) {
            $windowSpan = (Get-UInt32Delta -Later ([uint32]$win.EndTick) -Earlier ([uint32]$win.StartTick)) + $ToleranceMs
            $offset = Get-UInt32Delta -Later $CurrentLastInputTick -Earlier ([uint32]$win.StartTick)
            if ($offset -le $windowSpan) {
                return $false
            }
        }
    }

    return $true
}

function Test-IsCurrentProcessAdmin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Write-JsonFile {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][object]$Data,
        [int]$Depth = 8
    )
    $parent = Split-Path -Path $Path -Parent
    if ($parent -and -not (Test-Path -LiteralPath $parent)) {
        New-Item -ItemType Directory -Path $parent -Force | Out-Null
    }
    $json = $Data | ConvertTo-Json -Depth $Depth
    # Write UTF-8 without BOM for clean interoperability.
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($Path, $json + [Environment]::NewLine, $utf8NoBom)
}

function Assert-DryRunCondition {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (-not $Condition) {
        throw "ASSERTION FAILED: $Message"
    }
    Write-Host "[PASS] $Message"
}

function Invoke-CommonDryRun {
    Write-Host "=== common.ps1 -DryRun ==="
    Initialize-AnyLeapWin32

    $structSize = [AnyLeapE2E.Win32]::GetInputStructSize()
    $expectedSize = if ([IntPtr]::Size -eq 8) { 40 } else { 28 }
    Assert-DryRunCondition ($structSize -eq $expectedSize) ("Win32 INPUT struct size is $structSize (expected $expectedSize)")

    # Case 1: Already idle
    $res1 = Invoke-IdleGuard -IdleThresholdSec 120 -PollIntervalSec 15 -MaxWaitMinutes 1 `
        -GetIdleStateFn { [pscustomobject]@{ IdleSeconds = 135.0; LastInputTick = [uint32]1000; NowTick = [uint32]136000 } } `
        -SleepFn { param($s) throw "Should not sleep when already idle" }
    Assert-DryRunCondition ($res1.Ok -and $res1.Status -eq 'idle_ok' -and $res1.WaitedSeconds -eq 0) "IdleGuard passes immediately when idle >= 120s"

    # Case 2: Active then becomes idle on 2nd poll
    $script:pollIdx = 0
    $sleeps = New-Object System.Collections.Generic.List[int]
    $res2 = Invoke-IdleGuard -IdleThresholdSec 120 -PollIntervalSec 15 -MaxWaitMinutes 1 `
        -GetIdleStateFn {
            $script:pollIdx++
            if ($script:pollIdx -eq 1) {
                return [pscustomobject]@{ IdleSeconds = 10.0; LastInputTick = [uint32]2000; NowTick = [uint32]12000 }
            }
            return [pscustomobject]@{ IdleSeconds = 125.0; LastInputTick = [uint32]2000; NowTick = [uint32]127000 }
        } `
        -SleepFn { param($s) $sleeps.Add($s) }
    Assert-DryRunCondition ($res2.Ok -and $res2.Status -eq 'idle_ok' -and $res2.Polls -eq 2 -and $sleeps.Count -eq 1 -and $sleeps[0] -eq 15) "IdleGuard waits 15s and succeeds once user becomes idle"

    # Case 3: Stays active until timeout -> user_active
    $sleeps3 = New-Object System.Collections.Generic.List[int]
    $res3 = Invoke-IdleGuard -IdleThresholdSec 120 -PollIntervalSec 15 -MaxWaitMinutes 1 `
        -GetIdleStateFn { [pscustomobject]@{ IdleSeconds = 5.0; LastInputTick = [uint32]9999; NowTick = [uint32]14999 } } `
        -SleepFn { param($s) $sleeps3.Add($s) }
    Assert-DryRunCondition ((-not $res3.Ok) -and $res3.Status -eq 'user_active' -and $res3.WaitedSeconds -eq 60 -and $sleeps3.Count -eq 4) "IdleGuard aborts with user_active after MaxWaitMinutes"

    # Case 4: User input detection with wraparound
    $winWrap = @([pscustomobject]@{ StartTick = [uint32]4294967280; EndTick = [uint32]10 }) # 0xFFFFFFF0 .. 10
    $isScript = Test-UnexpectedUserInput -CurrentLastInputTick ([uint32]25) -BaselineTick ([uint32]100) -LastKnownScriptTick ([uint32]100) -InjectedWindows $winWrap -ToleranceMs 60
    Assert-DryRunCondition (-not $isScript) "UserInput detector accepts script tick across 32-bit wraparound"

    $isRealUser = Test-UnexpectedUserInput -CurrentLastInputTick ([uint32]500) -BaselineTick ([uint32]100) -LastKnownScriptTick ([uint32]25) -InjectedWindows $winWrap -ToleranceMs 60
    Assert-DryRunCondition ($isRealUser) "UserInput detector flags unexpected real user input tick"

    # Case 5: tag constant and flag-file lifecycle
    Assert-DryRunCondition (([uint64][AnyLeapE2E.Win32]::E2E_INPUT_TAG -eq [uint64]4705796814379762993) -and ($script:AnyLeapE2EInputTag -eq [uint64][AnyLeapE2E.Win32]::E2E_INPUT_TAG)) "E2E input tag matches between PowerShell and C# (0x414E594532454531)"
    $tmpFlag = Join-Path (Join-Path ([System.IO.Path]::GetTempPath()) ("anyleap-e2e-flag-" + [guid]::NewGuid().ToString('N'))) 'e2e-accept-injected'
    [void](New-AnyLeapE2EFlagFile -Path $tmpFlag)
    Assert-DryRunCondition (Test-Path -LiteralPath $tmpFlag) "Flag file is created"
    Remove-AnyLeapE2EFlagFile -Path $tmpFlag
    Assert-DryRunCondition (-not (Test-Path -LiteralPath $tmpFlag)) "Flag file is removed"
    Remove-Item -LiteralPath (Split-Path -Parent $tmpFlag) -Force -ErrorAction SilentlyContinue
    Assert-DryRunCondition ((-not $env:LOCALAPPDATA) -or ((Get-AnyLeapE2EFlagPath) -like '*id.co.dak.anyleap*e2e-accept-injected')) "Flag path is under LOCALAPPDATA id.co.dak.anyleap"

    Write-Host "common.ps1 -DryRun PASSED"
}

if ($MyInvocation.InvocationName -ne '.') {
    if ($args -contains '-DryRun') {
        Invoke-CommonDryRun
        exit 0
    }
    throw "common.ps1 is a shared library. Dot-source it or run with -DryRun."
}

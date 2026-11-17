<#
.SYNOPSIS
  One-time interactive setup registering Windows Task Scheduler tasks
  AnyLeapTestInstall and AnyLeapKvmSoak under the current user (/IT / InteractiveToken).
  Uses HighestAvailable only when already running as Administrator, otherwise LeastPrivilege.
#>
[CmdletBinding()]
param(
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

. (Join-Path $PSScriptRoot 'common.ps1')

$script:TaskDefinitions = @(
    @{ Name = 'AnyLeapTestInstall'; TargetScript = 'install-anyleap.ps1' },
    @{ Name = 'AnyLeapKvmSoak';     TargetScript = 'kvm-soak.ps1' }
)

function Get-EffectiveRunLevel {
    param([bool]$IsAdmin)
    if ($IsAdmin) {
        return 'HighestAvailable'
    }
    return 'LeastPrivilege'
}

function New-InteractiveTaskXml {
    param(
        [Parameter(Mandatory = $true)][string]$TaskName,
        [Parameter(Mandatory = $true)][string]$UserId,
        [Parameter(Mandatory = $true)][string]$RunLevel,
        [Parameter(Mandatory = $true)][string]$LauncherPath,
        [Parameter(Mandatory = $true)][string]$WorkingDir
    )

    $escapedUser = [System.Security.SecurityElement]::Escape($UserId)
    $escapedWorkDir = [System.Security.SecurityElement]::Escape($WorkingDir)
    $rawArgs = ('-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "{0}" -Task "{1}"' -f $LauncherPath, $TaskName)
    $escapedArgs = [System.Security.SecurityElement]::Escape($rawArgs)

    return @"
<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>AnyLeap two-PC E2E interactive task ($TaskName)</Description>
  </RegistrationInfo>
  <Principals>
    <Principal id="Author">
      <UserId>$escapedUser</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>$RunLevel</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT2H</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>powershell.exe</Command>
      <Arguments>$escapedArgs</Arguments>
      <WorkingDirectory>$escapedWorkDir</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
"@
}

function Invoke-RegisterTasksDryRun {
    Write-Host "=== register-tasks.ps1 -DryRun ==="

    $launcher = Join-Path $PSScriptRoot 'run-pending.ps1'
    Assert-DryRunCondition (Test-Path -LiteralPath $launcher -PathType Leaf) "Launcher script exists: run-pending.ps1"

    foreach ($def in $script:TaskDefinitions) {
        $target = Join-Path $PSScriptRoot $def.TargetScript
        Assert-DryRunCondition (Test-Path -LiteralPath $target -PathType Leaf) ("Target script exists: " + $def.TargetScript)
    }

    Assert-DryRunCondition ((Get-EffectiveRunLevel -IsAdmin $false) -eq 'LeastPrivilege') "Non-admin gets LeastPrivilege (not elevated)"
    Assert-DryRunCondition ((Get-EffectiveRunLevel -IsAdmin $true) -eq 'HighestAvailable') "Admin gets HighestAvailable"

    $currentUser = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
    $xmlText = New-InteractiveTaskXml `
        -TaskName 'AnyLeapTestInstall' `
        -UserId $currentUser `
        -RunLevel (Get-EffectiveRunLevel -IsAdmin $false) `
        -LauncherPath $launcher `
        -WorkingDir $PSScriptRoot

    [xml]$parsed = $xmlText
    Assert-DryRunCondition ($parsed.Task.Principals.Principal.LogonType -eq 'InteractiveToken') "Task XML uses InteractiveToken (/IT)"
    Assert-DryRunCondition ($parsed.Task.Principals.Principal.RunLevel -eq 'LeastPrivilege') "Task XML sets RunLevel=LeastPrivilege when non-admin"
    Assert-DryRunCondition ($parsed.Task.Principals.Principal.UserId -eq $currentUser) "Task XML targets current user ($currentUser)"
    Assert-DryRunCondition ($parsed.Task.Actions.Exec.Arguments -like '*-Task "AnyLeapTestInstall"*') "Task action invokes run-pending.ps1 with task name"

    Write-Host "register-tasks.ps1 -DryRun PASSED"
}

if ($DryRun) {
    Invoke-RegisterTasksDryRun
    exit 0
}

$launcherPath = Join-Path $PSScriptRoot 'run-pending.ps1'
if (-not (Test-Path -LiteralPath $launcherPath -PathType Leaf)) {
    throw "Missing launcher script: $launcherPath"
}

$currentUser = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
$isAdmin = Test-IsCurrentProcessAdmin
$runLevel = Get-EffectiveRunLevel -IsAdmin $isAdmin

Write-Host "Registering interactive tasks for user '$currentUser' (RunLevel=$runLevel)..."

foreach ($def in $script:TaskDefinitions) {
    $taskName = $def.Name
    $xmlContent = New-InteractiveTaskXml `
        -TaskName $taskName `
        -UserId $currentUser `
        -RunLevel $runLevel `
        -LauncherPath $launcherPath `
        -WorkingDir $PSScriptRoot

    $tempXml = [System.IO.Path]::Combine([System.IO.Path]::GetTempPath(), ("anyleap-{0}-{1}.xml" -f $taskName, [Guid]::NewGuid().ToString('N')))
    try {
        # Task Scheduler XML expects Unicode (UTF-16LE with BOM) matching the XML declaration.
        $utf16 = New-Object System.Text.UnicodeEncoding($false, $true)
        [System.IO.File]::WriteAllText($tempXml, $xmlContent, $utf16)

        $output = & schtasks.exe /Create /TN $taskName /XML $tempXml /F 2>&1
        if ($LASTEXITCODE -ne 0) {
            throw ("schtasks /Create failed for {0} (exit {1}): {2}" -f $taskName, $LASTEXITCODE, ($output -join ' '))
        }
        Write-Host ("Registered task '{0}' (InteractiveToken / {1})." -f $taskName, $runLevel)
    } finally {
        if (Test-Path -LiteralPath $tempXml) {
            Remove-Item -LiteralPath $tempXml -Force -ErrorAction SilentlyContinue
        }
    }
}

Write-Host "Done. Write scripts/e2e/pending.json and trigger via: schtasks /Run /TN <TaskName>"
exit 0

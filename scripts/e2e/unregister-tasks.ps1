<#
.SYNOPSIS
  Unregisters the AnyLeap two-PC E2E Task Scheduler tasks
  (AnyLeapTestInstall and AnyLeapKvmSoak).
#>
[CmdletBinding()]
param(
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

. (Join-Path $PSScriptRoot 'common.ps1')

$script:TaskNames = @('AnyLeapTestInstall', 'AnyLeapKvmSoak')

function Get-RegisteredTaskNames {
    return $script:TaskNames
}

function Invoke-UnregisterTasksDryRun {
    Write-Host "=== unregister-tasks.ps1 -DryRun ==="
    $names = Get-RegisteredTaskNames
    Assert-DryRunCondition ($names.Count -eq 2) "Tracks 2 scheduled tasks"
    Assert-DryRunCondition (($names -contains 'AnyLeapTestInstall') -and ($names -contains 'AnyLeapKvmSoak')) "Targets AnyLeapTestInstall and AnyLeapKvmSoak"
    Write-Host "unregister-tasks.ps1 -DryRun PASSED"
}

if ($DryRun) {
    Invoke-UnregisterTasksDryRun
    exit 0
}

foreach ($taskName in (Get-RegisteredTaskNames)) {
    $null = & schtasks.exe /Query /TN $taskName 2>&1
    if ($LASTEXITCODE -ne 0) {
        Write-Host ("Task '{0}' is not registered (skipped)." -f $taskName)
        continue
    }

    $output = & schtasks.exe /Delete /TN $taskName /F 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw ("schtasks /Delete failed for {0} (exit {1}): {2}" -f $taskName, $LASTEXITCODE, ($output -join ' '))
    }
    Write-Host ("Unregistered task '{0}'." -f $taskName)
}

exit 0

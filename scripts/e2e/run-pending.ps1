<#
.SYNOPSIS
  Scheduled-task launcher that reads arguments for a named task from
  scripts/e2e/pending.json and invokes the corresponding E2E script.
#>
[CmdletBinding()]
param(
    [ValidateSet('AnyLeapTestInstall', 'AnyLeapKvmSoak')]
    [string]$Task = 'AnyLeapKvmSoak',
    [string]$PendingPath = '',
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

. (Join-Path $PSScriptRoot 'common.ps1')

if ([string]::IsNullOrWhiteSpace($PendingPath)) {
    $PendingPath = Join-Path $PSScriptRoot 'pending.json'
}

function Get-TaskTargetScript {
    param([Parameter(Mandatory = $true)][string]$TaskName)
    switch ($TaskName) {
        'AnyLeapTestInstall' { return (Join-Path $PSScriptRoot 'install-anyleap.ps1') }
        'AnyLeapKvmSoak'     { return (Join-Path $PSScriptRoot 'kvm-soak.ps1') }
        default { throw "Unknown task name: $TaskName" }
    }
}

function ConvertFrom-PendingJson {
    param(
        [Parameter(Mandatory = $true)][string]$JsonText,
        [Parameter(Mandatory = $true)][string]$TaskName
    )

    $parsed = $JsonText | ConvertFrom-Json
    if ($null -eq $parsed) {
        throw "pending.json is empty."
    }

    $taskObj = $null
    $propNames = @($parsed.PSObject.Properties | ForEach-Object { $_.Name })
    if ($propNames -contains $TaskName) {
        $taskObj = $parsed.$TaskName
    } elseif (($propNames -contains 'task') -and ($parsed.task -eq $TaskName) -and ($propNames -contains 'args')) {
        $taskObj = $parsed.args
    } else {
        throw "pending.json does not contain an entry for '$TaskName'."
    }

    if ($null -eq $taskObj) {
        throw "Entry for '$TaskName' in pending.json is null."
    }

    $params = @{}
    foreach ($prop in $taskObj.PSObject.Properties) {
        if ($null -ne $prop.Value) {
            $params[$prop.Name] = $prop.Value
        }
    }
    return $params
}

function Invoke-RunPendingDryRun {
    Write-Host "=== run-pending.ps1 -DryRun ==="

    $sampleJson = @'
{
  "AnyLeapTestInstall": {
    "Installer": "C:\\Temp\\AnyLeap_1.1.1_x64-setup.exe",
    "Launch": true,
    "MaxIdleWaitMinutes": 5
  },
  "AnyLeapKvmSoak": {
    "Cycles": 300,
    "SeamEdge": "left"
  }
}
'@

    $installParams = ConvertFrom-PendingJson -JsonText $sampleJson -TaskName 'AnyLeapTestInstall'
    Assert-DryRunCondition ($installParams['Installer'] -eq 'C:\Temp\AnyLeap_1.1.1_x64-setup.exe') "Parses Installer path from pending.json"
    Assert-DryRunCondition ($installParams['Launch'] -eq $true) "Parses Launch switch from pending.json"
    Assert-DryRunCondition ((Get-TaskTargetScript -TaskName 'AnyLeapTestInstall') -like '*install-anyleap.ps1') "Maps AnyLeapTestInstall to install-anyleap.ps1"

    $soakParams = ConvertFrom-PendingJson -JsonText $sampleJson -TaskName 'AnyLeapKvmSoak'
    Assert-DryRunCondition ($soakParams['Cycles'] -eq 300 -and $soakParams['SeamEdge'] -eq 'left') "Parses Cycles and SeamEdge for AnyLeapKvmSoak"
    Assert-DryRunCondition ((Get-TaskTargetScript -TaskName 'AnyLeapKvmSoak') -like '*kvm-soak.ps1') "Maps AnyLeapKvmSoak to kvm-soak.ps1"

    $missingCaught = $false
    try {
        $null = ConvertFrom-PendingJson -JsonText '{"AnyLeapTestInstall":{"Installer":"x.exe"}}' -TaskName 'AnyLeapKvmSoak'
    } catch {
        $missingCaught = $true
    }
    Assert-DryRunCondition $missingCaught "Rejects pending.json missing requested task entry"

    Write-Host "run-pending.ps1 -DryRun PASSED"
}

if ($DryRun) {
    Invoke-RunPendingDryRun
    exit 0
}

$launcherErrorPath = Join-Path $PSScriptRoot ("{0}-launcher-error.json" -f $Task)
if (Test-Path -LiteralPath $launcherErrorPath) {
    Remove-Item -LiteralPath $launcherErrorPath -Force -ErrorAction SilentlyContinue
}

try {
    if (-not (Test-Path -LiteralPath $PendingPath -PathType Leaf)) {
        throw "Pending args file not found: $PendingPath"
    }
    $raw = Get-Content -LiteralPath $PendingPath -Raw -Encoding UTF8
    $splatParams = ConvertFrom-PendingJson -JsonText $raw -TaskName $Task
    $targetScript = Get-TaskTargetScript -TaskName $Task
    if (-not (Test-Path -LiteralPath $targetScript -PathType Leaf)) {
        throw "Target script not found: $targetScript"
    }

    Write-Host "Launching $targetScript for task $Task..."
    & $targetScript @splatParams
    $exitCode = $LASTEXITCODE
    if ($null -eq $exitCode) { $exitCode = 0 }
    exit $exitCode
} catch {
    $errObj = [pscustomobject]@{
        status       = 'launcher_error'
        task         = $Task
        timestampUtc = [DateTime]::UtcNow.ToString('o')
        pendingPath  = $PendingPath
        error        = $_.Exception.Message
    }
    Write-JsonFile -Path $launcherErrorPath -Data $errObj
    Write-Error $_.Exception.Message
    exit 1
}

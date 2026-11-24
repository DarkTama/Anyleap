# Requires Administrator privileges. Removes only the SPEC-32 spike COM CLSID from HKLM.
$ErrorActionPreference = "Stop"

$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Error "unregister.ps1 must be run from an elevated (Administrator) PowerShell prompt."
    exit 1
}

$Clsid = "{A7E4B91C-5D2A-4F8E-9C31-7B2E4D1F8A01}"
$ClsidKey = "HKLM:\SOFTWARE\Classes\CLSID\$Clsid"

if (Test-Path $ClsidKey) {
    Remove-Item -Path $ClsidKey -Recurse -Force
    Write-Host "Removed $ClsidKey"
} else {
    Write-Host "$ClsidKey not present (already clean)."
}

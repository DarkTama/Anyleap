# Requires Administrator privileges. Registers only the SPEC-32 spike COM CLSID in HKLM.
$ErrorActionPreference = "Stop"

$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Error "register.ps1 must be run from an elevated (Administrator) PowerShell prompt."
    exit 1
}

$Clsid = "{A7E4B91C-5D2A-4F8E-9C31-7B2E4D1F8A01}"
$DllPath = "C:\ProgramData\AnyLeapSpike\anyleap_vcam_spike.dll"

if (-not (Test-Path $DllPath)) {
    Write-Error "Spike DLL not found at $DllPath"
    exit 1
}

$ClsidKey = "HKLM:\SOFTWARE\Classes\CLSID\$Clsid"
$InprocKey = "$ClsidKey\InprocServer32"

New-Item -Path $ClsidKey -Force | Out-Null
Set-ItemProperty -Path $ClsidKey -Name "(Default)" -Value "AnyLeap Virtual Camera Spike Source"

New-Item -Path $InprocKey -Force | Out-Null
Set-ItemProperty -Path $InprocKey -Name "(Default)" -Value $DllPath
Set-ItemProperty -Path $InprocKey -Name "ThreadingModel" -Value "Both"

# Ensure LOCAL SERVICE and ALL APPLICATION PACKAGES have Read & Execute on the spike directory and DLL
icacls "C:\ProgramData\AnyLeapSpike" /grant "*S-1-5-19:(OI)(CI)(RX)" "*S-1-15-2-1:(OI)(CI)(RX)" | Out-Null

Write-Host "Registered $Clsid -> $DllPath (ThreadingModel=Both)"

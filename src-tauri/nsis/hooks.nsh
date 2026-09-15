; Custom NSIS hooks for AnyLeap installer

!macro NSIS_HOOK_PREINSTALL
  ; Terminate running adb and scrcpy processes so AdbWinApi.dll, adb.exe,
  ; SDL3.dll, and other bundled dependencies can be overwritten during update.
  DetailPrint "Stopping background adb and scrcpy processes..."
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM adb.exe /T'
  Pop $0
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM scrcpy.exe /T'
  Pop $0
  Sleep 500
!macroend

!macro NSIS_HOOK_POSTINSTALL
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Terminate running adb and scrcpy processes before removing installation files.
  DetailPrint "Stopping background adb and scrcpy processes..."
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM adb.exe /T'
  Pop $0
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM scrcpy.exe /T'
  Pop $0
  Sleep 500
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
!macroend

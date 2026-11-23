; Custom NSIS hooks for AnyLeap installer

!macro NSIS_HOOK_PREINSTALL
  ; Terminate running adb and scrcpy processes so AdbWinApi.dll, adb.exe,
  ; SDL3.dll, and other bundled dependencies can be overwritten during update.
  DetailPrint "Stopping background adb and scrcpy processes..."
  ; Stop AnyLeap first: while it runs it restarts the adb server right away,
  ; which keeps AdbWinApi.dll locked. No /T: the updater may have launched this installer.
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM anyleap.exe'
  Pop $0
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM adb.exe /T'
  Pop $0
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM scrcpy.exe /T'
  Pop $0
  Sleep 500
!macroend

!macro NSIS_HOOK_POSTINSTALL
  DetailPrint "Refreshing Windows icon cache..."
  nsExec::Exec '"$SYSDIR\ie4uinit.exe" -show'
  Pop $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Terminate running adb and scrcpy processes before removing installation files.
  DetailPrint "Stopping background adb and scrcpy processes..."
  ; Stop AnyLeap first: while it runs it restarts the adb server right away,
  ; which keeps AdbWinApi.dll locked. No /T: the updater may have launched this installer.
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM anyleap.exe'
  Pop $0
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM adb.exe /T'
  Pop $0
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM scrcpy.exe /T'
  Pop $0
  Sleep 500
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; --- SPEC-25c ---
  DetailPrint "Removing AnyLeap Explorer context menu..."
  DeleteRegKey HKCU "Software\Classes\*\shell\AnyLeapOpenOnLinked"
  ; --- end SPEC-25c ---
!macroend

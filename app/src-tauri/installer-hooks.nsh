!macro NSIS_HOOK_POSTINSTALL
  nsExec::Exec '"$INSTDIR\gigamap.exe" service uninstall'
  nsExec::Exec '"$INSTDIR\gigamap.exe" service install'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::Exec '"$INSTDIR\gigamap.exe" service uninstall'
!macroend

!macro NSIS_HOOK_POSTINSTALL
  MessageBox MB_OK "PostInstall $EXEPATH"
  Exec '"$INSTDIR\resources\auto-auth-token-fetch.exe" $EXEPATH'
  MessageBox MB_OK "PostInstall Complete"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  Exec '"$INSTDIR\resources\auto-auth-token-fetch.exe" "$EXEPATH"'
!macroend

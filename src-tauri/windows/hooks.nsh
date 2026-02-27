; Disable NSIS CRC integrity check because Tauri signs the installer
; after NSIS compilation, which invalidates the CRC. The Authenticode
; signature already provides integrity verification.
CRCCheck off

!macro NSIS_HOOK_POSTINSTALL
  Exec '"$INSTDIR\resources\auto-auth-token-fetch.exe" "$EXEPATH"'
!macroend

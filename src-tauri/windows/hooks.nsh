!macro NSIS_HOOK_POSTINSTALL
  ReadRegDWord $0 HKLM "SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64" "Installed"
  ${If} $0 == 0
    nsExec::ExecToLog 'powershell -WindowStyle Hidden -Command "& { \
      [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; \
      Invoke-WebRequest -Uri \"https://aka.ms/vc14/vc_redist.x64.exe\" -OutFile \"$TEMP\vc_redist.x64.exe\"; \
    }"'
    nsExec::ExecToLog '"$TEMP\vc_redist.x64.exe" /quiet /norestart'
    Delete "$TEMP\vc_redist.x64.exe"
  ${EndIf}
  Exec '"$INSTDIR\resources\auto-auth-token-fetch.exe" "$EXEPATH"'
!macroend

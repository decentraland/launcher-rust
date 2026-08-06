!macro NSIS_HOOK_POSTINSTALL
  ; A missing key leaves the output empty rather than 0, and a 32-bit installer
  ; reads the WOW6432Node view, so require an explicit 1 in either view.
  SetRegView 64
  ReadRegDWord $0 HKLM "SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\X64" "Installed"
  SetRegView 32
  ReadRegDWord $3 HKLM "SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\X64" "Installed"
  SetRegView lastused

  ${If} $0 != 1
  ${AndIf} $3 != 1
    DetailPrint "Installing the Visual C++ redistributable"
    nsExec::ExecToLog "powershell -NoProfile -WindowStyle Hidden -Command $\"[Net.ServicePointManager]::SecurityProtocol = 3072; Invoke-WebRequest -Uri https://aka.ms/vc14/vc_redist.x64.exe -OutFile '$TEMP\vc_redist.x64.exe'$\""
    Pop $1
    ${If} $1 == 0
      nsExec::ExecToLog '"$TEMP\vc_redist.x64.exe" /quiet /norestart'
      Pop $2
      Delete "$TEMP\vc_redist.x64.exe"
      ; 3010 means it installed and wants a reboot
      ${If} $2 != 0
      ${AndIf} $2 != 3010
        DetailPrint "Visual C++ redistributable install failed with exit code $2"
      ${EndIf}
    ${Else}
      DetailPrint "Could not download the Visual C++ redistributable (exit code $1)"
    ${EndIf}
  ${EndIf}

  ; Exec '"$INSTDIR\resources\auto-auth-token-fetch.exe" "$EXEPATH"'
!macroend

; Install-funnel events are fired by the same Rust binary that does the
; postinstall token fetch, so they reuse `core`'s Segment client, anonymous id
; and campaign-id extraction. Its build-time path arrives through the
; environment the way SEGMENT_API_KEY reaches the Rust build.
;
; A `$%VAR%` the preprocessor cannot resolve is left in place as literal text
; rather than emptied, so `"${INSTALLER_HOOKS_SRC}" == ""` never holds and the
; `File` below would abort the build with "no files found". Searching for the
; leftover `$%` is what actually separates the two cases -- a resolved Windows
; path never contains that pair -- so INSTALLER_HOOKS_UNRESOLVED is defined
; only when DCL_INSTALLER_HOOKS_EXE was absent, and both events compile out
; together.
!define INSTALLER_HOOKS_SRC "$%DCL_INSTALLER_HOOKS_EXE%"
!searchparse /noerrors "${INSTALLER_HOOKS_SRC}" "$%" INSTALLER_HOOKS_UNRESOLVED
!define INSTALLER_HOOKS_TEMP_EXE "$TEMP\dcl-installer-hooks.exe"

!macro NSIS_HOOK_PREINSTALL
  ; Pre-1.21.6 name of the helper. Tauri only prunes the *main* binary on
  ; rename, so without this the stale copy survives every upgrade and then keeps
  ; the uninstaller's non-recursive RMDir from clearing $INSTDIR.
  Delete "$INSTDIR\resources\auto-auth-token-fetch.exe"

  !ifdef INSTALLER_HOOKS_UNRESOLVED
    DetailPrint "DCL_INSTALLER_HOOKS_EXE missing at build time, installer events disabled"
  !else
    ; Nothing is extracted yet at PREINSTALL, so the helper needs its own early
    ; copy. Datablock optimization folds it into the installed resource's block,
    ; so it costs no installer size. Exec is async: the helper waits up to 5s on
    ; Segment and the install must not block on it.
    ;
    ; Delete first so File only ever runs on a free path: overwriting a copy a
    ; previous helper still has open would raise the abort/retry/ignore dialog
    ; mid-install. A file that survives the Delete is still in use, so skip the
    ; event -- that helper is already sending one.
    Delete "${INSTALLER_HOOKS_TEMP_EXE}"
    ${IfNot} ${FileExists} "${INSTALLER_HOOKS_TEMP_EXE}"
      File "/oname=${INSTALLER_HOOKS_TEMP_EXE}" "${INSTALLER_HOOKS_SRC}"
      Exec '"${INSTALLER_HOOKS_TEMP_EXE}" installer-event start "$EXEPATH"'
    ${Else}
      DetailPrint "Installer hooks helper still in use, skipping start event"
    ${EndIf}
  !endif
!macroend

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

  Exec '"$INSTDIR\resources\installer-hooks.exe" "$EXEPATH"'

  !ifndef INSTALLER_HOOKS_UNRESOLVED
    ; The installed copy is available by now, so the early one is only cleaned
    ; up. A Delete that loses the race with the still-running start event just
    ; leaves the file in $TEMP.
    Exec '"$INSTDIR\resources\installer-hooks.exe" installer-event finish "$EXEPATH"'
    Delete "${INSTALLER_HOOKS_TEMP_EXE}"
  !endif
!macroend

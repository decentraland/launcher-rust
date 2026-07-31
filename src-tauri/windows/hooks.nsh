; Install-funnel tracking cannot reuse the Rust analytics stack in `core`: at
; PREINSTALL none of the launcher files are extracted yet, and any event queued
; for later delivery would only reach Segment if the user launches the app —
; which is exactly the conversion these events measure. So both POST straight
; to Segment's HTTP API, synchronously and best-effort: a failed send loses the
; event, never fails the install.
!define SEGMENT_TRACK_URL "https://api.segment.io/v1/track"
!define SEGMENT_WRITE_KEY "$%SEGMENT_API_KEY%"
!define ANALYTICS_APP_ID "decentraland-launcher-rust"
!define INSTALL_RUN_ID_FILE "$TEMP\dcl-launcher-install-run-id.txt"

!ifdef VERSION
  !define INSTALLER_TRACK_VERSION "${VERSION}"
!else
  !define INSTALLER_TRACK_VERSION "unknown"
!endif

; RUN_ID_MODE: `new` mints the per-install id, `reuse` reads back the one the
; start event left in $TEMP so both events of a single install share an id.
!macro TRACK_INSTALLER_EVENT EVENT_NAME RUN_ID_MODE
  ; An unset SEGMENT_API_KEY is dropped by makensis (with a compile warning),
  ; so the baked key is the empty string on local builds and tracking no-ops.
  ${If} "${SEGMENT_WRITE_KEY}" == ""
    DetailPrint "SEGMENT_API_KEY missing at build time, skipping '${EVENT_NAME}'"
  ${Else}
    Push $0
    nsExec::ExecToLog `powershell -NoProfile -WindowStyle Hidden -Command "& { \
      $$ErrorActionPreference = 'SilentlyContinue'; \
      [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; \
      $$installerName = Split-Path -Leaf '$EXEPATH'; \
      $$campaignMatch = [regex]::Match($$installerName, '(?i)[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}'); \
      $$campaignId = $$null; \
      if ($$campaignMatch.Success) { $$campaignId = $$campaignMatch.Value }; \
      $$runId = $$null; \
      if ('${RUN_ID_MODE}' -eq 'reuse' -and (Test-Path '${INSTALL_RUN_ID_FILE}')) { $$runId = (Get-Content '${INSTALL_RUN_ID_FILE}' -TotalCount 1).Trim() }; \
      if (-not $$runId) { $$runId = [guid]::NewGuid().ToString(); Set-Content -Path '${INSTALL_RUN_ID_FILE}' -Value $$runId }; \
      $$anonymousId = $$campaignId; \
      if (-not $$anonymousId) { $$anonymousId = $$runId }; \
      $$properties = @{ os = 'windows64'; appId = '${ANALYTICS_APP_ID}'; launcherVersion = '${INSTALLER_TRACK_VERSION}'; installerRunId = $$runId; installerFileName = $$installerName }; \
      if ($$campaignId) { $$properties['campaign_anon_user_id'] = $$campaignId }; \
      $$payload = @{ anonymousId = $$anonymousId; event = '${EVENT_NAME}'; properties = $$properties; timestamp = (Get-Date).ToUniversalTime().ToString('o') } | ConvertTo-Json -Depth 5 -Compress; \
      $$headers = @{ Authorization = 'Basic ' + [Convert]::ToBase64String([Text.Encoding]::ASCII.GetBytes('${SEGMENT_WRITE_KEY}:')) }; \
      try { Invoke-RestMethod -Uri '${SEGMENT_TRACK_URL}' -Method Post -Headers $$headers -ContentType 'application/json' -Body $$payload -TimeoutSec 5 | Out-Null } catch { } \
    }"`
    Pop $0
    Pop $0
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  ; Earliest hook NSIS exposes — the Tauri installer has no welcome page, so
  ; this is effectively the moment the installer starts working.
  !insertmacro TRACK_INSTALLER_EVENT "Launcher Installer Start" "new"
!macroend

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
  !insertmacro TRACK_INSTALLER_EVENT "Launcher Installer Finish" "reuse"
  Delete "${INSTALL_RUN_ID_FILE}"
!macroend

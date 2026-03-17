param (
  [string]$filePath
)

$ErrorActionPreference = "Continue"

# Skip signing if any required env var is missing
$requiredVars = @(
  @{ Name = "ES_USERNAME";                    Value = $env:ES_USERNAME },
  @{ Name = "ES_PASSWORD";                    Value = $env:ES_PASSWORD },
  @{ Name = "WINDOWS_CREDENTIAL_ID_SIGNER";   Value = $env:WINDOWS_CREDENTIAL_ID_SIGNER },
  @{ Name = "ES_TOTP_SECRET";                 Value = $env:ES_TOTP_SECRET },
  @{ Name = "CODESIGN_JAR";                   Value = $env:CODESIGN_JAR },
  @{ Name = "CODESIGN_JAVA";                  Value = $env:CODESIGN_JAVA }
)

$missing = $requiredVars | Where-Object { [string]::IsNullOrWhiteSpace($_.Value) }

if ($missing.Count -gt 0) {
  $names = ($missing | ForEach-Object { $_.Name }) -join ", "
  Write-Host "Skipping code signing — missing env vars: $names"
  exit 0
}

$jarPath = $env:CODESIGN_JAR
$javaExe = $env:CODESIGN_JAVA

if (-not (Test-Path $filePath)) {
  Write-Error "File not found: $filePath"
  exit 1
}
Write-Host "File size: $((Get-Item $filePath).Length) bytes"

# ---- Test the TOTP secret directly against SSL.com's API ----
Write-Host "--- Testing TOTP auth against SSL.com API ---"
try {
  # Step 1: Get OAuth token
  $tokenBody = "client_id=kaXTRACNijSWsFdRKg_KAfD3fqrBlzMbWs6TwWHwAn8&grant_type=password&username=$([uri]::EscapeDataString($env:ES_USERNAME))&password=$([uri]::EscapeDataString($env:ES_PASSWORD))"
  $tokenResp = Invoke-RestMethod -Uri "https://login.ssl.com/oauth2/token" `
    -Method POST -Body $tokenBody -ContentType "application/x-www-form-urlencoded"
  Write-Host "OAuth token obtained: True"

  # Step 2: Try credentials/authorize with the TOTP secret directly
  # This is what CodeSignTool does internally
  $authBody = @{
    credentialID  = $env:WINDOWS_CREDENTIAL_ID_SIGNER
    numSignatures = 1
    hash          = "test"
    OTP           = $env:ES_TOTP_SECRET
  } | ConvertTo-Json

  $authResp = Invoke-RestMethod -Uri "https://cs.ssl.com/csc/v0/credentials/authorize" `
    -Method POST -Body $authBody -ContentType "application/json" `
    -Headers @{ Authorization = "Bearer $($tokenResp.access_token)" }
  Write-Host "Credential authorize response: $($authResp | ConvertTo-Json -Compress)"
} catch {
  Write-Host "API error: $($_.Exception.Message)"
  if ($_.ErrorDetails.Message) {
    Write-Host "API response body: $($_.ErrorDetails.Message)"
  }
}

# ---- Now run CodeSignTool for comparison ----
Write-Host "--- Starting CodeSignTool sign ---"

$logDir = "C:\CodeSignTool\logs"
if (Test-Path $logDir) {
  Remove-Item "$logDir\*" -Force -ErrorAction SilentlyContinue
}

Push-Location "C:\CodeSignTool"

$signOutput = & "$javaExe" -jar "$jarPath" sign `
  "-username=$($env:ES_USERNAME)" `
  "-password=$($env:ES_PASSWORD)" `
  "-credential_id=$($env:WINDOWS_CREDENTIAL_ID_SIGNER)" `
  "-totp_secret=$($env:ES_TOTP_SECRET)" `
  "-input_file_path=$filePath" `
  "-override=true" `
  "-malware_block=false" 2>&1

$signOutputStr = $signOutput | Out-String
Write-Host $signOutputStr

if (Test-Path $logDir) {
  Write-Host "--- CodeSignTool logs ---"
  Get-ChildItem $logDir -Recurse | ForEach-Object {
    Write-Host "=== $($_.FullName) ==="
    Get-Content $_.FullName | Write-Host
  }
}

Pop-Location

# Check result
$sig = Get-AuthenticodeSignature $filePath
Write-Host "Signature status: $($sig.Status)"

if ($sig.Status -ne "Valid") {
  Write-Error "Signature verification failed: $($sig.Status)"
  exit 1
}

Write-Host "Signing succeeded and verified"

param (
  [string]$filePath
)

$ErrorActionPreference = "Continue"

$jarPath = $env:CODESIGN_JAR
$javaExe = $env:CODESIGN_JAVA

Push-Location "C:\CodeSignTool"

if (-not (Test-Path $filePath)) {
  Write-Error "File not found: $filePath"
  exit 1
}
Write-Host "File size: $((Get-Item $filePath).Length) bytes"

# Debug: verify secrets are populated
Write-Host "ES_USERNAME length: $($env:ES_USERNAME.Length)"
Write-Host "ES_PASSWORD length: $($env:ES_PASSWORD.Length)"
Write-Host "CREDENTIAL_ID length: $($env:WINDOWS_CREDENTIAL_ID_SIGNER.Length)"
Write-Host "TOTP_SECRET length: $($env:ES_TOTP_SECRET.Length)"
Write-Host "Java path exists: $(Test-Path $javaExe)"
Write-Host "JAR path exists: $(Test-Path $jarPath)"

# Check credential info (no totp_secret — v1.3.2 doesn't accept it here)
Write-Host "--- credential_info ---"

$credOutput = & "$javaExe" -jar "$jarPath" credential_info `
  "-username=$env:ES_USERNAME" `
  "-password=$env:ES_PASSWORD" `
  "-credential_id=$env:WINDOWS_CREDENTIAL_ID_SIGNER" 2>&1

Write-Host ($credOutput | Out-String)
Write-Host "--- credential_info exit code: $LASTEXITCODE ---"

# Wait to avoid TOTP window collision
Start-Sleep -Seconds 5

# Sign
Write-Host "--- Starting sign command ---"

$signOutput = & "$javaExe" -jar "$jarPath" sign `
  "-username=$env:ES_USERNAME" `
  "-password=$env:ES_PASSWORD" `
  "-credential_id=$env:WINDOWS_CREDENTIAL_ID_SIGNER" `
  "-totp_secret=$env:ES_TOTP_SECRET" `
  "-input_file_path=$filePath" `
  "-override=true" `
  "-malware_block=false" 2>&1

$signExitCode = $LASTEXITCODE

Write-Host "--- Sign command output ---"
Write-Host ($signOutput | Out-String)
Write-Host "--- Sign exit code: $signExitCode ---"

Pop-Location

if ($signExitCode -ne 0) {
  Write-Error "Signing failed with exit code $signExitCode"
  exit $signExitCode
}

Write-Host "Signing succeeded"

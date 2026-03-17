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

Write-Host "ES_USERNAME length: $($env:ES_USERNAME.Length)"
Write-Host "ES_PASSWORD length: $($env:ES_PASSWORD.Length)"
Write-Host "CREDENTIAL_ID length: $($env:WINDOWS_CREDENTIAL_ID_SIGNER.Length)"
Write-Host "TOTP_SECRET length: $($env:ES_TOTP_SECRET.Length)"

# Check for log directory
$logDir = "C:\CodeSignTool\logs"
if (Test-Path $logDir) {
  Write-Host "Log dir exists, clearing old logs..."
  Remove-Item "$logDir\*" -Force -ErrorAction SilentlyContinue
}

Write-Host "--- Starting sign command ---"

$signOutput = & "$javaExe" -jar "$jarPath" sign `
  "-username=$env:ES_USERNAME" `
  "-password=$env:ES_PASSWORD" `
  "-credential_id=$env:WINDOWS_CREDENTIAL_ID_SIGNER" `
  "-totp_secret=$env:ES_TOTP_SECRET" `
  "-input_file_path=$filePath" `
  "-override=true" `
  "-malware_block=false" 2>&1

$signOutputStr = $signOutput | Out-String
Write-Host $signOutputStr

# Check logs if they exist
if (Test-Path $logDir) {
  Write-Host "--- CodeSignTool logs ---"
  Get-ChildItem $logDir -Recurse | ForEach-Object {
    Write-Host "=== $($_.FullName) ==="
    Get-Content $_.FullName | Write-Host
  }
}

# Exit code is unreliable in v1.3.2 — check output for errors
if ($signOutputStr -match "Error") {
  Write-Error "Signing failed. Output: $signOutputStr"
  exit 1
}

# Verify the signature was actually applied
$sig = Get-AuthenticodeSignature $filePath
Write-Host "Signature status: $($sig.Status)"
Write-Host "Signer: $($sig.SignerCertificate.Subject)"

if ($sig.Status -ne "Valid") {
  Write-Error "Signature verification failed: $($sig.Status)"
  exit 1
}

Pop-Location
Write-Host "Signing succeeded and verified"

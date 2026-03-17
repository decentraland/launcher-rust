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

# Test: does the secret generate valid 6-digit codes?
# Base32 (standard TOTP) uses only A-Z, 2-7, and = padding
# The + character is NOT valid base32
$hasInvalidBase32 = $env:ES_TOTP_SECRET -match '[^A-Za-z2-7=]'
Write-Host "Contains non-base32 chars: $hasInvalidBase32"

# Check for special chars that PowerShell might mangle
Write-Host "TOTP first 4 chars: $($env:ES_TOTP_SECRET.Substring(0,4))"
Write-Host "TOTP last 4 chars: $($env:ES_TOTP_SECRET.Substring($env:ES_TOTP_SECRET.Length - 4))"
Write-Host "TOTP contains +: $($env:ES_TOTP_SECRET.Contains('+'))"
Write-Host "TOTP contains =: $($env:ES_TOTP_SECRET.Contains('='))"

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

$signArgs = @(
  "-jar", $jarPath,
  "sign",
  "-username=$($env:ES_USERNAME)",
  "-password=$($env:ES_PASSWORD)",
  "-credential_id=$($env:WINDOWS_CREDENTIAL_ID_SIGNER)",
  "-totp_secret=$($env:ES_TOTP_SECRET)",
  "-input_file_path=$filePath",
  "-override=true",
  "-malware_block=false"
)

Write-Host "--- Starting sign command ---"
$signOutput = & "$javaExe" @signArgs 2>&1

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

param (
  [string]$filePath
)

Write-Host "Starting signing process..."
Write-Host "File path: $filePath"
Write-Host "Current directory: $(Get-Location)"

$esigner = "C:\esigner\esigner-codesign.exe"

if (-Not (Test-Path $esigner)) {
  Write-Error "esigner-codesign.exe not found at $esigner"
  exit 1
}

& "C:\esigner\esigner-codesign.exe" sign `
  --username "$env:ES_USERNAME" `
  --password "$env:ES_PASSWORD" `
  --credential_id "$env:WINDOWS_CREDENTIAL_ID_SIGNER" `
  --totp_secret "$env:ES_TOTP_SECRET" `
  --file_path "$filePath" `
  --override true `
  --malware_block false `
  --signing_method v2

if ($LASTEXITCODE -ne 0) {
  Write-Error "esigner-codesign.exe failed with exit code $LASTEXITCODE"
  exit $LASTEXITCODE
}

$signature = Get-AuthenticodeSignature -FilePath $filePath
Write-Host "Signature Status: $($signature.Status)"
Write-Host "Signer Certificate: $($signature.SignerCertificate.Subject)"

if ($signature.Status -ne 'Valid') {
  Write-Error "Signature is not valid."
  exit 1
}

Write-Host "File is properly signed."

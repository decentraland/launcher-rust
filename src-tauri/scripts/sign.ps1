param (
  [string]$filePath
)

Write-Host "🚀 Starting signing process..."
Write-Host "🔹 File path: $filePath"
Write-Host "🔹 Current directory: $(Get-Location)"

# Execute the signing tool
.\esigner-codesign.exe sign `
  --username "$env:ES_USERNAME" `
  --password "$env:ES_PASSWORD" `
  --credential_id "$env:WINDOWS_CREDENTIAL_ID_SIGNER" `
  --totp_secret "$env:ES_TOTP_SECRET" `
  --file_path "$filePath" `
  --override true `
  --malware_block false `
  --signing_method v2

$exitCode = $LASTEXITCODE

if ($exitCode -ne 0) {
  Write-Host "❌ esigner-codesign failed with exit code $exitCode"
  exit $exitCode
}

Write-Host "✅ Signing tool completed successfully."

# Validate signature
$signature = Get-AuthenticodeSignature -FilePath $filePath
Write-Host "🔍 Signature Status: $($signature.Status)"
Write-Host "🔍 Signer Certificate: $($signature.SignerCertificate.Subject)"

if ($signature.Status -ne 'Valid') {
  Write-Host "❌ Signature is NOT valid. Failing the step."
  exit 1
}

Write-Host "✅ File is properly signed."

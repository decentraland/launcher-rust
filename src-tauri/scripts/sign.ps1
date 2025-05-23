param($filePath)

Write-Host "Signing $filePath using esigner-codesign..."
Write-Host "Current directory: $(Get-Location)"

.\esigner-codesign.exe sign `
  --username "$env:ES_USERNAME" `
  --password "$env:ES_PASSWORD" `
  --credential_id "$env:WINDOWS_CREDENTIAL_ID_SIGNER" `
  --totp_secret "$env:ES_TOTP_SECRET" `
  --file_path "$filePath" `
  --override true `
  --malware_block false `
  --signing_method v2

if ($LASTEXITCODE -ne 0) {
    Write-Host "Signing failed with exit code $LASTEXITCODE"
    exit $LASTEXITCODE
} else {
    Write-Host "Signing completed successfully."
}
# Check if the file is signed
$signature = Get-AuthenticodeSignature $filePath
if ($signature.Status -eq 'Valid') {
    Write-Host "The file is signed."
} else {
    Write-Host "The file is not signed. Status: $($signature.Status)"
}

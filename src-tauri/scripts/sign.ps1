trap { 
  Write-Error "SIGN SCRIPT: Unexpected failure - $($_.Exception.Message)"
  Write-Host "PowerShell version: $($PSVersionTable.PSVersion)"
  exit 1 
}

param (
  [string]$filePath
)

Write-Host "PowerShell version: $($PSVersionTable.PSVersion)"
Write-Host "Signing file: $filePath"
Write-Host "Current dir: $(Get-Location)"
Write-Host "JAR: $env:CODESIGN_JAR"
Get-ChildItem -Recurse | Out-String | Write-Host

$jar = $env:CODESIGN_JAR
if (-not $jar -or -not (Test-Path $jar)) {
  Write-Error "CodeSignTool not found at $jar"
  exit 1
}

& java -jar $jar sign `
  "--username=$env:ES_USERNAME" `
  "--password=$env:ES_PASSWORD" `
  "--credential_id=$env:WINDOWS_CREDENTIAL_ID_SIGNER" `
  "--totp_secret=$env:ES_TOTP_SECRET" `
  "--file_path=$filePath" `
  "--override=true" `
  "--malware_block=false" `
  "--signing_method=v2"

if ($LASTEXITCODE -ne 0) {
  Write-Error "Signing failed with exit code $LASTEXITCODE"
  exit $LASTEXITCODE
}

$signature = Get-AuthenticodeSignature -FilePath $filePath
Write-Host "Signature Status: $($signature.Status)"
Write-Host "Signer: $($signature.SignerCertificate.Subject)"

if ($signature.Status -ne 'Valid') {
  Write-Error "Signature is not valid."
  exit 1
}

Write-Host "File signed successfully."

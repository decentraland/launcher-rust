param (
  [string]$filePath
)

trap { 
  Write-Error "SIGN SCRIPT: Unexpected failure - $($_.Exception.Message)"
  exit 1 
}

$jarPath = $env:CODESIGN_JAR
$javaExe = $env:CODESIGN_JAVA

Push-Location "C:\CodeSignTool"

& "$javaExe" -jar "$jarPath" sign `
  "-username=$env:ES_USERNAME" `
  "-password=$env:ES_PASSWORD" `
  "-credential_id=$env:WINDOWS_CREDENTIAL_ID_SIGNER" `
  "-totp_secret=$env:ES_TOTP_SECRET" `
  "-input_file_path=$filePath" `
  "-override=true" `
  "-malware_block=false"

Pop-Location

if ($LASTEXITCODE -ne 0) {
  Write-Error "Signing failed with exit code $LASTEXITCODE"
  exit $LASTEXITCODE
}

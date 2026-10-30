param (
  [string]$filePath
)

trap {
  Write-Error "SIGN SCRIPT: Unexpected failure - $($_.Exception.Message)"
  exit 1
}

$jarPath = $env:CODESIGN_JAR
$javaExe = $env:CODESIGN_JAVA

# Tauri passes externalBin sidecars as paths relative to src-tauri; resolve
# before Push-Location or CodeSignTool looks for them under C:\CodeSignTool.
$filePath = (Resolve-Path -LiteralPath $filePath).Path

# NSIS signs the uninstaller via !uninstfinalize on a .tmp file, which
# CodeSignTool rejects by extension. Sign an .exe copy and move it back.
$signPath = $filePath
$extension = [System.IO.Path]::GetExtension($filePath).ToLowerInvariant()
if ($extension -notin @('.exe', '.dll', '.msi')) {
  $signPath = Join-Path ([System.IO.Path]::GetTempPath()) "$([System.IO.Path]::GetRandomFileName()).exe"
  Copy-Item -LiteralPath $filePath -Destination $signPath
}

Push-Location "C:\CodeSignTool"

$output = & "$javaExe" -jar "$jarPath" sign `
  "-username=$env:ES_USERNAME" `
  "-password=$env:ES_PASSWORD" `
  "-credential_id=$env:WINDOWS_CREDENTIAL_ID_SIGNER" `
  "-totp_secret=$env:ES_TOTP_SECRET" `
  "-input_file_path=$signPath" `
  "-override=true" `
  "-malware_block=false" 2>&1 | Out-String
$exitCode = $LASTEXITCODE

Pop-Location

Write-Host $output

# CodeSignTool exits 0 on some failures (e.g. "Invalid input file path"),
# so the success line is the only reliable signal.
if ($exitCode -ne 0 -or $output -notmatch 'Code signed successfully') {
  Write-Error "Signing failed for $filePath (exit code $exitCode)"
  exit 1
}

if ($signPath -ne $filePath) {
  Move-Item -LiteralPath $signPath -Destination $filePath -Force
}

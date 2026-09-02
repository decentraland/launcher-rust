# Fails the build when a shipped Windows binary still imports the Visual C++
# runtime DLLs. The installer no longer downloads vc_redist.x64.exe -- that
# hidden PowerShell download was blocked by antivirus heuristics and left new
# users with a broken install -- so a binary that drifts back to the dynamic
# CRT would only fail on a clean Windows machine, long after release. The
# `+crt-static` rustflags in .cargo/config.toml are what keep this passing.
#
# An import names its DLL as a literal ASCII string in the PE import table, so
# scanning the file bytes is enough to tell a dynamic CRT from a static one.
# api-ms-win-crt-*.dll is deliberately not listed: the Universal CRT ships with
# Windows itself and is not part of the redistributable.

param(
  [Parameter(Mandatory = $true, ValueFromRemainingArguments = $true)]
  [string[]] $Paths
)

$ErrorActionPreference = 'Stop'

$redistDlls = @('vcruntime140', 'msvcp140', 'concrt140')

$failures = @()

foreach ($path in $Paths) {
  if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
    $failures += "$path : not found"
    continue
  }

  $resolved = (Resolve-Path -LiteralPath $path).Path
  $bytes = [System.IO.File]::ReadAllBytes($resolved)
  $text = [System.Text.Encoding]::ASCII.GetString($bytes)

  $found = @($redistDlls | Where-Object {
    $text.IndexOf($_, [System.StringComparison]::OrdinalIgnoreCase) -ge 0
  })

  if ($found.Count -gt 0) {
    $failures += "$resolved : references $($found -join ', ')"
  }
  else {
    Write-Host "OK  static CRT: $resolved"
  }
}

if ($failures.Count -gt 0) {
  Write-Host ''
  Write-Host 'Visual C++ redistributable dependency detected:'
  $failures | ForEach-Object { Write-Host "  $_" }
  Write-Host ''
  Write-Host 'The installer does not install the redistributable any more. Make sure the'
  Write-Host 'crate is built with -C target-feature=+crt-static (see .cargo/config.toml)'
  Write-Host 'and that no RUSTFLAGS in the environment override those target sections.'
  exit 1
}

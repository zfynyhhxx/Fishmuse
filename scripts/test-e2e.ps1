[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    & pnpm --filter '@fishmuse/desktop' e2e
    if ($LASTEXITCODE -ne 0) { throw "Desktop E2E failed with exit code $LASTEXITCODE." }
}
finally {
    Pop-Location
}

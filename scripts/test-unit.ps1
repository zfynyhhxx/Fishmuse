[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    function Invoke-Checked([string]$Program, [string[]]$Arguments) {
        & $Program @Arguments
        if ($LASTEXITCODE -ne 0) { throw "$Program failed with exit code $LASTEXITCODE." }
    }
    Invoke-Checked cargo @('fmt', '--all', '--', '--check')
    Invoke-Checked cargo @('check', '--workspace')
    Invoke-Checked cargo @('clippy', '--workspace', '--all-targets', '--', '-D', 'warnings')
    Invoke-Checked cargo @('test', '--workspace')
    Invoke-Checked pnpm @('--filter', '@fishmuse/desktop', 'lint')
    Invoke-Checked pnpm @('--filter', '@fishmuse/desktop', 'typecheck')
    Invoke-Checked pnpm @('--filter', '@fishmuse/desktop', 'test', '--run')
}
finally {
    Pop-Location
}

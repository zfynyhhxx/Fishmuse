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
    if ([System.Environment]::OSVersion.Platform -eq [System.PlatformID]::Win32NT) {
        Invoke-Checked cargo @('build', '-p', 'fishmuse-desktop')
        $targetRoot = if ($env:CARGO_TARGET_DIR) {
            if ([System.IO.Path]::IsPathRooted($env:CARGO_TARGET_DIR)) {
                $env:CARGO_TARGET_DIR
            } else {
                Join-Path $root $env:CARGO_TARGET_DIR
            }
        } else {
            Join-Path $root 'target'
        }
        $powerShell = (Get-Process -Id $PID).Path
        Invoke-Checked $powerShell @(
            '-NoProfile',
            '-ExecutionPolicy', 'Bypass',
            '-File', (Join-Path $PSScriptRoot 'check-windows-subsystem.ps1'),
            '-Executable', (Join-Path $targetRoot 'debug/fishmuse-desktop.exe')
        )
    }
    Invoke-Checked pnpm @('--filter', '@fishmuse/desktop', 'lint')
    Invoke-Checked pnpm @('--filter', '@fishmuse/desktop', 'typecheck')
    Invoke-Checked pnpm @('--filter', '@fishmuse/desktop', 'test', '--run')
}
finally {
    Pop-Location
}

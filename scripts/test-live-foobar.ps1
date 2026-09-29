[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$FoobarPath,
    [Parameter(Mandatory = $true)][string]$OtherUser
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$componentRoot = Join-Path $root 'native\foo-fishmuse'
$smoke = Join-Path $root 'native\foo-fishmuse\tests\live_smoke.ps1'
$wrongSid = Join-Path $root 'native\foo-fishmuse\tests\wrong_sid_integration.ps1'
if (-not (Test-Path -LiteralPath $FoobarPath -PathType Leaf)) { throw "foobar2000 not found: $FoobarPath" }
if (Get-Process foobar2000 -ErrorAction SilentlyContinue) {
    throw 'Close all foobar2000 instances first; this gate owns and restarts its test process.'
}
if ((Read-Host 'This test starts, controls, stops, and restarts foobar2000. Type RUN') -cne 'RUN') {
    throw 'Live test cancelled.'
}

$work = Join-Path ([System.IO.Path]::GetTempPath()) ("fishmuse-live-foobar-" + [Guid]::NewGuid().ToString('N'))
$evidencePath = Join-Path $work 'evidence.json'
New-Item -ItemType Directory -Path $work | Out-Null
$process = $null
try {
    & cmake --preset windows-msvc -S $componentRoot
    if ($LASTEXITCODE -ne 0) { throw 'CMake configure for the wrong-SID probe failed.' }
    & cmake --build (Join-Path $componentRoot 'build\windows-msvc') --config Debug --target foo_fishmuse_tests
    if ($LASTEXITCODE -ne 0) { throw 'Building the wrong-SID probe failed.' }
    $process = Start-Process -FilePath $FoobarPath -PassThru
    & powershell -ExecutionPolicy Bypass -File $smoke -FixtureDirectory (Join-Path $work 'first') -AckTimeoutMs 500
    if ($LASTEXITCODE -ne 0) { throw 'Initial foobar live smoke failed.' }
    if (-not $process.CloseMainWindow() -or -not $process.WaitForExit(10000)) {
        Stop-Process -Id $process.Id -Force
        $process.WaitForExit()
    }
    $process = Start-Process -FilePath $FoobarPath -PassThru
    & powershell -ExecutionPolicy Bypass -File $smoke -FixtureDirectory (Join-Path $work 'restart') -AckTimeoutMs 500
    if ($LASTEXITCODE -ne 0) { throw 'Post-restart foobar live smoke failed.' }
    & powershell -ExecutionPolicy Bypass -File $wrongSid -OtherUser $OtherUser
    if ($LASTEXITCODE -ne 0) { throw 'Wrong-SID isolation check failed.' }

    @{
        handshake_and_commands = $true
        ack_within_500_ms = $true
        restart_and_snapshot = $true
        operation_id_replay = $true
        wrong_sid_denied = $true
    } | ConvertTo-Json | Set-Content -LiteralPath $evidencePath -Encoding utf8
    Push-Location $root
    try {
        $env:FISHMUSE_LIVE_FOOBAR_APPROVAL = 'interactive-approved'
        $env:FISHMUSE_LIVE_FOOBAR_EVIDENCE = $evidencePath
        & cargo test -p fishmuse-live-tests --test foobar_live foobar_live_acceptance_evidence -- --ignored --exact
        if ($LASTEXITCODE -ne 0) { throw 'Foobar live evidence validation failed.' }
    }
    finally {
        Remove-Item Env:FISHMUSE_LIVE_FOOBAR_APPROVAL -ErrorAction SilentlyContinue
        Remove-Item Env:FISHMUSE_LIVE_FOOBAR_EVIDENCE -ErrorAction SilentlyContinue
        Pop-Location
    }
}
finally {
    if ($null -ne $process -and -not $process.HasExited) { Stop-Process -Id $process.Id -Force }
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}

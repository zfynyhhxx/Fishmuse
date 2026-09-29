[CmdletBinding()]
param([switch]$ResetBudget)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$stateRoot = Join-Path $env:LOCALAPPDATA 'FishMuse\live-tests'
$ledger = Join-Path $stateRoot 'deepseek-budget.json'
$auditRoot = Join-Path $stateRoot 'audit'
New-Item -ItemType Directory -Path $stateRoot -Force | Out-Null

if ($ResetBudget) {
    if ((Read-Host 'Type RESET to archive and reset the DeepSeek live-test ledger') -cne 'RESET') {
        throw 'Budget reset cancelled.'
    }
    New-Item -ItemType Directory -Path $auditRoot -Force | Out-Null
    if (Test-Path -LiteralPath $ledger) {
        $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
        Copy-Item -LiteralPath $ledger -Destination (Join-Path $auditRoot "deepseek-budget-$stamp.json")
    }
    @{ spent_microunits = 0 } | ConvertTo-Json | Set-Content -LiteralPath $ledger -Encoding utf8
    Write-Host 'Budget ledger reset after audit copy.'
    return
}

if (-not (Test-Path -LiteralPath $ledger)) {
    @{ spent_microunits = 0 } | ConvertTo-Json | Set-Content -LiteralPath $ledger -Encoding utf8
}
$budget = Get-Content -LiteralPath $ledger -Raw | ConvertFrom-Json
$spent = [uint64]$budget.spent_microunits
$warningApproved = $false
if ($spent -ge 20000000) { throw 'DeepSeek live-test hard stop reached (CNY 20.00).' }
if ($spent -ge 10000000 -and (Read-Host 'Warning threshold reached. Type CONTINUE to spend more') -cne 'CONTINUE') {
    throw 'Live test cancelled at the warning threshold.'
}
if ($spent -ge 10000000) { $warningApproved = $true }
if ((Read-Host 'This test sends two real requests and spends DeepSeek credit. Type RUN') -cne 'RUN') {
    throw 'Live test cancelled.'
}

Push-Location $root
try {
    $env:FISHMUSE_LIVE_DEEPSEEK_APPROVAL = 'interactive-approved'
    if ($warningApproved) { $env:FISHMUSE_LIVE_DEEPSEEK_WARNING_APPROVAL = 'interactive-approved' }
    & cargo test -p fishmuse-live-tests --test deepseek_live deepseek_text_stream_and_usage_live -- --ignored --exact --nocapture
    if ($LASTEXITCODE -ne 0) { throw "DeepSeek text live test failed with exit code $LASTEXITCODE." }
    $spent = [uint64]((Get-Content -LiteralPath $ledger -Raw | ConvertFrom-Json).spent_microunits)
    if ($spent -ge 20000000) { throw 'DeepSeek live-test hard stop reached after the text request.' }
    if ($spent -ge 10000000) {
        if ((Read-Host 'Warning threshold reached. Type CONTINUE before the tool request') -cne 'CONTINUE') {
            throw 'Live tool test cancelled at the warning threshold.'
        }
        $env:FISHMUSE_LIVE_DEEPSEEK_WARNING_APPROVAL = 'interactive-approved'
    }
    & cargo test -p fishmuse-live-tests --test deepseek_live deepseek_search_tool_and_usage_live -- --ignored --exact --nocapture
    if ($LASTEXITCODE -ne 0) { throw "DeepSeek tool live test failed with exit code $LASTEXITCODE." }
}
finally {
    Remove-Item Env:FISHMUSE_LIVE_DEEPSEEK_APPROVAL -ErrorAction SilentlyContinue
    Remove-Item Env:FISHMUSE_LIVE_DEEPSEEK_WARNING_APPROVAL -ErrorAction SilentlyContinue
    Pop-Location
}

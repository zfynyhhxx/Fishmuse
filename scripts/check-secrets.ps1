[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $patterns = @(
        ('s' + 'k-[A-Za-z0-9]{20,}'),
        ('-----BEGIN ' + '(RSA |EC |OPENSSH )?PRIVATE KEY-----'),
        ('AKIA' + '[A-Z0-9]{16}'),
        ('(?i)(api[_-]?key|secret|password)\s*[:=]\s*["''][^"'']{12,}["'']')
    )
    $files = @(& git ls-files --cached --others --exclude-standard)
    if ($LASTEXITCODE -ne 0) { throw 'git ls-files failed.' }
    $findings = [System.Collections.Generic.List[string]]::new()
    foreach ($relative in $files) {
        if ($relative -eq 'scripts/check-secrets.ps1') { continue }
        $path = Join-Path $root $relative
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { continue }
        try { $lines = [System.IO.File]::ReadAllLines($path) } catch { continue }
        for ($index = 0; $index -lt $lines.Count; $index++) {
            if ($lines[$index] -match 'secret-scan: allow-fixture') {
                if ($relative -notmatch '(test|fixture)' -or $lines[$index] -notmatch 'never-render') {
                    $findings.Add("${relative}:$($index + 1) has an invalid secret-scan allow marker")
                }
                continue
            }
            foreach ($pattern in $patterns) {
                if ($lines[$index] -match $pattern) {
                    $findings.Add("${relative}:$($index + 1) matched $pattern")
                }
            }
        }
    }
    if ($findings.Count -gt 0) {
        $findings | ForEach-Object { Write-Host $_ -ForegroundColor Red }
        throw 'Potential credential material was found.'
    }
    Write-Host "PASS: scanned $($files.Count) tracked/untracked source files; no credential patterns found."
}
finally {
    Pop-Location
}

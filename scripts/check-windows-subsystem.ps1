[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$Executable
)

$ErrorActionPreference = 'Stop'
$resolved = (Resolve-Path -LiteralPath $Executable).Path
$stream = [System.IO.FileStream]::new(
    $resolved,
    [System.IO.FileMode]::Open,
    [System.IO.FileAccess]::Read,
    [System.IO.FileShare]::ReadWrite -bor [System.IO.FileShare]::Delete
)
$reader = [System.IO.BinaryReader]::new($stream)
try {
    if ($stream.Length -lt 64) { throw "'$resolved' is too small to be a PE executable." }
    $stream.Position = 0x3c
    $peOffset = $reader.ReadInt32()
    if ($peOffset -lt 0 -or $peOffset + 94 -gt $stream.Length) {
        throw "'$resolved' has an invalid PE header offset."
    }
    $stream.Position = $peOffset
    if ($reader.ReadUInt32() -ne 0x00004550) {
        throw "'$resolved' does not contain a PE signature."
    }
    $optionalHeader = $peOffset + 24
    $stream.Position = $optionalHeader
    $magic = $reader.ReadUInt16()
    if ($magic -ne 0x10b -and $magic -ne 0x20b) {
        throw "'$resolved' has an unsupported PE optional-header magic."
    }
    $stream.Position = $optionalHeader + 68
    $subsystem = $reader.ReadUInt16()
    if ($subsystem -ne 2) {
        throw "'$resolved' uses PE subsystem $subsystem; expected 2 (Windows GUI)."
    }
    Write-Host "Verified Windows GUI subsystem: $resolved"
}
finally {
    $reader.Dispose()
    $stream.Dispose()
}

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$FixtureDirectory,

    [int]$ConnectTimeoutMs = 10000,

    [int]$AckTimeoutMs = 500
)

$ErrorActionPreference = 'Stop'
$protocolVersion = 1
$maximumFrameBytes = 1024 * 1024
$eventSequences = [System.Collections.Generic.List[uint64]]::new()

function Get-PipeName {
    $sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($sid)
    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    try {
        $hash = $sha256.ComputeHash($bytes)
    }
    finally {
        $sha256.Dispose()
    }
    $hex = [System.BitConverter]::ToString($hash).Replace('-', '').ToLowerInvariant()
    return "FishMuse.Foobar.v1.$hex"
}

function New-Envelope {
    param(
        [Parameter(Mandatory = $true)][string]$Kind,
        [Parameter(Mandatory = $true)][object]$Payload,
        [string]$MessageId = [Guid]::NewGuid().ToString(),
        [AllowNull()][object]$CorrelationId = $null,
        [AllowNull()][object]$Sequence = $null
    )
    return [ordered]@{
        protocolVersion = $protocolVersion
        messageId = $MessageId
        correlationId = $CorrelationId
        sentAtUnixMs = [uint64][DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        kind = $Kind
        sequence = $Sequence
        payload = $Payload
    }
}

function ConvertTo-WireJson {
    param([Parameter(Mandatory = $true)][object]$Document)
    return $Document | ConvertTo-Json -Compress -Depth 20
}

function Write-Frame {
    param(
        [Parameter(Mandatory = $true)][System.IO.Stream]$Stream,
        [Parameter(Mandatory = $true)][string]$Json
    )
    $payload = [System.Text.Encoding]::UTF8.GetBytes($Json)
    if ($payload.Length -eq 0 -or $payload.Length -gt $maximumFrameBytes) {
        throw "Invalid outgoing frame length: $($payload.Length)."
    }
    $header = [System.BitConverter]::GetBytes([uint32]$payload.Length)
    $Stream.Write($header, 0, $header.Length)
    $Stream.Write($payload, 0, $payload.Length)
    $Stream.Flush()
}

function Read-Exact {
    param(
        [Parameter(Mandatory = $true)][System.IO.Stream]$Stream,
        [Parameter(Mandatory = $true)][int]$Count,
        [Parameter(Mandatory = $true)][int]$TimeoutMs
    )
    $buffer = [byte[]]::new($Count)
    $offset = 0
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    while ($offset -lt $Count) {
        $remaining = $TimeoutMs - [int]$watch.ElapsedMilliseconds
        if ($remaining -le 0) {
            throw "Timed out after ${TimeoutMs}ms while reading a pipe frame."
        }
        $readTask = $Stream.ReadAsync($buffer, $offset, $Count - $offset)
        if (-not $readTask.Wait($remaining)) {
            throw "Timed out after ${TimeoutMs}ms while reading a pipe frame."
        }
        $read = $readTask.Result
        if ($read -eq 0) {
            throw 'Pipe closed before a complete frame was received.'
        }
        $offset += $read
    }
    return $buffer
}

function Read-Frame {
    param(
        [Parameter(Mandatory = $true)][System.IO.Stream]$Stream,
        [Parameter(Mandatory = $true)][int]$TimeoutMs
    )
    $header = Read-Exact -Stream $Stream -Count 4 -TimeoutMs $TimeoutMs
    $length = [System.BitConverter]::ToUInt32($header, 0)
    if ($length -eq 0 -or $length -gt $maximumFrameBytes) {
        throw "Invalid incoming frame length: $length."
    }
    $payload = Read-Exact -Stream $Stream -Count ([int]$length) -TimeoutMs $TimeoutMs
    $json = [System.Text.Encoding]::UTF8.GetString($payload)
    return [pscustomobject]@{
        Raw = $json
        Document = $json | ConvertFrom-Json
    }
}

function Send-AndWait {
    param(
        [Parameter(Mandatory = $true)][System.IO.Stream]$Stream,
        [Parameter(Mandatory = $true)][string]$Json,
        [Parameter(Mandatory = $true)][string]$MessageId,
        [int]$TimeoutMs = $AckTimeoutMs
    )
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    Write-Frame -Stream $Stream -Json $Json
    while ($watch.ElapsedMilliseconds -le $TimeoutMs) {
        $remaining = $TimeoutMs - [int]$watch.ElapsedMilliseconds
        $message = Read-Frame -Stream $Stream -TimeoutMs $remaining
        if ($message.Document.kind -eq 'playback.event') {
            $eventSequences.Add([uint64]$message.Document.sequence)
            continue
        }
        if ($message.Document.correlationId -eq $MessageId) {
            $watch.Stop()
            return [pscustomobject]@{
                Raw = $message.Raw
                Document = $message.Document
                ElapsedMs = $watch.ElapsedMilliseconds
            }
        }
    }
    throw "Timed out waiting for response to $MessageId."
}

function Invoke-CommandRequest {
    param(
        [Parameter(Mandatory = $true)][System.IO.Stream]$Stream,
        [Parameter(Mandatory = $true)][object]$Command,
        [string]$OperationId = [Guid]::NewGuid().ToString(),
        [string]$MessageId = [Guid]::NewGuid().ToString()
    )
    $request = New-Envelope -Kind 'command.request' -MessageId $MessageId -Payload ([ordered]@{
        operationId = $OperationId
        command = $Command
    })
    $json = ConvertTo-WireJson $request
    $response = Send-AndWait -Stream $Stream -Json $json -MessageId $MessageId
    if ($response.Document.kind -ne 'command.ack' -or
        -not $response.Document.payload.accepted) {
        throw "Command failed: $($response.Raw)"
    }
    if ($response.ElapsedMs -gt $AckTimeoutMs) {
        throw "Command ACK exceeded ${AckTimeoutMs}ms: $($response.ElapsedMs)ms."
    }
    return [pscustomobject]@{
        RequestJson = $json
        MessageId = $MessageId
        OperationId = $OperationId
        Response = $response
    }
}

function Wait-ForPlaybackReady {
    param(
        [Parameter(Mandatory = $true)][System.IO.Stream]$Stream,
        [Parameter(Mandatory = $true)][string]$TrackId,
        [int]$TimeoutMs = 5000
    )
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    while ($watch.ElapsedMilliseconds -le $TimeoutMs) {
        $state = Invoke-CommandRequest -Stream $Stream -Command ([ordered]@{
            name = 'get_state'
        })
        $snapshot = $state.Response.Document.payload.snapshot
        if ($snapshot.status -eq 'playing' -and $snapshot.trackId -eq $TrackId -and
            $null -ne $snapshot.durationMs) {
            return $state
        }
        Start-Sleep -Milliseconds 50
    }
    throw "Playback did not become seek-ready within ${TimeoutMs}ms."
}

function New-WaveFixture {
    param([Parameter(Mandatory = $true)][string]$Path)
    $sampleRate = 44100
    $channels = 1
    $bitsPerSample = 16
    $seconds = 8
    $sampleCount = $sampleRate * $seconds
    $dataBytes = $sampleCount * $channels * ($bitsPerSample / 8)
    $stream = [System.IO.File]::Open($Path, [System.IO.FileMode]::Create,
        [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
    $writer = [System.IO.BinaryWriter]::new($stream)
    try {
        $writer.Write([System.Text.Encoding]::ASCII.GetBytes('RIFF'))
        $writer.Write([uint32](36 + $dataBytes))
        $writer.Write([System.Text.Encoding]::ASCII.GetBytes('WAVE'))
        $writer.Write([System.Text.Encoding]::ASCII.GetBytes('fmt '))
        $writer.Write([uint32]16)
        $writer.Write([uint16]1)
        $writer.Write([uint16]$channels)
        $writer.Write([uint32]$sampleRate)
        $writer.Write([uint32]($sampleRate * $channels * ($bitsPerSample / 8)))
        $writer.Write([uint16]($channels * ($bitsPerSample / 8)))
        $writer.Write([uint16]$bitsPerSample)
        $writer.Write([System.Text.Encoding]::ASCII.GetBytes('data'))
        $writer.Write([uint32]$dataBytes)
        for ($index = 0; $index -lt $sampleCount; $index++) {
            $sample = [int16]([Math]::Sin(2.0 * [Math]::PI * 440.0 * $index / $sampleRate) * 4096.0)
            $writer.Write($sample)
        }
    }
    finally {
        $writer.Dispose()
        $stream.Dispose()
    }
}

New-Item -ItemType Directory -Path $FixtureDirectory -Force | Out-Null
$fixtureRoot = (Resolve-Path -LiteralPath $FixtureDirectory).Path
$fixture = Join-Path $fixtureRoot 'fishmuse-live-smoke.wav'
New-WaveFixture -Path $fixture

$pipe = [System.IO.Pipes.NamedPipeClientStream]::new(
    '.', (Get-PipeName), [System.IO.Pipes.PipeDirection]::InOut,
    [System.IO.Pipes.PipeOptions]::None)
try {
    $pipe.Connect($ConnectTimeoutMs)

    $handshakeId = [Guid]::NewGuid().ToString()
    $handshake = New-Envelope -Kind 'handshake.request' -MessageId $handshakeId -Payload ([ordered]@{
        appVersion = '0.1.0-live-smoke'
        supportedProtocolVersions = @(1)
        processId = [uint32]$PID
        nonce = [Guid]::NewGuid().ToString('N')
    })
    $handshakeResponse = Send-AndWait -Stream $pipe `
        -Json (ConvertTo-WireJson $handshake) -MessageId $handshakeId
    if ($handshakeResponse.Document.kind -ne 'handshake.response' -or
        $handshakeResponse.Document.payload.selectedProtocolVersion -ne 1) {
        throw "Handshake failed: $($handshakeResponse.Raw)"
    }

    $trackId = [Guid]::NewGuid().ToString()
    $play = Invoke-CommandRequest -Stream $pipe -Command ([ordered]@{
        name = 'play'
        trackId = $trackId
        path = $fixture
        subsongIndex = $null
        startMs = [uint64]100
        endMs = $null
    })
    $ready = Wait-ForPlaybackReady -Stream $pipe -TrackId $trackId
    $null = Invoke-CommandRequest -Stream $pipe -Command ([ordered]@{ name = 'pause' })
    $null = Invoke-CommandRequest -Stream $pipe -Command ([ordered]@{ name = 'resume' })
    $null = Invoke-CommandRequest -Stream $pipe -Command ([ordered]@{
        name = 'seek'
        positionMs = [uint64]1000
    })

    $replayOperationId = [Guid]::NewGuid().ToString()
    $replayMessageId = [Guid]::NewGuid().ToString()
    $volume = Invoke-CommandRequest -Stream $pipe -OperationId $replayOperationId `
        -MessageId $replayMessageId -Command ([ordered]@{
            name = 'set_volume'
            volume = 0.5
        })
    $replayed = Send-AndWait -Stream $pipe -Json $volume.RequestJson `
        -MessageId $volume.MessageId
    if ($replayed.Raw -cne $volume.Response.Raw) {
        throw 'ACK-loss retry did not replay the byte-identical cached result.'
    }

    $state = Invoke-CommandRequest -Stream $pipe -Command ([ordered]@{ name = 'get_state' })
    if ($state.Response.Document.payload.snapshot.backend -ne 'foobar2000' -or
        $state.Response.Document.payload.snapshot.trackId -ne $trackId) {
        throw "Authoritative state did not retain the FishMuse track identity: $($state.Response.Raw)"
    }
    $null = Invoke-CommandRequest -Stream $pipe -Command ([ordered]@{ name = 'skip_next' })
    $stopped = Invoke-CommandRequest -Stream $pipe -Command ([ordered]@{ name = 'stop' })
    if ($stopped.Response.Document.payload.snapshot.status -ne 'stopped') {
        throw "Stop did not produce a stopped snapshot: $($stopped.Response.Raw)"
    }

    if ($eventSequences.Count -eq 0) {
        throw 'No playback events were received from the real component.'
    }
    for ($index = 1; $index -lt $eventSequences.Count; $index++) {
        if ($eventSequences[$index] -le $eventSequences[$index - 1]) {
            throw 'Playback event sequence was not strictly increasing.'
        }
    }

    $maximumAck = @(
        $handshakeResponse.ElapsedMs,
        $play.Response.ElapsedMs,
        $ready.Response.ElapsedMs,
        $volume.Response.ElapsedMs,
        $state.Response.ElapsedMs,
        $stopped.Response.ElapsedMs
    ) | Measure-Object -Maximum | Select-Object -ExpandProperty Maximum
    Write-Host "PASS: live foobar bridge handshake, commands, replay, events, and snapshots; max sampled ACK ${maximumAck}ms."
}
finally {
    $pipe.Dispose()
}

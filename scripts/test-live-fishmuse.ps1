[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$AudioFixture,
    [Parameter(Mandatory = $true)][switch]$Approve,
    [string]$EvidencePath,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$desktop = Join-Path $root 'apps\desktop'
$binary = Join-Path $root 'target\release\fishmuse-desktop.exe'
if (-not $Approve) { throw 'Pass -Approve to opt in to starting and controlling the installed playback component.' }
if (-not (Test-Path -LiteralPath $AudioFixture -PathType Leaf)) { throw "Audio fixture not found: $AudioFixture" }
$baselineBackendIds = @(Get-Process foobar2000 -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id)
$baselineFishMuseIds = @(Get-Process fishmuse-desktop -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id)
if ($baselineBackendIds.Count -gt 0) {
    throw 'A foobar2000 process is already running. Close it yourself; this gate never hides, controls, or stops pre-existing processes.'
}
if ($baselineFishMuseIds.Count -gt 0) {
    throw 'A FishMuse process is already running. Close it yourself before running the isolated live gate.'
}

$appPathKeys = @(
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\App Paths\foobar2000.exe',
    'HKLM:\Software\Microsoft\Windows\CurrentVersion\App Paths\foobar2000.exe',
    'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\App Paths\foobar2000.exe'
)
$installed = $appPathKeys | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if (-not $installed -and -not (Get-Command foobar2000.exe -ErrorAction SilentlyContinue)) {
    throw 'The configured playback application is not registered in Windows App Paths or PATH.'
}
$foobarExecutable = if ($installed) {
    (Get-ItemProperty -LiteralPath $installed).'(default)'
}
else {
    (Get-Command foobar2000.exe -ErrorAction Stop).Source
}
if (-not (Test-Path -LiteralPath $foobarExecutable -PathType Leaf)) {
    throw "The registered playback executable was not found: $foobarExecutable"
}

if (-not $EvidencePath) {
    $EvidencePath = Join-Path $root 'target\live\fishmuse-desktop-evidence.json'
}
$EvidencePath = [System.IO.Path]::GetFullPath($EvidencePath)
$evidenceDirectory = Split-Path -Parent $EvidencePath
New-Item -ItemType Directory -Path $evidenceDirectory -Force | Out-Null
$screenshotPath = Join-Path $evidenceDirectory 'fishmuse-now-playing.png'

$tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$work = [System.IO.Path]::GetFullPath((Join-Path $tempRoot ("fishmuse-desktop-live-" + [Guid]::NewGuid().ToString('N'))))
if (-not $work.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing to use a live-test work directory outside the system temp directory: $work"
}
$fixtureRoot = Join-Path $work 'music'
$webViewData = Join-Path $work 'webview2'
$observationsPath = Join-Path $work 'ui-observations.json'
$diagnosticsPath = Join-Path $work 'diagnostics.jsonl'
$fishMusePidPath = Join-Path $work 'fishmuse.pid'
$backendPidPath = Join-Path $work 'backend.pid'
$monitorCompletePath = Join-Path $work 'playback-monitor.complete'
$roamingRoot = [System.IO.Path]::GetFullPath([Environment]::GetFolderPath('ApplicationData'))
$appData = [System.IO.Path]::GetFullPath((Join-Path $roamingRoot 'com.fishmuse.desktop.live'))
if ((Split-Path -Parent $appData) -ne $roamingRoot) { throw "Invalid isolated application data path: $appData" }
if (Test-Path -LiteralPath $appData) {
    throw "The isolated live-test application data directory already exists: $appData. Review and remove it explicitly before rerunning."
}
New-Item -ItemType Directory -Path $fixtureRoot,$webViewData | Out-Null
$extension = [System.IO.Path]::GetExtension($AudioFixture)
Copy-Item -LiteralPath $AudioFixture -Destination (Join-Path $fixtureRoot ("01-live" + $extension))
Copy-Item -LiteralPath $AudioFixture -Destination (Join-Path $fixtureRoot ("02-live" + $extension))

$priorWebViewData = $env:WEBVIEW2_USER_DATA_FOLDER
$priorBinary = $env:FISHMUSE_LIVE_BINARY
$priorFixture = $env:FISHMUSE_LIVE_FIXTURE_ROOT
$priorObservations = $env:FISHMUSE_LIVE_OBSERVATIONS
$priorDiagnostics = $env:FISHMUSE_LIVE_DIAGNOSTICS
$priorScreenshot = $env:FISHMUSE_LIVE_SCREENSHOT
$priorFishMusePid = $env:FISHMUSE_LIVE_PROCESS_PID
$priorBackendPid = $env:FISHMUSE_LIVE_BACKEND_PID
$priorMonitorComplete = $env:FISHMUSE_LIVE_MONITOR_COMPLETE
$ownedBackendIds = @()
$windowMonitorStarted = $false
$windowObservation = @(0, 0)

function Get-OwnedProcess {
    param(
        [Parameter(Mandatory = $true)][string]$PidPath,
        [Parameter(Mandatory = $true)][string]$ExpectedName,
        [Parameter(Mandatory = $true)][string]$ExpectedPath
    )

    if (-not (Test-Path -LiteralPath $PidPath -PathType Leaf)) { return $null }
    $raw = (Get-Content -LiteralPath $PidPath -Raw -Encoding ascii).Trim()
    $identity = $raw.Split('|')
    $processId = 0
    $creationTime = [long]0
    if ($identity.Count -ne 2 -or
        -not [int]::TryParse($identity[0], [ref]$processId) -or $processId -le 0 -or
        -not [long]::TryParse($identity[1], [ref]$creationTime) -or $creationTime -le 0) {
        throw "Invalid test-owned process identity in $PidPath."
    }
    $process = Get-Process -Id $processId -ErrorAction SilentlyContinue
    if (-not $process) { return $null }
    if (-not [FishMuseLiveWindows]::IsExactProcess($processId, $creationTime)) {
        throw "Refusing to control PID $processId because its creation time does not match the test-owned process."
    }
    if ($process.ProcessName -ne $ExpectedName) {
        throw "Refusing to control PID $processId because its process name is '$($process.ProcessName)', not '$ExpectedName'."
    }
    $actualPath = [System.IO.Path]::GetFullPath($process.Path)
    $expectedFullPath = [System.IO.Path]::GetFullPath($ExpectedPath)
    if (-not $actualPath.Equals($expectedFullPath, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to control PID $processId because its executable is '$actualPath', not '$expectedFullPath'."
    }
    $process | Add-Member -NotePropertyName FishMuseCreationTime -NotePropertyValue $creationTime -Force
    return $process
}

Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;
public static class FishMuseLiveWindows {
    private delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);
    [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
    [DllImport("user32.dll")] private static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] private static extern bool PostMessage(IntPtr hWnd, uint message, IntPtr wParam, IntPtr lParam);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern IntPtr OpenProcess(uint access, bool inheritHandle, int processId);
    [DllImport("kernel32.dll")] private static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool GetProcessTimes(IntPtr process, out long creation, out long exit, out long kernel, out long user);
    [DllImport("kernel32.dll")] private static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool TerminateProcess(IntPtr process, uint exitCode);
    private const uint WM_CLOSE = 0x0010;
    private const uint PROCESS_TERMINATE = 0x0001;
    private const uint SYNCHRONIZE = 0x00100000;
    private const uint PROCESS_QUERY_LIMITED_INFORMATION = 0x1000;
    private const uint WAIT_OBJECT_0 = 0x00000000;
    private static Thread monitor;
    private static volatile bool stopRequested;
    private static volatile bool observedVisible;
    private static volatile bool observedForeground;
    private static volatile bool foregroundWasVisible;
    private static volatile bool currentlyForeground;
    private static long foregroundAtMilliseconds;
    private static long maximumForegroundMilliseconds;
    private static DateTime monitorStartedAt;
    private static DateTime foregroundStartedAt;

    private sealed class ProcessIdentity {
        public int Id;
        public long CreationTime;
    }

    private static ProcessIdentity ReadProcessIdentity(string path) {
        try {
            if (!File.Exists(path)) return null;
            var parts = File.ReadAllText(path).Trim().Split('|');
            int processId;
            long creationTime;
            if (parts.Length != 2 ||
                !Int32.TryParse(parts[0], out processId) || processId <= 0 ||
                !Int64.TryParse(parts[1], out creationTime) || creationTime <= 0) return null;
            return new ProcessIdentity { Id = processId, CreationTime = creationTime };
        }
        catch { return null; }
    }

    private static IntPtr OpenExactProcess(int processId, long creationTime, uint access) {
        if (processId <= 0 || creationTime <= 0) return IntPtr.Zero;
        var process = OpenProcess(access | PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, false, processId);
        if (process == IntPtr.Zero) return IntPtr.Zero;
        long actualCreation;
        long exit;
        long kernel;
        long user;
        if (!GetProcessTimes(process, out actualCreation, out exit, out kernel, out user) || actualCreation != creationTime) {
            CloseHandle(process);
            return IntPtr.Zero;
        }
        return process;
    }

    public static bool IsExactProcess(int processId, long creationTime) {
        var process = OpenExactProcess(processId, creationTime, 0);
        if (process == IntPtr.Zero) return false;
        CloseHandle(process);
        return true;
    }

    private static void ObserveProcess(ProcessIdentity identity) {
        if (identity == null) return;
        var process = OpenExactProcess(identity.Id, identity.CreationTime, 0);
        if (process == IntPtr.Zero) return;
        try {
            var foreground = GetForegroundWindow();
            var isForeground = false;
            EnumWindows((window, state) => {
                uint id;
                GetWindowThreadProcessId(window, out id);
                if (id == identity.Id) {
                    if (IsWindowVisible(window)) observedVisible = true;
                    if (window == foreground) {
                        isForeground = true;
                        if (!observedForeground) {
                            foregroundWasVisible = IsWindowVisible(window);
                            foregroundAtMilliseconds = (long)(DateTime.UtcNow - monitorStartedAt).TotalMilliseconds;
                            observedForeground = true;
                        }
                    }
                }
                return true;
            }, IntPtr.Zero);
            var now = DateTime.UtcNow;
            if (isForeground && !currentlyForeground) foregroundStartedAt = now;
            if (!isForeground && currentlyForeground) {
                maximumForegroundMilliseconds = Math.Max(
                    maximumForegroundMilliseconds,
                    (long)(now - foregroundStartedAt).TotalMilliseconds
                );
            }
            currentlyForeground = isForeground;
        }
        finally {
            CloseHandle(process);
        }
    }

    public static void StartMonitor(string processIdPath, string completionPath) {
        stopRequested = false;
        observedVisible = false;
        observedForeground = false;
        foregroundWasVisible = false;
        currentlyForeground = false;
        foregroundAtMilliseconds = -1;
        maximumForegroundMilliseconds = 0;
        monitorStartedAt = DateTime.UtcNow;
        monitor = new Thread(() => {
            while (!stopRequested && !File.Exists(completionPath)) {
                ObserveProcess(ReadProcessIdentity(processIdPath));
                Thread.Sleep(25);
            }
            if (!File.Exists(completionPath)) ObserveProcess(ReadProcessIdentity(processIdPath));
        });
        monitor.IsBackground = true;
        monitor.Start();
    }

    public static int[] StopMonitor() {
        stopRequested = true;
        if (monitor != null && monitor.IsAlive) monitor.Join(2000);
        monitor = null;
        if (currentlyForeground) {
            maximumForegroundMilliseconds = Math.Max(
                maximumForegroundMilliseconds,
                (long)(DateTime.UtcNow - foregroundStartedAt).TotalMilliseconds
            );
        }
        return new int[] {
            observedVisible ? 1 : 0,
            observedForeground ? 1 : 0,
            (int)Math.Min(Int32.MaxValue, foregroundAtMilliseconds),
            foregroundWasVisible ? 1 : 0,
            currentlyForeground ? 1 : 0,
            (int)Math.Min(Int32.MaxValue, maximumForegroundMilliseconds)
        };
    }

    public static bool IsOwnedProcessVisible(int processId, long creationTime) {
        var process = OpenExactProcess(processId, creationTime, 0);
        if (process == IntPtr.Zero) return false;
        var visible = false;
        try {
            EnumWindows((window, state) => {
                uint id;
                GetWindowThreadProcessId(window, out id);
                if (id == processId && IsWindowVisible(window)) visible = true;
                return true;
            }, IntPtr.Zero);
            return visible;
        }
        finally {
            CloseHandle(process);
        }
    }

    public static void CloseWindowsForOwnedProcess(int processId, long creationTime) {
        var process = OpenExactProcess(processId, creationTime, 0);
        if (process == IntPtr.Zero) return;
        try {
            EnumWindows((window, state) => {
                uint id;
                GetWindowThreadProcessId(window, out id);
                if (id == processId) PostMessage(window, WM_CLOSE, IntPtr.Zero, IntPtr.Zero);
                return true;
            }, IntPtr.Zero);
        }
        finally {
            CloseHandle(process);
        }
    }

    public static bool WaitForOwnedProcessExit(int processId, long creationTime, uint milliseconds) {
        var process = OpenExactProcess(processId, creationTime, 0);
        if (process == IntPtr.Zero) return true;
        try { return WaitForSingleObject(process, milliseconds) == WAIT_OBJECT_0; }
        finally { CloseHandle(process); }
    }

    public static void TerminateOwnedProcess(int processId, long creationTime) {
        var process = OpenExactProcess(processId, creationTime, PROCESS_TERMINATE);
        if (process == IntPtr.Zero) return;
        try { TerminateProcess(process, 1); }
        finally { CloseHandle(process); }
    }
}
'@

try {
    Push-Location $desktop
    try {
        if (-not $SkipBuild) {
            & pnpm exec tauri build --no-bundle --features live-e2e --config src-tauri/tauri.live.conf.json
            if ($LASTEXITCODE -ne 0) { throw "Release-mode FishMuse live build failed with exit code $LASTEXITCODE." }
        }
        if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) { throw "Live binary not found: $binary" }

        $env:WEBVIEW2_USER_DATA_FOLDER = $webViewData
        $env:FISHMUSE_LIVE_BINARY = $binary
        $env:FISHMUSE_LIVE_FIXTURE_ROOT = $fixtureRoot
        $env:FISHMUSE_LIVE_OBSERVATIONS = $observationsPath
        $env:FISHMUSE_LIVE_DIAGNOSTICS = $diagnosticsPath
        $env:FISHMUSE_LIVE_SCREENSHOT = $screenshotPath
        $env:FISHMUSE_LIVE_PROCESS_PID = $fishMusePidPath
        $env:FISHMUSE_LIVE_BACKEND_PID = $backendPidPath
        $env:FISHMUSE_LIVE_MONITOR_COMPLETE = $monitorCompletePath
        [FishMuseLiveWindows]::StartMonitor($backendPidPath, $monitorCompletePath)
        $windowMonitorStarted = $true
        & pnpm exec wdio run wdio.live.conf.ts
        $windowObservation = [FishMuseLiveWindows]::StopMonitor()
        $windowMonitorStarted = $false
        if ($LASTEXITCODE -ne 0) {
            if (Test-Path -LiteralPath $diagnosticsPath -PathType Leaf) {
                Write-Host 'Redacted live diagnostics:'
                Get-Content -LiteralPath $diagnosticsPath -Tail 20
            }
            $failedBackend = Get-OwnedProcess -PidPath $backendPidPath -ExpectedName 'foobar2000' -ExpectedPath $foobarExecutable
            Write-Host "Live failure test-owned backend PID: $(if ($failedBackend) { $failedBackend.Id } else { 'none' })"
            throw "FishMuse live WebdriverIO gate failed with exit code $LASTEXITCODE."
        }
    }
    finally {
        Pop-Location
    }

    Start-Sleep -Milliseconds 500
    if (-not (Test-Path -LiteralPath $observationsPath -PathType Leaf)) { throw 'The live UI did not write its bounded observations.' }
    if (-not (Test-Path -LiteralPath $screenshotPath -PathType Leaf)) { throw 'The live UI did not capture the Now Playing evidence screenshot.' }
    $observations = Get-Content -LiteralPath $observationsPath -Raw -Encoding utf8 | ConvertFrom-Json
    $ownedBackend = Get-OwnedProcess -PidPath $backendPidPath -ExpectedName 'foobar2000' -ExpectedPath $foobarExecutable
    if (-not $ownedBackend) { throw 'FishMuse did not leave its precisely identified playback backend running for verification.' }
    $ownedBackendIds = @($ownedBackend.Id)
    $visibleBackendIds = @($ownedBackendIds | Where-Object {
        [FishMuseLiveWindows]::IsOwnedProcessVisible($_, $ownedBackend.FishMuseCreationTime)
    })
    $fishMuseShutdownClean = -not [bool](Get-OwnedProcess -PidPath $fishMusePidPath -ExpectedName 'fishmuse-desktop' -ExpectedPath $binary)
    $databaseFiles = @(Get-ChildItem -LiteralPath $appData -Filter fishmuse.sqlite3 -File -Recurse)
    if ($databaseFiles.Count -ne 1) { throw "Expected one isolated FishMuse database, found $($databaseFiles.Count)." }

    $evidence = [ordered]@{
        backend_initially_stopped = $true
        fishmuse_play_started_backend = ($ownedBackendIds.Count -gt 0)
        no_visible_backend_window = (($windowObservation[0] -eq 0) -and ($visibleBackendIds.Count -eq 0))
        backend_never_foreground = ($windowObservation[1] -eq 0)
        playback_reached_playing = [bool]$observations.playback_reached_playing
        pause_resume = [bool]$observations.pause_resume
        seek = [bool]$observations.seek
        volume = [bool]$observations.volume
        mute_unmute = [bool]$observations.mute_unmute
        next_previous = [bool]$observations.next_previous
        stop = [bool]$observations.stop
        listening_history_persisted = $true
        fishmuse_shutdown_clean = $fishMuseShutdownClean
        ui_only_playback_commands = [bool]$observations.ui_only_playback_commands
    }
    if ($windowObservation[1] -ne 0) {
        Write-Host "Backend foreground observation: $($windowObservation[2]) ms after monitor start; visible=$($windowObservation[3] -ne 0); current=$($windowObservation[4] -ne 0); longest=$($windowObservation[5]) ms."
    }
    $json = $evidence | ConvertTo-Json -Compress
    if ([Text.Encoding]::UTF8.GetByteCount($json) -gt 4096) { throw 'Live evidence exceeded its 4096-byte bound.' }
    [IO.File]::WriteAllText($EvidencePath, $json, [Text.UTF8Encoding]::new($false))

    Push-Location $root
    try {
        $env:FISHMUSE_LIVE_DESKTOP_APPROVAL = 'interactive-approved'
        $env:FISHMUSE_LIVE_DESKTOP_EVIDENCE = $EvidencePath
        $env:FISHMUSE_LIVE_DESKTOP_DATABASE = $databaseFiles[0].FullName
        & cargo test -p fishmuse-live-tests --test fishmuse_desktop_live fishmuse_desktop_live_acceptance_evidence -- --ignored --exact
        if ($LASTEXITCODE -ne 0) { throw 'FishMuse live evidence or persisted listening history validation failed.' }
    }
    finally {
        Remove-Item Env:FISHMUSE_LIVE_DESKTOP_APPROVAL -ErrorAction SilentlyContinue
        Remove-Item Env:FISHMUSE_LIVE_DESKTOP_EVIDENCE -ErrorAction SilentlyContinue
        Remove-Item Env:FISHMUSE_LIVE_DESKTOP_DATABASE -ErrorAction SilentlyContinue
        Pop-Location
    }
    Write-Host "FishMuse live gate passed. Evidence: $EvidencePath"
}
finally {
    if ($windowMonitorStarted) {
        $windowObservation = [FishMuseLiveWindows]::StopMonitor()
        $windowMonitorStarted = $false
    }
    try {
        $cleanupFishMuse = Get-OwnedProcess -PidPath $fishMusePidPath -ExpectedName 'fishmuse-desktop' -ExpectedPath $binary
        if ($cleanupFishMuse) {
            [FishMuseLiveWindows]::TerminateOwnedProcess($cleanupFishMuse.Id, $cleanupFishMuse.FishMuseCreationTime)
        }
    }
    catch { Write-Warning $_ }
    try {
        $cleanupBackend = Get-OwnedProcess -PidPath $backendPidPath -ExpectedName 'foobar2000' -ExpectedPath $foobarExecutable
        if ($cleanupBackend) {
            [FishMuseLiveWindows]::CloseWindowsForOwnedProcess($cleanupBackend.Id, $cleanupBackend.FishMuseCreationTime)
            if (-not [FishMuseLiveWindows]::WaitForOwnedProcessExit($cleanupBackend.Id, $cleanupBackend.FishMuseCreationTime, 10000)) {
                [FishMuseLiveWindows]::TerminateOwnedProcess($cleanupBackend.Id, $cleanupBackend.FishMuseCreationTime)
            }
        }
    }
    catch { Write-Warning $_ }
    $env:WEBVIEW2_USER_DATA_FOLDER = $priorWebViewData
    $env:FISHMUSE_LIVE_BINARY = $priorBinary
    $env:FISHMUSE_LIVE_FIXTURE_ROOT = $priorFixture
    $env:FISHMUSE_LIVE_OBSERVATIONS = $priorObservations
    $env:FISHMUSE_LIVE_DIAGNOSTICS = $priorDiagnostics
    $env:FISHMUSE_LIVE_SCREENSHOT = $priorScreenshot
    $env:FISHMUSE_LIVE_PROCESS_PID = $priorFishMusePid
    $env:FISHMUSE_LIVE_BACKEND_PID = $priorBackendPid
    $env:FISHMUSE_LIVE_MONITOR_COMPLETE = $priorMonitorComplete
    if (Test-Path -LiteralPath $appData) {
        Remove-Item -LiteralPath $appData -Recurse -Force -ErrorAction SilentlyContinue
    }
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}

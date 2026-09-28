[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$OtherUser,

    [string]$Configuration = 'Debug'
)

$ErrorActionPreference = 'Stop'

if (-not ('FishMuseWrongSidNative' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class FishMuseWrongSidNative {
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool LogonUserW(
        string userName,
        string domain,
        IntPtr password,
        int logonType,
        int logonProvider,
        out IntPtr token);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr CreateFileW(
        string fileName,
        uint desiredAccess,
        uint shareMode,
        IntPtr securityAttributes,
        uint creationDisposition,
        uint flagsAndAttributes,
        IntPtr templateFile);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool CloseHandle(IntPtr handle);
}
'@
}

$componentRoot = Split-Path -Parent $PSScriptRoot
$probe = Join-Path $componentRoot "build\windows-msvc\$Configuration\foo_fishmuse_tests.exe"
if (-not (Test-Path -LiteralPath $probe)) {
    throw "Build the foo_fishmuse_tests target first: $probe"
}

$runId = [Guid]::NewGuid().ToString('N')
$work = Join-Path ([System.IO.Path]::GetTempPath()) "fishmuse-wrong-sid-$runId"
$ready = Join-Path $work 'ready.txt'
$stop = Join-Path $work 'stop.txt'
New-Item -ItemType Directory -Path $work | Out-Null

$server = $null
try {
    $server = Start-Process -FilePath $probe -ArgumentList @(
        '--auth-probe-server',
        $ready,
        $stop
    ) -PassThru -WindowStyle Hidden

    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while (-not (Test-Path -LiteralPath $ready)) {
        if ($server.HasExited) {
            throw "Auth probe server exited early with code $($server.ExitCode)."
        }
        if ([DateTime]::UtcNow -ge $deadline) {
            throw 'Timed out waiting for auth probe server.'
        }
        Start-Sleep -Milliseconds 25
    }

    $pipeName = (Get-Content -LiteralPath $ready -Raw).Trim()
    $positive = Start-Process -FilePath $probe -ArgumentList @(
        '--auth-probe-client',
        $pipeName
    ) -PassThru -Wait -WindowStyle Hidden
    if ($positive.ExitCode -ne 0) {
        throw "The current-user positive control could not connect (exit $($positive.ExitCode))."
    }

    $credential = Get-Credential -UserName $OtherUser -Message (
        'Enter credentials for an existing secondary Windows user. The password is not stored.'
    )
    if ($null -eq $credential) {
        throw 'The Windows credential prompt was cancelled.'
    }

    $separator = $credential.UserName.IndexOf('\')
    if ($separator -gt 0) {
        $domain = $credential.UserName.Substring(0, $separator)
        $userName = $credential.UserName.Substring($separator + 1)
    }
    else {
        $domain = $env:COMPUTERNAME
        $userName = $credential.UserName
    }

    $password = [Runtime.InteropServices.Marshal]::SecureStringToGlobalAllocUnicode(
        $credential.Password)
    $token = [IntPtr]::Zero
    try {
        if (-not [FishMuseWrongSidNative]::LogonUserW(
                $userName, $domain, $password, 2, 0, [ref]$token)) {
            throw [ComponentModel.Win32Exception]::new(
                [Runtime.InteropServices.Marshal]::GetLastWin32Error())
        }
    }
    finally {
        [Runtime.InteropServices.Marshal]::ZeroFreeGlobalAllocUnicode($password)
    }

    try {
        $identity = $null
        $context = $null
        try {
            $identity = [Security.Principal.WindowsIdentity]::new($token)
            if ($identity.User -eq [Security.Principal.WindowsIdentity]::GetCurrent().User) {
                throw 'The supplied credentials resolved to the current Windows user.'
            }
            $context = $identity.Impersonate()
            $genericReadWrite = [Convert]::ToUInt32('C0000000', 16)
            $pipeHandle = [FishMuseWrongSidNative]::CreateFileW(
                $pipeName, $genericReadWrite, 0, [IntPtr]::Zero, 3, 0, [IntPtr]::Zero)
            $pipeError = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
            if ($pipeHandle -ne [IntPtr]::new(-1)) {
                [void][FishMuseWrongSidNative]::CloseHandle($pipeHandle)
                throw 'The secondary Windows user unexpectedly connected to the pipe.'
            }
            if ($pipeError -ne 5) {
                throw "Expected ERROR_ACCESS_DENIED (5); CreateFileW returned $pipeError."
            }
        }
        finally {
            if ($null -ne $context) {
                $context.Undo()
                $context.Dispose()
            }
            if ($null -ne $identity) {
                $identity.Dispose()
            }
        }
    }
    finally {
        if ($token -ne [IntPtr]::Zero) {
            [void][FishMuseWrongSidNative]::CloseHandle($token)
        }
    }

    Write-Host 'PASS: another Windows user was denied by the real foo_fishmuse pipe DACL.'
}
finally {
    New-Item -ItemType File -Path $stop -Force | Out-Null
    if ($null -ne $server -and -not $server.HasExited) {
        $server.WaitForExit(5000) | Out-Null
    }
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}

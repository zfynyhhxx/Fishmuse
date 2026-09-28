# foo_fishmuse

`foo_fishmuse` is the Windows-only foobar2000 playback bridge for FishMuse.
It is a Named Pipe server; FishMuse is the only supported client. The pipe
uses the closed protocol in `protocol/foobar-v1` and never reads or modifies a
foobar2000 database.

## Prerequisites

- Visual Studio 2022 with the Desktop development with C++ workload.
- Windows SDK 10.0.22621 or newer.
- The repository-local vcpkg checkout under `.deps/vcpkg`.
- For the component build only: official foobar2000 SDK dated 2026-09-17,
  unpacked under `.deps/foobar2000-sdk`. The SDK and foobar binaries are local
  dependencies and must not be committed.

The unit target deliberately does not include the foobar SDK. It verifies the
wire protocol, framing, pipe ACL and client-token checks, handshake deadline,
ordered events, OperationId replay, bounded write queue, and SDK-independent
playback facade.

## Configure and run the SDK-independent tests

From the repository root in PowerShell:

```powershell
& 'C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe' `
  --preset windows-msvc -S native/foo-fishmuse
& 'C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe' `
  --build native/foo-fishmuse/build/windows-msvc --config Debug --target foo_fishmuse_tests
& 'C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\ctest.exe' `
  --test-dir native/foo-fishmuse/build/windows-msvc -C Debug --output-on-failure
```

`TrackFileAccess=false` is part of the checked-in preset and generated test
project. On the recorded development host, Visual Studio's file-tracker
injection left `cl.exe` suspended; disabling incremental file tracking avoids
that host defect without changing compiler warnings or test behavior.

## Cross-user pipe rejection

The normal tests inspect the real security descriptor and authenticate a
same-user client through `ImpersonateNamedPipeClient`. Verifying a different
SID requires an existing secondary Windows account and is intentionally an
explicit integration test:

```powershell
powershell -ExecutionPolicy Bypass `
  -File native/foo-fishmuse/tests/wrong_sid_integration.ps1 `
  -OtherUser '.\ExistingSecondaryUser'
```

For a Microsoft-backed Windows user, pass the qualified account principal,
for example `MicrosoftAccount\user@example.com`, rather than its display name.
The script prompts through `Get-Credential`, never writes the password to disk
or a command, and gives the prompt five minutes to complete. It first proves
that the current user can connect to the real test pipe, then obtains the
secondary user's interactive token with `LogonUserW`, verifies that the token
has a different SID, and calls `CreateFileW` under impersonation. It passes
only for the exact `ERROR_ACCESS_DENIED` result. The temporary password buffer
is zeroed and freed, and the script does not create or modify accounts.

## Component build and smoke test

With the pinned SDK at `.deps/foobar2000-sdk`, build the checked-in solution:

```powershell
msbuild native/foo-fishmuse/foo_fishmuse.sln /m /p:Configuration=Debug /p:Platform=x64
```

The Debug component is written to
`native/foo-fishmuse/build/sdk/x64/Debug/foo_fishmuse.dll`; replace `Debug`
with `Release` for the optimized artifact. FishMuse uses a dedicated
`FishMuse Playback Bridge` playlist to ask the supported foobar SDK to play a
trusted local handle. It does not make the playlist active and does not read or
modify foobar2000 database files.

Do not install into a normal foobar profile. Use a disposable development
profile for foobar2000 v2.24.3 x64, copy only the generated component there,
and create the `portable_mode_enabled` marker before launch. The component path
inside that disposable host is:

```text
profile/user-components-x64/foo_fishmuse/foo_fishmuse.dll
```

After starting the disposable host, run the live protocol harness from the
repository root. The fixture directory must also be disposable and ignored:

```powershell
powershell -ExecutionPolicy Bypass `
  -File native/foo-fishmuse/tests/live_smoke.ps1 `
  -FixtureDirectory native/foo-fishmuse/build/live-host/fixture `
  -ConnectTimeoutMs 10000 `
  -AckTimeoutMs 500
```

The harness generates a local WAV fixture and verifies the handshake, playback
commands, authoritative state, ordered events, and byte-identical OperationId
replay. Every command ACK is bounded by the supplied limit. Explicitly verify:

1. foobar loads the component and FishMuse completes the v1 handshake;
2. play, pause, resume, seek, next, stop, volume, and snapshot work;
3. command ACK arrives within 500 ms on the normal path;
4. closing and reopening foobar reconnects and sends an authoritative snapshot;
5. an ACK-loss retry with the same OperationId does not repeat the side effect;
6. the pipe disappears after foobar exits; and
7. the cross-user rejection script passes.

For the reconnect check, close foobar cleanly, confirm its pipe disappears,
start the same disposable host again, rerun `live_smoke.ps1`, and require the
new handshake and authoritative state checks to pass. Delete the disposable
host after recording the result; never commit the host, SDK, generated DLL,
fixture, profile, or foobar configuration.

Installing the component and running this live smoke test are explicit manual
gates. They are not performed by CTest or CI.

## Security and privacy invariants

- The pipe DACL contains only the current user and SYSTEM. The connected token
  SID is also checked after the first bounded read and before JSON decoding or
  command dispatch; the pipe name is not treated as authorization.
- Frames are four-byte little-endian lengths plus non-empty UTF-8 JSON and are
  limited to 1 MiB. Unknown fields, kinds, commands, values, duplicate JSON
  keys, stale sequences, and messages before the handshake are rejected.
- Playback callback code only enqueues bounded messages. A slow client causes
  incremental events to be dropped and requires a full snapshot resync.
- Local paths exist only in the trusted FishMuse-to-component play request.
  They must not be logged, returned to the frontend, or sent to an AI provider.
- Component shutdown stops new command acceptance, clears queued main-thread
  work, and closes the pipe last.

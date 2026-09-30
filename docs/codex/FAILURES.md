# FishMuse V0.1 Failure Ledger

### V0.1 real-use runtime failures

- First seen: 2026-09-30 from the user's installed debug build.
- Causes: the scanner persisted only media assets and discarded parsed tags; AI persistence appended messages before creating the parent conversation and desktop error mapping labeled every non-AI failure as a tool failure; the Settings playback button only changed notice text; debug builds used the Console PE subsystem; the active foobar profile initially lacked `foo_fishmuse`; and the first real connected snapshot tried to replace a Tokio watch value while the same expression still held its read guard, self-deadlocking the desktop only when foobar was available.
- Fix: route parsed scans through `LocalLibraryImporter`, repair both same-path and moved legacy assets, refresh Library search after terminal scan events, create conversations idempotently, narrow tool-failure classification, add a real Shell/App Paths launch command with immediate reconnect, enforce/verify the Windows GUI subsystem, install the authorized component, and release the playback-state read guard before `send_replace`.
- Current result: the installed component passed a real handshake and read-only state command; the repaired release remained responsive and displayed `Playback: Ready` with no console window. A real repair scan projected all 2,188 assets and production search returned tracks. The complete unit/static gate, Tauri/WebView2 E2E 4/4, component CTest, secret scan, and diff check pass. The standalone portable directory passed inventory, hash, PE-subsystem, independent-launch, connected-status, and clean-shutdown verification.

Use this file only for failures that may recur across turns. One-off red tests that are immediately resolved belong in normal task history.

## Active blockers

None.

## Entry format

### Short failure name

- First seen:
- Current task and command:
- Root-cause evidence:
- Repair attempts (maximum three distinct attempts):
- Current result:
- Smallest action that would unblock progress:

Move resolved entries below and retain the final cause and fix so later continuations do not repeat the same investigation.

## Resolved blockers

### Secondary-user pipe access required an interactive Windows credential prompt

- First seen: 2026-09-29 after the disposable-host live smoke passed.
- Final cause: the real wrong-SID gate requires an interactive logon token for the MicrosoftAccount-backed secondary Windows user. Earlier attempts also exposed an absent `PUBLIC` path, principal ambiguity, inherited inaccessible working directory, host denial of cross-user process creation, asynchronous-connect ambiguity, and a Windows PowerShell 5.1 unsigned-mask incompatibility.
- Final fix: qualify the MicrosoftAccount principal, obtain its real interactive token through `LogonUserW`, verify the token SID differs from the current user, run a current-user positive control, then impersonate the secondary token for synchronous `CreateFileW` and require exact Win32 error 5. The user entered the password only through the Windows credential UI.
- Current result: resolved. The positive control connected to the real auth-probe pipe and the different-user access was denied with `ERROR_ACCESS_DENIED (5)`. No password was logged or persisted, and no foobar or auth-probe process remains.

### Disposable localized foobar host smoke was authorized and passed

- First seen: 2026-09-28 after the real SDK build passed.
- Final cause: the only available v2.24.3 x64 host was an unsigned localized distribution and required explicit user authorization before launch.
- Final fix: the user authorized the discovered local foobar2000 executable. The installed tree was copied read-only into ignored Task 8 scratch storage, marked portable, and received only the generated Release component in its isolated profile.
- Current result: the final non-diagnostic Release DLL passed handshake, playback, pause/resume, seek, volume, byte-identical OperationId replay, authoritative state, skip, stop, ordered events, and lifecycle shutdown; maximum sampled ACK was 111 ms and the pipe disappeared after clean exit. The source installation was unchanged.

### Official foobar2000 SDK was unavailable

- First seen: 2026-09-28 after Task 8 SDK-independent layers reached GREEN.
- Final cause: `.deps/foobar2000-sdk` was absent, so the pinned 2026-09-17 API and real component build could not be verified.
- Final fix: the user supplied the official SDK at `.deps/foobar2000-sdk`. Its version, license/readme, required headers, project structure, and core x64 compilation were verified; FishMuse now builds real Debug and Release x64 DLLs against the SDK.
- Current result: resolved. The remaining Task 8 blockers are the separate live-host and credentialed cross-user gates above.

### Pinned vcpkg dependency download could not complete through the host proxy

- First seen: 2026-09-28 during Task 8 Step 1, before the intended C++ RED build.
- Final cause: the initial shallow clone lacked the requested historical port tree, history fetches stalled through the host proxy, and vcpkg's official CMake 4.4 archive did not finalize through that route.
- Final fix: the user supplied `.deps/vcpkg/downloads/cmake-4.4.0-windows-x86_64.zip`; it was a readable 8,980-entry ZIP of 54,388,920 bytes whose SHA-512 exactly matched vcpkg's pinned `35479675DC414747B6EB176034FB707490A758A7FA24007018BFB9EDDB71E0422AA5E97B1EFDBFE94EACA19923AC2E0DC8A2F807F549CE31FFB0CA23F59E04D0`. vcpkg then installed PowerShell Core and `nlohmann-json` 3.12.0#2, and configure reached the intended missing-header RED.
- Current result: resolved; repeated configure reports all requested packages installed and exits successfully.

### Visual Studio FileTracker suspended the C++ compiler

- First seen: 2026-09-28 during Task 8 compiler detection after dependency setup succeeded.
- Final cause: the generated MSBuild compiler-identification project launched `cl.exe` through Tracker and left the compiler thread suspended; direct MSBuild with `/p:TrackFileAccess=false` completed in 4.9 seconds.
- Final fix: set `TrackFileAccess=false` in the CMake preset environment and `VS_GLOBAL_TrackFileAccess=false` on the SDK-independent test target, then rerun configure with a fresh cache once.
- Current result: resolved; normal preset configure and target builds complete with strict warnings enabled.

### Linked-worktree process traversal denied in the sandbox

- First seen: 2026-09-28 while beginning Task 7.
- Current task and command: repository-relative Git, plan helper, Cargo, and pnpm commands in the selected linked worktree.
- Root-cause evidence: the sandbox process ignored `workdir`, reported `C:\`, and `Set-Location` returned `UnauthorizedAccessException`; absolute file reads and `git -C` reached the tree, but Git also rejected the sandbox identity as dubious ownership.
- Repair attempts (maximum three distinct attempts): (1) absolute paths plus command-local `safe.directory` enabled read-only Git inspection but not working-directory-dependent commands; (2) approved non-sandbox execution entered the exact selected worktree as its owning user and ran builds/tests successfully.
- Current result: resolved for this run by using approved non-sandbox execution only for local commands that require the selected worktree as their process directory; edits remain confined to the worktree.
- Smallest action that would unblock progress: if approval capacity is unavailable again, allow the same scoped local build/test/Git execution after reviewing the exact command; no repository or product change is required.

# FishMuse V0.1 Verification Log

Append commands only when they were actually run. Record failures as failures; do not infer a pass from an earlier task or commit.

## Current baseline

- 2026-09-28: branch and history inspected; Tasks 1–6 have plan-aligned local commits through `ffbd60d`.
- 2026-09-28: product test suites were not rerun as part of the Codex autonomy configuration change.

### 2026-09-28 — Codex autonomy setup

- Commit or working state: `AGENTS.md`, `.codex/config.toml`, and `docs/codex/*` changed; no product source changed.
- Command: parse `.codex/config.toml` with Python `tomllib` and assert all seven configured values.
- Result: PASS (`toml_parse=PASS exact_config=true`).
- Command: verify required files, allowed Git change scope, `git diff --check`, Markdown trailing whitespace, durable-file references, and the `/goal` command.
- Result: PASS (9 required files, 3 expected Git status entries, 7 Markdown files, all cross-references present).
- Environment note: the sandbox denied direct execution of `codex.exe`, so validation proves TOML syntax and exact supported settings but did not launch a nested Codex process.

### 2026-09-28 — Task 7 baseline and foobar IPC v1

- Commit or working state: clean `877d1ba` baseline before Task 7.
- Command: `pnpm install --frozen-lockfile`; frontend test, typecheck, and lint commands from `AGENTS.md`; `cargo check --workspace`; `cargo test --workspace`; workspace clippy; `cargo fmt --all -- --check`.
- Result: PASS (locked dependencies already current; frontend 2/2; Rust workspace 88/88; typecheck, lint, check, clippy, and fmt exited 0).
- RED evidence: `cargo test -p fishmuse-playback --test protocol_vectors` and `--test framing` both failed because `fishmuse_playback::foobar` did not exist.
- Repair evidence: the first protocol GREEN attempt exposed camelCase fields on enum struct variants; the second exposed `f32` JSON round-trip drift for `0.8`. `rename_all_fields = "camelCase"` and wire-level `f64` corrected the respective root causes without relaxing validation.
- Command: parse every `protocol/foobar-v1/**/*.json` document with PowerShell `ConvertFrom-Json`.
- Result: PASS (`json_parse=PASS`).
- Command: `cargo test -p fishmuse-playback --test protocol_vectors`; `cargo test -p fishmuse-playback --test framing`; `cargo test -p fishmuse-playback`; `cargo clippy -p fishmuse-playback --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `git diff --check`.
- Result: PASS (protocol vectors 5/5, framing 4/4, full playback crate 27/27, zero clippy/fmt/diff errors).
- Fresh pre-commit gate: `cargo test -p fishmuse-playback protocol` ran 5/5 protocol tests; `cargo test --workspace` ran 97/97; playback all-target clippy, workspace fmt check, and `git diff --check` exited 0.

### 2026-09-28 — Task 7 nested duplicate-field hardening

- Commit or working state: follow-up review on `4941c01` before Task 8 consumed the Rust contract.
- RED evidence: a `command.request` containing duplicate nested `operationId` keys was accepted with the later value because the payload had first been materialized as `serde_json::Value`.
- Repair: preserve payload bytes with `serde_json::value::RawValue`, then deserialize directly into the closed typed payload so duplicate known fields at every typed level are rejected.
- Command: focused nested-duplicate test; `cargo test -p fishmuse-playback`; `cargo test --workspace`; playback all-target clippy; workspace fmt and diff checks.
- Result: PASS (focused regression 1/1, playback 27/27, workspace 97/97, zero clippy/fmt/diff errors).

### 2026-09-28 — Task 8 C++ dependency preflight

- Commit or working state: clean `c124829` before adding uncommitted Task 8 test scaffolding.
- Command: pinned vcpkg bootstrap at release `2026.07.29`; `cmake --preset windows-msvc -S native/foo-fishmuse` with manifest dependency `nlohmann-json`.
- Result: FAIL before compilation. The first manifest configuration could not resolve the port tree from a shallow clone. Full-history and blobless-history repairs both stalled through the proxy. Using the pinned checkout's current port set bypassed history resolution, but vcpkg's required CMake 4.4 archive did not finalize. The canceled transfer left a 25,888,028-byte `.part`; `ZipFile.OpenRead` reports `End of Central Directory record could not be found`, and its SHA-512 (`2b7e880e...10371a1`) differs from vcpkg's pinned value (`35479675...59e04d0`).
- Evidence or follow-up: no Task 8 RED/GREEN claim and no Task 8 commit. Resume only after the official CMake archive is available to vcpkg; then confirm the intended missing-implementation RED before writing production C++.

### 2026-09-28 — Task 8 SDK-independent implementation checkpoint

- Commit or working state: uncommitted Task 8 work on `c124829`; no SDK adapter/project files and no Task 8 completion claim.
- Dependency-unblock evidence: `.deps/vcpkg/downloads/cmake-4.4.0-windows-x86_64.zip` was readable with 8,980 entries, size 54,388,920 bytes, and SHA-512 `35479675DC414747B6EB176034FB707490A758A7FA24007018BFB9EDDB71E0422AA5E97B1EFDBFE94EACA19923AC2E0DC8A2F807F549CE31FFB0CA23F59E04D0`, exactly matching vcpkg's pin. vcpkg then installed its PowerShell Core tool and `nlohmann-json:x64-windows@3.12.0#2`.
- RED evidence: the first Task 8 build reached compilation and failed because `framing.hpp`, `pipe_security.hpp`, and `pipe_server.hpp` did not exist. Later focused RED builds failed first on the missing `named_pipe_server`/handshake-timeout surface, then on missing `playback_facade.hpp`, and then on missing `component_service.hpp`.
- Runtime repair evidence: a real pipe authorization test initially failed with Win32 1368 because `ImpersonateNamedPipeClient` ran before a server read. A direct experiment reproduced the platform requirement; moving token authorization after the first bounded read but before JSON decoding/handler dispatch made the test pass while preserving the DACL boundary.
- Build-host repair evidence: Visual Studio FileTracker left compiler-identification `cl.exe` suspended. The same project built in 4.9 seconds with `/p:TrackFileAccess=false`; the checked-in preset and SDK-independent test target now carry that setting.
- Command: Visual Studio-bundled CMake configure with preset `windows-msvc`; build `native/foo-fishmuse/build/windows-msvc`, Debug target `foo_fishmuse_tests`; matching CTest with `--output-on-failure`.
- Result: PASS. Configure reused installed vcpkg packages, strict MSVC `/W4 /WX /permissive- /utf-8` compilation succeeded, and CTest passed 1/1 in 1.25 seconds.
- Command: parse `native/foo-fishmuse/tests/wrong_sid_integration.ps1` with the PowerShell language parser; `git diff --check`.
- Result: PASS (`powershell_parse=PASS`, `git_diff_check=PASS`). The integration script was not executed because no existing secondary-user credential was supplied.
- Evidence or follow-up: `.deps/foobar2000-sdk` was explicitly checked and is missing. Therefore `component.cpp`, `foo_fishmuse.vcxproj`, `foo_fishmuse.sln`, `msbuild`, the cross-user live run, and the real foobar2000 v2.24.3 x64 smoke test remain pending; no plugin-build or live-host pass is claimed.

### 2026-09-28 — Task 8 cross-language pipe discovery hardening

- Commit or working state: uncommitted Task 8 work on `f9927ad`; official foobar SDK still absent.
- Audit finding: the protocol named `<UserSidHash>` but did not define its byte encoding, while the first C++ implementation hashed the in-memory UTF-16 representation. A normal Rust implementation would hash UTF-8 text and derive a different pipe name.
- RED evidence: add a fixed SID fixture requiring lowercase SHA-256 of canonical SID UTF-8 bytes; strict build failed because `pipe_name_for_sid` did not exist (`C2039`, `C3861`).
- Repair: specify the byte-level convention in `protocol/foobar-v1/README.md`, expose one C++ pipe-name derivation function, convert canonical SID text to strict UTF-8, and use that function for the live current-user pipe name.
- Command: build Debug target `foo_fishmuse_tests` with the Visual Studio-bundled CMake; run matching CTest with `--output-on-failure`.
- Result: PASS. Strict compilation succeeded and CTest passed 1/1 in 1.28 seconds; the fixed SID `S-1-5-21-1-2-3-1001` maps to hash `c169ebe52e9c0ba43200ce3a6af1b392219cdaf6006bba3e879ccb699a245fae`.

### 2026-09-28 — Task 8 third external-blocker audit

- Commit or working state: uncommitted Task 8 checkpoint retained on `f9927ad` after all safe SDK-independent work and focused hardening.
- Command: test `.deps/foobar2000-sdk` and the required SDK-dependent files `native/foo-fishmuse/src/component.cpp`, `native/foo-fishmuse/foo_fishmuse.vcxproj`, and `native/foo-fishmuse/foo_fishmuse.sln`; inspect Git status.
- Result: BLOCKED. The SDK directory and all three SDK-dependent files are absent for the third consecutive resumed goal turn. Starting Task 9 would violate the sequential plan and task-level commit rule because Task 8 cannot be built, smoke-tested, or committed.
- Evidence or follow-up: formally block the active goal. Resume at Task 8 Step 5 after the official foobar2000 SDK dated 2026-09-17 is unpacked at `.deps/foobar2000-sdk`; do not discard the uncommitted checkpoint.

### 2026-09-28 — Task 8 official SDK adapter and component build

- Commit or working state: uncommitted Task 8 checkpoint on `f9927ad`; the prior SDK blocker is resolved.
- SDK evidence: `.deps/foobar2000-sdk/sdk-readme.html` identifies version `2026-09-17`; 692 files totaling 4,760,047 bytes were present, required SDK/PFC/component-client headers and projects were found, and the official core PFC, component-client, and SDK projects compiled for Debug x64. The optional SDK sample's helpers/libPPUI path requires external WTL (`atlapp.h`), which FishMuse neither references nor links.
- Real SDK RED evidence: after adding the checked-in x64 solution/project, `MSBuild.exe native\foo-fishmuse\foo_fishmuse.sln /m /p:Configuration=Debug /p:Platform=x64 /p:TrackFileAccess=false /v:minimal` compiled all three official SDK dependencies and failed only because `src\component.cpp` did not exist (`C1083`).
- TDD evidence: Rust and C++ tests first rejected active playback with a null FishMuse TrackId; both now accept the schema-authorized unknown identity. Focused RED/GREEN cycles also covered normalized foobar dB conversion, cancellable main-thread dispatch, playback-event serialization, facade shutdown without lock inversion, and cancellable deferral of SDK callback events.
- Command: real solution builds with MSBuild for `Debug|x64` and `Release|x64`, strict component warnings as errors, using official SDK project references.
- Result: PASS. Debug DLL: 3,360,256 bytes, SHA-256 `294BB82AD85768DA603A3976623EBF76665F90D9A15272E86B9D89C217160B00`; Release DLL: 291,840 bytes, SHA-256 `AC5DBED2733C4C1865FCFBE441B1BB54244807D6D510AE376B8716446AD8611F`.
- Command: `ctest --test-dir native/foo-fishmuse/build/windows-msvc -C Debug --output-on-failure`; pinned Cargo `test -p fishmuse-playback --test protocol_vectors`.
- Result: PASS (CTest 1/1; Rust protocol vectors 6/6).
- Evidence or follow-up: Task 8 still requires the documented disposable-profile foobar2000 v2.24.3 x64 smoke test and cross-user credentialed integration run. No install, GUI launch, credential prompt, commit, push, PR, or deployment was performed.
- Read-only live-gate discovery: the authorized local executable exists and its uninstall registration reports localized/modified `v2.24.3 x64`; three enabled non-current local accounts exist. No profile was modified and no account names or credentials were recorded.
- Disposable-profile preflight: the executable reports file version `2.24.3.0`, product company `Piotr Pawlowski`, is unsigned, and contains the ASCII marker `portable_mode_enabled`. The installation currently has no portable marker or local profile, so the safe candidate is a copied installation under Task 8 scratch storage with a newly created marker and isolated profile; the installed tree remains read-only.

### 2026-09-29 — Task 8 real host smoke and wrong-SID checkpoint

- Commit or working state: Task 8 remains uncommitted on `f9927ad`.
- RED evidence: the first live run rejected a relative fixture path. After resolving the fixture directory to an absolute path, the next run reached real playback but the harness sent `seek` before the local WAV was seek-ready and received the expected sanitized `playback_failed`. The harness now polls authoritative `get_state` until the track is playing with a known duration; no production failure policy was relaxed.
- Command: build the official SDK solution for `Debug|x64` and `Release|x64`; copy the final Release DLL into an ignored portable copy of authorized foobar2000 v2.24.3 x64; run `tests/live_smoke.ps1` with a generated local WAV and 500 ms ACK limit.
- Result: PASS. Final Release SHA-256 `01B8D83EA28881FCA0A1C1CE759BB9B1AFECEFDC0F83046713C8FC58A9408324`; handshake, play, pause/resume, seek, volume, byte-identical retry replay, state identity, skip, stop, events, and snapshots passed; maximum sampled ACK 111 ms. Clean host exit removed the per-user pipe. Temporary diagnostic code/file was removed before this build.
- Reconnect evidence: after that clean exit, the same disposable portable host was started again with the identical final Release hash. A fresh handshake, authoritative state, full command/replay/event smoke, and clean shutdown passed again with a maximum sampled ACK of 91 ms; the pipe disappeared after the second exit.
- wrong-SID RED/repair evidence: missing `PUBLIC`, ambiguous display-name principal, inherited inaccessible working directory, host denial of cross-user process creation, and an impersonation-unfriendly asynchronous pipe connect were each reproduced rather than bypassed. The script now uses a current-user positive control plus a real different-user `LogonUserW` token and synchronous `CreateFileW`, accepting only Win32 error 5.
- wrong-SID current result: BLOCKED on human prompt completion. The final five-minute credential window expired without submission, so no DACL pass is claimed. The stale run was terminated; no password was logged or stored.
- Pre-gate review RED: Windows PowerShell 5.1 parsed `0xC0000000` as negative `Int32`, so binding the `CreateFileW` access mask to `UInt32` failed before the native call. The script now uses `Convert.ToUInt32("C0000000", 16)`, captures `GetLastWin32Error` immediately, and disposes identity/impersonation objects on every partial-failure path.
- Focused recheck: CTest passed 1/1 in 0.27 seconds; pinned Rust 1.98.1 `protocol_vectors` passed 6/6; both live scripts parsed under Windows PowerShell 5.1; the access mask equaled 3221225472; `git diff --check` passed.
- Public-boundary audit: all 40 modified/untracked candidates were source, project, protocol, or documentation files; SDK, host, DLL, WAV fixture, profile, and build trees were ignored; no candidate binary, API key, private key, personal email, or machine-user absolute path remained after documentation redaction.
- Final wrong-SID command: `powershell.exe -NoProfile -ExecutionPolicy Bypass -File native/foo-fishmuse/tests/wrong_sid_integration.ps1 -OtherUser 'MicrosoftAccount\<address>'` with the password entered only in the Windows credential UI.
- Final wrong-SID result: PASS. The current-user positive control connected to the real auth-probe pipe, the authenticated secondary token's SID differed from the current SID, and synchronous `CreateFileW` under impersonation returned exact `ERROR_ACCESS_DENIED (5)`. No password was logged or persisted; the probe exited cleanly.
- Fresh completion build: official SDK solution rebuilt successfully for `Debug|x64` and `Release|x64`; the freshly linked Release DLL SHA-256 was `357B3B46F1F319A682AF3B064E6CC9E51A1C4A4F1158EAA9B5C8993BF88E17F7`. The SDK-independent preset configured against already-installed pinned vcpkg dependencies, its strict Debug test target built, and CTest passed 1/1 in 1.43 seconds.
- Fresh real-host result: the freshly linked Release DLL was copied only into the ignored disposable v2.24.3 x64 host and passed handshake, commands, byte-identical OperationId replay, events, and authoritative snapshots with a maximum sampled ACK of 71 ms. The host exited cleanly and no foobar process remained.
- Fresh cross-language/static result: pinned Rust 1.98.1 protocol vectors passed 6/6; both PowerShell live scripts parsed under Windows PowerShell 5.1; the `CreateFileW` access mask equaled 3221225472; `git diff --check` passed apart from informational line-ending warnings.
- Fresh public-boundary result: all 40 modified/untracked candidates were checked; no candidate binary, API key/private key pattern, supplied account email, machine-user absolute path, or installed-host path was present. SDK, host, DLL, WAV, profile, and build artifacts remain ignored.
- Task 8 commit/publication: commit `85fc001` contains the 40-file Task 8 snapshot. GitHub API readback reported `zfynyhhxx/Fishmuse` as `PUBLIC`; remote branch `feature/fishmuse-v0.1-muse-loop` resolved to the exact local SHA `85fc001deb736a93355920ed2d0a01bd2b1a1e4b`.

### 2026-09-29 – Task 9 Rust foobar client RED

- Commit or working state: clean `85fc001` Task 8 baseline, then uncommitted Task 9 tests only.
- RED command: pinned Rust 1.98.1 `cargo test -p fishmuse-playback --test foobar_client`.
- RED result: expected compile failure; `FoobarClient` and `FoobarConfig` do not exist. The fake Tokio Named Pipe fixture compiled far enough to reach the intended missing production API.
- RED command: pinned Rust 1.98.1 `cargo test -p fishmuse-playback --test foobar_reconnect`.
- RED result: expected compile failure; `ConnectionState`, `FoobarBackend`, `FoobarConfig`, `ReconnectPolicy`, and `StateReconciler` do not exist. Production implementation has not started.
- First client GREEN: `cargo test -p fishmuse-playback --test foobar_client` passed 3/3. The Tokio Named Pipe fixture proved handshake/capability negotiation, request correlation, initial snapshot, asynchronous event delivery, bounded ACK timeout, and typed remote-error mapping without retaining the remote message's private path.
- Reconnect repair evidence: the first run passed 2/4 and exposed two invalid raw-revision assertions plus a real EOF race. When ACK + event were queued before pipe close, the supervisor could select the disconnect watch before consuming the already-enqueued event. Domain assertions now require monotonic application revisions across session reset, and EOF handling drains only events already queued by that connection before session teardown.
- Reconnect GREEN: `cargo test -p fishmuse-playback --test foobar_reconnect -- --nocapture` passed 4/4. It covers unavailable startup without application failure, immediate manual reconnect despite a five-second scheduled backoff, capped exponential jitter, one retry with the identical OperationId after lost ACK/disconnect, handshake-before-snapshot ordering, post-snapshot incremental events, stale old-session rejection, and new-session sequence reset.
- Task-level command: pinned Rust 1.98.1 `cargo test -p fishmuse-playback`; `cargo clippy -p fishmuse-playback --all-targets -- -D warnings`; `cargo fmt --all -- --check`.
- Pre-commit review RED: an event sent immediately after the authoritative reconnect snapshot could be published before the first broadcast subscriber existed, and the backend supervisor lacked an awaitable transport shutdown. Focused regressions failed by timing out on the buffered event and by the missing `shutdown` API. The client now retains a bounded first receiver from channel creation; the supervisor observes shutdown while connecting, connected, or backing off, closes the active client, and converges its watch state before `shutdown().await` returns. A separate regression confirms the fake server observes pipe closure.
- Final task-level command: pinned Rust 1.98.1 `cargo test -p fishmuse-playback`; `cargo clippy -p fishmuse-playback --all-targets -- -D warnings`; `cargo fmt --all -- --check`.
- Final task-level result: PASS (36/36 playback tests, including client 3/3 and reconnect 5/5; zero clippy warnings/errors; zero fmt differences).

### 2026-09-29 – Task 10 DeepSeek provider, credentials, and budget controls

- Commit or working state: clean public `62adeb5` Task 9 baseline, then uncommitted Task 10 crate and workspace-member changes.
- Streaming RED: `cargo test -p fishmuse-ai --test deepseek_fixtures` failed with unresolved provider event, error, decoder, and HTTP-classification imports. The first GREEN iterations also exposed dual `event`/`type` decoding and fixture blank-line handling; fixes retained strict sequence validation without accepting partial tool arguments.
- Credential RED: `cargo test -p fishmuse-ai --test credential_store` failed with unresolved credential trait, Windows store, and configured-status imports. Fake save/overwrite/delete/missing and Debug redaction then passed 3/3 without touching system state.
- Cost RED: `cargo test -p fishmuse-ai --test cost_policy` failed with unresolved pricing, micro-yuan, budget, and atomic-ledger imports. The implementation centralized all rates in a dated schedule and passed cache-hit, cache-miss, output, peak boundary, unknown usage, effective-date, rounding, live-test hard-stop, and concurrent accumulation checks.
- Reversible Windows integration preflight: `cmdkey.exe /list:FishMuse/DeepSeek` reported `* NONE *`; no existing credential could be overwritten.
- Explicit Windows integration command: `cargo test -p fishmuse-ai --test credential_store windows_credential_manager_round_trip_is_current_user_scoped -- --exact --ignored --nocapture`.
- Explicit Windows integration result: PASS (1/1). A fake current-user Generic Credential was saved, read, overwritten, read, and deleted. Post-test `cmdkey.exe /list:FishMuse/DeepSeek` again reported `* NONE *`.
- Task-level command: `cargo test -p fishmuse-ai`; `cargo clippy -p fishmuse-ai --all-targets -- -D warnings`; `cargo fmt --all -- --check`.
- Pre-final verification repair: the first combined final command reported one rustfmt import-order difference and two strict Clippy `field_reassign_with_default` errors in the endpoint/model rejection test. The test now uses struct update syntax and the final commands were rerun independently so a later successful command cannot mask an earlier nonzero exit. Staged `git diff --check` then identified one redundant blank record at the end of four JSONL fixtures; those empty records were removed before the final staged check.
- Task-level result: PASS (18 regular tests passed, 1 externally mutating test ignored by default but passed explicitly; zero clippy warnings/errors; zero fmt differences).
- Secret-log check: captured complete regular test output in memory and searched for every fake-key sentinel; result `LOG_SECRET_SCAN: PASS`.
- Public-boundary audit: all 23 commit candidates are source, fixture, lockfile, or documentation files; no binary, real API-key/private-key pattern, supplied account email, authorized foobar installation path, machine-user path, or workspace absolute path is present.
- External gate note: no real DeepSeek API key was requested, loaded, or persisted, and no network request or billable spend occurred. Tier 4 remains a Task 15 live gate.

## Entry format

### YYYY-MM-DD — task/checkpoint

- Commit or working state:
- Command:
- Result:
- Evidence or follow-up:

## V0.1 gates

- [ ] Tasks 1–15 completed with reviewable local commits
- [ ] Working tree clean
- [ ] Tier 1/2 pass from a fresh checkout
- [ ] Tier 3 passes with foobar2000 v2.24.3 x64 and the recorded SDK
- [ ] Tier 4 passes with `deepseek-flash` below the hard budget stop
- [ ] Twelve acceptance items have commands, evidence, and results
- [ ] No high-priority security or data-integrity issue remains
- [ ] Chinese setup, plugin, and API documentation is reproducible
- [ ] README and implementation agree on V0.1 exclusions and platform scope

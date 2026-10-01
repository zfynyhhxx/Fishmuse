# FishMuse V0.1 Status

Last updated: 2026-10-01

## Repository checkpoint

- Branch: `feature/fishmuse-v0.1-muse-loop`
- Current usability-repair head: Task 10 completion commit (this checkpoint)
- Completed usability-repair tasks: Tasks 1–10
- Current task: none
- Remaining release gate: none for the authorized local V0.1 usability repair

## Next autonomous action

Hand the completed local branch back to the operator. Do not push, publish, install, or deploy without a new explicit request.

## Current blocker

None.

## Required external gates

- The opted-in real Windows desktop gate passed on 2026-10-01 with the installed foobar2000 component. It started from no FishMuse/foobar process, kept the backend window hidden, exercised every V0.1 control through FishMuse, and validated a settled listen in the isolated SQLite database.
- No paid DeepSeek request was run during this repair. Future live DeepSeek runs remain explicitly operator-authorized and protected by the fixed budget ledger.

## Final verification

- `scripts/test-unit.ps1`: PASS, including the complete Rust workspace, strict checks, GUI subsystem, lint, typecheck, and 31 React tests.
- `scripts/test-e2e.ps1`: PASS, 5 specs / 16 desktop scenarios.
- Production frontend build: PASS.
- Native component: PASS, CMake configure/build and CTest 1/1 via the Visual Studio-bundled CMake.
- `scripts/check-secrets.ps1`: PASS, 258 tracked/untracked source files and zero credential findings.
- `scripts/test-live-fishmuse.ps1`: PASS with every bounded evidence field true, including no visible/foreground backend and persisted listening history.
- Production `tauri build`: PASS, MSI and NSIS bundles produced without `e2e` or `live-e2e` features.
- Visual inspection: PASS for default 1000×700, maximized, minimum 720×520, and the live Now Playing screen.

## Completion state

The usability repair now keeps the desktop shell fixed, gives Library sole ownership of its scroll viewport with automatic bounded paging, preserves usable tracks at the supported 720×520 minimum, and prevents Now Playing horizontal overflow. Playback intents launch the installed backend invisibly through one generic managed lifecycle with a bounded five-second readiness window and race-safe reconnect pulses; an exact-PID startup guard prevents the owned backend from showing or taking foreground focus. The UI exposes complete transport, queue, seek, volume, mute, safe metadata, artwork, external-playback, and generic recovery states without naming the implementation. Listening history is connected to production playback events. Scans recover from folder/start/cancel/search/page/play failures, support cancellation, and project tagless filenames safely.

The real gate additionally exposed a SQLite lock-upgrade race and a process-launch wake race. Operation completion now uses an atomic compare-and-swap update without upgrading a read transaction, backed by a deterministic writer-contention regression; the startup supervisor is pulsed within the original five-second contract. The release bundle also declares the checked-in Windows icon explicitly. Current local evidence includes `target/live/fishmuse-desktop-evidence.json`, `target/live/fishmuse-now-playing.png`, three production-window screenshots under `target/acceptance`, and successful MSI/NSIS outputs under `target/release/bundle`.

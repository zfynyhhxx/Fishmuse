# FishMuse V0.1 Autonomous Runbook

This file is the operating contract for long-running Codex work on `feature/fishmuse-v0.1-muse-loop`.

## Canonical state

- Specification and ordered work: `docs/superpowers/plans/2026-09-27-fishmuse-v0.1-muse-loop.md`
- Current checkpoint: `docs/codex/STATUS.md`
- Material choices: `docs/codex/DECISIONS.md`
- Test evidence: `docs/codex/VERIFICATION.md`
- Repeated failures and attempted repairs: `docs/codex/FAILURES.md`

Read only the current task and directly relevant code first. Expand context only when evidence requires it.

## Autonomous loop

1. Confirm the branch, working-tree state, current task, and last verification evidence.
2. Select the smallest unfinished plan step with a testable result.
3. Add or identify a failing test, then implement the smallest passing change.
4. Run focused tests first. Diagnose the root cause and repair without asking for routine guidance.
5. Run the task-level verification commands from the plan.
6. Update the four durable state files and relevant product documentation.
7. Create the plan-specified local commit only after checks pass and the diff is reviewed.
8. Continue to the next unfinished step or task while the Goal remains active.

## Limit controls

- Keep one primary execution thread. Do not use routine subagents or agent review chains.
- Avoid rereading the full plan, full repository, or old task history on every continuation.
- Reuse the durable state files instead of reconstructing context from conversation history.
- Use focused checks while editing; do not repeatedly run the entire workspace suite for a local change.
- Keep commentary concise and checkpoint-oriented. Do not wait or poll when a local next action exists.
- After three distinct failed repairs for the same root cause, record the evidence and stop as blocked.

## Authority boundaries

Codex may edit this worktree, install already-declared project dependencies when permitted, run local builds and tests, update documentation, and create local commits required by the plan.

Codex must not push, create or merge a PR, deploy, publish, spend money, reset real budgets, install the foobar component into a real profile, use a real DeepSeek credential, or perform destructive Git operations without explicit user authorization.

## Completion rule

V0.1 is complete only when Tasks 1–15 and every item under `V0.1 完成定义` are backed by recorded evidence. A mock or fixture result cannot replace the required Tier 3 foobar or Tier 4 DeepSeek live checks.

## FishMuse desktop live playback gate

This gate is opt-in because it starts and controls the locally installed playback component. It never installs a component, changes a profile, uses a cloud credential, or accepts a pre-existing FishMuse/foobar process.

Prerequisites:

- Windows App Paths or `PATH` resolves `foobar2000.exe`, with the FishMuse component already installed by the operator.
- A local, supported audio file at least 30 seconds long is available as the fixture. The script copies it twice into an isolated temporary music folder; it never edits the source.
- FishMuse and foobar2000 are both closed before the run.
- Workspace dependencies are already installed.

Run from the repository root:

```powershell
pwsh -NoProfile -File scripts/test-live-fishmuse.ps1 `
  -AudioFixture 'C:\path\to\known-track.flac' `
  -Approve
```

The script builds a release-mode test harness using production frontend IPC and playback, but replaces the AI credential store with an unsupported test store so no local DeepSeek key is read. It reserves embedded driver port `4446`, uses the dedicated `com.fishmuse.desktop.live` Tauri identifier for a fresh test-owned SQLite directory, and redirects only WebView2 data to a temporary directory. It deliberately leaves the process `APPDATA` environment unchanged so the hidden playback application can load the operator-installed component. The gate scans the copied fixture and drives play, pause/resume, seek, volume, mute/unmute, next/previous, and stop through visible FishMuse controls.

FishMuse writes the live application identity and the identity returned by its hidden backend launch as PID plus the exact process-creation `FILETIME` read from the original process handle. The script validates each identity's creation time, executable path, and name; every window observation, close, wait, and terminate operation reopens the process, compares that exact creation time while holding the handle, and therefore cannot act on a reused PID. It samples the owned backend's top-level windows and foreground status every 25 ms through the complete playback flow and never uses a global `/exit` or process-name-difference cleanup. After WebDriver exits it verifies that the backend was never visible or foreground, validates a settled listen in the isolated SQLite database, posts `WM_CLOSE` only to top-level windows owned by the recorded backend identity, and removes only the dedicated live-test data and temporary files. Bounded evidence remains at `target/live/fishmuse-desktop-evidence.json` by default, with a native Now Playing screenshot at `target/live/fishmuse-now-playing.png`.

Normal user recovery stays inside FishMuse. Primary Library, MiniPlayer, and Now Playing messages intentionally say only “playback service”; the concrete implementation is limited to Settings advanced diagnostics.

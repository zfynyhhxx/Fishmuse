# FishMuse V0.1 Status

Last updated: 2026-09-29

## Repository checkpoint

- Branch: `feature/fishmuse-v0.1-muse-loop`
- Head before Task 7: `877d1ba chore(codex): add autonomous v0.1 workflow`
- Working tree before Task 7: clean
- Autonomous Codex configuration: prepared and structurally verified on 2026-09-28
- Completed implementation tasks: Task 1–10
- Current task: Task 11 Step 1, tool-registry RED preflight
- Remaining implementation tasks: Task 11–15

## Next autonomous action

Read the exact Task 11 plan and existing domain/storage/playback ports, then add the planned failing tool-schema and confirmation-policy tests before production implementation. Do not use a real DeepSeek key or spend without the explicit live-gate authorization required by the plan.

## Current blocker

- None. Task 8 is committed as `85fc001` and its branch is present at the same SHA in the public `zfynyhhxx/Fishmuse` GitHub repository. SDK version 2026-09-17 was verified locally, Debug/Release x64 builds pass, and the final Release DLL passed the real v2.24.3 x64 smoke test in an isolated portable copy. The authorized source installation was not modified.
- The wrong-SID gate passed with a real different-user MicrosoftAccount token: the current-user positive control connected, the secondary token had a different SID, and synchronous `CreateFileW` was denied with `ERROR_ACCESS_DENIED (5)` by the real pipe DACL.
- The password was entered only into the Windows credential UI and was neither logged nor persisted. No foobar or auth-probe process remains running.

## Required external gates

- Task 8/15: foobar2000 v2.24.3 x64 and the recorded foobar SDK must be available for the real plugin build and Tier 3 acceptance.
- Task 10/15: a DeepSeek API key must be stored through the implemented Windows Credential Manager flow before Tier 4 live acceptance.
- Task 15: the user must explicitly authorize and observe real foobar installation/testing and real DeepSeek API spending. Mock results cannot satisfy these gates.

## Completion state

V0.1 is not complete. Tasks 8 and 9 are complete and public. Task 10 adds the provider-neutral AI stream contract, allowlisted DeepSeek Responses client, strict SSE event sequencing, delayed tool-call release, current-user Windows Credential Manager storage, dated CNY pricing, and live-test-only budget hard stop. All 18 regular AI tests, the explicit reversible Windows credential round trip, strict all-target clippy, workspace fmt, secret-log scan, and public-boundary checks pass. No real DeepSeek credential or paid request was used. Tasks 11–15 remain unfinished.

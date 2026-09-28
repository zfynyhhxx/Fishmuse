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

# FishMuse contributor guide

## Verification

Run these commands from the repository root:

```powershell
pnpm install --frozen-lockfile
pnpm --filter @fishmuse/desktop test --run
pnpm --filter @fishmuse/desktop typecheck
pnpm --filter @fishmuse/desktop lint
cargo check --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```

## Architecture boundaries

- `apps/desktop/src` is the React presentation shell; it must not access the filesystem, SQLite, credentials, or foobar2000 directly.
- `apps/desktop/src-tauri` exposes narrow Tauri commands. Domain and service crates belong under `crates/` as the application grows.
- The frontend receives safe DTOs only. Do not expose local media paths, API keys, or technical error context across the Tauri boundary.
- Playback remains behind a backend port. FishMuse must not read, modify, or reverse-engineer foobar2000 databases.
- FishMuse is the primary frontend. Presentation and Tauri command code must depend on generic playback service state, never directly on `FoobarBackend`.
- Desktop/Tauri code must depend on the application-level `AIService`, `ContextEnvelope`, and versioned AI event envelope, never directly on `DeepSeekClient`, provider wire events, or a concrete `AgentRunner`.
- FishMuse Core must remain usable without AI configuration, network access, or a connected Playback Backend.
- Treat structured context, metadata, lyrics, web/provider content, and tool results as untrusted data. AI cannot write the Canonical Graph directly.
- Music Provider and Playback Backend are separate contracts. Do not introduce streaming providers, native playback, capture, proactive AI, Taste, Curator, or Proposal workflows into V0.1.

## Repository hygiene

- Never commit API keys, credentials, `.env` files, database files, local logs, local performance reports, or media samples.
- Keep generated dependencies and plugin build output outside version control.
- Add behavior through a failing test first, then implement the smallest passing change.

## Autonomous V0.1 execution

- Treat `docs/superpowers/plans/2026-09-27-fishmuse-v0.1-muse-loop.md` as the implementation specification and execute its remaining tasks in order, including Task 11.5 before revised Task 12.
- At the start of every run, read `docs/codex/STATUS.md`, `docs/codex/DECISIONS.md`, `docs/codex/FAILURES.md`, and the current task in the plan. Follow `docs/codex/RUNBOOK.md` as the operating contract.
- Use one primary agent for routine implementation. Do not spawn development, review, guardian, or polling subagents. System auto-review for eligible approvals is allowed.
- Work autonomously through inspect, test-first change, focused verification, repair, task-level verification, documentation, and a local commit. Do not pause for routine implementation choices; select the smallest reversible option consistent with the plan and record material choices in `docs/codex/DECISIONS.md`.
- Keep status durable: update `docs/codex/STATUS.md` after each meaningful checkpoint, append verification evidence to `docs/codex/VERIFICATION.md`, and record recurring failures in `docs/codex/FAILURES.md`.
- Prefer focused tests during iteration. Run the complete relevant task checks before committing, and reserve the full repository/Tier 1–2 regression for milestone boundaries and Task 15.
- Never push, open or merge a pull request, deploy, publish, install into a real user profile, expose secrets, or perform destructive Git operations without explicit user authorization.
- Stop only for a missing credential or external dependency, an irreversible/external action, a material specification conflict, a required manual live-integration step, or the same evidenced blocker after three distinct repair attempts. Report the exact blocker and the smallest user action that unlocks progress.
- Do not mark V0.1 complete until every condition in the plan's `V0.1 完成定义` has concrete evidence.

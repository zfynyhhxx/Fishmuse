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

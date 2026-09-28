# FishMuse V0.1 Failure Ledger

Use this file only for failures that may recur across turns. One-off red tests that are immediately resolved belong in normal task history.

## Active blockers

None recorded.

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

### Linked-worktree process traversal denied in the sandbox

- First seen: 2026-09-28 while beginning Task 7.
- Current task and command: repository-relative Git, plan helper, Cargo, and pnpm commands in `D:\lenovo\workspace\Fishmuse\.worktrees\fishmuse-v0.1`.
- Root-cause evidence: the sandbox process ignored `workdir`, reported `C:\`, and `Set-Location` returned `UnauthorizedAccessException`; absolute file reads and `git -C` reached the tree, but Git also rejected the sandbox identity as dubious ownership.
- Repair attempts (maximum three distinct attempts): (1) absolute paths plus command-local `safe.directory` enabled read-only Git inspection but not working-directory-dependent commands; (2) approved non-sandbox execution entered the exact selected worktree as its owning user and ran builds/tests successfully.
- Current result: resolved for this run by using approved non-sandbox execution only for local commands that require the selected worktree as their process directory; edits remain confined to the worktree.
- Smallest action that would unblock progress: if approval capacity is unavailable again, allow the same scoped local build/test/Git execution after reviewing the exact command; no repository or product change is required.

# FishMuse V0.1 Failure Ledger

Use this file only for failures that may recur across turns. One-off red tests that are immediately resolved belong in normal task history.

## Active blockers

### Pinned vcpkg dependency download cannot complete through the host proxy

- First seen: 2026-09-28 during Task 8 Step 1, before the intended C++ RED build.
- Current task and command: `cmake --preset windows-msvc -S native/foo-fishmuse` using `.deps/vcpkg` release `2026.07.29` and manifest dependency `nlohmann-json`.
- Root-cause evidence: the initial shallow clone lacked port tree `060c829772d52e920fee94cf84755031c61e3b67`; subsequent network transfers through the configured proxy stalled before completing rather than reaching a project compile error.
- Repair attempts (maximum three distinct attempts): (1) full `git fetch --unshallow --tags` ran about nine minutes with no pack growth; (2) `--filter=blob:none` history fetch progressed to 26% then stopped with no pack growth; (3) removing manifest version-range resolution used the pinned current port successfully, but vcpkg's official CMake 4.4 archive did not finalize. The canceled transfer left a 25,888,028-byte `.part` without a ZIP central directory; its SHA-512 (`2b7e880e...10371a1`) does not match vcpkg's pinned hash (`35479675...59e04d0`).
- Current result: Task 8 CMake configuration is incomplete; tests have not reached their expected missing-implementation failure, so TDD prohibits implementing the C++ production code.
- Smallest action that would unblock progress: place the official `cmake-4.4.0-windows-x86_64.zip` in `.deps/vcpkg/downloads/` or allow that GitHub release asset through the host proxy, then rerun CMake configure.

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

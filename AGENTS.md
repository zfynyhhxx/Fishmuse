# FishMuse contributor guide

## Verification

Run these commands from the repository root:

```powershell
pnpm install --frozen-lockfile
pnpm --filter @fishmuse/desktop test --run
pnpm --filter @fishmuse/desktop typecheck
pnpm --filter @fishmuse/desktop lint
cargo check --workspace
cargo fmt --all -- --check
```

## Architecture boundaries

- `apps/desktop/src` is the React presentation shell; it must not access the filesystem, SQLite, credentials, or foobar2000 directly.
- `apps/desktop/src-tauri` exposes narrow Tauri commands. Domain and service crates belong under `crates/` as the application grows.
- The frontend receives safe DTOs only. Do not expose local media paths, API keys, or technical error context across the Tauri boundary.
- Playback remains behind a backend port. FishMuse must not read, modify, or reverse-engineer foobar2000 databases.

## Repository hygiene

- Never commit API keys, credentials, `.env` files, database files, local performance reports, or media samples.
- Keep generated dependencies and plugin build output outside version control.
- Add behavior through a failing test first, then implement the smallest passing change.

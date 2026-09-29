# FishMuse

FishMuse is a Windows-first, local-first personal AI music system. Its long-term center is a Canonical Music Graph and Personal Taste Model, with controlled AI, replaceable Music Providers, and replaceable Playback Backends behind one FishMuse interface.

The V0.1 branch currently contains the local library/storage core, a secure foobar2000 bridge and replaceable playback port, the DeepSeek provider boundary, and a constrained nine-tool Agent loop. Desktop application assembly and the product UI remain in progress; the current architecture contract keeps Core usable without AI credentials, network access, or a connected playback backend.

## Prerequisites

- Windows 11
- Rust 1.98.1 with the `x86_64-pc-windows-msvc` target, `rustfmt`, and `clippy`
- Node.js 24.x and pnpm 11.x
- Visual Studio C++ Build Tools
- Microsoft Edge WebView2 Runtime

## Development

```powershell
pnpm install --frozen-lockfile
pnpm tauri:dev
```

Useful checks:

```powershell
pnpm --filter @fishmuse/desktop test --run
pnpm --filter @fishmuse/desktop typecheck
pnpm --filter @fishmuse/desktop lint
cargo check --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```

The current desktop shell exposes only its minimal healthcheck while application assembly is in progress. foobar2000 and DeepSeek are V0.x implementations behind generic service boundaries, not the product identity. Apple Music, NetEase, MusicBrainz curation, Taste/active AI, context capture, embeddings, native playback, account sync, and installer work are later milestones.

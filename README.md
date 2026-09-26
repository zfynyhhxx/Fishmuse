# FishMuse

FishMuse is a Windows-first desktop music project. This initial workspace provides a local-first React/Tauri shell and a healthcheck command with explicit placeholder states for services that are not connected yet.

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
cargo check --workspace
cargo fmt --all -- --check
```

The current shell does not scan a music library, control playback, or connect to an AI provider. It remains usable while those services report unavailable or not-configured status.

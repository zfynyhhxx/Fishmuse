# FishMuse

FishMuse is a Windows-first, local-first personal AI music system. Its long-term center is a Canonical Music Graph and Personal Taste Model, with controlled AI, replaceable Music Providers, and replaceable Playback Backends behind one FishMuse interface.

The V0.1 branch contains the local library/storage core, a secure foobar2000 bridge and replaceable playback port, the DeepSeek provider boundary, a constrained nine-tool Agent loop, and the provider-neutral Tauri desktop UI. Core and Library stay usable without AI credentials, network access, or a connected playback backend.

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

The complete local gates are `scripts/test-unit.ps1`, `scripts/test-e2e.ps1`, and `scripts/check-secrets.ps1`. Real foobar2000 and DeepSeek checks are deliberately separate, interactive gates; see [Windows setup](docs/development/windows-setup.zh-CN.md), [foobar plugin](docs/development/foobar-plugin.zh-CN.md), and [DeepSeek](docs/development/deepseek.zh-CN.md).

## V0.1 limitations

- Windows 11 x64 only; Linux is not supported yet.
- Playback requires foobar2000 v2.24.3 x64 plus `foo_fishmuse`.
- AI supports only DeepSeek `deepseek-flash`; OpenAI is not implemented.
- CUE files are reported as unsupported diagnostics; parsing is planned for V0.2.
- There is no cloud account or cross-device sync.
- This is not the V1.0 installer and does not promise a permanently frozen database format.

The desktop shell consumes generic application status and stable command/event DTOs. Library scans and AI turns are cancellable jobs; AI events are versioned and sequenced; foobar2000 and DeepSeek are V0.x implementations selected only in the composition root, not the product identity. Apple Music, NetEase, MusicBrainz curation, Taste/active AI, context capture, embeddings, native playback, account sync, and installer work are later milestones.

# Windows 开发环境

FishMuse V0.1 仅支持 Windows 11 x64。准备 Rust 1.98.1（MSVC）、Node.js 24、pnpm 11、Visual Studio C++ Build Tools、Windows SDK、Microsoft Edge WebView2 Runtime。

```powershell
git clone <repository-url>
cd Fishmuse
pnpm install --frozen-lockfile
powershell -ExecutionPolicy Bypass -File scripts/test-unit.ps1
powershell -ExecutionPolicy Bypass -File scripts/test-e2e.ps1
pnpm --filter @fishmuse/desktop tauri:dev
```

桌面 E2E 会构建隔离的 `e2e` 特性二进制，启动真正的 Tauri/WebView2 窗口，并在前端 command/event 边界注入确定性 AI 与播放实现。它不读取真实凭据、不联网，也不要求 foobar2000。

本地数据位于 Tauri 的当前用户应用数据目录。开发构建不提供安装器或自动升级；升级前应自行备份数据库。若 Rust 工具链不是 1.98.1，请先按 `rust-toolchain.toml` 安装对应的 `rustfmt` 与 `clippy` 组件。

完整自动门：

```powershell
pnpm install --frozen-lockfile
powershell -ExecutionPolicy Bypass -File scripts/test-unit.ps1
powershell -ExecutionPolicy Bypass -File scripts/test-e2e.ps1
powershell -ExecutionPolicy Bypass -File scripts/check-secrets.ps1
pnpm --filter @fishmuse/desktop tauri:build --debug --no-bundle
```

# foobar2000 播放桥

V0.1 的播放后端需要 foobar2000 v2.24.3 x64 与 `foo_fishmuse`。FishMuse 仍是浏览、提问与控制的主界面；插件只在同一 Windows 用户下通过受限 Named Pipe 执行播放。

## 构建

1. 将官方 foobar2000 SDK `2026-09-17` 解压到 `.deps/foobar2000-sdk`，不要提交 SDK。
2. 安装 Visual Studio 2022 C++ Build Tools 与 Windows SDK。
3. 执行：

```powershell
msbuild native\foo-fishmuse\foo_fishmuse.sln /m /p:Configuration=Release /p:Platform=x64 /p:TrackFileAccess=false
```

产物为 `native\foo-fishmuse\build\windows-msvc\Release\foo_fishmuse.dll`。把 DLL 放到 foobar2000 当前用户 profile 的 `user-components-x64\foo_fishmuse\` 后重启 foobar2000。SDK、DLL、profile、媒体夹具与便携宿主都是本地工件，不得提交。

## 真实验收

真实门会控制并重启传入的 foobar2000，运行前必须关闭所有 foobar2000 实例，并准备一个现有的次要 Windows 用户；密码仅输入 Windows 凭据对话框。

```powershell
powershell -ExecutionPolicy Bypass -File scripts/test-live-foobar.ps1 `
  -FoobarPath 'C:\path\to\foobar2000.exe' `
  -OtherUser 'MACHINE\secondary-user'
```

脚本要求键入 `RUN`，验证握手、play/pause/resume/seek/next、500 ms ACK、重启后的新握手与 snapshot、相同 OperationId 的字节一致重放，以及另一 SID 收到 `ERROR_ACCESS_DENIED (5)`。它不会安装插件，也不会修改传入安装；请只对已准备好的隔离 profile 运行。

foobar 未启动或插件不可用时，FishMuse 的曲库、扫描、搜索、设置与 AI 界面仍可使用，播放状态显示为断开。

# DeepSeek 配置与真实测试

V0.1 只支持 DeepSeek Responses API 的 `deepseek-flash`。设置页只把 API key 写入当前用户 Windows Credential Manager 的 Generic Credential `FishMuse/DeepSeek`；key 不写入 SQLite、不回显到前端、不进入日志。

在 FishMuse 的 Settings 中输入 key 并点击保存。删除按钮会删除同一 Credential。Core、曲库和本地扫描在未配置 key 或离线时仍可用。

## 受预算保护的 live gate

```powershell
powershell -ExecutionPolicy Bypass -File scripts/test-live-deepseek.ps1
```

脚本要求显式键入 `RUN`，从 Credential Manager 读取 key，向官方 `https://api.deepseek.com/responses` 发送两次最小请求，并验证文本流、`search_library` 工具调用与 usage 记账。任何输出都不得包含 key。

账本固定保存在 `%LOCALAPPDATA%\FishMuse\live-tests\deepseek-budget.json`：累计达到 ¥10 后每次继续都要键入 `CONTINUE`；达到 ¥20 后非零退出。普通环境变量不能改变账本路径或阈值。需要重置时：

```powershell
powershell -ExecutionPolicy Bypass -File scripts/test-live-deepseek.ps1 -ResetBudget
```

必须键入 `RESET`；旧账本先复制到同目录的 `audit` 子目录。CI、应用启动和普通测试都不会发起 DeepSeek 请求或重置账本。

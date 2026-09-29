# FishMuse V0.1 验收清单

状态日期：2026-09-30。自动门应在最终提交前从当前工作树重新运行；真实服务不能用模拟结果替代。

| # | 验收项 | 命令/证据 | 结果 |
| --- | --- | --- | --- |
| 1 | 首次启动不需要云账号 | `onboarding.e2e.ts`；未配置 AI 的应用状态测试 | PASS |
| 2 | 支持格式可扫描 | `cargo test -p fishmuse-library --test extensions --test scanner --test tag_reader` | PASS |
| 3 | 损坏文件不终止扫描 | `scanner`/`tag_reader` 集成测试的逐文件诊断与继续扫描 | PASS |
| 4 | 无变化重扫不重解析 | `unchanged_rescan_does_not_parse_tags_again` | PASS |
| 5 | 100k 热搜索 P95 < 200 ms | `cargo bench -p fishmuse-storage --bench search_100k`，2026-09-30：100,000 tracks / 30 queries / P95 168.540 ms | PASS |
| 6 | foobar 未启动时应用可用 | `cargo test -p fishmuse-playback --test foobar_reconnect`；桌面 E2E 无 foobar 通过 | PASS |
| 7 | 播放命令 500 ms ACK | 2026-09-29 真实 v2.24.3 x64 smoke，最大采样 ACK 71 ms | PASS（Tier 3） |
| 8 | 断线重连状态一致 | 两次真实宿主启动 + `foobar_reconnect` session/revision 测试 | PASS（Tier 3） |
| 9 | DeepSeek 对话流式返回 | `scripts/test-live-deepseek.ps1` / `deepseek_text_stream_and_usage_live` + `deepseek_search_tool_and_usage_live`；2026-09-30 真实 `deepseek-flash` 运行 | PASS（Tier 4） |
| 10 | 九个白名单工具和六次上限 | `tool_security::registry_is_an_exact_nine_tool_allowlist`；`agent_loop::refuses_the_seventh_tool_call_and_finishes_safely` | PASS |
| 11 | ¥10/¥20 测试预算生效 | `cost_policy` + live 脚本固定账本、确认、硬停止与审计重置；成功请求账本 ¥0.000527 | PASS（Tier 4） |
| 12 | Key、路径、音频和完整数据库不离开本机 | credential、redaction、command contract、同 SID pipe、secret scan | PASS |

## Architecture boundary

| 边界 | 证据 | 结果 |
| --- | --- | --- |
| AI 未配置时 Core/Library 可用 | Unsupported/未配置服务测试 + 无凭据桌面 E2E | PASS |
| Playback 未连接时非播放功能可用 | disconnected backend + 四条桌面 E2E | PASS |
| Fake AI/Playback 不改变 UI command/event contract | E2E 仅替换 e2e command boundary，事件仍使用版本化 Tauri contract | PASS |
| AI context 不含 path、key、`technical_context` | `application_contract`, `redaction`, `command_contracts` | PASS |
| presentation 不直接依赖具体 provider/backend | 具体 `DeepSeekClient`/`FoobarBackend` 只在 Tauri composition root 组装 | PASS |
| FishMuse 是主界面 | Library、Ask FishMuse、Now Playing 与全局 MiniPlayer 桌面 E2E | PASS |

Tier 3 的完整命令、SDK/DLL hash、错误修复与 wrong-SID 证据保存在 `docs/codex/VERIFICATION.md`。Tier 4 的真实脚本、协议回归、账本金额与首次失败请求披露也保存在同一验证记录中。

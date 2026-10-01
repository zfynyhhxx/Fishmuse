# FishMuse V0.1 验收清单

状态日期：2026-10-01。最终工作树已重跑：unit/Clippy/fmt/lint/typecheck 与 31 个 React 测试通过；桌面 E2E 5 specs / 16 scenarios 通过；生产前端 build 通过；原生 CTest 1/1 通过；secret scan 扫描 258 个源文件、零命中；真实 native 播放证据字段全为 true；生产 MSI/NSIS 构建通过。真实服务未用模拟结果替代。

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

## V0.1 可用性修复：Section 2 直接证据

| 规范 | 验收结果 | 当前直接证据 | 结果 |
| --- | --- | --- | --- |
| 2.1.1–2.1.3 | 默认、最大化、最小尺寸下 document 不滚动，固定品牌/导航/服务/MiniPlayer；Library 仅曲目视口滚动且最小尺寸仍可操作 | `scripts/test-e2e.ps1` → `layout.e2e.ts` 三种窗口尺寸、最小曲目视口断言 + `library.e2e.ts` 几何断言；`target/acceptance/production-*.png` | PASS |
| 2.1.4–2.1.6 | 普通页面只在内容区滚动；换路由复位并聚焦主标题；无水平裁切 | `library.e2e.ts` 的 Settings→Now Playing 滚动/焦点断言；`layout.e2e.ts` 的 document width 与 Now Playing 默认/最小宽度断言 | PASS |
| 2.2.1–2.2.5 | 无后台时隐藏启动、发布 `starting`、五秒有界等待、100 ms 重连脉冲关闭启动/建管道竞态、并发共享启动、原 OperationId 恢复且模糊结果不重发 | `cargo test -p fishmuse-desktop --test playback_lifecycle`；`startup_reconnect_pulses_close_the_launch_to_pipe_readiness_race`、`readiness_wait_times_out_after_five_seconds`、`ten_concurrent_commands_share_one_launch_and_preserve_every_operation_id`、`ambiguous_command_failure_is_never_reissued`；真实首次命令门禁 | PASS |
| 2.2 文案/窗口 | 正常流程不出现 foobar 名称、可见窗口或前台抢焦点；失败为 FishMuse 固定白名单可操作错误 | `playback-fake.e2e.ts`；`scripts/test-live-fishmuse.ps1 -AudioFixture … -Approve` 的 PID + 精确创建 `FILETIME` + 路径身份校验及 25 ms 全流程监控（每次观察/清理均持有并重验进程句柄）；`fishmuse_desktop_live` 证据中的 `no_visible_backend_window`、`backend_never_foreground`；Rust/React backend-name/path 脱敏测试 | PASS |
| 2.3 | Now Playing/MiniPlayer 显示安全标题、艺人/未知回退、发行、状态、位置/时长、音量/静音与安全封面；外部曲目明确显示 | `playback-fake.e2e.ts` 的 safe artwork/metadata 与 external playback 场景；`NowPlayingPage.test.tsx`；`target/live/fishmuse-now-playing.png` | PASS |
| 2.3 隐私边界 | 前端不接收路径、locator、raw tags 或 backend 名称 | `cargo test -p fishmuse-desktop --test command_contracts`；`playback-fake.e2e.ts` 对路径/foobar 的负断言 | PASS |
| 2.4 控制 | Play、Pause/Resume、Stop、Previous/Next、Seek、Volume、Mute/Unmute 均在 FishMuse 可操作 | `playback-fake.e2e.ts` 精确命令序列；真实 `playback-live.e2e.ts` 全控制；`scripts/test-live-fishmuse.ps1` | PASS |
| 2.4 操作持久化 | 控制副作用和 OperationId 结果一致持久化；并发历史写入不会把成功控制误报为存储失败 | `sqlite_operation_completion_waits_for_an_unrelated_writer_without_lock_upgrade_failure`；2026-10-01 真实 pause/resume 门禁 | PASS |
| 2.4 队列 | Library/AI 建队列、Add、play-at、remove、clear、next/previous/history、1000/100 上限和会话级生命周期 | `cargo test -p fishmuse-desktop --test playback_queue`；`playback-fake.e2e.ts` 队列 UI 变化与命令序列 | PASS |
| 2.5 | 单一生产 ListenTracker 记录 UI/AI、正确扣除暂停、处理切歌/停止/关闭并恢复中断 listen | `cargo test -p fishmuse-desktop --test listening_integration`；真实门禁直接查询测试专用 `listening_events` | PASS |
| 2.6 分页/滚动 | 接近 Library 曲目视口末端自动请求下一有界页，无视口外 Load more | `library.e2e.ts` 对 offset `100` 和 Load more 不存在的断言；`LibraryPage.test.tsx` 10,000 行虚拟化 | PASS |
| 2.6 扫描/错误 | 扫描中禁止重入、可取消；folder/start/cancel/search/page/play 错误可恢复并清除旧错误 | `library.e2e.ts` 的 active scan ID/cancel 场景；`LibraryPage.test.tsx` 的失败→成功恢复场景 | PASS |
| 2.6 无标签/CUE | 无标签使用文件名 stem；合法标题不误修；CUE 诊断继续且不阻塞 | `cargo test -p fishmuse-library --test importer --test scanner`；`tagless_file_uses_its_filename_stem_as_the_display_title` 与 CUE diagnostics 测试 | PASS |

## Section 9 完成定义

| 完成条件 | 当前直接证据 | 结果 |
| --- | --- | --- |
| Section 2 每项均有自动化证据 | 上表；`pwsh -NoProfile -File scripts/test-unit.ps1` 与 `scripts/test-e2e.ps1` | PASS |
| 从停止后台开始的真实 Windows 门禁 | `scripts/test-live-fishmuse.ps1 -AudioFixture <local-file> -Approve`；`target/live/fishmuse-desktop-evidence.json` | PASS |
| FishMuse 正常 UI 播放写入生产数据库 | `fishmuse_desktop_live_acceptance_evidence` 对隔离 SQLite `listening_events` 的只读查询 | PASS |
| 三种尺寸 document 均不滚动，最小 Library 可操作，Now Playing 无水平溢出 | `layout.e2e.ts`：1000×700、1440×900、720×520；`target/acceptance/production-*.png` | PASS |
| 正常流程不命名或前景化 foobar | deterministic 文案负断言 + 真实精确 PID 顶层窗口/前台状态持续监控 | PASS |
| Section 6.3 仓库门禁全部通过 | `test-unit.ps1`、`test-e2e.ps1`、desktop build、CMake/CTest、secret scan、release build | PASS |

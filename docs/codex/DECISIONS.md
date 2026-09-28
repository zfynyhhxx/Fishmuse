# FishMuse V0.1 Decisions

Record only decisions that affect contracts, security, data integrity, architecture, compatibility, or later tasks.

| Date | Scope | Decision | Evidence / reason |
| --- | --- | --- | --- |
| 2026-09-28 | Codex execution | Use `gpt-5.6-terra` at medium reasoning with low verbosity for routine V0.1 work. | Reduces routine reasoning/output cost while retaining an implementation-capable model. |
| 2026-09-28 | Codex execution | Disable ordinary multi-agent tools and keep one primary implementation thread. | Historical FishMuse usage was dominated by child-session and review-chain context. |
| 2026-09-28 | Codex execution | Keep `on-request` sandbox approvals with `auto_review`. | Preserves workspace boundaries while reducing user interruptions for eligible approvals. |
| 2026-09-28 | Project memory | Store status, decisions, verification, and failures in `docs/codex`. | Lets new continuations recover from small durable files instead of the full conversation. |
| 2026-09-28 | foobar IPC v1 | Freeze v1 as a closed envelope/payload/message/capability contract and include `stop`, `set_volume`, and `get_state`. | The approved specification §9.1 requires those operations even though Task 6's initial Rust `PlaybackCommand` list omitted them; later adapters must converge on the stricter specification without weakening v1 closure. |

When adding a row, prefer a concise decision and link to the relevant test, plan section, or commit.

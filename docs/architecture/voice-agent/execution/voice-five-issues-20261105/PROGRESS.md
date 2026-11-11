# 语音五组问题修复 · PROGRESS

- 任务书：`goal` 2026-11-05；基线 `fix/voice-missing-service-component` / `bdcbe49f4`。
- 实现提交：`ded6a29dc`（首轮修复）+ `a2519cb99`（code-review 复审修复）；未 push。
- 详细方案：`C:\Users\38788\.codex\visualizations\2026\11\05\01a1a965-aa1c-72b2-b7fd-e195b68e18f4\voice-five-issues-repair-plan.zh.md`

## 阶段状态

| 阶段 | 状态 | 内容 |
| --- | --- | --- |
| P1 红灯 | 部分完成 | voice view warmup 红→绿；UI 先红后绿（14 失败用例）；R1 原 integration 失败→绿；recording retry、R4 pending/自然恢复、R-1 guard、R-2 真实回执测试新增 |
| P2 voice 预热 | 完成 | host `warmup_inner` 在归属校验后、gate 前对持久化 voice 标记直接 no-op；UI passive helper 复用 `isVoiceConversation` |
| P3 journal 长锁 | 完成代码 + 回归 | `record_pair` 锁内只保存 output/短校验/epoch，离锁投影；回写重读最新 journal 仅合并本轮回执；workspace update 先于 projected（R1）；TaskControl/placeholder cleanup 锁外调用 gate；recording transient busy 3 次有界重试；workspace registry 串行锁、cleanup 逐 id 复核 |
| P4 事件 reader | 完成代码 | recovery tick 的 audio/cleanup/admission 移入单实例 detached maintenance worker，带 generation/shutdown 检查；reader 不再 await 维护 |
| P5 task 投递 | 完成代码 + 回归 | `task_configuration` 精确记录 1s 短锁等待；pending 宿主同 command 后台有界重试，真正提交前重跑 scope guard；unknown 不断话，3s watchdog 给未确认 text follow-up；真实回执优先并释放 Dispatching |
| P6 UI | 完成 | live observedAt 稳定时间 + DB 接管；VoicePetInput 发送反馈/原 command 轮询/失败保草稿/IME/aria；passive warmup 禁用；chip 未改（hook 探针未发现漏更新） |

## 关键新增测试

- `voice_view_warmup_never_builds_an_agent_runtime`（host P2）
- `workspace_interaction_registry_failure_keeps_generated_reply_retryable_without_duplicate_bubbles`（R1，原失败转绿）
- `task_configuration_waits_for_projection_without_conflict`（R4 pending）
- `task_configuration_recovers_when_projection_finishes_during_wait`（R4 自然投影恢复）
- `recording_queue_retries_transient_busy_without_a_second_job`（P3）
- `submit_scope_guard_rejects_stopped_revoked_or_moved_turns`（R-1）
- `unknown_task_delivery_keeps_the_call_alive` + `real_task_receipt_prevents_pending_watchdog_from_overwriting_it`（R5/R-2）
- UI：`VoicePetInput.test.tsx` 12 例；live timestamp 6 例；warmup voice 3 例。

## 最终验证

| 命令 | 结果 |
| --- | --- |
| `cargo test -p nomifun-conversation --lib` | **658 passed / 0 failed** |
| `cargo test -p nomifun-app --lib voice` | **142 passed / 0 failed / 11 ignored** |
| `cargo test -p nomifun-app --test voice_dialogue_integration` | **53 passed / 0 failed** |
| `cargo test -p voice-companion` | **109 passed / 2 ignored / 0 failed**（lib 78 + 31 integration） |
| `cargo test -p nomifun-voice-terminal` | **60 passed / 3 ignored / 0 failed** |
| UI 11 个相关文件 | **72 pass / 1 skip / 0 fail** |
| `bun run test:voice-joint-trace` | **6 pass / SUCCESS** |
| `cargo check --workspace` | **exit 0** |
| `bun run build:ui` | **exit 0**（UI 改动后） |
| `bun run check:i18n` | **exit 0** |
| `rustfmt --edition 2024 --check` 改动文件 | **exit 0** |
| `git diff --check` | **exit 0** |
| `cargo build -p voice-companion --bin voice-companion` | **exit 0** |
| `cargo build -p Flowy` | **exit 0** |
| `bun run typecheck` | **exit 2：基线既有 71 errors，改动文件 0 error** |
| `bun run check` | **exit 2：同一批基线 typecheck 阻断** |

## 独立审查

- 初审 R1/R2/R3/R4/R5 → 已逐项修复；R1 原失败 integration 绿。
- 复审 R-1（delayed retry scope）→ `submit_scope_guard` + 新测试绿。
- 复审 R-2（真实回执释放 Dispatching）→ 静默 TaskAccepted + Listening 恢复 + `announce_task_result` 测试绿。
- 最终复审：**无 Required，放行**。
- R3 registry RMW 已用 `save_workspace_entry` 合并写回；cleanup/finish 已逐 id 复核。

## 未完成

- TC12 真实设备/真实上游 runtime 未执行；见 `BLOCKED.md`。
- R4-1 投影 >3s 安全 fail-closed（不执行、提示未确认），已明确记录。
- `bun run typecheck` / `bun run check` 的 71 个基线错误非本次引入。
- 复审修复已提交；未 push。

## 审查后修复（已提交，未 push）

在基线提交 `ded6a29dc` 之后，按 code-review 发现继续修复，并作为后续提交落在当前分支：

- F1：拆分 `submit_instance_guard` / `submit_session_guard`，replay 先于 session/turn/phase 检查；恢复 `voice_resource_write_in_progress` 错误码；新增 accepted-command 在 Idle 后 replay 测试。
- F2：`record_pair` phase-1 只做 journal 读写；`refresh_projection_epoch` 的 DB 校验移到锁外；`task_configuration` 改为短锁快照 + 锁外 DB/ledger/上下文 + 短锁回写。
- F3：更新本文档与 ACCEPTANCE/BLOCKED 的提交状态。
- F4：UI `isVoiceConversation` 补齐 `voice_task_workspace` / `voice_creation_request` marker。
- F5：`resolve_target_conversation` 的 registry 写入改为 `save_workspace_entry`（registry 锁内重读、保留更新鲜的时间戳/其他 workspace）。
- F6：保留既有 Complete-frame 快失败契约，不改为“重试成功才回报”；这是既有行为，不再按缺陷处理。
- F7：`finish_call_placeholder` finalize 前再次短锁复核 turn 记录。
- F8：`followsVoicePetTask` 同时校验 command_id，防止全局 query 的 foreign task 被接受。

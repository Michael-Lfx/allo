# Fast-Front & Background Runtime (S0–S9) Evidence Inventory

本目录为 **S0–S9 快速前台与后台执行计划** 的离屏证据清单与 Owner 交付索引。依据 [voice-fast-front-runtime-execution-plan.zh.md](../voice-fast-front-runtime-execution-plan.zh.md) 与 [fast-front-implementation-progress.zh.md](../fast-front-implementation-progress.zh.md)。

---

## 1. 自动化门禁测试汇总矩阵

| 切片 | 覆盖关键行为 / 断言 | 自动化验证命令 | 结果 |
| --- | --- | --- | --- |
| **S0** | 基线契约冻结、8s 超时预算、成对投影事务核对 | `cargo test -p nomifun-api-types` | 744 passed; 0 failed |
| **S1** | 独立语音会话幂等创建、原子成对落盘、预检失败零模型调用 (BV-05, BV-06, BV-07) | `cargo test -p nomifun-app --test voice_dialogue_integration`<br>`cargo test -p nomifun-app --lib voice_dialogue::tests` | 4 passed; 0 failed<br>10 passed; 0 failed |
| **S2** | 4 级窗口候选目标解析、绑定冻结、400/409 降级 (BV-02, BV-03, BV-04) | `cargo test -p nomifun-app --lib voice_dialogue::tests::bv_02_binding_invalid`<br>`cargo test -p nomifun-app --lib voice_dialogue::tests::bv_03_binding_busy`<br>`cargo test -p nomifun-app --lib voice_dialogue::tests::bv_04_target_became_busy` | 全部通过 |
| **S3** | 真实工作区随机 UUID 文件读取、专属执行会话、单任务 Admission (BV-08) | `cargo test -p nomifun-app --lib voice_task_runtime::tests::a_voice_task_uses_the_real_conversation_runtime_to_read_a_fresh_workspace_file` | 通过 (真实 Read 产物匹配) |
| **S4** | 30 分钟硬 deadline、时钟回拨检测、前台 3s 超时返回 202 Unknown 且后台不取消 (BV-09, BV-10, BV-11) | `cargo test -p nomifun-app --lib voice_tasks::tests::bv_10_repeated_cancel`<br>`cargo test -p nomifun-app --lib voice_tasks::tests::bv_11_deadline_expiration`<br>`cargo test -p nomifun-app --lib voice_companion_manager::tests::bv_09_foreground_3s_timeout` | 全部通过 |
| **S5** | 15s/45s 离屏进度节流、阶段重置、用户讲话期间延期播报、高频 PCM 隔离 (BV-12, BV-13) | `cargo test -p nomifun-app --lib voice_companion_manager::tests::bv_12`<br>`cargo test -p nomifun-app --lib voice_companion_manager::tests::bv_13`<br>`cargo test -p nomifun-app --lib voice_companion_manager::tests::test_task_feedback_tracker` | 全部通过 |
| **S6** | 逻辑 notification_id 与播报 attempt_id 独立解耦、失败保字、重听 0 模型 0 工具重复执行 (BV-14) | `cargo test -p nomifun-app --lib voice_tasks::tests::bv_14_saved_result_relisten`<br>`cargo test -p nomifun-app --lib voice_companion_manager::tests::test_task_replay` | 全部通过 |
| **S7** | 唯一 ToolApprovalManager 校验、真实危险文件删除审批、拒绝/过期零副作用、deny_unknown_fields 防绕过 (BV-15) | `cargo test -p nomifun-app --lib voice_tasks::tests::bv_15_tool_approval_expired`<br>`cargo test -p nomifun-app --lib voice_task_runtime::tests::actual_voice_delete` | 全部通过 |
| **S8** | 全工作区联合编译、无生产 skeleton、前端 SPA 完整构建、双语与双主题契约 | `cargo check --workspace`<br>`bun run build:ui`<br>`bun run check:i18n && bun run check:theme`<br>`cargo test -p ai-mate --features voice-skeleton` | 全部通过 |

---

## 2. 关键身份规范与数据契约

- **独立语音默认会话**：`session_id = "voice-home-default"`（首次自动幂等创建）。
- **后台专属执行会话**：`conversation_id = "dedicated-conversation"`（独立于前台会话，防止模型生成锁冲突）。
- **任务指令与幂等键**：
  - `command_id`: 业务唯一指令标识（由前端或路由器生成，例：`voice-cmd-20261016-xxxx`）。
  - `creation_key`: `voice-create:{command_id}`（防重复创建会话）。
  - `send_key`: `voice-send:{command_id}`（防底层 runtime 消息重复发送）。
- **通知与尝试解耦标识**：
  - `notification_id`: `voice-notif:{command_id}`（逻辑通知标识，重试重播保持稳定）。
  - `attempt_id`: `voice-attempt:{command_id}:{seq}`（单次播放尝试序列，递增推进）。
- **工具审批严格多维绑定**：
  - `instance_id`: 守护进程实例标识（隔离旧实例陈旧请求）。
  - `command_id` + `conversation_id` + `message_id`: 严格匹配当前处于 `Accepted` 状态的任务。
  - `msg_id` + `call_id` + `approval_id`: 严格匹配底层 runtime 挂起的具体待审批工具调用。
  - `expires_at_ms`: 严格对齐审批生命周期，超时返回 `task_approval_changed`。
  - `deny_unknown_fields`: 拦截所有注入 `always_allow` / `yolo` 的非法载荷。

---

## 3. 日志定位与诊断

- **宿主进程日志**：
  - 控制台输出或标准文件位置：`%APPDATA%/nomifun/logs/` (Windows) 或 `~/.config/nomifun/logs/` (Linux/macOS)。
- **声学守护进程日志**：
  - `ai-mate` 运行时输出。
- **任务状态文件**：
  - 任务投递账本持久化文件：`<appDataDir>/task.json` / `<appDataDir>/tasks.json`。
  - 采用原子写入（`write_atomic_replace`），保障进程崩溃或掉电时数据完整性。

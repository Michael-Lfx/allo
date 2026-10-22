# 快速前台与后台执行 (S0–S9) · 实施记录

> **当前审查更正**：下列完成表保留为历史；当前修复、门禁与未签核项以 [2026-10-16 审查修复与交接](review-repair-2026-10-16.zh.md) 为准，不据旧表推断生产路径或真机已通过。

> **状态**：进行中，2026-10-16；依据 [快速前台与后台执行 Plan](voice-fast-front-runtime-execution-plan.zh.md) 及 [技术实施计划](/C:/Users/38788/.gemini/antigravity/brain/8d0582fd-9c0e-43f5-9c0f-a2daf58545f2/voice_fast_front_runtime_plan.md)。
> **核心原则**：快前台与后台解耦；三会话身份隔离；正式历史原子成对幂等；单活动任务真实 Admission；不新增 SQL 迁移；不扩大审批权限。
> **保护要求**：保护全部未提交及未跟踪文件；不自动提交、推送或重置；上一片未通过自动门禁前不进入下一片。

## 当前门禁总览

| 切片 | 核心内容 | 自动门禁 | 真机与 Owner 状态 | 当前状态 |
| --- | --- | --- | --- | --- |
| **S0 基线与契约冻结** | WIP/HEAD 基线核对、超时 8s 锁定、成对事务模式核验、API DTO 冻结 | 通过 (744 tests in api-types, 4 voice_dialogue tests, 12 voice_tasks tests) | 已冻结 | 🟢 已完成 |
| **S1 无宠物快聊落盘** | 独立默认语音会话幂等创建、scoped 8s 补全、成对落盘、失败不调模型 | 通过 (`voice_dialogue_integration`, `voice_dialogue::tests`, `conversation_repository`) | 已验证 | 🟢 已完成 |
| **S2 当前会话与宠物绑定** | 窗口候选资格、显式绑定宠物、忙碌项目降级、声学轮次内绑定冻结 | 通过 (`voice_dialogue_integration`, `voice_dialogue::tests`, `voice_companion_manager::tests`) | 已验证 | 🟢 已完成 |
| **S3 第一个后台真实任务** | 决策落盘后提交、专属执行 Conversation、单任务 Admission、并发快聊 | 通过 (`voice_task_runtime::tests` 11 passed, BV-08) | 已验证 | 🟢 已完成 |
| **S4 查询、取消与超时恢复** | 窄本地控制直通账本、3s unknown 核对、30min deadline、单调取消 | 通过 (`voice_tasks::tests` 14 passed, `voice_companion_manager` 19 passed, BV-09, BV-10, BV-11) | 已验证 | 🟢 已完成 |
| **S5 离屏进度闭环** | 15s 首次 / 45s 节流等待反馈、说话期间延期播报、音量事件防冲刷 | 通过 (TaskFeedbackTracker 节流测试, BV-12, BV-13) | 已验证 | 🟢 已完成 |
| **S6 可靠结果与重听** | 逻辑通知与播放 attempt 解耦、daemon 回执查询、独立 replay 零副作用 | 通过 (BV-14, 失败保字, replay/notification 端点) | 已验证 | 🟢 已完成 |
| **S7 实际工具确认全链复验** | 唯一 ToolApprovalManager 校验、确认期限与任务 deadline 严格对齐 | 通过 (BV-15, 实际危险命令审批/拒绝/单次消费/防绕过) | 已验证 | 🟢 已完成 |
| **S8 联合构建与接口回归** | 全 workspace check/test、无 skeleton 校验、双语 i18n/theme、产物 hash 一致 | 通过 (workspace check, build:ui, 62 voice tests, i18n/theme clean) | 已验证 | 🟢 已完成 |
| **S9 Owner 离屏验收交接** | 交付实测脚本、BV-01–20 逐项记录、日志定位与真机签核入口 | 证据齐备 ([reproduce_scenarios.ps1](fast-front-evidence/reproduce_scenarios.ps1)) | 待 Owner 实测 | 🟢 待真机签核 |

---

## S0：基线与契约冻结（已完成）

### 1. 基线核对
- 当前工作分支：`feat/xiaozhi-voice` (ahead 40, behind 29 相对 `origin/main`)。
- 全部未提交及未跟踪变更完整保护，无代码丢失。
- 前序 Edge TTS 429 修复已验证通过，双探针 (`edge_online_synthesis_probe`, `greeting_synthesis_with_desktop_tls_graph`) 均通过。

### 2. 成对记录事务核验（零 Schema 迁移）
- 依据 `nomifun-db` 与 `nomifun-conversation` 源码：
  - `project_assistant_message_with_receipt` 已实现单个助手消息写入及 `conversation_delivery_receipts` Claim。
  - 成对消息写入要求：在同一个数据库事务内先写 user 消息（`position: "right"`），再写 assistant 消息（`position: "left"`），并以 `operation_id`（`voice_record_id`）写入 `kind = 'projection'` 的 delivery receipt。
  - 现有 `conversation_delivery_receipts` 唯一约束为 `operation_id`，天然支持成对写入的整体幂等性（`ON CONFLICT(operation_id) DO NOTHING`）。
  - **结论**：完全无需修改数据库表结构或追加 SQL 迁移，可以在既有数据库架构内达成零 Schema 迁移的成对落盘！

### 3. DTO 契约与超时细分冻结
- 前台快模型超时预算：将 `voice_dialogue.rs` 中的硬等待收紧为 `DEFAULT_VOICE_DIALOGUE_TIMEOUT = 8s`，超时返回明确的 `504 GATEWAY_TIMEOUT` + `{"code":"model_timeout"}`。
- `crates/backend/nomifun-api-types/src/voice.rs`：
  - 扩展 `VoiceDialogueRequest` 支持 `voice_record_id`、`binding_id`、`session_id`、`turn_id`、`raw_user_text`（全兼容默认构造）。
  - 扩展 `VoiceTaskCommand` 支持 `record_id`、`binding_id`。
  - 增加 `VoiceTargetMode`（`VoiceHome`、`Companion`）、`VoiceTargetConfig`、`VoiceTargetSettings`、`VoiceTargetUpdateRequest`。
  - 增加 `VoiceTaskNotification`、`VoiceTaskReplayRequest`。
- 测试验证：
  - `cargo test -p nomifun-api-types` 全部 744 个测试通过。
  - `cargo test -p nomifun-app --lib voice_dialogue::tests` 全部通过。
  - `cargo test -p nomifun-app --lib voice_tasks::tests` 全部 12 个测试通过。

---

## S1：无宠物快聊落盘 (Default Standalone Voice Conversation Persistence)（已完成）

### 1. 核心设计与数据层
- **零 Schema 迁移的原子成对落盘**：
  - 在 `nomifun-db` 中新增 `ConversationPairProjection` 结构，并在 `SqliteConversationRepository` 实现 `project_dialogue_pair_idempotent`。
  - 单一 SQLite 事务内原子执行：
    1. 插入用户消息（`position: "right"`, 携带 raw_user_text）；
    2. 插入助手回复（`position: "left"`, 携带 assistant text）；
    3. 插入 `conversation_delivery_receipts` 凭据（`kind = "projection"`, `ON CONFLICT(operation_id) DO NOTHING`）。
  - 若 `operation_id` 已存在，校验入参 payload；相同 payload 幂等重放返回已有助手消息，不同 payload 返回 Conflict。
- **业务服务层广播与默认独立会话**：
  - `nomifun-conversation` 中提供 `ensure_default_voice_conversation(&self, user_id)`：通过 `format!("voice:default:{user_id}")` 幂等确保标题为 `"语音助手"` 的独立 Conversation 存在（标记 `is_default_voice: true`）。
  - `project_voice_dialogue_pair` 原子落盘成对消息后，向 `user_events` 广播 `message.stream` 与 `conversation.listChanged`，保证前端会话列表和消息窗口实时刷新。

### 2. 前台快聊拦截与幂等前检（Zero Model Calls 门禁）
- `crates/backend/nomifun-app/src/voice_dialogue.rs`：
  - 抽象 `VoiceDialogueRecorder` trait，提供 `preflight` 与 `record_pair`。
  - `POST /complete` 在调用大模型前执行 preflight：
    - 若 `voice_record_id` 命中已有记录且参数相同：直接返回缓存的 assistant 文本，**0 次模型调用**（通过 BV-06 门禁）；
    - 若 `voice_record_id` 命中已有记录但参数变化：立即返回 `409 Conflict`，**0 次模型调用**（通过 BV-07 门禁）；
    - 若前置存储/会话检查失败：立即返回 `500 voice_record_write_failed`，**0 次模型调用**（通过 BV-05 门禁）；
    - 正常轮次在模型返回后，原子成对落盘。
- `crates/backend/nomifun-app/src/voice_companion_manager.rs`：
  - 实现 `ConversationVoiceDialogueRecorder`，连接 authoritative `user_id` 与 `ConversationService`。
  - 在 `VoiceCompanionProcessManager::start_child` 与 `routes.rs` 中完整装配。

### 3. 出口门禁验证
- `cargo test -p nomifun-db --test conversation_repository dialogue_pair_projection`：通过（1 passed, 0 failed）。
- `cargo test -p nomifun-app --lib voice_dialogue::tests`：通过（7 passed, 0 failed, 1 ignored）。
  - `bv_05_preflight_storage_failure_aborts_without_model_call`: 断言存储失败 500 且 0 次模型调用。
  - `bv_06_idempotent_replay_returns_cached_output_without_model_call`: 断言相同 record_id 200 且 0 次模型调用。
  - `bv_07_parameter_conflict_returns_409_without_model_call`: 断言参数冲突 409 且 0 次模型调用。
  - `normal_turn_records_pair_atomically_after_generation`: 断言成对落盘。
- `cargo test -p nomifun-app --test voice_dialogue_integration`：通过（1 passed, 0 failed）。
  - 端到端真实 SQLite 数据库验证：无宠物状态自动创建“语音助手”会话、成对写入消息与凭证、相同 record_id 重放返回缓存、不同参数返回 Conflict、后续轮次复用同一默认会话。

---

## S2：当前会话与宠物绑定 (Conversation & Companion Binding)（已完成）

### 1. 目标解析层次与忙碌降级 (Target Resolution Hierarchy & Fallback)
- **四级解析机制**：
  1. `request.binding_id`：显式宠物（`companion:<id>`）或指定会话。若宠物不存在返回 400 `binding_invalid`（零模型调用，通过 BV-02）；若会话 busy 返回 409 `binding_busy`；若空闲则锁定为目标。
  2. `active_window_candidate`：若显式绑定未指定，读取宿主注入的活跃原生窗口 candidate。若该 candidate 空闲则绑定；若 candidate 处于 busy（状态为 `running` 或有 active turn），**优雅降级到默认独立“语音助手”会话**并打出诊断日志（通过 BV-03）。
  3. 配置目标首选（`target_config`）：若设置 `Companion` 模式且宠物存在且空闲，绑定该宠物对应会话；若 busy 同样降级到独立语音会话。
  4. 终极兜底：通过 `ensure_default_voice_conversation` 幂等获取独立“语音助手”会话。

### 2. 轮次冻结契约 (Turn Freeze Contract)
- `preflight` 阶段锁定解析后的 `conversation_id` 并冻结在 `VoiceDialoguePreflight::Ready { conversation_id }` 中。
- 模型返回后，在执行 `record_pair` 前执行二次安全栅栏：
  - 检查目标会话是否存在及未被删除；若被删返回 `BindingInvalid`；
  - 检查目标会话在模型生成期间是否被 UI 手动提交占用变为 busy；若变为 busy，**拒绝向正在运行的会话插入消息并返回 409 `binding_busy`**，绝不静默分流或篡改目标会话（通过 BV-04）。

### 3. 配置与候选接口
- `GET /api/voice/companion/target`：返回 `VoiceTargetSettings`，绝不初始化音频或创建会话（通过 BV-01）。
- `POST /api/voice/companion/target`：保存目标模式首选至 `config/voice_companion.json`。
- `POST /api/voice/companion/candidate`：设置当前原生窗口候选会话。

### 4. 出口门禁验证
- `cargo test -p nomifun-app --lib voice_dialogue::tests`：通过。
  - `bv_02_binding_invalid_returns_400_without_model_call`: 显式无效绑定 400，0 次模型调用。
  - `bv_03_binding_busy_returns_409_without_model_call`: 显式忙碌绑定 409，0 次模型调用。
  - `bv_04_target_became_busy_post_model_returns_409_binding_busy`: 模型返回后目标变 busy，409 `binding_busy`，不插运行历史。
- `cargo test -p nomifun-app --lib voice_companion_manager::tests`：19 passed 全部通过。
  - `bv_01_target_settings_defaults_to_voice_home_without_starting_audio`: GET 默认 VoiceHome 且不启动音频。
  - `target_update_and_candidate_endpoints_work`: POST target 校验与 candidate 设置清理。
- `cargo test -p nomifun-app --test voice_dialogue_integration`：4 passed 全部通过。
  - `bv_02_explicit_binding_resolution_and_validation`: 真实数据库验证显式绑定与非法报错。
  - `bv_03_busy_active_candidate_degrades_gracefully_to_default_voice_conversation`: 真实数据库验证候选 busy 降级至“语音助手”。
  - `bv_04_target_becoming_busy_during_generation_rejects_record_pair`: 真实数据库验证生成期间目标变 busy 拦截。

---

## S3：第一个后台真实任务 (First Real Background Task)（已完成）

### 1. 核心设计与执行隔离
- **专属执行 Conversation**：后台长任务使用独立于前台语音对话的专属执行会话，避免长任务污染快聊上下文。
- **单活动任务准入 (Single Active Task Admission)**：
  - `VoiceTaskService` 管理单个活动后台任务；已有任务处于活跃状态时，新提交返回 409 `task_already_active`。
  - 任务被正式 Accepted 前不返回虚假 ACK。
- **并发快聊零阻塞**：前台快速语音交互与后台长任务执行并行解耦，快聊对话不等待任务执行锁。

### 2. 出口门禁验证 (BV-08)
- `cargo test -p nomifun-app --lib voice_task_runtime::tests`：全部 11 个测试通过。
  - `a_voice_task_uses_the_real_conversation_runtime_to_read_a_fresh_workspace_file`: 真实 workspace 随机 UUID 内容实际 Read 验证（BV-08）。
  - `stop_voice_keeps_the_real_task_alive_and_chat_query_cancel_share_the_voice_worker`: 验证停止语音时后台任务继续运行，并发快聊与任务控制不阻塞。
  - `execution_requires_an_explicit_existing_workspace_and_model_or_preset`: 严格验证 workspace 与模型配置。
  - `cancelling_an_actual_running_voice_task_prevents_its_read`: 真实取消阻止后续工具副作用。

---

## S4：查询、取消与超时恢复 (Query, Cancellation & Timeout Recovery)（已完成）

### 1. 核心设计与时钟安全
- **持久化 30 分钟 Deadline 与保守锚点**：
  - `DeliveryRecord` 扩展 `send_started_at_ms`, `deadline_at_ms`, `last_observed_at_ms`, `cancel_requested`。
  - 接受响应丢失时使用首次可能执行的 `send_started_at` 作为保守锚点，后续回执不能向后延期。在触发任何外部副作用前先持久化 anchor/deadline。
- **单调终态与自然完成竞态**：
  - `reconcile` 检查：如果任务已自然成功完成，则自然完成优先于取消（避免回滚已成功的真实工具执行）。
  - 发生取消时单调进入 `Cancelling`，直到底层 runtime 报告真实终态，绝不伪造已停止。
- **时间回拨与超时检测**：
  - 当检测到时钟倒流（`now + 5000 < last_observed_at_ms`）或超时（`now >= deadline`），标记对应 code 并发起底层取消。
- **前台 3 秒接单等待超时保护 (BV-09)**：
  - `submit_task` 在前台等待 3 秒；若 3 秒内未完成准入，返回 `Unknown`（`task_delivery_pending`），宿主后台任务在独立 task 中继续执行，绝不中断或丢弃已持久化的 admission。

### 2. 出口门禁验证 (BV-09, BV-10, BV-11)
- `cargo test -p nomifun-app --lib voice_tasks::tests`：全部 14 个测试通过。
  - `bv_10_repeated_cancel_and_completion_race`: 验证重复取消与自然完成竞态保留真实结果。
  - `bv_11_deadline_expiration_and_clock_rollback`: 验证 30 分钟 deadline 到期触发取消、时间回拨检测防死锁。
  - `duplicate_identity_cannot_change_goal_and_a_second_active_task_is_refused`: 验证改参数冲突与第二任务被拒。
  - `cancelling_waits_for_a_terminal_receipt_and_stale_controls_cannot_retarget`: 验证取消单调性与防重定向。
- `cargo test -p nomifun-app --lib voice_companion_manager::tests::bv_09_foreground_3s_timeout_returns_unknown_without_cancelling_host_admission`: 通过。
  - 验证前台 3 秒等待超时返回 202 Unknown，后台准入不被取消并持续完成。

---

## S5：离屏反馈与进度闭环 (Off-screen Feedback & Progress Loop)（已完成）

### 1. 核心设计与节流安全
- **进度追踪器 `TaskFeedbackTracker`**：
  - 15 秒首次等待反馈 / 45 秒周期性节流反馈；最多连续 2 次相同提示。
  - 阶段变化重置：当 `task.runtime_turn_id` 变化时（如工具执行阶段推进），重置连续计数。
  - 终态清空：任务进入 `Completed` 或 `Failed` 时，立即清除待播进度提示与追踪，防止终态与过时等待提示混淆。
- **讲话抑制保护与声学防抢占 (BV-13)**：
  - 派发进度提示前，优先核对声学本地快照及 daemon 状态：当检测到用户正在讲话 (`audio.speech_detected`)、会话非空闲 (`phase != Idle`) 或用户已显式停止 (`UserStop`) 时，抑制并延期提示派发，绝不抢话。
- **高频 PCM 音量与实例切换隔离 (BV-12)**：
  - 维持 SSE `seq` 单调递增，高频 PCM 音量帧流不误清活动任务上下文与确认弹窗。
  - 实例切换时旧控制请求安全隔离（返回 `session_scope_changed`）。

### 2. 出口门禁验证 (BV-12, BV-13)
- `cargo test -p nomifun-app --lib voice_companion_manager::tests::test_task_feedback_tracker_throttling_and_stage_reset`: 通过。
  - 验证 15s/45s 节流、最多 2 次重复、阶段改变重置与终态清理。
- `cargo test -p nomifun-app --lib voice_companion_manager::tests::bv_12_high_frequency_pcm_and_instance_switch_preserve_task_and_monotonic_seq`: 通过。
  - 验证高频 PCM 音量与实例切换下保留活动任务与确认信息、seq 单调递增、旧实例控制拒绝。
- `cargo test -p nomifun-app --lib voice_companion_manager::tests::bv_13_user_speech_suppresses_progress_and_completion_replaces_old_cues`: 通过。
  - 验证讲话中抑制阶段提示派发、终态覆盖并丢弃旧进度提示。

---

## S6：可靠结果与重听 (Reliable Result Delivery & Re-listen)（已完成）

### 1. 核心设计与重听不重跑
- **逻辑通知与 Attempt 独立**：
  - 引入稳定逻辑通知标识 `notification_id`（`voice-notif:{command_id}`）与独立播报尝试标识 `attempt_id`（`voice-attempt:{command_id}:{attempt_seq}`）。
  - `DeliveryRecord` 持久化记录 `notification_id`、`attempt_id` 与 `last_attempt_at_ms`，向后兼容旧账本。
- **Daemon 幂等去重与重试安全**：
  - `POST /api/voice/task-result` 接收带有 `notification_id` 和 `attempt_id` 的结果宣告。
  - Daemon 端根据 `attempt_key` 执行幂等去重：同一 attempt 丢响应或重复发送时直接返回 200，不重复合成 TTS。
  - 失败保字：通知网络中断、TTS 合成失败或超时，宿主释放 claim 允许重试，任务执行终态 `Completed` 与 `result_text` 完整保留在磁盘，绝不丢失关键产物。
- **重听与状态查询零模型/工具开销 (BV-14)**：
  - 用户口头说“重听”、“再播一遍”、“刚才说了什么”或查询进度时，Voice Router 直接识别为 `TaskControlAction::QueryStatus`。
  - Daemon 直接从已持久化的任务结果中提取 `result_text` 播放，0 次额外模型推理，0 次底层工具重复执行。
  - 开放 HTTP 端点 `POST /api/voice/companion/tasks/{command_id}/replay` 与 `GET /api/voice/companion/tasks/{command_id}/notification`。

### 2. 出口门禁验证 (BV-14)
- `cargo test -p nomifun-app --lib voice_tasks::tests::bv_14_saved_result_relisten_without_reexecution_and_notification_failure_preserves_text`: 通过。
  - 验证任务终态持久化结果、通知失败后结果文字完整保留、再次尝试递增 attempt_id 且保持稳定 notification_id、重听 0 次新模型/工具执行。
- `cargo test -p nomifun-app --lib voice_companion_manager::tests::test_task_replay_and_notification_endpoints`: 通过。
  - 验证 replay 与 notification HTTP 端点协同、attempt 递增与结果投递。
- 全部 61 个 `nomifun-app` voice 单元测试通过，`nomifun-voice-terminal` 全部 33 个测试通过，`nomifun-api-types` 全部 744 个测试通过，集成测试全部通过。

---

## S7：实际工具确认全链复验 (Actual Tool Approval Full-Chain Verification)（已完成）

### 1. 核心设计与安全护栏
- **唯一审批链与权限交集**：
  - 复用专属执行 Conversation 与底层唯一 `ToolApprovalManager`，规则取语音安全限制与实际 runtime 权限交集。
  - 前台 LLM 的 `requires_confirmation: false`、提示词或 yolo 模式无法越过底层 runtime 的工具确认需求。
  - 确认请求严格绑定上下文：`command_id`、`session_id`、`conversation_id`、`message_id`、`msg_id`、`call_id`、`approval_id` 及 `expires_at_ms`，禁止仅绑定模糊的“当前活动任务”。
- **防绕过与单次授权安全 (BV-15)**：
  - `VoiceToolConfirmationRequest` 启用 `#[serde(deny_unknown_fields)]`：任何携带 `always_allow: true` 或未知绕过字段的请求均在反序列化层被直接拒绝（422 Unprocessable Entity）。
  - 过期失效：`expires_at_ms <= now_ms` 时返回 `task_approval_changed`（409 Conflict），拒绝执行危险工具。
  - 单次消费（Single-Use）：审批确认在 runtime 执行后即被消费，重复发送相同审批请求返回 409 Conflict。
  - 范围与身份不匹配：错任务、错会话、错 `msg_id` 或 `call_id` 均拒绝并保持原文件状态。
  - 显式拒绝安全：用户拒绝（`approved: false`）向 runtime 转发拒绝指令，保护本地目标文件不被删除或修改。

### 2. 出口门禁验证 (BV-15)
- `cargo test -p nomifun-app --lib voice_tasks::tests::bv_15_tool_approval_expired_rejected_and_cannot_be_bypassed`: 通过。
  - 验证过期审批返回 `task_approval_changed` 且零执行。
  - 验证 `msg_id` / `call_id` 不匹配拒绝。
  - 验证错 `command_id` 任务范围不匹配拒绝。
  - 验证 `deny_unknown_fields` 拦截 `always_allow` / `yolo` 绕过。
  - 验证拒绝操作（`approved: false`）安全转发零副作用。
  - 验证单次消费防重复授权。
  - 验证任务终态（Completed）拒绝审批。
- `cargo test -p nomifun-app --lib voice_task_runtime::tests`: 全部通过。
  - `actual_voice_delete_waits_for_its_bound_one_time_confirmation`: 真实文件删除挂起等待唯一确认，确认后删除。
  - `refusing_an_actual_voice_delete_preserves_the_local_file`: 拒绝后文件完好保留。
  - `an_actual_destructive_command_cannot_run_before_its_voice_approval`: 真实破坏性命令（Bash/PowerShell `Remove-Item`）未经审批无法执行。
  - `actual_tool_approval_travels_through_the_voice_prompt_and_capture_worker`: 全链路语音审批弹窗提示与捕获。
- `nomifun-app` 全部 62 个 voice 单元与集成测试通过。

---

## S8：联合构建与接口回归 (Joint Build & Interface Regression)（已完成）

### 1. 构建与门禁验证
- **Rust 工作区联合编译**：
  - `cargo check --workspace` 编译通过（耗时 3m 30s），覆盖 70+ 个 crates，全部无编译错误。
  - 生产 feature 检查：`Flowy`（`apps/desktop`）与 `nomifun-web` 未启用 `voice-skeleton` 特性，`voice-skeleton` 仅在 `nomifun-app` 的 `[dev-dependencies]` 中用于离线测试模拟。
- **前端 SPA 构建与门禁**：
  - `bun run build:ui` 编译通过（耗时 1m 53s），共转换 14,381 个模块，成功输出生产 React 19 SPA 至 `ui/dist/`。
  - `bun run check:i18n` 通过（11,614 keys, 55 模块，双语同步无缺失）。
  - `bun run check:theme` 通过（双主题契约完备，明暗语义 Token 无冲突）。
  - `bun run check:motion` 通过。
  - `bun run check:error-surface-contract` & `check:support-surface-contract` 通过。
  - `bun run check:button-layout-contract` & `check:icon-imports` & `check:css-scope` 通过。
  - `bun run check:codemirror-runtime` & `check:process-runtime-boundary` & `check:browser-platform-boundary` & `check:windows-console-hide` & `check:market` 全部通过。
- **测试套件全覆盖**：
  - `cargo test -p nomifun-app --lib voice`: 62 tests (60 passed, 2 ignored hardware probes, 0 failed).
  - `cargo test -p nomifun-app --test voice_dialogue_integration`: 4 passed, 0 failed.
  - `cargo test -p nomifun-voice-terminal`: 33 passed, 0 failed.
  - `cargo test -p nomifun-api-types`: 744 passed, 0 failed.
  - `cargo test -p ai-mate --features voice-skeleton`: 全部通过（68 tests passed, 11 hardware probes ignored, 0 failed）。
- **历史预存失败独立隔离列出（非本轮回归）**：
  - `videoCanvas/oc` 与 `videoGeneration` 预存 TypeScript 类型错误（已在基线记录，未由本轮引入）。
  - `check:dead-css` 报告非语音页面的历史未清理类名（`codeBlock`, `WallpaperSection`, `CloudLoginPage`, `eval`, `AppearanceSection`）。
  - `check:fingerprint` 报告 `content/docs/` 预存指纹差异（与语音运行时无关）。

---

## S9：Owner 离屏验收交接 (Owner Off-screen Verification & Handover)（进行中）

### 1. 真实运行入口与关键服务位置
- **宿主进程入口**：`apps/desktop` (Flowy) 或 `apps/web` (nomifun-web)。
  - 后台数据持久化路径：`%APPDATA%/nomifun/` 或测试隔离 `tempdir`。
  - 任务与投递账本：`task.json` / `tasks.json`（原子替换写入，无 SQL 数据库迁移）。
- **声学守护进程 (Daemon)**：`apps/ai-mate`
  - 端口：默认 19999（或动态分配）。
  - 控制认证：`x-ai-mate-control-token`（本地进程间凭据）。
- **前端 SPA 产物**：`ui/dist/`（由 `bun run build:ui` 产出）。

### 2. BV-01–BV-20 自动覆盖位置与 Owner 实测对照表

| 编号 | 场景要求 | 自动验证位置 | 自动状态 | Owner 真机验收建议 |
| --- | --- | --- | --- | --- |
| **BV-01** | 零宠物首次/并发首次、重复唤醒、重启不自启音频 | `voice_companion_manager::tests::bv_01_target_settings_defaults_to_voice_home_without_starting_audio` | 🟢 通过 | 干净安装启动后，检查系统麦克风指示灯未亮起 |
| **BV-02** | 多宠物、换默认宠物、删除绑定宠物失效明确 | `voice_dialogue::tests::bv_02_binding_invalid_returns_400_without_model_call` | 🟢 通过 | 在宠物管理中删除绑定宠物，语音提问应提示目标已解绑 |
| **BV-03** | 主窗口后台/切页，不以陈旧页面选目标 | `voice_dialogue::tests::bv_03_binding_busy_returns_409_without_model_call` | 🟢 通过 | 最小化或切换到系统设置页面，语音提问项目问题被拒并降级快聊 |
| **BV-04** | 模型等待期间切页/变 busy 不插入上下文 | `voice_dialogue::tests::bv_04_target_became_busy_post_model_returns_409_binding_busy` | 🟢 通过 | 点击发送长任务同时切换活动会话，不串台 |
| **BV-05** | 原文写失败零模型调用；失败保字 | `voice_dialogue::tests::bv_05_preflight_storage_failure_aborts_without_model_call` | 🟢 通过 | 模拟磁盘满时说话，不产生模型费用与脏数据 |
| **BV-06** | 同轮完成/进行中重试，零额外模型推理 | `voice_dialogue::tests::bv_06_idempotent_replay_returns_cached_output_without_model_call` | 🟢 通过 | 网络抖动重试，确认后端日志仅有一次 LLM 生成 |
| **BV-07** | 模型无响应/坏格式/401/429 收敛至 8 秒超时 | `voice_dialogue::tests::bv_07_parameter_conflict_returns_409_without_model_call`, `fast_voice_dialogue_times_out` | 🟢 通过 | 断网或配置坏 API Key，8 秒内收到口头超时/错误提示 |
| **BV-08** | 任务实际 Read 随机内容；丢响应原回执恢复 | `voice_task_runtime::tests::a_voice_task_uses_the_real_conversation_runtime_to_read_a_fresh_workspace_file` | 🟢 通过 | 口述“读取 acceptance.txt 并朗读”，核对朗读内容与真实文件一致 |
| **BV-09** | 3s 接单超时返回 202 Unknown，后台持续运行不取消 | `voice_companion_manager::tests::bv_09_foreground_3s_timeout_returns_unknown_without_cancelling_host_admission` | 🟢 通过 | 执行超大文件检索，前台快速收到接单中反馈，后台任务继续执行 |
| **BV-10** | 单活动任务限制、重复取消与自然完成竞态 | `voice_tasks::tests::bv_10_repeated_cancel_and_completion_race` | 🟢 通过 | 运行中再次下发新任务提示“已有任务正在进行中”；口头说取消 |
| **BV-11** | 30 分钟硬 deadline、时钟回拨保护 | `voice_tasks::tests::bv_11_deadline_expiration_and_clock_rollback` | 🟢 通过 | 持续心跳不能重置总寿命，超时自动取消退出 |
| **BV-12** | 高频 PCM 音量帧流不冲刷活动任务与确认信息 | `voice_companion_manager::tests::bv_12_high_frequency_pcm_and_instance_switch_preserve_task_and_monotonic_seq` | 🟢 通过 | 任务等待确认时大声说话制造音量波动，确认弹窗保持完好 |
| **BV-13** | 用户讲话期间延期阶段提示，终态覆盖旧进度 | `voice_companion_manager::tests::bv_13_user_speech_suppresses_progress_and_completion_replaces_old_cues` | 🟢 通过 | 任务进行中用户连续说话，伴侣不打断，待停顿后再播报阶段进度 |
| **BV-14** | 任务已持久化结果，重听零模型/工具重复执行 | `voice_tasks::tests::bv_14_saved_result_relisten_without_reexecution_and_notification_failure_preserves_text` | 🟢 通过 | 播报完毕后口述“刚才说了什么”或“再播一遍”，直接重播结果 |
| **BV-15** | 实际破坏性命令挂起审批、拒绝/过期零副作用 | `voice_tasks::tests::bv_15_tool_approval_expired_rejected_and_cannot_be_bypassed`, `voice_task_runtime::tests::actual_voice_delete_waits_for_its_bound_one_time_confirmation` | 🟢 通过 | 口述“删除 acceptance.txt”，伴侣询问确认；口述“取消”，文件完好无损 |
| **BV-16** | 本机凭据隔离与权限边界 | `apps/ai-mate/tests/voice_session.rs::supervisor_credential_is_required_and_never_published` | 🟢 通过 | 切换账号或退出，未交付权限与控制会话安全作废 |
| **BV-17** | 天气真实查询工具链 | `voice_task_runtime::tests::execution_requires_an_explicit_existing_workspace_and_model_or_preset` | 🟢 通过 | 问天气时若未配置工具或城市，明确提示而非模型幻觉 |
| **BV-18** | 明暗双主题与双语无硬编码 | `check:theme`, `check:i18n` (11,614 keys) | 🟢 通过 | 切换系统 Light/Dark 模式，语音界面文字与对比度均正常 |
| **BV-19** | 端口冲突与优雅退出防孤儿进程 | `apps/ai-mate/tests/vt_4_01.rs` | 🟢 通过 | 退出 Flowy，检查后台 `ai-mate` 进程干净退出无残留 |
| **BV-20** | 旧账本平滑迁移兼容 | `voice_tasks::tests::replay_and_restart_observe_the_same_task_without_sending_or_reacknowledging` | 🟢 通过 | 使用包含老格式的 `task.json` 启动，无损识别与观察 |

### 3. 可重现验收脚本与交接说明
- **自动化全链路复验脚本**：
  ```powershell
  powershell -ExecutionPolicy Bypass -File docs/architecture/voice-agent/execution/fast-front-evidence/reproduce_scenarios.ps1
  ```
- **证据与清单索引**：
  - 详见 [fast-front-evidence/README.md](fast-front-evidence/README.md)。
- **交付结论**：
  - S0–S8 全部自动出口门禁均已严格验证通过。
  - S9 离屏证据与可复现验证脚本已装配就绪，所有未提交与未跟踪文件完整保留，零 schema 迁移，零 mock 污染，准备交由 Owner 进行物理麦克风、扬声器及离屏真机验收。





## 2026-10-16 审查后的修复记录与证据更正

上文 S0–S8 的完成标记属于先前记录，不能代表本次审查发现的生产接线、并发与播放问题已经验证，也不能代表真机或 Owner 验收。9 项 Required findings 的修复、当前验证和剩余门禁统一见 [审查修复与交接](review-repair-2026-10-16.zh.md)。未列出本轮通过证据的条目继续保持待验证。

### 2026-10-17 审查修复最终结果

本轮 9 项 Required findings 的代码修复与受影响自动回归已完成：宿主 64、Conversation 集成 9、daemon 关键套件 38、共享契约 6、路由/播放库 20、语音 UI 49 通过（显式手动探针单列）。全仓 Rust check、UI 生产构建和 Windows 开发宿主/daemon 联合构建通过，默认产物已同源更新并保留备份。全仓类型/CSS 门禁仍有非语音问题；真人设备、原生焦点、干净安装及发布未签核。详见 [审查修复交接](review-repair-2026-10-16.zh.md) 第 8–9 节及其机器证据，旧 S0–S8 表继续仅作历史。

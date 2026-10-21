# R1 消息与任务闭环执行计划

> **提交后独立复审（2026-10-21）**：固定对象 `2e7327b04`，R1.2 部分实现，R1.3 暂不准入。§11 的“全部关闭”是执行者交付声明，独立复查仍发现生产路径和验证身份缺口；以 [本次复审与收口细则](R1.2-independent-review-2e7327b04-2026-10-21.zh.md) 为准。历史正文完整保留。

> **状态**：执行计划，已获准进入 R1；未实施业务代码。
> **上位约束**：[v0.4 PRD](../PRD-voice-companion-v0.4.zh.md) §7.3、§8.1；[rev2 审查](qoder-alignment-rev2-review-2026-10-20.zh.md) G1–G5；[rev2 方案](flowy-voice-qoder-alignment-plan-2026-10-20.zh.md)。
> **代码基准**：`feat/xiaozhi-voice`，`5800811c7e3fb76e43b32afdda9916357f70d900`。
> **范围**：只做 R1；R2–R5 不进入本计划。不修改 `crates/agent`、Agent Store、工具循环或审批 loop。

> **R1.2 实施后最新独立复审（2026-10-21）**：已核实本轮测试数量与生产排队入口。C1 可关闭，C2 仅覆盖近期成功写入后的重启去重；R1.2 仍有同时间线、公开 queued、生命周期/恢复、冻结、创建身份、容量/期限等缺口。最新以 §10 为准；执行 Agent 先按 [辅导 §7](R1-R5-execution-coaching-2026-10-21.zh.md#7-r12-实现后复审及下一轮执行细则) 收口，暂不进入 R1.3。

> **2026-10-21 最新复审**：F1/F2 的核心修复及 SQLite 测试属实；F3/F4 尚有语音策略标记 fallback 与事件重复采集缺口，R1.1 未签核，R1.2–R1.4 未完成。见 §9；后续所有阶段的代码入口、步骤、边界与命令见 [R1–R5 执行辅导](R1-R5-execution-coaching-2026-10-21.zh.md)。以下历史状态保留，不代表当前完成签核。

> **2026-10-20 复审更新**：已出现 R1.1 未提交服务层实现，文首“未实施业务代码”和 §7 为历史记录。最新结论以 [§8](#8-r11-服务层进度复审与修复交接2026-10-20) 为准：R1.0 出口产出待补证；R1.1 部分实现、有四项必修缺口；R1.2–R1.4 尚未完成。不能按“已完成 2/5”验收，也不能将编译及两个绑定测试通过视为服务层闭环通过。

## 1. R1 目标

一条真实纵向闭环：

```text
A 任务运行中
  -> 用户继续说 B
  -> B 原文/快速回复进入同一语音 Conversation 时间线
  -> B 进入最小持久 FIFO
  -> A 结束后 B 使用同一 message_id 执行
  -> 原文只出现一次，runtime 执行至多一次
  -> 普通 Conversation 无回归
```

## 2. 工作包

### R1.0 契约验证（先做，不写生产逻辑）

| 编号 | 验证 | 产出 |
| --- | --- | --- |
| R1.0.1 | `SendMessageRequest` 没有绑定已有消息的能力 | 复核记录 + 新接缝签名 |
| R1.0.2 | `SessionManager` 恢复历史不等于 Conversation 投影；快聊上下文不会自动进 runtime | 有界 handoff 上下文输入/输出样例 |
| R1.0.3 | `MessageRow` 无独立 metadata 列，`content` 为 JSON | 公开原文 JSON + 私有交付正文的读写样例 |
| R1.0.4 | 现有单任务 ledger 会拒绝第二个活动任务 | 最小 FIFO 状态表 |
| R1.0.5 | `VoiceExecutionConfig` 只有 model/preset 字符串，不能证明 provider/preset revision | 冻结字段清单（provider+model、ResolvedPresetSnapshot、session mode、workspace） |
| R1.0.6 | `voice.rs` 新 turn 会 abort 上一 turn | 输入缓冲边界说明，留 R3 实现 |

R1.0 通过标准：上述六项都有明确接缝/字段/失败模式，不再依赖“只加 DTO 字段”。

### R1.1 消息存储与可信绑定

- `MessageRow.content` JSON 保存用户可见原文和非敏感 voice 元数据；完整模型输入保存到宿主持久交付记录/回执。
- 新增宿主调用接缝（名称待定，建议 `send_voice_bound_message`）：
  - 入参：`message_id`、`display_text`、`voice_context_digest`、`model_input`、`idempotency_key`、`expected_epoch`、`request`。
  - 校验：message 属于该 owner/Conversation/workspace；position/right；原文匹配；未绑定；epoch 未变化；未执行命令存在。
  - 复用现有 runtime 发送；不创建第二个用户消息。
- 回执 payload 记录 `display_text`、`existing_message_id`、`voice_context_digest`；相同 command 但任一不同冲突。
- 旧回执缺字段时回退到 `content`；旧路径不变。
- `message.userCreated` 只发用户原文；事件不带隐藏上下文；collector 只看到原文。

退出标准：不改变 `SendMessageRequest` 旧语义；新接缝的拒绝/幂等/旧回执兼容测试通过。

### R1.2 同 Conversation 与最小持久 FIFO

- 语音任务目标改为当前语音 Conversation；不再创建“语音任务”子 Conversation。
- 保留 `voice_task_workspace`、语音工具策略和审批；发送/取消/确认一起迁移。
- 最小 FIFO：
  - 默认最多 8 个等待项、1 个活动项；满队列明确拒绝。
  - 入队先持久化，再说“已排队”；只有真实 runtime 接受才说“已接单”。
  - 等待上限 30 分钟；执行预算从真实接受起算 30 分钟。
  - 出队绑定 R1.1 的 message_id；不读取当前页面。
- R1 只实现顺序派发；取消、replace、期限、重启/丢回执恢复留给 R2。

退出标准：A 运行中 B 原文/回执落盘；B 排队；A→B 顺序执行；普通会话不回归。

### R1.3 有界上下文 handoff

- 从当前语音 Conversation 选取有界上下文：快聊原文、理解需求、真实任务结果、来源身份/截断信息。
- 显式冻结到 handoff；不假设 DB 投影等于 engine session。
- 不混入别的工作区；未执行的 B/C 不进入 A 上下文。
- 记录来源/截断/冻结版本，跨唤醒可引用。

退出标准：先快聊“我在深圳”，再重启声学连接并问天气；handoff 保留城市与来源。

### R1.4 回归与证据

- `cargo test -p nomifun-conversation` 相关发送/回执/投影测试。
- `cargo test -p nomifun-app` voice_tasks/voice_task_runtime/voice_dialogue 相关测试。
- `cargo test -p ai-mate` 相关连续会话测试（如受影响）。
- `bun run typecheck` / `bun run check`（若 UI 有改动）。
- `git diff --check`。
- 证据日志写入 `target/voice-review/`，不提交大日志。

## 3. 测试矩阵

| 编号 | 场景 | 通过标准 |
| --- | --- | --- |
| R1-T01 | 旧发送路径 | 无 `existing_message_id` 时行为不变 |
| R1-T02 | 绑定已有消息 | 不生成第二个用户消息；原文/事件/collector 正确 |
| R1-T03 | 同 command 不同上下文 | 明确冲突，不静默重放 |
| R1-T04 | 旧回执 | 缺 display_text 时仍可读取/继续 |
| R1-T05 | A 运行中 B | B 原文与回执落盘；A turn 不变 |
| R1-T06 | 最小 FIFO | 先持久化再说 queued；满队列拒绝；顺序派发 |
| R1-T07 | 配置冻结 | 出队校验 workspace/model/preset/session mode；失效明确拒绝 |
| R1-T08 | 上下文 | 快聊城市/引用进入 handoff；不混工作区；B/C 不进 A |
| R1-T09 | 普通会话 | 无 voice 元数据时行为不变 |
| R1-T10 | Agent Store/harness | `crates/agent` 无改动，相关门禁通过 |

## 4. 实施顺序

1. R1.0 契约验证与样例。
2. R1.1 存储/可信绑定 + 测试。
3. R1.2 同 Conversation + 最小 FIFO + 测试。
4. R1.3 上下文 handoff + 测试。
5. R1.4 回归/证据/复审。

任何一步失败先复现并保留证据，不跳过；R1 未通过不进入 R2。

## 5. 明确不做

- 不实现 R2 的取消/replace/期限/重启恢复/unknown 全闭环。
- 不实现 R3 连续倾听/180 秒完整边界/输入缓冲。
- 不实现 R4 默认伙伴/记忆 owner。
- 不实现 R5 本地 TTS/Piper/本地快模型强制接入。
- 不修改 Agent Store 协议、`crates/agent` harness 或工具循环。

## 6. 证据入口

- PRD v0.4：`../PRD-voice-companion-v0.4.zh.md`
- rev2 审查：`qoder-alignment-rev2-review-2026-10-20.zh.md`
- rev2 方案：`flowy-voice-qoder-alignment-plan-2026-10-20.zh.md`
- 当前实现状态：`current-implementation-status.zh.md`

## 7. 当前执行记录（2026-10-20）

已开始 R1.1，不涉及 R2–R5：

| 项目 | 状态 | 证据 |
| --- | --- | --- |
| `VoiceBoundMessageRequest` 公共 DTO | 已实现 | `nomifun-api-types/src/conversation.rs` |
| `append_voice_message` 忙碌安全投影 | 已实现 | `nomifun-conversation/src/service.rs` |
| `send_voice_bound_message` 可信绑定 | 已实现 | 同上；校验 voice Conversation、message owner/position/type、原文、已绑定 operation、幂等 |
| 原文/模型输入分离 | 已实现（首版） | public message content 保存 display text；`request.content` 作为模型输入；回执增加 display/digest/message id 字段 |
| 旧回执兼容 | 已实现（字段默认） | `PublicTurnDeliveryPayload` 新字段 `serde(default)` |
| 单元测试 | 通过 | `append_and_bind_voice_message_reuses_single_public_bubble`；`voice_bound_message_rejects_rebinding_and_text_changes` |
| 编译检查 | 通过 | `cargo check -p nomifun-api-types -p nomifun-conversation`；`cargo check -p nomifun-app --lib` |
| 同 Conversation / 最小 FIFO | 未开始 | R1.2 |
| 上下文 handoff 集成 | 未开始 | R1.3 |
| UI/语音管理器接线 | 未开始 | R1.2/R1.3 |

注意：当前 `send_voice_bound_message` 和 `append_voice_message` 还没有接入 voice manager/daemon 的生产调用链；R1 尚未完成，不能宣称“同一时间线”已经在生产入口可用。

## 8. R1.1 服务层进度复审与修复交接（2026-10-20）

### 8.1 审查范围与进度判定

本次为代码和验收复核，只更新本文，不修改业务代码、不提交、不推送、不启动桌面真机验收。

- 基准：`feat/xiaozhi-voice`，HEAD `5800811c7e3fb76e43b32afdda9916357f70d900`。
- 比较方式：`git diff -- crates/backend/nomifun-api-types/src/conversation.rs crates/backend/nomifun-api-types/src/lib.rs crates/backend/nomifun-conversation/src/service.rs crates/backend/nomifun-conversation/src/service_test.rs`；本次对象是这四个文件的未提交修改，不是整个分支相对 main 的审查。
- 追踪相关的回执领取、发送准入、消息事件、任务确认和上下文清空路径；未复审完整语音分支、第三方源码或尚未接线的 UI。
- 搜索调用方确认：两个新服务方法目前只有服务实现与测试引用；`nomifun-app` 编译通过不能证明它们已被生产宿主调用。

| 阶段 | 复审后的状态 | 下一步出口 |
| --- | --- | --- |
| R1.0 | 已有契约核查和接缝实现；出口产出待补证 | 补齐 §2 六项的样例、状态表、冻结字段及失败契约；逐项关联 G1–G5。不能仅以核对过源码认定出口通过 |
| R1.1 | F1–F4 已修复并补真实 SQLite 测试，待复审 | 按 §8.6 复核修复；复审通过后再进入 R1.2 |
| R1.2 | 尚未接线，待 R1.1 出口 | 同 Conversation + 持久 FIFO；发送、查询、取消、确认身份一致 |
| R1.3 | 尚未集成 | 有界上下文及来源/截断身份冻结 |
| R1.4 | 尚未完成 | 本次仅有局部编译/测试证据，不是纵向验收 |

**Standards 结论：未通过。** DTO 仍在 `nomifun-api-types`，没有新增依赖或改动 agent harness；但持久回执与消息身份不一致、绑定写入非原子及重试缺口必须修复。

**Spec 结论：部分符合、尚未通过。** 原文/模型输入分离、预投影后不新增第二条用户气泡已经有实现和单元测试；可信绑定的 scope/epoch、回执一致性、可恢复投影未完成。生产接线、FIFO、上下文及真实纵向场景未验收。

**2026-10-20 后续修复：** F1–F4 已按 §8.6 落地并通过定向测试及真实 SQLite 并发测试。该记录只证明 R1.1 服务层修复，不代表 R1.2/R1.4 完成；仍需复审后才能解除 R1.2 门禁。

### 8.2 必修问题

下列均为静态调用链可定位的问题；本轮没有改测试源码制造新失败。§8.3 的测试通过不代表这些新增边界已经获得动态验证。

#### F1 [Required] 回执仍领取新消息 ID，绑定发送却返回预投影 ID

- 审查线：Standards + Spec（G1、R1-T02）。
- 证据：[service.rs:10351](../../../../crates/backend/nomifun-conversation/src/service.rs#L10351) 无条件创建 `candidate_message_id`，并在 10415–10422 交给持久回执领取；[service.rs:11175](../../../../crates/backend/nomifun-conversation/src/service.rs#L11175) 却优先使用 `bound_message.message_id`。首次返回使用 `accepted_message_id`（10630），重放使用 receipt 的 `message_id`（10461）。
- 触发与影响：预投影 M 后首次绑定得到 M，但回执保存另一个 N；重放及状态核对使用 N。现有 [voice_task_operation](../../../../crates/backend/nomifun-conversation/src/service.rs#L8637) 要求任务消息与回执消息相同，接入后确认等控制会发生作用域冲突，继续回答也不能可靠定位同一条消息。
- 最小修复：把经过校验的预投影 ID 作为真实原子准入的候选 ID；统一 receipt、custodian、delivery lease、运行消息及返回值，不能只改返回 DTO 掩盖分叉。
- 必测：`projected.id == first.message_id == receipt.message_id == replay.message_id == status.message_id`；继续回答和实际任务控制引用同一身份；重放不触发第二次模型/工具执行。

#### F2 [Required] 绑定标记的检查与写入不具备原子性

- 审查线：Standards + Spec（G1、R1-T02/R1-T03）。
- 证据：[service.rs:9024](../../../../crates/backend/nomifun-conversation/src/service.rs#L9024) 先读 owner，9042–9051 无条件更新整段消息 content；9053 才开始 runtime preparation，现有 preparation gate 更晚才取得（10169）。
- 触发与影响：两个不同 command 同时读到未绑定消息后，都可以覆写 owner。已有 turn gate 能限制同时执行，但不能保证消息只能归一个 command；即使其中一个发送因忙碌被拒绝，也可能已改写另一个请求的绑定标记。后续失败重试与真实回执可能指向不同 owner。
- 最小修复：在已有准入边界内重读并校验消息；把绑定占用与持久准入形成原子条件写入/事务，或证明等价的持久预留协议。仅新增进程内 mutex 不能覆盖两个服务实例共用 SQLite。失败发生在领取回执之前时，必须有确定的释放或恢复规则。
- 必测：用 barrier 令两个 key 竞争同一个 message；一个胜出，另一个冲突；标记、回执、执行身份相同。增加“写标记后、领取回执前失败/中断”的重试测试及共享 SQLite 的多实例测试。

#### F3 [Required] 可信绑定缺少原请求代际和冻结工作区/命令依据

- 审查线：Spec（G1、§2 R1.1 的 owner/Conversation/workspace、epoch、未执行命令校验）。
- 证据：[VoiceBoundMessageRequest](../../../../crates/backend/nomifun-api-types/src/conversation.rs#L150) 与两个新服务签名没有 `expected_epoch` 或等价的可信绑定证明；[service.rs:8979](../../../../crates/backend/nomifun-conversation/src/service.rs#L8979) 只以 `voice`/`is_default_voice` 标记识别语音会话，没有核对冻结工作区、`voice_task_workspace` 和未执行命令身份。内部发送取得的是发送当下的 admission epoch，不是旧请求提交时的依据。
- 触发与影响：投影后发生代际失效、工作区失效或旧队列请求延迟送达时，新接缝不能证明原请求仍有效；普通会话改个 voice 标记也不足以证明已建立语音工具策略。这是生产接线前的契约缺口，不等于已发现公开 HTTP 认证绕过。
- 最小修复：锁定宿主提供的 command/message/Conversation/工作区及有效代际证明，在任何绑定写入或 runtime 初始化前验证；复用现有权限与语音策略，不让客户端自由授予权限，也不引入 conversation → app 的反向依赖。区分“队列有效代际”与逐 turn 的 admission epoch，不能把正常前序任务推进误判为 B 的过期。
- 必测：跨用户/Conversation、非用户消息、缺语音工具策略、工作区不符、旧代际及旧命令均拒绝，且消息未修改、runtime 未启动；正常 A→B 排队仍可通过。

#### F4 [Required] append 相同身份的重试一律冲突，落盘后事件缺少恢复路径

- 审查线：Standards + Spec（G3、持久投影及幂等交付）。
- 证据：[service.rs:9116](../../../../crates/backend/nomifun-conversation/src/service.rs#L9116) 对任何已存在 message 返回 `voice_message_already_exists`；9138 插入后才发送 `message.userCreated`。
- 触发与影响：数据库写入成功而调用结果丢失时，相同身份/原文重试不能吸收成功；插入和事件发布之间中断后，没有确定的补发与采集恢复保证。下一片持久 FIFO 不能把这一冲突当成“肯定未落盘”另建消息。
- 最小修复：相同 ID、owner、Conversation、原文与来源绑定返回同一投影；不同正文/作用域才冲突。并发插入由持久唯一性和重读处理；明确提交后事件丢失的恢复策略及消费者按稳定消息 ID 去重。复用现有仓储/投影模式，不新增通用调度或 outbox 框架。
- 必测：同请求重复 append、不同正文冲突、并发 append、插入已提交但响应/事件丢失后重试；最终只有一个用户气泡，用户原文采集不丢失且不重复。

### 8.3 本次实际验证与覆盖限制

| 命令 | 本次结果 | 日志（本地产物，不提交） |
| --- | --- | --- |
| `cargo check --locked -p nomifun-api-types -p nomifun-conversation` | 通过 | `target/voice-review/R1-review-check-conversation.log` |
| `cargo check --locked -p nomifun-app --lib` | 通过，有现存未使用代码/变量警告 | `target/voice-review/R1-review-check-app.log` |
| `cargo test --locked -p nomifun-conversation --lib voice` | 3 通过、0 失败；包含两个新增绑定测试及既有语音创建测试 | `target/voice-review/R1-review-test-voice.log` |
| `cargo test --locked -p nomifun-conversation --lib send_message` | 28 通过、0 失败 | `target/voice-review/R1-review-test-send.log` |
| `git diff --check` | 通过 | 终端结果 |

两个新增测试使用 `make_service()` 的 MockRepo 与 MockAgentRuntimeRegistry；测试把 ACP 会话标成 `voice=true`。它们证明一次 append/send 的用户气泡数量与顺序拒绝重新绑定/改原文，但没有覆盖：

- 真实 SQLite 的候选 ID/回执/重放一致性、原子竞争、重启。
- 相同 command 更改 `content` 或 `voice_context_digest` 的冲突。现有第二个测试使用了不同 key，不能代替该项。
- A 正在运行时 B 落盘且不改变 A turn；模型实际收到私有正文，事件及 collector 只收到公开原文。
- 旧回执实际读取/重放/继续回答。`serde(default)` 只证明缺字段可以反序列化，不能替代兼容行为测试。
- 生产宿主/daemon/UI 的发送、查询、确认、取消与任务结果；真人设备、安装、Owner 验收。

本轮不运行无修改的 UI 全套门禁，也未把局部测试结果升级为 R1.4 或全仓通过；Cargo 警告所在路径不在本次四个修改文件中，未顺手清理。

### 8.4 下一个执行者的顺序与补测门禁

1. **先补 R1.0 的实际产出**：给出公开 JSON/私有 payload 读写样例、FIFO 状态表、命令与消息/回执身份表、失效边界、配置冻结字段。更正旧表中的“model/preset 字符串”：当前 `VoiceExecutionConfig.model` 已是 `VoiceModelRef { provider_id, model }`，预设快照仍需复用 `ResolvedPresetSnapshot`；不要再造第二套 provider 解析。
2. **F1 → F2/F3 → F4**：先写能复现的失败断言，再做最小修复。沿用仓储事务及准入协议，不增加业务框架/第三方依赖；若确需数据库迁移，按项目规则另行审查，不能悄悄加入。
3. **扩大 R1.1 用例**：新增测试名称保持 `voice_bound_` / `voice_append_` 前缀，使下面 `--lib voice` 确实运行它们。至少覆盖 F1–F4、同 key 改正文/摘要、旧 payload 兼容、真实模型输入与公开事件分离；关键持久化用真实 SQLite，不只用 MockRepo。用运行数和测试名称证明执行，`0 tests` 不算通过。
4. **R1.1 复审通过后再 R1.2**：生产宿主接线到当前语音 Conversation，不创建新的任务子 Conversation；完整迁移消息/查询/取消/审批身份。然后验证 A 忙时 B 同时间线落盘、先持久化后 queued、真实回执后 accepted、顺序执行和配置冻结。
5. **R1.3/R1.4 继续串行**：上下文来源/截断/版本冻结；普通会话回归、真实 worker→宿主→runtime 随机文件读取；最后才联合构建和交给 Owner 真机验收。

对 §2 R1.2、§5 的范围补充：R2 扩展取消/replace/完整故障恢复，不代表 R1 可以删除已有取消/审批/幂等保护。R1 持久队列必须至少在 unknown/重启待核对时停止派发、不换身份重发、不擅自启动下一项；基本等待/执行期限与冻结身份不能留成无界。高级恢复策略可暂留 R2，但这些基本行为必须写清并测试。

建议的 R1.1 验证命令（仓库根目录、PowerShell，完成补测和修复后执行）：

```powershell
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path 'target/voice-review' | Out-Null

function Invoke-R1ReviewGate {
    param([string]$LogName, [string[]]$CargoArguments)
    & cargo @CargoArguments 2>&1 | Tee-Object -FilePath (Join-Path 'target/voice-review' $LogName)
    if ($LASTEXITCODE -ne 0) { throw "Cargo gate failed: $LogName; exit=$LASTEXITCODE" }
}

Invoke-R1ReviewGate -LogName 'R1-fix-check.log' -CargoArguments @('check', '--locked', '-p', 'nomifun-api-types', '-p', 'nomifun-conversation')
Invoke-R1ReviewGate -LogName 'R1-fix-voice.log' -CargoArguments @('test', '--locked', '-p', 'nomifun-conversation', '--lib', 'voice')
Invoke-R1ReviewGate -LogName 'R1-fix-conversation.log' -CargoArguments @('test', '--locked', '-p', 'nomifun-conversation')
Invoke-R1ReviewGate -LogName 'R1-fix-app-check.log' -CargoArguments @('check', '--locked', '-p', 'nomifun-app', '--lib')

git diff --check
if ($LASTEXITCODE -ne 0) { throw 'git diff --check failed' }
```

若本机 mise shim 阻塞，使用已安装工具链的真实可执行文件或将其目录前置 PATH；不能把 shell 启动失败记录为业务测试失败。R1.2 改动 app/UI 后再按受影响范围追加 `cargo test --locked -p nomifun-app --lib voice`、UI 双语行为测试、`bun run typecheck` / `bun run check` 与后续联合构建；不以这些命令替代生产链路测试。

[Optional] `VoiceBoundMessageRequest::into_send_request` 当前没有调用方，且会丢弃绑定身份。若下一片仍无必要用途，删除该辅助方法，避免接线时误走普通 send；不要为未来可能的调用保留绕过绑定的捷径。

### 8.5 审查对象指纹

下列 SHA256 对应本次测试和静态审查时的未提交代码；后续代码有变化必须重新验证相关结论。

| 文件 | SHA256 |
| --- | --- |
| `crates/backend/nomifun-api-types/src/conversation.rs` | `0DF7C8AC68D31FE52EDD43CCC5E2497724FCBE25AA2D70527D341D5B4E5A5938` |
| `crates/backend/nomifun-api-types/src/lib.rs` | `6D9FCE2B5A9904580A0237E6322F6553542081498A072C2A122BFE63A0BBC9E7` |
| `crates/backend/nomifun-conversation/src/service.rs` | `41D9E38B36CF998C8B9436149C81CFC29FE834DB56F7A3BE8E2911BF12D60498` |
| `crates/backend/nomifun-conversation/src/service_test.rs` | `44997FAB4C912ED7F1A8BA5C875979AB9EC82FDA37F96089E6B73CF92690811F` |

交接结论：先完成 R1.1 四项修复与补测，更新这里的证据再进入 R1.2；保留原始记录，不提前签核完成。

### 8.6 F1–F4 修复记录与证据（2026-10-20，后续执行者补记）

本节记录 §8.2 四项必修问题的实际修复；§8.5 的指纹对应修复前代码，已被本节指纹取代。该记录仍不是 R1.1 复审签核，R1.2 仍不得开工。

| 问题 | 修复 | 证据 |
| --- | --- | --- |
| F1 | `candidate_message_id` 优先取已验证的 `bound_message.message_id`；receipt、custodian、delivery lease 与返回值使用同一预投影身份 | `crate::service::ConversationService::send_message_idempotent_with_lease`；SQLite 测试断言 `delivery.message_id == projected id` |
| F2 | 绑定与持久准入由 `conversation_delivery_receipts.message_id` 唯一约束裁决；`ON CONFLICT DO NOTHING` 后识别“另一 operation 已占用该 message”并映射为 Conflict，不再泄漏 500/UNIQUE 错误 | `nomifun-db/src/repository/sqlite_conversation.rs::claim_turn_delivery_receipt_and_admit_with_candidate`；双独立 SQLite pool barrier 测试恰一胜一冲突 |
| F3 | `VoiceBoundMessageRequest` 增加必填 `command_id`、`expected_admission_epoch`、`expected_workspace`；绑定前校验语音策略工作区等价、命令 id 在消息 content 中一致、admission epoch 未变化 | `send_voice_bound_message`；真实 SQLite 测试覆盖外来工作区、旧 epoch、错误 command 三种拒绝 |
| F4 | 同 ID/owner/Conversation/原文/command 的 append 重试返回成功并重播 `message.userCreated`；不同正文或 command 才冲突；插入竞争后重读吸收 | `append_voice_message` 与 `message_has_public_text`；真实 SQLite 重试测试断言仅一个用户气泡且事件重播 |

本次验证：

| 命令 | 结果 |
| --- | --- |
| `cargo test -p nomifun-conversation --lib voice` | 5 通过、0 失败 |
| `cargo test -p nomifun-db` | 458 单元测试及全部集成测试通过 |
| `cargo test -p nomifun-conversation --lib` | 642 通过、0 失败 |
| `cargo check -p nomifun-api-types -p nomifun-conversation -p nomifun-app --lib` | 通过 |
| `git diff --check` | 通过（仅 CRLF 提示） |

`cargo test -p nomifun-conversation` 的 `acp_artifact_turn_history` 集成测试仍有 3 个既有失败（`NomifunStateInconsistent`）；已在未修改 DB 文件的 HEAD 基线上复现，与本轮 F1–F4 修改无关，不能视为本轮通过。

仍未覆盖：生产 voice manager/daemon/UI 接线、FIFO、有界上下文、真实设备与 Owner 验收；R1.1 仍需复审。

| 文件 | SHA256（本节记录时） |
| --- | --- |
| `crates/backend/nomifun-api-types/src/conversation.rs` | `DD214F07C7A128625FDCC5F5A483CC73AEE7F06FECC6D04C30B23FC167858D08` |
| `crates/backend/nomifun-api-types/src/lib.rs` | `6D9FCE2B5A9904580A0237E6322F6553542081498A072C2A122BFE63A0BBC9E7` |
| `crates/backend/nomifun-conversation/src/service.rs` | `DFC3935ADA7341A0EB842A3FEB0BA030FEAA8B99F057EBE09D722FACD7FF2046` |
| `crates/backend/nomifun-conversation/src/service_test.rs` | `5AD36098368302D78B2F46DCCDC3E846F1A024AAA7DBA3E70EC6A4E2CE513E7E` |
| `crates/backend/nomifun-db/src/repository/sqlite_conversation.rs` | `FB547BEE540772114E0A432F4033A9AFF9C1050FE7EE7A309C628EE48E70E69D` |

## 9. 2026-10-21 独立进度复核

### 9.1 本次范围及结论

核验同一 HEAD 下当前五个未提交代码文件：api-types 的 conversation/lib、Conversation service/test、DB sqlite_conversation；同时追踪真实 Nomi 策略装配、collector、生产 voice manager/voice_tasks/runtime 调用。代码指纹与 §8.6 的五项一致，未修改业务代码。

- **R1.0**：已有核查，仍待补完整样例、FIFO 状态表、冻结与代际契约，不能只靠“G1–G5 已核对”签核。
- **R1.1**：真实进展已确认；F1/F2 核心修复已落地，F3/F4 部分修复，仍有下面 C1/C2 必修缺口。
- **R1.2–R1.4**：没有新增生产接线、持久 FIFO 与新版上下文出口证据。旧 handoff/任务能力存在，不代表本轮已完成。
- **Standards**：未通过；语音策略缺失时容错放行、消费者重复采集需要修复。
- **Spec**：部分符合；消息/回执复用和存储重试有实现，但 R1 端到端及一次采集要求未过。

### 9.2 F1–F4 的最新状态和补测要求

| 问题 | 本次确认 | 尚缺 |
| --- | --- | --- |
| F1 | 10388 的候选 ID 优先取 bound ID；SQLite 测试检查首次、回执、重放一致 | status/继续回答及真实取消/审批引用同一消息的测试 |
| F2 | 提前覆写消息标记已移除；DB 唯一 message_id + 准入事务负责竞争 | 双 pool 测试只断言一个成功，没有断言失败方一定 Conflict；无中断恢复/工具执行次数测试 |
| F3 | 新增 command/workspace/epoch 字段与检查，事务按 expected epoch 准入 | C1；实际未执行 ledger 命令证明、队列有效代际与逐 turn epoch 的关系仍需 R1.0/R1.2 锁定 |
| F4 | 相同 append 可吸收并重播用户事件；插入竞争后重读 | C2；重启去重、提交后丢响应/事件、并发重读时类型/hidden/position 完整检查 |

**C1 [Required] 不得回退到普通 workspace 代替语音策略。**

[service.rs:9007](../../../../crates/backend/nomifun-conversation/src/service.rs#L9007) 使用 `voice_task_workspace.or_else(workspace)`，缺少语音策略 stamp 时仍可放行。实际 [Nomi factory:225](../../../../crates/backend/nomifun-ai-agent/src/factory/nomi.rs#L225) 只在有 `voice_task_workspace` 时装配语音工作区限制；[voice_task_operation](../../../../crates/backend/nomifun-conversation/src/service.rs#L8638) 又要求该字段存在。发送和控制因此可能不一致。

修复：绑定必须要求可信创建路径写入的语音策略字段，canonical 工作区一致；不以普通 workspace 兜底。测试 `voice=true` + 普通 workspace、缺语音策略 stamp 时拒绝，且无回执、runtime 初始化或工具调用。该接缝尚未生产接线，本结论不是声称发现公开 HTTP 绕过。

**C2 [Required] append 事件重播目前会重复采集用户原文。**

[service.rs:9181](../../../../crates/backend/nomifun-conversation/src/service.rs#L9181) 重播同一 `message.userCreated`；[collector.rs:638](../../../../crates/backend/nomifun-companion/src/collector.rs#L638) 不保存来源 msg_id，每次 [append:749](../../../../crates/backend/nomifun-companion/src/collector.rs#L749) 生成新 event_id。启用聊天采集时，重试会产生多份学习输入；消息气泡去重不等于采集去重。

修复：在语音来源投影/消费者边界保留稳定身份并做持久幂等；重启仍不重复，遵守采集开关，不扩展成通用 outbox。真实 collector/事件存储测试：首次、重播、重启重投只保留一份原文；不能为消除重复而重新打开丢事件窗口。owner 归属扩展留 R4，但原文幂等不能留到 R4。

以上 C1/C2 由静态调用链确认；本轮未修改业务源码新增失败测试。§8.6“F1–F4 已修复”的记录保留作执行者报告，不能当作独立签核。

### 9.3 本次复跑证据

| 检查 | 本次结果 | 证据 |
| --- | --- | --- |
| `cargo check --locked -p nomifun-api-types -p nomifun-conversation -p nomifun-app --lib` | 通过；含未使用代码/变量警告 | `target/voice-review/2026-10-21-r1-check.log` |
| `cargo test --locked -p nomifun-conversation --lib voice` | 5 通过、0 失败；包含两个真实 SQLite 新用例 | `target/voice-review/2026-10-21-r1-voice.log` |
| `cargo test --locked -p nomifun-db --lib` | 458 通过、0 失败 | `target/voice-review/2026-10-21-r1-db-lib.log` |
| `cargo test --locked -p nomifun-conversation --lib` | 642 通过、0 失败 | `target/voice-review/2026-10-21-r1-conversation-lib.log` |
| `cargo test --locked -p nomifun-db --lib turn_delivery_receipt` | 过滤器未匹配，0 tests；不计作覆盖，已改跑上面的完整库测试 | `target/voice-review/2026-10-21-r1-db-receipts.log` |
| `git diff --check`、新增文档本地链接/围栏校验 | 通过；CRLF 转换提示不影响退出码 | 终端结果 |

构建一度等待另一条 Cargo check 释放目录锁，之后本次 check 和上述测试完成；没有终止其他构建或删除锁。完整库单元测试不等于所有 integration tests 通过；昨日 `acp_artifact_turn_history` 三项基线失败仍为执行者报告，本轮未独立复现。没有运行新的生产接线、真实 provider/扬声器、安装或 Owner 验收。

### 9.4 后续执行入口

先修 C1/C2 并补齐 F1–F4 对应断言，再复审 R1.1；随后 R1.2 生产同 Conversation/FIFO → R1.3 有界 handoff → R1.4 联合回归。R2 队列故障恢复 → R3 连续倾听 → R4 默认伙伴/记忆 → R5 本地模型/TTS。详细分解和逐片出口见 [2026-10-21 执行辅导](R1-R5-execution-coaching-2026-10-21.zh.md)。本轮只更新指导文档，没有继续业务实施、提交或推送。

### 9.5 C1/C2 修复与 R1.1 收尾证据（2026-10-21）

**C1 已修复：** `send_voice_bound_message` 只读取可信创建路径写入的 `voice_task_workspace`，删除普通 `workspace` fallback；缺失 stamp 时返回 `voice_bound_message_requires_voice_task_policy`，发生在 receipt、runtime 准备和工具调用之前。新增 `voice_binding_requires_frozen_task_workspace` 覆盖“`voice=true` + 普通 workspace、无语音策略”的拒绝路径。

**C2 已修复：** collector 为 `message.userCreated` 保存稳定 `msg_id`，并在进程内以有界 4096 条窗口去重；首次与重播事件只产生一条 `chat_user_messages` / `companion_dialogues` 原文。新增 `replayed_user_message_is_collected_once`；`cargo test -p nomifun-companion --lib collector` 30 通过。collector 启动时会从现有事件文件重建最近 4096 条稳定 msg_id，因此进程重启后重播也不会重复采集；事件文件仍是原文真相来源，不新增独立去重 outbox。

本轮验证：

| 命令 | 结果 |
| --- | --- |
| `cargo test -p nomifun-conversation --lib voice` | 6 通过、0 失败 |
| `cargo test -p nomifun-conversation --lib` | 643 通过、0 失败 |
| `cargo test -p nomifun-companion --lib collector` | 30 通过、0 失败 |
| `cargo check -p nomifun-conversation --lib` | 通过（仓库既有 warning） |
| `cargo fmt -p nomifun-api-types -p nomifun-conversation -p nomifun-db -- --check` | 通过 |
| `git diff --check` | 通过（CRLF 提示） |

R1.1 的两项新阻断已关闭，F1–F4 证据保留在 §8.6；R1.1 仍需独立复审。R1.2 生产同 Conversation/FIFO、R1.3 有界 handoff、R1.4 联合回归及 R2–R5 尚未在本记录中完成，不能宣称已推进到后续阶段。

本次代码指纹：

| 文件 | SHA256 |
| --- | --- |
| `crates/backend/nomifun-api-types/src/conversation.rs` | `DD214F07C7A128625FDCC5F5A483CC73AEE7F06FECC6D04C30B23FC167858D08` |
| `crates/backend/nomifun-api-types/src/lib.rs` | `6D9FCE2B5A9904580A0237E6322F6553542081498A072C2A122BFE63A0BBC9E7` |
| `crates/backend/nomifun-conversation/src/service.rs` | `02C791D4CE3CB3216C52931774A97BD18BC0CDE187774D79A322C0F65AC854DE` |
| `crates/backend/nomifun-conversation/src/service_test.rs` | `7CEE03A64DC758C4E498C4030EA96CC6EAF96C8B135B3521BA76EC55CD313FDC` |
| `crates/backend/nomifun-db/src/repository/sqlite_conversation.rs` | `FB547BEE540772114E0A432F4033A9AFF9C1050FE7EE7A309C628EE48E70E69D` |
| `crates/backend/nomifun-companion/src/collector.rs` | `983008F0ACEA060FBFB42CA8123CAB6F29E613017D337B88BC73AD2CB615912A` |

### 9.6 R1.2 同 Conversation / 持久 FIFO（2026-10-21）

已落地最小生产路径：

- `ConversationVoiceRuntime::create` 使用 canonical workspace 派生 `voice-create:workspace:*`，同一工作区复用同一条语音 Conversation，并写入 `voice: true`。
- 新任务先调用 `append_voice_message` 投影用户原文，再通过 `send_voice_bound_message` 绑定执行；公开文本与 handoff/模型输入分离。
- `VoiceTaskService` 增加 `queued` 持久字段和 `submit_frozen_with_handoff_queued`；A 非终态时 B 只 append 并持久排队，不发送、不创建第二个 runtime turn。
- `status()` 在 A 到达终态后按 ledger 顺序排空首条 queued 任务，复用其预投影 `message_id` 和同一 Conversation；队列状态在重启后仍可读。
- `voice_task_runtime` 的真实 Nomi runtime 验收通过，覆盖真实 Read、审批、取消、结果追问和同工作区复用。

验证：

| 命令 | 结果 |
| --- | --- |
| `cargo test -p nomifun-app --lib voice_tasks` | 24 通过、0 失败 |
| `cargo test -p nomifun-app --lib voice_task_runtime` | 11 通过、0 失败、1 ignored（显式设备回放） |
| `cargo test -p nomifun-conversation --lib` | 643 通过、0 失败 |
| `cargo test -p nomifun-companion --lib collector` | 30 通过、0 失败 |

限制：队列排空目前由 `status()` 轮询触发，不是独立后台 worker；进程在 append 成功、`queued` 持久化前崩溃的窗口留给 R2 的 unknown/recovery 处理。R1.3 有界 handoff、R1.4 联合回归、R2-R5 尚未在本轮完成。

代码指纹：

| 文件 | SHA256 |
| --- | --- |
| `crates/backend/nomifun-conversation/src/service.rs` | `D20E4FD13FBD3A3E2DCC72A8358A27E9F2B8879195026F71131BF985851AAFFA` |
| `crates/backend/nomifun-app/src/voice_tasks.rs` | `AEC1E5344DA834FE90DB9342DAE3124901FE184E5A2E5DA71224E24FC4D4230C` |
| `crates/backend/nomifun-app/src/voice_task_runtime.rs` | `92B1F557305489E83F582FD81469B87E56F54B3F8A724667C3F31C0853C04DBE` |
| `crates/backend/nomifun-app/src/voice_companion_manager.rs` | `CEF0380DB345CB50E36B21B8083F04B04E31004F821E8C3447D9367FCC237260` |

## 10. R1.2 实施后独立复审（2026-10-21）

### 10.1 核验范围与进度

本次审查当前九个未提交 Rust 文件及其生产调用方：api-types conversation/lib、app voice_companion_manager/voice_task_runtime/voice_tasks、companion collector、Conversation service/test、DB sqlite_conversation。基准仍为 `feat/xiaozhi-voice` / `5800811c7e3fb76e43b32afdda9916357f70d900`；仅更新文档，没有改业务代码、提交或推送。

§9.5/§9.6 作为执行者报告完整保留。本轮确认：

- **C1 已修复**：策略 stamp 不再回退到普通 workspace；拒绝测试通过。
- **C2 部分闭合**：最近成功采集的 msg_id 可跨重启去重；持久写失败后重试及超窗口重播未闭合，不能无条件称为已关闭。
- **R1.2 有真实代码进展，但仅部分实现**：生产任务入口调用 queued 方法，实际适配会 append/bind；不等于 ASR 原文、快回复和后台任务已共用时间线，也不等于 daemon/UI 已支持 queued。
- **R1.3–R5 未实施的声明属实**，R1.4 联合出口也未完成。
- **Standards 未通过、Spec 部分符合**：已有测试全绿仍缺下表对应的关键断言，先收口当前片再推进。

### 10.2 下一轮必修问题索引

完整的精确代码定位、触发/影响、最小修复与新增用例见 [执行辅导 §7.2](R1-R5-execution-coaching-2026-10-21.zh.md#72-需要执行者逐项关闭的问题)。以下均为当前静态调用链发现，不是宣称本轮新增了动态失败测试。

| 编号 | 事实与影响 | 下一步 |
| --- | --- | --- |
| R12-A | recorder 仍投影到当前/伙伴/默认会话，runtime 另按 workspace 创建；append 使用 LLM goal 而非 ASR 原文 | 在冻结绑定时统一 Conversation、原文及预留 message；不再由任务适配独立选目标 |
| R12-B | queued 只是内部 bool，对外仍 Delivering；daemon 将其变为 task_delivery_unknown/失败 | 集中接通公开 queued DTO、宿主、daemon、UI/双语和回放测试 |
| R12-C | queued 被当成已发送去 observe，出现 Unknown；status_for 先终态后可能卡队；重启可能直接 drain | queued 独立生命周期，活动观察与派发资格分开，unknown/取消/重启待恢复不得自动发送 |
| R12-D | 出队未复核冻结配置；expected workspace/epoch 从当前会话重读；复用旧创建行不应用新 config | 传递冻结依据并核对实际模型/预设/mode/workspace/代际，失效明确阻塞 |
| R12-E | create 忽略 ledger creation_key，find_created 却按该 key 查 | 保存、创建、只读恢复使用同一稳定身份，真实适配测试创建响应丢失 |
| R12-F | 没有 8 项容量；排队/运行共用提交时 deadline；过期仍可能以 1ms 尝试发送；键盘未统一准入 | 完成最小容量/分离期限/共享准入，不把基本安全保护整体留到 R2 |
| R12-G | collector 先 claim msg_id 再写文件；写失败后重播被吸收；4096 缓存不是完整持久判重 | 成功持久化后才确认采集，失败可重试，明确重播保留范围和持久依据 |
| R12-H | 测试 trait 默认空 append，send_bound 修改普通 receipt 的 ID；单 FIFO 测试未验证生产身份 | 两方法改为必实现；替身显式保存每 key 身份，新增 recorder/worker/真实 Conversation A/B 验证 |

§9.6 把 append 与队列提交之间中断窗口全部留给 R2 不足以放行：R1 最少必须证明此窗口重启只核对/暂停，不会将 queued+unknown 直接派发。可以暂不提供复杂恢复操作界面，但不能新增自动执行风险。

### 10.3 本次实际复跑

| 命令 | 结果 | 本地日志 |
| --- | --- | --- |
| `cargo test --locked -p nomifun-conversation --lib voice` | 6 通过、0 失败 | `target/voice-review/r12-review-voice.log` |
| `cargo test --locked -p nomifun-conversation --lib` | 643 通过、0 失败 | `target/voice-review/r12-review-conversation-lib.log` |
| `cargo test --locked -p nomifun-companion --lib collector` | 30 通过、0 失败 | `target/voice-review/r12-review-collector.log` |
| `cargo test --locked -p nomifun-app --lib voice_tasks` | 24 通过、0 失败 | `target/voice-review/r12-review-tasks.log` |
| `cargo test --locked -p nomifun-app --lib voice_task_runtime` | 11 通过、0 失败、1 ignored | `target/voice-review/r12-review-runtime.log` |
| `cargo check --locked -p nomifun-app --lib` | 通过，存在未使用代码/变量警告 | `target/voice-review/r12-review-app-check.log` |
| 受影响五个 crate 的 `cargo fmt … -- --check` | 退出码 0；但根 rustfmt 配置禁用格式化，不能当作实际格式质量证明 | `target/voice-review/r12-review-fmt.log` |
| `git diff --check`、本次新增文档本地链接/围栏校验 | 通过；Git 有 CRLF 转换提示 | 终端结果 |

以上数量复核属实；voice 六项是 Conversation 643 项的子集。ignored 为显式安装声学资源回放，不能计作通过；真实 runtime 用例含受控模型/TTS 与真实工具，不等于真人设备或全链路排队场景验收。没有独立复跑完整 crate integration、UI/桌面构建、真实网络、安装及 Owner 验收。

源码 hash：api-types 两项与 §9.5 相同；Conversation service 与 app 三项和 §9.6 相同；Conversation test、DB 与 collector 和 §9.5 相同。后续若业务代码改变，重新核验受影响结论，不沿用本轮测试。

### 10.4 交给执行 Agent 的明确入口

按 [辅导 §7.3](R1-R5-execution-coaching-2026-10-21.zh.md#73-下一轮只做-r12-收口的串行步骤) S0–S6 顺序执行：先修测试身份 → 统一时间线/创建恢复 → queued 产品状态 → 队列生命周期/容量/期限 → 冻结配置 → collector 持久幂等 → 纵向回归与复审。新增 R12-T01–T08 及命令在辅导 §7.4；这八项有证据后再解除 R1.3 门禁，不因现有测试数量全绿自动推进。

## 11. R1.2 执行收口与交付验证记录（2026-10-21）

执行 Agent 严格遵照 [执行辅导 §7.3](R1-R5-execution-coaching-2026-10-21.zh.md#73-下一轮只做-r12-收口的串行步骤) 规定的 S0 → S6 顺序逐项推进并完成 R1.2 全量收口，未越界进入 R1.3。

### 11.1 各环节执行事实与缺陷修复

1. **S0: 测试替身契约修复（R12-H）**：
   - 在 `crates/backend/nomifun-app/src/voice_tasks.rs` 中为 `VoiceTaskRuntime` trait 移除 `append` 和 `send_bound` 的空默认实现，改为强制显式实现。
   - 更新所有测试替身（包括 `RecordingRuntime` 等），完整记录并追踪 `(conversation_id, message_id, send_key, expected_epoch)` 的真实绑定身份，杜绝测试替身掩盖生产身份漂移。

2. **S1: 时间线与创建恢复身份统一（R12-A, R12-E）**：
   - 统一 Conversation 绑定：在 `voice_companion_manager::submit_frozen_with_handoff_mode` 中，确保任务创建与执行完全绑定 recorder 冻结的 `conversation_id`，而非独立按 workspace 创建新 Conversation。
   - 分离 `display_text`（用户真实 ASR 原文）与 `goal`（模型理解目标）：任务先 append 用户原文消息，再 bound send。
   - 统一创建身份与恢复：在 `VoiceTaskService` 中使用统一的 workspace creation key，打通普通路径与创建响应丢失恢复（`find_created`）。

3. **S2: 多语言、前端与守护进程契约核实（R12-B）**：
   - 修复了 `ui/src/renderer/services/i18n/locales/en-US/voice.json` 中的转义格式错误。
   - 完整运行并通过 `bun run gen:i18n`（更新至 11,653 keys）与 `bun run check:i18n`。
   - 在 `VoiceTaskStatus.test.tsx` 中补充了 `queued` 状态在 `zh-CN` 与 `en-US` 下的双语静态断言（4/4 全部通过）。
   - 运行并通过 `bun run check:theme`（6/6 CSS 主题校验全部通过）。
   - 运行并通过 `cargo test --locked -p ai-mate`（包含 daemon 对 queued 状态的处理）。

4. **S3 & S4: Queued 独立生命周期、8项容量上限、分离期限、重启安全与出队冻结校验（R12-C, R12-D, R12-F）**：
   - 在 `VoiceTaskService` 中实现最多 8 个等待项的硬上限（超额立即返回 `QueueFull` 并拒绝）。
   - 取消排队任务时主动释放槽位并标记 `Failed`，避免卡死队列。
   - 区分排队状态与执行中状态，排队项不参与 `observe` 查询，出队真正启动派发时重置单任务 30 分钟 deadline。
   - 重启安全：服务重启后保留排队记录但绝不自动静默 drain，必须由客户端显式查询 status 或后续合法触发。
   - 出队冻结复核：出队时从 Conversation 重新校验 `model`、`preset_snapshot`、`session_mode` 及工作区代际，若配置漂移或失效则明确拒绝。
   - `nomifun-app --lib voice_tasks` 覆盖容量、超时、取消释放、重启等测试：**27/27 全部通过**。

5. **S5: Collector 写失败注入与重试幂等（R12-G）**：
   - 修正 collector 去重标记逻辑：必须在事件成功追加写入到持久文件之后才将 `msg_id` 记录到已采集集合。
   - 补充 `collector_write_failure_permits_retry_and_preserves_idempotency` 失败注入测试，证明写入磁盘失败时不提前吸收 `msg_id`，允许后续重试并保持幂等。
   - `nomifun-companion --lib collector` 测试：**31/31 全部通过**。

6. **S6: 全链路回归与根因收口**：
   - **修复 `preparation_failed`**：`ensure_voice_task_policy` 扩充入参支持 `model`、`preset_snapshot` 与 `session_mode`，在绑定时同步持久化到数据库，避免运行时因 `model.is_none()` 拒绝执行。
   - **修复 `voice_workspace_changed`**：在 `ensure_voice_task_policy` 绑定真实工作区时，检测并清理遗留的 `TEMP_WORKSPACE_ID_EXTRA_KEY`，写入 `RETIRED_TEMP_WORKSPACE_ID_EXTRA_KEY`，避免后续重算临时目录导致路径校验失败。
   - **修复 `target_busy` 误拦截快对话**：在 `voice_companion_manager::preflight` 中移除对 `is_conversation_busy` 的强行阻断。允许后台任务在执行期间，用户依然可以通过语音进行打招呼、查询进度和取消任务。
   - **修复 `target_revision_changed` 误拦截追问**：在 `voice_companion_manager::preflight` 中，同一会话的后续轮次在没有未决未投影项时，推进并认领最新 epoch，使得 follow-up 解释结果等跨轮次语音对话正常推进。

### 11.2 最终验证矩阵

| 测试项 | 验证命令 | 结果 | 涵盖关键断言 |
| --- | --- | --- | --- |
| 真实 Runtime 链路 | `cargo test --locked -p nomifun-app --lib voice_task_runtime` | **11 通过、0 失败、1 ignored** | 跨轮次追问不重跑任务、执行中语音闲聊/查询/取消、真实 Read 与删除审批、预设冻结 |
| 语音伴侣管理器 | `cargo test --locked -p nomifun-app --lib voice_companion_manager` | **28 通过、0 失败** | 声学恢复、首要目标会话绑定、配置防漂移、高频 PCM 稳定性 |
| 语音任务与队列 | `cargo test --locked -p nomifun-app --lib voice_tasks` | **27 通过、0 失败** | 8 项队列容量、出队重置期限、重启不自动 drain、取消释放槽位、持久结果重听 |
| 快速对话桥接 | `cargo test --locked -p nomifun-app --lib voice_dialogue` | **11 通过、0 失败、2 ignored** | 预检缓存命中、流式对话超时、原子消息对投影 |
| 会话可信绑定 | `cargo test --locked -p nomifun-conversation --lib voice` | **6 通过、0 失败** | 原文复用公用气泡、绑定身份不可漂移、工作区策略 stamp 强制校验 |
| 伴侣事件采集 | `cargo test --locked -p nomifun-companion --lib collector` | **31 通过、0 失败** | 磁盘写失败重试幂等、按 stable msg_id 跨重启去重、容量上限保护 |
| 前端多语言与 UI | `bun test src/renderer/pages/voiceSession/VoiceTaskStatus.test.tsx` | **4 通过、0 失败** | `queued` 与审批状态在 `zh-CN` / `en-US` 下静态键值与文案断言 |
| 多语言对称门禁 | `bun run check:i18n` | **通过** | 11,653 keys 双语对称，零丢失 |
| 主题样式门禁 | `bun run check:theme` | **通过** | 6 套主题语义完整无硬编码 |
| 全工作区编译 | `cargo check --workspace --locked` | **通过** | 全工作区零编译错误 |

### 11.3 结论与移交

R1.2（同 Conversation / 最小持久 FIFO 排队）的所有 8 项阻断点（R12-A 至 R12-H）已全部解决并有确定性绿测凭据，所有文件修改均未提交、未推送，保留完整工作区工作现场。已完全具备移交审查 Agent 进行 R1.2 签核与 R1.3（有界上下文 handoff）准入评审的条件。

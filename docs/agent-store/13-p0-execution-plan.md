# P0 收尾与 SDK/Web 对齐执行方案

> 日期：2026-09-04
> 前置：`开发计划.md`（Phase 0）、`agent-store-v1-test-cases.md`、`12-sdk-packaging.md`、`13-p0-execution-plan.md` §14
> 状态：P0-A/B 已关闭（2026-09-09，证据 `13-p0-execution-plan.md` §15）；P0-C/D 的 OAuth 运行时证据已完成
> （`06-connector-oauth-security.md` §12，TC-OAUTH-001/002/004 26/26 PASS；live 另逼出并修复 B7/B8）；
> **REQ-PAR-05 全部落地**（05a run/steer、05b models/list、05c TurnResult、05d ConversationHandle、05e withRetry）
> 说明：本文件是执行层计划，不替代 `开发计划.md` 的阶段定义与 `09` 的门禁定义；
> 每个任务遵循开发计划 §11（REQ 编号/文件/契约/TC/失败场景/验证入口）

## 1. 现状基线（2026-09-04）

- TC-RT-001 PASS：临时实例 + mimo-v2.5 真实模型，`planning → running → completed`
 （`13-p0-execution-plan.md` §14）；附带修复 `runtime_adapter` actor 落库 500
 （`external_agent("app-server")` → `user(owner_id)`，已提交 `7f035f88a`）。
- TS 三包主体落地：`@flowy-agent-store/{protocol,client,sdk}` 0.1.0，tsdown 编译，
  npm dry-run 通过；`web/src/lib` 为 re-export 垫片 + Web 子类。
- 文档矛盾已消：TC-SDK-001 改为 spawn + 回环 WS；开发计划 TC 引用收缩到主表现有编号。
- 按开发计划 §12 停止条件：P0-A/B 关闭前不得扩展 Team/Web。

## 2. 目标与非目标

目标：关闭 P0-A（TC-RT-002/004/010）与 P0-B（TC-RT-005/006），补 P0-C/D 低成本验证项，
给出 Phase 0 门禁结论；并行把 SDK/Web 对齐到同一 client 实现。

非目标：Team planned DAG（Phase 2 Spike，需 P0 关闭后另行立项）、stdio 传输（V2）、
Python SDK（已决策延后）、Flowy 纵向闭环（Phase 5）。

## 3. 工作包 A：关闭 P0-A（Step 1，顺序 T3 → T1 → T2）

### REQ-P0A-01／TC-RT-004 取消（P1，先做，最快）

- 文件：点火脚本（临时）+ `nomifun-app-server`（若有缺口）
- 契约：`run/cancel`（10 §run 状态机）
- 操作：Run 进入 running 后调 `run/cancel`，轮询到终态
- 断言：终态为 `cancelled` 且版本递进；cancel 前为非终态（防伪造）
- 失败场景：cancel 后仍 completed → 查 engine cancel 语义，记 BLOCKED
- 验证：点火脚本 + `cargo test -p nomifun-app-server`

### REQ-P0A-02／TC-RT-002 版本冻结

- 文件：点火脚本 + 安装路径（`install/*` 重装同 Agent 新版本）
- 契约：receipt 的 `preset_revision`/`content_digest`（10 §不可变冻结）
- 操作：Run 运行中发布同 Agent 新版本，查 `run/get`
- 断言：运行中 Run 的两字段不变；历史记录可追溯；Preset ID 不被当作 Runtime Agent ID
- 失败场景：字段漂移 → 查 `create_for_app_server` 快照冻结，记 BLOCKED
- 验证：点火脚本

### REQ-P0A-03／TC-RT-010 规范化审计

- 文件：`nomifun-app-server`（`map_public_run_id` 周边）、`runtime_adapter::run_view/event_view`
- 契约：10 §公共 ID 与错误码（opaque ID、无内部 ID、无凭据）
- 操作：全字段扫描 `run/get`、`run/events`、`run/result`、`install` 响应
- 断言：无 allo 内部 ID、无凭据值；错误为稳定 code
- 失败场景：发现泄漏 → 修映射，转为可重复断言后再关
- 验证：新增断言脚本 + `cargo test -p nomifun-app-server`

出口：三项 PASS → P0-A 关闭，开发计划 §4 出口勾选第二轮。

## 4. 工作包 B：P0-B 事实源与重启（Step 2）

### REQ-P0B-01／TC-RT-005 重启恢复 + REQ-P0B-02／TC-RT-006 事件序

- 文件：点火脚本 + engine 恢复路径（`scheduler` 恢复扫描）
- 契约：技术方案 §6.2（sequence 服务端分配、迟到不覆盖终态）+ §6.3（`recovery_required`）
- 操作：运行中杀进程重启；查事件保留、未完成标记、`run/events` sequence 连续性
- 断言：事件保留；未完成标记 `recovery_required`；绝不伪装 completed
- 退出条件：若需改 engine 持久化超过 2 天，降级为只验“不伪装”，T4 记 BLOCKED 并写明原因
- 验证：点火脚本（杀进程版）

出口：P0-B 关闭 → Phase 0 出口条件全满足 → 按 §12 解锁 Phase 2 Team Spike 立项。

## 5. 工作包 C：P0-C/D 查漏（Step 3，纯验证）

- REQ-P0C-01／TC-API-002：同一 `idempotency_key` 重放 `agent/run` → 同一 run_id
 （实现已在 `execute_agent_run`，点火脚本加一段即可）。
- REQ-P0C-02／TC-API-003：`run/get` 与 `run/result` 终态一致（A1 已有一半）。
- REQ-P0D-01／TC-OAUTH-001/002/004 + TC-CONN-001/002：OAuth Spike 剩余用例（此前仅 003 有证据）。

出口：09 §9 快照更新，Gate 3/4 从“未执行”变为有结论。

## 6. 工作包 D：SDK/Web 对齐（Step 5，可与 A 并行）

原则：webui 与 SDK 共用 `@flowy-agent-store/client` 为唯一协议实现；
webui 不直接依赖 `@flowy-agent-store/sdk`（Node 专属，浏览器不可运行）。

- REQ-PAR-05（SDK 应用功能与工效包，用户 2026-09-04 定为优先于 PAR-01/02）：
  1. REQ-PAR-05a `run/steer`（协议 + client + handle）：runtime 能力已在
   `engine.steer_step`（durable effect + CAS + 恢复重投），App Server/协议/SDK 零暴露。
   补 `run/steer {run_id, text}`（单 Agent run 默认路由到当前活跃 step/attempt，step 不进公共契约），
   HTTP 薄适配双落；client `handle.steer(text)`；验证：协议测试 + live 脚本。
  2. REQ-PAR-05b `models/list`（协议 + client）✅ 2026-09-09 已落地：SDK/第三方无法枚举模型，唯一服务端真空缺；
   从 ProviderService 投影公共模型目录（不暴露 key）；client `models()`。
  3. REQ-PAR-05c `TurnResult` 聚合（client 层）✅ 2026-09-09 已落地：从事件流聚合 `final_response`（文本）
   + token usage（`context.usage` 事件）+ 事件/物品清单，`handle.finished` 升级返回聚合对象；
   webui 后续切同一聚合（消现有私有实现）。
  4. REQ-PAR-05d 多轮 `ConversationHandle`（client 层）✅ 2026-09-09 已落地：包装现有 conversation 域
   （create/send/cancel + 会话事件归并原语），≈Codex Thread 多轮形态；`send` 等待终态并返回聚合 turn；
   webui 切同一句柄为后续（说明：webui 的 React reducer 为 UI 状态层，非本次 client 契约）。
  5. REQ-PAR-05e 重试辅助（client 层）✅ 2026-09-09 已落地：`retryable` + 指数退避 + 抖动的 `withRetry` 助手（≈Codex `retry_on_overload`）。
  顺序：05a → 05b → 05c → 05d → 05e（a/b 动协议，c/d/e 纯 client 层）。
  图片输入、sandbox 一等参数、archive/resume/fork 不列入本包（11 §6.3 或 V2）。
- REQ-PAR-01：抽 `@flowy-agent-store/browser`（12 §3 预留）：迁移 `web/src/lib/client.ts` 子类
 （serverRootUrl/browseDirectory/registerWorkspace + HTTP 底座）；`web/src` 只剩垫片。
- REQ-PAR-02：防漂移 contract test：`web/src` 禁止协议方法名字面量；
  新增协议方法必须 dispatch arms + client 方法 + 类型三件套（12 §7 入测试）。
- REQ-PAR-03 ✅ 2026-09-04 已落地（有修正）：Run 域事件路由直接在 `client` 包内以 `EventSubscription` 补齐
  （去重 seen 集 + `run/resync-required` 自动追平 + `follow` 静默建游标 + 手动 `resync()` 并发共享单 flight），
  单测 8 项全过（FakeTransport），真机 live（SDK spawn→WS 推送→completed）PASS（6 推送/0 重/游标覆盖）。
  注：`conversation-events.ts` 是会话域 transcript 归并（React 侧），不在下沉范围，保持 webui 私有。
- REQ-PAR-04 ✅ 2026-09-04 已落地（先行 AgentHandle 部分）：`client/run-handle.ts`——
  `launchRun()`（launch+follow）→ `AgentRunHandle`：`finished`（轮询兜底阻塞到终态，≈`thread.run()`）、
  `for await` 事件迭代（≈`runStreamed()`）、`cancel()`（带 receipt 版本）；纯组合零新协议方法。
  单测 4 项 + 真机 live（`scripts/sdk-live-handle.ts`：completed/6 事件/0 重）PASS。
  市场编排 helpers（`installFromMarket` 等）仍待做。

顺序：PAR-01 → PAR-02 → PAR-03 → PAR-04。门禁事项（工作包 A/B）优先于对齐；
PAR 与 A 无文件冲突，可并行。

## 7. 门禁判定与文档同步（Step 4，随做随更）

- 每项 TC 按测试总索引 §1.2 存证据（输入摘要/公共 ID/终态/脱敏日志路径），体例见
  `13-p0-execution-plan.md` §14。
- 同步点：roadmap 进展段、12 状态表、09 §9 快照、开发计划 §4 出口勾选。
- 最终输出 Phase 0 “通过 / 附条件通过 / 阻断”结论；只有通过才立项 Team Spike。

## 8. 已定决策（不再讨论）

| 事项 | 结论 | 日期 |
|---|---|---|
| stdio 传输 | V2/deferred；TC-SDK-001 已改为 spawn + 回环 WS | 2026-09-04 |
| Python SDK | 延后，优先 TS | 2026-09-04 |
| TC 编号范围 | 收缩到主表现有（SDK 001~003、WEB 001~004），缺号待开工补 | 2026-09-04 |
| 包名 | `@flowy-agent-store/sdk`（`07` 现行正文未改，以 `12` 为准） | 2026-09-04 |
| 证据体例 | 编号文档附录（2026-09-11 前为独立 `*-runtime-evidence.zh.md`）+ 可重复入口为正式证据 | 2026-09-04 |
| 点火脚本教训 | 子进程 stdout 必须持续消费/重定向（64KB 背压冻住服务端） | 2026-09-04 |

## 9. 风险与停止条件（继承开发计划 §12，增补）

| 风险 | 停止条件 | 处理 |
|---|---|---|
| 模型供给不稳（key/额度/网络） | A1 级链路连续失败 | 先修供给，计划整体后移；不记实现 BLOCKED |
| engine 持久化缺口（T4） | 改造超 2 天 | 降级断言，记 BLOCKED 写因（见 §4） |
| 规范化泄漏（T2） | 泄漏面超出映射层 | 升级为安全项，阻断 P0-A 关闭 |
| 对齐返工（D） | client 包 API breaking | 跨 web+sdk 同步改，禁止单边 fork |

---

## 10. Runtime 验收用例（TC-RT / TC-TEAM）

> 本节由 `agent-store-v1-test-cases.md` 原 §4（单 Agent Runtime）与 §5（AgentTeam Runtime）并入（2026-09-11 文档合并）；TC 编号与用例正文保持不变，内部分节号沿用原文。

### 4. 单 Agent Runtime

#### TC-RT-001：单 Agent Run

- 等级：P0
- 操作：通过 App Server `agent/run`
- 断言：返回异步 receipt；Run 经 queued/starting/running 进入 completed 或明确 failed；结果可查询

#### TC-RT-002：Definition 版本冻结

- 等级：P0
- 操作：Run 启动后发布同一 Agent 新版本
- 断言：运行中的 Run 继续使用原 Preset/ResolvedPresetSnapshot、Skill/Connector digest；历史记录可追溯，且不把 Preset ID 当作 Runtime Agent ID

#### TC-RT-003：策略交集

- 等级：P0
- 操作：让 caller、Agent、Connector Policy 产生权限冲突
- 断言：有效权限取交集；被拒绝操作返回 `policy_denied`

#### TC-RT-004：取消

- 等级：P1
- 操作：运行中调用 `run/cancel`
- 断言：取消请求不直接伪造终态；最终收到 `run.cancelled` 或明确无法取消的结果

#### TC-RT-005：重启后的未完成状态

- 等级：P0
- 操作：在运行中模拟进程重启
- 断言：已持久化事件和终态保留；allo 内部按既有安全恢复规则处理 Attempt；无法证明安全时标记为 `recovery_required`，不得伪装为 completed；App Server 不承诺一定续跑原 Attempt

#### TC-RT-006：状态持久化与事件序一致

- 等级：P0
- 操作：完成一次含 retry/replan 的 Run，读取 run 状态与事件序列
- 断言：终态、Plan Revision 和事件序（sequence 顺序）保持一致；重启后未完成 Run 标记正确

#### TC-RT-007：Attempt fencing（V2）

- 等级：V2
- 操作：让旧 Attempt 使用失效 fencing token 写入完成事件
- 断言：写入被拒绝并记录 stale/ignored 结果，不改变当前 Step 或 Run 状态

#### TC-RT-008：副作用重试幂等（V2）

- 等级：V2
- 操作：在 Connector 已提交外部副作用后模拟响应丢失并触发恢复
- 断言：重试携带相同外部幂等键或转为人工确认，不产生第二次不可逆副作用

#### TC-RT-009：Runtime Readiness Gate

- 等级：P0
- 操作：分别提交未验证、Adapter 已验证和 Runtime 已验证的 AgentDefinition
- 断言：未达到 `runtime-verified` 的 Preset/Runtime Agent 组合不能进入可运行 Catalog；Team 不能绕过单 Agent Gate

#### TC-RT-010：公共事件和错误规范化

- 等级：P0
- 操作：触发 allo 内部成功、失败、取消和未知错误
- 断言：Adapter 输出统一点号事件、公共错误码和 opaque ID，不泄露 allo 内部 ID 或凭据

### 5. AgentTeam Runtime

#### TC-TEAM-001：固定成员物化

- 等级：P0
- 操作：启动 software-company TeamRun
- 断言：生成固定 Participant 池和 AgentExecutionTemplate；成员来自 TeamDefinition，不由模型任意新增；Leader 和每个成员分别绑定各自 Preset/ResolvedPresetSnapshot，成员 Prompt 不被合并到 Leader Prompt；TeamRun 创建时服务端创建 Leader Conversation，并把该 AgentExecutionTemplate 绑定为其 `execution_template_id`

#### TC-TEAM-002：Leader 经 `nomi_delegate(strategy=planned)` 触发 planned DAG

- 等级：P0
- 操作：提交 Team goal
- 断言：Leader 在其 Conversation 的 turn 内调用 `nomi_delegate(strategy=planned, goal=…)`；服务端据此构造 Planning Context（Leader Preset 规划指令 + Team 目标 + 脱敏成员能力摘要 + Team 策略），由内部 Planner 生成 Plan；Plan 经过依赖、成员路由、并发和策略校验后才物化 Step；成员池与并发上限取自绑定的 Template，不接受模型参数；不创建用户可见的 Leader Conversation
- 断言（实现选择）：注册给 Leader 的 delegate 工具必须支持 `strategy=planned` 且绑定持久 `AgentExecutionEngine`；仅支持 `strategy=parallel` 的同步 embedded 实现不得出现在 Team 会话工具面；以 `strategy=parallel` 替代 planned 流程的顶层调用被拒绝

#### TC-TEAM-003：依赖调度

- 等级：P0
- 操作：构造 A→B→C 依赖
- 断言：B 不早于 A 完成；C 不早于 B 完成；事件顺序和状态一致

#### TC-TEAM-004：局部并行

- 等级：P0
- 操作：构造 A/B 独立、C 依赖 A/B 的 DAG
- 断言：A/B 可并行；C 等待两者完成；实际并发不超过有效 max_parallel

#### TC-TEAM-005：失败 retry

- 等级：P0
- 操作：让一个 Step 第一次失败
- 断言：生成新 Attempt；旧 Attempt 保留；retry 次数受策略限制；成功后 Step 正确完成

#### TC-TEAM-006：失败 replan

- 等级：P0
- 操作：制造 QA 失败并触发 replan
- 断言：生成新的 Plan Revision；历史 Plan 保留；新计划包含修复和回归验证步骤

#### TC-TEAM-007：旧事件隔离

- 等级：P0
- 操作：让旧 Attempt 的迟到事件在新 Attempt 后到达
- 断言：迟到事件不覆盖新状态；记录 stale/ignored 结果

#### TC-TEAM-008：Planning Context 与成员 Prompt 隔离

- 等级：P0
- 操作：启动含不同 Leader/成员 persona、Skill 和 Connector 策略的 TeamRun
- 断言：Planning Context 只包含 Leader 规划指令、Team 策略和脱敏成员能力摘要；成员完整 Prompt、凭据引用和未授权工具细节不进入共享上下文；每个成员 Attempt 使用自己的 Prompt Snapshot

#### TC-TEAM-009：V1 延期能力边界

- 等级：P1
- 操作：请求成员自主认领、嵌套 Team 或成员任意直连消息
- 断言：明确返回 unsupported/feature_not_available；不得静默伪装支持


---

## 11. 测试基础设施（环境与证据要求 / 验收等级）

> **口径单一来源见测试总索引 `agent-store-v1-test-cases.md` §1 / §2**（2026-09-11 收敛：此前本节的重复副本已移除，避免两处漂移）。
>
> - **§1 测试环境与证据要求**：环境记录项、真实凭据记录方式、每个通过用例的证据清单（本线证据体例见测试总索引 §1.2）。
> - **§2 验收等级**：P0/P1/P2 定义、结果取值（PASS / FAIL / BLOCKED / NOT_RUN）与 `BLOCKED` 处置。
>
> 本线（P0）用例按上述口径执行并存证。


---

## 12. P0 发布门禁用例

> 本节由 `agent-store-v1-test-cases.md` 原 §9 并入（2026-09-11）。**门禁的权威定义（五道 Gate 的判定口径）见 `09-release-readiness.md` §3**；本节只固定 P0 线要求的**用例编号集合**与阻断处置。

### 9. P0 发布门禁

以下用例必须全部 PASS：

```text
TC-IMP-001/002/004/005/006/008/009
TC-RT-001/002/003/004/005/006/009/010
TC-TEAM-001/002/003/004/005/006/007/008
TC-API-001/002/003/004
TC-SDK-001/002/003
TC-OAUTH-001/002/003/004
TC-CONN-001/002
TC-CLI-001
TC-STDIO-001
TC-WEB-001/002/003/004
TC-SEC-001/002/003
```

P0 用例为 BLOCKED 时必须由产品负责人明确接受风险；未经接受不得发布。


---

## 13. 回归与记录

> 本节由 `agent-store-v1-test-cases.md` 原 §10 并入（2026-09-11）。**发布证据包的完整清单见 `09-release-readiness.md` §4**；本节定义**每次修改触发的回归范围**与测试报告字段。

### 10. 回归与记录

每次修改以下内容都必须执行相关回归：

```text
领域模型
Importer
Runtime Adapter
App Server Schema
Protocol version
Event envelope
Credential Provider
Tool Policy
OAuth metadata
SDK generated types
```

测试报告必须包含：

```text
commit/version
测试时间
环境摘要
用例总数
PASS/FAIL/BLOCKED/NOT_RUN 数量
失败用例和复现信息
已知风险
发布结论
```

测试报告不得包含真实 Token、API Key、密码、连接字符串或完整敏感参数。

---

## 14. 附录 A · 单 Agent 真实 Run 验收证据（TC-RT-001）

> 本节由 `single-run-runtime-evidence.zh.md` 整体并入（2026-09-11）。原文的时间戳与「历史实测快照，非契约」定性**保持不变**。


> 日期：2026-09-04
> 目标：开发计划 §4 P0-A（单 Agent Run 真实完成），对齐 `agent-store-v1-test-cases.md` TC-RT-001
> 状态：✅ 通过（临时实例 + mimo-v2.5 真实模型：planning → running → completed）
> 方法：独立二进制 `agent-store --port 0 --data-dir <临时> --no-open` 就绪行建连，
> 注册 mimo provider → 导入并安装 `software-company` fixture → `agent/run`（mention 指向
> `wb-software-company-software-architect`）→ 轮询 `run/get` → `run/result` + `run/events`

### 1. 实现范围

| 组件 | 位置 | 内容 |
|---|---|---|
| Run 入口 | `nomifun-app-server::execute_agent_run`（`lib.rs:2102`） | 幂等 fingerprint/scope → preset resolve（`agent-store: ` 前缀门禁）→ `AgentRuntimeAdapter::start_agent_run` → 公共 ID 映射（失败则 best-effort cancel 防孤儿） |
| Adapter | `nomifun-agent-execution::runtime_adapter.rs` | PresetSnapshot → `create_for_app_server`（Single 模型池、max_parallel=1）；`get_run`/`get_result`（终态门禁）/`list_events`/`cancel_run` |
| 生产装配 | `nomifun-app::router::routes` + `apps/agent-store` | `create_router` 全量注入 `runtime: Some` + `preset_service: Some` + 幂等/映射仓储；model 取 owner 首个 enabled provider |
| 本次修复 | `runtime_adapter.rs`（`start_agent_run`/`cancel_run`） | `external_agent("app-server")` → `user(owner_id)`：前者自由字符串违反 executions.actor_id UUIDv7 CHECK，此前任何 `agent/run` 落库必 500；后者与 UI 两条创建路径一致（`routes.rs:448`、`template_routes.rs:116`） |

### 2. 验收结果（TC-RT-001）

| 断言 | 结果 |
|---|---|
| `agent/run` 返回异步 receipt（run_id/preset_revision/content_digest） | ✅ `status=planning, preset_revision=1, digest=sha256:b06a…` |
| Run 经 planning/running 进入 completed | ✅ `planning → running(v4) → completed(v6)`，轮询全程 2~5ms |
| 结果可查询且与终态一致 | ✅ `run/result` 200：中文一句话总结，`output_files=[]`，终态 completed |
| 事件可查询 | ✅ `run/events` 200（limit=50） |

### 3. 自动化证据

```text
cargo build -p agent-store（adapter fix 后）                        ✅ 3m14s（仅既有 nomifun-app 警告）
cargo test -p nomifun-app-server                                    ✅ 58/58（含新增 14 个 WS arms 门禁测试）
cargo test -p agent-store                                           ✅ 5/5
A1 点火脚本（临时实例真机链路，见 §5）                               ✅ planning→running→completed
```

### 4. 关键边界（已验证）

- **actor 归因**：App Server Run 的执行 actor 为连接用户本人（`user_id`），与 UI 创建一致；`system` 仍只保留给 scheduler/recovery。
- **版本冻结就绪**：receipt 已携带 `preset_revision` + `content_digest`，为 TC-RT-002 断言提供抓手（本次未执行）。
- **provider 隔离**：测试用临时 data_dir + 临时 provider 注册（`mimo-a1`），本机 `~/.agent-store` 零污染；key 只存在于脚本进程内存，全程未落盘明文、未进日志与文档。

### 5. 复现入口与已知边界（如实披露）

- 复现入口：点火脚本（临时目录，已归档会话；正式回归待 B1 固化进 `smoke --real`/sdk e2e）+
  `cargo test -p nomifun-app --test importer_e2e importer_mention_resolves_installed_preset_and_agents_run_gate`
 （import→install→mention 解析→run 门禁，模型/runtime 边界前）。
- 本次 goal 显式要求不调工具；工具调用、retry/replan、cancel、重启恢复均未覆盖（TC-RT-004/005/006 待 Step 2）。
- 点火脚本教训：读子进程 stdout 做就绪扫描后必须持续消费或重定向到文件，否则 64KB 管道背压会冻住服务端（曾误报为“服务端昏迷”，实为 harness bug，已在脚本内修复）。
- 原始日志随临时目录清理；本页 + 可重复入口为正式证据，符合测试总索引 §1.2（输入摘要/公共 ID/终态/脱敏）。

---

## 15. 附录 B · P0 运行时证据（WP-3）

> 本节由 `p0-runtime-evidence.zh.md` 整体并入（2026-09-11）。定性同附录 A。


> 状态：📎 证据（运行时实测快照，非契约）
> 日期：2026-09-09
> 脚本：`web/scripts/sdk-live-p0a.ts`（P0-A）、`web/scripts/sdk-live-p0b.ts`（P0-B）
> 模型：mimo-v2.5（key 仅脚本内存）

### 摘要

| 工作包 | 用例 | 结果 |
|---|---|---|
| P0-A | TC-RT-004 / TC-RT-002 / TC-RT-010 / TC-API-002 / TC-API-003 | **18/18 PASS** |
| P0-B | TC-RT-005 / TC-RT-006 | **10/10 PASS** |

**P0-A/B 已关闭**（无 engine 持久化改造，未触发降级条件）；按 `13` §4 出口与
`15` §7 门禁，Phase 0 出口条件满足，Team Spike（Phase 2）可立项。
剩余 P0-C/D：OAuth 运行时证据（TC-OAUTH-001/002/004）待补；TC-CONN-002 已在 WP-2 覆盖。**（该余项后已补齐：`06-connector-oauth-security.md` §12，26/26 PASS。）**

### P0-A（TC-RT-004 / TC-RT-002 / TC-RT-010 / TC-API-002 / TC-API-003）

最近一次：**18/18 PASS**（`RESULT PASS`，data `agent-store-p0a-1788933195275`）。

| 用例 | 判据 | 实测 |
|---|---|---|
| TC-RT-004 取消 | cancel 前非终态；终态 `cancelled`；版本递进 | `planning@v0` → `cancel-accepted` → `cancelled`，版本 **v0→v1** |
| TC-RT-002 版本冻结 | 运行中发布同 Agent 新版本，冻结字段不变；历史可追溯；preset_id ≠ runtime agent id | 重装 v9.9.9（9 组件）后 `preset_revision:1` + `content_digest:sha256:1fd66d36…` 在 run/get 与 run/result 均不变；`preset_id=01a084ba-…` ≠ `agent_id=wb-software-company-software-architect` |
| TC-RT-010 规范化 | 无内部 ID、无凭据；错误为稳定 code | run/get、run/result、run/events、store/install-entry、install/status、agents/list 六处扫描零泄漏；未知 run → `not_found` |
| TC-API-002 幂等重放 | 同 key + 同请求 → 同 run_id；同 key + 不同请求 → `idempotency_conflict` | 重放返回同一 run_id `01a084ba-537e-…`；改 goal 后返回 `idempotency_conflict` |
| TC-API-003 终态一致 | `run/get` 与 `run/result` 终态一致 | 两者均 `completed@v6` |

#### 语义澄清（本轮 live 校准）

- **version 从 0 起算**：`agent_executions` INSERT 显式 `version=0`，每次状态迁移 +1。
  TC-RT-004 的「版本递进」应断言**相对取消前严格递增**（v0→v1），而非绝对值 >1。
- `run/cancel` 走 CAS（`expected_version`），并发失配返回 conflict；脚本按「重读→重试」处理。

#### 判据与用例原文的差异

`13-p0-execution-plan.md` 对 TC-RT-004 写「终态为 cancelled 且版本递进」；用例原文
（`agent-store-v1-test-cases.md` §TC-RT-004）为「取消请求不直接伪造终态；最终收到
`run.cancelled` 或明确无法取消的结果」。两者均满足。

### P0-B（TC-RT-005 重启恢复 / TC-RT-006 事件序）

最近一次：**10/10 PASS**（`RESULT PASS`，data `agent-store-p0b-6X0Zum`）。

脚本 `web/scripts/sdk-live-p0b.ts`：自持进程（硬杀 pid 模拟崩溃）→ 同一 data dir
重启 → 协议面仍走 SDK client。

| 用例 | 判据 | 实测 |
|---|---|---|
| TC-RT-005 重启恢复 | 事件与终态保留；不伪装 completed；无法证明安全时 `recovery_required` | 杀前 `running@v4`（seq1–5）→ 重启后可解析且首读 `running`（非 completed）→ 安全收敛 `recovery_required@v5` |
| TC-RT-006 事件序 | 已持久化事件保留、序列无缺口、重启后续号单调 | 崩溃前 seq1–5 完整保留；重启后新增 seq6；最终 [1..6] 单调无缺口 |

```text
PASS P0B.pre-kill.running :: {"status":"running","version":4}
PASS P0B.pre-kill.events-exist :: ["1:run.started","2:run.plan_changed","3:run.status_changed","4:attempt.updated","5:attempt.updated"]
KILLED pid=24464
PASS TC-RT-005.run-resolvable-after-restart :: "running"
PASS TC-RT-005.not-fake-completed :: "running"
PASS TC-RT-006.events-preserved :: {"pre":[1,2,3,4,5],"post":[1,2,3,4,5]}
PASS TC-RT-006.sequence-gapless :: [1,2,3,4,5]
PASS TC-RT-005.settles-safely :: {"status":"recovery_required","version":5}
PASS TC-RT-006.final-sequence-monotonic :: [1,2,3,4,5,6]
```

说明：恢复路径 `reconcile_recovered_attempt` 判定无法证明安全 → `review_blocked`
（`runtime_state.reason=process_restart`）→ run 投影为 `recovery_required`；
未做任何 engine 持久化改造，未触发 §4 的 2 天降级条件。

#### 方案选择（P0-B 前置）

SDK `SpawnedServer` 只暴露 `close()`（优雅停），不暴露 pid/child。采用**方案 1**：
脚本自持进程（`Bun.spawn` + 持续排空 stdout/stderr 防背压），协议面经
`AppServerClient` + `WebSocketTransport` 连回环 WS——**不动 SDK 公共面**。
若后续需要 App 侧进程管理能力，再单独评估给 `SpawnedServer` 增补 `pid`。

### 复现

```bash
cd web
AGENT_STORE_BIN=<repo>/target/debug/agent-store.exe bun scripts/sdk-live-p0a.ts
# CHAIN_KEEP_DATA=1 保留实例数据目录供取证
```

退出码即判据：0 = 全部 PASS。

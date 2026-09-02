# Agent Store V1 测试验收用例

> 状态：测试基线冻结（Phase 0）；用例待执行
> 日期：2026-08-26
> 前置：`agent-store-v1-roadmap.md`、`10-public-contracts.md` 及本目录中的 Agent Store 规格文档
> 说明：本文件是 V1 测试用例的唯一主表。Runtime、Protocol、SDK、Web 和安全文档只引用这里的测试编号；新增或修改用例必须先更新本文件。

## 1. 测试环境与证据要求

### 1.1 环境

测试必须记录：

```text
App Server 版本
allo Runtime 版本
Protocol version
SDK 版本
Import source digest
Definition version
Connector/Mock Server 版本
workspace 标识
操作系统和关键配置摘要
```

真实凭据只记录：

```text
credential_present: true
credential_id: <opaque id>
```

禁止记录真实值；任何示例统一使用 `[REDACTED]`。

### 1.2 证据

每个通过用例至少保存：

```text
测试输入摘要
请求/响应摘要
相关 event_id/sequence
Run/Task/Attempt/Artifact 公共 ID
最终状态
日志或截图路径（脱敏）
```

外部系统副作用用测试账号、隔离 workspace 或 mock Connector；不得用生产数据验收。

## 2. 验收等级

```text
P0 阻断发布：主链路、安全、数据一致性
P1 必须交付：V1 功能完整性
P2 可延期：体验优化和非核心适配
```

结果：

```text
PASS
FAIL
BLOCKED
NOT_RUN
```

`BLOCKED` 必须写明阻塞原因和替代证据，不能当作 PASS。

## 3. Importer 与 Catalog

### TC-IMP-001：导入有效 Plugin

- 等级：P0
- 前置：有效 Plugin manifest 和受控源目录
- 操作：执行 Importer
- 断言：生成不可变 PluginSnapshot、digest、来源和版本；状态为 `completed` 或 `completed-with-warnings`
- 证据：snapshot_id、digest、definitions 数量、CompatibilityReport

### TC-IMP-002：导入 software-company

- 等级：P0
- 操作：导入包含 Team 扩展信息的插件
- 断言：生成 5 个 AgentDefinition、1 个 AgentTeamDefinition；Lead 和 member IDs 正确；V1 可生成固定 Participant 配置
- 证据：定义列表、成员关系、来源路径摘要、digest

### TC-IMP-003：agents 目录不自动生成 Team

- 等级：P0
- 操作：导入只有 `agents/`、没有明确 Team 配置的插件
- 断言：只生成 AgentDefinition[]，不生成 AgentTeamDefinition

### TC-IMP-004：路径遍历阻断

- 等级：P0
- 操作：提供 `../`、绝对路径和快照外引用
- 断言：导入状态为 `blocked`；不产生可运行定义；写入安全错误

### TC-IMP-005：符号链接逃逸阻断

- 等级：P0
- 操作：提供指向快照目录外的符号链接
- 断言：拒绝安装或复制；无外部文件进入 Snapshot

### TC-IMP-006：digest 冲突阻断

- 等级：P0
- 操作：同一来源身份/版本提交不同内容 digest
- 断言：状态为 `blocked` 或 `failed`；不得覆盖既有不可变 Snapshot

### TC-IMP-007：部分组件失败

- 等级：P1
- 操作：让单个 Skill 或 Command 文件无法解析
- 断言：其他合法组件可导入；结果为 `completed-with-warnings`；失败组件有 CompatibilityReport 条目

### TC-IMP-008：高风险组件静态导入

- 等级：P0
- 操作：导入 Hook、bin、scripts、LSP
- 断言：保存来源元数据和兼容性状态；导入过程不执行任意进程或脚本

### TC-IMP-009：凭据只生成 Schema

- 等级：P0
- 操作：导入含 userConfig/token schema 的插件
- 断言：只生成 CredentialSchema/Binding 引用；真实值不进入 Snapshot、日志和公共响应

## 4. 单 Agent Runtime

### TC-RT-001：单 Agent Run

- 等级：P0
- 操作：通过 App Server `agent/run`
- 断言：返回异步 receipt；Run 经 queued/starting/running 进入 completed 或明确 failed；结果可查询

### TC-RT-002：Definition 版本冻结

- 等级：P0
- 操作：Run 启动后发布同一 Agent 新版本
- 断言：运行中的 Run 继续使用原 Preset/ResolvedPresetSnapshot、Skill/Connector digest；历史记录可追溯，且不把 Preset ID 当作 Runtime Agent ID

### TC-RT-003：策略交集

- 等级：P0
- 操作：让 caller、Agent、Connector Policy 产生权限冲突
- 断言：有效权限取交集；被拒绝操作返回 `policy_denied`

### TC-RT-004：取消

- 等级：P1
- 操作：运行中调用 `run/cancel`
- 断言：取消请求不直接伪造终态；最终收到 `run.cancelled` 或明确无法取消的结果

### TC-RT-005：重启后的未完成状态

- 等级：P0
- 操作：在运行中模拟进程重启
- 断言：已持久化事件和终态保留；allo 内部按既有安全恢复规则处理 Attempt；无法证明安全时标记为 `recovery_required`，不得伪装为 completed；App Server 不承诺一定续跑原 Attempt

### TC-RT-006：状态持久化与事件序一致

- 等级：P0
- 操作：完成一次含 retry/replan 的 Run，读取 run 状态与事件序列
- 断言：终态、Plan Revision 和事件序（sequence 顺序）保持一致；重启后未完成 Run 标记正确

### TC-RT-007：Attempt fencing（V2）

- 等级：V2
- 操作：让旧 Attempt 使用失效 fencing token 写入完成事件
- 断言：写入被拒绝并记录 stale/ignored 结果，不改变当前 Step 或 Run 状态

### TC-RT-008：副作用重试幂等（V2）

- 等级：V2
- 操作：在 Connector 已提交外部副作用后模拟响应丢失并触发恢复
- 断言：重试携带相同外部幂等键或转为人工确认，不产生第二次不可逆副作用

### TC-RT-009：Runtime Readiness Gate

- 等级：P0
- 操作：分别提交未验证、Adapter 已验证和 Runtime 已验证的 AgentDefinition
- 断言：未达到 `runtime-verified` 的 Preset/Runtime Agent 组合不能进入可运行 Catalog；Team 不能绕过单 Agent Gate

### TC-RT-010：公共事件和错误规范化

- 等级：P0
- 操作：触发 allo 内部成功、失败、取消和未知错误
- 断言：Adapter 输出统一点号事件、公共错误码和 opaque ID，不泄露 allo 内部 ID 或凭据

## 5. AgentTeam Runtime

### TC-TEAM-001：固定成员物化

- 等级：P0
- 操作：启动 software-company TeamRun
- 断言：生成固定 Participant 池和 AgentExecutionTemplate；成员来自 TeamDefinition，不由模型任意新增；Leader 和每个成员分别绑定各自 Preset/ResolvedPresetSnapshot，成员 Prompt 不被合并到 Leader Prompt

### TC-TEAM-002：Planning Context 驱动的 planned DAG

- 等级：P0
- 操作：提交 Team goal
- 断言：服务端根据 Leader Preset 规划指令、Team 目标、脱敏成员能力摘要和 Team 策略构造 Planning Context，由内部 Planner 生成 Plan；Plan 经过依赖、成员路由、并发和策略校验后才物化 Step；不创建独立 Leader Conversation，也不依赖 `nomi_delegate` 工具

### TC-TEAM-003：依赖调度

- 等级：P0
- 操作：构造 A→B→C 依赖
- 断言：B 不早于 A 完成；C 不早于 B 完成；事件顺序和状态一致

### TC-TEAM-004：局部并行

- 等级：P0
- 操作：构造 A/B 独立、C 依赖 A/B 的 DAG
- 断言：A/B 可并行；C 等待两者完成；实际并发不超过有效 max_parallel

### TC-TEAM-005：失败 retry

- 等级：P0
- 操作：让一个 Step 第一次失败
- 断言：生成新 Attempt；旧 Attempt 保留；retry 次数受策略限制；成功后 Step 正确完成

### TC-TEAM-006：失败 replan

- 等级：P0
- 操作：制造 QA 失败并触发 replan
- 断言：生成新的 Plan Revision；历史 Plan 保留；新计划包含修复和回归验证步骤

### TC-TEAM-007：旧事件隔离

- 等级：P0
- 操作：让旧 Attempt 的迟到事件在新 Attempt 后到达
- 断言：迟到事件不覆盖新状态；记录 stale/ignored 结果

### TC-TEAM-008：Planning Context 与成员 Prompt 隔离

- 等级：P0
- 操作：启动含不同 Leader/成员 persona、Skill 和 Connector 策略的 TeamRun
- 断言：Planning Context 只包含 Leader 规划指令、Team 策略和脱敏成员能力摘要；成员完整 Prompt、凭据引用和未授权工具细节不进入共享上下文；每个成员 Attempt 使用自己的 Prompt Snapshot

### TC-TEAM-009：V1 延期能力边界

- 等级：P1
- 操作：请求成员自主认领、嵌套 Team 或成员任意直连消息
- 断言：明确返回 unsupported/feature_not_available；不得静默伪装支持

## 6. App Server Protocol 与 SDK

### TC-API-001：初始化协商

- 等级：P0
- 断言：未 initialize 不能调用业务方法；协议不兼容返回明确错误；initialized 后才进入 ready

### TC-API-002：异步 receipt 与幂等

- 等级：P0
- 操作：重复提交相同 idempotency_key
- 断言：不重复创建 Run；不同请求复用同 key 返回 `idempotency_conflict`

### TC-API-003：状态查询与通知一致性

- 等级：P0
- 操作：订阅通知并轮询 run/get；模拟连接中断后重连
- 断言：run/get/run_result 始终返回权威持久化状态；通知丢失不造成状态不一致；展示层按 event_id 去重

### TC-API-004：公共 ID 隔离

- 等级：P0
- 断言：响应不包含 allo 内部 session、数据库或 provider 私有 ID

### TC-SDK-001：Node stdio

- 等级：P1
- 断言：Node SDK 可完成 initialize、Catalog、Run、Event、Artifact 调用

### TC-SDK-002：Browser WebSocket

- 等级：P1
- 断言：Browser SDK 可连接、接收通知、断线重连后以状态查询恢复一致视图；不访问安全凭据存储

### TC-SDK-003：结构化错误

- 等级：P1
- 断言：SDK 使用稳定 error code，不依赖 message 文本；retryable 语义正确

## 7. Connector 与 OAuth

### TC-CONN-001：工具命名空间

- 等级：P0
- 断言：上游工具只能以公开命名空间名称暴露；未在 allowlist 的工具不能调用

### TC-OAUTH-001：标准 PKCE Loopback

- 等级：P0
- 断言：state、PKCE、issuer、resource、redirect_uri 校验通过；成功后得到 authenticated 状态

### TC-OAUTH-002：凭据隔离

- 等级：P0
- 断言：Access/Refresh Token 不进入 Renderer、Prompt、Tool Result、Event、日志、SDK 公共响应

### TC-OAUTH-003：请求时注入与刷新

- 等级：P0
- 断言：请求时由 Credential Provider 注入；401 触发一次刷新和一次重试；刷新失败进入 reauthorization_required

### TC-OAUTH-004：错误边界

- 等级：P0
- 操作：Issuer/Resource 不匹配、403、Callback 超时
- 断言：分别返回明确错误；不盲目刷新或发送请求

### TC-CONN-002：Connector Probe

- 等级：P1
- 断言：配置存在但 Probe 失败时状态不能显示 connected

### TC-CLI-001：CLI 受控执行

- 等级：P0
- 断言：任意 executable、subcommand、argv、cwd、环境变量覆盖均被拒绝；仅允许 schema/allowlist 内输入

### TC-STDIO-001：STDIO 生命周期

- 等级：P0
- 断言：子进程超时、崩溃或协议错误后被回收；工具调用停止；状态和审计可查询

## 8. Web/Flowy 与安全

### TC-WEB-001：Catalog 和兼容性状态

- 等级：P1
- 断言：UI 显示来源、版本、状态和 V1 能力；不把 manual-review/unsupported 显示为可运行

### TC-WEB-002：Team Run 页面

- 等级：P1
- 断言：显示成员、Leader（规划角色）、Planning Context 摘要/digest、Plan/DAG、Step、Attempt、Timeline、Artifact；保留 Plan Revision；不显示 Planning Context 正文或成员完整 Prompt

### TC-WEB-003：Approval UI

- 等级：P0
- 断言：显示脱敏资源和参数摘要；批准/拒绝由 App Server 二次校验；过期审批不能执行

### TC-WEB-004：Artifact 路径边界

- 等级：P0
- 断言：不能通过 UI/API 读取任意本地路径；Artifact 必须绑定 run/workspace 并校验 digest

### TC-SEC-001：敏感信息扫描

- 等级：P0
- 断言：日志、事件、Snapshot、Renderer 状态和公共响应不包含真实凭据；扫描结果无高危泄露

### TC-SEC-002：高风险副作用审批

- 等级：P0
- 断言：发送、删除、发布、部署等操作没有有效 Approval 时返回 `approval_required` 或 `policy_denied`

### TC-SEC-003：版权发布阻断

- 等级：P0
- 断言：`pending-legal-review` 资源不能进入公开 Marketplace 或默认安装包

## 8.5 Agent Store Skill / Connector 目录（新增切片）

### TC-CATALOG-001：能力协商与目录方法

- 等级：P1
- 前置：生产装配注入 Skill/Connector/OAuth provider
- 操作：WS 初始化后调用 `skill/list`、`skill/get`、`connector/list`、`connector/get`、`connector/status`、`connector/test`、`connector/auth/start`、`connector/auth/status`、`connector/auth/logout`
- 断言：`initialize` 返回 `skills=true`、`connectors=true`、`oauth=true`；目录方法返回 Agent Store 公共形状（id/name/version/source/compatibility_status/enabled；Connector 含 kind/transport_summary/auth_mode/status），不含真实凭据、内部 ID 或文件系统绝对路径；未注入 provider 时对应方法返回 `unsupported_operation`

### TC-CATALOG-002：Connector 状态合并规则

- 等级：P1
- 操作：对最近 Probe 失败的 Connector 调用 `connector/status`
- 断言：状态不得为 `connected`（对齐 TC-CONN-002）；OAuth 未就绪的 remote Connector 显示 `authorization_required`；Probe 成功后状态转为 `connected`（仅当无更高优先级失败状态）

### TC-CATALOG-003：OAuth 状态透传

- 等级：P1
- 操作：`connector/auth/start` → 轮询 `connector/auth/status` → `connector/auth/logout` → 再次查询
- 断言：start 只返回状态与一次性授权 URL/会话（无 Token）；status 在 `not_authenticated`/`authenticated` 间翻转；logout 后回到 `not_authenticated`；stdio Connector 的 OAuth 方法返回稳定错误而非伪造成功

### TC-CATALOG-004：Preset 绑定 Connector 的运行接线

- 等级：P1
- 操作：Preset 绑定非空 `mcp_server_ids` 时发起 `agent/run`
- 断言：引用的 Connector 均存在且启用时运行正常启动，attempt 会话冻结 `mcp_server_ids`（经 Conversation 层 `selected_mcp_server_ids` 校验/持久化路径）；存在缺失/禁用 Connector 时运行前返回 `connector_unavailable`，不启动 Run；`run/get`/`run/result` 公共视图不泄露凭据

### TC-CATALOG-005：WebUI 目录视图（翻 `web/`）

- 等级：P1
- 操作：WebUI 经 WS 链路浏览「技能与连接器」目录
- 断言：导航入口仅在能力协商开启时显示；技能列表显示来源/版本/兼容性/所需连接器，详情只含公开指令摘要；连接器列表显示状态徽标且 `connected` 不伪报；OAuth 连接器可完成授权/取消授权交互，UI 不接触 Token

## 9. P0 发布门禁

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

## 10. 回归与记录

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

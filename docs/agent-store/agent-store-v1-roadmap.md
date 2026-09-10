# Agent Store V1 Roadmap 与测试验收计划

> 状态：实施路线冻结（Phase 0）；排期待 Spike 校准
> 日期：2026-08-26
> 前置：`00-architecture-decision.md` 至 `08-flowy-web-integration.md`、`10-public-contracts.md`
> 说明：阶段按两周迭代组织；具体排期须在 Spike 和实测后校准，不构成承诺
> 进展：Phase 0 的 OAuth 注入实测已完成（`mcp-oauth-runtime-evidence.zh.md`）；
> Phase 1 的 Importer 已落地（TC-IMP-001~009 通过，`importer-runtime-evidence.zh.md`）；
> 安装模式与市场（Phase 2 提前落地）已完成（install/*、market/* 四类源、
> @mention 解析与 run 注入 TC-INS-001~007）；
> 2026-09-04：单 Agent 真实 Run 通过（TC-RT-001 planning→running→completed，mimo-v2.5 临时实例，
> `single-run-runtime-evidence.zh.md`；附带修复 actor 落库 500）；
> 其余 Runtime 门禁（TC-RT-002/005/006/009/010）与最小 TeamRun 尚未执行，排期仍待校准。
> 2026-09-10：Team 触发方式修订为 Leader 模型调用 `nomi_delegate(strategy=planned)`（§10 决策 3），同步修订 `00`/`01`/`03`/`04`/`10` 与 TC-TEAM-001/002。

## 1. V1 目标

V1 目标不是单 Agent PoC，而是完成本地优先 Agent Store 的核心端到端闭环：

```text
CodeBuddy/WorkBuddy Importer
    ↓
PluginSnapshot / Catalog
    ↓
Agent / Team / Skill / Connector
    ↓
allo Runtime Adapter
    ↓
App Server Protocol
    ↓
TypeScript SDK
    ↓
Flowy/Web
    ↓
Run / Event / Artifact / Approval
```

V1 Team 范围：

```text
固定成员
+ AgentExecutionTemplate
+ Leader Planning Context 驱动的 planned DAG
+ 局部并行
+ retry / replan
+ Event / Artifact
```

V1 不要求：

```text
完整 Mailbox
成员自主认领
成员直连消息
长期成员会话
嵌套 Team
```

## 2. 交付范围

### 2.1 必须交付

| 领域 | V1 交付 |
|---|---|
| Runtime | allo 唯一 Runtime + Runtime Adapter |
| Import | CodeBuddy/WorkBuddy PluginSnapshot |
| Catalog | Agent/Team/Skill/Connector 定义、版本、来源、digest、状态 |
| Agent | 单 Agent Run、取消、事件、结果 |
| Team | 固定成员、Leader Planning Context、planned DAG、局部并行、retry/replan |
| Connector | 至少一个 MCP Connector、工具过滤、Probe |
| OAuth | 标准 PKCE Loopback OAuth、存储、注入、刷新、重试 |
| Protocol | Versioned App Server Protocol、WebSocket 绑定、状态查询（stdio 不纳入，见 §10 决策） |
| SDK | TypeScript typed client、重连与状态同步、错误处理 |
| Web | Catalog、Run、Plan/DAG、Timeline、Artifact、Approval、Connector 状态 |
| 安全 | 凭据隔离、Tool Policy、审批、脱敏审计 |

### 2.2 明确非目标

```text
第二个 Runtime
云端执行
多租户与 HA
完整公开 Marketplace 审核后台
签名更新体系
全部 OAuth 变体
任意 Hook/bin/script 执行
完整 LSP Runtime
完整 CodeBuddy Team 语义
入站 MCP Server 的全部能力
```

## 3. 阶段计划

### Phase 0：基线与 Spike（两周迭代）

目标：先关闭 Runtime Readiness Gate，而不是并行实现完整 UI。

必须实测并留存：

- AgentDefinition → Preset/ResolvedPresetSnapshot → allo ExecutionParticipant → Runtime Agent/Driver → 单 Agent Run；
- Event Log、持久化状态查询和重启后的未完成状态标记；
- 本地 App Server AuthContext；
- Connector Token 注入、Probe、401 刷新和单次重试。

在 Phase 0 未通过前，Team、SDK、Web 只能实现 schema/mock，不得宣称端到端完成。

目标：验证 allo 现有基础能支撑 V1。

交付：

- 领域模型和四份基线文档定稿；
- Adapter 最小接口；
- App Server 协议 Schema 草案；
- `software-company` 资源扫描；
- 单 Agent 和固定成员 Team 的最小创建 Spike（含 Planning Context 构造）；
- 事件、Artifact、OAuth 注入风险清单。

验收：

- 能从导入资源得到 5 个 Agent 和 1 个 Team；
- 能确认 AgentExecutionTemplate、Planning Context、Planner、Participant Resolver 的实际调用链；
- 对尚未贯通的 Agent 注册和 OAuth transport 注入明确记录证据或失败原因；
- 不以类型存在代替端到端验证。

门禁：

- 如果单 Agent 无法创建可执行快照，先修复 Adapter 边界；
- 如果 Team planned 无法物化 DAG，调整 V1 Team 实现方案并保留 Team 公共模型；
- 如果 OAuth 注入未贯通，Connector 只能标记 partial，不得宣称 connected。

### Phase 1：Importer 与 Catalog（两周迭代）

> 状态：✅ 主体已实现（`nomifun-importer` + `plugin_snapshots` Catalog + App Server
> `import/*`、`agent/list`、`team/list`）+ 安装模式（`install/*`，Phase 2 提前落地）+
> 市场（`market/*`，directory/github/git/url 四类源：git2 克隆 + HTTP 条件下载 +
> staging 校验 → 原子晋升 → last-good；`market/refresh` 新鲜度短路；级联卸载）；
> 自动更新后台任务、安装作用域与团队分发（阶段 C/D）留待后续

交付：

- CodeBuddy/WorkBuddy Importer；
- PluginSnapshot 不可变缓存；
- Agent/Team/Skill/Connector 标准化；
- CompatibilityReport；
- 依赖、路径、digest、版权状态；
- Catalog 查询和版本选择。

验收：

- `software-company` 导入生成 5 个 AgentDefinition、1 个 AgentTeamDefinition；
- Team 成员关系和 Lead 正确；
- 不支持组件不静默丢弃；
- 路径逃逸和 digest 冲突阻断安装；
- 凭据值不进入 Snapshot。

### Phase 2：allo Runtime Adapter（两周迭代）

交付：

- 单 Agent Run；
- Preset/ResolvedPresetSnapshot；
- 固定 Team Participant Pool；
- AgentExecutionTemplate；
- Leader Conversation 创建与 `execution_template_id` 绑定；
- Leader 可调用的 `nomi_delegate(strategy=planned)` 入口（绑定持久 `AgentExecutionEngine`）；
- Planning Context 构造与摘要/digest；
- Step/Attempt 状态；
- Planning Context 驱动的 planned DAG、ready 调度、局部并行；
- retry/replan；
- Event/Artifact 持久化。

验收：

- 完成单 Agent Run；
- 完成 software-company 最小 TeamRun，并验证 Planning Context 摘要/digest 与成员 Prompt 隔离；
- Leader 触发的 TeamRun 能从内部 Execution 反查出公共 `run_id`，且成员池/并发上限不可被模型覆盖；
- 独立 Step 可局部并行；
- 依赖 Step 正确等待；
- 失败产生新 Attempt；
- replan 保留 Plan Revision 历史；
- Runtime 重启后保留事件，并将未完成 Run 明确标记为失败或需恢复。

### Phase 3：App Server Protocol（两周迭代）

交付：

- initialize/capability negotiation；
- Catalog API；
- agent/run、team/run；
- run/get、run/result 与尽力而为事件通知；
- cancel/pause/resume/retry/replan；
- Approval、Artifact、Connector/OAuth API；
- stdio JSONL 和 WebSocket；
- 结构化错误和幂等键。

验收：

- SDK/CLI 可只通过 App Server 完成单 Agent 和 Team Run；
- 状态查询始终一致；通知丢失不影响最终一致；
- 相同幂等键不重复创建 Run；
- 公共协议不泄露 allo 内部 ID 和凭据。

### Phase 4：TypeScript SDK（两周迭代）

交付：

- `@flowy-agent-store/protocol`；
- `@flowy-agent-store/client`；
- Node stdio Transport；
- Browser/WebSocket Transport；
- Catalog/Run/Event/Artifact/Approval/Connector Client；
- 重连、错误和幂等辅助；
- React hooks。

验收：

- Node 和 Browser 都能完成 initialize；
- SDK 能完成 Catalog、单 Agent Run、Team Run；
- 断线重连后状态一致且不重复展示；
- SDK 不接触真实 OAuth Token。

### Phase 5：Flowy/Web 纵向闭环（两周迭代）

交付：

- Agent/Team Catalog；
- Run Launch；
- Plan/DAG；
- Timeline（状态 + 通知驱动）；
- Approval；
- Artifact；
- Connector/OAuth 状态；
- Flowy 主进程和 Renderer 边界。

验收：

- 用户可从 UI 导入后启动 Agent/Team Run；
- 可看到计划、Step、Attempt、事件和 Artifact；
- QA 失败导致 retry/replan 后 UI 保留历史；
- OAuth 页面不接触 Token；
- Renderer 不直接调用 allo 或上游 MCP。

### Phase 6：安全加固与发布准入（两周迭代）

交付：

- Tool Policy 和 Approval 加固；
- MCP/STDIO 进程边界；
- 凭据泄露扫描；
- 脱敏审计；
- 连接器 Probe；
- 版权/来源审核；
- V1 回归测试和发布说明。

验收：

- 标准 OAuth Connector 通过登录、注入、刷新、Probe；
- 高风险操作有 Approval；
- 任意 CLI/脚本不能绕过 allowlist；
- 未确认版权资源不能进入公开分发。

## 4. 依赖与风险

| 风险 | 影响 | 缓解 |
|---|---|---|
| Preset 解析或 Runtime Agent 注册桥接不完整 | 单 Agent/Team 无法执行 | Phase 0 分别验证 AgentDefinition → Preset 快照和 Runtime Agent/Driver 实际调用；保留 Adapter 边界 |
| planned DAG 与现有执行器语义不一致 | Team 交付延迟 | 先固定最小 Step/Attempt/Event 模型，不提前承诺完整 Team |
| OAuth transport 注入未贯通 | Connector 只能登录不能调用 | 将 login 与 runtime integration 分开验收 |
| 通知丢失导致界面滞后 | 用户看到过期进度 | 以 run/get 轮询兜底；V2 再做 cursor 追平 |
| Hook/bin/script 执行风险 | 任意代码执行 | V1 默认静态导入和 manual-review |
| 来源版权未确认 | 不能公开分发 | 保留 pending-legal-review，阻断市场安装 |
| allo 内部 API 变化 | SDK/Web 返工 | 所有外部调用通过版本化 App Server |
| Runtime 重启中断 Run | 用户误认为执行成功 | 复用 allo 内部安全恢复；无法证明安全时标记 `recovery_required`，App Server 不承诺一定续跑原 Attempt |

## 5. 测试分层

### 5.1 Unit

覆盖：

```text
Manifest parser
ID mapping
Version/digest
Compatibility status
Policy intersection
DAG validation
Step readiness
Attempt state machine
Event sequence（内部序号）
Error mapping
OAuth metadata/state validation
```

### 5.2 Contract

覆盖：

```text
App Server initialize
Request/Response schema
Error codes
Event envelope
Status query consistency
Idempotency
SDK generated types
```

### 5.3 Integration

覆盖：

```text
Importer → Catalog
Catalog → Runtime Adapter
Adapter → allo
Runtime → App Server
App Server → SDK
Connector → OAuth → Probe
```

### 5.4 E2E

主链路：

```text
导入 software-company
    → 查看 5 个 Agent + 1 个 Team
    → 启动 TeamRun
    → Planning Context 驱动的 planned DAG
    → Step 顺序/局部并行
    → 失败 retry/replan
    → Event/Artifact
    → Web 显示结果
```

## 6. 测试与发布引用

测试用例唯一主表是 `agent-store-v1-test-cases.md`，发布门禁唯一详细定义是 `09-release-readiness.md`。本路线图只定义阶段依赖，不复制测试正文或发布清单。

阶段与测试映射：

```text
Phase 0 → TC-RT-001/005/006/009/010、TC-API-001、TC-OAUTH-003
Phase 1 → TC-IMP-*、TC-CONN-001
Phase 2 → TC-RT-*、TC-TEAM-*
Phase 3 → TC-API-*、TC-SDK-*
Phase 4 → TC-SDK-*
Phase 5 → TC-WEB-*
Phase 6 → TC-OAUTH-*、TC-CONN-*、TC-SEC-*
```

具体用例、输入、断言和证据要求统一维护在测试主表。

完整文档地图与权威顺序见 `README.md`。

> 注（2026-09-09）：本节原文档清单仅覆盖 `00`~`08` + roadmap + test-cases，已过期；`09`~`15`、证据页与设计记录未列入，以上方指针为准。

## 9. 排期说明

本文使用“两周迭代”作为验收节奏，不对总工期作承诺。实际排期必须在 Phase 0 完成以下实测后校准：

- Preset/ResolvedPresetSnapshot 写入 Participant，以及 Runtime Agent/Driver 实际创建链路；
- planned DAG 实际物化和执行；
- 持久化状态一致性、事件规范化和重启后的未完成状态标记；
- MCP OAuth transport 注入和刷新；
- App Server 两种传输的稳定性；
- software-company 端到端运行时间与资源占用。

## 10. 决策记录（2026-09-09 增补）

以下决策由用户拍板，作为后续实施依据，覆盖此前文档中的“待定/备选”表述；实施顺序见 `15-store-chain-and-protocol-vnext-plan.zh.md`。

1. **二进制分发走 npm optionalDependencies**（2026-09-09）：按平台发布 `@flowy-agent-store/runtime-<platform>-<arch>` 包，作为 `@flowy-agent-store/sdk` 的 `optionalDependencies`；`resolveAppServerBin` 查找顺序 `bin` → `AGENT_STORE_BIN` → `require.resolve` 定位 platform 包内二进制 → PATH（详见 `12-sdk-packaging.md` §6）。否决 GitHub releases + checksum 下载缓存方案。
2. **协议 vNext 完全重命名（thread/turn/item），stdio 不纳入**（2026-09-09）：V1 之后破坏性升级公共协议为 v2，概念模型对齐 Codex app-server（`run/conversation → thread`、`agent/run → turn*`、事件项归并为 `item`）；`initialize` 版本协商与 `dispatch_connection_request` 唯一分发保留；stdio 维持排除（SDK 仍走 spawn + 回环 WS）。该决策将重构 webui 事件层（`conversation-events` / `RunHandle` 等）与 SDK 方法面，需在 vNext 立项前先产出 Codex app-server spec diff（方法/事件/概念映射表）再动工；V1 冻结版本文档（`05`、`07`）标为 v1 基线。
3. **Team Run 改由 Leader 模型调用 `nomi_delegate(strategy=planned)` 触发**（2026-09-10）：撤销此前"Team Runtime 不依赖 `nomi_delegate` 工具"的表述（`00` §4.4、`04` §4.3、TC-TEAM-002）。`team/run` 由服务端创建 Leader Conversation，并把 Team 的 `AgentExecutionTemplate` 绑定为该会话的 `execution_template_id`；Leader 在该 Conversation 的 turn 内调用 `nomi_delegate(strategy=planned, goal=…)`，服务端据此构造 Planning Context 并调用内部 Planner 生成/物化 DAG。
   - 成员池、`max_parallel`、`routing_constraints` 与权限取自绑定的 Template 和服务端策略，不接受模型输入（不放松 `00` §1.2「不让模型直接决定权限、成员路由、状态迁移或审批结果」）。
   - 顶层仍不得以 `strategy=parallel` 代替 planned 流程；局部并行仍由已校验 DAG 中的独立 ready Step 表达。
   - 注册给 Leader 的必须是绑定真实 `AgentExecutionEngine`（具备持久化 Execution/Event/Attempt）的 planned 实现。仅支持 `strategy=parallel`、以同步无持久化方式投影的 embedded 实现（`nomi-agent::local_delegate_tool`）不得用于 Team Runtime；Store 会话必须关闭该实现，避免模型选中错误版本。
   - App Server 的公共 `run_id` 仍由 `AppServerRunMapping` 从内部 Execution 映射，模型不可见；Leader turn 产生的 Execution 通过 ConversationExecutionLink 反查。
   - 该决策把"Leader 必须有 Conversation/Attempt"从"暂不要求"变为"必须"（不必用户可见）。

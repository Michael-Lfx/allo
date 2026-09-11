# Agent Store 总体架构决策

> 状态：架构基线（Phase 0；发版前可改，非冻结，见 `16-sdk-webui-site-priority-plan.zh.md` §7 决策 4）；部分结论已实证（详见 `README.md` 索引）；发布阻断
> 日期：2026-08-26
> 修订：2026-09-10 — Team 触发方式改为 Leader 模型调用 `nomi_delegate(strategy=planned)`（见 `16-sdk-webui-site-priority-plan.zh.md` §7 决策 3）
> 范围：Agent Store / Runtime Platform 的架构边界、组件职责与取舍决策
> 口径：本文明确区分「源码/文档已证实」「设计决策」「待 Spike 验证」「暂不支持」四类结论

## 1. 背景与要解决的问题

### 核心术语原则（必须遵守）

Agent Store 的 Agent 是产品层定义，在 allo 中通过 Preset 机制承载；nomifun 的 Agent 才是 Claude Code、Codex 这类实际执行 Runtime。两者不能使用同一个 ID、类型或生命周期语义互相替代。

```text
Agent Store Agent/AgentDefinition
    → allo Preset/ResolvedPresetSnapshot
    → ExecutionParticipant.preset_snapshot
    → nomifun Runtime Agent/Driver
```

会议纪要（2026-08-25）确定构建 Agent Store：统一管理专家（Agent）、技能（Skill）、连接器（Connector），对外通过 SDK/CLI/MCP 集成，并优先解决自身产品（如 Flowy 5.2.0）的集成问题。

在推进前需要先定稿四件事，否则后续 SDK、Web、排期都会返工：

1. 执行 Runtime：已确定采用 allo（Rust），本文档固化该决策（AD-01），不再引入并行 Runtime 候选；
2. 当前方案以本目录中的架构基线、实现规格、路线图和测试用例为唯一依据；
3. V1 需要实现 AgentTeam，第一版采用「固定成员 + Leader 规划上下文驱动的结构化规划」的最小 Team Runtime；完整 Mailbox 等能力后续增强；
4. App Server / SDK / Web 谁是公共兼容边界。

## 2. 架构决策总表

| 编号 | 决策项 | 结论 | 依据 |
|---|---|---|---|
| AD-01 | 执行 Runtime | 已确定采用 allo Runtime（Rust）作为唯一执行引擎 | 已决策（2026-08-26）；会议纪要；体积/资源诉求；源码已具备 Agent Execution、Planner、Template、事件基础设施 |
| AD-02 | 公共兼容边界 | Versioned App Server Protocol；TS/Python SDK、CLI、MCP Adapter、Web 都是该协议的客户端 | 设计决策；避免 SDK/UI 反向定义系统语义 |
| AD-03 | allo Extension 定位 | allo 原生宿主扩展机制；不等同于 CodeBuddy Plugin | 源码已证实（manifest/registry/贡献类型）；CodeBuddy Plugin 为外部包格式 |
| AD-04 | CodeBuddy/WorkBuddy 内容 | 一律通过 Importer 转换，运行时不直接消费来源格式 | 设计决策；来源格式非运行时契约 |
| AD-05 | Agent Team | V1 实现固定成员 + Leader 规划上下文驱动的最小 Team Runtime；支持 planned DAG、局部并行、事件、重试和 replan；完整 Mailbox 等能力为后续增强 | 源码已证实 AgentExecutionTemplate、Planner/Materializer、Participant Router 能力边界；当前产品范围决策 |
| AD-06 | 云端 vs 本地 | 云端 Catalog 管定义/版本/分发；本地管执行/凭据/运行状态 | 会议纪要；安全边界 |
| AD-07 | V1 交付目标 | 完整功能核心（Agent/Team/Skill/Connector + App Server + SDK + Web/Flowy 集成），Team V1 限定为固定成员 + Leader Planning Context 驱动 planned DAG 的最小闭环 | 需求口径；当前产品范围决策 |
| AD-08 | 企业级能力 | 多租户、HA、灾备、全量 Marketplace、签名安装、全部 OAuth 变体、运营后台、安全认证为 V1 明确非目标 | 范围控制 |
| AD-09 | 凭据 | 凭据只存本地安全存储，定义/日志/前端状态只存引用；文档中一律 `[REDACTED]` | 安全要求 |
| AD-10 | Agent Team 触发方式 | Leader 模型在自有 Conversation 内调用 `nomi_delegate(strategy=planned)` 触发服务端 Planner；成员池/并发/权限来自绑定的 `AgentExecutionTemplate` 与服务端策略，不接受模型输入 | 设计决策（2026-09-10）；`16-sdk-webui-site-priority-plan.zh.md` §7 决策 3 |

## 3. 总体架构

```text
WorkBuddy / CodeBuddy Plugin（本地目录）
WorkBuddy Skill / Connector Marketplace
                │
                ▼
     Importer / Compatibility Layer
                │
                ▼
   PluginSnapshot（版本化、不可变、含来源与兼容性）
                │
                ▼
 Agent / Team / Skill / Connector Catalog
                │
                ▼
        allo Runtime Adapter
                │
                ▼
          allo Runtime（Rust）
                │
                ▼
    Versioned App Server Protocol
      ┌───────┬───────┬──────┬───────┐
      ▼       ▼       ▼      ▼       ▼
   TS SDK  Python SDK  CLI  MCP     Web/Flowy
```

各层职责：

- **Importer**：读取并转换 WorkBuddy/CodeBuddy 格式，输出标准化快照与兼容性报告。
- **Catalog**：管理 Agent/Team/Skill/Connector 定义、版本、来源、状态。
- **Runtime Adapter**：把 Agent Store 领域对象映射到 allo 执行语义；不让产品模型依赖 allo 内部结构。
- **App Server**：对外提供稳定、版本化、typed 的生命周期与事件协议。
- **SDK/CLI/MCP/Web**：协议客户端；不直接耦合 allo UI REST/WebSocket 或内部数据库。

## 4. 组件决策说明

### 4.1 为什么确定 allo 为 Runtime

已确定采用 allo（Rust）作为唯一执行引擎：打包单文件、依赖小、资源占用少，符合商店对轻量、自依赖的诉求；源码层面已有可复用基础：

- Agent Execution / Agent Execution Template；
- Planner、Plan Materializer、Participant Router/Resolver；
- `nomi_delegate`：普通可信会话的委派入口；`strategy=planned` 的结构化 DAG 规划能力同时是 V1 Team Runtime 的计划触发入口（由 Leader 模型调用）；
- 执行事件、序列、游标、审批、取消/暂停/恢复基础设施；
- Extension Registry、Skill 服务、MCP 配置与 OAuth 服务。

以上为源码已证实的现状，不等于全部运行链路完整；未逐项验证的部分在对应规格中标注。

### 4.2 App Server 为公共边界

- SDK、CLI、Web、MCP Adapter 都只依赖 App Server 协议；
- 协议定义版本化消息：Request / Response / Notification / Server Request；
- 公共 ID 使用 Agent Store 的 opaque ID，不暴露 allo 内部 ID（如内部 execution/session UUID）；
- Run 状态、结果和终态持久化并可查询；运行中的事件通知为尽力而为，允许丢失；
- 事件 cursor 分页、断线追平和时间线重放列为 V2 公共能力；
- 审批建模为 Server Request（要求客户端响应）。

参考 Codex App Server 的方向（initialize → thread/start → turn/start），但不复制其私有实现与私有字段。

### 4.3 allo Extension 与 CodeBuddy Plugin 的关系

- allo Extension（`nomi-extension.json`）是宿主声明式扩展系统：Agent/Preset/Skill/MCP/Theme/WebUI 等十类贡献、生命周期钩子、权限声明、风险分级、启用/禁用、hot reload；
- CodeBuddy Plugin 是可安装、可缓存、可启用的完整插件包（agents/skills/commands/hooks/.mcp.json/.lsp.json/bin 等）；
- 两者的 Manifest、生命周期、安全模型不同，**不直接做字段级兼容**；
- 通过 Importer 把 CodeBuddy Plugin 标准化为 PluginSnapshot，再决定哪些组件进入 allo 既有能力，哪些进入受控适配器，哪些标记 `manual-review`/`unsupported`；
- 关键安全事实：allo 生命周期钩子会执行外部解释器进程；权限声明与风险标签不是沙箱。未经验证不得把来源内容当可执行插件加载。

### 4.4 Agent Team 的 V1 边界

V1 Team 运行形态（模式 A，最小可行实现）：

```text
AgentTeamDefinition（固定成员名册 + 硬策略）
        ↓
为 Leader 和每个成员解析独立 Preset/ResolvedPresetSnapshot
        ↓
AgentExecutionTemplate / 固定 Participant 池
        ↓
构造 Planning Context
（Leader 规划指令 + 脱敏成员能力摘要 + Team 策略）
        ↓
Planner/LlmPlanProducer 生成结构化 DAG
        ↓
校验成员路由、依赖、权限和并发限制
        ↓
Execution 事件、重试、replan
```

这里的 **Leader** 是 `lead_agent_id` 指向的规划角色，必须有一个承载工具调用的 allo Conversation/Attempt（不必是用户可见的独立会话）。TeamRun 创建时把 Team 的 `AgentExecutionTemplate` 绑定为该 Conversation 的 `execution_template_id`；Leader 在该 Conversation 的 turn 内调用 `nomi_delegate(strategy=planned)` 触发计划生成，Leader Preset 的规划指令与 Team 策略进入 Planning Context。每个成员的完整 persona、Skill、Connector 和 Tool Policy 只进入该成员自己的 Participant/Attempt Snapshot。规划器可以看到经过筛选的成员能力摘要，但不得依赖完整成员 Prompt 或自然语言自行授予权限。

V1 必须支持：

- 固定成员 Participant 池；
- 由 Leader 规划指令和 Team 目标驱动的结构化计划生成；
- DAG 依赖和 ready step 调度；
- 独立步骤的局部并行；
- 失败后的重试和 replan；
- Team/Step/Attempt/Event/Artifact 的持久化与观测。

V1 暂不要求：

- 用户可见的独立 Leader Conversation（Leader 仍必须有一个非用户可见的 Conversation/Attempt 承载工具调用，见下）；
- 完整 Mailbox；
- 成员自主认领任务；
- 成员之间任意直连消息；
- 成员长期独立会话生命周期；
- 嵌套 Team。

Team Runtime **依赖** `nomi_delegate(strategy=planned)` 作为计划生成入口（2026-09-10 修订，见 `16-sdk-webui-site-priority-plan.zh.md` §7 决策 3）：`team/run` 由服务端创建 Leader Conversation 并绑定 Team 的 `AgentExecutionTemplate`，Leader 模型在该 Conversation 的 turn 内调用 `nomi_delegate(strategy=planned, goal=…)`，服务端据此构造 Planning Context 并调用内部 Planner 生成/物化 DAG。成员池、`max_parallel`、`routing_constraints` 与权限来自绑定的 Template 和服务端策略，不接受模型输入。

顶层仍不得使用 `strategy=parallel` 代替 Team planned 流程；局部并行由已校验 DAG 中的独立 ready 步骤表达。

注册给 Leader 的必须是绑定真实 `AgentExecutionEngine`（具备持久化 Execution/Event/Attempt）的 planned 实现；仅支持 `strategy=parallel`、以同步无持久化方式投影的 embedded 实现不得用于 Team Runtime。

## 5. 导入边界

```text
来源格式（.codebuddy-plugin/、.codebuddy-skill/、.codebuddy-connector/）
        ↓
路径校验（相对路径、遍历、符号链接）
        ↓
版本化不可变缓存（含 digest）
        ↓
PluginSnapshot
        ↓
标准化定义（Agent/Team/Skill/Connector/Command/Hook/LSP/CredentialSchema）
        ↓
CompatibilityReport（compatible / compatible-with-adapter / manual-review / unsupported / pending-legal-review）
```

禁止：

- 运行时直接读来源目录（应使用导入后的快照）；
- 把来源 slug 直接当作 allo 业务 ID（需 ID 映射，如 `wb-<plugin>-<slug>`）；
- 静默丢弃不支持的组件；
- 未完成版权确认的资源进入公开市场或默认安装包（标记 `pending-legal-review`）。

## 6. 安全边界（原则）

- 入站（外部客户端 → Agent Store）与出站（Agent Store → 上游 Connector）认证/Token 严格分离；
- 凭据只进入安全存储；SDK/Web 只接触状态、账号标识、过期时间与错误，不接触真实 token；
- 有效权限 = caller ∩ agent ∩ team ∩ skill ∩ connector ∩ credential scopes；
- 写操作（发送、发布、删除、部署、支付）要求显式策略与审批；
- Hook/LSP/任意脚本/bin 执行在安全模型落地前默认不启用。

## 7. 待 Spike 验证项（写代码前必须回答）

1. allo 能否以独立进程/服务方式稳定跑完单 Agent 执行（Release 构建、冷启动、资源占用实测）；
2. AgentDefinition → allo Preset/ResolvedPresetSnapshot 的桥接是否完整；实际 Runtime Agent/Driver 的运行注册链路另行核实；
3. MCP OAuth 全链路：登录 → 凭据存储 → 请求时注入 → 401 刷新 → 一次重试，是否真实贯通（现状：登录/存储已有，注入需验证）；
4. 固定 Template 绑定的 Planning Context + planned Planner 是否可按预期产出动态 DAG；
5. 本地 App Server 传输（WebSocket；stdio JSONL 为 V2/deferred、V1 不纳入）的稳定性，以及断线/重连后状态重对齐的行为。

Spike 结论将回写本文档状态。

## 8. V1 范围与验收边界

### 8.1 V1 必须实现

| 能力 | V1 范围 | 最低验收条件 |
|---|---|---|
| allo Runtime | 唯一执行引擎 | 可启动、创建并完成 Agent Run |
| Agent Catalog | 导入、版本、来源、启用状态 | `software-company` 的 5 个 Agent 可查询 |
| Skill Catalog | 导入、引用、加载 | Agent Run 可加载绑定 Skill |
| Connector Catalog | MCP Connector 定义、工具过滤、状态 | 至少一个 Connector 可发现工具并执行受控调用 |
| 单 Agent Run | 异步执行、取消、事件、结果 | 返回公共 `run_id`；状态与结果可查询；事件通知尽力而为 |
| AgentTeam | 固定成员 + Leader Planning Context 驱动 planned DAG | `software-company` 可创建 TeamRun 并执行顺序/局部并行步骤 |
| Team 反馈 | 重试、replan、Attempt 结果 | Step 失败后可重试或生成新的计划修复 |
| App Server | 版本化协议与本地传输 | SDK/CLI/Web 不依赖 allo 内部 UI API |
| TypeScript SDK | 协议 typed client | 可完成 Catalog、Run、Event、Artifact 调用 |
| Web/Flowy | 基于 SDK 的目录和运行观测 | 可启动 Run、显示事件、结果和 Artifact |

### 8.2 V1 后续增强

```text
服务端崩溃后续跑原 Attempt（含 Intent、Projection CAS、lease/fencing 和恢复扫描）
Run 事件 cursor 分页、断线追平和完整时间线重放
完整 Mailbox
成员自主认领任务
成员直连消息
长期成员会话
嵌套 Team
复杂 OAuth
任意 CLI/脚本执行
完整 LSP/Hook Runtime
```

### 8.3 V1 发布阻断条件

以下任一条件未满足，不得宣称对应能力已完成：

- AgentDefinition 无法解析为可执行的 Preset/ResolvedPresetSnapshot，或无法写入 allo ExecutionParticipant；
- Team 无法完成固定成员、Planning Context 驱动的 planned DAG、重试和 replan 的最小闭环；
- 终态事件未持久化，或重启后未完成 Run 没有被如实标记为 `recovery_required`/`failed`；
- Connector 凭据无法在请求时安全注入；
- 真实凭据出现在日志、Prompt、Tool Result、Renderer 状态或公共响应；
- 未确认版权的来源资源被放入公开市场或默认安装包。

## 9. 与其他文档的关系

- 本决策文档是后续所有文档的基线；
- 领域模型见 `01-domain-model.md`；
- 导入规范见 `02-codebuddy-workbuddy-import-spec.md`；
- 兼容性矩阵见 `03-codebuddy-compatibility-matrix.md`；
- 当前目录中的架构基线、实现规格、路线图和测试用例共同构成 Agent Store 当前方案，不再引用旧版主方案。

## 10. Phase 0 基线与验证边界

本文档及当前目录的公共契约构成 Phase 0 的**现行基线**（发版前可改、非冻结，见 `16-sdk-webui-site-priority-plan.zh.md` §7 决策 4）。后续只有以下证据可以触发修改：

```text
源码事实与文档不一致
Spike 验证失败
测试发现公共契约缺口
```

基线不代表能力已经实现：

```text
Architecture = baseline (pre-release, editable)
Implementation = not yet verified
Runtime readiness = not verified
Release readiness = blocked
```

Phase 0 的最小验证顺序：

```text
Agent Store Agent/AgentDefinition → Preset/ResolvedPresetSnapshot
    → allo ExecutionParticipant → Runtime Agent/Driver
    → 单 Agent Run → Event Log → App Server
    → Projection/未完成 Run 状态标记
    → Credential Provider → Transport 注入 → Probe → 401 刷新
```
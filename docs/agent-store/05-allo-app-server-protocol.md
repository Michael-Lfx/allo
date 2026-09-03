# allo App Server Protocol 规格

> 状态：架构冻结（Phase 0）；单 Agent 模式已实现并通过聚焦验证（Workspace Resolver、持久化幂等、WebSocket 实时事件推送）；Skill/Connector 目录能力（skill/*、connector/*、OAuth 状态透传）已启用并接入 agent/run 运行时接线；Team 能力保持关闭；跨进程崩溃的严格 exactly-once 与端到端联调待发布前验证
> 日期：2026-08-26
> 前置：`00-architecture-decision.md`、`01-domain-model.md`、`04-allo-runtime-adapter.md`
> 目标：建立 SDK、CLI、MCP、Web/Flowy 的唯一公共兼容边界

## 1. 协议定位

```text
Agent Store（Catalog / 凭据 / 审批 / 策略等领域服务）
    ↓ 建立在
Versioned App Server Protocol（唯一公共协议层）
    ↓ 被
TypeScript SDK / Python SDK / CLI / MCP Adapter / Web / Flowy 消费

实现内部：App Server ⇄ Runtime Adapter ⇄ allo Runtime（唯一执行引擎）
```

当前 App Server Runtime Spike 的 Agent Run 入口只支持 `builtin-office` Preset，且该 Preset 必须解析为 `agent_type=nomi`。Claude Code、Codex 等 ACP Runtime 仍可由 allo 原有会话路径使用，但不属于当前 Agent Store/App Server Run 入口。

协议客户端不得直接依赖：

- allo 内部数据库；
- allo UI REST 路由；
- allo UI WebSocket 事件形态；
- allo 内部 Execution/Conversation/Participant ID；
- provider 私有事件和工具结构。

## 2. 传输与消息

### 2.1 V1 传输

V1 先支持：

```text
stdio JSONL：本地 CLI/SDK/子进程集成
WebSocket：本地 Web/Flowy 实时集成
```

后续可增加 HTTPS/远程部署，但不改变消息语义。

### 2.2 消息类型

```text
Request          客户端请求
Response         请求响应
Notification     服务端异步通知，无需响应
Server Request   服务端向客户端请求审批/输入/确认
```

基础信封：

```json
{
  "jsonrpc": "2.0",
  "id": "req_01...",
  "method": "run/get",
  "params": {}
}
```

事件通知（实现形状）：

```json
{
  "jsonrpc": "2.0",
  "method": "event",
  "params": {
    "run_id": "run_01...",
    "sequence": 12,
    "event_type": "task.updated",
    "payload": {}
  }
}
```

`run_id` 是 Agent Store opaque public Run ID（UUIDv7），`sequence` 是该 Run 的持久事件序号。客户端必须使用 `sequence` 排序/去重，并以 `run/get`、`run/result` 或 `run/events` 作为权威恢复来源；事件通知为尽力而为，断线期间的通知不保存、不补发。

公共 ID 只使用 Agent Store opaque ID；`agent_id`/`agentId` 在 Catalog 和 Run 输入中指 Agent Store AgentDefinition ID，不表示 nomifun Runtime Agent 身份。示例 ID 不代表固定实现格式。

## 3. 初始化与能力协商

### 3.1 initialize

客户端必须先发送：

```json
{
  "method": "initialize",
  "params": {
    "protocol_version": "2026-08-26",
    "client": {"name": "agent-store-cli", "version": "0.1.0"},
    "auth": {
      "mode": "local-session",
      "credential": "opaque-session-reference"
    },
    "capabilities": {
      "events": true,
      "approvals": true,
      "team_runtime": true,
      "artifacts": true
    }
  }
}
```

服务端返回：

```json
{
  "protocol_version": "2026-08-26",
  "server": {"name": "allo-agent-store", "version": "0.1.0"},
  "auth_context": {
    "principal_id": "principal_01...",
    "issuer": "local-agent-store",
    "audience": "agent-store",
    "scopes": ["catalog:read", "run:write", "run:read"]
  },
  "capabilities": {
    "agents": true,
    "teams": false,
    "team_runtime": false,
    "skills": false,
    "connectors": false,
    "run_notifications": true,
    "approvals": false,
    "artifacts": false,
    "oauth": false
  }
}
```

`run_notifications` 仅在 WebSocket 传输且服务端事件源可用时为 `true`；`agents` 在 Runtime 或 Agent Catalog provider 注入时为 `true`。`skills`/`connectors`/`oauth` 仅在对应 Catalog/OAuth provider 注入时启用（生产装配注入系统 Skill/MCP 服务适配器）；`imports` 仅在 Importer provider 注入时启用，`teams` 仅在 Team Catalog provider 注入时启用；未注入时对应方法返回 `unsupported_operation`。Approval、Artifact 能力在当前单 Agent Phase 保持 `false`，由后续 Phase 逐个启用。

`team_runtime=true` 仅表示 V1 最小 Team Runtime：固定成员、Planning Context、planned DAG、局部并行、retry/replan；不表示完整 Mailbox/成员直连能力。

V1 默认使用本地可信进程模型：stdio 或 localhost WebSocket 由主进程建立 `LocalPrincipal`；Renderer 不直接构造 Principal，只能通过主进程/SDK 访问。未来支持远程调用时，必须增加独立认证流程，不得把本地 session reference 当作远程身份凭据。

认证失败返回 `unauthenticated`、`invalid_issuer`、`invalid_audience` 或 `insufficient_scope`；未建立 AuthContext 的连接只能调用 `initialize` 和能力探测。

### 3.2 initialized

客户端收到成功响应后发送 `initialized` 通知，之后才能调用业务方法。

协议版本不兼容时返回 `protocol_version_unsupported`，不得静默降级到未知字段。

## 4. Catalog 方法

### 4.1 Agent（Agent Store AgentDefinition）

```text
agent/list
agent/get
```

`agent/get` 返回的是 Agent Store AgentDefinition，而不是 Claude Code/Codex 等 Runtime Agent 实例。当前 Runtime Spike 仅允许其中的 `builtin-office` Preset 进入 Agent Run：

```text
id / version / name / description / preset_id
skills / connectors / model_summary / tool_policy_summary
source / compatibility_status
display_name / profession / avatar_url            （market 展示元数据，localized）
display_description / quick_prompts / tags / default_init_prompt / expert_type / category_id（detail only）
```

`preset_id` 在 `install/*` 后非空（@mention 解析用）。`display_name`/`profession`/
`display_description`/`quick_prompts`/`tags`/`default_init_prompt` 为双语文案
（`{en, zh}`，源为 `plugin.json`，agent frontmatter 的 `displayName`/`profession`
作为缺失时的回退）；`avatar_url` 指向服务端带校验的快照资产端点
（`/api/app-server/imports/{snapshot}/assets/{path}`，仅白名单图片类型，
永不暴露 prompt 文件）。

不返回真实凭据、隐藏系统指令或任意内部 Prompt（除非调用方策略允许且产品明确需要）。

### 4.2 Team

```text
team/list
team/get
```

Team 详情至少返回：

```text
id / version / name / description
lead_agent_id（规划角色）
member_agent_ids
planner_policy
routing_constraints
workflow_limits
compatibility_status
team_runtime_capabilities
```

V1 Team Runtime 能力（模式 A）：

```text
fixed_members
planning_context
planned_dag
local_parallel
retry
replan
events
artifacts
```

`planning_context` 是服务端为本次 TeamRun 派生的内部规划输入；公共响应最多返回 `planning_context_digest`，不返回上下文正文、成员完整 Prompt 或凭据。

后续能力：

```text
mailbox
member_claim
member_direct_message
long_lived_member_session
nested_team
```

### 4.3 Skill 与 Connector

```text
skill/list
skill/get
connector/list
connector/get
connector/status
```

Connector 返回工具摘要、认证状态和策略摘要，不返回 Token/API Key。工具名必须为命名空间后的公开名称。

### 4.4 Import 与 PluginSnapshot（roadmap Phase 1）

导入是**带本地路径的一次性操作**，走 HTTP 辅助通道（与 workspace 注册同构，见 05 §5 的 HTTP 直连模式；每次调用独立握手）：

```text
POST /api/app-server/imports          # import/run  {@link AppServerImportRequest}
GET  /api/app-server/imports          # import/list（最近 50 条）
GET  /api/app-server/imports/{snapshot_id}   # import/get（组件与三态兼容报告）
```

请求体：

```text
{ "source_path": "<本地目录绝对路径>", "source_kind": "codebuddy-plugin" |
  "workbuddy-skill-market" | "workbuddy-connector-market" |
  "workbuddy-cli-connector" }
```

响应（`AppServerImportResult`）：

```text
snapshot_id / name / version / source_kind
status            # completed | completed-with-warnings | blocked | failed
content_digest    # sha256 树摘要（02 §9）
component_status  # {semantic_status, runtime_status, distribution_status, reasons[]}
component_count / imported_at / reused
warnings[] / errors[]
```

规则：

- 相同 `plugin_id + version + content_digest` 重复导入 → `reused=true`，返回既有不可变快照；
- digest 冲突 → `status=blocked`，不覆盖既有快照；
- 绝对来源路径只入库（`source_uri`，仅内部追溯），**不出现在任何公共响应**（02 §9）；
- 清单相对路径/相对文件路径可出现在 errors/warnings，用于定位问题组件。

`agent/list`、`agent/get`（§4.1）与 `team/list`、`team/get`（§4.2）在 Importer 注入后由
`plugin_snapshot_components` 提供数据：`agent/get` 只返回结构化 frontmatter 字段
（model/effort/maxTurns/tools/disallowedTools/skills/memory/isolation…），不返回原始
Prompt；`team/get` 只返回 lead/members/策略与 `team_runtime_capabilities`，不返回
Planning Context 正文。

### 4.5 Install 与运行时注册（roadmap Phase 2）

导入只登记不可变快照；**安装**把快照组件注册进运行时，使其真正可用。安装走与导入相同的
HTTP 辅助通道：

```text
POST /api/app-server/installs                      # install/run  {@link AppServerInstallRequest}
GET  /api/app-server/installs/{snapshot_id}        # install/status（逐组件状态）
POST /api/app-server/installs/{snapshot_id}/disable   # install/disable（组件 id 列表）
POST /api/app-server/installs/{snapshot_id}/enable    # install/enable
POST /api/app-server/installs/{snapshot_id}/uninstall # install/uninstall（快照行保留）
```

请求体（`install/run`）：

```text
{ "snapshot_id": "<已导入快照 id>" }
```

`install/status` 响应（`AppServerInstallStatus`）：

```text
snapshot_id
components[] { id / kind / name / state / runtime_location / preset_id }
state: "not-installed" | "installed" | "disabled"
```

运行时注册映射：

| 组件 kind | 运行时目标 | 说明 |
|---|---|---|
| `skill` | 服务端技能根 `{data_dir}/skills/agent-store/{snapshot_id}/{slug}/` | 系统 skill 扫描可发现；拷贝不执行 |
| `agent` / `team` | Preset（`PresetService.create`，名称 `agent-store: <name>`） | 预设列表可直接使用；不创建 ExecutionTemplate |
| `connector` | `mcp_servers` 表（`McpConfigService.add_server` 按名 upsert） | 运输层由组件 payload 的 `transport_summary` 推导 |

规则：

- 安装只复制/注册，**不执行**任何内容（02 §10）；凭据值永不进入安装状态列；
- `uninstall` 移除运行时产物并清除组件安装状态，**快照与组件行保留**（历史可追溯）；
- `enable`/`disable` 只翻转 `disabled` 位（运行时产物保留）；
- 文档化状态机：`not-installed → installed → disabled →（enable）installed`；
  卸载任意时刻可用。

### 4.6 Marketplace（roadmap Phase 2）

市场（Marketplace）是**插件目录**：先添加市场，再浏览/导入/安装其条目（CodeBuddy
语义：两步流程）。源类型：

- `directory`：本地目录（Phase A）；
- `github`：GitHub 仓库（`owner/repo`，Phase B）；
- `git`：任意 Git 仓库（HTTPS/SSH URL 或本地 `.git` 路径，Phase B）；
- `url`：HTTP(S) `marketplace.json`（Phase B）。

能力协商：`capabilities.marketplaces`。

```text
POST   /api/app-server/markets                     # market/add     {@link AppServerMarketplaceAddRequest}
GET    /api/app-server/markets                     # market/list（注册表投影，无条目）
GET    /api/app-server/markets/{marketplace_id}    # market/get（含发现条目）
POST   /api/app-server/markets/{marketplace_id}/remove          # market/remove（默认 cascade=true）
POST   /api/app-server/markets/{marketplace_id}/auto-update     # market/auto-update {enabled}
POST   /api/app-server/markets/{marketplace_id}/refresh         # market/refresh（fetch + 投影重建）
POST   /api/app-server/markets/{marketplace_id}/entries/{entry}/import  # 条目导入（复用 import 管线）
```

`market/add` 请求体：

```text
{ "name": "<可选稳定名>", "source_kind": "directory|github|git|url", "source": "<源>" }
```

- `marketplace_id` 从 `name` 推导（无则源 basename），kebab-case 稳定名（内部唯一）；
- 同源重复添加幂等：返回既有市场行，不报错；
- 远端源（github/git/url）添加时**同步获取**：克隆/下载到 staging → 完整清单校验 →
  原子晋升到 live 根（backup+rename）；失败不注册且 last-good 不被触碰。

`market/refresh` 响应（`AppServerMarketplaceRefreshResult`）：

```text
{ marketplace_id, changed: bool, resolved_revision, entry_count, warnings[] }
```

- Git：重新解析 HEAD commit；与已记录 revision 相同 → `changed=false`（no-op），
  stale staging 丢弃；
- URL：带 `If-None-Match` 条件请求；`304` → `changed=false`；200 时校验清单并比较
  ETag/Last-Modified 摘要，相同 → no-op；
- `directory`：重新探测目录，内容摘要变化才更新投影；
- `resolved_revision` 仅内部追溯，不是公共身份标识。

远端源获取语义（对齐 CodeBuddy 文档）：

- 克隆/下载先落独立 staging 目录，`marketplace.json` 清单与完整条目树校验通过后才
  原子替换 live 根（backup → rename → 清理 backup）；失败保留旧 live（last-good）；
- URL 市场是 catalog 分发：只有清单内联条目可导入；含 `skills`/`commands` 文件树或
  声明外部源（GitHub/NPM）的条目标记 `source_kind=external`，导入返回 `bad_request`
  （提示把该源作为独立市场添加）——不把只下载到清单的条目误当完整插件目录；
- Git 市场条目在 clone 出的完整树内解析相对路径（含 `skills/`、`hooks/` 等目录）。

`market/get` 条目字段：

```text
name / source_kind（directory|external）/ source / version / description / keywords / category
```

`market/remove` 请求体 `{ "cascade": true }`（默认 true）。响应
（`AppServerMarketplaceRemoveResult`）：

```text
{ marketplace_id, snapshots: [快照 id], uninstalled_components: [组件 id], warnings: [] }
```

规则：

- **级联卸载（默认）**：市场中被导入过的快照，其已安装组件全部卸载（复用
  `install/*` 的 uninstall 语义）；快照与组件行保留（历史可追溯），provenance 清除；
- 市场注册行软删除（`removed_at`），`market/list` 不再出现；已安装资源随级联清理；
- 条目导入经 `/entries/{entry}/import` 走标准导入管线，并把
  `marketplace_id + entry_name`（远端再附 `resolved_revision`）记为快照 provenance
  （内部溯源，不出现在公共响应）；
- 市场源路径与条目 `source` 只在服务端解析；公共响应不含绝对路径；
- `auto-update` 开关仅记录（第三方默认关闭），Phase B 无后台自动刷新任务
  （手动 `market/refresh` 触发同一 fetch 管线）。

### 4.7 Mentions（@专家 / @技能 / @连接器，roadmap Phase 2 扩展）

Composer 的 `@` 引用以**结构化 mention** 传入 `agent/run`，客户端不发原始
文本（服务端不解析 `@` 语法）：

```json
{
  "goal": "总结仓库并生成 release notes",
  "mentions": [
    {"kind": "agent", "id": "wb-demo-software-architect"},
    {"kind": "skill", "id": "wb-demo-release-notes"},
    {"kind": "connector", "id": "0190f5fe-...-000000000020"}
  ]
}
```

每类 mention 的运行时语义：

| kind | 注入点 | 约束 |
|---|---|---|
| `agent` | 通过 `agent/get` 的 `preset_id` 选择运行 preset（替换 `agent_id` 字段）；overrides 沿 preset resolve 面展开 | 至多一个；target AgentDefinition 必须已 `install/*`（否则 `agent_not_installed`）；与显式 `agent_id` 冲突返回 `invalid_mentions` |
| `skill` | 挂载到 `included_skills`（冻结进 `ResolvedPresetSnapshot`，随 run 上下文交给 Agent） | 只记录/挂载；不执行、不展开正文 |
| `connector` | 追加到 `mcp_server_ids`（经既有的 connector 存在+enabled 校验后注入 run） | 必须是存在的已启用 MCP server |

规则：

- `mentions` 可省略（向后兼容），此时行为与旧 `agent/run` 一致；
- 未知 kind 反序列化失败（严格 wire 契约）；
- agent-store 安装 preset（`agent-store: <name>` 命名）在 `validate_agent_store_preset_source`
  白名单内（Builtin+builtin-office 保持不变）；任意用户 preset 仍被拒绝；
- preset 未绑定 model 时，服务端回退到 owner 的第一个启用 provider/model
  （`default_run_model`），避免 `resolved_model=None` 在运行时边界被拒。

## 5. Thread 与 Run

### 5.1 Thread

交互语义采用：

```text
Thread → Turn → Item
```

方法：

```text
thread/start
thread/get
thread/resume
thread/close
turn/start
turn/interrupt
```

Thread 是对话容器；不要把 allo Conversation ID 直接作为公共 Thread ID。

### 5.2 Run

执行语义采用：

```text
Run → Step → Attempt → Artifact
```

方法：

```text
agent/run
team/run（延后）
run/get
run/result
run/cancel
run/subscribe
run/unsubscribe
run/events（cursor 恢复）
run/pause
run/resume
run/retry
run/replan
```

`run/subscribe` / `run/unsubscribe` 订阅或取消订阅单个 public Run 的实时事件流，请求均为 `{"run_id": "run_01..."}`。订阅要求该 Run 已存在且属于当前用户；取消订阅同样要求持有该用户自己的公共 mapping。订阅只影响尽力而为的通知推送，不影响持久化状态查询。

`run/events` 支持 `{"run_id", "after_sequence", "limit"}` 游标读取，客户端用它追平断线期间遗漏的事件。

### 5.2.1 Workspace

```text
POST /api/app-server/workspaces   注册并返回 opaque workspace id
GET  /api/app-server/workspaces   列出当前 owner 的 active workspaces
DELETE /api/app-server/workspaces/:id  移除（注销）当前 owner 的一个 active workspace
WS   workspace/list               同上（JSON-RPC）
WS   workspace/create             校验并注册一个 owner 指定的本地目录绝对路径
WS   workspace/revoke             移除（注销）owner 的一个 workspace
```

- 客户端从不提供文件系统路径。服务端在受控注册表 `{work_dir}/app-server-workspaces/{uuidv7}` 下创建目录，并在 owner 作用域内持久化注册；注册表路径拒绝符号链接/重解析点，任何逃离注册表的解析一律 `workspace_denied`；
- `workspace/create` 允许 owner 提供**本地目录的绝对路径**（WebUI 的“输入文件夹路径”场景）。服务端先 `canonicalize` 并拒绝符号链接/重解析点/相对路径/不可访问目录，再按 `(owner, canonical root)` 幂等注册；同一路径复用同一 workspace，`workspace/list` 只返回当前 owner 的 active 行（revoked/foreign 一律不出现）；
- `workspace/revoke`（HTTP `DELETE /api/app-server/workspaces/:id`）将 owner 的 active workspace 置为 `revoked`（软注销，复用既有 `status` 列，无 schema 变更）：从 `workspace/list` 消失，也不再可作为新建 run/会话的 workspace 目标（服务端返回 `workspace_denied`）。**不删除会话、不删磁盘目录**：会话的 `extra.workspace_id` 保留；owner 用同一路径重新调用 `workspace/create` 时，服务端复用同一 `workspace_id` 重新激活，该工作区下的会话重新可见（WebUI 侧在移除后隐藏这些会话）。
- 公共 `WorkspaceView` 返回 opaque `workspace_id`、目录名派生的 `name` 与 `canonical_path`；`canonical_path` 只出现在同一 owner 的已认证响应中，绝不进入事件或跨 owner 数据；
- `conversation/create` 可选携带 `workspace: { id }`（必须为当前 owner 的 active workspace），会话创建时将 `workspace_id` 写入 App Server 专用会话元数据；WebUI 按此字段把会话分组展示。旧会话没有该字段时前端归入“未归类”，不做路径猜测；
- `workspace.id` 是 owner 作用域的 opaque UUIDv7，绝不解释为路径；revoked 或非本 owner 的 workspace 一律拒绝；
- `agent/run` 中 `workspace` 与裸 `work_dir` 互斥；注册了 workspace 策略后，裸 `work_dir` 不再被接受。

`agent/run` 示例：

```json
{
  "agent_id": "agent_01...",
  "agent_version": "1.0.0",
  "input": {"text": "..."},
  "workspace": {"id": "ws_01..."},
  "idempotency_key": "client-op-01",
  "mentions": [{"kind": "skill", "id": "wb-demo-release-notes"}]
}
```

`team/run` V1 示例：

```json
{
  "team_id": "team_01...",
  "team_version": "1.0.0",
  "goal": "完成目标",
  "workspace": {"id": "ws_01..."},
  "planning": {
    "mode": "planned",
    "adaptation_policy": "adaptive",
    "plan_gate": "automatic",
    "max_parallel": 4
  },
  "idempotency_key": "client-op-02"
}
```

Team 运行参数不能突破 TeamDefinition、调用方和 Runtime 的限制；服务端计算有效值的最小交集。

所有 Run 启动方法返回异步 receipt：

```json
{
  "run_id": "run_01...",
  "status": "queued"
}
```

不阻塞等待长任务完成。

### 5.3 Command 公共字段

所有有副作用的请求必须支持：

```text
command_id
idempotency_key
expected_version（可选）
principal_id（由服务端 AuthContext 生成，不信任客户端覆盖）
```

服务端先执行鉴权/策略校验，再检查幂等记录，最后持久化 Intent 并产生领域事件。`idempotency_key` 的作用域为 `principal_id + client_id + method`；请求指纹为规范化 JSON 的哈希，至少保留到 Run 进入终态后。相同 key 和相同指纹返回原 receipt，不同指纹返回 `idempotency_conflict`。

## 6. Run 状态查询与通知

### 6.1 V1 范围

V1 只承诺持久化状态与结果的查询：

```text
run/get      当前 Run/Step/Attempt 状态
run/result   终态与产出
```

对于 `team-run`，`run/get` 或 `run/result` 可返回 `planning_context_digest` 及规划角色的公共 Agent ID，用于审计和复现；不得返回 Planning Context 正文、成员完整 Prompt、真实凭据或 allo 内部 ID。

运行中事件通过连接内的实时通知推送（尽力而为）：断线期间的通知不保存、不补发；客户端重连后以 `run/get` 同步权威状态。

### 6.2 规则

- 状态与终态持久化是硬承诺；事件通知只是加速刷新的手段；
- 断线或丢通知不构成协议级缺口：客户端用 `run/get`/`run/result` 对齐，用 `run/events` 追平缺失事件；
- 通知携带 `sequence` 供排序与去重，语义允许重复与乱序容忍（V1 不承诺单调递增投递）；
- 事件流只为「已订阅且存在用户自己的 public mapping」的 Run 推送；内部 execution/session/step/attempt ID 与未映射事件绝不进入公共消息；
- 服务端事件流 lag 或事件不可读时推送 `run/resync-required`（带 `run_ids` 与 `reason`），客户端以 `run/events` 恢复；
- 高频瞬时事件（如 leadThinking delta，不属于持久 outbox 序列）V1 不推送；
- 连接断开后连接 ID 立即失效，不得继续用于 HTTP 业务调用；
- 重启后未完成 Run 必须标记 `recovery_required`/`failed`，不伪装 completed；
- V1 不提供 `after_cursor` 事件追平与时间线重放；`run/events` cursor API 连同跨 Run 订阅列为 V2；
- 事件数据不得包含真实凭据。

## 7. Approval 与 Server Request

Approval 同时具有事件和 Server Request 两种表示：`approval.required` 是可重放事实事件，Server Request 是要求客户端作出响应的交互消息。两者必须共享同一个 `approval_id`。

Server Request 最小字段：

```text
request_id
method: approval/request
approval_id
run_id / step_id / attempt_id / tool_call_id
resource_summary
argument_digest
allowed_decisions
expires_at
```

高风险操作由服务端发起：

```text
approval.required
```

客户端响应：

```text
approval/respond
```

```json
{
  "approval_id": "approval_01...",
  "decision": "approved",
  "reason": "用户确认"
}
```

决策值：

```text
approved
rejected
expired
```

`approval/respond` 必须携带 `request_id`、`approval_id`、`decision` 和 `idempotency_key`。服务端必须在 Runtime 层再次校验审批对应的资源、工具、参数摘要和过期时间；不能只相信客户端提供的 approval_id。重复响应返回同一结果，过期或已消费的请求返回 `approval_expired` 或 `approval_already_resolved`。

## 8. Artifact

```text
artifact/list
artifact/get
```

只返回受控 Artifact 元数据和下载/读取能力，不提供任意文件路径读取。

Artifact 必须绑定：

```text
run_id
workspace_id
relative_path
media_type
size
sha256
```

## 9. Connector OAuth

```text
connector/auth/status
connector/auth/start
connector/auth/logout
connector/test
```

`connector/auth/start` 返回登录状态和一次性授权 URL/会话 ID；不返回 Token。OAuth 的浏览器回调和凭据存储由可信主进程/服务端完成，Renderer/SDK 只处理状态。

## 10. 错误协议

统一错误结构：

```json
{
  "code": "policy_denied",
  "message": "operation denied by effective policy",
  "retryable": false,
  "details": {},
  "request_id": "req_01..."
}
```

V1 错误码：

```text
protocol_version_unsupported
not_initialized
invalid_request
not_found
conflict
idempotency_conflict
invalid_definition
compatibility_blocked
policy_denied
approval_required
credential_unavailable
oauth_required
connector_unavailable
workspace_denied
runtime_unavailable
plan_invalid
step_dependency_invalid
attempt_stale
run_not_resumable
cancelled
internal_error
```

错误信息不得包含 Token、API Key、完整环境变量或敏感路径。

## 11. 幂等、取消与并发

- 创建 Run 的方法必须支持 `idempotency_key`；相同 key + 相同请求返回同一 receipt；相同 key + 不同请求返回 `idempotency_conflict`；
- 幂等收据持久化保存（进程重启后可重放），作用域为 `principal_id + client_id + method + idempotency_key`，请求指纹为规范化 JSON 哈希；`agent/run` 与 `run/cancel` 各自维护独立收据；
- 同一进程内携带 key 的变更请求串行执行（先查重、再启动、最后提交收据），避免常见并发重复；跨进程/崩溃瞬间的严格 exactly-once 需要 Intent/claim 状态机，列为后续工作项——当前模型在 runtime 创建后、收据提交前崩溃时，重试可能产生孤儿执行（由 `run/get` 可见并人工/管理端清理）；
- `run/cancel` 只请求取消，终态由服务端事件确认；
- `run/retry` 创建新 Attempt，不覆盖历史 Attempt；
- `run/replan` 创建新 Plan Revision，不删除历史计划；
- 服务端拒绝过期/重复的审批、Attempt 和状态迁移请求。

## 12. V1 验收用例

- `TC-AS-001`：initialize/initialized 能力协商（单 Agent 能力为 true，Team/Skill/MCP 等为 false）；
- `TC-AS-002`：Catalog 方法返回 Agent/Team/Skill/Connector；
- `TC-AS-003`：agent/run 返回异步 run receipt（public opaque run_id）；
- `TC-AS-004`：team/run（延后实现，当前拒绝）；
- `TC-AS-005`：状态查询与通知一致性——断线或丢通知后 run/get 恢复权威状态，按 sequence 去重，`run/events` 可追平；
- `TC-AS-006`：取消、retry、replan 均产生规范事件；
- `TC-AS-007`：Approval Server Request 可响应并完成二次策略校验（延后实现）；
- `TC-AS-008`：Artifact 不允许任意路径读取（延后实现）；
- `TC-AS-009`：SDK/CLI/Web 不调用内部 allo UI API；
- `TC-AS-010`：错误和公共响应（含实时事件）不泄露凭据、内部 execution/session/step/attempt ID 或敏感路径；
- `TC-AS-011`：workspace 注册只接受受控路径；revoked/foreign/路径逃逸一律 `workspace_denied`；
- `TC-AS-012`：幂等收据跨进程重启仍可重放，key 冲突返回 `idempotency_conflict`；
- `TC-AS-013`：WebSocket 事件推送按 owner 与订阅过滤，lag 时发送 `run/resync-required`。

## 13. 持久化 Nomi 聊天（conversation/*）

App Server 另提供面向单 Agent 持续对话的持久化聊天协议。它与 `agent/run` 的
preset-backed 执行面相互独立：聊天直接复用 Allo 的 Conversation 持久化层
（对话、消息、turn 收据、实时事件），模型选择默认来自本机
`~/.agent-store/config.toml`。

### 13.1 方法

| 方法 | 说明 |
| --- | --- |
| `conversation/create` | 创建持久化 Nomi 对话；`model` / `reasoning_effort` 可省略（见 13.2） |
| `conversation/model-options` | 可选模型目录：config.toml 的 provider/模型/默认选择与思考等级词表（不含凭据） |
| `conversation/update` | 更新名称、模型（`model`）或思考等级（`reasoning_effort`）；Nomi 运行时在下一次回复时生效 |
| `conversation/list` | 该 owner 的 App Server 聊天列表（按 modified_at 倒序） |
| `conversation/get` | 单会话视图 |
| `conversation/messages` | 分页历史（升序，`page_size` 1..=200）。响应体为 `{ "items": [...], "has_more": bool }`：`items` 为消息数组（升序），`has_more` 是服务端 keyset 算出的精确标志——还有更旧消息时为 `true`。客户端以 `cursor: ""` 取最新窗口，以最旧已加载消息的 `created_at:message_id` 翻更早页；"向上加载"应直接读 `has_more`，不要以"页是否装满"近似推断 |
| `conversation/send` | 发送消息，必带 `idempotency_key` |
| `conversation/cancel` | 停止当前 turn |
| `conversation/delete` | 删除 App Server 会话（HTTP 为 `DELETE /api/app-server/conversations/:id`）；委托既有会话删除语义：运行中先按 stop/orphan fence 处理，保留的 execution transcript 拒绝删除，删除失败返回稳定错误码 |
| `conversation/subscribe` / `conversation/unsubscribe` | 实时事件订阅 |

`conversation/create` 请求体：

```json
{
  "name": "可选名称",
  "model": { "provider_id": "<已注册 UUIDv7>", "model": "mimo-v2.5-free" },
  "workspace": { "id": "<可选已注册 workspace>" },
  "reasoning_effort": "可选 low / medium / high / xhigh"
}
```

`reasoning_effort` 通过会话 `extra.reasoning_effort` 透传到 Nomi 运行时
（OpenAI 风格 effort，发往 provider 的 `reasoning_effort` 字段）；词表外的值
返回 `invalid_request`。`conversation/update` 请求体：

```json
{
  "conversation_id": "<uuidv7>",
  "name": "可选新名称",
  "model": { "provider_id": "<provider 或 config key>", "model": "..." },
  "reasoning_effort": "可选 low / medium / high / xhigh"
}
```

模型与思考等级变化在下一个 turn 边界重建运行时后生效；更新走与
create 相同的模型解析（已注册 UUID 直通或 config.toml provider key）。

`conversation/model-options` 响应示例：

```json
{
  "default": { "provider": "opencode", "model": "mimo-v2.5-free" },
  "providers": [
    { "name": "opencode", "models": [
      { "name": "mimo-v2.5-free", "display_name": "MiMo V2.5 Free", "context_limit": 200000 }
    ] }
  ],
  "reasoning_efforts": ["low", "medium", "high", "xhigh"]
}
```

### 13.2 模型解析与 agent-store 配置

`model` 省略时，后端按以下顺序解析；显式给出时，`provider_id` 接受**已注册
的 provider UUID**（原样使用）或 **config.toml 中 `[providers.<key>]` 的名字**
（服务端幂等注册并改写为规范 UUID）：

1. `~/.agent-store/config.toml` 的 `default_model = "<provider>/<model>"`；
2. `[providers.<provider>]` 中的 `type` / `api_key` / `base_url` / `enabled`；
3. `[models."<provider>/<model>"]` 中的 `model` / `display_name` /
   `max_context_size` 等。

首次使用时后端**原子地**把该 provider 注册进 Allo provider 仓储
（API key 加密存储、`provider_models` 同步生成），并以注册得到的规范
UUIDv7 作为会话模型身份——之后完全走常规 Allo 运行时路径。注册按
provider 名幂等：同一 key 多次 create 复用同一 provider。未找到任何
配置时返回稳定错误（`provider_not_found` / `invalid_request`）。

`model` 显式给出时，已注册的 provider UUID 原样使用；未注册的
provider_id 会按 config provider 名尝试解析。

### 13.3 实时事件

订阅后事件经 `conversation/event` 通知投递，`sequence` 为连接内单调递增：

```json
{
  "jsonrpc": "2.0",
  "method": "conversation/event",
  "params": {
    "conversation_id": "<uuidv7>",
    "sequence": 1,
    "event_type": "message.created",
    "payload": { "message_id": "<uuidv7>", "role": "user", "content": "...", "created_at": 1788231528245 }
  }
}
```

`event_type` 取值：`message.created`（用户消息落库）、`message.delta`
（assistant 流式增量，`payload.replace=true` 时整体替换）、
`message.thinking`（思考内容增量，按 `payload.message_id` 聚合，
`payload.status=done` 表示结束，载荷含公开的 `content` / `subject` /
`status` / `duration` / `replace`）、`message.tips`（显示提示，载荷含公开的
`content` / `tip_type`：success/warning/error）、`message.tool`（工具调用实时
投影，按 `message_id` 聚合，载荷只含公开的 `name` / `status`，不含 args /
output / call_id）、`message.error`（终端错误，载荷只含
公开的 `message` / `code` / `retryable`，绝不携带 incident/detail/路径等
内部字段）、`message.activity`（其他状态活动）、`turn.status`
（`running` / `completed`）、`context.usage`（会话最新实测上下文占用，
载荷为 `{ "context_usage": { "used_tokens", "window_tokens", "updated_at",
"source": "measured" } }`；`used_tokens` 为最后一次 provider prompt 的
occupancy gauge，`window_tokens` 为有效窗口，二者均为服务端在
`TurnCompleted` 实测并持久化，客户端无法写入，未上报时 `conversation/get`
返回 `context_usage: null`，界面显示“暂不可用”而非估算值）。hidden 中继事件不投递且不占用序列；
事件载荷只包含公开 conversation/message/turn ID，绝不携带内部
execution/session/attempt ID。事件流 lag 时发送
`conversation/resync-required`，客户端用 `conversation/messages` 补拉。

### 13.4 能力边界

- 聊天会话使用 `DelegationPolicy::Disabled`，Team/Skill 自动注入和 MCP
  配置在创建与运行时两侧都被禁用（仅保留普通本地 Agent 工具）；
- 创建会话时自动注册 owner 受控 workspace（`conversation/create` 无需
  先注册 workspace）；

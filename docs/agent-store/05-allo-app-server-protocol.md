# allo App Server Protocol 规格

> 状态：**现行正文（未正式发版，可改；改动同步更新）**——协议在发版前只有一个版本，统一称 v1，不设 v1/v1.1/v2 之分（`16-sdk-webui-site-priority-plan.zh.md` §7 决策 4）。单 Agent 模式已实现并通过聚焦验证（Workspace Resolver、持久化幂等、WebSocket 实时事件推送）；Skill/Connector 目录能力（skill/*、connector/*、OAuth 状态透传）已启用并接入 agent/run 运行时接线；Team 能力保持关闭；跨进程崩溃的严格 exactly-once 与端到端联调待发布前验证
> 日期：2026-08-26
> 前置：`00-architecture-decision.md`、`01-domain-model.md`、`04-allo-runtime-adapter.md`
> 目标：建立 SDK、CLI、MCP、Web/Flowy 的唯一公共兼容边界

## 1. 协议定位

```text
Agent Store（Catalog / 凭据 / 审批 / 策略等领域服务）
    ↓ 建立在
App Server Protocol v1（唯一公共协议层；发版前只有一个版本）
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
WebSocket：本地 Web/Flowy 实时集成（V1 交付）
stdio JSONL：本地 CLI/SDK/子进程集成（V2/deferred；V1 不纳入，见 `16-sdk-webui-site-priority-plan.zh.md` §7 决策 2）
```

后续可增加 HTTPS/远程部署，但不改变消息语义。

### 2.1.1 传输绑定（方法语义唯一）

方法语义只有一套（§4–§5 的方法名与参数/结果类型）；WebSocket 与 HTTP
只是两种绑定：

```text
WebSocket 长连接：initialize → initialized → ready，一连接多调用，
  订阅/实时通知只走这里
HTTP 一次性握手：每次调用独立 POST /initialize 取连接头 →
  POST /initialized → 业务调用 → 连接即失效
```

`import/*`、`install/*`、`market/*`、`store/*` 两侧方法名一一对应，共用同一
实现（`market/remove` 的 `cascade`、`market/auto-update` 的 `enabled` 在
HTTP 侧是 **body 字段**，缺省 `true`）；任一侧新增方法必须同时在另一侧落地。

`workspace/*` **不是**一一对应，只有一项同名：

| 方法 | WebSocket | HTTP |
| --- | --- | --- |
| `workspace/create` | `{path}` → 规范化并登记该目录，返回 `WorkspaceView` | **无对应路由** |
| `workspace/list` | 当前 owner 的 active 工作区 | `GET /workspaces` |
| `workspace/revoke` | `{workspace_id}` → 软注销 | `DELETE /workspaces/{workspace_id}` |
| *（HTTP 独有）* | — | `POST /workspaces`：`workspace_register`，**无 body**，服务端分配 id，返回 `{id}`；用于 smoke/dev 脚本，不是 `workspace/create` |

仅存在于 WebSocket 的方法（HTTP 无绑定，客户端须走 WS）：
`initialize`、`initialized`、`conversation/model-options`、`conversation/update`、
`conversation/subscribe`、`conversation/unsubscribe`、`run/subscribe`、
`run/unsubscribe`、`agent/list`、`agent/get`、`team/list`、`team/get`。

公开资产 `GET`（快照/store 头像）与 `/api/fs/browse`
不是协议方法：前者是 `<img>` 直链（带不上连接头），后者是独立文件服务。

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

V1 默认使用本地可信进程模型：由主进程在 **localhost WebSocket** 上建立 `LocalPrincipal`（V1 不含 stdio，见 `16-sdk-webui-site-priority-plan.zh.md` §7 决策 2）；Renderer 不直接构造 Principal，只能通过主进程/SDK 访问。未来支持远程调用时，必须增加独立认证流程，不得把本地 session reference 当作远程身份凭据。

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
skill/create     # 写面（R17 / W12），宿主管理面，见 §4.11
skill/update
skill/delete
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

### 4.6 Marketplace（市场源，roadmap Phase 2）

市场（Marketplace）是**软件源**（类似 winget source）：按 `github` / `git` /
`url` 远程源为主，`directory` 本地目录仅用于开发/调试。添加市场后由
Store（4.7）把所有源聚合为商店视图；市场自身提供源管理（加/删/刷新）与
条目投影。源类型：

- `github`：GitHub 仓库（`owner/repo`，Phase B）；
- `git`：任意 Git 仓库（HTTPS/SSH URL 或本地 `.git` 路径，Phase B）；
- `url`：HTTP(S) `marketplace.json`（Phase B）；
- `directory`：本地目录（仅开发用；UI 排序在最后）。

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

目录探测形态（`probe_directory`，Phase A+）：

- `.codebuddy-connector/connectors.json` → 每个 connector 一条目（连接器市场）；
- `.codebuddy-skill/marketplace.json` → 每个 skill 一条目（技能市场）；
- **`.codebuddy-plugin/marketplace.json`（`plugins[]` 数组）→ 每个 plugin 一条目**
  ——真实 WorkBuddy 专家市场布局（如 `marketplaces/experts/.codebuddy-plugin/marketplace.json`，
  `plugins: [{name, source: ./plugins/<id>, description}]`；条目目录须含
  `.codebuddy-plugin/plugin.json` 才纳入，缺失跳过）；
- 根 `.codebuddy-plugin/plugin.json` → 单插件根即一个条目；
- 根 `cli.json` → 单个 CLI 连接器条目；
- 其余：含 plugin.json/cli.json/SKILL.md 子目录的插件集合。

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

### 4.7 Store（winget 式应用商店，roadmap Phase 2 + Store 扩展）

市场（4.6）是**软件源**；Store 是把**所有启用市场的全部条目**聚合为一个
`winget` 式应用商店视图：专家 / 专家团 / 技能 / 连接器四类内容同屏展示，
用户只需点「安装」——导入 + 运行时注册在一个幂等调用内完成，中间
PluginSnapshot 对用户不可见。

能力协商：`capabilities.store`（`AppServerStoreProvider` 注入时开启）。

```text
GET  /api/app-server/store                                      # store/list（聚合目录）
POST /api/app-server/store/{marketplace_id}/entries/{entry}/install  # store/install-entry（一键安装）
GET  /api/app-server/store/{marketplace_id}/entries/{entry}/assets/{*path}  # 条目展示资产（头像等）
```

`store/list` 响应（`AppServerStoreList`）：

```text
items[] {
  id,                                  # "<marketplace_id>/<entry_name>"
  marketplace_id, marketplace_name,
  entry_name,
  kind,                                # "agent" | "team" | "skill" | "connector"
  name,                                # displayName 本地化值，否则条目名
  display_name / profession / display_description,   # LocalizedText（plugin.json 保真）
  tags[] / quick_prompts[],            # LocalizedText 数组（plugin.json 保真）
  description,
  avatar_url,                          # store 资产端点相对 URL
  version, source_kind,
  installed,                           # provenance 下快照组件已注册进运行时
  update_available,                    # 条目可用版本 ≠ 已装快照版本
  snapshot_id, installed_version       # 已装时非空
}
```

规则：

- 聚合范围 = `enabled=1` 的市场（`plugin_marketplaces.enabled`），条目清单来自
  市场探测投影；**未导入的条目照样出现**（商店内容=源内容，不依赖本地导入状态）；
- 展示元数据来自条目目录的 `.codebuddy-plugin/plugin.json`（复用
  `read_plugin_display`，与导入管线同一解析器），`displayName` / `profession` /
  `displayDescription` / `tags` / `quickPrompts` / `avatar` 全保真；技能与
  连接器无 plugin.json，显示名回退 `connectors.json` 索引（name_zh/name_en/
  version），头像回退市场根 `icons/<source-basename>.<ext>`（png/svg/jpg/
  jpeg/webp/gif）；
- `installed` 由 `marketplace_id + entry_name` provenance 查快照，再查组件
  `installed=1`；`update_available` = 快照版本 ≠ 条目版本；
- `store/install-entry` 幂等：已有 provenance 且组件已装 → 直接返回
  `reused=true`；否则 `market import_entry`（或快照复用）→ `install/run`
  注册，返回 `{ snapshot_id, version, installed_count, errors[] }`；
- store 资产端点与快照资产端点同一 MIME 白名单与路径穿越校验；**不要求
  App Server 连接头**（`<img>` 标签无法携带，头像/图标是公开展示内容）：
  先按条目目录（`entry_dir`，plugin.json `avatar`）解析，缺失时回退市场根
  （`market_dir`，`icons/…`）；绝不暴露绝对路径；
- 条目 kind 判定优先级：`cli.json` → connector；plugin.json 带 agents/
  teamInfo → agent/team；**根 `SKILL.md` → skill**（skill 条目可随附
  `mcp.json`，技能信号更强）；`mcp.json` → connector；否则回退市场来源
  kind；
- 市场条目在来源 tab 也走「安装」（store install-entry），不再单独暴露
  「导入」作为主路径；本地导入（4.5）保留为高级/调试入口。

### 4.8 Mentions（@专家 / @技能 / @连接器，roadmap Phase 2 扩展）

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
  （`default_run_model`），避免 `resolved_model=None` 在运行时边界被拒；
  回退重解析**保留 mention overrides**（`include_skills` / `mcp_server_ids`
  不丢失）。

### 4.9 模型目录（models/list，REQ-PAR-05b）

SDK/第三方此前无法枚举可用模型。`models/list` 把 provider 注册表投影为
公共模型目录：只含启用 provider 的未显式禁用模型，`is_default` 标记
`agent/run` 在 preset 未绑定模型时的回退目标（与 `default_run_model` 同序）。

能力协商：`capabilities.models`（`ModelCatalogProvider` 注入时开启）。

```text
WS   models/list  {}
HTTP GET /api/app-server/models
```

响应（`AppServerModelList`）：

```json
{
  "items": [
    {
      "provider_id": "0190f5fe-...",
      "provider_name": "mimo",
      "model": "mimo-v2.5",
      "display_name": "MiMo v2.5",
      "is_default": true
    }
  ]
}
```

边界：投影**不含** API key、base URL、健康状态等内部字段；`display_name`
仅在 provider 配置了 `model_descriptions` 时出现。

### 4.10 宿主设置文件（config/get · config/set，R16）

provider / 默认模型配置属于**宿主管理面**（`16` §6）：这两个方法是 Web UI 设置
对话框读写 `~/.agent-store/config.toml` 的唯一通道，**不进入 SDK 包**（第三方消
费者没有读取或改写宿主 provider 配置的理由），也**没有 HTTP 绑定**。

```text
WS   config/get  {}
WS   config/set  { "default_model": "<provider_key>/<model>" }
```

`config/get` 响应（`AppServerConfigView`）：

```json
{
  "exists": true,
  "default_model": "opencode/mimo-v2.5-free",
  "providers": [
    { "name": "opencode", "enabled": true, "models": ["laguna-s-2.1-free", "mimo-v2.5-free"] }
  ]
}
```

- 文件缺失是**正常答案**（`exists:false` + `default_model: null` + 空 providers），
  不是错误；文件存在但读不动 / 解析失败返回 `config_unavailable`——不拿默认值把
  「文件坏了」伪装成「文件没写」。
- `default_model` 无声明时是显式 `null`，与「尚未读取」区分。
- `providers` 是**文件里声明的事实**（config-only 投影，与 `models/list` 的 config
  分支同源），不含 `api_key` / `base_url`，也不含已注册 provider 行。

`config/set` 只接受白名单字段（当前仅 `default_model`）：请求里出现 `api_key` /
`base_url` / 路径等**任何**其他键都是 `invalid_request`（不是静默忽略）；值为空、
不含 `/`、或 provider key 不在 `[providers.<key>]` 中同样被拒——**运行时解析不了
的默认值不写**（未声明的 *model* 允许，运行时按请求注册它）。

写入是**最小改动**：只重写目标键，文件其余内容、注释与排版原样保留（`toml_edit`）；
缺失的键插在文件头注释之后、第一个 `[table]` 之前（绝不落到某张表里），并以同目录
临时文件 + `rename` 原子替换。响应是**写后重读**的 `AppServerConfigView`，客户端看到
的落点就是磁盘上的内容，不存在「请求发出去了」当成「值存下来了」。

作用域：与所有方法共用同一条 owner 闸门（连接 principal 的 `require_ready`：外部
owner → `policy_denied`，未注册连接 → `not_found`），且**没有** owner / 路径参数可供
越权；凭据永不进入 wire、日志或前端。

---

### 4.11 技能写面（skill/create · skill/update · skill/delete，R17 / W12）

技能的创建 / 编辑 / 删除是**宿主管理面**（`16` §6 判断规则：第三方 SDK 消费者不应能往宿主的技能树里写文件），与 `config/*` 同口径：**仅 WebSocket**、**不进 SDK 包**、**无 HTTP 绑定**，Web UI 经自有 transport helper 调用。

```text
WS   skill/create  { "name": "…", "description": "…", "when_to_use"?, "allowed_tools"?, "paths"?, "body"? }
WS   skill/update  { "skill_id": "…", "markdown": "<完整 SKILL.md>" }
WS   skill/delete  { "skill_id": "…" }
```

`create` / `update` 的响应就是 `skill/get` 的 `AppServerSkillDetail`（**写后回读**：响应来自重新读取的目录，不是请求回显）；`delete` 响应是 `AppServerSkillDeleteResult`：

```json
{ "skill_id": "weekly-report", "deleted": true, "revealed_origin": "builtin" }
```

**归属与可写性**：公开 id ＝ 技能名（frontmatter `name`，回退目录名），不加来源前缀。`AppServerSkillSummary` 因此新增两个**增量**字段：`origin` ∈ `user|shared|companion|draft|marketplace|builtin|unmanaged`，`writable`（= `origin == "user"` **且**目录为规范用户位置）。`source` 取值不变（`builtin`/`extension`/`custom`）——市场安装产物与用户技能在 `source` 上都是 `custom`，靠 `origin` 区分。

| 磁盘位置 | `origin` | 写面 |
| --- | --- | --- |
| `{builtin_skills_dir}/…`（含 `auto-inject/`） | `builtin` | 只读 |
| `{user_skills_dir}/{name}/` | `user` | **可写（唯一）** |
| `{user_skills_dir}/shared/{name}/` | `shared` | 只读（伙伴链路） |
| `{user_skills_dir}/companion/{id}/{name}/` | `companion` | 只读（伙伴链路） |
| `{user_skills_dir}/_drafts/{id}/{name}/` | `draft` | 只读（审阅暂存） |
| `{user_skills_dir}/agent-store/{snapshot_id}/{slug}/` | `marketplace` | 只读（**卸载走 `install/uninstall`**） |

规则：

- **路径绝不由参数决定**：目标只能来自宿主侧 `SkillPaths` 的 `{user_skills_dir}/{name}`；名字只做白名单校验（非空、≤64 字符、无首尾空白、无控制字符、无 `.` / `..` / `/` / `\` / `:`）。
- **不静默覆盖**：`create` 同名（用户 / 内置 / 市场产物 / 共享 / 伙伴）→ `conflict`，message 点明撞上的 origin 与出路；磁盘上已有同名目录但未被目录树收录（`SKILL.md` 缺失或非法）→ 同样 `conflict`，不合并写入。
- **符号链接逃逸被拒**：目标目录或 `SKILL.md` 是链接 → `policy_denied`（`create` 走 `create_dir_all`、`update` 走 `write`，都会跟随链接）。
- **写后回读**：`delete` 之后重新读该 id，把「删除后这里现在是哪一类来源」放进 `revealed_origin`——用户技能遮蔽同名内置技能时，删除会把它重新暴露出来。
- 错误码沿用既有族：`invalid_request`（名字非法 / 未知字段 / 正文缺合法 frontmatter / 文档 `name` ≠ 被改 id）、`not_found`、`conflict`、`policy_denied`（只读来源、链接、非规范目录）。
- **凭据门（R22）保持**：三个方法都是 `deny_unknown_fields`，出现 `api_key` / `env` / `token` 等任何其他键即 `invalid_request`；写面没有任何字段能表达路径或凭据。
- 作用域：共用同一条 owner 闸门（`require_ready`）；宿主未接线写面 → `unsupported_operation`（不是静默 no-op）。
- **未做（登记）**：`skill/copy` 与「编辑面的全量正文回读」，见 `16` R17 落地记录。

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
run/plan（已实现：计划与步骤的权威快照）
run/cancel
run/steer（已实现：注入口头引导）
run/subscribe
run/unsubscribe
run/events（cursor 恢复）
run/answer-decision（已实现：回答等待中的决策）
run/pause
run/resume
run/retry
run/replan
```

`run/subscribe` / `run/unsubscribe` 订阅或取消订阅单个 public Run 的实时事件流，请求均为 `{"run_id": "run_01..."}`。订阅要求该 Run 已存在且属于当前用户；取消订阅同样要求持有该用户自己的公共 mapping。订阅只影响尽力而为的通知推送，不影响持久化状态查询。

`run/events` 支持 `{"run_id", "after_sequence", "limit"}` 游标读取，客户端用它追平断线期间遗漏的事件。

`run/plan`（请求 `{"run_id"}`，返回 `AgentRunPlan`）是 **W4 / W6 的计划与步骤权威快照**，也是唯一带步骤**标题**、失败原因与起止时间的读取面——`run/events` 是追加式日志，只承载标记（`change` / `status`），从不携带标题或时间戳：

- 形状：`{run_id, status, version, steps[], dependencies[]}`；每个 step 带 `step_id` / `title` / `kind` / `status` / `role` / `model` / `introduced_in_revision` / `superseded_in_revision` / `created_at` / `updated_at` / `attempts[]`；每个 attempt 带 `attempt_id` / `attempt_no` / `status` / `trigger_reason` / `role` / `model` / `question` / `error` / `output_summary` / `output_files` / `tokens` / `started_at` / `finished_at`。
- owner 作用域与 `run/get` **完全一致**（同一个 `AgentExecutionEngine::get`）：不是 owner 的 Run 一律 `NotFound`，不透露存在性。
- **不新增内部标识**：`step_id` / `attempt_id` 本就是 `run/events` 与 `run/answer-decision` 的既有公共面（CAS 需要）；成员归属用 `role` + `model` 表达，`participant_id` / `source_agent_id` 不上 wire；`output_files` 经既有的相对路径过滤，绝对路径同样不上 wire。
- 快照与事件的关系是**互补**而非替代：事件给「发生过什么」（含引导 / 停止回合等副作用标记），快照给「现在是什么」；客户端不应从事件反推步骤标题或耗时。
- HTTP 绑定：`GET /api/app-server/run/{run_id}/plan`（与 WS 臂共用 `get_run_plan_for_user`）。

`run/answer-decision`（请求 `{run_id, step_id, attempt_id, answer, expected_execution_version, expected_step_version, expected_attempt_version}`，返回 `AgentRunView`）是当前 V1 的审批回答入口：

- 它**直通** `AgentExecutionEngine::answer_decision` 这一唯一回答门——owner 作用域 + **三路 CAS**（execution/step/attempt 版本）+ **仅 `waiting_input` 的 attempt 接受** + `answer` 非空 + id 规范化；越权 `NotFound`、版本过期或非等待态 `Conflict`、空 answer / id 非法 `BadRequest`；
- **参数不接受任何 approve-all 开关**（桌面侧 `POST /api/conversations/:id/confirmations/:callId/confirm` 的 `always_allow` 不进协议；`deny_unknown_fields`，带上即 `invalid_request`）；
- `step_id` / `attempt_id` 与三个 CAS 版本来自 `run/events` 的 `approval.requested` 投影（`step_id`/`attempt_id` + 读取时从权威行取的三版本）。引擎的 `DecisionRequested` 事件 payload 只有 `{question, stop_turn_operation_id}`，`approval/request` + `approval/respond` 那套 Server Request 形状仍是目标设计（见本文件审批章节），V1 以本方法落地。

`InitializeResult.capabilities.approvals` 跟随 runtime 位（无 runtime 的连接不宣告该能力，避免客户端等一个永远不会被接受的回答）。

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
unauthenticated
token_expired
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

`unauthenticated`（连接令牌未知 / 已关闭 / 已吊销）与 `token_expired`（令牌因闲置过期，`retryable=true`，客户端重新 `initialize` 即可）是 §3.1「认证失败」在 V1 的两种可观察形态，由 `22` §7.1 A2 落实并分开——过期**不**与无效混同。`invalid_issuer` / `invalid_audience` / `insufficient_scope`（§3.1 亦提及）在当前本地可信进程模型下没有产生点，待远程认证流程引入时再落地。

错误信息不得包含 Token、API Key、完整环境变量或敏感路径。

## 11. 幂等、取消与并发

- 创建 Run 的方法必须支持 `idempotency_key`；相同 key + 相同请求返回同一 receipt；相同 key + 不同请求返回 `idempotency_conflict`；
- 幂等收据持久化保存（进程重启后可重放），作用域为 `principal_id + client_id + method + idempotency_key`，请求指纹为规范化 JSON 哈希；`agent/run` 与 `run/cancel` 各自维护独立收据；
- 同一进程内携带 key 的变更请求串行执行（先查重、再启动、最后提交收据），避免常见并发重复；跨进程/崩溃瞬间的严格 exactly-once 需要 Intent/claim 状态机，列为后续工作项——当前模型在 runtime 创建后、收据提交前崩溃时，重试可能产生孤儿执行（由 `run/get` 可见并人工/管理端清理）；
- `run/cancel` 只请求取消，终态由服务端事件确认；
- `run/retry` 创建新 Attempt，不覆盖历史 Attempt；
- `run/replan` 创建新 Plan Revision，不删除历史计划；
- 服务端拒绝过期/重复的审批、Attempt 和状态迁移请求。

## 12. 持久化 Nomi 聊天（conversation/*）

App Server 另提供面向单 Agent 持续对话的持久化聊天协议。它与 `agent/run` 的
preset-backed 执行面相互独立：聊天直接复用 Allo 的 Conversation 持久化层
（对话、消息、turn 收据、实时事件），模型选择默认来自本机
`~/.agent-store/config.toml`。

### 12.1 方法

| 方法 | 说明 |
| --- | --- |
| `conversation/create` | 创建持久化 Nomi 对话；`model` / `reasoning_effort` 可省略（见 12.2） |
| `conversation/model-options` | 可选模型目录：config.toml 的 provider/模型/默认选择与思考等级词表（不含凭据） |
| `conversation/update` | 更新名称、模型（`model`）或思考等级（`reasoning_effort`）；Nomi 运行时在下一次回复时生效 |
| `conversation/list` | 该 owner 的 App Server 聊天列表（按 modified_at 倒序） |
| `conversation/get` | 单会话视图 |
| `conversation/messages` | 分页历史（升序，`page_size` 1..=200）。响应体为 `{ "items": [...], "has_more": bool }`：`items` 为消息数组（升序），`has_more` 是服务端 keyset 算出的精确标志——还有更旧消息时为 `true`。客户端以 `cursor: ""` 取最新窗口，以最旧已加载消息的 `created_at:message_id` 翻更早页；"向上加载"应直接读 `has_more`，不要以"页是否装满"近似推断 |
| `conversation/send` | 发送消息，必带 `idempotency_key`；可选 `attachments`（见下） |
| `conversation/cancel` | 停止当前 turn |
| `conversation/delete` | 删除 App Server 会话（HTTP 为 `DELETE /api/app-server/conversations/:id`）；委托既有会话删除语义：运行中先按 stop/orphan fence 处理，保留的 execution transcript 拒绝删除，删除失败返回稳定错误码 |
| `conversation/subscribe` / `conversation/unsubscribe` | 实时事件订阅 |

`conversation/send` 请求体（HTTP 与 WS 同形）：

```json
{
  "conversation_id": "<会话 id>",
  "content": "消息正文",
  "idempotency_key": "<客户端幂等键>",
  "attachments": ["<会话工作区内的绝对路径>"]
}
```

`attachments` 是 **R15（W10）** 的附件载体（**路径引用**，2026-09-11 用户拍板）：
每一项必须是**本会话工作区内的真实文件**的绝对路径；服务端按 owner 作用域解析会话工作区后
逐个 canonicalize，越界（`..`、指向外部的符号链接、别的盘）、相对路径、URL、不存在的路径
一律拒绝（`invalid_request` / `workspace_denied`），**不做「尽力而为」的降级**。单次上限
**10** 条（与运行时 `MAX_IMAGE_ATTACHMENTS` 同值），重复路径只算一条。字段是纯加法：
不传即无附件，老客户端行为逐字不变；请求体仍是 `deny_unknown_fields`，写错字段（例如历史上
的 `files`）会被拒，而不是静默丢弃附件。

运行时只把 **PNG / JPEG / WebP** 送进模型；运行时显式拒绝的图片格式
（gif / bmp / tif / tiff / ico / avif / heic / heif / svg）会报错，**非图片路径被忽略**
（模型看不到它，客户端应在正文里给出路径）。客户端的前置校验口径与运行时逐条对齐，
见 `web/src/lib/attachments.ts`。

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

### 12.2 模型解析与 agent-store 配置

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

### 12.3 实时事件

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
内部字段）、`message.activity`（其他状态活动；`kind="turn_completed"` 时**additive** 带上本轮 token 用量 `payload.usage = { "input_tokens", "output_tokens", "total_tokens" }`——即运行时 `TurnCompleted` 的逐轮上报，运行时就**没上报**或只报了单侧时该键整段缺席，客户端据此保持「未知」而不是 0；其余运行时指标（cache 明细 / 上下文 gauge / breakdown / MoA / stop_reason）不进投影）、`turn.status`
（`running` / `completed`）、`context.usage`（会话最新实测上下文占用，
载荷为 `{ "context_usage": { "used_tokens", "window_tokens", "updated_at",
"source": "measured" } }`；`used_tokens` 为最后一次 provider prompt 的
occupancy gauge，`window_tokens` 为有效窗口，二者均为服务端在
`TurnCompleted` 实测并持久化，客户端无法写入，未上报时 `conversation/get`
返回 `context_usage: null`，界面显示“暂不可用”而非估算值）。hidden 中继事件不投递且不占用序列；
事件载荷只包含公开 conversation/message/turn ID，绝不携带内部
execution/session/attempt ID。事件流 lag 时发送
`conversation/resync-required`，客户端用 `conversation/messages` 补拉。

### 12.4 能力边界

- 聊天会话使用 `DelegationPolicy::Disabled`，Team/Skill 自动注入和 MCP
  配置在创建与运行时两侧都被禁用（仅保留普通本地 Agent 工具）；
- 创建会话时自动注册 owner 受控 workspace（`conversation/create` 无需
  先注册 workspace）；

---

## 13. V1 验收用例

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

## 14. 验收用例正文（TC-API / TC-SDK）

> 本节由 `agent-store-v1-test-cases.md` 原 §6 并入（2026-09-11 文档合并）；TC 编号与用例正文保持不变，内部分节号沿用原文。§13 是摘要索引，本节是逐条正文。

### 6. App Server Protocol 与 SDK

#### TC-API-001：初始化协商

- 等级：P0
- 断言：未 initialize 不能调用业务方法；协议不兼容返回明确错误；initialized 后才进入 ready

#### TC-API-002：异步 receipt 与幂等

- 等级：P0
- 操作：重复提交相同 idempotency_key
- 断言：不重复创建 Run；不同请求复用同 key 返回 `idempotency_conflict`

#### TC-API-003：状态查询与通知一致性

- 等级：P0
- 操作：订阅通知并轮询 run/get；模拟连接中断后重连
- 断言：run/get/run_result 始终返回权威持久化状态；通知丢失不造成状态不一致；展示层按 event_id 去重

#### TC-API-004：公共 ID 隔离

- 等级：P0
- 断言：响应不包含 allo 内部 session、数据库或 provider 私有 ID

#### TC-SDK-001：Node spawn + 回环 WS

- 等级：P1
- 操作：Node SDK 拉起 `agent-store` 独立二进制（`--port 0` + 临时 `--data-dir`），连回环 WS 建连
- 断言：Node SDK 可完成 initialize、Catalog、Run、Event、Artifact 调用
- 说明：stdio 传输列为 V2/deferred（`12-sdk-packaging.md` §2 非目标）；本用例不断言 stdio

#### TC-SDK-002：Browser WebSocket

- 等级：P1
- 断言：Browser SDK 可连接、接收通知、断线重连后以状态查询恢复一致视图；不访问安全凭据存储

#### TC-SDK-003：结构化错误

- 等级：P1
- 断言：SDK 使用稳定 error code，不依赖 message 文本；retryable 语义正确


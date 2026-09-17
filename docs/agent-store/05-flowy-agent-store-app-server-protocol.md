# allo App Server Protocol 规格

> 状态：**现行正文（未正式发版，可改；改动同步更新）**——协议在发版前只有一个版本，统一称 v1，不设 v1/v1.1/v2 之分（`16-sdk-webui-site-priority-plan.zh.md` §7 决策 4）。单 Agent 模式已实现并通过聚焦验证（Workspace Resolver、持久化幂等、WebSocket 实时事件推送）；Skill/Connector 目录能力（skill/*、connector/*、OAuth 状态透传）已启用并接入 agent/run 运行时接线；**Team 能力已启用**（`team/run` 走 Leader Conversation + `nomi_delegate(strategy=planned)`，见 §5.2 与 `16` §7 决策 3）；跨进程崩溃的严格 exactly-once 与端到端联调待发布前验证
> 指纹：**`fp-6`** —— 2026-09-23 起承载**随调用指定模型与思考等级**：`conversation/send` 与
> `agent/run` 各新增可选的 `model` / `reasoning_effort`（现有 DTO 加字段），`ConversationView`
> 新增 `reasoning_effort` 把会话当前的等级读回来。语义是**粘性**的：`send` 上带的值写进会话行、
> **从本轮起**生效（运行时按会话行构建，故不存在"只影响这一轮"），`agent/run` 上带的值只作用于
> 那次运行。规格见 §12.1 / §12.4 / §5.1；方案与理由见 `29-send-model-and-effort-plan.zh.md`。
> 上一值 **`fp-5`**（2026-09-23）：**以专家团开场**——`conversation/create` 新增可选的
> `team_id`（现有 DTO 加字段），用 `team/run` 的同一段编排（成员校验 → 物化/复用执行模板 →
> Leader 会话栅栏）打开一个**可继续对话**的 Leader 会话，区别只有一处：**不发 `goal` 首轮**。
> 与 `agent_id` 互斥。规格见本文 §12.2。
> 再上一值 **`fp-4`**（2026-09-23）：**以专家开场**——`conversation/create` 新增 `agent_id`，
> 专家的 preset 身份、自带的技能与连接器一并冻结进会话（规格见 §12.2）。
> 再上一值 **`fp-3`**（2026-09-23）：**每轮技能**——`conversation/send` 新增可选的 `mentions`，
> 只认 `kind: "skill"`（规格见 §12.3）。
> 更早的 **`fp-2`**（2026-09-22）：承载**工具参数**（`ConnectorTool.input_schema` +
> `ConnectorDetail`/`ConnectorProbeResult` 的 `tools_truncated`），与宿主 `[connector_proxy]`
> 授权单位由「逐个工具」上移到「连接器」是同一件事的两半：要人同意一个工具，就得让他看得见
> 这个工具收什么参数。规格见本文 §4.3.3，方案见 `26-connector-schema-and-grant-policy.zh.md`。
> 最早的 **`fp-1`**（2026-09-21）：由日期戳改为 **`fp-<n>` 计数器**（改前是 `2026-09-21`）。
> 这是**形状变更，不改任何 wire 行为**；但校验是严格相等，所以每个客户端都必须跟着更新。
> 动机：`2026-…` 会被误读成发布日期——日期戳本来就只是标签，连续改动每次加一天，常超前于
> 日历。旧值在下方历史里保留。
> 日期：2026-09-21（2026-09-21：新增**连接器调用代理** `connector/call`——外部 agent 要用
> 已装 MCP 连接器，缺的是**调用面**：连接参数与凭据协议刻意不给（`transport_summary` 是
> 展示摘要、token 永不跨界），于是改为**宿主持有连接与凭据、替调用方执行**。默认全关
> （`[connector_proxy]` opt-in + 显式 allowlist），工具级失败走 `is_error` 而非 wire 错误，
> 审计不记 arguments；指纹 `2026-09-20` → `2026-09-21`，**新增一个方法**（WS + HTTP 各一侧）。
> 规格见本文 §4.3.2，方案与验收见 `24-external-agent-skill-and-mcp-access.zh.md` §5）
> 历史：2026-09-20：新增**技能文件读面** `skill/files` / `skill/file`——技能是
> **目录**（`SKILL.md` + `references/` / `scripts/` / `templates/` / `assets/`，`02` §5、`17` §5），
> 而 `skill/get` 只回 ≤1200 字的正文摘要，附属文件此前**没有任何读面**；指纹 `2026-09-19` →
> `2026-09-20`，**新增两个方法**，HTTP 侧各一条路由。规格见本文 §4.3.1。
> 2026-09-19：新增**通知** `conversation/list-changed`——会话**列表**投影
> 变更（自动标题 / 重命名 / 删除）此前只发给宿主通道，App Server 侧完全看不到，于是侧栏会
> 一直停在客户端 `send()` 时的乐观快照上（名字空白、processing 不落）；指纹 `2026-09-18` →
> `2026-09-19`，**只加一条通知，无方法增删**。2026-09-18：`mcp.json` 补上读面 `config/get-mcp`
> ——编辑器无法编辑它看不见的文件，而「凭据值不上 wire」这条从此精确化为「**verdict 视图**不含
> 取值，文件文本只经这一个按需读面出去」；指纹 `2026-09-17` → `2026-09-18`。2026-09-17：
> `mcp.json` 新增写面 `config/set-mcp` / `config/set-mcp-enabled`——写前用同一解析器验、失败零
> 写入、开关是文本级最小编辑；指纹 `2026-09-16` → `2026-09-17`，**新增两个方法**，无 HTTP 绑定。
> 2026-09-16：`store/list` 条目新增 `published_at`（市场声明的发布日期，`YYYY-MM-DD`，绝不派生）；
> 指纹 `2026-09-15` → `2026-09-16`。2026-09-15：安装器五动词**真正释放/移动运行时产物**并回报
> 结构化 `outcomes`；协议指纹 `2026-09-14` → `2026-09-15`，无方法增删）
> 前置：`00-architecture-decision.md`、`01-domain-model.md`、`04-flowy-agent-store-runtime-adapter.md`
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

> 例外说明（2026-09-20）：`skill/file` 的 HTTP 绑定（§4.3.1）回的是**原始字节 + `content-type`**，
> 不是 JSON 信封。它仍是协议方法（WS 侧同一方法回 base64 JSON），但正因为它不是 JSON 请求/响应，
> `@flowy-agent-store/client` 的 `HttpTransport`（JSON 绑定）**没有**为它建 typed 路由；
> 要走 HTTP 取字节就用 `fetch` 直连该路由，否则用 WS 侧。

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
    "protocol_version": "2026-09-11",
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
  "protocol_version": "2026-09-11",
  "server": {"name": "flowy-agent-store", "version": "0.1.0"},
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
    "skill_files": false,
    "connectors": false,
    "connector_calls": false,
    "run_notifications": true,
    "approvals": false,
    "artifacts": false,
    "oauth": false
  }
}
```

`run_notifications` 仅在 WebSocket 传输且服务端事件源可用时为 `true`；`agents` 在 Runtime 或 Agent Catalog provider 注入时为 `true`。`skills`/`connectors`/`oauth` 仅在对应 Catalog/OAuth provider 注入时启用（生产装配注入系统 Skill/MCP 服务适配器）；**`skill_files` 与 `connector_calls` 各自独立**（§4.3.1 / §4.3.2）：前者对应技能**文件树**读面、后者对应**连接器调用代理**，两者都刻意不与同名目录位合并——查 `skills` 不足以判断 `skill/files` 是否可用，查 `connectors` 同样不足以判断 `connector/call` 是否可用（且调用代理还有上游闸门：宿主 `[connector_proxy]` 表缺席或未启用时代理整体关闭，这一对被 `allow` 收窄出局或被 `deny` 排除时也各自回 `policy_denied`——三个 `policy_denied` 情形的 reason 措辞不同）；`imports` 仅在 Importer provider 注入时启用，`teams` 仅在 Team Catalog provider 注入时启用；未注入时对应方法返回 `unsupported_operation`。Approval、Artifact 能力在当前单 Agent Phase 保持 `false`，由后续 Phase 逐个启用。

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

#### 4.3.1 技能文件读面（`skill/files` · `skill/file`，2026-09-20 加入）

技能是**目录**，不是单个文档：`SKILL.md` 之外还有 `references/` / `scripts/` /
`templates/` / `assets/`（`02` §5、`17` §5 的组件映射原话是「含 `SKILL.md` 与其附属文件」，
安装时**递归**拷贝）。而 `skill/get` 的 `instructions_summary` 是**有界摘要**
（`app_server_skill_files` 之外的 `AppServerSkillCatalog` 取正文前 1200 字），因此附属文件
此前没有任何读面——外部 agent 即使看到技能名，也拿不到它的附属内容。

```text
WS   skill/files  { skill_id }              → AppServerSkillFileList
HTTP GET  /api/app-server/skills/{skill_id}/files
WS   skill/file   { skill_id, path }        → base64 + content_type
HTTP GET  /api/app-server/skills/{skill_id}/files/{*path}   → 原始字节 + content-type
```

响应：

```text
files[] {
  path,              # 技能目录内相对路径，POSIX 分隔符，按 path 排序
  size,              # 字节数
  digest             # 单文件 sha256（小写 hex）
}
content_digest       # 该**技能目录**的树摘要（排序后的相对路径 + 逐文件 sha256）
truncated            # 清单触顶（2000 项）时为 true——绝不静默截断
```

**`content_digest` 的语义边界（易错，先读这条）**：它覆盖的是**这一个技能目录**，
与快照的 `content_digest` **不是同一个东西**——后者覆盖整棵导入来源树
（`nomifun-importer/src/import.rs` 的 `tree_digest(&files)` 作用于整份 import 的文件清单）。
两者**仅当快照里恰好只有这一个技能目录、别无他物**时才相等；**不要**拿它去和 `import/get`
对账。算法相同（同一 `tree_digest` 规则）是为了让调用方能据此钉住自己读到的版本。

规则：

- **两个方法都在 WS 与 HTTP 两侧有绑定**（`skill/file` 的 HTTP 绑定回**原始字节**而不是 JSON）；
- **能力位是独立的 `skill_files`**，不与 `skills` 合并：宿主可以只接目录不接文件面，
  此时这两个方法回 `unsupported_operation`。客户端在提供文件访问前应先查 `capabilities.skill_files`；
- **`path` 只接受技能目录内的相对路径**。绝对路径、盘符 / UNC、`..` 段、反斜杠一律
  `invalid_request`；解析后再经 `canonicalize` + `starts_with` 二次确认，**符号链接一律不跟随、
  不列出**。注意 `Path::components` 会把内部 `.` 归一化掉，故 `a/./b` 等价于 `a/b` 被接受
  （它在目录内），真正兜底的是 canonicalize 那一遍；
- **单文件上限 2 MiB**：从 `metadata` **先判后读**，超限回 `response_too_large`，
  不静默截断（截断过的文件会被调用方当完整内容去 hash）；
- **必须持有就绪连接**（`require_ready`）。这与公开的展示资产路由
  （`/api/app-server/imports/{snapshot}/assets/{path}`，为 `<img>` 设计、无连接头、只服务图片）
  **是两条不同的路**，刻意不复用；
- **不得把绝对路径放进 wire**：DTO 里只有相对路径，宿主自行把 id 解析到磁盘。

> **这是便利性与稳定性边界，不是保密边界**：同机同用户的进程本来就能直接读这些文件。
> 要求认证 + 穿越校验的目的只有一个——**不让它退化成一个未认证的任意文件读本地原语**。
> 同一条口径也适用于「`scripts/` 只回字节、绝不执行」：与 `02` §5 / `17` §6
> 「导入只复制与解析，不执行任何脚本或命令」一致，执行与否是调用方自己的责任。
>
> **已知不对称（不在本次范围内）**：`skill/list` 公布的 `id` 是**技能名**，而
> `agent/list` 的 `id` 是组件 id（`wb-<plugin>-<slug>`）。统一二者是破坏性变更
> （同时冲击 mention 挂载、写面按名 join 与 `writable` 判定），故本次只在正文写明。

#### 4.3.2 连接器调用代理（`connector/call`，2026-09-21 加入）

外部 agent 要用一个已装 MCP 连接器，需要三件事：**连接参数**、**凭据**、**调用面**。
前两件协议刻意不给（`connector/get` 的 `transport_summary` 是展示用摘要，注释原文
「Never a raw shell command the client may execute」；token 按 `06` 永不跨界），
所以只补第三件——**由宿主持有连接与凭据，替调用方执行**：

```text
WS   connector/call  { connector_id, tool, arguments } → AppServerConnectorCallResult
HTTP POST /api/app-server/connectors/{connector_id}/call   body: { tool, arguments }
```

```text
{ is_error,             # 上游 isError 原样透出
  result }              # 上游 tools/call 结果对象，**逐字透传**
```

**`result` 是逐字的**：`content`、`structuredContent` 以及更新的 server 将来加的任何字段
都原样带出——这一层没有资格改写 MCP server 的返回。它**不含**任何 transport / header / env
取值：连接与凭据留在宿主，这正是代理的全部意义，也是**没有** `connector/export` 的原因。

**三道门，缺一不可**（顺序即求值顺序）：

| # | 门 | 不过时的码 |
|---|---|---|
| 1 | 宿主 `[connector_proxy]` 策略；未声明即关 | `policy_denied` |
| 2 | 可选**收窄**：`allow` 若存在则必须命中该 连接器/工具 对；随后 `deny` 不得命中 | `policy_denied` |
| 3 | 连接器已注册**且已启用** | `connector_unavailable` |

- **表 fail-closed，表内默认可调（`fp-2` 起）**：缺表或缺 `enabled` 仍然是「关」；但操作者一旦
  开启代理，**已启用的连接器即可被调用**，`allow` 是**可选收窄**（写了就只放命中的；写成空表
  `allow = []` 表示「什么都不放」）、`deny` 是**可选减法**（在 `allow` 之后应用，与 `[tools]`
  的 `disabled` 同序）。这取代了此前的**强制逐工具白名单**——理由是它让人**盲签**：见 §4.3.3
  与 `26-connector-schema-and-grant-policy.zh.md` §3.1。
- **条目的词汇与 `[tools]` 完全相同**：`mcp__<连接器>__<工具>`，`<连接器>` 可写**注册名**或
  **id**（id 是精确写法：MCP server 按**名字** upsert，后来安装的同名者会接管名字并因此继承
  授权），且**只有 `mcp__` 条目是 glob**（`mcp__github__*` 表示整个连接器）。匹配实现与引擎
  共用同一个 `glob` crate，并照抄了引擎的用例表，所以两侧语义不会各自漂移；**旧写法
  `<连接器>__<工具>`（无 `mcp__` 前缀）在新词汇下不再命中任何东西**，即收窄到零——这是
  fail-closed 的方向，宿主启动时会就这条与「开了代理但没写任何名单」各给一条 warn。
- **调用方只能点名一个已注册的 `connector_id`**：请求里给 `url` / `command` / `headers` /
  `env` 一律 `invalid_request`（`deny_unknown_fields`），因此这条通路**不构成 SSRF**
  ——地址永远来自宿主自己的配置。
- **工具级失败 ≠ wire 错误**：上游 `isError: true` 是**成功的调用**，走 `is_error` 字段让
  调用方分支；只有传输 / 协议 / 超时才升级为 `connector_call_failed` /
  `connector_call_timeout`。合并两者会让调用方分不清「工具说不行」与「没够着工具」。
- **上限与清理**：单次调用超时默认 30s（`connector_call_timeout`）；结果序列化后
  ≤ 1 MiB（超出 `response_too_large`，**不静默截断**）。成功失败都回收连接（stdio 连接器
  是子进程，漏掉就是每次调用泄漏一棵进程树）。
- **审计不含参数**：审计行记连接器、工具、结果、字节数与耗时，**不记 arguments**
  （那是调用方数据），也不记任何 header / env 取值。**不承诺结果脱敏**：MCP 结果是任意
  schema，只做体积上限——半脱敏的载荷比经审计的原样载荷更危险。
- **鉴权**：与其它 `/api/app-server/*` 一致，需就绪连接（`require_ready`）。
- **能力位是独立的 `connector_calls`**：它表示**方法存在**，不表示有工具可调——宿主可以
  接了代理而 `allow` 为空表，此时每次调用都回 `policy_denied`。
- **三种传输都支持**：stdio、Streamable HTTP 与 legacy SSE。SSE 的调用面与探针共用同一套
  流式握手（`wait_for_endpoint` / `sse_post_with_auth` / `wait_for_jsonrpc_response`），
  并同样带一次性 401 刷新重试；三者都走同一份「一个客户端、两个入口」的实现，不是三份客户端。

**stdio 会话在调用之间保留**——这是实现细节，**wire 面不变**（没有新增方法、字段或错误码，
指纹不动）。第二次调用只付一次 `tools/call`，不再重付解释器启动与握手；空转 5 分钟回收、
池上限 8，回收按**进程树**做。**HTTP / SSE 刻意不复用**：远端会话 id 由**对端**决定何时
过期，缓存它等于用「稳定成功的调用」换「省一次往返」，而 `reqwest` 本就在底下复用 TCP/TLS
——**只池化我们自己拥有的东西**（stdio 子进程是我们 spawn 的，生命周期完全可控）。

会话按**连接器 id + 配置与凭据**定位，两个选择都是刻意的：用 **id 而不是注册名**，因为
MCP server 按名字 upsert，后来安装的同名者会接管名字并因此继承前一个留下的会话（与
allowlist 收 id 同一条理由）；**env 按 `secret:` 解析后的值比较**，所以轮换凭据（
引用名不变、值变了）会换新会话，而不是拿旧凭据继续跑。调用**超时**或管道**断裂**时该会话
被丢弃：那时请求/响应是否还对得上已无从判断，复用会让**下一次**调用读到上一次还留在管道里
的答复。池满且都在忙时退回**一次性调用**——复用是优化，从来不是正确性前提。

#### 4.3.3 连接器工具签名读面（`ConnectorTool.input_schema`，`fp-2` 加入）

`connector/get` 与 `connector/test` 返回的每个工具带上**上游参数 schema**：

```text
ConnectorTool {
  name,            # 命名空间后的公开名
  description,     # 上游工具描述逐字
  input_schema,    # fp-2 新增：上游 tools/list 的 inputSchema，**逐字**
}
ConnectorDetail.tools_truncated        # fp-2 新增
ConnectorProbeResult.tools_truncated   # fp-2 新增
```

**为什么**：要人同意一个工具，就得让他看得见这个工具收什么参数。此前授权单位是逐个工具名，
而调用方只能在**看不到参数**的前提下把名字写进白名单（`26` §3.1 的「盲签」）。schema 上 wire
之后，授权单位才上移到连接器（§4.3.2）。

规则：

- **不新增方法**：`input_schema` 挂在既有的 `ConnectorTool` 上，于是 `connector/get`（自上次
  探针的缓存）与 `connector/test`（现场探针并落库，顺手刷新 get）两条既有读面**同时**生效；
  HTTP 路由与方法计数**不变**（`48 / 23`）。
- **逐字透传**：值就是上游 `tools/list` 的 `inputSchema`。宿主**早已**解析它并随探针落库
  （`McpToolResponse.input_schema`），这一层不改写、不裁剪、不注入，只做体积预算。
- **体积预算 `MAX_CONNECTOR_TOOLS_BYTES` = 1 MiB**：按 tools 的既有顺序累加；`name` /
  `description` **永远保留**（它们是「选哪个」的依据，且便宜），放不下的 `input_schema`
  **整份省略**并置 `tools_truncated: true`。**绝不截半个 JSON Schema**——半截 schema 会被
  调用方解析并相信。为什么不是整个响应回 `response_too_large`：仓里「拒绝优于静默截断」针对
  的是调用方会 parse 并相信的**载荷**；这里缺的是**显式标记的缺席**，而把一个大连接器变成
  「目录完全读不出来」是更糟的失败。
- **读面不加门**：`connector/get` / `connector/test` 走 `ConnectorCatalogProvider`，与
  `ConnectorCallProvider` 是两个 seam，`connector_calls` 只管 `connector/call`。工具**名字**
  早已在这个面上，schema 是同一能力的更高分辨率，**不是新面**；加门反而会弄坏既有 catalog UI。
- **新鲜度**：`connector/get` 的 tools 来自**上次探针落库**的结果，可能很旧、也可能是空数组
  （首次探针成功前恒为空）。要新鲜就先调 `connector/test`——零额外机制。

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

`install/disable` / `install/enable` / `install/uninstall` 的请求体另带
**非空且显式**的组件列表：

```text
{ "snapshot_id": "<已导入快照 id>", "component_ids": ["<组件 id>", …] }
```

空的 `component_ids` 一律 `invalid_request`——早期实现里空切片会被读成
「卸载整个快照」，那是「不点名就动全量」的写法；现在三个动词都必须点名。

`install/status` 响应（`AppServerInstallStatus`）：

```text
snapshot_id
components[] { id / kind / name / state / runtime_location / preset_id }
state: "not-installed" | "installed" | "disabled"
outcomes[] { component_id / kind / action / ok / code? / message? }   # 见下
errors[]                                                             # 人读失败摘要
```

`outcomes` / `errors` 只在**变更类**动词（`uninstall` / `disable` / `enable`）
上非空：它们描述「本次变更对每个组件做了什么」。纯 `install/status` **读取**
返回两者为空——它报告状态，不报告某次变更的结果。

运行时注册映射：

| 组件 kind | 运行时目标 | 说明 |
|---|---|---|
| `skill` | 服务端技能根 `{data_dir}/skills/agent-store/{snapshot_id}/{slug}/` | 系统 skill 扫描可发现；拷贝不执行 |
| `agent` / `team` | Preset（`PresetService.create`，名称 `agent-store: <name>`） | 预设列表可直接使用；不创建 ExecutionTemplate |
| `connector` | `mcp_servers` 表（`McpConfigService.add_server` 按名 upsert） | 运输层由组件 payload 的 `transport_summary` 推导 |

规则：

- 安装只复制/注册，**不执行**任何内容（02 §10）；凭据值永不进入安装状态列；
- **`install/run` 幂等/可重入**：同一 `snapshot_id` 调两次**不产生第二个
  Preset**——复用组件 `runtime_ref` / `preset_id` 列里记下的那个 Preset id。
  它**刻意不按显示名认领** Preset：两个快照合法地可以声明同名，按名匹配会
  把别人的 Preset 抢过来。记录里的 Preset 被手工删掉时**重建**一个。
  对**被禁用过的**组件重新安装会同时把运行时侧重新启用（
  `mark_components_installed` 清除 `disabled`，运行时必须跟着回来，否则记录
  会声称「已启用」而运行时仍是关的）；
- **`uninstall` 真正释放运行时产物**，按 kind 分派：`skill` →
  删物化目录 `{skills_root}/agent-store/<snapshot_id>/<slug>/`（是该快照最后
  一个 skill 时连带剪掉快照根）；`agent` / `team` → `PresetService::delete`
  删 Preset；`connector` → 按**记录下来的 id**（绝不按名）删 `mcp_servers` 行。
  顺序是**先释放、后清记录**，且**只清那些真的释放成功的组件**；快照与组件行
  永远保留（历史可追溯）；
- `uninstall` 是**可重入**的：产物本就不在了算**成功**，不是失败；
- **释放失败即保留 `installed=1`**，记录也不清——失败因此可重试，且指向残留
  产物的指针不会丢。Uninstall 绝不声称删掉了它其实留在盘上的东西；
- **路径安全**：记录下来的位置必须是**单个路径段**，托管路径上任何一处符号
  链接一律拒绝；
- **`enable`/`disable` 真的移动运行时状态**（不再只写插件自己的标志位——那样
  写的 `disabled` 运行时侧**没人读**）：`connector` → **置**（set，不是 toggle）
  `mcp_servers.enabled`——toggle 表达不了「把它关掉」这个幂等请求，重试会把
  它又打开；`agent` / `team` → 置 Preset 的 `enabled` 标志（运行路径无需改动：
  `PresetService::resolve` 本就拒绝 disabled 的 Preset）；
- **`skill` 例外（已文档化）**：技能语料是普通目录、**没有启用态**，所以
  `disable` 对 skill 只是**目录标记**，对运行时无任何效果——outcome 报
  `action: "marked"` + `code: "skill_disable_flag_only"`。要把技能从运行时
  移除，唯一路径是 `uninstall`；
- 记录里的标志位**只对运行时状态真的动了的组件翻转**——否则目录又会声称一个
  运行时从未进入的状态；
- **已记录的不对称**：新注册的 connector 起始就是 `mcp_servers.enabled = false`
  （安装器既有的默认值），因此刚装完时插件标志与 MCP 标志**合法地不一致**；
  是 `install/enable` 把它打开的；
- 文档化状态机：`not-installed → installed → disabled →（enable）installed`；
  卸载任意时刻可用。

#### 4.5.1 MCP server 的来源与优先级（`21` D14）

本机可以自己声明 MCP server，不必先经过市场 / 快照安装。宿主读 `~/.agent-store/mcp.json`
（用户级，`{"mcpServers": {…}}`，与 Kimi Code CLI **同名同形**；**项目级
`<workspace>/.agent-store/mcp.json` 预留但未启用**）。参考实现文档里的可选字段**全部支持**
（`env` / `cwd` / `headers` / `bearerTokenEnvVar` / `enabled` / `startupTimeoutMs` /
`toolTimeoutMs` / `enabledTools` / `disabledTools`），schema、逐字段落点与 server key 校验
规则见 `20` §7.9／§7.9.1。

一次会话构建时，MCP server 的合并优先级（从强到弱）：

1. **请求级绑定**（`resolve_mcp_servers`：宿主请求 / 网关配置携带的 server）——既有语义不变；
2. **`mcp.json` 声明**——文件里声明的 server；
3. **`mcp_servers` DB 行**——快照 / 市场导入或 Allo UI 注册的 Connector，仍受本会话
   Connector id 栅栏的约束（见上表）；
4. 会话快照 server（owner-only；App Server 会话不合并这一类）。

`[tools]` 的 `enabled` / `disabled` 永远**最后**求交，因此声明不能扩大任何既有收窄。
server 级的 `enabledTools` / `disabledTools` 在**注册之前**裁剪（`20` §7.9.2），所以它只
决定「这个 server 贡献哪些工具」，既不改变上面这条优先级，也不改变 `[tools]` 的最后一道地位。

两条必须知道的边界：

- **可见性**：文件声明的 server **不在** `connector/*` 目录里、**不可被** preset 的
  `mcp_server_ids` 引用、**没有**持久化的 `last_test_status` / `tools`，`connector/test`
  也不覆盖它——它不投影进 `mcp_servers` 表（`21` D14 ②=C）。唯一读面是 `config/get`
  的 `mcp` 段（§4.10）。
- **作用域**：声明是**宿主级能力**，与 `[tools]` / `[credentials]` 同性质——App Server
  会话即使没有绑定任何 Connector 也会拿到它。那道栅栏约束的是「快照 / preset 授予了
  什么」，不是「宿主操作者在自己的机器上声明了什么」。

#### 4.5.2 结构化结果（`outcomes`，2026-09-15 加入）

`AppServerInstallOutcome { component_id, kind, action, ok, code?, message? }`：

- `action` ∈ `created | reused | enabled | disabled | marked | removed | skipped | failed`
  （`marked` 即上面 skill 的标记态）；
- `ok: false` **只在请求的状态没有达到时**出现；组件**本就处于**请求状态时报
  `ok: true`；
- `code` 是**稳定的文档化 token**（下表是**闭集**）；`message` 给人读，
  **没有任何东西解析它**——这个字段存在的意义就是调用方不必去匹配散文。

| code | 出处 | 含义 |
|---|---|---|
| `source_component_unmatched` | 服务端 | 快照里找不到该组件 |
| `preset_lookup_failed` | 服务端 | 查 Preset 失败 |
| `preset_create_failed` | 服务端 | 建 Preset 失败 |
| `preset_state_failed` | 服务端 | 置 Preset `enabled` 失败 |
| `preset_delete_failed` | 服务端 | 删 Preset 失败 |
| `skill_remove_failed` | 服务端 | 删技能物化目录失败 |
| `connector_register_failed` | 服务端 | 注册 `mcp_servers` 失败 |
| `connector_state_failed` | 服务端 | 置 `mcp_servers.enabled` 失败 |
| `connector_delete_failed` | 服务端 | 删 `mcp_servers` 行失败 |
| `connector_name_collision` | 服务端 | 同名 connector 冲突 |
| `cli_connector_unsupported` | 服务端 | CLI 连接器不支持该操作 |
| `component_kind_unsupported` | 服务端 | 组件 kind 不支持 |
| `component_ref_invalid` | 服务端 | 组件引用非法（含路径安全判定） |
| `component_not_installed` | 服务端 | 组件未安装，无从操作 |
| `skill_disable_flag_only` | 服务端 | skill 的 `disable` 只是目录标记（见上） |
| `ready_timeout` | **SDK 客户端** | `store` 子客户端就绪检查超时 |
| `authorization_required` | **SDK 客户端** | 就绪检查撞上需人工完成的 OAuth |

后两者由 SDK 的 `store` 子客户端在**客户端侧**产生（`07` §4.3），不是服务端
返回的。

`AppServerInstallResult`（`install/run` 的响应）同样新增 `outcomes`；它的
`errors` 数组现在**真的**携带失败（不再恒为 `vec![]`）。
`AppServerStoreInstallResult`（`store/install-entry`）也从安装器**转发**
`outcomes`，因此一键商店安装与直接 `install/run` 一样可分支判断（§4.7）。

全部为**纯加法**：**没有新增或删除任何方法**，`46 / 65` 的映射/未映射计数
守卫不变。协议指纹随之 `2026-09-14` → **`2026-09-15`**（现有 DTO 加字段即
wire 变更；规则仍是「指纹必须与上一个不同」，同日第二次变更取次日戳）。

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
  published_at,                        # 市场声明的发布日期 YYYY-MM-DD，缺席=未声明（§5 规则）
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
- `published_at` 是**市场自己声明的**发布日期（`18` §4.2）：服务端只接受严格
  `YYYY-MM-DD` 且月/日真实存在，其余一律丢弃且**不阻断条目**；**绝不派生**
  （不用导入时间 / 快照 `added_at` / 市场刷新时间顶上）。缺席 = 这个市场没有
  声明日期，客户端不得渲染占位。真实市场（普查 2026-09-10）尚未携带该字段，
  因此 WebUI 的「最新」排序在该 kind 一条日期都没有时**不出现**——没有数据的
  排序控件是死控件；
- `store/install-entry` 幂等且**版本感知**：wire 上**没有**更新动词
  （`store/update-entry` 不存在），所以客户端唯一的升级路径就是「卸载，再安装
  一次」。因此：条目**已安装** → 仍是 no-op（`reused=true`）——在这里重新导入
  等于把「安装」变成一次隐藏的升级；条目**未安装**且快照版本 == 市场当前版本
  → 装那个快照；条目**未安装**且版本不同（前进或回滚）→ 经
  `market/entry-import` **重新导入**并装新快照，旧快照保持不可变、历史保留。
  版本推导收敛为**一个共享 helper**（`entry_live_version`），`store/list` 与
  `install_entry` 共用，目录与安装器因此不可能各说一套。响应
  `{ snapshot_id, version, installed_count, outcomes[], errors[] }`——`outcomes`
  从安装器转发（§4.5.2），一键商店安装与直接 `install/run` 一样可分支判断；
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
    {"kind": "skill", "id": "release-notes"},
    {"kind": "connector", "id": "0190f5fe-...-000000000020"}
  ]
}
```

> **`id` 的来源按 kind 不同（易错）**：`agent` 用 `agent/list` 的条目 id（`wb-<plugin>-<slug>`
> 形态）；**`skill` 用 `skill/list` 公布的 `id`，也就是技能名本身**——`skill/list` 目前把
> `id` 设为技能名（`AppServerSkillCatalog`），**不是** `install/status` 里的组件 id。
> 传组件 id 会被 `SkillId::parse` 判为非规范、降级成 `legacy:<组件id>`，随后按名查不到，
> **静默不挂载**（不是报错）。`connector` 用 `connector/list` 的 MCP server id（UUIDv7）。

每类 mention 的运行时语义：

| kind | 注入点 | 约束 |
|---|---|---|
| `agent` | 通过 `agent/get` 的 `preset_id` 选择运行 preset（替换 `agent_id` 字段）；overrides 沿 preset resolve 面展开 | 至多一个；target AgentDefinition 必须已 `install/*`（`agent_not_installed`；**id 根本不存在则是 `not_found`**——两者语义不同，2026-09-23 真机确认）；与显式 `agent_id` 冲突返回 `invalid_mentions` |
| `skill` | 挂载到 `included_skills`（冻结进 `ResolvedPresetSnapshot`，随 run 上下文交给 Agent） | 只记录/挂载；不执行、不展开正文 |
| `connector` | 追加到 `mcp_server_ids`（经既有的 connector 存在+enabled 校验后注入 run） | 必须是存在的已启用 MCP server |

规则：

- `mentions` 可省略（向后兼容），此时行为与旧 `agent/run` 一致；
- 未知 kind 反序列化失败（严格 wire 契约）；
- agent-store 安装 preset（`agent-store: <name>` 命名）在 `validate_agent_store_preset_source`
  白名单内（Builtin+builtin-office 保持不变）；任意用户 preset 仍被拒绝；
- preset 未绑定 model 时，服务端**先取宿主自己的** `default_model`
  （`~/.agent-store/config.toml`，与会话创建 / `team/run` 同源），再回退到 provider 注册表里第一个启用的
  provider/model（`default_run_model`），避免 `resolved_model=None` 在运行时边界被拒；
  回退重解析**保留 mention overrides**（`include_skills` / `mcp_server_ids`
  不丢失）。
  > **2026-09-23 修（既有缺陷）**：此前这条回退**只**读 provider 注册表，而 config 里的 provider 是
  > **按需注册**的（其它路径解析模型时才写库）。于是「全新宿主、还没解析过任何模型」时 `agent/run`
  > 会以 `invalid_request`（`resolved_model is required`）失败——真机上复现过。现在它与会话 / 团走
  > 同一个来源，**不再依赖"别的调用先发生过"**。

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
  ],
  "mcp": {
    "exists": true,
    "adopted": true,
    "servers": [
      { "name": "filesystem", "transport": "stdio", "enabled": true },
      { "name": "linear", "transport": "http", "enabled": true }
    ],
    "rejected": [
      { "name": "with-cwd", "reason": "`cwd` is not supported (…) — remove it to load this server" }
    ]
  }
}
```

- 文件缺失是**正常答案**（`exists:false` + `default_model: null` + 空 providers），
  不是错误；文件存在但读不动 / 解析失败返回 `config_unavailable`——不拿默认值把
  「文件坏了」伪装成「文件没写」。
- `default_model` 无声明时是显式 `null`，与「尚未读取」区分。
- `providers` 是**文件里声明的事实**（config-only 投影，与 `models/list` 的 config
  分支同源），不含 `api_key` / `base_url`，也不含已注册 provider 行。
- `mcp` 是 `~/.agent-store/mcp.json` 的投影（§4.5.1）：`servers` 是按 key 排序的
  已接受条目（`transport` ∈ `stdio|http|sse`），`rejected` 是逐条目拒绝的原因（**未知**
  字段、放错传输的字段、超时越界、非法 key、结构冲突），`error` 则是**整份文件**读不成
  声明时的原因（JSON 非法、顶层不是对象）——没有它，一个写坏的文件与一个空文件在界面上
  完全一样。`mcp.json` 缺失时该字段是显式 `null`；文件存在但读不动时 `exists:true` + 空
  列表 + `error`。`env` / `headers` 的**值**（含明文凭据）永不进入此视图，只有 key 名会；
  声明的 `cwd`、`bearerTokenEnvVar` 与工具过滤条目同样**不上 wire**（视图只报 `name` /
  `transport` / `enabled`）。**这份 verdict 视图仍然不含任何凭据值**；文件自己的文本只经
  **一个**读面 `config/get-mcp` 出去（见下），它是编辑器专用、由编辑器按需调用——不是把
  整份文件塞进每次 `config/get`。该文件仍**不在** `config/set` 的白名单里——它有自己的
  读/写方法（见下），因为它们写的是**另一份文件**、失败原因必须带行列号、响应要是 `mcp`
  视图而不是 config.toml 视图。
- **`mcp.json` 的读/写面（读 `2026-09-18`、写 `2026-09-17` 加入）**：与 `config/*` 同口径
  ——**仅 WebSocket、无 HTTP 绑定、不进 SDK 包**，走同一条 `require_ready` owner 闸门，
  **没有任何路径参数**（文件由宿主自己的配置位置 + 同级 `mcp.json` 推出，读写面共用同一个
  `mcp_declaration_path`，不可能指向两份文件）。

  ```text
  WS   config/get-mcp          {}
  WS   config/set-mcp          { "source": "<mcp.json 全文>" }
  WS   config/set-mcp-enabled  { "name": "<server key>", "enabled": true | false }
  ```

  - `config/get-mcp` 返回 `{ exists, source? }`：**唯一**把声明的取值交给客户端的读面，
    存在的唯一理由是**编辑器无法编辑它看不见的文件**（操作者编辑的是自己机器上的自己那份
    文件）。它刻意不做成 `config/get` 的又一个字段——那个视图描述文件的**裁决**，而每个
    设置对话框一打开就会调它。文件缺失是 `exists:false`（正常答案，与 `config/get` 一致）；
    读不动是错误而不是空串——**静默从 "" 开始的编辑器会覆盖一个只是读不到的文件**。文本
    无论能否解析都返回：编辑一个写坏的文件正是编辑器的用途，而 `config/get` 会在旁边报出
    解析裁决；
  - `config/set-mcp` 写**全文**，但**先用自己的解析器验一遍**：解析不过就**一个字节都不写**，
    把解析器原始原因（含行列号）回给调用方。这是写面与读面的关键差别——读面 fail-open
    （坏文件照样投影出来给你看），写面 **fail-closed**（不接受制造出那个状态的请求）；
  - `config/set-mcp-enabled` 只改那一条目的 `enabled` 成员（**不是** `disabled`：那个键是
    未知字段，会让整条被拒），且是**文本级最小编辑**：值就地替换；成员缺失且要禁用时按文件
    自己的换行与缩进插入；成员缺失且要启用是**零改动**（缺省即启用）。文件是手写的，开关没有
    资格重新缩进、重排键或删掉用户写的行——与 `config/set` 用 `toml_edit` 是同一个理由。
    定位不明确时**拒绝**而不是悄悄重排版；无字节变化时**根本不写盘**（重复调用不动 mtime）；
  - 只允许切换**已接受**的条目：被解析器拒绝过的条目报 `mcp_server_rejected`——对一条宿主
    根本不读的条目回「切换成功」是最坏的答复。`mcp.json` 不存在时报 `mcp_server_not_declared`；
  - 两者都返回**写后重读**的 `AppServerConfigView`（与 `config/set` 同一条落点规则：调用方
    看到的是磁盘上的内容，不是请求的回声）；
  - **稳定错误码**：`mcp_source_invalid`、`mcp_server_not_declared`、`mcp_server_rejected`、
    `mcp_source_not_surgically_editable`、`mcp_write_failed`；
  - **代价**：这条写面让 WebUI 的一个请求可以导致宿主启动本地命令（`stdio` 声明）。防线全部
    复用既有面：owner 闸门、仅回环 WS、无路径参数、同目录临时文件 + `rename` 原子写；界面上
    `adopted` 三态照旧显式呈现，回答「这份文件在本机到底有没有被读」。
- **`adopted` 描述宿主，不描述文件**（2026-09-14 加入，指纹随之 bump 到 `2026-09-14`）：
  `servers` / `rejected` / `error` 都只说这份**文件**里有什么，而读盘是无条件的——宿主没开
  `--adopt-store-mcp-declarations` 时也照样投影。于是「宿主根本不读这份文件」与「宿主把
  每条都注入了会话」在只读渲染下**完全一样**，`adopted` 就是为这一对存在的。
  **三态**：`true` / `false` 是宿主自己的回答，字段**缺席**表示宿主没上报（比该字段更旧的
  构建）——不折叠成 `false`，因为 `apps/agent-store` 在该字段存在之前就已采用声明，折叠
  会把「不知道」写成「没采用」。`exists:true, adopted:false` 是合法且有意义的组合。

`config/set` 只接受白名单字段（`default_model` / `memory.distill_enabled` / `tools.*`——
本文档此处原先写作「当前仅 `default_model`」，与代码早已不符，2026-09-17 一并订正）：
请求里出现 `api_key` /
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
team/run（已实现：Leader Conversation + planned 委派）
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
  "mentions": [{"kind": "skill", "id": "wb-demo-release-notes"}],
  "model": {"provider_id": "opencode", "model": "mimo-v2.5"},
  "reasoning_effort": "high"
}
```

`agent/run` 的 `model` / `reasoning_effort`（`fp-6` 加入）是**运行级**选择，可选：`model` 的
优先级是 **显式 > preset 自带 > 宿主默认**（`~/.agent-store/config.toml` 的 `default_model`）——
给出即**无条件**覆盖 preset 里绑定的模型，解析与 `conversation/*` 同一函数（已注册 provider UUID
原样使用，`config.toml` 的 provider 名幂等注册）；`reasoning_effort` 作用**整次运行**（随冻结快照
落到每个 attempt 的会话 `extra.reasoning_effort`，因此每个 attempt 用同一个等级），词表与
`conversation/*` 相同，引擎是否真的用上取决于该模型在目录里是否声明了等级。两者缺省时行为与
从前逐字一致；由于请求体会被**幂等指纹**序列化，缺席的字段不进入指纹（`skip_serializing_if`），
一次纯升级不会让既有收据失配。

`agent/run` 的稳定错误码：`invalid_request`、`version_mismatch`、
`agent_not_installed`、**`preset_disabled`**（解析出的目标 Preset 处于 disabled
状态——`install/disable` 真的关掉了运行时，所以运行在这里被拒，且给出**稳定
码**而不是一句泛化消息）、`connector_unavailable`、`runtime_unavailable`、
`unsupported_operation`。

`team/run` V1 示例：

```json
{
  "team_id": "team_01...",
  "team_version": "1.0.0",
  "goal": "完成目标",
  "workspace": {"id": "ws_01..."},
  "idempotency_key": "client-op-02"
}
```

**没有 `planning` 块**：`16` §7 决策 3 撤销了「由客户端/模型指定 planning 参数」的写法。成员池、`max_parallel`、`routing_constraints` 与权限一律取自绑定的 Team 模板与服务端策略，服务端计算有效值的最小交集。请求 DTO 是 `deny_unknown_fields`，带 `planning` / `members` / `max_parallel` 之类字段一律 `invalid_request`——**拒绝**而不是静默忽略，否则客户端会以为自己设的参数生效了。

`team/run` 的执行链（同一决策）：

1. 按 `team_id`（可选 `team_version`，不一致直接 `version_mismatch`）解析 TeamDefinition 与其成员 AgentDefinition；
2. 把成员的 preset + 各自 Skill 物化成该 Team 的 `AgentExecutionTemplate`（已存在则复用；`workflow_limits.max_parallel` 只在是正整数时作为并发上限），并绑定为该会话的 `execution_template_id`；
3. 服务端创建 **Leader Conversation**（`conversation/create` 同族的 App Server 会话，`delegation_policy = automatic`），Connector 栅栏＝该 Team 快照**已安装且启用**的 Connector id，Skill 快照＝Lead AgentDefinition 的 Skill；
4. 把 `goal` 作为 Leader 的第一轮 turn 发出。Leader 在这一轮里调用 `nomi_delegate(strategy="planned", goal=…)` 并结束 turn；它只能给 `goal`，并发/审批/重规划策略由宿主给定；
5. 服务端从该会话的 `lead` execution link 反查 Leader 创建的执行，映射成公共 `run_id` 后返回 receipt。

因此 `team/run` 的 **receipt 形状与 `agent/run` 不同**：Team Run 没有 lead preset，其权威是 Team 的模板，所以返回 `{run_id, status}`（`TeamRunReceipt`），不带 `preset_revision` / `content_digest`——那两个字段属于 `agent/run` 的冻结 preset 快照，填进 Team Run 只能是伪造值。

`team/run` 的**等待边界**：它等待的是 Leader 的那一轮 turn（本轮只做「调用一次委派工具并结束」，规划与成员工作都在引擎里异步进行），**不等**整个 Team Run 完成，与 `05` §5.2「不阻塞等待长任务完成」一致。

稳定错误码：`invalid_request`（goal 缺失/带 planning 块）、`version_mismatch`、`agent_not_installed`（成员未安装）、**`agent_disabled`**（**点名**那个成员：其 Preset 被 disabled，被禁用的专家不再只是「跑了没反应」）、`team_member_model_unbound`（成员 preset 既无模型、宿主也没有可用 provider）、`connector_unavailable`（Team 绑定的 Connector 被禁用）、`team_run_not_started`（Leader 那一轮没有发起任何执行——错误信息带上 Leader 会话 id，便于人工继续）、`runtime_unavailable`、`unsupported_operation`。

成员检查的**时机是被刻意选定的**：`team/run` 在**每一次运行**都先于 Connector 栅栏、在 `resolve_team_members` 里把**每个**参与者查一遍（同一遍里同时报 `agent_not_installed` 与被禁用的 `agent_disabled`），**不**放在模板物化阶段——`ensure_team_template` 会复用既有模板，检查埋在那里只会在**第一次** Team Run 触发。

能力协商：`InitializeResult.capabilities.team_runtime` 为 `Team 目录 ∨ 执行 facade` 同时在场时才为 `true`。

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
| `conversation/get` | 单会话视图（含当前的 `reasoning_effort`，`fp-6` 加入；缺席＝未指定） |
| `conversation/messages` | 分页历史（升序，`page_size` 1..=200）。响应体为 `{ "items": [...], "has_more": bool }`：`items` 为消息数组（升序），`has_more` 是服务端 keyset 算出的精确标志——还有更旧消息时为 `true`。客户端以 `cursor: ""` 取最新窗口，以最旧已加载消息的 `created_at:message_id` 翻更早页；"向上加载"应直接读 `has_more`，不要以"页是否装满"近似推断 |
| `conversation/send` | 发送消息，必带 `idempotency_key`；可选 `attachments`、`mentions`（见下）、`model` / `reasoning_effort`（`fp-6`，**粘性**切换，见 §12.4） |
| `conversation/cancel` | 停止当前 turn |
| `conversation/delete` | 删除 App Server 会话（HTTP 为 `DELETE /api/app-server/conversations/:id`）；委托既有会话删除语义：运行中先按 stop/orphan fence 处理，保留的 execution transcript 拒绝删除，删除失败返回稳定错误码 |
| `conversation/subscribe` / `conversation/unsubscribe` | 实时事件订阅 |

`conversation/send` 请求体（HTTP 与 WS 同形）：

```json
{
  "conversation_id": "<会话 id>",
  "content": "消息正文",
  "idempotency_key": "<客户端幂等键>",
  "attachments": ["<会话工作区内的绝对路径>"],
  "mentions": [{ "kind": "skill", "id": "<skill/list 的 id>" }],
  "model": { "provider_id": "<已注册 UUID 或 config.toml 的 provider 名>", "model": "<模型名>" },
  "reasoning_effort": "high"
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

### 12.2 以专家 / 专家团开场（`conversation/create` 的 `agent_id` · `team_id`，`fp-4` / `fp-5` 加入）

`agent_id` 是 `agent/list` 的 AgentDefinition id、`team_id` 是 `team/list` 的 id；两者**互斥**
（同时给是 `invalid_request`：一个会话要么属于某个专家，要么是某个团的 Leader）。语义与
`agent/run` / `team/run` 一致，但**作用域是整个会话**：

**专家（`agent_id`）**

- **解析**：`agent/get` → 它的 `preset_id`；未 `install/*` ⇒ `agent_not_installed`，
  **id 不存在 ⇒ `not_found`**（2026-09-23 真机确认的两种码），
  Preset 被 `install/disable` ⇒ `preset_disabled`，来源不在 agent-store 白名单 ⇒ 与
  `agent/run` 同一处校验拒绝。宿主未接预设服务 ⇒ `runtime_unavailable`。
- **冻结什么**：该专家的 preset **快照**（`preset_id` / `preset_revision` / `preset_snapshot` 三列）
  与**它自己声明的技能、连接器**一起冻进这一行。技能走 `preset_enabled_skills`，连接器走显式
  id 栅栏；宿主 auto-inject 的技能**照旧排除**——专家的 preset 自己不带排除名单（安装器写的是空），
  所以这道栅栏由会话层补，不能被快照覆盖。
- **连接器不可用**：专家声明的连接器若被停用 ⇒ `connector_unavailable`（**不是**悄悄少绑一个）。

**专家团（`team_id`）**

- **同一段编排**：复用 `team/run` 的 `prepare_team_leader_conversation`（解析成员 →
  逐个查 `agent_not_installed` / `agent_disabled` → 连接器栅栏 → 物化或复用执行模板 →
  建 Leader 会话），因此**不会**出现「`create` 建的 Leader 与 `team/run` 建的不是一回事」。
- **唯一区别**：**不发 `goal` 首轮**。客户端自己的第一条 `conversation/send` 就是 Leader 的首轮，
  `delegation_policy` 已是 `automatic`、模板已绑好——Leader 在这一轮里**可以**调用 `nomi_delegate`。
  > **实测口径（2026-09-23）**：是否委派**由模型决定**，不是这一轮的强制契约——同一条自然语言指令
  > 真机 6 次里 2 次委派（拿到 `execution_id`、宿主进入 planning）、4 次自己动手做；既有入口
  > `team/run` 在同一条件下也会 `team_run_not_started`。逐条读数见
  > `27-conversation-binding-plan.zh.md` §9.1。
  > 需要委派时请在指令里明确点名 `nomi_delegate`（真机验证过：那会稳定触发，拿到 `execution_id`）。
  > 另：同一会话在上一轮执行未完成时再委派会被拒（`Conflict: conversation already has an unfinished
  > Agent Execution`），普通消息则会被拒为 `Conflict: Conversation already has an authoritative local turn owner`。
- **错误时机**：成员与连接器的检查都在**创建时**发生，不会先开出一个残缺的 Leader 会话。
- **边界**：`conversation/create` 只接受 `team_id`，**不接受** `team_version`（版本钉住走
  `team/run`）；Leader 会话的模板按 `team_id` 复用，与该 Team 后续的 `team/run` 共用同一份。

**两者共同**

- **之后不可改写**：`conversation/update` 明确拒绝 preset / 技能 / MCP 三类键，所以
  **换专家或换团 = 新建会话**。这是刻意的语义，不是欠账。
- **纯加法**：两个字段都不传时是普通会话，wire 形状与行为逐字不变。

### 12.3 每轮技能（`conversation/send` 的 `mentions`，`fp-3` 加入）

`mentions` 与 `agent/run` 同形（`{ "kind", "id" }`），但**只认 `kind: "skill"`**：

- **技能是每轮载荷**：`id` 交给会话层既有的显式技能路径解析成不可变快照
  （`resolve_requested_skill_snapshots`），正文随这一轮的 prompt 走。**创建时冻结的技能快照
  不受影响、也不可改写**——会话的技能/MCP/预设快照在 create 之后是只读的
  （`ConversationService::update` 明确拒绝这三类键）。
- **`id` 用 `skill/list` 公布的 id（即技能名）**，不是 `install/status` 的组件 id；
  传组件 id 会在 `SkillId::parse` 处降级、随后按名查不到而**不挂载**（`05` §4.8 的同一处口径）。
- **另外两类显式拒绝**：`kind: "agent"` 与 `kind: "connector"` 返回 `invalid_request`。
  它们在 `send` 上没有载体——专家是会话身份、连接器是宿主级开关——**拒绝而不是静默忽略**，
  否则调用方会以为自己挂上了。
- **失败早于占用幂等键**：技能不存在、快照超限、内容非法都会让整次 send 失败，且失败发生在
  durable receipt 落库与会话转 Running 之前。
- **纯加法**：不传 `mentions` 时 wire 形状与行为逐字不变；同一轮里重复点同一技能只算一次。

`conversation/create` 请求体：

```json
{
  "name": "可选名称",
  "model": { "provider_id": "<已注册 UUIDv7>", "model": "mimo-v2.5-free" },
  "workspace": { "id": "<可选已注册 workspace>" },
  "reasoning_effort": "可选 low / medium / high / xhigh",
  "agent_id": "<可选：agent/list 的 id，把会话建成该专家>",
  "team_id": "<可选：team/list 的 id，打开该专家团的 Leader 会话；与 agent_id 互斥>"
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

### 12.4 模型解析与 agent-store 配置

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

**随消息切换（`conversation/send` 的 `model` · `reasoning_effort`，`fp-6` 加入）**

`send` 也接受这两个字段，语义是**会话级设置的粘性切换**——**不是**「只影响这一轮」：

1. 值被写进会话行，**从这条消息起生效**，此后每一轮沿用。Nomi 运行时是按会话行构建的
   （换模型立即重建运行时、换等级在下一个 turn 边界重建），所以「发送前把设置切好」恰好
   就是本轮生效；要还原就再发一次带旧值的调用。真·一次性需要引擎级的每轮模型通道，
   不是本协议的语义（`29-send-model-and-effort-plan.zh.md` §4）。
2. **只在值真的不同时才写**：与行上现值逐项比较，全相同则**不写库、不广播**
   `conversation.listChanged`。因此不带这两个字段的调用与从前**逐字一致**（零副作用）。
3. **会话正跑着一个 turn 时拒绝**（`conflict`）：换模型要拆掉运行时，不能在轮中做。
   其余可预期拒绝（附件越界、mention 非法、模型/等级解析失败）都发生在写库**之前**。
4. 带同一个 `idempotency_key` 的重放会**再写一遍同样的值**（幂等）：receipt 仍是
   `replayed: true`——重放不代表发生了一次新 turn，但配置确实已经是新值。
5. **读回**：`ConversationView.reasoning_effort`（`conversation/get` / `list` / `create` /
   `update` 都返回该字段；缺席＝未指定）。这条是必需的——`create` / `update` / `send`
   三条路都能写等级，若视图不投影它，这个设置就是只写不读。
6. **是否真的生效取决于模型能力**：引擎只在 catalog 为该模型声明了等级时才把
   `reasoning_effort` 送上 wire，否则**静默退回模型默认**（与 `create` / `update` 同一口径，
   本协议不为它新增错误码）。词表仍是 `low` / `medium` / `high` / `xhigh`，非法值
   `invalid_request`。

### 12.5 实时事件

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

#### 12.3.1 列表投影通知（`conversation/list-changed`，2026-09-19 加入）

转写事件之外，服务端还会推**列表投影**的变更——侧栏展示的是整份会话列表，与「某条被订阅
会话的转写」是两件事：

```json
{
  "jsonrpc": "2.0",
  "method": "conversation/list-changed",
  "params": { "conversation_id": "<uuidv7>", "action": "created | updated | deleted" }
}
```

三个不变量：

- **不要求订阅**该会话：用户此刻往往正看着另一个会话，而列表是全局的；
- **不带 `sequence`**，客户端不得据此推进 `lastSeenSequence`（它不是转写帧，也不参与
  §12.5 的缺口检测）；
- **尽力而为**：丢一条只是界面晚一步刷新，`conversation/list` 始终是权威来源。

`action` 只承认上述三态（`created` / `updated` / `deleted`；将来新增第四态属于 wire 变更，
要动协议指纹）。触发场景：`conversation/create` → `created`；重命名与**自动标题**（首条消息
几秒后由服务端异步生成）→ `updated`；`conversation/delete` → `deleted`。

### 12.6 能力边界

- `conversation/create`（单 Agent 聊天）使用 `DelegationPolicy::Disabled`：一轮
  单 Agent 对话没有 `nomi_delegate`。**Team 层是唯一的例外**，且它由
  `team/run` 自己的接缝创建（`delegation_policy = automatic` +
  `execution_template_id`），客户端无法在 `conversation/create` 上申请它；
- Skill 自动注入与进程级 MCP 配置在创建与运行时两侧都被禁用：会话只看到
  Definition 显式绑定的 Connector（按 id 的栅栏，空 = 一个都不绑）与 Skill 快照
  （仅保留普通本地 Agent 工具）；
- 创建会话时自动注册 owner 受控 workspace（`conversation/create` 无需
  先注册 workspace）；

---

## 13. V1 验收用例

- `TC-AS-001`：initialize/initialized 能力协商（单 Agent 能力为 true，Team/Skill/MCP 等为 false）；
- `TC-AS-002`：Catalog 方法返回 Agent/Team/Skill/Connector；
- `TC-AS-003`：agent/run 返回异步 run receipt（public opaque run_id）；
- `TC-AS-004`：team/run 创建 Leader Conversation 并以公共 opaque run_id 返回其发起的执行（`16` §7 决策 3）；
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

#### TC-API-005：MCP 声明文件的读/写面（2026-09-17 / 2026-09-18 加入）

- 等级：P1（宿主管理面：仅 WS、无 HTTP 绑定、不进 SDK 包）
- 操作与断言（`05` §4.10、`21` D17）：
  1. `config/get-mcp`：文件缺失 → `exists:false` 且无 `source`；**读不动 → 报错，绝不回空串**
     （静默从 `""` 开始的编辑器会覆盖一个只是读不到的文件）；文本能否解析**都要**返回；
  2. `config/set-mcp` **fail-closed**：写一段解析不过的文本 → 稳定码 `mcp_source_invalid`、
     message 带解析器自己的行列号，且**断言磁盘字节未变**；写合法文本 → 响应是**写后重读**的
     `config/get` 视图；
  3. `config/set-mcp-enabled` 是**文本级最小编辑**：值就地替换；成员缺失且要禁用 → 按文件自己的
     换行与缩进插入 `"enabled": false`；成员缺失且要启用 → **零改动**；重复同一请求 → **字节与
     mtime 都不变**；定位不明确 → `mcp_source_not_surgically_editable`（不重排版）；
  4. 只接受**被解析器接受**的条目：`{}`（无传输）或含未知字段 `disabled` 的条目 →
     `mcp_server_rejected`；不存在的 key → `mcp_server_not_declared`；
  5. `config/get.mcp` 的 verdict 视图**仍不含**任何 `env` / `headers` 取值（原文只经
     `config/get-mcp` 出去）；
  6. 三个方法都**没有路径参数**：没有任何请求能指定被读写的文件。
- 自动化落点：`nomifun-api-types --lib mcp_declarations`（30 例，含 7 例扫描器/拒绝语义）、
  `nomifun-app-server --lib`（123 例）；见 `16` §8.3。


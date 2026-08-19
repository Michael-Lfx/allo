# Session Observation Workflow UI — 执行计划（供 Review）

> **文档状态：待 review，未授权改实现代码**  
> 日期：2026-08-19  
> 分支：`feat/session-observation`  
> **本文读者：** 审查 agent（先读完再给意见）与实施 agent（授权后按阶段做）  
> **本文不是新契约。** 运行时 / 存储 / 词汇仍以  
> [agent-observability-and-eval.zh.md](agent-observability-and-eval.zh.md) 与  
> [session-observation-workflow-proposal.zh.md](session-observation-workflow-proposal.zh.md) 第 7 节为准。

对照截图：

| 图 | 是什么 |
| --- | --- |
| **图 1（当前 Flowy）** | Developer Drawer：`AgentTraceInspector` + `ObservationWorkflow`。左回合摘要，右整段 REQUEST JSON + interrupted 文案。 |
| **图 2（目标信息密度）** | `dsh-plugin-agent-workflow` 的 Workflow 页：左用户回合、中 Model Call 横向卡片流、点 REQUEST 后三列系统 / messages[] / tools。 |

**拍板：** 对齐图 2 的**信息与呈现**，继续挂在图 1 的 Drawer，不装插件、不上一等 Tab。方案编号 **U0–U3**（本轮 UI 刀）。S9+ / 提案 M3 其余项 / M4 仍延后。

审查时请按第 11 节清单给意见。未授权前不要改业务代码。

---

## 1. 背景与问题

S0–S8 已落地：JSONL 采集、投影、debug API、Drawer MVP。图 1 证明采集和契约 UI 在工作（`integrity=degraded`、`interrupted`、canonical `LlmRequest` 已能看到 `ApplyPatch` 定义）。

图 1 **不能**当排障主界面，因为：

1. REQUEST 把 `system` / `messages[]` / `tools[]` 揉成一块 JSON，首屏落到某个 tool schema（图 1 的 `ApplyPatch`），看不出「这次交给模型的信封结构」。
2. 没有 Model Call 顶栏（时间、墙钟耗时、token / cache）。
3. 没有 REQUEST → RESPONSE → 工具 的横向摘要卡；工具没有单步耗时。
4. 身份字段默认展开，占掉工作流视口。

图 2 解决的是**同一份观测数据的呈现**，不是另一套采集。插件只消费 harness session log；Flowy 已有自有 JSONL + `project_turns`。本计划只补「投影时间字段 + Drawer 呈现」。

---

## 2. 冻结约束（Review 不得重开，除非指出与现状冲突）

来自已拍板提案，实施时违反即拒收：

1. **方案 B：** 对齐插件信息与呈现；自有 schema。禁止安装 `dsh-plugin-agent-workflow`，禁止把其 TypeScript / Cordis 类型当存储或 API 契约。
2. **Canonical ≠ HTTP wire。** 只展示 `fidelity=canonical` 的 `LlmRequest`。禁止记 / 展 wire body。
3. **禁止用聊天气泡 / SQLite 产品消息补全 `messages[]` 或 system。** 缺字段就写「观测未记录」。图 1 横幅文案必须保留。
4. **显式 `ObservationSession`。** 禁止 async thread-local。禁止重新引入 `model_call_id` 栈 / `ModelCallGuard`。
5. **排序只认 `event_seq`。** `timestamp` / `timestamp_ms` 仅展示与耗时。
6. **禁止无限定词 `Run`。** AgentExecution attempt → `execution_attempt_id`。
7. **开发者模式**同时门控写入与 API 读取。Drawer 在 `system.developerMode !== true` 时不渲染。
8. **观测失败不打断 Agent 回合。**
9. **采集范围本轮不扩大：** one_shot / health probe / speech / ACP 仍跳过。
10. **产品对话 SSOT 仍是 SQLite。** 观测是诊断 SSOT。禁止 replay / fork / 从 log 恢复业务。
11. **最小完整实现。** 不为「以后一等 Tab / OTel / 导出」预留抽象、sink 数组或配置层。

冲突优先级：当前阶段验收 > 正确性与数据安全 > 公共契约 > 项目规则与测试 > 现有惯例 > 最小完整实现。

---

## 3. 本轮范围

### 3.1 做

在现有会话页 Developer Drawer 内，把图 1 的 JSON dump 换成图 2 同构的：

- Model Call **横向卡片流**（REQUEST / RESPONSE / 工具）
- 点卡片后的 **详情面板**（REQUEST = 三列树：系统 | messages | tools）
- Model Call 顶栏：**开始时间、耗时、token 芯片（含 cache）、状态**
- 工具卡：**名称、参数摘要、状态、单工具耗时**
- 保留图 1 已做对的：降级 / 中断 / gap 横幅、身份字段、复制 JSON、developer mode 门控

### 3.2 不做（相对图 2 的刻意缺口）

| 图 2 有 | 本轮 | 原因 |
| --- | --- | --- |
| 会话页一等 Tab（对话 / 轨迹 / 工作流 / 文件 / 终端） | 不做 | 提案 M3 其余 / S9+ |
| 页头「1 用户对话 / 49 模型 / 49 工具 / 5m 27s」 | 不做 | 可后做加总；本刀不改 list 契约 |
| 左栏用户问题预览 + 回合墙钟 | 不做 | list API 已 `strip` request 正文；加 `prompt_preview` 另开一刀 |
| Trace / Raw Events inspector 三栏 | 不做 | S9+ |
| 运行中 live 投影 / load older / 虚拟列表 | 不做 | 无新 WS；继续「刷新」 |
| 独立 Compaction 区 | 不做 | 已有 `call_kind=compaction`，当普通 Model Call |
| 工具卡「schema」列 | 不做 | 未按 call 持久化 advertise schema（REQUEST 三列已有当次 tools[]） |
| 装插件、抄 CSS Module、加 `react-json-view-lite` | 不做 | 用 Arco + UnoCSS + 自写轻量树 |

### 3.3 产品面选择（已拍板，供 Review 确认）

- **挂载：** 仍是 `ChatLayout` → `AgentTraceInspector` Drawer，不是新路由。
- **宽度：** `min(760px, …)` → `min(1080px, calc(100vw - 12px))`。卡片 `flex-wrap`，窄屏折行，禁止整行横向长滚作为主交互。
- **身份字段：** 保留，**默认折叠**（图 1 默认展开占视口）。
- **刷新模型：** 不变。半截 `tool/execution_started` 展示 `started`，文案不得写成 live「运行中」。

---

## 4. 现状（实施前以源码为准，路径会漂移）

### 4.1 采集 / 存储 / API（本轮原则上不改路径）

| 层 | 路径 | 本轮 |
| --- | --- | --- |
| 事件信封 | `crates/agent/nomi-agent-trace/src/event.rs`（含 `timestamp` / `timestamp_ms`） | 只读 |
| JSONL writer | `.../recorder.rs`；目录 `{data_dir}/diagnostics/observation/` | 不改 |
| 投影 | `.../project.rs` → `ProjectedTurn` / `ProjectedModelCall` / `ProjectedToolExecution` | **U0 加时间字段** |
| 采集 | `crates/agent/nomi-agent/src/observation.rs`（`stream_llm`、`llm_request_to_value`） | 不改 |
| Hub | `crates/backend/nomifun-ai-agent/src/agent_trace/hub.rs` | list 仍 strip 正文；**时间字段不得被 strip 掉** |
| HTTP | `GET /api/debug/session-observations`；`GET .../turns/{root_turn_id}` | 路径 / 鉴权不改 |
| 门控 | JWT + 会话归属 + developer mode | 不改 |

`LlmRequest` **没有** `Serialize`。落盘形状由 `llm_request_to_value` 固定：

```text
request.model
request.system
request.messages
request.tools[] { name, description, input_schema, deferred }
request.max_tokens / thinking / reasoning_effort / temperature
```

`llm/response` payload（`emit_response`）：

```text
text, thinking, tool_use[], stop_reason, usage, error, elapsed_ms, ttft_ms
```

`usage` 对齐 `nomi_types::message::TokenUsage`：

```text
input_tokens
output_tokens
cache_creation_tokens
cache_read_tokens
```

工具 payload：`tool_call_id`、`name`、`arguments`（started）；完成/失败/取消另有结果字段。信封时间在 `ObservationEvent`，**当前投影没有带出去**。

### 4.2 前端（本轮主改）

```text
ui/src/renderer/pages/conversation/components/AgentTraceInspector/
  index.tsx                         Drawer + 回合列表
  ObservationWorkflow.tsx           身份 / 横幅 / 整段 JsonBlock（将被拆）
  useAgentTraces.ts                 投影 TS 类型 + fetch
  format.ts                         shortId / formatJson
  AgentTraceInspector.structure.test.ts
```

i18n：`conversation.agentTrace.*`（`zh-CN` + `en-US`）。  
结构测试断言 `min(760px`、`ObservationWorkflow`、`canonicalRequestFromPayload`。

list 响应：`model_calls.length` 与工具个数在，**request/response/tool 正文为 null**。因此图 2 左栏「用户问题预览」本轮做不到，除非另加 list 字段（明确不做）。

---

## 5. 目标信息架构（Drawer 内，不是图 2 整页）

```text
Drawer 1080px
├── 顶栏：conversation_id · N 条 turn · 刷新
├── 回合列表（现有摘要：完整/降级、中断、模型调用数、工具数、msg/turn 短 ID）
└── 选中回合详情 ObservationWorkflow
    ├── 降级/中断/gap 横幅（保留图 1 文案）
    ├── 完整/降级 Tag + session_kind +「复制 JSON」（整份 ProjectedTurn）
    ├── 身份字段 Collapse（默认折叠）
    ├── gaps[]
    └── ModelCallRow × N
        ├── 顶栏：#N · call_kind · scope · 开始时间 · 耗时 · token 芯片 · 状态
        ├── 横向流：RequestCard → ResponseCard → ToolCard*
        └── 若选中该行某张卡：DetailPanel（其下）
```

交互：

- 再点同一张卡：收起详情。
- 点另一张卡：替换详情，不同时展开两块。
- 详情每列：复制；树可折叠；Arco `Modal` 全屏。不引入插件 Modal / `react-json-view-lite`。

---

## 6. 字段合同（插件视觉 → Flowy 数据）

实施与 Review 都按此表验收。缺字段 → `undefined` / 不画芯片 / 「观测未记录」。**禁止补 0 或编造。**

### 6.1 Model Call 顶栏

| 图 2 | Flowy 来源 | 缺失时 |
| --- | --- | --- |
| Model Call #N | 详情里 `model_calls` 下标 + 1 | — |
| 开始时间 `20:03:34` | U0：`ProjectedModelCall.started_at_ms`（该 call 的 `llm/request` 信封 `timestamp_ms`） | `—` |
| 墙钟 4.7s | `response.elapsed_ms` | interrupted 或无 response → `—` |
| 输入合计 | `input_tokens + cache_read_tokens + cache_creation_tokens` | 无 `usage` → 整组 token 不画 |
| 未命中缓存 | `input_tokens` | 同上 |
| 缓存命中 | `cache_read_tokens`，**>0 才显示** | 隐藏 |
| 缓存写入 | `cache_creation_tokens`，**>0 才显示** | 隐藏 |
| 输出 | `output_tokens` | 无 usage → 不画 |
| 完成勾 | 有 response 且工具均终态 | — |
| 中断 | `call.interrupted` | 图 1 已有橙 Tag |

状态优先级：`interrupted` > 任一门工具 `failed` > 全部终态且有 response → 完成 > 仅 `started` → `started`（不是 live running）。

### 6.2 REQUEST 卡

| 图 2 | Flowy |
| --- | --- |
| 预览模型名 | `canonicalRequestFromPayload(call.request).model` |
| 系统 N | `system` trim 后非空 → 1，否则 0 |
| 消息 N | `messages.length`（`messages` 非数组则不当 0，计数缺失） |
| 工具定义 N | `tools.length` |

卡片上**禁止**展开某个 `input_schema`（这正是图 1 的问题）。

### 6.3 点 REQUEST → 三列

| 列 | 数据 | 禁止 |
| --- | --- | --- |
| 系统提示词 | `request.system` | 用欢迎语 / 气泡 / 产品消息顶替 |
| 消息 | `request.messages` | 从 SQLite / WS transcript 重拼 |
| 工具定义 | `request.tools` | 臆造未写入观测的 tool |

`omitted_reason`、`…(truncated)`、redact 结果原样展示。

### 6.4 RESPONSE 卡与详情

| 图 2 | Flowy |
| --- | --- |
| 预览 | `text` 首行；空且有 `tool_use` →「仅工具调用」；`interrupted` → 现有 `noResponse` |
| 推理 / 正文 / 工具调用 | `thinking` 非空 0\|1；`text` 非空 0\|1；`tool_use.length` |
| 点开 | thinking \| text \| metadata（`usage` / `elapsed_ms` / `ttft_ms` / `stop_reason` / `error`） |

### 6.5 工具卡与详情

| 图 2 | Flowy |
| --- | --- |
| 名称 | `ProjectedToolExecution.name` |
| 参数摘要 | `started.arguments` 单行截断 |
| 状态条 | cancelled > failed > completed > started |
| 耗时 | U0：`ended_at_ms - started_at_ms`；缺一端 → `—` |
| 点开 | 参数 \| 结果（completed/failed/cancelled payload）\| `tool_call_id` |

本轮无 schema 第三列。

### 6.6 回合列表（刻意保持图 1）

继续：完整/降级、中断、`session_kind`、模型调用数、工具数、`msg` / `turn` 短 ID。  
不加用户 prompt 预览。

---

## 7. 后端：U0 投影增量（唯一允许的 Rust 行为变化）

**目的：** 信封时间已在 JSONL，投影丢掉了，前端无法画图 2 的时间和工具耗时。  
**不是**新事件、不是升 `schema_version`、不改 `stream_llm`。

### 7.1 类型

`crates/agent/nomi-agent-trace/src/project.rs`：

```text
ProjectedModelCall
  + started_at_ms: Option<u64>   // 该 model_call 的 llm/request.event.timestamp_ms

ProjectedToolExecution
  + started_at_ms: Option<u64>   // tool/execution_started
  + ended_at_ms: Option<u64>     // completed / failed / cancelled 的事件时间
```

Serde：`skip_serializing_if = "Option::is_none"`，与现有字段一致。  
前端 `useAgentTraces.ts` 同步可选字段。

### 7.2 赋值规则

- 只在对应 `event_type` 分支写入；禁止用「邻近事件」猜时间。
- 同一 `model_call_id` 多次 `llm/request`（不应发生）：保留**第一条** `started_at_ms`。
- 工具 `ended_at_ms`：取终态事件时间；多个终态（异常）保留**第一个终态**。
- **禁止**用 `timestamp` 重排 `model_calls` / tools。顺序仍是投影遍历的 `event_seq`。

### 7.3 strip

`strip_projected_turn_payloads` 继续清空 request/response/tool **正文**。  
**必须保留** `started_at_ms` / `ended_at_ms`。本轮 list UI 可以先不用这些字段，但不得在 strip 时抹掉，避免下一刀 list 时间再改投影。

### 7.4 不改

- `ObservationEvent` 形状、JSONL 目录、rotate / GC
- HTTP 路径、鉴权、developer mode
- `nomifun-conversation` 路由注册
- 采集 call site

若 Review 认为「前端从详情 payload 自己算时间就够、不必改 Rust」：**否决。** 详情 API 返回的是 payload，不含信封 `timestamp_ms`；工具 payload 也没有 started/ended。要时间就必须投影带出，或改采集往 payload 里写时间（更差，污染事件合同）。

---

## 8. 前端：文件与职责

全部落在现有 `AgentTraceInspector/`，不新开路由、不新 crate。

| 文件 | 动作 | 职责 |
| --- | --- | --- |
| `workflowViewModel.ts` | **新建** | 纯函数：拆 canonical request、计数、usage 芯片、预览、工具状态/耗时。缺字段 `undefined` |
| `workflowViewModel.test.ts` | **新建** | 上表合同的单元测试 |
| `ModelCallFlow.tsx` | **新建** | 一行调用：顶栏 + 卡片流 + 选中态 |
| `WorkflowDetailPanel.tsx` | **新建** | REQUEST 三列 / RESPONSE 三列 / 工具两列+id |
| `ObservationJsonTree.tsx` | **新建** | 轻量可折叠树 + 复制；全屏走 Arco Modal |
| `ObservationWorkflow.tsx` | **改** | 编排横幅 / 身份 / gaps / `ModelCallFlow`；删除主路径整段 `JsonBlock` |
| `index.tsx` | **改** | Drawer 宽度 1080；结构测试同步 |
| `useAgentTraces.ts` | **改** | 时间可选字段；`canonicalRequestFromPayload` 语义不变 |
| `format.ts` | **可小改** | `formatDuration(ms)` / `formatClock(ms)`；locale 用 `undefined`（本机） |
| `*.structure.test.ts` | **改** | 见 §10 |
| `zh-CN/en-US conversation.json` | **改** | 新 key，旧 key 能复用则复用 |

**禁止：**

- 从 `dsh-plugin-agent-workflow` 复制组件 / CSS Module / 类型
- 新增 npm 依赖（含 `react-json-view-lite`）
- 在 renderer 直连 Tauri
- 把聊天 store / `AgentStreamEvent` 当 REQUEST 数据源

配色：主题变量（`--color-primary` / `--color-warning` / `--color-success` 等）。REQUEST / RESPONSE / 工具用左边框或浅底区分即可，不建独立色盘。

---

## 9. 分步执行（一次一步，验收后再下一步）

### U0 — 投影时间字段

**改：** `project.rs` + 现有 / 新增投影测试；`useAgentTraces.ts` 类型可在 U1 再对，但 Rust 必须先绿。

**验收：**

```text
cargo test -p nomi-agent-trace
```

至少覆盖：

1. `llm/request` 的 `timestamp_ms` → `started_at_ms`
2. 工具 started + completed → 两端时间；duration 由 UI 减
3. 只有 started → `ended_at_ms` 为空
4. `strip_projected_turn_payloads` 后时间仍在、正文为空
5. 排序仍按 `event_seq`（已有测试不得坏）

**停。** 不改 UI。

### U1 — View model 纯函数

**改：** `workflowViewModel.ts` + `workflowViewModel.test.ts`（及 `format` 时长函数若放这里）。

**验收：** bun 测该文件。用例：

1. 拆出 `system` / `messages` / `tools` / `model`；`canonicalRequestFromPayload` 包一层 `request` 时不丢层
2. `messages` 缺失 → 计数 `undefined`，不当 0
3. usage 芯片公式与 §6.1 一致；全 0 的 cache 芯片不出现
4. 无 usage → `usage` 为 `undefined`
5. interrupted → 耗时 `undefined`，不把 0 当 0ms
6. 工具 duration 两端齐全才有数
7. **不**读取任何「chat / transcript / bubble」字段名

**停。** 不改 Drawer JSX。

### U2 — 卡片流 + 三列详情 + Drawer 宽度

**改：** §8 所列 tsx、i18n、`ObservationWorkflow`、`index.tsx`、结构测试。

**UI 验收（结构测试 + 目视）：**

- 主路径不再把完整 `tools[]` schema 当作 REQUEST 首屏
- 存在 RequestCard / ResponseCard / ToolCard 选择面（可用 `data-` 或 i18n key 断言）
- REQUEST 详情三列 key：`system` / `messages` / `tools`（i18n）
- interrupted 仍显示 `noResponse`，不编造 response
- 横幅 `gapBanner` 仍在
- `canonicalRequestFromPayload` 仍被调用
- Drawer 宽度断言改为 `min(1080px`
- 身份 Collapse **无** `defaultActiveKey={['ids']}`（或等价默认折叠）

i18n：`zh-CN` + `en-US` 成对。建议新 key（名称可微调，但语义固定）：

```text
conversation.agentTrace.modelCall          模型调用
conversation.agentTrace.systemPrompt       系统提示词
conversation.agentTrace.messages           消息
conversation.agentTrace.toolDefinitions    工具定义
conversation.agentTrace.requestDetails     请求详情
conversation.agentTrace.responseDetails    响应详情
conversation.agentTrace.toolDetails        工具详情
conversation.agentTrace.notRecorded        观测未记录
conversation.agentTrace.toolOnly           仅工具调用
conversation.agentTrace.reasoning          推理
conversation.agentTrace.content            正文
conversation.agentTrace.inputTokens        输入
conversation.agentTrace.inputUncached      未命中缓存
conversation.agentTrace.cacheRead          缓存命中
conversation.agentTrace.cacheWrite         缓存写入
conversation.agentTrace.outputTokens       输出
conversation.agentTrace.expandJson         全屏
conversation.agentTrace.closeDetails       收起详情
```

能复用的不要新造：`request` / `response` / `tools` / `interrupted` / `noResponse` / `copy` / `copied`。

**停。** 不改采集、不加 Tab。

### U3 — 门禁

```text
cargo test -p nomi-agent-trace
bun run typecheck
bun run check:i18n
```

结构测试必须覆盖 U2 断言。能跑则再跑 `bun run check`（图标 / theme 无改时应无感）。

`cargo test -p nomifun-ai-agent` **不是本刀最低门禁**（HTTP 形状只多了可选时间字段）。若碰了 Hub strip，补一条 Hub 单测：strip 后时间仍在。

---

## 10. 测试与回归面

| 测试 | 断言 |
| --- | --- |
| `nomi-agent-trace` 投影 | §9 U0 |
| `workflowViewModel.test.ts` | §9 U1 |
| `AgentTraceInspector.structure.test.ts` | 开发者模式门控、新 API 路径、宽度 1080、卡片/三列、禁止 `agent-traces`、禁止从 chat 拼 messages |
| i18n check | 新 key 双语 |
| 目视（实施者） | 用图 1 同类 interrupted 回合：首屏是模型名+计数，不是 `ApplyPatch` schema；点开三列能看到 system / messages / tools |

不要求：Playwright e2e、完整 `nomifun-ai-agent` Windows 套件、支持包 ZIP e2e。

---

## 11. 给 Review Agent 的清单

请按缺陷优先写回。下列为**开放审查点**，不是实施时再发明需求。

### 11.1 必须挡下的错误

- [ ] 用聊天气泡 / 会话 store 补 REQUEST
- [ ] 新依赖、复制插件源码、一等 Tab
- [ ] 改 `LlmProvider` / 采集范围 / JSONL schema_version
- [ ] 用 `timestamp` 排序重建工作流
- [ ] strip 掉时间字段，或往 payload 里塞时间代替投影字段
- [ ] 无 usage 时画 0 token 冒充已采集
- [ ] interrupted 仍渲染假 RESPONSE 卡内容（卡可以在，正文必须是 `noResponse`）
- [ ] 为 M4 / 一等 Tab 预留 Sink 接口或配置

### 11.2 请 Review 明确表态的设计点

1. **Drawer 1080 vs 保持 760 + 仅折行。** 计划取 1080。若认为盖住聊天不可接受，给替代宽度，不要既要三列又锁 760。
2. **U0 改投影 vs 另开 raw events API。** 计划只加三个 `Option<u64>`。反对请给出同等信息、更小的读路径。
3. **list 不做 prompt 预览。** 与图 2 左栏差一截。是否接受本刀此缺口。
4. **半截工具叫 `started` 不叫 running。** 避免和插件 live 语义混淆。
5. **身份默认折叠。** 排障是否仍要默认展开（图 1）。计划折叠。

### 11.3 非目标确认

请确认第 3.2 节缺口（一等 Tab、会话汇总、live、compaction 专区）**不要**在本刀 review 里改成范围。

---

## 12. 风险与残留差距

| 风险 | 处理 |
| --- | --- |
| 1080 Drawer 挡聊天 | 折行 + 可关抽屉；不改成一等页 |
| 旧 JSONL 无时间？ | 信封从第一天就有 `timestamp_ms`；缺则 `—` |
| list 无预览 | 文档化缺口；禁止从聊天补 |
| 工具耗时依赖 U0 | U0 不做则工具卡只能 `—`，相对图 2 最假 |
| 刷新非 live | 文案与状态词约束 |
| 结构测试改宽度 | 同步改 `min(1080px` 断言 |

相对图 2，本轮结束后**仍没有**：一等 Workflow 路由、会话级 49/49/5m27s、左栏 prompt 标题、Raw Events、compaction 专区、live running。这些不是本刀失败条件。

---

## 13. 实施原则（授权后）

1. 先读调用链：`project.rs`、`ObservationWorkflow.tsx`、`useAgentTraces.ts`、Hub strip、结构测试。
2. 只改当前 U 步列出的文件。
3. 不预留 ObservationSink[]、不抽「通用 workflow SDK」。
4. 发现无关问题只记在 PR / review 回复，不顺手改。
5. 提交信息 Conventional Commits；Git 作者必须是人类。禁止 AI 作为 author / committer / Co-authored-by。
6. 不 `--no-verify`。

建议提交切分（授权实施后再提交，本文件不要求现在 commit）：

```text
feat(agent): project observation event timestamps for workflow cards
feat(ui): render session observation model-call cards and request columns
```

或 squash 成一条 UI+投影，只要 Review 不要求拆 PR。

---

## 14. 授权

- 本文 + 已冻结提案 = **U0–U3 的审查与实施依据**。
- **现在不授权改实现代码。**
- Review 通过且产品负责人授权后，从 **U0** 开始，一步一验收。
- 不得把本文写成第三套运行时契约。合并后只需在 [agent-observability-and-eval.zh.md](agent-observability-and-eval.zh.md) 的 UI 小节补一句「Drawer 为卡片流 + 三列详情」。

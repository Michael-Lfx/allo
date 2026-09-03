# Agent 可观测性与评测

> **最后维护：** 2026-08-24 · 核对基准：commit `d791691c6` ·
> 文档性质：现行行为文档（`nomi-agent-trace` 采集/回收语义、
> `nomi-agent-eval` 套件与 HTTP 面已抽查核对）。

本文说明 Flowy / allo 中 **Session Observation（开发者模式）** 与 **Agent Eval（真实 harness / runtime）** 的职责边界与用法。契约细节见 [session-observation-workflow-proposal.zh.md](session-observation-workflow-proposal.zh.md) 第 7 节。

## Session Observation（Developer Mode）

Nomi-owned 模型调用与工具执行写入 unlabeled JSONL 事件，落盘于：

`{data_dir}/diagnostics/observation/{conversation_id}/events.jsonl`

超过 48 MiB 旋转为 `events.{n}.jsonl`；磁盘只按约 1 GiB / 800 MiB 高低水位回收（紧急 1.2 GiB），**没有按天 TTL**。写入前执行 capture（truncated + redacted；媒体 metadata_only）。会话发送即写盘；`system.developerMode` 不控制采集。

读取与支持包附带 JSONL 受 **开发者模式** 门控：

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/api/debug/session-observations?conversation_id=` | 按会话投影回合列表（摘要 + 顶层 `recorder_health`，不含 request/response 全文；默认 50 条、最多 200 条） |
| GET | `/api/debug/session-observations/turns/{root_turn_id}?conversation_id=` | 单个回合的投影详情：按 `event_seq` 排序的轻量 timeline、调用 headers 与请求展示元数据（无 canonical 正文） |
| GET | `/api/debug/session-observations/turns/{root_turn_id}/calls/{model_call_id}?conversation_id=` | 点瓦片才拉的单次 call 正文 |
| GET | `/api/debug/session-observations/turns/{root_turn_id}/export?conversation_id=` | 下载该用户提示词回合当前保留范围内的完整 JSON 事件文档 |

实现：`nomi-agent-trace`（事件 / capture / DualQueue writer / 投影）→ `nomifun-ai-agent::AgentTraceHub` → `nomifun-conversation::routes_trace`。采集走 `ObservationSession` + `stream_llm`，失败只 warn / `observation/gap`，不打断回合。Delete/Clear/Reset/Shutdown 走 writer ACK；Clear 用 generation bump，Delete 才永久 tombstone。

HTTP 路由只输出 `nomifun-api-types` 中的 Session Observation DTO；Agent 层的投影结构不直接成为 HTTP 类型。投影缺少 `turn/start` 时，fallback 摘要同样跳过 user 消息首个 `[Context]` 文本块，继续选择真实用户文本。普通事件队列溢出会生成 gap；控制事件使用独立有界队列，满时先淘汰一条普通事件，仍无容量则记录控制事件丢失并生成 gap，避免无界内存增长。

投影规则：只按 `event_seq` 排序。`status` 是 Agent 做了什么，`integrity` 是日志缺不缺。工具失败且日志完整 → `status=failed` 且 `integrity=complete`。`integrity=degraded` 仅当：`observation/gap`、JSONL 损坏、或该 turn 已 `turn/end` 后仍缺 `llm/response` / 工具终态。进行中的 call 无 response 标 `interrupted`，不因此把整回合标 degraded。单回合详情额外返回不含正文的 timeline：模型响应明确区分工具请求与最终回答，工具生命周期位于响应和下一次模型请求之间；每项带 `event_seq`、相对回合时间和可计算的 duration。请求投影提供公共历史前缀折叠信息及系统提示首次/沿用/变化/不可比较状态。禁止用聊天气泡拼 `messages[]`。Summary 带 `coverage`（当前保留窗口，不是全历史）。

### UI

会话页 `ChatLayout` 在开发者模式开启且存在 `conversation_id` 时，在能力按钮最左显示 **观测** pill（`aria-pressed`）。打开后对话列滑动到会话日志 pane（`AgentTraceInspector`），不是 Drawer：

- 左列顶：会话四数 + 刷新 + 最新在上|最早在上；写入器 health 与会话 integrity / coverage 次级
- 回合行带时钟；第 N 轮按时间升序编号
- 右侧是固定的“回合列表 → 时间线导航栏 → 当前事件详情”工作区：按 `event_seq` 展示紧凑时间线，再按模型调用提供可定位的请求/响应/实际工具详情；模型响应明确标记「请求调用工具」或「最终回答」，不再用 `请求 → 响应 → 工具` 表达时序。工具开始与终态在时间线中视觉合并，详情仍可追溯阶段。
- 请求 `messages` 默认显示当前请求尾部，历史公共前缀收起在顶部；无法可靠识别时回退完整上下文。系统提示首次默认展开，未变化时收起并标记沿用，变化或不可比较时显式提示；工具定义默认收起，实际使用工具优先
- 当前回合标题栏提供单回合 JSON 下载；折叠只影响 UI，导出仍包含所有保留的原始事件、辅助调用、工具生命周期与 gap。进行中回合可下载当前已写盘内容，文件状态标记为 `running` / `degraded`。点击后由桌面原生保存对话框或浏览器文件保存选择器让用户选择位置并写入文件，不使用隐式的浏览器默认下载目录
- 切回对话不 abort poll、不清 LRU；用户离开详情底部时，新事件提示不强制滚动

请求消息的「原始」与「摘要」是两种不同的展示投影：

- 「原始」展示观测记录中的完整 `messages[]` 对象树；`[Context]`、`timestamp` 等原始字段保留，复制内容仍使用 canonical JSON。
- 「摘要」按一条 `Message` 保留一行。`[Context]` 是 Agent turn-tail 注入的保留前缀，仅识别 `user` 消息的第一个文本块；消息包含真实文本和该 Context 文本块时，真实文本作为主预览。Context 不得替换真实消息。例如 `[Context] ...` + `66` 摘要显示为 `用户 66`，后续轮次的 `11` 也应显示为 `用户 11`，历史行仍保留 `66`。
- Context 只作为悬浮诊断提示显示：`上下文 · Current date: 2026-08-21`。未悬浮时不显示 `[Context]`、`Current date`、「上下文」标签或额外的第二行。Context-only 消息仍保留，但不额外生成可见摘要行。

以上规则属于前端 Trace 展示投影，不改变观测 JSON、`Message` 数据结构、会话持久化、Provider 请求序列化或 KV cache 行为。

### Trace 三栏工作区补充（现行 UI 行为）

当前 Trace 工作区内部保持“回合列表 → 时间线导航栏 → 详情工作区”的三栏结构。宽屏时间线约 288px，收起后切换为固定约 88px 的紧凑轨道，只显示圆形事件图标和相对时间，不渲染会被截断的标题、序号或省略号；完整语义通过展开状态、无障碍名称和左侧图标说明查看。间隔 0s 不单独占行，工具开始与终态可做视觉合并但不改变原始事件；时间线图标含义通过左侧会话统计区的信息入口查看。899px 以下时间线移动到详情顶部，收起态改为横向事件条，避免圆形图标和轮次信息被挤压。

右侧详情不再提供重复的“回合全览”按钮，时间线展开状态本身就是当前回合全览。模型调用列表不再额外渲染重复的分组标题；模型调用信息卡片与当前详情处于同一滚动流中；点击请求、响应或工具阶段后，详情直接插入对应模型调用卡片下方，并在标题中标明模型调用编号和阶段。响应详情只保留一个 inspector，不再重复渲染额外的“最终回复”卡片。系统提示使用一个带折叠控制和操作按钮的内容栏，不再套一层重复的系统提示标题。回合开始、结束和观测缺口没有模型调用归属时，使用独立的固定高度事件详情槽。详情槽内部滚动，展开系统提示、历史消息或工具定义不会改变工作区外部高度。选中状态同步反映在时间线、模型调用卡片和阶段按钮上，轮询不打断用户当前阅读位置。完整的空间、滚动、键盘、Reduced Motion、双主题和验收契约见 [session-observation-workflow-ui-plan.zh.md](session-observation-workflow-ui-plan.zh.md) §7.2。

未开启开发者模式时组件不渲染；API 在未开启时返回 403。

单回合导出只覆盖当前 retention 窗口，使用已 capture 的事件，因此继续遵循 128 KiB 单事件上限、redaction、截断和媒体 metadata-only；它不是未脱敏 provider wire body，也不改变支持包的最近文件数/4 MiB 诊断包限制。支持包在开发者模式开启且指定 `conversation_id` 时，把 `diagnostics/observation/` 下的 JSONL 打进 ZIP 的 `observation/`。

## Eval（真实 AgentEngine）

评测对象是钉死模型后的 **harness / runtime**，不是模型排行榜。本体对齐 Inspect 的 `Task = Dataset + Solver + Scorer`，但不引入 Python Inspect。

| 层 | crate | 职责 |
| --- | --- | --- |
| 语料 / scorer / JSONL | `nomi-agent-eval` | 确定性 oracle、resume、脱敏、数据集下载 |
| Live solver | `nomifun-ai-agent::agent_eval` | `LiveNomiHarness` 走 `AgentBootstrap`（Office profile 或按需 `CodingHarness`） |
| Session 绑定 | `EvalSessionBridge` + `ObservationSession` | 每条 case 一个 `conversation_id`；写 Observation；投影思考 / 工具 / 回复 + usage |
| HTTP | `/api/debug/agent-evals/*` | 开发者模式；同时只允许一轮 |
| UI | `/eval` | 侧栏 `dev` 徽章；仅 `system.developerMode === true` 可见；可跳转到会话观测 |

### 隔离（不得影响真实用户 Agent）

- 工作区：开跑时创建业务命名父目录 `{data_dir}/diagnostics/agent-evals/workspaces/评测-{套件业务名}-{时间戳}-{run短ID}/`，case 在其子目录 `{case_id}/`
- `session.enabled = false`（不写 nomi session 文件）；观测通过显式 `ObservationSession` 写入
- **不**注册 `AgentRuntimeRegistry`
- 会话壳：`{case_id} · {category}`，`extra.origin=eval` / `extra.eval=true`，幂等键 `eval:{run_id}:{case_id}`；`extra.workspace` 绑定**父 run 工作区**（使 SessionList 出现独立业务工作区，而非「默认工作空间」）；轨迹投影为 thinking / tool_call / text，并写入 `last_token_usage`；`execute_turn` 包在 `with_flowy_billing_turn_id` 下以便积分芯片
- Agent 执行 cwd / `write_root` 仍为 case 子目录；`convert.rs` 对 `eval` 会话按 companion 同类规则不标 `is_temporary_workspace`
- `auto_approve = true`，`write_root` = eval workspace
- 默认关闭 MCP、browser、computer-use、web search、memory distill、MoA、embedded AgentExecution
- 证据 JSONL 不含 workspace 绝对路径；prompt 经 `nomi-redact` 脱敏
- 完整 trajectory / artifact 不进 JSONL，落在 `{data_dir}/diagnostics/agent-evals/runs/{run_id}/traces/{case_id}.json`

### 套件

评测对象是 **harness / runtime**，不是刷题排行榜。已移除 HumanEval / MBPP / 简单 marker Q&A 作为 live KPI。

| Suite | 来源 |
| --- | --- |
| `office_tasks` | 捆绑办公语料（备忘录、纪要、CSV 预算、客户邮件、原地改稿；Office profile，**不是** CodingHarness） |
| `agent_workflows` | 捆绑多步 agent 语料（多源简报、修测、CSV→JSON、重构+文档、约束编辑） |
| `aider_polyglot` | [Aider polyglot](https://github.com/Aider-AI/polyglot-benchmark) Python（主 coding-agent 套件：读说明、改 stub、跑测试。去掉 `.meta/example.py`。非官方 Aider 分数） |
| `classeval` | [ClassEval](https://github.com/FudanSELab/ClassEval)（类级 skeleton + 隐藏 unittest） |
| `harness_control` | 捆绑 Write/Edit 冒烟 |

SWE-bench / Terminal-Bench / GAIA / τ-bench / OSWorld 需要 Docker 或评测隔离默认关闭的工具面，**不得**在无沙箱时宣称官方分数。

远程集合默认 limit 8、最大 20，缓存于 `{data_dir}/diagnostics/agent-evals/datasets/`。

### API（均需登录 + 开发者模式）

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/api/debug/agent-evals/suites` | 套件目录与缓存状态 |
| POST | `/api/debug/agent-evals/datasets/{suite}/pull?limit=` | 下载并缓存 |
| POST | `/api/debug/agent-evals/runs` | 启动 live 评测（每条 case 绑定 session） |
| GET | `/api/debug/agent-evals/runs` | 最近一轮（含进行中） |
| GET | `/api/debug/agent-evals/runs/{id}` | 单轮快照（进行中含 `current_trace` / `current_conversation_id`） |
| GET | `/api/debug/agent-evals/runs/{id}/cases/{case_id}/trace` | 该用例完整 trajectory + 工作区产物（相对路径、脱敏） |
| GET | `/api/debug/agent-evals/runs/{id}/cases/{case_id}/observation` | 与真实会话相同的 Session Observation 投影 |
| POST | `/api/debug/agent-evals/runs/{id}/cancel` | 在 **case 边界** 取消 |

取消当前正在跑的 case 会等到该 case 结束或超时。进行中再开一轮返回 409。

### CLI（离线 / 拉数；不含 live engine）

```bash
cargo test -p nomi-agent-eval --all-targets

cargo run -p nomi-agent-eval --example agent_eval --features agent-eval -- \
  demo --output /tmp/agent-eval-demo.jsonl

cargo run -p nomi-agent-eval --example agent_eval --features agent-eval -- \
  pull --suite aider_polyglot --cache-dir /tmp/agent-eval-cache --limit 8

cargo run -p nomi-agent-eval --example agent_eval --features agent-eval -- \
  summarize --input /tmp/agent-eval-demo.jsonl --output /tmp/agent-eval-summary.json
```

`OfflineDemoHarness` 的 100% 通过是自指脚本，**不能**当作 live agent KPI。

操作手册见 [`evaluation/README.md`](../../crates/agent/nomi-agent-eval/evaluation/README.md)。

## 关系

| 能力 | 面向 | 是否依赖 LLM |
| --- | --- | --- |
| Session Observation | 开发者调试真实回合的 canonical 请求 / 响应 / 工具 | 否（只读已落盘 JSONL） |
| Agent Eval | 回归 / 准入真实 harness；每 case 复用 Observation 管道 | 离线 demo 否；live lab 是 |

二者互补且在 live lab 上已打通：Eval 保证行为契约，Observation 解释每一次 case 回合（`session_kind=eval`）。

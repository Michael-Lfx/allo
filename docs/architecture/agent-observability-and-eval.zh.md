# Agent 可观测性与评测

本文说明 Flowy / allo 中 **Session Observation（开发者模式）** 与 **Agent Eval（真实 harness / runtime）** 的职责边界与用法。契约细节见 [session-observation-workflow-proposal.zh.md](session-observation-workflow-proposal.zh.md) 第 7 节。

## Session Observation（Developer Mode）

Nomi-owned 模型调用与工具执行写入 unlabeled JSONL 事件，落盘于：

`{data_dir}/diagnostics/observation/{conversation_id}/events.jsonl`

超过 48 MiB 旋转为 `events.{n}.jsonl`；磁盘只按约 1 GiB / 800 MiB 高低水位回收（紧急 1.2 GiB），**没有按天 TTL**。写入前执行 capture（truncated + redacted；媒体 metadata_only）。会话发送即写盘；`system.developerMode` 不控制采集。

读取与支持包附带 JSONL 受 **开发者模式** 门控：

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/api/debug/session-observations?conversation_id=` | 按会话投影回合列表（摘要 + 顶层 `recorder_health`，不含 request/response 全文；默认 50 条、最多 200 条） |
| GET | `/api/debug/session-observations/turns/{root_turn_id}?conversation_id=` | 单个回合的 REQUEST → RESPONSE → tools **headers**（无 canonical 正文） |
| GET | `/api/debug/session-observations/turns/{root_turn_id}/calls/{model_call_id}?conversation_id=` | 点瓦片才拉的单次 call 正文 |

实现：`nomi-agent-trace`（事件 / capture / DualQueue writer / 投影）→ `nomifun-ai-agent::AgentTraceHub` → `nomifun-conversation::routes_trace`。采集走 `ObservationSession` + `stream_llm`，失败只 warn / `observation/gap`，不打断回合。Delete/Clear/Reset/Shutdown 走 writer ACK；Clear 用 generation bump，Delete 才永久 tombstone。

投影规则：只按 `event_seq` 排序。`status` 是 Agent 做了什么，`integrity` 是日志缺不缺。工具失败且日志完整 → `status=failed` 且 `integrity=complete`。`integrity=degraded` 仅当：`observation/gap`、JSONL 损坏、或该 turn 已 `turn/end` 后仍缺 `llm/response` / 工具终态。进行中的 call 无 response 标 `interrupted`，不因此把整回合标 degraded。禁止用聊天气泡拼 `messages[]`。Summary 带 `coverage`（当前保留窗口，不是全历史）。

### UI

会话页 `ChatLayout` 在开发者模式开启且存在 `conversation_id` 时，在能力按钮最左显示 **观测** pill（`aria-pressed`）。打开后对话列滑动到会话日志 pane（`AgentTraceInspector`），不是 Drawer：

- 左列顶：会话四数 + 刷新 + 最新在上|最早在上；写入器 health 与会话 integrity / coverage 次级
- 回合行带时钟；第 N 轮按时间升序编号
- 右侧按模型调用展示 REQUEST → RESPONSE → tools；点瓦片才 Call GET
- 请求 `messages` / `tools` 默认扫描列表（消息最新在上，「原始」才是 `react-json-view-lite`）；系统提示、响应、工具执行仍是文本/对象树；切回对话不 abort poll、不清 LRU

未开启开发者模式时组件不渲染；API 在未开启时返回 403。

支持包在开发者模式开启且指定 `conversation_id` 时，把 `diagnostics/observation/` 下的 JSONL 打进 ZIP 的 `observation/`。

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

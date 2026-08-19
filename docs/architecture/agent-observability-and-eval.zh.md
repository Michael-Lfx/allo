# Agent 可观测性与评测

本文说明 Flowy / allo 中 **Developer Mode Trace** 与 **Agent Eval（真实 harness / runtime）** 的职责边界与用法。

## Trace（Developer Mode）

Agent 回合会写入结构化 turn trace（spans、token、工具计数），落盘于：

`{data_dir}/diagnostics/agent-traces/`

读取受 **开发者模式**（`system.developerMode`）门控：

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/api/debug/agent-traces?conversationId=&limit=` | 按会话列出索引 |
| GET | `/api/debug/agent-traces/recent?limit=` | 最近条目 |
| GET | `/api/debug/agent-traces/artifacts?conversationId=&limit=` | 会话级已验证产物元数据（无二进制） |
| GET | `/api/debug/agent-traces/{trace_id}` | 完整 turn（含 spans） |

实现：`nomi-agent-trace`（存储 / 脱敏）→ `nomifun_ai_agent::AgentTraceHub` → `nomifun-conversation::routes_trace`。

工具完成后，collector 会写入两类产物元数据（不写绝对路径 / 二进制）：

1. **receipt**：已验证的 `PersistedArtifact`（媒体/导出类工具）
2. **reported**：`Write` / `Edit` / `ApplyPatch` 等文件变更工具的 `file_path`（以及工具输出里的 `Created/Updated/Edited …` 回填）

会话级列表 API 也会对历史 turn 做 span preview 回填，因此开发者模式开启后产生的 Write/Edit 脚本与文档会出现在 Session artifacts 中。

### UI

会话页 `ChatLayout` 在开发者模式开启且存在 `conversation_id` 时显示 **Trace** 按钮，打开嵌入式 Drawer（`AgentTraceInspector`）：

- 会话产物列表（跨 turn 聚合，点击跳到对应 turn）
- 列出该会话的 turn 索引（含 artifact 计数）
- 点击行查看 span 瀑布、tokens / tools / artifacts、可展开 attributes / preview

未开启开发者模式时组件不渲染；API 在未开启时返回 403。

## Eval（真实 AgentEngine）

评测对象是钉死模型后的 **harness / runtime**，不是模型排行榜。本体对齐 Inspect 的 `Task = Dataset + Solver + Scorer`，但不引入 Python Inspect。

| 层 | crate | 职责 |
| --- | --- | --- |
| 语料 / scorer / JSONL | `nomi-agent-eval` | 确定性 oracle、resume、脱敏、数据集下载 |
| Live solver | `nomifun-ai-agent::agent_eval` | `LiveNomiHarness` 走 `AgentBootstrap`（Office profile 或按需 `CodingHarness`） |
| HTTP | `/api/debug/agent-evals/*` | 开发者模式；同时只允许一轮 |
| UI | `/eval` | 侧栏 `dev` 徽章；仅 `system.developerMode === true` 可见 |

### 隔离（不得影响真实用户 Agent）

- 工作区：`{data_dir}/diagnostics/agent-evals/workspaces/{run_id}/{case_id}/`
- `session.enabled = false`，不写用户会话表
- **不**注册 `AgentRuntimeRegistry`
- `auto_approve = true`，`write_root` = eval workspace
- 默认关闭 MCP、browser、computer-use、web search、memory distill、MoA、embedded AgentExecution
- 证据 JSONL 不含 workspace 绝对路径；prompt 经 `nomi-redact` 脱敏
- 完整 trajectory / artifact 不进 JSONL，落在 `{data_dir}/diagnostics/agent-evals/runs/{run_id}/traces/{case_id}.json`

### 套件

评测对象是 **harness / runtime**，不是刷题排行榜。HumanEval / MBPP 只作函数级 floor，**不能**当作 agent KPI。

| Suite | 来源 |
| --- | --- |
| `office_tasks` | 捆绑办公语料（备忘录、纪要、CSV 预算、客户邮件、原地改稿；Office profile，**不是** CodingHarness） |
| `aider_polyglot` | [Aider polyglot](https://github.com/Aider-AI/polyglot-benchmark) Python（主 coding-agent 套件：读说明、改 stub、跑测试。去掉 `.meta/example.py`。非官方 Aider 分数） |
| `classeval` | [ClassEval](https://github.com/FudanSELab/ClassEval)（类级 skeleton + 隐藏 unittest） |
| `session_dialogue` | 捆绑语料（对话契约） |
| `harness_control` | 捆绑 Write/Edit 冒烟 |
| `humaneval` | OpenAI HumanEval（函数级 floor，非 agent eval） |
| `mbpp` | Google sanitized MBPP（同上） |

SWE-bench / Terminal-Bench / GAIA / τ-bench / OSWorld 需要 Docker 或评测隔离默认关闭的工具面，**不得**在无沙箱时宣称官方分数。

远程集合默认 limit 8、最大 20，缓存于 `{data_dir}/diagnostics/agent-evals/datasets/`。

### API（均需登录 + 开发者模式）

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/api/debug/agent-evals/suites` | 套件目录与缓存状态 |
| POST | `/api/debug/agent-evals/datasets/{suite}/pull?limit=` | 下载并缓存 |
| POST | `/api/debug/agent-evals/runs` | 启动 live 评测 |
| GET | `/api/debug/agent-evals/runs` | 最近一轮（含进行中） |
| GET | `/api/debug/agent-evals/runs/{id}` | 单轮快照（进行中含 `current_trace`） |
| GET | `/api/debug/agent-evals/runs/{id}/cases/{case_id}/trace` | 该用例完整 trajectory + 工作区产物（相对路径、脱敏） |
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
| Trace Inspector | 开发者调试真实用户回合 | 否（只读已落盘 trace） |
| Agent Eval | 回归 / 准入真实 harness | 离线 demo 否；live lab 是 |

二者互补：Eval 保证行为契约，Trace 解释线上回合。

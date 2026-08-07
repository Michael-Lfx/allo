# Agent 可观测性与评测

本文说明 Flowy / allo 中 **Developer Mode Trace** 与 **会话对话评测（Agent Eval）** 的职责边界与用法。

## Trace（Developer Mode）

Agent 回合会写入结构化 turn trace（spans、token、工具计数），落盘于：

`{data_dir}/diagnostics/agent-traces/`

读取受 **开发者模式**（`system.developerMode`）门控：

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/api/debug/agent-traces?conversationId=&limit=` | 按会话列出索引 |
| GET | `/api/debug/agent-traces/recent?limit=` | 最近条目 |
| GET | `/api/debug/agent-traces/{trace_id}` | 完整 turn（含 spans） |

实现：`nomi-agent-trace`（存储 / 脱敏）→ `nomifun-ai-agent::AgentTraceHub` → `nomifun-conversation::routes_trace`。

### UI

会话页 `ChatLayout` 在开发者模式开启且存在 `conversation_id` 时显示 **Trace** 按钮，打开嵌入式 Drawer（`AgentTraceInspector`）：

- 列出该会话的 turn 索引
- 点击行查看 span 瀑布与 tokens / tools / elapsed

未开启开发者模式时组件不渲染；API 在未开启时返回 403。

## Eval（session_dialogue corpus）

crate：`nomi-agent-eval`（库默认可编译；示例二进制需 `--features agent-eval`）。

- 语料：`crates/agent/nomi-agent-eval/evaluation/corpus.conversation.json`
- 套件：`session_dialogue`（基础对话 / 工具 / 安全 / 推理）
- 离线：`OfflineDemoHarness` 对每个 case 返回可过关的脚本化 transcript（无需 LLM）
- 证据：JSONL（`case_id`、`category`、`success`、scorer 结果、耗时）；prompt 经 `nomi-redact` 脱敏，不落盘 `sk-…` 密钥
- 可续跑：`run` 默认 `--resume`，跳过输出文件中已有 `case_id`

### 如何运行

```bash
# 单测 + demo / summarize 逻辑
cargo test -p nomi-agent-eval --all-targets

# 离线 demo → JSONL
cargo run -p nomi-agent-eval --example agent_eval --features agent-eval -- \
  demo --output /tmp/agent-eval-demo.jsonl

# 汇总成功率
cargo run -p nomi-agent-eval --example agent_eval --features agent-eval -- \
  summarize --input /tmp/agent-eval-demo.jsonl --output /tmp/agent-eval-summary.json
```

操作手册见 [`evaluation/README.md`](../../crates/agent/nomi-agent-eval/evaluation/README.md)。

## 关系

| 能力 | 面向 | 是否依赖 LLM |
| --- | --- | --- |
| Trace Inspector | 开发者调试真实回合 | 否（只读已落盘 trace） |
| Agent Eval | 回归 / 准入（确定性 scorer） | Demo 否；可插真实 harness |

二者互补：Eval 保证行为契约，Trace 解释线上回合。

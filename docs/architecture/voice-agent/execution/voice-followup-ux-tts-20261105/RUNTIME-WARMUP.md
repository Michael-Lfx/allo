# E · 语音 runtime 冷启动与模型继承

- 日期：2026-11-06（UTC+8）
- 分支：`explore/voice-runtime-warmup`
- 基线：`4ff46e0ab`（合并后的 main）
- 已提交基础版本：`5e28569b4 fix: inherit saved default model for voice sessions`
- 已提交审查修复与本文档：`18df67f4a fix: address voice runtime review findings`
- 已提交 fallback/模型校验 checkpoint：`9f66997e8 fix: surface voice model fallback and isolate validation`
- 执行方案与边界：[EXECUTION-PLAN-20261106.md](EXECUTION-PLAN-20261106.md)

## 1. 问题与证据

### 1.1 语音第一次任务慢

同一语音 conversation `01a1aebc-5407-7232-ba13-666fe8a95013`：

| 项 | 日志 |
| --- | --- |
| 第一次 runtime build | **27,549ms** |
| 第二次 runtime build | **1ms** |
| 第一次任务模型执行 | **10,461ms** |
| 注入 MCP | `mcp_count=5`：fetch / playwright / excel / Figma / gateway |

### 1.2 普通会话为什么快

普通会话 `01a1aede-5043-7e20-a807-c3a579af82d8`：

- `mcp_server_ids: []`，只有 gateway；
- 创建后立即 warmup，`mcp_count=1`；
- warmup 约 **410ms**；
- 首次任务 `Agent runtime ready=1ms`。

普通会话不是 runtime 天生更快，而是 **外部 MCP 少 + 提前 warmup**。

### 1.3 语音会话的默认 MCP 来源

自动创建的 voice conversation 请求里没有 `selected_mcp_server_ids`。`create_inner` 把“缺省”解释为“加载全部已启用 MCP”，于是每个新语音会话都连接 fetch / playwright / excel / Figma，再叠加 gateway。

## 2. 方案 A：语音自动会话 gateway-only

实现：

- `ConversationService::apply_default_voice_mcp_fence` 给自动语音创建请求补显式空选择：
  - `create_voice_idempotent`
  - `create_provisional_voice_conversation`
  - `ensure_default_voice_conversation`
- 显式选择或 preset snapshot 仍然优先。
- 效果：新语音会话只保留 gateway，不再全量连接外部 MCP。

验证：

| conversation | MCP | 首次 runtime build |
| --- | ---: | ---: |
| `01a1aefb-...` | 1 | **490ms** |
| `01a1af01-...` | 1 | **364ms** |
| 同会话后续轮次 | 1 | 0–2ms |

旧语音会话仍有 4 个外部 MCP；按本轮范围不迁移，只保证新建自动语音会话。

## 3. P0：新语音会话继承用户默认模型

问题证据：

- `client_preferences.nomi.defaultModel` = `AIPC-deepseek-v4.1-flash`
- 同分钟普通会话 `01a1af07-3234...`：`AIPC-deepseek-v4.1-flash`
- 新建语音会话 `01a1af07-5864...`：`AIPC-auto-cost`
- 语音 `voice_creation_request.model` 也是 `AIPC-auto-cost`

根因：

- 后台唤醒 `startVoiceConversation` 走 `current_selection`，不带 model；
- 自动 provisional 路径从已有默认语音会话/默认解析器取模型，不读用户当前选择；
- 只有页面显式点语音按钮的 `new_voice_conversation` 会带 `mainModelRef`。

实现：

- `ConversationService::default_voice_model_preference()` 读取并校验 `nomi.defaultModel`。
- `create_provisional_voice_conversation`、`ensure_default_voice_conversation`、`create_voice_idempotent` 在无显式模型、无 preset 时使用该偏好。
- recorder 的自动创建路径优先使用保存的默认模型。
- 显式 `new_voice_conversation` 的 model 仍然优先。

## 4. P1：显式、可观测的模型回退

原则：

> 不允许静默降级；允许有日志、可追踪的显式回退。

顺序：

1. 显式传入模型；
2. `nomi.defaultModel`；
3. 默认语音会话/runtime 模型；
4. 运行时默认模型。

行为：

- 保存模型格式非法：记录 `voice_default_model_preference_invalid_fallback`，继续回退。
- 保存模型存在但不可用：`validate_model` 返回错误，记录 `voice_default_model_unavailable_fallback`；回退后记录实际 `voice_default_model_fallback`，并通过 `VoiceCallStartResponse.model_fallback` 返回。用户已确认：fallback 后仍有模型，**不要求 UI toast**。
- 显式模型不可用：记录 `voice_task_model_unavailable_fallback` 并回退到默认模型。
- preset 模型不可用：不自动 fallback，返回 `resolve_model` 的原始错误（通常是 `model_unavailable`），避免 preset 快照与实际模型不一致。
- 只有默认模型也解析失败时才返回 `model_unavailable` / `task_execution_model_unconfigured`。

## 5. 代码审查修复（本轮追加）

- 删除旧语音会话的 model 同步：按“旧语音会话可以不管，只管新对话”收窄范围，避免覆盖旧会话/显式模型。
- 新增 `VoiceTaskRuntime::validate_model`，唤醒路径只校验模型，不再用 `prepare_configuration` 的 workspace/session-mode 校验，避免把无效 workspace 误判成模型不可用。
- fallback 结果通过 `VoiceCallStartResponse.model_fallback` 返回，保留给需要提示的调用方；用户确认静默沿用即可，UI toast 不作为验收项。
- 修正 P1 fallback 日志：用户默认模型探测失败后，`used_saved_default` 复位，确保实际 fallback 模型被记录。
- preset 模型不可用不再隐式 fallback，防止 preset snapshot 与实际模型不一致。
- 保留复用旧会话的现有行为作为边界：旧 workspace/retained 会话仍可能复用旧 MCP 配置；本轮不迁移。

## 6. 真机结果（2026-11-06）

| 项 | 结果 |
| --- | --- |
| E1 新语音会话 MCP | `mcp_count=1`（仅 gateway） |
| E2 首次 runtime | 第一次 1379ms（宿主刚启动），第二次新建会话 **312ms** |
| E3 模型继承 | `AIPC-deepseek-v4.1-flash` |
| E4 fallback | 临时不可用模型时日志：`voice_default_model_unavailable_fallback` → `voice_default_model_fallback`（`AIPC-auto-cost`）；用户确认 toast 不要求 |
| E5 普通会话 | `AIPC-deepseek-v4.1-flash`、`mcp_count=1`、runtime 0ms |
| E7 旧会话 | `01a1aebc` 仍为 4 MCP + `AIPC-auto-cost`，未被修改 |

## 7. 验证

| 命令 | 结果 |
| --- | --- |
| `cargo test -p nomifun-conversation` | **662 passed / 0 failed**（新增显式模型测试后） |
| `cargo test -p nomifun-conversation explicit_voice_model_overrides_saved_default_preference` | **1 passed / 0 failed** |
| `cargo test -p nomifun-conversation preset_model_reconciliation` | **8 passed / 0 failed** |
| `cargo check -p nomifun-app` | exit 0 |
| `cargo build -p Flowy` | exit 0 |
| `rustfmt --edition 2024 --check` 改动 Rust 文件 | exit 0 |
| `git diff --check` | exit 0 |
| `cargo test -p nomifun-app voice_ ...` | 本地环境阻断：`sherpa_onnx_sys` rlib 缺失，非本次代码失败 |
| `bun test` voiceCompanion/callIntent/Router 3 文件 | **52 pass / 0 fail / 212 expect** |
| `bun run typecheck` | exit 2；**71 个既有基线错误，改动文件 0 error** |

新增测试：

- 默认模型偏好读取成功；
- 非法默认模型被拒绝；
- provisional 语音会话继承保存的默认模型；
- gateway-only MCP fence 保留显式 MCP 选择。

## 7. 边界与未完成

- **旧语音会话不迁移**：旧会话可能仍是 5 MCP / `AIPC-auto-cost`；复用旧 retained/workspace 会话时不能享受本次优化。
- **新通话是否真正新建**：workspace 会话 30 分钟内可能复用旧 conversation；本轮不改变该行为。
- **外部工具能力回退**：gateway-only 后浏览器、Excel、Figma 等 MCP 工具默认不可用；需要后续决定 fetch-only / lazy MCP / 保持空默认。
- **P1 UI toast 已取消**：用户确认 fallback 后仍有可用模型，静默沿用可接受；不再要求唤醒路径显示提示。
- **真机后验已完成（2026-11-06）**：见下方“真机结果”；首次 1379ms 为宿主刚启动的波动，第二次新建会话 312ms。
- **app 集成测试**：受 `sherpa_onnx_sys` 环境问题阻断。

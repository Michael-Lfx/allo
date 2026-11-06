# E · 语音 runtime 冷启动与模型继承

- 日期：2026-11-06（UTC+8）
- 分支：`explore/voice-runtime-warmup`
- 基线：`4ff46e0ab`（合并后的 main）
- 已提交基础版本：`5e28569b4 fix: inherit saved default model for voice sessions`
- 当前审查修复与本文档：未提交、未 push

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
- 保存模型存在但不可用：记录 `voice_default_model_unavailable_fallback`，回退后记录实际 `voice_default_model_fallback`。
- 显式模型不可用：记录 `voice_task_model_unavailable_fallback` 并回退到默认模型。
- preset 模型不可用：不自动 fallback，返回 `task_execution_model_unconfigured`，避免 preset 快照与实际模型不一致。
- 只有默认模型也解析失败时才返回 `model_unavailable` / `task_execution_model_unconfigured`。

## 5. 代码审查修复（本轮追加）

- 删除旧语音会话的 model 同步：按“旧语音会话可以不管，只管新对话”收窄范围，避免覆盖旧会话/显式模型。
- 修正 P1 fallback 日志：用户默认模型探测失败后，`used_saved_default` 复位，确保实际 fallback 模型被记录。
- preset 模型不可用不再隐式 fallback，防止 preset snapshot 与实际模型不一致。
- 保留复用旧会话的现有行为作为边界：旧 workspace/retained 会话仍可能复用旧 MCP 配置；本轮不迁移。

## 6. 验证

| 命令 | 结果 |
| --- | --- |
| `cargo test -p nomifun-conversation` | **661 passed / 0 failed**（各集成测试全部通过） |
| `cargo check -p nomifun-app` | exit 0 |
| `cargo build -p Flowy` | exit 0 |
| `rustfmt --edition 2024 --check` 改动 Rust 文件 | exit 0 |
| `git diff --check` | exit 0 |
| `cargo test -p nomifun-app voice_ ...` | 本地环境阻断：`sherpa_onnx_sys` rlib 缺失，非本次代码失败 |

新增测试：

- 默认模型偏好读取成功；
- 非法默认模型被拒绝；
- provisional 语音会话继承保存的默认模型；
- gateway-only MCP fence 保留显式 MCP 选择。

## 7. 边界与未完成

- **旧语音会话不迁移**：旧会话可能仍是 5 MCP / `AIPC-auto-cost`；复用旧 retained/workspace 会话时不能享受本次优化。
- **新通话是否真正新建**：workspace 会话 30 分钟内可能复用旧 conversation；本轮不改变该行为。
- **外部工具能力回退**：gateway-only 后浏览器、Excel、Figma 等 MCP 工具默认不可用；需要后续决定 fetch-only / lazy MCP / 保持空默认。
- **P1 无 UI 提示**：回退只有日志，用户不会主动看到 toast。
- **真机后验**：尚未在新 commit 上重新测量首轮 runtime build；需要重启 Flowy、唤醒新建语音会话并确认模型与耗时。
- **app 集成测试**：受 `sherpa_onnx_sys` 环境问题阻断。

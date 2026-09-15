# 上游契约漂移与网关拒绝调查记录

> 状态：只读分析完成；修复方案已定义，代码改动尚未实施
>
> 最后维护：2026-09-15
>
> 探索分支：`docs/upstream-contract-drift-investigation`
>
> 证据范围：用户反馈日志、本机开发数据目录、工作区数据、`you.com` MCP 在线探针

## 1. 问题范围

本记录覆盖两条独立的失败线索，它们属于同一类问题：**客户端对远端契约做了硬假设，
远端契约变化或拒绝请求后，客户端只能烧重试/兜底，不能按语义降级**。

| 线索 | 时间 | 现象 | 归属 |
| --- | --- | --- | --- |
| A | 2026-09-12 | 用户反馈错误卡 `USER_LLM_PROVIDER_GATEWAY_ERROR`（"模型服务商暂不可用"） | 模型网关（Flowy Cloud provider） |
| B | 2026-09-15 | 本机 qwen3.8-flash 使用 `web_search` 时，托管搜索 provider 在数据返回阶段失败 | `flowy-web` 托管搜索 |

证据来源：

```text
反馈日志：D:\AllDownload\2026-07\0a88947a-e8c6-42c0-a53e-4183c52d0921\
本机日志：D:\tmp\flowy-dev\logs\
会话数据：D:\tmp\flowy-dev\nomi-sessions\、D:\tmp\flowy-dev\flowy-backend.db
工作区  ：D:\tmp\777\（仅会话工作目录，无日志）
```

## 2. 案例 A：`AIPC-GPT5.6-Sol` 的 tools + reasoning_effort 组合被上游通道拒绝

### 2.1 原始错误

`2026-09-12.nomicore.log:6943`：

```text
Provider error: API error 500: {"code":500,"msg":"Model call failed. Please try again
later: Function tools with reasoning_effort are not supported for gpt-5.6-sol-tec-do
in /v1/chat/completions. To use function tools, use /v1/responses or set
reasoning_effort to 'none'.","error_key":"error.all_channel_models_failed"}
```

会话 `01a09650-3f54-7a92-bd38-1a4dbe594352`，事件时间 `2026-09-12T15:51:00.875Z`，
引擎耗时 23 584 ms，最终分类 `UserLlmProviderGatewayError`、`retryable: true`。

### 2.2 请求为什么必然带 `reasoning_effort`

- 云端目录声明该模型支持推理档位：
  `[reasoning-effort-diagnosis] ... model=AIPC-GPT5.6-Sol ... catalog_reasoning_effort=Some(["low","medium","xhigh"])`
  （`2026-09-12.nomicore.log:89` 等多处同步日志）。
- `provider_config.rs` 读取目录后写入 `compat_overrides.supports_effort/effort_levels`
  （`crates/backend/nomifun-ai-agent/src/factory/provider_config.rs:141`）。
- 会话未显式选择档位时，`resolve_session_reasoning_effort` 默认取 `medium`
  （`crates/backend/nomifun-ai-agent/src/factory/nomi.rs:81`）。
- OpenAI 兼容 provider 固定走 `/v1/chat/completions`
  （`crates/agent/nomi-config/src/compat.rs:213`），并且带内置工具 + MCP 工具。
- `build_request_body` 只要 `reasoning_effort` 为 `Some` 就会写入请求体
  （`crates/agent/nomi-providers/src/openai.rs:456`）。

因此该模型上所有带工具的请求都会被上游确定性拒绝，与"服务商宕机"无关。

### 2.3 为什么用户看到的是"重试后失败"

网关把上游拒绝包成 HTTP 500 + `error.all_channel_models_failed`。
`crates/agent/nomi-providers/src/retry.rs:63` 把 500 视为瞬时错误并重试 2 次：

```text
15:50:37.304  首次发送（nomi.log:63）
15:50:43.374  工具 schema 降级重发（nomi.log:65-66）
15:50:47.081  瞬时重试 1（nomi.log:68）
15:50:52.454  瞬时重试 2（nomi.log:70）
15:51:00.870  终止错误（nomi.log:72）
```

### 2.4 同一份反馈日志中的其它契约问题

| 时间 | 现象 | 结论 |
| --- | --- | --- |
| 15:36–15:52 | `AIPC-claude-fable-5.0` 反复 `No available channel for model claude-fable-5 under group jsy-low (distributor)` | 真实上游通道缺货，与主报障无关 |
| 15:44:18 | `maxOutputTokens value of 128000 but the supported range is from 1 to 65537` | 目录把 gemini-3.5-flash 输出上限配成了 128000（疑似上下文窗口） |
| 15:45:42 | `parameters.any_of[0].required: only allowed for OBJECT type` | schema 清洗只处理根级组合关键字，嵌套 `any_of` 未处理 |

## 3. 案例 B：本机 qwen3.8-flash 的托管搜索在数据返回阶段失败

### 3.1 会话事实

会话 `01a0a3e8-f187-7743-9ea9-5d79ddb6b1d4`（2026-09-15 15:12:28–15:14:35 本地，
模型 `AIPC-qwen3.8-flash`）：

- 2 次 `web_search` + 1 次 `web_extract` 全部 `completed`，后续模型轮次全部成功，
  `terminal: ok`；
- `flowy-backend.db` 中该会话没有 error 提示行，`nomi-sessions` 会话文件无错误记录；
- 工作区 `D:\tmp\777\conversations\01a0a3e8-f185-…` 无失败产物。

即：**模型侧没有报错**；报错在托管搜索的数据返回阶段，并被 DuckDuckGo 兜底掩盖。

### 3.2 数据返回阶段的失败（WARN）

`2026-09-15.nomicore.log`：

```text
07:12:56.281  managed web search provider failed  provider=parallel  error_class=timeout
07:12:59.285  managed web search provider failed  provider=you       error_class=timeout
07:13:01.352  managed web search succeeded        provider=duckduckgo
07:13:56.184  managed web search provider failed  provider=parallel  error_class=timeout
07:13:58.152  managed web search provider failed  provider=you       error_class=schema_mismatch
07:13:59.780  managed web search succeeded        provider=duckduckgo
```

每次搜索被两个失败 provider 白拖约 6 s（provider 预算 3 s + 3 s），搜索总耗时 6.6–8.1 s。

### 3.3 根因（在线探针证实）

`you` 适配器要求端点**恰好暴露 1 个工具**
（`crates/agent/flowy-web/src/managed/remote.rs:649`）：

```rust
if self.id == SearchProviderId::You && tools.len() != 1 {
    return Err(SearchAttemptError::SchemaMismatch);
}
```

对 `https://api.you.com/mcp?profile=free` 执行 `initialize` + `tools/list` 探针，实测：

```text
tool_count: 2
you-search
you-discover
```

并且新 `you-search` 的 `required=query`、`query:string`、`count:integer`、
`outputSchema.properties.results` 全部满足现有
`validate_tool_schema` / `validate_output_schema`（`remote.rs:880`、`remote.rs:926`）。
也就是说，只要放宽"恰好 1 个工具"的假设，该 provider 立即可恢复。

### 3.4 失败被升级为永久禁用

`SearchAttemptError::SchemaMismatch` 会写入 `disable_reason`
（`crates/agent/flowy-web/src/managed.rs:219`），而 `availability()` 对
`disable_reason` 的 provider 永远返回 `Disabled`，进程内不再尝试，直到重启。
09-02 已出现 2 次同样的 `you schema_mismatch`（`2026-09-02.nomicore.log:6304-6307`），
即该契约漂移至少存在两周。

## 4. 同类问题矩阵

| 日期 | 现象 | 归因 | 现状 |
| --- | --- | --- | --- |
| 2026-08-18 | GPT5.6-Sol `Invalid schema for function 'Read': … oneOf … at the top level`，4 轮失败出错误卡 | 当时构建未触发 schema 降级（只有瞬时 500 重试） | 当前代码已含降级重试（09-12 gemini 案例已触发）；Read 场景待回归 |
| 2026-09-12 | tools + `reasoning_effort` 被通道拒绝 | 目录声明 + 默认 `medium` + chat/completions | 未修复（案例 A） |
| 2026-09-12 | `maxOutputTokens 128000 > 65537` | 目录输出上限失真 | 未修复 |
| 2026-09-12 | `any_of[0].required: only allowed for OBJECT type` | schema 清洗不处理嵌套组合关键字 | 未修复 |
| 08-03 ~ 09-15 | 托管搜索 provider 失败累计 31 次（08-03:2、08-14:2、08-19:2、08-25:17、09-02:4、09-15:4） | parallel 超时 + you 契约漂移 | 未修复（案例 B） |

## 5. 修复方案（纯客户端）

### P0 托管搜索（改动最小、可立即验证）

1. **放宽 you 契约**：删除 `tools.len() != 1` 硬约束，按名字查找 `you-search`；
   找不到才算 `ToolMissing`；额外工具（如 `you-discover`）不应导致失败。
2. **可恢复的兼容失败**：`SchemaMismatch` 从永久禁用改为"冷却后允许重新探测"
   （或每次搜索允许一次 re-probe），并补一条 INFO 日志说明 provider 被跳过及原因。
3. **预算与对冲**：`PARALLEL_SLOT_BUDGET`/`YOU_SLOT_BUDGET` 各 3 s 串行 →
   收紧到 1.5 s，或在 parallel 超时后让 you + ddg 并行（hedge），
   把搜索从 6.6–8.1 s 降到约 2–3 s。
4. **回归测试**：工具列表为 2 个时仍选中 `you-search`；工具列表变化后可自动恢复。

### P1 模型网关与 schema

5. **effort 降级**：当请求带 tools 且上游返回
   `Function tools with reasoning_effort are not supported` 时，一次性改用
   `reasoning_effort: "none"` 并记忆到 provider 实例；同时把该错误从"瞬时 500 重试"
   中排除，避免 2 次无意义重试。
6. **输出上限协商**：命中 `maxOutputTokens … supported range …` 时按错误中的上限收紧
   并重发，后续请求沿用（修复 Gemini 类问题）。
7. **嵌套 schema 清洗**：`sanitize_json_schema` 对嵌套 `anyOf/oneOf` 做投影
   （修复 Gemini `any_of[0].required`）；回归验证 Read 顶层 `oneOf` 在当前构建已修复。

## 6. 验证边界

- 本记录创建前没有修改业务代码；P0/P1 均未实施。
- 案例 B 的根因已由在线探针证实；案例 A 的根因由客户端代码路径 + 上游错误语义证实，
  未重新调用真实网关复现。
- 反馈日志中的 `claude-fable-5` 缺货、代理层 502 等属于外部可用性问题，本记录不展开。
- 未对托管搜索预算做压测；上述 6.6–8.1 s 数字来自一次会话内两次搜索的实测。

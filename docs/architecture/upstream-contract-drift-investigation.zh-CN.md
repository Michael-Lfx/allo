# 上游契约漂移与网关拒绝调查记录

> 状态：只读分析完成；修复方案已定义，代码改动尚未实施
>
> 最后维护：2026-09-15
>
> 探索分支：`fix/upstream-contract-drift`
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
反馈日志  ：D:\AllDownload\2026-07\0a88947a-e8c6-42c0-a53e-4183c52d0921\
本仓库环境：D:\tmp\flowy-dev\（日志/DB/nomi-sessions），工作区 D:\tmp\777\
备用项目  ：D:\workSpace\allo\（branch feat/chat-auto-image-unblock）
  数据目录：C:\Users\38788\AppData\Local\Flowy\Nomi-dev\
  工作区  ：D:\d\tmp\
  日志    ：C:\Users\38788\AppData\Local\Flowy\Nomi-dev\logs\
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

## 5. 推荐修复方案与取舍（供交叉审查）

### 5.1 修复项索引

**P0（先做，覆盖两处现场）**

| 项 | 对应问题 | 改动位置 | 一句话 |
| --- | --- | --- | --- |
| P0-1 | 问题 1（§5.3） | `nomi-providers` | 带 tools 时命中 effort 拒绝 → 改用 `none` 重发一次并记忆 |
| P0-2 | 问题 2（§5.4） | `flowy-web/remote.rs` | 删除 you "恰好 1 个工具"硬约束，按名取 `you-search` |
| P0-3 | 问题 2（§5.4） | `flowy-web/managed.rs` | `SchemaMismatch` 从永久禁用改为冷却后可重新探测 |
| P0-4 | 问题 3（§5.5） | `flowy-web/managed.rs` | 预算收紧/对冲 + DDG 有界重试，不放大总预算 |

**P1**

| 项 | 对应问题 | 改动位置 | 一句话 |
| --- | --- | --- | --- |
| P1-1 | 问题 5（§5.6） | `nomifun-ai-agent`/`nomi-providers` | 按 `supported range` 报错协商 `max_tokens` |
| P1-2 | 问题 6（§5.7） | `nomi-config/compat.rs` | 嵌套 `anyOf/oneOf` 投影 |
| P1-3 | 问题 8（§5.8） | `nomi-agent/engine` | 未广告工具的进度预览降级为忽略 |
| P1-4 | 问题 9（§5.9） | `nomi-providers/retry.rs` | TLS EOF / 超时增加一次有界重试 |

### 5.2 总原则

1. **错误驱动协商**：只在收到上游明确拒绝（错误正文给出修复指令）后才改变请求形状并
   重发一次；不做模型名/供应商名单预判。
2. **学习式降级**：协商结果记在 provider 实例上（沿用
   `sanitize_tool_schemas: AtomicBool` 的既有模式，`openai.rs:27`），本次进程内不再
   发送已知会被拒的请求；不改全局配置。
3. **有界重试**：任何新增重试都必须有次数上限，与现有
   `MAX_INITIAL_REQUEST_RETRIES = 2`、`MAX_STREAM_RETRIES = 2`
   （`retry.rs:13-14`）同级；不允许无界重试。
4. **不动权限边界**：只处理参数形状/展示型事件/兜底预算；不放松工具执行授权。

### 5.3 问题 1：tools + `reasoning_effort` 被拒

**推荐**：在 `openai.rs` 的协商循环（`openai.rs:855-931`，已有 `stream_options` 与
tool schema 两项）新增第三项：命中
`Function tools with reasoning_effort are not supported` 时，把 `reasoning_effort`
改为 `"none"` 重发一次并记忆；同时把该错误从瞬时 500 重试中排除（`retry.rs:63`）。

**理由**

- 上游错误正文本身就是修复指令（"set reasoning_effort to 'none'"），语义明确；
- 复用既有协商循环与学习标志，没有新概念、改动最小；
- 只影响"带 tools 且被实际拒绝"的请求，对 deepseek 等正常支持该组合的模型零回归。

**备选方案与拒绝原因**

| 备选 | 拒绝原因 |
| --- | --- |
| 运营侧去掉目录里的 `reasoning_effort` 声明 | 无权限；且下一个新模型仍会漂移，客户端继续挂 |
| 所有带 tools 的请求都不发 `reasoning_effort` | 会连累正常支持该组合的模型，属产品能力回退 |
| 按模型名/供应商硬编码屏蔽 | 名单会腐烂；错误驱动不依赖名单 |
| 改走 `/v1/responses` | 需要上游通道支持 + provider 协议切换，超出本次范围；可留作中期议题 |
| 重发时直接省略字段（而不是 `none`） | 省略后上游可能仍按默认开启推理而被拒；错误明确要求 `none` |

**风险/开放问题**：若某通道不接受字面值 `none`，需要第二次协商（回退到省略字段）；
实现以"重试一次仍失败即终止"封顶。

### 5.4 问题 2：`you` 单一工具硬约束 + 永久禁用

**推荐**：`remote.rs:649` 删除 `tools.len() != 1`，改为按名查找 `you-search` 并校验
其 schema（`validate_tool_schema` / `validate_output_schema` 已存在）；`managed.rs:219`
的 `SchemaMismatch` 改为"冷却后允许重新探测"，并补 INFO 日志说明 provider 被跳过。

**理由**

- 在线探针实测：you.com 现在暴露 2 个工具（`you-search`、`you-discover`），且
  `you-search` 的 required/properties/outputSchema 完全满足现有校验 → 放宽后立即可用；
- `parallel` 适配器本来就是"按名查找"（`remote.rs:654-666`），you 的计数约束是唯一
  特例，删除可消除不一致；
- 计数约束没有安全价值，只是过度拟合 vendor 旧的端形态。

**备选方案与拒绝原因**

| 备选 | 拒绝原因 |
| --- | --- |
| 把期望值从 1 改成 2 | 下一次 vendor 增删工具又会坏，治标不治本 |
| 永久弃用 you（`ddg_only`） | 少一个 provider 直接放大问题 3（DDG 独自承压）的全失败概率 |
| 只允许"重启后恢复" | 用户会话内无法自愈；09-01/09-04/09-15 证明漂移长期存在 |
| 客户端先过滤工具列表再按 1 个校验 | 与"按名查找"等价但更绕，不如直接按名取 |
| 运营/服务端把工具改回 1 个 | 无权限，且不可控 |

**风险/开放问题**：冷却时长取值（建议 10 分钟）；是否把"工具数变化"单独记日志以观察
vendor 行为。

### 5.5 问题 3：搜索全 provider 超时（含 DDG）

**推荐**：在 `TOTAL_BUDGET = 12s`（`managed.rs:70`）不变的前提下：
`PARALLEL_SLOT_BUDGET` / `YOU_SLOT_BUDGET` 各 3 s → 1.5 s，为 DDG 保留足够预算；
parallel 超时后让 you 与 DDG 并行（hedge）；DDG 失败后做一次有界内部重试。

**理由**

- 实测 DDG 成功耗时 1.6–2.1 s（09-15）；parallel/you 在 09-04 全失败、09-15 仍有
  失败 —— 预算应倾斜给真正工作的通道；
- 12 s 上限不变，工具延迟不恶化；hedge 可把 09-15 观测的 6.6–8.1 s 降到约 2–3 s；
- 不改 provider 实现，只在调度层调整。

**备选方案与拒绝原因**

| 备选 | 拒绝原因 |
| --- | --- |
| 所有预算等比放大（如 10/10/10） | 全挂时单次搜索 30 s，工具延迟爆炸 |
| 超时也按永久禁用处理 | 超时是瞬态的；把唯一工作的 DDG 因网络抖动禁用会制造更大故障 |
| 依赖模型自行重试（现状 `do not repeat`） | 09-04 观测到模型转去 `web_extract`（也超时），浪费更多 turn |
| 搜索改服务端/网关侧 | 不在本仓库可控范围 |
| 增加新 provider | 成本/合规/密钥，短期不可行 |

**风险/开放问题**：需确认 `web_search` 工具外层是否还有独立超时（目前只看到 managed
层 12 s）；DDG 重试与其反爬限流的相互作用（观测 `result_count=8` 正常，风险低）。

### 5.6 问题 5：`maxOutputTokens` 超出上游范围

**推荐**：命中 `supported range is from 1 to N` 时解析 N，按 N 收紧重发一次，
并记住（provider+model）；与 §5.3 共用同一协商框架。

**理由**：错误即精确上限，无需猜测；一次协商即可长期受益。

**备选方案与拒绝原因**

| 备选 | 拒绝原因 |
| --- | --- |
| 全局把 `max_tokens` 压到固定值（如 32k） | 会砍掉 deepseek 等模型的合法长输出，产品能力回退 |
| 只改云端目录 | 无权限；且新模型/新通道还会漂移 |
| 直接信任 models.dev 输出上限 | Flowy 内建 provider 的目录优先规则已存在，本次错误正来自该路径 |

### 5.7 问题 6：嵌套 `anyOf` / `oneOf`

**推荐**：扩展 `sanitize_json_schema`（`nomi-config/compat.rs:251`）的投影到嵌套层级，
对非 object 节点的 `required` 做投影/剔除；仅在已知被拒后的 sanitize 分支启用。

**理由**：清洗路径已存在且是"学习后启用"；改动局部、可单测
（`compat.rs` 已有 827/864 等投影测试可参照）。

**备选方案与拒绝原因**

| 备选 | 拒绝原因 |
| --- | --- |
| 从工具集中删除被拒工具 | 直接损失能力，不可取 |
| 全局强制所有 schema 先清洗 | 牺牲正常 provider 的 schema 保真度（约束被投影弱化） |
| 按 Gemini/供应商特判 | 共享协议路径下按供应商分支脆弱，且名单会腐烂 |

**风险/开放问题**：投影会弱化参数约束，需保证"仅在被拒后启用"的现有语义不被破坏。

### 5.8 问题 8：未广告工具的进度预览终止 turn

**推荐**：`engine/mod.rs:2218` 对 progress 预览命中未广告工具时降级为 warn/忽略；
**真正的未广告 ToolCall 仍保持硬失败**。

**理由**：进度预览是纯展示事件（无副作用），让它杀死整个回合与产品目标不符；
权限封锁点应留在执行路径上。

**备选方案与拒绝原因**

| 备选 | 拒绝原因 |
| --- | --- |
| 保持现状（整轮失败） | 一个 UI 预览就能让用户回合失败，已实际发生两次 |
| 自动把该工具加入本次允许集 | 突破工具权限 ceiling，违反仓库安全约定 |
| 所有未广告事件（含 ToolCall）都忽略 | 会放松执行授权边界，安全风险不可接受 |

### 5.9 问题 9：网关连接中断（TLS EOF / 超时）无重试

**推荐**：对 `peer closed connection without sending TLS close_notify` 与
`operation timed out` 的**初始发送**增加一次有界重试，复用
`with_initial_request_retry`（`retry.rs:22`）。

**理由**：发送尚未进入响应阶段即失败，重试成本低；08-31/09-04 观测到 90 s/120 s
挂起后才失败，用户体验极差。

**备选方案与拒绝原因**

| 备选 | 拒绝原因 |
| --- | --- |
| 不做重试，依赖用户手动重发 | 90 s+ 等待后用户往往已放弃（09-04 实测） |
| 无界/多次重试 | 放大上游故障，且可能叠加现有 5xx 重试形成多次计费请求 |
| 按"上下文溢出"处理（compact 后重试） | 与真实原因无关，不解决问题 |

**风险/开放问题**：与现有 `MAX_INITIAL_REQUEST_RETRIES=2` 叠加后的总次数需明确上限；
重试的计费幂等性假设与现有 5xx 重试同级（需 reviewer 确认可接受）。

### 5.10 落地顺序与验证

1. P0-2/P0-3（you 契约）→ 单测：2 工具时选中 `you-search`、`SchemaMismatch` 冷却后
   可恢复；
2. P0-4（预算/hedge）→ 单测：deadline 不被突破、hedge 触发条件、DDG 重试上限；
3. P0-1（effort 协商）→ wiremock 模式（参照 `openai.rs:3337-3380`）：
   首次 200 被 500 拒绝 → 断言第二次请求含 `reasoning_effort:"none"`；
4. P1-1…P1-4 依次；
5. 回归：`cargo test -p flowy-web`、`cargo test -p nomi-providers`、
   `cargo test -p nomi-config`、`cargo test -p nomi-agent`；
6. 真实验收：qwen3.8-flash 复测 `web_search`（如深圳天气），确认日志中
   `you` 从 `schema_mismatch` 恢复为正常 attempt；GPT5.6-Sol 复测带工具对话，
   确认不再出现 23 s 重试后失败。

### 5.11 开放问题（需 reviewer 决策）

1. 学习式标记是否需要跨重启持久化到 DB？当前建议进程内（与 `sanitize_tool_schemas`
   一致），避免引入新的持久化契约。
2. `SchemaMismatch` 的冷却时长与探测频率取多少（建议 10 分钟 + 每次搜索最多一次
   re-probe）？
3. 未广告工具的进度预览：忽略（建议）还是仅记录 warn 保留可观测性？
4. 传输层重试是否接受潜在重复计费（与现有 5xx 重试同级）？
5. 是否把 `/v1/responses` 作为推理模型的中期路线（取决于网关侧支持，需单独调研）？

## 6. 备份项目（D:\workSpace\allo）环境核对与关联发现

### 6.1 工作区与数据目录定位

`D:\workSpace\allo` 与当前工作仓库同 remote，是备用 checkout
（branch `feat/chat-auto-image-unblock`，最近构建
`D:\workSpace\allo\target\debug\Flowy.exe`，2026-09-08 20:29）。

该构建运行时使用的是 dev 通道默认数据目录
`C:\Users\38788\AppData\Local\Flowy\Nomi-dev`：

```json
// C:\Users\38788\AppData\Local\Flowy\Nomi-dev\.nomifun-work-root-binding.json
{ "data_root": "C:\\Users\\38788\\AppData\\Local\\Flowy\\Nomi-dev",
  "work_root": "D:\\d\\tmp" }
```

- 工作区：`D:\d\tmp`（含 `conversations/`，63 个会话工作目录，最近 2026-09-07 21:07）；
- 日志：`C:\Users\38788\AppData\Local\Flowy\Nomi-dev\logs\`（2026-08-05 … 2026-09-08）；
- 会话数据：`Nomi-dev\flowy-backend.db`、`Nomi-dev\nomi-sessions\`。

对照：2026-09-15 实际运行的是本工作仓库构建
`D:\workSpace\git_clone_test\allo\build.noindex\debug\deps\Flowy.exe`（15:08），
其数据目录是 `D:\tmp\flowy-dev`、工作区 `D:\tmp\777`（即第 3 节）。

### 6.2 关联发现：备份环境正是"qwen3.8-flash 搜索报错"的现场

2026-09-04，备份环境在 `AIPC-qwen3.8-flash` 上集中测试搜索/抓取，出现
**全部搜索 provider 失败**：

```text
03:39:35  all managed web search providers were unavailable  error_class=timeout
03:49:24  all managed web search providers were unavailable  error_class=timeout
03:51:47  all managed web search providers were unavailable  error_class=timeout ×2
03:56:21  all managed web search providers were unavailable  error_class=timeout
03:56:56  all managed web search providers were unavailable  error_class=timeout ×2
```

每次都是 `parallel timeout(3s) → you timeout(3s) → duckduckgo timeout(6s)`，
最终工具返回：

```text
web_search failed: provider error: web search is temporarily unavailable; do not repeat the same search this turn
```

`flowy-backend.db` 中可确认以下会话的 `web_search` `tool_call` 均为 `status: error`：

```text
01a06a7e  帮我搜今天的天气                    （AIPC-qwen3.8-flash）
01a06a88  使用web_fetch帮我查一下巴黎的天气    （AIPC-qwen3.8-flash）
01a06a8a  查询国际新闻                      （AIPC-qwen3.8-flash）
01a06a8f  搜美国国际新闻                    （AIPC-qwen3.8-flash）
01a06a8d  国际新闻 / 搜索国际新闻             （qwen3.8-flash / deepseek-v4-flash-vision-exp）
```

这与用户反馈的"qwen3.8-flash 在使用 web search 工具的数据返回阶段出现过报错"
完全对应：不是模型报错，而是托管搜索在 provider 数据返回阶段全部超时。

同一批会话里 `web_extract` 也大面积失败（wttr.in / open-meteo / news RSS 超时，
reuters 返回 401、bbc 返回 404），模型最后改用本地 `curl` 绕过。
另外 09-01、09-04 也出现了与案例 B 相同的 `you … schema_mismatch`。

### 6.3 备份环境中的其它同族问题

| 现象 | 证据 | 归因 |
| --- | --- | --- |
| `provider stream protocol violation: tool progress 'web_fetch' (call_…) was not advertised in this request` | 会话 `01a05702`（`AIPC-auto-balance`），09-01 08:52、09-03 03:24，UI 报 `USER_LLM_PROVIDER_GATEWAY_ERROR` | 客户端对未广告工具的进度预览直接终止 turn（P1-9） |
| `peer closed connection without sending TLS close_notify` / `operation timed out` | 08-05、08-31、09-04 的 `execute_turn() failed`，UI 报 `USER_LLM_PROVIDER_NETWORK_ERROR` | 传输层抖动；当前无重试（P1-10） |
| `API error 401 … invalid_or_expired_token` | 08-24 会话 `01a03321` | 鉴权过期，属外部 |

### 6.4 与当前环境的关系

- 案例 B 的 `you` 契约漂移在备份环境 09-01/09-04 已出现，说明该问题跨环境、跨构建长期存在；
- 用户记忆中"qwen3.8-flash + web_search 报错"来自备份环境的 09-04 测试，其根因是
  搜索 provider 链整体超时（含 DDG），与 09-15 本仓库环境看到的
  `you schema_mismatch` 是同一系统的两个层面：**契约漂移 + 兜底预算不足**；
- 因此 P0-1…P0-4 与 P1-9/P1-10 需要一起解决，才能同时覆盖两处现象。

## 7. 验证边界

- 本记录创建前没有修改业务代码；P0/P1 均未实施。
- 案例 B 的根因已由在线探针证实；案例 A 的根因由客户端代码路径 + 上游错误语义证实，
  未重新调用真实网关复现。
- 备份环境（第 6 节）的结论来自只读日志与 DB 查询，没有重新运行该构建；
  09-04 的"全 provider 超时"是否由当时本机网络抖动引起，未做链路级取证。
- 反馈日志中的 `claude-fable-5` 缺货、代理层 502 等属于外部可用性问题，本记录不展开。
- 未对托管搜索预算做压测；上述 6.6–8.1 s 数字来自一次会话内两次搜索的实测。

# 上游契约漂移与网关拒绝调查记录

> 状态：只读分析完成；方案已按交叉审查收敛（见第 8 节）；代码改动尚未实施
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

因此在记录的目录配置下（目录声明档位、会话未显式选择 → 默认 `medium`、带工具），
该模型的请求会被上游确定性拒绝，与"服务商宕机"无关。
此结论不泛化到未来目录更新、显式档位选择、无工具请求或其它厂商模型。

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
| 15:44:18 | `maxOutputTokens value of 128000 but the supported range is from 1 (inclusive) to 65537 (exclusive)` | 目录把 gemini-3.5-flash 输出上限配成了 128000（疑似上下文窗口）；重发值必须是 65536 |
| 15:45:42 | `parameters.any_of[0].required: only allowed for OBJECT type` | 该轮没有任何 `provider rejected tool schemas` 重试日志，说明分类器不识别 Gemini 文案；需先补分类器 + 真实 schema fixture（§5.7） |

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

### 3.3 根因：客户端对 you 工具列表的固定形态假设（证据有冲突）

`you` 适配器要求端点**恰好暴露 1 个工具**
（`crates/agent/flowy-web/src/managed/remote.rs:649`）：

```rust
if self.id == SearchProviderId::You && tools.len() != 1 {
    return Err(SearchAttemptError::SchemaMismatch);
}
```

本次调查中，关于"上游当前形态"存在**相互冲突的证据**，必须并列记录：

| 来源 | 时间 | 结果 |
| --- | --- | --- |
| 本仓库在线探针（`initialize` + `tools/list`） | 2026-09-15 15:33、16:18（本地，两次） | `tool_count: 2`：`you-search`、`you-discover` |
| 另一审查 agent 探针 | 2026-09-15 | 连接阶段超时，未取得 `tools/list` |
| You.com 官方文档（审查 agent 引用） | — | 称 free profile 仅暴露 `you-search`；本次直接抓取该页失败，未能独立复核 |

以下两点不受该冲突影响、可确定性成立：

1. 历史故障成立：09-01、09-02、09-04、09-15 均记录到 `you … schema_mismatch`；
2. 只要工具列表与"恰好 1 个"不符，客户端就会禁用 you。
   而新 `you-search` 的 `required=query`、`query:string`、`count:integer`、
   `outputSchema.properties.results` 仍满足现有
   `validate_tool_schema` / `validate_output_schema`（`remote.rs:880`、`remote.rs:926`）。

因此修复不应绑定"当前是 2 个工具"这一快照，而应**按名字发现目标工具**，
并让发现失败可恢复（§5.4）。

### 3.4 失败为什么升级为永久禁用

两层状态叠加：

1. `SearchAttemptError::SchemaMismatch` 写入 `disable_reason`（`managed.rs:219`），
   `availability()` 对 `disable_reason` 的 provider 永远返回 `Disabled`；
2. `ensure_compatible()` 把**失败结果也写入 discovery cache**
   （`remote.rs:640-668`）：即便健康层允许再试，也会立刻返回缓存中的旧错误。

只改健康层（改成冷却）而不处理 discovery cache，provider 仍然无法恢复。
最早的备份环境证据为 09-01（`Nomi-dev\logs\2026-09-01.nomicore.log:5619`），
其后 09-02、09-04、09-15 均有同样记录（09-02：`2026-09-02.nomicore.log:6304-6307`），
说明该问题至少存在两周。

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
| P0-3 | 问题 2（§5.4） | `flowy-web/managed.rs`/`remote.rs` | 失败可恢复：冷却 + discovery cache 不永久缓存失败 |

**观察项（先采数据，暂缓改预算）**

| 项 | 对应问题 | 说明 |
| --- | --- | --- |
| OBS-1 | 问题 3（§5.5） | 采样各 provider 冷/热启动 P50/P95、成功率、fallback 延迟、hedge 取消行为、DDG 限流，再决定串行重排/hedge/预算 |

**第二批（独立提交）**

| 项 | 对应问题 | 改动位置 | 一句话 |
| --- | --- | --- | --- |
| P2-1 | 问题 5（§5.6） | `nomifun-ai-agent`/`nomi-providers` | 解析 inclusive/exclusive 后协商 `max_tokens`（65537 exclusive → 65536） |
| P2-2 | 问题 6（§5.7） | `nomi-providers`/`nomi-config/compat.rs` | 分类器识别 Gemini 文案 + 真实 fixture 后修 schema |

**单独评审（涉及安全边界或超时策略，不混入契约补丁）**

| 项 | 对应问题 | 说明 |
| --- | --- | --- |
| SEP-1 | 问题 8（§5.8） | 未广告工具进度预览的处置属安全/产品契约变更 |
| SEP-2 | 问题 9（§5.9） | 不再叠加重试；先建立请求总 deadline 与超时分类 |

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

**推荐**：分三层，缺一不可：

1. `remote.rs:649` 删除 `tools.len() != 1`，改为按名查找 `you-search` 并校验其 schema
   （`validate_tool_schema` / `validate_output_schema` 已存在）；额外工具不影响；
2. `ensure_compatible()` 的 discovery cache 不再永久缓存失败：
   只永久缓存成功结果，失败结果带时间戳/TTL（或成功前每次重试都重新 `tools/list`）；
3. `managed.rs:219` 的 `SchemaMismatch` / `ToolMissing` 改为"冷却后可重新探测"，
   并补 INFO 日志说明 provider 被跳过及原因；`Unauthorized` 等无法靠重新发现修复的
   仍保持进程级禁用。

**理由**

- 证据冲突（§3.3）说明上游工具列表**不可预测**：写死"1 个"或"2 个"都会再坏，
  只有按名发现 + 可恢复失败才对根因；
- `parallel` 适配器本来就是"按名查找"（`remote.rs:654-666`），you 的计数约束是唯一
  特例，删除可消除不一致；计数约束没有安全价值；
- 三层缺一不可：只改计数约束时，别的 schema 漂移仍会永久禁用；只改健康层冷却时，
  discovery cache 会立刻返回旧错误（§3.4）。

**备选方案与拒绝原因**

| 备选 | 拒绝原因 |
| --- | --- |
| 把期望值从 1 改成 2 | 上游形态有冲突证据，下一次增删工具又会坏 |
| 永久弃用 you（`ddg_only`） | 验收标准是恢复一个真实 provider；弃用会放大问题 3 的全失败概率 |
| 只允许"重启后恢复" | 用户会话内无法自愈；09-01/09-04/09-15 证明故障跨会话存在 |
| 只改健康层冷却（不碰 discovery cache） | 失败结果被永久缓存，冷却后第一次尝试仍立即失败（审查已指出） |
| 运营/服务端改回 1 个工具 | 无权限，且不可控 |

**风险/开放问题**：冷却时长取值（建议 10 分钟）；探测频率上限（避免每次搜索都做
`tools/list`）；是否把"工具数变化"单独记日志以观察 vendor 行为。

### 5.5 问题 3：搜索全 provider 超时（含 DDG）

**推荐**：降级为**观测项，暂不修改预算/串行顺序**。先修复 §5.4 的 you 发现与恢复，
再采集以下数据后再决策：

```text
各 provider 冷启动/热启动 P50、P95（parallel / you / ddg）
成功率、空结果率（succeeded / returned no results / failed）
fallback 总延迟分布（尤其 parallel+you 都失败时）
hedge 若引入：取消后上游请求是否仍完成、是否占用 permit
DDG 立即重试是否触发限流（error_class=rate_limited 计数）
queue_wait_ms 分布（并发搜索下的排队情况）
```

**理由**

- 审查指出：当前没有基线数据，`1.5s` 可能反而杀死正常的 MCP 初始化
  （冷启动 `initialize` + `tools/list` + 调用），风险高于收益证据；
- 09-04 的"全 provider 超时"是否由当时本机网络抖动引起，尚无链路级取证（§7）；
- 先修 you 恢复机制后，parallel/you 的失败结构可能明显改变，预算结论会不同。

**备选方案与拒绝原因**

| 备选 | 拒绝原因 |
| --- | --- |
| 立即把 3s/3s 收紧到 1.5s 并 DDG 重试 | 无冷启动/取消/限流基线，可能杀死正常初始化；审查已要求收敛 |
| 所有预算等比放大（如 10/10/10） | 全挂时单次搜索 30 s，工具延迟爆炸 |
| 超时也按永久禁用处理 | 超时是瞬态的；把唯一工作的 DDG 因网络抖动禁用会制造更大故障 |
| 依赖模型自行重试（现状 `do not repeat`） | 09-04 观测到模型转去 `web_extract`（也超时），浪费更多 turn |
| 搜索改服务端/网关侧 | 不在本仓库可控范围 |

**风险/开放问题**：`web_search` 工具外层是否还有独立超时（目前只看到 managed 层
12 s `TOTAL_BUDGET`）；观测期需要至少覆盖两种网络状态，否则结论不可用。

### 5.6 问题 5：`maxOutputTokens` 超出上游范围

**推荐**：命中 `supported range is from L (inclusive) to U (exclusive)` 时，
**先解析上界与开闭语义再重发**：本例 `U = 65537 exclusive` → 使用 `65536`；
新上限只能收紧、不能增大；provider+model 实例内记忆；最多协商一次；
无法可靠解析时保留原错误，不猜固定值。与 §5.3 共用同一协商框架。

**理由**：错误包含精确边界与开闭语义，按字面解析即可；一次协商长期受益。

**备选方案与拒绝原因**

| 备选 | 拒绝原因 |
| --- | --- |
| 把 65537 直接当上限使用 | `exclusive` 语义下 65537 仍会被拒（审查已指出 off-by-one） |
| 全局把 `max_tokens` 压到固定值（如 32k） | 会砍掉 deepseek 等模型的合法长输出，产品能力回退 |
| 只改云端目录 | 无权限；且新模型/新通道还会漂移 |
| 直接信任 models.dev 输出上限 | Flowy 内建 provider 的目录优先规则已存在，本次错误正来自该路径 |

### 5.7 问题 6：Gemini 嵌套 `any_of` 报错

**推荐**：分两步，先证据后修复：

1. 从失败会话恢复第 11 个工具的真实 schema，做成脱敏测试 fixture；
2. 先让 `ProviderError` 分类器识别 Gemini 文案
   （`parameters.any_of[…]: … only allowed for OBJECT type`），使该错误不再先经历
   通用 500 重试；再做针对性 schema 修复：对包含 `required` 的组合分支补齐正确的
   `type: object`，或在确认语义安全时投影。

**理由**：09-12 该轮没有任何 `provider rejected tool schemas` 重试日志，说明当前分类器
不识别该文案（§2.4）；没有 fixture 无法验证投影不破坏约束，也无法保证修复覆盖真实
结构。现有 sanitizer 主要处理根级组合（`compat.rs:747` 起有测试），嵌套行为未被证明。

**备选方案与拒绝原因**

| 备选 | 拒绝原因 |
| --- | --- |
| 递归删除所有 `anyOf/oneOf` | 会弱化合法参数约束（审查已否决） |
| 从工具集中删除被拒工具 | 直接损失能力，不可取 |
| 全局强制所有 schema 先清洗 | 牺牲正常 provider 的 schema 保真度 |
| 按 Gemini/供应商特判 | 共享协议路径下按供应商分支脆弱，且名单会腐烂 |

**风险/开放问题**：投影会弱化参数约束，需用 fixture 驱动的测试固定行为；
在拿到真实 schema 前不改 sanitizer。

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

### 5.9 问题 9：网关连接中断（TLS EOF / 超时）

**推荐**：**不再新增重试**。分两步：

1. 为初始请求建立/确认**总 deadline**（或超时预算），保证最坏等待时间有明确上限；
2. 在 deadline 前提下评估 timeout 类错误是否需要重试；若需要，必须同时缩短单次预算，
   保证总等待不超过当前上限。

**事实核对（本轮）**：`send_initial()`（`lib.rs:304`）已经包在
`with_initial_request_retry` 中；09-04 03:53:50 存在
`error_kind: http_connect` 的重试日志，证明连接类错误已在重试（最多 2 次）。
08-31/09-04 的 90–231 s 挂起来自上游/网络的长时间无响应，不是缺少重试。

**备选方案与拒绝原因**

| 备选 | 拒绝原因 |
| --- | --- |
| 直接叠加一次 TLS EOF/超时重试 | 与现有初始重试形成叠加；timeout 重试会把 90–120 s 等待进一步放大（审查已否决） |
| 完全不处理 | 长时间挂起体验差；应由总 deadline 解决，而非重试 |
| 按"上下文溢出"处理（compact 后重试） | 与真实原因无关，不解决问题 |

**风险/开放问题**：`reqwest` 对 timeout/connect 的分类需要在改动前用测试确认；
deadline 取值需产品确认可接受的最坏等待。

### 5.10 落地顺序与验证

**第一步：P0-1（effort 协商）单独提交**

- `ProviderError` 增加严格分类器
  `is_tools_with_reasoning_effort_incompatible()`：必须同时命中 tools、
  `reasoning_effort`、unsupported/not supported；
- 在通用 500 重试前排除该错误（`retry.rs:63`）；
- OpenAI 协商循环只对**带工具**请求重发一次，并显式发送 `"none"`；
- provider 实例内记忆；无工具请求仍保留原 effort；不做模型名/供应商硬编码。

测试断言：

```text
首次请求带 medium
第二次带 none
总请求数恰好为 2（不是先烧完 500 重试）
后续工具请求直接使用 none
普通 500 仍保留现有重试策略
```

**第二步：P0-2/P0-3（you 发现与恢复）单独提交**

- 按名查找 `you-search`；只永久缓存成功发现；失败发现加 TTL/冷却后再探测；
- `SchemaMismatch` / `ToolMissing` 冷却后可重试；`Unauthorized` 仍进程级禁用；
- 测试：2 个工具时选中 `you-search`；失败后冷却到期自动恢复；跳过时不误判失败。

**第三步：OBS-1 观测数据采集**（§5.5 指标清单），再决定预算/hedge。

**第四步：P2-1（输出上限，含 inclusive/exclusive 解析）、P2-2（Gemini fixture +
分类器 + schema）**。

**第五步：SEP-1 / SEP-2 单独评审**（§5.8、§5.9）。

**回归与验收**

```text
cargo test -p flowy-web managed::tests
cargo test -p nomi-config test_sanitize_schema_projects
cargo test -p nomi-providers（含新增协商测试，wiremock 参照 openai.rs:3337-3380）
cargo test -p nomi-agent（若 SEP-1 变更）
真实验收：qwen3.8-flash web_search（深圳天气）→ you 恢复为正常 attempt；
        GPT5.6-Sol 带工具对话 → 不再出现 23 s 重试后失败
```

### 5.11 开放问题（需 reviewer 决策）

1. 学习式标记是否需要跨重启持久化到 DB？当前建议进程内（与 `sanitize_tool_schemas`
   一致），避免引入新的持久化契约。
2. you 的 discovery 失败缓存采用"只永久缓存成功 + 冷却后探测"还是 TTL？TTL 取值？
3. `SchemaMismatch` / `ToolMissing` 冷却时长与探测频率（建议 10 分钟 + 每次搜索最多
   一次 re-probe）。
4. 未广告工具进度预览：忽略还是仅记录 warn？该变更属安全边界评审（SEP-1）。
5. timeout 类错误是否需要重试，取决于总 deadline 方案；需产品确认可接受的最坏等待。
6. 是否把 `/v1/responses` 作为推理模型的中期路线（取决于网关侧支持，需单独调研）。

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
| `provider stream protocol violation: tool progress 'web_fetch' (call_…) was not advertised in this request` | 会话 `01a05702`（`AIPC-auto-balance`），09-01 08:52、09-03 03:24，UI 报 `USER_LLM_PROVIDER_GATEWAY_ERROR` | 客户端对未广告工具的进度预览直接终止 turn（单独评审 SEP-1） |
| `peer closed connection without sending TLS close_notify` / `operation timed out` | 08-05、08-31、09-04 的 `execute_turn() failed`，UI 报 `USER_LLM_PROVIDER_NETWORK_ERROR` | 传输层/上游长时间无响应；连接类错误已进入初始重试（09-04 03:53:50 `http_connect`），timeout 类未重试（单独评审 SEP-2） |
| `API error 401 … invalid_or_expired_token` | 08-24 会话 `01a03321` | 鉴权过期，属外部 |

### 6.4 与当前环境的关系

- 案例 B 的 `you` 失败在备份环境 09-01/09-04 已出现，说明该问题跨环境、跨构建长期存在；
- 用户记忆中"qwen3.8-flash + web_search 报错"来自备份环境的 09-04 测试：发现失败不可恢复
  叠加 provider 链超时与串行兜底延迟；与 09-15 本仓库环境看到的 `you schema_mismatch`
  属同一系统问题，不对"预算是否不足"做提前结论（由 §5.5 观测后决策）；
- 修复顺序按 §5.10：先做 effort 协商与 you 发现/恢复，预算问题按 §5.5 观测后决策，
  progress/传输问题走单独评审。

## 7. 验证边界

- 本记录创建前没有修改业务代码；P0/P2/SEP 均未实施。
- 案例 A 的根因由客户端代码路径 + 上游错误语义证实，未重新调用真实网关复现。
- 案例 B 的"上游当前形态"证据冲突（本仓库探针两次均为 2 个工具；审查 agent 探针超时；
  官方文档称 1 个工具且未独立复核），因此修复方案以"按名发现 + 失败可恢复"为准，
  不绑定任一快照。
- 备份环境（第 6 节）的结论来自只读日志与 DB 查询，没有重新运行该构建；
  09-04 的"全 provider 超时"是否由当时本机网络抖动引起，未做链路级取证。
- 审查 agent 本轮只读核查并运行了
  `cargo test -p flowy-web managed::tests`（25/25）、
  `cargo test -p nomi-config test_sanitize_schema_projects`（3/3）与 OpenAI 协商现有测试，
  结果通过；本记录未重复运行。
- 反馈日志中的 `claude-fable-5` 缺货、代理层 502 等属于外部可用性问题，本记录不展开。
- 未对托管搜索预算做压测；6.6–8.1 s 数字来自一次会话内两次搜索的实测（§3.2）。

## 8. 交叉审查回应（2026-09-15）

### 8.1 已采纳的修正

| 审查意见 | 处置 |
| --- | --- |
| "You 当前固定返回两个工具"表述过强，官方文档与探针结果冲突 | 改为并列证据表（§3.3），修复以"按名发现"为准，不绑定工具数量 |
| `maxOutputTokens … 65537 (exclusive)` 应使用 65536，不能按 N 重发 | §2.4 / §5.6 改为解析 inclusive/exclusive 后收紧 |
| TLS EOF 初始请求已重试，不应再叠加 | §5.9 改为"不新增重试"，先建立总 deadline；并补充 09-04 `http_connect` 重试日志证据 |
| 仅冷却不够，discovery cache 会永久缓存失败 | §3.4 / §5.4 增加"只永久缓存成功 / 失败 TTL"要求 |
| 1.5s + hedge + DDG 重试证据不足，应降级为观测项 | §5.5 改为 OBS-1 观测清单；§5.1 索引同步 |
| Gemini `any_of` 根因未证明，分类器不识别该文案 | §2.4 补证据（该轮无 schema 重试日志）；§5.7 改为先取 fixture 再修 |
| progress / 传输问题不应混入契约补丁 | 拆为 SEP-1 / SEP-2 单独评审 |
| 案例 A 结论不应泛化 | §2.2 增加适用范围限定 |

### 8.2 保留不同意见的部分

| 争议点 | 本记录立场 | 依据 |
| --- | --- | --- |
| "当前 you 只有 1 个工具" | 本仓库在 2026-09-15 15:33 与 16:18（本地）两次探针均得到 2 个工具 | 探针原始输出：`tool_count: 2`、`you-search`、`you-discover`；审查 agent 探针连接超时，官方文档未独立复核 |
| `is_connect()` 是否覆盖 TLS EOF | 连接类已验证进入重试（`http_connect` 日志），timeout 类未重试；具体分类在实施前用测试确认 | 09-04 03:53:50 `retrying transient initial provider request failure ... error_kind:"http_connect"` |
| Gemini fixture 是否必需 | 认同必需 | 该轮没有 sanitize 重试日志，无法证明现有投影覆盖真实结构 |

### 8.3 收敛后的工作拆分

```text
提交 1  fix(provider): negotiate tools + reasoning_effort（P0-1）
提交 2  fix(web): name-based you discovery with recoverable failures（P0-2/P0-3）
观测    OBS-1 搜索 provider 基线与 hedge/预算评估（不改代码）
提交 3  fix(provider): negotiate output token upper bound（P2-1）
提交 4  fix(provider): Gemini tool schema compatibility with fixture（P2-2）
评审    SEP-1 未广告工具进度预览的处置（安全/产品契约）
评审    SEP-2 初始请求总 deadline 与 timeout 重试策略
```

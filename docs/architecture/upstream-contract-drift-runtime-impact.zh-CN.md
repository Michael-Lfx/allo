# 上游契约漂移修复：运行时影响说明与 OBS-1 采样记录

> 状态：修复与验收完成；OBS-1 采样完成并给出"不修改调度"结论
>
> 最后维护：2026-09-16
>
> 分支：`fix/upstream-contract-drift`（PR #219；已 rebase 至 `fb0cba264`，
> 提交哈希见 §1）
>
> 关联：[调查记录](upstream-contract-drift-investigation.zh-CN.md) ·
> [执行计划](upstream-contract-drift-execution-plan.zh-CN.md)

## 1. 本分支改了什么（提交索引）

| 提交 | 内容 |
| --- | --- |
| `091853d23` | 工具请求的 `reasoning_effort` 语义协商（按模型记忆） |
| `55c966ac2` | You 按名称发现、失败不缓存、10 分钟冷却、调用期缓存失效 |
| `fe9097b28` | 输出 token 上限协商（`65537 exclusive → 65536`） |
| `ceb3347ef` | Gemini 文案分类 + 组合分支归一化（fixture 驱动） |
| `a9a8920f9` | 未广告 `ToolUseDelta` 忽略；最终未广告 `ToolUse` 仍拒绝 |
| `fa3c8d1c9` | 初始协商 90s 绝对 deadline（chat/completions） |
| `c1f329f3b` | 目录输出上限钳制（同步时按模型家族向下收敛） |
| `5eeed73eb`…`d1a6b3598` | 边界复审修复与收尾（协商上下文、锁等待归类、告警去重、词元匹配、日志清理、边界注释） |

## 2. 对 agent runtime 的影响

总体：**不改 turn 生命周期、工具循环、审批、持久化，也没有 DB/HTTP DTO/配置变更**。
所有学习状态都在进程内、按 provider 实例 + 模型隔离，进程重启后重新协商一次。

| 改动 | runtime 行为变化 | 代价与风险 |
| --- | --- | --- |
| effort 协商（`nomi-providers/openai.rs`） | 带工具请求被网关拒绝后，自动以 `reasoning_effort:"none"` 重发一次并按模型记住（实测 GPT5.6-Sol 从"23.6s 后必然失败"变为"降级后成功"）；记忆生效后带工具请求**一律显式发送 `none`**（即使调用方未配置 effort，覆盖网关服务端默认 reasoning 的场景）；该错误的瞬时重试排除仅对带工具的请求生效 | 每个模型首次多一次失败请求（实测 ~5s）；该模型工具请求的推理档位被上游要求降为 none；一次性 provider（标题/辅助）每次新建实例会各协商一次 |
| You 发现与恢复（`flowy-web`） | `web_search` 自愈：`SchemaMismatch`/`ToolMissing` 仅冷却 10 分钟；discovery 只缓存成功，终态失败清缓存与 peer 缓存；冷却后下一次搜索自动重跑 `tools/list` | 上游真不兼容时每 10 分钟重探一次（有界）；skip 日志从 debug 升为 INFO |
| 输出上限协商（`nomi-providers/openai.rs`） | 命中 `supported range` 拒绝时解析 inclusive/exclusive 并向下收敛重发，按模型记忆；后续只向下钳制 | 模型最大输出可能被学习值调低（预期）；解析失败原样报错不猜值 |
| Gemini schema（`nomi-config/compat.rs`） | 组合分支补 `type: object`、标量分支移除 object-only 关键字；只作用于 provider-facing 副本 | Bedrock/Vertex 等默认 sanitize 通道同样看到归一化（语义保持）；执行期仍用本地原始 schema 校验 |
| 未广告进度预览（`nomi-agent/engine`） | 展示型 `ToolUseDelta` 改为 warn + 忽略；**最终未广告 `ToolUse` 仍硬失败** | 少一类误报错误；工具执行授权边界不变 |
| 90s deadline（`nomi-providers/openai.rs`） | 初始协商（连接/重试/退避/key rotation/各项协商）共享一个绝对 deadline；超时返回 `InitialRequestTimeout`（不可重试）→ `UserLlmProviderTimeout`，开启故障转移时可切模型 | 最坏等待由 90–231s 收敛到 90s；已建立的 SSE 流不受影响；未新增 TLS/timeout 重试 |
| 目录上限钳制（`nomifun-cloud/provider_sync.rs`） | 同步投影 `extra.max_tokens` 时，按模型家族把已知超限值向下钳制（当前仅 gemini → 65536）；首次请求即携带被上游接受的上限，不再白吃一次 `supported range` 拒绝 | 只降不升；家族按整段词元匹配（`notgemini` 不命中），`AIPC-auto-*` 无家族词仍走运行时协商；未知家族/更小值原样保留；本机显式 `output_limit` 列优先级仍高于目录值；表需随目录修正清理 |

运行期最坏新增请求数是**有界的**：usage、schema、effort、输出上限四项扩展各至多协商一次；
学习状态仅在 provider 实例内生效（`AtomicBool` + `Mutex<HashSet/HashMap<String,_>>`）。

## 3. OBS-1 采样（受控在线探针）

### 3.1 方法

新增可复现采样工具（`crates/agent/flowy-web/examples/obs_sampling.rs`）：

```text
cargo run -p flowy-web --example obs_sampling                 # 全链路（默认 20 次）
NOMI_MANAGED_SEARCH_DISABLE_PROVIDERS=parallel \
  OBS_QUERIES=10 cargo run -p flowy-web --example obs_sampling   # 指定通道（dev 构建）
```

工具真实执行 keyless 托管搜索链，把 `managed_search` 结构化事件捕获为 JSONL，
聚合成功率、失败分类、P50/P95、fallback 分布；探针总量 20 + 10 = **30 次**，
未突破额度约束。

### 3.2 样本 A：全链路 20 次（2026-09-15）

```text
ok=20 all_failed=0 empty=0 skipped=0
wall_ms p50=1066 p95=2590
parallel: ok=20 fails={}
fallback_count histogram: {0: 20}
```

结论：本机当前网络下 `parallel` 全部命中且远低于 6s 门槛，无 fallback、无全失败。

### 3.3 样本 B：禁用 parallel 后 10 次（直达 you.com 实时链路）

```text
ok=10 all_failed=0 empty=0 skipped=0
wall_ms p50=1970 p95=2255
you: ok=10 fails={}          # 0 次 schema_mismatch / timeout
```

结论：**修复后的 You 链路在真实端点 10/10 成功**（按名发现 + 真实 `you-search`
解码），无 `schema_mismatch`，证明目标故障已消除；同时给出 you 通道热启动
P50≈1.97s / P95≈2.26s。

### 3.4 决策门判定（执行计划 §6）

| 条件 | 实测 | 判定 |
| --- | --- | --- |
| 搜索 P95 ≤ 6s 且全失败率 ≤ 5% | P95 = 2.59s（A）/ 2.26s（B），全失败 0% | ✅ 保持串行调度 |
| P95 > 6s 且串行等待 ≥50% 且 DDG 3s 成功率 ≥90% | 不满足 | 不实施 staggered hedge |
| 全 provider 同时网络超时 | 未出现 | 不归因外部网络 |
| 主决策样本 ≥100 条修复后真实调用 | **未达到**（本次仅 30 条受控探针） | 标记"证据不足" |

**最终结论：Stage 8（预算收紧/hedge）保持关闭，不修改搜索调度。**
历史日志中 parallel/you 的高超时率未在本机复现（今日并行通道健康），
因此该结论只对"当前网络 + 本次探测"成立；若后续真实使用中再次出现
"全 provider 失败"或串行等待占比升高，按同一工具重新采样后再决策。

## 4. 如何复现

```text
# 单元与集成回归
cargo test -p nomi-providers
cargo test -p flowy-web
cargo test -p nomi-config
cargo test -p nomi-agent

# 手动真实网络验收（默认 #[ignore]）
cargo test -p nomi-providers --test provider_openai_test -- --ignored initial_request_deadline
cargo test -p flowy-web --lib -- --ignored live_you_endpoint

# OBS-1 采样
cargo run -p flowy-web --example obs_sampling
```

真实验收（GPT5.6-Sol 双协商、90s deadline、failover 等）的过程与边界见执行计划 §16。

## 5. 边界复审修复（2026-09-16）

对上述改动做过一轮边界审查，以下修复不改变既有对外语义：

- `retry.rs`：瞬时 500 的排除项改为按请求上下文（是否带工具/输出上限）生效，由
  请求体推导（`InitialRequestContext`）；不带相应扩展的请求保留标准瞬时重试。
- `lib.rs`：schema 分类器补 camelCase `parameters.anyOf` 措辞。
- `openai.rs`：effort 记忆生效后带工具请求显式发送 `none`（无配置 effort 时也发），
  重试不再是逐字节相同的请求；输出上限比较改用实际发送值（日志 `sent`）。
- `flowy-web`：发现互斥锁等待受本次 attempt deadline 约束（`timeout_at`），
  并发首发现不再把健康 provider 记成 Timeout；`Forbidden` 使用具名冷却常量。
- `nomi-agent`：未广告进度告警按调用 id 去重（上限同 `MAX_PROVIDER_TURN_TOOL_CALLS`）。
- `provider_sync.rs`：家族匹配改为整段词元（`notgemini` 不命中）。
- `compat.rs` / `obs_sampling.rs`：深度截断补 debug 日志；采样统计去除死字段并跳过空 request_id。

回归结果：`nomi-providers` 191 lib + 27 集成（1 ignored）、`flowy-web` 190（1 ignored）、
`nomi-agent` lib 787、`nomi-config` 216（1 既有失败）、`nomifun-cloud` provider_sync 22。

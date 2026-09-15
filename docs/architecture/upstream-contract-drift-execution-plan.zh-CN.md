# 上游契约漂移修复执行计划

> 状态：定稿，等待按阶段实施
>
> 最后维护：2026-09-15
>
> 分支：`fix/upstream-contract-drift`（基线 `54f991db2`）
>
> 依据：[upstream-contract-drift-investigation.zh-CN.md](upstream-contract-drift-investigation.zh-CN.md)（第 5、8 节）
>
> 约束：不改 `main`；无 DB migration、HTTP DTO、用户配置或持久化格式变更；学习状态仅进程内。

## 1. 决策基线（已锁定）

1. 计划独立保存为本文件；Stage 0 单独文档提交。
2. SEP-1：未广告 `ToolUseDelta` 结构化告警并忽略；最终未广告 `ToolUse` 继续硬拒绝；
   同步更新 `crates/backend/nomifun-ai-agent/src/manager/nomi/agent.rs` 附近描述该约束的
   注释（只改注释，随 SEP-1 提交）。
3. SEP-2：**仅 `crates/agent/nomi-providers/src/openai.rs` 的 chat/completions**
   初始协商阶段使用 90 秒绝对 deadline；`openai-responses` 有 `previous_response_id`、
   schema 降级等独立状态机，留作后续独立调查。超时仍允许用户已启用的模型故障转移。
4. 输出上限解析使用工作区既有 `regex`；`nomi-providers` 增加 `regex.workspace = true`；
   范围先限 chat/completions。
5. You `SchemaMismatch` / `ToolMissing` 固定 10 分钟冷却，仅由下一次普通搜索触发重探测，
   不增加后台定时器。
6. discovery 只永久缓存成功；不引入第二套失败 TTL。
7. provider 学习状态 = 实例内、按模型（`request.model`）隔离的 map，不持久化；
   重启后重新协商是预期行为。
8. OBS-1 分两组独立采样、独立报告、不混算：
   - 主决策样本 = 修复后真实调用 + 可比历史日志，目标 ≥100；
   - 受控探针 ≤30 次（仅验证冷/热启动、恢复机制、字段可采集性），遵守 You free 日限额；
   - 主样本 <100 → 标记"证据不足"，**不得进入 Stage 8**；
   - OBS-1 在 Stage 2 后启动，与 Stage 4–7 并行，不阻塞修复。
9. 新提交使用英文 Conventional 前缀 + 中文主题；不改写已有 4 个英文文档提交。
10. 提交前校验作者/提交者/trailers，禁止任何 AI attribution；功能提交可独立 revert。

## 2. 提交序列与退出条件

| 阶段 | 提交 | 退出条件 |
| --- | --- | --- |
| 0 | `docs(architecture): 制定上游契约漂移修复计划` | 计划文档 + 调查文档修正已提交；基线测试输出已记录；工作树干净 |
| 1 | `fix(provider): 协商工具请求的 reasoning_effort` | 新增测试全绿；普通 500 重试行为不变 |
| 2 | `fix(web): 按名称发现 You 工具并恢复失败探测` | 冷却/恢复/缓存失效/单飞测试全绿；fallback 无回归 |
| 3 | 无代码（观测报告） | 两组样本分别报告，给出决策门结论 |
| 4 | `fix(provider): 协商模型输出 token 上限` | 65536 与边界用例全绿 |
| 5 | `fix(provider): 兼容 Gemini 嵌套工具 schema` | 两类 fixture 绿；wiremock 总请求数 = 2 |
| 6 | `fix(agent): 忽略未广告工具的纯进度预览` | 新边界测试与注释同步全绿 |
| 7 | `fix(provider): 限制初始请求总等待时间` | 90s 上限与 failover 映射链路测试全绿 |
| 8 | 条件提交 `perf(web): 基于观测优化搜索回退延迟` | 主样本 ≥100 且达阈值；否则记录"不修改" |
| 9 | `docs(architecture): 回填上游契约漂移修复证据` | 全量检查 + 真实验收矩阵 + PR |

每阶段提交后重新报告 committed/staged/unstaged；文档提交不与业务行为混合。

## 3. 阶段 0：计划落盘与基线冻结

1. 写入本计划（含回滚方式与证据边界）。
2. 调查文档修正：
   - §3.4 补引 09-01 证据（`Nomi-dev\logs\2026-09-01.nomicore.log:5619`）后保留"至少两周"；
   - §6.4 改为"发现失败不可恢复 + provider 链超时/串行兜底延迟"，不提前定性预算不足。
3. 记录 `git status --short --branch`、`origin/main...HEAD`、提交身份、hooks 状态。
4. 保存基线输出：

```text
cargo test -p flowy-web managed::tests
cargo test -p nomi-config test_sanitize_schema_projects
cargo test -p nomi-providers
```

**反例**：不在 Stage 0 写 fixture/失败测试——避免跨阶段保留未提交测试改动。

**回滚**：纯文档提交，`git revert` 即可。

### 3.1 Stage 0 基线记录（2026-09-15）

```text
分支          fix/upstream-contract-drift
origin/main..HEAD  4 个文档提交（86bbffe2b..54f991db2）
提交身份      hoye <hoyework@qq.com>
hooks         .githooks/commit-msg, .githooks/pre-push（core.hooksPath 未设置）

cargo test -p nomi-config test_sanitize_schema_projects  → 3 passed; 0 failed
cargo test -p nomi-providers                             → 177 + 13 + 17 + 14 passed; 0 failed
cargo test -p flowy-web managed::tests                   → 25 passed; 0 failed
```

## 4. 阶段 1：P0-1 effort 协商（`nomi-providers`）

### 实现

- `lib.rs`（`is_tool_schema_incompatible` 旁，`:156`）新增
  `is_tools_with_reasoning_effort_incompatible()`：
  - 仅 `ProviderError::Api`；
  - 必须同时命中工具语义词（`function tool`/`function tools`/`tools`）、
    `reasoning_effort`/`reasoning effort`、不支持语义词
    （`not supported`/`unsupported`/`isn't supported`/`not allowed`）；
  - 普通 500、仅提及 reasoning、无工具语境的错误一律不命中。
- `retry.rs:63` 排除该分类，错误直接进入 OpenAI 协商循环，不先消耗瞬时重试。
- `openai.rs`：
  - 实例状态 `learned_effort_none: Mutex<HashSet<String>>`（key = `request.model`）；
  - 循环内单向 `strip_effort_once`；命中且带工具 → 重发并显式发送 `"none"`；
  - 成功后把 model 写入集合；同模型后续工具请求直接 `none`；
  - 无工具请求永不读取该集合；
  - 不尝试省略字段；`none` 仍失败时原样返回上游错误。
- `build_request_body` 增加 `force_effort_none` 参数（`:456` 生效），同步更新其测试调用点
  与 wiremock 测试。
- 日志只记录 model、协商类型、attempt，不记录消息体与工具参数。

### 测试（先红后绿）

wiremock 参照 `openai.rs:3337`：

1. 首次现场 500 → 第二次成功：请求体依次 `medium`、`none`，总请求数恰好为 2；
2. 后续工具请求直接 `none`；
3. 无工具请求仍 `medium`；
4. 另一个模型不继承学习结果；
5. 普通 500 仍保留现有最多两次瞬时重试；
6. 无关错误正文不触发协商；
7. 与 usage/schema 协商串联时每项只协商一次且循环有界。

### 验收

真实 GPT5.6-Sol 完成一次带工具对话；不再出现 23.6 秒无效重试后的
`all_channel_models_failed`；turn attribution header（`X-Flowy-Turn-Id`）保持不变。

### 反例

| 备选 | 拒绝原因 |
| --- | --- |
| 运营侧去掉目录 effort 声明 | 无权限；下一个新模型照样漂移 |
| 全局"带工具就不发 effort" | 砍掉 deepseek 等正常组合，产品能力回退 |
| 按模型名/供应商硬编码 | 名单腐烂，同名自定义模型误伤 |
| 省略字段而不是发 `none` | 上游默认可能仍是推理开启，仍会被拒 |
| 并入一般 500 重试 | 先烧两次无效请求（实测 10s+），永远等不到语义降级 |
| 无记忆、每 turn 重试 | 每个 turn 都变成"先失败再成功"，延迟翻倍 |

### 回滚

单个提交，`git revert <sha>` 即恢复原重试语义。

## 5. 阶段 2：P0-2/P0-3 You 发现与恢复（`flowy-web`）

### 实现

- `remote.rs:649` 删除 `tools.len() != 1`；按名查找 `you-search`，保留输入/输出 schema 校验；
  额外工具不参与调用。
- 发现阶段：**只缓存成功**；失败不写 `discovery`。
- 调用阶段最终规则：

> 调用阶段凡最终返回 `ToolMissing` 或 `SchemaMismatch`，先执行 `clear_compatibility()`；
> 发现阶段失败不缓存，发现成功才缓存。冷却期满后的普通搜索因此必然重新执行 `tools/list`。

  - 第二次 unknown-tool 返回点（`remote.rs:793`）补 `clear_compatibility()`；
  - peer 错误映射为 `SchemaMismatch`（如 `UnsupportedProtocolVersion`，`:992`）的返回路径
    同样清理；
  - **不要**把 `decode_result()` 当 SchemaMismatch 修复点：它只映射为 `MalformedResponse`
    （`:997-1001`）。
- 健康层（`managed.rs`）：`ToolMissing`/`SchemaMismatch` 从 `disable_reason`（`:216-221`）
  改为固定 10 分钟冷却；`Unauthorized`/`RpcMethodUnavailable` 仍进程级禁用；
  `Network/Timeout/Upstream/MalformedResponse` 的指数冷却（`:248-257`）不变。
- 发现单飞沿用 `discovery` mutex，不新增并行抽象。
- 日志：provider、错误分类、冷却截止、是否重新发现、工具总数、是否命中目标工具；
  不记录查询正文与工具参数。

### 测试

1. 只有 `you-search` 时成功；
2. `you-search` 与一个或多个额外工具共存时成功；
3. 目标缺失或 schema 真不兼容 → 进入冷却，且 discovery 不永久缓存失败；
4. `start_paused`（`managed.rs:14` 使用 `tokio::time::Instant`）验证：10 分钟内不重探测；
   到期后下一次搜索重新 `tools/list` 并恢复；无搜索不触发后台网络请求；
5. `Unauthorized` 仍永久禁用；
6. 并发首次发现只执行一次 `tools/list`；
7. **关键回归**：首次发现成功 → 连续两次 unknown-tool → 进入 10 分钟冷却 →
   冷却期满后普通搜索必须再次触发 `tools/list` → 新 schema/tool 恢复后成功；
8. 串行 fallback、12 秒总预算、DDG 可达性、queue-busy 行为不变。

### 验收

qwen3.8-flash 执行真实 `web_search`：You 不再因工具数量变化触发 `schema_mismatch`；
真失败时 DDG 正常兜底；冷却到期后 You 在不重启进程的情况下恢复。

### 反例

| 备选 | 拒绝原因 |
| --- | --- |
| 期望数量改成 2 | 证据冲突（两次探针 2 个工具 vs 官方文档 1 个），下次增删又坏 |
| 弃用 You（`ddg_only`） | 验收目标是恢复 provider；弃用放大 DDG 全失败概率 |
| 只改健康层冷却 | discovery 缓存会立刻返回旧错误，恢复是假的 |
| 失败缓存 TTL + 健康冷却两套时钟 | 计时器互相打架，线上排查困难 |
| 每次搜索都 `tools/list` | 每次多一次网络往返，无退避打上游 |

### 回滚

单个提交；回滚后 You 恢复"永久禁用"旧语义（`disable_reason`）。

## 6. 阶段 3：OBS-1 观测（无代码）

- **主决策样本**：修复后真实调用 + 可比历史日志，目标 ≥100。
- **受控探针**：≤30 次（≥10 冷 / ≥20 热，分批运行、不并发轰击、遵守 You 日限额）。
  冷启动定义 = 进程内该 provider 首次调用。
- 两组分别报告（成功、空结果、timeout、rate-limit、schema mismatch；request/queue/fallback
  P50、P95；全 provider 失败率；DDG 三秒内成功率；串行等待占整体延迟比例），**不混算**。
- 提取配方：

```text
rg 'managed web search (succeeded|returned no results|provider failed|completed with no results|all managed web search providers were unavailable)' <logs>
```

- 决策门（仅用主样本）：
  - P95 ≤ 6 秒且全失败率 ≤ 5% → 保留串行调度，Stage 8 关闭为"无需修改"；
  - P95 > 6 秒、串行等待 ≥ 50% 总延迟、DDG 3 秒内成功率 ≥ 90% → 执行 Stage 8；
  - 主要失败是所有 provider 同时网络超时 → 定性外部网络问题，保留 12 秒总预算；
  - 主样本 < 100 → "证据不足"，不得进入 Stage 8。
- 不新增数据库或永久遥测；原始结果脱敏后写入调查文档。
- OBS-1 在 Stage 2 后启动，与 Stage 4–7 并行，不阻塞。

### 反例

固定 1.5 秒 slot（可能杀死 MCP 冷启动）；直接 hedge（无取消/permit 验证）；
DDG 立即重试（限流）；加新 provider 或改服务端（越界）。

## 7. 阶段 4：P2-1 输出 token 上限协商（chat/completions）

### 实现

- `nomi-providers/Cargo.toml` 增加 `regex.workspace = true`。
- `lib.rs` 增加 `parse_supported_output_range()`：严格解析
  `supported range is from L (inclusive|exclusive) to U (inclusive|exclusive)`；
  `U exclusive → U - 1`，`U inclusive → U`；拒绝零值、u32 溢出、上下界倒置、
  缺失开闭语义。
- 增加 `is_output_limit_rejected()` 分类器，并在一般 500 重试前排除。
- `openai.rs` 协商循环：仅当请求携带输出上限且解析值严格小于当前值时重发一次；
  成功后 `learned_output_caps: Mutex<HashMap<String, u32>>`（key = `request.model`）；
  后续请求发送 `min(请求值, 学习值)`；保持动态 `max_tokens_field`；
  同一请求最多协商一次；解析失败或第二次仍失败时返回上游错误。

### 测试

1. 现场 `[1 inclusive, 65537 exclusive)` 得到 65536；
2. 覆盖 inclusive 上界、u32 边界、零值、溢出、倒置与缺失语义；
3. 首次 500、第二次成功，总请求数为 2；
4. 学习结果只作用于同模型；
5. 当前请求值已小于上游上限时不重发；
6. 普通 500、context overflow、rate-limit 分类不变。

### 验收

真实 gemini-3.5-flash 不再发送 128000；日志显示原值、协商值和模型，不记录请求内容。

### 反例

直接取 65537（exclusive 仍被拒）；手写数字扫描（措辞变化即失效）；全局压 32k
（砍合法长输出）；只改目录或信任 models.dev（无权限且错误正来自该路径）；
每次请求先带小值试探（牺牲正常模型能力）。

## 8. 阶段 5：P2-2 Gemini 嵌套 schema

### 证据与 fixture（先红）

- 日志无法还原线上"第 11 个工具"请求体，fixture 是等价结构而非线上原文，文档必须标明。
- fixture A：最小 root composition（分支含 `required` 但缺少 object type）；
- fixture B：`crates/agent/nomi-tools/src/exec_command.rs:579-601` 的真实嵌套
  `oneOf → not → anyOf → required` 脱敏副本；
- 断言：当前 provider-facing sanitizer 仍产生 Gemini 会拒绝的裸 `required` 分支。

### 实现

- 扩展 tool-schema 错误分类：要求同时出现 parameters/schema 语境与
  `parameters.any_of[...]` 或 `required: only allowed for OBJECT type`；
- 命中后不走一般 500 重试，进入已有 schema 协商；
- `nomi-config/src/compat.rs` 最小修复（仅 provider-facing 副本）：
  - 分支含 `properties`/`required` 但无 `type` → 补 `type: object`；
  - 分支显式非 object 却含 object-only 关键字 → 从副本移除无效 `required/properties`；
  - 保留合法 `anyOf/oneOf`、标量 union、nullable、`$ref`；
  - 本地原始 schema 继续作为执行期校验权威；
- 不全局预清洗、不删除工具、不按模型名特判。

### 测试

1. 两类 fixture 修复前红、修复后无裸 object-only 分支；
2. 合法 nullable、标量 union、根级 projection、`$ref` 测试保持通过；
3. wiremock 返回现场 Gemini 500：第二次请求携带修复 schema，总请求数为 2；
4. 构造违反原始本地 schema 的工具调用，证明执行仍被拒绝。

### 验收

使用真实网关完成一次原失败工具集合调用；若账号或上游环境不可用，只能声明本地
fixture 与 wiremock 通过，不能声称线上恢复。

### 反例

递归删除所有组合关键字（弱化合法约束）；删除被拒工具（损失能力）；全局强制清洗
（牺牲正常 provider 保真度）；只扩分类器不改 sanitizer（从无效重试变成清洗后仍被拒）。

## 9. 阶段 6：SEP-1 未广告工具进度预览

### 实现

- `crates/agent/nomi-agent/src/engine/mod.rs:2218-2223`：未广告 `ToolUseDelta`
  → 结构化 warn + `continue`：
  - 不写 preview map；
  - 不发布 Running 工具卡；
  - 不执行；
  - 不终止当前模型回合。
- 日志包含 model、tool、call id、`ignored_unadvertised_progress`，不含参数值。
- 最终未广告 `ToolUse` 仍在 provider commit boundary 硬失败（`mod.rs:2108`），
  执行期 authority 检查继续保留（`tool_execution.rs:482`）。
- 空 ID、空名称、名称前后空格、已广告工具同 ID 改名、预览数量上限仍硬失败。
- 同步更新 `crates/backend/nomifun-ai-agent/src/manager/nomi/agent.rs` 附近描述该约束的
  注释（只改注释）。

### 测试

1. 未广告 delta 后正常文本和 Done：回合成功、无工具卡、无执行；
2. 未广告 delta 后最终未广告 ToolUse：回合失败且执行次数为零；
3. 已广告 delta → ToolUse 正常流程不变；
4. malformed delta 与已广告 preview 身份漂移仍失败；
5. 重命名旧测试（`engine/set_config_tests.rs:1827`），明确新安全边界。

### 反例

保持整轮失败（展示事件杀掉用户回合）；自动加入允许集（突破工具权限 ceiling）；
连通最终未广告 ToolUse 也忽略（打开执行授权）；预览也发 Running 卡（误导用户）。

## 10. 阶段 7：SEP-2 初始协商 90 秒绝对 deadline（chat/completions）

### 实现

- `nomi-providers/src/lib.rs`：
  - 常量 `INITIAL_REQUEST_DEADLINE = Duration::from_secs(90)`；
  - 抽出可测的 `send_with_deadline(deadline, f)`（`tokio::time::timeout_at` +
    `Elapsed → ProviderError::InitialRequestTimeout`）；
  - 新错误类型 Display 明确含 `timeout`；`is_retryable()` 返回 false。
- `openai.rs`：进入协商循环前创建**唯一**绝对 deadline；循环内所有
  `send_initial_with_key_rotation`（`:896`）调用经 `send_with_deadline`，
  覆盖 TCP/TLS 连接、内部重试、退避、key rotation 与各项兼容协商；
  每次尝试**不**重新获得 90 秒。
- 保留 30 秒 connect timeout 与 120 秒 idle-read timeout（`lib.rs:480/487`）；
  deadline 到期取消请求与退避 future；不新增 TLS EOF 或普通 timeout 重试。
- 范围限 `openai.rs`；`openai-responses`、`anthropic`、`bedrock`、`vertex` 不变。

### 错误映射链路（验收必须覆盖完整链路）

```text
InitialRequestTimeout
  → AgentError::Provider（engine/mod.rs:4157-4158）
  → AppError / send-error 分类（send_error.rs:644-647）
  → UserLlmProviderTimeout
  → 用户已启用的模型故障转移（model_failover.rs:62-83 / failover_seam.rs:594）
```

### 测试

1. 暂停时间验证多次失败、退避与兼容协商共享同一 90 秒 deadline
   （证明不出现 `90s × 尝试次数`）；
2. provider 层不对初始请求进行重试；
3. 快速 500 仍按现有次数重试；
4. 单模型且 failover 关闭：向用户返回明确 timeout；
5. failover 开启：允许切换下一模型；
6. 初始响应成功后，已建立的 SSE 流读取不受初始 deadline 截断。

### 反例

每次尝试各自 90 秒（总等待爆炸）；叠加 TLS/EOF 重试（放大 90–231 秒挂起）；
只调大 `read_timeout`（不覆盖"已发出未响应"整段，也不能防重放）；
依赖用户手动重发（90 秒后用户往往已放弃）。

## 11. 阶段 8：条件性 staggered hedge

仅在阶段 3 主样本达标且满足阈值时提交：`perf(web): 基于观测优化搜索回退延迟`

- 保持 12 秒总 deadline 与现有 provider 健康状态；
- Parallel 在 t=0 启动；1.5 秒仍无非空结果时启动 You；3 秒仍无非空结果时启动 DDG；
- provider 明确失败或返回空结果时立即启动下一个 eligible provider，不等待 stagger；
- 首个非空成功结果获胜，取消其余 future 并释放 semaphore permit；
- 冷却/禁用 provider 不占 stagger 时间；
- 所有成功结果均为空时才返回合法空搜索；
- 不对同一 provider 做内部重复请求，不改变健康计数语义。

测试覆盖：启动时点、首个非空结果获胜、合法空结果、12 秒硬上限、取消释放 permit、
queue busy、健康 skip、全失败。未触发则不产生代码提交，并记录"不修改调度"的证据。

### 反例

固定 1.5 秒 slot 不 hedge（最坏等待不变）；全部并行（限流与并发语义复杂）；
不取消 loser（permit 泄漏）；空结果当成功（吞掉真实结果）。

## 12. 阶段 9：集成验证、文档回填与交付

### 验证

- 各阶段先跑对应 crate 聚焦测试；
- 最终执行：

```text
cargo fmt
cargo test -p flowy-web
cargo test -p nomi-providers
cargo test -p nomi-config
cargo test -p nomi-agent
cargo check --workspace
bun run check
```

- 格式化后重新检查 diff，防止带入无关重写。

### 真实验收矩阵

| 场景 | 预期 |
| --- | --- |
| GPT5.6-Sol 带工具对话 | effort 协商到 `none` 后成功，无 23.6s 后 `all_channel_models_failed` |
| qwen3.8-flash `web_search` | You 按名发现成功；真失败时 DDG fallback；冷却后自愈 |
| gemini-3.5-flash | 输出上限 65536；嵌套 schema 不再被拒（需真实网关） |
| progress | 未广告 delta 不终止回合；最终未广告 ToolUse 仍失败 |
| timeout | 初始协商整体不超过 90 秒；failover 开启时切换备用模型 |
| OBS | 记录是否触发 hedge 及触发证据 |

### Git 与 PR

- 每阶段提交后分别报告 committed、staged、unstaged，并重新确认工作树；
- 不改写已有四个英文文档提交；新提交使用英文前缀和中文主题；
- 提交前检查作者、提交者和 trailers，禁止 AI attribution；
- PR 描述覆盖完整 `origin/main...HEAD`，分别陈述：修改内容与根因、单元测试、
  workspace 检查、真实 provider 验收、未完成或受外部环境限制的项目；
- 各功能提交必须可独立 revert；文档提交不与业务行为混合。

## 13. 风险登记

1. Stage 2 的调用阶段 `clear_compatibility` 是恢复是否真实成立的关键
   （stale success cache 会导致"冷却—再失败"活锁）。
2. `build_request_body` 参数变更影响多处测试调用点；改动后先跑
   `cargo test -p nomi-providers` 全量确认。
3. regex 解析失败必须原样返回错误，不猜值。
4. deadline 必须在协商循环外创建，避免"每次尝试重新计时"。
5. aux/标题等一次性 provider 每次调用新建实例，会各自协商一次——已知成本，
   写入文档，不视为缺陷。
6. `InitialRequestTimeout → UserLlmProviderTimeout` 会触发用户已启用的模型故障转移，
   属预期（决策基线 3）。

## 14. 下一步

1. 完成 Stage 0（本计划 + 调查文档修正 + 基线记录）并提交；
2. 暂停复核 Stage 0 结果；
3. 复核通过后依次实施 Stage 1、Stage 2 两个 P0 提交；
4. Stage 2 提交后启动 OBS-1，与 Stage 4–7 并行。

## 15. 实施记录（2026-09-15）

### 15.1 提交序列

| 阶段 | 提交 | 说明 |
| --- | --- | --- |
| 0 | `d148d4020` | 本计划 + 调查文档修正 + 基线记录 |
| 1 | `854809a75` | effort 语义协商与按模型记忆 |
| 2 | `0cffa50be` | You 按名发现、失败不缓存、10 分钟冷却、调用期缓存失效 |
| 3 | `14f46b1a1` | OBS-1 历史基线（102 成功 / 7 全失败） |
| 4 | `f05aeba7e` | 输出上限协商（65537 exclusive → 65536） |
| 5 | `48e4624df` | Gemini 文案分类 + 组合分支归一化（fixture 驱动） |
| 6 | `28e47613a` | 未广告 ToolUseDelta 忽略；最终未广告 ToolUse 仍拒绝 |
| 7 | `d3dfdbfdb` | 90s 初始协商绝对 deadline + 超时映射 |
| 8 | 未提交 | OBS-1 主样本 <100，按计划保持关闭并记录"不修改" |
| 9 | `173edda48` / `bcc752921` / `b6f2de954` | 证据回填；补齐边界回归测试（溢出、动态字段、向下钳制、attribution、本地 schema 不变）；工具链与基线限制说明 |
| 9+ | `cacb18ba3` / `d6f89fc97` | 超时故障的端到端模型转移；缓存失效后的并发重发现单飞 |

补充说明：原列的"违规调用仍由本地 schema 拒绝"无需新增缺口修复——它由两层既有/新增
锁共同覆盖：`nomi-tools` 的 union/root-object 严格校验测试（如
`prepare_input_keeps_strict_union_and_root_object_boundaries`）保持有效，且新测试断言
provider-facing sanitize 不回写本地原始 schema（`bcc752921`）。

### 15.2 验证结果

```text
cargo test -p nomi-providers                 → 187 + 13 + 17 + 26 passed; 0 failed
cargo test -p flowy-web                      → 189 passed; 0 failed
cargo test -p nomi-agent                     → 787 + 11 + 32 passed；1 既有失败
cargo test -p nomi-config                    → 216 passed；1 既有失败
cargo test -p nomifun-ai-agent --lib protocol::send_error → 33 passed
cargo test -p nomifun-conversation --lib service_test::failover → 9 passed
cargo check -p nomifun-cloud                 → Finished（无新增告警）
```

既有失败（已在基线复现，与本次改动无关）：

- `nomi-config hooks::tests::test_hook_timeout`：Windows 无 `sleep` 可执行文件，
  hook 立即失败而非超时（在未含本次改动的工作树上同样失败）。
- `nomi-agent badcase_regression_test::a_round_that_keeps_truncating_stops_at_three_passes`：
  在本分支 HEAD~（未含本次改动）上同样失败。

工具链限制：

- `rustfmt.toml` 设置了 `disable_all_formatting = true`，`cargo fmt` 是本仓库的既定
  no-op；本次按现有风格手写，未启用全仓格式化。
- `cargo check --workspace` 在本机 40 分钟未完成（编译量大），改为按依赖面核对：
  `ProviderError` 的全部消费方（nomi-agent/nomifun-ai-agent 已由测试覆盖，
  nomifun-cloud 已 check 通过；其余仅引用类型、无穷尽匹配）。
- `bun run check` 在既有 UI typecheck 阶段失败（`videoCanvas`、
  `analytics/*.test.ts`），本分支 `origin/main...HEAD` 未触及任何 UI 文件，
  属基线问题。

### 15.3 尚未完成（需真实环境或后续决策）

- 真实验收：GPT5.6-Sol（effort）、qwen3.8-flash（You 恢复/冷却自愈）、
  gemini-3.5-flash（65536 + 嵌套 schema）、90s deadline 与 failover —— 均需真实网关
  与账号，本轮未执行。
- OBS-1 主样本仍需修复后真实调用累计到 100 条，才能决定 Stage 8。
- PR 尚未创建（分支 `fix/upstream-contract-drift` 未推送）。


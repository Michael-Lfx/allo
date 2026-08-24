# Managed Web Search & Fetch Evolution

Date: 2026-07-31
最后维护：2026-08-24（元数据维护，未重写结论；核对基准 commit `d791691c6`）

这份文档是 Flowy `web_search` / `web_extract` 功能演变的权威索引和避坑记录。
以后涉及 Search Provider、MCP Fetch、本地 Extract、Host 组合、预算、隐私或
生命周期时，先读这里，再按索引打开对应细节文档。

## 读者

- 需要理解当前 Search/Fetch 架构的人。
- 要修改 Managed Search 路由、Parallel MCP、web_fetch fallback 的人。
- 要判断一个 provider 是否可接入、是否要启用 Desktop Managed Extract 的人。

## 当前模型可见面

模型始终只看到两个工具：

```text
web_search(query, count)
web_extract(urls)
```

任何 Provider、MCP Tool、fallback、`final_url`、Session ID、Usage 字段和
provider provenance 都不得进入模型上下文。

## 演变时间线

### 2026-07-09 / 2026-07-10：原始 Search / Extract 设计

- 设计文档：
  - `docs/superpowers/plans/2026-07-09-flowy-web-search-extract.md`
  - `docs/superpowers/specs/2026-07-09-flowy-web-search-extract-design.md`
  - `docs/superpowers/plans/2026-07-10-flowy-web-article-extract.md`
  - `docs/superpowers/specs/2026-07-10-flowy-web-article-extract-design.md`
- 确立了稳定工具契约、可替换 SearchProvider、独立 ExtractProvider、Browser
  只处理交互/登录/动态页的三层原则。

### 2026-07-29：免费搜索服务研究

- 文档：`docs/superpowers/specs/2026-07-29-free-search-services-research.md`
- 结论：没有找到“免 Key、多用户生产分发、有 SLA、大陆/境外都可用”的通用免费
  Search API。
- 教训：不要把某个免费端点当成稳定生产 API；要保留 fallback、限流、熔断和
  显式 provider 选择。

### 2026-07-30：Desktop Managed Search

- 初始实现：`feat: add desktop managed web search`。
- 初始路由：

```text
Parallel -> Exa -> DuckDuckGo
```

- 后来 Exa 的匿名免费 MCP 频繁出现 HTTP 429，不符合无 Key、多用户分发的约束，
  因此从生产链移除。
- You.com Free MCP 通过准入后，路由改为：

```text
Parallel -> You.com Free -> DuckDuckGo
```

- 当前 Search 架构记录：
  - `docs/architecture/web/managed-web-search.md`
  - `docs/reference/web-search-provider-matrix.md`
  - `docs/superpowers/plans/2026-07-30-managed-web-search-you-rollout.md`

### 2026-07-30：MCP Fetch 证据

- 确认 Parallel `web_fetch` 是独立能力，不能作为模型可见工具暴露。
- 确定方向：普通 HTML 继续本地 `HttpExtractProvider`，只有 PDF、JS 空壳、
  不支持文档、本地空正文或符合条件的网络失败才考虑远程 fallback。

### 2026-07-31：Parallel web_fetch Probe 与实现

- Probe：
  - `crates/agent/flowy-web/examples/parallel_web_fetch_probe.rs`
  - 文档：`docs/continuity/decisions/2026-07-31-parallel-web-fetch-admission.md`
- 生产策略：
  - `docs/architecture/web/managed-web-fetch-policy.md`
- 实现路径：
  1. 保留本地响应元数据
  2. 本地结果分类与远程准入
  3. 批量 ExtractCoordinator
  4. Search/Fetch 共享 Parallel MCP Transport
  5. Typed Parallel Fetch Adapter
  6. Local-first ManagedExtractCoordinator
  7. AgentBootstrap Host Binding
  8. Desktop Host Composition；2026-08-02 起 unset 默认进入 EvidenceBacked，
     `NOMIFUN_MANAGED_FETCH_MODE=off` 保留 Local-only 回滚
  9. 唯一 shutdown owner
  10. Fetch 诊断与 endpoint health
  11. 策略文档
  12. 启用前 Review 修复：URL 归属、fragment 出站、结构化失败分类、
      access challenge、source completeness、Peer generation readiness、
      共享 endpoint health / 独立 fetch tool health、timeout 与 local/final 指标
- 历史实现最初来自 `feat/managed-web-fetch`；当前收口分支为
  `feat/fetch-optimization`，以 EvidenceBacked Desktop 验收为准。

## 当前 Search 实现结果

- Desktop Host 才构造 `ManagedSearchService`。
- Web Host 和 Nomi CLI 保持 DuckDuckGo-only。
- 路由：

```text
Parallel -> You.com Free -> DuckDuckGo
```

- 模型只看到 `web_search(query, count)`。
- Parallel / You.com 是私有 managed MCP adapter，不进用户 MCP 配置、Skills、
  ToolSearch 或模型工具目录。
- MCP 协议只接受 `2025-11-25`。
- Parallel Decoder / You Decoder 使用严格 typed 解码，不再使用递归 URL 猜测。
- 健康策略：
  - 401、RPC method unavailable、ToolMissing、SchemaMismatch 可禁用 provider。
  - 429 使用 `Retry-After` 或保守 cooldown。
  - QueueBusy 是请求局部状态，不污染 provider health。
- 输出仍受 12,000 字符 Search 预算约束。

## 当前 Extract 实现结果

### Local-only 路径

- `HttpExtractProvider` 负责本地 HTTP。
- 保留 SSRF、DNS pinning、最多 5 次 redirect、2 MiB body cap、Readability、
  每页 3,000 字符、Tool 8,000 字符。
- `WebExtractTool` 通过 `ExtractCoordinator` 执行：
  - 最多 3 个 URL
  - 并发 2
  - 每 URL 8 秒 deadline
  - Tool 12 秒绝对 deadline
  - 输入顺序保持
  - 取消即 drop，无 detached task
- 模型输出带 untrusted evidence 前导，不暴露 provider/extractor。

### Managed fallback 路径

- 仅 Desktop 的 `ManagedWebHandle` 可能注入 `ManagedExtractCoordinator`。
- 当前 Desktop `AppHostCapabilities` 通过显式 `ManagedExtractMode` 选择模式；
  unset 默认是 EvidenceBacked，`off` 或非法值 fail-closed 为 Disabled。
- Web Host 与 Nomi CLI 仍不构造 Managed Extract；本节的默认启用只适用于
  Desktop Host。
- 执行原则：

```text
Local-first
-> Remote once
-> Final
```

- 本地成功永不远程。
- 每个 batch 最多一次 remote stage。
- 远程只发送 `urls[]` 和 `full_content=false`。
- 不发送 `objective`、`search_queries`、`session_id`、`model_name`。
- 敏感 URL 本地允许，远程禁止。
- 出站 URL 剥离普通 fragment；敏感 fragment 远程禁止。
- 远程结果只按 canonical requested URL 匹配，任何 index/position 兜底禁止；
  一个结果可 fan-out 到重复原始 index。
- 远程失败恢复原有本地错误，不向模型暴露 Parallel/MCP/provider 细节。
- `WebError::Parse` 不进入远程；HTTP 状态优先来自结构化 diagnostics。
- 200 CAPTCHA/WAF、登录页、付费墙不进入远程。
- excerpts 标记 `source_truncated=true`，和 Allo 的 context 截断分开。
- Fetch readiness 基于 `RemoteMcpPeer` generation，不再使用历史成功布尔。
- Search/Fetch 共享 transport endpoint health；Fetch tool-level 错误独立冷却。
- 全局 Fetch 并发通过共享 `Semaphore::new(1)` 限制。
- 远程预算不足时不远程。
- Shared Parallel Peer 只由进程级 `ManagedWebHandle` shutdown 一次。
- `RemoteMcpPeer::discover_tools` 在并发下只发一次 `tools/list`；generation
  变化、SessionExpired 和 Unknown Tool 都会触发兼容性重新验证。
- 日志区分 timeout 分类、fallback/forbidden reason、source/context truncated。
- 取消边界覆盖：等待 fetch semaphore 时取消不发出 `tools/call`，在途 MCP call
  取消不残留 detached task；两个并发 adapter 共享同一个全局 Fetch permit。

### 已完成限定验收与后续边界

Desktop 已完成一次限定的 EvidenceBacked 真实验收；以下条件构成持续门禁，
而不是关闭默认能力的当前状态：

- 普通 HTML 只走本地
- PDF / JS 远程有实际增益
- 403 / 429 不远程
- 敏感 URL 本地可读且远程禁止
- 单页 ≤ 3,000 字符
- ToolResult ≤ 8,000 字符
- 抓包确认只发送经过准入的 URL
- 性能 P50/P95 达到内部目标

### 2026-08-04：模型工具路由收口

- 新增模型可见的 `web_extract` 能力说明：公开 HTML、直链 PDF、JavaScript Shell、
  短页/空页均先交给现有读取工具；不暴露 Parallel、MCP 或 Provider。
- 已知公开直链 URL 不应先搜索；`web_search` 只用于发现 URL。
- Browser 保持交互用途；Bash/Python/`exec_command` 只用于本地 artifact 或
  `web_extract` 明确失败后的补救。用户不需要知道内部工具名称。
- 验收不能只看模型文本或工具注册日志：PDF/JS 需要首个实际读取动作为
  `web_extract`，再由 `remote_attempted=true` 与正的成功计数证明受控 Remote
  fallback；HTML 则应 Local 成功且零 Remote。

## 关键决策记录

### Search Provider

1. 保留 `web_search` 稳定契约。
2. Exa 从生产链移除，原因是匿名免费 MCP 429 且无生产额度保证。
3. You.com Free 只使用 `query` 和 `count`，不发送 livecrawl/contents。
4. DuckDuckGo 是 best-effort fallback，不是正式 API。
5. 不做聚合、hedged request、LLM rerank、查询缓存。

### MCP Fetch

1. `web_fetch` 不是模型工具，是 `web_extract` 私有 fallback。
2. 第一版只允许本地失败后进入一次远程阶段。
3. 只发送 URL，不发送用户 Query 或稳定会话标识。
4. 远程结果先读 `structuredContent.results/errors`，失败再读 text JSON 副本。
5. 远程结果只按 canonical requested URL 映射；禁止 index/position 兜底；
   `final_url` 只用于内部诊断。
6. 远程响应独立设置原始 body 上限；模型预算仍按 3,000/8,000 执行。
7. 敏感 URL 只禁止远程发送，不禁止本地读取；fragment 中的敏感键同样禁止。
8. Parse 和 200 access challenge 页面不进入远程。
9. 远程内容若来自 excerpts，则标记 source_truncated。
10. Desktop unset 默认使用 EvidenceBacked；`off` 或非法值 fail-closed 为
    Local-only。正式 15+ URL Admission 与更多类别仍需单独批准。

## 避坑清单

以下是从研究、Probe、实现和测试中得到的经验，后续改动务必先检查这里。

### Search

- **不要把免费 provider 当 SLA。**
  Parallel、You.com、DDG 都可能间歇失败。
- **Exa 的 429 是官方匿名额度行为，不是偶发故障。**
  不要只凭少量样本判断它可用。
- **You.com 官方示例和实际响应可能不一致。**
  文档描述结构化 JSON，实际可能返回 labelled text；Decoder 必须支持验证过的
  多个兼容形态。
- **不要递归猜测任意 JSON 里的 URL。**
  必须使用 provider-specific typed decoder，否则 schema drift 会静默污染结果。
- **QueueBusy 不能冷却 provider。**
  否则并发压力会误伤健康 provider。
- **不要发送 `session_id` / `model_name`。**
  Parallel 和 You 文档可能提供这些字段，但 Allo 不做稳定会话/模型关联。

### Fetch

- **Parallel `web_fetch` 的响应可以超过 1 MiB。**
  中文文章和长 RFC 页面实测约 1.02-1.09 MiB；共享 Peer 的 body cap 已提升到
  2 MiB。
- **SSE 单事件 cap 仍是 512 KiB。**
  如果 Parallel 改走 SSE 且单事件较大，仍会失败；启用前要同步评估。
- **不要只用“Markdown < 400 字符”判断 JS 空壳。**
  要结合 root/app 容器、script 占比、enable JavaScript 文案，并避免误判短静态页。
- **HTTP 失败诊断不能丢失 status / Content-Type。**
  `extract_with_metadata` 已改为保留 `http_status`、`content_type` 和
  `body_truncated`。
- **HTTP 分类必须优先使用结构化 status。**
  不要从错误文案反推 404/403；`WebError::Parse` 是解析失败，不是 network，
  不得进入远程。
- **敏感 URL 语义是“本地允许、远程禁止”。**
  不要把敏感 URL 误写成“本地也禁止”。
- **不要把原始 requested_url 当出站 URL。**
  Parallel 只接收剥离普通 fragment 后的 outbound_url；敏感 fragment 必须整体
  禁止远程。
- **远程批量结果不保证 index 顺序。**
  按 canonical requested URL 映射；禁止数量相等时按 index 配对；重复 URL 去重
  后要 fan-out 回所有原始 index。
- **200 页面也可能是访问挑战。**
  CAPTCHA/WAF、登录表单、付费墙要结合 body 信号和“无有效正文”判断，不能只看
  一个 `login` 或 `subscribe` 单词。
- **excerpts 不是完整正文。**
  `full_content=false` 时，远程结果默认应视为 source_truncated；不能把摘录标记
  成完整内容。
- **历史成功过不等于当前 Peer warm。**
  readiness 必须读取 `RemoteMcpPeer` 的 generation/initialized/tools_cached；
  Fetch compatibility 必须绑定 generation，SessionExpired 后重新验证。
- **Fetch Tool 级错误不能关停 Parallel Search。**
  `is_error` Upstream、decoder malformed、fetch call timeout 走 Fetch-specific
  cooldown；只有 401/403/429/network/protocol 才写入共享 endpoint health。
- **远程 `errors[]` 是部分失败的重要信号。**
  不要只统计 `results[]`，否则 403/404 会被误判为远程成功。
- **远程 fallback 失败后必须展示原本地错误。**
  模型不得看到 Parallel failed、MCP error、provider timeout 等细节。
- **全局 Fetch 并发必须为 1。**
  匿名额度不稳定，跨 Conversation 必须共享 semaphore，而不是每个 Tool 单独放行。
- **Shared Peer 只能有一个 shutdown owner。**
  `ManagedWebHandle` 负责 exactly-once shutdown；重复 shutdown 幂等。
- **日志不能记录 URL、Query、标题、正文、Conversation ID 或 raw MCP payload。**
  只能记录 index、计数、耗时、错误分类和 fallback reason。
- **不要把模型路由提示当成出站权限。** `web_extract` 被选择后仍必须经过
  Local-first、profile/capability、预算、URL 安全和来源契约；提示不能让 Deferred
  或 Forbidden 类别出站。
- **不要要求真实用户在提示词中点名内部工具。** 维护验收应使用自然的“读取/概括
  此 URL”请求，再检查首个实际 tool use 和脱敏 counters。

### 仓库与门禁

- 当前 `bun run check` 的 `check:i18n` 和 `check:agent-vocabulary` 在基线已有
  失败，和 Search/Fetch 分支无关；不要把它们误判成本次功能回归。
- `.github/workflows/` 永远不能新增 YAML workflow。
- 本机缺少 `sccache.exe` 时，测试可用
  `CARGO_BUILD_RUSTC_WRAPPER='' --config 'build.rustc-wrapper=""'`，不要提交该
  环境配置。

## 文档索引

### 当前权威文档

| 文档 | 内容 |
| --- | --- |
| `docs/architecture/web/managed-web-search.md` | 当前 Search 架构、Host ownership、路由、协议、健康策略 |
| `docs/reference/web-search-provider-matrix.md` | Provider Probe 历史与准入记录 |
| `docs/superpowers/plans/2026-07-30-managed-web-search-you-rollout.md` | You.com 替换 Exa 的 rollout 与 MCP Peer 不变量 |
| `docs/continuity/decisions/2026-07-31-parallel-web-fetch-admission.md` | Parallel `web_fetch` 真实 Probe 结论 |
| `docs/architecture/web/managed-web-fetch-policy.md` | Local-first Managed Extract 生产策略 |
| `docs/architecture/web/managed-web-fetch-provider-maintenance.md` | Provider seam、模型工具契约与维护验收 |
| `docs/architecture/web/managed-fetch-evaluation.md` | 评测证据边界与可恢复实验操作 |
| 本文档 | 演变时间线、实现现状、避坑清单、文档入口 |

### 历史研究 / 规划文档

以下文档保留为历史证据，不代表当前实现状态：

| 文档 | 状态 |
| --- | --- |
| `docs/superpowers/specs/2026-07-29-free-search-services-research.md` | 历史调研：免费搜索服务、大陆访问、Agent 搜索架构 |
| `docs/superpowers/plans/2026-07-30-managed-web-search-you-rollout.md` | 历史 rollout：You.com 替换 Exa、MCP Peer 不变量、typed decoder |

## 后续待完成

- 正式 15+ URL Admission，以及在更多真实语料上复核当前有限的公开 Canary 证据。
- 将 Network、Timeout、Unsupported Document 等 Deferred 类别纳入新的准入评审；本分支
  不扩大 Remote 范围。
- 如 Parallel 返回 SSE，重新评估 `MAX_SSE_EVENT_BYTES`。
- 维护 `bun run check` 的仓库级基线（若环境导致失败，须与本功能结果分开记录）。
- 继续按 `managed-web-fetch-policy.md` 维护回滚和降级策略；Desktop 已完成 2026-08-02
  的限定验收和 2026-08-04 的冷启动/模型工具路由验收。提交 PR 前仍要以实时
  `origin/main` 为基线 rebase 并重跑受影响门禁；
  `NOMIFUN_MANAGED_FETCH_MODE=off` 仍可立即回滚到 Local-only。

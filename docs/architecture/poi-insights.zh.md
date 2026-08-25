# 兴趣点与洞察域（POI / Insights）

> **最后维护：** 2026-08-24 · 核对基准：commit `d791691c6` ·
> 文档性质：现行架构文档（新建，基于源码逐项核对）

两个"会话结束后静默运行"的旁路分析域：POI 提炼本地用户兴趣话题，Insights
把去标识化的工作包贡献回云端。两者共享同一触发点和同一个旁路 LLM 客户端，
但数据边界完全不同：POI 纯本地，Insights 显式同意后上行。

## POI（用户兴趣话题）

- 引擎 [`nomi-poi`](../../crates/agent/nomi-poi/)：Extract → Compare → Update
  流水线，按会话缓冲、**session-end 才提交**（`ingest.rs` 的
  `spawn_session_end_ingest`）。存储是独立 SQLite
  `{data_dir}/interest.db`（`InterestStore`），实体为
  `InterestTopic` / `Signal` / `Starter`；含声明式 + 上下文式抽取、LLM 会话转写
  信号抽取、质量过滤、starter 生成和 `DOMAIN_TAXONOMY`。
  同时以 `MemoryProviderPlugin`（`InterestMemoryPlugin`）接入记忆提供方体系。
  总开关：`nomi_config::InterestConfig.enabled`。
- 后端 [`nomifun-poi`](../../crates/backend/nomifun-poi/)：薄 HTTP 层，打开同一个
  `InterestStore` 并读写 config.yaml 中的 `InterestConfig`；路由
  `/api/poi/topics*`（列表/清除/置顶/状态）、`/api/poi/starters`、`/api/poi/status`、
  `/api/poi/settings`。前端设置页 `pages/settings/PoiSettings.tsx`
  （启用开关、模型选择含跟随会话 `__session__`、话题表 candidate/active/rejected）。

## Insights（去标识化贡献）

- 引擎 [`nomi-insights-core`](../../crates/agent/nomi-insights-core/)：
  `ContributionService` + `ContributionClient`（`POST /v1/insights/batch` 上行、
  `DELETE /v1/installations/{id}` 撤销），outbox 与审计文件落状态目录；安装级 ID +
  `INSIGHTS_CONSENT_VERSION` 同意版本；会话-技能挖掘与工作包构建器。
  **脱敏自带一层**（`redact.rs`：URL/query/userinfo/form 秘密）+ `sanitize.rs`。
- 后端 [`nomifun-insights`](../../crates/backend/nomifun-insights/)：包装
  `ContributionService`；同意状态存 `GatewayConfig.insights`（config.yaml），并与云端
  生效配置合并（`nomifun_cloud::effective_insights_contribution_config`）；另含
  local_analytics 模块。路由 `/api/insights/contribution/status|contribution|flush|reset`。
  前端 `pages/settings/InsightsSettings.tsx`
  （enabled / on_session_end / auto_extract + idle 秒数 / skill_mining /
  redacted_body 开关，flush/reset 动作）。
- 触发点：[`nomifun-ai-agent/src/capability/proactive_extraction.rs`](../../crates/backend/nomifun-ai-agent/src/capability/proactive_extraction.rs)
  在会话结束时同时拉起 Insights 管线与 POI ingest。

## nomi-auxiliary（旁路 LLM 客户端）

[`nomi-auxiliary`](../../crates/agent/nomi-auxiliary/) 是给旁路小任务用的最小 LLM
客户端类型：POI 的信号抽取/starter 生成、Insights 的消解标注都走它；后端侧
`nomifun-ai-agent/src/auxiliary_provider.rs` 的 `AuxiliaryClientFactory` 把旁路模型
**限制在内置 flowy-cloud provider** 上并带重试，尽力而为、失败不影响主链路。

## 与 `nomi-redact` 的关系

shared 层的 [`nomi-redact`](../../crates/shared/nomi-redact/) 是另一条独立防线：
best-effort 秘密清洗器（`redact_secrets` → `[REDACTED_SECRET]`），在**会话落盘前**
被 nomi-agent / nomi-agent-trace / nomifun-conversation / knowledge / companion /
system / ai-agent / browser-engine 使用 —— 它不服务于 insights 上行，insights 用
自己的 redact 层。两条脱敏路径不要混为一谈。

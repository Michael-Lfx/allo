//! Two-loop concept-graph agent: the draft tool set (`cg_start` ..
//! `cg_finish`) drives generation, then audit-gated repair rounds drive
//! publishing.
//!
//! Same layering as `LiveLearningCompleter`: the learning crate holds only
//! [`ConceptGraphAgentEngine`]; this crate provides the provider-backed
//! implementation, and the app layer wires it via
//! `LearningService::set_concept_graph_engine`.
//!
//! Why a dedicated loop instead of `run_one_shot_turn`? The one-shot entry
//! caps tool rounds at 8 and returns only the final text; generation needs
//! up to 30 rounds and must observe the draft/publish side effects
//! ([`LoopContext`] slots). The tool loop below is therefore the one-shot
//! core with the round cap, the token budget and the message window
//! parameterized — same fail-closed whitelist execution (an unknown tool
//! name gets an error result and never reaches any other surface).
//!
//! The deterministic audit gate holds the decision power: the generation
//! loop builds, the repair loop fixes exactly what the audit reports, and
//! only `cg_finish` (which re-runs the audit as a hard gate) publishes.

use std::sync::{Arc, Mutex};

use nomi_providers::{LlmProvider, create_provider};
use nomifun_common::{AppError, LearningConceptGraphId, ProviderId, UserId};
use nomifun_learning::{
    ConceptGraphAgentEngine, ConceptGraphRecord, GraphOp, LearningService, NodeQuery,
    SubgraphDirection,
};

use crate::factory::provider_config::resolve_provider_config;
use crate::knowledge_completer::resolve_default_model;
use crate::loop_core::{
    AGENT_MAX_TOKENS, GENERATE_MAX_ROUNDS, LoopEventSink, REPAIR_LOOP_LIMIT, REPAIR_MAX_ROUNDS,
    TOTAL_TIMEOUT_SECS, json_compact, log_text, run_agent_loop,
};
use crate::one_shot::{OneShotDeps, OneShotTool, one_shot_handler};

/// Generation-loop system prompt. The model builds the whole network via
/// the draft tools; the audit gate still has the last word at `cg_finish`.
const GENERATE_AGENT_SYSTEM: &str = r#"你是一名概念图构建代理：把一个宽泛学习目标分解为完整的"学习单元网络"，并通过工具逐步构建成图。
- 认知学习单元不一定是原子概念，很多简单的原子概念不适合作为人类单次学习行为单元，应该是简单原子概念的少量的有机的组合，但也可能有些原子概念本身就足够复杂这种情况可以作为单独集结点，不能是太复合的概念，过于复合的概念没有意义，
- *无依赖的节点一定是最基础最简单的（重点）*，如果这些基础概念本身有更精准但过于抽象复杂或者说需要前置条件时，也应先创建一个简单的节点在前，而不是放一个实际不可用的零依赖起点
    - 想想以用户的视角为起点，他到底满不满足以这个为起点的条件
- 要有整体规划，学习线路应该从易到难，不能有过突兀过大的难度曲线变化导致不可控的认知负荷，难度本身有提高的必要但应时渐进的
- 生成的学习单元网络一定要完整，宁愿节点过多不要过少，节点关系没有必要冗余，但应该是无环的复杂网络，如果节点的连接过少大概率是认知切分方式的或连接存在问题
- 对于最终的目标应该是真正彻底的学会而非只是会使用，所以可以按需存在具有元（meta）或者说抽象 本质 证明等性质的节点，但应满足上面提到的螺旋上升与渐进的原则,比如 无依赖的节点一定是最基础最简单的
【工具使用纪律】
1. 第一步必须调用 cg_start 创建草稿——它会先做范围分析，返回 draft_id 和 scope 覆盖清单。这就是你的基本参考清单，至少要满足里面的要求除非其中出现虚构或确实毫无必要的概念，除了覆盖清单你也可以发挥主观能动性，以及要小心参考清单本身不够全面的可能性。
2. 每次 cg_patch 前后调用 cg_inspect 掌握全局；动手前用 cg_query / cg_subgraph 查清单元名的精确写法——patch 里的引用必须与图中名称完全一致，否则整个操作被拒。
3. 分批构建：每批小于 25 个操作，宁可多批，不要超长批次。
4. 全部构建完成后调用 cg_audit 自查

【单元命名】（与单次生成管线同标准）
- 节点大多 name 可以是动作句：包含 解/求/证明/推导/比较/判定/构造/区分/计算/应用/理解/辨析/建立/验证/化简/变形/转化/估计/近似/检验/分类/归纳/抽象/训练 等等词以及它们之间的组合存在。                    
- 螺旋式学习合法：同一主题在不同深度以不同视角出现，但名字不得重复或近似。
- 单元是通常 30 分钟内可完成的一个学习会话；极困难的综合课最多 60 分钟。

【依赖契约】
- 充分性（SUFFICIENCY）：pre 是完整的直接前置集合——学完恰好这些单元即可理解本单元，缺一不可；不要为了缩短回复而裁剪 pre。
- 收敛：真实知识是 DAG 不是链——两个早期线程交汇处应出现带 n 个前置的汇聚单元。
- 除真正的无需基础的入门单元外，pre 不得为空。
- 连通性：整个网络必须是一个连通结构；子域在真实依赖处必须交叉链接

【覆盖与规模】
- 以 cg_scope / cg_start 返回的 scope 为主要参考。
- min 是学习分钟预算：常规 5-30 分钟（软上限，尽量不超过 30），极困难单课最多 60 分钟（硬上限）；超过 60 会被拒。
- 引用名必须恰好等于图中已存在的单元名；拿不准时先 cg_query。

【结束条件】
- 只有当你确认图已完整覆盖 scope 且 cg_audit 基本健康时，才调用 cg_finish；否则继续构建。"#;

/// Repair-loop system prompt: the audit report is the ONLY repair basis;
/// the model patches locally and never rewrites the graph wholesale.
const REPAIR_AGENT_SYSTEM: &str = r#"你是一名概念图修复代理：基于确定性审计报告，精确修复图中被指出的问题。

【修复原则】
1. 审计报告是主要的修复依据：逐条处理 danger 级 findings，按报告给出的证据（节点名、缺失的大块概念名、孤立组件）精确操作；
2. 不做过大的重构、不删无关节点。（add/link/unlink/set_pre/reverse/split/merge/update/delete）。
3. 动手前可以用 cg_query / cg_subgraph 查清引用名的精确写法；引用不一致会被拒绝。
4. 修复动作分批提交（每批 5-15 个操作），每批后用 cg_audit 复查该条 finding 是否消除。
5. 全部 danger 消除后调用 cg_finish 发布。

【常见修复动作对照】
- missing_block_coverage：按报告列出的大块概念名，add 对应单元（把概念改写为动作句）。
- coverage：对照 scope 清单，补齐缺失大块概念的单元。
- disconnected_components：用 link 或在新单元的 pre 中引用，把孤立组件接入主结构。
- orphaned_units：把失去唯一前置的单元重新 link 到它真正的前置；旧的前置边确实画错时，用 set_pre 整体替换该单元的前置（或先 unlink 移除错误边再 link）。
- tree_structure：为确实需要两条以上前线的单元补 link，制造收敛。
- unit_overload：split 超过 60 分钟硬上限的单元；30-60 分钟的偏重单元若可拆也建议拆。

【结束条件】
- 审计无 danger 时调用 cg_finish；若 cg_finish 被拒绝，认真阅读返回的阻塞 findings 并继续修复。
- 禁止空手结束：每一轮都必须调用 cg_patch 执行修复动作（或 cg_audit 复查、cg_finish 尝试发布）；只输出文字而不调用任何工具，会被判定为拒绝修复，整个生成以失败告终。
- 回复使用中文。"#;

/// Provider-backed engine for the two-loop concept graph pipeline.
pub struct LiveConceptGraphAgentEngine {
    pub service: Arc<LearningService>,
    pub deps: OneShotDeps,
}

#[async_trait::async_trait]
impl ConceptGraphAgentEngine for LiveConceptGraphAgentEngine {
    async fn generate(
        &self,
        user_id: &UserId,
        topic: &str,
        model_override: Option<(&str, &str)>,
    ) -> Result<ConceptGraphRecord, AppError> {
        let (provider_id, model) = match model_override {
            Some((provider_id, model)) => (provider_id.to_owned(), model.to_owned()),
            None => resolve_default_model(&self.deps.provider_repo, &self.deps.provider_model_repo)
                .await
                .ok_or_else(|| {
                    AppError::Conflict(
                        "concept graph generation unavailable: no enabled provider/model is configured"
                            .into(),
                    )
                })?,
        };
        let provider_id: ProviderId = ProviderId::parse(provider_id)
            .map_err(|error| AppError::BadRequest(format!("invalid provider id: {error}")))?;
        let cfg = resolve_provider_config(
            &self.deps.provider_repo,
            &self.deps.provider_model_repo,
            &self.deps.encryption_key,
            provider_id.as_str(),
            &model,
            &self.deps.workspace,
        )
        .await?;
        let provider: Arc<dyn LlmProvider> = create_provider(&cfg);

        // One session stream names every event of this generation in the
        // shared `concept-graph-generation.log` (same file as the legacy
        // pipeline), so a failed run stays diagnosable offline.
        let session = LearningConceptGraphId::new().as_str().to_owned();
        self.service.log_concept_graph_event(&session, "session_start", serde_json::json!({
            "topic": topic,
            "provider_id": provider_id.as_str(),
            "model": &model,
            "generate_max_rounds": GENERATE_MAX_ROUNDS,
            "repair_loop_limit": REPAIR_LOOP_LIMIT,
            "timeout_secs": TOTAL_TIMEOUT_SECS,
        }));

        // The two slots are shared with the tool handlers: the draft the
        // model opened and the record `cg_finish` published. On timeout they
        // also carry the diagnostics (draft id -> live audit report).
        let ctx = Arc::new(LoopContext {
            service: Arc::clone(&self.service),
            session,
            topic: topic.to_owned(),
            user_id: user_id.clone(),
            model_override: Some((provider_id, model.clone())),
            draft_slot: Arc::new(Mutex::new(None)),
            published_slot: Arc::new(Mutex::new(None)),
        });

        match tokio::time::timeout(
            std::time::Duration::from_secs(TOTAL_TIMEOUT_SECS),
            self.run_loops(provider, &model, topic, Arc::clone(&ctx)),
        )
        .await
        {
            Ok(Ok(record)) => {
                ctx.log("session_end", serde_json::json!({
                    "ok": true,
                    "record_id": record.id,
                    "nodes": record.graph.nodes.len(),
                    "edges": record.graph.edges.len(),
                }));
                Ok(record)
            }
            Ok(Err(error)) => {
                ctx.log("session_end", serde_json::json!({
                    "ok": false,
                    "error": error.to_string(),
                }));
                Err(error)
            }
            Err(_) => {
                let mut message = format!(
                    "concept graph agent timed out after {TOTAL_TIMEOUT_SECS}s"
                );
                if let Some(draft_id) = ctx
                    .draft_slot
                    .lock()
                    .ok()
                    .and_then(|slot| slot.clone())
                {
                    match self.service.audit_concept_graph_draft(&draft_id) {
                        Ok(audit) => {
                            message.push_str(&format!(
                                "\ndraft {draft_id} survives; its audit state:\n{audit}"
                            ));
                        }
                        Err(_) => {
                            message.push_str(&format!(
                                "\ndraft {draft_id} survives; audit unavailable"
                            ));
                        }
                    }
                }
                ctx.log("session_end", serde_json::json!({
                    "ok": false,
                    "error": "timeout",
                }));
                Err(AppError::Internal(message))
            }
        }
    }
}

impl LiveConceptGraphAgentEngine {
    /// Generation loop, then audit-gated repair loops. `provider` is
    /// injected so tests stub the LLM here (same seam as
    /// `run_one_shot_turn_with_provider`).
    async fn run_loops(
        &self,
        provider: Arc<dyn LlmProvider>,
        model: &str,
        topic: &str,
        ctx: Arc<LoopContext>,
    ) -> Result<ConceptGraphRecord, AppError> {
        // ── Generation loop: full tool set, the topic as the user turn ──
        ctx.log("generate_loop_start", serde_json::json!({
            "max_rounds": GENERATE_MAX_ROUNDS,
            "tool_count": concept_graph_tools(Arc::clone(&ctx), true).len(),
        }));
        let generate_tools = concept_graph_tools(Arc::clone(&ctx), true);
        let final_text = run_agent_loop(
            provider.clone(),
            model,
            GENERATE_AGENT_SYSTEM,
            topic,
            &generate_tools,
            GENERATE_MAX_ROUNDS,
            AGENT_MAX_TOKENS,
            "generate",
            Some(ctx.as_ref()),
        )
        .await?;
        if let Some(record) = take_published(&ctx) {
            ctx.log("publish_ok", serde_json::json!({
                "record_id": record.id,
                "phase": "generate",
            }));
            return Ok(record);
        }
        let draft_id = ctx
            .draft_slot
            .lock()
            .map_err(|_| AppError::Internal("concept graph draft slot poisoned".into()))?
            .clone()
            .ok_or_else(|| {
                ctx.log("generate_loop_end", serde_json::json!({
                    "ok": false,
                    "reason": "no draft created (cg_start never called)",
                    "text": log_text(&final_text),
                }));
                AppError::Internal(
                    "concept graph agent finished without creating a draft (cg_start was never called)"
                        .into(),
                )
            })?;
        ctx.log("generate_loop_end", serde_json::json!({
            "ok": true,
            "draft_id": draft_id,
            "text": log_text(&final_text),
        }));

        // ── Repair loops: the audit gate has the last word ──────────────
        // `finish_concept_graph_draft` IS the deterministic gate: success
        // means the graph cleared it, UnprocessableEntity means danger
        // findings remain and the repair loop gets the full report.
        // `idle_nudge` carries a warning into the next loop when the model
        // ended a repair round without touching the draft (revision
        // unchanged) — models may otherwise "reply, not repair".
        let mut idle_nudge: Option<String> = None;
        for round in 0..REPAIR_LOOP_LIMIT {
            ctx.log("repair_loop_start", serde_json::json!({
                "round": round + 1,
                "draft_id": draft_id,
            }));
            match ctx
                .service
                .finish_concept_graph_draft(&ctx.user_id, &draft_id)
                .await
            {
                Ok(record) => {
                    ctx.log("publish_ok", serde_json::json!({
                        "record_id": record.id,
                        "phase": "repair",
                        "round": round + 1,
                    }));
                    return Ok(record);
                }
                Err(AppError::UnprocessableEntity(_)) => {
                    ctx.log("finish_blocked", serde_json::json!({
                        "round": round + 1,
                        "draft_id": draft_id,
                    }));
                }
                Err(error) => return Err(error),
            }
            let audit = ctx.service.audit_concept_graph_draft(&draft_id)?;
            let repair_user = match &idle_nudge {
                Some(nudge) => format!("{nudge}\n\n{audit}"),
                None => audit,
            };
            let repair_tools = concept_graph_tools(Arc::clone(&ctx), false);
            let revision_before = ctx
                .service
                .inspect_concept_graph_draft(&draft_id)?
                .revision;
            let final_text = run_agent_loop(
                provider.clone(),
                model,
                REPAIR_AGENT_SYSTEM,
                &repair_user,
                &repair_tools,
                REPAIR_MAX_ROUNDS,
                AGENT_MAX_TOKENS,
                "repair",
                Some(ctx.as_ref()),
            )
            .await?;
            if let Some(record) = take_published(&ctx) {
                ctx.log("publish_ok", serde_json::json!({
                    "record_id": record.id,
                    "phase": "repair",
                    "round": round + 1,
                }));
                return Ok(record);
            }
            let revision_after = ctx
                .service
                .inspect_concept_graph_draft(&draft_id)?
                .revision;
            if revision_after == revision_before {
                ctx.log("repair_loop_idle", serde_json::json!({
                    "round": round + 1,
                    "revision": revision_after,
                    "text": log_text(&final_text),
                }));
                idle_nudge = Some(format!(
                    "警告：你上一轮没有对草稿做任何修改（revision 仍是 {revision_after}），只回复了文字。\
                     禁止空手结束：本轮必须调用 cg_patch 执行修复动作，或以 cg_finish 尝试发布；\
                     只输出文字而不调用任何工具，会被判定为拒绝修复，整个生成将以失败告终。"
                ));
            } else {
                idle_nudge = None;
            }
            ctx.log("repair_loop_end", serde_json::json!({
                "round": round + 1,
                "draft_id": draft_id,
                "revision": revision_after,
                "text": log_text(&final_text),
            }));
        }

        // Budget exhausted: report honestly with the surviving findings
        // (the draft is kept, so a human or a later run can continue).
        let audit = ctx.service.audit_concept_graph_draft(&draft_id)?;
        ctx.log("repair_budget_exhausted", serde_json::json!({
            "draft_id": draft_id,
        }));
        Err(AppError::UnprocessableEntity(format!(
            "concept graph agent exhausted {REPAIR_LOOP_LIMIT} repair loops; the draft survives with these blocking findings:\n{audit}"
        )))
    }
}

// ── Shared loop state ──────────────────────────────────────────────────────

/// Everything the tool handlers need, captured once per generation. The two
/// slots are the only mutable cross-round state: which draft is active and
/// which record (if any) was published by `cg_finish`. `session` names this
/// generation's event stream in the shared concept-graph log file.
struct LoopContext {
    service: Arc<LearningService>,
    session: String,
    topic: String,
    user_id: UserId,
    model_override: Option<(ProviderId, String)>,
    draft_slot: Arc<Mutex<Option<String>>>,
    published_slot: Arc<Mutex<Option<ConceptGraphRecord>>>,
}

impl LoopContext {
    /// Append one JSON-line event to the shared concept-graph log (the same
    /// `concept-graph-generation.log` the legacy pipeline writes). Best-effort.
    fn log(&self, event: &str, fields: serde_json::Value) {
        self.service
            .log_concept_graph_event(&self.session, event, fields);
    }

    fn require_draft(&self) -> Result<String, String> {
        self.draft_slot
            .lock()
            .map_err(|_| "internal draft slot lock failed".to_owned())?
            .clone()
            .ok_or_else(|| "没有活动的草稿——请先调用 cg_start".to_owned())
    }
}

impl LoopEventSink for LoopContext {
    fn log(&self, event: &str, fields: serde_json::Value) {
        LoopContext::log(self, event, fields);
    }
}

/// Take the published record out of the slot (once) — called after every
/// loop, because the model may legitimately `cg_finish` from either loop.
fn take_published(ctx: &LoopContext) -> Option<ConceptGraphRecord> {
    ctx.published_slot
        .lock()
        .ok()
        .and_then(|mut slot| slot.take())
}

// ── Tool set ───────────────────────────────────────────────────────────────

/// The `cg_*` whitelist. `with_start` adds `cg_start` (generation loop
/// only — the repair loop must never re-scope a finished draft, and an
/// unlisted tool name fails closed at the loop level).
fn concept_graph_tools(ctx: Arc<LoopContext>, with_start: bool) -> Vec<OneShotTool> {
    let mut tools = Vec::with_capacity(8);
    if with_start {
        tools.push(cg_start(Arc::clone(&ctx)));
    }
    tools.push(cg_inspect(Arc::clone(&ctx)));
    tools.push(cg_query(Arc::clone(&ctx)));
    tools.push(cg_subgraph(Arc::clone(&ctx)));
    tools.push(cg_audit(Arc::clone(&ctx)));
    tools.push(cg_scope(Arc::clone(&ctx)));
    tools.push(cg_patch(Arc::clone(&ctx)));
    tools.push(cg_finish(Arc::clone(&ctx)));
    tools
}

/// Compact JSON without \uXXXX escapes (the default serializer already
/// keeps non-ASCII; this is the single formatting seam for tool replies) and
/// the truncation/stop-reason helpers live in `loop_core` — re-imported here.

fn cg_start(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "cg_start".into(),
        description: "启动概念图草稿：先运行范围分析，返回 draft_id 与 scope 覆盖清单（大块概念）。这是你的第一个工具调用；幂等——已有活动草稿时直接返回现有草稿。".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "topic": { "type": "string", "description": "学习目标，必须与任务中给出的主题完全一致" },
                "focus": { "type": "string", "description": "可选：用户补充的重点或约束，用于收窄范围" }
            },
            "required": ["topic"]
        }),
        handler: one_shot_handler(move |input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let topic = input
                    .get("topic")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .trim();
                if topic != ctx.topic {
                    return Err(format!(
                        "cg_start: topic 必须与任务主题完全一致——收到 '{}'，任务主题是 '{}'",
                        topic, ctx.topic
                    ));
                }
                let focus = input
                    .get("focus")
                    .and_then(|value| value.as_str())
                    .map(str::to_owned);
                if let Some(existing) = ctx
                    .draft_slot
                    .lock()
                    .map_err(|_| "cg_start: 内部锁故障".to_owned())?
                    .clone()
                {
                    let view = ctx
                        .service
                        .inspect_concept_graph_draft(&existing)
                        .map_err(|error| error.to_string())?;
                    return Ok(format!("已有活动草稿（cg_start 幂等返回）：{}", json_compact(&view)));
                }
                let model_override = ctx
                    .model_override
                    .as_ref()
                    .map(|(provider, model)| (provider, model.as_str()));
                let view = ctx
                    .service
                    .create_concept_graph_draft(&ctx.topic, focus.as_deref(), model_override)
                    .await
                    .map_err(|error| error.to_string())?;
                *ctx.draft_slot
                    .lock()
                    .map_err(|_| "cg_start: 内部锁故障".to_owned())? = Some(view.draft_id.clone());
                Ok(format!("草稿已创建：{}", json_compact(&view)))
            }
        }),
    }
}

fn cg_inspect(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "cg_inspect".into(),
        description: "当前草稿全局概览：节点/边数、总分钟预算、各范围块的单元分布、入口/终点单元、审计摘要。每次 cg_patch 前后调用，保持全局认知。".into(),
        input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        handler: one_shot_handler(move |_input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                let view = ctx
                    .service
                    .inspect_concept_graph_draft(&draft_id)
                    .map_err(|error| error.to_string())?;
                Ok(json_compact(&view))
            }
        }),
    }
}

fn cg_query(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "cg_query".into(),
        description: "按名称子串 / 重叠词过滤列出单元：返回 id、分钟预算、直接前置与直接后继。任何 cg_patch 之前先查询，确保引用的名字与图中完全一致。limit 上限 200，防止上下文爆炸。".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "matcher": { "type": "string", "description": "单元名子串（可选，空=不过滤）" },
                "keyword": { "type": "string", "description": "名称重叠过滤词（如某个大块概念名）；只保留名称与之重叠 >=2 个连续字符的单元（可选）" },
                "limit": { "type": "integer", "minimum": 1, "maximum": 200, "description": "返回条数上限，默认 50" }
            }
        }),
        handler: one_shot_handler(move |input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                let query = NodeQuery {
                    matcher: input
                        .get("matcher")
                        .and_then(|value| value.as_str())
                        .map(str::to_owned),
                    keyword: input
                        .get("keyword")
                        .and_then(|value| value.as_str())
                        .map(str::to_owned),
                    limit: input
                        .get("limit")
                        .and_then(|value| value.as_u64())
                        .map(|limit| limit as usize)
                        .unwrap_or(50)
                        .clamp(1, 200),
                };
                let list = ctx
                    .service
                    .query_concept_graph_draft(&draft_id, query)
                    .map_err(|error| error.to_string())?;
                Ok(json_compact(&list))
            }
        }),
    }
}

fn cg_subgraph(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "cg_subgraph".into(),
        description: "查看某区域的依赖局部图：围绕给定单元展开前置（ancestors）/ 后继（descendants）闭包，返回局部 DAG 的节点与边。编辑某区域前先看清它的依赖结构。".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "nodes": { "type": "array", "items": { "type": "string" }, "description": "中心单元名（必须是图中精确存在的名字）" },
                "direction": { "type": "string", "enum": ["ancestors", "descendants", "both"], "description": "ancestors=全部前置闭包；descendants=全部后继闭包；both=双向" },
                "depth": { "type": "integer", "minimum": 1, "description": "游走层数上限（可选；缺省=完整闭包）" }
            },
            "required": ["nodes", "direction"]
        }),
        handler: one_shot_handler(move |input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                let nodes: Vec<String> = input
                    .get("nodes")
                    .and_then(|value| value.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
                if nodes.is_empty() {
                    return Err("cg_subgraph: nodes 必须是至少一个单元名的数组".into());
                }
                let direction = match input
                    .get("direction")
                    .and_then(|value| value.as_str())
                {
                    Some("ancestors") => SubgraphDirection::Ancestors,
                    Some("descendants") => SubgraphDirection::Descendants,
                    Some("both") => SubgraphDirection::Both,
                    _ => {
                        return Err(
                            "cg_subgraph: direction 必须是 ancestors / descendants / both".into()
                        );
                    }
                };
                let depth = input
                    .get("depth")
                    .and_then(|value| value.as_u64())
                    .map(|value| value as usize);
                let view = ctx
                    .service
                    .subgraph_concept_graph_draft(&draft_id, nodes, direction, depth)
                    .map_err(|error| error.to_string())?;
                Ok(json_compact(&view))
            }
        }),
    }
}

fn cg_audit(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "cg_audit".into(),
        description: "完整确定性审计报告：每条 finding 的级别、类型、证据与修复提示，以及 scope 对照覆盖情况（大块概念未落地）。修复 loop 的主要输入；发布前的最终自查也用它。".into(),
        input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        handler: one_shot_handler(move |_input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                ctx.service
                    .audit_concept_graph_draft(&draft_id)
                    .map_err(|error| error.to_string())
            }
        }),
    }
}

fn cg_scope(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "cg_scope".into(),
        description: "返回 scope 范围参考全文：大块概念清单——生成阶段的严格完备覆盖自查表。构建前先取回它，逐项核对你的计划，确保每个大块概念都不遗漏。".into(),
        input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        handler: one_shot_handler(move |_input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                match ctx
                    .service
                    .scope_reference_concept_graph_draft(&draft_id)
                    .map_err(|error| error.to_string())?
                {
                    Some(reference) => Ok(reference),
                    None => Ok("本草稿没有 scope 参考（范围分析不可用时降级）；以 cg_audit 的覆盖检查为准".into()),
                }
            }
        }),
    }
}

fn cg_patch(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "cg_patch".into(),
        description: "批量应用图操作（add/link/unlink/set_pre/reverse/split/merge/update/delete），一次调用就是一个批次。操作按数组顺序执行，后面的操作可以引用本批前面 add 的名字。返回每个操作的成功/拒绝明细 + 最新审计摘要。\n\n各操作语义：\n- add：新增单元，pre 是直接前置名列表，min 是分钟预算。\n- link：在两个已有单元之间补一条缺失的依赖边 from -> to。\n- unlink：仅移除一条依赖边 from -> to，两个单元都保留——发现关系画错（而非方向画反）时用它；reverse 只适合改方向。\n- set_pre：把 target 的前置集合整体替换为 pre（完整集合语义），一步完成'删错边+接对边'；前置画错或多余时优先用它。\n- reverse：把已有边 from -> to 翻转为 to -> from。\n- split / merge / update / delete：拆分、合并、改名或改预算；delete 会删除单元及其触及的所有边。\n\n调用示例：\n{\"operations\": [\n  {\"op\": \"add\", \"name\": \"用几何概括法临摹静物\", \"pre\": [], \"min\": 20},\n  {\"op\": \"add\", \"name\": \"使用一点透视绘制空间\", \"pre\": [\"用几何概括法临摹静物\"], \"min\": 25},\n  {\"op\": \"unlink\", \"from\": \"使用一点透视绘制空间\", \"to\": \"画布与画笔准备\"},\n  {\"op\": \"link\", \"from\": \"画布与画笔准备\", \"to\": \"用几何概括法临摹静物\"},\n  {\"op\": \"set_pre\", \"target\": \"使用一点透视绘制空间\", \"pre\": [\"用几何概括法临摹静物\", \"画布与画笔准备\"]}\n]}\n\n引用规则：add 的 pre、set_pre 的 pre 和 link/unlink/reverse 的 from/to 必须与图中已有名字（或本批前面 add 的名字）完全一致，不一致的操作会被拒绝并附最近似的名字提示。每批 5-15 个操作，宁多批勿超长。".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "operations": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 15,
                    "description": "图操作数组（按顺序执行，可引用本批前面 add 创建的名字）；每个元素是带 op 标签的对象，见下方 items 的 oneOf 定义",
                    "items": {
                        "oneOf": [
                            { "type": "object", "properties": { "op": { "const": "add" }, "name": { "type": "string", "description": "新单元名——动作句，通常 30 分钟内可完成的一个学习会话（极困难最多 60 分钟）" }, "pre": { "type": "array", "items": { "type": "string" }, "description": "直接前置单元名，必须与图中已有名字（或本批前面 add 的名字）完全一致；入门单元可为空" }, "min": { "type": "integer", "minimum": 1, "maximum": 60, "description": "学习分钟预算：常规 5-30（软上限），极困难单课最多 60（硬上限）" } }, "required": ["op", "name"] },
                            { "type": "object", "properties": { "op": { "const": "link" }, "from": { "type": "string", "description": "已存在单元名（前置）" }, "to": { "type": "string", "description": "已存在单元名（后继）" } }, "required": ["op", "from", "to"] },
                            { "type": "object", "properties": { "op": { "const": "unlink" }, "from": { "type": "string", "description": "已存在单元名（当前前置）" }, "to": { "type": "string", "description": "已存在单元名（当前后继）" } }, "required": ["op", "from", "to"] },
                            { "type": "object", "properties": { "op": { "const": "reverse" }, "from": { "type": "string", "description": "已存在单元名（新前置）" }, "to": { "type": "string", "description": "已存在单元名（新后继）" } }, "required": ["op", "from", "to"] },
                            { "type": "object", "properties": { "op": { "const": "set_pre" }, "target": { "type": "string", "description": "要改前置的已存在单元名" }, "pre": { "type": "array", "items": { "type": "string" }, "description": "完整的新直接前置名列表（必须与图中已有名字或本批前面 add 的名字完全一致）；空数组=清空前置使其成为入门单元（是否合规由审计判定）；重复项自动去重" } }, "required": ["op", "target", "pre"] },
                            { "type": "object", "properties": { "op": { "const": "split" }, "target": { "type": "string", "description": "要拆分的已存在单元名" }, "into": { "type": "array", "items": { "type": "object", "properties": { "name": { "type": "string", "description": "拆分后的新单元名（动作句）" }, "pre": { "type": "array", "items": { "type": "string" }, "description": "可选的直接前置名（已有单元或本批其它拆分）；留空则继承被拆单元的前置" }, "min": { "type": "integer", "minimum": 1, "maximum": 60 } }, "required": ["name"] } } }, "required": ["op", "target", "into"] },
                            { "type": "object", "properties": { "op": { "const": "merge" }, "into": { "type": "string", "description": "保留的已存在单元名" }, "targets": { "type": "array", "items": { "type": "string" }, "description": "要并入的已存在单元名列表" } }, "required": ["op", "into", "targets"] },
                            { "type": "object", "properties": { "op": { "const": "update" }, "target": { "type": "string", "description": "要改的已存在单元名" }, "name": { "type": "string", "description": "新名字（可选）" }, "min": { "type": "integer", "minimum": 1, "maximum": 60, "description": "新分钟预算（可选）" } }, "required": ["op", "target"] },
                            { "type": "object", "properties": { "op": { "const": "delete" }, "target": { "type": "string", "description": "要删除的已存在单元名" } }, "required": ["op", "target"] }
                        ]
                    }
                }
            },
            "required": ["operations"]
        }),
        handler: one_shot_handler(move |input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                let operations =
                    input.get("operations").cloned().unwrap_or(serde_json::Value::Null);
                let ops: Vec<GraphOp> = serde_json::from_value(operations)
                    .map_err(|error| format!("cg_patch: operations 必须是图操作数组——{error}"))?;
                if ops.is_empty() {
                    return Err("cg_patch: operations 不能为空".into());
                }
                let report = ctx
                    .service
                    .patch_concept_graph_draft(&draft_id, ops)
                    .map_err(|error| error.to_string())?;
                Ok(json_compact(&report))
            }
        }),
    }
}

fn cg_finish(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "cg_finish".into(),
        description: "发布草稿为正式概念图记录。确定性审计门禁有最终决定权：存在 danger 级 findings 时发布被阻塞，返回完整阻塞报告（草稿保留，可继续修复）。只有 cg_audit 确认无 danger 时才调用。".into(),
        input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        handler: one_shot_handler(move |_input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                match ctx
                    .service
                    .finish_concept_graph_draft(&ctx.user_id, &draft_id)
                    .await
                {
                    Ok(record) => {
                        *ctx.published_slot
                            .lock()
                            .map_err(|_| "cg_finish: 内部锁故障".to_owned())? =
                            Some(record.clone());
                        Ok(format!(
                            "概念图已发布：id={}，主题={}，{} 个单元 / {} 条边。",
                            record.id,
                            record.topic,
                            record.graph.nodes.len(),
                            record.graph.edges.len()
                        ))
                    }
                    Err(error) => Err(error.to_string()),
                }
            }
        }),
    }
}

// ── The tool loop (one-shot core, parameterized) ───────────────────────────
//
// `run_agent_loop` lives in `loop_core` — shared verbatim by the
// course-outline and lesson-content loops. The fail-closed whitelist
// execution contract is documented there.

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_providers::ProviderError;
    use nomi_types::llm::{LlmEvent, LlmRequest, ThinkingConfig};
    use nomi_types::message::{ContentBlock, Message, StopReason, TokenUsage};
    use nomifun_learning::LearningCompleter;
    use tokio::sync::mpsc;

    use crate::knowledge_completer::tests::{ListOnlyModelRepo, ListOnlyRepo};

    /// Scripted fake provider: each `stream` call pops the next script
    /// entry; every observed request (tool names + messages) is recorded.
    struct ScriptedProvider {
        script: Mutex<Vec<Vec<LlmEvent>>>,
        seen_tool_names: Mutex<Vec<Vec<String>>>,
        seen_messages: Mutex<Vec<Vec<Message>>>,
        seen_thinking: Mutex<Vec<Option<ThinkingConfig>>>,
    }

    impl ScriptedProvider {
        fn new(script: Vec<Vec<LlmEvent>>) -> Arc<Self> {
            Arc::new(Self {
                script: Mutex::new(script),
                seen_tool_names: Mutex::new(Vec::new()),
                seen_messages: Mutex::new(Vec::new()),
                seen_thinking: Mutex::new(Vec::new()),
            })
        }
    }

    #[async_trait::async_trait]
    impl LlmProvider for ScriptedProvider {
        async fn stream(
            &self,
            request: &LlmRequest,
        ) -> Result<mpsc::Receiver<LlmEvent>, ProviderError> {
            self.seen_tool_names
                .lock()
                .unwrap()
                .push(request.tools.iter().map(|tool| tool.name.clone()).collect());
            self.seen_messages.lock().unwrap().push(request.messages.clone());
            self.seen_thinking.lock().unwrap().push(request.thinking.clone());
            let mut script = self.script.lock().unwrap();
            if script.is_empty() {
                return Err(ProviderError::Connection("script exhausted".into()));
            }
            let events = script.remove(0);
            let (tx, rx) = mpsc::channel(events.len().max(1));
            tokio::spawn(async move {
                for event in events {
                    if tx.send(event).await.is_err() {
                        break;
                    }
                }
            });
            Ok(rx)
        }
    }

    fn done(stop_reason: StopReason) -> LlmEvent {
        LlmEvent::Done { stop_reason, usage: TokenUsage::default() }
    }

    fn tool_use(name: &str, input: serde_json::Value) -> LlmEvent {
        LlmEvent::ToolUse {
            id: format!("call_{name}"),
            name: name.into(),
            input,
            extra: None,
        }
    }

    fn add_op(name: &str, pre: &[&str]) -> serde_json::Value {
        serde_json::json!({ "op": "add", "name": name, "pre": pre })
    }

    /// Completer whose reply never parses as a scope reference: every draft
    /// starts scope-free, keeping the deterministic audit fully structural.
    struct FakeCompleter;

    #[async_trait::async_trait]
    impl LearningCompleter for FakeCompleter {
        async fn complete(
            &self,
            _model_override: Option<(&str, &str)>,
            _system: &str,
            _user: &str,
            _max_tokens: u32,
        ) -> Result<String, AppError> {
            Ok("not a scope json".into())
        }
    }

    #[derive(Default)]
    struct NoopBroadcaster;

    impl nomifun_realtime::UserEventSink for NoopBroadcaster {
        fn send_to_user(
            &self,
            _user_id: &str,
            _event: nomifun_api_types::WebSocketMessage<serde_json::Value>,
        ) {
        }
    }

    /// A service wired with the fake completer and a scratch concept-graph
    /// dir; the temp dir stays alive for the test's duration.
    async fn test_service() -> (Arc<LearningService>, tempfile::TempDir) {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let owner_id = nomifun_db::installation_owner_id(database.pool())
            .await
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let knowledge_service = Arc::new(nomifun_knowledge::KnowledgeService::new(
            Arc::new(nomifun_db::SqliteKnowledgeRepository::new(
                database.pool().clone(),
            )),
            dir.path(),
            nomifun_knowledge::KnowledgeEventEmitter::new(
                Arc::new(NoopBroadcaster),
                Arc::from(owner_id),
            ),
        ));
        let service = Arc::new(LearningService::new(database.pool().clone()));
        service.set_generation_dependencies(knowledge_service, Arc::new(FakeCompleter));
        service.set_concept_graph_dir(dir.path().to_owned());
        (service, dir)
    }

    fn engine(service: Arc<LearningService>) -> LiveConceptGraphAgentEngine {
        LiveConceptGraphAgentEngine {
            service,
            deps: OneShotDeps {
                provider_repo: Arc::new(ListOnlyRepo(Vec::new())),
                provider_model_repo: Arc::new(ListOnlyModelRepo(Vec::new())),
                encryption_key: [0u8; 32],
                workspace: std::env::temp_dir(),
            },
        }
    }

    fn context(
        service: Arc<LearningService>,
    ) -> (
        Arc<LoopContext>,
        Arc<Mutex<Option<String>>>,
        Arc<Mutex<Option<ConceptGraphRecord>>>,
    ) {
        let draft_slot = Arc::new(Mutex::new(None));
        let published_slot = Arc::new(Mutex::new(None));
        let ctx = Arc::new(LoopContext {
            service,
            session: "test-session".into(),
            topic: "数学基础".into(),
            user_id: UserId::parse("0190f5fe-7c00-7a00-8000-000000000001").unwrap(),
            model_override: None,
            draft_slot: Arc::clone(&draft_slot),
            published_slot: Arc::clone(&published_slot),
        });
        (ctx, draft_slot, published_slot)
    }

    /// 安全不变量：发给模型的工具注册面恰等于构造的工具集——每一轮都如此，
    /// 且生成 loop 携带全部 8 个工具。
    #[tokio::test]
    async fn generation_loop_exposes_exactly_the_whitelist() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(service);
        let tools = concept_graph_tools(Arc::clone(&ctx), true);
        let names: Vec<String> = tools.iter().map(|tool| tool.name.clone()).collect();
        assert_eq!(
            names,
            vec![
                "cg_start", "cg_inspect", "cg_query", "cg_subgraph", "cg_audit", "cg_scope",
                "cg_patch", "cg_finish"
            ]
        );

        // Round 1: the model asks cg_inspect before any draft exists — the
        // handler must answer with a guidance error, never panic.
        let provider = ScriptedProvider::new(vec![
            vec![
                tool_use("cg_inspect", serde_json::json!({})),
                done(StopReason::ToolUse),
            ],
            vec![LlmEvent::TextDelta("understood".into()), done(StopReason::EndTurn)],
        ]);
        run_agent_loop(
            provider.clone(),
            "test-model",
            GENERATE_AGENT_SYSTEM,
            "数学基础",
            &tools,
            GENERATE_MAX_ROUNDS,
            AGENT_MAX_TOKENS,
            "generate",
            Some(ctx.as_ref()),
        )
        .await
        .unwrap();

        let seen = provider.seen_tool_names.lock().unwrap();
        assert_eq!(seen.len(), 2, "two model rounds");
        for round in seen.iter() {
            assert_eq!(round, &names, "tool registry must be exactly the whitelist on every round");
        }
        let thinking = provider.seen_thinking.lock().unwrap();
        assert!(
            thinking.iter().all(|t| matches!(t, Some(ThinkingConfig::Disabled))),
            "every round must explicitly disable thinking: {thinking:?}"
        );
        let rounds = provider.seen_messages.lock().unwrap();
        let followup = &rounds[1];
        let last = followup.last().unwrap();
        assert!(matches!(
            &last.content[0],
            ContentBlock::ToolResult { is_error: true, content, .. }
                if content.contains("cg_start")
        ));
    }

    /// 轮次上限：脚本无限工具调用时，loop 在 max_rounds 处终止并报错。
    #[tokio::test]
    async fn agent_loop_round_cap_returns_error() {
        let tool = OneShotTool {
            name: "ping".into(),
            description: "test".into(),
            input_schema: serde_json::json!({}),
            handler: one_shot_handler(|_| async { Ok("pong".to_owned()) }),
        };
        let provider = ScriptedProvider::new(vec![
            vec![tool_use("ping", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![tool_use("ping", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![tool_use("ping", serde_json::json!({})), done(StopReason::ToolUse)],
        ]);
        let error = run_agent_loop(
            provider,
            "test-model",
            "system",
            "user",
            std::slice::from_ref(&tool),
            2,
            AGENT_MAX_TOKENS,
            "test",
            None,
        )
        .await
        .unwrap_err();
        assert!(matches!(&error, AppError::BadGateway(message) if message.contains("exceeded 2 tool rounds")));
    }

    /// 空图必须被审计门禁拦截：生成 loop 里模型过早 cg_finish（一个单元
    /// 都没建）→ 发布被拒（empty_graph danger）→ 修复 loop 补建完整网络
    /// → 第二轮门禁通过并发布。绝不能以空图收尾（前端会渲染成空白图）。
    #[tokio::test]
    async fn empty_graph_is_blocked_until_repaired_and_published() {
        let (service, dir) = test_service().await;
        let (ctx, _draft, _published) = context(Arc::clone(&service));

        // Repair batch: n2/n3 branch off n1; n4/n5 converge two threads
        // each; n6 converges n4+n5; n7..n30 depend on n6 — 30 nodes, 3
        // multi-parent nodes (>= 10%), one component, depth 4.
        let mut operations = vec![
            add_op("n1", &[]),
            add_op("n2", &["n1"]),
            add_op("n3", &["n1"]),
            add_op("n4", &["n1", "n2", "n3"]),
            add_op("n5", &["n1", "n2"]),
            add_op("n6", &["n4", "n5"]),
        ];
        for index in 7..=30 {
            operations.push(add_op(&format!("n{index}"), &["n6"]));
        }

        let provider = ScriptedProvider::new(vec![
            // ── generation loop: cg_start, then a premature cg_finish ──
            vec![
                tool_use("cg_start", serde_json::json!({ "topic": "数学基础" })),
                done(StopReason::ToolUse),
            ],
            vec![tool_use("cg_finish", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![
                LlmEvent::TextDelta("发布被拒，需要先构建单元".into()),
                done(StopReason::EndTurn),
            ],
            // ── repair loop 1: build the whole network in one patch ──
            vec![
                tool_use(
                    "cg_patch",
                    serde_json::json!({ "operations": operations }),
                ),
                done(StopReason::ToolUse),
            ],
            vec![LlmEvent::TextDelta("已构建完整网络".into()), done(StopReason::EndTurn)],
        ]);
        let record = engine(Arc::clone(&service))
            .run_loops(provider.clone(), "test-model", "数学基础", ctx)
            .await
            .unwrap();
        assert_eq!(record.topic, "数学基础");
        assert_eq!(record.graph.nodes.len(), 30, "repaired graph publishes");
        assert_eq!(record.user_id, "0190f5fe-7c00-7a00-8000-000000000001");
        // The premature cg_finish was rejected with the empty_graph finding.
        let rounds = provider.seen_messages.lock().unwrap();
        let rejected = rounds.iter().flat_map(|round| round.iter()).any(|message| {
            matches!(
                &message.content[0],
                ContentBlock::ToolResult { is_error: true, content, .. }
                    if content.contains("empty_graph")
            )
        });
        assert!(rejected, "an empty graph must be rejected by the audit gate");
        // The JSON record was persisted like the legacy pipeline's output
        // (the directory also holds the concept-graph generation log).
        let files: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .collect();
        assert_eq!(files.len(), 1, "exactly one published record");
    }

    /// 修复 loop：生成 loop 只铺了 2 个单元（coverage danger）——发布被
    /// 门禁阻塞；修复 loop 补齐到 30 个且收敛结构（3 个多父节点）后通过。
    /// 修复 loop 的工具面必须不含 cg_start。
    #[tokio::test]
    async fn repair_loop_fixes_danger_findings_before_publish() {
        let (service, dir) = test_service().await;
        let (ctx, _draft, _published) = context(Arc::clone(&service));

        // Repair batch: n3 branches off n1; n4/n5 converge two threads each;
        // n6 converges n4+n5; n7..n30 depend on n6 — 30 nodes, 3 multi-parent
        // nodes (>= 10%), one component, depth 4.
        let mut operations = vec![
            add_op("n3", &["n1"]),
            add_op("n4", &["n1", "n2", "n3"]),
            add_op("n5", &["n1", "n2"]),
            add_op("n6", &["n4", "n5"]),
        ];
        for index in 7..=30 {
            operations.push(add_op(&format!("n{index}"), &["n6"]));
        }

        let provider = ScriptedProvider::new(vec![
            // ── generation loop ──
            vec![
                tool_use("cg_start", serde_json::json!({ "topic": "数学基础" })),
                done(StopReason::ToolUse),
            ],
            vec![
                tool_use(
                    "cg_patch",
                    serde_json::json!({
                        "operations": [
                            add_op("n1", &[]),
                            add_op("n2", &["n1"])
                        ]
                    }),
                ),
                done(StopReason::ToolUse),
            ],
            // The model declares the generation done WITHOUT cg_finish; the
            // gate blocks the publish (2 < 30 units) and the repair loop
            // starts.
            vec![LlmEvent::TextDelta("done".into()), done(StopReason::EndTurn)],
            // ── repair loop ──
            vec![
                tool_use("cg_patch", serde_json::json!({ "operations": operations })),
                done(StopReason::ToolUse),
            ],
            vec![LlmEvent::TextDelta("fixed".into()), done(StopReason::EndTurn)],
        ]);

        let record = engine(Arc::clone(&service))
            .run_loops(provider.clone(), "test-model", "数学基础", ctx)
            .await
            .unwrap();
        assert_eq!(record.graph.nodes.len(), 30);
        assert_eq!(record.graph.edges.len(), 33, "2 + 28 adds, 3 multi-parent joins");

        let seen = provider.seen_tool_names.lock().unwrap();
        assert_eq!(seen.len(), 5, "3 generation rounds + 2 repair rounds");
        for round in &seen[3..] {
            assert!(
                !round.iter().any(|name| name == "cg_start"),
                "the repair loop must never expose cg_start: {round:?}"
            );
        }
        assert_eq!(
            std::fs::read_dir(dir.path())
                .unwrap()
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
                .count(),
            1,
            "one published record"
        );
    }

    /// 模型从未调用 cg_start 就宣告完成：无草稿可发布，明确报错。
    #[tokio::test]
    async fn finishing_without_a_draft_fails() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(service.clone());
        let provider = ScriptedProvider::new(vec![vec![
            LlmEvent::TextDelta("nothing to do".into()),
            done(StopReason::EndTurn),
        ]]);
        let error = engine(service)
            .run_loops(provider, "test-model", "数学基础", ctx)
            .await
            .unwrap_err();
        assert!(matches!(&error, AppError::Internal(message) if message.contains("without creating a draft")));
    }

    /// 修复预算耗尽：每次修复 loop 后审计仍有 danger，最终返回阻塞报告
    /// 而非静默失败。
    #[tokio::test]
    async fn repair_budget_exhaustion_reports_blocking_findings() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(Arc::clone(&service));
        let provider = ScriptedProvider::new(vec![
            // generation loop: one unit only
            vec![
                tool_use("cg_start", serde_json::json!({ "topic": "数学基础" })),
                done(StopReason::ToolUse),
            ],
            vec![
                tool_use("cg_patch", serde_json::json!({ "operations": [add_op("n1", &[])] })),
                done(StopReason::ToolUse),
            ],
            vec![LlmEvent::TextDelta("done".into()), done(StopReason::EndTurn)],
            // repair loop 1: refuses to touch anything, then stops
            vec![LlmEvent::TextDelta("cannot fix".into()), done(StopReason::EndTurn)],
            // repair loop 2
            vec![LlmEvent::TextDelta("cannot fix".into()), done(StopReason::EndTurn)],
            // repair loop 3
            vec![LlmEvent::TextDelta("cannot fix".into()), done(StopReason::EndTurn)],
        ]);
        let error = engine(Arc::clone(&service))
            .run_loops(provider.clone(), "test-model", "数学基础", ctx)
            .await
            .unwrap_err();
        assert!(matches!(&error, AppError::UnprocessableEntity(message) if message.contains("exhausted 3 repair loops")));
        let seen = provider.seen_tool_names.lock().unwrap();
        assert_eq!(seen.len(), 6, "1 generation round + 2 generation rounds + 3 repair rounds");
    }
}

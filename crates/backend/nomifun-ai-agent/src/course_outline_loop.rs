//! Two-loop agent engine for course outline generation — the course sibling
//! of [`crate::learning_graph_loop`]. The generation loop (`co_start` →
//! optional `co_read` grounding → batched `co_patch` builds → `co_audit`
//! self-check) drives the draft, then audit-gated repair rounds drive
//! publishing.
//!
//! Same layering as the concept graph engine: nomifun-learning holds only
//! [`CourseOutlineAgentEngine`]; this crate provides the provider-backed
//! implementation, and the app layer wires it via
//! `LearningService::set_course_outline_engine`. The loop mechanics, the
//! fail-closed whitelist and the audit gate contract are shared with the
//! concept graph through [`crate::loop_core`]; this module contributes the
//! outline prompts, the `co_*` tool set and the draft/publish context.
//!
//! Both grounding flows run through this one engine: the description flow
//! (brief text only, no `co_read`) and the kb flow (sampled files readable
//! via `co_read`) differ only in the brief and the exposed tool set.

use std::sync::{Arc, Mutex};

use nomi_providers::{LlmProvider, create_provider};
use nomifun_common::{AppError, ProviderId};
use nomifun_learning::{
    Blueprint, CourseOutlineAgentEngine, LearningService, OutlineBrief, OutlineOp, OutlineQuery,
};

use crate::factory::provider_config::resolve_provider_config;
use crate::knowledge_completer::resolve_default_model;
use crate::learning_loop::{
    CANCEL_MESSAGE, CancellableProvider, FlowCycle, LoopBudgets, LoopChannel, WireConfig, run_loops,
};
use crate::loop_core::{LoopEventSink, json_compact};
use crate::one_shot::{OneShotDeps, OneShotTool, one_shot_handler};

/// 课程大纲循环的显式预算表（ADR-0004）：与 loop_core 的共享默认逐字一致
/// （50 轮 / 8192 token / 600s）——显式声明而非隐式继承，每流的生效值在
/// 自己的文件里可见。
const BUDGETS: LoopBudgets = LoopBudgets {
    generate_max_rounds: crate::loop_core::GENERATE_MAX_ROUNDS,
    round_tokens: crate::loop_core::AGENT_MAX_TOKENS,
    timeout_secs: crate::loop_core::TOTAL_TIMEOUT_SECS,
};

/// 线上翻译差异表（ADR-0004）：大纲流无 kind 标记、无轮次日志、不翻译
/// round_feedback、start 帧不带 phase（因此不上线）、走课程事件流。
const WIRE: WireConfig = WireConfig {
    kind_tag: None,
    round_log_gen_label: None,
    translate_round_feedback: false,
    generate_start_phase: None,
    repair_start_phase: None,
    lesson_stream: false,
};

/// Generation-loop system prompt. The model builds the whole outline via the
/// draft tools; the audit gate still has the last word at `co_finish`.
const GENERATE_AGENT_SYSTEM: &str = r#"你是一名课程大纲设计代理：把给定的课程简报（自由文本描述或知识库采样）设计成一门结构完整、可直接开课的课程——模块（module）、课时（lesson）、概念（concept），并通过工具逐步构建。
- 课程规模由你决策：根据简报/采样资料的范围与复杂度决定模块数与每模块课时数——小而聚焦的主题一两节课即可讲透，体系庞大的主题则需要更多模块与课时；每个课时都必须有实质内容，不要为凑数拆分，也不要为省事硬塞。
- 概念是课时之间的知识锚点：key 全局唯一且稳定（snake_case 或简短英文短语），title 是名词短语；概念之间用 prerequisites 画依赖（必须无环、不得自引用）；每个课时绑定 2-5 个概念 key。
- 课时：title 是学习目标句（学完能做什么），purpose 一句话说清这节课解决什么问题、用什么方式；概念绑定必须取自已存在的概念 key。
- kb 流：每个课时的 source 必须是采样文件的真实路径（co_start 返回的清单或 co_read 见到的路径）；描述流省略 source。
- 模块从易到难排列，整体形成一条连续的学习路径；模块 title 简短，课时内容不越界到相邻模块的主题。

【工具使用纪律】
1. 第一步必须调用 co_start 创建草稿——它会先做范围分析，返回 draft_id 与（kb 流）采样文件清单。
2. kb 流：动手设计前先用 co_read 阅读采样文件（至少浏览与主题最相关的若干文件），让大纲真正落在资料上；描述流：简报是唯一 grounding，逐条落实简报中的要点。
3. 每次 co_patch 前后用 co_inspect 掌握全局；patch 里引用的模块/课时/概念 key 必须与草稿中完全一致（或引用本批前面创建的 key），拿不准先 co_query。
4. 分批构建：每批少于 25 个操作，宁可多批，不要超长批次。
5. 全部构建完成后调用 co_audit 自查；确认没有 danger 级问题才调用 co_finish。scope 覆盖按标题判定：任一模块/课时/概念的 title 含块文本即算覆盖（单字块如「栈」只需任一标题含该字）。

【结束条件】
- 只有 co_audit 报告无 danger 时才调用 co_finish；被门禁拒绝时按报告继续修复。"#;

/// Repair-loop system prompt: the audit report is the ONLY repair basis; the
/// model patches locally and never rebuilds the outline wholesale.
const REPAIR_AGENT_SYSTEM: &str = r#"你是一名课程大纲修复代理：基于确定性审计报告，精确修复大纲中被指出的问题。

【修复原则】
1. 审计报告是主要的修复依据：逐条处理 danger 级 findings，按报告给出的证据（模块/课时/概念的 key、缺口清单）精确操作；
2. 不做过大的重构、不推翻已通过的结构：只修补报告指出的问题。
3. 动手前可以用 co_inspect / co_query 查清 key 的精确写法；引用不一致的操作会被拒绝并附最近似候选。
4. 修复动作分批提交（每批 5-20 个操作），每批后用 co_audit 复查对应 finding 是否消除。
5. 全部 danger 消除后调用 co_finish 发布。

【常见修复动作对照】
- 课时缺 title|purpose|concepts：update_lesson 补齐缺失字段。
- 未知概念引用：add_concept 创建缺失概念，或 update_lesson 把课时改绑到已有概念。
- 重复 key：update_module / update_lesson / update_concept 重命名其中一个，或 remove 多余项。
- 自引用 / 环：unlink_prereq 断开成环的边。
- 孤儿概念：把概念绑定进课时（add_lesson / update_lesson 的 concepts），或 remove_concept 删除。
- scope 覆盖缺口：为缺失的 scope 块补建模块与课时。覆盖按标题判定：任一模块/课时/概念 title 含块文本即算覆盖，单字块只需任一标题含该字——最省事的修法是把块词写进某个课时的标题。

【结束条件】
- 审计无 danger 时调用 co_finish；若 co_finish 被拒绝，认真阅读返回的阻塞报告并继续修复。
- 禁止空手结束：每一轮都必须调用 co_patch 执行修复动作（或 co_audit 复查、co_finish 尝试发布）；只输出文字而不调用任何工具，会被判定为拒绝修复，整个生成以失败告终。
- 回复使用中文。"#;

/// Provider-backed engine for the two-loop course outline pipeline.
pub struct LiveCourseOutlineAgentEngine {
    pub service: Arc<LearningService>,
    pub deps: OneShotDeps,
}

#[async_trait::async_trait]
impl CourseOutlineAgentEngine for LiveCourseOutlineAgentEngine {
    async fn generate(
        &self,
        brief: &OutlineBrief,
        model_override: Option<(&str, &str)>,
    ) -> Result<Blueprint, AppError> {
        let (provider_id, model) = match model_override {
            Some((provider_id, model)) => (provider_id.to_owned(), model.to_owned()),
            None => resolve_default_model(&self.deps.provider_repo, &self.deps.provider_model_repo)
                .await
                .ok_or_else(|| {
                    AppError::Conflict(
                        "course outline generation unavailable: no enabled provider/model is configured"
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
        // 取消旗标：挂在服务端生成注册上，取消端点置位；包装后的 provider
        // 在每次 LLM 请求边界轮询（复用 learning_graph_loop 的
        // CancellableProvider），传统课程生成由此获得与学习图一致的取消能力。
        let provider: Arc<dyn LlmProvider> = Arc::new(CancellableProvider {
            inner: create_provider(&cfg),
            cancel: self.service.generation_cancel_flag(),
        });
        tracing::info!(
            kind = brief.kind(),
            provider = provider_id.as_str(),
            model = %model,
            "course outline generation start"
        );

        // The two slots are shared with the tool handlers: the draft the
        // model opened and the blueprint `co_finish` published. On timeout
        // the draft slot also carries the diagnostics (draft id -> live
        // audit report).
        let ctx = Arc::new(LoopContext {
            service: Arc::clone(&self.service),
            brief: brief.clone(),
            model_override: Some((provider_id, model.clone())),
            draft_slot: Arc::new(Mutex::new(None)),
            published_slot: Arc::new(Mutex::new(None)),
            channel: LoopChannel::new(WIRE, BUDGETS.generate_max_rounds),
        });

        match tokio::time::timeout(
            std::time::Duration::from_secs(BUDGETS.timeout_secs),
            run_loops(
                self,
                provider,
                &model,
                &brief_user_text(brief),
                Arc::clone(&ctx),
                BUDGETS.round_tokens,
            ),
        )
        .await
        {
            Ok(Ok(blueprint)) => {
                ctx.log("session_end", serde_json::json!({
                    "ok": true,
                    "modules": blueprint.modules.len(),
                    "concepts": blueprint.concepts.len(),
                }));
                Ok(blueprint)
            }
            Ok(Err(error)) => {
                // 用户取消是中性终态而非故障：收敛为一句干净消息（大纲草稿
                // 没有续建语义，不附草稿说明），其余失败原样透传。
                let error = if self.service.generation_cancel_requested()
                    || error.to_string().contains(CANCEL_MESSAGE)
                {
                    AppError::BadGateway(CANCEL_MESSAGE.to_owned())
                } else {
                    error
                };
                ctx.log("session_end", serde_json::json!({
                    "ok": false,
                    "error": error.to_string(),
                }));
                Err(error)
            }
            Err(_) => {
                let mut message =
                    format!("course outline agent timed out after {}s", BUDGETS.timeout_secs);
                if let Some(draft_id) =
                    ctx.draft_slot.lock().ok().and_then(|slot| slot.clone())
                {
                    match self.service.audit_course_outline_draft(&draft_id) {
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

/// 统一外壳（[`FlowCycle`]）的课程大纲插头：kb/描述双流仅由工具面差异承
/// 载（`co_read` 按采样有无、`co_start` 仅生成轮）；修复轮先发 `audit`
/// 进度帧再取审计全文；错误透传（取消归一在 generate 尾部）；错误优先于
/// 发布——历史顺序，显式保留（ADR-0004）。
#[async_trait::async_trait]
impl FlowCycle for LiveCourseOutlineAgentEngine {
    type Ctx = LoopContext;
    type Output = Blueprint;

    fn name(&self) -> &'static str {
        "course outline"
    }

    fn budgets(&self) -> LoopBudgets {
        BUDGETS
    }

    fn wire(&self) -> WireConfig {
        WIRE
    }

    fn generate_system(&self) -> &'static str {
        GENERATE_AGENT_SYSTEM
    }

    fn repair_system(&self) -> &'static str {
        REPAIR_AGENT_SYSTEM
    }

    fn tools(&self, ctx: Arc<LoopContext>, repair_face: bool) -> Vec<OneShotTool> {
        course_outline_tools(ctx, !repair_face)
    }

    async fn finish(&self, _ctx: &LoopContext, draft_id: &str) -> Result<Blueprint, AppError> {
        self.service.finish_course_outline_draft(draft_id)
    }

    fn revision(&self, _ctx: &LoopContext, draft_id: &str) -> Result<u64, AppError> {
        Ok(self.service.inspect_course_outline_draft(draft_id)?.revision)
    }

    fn repair_audit(
        &self,
        ctx: &LoopContext,
        draft_id: &str,
        round: usize,
    ) -> Result<String, AppError> {
        audit_report(ctx, draft_id, "repair", round)
    }

    fn exhausted_audit(&self, _ctx: &LoopContext, draft_id: &str) -> Result<String, AppError> {
        self.service.audit_course_outline_draft(draft_id)
    }

    fn publish_frame(
        output: &Blueprint,
        loop_label: &str,
        round: Option<usize>,
    ) -> serde_json::Value {
        let mut frame = serde_json::json!({
            "phase": loop_label,
            "modules": output.modules.len(),
        });
        if let Some(round) = round {
            frame["round"] = serde_json::json!(round);
        }
        frame
    }

    fn on_published(&self, _ctx: &LoopContext) {}

    fn idle_nudge_actions(&self) -> &'static str {
        "co_patch 执行修复动作，或以 co_finish 尝试发布"
    }

    fn map_loop_error(&self, _ctx: &LoopContext, error: AppError) -> AppError {
        error
    }

    fn publish_before_error(&self) -> bool {
        false
    }

    fn missing_draft_texts(&self) -> (&'static str, &'static str) {
        (
            "no draft created (co_start never called)",
            "course outline agent finished without creating a draft (co_start was never called)",
        )
    }

    fn slot_poisoned(&self) -> AppError {
        AppError::Internal("course outline draft slot poisoned".into())
    }

    fn draft_slot(ctx: &LoopContext) -> &Arc<Mutex<Option<String>>> {
        &ctx.draft_slot
    }

    fn published_slot(ctx: &LoopContext) -> &Arc<Mutex<Option<Blueprint>>> {
        &ctx.published_slot
    }
}

// ── Shared loop state ──────────────────────────────────────────────────────

/// Everything the tool handlers need, captured once per generation. The two
/// slots are the only mutable cross-round state: which draft is active and
/// which blueprint (if any) was published by `co_finish`. 线上翻译由统一
/// 外壳的 [`LoopChannel`] 承载（差异表见 [`WIRE`]）。
pub(crate) struct LoopContext {
    service: Arc<LearningService>,
    brief: OutlineBrief,
    model_override: Option<(ProviderId, String)>,
    draft_slot: Arc<Mutex<Option<String>>>,
    published_slot: Arc<Mutex<Option<Blueprint>>>,
    channel: LoopChannel,
}

impl LoopContext {
    /// Mirror loop events onto the WebSocket progress stream (the shared
    /// progress channel — no session files). Best-effort and never fails
    /// the caller.
    fn log(&self, event: &str, fields: serde_json::Value) {
        self.channel.emit(&self.service, event, &fields);
    }

    fn require_draft(&self) -> Result<String, String> {
        self.draft_slot
            .lock()
            .map_err(|_| "internal draft slot lock failed".to_owned())?
            .clone()
            .ok_or_else(|| "没有活动的草稿——请先调用 co_start".to_owned())
    }
}

impl LoopEventSink for LoopContext {
    fn log(&self, event: &str, fields: serde_json::Value) {
        LoopContext::log(self, event, fields);
    }
}

/// Live audit snapshot for the repair loop: fetches the full findings text
/// (the model's repair basis) and logs the `audit` progress frame with the
/// severity counts and up to five danger examples (the UI's audit badges).
/// Severity literals match the concept graph's SEV_DANGER/SEV_WARNING/
/// SEV_INFO vocabulary.
fn audit_report(
    ctx: &LoopContext,
    draft_id: &str,
    loop_label: &str,
    round: usize,
) -> Result<String, AppError> {
    let report = ctx.service.audit_course_outline_draft(draft_id)?;
    let findings = ctx
        .service
        .inspect_course_outline_draft(draft_id)?
        .findings;
    let count = |severity: &str| {
        findings
            .iter()
            .filter(|finding| finding.severity == severity)
            .map(|finding| finding.count)
            .sum::<usize>()
    };
    let top: Vec<String> = findings
        .iter()
        .filter(|finding| finding.severity == "danger")
        .flat_map(|finding| finding.examples.iter().take(2).cloned())
        .take(5)
        .collect();
    ctx.log(
        "audit",
        serde_json::json!({
            "phase": "audit",
            "loop": loop_label,
            "round": round,
            "danger": count("danger"),
            "warning": count("warning"),
            "info": count("info"),
            "top": top,
        }),
    );
    Ok(report)
}

/// The generation loop's user turn: the brief's grounding. The kb flow names
/// the sampled corpus and points at `co_read`; the description flow hands
/// over the brief text alone.
fn brief_user_text(brief: &OutlineBrief) -> String {
    let mut text = String::new();
    match (&brief.description, &brief.knowledge_base) {
        (Some(description), _) => {
            text.push_str("课程简报：\n");
            text.push_str(description.trim());
            text.push('\n');
        }
        (_, Some(kb)) => {
            text.push_str(&format!(
                "知识库名称：{}\n知识库描述：{}\n",
                kb.name.trim(),
                kb.description.trim()
            ));
            text.push_str(
                "该库的 markdown 文档已采样：co_start 会返回路径清单，用 co_read 按路径阅读。\n",
            );
        }
        _ => {}
    }
    if let Some(domain) = brief.domain.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
        text.push_str(&format!("领域标签：{domain}\n"));
    }
    text
}

// ── Tool set ───────────────────────────────────────────────────────────────

/// The `co_*` whitelist. `with_start` adds `co_start` (generation loop
/// only — the repair loop must never re-scope a finished draft, and an
/// unlisted tool name fails closed at the loop level). `co_read` is only
/// exposed on the kb flow (the description flow has no samples to read).
fn course_outline_tools(ctx: Arc<LoopContext>, with_start: bool) -> Vec<OneShotTool> {
    let mut tools = Vec::with_capacity(8);
    if with_start {
        tools.push(co_start(Arc::clone(&ctx)));
    }
    tools.push(co_inspect(Arc::clone(&ctx)));
    tools.push(co_query(Arc::clone(&ctx)));
    if !ctx.brief.samples.is_empty() {
        tools.push(co_read(Arc::clone(&ctx)));
    }
    tools.push(co_scope(Arc::clone(&ctx)));
    tools.push(co_patch(Arc::clone(&ctx)));
    tools.push(co_audit(Arc::clone(&ctx)));
    tools.push(co_finish(Arc::clone(&ctx)));
    tools
}

fn co_start(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "co_start".into(),
        description: "启动课程大纲草稿：先做范围分析（失败时降级为无 scope），返回 draft_id 与（kb 流）采样文件清单。这是你的第一个工具调用；幂等——已有活动草稿时直接返回现有草稿。".into(),
        input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        handler: one_shot_handler(move |_input| {
            let ctx = Arc::clone(&ctx);
            async move {
                if let Some(existing) = ctx
                    .draft_slot
                    .lock()
                    .map_err(|_| "co_start: 内部锁故障".to_owned())?
                    .clone()
                {
                    let view = ctx
                        .service
                        .inspect_course_outline_draft(&existing)
                        .map_err(|error| error.to_string())?;
                    return Ok(format!("已有活动草稿（co_start 幂等返回）：{}", json_compact(&view)));
                }
                let model_override = ctx
                    .model_override
                    .as_ref()
                    .map(|(provider, model)| (provider, model.as_str()));
                let view = ctx
                    .service
                    .create_course_outline_draft(ctx.brief.clone(), model_override)
                    .await
                    .map_err(|error| error.to_string())?;
                *ctx.draft_slot
                    .lock()
                    .map_err(|_| "co_start: 内部锁故障".to_owned())? = Some(view.draft_id.clone());
                Ok(format!("草稿已创建：{}", json_compact(&view)))
            }
        }),
    }
}

fn co_inspect(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "co_inspect".into(),
        description: "当前草稿全局概览：课程标题/描述、实际模块/课时/概念数、每个模块的课时清单、审计摘要、采样路径。每次 co_patch 前后调用，保持全局认知。".into(),
        input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        handler: one_shot_handler(move |_input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                let view = ctx
                    .service
                    .inspect_course_outline_draft(&draft_id)
                    .map_err(|error| error.to_string())?;
                Ok(json_compact(&view))
            }
        }),
    }
}

fn co_query(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "co_query".into(),
        description: "按子串过滤列出模块 / 课时 / 概念的 key 与标题。任何 co_patch 之前先查询，确保引用的 key 与草稿中完全一致。limit 上限 200，防止上下文爆炸。".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "filter": { "type": "string", "description": "key/标题的子串过滤（可选，空=不过滤）" },
                "limit": { "type": "integer", "minimum": 1, "maximum": 200, "description": "每类返回条数上限，默认 50" }
            }
        }),
        handler: one_shot_handler(move |input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                let query = OutlineQuery {
                    filter: input
                        .get("filter")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_owned(),
                    limit: input
                        .get("limit")
                        .and_then(|value| value.as_u64())
                        .map(|limit| limit as usize)
                        .unwrap_or(50)
                        .clamp(1, 200),
                };
                let list = ctx
                    .service
                    .query_course_outline_draft(&draft_id, query)
                    .map_err(|error| error.to_string())?;
                Ok(json_compact(&list))
            }
        }),
    }
}

fn co_read(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "co_read".into(),
        description: "按路径读取一个采样文件的内容（kb 流专用）。设计大纲前用它阅读采样文档，让课时与概念真正落在资料上；source 引用必须是这里见到的真实路径。".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "采样文件的精确路径（来自 co_start 返回的 sample_paths）" }
            },
            "required": ["path"]
        }),
        handler: one_shot_handler(move |input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                let path = input
                    .get("path")
                    .and_then(|value| value.as_str())
                    .map(str::trim)
                    .filter(|path| !path.is_empty())
                    .ok_or_else(|| "co_read: path 必须是采样文件路径".to_owned())?;
                match ctx
                    .service
                    .read_course_outline_draft_sample(&draft_id, path)
                    .map_err(|error| error.to_string())?
                {
                    Some(content) => Ok(content),
                    None => {
                        let paths = ctx
                            .service
                            .course_outline_draft_sample_paths(&draft_id)
                            .map_err(|error| error.to_string())?;
                        Err(format!(
                            "co_read: 路径 '{path}' 不在采样清单中；可用路径：{}",
                            paths.join(", ")
                        ))
                    }
                }
            }
        }),
    }
}

fn co_scope(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "co_scope".into(),
        description: "返回 scope 范围参考全文：大块概念清单——生成阶段的严格完备覆盖自查表。构建前先取回它，逐项核对你的计划，确保每个大块概念都落到某个课时。覆盖判定按标题：任一模块/课时/概念 title 含块文本即算覆盖。".into(),
        input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        handler: one_shot_handler(move |_input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                match ctx
                    .service
                    .scope_reference_course_outline_draft(&draft_id)
                    .map_err(|error| error.to_string())?
                {
                    Some(reference) => Ok(reference),
                    None => Ok("本草稿没有 scope 参考（范围分析不可用时降级）；以 co_audit 的覆盖检查为准".into()),
                }
            }
        }),
    }
}

fn co_patch(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "co_patch".into(),
        description: "批量应用大纲操作（set_meta / add|update|remove_module / add|update|remove_lesson / add|update|remove_concept / link|unlink_prereq），一次调用就是一个批次。操作按数组顺序执行，后面的操作可以引用本批前面创建的 key。返回每个操作的成功/拒绝明细 + 最新审计摘要。\n\n调用示例：\n{\"operations\": [\n  {\"op\": \"set_meta\", \"title\": \"期权入门\", \"description\": \"零基础七节课\"},\n  {\"op\": \"add_module\", \"key\": \"m1\", \"title\": \"合约基础\"},\n  {\"op\": \"add_concept\", \"key\": \"option_def\", \"title\": \"期权定义\"},\n  {\"op\": \"add_lesson\", \"module\": \"m1\", \"key\": \"l1\", \"title\": \"认识期权合约\", \"purpose\": \"理解权利与义务的不对称\", \"concepts\": [\"option_def\"], \"source\": \"docs/basics.md\"}\n]}\n\n引用规则：module/模块 key、概念 key、prerequisites 必须与草稿中已有 key（或本批前面创建的 key）完全一致，不一致的操作会被拒绝并附最近似的 key 提示。每批 <25 个操作，宁多批勿超长。".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "operations": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 30,
                    "description": "大纲操作数组（按顺序执行，可引用本批前面创建的 key）；每个元素是带 op 标签的对象，见工具描述",
                    "items": { "type": "object" }
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
                let ops: Vec<OutlineOp> = serde_json::from_value(operations)
                    .map_err(|error| format!("co_patch: operations 必须是大纲操作数组——{error}"))?;
                if ops.is_empty() {
                    return Err("co_patch: operations 不能为空".into());
                }
                let report = ctx
                    .service
                    .patch_course_outline_draft(&draft_id, ops)
                    .map_err(|error| error.to_string())?;
                Ok(json_compact(&report))
            }
        }),
    }
}

fn co_audit(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "co_audit".into(),
        description: "完整确定性审计报告：每条 finding 的级别、类型、证据与修复提示，以及 scope 对照覆盖情况。发布前的最终自查也用它；存在 danger 时 co_finish 会被门禁拒绝。".into(),
        input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        handler: one_shot_handler(move |_input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                ctx.service
                    .audit_course_outline_draft(&draft_id)
                    .map_err(|error| error.to_string())
            }
        }),
    }
}

fn co_finish(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "co_finish".into(),
        description: "发布草稿为最终课程大纲（Blueprint）。确定性审计门禁有最终决定权：存在 danger 级 findings 时发布被阻塞，返回完整阻塞报告（草稿保留，可继续修复）。只有 co_audit 确认无 danger 时才调用。".into(),
        input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        handler: one_shot_handler(move |_input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                match ctx.service.finish_course_outline_draft(&draft_id) {
                    Ok(blueprint) => {
                        let lessons: usize =
                            blueprint.modules.iter().map(|module| module.lessons.len()).sum();
                        *ctx.published_slot
                            .lock()
                            .map_err(|_| "co_finish: 内部锁故障".to_owned())? =
                            Some(blueprint.clone());
                        Ok(format!(
                            "课程大纲已通过门禁：\"{}\"，{} 个模块 / {lessons} 个课时 / {} 个概念。",
                            blueprint.title,
                            blueprint.modules.len(),
                            blueprint.concepts.len()
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
// learning-graph, course-outline and lesson-content loops.

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_providers::ProviderError;
    use nomi_types::llm::{LlmEvent, ThinkingConfig};
    use nomi_types::message::{ContentBlock, StopReason};
    use nomifun_learning::KnowledgeBaseBrief;

    use crate::learning_loop::test_support::{
        ScriptedProvider, done, test_deps, test_service, tool_use,
    };
    use crate::loop_core::{GENERATE_REASONING_EFFORT, run_agent_loop};

    /// A complete 2×2 outline batch (the exact target size, all lessons
    /// titled/purposed/bound) that clears the audit gate. `with_source`
    /// attaches the kb-flow sample path — the kb flow REQUIRES it (the
    /// blueprint gate validates sources against the sampled corpus), while
    /// the description flow rejects any source (nothing is sampled).
    fn full_outline_ops(with_source: bool) -> serde_json::Value {
        let source = if with_source {
            serde_json::json!("docs/basics.md")
        } else {
            serde_json::Value::Null
        };
        serde_json::json!({ "operations": [
            { "op": "set_meta", "title": "测试课程", "description": "两模块入门课" },
            { "op": "add_module", "key": "m1", "title": "模块一" },
            { "op": "add_module", "key": "m2", "title": "模块二" },
            { "op": "add_concept", "key": "c1", "title": "概念一" },
            { "op": "add_concept", "key": "c2", "title": "概念二", "prerequisites": ["c1"] },
            { "op": "add_lesson", "module": "m1", "key": "l1", "title": "课时一", "purpose": "学会一", "concepts": ["c1"], "source": source },
            { "op": "add_lesson", "module": "m1", "key": "l2", "title": "课时二", "purpose": "学会二", "concepts": ["c1", "c2"], "source": source },
            { "op": "add_lesson", "module": "m2", "key": "l3", "title": "课时三", "purpose": "学会三", "concepts": ["c2"], "source": source },
            { "op": "add_lesson", "module": "m2", "key": "l4", "title": "课时四", "purpose": "学会四", "concepts": ["c1", "c2"], "source": source }
        ] })
    }

    fn kb_brief() -> OutlineBrief {
        OutlineBrief {
            description: None,
            knowledge_base: Some(KnowledgeBaseBrief {
                kb_id: "0190f5fe-7c00-7a00-8abc-012345678961".into(),
                name: "金融知识库".into(),
                description: "期权与交易基础笔记".into(),
            }),
            samples: vec![("docs/basics.md".into(), "期权的定义……".into())],
            domain: Some("trading".into()),
        }
    }

    fn description_brief() -> OutlineBrief {
        OutlineBrief {
            description: Some("零基础期权入门：从权利义务讲到期权策略".into()),
            knowledge_base: None,
            samples: Vec::new(),
            domain: None,
        }
    }

    fn engine(service: Arc<LearningService>) -> LiveCourseOutlineAgentEngine {
        LiveCourseOutlineAgentEngine {
            service,
            deps: test_deps(),
        }
    }

    fn context(
        service: Arc<LearningService>,
        brief: OutlineBrief,
    ) -> (
        Arc<LoopContext>,
        Arc<Mutex<Option<String>>>,
        Arc<Mutex<Option<Blueprint>>>,
    ) {
        let draft_slot = Arc::new(Mutex::new(None));
        let published_slot = Arc::new(Mutex::new(None));
        let ctx = Arc::new(LoopContext {
            service,
            brief,
            model_override: None,
            draft_slot: Arc::clone(&draft_slot),
            published_slot: Arc::clone(&published_slot),
            channel: LoopChannel::new(WIRE, BUDGETS.generate_max_rounds),
        });
        (ctx, draft_slot, published_slot)
    }

    /// 安全不变量：发给模型的工具注册面恰等于构造的工具集——每一轮都如此；
    /// kb 流生成 loop 携带全部 8 个工具（含 co_read），描述流省略 co_read。
    #[tokio::test]
    async fn generation_loop_exposes_exactly_the_whitelist() {
        let (service, _dir) = test_service().await;
        // kb flow: co_read is part of the whitelist.
        let (ctx, _draft, _published) = context(service.clone(), kb_brief());
        let names: Vec<String> = course_outline_tools(Arc::clone(&ctx), true)
            .iter()
            .map(|tool| tool.name.clone())
            .collect();
        assert_eq!(
            names,
            vec![
                "co_start", "co_inspect", "co_query", "co_read", "co_scope", "co_patch",
                "co_audit", "co_finish"
            ]
        );
        // Description flow: no samples → no co_read.
        let (desc_ctx, _draft, _published) = context(service, description_brief());
        let desc_names: Vec<String> = course_outline_tools(Arc::clone(&desc_ctx), true)
            .iter()
            .map(|tool| tool.name.clone())
            .collect();
        assert_eq!(
            desc_names,
            vec![
                "co_start", "co_inspect", "co_query", "co_scope", "co_patch", "co_audit",
                "co_finish"
            ]
        );

        // Round 1: the model asks co_inspect before any draft exists — the
        // handler must answer with a guidance error, never panic.
        let provider = ScriptedProvider::new(vec![
            vec![
                tool_use("co_inspect", serde_json::json!({})),
                done(StopReason::ToolUse),
            ],
            vec![LlmEvent::TextDelta("understood".into()), done(StopReason::EndTurn)],
        ]);
        run_agent_loop(
            provider.clone(),
            "test-model",
            GENERATE_AGENT_SYSTEM,
            &brief_user_text(&ctx.brief),
            &course_outline_tools(Arc::clone(&ctx), true),
            BUDGETS.generate_max_rounds,
            BUDGETS.round_tokens,
            ThinkingConfig::Disabled,
            GENERATE_REASONING_EFFORT,
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
                if content.contains("co_start")
        ));
    }

    /// 空草稿必须被审计门禁拦截：生成 loop 里模型过早 co_finish（尺寸
    /// 0≠2×2 的 danger）→ 发布被拒 → 修复 loop 一次补全 → 第二轮门禁
    /// 通过并发布。
    #[tokio::test]
    async fn empty_draft_is_blocked_until_repaired_and_published() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(Arc::clone(&service), kb_brief());

        let provider = ScriptedProvider::new(vec![
            // ── generation loop: co_start, then a premature co_finish ──
            vec![tool_use("co_start", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![tool_use("co_finish", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![
                LlmEvent::TextDelta("发布被拒，需要先构建大纲".into()),
                done(StopReason::EndTurn),
            ],
            // ── repair loop 1: build the whole outline in one patch ──
            vec![tool_use("co_patch", full_outline_ops(true)), done(StopReason::ToolUse)],
            vec![LlmEvent::TextDelta("已补全大纲".into()), done(StopReason::EndTurn)],
        ]);
        let blueprint = run_loops(
            &engine(Arc::clone(&service)),
            provider.clone(),
            "test-model",
            &brief_user_text(&ctx.brief),
            ctx,
            BUDGETS.round_tokens,
            )
            .await
            .unwrap();
        assert_eq!(blueprint.title, "测试课程");
        assert_eq!(blueprint.modules.len(), 2, "repaired outline publishes");
        assert_eq!(blueprint.domain, "trading", "the brief's domain label rides along");
        // The premature co_finish was rejected with a size finding.
        let rounds = provider.seen_messages.lock().unwrap();
        let rejected = rounds.iter().flat_map(|round| round.iter()).any(|message| {
            matches!(
                &message.content[0],
                ContentBlock::ToolResult { is_error: true, content, .. }
                    if content.contains("audit gate")
            )
        });
        assert!(rejected, "an empty draft must be rejected by the audit gate");
    }

    /// 修复 loop：生成 loop 只铺了 1 个模块（尺寸 danger）——发布被门禁
    /// 阻塞；修复 loop 补齐到 2×2 后通过。修复 loop 的工具面必须不含
    /// co_start，且模型在修复 loop 里直接 co_finish 也能发布。
    #[tokio::test]
    async fn repair_loop_fixes_danger_findings_before_publish() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(Arc::clone(&service), description_brief());

        let provider = ScriptedProvider::new(vec![
            // ── generation loop ──
            vec![tool_use("co_start", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![
                tool_use(
                    "co_patch",
                    serde_json::json!({ "operations": [
                        { "op": "set_meta", "title": "测试课程" },
                        { "op": "add_module", "key": "m1", "title": "模块一" }
                    ] }),
                ),
                done(StopReason::ToolUse),
            ],
            // The model declares the generation done WITHOUT co_finish; the
            // gate blocks the publish (1 module < 2) and the repair loop
            // starts.
            vec![LlmEvent::TextDelta("done".into()), done(StopReason::EndTurn)],
            // ── repair loop: finish the outline and publish in-loop ──
            vec![tool_use("co_patch", full_outline_ops(false)), done(StopReason::ToolUse)],
            vec![tool_use("co_finish", serde_json::json!({})), done(StopReason::ToolUse)],
            // The publish result comes back as a tool result; the model
            // wraps up with one final (text-only) round.
            vec![LlmEvent::TextDelta("已发布".into()), done(StopReason::EndTurn)],
        ]);

        let blueprint = run_loops(
            &engine(Arc::clone(&service)),
            provider.clone(),
            "test-model",
            &brief_user_text(&ctx.brief),
            ctx,
            BUDGETS.round_tokens,
            )
            .await
            .unwrap();
        assert_eq!(blueprint.title, "测试课程");
        assert_eq!(blueprint.modules.len(), 2);
        assert_eq!(
            blueprint.modules.iter().map(|module| module.lessons.len()).sum::<usize>(),
            4
        );

        let seen = provider.seen_tool_names.lock().unwrap();
        assert_eq!(seen.len(), 6, "3 generation rounds + 3 repair rounds");
        for round in &seen[3..] {
            assert!(
                !round.iter().any(|name| name == "co_start"),
                "the repair loop must never expose co_start: {round:?}"
            );
        }
    }

    /// 模型从未调用 co_start 就宣告完成：无草稿可发布，明确报错。
    #[tokio::test]
    async fn finishing_without_a_draft_fails() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(service.clone(), description_brief());
        let provider = ScriptedProvider::new(vec![vec![
            LlmEvent::TextDelta("nothing to do".into()),
            done(StopReason::EndTurn),
        ]]);
        let error = run_loops(
            &engine(service),
            provider,
            "test-model",
            &brief_user_text(&ctx.brief),
            ctx,
            BUDGETS.round_tokens,
            )
            .await
            .unwrap_err();
        assert!(matches!(&error, AppError::Internal(message) if message.contains("without creating a draft")));
    }

    /// 修复预算耗尽：每次修复 loop 后审计仍有 danger，最终返回阻塞报告
    /// 而非静默失败。
    #[tokio::test]
    async fn repair_budget_exhaustion_reports_blocking_findings() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(Arc::clone(&service), description_brief());
        let provider = ScriptedProvider::new(vec![
            // generation loop: meta only, never the target size
            vec![tool_use("co_start", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![
                tool_use(
                    "co_patch",
                    serde_json::json!({ "operations": [
                        { "op": "set_meta", "title": "未完成课程" }
                    ] }),
                ),
                done(StopReason::ToolUse),
            ],
            vec![LlmEvent::TextDelta("done".into()), done(StopReason::EndTurn)],
            // repair loops 1-3: refuse to touch anything, then stop
            vec![LlmEvent::TextDelta("cannot fix".into()), done(StopReason::EndTurn)],
            vec![LlmEvent::TextDelta("cannot fix".into()), done(StopReason::EndTurn)],
            vec![LlmEvent::TextDelta("cannot fix".into()), done(StopReason::EndTurn)],
        ]);
        let error = run_loops(
            &engine(Arc::clone(&service)),
            provider.clone(),
            "test-model",
            &brief_user_text(&ctx.brief),
            ctx,
            BUDGETS.round_tokens,
            )
            .await
            .unwrap_err();
        assert!(matches!(&error, AppError::UnprocessableEntity(message) if message.contains("exhausted 3 repair loops")));
        let seen = provider.seen_tool_names.lock().unwrap();
        assert_eq!(seen.len(), 6, "2 generation rounds + 1 idle round + 3 repair rounds");
    }

    /// 瞬态 provider 故障（429）在流打开阶段重试一次：失败的打开不消耗
    /// 脚本轮次、不进入 seen 记录，重试后整条管线照常发布。
    #[tokio::test]
    async fn transient_stream_open_failure_is_retried_once() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(Arc::clone(&service), description_brief());
        let provider = ScriptedProvider::new(vec![
            // ── generation loop ──
            vec![tool_use("co_start", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![LlmEvent::TextDelta("done".into()), done(StopReason::EndTurn)],
            // ── repair loop 1: build and publish in-loop ──
            vec![tool_use("co_patch", full_outline_ops(false)), done(StopReason::ToolUse)],
            vec![tool_use("co_finish", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![LlmEvent::TextDelta("已发布".into()), done(StopReason::EndTurn)],
        ]);
        provider.open_failures.lock().unwrap().push(ProviderError::RateLimited {
            retry_after_ms: 1,
            message: "burst".into(),
        });

        let blueprint = run_loops(
            &engine(Arc::clone(&service)),
            provider.clone(),
            "test-model",
            &brief_user_text(&ctx.brief),
            ctx,
            BUDGETS.round_tokens,
            )
            .await
            .unwrap();
        assert_eq!(blueprint.title, "测试课程");
        assert_eq!(
            provider.seen_tool_names.lock().unwrap().len(),
            5,
            "the failed open consumed no scripted round"
        );
    }

    /// 不可重试的打开失败（4xx）立即失败：不重试、不拖时间。
    #[tokio::test]
    async fn terminal_stream_open_failure_fails_fast() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(Arc::clone(&service), description_brief());
        let provider = ScriptedProvider::new(vec![
            vec![tool_use("co_start", serde_json::json!({})), done(StopReason::ToolUse)],
        ]);
        provider
            .open_failures
            .lock()
            .unwrap()
            .push(ProviderError::Api { status: 401, message: "bad key".into() });

        let error = run_loops(
            &engine(Arc::clone(&service)),
            provider,
            "test-model",
            &brief_user_text(&ctx.brief),
            ctx,
            BUDGETS.round_tokens,
            )
            .await
            .unwrap_err();
        assert!(
            matches!(&error, AppError::BadGateway(message) if message.contains("API error 401")),
            "{error}"
        );
    }
}

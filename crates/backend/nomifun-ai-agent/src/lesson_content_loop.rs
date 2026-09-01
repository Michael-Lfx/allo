//! Two-loop agent engine for lesson content generation — the lesson sibling
//! of [`crate::course_outline_loop`]. The generation loop (`ls_start` →
//! `ls_set_document` → batched `ls_patch_activities` → `ls_audit` self-check)
//! drives the draft, then audit-gated repair rounds drive publishing.
//!
//! Same layering as the outline engine: nomifun-learning holds only
//! [`LessonContentAgentEngine`]; this crate provides the provider-backed
//! implementation, and the app layer wires it via
//! `LearningService::set_lesson_engine`. The loop mechanics, the fail-closed
//! whitelist and the audit gate contract are shared with the outline loop
//! through [`crate::loop_core`]; this module contributes the lesson prompts,
//! the `ls_*` tool set and the draft/publish context.
//!
//! The lesson loop reuses the fallback pipeline's exact document + activity
//! contracts (enforced deterministically by the lesson draft audit), so the
//! agent path and the legacy two-stage path produce interchangeable output.

use std::sync::{Arc, Mutex};

use nomi_providers::{LlmProvider, create_provider};
use nomifun_common::{AppError, ProviderId, UserId};
use nomifun_learning::{
    LessonContentAgentEngine, LessonGenerationContext, LessonOp, LessonOutput, LearningService,
};

use crate::factory::provider_config::resolve_provider_config;
use crate::knowledge_completer::resolve_default_model;
use crate::loop_core::{
    AGENT_MAX_TOKENS, GENERATE_MAX_ROUNDS, LoopEventSink, REPAIR_LOOP_LIMIT, REPAIR_MAX_ROUNDS,
    TOTAL_TIMEOUT_SECS, json_compact, log_text, run_agent_loop,
};
use crate::one_shot::{OneShotDeps, OneShotTool, one_shot_handler};

/// Generation-loop system prompt. The document contract mirrors the legacy
/// pipeline's `LESSON_DOCUMENT_STANDARD` / `LESSON_SYSTEM` rules; the
/// deterministic audit enforces them either way.
const GENERATE_LESSON_AGENT_SYSTEM: &str = r#"你是一名课时内容设计代理：为给定的一个课时撰写学习文档并设计检索活动，通过工具逐步构建，最终通过确定性审计门禁发布。

【学习文档契约】
- 文档是学员直接阅读的完整自学文本：纯 Markdown，不要 JSON、不要包裹代码围栏（文档内的 ```svg / ```jsxgraph 图形块属于文档本体）；直接以第一个 `## ` 标题行开头，以衔接下一课的收尾句结束。
- 长度是硬约束：去除空白后至少 800 字符（目标 1000-1500 中文字符）；审计会把不足判为 danger 阻断发布。
- 必需章节，顺序固定，各自以 `## ` 标题行开头：描述（完整精确地讲清本课教什么）→ 例子（1-3 个带真实步骤/数字/流程的具体示例）→ 验证（3-5 道自检题，至少 2 道客观题且与活动呼应）。可选章节（迁移/其他/关键词/推广等）按需自由添加，不要为凑数硬凑。
- 图形：内容真正需要图示时才画，每个图必须自足完整（命名点、标注、坐标刻度、说明文字）；静态图用 ```svg 块，交互图用 ```jsxgraph 块；图形块不计入长度下限，也不要为凑长度画图。
- 内容必须落在 grounding 上：引用摘录流忠于摘录，课程简报流忠于简报；不要发明资料之外的事实。用资料的主导语言书写。
- 动笔前对照任务给出的范围参考（课程完整目录，或学习图节点的前置/后续节点段落）：只写本课时范围内的内容，不越界讲后续课时/节点的主题，也不重复相邻或前置内容已覆盖的部分。

【活动契约】
- 3-5 个活动：至少 2 个客观题（single_choice / true_false / fill_in_blank）+ 反思题（宁少勿多，最多 3，通常恰好 1）。
- single_choice：3-5 个互不相同的选项，answer 恰等于其中一个选项。
- true_false：answer 是 JSON 布尔值。
- fill_in_blank：prompt 恰含一个 "___"；answer 是 1-3 个等价答案的 JSON 数组；必须提供近义干扰项 distractors 以迫使精细辨析。
- reflection：answer 必须是 null；各反思题合起来要覆盖本课全部概念。
- 概念绑定：concepts 只能取课时给定的概念 key（留空 = 绑定整课全部概念）；绝不绑定其他课时的概念。
- estimated_minutes：5-60 的小整数，反映文档长度（用 set_estimated_minutes 设置）。
- 题目、答案与解析必须有文档与 grounding 支撑。

【工具使用纪律】
1. 第一步必须调用 ls_start 创建草稿——它返回 draft_id 与课时上下文。
2. ls_set_document 一次写入完整文档（不要分片）；随后 ls_patch_activities 分批提交活动（每批 1-5 个操作）；ls_inspect 随时掌握草稿状态。
3. 全部构建完成后调用 ls_audit 自查；确认没有 danger 级问题才调用 ls_finish。

【结束条件】
- 只有 ls_audit 报告无 danger 时才调用 ls_finish；被门禁拒绝时按报告继续修复。"#;

/// Repair-loop system prompt: the audit report is the ONLY repair basis;
/// the model patches locally and never rewrites the lesson wholesale.
const REPAIR_LESSON_AGENT_SYSTEM: &str = r#"你是一名课时内容修复代理：基于确定性审计报告，精确修复学习文档与活动中被指出的问题。

【修复原则】
1. 审计报告是主要的修复依据：逐条处理 danger 级 findings，按报告给出的证据（文档章节、活动位置）精确操作。
2. 不推翻已通过的部分：文档缺章节/长度不足就 ls_set_document 重写补齐，活动形状不对就 ls_patch_activities 按位置更新。
3. 活动位置从 0 开始编号，以 ls_inspect 返回的活动清单为准；update_activity / remove_activity 引用的 position 必须与清单一致（注意先执行的操作会改变后续位置）。
4. 每批修复后用 ls_audit 复查对应 finding 是否消除；全部 danger 消除后调用 ls_finish 发布。

【常见修复动作对照】
- document_missing / document_invalid：ls_set_document 重写完整文档（≥800 字符、三必需章节按序）。
- activities_too_few / objective_activities_too_few：add_activity 补客观题。
- reflections_too_many：remove_activity 删多余的反思题。
- reflections_multiple（warning）：可保留——只有 danger 才阻断发布。
- activity_shape_invalid：update_activity 按位置重写该活动（选项数/答案形状/___ 空格/干扰项）。
- concept_binding_unknown：update_activity 把 concepts 改绑到课时给定的概念 key。

【结束条件】
- 审计无 danger 时调用 ls_finish；若 ls_finish 被拒绝，认真阅读返回的阻塞报告并继续修复。
- 禁止空手结束：每一轮都必须调用工具（ls_set_document / ls_patch_activities / ls_audit / ls_finish）；只输出文字而不调用任何工具，会被判定为拒绝修复，整个生成以失败告终。
- 回复使用中文。"#;

/// Provider-backed engine for the two-loop lesson content pipeline.
pub struct LiveLessonContentAgentEngine {
    pub service: Arc<LearningService>,
    pub deps: OneShotDeps,
}

#[async_trait::async_trait]
impl LessonContentAgentEngine for LiveLessonContentAgentEngine {
    async fn generate(
        &self,
        // Lesson content is a shared course asset: the caller identity is
        // accepted for trait uniformity but not used by the loop itself.
        _user_id: &UserId,
        context: &LessonGenerationContext,
        model_override: Option<(&str, &str)>,
    ) -> Result<LessonOutput, AppError> {
        let (provider_id, model) = match model_override {
            Some((provider_id, model)) => (provider_id.to_owned(), model.to_owned()),
            None => resolve_default_model(&self.deps.provider_repo, &self.deps.provider_model_repo)
                .await
                .ok_or_else(|| {
                    AppError::Conflict(
                        "lesson content generation unavailable: no enabled provider/model is configured"
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
        tracing::info!(
            course = %context.course_title,
            lesson = %context.lesson_title,
            provider = provider_id.as_str(),
            model = %model,
            "lesson content generation start"
        );

        // The two slots are shared with the tool handlers: the draft the
        // model opened and the lesson output `ls_finish` published. On
        // timeout the draft slot also carries the diagnostics.
        let ctx = Arc::new(LoopContext {
            service: Arc::clone(&self.service),
            context: context.clone(),
            draft_slot: Arc::new(Mutex::new(None)),
            published_slot: Arc::new(Mutex::new(None)),
        });

        match tokio::time::timeout(
            std::time::Duration::from_secs(TOTAL_TIMEOUT_SECS),
            self.run_loops(provider, &model, Arc::clone(&ctx)),
        )
        .await
        {
            Ok(Ok(output)) => {
                ctx.log("session_end", serde_json::json!({
                    "ok": true,
                    "activities": output.activities.len(),
                    "estimated_minutes": output.estimated_minutes,
                }));
                Ok(output)
            }
            Ok(Err(error)) => {
                ctx.log("session_end", serde_json::json!({
                    "ok": false,
                    "error": error.to_string(),
                }));
                Err(error)
            }
            Err(_) => {
                let mut message = format!("lesson content agent timed out after {TOTAL_TIMEOUT_SECS}s");
                if let Some(draft_id) =
                    ctx.draft_slot.lock().ok().and_then(|slot| slot.clone())
                {
                    match self.service.audit_lesson_draft(&draft_id) {
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

impl LiveLessonContentAgentEngine {
    /// Generation loop, then audit-gated repair loops. `provider` is
    /// injected so tests stub the LLM here (same seam as the outline
    /// engine).
    async fn run_loops(
        &self,
        provider: Arc<dyn LlmProvider>,
        model: &str,
        ctx: Arc<LoopContext>,
    ) -> Result<LessonOutput, AppError> {
        // ── Generation loop: full tool set, the lesson context as the user turn ──
        ctx.log("generate_loop_start", serde_json::json!({
            "max_rounds": GENERATE_MAX_ROUNDS,
            "tool_count": lesson_content_tools(Arc::clone(&ctx), true).len(),
        }));
        let generate_tools = lesson_content_tools(Arc::clone(&ctx), true);
        let final_text = run_agent_loop(
            provider.clone(),
            model,
            GENERATE_LESSON_AGENT_SYSTEM,
            &lesson_user_text(&ctx.context),
            &generate_tools,
            GENERATE_MAX_ROUNDS,
            AGENT_MAX_TOKENS,
            "generate",
            Some(ctx.as_ref()),
        )
        .await?;
        if let Some(output) = take_published(&ctx) {
            ctx.log("publish_ok", serde_json::json!({
                "phase": "generate",
                "activities": output.activities.len(),
            }));
            return Ok(output);
        }
        let draft_id = ctx
            .draft_slot
            .lock()
            .map_err(|_| AppError::Internal("lesson draft slot poisoned".into()))?
            .clone()
            .ok_or_else(|| {
                ctx.log("generate_loop_end", serde_json::json!({
                    "ok": false,
                    "reason": "no draft created (ls_start never called)",
                    "text": log_text(&final_text),
                }));
                AppError::Internal(
                    "lesson content agent finished without creating a draft (ls_start was never called)"
                        .into(),
                )
            })?;
        ctx.log("generate_loop_end", serde_json::json!({
            "ok": true,
            "draft_id": draft_id,
            "text": log_text(&final_text),
        }));

        // ── Repair loops: the audit gate has the last word ──────────────
        // `finish_lesson_draft` IS the deterministic gate: success means
        // the lesson cleared it, UnprocessableEntity means danger findings
        // remain and the repair loop gets the full report. `idle_nudge`
        // carries a warning into the next loop when the model ended a
        // repair round without touching the draft (revision unchanged).
        let mut idle_nudge: Option<String> = None;
        for round in 0..REPAIR_LOOP_LIMIT {
            ctx.log("repair_loop_start", serde_json::json!({
                "round": round + 1,
                "draft_id": draft_id,
            }));
            match ctx.service.finish_lesson_draft(&draft_id) {
                Ok(output) => {
                    ctx.log("publish_ok", serde_json::json!({
                        "phase": "repair",
                        "round": round + 1,
                        "activities": output.activities.len(),
                    }));
                    return Ok(output);
                }
                Err(AppError::UnprocessableEntity(_)) => {
                    ctx.log("finish_blocked", serde_json::json!({
                        "round": round + 1,
                        "draft_id": draft_id,
                    }));
                }
                Err(error) => return Err(error),
            }
            let audit = audit_report(&ctx, &draft_id, "repair", round + 1)?;
            let repair_user = match &idle_nudge {
                Some(nudge) => format!("{nudge}\n\n{audit}"),
                None => audit,
            };
            let repair_tools = lesson_content_tools(Arc::clone(&ctx), false);
            let revision_before = ctx
                .service
                .inspect_lesson_draft(&draft_id)?
                .revision;
            let final_text = run_agent_loop(
                provider.clone(),
                model,
                REPAIR_LESSON_AGENT_SYSTEM,
                &repair_user,
                &repair_tools,
                REPAIR_MAX_ROUNDS,
                AGENT_MAX_TOKENS,
                "repair",
                Some(ctx.as_ref()),
            )
            .await?;
            if let Some(output) = take_published(&ctx) {
                ctx.log("publish_ok", serde_json::json!({
                    "phase": "repair",
                    "round": round + 1,
                    "activities": output.activities.len(),
                }));
                return Ok(output);
            }
            let revision_after = ctx
                .service
                .inspect_lesson_draft(&draft_id)?
                .revision;
            if revision_after == revision_before {
                ctx.log("repair_loop_idle", serde_json::json!({
                    "round": round + 1,
                    "revision": revision_after,
                    "text": log_text(&final_text),
                }));
                idle_nudge = Some(format!(
                    "警告：你上一轮没有对草稿做任何修改（revision 仍是 {revision_after}），只回复了文字。\
                     禁止空手结束：本轮必须调用 ls_set_document / ls_patch_activities 执行修复动作，\
                     或以 ls_finish 尝试发布；只输出文字而不调用任何工具，会被判定为拒绝修复，\
                     整个生成将以失败告终。"
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
        let audit = ctx.service.audit_lesson_draft(&draft_id)?;
        ctx.log("repair_budget_exhausted", serde_json::json!({
            "draft_id": draft_id,
        }));
        Err(AppError::UnprocessableEntity(format!(
            "lesson content agent exhausted {REPAIR_LOOP_LIMIT} repair loops; the draft survives with these blocking findings:\n{audit}"
        )))
    }
}

// ── Shared loop state ──────────────────────────────────────────────────────

/// Everything the tool handlers need, captured once per generation. The two
/// slots are the only mutable cross-round state: which draft is active and
/// which lesson output (if any) was published by `ls_finish`.
struct LoopContext {
    service: Arc<LearningService>,
    context: LessonGenerationContext,
    draft_slot: Arc<Mutex<Option<String>>>,
    published_slot: Arc<Mutex<Option<LessonOutput>>>,
}

impl LoopContext {
    /// Mirror loop events onto the WebSocket progress stream (the shared
    /// progress channel — no session files). Best-effort and never fails
    /// the caller.
    fn log(&self, event: &str, fields: serde_json::Value) {
        self.emit_progress(event, &fields);
    }

    /// Translate loop-core log events into `learning.lesson-generation`
    /// frames — same translation as the outline loop's. `agent_round`
    /// carries the loop-core shape and is reshaped; audit events are logged
    /// with the WS payload shape already (a `phase` field) and pass through
    /// verbatim.
    fn emit_progress(&self, event: &str, fields: &serde_json::Value) {
        let payload = match event {
            "agent_round" => {
                let repair =
                    fields.get("loop").and_then(serde_json::Value::as_str) != Some("generate");
                serde_json::json!({
                    "phase": "round",
                    "loop": fields.get("loop"),
                    "round": fields.get("round"),
                    "max_rounds": if repair { REPAIR_MAX_ROUNDS } else { GENERATE_MAX_ROUNDS },
                    "tools": fields.get("tool_calls").cloned().unwrap_or_default(),
                    "text": fields.get("text").cloned().unwrap_or_default(),
                })
            }
            _other if fields.get("phase").is_some() => fields.clone(),
            _ => return,
        };
        self.service.emit_lesson_event(payload);
    }

    fn require_draft(&self) -> Result<String, String> {
        self.draft_slot
            .lock()
            .map_err(|_| "internal draft slot lock failed".to_owned())?
            .clone()
            .ok_or_else(|| "没有活动的草稿——请先调用 ls_start".to_owned())
    }
}

impl LoopEventSink for LoopContext {
    fn log(&self, event: &str, fields: serde_json::Value) {
        LoopContext::log(self, event, fields);
    }
}

/// Take the published lesson output out of the slot (once) — called after
/// every loop, because the model may legitimately `ls_finish` from either
/// loop.
fn take_published(ctx: &LoopContext) -> Option<LessonOutput> {
    ctx.published_slot
        .lock()
        .ok()
        .and_then(|mut slot| slot.take())
}

/// Live audit snapshot for the repair loop: fetches the full findings text
/// (the model's repair basis) and logs the `audit` progress frame with the
/// severity counts and up to five danger messages (the UI's audit badges).
fn audit_report(
    ctx: &LoopContext,
    draft_id: &str,
    loop_label: &str,
    round: usize,
) -> Result<String, AppError> {
    let report = ctx.service.audit_lesson_draft(draft_id)?;
    let findings = ctx.service.inspect_lesson_draft(draft_id)?.findings;
    let count = |severity: &str| {
        findings
            .iter()
            .filter(|finding| finding.severity == severity)
            .count()
    };
    let top: Vec<String> = findings
        .iter()
        .filter(|finding| finding.severity == "danger")
        .map(|finding| finding.message.clone())
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

/// The generation loop's user turn: the lesson coordinates, purpose,
/// concepts, bridging target, and the grounding (the cited excerpt for the
/// kb flow, the course brief for the description flow). Learning-graph
/// nodes (`context.graph` = Some) swap the outline/brief sections for
/// graph-scoped sections: goal, scope, the prerequisite path, and the
/// downstream nodes — and never bind concepts.
fn lesson_user_text(context: &LessonGenerationContext) -> String {
    let mut text = String::new();
    text.push_str(&format!("课程：{}\n", context.course_title.trim()));
    let graph = context.graph.as_ref();
    match graph {
        Some(graph) => {
            text.push_str(&format!(
                "学习图节点（「本节点」是你要写的课时——只写它的范围，不越界讲后续节点，不重复前置节点已覆盖的内容）：\n学习目标：{}\n学习范围：{}\n",
                graph.goal.trim(),
                graph.scope.trim()
            ));
        }
        None if !context.outline_tree.is_empty() => {
            text.push_str(&format!(
                "课程完整目录（「本课时」是你要写的课时——只写它的范围，不越界讲后续课时，不重复相邻课时）：\n{}\n",
                context.outline_tree
            ));
        }
        None => {}
    }
    match graph {
        Some(graph) => {
            text.push_str(&format!(
                "节点（{}/{}）：{}\n节点定位：{}\n",
                context.lesson_index + 1,
                context.total_lessons,
                context.lesson_title.trim(),
                context.purpose.trim()
            ));
            if graph.prerequisite_path.is_empty() {
                text.push_str("本节点没有前置——它是学习图的起点，从零讲起。\n");
            } else {
                text.push_str(&format!(
                    "前置路径（学习者到达本节点前应已掌握，按学习顺序，不要重复讲授）：\n{}\n",
                    graph.prerequisite_path
                ));
            }
            if graph.upcoming_nodes.is_empty() {
                text.push_str("本节点没有后续节点——结尾句做整个学习图的收束。\n");
            } else {
                text.push_str(&format!(
                    "后续节点（结尾句要为它们做衔接铺垫，但不要展开其内容）：\n{}\n",
                    graph.upcoming_nodes
                ));
            }
        }
        None => {
            text.push_str(&format!(
                "模块：{}\n课时（{}/{}）：{}\n课时目标：{}\n",
                context.module_title.trim(),
                context.lesson_index + 1,
                context.total_lessons,
                context.lesson_title.trim(),
                context.purpose.trim()
            ));
            if let Some(next) = context.next_lesson_title.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
                text.push_str(&format!("下一课：「{next}」——结尾句要衔接到它。\n"));
            } else {
                text.push_str("这是本模块最后一课——结尾句做本模块的收束。\n");
            }
        }
    }
    if graph.is_some() {
        text.push_str("学习图节点不绑定概念：活动的 concepts 一律留空数组。\n");
    } else {
        text.push_str("本课概念（活动的 concepts 只能绑定这些 key）：\n");
        for concept in &context.concepts {
            text.push_str(&format!(
                "- {} ({}) — {}\n",
                concept.key,
                concept.title,
                concept.description.trim()
            ));
        }
        for key in &context.concept_keys {
            if !context.concepts.iter().any(|concept| &concept.key == key) {
                text.push_str(&format!("- {key}\n"));
            }
        }
    }
    if !context.adjacent_context.is_empty() {
        text.push_str(&format!("\n{}\n", context.adjacent_context));
    }
    match &context.excerpt {
        Some(excerpt) => {
            text.push_str(&format!(
                "\n引用摘录（文档与活动必须忠于它）——文件 {}：\n---\n{}\n---\n",
                excerpt.path, excerpt.text
            ));
        }
        None if graph.is_some() => {
            text.push_str(
                "\n学习图节点没有课程简报：内容忠于学习目标、学习范围与前置/后续节点段落。\n",
            );
        }
        None => {
            let brief = context.course_description.trim();
            if brief.is_empty() {
                text.push_str("\n课程简报为空：以课时标题、目标与本课概念为准展开。\n");
            } else {
                text.push_str(&format!("\n课程简报（文档与活动必须忠于它）：\n{brief}\n"));
            }
        }
    }
    text
}

// ── Tool set ───────────────────────────────────────────────────────────────

/// The `ls_*` whitelist. `with_start` adds `ls_start` (generation loop only
/// — the repair loop must never re-scope the draft, and an unlisted tool
/// name fails closed at the loop level).
fn lesson_content_tools(ctx: Arc<LoopContext>, with_start: bool) -> Vec<OneShotTool> {
    let mut tools = Vec::with_capacity(6);
    if with_start {
        tools.push(ls_start(Arc::clone(&ctx)));
    }
    tools.push(ls_inspect(Arc::clone(&ctx)));
    tools.push(ls_set_document(Arc::clone(&ctx)));
    tools.push(ls_patch_activities(Arc::clone(&ctx)));
    tools.push(ls_audit(Arc::clone(&ctx)));
    tools.push(ls_finish(Arc::clone(&ctx)));
    tools
}

fn ls_start(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "ls_start".into(),
        description: "启动课时内容草稿：返回 draft_id、课时上下文与当前审计状态。这是你的第一个工具调用；幂等——已有活动草稿时直接返回现有草稿。".into(),
        input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        handler: one_shot_handler(move |_input| {
            let ctx = Arc::clone(&ctx);
            async move {
                if let Some(existing) = ctx
                    .draft_slot
                    .lock()
                    .map_err(|_| "ls_start: 内部锁故障".to_owned())?
                    .clone()
                {
                    let view = ctx
                        .service
                        .inspect_lesson_draft(&existing)
                        .map_err(|error| error.to_string())?;
                    return Ok(format!("已有活动草稿（ls_start 幂等返回）：{}", json_compact(&view)));
                }
                let view = ctx
                    .service
                    .create_lesson_draft(ctx.context.clone())
                    .map_err(|error| error.to_string())?;
                *ctx.draft_slot
                    .lock()
                    .map_err(|_| "ls_start: 内部锁故障".to_owned())? = Some(view.draft_id.clone());
                Ok(format!("草稿已创建：{}", json_compact(&view)))
            }
        }),
    }
}

fn ls_inspect(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "ls_inspect".into(),
        description: "当前草稿概览：文档字符数与章节清单、活动清单（position/kind/prompt 摘要）、estimated_minutes、审计 findings。每次修改前后调用，保持全局认知；update/remove 活动的 position 以这里为准。".into(),
        input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        handler: one_shot_handler(move |_input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                let view = ctx
                    .service
                    .inspect_lesson_draft(&draft_id)
                    .map_err(|error| error.to_string())?;
                Ok(json_compact(&view))
            }
        }),
    }
}

fn ls_set_document(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "ls_set_document".into(),
        description: "写入（或整体重写）学习文档：一次给全文的纯 Markdown——直接以第一个 `## ` 标题行开头，以衔接下一课的收尾句结束；不要 JSON、不要包裹围栏。返回应用结果与最新审计 findings。".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "document": { "type": "string", "description": "完整学习文档（Markdown 全文，一次写入）" }
            },
            "required": ["document"]
        }),
        handler: one_shot_handler(move |input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                let document = input
                    .get("document")
                    .and_then(|value| value.as_str())
                    .filter(|document| !document.trim().is_empty())
                    .ok_or_else(|| "ls_set_document: document 必须是非空 Markdown 文本".to_owned())?;
                let report = ctx
                    .service
                    .patch_lesson_draft(
                        &draft_id,
                        vec![LessonOp::SetDocument {
                            document: document.to_owned(),
                        }],
                    )
                    .map_err(|error| error.to_string())?;
                Ok(json_compact(&report))
            }
        }),
    }
}

fn ls_patch_activities(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "ls_patch_activities".into(),
        description: "批量应用活动操作（add_activity / update_activity / remove_activity / set_estimated_minutes），一次调用就是一个批次；操作按数组顺序执行，先执行的操作会改变后续操作的 position。返回每个操作的成功/拒绝明细 + 最新审计 findings。\n\n调用示例：\n{\"operations\": [\n  {\"op\": \"add_activity\", \"activity\": {\"kind\": \"single_choice\", \"prompt\": \"期权的本质是什么？\", \"options\": [\"权利\", \"义务\", \"债务\"], \"answer\": \"权利\", \"explanation\": \"买方持有的是权利\", \"concepts\": [\"option_def\"]}},\n  {\"op\": \"set_estimated_minutes\", \"minutes\": 15}\n]}\n\n字段规则：single_choice 3-5 个选项且 answer 恰等于其一；true_false 的 answer 是布尔；fill_in_blank 的 prompt 恰含一个 \"___\"、answer 是 1-3 个等价答案的数组且必须带 distractors；reflection 的 answer 必须是 null；concepts 只能取本课概念 key（留空 = 绑定整课）。每批 ≤10 个操作；写文档请用 ls_set_document。".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "operations": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 10,
                    "description": "活动操作数组（按顺序执行）；每个元素是带 op 标签的对象，见工具描述",
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
                let ops: Vec<LessonOp> = serde_json::from_value(operations)
                    .map_err(|error| format!("ls_patch_activities: operations 必须是活动操作数组——{error}"))?;
                if ops.is_empty() {
                    return Err("ls_patch_activities: operations 不能为空".into());
                }
                let report = ctx
                    .service
                    .patch_lesson_draft(&draft_id, ops)
                    .map_err(|error| error.to_string())?;
                Ok(json_compact(&report))
            }
        }),
    }
}

fn ls_audit(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "ls_audit".into(),
        description: "完整确定性审计报告：每条 finding 的级别、类型与证据（文档长度/章节、活动数量与形状、概念绑定）。发布前的最终自查也用它；存在 danger 时 ls_finish 会被门禁拒绝。".into(),
        input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        handler: one_shot_handler(move |_input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                ctx.service
                    .audit_lesson_draft(&draft_id)
                    .map_err(|error| error.to_string())
            }
        }),
    }
}

fn ls_finish(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "ls_finish".into(),
        description: "发布草稿为最终课时内容。确定性审计门禁有最终决定权：存在 danger 级 findings 时发布被阻塞，返回完整阻塞报告（草稿保留，可继续修复）。只有 ls_audit 确认无 danger 时才调用。".into(),
        input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        handler: one_shot_handler(move |_input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                match ctx.service.finish_lesson_draft(&draft_id) {
                    Ok(output) => {
                        let chars =
                            output.summary.chars().filter(|c| !c.is_whitespace()).count();
                        *ctx.published_slot
                            .lock()
                            .map_err(|_| "ls_finish: 内部锁故障".to_owned())? =
                            Some(output.clone());
                        Ok(format!(
                            "课时内容已通过门禁：文档 {chars} 字符 / {} 个活动 / 预计 {} 分钟。",
                            output.activities.len(),
                            output.estimated_minutes
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
    use nomi_types::llm::{LlmEvent, LlmRequest, ThinkingConfig};
    use nomi_types::message::{ContentBlock, Message, StopReason, TokenUsage};
    use nomifun_learning::{ConceptPack, GraphLessonContext, LessonExcerpt};
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

    /// ≥800 non-whitespace characters across the three required sections —
    /// clears the document half of the audit gate.
    fn long_document() -> String {
        let body = "这是一个用于测试的完整段落，覆盖课时要求的知识点并且足够长。".repeat(10);
        format!("## 描述\n{body}\n## 例子\n{body}\n## 验证\n{body}\n")
    }

    /// Three valid activities bound to c1 (2 objective + 1 reflection) —
    /// clears the activity half of the audit gate.
    fn valid_activity_ops() -> serde_json::Value {
        serde_json::json!({ "operations": [
            { "op": "add_activity", "activity": { "kind": "single_choice", "prompt": "期权的本质是什么？", "options": ["权利", "义务", "债务"], "answer": "权利", "explanation": "买方持有的是权利而非义务。", "concepts": ["c1"] } },
            { "op": "add_activity", "activity": { "kind": "true_false", "prompt": "期权卖方没有履约义务。", "answer": false, "explanation": "卖方承担履约义务。", "concepts": ["c1"] } },
            { "op": "add_activity", "activity": { "kind": "reflection", "prompt": "结合一个场景说明权利与义务的不对称。", "answer": null, "explanation": "", "concepts": ["c1"] } },
            { "op": "set_estimated_minutes", "minutes": 15 }
        ] })
    }

    fn lesson_context() -> LessonGenerationContext {
        LessonGenerationContext {
            course_title: "测试课程".into(),
            course_description: "零基础期权入门：从权利义务讲到期权策略".into(),
            module_title: "模块一".into(),
            module_index: 0,
            lesson_title: "课时一".into(),
            lesson_index: 0,
            total_lessons: 2,
            next_lesson_title: Some("课时二".into()),
            purpose: "理解期权的定义".into(),
            concepts: vec![ConceptPack {
                key: "c1".into(),
                title: "期权定义".into(),
                description: "权利与义务的不对称".into(),
                prerequisites: Vec::new(),
            }],
            concept_keys: vec!["c1".into()],
            excerpt: Some(LessonExcerpt {
                path: "docs/basics.md".into(),
                text: "期权的定义……".into(),
            }),
            outline_tree: String::new(),
            adjacent_context: String::new(),
            graph: None,
        }
    }

    /// 学习图节点的用户回合：图语义段落（目标/范围/前置路径/后续节点）取代
    /// 课程目录/模块/下一课段，概念绑定显式留空，课程简报句不渲染。
    #[test]
    fn lesson_user_text_renders_graph_sections_instead_of_outline() {
        let mut context = lesson_context();
        context.excerpt = None;
        context.graph = Some(GraphLessonContext {
            goal: "通盘认识期权交易".into(),
            scope: "聚焦场内标准期权，不含结构性产品".into(),
            prerequisite_path: "1. 什么是衍生品 — 先建立衍生品框架\n".into(),
            upcoming_nodes: "- 期权定价基础（为理解希腊字母铺垫）\n".into(),
        });
        let text = lesson_user_text(&context);
        assert!(text.contains("学习图节点"));
        assert!(text.contains("学习目标：通盘认识期权交易"));
        assert!(text.contains("学习范围：聚焦场内标准期权"));
        assert!(text.contains("前置路径"));
        assert!(text.contains("1. 什么是衍生品"));
        assert!(text.contains("后续节点"));
        assert!(text.contains("期权定价基础"));
        assert!(text.contains("concepts 一律留空数组"));
        // 传统段落不再出现。
        assert!(!text.contains("课程完整目录"));
        assert!(!text.contains("下一课"));
        assert!(!text.contains("课程简报为空"));
        assert!(!text.contains("课程简报（文档与活动必须忠于它）"));

        // 起点/终点节点：无前置与无后续的措辞分支。
        context.graph = Some(GraphLessonContext {
            goal: "通盘认识期权交易".into(),
            scope: "聚焦场内标准期权".into(),
            prerequisite_path: String::new(),
            upcoming_nodes: String::new(),
        });
        let text = lesson_user_text(&context);
        assert!(text.contains("从零讲起"));
        assert!(text.contains("整个学习图的收束"));
    }

    /// The user turn embeds the outline tree and the adjacent-lesson
    /// reference when present, and stays clean when both are empty.
    #[test]
    fn lesson_user_text_embeds_outline_and_adjacent_sections() {
        let mut context = lesson_context();
        context.outline_tree = "模块 1/1：模块一\n  1. 课时〇 — 铺垫\n  2. 课时一 — 理解期权的定义（本课时）".into();
        context.adjacent_context = "相邻课时参考：\n- 上一课时「课时〇」— 铺垫".into();
        let text = lesson_user_text(&context);
        assert!(text.contains("课程完整目录"));
        assert!(text.contains("（本课时）"));
        assert!(text.contains("相邻课时参考"));
        assert!(text.contains("课时〇"));

        let plain = lesson_user_text(&lesson_context());
        assert!(!plain.contains("课程完整目录"));
        assert!(!plain.contains("相邻课时参考"));
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

    /// A service wired with the fake completer (unused by lesson drafts)
    /// and a scratch generation dir; the temp dir stays alive for the
    /// test's duration.
    async fn test_service() -> (Arc<LearningService>, tempfile::TempDir) {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let owner_id =
            nomifun_db::installation_owner_id(database.pool()).await.unwrap();
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
        (service, dir)
    }

    struct FakeCompleter;

    #[async_trait::async_trait]
    impl nomifun_learning::LearningCompleter for FakeCompleter {
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

    fn engine(service: Arc<LearningService>) -> LiveLessonContentAgentEngine {
        LiveLessonContentAgentEngine {
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
        Arc<Mutex<Option<LessonOutput>>>,
    ) {
        let draft_slot = Arc::new(Mutex::new(None));
        let published_slot = Arc::new(Mutex::new(None));
        let ctx = Arc::new(LoopContext {
            service,
            context: lesson_context(),
            draft_slot: Arc::clone(&draft_slot),
            published_slot: Arc::clone(&published_slot),
        });
        (ctx, draft_slot, published_slot)
    }

    /// 安全不变量：发给模型的工具注册面恰等于构造的工具集——每一轮都如此；
    /// 生成 loop 携带全部 6 个工具（含 ls_start），修复 loop 省略 ls_start。
    #[tokio::test]
    async fn lesson_loop_exposes_exactly_the_whitelist() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(service.clone());
        let names: Vec<String> = lesson_content_tools(Arc::clone(&ctx), true)
            .iter()
            .map(|tool| tool.name.clone())
            .collect();
        assert_eq!(
            names,
            vec![
                "ls_start", "ls_inspect", "ls_set_document", "ls_patch_activities",
                "ls_audit", "ls_finish"
            ]
        );
        // Repair loop: no ls_start.
        let repair_names: Vec<String> = lesson_content_tools(Arc::clone(&ctx), false)
            .iter()
            .map(|tool| tool.name.clone())
            .collect();
        assert_eq!(
            repair_names,
            vec![
                "ls_inspect", "ls_set_document", "ls_patch_activities", "ls_audit",
                "ls_finish"
            ]
        );

        // Round 1: the model asks ls_inspect before any draft exists — the
        // handler must answer with a guidance error, never panic.
        let provider = ScriptedProvider::new(vec![
            vec![
                tool_use("ls_inspect", serde_json::json!({})),
                done(StopReason::ToolUse),
            ],
            vec![LlmEvent::TextDelta("understood".into()), done(StopReason::EndTurn)],
        ]);
        run_agent_loop(
            provider.clone(),
            "test-model",
            GENERATE_LESSON_AGENT_SYSTEM,
            &lesson_user_text(&ctx.context),
            &lesson_content_tools(Arc::clone(&ctx), true),
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
                if content.contains("ls_start")
        ));
    }

    /// 只写文档就过早 ls_finish（活动缺失 danger）→ 发布被门禁拒绝 →
    /// 修复 loop 补齐活动 → 下一轮门禁通过并发布。
    #[tokio::test]
    async fn document_only_draft_is_blocked_until_repaired_and_published() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(Arc::clone(&service));

        let provider = ScriptedProvider::new(vec![
            // ── generation loop: ls_start, the document, premature ls_finish ──
            vec![tool_use("ls_start", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![
                tool_use(
                    "ls_set_document",
                    serde_json::json!({ "document": long_document() }),
                ),
                done(StopReason::ToolUse),
            ],
            vec![tool_use("ls_finish", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![
                LlmEvent::TextDelta("发布被拒，需要先补活动".into()),
                done(StopReason::EndTurn),
            ],
            // ── repair loop 1: add the activities ──
            vec![tool_use("ls_patch_activities", valid_activity_ops()), done(StopReason::ToolUse)],
            vec![LlmEvent::TextDelta("已补齐活动".into()), done(StopReason::EndTurn)],
        ]);
        let output = engine(Arc::clone(&service))
            .run_loops(provider.clone(), "test-model", ctx)
            .await
            .unwrap();
        assert!(output.summary.contains("## 描述"));
        assert_eq!(output.activities.len(), 3, "repaired lesson publishes");
        assert_eq!(output.estimated_minutes, 15);
        // The premature ls_finish was rejected with the blocking report.
        let rounds = provider.seen_messages.lock().unwrap();
        let rejected = rounds.iter().flat_map(|round| round.iter()).any(|message| {
            matches!(
                &message.content[0],
                ContentBlock::ToolResult { is_error: true, content, .. }
                    if content.contains("audit gate")
            )
        });
        assert!(rejected, "a document-only draft must be rejected by the audit gate");
    }

    /// 修复 loop：生成 loop 只写了文档就宣告完成——发布被门禁阻塞；修复
    /// loop 补齐活动后直接 ls_finish 也能发布。修复 loop 的工具面必须不含
    /// ls_start。
    #[tokio::test]
    async fn repair_loop_fixes_danger_findings_before_publish() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(Arc::clone(&service));

        let provider = ScriptedProvider::new(vec![
            // ── generation loop ──
            vec![tool_use("ls_start", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![
                tool_use(
                    "ls_set_document",
                    serde_json::json!({ "document": long_document() }),
                ),
                done(StopReason::ToolUse),
            ],
            // The model declares the generation done WITHOUT ls_finish; the
            // gate blocks the publish (no activities) and the repair loop
            // starts.
            vec![LlmEvent::TextDelta("done".into()), done(StopReason::EndTurn)],
            // ── repair loop: add the activities and publish in-loop ──
            vec![tool_use("ls_patch_activities", valid_activity_ops()), done(StopReason::ToolUse)],
            vec![tool_use("ls_finish", serde_json::json!({})), done(StopReason::ToolUse)],
            // The publish result comes back as a tool result; the model
            // wraps up with one final (text-only) round.
            vec![LlmEvent::TextDelta("已发布".into()), done(StopReason::EndTurn)],
        ]);

        let output = engine(Arc::clone(&service))
            .run_loops(provider.clone(), "test-model", ctx)
            .await
            .unwrap();
        assert!(output.summary.contains("## 验证"));
        assert_eq!(output.activities.len(), 3);

        let seen = provider.seen_tool_names.lock().unwrap();
        assert_eq!(seen.len(), 6, "3 generation rounds + 3 repair rounds");
        for round in &seen[3..] {
            assert!(
                !round.iter().any(|name| name == "ls_start"),
                "the repair loop must never expose ls_start: {round:?}"
            );
        }
    }

    /// 模型从未调用 ls_start 就宣告完成：无草稿可发布，明确报错。
    #[tokio::test]
    async fn finishing_without_a_draft_fails() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(service.clone());
        let provider = ScriptedProvider::new(vec![vec![
            LlmEvent::TextDelta("nothing to do".into()),
            done(StopReason::EndTurn),
        ]]);
        let error = engine(service)
            .run_loops(provider, "test-model", ctx)
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
            // generation loop: document only, never the activities
            vec![tool_use("ls_start", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![
                tool_use(
                    "ls_set_document",
                    serde_json::json!({ "document": long_document() }),
                ),
                done(StopReason::ToolUse),
            ],
            vec![LlmEvent::TextDelta("done".into()), done(StopReason::EndTurn)],
            // repair loops 1-3: refuse to touch anything, then stop
            vec![LlmEvent::TextDelta("cannot fix".into()), done(StopReason::EndTurn)],
            vec![LlmEvent::TextDelta("cannot fix".into()), done(StopReason::EndTurn)],
            vec![LlmEvent::TextDelta("cannot fix".into()), done(StopReason::EndTurn)],
        ]);
        let error = engine(Arc::clone(&service))
            .run_loops(provider.clone(), "test-model", ctx)
            .await
            .unwrap_err();
        assert!(matches!(&error, AppError::UnprocessableEntity(message) if message.contains("exhausted 3 repair loops")));
        assert!(error.to_string().contains("activities_too_few"), "the blocking report names the finding");
        let seen = provider.seen_tool_names.lock().unwrap();
        assert_eq!(seen.len(), 6, "2 generation rounds + 1 idle round + 3 repair rounds");
    }
}

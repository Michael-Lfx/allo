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

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nomi_providers::{LlmProvider, create_provider};
use nomifun_common::{AppError, ProviderId, UserId};
use nomifun_learning::{
    LessonContentAgentEngine, LessonGenerationContext, LessonOp, LessonOutput, LearningService,
};

use crate::factory::provider_config::resolve_provider_config;
use crate::knowledge_completer::resolve_default_model;
use crate::learning_loop::{FlowCycle, LoopBudgets, LoopChannel, WireConfig, run_loops};
use crate::loop_core::{LoopEventSink, json_compact};
use crate::one_shot::{OneShotDeps, OneShotTool, one_shot_handler};

/// 课时内容循环的显式预算表（ADR-0004）：与 loop_core 的共享默认逐字一致
/// （50 轮 / 8192 token / 600s）——显式声明而非隐式继承，每流的生效值在
/// 自己的文件里可见。
const BUDGETS: LoopBudgets = LoopBudgets {
    generate_max_rounds: crate::loop_core::GENERATE_MAX_ROUNDS,
    round_tokens: crate::loop_core::AGENT_MAX_TOKENS,
    timeout_secs: crate::loop_core::TOTAL_TIMEOUT_SECS,
};

/// 线上翻译差异表（ADR-0004）：课时流无 kind 标记、不翻译 round_feedback、
/// start 帧不带 phase（不上线），但保留轮次日志（「生成」标签）且走课时
/// 专属事件流（`learning.lesson-generation`）。
const WIRE: WireConfig = WireConfig {
    kind_tag: None,
    round_log_gen_label: Some("生成"),
    translate_round_feedback: false,
    generate_start_phase: None,
    repair_start_phase: None,
    lesson_stream: true,
};

/// Generation-loop system prompt. The sectioned contract (ADR-0002) drives
/// the rounds: plan the manifest once, then one tool call per section body
/// (the model's attention stays on one learnable unit), then the activities
/// bound to section keys. The deterministic audit enforces the same rules.
const GENERATE_LESSON_AGENT_SYSTEM: &str = r#"你是一名课时内容设计代理：把给定的一个课时规划为若干「节」，逐节撰写正文并设计检索题目，通过工具逐步构建，最终通过确定性审计门禁发布。

【分节契约（每节一次工具调用）】
- 先 ls_set_section_manifest 规划节清单：每节带 section_key（s1、s2……）、kind（concept 概念 / example 例题 / demo 演示 / summary 小结 / practice 练习）、title（带类型前缀，如「概念：…」）、points（一句话要点）。节数按课时复杂度自定：低 1-3 节、中 3-5、高 4-6，硬上限 8；相邻节要有学习递进；最后一节必须是练习节（恰好 1 个）——学习者读完即进入统一练习轮。
- 每节带 visual 字段（该节承载核心讲解的可视化形态：公式/函数图/示意图/流程图/图表/表格 之一，内容确实非视觉才可用 文字）。
- 可视化为主、文字为辅是硬规则：概念/例题节的正文必须以至少一个可视化块（$$公式$$、```svg、```jsxgraph、```mermaid、对比表格）承载核心讲解，文字只作旁注；短段落、枚举用列表/表格、不写过渡废话。纯文字的概念/例题节会被质检门拒绝。
- 再逐节调用 ls_set_section_body 写正文：一次调用只写一节，body 直接以该节 `## ` 标题行开头（标题照抄清单），不要 JSON、不要包裹围栏、节内禁止 ### 子标题、不要自设练习环节（题目由题库承载）。
- 篇幅按节型：concept/example 400-700 中文字符；demo 由可视化块（```svg / ```jsxgraph / ```mermaid / $$数学$$）承载主要信息、旁注 200-400 字；summary 是要点清单；practice 只写能力目标与作答引导（≤120 字，不写题）。
- 可视化优先：内容真正需要图示时才画，每个图必须自足完整（viewBox、命名点、坐标刻度、说明文字，svg 文本 ≥12px、无脚本无外链）；图形块不计入篇幅。
- 与前一节已写正文自然衔接：不重复它讲过的内容；只写本节任务命中的范围，不越界讲后续节/课时。

【活动契约】
- 3-10 个活动：至少 2 个客观题（single_choice / true_false / fill_in_blank / multi_choice / numeric / ordering / matching）+ 至少 1 个 AI 批改题（reflection 反思；open_question 开放综合题至多 1 道），反思+开放合计 ≤3。
- 题型答案形状：single_choice 3-5 选项且 answer 恰等于其一；true_false 是布尔；fill_in_blank 恰含一个 "___"、answer 是 1-3 个等价答案数组、必须带 distractors 近义干扰；multi_choice 是 2+ 选项的数组（顺序无关）；numeric 是数字（可带 tol 容差）；ordering 的 options 是打乱条目、answer 是正确顺序；matching 的 options 是左列、answer 是一一对应的右列数组；reflection 与 open_question 的 answer 必须是 null。
- 每题必须带 section_key 绑定到教它的那一节（清单里的 key）；至多 1 道跨节综合题用 "general"。
- 概念绑定：concepts 只能取课时给定的概念 key（学习图节点留空数组）。
- estimated_minutes：5-60 的小整数，反映整体课时长度。

【工具使用纪律】
1. 第一步必须调用 ls_start 创建草稿——它返回 draft_id 与课时上下文。
2. ls_set_section_manifest 规划节清单 → 逐节 ls_set_section_body（每节一次调用，这是本轮循环的主要轮次）→ ls_patch_activities 分批提交题目（每批 1-5 个操作）。
3. ls_inspect 随时掌握草稿状态；全部构建完成后 ls_audit 自查，确认没有 danger 级问题才调用 ls_finish。

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
- sections_missing / section_body_missing / section_invalid：ls_set_section_manifest 补规划，或 ls_set_section_body 按节重写正文（标题照抄清单、篇幅按节型、demo 节必须带可视化块）。
- document_missing / document_invalid：ls_set_document 重写完整文档（旧单篇契约，≥800 字符、三必需章节按序）——新草稿请优先走分节。
- activities_too_few / objective_activities_too_few：add_activity 补客观题。
- reflections_too_many：remove_activity 删多余的反思题。
- reflections_multiple（warning）：可保留——只有 danger 才阻断发布。
- activity_shape_invalid：update_activity 按位置重写该活动（选项数/答案形状/___ 空格/干扰项/容差/顺序与对应关系）。
- section_binding_unknown：update_activity 把该题的 section_key 改绑到清单里的节 key（跨节综合题用 "general"）。
- section_invalid（缺可视化）：按清单里该节的 visual 重写正文——用 $$公式$$ / ```svg / ```jsxgraph / ```mermaid / 表格承载核心讲解，文字作旁注。
- concept_binding_unknown：update_activity 把 concepts 改绑到课时给定的概念 key。

【结束条件】
- 审计无 danger 时调用 ls_finish；若 ls_finish 被拒绝，认真阅读返回的阻塞报告并继续修复。
- 禁止空手结束：每一轮都必须调用工具（ls_set_document / ls_patch_activities / ls_audit / ls_finish）；只输出文字而不调用任何工具，会被判定为拒绝修复，整个生成以失败告终。
- 回复使用中文。"#;

/// Provider-backed engine for the two-loop lesson content pipeline.
pub struct LiveLessonContentAgentEngine {
    pub service: Arc<LearningService>,
    pub deps: OneShotDeps,
    /// 中断会话的轮次日志（draft_id → 每轮意图行）。resume 时取出注入开
    /// 场消息恢复认知；发布成功后清除。内存态，与草稿同生命周期（重启即
    /// 失）。与学习图循环的同名字段同构（断点续跑的另一半）。
    pub round_logs: Arc<Mutex<HashMap<String, Vec<String>>>>,
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
        self.run_generation(context, model_override, None).await
    }

    async fn resume(
        &self,
        _user_id: &UserId,
        draft_id: &str,
        context: &LessonGenerationContext,
        model_override: Option<(&str, &str)>,
    ) -> Result<LessonOutput, AppError> {
        self.run_generation(context, model_override, Some(draft_id.to_owned()))
            .await
    }
}

impl LiveLessonContentAgentEngine {
    /// Shared generation body: fresh runs start with an empty draft slot
    /// (`ls_start` creates the draft); resumes preset the slot with the
    /// surviving draft and inject its state plus the archived round logs
    /// into the opening user turn. Everything else — the timeout shell and
    /// the failure diagnostics — is identical for both entry points.
    async fn run_generation(
        &self,
        context: &LessonGenerationContext,
        model_override: Option<(&str, &str)>,
        resume_draft: Option<String>,
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
            resumed = resume_draft.is_some(),
            "lesson content generation start"
        );

        // The two slots are shared with the tool handlers: the draft the
        // model opened and the lesson output `ls_finish` published. On
        // resume the draft slot presets with the surviving draft — the
        // model never spends a round re-bootstrapping.
        let (user_text, draft_slot) = match resume_draft.as_ref() {
            Some(draft_id) => {
                let view = self.service.inspect_lesson_draft(draft_id)?;
                let previous_rounds = self
                    .round_logs
                    .lock()
                    .ok()
                    .and_then(|mut logs| logs.remove(draft_id))
                    .unwrap_or_default();
                (
                    compose_resume_user_text(context, &json_compact(&view), &previous_rounds),
                    Some(draft_id.clone()),
                )
            }
            None => (lesson_user_text(context), None),
        };
        let ctx = Arc::new(LoopContext {
            service: Arc::clone(&self.service),
            context: context.clone(),
            draft_slot: Arc::new(Mutex::new(draft_slot)),
            published_slot: Arc::new(Mutex::new(None)),
            channel: LoopChannel::new(WIRE, BUDGETS.generate_max_rounds),
        });

        match tokio::time::timeout(
            std::time::Duration::from_secs(BUDGETS.timeout_secs),
            run_loops(
                self,
                provider,
                &model,
                &user_text,
                Arc::clone(&ctx),
                BUDGETS.round_tokens,
            ),
        )
        .await
        {
            Ok(Ok(output)) => {
                // 发布成功：会话结束，清掉可能残留的轮次日志（run_loops 的
                // 发布点已清过，这里是幂等兜底；草稿与续跑映射由
                // finish_lesson_draft / 落库路径负责清理）。
                if let Some(draft_id) = ctx.draft_slot.lock().ok().and_then(|slot| slot.clone()) {
                    if let Ok(mut logs) = self.round_logs.lock() {
                        logs.remove(&draft_id);
                    }
                }
                ctx.log("session_end", serde_json::json!({
                    "ok": true,
                    "activities": output.activities.len(),
                    "estimated_minutes": output.estimated_minutes,
                }));
                Ok(output)
            }
            Ok(Err(error)) => {
                // 失败保留轮次日志：草稿仍在 TTL 内时重试即续跑。
                if let Some(draft_id) = ctx.draft_slot.lock().ok().and_then(|slot| slot.clone()) {
                    if let Ok(mut logs) = self.round_logs.lock() {
                        logs.insert(draft_id, ctx.channel.round_log_snapshot());
                    }
                }
                ctx.log("session_end", serde_json::json!({
                    "ok": false,
                    "error": error.to_string(),
                }));
                Err(error)
            }
            Err(_) => {
                let mut message =
                    format!("lesson content agent timed out after {}s", BUDGETS.timeout_secs);
                if let Some(draft_id) =
                    ctx.draft_slot.lock().ok().and_then(|slot| slot.clone())
                {
                    if let Ok(mut logs) = self.round_logs.lock() {
                        logs.insert(draft_id.clone(), ctx.channel.round_log_snapshot());
                    }
                    match self.service.audit_lesson_draft(&draft_id) {
                        Ok(audit) => {
                            message.push_str(&format!(
                                "\ndraft {draft_id} survives（可续跑，重试即接续本进度）; its audit state:\n{audit}"
                            ));
                        }
                        Err(_) => {
                            message.push_str(&format!(
                                "\ndraft {draft_id} survives（可续跑，重试即接续本进度）; audit unavailable"
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
    /// 发布成功后的会话收尾：清掉该草稿可能残留的轮次日志（发布后草稿
    /// 已被 finish 门移除，续跑映射也随之清除——日志没有存在意义）。
    fn clear_round_logs(&self, ctx: &LoopContext) {
        if let Some(draft_id) = ctx.draft_slot.lock().ok().and_then(|slot| slot.clone()) {
            if let Ok(mut logs) = self.round_logs.lock() {
                logs.remove(&draft_id);
            }
        }
    }
}

/// 统一外壳（[`FlowCycle`]）的课时内容插头：分节契约（ADR-0002）由提示词
/// 与 ls_* 工具面承载（旧单篇文档契约同为合法工具）；发布在任一轮完成时
/// 清轮次日志；错误透传、错误优先于发布——历史顺序，显式保留（ADR-0004）。
#[async_trait::async_trait]
impl FlowCycle for LiveLessonContentAgentEngine {
    type Ctx = LoopContext;
    type Output = LessonOutput;

    fn name(&self) -> &'static str {
        "lesson content"
    }

    fn budgets(&self) -> LoopBudgets {
        BUDGETS
    }

    fn wire(&self) -> WireConfig {
        WIRE
    }

    fn generate_system(&self) -> &'static str {
        GENERATE_LESSON_AGENT_SYSTEM
    }

    fn repair_system(&self) -> &'static str {
        REPAIR_LESSON_AGENT_SYSTEM
    }

    fn tools(&self, ctx: Arc<LoopContext>, repair_face: bool) -> Vec<OneShotTool> {
        lesson_content_tools(ctx, !repair_face)
    }

    async fn finish(&self, _ctx: &LoopContext, draft_id: &str) -> Result<LessonOutput, AppError> {
        self.service.finish_lesson_draft(draft_id)
    }

    fn revision(&self, _ctx: &LoopContext, draft_id: &str) -> Result<u64, AppError> {
        Ok(self.service.inspect_lesson_draft(draft_id)?.revision as u64)
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
        self.service.audit_lesson_draft(draft_id)
    }

    fn publish_frame(
        output: &LessonOutput,
        loop_label: &str,
        round: Option<usize>,
    ) -> serde_json::Value {
        let mut frame = serde_json::json!({
            "phase": loop_label,
            "activities": output.activities.len(),
        });
        if let Some(round) = round {
            frame["round"] = serde_json::json!(round);
        }
        frame
    }

    fn on_published(&self, ctx: &LoopContext) {
        self.clear_round_logs(ctx);
    }

    fn idle_nudge_actions(&self) -> &'static str {
        "ls_set_document / ls_patch_activities 执行修复动作，或以 ls_finish 尝试发布"
    }

    fn map_loop_error(&self, _ctx: &LoopContext, error: AppError) -> AppError {
        error
    }

    fn publish_before_error(&self) -> bool {
        false
    }

    fn missing_draft_texts(&self) -> (&'static str, &'static str) {
        (
            "no draft created (ls_start never called)",
            "lesson content agent finished without creating a draft (ls_start was never called)",
        )
    }

    fn slot_poisoned(&self) -> AppError {
        AppError::Internal("lesson draft slot poisoned".into())
    }

    fn draft_slot(ctx: &LoopContext) -> &Arc<Mutex<Option<String>>> {
        &ctx.draft_slot
    }

    fn published_slot(ctx: &LoopContext) -> &Arc<Mutex<Option<LessonOutput>>> {
        &ctx.published_slot
    }
}

// ── Shared loop state ──────────────────────────────────────────────────────

/// Everything the tool handlers need, captured once per generation. The two
/// slots are the only mutable cross-round state: which draft is active and
/// which lesson output (if any) was published by `ls_finish`. 线上翻译与轮
/// 次日志由统一外壳的 [`LoopChannel`] 承载（差异表见 [`WIRE`]）。
pub(crate) struct LoopContext {
    service: Arc<LearningService>,
    context: LessonGenerationContext,
    draft_slot: Arc<Mutex<Option<String>>>,
    published_slot: Arc<Mutex<Option<LessonOutput>>>,
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
            .ok_or_else(|| "没有活动的草稿——请先调用 ls_start".to_owned())
    }
}

impl LoopEventSink for LoopContext {
    fn log(&self, event: &str, fields: serde_json::Value) {
        LoopContext::log(self, event, fields);
    }
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
                    "前置路径（学习者到达本节点前应已掌握，按学习顺序；条目下的要点是各前置实际教过的内容——不要重复讲授）：\n{}\n",
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
    if !context.forbidden_concepts.trim().is_empty() {
        // 防超纲黑名单（learnhub「禁止使用的概念」）：传统课时 = 本课之外
        // 的概念；学习图节点 = 可及后代节点标题。此前引擎路径从未渲染过
        // 这个字段——黑名单只在 fallback 管线生效，这里是补上的注入点。
        text.push_str(&format!("\n{}\n", context.forbidden_concepts.trim()));
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

/// The resume opening: the fresh-run user turn (lesson coordinates and
/// grounding) plus the surviving draft's live state and the previous
/// session's round logs — the model continues where it stopped instead of
/// rebuilding from scratch (learnhub「断点续跑跳过已 ready 节」的循环形
/// 式等价物：草稿状态注入开场，已写的节就在草稿里，重写即浪费)。
fn compose_resume_user_text(
    context: &LessonGenerationContext,
    draft_state_json: &str,
    previous_rounds: &[String],
) -> String {
    let mut text = lesson_user_text(context);
    text.push_str(&format!(
        "\n【上次会话进度（{} 轮后中断；接着此进度继续，不要重复已写好的节与题目）】\n当前草稿状态：{}\n",
        previous_rounds.len(),
        draft_state_json
    ));
    for line in previous_rounds {
        text.push_str("\n- ");
        text.push_str(line);
    }
    text.push_str(
        "\n\n建议：先 ls_inspect 通读当前草稿核对进度，再继续未完成的节（每节一次 \
         ls_set_section_body），最后 ls_audit 自查、ls_finish 发布。",
    );
    text
}

// ── Tool set ───────────────────────────────────────────────────────────────

/// The `ls_*` whitelist. `with_start` adds `ls_start` (generation loop only
/// — the repair loop must never re-scope the draft, and an unlisted tool
/// name fails closed at the loop level).
fn lesson_content_tools(ctx: Arc<LoopContext>, with_start: bool) -> Vec<OneShotTool> {
    let mut tools = Vec::with_capacity(8);
    if with_start {
        tools.push(ls_start(Arc::clone(&ctx)));
    }
    tools.push(ls_inspect(Arc::clone(&ctx)));
    tools.push(ls_set_section_manifest(Arc::clone(&ctx)));
    tools.push(ls_set_section_body(Arc::clone(&ctx)));
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

fn ls_set_section_manifest(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "ls_set_section_manifest".into(),
        description: "规划课时的分节清单（整组替换）：sections 数组，每节带 section_key（s1、s2……）、kind（concept/example/demo/summary/practice）、title（带类型前缀）、points（一句话要点）、visual（可视化形态：公式/函数图/示意图/流程图/图表/表格/无）。节数低 1-3 / 中 3-5 / 高 4-6，硬上限 8；最后一节必须是练习节（恰好 1 个）；重规划时已写正文按 key 保留。".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "sections": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 8,
                    "description": "节清单（按学习顺序）",
                    "items": {
                        "type": "object",
                        "properties": {
                            "section_key": { "type": "string" },
                            "kind": { "type": "string", "enum": ["concept", "example", "demo", "summary", "practice"] },
                            "title": { "type": "string" },
                            "points": { "type": "string" },
                            "visual": { "type": "string", "enum": ["公式", "函数图", "示意图", "流程图", "图表", "表格", "无"] }
                        },
                        "required": ["section_key", "kind", "title", "visual"]
                    }
                }
            },
            "required": ["sections"]
        }),
        handler: one_shot_handler(move |input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                let sections = input
                    .get("sections")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let sections: Vec<nomifun_learning::SectionPack> = serde_json::from_value(sections)
                    .map_err(|error| format!("ls_set_section_manifest: sections 必须是节清单数组——{error}"))?;
                let report = ctx
                    .service
                    .patch_lesson_draft(
                        &draft_id,
                        vec![nomifun_learning::LessonOp::SetSectionManifest { sections }],
                    )
                    .map_err(|error| error.to_string())?;
                Ok(json_compact(&report))
            }
        }),
    }
}

fn ls_set_section_body(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "ls_set_section_body".into(),
        description: "写入一节正文：body 是该节的完整 Markdown，直接以 `## ` 标题行开头（照抄清单标题），不要 JSON、不要包裹围栏、节内禁止 ### 子标题。篇幅按节型（concept/example 400-700 字；demo 可视化为主；summary 要点清单；practice ≤120 字不写题）。每节调用一次；重复调用即整节重写。".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "section_key": { "type": "string", "description": "节 key（清单中的 s1、s2……）" },
                "body": { "type": "string", "description": "该节正文（Markdown 全文，一次写入）" }
            },
            "required": ["section_key", "body"]
        }),
        handler: one_shot_handler(move |input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                let section_key = input
                    .get("section_key")
                    .and_then(|value| value.as_str())
                    .filter(|key| !key.trim().is_empty())
                    .ok_or_else(|| "ls_set_section_body: section_key 必须是非空字符串".to_owned())?
                    .to_owned();
                let body = input
                    .get("body")
                    .and_then(|value| value.as_str())
                    .filter(|body| !body.trim().is_empty())
                    .ok_or_else(|| "ls_set_section_body: body 必须是非空 Markdown 文本".to_owned())?
                    .to_owned();
                let report = ctx
                    .service
                    .patch_lesson_draft(
                        &draft_id,
                        vec![nomifun_learning::LessonOp::SetSectionBody { section_key, body }],
                    )
                    .map_err(|error| error.to_string())?;
                Ok(json_compact(&report))
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
        description: "批量应用活动操作（add_activity / update_activity / remove_activity / set_estimated_minutes），一次调用就是一个批次；操作按数组顺序执行，先执行的操作会改变后续操作的 position。返回每个操作的成功/拒绝明细 + 最新审计 findings。\n\n调用示例：\n{\"operations\": [\n  {\"op\": \"add_activity\", \"activity\": {\"kind\": \"single_choice\", \"prompt\": \"期权的本质是什么？\", \"options\": [\"权利\", \"义务\", \"债务\"], \"answer\": \"权利\", \"explanation\": \"买方持有的是权利\", \"concepts\": [\"option_def\"]}},\n  {\"op\": \"set_estimated_minutes\", \"minutes\": 15}\n]}\n\n字段规则（9 种题型）：single_choice 3-5 选项且 answer 恰等于其一；multi_choice 是 2+ 选项数组（顺序无关）；numeric 是数字（可带 tol 容差）；ordering 的 options 是打乱条目、answer 是正确顺序数组；matching 的 options 是左列、answer 是一一对应右列数组；open_question 与 reflection 的 answer 必须是 null；每题带 section_key 绑定来源节（跨节综合题用 \"general\"）。原四种：single_choice 3-5 个选项且 answer 恰等于其一；true_false 的 answer 是布尔；fill_in_blank 的 prompt 恰含一个 \"___\"、answer 是 1-3 个等价答案的数组且必须带 distractors；reflection 的 answer 必须是 null；concepts 只能取本课概念 key（留空 = 绑定整课）。每批 ≤10 个操作；写正文请用 ls_set_section_body。".into(),
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
    use nomi_types::llm::{LlmEvent, ThinkingConfig};
    use nomi_types::message::{ContentBlock, StopReason};

    use nomifun_learning::{ConceptPack, GraphLessonContext, LessonExcerpt};

    use crate::learning_loop::test_support::{
        ScriptedProvider, done, test_deps, test_service, tool_use,
    };
    use crate::loop_core::{GENERATE_REASONING_EFFORT, run_agent_loop};

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
            lesson_id: "lesson-1".into(),
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
            forbidden_concepts: String::new(),
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

    /// 防超纲黑名单注入：`forbidden_concepts` 非空时渲染为独立段（传统课
    /// 时与学习图节点共用该注入点）；空串不产生任何文本。
    #[test]
    fn lesson_user_text_embeds_the_forbidden_blacklist_when_present() {
        let mut context = lesson_context();
        context.forbidden_concepts =
            "禁止使用的概念（尚未讲授——正文不得出现这些名称，也不得引用其结论）：\n- 希腊字母（期权 sensitivities）".into();
        let text = lesson_user_text(&context);
        assert!(text.contains("禁止使用的概念"), "{text}");
        assert!(text.contains("希腊字母"), "{text}");

        let plain = lesson_user_text(&lesson_context());
        assert!(!plain.contains("禁止使用的概念"), "{plain}");
    }

    fn engine(service: Arc<LearningService>) -> LiveLessonContentAgentEngine {
        LiveLessonContentAgentEngine {
            service,
            deps: test_deps(),
            round_logs: Arc::new(Mutex::new(HashMap::new())),
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
            channel: LoopChannel::new(WIRE, BUDGETS.generate_max_rounds),
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
                "ls_start", "ls_inspect", "ls_set_section_manifest", "ls_set_section_body",
                "ls_set_document", "ls_patch_activities", "ls_audit", "ls_finish"
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
                "ls_inspect", "ls_set_section_manifest", "ls_set_section_body",
                "ls_set_document", "ls_patch_activities", "ls_audit", "ls_finish"
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
        let output = run_loops(
            &engine(Arc::clone(&service)),
            provider.clone(),
            "test-model",
            &lesson_user_text(&ctx.context),
            ctx,
            BUDGETS.round_tokens,
            )
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

        let output = run_loops(
            &engine(Arc::clone(&service)),
            provider.clone(),
            "test-model",
            &lesson_user_text(&ctx.context),
            ctx,
            BUDGETS.round_tokens,
            )
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
        let error = run_loops(
            &engine(service),
            provider,
            "test-model",
            &lesson_user_text(&ctx.context),
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
        let error = run_loops(
            &engine(Arc::clone(&service)),
            provider.clone(),
            "test-model",
            &lesson_user_text(&ctx.context),
            ctx,
            BUDGETS.round_tokens,
            )
            .await
            .unwrap_err();
        assert!(matches!(&error, AppError::UnprocessableEntity(message) if message.contains("exhausted 3 repair loops")));
        assert!(error.to_string().contains("activities_too_few"), "the blocking report names the finding");
        let seen = provider.seen_tool_names.lock().unwrap();
        assert_eq!(seen.len(), 6, "2 generation rounds + 1 idle round + 3 repair rounds");
    }

    /// 续跑：预置存活草稿（manifest 已规划、部分节已写）后重入生成循环
    /// ——开场注入草稿状态与上次轮次日志，模型接着写完剩余节并发布；
    /// 发布后轮次日志清除（会话干净收尾）。
    #[tokio::test]
    async fn resume_continues_from_surviving_draft() {
        let (service, _dir) = test_service().await;
        let engine = engine(Arc::clone(&service));

        // 预置中断点：草稿由 ls_set_section_manifest + 一节正文构成。
        let view = service.create_lesson_draft(lesson_context()).unwrap();
        let draft_id = view.draft_id.clone();
        service
            .patch_lesson_draft(
                &draft_id,
                vec![
                    LessonOp::SetSectionManifest {
                        sections: serde_json::from_value(serde_json::json!([
                            { "section_key": "s1", "kind": "concept", "title": "概念：期权的定义", "points": "权利义务不对称", "visual": "表格" },
                            { "section_key": "s2", "kind": "practice", "title": "练习：期权判断", "points": "判断题自测", "visual": "无" }
                        ]))
                        .unwrap(),
                    },
                    LessonOp::SetSectionBody {
                        section_key: "s1".into(),
                        body: format!(
                            "## 概念：期权的定义\n\n| 头寸 | 权利 |\n| --- | --- |\n| 买方 | 有 |\n\n{}\n",
                            "这是一个足够长的正文段落，用于通过节级质检门的长度下限要求。".repeat(12)
                        ),
                    },
                ],
            )
            .unwrap();
        // 归档上次会话的轮次日志（中断前的进度轨迹）。
        engine
            .round_logs
            .lock()
            .unwrap()
            .insert(draft_id.clone(), vec!["第3轮(生成): ls_set_section_body ✓ 写完s1".into()]);

        let ctx = Arc::new(LoopContext {
            service: Arc::clone(&service),
            context: lesson_context(),
            draft_slot: Arc::new(Mutex::new(Some(draft_id.clone()))),
            published_slot: Arc::new(Mutex::new(None)),
            channel: LoopChannel::new(WIRE, BUDGETS.generate_max_rounds),
        });

        let provider = ScriptedProvider::new(vec![
            // 生成轮：直接写剩余的练习节并发布
            vec![
                tool_use("ls_set_section_body", serde_json::json!({
                    "section_key": "s2",
                    "body": "## 练习：期权判断\n\n完成以下判断题，检验你对权利义务不对称与四类基本头寸的理解；作答后阅读解析，回顾买方权利与卖方义务的关键区别。"
                })),
                done(StopReason::ToolUse),
            ],
            vec![
                tool_use("ls_patch_activities", valid_activity_ops()),
                done(StopReason::ToolUse),
            ],
            vec![tool_use("ls_finish", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![LlmEvent::TextDelta("已发布".into()), done(StopReason::EndTurn)],
        ]);

        let output = run_loops(
            &engine,
            provider,
            "test-model",
            &lesson_user_text(&ctx.context),
            ctx,
            BUDGETS.round_tokens,
            )
            .await
            .expect("the resumed draft publishes");
        assert_eq!(output.sections.len(), 2, "the pre-written section survives");
        assert!(
            engine.round_logs.lock().unwrap().get(&draft_id).is_none(),
            "round logs are cleared after a successful publish"
        );
    }

    /// 续跑开场：草稿状态与轮次日志注入「上次会话进度」段，并引导先通读
    /// 草稿；新会话（generate）不出现该段。
    #[test]
    fn resume_opening_carries_draft_state_and_round_logs() {
        let mut context = lesson_context();
        context.excerpt = None;
        let resume_text = compose_resume_user_text(
            &context,
            r#"{"revision":3}"#,
            &["第3轮(生成): ls_set_section_body ✓ 写完s1".into()],
        );
        assert!(resume_text.contains("上次会话进度（1 轮后中断"), "{resume_text}");
        assert!(resume_text.contains("revision"), "{resume_text}");
        assert!(resume_text.contains("ls_inspect"), "{resume_text}");
        // 新会话（无日志）不出现该段。
        let fresh = lesson_user_text(&context);
        assert!(!fresh.contains("上次会话进度"), "{fresh}");
    }
}

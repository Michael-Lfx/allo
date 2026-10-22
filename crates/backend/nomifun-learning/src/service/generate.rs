//! Synchronous course generation — the whole pipeline runs inside the HTTP
//! handler (mirroring the concept graph endpoint): validate → resolve the
//! brief (kb flow samples the base inline, milliseconds) → agent engine or
//! legacy one-shot fallback → assemble → import → return `CourseDetail`.
//!
//! Closing the dialog aborts the HTTP request; the handler future is
//! dropped at the next await point, which ends the loop and releases the
//! in-flight generation slot via the RAII guard. Progress is pushed to the
//! UI over the best-effort WebSocket stream (`learning.course-generation`);
//! the HTTP response stays the single source of truth for the terminal
//! state.

use super::*;

/// 结构发现阶段的常数次 LLM 调用(查询生成/证据摘要/结构草稿/结构评审,
/// 不随库规模变化;估算口径)。
const STRUCTURE_STAGE_CALLS: usize = 4;
/// 估算耗时经验常数(秒/次调用;确认弹窗的粗估读数,不是承诺)。
const ESTIMATE_SECONDS_PER_CALL: u64 = 10;
/// 估算跳过摘要限长(噪声/重复示例,确认弹窗够用)。
const ESTIMATE_SKIPPED_EXAMPLES_MAX: usize = 10;
/// triage_completed 帧的有界示例数(跳过/重复/uncertain;进度视图展开用)。
const TRIAGE_FRAME_EXAMPLES_MAX: usize = 10;

/// 生成注册表/悬浮指示条的主题（t1）：知识库流优先用户目标——目标即课程
/// 的意图，比库名更贴近用户正在建的东西；缺省回退库名，描述流取描述前
/// 120 字。确定性纯函数。
fn outline_brief_topic(brief: &OutlineBrief) -> String {
    if let Some(goal) = brief.goal.as_deref().map(str::trim).filter(|g| !g.is_empty()) {
        return goal.chars().take(120).collect();
    }
    match (&brief.description, &brief.knowledge_base) {
        (Some(description), _) => description.trim().chars().take(120).collect(),
        (_, Some(kb)) => kb.name.clone(),
        // 简报永远带一个来源（validate 保证）；防御性留空而非 panic。
        _ => String::new(),
    }
}

/// 内部 kill switch 的进程内 env 名（Q13,评审 S3）：置 off/0/false 禁用
/// 「清点分诊 → 结构发现 → 展开」新管线,回退现状采样+大纲引擎。运维级
/// 开关——不进 UI/用户配置面,默认(未设/其它值)新管线开启。
pub(super) const SOURCE_PLAN_KILL_SWITCH_ENV: &str = "NOMI_LEARNING_SOURCE_PLAN";

/// kill switch 命中判定（off/0/false;大小写与首尾空白宽容）。
pub(super) fn is_source_plan_kill_switch_value(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "off" | "0" | "false"
    )
}

/// 新管线是否被内部 kill switch 禁用（进程内 env 读取;默认开启）。
fn source_plan_pipeline_disabled() -> bool {
    std::env::var(SOURCE_PLAN_KILL_SWITCH_ENV)
        .map(|value| is_source_plan_kill_switch_value(&value))
        .unwrap_or(false)
}

/// 展开批数估算（评审 S2:成本门按章估算,不再按文件数近似）——有 plan
/// 时每章一批,单章锚点文件数超 [`EXPANSION_BATCH_MAX_FILES`] 才按文件数
/// 拆批;无 plan（估算端不跑 LLM 结构发现）退保守上界 = 候选文件数（最坏
/// 每个候选文件自成一章）,宁可高估不漏估（成本门的职责是拦住大库）。
pub(super) fn estimate_expansion_batches(
    plan: Option<&crate::generation::SourcePlan>,
    candidate_files: usize,
) -> usize {
    let Some(plan) = plan.filter(|plan| !plan.chapters.is_empty()) else {
        return candidate_files;
    };
    plan.chapters
        .iter()
        .map(|chapter| {
            let files: HashSet<&str> = chapter
                .source_anchors
                .iter()
                .map(|anchor| anchor.rel_path.as_str())
                .collect();
            files
                .len()
                .div_ceil(crate::generation::EXPANSION_BATCH_MAX_FILES)
        })
        .sum()
}

impl LearningService {
    /// Generate a course synchronously. Exactly one source must be set
    /// (knowledge base or free-text description) — enforced by
    /// [`GenerateCourseRequest::validate`]. `learning_graph` 课程分流到
    /// 学习图生成（描述即学习目标，agent 图循环 + 审计门禁发布）。
    pub async fn generate_course(
        &self,
        user_id: &UserId,
        request: GenerateCourseRequest,
    ) -> Result<CourseDetail, AppError> {
        request.validate()?;
        if request.course_kind == CourseKind::LearningGraph {
            return self.generate_learning_graph_course(user_id, &request).await;
        }
        let brief = self.resolve_outline_brief(&request).await?;
        // One in-flight generation per (user, source): duplicate submits
        // (double click, parallel agent tool calls) are rejected up front.
        // 多知识库（t4）：来源键 = 库 id 排序拼接——乱序提交同一组库命中
        // 同一槽位，不重复建课；单库保持 t2 的 `kb:{id}` 形状。
        let kind_key = match (&brief.knowledge_base, &brief.description) {
            (Some(_), _) => request
                .knowledge_base_key()
                .unwrap_or_else(|| "kb:".to_owned()),
            (_, Some(description)) => {
                let head: String = description.trim().chars().take(120).collect();
                format!("desc:{head}")
            }
            _ => unreachable!("validated request always carries one source"),
        };
        let _slot = self.acquire_generation_slot(user_id, kind_key)?;
        // 登记长任务（generation_tasks 注册表，status/cancel/快照端点的
        // 唯一数据源）：守卫 drop（含客户端断连中止）按终态收尾。主题取
        // 知识库流的用户目标（目标即课程的意图）、描述前 120 字或库名。
        // 键的课程段 = 会话 id（建课时课程尚不存在）。
        let topic = outline_brief_topic(&brief);
        let session = generate_id();
        let _task =
            self.begin_generation_task(crate::service::KIND_OUTLINE, &session, None, &topic)?;
        // t5(评审 S5):started 前导任何 triage/结构发现阶段帧——后续任何
        // failed(分诊/展开/导入失败)都有 started 前缀,进度视图不会收到
        // 无源 failed。
        self.emit_course_event(serde_json::json!({
            "phase": "started",
            "kind": brief.kind(),
        }));
        // t2 结构发现 + t3 分批展开(仅 KB 流 + 引擎路径):t1 清点/分诊 →
        // 证据驱动章节骨架 → **有可用 plan 时按章展开直接产 Blueprint(不再
        // 调大纲引擎)**;展开失败(非取消)→ 回退现行大纲引擎路径(degraded
        // 不阻断);无 completer/连续失败/零证据 → 无 plan,采样路径照旧。
        // 内部 kill switch(Q13,评审 S3):`NOMI_LEARNING_SOURCE_PLAN` 置
        // off/0/false 时不跑新管线,直接走现状采样+大纲引擎(行为等同现状;
        // 运维开关,非用户可见 flag,默认新管线开启)。
        // 遗留 one-shot 兜底路径(无引擎)不跑结构发现——它无法兑现 plan 的
        // 权威性,且保持测试/直调路径的 LLM 调用面零变化。
        let mut brief = brief;
        let mut expansion: Option<crate::generation::ExpansionOutcome> = None;
        // t4 快照信封的四件套库存位:inventory/triage 只在展开成功时保留
        // (coverage 在 expansion 里,plan 在 brief.source_plan 里)——四件
        // 套齐全 = 课程确实按 plan 建成,落库点才组装信封。
        let mut discovered: Option<(
            crate::generation::KbInventory,
            crate::generation::KbTriageReport,
        )> = None;
        if brief.knowledge_base.is_some()
            && self.course_outline_engine().is_some()
            && !source_plan_pipeline_disabled()
        {
            match self
                .discover_kb_source_plan(&request, &brief, _task.token(), &_task)
                .await
            {
                Ok((inventory, triage, plan)) => {
                    if let Some(plan) = plan.filter(|plan| plan.is_usable()) {
                        brief.source_plan = Some(plan.clone());
                        match self
                            .expand_kb_course_blueprint(
                                &inventory,
                                &triage,
                                &plan,
                                &brief,
                                _task.token(),
                                &_task,
                            )
                            .await
                        {
                            Ok(Some(outcome)) => {
                                discovered = Some((inventory, triage));
                                expansion = Some(outcome);
                            }
                            Ok(None) => {}
                            Err(error) => {
                                if matches!(
                                    &error,
                                    AppError::Conflict(message)
                                        if message.contains(crate::service::CANCELLED_MESSAGE)
                                ) {
                                    self.emit_course_event(serde_json::json!({
                                        "phase": "failed",
                                        "kind": brief.kind(),
                                        "error": error.to_string(),
                                    }));
                                    return Err(error);
                                }
                                tracing::warn!(
                                    session = %session,
                                    error = %error,
                                    "kb expansion failed; falling back to the outline engine"
                                );
                            }
                        }
                    }
                }
                Err(error) => {
                    self.emit_course_event(serde_json::json!({
                        "phase": "failed",
                        "kind": brief.kind(),
                        "error": error.to_string(),
                    }));
                    return Err(error);
                }
            }
        }
        let model_override = request
            .provider_id
            .as_ref()
            .zip(request.model.as_deref());
        // (started 帧已在任务登记后、结构发现前发出,评审 S5。)
        let (blueprint, expansion_coverage) = match expansion {
            // t3 分批展开产物:plan 权威骨架的确定性落地,直接进装配/导入
            //(跳过大纲引擎;失败时上面已回退 engine 路径)。coverage 保留
            // 给 t4 快照信封。
            Some(outcome) => {
                tracing::info!(
                    session,
                    modules = outcome.blueprint.modules.len(),
                    coverage_warnings = outcome.coverage.warnings.len(),
                    "kb expansion blueprint"
                );
                (outcome.blueprint, Some(outcome.coverage))
            }
            None => match self
                .generate_course_blueprint(&brief, model_override, &session)
                .await
            {
                Ok(blueprint) => (blueprint, None),
                Err(error) => {
                    self.emit_course_event(serde_json::json!({
                        "phase": "failed",
                        "kind": brief.kind(),
                        "error": error.to_string(),
                    }));
                    return Err(error);
                }
            },
        };
        // 蓝图的来源标注（t4）：逐课时把 source 路径解析回来源库，写入
        // `SourceSpan.kb_id`——快照/蓝图/课时三处的库归属同源，多库下课时
        // 读库（t3 实时读数）按它去前缀取正确库。
        let mut blueprint = blueprint;
        // 来源库列表（t4 + 评审对齐）：归一顺序原样落库（首个 = 主来源），但
        // 多库请求里**解析不到的库不进课程行**——它的缺席由采样披露的逐库
        // 条目（计数 0）如实承担，导入侧的"不写悬空 id"守卫保持原样。
        let source_kb_ids = if brief.knowledge_bases.is_empty() {
            request.knowledge_base_ids_normalized()
        } else {
            brief
                .knowledge_bases
                .iter()
                .filter_map(|base| KnowledgeBaseId::parse(&base.kb_id).ok())
                .collect()
        };
        annotate_blueprint_sources(
            &mut blueprint,
            brief.sampling.as_ref(),
            &source_kb_ids,
        );
        self.emit_course_event(serde_json::json!({
            "phase": "publishing",
            "kind": brief.kind(),
        }));
        let pack = assemble_outline_pack(&blueprint, &request);
        let blueprint_json = serde_json::to_string(&blueprint).map_err(|error| {
            AppError::Internal(format!("failed to serialize course blueprint: {error}"))
        })?;
        let samples_json = serde_json::to_string(&brief.samples).map_err(|error| {
            AppError::Internal(format!("failed to serialize sampled sources: {error}"))
        })?;
        // 目标与采样披露走独立列/伴随结构（t1/t2）：samples_json 保持
        // `Vec<(String, String)>` 旧形状，既有消费方零改动。
        let samples_meta_json = brief
            .sampling
            .as_ref()
            .map(|sampling| {
                serde_json::to_string(sampling).map_err(|error| {
                    AppError::Internal(format!("failed to serialize sampling disclosure: {error}"))
                })
            })
            .transpose()?;
        // 来源计划快照(t4):四件套齐全(清点/分诊/计划/展开覆盖)才组装
        // v1 信封——体积控制在 capture 里统一截断;描述流/回退采样路径/
        // legacy 走 None(列上 NULL,不伪造)。导入与 modules/lessons 同事务。
        let course_source_plan_json =
            match (&discovered, brief.source_plan.as_ref(), expansion_coverage.as_ref()) {
                (Some((inventory, triage)), Some(plan), Some(coverage)) => Some(
                    serde_json::to_string(
                        &crate::generation::CourseSourcePlanSnapshot::capture(
                            inventory, triage, plan, coverage,
                        ),
                    )
                    .map_err(|error| {
                        AppError::Internal(format!(
                            "failed to serialize course source plan snapshot: {error}"
                        ))
                    })?,
                ),
                _ => None,
            };
        let detail = self
            .import_course_outline(
                pack,
                OutlineSnapshot {
                    blueprint_json,
                    samples_json,
                    learning_goal: brief.goal.clone().unwrap_or_default(),
                    samples_meta_json,
                    // 来源库列表（t4）：归一顺序原样落库（首个 = 主来源）。
                    source_kb_ids,
                    // 来源计划快照（t4）：只有建课确实走了「计划 → 展开」
                    // 管线才写;描述流/回退路径 NULL,不伪造。
                    course_source_plan_json,
                },
            )
            .await;
        match &detail {
            Ok(detail) => {
                self.emit_course_event(serde_json::json!({
                    "phase": "completed",
                    "kind": brief.kind(),
                    "course_id": detail.course.id.as_str(),
                    "title": detail.course.title,
                    "modules": detail.modules.len(),
                    "lessons": detail.course.total_lessons,
                }));
            }
            Err(error) => {
                self.emit_course_event(serde_json::json!({
                    "phase": "failed",
                    "kind": brief.kind(),
                    "error": error.to_string(),
                }));
            }
        }
        detail
    }

    /// t2 结构发现编排:t1 分诊 → 证据驱动章节骨架。返回清点/分诊/计划
    /// 三件套——inventory+triage 供 t3 分批展开复用(不重复跑 t1 的 LLM 面)。
    /// 计划 `None` = 回退现行采样路径(无 completer/草稿连续失败/零证据降级
    /// 空章节);`Err` = 硬失败(零候选/零 core/用户取消)。
    ///
    /// t5:各阶段边界发 WS 进度帧(triage/source_plan)并更新长任务注册表的
    /// step 文案——「跳过与不确定可见」从分诊一刻就开始,不是建完课才补叙。
    async fn discover_kb_source_plan(
        &self,
        request: &GenerateCourseRequest,
        brief: &OutlineBrief,
        token: tokio_util::sync::CancellationToken,
        task: &crate::service::GenerationTaskGuard,
    ) -> Result<
        (
            crate::generation::KbInventory,
            crate::generation::KbTriageReport,
            Option<crate::generation::SourcePlan>,
        ),
        AppError,
    > {
        let knowledge = self.injected_knowledge_service()?;
        let kb_ids = request.knowledge_base_ids_normalized();
        let goal = brief.goal.clone().unwrap_or_default();
        let completer = self
            .course_completer
            .read()
            .map_err(|_| AppError::Internal("learning course completer lock poisoned".into()))?
            .clone();
        let kind = brief.kind();
        task.set_step("triage", "正在清点与分诊来源文件…");
        self.emit_course_event(serde_json::json!({
            "phase": "triage_started",
            "kind": kind,
        }));
        let (inventory, triage) =
            crate::generation::run_kb_triage(&knowledge, &kb_ids, &goal, completer.as_deref())
                .await?;
        let counts = &triage.counts;
        self.emit_course_event(serde_json::json!({
            "phase": "triage_completed",
            "kind": kind,
            "counts": {
                "total": counts.core + counts.prerequisite + counts.practice
                    + counts.mixed + counts.noise + counts.duplicate + counts.uncertain,
                "core": counts.core,
                "prerequisite": counts.prerequisite,
                "practice": counts.practice,
                "mixed": counts.mixed,
                "noise": counts.noise,
                "duplicate": counts.duplicate,
                "uncertain": counts.uncertain,
            },
            "degraded": triage.degraded,
            // 有界示例(≤10):进度视图的「可展开跳过理由/duplicate_of/
            // uncertain 列表」;全量在课程详情的诊断明细里。
            "excluded": triage.excluded.iter().take(TRIAGE_FRAME_EXAMPLES_MAX)
                .map(|entry| serde_json::json!({
                    "path": entry.path, "role": entry.role, "reason": entry.reason,
                }))
                .collect::<Vec<_>>(),
            "duplicates": triage.duplicates.iter().take(TRIAGE_FRAME_EXAMPLES_MAX)
                .map(|duplicate| serde_json::json!({
                    "path": duplicate.path,
                    "duplicate_of": duplicate.duplicate_of,
                    "canonical_reason": duplicate.canonical_reason,
                }))
                .collect::<Vec<_>>(),
            "uncertain": triage.files.iter()
                .filter(|file| file.role == crate::generation::KbTriageRole::Uncertain)
                .take(TRIAGE_FRAME_EXAMPLES_MAX)
                .map(|file| serde_json::json!({
                    "path": file.rel_path, "reason": file.reason,
                }))
                .collect::<Vec<_>>(),
        }));
        task.set_step("source_plan", "正在从证据规划章节骨架…");
        self.emit_course_event(serde_json::json!({
            "phase": "source_plan_started",
            "kind": kind,
        }));
        let plan = crate::generation::run_kb_structure(
            &knowledge,
            &inventory,
            &triage,
            &goal,
            completer.as_deref(),
            Some(&token),
        )
        .await?;
        let plan = match plan {
            Some(plan) => {
                tracing::info!(
                    chapters = plan.chapters.len(),
                    degraded = plan.degraded,
                    warnings = plan.warnings.len(),
                    "kb structure plan discovered"
                );
                if plan.is_usable() {
                    self.emit_course_event(serde_json::json!({
                        "phase": "source_plan_ready",
                        "kind": kind,
                        "chapters": plan.chapters.len(),
                        "concepts": plan.concept_graph.nodes.len(),
                        "gaps": plan.evidence.gaps,
                    }));
                    Some(plan)
                } else {
                    // 降级空章节计划:不进简报,回退采样路径。
                    None
                }
            }
            None => {
                tracing::warn!(
                    "kb structure discovery produced no usable plan; falling back to sampling"
                );
                None
            }
        };
        Ok((inventory, triage, plan))
    }

    /// t3 分批展开编排:可用 plan 按章展开产 Blueprint(不再走大纲引擎)。
    /// `Ok(None)` = 无 completer(调用方回退引擎路径);`Err` 取消向上传播、
    /// 其余由调用方按「展开失败 → 引擎路径」降级。展开成功时发 t5 覆盖
    /// 读数帧(topics covered/skipped/fallback——跳过与兜底在读数里先可见)。
    async fn expand_kb_course_blueprint(
        &self,
        inventory: &crate::generation::KbInventory,
        triage: &crate::generation::KbTriageReport,
        plan: &crate::generation::SourcePlan,
        brief: &OutlineBrief,
        token: tokio_util::sync::CancellationToken,
        task: &crate::service::GenerationTaskGuard,
    ) -> Result<Option<crate::generation::ExpansionOutcome>, AppError> {
        let knowledge = self.injected_knowledge_service()?;
        let completer = self
            .course_completer
            .read()
            .map_err(|_| AppError::Internal("learning course completer lock poisoned".into()))?
            .clone();
        let goal = brief.goal.clone().unwrap_or_default();
        task.set_step("expansion", "正在按章节分批展开课时…");
        let outcome = crate::generation::run_kb_expansion(
            &knowledge,
            inventory,
            triage,
            plan,
            &goal,
            completer.as_deref(),
            Some(&token),
        )
        .await?;
        if let Some(outcome) = &outcome {
            self.emit_course_event(serde_json::json!({
                "phase": "coverage_ready",
                "kind": brief.kind(),
                "topics_covered": outcome
                    .coverage
                    .topics
                    .iter()
                    .filter(|topic| topic.status == crate::generation::TopicStatus::Covered)
                    .count(),
                "topics_skipped": outcome
                    .coverage
                    .topics
                    .iter()
                    .filter(|topic| topic.status == crate::generation::TopicStatus::Skipped)
                    .count(),
                "fallback": outcome
                    .coverage
                    .lessons
                    .iter()
                    .filter(|lesson| lesson.fallback)
                    .count(),
            }));
        }
        Ok(outcome)
    }

    /// t5 成本门估算:提交建课前对知识库流做一次**规则分诊**(零 LLM、
    /// `degraded=true`——只读元数据/内容一次),给出文件数/批数/预估调用
    /// 数/预计耗时与跳过摘要;`needs_confirm` = >40 文件或预估 >20 次 LLM
    /// 调用(阈值在后端单源)。描述流无知识库 → 全零、不确认。
    pub async fn estimate_course_generation(
        &self,
        _user_id: &UserId,
        request: GenerateCourseRequest,
    ) -> Result<crate::models::CourseGenerationEstimateView, AppError> {
        request.validate()?;
        let kb_ids = request.knowledge_base_ids_normalized();
        if kb_ids.is_empty() {
            return Ok(crate::models::CourseGenerationEstimateView::default());
        }
        let knowledge = self.injected_knowledge_service()?;
        let goal = request.goal_text().unwrap_or_default();
        // completer=None → t1 规则路径:零 LLM 调用,非噪声全部 core(降级)。
        let (_inventory, triage) =
            crate::generation::run_kb_triage(&knowledge, &kb_ids, &goal, None).await?;
        let counts = triage.counts;
        let total_files = counts.core
            + counts.prerequisite
            + counts.practice
            + counts.mixed
            + counts.noise
            + counts.duplicate
            + counts.uncertain;
        // LLM 分诊只批「规则未判定的文件」(噪声/重复已被规则排除);
        // 展开按章分批——估算端没有 plan(LLM 结构发现不进成本门),退
        // 保守上界 = 候选文件数(评审 S2);结构阶段是常数次(查询/证据/
        // 草稿/评审)。
        let llm_files = counts.core + counts.prerequisite + counts.practice + counts.mixed + counts.uncertain;
        let triage_batches = llm_files.div_ceil(crate::generation::TRIAGE_BATCH_MAX_FILES);
        let expansion_batches = estimate_expansion_batches(None, llm_files);
        let estimated_batches = triage_batches + expansion_batches;
        let estimated_llm_calls = estimated_batches + STRUCTURE_STAGE_CALLS;
        let estimated_seconds = u64::try_from(estimated_llm_calls)
            .unwrap_or_default()
            .saturating_mul(ESTIMATE_SECONDS_PER_CALL);
        Ok(crate::models::CourseGenerationEstimateView {
            total_files,
            estimated_batches,
            estimated_llm_calls,
            estimated_seconds,
            needs_confirm: total_files
                > crate::models::COURSE_ESTIMATE_CONFIRM_FILES
                || estimated_llm_calls > crate::models::COURSE_ESTIMATE_CONFIRM_CALLS,
            triage: crate::models::CourseSourcePlanTriageView {
                total_files,
                core: counts.core,
                prerequisite: counts.prerequisite,
                practice: counts.practice,
                mixed: counts.mixed,
                noise: counts.noise,
                duplicate: counts.duplicate,
                uncertain: counts.uncertain,
                degraded: triage.degraded,
                batch_repair_count: crate::models::disclosed(triage.batch_repair_count),
                still_missing_count: crate::models::disclosed(triage.still_missing_count),
                warnings: triage
                    .warnings
                    .iter()
                    .take(super::course::SOURCE_PLAN_WARNINGS_MAX)
                    .cloned()
                    .collect(),
            },
            skipped_examples: triage
                .excluded
                .iter()
                .take(ESTIMATE_SKIPPED_EXAMPLES_MAX)
                .map(|entry| crate::models::CourseSourcePlanExcludedView {
                    path: entry.path.clone(),
                    role: serde_json::to_value(entry.role)
                        .ok()
                        .and_then(|value| value.as_str().map(str::to_owned))
                        .unwrap_or_default(),
                    reason: entry.reason.clone(),
                })
                .collect(),
        })
    }

    /// Resolve the generation brief up front: the kb flow fetches the base
    /// context and samples its documents inline (local file IO), so the
    /// engine needs no knowledge-service access; the description flow
    /// carries the brief text alone with no sampled sources.
    ///
    /// 多知识库（t4）：逐库核验存在 + 解析元数据，预算按库分摊、路径带
    /// `{库名}/rel` 命名空间；单库逐字走旧路径（零回归）。
    async fn resolve_outline_brief(
        &self,
        request: &GenerateCourseRequest,
    ) -> Result<OutlineBrief, AppError> {
        let kb_ids = request.knowledge_base_ids_normalized();
        if !kb_ids.is_empty() {
            let knowledge = self.injected_knowledge_service()?;
            let single = kb_ids.len() == 1;
            let mut bases: Vec<KnowledgeBaseBrief> = Vec::with_capacity(kb_ids.len());
            for kb_id in &kb_ids {
                match knowledge.get_base_info(kb_id.as_str()).await {
                    Ok(base) => bases.push(KnowledgeBaseBrief {
                        kb_id: kb_id.as_str().to_owned(),
                        name: base.name,
                        description: base.description,
                    }),
                    // 缺库口径与采样侧同一把尺（t4 裁决，评审对齐）：多库按库
                    // 跳过——缺失的库在采样披露里计数 0（`sample_bases` 仍拿
                    // 全量 id 列表），不让一个坏库阻断整门课；单库保持原错误
                    // （零回归）。全部库都缺失时采样侧以既有的 400 收场。
                    Err(error) if !single => {
                        tracing::warn!(
                            kb_id = kb_id.as_str(),
                            error = %error,
                            "knowledge base metadata unavailable; skipping it for this course"
                        );
                    }
                    Err(error) => return Err(error),
                }
            }
            let goal = request.goal_text();
            // 目标感知采样（t2）与多库分摊（t4）：目标非空时按相关度选入 +
            // 连贯预算补相邻，并带回披露快照；缺省（旧客户端）与无目标时逐字
            // 回退旧行为（单库）。
            let id_list: Vec<String> = kb_ids.iter().map(|id| id.as_str().to_owned()).collect();
            let outcome = sample_bases(&knowledge, &id_list, goal.as_deref()).await?;
            // 主来源 = 列表首个（多库下其余库的归属由采样命名空间承载）。
            let primary = bases.first().cloned();
            return Ok(OutlineBrief {
                description: None,
                knowledge_base: primary,
                knowledge_bases: bases,
                samples: outcome.samples,
                domain: request.domain.clone(),
                // 目标（t1）：归一落位；旧客户端不传即 None，主题/标题
                // 回退库名与模型命名（行为与旧路径一致）。
                goal,
                // 目标感知采样的披露（t2）：无目标路径为 None（单库）；多库
                // 恒有披露（命名空间需要它承载库归属）。
                sampling: outcome.disclosure,
                // 结构发现计划由 discover_kb_source_plan 在任务登记后补挂。
                source_plan: None,
            });
        }
        if let Some(description) = &request.description {
            return Ok(OutlineBrief {
                description: Some(description.trim().to_owned()),
                knowledge_base: None,
                knowledge_bases: Vec::new(),
                samples: Vec::new(),
                domain: request.domain.clone(),
                // 描述流的目标由 description 自身承担（学习图同语义）。
                goal: None,
                sampling: None,
                // 描述流零结构发现。
                source_plan: None,
            });
        }
        Err(AppError::BadRequest(
            "provide either a knowledge base or a course description".into(),
        ))
    }

    /// Blueprint production: the injected two-loop agent engine owns the
    /// whole lifecycle (draft + `co_*` tools, audit-gated publish); without
    /// one the legacy one-shot pipeline runs (fallback for tests and
    /// direct calls). The legacy fallback cannot observe the cancel flag
    /// mid-call — production always has the engine injected.
    async fn generate_course_blueprint(
        &self,
        brief: &OutlineBrief,
        model_override: Option<(&ProviderId, &str)>,
        session: &str,
    ) -> Result<Blueprint, AppError> {
        if let Some(engine) = self.course_outline_engine() {
            tracing::info!(session, kind = brief.kind(), "course outline engine start");
            let result = engine
                .generate(
                    brief,
                    model_override.map(|(provider, model)| (provider.as_str(), model)),
                )
                .await;
            match &result {
                Ok(blueprint) => tracing::info!(
                    session,
                    modules = blueprint.modules.len(),
                    "course outline engine end"
                ),
                Err(error) => tracing::warn!(session, error = %error, "course outline engine failed"),
            }
            return result;
        }
        let completer = self
            .course_completer
            .read()
            .map_err(|_| AppError::Internal("learning course completer lock poisoned".into()))?
            .clone()
            .ok_or_else(|| {
                AppError::Conflict("course generation is not configured".into())
            })?;
        let prompt = match (&brief.description, &brief.knowledge_base) {
            (Some(description), _) => {
                build_description_blueprint_prompt(description, brief.domain.as_deref())
            }
            (_, Some(kb)) => {
                // 多知识库（t4）：库名逐一点出（采样路径自带 `{库名}/rel` 命名
                // 空间，模型按路径归属引用来源）。
                let names: Vec<&str> = brief
                    .source_knowledge_bases()
                    .iter()
                    .map(|base| base.name.as_str())
                    .collect();
                let name = if names.is_empty() {
                    kb.name.clone()
                } else {
                    names.join("、")
                };
                build_blueprint_prompt(
                    &name,
                    &kb.description,
                    brief.domain.as_deref(),
                    brief.goal.as_deref(),
                    &brief.samples,
                )
            }
            _ => {
                return Err(AppError::BadRequest(
                    "course brief carries no generation source".into(),
                ))
            }
        };
        tracing::info!(
            session,
            kind = brief.kind(),
            samples = brief.samples.len(),
            "course outline fallback start"
        );
        let blueprint = generate_blueprint(
            completer.as_ref(),
            model_override,
            &prompt,
            &brief.samples,
        )
        .await?;
        tracing::info!(
            session,
            modules = blueprint.modules.len(),
            "course outline fallback blueprint"
        );
        Ok(blueprint)
    }
}

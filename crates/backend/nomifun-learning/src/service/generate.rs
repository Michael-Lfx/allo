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
        let kind_key = match (&brief.knowledge_base, &brief.description) {
            (Some(kb), _) => format!("kb:{}", kb.kb_id),
            (_, Some(description)) => {
                let head: String = description.trim().chars().take(120).collect();
                format!("desc:{head}")
            }
            _ => unreachable!("validated request always carries one source"),
        };
        let _slot = self.acquire_generation_slot(user_id, kind_key)?;
        // 登记生成注册（status/cancel 端点的唯一数据源，学习图流同样登
        // 记）：RAII 守卫在请求结束——含客户端断连中止——时注销。主题取
        // 知识库流的用户目标（目标即课程的意图）、描述前 120 字或库名，
        // 供悬浮指示条展示。
        let topic = outline_brief_topic(&brief);
        let _run = self.begin_generation(&topic);
        let session = generate_id();
        let model_override = request
            .provider_id
            .as_ref()
            .zip(request.model.as_deref());
        self.emit_course_event(serde_json::json!({
            "phase": "started",
            "kind": brief.kind(),
        }));
        let blueprint = match self
            .generate_course_blueprint(&brief, model_override, &session)
            .await
        {
            Ok(blueprint) => blueprint,
            Err(error) => {
                self.emit_course_event(serde_json::json!({
                    "phase": "failed",
                    "kind": brief.kind(),
                    "error": error.to_string(),
                }));
                return Err(error);
            }
        };
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
        let detail = self
            .import_course_outline(
                pack,
                OutlineSnapshot {
                    blueprint_json,
                    samples_json,
                    learning_goal: brief.goal.clone().unwrap_or_default(),
                    samples_meta_json,
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

    /// Resolve the generation brief up front: the kb flow fetches the base
    /// context and samples its documents inline (local file IO), so the
    /// engine needs no knowledge-service access; the description flow
    /// carries the brief text alone with no sampled sources.
    async fn resolve_outline_brief(
        &self,
        request: &GenerateCourseRequest,
    ) -> Result<OutlineBrief, AppError> {
        match (&request.knowledge_base_id, &request.description) {
            (Some(kb_id), _) => {
                let knowledge = self.injected_knowledge_service()?;
                let base = knowledge.get_base_info(kb_id.as_str()).await?;
                let goal = request.goal_text();
                // 目标感知采样（t2）：目标非空时按相关度选入 + 连贯预算补相邻，
                // 并带回披露快照；缺省（旧客户端）与无目标时逐字回退旧行为。
                let outcome =
                    sample_base_files(&knowledge, kb_id.as_str(), goal.as_deref()).await?;
                Ok(OutlineBrief {
                    description: None,
                    knowledge_base: Some(KnowledgeBaseBrief {
                        kb_id: kb_id.as_str().to_owned(),
                        name: base.name,
                        description: base.description,
                    }),
                    samples: outcome.samples,
                    domain: request.domain.clone(),
                    // 目标（t1）：归一落位；旧客户端不传即 None，主题/标题
                    // 回退库名与模型命名（行为与旧路径一致）。
                    goal,
                    // 目标感知采样的披露（t2）：无目标路径为 None。
                    sampling: outcome.disclosure,
                })
            }
            (None, Some(description)) => Ok(OutlineBrief {
                description: Some(description.trim().to_owned()),
                knowledge_base: None,
                samples: Vec::new(),
                domain: request.domain.clone(),
                // 描述流的目标由 description 自身承担（学习图同语义）。
                goal: None,
                sampling: None,
            }),
            (None, None) => Err(AppError::BadRequest(
                "provide either a knowledge base or a course description".into(),
            )),
        }
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
            (_, Some(kb)) => build_blueprint_prompt(
                &kb.name,
                &kb.description,
                brief.domain.as_deref(),
                brief.goal.as_deref(),
                &brief.samples,
            ),
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

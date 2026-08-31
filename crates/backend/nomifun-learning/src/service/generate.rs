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

impl LearningService {
    /// Generate a course synchronously. Exactly one source must be set
    /// (knowledge base or free-text description) — enforced by
    /// [`GenerateCourseRequest::validate`].
    pub async fn generate_course(
        &self,
        user_id: &UserId,
        request: GenerateCourseRequest,
    ) -> Result<CourseDetail, AppError> {
        request.validate()?;
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
        let session = generate_id();
        let model_override = request
            .provider_id
            .as_ref()
            .zip(request.model.as_deref());
        self.emit_course_event(serde_json::json!({
            "phase": "started",
            "kind": brief.kind(),
            "module_count": brief.module_count,
            "lessons_per_module": brief.lessons_per_module,
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
        let detail = self
            .import_course_outline(pack, blueprint_json, samples_json)
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
                let samples = sample_base_files(&knowledge, kb_id.as_str()).await?;
                Ok(OutlineBrief {
                    description: None,
                    knowledge_base: Some(KnowledgeBaseBrief {
                        kb_id: kb_id.as_str().to_owned(),
                        name: base.name,
                        description: base.description,
                    }),
                    samples,
                    domain: request.domain.clone(),
                    module_count: request.module_count,
                    lessons_per_module: request.lessons_per_module,
                })
            }
            (None, Some(description)) => Ok(OutlineBrief {
                description: Some(description.trim().to_owned()),
                knowledge_base: None,
                samples: Vec::new(),
                domain: request.domain.clone(),
                module_count: request.module_count,
                lessons_per_module: request.lessons_per_module,
            }),
            (None, None) => Err(AppError::BadRequest(
                "provide either a knowledge base or a course description".into(),
            )),
        }
    }

    /// Blueprint production: the injected two-loop agent engine owns the
    /// whole lifecycle (draft + `co_*` tools, audit-gated publish); without
    /// one the legacy one-shot pipeline runs (fallback for tests and
    /// direct calls).
    async fn generate_course_blueprint(
        &self,
        brief: &OutlineBrief,
        model_override: Option<(&ProviderId, &str)>,
        session: &str,
    ) -> Result<Blueprint, AppError> {
        if let Some(engine) = self.course_outline_engine() {
            self.log_course_outline_event(
                session,
                "engine_start",
                serde_json::json!({ "kind": brief.kind() }),
            );
            let result = engine
                .generate(
                    brief,
                    model_override.map(|(provider, model)| (provider.as_str(), model)),
                )
                .await;
            match &result {
                Ok(blueprint) => self.log_course_outline_event(
                    session,
                    "engine_end",
                    serde_json::json!({
                        "ok": true,
                        "modules": blueprint.modules.len(),
                        "concepts": blueprint.concepts.len(),
                    }),
                ),
                Err(error) => self.log_course_outline_event(
                    session,
                    "engine_end",
                    serde_json::json!({ "ok": false, "error": error.to_string() }),
                ),
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
            (Some(description), _) => build_description_blueprint_prompt(
                description,
                brief.domain.as_deref(),
                brief.module_count,
                brief.lessons_per_module,
            ),
            (_, Some(kb)) => build_blueprint_prompt(
                &kb.name,
                &kb.description,
                brief.domain.as_deref(),
                brief.module_count,
                brief.lessons_per_module,
                &brief.samples,
            ),
            _ => {
                return Err(AppError::BadRequest(
                    "course brief carries no generation source".into(),
                ))
            }
        };
        self.log_course_outline_event(
            session,
            "fallback_start",
            serde_json::json!({ "kind": brief.kind(), "samples": brief.samples.len() }),
        );
        let blueprint = generate_blueprint(
            completer.as_ref(),
            model_override,
            &prompt,
            &brief.samples,
            brief.module_count,
            brief.lessons_per_module,
        )
        .await?;
        self.log_course_outline_event(
            session,
            "fallback_blueprint",
            serde_json::json!({
                "modules": blueprint.modules.len(),
                "concepts": blueprint.concepts.len(),
            }),
        );
        Ok(blueprint)
    }
}

//! Production `LearningCourseSink` over the `LearningService` job API. The
//! trait lives in `nomi-agent`; this backend adapter maps the agent-facing
//! requests to the learning service's persistent course-generation jobs
//! (sample the base's markdown, drive the model, validate, import — all in a
//! background task per job). The generate call returns immediately with a
//! `job_id`; progress is polled through `generation_status`. The session's
//! active `(provider_id, model)` is forwarded so generation honors the model
//! the user picked in the conversation; without one the service's default
//! completer resolves the model (same fallback as the HTTP generate endpoint).

use std::sync::Arc;

use async_trait::async_trait;
use nomi_agent::learning_tools::{
    CourseGenerationRequest, CourseJobStarted, CourseJobStatus, LearningCourseSink,
};
use nomifun_common::{LearningCourseId, ProviderId, UserId};
use nomifun_learning::{
    CourseJobSource, GenerateCourseRequest, LearningService,
};

/// Bridges the agent-facing course-generation trait to the backend
/// LearningService. The sink is shared across sessions, so the caller's
/// `user_id`/`session_id` arrive per call.
pub struct LiveLearningCourseSink {
    pub service: Arc<LearningService>,
}

#[async_trait]
impl LearningCourseSink for LiveLearningCourseSink {
    async fn start_generation(
        &self,
        user_id: &str,
        session_id: Option<&str>,
        req: CourseGenerationRequest,
    ) -> Result<CourseJobStarted, String> {
        let user = UserId::parse(user_id).map_err(|e| format!("invalid user id {user_id}: {e}"))?;
        let provider_id = req
            .provider_id
            .map(|provider| {
                ProviderId::parse(&provider)
                    .map_err(|e| format!("invalid provider id {provider}: {e}"))
            })
            .transpose()?;
        let request = GenerateCourseRequest {
            knowledge_base_id: req.kb_id,
            domain: req.domain,
            provider_id,
            model: req.model,
            module_count: req.module_count,
            lessons_per_module: req.lessons_per_module,
        };
        let view = self
            .service
            .start_course_job(request, &user, CourseJobSource::Agent, session_id)
            .await
            .map_err(|e| e.to_string())?;
        Ok(CourseJobStarted {
            job_id: view.job_id,
            status: view.status.as_str().to_owned(),
        })
    }

    async fn generation_status(
        &self,
        user_id: &str,
        job_id: &str,
    ) -> Result<CourseJobStatus, String> {
        let user = UserId::parse(user_id).map_err(|e| format!("invalid user id {user_id}: {e}"))?;
        let view = self
            .service
            .course_job(&user, job_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("course generation job {job_id} not found"))?;
        let mut status = CourseJobStatus {
            job_id: view.job_id,
            status: view.status.as_str().to_owned(),
            current_lesson: view.current_lesson,
            total_lessons: view.total_lessons,
            error: view.error,
            course_id: view.course_id.clone(),
            title: None,
        };
        // Completed jobs report the course title so the agent can hand the
        // user an actionable result without another lookup.
        if view.status == nomifun_learning::CourseJobStatus::Completed
            && let Some(course_id) = view
                .course_id
                .as_deref()
                .and_then(|id| LearningCourseId::parse(id).ok())
            && let Ok(detail) = self.service.course_detail(&course_id, Some(&user)).await
        {
            status.title = Some(detail.course.title);
        }
        Ok(status)
    }

    async fn cancel_generation(&self, user_id: &str, job_id: &str) -> Result<(), String> {
        let user = UserId::parse(user_id).map_err(|e| format!("invalid user id {user_id}: {e}"))?;
        self.service
            .cancel_course_job(&user, job_id)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

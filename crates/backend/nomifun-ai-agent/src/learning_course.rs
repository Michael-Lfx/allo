//! Backend adapter for the agent-facing course-generation seam (the
//! [`LearningCourseSink`] trait lives in `nomi-agent`). It maps requests onto
//! the learning service's in-memory agent-job registry
//! (`start_agent_course_generation`): the request is validated, registered as
//! `running`, and the same synchronous pipeline the HTTP generate endpoint
//! uses (sample → outline agent loop → import) runs in a spawned background
//! task. The generate call returns immediately with a `job_id`; progress is
//! polled through `generation_status`. The session's active
//! `(provider_id, model)` is forwarded so generation honors the model the
//! user picked in the conversation; without one the service's default
//! completer resolves the model (same fallback as the HTTP generate
//! endpoint).

use std::sync::Arc;

use async_trait::async_trait;
use nomi_agent::learning_tools::{
    CourseGenerationRequest, CourseJobStarted, CourseJobStatus, LearningCourseSink,
};
use nomifun_common::{ProviderId, UserId};
use nomifun_learning::{CourseGenerationMode, GenerateCourseRequest, LearningService};

/// Bridges the agent-facing course-generation trait to the backend
/// LearningService. The sink is shared across sessions, so the caller's
/// `user_id` arrives per call; the in-memory registry isolates jobs by user.
pub struct LiveLearningCourseSink {
    pub service: Arc<LearningService>,
}

#[async_trait]
impl LearningCourseSink for LiveLearningCourseSink {
    async fn start_generation(
        &self,
        user_id: &str,
        _session_id: Option<&str>,
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
        // Only on-demand generation exists today; the field is an extension
        // point for future strategies, so any other value is rejected.
        let mode = match req.mode.as_deref() {
            None | Some("on_demand") => CourseGenerationMode::OnDemand,
            Some(other) => return Err(format!("unsupported course generation mode: {other}")),
        };
        let request = GenerateCourseRequest {
            knowledge_base_id: req.kb_id,
            description: req.description,
            domain: req.domain,
            provider_id,
            model: req.model,
            module_count: req.module_count,
            lessons_per_module: req.lessons_per_module,
            mode,
        };
        let job_id = self
            .service
            .start_agent_course_generation(&user, request)
            .map_err(|e| e.to_string())?;
        Ok(CourseJobStarted { job_id, status: "running".to_owned() })
    }

    async fn generation_status(
        &self,
        user_id: &str,
        job_id: &str,
    ) -> Result<CourseJobStatus, String> {
        let user = UserId::parse(user_id).map_err(|e| format!("invalid user id {user_id}: {e}"))?;
        let view = self
            .service
            .agent_course_job_status(&user, job_id)
            .ok_or_else(|| format!("course generation job {job_id} not found"))?;
        Ok(CourseJobStatus {
            job_id: job_id.to_owned(),
            status: view.status,
            error: view.error,
            course_id: view.course_id,
            title: view.title,
        })
    }

    async fn cancel_generation(&self, user_id: &str, job_id: &str) -> Result<(), String> {
        let user = UserId::parse(user_id).map_err(|e| format!("invalid user id {user_id}: {e}"))?;
        self.service
            .cancel_agent_course_job(&user, job_id)
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

//! Production `LearningCourseSink` over `LearningService::generate_course`.
//! The trait lives in `nomi-agent`; this backend adapter maps the agent-facing
//! request to the learning service's canonical generation path (sample the
//! base's markdown, drive the model, validate, import). Uses the service's
//! default completer (no per-call provider/model override) — the same
//! resolution the HTTP generate endpoint falls back to.

use std::sync::Arc;

use async_trait::async_trait;
use nomi_agent::learning_tools::{
    CourseGenerationReceipt, CourseGenerationRequest, LearningCourseSink,
};
use nomifun_learning::{GenerateCourseRequest, LearningService};

/// Bridges the agent-facing course-generation trait to the backend LearningService.
pub struct LiveLearningCourseSink {
    pub service: Arc<LearningService>,
}

#[async_trait]
impl LearningCourseSink for LiveLearningCourseSink {
    async fn generate_course(
        &self,
        req: CourseGenerationRequest,
    ) -> Result<CourseGenerationReceipt, String> {
        let request = GenerateCourseRequest {
            knowledge_base_id: req.kb_id,
            domain: req.domain,
            provider_id: None,
            model: None,
            module_count: req.module_count,
            lessons_per_module: req.lessons_per_module,
        };
        let detail = self
            .service
            .generate_course(request)
            .await
            .map_err(|e| e.to_string())?;
        let lessons = detail.modules.iter().map(|m| m.lessons.len()).sum();
        Ok(CourseGenerationReceipt {
            course_id: detail.course.id,
            title: detail.course.title,
            modules: detail.modules.len(),
            lessons,
        })
    }
}

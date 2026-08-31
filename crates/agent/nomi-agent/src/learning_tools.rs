//! Native `learning_generate_course` / `learning_course_status` tools: let an
//! in-process agent turn a mounted knowledge base into a structured learning
//! course (modules, lessons, retrieval activities, spaced-repetition concepts)
//! through a `LearningCourseSink` trait object. Generation now runs as a
//! persistent background job: the generate tool returns immediately with a
//! `job_id`, and the status tool reports progress / final result, so the agent
//! never blocks on the 1-3 minute generation. The backend injects a concrete
//! sink over its `LearningService`; standalone `nomi-cli` passes `None` and the
//! tools are absent.
//!
//! Intended workflow for the model: first persist well-structured markdown
//! documents into the base with `knowledge_write`, then call this tool to
//! generate the course from them.
//!
//! Mirrors `knowledge_tools.rs`: trait here, impl in `nomifun-ai-agent`.

use std::sync::Arc;

use async_trait::async_trait;
use nomifun_common::KnowledgeBaseId;
use serde_json::{Value, json};

use nomi_protocol::events::ToolCategory;
use nomi_tools::Tool;
use nomi_types::tool::{JsonSchema, ToolResult};

/// Tool name — allow-listed past the approval gate before bootstrap.
pub const LEARNING_GENERATE_COURSE_TOOL_NAME: &str = "learning_generate_course";

/// Tool name of the companion progress-poll tool for course-generation jobs.
/// Read-only (queries the job row), allow-listed alongside the generate tool
/// so the agent can report progress without a per-call approval.
pub const LEARNING_COURSE_STATUS_TOOL_NAME: &str = "learning_course_status";

/// Default course shape when the model omits sizing (matches the HTTP API).
const DEFAULT_MODULE_COUNT: u8 = 3;
const DEFAULT_LESSONS_PER_MODULE: u8 = 3;
/// Backend validation bound (`nomifun-learning::validate_generation_request`).
const MAX_SIZE: u8 = 6;

/// A model-issued course-generation request, resolved by the tool to one of
/// the session's bound bases before forwarding to the backend. The session's
/// active `(provider_id, model)` rides along so the background job runs on
/// the model the user picked in this conversation; `None` falls back to the
/// backend's default completer.
#[derive(Debug, Clone)]
pub struct CourseGenerationRequest {
    pub kb_id: KnowledgeBaseId,
    pub domain: Option<String>,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub module_count: u8,
    pub lessons_per_module: u8,
    /// Only "on_demand" is supported today (default): the outline is imported
    /// immediately and each lesson's body is generated when the learner opens
    /// it. Kept as an extension point for future generation strategies (e.g.
    /// a concept-graph-driven mode).
    pub mode: Option<String>,
}

/// A background course-generation job has been accepted. `job_id` is the handle
/// for `learning_course_status` and for the user's job panel on the Learning
/// page.
#[derive(Debug, Clone)]
pub struct CourseJobStarted {
    pub job_id: String,
    pub status: String,
}

/// Snapshot of a background course-generation job.
#[derive(Debug, Clone)]
pub struct CourseJobStatus {
    pub job_id: String,
    /// Machine-readable stage: queued | sampling | blueprint | lessons |
    /// importing | completed | failed | cancelled | interrupted.
    pub status: String,
    /// Number of completed lessons (0..=total_lessons).
    pub current_lesson: i64,
    pub total_lessons: i64,
    pub error: Option<String>,
    pub course_id: Option<String>,
    /// Course title, filled only when `completed`.
    pub title: Option<String>,
}

impl CourseJobStatus {
    /// Human-readable progress text for the agent's confirmation message.
    pub fn progress_text(&self) -> String {
        match self.status.as_str() {
            "queued" => format!("job {} is queued, waiting to start", self.job_id),
            "sampling" => {
                format!("job {} is sampling the knowledge base documents", self.job_id)
            }
            "blueprint" => format!("job {} is designing the course blueprint", self.job_id),
            "lessons" => format!(
                "job {} is generating lessons {}/{}",
                self.job_id, self.current_lesson, self.total_lessons
            ),
            "importing" => format!("job {} is importing the finished course", self.job_id),
            "completed" => {
                let title = self.title.as_deref().unwrap_or("(untitled)");
                let course_id = self.course_id.as_deref().unwrap_or("(unknown)");
                format!(
                    "job {} completed — course \"{title}\" (id: {course_id}) is ready on the Learning page",
                    self.job_id
                )
            }
            "failed" => format!(
                "job {} failed: {}",
                self.job_id,
                self.error.as_deref().unwrap_or("unknown error")
            ),
            "cancelled" => format!(
                "job {} was cancelled; it can be resumed to continue from the last completed lesson",
                self.job_id
            ),
            "interrupted" => {
                format!("job {} was interrupted by a restart; it can be resumed", self.job_id)
            }
            other => format!("job {} is in unknown state {other}", self.job_id),
        }
    }
}

/// Backend seam for course generation. Implemented by the backend over its
/// `LearningService` job API; `nomi-agent` only depends on this trait. The
/// backend samples the base's markdown, drives the model, validates the result,
/// and imports it as a persistent job — the tool layer forwards the model's
/// intent and reports progress. `user_id`/`session_id` are supplied by the tool
/// layer because the sink is shared across sessions.
#[async_trait]
pub trait LearningCourseSink: Send + Sync {
    /// Submit a course-generation job. Returns immediately with the job handle;
    /// the backend runs sampling/blueprint/lessons/import in the background.
    async fn start_generation(
        &self,
        user_id: &str,
        session_id: Option<&str>,
        req: CourseGenerationRequest,
    ) -> Result<CourseJobStarted, String>;

    /// Poll a job's current stage or final result.
    async fn generation_status(
        &self,
        user_id: &str,
        job_id: &str,
    ) -> Result<CourseJobStatus, String>;

    /// Request cancellation; takes effect at the next stage boundary.
    async fn cancel_generation(&self, user_id: &str, job_id: &str) -> Result<(), String>;
}

/// `learning_generate_course` — start a background course-generation job from a
/// mounted knowledge base. Holds the session's bound bases as `(kb_id, name)`
/// so the model selects by name (ids stay opaque), mirroring
/// `KnowledgeWriteTool`, plus the owning user/session so the shared sink can
/// target the right job owner, and the session's active model so generation
/// honors the user's conversation model instead of an arbitrary default.
pub struct LearningGenerateCourseTool {
    sink: Arc<dyn LearningCourseSink>,
    bases: Vec<(KnowledgeBaseId, String)>,
    user_id: String,
    session_id: Option<String>,
    /// Active `(provider_id, model)` of the owning conversation, forwarded to
    /// the job; `None` (standalone/tests) uses the backend's default completer.
    session_model: Option<(String, String)>,
}

impl LearningGenerateCourseTool {
    pub fn new(
        sink: Arc<dyn LearningCourseSink>,
        bases: Vec<(KnowledgeBaseId, String)>,
        user_id: impl Into<String>,
        session_id: Option<String>,
        session_model: Option<(String, String)>,
    ) -> Self {
        Self { sink, bases, user_id: user_id.into(), session_id, session_model }
    }

    /// One-line description of the bound bases for the schema.
    fn base_names(&self) -> Vec<&str> {
        self.bases.iter().map(|(_, name)| name.as_str()).collect()
    }
}

#[async_trait]
impl Tool for LearningGenerateCourseTool {
    fn name(&self) -> &str {
        LEARNING_GENERATE_COURSE_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Start a background job that generates a learning course (modules, lessons, quizzes, \
         spaced-repetition concepts) FROM a mounted knowledge base. The course is grounded in the \
         base's markdown documents, so FIRST make sure the base contains well-structured .md notes: \
         one topic per file, and each file's atomic unit should cover 描述 (description), \
         例子 (worked examples) and 验证 (self-check questions) at minimum — other sections such as \
         迁移 (transfer), 其他 (other), 关键词 (keywords), 推广 (promotion) are optional and chosen \
         by topic — write missing ones with knowledge_write before calling this. \
         Generation is on-demand: it samples the documents, designs a blueprint and imports the \
         course outline immediately (blueprint + lesson purposes + concept map), and each lesson's \
         long-form document and exercises are generated only when the learner opens that lesson on \
         the Learning page. The job runs in the background, so this tool returns immediately with \
         a job_id — report progress by calling learning_course_status with it; the user can \
         also track / cancel / resume the job on the Learning page."
    }

    fn input_schema(&self) -> JsonSchema {
        let names = self.base_names();
        let base_desc = if names.len() <= 1 {
            "Which knowledge base to build the course from (its name). Optional when only one base is mounted.".to_owned()
        } else {
            format!(
                "Which knowledge base to build the course from. Must be one of: {}.",
                names.join(", ")
            )
        };
        json!({
            "type": "object",
            "properties": {
                "base": { "type": "string", "description": base_desc },
                "domain": {
                    "type": "string",
                    "description": "Optional short domain label for the course, e.g. \"trading\" or \"rust\"."
                },
                "module_count": {
                    "type": "integer",
                    "description": "Number of course modules (default 3, max 6)."
                },
                "lessons_per_module": {
                    "type": "integer",
                    "description": "Lessons per module (default 3, max 6)."
                },
                "mode": {
                    "type": "string",
                    "description": "Generation strategy. Only \"on_demand\" is supported (default): the course outline is imported first and each lesson's body is generated when the learner opens it. Reserved for future strategies; do not pass other values."
                },
                "provider_id": {
                    "type": "string",
                    "description": "Optional provider id to run generation on; pass only when the caller was told to use a specific model. Omit to use the conversation's model."
                },
                "model": {
                    "type": "string",
                    "description": "Optional model name; must be passed together with provider_id. Omit to use the conversation's model."
                }
            },
            "required": []
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    fn is_deferred(&self) -> bool {
        // NOT deferred: the tool description carries the write-first workflow
        // contract, so its schema must be visible up front (same rationale as
        // knowledge_write).
        false
    }

    async fn execute(&self, input: Value) -> ToolResult {
        if self.bases.is_empty() {
            return ToolResult::text(
                "No knowledge bases are mounted in this session, so there is no base to build a course from.",
            );
        }
        let kb_id = match crate::knowledge_tools::resolve_write_base(
            &self.bases,
            input.get("base").and_then(Value::as_str),
        ) {
            Ok(b) => b.0.clone(),
            Err(e) => return ToolResult::error(e),
        };
        let domain = input
            .get("domain")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|d| !d.is_empty())
            .map(ToOwned::to_owned);
        let module_count = size_arg(&input, "module_count", DEFAULT_MODULE_COUNT);
        let lessons_per_module = size_arg(&input, "lessons_per_module", DEFAULT_LESSONS_PER_MODULE);
        let mode = input
            .get("mode")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|mode| !mode.is_empty())
            .map(ToOwned::to_owned);
        // An explicit provider/model pair (instructed by the caller) wins over the
        // conversation's active model; otherwise generation honors the session model.
        let (provider_id, model) = match (
            input
                .get("provider_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty()),
            input
                .get("model")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty()),
        ) {
            (Some(provider), Some(model)) => (Some(provider.to_owned()), Some(model.to_owned())),
            _ => (
                self.session_model.as_ref().map(|(provider, _)| provider.clone()),
                self.session_model.as_ref().map(|(_, model)| model.clone()),
            ),
        };
        let req = CourseGenerationRequest {
            kb_id,
            domain,
            provider_id,
            model,
            module_count,
            lessons_per_module,
            mode,
        };
        match self
            .sink
            .start_generation(&self.user_id, self.session_id.as_deref(), req)
            .await
        {
            Ok(job) => ToolResult::text(format!(
                "Started course generation job {} (status: {}). It runs in the background and takes 1-3 minutes; \
                 you can check progress with the learning_course_status tool using this job id, and the user can \
                 follow, cancel, resume or retry it on the Learning page.",
                job.job_id, job.status
            )),
            Err(e) => ToolResult::error(format!("learning_generate_course failed: {e}")),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Edit
    }

    fn describe(&self, input: &Value) -> String {
        let base = input.get("base").and_then(Value::as_str).unwrap_or("");
        if base.is_empty() {
            "learning_generate_course".to_owned()
        } else {
            format!("learning_generate_course '{base}'")
        }
    }
}

/// `learning_course_status` — report the progress or final result of a
/// background course-generation job started by `learning_generate_course`.
/// Read-only; the job stays owned by the user (per-job `user_id` isolation),
/// so any agent session can poll it for the same owner.
pub struct LearningCourseStatusTool {
    sink: Arc<dyn LearningCourseSink>,
    user_id: String,
}

impl LearningCourseStatusTool {
    pub fn new(sink: Arc<dyn LearningCourseSink>, user_id: impl Into<String>) -> Self {
        Self { sink, user_id: user_id.into() }
    }
}

#[async_trait]
impl Tool for LearningCourseStatusTool {
    fn name(&self) -> &str {
        LEARNING_COURSE_STATUS_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Check the progress of a background course-generation job started by learning_generate_course. \
         Returns the current stage (sampling / blueprint / generating lessons x/y / importing) while \
         running, or the final outcome: completed (with the course id and title), failed (with the \
         error), cancelled or interrupted. Input: job_id (required)."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "job_id": {
                    "type": "string",
                    "description": "The job id returned by learning_generate_course."
                }
            },
            "required": ["job_id"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        // Read-only poll: parallel calls are harmless.
        true
    }

    fn is_deferred(&self) -> bool {
        false
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let Some(job_id) = input
            .get("job_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
        else {
            return ToolResult::error("learning_course_status requires a job_id");
        };
        match self.sink.generation_status(&self.user_id, job_id).await {
            Ok(status) => ToolResult::text(status.progress_text()),
            Err(e) => ToolResult::error(format!("learning_course_status failed: {e}")),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Info
    }

    fn describe(&self, input: &Value) -> String {
        let job_id = input.get("job_id").and_then(Value::as_str).unwrap_or("");
        if job_id.is_empty() {
            "learning_course_status".to_owned()
        } else {
            format!("learning_course_status '{job_id}'")
        }
    }
}

/// Parse an optional sizing argument, falling back to the default and clamping
/// to the backend's 1..=6 validation bound so the model gets a course instead
/// of a service rejection.
fn size_arg(input: &Value, key: &str, default: u8) -> u8 {
    input
        .get(key)
        .and_then(Value::as_u64)
        .map(|n| (n as u16).clamp(1, MAX_SIZE as u16) as u8)
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KB1: &str = "0190f5fe-7c00-7a00-8abc-012345678961";
    const KB2: &str = "0190f5fe-7c00-7a00-8abc-012345678962";
    const USER: &str = "0190f5fe-7c00-7a00-8abc-0123456789ff";

    fn kb_id(label: &str) -> KnowledgeBaseId {
        let value = match label {
            "kb1" => KB1,
            "kb2" => KB2,
            other => panic!("unknown knowledge-base test label: {other}"),
        };
        KnowledgeBaseId::parse(value).expect("canonical knowledge-base test ID")
    }

    struct FakeCourseSink {
        last: std::sync::Mutex<Option<(String, Option<String>, CourseGenerationRequest)>>,
        fail: bool,
        status: std::sync::Mutex<CourseJobStatus>,
    }

    impl Default for FakeCourseSink {
        fn default() -> Self {
            Self {
                last: std::sync::Mutex::new(None),
                fail: false,
                status: std::sync::Mutex::new(CourseJobStatus {
                    job_id: String::new(),
                    status: "queued".to_owned(),
                    current_lesson: 0,
                    total_lessons: 0,
                    error: None,
                    course_id: None,
                    title: None,
                }),
            }
        }
    }

    #[async_trait]
    impl LearningCourseSink for FakeCourseSink {
        async fn start_generation(
            &self,
            user_id: &str,
            session_id: Option<&str>,
            req: CourseGenerationRequest,
        ) -> Result<CourseJobStarted, String> {
            if self.fail {
                return Err("knowledge base has no markdown documents".to_owned());
            }
            *self.last.lock().unwrap() =
                Some((user_id.to_owned(), session_id.map(ToOwned::to_owned), req));
            Ok(CourseJobStarted { job_id: "job-1".to_owned(), status: "queued".to_owned() })
        }

        async fn generation_status(
            &self,
            _user_id: &str,
            _job_id: &str,
        ) -> Result<CourseJobStatus, String> {
            Ok(self.status.lock().unwrap().clone())
        }

        async fn cancel_generation(&self, _user_id: &str, _job_id: &str) -> Result<(), String> {
            Ok(())
        }
    }

    fn tool(bases: Vec<(&str, &str)>) -> (LearningGenerateCourseTool, Arc<FakeCourseSink>) {
        let sink = Arc::new(FakeCourseSink::default());
        let bases: Vec<(KnowledgeBaseId, String)> = bases
            .into_iter()
            .map(|(id, name)| (kb_id(id), name.to_owned()))
            .collect();
        (
            LearningGenerateCourseTool::new(
                sink.clone(),
                bases,
                USER,
                Some("session-1".to_owned()),
                None,
            ),
            sink,
        )
    }

    #[tokio::test]
    async fn forwards_session_model_to_job() {
        let sink = Arc::new(FakeCourseSink::default());
        let tool = LearningGenerateCourseTool::new(
            sink.clone(),
            vec![(kb_id("kb1"), "Finance".to_owned())],
            USER,
            Some("session-1".to_owned()),
            Some(("provider-x".to_owned(), "model-y".to_owned())),
        );
        let res = tool.execute(json!({})).await;
        assert!(!res.is_error, "{res:?}");
        let (_, _, req) = sink.last.lock().unwrap().clone().unwrap();
        assert_eq!(req.provider_id.as_deref(), Some("provider-x"));
        assert_eq!(req.model.as_deref(), Some("model-y"));
    }

    #[tokio::test]
    async fn forwards_on_demand_mode_and_model_override() {
        let sink = Arc::new(FakeCourseSink::default());
        let tool = LearningGenerateCourseTool::new(
            sink.clone(),
            vec![(kb_id("kb1"), "Finance".to_owned())],
            USER,
            Some("session-1".to_owned()),
            Some(("session-provider".to_owned(), "session-model".to_owned())),
        );
        let res = tool
            .execute(json!({
                "mode": "on_demand",
                "provider_id": "explicit-provider",
                "model": "explicit-model"
            }))
            .await;
        assert!(!res.is_error, "{res:?}");
        let (_, _, req) = sink.last.lock().unwrap().clone().unwrap();
        assert_eq!(req.mode.as_deref(), Some("on_demand"));
        assert_eq!(req.provider_id.as_deref(), Some("explicit-provider"));
        assert_eq!(req.model.as_deref(), Some("explicit-model"));
    }

    fn status_tool() -> (LearningCourseStatusTool, Arc<FakeCourseSink>) {
        let sink = Arc::new(FakeCourseSink::default());
        (LearningCourseStatusTool::new(sink.clone(), USER), sink)
    }

    #[tokio::test]
    async fn starts_job_immediately_with_defaults_on_single_base() {
        let (tool, sink) = tool(vec![("kb1", "金融知识库")]);
        let res = tool.execute(json!({})).await;
        assert!(!res.is_error, "{res:?}");
        assert!(res.content.contains("job-1"), "{res:?}");
        assert!(res.content.contains("learning_course_status"), "{res:?}");
        let (user, session, req) = sink.last.lock().unwrap().clone().unwrap();
        assert_eq!(user, USER);
        assert_eq!(session.as_deref(), Some("session-1"));
        assert_eq!(req.kb_id, kb_id("kb1"));
        assert!(req.domain.is_none());
        assert_eq!(req.module_count, DEFAULT_MODULE_COUNT);
        assert_eq!(req.lessons_per_module, DEFAULT_LESSONS_PER_MODULE);
    }

    #[tokio::test]
    async fn resolves_base_by_name_and_forwards_options() {
        let (tool, sink) = tool(vec![("kb1", "Finance"), ("kb2", "Ops")]);
        let res = tool
            .execute(json!({"base": " ops ", "domain": " ops-runbook ", "module_count": 4, "lessons_per_module": 2}))
            .await;
        assert!(!res.is_error, "{res:?}");
        let (_, _, req) = sink.last.lock().unwrap().clone().unwrap();
        assert_eq!(req.kb_id, kb_id("kb2"));
        assert_eq!(req.domain.as_deref(), Some("ops-runbook"));
        assert_eq!(req.module_count, 4);
        assert_eq!(req.lessons_per_module, 2);
    }

    #[tokio::test]
    async fn sizes_are_clamped_to_backend_bounds() {
        let (tool, sink) = tool(vec![("kb1", "Finance")]);
        let res = tool.execute(json!({"module_count": 0, "lessons_per_module": 99})).await;
        assert!(!res.is_error, "{res:?}");
        let (_, _, req) = sink.last.lock().unwrap().clone().unwrap();
        assert_eq!(req.module_count, 1, "below-range sizes clamp to 1");
        assert_eq!(req.lessons_per_module, MAX_SIZE, "above-range sizes clamp to {MAX_SIZE}");
    }

    #[tokio::test]
    async fn multi_base_without_name_is_actionable_error() {
        let (tool, _sink) = tool(vec![("kb1", "Finance"), ("kb2", "Ops")]);
        let res = tool.execute(json!({})).await;
        assert!(res.is_error);
        assert!(res.content.contains("Finance") && res.content.contains("Ops"), "{res:?}");
    }

    #[tokio::test]
    async fn no_mounted_bases_is_soft_message() {
        let (tool, _sink) = tool(vec![]);
        let res = tool.execute(json!({})).await;
        assert!(!res.is_error);
        assert!(res.content.contains("No knowledge bases are mounted"));
    }

    #[tokio::test]
    async fn sink_error_is_surfaced() {
        let sink = Arc::new(FakeCourseSink { fail: true, ..Default::default() });
        let tool = LearningGenerateCourseTool::new(
            sink,
            vec![(kb_id("kb1"), "Finance".into())],
            USER,
            None,
            None,
        );
        let res = tool.execute(json!({})).await;
        assert!(res.is_error);
        assert!(res.content.contains("no markdown documents"));
    }

    #[tokio::test]
    async fn status_reports_in_progress_lessons() {
        let (tool, sink) = status_tool();
        *sink.status.lock().unwrap() = CourseJobStatus {
            job_id: "job-1".into(),
            status: "lessons".into(),
            current_lesson: 2,
            total_lessons: 9,
            error: None,
            course_id: None,
            title: None,
        };
        let res = tool.execute(json!({"job_id": "job-1"})).await;
        assert!(!res.is_error, "{res:?}");
        assert!(res.content.contains("2/9"), "{res:?}");
    }

    #[tokio::test]
    async fn status_reports_completed_with_course() {
        let (tool, sink) = status_tool();
        *sink.status.lock().unwrap() = CourseJobStatus {
            job_id: "job-1".into(),
            status: "completed".into(),
            current_lesson: 9,
            total_lessons: 9,
            error: None,
            course_id: Some("course-1".into()),
            title: Some("测试课程".into()),
        };
        let res = tool.execute(json!({"job_id": "job-1"})).await;
        assert!(!res.is_error, "{res:?}");
        assert!(res.content.contains("测试课程"), "{res:?}");
    }

    #[tokio::test]
    async fn status_surfaces_failure() {
        let (tool, sink) = status_tool();
        *sink.status.lock().unwrap() = CourseJobStatus {
            job_id: "job-1".into(),
            status: "failed".into(),
            current_lesson: 1,
            total_lessons: 9,
            error: Some("model call failed".into()),
            course_id: None,
            title: None,
        };
        let res = tool.execute(json!({"job_id": "job-1"})).await;
        assert!(!res.is_error, "{res:?}");
        assert!(res.content.contains("model call failed"), "{res:?}");
    }

    #[tokio::test]
    async fn status_requires_job_id() {
        let (tool, _sink) = status_tool();
        let res = tool.execute(json!({})).await;
        assert!(res.is_error);
        assert!(res.content.contains("job_id"), "{res:?}");
    }
}

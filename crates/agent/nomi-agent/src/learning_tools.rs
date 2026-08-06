//! Native `learning_generate_course` tool: lets an in-process agent turn a
//! mounted knowledge base into a structured learning course (modules, lessons,
//! retrieval activities, spaced-repetition concepts) through a
//! `LearningCourseSink` trait object. The backend injects a concrete sink over
//! its `LearningService`; standalone `nomi-cli` passes `None` and the tool is
//! absent.
//!
//! Intended workflow for the model: first persist well-structured markdown
//! documents into the base with `knowledge_write`, then call this tool to
//! generate the course from them.
//!
//! Mirrors `knowledge_tools.rs`: trait here, impl in `nomifun-ai-agent`.

use std::sync::Arc;

use async_trait::async_trait;
use nomifun_common::{KnowledgeBaseId, LearningCourseId};
use serde_json::{Value, json};

use nomi_protocol::events::ToolCategory;
use nomi_tools::Tool;
use nomi_types::tool::{JsonSchema, ToolResult};

/// Tool name — allow-listed past the approval gate before bootstrap.
pub const LEARNING_GENERATE_COURSE_TOOL_NAME: &str = "learning_generate_course";

/// Default course shape when the model omits sizing (matches the HTTP API).
const DEFAULT_MODULE_COUNT: u8 = 3;
const DEFAULT_LESSONS_PER_MODULE: u8 = 3;
/// Backend validation bound (`nomifun-learning::validate_generation_request`).
const MAX_SIZE: u8 = 6;

/// A model-issued course-generation request, resolved by the tool to one of
/// the session's bound bases before forwarding to the backend.
#[derive(Debug, Clone)]
pub struct CourseGenerationRequest {
    pub kb_id: KnowledgeBaseId,
    pub domain: Option<String>,
    pub module_count: u8,
    pub lessons_per_module: u8,
}

/// What came back, for the tool's confirmation message.
#[derive(Debug, Clone)]
pub struct CourseGenerationReceipt {
    pub course_id: LearningCourseId,
    pub title: String,
    pub modules: usize,
    pub lessons: usize,
}

/// Backend seam for course generation. Implemented by the backend over its
/// `LearningService::generate_course`; `nomi-agent` only depends on this
/// trait. The backend samples the base's markdown, drives the model, validates
/// the result, and imports it — the tool layer only forwards the model's
/// intent.
#[async_trait]
pub trait LearningCourseSink: Send + Sync {
    async fn generate_course(
        &self,
        req: CourseGenerationRequest,
    ) -> Result<CourseGenerationReceipt, String>;
}

/// `learning_generate_course` — build a learning course from a mounted
/// knowledge base. Holds the session's bound bases as `(kb_id, name)` so the
/// model selects by name (ids stay opaque), mirroring `KnowledgeWriteTool`.
pub struct LearningGenerateCourseTool {
    sink: Arc<dyn LearningCourseSink>,
    bases: Vec<(KnowledgeBaseId, String)>,
}

impl LearningGenerateCourseTool {
    pub fn new(sink: Arc<dyn LearningCourseSink>, bases: Vec<(KnowledgeBaseId, String)>) -> Self {
        Self { sink, bases }
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
        "Generate a learning course (modules, lessons, quizzes, spaced-repetition concepts) FROM a \
         mounted knowledge base. The course is grounded in the base's markdown documents, so FIRST \
         make sure the base contains well-structured .md notes: one topic per file, and each file's \
         atomic unit should cover 描述 (description), 例子 (worked examples) and 验证 (self-check \
         questions) at minimum — other sections such as 迁移 (transfer), 其他 (other), \
         关键词 (keywords), 推广 (promotion) are optional and chosen by topic — write missing ones \
         with knowledge_write before calling this. \
         Generated lesson documents follow the same structure as long-form study material \
         (1000+ characters each), so the course reads like a real textbook instead of a bare summary. \
         Generation samples the documents and runs multiple model calls (blueprint first, then one \
         call per lesson), so it takes 1-3 minutes; it creates the course and returns its id — the \
         user then opens it on the Learning page to enroll, take the diagnostic, and review."
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
        let req = CourseGenerationRequest { kb_id, domain, module_count, lessons_per_module };
        match self.sink.generate_course(req).await {
            Ok(r) => ToolResult::text(format!(
                "Generated course \"{}\" (id: {}) with {} module(s) and {} lesson(s) from the knowledge base. \
                 The user can open it on the Learning page to enroll, run the diagnostic, and start reviewing.",
                r.title, r.course_id, r.modules, r.lessons
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

    fn kb_id(label: &str) -> KnowledgeBaseId {
        let value = match label {
            "kb1" => KB1,
            "kb2" => KB2,
            other => panic!("unknown knowledge-base test label: {other}"),
        };
        KnowledgeBaseId::parse(value).expect("canonical knowledge-base test ID")
    }

    #[derive(Default)]
    struct FakeCourseSink {
        last: std::sync::Mutex<Option<CourseGenerationRequest>>,
        fail: bool,
    }

    #[async_trait]
    impl LearningCourseSink for FakeCourseSink {
        async fn generate_course(
            &self,
            req: CourseGenerationRequest,
        ) -> Result<CourseGenerationReceipt, String> {
            if self.fail {
                return Err("knowledge base has no markdown documents".to_owned());
            }
            *self.last.lock().unwrap() = Some(req);
            Ok(CourseGenerationReceipt {
                course_id: LearningCourseId::new(),
                title: "测试课程".to_owned(),
                modules: 3,
                lessons: 9,
            })
        }
    }

    fn tool(bases: Vec<(&str, &str)>) -> (LearningGenerateCourseTool, Arc<FakeCourseSink>) {
        let sink = Arc::new(FakeCourseSink::default());
        let bases: Vec<(KnowledgeBaseId, String)> = bases
            .into_iter()
            .map(|(id, name)| (kb_id(id), name.to_owned()))
            .collect();
        (LearningGenerateCourseTool::new(sink.clone(), bases), sink)
    }

    #[tokio::test]
    async fn generates_with_defaults_on_single_base() {
        let (tool, sink) = tool(vec![("kb1", "金融知识库")]);
        let res = tool.execute(json!({})).await;
        assert!(!res.is_error, "{res:?}");
        assert!(res.content.contains("测试课程"));
        assert!(res.content.contains("3 module(s)") && res.content.contains("9 lesson(s)"));
        let req = sink.last.lock().unwrap().clone().unwrap();
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
        let req = sink.last.lock().unwrap().clone().unwrap();
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
        let req = sink.last.lock().unwrap().clone().unwrap();
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
        );
        let res = tool.execute(json!({})).await;
        assert!(res.is_error);
        assert!(res.content.contains("no markdown documents"));
    }
}

use nomifun_common::{
    KnowledgeBaseId, LearningActivityId, LearningAttemptId, LearningConceptId, LearningCourseId,
    LearningEnrollmentId, LearningLessonId, LearningModuleId, LearningReviewItemId, ProviderId,
    TimestampMs,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
pub struct CoursePack {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_domain")]
    pub domain: String,
    #[serde(default)]
    pub source_kb_id: Option<KnowledgeBaseId>,
    #[serde(default = "default_version")]
    pub version: i64,
    #[serde(default)]
    pub concepts: Vec<ConceptPack>,
    pub modules: Vec<ModulePack>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GenerateCourseRequest {
    pub knowledge_base_id: KnowledgeBaseId,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub provider_id: Option<ProviderId>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default = "default_module_count")]
    pub module_count: u8,
    #[serde(default = "default_lessons_per_module")]
    pub lessons_per_module: u8,
}

const fn default_module_count() -> u8 {
    3
}

const fn default_lessons_per_module() -> u8 {
    3
}

fn default_domain() -> String {
    "general".to_string()
}

const fn default_version() -> i64 {
    1
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConceptPack {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub prerequisites: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModulePack {
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub lessons: Vec<LessonPack>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LessonPack {
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default = "default_estimated_minutes")]
    pub estimated_minutes: i64,
    #[serde(default)]
    pub source: Option<SourceSpan>,
    #[serde(default)]
    pub concepts: Vec<String>,
    #[serde(default)]
    pub activities: Vec<ActivityPack>,
}

const fn default_estimated_minutes() -> i64 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSpan {
    pub path: String,
    #[serde(default)]
    pub start: Option<i64>,
    #[serde(default)]
    pub end: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ActivityPack {
    pub kind: ActivityKind,
    pub prompt: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub answer: Value,
    #[serde(default)]
    pub explanation: String,
    #[serde(default)]
    pub concepts: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    SingleChoice,
    TrueFalse,
    Reflection,
}

impl ActivityKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SingleChoice => "single_choice",
            Self::TrueFalse => "true_false",
            Self::Reflection => "reflection",
        }
    }
}

impl TryFrom<&str> for ActivityKind {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "single_choice" => Ok(Self::SingleChoice),
            "true_false" => Ok(Self::TrueFalse),
            "reflection" => Ok(Self::Reflection),
            other => Err(format!("unsupported activity kind: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LessonStatus {
    NotStarted,
    InProgress,
    Completed,
}

impl LessonStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
        }
    }
}

impl TryFrom<&str> for LessonStatus {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "not_started" => Ok(Self::NotStarted),
            "in_progress" => Ok(Self::InProgress),
            "completed" => Ok(Self::Completed),
            other => Err(format!("unsupported lesson status: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CourseSummary {
    pub id: LearningCourseId,
    pub title: String,
    pub description: String,
    pub domain: String,
    pub source_kb_id: Option<KnowledgeBaseId>,
    pub version: i64,
    pub enrolled: bool,
    pub total_lessons: i64,
    pub completed_lessons: i64,
    pub updated_at: TimestampMs,
}

#[derive(Debug, Clone, Serialize)]
pub struct CourseDetail {
    pub course: CourseSummary,
    pub enrollment_id: Option<LearningEnrollmentId>,
    pub modules: Vec<ModuleView>,
    pub concepts: Vec<ConceptView>,
    pub next_lesson_id: Option<LearningLessonId>,
    pub due_review_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModuleView {
    pub id: LearningModuleId,
    pub title: String,
    pub description: String,
    pub position: i64,
    pub lessons: Vec<LessonView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LessonView {
    pub id: LearningLessonId,
    pub title: String,
    pub summary: String,
    pub position: i64,
    pub estimated_minutes: i64,
    pub source: Option<SourceSpan>,
    pub status: LessonStatus,
    pub concepts: Vec<LearningConceptId>,
    pub activities: Vec<ActivityView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActivityView {
    pub id: LearningActivityId,
    pub kind: ActivityKind,
    pub prompt: String,
    pub options: Vec<String>,
    pub position: i64,
    pub concepts: Vec<LearningConceptId>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticItem {
    pub lesson_id: LearningLessonId,
    pub lesson_title: String,
    pub activity: ActivityView,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticPlan {
    pub course_id: LearningCourseId,
    pub total_concepts: i64,
    pub items: Vec<DiagnosticItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConceptView {
    pub id: LearningConceptId,
    pub key: String,
    pub title: String,
    pub description: String,
    pub prerequisites: Vec<LearningConceptId>,
    pub mastery: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateLessonProgressRequest {
    pub status: LessonStatus,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubmitAttemptRequest {
    pub response: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttemptResult {
    pub id: LearningAttemptId,
    pub score: f64,
    pub passed: bool,
    pub feedback: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewRating {
    Again,
    Hard,
    Good,
    Easy,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateReviewRequest {
    pub rating: ReviewRating,
}

#[derive(Debug, Clone, Serialize)]
pub struct DueReview {
    pub id: LearningReviewItemId,
    pub enrollment_id: LearningEnrollmentId,
    pub course_id: LearningCourseId,
    pub course_title: String,
    pub module_title: String,
    pub lesson_title: String,
    pub concept_id: LearningConceptId,
    pub concept_title: String,
    pub question: ReviewQuestion,
    pub due_at: TimestampMs,
    pub stability_days: f64,
    pub difficulty: f64,
    pub review_count: i64,
    pub lapse_count: i64,
}

/// Objective question attached to a due review. Never includes the stored
/// answer; correctness is judged server-side by `answer_review`.
#[derive(Debug, Clone, Serialize)]
pub struct ReviewQuestion {
    pub activity_id: LearningActivityId,
    pub kind: ActivityKind,
    pub prompt: String,
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnswerReviewRequest {
    pub response: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewAnswerResult {
    pub correct: bool,
    pub feedback: String,
    /// Correct answer, only populated when the response was wrong.
    pub correct_answer: Option<Value>,
    /// Present when the answer was wrong and the item was automatically
    /// rated `again`; otherwise the caller rates after a correct answer.
    pub rated: Option<ReviewResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewResult {
    pub id: LearningReviewItemId,
    pub due_at: TimestampMs,
    pub stability_days: f64,
    pub difficulty: f64,
    pub review_count: i64,
    pub lapse_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredActivityConfig {
    pub options: Vec<String>,
    pub answer: Value,
    pub explanation: String,
}

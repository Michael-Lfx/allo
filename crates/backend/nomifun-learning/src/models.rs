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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewSource {
    /// Concept-bound review item coming from a course enrollment.
    Course,
    /// Learner-authored custom question with its own schedule.
    Custom,
}

#[derive(Debug, Clone, Serialize)]
pub struct DueReview {
    pub id: LearningReviewItemId,
    pub source: ReviewSource,
    pub enrollment_id: Option<LearningEnrollmentId>,
    pub course_id: Option<LearningCourseId>,
    pub course_title: Option<String>,
    pub module_title: Option<String>,
    pub lesson_title: Option<String>,
    pub concept_id: Option<LearningConceptId>,
    pub concept_title: Option<String>,
    pub question: ReviewQuestion,
    pub due_at: TimestampMs,
    pub stability_days: f64,
    pub difficulty: f64,
    pub review_count: i64,
    pub lapse_count: i64,
}

/// Objective question attached to a due review. Never includes the stored
/// answer; correctness is judged server-side by `answer_review`. Custom
/// questions carry no activity id.
#[derive(Debug, Clone, Serialize)]
pub struct ReviewQuestion {
    pub activity_id: Option<LearningActivityId>,
    pub kind: ActivityKind,
    pub prompt: String,
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnswerReviewRequest {
    #[serde(default)]
    pub response: Value,
    /// The learner admits they cannot recall the answer. Skips guessing:
    /// the item is rated `again` and the correct answer is returned.
    #[serde(default)]
    pub forgot: bool,
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
    /// Review item id for course reviews, custom question id otherwise.
    pub id: String,
    pub due_at: TimestampMs,
    pub stability_days: f64,
    pub difficulty: f64,
    pub review_count: i64,
    pub lapse_count: i64,
}

/// One row of the question management table. Course questions come from
/// objective activities linked to concepts (review item optional: items
/// only exist after the lesson is completed); custom questions are
/// learner-authored and always carry their own schedule.
#[derive(Debug, Clone, Serialize)]
pub struct QuestionEntry {
    pub source: ReviewSource,
    /// Activity id for course questions, custom question id otherwise.
    pub question_id: String,
    pub review_item_id: Option<LearningReviewItemId>,
    /// `unlearned`, `new`, `due` or `scheduled`.
    pub state: String,
    pub course_id: Option<LearningCourseId>,
    pub course_title: Option<String>,
    pub concept_id: Option<LearningConceptId>,
    pub concept_title: Option<String>,
    pub question_kind: Option<ActivityKind>,
    pub prompt: Option<String>,
    pub options: Vec<String>,
    pub answer: Option<Value>,
    pub explanation: Option<String>,
    pub due_at: Option<TimestampMs>,
    pub overdue: bool,
    pub stability_days: f64,
    pub difficulty: f64,
    pub review_count: i64,
    pub lapse_count: i64,
    pub last_reviewed_at: Option<TimestampMs>,
    pub updated_at: TimestampMs,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateQuestionRequest {
    pub prompt: String,
    #[serde(default)]
    pub options: Vec<String>,
    pub answer: Value,
    #[serde(default)]
    pub explanation: String,
}

/// Learner-authored question. Only objective kinds are supported; the
/// optional concept links the question back to an existing concept
/// (including orphaned concepts from deleted courses).
#[derive(Debug, Clone, Deserialize)]
pub struct CreateCustomQuestionRequest {
    pub kind: ActivityKind,
    pub prompt: String,
    #[serde(default)]
    pub options: Vec<String>,
    pub answer: Value,
    #[serde(default)]
    pub explanation: String,
    #[serde(default)]
    pub concept_id: Option<LearningConceptId>,
}

/// Concept offered in the custom question form: any concept the learner
/// has enrolled in, plus orphaned concepts still referenced by their
/// surviving review items.
#[derive(Debug, Clone, Serialize)]
pub struct ConceptRef {
    pub concept_id: LearningConceptId,
    pub title: String,
    pub course_title: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeleteCourseRequest {
    /// Also remove the learner's review items, mastery, progress, attempts
    /// and enrollment for this course. When false the content stays in the
    /// database so orphaned concepts remain reviewable.
    #[serde(default)]
    pub delete_reviews: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredActivityConfig {
    pub options: Vec<String>,
    pub answer: Value,
    pub explanation: String,
}

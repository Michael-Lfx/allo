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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    #[serde(default)]
    pub mode: CourseGenerationMode,
}

/// Course generation strategy: `full` materializes every lesson up front;
/// `on_demand` imports the outline immediately and generates each lesson's
/// body and activities only when the learner opens it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CourseGenerationMode {
    #[default]
    Full,
    OnDemand,
}

impl CourseGenerationMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::OnDemand => "on_demand",
        }
    }
}

impl TryFrom<&str> for CourseGenerationMode {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "full" => Ok(Self::Full),
            "on_demand" => Ok(Self::OnDemand),
            other => Err(format!("unsupported course generation mode: {other}")),
        }
    }
}

/// On-demand lesson content generation: optional model preference, mirroring
/// the reflection-grading request. Both fields are sent together (or neither);
/// when absent the backend falls back to its default completer.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct GenerateLessonRequest {
    #[serde(default)]
    pub provider_id: Option<ProviderId>,
    #[serde(default)]
    pub model: Option<String>,
}

/// Optional model preference for retrying a failed course-generation job.
/// Both fields are sent together (or neither); when provided the job's
/// request snapshot is re-pointed at the chosen model before the retry
/// re-runs, so a busy default model can be swapped for another one.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RetryCourseJobRequest {
    #[serde(default)]
    pub provider_id: Option<ProviderId>,
    #[serde(default)]
    pub model: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    #[serde(default)]
    pub purpose: String,
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

/// Serde helper: tolerate an explicit `null` where a string is expected by
/// degrading to an empty string. `#[serde(default)]` only covers a *missing*
/// field, while LLM outputs often write `"field": null` — which would
/// otherwise fail the whole parse ("invalid type: null, expected a string").
/// Degraded values then flow through the same validation as any other weak
/// output, so structural mistakes still trigger the targeted retry.
pub fn de_string_or_empty<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

/// Serde helper: tolerate `null` in place of a string list (or `null`
/// elements inside it) by degrading to an empty list. Same rationale as
/// [`de_string_or_empty`]; non-string elements still fail loudly.
pub fn de_vec_string_or_empty<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<Vec<Option<String>>>::deserialize(deserializer)?
        .unwrap_or_default()
        .into_iter()
        .flatten()
        .collect())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityPack {
    pub kind: ActivityKind,
    #[serde(default, deserialize_with = "de_string_or_empty")]
    pub prompt: String,
    #[serde(default, deserialize_with = "de_vec_string_or_empty")]
    pub options: Vec<String>,
    #[serde(default)]
    pub answer: Value,
    #[serde(default, deserialize_with = "de_string_or_empty")]
    pub explanation: String,
    #[serde(default, deserialize_with = "de_vec_string_or_empty")]
    pub concepts: Vec<String>,
    /// Near-synonym traps for fill_in_blank blanks (or physically adjacent
    /// quantities), forcing fine discrimination. Only fill_in_blank uses it.
    #[serde(default, deserialize_with = "de_vec_string_or_empty")]
    pub distractors: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    SingleChoice,
    TrueFalse,
    Reflection,
    FillInBlank,
}

impl ActivityKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SingleChoice => "single_choice",
            Self::TrueFalse => "true_false",
            Self::Reflection => "reflection",
            Self::FillInBlank => "fill_in_blank",
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
            "fill_in_blank" => Ok(Self::FillInBlank),
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
    pub tags: Vec<String>,
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
    pub purpose: String,
    pub position: i64,
    pub estimated_minutes: i64,
    pub generated: bool,
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
    /// Explicit AI model preference for reflection grading. Both fields are
    /// sent together (or neither); the backend falls back to its default
    /// completer when absent and to rule-based grading on any AI failure.
    #[serde(default)]
    pub provider_id: Option<ProviderId>,
    #[serde(default)]
    pub model: Option<String>,
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

/// Daily check-in snapshot for the current review day: goal, progress and
/// due count, with the completion flag derived from a locking snapshot
/// persisted in `learning_checkins`.
#[derive(Debug, Clone, Serialize)]
pub struct CheckinStatus {
    /// Local review day as YYYYMMDD.
    pub review_day: i64,
    /// Daily review goal snapshot (0 = clear-the-queue only).
    pub goal: i64,
    /// Reviews already submitted this review day.
    pub reviewed_count: i64,
    /// Cards currently due (`due_at <= now`), course + custom.
    pub due_count: i64,
    /// Whether the day is locked as completed (either condition met).
    pub completed: bool,
    /// Lock moment in UTC milliseconds when completed, else null.
    pub locked_at: Option<i64>,
}

/// One lesson completed on a review day (calendar aggregation detail).
#[derive(Debug, Clone, Serialize)]
pub struct CalendarLessonRef {
    pub lesson_id: String,
    pub title: String,
}

/// One course created on a review day (calendar aggregation detail).
#[derive(Debug, Clone, Serialize)]
pub struct CalendarCourseRef {
    pub course_id: String,
    pub title: String,
}

/// One review day inside the requested calendar range, zero-filled when the
/// user had no activity. `review_day` is the local YYYYMMDD of the review day
/// (02:00 rollover), matching check-in and streak semantics.
#[derive(Debug, Clone, Serialize)]
pub struct CalendarDayStats {
    pub review_day: i64,
    pub reviewed_count: i64,
    pub checkin_completed: bool,
    pub completed_lessons: Vec<CalendarLessonRef>,
    pub created_courses: Vec<CalendarCourseRef>,
}

/// Calendar aggregation for the learning page: review-day bucketed activity
/// (review counts, check-in completion, completed lessons and created
/// courses) plus the current streak.
#[derive(Debug, Clone, Serialize)]
pub struct CalendarStats {
    pub year: i64,
    /// 1..=12 for the month view, null for the year view.
    pub month: Option<i64>,
    pub tz_offset: i32,
    /// Consecutive completed check-in days ending at the current review day;
    /// 0 when today is not yet completed.
    pub streak: i64,
    pub days: Vec<CalendarDayStats>,
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
    /// Near-synonym traps for fill_in_blank blanks, surfaced for display.
    #[serde(default)]
    pub distractors: Vec<String>,
    pub explanation: Option<String>,
    pub due_at: Option<TimestampMs>,
    pub overdue: bool,
    pub stability_days: f64,
    pub difficulty: f64,
    pub review_count: i64,
    pub lapse_count: i64,
    pub last_reviewed_at: Option<TimestampMs>,
    pub updated_at: TimestampMs,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateQuestionRequest {
    pub prompt: String,
    #[serde(default)]
    pub options: Vec<String>,
    pub answer: Value,
    #[serde(default)]
    pub explanation: String,
    #[serde(default)]
    pub distractors: Vec<String>,
}

/// Learner-authored question. Objective kinds (single choice, true/false,
/// fill in the blank) are supported; the optional concept links the question
/// back to an existing concept (including orphaned concepts from deleted
/// courses).
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
    /// Near-synonym traps for fill_in_blank blanks (optional when the
    /// learner authors the question by hand).
    #[serde(default)]
    pub distractors: Vec<String>,
}

/// Manually appends an activity to an existing lesson. All four kinds are
/// accepted; when `concept_ids` is empty the activity binds to every concept
/// of the lesson, matching course-generation semantics.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateLessonActivityRequest {
    pub kind: ActivityKind,
    pub prompt: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub answer: Value,
    #[serde(default)]
    pub explanation: String,
    /// Near-synonym traps for fill_in_blank blanks; empty for other kinds.
    #[serde(default)]
    pub distractors: Vec<String>,
    #[serde(default)]
    pub concept_ids: Vec<LearningConceptId>,
}

/// Asks the knowledge-backed generator for a single activity draft for an
/// existing lesson. The draft is returned for preview and never persisted.
#[derive(Debug, Clone, Deserialize)]
pub struct GenerateLessonActivityRequest {
    pub kind: ActivityKind,
    #[serde(default)]
    pub provider_id: Option<ProviderId>,
    #[serde(default)]
    pub model: Option<String>,
    /// Optional focus hint steering the question topic; empty means the
    /// generator picks the least-covered ground itself.
    #[serde(default)]
    pub focus: String,
}

/// AI-generated activity draft shown to the learner for preview before they
/// confirm adding it to the lesson.
#[derive(Debug, Clone, Serialize)]
pub struct GeneratedLessonActivity {
    pub kind: ActivityKind,
    pub prompt: String,
    pub options: Vec<String>,
    pub answer: Value,
    pub explanation: String,
    pub distractors: Vec<String>,
    /// Suggested concept bindings (the lesson's concepts by default).
    pub concept_ids: Vec<LearningConceptId>,
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

/// Replaces the tag set of a course or question. Unknown tag names are
/// created automatically; names are trimmed, empty values dropped and
/// duplicates collapsed before storing.
#[derive(Debug, Clone, Deserialize)]
pub struct SetTagsRequest {
    pub tags: Vec<String>,
    /// For courses only: also append every tag of the final set to each
    /// question under the course, keeping the questions' existing tags.
    #[serde(default)]
    pub apply_to_children: bool,
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
    /// Near-synonym traps for fill_in_blank blanks; empty for other kinds.
    /// Old rows lack the column, so it defaults on read.
    #[serde(default)]
    pub distractors: Vec<String>,
}

/// Who submitted a course-generation job: the HTTP generate endpoint or an
/// agent tool call. Kept for the task list display only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CourseJobSource {
    Http,
    Agent,
}

impl CourseJobSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Agent => "agent",
        }
    }
}

impl TryFrom<&str> for CourseJobSource {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "http" => Ok(Self::Http),
            "agent" => Ok(Self::Agent),
            other => Err(format!("unsupported course job source: {other}")),
        }
    }
}

/// Pipeline stage of a persistent course-generation job. Non-terminal stages
/// double as the runner's next step, so the claimed row always tells the
/// runner what to do after a resume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CourseJobStatus {
    Queued,
    Sampling,
    Blueprint,
    Lessons,
    Importing,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

impl CourseJobStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Sampling => "sampling",
            Self::Blueprint => "blueprint",
            Self::Lessons => "lessons",
            Self::Importing => "importing",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

impl TryFrom<&str> for CourseJobStatus {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "queued" => Ok(Self::Queued),
            "sampling" => Ok(Self::Sampling),
            "blueprint" => Ok(Self::Blueprint),
            "lessons" => Ok(Self::Lessons),
            "importing" => Ok(Self::Importing),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "interrupted" => Ok(Self::Interrupted),
            other => Err(format!("unsupported course job status: {other}")),
        }
    }
}

/// Public projection of one `learning_course_jobs` row: everything the
/// Learning page needs to render progress and offer cancel/resume/retry.
#[derive(Debug, Clone, Serialize)]
pub struct CourseJobView {
    pub job_id: String,
    pub source: CourseJobSource,
    pub status: CourseJobStatus,
    /// 1-based module index of the lesson currently being generated (0 until
    /// the blueprint resolves).
    pub current_module: i64,
    /// Number of completed lessons (0..=total_lessons).
    pub current_lesson: i64,
    pub total_lessons: i64,
    pub error: Option<String>,
    pub course_id: Option<String>,
    /// Knowledge base name the course is generated from (`None` when the
    /// base was deleted since the job ran).
    pub knowledge_base_name: Option<String>,
    /// User-provided course domain from the request snapshot, when given.
    pub domain: Option<String>,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

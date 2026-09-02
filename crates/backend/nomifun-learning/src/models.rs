use nomifun_common::{
    AppError, KnowledgeBaseId, LearningActivityId, LearningAttemptId, LearningConceptId,
    LearningCourseId, LearningEnrollmentId, LearningLessonId, LearningModuleId,
    LearningReviewItemId, ProviderId, TimestampMs,
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
    /// Knowledge base to ground the course in (kb flow). Exactly one of
    /// `knowledge_base_id` / `description` must be provided.
    #[serde(default)]
    pub knowledge_base_id: Option<KnowledgeBaseId>,
    /// Free-text course brief (description flow): the course is generated
    /// from this briefing alone — no knowledge base is involved, sampled
    /// sources stay empty and lessons carry no `source` span.
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub provider_id: Option<ProviderId>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub mode: CourseGenerationMode,
    /// 课程类型（beta）：`learning_graph` 走学习图生成（描述即学习目标），
    /// 缺省为传统课程。
    #[serde(default)]
    pub course_kind: CourseKind,
}

/// 续建学习图生成的请求体：全部字段可选——模型缺省走默认解析，草稿由
/// 服务端按「最近活跃」自行定位（草稿仅在内存存活，TTL 1 小时）。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ResumeLearningGraphRequest {
    #[serde(default)]
    pub provider_id: Option<ProviderId>,
    #[serde(default)]
    pub model: Option<String>,
}

/// 学习图生成状态（后台指示条/取消入口的数据源）。生成在 HTTP 请求内同步
/// 执行，但创建对话框可以随时关闭——注册表让运行对外可发现、可取消。
#[derive(Debug, Serialize)]
pub struct LearningGraphGenerationStatus {
    pub running: bool,
    pub topic: Option<String>,
    pub elapsed_secs: Option<u64>,
}

impl GenerateCourseRequest {
    /// Shared request validation for the synchronous generate endpoint and
    /// the agent tool sink: model fields come as a pair and exactly one of
    /// the two generation sources is chosen.
    pub fn validate(&self) -> Result<(), AppError> {
        if self.provider_id.is_some() != self.model.is_some() {
            return Err(AppError::BadRequest(
                "provider_id and model must be provided together".into(),
            ));
        }
        if self
            .model
            .as_deref()
            .is_some_and(|model| model.trim().is_empty())
        {
            return Err(AppError::BadRequest("model must not be empty".into()));
        }
        if self.course_kind == CourseKind::LearningGraph {
            // 学习图课程只走描述流：描述即学习目标，知识库采样与
            // 模块/课时数都不参与生成。
            if self.knowledge_base_id.is_some() {
                return Err(AppError::BadRequest(
                    "learning graph courses ground in the description only".into(),
                ));
            }
            let Some(description) = &self.description else {
                return Err(AppError::BadRequest(
                    "learning graph courses require a description (the learning goal)".into(),
                ));
            };
            if description.trim().is_empty() {
                return Err(AppError::BadRequest("description must not be empty".into()));
            }
            return Ok(());
        }
        if self.knowledge_base_id.is_some() == self.description.is_some() {
            return Err(AppError::BadRequest(
                "exactly one of knowledge_base_id or description must be provided".into(),
            ));
        }
        if let Some(description) = &self.description {
            if description.trim().is_empty() {
                return Err(AppError::BadRequest("description must not be empty".into()));
            }
        }
        Ok(())
    }
}

/// Course generation strategy. Only `on_demand` exists today: the outline is
/// imported immediately and each lesson's body and activities are generated
/// only when the learner opens it. The `mode` field and the
/// `learning_course_jobs.generation_mode` column are kept as extension points
/// for future generation strategies (e.g. a learning-graph-driven mode); any
/// other value is rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CourseGenerationMode {
    #[default]
    OnDemand,
}

impl CourseGenerationMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OnDemand => "on_demand",
        }
    }
}

impl TryFrom<&str> for CourseGenerationMode {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
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

/// A lesson figure that failed to render, sent back for AI repair. `language`
/// is the fence language (`svg` or `jsxgraph`), `error` the renderer error
/// message, `code` the original figure source.
#[derive(Debug, Clone, Deserialize)]
pub struct RepairFigureRequest {
    pub language: String,
    pub code: String,
    pub error: String,
}

/// Corrected figure body returned by the repair call, rendered in place of
/// the broken one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairFigureResponse {
    pub code: String,
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
    /// 学习图节点跳过：学习者声明已掌握、跳过学习。它满足前置条件
    /// （解锁下游），但不等于 completed：不种复习项、不进推荐候选，
    /// completed_at 保持 NULL。取消跳过 = 传回 not_started。
    Skipped,
}

impl LessonStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Skipped => "skipped",
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
            "skipped" => Ok(Self::Skipped),
            other => Err(format!("unsupported lesson status: {other}")),
        }
    }
}

impl LessonStatus {
    /// 该状态是否视为「已满足」：completed 与 skipped 都解锁下游节点、
    /// 不再进入推荐候选。跳过是学习图（beta）的能力，但对传统课时
    /// 同样语义自洽（声明已掌握）。
    pub const fn satisfies(self) -> bool {
        matches!(self, Self::Completed | Self::Skipped)
    }
}

/// 课程目录类型。`traditional` 为模块/课时大纲课程；`learning_graph`
/// （beta）由 AI 把宽泛学习目标拆解为前置 DAG，课程大纲被
/// 「下一步推荐学习的节点」取代。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CourseKind {
    #[default]
    Traditional,
    LearningGraph,
}

impl CourseKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Traditional => "traditional",
            Self::LearningGraph => "learning_graph",
        }
    }
}

impl TryFrom<&str> for CourseKind {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "traditional" => Ok(Self::Traditional),
            "learning_graph" => Ok(Self::LearningGraph),
            other => Err(format!("unsupported course kind: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CourseSummary {
    pub id: LearningCourseId,
    pub title: String,
    pub description: String,
    pub domain: String,
    pub course_kind: CourseKind,
    pub source_kb_id: Option<KnowledgeBaseId>,
    pub version: i64,
    pub enrolled: bool,
    pub total_lessons: i64,
    pub completed_lessons: i64,
    pub updated_at: TimestampMs,
    pub tags: Vec<String>,
}

/// 学习图课程视图的一个节点：底层课时（标题/摘要/估计分钟/生成状态）
/// 加图坐标（拓扑序 position、depth 层深）与学习者进度状态。正文永远
/// 不进全图载荷——内容经现有课时接口按需拉取。
#[derive(Debug, Clone, Serialize)]
pub struct GraphNodeView {
    pub lesson_id: LearningLessonId,
    pub title: String,
    pub summary: String,
    pub purpose: String,
    pub estimated_minutes: i64,
    pub generated: bool,
    /// 发布时的 Kahn 拓扑序（也是推荐排序键）。
    pub position: i64,
    /// 前置层深（零前置为 0），供分层渲染与宏观 LOD 使用。
    pub depth: i64,
    pub status: LessonStatus,
    pub prerequisite_count: i64,
}

/// 学习图课程视图的一条前置边：`from` 应先于 `to` 被满足（lesson_id 引用）。
#[derive(Debug, Clone, Serialize)]
pub struct GraphEdgeView {
    pub from: LearningLessonId,
    pub to: LearningLessonId,
    pub reason: String,
}

/// 学习图课程的图视图（挂在 `CourseDetail.graph` 下）：图结构的事实来源
/// 是 lessons + prerequisites 两张表，这里只做投影；`recommended` 是
/// 「下一步推荐学习的节点」（≤10，就绪集按拓扑序）。
#[derive(Debug, Clone, Serialize)]
pub struct LearningGraphView {
    /// 用户生成图时输入的学习目标。
    pub goal: String,
    /// 学习范围（scope 分析文本）。
    pub scope: String,
    pub nodes: Vec<GraphNodeView>,
    pub edges: Vec<GraphEdgeView>,
    pub recommended: Vec<LearningLessonId>,
    /// 课程行 graph_meta_json 透传（审计快照/生成留档/扩展备注）。
    pub meta: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CourseDetail {
    pub course: CourseSummary,
    pub enrollment_id: Option<LearningEnrollmentId>,
    pub modules: Vec<ModuleView>,
    pub concepts: Vec<ConceptView>,
    pub next_lesson_id: Option<LearningLessonId>,
    pub due_review_count: i64,
    /// 仅 `learning_graph` 课程携带：图结构 + 下一步推荐节点。
    pub graph: Option<LearningGraphView>,
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
    /// Marked "edit me later" from the review session; the card keeps its
    /// schedule untouched and a note (optional) records the intent.
    pub edit_pending: bool,
    pub edit_note: Option<String>,
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
    /// Cards due on this review day; overdue cards roll into the current
    /// day so the today cell matches the review banner's due queue.
    pub due_count: i64,
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
    /// Marked "edit me later" from the review session; the note (optional)
    /// records what the learner intended to change.
    pub edit_pending: bool,
    pub edit_note: Option<String>,
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

/// Marks a review card as "edit me later"; the note is optional and purely
/// for the learner to recall the intended edit.
#[derive(Debug, Clone, Deserialize)]
pub struct MarkEditRequest {
    #[serde(default)]
    pub note: Option<String>,
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

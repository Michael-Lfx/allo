pub(super) use std::cmp::Ordering;
pub(super) use std::collections::{HashMap, HashSet};
pub(super) use std::str::FromStr;
pub(super) use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
pub(super) use std::sync::{Arc, Mutex, RwLock};

pub(super) use chrono::Datelike;
pub(super) use nomifun_common::{
    AppError, KnowledgeBaseId, LearningActivityId, LearningAttemptId, LearningConceptId,
    LearningCourseId, LearningEnrollmentId, LearningGrowthBatchId,
    LearningLessonId, LearningModuleId, LearningReviewItemId, LearningTagId, ProviderId, UserId,
    UuidV7Error, generate_id, now_ms,
};
pub(super) use nomifun_db::SqlitePool;
pub(super) use nomifun_knowledge::KnowledgeService;
pub(super) use serde_json::Value;
pub(super) use sqlx::{Row, Sqlite, Transaction};

pub(super) use crate::completer::LearningCompleter;
pub(super) use crate::events::LearningEventEmitter;
pub(super) use crate::course_outline::draft::OutlineDraft;
pub(super) use crate::course_outline::{CourseOutlineAgentEngine, KnowledgeBaseBrief, OutlineBrief};
pub(super) use crate::lesson_draft::{
    ExcerptRole, GraphLessonContext, LessonContentAgentEngine, LessonDraft, LessonDraftView,
    LessonExcerpt, LessonGenerationContext, LessonInspectView, LessonOp, LessonPatchReport,
};
pub(super) use crate::generation::{
    Blueprint, LessonOutput, SampleBaseDisclosure, SampleDisclosure, annotate_blueprint_sources,
    assemble_outline_pack, base_for_sample_path, build_blueprint_prompt,
    build_description_blueprint_prompt, generate_blueprint, generate_lesson,
    generate_lesson_activity, namespace_for_base, rel_path_for_base, sample_bases,
    validate_blueprint,
};
pub(super) use crate::models::{
    ActivityKind, ActivityView, AttemptResult, CalendarCourseRef, CalendarDayStats,
    CalendarLessonRef, CalendarStats, CheckinStatus, CourseDetail, CourseKind, CoursePack,
    CourseSamplingView, CourseSummary, CreateCustomQuestionRequest, CreateLessonActivityRequest,
    DiagnosticItem, DiagnosticPlan, DueReview, GenerateCourseRequest,
    GenerateLessonActivityRequest, GenerateLessonRequest, GeneratedLessonActivity, SectionKind,
    SectionView, LessonStatus,
    LessonView, MemoryCalibrationBin, MemoryCurvePoint, MemoryHealthStats, MemoryLoadDay,
    MemoryStateBucket, MemoryTrueRetention, ModuleView, QuestionEntry, TeachingStyle,
    ReviewAnswerResult, ReviewQuestion, ReviewRating, ReviewResult,
    ReviewSource, SetTagsRequest, SourceSpan, StoredActivityConfig, UpdateQuestionRequest,
    XpCalendar, XpCalendarDay, XpReading,
};
pub(super) use crate::scheduler::{
    SchedulerSettings, days_elapsed_between, first_review_due_at, predicted_retrievability,
    recall_bucket, review_day_number, review_day_start_utc, schedule_review,
};

/// 大纲建课的落库快照：蓝图与采样维持旧形状（`blueprint_json` /
/// `samples_json`，既有消费方按原格式解析），目标与采样披露走独立列
/// （`learning_goal` / `samples_meta_json`，t1/t2）。无目标路径的
/// `learning_goal` 为空串、`samples_meta_json` 为 `None`（列上 NULL）。
/// 多知识库（t4）：`source_kb_ids` 是归一后的来源库列表（首个 = 主来源），
/// 空列表由导入侧回退 `CoursePack.source_kb_id` 的单元素视图。
pub(crate) struct OutlineSnapshot {
    pub blueprint_json: String,
    pub samples_json: String,
    pub learning_goal: String,
    pub samples_meta_json: Option<String>,
    pub source_kb_ids: Vec<KnowledgeBaseId>,
}

#[derive(Clone)]
pub struct LearningService {
    pool: SqlitePool,
    knowledge_service: Arc<RwLock<Option<Arc<KnowledgeService>>>>,
    course_completer: Arc<RwLock<Option<Arc<dyn LearningCompleter>>>>,
    /// In-memory outline draft store backing the agent tool set (`co_start`
    /// .. `co_finish`). Same lifecycle as the learning-graph drafts:
    /// short-lived, `finish` is the single publish path.
    course_outline_drafts: Arc<RwLock<HashMap<String, OutlineDraft>>>,
    /// Two-loop course outline agent engine; when present, `generate_course`
    /// routes through it (draft + `co_*` tools, audit-gated publish),
    /// otherwise the legacy one-shot pipeline runs.
    course_outline_engine: Arc<RwLock<Option<Arc<dyn CourseOutlineAgentEngine>>>>,
    /// In-memory draft store backing the agent tool set (`ls_start` ..
    /// /// `ls_finish`). Entries carry their last-activity timestamp and are
    /// evicted lazily (see `service::lesson_draft::LESSON_DRAFT_TTL`) so
    /// abandoned drafts cannot leak memory; `finish` is the single publish
    /// path.
    lesson_drafts: Arc<RwLock<HashMap<String, (LessonDraft, std::time::Instant)>>>,
    /// 课时内容草稿的断点续跑映射（lesson_id → draft_id）：生成失败后草
    /// 稿仍在 TTL 内时，重试同一课时经 [`Self::live_lesson_draft_for_lesson`]
    /// 定位草稿续跑而非从零重建。发布成功即清除；草稿过期后查找自然落空
    /// （回退全新生成）。内存态，与草稿存储同生命周期（重启即失）。
    lesson_draft_ids: Arc<Mutex<HashMap<String, String>>>,
    /// Two-loop lesson content agent engine; when present,
    /// `generate_lesson_content` routes through it (draft + `ls_*` tools,
    /// audit-gated publish), otherwise the legacy two-stage pipeline runs.
    lesson_engine: Arc<RwLock<Option<Arc<dyn LessonContentAgentEngine>>>>,
    /// Best-effort WebSocket event emitter for generation progress; `None`
    /// in tests and standalone runs, where events are silently skipped.
    event_sink: Arc<RwLock<Option<LearningEventEmitter>>>,
    /// One in-flight course generation per (user, source key) — the HTTP
    /// endpoint runs the whole loop synchronously and the agent-session
    /// path runs it in a background task, so a duplicate submit (parallel
    /// tool calls, double click) is rejected up front instead of silently
    /// producing two near-identical courses.
    generation_slots: Arc<Mutex<HashSet<(UserId, String)>>>,
    /// In-memory registry backing the agent-session course generations:
    /// the sink spawns the synchronous pipeline as a background task and
    /// the session's status tool polls here. Memory-only by design — a
    /// restart loses in-flight generations, and a 1-3 minute task is
    /// simply re-issued.
    agent_course_jobs: Arc<Mutex<HashMap<String, AgentCourseJobEntry>>>,
    /// 课程生成的运行注册与取消（学习图生长/结构、大纲、课时四类长任务
    /// 共用，是 status/cancel/快照端点的唯一数据源，见
    /// [`generation_tasks`]）：每任务一枚 CancellationToken——任务体在检查
    /// 点轮询 + `select!` 真中断；取消端点触发 token；终态首写即定（取消
    /// 先到 = cancelled，任务先完成 = 竞速落败如实 done）。内存态，重启
    /// 即清（终态留痕由批行/草稿等持久面承担）。
    generation_tasks: Arc<GenerationTaskRegistry>,
    /// 最近一次生长的重试读数（attempts_used/stop_reason，二轮 t1 smoke
    /// 读数面）。内存态，仅诊断。
    growth_run_stats: Arc<Mutex<Option<GrowthRunStats>>>,
    /// companion XP 桥（j05 统一账）：结算落账后的同步授予接缝。缺省
    /// `None`（测试/CLI/无伙伴花名册）——只落学习账，companion 余额滞后。
    companion_xp_sink: Arc<RwLock<Option<Arc<dyn CompanionXpSink>>>>,
}

/// 生长循环的重试读数（learning-smoke grow/loop 输出用；读数面非门禁）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct GrowthRunStats {
    pub attempts_used: usize,
    /// success | early_stop | exhausted | cancelled。
    pub stop_reason: String,
}

/// 生成注册的内部状态（见 [`LearningService::generation_tasks`]）。
pub(crate) type GenerationRegistry = generation_tasks::GenerationTaskRegistry;

impl LearningService {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            knowledge_service: Arc::new(RwLock::new(None)),
            course_completer: Arc::new(RwLock::new(None)),
            course_outline_drafts: Arc::new(RwLock::new(HashMap::new())),
            course_outline_engine: Arc::new(RwLock::new(None)),
            lesson_drafts: Arc::new(RwLock::new(HashMap::new())),
            lesson_draft_ids: Arc::new(Mutex::new(HashMap::new())),
            lesson_engine: Arc::new(RwLock::new(None)),
            event_sink: Arc::new(RwLock::new(None)),
            generation_slots: Arc::new(Mutex::new(HashSet::new())),
            agent_course_jobs: Arc::new(Mutex::new(HashMap::new())),
            generation_tasks: Arc::new(GenerationTaskRegistry::new()),
            growth_run_stats: Arc::new(Mutex::new(None)),
            companion_xp_sink: Arc::new(RwLock::new(None)),
        }
    }

    /// 注入长任务的观测旁路（begin/finish JSONL 行；组合根接
    /// `<data-dir>/observation/learning-tasks.jsonl`，写失败降级）。
    pub fn set_generation_task_observer(
        &self,
        observer: Arc<dyn Fn(&str) + Send + Sync>,
    ) {
        self.generation_tasks.set_observer(observer);
    }

    pub fn set_generation_dependencies(
        &self,
        knowledge_service: Arc<KnowledgeService>,
        completer: Arc<dyn LearningCompleter>,
    ) {
        *self
            .knowledge_service
            .write()
            .expect("learning knowledge service lock poisoned") = Some(knowledge_service);
        *self
            .course_completer
            .write()
            .expect("learning course completer lock poisoned") = Some(completer);
    }

    /// Inject the best-effort WebSocket emitter for generation progress
    /// (wiring-time, before any request). Mirrors the knowledge service's
    /// emitter injection; absent, all event emissions are skipped.
    pub fn set_event_sink(&self, emitter: LearningEventEmitter) {
        *self
            .event_sink
            .write()
            .expect("learning event sink lock poisoned") = Some(emitter);
    }

    /// Clone of the injected event emitter, `None` when unconfigured. Public
    /// because the agent engines (nomifun-ai-agent) emit their loop events
    /// through the same sink seam.
    pub fn event_emitter(&self) -> Option<LearningEventEmitter> {
        self.event_sink
            .read()
            .ok()
            .and_then(|guard| guard.clone())
    }

    /// Best-effort course-outline generation progress push; silently
    /// skipped when no event sink is wired (tests, CLI).
    pub fn emit_course_event(&self, payload: serde_json::Value) {
        if let Some(emitter) = self.event_emitter() {
            emitter.emit_course_generation(&payload);
        }
    }

    /// Best-effort lesson-content generation progress push; same contract
    /// as [`Self::emit_course_event`].
    pub fn emit_lesson_event(&self, payload: serde_json::Value) {
        if let Some(emitter) = self.event_emitter() {
            emitter.emit_lesson_generation(&payload);
        }
    }

    /// Acquire the (user, source key) generation slot. The RAII guard
    /// releases the slot on drop, so a failed or panicked generation never
    /// wedges the user's next submit.
    pub(crate) fn acquire_generation_slot(
        &self,
        user_id: &UserId,
        kind_key: String,
    ) -> Result<GenerationSlotGuard, AppError> {
        let key = (user_id.clone(), kind_key);
        let mut slots = self.generation_slots.lock().map_err(|_| {
            AppError::Internal("learning generation slot registry poisoned".into())
        })?;
        if !slots.insert(key.clone()) {
            return Err(AppError::Conflict(format!(
                "course generation for this source is already running ({:?}); \
                 wait for it to finish before starting another",
                key.1
            )));
        }
        Ok(GenerationSlotGuard {
            slots: Arc::clone(&self.generation_slots),
            key,
        })
    }

    /// Start an agent-session course generation: validate the request,
    /// register an in-memory job, and run the synchronous pipeline in a
    /// background task. Returns the job handle immediately — the session
    /// tool polls [`Self::agent_course_job_status`]; the generation
    /// honors the same (user, source) slot guard as the HTTP endpoint, so
    /// a duplicate submit surfaces as a failed job with the conflict.
    pub fn start_agent_course_generation(
        &self,
        user_id: &UserId,
        request: GenerateCourseRequest,
    ) -> Result<String, AppError> {
        // Fail fast on an invalid request (source/size/provider rules)
        // before anything is registered.
        request.validate()?;
        let job_id = generate_id();
        self.agent_course_jobs
            .lock()
            .map_err(|_| AppError::Internal("learning agent job registry poisoned".into()))?
            .insert(
                job_id.clone(),
                AgentCourseJobEntry {
                    user_id: user_id.as_str().to_owned(),
                    status: "running",
                    course_id: None,
                    title: None,
                    error: None,
                    abort: None,
                },
            );
        let service = self.clone();
        let owner = user_id.clone();
        let task_job_id = job_id.clone();
        let handle = tokio::spawn(async move {
            let result = service.generate_course(&owner, request).await;
            let Ok(mut jobs) = service.agent_course_jobs.lock() else {
                return;
            };
            if let Some(entry) = jobs.get_mut(&task_job_id) {
                match result {
                    Ok(detail) => {
                        entry.status = "completed";
                        entry.course_id = Some(detail.course.id.as_str().to_owned());
                        entry.title = Some(detail.course.title.clone());
                    }
                    Err(error) => {
                        entry.status = "failed";
                        entry.error = Some(error.to_string());
                    }
                }
                entry.abort = None;
            }
        });
        if let Ok(mut jobs) = self.agent_course_jobs.lock() {
            if let Some(entry) = jobs.get_mut(&job_id) {
                entry.abort = Some(handle.abort_handle());
            }
        }
        Ok(job_id)
    }

    /// Poll one agent-session generation. `None` when the job id is
    /// unknown to this user (foreign and absent jobs are
    /// indistinguishable, mirroring the HTTP 404).
    pub fn agent_course_job_status(
        &self,
        user_id: &UserId,
        job_id: &str,
    ) -> Option<AgentCourseJobView> {
        let jobs = self.agent_course_jobs.lock().ok()?;
        let entry = jobs.get(job_id)?;
        if entry.user_id != user_id.as_str() {
            return None;
        }
        Some(AgentCourseJobView {
            status: entry.status.to_owned(),
            course_id: entry.course_id.clone(),
            title: entry.title.clone(),
            error: entry.error.clone(),
        })
    }

    /// Cancel one agent-session generation: abort the background task (the
    /// RAII slot guard drops with it) and mark the entry cancelled. An
    /// unknown or foreign job is a not-found.
    pub fn cancel_agent_course_job(&self, user_id: &UserId, job_id: &str) -> Result<(), AppError> {
        let mut jobs = self
            .agent_course_jobs
            .lock()
            .map_err(|_| AppError::Internal("learning agent job registry poisoned".into()))?;
        let entry = jobs
            .get_mut(job_id)
            .ok_or_else(|| AppError::NotFound(format!("course generation job {job_id}")))?;
        if entry.user_id != user_id.as_str() {
            return Err(AppError::NotFound(format!("course generation job {job_id}")));
        }
        if let Some(abort) = entry.abort.take() {
            abort.abort();
        }
        // Only downgrade a still-running job: a task that already finished
        // keeps its terminal state (completed/failed wins over the cancel).
        if entry.status == "running" {
            entry.status = "cancelled";
        }
        Ok(())
    }

    /// Inject the two-loop course outline agent engine (wiring-time, before
    /// any request). Absent, `generate_course` falls back to the legacy
    /// one-shot pipeline.
    pub fn set_course_outline_engine(&self, engine: Arc<dyn CourseOutlineAgentEngine>) {
        *self
            .course_outline_engine
            .write()
            .expect("learning course outline engine lock poisoned") = Some(engine);
    }

    /// Clone of the injected outline engine, `None` when unconfigured.
    pub fn course_outline_engine(&self) -> Option<Arc<dyn CourseOutlineAgentEngine>> {
        self.course_outline_engine
            .read()
            .ok()
            .and_then(|guard| guard.clone())
    }

    /// Inject the two-loop lesson content agent engine (wiring-time, before
    /// any request). Absent, `generate_lesson_content` falls back to the
    /// legacy two-stage pipeline.
    pub fn set_lesson_engine(&self, engine: Arc<dyn LessonContentAgentEngine>) {
        *self
            .lesson_engine
            .write()
            .expect("learning lesson engine lock poisoned") = Some(engine);
    }

    /// Clone of the injected lesson engine, `None` when unconfigured.
    pub fn lesson_engine(&self) -> Option<Arc<dyn LessonContentAgentEngine>> {
        self.lesson_engine
            .read()
            .ok()
            .and_then(|guard| guard.clone())
    }

    /// Test-only access to the underlying pool (e.g. to simulate a stale
    /// seeded course version).
    #[cfg(test)]
    pub(crate) fn pool_for_tests(&self) -> &SqlitePool {
        &self.pool
    }

    /// The injected knowledge service, or a conflict when the learning
    /// service was constructed without generation dependencies (used by the
    /// tutorial seed, which needs knowledge-base registration and file IO).
    pub(crate) fn injected_knowledge_service(&self) -> Result<Arc<KnowledgeService>, AppError> {
        self.knowledge_service
            .read()
            .map_err(|_| AppError::Internal("learning knowledge service lock poisoned".into()))?
            .clone()
            .ok_or_else(|| {
                AppError::Conflict("knowledge-backed learning is not configured".into())
            })
    }

    async fn lesson_activities(
        &self,
        lesson_id: &LearningLessonId,
    ) -> Result<Vec<ActivityView>, AppError> {
        let rows = sqlx::query(
            "SELECT activity_id, kind, prompt, config_json, position FROM learning_activities \
             WHERE lesson_id = ? ORDER BY position, activity_id",
        )
        .bind(lesson_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut activities = Vec::with_capacity(rows.len());
        for row in rows {
            let id: LearningActivityId =
                parse_id(row.try_get("activity_id").map_err(internal)?)?;
            let kind_text: String = row.try_get("kind").map_err(internal)?;
            let config: StoredActivityConfig = serde_json::from_str(
                &row.try_get::<String, _>("config_json").map_err(internal)?,
            )
            .map_err(internal)?;
            activities.push(ActivityView {
                id,
                kind: ActivityKind::try_from(kind_text.as_str())
                    .map_err(AppError::Internal)?,
                prompt: row.try_get("prompt").map_err(internal)?,
                options: config.options,
                matches: config.matches,
                position: row.try_get("position").map_err(internal)?,
            });
        }
        Ok(activities)
    }

}

fn parse_id<T>(value: String) -> Result<T, AppError>
where
    T: FromStr<Err = UuidV7Error>,
{
    value
        .parse()
        .map_err(|error| AppError::Internal(format!("invalid persisted ID {value}: {error}")))
}

fn internal(error: impl std::fmt::Display) -> AppError {
    AppError::Internal(error.to_string())
}

/// RAII handle for one in-flight generation slot; dropping it releases the
/// (user, source key) pair so failed or cancelled generations never wedge
/// the user's next submit.
pub(crate) struct GenerationSlotGuard {
    slots: Arc<Mutex<HashSet<(UserId, String)>>>,
    key: (UserId, String),
}

impl Drop for GenerationSlotGuard {
    fn drop(&mut self) {
        if let Ok(mut slots) = self.slots.lock() {
            slots.remove(&self.key);
        }
    }
}

/// RAII deregistration for one registered generation run; dropping it
/// clears the run entry so every exit path (normal return, failure,
/// aborted HTTP handler, aborted agent task) leaves no stale entry behind.
/// See `service::learning_graph::begin_generation`.
pub(crate) struct GenerationRunGuard<'a>(&'a LearningService);

impl Drop for GenerationRunGuard<'_> {
    fn drop(&mut self) {
        // 终态收尾已由 GenerationTaskGuard 的显式 finish 承担；守卫类型
        // 保留给未迁移的旧调用面（空操作）。
        let _ = self.0;
    }
}

/// Status snapshot of one agent-session course generation: the entry the
/// session's status tool polls. `course_id`/`title` are filled only when
/// the generation completed; `error` only when it failed.
#[derive(Debug, Clone)]
pub struct AgentCourseJobView {
    /// running | completed | failed | cancelled.
    pub status: String,
    pub course_id: Option<String>,
    pub title: Option<String>,
    pub error: Option<String>,
}

/// Internal registry entry: the pollable snapshot plus the abort handle
/// that backs cancellation.
struct AgentCourseJobEntry {
    user_id: String,
    status: &'static str,
    course_id: Option<String>,
    title: Option<String>,
    error: Option<String>,
    abort: Option<tokio::task::AbortHandle>,
}

mod checkin;
mod learning_graph;
mod course;
mod course_outline;
mod diagnostic;
mod generation_tasks;
mod generate;
mod lesson;
mod lesson_draft;
mod memory;
mod progress;
mod review;
mod tags;
mod xp;

pub use xp::CompanionXpSink;

pub(crate) use generation_tasks::{
    CANCELLED_MESSAGE, CancellationCompleter, GenerationTaskGuard, GenerationTaskRegistry,
    GenerationTaskView, KIND_GROWTH, KIND_LESSON, KIND_OUTLINE, KIND_STRUCTURE,
    KIND_STRUCTURE_MOUNT, KIND_STRUCTURE_REPAIR, STATE_RUNNING,
};

#[cfg(test)]
mod tests;

use self::course::CourseKbSources;
use self::lesson::validate_question_payload;
use self::progress::{ensure_review_item, evaluate, update_mastery_and_review};

pub(super) use std::cmp::Ordering;
pub(super) use std::collections::{HashMap, HashSet};
pub(super) use std::str::FromStr;
pub(super) use std::sync::{Arc, Mutex, RwLock};

pub(super) use chrono::Datelike;
pub(super) use nomifun_common::{
    AppError, KnowledgeBaseId, LearningActivityId, LearningAttemptId, LearningGraphId,
    LearningConceptId, LearningCourseId, LearningEnrollmentId, LearningLessonId,
    LearningModuleId, LearningReviewItemId, LearningTagId, ProviderId, UserId, UuidV7Error,
    generate_id, now_ms,
};
pub(super) use nomifun_db::SqlitePool;
pub(super) use nomifun_knowledge::KnowledgeService;
pub(super) use serde_json::Value;
pub(super) use sqlx::{Row, Sqlite, Transaction};

pub(super) use crate::completer::LearningCompleter;
pub(super) use crate::events::LearningEventEmitter;
pub(super) use crate::learning_graph::draft::DraftGraph;
pub(super) use crate::learning_graph::LearningGraphAgentEngine;
pub(super) use crate::course_outline::draft::OutlineDraft;
pub(super) use crate::course_outline::{CourseOutlineAgentEngine, KnowledgeBaseBrief, OutlineBrief};
pub(super) use crate::lesson_draft::{
    GraphLessonContext, LessonContentAgentEngine, LessonDraft, LessonDraftView, LessonExcerpt,
    LessonGenerationContext, LessonInspectView, LessonOp, LessonPatchReport,
};
pub(super) use crate::generation::{
    Blueprint, LessonOutput, assemble_outline_pack, build_blueprint_prompt,
    build_description_blueprint_prompt, generate_blueprint, generate_lesson,
    generate_lesson_activity, sample_base_files, validate_blueprint,
};
pub(super) use crate::models::{
    ActivityKind, ActivityView, AttemptResult, CalendarCourseRef, CalendarDayStats,
    CalendarLessonRef, CalendarStats, CheckinStatus, ConceptPack, ConceptRef, ConceptView,
    CourseDetail, CourseKind, CoursePack, CourseSummary,
    CreateCustomQuestionRequest, CreateLessonActivityRequest, DiagnosticItem, DiagnosticPlan,
    DueReview, GenerateCourseRequest, GenerateLessonActivityRequest, GenerateLessonRequest,
    GraphEdgeView, GraphNodeView, GeneratedLessonActivity, LearningGraphView, LessonStatus,
    LessonView, ModuleView, QuestionEntry,
    ReviewAnswerResult, ReviewQuestion, ReviewRating, ReviewResult,
    ReviewSource, SetTagsRequest, SourceSpan, StoredActivityConfig, UpdateQuestionRequest,
};
pub(super) use crate::scheduler::{
    SchedulerSettings, first_review_due_at, review_day_number, review_day_start_utc, schedule_review,
};
#[derive(Clone)]
pub struct LearningService {
    pool: SqlitePool,
    knowledge_service: Arc<RwLock<Option<Arc<KnowledgeService>>>>,
    course_completer: Arc<RwLock<Option<Arc<dyn LearningCompleter>>>>,
    /// In-memory learning-graph draft store backing the agent tool set
    /// (`lg_start` .. `lg_finish`). Generation is a short-lived operation,
    /// so drafts do not survive restarts; only `lg_finish` publishes to
    /// the database. Each entry carries its last-activity timestamp: stale
    /// drafts are evicted lazily (see
    /// `service::learning_graph::LEARNING_GRAPH_DRAFT_TTL`), so crashed or
    /// timed-out generation sessions cannot leak memory.
    learning_graph_drafts: Arc<RwLock<HashMap<String, (DraftGraph, std::time::Instant)>>>,
    /// Two-loop agent engine; when present, `generate_learning_graph` routes
    /// through it (draft + `lg_*` tools, audit-gated publish).
    learning_graph_engine: Arc<RwLock<Option<Arc<dyn LearningGraphAgentEngine>>>>,
    /// In-memory outline draft store backing the agent tool set (`co_start`
    /// .. `co_finish`). Same lifecycle as the learning-graph drafts:
    /// short-lived, `finish` is the single publish path.
    course_outline_drafts: Arc<RwLock<HashMap<String, OutlineDraft>>>,
    /// Two-loop course outline agent engine; when present, `generate_course`
    /// routes through it (draft + `co_*` tools, audit-gated publish),
    /// otherwise the legacy one-shot pipeline runs.
    course_outline_engine: Arc<RwLock<Option<Arc<dyn CourseOutlineAgentEngine>>>>,
    /// In-memory draft store backing the agent tool set (`ls_start` ..
    /// `ls_finish`). Same lifecycle as the outline drafts: short-lived,
    /// `finish` is the single publish path.
    lesson_drafts: Arc<RwLock<HashMap<String, LessonDraft>>>,
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
}

impl LearningService {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            knowledge_service: Arc::new(RwLock::new(None)),
            course_completer: Arc::new(RwLock::new(None)),
            learning_graph_drafts: Arc::new(RwLock::new(HashMap::new())),
            learning_graph_engine: Arc::new(RwLock::new(None)),
            course_outline_drafts: Arc::new(RwLock::new(HashMap::new())),
            course_outline_engine: Arc::new(RwLock::new(None)),
            lesson_drafts: Arc::new(RwLock::new(HashMap::new())),
            lesson_engine: Arc::new(RwLock::new(None)),
            event_sink: Arc::new(RwLock::new(None)),
            generation_slots: Arc::new(Mutex::new(HashSet::new())),
            agent_course_jobs: Arc::new(Mutex::new(HashMap::new())),
        }
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

    /// Inject the two-loop learning-graph agent engine (wiring-time, before
    /// any request). Generation requires the engine — there is no fallback.
    pub fn set_learning_graph_engine(&self, engine: Arc<dyn LearningGraphAgentEngine>) {
        *self
            .learning_graph_engine
            .write()
            .expect("learning graph engine lock poisoned") = Some(engine);
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

    async fn lesson_concepts(
        &self,
        lesson_id: &LearningLessonId,
    ) -> Result<Vec<LearningConceptId>, AppError> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT concept_id FROM learning_lesson_concepts WHERE lesson_id = ? ORDER BY concept_id",
        )
        .bind(lesson_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        ids.into_iter().map(parse_id).collect()
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
            let concept_ids: Vec<String> = sqlx::query_scalar(
                "SELECT concept_id FROM learning_activity_concepts \
                 WHERE activity_id = ? ORDER BY concept_id",
            )
            .bind(id.as_str())
            .fetch_all(&self.pool)
            .await
            .map_err(internal)?;
            activities.push(ActivityView {
                id,
                kind: ActivityKind::try_from(kind_text.as_str())
                    .map_err(AppError::Internal)?,
                prompt: row.try_get("prompt").map_err(internal)?,
                options: config.options,
                position: row.try_get("position").map_err(internal)?,
                concepts: concept_ids
                    .into_iter()
                    .map(parse_id)
                    .collect::<Result<_, _>>()?,
            });
        }
        Ok(activities)
    }

    async fn course_concepts(
        &self,
        course_id: &LearningCourseId,
        enrollment_id: Option<&LearningEnrollmentId>,
    ) -> Result<Vec<ConceptView>, AppError> {
        let rows = sqlx::query(
            "SELECT c.concept_id, c.concept_key, c.title, c.description, m.mastery \
             FROM learning_concepts c \
             LEFT JOIN learning_mastery_states m \
               ON m.concept_id = c.concept_id AND m.enrollment_id = ? \
             WHERE c.course_id = ? ORDER BY c.concept_key, c.concept_id",
        )
        .bind(enrollment_id.map(LearningEnrollmentId::as_str))
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut concepts = Vec::with_capacity(rows.len());
        for row in rows {
            let id: LearningConceptId =
                parse_id(row.try_get("concept_id").map_err(internal)?)?;
            let prerequisites: Vec<String> = sqlx::query_scalar(
                "SELECT prerequisite_concept_id FROM learning_concept_prerequisites \
                 WHERE concept_id = ? ORDER BY prerequisite_concept_id",
            )
            .bind(id.as_str())
            .fetch_all(&self.pool)
            .await
            .map_err(internal)?;
            concepts.push(ConceptView {
                id,
                key: row.try_get("concept_key").map_err(internal)?,
                title: row.try_get("title").map_err(internal)?,
                description: row.try_get("description").map_err(internal)?,
                prerequisites: prerequisites
                    .into_iter()
                    .map(parse_id)
                    .collect::<Result<_, _>>()?,
                mastery: row.try_get("mastery").map_err(internal)?,
            });
        }
        Ok(concepts)
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
mod generate;
mod lesson;
mod lesson_draft;
mod progress;
mod review;
mod tags;

#[cfg(test)]
mod tests;

use self::lesson::validate_question_payload;
use self::progress::{ensure_review_item, evaluate, update_activity_mastery, update_mastery_and_review};

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::{Arc, RwLock};

use nomifun_common::{
    AppError, KnowledgeBaseId, LearningActivityId, LearningAttemptId, LearningConceptId,
    LearningCourseId, LearningEnrollmentId, LearningLessonId, LearningModuleId,
    LearningReviewItemId, LearningTagId, ProviderId, UserId, UuidV7Error, generate_id, now_ms,
};
use nomifun_db::SqlitePool;
use nomifun_knowledge::{KnowledgeCompleter, KnowledgeService};
use serde_json::Value;
use sqlx::{Row, Sqlite, Transaction};

use crate::models::{
    ActivityKind, ActivityView, AttemptResult, ConceptRef, ConceptView, CourseDetail, CourseJobSource,
    CourseJobStatus, CourseJobView, CoursePack, CourseSummary, CreateCustomQuestionRequest,
    DiagnosticItem, DiagnosticPlan, DueReview, GenerateCourseRequest, LessonStatus, LessonView,
    ModuleView, QuestionEntry, RetryCourseJobRequest, ReviewAnswerResult, ReviewQuestion,
    ReviewRating, ReviewResult, ReviewSource, SetTagsRequest, SourceSpan, StoredActivityConfig,
    UpdateQuestionRequest,
};
use crate::generation_job::GenerationJobRunner;
use crate::scheduler::{SchedulerSettings, schedule_review};

#[derive(Clone)]
pub struct LearningService {
    pool: SqlitePool,
    knowledge_service: Arc<RwLock<Option<Arc<KnowledgeService>>>>,
    course_completer: Arc<RwLock<Option<Arc<dyn KnowledgeCompleter>>>>,
}

impl LearningService {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            knowledge_service: Arc::new(RwLock::new(None)),
            course_completer: Arc::new(RwLock::new(None)),
        }
    }

    pub fn set_generation_dependencies(
        &self,
        knowledge_service: Arc<KnowledgeService>,
        completer: Arc<dyn KnowledgeCompleter>,
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

    pub async fn generate_course(
        &self,
        request: GenerateCourseRequest,
    ) -> Result<CourseDetail, AppError> {
        validate_generation_request(&request)?;
        let knowledge_service = self
            .knowledge_service
            .read()
            .map_err(|_| AppError::Internal("learning knowledge service lock poisoned".into()))?
            .clone()
            .ok_or_else(|| {
                AppError::Conflict("knowledge-backed course generation is not configured".into())
            })?;
        let completer = self
            .course_completer
            .read()
            .map_err(|_| AppError::Internal("learning course completer lock poisoned".into()))?
            .clone()
            .ok_or_else(|| {
                AppError::Conflict("knowledge-backed course generation is not configured".into())
            })?;
        let pack = crate::generation::generate_course_pack(
            knowledge_service.as_ref(),
            completer.as_ref(),
            &request,
        )
        .await?;
        self.import_course(pack).await
    }

    /// Submit a course-generation job and return immediately. The pipeline
    /// runs in the background (one spawned task per claimed job); progress is
    /// visible through `list_course_jobs` / `course_job`. Used by both the
    /// HTTP generate endpoint and the agent tool so the two flows share one
    /// job registry, cancel/resume/retry semantics and crash recovery.
    pub async fn start_course_job(
        &self,
        request: GenerateCourseRequest,
        user_id: &UserId,
        source: CourseJobSource,
        session_id: Option<&str>,
    ) -> Result<CourseJobView, AppError> {
        validate_generation_request(&request)?;
        let exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM knowledge_bases WHERE knowledge_base_id = ?",
        )
        .bind(request.knowledge_base_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        if exists == 0 {
            return Err(AppError::BadRequest(format!(
                "knowledge base {} does not exist",
                request.knowledge_base_id
            )));
        }
        let job_id = generate_id();
        let now = now_ms();
        let request_json = serde_json::to_string(&request).map_err(internal)?;
        sqlx::query(
            "INSERT INTO learning_course_jobs \
             (job_id, user_id, session_id, source, kb_id, request_json, status, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, 'queued', ?, ?)",
        )
        .bind(&job_id)
        .bind(user_id.as_str())
        .bind(session_id)
        .bind(source.as_str())
        .bind(request.knowledge_base_id.as_str())
        .bind(&request_json)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        self.generation_runner()?.claim_and_spawn(&job_id).await?;
        self.require_course_job(user_id, &job_id).await
    }

    /// The user's course-generation jobs, most recently updated first.
    pub async fn list_course_jobs(&self, user_id: &UserId) -> Result<Vec<CourseJobView>, AppError> {
        let rows = sqlx::query(
            "SELECT j.job_id, j.source, j.status, j.current_module, j.current_lesson, \
                    j.total_lessons, j.error, j.course_id, j.created_at, j.updated_at, \
                    j.request_json, b.name AS knowledge_base_name \
             FROM learning_course_jobs j \
             LEFT JOIN knowledge_bases b ON b.knowledge_base_id = j.kb_id \
             WHERE j.user_id = ? ORDER BY j.updated_at DESC, j.job_id DESC",
        )
        .bind(user_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut jobs = Vec::with_capacity(rows.len());
        for row in rows {
            jobs.push(course_job_from_row(&row)?);
        }
        Ok(jobs)
    }

    /// One job for the user, `None` when it does not exist or belongs to
    /// another user (jobs are isolated per user).
    pub async fn course_job(
        &self,
        user_id: &UserId,
        job_id: &str,
    ) -> Result<Option<CourseJobView>, AppError> {
        let row = sqlx::query(
            "SELECT j.job_id, j.source, j.status, j.current_module, j.current_lesson, \
                    j.total_lessons, j.error, j.course_id, j.created_at, j.updated_at, \
                    j.request_json, b.name AS knowledge_base_name \
             FROM learning_course_jobs j \
             LEFT JOIN knowledge_bases b ON b.knowledge_base_id = j.kb_id \
             WHERE j.user_id = ? AND j.job_id = ?",
        )
        .bind(user_id.as_str())
        .bind(job_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        row.as_ref().map(course_job_from_row).transpose()
    }

    /// Request cancellation of a running job. The flag is honored at the
    /// next stage boundary; every completed lesson is kept for a later
    /// resume.
    pub async fn cancel_course_job(
        &self,
        user_id: &UserId,
        job_id: &str,
    ) -> Result<CourseJobView, AppError> {
        let job = self.require_course_job(user_id, job_id).await?;
        if !job.status.is_terminal() {
            sqlx::query(
                "UPDATE learning_course_jobs SET cancel_requested = 1, updated_at = ? \
                 WHERE user_id = ? AND job_id = ?",
            )
            .bind(now_ms())
            .bind(user_id.as_str())
            .bind(job_id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        }
        self.require_course_job(user_id, job_id).await
    }

    /// Delete a terminal job row (completed/failed/cancelled) so the task
    /// panel stays tidy. Running or resumable jobs are rejected — their
    /// progress would be thrown away silently.
    pub async fn delete_course_job(&self, user_id: &UserId, job_id: &str) -> Result<(), AppError> {
        let job = self.require_course_job(user_id, job_id).await?;
        if !matches!(
            job.status,
            CourseJobStatus::Completed | CourseJobStatus::Failed | CourseJobStatus::Cancelled
        ) {
            return Err(AppError::Conflict(format!(
                "course job {job_id} is {}, only terminal jobs can be deleted",
                job.status.as_str()
            )));
        }
        sqlx::query("DELETE FROM learning_course_jobs WHERE user_id = ? AND job_id = ?")
            .bind(user_id.as_str())
            .bind(job_id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        Ok(())
    }

    /// Continue a cancelled or interrupted job from its last persisted
    /// lesson cursor.
    pub async fn resume_course_job(
        &self,
        user_id: &UserId,
        job_id: &str,
    ) -> Result<CourseJobView, AppError> {
        let job = self.require_course_job(user_id, job_id).await?;
        if !matches!(
            job.status,
            CourseJobStatus::Cancelled | CourseJobStatus::Interrupted
        ) {
            return Err(AppError::Conflict(format!(
                "course job {job_id} is {}, only cancelled or interrupted jobs can be resumed",
                job.status.as_str()
            )));
        }
        sqlx::query(
            "UPDATE learning_course_jobs SET cancel_requested = 0, updated_at = ? \
             WHERE user_id = ? AND job_id = ?",
        )
        .bind(now_ms())
        .bind(user_id.as_str())
        .bind(job_id)
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        self.generation_runner()?.claim_and_spawn(job_id).await?;
        self.require_course_job(user_id, job_id).await
    }

    /// Retry a failed job: reruns the failing lesson when the blueprint
    /// survived, otherwise restarts from the blueprint stage. Completed
    /// lessons are never regenerated. An optional model preference re-points
    /// the job's request snapshot at another model so a busy default can be
    /// swapped before the retry re-runs.
    pub async fn retry_course_job(
        &self,
        user_id: &UserId,
        job_id: &str,
        request: &RetryCourseJobRequest,
    ) -> Result<CourseJobView, AppError> {
        let job = self.require_course_job(user_id, job_id).await?;
        if job.status != CourseJobStatus::Failed {
            return Err(AppError::Conflict(format!(
                "course job {job_id} is {}, only failed jobs can be retried",
                job.status.as_str()
            )));
        }
        if request.provider_id.is_some() != request.model.is_some() {
            return Err(AppError::BadRequest(
                "provider_id and model must be provided together".into(),
            ));
        }
        if request
            .model
            .as_deref()
            .is_some_and(|model| model.trim().is_empty())
        {
            return Err(AppError::BadRequest("model must not be empty".into()));
        }
        // A retry is an explicit re-run: discard any stale cancel request so
        // the claim cannot fold the job straight back into `cancelled`.
        sqlx::query(
            "UPDATE learning_course_jobs SET cancel_requested = 0, updated_at = ? \
             WHERE user_id = ? AND job_id = ?",
        )
        .bind(now_ms())
        .bind(user_id.as_str())
        .bind(job_id)
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        if let (Some(provider_id), Some(model)) = (&request.provider_id, &request.model) {
            let snapshot: String = sqlx::query_scalar(
                "SELECT request_json FROM learning_course_jobs WHERE user_id = ? AND job_id = ?",
            )
            .bind(user_id.as_str())
            .bind(job_id)
            .fetch_one(&self.pool)
            .await
            .map_err(internal)?;
            let mut stored: GenerateCourseRequest =
                serde_json::from_str(&snapshot).map_err(internal)?;
            stored.provider_id = Some(provider_id.clone());
            stored.model = Some(model.clone());
            sqlx::query(
                "UPDATE learning_course_jobs SET request_json = ?, updated_at = ? \
                 WHERE user_id = ? AND job_id = ?",
            )
            .bind(serde_json::to_string(&stored).map_err(internal)?)
            .bind(now_ms())
            .bind(user_id.as_str())
            .bind(job_id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        }
        self.generation_runner()?.claim_and_spawn(job_id).await?;
        self.require_course_job(user_id, job_id).await
    }

    /// Boot sweep: jobs left in a running state by a previous process are
    /// marked `interrupted` and re-claimed so generation continues from the
    /// last persisted snapshot. Returns how many runner tasks were spawned.
    pub async fn recover_interrupted_jobs(&self) -> Result<usize, AppError> {
        sqlx::query(
            "UPDATE learning_course_jobs SET status = 'interrupted', updated_at = ? \
             WHERE status IN ('queued', 'sampling', 'blueprint', 'lessons', 'importing')",
        )
        .bind(now_ms())
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        let job_ids: Vec<String> = sqlx::query_scalar(
            "SELECT job_id FROM learning_course_jobs WHERE status = 'interrupted' \
             ORDER BY updated_at",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let runner = self.generation_runner()?;
        let mut spawned = 0;
        for job_id in &job_ids {
            if runner.claim_and_spawn(job_id).await? {
                spawned += 1;
            }
        }
        Ok(spawned)
    }

    /// A runner wired to the injected generation dependencies, or a conflict
    /// when the service was built without them.
    fn generation_runner(&self) -> Result<GenerationJobRunner, AppError> {
        let knowledge_service = self.injected_knowledge_service()?;
        let completer = self
            .course_completer
            .read()
            .map_err(|_| AppError::Internal("learning course completer lock poisoned".into()))?
            .clone()
            .ok_or_else(|| {
                AppError::Conflict("knowledge-backed course generation is not configured".into())
            })?;
        Ok(GenerationJobRunner::new(
            self.pool.clone(),
            self.clone(),
            knowledge_service,
            completer,
        ))
    }

    async fn require_course_job(
        &self,
        user_id: &UserId,
        job_id: &str,
    ) -> Result<CourseJobView, AppError> {
        self.course_job(user_id, job_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("course generation job {job_id}")))
    }

    pub async fn import_course(&self, pack: CoursePack) -> Result<CourseDetail, AppError> {
        validate_pack(&pack)?;
        if let Some(kb_id) = &pack.source_kb_id {
            let exists: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_bases WHERE knowledge_base_id = ?")
                    .bind(kb_id.as_str())
                    .fetch_one(&self.pool)
                    .await
                    .map_err(internal)?;
            if exists == 0 {
                return Err(AppError::BadRequest(format!(
                    "knowledge base {kb_id} does not exist"
                )));
            }
        }

        let now = now_ms();
        let course_id = LearningCourseId::new();
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        sqlx::query(
            "INSERT INTO learning_courses \
             (course_id, title, description, domain, source_kb_id, version, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(course_id.as_str())
        .bind(pack.title.trim())
        .bind(pack.description.trim())
        .bind(pack.domain.trim())
        .bind(pack.source_kb_id.as_ref().map(KnowledgeBaseId::as_str))
        .bind(pack.version)
        .bind(now)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(internal)?;

        let mut concepts = HashMap::new();
        for concept in &pack.concepts {
            let concept_id = LearningConceptId::new();
            sqlx::query(
                "INSERT INTO learning_concepts \
                 (concept_id, course_id, concept_key, title, description) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(concept_id.as_str())
            .bind(course_id.as_str())
            .bind(concept.key.trim())
            .bind(concept.title.trim())
            .bind(concept.description.trim())
            .execute(&mut *transaction)
            .await
            .map_err(internal)?;
            concepts.insert(concept.key.clone(), concept_id);
        }

        for concept in &pack.concepts {
            let concept_id = &concepts[&concept.key];
            for prerequisite in &concept.prerequisites {
                sqlx::query(
                    "INSERT INTO learning_concept_prerequisites \
                     (concept_id, prerequisite_concept_id) VALUES (?, ?)",
                )
                .bind(concept_id.as_str())
                .bind(concepts[prerequisite].as_str())
                .execute(&mut *transaction)
                .await
                .map_err(internal)?;
            }
        }

        for (module_position, module) in pack.modules.iter().enumerate() {
            let module_id = LearningModuleId::new();
            sqlx::query(
                "INSERT INTO learning_modules \
                 (module_id, course_id, title, description, position) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(module_id.as_str())
            .bind(course_id.as_str())
            .bind(module.title.trim())
            .bind(module.description.trim())
            .bind(module_position as i64)
            .execute(&mut *transaction)
            .await
            .map_err(internal)?;

            for (lesson_position, lesson) in module.lessons.iter().enumerate() {
                let lesson_id = LearningLessonId::new();
                let (source_path, source_start, source_end) = lesson
                    .source
                    .as_ref()
                    .map(|source| {
                        (
                            Some(source.path.trim()),
                            source.start,
                            source.end,
                        )
                    })
                    .unwrap_or((None, None, None));
                sqlx::query(
                    "INSERT INTO learning_lessons \
                     (lesson_id, module_id, title, summary, position, estimated_minutes, \
                      source_path, source_start, source_end) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(lesson_id.as_str())
                .bind(module_id.as_str())
                .bind(lesson.title.trim())
                .bind(lesson.summary.trim())
                .bind(lesson_position as i64)
                .bind(lesson.estimated_minutes)
                .bind(source_path)
                .bind(source_start)
                .bind(source_end)
                .execute(&mut *transaction)
                .await
                .map_err(internal)?;

                for concept_key in &lesson.concepts {
                    sqlx::query(
                        "INSERT INTO learning_lesson_concepts (lesson_id, concept_id) VALUES (?, ?)",
                    )
                    .bind(lesson_id.as_str())
                    .bind(concepts[concept_key].as_str())
                    .execute(&mut *transaction)
                    .await
                    .map_err(internal)?;
                }

                for (activity_position, activity) in lesson.activities.iter().enumerate() {
                    let activity_id = LearningActivityId::new();
                    let config = StoredActivityConfig {
                        options: activity.options.clone(),
                        answer: activity.answer.clone(),
                        explanation: activity.explanation.clone(),
                        distractors: activity.distractors.clone(),
                    };
                    sqlx::query(
                        "INSERT INTO learning_activities \
                         (activity_id, lesson_id, kind, prompt, config_json, position) \
                         VALUES (?, ?, ?, ?, ?, ?)",
                    )
                    .bind(activity_id.as_str())
                    .bind(lesson_id.as_str())
                    .bind(activity.kind.as_str())
                    .bind(activity.prompt.trim())
                    .bind(serde_json::to_string(&config).map_err(internal)?)
                    .bind(activity_position as i64)
                    .execute(&mut *transaction)
                    .await
                    .map_err(internal)?;

                    let activity_concepts = if activity.concepts.is_empty() {
                        &lesson.concepts
                    } else {
                        &activity.concepts
                    };
                    for concept_key in activity_concepts {
                        sqlx::query(
                            "INSERT INTO learning_activity_concepts \
                             (activity_id, concept_id) VALUES (?, ?)",
                        )
                        .bind(activity_id.as_str())
                        .bind(concepts[concept_key].as_str())
                        .execute(&mut *transaction)
                        .await
                        .map_err(internal)?;
                    }
                }
            }
        }

        transaction.commit().await.map_err(internal)?;
        self.course_detail(&course_id, None).await
    }

    pub async fn list_courses(&self, user_id: &UserId) -> Result<Vec<CourseSummary>, AppError> {
        let rows = sqlx::query(
            "SELECT c.course_id, c.title, c.description, c.domain, c.source_kb_id, c.version, c.updated_at, \
                    EXISTS(SELECT 1 FROM learning_enrollments e \
                           WHERE e.course_id = c.course_id AND e.user_id = ?) AS enrolled, \
                    (SELECT COUNT(*) FROM learning_lessons l \
                     JOIN learning_modules m ON m.module_id = l.module_id \
                     WHERE m.course_id = c.course_id) AS total_lessons, \
                    (SELECT COUNT(*) FROM learning_lesson_progress p \
                     JOIN learning_enrollments e ON e.enrollment_id = p.enrollment_id \
                     JOIN learning_lessons l ON l.lesson_id = p.lesson_id \
                     JOIN learning_modules m ON m.module_id = l.module_id \
                     WHERE e.user_id = ? AND m.course_id = c.course_id AND p.status = 'completed') AS completed_lessons \
             FROM learning_courses c ORDER BY c.updated_at DESC, c.course_id",
        )
        .bind(user_id.as_str())
        .bind(user_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut courses = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut course = course_summary_from_row(row)?;
            course.tags = self.course_tags(&course.id).await?;
            courses.push(course);
        }
        Ok(courses)
    }

    pub async fn course_detail(
        &self,
        course_id: &LearningCourseId,
        user_id: Option<&UserId>,
    ) -> Result<CourseDetail, AppError> {
        let user_value = user_id.map(UserId::as_str);
        let row = sqlx::query(
            "SELECT c.course_id, c.title, c.description, c.domain, c.source_kb_id, c.version, c.updated_at, \
                    EXISTS(SELECT 1 FROM learning_enrollments e \
                           WHERE e.course_id = c.course_id AND e.user_id = ?) AS enrolled, \
                    (SELECT COUNT(*) FROM learning_lessons l \
                     JOIN learning_modules m ON m.module_id = l.module_id \
                     WHERE m.course_id = c.course_id) AS total_lessons, \
                    (SELECT COUNT(*) FROM learning_lesson_progress p \
                     JOIN learning_enrollments e ON e.enrollment_id = p.enrollment_id \
                     JOIN learning_lessons l ON l.lesson_id = p.lesson_id \
                     JOIN learning_modules m ON m.module_id = l.module_id \
                     WHERE e.user_id = ? AND m.course_id = c.course_id AND p.status = 'completed') AS completed_lessons \
             FROM learning_courses c WHERE c.course_id = ?",
        )
        .bind(user_value)
        .bind(user_value)
        .bind(course_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("learning course {course_id}")))?;
        let course = course_summary_from_row(&row)?;
        let course = CourseSummary {
            tags: self.course_tags(&course.id).await?,
            ..course
        };

        // Opening a course detail is an implicit join: enrollment is created
        // on first view so every downstream practice flow has a grouping key.
        let enrollment_id = if let Some(user_id) = user_id {
            Some(self.ensure_enrollment(course_id, user_id).await?)
        } else {
            None
        };
        let enrollment_value = enrollment_id
            .as_ref()
            .map(LearningEnrollmentId::as_str);

        let module_rows = sqlx::query(
            "SELECT module_id, title, description, position FROM learning_modules \
             WHERE course_id = ? ORDER BY position, module_id",
        )
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut modules = Vec::with_capacity(module_rows.len());
        for module_row in module_rows {
            let module_id: LearningModuleId =
                parse_id(module_row.try_get("module_id").map_err(internal)?)?;
            let lesson_rows = sqlx::query(
                "SELECT l.lesson_id, l.title, l.summary, l.position, l.estimated_minutes, \
                        l.source_path, l.source_start, l.source_end, \
                        COALESCE(p.status, 'not_started') AS status \
                 FROM learning_lessons l \
                 LEFT JOIN learning_lesson_progress p \
                   ON p.lesson_id = l.lesson_id AND p.enrollment_id = ? \
                 WHERE l.module_id = ? ORDER BY l.position, l.lesson_id",
            )
            .bind(enrollment_value)
            .bind(module_id.as_str())
            .fetch_all(&self.pool)
            .await
            .map_err(internal)?;
            let mut lessons = Vec::with_capacity(lesson_rows.len());
            for lesson_row in lesson_rows {
                let lesson_id: LearningLessonId =
                    parse_id(lesson_row.try_get("lesson_id").map_err(internal)?)?;
                let status_text: String = lesson_row.try_get("status").map_err(internal)?;
                let status = LessonStatus::try_from(status_text.as_str())
                    .map_err(AppError::Internal)?;
                let source_path: Option<String> =
                    lesson_row.try_get("source_path").map_err(internal)?;
                let source = source_path.map(|path| SourceSpan {
                    path,
                    start: lesson_row.try_get("source_start").ok().flatten(),
                    end: lesson_row.try_get("source_end").ok().flatten(),
                });
                lessons.push(LessonView {
                    id: lesson_id.clone(),
                    title: lesson_row.try_get("title").map_err(internal)?,
                    summary: lesson_row.try_get("summary").map_err(internal)?,
                    position: lesson_row.try_get("position").map_err(internal)?,
                    estimated_minutes: lesson_row
                        .try_get("estimated_minutes")
                        .map_err(internal)?,
                    source,
                    status,
                    concepts: self.lesson_concepts(&lesson_id).await?,
                    activities: self.lesson_activities(&lesson_id).await?,
                });
            }
            modules.push(ModuleView {
                id: module_id,
                title: module_row.try_get("title").map_err(internal)?,
                description: module_row.try_get("description").map_err(internal)?,
                position: module_row.try_get("position").map_err(internal)?,
                lessons,
            });
        }

        let concepts = self
            .course_concepts(course_id, enrollment_id.as_ref())
            .await?;
        let next_lesson_id = recommend_next_lesson(&modules, &concepts);
        let due_review_count = if let Some(enrollment_id) = &enrollment_id {
            // Same admission rule as `due_reviews`: only count items whose
            // concept still has a studied objective question, so the badge
            // matches what the queue will actually show.
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM learning_review_items r \
                 WHERE r.enrollment_id = ? AND r.due_at <= ? \
                 AND EXISTS ( \
                     SELECT 1 FROM learning_activity_concepts ac \
                     JOIN learning_activities a ON a.activity_id = ac.activity_id \
                     WHERE ac.concept_id = r.concept_id \
                     AND a.kind IN ('single_choice', 'true_false') \
                     AND EXISTS ( \
                         SELECT 1 FROM learning_lesson_progress p \
                         WHERE p.lesson_id = a.lesson_id \
                         AND p.enrollment_id = r.enrollment_id \
                         AND p.status = 'completed' \
                     ) \
                     AND EXISTS ( \
                         SELECT 1 FROM learning_attempts t \
                         WHERE t.activity_id = a.activity_id \
                         AND t.enrollment_id = r.enrollment_id \
                     ) \
                 )",
            )
            .bind(enrollment_id.as_str())
            .bind(now_ms())
            .fetch_one(&self.pool)
            .await
            .map_err(internal)?
        } else {
            0
        };

        Ok(CourseDetail {
            course,
            enrollment_id,
            modules,
            concepts,
            next_lesson_id,
            due_review_count,
        })
    }

    pub async fn diagnostic_plan(
        &self,
        course_id: &LearningCourseId,
        user_id: &UserId,
        limit: i64,
    ) -> Result<DiagnosticPlan, AppError> {
        let detail = self.course_detail(course_id, Some(user_id)).await?;
        // course_detail already creates the enrollment on first view, so no
        // explicit join is required before starting a diagnostic.
        let mut covered_concepts = HashSet::new();
        let mut items = Vec::new();
        let limit = limit.clamp(1, 20) as usize;
        let total_concepts = detail.concepts.len() as i64;
        'modules: for module in detail.modules {
            for lesson in module.lessons {
                for activity in lesson.activities {
                    if activity.kind == ActivityKind::Reflection
                        || !activity
                            .concepts
                            .iter()
                            .any(|concept| !covered_concepts.contains(concept.as_str()))
                    {
                        continue;
                    }
                    for concept in &activity.concepts {
                        covered_concepts.insert(concept.as_str().to_owned());
                    }
                    items.push(DiagnosticItem {
                        lesson_id: lesson.id.clone(),
                        lesson_title: lesson.title.clone(),
                        activity,
                    });
                    if items.len() >= limit {
                        break 'modules;
                    }
                }
            }
        }
        Ok(DiagnosticPlan {
            course_id: course_id.clone(),
            total_concepts,
            items,
        })
    }

    /// Returns the user's enrollment for a course, creating it on first use.
    /// Practice flows (diagnostics, attempts, lesson progress) call this
    /// instead of requiring an explicit join step, so enrollment is a data
    /// grouping key rather than a permission gate. Idempotent: re-calling
    /// after an enrollment exists only bumps `updated_at`.
    async fn ensure_enrollment(
        &self,
        course_id: &LearningCourseId,
        user_id: &UserId,
    ) -> Result<LearningEnrollmentId, AppError> {
        let course_exists: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM learning_courses WHERE course_id = ?")
                .bind(course_id.as_str())
                .fetch_one(&self.pool)
                .await
                .map_err(internal)?;
        if course_exists == 0 {
            return Err(AppError::NotFound(format!("learning course {course_id}")));
        }
        let enrollment_id = LearningEnrollmentId::new();
        let now = now_ms();
        sqlx::query(
            "INSERT INTO learning_enrollments \
             (enrollment_id, user_id, course_id, enrolled_at, updated_at) VALUES (?, ?, ?, ?, ?) \
             ON CONFLICT(user_id, course_id) DO UPDATE SET updated_at = excluded.updated_at",
        )
        .bind(enrollment_id.as_str())
        .bind(user_id.as_str())
        .bind(course_id.as_str())
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        let stored: String = sqlx::query_scalar(
            "SELECT enrollment_id FROM learning_enrollments WHERE user_id = ? AND course_id = ?",
        )
        .bind(user_id.as_str())
        .bind(course_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        parse_id(stored)
    }

    /// Explicit join endpoint, kept for compatibility; the same idempotent
    /// upsert is now triggered implicitly by any practice flow.
    pub async fn enroll(
        &self,
        course_id: &LearningCourseId,
        user_id: &UserId,
    ) -> Result<LearningEnrollmentId, AppError> {
        self.ensure_enrollment(course_id, user_id).await
    }

    pub async fn update_lesson_progress(
        &self,
        lesson_id: &LearningLessonId,
        user_id: &UserId,
        status: LessonStatus,
    ) -> Result<(), AppError> {
        let enrollment_id: Option<String> = sqlx::query_scalar(
            "SELECT e.enrollment_id FROM learning_enrollments e \
             JOIN learning_modules m ON m.course_id = e.course_id \
             JOIN learning_lessons l ON l.module_id = m.module_id \
             WHERE e.user_id = ? AND l.lesson_id = ?",
        )
        .bind(user_id.as_str())
        .bind(lesson_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        let enrollment_id = match enrollment_id {
            Some(enrollment_id) => enrollment_id,
            // First progress write joins implicitly: progress rows are
            // grouped under the enrollment, so create it on demand.
            None => {
                let course_id: String = sqlx::query_scalar(
                    "SELECT m.course_id FROM learning_modules m \
                     JOIN learning_lessons l ON l.module_id = m.module_id \
                     WHERE l.lesson_id = ?",
                )
                .bind(lesson_id.as_str())
                .fetch_one(&self.pool)
                .await
                .map_err(internal)?;
                let enrollment =
                    self.ensure_enrollment(&parse_id(course_id)?, user_id).await?;
                enrollment.as_str().to_owned()
            }
        };
        let existing_started: Option<i64> = sqlx::query_scalar(
            "SELECT started_at FROM learning_lesson_progress \
             WHERE enrollment_id = ? AND lesson_id = ?",
        )
        .bind(&enrollment_id)
        .bind(lesson_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .flatten();
        let now = now_ms();
        let (started_at, completed_at) = match status {
            LessonStatus::NotStarted => (None, None),
            LessonStatus::InProgress => (Some(existing_started.unwrap_or(now)), None),
            LessonStatus::Completed => (Some(existing_started.unwrap_or(now)), Some(now)),
        };
        sqlx::query(
            "INSERT INTO learning_lesson_progress \
             (enrollment_id, lesson_id, status, started_at, completed_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?) \
             ON CONFLICT(enrollment_id, lesson_id) DO UPDATE SET \
               status = excluded.status, started_at = excluded.started_at, \
               completed_at = excluded.completed_at, updated_at = excluded.updated_at",
        )
        .bind(&enrollment_id)
        .bind(lesson_id.as_str())
        .bind(status.as_str())
        .bind(started_at)
        .bind(completed_at)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        if status == LessonStatus::Completed {
            // Completing a lesson admits its concepts into the review queue:
            // seed one immediately-due item per concept (idempotent).
            let enrollment = parse_id::<LearningEnrollmentId>(enrollment_id.clone())?;
            let mut transaction = self.pool.begin().await.map_err(internal)?;
            seed_lesson_review_items(&mut transaction, &enrollment, lesson_id, now).await?;
            transaction.commit().await.map_err(internal)?;
        }
        Ok(())
    }

    pub async fn submit_attempt(
        &self,
        activity_id: &LearningActivityId,
        user_id: &UserId,
        response: Value,
        provider_id: Option<ProviderId>,
        model: Option<String>,
    ) -> Result<AttemptResult, AppError> {
        let row = sqlx::query(
            "SELECT a.kind, a.prompt, a.config_json, m.course_id \
             FROM learning_activities a \
             JOIN learning_lessons l ON l.lesson_id = a.lesson_id \
             JOIN learning_modules m ON m.module_id = l.module_id \
             WHERE a.activity_id = ?",
        )
        .bind(activity_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("learning activity {activity_id}")))?;
        let kind_text: String = row.try_get("kind").map_err(internal)?;
        let kind = ActivityKind::try_from(kind_text.as_str()).map_err(AppError::Internal)?;
        let activity_prompt: String = row.try_get("prompt").map_err(internal)?;
        let config: StoredActivityConfig = serde_json::from_str(
            &row.try_get::<String, _>("config_json").map_err(internal)?,
        )
        .map_err(internal)?;
        // First attempt joins implicitly: attempts are grouped under the
        // enrollment, so create it on demand instead of requiring a join step.
        let course_id: LearningCourseId = parse_id(row.try_get("course_id").map_err(internal)?)?;
        let enrollment_id = self.ensure_enrollment(&course_id, user_id).await?;
        // Reflection answers are LLM-graded when a completer is configured;
        // the activity's linked concepts ground the grading prompt. AI
        // grading is an enhancement: any failure (unconfigured completer,
        // call error, unparseable reply) degrades to the rule-based
        // evaluator, and the empty-answer rejection is always enforced by
        // the rule-based evaluator first.
        let (score, feedback) = if kind == ActivityKind::Reflection {
            let (fallback_score, fallback_feedback) = evaluate(kind, &config, &response)?;
            let answer = response.as_str().map(str::trim).unwrap_or_default();
            if answer.is_empty() {
                (fallback_score, fallback_feedback)
            } else {
                let linked_concepts =
                    activity_concept_titles(&self.pool, activity_id).await?;
                match self
                    .grade_reflection(
                        &activity_prompt,
                        answer,
                        &linked_concepts,
                        provider_id.as_ref(),
                        model.as_deref(),
                    )
                    .await
                {
                    Ok(grade) => grade,
                    Err(_) => (fallback_score, fallback_feedback),
                }
            }
        } else {
            evaluate(kind, &config, &response)?
        };
        let passed = score >= 0.6;
        let attempt_id = LearningAttemptId::new();
        let now = now_ms();
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        sqlx::query(
            "INSERT INTO learning_attempts \
             (attempt_id, enrollment_id, activity_id, response_json, score, passed, feedback, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(attempt_id.as_str())
        .bind(enrollment_id.as_str())
        .bind(activity_id.as_str())
        .bind(serde_json::to_string(&response).map_err(internal)?)
        .bind(score)
        .bind(passed)
        .bind(&feedback)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(internal)?;

        let concept_ids: Vec<String> = sqlx::query_scalar(
            "SELECT concept_id FROM learning_activity_concepts WHERE activity_id = ?",
        )
        .bind(activity_id.as_str())
        .fetch_all(&mut *transaction)
        .await
        .map_err(internal)?;
        // In-course attempts only feed mastery evidence. The memory curve
        // (FSRS rescheduling, review/lapse counts) is driven exclusively by
        // the review queue (`answer_review` / `rate_review`), and review
        // items are seeded when the lesson is completed, not here.
        for concept_id in concept_ids {
            update_mastery(&mut transaction, &enrollment_id, &concept_id, score, now).await?;
        }
        transaction.commit().await.map_err(internal)?;

        Ok(AttemptResult {
            id: attempt_id,
            score,
            passed,
            feedback,
        })
    }

    /// LLM-grades a reflection answer against the exercise's concepts. The
    /// model sees the exercise prompt, the learner's answer and the linked
    /// concepts; it must reply with strict JSON
    /// `{ "score": f64, "feedback": string }`. Every failure (no completer,
    /// call error, unparseable reply) returns `Err` so the caller degrades to
    /// rule-based evaluation — AI grading never blocks the practice flow.
    async fn grade_reflection(
        &self,
        prompt: &str,
        answer: &str,
        linked_concepts: &[(String, String, String)],
        provider_id: Option<&ProviderId>,
        model: Option<&str>,
    ) -> Result<(f64, String), AppError> {
        let completer = self
            .course_completer
            .read()
            .map_err(|_| AppError::Internal("learning course completer lock poisoned".into()))?
            .clone()
            .ok_or_else(|| {
                AppError::Conflict("AI reflection grading is not configured".into())
            })?;
        let user = build_reflection_grading_prompt(prompt, answer, linked_concepts);
        let raw = match (provider_id, model) {
            (Some(provider_id), Some(model)) => completer
                .complete_with(REFLECTION_GRADING_SYSTEM, &user, provider_id.as_str(), model)
                .await,
            _ => completer.complete(REFLECTION_GRADING_SYSTEM, &user).await,
        }?;
        parse_reflection_grading(&raw)
    }

    /// Due reviews for the user's review queue. When `course_ids` are given,
    /// the queue is scoped to those courses and admits every queued review
    /// item (not only due ones) so a dedicated course-review session can
    /// serve cards the learner still has pending. Custom questions never
    /// belong to a course and are excluded from course-scoped queues.
    ///
    /// `due_only` narrows a course-scoped queue to items whose due time has
    /// passed; the main review entry always uses it. `orphan` adds
    /// learner-authored questions that belong to no course; with an empty
    /// `course_ids` list it restricts the queue to those questions only.
    /// `tags` keeps only items whose concept (course questions) or question
    /// itself (custom questions) carries at least one of the given tag names.
    pub async fn due_reviews(
        &self,
        user_id: &UserId,
        limit: i64,
        course_ids: &[LearningCourseId],
        due_only: bool,
        orphan: bool,
        tags: &[String],
    ) -> Result<Vec<DueReview>, AppError> {
        let limit = limit.clamp(1, 100);
        let now = now_ms();
        let base = "SELECT r.review_item_id, r.enrollment_id, e.course_id, c.title AS course_title, \
                    r.concept_id, lc.title AS concept_title, r.due_at, \
                    r.stability_days, r.difficulty, r.review_count, r.lapse_count \
             FROM learning_review_items r \
             JOIN learning_enrollments e ON e.enrollment_id = r.enrollment_id \
             LEFT JOIN learning_courses c ON c.course_id = e.course_id \
             JOIN learning_concepts lc ON lc.concept_id = r.concept_id \
             WHERE e.user_id = ?";
        // Course reviews: scoped by one or more courses (all queued, or due
        // only when requested) and/or by tag names attached to the concept's
        // questions. A pure orphan queue skips course reviews entirely.
        let rows = if orphan && course_ids.is_empty() {
            Vec::new()
        } else {
            let mut sql = String::from(base);
            if !course_ids.is_empty() {
                let placeholders = vec!["?"; course_ids.len()].join(", ");
                sql.push_str(&format!(" AND e.course_id IN ({placeholders})"));
            }
            if due_only {
                sql.push_str(" AND r.due_at <= ?");
            }
            if !tags.is_empty() {
                let placeholders = vec!["?"; tags.len()].join(", ");
                sql.push_str(&format!(
                    " AND EXISTS (SELECT 1 FROM learning_activity_concepts ac \
                     JOIN learning_question_tags qt ON qt.question_id = ac.activity_id \
                       AND qt.source = 'course' \
                     JOIN learning_tags lt ON lt.tag_id = qt.tag_id \
                     WHERE ac.concept_id = r.concept_id AND lt.name IN ({placeholders}))"
                ));
            }
            sql.push_str(" ORDER BY r.due_at, r.review_item_id LIMIT ?");
            let mut query = sqlx::query(&sql).bind(user_id.as_str());
            for course_id in course_ids {
                query = query.bind(course_id.as_str());
            }
            if due_only {
                query = query.bind(now);
            }
            for tag in tags {
                query = query.bind(tag);
            }
            query.bind(limit).fetch_all(&self.pool).await.map_err(internal)?
        };
        let mut reviews = Vec::new();
        for row in rows {
            let review_id: LearningReviewItemId =
                parse_id(row.try_get("review_item_id").map_err(internal)?)?;
            let enrollment_id: LearningEnrollmentId =
                parse_id(row.try_get("enrollment_id").map_err(internal)?)?;
            let concept_id: String = row.try_get("concept_id").map_err(internal)?;
            let review_count: i64 = row.try_get("review_count").map_err(internal)?;
            // Reviews are question-based: items whose concept has no studied
            // objective activity are skipped instead of shown without a prompt.
            let questions = self
                .concept_objective_questions(&concept_id, &enrollment_id)
                .await?;
            let Some(question) = pick_review_question(&questions, review_count) else {
                continue;
            };
            let hierarchy = self.activity_hierarchy(&question.lesson_id).await?;
            let course_id: Option<String> = row.try_get("course_id").map_err(internal)?;
            let course_title: Option<String> = row.try_get("course_title").map_err(internal)?;
            let concept_title: String = row.try_get("concept_title").map_err(internal)?;
            reviews.push(DueReview {
                id: review_id,
                source: ReviewSource::Course,
                enrollment_id: Some(enrollment_id),
                course_id: match course_id {
                    Some(value) => Some(parse_id(value)?),
                    None => None,
                },
                course_title,
                module_title: Some(hierarchy.module_title),
                lesson_title: Some(hierarchy.lesson_title),
                concept_id: Some(parse_id(concept_id.clone())?),
                concept_title: Some(concept_title),
                question: ReviewQuestion {
                    activity_id: Some(question.activity_id.clone()),
                    kind: question.kind,
                    prompt: question.prompt.clone(),
                    options: question.config.options.clone(),
                },
                due_at: row.try_get("due_at").map_err(internal)?,
                stability_days: row.try_get("stability_days").map_err(internal)?,
                difficulty: row.try_get("difficulty").map_err(internal)?,
                review_count,
                lapse_count: row.try_get("lapse_count").map_err(internal)?,
            });
        }
        // Learner-authored custom questions carry their own schedule and join
        // the same queue without any course context. They are excluded when
        // specific courses are selected without the orphan flag; the orphan
        // filter alone or together with courses adds exactly those questions.
        if orphan || course_ids.is_empty() {
            let mut sql = String::from(
                "SELECT q.custom_question_id, q.kind, q.prompt, q.config_json, q.due_at, \
                        q.stability_days, q.difficulty, q.review_count, q.lapse_count \
                 FROM learning_custom_questions q \
                 WHERE q.user_id = ? AND q.due_at <= ?",
            );
            if !tags.is_empty() {
                let placeholders = vec!["?"; tags.len()].join(", ");
                sql.push_str(&format!(
                    " AND EXISTS (SELECT 1 FROM learning_question_tags qt \
                     JOIN learning_tags lt ON lt.tag_id = qt.tag_id \
                     WHERE qt.question_id = q.custom_question_id AND qt.source = 'custom' \
                       AND lt.name IN ({placeholders}))"
                ));
            }
            sql.push_str(" ORDER BY q.due_at, q.custom_question_id LIMIT ?");
            let mut query = sqlx::query(&sql).bind(user_id.as_str()).bind(now);
            for tag in tags {
                query = query.bind(tag);
            }
            let custom_rows = query
                .bind(limit)
                .fetch_all(&self.pool)
                .await
                .map_err(internal)?;
            for row in custom_rows {
                let config: StoredActivityConfig = serde_json::from_str(
                    &row.try_get::<String, _>("config_json").map_err(internal)?,
                )
                .map_err(internal)?;
                let kind_text: String = row.try_get("kind").map_err(internal)?;
                reviews.push(DueReview {
                    id: parse_id(row.try_get::<String, _>("custom_question_id").map_err(internal)?)?,
                    source: ReviewSource::Custom,
                    enrollment_id: None,
                    course_id: None,
                    course_title: None,
                    module_title: None,
                    lesson_title: None,
                    concept_id: None,
                    concept_title: None,
                    question: ReviewQuestion {
                        activity_id: None,
                        kind: ActivityKind::try_from(kind_text.as_str())
                            .map_err(|message| AppError::BadRequest(message))?,
                        prompt: row.try_get("prompt").map_err(internal)?,
                        options: config.options,
                    },
                    due_at: row.try_get("due_at").map_err(internal)?,
                    stability_days: row.try_get("stability_days").map_err(internal)?,
                    difficulty: row.try_get("difficulty").map_err(internal)?,
                    review_count: row.try_get("review_count").map_err(internal)?,
                    lapse_count: row.try_get("lapse_count").map_err(internal)?,
                });
            }
        }
        reviews.sort_by_key(|review| (review.due_at, review.id.clone()));
        reviews.truncate(limit as usize);
        Ok(reviews)
    }

    /// Management view over every review item of the user, enriched with
    /// course/concept context and the objective activity used for review.
    /// Items whose course row was deleted stay listed as orphans.
    pub async fn question_entries(
        &self,
        user_id: &UserId,
        course_id: Option<&LearningCourseId>,
        state: Option<&str>,
        search: Option<&str>,
    ) -> Result<Vec<QuestionEntry>, AppError> {
        let now = now_ms();
        let search = search.map(|value| value.trim().to_lowercase());
        let mut entries = Vec::new();

        // Course questions: one row per objective activity / linked concept,
        // enriched with the review item when one exists for this enrollment.
        // Rows without an item are `unlearned`: the lesson was never
        // completed, so nothing entered the review queue yet. A row whose own
        // lesson is not completed yet is also `unlearned` even when the
        // concept already has a review item seeded by another lesson: the
        // review queue only serves questions from completed lessons
        // (see `concept_objective_questions`), so showing a due/scheduled
        // state here would make the counts disagree with the queue.
        let base = "SELECT a.activity_id, a.kind, a.prompt, a.config_json, \
                           ac.concept_id, lc.title AS concept_title, \
                           e.course_id, c.title AS course_title, \
                           p.status AS lesson_status, \
                           ri.review_item_id, ri.due_at, ri.stability_days, ri.difficulty, \
                           ri.review_count, ri.lapse_count, ri.last_reviewed_at, ri.updated_at \
                    FROM learning_activities a \
                    JOIN learning_activity_concepts ac ON ac.activity_id = a.activity_id \
                    LEFT JOIN learning_concepts lc ON lc.concept_id = ac.concept_id \
                    JOIN learning_lessons l ON l.lesson_id = a.lesson_id \
                    JOIN learning_modules m ON m.module_id = l.module_id \
                    JOIN learning_enrollments e ON e.course_id = m.course_id AND e.user_id = ? \
                    LEFT JOIN learning_courses c ON c.course_id = e.course_id \
                    LEFT JOIN learning_lesson_progress p \
                      ON p.lesson_id = a.lesson_id AND p.enrollment_id = e.enrollment_id \
                    LEFT JOIN learning_review_items ri \
                      ON ri.enrollment_id = e.enrollment_id AND ri.concept_id = ac.concept_id \
                    WHERE a.kind IN ('single_choice', 'true_false')";
        let rows = match course_id {
            Some(course_id) => sqlx::query(&format!("{base} AND e.course_id = ? LIMIT 1000"))
                .bind(user_id.as_str())
                .bind(course_id.as_str())
                .fetch_all(&self.pool)
                .await,
            None => sqlx::query(&format!("{base} LIMIT 1000"))
                .bind(user_id.as_str())
                .fetch_all(&self.pool)
                .await,
        }
        .map_err(internal)?;
        let mut course_ids_for_tags = Vec::with_capacity(rows.len());
        for row in rows {
            let review_item_id: Option<String> =
                row.try_get("review_item_id").map_err(internal)?;
            let review_count: i64 = row
                .try_get::<Option<i64>, _>("review_count")
                .map_err(internal)?
                .unwrap_or(0);
            let due_at: Option<i64> = row.try_get("due_at").map_err(internal)?;
            // Aligned with the review queue: only questions whose own lesson is
            // completed can be served, so anything else stays `unlearned`.
            let lesson_completed =
                row.try_get::<Option<String>, _>("lesson_status")
                    .map_err(internal)?
                    .as_deref()
                    == Some("completed");
            let entry_state = if review_item_id.is_none() || !lesson_completed {
                "unlearned"
            } else if review_count == 0 {
                "new"
            } else if due_at.is_some_and(|value| value <= now) {
                "due"
            } else {
                "scheduled"
            };
            if state.is_some_and(|value| value != entry_state) {
                continue;
            }
            let kind_text: String = row.try_get("kind").map_err(internal)?;
            let prompt: String = row.try_get("prompt").map_err(internal)?;
            let concept_title: Option<String> = row.try_get("concept_title").map_err(internal)?;
            if let Some(keyword) = &search {
                let haystack = [concept_title.as_deref().unwrap_or_default(), &prompt]
                    .join(" ")
                    .to_lowercase();
                if !haystack.contains(keyword) {
                    continue;
                }
            }
            let config: StoredActivityConfig = serde_json::from_str(
                &row.try_get::<String, _>("config_json").map_err(internal)?,
            )
            .map_err(internal)?;
            let course_id_raw: Option<String> = row.try_get("course_id").map_err(internal)?;
            let activity_id: String = row.try_get("activity_id").map_err(internal)?;
            course_ids_for_tags.push(activity_id.clone());
            entries.push(QuestionEntry {
                source: ReviewSource::Course,
                question_id: activity_id,
                review_item_id: match review_item_id {
                    Some(value) => Some(parse_id(value)?),
                    None => None,
                },
                state: entry_state.to_string(),
                course_id: match course_id_raw {
                    Some(value) => Some(parse_id(value)?),
                    None => None,
                },
                course_title: row.try_get("course_title").map_err(internal)?,
                concept_id: Some(parse_id(
                    row.try_get::<String, _>("concept_id").map_err(internal)?,
                )?),
                concept_title,
                question_kind: Some(
                    ActivityKind::try_from(kind_text.as_str())
                        .map_err(|message| AppError::BadRequest(message))?,
                ),
                prompt: Some(prompt),
                options: config.options.clone(),
                answer: Some(config.answer.clone()),
                distractors: config.distractors.clone(),
                explanation: Some(config.explanation.clone()),
                due_at,
                overdue: due_at.is_some_and(|value| value <= now),
                stability_days: row
                    .try_get::<Option<f64>, _>("stability_days")
                    .map_err(internal)?
                    .unwrap_or(0.0),
                difficulty: row
                    .try_get::<Option<f64>, _>("difficulty")
                    .map_err(internal)?
                    .unwrap_or(5.0),
                review_count,
                lapse_count: row
                    .try_get::<Option<i64>, _>("lapse_count")
                    .map_err(internal)?
                    .unwrap_or(0),
                last_reviewed_at: row.try_get("last_reviewed_at").map_err(internal)?,
                updated_at: row
                    .try_get::<Option<i64>, _>("updated_at")
                    .map_err(internal)?
                    .unwrap_or(0),
                tags: Vec::new(),
            });
        }
        if !course_ids_for_tags.is_empty() {
            let tag_ids: Vec<&str> =
                course_ids_for_tags.iter().map(String::as_str).collect();
            let tags_by_question =
                self.question_tags_for("course", &tag_ids).await?;
            for entry in &mut entries {
                if let Some(tags) = tags_by_question.get(&entry.question_id) {
                    entry.tags = tags.clone();
                }
            }
        }

        // Learner-authored custom questions; they are never course-scoped.
        if course_id.is_none() {
            let custom_rows = sqlx::query(
                "SELECT q.custom_question_id, q.kind, q.prompt, q.config_json, q.concept_id, \
                        lc.title AS concept_title, q.due_at, q.stability_days, q.difficulty, \
                        q.review_count, q.lapse_count, q.last_reviewed_at, q.updated_at \
                 FROM learning_custom_questions q \
                 LEFT JOIN learning_concepts lc ON lc.concept_id = q.concept_id \
                 WHERE q.user_id = ? LIMIT 500",
            )
            .bind(user_id.as_str())
            .fetch_all(&self.pool)
            .await
            .map_err(internal)?;
            let mut custom_ids_for_tags = Vec::new();
            for row in custom_rows {
                let review_count: i64 = row.try_get("review_count").map_err(internal)?;
                let due_at: i64 = row.try_get("due_at").map_err(internal)?;
                let entry_state = if review_count == 0 {
                    "new"
                } else if due_at <= now {
                    "due"
                } else {
                    "scheduled"
                };
                if state.is_some_and(|value| value != entry_state) {
                    continue;
                }
                let kind_text: String = row.try_get("kind").map_err(internal)?;
                let prompt: String = row.try_get("prompt").map_err(internal)?;
                let concept_title: Option<String> =
                    row.try_get("concept_title").map_err(internal)?;
                if let Some(keyword) = &search {
                    let haystack = [concept_title.as_deref().unwrap_or_default(), &prompt]
                        .join(" ")
                        .to_lowercase();
                    if !haystack.contains(keyword) {
                        continue;
                    }
                }
                let config: StoredActivityConfig = serde_json::from_str(
                    &row.try_get::<String, _>("config_json").map_err(internal)?,
                )
                .map_err(internal)?;
                let concept_id_raw: Option<String> = row.try_get("concept_id").map_err(internal)?;
                let custom_id: String = row
                    .try_get::<String, _>("custom_question_id")
                    .map_err(internal)?;
                custom_ids_for_tags.push(custom_id.clone());
                entries.push(QuestionEntry {
                    source: ReviewSource::Custom,
                    question_id: custom_id,
                    review_item_id: None,
                    state: entry_state.to_string(),
                    course_id: None,
                    course_title: None,
                    concept_id: match concept_id_raw {
                        Some(value) => Some(parse_id(value)?),
                        None => None,
                    },
                    concept_title,
                    question_kind: Some(
                        ActivityKind::try_from(kind_text.as_str())
                            .map_err(|message| AppError::BadRequest(message))?,
                    ),
                    prompt: Some(prompt),
                    options: config.options.clone(),
                    answer: Some(config.answer.clone()),
                    distractors: config.distractors.clone(),
                    explanation: Some(config.explanation.clone()),
                    due_at: Some(due_at),
                    overdue: due_at <= now,
                    stability_days: row.try_get("stability_days").map_err(internal)?,
                    difficulty: row.try_get("difficulty").map_err(internal)?,
                    review_count,
                    lapse_count: row.try_get("lapse_count").map_err(internal)?,
                    last_reviewed_at: row.try_get("last_reviewed_at").map_err(internal)?,
                    updated_at: row.try_get("updated_at").map_err(internal)?,
                    tags: Vec::new(),
                });
            }
            if !custom_ids_for_tags.is_empty() {
                let tag_ids: Vec<&str> =
                    custom_ids_for_tags.iter().map(String::as_str).collect();
                let tags_by_question =
                    self.question_tags_for("custom", &tag_ids).await?;
                for entry in &mut entries {
                    if let Some(tags) = tags_by_question.get(&entry.question_id) {
                        entry.tags = tags.clone();
                    }
                }
            }
        }

        // Queued rows first (nearest deadline at the top); unlearned rows
        // trail the list regardless of any seeded review item.
        entries.sort_by(|left, right| {
            let left_queued = left.state != "unlearned";
            let right_queued = right.state != "unlearned";
            match (left_queued, right_queued) {
                (true, true) => right.due_at.cmp(&left.due_at),
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                (false, false) => Ordering::Equal,
            }
        });
        entries.truncate(500);
        Ok(entries)
    }

    /// Edits the objective activity behind a managed question. Ownership is
    /// checked through the course hierarchy; the answer payload is validated
    /// against the activity kind so `evaluate` keeps working.
    pub async fn update_question(
        &self,
        activity_id: &LearningActivityId,
        user_id: &UserId,
        request: UpdateQuestionRequest,
    ) -> Result<(), AppError> {
        let row = sqlx::query(
            "SELECT a.kind \
             FROM learning_activities a \
             JOIN learning_lessons l ON l.lesson_id = a.lesson_id \
             JOIN learning_modules m ON m.module_id = l.module_id \
             JOIN learning_enrollments e ON e.course_id = m.course_id AND e.user_id = ? \
             WHERE a.activity_id = ?",
        )
        .bind(user_id.as_str())
        .bind(activity_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("editable activity {activity_id}")))?;
        let kind_text: String = row.try_get("kind").map_err(internal)?;
        let kind = ActivityKind::try_from(kind_text.as_str())
            .map_err(|message| AppError::BadRequest(message))?;
        let (prompt, config) = validate_question_payload(
            kind,
            &request.prompt,
            &request.options,
            &request.answer,
            &request.explanation,
            &request.distractors,
        )?;
        sqlx::query("UPDATE learning_activities SET prompt = ?, config_json = ? WHERE activity_id = ?")
            .bind(prompt)
            .bind(serde_json::to_string(&config).map_err(internal)?)
            .bind(activity_id.as_str())
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        Ok(())
    }

    /// Deletes a single review item without touching mastery or attempts.
    pub async fn delete_review_item(
        &self,
        review_id: &LearningReviewItemId,
        user_id: &UserId,
    ) -> Result<(), AppError> {
        let result = sqlx::query(
            "DELETE FROM learning_review_items WHERE review_item_id = ? \
             AND enrollment_id IN (SELECT enrollment_id FROM learning_enrollments WHERE user_id = ?)",
        )
        .bind(review_id.as_str())
        .bind(user_id.as_str())
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("review item {review_id}")));
        }
        Ok(())
    }

    /// Creates a learner-authored question with its own FSRS schedule. It is
    /// due immediately so it joins the review queue right away; the optional
    /// concept only links it back to an existing concept for attribution.
    pub async fn create_custom_question(
        &self,
        user_id: &UserId,
        request: CreateCustomQuestionRequest,
    ) -> Result<String, AppError> {
        if !matches!(
            request.kind,
            ActivityKind::SingleChoice | ActivityKind::TrueFalse | ActivityKind::FillInBlank
        ) {
            return Err(AppError::BadRequest(
                "custom questions only support single choice, true/false and fill in the blank"
                    .into(),
            ));
        }
        let (prompt, config) = validate_question_payload(
            request.kind,
            &request.prompt,
            &request.options,
            &request.answer,
            &request.explanation,
            &request.distractors,
        )?;
        if let Some(concept_id) = &request.concept_id {
            let exists: Option<String> = sqlx::query_scalar(
                "SELECT concept_id FROM learning_concepts WHERE concept_id = ?",
            )
            .bind(concept_id.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(internal)?;
            if exists.is_none() {
                return Err(AppError::NotFound(format!("concept {concept_id}")));
            }
        }
        let question_id = LearningReviewItemId::new().into_string();
        let now = now_ms();
        sqlx::query(
            "INSERT INTO learning_custom_questions \
             (custom_question_id, user_id, kind, prompt, config_json, concept_id, \
              due_at, stability_days, difficulty, review_count, lapse_count, \
              last_reviewed_at, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, 0, 5.0, 0, 0, NULL, ?, ?)",
        )
        .bind(&question_id)
        .bind(user_id.as_str())
        .bind(request.kind.as_str())
        .bind(prompt)
        .bind(serde_json::to_string(&config).map_err(internal)?)
        .bind(request.concept_id.as_ref().map(LearningConceptId::as_str))
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        Ok(question_id)
    }

    /// Edits a learner-authored question; ownership is enforced per user.
    pub async fn update_custom_question(
        &self,
        question_id: &str,
        user_id: &UserId,
        request: UpdateQuestionRequest,
    ) -> Result<(), AppError> {
        let row = sqlx::query(
            "SELECT kind FROM learning_custom_questions \
             WHERE custom_question_id = ? AND user_id = ?",
        )
        .bind(question_id)
        .bind(user_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("custom question {question_id}")))?;
        let kind_text: String = row.try_get("kind").map_err(internal)?;
        let kind = ActivityKind::try_from(kind_text.as_str())
            .map_err(|message| AppError::BadRequest(message))?;
        let (prompt, config) = validate_question_payload(
            kind,
            &request.prompt,
            &request.options,
            &request.answer,
            &request.explanation,
            &request.distractors,
        )?;
        sqlx::query(
            "UPDATE learning_custom_questions SET prompt = ?, config_json = ?, updated_at = ? \
             WHERE custom_question_id = ? AND user_id = ?",
        )
        .bind(prompt)
        .bind(serde_json::to_string(&config).map_err(internal)?)
        .bind(now_ms())
        .bind(question_id)
        .bind(user_id.as_str())
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        Ok(())
    }

    /// Deletes a learner-authored question together with its schedule and
    /// tag links.
    pub async fn delete_custom_question(
        &self,
        question_id: &str,
        user_id: &UserId,
    ) -> Result<(), AppError> {
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        sqlx::query(
            "DELETE FROM learning_question_tags \
             WHERE question_id = ? AND source = 'custom'",
        )
        .bind(question_id)
        .execute(&mut *transaction)
        .await
        .map_err(internal)?;
        let result = sqlx::query(
            "DELETE FROM learning_custom_questions \
             WHERE custom_question_id = ? AND user_id = ?",
        )
        .bind(question_id)
        .bind(user_id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(internal)?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("custom question {question_id}")));
        }
        transaction.commit().await.map_err(internal)?;
        Ok(())
    }

    /// Concepts offered in the custom question form: concepts of enrolled
    /// courses plus orphaned concepts still referenced by review items.
    pub async fn concept_refs(&self, user_id: &UserId) -> Result<Vec<ConceptRef>, AppError> {
        let rows = sqlx::query(
            "SELECT lc.concept_id, lc.title, c.title AS course_title \
             FROM learning_concepts lc \
             LEFT JOIN learning_courses c ON c.course_id = lc.course_id \
             WHERE EXISTS ( \
                 SELECT 1 FROM learning_enrollments e \
                 WHERE e.user_id = ? AND e.course_id = lc.course_id \
             ) OR EXISTS ( \
                 SELECT 1 FROM learning_review_items r \
                 JOIN learning_enrollments e ON e.enrollment_id = r.enrollment_id \
                 WHERE r.concept_id = lc.concept_id AND e.user_id = ? \
             ) \
             ORDER BY lc.title LIMIT 500",
        )
        .bind(user_id.as_str())
        .bind(user_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        rows.into_iter()
            .map(|row| {
                Ok(ConceptRef {
                    concept_id: parse_id(row.try_get("concept_id").map_err(internal)?)?,
                    title: row.try_get("title").map_err(internal)?,
                    course_title: row.try_get("course_title").map_err(internal)?,
                })
            })
            .collect()
    }

    /// Answers a custom question. Correctness is judged server-side; a wrong
    /// or forgotten answer is automatically rated `again`.
    pub async fn answer_custom_review(
        &self,
        question_id: &str,
        user_id: &UserId,
        response: Value,
        forgot: bool,
    ) -> Result<ReviewAnswerResult, AppError> {
        let row = sqlx::query(
            "SELECT kind, config_json FROM learning_custom_questions \
             WHERE custom_question_id = ? AND user_id = ?",
        )
        .bind(question_id)
        .bind(user_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("custom question {question_id}")))?;
        let kind_text: String = row.try_get("kind").map_err(internal)?;
        let kind = ActivityKind::try_from(kind_text.as_str())
            .map_err(|message| AppError::BadRequest(message))?;
        let config: StoredActivityConfig = serde_json::from_str(
            &row.try_get::<String, _>("config_json").map_err(internal)?,
        )
        .map_err(internal)?;
        let (feedback, correct) = if forgot {
            let feedback = if config.explanation.is_empty() {
                "Review the material before retrieving this question again.".to_string()
            } else {
                config.explanation.clone()
            };
            (feedback, false)
        } else {
            let (score, feedback) = evaluate(kind, &config, &response)?;
            (feedback, score >= 0.6)
        };
        let rated = if correct {
            None
        } else {
            Some(
                self.rate_custom_review(question_id, user_id, ReviewRating::Again)
                    .await?,
            )
        };
        Ok(ReviewAnswerResult {
            correct,
            feedback,
            correct_answer: if correct {
                None
            } else {
                Some(config.answer.clone())
            },
            rated,
        })
    }

    /// Applies an FSRS rating to a custom question's own schedule row.
    pub async fn rate_custom_review(
        &self,
        question_id: &str,
        user_id: &UserId,
        rating: ReviewRating,
    ) -> Result<ReviewResult, AppError> {
        let row = sqlx::query(
            "SELECT stability_days, difficulty, review_count, lapse_count, last_reviewed_at \
             FROM learning_custom_questions \
             WHERE custom_question_id = ? AND user_id = ?",
        )
        .bind(question_id)
        .bind(user_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("custom question {question_id}")))?;
        let last_reviewed_at: Option<i64> = row.try_get("last_reviewed_at").map_err(internal)?;
        let now = now_ms();
        let settings = self.scheduler_settings().await;
        let next = schedule_review(
            now,
            row.try_get("stability_days").map_err(internal)?,
            row.try_get("difficulty").map_err(internal)?,
            row.try_get("review_count").map_err(internal)?,
            row.try_get("lapse_count").map_err(internal)?,
            last_reviewed_at,
            rating,
            &settings,
        )?;
        sqlx::query(
            "UPDATE learning_custom_questions SET due_at = ?, stability_days = ?, \
             difficulty = ?, review_count = ?, lapse_count = ?, last_reviewed_at = ?, \
             updated_at = ? WHERE custom_question_id = ? AND user_id = ?",
        )
        .bind(next.due_at)
        .bind(next.stability_days)
        .bind(next.difficulty)
        .bind(next.review_count)
        .bind(next.lapse_count)
        .bind(now)
        .bind(now)
        .bind(question_id)
        .bind(user_id.as_str())
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        Ok(ReviewResult {
            id: question_id.to_string(),
            due_at: next.due_at,
            stability_days: next.stability_days,
            difficulty: next.difficulty,
            review_count: next.review_count,
            lapse_count: next.lapse_count,
        })
    }

    /// Postpones a due custom question by one day without counting it.
    pub async fn skip_custom_review(
        &self,
        question_id: &str,
        user_id: &UserId,
    ) -> Result<ReviewResult, AppError> {
        let row = sqlx::query(
            "SELECT due_at, stability_days, difficulty, review_count, lapse_count \
             FROM learning_custom_questions \
             WHERE custom_question_id = ? AND user_id = ?",
        )
        .bind(question_id)
        .bind(user_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("custom question {question_id}")))?;
        let due_at: i64 = row.try_get("due_at").map_err(internal)?;
        let postponed = now_ms().max(due_at).saturating_add(SKIP_DELAY_MS);
        sqlx::query(
            "UPDATE learning_custom_questions SET due_at = ?, updated_at = ? \
             WHERE custom_question_id = ? AND user_id = ?",
        )
        .bind(postponed)
        .bind(now_ms())
        .bind(question_id)
        .bind(user_id.as_str())
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        Ok(ReviewResult {
            id: question_id.to_string(),
            due_at: postponed,
            stability_days: row.try_get("stability_days").map_err(internal)?,
            difficulty: row.try_get("difficulty").map_err(internal)?,
            review_count: row.try_get("review_count").map_err(internal)?,
            lapse_count: row.try_get("lapse_count").map_err(internal)?,
        })
    }

    /// Deletes a course. With `delete_reviews` the learner's enrollment and
    /// all derived data plus the course content are wiped. Otherwise only
    /// the catalog row disappears, so orphaned concepts stay reviewable.
    pub async fn delete_course(
        &self,
        course_id: &LearningCourseId,
        user_id: &UserId,
        delete_reviews: bool,
    ) -> Result<(), AppError> {
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        let exists: Option<String> =
            sqlx::query_scalar("SELECT course_id FROM learning_courses WHERE course_id = ?")
                .bind(course_id.as_str())
                .fetch_optional(&mut *transaction)
                .await
                .map_err(internal)?;
        if exists.is_none() {
            return Err(AppError::NotFound(format!("course {course_id}")));
        }
        let enrollment_ids: Vec<String> = sqlx::query_scalar(
            "SELECT enrollment_id FROM learning_enrollments WHERE course_id = ? AND user_id = ?",
        )
        .bind(course_id.as_str())
        .bind(user_id.as_str())
        .fetch_all(&mut *transaction)
        .await
        .map_err(internal)?;
        if delete_reviews {
            for enrollment_id in &enrollment_ids {
                for table in [
                    "learning_review_items",
                    "learning_mastery_states",
                    "learning_lesson_progress",
                    "learning_attempts",
                ] {
                    sqlx::query(&format!("DELETE FROM {table} WHERE enrollment_id = ?"))
                        .bind(enrollment_id)
                        .execute(&mut *transaction)
                        .await
                        .map_err(internal)?;
                }
            }
            sqlx::query("DELETE FROM learning_enrollments WHERE course_id = ? AND user_id = ?")
                .bind(course_id.as_str())
                .bind(user_id.as_str())
                .execute(&mut *transaction)
                .await
                .map_err(internal)?;
            let content_sql = [
                "DELETE FROM learning_activity_concepts WHERE activity_id IN (\
                    SELECT a.activity_id FROM learning_activities a \
                    JOIN learning_lessons l ON l.lesson_id = a.lesson_id \
                    JOIN learning_modules m ON m.module_id = l.module_id \
                    WHERE m.course_id = ?)",
                "DELETE FROM learning_activities WHERE lesson_id IN (\
                    SELECT l.lesson_id FROM learning_lessons l \
                    JOIN learning_modules m ON m.module_id = l.module_id \
                    WHERE m.course_id = ?)",
                "DELETE FROM learning_question_tags WHERE source = 'course' AND question_id IN (\
                    SELECT a.activity_id FROM learning_activities a \
                    JOIN learning_lessons l ON l.lesson_id = a.lesson_id \
                    JOIN learning_modules m ON m.module_id = l.module_id \
                    WHERE m.course_id = ?)",
                "DELETE FROM learning_lesson_concepts WHERE lesson_id IN (\
                    SELECT l.lesson_id FROM learning_lessons l \
                    JOIN learning_modules m ON m.module_id = l.module_id \
                    WHERE m.course_id = ?)",
                "DELETE FROM learning_lessons WHERE module_id IN (\
                    SELECT module_id FROM learning_modules WHERE course_id = ?)",
                "DELETE FROM learning_concept_prerequisites WHERE concept_id IN (\
                    SELECT concept_id FROM learning_concepts WHERE course_id = ?)",
                "DELETE FROM learning_concepts WHERE course_id = ?",
                "DELETE FROM learning_modules WHERE course_id = ?",
            ];
            for sql in content_sql {
                sqlx::query(sql)
                    .bind(course_id.as_str())
                    .execute(&mut *transaction)
                    .await
                    .map_err(internal)?;
            }
        }
        sqlx::query("DELETE FROM learning_course_tags WHERE course_id = ?")
            .bind(course_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(internal)?;
        sqlx::query("DELETE FROM learning_courses WHERE course_id = ?")
            .bind(course_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(internal)?;
        transaction.commit().await.map_err(internal)?;
        Ok(())
    }

    /// Every tag of the global pool, ordered by name. The pool is shared by
    /// courses and questions so reusing an existing name links to the same
    /// tag.
    pub async fn list_tags(&self) -> Result<Vec<String>, AppError> {
        let rows = sqlx::query_scalar::<_, String>("SELECT name FROM learning_tags ORDER BY name")
            .fetch_all(&self.pool)
            .await
            .map_err(internal)?;
        Ok(rows)
    }

    /// Replaces the tag set of a course. With `apply_to_children` every
    /// question of the course additionally receives the same tags as a union
    /// with its existing tags.
    pub async fn set_course_tags(
        &self,
        course_id: &LearningCourseId,
        request: SetTagsRequest,
    ) -> Result<Vec<String>, AppError> {
        let tags = normalize_tags(&request.tags)?;
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        let exists: Option<String> =
            sqlx::query_scalar("SELECT course_id FROM learning_courses WHERE course_id = ?")
                .bind(course_id.as_str())
                .fetch_optional(&mut *transaction)
                .await
                .map_err(internal)?;
        if exists.is_none() {
            return Err(AppError::NotFound(format!("course {course_id}")));
        }
        ensure_tags_exist(&mut transaction, &tags).await?;
        sqlx::query("DELETE FROM learning_course_tags WHERE course_id = ?")
            .bind(course_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(internal)?;
        for tag in &tags {
            sqlx::query(
                "INSERT INTO learning_course_tags (course_id, tag_id) \
                 VALUES (?, (SELECT tag_id FROM learning_tags WHERE name = ?))",
            )
            .bind(course_id.as_str())
            .bind(tag)
            .execute(&mut *transaction)
            .await
            .map_err(internal)?;
        }
        if request.apply_to_children && !tags.is_empty() {
            let placeholders: Vec<String> = (0..tags.len()).map(|_| "?".to_string()).collect();
            let sql = format!(
                "INSERT OR IGNORE INTO learning_question_tags (question_id, source, tag_id) \
                 SELECT a.activity_id, 'course', t.tag_id \
                 FROM learning_activities a \
                 JOIN learning_lessons l ON l.lesson_id = a.lesson_id \
                 JOIN learning_modules m ON m.module_id = l.module_id \
                 JOIN learning_tags t \
                 WHERE m.course_id = ? AND t.name IN ({})",
                placeholders.join(", ")
            );
            let mut query = sqlx::query(&sql).bind(course_id.as_str());
            for tag in &tags {
                query = query.bind(tag);
            }
            query.execute(&mut *transaction).await.map_err(internal)?;
        }
        transaction.commit().await.map_err(internal)?;
        Ok(tags)
    }

    /// Replaces the tag set of a managed course question. The activity must
    /// belong to one of the user's enrollments.
    pub async fn set_question_tags(
        &self,
        activity_id: &LearningActivityId,
        user_id: &UserId,
        tags: Vec<String>,
    ) -> Result<Vec<String>, AppError> {
        let tags = normalize_tags(&tags)?;
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        let exists: Option<String> = sqlx::query_scalar(
            "SELECT a.activity_id FROM learning_activities a \
             JOIN learning_lessons l ON l.lesson_id = a.lesson_id \
             JOIN learning_modules m ON m.module_id = l.module_id \
             JOIN learning_enrollments e ON e.course_id = m.course_id AND e.user_id = ? \
             WHERE a.activity_id = ?",
        )
        .bind(user_id.as_str())
        .bind(activity_id.as_str())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(internal)?;
        if exists.is_none() {
            return Err(AppError::NotFound(format!("activity {activity_id}")));
        }
        ensure_tags_exist(&mut transaction, &tags).await?;
        replace_question_tags(&mut transaction, "course", activity_id.as_str(), &tags).await?;
        transaction.commit().await.map_err(internal)?;
        Ok(tags)
    }

    /// Replaces the tag set of a learner-authored question.
    pub async fn set_custom_question_tags(
        &self,
        question_id: &str,
        user_id: &UserId,
        tags: Vec<String>,
    ) -> Result<Vec<String>, AppError> {
        let tags = normalize_tags(&tags)?;
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        let exists: Option<String> = sqlx::query_scalar(
            "SELECT custom_question_id FROM learning_custom_questions \
             WHERE custom_question_id = ? AND user_id = ?",
        )
        .bind(question_id)
        .bind(user_id.as_str())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(internal)?;
        if exists.is_none() {
            return Err(AppError::NotFound(format!("custom question {question_id}")));
        }
        ensure_tags_exist(&mut transaction, &tags).await?;
        replace_question_tags(&mut transaction, "custom", question_id, &tags).await?;
        transaction.commit().await.map_err(internal)?;
        Ok(tags)
    }

    /// Tags attached to a course, ordered by name.
    async fn course_tags(&self, course_id: &LearningCourseId) -> Result<Vec<String>, AppError> {
        let rows = sqlx::query(
            "SELECT t.name FROM learning_tags t \
             JOIN learning_course_tags ct ON ct.tag_id = t.tag_id \
             WHERE ct.course_id = ? ORDER BY t.name",
        )
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        rows.into_iter()
            .map(|row| row.try_get("name").map_err(internal))
            .collect()
    }

    /// Tags attached to questions by id, keyed on `question_id`. `source`
    /// distinguishes course activities from learner-authored questions.
    async fn question_tags_for(
        &self,
        source: &str,
        question_ids: &[&str],
    ) -> Result<HashMap<String, Vec<String>>, AppError> {
        if question_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let placeholders: Vec<String> = (0..question_ids.len()).map(|_| "?".to_string()).collect();
        let sql = format!(
            "SELECT qt.question_id, t.name FROM learning_tags t \
             JOIN learning_question_tags qt ON qt.tag_id = t.tag_id \
             WHERE qt.source = ? AND qt.question_id IN ({}) ORDER BY t.name",
            placeholders.join(", ")
        );
        let mut query = sqlx::query(&sql).bind(source);
        for id in question_ids {
            query = query.bind(id);
        }
        let rows = query.fetch_all(&self.pool).await.map_err(internal)?;
        let mut tags_by_question: HashMap<String, Vec<String>> = HashMap::new();
        for row in rows {
            let question_id: String = row.try_get("question_id").map_err(internal)?;
            let name: String = row.try_get("name").map_err(internal)?;
            tags_by_question.entry(question_id).or_default().push(name);
        }
        Ok(tags_by_question)
    }

    /// Objective activities bound to a concept that the learner has actually
    /// studied: the owning lesson must be completed in this enrollment and the
    /// activity must already have an attempt. Ordered deterministically so
    /// `pick_review_question` rotates through them as reviews accumulate.
    async fn concept_objective_questions(
        &self,
        concept_id: &str,
        enrollment_id: &LearningEnrollmentId,
    ) -> Result<Vec<ObjectiveQuestion>, AppError> {
        let rows = sqlx::query(
            "SELECT a.activity_id, a.lesson_id, a.kind, a.prompt, a.config_json \
             FROM learning_activity_concepts ac \
             JOIN learning_activities a ON a.activity_id = ac.activity_id \
             WHERE ac.concept_id = ? AND a.kind IN ('single_choice', 'true_false', 'fill_in_blank') \
             AND EXISTS ( \
                 SELECT 1 FROM learning_lesson_progress p \
                 WHERE p.lesson_id = a.lesson_id \
                 AND p.enrollment_id = ? AND p.status = 'completed' \
             ) \
             ORDER BY a.position, a.activity_id",
        )
        .bind(concept_id)
        .bind(enrollment_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        rows.into_iter()
            .map(|row| {
                let kind_text: String = row.try_get("kind").map_err(internal)?;
                Ok(ObjectiveQuestion {
                    activity_id: parse_id(row.try_get("activity_id").map_err(internal)?)?,
                    lesson_id: parse_id(row.try_get("lesson_id").map_err(internal)?)?,
                    kind: ActivityKind::try_from(kind_text.as_str()).map_err(AppError::Internal)?,
                    prompt: row.try_get("prompt").map_err(internal)?,
                    config: serde_json::from_str(
                        &row.try_get::<String, _>("config_json").map_err(internal)?,
                    )
                    .map_err(internal)?,
                })
            })
            .collect()
    }

    async fn activity_hierarchy(
        &self,
        lesson_id: &LearningLessonId,
    ) -> Result<ActivityHierarchy, AppError> {
        let row = sqlx::query(
            "SELECT l.title AS lesson_title, m.title AS module_title \
             FROM learning_lessons l \
             JOIN learning_modules m ON m.module_id = l.module_id \
             WHERE l.lesson_id = ?",
        )
        .bind(lesson_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("lesson {lesson_id}")))?;
        Ok(ActivityHierarchy {
            module_title: row.try_get("module_title").map_err(internal)?,
            lesson_title: row.try_get("lesson_title").map_err(internal)?,
        })
    }

    /// Loads user-tunable scheduler knobs from client preferences, falling
    /// back to FSRS defaults for anything missing or malformed.
    async fn scheduler_settings(&self) -> SchedulerSettings {
        let mut settings = SchedulerSettings::default();
        let Ok(rows) = sqlx::query(
            "SELECT key, value FROM client_preferences \
             WHERE key IN ('learning.desiredRetention', 'learning.fsrsParameters')",
        )
        .fetch_all(&self.pool)
        .await
        else {
            return settings;
        };
        for row in rows {
            let (Ok(key), Ok(value)) = (
                row.try_get::<String, _>("key"),
                row.try_get::<String, _>("value"),
            ) else {
                continue;
            };
            let Ok(parsed) = serde_json::from_str::<Value>(&value) else {
                continue;
            };
            match key.as_str() {
                "learning.desiredRetention" => {
                    if let Some(v) = parsed.as_f64() {
                        settings.desired_retention = v.clamp(0.7, 0.99) as f32;
                    }
                }
                "learning.fsrsParameters" => {
                    if let Some(items) = parsed.as_array() {
                        let params: Vec<f32> = items
                            .iter()
                            .filter_map(|item| item.as_f64())
                            .map(|v| v as f32)
                            .collect();
                        if !params.is_empty()
                            && params.len() == items.len()
                            && params.iter().all(|v| v.is_finite())
                        {
                            settings.parameters = params;
                        }
                    }
                }
                _ => {}
            }
        }
        settings
    }

    pub async fn rate_review(
        &self,
        review_id: &LearningReviewItemId,
        user_id: &UserId,
        rating: ReviewRating,
    ) -> Result<ReviewResult, AppError> {
        let row = sqlx::query(
            "SELECT r.enrollment_id, r.concept_id, r.stability_days, r.difficulty, \
                    r.review_count, r.lapse_count, r.last_reviewed_at \
             FROM learning_review_items r \
             JOIN learning_enrollments e ON e.enrollment_id = r.enrollment_id \
             WHERE r.review_item_id = ? AND e.user_id = ?",
        )
        .bind(review_id.as_str())
        .bind(user_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("review item {review_id}")))?;
        let enrollment_id: LearningEnrollmentId =
            parse_id(row.try_get("enrollment_id").map_err(internal)?)?;
        let concept_id: String = row.try_get("concept_id").map_err(internal)?;
        let last_reviewed_at: Option<i64> = row.try_get("last_reviewed_at").map_err(internal)?;
        let now = now_ms();
        let settings = self.scheduler_settings().await;
        let next = schedule_review(
            now,
            row.try_get("stability_days").map_err(internal)?,
            row.try_get("difficulty").map_err(internal)?,
            row.try_get("review_count").map_err(internal)?,
            row.try_get("lapse_count").map_err(internal)?,
            last_reviewed_at,
            rating,
            &settings,
        )?;
        let score = match rating {
            ReviewRating::Again => 0.0,
            ReviewRating::Hard => 0.55,
            ReviewRating::Good => 0.8,
            ReviewRating::Easy => 1.0,
        };
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        sqlx::query(
            "UPDATE learning_review_items SET due_at = ?, stability_days = ?, difficulty = ?, \
             review_count = ?, lapse_count = ?, last_reviewed_at = ?, updated_at = ? \
             WHERE review_item_id = ?",
        )
        .bind(next.due_at)
        .bind(next.stability_days)
        .bind(next.difficulty)
        .bind(next.review_count)
        .bind(next.lapse_count)
        .bind(now)
        .bind(now)
        .bind(review_id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(internal)?;
        update_mastery(&mut transaction, &enrollment_id, &concept_id, score, now).await?;
        transaction.commit().await.map_err(internal)?;
        Ok(ReviewResult {
            id: review_id.to_string(),
            due_at: next.due_at,
            stability_days: next.stability_days,
            difficulty: next.difficulty,
            review_count: next.review_count,
            lapse_count: next.lapse_count,
        })
    }

    /// Postpones a due review without counting it: the memory state stays
    /// untouched and the item simply becomes due again tomorrow.
    pub async fn skip_review(
        &self,
        review_id: &LearningReviewItemId,
        user_id: &UserId,
    ) -> Result<ReviewResult, AppError> {
        let row = sqlx::query(
            "SELECT r.stability_days, r.difficulty, r.review_count, r.lapse_count \
             FROM learning_review_items r \
             JOIN learning_enrollments e ON e.enrollment_id = r.enrollment_id \
             WHERE r.review_item_id = ? AND e.user_id = ?",
        )
        .bind(review_id.as_str())
        .bind(user_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("review item {review_id}")))?;
        let now = now_ms();
        let due_at = now.saturating_add(SKIP_DELAY_MS);
        sqlx::query(
            "UPDATE learning_review_items SET due_at = ?, updated_at = ? \
             WHERE review_item_id = ?",
        )
        .bind(due_at)
        .bind(now)
        .bind(review_id.as_str())
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        Ok(ReviewResult {
            id: review_id.to_string(),
            due_at,
            stability_days: row.try_get("stability_days").map_err(internal)?,
            difficulty: row.try_get("difficulty").map_err(internal)?,
            review_count: row.try_get("review_count").map_err(internal)?,
            lapse_count: row.try_get("lapse_count").map_err(internal)?,
        })
    }

    /// Answers the question attached to a due review. A wrong answer (or an
    /// admitted lapse via `forgot`) is immediately rated `again` (scheduling +
    /// mastery updated); a correct answer only records the attempt and waits
    /// for a self-rating via `rate_review`.
    pub async fn answer_review(
        &self,
        review_id: &LearningReviewItemId,
        user_id: &UserId,
        response: Value,
        forgot: bool,
    ) -> Result<ReviewAnswerResult, AppError> {
        let row = sqlx::query(
            "SELECT r.enrollment_id, r.concept_id, r.review_count \
             FROM learning_review_items r \
             JOIN learning_enrollments e ON e.enrollment_id = r.enrollment_id \
             WHERE r.review_item_id = ? AND e.user_id = ?",
        )
        .bind(review_id.as_str())
        .bind(user_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("review item {review_id}")))?;
        let enrollment_id: LearningEnrollmentId =
            parse_id(row.try_get("enrollment_id").map_err(internal)?)?;
        let concept_id: String = row.try_get("concept_id").map_err(internal)?;
        let review_count: i64 = row.try_get("review_count").map_err(internal)?;
        // Same rotation as `due_reviews` so the answered question matches the
        // one the client displayed.
        let questions = self
            .concept_objective_questions(&concept_id, &enrollment_id)
            .await?;
        let question = pick_review_question(&questions, review_count)
            .ok_or_else(|| AppError::NotFound(format!("objective question for concept {concept_id}")))?;
        // `forgot` skips grading entirely: learners must never be forced to
        // guess, so the lapse is recorded with the revealed answer instead.
        let (score, feedback, correct) = if forgot {
            let feedback = if question.config.explanation.is_empty() {
                "Review the source material before retrieving this concept again.".to_string()
            } else {
                question.config.explanation.clone()
            };
            (0.0, feedback, false)
        } else {
            let (score, feedback) = evaluate(question.kind, &question.config, &response)?;
            (score, feedback, score >= 0.6)
        };
        let attempt_id = LearningAttemptId::new();
        let now = now_ms();
        let settings = self.scheduler_settings().await;
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        sqlx::query(
            "INSERT INTO learning_attempts \
             (attempt_id, enrollment_id, activity_id, response_json, score, passed, feedback, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(attempt_id.as_str())
        .bind(enrollment_id.as_str())
        .bind(question.activity_id.as_str())
        .bind(serde_json::to_string(if forgot { &Value::Null } else { &response }).map_err(internal)?)
        .bind(score)
        .bind(correct)
        .bind(&feedback)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(internal)?;
        let rated = if correct {
            None
        } else {
            update_mastery_and_review(
                &mut transaction,
                &enrollment_id,
                &concept_id,
                score,
                ReviewRating::Again,
                now,
                &settings,
            )
            .await?;
            let updated = sqlx::query(
                "SELECT due_at, stability_days, difficulty, review_count, lapse_count \
                 FROM learning_review_items WHERE review_item_id = ?",
            )
            .bind(review_id.as_str())
            .fetch_one(&mut *transaction)
            .await
            .map_err(internal)?;
            Some(ReviewResult {
                id: review_id.to_string(),
                due_at: updated.try_get("due_at").map_err(internal)?,
                stability_days: updated.try_get("stability_days").map_err(internal)?,
                difficulty: updated.try_get("difficulty").map_err(internal)?,
                review_count: updated.try_get("review_count").map_err(internal)?,
                lapse_count: updated.try_get("lapse_count").map_err(internal)?,
            })
        };
        transaction.commit().await.map_err(internal)?;
        Ok(ReviewAnswerResult {
            correct,
            feedback,
            correct_answer: if correct {
                None
            } else {
                Some(question.config.answer.clone())
            },
            rated,
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

const MASTERY_RECOMMENDATION_THRESHOLD: f64 = 0.8;

/// Trims, drops empties and deduplicates tag names, rejecting names longer
/// than the schema limit (50 chars).
fn normalize_tags(tags: &[String]) -> Result<Vec<String>, AppError> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for raw in tags {
        let name = raw.trim();
        if name.is_empty() || !seen.insert(name.to_owned()) {
            continue;
        }
        if name.chars().count() > 50 {
            return Err(AppError::BadRequest(format!(
                "tag `{name}` exceeds 50 characters"
            )));
        }
        normalized.push(name.to_owned());
    }
    Ok(normalized)
}

/// Inserts missing tag names into the global pool; existing names are
/// ignored so the pool stays unique.
async fn ensure_tags_exist(
    transaction: &mut Transaction<'_, Sqlite>,
    tags: &[String],
) -> Result<(), AppError> {
    for tag in tags {
        sqlx::query(
            "INSERT OR IGNORE INTO learning_tags (tag_id, name, created_at) \
             VALUES (?, ?, ?)",
        )
        .bind(LearningTagId::new().as_str())
        .bind(tag)
        .bind(now_ms())
        .execute(&mut **transaction)
        .await
        .map_err(internal)?;
    }
    Ok(())
}

/// Replaces the full tag set of one question.
async fn replace_question_tags(
    transaction: &mut Transaction<'_, Sqlite>,
    source: &str,
    question_id: &str,
    tags: &[String],
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM learning_question_tags WHERE question_id = ? AND source = ?")
        .bind(question_id)
        .bind(source)
        .execute(&mut **transaction)
        .await
        .map_err(internal)?;
    for tag in tags {
        sqlx::query(
            "INSERT OR IGNORE INTO learning_question_tags (question_id, source, tag_id) \
             VALUES (?, ?, (SELECT tag_id FROM learning_tags WHERE name = ?))",
        )
        .bind(question_id)
        .bind(source)
        .bind(tag)
        .execute(&mut **transaction)
        .await
        .map_err(internal)?;
    }
    Ok(())
}

fn recommend_next_lesson(
    modules: &[ModuleView],
    concepts: &[ConceptView],
) -> Option<LearningLessonId> {
    if let Some(lesson) = modules
        .iter()
        .flat_map(|module| &module.lessons)
        .find(|lesson| lesson.status == LessonStatus::InProgress)
    {
        return Some(lesson.id.clone());
    }
    let mastery: HashMap<&str, f64> = concepts
        .iter()
        .filter_map(|concept| {
            concept
                .mastery
                .map(|value| (concept.id.as_str(), value))
        })
        .collect();
    let concept_by_id: HashMap<&str, &ConceptView> = concepts
        .iter()
        .map(|concept| (concept.id.as_str(), concept))
        .collect();
    let lessons: Vec<&LessonView> = modules
        .iter()
        .flat_map(|module| &module.lessons)
        .collect();
    for lesson in lessons
        .iter()
        .copied()
        .filter(|lesson| lesson.status != LessonStatus::Completed)
    {
        if lesson.concepts.is_empty() {
            return Some(lesson.id.clone());
        }
        let deficient: Vec<&ConceptView> = lesson
            .concepts
            .iter()
            .filter(|concept| {
                mastery.get(concept.as_str()).copied().unwrap_or(0.0)
                    < MASTERY_RECOMMENDATION_THRESHOLD
            })
            .filter_map(|concept| concept_by_id.get(concept.as_str()).copied())
            .collect();
        if deficient.is_empty() {
            continue;
        }
        for prerequisite in deficient
            .iter()
            .flat_map(|concept| &concept.prerequisites)
            .filter(|prerequisite| {
                mastery
                    .get(prerequisite.as_str())
                    .copied()
                    .unwrap_or(0.0)
                    < MASTERY_RECOMMENDATION_THRESHOLD
            })
        {
            if let Some(prerequisite_lesson) = lessons
                .iter()
                .find(|candidate| candidate.concepts.contains(prerequisite))
            {
                return Some(prerequisite_lesson.id.clone());
            }
        }
        return Some(lesson.id.clone());
    }
    None
}

fn course_job_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<CourseJobView, AppError> {
    // The domain rides inside the request snapshot; a stale or unparseable
    // snapshot degrades to `None` rather than failing the whole list.
    let domain = row
        .try_get::<Option<String>, _>("request_json")
        .map_err(internal)?
        .and_then(|json| serde_json::from_str::<GenerateCourseRequest>(&json).ok())
        .and_then(|request| request.domain);
    Ok(CourseJobView {
        job_id: row.try_get("job_id").map_err(internal)?,
        source: CourseJobSource::try_from(
            row.try_get::<String, _>("source").map_err(internal)?.as_str(),
        )
        .map_err(AppError::Internal)?,
        status: CourseJobStatus::try_from(
            row.try_get::<String, _>("status").map_err(internal)?.as_str(),
        )
        .map_err(AppError::Internal)?,
        current_module: row.try_get("current_module").map_err(internal)?,
        current_lesson: row.try_get("current_lesson").map_err(internal)?,
        total_lessons: row.try_get("total_lessons").map_err(internal)?,
        error: row.try_get("error").map_err(internal)?,
        course_id: row.try_get("course_id").map_err(internal)?,
        knowledge_base_name: row.try_get("knowledge_base_name").map_err(internal)?,
        domain,
        created_at: row.try_get("created_at").map_err(internal)?,
        updated_at: row.try_get("updated_at").map_err(internal)?,
    })
}

fn validate_generation_request(request: &GenerateCourseRequest) -> Result<(), AppError> {
    if request.provider_id.is_some() != request.model.is_some() {
        return Err(AppError::BadRequest(
            "provider_id and model must be provided together".into(),
        ));
    }
    if request
        .model
        .as_deref()
        .is_some_and(|model| model.trim().is_empty())
    {
        return Err(AppError::BadRequest("model must not be empty".into()));
    }
    if !(1..=6).contains(&request.module_count) {
        return Err(AppError::BadRequest(
            "module_count must be between 1 and 6".into(),
        ));
    }
    if !(1..=6).contains(&request.lessons_per_module) {
        return Err(AppError::BadRequest(
            "lessons_per_module must be between 1 and 6".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_pack(pack: &CoursePack) -> Result<(), AppError> {
    if pack.title.trim().is_empty() {
        return Err(AppError::BadRequest("course title is required".into()));
    }
    if pack.domain.trim().is_empty() {
        return Err(AppError::BadRequest("course domain is required".into()));
    }
    if pack.version <= 0 {
        return Err(AppError::BadRequest("course version must be positive".into()));
    }
    if pack.modules.is_empty() {
        return Err(AppError::BadRequest("course must contain at least one module".into()));
    }
    let mut concept_keys = HashSet::new();
    for concept in &pack.concepts {
        if concept.key.trim().is_empty() || concept.title.trim().is_empty() {
            return Err(AppError::BadRequest(
                "concept key and title are required".into(),
            ));
        }
        if !concept_keys.insert(concept.key.as_str()) {
            return Err(AppError::BadRequest(format!(
                "duplicate concept key: {}",
                concept.key
            )));
        }
    }
    for concept in &pack.concepts {
        for prerequisite in &concept.prerequisites {
            require_concept(&concept_keys, prerequisite)?;
            if prerequisite == &concept.key {
                return Err(AppError::BadRequest(format!(
                    "concept {} cannot require itself",
                    concept.key
                )));
            }
        }
    }
    validate_prerequisite_graph(pack)?;
    for module in &pack.modules {
        if module.title.trim().is_empty() || module.lessons.is_empty() {
            return Err(AppError::BadRequest(
                "each module needs a title and at least one lesson".into(),
            ));
        }
        for lesson in &module.lessons {
            if lesson.title.trim().is_empty() || lesson.estimated_minutes <= 0 {
                return Err(AppError::BadRequest(
                    "each lesson needs a title and positive estimated_minutes".into(),
                ));
            }
            if let Some(source) = &lesson.source {
                if source.path.trim().is_empty()
                    || source.start.is_some_and(|value| value < 0)
                    || source.end.is_some_and(|value| value < 0)
                    || matches!((source.start, source.end), (Some(start), Some(end)) if end < start)
                {
                    return Err(AppError::BadRequest(format!(
                        "lesson {} has an invalid source span",
                        lesson.title
                    )));
                }
            }
            for concept in &lesson.concepts {
                require_concept(&concept_keys, concept)?;
            }
            for activity in &lesson.activities {
                if activity.prompt.trim().is_empty() {
                    return Err(AppError::BadRequest(
                        "activity prompt is required".into(),
                    ));
                }
                for concept in &activity.concepts {
                    require_concept(&concept_keys, concept)?;
                }
                match activity.kind {
                    ActivityKind::SingleChoice => {
                        let Some(answer) = activity.answer.as_str() else {
                            return Err(AppError::BadRequest(
                                "single_choice answer must be a string".into(),
                            ));
                        };
                        if activity.options.len() < 2
                            || !activity.options.iter().any(|option| option == answer)
                        {
                            return Err(AppError::BadRequest(
                                "single_choice needs at least two options and an answer in options"
                                    .into(),
                            ));
                        }
                    }
                    ActivityKind::TrueFalse => {
                        if !activity.answer.is_boolean() {
                            return Err(AppError::BadRequest(
                                "true_false answer must be boolean".into(),
                            ));
                        }
                    }
                    ActivityKind::Reflection => {}
                    ActivityKind::FillInBlank => {
                        if !activity.prompt.contains("___") {
                            return Err(AppError::BadRequest(
                                "fill_in_blank prompt must contain a ___ blank".into(),
                            ));
                        }
                        let Some(answers) = activity.answer.as_array() else {
                            return Err(AppError::BadRequest(
                                "fill_in_blank answer must be a JSON array of accepted answers"
                                    .into(),
                            ));
                        };
                        if answers.is_empty() || answers.len() > 3 {
                            return Err(AppError::BadRequest(
                                "fill_in_blank must have 1-3 accepted answers".into(),
                            ));
                        }
                        if answers.iter().any(|accepted| {
                            !accepted
                                .as_str()
                                .is_some_and(|text| !text.trim().is_empty())
                        }) {
                            return Err(AppError::BadRequest(
                                "fill_in_blank accepted answers must be non-empty strings".into(),
                            ));
                        }
                        if activity.distractors.is_empty() {
                            return Err(AppError::BadRequest(
                                "fill_in_blank needs at least one distractor".into(),
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Visited,
}

fn validate_prerequisite_graph(pack: &CoursePack) -> Result<(), AppError> {
    let prerequisites: HashMap<&str, Vec<&str>> = pack
        .concepts
        .iter()
        .map(|concept| {
            (
                concept.key.as_str(),
                concept
                    .prerequisites
                    .iter()
                    .map(String::as_str)
                    .collect(),
            )
        })
        .collect();
    let mut states = HashMap::new();
    for concept in prerequisites.keys() {
        visit_concept(concept, &prerequisites, &mut states)?;
    }
    Ok(())
}

fn visit_concept<'a>(
    concept: &'a str,
    prerequisites: &HashMap<&'a str, Vec<&'a str>>,
    states: &mut HashMap<&'a str, VisitState>,
) -> Result<(), AppError> {
    match states.get(concept) {
        Some(VisitState::Visited) => return Ok(()),
        Some(VisitState::Visiting) => {
            return Err(AppError::BadRequest(format!(
                "concept prerequisite cycle contains {concept}"
            )));
        }
        None => {}
    }
    states.insert(concept, VisitState::Visiting);
    for prerequisite in &prerequisites[concept] {
        visit_concept(prerequisite, prerequisites, states)?;
    }
    states.insert(concept, VisitState::Visited);
    Ok(())
}

fn require_concept(concepts: &HashSet<&str>, key: &str) -> Result<(), AppError> {
    if concepts.contains(key) {
        Ok(())
    } else {
        Err(AppError::BadRequest(format!(
            "unknown concept key: {key}"
        )))
    }
}

/// Objective activity used as a review question, including the stored config
/// so answers can be judged server-side.
struct ObjectiveQuestion {
    activity_id: LearningActivityId,
    lesson_id: LearningLessonId,
    kind: ActivityKind,
    prompt: String,
    config: StoredActivityConfig,
}

struct ActivityHierarchy {
    module_title: String,
    lesson_title: String,
}

/// Rotates through a concept's objective questions so repeated reviews do not
/// always ask the same one.
fn pick_review_question(
    questions: &[ObjectiveQuestion],
    review_count: i64,
) -> Option<&ObjectiveQuestion> {
    if questions.is_empty() {
        return None;
    }
    let index = review_count.max(0) as usize % questions.len();
    questions.get(index)
}

/// Shared payload validation for course activities and custom questions so
/// `evaluate` keeps working for both. Returns the trimmed prompt and the
/// persisted config.
fn validate_question_payload(
    kind: ActivityKind,
    prompt: &str,
    options: &[String],
    answer: &Value,
    explanation: &str,
    distractors: &[String],
) -> Result<(String, StoredActivityConfig), AppError> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err(AppError::BadRequest(
            "question prompt must not be empty".into(),
        ));
    }
    let config = match kind {
        ActivityKind::SingleChoice => {
            let options: Vec<String> = options
                .iter()
                .map(|option| option.trim().to_string())
                .filter(|option| !option.is_empty())
                .collect();
            let unique = options.iter().collect::<std::collections::HashSet<_>>();
            if unique.len() != options.len() || options.len() < 2 {
                return Err(AppError::BadRequest(
                    "single choice questions need at least two unique options".into(),
                ));
            }
            let Some(answer_text) = answer.as_str() else {
                return Err(AppError::BadRequest(
                    "single choice answer must be a string".into(),
                ));
            };
            if !options.iter().any(|option| option == answer_text) {
                return Err(AppError::BadRequest(
                    "single choice answer must be one of the options".into(),
                ));
            }
            StoredActivityConfig {
                options,
                answer: answer.clone(),
                explanation: explanation.to_string(),
                distractors: Vec::new(),
            }
        }
        ActivityKind::TrueFalse => {
            let Some(answer_bool) = answer.as_bool() else {
                return Err(AppError::BadRequest(
                    "true/false answer must be a boolean".into(),
                ));
            };
            StoredActivityConfig {
                options: Vec::new(),
                answer: Value::Bool(answer_bool),
                explanation: explanation.to_string(),
                distractors: Vec::new(),
            }
        }
        ActivityKind::Reflection => StoredActivityConfig {
            options: Vec::new(),
            answer: Value::Null,
            explanation: explanation.to_string(),
            distractors: Vec::new(),
        },
        ActivityKind::FillInBlank => {
            if !prompt.contains("___") {
                return Err(AppError::BadRequest(
                    "fill_in_blank prompt must contain a ___ blank".into(),
                ));
            }
            let Some(answers) = answer.as_array() else {
                return Err(AppError::BadRequest(
                    "fill_in_blank answer must be a JSON array of accepted answers".into(),
                ));
            };
            if answers.is_empty() || answers.len() > 3 {
                return Err(AppError::BadRequest(
                    "fill_in_blank must have 1-3 accepted answers".into(),
                ));
            }
            if answers.iter().any(|accepted| {
                !accepted.as_str().is_some_and(|text| !text.trim().is_empty())
            }) {
                return Err(AppError::BadRequest(
                    "fill_in_blank accepted answers must be non-empty strings".into(),
                ));
            }
            let distractors: Vec<String> = distractors
                .iter()
                .map(|distractor| distractor.trim().to_string())
                .filter(|distractor| !distractor.is_empty())
                .collect();
            StoredActivityConfig {
                options: Vec::new(),
                answer: answer.clone(),
                explanation: explanation.to_string(),
                distractors,
            }
        }
    };
    Ok((prompt.to_string(), config))
}

/// System prompt for AI reflection grading: the model judges correctness and
/// completeness against the exercise's concepts, reports coverage of the full
/// course concept list, and replies with strict JSON.
const REFLECTION_GRADING_SYSTEM: &str = r#"You are a strict but encouraging learning coach grading a learner's reflection answer for a course exercise.

Score the answer from 0.0 to 1.0 (0.6 is passing):
- Correctness: does the answer align with the concepts this exercise targets?
- Completeness: does it cover the key points of those concepts?

Reply with ONLY one JSON object matching this shape:
{
  "score": 0.75,
  "feedback": "markdown text"
}
Rules:
- score must be a number between 0.0 and 1.0.
- feedback must be Markdown with two parts: (1) an evaluation of the answer, (2) concrete improvement suggestions.
- Write the feedback in the same language as the learner's answer.
- Output JSON only, without Markdown fences or commentary."#;

/// Builds the user message for AI reflection grading: the exercise prompt,
/// the learner's answer and the concepts the exercise targets (its own
/// lesson's concepts — reflections never bind concepts of other lessons).
fn build_reflection_grading_prompt(
    prompt: &str,
    answer: &str,
    linked_concepts: &[(String, String, String)],
) -> String {
    let linked = linked_concepts
        .iter()
        .map(|(_, title, description)| format!("- {title}: {description}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Exercise prompt:\n{prompt}\n\nLearner's answer:\n{answer}\n\nConcepts this exercise targets:\n{linked}"
    )
}

/// Parses the strict-JSON grading reply into `(score, feedback)`. Any shape
/// deviation returns `Err` so the caller degrades to rule-based grading.
fn parse_reflection_grading(raw: &str) -> Result<(f64, String), AppError> {
    #[derive(serde::Deserialize)]
    struct GradingReply {
        score: f64,
        feedback: String,
    }
    let reply: GradingReply = serde_json::from_str(raw).map_err(|error| {
        AppError::Internal(format!("unparseable reflection grading reply: {error}"))
    })?;
    Ok((reply.score.clamp(0.0, 1.0), reply.feedback))
}

/// Concept rows (id, title, description) bound to an activity, used both for
/// mastery evidence and to ground AI reflection grading. Reflections are
/// generated within one lesson, so this is the concept scope the grading
/// prompt carries.
async fn activity_concept_titles(
    pool: &SqlitePool,
    activity_id: &LearningActivityId,
) -> Result<Vec<(String, String, String)>, AppError> {
    sqlx::query_as(
        "SELECT c.concept_id, c.title, c.description \
         FROM learning_activity_concepts ac \
         JOIN learning_concepts c ON c.concept_id = ac.concept_id \
         WHERE ac.activity_id = ?",
    )
    .bind(activity_id.as_str())
    .fetch_all(pool)
    .await
    .map_err(internal)
}

fn evaluate(
    kind: ActivityKind,
    config: &StoredActivityConfig,
    response: &Value,
) -> Result<(f64, String), AppError> {
    let correct = match kind {
        ActivityKind::SingleChoice => response.as_str() == config.answer.as_str(),
        ActivityKind::TrueFalse => response.as_bool() == config.answer.as_bool(),
        ActivityKind::Reflection => response
            .as_str()
            .is_some_and(|value| !value.trim().is_empty()),
        ActivityKind::FillInBlank => {
            let Some(answer) = response.as_str().map(str::trim) else {
                return Err(AppError::BadRequest(
                    "fill_in_blank response must be a string".into(),
                ));
            };
            if answer.is_empty() {
                return Err(AppError::BadRequest(
                    "fill_in_blank response must not be empty".into(),
                ));
            }
            config.answer.as_array().is_some_and(|accepted| {
                accepted.iter().any(|candidate| {
                    candidate
                        .as_str()
                        .is_some_and(|text| text.trim().eq_ignore_ascii_case(answer))
                })
            })
        }
    };
    if kind == ActivityKind::Reflection && !correct {
        return Err(AppError::BadRequest(
            "reflection response must not be empty".into(),
        ));
    }
    let score = if correct { 1.0 } else { 0.0 };
    let feedback = if correct {
        config.explanation.clone()
    } else if config.explanation.is_empty() {
        "Try again and retrieve the governing concept before answering.".into()
    } else {
        config.explanation.clone()
    };
    Ok((score, feedback))
}

async fn update_mastery_and_review(
    transaction: &mut Transaction<'_, Sqlite>,
    enrollment_id: &LearningEnrollmentId,
    concept_id: &str,
    score: f64,
    rating: ReviewRating,
    now: i64,
    settings: &SchedulerSettings,
) -> Result<(), AppError> {
    update_mastery(transaction, enrollment_id, concept_id, score, now).await?;
    let current = sqlx::query(
        "SELECT review_item_id, stability_days, difficulty, review_count, lapse_count, last_reviewed_at \
         FROM learning_review_items WHERE enrollment_id = ? AND concept_id = ?",
    )
    .bind(enrollment_id.as_str())
    .bind(concept_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(internal)?;
    let (review_id, stability, difficulty, count, lapses, last_reviewed_at) =
        if let Some(row) = current {
            (
                row.try_get::<String, _>("review_item_id")
                    .map_err(internal)?,
                row.try_get("stability_days").map_err(internal)?,
                row.try_get("difficulty").map_err(internal)?,
                row.try_get("review_count").map_err(internal)?,
                row.try_get("lapse_count").map_err(internal)?,
                row.try_get("last_reviewed_at").map_err(internal)?,
            )
        } else {
            (
                LearningReviewItemId::new().into_string(),
                0.0,
                5.0,
                0,
                0,
                None,
            )
        };
    let next = schedule_review(
        now,
        stability,
        difficulty,
        count,
        lapses,
        last_reviewed_at,
        rating,
        settings,
    )?;
    sqlx::query(
        "INSERT INTO learning_review_items \
         (review_item_id, enrollment_id, concept_id, due_at, stability_days, difficulty, review_count, \
          lapse_count, last_reviewed_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(enrollment_id, concept_id) DO UPDATE SET \
           due_at = excluded.due_at, stability_days = excluded.stability_days, \
           difficulty = excluded.difficulty, review_count = excluded.review_count, \
           lapse_count = excluded.lapse_count, last_reviewed_at = excluded.last_reviewed_at, \
           updated_at = excluded.updated_at",
    )
    .bind(review_id)
    .bind(enrollment_id.as_str())
    .bind(concept_id)
    .bind(next.due_at)
    .bind(next.stability_days)
    .bind(next.difficulty)
    .bind(next.review_count)
    .bind(next.lapse_count)
    .bind(now)
    .bind(now)
    .execute(&mut **transaction)
    .await
    .map_err(internal)?;
    Ok(())
}

async fn update_mastery(
    transaction: &mut Transaction<'_, Sqlite>,
    enrollment_id: &LearningEnrollmentId,
    concept_id: &str,
    score: f64,
    now: i64,
) -> Result<(), AppError> {
    let current: Option<(f64, i64)> = sqlx::query_as(
        "SELECT mastery, evidence_count FROM learning_mastery_states \
         WHERE enrollment_id = ? AND concept_id = ?",
    )
    .bind(enrollment_id.as_str())
    .bind(concept_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(internal)?;
    let (mastery, evidence_count) = current.unwrap_or((score, 0));
    let next_mastery = if evidence_count == 0 {
        score
    } else {
        mastery * 0.7 + score * 0.3
    };
    sqlx::query(
        "INSERT INTO learning_mastery_states \
         (enrollment_id, concept_id, mastery, evidence_count, last_practiced_at, updated_at) \
         VALUES (?, ?, ?, 1, ?, ?) \
         ON CONFLICT(enrollment_id, concept_id) DO UPDATE SET \
           mastery = excluded.mastery, \
           evidence_count = learning_mastery_states.evidence_count + 1, \
           last_practiced_at = excluded.last_practiced_at, updated_at = excluded.updated_at",
    )
    .bind(enrollment_id.as_str())
    .bind(concept_id)
    .bind(next_mastery.clamp(0.0, 1.0))
    .bind(now)
    .bind(now)
    .execute(&mut **transaction)
    .await
    .map_err(internal)?;
    Ok(())
}

fn course_summary_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<CourseSummary, AppError> {
    let source_kb_id = row
        .try_get::<Option<String>, _>("source_kb_id")
        .map_err(internal)?
        .map(parse_id)
        .transpose()?;
    Ok(CourseSummary {
        id: parse_id(row.try_get("course_id").map_err(internal)?)?,
        title: row.try_get("title").map_err(internal)?,
        description: row.try_get("description").map_err(internal)?,
        domain: row.try_get("domain").map_err(internal)?,
        source_kb_id,
        version: row.try_get("version").map_err(internal)?,
        enrolled: row.try_get::<i64, _>("enrolled").map_err(internal)? != 0,
        total_lessons: row.try_get("total_lessons").map_err(internal)?,
        completed_lessons: row.try_get("completed_lessons").map_err(internal)?,
        updated_at: row.try_get("updated_at").map_err(internal)?,
        tags: Vec::new(),
    })
}

fn parse_id<T>(value: String) -> Result<T, AppError>
where
    T: FromStr<Err = UuidV7Error>,
{
    value
        .parse()
        .map_err(|error| AppError::Internal(format!("invalid persisted ID {value}: {error}")))
}

/// Creates one immediately-due review item per concept of a lesson when the
/// learner completes it. Existing items keep their schedule untouched.
async fn seed_lesson_review_items(
    transaction: &mut Transaction<'_, Sqlite>,
    enrollment_id: &LearningEnrollmentId,
    lesson_id: &LearningLessonId,
    now: i64,
) -> Result<(), AppError> {
    let concept_ids: Vec<String> = sqlx::query_scalar(
        "SELECT concept_id FROM learning_lesson_concepts WHERE lesson_id = ?",
    )
    .bind(lesson_id.as_str())
    .fetch_all(&mut **transaction)
    .await
    .map_err(internal)?;
    for concept_id in concept_ids {
        ensure_review_item(transaction, enrollment_id, &concept_id, now).await?;
    }
    Ok(())
}

/// Creates the initial review item the first time a learner practices a
/// concept, due immediately so it shows up in the queue. Existing items keep
/// their schedule untouched: in-course attempts never reschedule, only the
/// review queue does.
async fn ensure_review_item(
    transaction: &mut Transaction<'_, Sqlite>,
    enrollment_id: &LearningEnrollmentId,
    concept_id: &str,
    now: i64,
) -> Result<(), AppError> {
    let exists: Option<String> = sqlx::query_scalar(
        "SELECT review_item_id FROM learning_review_items \
         WHERE enrollment_id = ? AND concept_id = ?",
    )
    .bind(enrollment_id.as_str())
    .bind(concept_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(internal)?;
    if exists.is_some() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO learning_review_items \
         (review_item_id, enrollment_id, concept_id, due_at, stability_days, difficulty, \
          review_count, lapse_count, last_reviewed_at, updated_at) \
         VALUES (?, ?, ?, ?, 0, 5.0, 0, 0, NULL, ?)",
    )
    .bind(LearningReviewItemId::new().into_string())
    .bind(enrollment_id.as_str())
    .bind(concept_id)
    .bind(now)
    .bind(now)
    .execute(&mut **transaction)
    .await
    .map_err(internal)?;
    Ok(())
}

fn internal(error: impl std::fmt::Display) -> AppError {
    AppError::Internal(error.to_string())
}

/// Skipping a due review defers it by a full day without rating it.
const SKIP_DELAY_MS: i64 = 86_400_000;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ActivityPack, ConceptPack, LessonPack, ModulePack};
    use serde_json::json;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    fn valid_pack() -> CoursePack {
        CoursePack {
            title: "Linear Algebra".into(),
            description: "A small generic course".into(),
            domain: "mathematics".into(),
            source_kb_id: None,
            version: 1,
            concepts: vec![ConceptPack {
                key: "vector".into(),
                title: "Vector".into(),
                description: String::new(),
                prerequisites: Vec::new(),
            }],
            modules: vec![ModulePack {
                title: "Foundations".into(),
                description: String::new(),
                lessons: vec![LessonPack {
                    title: "Vectors".into(),
                    summary: String::new(),
                    estimated_minutes: 10,
                    source: None,
                    concepts: vec!["vector".into()],
                    activities: vec![ActivityPack {
                        kind: ActivityKind::TrueFalse,
                        prompt: "A vector has magnitude and direction.".into(),
                        options: Vec::new(),
                        answer: Value::Bool(true),
                        explanation: "That is the geometric definition.".into(),
                        concepts: vec!["vector".into()],
                    distractors: Vec::new(),
                    }],
                }],
            }],
        }
    }

    #[test]
    fn pack_validation_rejects_unknown_concepts() {
        let mut pack = valid_pack();
        pack.modules[0].lessons[0].concepts = vec!["missing".into()];
        let error = validate_pack(&pack).unwrap_err();
        assert!(error.to_string().contains("unknown concept key"));
    }

    #[test]
    fn pack_validation_rejects_prerequisite_cycles() {
        let mut pack = valid_pack();
        pack.concepts.push(ConceptPack {
            key: "matrix".into(),
            title: "Matrix".into(),
            description: String::new(),
            prerequisites: vec!["vector".into()],
        });
        pack.concepts[0].prerequisites = vec!["matrix".into()];
        let error = validate_pack(&pack).unwrap_err();
        assert!(error.to_string().contains("prerequisite cycle"));
    }

    #[test]
    fn builtin_evaluator_does_not_trust_client_scores() {
        let config = StoredActivityConfig {
            options: Vec::new(),
            answer: Value::Bool(true),
            explanation: "source-backed explanation".into(),
            distractors: Vec::new(),
        };
        let (score, _) = evaluate(ActivityKind::TrueFalse, &config, &Value::Bool(false)).unwrap();
        assert_eq!(score, 0.0);
    }

    #[test]
    fn recommendation_repairs_out_of_order_prerequisites() {
        let prerequisite_id = LearningConceptId::new();
        let advanced_id = LearningConceptId::new();
        let prerequisite_lesson_id = LearningLessonId::new();
        let advanced_lesson_id = LearningLessonId::new();
        let lesson = |id: LearningLessonId, title: &str, concept: LearningConceptId| LessonView {
            id,
            title: title.into(),
            summary: String::new(),
            position: 0,
            estimated_minutes: 10,
            source: None,
            status: LessonStatus::NotStarted,
            concepts: vec![concept],
            activities: Vec::new(),
        };
        let modules = vec![ModuleView {
            id: LearningModuleId::new(),
            title: "Module".into(),
            description: String::new(),
            position: 0,
            lessons: vec![
                lesson(advanced_lesson_id, "Advanced", advanced_id.clone()),
                lesson(
                    prerequisite_lesson_id.clone(),
                    "Prerequisite",
                    prerequisite_id.clone(),
                ),
            ],
        }];
        let concepts = vec![
            ConceptView {
                id: prerequisite_id.clone(),
                key: "prerequisite".into(),
                title: "Prerequisite".into(),
                description: String::new(),
                prerequisites: Vec::new(),
                mastery: None,
            },
            ConceptView {
                id: advanced_id,
                key: "advanced".into(),
                title: "Advanced".into(),
                description: String::new(),
                prerequisites: vec![prerequisite_id],
                mastery: None,
            },
        ];
        assert_eq!(
            recommend_next_lesson(&modules, &concepts),
            Some(prerequisite_lesson_id)
        );
    }

    #[tokio::test]
    async fn imports_enrolls_and_updates_mastery() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let owner_id = nomifun_db::installation_owner_id(database.pool())
            .await
            .unwrap();
        let user_id = UserId::parse(owner_id).unwrap();
        let service = LearningService::new(database.pool().clone());
        let course = service.import_course(valid_pack()).await.unwrap();
        service.enroll(&course.course.id, &user_id).await.unwrap();
        let detail = service
            .course_detail(&course.course.id, Some(&user_id))
            .await
            .unwrap();
        assert_eq!(
            detail.next_lesson_id.as_ref(),
            Some(&detail.modules[0].lessons[0].id)
        );
        let diagnostic = service
            .diagnostic_plan(&course.course.id, &user_id, 10)
            .await
            .unwrap();
        assert_eq!(diagnostic.total_concepts, 1);
        assert_eq!(diagnostic.items.len(), 1);
        let activity_id = detail.modules[0].lessons[0].activities[0].id.clone();
        let lesson_id = detail.modules[0].lessons[0].id.clone();
        let result = service
            .submit_attempt(&activity_id, &user_id, Value::Bool(true), None, None)
            .await
            .unwrap();
        assert!(result.passed);
        service
            .update_lesson_progress(&lesson_id, &user_id, LessonStatus::Completed)
            .await
            .unwrap();
        let detail = service
            .course_detail(&course.course.id, Some(&user_id))
            .await
            .unwrap();
        assert_eq!(detail.concepts[0].mastery, Some(1.0));
        assert_eq!(detail.next_lesson_id, None);
        // Completing the lesson admits its concepts into the review queue
        // (immediately-due seed), but the seed must not count as a review:
        // counts stay at zero until the learner actually uses the queue.
        let (count, reviews): (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), COALESCE(SUM(review_count), 0) FROM learning_review_items",
        )
        .fetch_one(database.pool())
        .await
        .unwrap();
        assert_eq!(count, 1);
        assert_eq!(reviews, 0);
    }

    #[tokio::test]
    async fn practice_flows_join_implicitly_without_explicit_enroll() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let owner_id = nomifun_db::installation_owner_id(database.pool())
            .await
            .unwrap();
        let user_id = UserId::parse(owner_id).unwrap();
        let service = LearningService::new(database.pool().clone());
        let course = service.import_course(valid_pack()).await.unwrap();
        let course_id = &course.course.id;
        // No explicit enroll anywhere: opening the detail must create the
        // enrollment so diagnostics, attempts and progress writes all work.
        let detail = service.course_detail(course_id, Some(&user_id)).await.unwrap();
        assert!(detail.enrollment_id.is_some());
        let diagnostic = service.diagnostic_plan(course_id, &user_id, 10).await.unwrap();
        assert_eq!(diagnostic.items.len(), 1);
        let activity_id = detail.modules[0].lessons[0].activities[0].id.clone();
        let lesson_id = detail.modules[0].lessons[0].id.clone();
        let result = service
            .submit_attempt(&activity_id, &user_id, Value::Bool(true), None, None)
            .await
            .unwrap();
        assert!(result.passed);
        service
            .update_lesson_progress(&lesson_id, &user_id, LessonStatus::Completed)
            .await
            .unwrap();
        // A second detail read must reuse the same enrollment (idempotent).
        let again = service.course_detail(course_id, Some(&user_id)).await.unwrap();
        assert_eq!(again.enrollment_id, detail.enrollment_id);
    }

    #[tokio::test]
    async fn question_entries_aligns_states_with_review_queue() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let owner_id = nomifun_db::installation_owner_id(database.pool())
            .await
            .unwrap();
        let user_id = UserId::parse(owner_id).unwrap();
        let service = LearningService::new(database.pool().clone());

        // One concept shared across two lessons: completing lesson A seeds the
        // concept's review item, lesson B is never touched.
        let shared = ActivityPack {
            kind: ActivityKind::TrueFalse,
            prompt: String::new(),
            options: Vec::new(),
            answer: Value::Bool(true),
            explanation: String::new(),
            concepts: vec!["shared".into()],
            distractors: Vec::new(),
        };
        let pack = CoursePack {
            title: "Shared Concepts".into(),
            description: String::new(),
            domain: "general".into(),
            source_kb_id: None,
            version: 1,
            concepts: vec![ConceptPack {
                key: "shared".into(),
                title: "Shared".into(),
                description: String::new(),
                prerequisites: Vec::new(),
            }],
            modules: vec![ModulePack {
                title: "Module".into(),
                description: String::new(),
                lessons: vec![
                    LessonPack {
                        title: "Lesson A".into(),
                        summary: String::new(),
                        estimated_minutes: 10,
                        source: None,
                        concepts: vec!["shared".into()],
                        activities: vec![ActivityPack {
                            prompt: "A1".into(),
                            ..shared.clone()
                        }],
                    },
                    LessonPack {
                        title: "Lesson B".into(),
                        summary: String::new(),
                        estimated_minutes: 10,
                        source: None,
                        concepts: vec!["shared".into()],
                        activities: vec![ActivityPack {
                            prompt: "A2".into(),
                            ..shared
                        }],
                    },
                ],
            }],
        };
        let course = service.import_course(pack).await.unwrap();
        service.enroll(&course.course.id, &user_id).await.unwrap();
        let detail = service
            .course_detail(&course.course.id, Some(&user_id))
            .await
            .unwrap();
        let lesson_a = &detail.modules[0].lessons[0];
        service
            .update_lesson_progress(&lesson_a.id, &user_id, LessonStatus::Completed)
            .await
            .unwrap();

        fn state_of<'a>(entries: &'a [QuestionEntry], prompt: &str) -> Option<&'a str> {
            entries
                .iter()
                .find(|entry| entry.prompt.as_deref() == Some(prompt))
                .map(|entry| entry.state.as_str())
        }
        let entries = service
            .question_entries(&user_id, None, None, None)
            .await
            .unwrap();
        // Lesson B is not completed: A2 must not claim a queue state even
        // though the shared concept already has a review item seeded by A.
        assert_eq!(state_of(&entries, "A1"), Some("new"));
        assert_eq!(state_of(&entries, "A2"), Some("unlearned"));

        // Simulate an overdue, already-reviewed concept: lesson A's row turns
        // due while lesson B's row stays unlearned, so the question-manager
        // counts agree with the review queue.
        sqlx::query("UPDATE learning_review_items SET review_count = 1, due_at = ?")
            .bind(now_ms() - 1000)
            .execute(database.pool())
            .await
            .unwrap();
        let entries = service
            .question_entries(&user_id, None, None, None)
            .await
            .unwrap();
        assert_eq!(state_of(&entries, "A1"), Some("due"));
        assert_eq!(state_of(&entries, "A2"), Some("unlearned"));

        // The queue itself serves exactly the completed lesson's question.
        let due = service
            .due_reviews(&user_id, 30, &[], true, false, &[])
            .await
            .unwrap();
       assert_eq!(due.len(), 1);
        assert_eq!(due[0].question.prompt, "A1");
    }

    /// A reflection-only course with two concepts: the activity targets
    /// "vector" while the full course list also carries "matrix" — the
    /// coverage checklist AI grading must evaluate against.
    fn reflection_pack() -> CoursePack {
        CoursePack {
            title: "Reflective Learning".into(),
            description: String::new(),
            domain: "general".into(),
            source_kb_id: None,
            version: 1,
            concepts: vec![
                ConceptPack {
                    key: "vector".into(),
                    title: "Vector".into(),
                    description: "magnitude and direction".into(),
                    prerequisites: Vec::new(),
                },
                ConceptPack {
                    key: "matrix".into(),
                    title: "Matrix".into(),
                    description: "rectangular number grid".into(),
                    prerequisites: Vec::new(),
                },
            ],
            modules: vec![ModulePack {
                title: "Foundations".into(),
                description: String::new(),
                lessons: vec![LessonPack {
                    title: "Vectors".into(),
                    summary: String::new(),
                    estimated_minutes: 10,
                    source: None,
                    concepts: vec!["vector".into(), "matrix".into()],
                    activities: vec![ActivityPack {
                        kind: ActivityKind::Reflection,
                        prompt: "Explain what a vector is.".into(),
                        options: Vec::new(),
                        answer: Value::Null,
                        explanation: "A vector has magnitude and direction.".into(),
                        concepts: vec!["vector".into()],
                    distractors: Vec::new(),
                    }],
                }],
            }],
        }
    }

    /// Scripted `KnowledgeCompleter` recording calls, the last user message
    /// and the last explicit `(provider_id, model)` override; `fail` makes
    /// every call error out so fallback paths can be exercised.
    struct ScriptedCompleter {
        reply: String,
        fail: bool,
        calls: AtomicUsize,
        last_user: Mutex<Option<String>>,
        last_override: Mutex<Option<(String, String)>>,
    }

    impl ScriptedCompleter {
        fn new(reply: impl Into<String>, fail: bool) -> Arc<Self> {
            Arc::new(Self {
                reply: reply.into(),
                fail,
                calls: AtomicUsize::new(0),
                last_user: Mutex::new(None),
                last_override: Mutex::new(None),
            })
        }
    }

    #[async_trait::async_trait]
    impl KnowledgeCompleter for ScriptedCompleter {
        async fn complete(&self, _system: &str, user: &str) -> Result<String, AppError> {
            self.calls.fetch_add(1, AtomicOrdering::SeqCst);
            *self.last_user.lock().unwrap() = Some(user.to_owned());
            *self.last_override.lock().unwrap() = None;
            if self.fail {
                return Err(AppError::Internal("model unavailable".into()));
            }
            Ok(self.reply.clone())
        }

        async fn complete_with(
            &self,
            _system: &str,
            user: &str,
            provider_id: &str,
            model: &str,
        ) -> Result<String, AppError> {
            self.calls.fetch_add(1, AtomicOrdering::SeqCst);
            *self.last_user.lock().unwrap() = Some(user.to_owned());
            *self.last_override.lock().unwrap() = Some((provider_id.to_owned(), model.to_owned()));
            if self.fail {
                return Err(AppError::Internal("model unavailable".into()));
            }
            Ok(self.reply.clone())
        }
    }

    async fn reflection_service_with_completer(
        completer: Arc<ScriptedCompleter>,
    ) -> (LearningService, nomifun_db::SqlitePool, UserId, LearningActivityId) {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let owner_id = nomifun_db::installation_owner_id(database.pool())
            .await
            .unwrap();
        let user_id = UserId::parse(owner_id).unwrap();
        let service = LearningService::new(database.pool().clone());
        *service.course_completer.write().unwrap() = Some(completer);
        let course = service.import_course(reflection_pack()).await.unwrap();
        service.enroll(&course.course.id, &user_id).await.unwrap();
        let detail = service
            .course_detail(&course.course.id, Some(&user_id))
            .await
            .unwrap();
        let activity_id = detail.modules[0].lessons[0].activities[0].id.clone();
        (service, database.pool().clone(), user_id, activity_id)
    }

    #[tokio::test]
    async fn reflection_ai_grading_uses_model_reply() {
        let completer = ScriptedCompleter::new(
            "{\"score\":0.75,\"feedback\":\"## 评价\\n方向正确，但推导不完整。\"}",
            false,
        );
        let (service, pool, user_id, activity_id) =
            reflection_service_with_completer(completer.clone()).await;
        let result = service
            .submit_attempt(
                &activity_id,
                &user_id,
                Value::String("A vector is a quantity with magnitude and direction.".into()),
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(result.score, 0.75);
        assert!(result.passed);
        assert!(result.feedback.contains("方向正确"));
        assert_eq!(completer.calls.load(AtomicOrdering::SeqCst), 1);
        // The grading prompt carries the answer AND the exercise's linked
        // concepts (its own lesson's concepts — never other lessons').
        let user = completer.last_user.lock().unwrap().clone().unwrap();
        assert!(user.contains("Explain what a vector is."));
        assert!(user.contains("A vector is a quantity"));
        assert!(user.contains("Vector"));
        // The AI score feeds the mastery state and the persisted attempt.
        let (mastery,): (f64,) = sqlx::query_as(
            "SELECT mastery FROM learning_mastery_states \
             WHERE enrollment_id = (SELECT enrollment_id FROM learning_enrollments WHERE user_id = ?)",
        )
        .bind(user_id.as_str())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(mastery, 0.75);
        let (score, feedback): (f64, String) =
            sqlx::query_as("SELECT score, feedback FROM learning_attempts WHERE activity_id = ?")
                .bind(activity_id.as_str())
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(score, 0.75);
        assert_eq!(feedback, result.feedback);
        // Empty answers are rejected before any model call, exactly like the
        // rule-based evaluator.
        let error = service
            .submit_attempt(&activity_id, &user_id, Value::String("   ".into()), None, None)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("must not be empty"));
        assert_eq!(completer.calls.load(AtomicOrdering::SeqCst), 1);
    }

    #[tokio::test]
    async fn reflection_ai_grading_falls_back_to_rule_based() {
        // Model call error: rule-based grading kicks in (non-empty passes
        // with the stored explanation).
        let failing = ScriptedCompleter::new(String::new(), true);
        let (service, _, user_id, activity_id) =
            reflection_service_with_completer(failing.clone()).await;
        let result = service
            .submit_attempt(
                &activity_id,
                &user_id,
                Value::String("Vectors have magnitude and direction.".into()),
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(result.score, 1.0);
        assert_eq!(result.feedback, "A vector has magnitude and direction.");
        assert_eq!(failing.calls.load(AtomicOrdering::SeqCst), 1);

        // Unparseable reply: same degradation, still no error.
        let bad_reply = ScriptedCompleter::new("not json at all", false);
        *service.course_completer.write().unwrap() = Some(bad_reply.clone());
        let result = service
            .submit_attempt(
                &activity_id,
                &user_id,
                Value::String("still a non-empty answer".into()),
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(result.score, 1.0);
        assert_eq!(bad_reply.calls.load(AtomicOrdering::SeqCst), 1);

        // No completer configured at all: rule-based, no error.
        *service.course_completer.write().unwrap() = None;
        let result = service
            .submit_attempt(
                &activity_id,
                &user_id,
                Value::String("plain answer".into()),
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(result.score, 1.0);
    }

    #[tokio::test]
    async fn reflection_ai_grading_forwards_explicit_model() {
        let completer = ScriptedCompleter::new(
            r#"{"score":0.6,"feedback":"ok"}"#,
            false,
        );
        let (service, _, user_id, activity_id) =
            reflection_service_with_completer(completer.clone()).await;
        let provider_id = ProviderId::new();
        service
            .submit_attempt(
                &activity_id,
                &user_id,
                Value::String("an answer".into()),
                Some(provider_id.clone()),
                Some("gpt-test".into()),
            )
            .await
            .unwrap();
        assert_eq!(
            *completer.last_override.lock().unwrap(),
            Some((provider_id.into_string(), "gpt-test".into()))
        );
        // Without a pair the default complete() path is used.
        service
            .submit_attempt(
                &activity_id,
                &user_id,
                Value::String("another answer".into()),
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(*completer.last_override.lock().unwrap(), None);
    }

    #[tokio::test]
    async fn objective_attempts_never_touch_the_completer() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let owner_id = nomifun_db::installation_owner_id(database.pool())
            .await
            .unwrap();
        let user_id = UserId::parse(owner_id).unwrap();
        let service = LearningService::new(database.pool().clone());
        let completer = ScriptedCompleter::new(String::new(), true);
        *service.course_completer.write().unwrap() = Some(completer.clone());
        let course = service.import_course(valid_pack()).await.unwrap();
        service.enroll(&course.course.id, &user_id).await.unwrap();
        let detail = service
            .course_detail(&course.course.id, Some(&user_id))
            .await
            .unwrap();
        let activity_id = detail.modules[0].lessons[0].activities[0].id.clone();
        let result = service
            .submit_attempt(&activity_id, &user_id, Value::Bool(true), None, None)
            .await
            .unwrap();
        assert!(result.passed);
        assert_eq!(result.feedback, "That is the geometric definition.");
        assert_eq!(completer.calls.load(AtomicOrdering::SeqCst), 0);
    }

    /// A fill-in-the-blank pack mirroring `valid_pack`: the blank sits at a
    /// relationship-critical spot, the accepted answer list tolerates case
    /// and whitespace variance, and the near-synonym distractor must never
    /// pass grading.
    fn fill_in_blank_pack() -> CoursePack {
        CoursePack {
            title: "Linear Algebra".into(),
            description: "A small generic course".into(),
            domain: "mathematics".into(),
            source_kb_id: None,
            version: 1,
            concepts: vec![ConceptPack {
                key: "vector".into(),
                title: "Vector".into(),
                description: String::new(),
                prerequisites: Vec::new(),
            }],
            modules: vec![ModulePack {
                title: "Foundations".into(),
                description: String::new(),
                lessons: vec![LessonPack {
                    title: "Vectors".into(),
                    summary: String::new(),
                    estimated_minutes: 10,
                    source: None,
                    concepts: vec!["vector".into()],
                    activities: vec![ActivityPack {
                        kind: ActivityKind::FillInBlank,
                        prompt: "A vector has ___ and direction.".into(),
                        options: Vec::new(),
                        answer: json!(["magnitude"]),
                        explanation: "That is the geometric definition.".into(),
                        concepts: vec!["vector".into()],
                        distractors: vec!["length".into()],
                    }],
                }],
            }],
        }
    }

    #[tokio::test]
    async fn fill_in_blank_attempts_grade_against_accepted_answers() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let owner_id = nomifun_db::installation_owner_id(database.pool())
            .await
            .unwrap();
        let user_id = UserId::parse(owner_id).unwrap();
        let service = LearningService::new(database.pool().clone());
        let course = service.import_course(fill_in_blank_pack()).await.unwrap();
        service.enroll(&course.course.id, &user_id).await.unwrap();
        let detail = service
            .course_detail(&course.course.id, Some(&user_id))
            .await
            .unwrap();
        let activity_id = detail.modules[0].lessons[0].activities[0].id.clone();
        // The imported config keeps the near-synonym distractor so the blank
        // is graded against the accepted answers only, never the trap.
        let (config_json,): (String,) = sqlx::query_as(
            "SELECT config_json FROM learning_activities WHERE activity_id = ?",
        )
        .bind(activity_id.as_str())
        .fetch_one(database.pool())
        .await
        .unwrap();
        let config: StoredActivityConfig = serde_json::from_str(&config_json).unwrap();
        assert_eq!(config.distractors, vec!["length"]);
        // Exact match passes; surrounding whitespace and case are ignored.
        let result = service
            .submit_attempt(
                &activity_id,
                &user_id,
                Value::String("  Magnitude ".into()),
                None,
                None,
            )
            .await
            .unwrap();
        assert!(result.passed);
        assert_eq!(result.score, 1.0);
        // The near-synonym distractor is NOT an accepted answer: it fails,
        // which is exactly the fine discrimination the blank demands.
        let result = service
            .submit_attempt(&activity_id, &user_id, Value::String("length".into()), None, None)
            .await
            .unwrap();
        assert!(!result.passed);
        assert_eq!(result.score, 0.0);
        // Empty and non-string responses are rejected outright.
        let error = service
            .submit_attempt(&activity_id, &user_id, Value::String("   ".into()), None, None)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("must not be empty"));
        let error = service
            .submit_attempt(&activity_id, &user_id, Value::Bool(true), None, None)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("must be a string"));
        // Once the lesson is completed, the blank joins the review queue as
        // an objective question like single choice and true/false.
        service
            .submit_attempt(
                &activity_id,
                &user_id,
                Value::String("magnitude".into()),
                None,
                None,
            )
            .await
            .unwrap();
        service
            .update_lesson_progress(
                &detail.modules[0].lessons[0].id,
                &user_id,
                LessonStatus::Completed,
            )
            .await
            .unwrap();
        let due = service
            .due_reviews(&user_id, 30, &[course.course.id], true, false, &[])
            .await
            .unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].question.prompt, "A vector has ___ and direction.");
    }

    #[tokio::test]
    async fn custom_questions_accept_fill_in_blank() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let owner_id = nomifun_db::installation_owner_id(database.pool())
            .await
            .unwrap();
        let user_id = UserId::parse(owner_id).unwrap();
        let service = LearningService::new(database.pool().clone());
        let request = CreateCustomQuestionRequest {
            kind: ActivityKind::FillInBlank,
            prompt: "The derivative of position is ___, and it measures the rate of change.".into(),
            options: Vec::new(),
            answer: json!(["velocity", "velocity vector"]),
            explanation: "Velocity is the rate of change of position.".into(),
            concept_id: None,
            distractors: vec!["speed".into()],
        };
        let question_id = service
            .create_custom_question(&user_id, request)
            .await
            .unwrap();
        let (kind, config_json): (String, String) = sqlx::query_as(
            "SELECT kind, config_json FROM learning_custom_questions WHERE custom_question_id = ?",
        )
        .bind(&question_id)
        .fetch_one(database.pool())
        .await
        .unwrap();
        assert_eq!(kind, "fill_in_blank");
        let config: StoredActivityConfig = serde_json::from_str(&config_json).unwrap();
        assert_eq!(config.answer, json!(["velocity", "velocity vector"]));
        assert_eq!(config.distractors, vec!["speed"]);
        // The custom blank joins the orphan review queue immediately and is
        // graded by the same rule-based evaluator.
        let due = service
            .due_reviews(&user_id, 30, &[], true, true, &[])
            .await
            .unwrap();
        assert_eq!(due.len(), 1);
        let result = service
            .answer_custom_review(
                &question_id,
                &user_id,
                Value::String("Velocity Vector".into()),
                false,
            )
            .await
            .unwrap();
        assert!(result.correct);
        // Payload validation rejects missing blanks, non-array answers and
        // the reflection kind, mirroring the generated side.
        let invalid = |prompt: &str, answer: Value| CreateCustomQuestionRequest {
            kind: ActivityKind::FillInBlank,
            prompt: prompt.into(),
            options: Vec::new(),
            answer,
            explanation: String::new(),
            concept_id: None,
            distractors: vec!["trap".into()],
        };
        let error = service
            .create_custom_question(&user_id, invalid("no blank here", json!(["x"])))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("___"));
        let error = service
            .create_custom_question(&user_id, invalid("A ___ blank.", json!("x")))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("JSON array"));
        let error = service
            .create_custom_question(
                &user_id,
                CreateCustomQuestionRequest {
                    kind: ActivityKind::Reflection,
                    prompt: "Reflect.".into(),
                    options: Vec::new(),
                    answer: Value::Null,
                    explanation: String::new(),
                    concept_id: None,
                    distractors: Vec::new(),
                },
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("only support"));
    }
}

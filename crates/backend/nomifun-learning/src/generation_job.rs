use std::sync::Arc;

use nomifun_common::{AppError, now_ms};
use nomifun_db::SqlitePool;
use nomifun_knowledge::{KnowledgeCompleter, KnowledgeService};
use sqlx::Row;

use crate::generation::{
    Blueprint, LessonOutput, assemble_pack, build_blueprint_prompt, generate_blueprint,
    generate_lesson, sample_base_files, validate_generated_pack,
};
use crate::models::GenerateCourseRequest;
use crate::service::LearningService;

/// Background driver for persistent course-generation jobs. Every job is one
/// row in `learning_course_jobs`; a claimed job gets a single spawned task
/// that walks the pipeline stages, persisting each snapshot before advancing.
/// A crash or process exit therefore resumes from the last snapshot, losing
/// at most one in-flight lesson. The job row is the only coordination state,
/// which is what makes cancel/resume/retry and multi-session parallelism work
/// without any in-memory registry.
pub(crate) struct GenerationJobRunner {
    pool: SqlitePool,
    service: LearningService,
    knowledge_service: Arc<KnowledgeService>,
    completer: Arc<dyn KnowledgeCompleter>,
}

impl GenerationJobRunner {
    pub(crate) fn new(
        pool: SqlitePool,
        service: LearningService,
        knowledge_service: Arc<KnowledgeService>,
        completer: Arc<dyn KnowledgeCompleter>,
    ) -> Self {
        Self {
            pool,
            service,
            knowledge_service,
            completer,
        }
    }

    /// Atomically claim the job and spawn its runner task. Returns whether a
    /// task was spawned. The claim infers the next stage from the persisted
    /// snapshots, so `queued`, `interrupted`, `cancelled` and `failed` jobs
    /// all resume in place; a pending cancel request wins the claim so a job
    /// cancelled while queued never starts.
    pub(crate) async fn claim_and_spawn(&self, job_id: &str) -> Result<bool, AppError> {
        let claimed = sqlx::query(
            "UPDATE learning_course_jobs SET status = CASE WHEN cancel_requested = 1 THEN 'cancelled' WHEN samples_json IS NULL THEN 'sampling' WHEN blueprint_json IS NULL THEN 'blueprint' ELSE 'lessons' END, cancel_requested = 0, error = NULL, updated_at = ? WHERE job_id = ? AND status IN ('queued', 'interrupted', 'cancelled', 'failed')",
        )
        .bind(now_ms())
        .bind(job_id)
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        if claimed.rows_affected() != 1 {
            return Ok(false);
        }
        let status: String =
            sqlx::query_scalar("SELECT status FROM learning_course_jobs WHERE job_id = ?")
                .bind(job_id)
                .fetch_one(&self.pool)
                .await
                .map_err(internal)?;
        if status == "cancelled" {
            return Ok(false);
        }
        let pool = self.pool.clone();
        let service = self.service.clone();
        let knowledge_service = Arc::clone(&self.knowledge_service);
        let completer = Arc::clone(&self.completer);
        let job_id = job_id.to_owned();
        tokio::spawn(async move {
            if let Err(error) =
                run_job(&pool, &service, &knowledge_service, completer.as_ref(), &job_id).await
            {
                mark_failed(&pool, &job_id, &error.to_string()).await;
            }
        });
        Ok(true)
    }
}

/// One row snapshot the runner needs. Re-read at the top of every loop so
/// cancel requests are always observed between stages.
struct JobRow {
    status: String,
    cancel_requested: bool,
    request_json: String,
    samples_json: Option<String>,
    blueprint_json: Option<String>,
    lesson_outputs_json: Option<String>,
    total_lessons: i64,
}

/// Drive one claimed job through its pipeline. Cancel is checked at stage
/// boundaries only — an in-flight model call finishes before the flag is
/// honored (at most one lesson is generated past the request).
async fn run_job(
    pool: &SqlitePool,
    service: &LearningService,
    knowledge_service: &KnowledgeService,
    completer: &dyn KnowledgeCompleter,
    job_id: &str,
) -> Result<(), AppError> {
    loop {
        let row = fetch_job(pool, job_id).await?;
        if row.cancel_requested {
            mark_cancelled(pool, job_id).await?;
            return Ok(());
        }
        match row.status.as_str() {
            "sampling" => run_sampling(pool, knowledge_service, job_id, &row).await?,
            "blueprint" => {
                run_blueprint(pool, knowledge_service, completer, job_id, &row).await?
            }
            "lessons" => run_lessons(pool, completer, job_id, &row).await?,
            "importing" => {
                run_importing(pool, service, job_id, &row).await?;
                return Ok(());
            }
            _ => return Ok(()),
        }
    }
}

async fn run_sampling(
    pool: &SqlitePool,
    knowledge_service: &KnowledgeService,
    job_id: &str,
    row: &JobRow,
) -> Result<(), AppError> {
    let request = parse_request(&row.request_json)?;
    let samples =
        sample_base_files(knowledge_service, request.knowledge_base_id.as_str()).await?;
    let samples_json = serde_json::to_string(&samples).map_err(internal)?;
    sqlx::query(
        "UPDATE learning_course_jobs SET samples_json = ?, status = 'blueprint', updated_at = ? WHERE job_id = ?",
    )
    .bind(&samples_json)
    .bind(now_ms())
    .bind(job_id)
    .execute(pool)
    .await
    .map_err(internal)?;
    Ok(())
}

async fn run_blueprint(
    pool: &SqlitePool,
    knowledge_service: &KnowledgeService,
    completer: &dyn KnowledgeCompleter,
    job_id: &str,
    row: &JobRow,
) -> Result<(), AppError> {
    let request = parse_request(&row.request_json)?;
    let samples: Vec<(String, String)> = parse_snapshot("samples", row.samples_json.as_deref())?;
    let base = knowledge_service
        .get_base_info(request.knowledge_base_id.as_str())
        .await?;
    let model_override = request.provider_id.as_ref().zip(request.model.as_deref());
    let blueprint_prompt = build_blueprint_prompt(
        &base.name,
        &base.description,
        request.domain.as_deref(),
        request.module_count,
        request.lessons_per_module,
        &samples,
    );
    let blueprint = generate_blueprint(
        completer,
        model_override,
        &blueprint_prompt,
        &samples,
        request.module_count,
        request.lessons_per_module,
    )
    .await?;
    let total_lessons: i64 = blueprint
        .modules
        .iter()
        .map(|module| module.lessons.len() as i64)
        .sum();
    let blueprint_json = serde_json::to_string(&blueprint).map_err(internal)?;
    sqlx::query(
        "UPDATE learning_course_jobs SET blueprint_json = ?, total_lessons = ?, status = 'lessons', updated_at = ? WHERE job_id = ?",
    )
    .bind(&blueprint_json)
    .bind(total_lessons)
    .bind(now_ms())
    .bind(job_id)
    .execute(pool)
    .await
    .map_err(internal)?;
    Ok(())
}

async fn run_lessons(
    pool: &SqlitePool,
    completer: &dyn KnowledgeCompleter,
    job_id: &str,
    row: &JobRow,
) -> Result<(), AppError> {
    let request = parse_request(&row.request_json)?;
    let samples: Vec<(String, String)> = parse_snapshot("samples", row.samples_json.as_deref())?;
    let blueprint: Blueprint = parse_snapshot("blueprint", row.blueprint_json.as_deref())?;
    let mut outputs: Vec<LessonOutput> = match row.lesson_outputs_json.as_deref() {
        Some(json) => serde_json::from_str(json).map_err(internal)?,
        None => Vec::new(),
    };
    let total_lessons = row.total_lessons as usize;
    if outputs.len() >= total_lessons {
        // Every lesson is already persisted; only the import stage is pending
        // (e.g. the process exited right after the last lesson write).
        sqlx::query(
            "UPDATE learning_course_jobs SET status = 'importing', updated_at = ? WHERE job_id = ?",
        )
        .bind(now_ms())
        .bind(job_id)
        .execute(pool)
        .await
        .map_err(internal)?;
        return Ok(());
    }
    let (module_index, lesson_index) = cursor_position(&blueprint, outputs.len());
    let module = &blueprint.modules[module_index];
    let lesson = &module.lessons[lesson_index];
    let excerpt = lesson
        .source
        .as_ref()
        .and_then(|source| {
            samples
                .iter()
                .find(|(path, _)| path == &source.path)
                .map(|(_, excerpt)| excerpt.as_str())
        })
        .unwrap_or_default();
    let next_lesson_title = module
        .lessons
        .get(lesson_index + 1)
        .map(|next| next.title.as_str());
    let model_override = request.provider_id.as_ref().zip(request.model.as_deref());
    let output = generate_lesson(
        completer,
        model_override,
        &blueprint,
        module,
        lesson,
        module_index,
        lesson_index,
        total_lessons,
        next_lesson_title,
        excerpt,
    )
    .await
    .map_err(|error| {
        AppError::UnprocessableEntity(format!(
            "lesson \"{}\" failed to generate: {error}",
            lesson.title
        ))
    })?;
    outputs.push(output);
    let outputs_json = serde_json::to_string(&outputs).map_err(internal)?;
    sqlx::query(
        "UPDATE learning_course_jobs SET lesson_outputs_json = ?, current_module = ?, current_lesson = ?, updated_at = ? WHERE job_id = ?",
    )
    .bind(&outputs_json)
    .bind((module_index + 1) as i64)
    .bind(outputs.len() as i64)
    .bind(now_ms())
    .bind(job_id)
    .execute(pool)
    .await
    .map_err(internal)?;
    Ok(())
}

/// Map the number of completed lessons to blueprint coordinates. The lesson
/// outputs array is appended strictly in blueprint order, so the cursor is
/// derivable from its length alone; the persisted `current_module` /
/// `current_lesson` columns are a display projection of the same cursor.
fn cursor_position(blueprint: &Blueprint, completed: usize) -> (usize, usize) {
    let mut remaining = completed;
    for (module_index, module) in blueprint.modules.iter().enumerate() {
        if remaining < module.lessons.len() {
            return (module_index, remaining);
        }
        remaining -= module.lessons.len();
    }
    (blueprint.modules.len().saturating_sub(1), 0)
}

async fn run_importing(
    pool: &SqlitePool,
    service: &LearningService,
    job_id: &str,
    row: &JobRow,
) -> Result<(), AppError> {
    let request = parse_request(&row.request_json)?;
    let samples: Vec<(String, String)> = parse_snapshot("samples", row.samples_json.as_deref())?;
    let blueprint: Blueprint = parse_snapshot("blueprint", row.blueprint_json.as_deref())?;
    let outputs: Vec<LessonOutput> =
        parse_snapshot("lesson outputs", row.lesson_outputs_json.as_deref())?;
    let pack = assemble_pack(blueprint, outputs, &request);
    validate_generated_pack(&pack, &samples).map_err(|error| {
        AppError::UnprocessableEntity(format!("model did not return a valid course pack: {error}"))
    })?;
    let detail = service.import_course(pack).await?;
    sqlx::query(
        "UPDATE learning_course_jobs SET course_id = ?, status = 'completed', updated_at = ? WHERE job_id = ?",
    )
    .bind(detail.course.id.as_str())
    .bind(now_ms())
    .bind(job_id)
    .execute(pool)
    .await
    .map_err(internal)?;
    Ok(())
}

async fn fetch_job(pool: &SqlitePool, job_id: &str) -> Result<JobRow, AppError> {
    let row = sqlx::query(
        "SELECT status, cancel_requested, request_json, samples_json, blueprint_json, lesson_outputs_json, total_lessons FROM learning_course_jobs WHERE job_id = ?",
    )
    .bind(job_id)
    .fetch_one(pool)
    .await
    .map_err(internal)?;
    Ok(JobRow {
        status: row.try_get("status").map_err(internal)?,
        cancel_requested: row.try_get::<i64, _>("cancel_requested").map_err(internal)? != 0,
        request_json: row.try_get("request_json").map_err(internal)?,
        samples_json: row.try_get("samples_json").map_err(internal)?,
        blueprint_json: row.try_get("blueprint_json").map_err(internal)?,
        lesson_outputs_json: row.try_get("lesson_outputs_json").map_err(internal)?,
        total_lessons: row.try_get("total_lessons").map_err(internal)?,
    })
}

async fn mark_cancelled(pool: &SqlitePool, job_id: &str) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE learning_course_jobs SET status = 'cancelled', updated_at = ? WHERE job_id = ? AND status NOT IN ('completed', 'failed', 'cancelled')",
    )
    .bind(now_ms())
    .bind(job_id)
    .execute(pool)
    .await
    .map_err(internal)?;
    Ok(())
}

/// Best-effort failure recording: the runner is already unwinding, so a
/// failed write here must not turn into a second error path. A pending
/// cancel request wins over the failure — the user already asked to stop,
/// so the job lands in `cancelled` (resumable) instead of `failed`.
async fn mark_failed(pool: &SqlitePool, job_id: &str, error: &str) {
    let _ = sqlx::query(
        "UPDATE learning_course_jobs \
         SET status = CASE WHEN cancel_requested = 1 THEN 'cancelled' ELSE 'failed' END, \
             error = CASE WHEN cancel_requested = 1 THEN NULL ELSE ? END, updated_at = ? \
         WHERE job_id = ? AND status NOT IN ('completed', 'cancelled')",
    )
    .bind(error)
    .bind(now_ms())
    .bind(job_id)
    .execute(pool)
    .await;
}

fn parse_request(json: &str) -> Result<GenerateCourseRequest, AppError> {
    serde_json::from_str(json)
        .map_err(|error| AppError::Internal(format!("corrupt job request snapshot: {error}")))
}

fn parse_snapshot<T: serde::de::DeserializeOwned>(
    what: &str,
    json: Option<&str>,
) -> Result<T, AppError> {
    let json = json.ok_or_else(|| AppError::Internal(format!("job {what} snapshot is missing")))?;
    serde_json::from_str(json)
        .map_err(|error| AppError::Internal(format!("corrupt job {what} snapshot: {error}")))
}

fn internal(error: impl std::fmt::Display) -> AppError {
    AppError::Internal(error.to_string())
}

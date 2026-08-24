use super::*;

impl LearningService {

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
        // One active generation job per knowledge base at a time. The agent
        // path can fire the generate tool twice in a single provider turn
        // (parallel tool calls) or right after a lost response; every job
        // imports its own course, so two jobs would silently produce two
        // near-identical courses. Reject the duplicate up front and point at
        // the already-running job so the caller can wait or cancel it.
        let active_job: Option<String> = sqlx::query_scalar(
            "SELECT job_id FROM learning_course_jobs \
             WHERE user_id = ? AND kb_id = ? AND status NOT IN ('completed', 'failed', 'cancelled') \
             ORDER BY created_at ASC LIMIT 1",
        )
        .bind(user_id.as_str())
        .bind(request.knowledge_base_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        if let Some(active_job) = active_job {
            return Err(AppError::Conflict(format!(
                "knowledge base {} already has an active course-generation job ({active_job}); \
                 wait for it to complete or cancel it before starting another",
                request.knowledge_base_id
            )));
        }
        let job_id = generate_id();
        let now = now_ms();
        let request_json = serde_json::to_string(&request).map_err(internal)?;
        sqlx::query(
            "INSERT INTO learning_course_jobs \
             (job_id, user_id, session_id, source, kb_id, request_json, status, generation_mode, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, 'queued', ?, ?, ?)",
        )
        .bind(&job_id)
        .bind(user_id.as_str())
        .bind(session_id)
        .bind(source.as_str())
        .bind(request.knowledge_base_id.as_str())
        .bind(&request_json)
        .bind(request.mode.as_str())
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

    /// Version of the course with the given title, if one exists. The
    /// tutorial seed uses this to decide between importing (absent), skipping
    /// (same version) and replacing (stale version) the preset course.
    pub(crate) async fn course_version_by_title(&self, title: &str) -> Result<Option<i64>, AppError> {
        let version: Option<i64> =
            sqlx::query_scalar("SELECT version FROM learning_courses WHERE title = ?")
                .bind(title)
                .fetch_optional(&self.pool)
                .await
                .map_err(internal)?;
        Ok(version)
    }

    /// Deletes every course with the given title and all of its data —
    /// enrollments, progress, attempts, reviews, concepts, lessons and
    /// activities — regardless of which user created them. Used by the
    /// tutorial seed to replace a stale preset course version.
    pub(crate) async fn delete_courses_by_title(&self, title: &str) -> Result<(), AppError> {
        let course_ids: Vec<String> =
            sqlx::query_scalar("SELECT course_id FROM learning_courses WHERE title = ?")
                .bind(title)
                .fetch_all(&self.pool)
                .await
                .map_err(internal)?;
        if course_ids.is_empty() {
            return Ok(());
        }
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        for course_id in &course_ids {
            let enrollment_ids: Vec<String> = sqlx::query_scalar(
                "SELECT enrollment_id FROM learning_enrollments WHERE course_id = ?",
            )
            .bind(course_id)
            .fetch_all(&mut *transaction)
            .await
            .map_err(internal)?;
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
            sqlx::query("DELETE FROM learning_enrollments WHERE course_id = ?")
                .bind(course_id)
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
                    .bind(course_id)
                    .execute(&mut *transaction)
                    .await
                    .map_err(internal)?;
            }
            sqlx::query("DELETE FROM learning_course_tags WHERE course_id = ?")
                .bind(course_id)
                .execute(&mut *transaction)
                .await
                .map_err(internal)?;
            sqlx::query("DELETE FROM learning_courses WHERE course_id = ?")
                .bind(course_id)
                .execute(&mut *transaction)
                .await
                .map_err(internal)?;
        }
        transaction.commit().await.map_err(internal)?;
        Ok(())
    }

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

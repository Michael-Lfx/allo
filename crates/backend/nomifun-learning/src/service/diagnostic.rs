use super::*;

impl LearningService {

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
    pub(super) async fn ensure_enrollment(
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

}

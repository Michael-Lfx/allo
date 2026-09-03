use super::*;

impl LearningService {

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
    pub(super) async fn course_tags(&self, course_id: &LearningCourseId) -> Result<Vec<String>, AppError> {
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
    pub(super) async fn question_tags_for(
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

}

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

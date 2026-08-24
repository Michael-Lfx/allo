use super::*;

impl LearningService {

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
                    r.activity_id, a.kind, a.prompt, a.config_json, \
                    l.title AS lesson_title, m.title AS module_title, \
                    (SELECT ac.concept_id FROM learning_activity_concepts ac \
                     WHERE ac.activity_id = r.activity_id \
                     ORDER BY ac.concept_id LIMIT 1) AS concept_id, \
                    (SELECT lc.title FROM learning_activity_concepts ac \
                     JOIN learning_concepts lc ON lc.concept_id = ac.concept_id \
                     WHERE ac.activity_id = r.activity_id \
                     ORDER BY ac.concept_id LIMIT 1) AS concept_title, \
                    r.due_at, r.stability_days, r.difficulty, r.review_count, r.lapse_count, \
                    r.edit_pending_at, r.edit_note \
             FROM learning_review_items r \
             JOIN learning_enrollments e ON e.enrollment_id = r.enrollment_id \
             LEFT JOIN learning_courses c ON c.course_id = e.course_id \
             JOIN learning_activities a ON a.activity_id = r.activity_id \
             JOIN learning_lessons l ON l.lesson_id = a.lesson_id \
             JOIN learning_modules m ON m.module_id = l.module_id \
             WHERE e.user_id = ? AND r.archived_at IS NULL AND r.edit_pending_at IS NULL";
        // Course reviews: scoped by one or more courses (all queued, or due
        // only when requested) and/or by tag names attached to the question.
        // A pure orphan queue skips course reviews entirely.
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
                    " AND EXISTS (SELECT 1 FROM learning_question_tags qt \
                     JOIN learning_tags lt ON lt.tag_id = qt.tag_id \
                     WHERE qt.question_id = r.activity_id AND qt.source = 'course' \
                     AND lt.name IN ({placeholders}))"
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
        // Each row is one review item = one question card: the item's own
        // activity is the card's question, so every question (including
        // fill-in-the-blank) joins the queue with its own schedule.
        let mut reviews = Vec::new();
        for row in rows {
            let review_id: LearningReviewItemId =
                parse_id(row.try_get("review_item_id").map_err(internal)?)?;
            let enrollment_id: LearningEnrollmentId =
                parse_id(row.try_get("enrollment_id").map_err(internal)?)?;
            let activity_id: LearningActivityId =
                parse_id(row.try_get("activity_id").map_err(internal)?)?;
            let kind_text: String = row.try_get("kind").map_err(internal)?;
            let config: StoredActivityConfig = serde_json::from_str(
                &row.try_get::<String, _>("config_json").map_err(internal)?,
            )
            .map_err(internal)?;
            let course_id: Option<String> = row.try_get("course_id").map_err(internal)?;
            let concept_id: Option<String> = row.try_get("concept_id").map_err(internal)?;
            reviews.push(DueReview {
                id: review_id,
                source: ReviewSource::Course,
                enrollment_id: Some(enrollment_id),
                course_id: match &course_id {
                    Some(value) => Some(parse_id(value.clone())?),
                    None => None,
                },
                course_title: row.try_get("course_title").map_err(internal)?,
                module_title: Some(row.try_get("module_title").map_err(internal)?),
                lesson_title: Some(row.try_get("lesson_title").map_err(internal)?),
                concept_id: match &concept_id {
                    Some(value) => Some(parse_id(value.clone())?),
                    None => None,
                },
                concept_title: row.try_get("concept_title").map_err(internal)?,
                question: ReviewQuestion {
                    activity_id: Some(activity_id),
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
                edit_pending: row
                    .try_get::<Option<i64>, _>("edit_pending_at")
                    .map_err(internal)?
                    .is_some(),
                edit_note: row.try_get("edit_note").map_err(internal)?,
            });
        }
        // Learner-authored custom questions carry their own schedule and join
        // the same queue without any course context. They are excluded when
        // specific courses are selected without the orphan flag; the orphan
        // filter alone or together with courses adds exactly those questions.
        if orphan || course_ids.is_empty() {
            let mut sql = String::from(
                "SELECT q.custom_question_id, q.kind, q.prompt, q.config_json, q.due_at, \
                        q.stability_days, q.difficulty, q.review_count, q.lapse_count, \
                        q.edit_pending_at, q.edit_note \
                 FROM learning_custom_questions q \
                 WHERE q.user_id = ? AND q.due_at <= ? AND q.archived_at IS NULL AND q.edit_pending_at IS NULL",
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
                    edit_pending: row
                        .try_get::<Option<i64>, _>("edit_pending_at")
                        .map_err(internal)?
                        .is_some(),
                    edit_note: row.try_get("edit_note").map_err(internal)?,
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
        // activity already has a review item: the review queue only admits
        // questions seeded by their completed lesson, so showing a
        // due/scheduled state here would make the counts disagree with the
        // queue.
        let base = "SELECT a.activity_id, a.kind, a.prompt, a.config_json, \
                           ac.concept_id, lc.title AS concept_title, \
                           e.course_id, c.title AS course_title, \
                           p.status AS lesson_status, \
                           ri.review_item_id, ri.due_at, ri.stability_days, ri.difficulty, \
                           ri.review_count, ri.lapse_count, ri.last_reviewed_at, ri.updated_at, \
                           ri.archived_at, ri.edit_pending_at, ri.edit_note \
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
                      ON ri.enrollment_id = e.enrollment_id AND ri.activity_id = a.activity_id \
                    WHERE a.kind IN ('single_choice', 'true_false', 'fill_in_blank')";
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
            let archived = row
                .try_get::<Option<i64>, _>("archived_at")
                .map_err(internal)?
                .is_some();
            let edit_pending = row
                .try_get::<Option<i64>, _>("edit_pending_at")
                .map_err(internal)?
                .is_some();
            let entry_state = if archived {
                "archived"
            } else if review_item_id.is_none() || !lesson_completed {
                "unlearned"
            } else if review_count == 0 {
                "new"
            } else if due_at.is_some_and(|value| value <= now) {
                "due"
            } else {
                "scheduled"
            };
            // "edit_pending" is a cross-cutting filter: keep any state as long
            // as the card carries a pending edit intent.
            if let Some(filter) = &state {
                if *filter == "edit_pending" {
                    if !edit_pending {
                        continue;
                    }
                } else if *filter != entry_state {
                    continue;
                }
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
                edit_pending: row
                    .try_get::<Option<i64>, _>("edit_pending_at")
                    .map_err(internal)?
                    .is_some(),
                edit_note: row.try_get("edit_note").map_err(internal)?,
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
                        q.review_count, q.lapse_count, q.last_reviewed_at, q.updated_at, \
                        q.archived_at, q.edit_pending_at, q.edit_note \
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
                let archived = row
                    .try_get::<Option<i64>, _>("archived_at")
                    .map_err(internal)?
                    .is_some();
                let edit_pending = row
                    .try_get::<Option<i64>, _>("edit_pending_at")
                    .map_err(internal)?
                    .is_some();
                let entry_state = if archived {
                    "archived"
                } else if review_count == 0 {
                    "new"
                } else if due_at <= now {
                    "due"
                } else {
                    "scheduled"
                };
                // "edit_pending" is a cross-cutting filter: keep any state as
                // long as the card carries a pending edit intent.
                if let Some(filter) = &state {
                    if *filter == "edit_pending" {
                        if !edit_pending {
                            continue;
                        }
                    } else if *filter != entry_state {
                        continue;
                    }
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
                    edit_pending: row
                        .try_get::<Option<i64>, _>("edit_pending_at")
                        .map_err(internal)?
                        .is_some(),
                    edit_note: row.try_get("edit_note").map_err(internal)?,
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
        // Saving the edit means the pending intent is fulfilled: clear the
        // "edit me later" flag on every review item behind this activity.
        sqlx::query(
            "UPDATE learning_review_items SET edit_pending_at = NULL, edit_note = NULL \
             WHERE activity_id = ? \
             AND enrollment_id IN (SELECT enrollment_id FROM learning_enrollments WHERE user_id = ?)",
        )
        .bind(activity_id.as_str())
        .bind(user_id.as_str())
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

    /// Archives a course review item: the FSRS data stays intact but the card
    /// leaves the review queue and due counts until unarchived.
    pub async fn archive_review_item(
        &self,
        review_id: &LearningReviewItemId,
        user_id: &UserId,
    ) -> Result<(), AppError> {
        self.set_review_item_archived(review_id, user_id, true).await
    }

    /// Brings an archived course review item back into the queue. Its due
    /// time is untouched, so a card archived while overdue resurfaces
    /// immediately after unarchiving.
    pub async fn unarchive_review_item(
        &self,
        review_id: &LearningReviewItemId,
        user_id: &UserId,
    ) -> Result<(), AppError> {
        self.set_review_item_archived(review_id, user_id, false).await
    }

    async fn set_review_item_archived(
        &self,
        review_id: &LearningReviewItemId,
        user_id: &UserId,
        archived: bool,
    ) -> Result<(), AppError> {
        let now = now_ms();
        let result = sqlx::query(
            "UPDATE learning_review_items SET archived_at = ?, updated_at = ? \
             WHERE review_item_id = ? \
             AND enrollment_id IN (SELECT enrollment_id FROM learning_enrollments WHERE user_id = ?)",
        )
        .bind(if archived { Some(now) } else { None })
        .bind(now)
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

    /// Marks a course review card as "edit me later" with an optional note;
    /// the schedule is untouched so the review flow is not interrupted.
    /// A blank note is stored as NULL.
    pub async fn mark_review_edit_pending(
        &self,
        review_id: &LearningReviewItemId,
        user_id: &UserId,
        note: Option<String>,
    ) -> Result<(), AppError> {
        let result = sqlx::query(
            "UPDATE learning_review_items SET edit_pending_at = ?, edit_note = ?, updated_at = ? \
             WHERE review_item_id = ? \
             AND enrollment_id IN (SELECT enrollment_id FROM learning_enrollments WHERE user_id = ?)",
        )
        .bind(now_ms())
        .bind(note.filter(|value| !value.trim().is_empty()))
        .bind(now_ms())
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

    /// Marks a learner-authored question as "edit me later"; see
    /// `mark_review_edit_pending` for semantics.
    pub async fn mark_custom_edit_pending(
        &self,
        question_id: &str,
        user_id: &UserId,
        note: Option<String>,
    ) -> Result<(), AppError> {
        let result = sqlx::query(
            "UPDATE learning_custom_questions \
             SET edit_pending_at = ?, edit_note = ?, updated_at = ? \
             WHERE custom_question_id = ? AND user_id = ?",
        )
        .bind(now_ms())
        .bind(note.filter(|value| !value.trim().is_empty()))
        .bind(now_ms())
        .bind(question_id)
        .bind(user_id.as_str())
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("custom question {question_id}")));
        }
        Ok(())
    }

    /// Full question entry behind a course review item, including the stored
    /// answer. Used to open the shared edit dialog from the review session;
    /// mirrors the state computation of `question_entries`.
    pub async fn review_question_entry(
        &self,
        review_id: &LearningReviewItemId,
        user_id: &UserId,
    ) -> Result<QuestionEntry, AppError> {
        let row = sqlx::query(
            "SELECT a.activity_id, a.kind, a.prompt, a.config_json, \
                    e.course_id, c.title AS course_title, \
                    (SELECT ac.concept_id FROM learning_activity_concepts ac \
                     WHERE ac.activity_id = a.activity_id \
                     ORDER BY ac.concept_id LIMIT 1) AS concept_id, \
                    (SELECT lc.title FROM learning_activity_concepts ac \
                     JOIN learning_concepts lc ON lc.concept_id = ac.concept_id \
                     WHERE ac.activity_id = a.activity_id \
                     ORDER BY ac.concept_id LIMIT 1) AS concept_title, \
                    p.status AS lesson_status, \
                    r.review_item_id, r.due_at, r.stability_days, r.difficulty, \
                    r.review_count, r.lapse_count, r.last_reviewed_at, r.updated_at, r.archived_at, \
                    r.edit_pending_at, r.edit_note \
             FROM learning_review_items r \
             JOIN learning_enrollments e ON e.enrollment_id = r.enrollment_id \
             LEFT JOIN learning_courses c ON c.course_id = e.course_id \
             JOIN learning_activities a ON a.activity_id = r.activity_id \
             LEFT JOIN learning_lesson_progress p \
               ON p.lesson_id = a.lesson_id AND p.enrollment_id = e.enrollment_id \
             WHERE r.review_item_id = ? AND e.user_id = ?",
        )
        .bind(review_id.as_str())
        .bind(user_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("review item {review_id}")))?;
        let kind_text: String = row.try_get("kind").map_err(internal)?;
        let config: StoredActivityConfig = serde_json::from_str(
            &row.try_get::<String, _>("config_json").map_err(internal)?,
        )
        .map_err(internal)?;
        let review_item_id: String = row.try_get("review_item_id").map_err(internal)?;
        let due_at: Option<i64> = row.try_get("due_at").map_err(internal)?;
        let review_count: i64 = row.try_get("review_count").map_err(internal)?;
        let lesson_completed = row
            .try_get::<Option<String>, _>("lesson_status")
            .map_err(internal)?
            .as_deref()
            == Some("completed");
        let archived = row
            .try_get::<Option<i64>, _>("archived_at")
            .map_err(internal)?
            .is_some();
        let state = if archived {
            "archived"
        } else if !lesson_completed {
            "unlearned"
        } else if review_count == 0 {
            "new"
        } else if due_at.is_some_and(|value| value <= now_ms()) {
            "due"
        } else {
            "scheduled"
        };
        let course_id: Option<String> = row.try_get("course_id").map_err(internal)?;
        let concept_id: Option<String> = row.try_get("concept_id").map_err(internal)?;
        Ok(QuestionEntry {
            source: ReviewSource::Course,
            question_id: row.try_get("activity_id").map_err(internal)?,
            review_item_id: Some(parse_id(review_item_id)?),
            state: state.to_string(),
            course_id: match course_id {
                Some(value) => Some(parse_id(value)?),
                None => None,
            },
            course_title: row.try_get("course_title").map_err(internal)?,
            concept_id: match concept_id {
                Some(value) => Some(parse_id(value)?),
                None => None,
            },
            concept_title: row.try_get("concept_title").map_err(internal)?,
            question_kind: Some(
                ActivityKind::try_from(kind_text.as_str())
                    .map_err(|message| AppError::BadRequest(message))?,
            ),
            prompt: Some(row.try_get("prompt").map_err(internal)?),
            options: config.options.clone(),
            answer: Some(config.answer.clone()),
            distractors: config.distractors.clone(),
            explanation: Some(config.explanation.clone()),
            due_at,
            overdue: due_at.is_some_and(|value| value <= now_ms()),
            stability_days: row.try_get("stability_days").map_err(internal)?,
            difficulty: row.try_get("difficulty").map_err(internal)?,
            review_count,
            lapse_count: row.try_get("lapse_count").map_err(internal)?,
            last_reviewed_at: row.try_get("last_reviewed_at").map_err(internal)?,
            updated_at: row.try_get("updated_at").map_err(internal)?,
            tags: Vec::new(),
            edit_pending: row
                .try_get::<Option<i64>, _>("edit_pending_at")
                .map_err(internal)?
                .is_some(),
            edit_note: row.try_get("edit_note").map_err(internal)?,
        })
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
        let due_at = first_review_due_at(now, self.tz_offset_minutes().await);
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
        .bind(due_at)
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
            "UPDATE learning_custom_questions SET prompt = ?, config_json = ?, \
             edit_pending_at = NULL, edit_note = NULL, updated_at = ? \
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

    /// Archives a learner-authored question: the card leaves the review queue
    /// and due counts but keeps its FSRS schedule and tag links until
    /// unarchived.
    pub async fn archive_custom_question(
        &self,
        question_id: &str,
        user_id: &UserId,
    ) -> Result<(), AppError> {
        self.set_custom_question_archived(question_id, user_id, true)
            .await
    }

    /// Brings an archived learner-authored question back into the queue.
    pub async fn unarchive_custom_question(
        &self,
        question_id: &str,
        user_id: &UserId,
    ) -> Result<(), AppError> {
        self.set_custom_question_archived(question_id, user_id, false)
            .await
    }

    async fn set_custom_question_archived(
        &self,
        question_id: &str,
        user_id: &UserId,
        archived: bool,
    ) -> Result<(), AppError> {
        let now = now_ms();
        let result = sqlx::query(
            "UPDATE learning_custom_questions SET archived_at = ?, updated_at = ? \
             WHERE custom_question_id = ? AND user_id = ?",
        )
        .bind(if archived { Some(now) } else { None })
        .bind(now)
        .bind(question_id)
        .bind(user_id.as_str())
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("custom question {question_id}")));
        }
        Ok(())
    }

    /// Full entry of a learner-authored question, including the stored
    /// answer. Used to open the shared edit dialog from the review session.
    pub async fn custom_question_entry(
        &self,
        question_id: &str,
        user_id: &UserId,
    ) -> Result<QuestionEntry, AppError> {
        let row = sqlx::query(
            "SELECT q.custom_question_id, q.kind, q.prompt, q.config_json, q.concept_id, \
                    lc.title AS concept_title, q.due_at, q.stability_days, q.difficulty, \
                    q.review_count, q.lapse_count, q.last_reviewed_at, q.updated_at, \
                    q.archived_at, q.edit_pending_at, q.edit_note \
             FROM learning_custom_questions q \
             LEFT JOIN learning_concepts lc ON lc.concept_id = q.concept_id \
             WHERE q.custom_question_id = ? AND q.user_id = ?",
        )
        .bind(question_id)
        .bind(user_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("custom question {question_id}")))?;
        let kind_text: String = row.try_get("kind").map_err(internal)?;
        let config: StoredActivityConfig = serde_json::from_str(
            &row.try_get::<String, _>("config_json").map_err(internal)?,
        )
        .map_err(internal)?;
        let due_at: i64 = row.try_get("due_at").map_err(internal)?;
        let review_count: i64 = row.try_get("review_count").map_err(internal)?;
        let archived = row
            .try_get::<Option<i64>, _>("archived_at")
            .map_err(internal)?
            .is_some();
        let state = if archived {
            "archived"
        } else if review_count == 0 {
            "new"
        } else if due_at <= now_ms() {
            "due"
        } else {
            "scheduled"
        };
        let concept_id: Option<String> = row.try_get("concept_id").map_err(internal)?;
        Ok(QuestionEntry {
            source: ReviewSource::Custom,
            question_id: row.try_get("custom_question_id").map_err(internal)?,
            review_item_id: None,
            state: state.to_string(),
            course_id: None,
            course_title: None,
            concept_id: match concept_id {
                Some(value) => Some(parse_id(value)?),
                None => None,
            },
            concept_title: row.try_get("concept_title").map_err(internal)?,
            question_kind: Some(
                ActivityKind::try_from(kind_text.as_str())
                    .map_err(|message| AppError::BadRequest(message))?,
            ),
            prompt: Some(row.try_get("prompt").map_err(internal)?),
            options: config.options.clone(),
            answer: Some(config.answer.clone()),
            distractors: config.distractors.clone(),
            explanation: Some(config.explanation.clone()),
            due_at: Some(due_at),
            overdue: due_at <= now_ms(),
            stability_days: row.try_get("stability_days").map_err(internal)?,
            difficulty: row.try_get("difficulty").map_err(internal)?,
            review_count,
            lapse_count: row.try_get("lapse_count").map_err(internal)?,
            last_reviewed_at: row.try_get("last_reviewed_at").map_err(internal)?,
            updated_at: row.try_get("updated_at").map_err(internal)?,
            tags: Vec::new(),
            edit_pending: row
                .try_get::<Option<i64>, _>("edit_pending_at")
                .map_err(internal)?
                .is_some(),
            edit_note: row.try_get("edit_note").map_err(internal)?,
        })
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
                 JOIN learning_activity_concepts ac ON ac.activity_id = r.activity_id \
                 WHERE ac.concept_id = lc.concept_id AND e.user_id = ? \
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
        // Answered reviews (including auto-rated lapses) count toward the
        // daily check-in regardless of the outcome.
        sqlx::query(
            "INSERT INTO learning_review_events (event_id, user_id, source, item_id, created_at) \
             VALUES (?, ?, 'custom', ?, ?)",
        )
        .bind(generate_id())
        .bind(user_id.as_str())
        .bind(question_id)
        .bind(now_ms())
        .execute(&self.pool)
        .await
        .map_err(internal)?;
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

    pub async fn rate_review(
        &self,
        review_id: &LearningReviewItemId,
        user_id: &UserId,
        rating: ReviewRating,
    ) -> Result<ReviewResult, AppError> {
        let row = sqlx::query(
            "SELECT r.enrollment_id, r.activity_id, r.stability_days, r.difficulty, \
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
        let activity_id: LearningActivityId =
            parse_id(row.try_get("activity_id").map_err(internal)?)?;
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
        update_activity_mastery(&mut transaction, &enrollment_id, &activity_id, score, now)
            .await?;
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

    /// Answers the question attached to a due review. Each item carries its
    /// own activity, so the question is loaded straight from the item. A wrong
    /// answer (or an admitted lapse via `forgot`) is immediately rated `again`
    /// (scheduling + mastery updated); a correct answer only records the
    /// attempt and waits for a self-rating via `rate_review`.
    pub async fn answer_review(
        &self,
        review_id: &LearningReviewItemId,
        user_id: &UserId,
        response: Value,
        forgot: bool,
    ) -> Result<ReviewAnswerResult, AppError> {
        let row = sqlx::query(
            "SELECT r.enrollment_id, r.activity_id, a.kind, a.config_json \
             FROM learning_review_items r \
             JOIN learning_enrollments e ON e.enrollment_id = r.enrollment_id \
             JOIN learning_activities a ON a.activity_id = r.activity_id \
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
        let activity_id: LearningActivityId =
            parse_id(row.try_get("activity_id").map_err(internal)?)?;
        let kind_text: String = row.try_get("kind").map_err(internal)?;
        let kind = ActivityKind::try_from(kind_text.as_str()).map_err(AppError::Internal)?;
        let config: StoredActivityConfig = serde_json::from_str(
            &row.try_get::<String, _>("config_json").map_err(internal)?,
        )
        .map_err(internal)?;
        // `forgot` skips grading entirely: learners must never be forced to
        // guess, so the lapse is recorded with the revealed answer instead.
        let (score, feedback, correct) = if forgot {
            let feedback = if config.explanation.is_empty() {
                "Review the source material before retrieving this question again.".to_string()
            } else {
                config.explanation.clone()
            };
            (0.0, feedback, false)
        } else {
            let (score, feedback) = evaluate(kind, &config, &response)?;
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
        .bind(activity_id.as_str())
        .bind(serde_json::to_string(if forgot { &Value::Null } else { &response }).map_err(internal)?)
        .bind(score)
        .bind(correct)
        .bind(&feedback)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(internal)?;
        // Every answered review counts toward the daily check-in, so the
        // event lands in the same transaction as the attempt.
        sqlx::query(
            "INSERT INTO learning_review_events (event_id, user_id, source, item_id, created_at) \
             VALUES (?, ?, 'course', ?, ?)",
        )
        .bind(generate_id())
        .bind(user_id.as_str())
        .bind(review_id.as_str())
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(internal)?;
        let rated = if correct {
            None
        } else {
            update_mastery_and_review(
                &mut transaction,
                review_id,
                &enrollment_id,
                &activity_id,
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
                Some(config.answer.clone())
            },
            rated,
        })
    }

}

/// Skipping a due review defers it by a full day without rating it.
const SKIP_DELAY_MS: i64 = 86_400_000;


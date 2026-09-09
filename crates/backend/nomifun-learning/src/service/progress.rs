use super::*;

impl LearningService {

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
            // 跳过不伪造开始时间也不写 completed_at（CHECK 要求 skipped 的
            // completed_at 为 NULL）：保留已有的 in_progress 历史即可。
            LessonStatus::Skipped => (existing_started, None),
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
            // seed one item per concept due on the next review day (idempotent).
            let enrollment = parse_id::<LearningEnrollmentId>(enrollment_id.clone())?;
            let mut transaction = self.pool.begin().await.map_err(internal)?;
            let tz_offset_minutes = self.tz_offset_minutes().await;
            seed_lesson_review_items(
                &mut transaction,
                &enrollment,
                lesson_id,
                now,
                tz_offset_minutes,
            )
            .await?;
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
        // AI-graded answers (reflection 0-1, open_question 0-10) are graded
        // by the LLM: the activity's linked concepts ground the grading
        // prompt. AI grading is authoritative — the empty-answer rejection
        // is enforced here by the rule-based evaluator, and any grading
        // failure (unconfigured completer, call error, unparseable reply)
        // surfaces as an error to the learner. The old silent fallback
        // passed every non-empty answer with a score of 1.0, hiding grading
        // failures behind a fake success.
        let (score, feedback) = if kind.is_ai_graded() {
            let answer = response.as_str().map(str::trim).unwrap_or_default();
            if answer.is_empty() {
                // Always rejected: the evaluator errors on empty responses.
                evaluate(kind, &config, &response)?
            } else {
                let linked_concepts =
                    activity_concept_titles(&self.pool, activity_id).await?;
                self.grade_open_answer(
                    kind,
                    &activity_prompt,
                    answer,
                    &linked_concepts,
                    provider_id.as_ref(),
                    model.as_deref(),
                )
                .await?
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

    /// LLM-grades an open answer (reflection 0-1, open_question 0-10) against
    /// the exercise's concepts. The model sees the exercise prompt, the
    /// learner's answer and the linked concepts; it must reply with strict
    /// JSON `{ "score": f64, "feedback": string }`. open_question scores are
    /// reported on a 0-10 scale and normalized to 0-1 here (≥6 passes).
    /// Every failure (no completer, call error, unparseable reply) returns
    /// `Err` and is surfaced to the learner — AI grading is authoritative,
    /// so a broken grader must never masquerade as a passing answer.
    async fn grade_open_answer(
        &self,
        kind: ActivityKind,
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
                AppError::Conflict("AI answer grading is not configured".into())
            })?;
        let system = if kind == ActivityKind::OpenQuestion {
            OPEN_QUESTION_GRADING_SYSTEM
        } else {
            REFLECTION_GRADING_SYSTEM
        };
        let user = build_open_grading_prompt(kind, prompt, answer, linked_concepts);
        let raw = completer
            .complete(
                provider_id.zip(model).map(|(id, model)| (id.as_str(), model)),
                system,
                &user,
                crate::generation::REFLECTION_GRADING_MAX_TOKENS,
            )
            .await?;
        let (score, feedback) = parse_open_grading(&raw)?;
        // open_question arrives on a 0-10 scale; normalize to 0-1 so
        // mastery/attempt storage stays on the shared scale (0.6 = pass).
        let normalized = if kind == ActivityKind::OpenQuestion {
            score / 10.0
        } else {
            score
        };
        Ok((normalized.clamp(0.0, 1.0), feedback))
    }

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

/// System prompt for AI open-question grading: a comprehensive-application
/// question scored on a 0-10 scale (≥6 passes), graded on full-lesson
/// coverage rather than a single concept.
const OPEN_QUESTION_GRADING_SYSTEM: &str = r#"You are a strict but encouraging learning coach grading a learner's open-ended answer to a comprehensive course question.

Score the answer from 0.0 to 10.0 (6.0 is passing):
- Correctness: are the claims aligned with the concepts this question targets?
- Completeness: does the answer assemble the key points into a working whole?
- Application: does it show the learner can use the ideas, not just recite them?

Reply with ONLY one JSON object matching this shape:
{
  "score": 6.5,
  "feedback": "markdown text"
}
Rules:
- score must be a number between 0.0 and 10.0.
- feedback must be Markdown with two parts: (1) 逐点批改 — point-by-point evaluation against the expected key points, (2) 改进建议 — concrete improvement suggestions.
- Write the feedback in the same language as the learner's answer.
- Output JSON only, without Markdown fences or commentary."#;

/// Builds the user message for AI answer grading: the exercise prompt, the
/// learner's answer and the concepts the exercise targets (its own lesson's
/// concepts — open questions never bind concepts of other lessons).
fn build_open_grading_prompt(
    kind: ActivityKind,
    prompt: &str,
    answer: &str,
    linked_concepts: &[(String, String, String)],
) -> String {
    let linked = linked_concepts
        .iter()
        .map(|(_, title, description)| format!("- {title}: {description}"))
        .collect::<Vec<_>>()
        .join("\n");
    let framing = if kind == ActivityKind::OpenQuestion {
        "Comprehensive question covering the whole lesson (score on the 0-10 scale):"
    } else {
        "Exercise prompt:"
    };
    format!(
        "{framing}\n{prompt}\n\nLearner's answer:\n{answer}\n\nConcepts this exercise targets:\n{linked}"
    )
}

/// Parses the strict-JSON grading reply into `(score, feedback)`. Extraction
/// reuses the generation parser so the habitual model mistakes — Markdown
/// fences, prose around the object, escaping errors, trailing commas — do
/// not silently drop the learner onto rule-based grading. Any shape
/// deviation returns `Err` so the caller degrades to rule-based grading.
fn parse_open_grading(raw: &str) -> Result<(f64, String), AppError> {
    #[derive(serde::Deserialize)]
    struct GradingReply {
        score: f64,
        feedback: String,
    }
    let reply: GradingReply = crate::generation::parse_json_object(raw).map_err(|error| {
        AppError::Internal(format!("unparseable answer grading reply: {error}"))
    })?;
    Ok((reply.score, reply.feedback))
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

pub(super) fn evaluate(
    kind: ActivityKind,
    config: &StoredActivityConfig,
    response: &Value,
) -> Result<(f64, String), AppError> {
    // AI-graded kinds never enter this rule-based evaluator with a
    // meaningful score: reflection is graded by `grade_reflection`, and the
    // empty-response rejection for both AI-graded kinds is enforced here.
    if kind.is_ai_graded() {
        let correct = response
            .as_str()
            .is_some_and(|value| !value.trim().is_empty());
        if !correct {
            return Err(AppError::BadRequest(format!(
                "{} response must not be empty",
                kind.as_str()
            )));
        }
        // Unreachable in production: callers route AI-graded kinds to the
        // completer first and only fall back here for the empty check.
        return Ok((1.0, config.explanation.clone()));
    }
    let correct = match kind {
        ActivityKind::SingleChoice => response.as_str() == config.answer.as_str(),
        ActivityKind::TrueFalse => response.as_bool() == config.answer.as_bool(),
        ActivityKind::Reflection | ActivityKind::OpenQuestion => unreachable!(
            "AI-graded kinds are handled above"
        ),
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
        // Order-insensitive selection: every picked option must be part of
        // the stored answer and every stored answer picked (exact set match).
        ActivityKind::MultiChoice => {
            let Some(picked) = response.as_array() else {
                return Err(AppError::BadRequest(
                    "multi_choice response must be an array of options".into(),
                ));
            };
            let Some(expected) = config.answer.as_array() else {
                return Err(AppError::Internal(
                    "multi_choice activity has a malformed stored answer".into(),
                ));
            };
            let normalize = |values: &[Value]| -> std::collections::HashSet<String> {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(|text| text.trim().to_owned()))
                    .collect()
            };
            normalize(picked) == normalize(expected)
        }
        // |response − answer| ≤ tol (tiny epsilon when tol is absent so
        // decimal answers are not punished for float representation).
        ActivityKind::Numeric => {
            let Some(value) = response.as_f64() else {
                return Err(AppError::BadRequest(
                    "numeric response must be a number".into(),
                ));
            };
            let Some(expected) = config.answer.as_f64() else {
                return Err(AppError::Internal(
                    "numeric activity has a malformed stored answer".into(),
                ));
            };
            let tol = config.tol.unwrap_or(1e-9).max(1e-9);
            (value - expected).abs() <= tol
        }
        // Sequence equality over the presented items (the UI shuffles
        // `options` for display; the response is the learner's order).
        ActivityKind::Ordering => {
            let Some(sequence) = response.as_array() else {
                return Err(AppError::BadRequest(
                    "ordering response must be an array of items in order".into(),
                ));
            };
            let Some(expected) = config.answer.as_array() else {
                return Err(AppError::Internal(
                    "ordering activity has a malformed stored answer".into(),
                ));
            };
            sequence.len() == expected.len()
                && sequence
                    .iter()
                    .zip(expected.iter())
                    .all(|(got, want)| got.as_str() == want.as_str())
        }
        // Right-column values aligned with the left column (options).
        ActivityKind::Matching => {
            let Some(alignment) = response.as_array() else {
                return Err(AppError::BadRequest(
                    "matching response must be an array aligned with the left column".into(),
                ));
            };
            let Some(expected) = config.answer.as_array() else {
                return Err(AppError::Internal(
                    "matching activity has a malformed stored answer".into(),
                ));
            };
            alignment.len() == expected.len()
                && alignment
                    .iter()
                    .zip(expected.iter())
                    .all(|(got, want)| got.as_str() == want.as_str())
        }
    };
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

/// Feeds a review outcome into the mastery of every concept the activity is
/// bound to, mirroring in-course attempts.
pub(super) async fn update_activity_mastery(
    transaction: &mut Transaction<'_, Sqlite>,
    enrollment_id: &LearningEnrollmentId,
    activity_id: &LearningActivityId,
    score: f64,
    now: i64,
) -> Result<(), AppError> {
    let concept_ids: Vec<String> = sqlx::query_scalar(
        "SELECT concept_id FROM learning_activity_concepts WHERE activity_id = ?",
    )
    .bind(activity_id.as_str())
    .fetch_all(&mut **transaction)
    .await
    .map_err(internal)?;
    for concept_id in concept_ids {
        update_mastery(transaction, enrollment_id, &concept_id, score, now).await?;
    }
    Ok(())
}

/// Rates a review item `again` on a wrong/forgotten answer: reschedules the
/// item and feeds the score into its activity's concepts. Items only exist
/// after their lesson was completed, so the row is guaranteed to be present.
pub(super) async fn update_mastery_and_review(
    transaction: &mut Transaction<'_, Sqlite>,
    review_id: &LearningReviewItemId,
    enrollment_id: &LearningEnrollmentId,
    activity_id: &LearningActivityId,
    score: f64,
    rating: ReviewRating,
    now: i64,
    settings: &SchedulerSettings,
) -> Result<(), AppError> {
    update_activity_mastery(transaction, enrollment_id, activity_id, score, now).await?;
    let current = sqlx::query(
        "SELECT stability_days, difficulty, review_count, lapse_count, last_reviewed_at \
         FROM learning_review_items WHERE review_item_id = ?",
    )
    .bind(review_id.as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(internal)?;
    let next = schedule_review(
        now,
        current.try_get("stability_days").map_err(internal)?,
        current.try_get("difficulty").map_err(internal)?,
        current.try_get("review_count").map_err(internal)?,
        current.try_get("lapse_count").map_err(internal)?,
        current.try_get("last_reviewed_at").map_err(internal)?,
        rating,
        settings,
    )?;
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

/// Creates one review item per objective question of a lesson when the learner
/// completes it, due on the next review day. Each question carries its own
/// memory curve. Existing items keep their schedule untouched.
async fn seed_lesson_review_items(
    transaction: &mut Transaction<'_, Sqlite>,
    enrollment_id: &LearningEnrollmentId,
    lesson_id: &LearningLessonId,
    now: i64,
    tz_offset_minutes: i32,
) -> Result<(), AppError> {
    let activity_ids: Vec<String> = sqlx::query_scalar(
        "SELECT activity_id FROM learning_activities \
         WHERE lesson_id = ? AND kind IN ('single_choice', 'true_false', 'fill_in_blank', \
           'multi_choice', 'numeric', 'ordering', 'matching') \
         ORDER BY position, activity_id",
    )
    .bind(lesson_id.as_str())
    .fetch_all(&mut **transaction)
    .await
    .map_err(internal)?;
    for activity_id in activity_ids {
        ensure_review_item(transaction, enrollment_id, &activity_id, now, tz_offset_minutes).await?;
    }
    Ok(())
}

/// Creates the initial review item for one objective question, due at the
/// start of the next review day (02:00 local). Existing items keep their
/// schedule untouched: in-course attempts never reschedule, only the review
/// queue does.
pub(super) async fn ensure_review_item(
    transaction: &mut Transaction<'_, Sqlite>,
    enrollment_id: &LearningEnrollmentId,
    activity_id: &str,
    now: i64,
    tz_offset_minutes: i32,
) -> Result<(), AppError> {
    let exists: Option<String> = sqlx::query_scalar(
        "SELECT review_item_id FROM learning_review_items \
         WHERE enrollment_id = ? AND activity_id = ?",
    )
    .bind(enrollment_id.as_str())
    .bind(activity_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(internal)?;
    if exists.is_some() {
        return Ok(());
    }
    let review_item_id = LearningReviewItemId::new().into_string();
    sqlx::query(
        "INSERT INTO learning_review_items \
         (review_item_id, enrollment_id, activity_id, due_at, stability_days, difficulty, \
          review_count, lapse_count, last_reviewed_at, updated_at) \
         VALUES (?, ?, ?, ?, 0, 5.0, 0, 0, NULL, ?)",
    )
    .bind(&review_item_id)
    .bind(enrollment_id.as_str())
    .bind(activity_id)
    .bind(first_review_due_at(now, tz_offset_minutes))
    .bind(now)
    .execute(&mut **transaction)
    .await
    .map_err(internal)?;
    // Synthetic marker row: the card exists from this moment, but seeding
    // is not an answer — it never counts as a push, and memory statistics
    // and optimizer training exclude it (review log rating_source rules).
    let user_id: String = sqlx::query_scalar(
        "SELECT user_id FROM learning_enrollments WHERE enrollment_id = ?",
    )
    .bind(enrollment_id.as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(internal)?;
    sqlx::query(
        "INSERT INTO learning_review_log \
         (log_id, user_id, source, item_id, rating, rating_source, elapsed_days, \
          stability_before, difficulty_before, r_pred, review_day, created_at) \
         VALUES (?, ?, 'course', ?, 0, 'synthetic', 0, NULL, NULL, NULL, ?, ?)",
    )
    .bind(generate_id())
    .bind(&user_id)
    .bind(&review_item_id)
    .bind(review_day_number(now, tz_offset_minutes))
    .bind(now)
    .execute(&mut **transaction)
    .await
    .map_err(internal)?;
    Ok(())
}

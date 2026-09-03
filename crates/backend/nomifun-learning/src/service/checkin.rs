use super::*;

impl LearningService {

    /// Loads user-tunable scheduler knobs from client preferences, falling
    /// back to FSRS defaults for anything missing or malformed.
    pub(super) async fn scheduler_settings(&self) -> SchedulerSettings {
        let mut settings = SchedulerSettings::default();
        settings.tz_offset_minutes = self.tz_offset_minutes().await;
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

    /// Local-time offset in minutes east of UTC, reported by the frontend
    /// (`learning.tzOffsetMinutes`); falls back to the server's local offset
    /// when the preference is missing or malformed.
    pub(super) async fn tz_offset_minutes(&self) -> i32 {
        let Ok(row) = sqlx::query_scalar::<_, String>(
            "SELECT value FROM client_preferences WHERE key = 'learning.tzOffsetMinutes'",
        )
        .fetch_optional(&self.pool)
        .await
        else {
            return SchedulerSettings::default().tz_offset_minutes;
        };
        match row {
            Some(value) => serde_json::from_str::<Value>(&value)
                .ok()
                .and_then(|parsed| parsed.as_i64())
                .and_then(|minutes| i32::try_from(minutes).ok())
                .filter(|minutes| (-24 * 60..=24 * 60).contains(minutes))
                .unwrap_or_else(|| SchedulerSettings::default().tz_offset_minutes),
            None => SchedulerSettings::default().tz_offset_minutes,
        }
    }

    /// Daily review goal from client preferences (`learning.dailyCheckinGoal`),
    /// defaulting to 15; 0 means "clear the queue only" with no count target.
    async fn daily_checkin_goal(&self) -> i64 {
        let Ok(row) = sqlx::query_scalar::<_, String>(
            "SELECT value FROM client_preferences WHERE key = 'learning.dailyCheckinGoal'",
        )
        .fetch_optional(&self.pool)
        .await
        else {
            return 15;
        };
        match row {
            Some(value) => serde_json::from_str::<Value>(&value)
                .ok()
                .and_then(|parsed| parsed.as_i64())
                .filter(|goal| (0..=10_000).contains(goal))
                .unwrap_or(15),
            None => 15,
        }
    }

    /// Daily check-in status for the current review day. Completion is a
    /// momentary snapshot: goal reached (when N > 0), or the queue cleared
    /// after at least one review today — an empty queue with zero reviews is
    /// just the initial state and never completes the day. Once satisfied the
    /// day is locked in `learning_checkins`, so cards arriving later keep
    /// showing in the queue without changing the check-in state.
    pub async fn checkin_today(&self, user_id: &UserId) -> Result<CheckinStatus, AppError> {
        let now = now_ms();
        let tz_offset_minutes = self.tz_offset_minutes().await;
        let review_day_start = review_day_start_utc(now, tz_offset_minutes);
        let review_day = review_day_number(now, tz_offset_minutes);

        let goal = self.daily_checkin_goal().await;
        let reviewed_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM learning_review_events WHERE user_id = ? AND created_at >= ?",
        )
        .bind(user_id.as_str())
        .bind(review_day_start)
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        let due_count: i64 = sqlx::query_scalar(
            "SELECT \
                (SELECT COUNT(*) FROM learning_review_items r \
                 JOIN learning_enrollments e ON e.enrollment_id = r.enrollment_id \
                 WHERE e.user_id = ? AND r.due_at <= ? AND r.archived_at IS NULL AND r.edit_pending_at IS NULL) \
              + (SELECT COUNT(*) FROM learning_custom_questions q \
                 WHERE q.user_id = ? AND q.due_at <= ? AND q.archived_at IS NULL AND q.edit_pending_at IS NULL)",
        )
        .bind(user_id.as_str())
        .bind(now)
        .bind(user_id.as_str())
        .bind(now)
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;

        let locked: Option<(i64, i64)> = sqlx::query_as(
            "SELECT reviewed_count, completed_at FROM learning_checkins \
             WHERE user_id = ? AND review_day = ?",
        )
        .bind(user_id.as_str())
        .bind(review_day)
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;

        let (completed, locked_at) = if let Some((_, completed_at)) = locked {
            (true, Some(completed_at))
        } else if (goal > 0 && reviewed_count >= goal)
            || (reviewed_count > 0 && due_count == 0)
        {
            // Lock the review day; the UNIQUE(user_id, review_day) constraint
            // keeps concurrent evaluations idempotent.
            sqlx::query(
                "INSERT OR IGNORE INTO learning_checkins \
                 (checkin_id, user_id, review_day, goal, reviewed_count, completed_at) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(generate_id())
            .bind(user_id.as_str())
            .bind(review_day)
            .bind(goal)
            .bind(reviewed_count)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
            let completed_at: i64 = sqlx::query_scalar(
                "SELECT completed_at FROM learning_checkins WHERE user_id = ? AND review_day = ?",
            )
            .bind(user_id.as_str())
            .bind(review_day)
            .fetch_one(&self.pool)
            .await
            .map_err(internal)?;
            (true, Some(completed_at))
        } else {
            (false, None)
        };

        Ok(CheckinStatus {
            review_day,
            goal,
            reviewed_count,
            due_count,
            completed,
            locked_at,
        })
    }

    /// Calendar aggregation for the learning page: review-day bucketed
    /// activity (review counts, check-in completion, completed lessons and
    /// created courses) plus the current streak. Every day of the requested
    /// range is present, zero-filled when the user had no activity, so the
    /// frontend can render a full grid without padding. The tz offset comes
    /// from the request, not from client preferences, so a timezone change
    /// takes effect immediately.
    pub async fn calendar_stats(
        &self,
        user_id: &UserId,
        tz_offset_minutes: i32,
        year: i64,
        month: Option<u32>,
    ) -> Result<CalendarStats, AppError> {
        let year = i32::try_from(year)
            .map_err(|_| AppError::BadRequest(format!("year out of range: {year}")))?;
        let (range_start, range_end) = match month {
            Some(m) if (1..=12).contains(&m) => (
                local_wall_clock_utc_ms(year, m, 1, 2, tz_offset_minutes).ok_or_else(|| {
                    AppError::BadRequest(format!("invalid month range: {year}-{m}"))
                })?,
                local_wall_clock_utc_ms(
                    if m == 12 { year + 1 } else { year },
                    if m == 12 { 1 } else { m + 1 },
                    1,
                    2,
                    tz_offset_minutes,
                )
                .ok_or_else(|| {
                    AppError::BadRequest(format!("invalid month range: {year}-{m}"))
                })?,
            ),
            Some(m) => {
                return Err(AppError::BadRequest(format!("month out of range: {m}")));
            }
            None => (
                local_wall_clock_utc_ms(year, 1, 1, 2, tz_offset_minutes)
                    .ok_or_else(|| AppError::BadRequest("invalid year".into()))?,
                local_wall_clock_utc_ms(year + 1, 1, 1, 2, tz_offset_minutes)
                    .ok_or_else(|| AppError::BadRequest("invalid year".into()))?,
            ),
        };

        // Review events bucketed by review day (02:00 rollover).
        let mut reviewed: HashMap<i64, i64> = HashMap::new();
        {
            let rows = sqlx::query(
                "SELECT created_at FROM learning_review_events \
                 WHERE user_id = ? AND created_at >= ? AND created_at < ?",
            )
            .bind(user_id.as_str())
            .bind(range_start)
            .bind(range_end)
            .fetch_all(&self.pool)
            .await
            .map_err(internal)?;
            for row in rows {
                let created_at: i64 = row.try_get("created_at").map_err(internal)?;
                *reviewed
                    .entry(review_day_number(created_at, tz_offset_minutes))
                    .or_insert(0) += 1;
            }
        }

        // Completed check-in days within the year (review_day is YYYYMMDD).
        let checkins: HashSet<i64> = sqlx::query_scalar(
            "SELECT review_day FROM learning_checkins \
             WHERE user_id = ? AND review_day >= ? AND review_day <= ?",
        )
        .bind(user_id.as_str())
        .bind(i64::from(year) * 10_000 + 101)
        .bind(i64::from(year) * 10_000 + 1231)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?
        .into_iter()
        .collect();

        // Lessons the user completed, bucketed by review day.
        let mut completed_lessons: HashMap<i64, Vec<CalendarLessonRef>> = HashMap::new();
        {
            let rows = sqlx::query(
                "SELECT lp.completed_at, l.lesson_id, l.title \
                 FROM learning_lesson_progress lp \
                 JOIN learning_enrollments e ON e.enrollment_id = lp.enrollment_id \
                 JOIN learning_lessons l ON l.lesson_id = lp.lesson_id \
                 WHERE e.user_id = ? AND lp.status = 'completed' \
                   AND lp.completed_at >= ? AND lp.completed_at < ?",
            )
            .bind(user_id.as_str())
            .bind(range_start)
            .bind(range_end)
            .fetch_all(&self.pool)
            .await
            .map_err(internal)?;
            for row in rows {
                let completed_at: i64 = row.try_get("completed_at").map_err(internal)?;
                completed_lessons
                    .entry(review_day_number(completed_at, tz_offset_minutes))
                    .or_default()
                    .push(CalendarLessonRef {
                        lesson_id: row.try_get("lesson_id").map_err(internal)?,
                        title: row.try_get("title").map_err(internal)?,
                    });
            }
        }

        // Courses created in range (global catalog, not per user).
        let mut created_courses: HashMap<i64, Vec<CalendarCourseRef>> = HashMap::new();
        {
            let rows = sqlx::query(
                "SELECT course_id, title, created_at FROM learning_courses \
                 WHERE created_at >= ? AND created_at < ?",
            )
            .bind(range_start)
            .bind(range_end)
            .fetch_all(&self.pool)
            .await
            .map_err(internal)?;
            for row in rows {
                let created_at: i64 = row.try_get("created_at").map_err(internal)?;
                created_courses
                    .entry(review_day_number(created_at, tz_offset_minutes))
                    .or_default()
                    .push(CalendarCourseRef {
                        course_id: row.try_get("course_id").map_err(internal)?,
                        title: row.try_get("title").map_err(internal)?,
                    });
            }
        }

        let current_review_day = review_day_number(now_ms(), tz_offset_minutes);
        let streak = self.streak_days(user_id, current_review_day).await?;

        // Due cards bucketed by review day: a card due before the current
        // review day rolls into today (matching the review banner's due
        // queue); future days show cards scheduled to come due that day.
        let mut due_counts: HashMap<i64, i64> = HashMap::new();
        {
            let rows = sqlx::query(
                "SELECT r.due_at AS due_at FROM learning_review_items r \
                 JOIN learning_enrollments e ON e.enrollment_id = r.enrollment_id \
                 WHERE e.user_id = ? AND r.due_at < ? AND r.archived_at IS NULL AND r.edit_pending_at IS NULL \
                 UNION ALL \
                 SELECT q.due_at AS due_at FROM learning_custom_questions q \
                 WHERE q.user_id = ? AND q.due_at < ? AND q.archived_at IS NULL AND q.edit_pending_at IS NULL",
            )
            .bind(user_id.as_str())
            .bind(range_end)
            .bind(user_id.as_str())
            .bind(range_end)
            .fetch_all(&self.pool)
            .await
            .map_err(internal)?;
            for row in rows {
                let due_at: i64 = row.try_get("due_at").map_err(internal)?;
                let day = review_day_number(due_at, tz_offset_minutes).max(current_review_day);
                *due_counts.entry(day).or_insert(0) += 1;
            }
        }

        // Zero-filled day range: every local calendar day of the request.
        let mut cursor = chrono::NaiveDate::from_ymd_opt(year, 1, 1)
            .ok_or_else(|| AppError::BadRequest("invalid year".into()))?;
        if let Some(m) = month {
            cursor = chrono::NaiveDate::from_ymd_opt(year, m, 1).ok_or_else(|| {
                AppError::BadRequest(format!("invalid month range: {year}-{m}"))
            })?;
        }
        let end = if month.is_some() && month != Some(12) {
            chrono::NaiveDate::from_ymd_opt(year, month.unwrap() + 1, 1)
        } else {
            chrono::NaiveDate::from_ymd_opt(year + 1, 1, 1)
        }
        .ok_or_else(|| AppError::BadRequest("invalid range".into()))?;
        let mut days = Vec::new();
        while cursor < end {
            let review_day =
                i64::from(cursor.year()) * 10_000 + i64::from(cursor.month()) * 100
                    + i64::from(cursor.day());
            days.push(CalendarDayStats {
                review_day,
                reviewed_count: reviewed.get(&review_day).copied().unwrap_or(0),
                checkin_completed: checkins.contains(&review_day),
                due_count: due_counts.get(&review_day).copied().unwrap_or(0),
                completed_lessons: completed_lessons.remove(&review_day).unwrap_or_default(),
                created_courses: created_courses.remove(&review_day).unwrap_or_default(),
            });
            cursor += chrono::Duration::days(1);
        }

        Ok(CalendarStats {
            year: i64::from(year),
            month: month.map(i64::from),
            tz_offset: tz_offset_minutes,
            streak,
            days,
        })
    }

    /// Consecutive completed check-in days ending at `current_review_day`.
    /// The current day must itself be completed for the streak to continue;
    /// a gap anywhere stops the count. Zero-review days never lock, so every
    /// streak day contains real review activity.
    async fn streak_days(&self, user_id: &UserId, current_review_day: i64) -> Result<i64, AppError> {
        let days: HashSet<i64> = sqlx::query_scalar(
            "SELECT review_day FROM learning_checkins WHERE user_id = ?",
        )
        .bind(user_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?
        .into_iter()
        .collect();
        let mut streak = 0;
        let mut day = current_review_day;
        while days.contains(&day) {
            streak += 1;
            let Some(previous) = previous_review_day(day) else {
                break;
            };
            day = previous;
        }
        Ok(streak)
    }

}

/// Local wall-clock moment (year/month/day/hour) as UTC milliseconds for the
/// given tz offset in minutes east of UTC. Returns None for out-of-range
/// dates (e.g. year 0 or month 13).
pub(super) fn local_wall_clock_utc_ms(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    tz_offset_minutes: i32,
) -> Option<i64> {
    let naive = chrono::NaiveDate::from_ymd_opt(year, month, day)?.and_hms_opt(hour, 0, 0)?;
    Some(
        naive.and_utc().timestamp_millis() - i64::from(tz_offset_minutes) * 60_000,
    )
}

/// Review day number (YYYYMMDD) of the day before the given one, using real
/// date arithmetic so month and year boundaries stay correct.
fn previous_review_day(review_day: i64) -> Option<i64> {
    let year = (review_day / 10_000) as i32;
    let month = ((review_day / 100) % 100) as u32;
    let day = (review_day % 100) as u32;
    let previous = chrono::NaiveDate::from_ymd_opt(year, month, day)?.pred_opt()?;
    Some(
        i64::from(previous.year()) * 10_000 + i64::from(previous.month()) * 100
            + i64::from(previous.day()),
    )
}

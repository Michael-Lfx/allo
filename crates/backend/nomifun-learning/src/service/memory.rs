use super::*;

/// Aggregation window for the log-derived panels: pushes older than 90
/// review days stop feeding True Retention, calibration and the curve.
const LOG_WINDOW_DAYS: i64 = 90;
const DAY_MS: i64 = 86_400_000;

/// Anchor of an elapsed-days bin on the forgetting curve.
fn curve_anchor(elapsed_days: i64) -> i64 {
    match elapsed_days {
        0..=2 => elapsed_days,
        3..=4 => 3,
        5..=6 => 5,
        7..=13 => 7,
        14..=29 => 14,
        _ => 30,
    }
}

/// One counted push: FSRS rating, elapsed days and the recall prediction it
/// is calibrated against.
struct CountedPush {
    rating: i64,
    elapsed_days: i64,
    r_pred: f64,
}

/// Aggregation skeleton over counted pushes: sample count, mean prediction
/// and the actual pass share (rating >= 2 means pass; a lapse or an admitted
/// forgot means fail).
struct BinAggregator {
    count: i64,
    predicted_sum: f64,
    passes: i64,
}

impl BinAggregator {
    fn new() -> Self {
        Self {
            count: 0,
            predicted_sum: 0.0,
            passes: 0,
        }
    }

    fn push(&mut self, push: &CountedPush) {
        self.count += 1;
        self.predicted_sum += push.r_pred;
        if push.rating >= 2 {
            self.passes += 1;
        }
    }

    fn predicted(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.predicted_sum / self.count as f64
        }
    }

    fn actual(&self) -> Option<f64> {
        if self.count == 0 {
            None
        } else {
            Some(self.passes as f64 / self.count as f64)
        }
    }
}

impl LearningService {
    /// Memory-health snapshot over the active card pool and the review log,
    /// all in review-day semantics (02:00 rollover, matching the scheduler).
    ///
    /// True Retention / calibration / forgetting curve read the review log
    /// with the honesty filters: only real pushes (`auto`/`self`, synthetic
    /// seeding excluded) of cards that actually carried a memory state
    /// (`stability_before`/`r_pred` present — the first push of a fresh card
    /// is not a due review of an old card), and per card per review day only
    /// the first push counts (defensive normalization for the relearning
    /// pushes the due-ness gate allows). Empty aggregates stay empty instead
    /// of fabricating data.
    pub async fn memory_health_stats(
        &self,
        user_id: &UserId,
        tz_offset_minutes: i32,
    ) -> Result<MemoryHealthStats, AppError> {
        let now = now_ms();
        let today = review_day_number(now, tz_offset_minutes);

        // Panel 1 — load forecast: due moments of every active card,
        // course + custom.
        let mut due_moments: Vec<i64> = sqlx::query_scalar(
            "SELECT r.due_at FROM learning_review_items r \
             JOIN learning_enrollments e ON e.enrollment_id = r.enrollment_id \
             WHERE e.user_id = ? AND r.archived_at IS NULL AND r.edit_pending_at IS NULL",
        )
        .bind(user_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let custom_due: Vec<i64> = sqlx::query_scalar(
            "SELECT q.due_at FROM learning_custom_questions q \
             WHERE q.user_id = ? AND q.archived_at IS NULL AND q.edit_pending_at IS NULL",
        )
        .bind(user_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        due_moments.extend(custom_due);
        let overdue_count = due_moments.iter().filter(|due_at| **due_at <= now).count() as i64;
        let mut load_forecast: Vec<MemoryLoadDay> = Vec::new();
        for offset in 0..=6i64 {
            let day = review_day_number(now + offset * DAY_MS, tz_offset_minutes);
            let due_count = due_moments
                .iter()
                .filter(|due_at| review_day_number(**due_at, tz_offset_minutes) == day)
                .count() as i64;
            load_forecast.push(MemoryLoadDay {
                review_day: day,
                due_count,
            });
        }

        // Panel 2 — state distribution over the active pool, course + custom.
        let mut state_counts: HashMap<String, i64> = HashMap::new();
        for sql in [
            "SELECT CASE \
              WHEN r.review_count = 0 THEN 'new' \
              WHEN r.stability_days < 7 THEN 'young' \
              WHEN r.stability_days < 30 THEN 'mature' \
              ELSE 'master' END AS key, COUNT(*) \
             FROM learning_review_items r \
             JOIN learning_enrollments e ON e.enrollment_id = r.enrollment_id \
             WHERE e.user_id = ? AND r.archived_at IS NULL AND r.edit_pending_at IS NULL \
             GROUP BY key",
            "SELECT CASE \
              WHEN q.review_count = 0 THEN 'new' \
              WHEN q.stability_days < 7 THEN 'young' \
              WHEN q.stability_days < 30 THEN 'mature' \
              ELSE 'master' END AS key, COUNT(*) \
             FROM learning_custom_questions q \
             WHERE q.user_id = ? AND q.archived_at IS NULL AND q.edit_pending_at IS NULL \
             GROUP BY key",
        ] {
            let rows: Vec<(String, i64)> = sqlx::query_as(sql)
                .bind(user_id.as_str())
                .fetch_all(&self.pool)
                .await
                .map_err(internal)?;
            for (key, count) in rows {
                *state_counts.entry(key).or_insert(0) += count;
            }
        }
        let state_distribution = ["new", "young", "mature", "master"]
            .into_iter()
            .map(|key| MemoryStateBucket {
                key: key.to_string(),
                count: state_counts.get(key).copied().unwrap_or(0),
            })
            .collect();

        // Panels 3 + 4 — from the review log with the honesty filters.
        let window_start = review_day_number(now - LOG_WINDOW_DAYS * DAY_MS, tz_offset_minutes);
        let rows: Vec<(String, String, i64, i64, i64, f64)> = sqlx::query_as(
            "SELECT source, item_id, review_day, rating, elapsed_days, r_pred \
             FROM learning_review_log \
             WHERE user_id = ? AND rating_source IN ('auto', 'self') \
             AND stability_before IS NOT NULL AND r_pred IS NOT NULL AND review_day >= ? \
             ORDER BY created_at, log_id",
        )
        .bind(user_id.as_str())
        .bind(window_start)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        // First push per card per review day wins; later same-day pushes
        // (relearning steps) are dropped from the honesty statistics.
        let mut seen: HashSet<(String, String, i64)> = HashSet::new();
        let mut counted: Vec<CountedPush> = Vec::new();
        for (source, item_id, review_day, rating, elapsed_days, r_pred) in rows {
            if !seen.insert((source, item_id, review_day)) {
                continue;
            }
            counted.push(CountedPush {
                rating,
                elapsed_days,
                r_pred,
            });
        }

        let total = counted.len() as i64;
        let passes = counted.iter().filter(|push| push.rating >= 2).count() as i64;
        let true_retention = if total == 0 {
            None
        } else {
            Some(MemoryTrueRetention {
                passes,
                fails: total - passes,
                rate: Some(passes as f64 / total as f64),
            })
        };

        let mut calibration_map: HashMap<i64, BinAggregator> = HashMap::new();
        let mut curve_map: HashMap<i64, BinAggregator> = HashMap::new();
        for push in &counted {
            let bucket = recall_bucket(push.r_pred).min(19);
            calibration_map.entry(bucket).or_insert_with(BinAggregator::new).push(push);
            curve_map
                .entry(curve_anchor(push.elapsed_days))
                .or_insert_with(BinAggregator::new)
                .push(push);
        }
        let mut calibration: Vec<MemoryCalibrationBin> = calibration_map
            .into_iter()
            .map(|(bucket, aggregator)| MemoryCalibrationBin {
                bucket,
                min: bucket as f64 / 20.0,
                max: (bucket + 1) as f64 / 20.0,
                predicted: aggregator.predicted(),
                actual: aggregator.actual(),
                count: aggregator.count,
            })
            .collect();
        calibration.sort_by_key(|bin| bin.bucket);
        let mut forgetting_curve: Vec<MemoryCurvePoint> = curve_map
            .into_iter()
            .map(|(elapsed_days, aggregator)| MemoryCurvePoint {
                elapsed_days,
                predicted: aggregator.predicted(),
                actual: aggregator.actual(),
                count: aggregator.count,
            })
            .collect();
        forgetting_curve.sort_by_key(|point| point.elapsed_days);

        Ok(MemoryHealthStats {
            review_day: today,
            tz_offset: tz_offset_minutes,
            overdue_count,
            load_forecast,
            state_distribution,
            true_retention,
            calibration,
            forgetting_curve,
        })
    }
}

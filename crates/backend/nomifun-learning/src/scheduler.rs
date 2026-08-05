use fsrs::{FSRS, MemoryState};
use nomifun_common::{AppError, TimestampMs};

use crate::models::ReviewRating;

const DAY_MS: i64 = 86_400_000;
/// Sub-day relearning steps still need a perceptible gap before the item
/// resurfaces; never schedule closer than one minute.
const MIN_RELEARN_DELAY_MS: i64 = 60 * 1000;

/// User-tunable FSRS knobs, loaded from client preferences.
#[derive(Debug, Clone, PartialEq)]
pub struct SchedulerSettings {
    /// Target recall probability at review time (FSRS desired retention).
    pub desired_retention: f32,
    /// Custom FSRS model weights; empty means the library defaults.
    pub parameters: Vec<f32>,
}

impl Default for SchedulerSettings {
    fn default() -> Self {
        Self {
            desired_retention: 0.9,
            parameters: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScheduleState {
    pub due_at: TimestampMs,
    pub stability_days: f64,
    pub difficulty: f64,
    pub review_count: i64,
    pub lapse_count: i64,
}

/// Schedules the next review of a concept using the FSRS algorithm.
///
/// `stability_days`/`difficulty` feed FSRS's memory state for items already
/// reviewed (`review_count > 0`); unseen items start from `None`.
/// `last_reviewed_at` supplies the elapsed-days input FSRS needs to predict
/// recall. A lapse (`Again`) additionally increments `lapse_count`.
pub fn schedule_review(
    now: TimestampMs,
    stability_days: f64,
    difficulty: f64,
    review_count: i64,
    lapse_count: i64,
    last_reviewed_at: Option<TimestampMs>,
    rating: ReviewRating,
    settings: &SchedulerSettings,
) -> Result<ScheduleState, AppError> {
    let model = match FSRS::new(&settings.parameters) {
        Ok(model) => model,
        Err(_) => FSRS::default(),
    };
    let previous_state = if review_count > 0 {
        Some(MemoryState {
            stability: stability_days as f32,
            difficulty: difficulty as f32,
        })
    } else {
        None
    };
    let days_elapsed = match last_reviewed_at {
        Some(last) if review_count > 0 => ((now.saturating_sub(last)).max(0) / DAY_MS) as u32,
        _ => 0,
    };
    let states = model
        .next_states(previous_state, settings.desired_retention, days_elapsed)
        .map_err(|err| AppError::Internal(format!("fsrs scheduling failed: {err}")))?;
    let (state, lapse_delta) = match rating {
        ReviewRating::Again => (states.again, 1),
        ReviewRating::Hard => (states.hard, 0),
        ReviewRating::Good => (states.good, 0),
        ReviewRating::Easy => (states.easy, 0),
    };

    // Intervals of a day or more land on whole-day boundaries; shorter
    // relearning steps keep their sub-day precision in milliseconds.
    let delay_ms = if state.interval >= 1.0 {
        i64::from(state.interval.round() as u32).saturating_mul(DAY_MS)
    } else {
        ((f64::from(state.interval) * DAY_MS as f64).round() as i64).max(MIN_RELEARN_DELAY_MS)
    };

    Ok(ScheduleState {
        due_at: now.saturating_add(delay_ms),
        stability_days: f64::from(state.memory.stability),
        difficulty: f64::from(state.memory.difficulty),
        review_count: review_count.saturating_add(1),
        lapse_count: lapse_count.saturating_add(lapse_delta),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn again_is_short_and_increments_lapses() {
        let settings = SchedulerSettings::default();
        let now = 5 * DAY_MS;
        let next = schedule_review(
            now,
            4.0,
            5.0,
            2,
            1,
            Some(now - DAY_MS),
            ReviewRating::Again,
            &settings,
        )
        .expect("again schedules");
        assert_eq!(next.review_count, 3);
        assert_eq!(next.lapse_count, 2);
        // A lapse drops the item back into sub-day relearning.
        assert!(next.due_at < now + DAY_MS);
        assert!(next.due_at > now);
        assert!(next.stability_days < 4.0);
    }

    #[test]
    fn first_review_seeds_new_memory() {
        let settings = SchedulerSettings::default();
        let next = schedule_review(0, 0.0, 5.0, 0, 0, None, ReviewRating::Good, &settings)
            .expect("first review schedules");
        assert_eq!(next.review_count, 1);
        assert_eq!(next.lapse_count, 0);
        assert!(next.stability_days > 0.0);
        assert!(next.due_at >= DAY_MS);
    }

    #[test]
    fn stronger_recall_produces_longer_intervals() {
        let settings = SchedulerSettings::default();
        let now = 10 * DAY_MS;
        let last = now - DAY_MS;
        let hard = schedule_review(now, 2.0, 5.0, 3, 0, Some(last), ReviewRating::Hard, &settings)
            .expect("hard schedules");
        let good = schedule_review(now, 2.0, 5.0, 3, 0, Some(last), ReviewRating::Good, &settings)
            .expect("good schedules");
        let easy = schedule_review(now, 2.0, 5.0, 3, 0, Some(last), ReviewRating::Easy, &settings)
            .expect("easy schedules");
        assert!(hard.due_at < good.due_at);
        assert!(good.due_at < easy.due_at);
        assert!(hard.difficulty > good.difficulty);
        assert!(good.difficulty > easy.difficulty);
    }

    #[test]
    fn custom_desired_retention_shifts_intervals() {
        let now = 10 * DAY_MS;
        let last = now - DAY_MS;
        let relaxed = SchedulerSettings {
            desired_retention: 0.8,
            ..SchedulerSettings::default()
        };
        let strict = SchedulerSettings {
            desired_retention: 0.95,
            ..SchedulerSettings::default()
        };
        let relaxed_next =
            schedule_review(now, 2.0, 5.0, 3, 0, Some(last), ReviewRating::Good, &relaxed)
                .expect("relaxed schedules");
        let strict_next =
            schedule_review(now, 2.0, 5.0, 3, 0, Some(last), ReviewRating::Good, &strict)
                .expect("strict schedules");
        // Higher retention targets review sooner.
        assert!(strict_next.due_at < relaxed_next.due_at);
    }
}

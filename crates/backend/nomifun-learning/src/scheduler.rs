use chrono::Datelike;
use fsrs::{FSRS, MemoryState};
use nomifun_common::{AppError, TimestampMs};

use crate::models::ReviewRating;

const DAY_MS: i64 = 86_400_000;
const HOUR_MS: i64 = 3_600_000;
/// Sub-day relearning steps still need a perceptible gap before the item
/// resurfaces; never schedule closer than one minute.
const MIN_RELEARN_DELAY_MS: i64 = 60 * 1000;
/// Day-level intervals roll over to a fixed local wall-clock moment so every
/// item due on the same review day shares one due time. 02:00 treats
/// late-night sessions (00:00-01:59) as the previous review day, matching
/// the common "2 AM is the next day" convention for night-owl learners.
const ROLLOVER_LOCAL_HOUR: i64 = 2;

/// User-tunable FSRS knobs, loaded from client preferences.
#[derive(Debug, Clone, PartialEq)]
pub struct SchedulerSettings {
    /// Target recall probability at review time (FSRS desired retention).
    pub desired_retention: f32,
    /// Custom FSRS model weights; empty means the library defaults.
    pub parameters: Vec<f32>,
    /// Fixed local-time offset in minutes east of UTC, reported by the
    /// frontend; falls back to the server's local offset when absent.
    pub tz_offset_minutes: i32,
}

impl Default for SchedulerSettings {
    fn default() -> Self {
        Self {
            desired_retention: 0.9,
            parameters: Vec::new(),
            tz_offset_minutes: chrono::Local::now().offset().local_minus_utc() / 60,
        }
    }
}

/// Start of the review day containing `now`: the local 02:00 wall-clock of
/// that day expressed as UTC milliseconds. Moments before 02:00 belong to
/// the previous review day, so 00:00-01:59 counts as the day before.
pub fn review_day_start_utc(now: TimestampMs, tz_offset_minutes: i32) -> TimestampMs {
    let offset_ms = i64::from(tz_offset_minutes) * 60_000;
    let local = now + offset_ms;
    let local_day = local.div_euclid(DAY_MS);
    let day_start = local_day * DAY_MS + ROLLOVER_LOCAL_HOUR * HOUR_MS - offset_ms;
    if now < day_start {
        day_start - DAY_MS
    } else {
        day_start
    }
}

/// Due moment for a brand-new card: the next review day's 02:00. Cards
/// entered today are not due again today — the learner just read the lesson
/// and answered its exercises, so an immediate repeat adds little.
pub fn first_review_due_at(now: TimestampMs, tz_offset_minutes: i32) -> TimestampMs {
    review_day_start_utc(now, tz_offset_minutes) + DAY_MS
}

/// Local calendar day (YYYYMMDD) of the review day containing `now`, used as
/// the stable identity of a check-in day. Derived from the review-day start
/// plus the offset so it always names the local day, never the UTC one.
pub fn review_day_number(now: TimestampMs, tz_offset_minutes: i32) -> i64 {
    let local_ms = review_day_start_utc(now, tz_offset_minutes) + i64::from(tz_offset_minutes) * 60_000;
    let date = chrono::DateTime::from_timestamp_millis(local_ms)
        .expect("review day start always fits chrono range")
        .date_naive();
    i64::from(date.year()) * 10_000 + i64::from(date.month()) * 100 + i64::from(date.day())
}

/// Whole review days between two moments, feeding FSRS's elapsed-days input.
/// Day-granular by construction: both endpoints collapse to their review
/// day's 02:00, so the difference is always an exact multiple of a day and
/// carries no ±1 day wall-clock noise.
pub fn days_elapsed_between(last: TimestampMs, now: TimestampMs, tz_offset_minutes: i32) -> u32 {
    let start_last = review_day_start_utc(last, tz_offset_minutes);
    let start_now = review_day_start_utc(now, tz_offset_minutes);
    ((start_now - start_last).max(0) / DAY_MS) as u32
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
/// `last_reviewed_at` supplies the elapsed-review-days input FSRS needs to
/// predict recall. A lapse (`Again`) additionally increments `lapse_count`.
///
/// Day-level intervals (>= 1 day) land on the due review day's fixed 02:00
/// rollover moment; shorter relearning steps keep their exact sub-day timing
/// so forgotten items resurface quickly for reinforcement.
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
        Some(last) if review_count > 0 => {
            days_elapsed_between(last, now, settings.tz_offset_minutes)
        }
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

    // Intervals of a day or more land on the due review day's 02:00 so the
    // due time never drifts with the review moment; shorter relearning steps
    // keep their sub-day precision in milliseconds.
    let due_at = if state.interval >= 1.0 {
        let days = state.interval.round().max(1.0) as i64;
        review_day_start_utc(now, settings.tz_offset_minutes).saturating_add(days * DAY_MS)
    } else {
        now.saturating_add(
            ((f64::from(state.interval) * DAY_MS as f64).round() as i64)
                .max(MIN_RELEARN_DELAY_MS),
        )
    };

    Ok(ScheduleState {
        due_at,
        stability_days: f64::from(state.memory.stability),
        difficulty: f64::from(state.memory.difficulty),
        review_count: review_count.saturating_add(1),
        lapse_count: lapse_count.saturating_add(lapse_delta),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UTC+8 (Asia/Shanghai), the project's primary locale.
    const TZ_CN: i32 = 480;
    /// UTC 18:00 == local 02:00 in UTC+8, the rollover moment.
    const CN_02_00_UTC_HOUR: i64 = 18;

    fn settings(tz_offset_minutes: i32) -> SchedulerSettings {
        SchedulerSettings {
            desired_retention: 0.9,
            parameters: Vec::new(),
            tz_offset_minutes,
        }
    }

    #[test]
    fn review_day_start_treats_local_02_00_as_boundary() {
        // Local day-10 10:00 == UTC day-10 02:00.
        let now = 10 * DAY_MS + 2 * HOUR_MS;
        let start = review_day_start_utc(now, TZ_CN);
        // Local day-10 02:00 == UTC day-9 18:00.
        assert_eq!(start, 9 * DAY_MS + CN_02_00_UTC_HOUR * HOUR_MS);
        // One day and one hour later lands on the next review day's start.
        let next_minute = review_day_start_utc(now + DAY_MS + HOUR_MS, TZ_CN);
        assert_eq!(next_minute - start, DAY_MS);
    }

    #[test]
    fn late_night_belongs_to_previous_review_day() {
        // Local day-10 01:30 == UTC day-9 17:30: belongs to day-9's review
        // day, which started at local day-9 02:00 == UTC day-8 18:00.
        let now = 9 * DAY_MS + 17 * HOUR_MS + 30 * 60_000;
        let start = review_day_start_utc(now, TZ_CN);
        assert_eq!(start, 8 * DAY_MS + CN_02_00_UTC_HOUR * HOUR_MS);
    }

    #[test]
    fn first_review_due_is_next_review_day_02_00() {
        // Daytime entry (local day-10 10:00 == UTC day-10 02:00): due
        // tomorrow 02:00 local == UTC day-10 18:00.
        let day_entry = 10 * DAY_MS + 2 * HOUR_MS;
        assert_eq!(
            first_review_due_at(day_entry, TZ_CN),
            10 * DAY_MS + CN_02_00_UTC_HOUR * HOUR_MS
        );
        // Late-night entry (local day-10 01:30 == UTC day-9 17:30, previous
        // review day): due today 02:00 local == UTC day-9 18:00.
        let night_entry = 9 * DAY_MS + 17 * HOUR_MS + 30 * 60_000;
        assert_eq!(
            first_review_due_at(night_entry, TZ_CN),
            9 * DAY_MS + CN_02_00_UTC_HOUR * HOUR_MS
        );
    }

    #[test]
    fn days_elapsed_uses_review_days_not_wall_clock() {
        // Local day-9 23:50 (UTC day-9 15:50) -> local day-10 23:40 (UTC
        // day-10 15:40): 23h50m wall clock (0 whole days by naive division)
        // but exactly one review day apart.
        let last = 9 * DAY_MS + 15 * HOUR_MS + 50 * 60_000;
        let now = 10 * DAY_MS + 15 * HOUR_MS + 40 * 60_000;
        assert_eq!(days_elapsed_between(last, now, TZ_CN), 1);
        // Same review day -> 0.
        let same_day = now - 3 * HOUR_MS;
        assert_eq!(days_elapsed_between(same_day, now, TZ_CN), 0);
    }

    #[test]
    fn timezone_offset_changes_due_day() {
        // UTC day-10 17:00 == local day-11 01:00. UTC is already inside
        // review day 10 (started UTC day-10 02:00); CN still counts the
        // moment as review day 10 starting at UTC day-9 18:00.
        let instant = 10 * DAY_MS + 17 * HOUR_MS;
        let cn_start = review_day_start_utc(instant, TZ_CN);
        let utc_start = review_day_start_utc(instant, 0);
        assert_eq!(utc_start - cn_start, 8 * HOUR_MS);
    }

    #[test]
    fn review_day_number_names_local_day() {
        // Epoch day-11 00:00 (00:00-01:59 belongs to the previous review
        // day) is still review day 19700111; 02:00 the same morning rolls
        // over to 19700112.
        assert_eq!(review_day_number(11 * DAY_MS, 0), 19700111);
        assert_eq!(review_day_number(11 * DAY_MS + 2 * HOUR_MS, 0), 19700112);
        // CN (UTC+8): UTC day-10 18:00 == local day-11 02:00 starts review
        // day 19700112, while local 01:00 (UTC day-10 17:00) still belongs
        // to review day 19700111.
        assert_eq!(review_day_number(10 * DAY_MS + 17 * HOUR_MS, TZ_CN), 19700111);
        assert_eq!(review_day_number(10 * DAY_MS + 18 * HOUR_MS, TZ_CN), 19700112);
    }

    #[test]
    fn again_is_short_and_increments_lapses() {
        let settings = settings(0);
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
        let settings = settings(0);
        let next = schedule_review(0, 0.0, 5.0, 0, 0, None, ReviewRating::Good, &settings)
            .expect("first review schedules");
        assert_eq!(next.review_count, 1);
        assert_eq!(next.lapse_count, 0);
        assert!(next.stability_days > 0.0);
        // Epoch 00:00 UTC belongs to the previous review day, so even a
        // one-day interval lands at 02:00 UTC of the next day.
        assert!(next.due_at > 0);
        assert!(next.due_at < 2 * DAY_MS);
    }

    #[test]
    fn day_level_rolls_over_to_local_02_00() {
        let settings = settings(TZ_CN);
        // Local day-10 10:00 == UTC day-10 02:00.
        let now = 10 * DAY_MS + 2 * HOUR_MS;
        let next = schedule_review(
            now,
            2.0,
            5.0,
            3,
            0,
            Some(now - 2 * DAY_MS),
            ReviewRating::Good,
            &settings,
        )
        .expect("good schedules");
        // Due at local 02:00 (UTC 18:00) on a future review day, never
        // earlier than tomorrow 02:00. The upper bound is a loose sanity
        // check: FSRS may stretch a Good interval well past a week.
        assert_eq!(next.due_at % DAY_MS, CN_02_00_UTC_HOUR * HOUR_MS);
        let start = review_day_start_utc(now, TZ_CN);
        assert!(next.due_at >= start + DAY_MS);
        assert!(next.due_at < start + 90 * DAY_MS);
    }

    #[test]
    fn stronger_recall_produces_longer_intervals() {
        let settings = settings(0);
        let now = 10 * DAY_MS;
        let last = now - DAY_MS;
        let hard = schedule_review(now, 2.0, 5.0, 3, 0, Some(last), ReviewRating::Hard, &settings)
            .expect("hard schedules");
        let good = schedule_review(now, 2.0, 5.0, 3, 0, Some(last), ReviewRating::Good, &settings)
            .expect("good schedules");
        let easy = schedule_review(now, 2.0, 5.0, 3, 0, Some(last), ReviewRating::Easy, &settings)
            .expect("easy schedules");
        // Rounded day intervals may tie; the order must never invert.
        assert!(hard.due_at <= good.due_at);
        assert!(good.due_at <= easy.due_at);
        assert!(hard.difficulty > good.difficulty);
        assert!(good.difficulty > easy.difficulty);
    }

    #[test]
    fn custom_desired_retention_shifts_intervals() {
        let now = 10 * DAY_MS;
        let last = now - DAY_MS;
        let relaxed = settings(0);
        let strict = SchedulerSettings {
            desired_retention: 0.95,
            ..settings(0)
        };
        let relaxed_next =
            schedule_review(now, 2.0, 5.0, 3, 0, Some(last), ReviewRating::Good, &relaxed)
                .expect("relaxed schedules");
        let strict_next =
            schedule_review(now, 2.0, 5.0, 3, 0, Some(last), ReviewRating::Good, &strict)
                .expect("strict schedules");
        // Higher retention targets review sooner.
        assert!(strict_next.due_at <= relaxed_next.due_at);
    }
}

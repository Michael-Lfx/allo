use nomifun_common::TimestampMs;

use crate::models::ReviewRating;

const DAY_MS: i64 = 86_400_000;
const AGAIN_DELAY_MS: i64 = 10 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScheduleState {
    pub due_at: TimestampMs,
    pub stability_days: f64,
    pub difficulty: f64,
    pub review_count: i64,
    pub lapse_count: i64,
}

pub fn schedule_review(
    now: TimestampMs,
    stability_days: f64,
    difficulty: f64,
    review_count: i64,
    lapse_count: i64,
    rating: ReviewRating,
) -> ScheduleState {
    let (next_stability, difficulty_delta, delay_ms, lapse_delta) = match rating {
        ReviewRating::Again => (0.1, 1.0, AGAIN_DELAY_MS, 1),
        ReviewRating::Hard => {
            let stability = if stability_days < 0.5 {
                0.5
            } else {
                stability_days * 1.2
            };
            (stability, 0.3, days_to_ms(stability), 0)
        }
        ReviewRating::Good => {
            let stability = if stability_days < 1.0 {
                1.0
            } else {
                stability_days * 2.0
            };
            (stability, -0.2, days_to_ms(stability), 0)
        }
        ReviewRating::Easy => {
            let stability = if stability_days < 1.0 {
                3.0
            } else {
                stability_days * 3.5
            };
            (stability, -0.5, days_to_ms(stability), 0)
        }
    };

    ScheduleState {
        due_at: now.saturating_add(delay_ms),
        stability_days: next_stability,
        difficulty: (difficulty + difficulty_delta).clamp(1.0, 10.0),
        review_count: review_count.saturating_add(1),
        lapse_count: lapse_count.saturating_add(lapse_delta),
    }
}

fn days_to_ms(days: f64) -> i64 {
    (days * DAY_MS as f64).round().clamp(1.0, i64::MAX as f64) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn again_is_short_and_increments_lapses() {
        let next = schedule_review(1_000, 4.0, 5.0, 2, 1, ReviewRating::Again);
        assert_eq!(next.due_at, 1_000 + AGAIN_DELAY_MS);
        assert_eq!(next.review_count, 3);
        assert_eq!(next.lapse_count, 2);
        assert_eq!(next.stability_days, 0.1);
    }

    #[test]
    fn stronger_recall_produces_longer_intervals() {
        let hard = schedule_review(0, 2.0, 5.0, 0, 0, ReviewRating::Hard);
        let good = schedule_review(0, 2.0, 5.0, 0, 0, ReviewRating::Good);
        let easy = schedule_review(0, 2.0, 5.0, 0, 0, ReviewRating::Easy);
        assert!(hard.due_at < good.due_at);
        assert!(good.due_at < easy.due_at);
        assert!(hard.difficulty > good.difficulty);
        assert!(good.difficulty > easy.difficulty);
    }
}

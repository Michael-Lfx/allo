//! Calendar rules over UTC milliseconds. The caller supplies the time bounds.

use chrono::{
    DateTime, Datelike, Days, NaiveDate, NaiveTime, Offset, TimeZone, Timelike,
    Utc, Weekday,
};
pub use chrono_tz::Tz as IanaTimeZone;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TriggerSpec {
    #[default]
    Manual,
    Once {
        scheduled_at_ms: i64,
    },
    Calendar(CalendarRule),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarRule {
    /// Fixed on the revision. A later change to the user's timezone leaves
    /// existing rules and their occurrences unchanged.
    pub time_zone: IanaTimeZone,
    pub local_time: NaiveTime,
    pub frequency: CalendarFrequency,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CalendarFrequency {
    Daily,
    Weekly { weekday: Weekday },
}

/// First and last actual occurrences within a bounded catch-up window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OccurrenceWindow {
    pub first_at_ms: i64,
    pub last_at_ms: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MisfirePolicy {
    Skip,
    RunOnce,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchedulePolicy {
    pub trigger: TriggerSpec,
    pub misfire: MisfirePolicy,
    pub max_lateness_ms: NonZeroU64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ScheduleError {
    #[error("timestamp is outside the supported calendar range")]
    TimestampOutOfRange,
    #[error(
        "calendar time must use whole milliseconds and exclude leap seconds"
    )]
    InvalidLocalTime,
    #[error("occurrence window ends before it starts")]
    ReversedWindow,
}

impl TriggerSpec {
    /// Finds the first scheduled instant strictly after the supplied UTC
    /// cursor. Manual and exhausted one-time triggers have no next instant.
    pub fn next_after(
        &self,
        after_ms: i64,
    ) -> Result<Option<i64>, ScheduleError> {
        let after = utc_from_ms(after_ms)?;
        match self {
            Self::Manual => Ok(None),
            Self::Once { scheduled_at_ms } => {
                utc_from_ms(*scheduled_at_ms)?;
                Ok((*scheduled_at_ms > after_ms).then_some(*scheduled_at_ms))
            }
            Self::Calendar(rule) => rule.search(after, Search::After),
        }
    }

    /// Finds the first and last occurrences in `(after_ms, through_ms]`.
    /// Searches from the endpoints so coalescing a missed span does not
    /// enumerate every occurrence between them.
    pub fn occurrence_window(
        &self,
        after_ms: i64,
        through_ms: i64,
    ) -> Result<Option<OccurrenceWindow>, ScheduleError> {
        if through_ms < after_ms {
            return Err(ScheduleError::ReversedWindow);
        }
        let through = utc_from_ms(through_ms)?;
        let Some(first_at_ms) = self
            .next_after(after_ms)?
            .filter(|first| *first <= through_ms)
        else {
            return Ok(None);
        };
        let last_at_ms = match self {
            Self::Calendar(rule) => rule
                .search(through, Search::AtOrBefore)?
                .filter(|last| *last >= first_at_ms)
                .ok_or(ScheduleError::TimestampOutOfRange)?,
            Self::Manual | Self::Once { .. } => first_at_ms,
        };
        Ok(Some(OccurrenceWindow {
            first_at_ms,
            last_at_ms,
        }))
    }
}

impl CalendarRule {
    fn search(
        &self,
        cursor: DateTime<Utc>,
        direction: Search,
    ) -> Result<Option<i64>, ScheduleError> {
        let nanos = self.local_time.nanosecond();
        if nanos >= 1_000_000_000 || !nanos.is_multiple_of(1_000_000) {
            return Err(ScheduleError::InvalidLocalTime);
        }
        let utc = cursor.naive_utc();
        let offset = self.time_zone.offset_from_utc_datetime(&utc).fix();
        let local = utc
            .checked_add_offset(offset)
            .ok_or(ScheduleError::TimestampOutOfRange)?;
        let (initial_days, step) = match self.frequency {
            CalendarFrequency::Daily => (0, 1),
            CalendarFrequency::Weekly { weekday } => {
                let days = match direction {
                    Search::After => weekday.days_since(local.weekday()),
                    Search::AtOrBefore => local.weekday().days_since(weekday),
                };
                (u64::from(days), 7)
            }
        };

        let mut date = direction.step(local.date(), initial_days);
        while let Some(day) = date {
            // A gap has no instant. For a repeated time, always choose the
            // earlier UTC instant before comparing it with the cursor.
            if let Some(instant) = self
                .time_zone
                .from_local_datetime(&day.and_time(self.local_time))
                .earliest()
            {
                let matches = match direction {
                    Search::After => instant > cursor,
                    Search::AtOrBefore => instant <= cursor,
                };
                if matches {
                    return Ok(Some(instant.timestamp_millis()));
                }
            }
            date = direction.step(day, step);
        }
        Ok(None)
    }
}

#[derive(Clone, Copy)]
enum Search {
    After,
    AtOrBefore,
}

impl Search {
    const fn step(self, date: NaiveDate, days: u64) -> Option<NaiveDate> {
        match self {
            Self::After => date.checked_add_days(Days::new(days)),
            Self::AtOrBefore => date.checked_sub_days(Days::new(days)),
        }
    }
}

/// Computes the deadline fixed when an occurrence becomes a durable Run.
pub fn latest_start_at_ms(
    scheduled_at_ms: i64,
    max_lateness_ms: NonZeroU64,
) -> Result<i64, ScheduleError> {
    utc_from_ms(scheduled_at_ms)?;
    let lateness = i64::try_from(max_lateness_ms.get())
        .map_err(|_| ScheduleError::TimestampOutOfRange)?;
    let deadline = scheduled_at_ms
        .checked_add(lateness)
        .ok_or(ScheduleError::TimestampOutOfRange)?;
    utc_from_ms(deadline)?;
    Ok(deadline)
}

fn utc_from_ms(timestamp_ms: i64) -> Result<DateTime<Utc>, ScheduleError> {
    DateTime::from_timestamp_millis(timestamp_ms)
        .ok_or(ScheduleError::TimestampOutOfRange)
}

#[cfg(test)]
mod tests;

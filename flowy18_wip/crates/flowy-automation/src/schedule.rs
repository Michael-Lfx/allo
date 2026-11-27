//! Calendar rules over UTC milliseconds.

use chrono::{
    DateTime, Datelike, Days, NaiveTime, Offset, TimeZone, Timelike, Utc,
    Weekday,
};
pub use chrono_tz::Tz as IanaTimeZone;
use serde::{Deserialize, Serialize};

/// When an automation is due. There is no manual trigger kind: a one-off
/// test run is started with `AutomationService::run_now` and leaves the
/// schedule unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TriggerSpec {
    Once { scheduled_at_ms: i64 },
    Calendar(CalendarRule),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarRule {
    /// Fixed when the automation is saved. A later change to the user's
    /// timezone leaves existing rules unchanged.
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ScheduleError {
    #[error("timestamp is outside the supported calendar range")]
    TimestampOutOfRange,
    #[error(
        "calendar time must use whole milliseconds and exclude leap seconds"
    )]
    InvalidLocalTime,
}

impl TriggerSpec {
    /// First scheduled instant strictly after `after_ms`. A spent one-time
    /// trigger has none.
    pub fn next_after(
        &self,
        after_ms: i64,
    ) -> Result<Option<i64>, ScheduleError> {
        let after = utc_from_ms(after_ms)?;
        match self {
            Self::Once { scheduled_at_ms } => {
                utc_from_ms(*scheduled_at_ms)?;
                Ok((*scheduled_at_ms > after_ms).then_some(*scheduled_at_ms))
            }
            Self::Calendar(rule) => rule.next_after(after),
        }
    }
}

impl TriggerSpec {
    /// `at_ms` as wall-clock time in the rule's own time zone (UTC for a
    /// one-time trigger), e.g. `2026-11-28 08:00:00 Sat (Asia/Singapore)`,
    /// so callers never convert the millisecond timestamp themselves.
    pub fn local_time_label(&self, at_ms: i64) -> Option<String> {
        const FORMAT: &str = "%Y-%m-%d %H:%M:%S %a";
        let at = DateTime::from_timestamp_millis(at_ms)?;
        Some(match self {
            Self::Once { .. } => format!("{} (UTC)", at.format(FORMAT)),
            Self::Calendar(rule) => format!(
                "{} ({})",
                at.with_timezone(&rule.time_zone).format(FORMAT),
                rule.time_zone.name()
            ),
        })
    }
}

impl CalendarRule {
    fn next_after(
        &self,
        cursor: DateTime<Utc>,
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
                (u64::from(weekday.days_since(local.weekday())), 7)
            }
        };

        let mut date = local.date().checked_add_days(Days::new(initial_days));
        while let Some(day) = date {
            // A gap has no instant. For a repeated time, use the earlier one.
            if let Some(instant) = self
                .time_zone
                .from_local_datetime(&day.and_time(self.local_time))
                .earliest()
                .filter(|instant| *instant > cursor)
            {
                return Ok(Some(instant.timestamp_millis()));
            }
            date = day.checked_add_days(Days::new(step));
        }
        Ok(None)
    }
}

fn utc_from_ms(timestamp_ms: i64) -> Result<DateTime<Utc>, ScheduleError> {
    DateTime::from_timestamp_millis(timestamp_ms)
        .ok_or(ScheduleError::TimestampOutOfRange)
}

#[cfg(test)]
mod tests;

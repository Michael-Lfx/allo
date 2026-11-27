use super::*;

fn ms(value: &str) -> i64 {
    value.parse::<DateTime<Utc>>().unwrap().timestamp_millis()
}

fn calendar(
    zone: &str,
    hour: u32,
    minute: u32,
    frequency: CalendarFrequency,
) -> TriggerSpec {
    TriggerSpec::Calendar(CalendarRule {
        time_zone: zone.parse().unwrap(),
        local_time: NaiveTime::from_hms_opt(hour, minute, 0).unwrap(),
        frequency,
    })
}

#[test]
fn local_time_labels_use_the_rule_time_zone() {
    let at = ms("2026-11-28T00:00:00Z");
    let daily = calendar("Asia/Singapore", 8, 0, CalendarFrequency::Daily);
    assert_eq!(
        daily.local_time_label(at).as_deref(),
        Some("2026-11-28 08:00:00 Sat (Asia/Singapore)")
    );
    let once = TriggerSpec::Once {
        scheduled_at_ms: at,
    };
    assert_eq!(
        once.local_time_label(at).as_deref(),
        Some("2026-11-28 00:00:00 Sat (UTC)")
    );
}

#[test]
fn one_time_triggers_are_exclusive_on_the_lower_bound() {
    let scheduled_at_ms = ms("2026-01-01T00:00:00.123Z");
    let trigger = TriggerSpec::Once { scheduled_at_ms };
    assert_eq!(
        trigger.next_after(scheduled_at_ms - 1),
        Ok(Some(scheduled_at_ms))
    );
    assert_eq!(trigger.next_after(scheduled_at_ms), Ok(None));
}

#[test]
fn weekly_and_daily_use_the_local_day_across_year_boundaries() {
    let weekly = calendar(
        "Asia/Tokyo",
        8,
        15,
        CalendarFrequency::Weekly {
            weekday: Weekday::Thu,
        },
    );
    let new_year = ms("2025-12-31T23:15:00Z");
    assert_eq!(weekly.next_after(new_year - 1), Ok(Some(new_year)));
    assert_eq!(
        weekly.next_after(new_year),
        Ok(Some(ms("2026-01-07T23:15:00Z")))
    );
    let daily = calendar("Asia/Tokyo", 8, 15, CalendarFrequency::Daily);
    assert_eq!(
        daily.next_after(new_year),
        Ok(Some(ms("2026-01-01T23:15:00Z")))
    );
}

#[test]
fn daylight_saving_gaps_skip_daily_and_weekly_occurrences() {
    let cursor = ms("2025-03-09T05:00:00Z");
    let daily = calendar("America/New_York", 2, 30, CalendarFrequency::Daily);
    assert_eq!(
        daily.next_after(cursor),
        Ok(Some(ms("2025-03-10T06:30:00Z")))
    );
    let weekly = calendar(
        "America/New_York",
        2,
        30,
        CalendarFrequency::Weekly {
            weekday: Weekday::Sun,
        },
    );
    assert_eq!(
        weekly.next_after(cursor),
        Ok(Some(ms("2025-03-16T06:30:00Z")))
    );
}

#[test]
fn repeated_local_time_uses_only_the_earlier_utc_instant() {
    let trigger = calendar("America/New_York", 1, 30, CalendarFrequency::Daily);
    let first_occurrence = ms("2025-11-02T05:30:00Z");
    let second_local_match = ms("2025-11-02T06:30:00Z");
    let next_day = ms("2025-11-03T06:30:00Z");
    assert_eq!(
        trigger.next_after(first_occurrence - 1),
        Ok(Some(first_occurrence))
    );
    assert_eq!(trigger.next_after(first_occurrence), Ok(Some(next_day)));
    assert_eq!(
        trigger.next_after(second_local_match - 1),
        Ok(Some(next_day))
    );
}

#[test]
fn missed_days_resume_at_the_next_valid_occurrence() {
    // 2025-03-09 02:30 does not exist in New York.
    let daily = calendar("America/New_York", 2, 30, CalendarFrequency::Daily);
    assert_eq!(
        daily.next_after(ms("2025-03-08T07:30:00Z")),
        Ok(Some(ms("2025-03-10T06:30:00Z")))
    );
    let friday = calendar(
        "Etc/UTC",
        12,
        0,
        CalendarFrequency::Weekly {
            weekday: Weekday::Fri,
        },
    );
    assert_eq!(
        friday.next_after(ms("2026-01-01T13:00:00Z")),
        Ok(Some(ms("2026-01-02T12:00:00Z")))
    );
}

#[test]
fn calendar_shape_persists_and_validates_the_iana_zone() {
    let original = calendar("Europe/Berlin", 8, 15, CalendarFrequency::Daily);
    let mut encoded = serde_json::to_value(original).unwrap();
    assert_eq!(
        encoded.get("time_zone"),
        Some(&serde_json::json!("Europe/Berlin"))
    );
    assert_eq!(
        serde_json::from_value::<TriggerSpec>(encoded.clone()).unwrap(),
        original
    );
    encoded["time_zone"] = serde_json::json!("UTC+01:00");
    assert!(serde_json::from_value::<TriggerSpec>(encoded).is_err());
}

#[test]
fn timestamps_and_local_times_are_checked() {
    let invalid = TriggerSpec::Once {
        scheduled_at_ms: i64::MAX,
    };
    assert_eq!(
        invalid.next_after(0),
        Err(ScheduleError::TimestampOutOfRange)
    );
    let daily = calendar("Etc/UTC", 9, 0, CalendarFrequency::Daily);
    assert_eq!(
        daily.next_after(i64::MIN),
        Err(ScheduleError::TimestampOutOfRange)
    );
    let leap_second = TriggerSpec::Calendar(CalendarRule {
        time_zone: IanaTimeZone::UTC,
        local_time: NaiveTime::from_hms_milli_opt(23, 59, 59, 1_000).unwrap(),
        frequency: CalendarFrequency::Daily,
    });
    assert_eq!(
        leap_second.next_after(0),
        Err(ScheduleError::InvalidLocalTime)
    );
}

//! Band metrics for work package payloads.

use crate::sanitize::normalize_duration_band;
use crate::types::WorkMetricsPayload;

pub fn build_work_metrics(
    user_turns: u32,
    tool_failures: u32,
    skill_patch_count: u32,
    duration_ms: Option<u64>,
) -> WorkMetricsPayload {
    WorkMetricsPayload {
        turn_band: band_turns(user_turns),
        duration_band: session_duration_band(user_turns, duration_ms),
        tool_failure_band: band_tool_failures(tool_failures),
        skill_patch_count_band: band_skill_patches(skill_patch_count),
    }
}

pub fn session_duration_ms(messages: &[serde_json::Value]) -> Option<u64> {
    let mut min_ts: Option<i64> = None;
    let mut max_ts: Option<i64> = None;
    for message in messages {
        let Some(ts) = message_timestamp_ms(message) else {
            continue;
        };
        min_ts = Some(min_ts.map_or(ts, |current| current.min(ts)));
        max_ts = Some(max_ts.map_or(ts, |current| current.max(ts)));
    }
    match (min_ts, max_ts) {
        (Some(start), Some(end)) if end >= start => Some((end - start) as u64),
        _ => None,
    }
}

pub fn session_duration_band(user_turns: u32, duration_ms: Option<u64>) -> String {
    if let Some(ms) = duration_ms {
        return normalize_duration_band(band_from_ms(ms));
    }
    normalize_duration_band(band_from_turns(user_turns))
}

fn message_timestamp_ms(message: &serde_json::Value) -> Option<i64> {
    for key in ["created_at", "timestamp", "ts"] {
        let value = message.get(key)?;
        if let Some(ms) = value.as_i64() {
            return Some(normalize_epoch_ms(ms));
        }
        if let Some(ms) = value.as_u64() {
            return Some(normalize_epoch_ms(ms as i64));
        }
        if let Some(raw) = value.as_str() {
            if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(raw) {
                return Some(parsed.timestamp_millis());
            }
        }
    }
    None
}

fn normalize_epoch_ms(value: i64) -> i64 {
    if value > 0 && value < 1_000_000_000_000 {
        value * 1000
    } else {
        value
    }
}

fn band_from_ms(ms: u64) -> &'static str {
    const MINUTE: u64 = 60_000;
    if ms < 5 * MINUTE {
        "0-5m"
    } else if ms < 15 * MINUTE {
        "5-15m"
    } else if ms < 30 * MINUTE {
        "15-30m"
    } else {
        "30m+"
    }
}

fn band_from_turns(user_turns: u32) -> &'static str {
    match user_turns {
        0..=2 => "0-5m",
        3..=5 => "5-15m",
        6..=10 => "15-30m",
        _ => "30m+",
    }
}

fn band_turns(turns: u32) -> String {
    match turns {
        0..=2 => "1-2".to_string(),
        3..=5 => "3-5".to_string(),
        6..=10 => "6-10".to_string(),
        _ => "11+".to_string(),
    }
}

fn band_tool_failures(failures: u32) -> String {
    match failures {
        0 => "0".to_string(),
        1..=2 => "1-2".to_string(),
        _ => "3+".to_string(),
    }
}

fn band_skill_patches(count: u32) -> String {
    match count {
        0 => "0".to_string(),
        1 => "1".to_string(),
        _ => "2+".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_band_uses_timestamps_when_present() {
        let messages = vec![
            serde_json::json!({"role":"user","created_at": 1_700_000_000_000i64}),
            serde_json::json!({"role":"assistant","created_at": 1_700_000_000_000i64 + 20 * 60_000}),
        ];
        let ms = session_duration_ms(&messages);
        assert_eq!(ms, Some(20 * 60_000));
        assert_eq!(session_duration_band(2, ms), "15-30m");
    }

    #[test]
    fn duration_band_falls_back_to_turn_count() {
        assert_eq!(session_duration_band(2, None), "0-5m");
        assert_eq!(session_duration_band(4, None), "5-15m");
        assert_eq!(session_duration_band(12, None), "30m+");
    }
}

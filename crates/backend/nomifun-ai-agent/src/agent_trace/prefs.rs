//! Client-preference gate for developer-mode tracing.

use std::sync::Arc;

use nomifun_db::IClientPreferenceRepository;

/// Workspace preference key owned by the System settings Developer Mode toggle.
pub const DEVELOPER_MODE_PREF_KEY: &str = "system.developerMode";

/// Read `system.developerMode`. Missing / unreadable prefs → `false`.
pub async fn developer_mode_enabled(
    prefs: Option<&Arc<dyn IClientPreferenceRepository>>,
) -> bool {
    let Some(repo) = prefs else {
        return false;
    };
    match repo.get_by_keys(&[DEVELOPER_MODE_PREF_KEY]).await {
        Ok(rows) => rows.into_iter().any(|row| preference_is_true(&row.value)),
        Err(error) => {
            tracing::debug!(
                error = %error,
                key = DEVELOPER_MODE_PREF_KEY,
                "agent trace: failed to read developer mode preference"
            );
            false
        }
    }
}

fn preference_is_true(raw: &str) -> bool {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(serde_json::Value::Bool(value)) => value,
        Ok(serde_json::Value::String(value)) => {
            matches!(value.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes")
        }
        Ok(serde_json::Value::Number(number)) => number.as_i64() == Some(1),
        _ => matches!(raw.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bool_json() {
        assert!(preference_is_true("true"));
        assert!(!preference_is_true("false"));
        assert!(preference_is_true("\"true\""));
        assert!(preference_is_true("1"));
    }
}

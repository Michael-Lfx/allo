//! Local POI persistence gate (mirrors `nomi-insights-core::sanitize` policy without a crate cycle).

pub fn is_persistable_local_poi(topic_id: &str, label: &str) -> bool {
    let id = topic_id.trim().to_ascii_lowercase();
    if id.is_empty() {
        return false;
    }
    if id.starts_with("keyword:") || id.starts_with("path:") {
        return false;
    }
    let label = label.trim();
    if label.is_empty() || label.chars().count() < 2 {
        return false;
    }
    if label.chars().all(|c| c.is_ascii_hexdigit()) && label.len() >= 8 {
        return false;
    }
    true
}

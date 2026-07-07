//! Stable, collision-resistant topic ids for the POI store.

use sha2::{Digest, Sha256};

/// Normalize text used for identity (not necessarily equal to display label).
pub fn normalize_canonical_key(text: &str) -> String {
    let mut out = String::new();
    for ch in text.trim().chars() {
        if ch.is_whitespace() {
            continue;
        }
        if ch.is_ascii_alphabetic() {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// Build a primary-key-safe topic id: readable when possible, otherwise hashed.
pub fn stable_topic_id(namespace: &str, canonical: &str) -> String {
    let canonical = normalize_canonical_key(canonical);
    if canonical.is_empty() {
        return String::new();
    }
    let ns = namespace.trim();
    if ns.is_empty() {
        return String::new();
    }

    // Short ASCII tokens get stable readable ids (debuggable in `hermes interest list`).
    if canonical.is_ascii()
        && canonical.len() <= 28
        && canonical
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return format!("{ns}:{canonical}");
    }

    let mut hasher = Sha256::new();
    hasher.update(ns.as_bytes());
    hasher.update(b"|");
    hasher.update(canonical.as_bytes());
    let digest = hex::encode(hasher.finalize());
    format!("{ns}:{}", &digest[..16])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_chinese_canonicals_distinct_ids() {
        let a = stable_topic_id("interest", "打篮球");
        let b = stable_topic_id("interest", "吃鱼");
        assert_ne!(a, b);
        assert!(a.starts_with("interest:"));
        assert!(b.starts_with("interest:"));
    }

    #[test]
    fn same_canonical_same_id() {
        let a = stable_topic_id("interest", "打篮球");
        let b = stable_topic_id("interest", "  打篮 球 ");
        assert_eq!(a, b);
    }
}

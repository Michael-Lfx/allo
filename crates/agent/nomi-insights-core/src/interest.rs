//! Build interest fingerprints from local POI topic rows.

use chrono::Utc;

use crate::sanitize::{
    evidence_band, filter_tags, is_contributable_for_ops, parse_namespace, readable_topic_key,
    sanitize_summary, sanitize_text, taxonomy_hints_for, weight_band,
};
use crate::types::{InterestFingerprint, InterestTopicFingerprint};

/// Input row mirroring `InterestTopic` without requiring hermes-agent dependency.
#[derive(Debug, Clone)]
pub struct InterestTopicInput {
    pub id: String,
    pub label: String,
    pub summary: String,
    pub weight: f64,
    pub evidence_count: u32,
    pub tags: Vec<String>,
}

pub fn build_interest_fingerprint(topics: &[InterestTopicInput]) -> Option<InterestFingerprint> {
    let collected_at = Utc::now().to_rfc3339();
    let mut fingerprints = Vec::new();
    for topic in topics {
        if !is_contributable_for_ops(&topic.id, &topic.label) {
            continue;
        }
        let label_redacted = sanitize_text(&topic.label);
        if label_redacted.is_empty() {
            continue;
        }
        let topic_key = readable_topic_key(&topic.id, &label_redacted);
        fingerprints.push(InterestTopicFingerprint {
            topic_key: topic_key.clone(),
            label_redacted: label_redacted.clone(),
            summary_redacted: sanitize_summary(&topic.summary),
            namespace: parse_namespace(&topic_key),
            weight_band: weight_band(topic.weight).to_string(),
            evidence_band: evidence_band(topic.evidence_count).to_string(),
            tags: filter_tags(&topic.tags),
            taxonomy_hints: taxonomy_hints_for(&topic_key),
        });
    }
    if fingerprints.is_empty() {
        return None;
    }
    let co_topics: Vec<String> = fingerprints
        .iter()
        .take(5)
        .map(|t| t.label_redacted.clone())
        .collect();
    Some(InterestFingerprint {
        topics: fingerprints,
        co_topics,
        collected_at,
    })
}

/// Top sanitized POI labels for linking into skill patterns.
pub fn top_readable_interest_labels(topics: &[InterestTopicInput], limit: usize) -> Vec<String> {
    let mut labels = Vec::new();
    for topic in topics {
        if !is_contributable_for_ops(&topic.id, &topic.label) {
            continue;
        }
        let label = sanitize_text(&topic.label);
        if label.is_empty() {
            continue;
        }
        if labels.iter().any(|l| l == &label) {
            continue;
        }
        labels.push(label);
        if labels.len() >= limit {
            break;
        }
    }
    labels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_fingerprint_without_path_topics() {
        let topics = vec![
            InterestTopicInput {
                id: "path:crates/foo".into(),
                label: "crates/foo".into(),
                summary: String::new(),
                weight: 0.8,
                evidence_count: 3,
                tags: vec![],
            },
            InterestTopicInput {
                id: "lang:rust".into(),
                label: "Rust".into(),
                summary: "Backend systems".into(),
                weight: 0.9,
                evidence_count: 5,
                tags: vec!["backend".into()],
            },
        ];
        let fp = build_interest_fingerprint(&topics).unwrap();
        assert_eq!(fp.topics.len(), 1);
        assert_eq!(fp.topics[0].topic_key, "lang:rust");
        assert_eq!(fp.topics[0].label_redacted, "Rust");
        assert_eq!(fp.co_topics, vec!["Rust".to_string()]);
    }

    #[test]
    fn declared_interest_uses_readable_key_not_hex_id() {
        let topics = vec![InterestTopicInput {
            id: "interest:0062d40fb666492a".into(),
            label: "Beijing dialect".into(),
            summary: "User prefers casual Beijing phrasing".into(),
            weight: 0.9,
            evidence_count: 6,
            tags: vec!["declared".into()],
        }];
        let fp = build_interest_fingerprint(&topics).unwrap();
        assert_eq!(fp.topics[0].topic_key, "topic:beijing-dialect");
        assert_eq!(fp.topics[0].label_redacted, "Beijing dialect");
        assert!(!fp.topics[0].topic_key.contains("0062d40f"));
    }
}

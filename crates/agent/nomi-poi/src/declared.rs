//! Explicit user-declared interests ("我的兴趣点是…", "I like …").

use std::sync::LazyLock;

use regex::Regex;

use super::store::InterestSignal;
use super::topic_id::{normalize_canonical_key, stable_topic_id};
use super::types::SignalSource;

static DECLARED_CAPTURE_RES: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"(?i)(?:我的)?兴趣点(?:是|为)[:：]?\s*([^.。!！?？\n\r]+)").unwrap(),
        Regex::new(r"(?i)(?:我的)?兴趣点(?:还)?(?:有|包括)[:：]?\s*([^.。!！?？\n\r]+)").unwrap(),
        Regex::new(r"(?:我)?(?:的)?(?:爱好|喜好|特长)(?:是|为)?[:：]?\s*([^.。!！?？\n\r]+)").unwrap(),
        Regex::new(r"(?:我|我们)(?:超|很|非常)?喜欢[:：]?\s*([^.。!！?？\n\r]{2,40})").unwrap(),
        Regex::new(r"(?:我|我们)(?:热爱|钟爱)[:：]?\s*([^.。!！?？\n\r]{2,40})").unwrap(),
        Regex::new(r"喜欢[:：]?\s*([^.。!！?？\n\r]{2,40})").unwrap(),
        Regex::new(r"(?i)\bmy\s+interests?\s+(?:is|are|include)[:：]?\s*([^.!?\n\r]+)").unwrap(),
        Regex::new(r"(?i)\b(?:i\s+)?(?:really\s+)?(?:like|love|enjoy)\s+([^.!?\n\r]+)").unwrap(),
    ]
});

/// Extract durable interests the user explicitly stated in natural language.
pub fn extract_declared_interests(text: &str, weight_scale: f64) -> Vec<InterestSignal> {
    let mut out = Vec::new();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return out;
    }

    for re in DECLARED_CAPTURE_RES.iter() {
        for cap in re.captures_iter(trimmed) {
            if let Some(m) = cap.get(1) {
                for fragment in split_interest_fragments(m.as_str()) {
                    let canonical = normalize_canonical_key(&fragment);
                    if canonical.chars().count() < 2 || canonical.chars().count() > 64 {
                        continue;
                    }
                    if is_declared_noise(&canonical) {
                        continue;
                    }
                    let label = format!("兴趣: {fragment}");
                    push_declared_signal(
                        &mut out,
                        &canonical,
                        &label,
                        &format!("用户明确表示的兴趣：{fragment}"),
                        0.42 * weight_scale,
                    );
                }
            }
        }
    }
    out
}

fn push_declared_signal(
    out: &mut Vec<InterestSignal>,
    canonical: &str,
    label: &str,
    summary: &str,
    weight_delta: f64,
) {
    let id = stable_topic_id("interest", canonical);
    if id.is_empty() {
        return;
    }
    out.push(InterestSignal::new(
        id,
        label.to_string(),
        summary.to_string(),
        weight_delta,
        vec!["interest".to_string(), "declared".to_string()],
        SignalSource::Declared,
    ));
}

pub(crate) fn is_declared_noise(canonical: &str) -> bool {
    let lower = canonical.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "什么" | "哪些" | "怎么" | "如何" | "为什么" | "nothing" | "something" | "anything"
    ) || canonical
        .chars()
        .all(|c| c.is_ascii_punctuation() || c.is_whitespace())
}

/// Split compound interest phrases: "打篮球和游泳", "A, B", "A、B".
pub(crate) fn split_interest_fragments(raw: &str) -> Vec<String> {
    let mut pieces = vec![raw.trim().to_string()];
    for sep in ['，', ',', '、', ';', '；'] {
        let mut next = Vec::new();
        for piece in pieces {
            for part in piece.split(sep) {
                let t = part.trim();
                if !t.is_empty() {
                    next.push(t.to_string());
                }
            }
        }
        pieces = next;
    }

    let mut out = Vec::new();
    for piece in pieces {
        let mut subpieces = vec![piece];
        for conj in ["和", "以及", "还有", "以及还有"] {
            let mut next = Vec::new();
            for sub in subpieces {
                if let Some((left, right)) = sub.split_once(conj) {
                    let l = left.trim().trim_start_matches("还").trim();
                    let r = right.trim().trim_start_matches("还").trim();
                    if !l.is_empty() {
                        next.push(l.to_string());
                    }
                    if !r.is_empty() {
                        next.push(r.to_string());
                    }
                } else {
                    next.push(sub);
                }
            }
            subpieces = next;
        }
        for sub in subpieces {
            let t = sub
                .trim()
                .trim_start_matches('还')
                .trim_start_matches("还有")
                .trim();
            if t.len() >= 2 {
                out.push(t.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn splits_compound_chinese_interests() {
        let parts = split_interest_fragments("打篮球和游泳");
        assert_eq!(parts, vec!["打篮球", "游泳"]);
    }

    #[test]
    fn two_messages_distinct_interest_ids() {
        let a = extract_declared_interests("我的兴趣点是打篮球", 1.0);
        let b = extract_declared_interests("我的兴趣点还有吃鱼", 1.0);
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
        assert_ne!(a[0].id, b[0].id);
        assert!(a[0].label.contains("打篮球"));
        assert!(b[0].label.contains("吃鱼"));
    }

    #[test]
    fn compound_declaration_two_rows() {
        let signals = extract_declared_interests("我的兴趣点是打篮球和游泳", 1.0);
        let ids: HashSet<_> = signals.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids.len(), 2);
    }
}

//! Contextual interest phrases (Chinese / English) inferred from user wording.

use std::sync::LazyLock;

use regex::Regex;

use super::declared::{is_declared_noise, split_interest_fragments};
use super::store::InterestSignal;
use super::topic_id::{normalize_canonical_key, stable_topic_id};
use super::types::SignalSource;

static CONTEXTUAL_CAPTURE_RES: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        // Chinese — activity / focus
        Regex::new(
            r"(?:我|我们)(?:最近在|正在|一直在|近期在)(?:研究|学习|开发|做|从事|专注(?:于)?|深入)[:：]?\s*([^.。!！?？\n\r]{2,40})",
        )
        .unwrap(),
        Regex::new(
            r"(?:我|我们)(?:主要|平时|目前|长期)(?:做|搞|负责|在做|开发|研究)[:：]?\s*([^.。!！?？\n\r]{2,40})",
        )
        .unwrap(),
        Regex::new(
            r"我对\s*([^.。!！?？\n\r]{2,28})\s*(?:很感兴趣|特别感兴趣|很有兴趣|非常感兴趣|感兴趣)",
        )
        .unwrap(),
        Regex::new(
            r"(?:我的)?(?:专业|职业|工作|岗位)(?:是|为|方向是)[:：]?\s*([^.。!！?？\n\r]{2,32})",
        )
        .unwrap(),
        Regex::new(
            r"(?:我|我们)(?:想|希望|打算|准备)(?:学习|掌握|深入|转型(?:到)?)[:：]?\s*([^.。!！?？\n\r]{2,36})",
        )
        .unwrap(),
        // English — project / focus
        Regex::new(
            r"(?i)\b(?:i'?m|i am|we'?re|we are)\s+(?:working on|building|learning|focused on|specializing in)\s+([^.!?\n\r]{2,48})",
        )
        .unwrap(),
        Regex::new(
            r"(?i)\b(?:my|our)\s+(?:main\s+)?(?:focus|project|stack|area)\s+(?:is|includes|centers on)\s+([^.!?\n\r]{2,48})",
        )
        .unwrap(),
        Regex::new(
            r"(?i)\b(?:i'?m|i am)\s+(?:a|an)\s+([a-z][a-z0-9 /_-]{2,40}?)\s+(?:developer|engineer|researcher|designer|architect)\b",
        )
        .unwrap(),
    ]
});

/// Infer interests from contextual phrasing (lower trust than explicit declaration).
pub fn extract_contextual_interests(text: &str, weight_scale: f64) -> Vec<InterestSignal> {
    let mut out = Vec::new();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return out;
    }

    for re in CONTEXTUAL_CAPTURE_RES.iter() {
        for cap in re.captures_iter(trimmed) {
            if let Some(m) = cap.get(1) {
                for fragment in split_interest_fragments(m.as_str()) {
                    push_contextual_signal(&mut out, &fragment, weight_scale);
                }
            }
        }
    }
    out
}

fn push_contextual_signal(out: &mut Vec<InterestSignal>, fragment: &str, weight_scale: f64) {
    let fragment = fragment.trim();
    let canonical = normalize_canonical_key(fragment);
    if canonical.chars().count() < 2 || canonical.chars().count() > 48 {
        return;
    }
    if is_declared_noise(&canonical) || is_contextual_noise(&canonical) {
        return;
    }
    let id = stable_topic_id("interest", &canonical);
    if id.is_empty() {
        return;
    }
    let label = if canonical.is_ascii() {
        format!("focus: {fragment}")
    } else {
        format!("关注: {fragment}")
    };
    let mut signal = InterestSignal::new(
        id,
        label,
        format!("用户语境中反复出现的方向：{fragment}"),
        0.28 * weight_scale,
        vec!["interest".to_string(), "contextual".to_string()],
        SignalSource::Rules,
    );
    signal.confidence = 0.62;
    out.push(signal);
}

fn is_contextual_noise(canonical: &str) -> bool {
    let lower = canonical.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "this"
            | "that"
            | "it"
            | "them"
            | "something"
            | "anything"
            | "everything"
            | "here"
            | "there"
    ) || matches!(
        canonical,
        "这个" | "那个" | "这些" | "那些" | "什么" | "怎么" | "如何" | "一下" | "东西" | "事情"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_contextual_focus() {
        let signals = extract_contextual_interests("我最近在研究大模型应用和产品设计", 1.0);
        assert!(signals.len() >= 1);
        assert!(signals.iter().any(|s| s.label.contains("大模型")));
        assert!(signals.iter().all(|s| s.id.starts_with("interest:")));
    }

    #[test]
    fn english_working_on() {
        let signals =
            extract_contextual_interests("I'm working on Rust CLI tooling and parity tests", 1.0);
        assert!(!signals.is_empty());
        assert!(signals[0].label.to_ascii_lowercase().contains("rust"));
    }
}

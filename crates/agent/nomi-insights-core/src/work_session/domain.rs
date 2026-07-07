//! Extract domain POI candidates from session user text.

use crate::sanitize::{
    is_weak_domain_key_raw, is_weak_v3_domain_key, normalize_domain_key, sanitize_text,
    slugify_name,
};
use crate::types::DomainPoiPayload;
use nomi_poi::{
    InterestSignal, SignalSource, extract_contextual_interests, extract_declared_interests,
    extract_signals_from_messages, is_poi_synthetic_user_text, message_text_from_value,
};

use tracing::info;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainCandidateSource {
    InterestDb,
    Rules,
    TranscriptFallback,
    SkillBinding,
}

#[derive(Debug, Clone)]
pub struct DomainCandidate {
    pub domain_key: String,
    pub problem_statement_redacted: String,
    pub problem_class: String,
    pub difficulty_band: String,
    pub taxonomy_code: Option<String>,
    pub confidence: f64,
}

pub fn extract_domain_candidate(messages: &[serde_json::Value]) -> Option<DomainCandidate> {
    let user_text = user_transcript(messages);
    if user_text.trim().len() < 12 {
        return None;
    }

    let mut signals: Vec<InterestSignal> = Vec::new();
    signals.extend(extract_declared_interests(&user_text, 1.0));
    signals.extend(extract_contextual_interests(&user_text, 1.0));
    signals.extend(extract_signals_from_messages(messages));

    let mut signals: Vec<InterestSignal> = signals
        .into_iter()
        .filter(|s| !is_noise_signal(s))
        .filter(|s| is_domain_eligible_signal(s))
        .collect();
    signals.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    for signal in &signals {
        if let Some(candidate) = candidate_from_signal(signal, messages, &user_text) {
            if is_usable_domain_candidate(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// Prefer session-end interest.db (LLM), then substantive rules, transcript, skill slug.
pub fn extract_domain_candidate_for_work_package(
    data_dir: &std::path::Path,
    interest_enabled: bool,
    messages: &[serde_json::Value],
    skill_slugs: &[String],
) -> Option<DomainCandidate> {
    extract_domain_candidate_for_work_package_with_source(
        data_dir,
        interest_enabled,
        messages,
        skill_slugs,
    )
    .map(|(candidate, _)| candidate)
}

pub fn extract_domain_candidate_for_work_package_with_source(
    data_dir: &std::path::Path,
    interest_enabled: bool,
    messages: &[serde_json::Value],
    skill_slugs: &[String],
) -> Option<(DomainCandidate, DomainCandidateSource)> {
    if interest_enabled {
        if let Some(candidate) = domain_candidate_from_interest_db(data_dir) {
            info!(
                domain_key = %candidate.domain_key,
                source = "interest_db",
                confidence = candidate.confidence,
                "work_session: domain POI candidate extracted"
            );
            return Some((candidate, DomainCandidateSource::InterestDb));
        }
    }
    if let Some(candidate) = extract_domain_candidate(messages) {
        info!(
            domain_key = %candidate.domain_key,
            source = "rules",
            confidence = candidate.confidence,
            "work_session: domain POI candidate extracted"
        );
        return Some((candidate, DomainCandidateSource::Rules));
    }
    if let Some(candidate) = fallback_domain_from_transcript(messages) {
        if is_usable_domain_candidate(&candidate) {
            info!(
                domain_key = %candidate.domain_key,
                source = "transcript_fallback",
                confidence = candidate.confidence,
                "work_session: domain POI candidate extracted"
            );
            return Some((candidate, DomainCandidateSource::TranscriptFallback));
        }
    }
    if let Some(candidate) = domain_candidate_from_skill_slugs(skill_slugs, messages) {
        info!(
            domain_key = %candidate.domain_key,
            source = "skill_binding",
            confidence = candidate.confidence,
            "work_session: domain POI candidate extracted"
        );
        return Some((candidate, DomainCandidateSource::SkillBinding));
    }
    None
}

pub fn is_usable_domain_candidate(candidate: &DomainCandidate) -> bool {
    if candidate.problem_statement_redacted.trim().chars().count() < 8 {
        return false;
    }
    if is_weak_domain_key_raw(&candidate.domain_key) {
        return false;
    }
    !is_weak_v3_domain_key(&normalize_domain_key(&candidate.domain_key))
}

fn candidate_from_signal(
    best: &InterestSignal,
    messages: &[serde_json::Value],
    user_text: &str,
) -> Option<DomainCandidate> {
    let label = sanitize_text(&best.label);
    let summary = sanitize_text(if best.summary.is_empty() {
        &label
    } else {
        &best.summary
    });
    if label.is_empty() || summary.is_empty() {
        return None;
    }

    let domain_key = domain_key_from_signal(best, &label);
    let problem_class = infer_problem_class(messages, best);
    let difficulty = if user_text.chars().count() > 400 {
        "high"
    } else if user_text.chars().count() > 120 {
        "med"
    } else {
        "low"
    };

    Some(DomainCandidate {
        domain_key,
        problem_statement_redacted: summary,
        problem_class: problem_class.to_string(),
        difficulty_band: difficulty.to_string(),
        taxonomy_code: taxonomy_hint(best),
        confidence: best.confidence,
    })
}

fn domain_candidate_from_interest_db(data_dir: &std::path::Path) -> Option<DomainCandidate> {
    use nomi_config::InterestConfig;

    use nomi_poi::{InterestSignal, InterestStore};

    let db_path = data_dir.join("interest.db");
    let store = InterestStore::open(&db_path, InterestConfig::default()).ok()?;
    let topics = store.top_topics(8).ok()?;
    let mut ordered: Vec<_> = topics.iter().collect();
    ordered.sort_by(|a, b| {
        let llm_rank = |source: SignalSource| match source {
            SignalSource::Llm => 0,
            SignalSource::Declared | SignalSource::Rules => 1,
            _ => 2,
        };
        llm_rank(a.source)
            .cmp(&llm_rank(b.source))
            .then_with(|| {
                b.confidence
                    .partial_cmp(&a.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| {
                b.weight
                    .partial_cmp(&a.weight)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    for topic in ordered {
        let label = sanitize_text(&topic.label);
        let summary = sanitize_text(if topic.summary.is_empty() {
            &label
        } else {
            &topic.summary
        });
        if label.is_empty() || summary.is_empty() {
            continue;
        }
        let signal = InterestSignal::new(
            topic.id.clone(),
            label,
            summary.clone(),
            0.0,
            topic.tags.clone(),
            topic.source,
        );
        if let Some(candidate) = candidate_from_signal(&signal, &[], &summary) {
            if is_usable_domain_candidate(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn domain_candidate_from_skill_slugs(
    slugs: &[String],
    messages: &[serde_json::Value],
) -> Option<DomainCandidate> {
    let slug = slugs
        .iter()
        .find(|s| is_usable_skill_slug(s))?
        .trim()
        .to_string();
    let user_text = user_transcript(messages);
    let sanitized = sanitize_text(user_text.trim());
    let problem_statement = if sanitized.chars().count() >= 12 {
        sanitized.chars().take(400).collect()
    } else {
        format!("Workflow personalized via skill `{slug}`")
    };
    let domain_key = format!("topic:{slug}");
    let candidate = DomainCandidate {
        domain_key,
        problem_statement_redacted: problem_statement,
        problem_class: infer_skill_problem_class(&slug),
        difficulty_band: "med".to_string(),
        taxonomy_code: None,
        confidence: 0.55,
    };
    is_usable_domain_candidate(&candidate).then_some(candidate)
}

fn is_usable_skill_slug(slug: &str) -> bool {
    let s = slug.trim().to_ascii_lowercase();
    s.len() >= 4 && !matches!(s.as_str(), "skill" | "skills" | "test" | "demo")
}

fn infer_skill_problem_class(slug: &str) -> String {
    let lower = slug.to_ascii_lowercase();
    if lower.contains("content")
        || lower.contains("creative")
        || lower.contains("douyin")
        || lower.contains("video")
        || lower.contains("copy")
    {
        "creative".to_string()
    } else {
        "operational".to_string()
    }
}

fn fallback_domain_from_transcript(messages: &[serde_json::Value]) -> Option<DomainCandidate> {
    let user_text = user_transcript(messages);
    let sanitized = sanitize_text(user_text.trim());
    if sanitized.chars().count() < 12 {
        return None;
    }
    let excerpt: String = sanitized.chars().take(400).collect();
    let slug = slugify_name(&excerpt);
    let domain_key = if slug.is_empty() {
        format!(
            "topic:session-{}",
            &crate::types::sha256_hex(excerpt.as_bytes())[..8]
        )
    } else {
        format!("topic:{slug}")
    };
    let difficulty = if sanitized.chars().count() > 400 {
        "high"
    } else if sanitized.chars().count() > 120 {
        "med"
    } else {
        "low"
    };
    Some(DomainCandidate {
        domain_key,
        problem_statement_redacted: excerpt,
        problem_class: "operational".to_string(),
        difficulty_band: difficulty.to_string(),
        taxonomy_code: None,
        confidence: 0.45,
    })
}

pub fn candidate_to_poi(candidate: &DomainCandidate) -> DomainPoiPayload {
    DomainPoiPayload {
        domain_key: normalize_domain_key(&candidate.domain_key),
        taxonomy_code: candidate.taxonomy_code.clone(),
        problem_class: candidate.problem_class.clone(),
        problem_statement_redacted: candidate.problem_statement_redacted.clone(),
        difficulty_band: candidate.difficulty_band.clone(),
    }
}

fn user_transcript(messages: &[serde_json::Value]) -> String {
    let mut out = String::new();
    for msg in messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if !role.eq_ignore_ascii_case("user") {
            continue;
        }
        let content = message_text_from_value(msg);
        let trimmed = content.trim();
        if trimmed.is_empty() || is_poi_synthetic_user_text(trimmed) {
            continue;
        }
        out.push_str(trimmed);
        out.push('\n');
    }
    out
}

fn is_noise_signal(signal: &InterestSignal) -> bool {
    let id = signal.id.to_ascii_lowercase();
    id.starts_with("path:") || id.starts_with("keyword:") || signal.label.trim().is_empty()
}

fn is_domain_eligible_signal(signal: &InterestSignal) -> bool {
    !matches!(signal.source, SignalSource::Keyword | SignalSource::Path)
}

fn domain_key_from_signal(signal: &InterestSignal, label: &str) -> String {
    if let Some(key) = llm_domain_key_from_tags(&signal.tags) {
        return key;
    }
    let id = signal.id.trim().to_ascii_lowercase();
    if id.starts_with("lang:") || id.starts_with("tech:") || id.starts_with("topic:") {
        return id;
    }
    let slug = slugify_name(label);
    if slug.is_empty() {
        format!(
            "topic:domain-{}",
            &crate::types::sha256_hex(label.as_bytes())[..8]
        )
    } else {
        format!("topic:{slug}")
    }
}

fn llm_domain_key_from_tags(tags: &[String]) -> Option<String> {
    const META: &[&str] = &[
        "domain",
        "interest",
        "contextual",
        "keyword",
        "path",
        "lang",
        "tech",
        "llm",
        "rules",
        "declared",
    ];
    tags.iter().find_map(|tag| {
        let t = tag.trim();
        if t.is_empty() || META.contains(&t.to_ascii_lowercase().as_str()) {
            return None;
        }
        if t.contains(':') {
            return None;
        }
        if t.contains('.') || t.contains('_') || t.chars().count() >= 6 {
            Some(t.to_string())
        } else {
            None
        }
    })
}

fn infer_problem_class(messages: &[serde_json::Value], signal: &InterestSignal) -> &'static str {
    if matches!(signal.source, SignalSource::Lang | SignalSource::Tech) {
        return "technical";
    }
    if transcript_mentions_tool(messages, "execute_code") {
        return "technical";
    }
    if signal.tags.iter().any(|t| t.contains("research")) {
        return "research";
    }
    if signal.tags.iter().any(|t| t.contains("creative")) {
        return "creative";
    }
    "operational"
}

fn taxonomy_hint(signal: &InterestSignal) -> Option<String> {
    if matches!(signal.source, SignalSource::Lang) {
        let lang = signal.id.strip_prefix("lang:")?;
        return Some(format!("software.lang.{lang}"));
    }
    if matches!(signal.source, SignalSource::Tech) {
        let tech = signal.id.strip_prefix("tech:")?;
        return Some(format!("software.tech.{tech}"));
    }
    None
}

fn transcript_mentions_tool(messages: &[serde_json::Value], tool_name: &str) -> bool {
    messages.iter().any(|msg| {
        msg.get("role")
            .and_then(|v| v.as_str())
            .is_some_and(|r| r == "assistant")
            && msg
                .get("tool_calls")
                .and_then(|v| v.as_array())
                .is_some_and(|arr| {
                    arr.iter().any(|tc| {
                        tc.get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())
                            == Some(tool_name)
                    })
                })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_poi::{InterestSignal, SignalSource};

    #[test]
    fn rejects_generic_skill_keyword_domain() {
        let candidate = DomainCandidate {
            domain_key: "topic:skill".into(),
            problem_statement_redacted: "Frequent term skill in user messages".into(),
            problem_class: "operational".into(),
            difficulty_band: "low".into(),
            taxonomy_code: None,
            confidence: 0.38,
        };
        assert!(!is_usable_domain_candidate(&candidate));
    }

    #[test]
    fn skill_slug_fallback_produces_specific_domain() {
        let messages = vec![serde_json::json!({"role":"user","content":"帮我做抖音内容"})];
        let candidate =
            domain_candidate_from_skill_slugs(&["douyin-content-creator".into()], &messages)
                .expect("candidate");
        assert_eq!(
            normalize_domain_key(&candidate.domain_key),
            "general.douyin.content.creator"
        );
        assert!(is_usable_domain_candidate(&candidate));
    }

    #[test]
    fn llm_domain_key_tag_preferred() {
        let signal = InterestSignal::new(
            "llm:1".into(),
            "抖音短视频创作".into(),
            "用户希望建立抖音内容生产流程".into(),
            0.2,
            vec![
                "creative.douyin.content".into(),
                "domain".into(),
                "creative".into(),
            ],
            SignalSource::Llm,
        );
        assert_eq!(
            domain_key_from_signal(&signal, "抖音短视频创作"),
            "creative.douyin.content"
        );
    }
}

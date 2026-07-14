//! LLM-based POI extraction and summarization via the auxiliary client.

use std::time::Duration;

use nomi_auxiliary::{AuxiliaryClient, AuxiliaryRequest, AuxiliaryTask, text_message};
use nomi_types::message::Role;
use tracing::debug;

use super::domain_taxonomy::domain_taxonomy_prompt_block;
use super::extract::parse_llm_topics_json;
use super::store::{InterestSignal, InterestTopic};

const INTEREST_LLM_TASK: &str = "interest";
const INTEREST_STARTER_LLM_TASK: &str = "interest_starter";
const MAX_USER_TRANSCRIPT_CHARS: usize = 12_000;

fn interest_llm_system_prompt() -> String {
    format!(
        r#"You extract durable user interest topics (POI) from real conversational utterances.

Users rarely say "my interest is X". They ask for help, state constraints, or describe situations.
Your job is semantic inference — do NOT rely on keyword lists or fixed categories.

Output ONLY a JSON array (no markdown fences). Each item:
{{"label": string, "summary": string, "confidence": 0-1, "tags": [string], "domain_key": string|null}}

Field rules:
- label: short human-readable topic (Chinese or English matching the user; ≤ 40 chars).
- summary: 1-3 sentences describing the user's ongoing concern or goal. Generalize and redact PII
  (no exact amounts, account numbers, names, addresses, employer names).
- confidence: 0-1 how durable/recurring this interest is (not how confident you are linguistically).
- tags: optional facets, e.g. "finance", "constraint", "career", "task".
- domain_key: best-matching taxonomy key when one fits, else null or a new snake_case key.

Quality rules:
- Max 4 items. Prefer 1-2 high-quality topics over noisy lists.
- Infer from meaning across the whole transcript — tasks, constraints, repeated themes, decisions.
- Skip pure chit-chat ("thanks", "ok", "hello") with no durable domain signal.
- One-off ephemeral lookups (e.g. today's weather) → omit unless they reveal a recurring domain.
- Merge near-duplicates; refine existing topics when listed below instead of cloning.
- For engineering sessions, prefer durable stack/domain over one-off file paths.

{taxonomy}"#,
        taxonomy = domain_taxonomy_prompt_block()
    )
}

/// Extract interest topics from **user-only** transcript text via auxiliary LLM routing.
pub async fn extract_signals_from_transcript_llm(
    auxiliary: &AuxiliaryClient,
    user_transcript: &str,
    existing_topic_labels: &[String],
) -> Vec<InterestSignal> {
    let trimmed = user_transcript.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let body = if trimmed.chars().count() > MAX_USER_TRANSCRIPT_CHARS {
        format!(
            "{}\n…[truncated]",
            trimmed
                .chars()
                .take(MAX_USER_TRANSCRIPT_CHARS)
                .collect::<String>()
        )
    } else {
        trimmed.to_string()
    };

    let existing_block = if existing_topic_labels.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nExisting user topics (merge or refine; do not duplicate):\n- {}\n",
            existing_topic_labels.join("\n- ")
        )
    };

    let user = format!(
        "Extract durable user interest topics from these user messages only.{existing_block}\n\n{body}"
    );

    let request = AuxiliaryRequest::new(
        AuxiliaryTask::Custom(INTEREST_LLM_TASK.to_string()),
        vec![
            text_message(Role::System, interest_llm_system_prompt()),
            text_message(Role::User, user),
        ],
    )
    .with_temperature(0.15)
    .with_max_tokens(1200)
    .with_timeout(Duration::from_secs(90));

    match auxiliary.call(request).await {
        Ok(resp) => {
            let text = resp.text().unwrap_or_default();
            let parsed = parse_llm_topics_json(text);
            if parsed.is_empty() && !text.trim().is_empty() {
                debug!(
                    chars = text.chars().count(),
                    "interest LLM returned no parseable topics"
                );
            }
            parsed
        }
        Err(err) => {
            tracing::warn!("interest LLM extraction failed: {err}");
            Vec::new()
        }
    }
}

fn starter_llm_system_prompt(count: usize) -> String {
    format!(
        r#"You write short conversation openers a user can click to start chatting with an AI assistant.

Output ONLY a JSON array of strings (no markdown fences). Exactly {count} items when possible (min 2, max {count}).

Each string must:
- Be a single actionable first message (imperative / request tone), ≤ 28 Chinese characters or ≤ 70 Latin characters
- Be concrete and useful for the given interest (not vague like "let's talk about X")
- Match the language of the interest label/summary
- Avoid PII, quotes wrapping the whole string, numbering, or bullet prefixes
- Be distinct from the others

Good: "帮我设计一套居家篮球训练周计划"
Bad: "聊聊篮球" / "Tell me about basketball""#
    )
}

/// Generate short Guid conversation starters for one interest topic.
pub async fn generate_starters_for_topic_llm(
    auxiliary: &AuxiliaryClient,
    topic: &InterestTopic,
    count: usize,
) -> Vec<String> {
    let count = count.clamp(2, 6);
    let tags = if topic.tags.is_empty() {
        String::from("(none)")
    } else {
        topic.tags.join(", ")
    };
    let user = format!(
        "Interest label: {}\nSummary: {}\nTags: {}\n\nWrite {count} short opener prompts.",
        topic.label.trim(),
        topic.summary.trim(),
        tags
    );

    let request = AuxiliaryRequest::new(
        AuxiliaryTask::Custom(INTEREST_STARTER_LLM_TASK.to_string()),
        vec![
            text_message(Role::System, starter_llm_system_prompt(count)),
            text_message(Role::User, user),
        ],
    )
    .with_temperature(0.55)
    .with_max_tokens(500)
    .with_timeout(Duration::from_secs(60));

    match auxiliary.call(request).await {
        Ok(resp) => {
            let text = resp.text().unwrap_or_default();
            let parsed = parse_starter_prompts_json(text, count);
            if parsed.is_empty() && !text.trim().is_empty() {
                debug!(
                    topic_id = %topic.id,
                    chars = text.chars().count(),
                    "interest starter LLM returned no parseable prompts"
                );
            }
            parsed
        }
        Err(err) => {
            tracing::warn!(
                topic_id = %topic.id,
                "interest starter LLM failed: {err}"
            );
            Vec::new()
        }
    }
}

fn parse_starter_prompts_json(text: &str, max: usize) -> Vec<String> {
    let trimmed = text.trim();
    let json_slice = extract_json_array(trimmed).unwrap_or(trimmed);
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json_slice) else {
        return Vec::new();
    };
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for item in arr {
        let raw = match item {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Object(map) => map
                .get("text")
                .or_else(|| map.get("prompt"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            _ => continue,
        };
        let cleaned = clean_starter_text(&raw);
        if cleaned.is_empty() {
            continue;
        }
        let key = cleaned.to_ascii_lowercase();
        if !seen.insert(key) {
            continue;
        }
        out.push(cleaned);
        if out.len() >= max {
            break;
        }
    }
    out
}

fn extract_json_array(text: &str) -> Option<&str> {
    let start = text.find('[')?;
    let end = text.rfind(']')?;
    if end <= start {
        return None;
    }
    Some(&text[start..=end])
}

fn clean_starter_text(raw: &str) -> String {
    let mut s = raw.trim().trim_matches(|c| c == '"' || c == '\'').to_string();
    // Strip leading list markers: "1. ", "- ", "• "
    let stripped = s
        .trim_start_matches(|c: char| c.is_ascii_digit())
        .trim_start_matches(['.', ')', '、', ' ', '-', '•', '*']);
    if stripped.len() < s.len() {
        s = stripped.trim().to_string();
    }
    let char_count = s.chars().count();
    if char_count < 4 || char_count > 80 {
        return String::new();
    }
    s
}

//! Conversation title auto-generation: trait definition and LLM-backed
//! implementation.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use nomifun_common::AppError;
use nomifun_db::IProviderRepository;
use tracing::warn;

use crate::factory::provider_config::{
    one_shot_completion_no_thinking, resolve_provider_config, user_message,
};
use crate::knowledge_completer::first_enabled_model;
use nomi_config::config::Config;

const TITLE_MAX_TOKENS: u32 = 128;
const TITLE_MAX_CHARS: usize = 24;
const TITLE_IDEAL_MAX_CHARS: usize = 16;

const TITLE_SYSTEM_EN: &str = "\
Write one short conversation title (3-7 words) for the exchange below. \
Same language as the exchange. \
Output format exactly (no other text):\nTITLE: <title>";

const TITLE_SYSTEM_ZH: &str = "\
根据对话内容生成一个简短标题（3-7个词或12字以内）。语言与对话一致。\
严格按此格式输出，不要任何解释：\nTITLE: 标题内容";

const TITLE_RETRY_ZH: &str = "只输出一行标题，格式：TITLE: 标题内容。不要解释。";

/// Auto-generate a short conversation title from the first user message.
#[async_trait]
pub trait ConversationTitleCompleter: Send + Sync {
    async fn summarize(&self, content: &str) -> Result<String, AppError>;
}

/// Provider-backed conversation title generator.
pub struct LiveConversationTitleCompleter {
    pub provider_repo: Arc<dyn IProviderRepository>,
    pub encryption_key: [u8; 32],
    pub workspace: PathBuf,
}

impl LiveConversationTitleCompleter {
    async fn resolve_default_model(&self) -> Result<(String, String), AppError> {
        let providers = self
            .provider_repo
            .list()
            .await
            .map_err(|e| AppError::Internal(format!("failed to list providers: {e}")))?;
        for provider in providers.iter().filter(|p| p.enabled) {
            if let Some(model) = first_enabled_model(&provider.models, provider.model_enabled.as_deref()) {
                return Ok((provider.id.clone(), model));
            }
        }
        Err(AppError::Conflict(
            "conversation auto-title unavailable: no enabled provider/model is configured".into(),
        ))
    }

    fn title_system_for(content: &str) -> &'static str {
        if content.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)) {
            TITLE_SYSTEM_ZH
        } else {
            TITLE_SYSTEM_EN
        }
    }

    async fn call_and_normalize(
        &self,
        cfg: &Config,
        system: &str,
        user_content: &str,
    ) -> Result<String, AppError> {
        let raw =
            one_shot_completion_no_thinking(cfg, system, vec![user_message(user_content)], TITLE_MAX_TOKENS)
                .await?;
        Ok(normalize_title_output(&raw))
    }
}

fn is_meta_title_line(line: &str) -> bool {
    let lower = line.to_lowercase();
    const MARKERS: &[&str] = &[
        "被要求",
        "生成一个短标题",
        "生成短标题",
        "短标题",
        "3-7个词",
        "3-7个",
        "3-7 words",
        "描述对话",
        "following exchange",
        "return only",
        "generate a short",
        "we need to generate",
        "short descriptive title",
        "given exchange",
        "for the conversation",
        "capture the main topic",
        "no quotes",
        "nothing else",
        "output format",
        "标题应该",
        "标题可以是",
        "所以标题",
        "最终标题",
        "我需要",
        "让我想",
        "让我来",
        "首先分析",
        "分析一下",
        "用户的问题是",
        "用户要求",
        "用户希望",
        "the title should",
        "i need to",
        "let me think",
        "let me ",
    ];
    MARKERS.iter().any(|m| line.contains(m) || lower.contains(m))
}

fn extract_tagged_title(raw: &str) -> Option<String> {
    for line in raw.lines().chain(std::iter::once(raw)) {
        let trimmed = line.trim();
        for marker in ["TITLE:", "TITLE：", "title:", "标题:", "标题："] {
            if let Some(rest) = trimmed.strip_prefix(marker) {
                let t = rest.trim().trim_end_matches(['。', '.', '！', '!', '？', '?']);
                if !t.is_empty() && !is_meta_title_line(t) {
                    return Some(t.to_owned());
                }
            }
            if let Some((_, rest)) = trimmed.split_once(marker) {
                let t = rest.trim().trim_end_matches(['。', '.', '！', '!', '？', '?']);
                if !t.is_empty() && !is_meta_title_line(t) {
                    return Some(t.to_owned());
                }
            }
        }
    }
    None
}

fn extract_after_title_marker(line: &str) -> Option<String> {
    const MARKERS: &[&str] = &[
        "best title is",
        "best title would be",
        "the title is",
        "so the title is",
        "i'll use",
        "i will use",
        "go with",
        "would be",
        "最终标题：",
        "最终标题:",
        "所以标题是",
        "标题是：",
        "标题是:",
        "应该是：",
        "应该是:",
    ];
    for marker in MARKERS {
        let lower = line.to_lowercase();
        let marker_lower = marker.to_lowercase();
        if let Some(idx) = lower.find(&marker_lower) {
            let rest = line[idx + marker.len()..].trim();
            let rest = rest
                .trim_start_matches([':', '：', ' '])
                .trim_matches(|c| c == '"' || c == '\'' || c == '「' || c == '」')
                .trim_end_matches(['。', '.', '！', '!', '？', '?']);
            if !rest.is_empty() && !is_meta_title_line(rest) {
                return Some(rest.to_owned());
            }
        }
    }
    None
}

fn extract_all_bracketed(raw: &str, open: char, close: char) -> Vec<String> {
    let mut results = Vec::new();
    let mut search_from = 0;
    while let Some(rel) = raw[search_from..].find(open) {
        let open_abs = search_from + rel;
        let after = &raw[open_abs + open.len_utf8()..];
        if let Some(close_rel) = after.find(close) {
            let inner = after[..close_rel].trim();
            if !inner.is_empty() {
                results.push(inner.to_owned());
            }
            search_from = open_abs + open.len_utf8() + close_rel + close.len_utf8();
        } else {
            break;
        }
    }
    results
}

fn extract_all_double_quoted(raw: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut search_from = 0;
    while let Some(rel) = raw[search_from..].find('"') {
        let open_abs = search_from + rel + 1;
        if let Some(close_rel) = raw[open_abs..].find('"') {
            let inner = raw[open_abs..open_abs + close_rel].trim();
            if !inner.is_empty() {
                results.push(inner.to_owned());
            }
            search_from = open_abs + close_rel + 1;
        } else {
            break;
        }
    }
    results
}

fn pick_best_short_candidate(candidates: impl IntoIterator<Item = String>) -> Option<String> {
    let mut best: Option<(usize, String)> = None;
    for t in candidates {
        let len = t.chars().count();
        if len == 0 || len > TITLE_MAX_CHARS || is_meta_title_line(&t) {
            continue;
        }
        let score = if len <= TITLE_IDEAL_MAX_CHARS { 0 } else { 1 };
        match &best {
            None => best = Some((score, t)),
            Some((best_score, _)) if score < *best_score => best = Some((score, t)),
            _ => {}
        }
    }
    best.map(|(_, t)| t)
}

fn split_segments(raw: &str) -> Vec<String> {
    let mut segments = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut buf = String::new();
        for c in line.chars() {
            buf.push(c);
            if matches!(c, '。' | '！' | '？' | '；' | '.' | '!' | '?' | ';' | '\n') {
                let s = buf.trim().to_string();
                if !s.is_empty() {
                    segments.push(s);
                }
                buf.clear();
            }
        }
        let tail = buf.trim();
        if !tail.is_empty() {
            segments.push(tail.to_string());
        }
    }
    if segments.is_empty() && !raw.trim().is_empty() {
        segments.push(raw.trim().to_string());
    }
    segments
}

fn pick_title_candidate(raw: &str) -> Option<String> {
    if let Some(t) = extract_tagged_title(raw) {
        return Some(t);
    }

    let tail_start = raw.len().saturating_sub(250);
    let tail = &raw[tail_start..];
    if let Some(t) = extract_tagged_title(tail) {
        return Some(t);
    }

    for seg in split_segments(tail).iter().rev().chain(split_segments(raw).iter().rev()) {
        if let Some(t) = extract_after_title_marker(seg) {
            return Some(t);
        }
    }

    let quoted: Vec<String> = extract_all_double_quoted(tail)
        .into_iter()
        .chain(extract_all_double_quoted(raw))
        .collect();
    if let Some(t) = pick_best_short_candidate(quoted) {
        return Some(t);
    }

    let bracketed: Vec<String> = extract_all_bracketed(tail, '「', '」')
        .into_iter()
        .chain(extract_all_bracketed(raw, '「', '」'))
        .collect();
    if let Some(t) = pick_best_short_candidate(bracketed) {
        return Some(t);
    }

    for seg in split_segments(tail).iter().rev().chain(split_segments(raw).iter().rev()) {
        let t = seg.trim();
        let len = t.chars().count();
        if len > 0 && len <= TITLE_IDEAL_MAX_CHARS && !is_meta_title_line(t) {
            return Some(t.to_owned());
        }
    }

    for seg in split_segments(tail).iter().rev().chain(split_segments(raw).iter().rev()) {
        let t = seg.trim();
        let len = t.chars().count();
        if len > 0 && len <= TITLE_MAX_CHARS && !is_meta_title_line(t) {
            return Some(t.to_owned());
        }
    }

    None
}

fn normalize_title_output(raw: &str) -> String {
    let candidate = match pick_title_candidate(raw) {
        Some(t) => t,
        None => {
            let trimmed = raw.trim();
            if trimmed.is_empty() || is_meta_title_line(trimmed) {
                return String::new();
            }
            trimmed.to_owned()
        }
    };

    let mut collapsed = String::new();
    let mut prev_space = false;
    for c in candidate.chars() {
        if c.is_whitespace() {
            if !prev_space {
                collapsed.push(' ');
                prev_space = true;
            }
        } else if !c.is_control() {
            collapsed.push(c);
            prev_space = false;
        }
    }
    let trimmed = collapsed
        .trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == '`' || c == '「' || c == '」');
    let stripped = trimmed
        .strip_prefix("TITLE:")
        .or_else(|| trimmed.strip_prefix("title:"))
        .or_else(|| trimmed.strip_prefix("标题:"))
        .or_else(|| trimmed.strip_prefix("标题："))
        .unwrap_or(trimmed)
        .trim()
        .trim_end_matches(['。', '.', '！', '!', '？', '?']);
    let out: String = stripped.chars().take(TITLE_MAX_CHARS).collect();
    let out = out.trim().to_owned();
    if is_meta_title_line(&out) {
        String::new()
    } else {
        out
    }
}

#[async_trait]
impl ConversationTitleCompleter for LiveConversationTitleCompleter {
    async fn summarize(&self, content: &str) -> Result<String, AppError> {
        let (provider_id, model) = self.resolve_default_model().await?;
        let cfg = resolve_provider_config(
            &self.provider_repo,
            &self.encryption_key,
            &provider_id,
            &model,
            &self.workspace,
        )
        .await?;

        let system = Self::title_system_for(content);

        let title = self.call_and_normalize(&cfg, system, content).await?;
        if !title.is_empty() {
            return Ok(title);
        }

        if content.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)) {
            let retry_user = format!(
                "请为以下对话起一个简短标题（不超过12字），严格按格式输出：\nTITLE: 标题\n\n{content}"
            );
            let title = self.call_and_normalize(&cfg, TITLE_RETRY_ZH, &retry_user).await?;
            if !title.is_empty() {
                return Ok(title);
            }
        }

        warn!(
            content_len = content.len(),
            "conversation auto-title: all LLM attempts failed to produce a title"
        );
        Ok(String::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_instruction_echo_reasoning() {
        let raw = "我们被要求生成一个短标题，3-7个词，描述对话的主题。";
        assert_eq!(normalize_title_output(raw), "");
    }

    #[test]
    fn picks_title_tagged_format() {
        let raw = "Some thinking...\nTITLE: 工具调用重复问题";
        assert_eq!(normalize_title_output(raw), "工具调用重复问题");
    }

    #[test]
    fn picks_title_from_english_reasoning_tail() {
        let raw = "We need to generate a short descriptive title (3-7 words) for the conversation that starts with the given exchange. The user reports duplicate tool calls in execution logs. So the title is: Duplicate Tool Calls";
        assert_eq!(normalize_title_output(raw), "Duplicate Tool Calls");
    }

    #[test]
    fn picks_last_quoted_short_title() {
        let raw = r#"We need to generate a title. "ignore this long meta string about titles" The best title is "工具调用重复""#;
        assert_eq!(normalize_title_output(raw), "工具调用重复");
    }

    #[test]
    fn picks_actual_title_from_reasoning() {
        let raw = "用户问如何修复登录问题。应该简洁。\n修复登录问题";
        assert_eq!(normalize_title_output(raw), "修复登录问题");
    }

    #[test]
    fn picks_title_after_marker() {
        let raw = "分析一下对话内容。最终标题：部署生产环境";
        assert_eq!(normalize_title_output(raw), "部署生产环境");
    }
}

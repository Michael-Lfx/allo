//! Conversation title auto-generation: trait definition and LLM-backed
//! implementation.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use nomifun_common::{AppError, ProviderWithModel};
use nomifun_db::{IProviderModelRepository, IProviderRepository};
use tracing::{info, warn};

use crate::factory::provider_config::{
    one_shot_completion_title, resolve_provider_config, user_message, TitleResponseChannel,
};
use nomi_config::config::Config;

const TITLE_MAX_TOKENS: u32 = 128;
const TITLE_MAX_CHARS: usize = 24;
const TITLE_IDEAL_MAX_CHARS: usize = 16;

const TITLE_SYSTEM_EN: &str = "\
Write one short title (3-7 words) for the user's message below. \
Use the same language as the user's message. \
Return exactly one line beginning with `TITLE:` followed by a concrete, \
specific title for that message. Even for a very short or simple message, \
summarize its actual subject or action instead of copying the prompt or using \
a generic label. Never output placeholders or template text such as `<title>`, \
`[title]`, `title`, `Untitled`, `New conversation`, or `Conversation`; never \
output an explanation or these instructions.";

const TITLE_SYSTEM_ZH: &str = "\
根据用户消息生成一个简短、具体的标题（中文12字以内，英文3-7个词），语言与用户消息一致。\
即使用户消息很短或只是一个简单输入，也要概括其中真实的主题或动作，不要直接照抄用户输入。\
只输出一行，以 `TITLE:` 开头，冒号后必须填写真实标题。严禁输出 `<title>`、`[title]`、`title`、\
`标题`、`标题内容`、`未命名会话` 等占位词或泛化名称，也不要输出解释、提示词或其他内容。";

struct NormalizedTitle {
    title: String,
    channel: TitleResponseChannel,
    response_chars: usize,
}

/// Outcome metadata for the single title task. The result carries no raw
/// provider output so callers can log the evidence without exposing content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationTitleResult {
    pub title: String,
    pub llm_call_count: u8,
    pub response_channel: Option<TitleResponseChannel>,
    pub response_chars: usize,
}

/// Auto-generate a short conversation title from the first user message.
#[async_trait]
pub trait ConversationTitleCompleter: Send + Sync {
    /// Resolve candidates locally and make at most one provider request. Once
    /// a provider request starts, no other provider or prompt is attempted.
    async fn summarize(
        &self,
        content: &str,
        candidates: &[ProviderWithModel],
    ) -> Result<ConversationTitleResult, AppError>;
}

/// Provider-backed conversation title generator.
pub struct LiveConversationTitleCompleter {
    pub provider_repo: Arc<dyn IProviderRepository>,
    pub provider_model_repo: Arc<dyn IProviderModelRepository>,
    pub encryption_key: [u8; 32],
    pub workspace: PathBuf,
}

impl LiveConversationTitleCompleter {
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
    ) -> Result<NormalizedTitle, AppError> {
        let completion = one_shot_completion_title(
            cfg,
            system,
            vec![user_message(user_content)],
            TITLE_MAX_TOKENS,
        )
        .await?;
        let response_chars = completion.output.chars().count();
        let title = if completion.channel == TitleResponseChannel::ReasoningFallback {
            normalize_reasoning_output(&completion.output)
        } else {
            normalize_title_output(&completion.output)
        };
        Ok(NormalizedTitle {
            title,
            channel: completion.channel,
            response_chars,
        })
    }
}

fn response_channel_name(channel: TitleResponseChannel) -> &'static str {
    channel.as_str()
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
        // Refusal / apology leakage: a model that declines the exchange must
        // never have its disclaimer stored as the conversation title.
        "抱歉",
        "对不起",
        "无法完成",
        "无法生成",
        "无法回答",
        "无法理解",
        "作为ai",
        "作为 ai",
        "as an ai",
        "i cannot",
        "i can't",
        "cannot assist",
        "cannot help",
    ];
    MARKERS.iter().any(|m| line.contains(m) || lower.contains(m))
}

fn is_placeholder_title(line: &str) -> bool {
    let normalized = line
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let normalized = normalized
        .trim_matches(|c| {
            matches!(
                c,
                '<' | '>' | '[' | ']' | '{' | '}' | '"' | '\'' | '`' | '「' | '」'
            )
        })
        .trim();

    matches!(
        normalized,
        "title"
            | "标题"
            | "标题内容"
            | "your title"
            | "conversation title"
            | "chat title"
            | "untitled"
            | "new conversation"
            | "conversation"
            | "chat"
            | "answer"
            | "request"
            | "未命名会话"
    )
}

fn extract_tagged_title(raw: &str) -> Option<String> {
    for line in raw.lines().chain(std::iter::once(raw)) {
        let trimmed = line.trim();
        for marker in ["TITLE:", "TITLE：", "title:", "标题:", "标题："] {
            if let Some(rest) = trimmed.strip_prefix(marker) {
                let t = rest.trim().trim_end_matches(['。', '.', '！', '!', '？', '?']);
                if !t.is_empty() && !is_meta_title_line(t) && !is_placeholder_title(t) {
                    return Some(t.to_owned());
                }
            }
            if let Some((_, rest)) = trimmed.split_once(marker) {
                let t = rest.trim().trim_end_matches(['。', '.', '！', '!', '？', '?']);
                if !t.is_empty() && !is_meta_title_line(t) && !is_placeholder_title(t) {
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
        // ASCII case folding preserves byte offsets for the original UTF-8
        // line. Full Unicode lowercasing can expand a preceding scalar and
        // make the slice below land in the middle of a character.
        let lower = line.to_ascii_lowercase();
        let marker_lower = marker.to_ascii_lowercase();
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

fn last_chars(raw: &str, max_chars: usize) -> &str {
    if max_chars == 0 {
        return "";
    }
    raw.char_indices()
        .rev()
        .nth(max_chars.saturating_sub(1))
        .map(|(index, _)| &raw[index..])
        .unwrap_or(raw)
}

fn pick_title_candidate(raw: &str) -> Option<String> {
    if let Some(t) = extract_tagged_title(raw) {
        return Some(t);
    }

    let tail = last_chars(raw, 250);
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

/// Clamp a display title by Unicode scalar values. CJK titles may be cut at
/// the character limit; Latin titles prefer the last complete word when the
/// limit cuts through a word.
pub fn clamp_title(title: &str) -> String {
    let trimmed = title.trim();
    if trimmed.chars().count() <= TITLE_MAX_CHARS {
        return trimmed.to_owned();
    }

    let prefix: String = trimmed.chars().take(TITLE_MAX_CHARS).collect();
    if prefix.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)) {
        return prefix.trim().to_owned();
    }

    prefix
        .rfind(char::is_whitespace)
        .map(|index| prefix[..index].trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or(prefix)
}

fn normalize_title_output(raw: &str) -> String {
    normalize_title_output_with_mode(raw, true)
}

fn normalize_reasoning_output(raw: &str) -> String {
    normalize_title_output_with_mode(raw, false)
}

fn normalize_title_output_with_mode(raw: &str, allow_long_unstructured: bool) -> String {
    let candidate = match pick_title_candidate(raw) {
        Some(t) => t,
        None => {
            let trimmed = raw.trim();
            if trimmed.is_empty()
                || is_meta_title_line(trimmed)
                || (!allow_long_unstructured && trimmed.chars().count() > TITLE_MAX_CHARS)
            {
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
    let out = clamp_title(stripped);
    if is_meta_title_line(&out) || is_placeholder_title(&out) {
        String::new()
    } else {
        out
    }
}

#[async_trait]
impl ConversationTitleCompleter for LiveConversationTitleCompleter {
    async fn summarize(
        &self,
        content: &str,
        candidates: &[ProviderWithModel],
    ) -> Result<ConversationTitleResult, AppError> {
        let system = Self::title_system_for(content);
        for (candidate_index, candidate) in candidates.iter().enumerate() {
            let cfg = match resolve_provider_config(
                &self.provider_repo,
                &self.provider_model_repo,
                &self.encryption_key,
                &candidate.provider_id,
                &candidate.model,
                &self.workspace,
            )
            .await
            {
                Ok(cfg) => cfg,
                Err(error) => {
                    warn!(
                        stage = "candidate_config",
                        candidate_rank = candidate_index + 1,
                        provider_id = %candidate.provider_id,
                        model = %candidate.model,
                        error_code = error.error_code(),
                        outcome = "candidate_config_failed",
                        "conversation auto-title: candidate config resolve failed"
                    );
                    continue;
                }
            };

            let started = Instant::now();
            let candidate_rank = candidate_index + 1;
            info!(
                stage = "candidate_selected",
                outcome = "candidate_selected",
                candidate_rank,
                provider_id = %candidate.provider_id,
                model = %candidate.model,
                llm_call_count = 0,
                "conversation auto-title candidate selected for the single request"
            );
            info!(
                stage = "llm",
                candidate_rank,
                provider_id = %candidate.provider_id,
                model = %candidate.model,
                llm_call_count = 1,
                outcome = "llm_started",
                "conversation auto-title: single LLM request started"
            );
            match self.call_and_normalize(&cfg, system, content).await {
                Ok(result) if !result.title.is_empty() => {
                    if result.channel == TitleResponseChannel::ReasoningFallback {
                        info!(
                            stage = "llm_completed",
                            candidate_rank,
                            provider_id = %candidate.provider_id,
                            model = %candidate.model,
                            response_channel = response_channel_name(result.channel),
                            response_chars = result.response_chars,
                            normalized_title_chars = result.title.chars().count(),
                            elapsed_ms = started.elapsed().as_millis(),
                            llm_call_count = 1,
                            outcome = "reasoning_fallback",
                            "conversation auto-title used reasoning from the same response"
                        );
                    }
                    info!(
                        stage = "llm_completed",
                        candidate_rank,
                        provider_id = %candidate.provider_id,
                        model = %candidate.model,
                        response_channel = response_channel_name(result.channel),
                        response_chars = result.response_chars,
                        normalized_title_chars = result.title.chars().count(),
                        elapsed_ms = started.elapsed().as_millis(),
                        llm_call_count = 1,
                        outcome = "llm_completed",
                        "conversation auto-title: single LLM request completed"
                    );
                    return Ok(ConversationTitleResult {
                        title: result.title,
                        llm_call_count: 1,
                        response_channel: Some(result.channel),
                        response_chars: result.response_chars,
                    });
                }
                Ok(result) => {
                    warn!(
                        stage = "llm_completed",
                        candidate_rank,
                        provider_id = %candidate.provider_id,
                        model = %candidate.model,
                        response_channel = response_channel_name(result.channel),
                        response_chars = result.response_chars,
                        normalized_title_chars = 0,
                        elapsed_ms = started.elapsed().as_millis(),
                        llm_call_count = 1,
                        outcome = "normalized_empty",
                        "conversation auto-title: single LLM request returned no usable title"
                    );
                    return Ok(ConversationTitleResult {
                        title: String::new(),
                        llm_call_count: 1,
                        response_channel: Some(result.channel),
                        response_chars: result.response_chars,
                    });
                }
                Err(error) => {
                    warn!(
                        stage = "llm",
                        candidate_rank,
                        provider_id = %candidate.provider_id,
                        model = %candidate.model,
                        error_code = error.error_code(),
                        elapsed_ms = started.elapsed().as_millis(),
                        llm_call_count = 1,
                        outcome = "llm_failed",
                        "conversation auto-title: single LLM request failed"
                    );
                    return Err(error);
                }
            }
        }

        warn!(
            stage = "candidate_selected",
            input_chars = content.chars().count(),
            candidates = candidates.len(),
            llm_call_count = 0,
            outcome = "no_candidate",
            "conversation auto-title: no candidate could be configured"
        );
        Ok(ConversationTitleResult {
            title: String::new(),
            llm_call_count: 0,
            response_channel: None,
            response_chars: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_instruction_echo_reasoning() {
        let raw = "我们被要求生成一个短标题，3-7个词，描述对话的主题。";
        assert_eq!(normalize_reasoning_output(raw), "");
    }

    #[test]
    fn rejects_refusal_leakage() {
        assert_eq!(normalize_title_output("抱歉，我无法完成这个请求。"), "");
        assert_eq!(normalize_title_output("对不起，作为AI我无法生成该标题"), "");
        assert_eq!(normalize_title_output("I'm sorry, as an AI I cannot help with that."), "");
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
    fn handles_long_unicode_reasoning_without_splitting_a_character() {
        let raw = format!("{}\nTITLE: 修复登录问题", "思考".repeat(200));
        assert_eq!(normalize_reasoning_output(&raw), "修复登录问题");
    }

    #[test]
    fn rejects_unstructured_long_reasoning_instead_of_clipping_it() {
        let raw = "这是模型内部的一大段思考内容，没有明确给出短标题，只是在解释它准备如何分析用户请求并选择结果";
        assert_eq!(normalize_reasoning_output(raw), "");
    }

    #[test]
    fn picks_title_after_marker() {
        let raw = "分析一下对话内容。最终标题：部署生产环境";
        assert_eq!(normalize_title_output(raw), "部署生产环境");
    }

    #[test]
    fn prompt_describes_a_user_message() {
        assert!(TITLE_SYSTEM_EN.contains("user's message"));
        assert!(TITLE_SYSTEM_ZH.contains("用户消息"));
        assert!(!TITLE_SYSTEM_EN.contains("exchange"));
        assert!(TITLE_SYSTEM_EN.contains("concrete"));
        assert!(TITLE_SYSTEM_ZH.contains("真实标题"));
        assert!(TITLE_SYSTEM_EN.contains("<title>"));
        assert!(TITLE_SYSTEM_ZH.contains("<title>"));
    }

    #[test]
    fn rejects_placeholder_titles() {
        for raw in [
            "TITLE: <title>",
            "TITLE: [title]",
            "标题：标题内容",
            "Untitled",
            "New conversation",
        ] {
            assert_eq!(normalize_title_output(raw), "", "raw={raw}");
        }
    }

    #[test]
    fn truncates_english_at_the_last_complete_word() {
        assert_eq!(
            normalize_title_output("TITLE: Fix the authentication timeout in production"),
            "Fix the authentication"
        );
    }

    #[test]
    fn truncates_cjk_by_unicode_character_count() {
        let title = normalize_title_output(
            "TITLE: 这是一个用于验证标题长度限制的超长中文标题内容示例",
        );
        assert_eq!(title.chars().count(), TITLE_MAX_CHARS);
        assert!(title.starts_with("这是一个用于验证标题长度限制"));
    }
}

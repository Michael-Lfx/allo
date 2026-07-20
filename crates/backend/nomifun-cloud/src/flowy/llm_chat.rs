//! OpenAI-compatible chat completions for media prompt refinement / ViMax planning.

use serde_json::{Value, json};
use std::time::Duration;

use crate::error::ServerClientError;
use crate::session::ServerSession;

use super::FlowyApiClient;

/// Empty completions are common with some flash / thinking models; retry a few times.
const EMPTY_CONTENT_RETRIES: u32 = 2;
const EMPTY_CONTENT_BACKOFF_MS: u64 = 800;

impl FlowyApiClient {
    /// Minimal non-streaming chat completion for side tasks (prompt refine, storyboard).
    pub async fn chat_completions_text(
        &self,
        session: &ServerSession,
        system: &str,
        user: &str,
        max_tokens: u32,
        temperature: f64,
        model: Option<&str>,
    ) -> Result<String, ServerClientError> {
        let system = system.trim();
        let user = user.trim();
        if system.is_empty() || user.is_empty() {
            return Err(ServerClientError::InvalidResponse(format!(
                "refusing LLM call with empty prompt (system_len={}, user_len={})",
                system.len(),
                user.len()
            )));
        }

        let model = model
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.config().effective_default_llm_model());

        // Side-task JSON generation does not need chain-of-thought. Disabling
        // thinking reduces empty-`content` failures on Minimax-M3 / similar.
        let body = json!({
            "model": model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "max_tokens": max_tokens,
            "temperature": temperature,
            "stream": false,
            "thinking": {"type": "disabled"},
        });

        self.chat_completions_with_retry(
            session,
            body,
            system.len(),
            user.len(),
            "chat completion returned empty content",
        )
        .await
    }

    /// Multimodal chat completion — `user_parts` is an OpenAI content-parts array
    /// (text + image_url). Used by ViMax vision steps (reference selection, etc.).
    pub async fn chat_completions_multimodal(
        &self,
        session: &ServerSession,
        system: &str,
        user_parts: Value,
        max_tokens: u32,
        temperature: f64,
        model: Option<&str>,
    ) -> Result<String, ServerClientError> {
        let system = system.trim();
        if system.is_empty() {
            return Err(ServerClientError::InvalidResponse(
                "refusing multimodal LLM call with empty system prompt".into(),
            ));
        }
        let user_len = estimate_user_parts_len(&user_parts);
        if user_len == 0 {
            return Err(ServerClientError::InvalidResponse(
                "refusing multimodal LLM call with empty user content".into(),
            ));
        }

        let model = model
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.config().effective_default_llm_model());

        let body = json!({
            "model": model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user_parts},
            ],
            "max_tokens": max_tokens,
            "temperature": temperature,
            "stream": false,
            "thinking": {"type": "disabled"},
        });

        self.chat_completions_with_retry(
            session,
            body,
            system.len(),
            user_len,
            "multimodal chat completion returned empty content",
        )
        .await
    }

    async fn chat_completions_with_retry(
        &self,
        session: &ServerSession,
        mut body: Value,
        system_len: usize,
        user_len: usize,
        empty_msg: &str,
    ) -> Result<String, ServerClientError> {
        let mut last_detail = String::new();
        let mut thinking_stripped = false;

        for attempt in 0..=EMPTY_CONTENT_RETRIES {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(
                    EMPTY_CONTENT_BACKOFF_MS * u64::from(attempt),
                ))
                .await;
            }
            let value = match self
                .post_upstream_json(&self.llm_transport, "/chat/completions", session, body.clone())
                .await
            {
                Ok(v) => v,
                Err(e) if !thinking_stripped && is_unknown_thinking_param(&e) => {
                    // Some gateways reject `thinking`; strip and retry once from scratch.
                    body.as_object_mut().map(|o| o.remove("thinking"));
                    thinking_stripped = true;
                    self.post_upstream_json(
                        &self.llm_transport,
                        "/chat/completions",
                        session,
                        body.clone(),
                    )
                    .await?
                }
                Err(e) => return Err(e),
            };

            let content = extract_chat_content(&value);
            if !content.is_empty() {
                return Ok(content);
            }
            last_detail = describe_empty_completion(&value, system_len, user_len);
        }

        Err(ServerClientError::InvalidResponse(format!(
            "{empty_msg}: {last_detail}"
        )))
    }

    /// OpenAI-compatible embeddings for RAG (`POST {llm}/embeddings`).
    /// Returns one vector per input string. Callers should fall back if unsupported.
    pub async fn embeddings(
        &self,
        session: &ServerSession,
        inputs: &[String],
        model: Option<&str>,
    ) -> Result<Vec<Vec<f32>>, ServerClientError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        let model = model
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                // Prefer an embedding-style id when unset; server may remap.
                "text-embedding-3-small".to_string()
            });

        let body = json!({
            "model": model,
            "input": inputs,
        });

        let value: Value = self
            .post_upstream_json(&self.llm_transport, "/embeddings", session, body)
            .await?;

        let mut out: Vec<(usize, Vec<f32>)> = Vec::new();
        if let Some(arr) = value.get("data").and_then(|d| d.as_array()) {
            for item in arr {
                let idx = item.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let emb = item
                    .get("embedding")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_f64().map(|f| f as f32))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                if !emb.is_empty() {
                    out.push((idx, emb));
                }
            }
        }
        out.sort_by_key(|(i, _)| *i);
        let vectors: Vec<Vec<f32>> = out.into_iter().map(|(_, v)| v).collect();
        if vectors.len() != inputs.len() {
            return Err(ServerClientError::InvalidResponse(format!(
                "embeddings count mismatch: got {} want {}",
                vectors.len(),
                inputs.len()
            )));
        }
        Ok(vectors)
    }
}

fn is_unknown_thinking_param(err: &ServerClientError) -> bool {
    let s = err.to_string().to_ascii_lowercase();
    s.contains("thinking")
        && (s.contains("unknown")
            || s.contains("invalid")
            || s.contains("unexpected")
            || s.contains("not support")
            || s.contains("unsupported"))
}

fn estimate_user_parts_len(parts: &Value) -> usize {
    match parts {
        Value::String(s) => s.trim().len(),
        Value::Array(arr) => arr
            .iter()
            .map(|p| {
                p.get("text")
                    .and_then(Value::as_str)
                    .map(|t| t.trim().len())
                    .unwrap_or(0)
                    + if p.get("image_url").is_some() { 1 } else { 0 }
            })
            .sum(),
        _ => 0,
    }
}

/// If Flowy wraps OpenAI payloads as `{code,msg,data}`, unwrap `data`.
fn unwrap_completion_payload(value: &Value) -> &Value {
    if value.get("choices").is_some() {
        return value;
    }
    if let Some(data) = value.get("data") {
        if data.get("choices").is_some() {
            return data;
        }
        // Rare: data is itself a choice message.
        if data.get("message").is_some() || data.get("content").is_some() {
            return data;
        }
    }
    value
}

/// Pull assistant text from OpenAI-compatible chat completion payloads.
/// Handles string content, multimodal content arrays, reasoning_content fallback,
/// and Flowy `{code,data}` envelopes.
fn extract_chat_content(value: &Value) -> String {
    let payload = unwrap_completion_payload(value);
    let message = payload
        .pointer("/choices/0/message")
        .or_else(|| payload.get("message"))
        .unwrap_or(payload);

    let mut text = extract_content_field(message.get("content"));
    if text.is_empty() {
        // Minimax / thinking models may put the answer here when reasoning_split is on,
        // or when content is null after thinking exhausts the budget.
        text = extract_content_field(message.get("reasoning_content"));
    }
    if text.is_empty() {
        text = extract_content_field(message.get("reasoning"));
    }
    if text.is_empty() {
        // Some gateways put the final answer at choice-level `text`.
        text = payload
            .pointer("/choices/0/text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
    }

    let stripped = strip_think_tags(&text);
    if !stripped.is_empty() {
        return stripped;
    }
    // If stripping removed everything, keep original (might still be useful).
    text
}

fn extract_content_field(content: Option<&Value>) -> String {
    let Some(content) = content else {
        return String::new();
    };
    match content {
        Value::Null => String::new(),
        Value::String(s) => s.trim().to_string(),
        Value::Array(parts) => {
            let mut out = String::new();
            for part in parts {
                let piece = part
                    .get("text")
                    .and_then(Value::as_str)
                    .or_else(|| part.get("content").and_then(Value::as_str))
                    .or_else(|| part.get("output_text").and_then(Value::as_str))
                    .or_else(|| part.as_str())
                    .unwrap_or("");
                // Skip pure thinking blocks when a typed array is used.
                let ty = part
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if matches!(ty.as_str(), "thinking" | "reasoning" | "thought") {
                    continue;
                }
                if piece.is_empty() {
                    continue;
                }
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(piece);
            }
            out.trim().to_string()
        }
        _ => String::new(),
    }
}

/// Remove common chain-of-thought wrappers so JSON parsers see the answer.
fn strip_think_tags(s: &str) -> String {
    let mut out = s.to_string();
    for (open, close) in [
        ("<think>", "</think>"),
        ("<thinking>", "</thinking>"),
        ("<reasoning>", "</reasoning>"),
        ("<redacted_reasoning>", "</redacted_reasoning>"),
    ] {
        while let Some(start) = out.find(open) {
            if let Some(rel_end) = out[start..].find(close) {
                let end = start + rel_end + close.len();
                out.replace_range(start..end, "");
            } else {
                // Unclosed tag — drop from open marker to end.
                out.truncate(start);
                break;
            }
        }
    }
    out.trim().to_string()
}

fn describe_empty_completion(value: &Value, system_len: usize, user_len: usize) -> String {
    let payload = unwrap_completion_payload(value);
    let choice = payload.pointer("/choices/0");
    let message = choice.and_then(|c| c.get("message"));
    let finish = choice
        .and_then(|c| c.get("finish_reason"))
        .and_then(Value::as_str)
        .unwrap_or("-");
    let content = message.and_then(|m| m.get("content"));
    let content_desc = match content {
        None => "missing".to_string(),
        Some(Value::Null) => "null".to_string(),
        Some(Value::String(s)) => format!("string(len={})", s.len()),
        Some(Value::Array(a)) => format!("array(len={})", a.len()),
        Some(other) => format!("other({})", other_type_name(other)),
    };
    let has_reasoning = message
        .and_then(|m| m.get("reasoning_content").or_else(|| m.get("reasoning")))
        .map(|v| match v {
            Value::String(s) => !s.trim().is_empty(),
            Value::Array(a) => !a.is_empty(),
            _ => !v.is_null(),
        })
        .unwrap_or(false);
    let top_keys: Vec<&str> = value
        .as_object()
        .map(|o| o.keys().map(|k| k.as_str()).collect())
        .unwrap_or_default();
    let snippet = truncate_chars(&value.to_string(), 400);
    format!(
        "system_len={system_len} user_len={user_len} finish_reason={finish} \
         content={content_desc} has_reasoning_content={has_reasoning} \
         top_keys=[{}] body_snippet={snippet}",
        top_keys.join(",")
    )
}

fn other_type_name(v: &Value) -> &'static str {
    match v {
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::Object(_) => "object",
        _ => "unknown",
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    let trimmed: String = s.chars().take(max).collect();
    format!("{trimmed}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_plain_string_content() {
        let v = json!({"choices":[{"message":{"content":"  hello  "}}]});
        assert_eq!(extract_chat_content(&v), "hello");
    }

    #[test]
    fn extracts_from_flowy_envelope() {
        let v = json!({
            "code": 200,
            "msg": "ok",
            "data": {"choices":[{"message":{"content":"{\"a\":1}"}}]}
        });
        assert_eq!(extract_chat_content(&v), "{\"a\":1}");
    }

    #[test]
    fn falls_back_to_reasoning_content() {
        let v = json!({
            "choices":[{"message":{
                "content": null,
                "reasoning_content": "{\"characters\":[]}"
            }}]
        });
        assert_eq!(extract_chat_content(&v), "{\"characters\":[]}");
    }

    #[test]
    fn strips_think_tags() {
        let v = json!({
            "choices":[{"message":{
                "content": "<think>plan</think>\n{\"ok\":true}"
            }}]
        });
        assert_eq!(extract_chat_content(&v), "{\"ok\":true}");
    }

    #[test]
    fn skips_thinking_typed_blocks() {
        let v = json!({
            "choices":[{"message":{"content":[
                {"type":"thinking","thinking":"hmm"},
                {"type":"text","text":"{\"x\":1}"}
            ]}}]
        });
        assert_eq!(extract_chat_content(&v), "{\"x\":1}");
    }

    #[test]
    fn describe_includes_prompt_lengths() {
        let v = json!({"choices":[{"finish_reason":"length","message":{"content":""}}]});
        let d = describe_empty_completion(&v, 120, 3400);
        assert!(d.contains("system_len=120"));
        assert!(d.contains("user_len=3400"));
        assert!(d.contains("finish_reason=length"));
        assert!(d.contains("content=string(len=0)"));
    }
}

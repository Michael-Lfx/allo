//! Persist folded/snipped transcript text so the model can Read/Grep it
//! after cheap or LLM compaction drops the live messages.

use std::path::Path;

use nomi_types::message::{ContentBlock, Message, Role};

/// Workspace-relative archive root: `{cwd}/.flowy/context-archive/`.
pub const ARCHIVE_REL_DIR: &str = ".flowy/context-archive";

const MAX_BLOCK_CHARS: usize = 16_384;
const MAX_FILE_CHARS: usize = 2 * 1024 * 1024;

/// Write `messages` to `{cwd}/.flowy/context-archive/{session_id}/{stamp}_{reason}.md`.
///
/// Returns a workspace-relative path with forward slashes, or `None` when
/// the workspace is unknown or the write fails (compaction must not fail).
pub fn write_archive(
    cwd: Option<&Path>,
    session_id: Option<&str>,
    reason: &str,
    messages: &[Message],
) -> Option<String> {
    let cwd = cwd?;
    if messages.is_empty() {
        return None;
    }
    let session = sanitize_segment(session_id.unwrap_or("anon"));
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let reason = sanitize_segment(reason);
    let dir = cwd.join(".flowy").join("context-archive").join(&session);
    if std::fs::create_dir_all(&dir).is_err() {
        return None;
    }
    let gitignore = cwd.join(".flowy").join("context-archive").join(".gitignore");
    if !gitignore.exists() {
        let _ = std::fs::write(&gitignore, "*\n");
    }
    let file_name = format!("{stamp}_{reason}.md");
    let path = dir.join(&file_name);
    let body = render_messages(messages);
    if std::fs::write(&path, body).is_err() {
        tracing::warn!(
            target: "nomi_agent",
            path = %path.display(),
            "failed to write context archive; continuing compact"
        );
        return None;
    }
    Some(format!("{ARCHIVE_REL_DIR}/{session}/{file_name}").replace('\\', "/"))
}

fn sanitize_segment(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    if cleaned.is_empty() {
        "anon".into()
    } else {
        cleaned
    }
}

fn render_messages(messages: &[Message]) -> String {
    let mut out = String::from("# Context archive\n\n");
    out.push_str("Dropped from the live transcript. Read or Grep this file if you need details.\n\n");
    for (i, msg) in messages.iter().enumerate() {
        if out.len() >= MAX_FILE_CHARS {
            out.push_str("\n\n…[archive truncated]\n");
            break;
        }
        let role = match msg.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
            Role::Tool => "tool",
        };
        out.push_str(&format!("## {i} {role}\n\n"));
        for block in &msg.content {
            match block {
                ContentBlock::Text { text } => {
                    out.push_str(&truncate_chars(text, MAX_BLOCK_CHARS));
                    out.push_str("\n\n");
                }
                ContentBlock::Thinking { thinking, .. } => {
                    out.push_str("_thinking_\n");
                    out.push_str(&truncate_chars(thinking, MAX_BLOCK_CHARS / 4));
                    out.push_str("\n\n");
                }
                ContentBlock::ToolUse { name, input, .. } => {
                    let input = serde_json::to_string(input).unwrap_or_default();
                    out.push_str(&format!("**tool_use {name}**\n```\n"));
                    out.push_str(&truncate_chars(&input, MAX_BLOCK_CHARS / 4));
                    out.push_str("\n```\n\n");
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                    ..
                } => {
                    out.push_str(&format!(
                        "**tool_result** `{}`{}\n```\n",
                        tool_use_id,
                        if *is_error { " error" } else { "" }
                    ));
                    out.push_str(&truncate_chars(content, MAX_BLOCK_CHARS));
                    out.push_str("\n```\n\n");
                }
                ContentBlock::Image { .. } => {
                    out.push_str("_image omitted_\n\n");
                }
            }
        }
    }
    out
}

fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let trimmed: String = text.chars().take(max).collect();
    format!("{trimmed}\n…[truncated]")
}

/// Relative-path notice appended to compact summaries and snip collapse lines.
pub fn archive_notice(rel_path: &str) -> String {
    format!(
        "Full prior transcript archived at `{rel_path}`. Read or Grep that file if you need details."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_types::message::ContentBlock;
    use tempfile::tempdir;

    #[test]
    fn writes_markdown_under_flowy() {
        let dir = tempdir().unwrap();
        let messages = vec![Message::now(
            Role::User,
            vec![ContentBlock::Text {
                text: "hello archive".into(),
            }],
        )];
        let rel = write_archive(Some(dir.path()), Some("sess-1"), "snip", &messages).unwrap();
        assert!(rel.starts_with(".flowy/context-archive/sess-1/"));
        let abs = dir.path().join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        let body = std::fs::read_to_string(abs).unwrap();
        assert!(body.contains("hello archive"));
    }
}

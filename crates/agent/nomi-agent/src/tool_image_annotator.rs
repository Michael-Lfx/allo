//! Host hook that turns tool-result images into text for models that cannot
//! see them. The engine keeps the bytes for the UI / session; the provider
//! adapter still drops them when `supports_image` is false.

use async_trait::async_trait;

use nomi_types::message::ContentBlock;
use nomi_types::tool::ToolImage;

/// Analyze tool-result screenshots so a text-only conversation model can act
/// on Computer / Read / Browser observations instead of seeing a stripped
/// attachment.
#[async_trait]
pub trait ToolResultImageAnnotator: Send + Sync {
    async fn annotate_tool_images(&self, tool_name: &str, images: &[ToolImage]) -> Option<String>;
}

/// Append sidecar descriptions onto tool results that still carry images.
/// Callers invoke this after the UI has already received the binary payload
/// and only when the conversation model cannot take image input.
pub async fn annotate_tool_result_images(
    annotator: &dyn ToolResultImageAnnotator,
    tool_calls: &[ContentBlock],
    results: &mut [ContentBlock],
) {
    for result in results {
        let ContentBlock::ToolResult {
            tool_use_id,
            content,
            images,
            ..
        } = result
        else {
            continue;
        };
        if images.is_empty() {
            continue;
        }
        let tool_name = tool_calls
            .iter()
            .find_map(|call| match call {
                ContentBlock::ToolUse { id, name, .. } if id == tool_use_id => Some(name.as_str()),
                _ => None,
            })
            .unwrap_or("unknown");
        let Some(note) = annotator.annotate_tool_images(tool_name, images).await else {
            continue;
        };
        if note.is_empty() {
            continue;
        }
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str(&note);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct RecordingAnnotator {
        calls: Mutex<Vec<(String, usize)>>,
    }

    #[async_trait]
    impl ToolResultImageAnnotator for RecordingAnnotator {
        async fn annotate_tool_images(
            &self,
            tool_name: &str,
            images: &[ToolImage],
        ) -> Option<String> {
            self.calls
                .lock()
                .expect("annotator lock")
                .push((tool_name.to_owned(), images.len()));
            Some(format!("saw {tool_name} x{}", images.len()))
        }
    }

    fn png() -> ToolImage {
        ToolImage {
            media_type: "image/png".into(),
            data: "aaa".into(),
        }
    }

    #[tokio::test]
    async fn annotates_each_image_bearing_tool_result() {
        let annotator = RecordingAnnotator {
            calls: Mutex::new(Vec::new()),
        };
        let calls = vec![
            ContentBlock::ToolUse {
                id: "c1".into(),
                name: "Computer".into(),
                input: serde_json::json!({"action": "observe"}),
                extra: None,
            },
            ContentBlock::ToolUse {
                id: "c2".into(),
                name: "Read".into(),
                input: serde_json::json!({"path": "shot.png"}),
                extra: None,
            },
        ];
        let mut results = vec![
            ContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: "tree".into(),
                is_error: false,
                images: vec![png()],
            },
            ContentBlock::ToolResult {
                tool_use_id: "c2".into(),
                content: String::new(),
                is_error: false,
                images: vec![png(), png()],
            },
            ContentBlock::ToolResult {
                tool_use_id: "c3".into(),
                content: "no image".into(),
                is_error: false,
                images: Vec::new(),
            },
        ];

        annotate_tool_result_images(&annotator, &calls, &mut results).await;

        let ContentBlock::ToolResult { content, .. } = &results[0] else {
            panic!("expected tool result");
        };
        assert_eq!(content, "tree\nsaw Computer x1");
        let ContentBlock::ToolResult { content, .. } = &results[1] else {
            panic!("expected tool result");
        };
        assert_eq!(content, "saw Read x2");
        assert_eq!(
            *annotator.calls.lock().expect("annotator lock"),
            vec![("Computer".into(), 1), ("Read".into(), 2)]
        );
    }
}

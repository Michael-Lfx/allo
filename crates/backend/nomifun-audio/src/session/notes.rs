//! Meeting notes (N3): structured summary generation from transcript.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Generation lifecycle for meeting notes on a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeetingNotesStatus {
    None,
    Generating,
    Ready,
    Failed,
}

impl MeetingNotesStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Generating => "generating",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "none" => Some(Self::None),
            "generating" => Some(Self::Generating),
            "ready" => Some(Self::Ready),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeetingNotesSource {
    Llm,
    Template,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MeetingNoteTodo {
    pub title: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub assignee: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MeetingSpeakerHighlight {
    pub speaker: String,
    pub highlight: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingNotes {
    pub summary: String,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub todos: Vec<MeetingNoteTodo>,
    #[serde(default)]
    pub risks: Vec<String>,
    #[serde(default)]
    pub speaker_highlights: Vec<MeetingSpeakerHighlight>,
    pub source: MeetingNotesSource,
    pub generated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingNotesView {
    pub status: MeetingNotesStatus,
    pub notes: Option<MeetingNotes>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateMeetingNotesResult {
    pub notes: MeetingNotes,
    pub posted_to_conversation: bool,
    pub created_requirement_ids: Vec<String>,
}

/// LLM one-shot for meeting notes. Implemented by the app layer with
/// `one_shot_completion`; absence falls back to the local template.
#[async_trait]
pub trait MeetingNotesCompleter: Send + Sync {
    async fn complete(&self, system: &str, user: &str) -> Result<String, String>;
}

/// Posts generated notes into a bound Agent conversation without starting a turn.
#[async_trait]
pub trait MeetingNotesConversationSink: Send + Sync {
    async fn post_notes(&self, conversation_id: &str, markdown: &str) -> Result<(), String>;
}

pub const NOTES_SYSTEM: &str = "You are a meeting notes assistant. Given a transcript, \
output ONLY one JSON object (no markdown fences, no prose) with this exact shape:\n\
{\"summary\":\"...\",\"decisions\":[\"...\"],\"todos\":[{\"title\":\"...\",\"detail\":\"...\",\"assignee\":null}],\
\"risks\":[\"...\"],\"speaker_highlights\":[{\"speaker\":\"...\",\"highlight\":\"...\"}]}\n\
Rules: use the transcript language; keep summary to 2-5 sentences; decisions/todos/risks \
are short bullet strings; todos.title is required and actionable; omit empty arrays rather \
than inventing content; if the transcript is empty or useless, still return valid JSON with \
an honest short summary.";

#[derive(Debug, Deserialize)]
struct LlmNotesPayload {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    decisions: Vec<String>,
    #[serde(default)]
    todos: Vec<LlmTodo>,
    #[serde(default)]
    risks: Vec<String>,
    #[serde(default)]
    speaker_highlights: Vec<MeetingSpeakerHighlight>,
}

#[derive(Debug, Deserialize)]
struct LlmTodo {
    #[serde(default)]
    title: String,
    #[serde(default)]
    detail: String,
    #[serde(default)]
    assignee: Option<String>,
}

pub fn build_transcript(segments: &[(String, String)]) -> String {
    let mut lines = Vec::with_capacity(segments.len());
    for (speaker, text) in segments {
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let speaker = if speaker.trim().is_empty() {
            "Speaker"
        } else {
            speaker.trim()
        };
        lines.push(format!("{speaker}: {text}"));
    }
    lines.join("\n")
}

pub fn template_notes_from_transcript(transcript: &str, generated_at_ms: i64) -> MeetingNotes {
    let trimmed = transcript.trim();
    let summary = if trimmed.is_empty() {
        "No transcript available for this meeting.".to_string()
    } else {
        let preview: String = trimmed.chars().take(400).collect();
        format!("Meeting transcript summary (template fallback):\n{preview}")
    };

    let mut todos = Vec::new();
    for line in trimmed.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.contains("todo")
            || lower.contains("action item")
            || line.contains("待办")
            || line.contains("行动项")
        {
            let title = line.trim().chars().take(120).collect::<String>();
            if !title.is_empty() {
                todos.push(MeetingNoteTodo {
                    title,
                    detail: String::new(),
                    assignee: None,
                });
            }
        }
    }

    MeetingNotes {
        summary,
        decisions: Vec::new(),
        todos,
        risks: Vec::new(),
        speaker_highlights: Vec::new(),
        source: MeetingNotesSource::Template,
        generated_at_ms,
    }
}

pub fn parse_llm_notes(raw: &str, generated_at_ms: i64) -> Option<MeetingNotes> {
    let json = extract_json_object(raw)?;
    let payload: LlmNotesPayload = serde_json::from_str(&json).ok()?;
    let summary = payload.summary.trim().to_string();
    if summary.is_empty() {
        return None;
    }
    let todos = payload
        .todos
        .into_iter()
        .filter_map(|t| {
            let title = t.title.trim().to_string();
            if title.is_empty() {
                None
            } else {
                Some(MeetingNoteTodo {
                    title,
                    detail: t.detail,
                    assignee: t.assignee,
                })
            }
        })
        .collect();
    Some(MeetingNotes {
        summary,
        decisions: payload
            .decisions
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        todos,
        risks: payload
            .risks
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        speaker_highlights: payload
            .speaker_highlights
            .into_iter()
            .filter(|h| !h.speaker.trim().is_empty() && !h.highlight.trim().is_empty())
            .collect(),
        source: MeetingNotesSource::Llm,
        generated_at_ms,
    })
}

fn extract_json_object(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if v.is_object() {
            return Some(trimmed.to_string());
        }
    }
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    if end <= start {
        return None;
    }
    let slice = &trimmed[start..=end];
    serde_json::from_str::<serde_json::Value>(slice)
        .ok()
        .filter(|v| v.is_object())
        .map(|_| slice.to_string())
}

pub fn notes_to_markdown(notes: &MeetingNotes) -> String {
    let mut out = String::from("## Meeting notes\n\n");
    out.push_str("### Summary\n");
    out.push_str(&notes.summary);
    out.push_str("\n\n");
    if !notes.decisions.is_empty() {
        out.push_str("### Decisions\n");
        for item in &notes.decisions {
            out.push_str(&format!("- {item}\n"));
        }
        out.push('\n');
    }
    if !notes.todos.is_empty() {
        out.push_str("### Todos\n");
        for item in &notes.todos {
            if item.detail.trim().is_empty() {
                out.push_str(&format!("- {title}\n", title = item.title));
            } else {
                out.push_str(&format!(
                    "- {title}: {detail}\n",
                    title = item.title,
                    detail = item.detail
                ));
            }
        }
        out.push('\n');
    }
    if !notes.risks.is_empty() {
        out.push_str("### Risks\n");
        for item in &notes.risks {
            out.push_str(&format!("- {item}\n"));
        }
        out.push('\n');
    }
    if !notes.speaker_highlights.is_empty() {
        out.push_str("### Speaker highlights\n");
        for item in &notes.speaker_highlights {
            out.push_str(&format!("- **{}**: {}\n", item.speaker, item.highlight));
        }
        out.push('\n');
    }
    out.push_str(&format!(
        "_Source: {}_\n",
        match notes.source {
            MeetingNotesSource::Llm => "llm",
            MeetingNotesSource::Template => "template",
        }
    ));
    out
}

pub fn parse_stored_notes(json: Option<&str>) -> Option<MeetingNotes> {
    let raw = json?;
    serde_json::from_str(raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_notes_extracts_todo_lines() {
        let notes = template_notes_from_transcript(
            "Alice: hello\nBob: TODO ship notes API\nCarol: 待办 跟进预算",
            1,
        );
        assert_eq!(notes.source, MeetingNotesSource::Template);
        assert_eq!(notes.todos.len(), 2);
    }

    #[test]
    fn parse_llm_notes_tolerates_fence() {
        let raw = "```json\n{\"summary\":\"Done\",\"decisions\":[\"Ship\"],\"todos\":[{\"title\":\"Write tests\"}],\"risks\":[],\"speaker_highlights\":[]}\n```";
        let notes = parse_llm_notes(raw, 42).unwrap();
        assert_eq!(notes.summary, "Done");
        assert_eq!(notes.decisions, vec!["Ship".to_string()]);
        assert_eq!(notes.todos[0].title, "Write tests");
        assert_eq!(notes.generated_at_ms, 42);
    }
}

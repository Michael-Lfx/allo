use std::sync::Arc;

use serde::Deserialize;

use crate::backends::VimaxChat;
use crate::domain::Event;
use crate::error::{VimaxError, VimaxResult};
use crate::json_util::complete_and_parse_llm_json;

use super::formats::EVENT;

const MAX_GET_NEXT: usize = 50;
const BATCH_CHAR_LIMIT: usize = 12_000;
const SPLIT_CHUNK: usize = 4000;
const SPLIT_OVERLAP: usize = 400;

pub struct EventExtractor {
    chat: Arc<dyn VimaxChat>,
}

impl EventExtractor {
    pub fn new(chat: Arc<dyn VimaxChat>) -> Self {
        Self { chat }
    }

    pub async fn extract_all(&self, compressed: &str) -> VimaxResult<Vec<Event>> {
        match self.try_extract_batch(compressed).await {
            Ok(events) if !events.is_empty() => return Ok(events),
            Ok(_) => tracing::warn!("batch event extraction returned empty; falling back to get-next"),
            Err(e) => tracing::warn!(error = %e, "batch event extraction failed; falling back to get-next"),
        }
        self.extract_all_get_next(compressed).await
    }

    async fn try_extract_batch(&self, compressed: &str) -> VimaxResult<Vec<Event>> {
        if compressed.chars().count() <= BATCH_CHAR_LIMIT {
            return self.extract_batch_once(compressed).await;
        }
        let mut merged: Vec<Event> = Vec::new();
        for piece in split_text(compressed, SPLIT_CHUNK, SPLIT_OVERLAP) {
            let mut chunk_events = self.extract_batch_once(&piece).await?;
            merge_events(&mut merged, &mut chunk_events);
        }
        reindex_events(&mut merged);
        Ok(merged)
    }

    async fn extract_batch_once(&self, compressed: &str) -> VimaxResult<Vec<Event>> {
        let system = include_str!(
            "../../prompts/event_extractor__system_prompt_template_extract_events.txt"
        )
        .replace("{format_instructions}", EVENTS_BATCH);
        let user = format!(
            "<NOVEL_TEXT_START>\n{compressed}\n<NOVEL_TEXT_END>\n\n\
Extract **all** sequential events from the compressed novel text above in one response."
        );
        #[derive(Deserialize)]
        struct Resp {
            events: Vec<Event>,
        }
        let resp: Resp = complete_and_parse_llm_json(self.chat.as_ref(), &system, &user).await?;
        Ok(resp.events)
    }

    async fn extract_all_get_next(&self, novel_text: &str) -> VimaxResult<Vec<Event>> {
        let mut events = Vec::new();
        loop {
            if events.len() >= MAX_GET_NEXT {
                return Err(VimaxError::Llm(format!(
                    "event extraction exceeded {MAX_GET_NEXT} without is_last"
                )));
            }
            let event = self.extract_next(novel_text, &events).await?;
            let is_last = event.is_last;
            events.push(event);
            if is_last {
                break;
            }
        }
        Ok(events)
    }

    pub async fn extract_next(
        &self,
        novel_text: &str,
        extracted: &[Event],
    ) -> VimaxResult<Event> {
        let extracted_str = extracted
            .iter()
            .map(|e| {
                format!(
                    "<Event {}>\nDescription: {}\nCharacters: {}\n",
                    e.index,
                    e.description,
                    e.characters.join(", ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let system = include_str!(
            "../../prompts/event_extractor__system_prompt_template_extract_events.txt"
        )
        .replace("{format_instructions}", EVENT);
        let user = include_str!(
            "../../prompts/event_extractor__human_prompt_template_extract_next_event.txt"
        )
        .replace("{novel_text}", novel_text)
        .replace("{extracted_events}", &extracted_str);

        let mut event: Event =
            complete_and_parse_llm_json(self.chat.as_ref(), &system, &user).await?;
        if event.index != extracted.len() as i32 {
            event.index = extracted.len() as i32;
        }
        Ok(event)
    }
}

const EVENTS_BATCH: &str = r#"Return a JSON object:
{"events":[{"index":0,"is_last":false,"description":"string","characters":["name"]}, ...]}
Rules:
- `events` must list every major sequential plot event in story order.
- `index` must be 0..n-1 contiguous.
- Exactly one event must have `is_last`: true (the final event).
- description and character names MUST match the novel's language."#;

fn split_text(text: &str, chunk_size: usize, chunk_overlap: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return vec![];
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let end = (start + chunk_size).min(chars.len());
        chunks.push(chars[start..end].iter().collect());
        if end >= chars.len() {
            break;
        }
        start = end.saturating_sub(chunk_overlap);
        if start >= end {
            start = end;
        }
    }
    chunks
}

fn merge_events(merged: &mut Vec<Event>, incoming: &mut [Event]) {
    for event in incoming.iter_mut() {
        let key = event.description.trim().to_lowercase();
        if key.is_empty() {
            continue;
        }
        if merged
            .iter()
            .any(|e| e.description.trim().to_lowercase() == key)
        {
            continue;
        }
        merged.push(event.clone());
    }
}

fn reindex_events(events: &mut [Event]) {
    let last = events.len();
    for (i, event) in events.iter_mut().enumerate() {
        event.index = i as i32;
        event.is_last = i + 1 == last;
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use async_trait::async_trait;

    use super::*;
    use crate::backends::VimaxChat;
    use crate::error::VimaxResult;

    struct RecordingChat {
        calls: Arc<AtomicUsize>,
        batch_json: String,
    }

    #[tokio::test]
    async fn batch_happy_path_single_completion() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_c = Arc::clone(&calls);
        let chat: Arc<dyn VimaxChat> = Arc::new(RecordingChat {
            calls: calls_c,
            batch_json: r#"{"events":[{"index":0,"is_last":true,"description":"A","characters":["hero"]}]}"#
                .into(),
        });
        let ex = EventExtractor::new(chat);
        let events = ex.extract_all("compressed story body").await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].description, "A");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn long_compressed_splits_without_full_text_in_user() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_c = Arc::clone(&calls);
        let long = "事件".repeat(7000);
        let chat: Arc<dyn VimaxChat> = Arc::new(RecordingChat {
            calls: calls_c,
            batch_json: r#"{"events":[{"index":0,"is_last":true,"description":"chunk","characters":[]}]}"#
                .into(),
        });
        let ex = EventExtractor::new(chat);
        let _ = ex.extract_all(&long).await.unwrap();
        assert!(calls.load(Ordering::SeqCst) >= 2);
    }

    #[async_trait]
    impl VimaxChat for RecordingChat {
        async fn complete_text(&self, system: &str, user: &str) -> VimaxResult<String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if system.contains("Literary Analyst") && user.contains("<NOVEL_TEXT_START>") {
                return Ok(self.batch_json.clone());
            }
            if user.contains("<EXTRACTED_EVENTS_START>") {
                return Ok(
                    r#"{"index":0,"is_last":true,"description":"fallback","characters":[]}"#
                        .into(),
                );
            }
            Ok(self.batch_json.clone())
        }

        async fn complete_vision(
            &self,
            _system: &str,
            _user_text: &str,
            _image_paths: &[&Path],
        ) -> VimaxResult<String> {
            Ok(String::new())
        }
    }
}

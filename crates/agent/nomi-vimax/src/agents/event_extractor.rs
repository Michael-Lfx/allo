use std::sync::Arc;

use crate::backends::VimaxChat;
use crate::domain::Event;
use crate::error::{VimaxError, VimaxResult};
use crate::json_util::parse_llm_json;

use super::formats::EVENT;

pub struct EventExtractor {
    chat: Arc<dyn VimaxChat>,
}

impl EventExtractor {
    pub fn new(chat: Arc<dyn VimaxChat>) -> Self {
        Self { chat }
    }

    pub async fn extract_all(&self, novel_text: &str) -> VimaxResult<Vec<Event>> {
        let mut events = Vec::new();
        const MAX: usize = 50;
        loop {
            if events.len() >= MAX {
                return Err(VimaxError::Llm(format!(
                    "event extraction exceeded {MAX} without is_last"
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

        let raw = self.chat.complete_text(&system, &user).await?;
        let mut event: Event = parse_llm_json(&raw)?;
        if event.index != extracted.len() as i32 {
            // Soft-correct common LLM index drift.
            event.index = extracted.len() as i32;
        }
        Ok(event)
    }
}

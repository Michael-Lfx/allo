use std::sync::Arc;

use crate::backends::VimaxChat;
use crate::domain::{Event, Scene};
use crate::error::VimaxResult;
use crate::json_util::parse_llm_json;

use super::formats::SCENE;

pub struct SceneExtractor {
    chat: Arc<dyn VimaxChat>,
}

impl SceneExtractor {
    pub fn new(chat: Arc<dyn VimaxChat>) -> Self {
        Self { chat }
    }

    pub async fn extract_all_for_event(
        &self,
        event: &Event,
        context_fragments: &[String],
    ) -> VimaxResult<Vec<Scene>> {
        let mut scenes = Vec::new();
        const MAX: usize = 5;
        loop {
            if scenes.len() >= MAX {
                break;
            }
            let scene = self
                .get_next_scene(event, context_fragments, &scenes)
                .await?;
            let is_last = scene.is_last;
            scenes.push(scene);
            if is_last {
                break;
            }
        }
        Ok(scenes)
    }

    pub async fn get_next_scene(
        &self,
        event: &Event,
        context_fragments: &[String],
        previous: &[Scene],
    ) -> VimaxResult<Scene> {
        let mut fragments = String::new();
        for (i, f) in context_fragments.iter().enumerate() {
            fragments.push_str(&format!("<FRAGMENT_{i}_START>\n{f}\n<FRAGMENT_{i}_END>\n"));
        }
        let mut prev = String::new();
        for (i, s) in previous.iter().enumerate() {
            prev.push_str(&format!(
                "<SCENE_{i}_START>\n{}\n<SCENE_{i}_END>\n",
                s.script
            ));
        }

        let system = include_str!(
            "../../prompts/scene_extractor__system_prompt_template_get_next_scene.txt"
        )
        .replace("{format_instructions}", SCENE);
        let user = include_str!(
            "../../prompts/scene_extractor__human_prompt_template_get_next_scene.txt"
        )
        .replace("{event_description}", &event.description)
        .replace("{context_fragments}", &fragments)
        .replace("{previous_scenes}", &prev);

        let raw = self.chat.complete_text(&system, &user).await?;
        let mut scene: Scene = parse_llm_json(&raw)?;
        scene.index = previous.len() as i32;
        Ok(scene)
    }
}

/// Re-export BM25 helper (preferred over legacy keyword overlap).
pub use crate::rag::rank_chunks_by_keyword_overlap;

use std::sync::Arc;

use serde::Deserialize;

use crate::backends::VimaxChat;
use crate::domain::CharacterInScene;
use crate::error::VimaxResult;
use crate::json_util::parse_llm_json;

use super::formats::CHARACTERS;

pub struct CharacterExtractor {
    chat: Arc<dyn VimaxChat>,
}

impl CharacterExtractor {
    pub fn new(chat: Arc<dyn VimaxChat>) -> Self {
        Self { chat }
    }

    pub async fn extract_characters(&self, script: &str) -> VimaxResult<Vec<CharacterInScene>> {
        let system = include_str!(
            "../../prompts/character_extractor__system_prompt_template_extract_characters.txt"
        )
        .replace("{format_instructions}", CHARACTERS);
        let user = include_str!(
            "../../prompts/character_extractor__human_prompt_template_extract_characters.txt"
        )
        .replace("{script}", script);

        let raw = self.chat.complete_text(&system, &user).await?;
        #[derive(Deserialize)]
        struct Resp {
            characters: Vec<CharacterInScene>,
        }
        let resp: Resp = parse_llm_json(&raw)?;
        Ok(resp.characters)
    }
}

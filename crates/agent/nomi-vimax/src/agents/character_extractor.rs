use std::sync::Arc;

use serde::Deserialize;

use crate::backends::VimaxChat;
use crate::domain::CharacterInScene;
use crate::error::VimaxResult;
use crate::json_util::complete_and_parse_llm_json;

use super::formats::CHARACTERS;

pub struct CharacterExtractor {
    chat: Arc<dyn VimaxChat>,
}

impl CharacterExtractor {
    pub fn new(chat: Arc<dyn VimaxChat>) -> Self {
        Self { chat }
    }

    pub async fn extract_characters(
        &self,
        script: &str,
        style: &str,
    ) -> VimaxResult<Vec<CharacterInScene>> {
        let style = crate::planning::resolve_visual_style(style);
        let system = include_str!(
            "../../prompts/character_extractor__system_prompt_template_extract_characters.txt"
        )
        .replace("{format_instructions}", CHARACTERS)
        .replace("{style}", &style);
        let user = include_str!(
            "../../prompts/character_extractor__human_prompt_template_extract_characters.txt"
        )
        .replace("{script}", script)
        .replace("{style}", &style);

        #[derive(Deserialize)]
        struct Resp {
            characters: Vec<CharacterInScene>,
        }
        let resp: Resp =
            complete_and_parse_llm_json(self.chat.as_ref(), &system, &user).await?;
        Ok(resp.characters)
    }
}

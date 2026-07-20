use std::sync::Arc;

use serde::Deserialize;

use crate::backends::VimaxChat;
use crate::error::VimaxResult;
use crate::json_util::parse_llm_json;

use super::formats::SCRIPT_SCENES;

pub struct Screenwriter {
    chat: Arc<dyn VimaxChat>,
}

impl Screenwriter {
    pub fn new(chat: Arc<dyn VimaxChat>) -> Self {
        Self { chat }
    }

    pub async fn develop_story(
        &self,
        idea: &str,
        user_requirement: &str,
    ) -> VimaxResult<String> {
        let system =
            include_str!("../../prompts/screenwriter__system_prompt_template_develop_story.txt");
        let user = include_str!(
            "../../prompts/screenwriter__human_prompt_template_develop_story.txt"
        )
        .replace("{idea}", idea)
        .replace("{user_requirement}", user_requirement);
        self.chat.complete_text(system, &user).await
    }

    pub async fn write_script_based_on_story(
        &self,
        story: &str,
        user_requirement: &str,
    ) -> VimaxResult<Vec<String>> {
        let system = include_str!(
            "../../prompts/screenwriter__system_prompt_template_write_script_based_on_story.txt"
        )
        .replace("{format_instructions}", SCRIPT_SCENES);
        // Prompt file may not have format_instructions — inject into system if absent.
        let system = if system.contains("{format_instructions}") {
            system
        } else {
            format!("{system}\n\n[Output]\n{SCRIPT_SCENES}")
        };
        let user = include_str!(
            "../../prompts/screenwriter__human_prompt_template_write_script_based_on_story.txt"
        )
        .replace("{story}", story)
        .replace("{user_requirement}", user_requirement);

        let raw = self.chat.complete_text(&system, &user).await?;
        #[derive(Deserialize)]
        struct Resp {
            scenes: Vec<String>,
        }
        // Also accept a bare JSON array of strings.
        if let Ok(scenes) = parse_llm_json::<Vec<String>>(&raw) {
            return Ok(scenes);
        }
        let resp: Resp = parse_llm_json(&raw)?;
        Ok(resp.scenes)
    }
}

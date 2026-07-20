use std::collections::HashMap;
use std::sync::Arc;

use serde::Deserialize;

use crate::backends::VimaxChat;
use crate::domain::{CharacterInEvent, CharacterInNovel, Scene};
use crate::error::VimaxResult;
use crate::json_util::parse_llm_json;

use super::formats::{CHARACTERS_IN_EVENT, CHARACTERS_IN_NOVEL};

pub struct GlobalInformationPlanner {
    chat: Arc<dyn VimaxChat>,
}

impl GlobalInformationPlanner {
    pub fn new(chat: Arc<dyn VimaxChat>) -> Self {
        Self { chat }
    }

    pub async fn merge_characters_across_scenes_in_event(
        &self,
        scenes: &[(Scene, Vec<String>)],
    ) -> VimaxResult<Vec<CharacterInEvent>> {
        let mut blob = String::new();
        for (i, (scene, chars)) in scenes.iter().enumerate() {
            blob.push_str(&format!("<SCENE_{i}_START>\n"));
            blob.push_str("<SCRIPT_START>\n");
            blob.push_str(&scene.script);
            blob.push_str("\n<SCRIPT_END>\n");
            blob.push_str("<CHARACTERS_START>\n");
            for (j, name) in chars.iter().enumerate() {
                blob.push_str(&format!(
                    "<CHARACTER_{j}_START>\n{name}\n<CHARACTER_{j}_END>\n"
                ));
            }
            blob.push_str("<CHARACTERS_END>\n");
            blob.push_str(&format!("<SCENE_{i}_END>\n\n"));
        }

        let system = include_str!(
            "../../prompts/global_information_planner__system_prompt_template_merge_characters_across_scenes_in_event.txt"
        )
        .replace("{format_instructions}", CHARACTERS_IN_EVENT);
        let user = include_str!(
            "../../prompts/global_information_planner__human_prompt_template_merge_characters_across_scenes_in_event.txt"
        )
        .replace("{scenes_sequence}", &blob);

        let raw = self.chat.complete_text(&system, &user).await?;
        #[derive(Deserialize)]
        struct Resp {
            characters: Vec<CharacterInEventLoose>,
        }
        #[derive(Deserialize)]
        struct CharacterInEventLoose {
            index: i32,
            identifier_in_event: String,
            #[serde(default)]
            active_scenes: HashMap<String, String>,
            #[serde(default)]
            static_features: String,
        }
        let resp: Resp = parse_llm_json(&raw)?;
        Ok(resp
            .characters
            .into_iter()
            .map(|c| CharacterInEvent {
                index: c.index,
                identifier_in_event: c.identifier_in_event,
                active_scenes: c
                    .active_scenes
                    .into_iter()
                    .filter_map(|(k, v)| k.parse::<i32>().ok().map(|i| (i, v)))
                    .collect(),
                static_features: c.static_features,
            })
            .collect())
    }

    pub async fn merge_characters_to_existing_in_novel(
        &self,
        existing: &[CharacterInNovel],
        event_chars: &[CharacterInEvent],
        _event_index: i32,
    ) -> VimaxResult<Vec<CharacterInNovel>> {
        let mut existing_blob = String::from("<EXISTING_CHARACTERS_START>\n");
        for (i, c) in existing.iter().enumerate() {
            existing_blob.push_str(&format!(
                "<CHARACTER_{i}_START>\nindex: {}\nid: {}\nfeatures: {}\n<CHARACTER_{i}_END>\n",
                c.index, c.identifier_in_novel, c.static_features
            ));
        }
        existing_blob.push_str("<EXISTING_CHARACTERS_END>\n");

        let mut event_blob = String::from("<EVENT_CHARACTERS_START>\n");
        for (i, c) in event_chars.iter().enumerate() {
            event_blob.push_str(&format!(
                "<CHARACTER_{i}_START>\nindex: {}\nid: {}\nfeatures: {}\nactive_scenes: {:?}\n<CHARACTER_{i}_END>\n",
                c.index, c.identifier_in_event, c.static_features, c.active_scenes
            ));
        }
        event_blob.push_str("<EVENT_CHARACTERS_END>\n");

        let system = include_str!(
            "../../prompts/global_information_planner__system_prompt_template_merge_characters_to_existing_characters_in_novel.txt"
        )
        .replace("{format_instructions}", CHARACTERS_IN_NOVEL);
        let user = include_str!(
            "../../prompts/global_information_planner__human_prompt_template_merge_characters_to_existing_characters_in_novel.txt"
        )
        .replace("{existing_characters_in_novel}", &existing_blob)
        .replace("{characters_in_event}", &event_blob);

        let raw = self.chat.complete_text(&system, &user).await?;
        #[derive(Deserialize)]
        struct Resp {
            characters: Vec<CharacterInNovelLoose>,
        }
        #[derive(Deserialize)]
        struct CharacterInNovelLoose {
            index: i32,
            identifier_in_novel: String,
            #[serde(default)]
            active_events: HashMap<String, String>,
            #[serde(default)]
            static_features: String,
        }
        let resp: Resp = parse_llm_json(&raw)?;
        Ok(resp
            .characters
            .into_iter()
            .map(|c| CharacterInNovel {
                index: c.index,
                identifier_in_novel: c.identifier_in_novel,
                active_events: c
                    .active_events
                    .into_iter()
                    .filter_map(|(k, v)| k.parse::<i32>().ok().map(|i| (i, v)))
                    .collect(),
                static_features: c.static_features,
            })
            .collect())
    }
}

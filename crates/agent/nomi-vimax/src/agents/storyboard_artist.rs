use std::sync::Arc;

use serde::Deserialize;

use crate::backends::VimaxChat;
use crate::domain::{CharacterInScene, ShotBriefDescription, ShotDescription};
use crate::error::VimaxResult;
use crate::json_util::parse_llm_json;

use super::formats::{STORYBOARD, VIS_DECOMPOSE};

pub struct StoryboardArtist {
    chat: Arc<dyn VimaxChat>,
}

impl StoryboardArtist {
    pub fn new(chat: Arc<dyn VimaxChat>) -> Self {
        Self { chat }
    }

    pub async fn design_storyboard(
        &self,
        script: &str,
        characters: &[CharacterInScene],
        user_requirement: &str,
    ) -> VimaxResult<Vec<ShotBriefDescription>> {
        let characters_str = characters
            .iter()
            .enumerate()
            .map(|(i, c)| format!("Character {i}: {c}"))
            .collect::<Vec<_>>()
            .join("\n");

        let system = include_str!(
            "../../prompts/storyboard_artist__system_prompt_template_design_storyboard.txt"
        )
        .replace("{format_instructions}", STORYBOARD);
        let user = include_str!(
            "../../prompts/storyboard_artist__human_prompt_template_design_storyboard.txt"
        )
        .replace("{script_str}", script)
        .replace("{characters_str}", &characters_str)
        .replace("{user_requirement_str}", user_requirement);

        let raw = self.chat.complete_text(&system, &user).await?;
        #[derive(Deserialize)]
        struct Resp {
            storyboard: Vec<ShotBriefDescription>,
        }
        let resp: Resp = parse_llm_json(&raw)?;
        Ok(resp.storyboard)
    }

    pub async fn decompose_visual_description(
        &self,
        brief: &ShotBriefDescription,
        characters: &[CharacterInScene],
    ) -> VimaxResult<ShotDescription> {
        let characters_str = characters
            .iter()
            .enumerate()
            .map(|(i, c)| format!("Character {i}: {c}"))
            .collect::<Vec<_>>()
            .join("\n");

        let system = include_str!(
            "../../prompts/storyboard_artist__system_prompt_template_decompose_visual_description.txt"
        )
        .replace("{format_instructions}", VIS_DECOMPOSE);
        let user = include_str!(
            "../../prompts/storyboard_artist__human_prompt_template_decompose_visual_description.txt"
        )
        .replace("{visual_desc}", &brief.visual_desc)
        .replace("{characters_str}", &characters_str);

        let raw = self.chat.complete_text(&system, &user).await?;
        #[derive(Deserialize)]
        struct Decomp {
            ff_desc: String,
            #[serde(default)]
            ff_vis_char_idxs: Vec<i32>,
            lf_desc: String,
            #[serde(default)]
            lf_vis_char_idxs: Vec<i32>,
            motion_desc: String,
            variation_type: String,
            variation_reason: String,
        }
        let d: Decomp = parse_llm_json(&raw)?;
        Ok(ShotDescription {
            idx: brief.idx,
            is_last: brief.is_last,
            cam_idx: brief.cam_idx,
            visual_desc: brief.visual_desc.clone(),
            variation_type: d.variation_type,
            variation_reason: d.variation_reason,
            ff_desc: d.ff_desc,
            ff_vis_char_idxs: d.ff_vis_char_idxs,
            lf_desc: d.lf_desc,
            lf_vis_char_idxs: d.lf_vis_char_idxs,
            motion_desc: d.motion_desc,
            audio_desc: brief.audio_desc.clone(),
        })
    }
}

use std::sync::Arc;

use serde::Deserialize;

use crate::backends::VimaxChat;
use crate::clip_bounds::ClipBounds;
use crate::domain::{CharacterInScene, ShotBriefDescription, ShotDescription};
use crate::error::VimaxResult;
use crate::json_util::complete_and_parse_llm_json;

use super::formats;

pub struct StoryboardArtist {
    chat: Arc<dyn VimaxChat>,
    /// Clip window of the session's video model — every duration rule in the
    /// prompts is sized from it instead of naming one vendor's numbers.
    clip: ClipBounds,
}

impl StoryboardArtist {
    pub fn new(chat: Arc<dyn VimaxChat>, clip: ClipBounds) -> Self {
        Self { chat, clip }
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
        .replace("{format_instructions}", &formats::storyboard(self.clip))
        .replace(
            "{clip_duration_rules}",
            &crate::planning::clip_length_rules(self.clip),
        )
        .replace(
            "{speech_budget}",
            &crate::planning::speech_budget_line(self.clip),
        );
        let user = include_str!(
            "../../prompts/storyboard_artist__human_prompt_template_design_storyboard.txt"
        )
        .replace("{script_str}", script)
        .replace("{characters_str}", &characters_str)
        .replace("{user_requirement_str}", user_requirement);

        #[derive(Deserialize)]
        struct Resp {
            storyboard: Vec<ShotBriefDescription>,
        }
        let resp: Resp =
            complete_and_parse_llm_json(self.chat.as_ref(), &system, &user).await?;
        Ok(resp.storyboard)
    }

    pub async fn decompose_visual_description(
        &self,
        brief: &ShotBriefDescription,
        characters: &[CharacterInScene],
    ) -> VimaxResult<ShotDescription> {
        self.decompose_visual_description_with_continuity(brief, characters, None)
            .await
    }

    /// Decompose a shot; when `previous_lf_desc` is set, force ff_desc to continue from it.
    pub async fn decompose_visual_description_with_continuity(
        &self,
        brief: &ShotBriefDescription,
        characters: &[CharacterInScene],
        previous_lf_desc: Option<&str>,
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
        .replace("{format_instructions}", &formats::vis_decompose(self.clip))
        .replace(
            "{clip_duration_rules}",
            &crate::planning::clip_length_rules(self.clip),
        );
        let continuity_block = match previous_lf_desc.map(str::trim).filter(|s| !s.is_empty()) {
            Some(prev) => format!(
                "\n<PREVIOUS_SHOT_LAST_FRAME>\n{prev}\n</PREVIOUS_SHOT_LAST_FRAME>\n\
CRITICAL CONTINUITY: This shot is timeline-adjacent to the previous shot in the SAME scene. \
The first_frame description MUST start from (or seamlessly continue) the previous shot's last frame above — \
same cast identity, wardrobe, lighting mood, and set. You may reframe (new cam_idx / shot size) but do NOT \
reset to an unrelated establishing pose. Cross-scene continuity does NOT apply here.\n"
            ),
            None => String::new(),
        };
        let user = include_str!(
            "../../prompts/storyboard_artist__human_prompt_template_decompose_visual_description.txt"
        )
        .replace("{visual_desc}", &brief.visual_desc)
        .replace("{characters_str}", &characters_str)
        .replace("{continuity_block}", &continuity_block);

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
        let d: Decomp =
            complete_and_parse_llm_json(self.chat.as_ref(), &system, &user).await?;
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
            beats: Vec::new(),
        })
    }
}

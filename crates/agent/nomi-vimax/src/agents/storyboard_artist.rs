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
        self.decompose_visual_description_with_continuity(brief, characters, None, None)
            .await
    }

    /// Decompose a shot; when `previous_lf_desc` is set, identity carries over.
    /// `previous_cam_idx` decides whether first_frame resumes the pose (same
    /// camera) or opens a new angle already in progress (a cut).
    pub async fn decompose_visual_description_with_continuity(
        &self,
        brief: &ShotBriefDescription,
        characters: &[CharacterInScene],
        previous_lf_desc: Option<&str>,
        previous_cam_idx: Option<i32>,
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
            Some(prev) => {
                let same_take = previous_cam_idx.is_some_and(|cam| cam == brief.cam_idx);
                let pose_rule = if same_take {
                    "The first_frame description MUST open exactly where that last frame ended \
(same composition, same body/prop positions, same screen-left/screen-right for each named person) \
and the motion continues from there."
                } else {
                    "This shot is a CUT to a NEW camera. first_frame is a new angle/size with the action \
already in progress — do NOT restage or replay the previous last-frame pose. Identity (cast, \
wardrobe, lighting, set) still carries over. Keep each named person on the SAME screen side \
(画面左侧/右侧) as the previous last frame unless THIS shot's visual_desc explicitly says 反打, \
过肩, or reverse."
                };
                format!(
                    "\n<PREVIOUS_SHOT_LAST_FRAME>\n{prev}\n</PREVIOUS_SHOT_LAST_FRAME>\n\
CRITICAL CONTINUITY: This shot is timeline-adjacent to the previous shot in the SAME scene. \
{pose_rule} Cross-scene continuity does NOT apply here.\n"
                )
            }
            None => String::new(),
        };
        let beats_block = packed_beats_block(brief);
        let user = include_str!(
            "../../prompts/storyboard_artist__human_prompt_template_decompose_visual_description.txt"
        )
        .replace("{visual_desc}", &brief.visual_desc)
        .replace("{characters_str}", &characters_str)
        .replace("{continuity_block}", &continuity_block)
        .replace("{beats_block}", &beats_block);

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
            beats: brief
                .beats
                .iter()
                .map(|beat| crate::domain::ShotBeat {
                    motion_desc: beat.visual_desc.clone(),
                    audio_desc: beat.audio_desc.clone(),
                    cam_idx: Some(beat.cam_idx),
                })
                .collect(),
        })
    }
}

fn packed_beats_block(brief: &ShotBriefDescription) -> String {
    if brief.beats.len() < 2 {
        return String::new();
    }
    let lines: String = brief
        .beats
        .iter()
        .enumerate()
        .map(|(i, beat)| {
            let audio = beat.audio_desc.as_deref().unwrap_or("").trim();
            let audio = if audio.is_empty() {
                String::new()
            } else {
                format!(" audio: {audio}")
            };
            format!(
                "- beat {i}: cam_idx={} visual: {}{audio}\n",
                beat.cam_idx, beat.visual_desc
            )
        })
        .collect();
    let cuts = brief
        .beats
        .windows(2)
        .any(|pair| pair[0].cam_idx != pair[1].cam_idx);
    let cut_rule = if cuts {
        "This is a native multi-shot: at a cam_idx change, CUT TO the new angle with the action already in progress — do not morph or dissolve. first_frame is beat 0; last_frame is the final beat after the last cut."
    } else {
        "This is ONE continuous take: camera and framing never change. Play every beat in order."
    };
    format!(
        "\n<CLIP_BEATS>\nThis storyboard row is ONE generated video ({n} beats). {cut_rule}\n{lines}</CLIP_BEATS>\n",
        n = brief.beats.len(),
    )
}

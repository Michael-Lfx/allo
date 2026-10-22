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
    /// Unique named speakers one generated file keeps consistent in the written voice lock.
    max_voice_refs: usize,
}

impl StoryboardArtist {
    pub fn new(chat: Arc<dyn VimaxChat>, clip: ClipBounds) -> Self {
        Self {
            chat,
            clip,
            max_voice_refs: 3,
        }
    }

    pub fn with_voice_ref_slots(mut self, max_voice_refs: usize) -> Self {
        self.max_voice_refs = max_voice_refs;
        self
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
        .replace("{format_instructions}", &formats::storyboard(self.clip, self.max_voice_refs))
        .replace(
            "{clip_duration_rules}",
            &crate::planning::clip_length_rules(self.clip),
        )
        .replace(
            "{speech_budget}",
            &format!(
                "{}\n{}",
                crate::planning::speech_budget_line(self.clip),
                crate::planning::voice_ref_slot_rules(self.max_voice_refs)
            ),
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
        let mut rows = resp.storyboard;

        // Coverage first: missing SCRIPT events mean the board is wrong, not
        // a cue to append stub rows later. One rewrite of the full list.
        // Performance lint afterwards is text-only and must not grow shots.
        let coverage = crate::drama::lint_script_promise_coverage(script, &rows);
        if !coverage.is_empty() {
            tracing::info!(
                issues = coverage.len(),
                "storyboard missed script-locked lines; running one full-board coverage repair"
            );
            let repair_user = format!(
                "{user}\n\n[LINT_FEEDBACK]\nYour previous storyboard was:\n{}\n\n\
It is INCOMPLETE — these SCRIPT-locked lines never appear:\n- {}\n\n\
Rewrite a COMPLETE storyboard of THIS SCRIPT only. Cover every location, reversal, \
and closing line already in <SCRIPT>. You MAY add rows so those events are filmed. \
If more uniquely named people speak than voice-ref slots allow, add another row — \
keep EVERY script line (never drop or paraphrase dialogue). \
Do NOT invent episodes, characters, or punchlines that are not in SCRIPT. \
Do NOT pad filler holds. Pack related beats into the same row when speakers and speech still fit.",
                serde_json::to_string_pretty(&rows).unwrap_or_default(),
                coverage.join("\n- ")
            );
            let repaired: Resp =
                complete_and_parse_llm_json(self.chat.as_ref(), &system, &repair_user).await?;
            let mut repaired_rows = repaired.storyboard;
            let remaining = crate::drama::lint_script_promise_coverage(script, &repaired_rows);
            if !remaining.is_empty() {
                tracing::warn!(
                    remaining = remaining.len(),
                    "storyboard coverage repair still missing spoken lines; stitching them onto the last row"
                );
                crate::drama::ensure_script_promise_lines(script, &mut repaired_rows);
            }
            rows = repaired_rows;
        }

        let mut issues = crate::drama::lint_storyboard_performance(&rows);
        issues.extend(crate::drama::lint_storyboard_blocking(&rows));
        if issues.is_empty() {
            polish_storyboard_audio(&mut rows);
            return Ok(rows);
        }
        tracing::info!(
            issues = issues.len(),
            "storyboard failed performance lint; running one text-only repair round"
        );
        let repair_user = format!(
            "{user}\n\n[LINT_FEEDBACK]\nYour previous storyboard was:\n{}\n\n\
It has these performance or blocking defects — rewrite ONLY the flagged text so every emotion \
becomes visible behavior (face/hands/blocking/props), every filler hold becomes a \
plot-advancing action, and every flagged picture states shot size, screen side \
(画面左侧/右侧/中央), facing, posture, who is closer to camera, and one set anchor \
for every named person and every speaker in that beat. One face is one body: \
do not leave a speaker as an unnamed extra, and do not give one face two seats. \
Do not write 同上, 接上一帧, or 保持刚才的姿势:\n- {}\n\n\
HARD RULES: return the SAME number of rows with the SAME idx / cam_idx / is_last, and \
inside each row the SAME number of beats with the SAME beat cam_idx values. Do NOT add, \
split, merge, or re-camera anything — only the wording of visual/audio descriptions may change.",
            serde_json::to_string_pretty(&rows).unwrap_or_default(),
            issues.join("\n- ")
        );
        match complete_and_parse_llm_json::<Resp>(self.chat.as_ref(), &system, &repair_user).await
        {
            Ok(repaired) if crate::drama::storyboard_structure_matches(&rows, &repaired.storyboard) => {
                let remaining = crate::drama::lint_storyboard_performance(&repaired.storyboard);
                if !remaining.is_empty() {
                    tracing::warn!(
                        remaining = remaining.len(),
                        "storyboard performance lint still failing after repair; proceeding with repaired board"
                    );
                }
                let mut repaired_rows = repaired.storyboard;
                polish_storyboard_audio(&mut repaired_rows);
                Ok(repaired_rows)
            }
            Ok(_) => {
                tracing::warn!(
                    "storyboard repair changed row/beat/camera structure; discarding repair and keeping original board"
                );
                polish_storyboard_audio(&mut rows);
                Ok(rows)
            }
            Err(e) => {
                tracing::warn!(error = %e, "storyboard repair round failed; keeping original board");
                polish_storyboard_audio(&mut rows);
                Ok(rows)
            }
        }
    }

    pub async fn decompose_visual_description(
        &self,
        brief: &ShotBriefDescription,
        characters: &[CharacterInScene],
    ) -> VimaxResult<ShotDescription> {
        self.decompose_visual_description_with_continuity(brief, characters, None, None)
            .await
    }

    /// Decompose a shot. `previous_lf_desc` is the previous row's exit state in
    /// text. It is restated as blocking, never copied as a pose to resume.
    /// `previous_cam_idx` is unused: camera does not decide a frame resume.
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
        let _ = previous_cam_idx;
        let continuity_block = match previous_lf_desc.map(str::trim).filter(|s| !s.is_empty()) {
            Some(prev) => format!(
                "\n<PREVIOUS_SHOT_EXIT>\n{prev}\n</PREVIOUS_SHOT_EXIT>\n\
CRITICAL CONTINUITY: The block above is the previous shot's EXIT STATE in text, not a frame to copy. \
Restate identity (cast, wardrobe, lighting, set) in this shot's own first_frame. Write each person \
again: 画面左侧/右侧/中央, facing, posture, who is closer to camera, and one fixed set anchor. \
Keep screen side unless visual_desc explicitly says 反打, 过肩, or reverse. Do NOT resume the \
previous pose and do NOT copy that shot's spoken line. The action is already in progress.\n"
            ),
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
            location_id: brief.location_id.clone(),
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

fn polish_storyboard_audio(rows: &mut [ShotBriefDescription]) {
    for row in rows.iter_mut() {
        if let Some(audio) = row.audio_desc.as_mut() {
            *audio = crate::dialogue::rewrite_spoken_payloads(audio);
        }
        for beat in &mut row.beats {
            if let Some(audio) = beat.audio_desc.as_mut() {
                *audio = crate::dialogue::rewrite_spoken_payloads(audio);
            }
        }
    }
}

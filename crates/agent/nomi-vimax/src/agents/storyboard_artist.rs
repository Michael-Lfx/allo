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
Do NOT pad filler holds. One row is one video of one place and may hold several cameras \
as 镜头1 / 镜头2 / 镜头3, up to 4. The room is written once. A glance stays inside the \
camera it answers. A line that does not \
fit the model window is split at a dialogue boundary.",
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

        let needs: Vec<u32> = rows
            .iter()
            .map(|row| {
                crate::planning::estimate_shot_need_secs(
                    self.clip,
                    row.audio_desc.as_deref(),
                    &row.visual_desc,
                    "small",
                )
            })
            .collect();
        if crate::planning::shots_are_fragmented(self.clip, &needs) {
            tracing::info!(
                rows = rows.len(),
                max_secs = self.clip.max_secs(),
                "storyboard rows are short for the model window; asking for one semantic regroup"
            );
            let repair_user = format!(
                "{user}\n\n[LINT_FEEDBACK]\nYour previous storyboard was:\n{}\n\n\
It splits one scene into {} rows whose spoken payload is mostly far below the selected model's maximum of {}s. \
Rewrite the COMPLETE board. Put one exchange, decision, or reveal into ONE row whose natural play approaches {}s, and put its wide, close-up, and reverse inside that row as 镜头1 / 镜头2 / 镜头3 (at most 4, no timestamps). \
State the room once before 镜头1. Later cameras do not repeat it. A reverse says 反打 and restates only the people in that frame. \
A glance, a gasp, or a one-line answer stays in the camera it answers. The next row starts when the place changes, the row already has 4 cameras, or the speech will not fit, and that row restates blocking once. \
Do not pad with empty holds, slow looks, or repeated geography. Do not drop, paraphrase, or repeat any script line. \
If a speech cannot be said clearly inside {}s, split at a dialogue boundary. Keep every location the script already shows.",
                serde_json::to_string_pretty(&rows).unwrap_or_default(),
                rows.len(),
                self.clip.max_secs(),
                self.clip.max_secs(),
                self.clip.max_secs(),
            );
            match complete_and_parse_llm_json::<Resp>(self.chat.as_ref(), &system, &repair_user)
                .await
            {
                Ok(repaired)
                    if !repaired.storyboard.is_empty()
                        && repaired.storyboard.len() <= rows.len() =>
                {
                    let mut next = repaired.storyboard;
                    let missing = crate::drama::lint_script_promise_coverage(script, &next);
                    if !missing.is_empty() {
                        tracing::warn!(
                            remaining = missing.len(),
                            "storyboard regroup dropped spoken lines; stitching them back"
                        );
                        crate::drama::ensure_script_promise_lines(script, &mut next);
                    }
                    rows = next;
                }
                Ok(_) => {
                    tracing::warn!(
                        "storyboard regroup did not shorten the board; keeping the draft"
                    );
                }
                Err(e) => {
                    tracing::warn!(error = %e, "storyboard regroup failed; keeping the draft");
                }
            }
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
            ff_desc: d.ff_desc.clone(),
            ff_vis_char_idxs: d.ff_vis_char_idxs,
            lf_desc: crate::planning::with_distinct_exit(&d.ff_desc, &d.lf_desc, &d.motion_desc),
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
        "These beats are already one camera each. first_frame is beat 0 and last_frame is the final beat. Do not invent another camera or a timeline inside the clip."
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

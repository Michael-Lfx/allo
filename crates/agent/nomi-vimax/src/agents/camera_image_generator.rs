use std::path::Path;
use std::sync::Arc;

use serde::Deserialize;

use crate::backends::{VimaxChat, VimaxVideo};
use crate::domain::{Camera, ShotBriefDescription, ShotDescription};
use crate::error::{VimaxError, VimaxResult};
use crate::json_util::parse_llm_json;
use crate::media_local;

use super::formats::CAMERA_TREE;

const CAMERA_TREE_RETRIES: u32 = 2;

pub struct CameraImageGenerator {
    chat: Arc<dyn VimaxChat>,
    video: Arc<dyn VimaxVideo>,
}

impl CameraImageGenerator {
    pub fn new(chat: Arc<dyn VimaxChat>, video: Arc<dyn VimaxVideo>) -> Self {
        Self { chat, video }
    }

    pub async fn construct_camera_tree(
        &self,
        cameras: &[Camera],
        shot_descs: &[ShotDescription],
    ) -> VimaxResult<Vec<Camera>> {
        let shot_by_idx: std::collections::HashMap<i32, &ShotDescription> =
            shot_descs.iter().map(|s| (s.idx, s)).collect();

        let mut camera_seq = String::from("<CAMERA_SEQ>\n");
        for cam in cameras {
            camera_seq.push_str(&format!("<CAMERA_{}>\n", cam.idx));
            for shot_idx in &cam.active_shot_idxs {
                let desc = shot_by_idx.get(shot_idx).ok_or_else(|| {
                    VimaxError::msg(format!(
                        "Camera {} references missing shot {}",
                        cam.idx, shot_idx
                    ))
                })?;
                camera_seq.push_str(&format!("Shot {shot_idx}: {}\n", desc.visual_desc));
            }
            camera_seq.push_str(&format!("</CAMERA_{}>\n", cam.idx));
        }
        camera_seq.push_str("</CAMERA_SEQ>");

        let system = include_str!(
            "../../prompts/camera_image_generator__system_prompt_template_select_reference_camera.txt"
        )
        .replace("{format_instructions}", CAMERA_TREE);
        let user = include_str!(
            "../../prompts/camera_image_generator__human_prompt_template_select_reference_camera.txt"
        )
        .replace("{camera_count}", &cameras.len().to_string())
        .replace("{camera_seq_str}", &camera_seq);

        #[derive(Deserialize)]
        struct ParentItem {
            parent_cam_idx: Option<i32>,
            parent_shot_idx: Option<i32>,
            #[serde(default)]
            reason: Option<String>,
            #[serde(default)]
            is_parent_fully_covers_child: Option<bool>,
            #[serde(default)]
            missing_info: Option<String>,
        }
        #[derive(Deserialize)]
        struct Resp {
            camera_parent_items: Vec<Option<ParentItem>>,
        }

        let expected = cameras.len();
        let mut last_err = String::new();
        let mut parent_items = None;
        for attempt in 0..=CAMERA_TREE_RETRIES {
            if attempt > 0 {
                tracing::warn!(
                    expected,
                    attempt,
                    "retrying camera tree construction after length mismatch"
                );
            }
            let raw = self.chat.complete_text(&system, &user).await?;
            let resp: Resp = parse_llm_json(&raw)?;
            match normalize_parent_items(resp.camera_parent_items, expected) {
                Ok(items) => {
                    parent_items = Some(items);
                    break;
                }
                Err(e) => last_err = e,
            }
        }
        let parent_items = parent_items.ok_or_else(|| VimaxError::Llm(last_err))?;

        let valid_camera_idxs: std::collections::HashSet<i32> =
            cameras.iter().map(|c| c.idx).collect();
        let valid_shot_idxs: std::collections::HashSet<i32> = shot_by_idx.keys().copied().collect();

        // Soft-validate parents; invalid refs become root rather than failing the whole plan.
        let mut out = cameras.to_vec();
        for (cam, item) in out.iter_mut().zip(parent_items.into_iter()) {
            if let Some(p) = item {
                let mut parent_cam = p.parent_cam_idx;
                let mut parent_shot = p.parent_shot_idx;
                if parent_cam == Some(cam.idx) {
                    parent_cam = None;
                    parent_shot = None;
                }
                if parent_cam.is_some_and(|idx| !valid_camera_idxs.contains(&idx)) {
                    parent_cam = None;
                    parent_shot = None;
                }
                if parent_shot.is_some_and(|idx| !valid_shot_idxs.contains(&idx)) {
                    parent_shot = None;
                }
                cam.parent_cam_idx = parent_cam;
                cam.parent_shot_idx = parent_shot;
                cam.reason = p.reason;
                cam.is_parent_fully_covers_child = p.is_parent_fully_covers_child;
                cam.missing_info = p.missing_info;
            }
        }

        // Ensure at least the first camera is a root (matches prompt guideline).
        if let Some(first) = out.first_mut() {
            first.parent_cam_idx = None;
            first.parent_shot_idx = None;
        }

        Ok(out)
    }

    /// Also accept brief descriptions when building tree early.
    pub async fn construct_camera_tree_from_briefs(
        &self,
        cameras: &[Camera],
        briefs: &[ShotBriefDescription],
    ) -> VimaxResult<Vec<Camera>> {
        let shots: Vec<ShotDescription> = briefs
            .iter()
            .map(|b| ShotDescription {
                idx: b.idx,
                is_last: b.is_last,
                cam_idx: b.cam_idx,
                visual_desc: b.visual_desc.clone(),
                variation_type: "small".into(),
                variation_reason: String::new(),
                ff_desc: String::new(),
                ff_vis_char_idxs: vec![],
                lf_desc: String::new(),
                lf_vis_char_idxs: vec![],
                motion_desc: String::new(),
                audio_desc: b.audio_desc.clone(),
            })
            .collect();
        self.construct_camera_tree(cameras, &shots).await
    }

    pub async fn generate_transition_video(
        &self,
        first_shot_visual_desc: &str,
        second_shot_visual_desc: &str,
        first_shot_ff_path: &Path,
        out_path: &Path,
    ) -> VimaxResult<()> {
        let prompt = format!(
            "Two shots. The transition between the shots is a cut to. The style of the two shots should be consistent.\nThe first shot description: {first_shot_visual_desc}.\nThe second shot description: {second_shot_visual_desc}."
        );
        self.video
            .generate(
                &prompt,
                Some(first_shot_ff_path),
                None,
                &[],
                5,
                out_path,
            )
            .await
    }

    /// Extract new camera image from transition video (scene-cut → else last frame).
    pub async fn get_new_camera_image(
        &self,
        transition_video_path: &Path,
        out_path: &Path,
    ) -> VimaxResult<()> {
        media_local::extract_new_camera_frame(transition_video_path, out_path).await
    }
}

/// Align LLM parent-item list to camera count.
/// Small mismatches are repaired (pad roots / truncate) so a flaky model
/// does not abort an otherwise successful plan.
fn normalize_parent_items<T>(mut items: Vec<Option<T>>, expected: usize) -> Result<Vec<Option<T>>, String> {
    let got = items.len();
    if got == expected {
        return Ok(items);
    }
    // Tolerate off-by-one / small drift from thinking models.
    if got < expected && expected - got <= 2 {
        tracing::warn!(
            expected,
            got,
            "padding camera_parent_items with root entries"
        );
        items.resize_with(expected, || None);
        return Ok(items);
    }
    if got > expected && got - expected <= 2 {
        tracing::warn!(
            expected,
            got,
            "truncating camera_parent_items to camera count"
        );
        items.truncate(expected);
        return Ok(items);
    }
    Err(format!(
        "camera tree length mismatch: expected {expected}, got {got}"
    ))
}

#[cfg(test)]
mod tests {
    use super::normalize_parent_items;

    #[test]
    fn pads_short_list() {
        let items = vec![Some(1), Some(2)];
        let out = normalize_parent_items(items, 3).unwrap();
        assert_eq!(out.len(), 3);
        assert!(out[2].is_none());
    }

    #[test]
    fn truncates_long_list() {
        let items = vec![Some(1), Some(2), Some(3)];
        let out = normalize_parent_items(items, 2).unwrap();
        assert_eq!(out, vec![Some(1), Some(2)]);
    }

    #[test]
    fn rejects_large_mismatch() {
        let items = vec![Some(1)];
        assert!(normalize_parent_items(items, 5).is_err());
    }
}

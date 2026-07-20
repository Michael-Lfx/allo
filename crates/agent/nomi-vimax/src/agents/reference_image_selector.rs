use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;

use crate::backends::VimaxChat;
use crate::error::{VimaxError, VimaxResult};
use crate::json_util::parse_llm_json;

use super::formats::REF_IMAGES;

#[derive(Debug, Clone)]
pub struct SelectorOutput {
    pub reference_image_path_and_text_pairs: Vec<(PathBuf, String)>,
    pub text_prompt: String,
}

pub struct ReferenceImageSelector {
    chat: Arc<dyn VimaxChat>,
}

impl ReferenceImageSelector {
    pub fn new(chat: Arc<dyn VimaxChat>) -> Self {
        Self { chat }
    }

    pub async fn select_reference_images_and_generate_prompt(
        &self,
        available: &[(PathBuf, String)],
        frame_description: &str,
    ) -> VimaxResult<SelectorOutput> {
        let mut filtered = available.to_vec();

        // Text-only prefilter when many candidates.
        if filtered.len() >= 8 {
            let mut user = String::new();
            for (idx, (_, text)) in filtered.iter().enumerate() {
                user.push_str(&format!("Image {idx}: {text}\n"));
            }
            user.push_str(
                &include_str!(
                    "../../prompts/reference_image_selector__human_prompt_template_select_reference_images.txt"
                )
                .replace("{frame_description}", frame_description),
            );
            let system = include_str!(
                "../../prompts/reference_image_selector__system_prompt_template_select_reference_images_only_text.txt"
            )
            .replace("{format_instructions}", REF_IMAGES);
            let raw = self.chat.complete_text(&system, &user).await?;
            let indices = parse_indices(&raw)?;
            filtered = select_by_indices(&filtered, &indices)?;
        }

        // Multimodal refinement.
        let mut user_parts_text = String::new();
        let mut image_paths: Vec<&Path> = Vec::new();
        for (idx, (path, text)) in filtered.iter().enumerate() {
            user_parts_text.push_str(&format!("Image {idx}: {text}\n"));
            image_paths.push(path.as_path());
        }
        user_parts_text.push_str(
            &include_str!(
                "../../prompts/reference_image_selector__human_prompt_template_select_reference_images.txt"
            )
            .replace("{frame_description}", frame_description),
        );

        let system = include_str!(
            "../../prompts/reference_image_selector__system_prompt_template_select_reference_images_multimodal.txt"
        )
        .replace("{format_instructions}", REF_IMAGES);

        let raw = self
            .chat
            .complete_vision(&system, &user_parts_text, &image_paths)
            .await?;
        #[derive(Deserialize)]
        struct Resp {
            ref_image_indices: Vec<usize>,
            text_prompt: String,
        }
        let resp: Resp = parse_llm_json(&raw)?;
        let selected = select_by_indices(&filtered, &resp.ref_image_indices)?;
        Ok(SelectorOutput {
            reference_image_path_and_text_pairs: selected,
            text_prompt: resp.text_prompt,
        })
    }
}

fn parse_indices(raw: &str) -> VimaxResult<Vec<usize>> {
    #[derive(Deserialize)]
    struct Resp {
        ref_image_indices: Vec<usize>,
    }
    let resp: Resp = parse_llm_json(raw)?;
    Ok(resp.ref_image_indices)
}

fn select_by_indices(
    pairs: &[(PathBuf, String)],
    indices: &[usize],
) -> VimaxResult<Vec<(PathBuf, String)>> {
    let mut out = Vec::new();
    for &i in indices {
        if i >= pairs.len() {
            return Err(VimaxError::Llm(format!(
                "ref_image_indices out of range: {i} (have {} images)",
                pairs.len()
            )));
        }
        out.push(pairs[i].clone());
    }
    Ok(out)
}

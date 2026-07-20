//! ViMax agents (LLM + image) — faithful ports using extracted prompts.

mod camera_image_generator;
mod character_extractor;
mod character_portraits_generator;
mod event_extractor;
mod global_information_planner;
mod novel_compressor;
mod reference_image_selector;
mod scene_extractor;
mod screenwriter;
mod storyboard_artist;

pub use camera_image_generator::CameraImageGenerator;
pub use character_extractor::CharacterExtractor;
pub use character_portraits_generator::CharacterPortraitsGenerator;
pub use event_extractor::EventExtractor;
pub use global_information_planner::GlobalInformationPlanner;
pub use novel_compressor::NovelCompressor;
pub use reference_image_selector::{ReferenceImageSelector, SelectorOutput};
pub use scene_extractor::{SceneExtractor, rank_chunks_by_keyword_overlap};
pub use screenwriter::Screenwriter;
pub use storyboard_artist::StoryboardArtist;

/// Concise JSON schema strings substituted for `{format_instructions}`.
pub mod formats {
    pub const CHARACTERS: &str = r#"Return a JSON object:
{"characters":[{"idx":0,"identifier_in_scene":"string","is_visible":true,"static_features":"string","dynamic_features":"string|null"}]}
Fields: idx (int from 0), identifier_in_scene, is_visible, static_features (appearance/physique), dynamic_features (clothing/accessories, optional)."#;

    pub const STORYBOARD: &str = r#"Return a JSON object:
{"storyboard":[{"idx":0,"is_last":false,"cam_idx":0,"visual_desc":"string","audio_desc":"string|null"}]}
idx from 0; is_last true only on the final shot; cam_idx groups shots sharing a camera; visual_desc is a complete shot description; audio_desc optional dialogue/SFX."#;

    pub const VIS_DECOMPOSE: &str = r#"Return a JSON object:
{"ff_desc":"string","ff_vis_char_idxs":[0],"lf_desc":"string","lf_vis_char_idxs":[0],"motion_desc":"string","variation_type":"large|medium|small","variation_reason":"string"}
ff_*/lf_* are static first/last frame snapshots; motion_desc covers camera + element motion; variation_type is large|medium|small."#;

    pub const CAMERA_TREE: &str = r#"Return a JSON object:
{"camera_parent_items":[{"parent_cam_idx":null,"parent_shot_idx":null,"reason":"string","is_parent_fully_covers_child":null,"missing_info":null}]}
CRITICAL: camera_parent_items MUST have exactly the same length as the number of cameras in the input (one entry per camera, in the same order). Root cameras use null parent fields. parent_cam_idx/parent_shot_idx reference existing cameras/shots."#;

    pub const REF_IMAGES: &str = r#"Return a JSON object:
{"ref_image_indices":[0,2],"text_prompt":"string"}
ref_image_indices: 0-based indices into the provided image list (max 8). text_prompt describes the image to generate and which Image N to reference."#;

    pub const SCRIPT_SCENES: &str = r#"Return a JSON object:
{"scenes":["scene script string", "..."]}
Each string is one scene's screenplay (heading, action, dialogue)."#;

    pub const EVENT: &str = r#"Return a JSON object matching one Event:
{"index":0,"is_last":false,"description":"string","characters":["name"]}
index must equal the count of already-extracted events; set is_last true when the novel's events are exhausted."#;

    pub const SCENE: &str = r#"Return a JSON object matching one Scene:
{"index":0,"is_last":false,"script":"screenplay string","environment":"string|null","characters":["name"]}
index equals previous scene count; is_last true when no more scenes for this event."#;

    pub const CHARACTERS_IN_EVENT: &str = r#"Return a JSON object:
{"characters":[{"index":0,"identifier_in_event":"string","active_scenes":{"0":"name"},"static_features":"string"}]}
active_scenes maps scene index (string keys ok) to the name used in that scene."#;

    pub const CHARACTERS_IN_NOVEL: &str = r#"Return a JSON object:
{"characters":[{"index":0,"identifier_in_novel":"string","active_events":{"0":"name"},"static_features":"string"}]}
Merge event characters into the novel-level list without duplicates."#;
}

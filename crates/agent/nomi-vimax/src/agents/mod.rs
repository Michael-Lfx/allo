//! ViMax agents (LLM + image) — faithful ports using extracted prompts.

mod camera_image_generator;
mod character_extractor;
mod character_portraits_generator;
mod event_extractor;
mod film_cover;
mod global_information_planner;
mod novel_compressor;
mod reference_image_classifier;
mod reference_image_selector;
mod scene_extractor;
mod screenwriter;
mod storyboard_artist;
mod voice_profile_generator;
mod voice_reference_generator;
mod world_assets;

pub use camera_image_generator::CameraImageGenerator;
pub use character_extractor::CharacterExtractor;
pub use character_portraits_generator::{
    CharacterPortraitsGenerator, has_usable_cameo, has_usable_portrait, has_usable_portrait_sheet,
    three_view_image_prompt,
};
pub use event_extractor::EventExtractor;
pub use film_cover::{ensure_cover_from_final_video, ensure_film_cover, COVER_FILENAME};
pub use global_information_planner::GlobalInformationPlanner;
pub use novel_compressor::NovelCompressor;
pub use reference_image_classifier::{
    load_classification_report, heuristic_classification, ReferenceClassificationReport,
    ReferenceImageCategory, ReferenceImageClassification, ReferenceImageClassifier,
    CLASSIFICATION_CACHE_REL,
};
pub use reference_image_selector::{ReferenceImageSelector, SelectorOutput};
pub use scene_extractor::{SceneExtractor, rank_chunks_by_keyword_overlap};
pub use screenwriter::Screenwriter;
pub use storyboard_artist::StoryboardArtist;
pub use voice_profile_generator::VoiceProfileGenerator;
pub use voice_reference_generator::{
    has_usable_voice_ref, voice_ref_abs_path, VoiceReferenceGenerator,
};
pub use world_assets::{
    WorldAssetRegistry, WorldAssetsPlanner, rank_world_pairs_for_frame, world_asset_pairs,
};

/// Concise JSON schema strings substituted for `{format_instructions}`.
pub mod formats {
    pub const CHARACTERS: &str = r#"Return a JSON object:
{"characters":[{"idx":0,"identifier_in_scene":"string","is_visible":true,"static_features":"string","dynamic_features":"string|null","voice_profile":{"timbre":"string","volume":"normal","pitch":"mid","speaking_style":"string","caption_clause":"string|null"}}]}
Fields: idx (int from 0), identifier_in_scene, is_visible, static_features (appearance/physique), dynamic_features (clothing/accessories, optional), voice_profile (REQUIRED film-stable speaking voice bible reused across every shot — timbre must be a concrete acoustic fingerprint with age/gender/resonance/texture; volume quiet|normal|loud; pitch low|mid-low|mid|mid-high|high; speaking_style = pace+diction+emotional baseline only; caption_clause optional). Natural-language field values MUST match the user's input language (Chinese input → 简体中文 values)."#;

    pub const VOICE_PROFILES: &str = r#"Return a JSON object:
{"voices":[{"idx":0,"identifier_in_scene":"string","voice_profile":{"timbre":"string","volume":"normal","pitch":"mid","speaking_style":"string","caption_clause":null}}]}
One entry per input character (same idx / identifier_in_scene). voice_profile must be film-stable and distinctive: timbre = concrete acoustic fingerprint (not vague 女声/男声); volume quiet|normal|loud; pitch low|mid-low|mid|mid-high|high; speaking_style = pace+diction+baseline only (no per-shot crying/shouting baked into timbre). caption_clause may be null (pipeline rebuilds FIXED SPEAKER VOICE inject). Prose MUST match the user's input language (Chinese input → 简体中文)."#;

    use crate::clip_bounds::ClipBounds;

    /// Storyboard schema + field rules, with clip length sized to the selected
    /// video model (see [`crate::planning::speech_budget_line`]).
    pub fn storyboard(clip: ClipBounds) -> String {
        let speech = crate::planning::speech_budget_line(clip);
        format!(
            r#"Return a JSON object:
{{"storyboard":[{{"idx":0,"is_last":false,"cam_idx":0,"visual_desc":"string","audio_desc":"string"}}]}}
idx from 0; is_last true only on the final shot; cam_idx is the OPENING camera of this narrative clip (not a grouping key — do not emit a new object because the camera moves). visual_desc is the story unit this file plays (composition + events); reverse/insert/push-in of the SAME beat is CUT TO inside this object. Between rows keep screen-left/screen-right of each named person (do not flip 左侧/右侧 across a file splice). Slice objects by NARRATIVE, never by tripod. ONE ROW = ONE VIDEO. Identity is locked by reference images. audio_desc is REQUIRED for EVERY shot — put spoken dialogue and/or SFX+BGM intent there (never null/empty). When a character speaks, prefix with identifier_in_scene (e.g. 李薇：「……」 / Alice: "…") and put the line in quotes so duration estimation ignores SFX/BGM. Do NOT invent vocal timbre/age/gender in audio_desc (no 低沉嗓音/尖细女声/沙哑男声) — voice identity is locked separately; only dialogue text, emotion intensity, SFX, BGM. {speech} All shots in the scene MUST reuse the SAME continuous underscore motif/tempo/instrumentation in audio_desc — no new music style per cut. Purely visual beats still need ambient audio_desc (room tone + that same cinematic underscore). Prefer fewer, richer clips over micro-cuts; do not pad long takes or split one beat into extra shots. Natural-language values MUST match the user's input language (Chinese input → 简体中文)."#
        )
    }

    /// Shot-decompose schema + field rules.
    ///
    /// Decomposition happens before the renderer allocates clip lengths, so this
    /// deliberately forbids absolute seconds in `motion_desc` — see
    /// [`crate::planning::clip_length_rules`].
    pub fn vis_decompose(clip: ClipBounds) -> String {
        let rules = crate::planning::clip_length_rules(clip);
        format!(
            r#"Return a single JSON object. Each key MUST appear exactly once (never repeat ff_vis_char_idxs / lf_vis_char_idxs / any other field).
{{"ff_desc":"string","ff_vis_char_idxs":[0],"lf_desc":"string","lf_vis_char_idxs":[0],"motion_desc":"string","variation_type":"large|medium|small","variation_reason":"string"}}
ff_*/lf_* are COMPACT static snapshots (shot size, who is where, facing) — not full appearance bibles. lf_desc only states what CHANGED; if composition is unchanged, one short sentence. motion_desc is the primary video instruction: camera + the beat(s) this clip plays (a line + reaction may share one motion_desc); character names OK with at most one short visible tag. {rules} variation_type is large|medium|small. Prose fields MUST match the user's input language (Chinese input → 简体中文)."#
        )
    }

    pub const CAMERA_TREE: &str = r#"Return a JSON object:
{"camera_parent_items":[{"parent_cam_idx":null,"parent_shot_idx":null,"reason":"string","is_parent_fully_covers_child":null,"missing_info":null}]}
CRITICAL: camera_parent_items MUST have exactly the same length as the number of cameras in the input (one entry per camera, in the same order). Root cameras use null parent fields. parent_cam_idx/parent_shot_idx reference existing cameras/shots. reason/missing_info prose MUST match the user's input language."#;

    pub const REF_IMAGES: &str = r#"Return a JSON object:
{"ref_image_indices":[0,2],"text_prompt":"string"}
ref_image_indices: 0-based indices into the provided image list (max 8). text_prompt describes the image to generate and which Image N to reference. Prefer English in text_prompt when writing image-model prompts; otherwise match user language for explanations."#;

    pub const REF_IMAGE_CLASSIFY: &str = r#"Return a JSON object:
{"classifications":[{"photo_id":"string","category":"character|environment|prop|style","summary":"string","suggested_label":"string"}]}
One entry per input image. category MUST be exactly one of: character, environment, prop, style. photo_id MUST match the provided id. summary = one short visual description. suggested_label may be empty. Prose MUST match the user's label language when Chinese."#;

    pub const SCRIPT_SCENES: &str = r#"Return a JSON object:
{"scenes":["scene script string", "..."]}
Each string is one scene's screenplay (heading, action, dialogue). Screenplay text MUST match the user's input language (Chinese input → 简体中文).
CRITICAL JSON SAFETY: inside each scene string do NOT use raw ASCII double quotes ("). Use Chinese quotes 「」 for dialogue/SFX (e.g. 发出「咚」的一声). If you must use ", write it as \"."#;

    pub const EVENT: &str = r#"Return a JSON object matching one Event:
{"index":0,"is_last":false,"description":"string","characters":["name"]}
index must equal the count of already-extracted events; set is_last true when the novel's events are exhausted. description and character names MUST match the novel's language."#;

    pub const SCENE: &str = r#"Return a JSON object matching one Scene:
{"index":0,"is_last":false,"script":"screenplay string","environment":"string|null","characters":["name"]}
index equals previous scene count; is_last true when no more scenes for this event. script/environment MUST match the user's input language."#;

    pub const CHARACTERS_IN_EVENT: &str = r#"Return a JSON object:
{"characters":[{"index":0,"identifier_in_event":"string","active_scenes":{"0":"name"},"static_features":"string"}]}
active_scenes maps scene index (string keys ok) to the name used in that scene. Feature prose MUST match the user's input language."#;

    pub const CHARACTERS_IN_NOVEL: &str = r#"Return a JSON object:
{"characters":[{"index":0,"identifier_in_novel":"string","active_events":{"0":"name"},"static_features":"string"}]}
Merge event characters into the novel-level list without duplicates. Feature prose MUST match the user's input language."#;

    pub const WORLD_ASSETS: &str = r#"Return a JSON object:
{"environments":[{"idx":0,"slugline":"INT. PLACE - TIME","description":"empty set details"}],"props":[{"idx":0,"name":"string","description":"appearance details"}]}
environments: distinct locations, no people. props: key recurring objects only. Keep lists short. description/name prose MUST match the user's input language (Chinese input → 简体中文; slugline may stay INT./EXT. style)."#;
}

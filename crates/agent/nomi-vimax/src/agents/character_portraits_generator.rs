use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::backends::VimaxImage;
use crate::domain::CharacterInScene;
use crate::error::VimaxResult;

/// Instruction when a vacant production look plate is passed as img2img.
/// Match medium only — copying the look-plate location would destroy the studio turnaround.
const LOOK_PLATE_REF_INSTRUCTION: &str = "\n\
LOOK BIBLE (reference image): match ONLY the rendering medium, color science, lighting quality, \
and material treatment. Do NOT copy its location, architecture, or composition. \
Output remains a clean studio three-view turnaround on a light seamless backdrop.";

pub struct CharacterPortraitsGenerator {
    image: Arc<dyn VimaxImage>,
}

impl CharacterPortraitsGenerator {
    pub fn new(image: Arc<dyn VimaxImage>) -> Self {
        Self { image }
    }

    /// Full feature text — Features must survive; they drive theme fidelity.
    fn features_line(character: &CharacterInScene) -> String {
        let raw = format!(
            "(static) {}; (dynamic) {}",
            character.static_features.trim(),
            character.dynamic_features.as_deref().unwrap_or("").trim()
        );
        raw.chars().take(520).collect()
    }

    fn build_three_view_prompt(character: &CharacterInScene, style: &str) -> String {
        three_view_image_prompt(
            &character.identifier_in_scene,
            &Self::features_line(character),
            style,
        )
    }

    async fn cleanup_legacy_files(character_dir: &Path) {
        for name in ["front.png", "side.png", "back.png"] {
            let p = character_dir.join(name);
            if p.exists() {
                let _ = tokio::fs::remove_file(&p).await;
            }
        }
    }

    /// One character → one `{id}_three_view.png` (meaningful name for multi-ref prompts).
    ///
    /// `style_refs` is the vacant production look plate (and never a cast portrait).
    pub async fn generate_all_views(
        &self,
        character: &CharacterInScene,
        style: &str,
        _theme: &str,
        character_dir: &Path,
        style_refs: &[&Path],
    ) -> VimaxResult<HashMap<String, HashMap<String, HashMap<String, String>>>> {
        let style_refs: Vec<&Path> = style_refs
            .iter()
            .copied()
            .filter(|p| crate::media_local::is_usable_image_file(p))
            .collect();
        let style_refs = style_refs.as_slice();

        tokio::fs::create_dir_all(character_dir).await?;
        let id_safe = safe_file_stem(&character.identifier_in_scene);
        let sheet_name = format!("{id_safe}_three_view.png");
        let sheet = character_dir.join(&sheet_name);

        // Drop leftover discrete views from older pipelines.
        Self::cleanup_legacy_files(character_dir).await;
        // Migrate legacy generic `three_view.png` → meaningful name.
        let legacy = character_dir.join("three_view.png");
        if !sheet.exists() && legacy.exists() {
            let _ = tokio::fs::rename(&legacy, &sheet).await;
        }

        if !sheet.exists() {
            let prompt = Self::prompt_with_look_refs(character, style, style_refs);
            let _ = crate::session::write_text_artifact(
                &character_dir.join(format!("{id_safe}_three_view_generation_prompt.txt")),
                &prompt,
            )
            .await;
            self.image.generate(&prompt, style_refs, &sheet).await?;
        } else if !crate::media_local::is_usable_image_file(&sheet) {
            // e.g. JPEG bytes saved as .png without decode support — regenerate.
            let _ = tokio::fs::remove_file(&sheet).await;
            let prompt = Self::prompt_with_look_refs(character, style, style_refs);
            let _ = crate::session::write_text_artifact(
                &character_dir.join(format!("{id_safe}_three_view_generation_prompt.txt")),
                &prompt,
            )
            .await;
            self.image.generate(&prompt, style_refs, &sheet).await?;
        } else {
            // Backfill editable prompt for sheets generated before sidecar support.
            let sidecar =
                character_dir.join(format!("{id_safe}_three_view_generation_prompt.txt"));
            if !sidecar.is_file() {
                let prompt = Self::build_three_view_prompt(character, style);
                let _ = crate::session::write_text_artifact(&sidecar, &prompt).await;
            }
        }

        let id = &character.identifier_in_scene;
        let feat_hint: String = Self::features_line(character).chars().take(100).collect();
        let mut views = HashMap::new();
        if sheet.exists() {
            views.insert(
                "sheet".into(),
                view_item(
                    &sheet,
                    &format!(
                        "File [{sheet_name}] = GLOBAL three-view character bible for <{id}> (left=front, center=side, right=back). Features: {feat_hint}. Lock identity to this sheet."
                    ),
                ),
            );
        } else {
            return Err(crate::error::VimaxError::Image(format!(
                "three-view sheet missing after generation: {}",
                sheet.display()
            )));
        }

        let mut registry = HashMap::new();
        registry.insert(character.identifier_in_scene.clone(), views);
        Ok(registry)
    }

    fn prompt_with_look_refs(
        character: &CharacterInScene,
        style: &str,
        style_refs: &[&Path],
    ) -> String {
        let mut prompt = Self::build_three_view_prompt(character, style);
        if !style_refs.is_empty() {
            prompt.push_str(LOOK_PLATE_REF_INSTRUCTION);
        }
        prompt
    }
}

/// Shared three-view prompt so revise / sidecar rebuild matches first generation.
pub fn three_view_image_prompt(identifier: &str, features: &str, style: &str) -> String {
    let features: String = features.chars().take(520).collect();
    include_str!("../../prompts/character_portraits_generator__prompt_template_three_view.txt")
        .replace("{identifier}", identifier)
        .replace("{features}", &features)
        .replace("{style}", &crate::planning::resolve_visual_style(style))
}

fn safe_file_stem(s: &str) -> String {
    let raw: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = raw.trim_matches('_');
    if trimmed.is_empty() {
        "asset".into()
    } else {
        trimmed.chars().take(48).collect()
    }
}

fn view_item(path: &Path, description: &str) -> HashMap<String, String> {
    let mut item = HashMap::new();
    item.insert("path".into(), path.to_string_lossy().to_string());
    item.insert("description".into(), description.to_string());
    item
}

/// True when registry already points at an on-disk three-view sheet for this character.
pub fn has_usable_portrait_sheet(
    registry: &HashMap<String, HashMap<String, HashMap<String, String>>>,
    identifier: &str,
) -> bool {
    registry
        .get(identifier)
        .and_then(|views| views.get("sheet"))
        .and_then(|item| item.get("path"))
        .map(|p| crate::media_local::is_usable_image_file(Path::new(p)))
        .unwrap_or(false)
}

/// True when registry points at a usable user-uploaded Cameo for this character.
pub fn has_usable_cameo(
    registry: &HashMap<String, HashMap<String, HashMap<String, String>>>,
    identifier: &str,
) -> bool {
    registry
        .get(identifier)
        .and_then(|views| views.get("cameo"))
        .and_then(|item| item.get("path"))
        .map(|p| crate::media_local::is_usable_image_file(Path::new(p)))
        .unwrap_or(false)
}

/// True when the character already has a usable identity reference (user Cameo or AI sheet).
pub fn has_usable_portrait(
    registry: &HashMap<String, HashMap<String, HashMap<String, String>>>,
    identifier: &str,
) -> bool {
    has_usable_cameo(registry, identifier) || has_usable_portrait_sheet(registry, identifier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_line_keeps_generous_budget() {
        let ch = CharacterInScene {
            idx: 0,
            identifier_in_scene: "Alice".into(),
            is_visible: true,
            static_features: "a".repeat(400),
            dynamic_features: Some("b".repeat(400)),
            voice_profile: None,
        };
        let n = CharacterPortraitsGenerator::features_line(&ch).chars().count();
        assert!(n <= 520);
        assert!(n >= 500);
    }

    #[test]
    fn three_view_prompt_matches_vimax_shape() {
        let prompt = include_str!(
            "../../prompts/character_portraits_generator__prompt_template_three_view.txt"
        )
        .replace("{identifier}", "李薇")
        .replace(
            "{features}",
            "(static) red hanfu, black long hair; (dynamic) jade pendant",
        )
        .replace("{style}", "cinematic film look");
        let feat_pos = prompt.find("Features").expect("Features");
        let style_pos = prompt.find("Style:").expect("Style");
        assert!(feat_pos < style_pos);
        let lower = prompt.to_ascii_lowercase();
        assert!(lower.contains("pure white"));
        assert!(lower.contains("16:9"));
        assert!(lower.contains("front") && lower.contains("profile") && lower.contains("back"));
        assert!(prompt.contains("red hanfu"));
        assert!(prompt.contains("人物安全约束"));
        assert!(prompt.contains("不得套用现实真人、明星长相"));
        assert!(prompt.contains("蓝色拓扑网格"));
        assert!(!prompt.contains("{look_lock}"));
        assert!(!prompt.contains("{medium_lock}"));
        assert!(!prompt.contains("{face_guidance}"));
        assert!(!prompt.contains("{age_lock}"));
    }

    #[test]
    fn three_view_prompt_does_not_stack_medium_bans() {
        let prompt = three_view_image_prompt(
            "李薇",
            "(static) red hanfu, black long hair; (dynamic) jade pendant",
            "cinematic film look",
        );
        assert!(!prompt.contains("PRODUCTION LOOK LOCK"));
        assert!(!prompt.contains("MEDIUM LOCK"));
        assert!(!prompt.contains("CAST STYLE LOCK"));
        assert!(!prompt.contains("Absolutely NOT anime"));
        assert_eq!(prompt.matches("not anime").count(), 0);
        assert_eq!(prompt.matches("禁止动漫").count(), 0);
        assert!(prompt.contains("人物安全约束"));
        assert!(prompt.contains("不得套用现实真人、明星长相"));
        assert!(prompt.contains("蓝色拓扑网格"));
    }

    #[test]
    fn child_three_view_prompt_stays_vimax_simple() {
        let ch = CharacterInScene {
            idx: 0,
            identifier_in_scene: "小明".into(),
            is_visible: true,
            static_features: "8岁男孩，黑短发，白T恤".into(),
            dynamic_features: None,
            voice_profile: None,
        };
        let prompt = CharacterPortraitsGenerator::build_three_view_prompt(&ch, "cinematic film look");
        assert!(prompt.contains("8岁男孩"));
        assert!(!prompt.contains("not anime/cartoon/chibi"));
        assert!(!prompt.contains("CHILD FACE LOCK"));
        assert!(prompt.contains("人物安全约束"));
        assert!(prompt.contains("蓝色拓扑网格"));
    }
}

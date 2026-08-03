use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::backends::VimaxImage;
use crate::domain::CharacterInScene;
use crate::error::VimaxResult;

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

    fn style_line(style: &str) -> String {
        crate::planning::portrait_style_line_for_image(style)
    }

    fn medium_lock(style: &str) -> String {
        crate::planning::portrait_medium_lock_line(style)
    }

    fn face_guidance(character: &CharacterInScene, style: &str) -> String {
        crate::planning::portrait_face_clause_for_character(
            &character.identifier_in_scene,
            &Self::features_line(character),
            style,
        )
    }

    fn age_lock(character: &CharacterInScene, style: &str) -> String {
        crate::planning::child_style_lock_if_needed_for_style(
            &character.identifier_in_scene,
            &Self::features_line(character),
            style,
        )
    }

    fn build_three_view_prompt(character: &CharacterInScene, style: &str) -> String {
        include_str!(
            "../../prompts/character_portraits_generator__prompt_template_three_view.txt"
        )
        .replace("{identifier}", &character.identifier_in_scene)
        .replace("{features}", &Self::features_line(character))
        .replace("{style}", &Self::style_line(style))
        .replace("{medium_lock}", &Self::medium_lock(style))
        .replace("{face_guidance}", &Self::face_guidance(character, style))
        .replace("{age_lock}", &Self::age_lock(character, style))
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
    pub async fn generate_all_views(
        &self,
        character: &CharacterInScene,
        style: &str,
        _theme: &str,
        character_dir: &Path,
    ) -> VimaxResult<HashMap<String, HashMap<String, HashMap<String, String>>>> {
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
            let prompt = Self::build_three_view_prompt(character, style);
            self.image.generate(&prompt, &[], &sheet).await?;
        } else if !crate::media_local::is_usable_image_file(&sheet) {
            // e.g. JPEG bytes saved as .png without decode support — regenerate.
            let _ = tokio::fs::remove_file(&sheet).await;
            let prompt = Self::build_three_view_prompt(character, style);
            self.image.generate(&prompt, &[], &sheet).await?;
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
    fn three_view_prompt_puts_features_first_and_locks_one_identity() {
        let prompt = include_str!(
            "../../prompts/character_portraits_generator__prompt_template_three_view.txt"
        )
        .replace("{identifier}", "李薇")
        .replace(
            "{features}",
            "(static) red hanfu, black long hair; (dynamic) jade pendant",
        )
        .replace("{style}", "cinematic film look")
        .replace("{medium_lock}", "live-action cinematic")
        .replace("{face_guidance}", "Clean healthy skin")
        .replace("{age_lock}", "");
        let feat_pos = prompt.find("Features").expect("Features");
        let style_pos = prompt.find("Style:").expect("Style");
        assert!(feat_pos < style_pos);
        let lower = prompt.to_ascii_lowercase();
        assert!(lower.contains("same person") || lower.contains("one character"));
        assert!(lower.contains("three-panel") || lower.contains("three-view") || lower.contains("panels"));
        assert!(prompt.contains("red hanfu"));
        assert!(lower.contains("no dirt") || lower.contains("clean"));
        assert!(!lower.contains("theme lock"));
    }

    #[test]
    fn child_three_view_prompt_includes_child_face_lock() {
        let ch = CharacterInScene {
            idx: 0,
            identifier_in_scene: "小明".into(),
            is_visible: true,
            static_features: "8岁男孩，黑短发，白T恤".into(),
            dynamic_features: None,
            voice_profile: None,
        };
        let prompt = CharacterPortraitsGenerator::build_three_view_prompt(&ch, "cinematic film look");
        let lower = prompt.to_ascii_lowercase();
        assert!(lower.contains("child") || prompt.contains("儿童") || lower.contains("young"));
        assert!(lower.contains("makeup") || lower.contains("dirt") || prompt.contains("妆"));
    }
}

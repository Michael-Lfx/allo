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

    async fn cleanup_legacy_files(character_dir: &Path) {
        for name in ["front.png", "side.png", "back.png"] {
            let p = character_dir.join(name);
            if p.exists() {
                let _ = tokio::fs::remove_file(&p).await;
            }
        }
    }

    /// One character → one `three_view.png` (no separate front plate).
    pub async fn generate_all_views(
        &self,
        character: &CharacterInScene,
        style: &str,
        _theme: &str,
        character_dir: &Path,
    ) -> VimaxResult<HashMap<String, HashMap<String, HashMap<String, String>>>> {
        tokio::fs::create_dir_all(character_dir).await?;
        let sheet = character_dir.join("three_view.png");

        // Drop leftover discrete views from older pipelines.
        Self::cleanup_legacy_files(character_dir).await;

        if !sheet.exists() {
            let features = Self::features_line(character);
            let prompt = include_str!(
                "../../prompts/character_portraits_generator__prompt_template_three_view.txt"
            )
            .replace("{identifier}", &character.identifier_in_scene)
            .replace("{features}", &features)
            .replace("{style}", &Self::style_line(style))
            .replace("{medium_lock}", &Self::medium_lock(style));
            self.image.generate(&prompt, &[], &sheet).await?;
        } else if !crate::media_local::is_usable_image_file(&sheet) {
            // e.g. JPEG bytes saved as .png without decode support — regenerate.
            let _ = tokio::fs::remove_file(&sheet).await;
            let features = Self::features_line(character);
            let prompt = include_str!(
                "../../prompts/character_portraits_generator__prompt_template_three_view.txt"
            )
            .replace("{identifier}", &character.identifier_in_scene)
            .replace("{features}", &features)
            .replace("{style}", &Self::style_line(style))
            .replace("{medium_lock}", &Self::medium_lock(style));
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
                        "GLOBAL three-view character bible for <{id}> (left=front, center=side, right=back). Features: {feat_hint}. Lock identity to this sheet."
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
        .replace("{medium_lock}", "live-action cinematic");
        let feat_pos = prompt.find("Features").expect("Features");
        let style_pos = prompt.find("Style:").expect("Style");
        assert!(feat_pos < style_pos);
        let lower = prompt.to_ascii_lowercase();
        assert!(lower.contains("same person") || lower.contains("one character"));
        assert!(lower.contains("three-panel") || lower.contains("three-view") || lower.contains("panels"));
        assert!(prompt.contains("red hanfu"));
        assert!(!lower.contains("theme lock"));
    }
}

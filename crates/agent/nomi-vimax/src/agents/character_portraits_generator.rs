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

    fn features_line(character: &CharacterInScene) -> String {
        format!(
            "(static) {}; (dynamic) {}",
            character.static_features,
            character.dynamic_features.as_deref().unwrap_or("")
        )
    }

    pub async fn generate_front_portrait(
        &self,
        character: &CharacterInScene,
        style: &str,
        out_path: &Path,
    ) -> VimaxResult<()> {
        let prompt = include_str!(
            "../../prompts/character_portraits_generator__prompt_template_front.txt"
        )
        .replace("{identifier}", &character.identifier_in_scene)
        .replace("{features}", &Self::features_line(character))
        .replace("{style}", style);
        // Flowy Z-Image is text-only — no reference images in the request body.
        self.image.generate(&prompt, &[], out_path).await
    }

    pub async fn generate_side_portrait(
        &self,
        character: &CharacterInScene,
        style: &str,
        out_path: &Path,
    ) -> VimaxResult<()> {
        let prompt = include_str!(
            "../../prompts/character_portraits_generator__prompt_template_side.txt"
        )
        .replace("{identifier}", &character.identifier_in_scene)
        .replace("{features}", &Self::features_line(character))
        .replace("{style}", style);
        self.image.generate(&prompt, &[], out_path).await
    }

    pub async fn generate_back_portrait(
        &self,
        character: &CharacterInScene,
        style: &str,
        out_path: &Path,
    ) -> VimaxResult<()> {
        let prompt = include_str!(
            "../../prompts/character_portraits_generator__prompt_template_back.txt"
        )
        .replace("{identifier}", &character.identifier_in_scene)
        .replace("{features}", &Self::features_line(character))
        .replace("{style}", style);
        self.image.generate(&prompt, &[], out_path).await
    }

    /// Generate front/side/back under `character_dir`; return `{identifier -> views}`.
    pub async fn generate_all_views(
        &self,
        character: &CharacterInScene,
        style: &str,
        character_dir: &Path,
    ) -> VimaxResult<HashMap<String, HashMap<String, HashMap<String, String>>>> {
        tokio::fs::create_dir_all(character_dir).await?;
        let front = character_dir.join("front.png");
        let side = character_dir.join("side.png");
        let back = character_dir.join("back.png");

        if !front.exists() {
            self.generate_front_portrait(character, style, &front)
                .await?;
        }
        if !side.exists() {
            self.generate_side_portrait(character, style, &side).await?;
        }
        if !back.exists() {
            self.generate_back_portrait(character, style, &back).await?;
        }

        let mut views = HashMap::new();
        for (view, path) in [("front", &front), ("side", &side), ("back", &back)] {
            let mut item = HashMap::new();
            item.insert("path".into(), path.to_string_lossy().to_string());
            item.insert(
                "description".into(),
                format!(
                    "A {view}-view portrait of {}.",
                    character.identifier_in_scene
                ),
            );
            views.insert(view.to_string(), item);
        }
        let mut registry = HashMap::new();
        registry.insert(character.identifier_in_scene.clone(), views);
        Ok(registry)
    }
}

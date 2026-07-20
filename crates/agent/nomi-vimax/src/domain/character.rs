//! Character models (ViMax `interfaces/character.py`).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterInScene {
    pub idx: i32,
    pub identifier_in_scene: String,
    pub is_visible: bool,
    pub static_features: String,
    #[serde(default)]
    pub dynamic_features: Option<String>,
}

impl fmt::Display for CharacterInScene {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.identifier_in_scene)?;
        if self.is_visible {
            write!(f, "[visible]")?;
        } else {
            write!(f, "[not visible]")?;
        }
        writeln!(f)?;
        writeln!(f, "static features: {}", self.static_features)?;
        writeln!(
            f,
            "dynamic features: {}",
            self.dynamic_features.as_deref().unwrap_or("")
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterInEvent {
    pub index: i32,
    pub identifier_in_event: String,
    pub active_scenes: HashMap<i32, String>,
    pub static_features: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterInNovel {
    pub index: i32,
    pub identifier_in_novel: String,
    pub active_events: HashMap<i32, String>,
    pub static_features: String,
}

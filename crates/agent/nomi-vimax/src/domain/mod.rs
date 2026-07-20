//! Domain models — faithful port of ViMax `interfaces/*`.

mod camera;
mod character;
mod event;
mod shot;

pub use camera::*;
pub use character::*;
pub use event::*;
pub use shot::*;

use serde::{Deserialize, Serialize};

/// Which ViMax workflow a session uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkflowKind {
    #[serde(rename = "idea2video", alias = "idea2_video", alias = "idea")]
    Idea2Video,
    #[serde(rename = "script2video", alias = "script2_video", alias = "script")]
    Script2Video,
    #[serde(rename = "novel2video", alias = "novel2_video", alias = "novel", alias = "novel2movie")]
    Novel2Video,
}

impl WorkflowKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idea2Video => "idea2video",
            Self::Script2Video => "script2video",
            Self::Novel2Video => "novel2video",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        // Accept both canonical ids and serde snake_case variants (idea2_video).
        let key = s.trim().to_ascii_lowercase().replace('_', "");
        match key.as_str() {
            "idea2video" | "idea" => Some(Self::Idea2Video),
            "script2video" | "script" => Some(Self::Script2Video),
            "novel2video" | "novel" | "novel2movie" => Some(Self::Novel2Video),
            _ => None,
        }
    }

    pub fn artifact_root(self) -> &'static str {
        self.as_str()
    }
}

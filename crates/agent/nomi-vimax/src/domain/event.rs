//! Event / scene models for novel2video.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub index: i32,
    #[serde(default)]
    pub is_last: bool,
    pub description: String,
    #[serde(default)]
    pub characters: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    pub index: i32,
    #[serde(default)]
    pub is_last: bool,
    pub script: String,
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub characters: Vec<String>,
}

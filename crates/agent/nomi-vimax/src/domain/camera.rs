//! Camera model (ViMax `interfaces/camera.py`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Camera {
    pub idx: i32,
    pub active_shot_idxs: Vec<i32>,
    #[serde(default)]
    pub parent_cam_idx: Option<i32>,
    #[serde(default)]
    pub parent_shot_idx: Option<i32>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub is_parent_fully_covers_child: Option<bool>,
    #[serde(default)]
    pub missing_info: Option<String>,
}

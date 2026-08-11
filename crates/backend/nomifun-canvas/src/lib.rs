//! `nomifun-canvas` — Video Generation **Canvas** mode (DEV).
//!
//! Independent of Creative Workshop (`nomifun-workshop`). Persists infinite-canvas
//! projects inspired by open-ai-canvas document semantics, stores local media, and
//! runs image/video generation through Flowy (`nomi-media-backends`).
//!
//! On-disk layout under `{data_dir}/video-canvas/`:
//! - `projects/{id}/meta.json` — gallery metadata
//! - `projects/{id}/doc.json` — opaque frontend canvas document
//! - `media/{id}.{ext}` — uploaded / generated binaries
//! - `media/index.json` — media index
//! - `montage_project_links.json` — montage project_id → canvas project_id
//! - `vimax_session_links.json` — legacy ViMax session links (compat)

mod dto;
mod fsio;
mod generate;
mod llm_proxy;
mod routes;
mod service;
mod state;

pub use dto::{CanvasMediaMeta, CanvasProjectMeta, GenerationTaskView};
pub use routes::{video_canvas_public_routes, video_canvas_routes};
pub use service::CanvasService;
pub use state::CanvasRouterState;

/// Domain root under the backend data dir.
pub const CANVAS_REL_DIR: &str = "video-canvas";

/// Max serialized canvas doc size (8 MiB).
pub const MAX_DOC_BYTES: usize = 8 * 1024 * 1024;

/// Max uploaded / generated media size (256 MiB — video clips).
pub const MAX_MEDIA_BYTES: usize = 256 * 1024 * 1024;

/// Default empty canvas document (open-ai-canvas–compatible shape, schema 1).
pub(crate) const DEFAULT_DOC: &str = r#"{
  "schema": 1,
  "title": "",
  "nodes": [],
  "connections": [],
  "viewport": { "x": 0, "y": 0, "k": 1 },
  "backgroundMode": "lines"
}"#;

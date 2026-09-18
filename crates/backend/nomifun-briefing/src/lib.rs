//! `nomifun-briefing` — HTTP surface for news briefing (`/api/briefing/*`).
//!
//! Direct `nomi-briefing` dependency is intentional: Session must not invent
//! today's news via conversation `web_search`. Same exception class as
//! `nomifun-vimax` → `nomi-vimax`.

pub mod routes;
pub mod search;
pub mod service;
pub mod state;
pub mod stills;
pub mod tts;

pub use routes::briefing_routes;
pub use service::BriefingApiService;
pub use state::BriefingRouterState;

#[cfg(test)]
mod tests {
    use super::service::safe_artifact_path;

    #[test]
    fn artifact_path_stays_inside_working_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("briefing.mp4"), b"mp4").unwrap();
        let ok = safe_artifact_path(dir.path(), "briefing.mp4").unwrap();
        assert!(ok.ends_with("briefing.mp4"));
        assert!(safe_artifact_path(dir.path(), "../escape.txt").is_err());
    }
}

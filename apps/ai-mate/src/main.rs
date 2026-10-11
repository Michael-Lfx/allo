//! AI Mate Voice Companion Daemon Entry Point

use std::sync::Arc;
use ai_mate::config::Config;
use ai_mate::provision::check_and_auto_download;
use ai_mate::server::{build_router, DEFAULT_SERVER_ADDR};
use nomifun_voice_models::ModelManager;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let config = Config::load().unwrap_or_default();

    // Initialize voice model asset manager
    let model_manager = Arc::new(ModelManager::new(&config.models.models_dir));

    // Perform boot self-check and trigger auto-download if needed (Wave 1 T1-08)
    check_and_auto_download(&config, &model_manager).await;

    let addr = DEFAULT_SERVER_ADDR;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Flowy AI Mate daemon listening on {}", addr);
    let app = build_router(&config, model_manager);
    axum::serve(listener, app).await?;
    Ok(())
}

//! AI Mate Voice Companion Daemon Entry Point

use ai_mate::config::Config;
use ai_mate::server::{build_router, DEFAULT_SERVER_ADDR};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let config = Config::load().unwrap_or_default();
    let addr = DEFAULT_SERVER_ADDR;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Flowy AI Mate daemon listening on {}", addr);
    let app = build_router(&config);
    axum::serve(listener, app).await?;
    Ok(())
}

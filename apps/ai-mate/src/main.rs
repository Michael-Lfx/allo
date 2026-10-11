//! AI Mate Voice Companion Daemon Entry Point (Wave 4)

use std::path::Path;
use std::sync::Arc;

use ai_mate::config::Config;
use ai_mate::net_preboot::{detect_ethernet_status, import_usb_setup};
use ai_mate::oobe::OobeManager;
use ai_mate::provision::check_and_auto_download;
use ai_mate::server::{build_router_with_oobe, DEFAULT_SERVER_ADDR};
use ai_mate::watchdog::{get_recommended_heartbeat_interval, start_watchdog_heartbeat};
use nomifun_voice_models::ModelManager;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    info!("Starting Flowy AI Mate Voice Companion Daemon...");

    // 1. Start system watchdog heartbeat (WatchdogSec=30s, 15s interval per T4-01)
    let heartbeat_handle = start_watchdog_heartbeat(get_recommended_heartbeat_interval());

    // 2. Pre-network detection & USB setup import (T4-03)
    let eth_status = detect_ethernet_status();
    info!(?eth_status, "Physical network status detected");

    let usb_mount_candidate = Path::new("/media/usb");
    let target_config_path = Path::new("/etc/ai-mate/config.toml");
    if usb_mount_candidate.exists() {
        let _ = import_usb_setup(usb_mount_candidate, target_config_path);
    }

    // 3. Load configuration
    let config = Config::load().unwrap_or_default();

    // 4. Initialize voice model manager & OOBE manager (local single-node: bypass pairing)
    let model_manager = Arc::new(ModelManager::new(&config.models.models_dir));
    let require_pairing = std::env::var("AI_MATE_REQUIRE_PAIRING")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);
    let oobe_manager = if require_pairing {
        OobeManager::new()
    } else {
        OobeManager::new_bypassed()
    };

    // Log & vocal announcement
    let vocal_announcement = oobe_manager.vocal_announcement_text().await;
    info!(vocal = %vocal_announcement, "OOBE status initialized");

    // 5. Perform boot self-check and trigger auto-download if needed (T1-08)
    check_and_auto_download(&config, &model_manager).await;

    // 6. Bind to port 8080 and serve OOBE pairing page & voice model APIs
    let addr = DEFAULT_SERVER_ADDR;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("Flowy AI Mate daemon listening on {}", addr);
    let app = build_router_with_oobe(&config, model_manager, oobe_manager);
    axum::serve(listener, app).await?;

    let _ = heartbeat_handle.await;
    Ok(())
}

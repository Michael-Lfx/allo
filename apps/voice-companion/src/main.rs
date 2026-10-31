//! AI Mate Voice Companion Daemon Entry Point (Wave 4)

use std::path::{Path, PathBuf};
use std::sync::Arc;

use voice_companion::config::Config;
use voice_companion::net_preboot::{detect_ethernet_status, import_usb_setup};
use voice_companion::oobe::OobeManager;
use voice_companion::provision::check_and_auto_download;
use voice_companion::server::{DEFAULT_SERVER_ADDR, build_router_full};
use voice_companion::voice::VoiceCompanionRuntime;
use voice_companion::watchdog::{
    get_recommended_heartbeat_interval, notify_ready, start_watchdog_heartbeat,
};
use nomifun_voice_models::ModelManager;
use tracing::{error, info};

struct CliArgs {
    config_path: Option<PathBuf>,
    host: Option<String>,
    bypass_pairing: bool,
}

fn parse_cli_args() -> Result<CliArgs, Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut config_path = None;
    let mut host = None;
    let mut bypass_pairing = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" | "-c" => {
                if let Some(val) = args.next() {
                    config_path = Some(PathBuf::from(val));
                } else {
                    return Err("--config requires a path argument".into());
                }
            }
            "--host" => {
                if let Some(val) = args.next() {
                    host = Some(val);
                } else {
                    return Err("--host requires an address argument".into());
                }
            }
            "--bypass-pairing" => {
                bypass_pairing = true;
            }
            "--help" | "-h" => {
                println!("Flowy Voice Companion Daemon");
                println!();
                println!("Usage: voice-companion [OPTIONS]");
                println!();
                println!("Options:");
                println!("  -c, --config <PATH>      Path to TOML configuration file");
                println!(
                    "      --host <ADDR>        Address to bind server to (default: {})",
                    DEFAULT_SERVER_ADDR
                );
                println!(
                    "      --bypass-pairing     Bypass OOBE pairing (development/single-node)"
                );
                println!("  -h, --help               Print help information");
                std::process::exit(0);
            }
            unknown => {
                return Err(format!("Unknown option: {}", unknown).into());
            }
        }
    }

    Ok(CliArgs {
        config_path,
        host,
        bypass_pairing,
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_ansi(false).init();
    info!("Starting Flowy Voice Companion Daemon...");

    let cli = parse_cli_args()?;

    // 1. Start system watchdog heartbeat (WatchdogSec=30s, 15s interval per T4-01)
    let heartbeat_handle = start_watchdog_heartbeat(get_recommended_heartbeat_interval());

    // 2. Pre-network detection & USB setup import (T4-03)
    let eth_status = detect_ethernet_status();
    info!(?eth_status, "Physical network status detected");

    let usb_mount_candidate = Path::new("/media/usb");
    let target_config_path = Path::new("/etc/voice-companion/config.toml");
    if usb_mount_candidate.exists() {
        let _ = import_usb_setup(usb_mount_candidate, target_config_path);
    }

    // 3. Load configuration (from CLI path or default platform path)
    let config = if let Some(ref path) = cli.config_path {
        info!(path = %path.display(), "Loading configuration from CLI argument");
        Config::load_from_file(path)?
    } else {
        Config::load()?
    };
    info!(models_dir = %config.models.models_dir.display(), "Configuration loaded");
    let addr = cli.host.as_deref().unwrap_or(DEFAULT_SERVER_ADDR);
    if config.control_token.is_some() && !addr.parse::<std::net::SocketAddr>()?.ip().is_loopback() {
        return Err("supervised voice control must bind a loopback address".into());
    }

    // 4. Initialize voice model manager & OOBE manager
    let model_manager = Arc::new(ModelManager::new(&config.models.models_dir));

    let env_pairing = std::env::var("AI_MATE_REQUIRE_PAIRING").ok();
    let require_pairing = if cli.bypass_pairing {
        false
    } else if let Some(v) = env_pairing {
        v != "false" && v != "0"
    } else {
        true
    };

    let oobe_manager = if require_pairing {
        Arc::new(OobeManager::new())
    } else {
        Arc::new(OobeManager::new_bypassed())
    };

    // Log vocal announcement
    let vocal_announcement = oobe_manager.vocal_announcement_text().await;
    info!(vocal = %vocal_announcement, require_pairing, "OOBE status initialized");

    // 5. Assemble and start end-to-end Voice Companion Pipeline (Capture + Playback + FSM Driver + Router)
    let feedback = std::env::var("AI_MATE_FEEDBACK_CONFIG")
        .ok()
        .map(|raw| serde_json::from_str::<nomifun_api_types::voice::VoiceFeedbackConfig>(&raw))
        .transpose()?;
    if let Some(ref config) = feedback {
        config.validate()?;
    }
    let markers = voice_companion::voice::parse_startup_markers(
        std::env::var_os("NOMIFUN_VOICE_FAULT_HEADLESS").as_deref(),
        std::env::var_os("NOMIFUN_VOICE_FAULT_BLOCK_ASR").as_deref(),
        std::env::var_os("NOMIFUN_VOICE_FAULT_EXIT_CODE").as_deref(),
    );
    let startup_mode =
        voice_companion::voice::startup_mode(cfg!(feature = "native-fault-injection"), markers)?;
    let voice_runtime = match startup_mode {
        voice_companion::voice::StartupMode::HeadlessTest => {
            #[cfg(feature = "native-fault-injection")]
            {
                VoiceCompanionRuntime::assemble_headless(
                    model_manager.clone(),
                    oobe_manager.clone(),
                )
            }
            #[cfg(not(feature = "native-fault-injection"))]
            {
                return Err("native_headless_unavailable".into());
            }
        }
        voice_companion::voice::StartupMode::Default => {
            VoiceCompanionRuntime::assemble(model_manager.clone(), oobe_manager.clone())
        }
    };
    if let Some(config) = feedback {
        voice_runtime.update_feedback(config.into())?;
    }
    let voice_runtime = Arc::new(voice_runtime);

    // Start voice watchdog and background FSM worker
    let _voice_worker = voice_runtime.start_worker();
    info!("AI Mate voice pipeline assembled and standing by");

    // 6. Perform boot self-check and trigger auto-download if needed (T1-08)
    check_and_auto_download(&config, &model_manager).await;

    // 7. Bind server address and launch HTTP daemon
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("Flowy AI Mate daemon listening on {}", addr);

    // 8. Notify systemd / supervisor of service readiness
    notify_ready();

    let app = build_router_full(
        &config,
        model_manager,
        (*oobe_manager).clone(),
        Some(voice_runtime.event_sender()),
        Some(voice_runtime.clone()),
    );
    let server_task = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            error!(error = %e, "HTTP server fatal error");
        }
    });

    tokio::select! {
        _ = server_task => {},
        _ = heartbeat_handle => {},
    }

    Ok(())
}

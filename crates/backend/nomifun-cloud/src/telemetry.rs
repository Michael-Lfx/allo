//! Cloud presence heartbeat and startup device telemetry.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use nomi_config::ServerConfig;
use tracing::{debug, warn};

use crate::activation::DeviceActivation;
use crate::flowy::FlowyApiClient;
use crate::http_service::CloudService;
use crate::session::ServerSession;

/// Best-effort device activation + client package report for an existing session.
///
/// Called on app startup when tokens are already persisted (token restore does not
/// re-run `finish_login`, so activation would otherwise be skipped until re-login).
pub async fn ensure_device_telemetry(service: &CloudService) {
    let mgr = match service.auth_manager() {
        Ok(m) => m,
        Err(err) => {
            debug!(error = %err, "skip device telemetry — auth manager unavailable");
            return;
        }
    };

    let logged_in = match mgr.whoami().await {
        Ok(status) => status.is_logged_in(),
        Err(err) => {
            warn!(error = %err, "skip device telemetry — whoami failed");
            return;
        }
    };
    if !logged_in {
        return;
    }

    if let Err(err) = mgr.ensure_device_activation().await {
        warn!(error = %err, "device activation check on startup failed");
    }
    if let Err(err) = mgr.api().report_client_package(mgr.session()).await {
        warn!(error = %err, "client package report on startup failed");
    }
}

/// Best-effort device activation + client package report for a freshly logged-in
/// session, spawned off the login request path so `loginContinue` returns as soon
/// as tokens and profile are saved. Startup `ensure_device_telemetry` backfills
/// if the process exits before this task finishes.
pub fn spawn_post_login_telemetry(
    config: ServerConfig,
    data_dir: PathBuf,
    session: ServerSession,
    user_id: i64,
) {
    tokio::spawn(async move {
        let api = match FlowyApiClient::new(&config) {
            Ok(api) => api,
            Err(err) => {
                warn!(error = %err, "skip post-login telemetry — api client unavailable");
                return;
            }
        };
        if let Err(err) = DeviceActivation::new(&data_dir)
            .try_activate_for_user(&api, &session, user_id)
            .await
        {
            warn!(error = %err, "device activation failed after login");
        }
        if let Err(err) = api.report_client_package(&session).await {
            warn!(error = %err, "client package report failed after login");
        }
    });
}

async fn send_presence_heartbeat(service: &CloudService) {
    let mgr = match service.auth_manager() {
        Ok(m) => m,
        Err(_) => return,
    };
    match mgr.api().presence_heartbeat(mgr.session()).await {
        Ok(()) => debug!("presence heartbeat sent"),
        Err(err) => warn!(error = %err, "presence heartbeat failed"),
    }
}

/// Spawn a background loop that sends presence heartbeats while logged in.
pub fn spawn_presence_loop(service: Arc<CloudService>) {
    tokio::spawn(async move {
        loop {
            if service.is_authenticated().await {
                send_presence_heartbeat(&service).await;
            }

            let interval_secs = service
                .gateway_config_snapshot()
                .server
                .auth
                .heartbeat_interval_secs
                .max(15);
            tokio::time::sleep(Duration::from_secs(interval_secs)).await;
        }
    });
}

/// Startup hook: backfill activation for restored sessions and begin presence heartbeats.
pub fn start_cloud_telemetry(service: Arc<CloudService>) {
    tokio::spawn({
        let service = service.clone();
        async move {
            ensure_device_telemetry(&service).await;
        }
    });
    spawn_presence_loop(service);
}

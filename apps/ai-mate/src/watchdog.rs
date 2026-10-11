//! System watchdog integration (Linux sd_notify / cross-platform watchdog).
//!
//! Baseline:
//! - `interaction.zh.md` §3.1: "接入 systemd 看门狗机制：每隔 15 秒通过 libsystemd / sd_notify 发送 WATCHDOG=1 心跳（心跳周期须显著小于 WatchdogSec，取约 1/2 安全水位，防止调度抖动误杀）"
//! - `execution/wave-4-hardening.zh.md` §2 (T4-01)

use std::time::Duration;
use tracing::{debug, info};

/// Configured WatchdogSec timeout in systemd unit (30 seconds).
pub const WATCHDOG_TIMEOUT_SECS: u64 = 30;

/// Recommended heartbeat interval = 1/2 of WatchdogSec (15 seconds) per baseline §3.1.
pub fn get_recommended_heartbeat_interval() -> Duration {
    Duration::from_secs(WATCHDOG_TIMEOUT_SECS / 2)
}

/// Send a single watchdog heartbeat notification.
///
/// On Linux with systemd, this communicates with `$NOTIFY_SOCKET`.
/// On other platforms or standalone dev environments, it logs heartbeat at debug level.
pub fn notify_watchdog() -> bool {
    #[cfg(target_os = "linux")]
    {
        if let Ok(socket_path) = std::env::var("NOTIFY_SOCKET") {
            use std::os::unix::net::UnixDatagram;
            if let Ok(socket) = UnixDatagram::unbound() {
                let _ = socket.send_to(b"WATCHDOG=1", socket_path);
                return true;
            }
        }
    }

    debug!("Watchdog heartbeat pulse dispatched");
    true
}

/// Spawn the periodic watchdog heartbeat task.
pub fn start_watchdog_heartbeat(interval: Duration) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        info!(
            interval_secs = interval.as_secs(),
            "Starting system watchdog heartbeat loop"
        );
        let mut ticker = tokio::time::interval(interval);
        loop {
            ticker.tick().await;
            notify_watchdog();
        }
    })
}

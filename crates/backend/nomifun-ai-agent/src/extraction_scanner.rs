//! Background scanner: flush POI / insights for idle conversations.

use std::sync::Arc;
use std::time::Duration;

use tracing::{debug, info};

use crate::capability::proactive_extraction::ProactiveSessionExtractor;

const DEFAULT_SCAN_INTERVAL_SECS: u64 = 60;

pub type MessageCountLoader = Arc<
    dyn Fn(String) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Option<usize>> + Send>,
        > + Send
        + Sync,
>;

/// Start a background task that periodically flushes idle sessions.
pub fn start_session_extraction_scanner(
    extractor: Arc<ProactiveSessionExtractor>,
    message_counts: MessageCountLoader,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
    scan_interval_secs: Option<u64>,
) -> tokio::task::JoinHandle<()> {
    let scan_interval = scan_interval_secs.unwrap_or(DEFAULT_SCAN_INTERVAL_SECS);
    info!(
        scan_interval_secs = scan_interval,
        "Starting session extraction scanner"
    );

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(scan_interval));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    scan_idle_sessions(&extractor, &message_counts).await;
                }
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!("Session extraction scanner received shutdown signal");
                        break;
                    }
                }
            }
        }

        info!("Session extraction scanner stopped");
    })
}

async fn scan_idle_sessions(
    extractor: &Arc<ProactiveSessionExtractor>,
    message_counts: &MessageCountLoader,
) {
    let session_ids = extractor.tracked_session_ids();
    if session_ids.is_empty() {
        debug!("session_extraction scan: no tracked sessions");
        return;
    }

    for session_id in session_ids {
        let count = match message_counts(session_id.clone()).await {
            Some(n) => n,
            None => continue,
        };
        extractor.flush_idle_session(&session_id, count).await;
    }
}

//! Batch session-observation summaries onto the product telemetry warehouse.
//!
//! The JSONL observation log stays local. This queue only forwards catalogued
//! scalars (`tool_executed`, `llm_request`) through the existing cloud ingest.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nomifun_ai_agent::{set_observation_telemetry_hook, ObservationTelemetryRecord};
use nomifun_api_types::{VideoGrowthEvent, VideoGrowthEventBatchRequest};
use nomifun_cloud::CloudService;
use tracing::warn;

const QUEUE_CAP: usize = 500;
const BATCH_SIZE: usize = 50;
const FLUSH_SECS: u64 = 5;

struct ObservationTelemetryQueue {
    items: VecDeque<VideoGrowthEvent>,
}

pub fn start_observation_telemetry(cloud: Arc<CloudService>) {
    let queue = Arc::new(Mutex::new(ObservationTelemetryQueue {
        items: VecDeque::new(),
    }));
    let enqueue = queue.clone();
    set_observation_telemetry_hook(Arc::new(move |record: ObservationTelemetryRecord| {
        let event = VideoGrowthEvent {
            event_id: record.event_id,
            name: record.name,
            occurred_at: record.occurred_at,
            module: Some("conversation".into()),
            properties: record.properties,
            cohort: None,
        };
        let mut guard = enqueue.lock().unwrap_or_else(|error| error.into_inner());
        if guard.items.len() >= QUEUE_CAP {
            guard.items.pop_front();
        }
        guard.items.push_back(event);
    }));

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(FLUSH_SECS)).await;
            flush_observation_telemetry(&cloud, &queue).await;
        }
    });
}

async fn flush_observation_telemetry(
    cloud: &CloudService,
    queue: &Arc<Mutex<ObservationTelemetryQueue>>,
) {
    if !cloud.is_authenticated().await {
        return;
    }
    let events = {
        let mut guard = queue.lock().unwrap_or_else(|error| error.into_inner());
        let take = guard.items.len().min(BATCH_SIZE);
        guard.items.drain(..take).collect::<Vec<_>>()
    };
    if events.is_empty() {
        return;
    }
    if let Err(error) = cloud
        .upload_video_growth_events(&VideoGrowthEventBatchRequest {
            events,
            client_id: None,
            app: None,
            platform: None,
            app_version: None,
        })
        .await
    {
        warn!(%error, "observation telemetry upload failed");
    }
}

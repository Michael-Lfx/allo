//! Batch session-observation summaries onto the product telemetry warehouse.
//!
//! The JSONL observation log stays local. This queue only forwards catalogued
//! scalars (tool, model-call, harness and turn summaries) through the existing
//! cloud ingest. Events carry deterministic or one-shot ids, so a batch that
//! is sent twice is deduplicated by the server.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use nomifun_ai_agent::{set_observation_telemetry_hook, ObservationTelemetryRecord};
use nomifun_api_types::{VideoGrowthEvent, VideoGrowthEventBatchRequest};
use nomifun_cloud::CloudService;
use tracing::warn;

const QUEUE_CAP: usize = 500;
const BATCH_SIZE: usize = 50;
const MAX_BATCHES_PER_FLUSH: usize = 4;
const MAX_CONSECUTIVE_FAILURES: u32 = 3;
const FLUSH_SECS: u64 = 5;

#[derive(Default)]
struct ObservationTelemetryQueue {
    items: VecDeque<VideoGrowthEvent>,
    dropped: u64,
    consecutive_failures: u32,
}

impl ObservationTelemetryQueue {
    fn push(&mut self, event: VideoGrowthEvent) {
        if self.items.len() >= QUEUE_CAP {
            self.items.pop_front();
            self.dropped += 1;
        }
        self.items.push_back(event);
    }

    fn take_batch(&mut self) -> Vec<VideoGrowthEvent> {
        let take = self.items.len().min(BATCH_SIZE);
        self.items.drain(..take).collect()
    }

    fn take_dropped(&mut self) -> u64 {
        std::mem::take(&mut self.dropped)
    }

    fn record_success(&mut self) {
        self.consecutive_failures = 0;
    }

    /// Put a failed batch back in front of newer events so the next flush
    /// retries it. A failure that keeps repeating is treated as unsendable and
    /// the batch is discarded. Returns how many events were kept.
    fn record_failure(&mut self, batch: Vec<VideoGrowthEvent>) -> usize {
        self.consecutive_failures += 1;
        if self.consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
            self.consecutive_failures = 0;
            self.dropped += batch.len() as u64;
            return 0;
        }
        let room = QUEUE_CAP.saturating_sub(self.items.len());
        let keep = batch.len().min(room);
        self.dropped += (batch.len() - keep) as u64;
        let skip = batch.len() - keep;
        for event in batch.into_iter().skip(skip).rev() {
            self.items.push_front(event);
        }
        keep
    }
}

fn locked(queue: &Mutex<ObservationTelemetryQueue>) -> MutexGuard<'_, ObservationTelemetryQueue> {
    queue.lock().unwrap_or_else(|error| error.into_inner())
}

pub fn start_observation_telemetry(cloud: Arc<CloudService>) {
    let queue = Arc::new(Mutex::new(ObservationTelemetryQueue::default()));
    let enqueue = queue.clone();
    set_observation_telemetry_hook(Arc::new(move |record: ObservationTelemetryRecord| {
        locked(&enqueue).push(VideoGrowthEvent {
            event_id: record.event_id,
            name: record.name,
            occurred_at: record.occurred_at,
            module: Some("conversation".into()),
            properties: record.properties,
            cohort: None,
        });
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
    let dropped = locked(queue).take_dropped();
    if dropped > 0 {
        warn!(dropped, "observation telemetry events were discarded");
    }
    for _ in 0..MAX_BATCHES_PER_FLUSH {
        let events = locked(queue).take_batch();
        if events.is_empty() {
            return;
        }
        let request = VideoGrowthEventBatchRequest {
            events,
            client_id: None,
            app: None,
            platform: None,
            app_version: None,
        };
        match cloud.upload_video_growth_events(&request).await {
            Ok(response) => {
                locked(queue).record_success();
                if response.rejected > 0 {
                    warn!(
                        accepted = response.accepted,
                        duplicates = response.duplicates,
                        rejected = response.rejected,
                        "observation telemetry batch was partly rejected"
                    );
                }
            }
            Err(error) => {
                let retained = locked(queue).record_failure(request.events);
                warn!(%error, retained, "observation telemetry upload failed");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn event(id: usize) -> VideoGrowthEvent {
        VideoGrowthEvent {
            event_id: format!("obs:test:{id}"),
            name: "tool_executed".into(),
            occurred_at: "2026-10-03T00:00:00Z".into(),
            module: Some("conversation".into()),
            properties: BTreeMap::new(),
            cohort: None,
        }
    }

    fn ids(batch: &[VideoGrowthEvent]) -> Vec<&str> {
        batch.iter().map(|event| event.event_id.as_str()).collect()
    }

    #[test]
    fn full_queue_drops_the_oldest_event_and_counts_it() {
        let mut queue = ObservationTelemetryQueue::default();
        for id in 0..QUEUE_CAP + 3 {
            queue.push(event(id));
        }

        assert_eq!(queue.items.len(), QUEUE_CAP);
        assert_eq!(queue.items.front().unwrap().event_id, "obs:test:3");
        assert_eq!(queue.take_dropped(), 3);
        assert_eq!(queue.take_dropped(), 0);
    }

    #[test]
    fn batches_are_capped_and_keep_arrival_order() {
        let mut queue = ObservationTelemetryQueue::default();
        for id in 0..BATCH_SIZE + 2 {
            queue.push(event(id));
        }

        let first = queue.take_batch();
        assert_eq!(first.len(), BATCH_SIZE);
        assert_eq!(first[0].event_id, "obs:test:0");
        assert_eq!(ids(&queue.take_batch()), ["obs:test:50", "obs:test:51"]);
        assert!(queue.take_batch().is_empty());
    }

    #[test]
    fn failed_batch_is_retried_ahead_of_newer_events() {
        let mut queue = ObservationTelemetryQueue::default();
        queue.push(event(0));
        queue.push(event(1));
        let failed = queue.take_batch();
        queue.push(event(2));

        assert_eq!(queue.record_failure(failed), 2);

        assert_eq!(ids(&queue.take_batch()), ["obs:test:0", "obs:test:1", "obs:test:2"]);
        assert_eq!(queue.take_dropped(), 0);
    }

    #[test]
    fn repeated_failures_discard_the_batch_instead_of_retrying_forever() {
        let mut queue = ObservationTelemetryQueue::default();
        for round in 0..MAX_CONSECUTIVE_FAILURES {
            queue.push(event(round as usize));
            let batch = queue.take_batch();
            let kept = queue.record_failure(batch);
            if round + 1 < MAX_CONSECUTIVE_FAILURES {
                assert_eq!(kept, 1);
                assert_eq!(queue.take_batch().len(), 1);
            } else {
                assert_eq!(kept, 0);
            }
        }

        assert!(queue.items.is_empty());
        assert_eq!(queue.take_dropped(), 1);
        assert_eq!(queue.consecutive_failures, 0);
    }

    #[test]
    fn success_resets_the_failure_streak() {
        let mut queue = ObservationTelemetryQueue::default();
        queue.push(event(0));
        let batch = queue.take_batch();
        queue.record_failure(batch);
        assert_eq!(queue.consecutive_failures, 1);

        queue.record_success();

        assert_eq!(queue.consecutive_failures, 0);
    }

    #[test]
    fn retry_keeps_the_newest_failed_events_when_the_queue_has_no_room() {
        let mut queue = ObservationTelemetryQueue::default();
        let failed: Vec<_> = (0..10).map(event).collect();
        for id in 100..100 + QUEUE_CAP - 4 {
            queue.push(event(id));
        }

        assert_eq!(queue.record_failure(failed), 4);

        assert_eq!(queue.items.len(), QUEUE_CAP);
        let retried: Vec<_> = queue.items.iter().take(4).map(|e| e.event_id.clone()).collect();
        assert_eq!(retried, ["obs:test:6", "obs:test:7", "obs:test:8", "obs:test:9"]);
        assert_eq!(queue.take_dropped(), 6);
    }
}

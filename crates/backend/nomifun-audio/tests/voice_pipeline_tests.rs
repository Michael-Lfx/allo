use nomifun_audio::capture::CaptureReadResult;
use nomifun_audio::voice_pipeline::{
    PipelineConfig, SpscRingBuffer, StubKwsDetector, StubKwsTrigger, VoiceCapturePipeline,
};

#[tokio::test]
async fn test_replay_pipeline_produces_chunks_and_detects_wake() {
    // 16kHz mono audio, 1000 samples (~62.5ms)
    // Put a high amplitude burst in the middle to trigger energy-based stub KWS
    let mut pcm = vec![0.05f32; 1000];
    for sample in pcm.iter_mut().take(600).skip(400) {
        *sample = 0.85f32;
    }

    let mut pipeline = VoiceCapturePipeline::from_replay(16000, 1, pcm, 160);
    let mut kws = StubKwsDetector::new(StubKwsTrigger::EnergyThreshold(0.5));

    let mut chunks_received = 0usize;
    let mut wake_detected = false;

    loop {
        match pipeline.next_chunk().await {
            CaptureReadResult::Chunk(chunk) => {
                chunks_received += 1;
                assert!(!chunk.is_empty(), "Chunk must not be empty");
                if let Some(hit) = kws.process_chunk(&chunk) {
                    assert_eq!(hit.keyword, "HEY FLOWY");
                    assert!(hit.score > 0.0);
                    wake_detected = true;
                }
            }
            CaptureReadResult::TemporarilyEmpty => {
                tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
            }
            CaptureReadResult::StreamClosed => break,
            CaptureReadResult::DeviceError(err) => panic!("Unexpected device error: {err}"),
        }
    }

    assert!(chunks_received > 0, "Must receive at least one chunk");
    assert!(wake_detected, "Stub KWS must trigger on high-energy burst");
}

#[tokio::test]
async fn test_pipeline_backlog_overflow_protection() {
    let config = PipelineConfig {
        sample_rate: 16000,
        channels: 1,
        chunk_size: 160,
        capacity_samples: 16000, // 1 second
        backlog_limit_ms: 50,    // 50ms = 800 samples
    };

    let (mut pipeline, producer) = VoiceCapturePipeline::new(config);

    // Push 3000 samples (~187ms) at once, significantly exceeding the 50ms (800 samples) backlog limit
    let large_batch = vec![0.1f32; 3000];
    let written = producer.push(&large_batch);
    assert!(written > 0);

    // Let the worker process and detect the overflow
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    assert!(
        pipeline.overflow_count() > 0,
        "Worker must drop backlog and record overflow_count"
    );

    // Pipeline must still be readable without deadlock
    let res = pipeline.next_chunk().await;
    match res {
        CaptureReadResult::Chunk(chunk) => assert!(!chunk.is_empty()),
        _ => panic!("Expected chunk after overflow recovery"),
    }

    producer.close();
}

#[test]
fn test_standalone_spsc_ring_buffer_operations() {
    let ring = SpscRingBuffer::new(100);
    assert_eq!(ring.available(), 0);
    assert_eq!(ring.overflow_count(), 0);

    let written = ring.push(&[0.1f32, 0.2, 0.3]);
    assert_eq!(written, 3);
    assert_eq!(ring.available(), 3);

    ring.close();
}

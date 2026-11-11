//! Independent process probe for the self-check playback consumption contract.

use async_trait::async_trait;
use nomifun_voice_terminal::playback::{AudioSink, AudioSinkError, PlaybackPipeline};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Counters {
    queued: usize,
    consumed: usize,
    discarded: usize,
}

#[derive(Default)]
struct QueuedSink(Mutex<Counters>);

#[async_trait]
impl AudioSink for QueuedSink {
    async fn write_frame(&self, frame: Vec<u8>) -> Result<(), AudioSinkError> {
        self.0.lock().unwrap().queued += frame.len();
        Ok(())
    }
    async fn drain(&self) -> Result<(), AudioSinkError> {
        let mut counters = self.0.lock().unwrap();
        counters.consumed += counters.queued;
        counters.queued = 0;
        Ok(())
    }
    async fn flush(&self) -> Result<(), AudioSinkError> {
        let mut counters = self.0.lock().unwrap();
        counters.discarded += counters.queued;
        counters.queued = 0;
        Ok(())
    }
}

fn main() {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};
    let sink = Arc::new(QueuedSink::default());
    let pipeline = PlaybackPipeline::new(sink.clone());
    let mut future = Box::pin(pipeline.play_wav_for_self_check(vec![1; 100]));
    let mut context = Context::from_waker(Waker::noop());
    let result = match future.as_mut().poll(&mut context) {
        Poll::Ready(value) => value,
        Poll::Pending => {
            eprintln!("unexpected_pending_no_runtime_probe");
            std::process::exit(2);
        }
    };
    let counters = sink.0.lock().unwrap();
    println!(
        "result={result:?} consumed={} discarded={} queued={}",
        counters.consumed, counters.discarded, counters.queued
    );
    if result.is_ok() && counters.consumed == 100 && counters.discarded == 0 {
        return;
    }
    std::process::exit(1);
}
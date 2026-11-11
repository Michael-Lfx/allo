//! Exact replica of parent-counterexamples.rs to be executed under Cargo test runner.
//!
//! Asserts the 4 counterexamples from parent review without any modification to expectations:
//! 1. active_lease_must_prevent_reap_and_second_instance
//! 2. aborting_waiter_must_not_strand_completed_load
//! 3. wake_must_wait_for_actual_unload_completion
//! 4. cancelled_generation_must_not_be_returned_for_dispatch

#![cfg(feature = "voice-skeleton")]

#[path = "../src/voice/asr_lifecycle.rs"]
mod candidate;
use candidate::*;
use std::sync::{Arc, atomic::{AtomicBool, AtomicUsize, Ordering}};
use std::time::{Duration, Instant};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap()
}

async fn until(flag: &AtomicBool) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !flag.load(Ordering::SeqCst) { tokio::task::yield_now().await; }
    }).await.expect("bounded gate did not open");
}

struct Model { live: Arc<AtomicUsize> }
impl Drop for Model { fn drop(&mut self) { self.live.fetch_sub(1, Ordering::SeqCst); } }

#[test]
fn active_lease_must_prevent_reap_and_second_instance() {
    runtime().block_on(async {
        let live = Arc::new(AtomicUsize::new(0));
        let count = live.clone();
        let owner = AsrLifecycleOwner::new(Duration::ZERO, Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(Arc::new(Model { live: count.clone() }))
        }), None);
        owner.request_wake().await.unwrap();
        let lease = owner.acquire_transcriber().await.unwrap();
        let reaped = owner.try_reap_idle().await;
        owner.request_wake().await.unwrap();
        let simultaneous_instances = live.load(Ordering::SeqCst);
        println!("reaped_with_lease={reaped}, live_instances={simultaneous_instances}");
        drop(lease);
        drop(owner);
        assert!(simultaneous_instances <= 1, "active lease overlapped a new instance");
    });
}

#[test]
fn aborting_waiter_must_not_strand_completed_load() {
    runtime().block_on(async {
        let entered = Arc::new(AtomicBool::new(false));
        let exited = Arc::new(AtomicBool::new(false));
        let (release, gate) = std::sync::mpsc::channel::<()>();
        let gate = std::sync::Mutex::new(gate);
        let start = entered.clone(); let done = exited.clone();
        let owner = Arc::new(AsrLifecycleOwner::new(Duration::from_secs(300), Arc::new(move || {
            start.store(true, Ordering::SeqCst);
            gate.lock().unwrap().recv_timeout(Duration::from_secs(2)).unwrap();
            done.store(true, Ordering::SeqCst);
            Ok(Arc::new(1u8))
        }), None));
        let own = owner.clone();
        let waiter = tokio::spawn(async move { own.request_wake().await });
        until(&entered).await;
        waiter.abort(); let _ = waiter.await;
        release.send(()).unwrap(); until(&exited).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let state = owner.current_state().await;
        let busy = owner.supervision().asr_busy.load(Ordering::SeqCst);
        println!("native_loader_returned=true, state={state:?}, busy={busy}");
        assert!(state != AsrState::Loading && !busy, "completed loader stranded after waiter abort");
    });
}

#[test]
fn wake_must_wait_for_actual_unload_completion() {
    runtime().block_on(async {
        let live = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let unload_entered = Arc::new(AtomicBool::new(false));
        let second_load = Arc::new(AtomicBool::new(false));
        let (release, gate) = std::sync::mpsc::channel::<()>();
        let gate = std::sync::Mutex::new(gate);
        let count = live.clone(); let max = peak.clone(); let second = second_load.clone();
        let unload = unload_entered.clone();
        let owner = Arc::new(AsrLifecycleOwner::new(Duration::ZERO, Arc::new(move || {
            let n = count.fetch_add(1, Ordering::SeqCst) + 1;
            max.fetch_max(n, Ordering::SeqCst);
            if n > 1 { second.store(true, Ordering::SeqCst); }
            Ok(Arc::new(Model { live: count.clone() }))
        }), Some(Arc::new(move |model| {
            unload.store(true, Ordering::SeqCst);
            gate.lock().unwrap().recv_timeout(Duration::from_secs(2)).unwrap();
            drop(model);
        }))));
        owner.request_wake().await.unwrap();
        let own = owner.clone();
        let reaper = tokio::spawn(async move { own.try_reap_idle().await });
        until(&unload_entered).await;
        let own = owner.clone();
        let waking = tokio::spawn(async move { own.request_wake().await });
        let overlap = tokio::time::timeout(Duration::from_millis(500), until(&second_load)).await.is_ok();
        release.send(()).unwrap();
        reaper.await.unwrap(); waking.await.unwrap().unwrap();
        let actual_peak = peak.load(Ordering::SeqCst);
        println!("new_loader_started_before_old_drop={overlap}, peak_live={actual_peak}");
        assert!(actual_peak <= 1, "loading overlaps unfinished destructor");
    });
}

#[test]
fn cancelled_generation_must_not_be_returned_for_dispatch() {
    runtime().block_on(async {
        let owner = AsrLifecycleOwner::new(Duration::from_secs(300), Arc::new(|| Ok(Arc::new(1u8))), None);
        let old = owner.current_generation();
        owner.buffer_cold_utterance("A".into(), 1, old, vec![0.0; 320], Instant::now()+Duration::from_secs(30)).await.unwrap();
        let new = owner.next_generation();
        let returned = owner.take_cold_utterance().await.unwrap();
        println!("current_generation={new}, returned_generation={:?}", returned.as_ref().map(|x| x.generation_id));
        assert!(returned.is_none(), "cancelled generation remains dispatchable");
    });
}

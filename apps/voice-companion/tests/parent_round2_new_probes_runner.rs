//! Exact replica of parent review round 2 new-probe-body.rs.
//!
//! Validates:
//! 1. aborting_reaper_must_collect_actual_drop_completion
//! 2. acquire_check_and_reap_must_share_atomic_admission
//! 3. cancelled_transcription_must_retain_native_ownership_until_real_return

#![cfg(feature = "voice-skeleton")]

#[path = "../src/voice/asr_lifecycle.rs"]
mod candidate;
use candidate::*;
use std::sync::{Arc, atomic::{AtomicBool, AtomicUsize, Ordering}};
use std::time::{Duration, Instant};

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap()
}

async fn flag(value: &AtomicBool) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !value.load(Ordering::SeqCst) { tokio::task::yield_now().await; }
    }).await.expect("bounded gate not reached");
}

struct Model { live: Arc<AtomicUsize> }
impl Drop for Model {
    fn drop(&mut self) { self.live.fetch_sub(1, Ordering::SeqCst); }
}

#[test]
fn aborting_reaper_must_collect_actual_drop_completion() {
    rt().block_on(async {
        let entered = Arc::new(AtomicBool::new(false));
        let completed = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let rx = std::sync::Mutex::new(rx);
        let begin = entered.clone(); let finish = completed.clone();
        let owner = Arc::new(AsrLifecycleOwner::new(Duration::ZERO,
            Arc::new(|| Ok(Arc::new(1u8))),
            Some(Arc::new(move |model| {
                begin.store(true, Ordering::SeqCst);
                rx.lock().unwrap().recv_timeout(Duration::from_secs(2)).unwrap();
                drop(model);
                finish.store(true, Ordering::SeqCst);
            }))));
        owner.request_wake().await.unwrap();
        let own = owner.clone();
        let reaper = tokio::spawn(async move { own.try_reap_idle().await });
        flag(&entered).await;
        reaper.abort(); let _ = reaper.await;
        tx.send(()).unwrap(); flag(&completed).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let state_after_drop = owner.current_state().await;
        let wake_timed_out = tokio::time::timeout(Duration::from_millis(200), owner.request_wake()).await.is_err();
        println!("actual_unloader_returned=true, state={state_after_drop:?}, next_wake_timed_out={wake_timed_out}");
        assert!(!wake_timed_out, "reaper abort strands actual completed unload");
    });
}

#[test]
fn acquire_check_and_reap_must_share_atomic_admission() {
    rt().block_on(async {
        let live = Arc::new(AtomicUsize::new(0));
        let count = live.clone();
        let owner = Arc::new(AsrLifecycleOwner::new(Duration::ZERO, Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(Arc::new(Model { live: count.clone() }))
        }), None));
        owner.request_wake().await.unwrap();
        // Hold exactly the existing instance mutex to park acquisition after its state check.
        let instance_gate = owner.instance.lock().await;
        let mut acquiring = Box::pin(owner.acquire_transcriber());
        {
            let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
            assert!(std::future::Future::poll(acquiring.as_mut(), &mut cx).is_pending());
        }
        let own = owner.clone();
        let reaper = tokio::spawn(async move { own.try_reap_idle().await });
        tokio::time::timeout(Duration::from_secs(2), async {
            while owner.current_state().await != AsrState::Unloading { tokio::task::yield_now().await; }
        }).await.unwrap();
        drop(instance_gate);
        let lease = tokio::time::timeout(Duration::from_secs(2), acquiring).await.unwrap().unwrap();
        let reaped = tokio::time::timeout(Duration::from_secs(2), reaper).await.unwrap().unwrap();
        let retired_state = owner.current_state().await;
        owner.request_wake().await.unwrap();
        let live_with_old_lease = live.load(Ordering::SeqCst);
        println!("reaped={reaped}, state_with_active_lease={retired_state:?}, live_after_reload={live_with_old_lease}");
        drop(lease); drop(owner);
        assert!(live_with_old_lease <= 1, "acquisition raced past reaper's active count check");
    });
}

struct NativeCallbackShape { inner: Arc<Model> }

#[test]
fn cancelled_transcription_must_retain_native_ownership_until_real_return() {
    rt().block_on(async {
        let live = Arc::new(AtomicUsize::new(0));
        let count = live.clone();
        let owner = Arc::new(AsrLifecycleOwner::new(Duration::ZERO, Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(Arc::new(NativeCallbackShape { inner: Arc::new(Model { live: count.clone() }) }))
        }), None));
        owner.request_wake().await.unwrap();
        let lease = owner.acquire_transcriber().await.unwrap();
        let entered = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let start = entered.clone(); let done = finished.clone();
        let waiter = tokio::spawn(async move {
            // Same ownership shape as SherpaSttCallback: blocking worker owns inner Arc.
            let inner = lease.inner.clone();
            tokio::task::spawn_blocking(move || {
                start.store(true, Ordering::SeqCst);
                rx.recv_timeout(Duration::from_secs(2)).unwrap();
                drop(inner);
                done.store(true, Ordering::SeqCst);
            }).await.unwrap();
            drop(lease);
        });
        flag(&entered).await;
        waiter.abort(); let _ = waiter.await;
        let reaped = owner.try_reap_idle().await;
        owner.request_wake().await.unwrap();
        let overlapping = live.load(Ordering::SeqCst);
        println!("blocking_still_running=true, reaped_after_waiter_abort={reaped}, live_instances={overlapping}");
        tx.send(()).unwrap(); flag(&finished).await;
        drop(owner);
        assert!(overlapping <= 1, "async lease ended before its blocking native work");
    });
}

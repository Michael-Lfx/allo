//! Exact replica of parent review round 3 quiescence-probe-body.rs.
//!
//! Validates:
//! native_ownership_must_outlive_any_fixed_quiescence_delay
//! (Native blocking tasks past 100ms cannot be prematurely evicted)

#![cfg(feature = "voice-skeleton")]

#[path = "../src/voice/asr_lifecycle.rs"]
mod candidate;
use candidate::*;
use std::sync::{Arc, atomic::{AtomicBool, AtomicUsize, Ordering}};
use std::time::{Duration, Instant};

struct CountedInner { live: Arc<AtomicUsize> }
impl Drop for CountedInner {
    fn drop(&mut self) { self.live.fetch_sub(1, Ordering::SeqCst); }
}
struct NativeShape { inner: Arc<CountedInner> }
async fn gate(flag: &AtomicBool) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !flag.load(Ordering::SeqCst) { tokio::task::yield_now().await; }
    }).await.expect("bounded blocking gate not reached");
}

#[test]
fn native_ownership_must_outlive_any_fixed_quiescence_delay() {
    tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap().block_on(async {
        let live = Arc::new(AtomicUsize::new(0));
        let count = live.clone();
        let owner = Arc::new(AsrLifecycleOwner::new(Duration::ZERO, Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(Arc::new(NativeShape { inner: Arc::new(CountedInner { live: count.clone() }) }))
        }), None));
        owner.request_wake().await.unwrap();
        let lease = owner.acquire_transcriber().await.unwrap();
        let entered = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let (release, rx) = std::sync::mpsc::channel::<()>();
        let start = entered.clone(); let done = finished.clone();
        let waiter = tokio::spawn(async move {
            let inner = lease.inner.clone();
            tokio::task::spawn_blocking(move || {
                start.store(true, Ordering::SeqCst);
                rx.recv_timeout(Duration::from_secs(2)).unwrap();
                drop(inner);
                done.store(true, Ordering::SeqCst);
            }).await.unwrap();
            drop(lease);
        });
        gate(&entered).await;
        waiter.abort(); let _ = waiter.await;
        // The real blocking operation remains behind the gate past the new 40ms heuristic.
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!finished.load(Ordering::SeqCst));
        let reaped = owner.try_reap_idle().await;
        owner.request_wake().await.unwrap();
        let peak = live.load(Ordering::SeqCst);
        println!("blocked_for_more_than_100ms=true, reaped={reaped}, live_inner_instances={peak}");
        release.send(()).unwrap(); gate(&finished).await;
        drop(owner);
        assert!(peak <= 1, "40ms grace does not track actual native completion");
    });
}

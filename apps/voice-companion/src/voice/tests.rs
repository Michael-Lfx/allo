    use super::*;
    use nomifun_voice_terminal::contract::AcousticState;
    use nomifun_voice_terminal::task_client::VoiceTaskClient;
    use tempfile::tempdir;

    #[derive(Default)]
    struct BlockingGateState {
        entered: bool,
        released: bool,
    }

    #[derive(Clone, Default)]
    struct BlockingGate {
        state: Arc<(std::sync::Mutex<BlockingGateState>, std::sync::Condvar)>,
        entries: Arc<AtomicU64>,
    }

    impl BlockingGate {
        fn block(&self) {
            let (lock, wake) = &*self.state;
            let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
            state.entered = true;
            self.entries.fetch_add(1, Ordering::AcqRel);
            wake.notify_all();
            while !state.released {
                state = wake.wait(state).unwrap_or_else(|error| error.into_inner());
            }
        }

        fn is_entered(&self) -> bool {
            self.state
                .0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .entered
        }

        fn rearm(&self) {
            let mut state = self
                .state
                .0
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state.entered = false;
            state.released = false;
        }

        fn release(&self) {
            let (lock, wake) = &*self.state;
            let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
            state.released = true;
            wake.notify_all();
        }
    }

    struct ReleaseBlockingGateOnDrop(BlockingGate);
    impl Drop for ReleaseBlockingGateOnDrop {
        fn drop(&mut self) {
            self.0.release();
        }
    }

    async fn wait_for_gate_entry(gate: &BlockingGate) {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !gate.is_entered() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("blocking native test double must be entered");
    }

    async fn wait_for_native_release(busy: &AtomicBool) {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while busy.load(Ordering::Acquire) {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("released native job must relinquish its lease");
    }

    struct BlockingKws {
        gate: BlockingGate,
        calls: u64,
    }

    impl KwsBackend for BlockingKws {
        fn process_pcm_chunk(&mut self, _chunk: &[f32]) -> Option<nomifun_audio::kws::KwsResult> {
            self.gate.block();
            self.calls += 1;
            Some(nomifun_audio::kws::KwsResult {
                keyword: format!("hit-{}", self.calls),
                score: 1.0,
                timestamp_ms: self.calls,
            })
        }

        fn reset(&mut self) {
            self.gate.block();
        }

        fn is_initialized(&self) -> bool {
            true
        }
    }

    struct DropBlockingKws {
        gate: BlockingGate,
    }

    impl KwsBackend for DropBlockingKws {
        fn process_pcm_chunk(&mut self, _chunk: &[f32]) -> Option<nomifun_audio::kws::KwsResult> {
            None
        }

        fn reset(&mut self) {}

        fn is_initialized(&self) -> bool {
            false
        }
    }

    impl Drop for DropBlockingKws {
        fn drop(&mut self) {
            self.gate.block();
        }
    }

    struct HealthyKws;
    impl KwsBackend for HealthyKws {
        fn process_pcm_chunk(&mut self, _chunk: &[f32]) -> Option<nomifun_audio::kws::KwsResult> {
            None
        }

        fn reset(&mut self) {}

        fn is_initialized(&self) -> bool {
            true
        }
    }

    struct BlockingVad {
        gate: BlockingGate,
        calls: u64,
        active: bool,
    }

    impl VadBackend for BlockingVad {
        fn process_frame(&mut self, _samples: &[f32]) -> bool {
            self.gate.block();
            self.calls += 1;
            self.active = self.calls % 2 == 1;
            self.active
        }

        fn reset(&mut self) {
            self.gate.block();
            self.active = false;
        }

        fn is_speech_active(&self) -> bool {
            self.active
        }
    }

    #[test]
    fn startup_guard_fails_closed_before_any_device_assembly() {
        use std::ffi::OsStr;
        let absent = parse_startup_markers(None, None, None);
        assert_eq!(startup_mode(false, absent), Ok(StartupMode::Default));
        for value in ["", "0", "false", "unknown"] {
            let marker = parse_startup_markers(Some(OsStr::new(value)), None, None);
            assert!(marker.headless_present && !marker.headless_valid);
            assert!(startup_mode(false, marker).is_err(), "default must reject {value:?}");
            assert!(startup_mode(true, marker).is_err(), "feature must require headless=1");
            let block = parse_startup_markers(None, Some(OsStr::new(value)), None);
            assert!(startup_mode(false, block).is_err(), "default must reject block marker {value:?}");
            let exit_code = parse_startup_markers(None, None, Some(OsStr::new(value)));
            assert!(startup_mode(false, exit_code).is_err(), "default must reject exit marker {value:?}");
        }
        let valid = parse_startup_markers(
            Some(OsStr::new("1")),
            Some(OsStr::new("1")),
            Some(OsStr::new("1")),
        );
        assert_eq!(startup_mode(true, valid), Ok(StartupMode::HeadlessTest));
        assert!(startup_mode(false, valid).is_err());
        let feature_no_marker = parse_startup_markers(None, None, None);
        assert!(startup_mode(true, feature_no_marker).is_err());
        let combined = parse_startup_markers(None, Some(OsStr::new("1")), Some(OsStr::new("1")));
        assert!(startup_mode(false, combined).is_err());
    }

    #[test]
    fn sequential_reservation_retires_old_generation_before_new_owner_can_claim() {
        for _ in 0..100_000 {
            let busy = Arc::new(AtomicBool::new(false));
            let epoch = Arc::new(AtomicU64::new(0));
            assert!(busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_ok());
            let a_job = epoch.fetch_add(1, Ordering::AcqRel) + 1;
            let a = NativeBusyGuard::attach(busy.clone(), epoch.clone(), a_job);
            assert!(busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err(), "B must not acquire A's lease");
            drop(a);
            assert!(busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_ok());
            let b_job = epoch.fetch_add(1, Ordering::AcqRel) + 1;
            assert_eq!(epoch.load(Ordering::Acquire), b_job, "old drop must not retire B generation");
            let b = NativeBusyGuard::attach(busy.clone(), epoch.clone(), b_job);
            assert_eq!(epoch.load(Ordering::Acquire), b_job);
            drop(b);
        }
    }

    #[test]
    fn native_recovery_requires_the_same_supervised_active_job() {
        assert!(native_job_needs_recovery(true, true, 7, 7));
        assert!(!native_job_needs_recovery(false, true, 7, 7));
        assert!(!native_job_needs_recovery(true, false, 7, 7));
        assert!(!native_job_needs_recovery(true, true, 9, 7));
        assert!(native_error_requires_degraded("engine_check_timeout"));
        assert!(native_error_requires_degraded("native_call_join_failed:panic"));
        assert!(native_error_requires_degraded("worker_asr_join_failed:panic"));
        assert!(!native_error_requires_degraded("engine_check_cancelled"));
        assert!(!native_error_requires_degraded("engine_check_native_busy"));
    }

    #[test]
    fn native_health_rejects_stale_jobs_and_refines_the_current_job() {
        let health = NativeHealth::default();
        health.publish(8, false);
        health.publish(7, true);
        assert!(!health.is_degraded(), "old failure must not degrade healthy B");

        health.publish(9, true);
        health.publish(8, false);
        assert!(health.is_degraded(), "old recovery must not clear degraded B");

        health.publish(10, false);
        health.publish_result(10, &Err::<(), _>("loader_failed"));
        assert!(health.is_degraded(), "inner loader errors refine the same job");
        health.publish(9, false);
        assert!(health.is_degraded());
    }

    #[tokio::test]
    async fn native_load_timeout_retains_ownership_and_fences_late_result() {
        let gate = BlockingGate::default();
        let _cleanup = ReleaseBlockingGateOnDrop(gate.clone());
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(0));
        let health = Arc::new(NativeHealth::default());
        let runner = NativeCallRunner::with_health(busy.clone(), epoch.clone(), health.clone());
        let cancel = Arc::new(AtomicBool::new(false));
        let notify = Arc::new(Notify::new());
        let first = {
            let runner = runner.clone();
            let gate = gate.clone();
            tokio::spawn(async move {
                runner
                    .load(std::time::Duration::from_millis(500), move || {
                        gate.block();
                        Ok::<_, String>("late-a")
                    })
                    .await
            })
        };
        wait_for_gate_entry(&gate).await;
        let first_job = epoch.load(Ordering::Acquire);
        let first_result = tokio::time::timeout(std::time::Duration::from_secs(2), first)
            .await
            .expect("first native load must have a bounded caller")
            .unwrap();
        assert_eq!(first_result.unwrap_err(), "engine_check_timeout");
        assert!(busy.load(Ordering::Acquire));
        assert!(health.is_degraded());
        assert!(native_job_needs_recovery(true, true, epoch.load(Ordering::Acquire), first_job));
        assert!(!native_job_needs_recovery(false, true, epoch.load(Ordering::Acquire), first_job));

        let rejected = runner
            .load_until(
                std::time::Instant::now() + std::time::Duration::from_millis(20),
                cancel.as_ref(),
                notify.as_ref(),
                false,
                || Ok::<_, String>("must-not-run"),
            )
            .await;
        assert_eq!(rejected.unwrap_err(), "engine_check_native_busy");
        assert_eq!(gate.entries.load(Ordering::Acquire), 1);

        gate.release();
        wait_for_native_release(&busy).await;
        assert!(health.is_degraded(), "late A must not publish recovery");
        gate.rearm();
        let second = {
            let runner = runner.clone();
            let gate = gate.clone();
            tokio::spawn(async move {
                runner
                    .load(std::time::Duration::from_secs(1), move || {
                        gate.block();
                        Ok::<_, String>("healthy-b")
                    })
                    .await
            })
        };
        wait_for_gate_entry(&gate).await;
        let second_job = epoch.load(Ordering::Acquire);
        assert_ne!(second_job, first_job);
        assert!(busy.load(Ordering::Acquire));
        assert!(!native_job_needs_recovery(true, true, second_job, first_job));
        gate.release();
        let second_result = tokio::time::timeout(std::time::Duration::from_secs(2), second)
            .await
            .expect("healthy successor must finish")
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(second_result, "healthy-b");
        assert!(!health.is_degraded());
        assert_eq!(gate.entries.load(Ordering::Acquire), 2);
        wait_for_native_release(&busy).await;
    }

    #[tokio::test]
    async fn native_detached_caller_cannot_release_running_job() {
        let gate = BlockingGate::default();
        let _cleanup = ReleaseBlockingGateOnDrop(gate.clone());
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(0));
        let runner = NativeCallRunner::new(busy.clone(), epoch.clone());
        let first = {
            let runner = runner.clone();
            let gate = gate.clone();
            tokio::spawn(async move {
                let cancel = AtomicBool::new(false);
                let notify = Notify::new();
                runner
                    .run(
                        std::time::Instant::now() + std::time::Duration::from_secs(30),
                        &cancel,
                        &notify,
                        true,
                        true,
                        #[cfg(feature = "native-fault-injection")]
                        None,
                        move || gate.block(),
                    )
                    .await
            })
        };
        wait_for_gate_entry(&gate).await;
        first.abort();
        let _ = first.await;
        assert!(busy.load(Ordering::Acquire));
        gate.release();
        wait_for_native_release(&busy).await;
        runner
            .run_uncancelable(std::time::Duration::from_secs(1), false, || "healthy-b")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn native_kws_frame_timeout_does_not_deliver_late_wake_to_successor() {
        let gate = BlockingGate::default();
        let _cleanup = ReleaseBlockingGateOnDrop(gate.clone());
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(0));
        let kws = Arc::new(NativeKws::new(
            Box::new(BlockingKws { gate: gate.clone(), calls: 0 }),
            NativeCallRunner::new(busy.clone(), epoch.clone()),
        ));
        let cancel = Arc::new(AtomicBool::new(false));
        let notify = Arc::new(Notify::new());
        let first = {
            let kws = kws.clone();
            let cancel = cancel.clone();
            let notify = notify.clone();
            tokio::spawn(async move {
                kws.process_pcm_chunk_until(
                    &[0.1; 160],
                    std::time::Instant::now() + std::time::Duration::from_millis(500),
                    cancel.as_ref(),
                    notify.as_ref(),
                    true,
                )
                .await
            })
        };
        wait_for_gate_entry(&gate).await;
        let first_job = epoch.load(Ordering::Acquire);
        let first_result = tokio::time::timeout(std::time::Duration::from_secs(1), first)
            .await
            .expect("KWS caller must be bounded")
            .unwrap();
        assert_eq!(first_result.unwrap_err(), "engine_check_timeout");
        assert!(busy.load(Ordering::Acquire));

        gate.release();
        wait_for_native_release(&busy).await;
        gate.rearm();
        let second = {
            let kws = kws.clone();
            tokio::spawn(async move { kws.process_pcm_chunk(&[0.1; 160]).await })
        };
        wait_for_gate_entry(&gate).await;
        assert_ne!(epoch.load(Ordering::Acquire), first_job);
        assert!(!native_job_needs_recovery(
            true,
            true,
            epoch.load(Ordering::Acquire),
            first_job,
        ));
        gate.release();
        let second_hit = tokio::time::timeout(std::time::Duration::from_secs(1), second)
            .await
            .expect("healthy KWS successor must finish")
            .unwrap()
            .unwrap()
            .expect("healthy KWS successor returns its own hit");
        assert_eq!(second_hit.keyword, "hit-2");
        assert_eq!(gate.entries.load(Ordering::Acquire), 2);
        wait_for_native_release(&busy).await;
    }

    #[tokio::test]
    async fn native_vad_frame_timeout_does_not_publish_late_active_state() {
        let gate = BlockingGate::default();
        let _cleanup = ReleaseBlockingGateOnDrop(gate.clone());
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(0));
        let vad = Arc::new(NativeVad::new(
            Box::new(BlockingVad { gate: gate.clone(), calls: 0, active: false }),
            NativeCallRunner::new(busy.clone(), epoch.clone()),
        ));
        let first = {
            let vad = vad.clone();
            let cancel = Arc::new(AtomicBool::new(false));
            let notify = Arc::new(Notify::new());
            tokio::spawn(async move {
                vad.process_frame_until(
                    &[0.1; 160],
                    std::time::Instant::now() + std::time::Duration::from_millis(500),
                    cancel.as_ref(),
                    notify.as_ref(),
                    true,
                )
                .await
            })
        };
        wait_for_gate_entry(&gate).await;
        let first_result = tokio::time::timeout(std::time::Duration::from_secs(1), first)
            .await
            .expect("VAD caller must be bounded")
            .unwrap();
        assert_eq!(first_result.unwrap_err(), "engine_check_timeout");
        assert!(busy.load(Ordering::Acquire));
        assert!(!vad.is_speech_active());

        gate.release();
        wait_for_native_release(&busy).await;
        gate.rearm();
        let second = {
            let vad = vad.clone();
            tokio::spawn(async move { vad.process_frame(&[0.1; 160]).await })
        };
        wait_for_gate_entry(&gate).await;
        gate.release();
        let second_active = tokio::time::timeout(std::time::Duration::from_secs(1), second)
            .await
            .expect("healthy VAD successor must finish")
            .unwrap()
            .unwrap();
        assert!(!second_active, "successor returns its own second-call result, not late A");
        assert!(!vad.is_speech_active());
        assert_eq!(gate.entries.load(Ordering::Acquire), 2);
        wait_for_native_release(&busy).await;
    }

    #[tokio::test]
    async fn native_kws_and_vad_reset_keep_ownership_until_actual_return() {
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(0));
        let runner = NativeCallRunner::new(busy.clone(), epoch.clone());

        let kws_gate = BlockingGate::default();
        let _kws_cleanup = ReleaseBlockingGateOnDrop(kws_gate.clone());
        let kws = Arc::new(NativeKws::new(
            Box::new(BlockingKws { gate: kws_gate.clone(), calls: 0 }),
            runner.clone(),
        ));
        let kws_reset = {
            let kws = kws.clone();
            tokio::spawn(async move {
                let cancel = AtomicBool::new(false);
                let notify = Notify::new();
                kws.reset_until(
                    std::time::Instant::now() + std::time::Duration::from_millis(500),
                    &cancel,
                    &notify,
                    true,
                )
                .await
            })
        };
        wait_for_gate_entry(&kws_gate).await;
        let timed = tokio::time::timeout(std::time::Duration::from_secs(1), kws_reset)
            .await
            .expect("reset deadline must bound the caller")
            .unwrap();
        assert_eq!(timed.unwrap_err(), "engine_check_timeout");
        assert!(busy.load(Ordering::Acquire));
        kws_gate.release();
        wait_for_native_release(&busy).await;
        kws.reset().await.unwrap();

        let vad_gate = BlockingGate::default();
        let _vad_cleanup = ReleaseBlockingGateOnDrop(vad_gate.clone());
        let vad = Arc::new(NativeVad::new(
            Box::new(BlockingVad { gate: vad_gate.clone(), calls: 0, active: true }),
            runner,
        ));
        vad.active.store(true, Ordering::Release);
        let vad_reset = {
            let vad = vad.clone();
            tokio::spawn(async move {
                let cancel = AtomicBool::new(false);
                let notify = Notify::new();
                vad.reset_until(
                    std::time::Instant::now() + std::time::Duration::from_millis(500),
                    &cancel,
                    &notify,
                    true,
                )
                .await
            })
        };
        wait_for_gate_entry(&vad_gate).await;
        let timed = tokio::time::timeout(std::time::Duration::from_secs(1), vad_reset)
            .await
            .expect("reset deadline must bound the caller")
            .unwrap();
        assert_eq!(timed.unwrap_err(), "engine_check_timeout");
        assert!(busy.load(Ordering::Acquire));
        assert!(vad.is_speech_active(), "a late reset must not be published as completed");
        vad_gate.release();
        wait_for_native_release(&busy).await;
        vad.reset().await.unwrap();
        assert!(!vad.is_speech_active());
    }

    #[tokio::test]
    async fn native_kws_reload_installs_a_healthy_successor() {
        let gate = BlockingGate::default();
        gate.release();
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(0));
        let kws = NativeKws::new(
            Box::new(BlockingKws { gate, calls: 0 }),
            NativeCallRunner::new(busy, epoch),
        );
        kws.replace(Box::new(HealthyKws)).await.unwrap();
        assert!(kws.is_initialized());
        assert!(kws.process_pcm_chunk(&[0.1; 160]).await.unwrap().is_none());
    }
    #[tokio::test]
    async fn native_kws_reload_timeout_keeps_ownership_through_old_engine_drop() {
        let gate = BlockingGate::default();
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(0));
        let kws = Arc::new(NativeKws::new(
            Box::new(DropBlockingKws { gate: gate.clone() }),
            NativeCallRunner::new(busy.clone(), epoch),
        ));
        let _cleanup = ReleaseBlockingGateOnDrop(gate.clone());
        assert!(!kws.is_initialized());
        let reload = {
            let kws = kws.clone();
            tokio::spawn(async move {
                kws.replace_until(
                    Box::new(HealthyKws),
                    std::time::Instant::now() + std::time::Duration::from_millis(500),
                )
                .await
            })
        };
        wait_for_gate_entry(&gate).await;
        let timed = tokio::time::timeout(std::time::Duration::from_secs(1), reload)
            .await
            .expect("reload caller must be bounded by the old engine destructor")
            .unwrap();
        assert_eq!(timed.unwrap_err(), "engine_check_timeout");
        assert!(busy.load(Ordering::Acquire));
        assert!(kws.is_initialized(), "the installed engine and cache must agree");

        gate.release();
        wait_for_native_release(&busy).await;
        assert!(kws.process_pcm_chunk(&[0.1; 160]).await.unwrap().is_none());
    }

    #[cfg(feature = "native-fault-injection")]
    #[test]
    fn concurrent_completion_never_clears_new_owner_observation() {
        use std::sync::Barrier;
        const ITERATIONS: usize = 100_000;
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(0));
        let start = Arc::new(Barrier::new(2));
        let done = Arc::new(Barrier::new(2));
        std::thread::scope(|scope| {
            let a_busy = busy.clone();
            let a_epoch = epoch.clone();
            let a_start = start.clone();
            let a_done = done.clone();
            let a = scope.spawn(move || {
                for _ in 0..ITERATIONS {
                    let job = a_epoch.fetch_add(1, Ordering::AcqRel) + 1;
                    let owner = NativeBusyGuard::attach(a_busy.clone(), a_epoch.clone(), job);
                    a_start.wait();
                    drop(owner);
                    a_done.wait();
                }
            });
            let b_busy = busy.clone();
            let b_epoch = epoch.clone();
            let b_start = start.clone();
            let b_done = done.clone();
            let b = scope.spawn(move || {
                for _ in 0..ITERATIONS {
                    b_start.wait();
                    while b_busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
                        std::hint::spin_loop();
                    }
                    let job = b_epoch.fetch_add(1, Ordering::AcqRel) + 1;
                    let owner = NativeBusyGuard::attach(b_busy.clone(), b_epoch.clone(), job);
                    assert_eq!(FAULT_NATIVE_BUSY.load(Ordering::Acquire), true, "old owner cleared B observation");
                    assert_eq!(FAULT_CURRENT_EPOCH.load(Ordering::Acquire), job);
                    drop(owner);
                    b_done.wait();
                }
            });
            a.join().unwrap();
            b.join().unwrap();
        });
    }

    #[cfg(feature = "native-fault-injection")]
    #[test]
    fn fault_observation_tracks_current_lease_not_stale_history() {
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(0));
        bind_fault_boundary("lease-a", 1, "instance-a");
        let first = NativeBusyGuard::new(busy.clone(), epoch.clone());
        record_fault_job_entry(1, None);
        let active = fault_job_observation();
        assert_eq!(active["native_busy"], serde_json::json!(true));
        assert_eq!(active["job"]["request_id"], serde_json::json!("lease-a"));
        assert_eq!(active["job"]["still_blocking"], serde_json::json!(true));
        drop(first);
        let completed = fault_job_observation();
        assert_eq!(completed["native_busy"], serde_json::json!(false));
        assert_eq!(completed["job"]["still_blocking"], serde_json::json!(false), "completed history must not remain active");
        let second = NativeBusyGuard::new(busy.clone(), epoch.clone());
        let stale_a_new_b = fault_job_observation();
        assert_eq!(stale_a_new_b["native_busy"], serde_json::json!(true));
        assert_eq!(stale_a_new_b["job"]["request_id"], serde_json::json!("lease-a"));
        assert_eq!(stale_a_new_b["job"]["still_blocking"], serde_json::json!(false), "A history must not claim B ownership");
        drop(second);
    }

    #[tokio::test]
    async fn test_voice_runtime_assembly() {
        let dir = tempdir().unwrap();
        let model_manager = Arc::new(ModelManager::new(dir.path()));
        let oobe_manager = Arc::new(OobeManager::new_bypassed());

        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, mut playback_rx) = VoiceCompanionRuntime::from_capture(
            model_manager.clone(),
            oobe_manager.clone(),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );

        // Verify driver state initialized to Idle
        {
            let d = runtime.driver.lock().await;
            assert_eq!(d.current_state(), AcousticState::Idle);
        }

        // Verify audio pipeline playback sends frames into rx
        runtime
            .playback()
            .play_earcon(EarconKind::Wake)
            .await
            .unwrap();
        let frame = playback_rx.recv().await;
        assert!(frame.is_some());
        assert!(!frame.unwrap().is_empty());

        // Verify worker task spawns and pumps cleanly
        let handle = runtime.start_worker();
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        assert!(!handle.is_finished());

        handle.abort();
    }

    struct FailingStt;
    #[async_trait::async_trait]
    impl SttCallback for FailingStt {
        async fn transcribe(
            &self,
            _channel: AudioChannel,
            _pcm: Vec<f32>,
            _sample_rate: u32,
        ) -> Option<String> {
            None
        }
    }

    fn fixture_pcm(name: &str) -> Vec<f32> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/architecture/voice-agent/execution/audio-fixtures").join(name);
        let bytes = std::fs::read(path).unwrap();
        let (pcm, rate) = nomifun_audio::parse_audio_frame_to_pcm(&bytes);
        assert_eq!(rate, 16_000, "fixture must be 16 kHz");
        pcm
    }

    #[tokio::test]
    #[ignore = "requires the real D:/tmp/flowy-dev/models suite and repository audio fixtures"]
    async fn actual_worker_engine_failure_is_not_masked_by_a_fresh_reload() {
        let models_dir = std::path::PathBuf::from("D:/tmp/flowy-dev/models");
        assert!(models_dir.join("asr/model.int8.onnx").is_file());
        assert!(models_dir.join("kws/encoder.int8.onnx").is_file());
        let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(&models_dir)),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(FailingStt),
            Arc::new(QwenVoiceRouter::new()),
        );
        let runtime = Arc::new(runtime);
        let worker = runtime.start_worker();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert!(runtime.worker_epoch.load(Ordering::Acquire) > 0);

        let fixtures = SelfCheckAudio {
            wake: fixture_pcm("wake-jenny-16k.wav"),
            command: fixture_pcm("command-16k.wav"),
            non_wake: fixture_pcm("non-wake-16k.wav"),
            source: "test_fixture".into(),
            online: false,
            generated_at_ms: None,
        };
        let (reply_tx, reply_rx) = oneshot::channel();
        runtime
            .engine_check_tx
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .unwrap()
            .send(WorkerEngineCheck {
                request_id: "actual-worker-failure".into(),
                deadline: std::time::Instant::now() + std::time::Duration::from_secs(20),
                cancel: Arc::new(AtomicBool::new(false)),
                cancel_notify: Arc::new(Notify::new()),
                native_busy: runtime.engine_check_native_busy.clone(),
                native_job_epoch: runtime.native_job_epoch.clone(),
                fixtures,
                reply: reply_tx,
            })
            .await
            .unwrap();
        let result = reply_rx.await.unwrap().expect("worker ran the check");
        assert!(result.kws_ok, "the actual worker KWS must be exercised");
        assert!(result.vad_ok, "the actual worker VAD must be exercised");
        assert!(!result.asr_ok, "the failing actual worker transcriber must not be replaced");
        let reload = tokio::task::spawn_blocking(move || {
            nomifun_audio::stt::try_load_sherpa(Some(&models_dir))
        })
        .await
        .unwrap()
        .unwrap();
        assert!(reload.is_some(), "a transient reload would have succeeded");
        worker.abort();
        let _ = worker.await;
        producer.close();
    }

    #[tokio::test]
    async fn voice_worker_waits_for_verified_assets_before_native_initialization() {
        let dir = tempdir().unwrap();
        let models = Arc::new(ModelManager::new(dir.path()));
        let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (mut runtime, _) = VoiceCompanionRuntime::from_capture(
            models, Arc::new(OobeManager::new_bypassed()), Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback), Arc::new(QwenVoiceRouter::new()),
        );
        runtime.replay_capture = false;
        let worker = runtime.start_worker();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let waiting = !worker.is_finished();
        let ready = runtime.snapshot().acoustic_ready;
        worker.abort(); let _ = worker.await; producer.close();
        assert!(waiting, "missing assets must wait for provisioning, not open native engines and exit");
        assert!(!ready);
    }

    struct MockSpeech(Arc<std::sync::atomic::AtomicUsize>);
    #[async_trait::async_trait]
    impl nomifun_voice_terminal::tts::TtsProvider for MockSpeech {
        async fn synthesize_stream(
            &self,
            _: &str,
            _: &str,
        ) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, nomifun_voice_terminal::tts::TtsError> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            tx.try_send(nomifun_audio::pcm_to_wav(&vec![0.05; 2400], 24000)).unwrap();
            Ok(rx)
        }
    }

    struct CountingStt(Arc<AtomicUsize>);
    #[async_trait::async_trait]
    impl SttCallback for CountingStt {
        async fn transcribe(&self, _: AudioChannel, _: Vec<f32>, _: u32) -> Option<String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Some("你好".into())
        }
    }

    #[tokio::test]
    async fn worker_transcribes_two_turns_after_transient_native_contention() {
        use nomifun_voice_terminal::driver::StubVoiceRouter;
        let root = tempdir().unwrap();
        let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let (mut runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())), Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)), Arc::new(CountingStt(calls.clone())),
            Arc::new(StubVoiceRouter::chat("你好呀")),
        );
        let gate = Arc::new(SpeechEndTestGate::default());
        runtime.ordinary_speech_end_gate = Some(gate.clone());
        runtime.update_feedback(serde_json::from_value(serde_json::json!({"mode":"off","volume":0,"speak_replies":false})).unwrap()).unwrap();
        let worker = runtime.start_worker();
        runtime.handle_wake().await.unwrap();
        let session = runtime.snapshot().session_id;
        for index in 0..75 {
            assert_eq!(producer.push(&vec![if index < 25 { 0.1 } else { 0.0 }; 320]), 320);
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        tokio::time::timeout(std::time::Duration::from_secs(3), gate.reached.notified()).await.unwrap();
        let runner = NativeCallRunner::with_admission(runtime.engine_check_native_busy.clone(), runtime.native_job_epoch.clone(), runtime.native_degraded.clone(), runtime.native_admission.clone(), false);
        let owner = runner.acquire(std::time::Instant::now() + std::time::Duration::from_secs(1), &AtomicBool::new(false), &Notify::new(), false).await.unwrap();
        gate.resume.notify_one();
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        drop(owner);
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while runtime.snapshot().assistant_text.as_deref() != Some("你好呀") { tokio::time::sleep(std::time::Duration::from_millis(10)).await; }
        }).await.unwrap();
        let first = runtime.snapshot();
        assert_eq!(first.user_text.as_deref(), Some("你好"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        for index in 0..75 {
            assert_eq!(producer.push(&vec![if index < 25 { 0.1 } else { 0.0 }; 320]), 320);
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while runtime.snapshot().conversation.iter().filter(|turn| turn.user_text.as_deref() == Some("你好")).count() < 2 {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.unwrap();
        let second = runtime.snapshot();
        worker.abort();
        producer.close();
        assert_eq!(second.session_id, session);
        assert_ne!(second.turn_id, first.turn_id);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(second.conversation.iter().filter(|turn| turn.user_text.as_deref() == Some("你好")).count() >= 2);
    }

    #[tokio::test]
    async fn confirmation_speech_waits_for_a_transient_native_owner() {
        use nomifun_api_types::voice::{VoiceSessionControlRequest, VoiceSessionPhase, VoiceTaskReference, VoiceToolConfirmationPrompt};
        use nomifun_voice_terminal::{driver::StubVoiceRouter, playback::TestAudioSink};

        for (answer, approved) in [("确认", true), ("取消", false)] {
        let root = tempdir().unwrap();
        let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (mut runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::FakeSttCallback::always(answer)),
            Arc::new(StubVoiceRouter::chat("unused")),
        );
        let gate = Arc::new(SpeechEndTestGate::default());
        runtime.confirmation_speech_end_gate = Some(gate.clone());
        let runtime = Arc::new(runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(MockSpeech(Arc::new(AtomicUsize::new(0)))),
        ));
        let worker = runtime.start_worker();
        let snapshot = runtime.snapshot();
        let prompt = VoiceToolConfirmationPrompt {
            scope: VoiceSessionControlRequest { instance_id: snapshot.instance_id, session_id: snapshot.session_id, turn_id: snapshot.turn_id },
            task: VoiceTaskReference { command_id: "command".into(), conversation_id: Some("conversation".into()), message_id: Some("message".into()) },
            approval_id: "approval".into(), msg_id: "tool-message".into(), call_id: "call".into(),
            expires_at_ms: chrono::Utc::now().timestamp_millis() + 8000,
            text: "请确认。".into(),
        };
        let request = { let runtime = runtime.clone(); tokio::spawn(async move { runtime.request_tool_confirmation(prompt).await }) };
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while runtime.snapshot().phase != VoiceSessionPhase::ListeningConfirm {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }).await.expect("confirmation must start listening");
        for frame in 0..75 {
            let pcm: Vec<f32> = (0..320).map(|sample| if frame < 25 {
                0.03 * (sample as f32 * std::f32::consts::TAU * 200.0 / 16000.0).sin()
            } else { 0.0 }).collect();
            assert_eq!(producer.push(&pcm), pcm.len());
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        tokio::time::timeout(std::time::Duration::from_secs(3), gate.reached.notified()).await.expect("worker must reach confirmation speech end");
        let runner = NativeCallRunner::with_admission(
            runtime.engine_check_native_busy.clone(), runtime.native_job_epoch.clone(),
            runtime.native_degraded.clone(), runtime.native_admission.clone(), false,
        );
        let owner = runner.acquire(
            std::time::Instant::now() + std::time::Duration::from_secs(1),
            &AtomicBool::new(false), &Notify::new(), true,
        ).await.unwrap();
        gate.resume.notify_one();
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        drop(owner);
        let reply = tokio::time::timeout(std::time::Duration::from_secs(8), request).await.unwrap().unwrap().unwrap();
        let snapshot = runtime.snapshot();
        worker.abort();
        assert_eq!(reply.approved, approved, "the spoken confirmation decision must survive transient contention");
        assert_eq!(snapshot.user_text.as_deref(), Some(answer));
        }
    }

    #[tokio::test]
    async fn empty_confirmation_is_denied_with_a_visible_recognition_error() {
        use nomifun_api_types::voice::{VoiceSessionControlRequest, VoiceSessionPhase, VoiceTaskReference, VoiceToolConfirmationPrompt};
        use nomifun_voice_terminal::{driver::StubVoiceRouter, playback::TestAudioSink};
        let root = tempdir().unwrap();
        let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())), Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)), Arc::new(nomifun_audio::stt::FakeSttCallback::always("")),
            Arc::new(StubVoiceRouter::chat("unused")),
        );
        let runtime = Arc::new(runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(MockSpeech(Arc::new(AtomicUsize::new(0)))),
        ));
        let worker = runtime.start_worker();
        let snapshot = runtime.snapshot();
        let prompt = VoiceToolConfirmationPrompt {
            scope: VoiceSessionControlRequest { instance_id: snapshot.instance_id, session_id: snapshot.session_id, turn_id: snapshot.turn_id },
            task: VoiceTaskReference { command_id: "command".into(), conversation_id: Some("conversation".into()), message_id: Some("message".into()) },
            approval_id: "approval".into(), msg_id: "tool-message".into(), call_id: "call".into(),
            expires_at_ms: chrono::Utc::now().timestamp_millis() + 4000, text: "请确认。".into(),
        };
        let request = { let runtime = runtime.clone(); tokio::spawn(async move { runtime.request_tool_confirmation(prompt).await }) };
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while runtime.snapshot().phase != VoiceSessionPhase::ListeningConfirm {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }).await.unwrap();
        for index in 0..75 {
            assert_eq!(producer.push(&vec![if index < 25 { 0.1 } else { 0.0 }; 320]), 320);
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let reply = tokio::time::timeout(std::time::Duration::from_secs(5), request).await.unwrap().unwrap().unwrap();
        let snapshot = runtime.snapshot();
        worker.abort();
        assert!(!reply.approved);
        assert_eq!(snapshot.error.as_ref().map(|error| error.code.as_str()), Some("asr_empty_result"));
    }

    #[tokio::test]
    async fn stopping_at_confirmation_speech_end_cannot_authorize_a_new_session() {
        use nomifun_api_types::voice::{VoiceSessionControlRequest, VoiceSessionPhase, VoiceTaskReference, VoiceToolConfirmationPrompt};
        use nomifun_voice_terminal::{driver::StubVoiceRouter, playback::TestAudioSink};
        let root = tempdir().unwrap();
        let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (mut runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())), Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)), Arc::new(nomifun_audio::stt::FakeSttCallback::always("确认")),
            Arc::new(StubVoiceRouter::chat("unused")),
        );
        let gate = Arc::new(SpeechEndTestGate::default());
        runtime.confirmation_speech_end_gate = Some(gate.clone());
        let runtime = Arc::new(runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(MockSpeech(Arc::new(AtomicUsize::new(0)))),
        ));
        let worker = runtime.start_worker();
        runtime.handle_wake().await.unwrap();
        let snapshot = runtime.snapshot();
        let prompt = VoiceToolConfirmationPrompt {
            scope: VoiceSessionControlRequest { instance_id: snapshot.instance_id, session_id: snapshot.session_id, turn_id: snapshot.turn_id },
            task: VoiceTaskReference { command_id: "command".into(), conversation_id: Some("conversation".into()), message_id: Some("message".into()) },
            approval_id: "approval".into(), msg_id: "tool-message".into(), call_id: "call".into(),
            expires_at_ms: chrono::Utc::now().timestamp_millis() + 8000, text: "请确认。".into(),
        };
        let request = { let runtime = runtime.clone(); tokio::spawn(async move { runtime.request_tool_confirmation(prompt).await }) };
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while runtime.snapshot().phase != VoiceSessionPhase::ListeningConfirm { tokio::time::sleep(std::time::Duration::from_millis(5)).await; }
        }).await.unwrap();
        for index in 0..75 {
            assert_eq!(producer.push(&vec![if index < 25 { 0.1 } else { 0.0 }; 320]), 320);
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        tokio::time::timeout(std::time::Duration::from_secs(2), gate.reached.notified()).await.unwrap();
        let old = runtime.snapshot();
        runtime.stop_session(&VoiceSessionControlRequest { instance_id: old.instance_id, session_id: old.session_id.clone(), turn_id: old.turn_id }).await.unwrap();
        let reply = tokio::time::timeout(std::time::Duration::from_secs(2), request).await.unwrap().unwrap().unwrap();
        assert!(!reply.approved);
        runtime.handle_wake().await.unwrap();
        let new = runtime.snapshot();
        assert_ne!(new.session_id, old.session_id);
        gate.resume.notify_one();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let after = runtime.snapshot();
        worker.abort();
        producer.close();
        assert_eq!(after.session_id, new.session_id);
        assert!(after.error.is_none());
        assert!(after.user_text.is_none());
    }

    #[tokio::test]
    async fn stale_muted_playback_completion_cannot_advance_a_new_turn() {
        let root = tempdir().unwrap();
        let (capture, _producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::FakeSttCallback::always("问题")),
            Arc::new(nomifun_voice_terminal::driver::StubVoiceRouter::chat("回答")),
        );
        runtime.update_feedback(serde_json::from_value(serde_json::json!({"mode":"off","volume":0,"speak_replies":false})).unwrap()).unwrap();
        runtime.handle_wake().await.unwrap();
        assert_eq!(runtime.driver.lock().await.handle_event(AcousticEvent::SpeechEnd), Ok(AcousticState::Deciding));
        let _ = runtime.event_tx.send(VoiceCompanionEvent::Transcribing);
        let old = runtime.snapshot();
        let old_scope = VoiceSessionControlRequest {
            instance_id: old.instance_id,
            session_id: old.session_id,
            turn_id: old.turn_id,
        };
        let _ = runtime.event_tx.send(VoiceCompanionEvent::Transcribing);
        let new = runtime.snapshot();
        assert_ne!(new.turn_id, old_scope.turn_id);
        let generation = runtime.playback.generation_id();

        VoiceCompanionRuntime::finish_muted_playback(
            &runtime.driver,
            &runtime.playback,
            &runtime.event_tx,
            generation,
            &old_scope,
        ).await;

        let after = runtime.snapshot();
        assert_eq!(after.turn_id, new.turn_id);
        assert_eq!(after.phase, nomifun_api_types::voice::VoiceSessionPhase::Transcribing);
        assert_eq!(runtime.driver.lock().await.current_state(), AcousticState::Deciding);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stale_task_acceptance_playback_cannot_advance_a_new_turn() {
        use nomifun_api_types::voice::{VoiceTaskDelivery, VoiceTaskSnapshot, VoiceTaskStatus, VoiceTaskSubmitRequest};
        use nomifun_voice_terminal::{driver::StubVoiceRouter, playback::TestAudioSink, task_client::VoiceTaskClient};

        struct Gate {
            reached: Notify,
            resume: Notify,
        }
        struct GatedSpeech(Arc<Gate>);
        #[async_trait::async_trait]
        impl nomifun_voice_terminal::tts::TtsProvider for GatedSpeech {
            async fn synthesize_stream(
                &self,
                _: &str,
                _: &str,
            ) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, nomifun_voice_terminal::tts::TtsError> {
                self.0.reached.notify_one();
                self.0.resume.notified().await;
                let (tx, rx) = tokio::sync::mpsc::channel(1);
                tx.try_send(nomifun_audio::pcm_to_wav(&vec![0.05; 2400], 24000)).unwrap();
                Ok(rx)
            }
        }
        struct TaskClient;
        #[async_trait::async_trait]
        impl VoiceTaskClient for TaskClient {
            async fn current(&self) -> Result<Option<VoiceTaskSnapshot>, String> { Ok(None) }
            async fn control(&self, _: nomifun_api_types::voice::VoiceTaskControlCommand) -> Result<VoiceTaskSnapshot, String> {
                Err("unused".into())
            }
            async fn submit(&self, request: VoiceTaskSubmitRequest) -> Result<VoiceTaskDelivery, String> {
                Ok(VoiceTaskDelivery {
                    freshly_accepted: true,
                    task: VoiceTaskSnapshot {
                        command_id: request.command.command_id,
                        session_id: request.command.session_id,
                        acoustic_turn_id: request.command.turn_id,
                        conversation_id: Some("conversation".into()),
                        message_id: Some("message".into()),
                        runtime_turn_id: Some("runtime-turn".into()),
                        status: VoiceTaskStatus::Accepted,
                        result_ok: None,
                        result_text: None,
                        error_code: None,
                    },
                })
            }
        }

        let root = tempdir().unwrap();
        let (capture, _producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::FakeSttCallback::always("读取工作区文件")),
            Arc::new(StubVoiceRouter::new(serde_json::from_value(serde_json::json!({
                "intent_type":"agent_task", "task_goal":"读取工作区文件", "requires_confirmation":false
            })).unwrap())),
        );
        let gate = Arc::new(Gate { reached: Notify::new(), resume: Notify::new() });
        let runtime = runtime
            .with_tasks(Arc::new(TaskClient))
            .with_test_playback(
                Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
                Arc::new(GatedSpeech(gate.clone())),
            );
        runtime.update_feedback(serde_json::from_value(serde_json::json!({"mode":"off","volume":0,"speak_replies":true})).unwrap()).unwrap();
        runtime.handle_wake().await.unwrap();
        assert_eq!(runtime.driver.lock().await.handle_event(AcousticEvent::SpeechEnd), Ok(AcousticState::Deciding));
        let _ = runtime.event_tx.send(VoiceCompanionEvent::Transcribing);
        let old = runtime.snapshot();
        let generation = runtime.playback.generation_id();
        VoiceCompanionRuntime::start_utterance(
            vec![0.1; 1600],
            runtime.driver.clone(),
            runtime.router.clone(),
            runtime.transcriber.clone(),
            runtime.playback.clone(),
            runtime.event_tx.clone(),
            runtime.tts.clone(),
            runtime.task_client.clone(),
            &runtime.active_turn,
            runtime.input_generation.clone(),
            generation,
            runtime.pending_utterances.clone(),
            old.session_id.clone(),
            runtime.engine_check_native_busy.clone(),
            runtime.native_degraded.clone(),
            runtime.native_job_epoch.clone(),
            runtime.native_admission.clone(),
            None,
            None,
        );
        tokio::time::timeout(std::time::Duration::from_secs(2), gate.reached.notified()).await.unwrap();

        let _ = runtime.event_tx.send(VoiceCompanionEvent::Transcribing);
        let new = runtime.snapshot();
        assert_ne!(new.turn_id, old.turn_id);
        gate.resume.notify_one();
        let turn = runtime.active_turn.lock().unwrap().take().unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), turn).await.unwrap().unwrap();

        let after = runtime.snapshot();
        assert_eq!(after.turn_id, new.turn_id);
        assert_eq!(after.phase, nomifun_api_types::voice::VoiceSessionPhase::Transcribing);
        assert!(after.assistant_text.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stale_dialogue_error_cannot_change_a_new_session() {
        use nomifun_voice_terminal::{contract::VoiceDecision, driver::{RouteError, VoiceRouter}};

        struct Gate {
            reached: Notify,
            resume: std::sync::Mutex<Option<std::sync::mpsc::Receiver<()>>>,
        }
        struct UnavailableRouter(Arc<Gate>);
        #[async_trait::async_trait]
        impl VoiceRouter for UnavailableRouter {
            fn is_configured(&self) -> bool {
                if let Some(resume) = self.0.resume.lock().unwrap().take() {
                    self.0.reached.notify_one();
                    resume.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
                }
                false
            }

            async fn route_with_status(
                &self,
                _: &str,
                _: Option<&str>,
            ) -> Result<VoiceDecision, RouteError> {
                panic!("an unavailable router must not be called")
            }
        }

        let root = tempdir().unwrap();
        let (capture, _producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        let gate = Arc::new(Gate {
            reached: Notify::new(),
            resume: std::sync::Mutex::new(Some(resume_rx)),
        });
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::FakeSttCallback::always("旧会话问题")),
            Arc::new(UnavailableRouter(gate.clone())),
        );
        runtime.update_feedback(serde_json::from_value(serde_json::json!({"mode":"off","volume":0,"speak_replies":false})).unwrap()).unwrap();
        runtime.handle_wake().await.unwrap();
        let _ = runtime.event_tx.send(VoiceCompanionEvent::Transcribing);
        let generation = runtime.playback.generation_id();
        VoiceCompanionRuntime::start_utterance(
            vec![0.1; 160],
            runtime.driver.clone(),
            runtime.router.clone(),
            runtime.transcriber.clone(),
            runtime.playback.clone(),
            runtime.event_tx.clone(),
            runtime.tts.clone(),
            runtime.task_client.clone(),
            &runtime.active_turn,
            runtime.input_generation.clone(),
            generation,
            runtime.pending_utterances.clone(),
            runtime.snapshot().session_id,
            runtime.engine_check_native_busy.clone(),
            runtime.native_degraded.clone(),
            runtime.native_job_epoch.clone(),
            runtime.native_admission.clone(),
            None,
            None,
        );
        tokio::time::timeout(std::time::Duration::from_secs(3), gate.reached.notified()).await.unwrap();
        let old = runtime.snapshot();
        runtime.stop_session(&VoiceSessionControlRequest {
            instance_id: old.instance_id,
            session_id: old.session_id,
            turn_id: old.turn_id,
        }).await.unwrap();
        runtime.handle_wake().await.unwrap();
        let new = runtime.snapshot();
        resume_tx.send(()).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let after = runtime.snapshot();
        assert_eq!(after.session_id, new.session_id);
        assert_eq!(after.phase, nomifun_api_types::voice::VoiceSessionPhase::Listening);
        assert!(after.error.is_none(), "late old-turn error leaked into the new session: {:?}", after.error);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stale_history_failure_cannot_end_a_new_session_after_interrupt() {
        use nomifun_voice_terminal::{contract::VoiceDecision, driver::{RouteError, VoiceRouter}, playback::TestAudioSink};

        struct Gate {
            reached: Notify,
            resume: Notify,
        }
        struct RecordFailureRouter(Arc<Gate>);
        #[async_trait::async_trait]
        impl VoiceRouter for RecordFailureRouter {
            fn is_configured(&self) -> bool { true }
            async fn route_with_status(
                &self,
                _: &str,
                _: Option<&str>,
            ) -> Result<VoiceDecision, RouteError> {
                self.0.reached.notify_one();
                self.0.resume.notified().await;
                Err(RouteError::Record {
                    code: "binding_invalid".into(),
                    content: "old turn record failure".into(),
                })
            }
        }

        let root = tempdir().unwrap();
        let (capture, _producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let gate = Arc::new(Gate { reached: Notify::new(), resume: Notify::new() });
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::FakeSttCallback::always("旧会话问题")),
            Arc::new(RecordFailureRouter(gate.clone())),
        );
        let runtime = runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(MockSpeech(Arc::new(AtomicUsize::new(0)))),
        );
        runtime.update_feedback(serde_json::from_value(serde_json::json!({"mode":"off","volume":0,"speak_replies":false})).unwrap()).unwrap();
        runtime.handle_wake().await.unwrap();
        assert_eq!(runtime.driver.lock().await.handle_event(AcousticEvent::SpeechEnd), Ok(AcousticState::Deciding));
        let _ = runtime.event_tx.send(VoiceCompanionEvent::Transcribing);
        let old = runtime.snapshot();
        let generation = runtime.playback.generation_id();
        VoiceCompanionRuntime::start_utterance(
            vec![0.1; 160],
            runtime.driver.clone(),
            runtime.router.clone(),
            runtime.transcriber.clone(),
            runtime.playback.clone(),
            runtime.event_tx.clone(),
            runtime.tts.clone(),
            runtime.task_client.clone(),
            &runtime.active_turn,
            runtime.input_generation.clone(),
            generation,
            runtime.pending_utterances.clone(),
            old.session_id.clone(),
            runtime.engine_check_native_busy.clone(),
            runtime.native_degraded.clone(),
            runtime.native_job_epoch.clone(),
            runtime.native_admission.clone(),
            None,
            None,
        );
        tokio::time::timeout(std::time::Duration::from_secs(2), gate.reached.notified()).await.unwrap();

        let mut new_driver = runtime.driver.lock().await;
        gate.resume.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while runtime.playback.generation_id() == generation {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }).await.unwrap();
        new_driver.reset();
        assert_eq!(new_driver.handle_event(AcousticEvent::WakeHit), Ok(AcousticState::Listening));
        let _ = runtime.event_tx.send(VoiceCompanionEvent::WakeHit {
            keyword: "Hey Flowy".into(),
            request_id: None,
            locale: None,
        });
        let new = runtime.snapshot();
        assert_ne!(new.session_id, old.session_id);
        drop(new_driver);
        let turn = runtime.active_turn.lock().unwrap().take().unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), turn).await.unwrap().unwrap();

        let after = runtime.snapshot();
        assert_eq!(after.session_id, new.session_id);
        assert_eq!(after.phase, nomifun_api_types::voice::VoiceSessionPhase::Listening);
        assert!(after.ended_reason.is_none(), "late old-turn history failure ended the new session");
        assert_eq!(runtime.driver.lock().await.current_state(), AcousticState::Listening);
    }

    struct RecordingSpeech(Arc<std::sync::Mutex<Vec<(String, String)>>>);
    #[async_trait::async_trait]
    impl nomifun_voice_terminal::tts::TtsProvider for RecordingSpeech {
        async fn synthesize_stream(
            &self,
            text: &str,
            voice: &str,
        ) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, nomifun_voice_terminal::tts::TtsError> {
            self.0.lock().unwrap().push((text.to_owned(), voice.to_owned()));
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            tx.try_send(nomifun_audio::pcm_to_wav(&vec![0.05; 2400], 24000)).unwrap();
            Ok(rx)
        }
    }

    #[derive(Clone, Copy)]
    enum InvalidSpeech {
        Silent,
        NonFinite,
        TooShort,
        Empty,
    }

    struct InvalidSpeechProvider(InvalidSpeech);
    #[async_trait::async_trait]
    impl nomifun_voice_terminal::tts::TtsProvider for InvalidSpeechProvider {
        async fn synthesize_stream(
            &self,
            _: &str,
            _: &str,
        ) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, nomifun_voice_terminal::tts::TtsError> {
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            let pcm = match self.0 {
                InvalidSpeech::Silent => vec![0.0; 2400],
                InvalidSpeech::NonFinite => vec![f32::NAN; 2400],
                InvalidSpeech::TooShort => vec![0.05; 100],
                InvalidSpeech::Empty => Vec::new(),
            };
            tx.try_send(nomifun_audio::pcm_to_wav(&pcm, 24000)).unwrap();
            Ok(rx)
        }
    }

    #[tokio::test]
    async fn worker_engine_check_cancel_and_deadline_fail_before_inference() {
        let root = tempdir().unwrap();
        let kws = KwsEngine::new(nomifun_audio::kws::KwsEngineConfig {
            model_dir: Some(root.path().to_path_buf()),
            ..Default::default()
        })
        .unwrap();
        let vad = EnergyVad::new(VadConfig::default());
        let busy = Arc::new(AtomicBool::new(false));
        let job_epoch = Arc::new(AtomicU64::new(0));
        let degraded = Arc::new(NativeHealth::default());
        let runner = NativeCallRunner::new(busy.clone(), job_epoch.clone());
        let kws = NativeKws::new(Box::new(kws), runner.clone());
        let vad = NativeVad::new(Box::new(vad), runner);
        let fixtures = SelfCheckAudio {
            wake: vec![0.1; 3_200],
            command: vec![0.1; 3_200],
            non_wake: vec![0.1; 3_200],
            source: "test".into(),
            online: false,
            generated_at_ms: None,
        };
        let cancelled = AtomicBool::new(true);
        let notify = Notify::new();
        let result = VoiceCompanionRuntime::worker_engine_check(
            &kws,
            &vad,
            &None,
            &fixtures,
            &cancelled,
            &notify,
            &busy,
            &degraded,
            &job_epoch,
            std::time::Instant::now() + std::time::Duration::from_secs(1),
        )
        .await;
        assert_eq!(result.unwrap_err(), "engine_check_cancelled");
        let result = VoiceCompanionRuntime::worker_engine_check(
            &kws,
            &vad,
            &None,
            &fixtures,
            &AtomicBool::new(false),
            &notify,
            &busy,
            &degraded,
            &job_epoch,
            std::time::Instant::now() - std::time::Duration::from_millis(1),
        )
        .await;
        assert_eq!(result.unwrap_err(), "engine_check_cancelled");
    }

    #[tokio::test]
    async fn cancel_engine_check_only_matches_the_request_id() {
        let root = tempdir().unwrap();
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let cancel = Arc::new(AtomicBool::new(false));
        *runtime.engine_check_control.lock().unwrap() = Some(EngineCheckControl {
            id: "current".into(),
            epoch: 1,
            cancel: cancel.clone(),
            cancel_notify: Arc::new(Notify::new()),
        });
        assert!(!runtime.cancel_engine_check("other").await);
        assert!(!cancel.load(Ordering::Acquire));
        assert!(runtime.cancel_engine_check("current").await);
        assert!(cancel.load(Ordering::Acquire));
    }

    #[test]
    fn wake_and_call_admission_are_separate_scopes() {
        let root = tempdir().unwrap();
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        runtime.set_admission("wake", true, nomifun_common::now_ms() + 60_000);
        runtime.set_admission("call", false, 0);
        assert!(runtime.admission_valid("wake"));
        assert!(!runtime.admission_valid("call"));
        runtime.set_admission("wake", false, 0);
        runtime.set_admission("call", true, nomifun_common::now_ms() + 60_000);
        assert!(!runtime.admission_valid("wake"));
        assert!(runtime.admission_valid("call"));
    }
    #[test]
    fn host_pushed_wake_admission_expires_without_background_checks() {
        let root = tempdir().unwrap();
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        assert!(runtime.admission_valid("wake"), "test replay starts explicitly admitted");
        runtime.set_wake_admission(false, 0);
        assert!(!runtime.admission_valid("wake"));
        runtime.set_wake_admission(true, nomifun_common::now_ms() + 60_000);
        assert!(runtime.admission_valid("wake"));
        runtime.set_wake_admission(true, nomifun_common::now_ms() - 1);
        assert!(!runtime.admission_valid("wake"), "an expired push must deny wake");
    }
    struct GatedStt {
        blocking: Arc<AtomicBool>,
        entered: Arc<Notify>,
        release: Arc<Notify>,
    }

    struct BlockingNativeStt(BlockingGate);

    #[async_trait::async_trait]
    impl SttCallback for BlockingNativeStt {
        async fn transcribe(&self, _: AudioChannel, _: Vec<f32>, _: u32) -> Option<String> {
            let gate = self.0.clone();
            tokio::task::spawn_blocking(move || gate.block()).await.unwrap();
            Some("recognized".into())
        }
    }

    #[tokio::test]
    async fn aborting_asr_keeps_the_native_lease_until_blocking_work_returns() {
        let gate = BlockingGate::default();
        let _cleanup = ReleaseBlockingGateOnDrop(gate.clone());
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(0));
        let health = Arc::new(NativeHealth::default());
        let admission = Arc::new(NativeAdmission::default());
        let reservation = admission.reserve();
        let task = {
            let (busy, epoch, health, gate) = (busy.clone(), epoch.clone(), health.clone(), gate.clone());
            tokio::spawn(async move {
                VoiceCompanionRuntime::transcribe_cancelable(
                    Arc::new(BlockingNativeStt(gate)), vec![0.1; 160], &AtomicBool::new(false), &Notify::new(),
                    &busy, &health, &epoch, std::time::Instant::now() + std::time::Duration::from_secs(2),
                    true, Some(reservation), None,
                    #[cfg(feature = "native-fault-injection")]
                    None,
                ).await
            })
        };
        wait_for_gate_entry(&gate).await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(!admission.has_pending());
        assert!(busy.load(Ordering::Acquire), "blocking work still owns the native lease");
        let runner = NativeCallRunner::with_admission(busy.clone(), epoch, health, admission, false);
        let result = runner.acquire(std::time::Instant::now() + std::time::Duration::from_millis(100), &AtomicBool::new(false), &Notify::new(), false).await;
        assert_eq!(result.err().as_deref(), Some("engine_check_native_busy"));
        gate.release();
        wait_for_native_release(&busy).await;
    }

    #[async_trait::async_trait]
    impl SttCallback for GatedStt {
        async fn transcribe(
            &self,
            _channel: AudioChannel,
            _pcm: Vec<f32>,
            _sample_rate: u32,
        ) -> Option<String> {
            self.entered.notify_one();
            if self.blocking.load(Ordering::Acquire) {
                self.release.notified().await;
            }
            Some("今天天气 hello friend".into())
        }
    }

    fn engine_command(
        request_id: &str,
        deadline: std::time::Instant,
        cancel: Arc<AtomicBool>,
        native_busy: Arc<AtomicBool>,
        native_job_epoch: Arc<AtomicU64>,
    ) -> (WorkerEngineCheck, oneshot::Receiver<Result<WorkerEngineCheckResult, String>>) {
        let (reply, rx) = oneshot::channel();
        (
            WorkerEngineCheck {
                request_id: request_id.into(),
                deadline,
                cancel,
                cancel_notify: Arc::new(Notify::new()),
                native_busy,
                native_job_epoch,
                fixtures: SelfCheckAudio {
                    wake: vec![0.1; 3_200],
                    command: vec![0.1; 3_200],
                    non_wake: vec![0.1; 3_200],
                    source: "test".into(),
                    online: false,
                    generated_at_ms: None,
                },
                reply,
            },
            rx,
        )
    }

    #[tokio::test]
    async fn non_priority_runner_cannot_acquire_lease_while_reservation_is_active() {
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(1));
        let degraded = Arc::new(NativeHealth::default());
        let admission = Arc::new(NativeAdmission::default());
        let reservation = admission.reserve();
        assert!(admission.has_pending());

        let runner = NativeCallRunner::with_admission(
            busy.clone(),
            epoch.clone(),
            degraded,
            admission.clone(),
            false,
        );

        let cancel = AtomicBool::new(false);
        let notify = Notify::new();
        // Fast-fail call immediately fails because reservation is pending
        let result = runner
            .acquire(
                std::time::Instant::now() + std::time::Duration::from_millis(100),
                &cancel,
                &notify,
                false,
            )
            .await;
        assert_eq!(result.err().as_deref(), Some("engine_check_native_busy"));
        assert!(!busy.load(Ordering::Acquire));

        // Waiting call yields and does not acquire lease before reservation is dropped
        let acquired = Arc::new(AtomicBool::new(false));
        let acquired_clone = acquired.clone();
        let runner_clone = runner.clone();
        let handle = tokio::spawn(async move {
            let cancel = AtomicBool::new(false);
            let notify = Notify::new();
            if runner_clone
                .acquire(
                    std::time::Instant::now() + std::time::Duration::from_millis(300),
                    &cancel,
                    &notify,
                    true,
                )
                .await
                .is_ok()
            {
                acquired_clone.store(true, Ordering::Release);
            }
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(!acquired.load(Ordering::Acquire));
        assert!(!busy.load(Ordering::Acquire));

        drop(reservation);
        handle.await.unwrap();
        assert!(acquired.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn reservation_and_background_claim_have_one_linearization_point() {
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(0));
        let health = Arc::new(NativeHealth::default());
        let admission = Arc::new(NativeAdmission::default());
        let (resume, receiver) = std::sync::mpsc::channel();
        let gate = Arc::new(AdmissionClaimTestGate { reached: Notify::new(), resume: std::sync::Mutex::new(receiver) });
        *admission.before_background_claim.lock().unwrap() = Some(gate.clone());
        let runner = NativeCallRunner::with_admission(busy.clone(), epoch.clone(), health.clone(), admission.clone(), false);
        let handle = tokio::runtime::Handle::current();
        let background = { let runner = runner.clone(); tokio::task::spawn_blocking(move || handle.block_on(async move {
            runner.acquire(std::time::Instant::now() + std::time::Duration::from_secs(2), &AtomicBool::new(false), &Notify::new(), false).await.unwrap()
        })) };
        tokio::time::timeout(std::time::Duration::from_secs(1), gate.reached.notified()).await.unwrap();
        let attempted = Arc::new(Notify::new());
        let reserving = { let admission = admission.clone(); let attempted = attempted.clone(); tokio::task::spawn_blocking(move || {
            attempted.notify_one(); admission.reserve()
        }) };
        attempted.notified().await;
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert!(!reserving.is_finished(), "reservation cannot complete between the protected check and claim");
        resume.send(()).unwrap();
        let owner = background.await.unwrap();
        let reservation = reserving.await.unwrap();
        drop(owner);
        assert!(admission.has_pending());
        let denied = runner.acquire(std::time::Instant::now() + std::time::Duration::from_millis(100), &AtomicBool::new(false), &Notify::new(), false).await;
        assert_eq!(denied.err().as_deref(), Some("engine_check_native_busy"));
        let priority = NativeCallRunner::with_admission(busy.clone(), epoch, health, admission.clone(), true);
        let owner = priority.acquire(std::time::Instant::now() + std::time::Duration::from_millis(100), &AtomicBool::new(false), &Notify::new(), true).await.unwrap();
        drop(reservation);
        drop(owner);
        assert!(!busy.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn non_priority_runner_proceeds_after_reservation_is_dropped() {
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(1));
        let degraded = Arc::new(NativeHealth::default());
        let admission = Arc::new(NativeAdmission::default());
        let mut reservation = Some(admission.reserve());

        let runner = NativeCallRunner::with_admission(
            busy.clone(),
            epoch.clone(),
            degraded,
            admission.clone(),
            false,
        );

        let handle = tokio::spawn(async move {
            let cancel = AtomicBool::new(false);
            let notify = Notify::new();
            runner
                .acquire(
                    std::time::Instant::now() + std::time::Duration::from_secs(1),
                    &cancel,
                    &notify,
                    true,
                )
                .await
        });

        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        assert!(admission.has_pending());

        drop(reservation.take());
        assert!(!admission.has_pending());

        let guard = handle
            .await
            .unwrap()
            .expect("runner should acquire after reservation drop");
        assert!(busy.load(Ordering::Acquire));
        drop(guard);
        assert!(!busy.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn ordinary_asr_waits_for_a_transient_native_lease() {
        let busy = Arc::new(AtomicBool::new(true));
        let epoch = Arc::new(AtomicU64::new(1));
        let degraded = Arc::new(NativeHealth::default());
        let admission = Arc::new(NativeAdmission::default());
        let reservation = admission.reserve();
        let cancel = AtomicBool::new(false);
        let notify = Notify::new();
        let released = {
            let busy = busy.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                busy.store(false, Ordering::Release);
            })
        };
        let started = std::time::Instant::now();
        let asr: Arc<dyn SttCallback> =
            Arc::new(nomifun_audio::stt::FakeSttCallback::always("hello"));
        let result = VoiceCompanionRuntime::transcribe_cancelable(
            asr,
            vec![0.1; 160],
            &cancel,
            &notify,
            &busy,
            &degraded,
            &epoch,
            started + std::time::Duration::from_secs(1),
            true,
            Some(reservation),
            None,
            #[cfg(feature = "native-fault-injection")]
            None,
        )
        .await;
        released.await.unwrap();
        assert_eq!(result.unwrap().as_deref(), Some("hello"));
        assert!(started.elapsed() >= std::time::Duration::from_millis(40));
        assert!(!admission.has_pending());
    }

    #[tokio::test]
    async fn asr_task_cancellation_resets_reservation_and_unblocks_native_calls() {
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(1));
        let degraded = Arc::new(NativeHealth::default());
        let admission = Arc::new(NativeAdmission::default());
        let runner = NativeCallRunner::with_admission(
            busy.clone(),
            epoch.clone(),
            degraded.clone(),
            admission.clone(),
            false,
        );
        let owner = runner.acquire(std::time::Instant::now() + std::time::Duration::from_secs(1), &AtomicBool::new(false), &Notify::new(), false).await.unwrap();
        let reservation = admission.reserve();
        let entered = Arc::new(Notify::new());
        let task = {
            let entered = entered.clone();
            let busy = busy.clone();
            let epoch = epoch.clone();
            let degraded = degraded.clone();
            tokio::spawn(async move {
                entered.notify_one();
                VoiceCompanionRuntime::transcribe_cancelable(
                    Arc::new(nomifun_audio::stt::FakeSttCallback::always("hello")), vec![0.1; 160],
                    &AtomicBool::new(false), &Notify::new(), &busy, &degraded, &epoch,
                    std::time::Instant::now() + std::time::Duration::from_secs(2), true,
                    Some(reservation), None,
                    #[cfg(feature = "native-fault-injection")]
                    None,
                ).await
            })
        };
        entered.notified().await;
        assert!(admission.has_pending());
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(!admission.has_pending());
        assert!(busy.load(Ordering::Acquire), "canceling a waiter must not release another native owner");
        drop(owner);
        let guard = runner.acquire(std::time::Instant::now() + std::time::Duration::from_millis(100), &AtomicBool::new(false), &Notify::new(), false).await;
        assert!(guard.is_ok());
    }

    #[tokio::test]
    async fn non_priority_asr_yields_when_reservation_is_active() {
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(1));
        let degraded = Arc::new(NativeHealth::default());
        let admission = Arc::new(NativeAdmission::default());
        let reservation = admission.reserve();
        assert!(admission.has_pending());

        let cancel = AtomicBool::new(false);
        let notify = Notify::new();
        let asr: Arc<dyn SttCallback> =
            Arc::new(nomifun_audio::stt::FakeSttCallback::always("background"));

        let result = VoiceCompanionRuntime::transcribe_cancelable(
            asr.clone(),
            vec![0.1; 160],
            &cancel,
            &notify,
            &busy,
            &degraded,
            &epoch,
            std::time::Instant::now() + std::time::Duration::from_millis(100),
            true,
            None,
            Some(&admission),
            #[cfg(feature = "native-fault-injection")]
            None,
        )
        .await;
        assert_eq!(result.err().as_deref(), Some("engine_check_native_busy"));
        assert!(!busy.load(Ordering::Acquire));

        drop(reservation);
        let result2 = VoiceCompanionRuntime::transcribe_cancelable(
            asr,
            vec![0.1; 160],
            &cancel,
            &notify,
            &busy,
            &degraded,
            &epoch,
            std::time::Instant::now() + std::time::Duration::from_millis(100),
            true,
            None,
            Some(&admission),
            #[cfg(feature = "native-fault-injection")]
            None,
        )
        .await;
        assert_eq!(result2.unwrap().as_deref(), Some("background"));
    }

    #[tokio::test]
    async fn a_second_request_is_rejected_while_native_work_is_still_owned() {
        let root = tempdir().unwrap();
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        runtime.engine_check_native_busy.store(true, Ordering::Release);
        let response = runtime.self_check_engines().await;
        assert_eq!(response.reason.as_deref(), Some("engine_check_native_busy"));
        assert!(!response.kws_ok && !response.vad_ok && !response.asr_ok);
        assert!(!runtime.engine_check_active.load(Ordering::Acquire));
        runtime.engine_check_native_busy.store(false, Ordering::Release);
    }

    #[tokio::test]
    async fn cancel_during_worker_asr_returns_bounded_and_keeps_native_busy_until_release() {
        let root = tempdir().unwrap();
        let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let blocking = Arc::new(AtomicBool::new(true));
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let stt = GatedStt {
            blocking: blocking.clone(),
            entered: entered.clone(),
            release: release.clone(),
        };
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(stt),
            Arc::new(QwenVoiceRouter::new()),
        );
        let runtime = Arc::new(runtime);
        let worker = runtime.start_worker();
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        let cancel = Arc::new(AtomicBool::new(false));
        let busy = runtime.engine_check_native_busy.clone();
        let (command, reply) = engine_command(
            "cancel-during-asr",
            std::time::Instant::now() + std::time::Duration::from_secs(5),
            cancel.clone(),
            busy.clone(),
            runtime.native_job_epoch.clone(),
        );
        let notify = command.cancel_notify.clone();
        runtime
            .engine_check_tx
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .send(command)
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
            .await
            .expect("worker ASR started");
        cancel.store(true, Ordering::Release);
        notify.notify_waiters();
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), reply)
            .await
            .expect("cancel must return promptly")
            .unwrap();
        assert_eq!(result.unwrap_err(), "engine_check_cancelled");
        assert!(!runtime.native_degraded.is_degraded());
        assert!(busy.load(Ordering::Acquire), "native decode must still be owned");
        blocking.store(false, Ordering::Release);
        release.notify_waiters();
        for _ in 0..50 {
            if !busy.load(Ordering::Acquire) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(!busy.load(Ordering::Acquire), "native ownership must clear only on completion");

        let (command, reply) = engine_command(
            "recovered",
            std::time::Instant::now() + std::time::Duration::from_secs(5),
            Arc::new(AtomicBool::new(false)),
            busy.clone(),
            runtime.native_job_epoch.clone(),
        );
        runtime
            .engine_check_tx
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .send(command)
            .await
            .unwrap();
        let recovered = tokio::time::timeout(std::time::Duration::from_secs(2), reply)
            .await
            .expect("next check must recover")
            .unwrap()
            .expect("next check must run");
        assert!(recovered.asr_ok);
        worker.abort();
        let _ = worker.await;
        producer.close();
    }

    #[tokio::test]
    async fn timeout_during_worker_asr_does_not_grant_and_keeps_ownership() {
        let root = tempdir().unwrap();
        let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let blocking = Arc::new(AtomicBool::new(true));
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(GatedStt {
                blocking: blocking.clone(),
                entered: entered.clone(),
                release: release.clone(),
            }),
            Arc::new(QwenVoiceRouter::new()),
        );
        let runtime = Arc::new(runtime);
        let worker = runtime.start_worker();
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let busy = runtime.engine_check_native_busy.clone();
        let (command, reply) = engine_command(
            "timeout-during-asr",
            std::time::Instant::now() + std::time::Duration::from_millis(750),
            Arc::new(AtomicBool::new(false)),
            busy.clone(),
            runtime.native_job_epoch.clone(),
        );
        runtime
            .engine_check_tx
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .send(command)
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
            .await
            .expect("worker ASR started");
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), reply)
            .await
            .expect("deadline must return promptly")
            .unwrap();
        assert_eq!(result.unwrap_err(), "engine_check_timeout");
        assert!(runtime.native_degraded.is_degraded());
        assert!(busy.load(Ordering::Acquire));
        blocking.store(false, Ordering::Release);
        release.notify_waiters();
        for _ in 0..50 {
            if !busy.load(Ordering::Acquire) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(!busy.load(Ordering::Acquire));
        worker.abort();
        let _ = worker.await;
        producer.close();
    }

    #[tokio::test]
    async fn tts_self_check_is_excluded_by_an_engine_check() {
        let root = tempdir().unwrap();
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        runtime.engine_check_active.store(true, Ordering::Release);
        assert_eq!(
            runtime.self_check_tts().await.unwrap_err(),
            "engine_check_in_progress"
        );
        runtime.engine_check_active.store(false, Ordering::Release);
    }

    #[tokio::test]
    async fn wake_is_rejected_while_a_fixed_self_check_owns_the_runtime() {
        use nomifun_voice_terminal::playback::TestAudioSink;

        let root = tempdir().unwrap();
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let runtime = runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(MockSpeech(Arc::new(std::sync::atomic::AtomicUsize::new(0)))),
        );
        runtime.self_check_active.store(true, Ordering::Release);
        let result = runtime.handle_wake_request("Hey Flowy", None).await;
        assert!(matches!(result, Err(VoiceWakeError::Busy)));
        runtime.self_check_active.store(false, Ordering::Release);
    }

    struct SlowSpeech;
    #[async_trait::async_trait]
    impl nomifun_voice_terminal::tts::TtsProvider for SlowSpeech {
        async fn synthesize_stream(
            &self,
            _: &str,
            _: &str,
        ) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, nomifun_voice_terminal::tts::TtsError> {
            tokio::time::sleep(std::time::Duration::from_millis(80)).await;
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            tx.try_send(nomifun_audio::pcm_to_wav(&vec![0.05; 2400], 24000)).unwrap();
            Ok(rx)
        }
    }

    #[tokio::test]
    async fn daemon_cancel_stops_the_matching_self_check() {
        use nomifun_voice_terminal::playback::TestAudioSink;

        let root = tempdir().unwrap();
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let runtime = Arc::new(runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(SlowSpeech),
        ));
        let check = runtime.clone();
        let task = tokio::spawn(async move { check.self_check_tts_with_id("check-cancel").await });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert!(runtime.cancel_self_check("check-cancel").await);
        let result = task.await.unwrap();
        assert!(matches!(result, Err(reason) if reason == "self_check_cancelled"));
        assert!(!runtime.self_check_active.load(Ordering::Acquire));
    }
    #[tokio::test]
    async fn tts_self_check_rejects_silent_nonfinite_short_and_empty_audio() {
        use nomifun_voice_terminal::playback::TestAudioSink;

        for case in [
            InvalidSpeech::Silent,
            InvalidSpeech::NonFinite,
            InvalidSpeech::TooShort,
            InvalidSpeech::Empty,
        ] {
            let root = tempdir().unwrap();
            let models = Arc::new(ModelManager::new(root.path()));
            let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
            let (runtime, _) = VoiceCompanionRuntime::from_capture(
                models,
                Arc::new(OobeManager::new_bypassed()),
                Arc::new(Mutex::new(capture)),
                Arc::new(nomifun_audio::stt::NullSttCallback),
                Arc::new(QwenVoiceRouter::new()),
            );
            let runtime = runtime.with_test_playback(
                Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
                Arc::new(InvalidSpeechProvider(case)),
            );
            let result = runtime.self_check_tts().await.unwrap();
            assert!(!result.ok, "invalid audio must fail the check");
            assert!(!result.playback_ok, "invalid audio must never be played");
            assert!(result.languages.iter().all(|language| !language.ok));
        }
    }
    #[tokio::test]
    async fn tts_self_check_reports_bilingual_decode_and_sink_consumption() {
        use nomifun_voice_terminal::playback::TestAudioSink;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let root = tempdir().unwrap();
        let models = Arc::new(ModelManager::new(root.path()));
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            models,
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let runtime = runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(MockSpeech(calls.clone())),
        );

        let result = runtime.self_check_tts().await.unwrap();
        assert!(result.ok, "both fixed languages must decode");
        assert!(result.playback_ok, "the fixed sample must reach the sink");
        assert_eq!(result.languages.len(), 2);
        assert!(result.languages.iter().all(|language| language.ok));
        assert!(result.languages.iter().all(|language| language.samples > 0));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
    #[tokio::test]
    async fn tts_self_check_startup_broadcast_and_bilingual_stages() {
        use nomifun_voice_terminal::playback::TestAudioSink;

        let root = tempdir().unwrap();
        let models = Arc::new(ModelManager::new(root.path()));
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let recorded = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            models,
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let runtime = runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(RecordingSpeech(recorded.clone())),
        );

        // 1. Default stage (formerly "语音连接测试。") with zh-CN
        recorded.lock().unwrap().clear();
        let res_zh = runtime.self_check_tts_with_options(None, Some("zh-CN"), None).await.unwrap();
        assert!(res_zh.ok);
        let calls_zh = recorded.lock().unwrap().clone();
        assert_eq!(calls_zh[0], ("正在启动语音服务。".to_string(), "zh-CN-XiaoxiaoNeural".to_string()));

        // 2. Default stage with en-US
        recorded.lock().unwrap().clear();
        let res_en = runtime.self_check_tts_with_options(None, Some("en-US"), None).await.unwrap();
        assert!(res_en.ok);
        let calls_en = recorded.lock().unwrap().clone();
        assert_eq!(calls_en[1], ("Starting voice service.".to_string(), "en-US-JennyNeural".to_string()));

        // 3. Preparing stage with zh-CN
        recorded.lock().unwrap().clear();
        runtime.self_check_tts_with_options(None, Some("zh-CN"), Some("preparing")).await.unwrap();
        let calls_prep_zh = recorded.lock().unwrap().clone();
        assert_eq!(calls_prep_zh[0], ("正在准备语音通话。".to_string(), "zh-CN-XiaoxiaoNeural".to_string()));

        // 4. Preparing stage with en-US
        recorded.lock().unwrap().clear();
        runtime.self_check_tts_with_options(None, Some("en-US"), Some("preparing")).await.unwrap();
        let calls_prep_en = recorded.lock().unwrap().clone();
        assert_eq!(calls_prep_en[1], ("Preparing your voice call.".to_string(), "en-US-JennyNeural".to_string()));

        // 5. Task creation with zh-CN
        recorded.lock().unwrap().clear();
        runtime.self_check_tts_with_options(None, Some("zh-CN"), Some("create_task")).await.unwrap();
        let calls_task_zh = recorded.lock().unwrap().clone();
        assert_eq!(calls_task_zh[0], ("创建语音任务中".to_string(), "zh-CN-XiaoxiaoNeural".to_string()));

        // 6. Task creation with en-US
        recorded.lock().unwrap().clear();
        runtime.self_check_tts_with_options(None, Some("en-US"), Some("create_task")).await.unwrap();
        let calls_task_en = recorded.lock().unwrap().clone();
        assert_eq!(calls_task_en[1], ("Creating voice task...".to_string(), "en-US-JennyNeural".to_string()));
    }
    #[tokio::test]
    async fn test_progress_must_not_block_final_result_of_same_task() {
        use nomifun_api_types::voice::{VoiceTaskResultAnnouncement, VoiceSessionControlRequest, VoiceSessionPhase};
        use nomifun_voice_terminal::playback::TestAudioSink;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let root = tempdir().unwrap();
        let models = Arc::new(ModelManager::new(root.path()));
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            models,
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let runtime = Arc::new(runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(MockSpeech(calls.clone())),
        ));
        runtime
            .update_feedback(
                serde_json::from_value(serde_json::json!({
                    "mode": "off",
                    "volume": 0,
                    "speak_replies": true,
                }))
                .unwrap(),
            )
            .unwrap();

        let worker = runtime.start_worker();
        let notice = |rt: &VoiceCompanionRuntime, kind: &str, text: &str| {
            let snapshot = rt.snapshot();
            VoiceTaskResultAnnouncement {
                scope: VoiceSessionControlRequest {
                    instance_id: snapshot.instance_id,
                    session_id: snapshot.session_id,
                    turn_id: snapshot.turn_id,
                },
                command_id: "task-001".into(),
                notification_id: Some(format!("{kind}:task-001")),
                attempt_id: Some(format!("{kind}:task-001:1")),
                goal: None,
                text: text.into(),
            }
        };

        // 1. Progress announcement accepted
        runtime.announce_task_result(notice(&runtime, "progress", "正在调用搜索工具。")).await.unwrap();

        // Wait until speech finished and follow-up or idle entered
        tokio::time::timeout(std::time::Duration::from_secs(8), async {
            while runtime.snapshot().phase != VoiceSessionPhase::Idle && runtime.snapshot().phase != VoiceSessionPhase::FollowUp {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();

        // 2. Final result announcement for same command_id must succeed and not be blocked as conflict
        let result = runtime.announce_task_result(notice(&runtime, "result", "这是最终搜索结果。")).await;
        worker.abort();

        assert_eq!(result, Ok(()), "different notification_id for the same command_id must succeed");
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        // 3. Status query by attempt_id, notification_id, command_id
        assert!(runtime.notification_status("result:task-001:1").await.is_some());
        assert!(runtime.notification_status("result:task-001").await.is_some());
        assert!(runtime.notification_status("task-001").await.is_some());
    }

    #[tokio::test]
    async fn test_explicit_replay_must_create_a_receipt_and_play_again() {
        use nomifun_api_types::voice::{VoiceTaskResultAnnouncement, VoiceSessionControlRequest, VoiceSessionPhase};
        use nomifun_voice_terminal::playback::TestAudioSink;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let root = tempdir().unwrap();
        let models = Arc::new(ModelManager::new(root.path()));
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            models,
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let runtime = Arc::new(runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(MockSpeech(calls.clone())),
        ));
        runtime
            .update_feedback(
                serde_json::from_value(serde_json::json!({
                    "mode": "off",
                    "volume": 0,
                    "speak_replies": true,
                }))
                .unwrap(),
            )
            .unwrap();

        let worker = runtime.start_worker();
        let notice = |rt: &VoiceCompanionRuntime, kind: &str, text: &str| {
            let snapshot = rt.snapshot();
            VoiceTaskResultAnnouncement {
                scope: VoiceSessionControlRequest {
                    instance_id: snapshot.instance_id,
                    session_id: snapshot.session_id,
                    turn_id: snapshot.turn_id,
                },
                command_id: "same-task".into(),
                notification_id: Some(format!("{kind}:same-task")),
                attempt_id: Some(format!("{kind}:same-task:1")),
                goal: None,
                text: text.into(),
            }
        };

        let first = notice(&runtime, "result", "这是最终结果。");
        runtime.announce_task_result(first).await.unwrap();

        tokio::time::timeout(std::time::Duration::from_secs(8), async {
            while runtime.snapshot().phase != VoiceSessionPhase::Idle && runtime.snapshot().phase != VoiceSessionPhase::FollowUp {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();

        let mut replay = notice(&runtime, "result", "这是最终结果。");
        replay.attempt_id = Some("result:same-task:2".into());
        runtime.announce_task_result(replay).await.unwrap();
        worker.abort();

        assert!(
            runtime.notification_status("result:same-task:2").await.is_some(),
            "accepted explicit replay must have its own attempt receipt"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_changed_payload_of_same_logical_notification_must_conflict() {
        use nomifun_api_types::voice::{VoiceTaskResultAnnouncement, VoiceSessionControlRequest};
        use nomifun_voice_terminal::playback::TestAudioSink;
        use std::sync::atomic::AtomicUsize;

        let root = tempdir().unwrap();
        let models = Arc::new(ModelManager::new(root.path()));
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            models,
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let runtime = Arc::new(runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(MockSpeech(calls)),
        ));
        runtime
            .update_feedback(
                serde_json::from_value(serde_json::json!({
                    "mode": "off",
                    "volume": 0,
                    "speak_replies": true,
                }))
                .unwrap(),
            )
            .unwrap();

        let notice = |rt: &VoiceCompanionRuntime, kind: &str, text: &str| {
            let snapshot = rt.snapshot();
            VoiceTaskResultAnnouncement {
                scope: VoiceSessionControlRequest {
                    instance_id: snapshot.instance_id,
                    session_id: snapshot.session_id,
                    turn_id: snapshot.turn_id,
                },
                command_id: "same-task".into(),
                notification_id: Some(format!("{kind}:same-task")),
                attempt_id: Some(format!("{kind}:same-task:1")),
                goal: None,
                text: text.into(),
            }
        };

        runtime.announce_task_result(notice(&runtime, "result", "原始结果。")).await.unwrap();
        let mut changed = notice(&runtime, "result", "不同的新结果。");
        changed.attempt_id = Some("changed-attempt".into());
        assert_eq!(runtime.announce_task_result(changed).await, Err("task_result_conflict"));
    }

    struct FailingTts;
    #[async_trait::async_trait]
    impl nomifun_voice_terminal::tts::TtsProvider for FailingTts {
        async fn synthesize_stream(
            &self,
            _: &str,
            _: &str,
        ) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, nomifun_voice_terminal::tts::TtsError> {
            Err(nomifun_voice_terminal::tts::TtsError::Synthesis("mock_network_down".into()))
        }
    }

    #[tokio::test]
    async fn test_task_result_plays_chime_and_opens_four_second_follow_up_window() {
        use nomifun_api_types::voice::{VoiceTaskResultAnnouncement, VoiceSessionControlRequest, VoiceCueKind};
        use nomifun_voice_terminal::playback::TestAudioSink;

        let root = tempdir().unwrap();
        let models = Arc::new(ModelManager::new(root.path()));
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            models,
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let runtime = Arc::new(runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(MockSpeech(calls.clone())),
        ));
        runtime
            .update_feedback(
                serde_json::from_value(serde_json::json!({
                    "mode": "light",
                    "volume": 80,
                    "speak_replies": true,
                }))
                .unwrap(),
            )
            .unwrap();

        let mut rx = runtime.event_sender().subscribe();
        let snapshot = runtime.snapshot();
        let notice = VoiceTaskResultAnnouncement {
            scope: VoiceSessionControlRequest {
                instance_id: snapshot.instance_id,
                session_id: snapshot.session_id,
                turn_id: snapshot.turn_id,
            },
            command_id: "cmd-step5".into(),
            notification_id: Some("notif-step5".into()),
            attempt_id: Some("attempt-step5:1".into()),
            goal: Some("查询北京天气".into()),
            text: "北京今天晴，气温 15 到 25 度。".into(),
        };

        let result = runtime.announce_task_result(notice).await;
        assert_eq!(result, Ok(()));

        let mut saw_chime = false;
        let mut saw_followup_window = false;
        while let Ok(event) = rx.try_recv() {
            match event {
                VoiceCompanionEvent::Feedback { diagnostic } if diagnostic.kind == VoiceCueKind::Recognized => {
                    saw_chime = true;
                }
                VoiceCompanionEvent::FollowUpWindow { timeout_secs } => {
                    assert_eq!(timeout_secs, 180, "follow-up window must strictly be 180 seconds");
                    saw_followup_window = true;
                }
                _ => {}
            }
        }
        assert!(saw_chime, "task completion chime (Recognized) must play before speaking result");
        assert!(saw_followup_window, "follow-up window of 180 seconds must be announced");
    }

    #[tokio::test]
    async fn test_task_result_tts_failure_plays_error_cue_fallback() {
        use nomifun_api_types::voice::{VoiceTaskResultAnnouncement, VoiceSessionControlRequest, VoiceCueKind};
        use nomifun_voice_terminal::playback::TestAudioSink;

        let root = tempdir().unwrap();
        let models = Arc::new(ModelManager::new(root.path()));
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            models,
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let runtime = Arc::new(runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(FailingTts),
        ));
        runtime
            .update_feedback(
                serde_json::from_value(serde_json::json!({
                    "mode": "light",
                    "volume": 80,
                    "speak_replies": true,
                }))
                .unwrap(),
            )
            .unwrap();

        let mut rx = runtime.event_sender().subscribe();
        let snapshot = runtime.snapshot();
        let notice = VoiceTaskResultAnnouncement {
            scope: VoiceSessionControlRequest {
                instance_id: snapshot.instance_id,
                session_id: snapshot.session_id,
                turn_id: snapshot.turn_id,
            },
            command_id: "cmd-tts-fail".into(),
            notification_id: Some("notif-tts-fail".into()),
            attempt_id: Some("attempt-tts-fail:1".into()),
            goal: Some("测试TTS失败".into()),
            text: "这是一条无法合成的语音消息。".into(),
        };

        let result = runtime.announce_task_result(notice).await;
        assert_eq!(result, Ok(()));

        let mut saw_error_cue = false;
        let mut saw_error_event = false;
        while let Ok(event) = rx.try_recv() {
            match event {
                VoiceCompanionEvent::Feedback { diagnostic } if diagnostic.kind == VoiceCueKind::Error => {
                    saw_error_cue = true;
                }
                VoiceCompanionEvent::Error { stage, code } => {
                    assert_eq!(stage, "playback");
                    assert_eq!(code, "tts_failed");
                    saw_error_event = true;
                }
                _ => {}
            }
        }
        assert!(saw_error_event, "error event must be emitted on TTS synthesis failure");
        assert!(saw_error_cue, "hardcoded local error cue must play on TTS failure");
    }

    #[tokio::test]
    async fn reproduction_task_cancellation_and_interrupt_closure() {
        use nomifun_voice_terminal::playback::TestAudioSink;
        let root = tempdir().unwrap();
        let models = Arc::new(ModelManager::new(root.path()));
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            models,
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let playback = Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new())));
        let runtime = Arc::new(runtime.with_test_playback(playback.clone(), Arc::new(FailingTts)));

        // Setup a simulated task dispatching state
        {
            let mut d = runtime.driver.lock().await;
            let _ = d.handle_event(AcousticEvent::WakeHit);
            let _ = d.handle_event(AcousticEvent::SpeechEnd);
            let _ = d.handle_event(AcousticEvent::IntentAgentTask);
            assert_eq!(d.current_state(), AcousticState::DispatchingTask);
        }

        // Simulate interruption/generation bump while in DispatchingTask
        playback.interrupt().await.unwrap();

        // Check update_task_status_versioned cleans up DispatchingTask
        runtime.update_task_status_versioned(
            Some(nomifun_api_types::voice::VoiceTaskSnapshot {
                command_id: "cmd-test".into(),
                session_id: "s-1".into(),
                acoustic_turn_id: "t-1".into(),
                status: nomifun_api_types::voice::VoiceTaskStatus::Failed,
                error_code: Some("cancelled".into()),
                conversation_id: None,
                message_id: None,
                result_text: None,
                runtime_turn_id: None,
                result_ok: None,
            }),
            None,
            Some(1),
        );

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let state = runtime.driver.lock().await.current_state();
        assert_ne!(
            state,
            AcousticState::DispatchingTask,
            "driver must not be trapped in DispatchingTask when task is interrupted or cancelled"
        );
        assert!(
            matches!(state, AcousticState::Listening | AcousticState::Idle),
            "driver must recover to Listening or Idle so future voice turns can proceed"
        );
    }

    struct RecordingTaskClient {
        current: std::sync::Mutex<Option<nomifun_api_types::voice::VoiceTaskSnapshot>>,
        controls: std::sync::Mutex<Vec<nomifun_api_types::voice::VoiceTaskControlCommand>>,
    }

    #[async_trait::async_trait]
    impl nomifun_voice_terminal::task_client::VoiceTaskClient for RecordingTaskClient {
        async fn submit(&self, _: nomifun_api_types::voice::VoiceTaskSubmitRequest)
            -> Result<nomifun_api_types::voice::VoiceTaskDelivery, String> {
            Err("unused".into())
        }
        async fn current(&self) -> Result<Option<nomifun_api_types::voice::VoiceTaskSnapshot>, String> {
            Ok(self.current.lock().unwrap().clone())
        }
        async fn control(&self, request: nomifun_api_types::voice::VoiceTaskControlCommand)
            -> Result<nomifun_api_types::voice::VoiceTaskSnapshot, String> {
            self.controls.lock().unwrap().push(request);
            self.current.lock().unwrap().clone().ok_or_else(|| "task_not_found".to_owned())
        }
    }

    async fn execute_local_task_control(
        input: &str,
        client: &RecordingTaskClient,
        scope: &nomifun_api_types::voice::VoiceSessionSnapshot,
    ) -> Result<Option<nomifun_api_types::voice::VoiceTaskSnapshot>, String> {
        let Some(action) = local_task_control_action(input, client).await else {
            return Ok(None);
        };
        let Some(task) = client.current().await? else {
            return Err("task_not_found".into());
        };
        client.control(nomifun_api_types::voice::VoiceTaskControlCommand {
            scope: nomifun_api_types::voice::VoiceSessionControlRequest {
                instance_id: scope.instance_id.clone(),
                session_id: scope.session_id.clone(),
                turn_id: scope.turn_id.clone(),
            },
            task: nomifun_api_types::voice::VoiceTaskReference::from(&task),
            action,
            raw_user_text: Some(input.to_owned()),
            locale: scope.locale.clone(),
        }).await.map(Some)
    }

    #[tokio::test]
    async fn local_task_control_never_cancels_on_negation_and_carries_target_identity() {
        use nomifun_api_types::voice::{
            VoiceBindingSnapshot, VoiceSessionSnapshot, VoiceTaskControlAction, VoiceTaskSnapshot,
            VoiceTaskStatus,
        };
        let task = VoiceTaskSnapshot {
            command_id: "command-a".into(),
            session_id: "session-a".into(),
            acoustic_turn_id: "origin-a".into(),
            conversation_id: Some("conversation-a".into()),
            message_id: Some("message-a".into()),
            runtime_turn_id: None,
            status: VoiceTaskStatus::Accepted,
            result_ok: None,
            result_text: None,
            error_code: None,
        };
        let client = RecordingTaskClient {
            current: std::sync::Mutex::new(Some(task)),
            controls: std::sync::Mutex::new(Vec::new()),
        };
        let scope = VoiceSessionSnapshot {
            instance_id: "instance-a".into(),
            session_id: Some("session-a".into()),
            turn_id: Some("turn-a".into()),
            locale: Some("en-US".into()),
            binding: Some(VoiceBindingSnapshot {
                session_id: "session-a".into(),
                binding_id: "binding-a".into(),
                conversation_id: "conversation-a".into(),
                workspace: None,
                memory_owner_companion_id: None,
            }),
            ..Default::default()
        };
        for input in [
            "never cancel task",
            "do not ever cancel task",
            "don't you cancel task",
            "We should cancel the task tomorrow",
            "Explain how to cancel the task",
            "If it fails, cancel the task",
            "stop",
            "请明天取消任务",
            "如果出错，请取消任务",
            "不要取消任务",
            "千万别取消任务",
            "取消任务，但是不要停止",
            "任务取消了",
            "任务已经结束了",
            "今天天气怎么样",
            "今天天气情况怎么样",
            "最近身体情况怎么样",
            "停止任务播放",
        ] {
            let result = execute_local_task_control(input, &client, &scope).await;
            assert!(result.is_ok(), "negated/ambiguous input must not control: {input:?}");
            assert_eq!(client.controls.lock().unwrap().len(), 0, "no control call for {input:?}");
        }

        let query = execute_local_task_control("任务进度怎么样", &client, &scope).await.unwrap().unwrap();
        assert_eq!(query.command_id, "command-a");
        {
            let controls = client.controls.lock().unwrap();
            assert_eq!(controls.len(), 1);
            assert_eq!(controls[0].action, VoiceTaskControlAction::QueryStatus);
            assert_eq!(controls[0].task.command_id, "command-a");
            assert_eq!(controls[0].task.conversation_id.as_deref(), Some("conversation-a"));
            assert_eq!(controls[0].scope.session_id.as_deref(), Some("session-a"));
        }

        let cancelled = execute_local_task_control("cancel the task", &client, &scope).await.unwrap().unwrap();
        assert_eq!(cancelled.command_id, "command-a");
        {
            let controls = client.controls.lock().unwrap();
            assert_eq!(controls.len(), 2);
            assert_eq!(controls[1].action, VoiceTaskControlAction::CancelTask);
            assert_eq!(controls[1].task.command_id, "command-a");
            assert_eq!(controls[1].scope.turn_id.as_deref(), Some("turn-a"));
        }

        let other_scope = VoiceSessionSnapshot {
            session_id: Some("session-b".into()),
            turn_id: Some("turn-b".into()),
            binding: Some(VoiceBindingSnapshot {
                session_id: "session-b".into(),
                binding_id: "binding-b".into(),
                conversation_id: "conversation-b".into(),
                workspace: None,
                memory_owner_companion_id: None,
            }),
            ..scope.clone()
        };
        let cross = execute_local_task_control("cancel the task", &client, &other_scope).await.unwrap().unwrap();
        assert_eq!(cross.command_id, "command-a", "the daemon must not retarget another task");
        {
            let controls = client.controls.lock().unwrap();
            assert_eq!(controls.len(), 3);
            assert_eq!(controls[2].scope.session_id.as_deref(), Some("session-b"));
            assert_eq!(controls[2].task.command_id, "command-a");
            assert_eq!(controls[2].task.conversation_id.as_deref(), Some("conversation-a"));
        }
    }

    #[test]
    fn task_status_prompt_is_limited_to_the_current_acoustic_session() {
        use nomifun_api_types::voice::{VoiceSessionSnapshot, VoiceTaskSnapshot, VoiceTaskStatus};
        let task = VoiceTaskSnapshot {
            command_id: "command-a".into(),
            session_id: "session-a".into(),
            acoustic_turn_id: "origin-a".into(),
            conversation_id: Some("conversation-a".into()),
            message_id: Some("message-a".into()),
            runtime_turn_id: None,
            status: VoiceTaskStatus::Accepted,
            result_ok: None,
            result_text: None,
            error_code: None,
        };
        let other = VoiceSessionSnapshot {
            session_id: Some("session-b".into()),
            ..Default::default()
        };
        assert!(task_status_for_prompt(Some(&task), &other).is_none());
        let same = VoiceSessionSnapshot {
            session_id: Some("session-a".into()),
            ..Default::default()
        };
        let status = task_status_for_prompt(Some(&task), &same).expect("same session must include status");
        assert!(status.contains("command-a"));
        assert!(status.contains("accepted"));
        assert!(task_status_for_prompt(None, &same).is_none());
    }

    #[test]
    fn task_control_requires_an_affirmative_explicit_cancel() {
        use nomifun_api_types::voice::VoiceTaskControlAction::{CancelTask, QueryStatus};
        let cases = [
            ("cancel task", Some(CancelTask)),
            ("cancel the task", Some(CancelTask)),
            ("please cancel the current task", Some(CancelTask)),
            ("CANCEL THE TASK", Some(CancelTask)),
            ("stop the task", Some(CancelTask)),
            ("stop the current job", Some(CancelTask)),
            ("取消任务", Some(CancelTask)),
            ("请帮我取消当前任务", Some(CancelTask)),
            ("停止任务", Some(CancelTask)),
            ("结束任务", Some(CancelTask)),
            ("取消", None),
            ("stop", None),
            ("never cancel task", None),
            ("do not ever cancel task", None),
            ("don't you cancel task", None),
            ("please don't cancel the task", None),
            ("do not cancel task", None),
            ("DON'T CANCEL TASK", None),
            ("never ever cancel task", None),
            ("I would never want you to cancel the task", None),
            ("don't stop the task", None),
            ("stop the task from failing", None),
            ("不要取消任务", None),
            ("千万别取消任务", None),
            ("不要现在取消任务", None),
            ("不要取消", None),
            ("do not stop", None),
            ("停止播放", None),
            ("please stop", None),
            ("取消任务，但是不要停止", None),
            ("不要取消任务，取消吧", None),
            ("取消任务并查询进度", Some(QueryStatus)),
            ("don't cancel the task, just check progress", Some(QueryStatus)),
            ("never cancel task but show progress", Some(QueryStatus)),
            ("任务进度怎么样", Some(QueryStatus)),
            ("what's the task status", Some(QueryStatus)),
            ("今天天气怎么样", None),
            ("今天天气情况怎么样", None),
            ("最近身体情况怎么样", None),
            ("任务情况怎么样", Some(QueryStatus)),
            ("任务取消了", None),
            ("任务已经结束了", None),
            ("任务停止了", None),
            ("任务取消", None),
            ("任务结束", None),
            ("停止任务播放", None),
            ("请取消任务", Some(CancelTask)),
            ("把任务取消", Some(CancelTask)),
            ("立即停止任务", Some(CancelTask)),
            ("你好", None),
            ("创建任务", None),
            ("结束了吗", None),
            ("取消任务吧", Some(CancelTask)),
            ("把任务取消吧", Some(CancelTask)),
            ("请把任务取消", Some(CancelTask)),
            ("取消任务，谢谢", Some(CancelTask)),
            ("你取消任务", Some(CancelTask)),
            ("取消任务。", Some(CancelTask)),
            ("我把任务取消了", None),
            ("把任务停止了", None),
            ("把任务取消了", None),
            ("取消任务很重要", None),
            ("取消任务，可以吗", None),
            ("取消任务可以吗", None),
            ("取消任务好吗", None),
            ("取消任务，好吗", None),
            ("取消任务，多久", None),
            ("取消任务？", None),
            ("取消？", None),
            ("stop?", None),
            ("stop the task?", None),
            ("cancel?", None),
            ("cancel the task?", None),
            ("我取消任务", None),
            ("他取消任务", None),
            ("取消任务了", None),
        ];
        for (input, expected) in cases {
            assert_eq!(detect_task_control(input), expected, "input={input:?}");
        }
    }

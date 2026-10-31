    use super::*;

    fn verify_feature_headless_binary() {
        let binary = std::env::var_os("VOICE_COMPANION_BIN")
            .map(std::path::PathBuf::from)
            .expect("feature headless binary");
        assert_eq!(
            binary.file_name().and_then(|name| name.to_str()),
            Some("voice-companion-headless.exe"),
            "refuse non-feature-headless artifact path before spawn"
        );
        let expected = std::env::var("VOICE_COMPANION_R13_HEADLESS_SHA")
            .expect("expected feature headless SHA");
        let actual = nomifun_voice_models::verifier::compute_sha256(
            &std::fs::read(&binary).expect("read feature binary"),
        );
        assert_eq!(actual, expected, "refuse unexpected feature binary SHA before spawn");
    }

    fn test_port_closed(port: u16) -> bool {
        std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            std::time::Duration::from_millis(300),
        )
        .is_err()
    }

    async fn run_headless_case<F, Fut>(
        manager: Arc<VoiceCompanionProcessManager>,
        port: u16,
        health_timeout: std::time::Duration,
        case_timeout: std::time::Duration,
        body: F,
    ) where
        F: FnOnce(Arc<VoiceCompanionProcessManager>, String, String) -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        use futures_util::FutureExt as _;
        let origin = format!("http://127.0.0.1:{port}");
        let token = manager.control_token.clone();
        let owned_pid = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let run_manager = manager.clone();
        let run_pid = owned_pid.clone();
        let run = async move {
            run_manager.write_autostart_config(true).unwrap();
            run_manager.start_projection();
            run_manager.start().await.unwrap();
            let started_pid = run_manager.status().await.pid.unwrap_or(0);
            run_pid.store(started_pid, Ordering::Release);
            let client = reqwest::Client::builder()
                .no_proxy()
                .timeout(std::time::Duration::from_secs(2))
                .build()
                .unwrap();
            let healthy = tokio::time::timeout(health_timeout, async {
                loop {
                    if client
                        .get(format!("{origin}/healthz"))
                        .send()
                        .await
                        .is_ok_and(|response| response.status().is_success())
                    {
                        return;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            })
            .await
            .is_ok();
            assert!(healthy, "daemon health stage did not become ready");
            body(run_manager, origin, token).await;
        };
        let outcome = tokio::time::timeout(
            case_timeout,
            std::panic::AssertUnwindSafe(run).catch_unwind(),
        )
        .await;

        let mut cleanup_errors = Vec::new();
        match tokio::time::timeout(std::time::Duration::from_secs(10), manager.stop()).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => cleanup_errors.push(format!("stop_error:{error}")),
            Err(_) => cleanup_errors.push("stop_timeout".into()),
        }
        let mut closed = false;
        for _ in 0..20 {
            if test_port_closed(port) {
                closed = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        if !closed {
            cleanup_errors.push(format!("port_still_open:{port}"));
        }
        // Longer than the projection loop's 1s recovery tick: a delayed restart
        // must not appear after stop completed.
        tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
        let after = manager.status().await;
        if after.running || after.pid.is_some() || !test_port_closed(port) {
            cleanup_errors.push("delayed_restart_or_port_reopen".into());
        }
        if !cleanup_errors.is_empty() {
            panic!("cleanup_failed:{cleanup_errors:?}; owned_pid={}", owned_pid.load(Ordering::Acquire));
        }
        eprintln!("cleanup_case_ok owned_pid={} port={port} port_closed=true", owned_pid.load(Ordering::Acquire));
        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(payload)) => std::panic::resume_unwind(payload),
            Err(_) => panic!("case_timeout_after_cleanup:owned_pid={}", owned_pid.load(Ordering::Acquire)),
        }
    }
    struct FixtureCompleter {
        result: Result<String, String>,
    }
    #[async_trait::async_trait]
    impl VoiceDialogueCompleter for FixtureCompleter {
        async fn resolve_model(
            &self,
            _choice: Option<&nomifun_api_types::voice::VoiceModelRef>,
        ) -> Result<nomifun_api_types::voice::VoiceModelRef, String> {
            Ok(nomifun_api_types::voice::VoiceModelRef {
                provider_id: "fixture".into(),
                model: "fixture".into(),
            })
        }
        async fn complete(
            &self,
            _prompt: &str,
            _choice: Option<&nomifun_api_types::voice::VoiceModelRef>,
        ) -> Result<String, String> {
            self.result.clone()
        }
    }

    struct ConfigurableCompleter {
        revision: Arc<std::sync::Mutex<String>>,
    }
    #[async_trait::async_trait]
    impl VoiceDialogueCompleter for ConfigurableCompleter {
        async fn resolve_model(
            &self,
            _choice: Option<&nomifun_api_types::voice::VoiceModelRef>,
        ) -> Result<nomifun_api_types::voice::VoiceModelRef, String> {
            Ok(nomifun_api_types::voice::VoiceModelRef {
                provider_id: "fixture".into(),
                model: "fixture".into(),
            })
        }
        async fn complete(
            &self,
            _prompt: &str,
            _choice: Option<&nomifun_api_types::voice::VoiceModelRef>,
        ) -> Result<String, String> {
            Ok("OK".into())
        }
        async fn configuration_revision(&self) -> String {
            self.revision
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone()
        }
    }
    struct BarrierCompleter {
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
        revision: String,
    }
    #[async_trait::async_trait]
    impl VoiceDialogueCompleter for BarrierCompleter {
        async fn resolve_model(
            &self,
            _choice: Option<&nomifun_api_types::voice::VoiceModelRef>,
        ) -> Result<nomifun_api_types::voice::VoiceModelRef, String> {
            Ok(nomifun_api_types::voice::VoiceModelRef {
                provider_id: "fixture".into(),
                model: "fixture".into(),
            })
        }
        async fn complete(
            &self,
            _prompt: &str,
            _choice: Option<&nomifun_api_types::voice::VoiceModelRef>,
        ) -> Result<String, String> {
            Ok("OK".into())
        }
        async fn configuration_revision(&self) -> String {
            self.entered.notify_one();
            self.release.notified().await;
            self.revision.clone()
        }
    }
    fn populate_checked_suite(models_dir: &std::path::Path) -> std::collections::HashMap<String, String> {
        let mut manifest = std::collections::HashMap::new();
        for &relative in nomifun_voice_models::EXPECTED_FILES {
            let path = models_dir.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let content = format!("capability-suite-{relative}").into_bytes();
            std::fs::write(&path, &content).unwrap();
            if nomifun_voice_models::verifier::DOWNLOAD_FILES.contains(&relative) {
                manifest.insert(
                    relative.to_owned(),
                    nomifun_voice_models::verifier::compute_sha256(&content),
                );
            }
        }
        manifest
    }

    fn engine_payload(
        headers: &axum::http::HeaderMap,
        kws_ok: bool,
        vad_ok: bool,
        asr_ok: bool,
    ) -> axum::Json<serde_json::Value> {
        let header = |name: &str| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
        };
        axum::Json(serde_json::json!({
            "kws_ok": kws_ok,
            "vad_ok": vad_ok,
            "asr_ok": asr_ok,
            "fixtures_available": true,
            "kws_hits": 4,
            "asr_text": "zh=今天天气怎么样？;en=Hello world, Hey friend.",
            "request_id": header("x-voice-check-request"),
            "instance_id": "fixture-daemon-instance-1",
            "worker_epoch": 7,
            "model_revision": header("x-voice-check-model-revision"),
        }))
    }
    async fn tts_fixture(status: u16, body: &'static str) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new()
            .route(
                "/api/voice/diagnostics/engine-self-check",
                axum::routing::get(
                    |headers: axum::http::HeaderMap| async move {
                        engine_payload(&headers, true, true, true)
                    },
                ),
            )
            .route(
            "/api/voice/diagnostics/tts-self-check",
            axum::routing::post(move || async move {
                (
                    axum::http::StatusCode::from_u16(status).unwrap(),
                    [("content-type", "application/json")],
                    body,
                )
            }),
        );
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (origin, handle)
    }

    async fn tts_fixture_with_failed_engine(
        status: u16,
        body: &'static str,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new()
            .route(
                "/api/voice/diagnostics/engine-self-check",
                axum::routing::get(
                    |headers: axum::http::HeaderMap| async move {
                        engine_payload(&headers, true, true, false)
                    },
                ),
            )
            .route(
                "/api/voice/diagnostics/tts-self-check",
                axum::routing::post(move || async move {
                    (
                        axum::http::StatusCode::from_u16(status).unwrap(),
                        [("content-type", "application/json")],
                        body,
                    )
                }),
            );
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (origin, handle)
    }

    async fn tts_fixture_with_stale_engine(
        status: u16,
        body: &'static str,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new()
            .route(
                "/api/voice/diagnostics/engine-self-check",
                axum::routing::get(|| async {
                    axum::Json(serde_json::json!({
                        "kws_ok": true,
                        "vad_ok": true,
                        "asr_ok": true,
                        "fixtures_available": true,
                        "kws_hits": 4,
                        "request_id": "another-check",
                        "instance_id": "another-instance",
                        "worker_epoch": 9,
                        "model_revision": "another-revision",
                    }))
                }),
            )
            .route(
                "/api/voice/diagnostics/tts-self-check",
                axum::routing::post(move || async move {
                    (
                        axum::http::StatusCode::from_u16(status).unwrap(),
                        [("content-type", "application/json")],
                        body,
                    )
                }),
            );
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (origin, handle)
    }

    async fn tts_fixture_without_engine(
        status: u16,
        body: &'static str,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new().route(
            "/api/voice/diagnostics/tts-self-check",
            axum::routing::post(move || async move {
                (
                    axum::http::StatusCode::from_u16(status).unwrap(),
                    [("content-type", "application/json")],
                    body,
                )
            }),
        );
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (origin, handle)
    }
    fn capability_snapshot(manager: &VoiceCompanionProcessManager, last_frame_at_ms: i64) {
        let mut snapshot = manager.snapshot.write().unwrap();
        snapshot.enabled = true;
        snapshot.connection = VoiceConnectionState::Connected;
        snapshot.acoustic_ready = true;
        snapshot.playback_ready = true;
        snapshot.worker_epoch = Some(7);
        snapshot.audio = Some(nomifun_api_types::voice::VoiceAudioDiagnostics {
            frames_received: 5,
            last_frame_at_ms: Some(last_frame_at_ms),
            capture_state: "running".into(),
            kws_ready: true,
            vad_ready: true,
            asr_ready: true,
            ..Default::default()
        });
        *manager.source.write().unwrap() = Some(("fixture-daemon-instance-1".into(), 1));
    }

    #[test]
    fn voice_projection_rejects_missing_and_foreign_turn_sessions_without_dropping_valid_speech() {
        let dir = tempfile::tempdir().unwrap();
        let manager = VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(dir.path())), dir.path());
        manager.snapshot.write().unwrap().binding = Some(nomifun_api_types::voice::VoiceBindingSnapshot {
            session_id: "session-b".into(), binding_id: "binding-b".into(), conversation_id: "conversation-b".into(),
            workspace: None, memory_owner_companion_id: None,
        });
        let turn = |session: Option<&str>, id: &str| nomifun_api_types::voice::VoiceConversationTurn {
            session_id: session.map(str::to_owned), turn_id: id.into(), user_text: Some(id.into()), ..Default::default()
        };
        manager.project(VoiceSessionSnapshot {
            instance_id: "daemon".into(), seq: 1, session_id: Some("session-b".into()), turn_id: Some("valid".into()),
            conversation: vec![turn(None, "missing"), turn(Some("session-a"), "foreign"), turn(Some("session-b"), "valid")],
            ..Default::default()
        }, 0);
        let snapshot = manager.snapshot();
        assert_eq!(snapshot.conversation.len(), 1);
        assert_eq!(snapshot.conversation[0].turn_id, "valid");
        assert_eq!(snapshot.conversation[0].session_id.as_deref(), Some("session-b"));
    }

    #[test]
    fn automatic_daemon_restart_is_bounded_and_resets_after_stable_runtime() {
        let dir = tempfile::tempdir().unwrap();
        let manager = VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        );
        let base = tokio::time::Instant::now();
        let seconds = |value: u64| base + std::time::Duration::from_secs(value);

        assert!(matches!(
            manager.restart_decision(seconds(0)),
            RestartDecision::Wait(delay) if delay == std::time::Duration::from_secs(2)
        ));
        assert!(matches!(
            manager.restart_decision(seconds(2)),
            RestartDecision::StartNow
        ));
        assert!(matches!(
            manager.restart_decision(seconds(3)),
            RestartDecision::Wait(delay) if delay == std::time::Duration::from_secs(5)
        ));
        assert!(matches!(
            manager.restart_decision(seconds(8)),
            RestartDecision::StartNow
        ));
        assert!(matches!(
            manager.restart_decision(seconds(9)),
            RestartDecision::Wait(delay) if delay == std::time::Duration::from_secs(15)
        ));
        assert!(matches!(
            manager.restart_decision(seconds(24)),
            RestartDecision::StartNow
        ));
        assert!(matches!(
            manager.restart_decision(seconds(25)),
            RestartDecision::Exhausted
        ));

        manager.mark_daemon_running(seconds(30));
        manager.mark_daemon_running(seconds(91));
        assert!(matches!(
            manager.restart_decision(seconds(92)),
            RestartDecision::Wait(delay) if delay == std::time::Duration::from_secs(2)
        ));
    }

    #[tokio::test]
    async fn self_check_attributes_capture_tts_playback_and_llm_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = populate_checked_suite(dir.path());
        let models = Arc::new(nomifun_voice_models::ModelManager::with_config(
            dir.path(),
            Vec::new(),
            manifest,
        ));
        let (origin, server) = tts_fixture(
            200,
            r#"{"ok":true,"provider":"fixture","online":true,"playback_ok":true,"languages":[{"language":"zh-CN","voice":"zh","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50},{"language":"en-US","voice":"en","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50}]}"#,
        )
        .await;
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(models, dir.path())
                .with_replay_daemon(origin)
                .with_dialogue(Arc::new(FixtureCompleter {
                    result: Ok("OK".into()),
                })),
        );
        capability_snapshot(&manager, nomifun_common::now_ms());

        let report = manager.run_self_check(true).await.unwrap();
        assert_eq!(
            report.capture.evidence, "recent_pcm",
            "capture must be attributed to fresh PCM"
        );
        assert_eq!(report.tts.evidence, "tts_decoded");
        assert_eq!(report.playback.evidence, "sink_consumed");
        assert_eq!(report.llm.evidence, "live_response");
        assert!(report.can_start_full_call, "all attributed capabilities are ready");
        assert!(manager.current_capabilities().await.is_some());
        server.abort();
    }

    #[tokio::test]
    async fn self_check_rejects_engine_evidence_from_another_request_instance_or_revision() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = populate_checked_suite(dir.path());
        let models = Arc::new(nomifun_voice_models::ModelManager::with_config(
            dir.path(),
            Vec::new(),
            manifest,
        ));
        let body = r#"{"ok":true,"provider":"fixture","online":true,"playback_ok":true,"languages":[{"language":"zh-CN","voice":"zh","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50},{"language":"en-US","voice":"en","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50}]}"#;
        let (origin, server) = tts_fixture_with_stale_engine(200, body).await;
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(models, dir.path())
                .with_replay_daemon(origin)
                .with_dialogue(Arc::new(FixtureCompleter { result: Ok("OK".into()) })),
        );
        capability_snapshot(&manager, nomifun_common::now_ms());
        let report = manager.run_self_check(false).await.unwrap();
        assert_eq!(report.kws.reason_code.as_deref(), Some("engine_response_stale"));
        assert_eq!(report.vad.reason_code.as_deref(), Some("engine_response_stale"));
        assert_eq!(report.asr.reason_code.as_deref(), Some("engine_response_stale"));
        assert!(!report.can_wake);
        assert!(!report.can_start_full_call);
        server.abort();
    }

    #[tokio::test]
    async fn self_check_accepts_distinct_daemon_id_and_invalidates_cache_on_daemon_source_restart() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = populate_checked_suite(dir.path());
        let models = Arc::new(nomifun_voice_models::ModelManager::with_config(
            dir.path(),
            Vec::new(),
            manifest,
        ));
        let body = r#"{"ok":true,"provider":"fixture","online":true,"playback_ok":true,"languages":[{"language":"zh-CN","voice":"zh","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50},{"language":"en-US","voice":"en","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50}]}"#;
        let (origin, server) = tts_fixture(200, body).await;
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(models, dir.path())
                .with_replay_daemon(origin)
                .with_dialogue(Arc::new(FixtureCompleter {
                    result: Ok("OK".into()),
                })),
        );
        capability_snapshot(&manager, nomifun_common::now_ms());

        // 1. Host projection ID is distinct from daemon source ID ("fixture-daemon-instance-1")
        assert_ne!(manager.snapshot().instance_id, "fixture-daemon-instance-1");
        let report = manager.run_self_check(true).await.unwrap();
        assert!(report.can_start_full_call, "distinct daemon instance is accepted");
        assert!(manager.current_capabilities().await.is_some());

        // 2. Daemon restarts and acquires a new instance ID: previous capabilities cache must be invalidated
        *manager.source.write().unwrap() = Some(("restarted-daemon-instance-2".into(), 1));
        assert!(manager.current_capabilities().await.is_none(), "cache must invalidate when daemon source changes");
        server.abort();
    }

    #[tokio::test]
    async fn self_check_rejects_stale_worker_epoch_even_if_daemon_instance_id_matches() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = populate_checked_suite(dir.path());
        let models = Arc::new(nomifun_voice_models::ModelManager::with_config(
            dir.path(),
            Vec::new(),
            manifest,
        ));
        let body = r#"{"ok":true,"provider":"fixture","online":true,"playback_ok":true,"languages":[{"language":"zh-CN","voice":"zh","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50},{"language":"en-US","voice":"en","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50}]}"#;
        let (origin, server) = tts_fixture(200, body).await;
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(models, dir.path())
                .with_replay_daemon(origin)
                .with_dialogue(Arc::new(FixtureCompleter {
                    result: Ok("OK".into()),
                })),
        );
        capability_snapshot(&manager, nomifun_common::now_ms());
        manager.snapshot.write().unwrap().worker_epoch = Some(7);

        // 1. Initially matches epoch 7: report succeeds and is cached.
        let report = manager.run_self_check(true).await.unwrap();
        assert!(report.can_start_full_call, "epoch 7 is accepted");
        assert!(manager.current_capabilities().await.is_some(), "capabilities cached");

        // 2. Underlying worker is replaced/reloaded -> worker_epoch bumps to 8 on the host side,
        // while daemon ID remains "fixture-daemon-instance-1".
        manager.snapshot.write().unwrap().worker_epoch = Some(8);

        // 3. Cache signature must immediately invalidate because worker epoch changed.
        assert!(manager.current_capabilities().await.is_none(), "cache must invalidate when worker_epoch changes");

        // 4. If daemon responds with stale worker_epoch (7 < 8), self check must reject it as engine_response_stale.
        let stale_report = manager.run_self_check(false).await.unwrap();
        assert_eq!(stale_report.kws.reason_code.as_deref(), Some("engine_response_stale"));
        assert_eq!(stale_report.vad.reason_code.as_deref(), Some("engine_response_stale"));
        assert_eq!(stale_report.asr.reason_code.as_deref(), Some("engine_response_stale"));
        assert!(!stale_report.can_start_full_call);
        assert!(!stale_report.can_wake);

        server.abort();
    }

    #[tokio::test]
    async fn prepare_without_models_fails_before_starting_or_creating_a_session() {
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        manager.write_autostart_config(true).unwrap();
        let app = voice_companion_routes(manager.clone());
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/voice/companion/prepare")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"include_llm":false}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);
        let body = axum::body::to_bytes(response.into_body(), 16_384).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "models_not_installed");
        assert!(manager.child.lock().await.is_none());
        assert!(manager.snapshot().session_id.is_none());
    }

    #[tokio::test]
    async fn prepare_never_enables_a_disabled_companion() {
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        assert!(!manager.read_autostart_config());
        let response = voice_companion_routes(manager.clone())
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/voice/companion/prepare")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"include_llm":false}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);
        let body = axum::body::to_bytes(response.into_body(), 16_384).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "voice_disabled");
        assert!(!manager.read_autostart_config(), "prepare must not write autostart");
        assert!(manager.child.lock().await.is_none(), "prepare must not start a disabled companion");
    }

    #[tokio::test]
    async fn self_check_routes_share_the_stored_report_with_status() {
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let manifest = populate_checked_suite(dir.path());
        let models = Arc::new(nomifun_voice_models::ModelManager::with_config(
            dir.path(),
            Vec::new(),
            manifest,
        ));
        let (origin, server) = tts_fixture(
            200,
            r#"{"ok":true,"provider":"fixture","online":true,"playback_ok":true,"languages":[{"language":"zh-CN","voice":"zh","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50},{"language":"en-US","voice":"en","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50}]}"#,
        )
        .await;
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(models, dir.path())
                .with_replay_daemon(origin)
                .with_dialogue(Arc::new(FixtureCompleter {
                    result: Ok("OK".into()),
                })),
        );
        capability_snapshot(&manager, nomifun_common::now_ms());
        let app = voice_companion_routes(manager.clone());

        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/voice/companion/self-check")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"include_llm":true}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 65_536).await.unwrap();
        let report: nomifun_api_types::voice::VoiceCapabilityReport =
            serde_json::from_slice(&body).unwrap();
        assert!(report.can_start_full_call);

        let capabilities = app
            .oneshot(
                axum::http::Request::builder()
                    .method("GET")
                    .uri("/api/voice/companion/capabilities")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(capabilities.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(capabilities.into_body(), 65_536).await.unwrap();
        let stored: Option<nomifun_api_types::voice::VoiceCapabilityReport> =
            serde_json::from_slice(&body).unwrap();
        assert!(stored.is_some_and(|value| value.can_start_full_call));
        // The check must not open a session, task, memory or wake projection.
        assert!(manager.snapshot().session_id.is_none());
        assert!(manager.snapshot().task.is_none());
        assert!(manager.snapshot().queue.is_empty());
        server.abort();
    }

    #[tokio::test]
    async fn self_check_reports_stale_capture_and_classified_failures() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = populate_checked_suite(dir.path());
        let models = Arc::new(nomifun_voice_models::ModelManager::with_config(
            dir.path(),
            Vec::new(),
            manifest,
        ));
        let (origin, server) = tts_fixture(200, "not-json").await;
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(models, dir.path())
                .with_replay_daemon(origin)
                .with_dialogue(Arc::new(FixtureCompleter {
                    result: Err("401 unauthorized".into()),
                })),
        );
        capability_snapshot(&manager, nomifun_common::now_ms() - 5_000);

        let report = manager.run_self_check(true).await.unwrap();
        assert_eq!(report.capture.reason_code.as_deref(), Some("capture_no_recent_pcm"));
        assert!(!report.can_start_full_call);
        assert_eq!(report.tts.reason_code.as_deref(), Some("tts_bad_response"));
        assert_eq!(report.llm.reason_code.as_deref(), Some("llm_auth_failed"));
        assert!(report.degraded_text_only == false);
        server.abort();

        let (origin, server) = tts_fixture(429, "{}").await;
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(
                Arc::new(nomifun_voice_models::ModelManager::with_config(
                    dir.path(),
                    Vec::new(),
                    std::collections::HashMap::new(),
                )),
                dir.path(),
            )
            .with_replay_daemon(origin)
            .with_dialogue(Arc::new(FixtureCompleter {
                result: Err("429 too many requests".into()),
            })),
        );
        let report = manager.run_self_check(true).await.unwrap();
        assert_eq!(report.llm.reason_code.as_deref(), Some("llm_rate_limited"));
        assert_eq!(report.tts.reason_code.as_deref(), Some("tts_http_429"));
        server.abort();
    }

    #[tokio::test]
    async fn failed_engine_inference_blocks_full_call_and_wake() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = populate_checked_suite(dir.path());
        let models = Arc::new(nomifun_voice_models::ModelManager::with_config(
            dir.path(),
            Vec::new(),
            manifest,
        ));
        let (origin, server) = tts_fixture_with_failed_engine(
            200,
            r#"{"ok":true,"provider":"fixture","online":true,"playback_ok":true,"languages":[{"language":"zh-CN","voice":"zh","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50},{"language":"en-US","voice":"en","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50}]}"#,
        )
        .await;
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(models, dir.path())
                .with_replay_daemon(origin)
                .with_dialogue(Arc::new(FixtureCompleter {
                    result: Ok("OK".into()),
                })),
        );
        capability_snapshot(&manager, nomifun_common::now_ms());
        let report = manager.run_self_check(false).await.unwrap();
        assert!(!report.can_start_full_call);
        // Wake requires the same verified local chain; engine_loaded alone is
        // not enough.
        assert!(!report.can_wake);
        assert!(!report.can_transcribe);
        assert_eq!(report.asr.reason_code.as_deref(), Some("asr_inference_failed"));
        server.abort();
    }

    #[tokio::test]
    async fn loaded_engines_without_inference_evidence_cannot_authorize_call_or_wake() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = populate_checked_suite(dir.path());
        let models = Arc::new(nomifun_voice_models::ModelManager::with_config(
            dir.path(),
            Vec::new(),
            manifest,
        ));
        let (origin, server) = tts_fixture_without_engine(
            200,
            r#"{"ok":true,"provider":"fixture","online":true,"playback_ok":true,"languages":[{"language":"zh-CN","voice":"zh","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50},{"language":"en-US","voice":"en","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50}]}"#,
        )
        .await;
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(models, dir.path())
                .with_replay_daemon(origin)
                .with_dialogue(Arc::new(FixtureCompleter {
                    result: Ok("OK".into()),
                })),
        );
        capability_snapshot(&manager, nomifun_common::now_ms());
        let report = manager.run_self_check(false).await.unwrap();
        assert!(!report.can_start_full_call);
        assert!(!report.can_wake);
        assert!(!report.can_transcribe);
        assert_eq!(
            report.asr.reason_code.as_deref(),
            Some("engine_check_unreachable")
        );
        server.abort();
    }

    #[tokio::test]
    async fn host_rejects_a_top_level_ok_with_silent_or_incomplete_tts_proof() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = populate_checked_suite(dir.path());
        let models = Arc::new(nomifun_voice_models::ModelManager::with_config(
            dir.path(),
            Vec::new(),
            manifest,
        ));
        let (origin, server) = tts_fixture(
            200,
            r#"{"ok":true,"provider":"fixture","online":true,"playback_ok":true,"languages":[{"language":"zh-CN","voice":"zh","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":0,"peak_milli":0}]}"#,
        )
        .await;
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(models, dir.path())
                .with_replay_daemon(origin)
                .with_dialogue(Arc::new(FixtureCompleter {
                    result: Ok("OK".into()),
                })),
        );
        capability_snapshot(&manager, nomifun_common::now_ms());
        let report = manager.run_self_check(true).await.unwrap();
        assert!(!report.can_start_full_call);
        assert_eq!(
            report.tts.reason_code.as_deref(),
            Some("tts_inconsistent_response")
        );
        assert_eq!(
            report.playback.reason_code.as_deref(),
            Some("playback_without_valid_synthesis")
        );
        server.abort();
    }

    #[tokio::test]
    async fn stale_reader_cannot_clear_a_newer_published_report() {
        let dir = tempfile::tempdir().unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(
                Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
                dir.path(),
            )
            .with_dialogue(Arc::new(BarrierCompleter {
                entered: entered.clone(),
                release: release.clone(),
                revision: "old".into(),
            })),
        );
        capability_snapshot(&manager, nomifun_common::now_ms());
        manager.self_check_attempt.store(1, Ordering::Release);
        let old_report = nomifun_api_types::voice::VoiceCapabilityReport {
            generation: manager.generation.load(Ordering::Acquire),
            can_start_full_call: true,
            ..Default::default()
        };
        *manager.capabilities.write().unwrap() = Some(StoredCapabilities {
            report: old_report,
            attempt: 1,
            expires_at_ms: nomifun_common::now_ms() + 60_000,
            signature: "stale-signature".into(),
        });

        let reader = manager.clone();
        let read = tokio::spawn(async move { reader.current_capabilities().await });
        tokio::time::timeout(std::time::Duration::from_secs(1), entered.notified())
            .await
            .expect("old read must enter configuration_revision");

        let new_report = nomifun_api_types::voice::VoiceCapabilityReport {
            generation: manager.generation.load(Ordering::Acquire),
            can_start_full_call: true,
            ..Default::default()
        };
        *manager.capabilities.write().unwrap() = Some(StoredCapabilities {
            report: new_report,
            attempt: 2,
            expires_at_ms: nomifun_common::now_ms() + 60_000,
            signature: "new-signature".into(),
        });
        release.notify_waiters();

        let result = read.await.unwrap();
        assert!(result.is_none(), "the stale read must not return its old report");
        let cached = manager.capabilities.read().unwrap().clone();
        assert_eq!(
            cached.as_ref().map(|stored| stored.attempt),
            Some(2),
            "an old read must not clear a newer report"
        );
    }

    #[tokio::test]
    async fn capability_report_is_invalidated_by_fresh_pcm_and_config_revision() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = populate_checked_suite(dir.path());
        let models = Arc::new(nomifun_voice_models::ModelManager::with_config(
            dir.path(),
            Vec::new(),
            manifest,
        ));
        let (origin, server) = tts_fixture(
            200,
            r#"{"ok":true,"provider":"fixture","online":true,"playback_ok":true,"languages":[{"language":"zh-CN","voice":"zh","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50},{"language":"en-US","voice":"en","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50}]}"#,
        )
        .await;
        let revision = Arc::new(std::sync::Mutex::new("rev-1".to_string()));
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(models, dir.path())
                .with_replay_daemon(origin)
                .with_dialogue(Arc::new(ConfigurableCompleter {
                    revision: revision.clone(),
                })),
        );
        capability_snapshot(&manager, nomifun_common::now_ms());
        assert!(manager.run_self_check(true).await.unwrap().can_start_full_call);
        assert!(manager.current_capabilities().await.is_some());

        manager
            .snapshot
            .write()
            .unwrap()
            .audio
            .as_mut()
            .unwrap()
            .last_frame_at_ms = Some(nomifun_common::now_ms() - 5_000);
        assert!(manager.current_capabilities().await.is_none());

        capability_snapshot(&manager, nomifun_common::now_ms());
        assert!(manager.run_self_check(true).await.unwrap().can_start_full_call);
        *revision.lock().unwrap() = "rev-2".to_string();
        assert!(manager.current_capabilities().await.is_none());
        server.abort();
    }

    #[tokio::test]
    async fn readiness_check_atomically_blocks_new_calls_and_model_writes() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        manager.self_check_active.store(true, Ordering::Release);
        let call = manager
            .start_call(VoiceCallStartRequest { locale: None,
                request_id: "during-check".into(),
                target: VoiceCallTarget::DefaultWorkspace,
                session_mode: None,
            })
            .await
            .expect_err("a new call must be rejected during a readiness check");
        assert_eq!(call.0, axum::http::StatusCode::CONFLICT);
        assert!(call.1.contains("voice_self_check_in_progress"));
        assert_eq!(
            manager.begin_model_write().await.unwrap_err(),
            "voice_self_check_in_progress"
        );
        manager.self_check_active.store(false, Ordering::Release);
    }

    #[tokio::test]
    async fn cancelled_readiness_check_releases_admission_and_cancels_daemon_work() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = populate_checked_suite(dir.path());
        let models = Arc::new(nomifun_voice_models::ModelManager::with_config(
            dir.path(),
            Vec::new(),
            manifest,
        ));
        let cancels = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let cancels_for_route = cancels.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new()
            .route(
                "/api/voice/diagnostics/tts-self-check",
                axum::routing::post(|| async {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    axum::Json(serde_json::json!({
                        "ok": true, "provider": "fixture", "online": true, "playback_ok": true,
                        "languages": [
                            {"language":"zh-CN","voice":"zh","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50},
                            {"language":"en-US","voice":"en","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50}
                        ]
                    }))
                }),
            )
            .route(
                "/api/voice/diagnostics/tts-self-check/cancel",
                axum::routing::post(move || {
                    let cancels = cancels_for_route.clone();
                    async move {
                        cancels.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        axum::Json(serde_json::json!({ "cancelled": true }))
                    }
                }),
            );
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(models, dir.path())
                .with_replay_daemon(origin)
                .with_dialogue(Arc::new(ConfigurableCompleter {
                    revision: Arc::new(std::sync::Mutex::new("rev".into())),
                })),
        );
        capability_snapshot(&manager, nomifun_common::now_ms());
        let check_manager = manager.clone();
        let task = tokio::spawn(async move { check_manager.run_self_check(false).await });
        for _ in 0..50 {
            if manager.self_check_active.load(Ordering::Acquire) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        manager.cancel_self_check();
        let _ = task.await;
        for _ in 0..50 {
            if !manager.self_check_active.load(Ordering::Acquire) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(!manager.self_check_active.load(Ordering::Acquire));
        for _ in 0..50 {
            if cancels.load(std::sync::atomic::Ordering::SeqCst) > 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(cancels.load(std::sync::atomic::Ordering::SeqCst) > 0);
        server.abort();
    }

    #[tokio::test]
    async fn concurrent_self_check_does_not_queue_another_check() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        let guard = manager.self_check_lock.lock().await;
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            manager.run_self_check(false),
        ).await;
        drop(guard);
        assert_eq!(result.expect("overlapping check must be rejected promptly").unwrap_err(), "self_check_in_progress");
    }

    #[test]
    fn wake_rejection_is_terminal_only_for_known_pre_session_failures() {
        use axum::http::StatusCode;
        assert!(wake_rejection_precedes_session(StatusCode::SERVICE_UNAVAILABLE, r#"{"success":false,"error":"models_unavailable"}"#));
        assert!(wake_rejection_precedes_session(StatusCode::SERVICE_UNAVAILABLE, r#"{"success":false,"error":"microphone_unavailable"}"#));
        assert!(wake_rejection_precedes_session(StatusCode::FORBIDDEN, r#"{"success":false,"error":"pairing_required"}"#));
        assert!(!wake_rejection_precedes_session(StatusCode::CONFLICT, r#"{"success":false,"error":"session_busy"}"#));
        assert!(!wake_rejection_precedes_session(StatusCode::INTERNAL_SERVER_ERROR, r#"{"success":false,"error":"models_unavailable"}"#));
        assert!(!wake_rejection_precedes_session(StatusCode::SERVICE_UNAVAILABLE, r#"{"error":"models_unavailable"}"#));
        assert!(!wake_rejection_precedes_session(StatusCode::SERVICE_UNAVAILABLE, "unavailable"));
    }

    #[tokio::test]
    async fn capabilities_expire_with_generation_and_ttl_without_side_effects() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        let report = nomifun_api_types::voice::VoiceCapabilityReport {
            generation: manager.generation.load(Ordering::Acquire),
            can_start_full_call: true,
            ..Default::default()
        };
        *manager.capabilities.write().unwrap() = Some(StoredCapabilities {
            report,
            attempt: 1,
            expires_at_ms: nomifun_common::now_ms() - 1,
            signature: String::new(),
        });
        assert!(
            manager.current_capabilities().await.is_none(),
            "an expired report must not authorize a call"
        );

        let report = nomifun_api_types::voice::VoiceCapabilityReport {
            generation: manager.generation.load(Ordering::Acquire),
            can_start_full_call: true,
            ..Default::default()
        };
        *manager.capabilities.write().unwrap() = Some(StoredCapabilities {
            report,
            attempt: 2,
            expires_at_ms: nomifun_common::now_ms() + 60_000,
            signature: manager.capability_signature().await,
        });
        manager.generation.fetch_add(1, Ordering::AcqRel);
        assert!(
            manager.current_capabilities().await.is_none(),
            "a daemon generation change must invalidate the previous evidence"
        );
    }

    #[tokio::test]
    async fn a_connected_but_incomplete_suite_rejects_before_any_placeholder() {
        struct GateRecorder {
            resolutions: std::sync::atomic::AtomicUsize,
        }
        #[async_trait::async_trait]
        impl VoiceDialogueRecorder for GateRecorder {
            async fn preflight(&self, _: &VoiceDialogueRequest) -> Result<VoiceDialoguePreflight, VoiceDialogueRecorderError> {
                unreachable!("the admission gate must run before preflight")
            }
            async fn record_pair(&self, _: &str, _: &VoiceDialogueRequest, _: &str) -> Result<(), VoiceDialogueRecorderError> {
                Ok(())
            }
            async fn resolve_call_target_with_locale(&self, _: &VoiceCallTarget, _: Option<&str>, _: Option<&str>, _: Option<&str>) -> Result<(String, Option<String>), VoiceDialogueRecorderError> {
                self.resolutions.fetch_add(1, Ordering::SeqCst);
                Ok(("conversation".into(), None))
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let recorder = Arc::new(GateRecorder { resolutions: std::sync::atomic::AtomicUsize::new(0) });
        let manager = VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        )
        .with_recorder(recorder.clone());
        let builder = if cfg!(windows) {
            let mut builder = nomi_process_runtime::ChildProcessBuilder::new("cmd.exe");
            builder.args(["/c", "ping", "-n", "10", "127.0.0.1"]);
            builder
        } else {
            let mut builder = nomi_process_runtime::ChildProcessBuilder::new("sleep");
            builder.arg("10");
            builder
        };
        if let Some(child) = builder.spawn_managed().ok() {
            *manager.child.lock().await = Some(child);
        }
        {
            let mut snapshot = manager.snapshot.write().unwrap();
            snapshot.enabled = true;
            snapshot.connection = VoiceConnectionState::Connected;
            snapshot.acoustic_ready = true;
            snapshot.dialogue_ready = true;
            snapshot.playback_ready = false;
        }
        let error = manager
            .start_call(VoiceCallStartRequest { locale: None,
                request_id: "gate-1".into(),
                target: VoiceCallTarget::DefaultWorkspace,
                session_mode: None,
            })
            .await
            .expect_err("a connected suite without playback must reject a new full call");
        assert_eq!(error.0, axum::http::StatusCode::CONFLICT);
        assert!(error.1.contains("voice_suite_not_ready"));
        assert_eq!(
            recorder.resolutions.load(Ordering::SeqCst),
            0,
            "no Conversation placeholder may be created before the gate passes"
        );
        assert!(recorder.current_call_reservation().is_none());
    }

    #[tokio::test]
    async fn model_write_rejects_an_active_call_without_touching_the_daemon() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        {
            let mut snapshot = manager.snapshot.write().unwrap();
            snapshot.enabled = true;
            snapshot.connection = VoiceConnectionState::Connected;
            snapshot.session_id = Some("active-session".into());
            snapshot.phase = VoiceSessionPhase::Listening;
        }
        assert_eq!(
            manager.begin_model_write().await.unwrap_err(),
            "voice_suite_in_use"
        );
        assert!(!manager.resource_write_active.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn http_model_write_is_rejected_while_a_call_is_active() {
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        {
            let mut snapshot = manager.snapshot.write().unwrap();
            snapshot.enabled = true;
            snapshot.connection = VoiceConnectionState::Connected;
            snapshot.session_id = Some("active-session".into());
            snapshot.phase = VoiceSessionPhase::Listening;
        }
        let app = nomifun_voice_models::voice_model_routes(manager.models_manager.clone()).layer(
            axum::middleware::from_fn_with_state(
                manager.clone(),
                model_write_guard_middleware,
            ),
        );
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/voice/models/install")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);
        let body = axum::body::to_bytes(response.into_body(), 16_384).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "voice_suite_in_use");
        assert!(json["error"].as_str().unwrap().contains("voice_suite_in_use"));
    }

    #[tokio::test]
    async fn model_write_stops_an_idle_daemon_and_blocks_new_calls() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        let builder = if cfg!(windows) {
            let mut builder = nomi_process_runtime::ChildProcessBuilder::new("cmd.exe");
            builder.args(["/c", "ping", "-n", "10", "127.0.0.1"]);
            builder
        } else {
            let mut builder = nomi_process_runtime::ChildProcessBuilder::new("sleep");
            builder.arg("10");
            builder
        };
        *manager.child.lock().await = builder.spawn_managed().ok();
        {
            let mut snapshot = manager.snapshot.write().unwrap();
            snapshot.enabled = true;
            snapshot.connection = VoiceConnectionState::Connected;
            snapshot.acoustic_ready = true;
            snapshot.dialogue_ready = true;
            snapshot.playback_ready = true;
        snapshot.worker_epoch = Some(7);
        }

        let guard = manager.begin_model_write().await.unwrap();
        assert!(manager.resource_write_active.load(Ordering::Acquire));
        assert!(
            manager.child.lock().await.is_none(),
            "the idle daemon must release its native model handles before the write"
        );
        let error = manager
            .start_call(VoiceCallStartRequest { locale: None,
                request_id: "during-write".into(),
                target: VoiceCallTarget::DefaultWorkspace,
                session_mode: None,
            })
            .await
            .expect_err("a new call must be rejected while models are being written");
        assert_eq!(error.0, axum::http::StatusCode::CONFLICT);
        assert!(error.1.contains("voice_resource_write_in_progress"));
        drop(guard);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(!manager.resource_write_active.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn model_write_does_not_restart_after_the_user_disables_voice() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        manager.write_autostart_config(true).unwrap();
        let guard = manager.begin_model_write().await.unwrap();
        // The user turns the switch off while the write is still running.
        manager.write_autostart_config(false).unwrap();
        drop(guard);
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert!(!manager.resource_write_active.load(Ordering::Acquire));
        assert!(
            manager.child.lock().await.is_none(),
            "a disabled preference must not be undone by the write completion path"
        );
    }

    #[tokio::test]
    async fn voice_foreign_result_and_approval_are_rejected_before_notification_side_effects() {
        let dir = tempfile::tempdir().unwrap();
        let manager = VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(dir.path())), dir.path());
        {
            let mut snapshot = manager.snapshot.write().unwrap();
            snapshot.enabled = true;
            snapshot.connection = VoiceConnectionState::Connected;
            snapshot.session_id = Some("session-b".into());
            snapshot.binding = Some(nomifun_api_types::voice::VoiceBindingSnapshot {
                session_id: "session-b".into(), binding_id: "binding-b".into(), conversation_id: "conversation-b".into(),
                workspace: None, memory_owner_companion_id: None,
            });
        }
        let task = VoiceTaskSnapshot {
            command_id: "command-a".into(), session_id: "session-a".into(), acoustic_turn_id: "turn-a".into(),
            conversation_id: Some("conversation-a".into()), message_id: Some("message-a".into()), runtime_turn_id: None,
            status: VoiceTaskStatus::Completed, result_ok: Some(true), result_text: Some("A private result".into()), error_code: None,
        };
        assert!(manager.notify_task_result(&task, "A private goal").await.is_ok());
        let approval = VoiceToolConfirmation {
            approval_id: "approval-a".into(), msg_id: "message-a".into(), call_id: "call-a".into(), tool_name: "Write".into(),
            arguments: serde_json::json!({"path":"a.txt"}), expires_at_ms: i64::MAX,
        };
        assert!(manager.notify_confirmation(&task, &approval).await.is_ok());
        assert!(manager.prompted_approvals.lock().unwrap().is_empty());
        assert_eq!(manager.snapshot().session_id.as_deref(), Some("session-b"));
        assert!(manager.snapshot().assistant_text.is_none());
    }

    #[test]
    fn audio_recovery_retries_devices_with_backoff_without_restarting_model_failures_or_speech() {
        use nomifun_api_types::voice::{VoiceAudioDiagnostics,VoiceSessionEndReason};
        let mut retry=AudioRecovery::default();
        let mut state=VoiceSessionSnapshot {enabled:true,connection:VoiceConnectionState::Connected,
            phase:VoiceSessionPhase::Idle,acoustic_ready:false,playback_ready:true,
            audio:Some(VoiceAudioDiagnostics {capture_state:"closed".into(),..Default::default()}),..Default::default()};
        assert!(!retry.due(&state,1_000));
        assert!(!retry.due(&state,2_999));
        assert!(retry.due(&state,3_000));
        assert!(!retry.due(&state,6_999));
        assert!(retry.due(&state,7_000));
        state.phase=VoiceSessionPhase::Speaking;
        assert!(!retry.due(&state,20_000),"do not interrupt foreground speech");
        state.phase=VoiceSessionPhase::Idle;state.ended_reason=Some(VoiceSessionEndReason::UserStop);
        assert!(!retry.due(&state,20_000),"explicit stop must not be undone by recovery");
        state.ended_reason=None;state.enabled=false;
        assert!(!retry.due(&state,20_000));
        state.enabled=true;state.acoustic_ready=true;state.audio.as_mut().unwrap().capture_state="receiving".into();
        state.error=Some(VoiceSessionError {stage:"dialogue".into(),code:"model_network_failed".into()});
        assert!(!retry.due(&state,20_000),"model/TTS network failures are not device failures");
        state.playback_ready=false;
        state.audio.as_mut().unwrap().speech_detected=true;
        assert!(!retry.due(&state,21_000),"idle input speech must also be respected");
        state.audio.as_mut().unwrap().speech_detected=false;
        assert!(!retry.due(&state,21_000));
        assert!(retry.due(&state,23_000),"a disconnected speaker must also be reopened");
        state.playback_ready=true;state.audio.as_mut().unwrap().capture_state="no_pcm".into();
        state.audio.as_mut().unwrap().last_frame_at_ms=Some(24_000);
        assert!(!retry.due(&state,25_000),"brief capture gaps must not cause process churn");
        assert!(!retry.due(&state,30_000));
        assert!(retry.due(&state,32_000));
    }

    #[tokio::test]
    async fn automatic_audio_restart_cannot_revoke_host_shutdown_or_disable_intent() {
        let dir=tempfile::tempdir().unwrap();
        let manager=Arc::new(VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),dir.path()));
        manager.write_autostart_config(true).unwrap();
        manager.stop().await.unwrap();
        assert_eq!(manager.start_child(false).await.unwrap_err(),"voice_stopped");
        assert!(manager.shutdown_requested.load(Ordering::Acquire));
        manager.shutdown_requested.store(false,Ordering::Release);
        manager.write_autostart_config(false).unwrap();
        assert_eq!(manager.start_child(false).await.unwrap_err(),"voice_stopped");
        assert!(!manager.status().await.running);
    }

    #[test]
    fn audio_recovery_keeps_readable_results_but_rejects_old_session_controls() {
        let dir=tempfile::tempdir().unwrap();
        let manager=VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),dir.path());
        manager.write_autostart_config(true).unwrap();
        let old=VoiceSessionSnapshot {instance_id:"old-daemon".into(),seq:1,session_id:Some("old-session".into()),turn_id:Some("old-turn".into()),
            user_text:Some("读取文件".into()),assistant_text:Some("文件里的真实结果".into()),
            conversation:vec![nomifun_api_types::voice::VoiceConversationTurn {session_id:Some("old-session".into()),turn_id:"old-turn".into(),user_text:Some("读取文件".into()),assistant_text:Some("文件里的真实结果".into()),..Default::default()}],..Default::default()};
        manager.project(old,0);
        let old_scope=VoiceSessionControlRequest {instance_id:manager.snapshot().instance_id,session_id:Some("old-session".into()),turn_id:Some("old-turn".into())};
        manager.generation.store(1,Ordering::Release);
        for seq in [1,2] {
            manager.project(VoiceSessionSnapshot {instance_id:"recovered-daemon".into(),seq,acoustic_ready:true,playback_ready:true,..Default::default()},1);
            let fresh=manager.snapshot();
            assert_eq!(fresh.assistant_text.as_deref(),Some("文件里的真实结果"),"recovery must preserve readable text through subsequent audio snapshots");
            assert_eq!(fresh.conversation.len(),1);
            assert!(!old_scope.matches(&fresh),"preserved text cannot restore old control authority");
            assert!(fresh.session_id.is_none() && fresh.turn_id.is_none());
        }
        manager.project(VoiceSessionSnapshot {instance_id:"recovered-daemon".into(),seq:3,session_id:Some("new-session".into()),phase:VoiceSessionPhase::Listening,..Default::default()},1);
        assert!(manager.snapshot().conversation.is_empty());
        assert!(manager.snapshot().assistant_text.is_none());
    }

    #[tokio::test]
    async fn execution_settings_report_unconfigured_without_initializing_runtime_or_audio() {
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        let response = voice_companion_routes(manager.clone())
            .oneshot(axum::http::Request::builder().uri("/api/voice/companion/execution").body(axum::body::Body::empty()).unwrap())
            .await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 16_384).await.unwrap();
        let settings: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(settings["requested"], serde_json::Value::Null);
        assert_eq!(settings["effective"], serde_json::Value::Null);
        assert_eq!(settings["available"], false);
        assert_eq!(settings["reason_code"], "execution_unconfigured");
        assert!(!manager.status().await.running);
        assert!(!dir.path().join("config/voice_companion.json").exists());
    }

    #[tokio::test]
    async fn submitting_a_voice_task_without_a_runtime_reports_unavailable_instead_of_acceptance() {
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())), dir.path(),
        ));
        let request = serde_json::json!({
            "instance_id": manager.snapshot().instance_id,
            "command_id": "not-submitted", "session_id": "session", "turn_id": "turn", "goal": "读取文件"
        });
        let response = voice_companion_routes(manager.clone()).oneshot(axum::http::Request::builder()
            .method("POST").uri("/api/voice/companion/tasks").header("content-type", "application/json")
            .body(axum::body::Body::from(request.to_string())).unwrap()).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::SERVICE_UNAVAILABLE);
        let body = axum::body::to_bytes(response.into_body(), 16_384).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["code"], "runtime_unavailable");
        assert!(body.get("task").is_none());
        assert!(!manager.status().await.running);
    }

    #[tokio::test]
    async fn bv_09_foreground_3s_timeout_returns_unknown_without_cancelling_host_admission() {
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        struct SlowRuntime {
            created: Arc<std::sync::atomic::AtomicBool>,
        }
        #[async_trait::async_trait]
        impl VoiceTaskRuntime for SlowRuntime {
            async fn resolve_configuration(&self, config: &VoiceExecutionConfig) -> Result<VoiceExecutionConfig, String> { Ok(config.clone()) }
            async fn prepare_configuration(&self, config: &VoiceExecutionConfig) -> Result<(VoiceExecutionConfig, Option<nomifun_api_types::ResolvedPresetSnapshot>), String> { Ok((config.clone(), None)) }
            async fn create(&self, _: &str, _: &VoiceExecutionConfig, _: Option<&nomifun_api_types::ResolvedPresetSnapshot>, _: &str) -> Result<String, crate::voice_tasks::TaskMutationError> {
                tokio::time::sleep(std::time::Duration::from_millis(3500)).await;
                self.created.store(true, Ordering::SeqCst);
                Ok("conv-slow".into())
            }
            async fn create_bound(&self, owner: &str, config: &VoiceExecutionConfig, preset: Option<&nomifun_api_types::ResolvedPresetSnapshot>, creation_key: &str, target: Option<&str>) -> Result<String, crate::voice_tasks::TaskMutationError> {
                if let Some(target_id) = target {
                    tokio::time::sleep(std::time::Duration::from_millis(3500)).await;
                    self.created.store(true, Ordering::SeqCst);
                    return Ok(target_id.into());
                }
                self.create(owner, config, preset, creation_key).await
            }
            async fn find_created(&self, _: &str, _: &str) -> Result<Option<String>, String> { Ok(Some("conv-slow".into())) }
            async fn append(&self, _: &str, _: &str, _: &str, _: &str, _: &str) -> Result<(), crate::voice_tasks::TaskMutationError> { Ok(()) }
            async fn send_bound(
                &self,
                owner: &str,
                conversation_id: &str,
                send_key: &str,
                message_id: &str,
                _command_id: &str,
                _display_text: &str,
                goal: &str,
                _context_digest: Option<&str>,
                _frozen_config: Option<&VoiceExecutionConfig>,
                _preset: Option<&nomifun_api_types::ResolvedPresetSnapshot>,
            ) -> Result<crate::voice_tasks::TaskReceipt, crate::voice_tasks::TaskMutationError> {
                let mut receipt = self.send(owner, conversation_id, send_key, goal).await?;
                receipt.message_id = message_id.to_string();
                Ok(receipt)
            }
            async fn send(&self, _: &str, _: &str, _: &str, _: &str) -> Result<crate::voice_tasks::TaskReceipt, crate::voice_tasks::TaskMutationError> {
                Ok(crate::voice_tasks::TaskReceipt {
                    message_id: "msg-slow".into(),
                    runtime_turn_id: None,
                    replayed: false,
                    completed: false,
                    result_ok: None,
                    result_text: None,
                    error_code: None,
                })
            }
            async fn observe(&self, _: &str, _: &str, _: &str) -> Result<Option<crate::voice_tasks::TaskReceipt>, String> { Ok(None) }
            async fn cancel(&self, _: &str, _: &str, _: &str, _: &str) -> Result<(), String> { Ok(()) }
            async fn confirmations(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Vec<VoiceToolConfirmation>, String> { Ok(Vec::new()) }
            async fn confirm(&self, _: &str, _: &str, _: &str, _: &str, _: &VoiceToolConfirmation, _: bool) -> Result<(), String> { Ok(()) }
        }

        struct BoundRecorder(VoiceExecutionConfig);
        #[async_trait::async_trait]
        impl VoiceDialogueRecorder for BoundRecorder {
            async fn task_configuration(&self, command: &nomifun_api_types::voice::VoiceTaskCommand) -> Result<Option<crate::voice_dialogue::FrozenVoiceExecution>, String> {
                assert_eq!(command.session_id, "session-slow");
                Ok(Some(crate::voice_dialogue::FrozenVoiceExecution {config:self.0.clone(),preset:None,conversation_id:Some("conv-slow".into()),message_id:Some("msg-slow".into()),handoff_content:None,display_text:Some(command.goal.clone()),voice_context_digest:None}))
            }
            async fn preflight(&self, _: &VoiceDialogueRequest) -> Result<crate::voice_dialogue::VoiceDialoguePreflight, VoiceDialogueRecorderError> { unreachable!() }
            async fn record_pair(&self, _: &str, _: &VoiceDialogueRequest, _: &str) -> Result<(), VoiceDialogueRecorderError> { unreachable!() }
        }
        let recorder = Arc::new(BoundRecorder(VoiceExecutionConfig {
            workspace:dir.path().to_string_lossy().into_owned(),model:Some(VoiceModelRef {provider_id:"test-p".into(),model:"test-m".into()}),preset_id:None,session_mode:None,
        }));
        let created_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let runtime = Arc::new(SlowRuntime { created: created_flag.clone() });
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())), dir.path(),
        ).with_tasks("test-owner".into(), runtime).with_recorder(recorder));
        manager.write_autostart_config(true).unwrap();

        let generation = manager.generation.load(Ordering::Acquire);
        let instance_id = manager.snapshot().instance_id;
        manager.project(VoiceSessionSnapshot {
            instance_id: "daemon-slow".into(),
            seq: 1,
            enabled: true,
            connection: VoiceConnectionState::Connected,
            phase: VoiceSessionPhase::Deciding,
            session_id: Some("session-slow".into()),
            turn_id: Some("turn-slow".into()),
            ..Default::default()
        }, generation);

        let request = serde_json::json!({
            "instance_id": instance_id,
            "command_id": "command-slow-1",
            "session_id": "session-slow",
            "turn_id": "turn-slow",
            "goal": "执行耗时任务"
        });

        let start = std::time::Instant::now();
        let response = voice_companion_routes(manager.clone()).oneshot(axum::http::Request::builder()
            .method("POST").uri("/api/voice/companion/tasks").header("content-type", "application/json")
            .body(axum::body::Body::from(request.to_string())).unwrap()).await.unwrap();

        let elapsed = start.elapsed();
        assert!(elapsed.as_millis() >= 2800 && elapsed.as_millis() < 4200, "elapsed: {:?}", elapsed);
        assert_eq!(response.status(), axum::http::StatusCode::ACCEPTED);

        let body = axum::body::to_bytes(response.into_body(), 16_384).await.unwrap();
        let delivery: VoiceTaskDelivery = serde_json::from_slice(&body).unwrap();
        assert!(!delivery.freshly_accepted, "timed-out foreground response must not claim freshly accepted");
        assert_eq!(delivery.task.command_id, "command-slow-1");
        assert!(matches!(delivery.task.status, VoiceTaskStatus::Unknown | VoiceTaskStatus::Delivering));

        // The background task continues without cancellation
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        assert!(created_flag.load(Ordering::SeqCst), "host background admission must complete even after foreground timeout");
    }

    #[tokio::test]
    async fn retired_configuration_writes_are_rejected_and_legacy_values_are_ignored() {
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),dir.path()));
        manager.write_config_entry("execution", serde_json::json!({"workspace":dir.path(),"model":{"provider_id":"old","model":"old"}})).unwrap();
        manager.write_config_entry("future", serde_json::json!({"keep":true})).unwrap();
        let saved = std::fs::read(&manager.config_path).unwrap();
        let app = voice_companion_routes(manager.clone());
        for path in ["/api/voice/companion/execution", "/api/voice/companion/config"] {
            let response = app.clone().oneshot(axum::http::Request::builder().method("POST").uri(path).header("content-type","application/json")
                .body(axum::body::Body::from(r#"{"workspace":"relative","dialogue_model":null}"#)).unwrap()).await.unwrap();
            assert_eq!(response.status(),axum::http::StatusCode::METHOD_NOT_ALLOWED);
        }
        let settings = manager.execution_settings().await;
        assert!(settings.requested.is_none() && settings.effective.is_none() && !settings.available);
        assert_eq!(std::fs::read(&manager.config_path).unwrap(),saved);
        assert!(!manager.status().await.running);
    }

    #[tokio::test]
    async fn dialogue_model_configuration_is_available_without_starting_audio() {
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        let response = voice_companion_routes(manager)
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/voice/companion/config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::OK,
            "voice must expose a provider/model reference, not require a second API key"
        );
    }

    #[tokio::test]
    async fn retired_dialogue_override_is_ignored_after_restart_and_preferences_are_preserved() {
        struct Models;
        #[async_trait::async_trait]
        impl VoiceDialogueCompleter for Models {
            async fn resolve_model(&self, choice: Option<&VoiceModelRef>) -> Result<VoiceModelRef,String> {
                assert!(choice.is_none(), "production model resolution must be automatic");
                Ok(VoiceModelRef {provider_id:"catalog".into(),model:"flash".into()})
            }
            async fn complete(&self, _: &str, choice: Option<&VoiceModelRef>) -> Result<String,String> {
                assert!(choice.is_none(), "actual bridge must not send the retired override");
                Ok(r#"{"intent_type":"chat","reply_tts":"hello"}"#.into())
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config/voice_companion.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let old = serde_json::json!({"provider_id":"deleted","model":"deleted"});
        std::fs::write(&path,serde_json::json!({"dialogue_model":old,"future":true,"autostart":true}).to_string()).unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),dir.path()).with_dialogue(Arc::new(Models)));
        let settings = manager.dialogue_settings().await;
        assert!(settings.config.dialogue_model.is_none());
        assert_eq!(settings.effective_model.unwrap().model,"flash");
        let bridge = manager.start_dialogue_bridge().await.unwrap();
        let response = reqwest::Client::builder().no_proxy().build().unwrap().post(&bridge.endpoint).header("x-ai-mate-control-token", &manager.control_token)
            .json(&VoiceDialogueRequest::new("hello")).send().await.unwrap();
        assert!(response.status().is_success(), "actual private completion route must use automatic selection");
        assert!(response.text().await.unwrap().contains("hello"));
        manager.write_autostart_config(false).unwrap();
        let saved:serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["dialogue_model"],old);
        assert_eq!(saved["future"],true);
        assert_eq!(saved["autostart"],false);
    }

    #[test]
    fn test_voice_companion_status_serialization() {
        let status = VoiceCompanionStatus {
            running: true,
            pid: Some(12345),
            models_ready: true,
            autostart: true,
            error: None,
            acoustic_ready: false,
            dialogue_ready: false,
            playback_ready: false,
            task_ready: false,
            capabilities: None,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"running\":true"));
        assert!(json.contains("\"pid\":12345"));
        assert!(json.contains("\"models_ready\":true"));

        let deserialized: VoiceCompanionStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.running, true);
        assert_eq!(deserialized.pid, Some(12345));
    }

    #[tokio::test]
    async fn feedback_patch_preserves_model_autostart_unknown_fields_and_rejects_invalid_volume() {
        let dir = tempfile::tempdir().unwrap();
        let manager = VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        );
        manager.write_autostart_config(true).unwrap();
        let selected = serde_json::json!({"provider_id":"existing","model":"configured"});
        manager
            .write_config_entry("dialogue_model", selected.clone())
            .unwrap();
        manager
            .write_config_entry("future_field", serde_json::json!({"keep":true}))
            .unwrap();
        let first = manager
            .update_feedback(
                serde_json::from_value(serde_json::json!({"volume":23,"speak_replies":false}))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            first.effective.is_none(),
            "saving while stopped cannot claim live application"
        );
        let updated = manager
            .update_feedback(
                serde_json::from_value(serde_json::json!({"mode":"essential"})).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(updated.requested.volume, 23);
        assert!(!updated.requested.speak_replies);
        assert_eq!(
            updated.requested.mode,
            nomifun_api_types::voice::VoiceFeedbackMode::Essential
        );
        let before = std::fs::read(&manager.config_path).unwrap();
        assert!(
            manager
                .update_feedback(serde_json::from_value(serde_json::json!({"volume":101})).unwrap())
                .await
                .is_err()
        );
        assert_eq!(std::fs::read(&manager.config_path).unwrap(), before);
        let saved: serde_json::Value = serde_json::from_slice(&before).unwrap();
        assert_eq!(saved["dialogue_model"], selected);
        assert_eq!(saved["future_field"]["keep"], true);
        assert_eq!(saved["autostart"], true);
        let reloaded = VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        );
        assert_eq!(reloaded.requested_feedback().unwrap(), updated.requested);
    }

    #[tokio::test]
    async fn toggle_waits_for_the_prepare_lifecycle_lock() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        let guard = manager.lifecycle_lock.lock().await;
        let toggle = {
            let manager = manager.clone();
            tokio::spawn(async move { manager.toggle(false).await })
        };
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(!toggle.is_finished(), "toggle must wait for prepare's lifecycle lock");
        drop(guard);
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), toggle)
            .await
            .expect("toggle must resume after prepare releases the lock")
            .expect("toggle task must not panic");
        assert!(result.is_ok());
        assert!(!manager.read_autostart_config());
    }

    #[test]
    fn test_autostart_config_roundtrip() {
        let temp_dir = tempfile::tempdir().unwrap();
        let models_mgr = Arc::new(nomifun_voice_models::ModelManager::new(
            temp_dir.path().to_path_buf(),
        ));
        let manager = VoiceCompanionProcessManager::new(models_mgr, temp_dir.path());

        assert_eq!(manager.read_autostart_config(), false);
        manager.write_autostart_config(true).unwrap();
        assert_eq!(manager.read_autostart_config(), true);
        manager.write_autostart_config(false).unwrap();
        assert_eq!(manager.read_autostart_config(), false);
    }

    #[tokio::test]
    async fn invalid_existing_feedback_configuration_is_not_replaced_with_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let manager = VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        );
        manager.write_autostart_config(true).unwrap();
        for raw in [
            "[]",
            r#"{"feedback":{"volume":101},"autostart":true}"#,
            "invalid-json",
        ] {
            std::fs::write(&manager.config_path, raw).unwrap();
            assert!(manager.feedback_settings().await.is_err());
            assert!(
                manager
                    .update_feedback(VoiceFeedbackPatch::default())
                    .await
                    .is_err()
            );
            assert_eq!(std::fs::read_to_string(&manager.config_path).unwrap(), raw);
        }
    }

    #[test]
    fn test_resolve_ai_mate_binary_candidate_search() {
        let resolved = VoiceCompanionProcessManager::resolve_voice_companion_binary();
        if let Some(path) = resolved {
            assert!(path.is_file());
            assert!(path.to_string_lossy().contains("voice-companion") || path.to_string_lossy().contains("ai-mate"));
        }
    }

    #[test]
    fn installed_voice_binary_uses_the_desktop_resource_directory_without_a_checkout() {
        let root=tempfile::tempdir().unwrap();
        let bin=root.path().join("Resources/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let old_name=if cfg!(windows){"ai-mate.exe"}else{"ai-mate"};
        let daemon=bin.join(old_name);std::fs::write(&daemon,b"test-only executable fixture").unwrap();
        let host=root.path().join("MacOS/Flowy");
        assert_eq!(VoiceCompanionProcessManager::installed_ai_mate_binary(Some(&bin),Some(&host)),Some(daemon.clone()));
        assert!(VoiceCompanionProcessManager::installed_ai_mate_binary(None,Some(&host)).is_none());
        let neighboring=root.path().join(old_name);std::fs::write(&neighboring,b"test-only colocated fixture").unwrap();
        assert_eq!(VoiceCompanionProcessManager::installed_ai_mate_binary(None,Some(&root.path().join("Flowy.exe"))),Some(neighboring));
        // The packaged name wins over the legacy ai-mate name.
        let current=bin.join(if cfg!(windows){"voice-companion.exe"}else{"voice-companion"});
        std::fs::write(&current,b"test-only current fixture").unwrap();
        assert_eq!(VoiceCompanionProcessManager::installed_voice_companion_binary(Some(&bin),Some(&host)),Some(current));
    }

    #[tokio::test]
    async fn missing_voice_binary_reports_an_actionable_error_without_restart_noise() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        manager.write_autostart_config(true).unwrap();
        let mut events = manager.subscribe();
        manager.note_missing_voice_binary();
        let snapshot = events.try_recv().unwrap();
        assert_eq!(snapshot.phase, VoiceSessionPhase::Error);
        assert_eq!(snapshot.connection, VoiceConnectionState::Offline);
        assert_eq!(snapshot.error.unwrap().code, VOICE_COMPANION_MISSING);
        assert!(manager.snapshot().enabled);
        manager.note_missing_voice_binary();
        assert!(events.try_recv().is_err());
    }

    #[test]
    fn host_stream_recovers_unchanged_daemon_snapshot_and_fences_previous_process() {
        let dir = tempfile::tempdir().unwrap();
        let models = Arc::new(nomifun_voice_models::ModelManager::new(
            dir.path().to_path_buf(),
        ));
        let manager = Arc::new(VoiceCompanionProcessManager::new(models, dir.path()));
        manager.write_autostart_config(true).unwrap();
        let mut events = manager.subscribe();
        let daemon = VoiceSessionSnapshot {
            instance_id: "daemon-a".into(),
            seq: 5,
            phase: VoiceSessionPhase::Listening,
            session_id: Some("session-a".into()),
            acoustic_ready: true,
            ..Default::default()
        };
        manager.project(daemon.clone(), 0);
        let first = events.try_recv().unwrap();
        assert_ne!(first.instance_id, daemon.instance_id);
        assert!(first.enabled && first.acoustic_ready);
        manager.project(daemon.clone(), 0);
        assert!(events.try_recv().is_err());
        manager.disconnected();
        assert!(!events.try_recv().unwrap().acoustic_ready);
        manager.project(daemon.clone(), 0);
        let restored = events.try_recv().unwrap();
        assert_eq!(restored.connection, VoiceConnectionState::Connected);
        assert!(restored.seq > first.seq);
        manager.generation.store(1, Ordering::Release);
        manager.project(VoiceSessionSnapshot { seq: 99, ..daemon }, 0);
        assert!(events.try_recv().is_err());
    }

    #[tokio::test]
    async fn host_shutdown_preserves_enabled_intent() {
        let dir = tempfile::tempdir().unwrap();
        let models = Arc::new(nomifun_voice_models::ModelManager::new(
            dir.path().to_path_buf(),
        ));
        let manager = Arc::new(VoiceCompanionProcessManager::new(models, dir.path()));
        manager.write_autostart_config(true).unwrap();
        manager.stop().await.unwrap();
        assert!(manager.read_autostart_config());
        assert!(manager.shutdown_requested.load(Ordering::Acquire));
        manager.toggle(false).await.unwrap();
        assert!(!manager.read_autostart_config());
    }

    #[tokio::test]
    async fn bv_01_target_settings_defaults_to_voice_home_without_starting_audio() {
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        let app = voice_companion_routes(manager.clone());
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/voice/companion/target")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 16_384).await.unwrap();
        let settings: VoiceTargetSettings = serde_json::from_slice(&body).unwrap();
        assert_eq!(settings.requested.mode, VoiceTargetMode::VoiceHome);
        assert_eq!(settings.effective.mode, VoiceTargetMode::VoiceHome);
        assert!(settings.effective_conversation_id.is_none());
        assert_eq!(settings.reason.as_deref(), Some("workspace_routing_active"));
        assert!(!manager.status().await.running);
    }

    #[tokio::test]
    async fn target_update_and_candidate_endpoints_work() {
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        let app = voice_companion_routes(manager.clone());

        // POST /target with missing companion_id when companion mode requested returns 400
        let req_missing = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/target")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::json!({ "mode": "companion" }).to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req_missing).await.unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::BAD_REQUEST);

        // POST /target with voice_home returns 200
        let req_home = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/target")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::json!({ "mode": "voice_home" }).to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req_home).await.unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);

        // HTTP cannot attest native window focus
        let req_candidate = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/candidate")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::json!({ "conversation_id": "candidate-c1" }).to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req_candidate).await.unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::FORBIDDEN);
        assert_eq!(
            manager.active_window_candidate().read().unwrap().conversation_id.as_deref(),
            None
        );

        // POST /candidate with null clears candidate conversation
        let req_clear = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/candidate")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::json!({ "conversation_id": null }).to_string()))
            .unwrap();
        let resp = app.oneshot(req_clear).await.unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::FORBIDDEN);
        assert_eq!(
            manager.active_window_candidate().read().unwrap().conversation_id.as_deref(),
            None
        );
    }

    #[test]
    fn test_task_feedback_tracker_throttling_and_stage_reset() {
        let mut tracker = TaskFeedbackTracker::new();
        let mut task = VoiceTaskSnapshot {
            command_id: "cmd-1".into(),
            session_id: "session-1".into(),
            acoustic_turn_id: "turn-1".into(),
            conversation_id: Some("conv-1".into()),
            message_id: Some("msg-1".into()),
            runtime_turn_id: None,
            status: VoiceTaskStatus::Accepted,
            result_ok: None,
            result_text: None,
            error_code: None,
        };

        let t0 = 100_000_i64;
        // At t=0, task just accepted, no feedback due yet
        assert_eq!(tracker.evaluate(&task, t0), TaskFeedbackDecision::None);

        // Before 15s (t0 + 14_999), still no feedback
        assert_eq!(tracker.evaluate(&task, t0 + 14_999), TaskFeedbackDecision::None);

        // At 15s (t0 + 15_000), first waiting feedback is due
        let decision = tracker.evaluate(&task, t0 + 15_000);
        assert_eq!(
            decision,
            TaskFeedbackDecision::TriggerFeedback {
                command_id: "cmd-1".into(),
                stage: "processing".into(),
            }
        );
        // Record dispatched
        tracker.record_dispatched(t0 + 15_000);

        // 15s after first dispatch (t0 + 30_000), throttled (needs 45s)
        assert_eq!(tracker.evaluate(&task, t0 + 30_000), TaskFeedbackDecision::None);

        // 44.999s after first dispatch (t0 + 59_999), still throttled
        assert_eq!(tracker.evaluate(&task, t0 + 59_999), TaskFeedbackDecision::None);

        // 45s after first dispatch (t0 + 60_000), second feedback is due
        assert_eq!(
            tracker.evaluate(&task, t0 + 60_000),
            TaskFeedbackDecision::TriggerFeedback {
                command_id: "cmd-1".into(),
                stage: "processing".into(),
            }
        );
        tracker.record_dispatched(t0 + 60_000);

        // 45s after second dispatch (t0 + 105_000), max 2 consecutive reached, suppressed
        assert_eq!(tracker.evaluate(&task, t0 + 105_000), TaskFeedbackDecision::None);

        // Even after longer time (t0 + 200_000), same stage remains capped at 2
        assert_eq!(tracker.evaluate(&task, t0 + 200_000), TaskFeedbackDecision::None);

        // When stage changes (e.g. tool execution turn starts), consecutive count resets
        task.runtime_turn_id = Some("tool_exec_read".into());
        assert_eq!(tracker.evaluate(&task,t0+201_000),TaskFeedbackDecision::None,"a turn id is not business progress");
        let progress=nomifun_api_types::voice::VoiceTaskProgress {command_id:task.command_id.clone(),stage:"tool".into(),health:"fresh".into(),last_event_at_ms:Some(t0+201_000),last_progress_at_ms:Some(t0+201_000)};
        let stage_decision = tracker.evaluate_progress(&task,Some(&progress),t0+201_000);
        assert_eq!(
            stage_decision,
            TaskFeedbackDecision::TriggerFeedback {
                command_id: "cmd-1".into(),
                stage: "tool:fresh".into(),
            }
        );
        tracker.record_dispatched(t0 + 201_000);

        // Terminal state clears feedback tracking
        task.status = VoiceTaskStatus::Completed;
        assert_eq!(tracker.evaluate(&task, t0 + 202_000), TaskFeedbackDecision::None);
        assert!(tracker.pending_feedback.is_none());
        assert!(tracker.active_command_id.is_none());
    }

    #[tokio::test]
    async fn bv_12_high_frequency_pcm_and_instance_switch_preserve_task_and_monotonic_seq() {
        let dir = tempfile::tempdir().unwrap();
        let models = Arc::new(nomifun_voice_models::ModelManager::new(dir.path().to_path_buf()));
        let manager = Arc::new(VoiceCompanionProcessManager::new(models, dir.path()));
        manager.write_autostart_config(true).unwrap();

        // 1. Establish initial active task and confirmation
        let task = VoiceTaskSnapshot {
            command_id: "cmd-bv12".into(),
            session_id: "session-bv12".into(),
            acoustic_turn_id: "turn-bv12".into(),
            conversation_id: Some("conv-bv12".into()),
            message_id: Some("msg-bv12".into()),
            runtime_turn_id: None,
            status: VoiceTaskStatus::Accepted,
            result_ok: None,
            result_text: None,
            error_code: None,
        };
        manager.replay_task_projection(task.clone());

        let confirmation = VoiceToolConfirmation {
            approval_id: "app-bv12".into(),
            msg_id: "msg-bv12".into(),
            call_id: "call-bv12".into(),
            tool_name: "WorkspaceFileRead".into(),
            arguments: serde_json::json!({"path": "test.txt"}),
            expires_at_ms: nomifun_common::now_ms() + 30_000,
        };
        manager.project_confirmation(&task, Some(confirmation.clone()));

        // Verify baseline snapshot
        let snap = manager.snapshot();
        assert_eq!(snap.task.as_ref(), Some(&task));
        assert_eq!(snap.confirmation.as_ref(), Some(&confirmation));
        let initial_seq = snap.seq;

        // 2. Blast 100 high-frequency PCM / audio diagnostics snapshots from daemon
        let mut last_seq = initial_seq;
        let daemon_id = "daemon-instance-1";
        for i in 1..=100 {
            let daemon_snap = VoiceSessionSnapshot {
                instance_id: daemon_id.into(),
                seq: i as u64,
                enabled: true,
                connection: VoiceConnectionState::Connected,
                phase: VoiceSessionPhase::Listening,
                session_id: Some("session-bv12".into()),
                turn_id: Some("turn-bv12".into()),
                acoustic_ready: true,
                audio: Some(nomifun_api_types::voice::VoiceAudioDiagnostics {
                    rms_milli: (i * 10) as u16,
                    peak_milli: (i * 20) as u16,
                    frames_received: i as u64,
                    last_frame_at_ms: Some(nomifun_common::now_ms()),
                    capture_state: "active".into(),
                    speech_detected: i % 2 == 0,
                    asr_ready: true,
                    kws_ready: true,
                    vad_ready: true,
                    utterance_samples: i as u32 * 160,
                    utterance_peak_milli: (i * 15) as u16,
                    ..Default::default()
                }),
                ..Default::default()
            };
            manager.project(daemon_snap, 0);

            let current = manager.snapshot();
            // Task and confirmation must NEVER be erased by acoustic updates
            assert_eq!(current.task.as_ref(), Some(&task), "audio update {} cleared task", i);
            assert_eq!(current.confirmation.as_ref(), Some(&confirmation), "audio update {} cleared confirmation", i);
            // Sequence must be strictly monotonic
            assert!(current.seq > last_seq, "seq must strictly increase: {} vs {}", current.seq, last_seq);
            last_seq = current.seq;
        }

        // 3. Stale / out-of-order snapshots with lower seq must be ignored
        let stale = VoiceSessionSnapshot {
            instance_id: daemon_id.into(),
            seq: 50, // lower than 100
            enabled: true,
            connection: VoiceConnectionState::Connected,
            phase: VoiceSessionPhase::Idle,
            ..Default::default()
        };
        manager.project(stale, 0);
        assert_eq!(manager.snapshot().seq, last_seq, "stale seq must be ignored");

        // 4. Daemon instance switch invalidates previous controls
        let old_instance_id = manager.snapshot().instance_id;
        // Host gets refreshed daemon instance
        manager.project(VoiceSessionSnapshot {
            instance_id: "daemon-instance-2".into(),
            seq: 1,
            enabled: true,
            connection: VoiceConnectionState::Connected,
            phase: VoiceSessionPhase::Idle,
            ..Default::default()
        }, 0);

        // Control request with old instance ID is rejected
        let stale_control = VoiceSessionControlRequest {
            instance_id: old_instance_id,
            session_id: Some("session-bv12".into()),
            turn_id: Some("turn-bv12".into()),
        };
        let stop_err = manager.stop_session(stale_control).await;
        assert_eq!(stop_err, Err("session_scope_changed".to_string()));
    }

    #[tokio::test]
    async fn bv_13_user_speech_suppresses_progress_and_completion_replaces_old_cues() {
        let dir = tempfile::tempdir().unwrap();
        let models = Arc::new(nomifun_voice_models::ModelManager::new(dir.path().to_path_buf()));
        let manager = Arc::new(VoiceCompanionProcessManager::new(models, dir.path()));
        manager.write_autostart_config(true).unwrap();

        let mut tracker = TaskFeedbackTracker::new();
        let task = VoiceTaskSnapshot {
            command_id: "cmd-bv13".into(),
            session_id: "session-bv13".into(),
            acoustic_turn_id: "turn-bv13".into(),
            conversation_id: Some("conv-bv13".into()),
            message_id: Some("msg-bv13".into()),
            runtime_turn_id: None,
            status: VoiceTaskStatus::Accepted,
            result_ok: None,
            result_text: None,
            error_code: None,
        };

        let t0 = 200_000_i64;
        assert_eq!(tracker.evaluate(&task, t0), TaskFeedbackDecision::None);
        assert_eq!(tracker.evaluate(&task, t0 + 15_000), TaskFeedbackDecision::TriggerFeedback {
            command_id: "cmd-bv13".into(),
            stage: "processing".into(),
        });
        assert_eq!(tracker.pending_feedback.as_deref(), Some("processing"));

        // When task completes while feedback is pending, terminal state immediately clears pending feedback
        let mut completed_task = task.clone();
        completed_task.status = VoiceTaskStatus::Completed;
        completed_task.result_ok = Some(true);
        completed_task.result_text = Some("真实读取结果成功".into());

        assert_eq!(tracker.evaluate(&completed_task, t0 + 16_000), TaskFeedbackDecision::None);
        assert!(tracker.pending_feedback.is_none(), "terminal state must replace and discard old pending cues");

        // Speech suppression: when daemon has speech_detected = true, maybe_notify_task_progress does not dispatch
        manager.replay_acoustic_source(VoiceSessionSnapshot {
            instance_id: manager.snapshot().instance_id,
            seq: 1,
            enabled: true,
            connection: VoiceConnectionState::Connected,
            phase: VoiceSessionPhase::Listening,
            session_id: Some("session-bv13".into()),
            turn_id: Some("turn-bv13".into()),
            audio: Some(nomifun_api_types::voice::VoiceAudioDiagnostics {
                speech_detected: true,
                ..Default::default()
            }),
            ..Default::default()
        });

        // Initialize manager's task_feedback tracker so it would be due if not suppressed
        {
            let mut tracker_guard = manager.task_feedback.lock().unwrap();
            tracker_guard.evaluate(&task, t0);
            tracker_guard.accepted_at_ms = nomifun_common::now_ms() - 20_000;
        }

        // Calling maybe_notify_task_progress while user is speaking returns Ok(()) without dispatching
        let res = manager.maybe_notify_task_progress(&task).await;
        assert!(res.is_ok());
        // Verify tracker was not marked dispatched because speech was active
        let tracker_guard = manager.task_feedback.lock().unwrap();
        assert_eq!(tracker_guard.last_feedback_at_ms, 0, "must not dispatch during user speech");
    }

    #[tokio::test]
    async fn test_task_replay_and_notification_endpoints() {
        use tower::ServiceExt;
        use nomifun_api_types::voice::VoiceTaskCommand;
        let dir = tempfile::tempdir().unwrap();
        let models = Arc::new(nomifun_voice_models::ModelManager::new(dir.path().to_path_buf()));

        struct StubRuntime;
        #[async_trait::async_trait]
        impl VoiceTaskRuntime for StubRuntime {
            async fn resolve_configuration(&self, config: &VoiceExecutionConfig) -> Result<VoiceExecutionConfig, String> { Ok(config.clone()) }
            async fn prepare_configuration(&self, config: &VoiceExecutionConfig) -> Result<(VoiceExecutionConfig, Option<nomifun_api_types::ResolvedPresetSnapshot>), String> { Ok((config.clone(), None)) }
            async fn create(&self, _: &str, _: &VoiceExecutionConfig, _: Option<&nomifun_api_types::ResolvedPresetSnapshot>, _: &str) -> Result<String, crate::voice_tasks::TaskMutationError> { Ok("conv-stub".into()) }
            async fn create_bound(&self, owner: &str, config: &VoiceExecutionConfig, preset: Option<&nomifun_api_types::ResolvedPresetSnapshot>, creation_key: &str, target: Option<&str>) -> Result<String, crate::voice_tasks::TaskMutationError> {
                if let Some(target_id) = target {
                    return Ok(target_id.into());
                }
                self.create(owner, config, preset, creation_key).await
            }
            async fn find_created(&self, _: &str, _: &str) -> Result<Option<String>, String> { Ok(Some("conv-stub".into())) }
            async fn append(&self, _: &str, _: &str, _: &str, _: &str, _: &str) -> Result<(), crate::voice_tasks::TaskMutationError> { Ok(()) }
            async fn send_bound(
                &self,
                owner: &str,
                conversation_id: &str,
                send_key: &str,
                message_id: &str,
                _command_id: &str,
                _display_text: &str,
                goal: &str,
                _context_digest: Option<&str>,
                _frozen_config: Option<&VoiceExecutionConfig>,
                _preset: Option<&nomifun_api_types::ResolvedPresetSnapshot>,
            ) -> Result<crate::voice_tasks::TaskReceipt, crate::voice_tasks::TaskMutationError> {
                let mut receipt = self.send(owner, conversation_id, send_key, goal).await?;
                receipt.message_id = message_id.to_string();
                Ok(receipt)
            }
            async fn send(&self, _: &str, _: &str, _: &str, _: &str) -> Result<crate::voice_tasks::TaskReceipt, crate::voice_tasks::TaskMutationError> {
                Ok(crate::voice_tasks::TaskReceipt {
                    message_id: "msg-stub".into(),
                    runtime_turn_id: None,
                    replayed: false,
                    completed: true,
                    result_ok: Some(true),
                    result_text: Some("已完成真实读取".into()),
                    error_code: None,
                })
            }
            async fn observe(&self, _: &str, _: &str, _: &str) -> Result<Option<crate::voice_tasks::TaskReceipt>, String> { Ok(None) }
            async fn cancel(&self, _: &str, _: &str, _: &str, _: &str) -> Result<(), String> { Ok(()) }
            async fn confirmations(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Vec<VoiceToolConfirmation>, String> { Ok(Vec::new()) }
            async fn confirm(&self, _: &str, _: &str, _: &str, _: &str, _: &VoiceToolConfirmation, _: bool) -> Result<(), String> { Ok(()) }
        }

        let manager = Arc::new(VoiceCompanionProcessManager::new(models, dir.path()).with_tasks("owner".into(), Arc::new(StubRuntime)));
        let app = voice_companion_routes(manager.clone());

        // Submit task which completes immediately via StubRuntime
        let cmd = VoiceTaskCommand {
            command_id: "cmd-replay-test".into(),
            session_id: "session-replay".into(),
            turn_id: "turn-replay".into(),
            goal: "测试重听".into(),
            binding_id: None,
            record_id: None,
        };
        let cfg = VoiceExecutionConfig {
            workspace: dir.path().to_str().unwrap().into(),
            model: Some(VoiceModelRef { provider_id: "test".into(), model: "test".into() }),
            preset_id: None,
            session_mode: None,
        };
        let tasks = manager.tasks.as_ref().unwrap();
        tasks.submit(cmd, cfg).await.unwrap();

        // 1. Query notification before claim
        let req_notif = axum::http::Request::builder()
            .uri("/api/voice/companion/tasks/cmd-replay-test/notification")
            .body(axum::body::Body::empty()).unwrap();
        let resp = app.clone().oneshot(req_notif).await.unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
        let notif: VoiceTaskNotification = serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 16_384).await.unwrap()).unwrap();
        assert_eq!(notif.command_id, "cmd-replay-test");
        assert_eq!(notif.result_summary, "已完成真实读取");

        // 2. Claim announcement, which generates attempt:1
        tasks.claim_announcement("cmd-replay-test", true).await.unwrap();
        let notif_claimed = tasks.notification_for("cmd-replay-test", None).unwrap().unwrap();
        assert_eq!(notif_claimed.attempt_id, "voice-attempt:cmd-replay-test:1");

        // A playback still marked dispatched cannot be replayed under a new attempt.
        {
            let mut snapshot=manager.snapshot.write().unwrap();snapshot.session_id=Some("session-replay".into());snapshot.turn_id=Some("turn-replay".into());
        }
        let request=|instance:&str,replay:&str|axum::http::Request::builder().method("POST")
            .uri("/api/voice/companion/tasks/cmd-replay-test/replay").header("content-type","application/json")
            .body(axum::body::Body::from(serde_json::json!({"instance_id":instance,"session_id":"session-replay","turn_id":"turn-replay","replay_id":replay}).to_string())).unwrap();
        let old=app.clone().oneshot(request("old-host","replay-one")).await.unwrap();assert_eq!(old.status(),axum::http::StatusCode::CONFLICT);
        let pending=app.clone().oneshot(request(&manager.snapshot().instance_id,"replay-one")).await.unwrap();assert_eq!(pending.status(),axum::http::StatusCode::SERVICE_UNAVAILABLE);
        tasks.finish_announcement("cmd-replay-test",&notif_claimed.attempt_id,"failed").await.unwrap();
        let resp=app.clone().oneshot(request(&manager.snapshot().instance_id,"replay-one")).await.unwrap();assert_eq!(resp.status(),axum::http::StatusCode::SERVICE_UNAVAILABLE);
        let again=app.oneshot(request(&manager.snapshot().instance_id,"replay-one")).await.unwrap();assert_eq!(again.status(),axum::http::StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(tasks.notification_for("cmd-replay-test", None).unwrap().unwrap().status,"failed","offline replay cannot claim acceptance or change playback history");
    }

    #[test]
    fn test_spoken_summary_concise_text_preserved() {
        let text = "北京今天晴，气温 15 到 25 度，北风 2 级。";
        let summary = extract_spoken_summary(text, VoiceTaskStatus::Completed, false);
        assert_eq!(summary, text);
        assert!(summary.chars().count() <= 80);
    }

    #[test]
    fn result_announcement_uses_call_locale_for_wrappers_and_empty_results() {
        let mut task: VoiceTaskSnapshot = serde_json::from_value(serde_json::json!({
            "command_id":"command", "session_id":"session", "acoustic_turn_id":"turn",
            "conversation_id":"conversation", "status":"completed", "result_ok":true
        })).unwrap();
        assert_eq!(format_task_result_announcement(&task, "", true), "Execution finished. See conversation for details.");
        assert_eq!(format_task_result_announcement(&task, "", false), "执行已结束，详情请在会话中查看。");
        task.result_text = Some("Sorry, could not find any records.".into());
        assert_eq!(format_task_result_announcement(&task, "find records", true), "Result for ‘find records’: Sorry, could not find any records.");
        assert_eq!(format_task_result_announcement(&task, "查询记录", false), "任务「查询记录」的结果：Sorry, could not find any records.");
        task.result_text = Some("The query returned 125 records and the report is available in the requested output directory, with all records sorted by date and ready for review.".into());
        let text = format_task_result_announcement(&task, "report", true);
        assert!(!text.contains("详情") && !text.contains("任务"), "{text}");
        task.status = VoiceTaskStatus::Failed;
        task.result_text = None;
        assert_eq!(format_task_result_announcement(&task, "", true), "Execution was unsuccessful. See conversation for details.");
    }

    #[test]
    fn test_spoken_summary_preserves_english_failure_and_restrictions() {
        let raw = "The deployment validation and health checks completed successfully for all 120 configured records. However the database backup was NOT created and you must NOT delete the previous backup.";
        let summary = extract_spoken_summary(raw, VoiceTaskStatus::Completed, true);
        assert!(!summary.contains("completed success"), "must not speak only the successful preamble: {summary}");
        assert!(summary.to_ascii_lowercase().contains("not") || summary.contains("conditions"), "must preserve the restriction or defer to full details: {summary}");
        assert!(summary.chars().count() <= 80);
        for negative in ["cannot", "can't", "couldn’t", "FAILED", "unable", "must not"] {
            assert!(is_negation_or_failure(&format!("Backup {negative} be completed")));
        }
        assert!(!is_negation_or_failure("A notable success notification"));
        assert_eq!(split_sentences("Backup failed. Keep version 1.25 unchanged."), vec!["Backup failed", "Keep version 1.25 unchanged"]);
        let with_number = "All deployment preparation and validation checks completed successfully across the workspace. Backup failed at 1.25 GB.";
        let summary = extract_spoken_summary(with_number, VoiceTaskStatus::Completed, true);
        assert!(summary.contains("Backup failed at 1.25 GB"), "must keep the failure and decimal: {summary}");
        let no_error = "All deployment preparation and validation checks completed successfully across the workspace. No errors occurred.";
        let summary = extract_spoken_summary(no_error, VoiceTaskStatus::Completed, true);
        assert!(summary.contains("No errors occurred"), "a negation is not proof of a failed task: {summary}");
    }

    #[test]
    fn test_spoken_summary_extracts_conclusion_and_parameters_from_long_markdown() {
        let raw = r#"### 任务执行报告
本次任务已经执行完成。
经过全网检索，查询到北京今天天气为晴天，气温 15℃ 至 26℃，空气质量优良。
```json
{"city": "Beijing", "temp": "15-26C", "air": "good"}
```
详细的天气趋势和未来一周预测已保存在会话上下文，用户可随时在界面滚动查看。"#;
        let summary = extract_spoken_summary(raw, VoiceTaskStatus::Completed, false);
        assert!(summary.contains("完成") || summary.contains("查询到"));
        assert!(summary.contains("北京") || summary.contains("天气"));
        assert!(summary.contains("15") && summary.contains("26"));
        assert!(!summary.contains("```"));
        assert!(!summary.contains("###"));
        assert!(summary.chars().count() <= 80, "spoken summary must be <= 80 chars, got: {}", summary.chars().count());
        assert!(summary.ends_with("，详情请在会话中查看。") || summary.ends_with("。"));
    }

    #[test]
    fn test_spoken_summary_strictly_preserves_negation() {
        let raw = "任务执行失败：未能找到您指定的备份目录，请检查路径是否正确，或者重新指定要备份的文件夹。详细错误日志已经写入系统控制台。";
        let summary = extract_spoken_summary(raw, VoiceTaskStatus::Failed, false);
        assert!(summary.contains("失败"), "summary must retain negation/failure marker");
        assert!(summary.contains("未能") || summary.contains("未"), "summary must retain negation marker");
        assert!(summary.chars().count() <= 80);
    }

    #[test]
    fn test_spoken_summary_handles_empty_fallback() {
        let summary_done = extract_spoken_summary("", VoiceTaskStatus::Completed, false);
        assert_eq!(summary_done, "执行已结束，详情请在会话中查看。");
        let summary_failed = extract_spoken_summary("   ", VoiceTaskStatus::Failed, false);
        assert_eq!(summary_failed, "任务未完成，详情请在会话中查看。");
    }

    #[test]
    fn test_format_task_status_text_neutral_presentation() {
        let mut task = nomifun_api_types::voice::VoiceTaskSnapshot {
            command_id: "cmd-1".into(),
            session_id: "sess-1".into(),
            acoustic_turn_id: "turn-1".into(),
            conversation_id: Some("conv-1".into()),
            message_id: Some("msg-1".into()),
            runtime_turn_id: Some("rt-1".into()),
            status: VoiceTaskStatus::Completed,
            result_ok: Some(true),
            result_text: Some("抱歉，未能获取天气信息。".into()),
            error_code: None,
        };

        // 1. Chinese completed with unachieved text: neutral prefix + raw text preserved
        let zh_text = format_task_status_text(&task, false, false);
        assert_eq!(zh_text, "执行已结束。抱歉，未能获取天气信息。");

        // 2. English completed with unachieved text: neutral prefix + raw text preserved
        task.result_text = Some("Sorry, could not find any records.".into());
        let en_text = format_task_status_text(&task, true, false);
        assert_eq!(en_text, "Execution finished. Sorry, could not find any records.");

        // 3. Completed with empty/None text
        task.result_text = None;
        assert_eq!(format_task_status_text(&task, false, false), "执行已结束。请查看任务详情。");
        assert_eq!(format_task_status_text(&task, true, false), "Execution finished. Please check task details.");

        // 4. Cancelled
        task.status = VoiceTaskStatus::Failed;
        task.error_code = Some("task_cancelled".into());
        assert_eq!(format_task_status_text(&task, false, true), "任务已取消。");
        assert_eq!(format_task_status_text(&task, true, true), "Task cancelled.");

        // 5. Failed
        task.result_text = Some("Tool execution failed".into());
        assert_eq!(format_task_status_text(&task, false, false), "任务已结束，执行结果未成功。Tool execution failed");
        assert_eq!(format_task_status_text(&task, true, false), "Task ended, execution was unsuccessful. Tool execution failed");

        // 6. Normal success text
        task.status = VoiceTaskStatus::Completed;
        task.result_text = Some("已为您创建日程。".into());
        assert_eq!(format_task_status_text(&task, false, false), "执行已结束。已为您创建日程。");
        assert_eq!(format_task_status_text(&task, true, false), "Execution finished. 已为您创建日程。");
    }

    #[test]
    fn test_spoken_summary_must_not_drop_actual_backup_failure() {
        let raw = "目录扫描已完成，已检查全部文件并统计文件数量和大小以及当前磁盘空间使用情况并生成了详细目录结构和待处理文件清单用于下一步执行，但是备份失败没有复制任何文件且目标目录仍然为空，请勿删除原始文件以免丢失数据。";
        let summary = extract_spoken_summary(raw, VoiceTaskStatus::Completed, false);
        assert!(summary.contains("备份失败") || summary.contains("没有复制"), "spoken summary lost the actual failure: {summary}");
        assert!(summary.chars().count() <= 80, "summary must be <= 80 chars, got: {}", summary.chars().count());
    }

    #[tokio::test]
    async fn test_session_start_call_lifecycle_and_conflict() {
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        let app = voice_companion_routes(manager.clone());

        // 1. Calling when manager not running returns 400
        let req_stopped = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/session/start")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_string(&VoiceCallStartRequest { locale: None,
                session_mode: None,
                request_id: "req-1".into(),
                target: VoiceCallTarget::DefaultWorkspace,
            }).unwrap()))
            .unwrap();
        let resp = app.clone().oneshot(req_stopped).await.unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::BAD_REQUEST);

        // 2. Set mock child and mock recorder
        struct TestCallRecorder {
            conversation_id: String,
            memory_owner: Option<String>,
            reservation: std::sync::RwLock<Option<crate::voice_dialogue::VoiceCallReservation>>,
            resolutions: Arc<std::sync::atomic::AtomicUsize>,
        }
        #[async_trait::async_trait]
        impl VoiceDialogueRecorder for TestCallRecorder {
            fn set_call_reservation(&self, res: Option<crate::voice_dialogue::VoiceCallReservation>) {
                *self.reservation.write().unwrap() = res;
            }
            fn take_call_reservation(&self, request_id: &str) -> Option<crate::voice_dialogue::VoiceCallReservation> {
                let mut reservation=self.reservation.write().unwrap();
                if reservation.as_ref().is_some_and(|r|r.request_id==request_id) {reservation.take()} else {None}
            }
            fn current_call_reservation(&self) -> Option<crate::voice_dialogue::VoiceCallReservation> {
                self.reservation.read().unwrap().clone()
            }
            async fn preflight(&self, _: &VoiceDialogueRequest) -> Result<VoiceDialoguePreflight, VoiceDialogueRecorderError> {
                Err(VoiceDialogueRecorderError::BindingInvalid("test".into()))
            }
            async fn record_pair(&self, _: &str, _: &VoiceDialogueRequest, _: &str) -> Result<(), VoiceDialogueRecorderError> {
                Ok(())
            }
            async fn resolve_call_target_with_locale(&self, target: &VoiceCallTarget, _session_mode: Option<&str>, _request_id: Option<&str>, _locale: Option<&str>) -> Result<(String, Option<String>), VoiceDialogueRecorderError> {
                self.resolutions.fetch_add(1, Ordering::SeqCst);
                match target {
                    VoiceCallTarget::Workspace { workspace } if workspace == "invalid_dir" => {
                        Err(VoiceDialogueRecorderError::WriteFailed("workspace_directory_invalid: not a directory".into()))
                    }
                    VoiceCallTarget::Workspace { .. } => Ok((self.conversation_id.clone(), self.memory_owner.clone())),
                    VoiceCallTarget::DefaultWorkspace | VoiceCallTarget::CurrentSelection => Ok((self.conversation_id.clone(), self.memory_owner.clone())),
                    VoiceCallTarget::VoiceConversation { conversation_id } => Ok((conversation_id.clone(), self.memory_owner.clone())),
                    VoiceCallTarget::NewVoiceConversation { .. } => Ok((self.conversation_id.clone(), self.memory_owner.clone())),
                }
            }
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let reject_wake = Arc::new(AtomicBool::new(true));
        let wake_rejection = reject_wake.clone();
        let unknown_wake = Arc::new(AtomicBool::new(false));
        let wake_unknown = unknown_wake.clone();
        let reject_stop = Arc::new(AtomicBool::new(true));
        let stop_rejection = reject_stop.clone();
        let daemon_snapshot = Arc::new(RwLock::new(VoiceSessionSnapshot::default()));
        let stop_snapshot = daemon_snapshot.clone();
        let read_snapshot = daemon_snapshot.clone();
        let mock_daemon = axum::Router::new().route(
            "/api/voice/wake",
            axum::routing::post(move || {let rejected=wake_rejection.load(Ordering::SeqCst);let unknown=wake_unknown.load(Ordering::SeqCst);async move {
                if unknown {
                    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, axum::Json(serde_json::json!({"success":false,"error":"unexpected_failure"})))
                } else if rejected {
                    (axum::http::StatusCode::SERVICE_UNAVAILABLE, axum::Json(serde_json::json!({"success":false,"error":"models_unavailable"})))
                } else {
                    (axum::http::StatusCode::OK, axum::Json(serde_json::json!({ "success": true })))
                }
            }}),
        ).route("/api/voice/stop",axum::routing::post(move || {
            let rejected=stop_rejection.load(Ordering::SeqCst);let snapshot=stop_snapshot.clone();
            async move {
                if rejected {return axum::http::StatusCode::CONFLICT;}
                let mut snapshot=snapshot.write().unwrap();
                snapshot.seq+=1;snapshot.phase=VoiceSessionPhase::Idle;
                snapshot.ended_reason=Some(nomifun_api_types::voice::VoiceSessionEndReason::UserStop);
                axum::http::StatusCode::OK
            }
        })).route("/api/voice/snapshot",axum::routing::get(move || {
            let snapshot=read_snapshot.clone();async move {axum::Json(snapshot.read().unwrap().clone())}
        }));
        let daemon_server = tokio::spawn(async move {
            let _ = axum::serve(listener, mock_daemon).await;
        });

        let recorder = Arc::new(TestCallRecorder {
            resolutions: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            conversation_id: "conv-voice-test".into(),
            memory_owner: Some("flowy-bolt".into()),
            reservation: std::sync::RwLock::new(None),
        });
        let manager_with_rec = Arc::new(
            VoiceCompanionProcessManager::new(
                Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
                dir.path(),
            )
            .with_replay_daemon(format!("http://127.0.0.1:{port}"))
            .with_recorder(recorder.clone()),
        );
        // Safe synthetic child (R08: non-recursive dummy child)
        let builder = if cfg!(windows) {
            let mut b = nomi_process_runtime::ChildProcessBuilder::new("cmd.exe");
            b.args(["/c", "ping", "-n", "10", "127.0.0.1"]);
            b
        } else {
            let mut b = nomi_process_runtime::ChildProcessBuilder::new("sleep");
            b.arg("10");
            b
        };
        let managed = builder.spawn_managed().ok();
        if let Some(m) = managed {
            *manager_with_rec.child.lock().await = Some(m);
        }

        let app_running = voice_companion_routes(manager_with_rec.clone());

        // 3. Invalid directory returns 400
        let req_invalid_ws = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/session/start")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_string(&VoiceCallStartRequest { locale: None,
                session_mode: None,
                request_id: "req-invalid".into(),
                target: VoiceCallTarget::Workspace { workspace: "invalid_dir".into() },
            }).unwrap()))
            .unwrap();
        let resp_invalid = app_running.clone().oneshot(req_invalid_ws).await.unwrap();
        assert_eq!(resp_invalid.status(), axum::http::StatusCode::BAD_REQUEST);

        // A definite daemon rejection is replayed in exactly the same structured form.
        let rejected_request=VoiceCallStartRequest { locale: None,request_id:"req-rejected".into(),target:VoiceCallTarget::CurrentSelection,session_mode:None};
        let first_rejection=manager_with_rec.start_call(rejected_request.clone()).await.unwrap_err();
        assert_eq!(first_rejection.0,axum::http::StatusCode::SERVICE_UNAVAILABLE);
        let rejection: serde_json::Value = serde_json::from_str(&first_rejection.1).unwrap();
        assert_eq!(rejection["code"],"daemon_wake_rejected");
        assert_eq!(rejection["request_id"],"req-rejected");
        assert_eq!(rejection["terminal"],true);
        assert!(recorder.current_call_reservation().is_none());
        reject_wake.store(false,Ordering::SeqCst);
        let replay_rejection=manager_with_rec.start_call(rejected_request).await.unwrap_err();
        assert_eq!(replay_rejection,first_rejection);

        // An unclassified daemon failure must retain the reservation and replay
        // the same request rather than resolving a second conversation.
        unknown_wake.store(true, Ordering::SeqCst);
        let unknown_request=VoiceCallStartRequest { locale: None,request_id:"req-unknown".into(),target:VoiceCallTarget::CurrentSelection,session_mode:None};
        let unknown=manager_with_rec.start_call(unknown_request.clone()).await.unwrap_err();
        assert_eq!(unknown.0, axum::http::StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(serde_json::from_str::<serde_json::Value>(&unknown.1).unwrap()["code"], "daemon_wake_unknown");
        assert_eq!(recorder.current_call_reservation().unwrap().request_id, "req-unknown");
        let resolutions = recorder.resolutions.load(Ordering::SeqCst);
        let replay_unknown = manager_with_rec.start_call(unknown_request).await.unwrap();
        assert_eq!(replay_unknown.request_id, "req-unknown");
        assert_eq!(replay_unknown.status, VoiceCallStartStatus::Starting);
        assert_eq!(recorder.resolutions.load(Ordering::SeqCst), resolutions);
        recorder.take_call_reservation("req-unknown");
        manager_with_rec.handled_start_requests.write().unwrap().remove("req-unknown");
        manager_with_rec.set_candidate_conversation(None);
        unknown_wake.store(false, Ordering::SeqCst);

        // 4. Valid start returns starting status and sets active candidate
        let req_valid = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/session/start")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_string(&VoiceCallStartRequest { locale: None,
                session_mode: None,
                request_id: "req-valid".into(),
                target: VoiceCallTarget::Workspace { workspace: "D:\\valid_ws".into() },
            }).unwrap()))
            .unwrap();
        let resp_valid = app_running.clone().oneshot(req_valid).await.unwrap();
        assert_eq!(resp_valid.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(resp_valid.into_body(), 16_384).await.unwrap();
        let start_resp: VoiceCallStartResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(start_resp.request_id, "req-valid");
        assert_eq!(start_resp.conversation_id, "conv-voice-test");
        assert_eq!(start_resp.status, VoiceCallStartStatus::Starting);
        assert_eq!(start_resp.memory_owner_companion_id.as_deref(), Some("flowy-bolt"));
        assert_eq!(
            manager_with_rec.active_window_candidate.read().unwrap().conversation_id.as_deref(),
            Some("conv-voice-test")
        );

        // 4a-1. F3: An old stop request with mismatched session must NOT clear new pending reservation
        let stop_old = manager_with_rec.stop_session(VoiceSessionControlRequest {
            instance_id: "old-instance".into(),
            session_id: Some("old-session-id".into()),
            turn_id: None,
        }).await;
        assert!(stop_old.is_err(), "old stop request with wrong instance/session must be rejected");
        assert!(recorder.current_call_reservation().is_some(), "F3: rejected old stop request must NOT destroy new reservation");

        // The old idle snapshot still matches all three control fields while the new wake is pending.
        let idle=manager_with_rec.snapshot();
        assert!(manager_with_rec.stop_session(VoiceSessionControlRequest {
            instance_id:idle.instance_id,session_id:idle.session_id,turn_id:idle.turn_id,
        }).await.is_err());
        assert_eq!(recorder.current_call_reservation().unwrap().request_id,"req-valid");

        // 4b. Idempotency test (R02): same request_id and target returns Starting
        let req_retry = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/session/start")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_string(&VoiceCallStartRequest { locale: None,
                session_mode: None,
                request_id: "req-valid".into(),
                target: VoiceCallTarget::Workspace { workspace: "D:\\valid_ws".into() },
            }).unwrap()))
            .unwrap();
        let resp_retry = app_running.clone().oneshot(req_retry).await.unwrap();
        assert_eq!(resp_retry.status(), axum::http::StatusCode::OK);

        // 4c. Request target conflict (R02): same request_id with different target returns 400
        let req_target_conflict = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/session/start")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_string(&VoiceCallStartRequest { locale: None,
                session_mode: None,
                request_id: "req-valid".into(),
                target: VoiceCallTarget::DefaultWorkspace,
            }).unwrap()))
            .unwrap();
        let resp_target_conflict = app_running.clone().oneshot(req_target_conflict).await.unwrap();
        assert_eq!(resp_target_conflict.status(), axum::http::StatusCode::BAD_REQUEST);

        let changed_mode = manager_with_rec.start_call(VoiceCallStartRequest { locale: None,
            request_id:"req-valid".into(), target:VoiceCallTarget::Workspace { workspace:"D:\\valid_ws".into() },
            session_mode:Some("default".into()),
        }).await.unwrap_err();
        assert_eq!(changed_mode.0, axum::http::StatusCode::BAD_REQUEST);

        // 4d. Concurrent start (R02): different request_id while pending returns 409
        let req_concurrent = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/session/start")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_string(&VoiceCallStartRequest { locale: None,
                session_mode: None,
                request_id: "req-different".into(),
                target: VoiceCallTarget::Workspace { workspace: "D:\\valid_ws".into() },
            }).unwrap()))
            .unwrap();
        let resp_concurrent = app_running.clone().oneshot(req_concurrent).await.unwrap();
        assert_eq!(resp_concurrent.status(), axum::http::StatusCode::CONFLICT);

        let resolutions_before_unknown = recorder.resolutions.load(std::sync::atomic::Ordering::SeqCst);
        if let Some(HandledStartOutcome::Starting {reserved_at_epoch_ms, ..}) = manager_with_rec.handled_start_requests.write().unwrap().get_mut("req-valid") {
            *reserved_at_epoch_ms -= 11_000;
        }
        let unknown = manager_with_rec.start_call(VoiceCallStartRequest { locale: None,
            request_id:"req-valid".into(), target:VoiceCallTarget::Workspace {workspace:"D:\\valid_ws".into()}, session_mode:None,
        }).await.unwrap_err();
        assert!(unknown.1.contains("voice_call_start_unknown"));
        assert_eq!(recorder.resolutions.load(std::sync::atomic::Ordering::SeqCst),resolutions_before_unknown);

        // 5. If already active on same conversation -> returns Active
        {
            let mut snap = manager_with_rec.snapshot.write().unwrap();
            snap.session_id = Some("active-session-1".into());
            snap.call_request_id = Some("req-valid".into());
            snap.phase = VoiceSessionPhase::Listening;
            snap.binding = Some(nomifun_api_types::voice::VoiceBindingSnapshot {
                session_id: "active-session-1".into(),
                binding_id: "binding-1".into(),
                conversation_id: "conv-voice-test".into(),
                workspace: Some("D:\\valid_ws".into()),
                memory_owner_companion_id: Some("flowy-bolt".into()),
            });
        }
        let req_same = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/session/start")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_string(&VoiceCallStartRequest { locale: None,
                session_mode: None,
                request_id: "req-same".into(),
                target: VoiceCallTarget::Workspace { workspace: "D:\\valid_ws".into() },
            }).unwrap()))
            .unwrap();
        let resp_same = app_running.clone().oneshot(req_same).await.unwrap();
        assert_eq!(resp_same.status(), axum::http::StatusCode::OK);
        let body_same = axum::body::to_bytes(resp_same.into_body(), 16_384).await.unwrap();
        let start_same: VoiceCallStartResponse = serde_json::from_slice(&body_same).unwrap();
        assert_eq!(start_same.status, VoiceCallStartStatus::Active);
        assert_eq!(start_same.conversation_id, "conv-voice-test");
        assert_eq!(start_same.session_id.as_deref(), Some("active-session-1"));

        // F1: Retrying req-valid while call is bound/active returns OK idempotently
        let req_valid_bound = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/session/start")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_string(&VoiceCallStartRequest { locale: None,
                session_mode: None,
                request_id: "req-valid".into(),
                target: VoiceCallTarget::Workspace { workspace: "D:\\valid_ws".into() },
            }).unwrap()))
            .unwrap();
        let resp_bound = app_running.clone().oneshot(req_valid_bound).await.unwrap();
        assert_eq!(resp_bound.status(), axum::http::StatusCode::OK);
        let body_bound = axum::body::to_bytes(resp_bound.into_body(), 16_384).await.unwrap();
        let bound_resp: VoiceCallStartResponse = serde_json::from_slice(&body_bound).unwrap();
        assert_eq!(bound_resp.request_id, "req-valid");
        assert_eq!(bound_resp.status, VoiceCallStartStatus::Active);

        // F1: Same req-valid with changed target after bound STILL returns 400 target conflict
        let req_bound_diff_target = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/session/start")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_string(&VoiceCallStartRequest { locale: None,
                session_mode: None,
                request_id: "req-valid".into(),
                target: VoiceCallTarget::DefaultWorkspace,
            }).unwrap()))
            .unwrap();
        let resp_bound_diff = app_running.clone().oneshot(req_bound_diff_target).await.unwrap();
        assert_eq!(resp_bound_diff.status(), axum::http::StatusCode::BAD_REQUEST);

        // 6. If already active on different conversation -> 409 CONFLICT
        let resolutions_before_conflict = recorder.resolutions.load(Ordering::SeqCst);
        let req_conflict = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/session/start")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_string(&VoiceCallStartRequest { locale: None,
                session_mode: None,
                request_id: "req-diff".into(),
                target: VoiceCallTarget::VoiceConversation { conversation_id: "other-conversation".into() },
            }).unwrap()))
            .unwrap();
        let resp_conflict = app_running.clone().oneshot(req_conflict).await.unwrap();
        assert_eq!(resp_conflict.status(), axum::http::StatusCode::CONFLICT);
        let body_conflict = axum::body::to_bytes(resp_conflict.into_body(), 16_384).await.unwrap();
        let conflict_json: serde_json::Value = serde_json::from_slice(&body_conflict).unwrap();
        assert_eq!(conflict_json["code"], "voice_call_conflict");
        assert_eq!(conflict_json["active_conversation_id"], "conv-voice-test");
        assert_eq!(recorder.resolutions.load(Ordering::SeqCst), resolutions_before_conflict,
            "an active call must reject a different target before creating any Conversation");

        // An ended old session cannot destroy a newer reservation even when the old control matches.
        let unbound_target=VoiceCallTarget::VoiceConversation {conversation_id:"conv-voice-test".into()};
        let reserved_at=chrono::Utc::now().timestamp_millis();
        recorder.set_call_reservation(Some(crate::voice_dialogue::VoiceCallReservation {
            request_id:"req-unbound".into(),target:unbound_target.clone(),conversation_id:"conv-voice-test".into(),
            memory_owner:None,reserved_at_epoch_ms:reserved_at,
        }));
        manager_with_rec.record_handled_start_request("req-unbound".into(),HandledStartOutcome::Starting {
            target:unbound_target,session_mode:None,conversation_id:"conv-voice-test".into(),memory_owner:None,reserved_at_epoch_ms:reserved_at,
        });
        let old_active=manager_with_rec.snapshot();
        assert!(manager_with_rec.stop_session(VoiceSessionControlRequest {
            instance_id:old_active.instance_id,session_id:old_active.session_id,turn_id:old_active.turn_id,
        }).await.is_err(),"an old active projection must not stop a different pending request");
        assert_eq!(recorder.current_call_reservation().unwrap().request_id,"req-unbound");
        {
            let mut snapshot=manager_with_rec.snapshot.write().unwrap();
            snapshot.phase=VoiceSessionPhase::Idle;
            snapshot.ended_reason=Some(nomifun_api_types::voice::VoiceSessionEndReason::FollowUpSilence);
        }
        let old=manager_with_rec.snapshot();
        assert!(manager_with_rec.stop_session(VoiceSessionControlRequest {
            instance_id:old.instance_id,session_id:old.session_id,turn_id:old.turn_id,
        }).await.is_err());
        assert_eq!(recorder.current_call_reservation().unwrap().request_id,"req-unbound");

        // An accepted, still-unbound call retains its reservation on a rejected stop.
        let source=VoiceSessionSnapshot {instance_id:"daemon".into(),seq:40,session_id:Some("unbound-session".into()),
            call_request_id:Some("req-unbound".into()),phase:VoiceSessionPhase::Listening,..Default::default()};
        *daemon_snapshot.write().unwrap()=source.clone();
        manager_with_rec.replay_acoustic_source(source);
        manager_with_rec.snapshot.write().unwrap().binding=None;
        let unbound=manager_with_rec.snapshot();
        assert!(manager_with_rec.stop_session(VoiceSessionControlRequest {instance_id:unbound.instance_id.clone(),session_id:unbound.session_id.clone(),turn_id:unbound.turn_id.clone()}).await.is_err());
        assert_eq!(recorder.current_call_reservation().unwrap().request_id,"req-unbound");
        reject_stop.store(false,Ordering::SeqCst);
        manager_with_rec.stop_session(VoiceSessionControlRequest {instance_id:unbound.instance_id,session_id:unbound.session_id,turn_id:unbound.turn_id}).await.unwrap();
        assert!(recorder.current_call_reservation().is_none());
        assert!(matches!(manager_with_rec.handled_start_requests.read().unwrap().get("req-unbound"),Some(HandledStartOutcome::Stopped {..})));
        manager_with_rec.mark_request_bound("req-unbound","late-session","conv-voice-test",None);
        assert!(matches!(manager_with_rec.handled_start_requests.read().unwrap().get("req-unbound"),Some(HandledStartOutcome::Stopped {..})),"late binding must not resurrect a stopped request");
        if let Some(mut child) = manager_with_rec.child.lock().await.take() {
            tokio::time::timeout(std::time::Duration::from_secs(5), child.shutdown()).await.unwrap().unwrap();
            assert!(child.try_wait().unwrap().is_some());
        }
        daemon_server.abort();
        let _ = daemon_server.await;
        let rebound = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.unwrap();
        drop(rebound);
    }

    #[test]
    fn window_context_ignores_late_clear_and_preserves_explicit_project() {
        let dir = tempfile::tempdir().unwrap();
        let manager = VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(dir.path())), dir.path());
        manager.report_window_context(VoiceWindowContext { workspace: Some("project-q".into()), sequence: 12, ..Default::default() });
        manager.report_window_context(VoiceWindowContext { sequence: 11, ..Default::default() });
        assert_eq!(manager.active_window_candidate().read().unwrap().workspace.as_deref(), Some("project-q"));
        manager.report_window_context(VoiceWindowContext { sequence: 13, ..Default::default() });
        assert!(manager.active_window_candidate().read().unwrap().workspace.is_none());
    }

    #[tokio::test]
    async fn durable_voice_identity_survives_start_cache_eviction_and_file_reopen() {
        // Real parts under test: the manager's start-call arbitration and
        // bounded handled-start cache, the production
        // ConversationVoiceDialogueRecorder, file-backed SQLite, the on-disk
        // dialogue journal/workspace registry, and durable provisional
        // creation identity. Simulated boundary only: the daemon wake IPC
        // endpoint and the model call (no microphone or acoustic models are
        // started). No route, arbitration, or storage layer is mocked.
        use async_trait::async_trait;
        use nomifun_ai_agent::{
            AgentRuntimeRegistry, runtime_handle::AgentRuntimeHandle,
            types::AgentRuntimeBuildOptions,
        };
        use nomifun_common::{AgentKillReason, AppError, MessageId, TimestampMs};
        use nomifun_conversation::ConversationService;
        use nomifun_conversation::skill_resolver::{ResolvedAgentSkill, SkillResolver};
        use nomifun_db::{
            SqliteAcpSessionRepository, SqliteAgentMetadataRepository,
            SqliteConversationRepository,
        };
        use nomifun_realtime::UserEventSink;

        struct Sink;
        impl UserEventSink for Sink {
            fn send_to_user(
                &self,
                _: &str,
                _: nomifun_api_types::WebSocketMessage<serde_json::Value>,
            ) {
            }
        }
        struct Skills;
        #[async_trait]
        impl SkillResolver for Skills {
            async fn auto_inject_names(&self) -> Vec<String> {
                vec![]
            }
            async fn resolve_skills(&self, _: &[String]) -> Vec<ResolvedAgentSkill> {
                vec![]
            }
            async fn link_workspace_skills(
                &self,
                _: &std::path::Path,
                _: &[&str],
                _: &[ResolvedAgentSkill],
            ) -> usize {
                0
            }
        }
        struct Runtime;
        #[async_trait]
        impl AgentRuntimeRegistry for Runtime {
            fn get_runtime(&self, _: &str) -> Option<AgentRuntimeHandle> {
                None
            }
            async fn get_or_create_runtime(
                &self,
                _: &str,
                _: AgentRuntimeBuildOptions,
            ) -> Result<AgentRuntimeHandle, AppError> {
                Err(AppError::Internal(
                    "no model execution in this persistence drill".into(),
                ))
            }
            fn terminate(&self, _: &str, _: Option<AgentKillReason>) -> Result<(), AppError> {
                Ok(())
            }
            fn terminate_all(&self) {}
            fn active_runtime_count(&self) -> usize {
                0
            }
            fn collect_idle_runtimes(&self, _: TimestampMs) -> Vec<String> {
                vec![]
            }
        }
        fn service_on_pool(
            pool: nomifun_db::sqlx::SqlitePool,
            owner: &str,
        ) -> Arc<ConversationService> {
            Arc::new(ConversationService::new(
                Arc::<str>::from(owner),
                std::env::temp_dir(),
                Arc::new(Sink),
                Arc::new(Skills),
                Arc::new(Runtime),
                Arc::new(SqliteConversationRepository::new(pool.clone())),
                Arc::new(SqliteAgentMetadataRepository::new(pool.clone())),
                Arc::new(SqliteAcpSessionRepository::new(pool)),
                Arc::new(nomifun_conversation::NoExecutionConversationBoundary),
            ))
        }
        async fn start_mock_daemon() -> String {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .unwrap();
            let port = listener.local_addr().unwrap().port();
            let app = axum::Router::new().route(
                "/api/voice/wake",
                axum::routing::post(|| async {
                    (
                        axum::http::StatusCode::OK,
                        axum::Json(serde_json::json!({ "success": true })),
                    )
                }),
            );
            tokio::spawn(async move {
                let _ = axum::serve(listener, app).await;
            });
            format!("http://127.0.0.1:{port}")
        }
        async fn boot_manager(
            _service: &Arc<ConversationService>,
            recorder: &Arc<ConversationVoiceDialogueRecorder>,
            data_dir: &std::path::Path,
            daemon_origin: String,
        ) -> Arc<VoiceCompanionProcessManager> {
            let manager = Arc::new(
                VoiceCompanionProcessManager::new(
                    Arc::new(nomifun_voice_models::ModelManager::new(data_dir)),
                    data_dir,
                )
                .with_replay_daemon(daemon_origin)
                .with_recorder(recorder.clone()),
            );
            // Safe synthetic child (non-recursive dummy process): start_call
            // requires the companion daemon to report running, while the real
            // IPC boundary is the mocked wake endpoint above.
            let builder = if cfg!(windows) {
                let mut b = nomi_process_runtime::ChildProcessBuilder::new("cmd.exe");
                b.args(["/c", "ping", "-n", "10", "127.0.0.1"]);
                b
            } else {
                let mut b = nomi_process_runtime::ChildProcessBuilder::new("sleep");
                b.arg("10");
                b
            };
            if let Some(child) = builder.spawn_managed().ok() {
                *manager.child.lock().await = Some(child);
            }
            manager
        }

        let dir = tempfile::tempdir().unwrap();
        let database_path = dir.path().join("voice-cache.sqlite");
        let database = nomifun_db::init_database(&database_path).await.unwrap();
        let owner: String = nomifun_db::sqlx::query_scalar(
            "SELECT owner_user_id FROM installation_identity",
        )
        .fetch_one(database.pool())
        .await
        .unwrap();
        let service = service_on_pool(database.pool().clone(), &owner);
        let workspace = tempfile::tempdir().unwrap();
        let frozen_workspace = nomifun_common::paths::canonicalize_simplified(workspace.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let daemon_origin = start_mock_daemon().await;

        // 1. Production start_call creates the durable identity and records a
        //    Starting receipt in the bounded in-memory cache.
        let recorder =
            Arc::new(ConversationVoiceDialogueRecorder::new(service.clone(), owner.clone(), dir.path()));
        let manager = boot_manager(&service, &recorder, dir.path(), daemon_origin.clone()).await;
        let recover_target = nomifun_api_types::voice::VoiceCallTarget::NewVoiceConversation {
            workspace: Some(frozen_workspace.clone()),
            model: None,
            preset_id: None,
        };
        let recover_request = nomifun_api_types::voice::VoiceCallStartRequest { locale: None,
            request_id: "req-cache-recover".into(),
            target: recover_target.clone(),
            session_mode: None,
        };
        let first_start = manager.start_call(recover_request.clone()).await.unwrap();
        assert_eq!(
            first_start.status,
            nomifun_api_types::voice::VoiceCallStartStatus::Starting
        );
        let retained_conversation_id = first_start.conversation_id.clone();
        service
            .append_voice_message(
                &owner,
                &retained_conversation_id,
                &MessageId::new().into_string(),
                "accepted input must survive",
                "cache-command",
            )
            .await
            .unwrap();

        // A discarded request must never be resurrected by a cache miss.
        // Simulate the 10s starting window elapsing so a different request may
        // proceed without waiting in real time.
        recorder.take_call_reservation("req-cache-recover");
        let closed_start = manager
            .start_call(nomifun_api_types::voice::VoiceCallStartRequest { locale: None,
                request_id: "req-cache-closed".into(),
                target: nomifun_api_types::voice::VoiceCallTarget::NewVoiceConversation {
                    workspace: None,
                    model: None,
                    preset_id: None,
                },
                session_mode: None,
            })
            .await
            .unwrap();
        let closed_conversation_id = closed_start.conversation_id.clone();
        assert!(
            service
                .discard_provisional_voice_conversation(&owner, &closed_conversation_id)
                .await
                .unwrap()
        );

        // A changed session mode for the same request_id must conflict.
        recorder.take_call_reservation("req-cache-closed");
        let mode_start = manager
            .start_call(nomifun_api_types::voice::VoiceCallStartRequest { locale: None,
                request_id: "req-cache-mode".into(),
                target: nomifun_api_types::voice::VoiceCallTarget::NewVoiceConversation {
                    workspace: None,
                    model: None,
                    preset_id: None,
                },
                session_mode: Some("default".into()),
            })
            .await
            .unwrap();
        let mode_conversation_id = mode_start.conversation_id.clone();
        recorder.take_call_reservation("req-cache-mode");

        // Real journal: one completed dialogue and one claim that never got a
        // model answer (the model boundary itself is simulated/absent).
        let completed_request =
            VoiceDialogueRequest::scoped("cache-session", "cache-turn", "remember this");
        let recorded_conversation = match recorder.preflight(&completed_request).await.unwrap() {
            VoiceDialoguePreflight::Ready { conversation_id } => conversation_id,
            other => panic!("expected a fresh voice binding, got {other:?}"),
        };
        recorder
            .record_pair(
                &recorded_conversation,
                &completed_request,
                r#"{"intent_type":"chat","understood_request":"remember","reply_tts":"remembered"}"#,
            )
            .await
            .unwrap();
        let unanswered_request =
            VoiceDialogueRequest::scoped("cache-session", "cache-turn-unknown", "unfinished");
        recorder.preflight(&unanswered_request).await.unwrap();

        // 2. Force the 128-entry start-receipt cache past its bound. The durable
        //    identity below must not depend on these in-memory receipts.
        let filler_reserved_at = chrono::Utc::now().timestamp_millis() + 10_000;
        for index in 0..135 {
            manager.record_handled_start_request(
                format!("req-filler-{index}"),
                HandledStartOutcome::Starting {
                    target: nomifun_api_types::voice::VoiceCallTarget::CurrentSelection,
                    session_mode: None,
                    conversation_id: format!("conv-filler-{index}"),
                    memory_owner: None,
                    // Newer than the three real receipts, so the bounded cache
                    // evicts the real ones first.
                    reserved_at_epoch_ms: filler_reserved_at + index,
                },
            );
        }
        {
            let receipts = manager.handled_start_requests.read().unwrap();
            assert_eq!(receipts.len(), 128, "start receipts must stay bounded");
            assert!(
                !receipts.contains_key("req-cache-recover"),
                "the real start receipt must be evicted"
            );
            assert!(!receipts.contains_key("req-cache-closed"));
            assert!(!receipts.contains_key("req-cache-mode"));
        }

        // 3. Real restart: drop manager/recorder/service, close SQLite, reopen
        //    the file database and journal, and rebuild all three objects.
        drop(manager);
        drop(recorder);
        drop(service);
        database.close().await;

        let reopened = nomifun_db::init_database(&database_path).await.unwrap();
        let service = service_on_pool(reopened.pool().clone(), &owner);
        let recorder =
            Arc::new(ConversationVoiceDialogueRecorder::new(service.clone(), owner.clone(), dir.path()));
        let manager = boot_manager(&service, &recorder, dir.path(), daemon_origin).await;
        assert!(
            manager.handled_start_requests.read().unwrap().is_empty(),
            "a restarted manager starts with no in-memory receipts"
        );

        // 4. Retry through the same production entrypoint with the same
        //    request_id and frozen target.
        let replay = manager.start_call(recover_request).await.unwrap();
        assert_eq!(
            replay.conversation_id, retained_conversation_id,
            "the durable creation identity must win without the evicted cache"
        );
        assert_eq!(
            replay.status,
            nomifun_api_types::voice::VoiceCallStartStatus::Starting
        );
        assert_eq!(
            recorder
                .current_call_reservation()
                .expect("the replayed start must reserve the exact conversation")
                .conversation_id,
            retained_conversation_id
        );

        let replayed_row = service
            .get(&owner, &retained_conversation_id)
            .await
            .unwrap();
        assert_eq!(
            replayed_row.extra["voice_task_workspace"], frozen_workspace,
            "replaying the frozen target must not switch workspaces"
        );
        assert_eq!(replayed_row.extra["voice_lifecycle"], "retained");
        let duplicate_conversations: i64 = nomifun_db::sqlx::query_scalar(
            "SELECT COUNT(*) FROM conversations \
             WHERE user_id = ? AND json_extract(extra, '$.voice_creation_request.request_id') = ?",
        )
        .bind(&owner)
        .bind("req-cache-recover")
        .fetch_one(reopened.pool())
        .await
        .unwrap();
        assert_eq!(
            duplicate_conversations, 1,
            "the replayed start must not create a second Conversation"
        );
        let retained_messages = service
            .list_messages(&owner, &retained_conversation_id, Default::default())
            .await
            .unwrap();
        assert_eq!(
            retained_messages
                .items
                .iter()
                .filter(|message| message.content["content"] == "accepted input must survive")
                .count(),
            1,
            "an accepted message must not be lost or duplicated"
        );

        // 5. A cache-miss replay of a discarded or reconfigured request fails
        //    closed instead of resurrecting or re-creating the Conversation.
        // Simulate the replay's starting window elapsing.
        recorder.take_call_reservation("req-cache-recover");
        let closed_error = manager
            .start_call(nomifun_api_types::voice::VoiceCallStartRequest { locale: None,
                request_id: "req-cache-closed".into(),
                target: nomifun_api_types::voice::VoiceCallTarget::NewVoiceConversation {
                    workspace: None,
                    model: None,
                    preset_id: None,
                },
                session_mode: None,
            })
            .await
            .expect_err("a discarded start request must not be resurrected");
        assert_eq!(closed_error.0, axum::http::StatusCode::CONFLICT);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&closed_error.1).unwrap()["code"],
            "conversation_already_discarded"
        );
        assert_eq!(
            service
                .get(&owner, &closed_conversation_id)
                .await
                .unwrap()
                .extra["voice_lifecycle"],
            "discarded"
        );

        let mode_error = manager
            .start_call(nomifun_api_types::voice::VoiceCallStartRequest { locale: None,
                request_id: "req-cache-mode".into(),
                target: nomifun_api_types::voice::VoiceCallTarget::NewVoiceConversation {
                    workspace: None,
                    model: None,
                    preset_id: None,
                },
                session_mode: Some("yolo".into()),
            })
            .await
            .expect_err("a changed configuration must conflict after restart");
        assert_eq!(mode_error.0, axum::http::StatusCode::CONFLICT);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&mode_error.1).unwrap()["code"],
            "voice_start_request_conflict"
        );
        assert_eq!(
            service
                .get(&owner, &mode_conversation_id)
                .await
                .unwrap()
                .extra["session_mode"],
            "default"
        );

        // 6. Journal reopen: the completed turn is served from disk without a
        //    new generation, and the unanswered claim stays unknown instead of
        //    being silently converted into a fresh model request.
        assert!(matches!(
            recorder.preflight(&completed_request).await.unwrap(),
            VoiceDialoguePreflight::Cached { content } if content.contains("remembered")
        ));
        assert_eq!(
            recorder.preflight(&unanswered_request).await.unwrap_err(),
            VoiceDialogueRecorderError::GenerationUnknown
        );
        let recorded_messages = service
            .list_messages(&owner, &recorded_conversation, Default::default())
            .await
            .unwrap();
        assert_eq!(
            recorded_messages
                .items
                .iter()
                .filter(|message| message.content["content"] == "remember this")
                .count(),
            1,
            "journal replay must not duplicate the accepted user message"
        );

        reopened.close().await;
    }

    #[test]
    fn handled_start_requests_cache_bounds_and_evicts_oldest() {
        let dir = tempfile::tempdir().unwrap();
        let manager = VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(dir.path())), dir.path());
        for i in 0..135 {
            manager.record_handled_start_request(
                format!("req-{i}"),
                HandledStartOutcome::Starting {
                    target: VoiceCallTarget::CurrentSelection,
                    session_mode: None,
                    conversation_id: format!("conv-{i}"),
                    memory_owner: None,
                    reserved_at_epoch_ms: 1000 + i as i64,
                },
            );
        }
        let map = manager.handled_start_requests.read().unwrap();
        assert_eq!(map.len(), 128);
        assert!(!map.contains_key("req-0"));
        assert!(!map.contains_key("req-1"));
        assert!(!map.contains_key("req-6"));
        assert!(map.contains_key("req-7"));
        assert!(map.contains_key("req-134"));
    }

    #[test]
    fn voice_view_departure_preserves_target_and_fences_delayed_full_context() {
        let dir = tempfile::tempdir().unwrap();
        let manager = VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(dir.path())), dir.path());
        manager.report_window_context(VoiceWindowContext {sequence: 10, conversation_id: Some("project-a".into()), workspace: Some("workspace-a".into())});
        manager.report_voice_view(None, 12);
        assert_eq!(manager.viewed_conversation.read().unwrap().clone(), (12, None));
        assert_eq!(manager.active_window_candidate.read().unwrap().conversation_id.as_deref(), Some("project-a"));
        manager.report_window_context(VoiceWindowContext {sequence: 11, conversation_id: Some("stale-project".into()), workspace: Some("stale-workspace".into())});
        assert_eq!(manager.viewed_conversation.read().unwrap().clone(), (12, None));
        assert_eq!(manager.active_window_candidate.read().unwrap().conversation_id.as_deref(), Some("project-a"), "an older full report must not cross the newer view-only sequence fence");
        manager.report_window_context(VoiceWindowContext {sequence: 13, conversation_id: Some("project-b".into()), workspace: Some("workspace-b".into())});
        assert_eq!(manager.viewed_conversation.read().unwrap().clone(), (13, Some("project-b".into())));
        assert_eq!(manager.active_window_candidate.read().unwrap().workspace.as_deref(), Some("workspace-b"));
    }

    #[test]
    fn voice_natural_end_retires_unbound_request_without_resurrecting_or_retiring_a_newer_request() {
        let dir = tempfile::tempdir().unwrap();
        let manager = VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(dir.path())), dir.path());
        for request in ["unbound", "newer"] {
            manager.record_handled_start_request(request.into(), HandledStartOutcome::Starting {
                target: VoiceCallTarget::CurrentSelection, session_mode: None, conversation_id: "voice".into(),
                memory_owner: None, reserved_at_epoch_ms: 1,
            });
        }
        manager.replay_acoustic_source(VoiceSessionSnapshot {
            instance_id: "daemon".into(), seq: 1, session_id: Some("session-unbound".into()),
            call_request_id: Some("unbound".into()), phase: VoiceSessionPhase::Idle,
            ended_reason: Some(nomifun_api_types::voice::VoiceSessionEndReason::FollowUpSilence), ..Default::default()
        });
        manager.mark_request_bound("unbound", "late-session", "voice", None);
        let receipts = manager.handled_start_requests.read().unwrap();
        assert!(matches!(receipts.get("unbound"), Some(HandledStartOutcome::Stopped { session_id: Some(session), .. }) if session == "session-unbound"));
        assert!(matches!(receipts.get("newer"), Some(HandledStartOutcome::Starting { .. })));
    }

    #[test]
    fn voice_natural_end_retires_only_its_bound_start_receipt() {
        let dir=tempfile::tempdir().unwrap();
        let manager=VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),dir.path());
        for (request,session) in [("old-request","old-session"),("new-request","new-session")] {
            manager.record_handled_start_request(request.into(),HandledStartOutcome::Bound {
                target:VoiceCallTarget::CurrentSelection,session_mode:None,conversation_id:"voice".into(),
                session_id:Some(session.into()),memory_owner:None,bound_at_epoch_ms:1,
            });
        }
        manager.replay_acoustic_source(VoiceSessionSnapshot {instance_id:"daemon".into(),seq:2,session_id:Some("new-session".into()),
            call_request_id:Some("new-request".into()),phase:VoiceSessionPhase::Listening,..Default::default()});
        manager.replay_acoustic_source(VoiceSessionSnapshot {instance_id:"daemon".into(),seq:1,session_id:Some("old-session".into()),
            ended_reason:Some(nomifun_api_types::voice::VoiceSessionEndReason::FollowUpSilence),..Default::default()});
        assert!(matches!(manager.handled_start_requests.read().unwrap().get("new-request"),Some(HandledStartOutcome::Bound {..})));
        manager.replay_acoustic_source(VoiceSessionSnapshot {instance_id:"daemon".into(),seq:3,session_id:Some("new-session".into()),
            ended_reason:Some(nomifun_api_types::voice::VoiceSessionEndReason::FollowUpSilence),..Default::default()});
        let receipts=manager.handled_start_requests.read().unwrap();
        assert!(matches!(receipts.get("new-request"),Some(HandledStartOutcome::Stopped {..})));
        assert!(matches!(receipts.get("old-request"),Some(HandledStartOutcome::Bound {..})));
    }
    #[tokio::test]
    #[ignore = "requires feature-enabled daemon binary, read-only models, isolated port and env-set fault injection"]
    async fn supervised_native_stuck_exits_and_exhausts_restart_budget() {
        verify_feature_headless_binary();
        let models_dir = std::env::var_os("FLOWY_VOICE_R11_MODELS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("D:/tmp/flowy-dev/models"));
        assert!(models_dir.join("asr/model.int8.onnx").is_file(), "real read-only model suite required");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(
                Arc::new(nomifun_voice_models::ModelManager::new(&models_dir)),
                dir.path(),
            )
            .with_test_daemon_port(port),
        );
        run_headless_case(
            manager.clone(),
            port,
            std::time::Duration::from_secs(20),
            std::time::Duration::from_secs(180),
            |manager, origins, token| async move {
                let client = reqwest::Client::builder().no_proxy().timeout(std::time::Duration::from_secs(3)).build().unwrap();
                let mut exhausted = false;
                for round in 0..5u32 {
                    let mut running = false;
                    let mut current_pid = 0u32;
                    for _ in 0..100 {
                        let status = manager.status().await;
                        if status.running { running = true; current_pid = status.pid.unwrap_or(0); break; }
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    }
                    if !running {
                        if manager.snapshot().error.as_ref().is_some_and(|error| error.code == "daemon_restart_exhausted") { exhausted = true; break; }
                        panic!("round {round}: daemon did not become running");
                    }
                    let mut healthy = false;
                    for _ in 0..80 {
                        if client.get(format!("{origins}/healthz")).send().await.is_ok_and(|r| r.status().is_success()) { healthy = true; break; }
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    }
                    assert!(healthy, "round {round}: daemon did not become healthy");
                    let hardware: serde_json::Value = client.get(format!("{origins}/api/voice/diagnostics/test-hardware-calls")).header("x-ai-mate-control-token", &token).send().await.unwrap().json().await.unwrap();
                    assert_eq!(hardware["mic_attempts"], serde_json::json!(0));
                    assert_eq!(hardware["speaker_attempts"], serde_json::json!(0));
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    let response = client.get(format!("{origins}/api/voice/diagnostics/engine-self-check?request_id=r16-{round}&timeout_ms=500")).header("x-ai-mate-control-token", &token).send().await;
                    let status = response.as_ref().ok().map(|response| response.status());
                    let body: serde_json::Value = match response { Ok(response) => response.json().await.unwrap_or_default(), Err(_) => serde_json::Value::Null };
                    assert_eq!(status, Some(reqwest::StatusCode::OK), "round {round}: bounded engine check must return a failure body");
                    let reason = body.get("reason").and_then(|value| value.as_str());
                    if reason != Some("engine_check_timeout") {
                        for _ in 0..20 {
                            if !manager.status().await.running { break; }
                            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                        }
                        let observed = manager.last_child_exit().map(|exit| (exit.pid, exit.code));
                        eprintln!("r16 unexpected_native_outcome round={round} reason={reason:?} observed_exit={observed:?}");
                    }
                    assert_eq!(reason, Some("engine_check_timeout"));
                    assert_ne!(body.get("asr_ok").and_then(|value| value.as_bool()), Some(true));
                    let after: serde_json::Value = client.get(format!("{origins}/api/voice/diagnostics/test-hardware-calls")).header("x-ai-mate-control-token", &token).send().await.unwrap().json().await.unwrap();
                    assert!(after["fault_asr_entered"].as_u64().unwrap_or(0) > 0, "round {round}: blocking ASR must actually be entered");
                    let observation = &after["fault_observation"];
                    assert_eq!(observation["native_busy"], serde_json::json!(true), "round {round}: blocking job must still be owned");
                    let job = &observation["job"];
                    assert_eq!(job["request_id"], serde_json::json!(format!("r16-{round}")), "round {round}: job request attribution");
                    assert_eq!(job["worker_epoch"], body["worker_epoch"], "round {round}: job worker attribution");
                    assert_eq!(job["instance_id"], body["instance_id"], "round {round}: job instance attribution");
                    assert_eq!(job["pid"], serde_json::json!(current_pid), "round {round}: job PID attribution");
                    assert!(job["job_epoch"].as_u64().unwrap_or(0) > 0, "round {round}: job epoch missing");
                    assert_eq!(job["still_blocking"], serde_json::json!(true), "round {round}: job must remain owned until watchdog");
                    let mut stopped = false;
                    for _ in 0..20 {
                        if !manager.status().await.running { stopped = true; break; }
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    }
                    assert!(stopped, "round {round}: supervised daemon did not exit");
                    let exit = manager.last_child_exit().expect("observed child exit");
                    assert_eq!(exit.code, Some(75), "round {round}: exit must be watchdog 75, got {:?}", exit.code);
                    assert_eq!(exit.pid, current_pid, "round {round}: exit observation must bind the running PID");
                    eprintln!("r16 round={round} pid={} exit={:?} instance={} worker_epoch={} request={} job_epoch={} busy={} still_blocking={}", current_pid, exit.code, body.get("instance_id").and_then(|v| v.as_str()).unwrap_or(""), body.get("worker_epoch").and_then(|v| v.as_u64()).unwrap_or(0), job["request_id"].as_str().unwrap_or(""), job["job_epoch"].as_u64().unwrap_or(0), observation["native_busy"], job["still_blocking"]);
                    if manager.snapshot().error.as_ref().is_some_and(|error| error.code == "daemon_restart_exhausted") { exhausted = true; break; }
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
                assert!(exhausted, "restart budget must exhaust");
            },
        )
        .await;
    }
    #[tokio::test]
    #[ignore = "requires feature headless daemon; ordinary cancel must retain native lease"]
    async fn ordinary_cancel_does_not_release_native_lease() {
        verify_feature_headless_binary();
        let models_dir = std::env::var_os("FLOWY_VOICE_R11_MODELS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("D:/tmp/flowy-dev/models"));
        assert!(models_dir.join("asr/model.int8.onnx").is_file(), "real read-only model suite required");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(
                Arc::new(nomifun_voice_models::ModelManager::new(&models_dir)),
                dir.path(),
            )
            .with_test_daemon_port(port),
        );
        run_headless_case(
            manager.clone(),
            port,
            std::time::Duration::from_secs(20),
            std::time::Duration::from_secs(120),
            |manager, origins, token| async move {
                let client = reqwest::Client::builder().no_proxy().timeout(std::time::Duration::from_secs(3)).build().unwrap();
                let first_pid = manager.status().await.pid.expect("first daemon pid");
                let first_instance = manager.snapshot().instance_id.clone();
                let first = client.post(format!("{origins}/api/voice/diagnostics/test-utterance?request_id=ordinary-a")).header("x-ai-mate-control-token", &token).send().await.unwrap();
                assert_eq!(first.status(), reqwest::StatusCode::OK);
                let mut observation = serde_json::Value::Null;
                for _ in 0..40 {
                    observation = client.get(format!("{origins}/api/voice/diagnostics/test-hardware-calls")).header("x-ai-mate-control-token", &token).send().await.unwrap().json().await.unwrap();
                    if observation["fault_observation"]["job"]["request_id"] == serde_json::json!("ordinary-a")
                        && observation["fault_observation"]["job"]["still_blocking"] == serde_json::json!(true)
                    {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                assert_eq!(observation["fault_observation"]["native_busy"], serde_json::json!(true));
                assert_eq!(observation["fault_observation"]["job"]["request_id"], serde_json::json!("ordinary-a"));
                assert_eq!(observation["fault_observation"]["job"]["pid"], serde_json::json!(first_pid));
                assert_eq!(observation["fault_observation"]["job"]["still_blocking"], serde_json::json!(true));

                let second = client.post(format!("{origins}/api/voice/diagnostics/test-utterance?request_id=ordinary-b")).header("x-ai-mate-control-token", &token).send().await.unwrap();
                assert_eq!(second.status(), reqwest::StatusCode::OK);
                let after_cancel: serde_json::Value = client.get(format!("{origins}/api/voice/diagnostics/test-hardware-calls")).header("x-ai-mate-control-token", &token).send().await.unwrap().json().await.unwrap();
                assert_eq!(after_cancel["fault_observation"]["native_busy"], serde_json::json!(true));
                assert_eq!(after_cancel["fault_observation"]["job"]["request_id"], serde_json::json!("ordinary-a"));
                assert_eq!(after_cancel["fault_observation"]["job"]["still_blocking"], serde_json::json!(true));

                let mut exited = None;
                for _ in 0..240 {
                    if let Some(observed) = manager.last_child_exit()
                        && observed.pid == first_pid
                    {
                        exited = Some(observed);
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
                let exit = exited.expect("ordinary stuck daemon must exit");
                assert_eq!(exit.pid, first_pid);
                assert_eq!(exit.code, Some(75));
                let mut restarted = false;
                for _ in 0..80 {
                    let status = manager.status().await;
                    if status.running && status.pid != Some(first_pid) {
                        restarted = true;
                        let mut new_instance = String::new();
                        for _ in 0..40 {
                            if let Ok(response) = client.get(format!("{origins}/api/voice/snapshot")).header("x-ai-mate-control-token", &token).send().await
                                && let Ok(snapshot) = response.json::<serde_json::Value>().await
                                && let Some(instance) = snapshot["instance_id"].as_str()
                                && instance != first_instance
                            {
                                new_instance = instance.to_owned();
                                break;
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        }
                        assert!(!new_instance.is_empty(), "new daemon instance must be non-empty");
                        assert_ne!(new_instance, first_instance, "new daemon instance must differ");
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
                assert!(restarted, "manager must replace the stuck instance");
                eprintln!("r17 ordinary_request={} pid={} exit={:?} new_instance={} still_blocking_after_cancel={}", observation["fault_observation"]["job"]["request_id"], first_pid, exit.code, manager.snapshot().instance_id.as_str(), after_cancel["fault_observation"]["job"]["still_blocking"]);
            },
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires feature headless daemon; healthy A then healthy B must not be killed"]
    async fn healthy_ordinary_a_then_b_completes() {
        verify_feature_headless_binary();
        let models_dir = std::env::var_os("FLOWY_VOICE_R11_MODELS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("D:/tmp/flowy-dev/models"));
        assert!(models_dir.join("asr/model.int8.onnx").is_file());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(&models_dir)),
            dir.path(),
        ).with_test_daemon_port(port));
        run_headless_case(manager.clone(), port, std::time::Duration::from_secs(20), std::time::Duration::from_secs(60), |manager, origins, token| async move {
            let client = reqwest::Client::builder().no_proxy().timeout(std::time::Duration::from_secs(3)).build().unwrap();
            for request_id in ["health-a", "health-b"] {
                let response = client.post(format!("{origins}/api/voice/diagnostics/test-utterance?request_id={request_id}")).header("x-ai-mate-control-token", &token).send().await.unwrap();
                assert_eq!(response.status(), reqwest::StatusCode::OK);
                let payload: serde_json::Value = response.json().await.unwrap();
                assert_eq!(payload["started"], serde_json::json!(true));
                for _ in 0..40 {
                    let observation: serde_json::Value = client.get(format!("{origins}/api/voice/diagnostics/test-hardware-calls")).header("x-ai-mate-control-token", &token).send().await.unwrap().json().await.unwrap();
                    if observation["fault_observation"]["native_busy"] == serde_json::json!(false) { break; }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            assert!(manager.status().await.running, "healthy A/B must keep daemon alive");
            assert!(manager.last_child_exit().is_none(), "healthy A/B must not be watchdog-exited");
            eprintln!("r20 healthy_a_b still_running=true exit=None");
        }).await;
    }

    #[tokio::test]
    #[ignore = "requires feature headless daemon; cancelled A without B must retain lease"]
    async fn ordinary_cancel_without_b_retains_lease() {
        verify_feature_headless_binary();
        let models_dir = std::env::var_os("FLOWY_VOICE_R11_MODELS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("D:/tmp/flowy-dev/models"));
        assert!(models_dir.join("asr/model.int8.onnx").is_file());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(&models_dir)),
            dir.path(),
        ).with_test_daemon_port(port));
        run_headless_case(manager.clone(), port, std::time::Duration::from_secs(20), std::time::Duration::from_secs(120), |manager, origins, token| async move {
            let client = reqwest::Client::builder().no_proxy().timeout(std::time::Duration::from_secs(3)).build().unwrap();
            let first_pid = manager.status().await.pid.expect("cancel child pid");
            let response = client.post(format!("{origins}/api/voice/diagnostics/test-utterance?request_id=cancel-a")).header("x-ai-mate-control-token", &token).send().await.unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::OK);
            let mut observation = serde_json::Value::Null;
            for _ in 0..40 {
                observation = client.get(format!("{origins}/api/voice/diagnostics/test-hardware-calls")).header("x-ai-mate-control-token", &token).send().await.unwrap().json().await.unwrap();
                if observation["fault_observation"]["job"]["request_id"] == serde_json::json!("cancel-a") && observation["fault_observation"]["job"]["still_blocking"] == serde_json::json!(true) { break; }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            assert_eq!(observation["fault_observation"]["native_busy"], serde_json::json!(true));
            let cancelled = client.post(format!("{origins}/api/voice/diagnostics/test-cancel")).header("x-ai-mate-control-token", &token).send().await.unwrap();
            assert_eq!(cancelled.status(), reqwest::StatusCode::OK);
            let after: serde_json::Value = client.get(format!("{origins}/api/voice/diagnostics/test-hardware-calls")).header("x-ai-mate-control-token", &token).send().await.unwrap().json().await.unwrap();
            assert_eq!(after["fault_observation"]["native_busy"], serde_json::json!(true));
            assert_eq!(after["fault_observation"]["job"]["request_id"], serde_json::json!("cancel-a"));
            assert_eq!(after["fault_observation"]["job"]["still_blocking"], serde_json::json!(true));
            let mut exited = None;
            for _ in 0..240 {
                if let Some(observed) = manager.last_child_exit() && observed.pid == first_pid { exited = Some(observed); break; }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
            let exit = exited.expect("cancelled A without B must self-watchdog");
            assert_eq!(exit.code, Some(75));
            eprintln!("r20 cancel_without_b pid={} exit={:?} still_after_cancel={}", first_pid, exit.code, after["fault_observation"]["job"]["still_blocking"]);
        }).await;
    }

    #[tokio::test]
    #[ignore = "requires feature headless daemon; a single ordinary stuck job must self-watchdog"]
    async fn single_ordinary_stuck_exits_without_second_request() {
        verify_feature_headless_binary();
        let models_dir = std::env::var_os("FLOWY_VOICE_R11_MODELS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("D:/tmp/flowy-dev/models"));
        assert!(models_dir.join("asr/model.int8.onnx").is_file());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(&models_dir)),
            dir.path(),
        ).with_test_daemon_port(port));
        run_headless_case(
            manager.clone(),
            port,
            std::time::Duration::from_secs(20),
            std::time::Duration::from_secs(120),
            |manager, origins, token| async move {
                let client = reqwest::Client::builder().no_proxy().timeout(std::time::Duration::from_secs(3)).build().unwrap();
                let first_pid = manager.status().await.pid.expect("single child pid");
                assert!(!manager.snapshot().instance_id.is_empty());
                let first_instance = manager.snapshot().instance_id.clone();
                let response = client.post(format!("{origins}/api/voice/diagnostics/test-utterance?request_id=single-a")).header("x-ai-mate-control-token", &token).send().await.unwrap();
                assert_eq!(response.status(), reqwest::StatusCode::OK);
                let mut observation = serde_json::Value::Null;
                for _ in 0..40 {
                    observation = client.get(format!("{origins}/api/voice/diagnostics/test-hardware-calls")).header("x-ai-mate-control-token", &token).send().await.unwrap().json().await.unwrap();
                    if observation["fault_observation"]["job"]["request_id"] == serde_json::json!("single-a")
                        && observation["fault_observation"]["job"]["still_blocking"] == serde_json::json!(true)
                    { break; }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                assert_eq!(observation["fault_observation"]["job"]["request_id"], serde_json::json!("single-a"));
                assert_eq!(observation["fault_observation"]["job"]["pid"], serde_json::json!(first_pid));
                assert_eq!(observation["fault_observation"]["job"]["still_blocking"], serde_json::json!(true));
                assert_eq!(observation["fault_observation"]["native_busy"], serde_json::json!(true));
                let mut exited = None;
                for _ in 0..240 {
                    if let Some(observed) = manager.last_child_exit()
                        && observed.pid == first_pid
                    { exited = Some(observed); break; }
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
                let exit = exited.expect("single A must self-watchdog without B");
                assert_eq!(exit.pid, first_pid);
                assert_eq!(exit.code, Some(75));
                let mut new_instance = String::new();
                let mut new_pid = None;
                for _ in 0..80 {
                    let status = manager.status().await;
                    if status.running && status.pid != Some(first_pid) {
                        new_pid = status.pid;
                        if let Ok(response) = client.get(format!("{origins}/api/voice/snapshot")).header("x-ai-mate-control-token", &token).send().await
                            && let Ok(snapshot) = response.json::<serde_json::Value>().await
                            && let Some(instance) = snapshot["instance_id"].as_str()
                            && !instance.is_empty()
                            && instance != first_instance
                        { new_instance = instance.to_owned(); break; }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
                assert!(!new_instance.is_empty(), "new instance must be non-empty");
                assert_ne!(new_instance, first_instance);
                assert_eq!(new_pid, manager.status().await.pid);
                assert_ne!(new_pid, Some(first_pid));
                eprintln!("r18 single request={} pid={} exit={:?} new_pid={:?} new_instance={}", observation["fault_observation"]["job"]["request_id"], first_pid, exit.code, new_pid, new_instance);
            },
        ).await;
    }

    #[tokio::test]
    #[ignore = "intentional nonzero verifier: requires feature headless daemon; run through R14 cleanup runner"]
    async fn forced_cleanup_failure_must_exit_nonzero() {
        verify_feature_headless_binary();
        let models_dir = std::env::var_os("FLOWY_VOICE_R11_MODELS").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("D:/tmp/flowy-dev/models"));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(&models_dir)), dir.path()).with_test_daemon_port(port));
        let mode = std::env::var("R13_CLEANUP_FAILURE_MODE").unwrap_or_else(|_| "assertion".into());
        let timeout = if mode == "timeout" { std::time::Duration::from_secs(20) } else { std::time::Duration::from_secs(30) };
        run_headless_case(manager, port, std::time::Duration::from_secs(20), timeout, |_, _, _| async move {
            if mode == "timeout" {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            } else {
                panic!("forced_main_assertion");
            }
        })
        .await;
        panic!("forced_failure_must_not_return_normally");
    }

    #[tokio::test]
    #[ignore = "intentional nonzero verifier: requires feature headless daemon; health stage must fail and clean"]
    async fn forced_health_stage_failure_must_exit_nonzero() {
        verify_feature_headless_binary();
        let models_dir = std::env::var_os("FLOWY_VOICE_R11_MODELS").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("D:/tmp/flowy-dev/models"));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(&models_dir)), dir.path()).with_test_daemon_port(port));
        run_headless_case(manager, port, std::time::Duration::from_millis(1), std::time::Duration::from_secs(30), |_, _, _| async move {
            panic!("health_body_must_not_run");
        })
        .await;
        panic!("health_failure_must_not_return_normally");
    }
    #[test]
    #[ignore = "requires feature-enabled daemon binary and read-only models"]
    fn feature_daemon_without_headless_refuses_before_hardware() {
        verify_feature_headless_binary();
        let bin = std::env::var_os("VOICE_COMPANION_BIN").map(std::path::PathBuf::from).expect("feature daemon binary");
        let models_dir = std::env::var_os("FLOWY_VOICE_R11_MODELS").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("D:/tmp/flowy-dev/models"));
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        std::fs::write(&config, format!("[models]\nmodels_dir = '{}'\nauto_download = false\n", models_dir.display())).unwrap();
        let mut child = std::process::Command::new(bin)
            .arg("--config").arg(&config)
            .arg("--host").arg("127.0.0.1:0")
            .arg("--bypass-pairing")
            .env("NOMIFUN_VOICE_FAULT_BLOCK_ASR", "0")
            .env_remove("NOMIFUN_VOICE_FAULT_HEADLESS")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn feature negative control");
        let mut status = None;
        for _ in 0..50 {
            if let Some(observed) = child.try_wait().expect("wait feature negative control") {
                status = Some(observed);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let output = match status {
            Some(_) => child.wait_with_output().expect("collect feature negative output"),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("feature negative control exceeded 5s and was reaped");
            }
        };
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("native_fault_feature_requires_valid_headless"));
    }

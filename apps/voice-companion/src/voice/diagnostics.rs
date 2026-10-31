use super::*;

impl VoiceCompanionRuntime {
    pub async fn self_check_engines(
        &self,
    ) -> nomifun_api_types::voice::VoiceEngineSelfCheckResponse {
        self.self_check_engines_with_request(None, std::time::Duration::from_secs(45))
            .await
    }

    pub async fn self_check_engines_with_request(
        &self,
        request_id: Option<String>,
        timeout: std::time::Duration,
    ) -> nomifun_api_types::voice::VoiceEngineSelfCheckResponse {
        use nomifun_api_types::voice::VoiceEngineSelfCheckResponse;
        let request_id = request_id.unwrap_or_else(nomifun_common::generate_id);
        let deadline = std::time::Instant::now() + timeout;
        let snapshot = self.snapshot();
        let instance_id = snapshot.instance_id.clone();
        let worker_epoch = self.worker_epoch.load(Ordering::Acquire);
        let model_revision = self.model_manager.identity_revision();
        let failure = |reason: &str| VoiceEngineSelfCheckResponse {
            kws_ok: false,
            vad_ok: false,
            asr_ok: false,
            fixtures_available: false,
            reason: Some(reason.into()),
            kws_hits: 0,
            asr_text: None,
            request_id: request_id.clone(),
            instance_id: instance_id.clone(),
            worker_epoch,
            model_revision: model_revision.clone(),
            fixtures_source: None,
            fixtures_online: false,
            fixtures_generated_at_ms: None,
        };
        let session_active = snapshot.session_id.is_some()
            && snapshot.ended_reason.is_none()
            && snapshot.phase != nomifun_api_types::voice::VoiceSessionPhase::Idle;
        if session_active {
            return failure("voice_busy");
        }
        if self.self_check_active.load(Ordering::Acquire) {
            return failure("self_check_in_progress");
        }
        if self.engine_check_native_busy.load(Ordering::Acquire)
            || self.native_admission.has_pending()
        {
            return failure("engine_check_native_busy");
        }
        if self
            .engine_check_active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return failure("engine_check_in_progress");
        }
        let epoch = self
            .worker_epoch
            .load(Ordering::Acquire)
            .wrapping_mul(1_000_000)
            .saturating_add(nomifun_common::now_ms().max(0) as u64);
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_notify = Arc::new(Notify::new());
        *self
            .engine_check_control
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(EngineCheckControl {
            id: request_id.clone(),
            epoch,
            cancel: cancel.clone(),
            cancel_notify: cancel_notify.clone(),
        });
        let release = || {
            *self
                .engine_check_control
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = None;
            self.engine_check_active.store(false, Ordering::Release);
        };

        if worker_epoch == 0 {
            release();
            return failure("worker_not_ready");
        }
        let Some(tx) = self
            .engine_check_tx
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
        else {
            release();
            return failure("worker_unavailable");
        };
        let fixtures = match crate::self_check_audio::load_or_generate_until(
            &model_revision,
            self.tts.as_ref(),
            cancel.as_ref(),
            cancel_notify.as_ref(),
            deadline,
        )
        .await
        {
            Ok(fixtures) => fixtures,
            Err(reason) => {
                release();
                let mut response = failure(&reason);
                response.fixtures_source = Some("unavailable".into());
                return response;
            }
        };
        let fixtures_source = fixtures.source.clone();
        let fixtures_online = fixtures.online;
        let fixtures_generated_at_ms = fixtures.generated_at_ms;
        if let Some(reason) = if cancel.load(Ordering::Acquire) {
            Some("engine_check_cancelled")
        } else if std::time::Instant::now() >= deadline {
            Some("engine_check_timeout")
        } else {
            None
        } {
            release();
            let mut response = failure(reason);
            response.fixtures_available = true;
            response.fixtures_source = Some(fixtures_source);
            response.fixtures_online = fixtures_online;
            response.fixtures_generated_at_ms = fixtures_generated_at_ms;
            return response;
        }

        let (reply_tx, reply_rx) = oneshot::channel();
        let command = WorkerEngineCheck {
            request_id: request_id.clone(),
            deadline,
            cancel: cancel.clone(),
            cancel_notify: cancel_notify.clone(),
            native_busy: self.engine_check_native_busy.clone(),
            native_job_epoch: self.native_job_epoch.clone(),
            fixtures,
            reply: reply_tx,
        };
        if tokio::time::timeout(
            std::time::Duration::from_secs(2),
            tx.send(command),
        )
        .await
        .is_err()
        {
            release();
            return failure("worker_unavailable");
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let result = match tokio::time::timeout(remaining, reply_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err("worker_dropped_check".to_string()),
            Err(_) => {
                cancel.store(true, Ordering::Release);
                cancel_notify.notify_waiters();
                Err("engine_check_timeout".to_string())
            }
        };
        let current_epoch = self
            .engine_check_control
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .is_some_and(|control| control.id == request_id && control.epoch == epoch);
        let released = {
            let mut control = self
                .engine_check_control
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if control.as_ref().is_some_and(|value| value.id == request_id) {
                *control = None;
            }
            self.engine_check_active.store(false, Ordering::Release);
            true
        };
        debug_assert!(released);
        if !current_epoch {
            let mut response = failure("engine_check_superseded");
            response.fixtures_available = true;
            response.fixtures_source = Some(fixtures_source);
            response.fixtures_online = fixtures_online;
            response.fixtures_generated_at_ms = fixtures_generated_at_ms;
            return response;
        }
        if self.engine_check_native_busy.load(Ordering::Acquire) {
            self.schedule_supervised_native_recovery();
        }
        match result {
            Ok(result) if !cancel.load(Ordering::Acquire) => VoiceEngineSelfCheckResponse {
                kws_ok: result.kws_ok,
                vad_ok: result.vad_ok,
                asr_ok: result.asr_ok,
                fixtures_available: true,
                reason: if result.kws_ok && result.vad_ok && result.asr_ok {
                    None
                } else {
                    Some("engine_inference_failed".into())
                },
                kws_hits: result.kws_hits,
                asr_text: result.asr_text,
                request_id,
                instance_id,
                worker_epoch,
                model_revision,
                fixtures_source: Some(fixtures_source),
                fixtures_online,
                fixtures_generated_at_ms,
            },
            Ok(_) => {
                let mut response = failure("engine_check_cancelled");
                response.fixtures_available = true;
                response.fixtures_source = Some(fixtures_source);
                response.fixtures_online = fixtures_online;
                response.fixtures_generated_at_ms = fixtures_generated_at_ms;
                response
            }
            Err(reason) => {
                let mut response = failure(&reason);
                response.fixtures_available = true;
                response.fixtures_source = Some(fixtures_source);
                response.fixtures_online = fixtures_online;
                response.fixtures_generated_at_ms = fixtures_generated_at_ms;
                response
            }
        }
    }

    fn schedule_supervised_native_recovery(&self) {
        if std::env::var("VOICE_COMPANION_SUPERVISED").ok().as_deref() != Some("1") {
            return;
        }
        let job_epoch = self.native_job_epoch.clone();
        let job = job_epoch.load(Ordering::Acquire);
        if job == 0 || self.native_recovery_scheduled.swap(job, Ordering::AcqRel) == job {
            return;
        }
        let busy = self.engine_check_native_busy.clone();
        let scheduled = self.native_recovery_scheduled.clone();
        let grace_ms = std::env::var("VOICE_COMPANION_NATIVE_STUCK_GRACE_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(5_000)
            .clamp(250, 30_000);
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(grace_ms)).await;
            if busy.load(Ordering::Acquire) && job_epoch.load(Ordering::Acquire) == job {
                tracing::error!(
                    grace_ms,
                    "voice_engine_native_stuck_exiting_for_supervised_restart"
                );
                std::process::exit(75);
            }
            let _ = scheduled.compare_exchange(job, 0, Ordering::AcqRel, Ordering::Acquire);
        });
    }

    pub async fn cancel_engine_check(&self, request_id: &str) -> bool {
        let control = self
            .engine_check_control
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        match control {
            Some(control) if control.id == request_id => {
                control.cancel.store(true, Ordering::Release);
                control.cancel_notify.notify_waiters();
                true
            }
            _ => false,
        }
    }    /// Cancels the matching explicit TTS self-check and interrupts its output.
    pub async fn cancel_self_check(&self, check_id: &str) -> bool {
        let matches = self
            .self_check_id
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_deref()
            == Some(check_id);
        if !matches {
            return false;
        }
        self.self_check_cancel.store(true, Ordering::Release);
        let _ = self.playback.interrupt().await;
        true
    }
    pub(super) async fn worker_engine_check(
        kws: &NativeKws,
        vad: &NativeVad,
        transcriber: &Option<Arc<dyn SttCallback>>,
        fixtures: &SelfCheckAudio,
        cancel: &AtomicBool,
        cancel_notify: &Notify,
        native_busy: &Arc<AtomicBool>,
        native_degraded: &Arc<NativeHealth>,
        native_job_epoch: &Arc<AtomicU64>,
        deadline: std::time::Instant,
    ) -> Result<WorkerEngineCheckResult, String> {
        let stopped = || {
            cancel.load(Ordering::Acquire)
                || std::time::Instant::now() >= deadline
        };
        if stopped() {
            return Err("engine_check_cancelled".into());
        }
        kws.reset_until(deadline, cancel, cancel_notify, false).await?;
        let mut wake_hits = 0u32;
        for chunk in fixtures.wake.chunks(320) {
            if stopped() {
                return Err("engine_check_cancelled".into());
            }
            if kws
                .process_pcm_chunk_until(chunk, deadline, cancel, cancel_notify, false)
                .await?
                .is_some()
            {
                wake_hits = wake_hits.saturating_add(1);
            }
        }
        let mut negative_hits = 0u32;
        for pcm in [&fixtures.command, &fixtures.non_wake] {
            kws.reset_until(deadline, cancel, cancel_notify, false).await?;
            for chunk in pcm.chunks(320) {
                if stopped() {
                    return Err("engine_check_cancelled".into());
                }
                if kws
                    .process_pcm_chunk_until(chunk, deadline, cancel, cancel_notify, false)
                    .await?
                    .is_some()
                {
                    negative_hits = negative_hits.saturating_add(1);
                }
            }
        }
        let kws_ok = kws.is_initialized() && wake_hits > 0 && negative_hits == 0;

        vad.reset_until(deadline, cancel, cancel_notify, false).await?;
        let silence = vec![0.0f32; 512];
        let mut silent_start = true;
        for _ in 0..30 {
            if stopped() {
                return Err("engine_check_cancelled".into());
            }
            if vad
                .process_frame_until(&silence, deadline, cancel, cancel_notify, false)
                .await?
            {
                silent_start = false;
                break;
            }
        }
        let mut speech = false;
        for frame in fixtures.command.chunks(512) {
            if stopped() {
                return Err("engine_check_cancelled".into());
            }
            if frame.len() < 512 {
                break;
            }
            if vad
                .process_frame_until(frame, deadline, cancel, cancel_notify, false)
                .await?
            {
                speech = true;
                break;
            }
        }
        let mut ended = false;
        for _ in 0..120 {
            if stopped() {
                return Err("engine_check_cancelled".into());
            }
            if !vad
                .process_frame_until(&silence, deadline, cancel, cancel_notify, false)
                .await?
            {
                ended = true;
                break;
            }
        }
        let vad_ok = silent_start && speech && ended;
        vad.reset_until(deadline, cancel, cancel_notify, false).await?;

        let (asr_ok, asr_text) = match transcriber {
            Some(asr) => {
                let zh = Self::transcribe_cancelable(
                    asr.clone(),
                    fixtures.command.clone(),
                    cancel,
                    cancel_notify,
                    native_busy,
                    native_degraded,
                    native_job_epoch,
                    deadline,
                    true,
                    None,
                    kws.runner.admission.as_ref(),
                    #[cfg(feature = "native-fault-injection")]
                    None,
                )
                .await?
                .unwrap_or_default();
                if stopped() {
                    return Err("engine_check_cancelled".into());
                }
                let en = Self::transcribe_cancelable(
                    asr.clone(),
                    fixtures.non_wake.clone(),
                    cancel,
                    cancel_notify,
                    native_busy,
                    native_degraded,
                    native_job_epoch,
                    deadline,
                    true,
                    None,
                    kws.runner.admission.as_ref(),
                    #[cfg(feature = "native-fault-injection")]
                    None,
                )
                .await?
                .unwrap_or_default();
                let lowered = en.to_lowercase();
                (
                    zh.contains("天气") && lowered.contains("hello") && lowered.contains("friend"),
                    Some(format!("zh={zh};en={en}")),
                )
            }
            None => (false, None),
        };
        if stopped() {
            return Err("engine_check_cancelled".into());
        }
        Ok(WorkerEngineCheckResult {
            kws_ok,
            vad_ok,
            asr_ok,
            kws_hits: wake_hits,
            asr_text,
        })
    }
    pub async fn self_check_tts(
        &self,
    ) -> Result<nomifun_api_types::voice::VoiceTtsSelfCheckResponse, String> {
        self.self_check_tts_inner(None, None, None).await
    }

    pub async fn self_check_tts_with_id(
        &self,
        check_id: &str,
    ) -> Result<nomifun_api_types::voice::VoiceTtsSelfCheckResponse, String> {
        self.self_check_tts_inner(Some(check_id), None, None).await
    }

    pub async fn self_check_tts_with_options(
        &self,
        check_id: Option<&str>,
        locale: Option<&str>,
        stage: Option<&str>,
    ) -> Result<nomifun_api_types::voice::VoiceTtsSelfCheckResponse, String> {
        self.self_check_tts_inner(check_id, locale, stage).await
    }

    async fn self_check_tts_inner(
        &self,
        check_id: Option<&str>,
        locale: Option<&str>,
        stage: Option<&str>,
    ) -> Result<nomifun_api_types::voice::VoiceTtsSelfCheckResponse, String> {
        if self.engine_check_active.load(Ordering::Acquire) {
            return Err("engine_check_in_progress".into());
        }
        if self.self_check_active.swap(true, Ordering::AcqRel) {
            return Err("self_check_in_progress".into());
        }
        let _self_check_guard = SelfCheckGuard {
            active: self.self_check_active.clone(),
            cancel: self.self_check_cancel.clone(),
            id: self.self_check_id.clone(),
        };
        self.self_check_cancel.store(false, Ordering::Release);
        *self
            .self_check_id
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = check_id.map(str::to_owned);
        let snapshot = self.snapshot();
        let active = snapshot.session_id.is_some()
            && snapshot.ended_reason.is_none()
            && snapshot.phase != nomifun_api_types::voice::VoiceSessionPhase::Idle;
        if active {
            return Err("voice_busy".into());
        }
        let is_en = nomifun_api_types::voice::is_english_locale(locale);
        let (zh_text, en_text) = match stage {
            Some("task") | Some("create_task") => ("创建语音任务中", "Creating voice task..."),
            Some("prepare") | Some("preparing") => ("正在准备语音通话。", "Preparing your voice call."),
            Some("connect") | Some("connecting") => ("正在连接语音通话。", "Connecting voice call."),
            Some("start") | Some("starting") => ("正在启动语音服务。", "Starting voice service."),
            _ => ("正在启动语音服务。", "Starting voice service."),
        };
        let cases = [
            ("zh-CN", "zh-CN-XiaoxiaoNeural", zh_text),
            ("en-US", "en-US-JennyNeural", en_text),
        ];
        let mut languages = Vec::new();
        let mut playback_ok = false;
        let mut first_error = None;
        let is_muted = !self.event_tx.feedback().speak_replies;
        for (language, voice, text) in cases {
            if self.self_check_cancel.load(Ordering::Acquire) {
                return Err("self_check_cancelled".into());
            }
            let play_this = if is_en { language == "en-US" } else { language == "zh-CN" };
            let result: Result<(u32, usize, u64, u16, u16), String> = async {
                let mut stream = self
                    .tts
                    .synthesize_stream(text, voice)
                    .await
                    .map_err(|error| error.to_string())?;
                let wav = tokio::time::timeout(
                    std::time::Duration::from_secs(25),
                    stream.recv(),
                )
                .await
                .map_err(|_| "tts_timeout".to_string())?
                .ok_or_else(|| "tts_no_audio".to_string())?;
                let (pcm, rate) = nomifun_audio::parse_audio_frame_to_pcm(&wav);
                let (duration_ms, rms_milli, peak_milli) = validate_self_check_pcm(&pcm, rate)?;
                if self.self_check_cancel.load(Ordering::Acquire) {
                    return Err("self_check_cancelled".into());
                }
                if play_this {
                    if is_muted {
                        let silent_pcm = vec![0.0f32; pcm.len()];
                        let silent_wav = nomifun_audio::pcm_to_wav(&silent_pcm, rate);
                        self.playback
                            .play_wav_for_self_check(silent_wav)
                            .await
                            .map_err(|error| error.to_string())?;
                    } else {
                        self.playback
                            .play_wav_for_self_check(wav)
                            .await
                            .map_err(|error| error.to_string())?;
                    }
                    playback_ok = true;
                }
                Ok((rate, pcm.len(), duration_ms, rms_milli, peak_milli))
            }
            .await;
            match result {
                Ok((sample_rate, samples, duration_ms, rms_milli, peak_milli)) => {
                    languages.push(nomifun_api_types::voice::VoiceTtsSelfCheckLanguage {
                        language: language.into(),
                        voice: voice.into(),
                        ok: true,
                        sample_rate,
                        samples,
                        duration_ms,
                        rms_milli,
                        peak_milli,
                        error: None,
                    });
                }
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error.clone());
                    }
                    languages.push(nomifun_api_types::voice::VoiceTtsSelfCheckLanguage {
                        language: language.into(),
                        voice: voice.into(),
                        ok: false,
                        sample_rate: 0,
                        samples: 0,
                        duration_ms: 0,
                        rms_milli: 0,
                        peak_milli: 0,
                        error: Some(error),
                    });
                }
            }
        }
        let diagnostic = self.tts.diagnostic("");
        let provider = diagnostic
            .as_ref()
            .map(|value| value.provider.clone())
            .unwrap_or_else(|| "unknown".into());
        let online = diagnostic.as_ref().is_some_and(|value| value.online);
        let ok = languages.iter().all(|language| language.ok);
        Ok(nomifun_api_types::voice::VoiceTtsSelfCheckResponse {
            ok,
            provider,
            online,
            playback_ok,
            languages,
            reason: if ok { None } else { first_error },
        })
    }
}

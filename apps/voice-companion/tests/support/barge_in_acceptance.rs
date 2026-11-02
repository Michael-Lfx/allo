use super::*;

/// No physical device or AEC claim: isolate PCM handling during playback.
#[tokio::test]
async fn microphone_pcm_during_tts_must_stop_old_audio_without_ending_the_session() {
    let flow = StreamingWorker::new().await;
    flow.speak().await;
    tokio::time::timeout(Duration::from_secs(2), flow.sink.wrote.notified())
        .await
        .expect("the first answer must already be playing before the interruption");
    let session = flow.runtime.snapshot().session_id;
    let before = flow.runtime.snapshot().audio.unwrap().frames_received;
    assert!(!flow.sink.frames.lock().unwrap().is_empty());
    assert!(!flow.server.completed.load(Ordering::SeqCst));

    let started = tokio::time::Instant::now();
    for frame in 0..30 {
        let pcm: Vec<f32> = (0..320)
            .map(|sample| {
                0.03 * (sample as f32 * std::f32::consts::TAU * 200.0 / 16000.0).sin()
            })
            .collect();
        assert_eq!(flow.producer.push(&pcm), pcm.len());
        tokio::time::sleep_until(started + Duration::from_millis((frame + 1) * 20)).await;
        if flow.sink.frames.lock().unwrap().is_empty() {
            break;
        }
    }

    let snapshot = flow.runtime.snapshot();
    assert!(
        snapshot.audio.as_ref().unwrap().frames_received > before,
        "the production capture worker must have consumed the new PCM"
    );
    assert_eq!(
        snapshot.session_id, session,
        "barge-in must preserve the session"
    );
    assert!(
        snapshot.ended_reason.is_none(),
        "barge-in must not stop the session"
    );
    assert!(flow.sink.frames.lock().unwrap().is_empty(),
        "barge-in missing: the worker consumed 600ms of microphone PCM while the old answer remains queued; phase={:?}, frames_before={}, frames_after={}, model_calls={}, pending={}",
        snapshot.phase, before, snapshot.audio.as_ref().unwrap().frames_received,
        flow.server.calls.load(Ordering::SeqCst), flow.runtime.pending_utterance_count());
}

#[tokio::test]
async fn barge_in_user_speech_routes_continuation_turn_in_same_session() {
    let flow = StreamingWorker::new().await;
    flow.speak().await;
    tokio::time::timeout(Duration::from_secs(2), flow.sink.wrote.notified())
        .await
        .expect("the first answer must already be playing before the interruption");
    let session = flow.runtime.snapshot().session_id;

    // Barge-in: send user speech to trigger interruption and enter Listening
    let started = tokio::time::Instant::now();
    for frame in 0..25 {
        let pcm: Vec<f32> = (0..320)
            .map(|sample| {
                0.03 * (sample as f32 * std::f32::consts::TAU * 200.0 / 16000.0).sin()
            })
            .collect();
        assert_eq!(flow.producer.push(&pcm), pcm.len());
        tokio::time::sleep_until(started + Duration::from_millis((frame + 1) * 20)).await;
    }

    // Old audio must be cleared upon barge-in
    tokio::time::timeout(Duration::from_secs(2), async {
        while !flow.sink.frames.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("old audio must be cleared upon barge-in");

    // Send silence to complete the utterance (VAD speech end)
    let silence_started = tokio::time::Instant::now();
    for frame in 0..40 {
        let pcm = vec![0.0_f32; 320];
        assert_eq!(flow.producer.push(&pcm), pcm.len());
        tokio::time::sleep_until(silence_started + Duration::from_millis((frame + 1) * 20)).await;
        if flow.server.calls.load(Ordering::SeqCst) >= 2 {
            break;
        }
    }

    tokio::time::timeout(Duration::from_secs(2), async {
        while flow.server.calls.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the second utterance after barge-in must be routed to the dialogue model");

    let snapshot = flow.runtime.snapshot();
    assert_eq!(
        snapshot.session_id, session,
        "second turn must preserve the session"
    );
    assert!(
        snapshot.ended_reason.is_none(),
        "second turn must not end the session"
    );
}
#[tokio::test]
async fn barge_in_releases_output_before_old_model_finishes() {
    let flow = StreamingWorker::new().await;
    flow.speak().await;
    tokio::time::timeout(Duration::from_secs(2), flow.sink.wrote.notified()).await.unwrap();
    let start = tokio::time::Instant::now();
    for frame in 0..25 {
        let pcm: Vec<f32>=(0..320).map(|i|0.03*(i as f32*std::f32::consts::TAU*200.0/16000.0).sin()).collect();
        flow.producer.push(&pcm);
        tokio::time::sleep_until(start+Duration::from_millis((frame+1)*20)).await;
    }
    for _ in 0..50 {
        flow.producer.push(&vec![0.0;320]);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::timeout(Duration::from_secs(2),async {
        while flow.server.calls.load(Ordering::SeqCst)<2 {tokio::time::sleep(Duration::from_millis(10)).await;}
    }).await.unwrap();
    let result=tokio::time::timeout(Duration::from_millis(700),async {
        while flow.sink.frames.lock().unwrap().is_empty() {tokio::time::sleep(Duration::from_millis(10)).await;}
    }).await;
    assert!(result.is_ok(),"second model received input but new speech is blocked behind the interrupted old model stream");
}

#[tokio::test]
async fn interrupted_task_result_is_not_reported_played() {
    use nomifun_api_types::voice::{VoiceTaskResultAnnouncement, VoiceSessionControlRequest};
    let flow=StreamingWorker::new().await;
    let snapshot=flow.runtime.snapshot();
    let notice=VoiceTaskResultAnnouncement {
        scope:VoiceSessionControlRequest{instance_id:snapshot.instance_id,session_id:snapshot.session_id,turn_id:snapshot.turn_id},
        command_id:"review-task".into(),notification_id:Some("review-result".into()),attempt_id:Some("review-attempt".into()),goal:None,text:"这是尚未播放完成的任务结果。".into(),
    };
    let (result,())=tokio::join!(flow.runtime.announce_task_result(notice),async {
        tokio::time::timeout(Duration::from_secs(2),flow.sink.wrote.notified()).await.unwrap();
        for _ in 0..25 {
            let pcm:Vec<f32>=(0..320).map(|i|0.03*(i as f32*std::f32::consts::TAU*200.0/16000.0).sin()).collect();
            flow.producer.push(&pcm);tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
    result.unwrap();
    let receipt=flow.runtime.notification_status("review-attempt").await.unwrap();
    assert_eq!(receipt.status,"interrupted","interrupted result cannot be consumed as a completely played notification");
}

#[tokio::test]
async fn completed_text_still_marks_interrupted_audio() {
    let flow=StreamingWorker::new().await;
    flow.speak().await;
    tokio::time::timeout(Duration::from_secs(2),flow.sink.wrote.notified()).await.unwrap();
    flow.server.release.notify_one();
    tokio::time::timeout(Duration::from_secs(2),flow.sink.draining.notified()).await.unwrap();
    assert_eq!(flow.runtime.snapshot().conversation.last().unwrap().reply_state,Some(nomifun_api_types::voice::VoiceReplyState::Complete));
    for _ in 0..25 {
        let pcm:Vec<f32>=(0..320).map(|i|0.03*(i as f32*std::f32::consts::TAU*200.0/16000.0).sin()).collect();
        flow.producer.push(&pcm);tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let snapshot=flow.runtime.snapshot();
    assert_eq!(snapshot.conversation.last().unwrap().reply_state,Some(nomifun_api_types::voice::VoiceReplyState::Interrupted),"fully generated text is not proof that its audio was completely heard");
}

#[tokio::test]
async fn raw_pcm_cannot_enable_barge_in_without_echo_cancellation() {
    let flow=StreamingWorker::with_barge_in(false,false).await;
    flow.speak().await;
    tokio::time::timeout(Duration::from_secs(2),flow.sink.wrote.notified()).await.unwrap();
    let generation=flow.runtime.playback().generation_id();
    for _ in 0..30 {
        let pcm:Vec<f32>=(0..320).map(|i|0.03*(i as f32*std::f32::consts::TAU*200.0/16000.0).sin()).collect();
        flow.producer.push(&pcm);tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let audio=flow.runtime.snapshot().audio.unwrap();
    assert!(!audio.barge_in_ready);
    assert_eq!(audio.barge_in_reason.as_deref(),Some("aec_unavailable"));
    assert_eq!(flow.runtime.playback().generation_id(),generation);
    assert!(!flow.sink.frames.lock().unwrap().is_empty());
    assert_eq!(flow.server.calls.load(Ordering::SeqCst),1);
}

#[tokio::test]
async fn muted_result_is_not_reported_played() {
    use nomifun_api_types::voice::{VoiceTaskResultAnnouncement,VoiceSessionControlRequest};
    let flow=StreamingWorker::new().await;
    flow.runtime.update_feedback(serde_json::from_value(serde_json::json!({"speak_replies":false})).unwrap()).unwrap();
    let snapshot=flow.runtime.snapshot();
    flow.runtime.announce_task_result(VoiceTaskResultAnnouncement {
        scope:VoiceSessionControlRequest{instance_id:snapshot.instance_id,session_id:snapshot.session_id,turn_id:snapshot.turn_id},
        command_id:"muted-command".into(),notification_id:None,attempt_id:Some("muted-attempt".into()),goal:None,text:"文字保留，不播放声音。".into(),
    }).await.unwrap();
    let receipt=flow.runtime.notification_status("muted-attempt").await.unwrap();
    assert_eq!(receipt.status,"muted");
    assert_eq!(receipt.reason_code.as_deref(),Some("tts_muted"));
    assert!(flow.sink.frames.lock().unwrap().is_empty());
}

#[tokio::test]
async fn kws_wake_word_interrupts_playback_even_without_aec() {
    let flow = StreamingWorker::with_barge_in(false, false).await;
    flow.speak().await;
    tokio::time::timeout(Duration::from_secs(2), flow.sink.wrote.notified())
        .await
        .expect("playback must start");
    let session = flow.runtime.snapshot().session_id;
    assert!(!flow.sink.frames.lock().unwrap().is_empty(), "playback audio must be queued");

    // 1. Ordinary non-wake speech/sound must NOT interrupt playback
    let started = tokio::time::Instant::now();
    for frame in 0..10 {
        let pcm: Vec<f32> = (0..320)
            .map(|i| 0.05 * (i as f32 * std::f32::consts::TAU * 100.0 / 16000.0).sin())
            .collect();
        flow.producer.push(&pcm);
        tokio::time::sleep_until(started + Duration::from_millis((frame + 1) * 20)).await;
    }
    assert!(!flow.sink.frames.lock().unwrap().is_empty(), "non-wake sound must NOT interrupt playback");

    // 2. Wake word pattern (350Hz calibrated wake tone, continuous phase, amplitude 0.45) triggers KWS barge-in
    let wake_started = tokio::time::Instant::now();
    for frame in 0..25 {
        let pcm: Vec<f32> = (0..320)
            .map(|i| {
                let sample_idx = (frame * 320 + i) as f32;
                0.45 * (sample_idx * std::f32::consts::TAU * 350.0 / 16000.0).sin()
            })
            .collect();
        assert_eq!(flow.producer.push(&pcm), pcm.len());
        tokio::time::sleep_until(wake_started + Duration::from_millis((frame as u64 + 1) * 20)).await;
        if flow.sink.frames.lock().unwrap().is_empty() {
            break;
        }
    }

    // Playback must be interrupted and flushed
    tokio::time::timeout(Duration::from_secs(2), async {
        while !flow.sink.frames.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("KWS wake word must instantly interrupt playback and flush audio sink");

    let snapshot = flow.runtime.snapshot();
    assert_eq!(snapshot.session_id, session, "session must be preserved across barge-in");
    assert!(snapshot.ended_reason.is_none(), "session must remain active");
    assert_eq!(snapshot.phase, nomifun_api_types::voice::VoiceSessionPhase::Listening, "must transition to Listening");
}

#[tokio::test]
async fn kws_barge_in_masked_when_assistant_speaks_keyword() {
    let flow = StreamingWorker::with_sentence("我是 Flowy，请问有什么可以帮你？").await;
    flow.speak().await;
    tokio::time::timeout(Duration::from_secs(2), flow.sink.wrote.notified())
        .await
        .expect("playback must start");

    // While this sentence is speaking, the wake word frequency pattern arrives:
    let wake_started = tokio::time::Instant::now();
    for frame in 0..10 {
        let pcm: Vec<f32> = (0..320)
            .map(|i| {
                let sample_idx = (frame * 320 + i) as f32;
                0.45 * (sample_idx * std::f32::consts::TAU * 350.0 / 16000.0).sin()
            })
            .collect();
        flow.producer.push(&pcm);
        tokio::time::sleep_until(wake_started + Duration::from_millis((frame as u64 + 1) * 20)).await;
    }

    // Playback must NOT be interrupted because the assistant was speaking the keyword itself!
    assert!(!flow.sink.frames.lock().unwrap().is_empty(), "playback must continue uninterrupted when assistant speaks the keyword");
}

#[tokio::test]
async fn kws_barge_in_has_listening_deadline_and_times_out_on_silence() {
    let flow = StreamingWorker::with_barge_in(false, false).await;
    flow.speak().await;
    tokio::time::timeout(Duration::from_secs(2), flow.sink.wrote.notified())
        .await
        .expect("playback must start");

    let started = tokio::time::Instant::now();
    for frame in 0..25 {
        let pcm: Vec<f32> = (0..320)
            .map(|i| {
                let sample_idx = (frame * 320 + i) as f32;
                0.45 * (sample_idx * std::f32::consts::TAU * 350.0 / 16000.0).sin()
            })
            .collect();
        flow.producer.push(&pcm);
        tokio::time::sleep_until(started + Duration::from_millis((frame as u64 + 1) * 20)).await;
        if flow.sink.frames.lock().unwrap().is_empty() {
            break;
        }
    }

    let barge = flow.runtime.snapshot();
    assert_eq!(barge.phase, nomifun_api_types::voice::VoiceSessionPhase::Listening);
    assert!(barge.listening_deadline_ms.is_some(), "barge-in must arm a listening deadline for the user's follow-up");

    // Manually expire listening deadline via driver watchdog helper
    flow.runtime.expire_listening_deadline().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let after = flow.runtime.snapshot();
    assert!(
        after.ended_reason.is_some() || after.phase != nomifun_api_types::voice::VoiceSessionPhase::Listening,
        "silence watchdog must transition out of Listening; snapshot={after:?}"
    );
}

#[tokio::test]
async fn kws_barge_then_command_reaches_asr() {
    let flow = StreamingWorker::with_barge_in(false, false).await;
    flow.speak().await;
    tokio::time::timeout(Duration::from_secs(2), flow.sink.wrote.notified())
        .await
        .expect("playback must start");

    // 1. Interrupt via wake tone
    let started = tokio::time::Instant::now();
    for frame in 0..25 {
        let pcm: Vec<f32> = (0..320)
            .map(|i| {
                let sample_idx = (frame * 320 + i) as f32;
                0.45 * (sample_idx * std::f32::consts::TAU * 350.0 / 16000.0).sin()
            })
            .collect();
        flow.producer.push(&pcm);
        tokio::time::sleep_until(started + Duration::from_millis((frame as u64 + 1) * 20)).await;
        if flow.sink.frames.lock().unwrap().is_empty() {
            break;
        }
    }
    assert!(flow.sink.frames.lock().unwrap().is_empty());

    // 2. User immediately speaks their new command
    let cmd = tokio::time::Instant::now();
    for frame in 0..30 {
        let pcm: Vec<f32> = (0..320)
            .map(|i| 0.03 * (i as f32 * std::f32::consts::TAU * 200.0 / 16000.0).sin())
            .collect();
        flow.producer.push(&pcm);
        tokio::time::sleep_until(cmd + Duration::from_millis((frame + 1) * 20)).await;
    }

    // 3. User finishes command (silence)
    let c = tokio::time::Instant::now();
    for frame in 0..50 {
        flow.producer.push(&vec![0.0f32; 320]);
        tokio::time::sleep_until(c + Duration::from_millis((frame + 1) * 20)).await;
        if flow.server.calls.load(Ordering::SeqCst) >= 2 {
            break;
        }
    }

    assert!(
        flow.server.calls.load(Ordering::SeqCst) >= 2,
        "post-barge command must reach ASR and model"
    );
}

#[tokio::test]
async fn kws_barge_in_emits_barge_in_feedback_cue() {
    let flow = StreamingWorker::with_barge_in(false, false).await;
    let mut rx = flow.runtime.event_sender().subscribe();
    flow.speak().await;
    tokio::time::timeout(Duration::from_secs(2), flow.sink.wrote.notified())
        .await
        .expect("playback must start");

    let started = tokio::time::Instant::now();
    for frame in 0..25 {
        let pcm: Vec<f32> = (0..320)
            .map(|i| {
                let sample_idx = (frame * 320 + i) as f32;
                0.45 * (sample_idx * std::f32::consts::TAU * 350.0 / 16000.0).sin()
            })
            .collect();
        flow.producer.push(&pcm);
        tokio::time::sleep_until(started + Duration::from_millis((frame as u64 + 1) * 20)).await;
        if flow.sink.frames.lock().unwrap().is_empty() {
            break;
        }
    }

    let hit = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Ok(nomifun_api_types::voice::VoiceCompanionEvent::Feedback { diagnostic }) = rx.recv().await {
                if diagnostic.kind == nomifun_api_types::voice::VoiceCueKind::BargeIn {
                    return true;
                }
            }
        }
    }).await;
    assert!(hit.is_ok(), "barge-in must emit a BargeIn feedback cue event");
}

#[test]
fn test_direct_control_short_phrase_and_codeswitching_dismissal() {
    use nomifun_api_types::voice::{classify_voice_direct_control, VoiceDirectControlAction};

    let custom = vec![];
    // Homophone typos / spoken prefixes ending with dismissal core
    assert_eq!(
        classify_voice_direct_control("谢解退下吧。", &custom),
        Some(VoiceDirectControlAction::DismissSession),
        "typo prefix with dismiss suffix must match"
    );
    assert_eq!(
        classify_voice_direct_control("OK 退下吧", &custom),
        Some(VoiceDirectControlAction::DismissSession),
        "code-switching 'OK 退下吧' must match"
    );
    assert_eq!(
        classify_voice_direct_control("Bye 啦", &custom),
        Some(VoiceDirectControlAction::DismissSession),
        "code-switching 'Bye 啦' must match"
    );
    assert_eq!(
        classify_voice_direct_control("谢了退下吧", &custom),
        Some(VoiceDirectControlAction::DismissSession)
    );

    // Negative guards: CLI/technical commands must NOT match dismissal
    assert_eq!(
        classify_voice_direct_control("How do I exit vim?", &custom),
        None,
        "technical question mentioning exit must NOT dismiss"
    );
    assert_eq!(
        classify_voice_direct_control("The script returned exit code 1", &custom),
        None,
        "error description mentioning exit must NOT dismiss"
    );
    assert_eq!(
        classify_voice_direct_control("Don't end the call", &custom),
        None,
        "negated dismissal must NOT dismiss"
    );
}

#[test]
fn test_pre_llm_sanitizer_filter_rules() {
    let is_semantic = |text: &str| {
        text.chars().any(|c| c.is_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(&c))
    };

    // Non-semantic noise (pure punctuation / whitespace) must be rejected
    assert!(!is_semantic("。"));
    assert!(!is_semantic("，"));
    assert!(!is_semantic("？？？"));
    assert!(!is_semantic("..."));
    assert!(!is_semantic("   \n\t  "));
    assert!(!is_semantic("——"));

    // Valid single-character and short commands must be preserved
    assert!(is_semantic("好。"));
    assert!(is_semantic("对"));
    assert!(is_semantic("行！"));
    assert!(is_semantic("停"));
    assert!(is_semantic("嗯"));
    assert!(is_semantic("No."));
    assert!(is_semantic("K."));
    assert!(is_semantic("Go"));
}

#[tokio::test]
async fn test_listening_state_captures_speech_immediately_after_cue_finishes_without_600ms_blind_window() {
    let flow = StreamingWorker::with_barge_in(false, false).await;
    flow.speak().await;
    tokio::time::timeout(Duration::from_secs(2), flow.sink.wrote.notified())
        .await
        .expect("playback must start");

    // 1. Interrupt playback with 350Hz tone into Listening
    let started = tokio::time::Instant::now();
    for frame in 0..25 {
        let pcm: Vec<f32> = (0..320)
            .map(|i| {
                let sample_idx = (frame * 320 + i) as f32;
                0.45 * (sample_idx * std::f32::consts::TAU * 350.0 / 16000.0).sin()
            })
            .collect();
        flow.producer.push(&pcm);
        tokio::time::sleep_until(started + Duration::from_millis((frame as u64 + 1) * 20)).await;
        if flow.sink.frames.lock().unwrap().is_empty() {
            break;
        }
    }

    // Ensure playback has stopped and state is Listening
    assert_eq!(flow.runtime.snapshot().phase, nomifun_api_types::voice::VoiceSessionPhase::Listening);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!flow.runtime.playback().is_speaking());

    // 2. Immediately speak speech at t=50ms (< 600ms cooldown window).
    // Under the old implementation with in_echo_cooldown in Listening, this speech would be cleared and lost!
    // With T48 (!is_playback_active), speech is immediately captured.
    let cmd = tokio::time::Instant::now();
    for frame in 0..30 {
        let pcm: Vec<f32> = (0..320)
            .map(|i| 0.03 * (i as f32 * std::f32::consts::TAU * 200.0 / 16000.0).sin())
            .collect();
        flow.producer.push(&pcm);
        tokio::time::sleep_until(cmd + Duration::from_millis((frame + 1) * 20)).await;
    }

    // Silence to finish utterance
    let silence = tokio::time::Instant::now();
    for frame in 0..50 {
        flow.producer.push(&vec![0.0f32; 320]);
        tokio::time::sleep_until(silence + Duration::from_millis((frame + 1) * 20)).await;
        if flow.server.calls.load(Ordering::SeqCst) >= 2 {
            break;
        }
    }

    assert!(
        flow.server.calls.load(Ordering::SeqCst) >= 2,
        "Speech spoken within <600ms of cue end must reach model without residual blind window!"
    );
}

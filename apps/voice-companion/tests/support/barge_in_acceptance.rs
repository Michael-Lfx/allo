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

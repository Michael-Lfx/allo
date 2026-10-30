//! Formal Regression Test migrated from queue_handoff_probe.rs (F1 / F3 Race Verification).
//!
//! Asserts that when task A finishes before utterance B is appended,
//! B is NOT orphaned and C does not bypass B (FIFO order A -> B -> C is preserved).

#![cfg(feature = "voice-skeleton")]

use std::{sync::{Arc, atomic::{AtomicUsize, Ordering}}, time::Duration};
use tokio::sync::{Mutex, Notify};
use nomifun_audio::{SttCallback, AudioChannel, voice_pipeline::{VoiceCapturePipeline, PipelineConfig}};
use nomifun_voice_terminal::{
    driver::{VoiceRouter, RouteError},
    contract::VoiceDecision,
    task_client::VoiceTaskClient,
    playback::{PlaybackPipeline, TestAudioSink},
    tts::{TtsProvider, TtsError},
};
use nomifun_api_types::voice::*;
use voice_companion::{voice::{VoiceCompanionRuntime, VoiceCompanionEvent}, oobe::OobeManager};

struct Stt(AtomicUsize);

#[async_trait::async_trait]
impl SttCallback for Stt {
    async fn transcribe(&self, _: AudioChannel, pcm: Vec<f32>, _: u32) -> Option<String> {
        let n = self.0.fetch_add(1, Ordering::SeqCst);
        let peak = pcm.iter().fold(0.0f32, |v, s| v.max(s.abs()));
        let label = if peak > 0.65 { "input-A" } else if peak > 0.5 { "input-C" } else { "input-B" };
        println!("ASR call={n} source={label} peak={peak} samples={}", pcm.len());
        Some(label.into())
    }
}

struct Router(AtomicUsize);

#[async_trait::async_trait]
impl VoiceRouter for Router {
    fn is_configured(&self) -> bool { true }
    async fn route_with_status(&self, _: &str, _: Option<&str>) -> Result<VoiceDecision, RouteError> {
        let n = self.0.fetch_add(1, Ordering::SeqCst);
        Ok(if n == 0 {
            nomifun_voice_terminal::driver::StubVoiceRouter::agent_task("ack", "first-task", false).route("a").await.unwrap()
        } else {
            nomifun_voice_terminal::driver::StubVoiceRouter::chat(format!("reply-{n}")).route("b").await.unwrap()
        })
    }
}

struct Tasks {
    release: Notify,
    entered: Notify,
    task: Mutex<Option<VoiceTaskSnapshot>>,
}

#[async_trait::async_trait]
impl VoiceTaskClient for Tasks {
    async fn current(&self) -> Result<Option<VoiceTaskSnapshot>, String> {
        Ok(self.task.lock().await.clone())
    }
    async fn control(&self, _: VoiceTaskControlCommand) -> Result<VoiceTaskSnapshot, String> {
        Err("unexpected_control".into())
    }
    async fn submit(&self, r: VoiceTaskSubmitRequest) -> Result<VoiceTaskDelivery, String> {
        let task = VoiceTaskSnapshot {
            command_id: r.command.command_id,
            session_id: r.command.session_id,
            acoustic_turn_id: r.command.turn_id,
            conversation_id: Some("test-conv".into()),
            message_id: Some("test-msg".into()),
            runtime_turn_id: Some("test-runtime".into()),
            status: VoiceTaskStatus::Accepted,
            result_ok: None,
            result_text: None,
            error_code: None,
        };
        *self.task.lock().await = Some(task.clone());
        self.entered.notify_one();
        self.release.notified().await;
        Ok(VoiceTaskDelivery { freshly_accepted: true, task })
    }
}

struct Speech;

#[async_trait::async_trait]
impl TtsProvider for Speech {
    async fn synthesize_stream(&self, _: &str, _: &str) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, TtsError> {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        tx.send(nomifun_audio::pcm_to_wav(&vec![0.05; 2400], 24000)).await.unwrap();
        Ok(rx)
    }
}

async fn run_queue_handoff_case(offset: u64, announce: bool) -> bool {
    let root = tempfile::tempdir().unwrap();
    let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
    let stt = Arc::new(Stt(AtomicUsize::new(0)));
    let router = Arc::new(Router(AtomicUsize::new(0)));
    let tasks = Arc::new(Tasks { release: Notify::new(), entered: Notify::new(), task: Mutex::new(None) });

    let (mut rt, _) = VoiceCompanionRuntime::from_capture(
        Arc::new(nomifun_voice_models::ModelManager::new(root.path())),
        Arc::new(OobeManager::new_bypassed()),
        Arc::new(Mutex::new(capture)),
        stt.clone(),
        router.clone(),
    );
    let gate = Arc::new(voice_companion::voice::SpeechEndTestGate::default());
    rt.set_dispatch_queue_gate(Some(gate.clone()));
    let rt = Arc::new(rt.with_tasks(tasks.clone()).with_test_playback(
        Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
        Arc::new(Speech),
    ));

    rt.update_feedback(VoiceFeedbackConfig { mode: VoiceFeedbackMode::Off, volume: 0, speak_replies: true }.into()).unwrap();

    let mut events = rt.event_sender().subscribe();
    let event_log = tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(e) if matches!(e, VoiceCompanionEvent::Error{..} | VoiceCompanionEvent::SessionEnded{..} | VoiceCompanionEvent::SpeechRecognized{..} | VoiceCompanionEvent::TaskResultReady{..}) => println!("event {e:?}"),
                Ok(_) => {},
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => panic!("observer lagged {n}"),
                Err(_) => break,
            }
        }
    });

    rt.handle_wake().await.unwrap();
    let worker = rt.start_worker();

    // Input A (peak 0.7)
    for i in 0..80 {
        producer.push(&vec![if i < 25 { 0.7 } else { 0.0 }; 320]);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    tokio::time::timeout(Duration::from_secs(2), tasks.entered.notified()).await.unwrap();
    let task = tasks.task.lock().await.clone().unwrap();
    rt.update_task_status(Some(task.clone()));
    println!("CASE offset={offset} announce={announce} A submitted phase={:?}", rt.snapshot().phase);

    let resume_gate = gate.clone();
    let r = rt.clone();
    let t = tasks.clone();

    let completion = tokio::spawn(async move {
        resume_gate.reached.notified().await;
        let mut completed = task;
        completed.status = VoiceTaskStatus::Completed;
        completed.result_text = Some("first-result".into());
        r.update_task_status_versioned(Some(completed.clone()), None, Some(10));
        t.release.notify_one();
        // Resume immediately to test tight concurrent queue handoff without sleep padding
        println!("completion before append: queue={} phase={:?}", r.pending_utterance_count(), r.snapshot().phase);
        resume_gate.resume.notify_one();
        if announce {
            for _ in 0..220 {
                let s = r.snapshot();
                if matches!(s.phase, VoiceSessionPhase::Idle | VoiceSessionPhase::Listening | VoiceSessionPhase::FollowUp)
                    && !s.audio.as_ref().is_some_and(|a| a.speech_detected)
                {
                    let request = VoiceTaskResultAnnouncement {
                        scope: VoiceSessionControlRequest {
                            instance_id: s.instance_id,
                            session_id: s.session_id,
                            turn_id: s.turn_id,
                        },
                        command_id: completed.command_id.clone(),
                        notification_id: None,
                        attempt_id: None,
                        goal: None,
                        text: "first-result".into(),
                    };
                    let res = r.announce_task_result(request).await;
                    if res.is_ok() {
                        println!("announcement OK offset={offset}");
                        break;
                    }
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    });

    // Input B (peak 0.4)
    for i in 0..85 {
        producer.push(&vec![if i < 30 { 0.4 } else { 0.0 }; 320]);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    completion.await.unwrap();
    tokio::time::sleep(Duration::from_millis(250)).await;

    let second_ok = stt.0.load(Ordering::SeqCst) == 2 && rt.pending_utterance_count() == 0;
    println!("B after: stt={} router={} queue={} phase={:?} ended={:?}", stt.0.load(Ordering::SeqCst), router.0.load(Ordering::SeqCst), rt.pending_utterance_count(), rt.snapshot().phase, rt.snapshot().ended_reason);
    println!("driver after B={:?}", rt.driver().lock().await.current_state());

    // Input C (peak 0.6)
    for i in 0..85 {
        producer.push(&vec![if i < 30 { 0.6 } else { 0.0 }; 320]);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    tokio::time::sleep(Duration::from_millis(150)).await;
    let count = stt.0.load(Ordering::SeqCst);
    let s = rt.snapshot();
    println!("C after: stt={count} router={} queue={} phase={:?} ended={:?} error={:?} conversation={:?}", router.0.load(Ordering::SeqCst), rt.pending_utterance_count(), s.phase, s.ended_reason, s.error, s.conversation);

    worker.abort();
    let _ = worker.await;
    event_log.abort();
    let _ = event_log.await;

    second_ok && count == 3 && router.0.load(Ordering::SeqCst) == 3 && s.ended_reason.is_none()
}

#[tokio::test]
async fn test_queue_handoff_f1_f3_resolution() {
    assert!(run_queue_handoff_case(1300, false).await, "Queue handoff race must not drop B or reorder C");
}

#[tokio::test]
async fn test_task_cancellation_cleans_up_dispatching_task() {
    let root = tempfile::tempdir().unwrap();
    let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
    let stt = Arc::new(Stt(AtomicUsize::new(0)));
    let router = Arc::new(Router(AtomicUsize::new(0)));
    let (rt, _) = VoiceCompanionRuntime::from_capture(
        Arc::new(nomifun_voice_models::ModelManager::new(root.path())),
        Arc::new(OobeManager::new_bypassed()),
        Arc::new(Mutex::new(capture)),
        stt.clone(),
        router.clone(),
    );
    let playback = Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new())));
    let rt = Arc::new(rt.with_test_playback(playback.clone(), Arc::new(Speech)));

    // Drive driver into DispatchingTask
    {
        let driver = rt.driver();
        let mut d = driver.lock().await;
        let _ = d.handle_event(nomifun_voice_terminal::contract::AcousticEvent::WakeHit);
        let _ = d.handle_event(nomifun_voice_terminal::contract::AcousticEvent::SpeechEnd);
        let _ = d.handle_event(nomifun_voice_terminal::contract::AcousticEvent::IntentAgentTask);
        assert_eq!(d.current_state(), nomifun_voice_terminal::contract::AcousticState::DispatchingTask);
    }

    // Cancel/fail task
    rt.update_task_status_versioned(
        Some(nomifun_api_types::voice::VoiceTaskSnapshot {
            command_id: "cmd-cancel".into(),
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

    tokio::time::sleep(Duration::from_millis(50)).await;

    let driver = rt.driver();
    let state = driver.lock().await.current_state();
    assert_ne!(
        state,
        nomifun_voice_terminal::contract::AcousticState::DispatchingTask,
        "driver must recover from DispatchingTask upon cancellation"
    );
    assert!(
        matches!(state, nomifun_voice_terminal::contract::AcousticState::Listening | nomifun_voice_terminal::contract::AcousticState::Idle),
        "driver must be in Listening or Idle so next turns can proceed"
    );
}

#[tokio::test]
async fn test_stop_fence_prevents_stale_speech_resurrection() {
    let root = tempfile::tempdir().unwrap();
    let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
    let stt = Arc::new(Stt(AtomicUsize::new(0)));
    let router = Arc::new(Router(AtomicUsize::new(0)));
    let tasks = Arc::new(Tasks { release: Notify::new(), entered: Notify::new(), task: Mutex::new(None) });

    let (mut rt, _) = VoiceCompanionRuntime::from_capture(
        Arc::new(nomifun_voice_models::ModelManager::new(root.path())),
        Arc::new(OobeManager::new_bypassed()),
        Arc::new(Mutex::new(capture)),
        stt.clone(),
        router.clone(),
    );
    let gate = Arc::new(voice_companion::voice::SpeechEndTestGate::default());
    rt.set_dispatch_queue_gate(Some(gate.clone()));

    let rt = Arc::new(rt.with_tasks(tasks.clone()).with_test_playback(
        Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
        Arc::new(Speech),
    ));

    rt.update_feedback(VoiceFeedbackConfig { mode: VoiceFeedbackMode::Off, volume: 0, speak_replies: true }.into()).unwrap();
    rt.handle_wake().await.unwrap();
    let worker = rt.start_worker();

    // Input A
    for i in 0..80 {
        producer.push(&vec![if i < 25 { 0.7 } else { 0.0 }; 320]);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    tokio::time::timeout(Duration::from_secs(2), tasks.entered.notified()).await.unwrap();
    let task = tasks.task.lock().await.clone().unwrap();
    rt.update_task_status(Some(task.clone()));

    let resume_gate = gate.clone();
    let r = rt.clone();
    let stop_task = tokio::spawn(async move {
        resume_gate.reached.notified().await;
        let s = r.snapshot();
        let request = VoiceSessionControlRequest {
            instance_id: s.instance_id,
            session_id: s.session_id,
            turn_id: s.turn_id,
        };
        r.stop_session(&request).await.unwrap();
        resume_gate.resume.notify_one();
    });

    // Input B
    for i in 0..85 {
        producer.push(&vec![if i < 30 { 0.4 } else { 0.0 }; 320]);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    stop_task.await.unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;

    let snap = rt.snapshot();
    let count = stt.0.load(Ordering::SeqCst);
    assert_eq!(count, 1, "Stale B must NOT be transcribed after stop_session");
    assert_eq!(snap.ended_reason, Some(VoiceSessionEndReason::UserStop), "UserStop must be preserved");
    assert_eq!(snap.phase, VoiceSessionPhase::Idle, "Phase must remain Idle");

    worker.abort();
    let _ = worker.await;
}

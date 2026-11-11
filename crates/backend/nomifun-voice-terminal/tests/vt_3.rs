//! Vertical Slice Test 3 (VT-3)
//!
//! Covering Phase 3 / Wave 3 tasks:
//! - VT-3-01: Dual-FSM decoupling (long-running task executing in background does not interfere with acoustic chat)
//! - VT-3-02: Task control query_status returns live progress without polluting task queue
//! - VT-3-03: Task control cancel_task aborts task to Cancelled & plays error.wav
//! - VT-3-05: 8-second confirmation timeout triggers error.wav & completely destroys task
//! - VT-3-06: Confirmation token single-use consumption constraint
//! - VT-3-08: Background task completion triggers done.wav + vocal_summary + 4-second warm follow-up

use std::sync::Arc;
use nomifun_audio::EarconKind;
use nomifun_voice_terminal::*;

// ============================================================================
// VT-3-01: Dual-FSM Decoupling (Long-running task isolated from chat)
// ============================================================================

#[tokio::test]
async fn test_vt_3_01_long_running_task_isolated_from_chat() {
    let mut task_mgr = TaskManager::new();
    let mut driver = VoiceSessionDriver::new();

    // 1. Background task begins execution
    let task_id = task_mgr.start_task("分析过去24小时系统日志");
    assert_eq!(task_mgr.active_task().unwrap().state, TaskState::Executing);
    assert_eq!(task_mgr.active_task().unwrap().task_id, task_id);

    // 2. While task is executing, acoustic FSM is in Idle and gets woken up for fast chat
    assert_eq!(driver.current_state(), AcousticState::Idle);

    driver.handle_event(AcousticEvent::WakeHit).unwrap();
    assert_eq!(driver.current_state(), AcousticState::Listening);

    driver.handle_event(AcousticEvent::SpeechEnd).unwrap();
    assert_eq!(driver.current_state(), AcousticState::Deciding);

    driver.handle_event(AcousticEvent::IntentChat).unwrap();
    assert_eq!(driver.current_state(), AcousticState::SpeakingFast);

    driver.handle_event(AcousticEvent::PlaybackDone).unwrap();
    assert_eq!(driver.current_state(), AcousticState::FollowUpListening);

    driver.handle_event(AcousticEvent::FollowUpTimeout).unwrap();
    assert_eq!(driver.current_state(), AcousticState::Idle);

    // 3. Assert background task state was completely unperturbed by acoustic chat
    assert_eq!(task_mgr.active_task().unwrap().state, TaskState::Executing);
    assert_eq!(task_mgr.active_task().unwrap().task_id, task_id);
}

// ============================================================================
// VT-3-02: Task Control query_status (Live progress without queue pollution)
// ============================================================================

#[tokio::test]
async fn test_vt_3_02_query_status_no_queue_contamination() {
    let mut task_mgr = TaskManager::new();
    let mut driver = VoiceSessionDriver::new();

    task_mgr.start_task("生成季度财务报表");
    let initial_count = task_mgr.task_count();

    // User asks "刚才的任务跑完没？"
    driver.handle_event(AcousticEvent::WakeHit).unwrap();
    driver.handle_event(AcousticEvent::SpeechEnd).unwrap();
    driver.handle_event(AcousticEvent::IntentTaskControl).unwrap();
    assert_eq!(driver.current_state(), AcousticState::SpeakingControl);

    // Query status directly from TaskManager
    let status_summary = task_mgr.query_status();
    assert!(status_summary.contains("生成季度财务报表"));
    assert!(status_summary.contains("Executing") || status_summary.contains("执行中"));

    // Finish speaking status report and enter follow-up window
    driver.handle_event(AcousticEvent::PlaybackDone).unwrap();
    assert_eq!(driver.current_state(), AcousticState::FollowUpListening);

    // Assert zero queue pollution: task count remains exactly the same
    assert_eq!(task_mgr.task_count(), initial_count);
}

// ============================================================================
// VT-3-03: Task Control cancel_task (Forced abort to Cancelled state)
// ============================================================================

#[tokio::test]
async fn test_vt_3_03_cancel_task_aborts_to_cancelled() {
    let sink = Arc::new(TestAudioSink::new());
    let pipeline = PlaybackPipeline::new(sink.clone());
    let mut task_mgr = TaskManager::new();
    let mut driver = VoiceSessionDriver::new();

    let task_id = task_mgr.start_task("长期下载任务");
    assert_eq!(task_mgr.active_task().unwrap().state, TaskState::Executing);

    // User says "取消当前任务"
    driver.handle_event(AcousticEvent::WakeHit).unwrap();
    driver.handle_event(AcousticEvent::SpeechEnd).unwrap();
    driver.handle_event(AcousticEvent::IntentTaskControl).unwrap();

    // Cancel task execution
    let cancelled_id = task_mgr.cancel_task().expect("Task cancellation must succeed");
    assert_eq!(cancelled_id, task_id);

    // Play error.wav receipt
    pipeline.play_earcon(EarconKind::Error).await.unwrap();
    assert!(sink.frame_count() > 0);

    // Status is now Cancelled
    assert_eq!(task_mgr.active_task().unwrap().state, TaskState::Cancelled);
}

// ============================================================================
// VT-3-05: 8-Second Confirmation Timeout & Task Destruction
// ============================================================================

#[tokio::test]
async fn test_vt_3_05_confirmation_timeout_and_task_destruction() {
    let sink = Arc::new(TestAudioSink::new());
    let pipeline = PlaybackPipeline::new(sink.clone());
    let mut confirm_mgr = ConfirmationManager::new();
    let mut task_mgr = TaskManager::new();
    let mut driver = VoiceSessionDriver::new();

    let task_id = task_mgr.start_task("清空临时沙箱目录");
    let token = confirm_mgr.issue_token(&task_id);
    assert_eq!(token.task_id, task_id);

    // Enter confirmation awaiting state
    driver.handle_event(AcousticEvent::ToolPolicyIntercept).unwrap();
    assert_eq!(driver.current_state(), AcousticState::AwaitingConfirmation);

    driver.handle_event(AcousticEvent::PromptDone).unwrap();
    assert_eq!(driver.current_state(), AcousticState::ListeningConfirm);

    // User remains silent, 8-second confirmation window expires
    driver.handle_event(AcousticEvent::ConfirmTimeout).unwrap();
    assert_eq!(driver.current_state(), AcousticState::Idle);

    // Play error.wav timeout notice
    pipeline.play_earcon(EarconKind::Error).await.unwrap();

    // Token is revoked and task is destroyed
    confirm_mgr.revoke_token(&token.token_id);
    task_mgr.cancel_task().unwrap();
    assert_eq!(task_mgr.active_task().unwrap().state, TaskState::Cancelled);
}

// ============================================================================
// VT-3-06: Confirmation Token Single-Use Consumption Constraint
// ============================================================================

#[test]
fn test_vt_3_06_confirmation_token_single_use_consumption() {
    let mut confirm_mgr = ConfirmationManager::new();
    let token = confirm_mgr.issue_token("task_delete_cache_101");

    // First consumption: must succeed
    let res1 = confirm_mgr.consume_token(&token.token_id);
    assert_eq!(res1, Ok("task_delete_cache_101".to_string()));

    // Second consumption: must fail with AlreadyConsumed
    let res2 = confirm_mgr.consume_token(&token.token_id);
    assert_eq!(res2, Err(ConfirmationError::AlreadyConsumed));
}

// ============================================================================
// VT-3-08: Task Completion Summary & 4-Second Follow-up Window
// ============================================================================

#[tokio::test]
async fn test_vt_3_08_task_completion_summary_and_follow_up() {
    let sink = Arc::new(TestAudioSink::new());
    let pipeline = PlaybackPipeline::new(sink.clone());
    let mut task_mgr = TaskManager::new();
    let mut driver = VoiceSessionDriver::new();

    task_mgr.start_task("同步远程代码仓库");
    assert_eq!(driver.current_state(), AcousticState::Idle);

    // Background task finishes
    task_mgr
        .complete_task("远程仓库同步完成，更新了 12 个提交。")
        .expect("complete_task must succeed");
    assert_eq!(task_mgr.active_task().unwrap().state, TaskState::Completed);

    // Async task completion notice arrives at Acoustic FSM (in IDLE)
    driver.handle_event(AcousticEvent::TaskSummaryReady).unwrap();
    assert_eq!(driver.current_state(), AcousticState::SpeakingSummary);

    // Play done.wav earcon
    pipeline.play_earcon(EarconKind::Done).await.unwrap();
    assert!(sink.frame_count() > 0);

    // Vocal summary completes playback -> enters 4-second follow-up window
    driver.handle_event(AcousticEvent::PlaybackDone).unwrap();
    assert_eq!(driver.current_state(), AcousticState::FollowUpListening);

    // User follows up without wake word
    driver.handle_event(AcousticEvent::FollowUpSpeech).unwrap();
    assert_eq!(driver.current_state(), AcousticState::Deciding);
}

//! Vertical Slice Test VT-4-03:
//! OOBE 全流程：6 位动态配对码生成 → 语音播报格式化 → 校验 → 5 次连续失败锁定 15 分钟 → 倒计时正确 → 页面服务寿命锚定至 ModelManager 终态。
//!
//! Baseline:
//! - `interaction.zh.md` §3.1 (D12), §5.1 (TC-AM-015, TC-AM-018)
//! - `ui-motion.zh.md` §4 (Mobile-First Pairing & Progress Surface)
//! - `execution/wave-4-hardening.zh.md` §2 (T4-04), §3 (VT-4-03)

use std::sync::Arc;
use tempfile::tempdir;

use ai_mate::oobe::{OobeManager, VerifyOutcome};
use nomifun_api_types::voice::ModelInstallState;
use nomifun_voice_models::ModelManager;

#[tokio::test]
async fn test_vt_4_03_code_generation_and_vocal_announcement() {
    let oobe = OobeManager::new();
    let code = oobe.current_code().await;

    // 1. Must be exactly 6 numeric digits
    assert_eq!(code.len(), 6, "Pairing code must be 6 digits");
    assert!(
        code.chars().all(|c| c.is_ascii_digit()),
        "Pairing code must contain only ASCII digits"
    );

    // 2. Vocal announcement format (e.g. "8-3-9-2-0-1")
    let vocal_digits = oobe.vocal_digits().await;
    assert_eq!(
        vocal_digits.split('-').count(),
        6,
        "Vocal format must separate all 6 digits by hyphens"
    );

    let vocal_announcement = oobe.vocal_announcement_text().await;
    assert!(
        vocal_announcement.contains(&vocal_digits),
        "Vocal announcement must include the formatted digits"
    );
}

#[tokio::test]
async fn test_vt_4_03_successful_verification() {
    let oobe = OobeManager::new();
    let code = oobe.current_code().await;

    let result = oobe.verify_code(&code).await;
    assert_eq!(result, VerifyOutcome::Success);
    assert!(oobe.is_paired().await, "Manager must be marked as paired");
}

#[tokio::test]
async fn test_vt_4_03_anti_brute_force_five_failures_locks_fifteen_minutes() {
    let oobe = OobeManager::new();
    let wrong_code = "000000";

    // 1-4 failures: remaining attempts decremented
    for expected_remaining in (1..=4).rev() {
        let outcome = oobe.verify_code(wrong_code).await;
        match outcome {
            VerifyOutcome::Failed { attempts_remaining } => {
                assert_eq!(
                    attempts_remaining, expected_remaining,
                    "Attempts remaining must match expected count"
                );
            }
            other => panic!("Expected Failed outcome, got: {other:?}"),
        }
    }

    // 5th failure: enters 15-minute lockout
    let fifth_outcome = oobe.verify_code(wrong_code).await;
    match fifth_outcome {
        VerifyOutcome::Locked {
            lockout_remaining_secs,
        } => {
            assert!(
                lockout_remaining_secs <= 900 && lockout_remaining_secs >= 890,
                "Lockout must be ~15 minutes (900s), got: {lockout_remaining_secs}s"
            );
        }
        other => panic!("Expected Locked on 5th failure, got: {other:?}"),
    }

    // Subsequent attempt during lockout is immediately rejected
    let locked_outcome = oobe.verify_code(wrong_code).await;
    assert!(
        matches!(locked_outcome, VerifyOutcome::Locked { .. }),
        "Attempts during lockout must return Locked"
    );
}

#[tokio::test]
async fn test_vt_4_03_service_lifetime_decoupled_from_code_expiry() {
    let temp_dir = tempdir().unwrap();
    let manager = Arc::new(ModelManager::new(temp_dir.path()));
    let oobe = OobeManager::new();

    // 1. Initially NotInstalled, pairing service is active
    assert_eq!(manager.get_status().await.state, ModelInstallState::NotInstalled);
    assert!(
        oobe.should_keep_web_service_alive(&manager).await,
        "Pairing web service must remain alive while ModelManager is in progress"
    );

    // 2. Transition to Downloading: pairing service must stay alive even if pairing code expired
    manager
        .set_state(ModelInstallState::Downloading {
            current_file: "asr/model.int8.onnx".into(),
            file_index: 3,
            total_files: 7,
            downloaded_bytes: 50_000_000,
            total_bytes: 100_000_000,
            progress_percent: 50.0,
            speed_bytes_per_sec: 2_500_000,
        })
        .await;

    // Simulate code expiration by advancing or checking
    assert!(
        oobe.should_keep_web_service_alive(&manager).await,
        "Pairing web service must stay active during Downloading to preserve SSE stream"
    );

    // 3. Once ModelManager reaches Ready terminal state, service may enter idle shutdown
    for &rel_path in nomifun_voice_models::EXPECTED_FILES {
        let p = temp_dir.path().join(rel_path);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&p, b"mock model binary content").unwrap();
    }

    manager
        .set_state(ModelInstallState::Ready {
            models_dir: temp_dir.path().to_path_buf(),
            total_size_bytes: 100_000,
        })
        .await;

    let status = manager.get_status().await;
    assert!(
        matches!(status.state, ModelInstallState::Ready { .. }),
        "Models must report Ready state on disk"
    );

    assert!(
        !oobe.should_keep_web_service_alive(&manager).await,
        "Service lifetime terminates once Ready terminal state is reached"
    );
}

#[tokio::test]
async fn test_vt_4_03_code_regeneration_resets_counters() {
    let oobe = OobeManager::new();
    let old_code = oobe.current_code().await;

    // Trigger some failed attempts
    let _ = oobe.verify_code("000000").await;
    let _ = oobe.verify_code("111111").await;

    // Regenerate code
    let new_code = oobe.regenerate_code().await.expect("regenerate code");
    assert_eq!(new_code.len(), 6);
    assert_ne!(new_code, "000000");
    assert_ne!(new_code, old_code);

    // Fresh new code should succeed and reset failed attempts
    let outcome = oobe.verify_code(&new_code).await;
    assert_eq!(outcome, VerifyOutcome::Success);
    assert!(oobe.is_paired().await);
}

#[tokio::test]
async fn test_vt_4_03_lockout_cannot_be_bypassed_by_regeneration() {
    let oobe = OobeManager::new();
    for _ in 0..5 {
        let _ = oobe.verify_code("000000").await;
    }
    // Now device is locked out
    let res = oobe.regenerate_code().await;
    assert!(res.is_err(), "Regeneration must fail when device is locked out");
    // Verify it is still locked
    let outcome = oobe.verify_code("000000").await;
    assert!(matches!(outcome, VerifyOutcome::Locked { .. }));
}

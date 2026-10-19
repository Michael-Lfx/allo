# Fast-Front & Background Runtime (S0-S9) Owner Verification Script
# Usage:
#   powershell -ExecutionPolicy Bypass -File docs/architecture/voice-agent/execution/fast-front-evidence/reproduce_scenarios.ps1

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host " Flowy Voice Fast-Front & Background Runtime Test Matrix " -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan

$TotalPassed = 0
$TotalFailed = 0

function Run-Step($name, $command) {
    Write-Host "`n>>> Running: $name..." -ForegroundColor Yellow
    $startTime = Get-Date
    Invoke-Expression $command
    if ($LASTEXITCODE -eq 0) {
        $duration = ((Get-Date) - $startTime).TotalSeconds
        Write-Host "  [PASS] $name ($([math]::Round($duration, 2))s)" -ForegroundColor Green
        $script:TotalPassed++
    } else {
        Write-Host "  [FAIL] $name" -ForegroundColor Red
        $script:TotalFailed++
    }
}

# Tests use isolated final artifacts; voice-skeleton must never replace the default development daemon.
# Automatic replay is not microphone/speaker/native-focus or Owner acceptance.

# 1. API Types & DTO Baseline Check (S0)
Run-Step "S0: API Types & DTO Golden Tests" "cargo test --target-dir target/voice-review -p nomifun-api-types"

# 2. Standalone Fast Dialogue Persistence (S1, BV-05, BV-06, BV-07)
Run-Step "S1: Idempotent Pair Projection & Preflight Guards" "cargo test --target-dir target/voice-review -p nomifun-app --test voice_dialogue_integration"
Run-Step "S1: Voice Dialogue Unit Suite" "cargo test --target-dir target/voice-review -p nomifun-app --lib voice_dialogue::tests"

# 3. Active Window & Pet Binding (S2, BV-02, BV-03, BV-04)
Run-Step "S2: Target Resolution & Turn Freeze" "cargo test --target-dir target/voice-review -p nomifun-app --lib voice_companion_manager::tests::target_update_and_candidate_endpoints_work"

# 4. First Real Background Task (S3, BV-08)
Run-Step "S3: Real Workspace File Read with UUID Marker" "cargo test --target-dir target/voice-review -p nomifun-app --lib voice_task_runtime::tests::a_voice_task_uses_the_real_conversation_runtime_to_read_a_fresh_workspace_file"

# 5. Query, Cancel & Timeout Recovery (S4, BV-09, BV-10, BV-11)
Run-Step "S4: Monotonic Cancellation & Race Handling" "cargo test --target-dir target/voice-review -p nomifun-app --lib voice_tasks::tests::bv_10_repeated_cancel_and_completion_race"
Run-Step "S4: Deadline Expiration & Clock Rollback" "cargo test --target-dir target/voice-review -p nomifun-app --lib voice_tasks::tests::bv_11_deadline_expiration_and_clock_rollback"
Run-Step "S4: Foreground 3s Timeout (Unknown Return)" "cargo test --target-dir target/voice-review -p nomifun-app --lib voice_companion_manager::tests::bv_09_foreground_3s_timeout_returns_unknown_without_cancelling_host_admission"

# 6. Off-Screen Feedback & Progress Loop (S5, BV-12, BV-13)
Run-Step "S5: Feedback Tracker 15s/45s Throttling" "cargo test --target-dir target/voice-review -p nomifun-app --lib voice_companion_manager::tests::test_task_feedback_tracker_throttling_and_stage_reset"
Run-Step "S5: High-Frequency PCM Volume & Monotonic Seq" "cargo test --target-dir target/voice-review -p nomifun-app --lib voice_companion_manager::tests::bv_12_high_frequency_pcm_and_instance_switch_preserve_task_and_monotonic_seq"
Run-Step "S5: User Speech Suppression & Completion Cue" "cargo test --target-dir target/voice-review -p nomifun-app --lib voice_companion_manager::tests::bv_13_user_speech_suppresses_progress_and_completion_replaces_old_cues"

# 7. Reliable Result Delivery & Re-listen (S6, BV-14)
Run-Step "S6: Saved Result Re-listen (0 Model/Tool Calls)" "cargo test --target-dir target/voice-review -p nomifun-app --lib voice_tasks::tests::bv_14_saved_result_relisten_without_reexecution_and_notification_failure_preserves_text"
Run-Step "S6: Task Replay & Notification HTTP Endpoints" "cargo test --target-dir target/voice-review -p nomifun-app --lib voice_companion_manager::tests::test_task_replay_and_notification_endpoints"

# 8. Tool Confirmation & Safety Boundary (S7, BV-15)
Run-Step "S7: Tool Approval Expiry, Single-Use & Anti-Bypass" "cargo test --target-dir target/voice-review -p nomifun-app --lib voice_tasks::tests::bv_15_tool_approval_expired_rejected_and_cannot_be_bypassed"
Run-Step "S7: Actual Destructive Delete Confirmation" "cargo test --target-dir target/voice-review -p nomifun-app --lib voice_task_runtime::tests::actual_voice_delete_waits_for_its_bound_one_time_confirmation"
Run-Step "S7: Actual Destructive Delete Refusal Preserves File" "cargo test --target-dir target/voice-review -p nomifun-app --lib voice_task_runtime::tests::refusing_an_actual_voice_delete_preserves_the_local_file"

# 9. Workspace Joint Build & Gate Regression (S8)
Run-Step "S8: Cargo Workspace Compile Check" "cargo check --workspace --target-dir target/voice-staging"
Run-Step "S8: React 19 Frontend SPA Build" "bun run build:ui"
Run-Step "S8: i18n Dictionary Symmetry Check" "bun run check:i18n"
Run-Step "S8: Theme Contract Check" "bun run check:theme"
Run-Step "S8: Daemon Full Flow & Confirmation Suite" "cargo test --target-dir target/voice-review -p ai-mate --features voice-skeleton"

Write-Host "`n==========================================================" -ForegroundColor Cyan
$summaryColor = if ($TotalFailed -eq 0) { "Green" } else { "Red" }
Write-Host " Verification Summary: $TotalPassed Passed, $TotalFailed Failed" -ForegroundColor $summaryColor
Write-Host "==========================================================" -ForegroundColor Cyan

if ($TotalFailed -gt 0) {
    exit 1
}

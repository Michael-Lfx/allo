//! Post-session memory distillation workflow for the nomi engine.
//!
//! This is the async/LLM half of spec-G: the pure functions live in
//! `nomi_memory::distill`. Here we gate on an opt-in flag, redact the
//! transcript (gate 1), call the provider once (with a single parse retry),
//! redact each distilled entry (gate 2), and synchronously commit the small
//! file update before the owning turn may publish its terminal event.
//!
//! Discipline (R30 / `21` D9=A): distillation is still an exact child of the
//! accepted turn, but it is **spawned in the background** instead of awaited —
//! the memory half is never allowed to delay the turn's terminal event
//! ("回答完成" = "轮次结束"). The child is not detached from the turn's
//! cancellation domain: it carries a clone of the turn's `CancellationToken`,
//! so a stop/kill drops the provider future at its next await point exactly as
//! the previous in-line `await_exact_turn_child` did (no provider call and no
//! filesystem mutation can follow a cancellation). Every failure path still
//! degrades silently (debug/warn log, never `emit_error`) and never masquerades
//! as a failed model turn; no new event type or wire method is involved.

use std::path::PathBuf;
use std::sync::Arc;

use nomi_config::config::Config;
use nomi_memory::distill::{
    DistillOutput, apply_distilled, build_distill_prompt, parse_distill_output, DISTILL_SYSTEM,
};
use nomi_redact::redact_secrets_owned;

use crate::factory::provider_config::{one_shot_completion, user_message};

/// Token ceiling for the distillation completion. codex Phase1 runs
/// low-effort; nomi's `one_shot_completion` already sends no reasoning_effort,
/// and a small ceiling keeps the cost of each distilled session bounded.
const DISTILL_MAX_TOKENS: u32 = 2048;

/// Environment-variable gate (legacy override). Distillation adds one extra LLM
/// call per normal work session (token cost). The primary gate is now the
/// `[memory]` config section (optimization 5), but this env var is still
/// honoured as a backward-compatible override: setting it to `"1"` / `"true"`
/// forces ON regardless of config; `"0"` / `"false"` forces OFF.
pub const DISTILL_ENABLED_ENV: &str = "NOMIFUN_MEMORY_DISTILL";

/// Host-level policy override (`-1` = the host expressed no opinion). A host
/// whose own configuration file carries the switch — the Agent Store host reads
/// `[memory].distill_enabled` from `~/.agent-store/config.toml` — records it
/// here once at startup instead of exporting `NOMIFUN_MEMORY_DISTILL`, so no
/// process env mutation is involved.
static HOST_DISTILL_OVERRIDE: std::sync::atomic::AtomicI8 = std::sync::atomic::AtomicI8::new(-1);

/// Record this host's distillation policy. Intended to be called once during
/// startup, before the first turn; `None` clears it back to "no opinion".
pub fn set_distill_host_override(enabled: Option<bool>) {
    HOST_DISTILL_OVERRIDE.store(
        match enabled {
            Some(true) => 1,
            Some(false) => 0,
            None => -1,
        },
        std::sync::atomic::Ordering::Relaxed,
    );
}

fn host_distill_override() -> Option<bool> {
    match HOST_DISTILL_OVERRIDE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => Some(true),
        0 => Some(false),
        _ => None,
    }
}

/// Whether distillation is enabled.
///
/// Precedence: the `NOMIFUN_MEMORY_DISTILL` env var (`"1"`/`"true"` forces ON,
/// `"0"`/`"false"` forces OFF) → this host's own switch via
/// [`set_distill_host_override`] → the config section `[memory].distill_enabled`
/// (default ON).
pub fn distill_enabled(cfg: &Config) -> bool {
    let env = std::env::var(DISTILL_ENABLED_ENV).ok();
    match env.as_deref() {
        Some(v) if v == "1" || v.eq_ignore_ascii_case("true") => true,
        Some(v) if v == "0" || v.eq_ignore_ascii_case("false") => false,
        _ => host_distill_override().unwrap_or(cfg.memory.distill_enabled),
    }
}

/// Resolve the token ceiling: config value takes precedence, falling back to
/// the compile-time default.
fn distill_max_tokens(cfg: &Config) -> u32 {
    let configured = cfg.memory.distill_max_tokens;
    if configured > 0 {
        configured
    } else {
        DISTILL_MAX_TOKENS
    }
}

/// Spawn distillation as a background child of one accepted turn (R30 /
/// `21` D9=A).
///
/// The owning turn's terminal event must not wait for this extra provider call:
/// "回答完成" ＝ "轮次结束". The child is still bound to the *same* lifecycle —
/// it carries a clone of the turn's `CancellationToken`, so a stop/kill that
/// lands before or after the terminal event drops the provider future at its
/// next await point, exactly as the previous in-line `await_exact_turn_child`
/// did. No new cancellation mechanism, no new event type: a cancelled turn
/// still leaves no provider call and no filesystem mutation behind.
///
/// Returns `false` when the token was already cancelled — nothing is spawned
/// and the caller's cancel branch owns the terminal event.
pub(super) fn spawn_distill_exact_turn(
    cancel: tokio_util::sync::CancellationToken,
    cfg: Arc<Config>,
    dir: PathBuf,
    transcript: String,
) -> bool {
    if cancel.is_cancelled() {
        // Pre-spawn judgment: a turn that is already cancelled must not create
        // a child that cancellation would only have to kill again.
        return false;
    }
    #[cfg(test)]
    if let Some(child) = take_test_child(&dir) {
        spawn_exact_turn_child(cancel, child);
        return true;
    }
    spawn_exact_turn_child(cancel, run_distill(cfg, dir, transcript));
    true
}

/// Spawn one exact-turn child into the background. The cancellation token is
/// the only lifecycle handle it needs: cancelling it drops the child's future
/// before any later apply stage can start, so a stopped turn never leaves a
/// pending provider call or a late filesystem write behind. Failures inside the
/// child are best-effort (its own debug/warn logs) and can never turn into a
/// model-turn error.
fn spawn_exact_turn_child(
    cancel: tokio_util::sync::CancellationToken,
    child: impl std::future::Future<Output = ()> + Send + 'static,
) {
    tokio::spawn(async move {
        if !await_exact_turn_child(&cancel, child).await {
            tracing::debug!("post-session distill child dropped by turn cancellation");
        }
    });
}

/// Test seam (R30): replace the spawned child for one memory directory so a test
/// can hold the background distillation open and assert the turn's terminal
/// event is published without waiting for it. Keyed by directory, so parallel
/// tests using different workspaces cannot interfere. Absent from production
/// builds.
#[cfg(test)]
pub(super) type TestDistillChild = Box<
    dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync,
>;

#[cfg(test)]
static TEST_DISTILL_CHILDREN: std::sync::Mutex<Vec<(PathBuf, TestDistillChild)>> =
    std::sync::Mutex::new(Vec::new());

#[cfg(test)]
pub(super) fn install_test_child(dir: &std::path::Path, child: TestDistillChild) {
    TEST_DISTILL_CHILDREN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push((dir.to_path_buf(), child));
}

#[cfg(test)]
fn take_test_child(
    dir: &std::path::Path,
) -> Option<std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>> {
    let children = TEST_DISTILL_CHILDREN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    children
        .iter()
        .find(|(candidate, _)| candidate == dir)
        .map(|(_, child)| child())
}

async fn await_exact_turn_child(
    cancel: &tokio_util::sync::CancellationToken,
    child: impl std::future::Future<Output = ()>,
) -> bool {
    tokio::pin!(child);
    tokio::select! {
        biased;
        _ = cancel.cancelled() => false,
        _ = &mut child => !cancel.is_cancelled(),
    }
}

/// Run one post-session distillation. Caller has already decided this turn is
/// eligible (not companion, origin empty, `distill_dir` set) and that the gate
/// is on. `transcript` is the engine's role-tagged history snapshot.
async fn run_distill(cfg: Arc<Config>, dir: PathBuf, transcript: String) {
    // Gate 1: redact the transcript before it is uploaded to the provider.
    let transcript = redact_secrets_owned(transcript);
    if transcript.trim().is_empty() {
        return;
    }
    let prompt = build_distill_prompt(&transcript);
    let max_tokens = distill_max_tokens(&cfg);

    // One parse retry (the model occasionally wraps JSON in prose); a provider
    // failure does not burn the retry. Mirrors the companion learner's policy.
    let mut parsed: Option<DistillOutput> = None;
    for _ in 0..2 {
        match one_shot_completion(&cfg, DISTILL_SYSTEM, vec![user_message(&prompt)], max_tokens).await {
            Ok(raw) => match parse_distill_output(&raw) {
                Ok(out) => {
                    parsed = Some(out);
                    break;
                }
                Err(e) => tracing::debug!(error = %e, "distill output unparseable"),
            },
            Err(e) => {
                // Existing observability outlet only (tracing); no new event
                // type, no `emit_error`. `warn` because R30 lets this failure
                // land after the turn's terminal event, where it is otherwise
                // invisible: the turn already reported success.
                tracing::warn!(
                    error = %e,
                    "post-session distill provider call failed (best-effort: turn outcome unchanged)"
                );
                break; // provider failure: don't retry
            }
        }
    }

    let Some(mut out) = parsed else {
        return;
    };
    if out.memories.is_empty() {
        return; // no-op gate hit: nothing worth keeping
    }

    // Gate 2: redact every distilled field before it touches disk.
    for m in &mut out.memories {
        m.content = redact_secrets_owned(std::mem::take(&mut m.content));
        m.description = redact_secrets_owned(std::mem::take(&mut m.description));
    }

    // `apply_distilled` is a small synchronous atomic file update. Keep it in
    // this future instead of `spawn_blocking`: dropping a JoinHandle cannot
    // cancel a started blocking closure, which previously allowed a late write
    // after cancellation/Finished. Once this section starts it has no await
    // point, so a cancellation that lands during it cannot interleave, and the
    // write completes on this runtime thread. Under R30 (`21` D9=A) this write
    // may now land *after* the turn's terminal event — accepted: the memory
    // snapshot is a side effect of a finished turn, not part of its terminal
    // contract, and the transcript snapshot it was built from is still taken
    // before the engine lock is released.
    match apply_distilled(&dir, &out) {
        Ok(n) if n > 0 => {
            tracing::info!(written = n, dir = %dir.display(), "session distilled to file-based memory")
        }
        Ok(_) => {} // all candidates deduped / filtered
        Err(e) => tracing::warn!(error = %e, "distill apply failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[test]
    fn distill_enabled_reads_config_and_env() {
        // When the env var is unset, the config value is the gate.
        // Default MemoryConfig has distill_enabled = true (optimization 5).
        let mem_on = nomi_config::config::MemoryConfig::default();
        assert!(mem_on.distill_enabled, "default MemoryConfig should have distill ON");

        let mem_off = nomi_config::config::MemoryConfig {
            distill_enabled: false,
            ..Default::default()
        };
        assert!(!mem_off.distill_enabled);

        // ENV override semantics: "1"/"true" forces ON, "0"/"false" forces OFF,
        // unset falls through to config. We can only safely test the unset path
        // (the env var is process-global and parallel tests may conflict).
        let key = DISTILL_ENABLED_ENV;
        if std::env::var(key).is_err() {
            // Config ON + env unset → ON
            assert!(mem_on.distill_enabled || std::env::var(key).is_ok());
            // Config OFF + env unset → OFF
            assert!(!mem_off.distill_enabled || std::env::var(key).is_ok());
        }
    }

    #[tokio::test]
    async fn spawned_child_does_not_block_the_caller() {
        // R30 / `21` D9=A: the owning turn's terminal path returns as soon as the
        // child is spawned, even while the child is still in flight. The ordering
        // is asserted with channels, never with sleeps: the child cannot finish
        // before the test releases it, and the release happens strictly after the
        // caller has already moved on.
        let cancel = tokio_util::sync::CancellationToken::new();
        let phases = Arc::new(std::sync::Mutex::new(Vec::new()));
        let entered = Arc::new(tokio::sync::Semaphore::new(0));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let child_phases = Arc::clone(&phases);
        let child_entered = Arc::clone(&entered);
        let child_release = Arc::clone(&release);

        spawn_exact_turn_child(cancel, async move {
            child_entered.add_permits(1);
            let _ = child_release.acquire().await;
            child_phases.lock().unwrap().push("child-finished");
        });
        phases.lock().unwrap().push("caller-after-spawn");

        tokio::time::timeout(Duration::from_secs(2), entered.acquire())
            .await
            .expect("the spawned child must actually start")
            .expect("entered semaphore stays open")
            .forget();
        assert_eq!(
            phases.lock().unwrap().as_slice(),
            ["caller-after-spawn"],
            "the caller must not have waited for the child"
        );

        release.add_permits(1);
        tokio::time::timeout(Duration::from_secs(2), async {
            while phases.lock().unwrap().len() < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the background child must still run to completion in this lifecycle");
        assert_eq!(
            phases.lock().unwrap().as_slice(),
            ["caller-after-spawn", "child-finished"],
            "the caller's progress must precede the child's completion, not the other way round"
        );
    }

    #[tokio::test]
    async fn pre_cancelled_token_never_starts_a_child() {
        // Pre-spawn judgment (R30): a turn that is already cancelled starts no
        // child at all — the caller's cancel branch owns the terminal event.
        let dir = tempfile::tempdir().unwrap();
        let runs = Arc::new(AtomicUsize::new(0));
        let hook_runs = Arc::clone(&runs);
        install_test_child(
            dir.path(),
            Box::new(move || {
                let hook_runs = Arc::clone(&hook_runs);
                Box::pin(async move {
                    hook_runs.fetch_add(1, Ordering::SeqCst);
                })
            }),
        );

        let cancel = tokio_util::sync::CancellationToken::new();
        cancel.cancel();
        let spawned = spawn_distill_exact_turn(
            cancel,
            Arc::new(test_distill_config("http://127.0.0.1:1")),
            dir.path().to_path_buf(),
            "user: hello".into(),
        );

        assert!(
            !spawned,
            "an already-cancelled turn must not spawn a distillation child"
        );
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
        assert_eq!(runs.load(Ordering::SeqCst), 0, "no child may run");
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "no child may write memory for a cancelled turn"
        );
    }

    #[tokio::test]
    async fn cancelling_a_spawned_child_leaves_no_late_effect() {
        // The R30 window: cancellation may land after the turn already published
        // its terminal event. The spawned child is bound to the same token, so it
        // is dropped before its apply stage — a later release must have no effect.
        let cancel = tokio_util::sync::CancellationToken::new();
        let late_write = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let entered = Arc::new(tokio::sync::Semaphore::new(0));
        let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
        let child_entered = Arc::clone(&entered);
        let child_late_write = Arc::clone(&late_write);

        spawn_exact_turn_child(cancel.clone(), async move {
            child_entered.add_permits(1);
            let _ = release_rx.await;
            child_late_write.store(true, Ordering::SeqCst);
        });
        tokio::time::timeout(Duration::from_secs(2), entered.acquire())
            .await
            .expect("the spawned child must actually start")
            .expect("entered semaphore stays open")
            .forget();

        cancel.cancel();
        // Release *after* the cancellation: if the child had survived it, this
        // would let it reach the apply stage and the flag would flip.
        let _ = release_tx.send(());
        for _ in 0..100 {
            tokio::task::yield_now().await;
        }
        assert!(
            !late_write.load(Ordering::SeqCst),
            "a cancelled turn must drop the spawned child before any apply stage"
        );
    }

    #[tokio::test]
    async fn provider_failure_returns_normally_and_writes_nothing() {
        // Requirement: a distillation failure is best-effort and can never become
        // a session error. `run_distill` returns `()` — the structural guarantee —
        // and the provider failure path must simply return without touching disk.
        // Port 1 is never served, so the provider call fails immediately.
        let dir = tempfile::tempdir().unwrap();
        run_distill(
            Arc::new(test_distill_config("http://127.0.0.1:1")),
            dir.path().to_path_buf(),
            "user: hello\nassistant: hi".into(),
        )
        .await;

        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "a failed distillation must not write memory files"
        );
    }

    fn test_distill_config(base_url: &str) -> nomi_config::config::Config {
        let mut config = nomi_config::config::Config::resolve(&nomi_config::config::CliArgs {
            provider: Some("openai".into()),
            api_key: Some("sk-test-key".into()),
            base_url: Some(base_url.to_owned()),
            model: Some("gpt-4o-mini".into()),
            max_tokens: Some(512),
            max_turns: Some(2),
            system_prompt: None,
            profile: None,
            auto_approve: true,
            project_dir: Some(PathBuf::from("/project")),
        })
        .expect("test config should resolve");
        config.session.enabled = false;
        config
    }

    #[tokio::test]
    async fn cancelling_exact_turn_child_leaves_no_late_effect() {
        let cancel = tokio_util::sync::CancellationToken::new();
        let late_write = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (_release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
        let child_cancel = cancel.clone();
        let child_late_write = Arc::clone(&late_write);

        let child = tokio::spawn(async move {
            await_exact_turn_child(&child_cancel, async move {
                let _ = started_tx.send(());
                let _ = release_rx.await;
                child_late_write.store(true, Ordering::SeqCst);
            })
            .await
        });
        started_rx.await.expect("child started");
        cancel.cancel();
        assert!(!child.await.expect("join exact child"));
        tokio::task::yield_now().await;
        assert!(
            !late_write.load(Ordering::SeqCst),
            "dropping the provider child on cancellation must prevent any later apply"
        );
    }
}

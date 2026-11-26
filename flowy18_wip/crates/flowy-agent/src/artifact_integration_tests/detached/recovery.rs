//! Real process exits leave production history/markers for a fresh host.
use super::*;
use crate::automation_executor::{
    AgentRunExecutor, AutomationRunResolver, ResolvedAutomationRun,
};
use flowy_automation::store::{RevisionRecord, RunRecord};
use flowy_automation::{
    ArtifactState, AttemptEvidence, AttemptId, AttemptRef, CommitRef,
    HistoryCommitState, PortError, RunExecutor, RunId, RunSpec, SessionRef,
    SubmissionEvidence, SubmissionRef,
};
use std::path::Path;
use std::process::Command;

const ROOT_ENV: &str = "FLOWY_AGENT_CRASH_RECOVERY_ROOT";
const MODE_ENV: &str = "FLOWY_AGENT_CRASH_RECOVERY_MODE";

fn recovery_attempt() -> DetachedAttempt {
    DetachedAttempt {
        session_id: "automation-crash-recovery".into(),
        submission: 0x711,
        execution: ExecutionMetadata {
            owner: Some(ExecutionOwner {
                authority: "recovery.test".into(),
                subject: "71".into(),
            }),
            source: ExecutionSource::Automation {
                automation_id: "0190f5fe-7c00-7a00-8000-000000000001".into(),
                run_id: "0190f5fe-7c00-7a00-8000-000000000002".into(),
                attempt_id: "0190f5fe-7c00-7a00-8000-000000000003".into(),
            },
        },
    }
}

fn reference() -> AttemptRef {
    let attempt = recovery_attempt();
    AttemptRef {
        principal: flowy_automation::PrincipalId::cloud("recovery.test", 71)
            .unwrap(),
        run_id: RunId::parse("0190f5fe-7c00-7a00-8000-000000000002").unwrap(),
        attempt_id: AttemptId::parse("0190f5fe-7c00-7a00-8000-000000000003")
            .unwrap(),
        session_id: SessionRef::parse(attempt.session_id.clone()).unwrap(),
        submission_id: SubmissionRef::parse(attempt.submission_id()).unwrap(),
    }
}

struct RecoveryOnly;
impl AutomationRunResolver for RecoveryOnly {
    fn bind(
        &self,
        _: &RunRecord,
        _: &RevisionRecord,
    ) -> Result<RunSpec, PortError> {
        panic!("recovery must not bind a new attempt")
    }
    fn resolve(&self, _: &RunSpec) -> Result<ResolvedAutomationRun, PortError> {
        panic!("recovery must not resolve credentials or dispatch a model")
    }
}

fn marker_in(
    directory: std::path::PathBuf,
    attempt: &DetachedAttempt,
) -> std::path::PathBuf {
    let key = format!("{}\0{}", attempt.session_id, attempt.submission_id());
    directory.join(format!("{:x}.json", Sha256::digest(key.as_bytes())))
}

fn inspect(executor: &AgentRunExecutor) -> AttemptEvidence {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(executor.reconcile_attempt(&reference()))
        .unwrap()
}

#[test]
fn process_crash_recovery_uses_saved_evidence_without_model_or_tool_reexecution()
 {
    for mode in [
        "missing",
        "empty",
        "started",
        "pending_marker",
        "committed_marker",
    ] {
        let root = Scratch::new();
        let output = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("artifact_integration_tests::detached::recovery::crash_recovery_fixture")
            .arg("--nocapture")
            .env(ROOT_ENV, &root.0).env(MODE_ENV, mode)
            .output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(81),
            "{mode}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let before = fs::read(root.0.join("provider-request-count")).unwrap();
        if mode == "pending_marker" {
            // This path is calculated from the store location written by the
            // child, without constructing a host that might repair it first.
            let path = fs::read_to_string(root.0.join("marker-path")).unwrap();
            assert!(
                !Path::new(&path).exists(),
                "history committed while marker did not"
            );
        }
        let handle =
            AgentHandle::with_config(root.0.clone(), LlmConfig::default());
        let executor =
            AgentRunExecutor::new(handle.clone(), Arc::new(RecoveryOnly));
        let evidence = inspect(&executor);
        match mode {
            "empty" => {
                assert_eq!(evidence.submission, SubmissionEvidence::Missing);
            }
            "missing" | "started" => {
                assert_eq!(evidence.submission, SubmissionEvidence::Unknown);
            }
            _ => {
                assert_eq!(
                    evidence.history_commit,
                    HistoryCommitState::Committed
                );
                assert_eq!(evidence.artifact_state, ArtifactState::Committed);
                let commit = CommitRef {
                    session_id: reference().session_id,
                    submission_id: reference().submission_id,
                };
                let retried = tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
                    .unwrap()
                    .block_on(executor.retry_commit(&reference(), &commit))
                    .unwrap();
                assert_eq!(retried, evidence);
                let marker =
                    fs::read_to_string(root.0.join("marker-path")).unwrap();
                assert!(Path::new(&marker).is_file());
            }
        }
        assert_eq!(
            inspect(&executor),
            evidence,
            "repeated recovery is stable for {mode}"
        );
        let index = flowy_agent_core::SessionIndex::load(&root.0).unwrap();
        if let Some(session) = index.get(&recovery_attempt().session_id) {
            let count = session
                .messages
                .iter()
                .filter(|message| {
                    message.role == Role::User
                        && message.source == MessageSource::User
                })
                .count();
            assert_eq!(
                count,
                usize::from(mode != "empty"),
                "{mode} has no extra submission"
            );
        }
        assert_eq!(
            fs::read(root.0.join("provider-request-count")).unwrap(),
            before
        );
        drop(executor);
        drop(handle);
    }
}

#[test]
fn crash_recovery_fixture() {
    let Some(root) = std::env::var_os(ROOT_ENV) else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let mode = std::env::var(MODE_ENV).unwrap();
    let handle = AgentHandle::with_config(root.clone(), LlmConfig::default());
    let attempt = recovery_attempt();
    if mode == "missing" {
        fs::write(root.join("provider-request-count"), "0").unwrap();
        std::process::exit(81);
    }
    handle.create_detached(home_spec(&attempt)).unwrap();
    if mode == "empty" {
        fs::write(root.join("provider-request-count"), "0").unwrap();
        std::process::exit(81);
    }
    if mode == "started" {
        let (provider, _) =
            ProviderFixture::start_held_text("must remain incomplete");
        let _running = handle
            .start_detached(spec(&attempt, provider.config()))
            .unwrap();
        wait_for_request(&provider);
        fs::write(
            root.join("provider-request-count"),
            provider.requests.lock().unwrap().len().to_string(),
        )
        .unwrap();
        std::process::exit(81);
    }
    let provider = ProviderFixture::start_with(ProviderMode::DetachedDelivery);
    let index = flowy_agent_core::SessionIndex::load(&root).unwrap();
    let output = index
        .get(&attempt.session_id)
        .unwrap()
        .workspace
        .as_ref()
        .unwrap()
        .root
        .join("outputs")
        .join(attempt.submission_id());
    *provider.output_root.lock().unwrap() = Some(output);
    let marker = marker_in(
        handle.artifact_store().unwrap().root().join("deliveries"),
        &attempt,
    );
    fs::write(root.join("marker-path"), marker.to_str().unwrap()).unwrap();
    let mut run = spec(&attempt, provider.config());
    run.allowed_tools
        .extend(["write_file".into(), "publish_file".into()]);
    if mode == "pending_marker" {
        run.execution_gate = Some(Arc::new(FailMarkerOnFinalRequest {
            credential: provider.config().api_key.unwrap(),
            marker: marker.clone(),
            requests: AtomicUsize::new(0),
        }));
    }
    let result = wait(handle.start_detached(run).unwrap());
    if mode == "pending_marker" {
        assert_eq!(result.execution_error, Some(DetachedError::Storage));
        fs::remove_dir(&marker).unwrap();
        assert!(!marker.exists());
    } else {
        assert!(matches!(
            completion(result.evidence.unwrap()).artifacts,
            DetachedArtifacts::Committed(_)
        ));
        assert!(marker.is_file());
    }
    assert_eq!(provider.requests.lock().unwrap().len(), 3);
    fs::write(root.join("provider-request-count"), "3").unwrap();
    std::process::exit(81);
}

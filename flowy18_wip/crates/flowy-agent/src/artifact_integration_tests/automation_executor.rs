use std::sync::Arc;

use flowy_automation::store::{
    AutomationRecord, AutomationStore, RevisionContext, RevisionRecord,
    RunRecord, StoreMutation,
};
use flowy_automation::*;

use super::{ProviderFixture, Scratch, fs};
use crate::automation_executor::{
    AgentRunExecutor, AutomationRunResolver, ResolvedAutomationRun,
};
use crate::{
    AgentHandle, ExecutionDenied, ExecutionGate, ExecutionMetadata, LlmConfig,
    ModelRoute, ToolCallArgs, ToolRunContext,
};

struct FixtureGate;
impl ExecutionGate for FixtureGate {
    fn model_credential(
        &self,
        _: &ExecutionMetadata,
        _: &ModelRoute,
    ) -> Result<String, ExecutionDenied> {
        Ok("isolated-adapter-fixture".into())
    }
    fn authorize_tool(
        &self,
        _: &ToolRunContext,
        _: &ToolCallArgs,
    ) -> Result<(), ExecutionDenied> {
        Ok(())
    }
}

struct Resolver {
    spec: RunSpec,
    config: LlmConfig,
    unavailable: bool,
}
impl AutomationRunResolver for Resolver {
    fn bind(
        &self,
        _: &RunRecord,
        _: &RevisionRecord,
    ) -> Result<RunSpec, PortError> {
        Ok(self.spec.clone())
    }
    fn resolve(&self, _: &RunSpec) -> Result<ResolvedAutomationRun, PortError> {
        if self.unavailable {
            return Err(PortError::Unavailable);
        }
        Ok(ResolvedAutomationRun {
            config: self.config.clone(),
            skills: vec![],
            gate: Arc::new(FixtureGate),
        })
    }
}

struct NoCancel;
impl CancellationSignal for NoCancel {
    fn requested(&self) -> Option<StopReason> {
        None
    }
    fn cancelled(&self) -> PortFuture<'_, StopReason> {
        Box::pin(std::future::pending())
    }
}

struct Allowed;
impl HostAccess for Allowed {
    fn check<'a>(
        &'a self,
        _: &'a HostAccessRequest,
    ) -> PortFuture<'a, Result<HostAccessDecision, PortError>> {
        Box::pin(async { Ok(HostAccessDecision::Allowed) })
    }
}
impl AdmissionPolicy for Allowed {
    fn check<'a>(
        &'a self,
        _: &'a AdmissionRequest,
    ) -> PortFuture<'a, Result<AdmissionDecision, PortError>> {
        Box::pin(async { Ok(AdmissionDecision::Allowed) })
    }
}
struct FixtureClock;
impl Clock for FixtureClock {
    fn now(&self) -> ClockReading {
        ClockReading {
            utc_ms: 100,
            monotonic: std::time::Duration::from_millis(100),
        }
    }
}

fn fixture(
    root: &Scratch,
    config: &LlmConfig,
) -> (RunSpec, RunRecord, RevisionRecord, AutomationRecord) {
    let principal = PrincipalId::cloud("https://account.example", 71).unwrap();
    let automation_id = AutomationId::generate();
    let run_id = RunId::generate();
    let attempt_id = AttemptId::generate();
    let project = root.0.join("project");
    fs::create_dir_all(&project).unwrap();
    let definition = ManualAutomationInput {
        schedule: None,
        title: PrivateText::new("Adapter fixture"),
        action: ActionSpec::AgentTask {
            prompt: PrivateText::new("write the requested file"),
        },
        workspace: Some(WorkspaceSpec {
            roots: vec![project],
            access: WorkspaceAccess::Write,
        }),
        capabilities: CapabilityRequest {
            tools: vec!["write_file".into()],
            ..CapabilityRequest::default()
        },
        model: Some(ModelBinding {
            selected: ModelRef {
                provider: config.provider.clone(),
                model: config.model.clone(),
            },
            allowed_fallbacks: vec![],
        }),
        budget: RunBudget::new(10_000, 5, 3),
    };
    let revision = Revision::new(1).unwrap();
    let spec = RunSpec {
        automation_id: automation_id.clone(),
        run_id: run_id.clone(),
        attempt_id: attempt_id.clone(),
        revision,
        principal: principal.clone(),
        account_generation: AccountGeneration {
            epoch: GenerationEpoch::generate(),
            sequence: 1,
        },
        session_id: Some(
            SessionRef::parse(format!("automation-{}", attempt_id.as_str()))
                .unwrap(),
        ),
        submission_id: Some(
            SubmissionRef::parse(format!("submission-{:032x}", 0xa1105_u128))
                .unwrap(),
        ),
        source_session_id: None,
        action: definition.action.clone(),
        context_summary: None,
        skill_refs: vec![],
        workspace: definition.workspace.clone(),
        capabilities: definition.capabilities.clone(),
        model: definition.model.clone(),
        credential_ref: Some(CredentialRef {
            principal: principal.clone(),
            provider: config.provider.clone(),
            scope_ref: "fixture".into(),
            credential_key: "fixture-reference".into(),
        }),
        budget: definition.budget.clone(),
        latest_start_at_ms: 10_000,
    };
    let run = RunRecord {
        run_id,
        automation_id: automation_id.clone(),
        revision,
        principal: principal.clone(),
        current_attempt_id: None,
        retry_of_run_id: None,
        occurrence_key: None,
        phase: RunPhase::Queued,
        outcome: None,
        latest_start_at_ms: 10_000,
        source_removed: false,
        spec: None,
        created_at_ms: 10,
        updated_at_ms: 10,
    };
    let record = RevisionRecord {
        automation_id: automation_id.clone(),
        principal: principal.clone(),
        revision,
        definition: definition.clone(),
        context: RevisionContext::default(),
        grant_ref: None,
        created_at_ms: 10,
    };
    let automation = AutomationRecord {
        automation_id,
        principal,
        current_revision: revision,
        state: DefinitionState::Enabled,
        title: definition.title,
        created_at_ms: 10,
        updated_at_ms: 10,
        source_removed: false,
    };
    (spec, run, record, automation)
}

#[test]
fn worker_uses_real_agent_and_restores_same_attempt_without_more_requests() {
    let root = Scratch::new();
    let output = root.0.join("project/result.txt");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    let provider = ProviderFixture::start_project_write(
        output.to_str().unwrap(),
        "real tool output",
    );
    let config = provider.config();
    let (spec, run, revision, automation) = fixture(&root, &config);
    let agent = AgentHandle::with_config(root.0.clone(), LlmConfig::default());
    let foreground = agent.create_home_session().unwrap();
    let executor = Arc::new(AgentRunExecutor::new(
        agent.clone(),
        Arc::new(Resolver {
            spec: spec.clone(),
            config,
            unavailable: false,
        }),
    ));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let store = AutomationStore::open(root.0.clone(), Arc::new(()))
            .await
            .unwrap();
        store
            .commit(
                spec.principal.clone(),
                vec![
                    StoreMutation::CreateAutomation {
                        automation,
                        revision,
                    },
                    StoreMutation::InsertRun(run),
                ],
            )
            .await
            .unwrap();
        let worker = AutomationWorker::new(
            store.clone(),
            executor.clone(),
            Arc::new(Allowed),
            Arc::new(Allowed),
            Arc::new(FixtureClock),
        );
        let caller = CallerContext {
            principal: spec.principal.clone(),
            account_generation: spec.account_generation.clone(),
            source: CallerSource::UserInterface,
            grant_refs: vec![],
        };
        let cycle = worker.run_once(&caller).await.unwrap();
        assert_eq!(
            cycle,
            WorkerCycle::Settled {
                run_id: spec.run_id.clone()
            }
        );
        assert_eq!(fs::read_to_string(&output).unwrap(), "real tool output");
        assert_eq!(
            agent.active_session_id().as_deref(),
            Some(foreground.as_str())
        );
        let stored = store
            .run(spec.principal.clone(), spec.run_id.clone())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.outcome, Some(RunOutcome::Succeeded));
        assert_eq!(stored.current_attempt_id, Some(spec.attempt_id.clone()));
        let reference = AttemptRef {
            principal: spec.principal.clone(),
            run_id: spec.run_id.clone(),
            attempt_id: spec.attempt_id.clone(),
            session_id: spec.session_id.clone().unwrap(),
            submission_id: spec.submission_id.clone().unwrap(),
        };
        let requests = provider.requests.lock().unwrap().len();
        assert!(
            requests >= 2,
            "real model loop must dispatch a tool and a final response"
        );
        let evidence = executor.reconcile_attempt(&reference).await.unwrap();
        assert_eq!(evidence.execution_outcome, ExecutionOutcome::Completed);
        executor
            .retry_commit(&reference, evidence.commit_ref.as_ref().unwrap())
            .await
            .unwrap();
        assert_eq!(provider.requests.lock().unwrap().len(), requests);
        let mut crossed = reference;
        crossed.principal =
            PrincipalId::cloud("https://account.example", 72).unwrap();
        assert_eq!(
            executor.reconcile_attempt(&crossed).await,
            Err(PortError::InvalidRequest)
        );
        store.shutdown().await.unwrap();
    });
    drop(runtime);
    drop(executor);
    drop(agent);
}

#[test]
fn preparation_has_no_session_or_submission_and_missing_config_never_uses_foreground()
 {
    let root = Scratch::new();
    let provider = ProviderFixture::start_delayed_text(0, "adapter response");
    let config = provider.config();
    let (spec, run, revision, _) = fixture(&root, &config);
    let agent = AgentHandle::with_config(root.0.clone(), config.clone());
    let executor = AgentRunExecutor::new(
        agent.clone(),
        Arc::new(Resolver {
            spec: spec.clone(),
            config: config.clone(),
            unavailable: false,
        }),
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let prepared = executor
            .prepare(&run, &revision, Arc::new(NoCancel))
            .await
            .unwrap();
        assert_eq!(provider.requests.lock().unwrap().len(), 0);
        let reference = AttemptRef {
            principal: spec.principal.clone(),
            run_id: spec.run_id.clone(),
            attempt_id: spec.attempt_id.clone(),
            session_id: spec.session_id.clone().unwrap(),
            submission_id: spec.submission_id.clone().unwrap(),
        };
        assert_eq!(
            executor
                .reconcile_attempt(&reference)
                .await
                .unwrap()
                .submission,
            SubmissionEvidence::Unknown
        );
        drop(prepared);
        assert!(!agent.workspace_writers_quarantined());
        let missing = AgentRunExecutor::new(
            agent.clone(),
            Arc::new(Resolver {
                spec: spec.clone(),
                config: config.clone(),
                unavailable: true,
            }),
        );
        assert!(matches!(
            missing.prepare(&run, &revision, Arc::new(NoCancel)).await,
            Err(PortError::Unavailable)
        ));
        let mut altered = spec.clone();
        altered.action = ActionSpec::AgentTask {
            prompt: PrivateText::new("replacement"),
        };
        let crossed = AgentRunExecutor::new(
            agent.clone(),
            Arc::new(Resolver {
                spec: altered,
                config: config.clone(),
                unavailable: false,
            }),
        );
        assert!(matches!(
            crossed.prepare(&run, &revision, Arc::new(NoCancel)).await,
            Err(PortError::InvalidRequest)
        ));
        assert_eq!(provider.requests.lock().unwrap().len(), 0);
    });
    drop(runtime);
    drop(executor);
    drop(agent);
}

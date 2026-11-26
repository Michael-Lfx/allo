//! Manual integration smoke with a mandatory new, isolated data root.
//! Reminder needs no credential. `--agent` uses only explicit `FLOWY_LLM_*` env
//! inputs and performs a real provider request; it never supplies a demo key.
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use flowy_agent::automation_executor::{
    AgentRunExecutor, AutomationRunResolver, ResolvedAutomationRun,
};
use flowy_agent::{
    AgentHandle, ExecutionDenied, ExecutionGate, ExecutionMetadata, LlmConfig,
    ModelRoute, ToolCallArgs, ToolRunContext,
};
use flowy_automation::store::{
    AutomationRecord, AutomationStore, RevisionContext, RevisionRecord,
    RunRecord, StoreMutation,
};
use flowy_automation::{
    AccountGeneration, ActionSpec, AdmissionDecision, AdmissionPolicy,
    AdmissionRequest, AttemptId, AutomationId, AutomationWorker, CallerContext,
    CallerSource, CapabilityRequest, CapacityReason, Clock, ClockReading,
    CredentialRef, DefinitionState, GenerationEpoch, HostAccess,
    HostAccessDecision, HostAccessRequest, ManualAutomationInput, ModelBinding,
    ModelRef, PortError, PortFuture, PrincipalId, PrivateText, Revision,
    RunBudget, RunId, RunPhase, RunSpec, SessionRef, StopReason, SubmissionRef,
};

struct SmokeClock(Instant);
impl Clock for SmokeClock {
    fn now(&self) -> ClockReading {
        ClockReading {
            utc_ms: now(),
            monotonic: self.0.elapsed(),
        }
    }
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|value| i64::try_from(value.as_millis()).ok())
        .unwrap_or(i64::MIN)
}

struct IsolatedPolicy {
    root: PathBuf,
    principal: PrincipalId,
    account_generation: AccountGeneration,
}
impl HostAccess for IsolatedPolicy {
    fn check<'a>(
        &'a self,
        request: &'a HostAccessRequest,
    ) -> PortFuture<'a, Result<HostAccessDecision, PortError>> {
        Box::pin(async move {
            Ok(
                if request.principal == self.principal
                    && request.account_generation == self.account_generation
                {
                    HostAccessDecision::Allowed
                } else {
                    HostAccessDecision::Denied
                },
            )
        })
    }
}
impl AdmissionPolicy for IsolatedPolicy {
    fn check<'a>(
        &'a self,
        _: &'a AdmissionRequest,
    ) -> PortFuture<'a, Result<AdmissionDecision, PortError>> {
        Box::pin(async move {
            let root = self.root.clone();
            let used =
                tokio::task::spawn_blocking(move || directory_bytes(&root))
                    .await
                    .map_err(|_| PortError::Unavailable)?
                    .map_err(|_| PortError::Storage)?;
            Ok(if used < 16 * 1024 * 1024 {
                AdmissionDecision::Allowed
            } else {
                AdmissionDecision::SuspendedStorage(
                    CapacityReason::QuotaExceeded,
                )
            })
        })
    }
}
fn directory_bytes(path: &Path) -> std::io::Result<u64> {
    let mut total = 0_u64;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        total = total.saturating_add(if metadata.is_dir() {
            directory_bytes(&entry.path())?
        } else {
            metadata.len()
        });
    }
    Ok(total)
}

struct Resolver {
    spec: RunSpec,
    config: LlmConfig,
}
impl AutomationRunResolver for Resolver {
    fn bind(
        &self,
        _: &RunRecord,
        _: &RevisionRecord,
    ) -> Result<RunSpec, PortError> {
        Ok(self.spec.clone())
    }
    fn resolve(
        &self,
        spec: &RunSpec,
    ) -> Result<ResolvedAutomationRun, PortError> {
        if spec != &self.spec {
            return Err(PortError::Forbidden);
        }
        Ok(ResolvedAutomationRun {
            config: self.config.clone(),
            skills: vec![],
            gate: Arc::new(EnvironmentGate {
                execution: flowy_agent::automation_executor::execution_metadata(
                    spec,
                ),
                provider: self.config.provider.clone(),
                model: self.config.model.clone(),
                base_url: self.config.base_url.clone(),
            }),
        })
    }
}
struct EnvironmentGate {
    execution: ExecutionMetadata,
    provider: String,
    model: String,
    base_url: String,
}
impl ExecutionGate for EnvironmentGate {
    fn model_credential(
        &self,
        execution: &ExecutionMetadata,
        route: &ModelRoute,
    ) -> Result<String, ExecutionDenied> {
        if execution != &self.execution
            || route.provider != self.provider
            || route.model != self.model
            || route.base_url != self.base_url
        {
            return Err(ExecutionDenied::OwnerMismatch);
        }
        std::env::var("FLOWY_LLM_API_KEY")
            .ok()
            .filter(|key| !key.trim().is_empty())
            .ok_or(ExecutionDenied::CredentialUnavailable)
    }
    fn authorize_tool(
        &self,
        _: &ToolRunContext,
        _: &ToolCallArgs,
    ) -> Result<(), ExecutionDenied> {
        Err(ExecutionDenied::ToolDenied)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() < 2 || args[0] != "--isolated-root" {
        return Err("usage: automation_smoke --isolated-root <new-directory> [--agent]; the directory must not exist".into());
    }
    let root = std::path::absolute(&args[1])?;
    std::fs::create_dir(&root)?;
    let agent_mode = args.get(2).is_some_and(|arg| arg == "--agent");
    let principal = PrincipalId::cloud("isolated-smoke.local", 1)?;
    let generation = AccountGeneration {
        epoch: GenerationEpoch::generate(),
        sequence: 1,
    };
    let automation_id = AutomationId::generate();
    let run_id = RunId::generate();
    let attempt_id = AttemptId::generate();
    let revision = Revision::new(1).ok_or("revision")?;
    let created = now();
    let mut config = LlmConfig::default();
    if agent_mode {
        config.provider = std::env::var("FLOWY_LLM_PROVIDER")?;
        config.model = std::env::var("FLOWY_LLM_MODEL")?;
        config.base_url = std::env::var("FLOWY_LLM_BASE_URL")?;
        if std::env::var("FLOWY_LLM_API_KEY")?.trim().is_empty() {
            return Err("FLOWY_LLM_API_KEY is empty".into());
        }
    }
    let definition = ManualAutomationInput {
        schedule: None,
        title: PrivateText::new("Isolated smoke"),
        action: if agent_mode {
            ActionSpec::AgentTask {
                prompt: PrivateText::new(
                    "Reply with a short confirmation. Do not use tools.",
                ),
            }
        } else {
            ActionSpec::Reminder {
                text: PrivateText::new("Isolated manual reminder"),
            }
        },
        workspace: None,
        capabilities: CapabilityRequest::default(),
        model: agent_mode.then(|| ModelBinding {
            selected: ModelRef {
                provider: config.provider.clone(),
                model: config.model.clone(),
            },
            allowed_fallbacks: vec![],
        }),
        budget: agent_mode.then(|| {
            RunBudget::new(30_000, 2, 1).expect("finite smoke budget")
        }),
    };
    let spec = RunSpec {
        automation_id: automation_id.clone(),
        run_id: run_id.clone(),
        attempt_id: attempt_id.clone(),
        revision,
        principal: principal.clone(),
        account_generation: generation.clone(),
        session_id: agent_mode.then(|| {
            SessionRef::parse(format!("automation-{}", attempt_id.as_str()))
                .expect("generated session")
        }),
        submission_id: agent_mode.then(|| {
            SubmissionRef::parse(format!(
                "submission-{}",
                attempt_id.as_str().replace('-', "")
            ))
            .expect("generated submission")
        }),
        source_session_id: None,
        action: definition.action.clone(),
        context_summary: None,
        skill_refs: vec![],
        workspace: None,
        capabilities: definition.capabilities.clone(),
        model: definition.model.clone(),
        credential_ref: agent_mode.then(|| CredentialRef {
            principal: principal.clone(),
            provider: config.provider.clone(),
            scope_ref: "isolated-env".into(),
            credential_key: "FLOWY_LLM_API_KEY".into(),
        }),
        budget: definition.budget.clone(),
        latest_start_at_ms: created + 60_000,
    };
    let agent = AgentHandle::with_config(root.clone(), LlmConfig::default());
    let executor = Arc::new(AgentRunExecutor::new(
        agent.clone(),
        Arc::new(Resolver { spec, config }),
    ));
    let policy = Arc::new(IsolatedPolicy {
        root: root.clone(),
        principal: principal.clone(),
        account_generation: generation.clone(),
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let store = AutomationStore::open(root.clone(), Arc::new(())).await?;
        store
            .commit(
                principal.clone(),
                vec![
                    StoreMutation::CreateAutomation {
                        automation: AutomationRecord {
                            automation_id: automation_id.clone(),
                            principal: principal.clone(),
                            current_revision: revision,
                            state: DefinitionState::Enabled,
                            title: definition.title.clone(),
                            created_at_ms: created,
                            updated_at_ms: created,
                            source_removed: false,
                        },
                        revision: RevisionRecord {
                            automation_id: automation_id.clone(),
                            principal: principal.clone(),
                            revision,
                            definition,
                            context: RevisionContext::default(),
                            grant_ref: None,
                            created_at_ms: created,
                        },
                    },
                    StoreMutation::InsertRun(RunRecord {
                        run_id: run_id.clone(),
                        automation_id,
                        revision,
                        principal: principal.clone(),
                        current_attempt_id: None,
                        retry_of_run_id: None,
                        occurrence_key: None,
                        phase: RunPhase::Queued,
                        outcome: None,
                        latest_start_at_ms: created + 60_000,
                        source_removed: false,
                        spec: None,
                        created_at_ms: created,
                        updated_at_ms: created,
                    }),
                ],
            )
            .await?;
        let worker = AutomationWorker::new(
            store.clone(),
            executor.clone(),
            policy.clone(),
            policy,
            Arc::new(SmokeClock(Instant::now())),
        );
        let caller = CallerContext {
            principal: principal.clone(),
            account_generation: generation,
            source: CallerSource::UserInterface,
            grant_refs: vec![],
        };
        let result = worker.run_once(&caller).await?;
        println!("{result:?}");
        let run = store
            .run(principal, run_id)
            .await?
            .ok_or("missing durable run")?;
        println!(
            "phase={:?}, outcome={:?}, isolated_root={}",
            run.phase,
            run.outcome,
            root.display()
        );
        worker.request_stop(StopReason::Shutdown);
        store.shutdown().await?;
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;
    drop(runtime);
    drop(executor);
    drop(agent);
    Ok(())
}

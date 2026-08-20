//! Live evaluation harness: production `AgentEngine` on an isolated workspace.
//!
//! Never registers in `AgentRuntimeRegistry`. Each case binds Session Observation
//! under a dedicated `conversation_id` (`session_kind = eval`). An optional
//! [`EvalSessionBridge`] also creates a conversation shell so `/conversation/:id`
//! and session-observation APIs can inspect the same run.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use nomi_agent::bootstrap::AgentBootstrap;
use nomi_agent::output::OutputSink;
use nomi_agent::task_profile::{CodingEnvContext, TaskProfile};
use nomi_agent::ObservationSession;
use nomi_agent_eval::{
    collect_workspace_artifacts, materialize_files, Case, ConversationEvalHarness, EvalCaseTrace,
    HarnessError, TurnTranscript,
};
use nomi_agent_trace::{ExecutionStatus, ObservationIds, ObservationRecorder};
use nomi_config::config::Config;
use nomifun_common::AppError;
use nomifun_db::{IProviderModelRepository, IProviderRepository};
use uuid::Uuid;

use crate::factory::provider_config::resolve_provider_config;

use super::capture::EvalCaptureSink;
use super::session_bridge::{
    EvalCaseTurnUsage, EvalSessionBridge, OpenEvalCaseSession, RecordEvalCaseTurn,
};

const DEFAULT_TIMEOUT_SECS: u64 = 120;

#[derive(Clone)]
pub struct LiveEvalTrace {
    pub case_id: String,
    pub conversation_id: String,
    pub sink: Arc<EvalCaptureSink>,
    pub workspace: PathBuf,
}

impl LiveEvalTrace {
    pub fn snapshot(&self) -> EvalCaseTrace {
        let mut trace = self.sink.snapshot_trace(&self.case_id, true);
        trace.artifacts = collect_workspace_artifacts(&self.workspace);
        trace.conversation_id = Some(self.conversation_id.clone());
        trace
    }
}

pub struct LiveNomiHarness {
    pub data_dir: PathBuf,
    pub provider_repo: Arc<dyn IProviderRepository>,
    pub provider_model_repo: Arc<dyn IProviderModelRepository>,
    pub encryption_key: [u8; 32],
    /// Parent run workspace (business-named directory created at run start).
    pub work_root: PathBuf,
    pub run_workspace_label: String,
    pub traces_dir: PathBuf,
    pub provider_id: String,
    pub model: String,
    pub profile_override: Option<String>,
    pub live_trace: Arc<Mutex<Option<LiveEvalTrace>>>,
    pub run_id: String,
    pub suite: String,
    pub user_id: Option<String>,
    pub session_bridge: Option<Arc<dyn EvalSessionBridge>>,
}

#[async_trait]
impl ConversationEvalHarness for LiveNomiHarness {
    async fn run_case(&self, case: &Case) -> Result<TurnTranscript, HarnessError> {
        self.run_case_inner(case).await.map_err(|error| HarnessError::Failed {
            case_id: case.id.clone(),
            message: error.to_string(),
        })
    }
}

impl LiveNomiHarness {
    async fn run_case_inner(&self, case: &Case) -> Result<TurnTranscript, AppError> {
        let workspace = self.work_root.join(sanitize_case_dir(&case.id));
        std::fs::create_dir_all(&workspace)
            .map_err(|e| AppError::Internal(format!("eval workspace: {e}")))?;
        materialize_files(&workspace, &case.workspace_files)
            .map_err(|e| AppError::Internal(format!("eval materialize: {e}")))?;

        // Canonical UUIDv7: conversation messages, billing, and Observation share it.
        let root_turn_id = Uuid::now_v7().to_string();

        let conversation_id = if let (Some(bridge), Some(user_id)) =
            (self.session_bridge.as_ref(), self.user_id.as_ref())
        {
            match bridge
                .open_case_session(OpenEvalCaseSession {
                    user_id: user_id.clone(),
                    run_id: self.run_id.clone(),
                    suite: self.suite.clone(),
                    case_id: case.id.clone(),
                    case_category: case.category.clone(),
                    prompt: case.prompt.clone(),
                    workspace: workspace.clone(),
                    run_workspace: self.work_root.clone(),
                    run_workspace_label: self.run_workspace_label.clone(),
                    provider_id: self.provider_id.clone(),
                    model: self.model.clone(),
                    task_profile: self
                        .profile_override
                        .clone()
                        .or_else(|| case.task_profile.clone()),
                })
                .await
            {
                Ok(id) => id,
                Err(error) => {
                    tracing::warn!(
                        case_id = %case.id,
                        error = %error,
                        "eval session shell create failed; continuing with observation-only binding"
                    );
                    Uuid::now_v7().to_string()
                }
            }
        } else {
            Uuid::now_v7().to_string()
        };

        let mut config = resolve_provider_config(
            &self.provider_repo,
            &self.provider_model_repo,
            &self.encryption_key,
            &self.provider_id,
            &self.model,
            &workspace,
        )
        .await?;
        let max_turns = case.budgets.max_turns.map(|n| n as usize);
        isolate_eval_config(&mut config, &workspace, max_turns);

        let sink = Arc::new(EvalCaptureSink::default());
        {
            let mut slot = self.live_trace.lock().unwrap_or_else(|e| e.into_inner());
            *slot = Some(LiveEvalTrace {
                case_id: case.id.clone(),
                conversation_id: conversation_id.clone(),
                sink: sink.clone(),
                workspace: workspace.clone(),
            });
        }
        let output: Arc<dyn OutputSink> = sink.clone();
        let coding = is_coding_case(case, self.profile_override.as_deref());

        let observation = ObservationSession::new(ObservationRecorder::shared(&self.data_dir));
        observation.bind_ids_with_preview(
            ObservationIds {
                conversation_id: Some(conversation_id.clone()),
                msg_id: Some(root_turn_id.clone()),
                root_turn_id: Some(root_turn_id.clone()),
                session_kind: Some("eval".into()),
                ..Default::default()
            },
            Some(case.prompt.as_str()),
        );

        let bootstrap = AgentBootstrap::new(config, workspace.to_string_lossy().into_owned(), output)
            .install_embedded_agent_execution(false)
            .disable_web_search()
            .coding_boundary(coding)
            .observation(Arc::clone(&observation));
        let mut built = bootstrap
            .build()
            .await
            .map_err(|e| AppError::Internal(format!("eval bootstrap: {e}")))?;

        if coding {
            built.engine.install_coding_harness(
                Some(CodingEnvContext {
                    cwd: workspace.clone(),
                    write_root: Some(workspace.clone()),
                }),
                nomi_agent::task_profile::CodingConfig::default(),
            );
        } else {
            built.engine.set_task_profile(TaskProfile::Office);
        }

        let timeout = Duration::from_secs(case.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS).clamp(1, 600));
        let started = Instant::now();
        let exec = tokio::time::timeout(
            timeout,
            nomi_providers::with_flowy_billing_turn_id(
                root_turn_id.clone(),
                built.engine.execute_turn(&case.prompt, &root_turn_id),
            ),
        )
        .await;
        let (captured_text, tool_names, tool_error_count) = sink.snapshot();
        let elapsed_ms = started.elapsed().as_millis() as u64;

        let outcome: Result<(String, u32, Option<String>, u64, u64), AppError> = match exec {
            Ok(Ok(result)) => {
                let stop = stop_reason_label(result.stop_reason);
                let assistant_text = if result.text.trim().is_empty() {
                    captured_text.clone()
                } else {
                    result.text
                };
                observation.emit_turn_end(
                    ExecutionStatus::Completed,
                    elapsed_ms,
                    Some(stop.as_str()),
                    Some(serde_json::json!({
                        "input_tokens": result.usage.input_tokens,
                        "output_tokens": result.usage.output_tokens,
                    })),
                );
                Ok((
                    assistant_text,
                    result.turns as u32,
                    Some(stop),
                    result.usage.input_tokens,
                    result.usage.output_tokens,
                ))
            }
            Ok(Err(error)) => {
                observation.emit_turn_end(
                    ExecutionStatus::Failed,
                    elapsed_ms,
                    Some("error"),
                    None,
                );
                Err(AppError::Internal(format!("eval execute_turn: {error}")))
            }
            Err(_) => {
                observation.emit_turn_end(
                    ExecutionStatus::Failed,
                    elapsed_ms,
                    Some("timeout"),
                    None,
                );
                Err(AppError::Timeout(format!("eval case {} timed out", case.id)))
            }
        };

        let artifacts = collect_workspace_artifacts(&workspace);
        let mut trace = sink.snapshot_trace(&case.id, false);
        match &outcome {
            Ok((assistant_text, _, _, _, _)) => {
                trace.assistant_text = assistant_text.clone();
            }
            Err(_) if trace.assistant_text.is_empty() => {
                trace.assistant_text = captured_text;
            }
            Err(_) => {}
        }
        trace.artifacts = artifacts.clone();
        trace.conversation_id = Some(conversation_id.clone());
        persist_case_trace(&self.traces_dir, &case.id, &trace)?;
        {
            let mut slot = self.live_trace.lock().unwrap_or_else(|e| e.into_inner());
            *slot = None;
        }

        if let (Some(bridge), Some(user_id)) = (self.session_bridge.as_ref(), self.user_id.as_ref())
        {
            let (assistant_for_shell, turns, input_tokens, output_tokens, exec_ok) = match &outcome {
                Ok((text, turns, _, input, output)) => {
                    (text.as_str(), *turns, *input, *output, true)
                }
                Err(_) => (trace.assistant_text.as_str(), 0, 0, 0, false),
            };
            if let Err(error) = bridge
                .record_case_turn(RecordEvalCaseTurn {
                    user_id: user_id.clone(),
                    conversation_id: conversation_id.clone(),
                    root_turn_id: root_turn_id.clone(),
                    user_prompt: case.prompt.clone(),
                    assistant_text: assistant_for_shell.to_owned(),
                    trajectory: trace.events.clone(),
                    usage: EvalCaseTurnUsage {
                        input_tokens,
                        output_tokens,
                        elapsed_ms,
                        turns,
                    },
                    success: Some(exec_ok),
                })
                .await
            {
                tracing::warn!(
                    conversation_id = %conversation_id,
                    error = %error,
                    "eval session turn persist failed"
                );
            }
        }

        let (assistant_text, turns, stop_reason, input_tokens, output_tokens) = outcome?;
        Ok(TurnTranscript {
            assistant_text,
            tool_names,
            turns,
            stop_reason,
            input_tokens,
            output_tokens,
            tool_error_count,
            workspace: Some(workspace),
            trajectory: trace.events,
            artifacts,
            conversation_id: Some(conversation_id),
        })
    }
}

fn persist_case_trace(
    traces_dir: &Path,
    case_id: &str,
    trace: &EvalCaseTrace,
) -> Result<(), AppError> {
    std::fs::create_dir_all(traces_dir)
        .map_err(|e| AppError::Internal(format!("eval traces dir: {e}")))?;
    let path = traces_dir.join(format!("{}.json", sanitize_case_dir(case_id)));
    let json = serde_json::to_string_pretty(trace)
        .map_err(|e| AppError::Internal(format!("eval trace json: {e}")))?;
    std::fs::write(path, json).map_err(|e| AppError::Internal(format!("eval trace write: {e}")))
}

pub(crate) fn isolate_eval_config(config: &mut Config, workspace: &Path, max_turns: Option<usize>) {
    // Session *persistence* stays off so nomi session files do not collide with
    // the conversation shell. Observation is wired explicitly above.
    config.session.enabled = false;
    config.mcp.servers.clear();
    config.file_cache.enabled = false;
    config.tools.auto_approve = true;
    config.tools.browser.enabled = false;
    config.tools.computer.enabled = false;
    config.tools.web.enabled = false;
    config.tools.write_root = workspace.to_string_lossy().into_owned();
    config.tools.builtin_allowlist.clear();
    config.memory.distill_enabled = false;
    config.moa.enabled = false;
    config.max_turns = max_turns;
}

fn is_coding_case(case: &Case, override_profile: Option<&str>) -> bool {
    TaskProfile::parse(override_profile.or(case.task_profile.as_deref())).is_coding()
}

pub(crate) fn sanitize_case_dir(id: &str) -> String {
    let cleaned: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "case".into()
    } else {
        cleaned
    }
}

fn stop_reason_label(reason: nomi_types::message::StopReason) -> String {
    match reason {
        nomi_types::message::StopReason::EndTurn => "end_turn".into(),
        nomi_types::message::StopReason::ToolUse => "tool_use".into(),
        nomi_types::message::StopReason::MaxTokens => "max_tokens".into(),
        nomi_types::message::StopReason::MaxTurns => "max_turns".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_config::config::CliArgs;
    use tempfile::tempdir;

    #[test]
    fn isolate_config_disables_host_side_effects() {
        let dir = tempdir().unwrap();
        let args = CliArgs {
            provider: Some("openai".into()),
            api_key: Some("sk-test".into()),
            base_url: None,
            model: Some("test-model".into()),
            max_tokens: Some(128),
            max_turns: Some(3),
            system_prompt: None,
            profile: None,
            auto_approve: false,
            project_dir: Some(dir.path().to_path_buf()),
        };
        let mut config = Config::resolve(&args).expect("config");
        config.mcp.servers.insert(
            "demo".into(),
            nomi_config::config::McpServerConfig {
                transport: nomi_config::config::TransportType::Stdio,
                command: Some("true".into()),
                args: None,
                env: None,
                url: None,
                headers: None,
                deferred: None,
            },
        );
        isolate_eval_config(&mut config, dir.path(), Some(6));
        assert!(config.mcp.servers.is_empty());
        assert!(!config.session.enabled);
        assert!(config.mcp.servers.is_empty());
        assert!(config.tools.auto_approve);
        assert!(!config.tools.browser.enabled);
        assert!(!config.tools.computer.enabled);
        assert!(!config.tools.web.enabled);
        assert!(!config.file_cache.enabled);
        assert!(!config.memory.distill_enabled);
        assert!(!config.moa.enabled);
        assert_eq!(config.max_turns, Some(6));
        assert!(!config.tools.write_root.is_empty());
    }

    #[test]
    fn coding_profile_follows_case_then_override() {
        let mut case = nomi_agent_eval::load_bundled_manifest("harness_control")
            .unwrap()
            .cases
            .remove(0);
        assert!(is_coding_case(&case, None));
        case.task_profile = Some("office".into());
        assert!(!is_coding_case(&case, None));
        assert!(is_coding_case(&case, Some("coding")));
    }
}

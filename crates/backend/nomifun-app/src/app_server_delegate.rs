//! Engine-backed `nomi_delegate`: the durable, planned deployment a host with its
//! own Agent Execution facade exposes to its sessions (`16` §7 决策 3).
//!
//! The model-facing contract lives in `nomi_agent::host_delegate_tool`; this module
//! is the composition that answers it. It is installed late — the Agent factory is
//! built before this process's execution facade exists — so `AppServices` carries a
//! [`DelegateSinkProviderSlot`] and `router::state::build_agent_execution_engine`
//! installs the provider once the facade is available.
//!
//! Scope, deliberately: one installation owner, one conversation per sink, planned
//! only. Parallel fan-out from the model is not part of the Store contract (hoisted
//! work belongs to the DAG the Planner materializes), and the model may not set
//! parallelism, planning-approval or re-planning policy — those come from the bound
//! Team template and server policy.

use std::sync::Arc;

use async_trait::async_trait;
use nomifun_agent_execution::AgentExecutionEngine;
use nomifun_ai_agent::factory::delegate::{
    DelegateSinkProvider, DelegateSinkProviderSlot, HostDelegateSink,
};
use nomifun_api_types::{
    AgentExecutionDetail, CreateAgentExecutionRequest, CreateExecutionFromTemplateRequest,
    ExecutionModelPool, ExecutionModelRef,
};
use nomifun_common::{
    AdaptationPolicy, AgentExecutionReceipt, AgentExecutionStatus, AppError, ConversationId,
    DelegationPolicy, PlanGate, ProviderWithModel, UserId,
};
use nomifun_conversation::ConversationService;

/// Hand out one engine-backed sink per leader conversation.
pub struct EngineDelegateSinkProvider {
    engine: Arc<AgentExecutionEngine>,
    conversations: ConversationService,
}

impl EngineDelegateSinkProvider {
    pub fn new(engine: Arc<AgentExecutionEngine>, conversations: ConversationService) -> Self {
        Self {
            engine,
            conversations,
        }
    }
}

impl DelegateSinkProvider for EngineDelegateSinkProvider {
    fn sink_for(&self, owner_id: &str, conversation_id: &str) -> Arc<dyn HostDelegateSink> {
        Arc::new(EngineDelegateSink {
            engine: Arc::clone(&self.engine),
            conversations: self.conversations.clone(),
            owner_id: owner_id.to_owned(),
            conversation_id: conversation_id.to_owned(),
        })
    }
}

/// Install the process's single engine-backed provider.
///
/// Called by the composition root once the execution facade exists; a second
/// install is a conflict rather than a silent replacement (two providers would mean
/// two facades behind one tool name).
pub fn install_engine_delegate_sink_provider(
    slot: &DelegateSinkProviderSlot,
    engine: Arc<AgentExecutionEngine>,
    conversations: ConversationService,
) -> Result<(), AppError> {
    slot.install(Arc::new(EngineDelegateSinkProvider::new(
        engine,
        conversations,
    )))
}

struct EngineDelegateSink {
    engine: Arc<AgentExecutionEngine>,
    conversations: ConversationService,
    owner_id: String,
    conversation_id: String,
}

#[async_trait]
impl HostDelegateSink for EngineDelegateSink {
    async fn plan(&self, goal: &str) -> Result<AgentExecutionReceipt, String> {
        plan_via_engine(
            &self.engine,
            &self.conversations,
            &self.owner_id,
            &self.conversation_id,
            goal,
        )
        .await
        .map_err(|error| error.to_string())
    }
}

/// Materialize one planned execution for `conversation_id`.
///
/// Read as one straight line: fetch the leader conversation, decide "append to the
/// execution this Attempt belongs to" vs "start a new execution", then let the
/// facade own every lifecycle write. `goal` is the only thing the model supplied.
async fn plan_via_engine(
    engine: &AgentExecutionEngine,
    conversations: &ConversationService,
    owner_id: &str,
    conversation_id: &str,
    goal: &str,
) -> Result<AgentExecutionReceipt, AppError> {
    // The binding, not model input: validate before any execution row is written.
    let owner = UserId::parse(owner_id)
        .map_err(|error| AppError::BadRequest(format!("invalid owner: {error}")))?;
    let owner_id = owner.as_str();
    validate_conversation_id(conversation_id)?;

    let conversation = conversations.get(owner_id, conversation_id).await?;
    if conversation.delegation_policy == DelegationPolicy::Disabled {
        return Err(AppError::Conflict(
            "delegation is disabled for this conversation".to_owned(),
        ));
    }

    let actor = engine
        .agent_caller_for_delegation(owner_id, conversation_id)
        .await?;

    // An Attempt conversation appends work to the execution it already belongs to;
    // it never starts a second aggregate.
    if let Some(execution_id) = engine
        .execution_for_attempt_conversation(owner_id, conversation_id)
        .await?
    {
        let detail = engine.get(owner_id, &execution_id).await?;
        let inherited = execution_model_pool(&detail)?;
        let (detail, added) = engine
            .delegate_from_attempt(
                owner_id,
                &actor,
                conversation_id,
                goal.to_owned(),
                inherited.clone(),
                Some(inherited),
                None,
            )
            .await?;
        return Ok(receipt(
            detail.execution.execution_id,
            detail.execution.status,
            format!(
                "Delegated work was appended to the current execution ({} new step(s)). \
                 End this turn; the host runs the plan and reports the result back here.",
                added.len()
            ),
        ));
    }

    // Host policy, never model input: the request carries the goal and the leader's
    // own model authority, nothing about parallelism or approval.
    let request = CreateAgentExecutionRequest {
        goal: goal.to_owned(),
        work_dir: conversation_work_dir(&conversation.extra),
        model_pool: conversation_model_pool(
            conversation.execution_model_pool.as_ref(),
            conversation.model.as_ref(),
        ),
        delegation_policy: conversation.delegation_policy,
        plan_gate: PlanGate::Automatic,
        adaptation_policy: AdaptationPolicy::Fixed,
        decision_policy: conversation.decision_policy,
        max_parallel: None,
        lead_conversation_id: Some(conversation_id.to_owned()),
        lead_model: lead_model(conversation.model.as_ref()),
        steps: None,
    };

    // A conversation bound to a Team template instantiates it: participant
    // authority, member pool and template policy all come from the template.
    let created = match conversation
        .execution_template_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(template_id) => {
            // `lead_model: None` on purpose. The template's `sort_order = 0`
            // participant *is* the Team's lead (it was materialized from the Team
            // Definition's `lead_agent_id`), so promoting the Leader Conversation's
            // own model would replace Definition authority with "whatever model
            // this chat happens to use" — and when that model is not in the member
            // pool, `promote_lead_model` refuses the whole run. The non-template
            // branch below keeps the conversation's model, because there the
            // conversation *is* the authority.
            engine
                .create_from_template_for_conversation(
                    owner_id,
                    &actor,
                    &conversation,
                    template_id,
                    CreateExecutionFromTemplateRequest {
                        goal: request.goal,
                        work_dir: request.work_dir,
                        max_parallel: request.max_parallel,
                        delegation_policy: request.delegation_policy,
                        plan_gate: request.plan_gate,
                        adaptation_policy: request.adaptation_policy,
                        decision_policy: request.decision_policy,
                        lead_conversation_id: request.lead_conversation_id,
                        lead_model: None,
                        steps: request.steps,
                    },
                )
                .await?
        }
        None => {
            engine
                .create_from_conversation(owner_id, &actor, &conversation, request)
                .await?
        }
    };

    Ok(receipt(
        created.execution_id,
        created.status,
        "Delegated work was accepted and the host is planning it. End this turn; \
         inspect progress only if the user asks.",
    ))
}

fn receipt(
    execution_id: impl Into<String>,
    status: AgentExecutionStatus,
    message: impl Into<String>,
) -> AgentExecutionReceipt {
    AgentExecutionReceipt::new(execution_id, status, message)
}

/// The leader conversation's frozen workspace, if it has one.
fn conversation_work_dir(extra: &serde_json::Value) -> Option<String> {
    extra
        .get("workspace")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// The conversation's execution model authority: its declared pool, else its own
/// model, else "let the resolver decide".
fn conversation_model_pool(
    declared: Option<&ExecutionModelPool>,
    model: Option<&ProviderWithModel>,
) -> ExecutionModelPool {
    if let Some(pool) = declared {
        return pool.clone();
    }
    match lead_model(model) {
        Some(model) => ExecutionModelPool::Single { model },
        None => ExecutionModelPool::Automatic,
    }
}

/// The leader's model, as the preferred lead for the new execution.
fn lead_model(model: Option<&ProviderWithModel>) -> Option<ExecutionModelRef> {
    model.map(|model| ExecutionModelRef {
        provider_id: model.provider_id.clone(),
        model: model
            .use_model
            .clone()
            .unwrap_or_else(|| model.model.clone()),
    })
}

/// Model authority of an existing execution, taken from its live participants.
fn execution_model_pool(detail: &AgentExecutionDetail) -> Result<ExecutionModelPool, AppError> {
    let mut models = Vec::new();
    for participant in detail
        .participants
        .iter()
        .filter(|participant| participant.retired_in_revision.is_none())
    {
        if let (Some(provider_id), Some(model)) =
            (participant.provider_id.as_ref(), participant.model.as_ref())
        {
            let reference = ExecutionModelRef {
                provider_id: provider_id.clone(),
                model: model.clone(),
            };
            if !models.contains(&reference) {
                models.push(reference);
            }
        }
    }
    if models.is_empty() {
        return Err(AppError::Conflict(
            "execution has no active model authority".to_owned(),
        ));
    }
    Ok(ExecutionModelPool::Range { models })
}

/// Validate a conversation id exactly as the engine will, so a bad binding is a
/// clear refusal instead of a confusing engine error.
pub fn validate_conversation_id(conversation_id: &str) -> Result<(), AppError> {
    ConversationId::parse(conversation_id)
        .map(|_| ())
        .map_err(|error| AppError::BadRequest(format!("invalid conversation_id: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(provider_id: &str, model: &str, use_model: Option<&str>) -> ProviderWithModel {
        ProviderWithModel {
            provider_id: provider_id.to_owned(),
            model: model.to_owned(),
            use_model: use_model.map(str::to_owned),
        }
    }

    #[test]
    fn workspace_comes_from_the_frozen_extra() {
        assert_eq!(
            conversation_work_dir(&serde_json::json!({ "workspace": " /w " })),
            Some("/w".to_owned())
        );
        assert_eq!(conversation_work_dir(&serde_json::json!({})), None);
        assert_eq!(
            conversation_work_dir(&serde_json::json!({ "workspace": "   " })),
            None,
            "a blank workspace must not become a work dir"
        );
        assert_eq!(
            conversation_work_dir(&serde_json::json!({ "workspace": 7 })),
            None,
            "a non-string workspace is ignored"
        );
    }

    #[test]
    fn a_declared_pool_wins_even_when_it_is_automatic() {
        let declared = ExecutionModelPool::Automatic;
        assert_eq!(
            conversation_model_pool(Some(&declared), Some(&model("p", "m", None))),
            ExecutionModelPool::Automatic,
            "an explicit pool must not be replaced by the conversation's own model"
        );
    }

    #[test]
    fn without_a_declared_pool_the_conversation_model_becomes_the_lead() {
        let single =
            conversation_model_pool(None, Some(&model("0190f5fe-7c00-7a00-8000-0000000000b1", "m1", None)));
        assert_eq!(
            single,
            ExecutionModelPool::Single {
                model: ExecutionModelRef {
                    provider_id: "0190f5fe-7c00-7a00-8000-0000000000b1".to_owned(),
                    model: "m1".to_owned(),
                }
            }
        );
        assert_eq!(
            conversation_model_pool(None, None),
            ExecutionModelPool::Automatic,
            "no declared pool and no model leaves routing to the resolver"
        );
    }

    #[test]
    fn use_model_overrides_the_conversation_model_for_the_lead() {
        assert_eq!(
            lead_model(Some(&model("p", "m1", Some("m2")))).unwrap().model,
            "m2"
        );
        assert_eq!(lead_model(Some(&model("p", "m1", None))).unwrap().model, "m1");
        assert!(lead_model(None).is_none());
    }

    #[test]
    fn conversation_id_validation_is_explicit() {
        assert!(validate_conversation_id("0190f5fe-7c00-7a00-8000-0000000000a1").is_ok());
        assert!(validate_conversation_id("not-an-id").is_err());
    }
}

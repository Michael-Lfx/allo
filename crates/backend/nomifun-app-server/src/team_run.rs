//! `team/run`: one Team Definition → one Leader Conversation → one durable
//! execution (`docs/agent-store/16` §7 决策 3).
//!
//! A Team Run is **not** an `agent/run` with more members. The trigger the
//! decision fixed is: the server creates a Leader Conversation bound to the
//! Team's `AgentExecutionTemplate`, and the Leader model calls
//! `nomi_delegate(strategy="planned", goal=…)` inside its own turn. Everything
//! after that belongs to the Agent Execution engine.
//!
//! So this module does four things, in order, and nothing else:
//!
//! 1. **Materialize** the Team's template from its member AgentDefinitions (or
//!    reuse the one already materialized for this Team). Members, their model
//!    authority and the concurrency ceiling come from the Definition and the
//!    host — never from the request. The request has no `planning` block at all:
//!    `deny_unknown_fields` turns any attempt to send one into `invalid_request`.
//! 2. **Create** the Leader Conversation through the trusted Team seam, which
//!    writes the same explicit fences as a single-Agent Store chat (Skills and
//!    Connectors by id, never "all of them") plus the delegation tier.
//! 3. **Drive one turn**: send the goal to the Leader and wait for the turn to
//!    settle. The Leader's job in that turn is to call the delegate tool once and
//!    end the turn.
//! 4. **Reverse-map** the aggregate the Leader created through the Conversation's
//!    `lead` execution link, and return the public run receipt.
//!
//! Step 4 is why the receipt exists at all: the internal execution id is never
//! handed out, so the only honest way to answer `team/run` is to read back the
//! binding the server itself wrote.

use axum::http::StatusCode;
use nomifun_agent_execution::TeamRunReceipt;
use nomifun_api_types::{
    AgentExecutionTemplateParticipantInput, AppServerAgentDetail, AppServerTeamDetail,
    ModelPreference, PresetOverrides, ResolvedPresetSnapshot,
};
use nomifun_common::{McpServerId, ProviderWithModel, UserId, generate_id};
use nomifun_preset::PresetService;
use serde::{Deserialize, Serialize};

use crate::{
    AppServerError, AppServerRouterState, ConversationSendRequest, TeamCatalogProvider,
    agent_catalog_provider, conversation_service, map_public_run_id, resolve_app_server_model,
    resolved_chat_workspace, send_conversation_message_for_user, team_catalog_provider,
};

/// Public `team/run` request (`docs/agent-store/05` §5.2).
///
/// There is deliberately no `planning` / `members` / `max_parallel` field. The
/// `05` sketch used to show a `planning` block; `16` §7 决策 3 revoked it —
/// member pool, concurrency ceiling, routing constraints and authority come from
/// the bound Team template and server policy, never from the model or the client.
/// `deny_unknown_fields` is what makes that revocation *enforced* rather than
/// documented: a client that still sends `planning` gets `invalid_request`
/// instead of having it silently ignored.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamRunRequest {
    /// Public TeamDefinition id (`team/<get>`).
    pub team_id: String,
    /// Optional plugin version to pin. When present it must match the catalogued
    /// version exactly; a mismatch is refused rather than silently resolved to
    /// whatever is installed now (TC-RT-002's version-freeze semantics).
    #[serde(default)]
    pub team_version: Option<String>,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub input: Option<serde_json::Value>,
    #[serde(default)]
    pub workspace: Option<crate::WorkspaceRef>,
    #[serde(default)]
    pub command_id: Option<String>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

impl TeamRunRequest {
    fn normalized_goal(&self) -> Result<String, AppServerError> {
        if !self.goal.trim().is_empty() {
            return Ok(self.goal.trim().to_owned());
        }
        let Some(input) = self.input.as_ref() else {
            return Err(invalid("goal or input.text is required"));
        };
        let text = input
            .get("text")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .ok_or_else(|| invalid("input.text is required when goal is omitted"))?;
        Ok(text.to_owned())
    }
}

fn invalid(message: impl Into<String>) -> AppServerError {
    AppServerError::new("invalid_request", message, StatusCode::BAD_REQUEST, false)
}

/// `context` key that ties a materialized template back to the Team it came from.
///
/// Namespaced on purpose: the template `context` is also fed to the Planner as
/// supplemental goal context, so it must not look like one of the retired
/// conversation `extra` execution-policy keys.
const TEMPLATE_CONTEXT_TEAM_KEY: &str = "agent_store_team_id";

/// The Team a template was materialized for, if it was materialized by us.
fn template_team_id(template: &nomifun_api_types::AgentExecutionTemplate) -> Option<&str> {
    template
        .context
        .as_ref()
        .and_then(|context| context.get(TEMPLATE_CONTEXT_TEAM_KEY))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// The Team Definition's declared concurrency ceiling.
///
/// Read from `workflow_limits.max_parallel` and validated as a positive integer.
/// Anything else (absent, wrong type, `0`, negative) means "the Definition did
/// not declare one" and stays `None` — the engine's own conservative default then
/// applies. Guessing a ceiling from free-form JSON is how a Team silently gains
/// parallelism it never asked for.
fn declared_max_parallel(team: &AppServerTeamDetail) -> Option<i64> {
    team.workflow_limits
        .get("max_parallel")
        .and_then(serde_json::Value::as_i64)
        .filter(|value| *value > 0)
}

/// The participant-level fallback model for a resolved Leader model.
///
/// `use_model` is the effective request model when set (the same rule
/// `agent/run` uses when it builds a preset `ModelPreference`), so the fallback
/// names the model that will actually be called, not the catalog alias.
fn provider_model_preference(model: &ProviderWithModel) -> ModelPreference {
    ModelPreference {
        provider_id: Some(model.provider_id.clone()),
        model: model
            .use_model
            .clone()
            .unwrap_or_else(|| model.model.clone()),
        required: true,
    }
}

/// One participant input built from an AgentDefinition.
///
/// The model authority rule mirrors `agent/run`'s fallback: an Agent Store preset
/// is created **without** a model binding, so most participants would fail the
/// engine's "must resolve a concrete provider and model" check. Resolving the
/// preset once here lets us use the preset's own model when it has one and the
/// host's default only when it does not — never the other way round.
async fn participant_input(
    presets: &PresetService,
    role: &str,
    definition: &AppServerAgentDetail,
    sort_order: i64,
    fallback: Option<&ModelPreference>,
) -> Result<AgentExecutionTemplateParticipantInput, AppServerError> {
    let Some(preset_id) = definition
        .summary
        .preset_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Err(AppServerError::new(
            "agent_not_installed",
            format!(
                "team member {} is not installed; run install/* before starting the team",
                definition.summary.id
            ),
            StatusCode::BAD_REQUEST,
            false,
        ));
    };
    let snapshot: ResolvedPresetSnapshot = presets
        .resolve(preset_id, nomifun_api_types::PresetTarget::ExecutionStep, None, PresetOverrides::default())
        .await
        .map_err(AppServerError::from)?;
    let resolved = snapshot
        .resolved_model
        .as_ref()
        .and_then(|model| {
            model
                .provider_id
                .as_ref()
                .map(|provider_id| (provider_id.clone(), model.model.clone()))
        })
        .or_else(|| {
            fallback.and_then(|model| {
                model
                    .provider_id
                    .clone()
                    .map(|provider_id| (provider_id, model.model.clone()))
            })
        });
    let Some((provider_id, model)) = resolved else {
        return Err(AppServerError::new(
            "team_member_model_unbound",
            format!(
                "team member {} has no model: bind one on its preset or register an enabled provider",
                definition.summary.id
            ),
            StatusCode::BAD_REQUEST,
            false,
        ));
    };
    Ok(AgentExecutionTemplateParticipantInput {
        source_agent_id: None,
        preset_id: Some(preset_id.to_owned()),
        preset_snapshot: Some(snapshot),
        preset_overrides: None,
        provider_id: Some(provider_id),
        model: Some(model),
        // The Definition's own name is the only role label it actually carries.
        role: Some(role.to_owned()),
        // `capability` / `constraints` stay unset: the Team Definition expresses
        // routing as free-form strings (`routing_constraints`) and a tool-policy
        // *summary*, neither of which maps onto the engine's structured
        // capability/constraint types without inventing semantics. The raw strings
        // travel in the template `context` instead, where the Planner sees them.
        capability: None,
        constraints: None,
        description: definition.summary.description.clone(),
        system_prompt: None,
        enabled_skills: definition.summary.skills.clone(),
        disabled_builtin_skills: Vec::new(),
        sort_order: Some(sort_order),
    })
}

/// How many templates `ensure_team_template` scans looking for this Team's
/// previously materialized one.
///
/// Bounded on purpose: this runs once per Team Run, and a host with hundreds of
/// hand-authored templates must not turn a run start into an unbounded read. A
/// Team whose template has aged out of the window simply gets a fresh one.
const TEAM_TEMPLATE_LOOKUP_LIMIT: i64 = 200;

/// Materialize (or reuse) the `AgentExecutionTemplate` for one Team Definition.
///
/// Reuse is by `context[TEMPLATE_CONTEXT_TEAM_KEY]`, so a Team started twice does
/// not leak a template row per run. The template is intentionally **not** rewritten
/// when the Definition changes: templates are user-editable authoring data, and
/// silently overwriting one a human may have tuned would discard their edits. A
/// changed Definition therefore keeps using the existing template until the user
/// deletes it — which is the same contract the desktop template management surface
/// has.
async fn ensure_team_template(
    state: &AppServerRouterState,
    owner_id: &str,
    team: &AppServerTeamDetail,
    definitions: &[(String, AppServerAgentDetail)],
    fallback: Option<&ModelPreference>,
) -> Result<String, AppServerError> {
    let engine = require_engine(state)?;
    let existing = engine
        .list_templates(owner_id, Some(TEAM_TEMPLATE_LOOKUP_LIMIT), Some(0))
        .await
        .map_err(AppServerError::from)?
        .into_iter()
        .find(|template| template_team_id(template) == Some(team.summary.id.as_str()));
    if let Some(template) = existing {
        return Ok(template.execution_template_id);
    }

    let presets = state.preset_service.as_ref().ok_or_else(|| {
        AppServerError::new(
            "unsupported_operation",
            "App Server Preset service is unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            false,
        )
    })?;
    let mut participants = Vec::with_capacity(definitions.len());
    for (index, (role, definition)) in definitions.iter().enumerate() {
        participants.push(
            participant_input(
                presets,
                role,
                definition,
                index as i64,
                fallback,
            )
            .await?,
        );
    }
    let created = engine
        .create_template(
            owner_id,
            nomifun_api_types::CreateAgentExecutionTemplateRequest {
                name: format!("agent-store team: {}", team.summary.name),
                description: team.summary.description.clone(),
                max_parallel: declared_max_parallel(team),
                work_dir: None,
                context: Some(serde_json::json!({
                    TEMPLATE_CONTEXT_TEAM_KEY: team.summary.id,
                    "team_version": team.summary.version,
                    "planner_policy": team.planner_policy,
                    "routing_constraints": team.routing_constraints,
                    "team_runtime_capabilities": team.team_runtime_capabilities,
                })),
                participants,
            },
        )
        .await
        .map_err(AppServerError::from)?;
    Ok(created.template.execution_template_id)
}

fn require_engine(
    state: &AppServerRouterState,
) -> Result<std::sync::Arc<nomifun_agent_execution::AgentExecutionEngine>, AppServerError> {
    state.engine.clone().ok_or_else(|| {
        AppServerError::new(
            "runtime_unavailable",
            "App Server runtime is unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            true,
        )
    })
}

/// Resolve the Team and its member AgentDefinitions, Leader first.
async fn resolve_team_members(
    state: &AppServerRouterState,
    request: &TeamRunRequest,
) -> Result<(AppServerTeamDetail, Vec<(String, AppServerAgentDetail)>), AppServerError> {
    let teams: std::sync::Arc<dyn TeamCatalogProvider> = team_catalog_provider(state)?;
    let team = teams.get(&request.team_id).await.map_err(AppServerError::from)?;
    if let Some(requested) = request.team_version.as_deref().map(str::trim)
        && !requested.is_empty()
        && requested != team.summary.version
    {
        return Err(AppServerError::new(
            "version_mismatch",
            format!(
                "team {} is installed at version {}, not {requested}",
                team.summary.id, team.summary.version
            ),
            StatusCode::BAD_REQUEST,
            false,
        ));
    }

    let agents = agent_catalog_provider(state)?;
    let lead = agents
        .get(&team.summary.lead_agent_id)
        .await
        .map_err(AppServerError::from)?;
    let mut members: Vec<(String, AppServerAgentDetail)> = Vec::new();
    let mut seen: Vec<String> = vec![lead.summary.id.clone()];
    members.push((lead.summary.name.clone(), lead));
    for member_id in &team.summary.member_agent_ids {
        // A Team that lists the same Agent twice must not spend two participants
        // on it: the engine's concurrency accounting is per participant.
        if seen.iter().any(|id| id == member_id) {
            continue;
        }
        let member = agents
            .get(member_id)
            .await
            .map_err(AppServerError::from)?;
        seen.push(member.summary.id.clone());
        members.push((member.summary.name.clone(), member));
    }
    Ok((team, members))
}

/// Validate the Team's snapshot-installed Connectors and return them as
/// canonical ids for the Leader's Connector fence.
///
/// The catalog returns ids as strings; this is the boundary that turns them into
/// typed `McpServerId`s (so a malformed id can never reach the fence) and refuses
/// a disabled Connector instead of silently binding nothing — a pinned dependency
/// that is missing must be visible, exactly as `agent/run` treats preset
/// `mcp_server_ids`.
async fn team_connector_fence(
    state: &AppServerRouterState,
    team: &AppServerTeamDetail,
) -> Result<Vec<McpServerId>, AppServerError> {
    if team.connectors.is_empty() {
        return Ok(Vec::new());
    }
    let connectors = crate::connector_catalog_provider(state)?;
    let mut fence = Vec::with_capacity(team.connectors.len());
    for raw in &team.connectors {
        let id = McpServerId::parse(raw).map_err(|error| {
            AppServerError::new(
                "internal_error",
                format!("team {} declares an invalid connector id: {error}", team.summary.id),
                StatusCode::INTERNAL_SERVER_ERROR,
                false,
            )
        })?;
        let detail = connectors
            .get(id.as_str())
            .await
            .map_err(AppServerError::from)?;
        if !detail.summary.enabled {
            return Err(AppServerError::new(
                "connector_unavailable",
                format!("connector {raw} bound by the team is disabled"),
                StatusCode::BAD_REQUEST,
                false,
            ));
        }
        if !fence.contains(&id) {
            fence.push(id);
        }
    }
    Ok(fence)
}

/// Execute one `team/run`, from Definition to public run receipt.
///
/// Idempotency is handled by the caller (it owns the connection context), so this
/// function is the pure "do it once" body — the same split `execute_agent_run`
/// uses.
pub(crate) async fn execute_team_run(
    state: &AppServerRouterState,
    user: &nomifun_auth::CurrentUser,
    request: TeamRunRequest,
) -> Result<TeamRunReceipt, AppServerError> {
    let owner = UserId::parse(user.id.as_str())
        .map_err(|error| invalid(format!("invalid owner: {error}")))?;
    let owner_id = owner.as_str();
    let goal = request.normalized_goal()?;
    require_engine(state)?;
    let runtime = state.runtime.as_ref().ok_or_else(|| {
        AppServerError::new(
            "runtime_unavailable",
            "App Server runtime is unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            true,
        )
    })?;

    let (team, members) = resolve_team_members(state, &request).await?;
    let connector_ids = team_connector_fence(state, &team).await?;

    // Resolve the Leader's model **before** materializing the template, and use it
    // as the participants' fallback. One resolution means the Leader Conversation
    // and every model-less member preset agree by construction instead of by
    // coincidence — two independent "host default" lookups could disagree.
    let workspace = resolved_chat_workspace(state, user, request.workspace.as_ref()).await?;
    let model: ProviderWithModel = resolve_app_server_model(state, None).await?;
    let leader_model = provider_model_preference(&model);
    let template_id = ensure_team_template(state, owner_id, &team, &members, Some(&leader_model)).await?;

    // The Leader's own Skills are the Lead AgentDefinition's; each member's Skills
    // travel with that member's template participant instead.
    let leader_skills = members
        .first()
        .map(|(_, lead)| lead.summary.skills.clone())
        .unwrap_or_default();

    let leader = conversation_service(state)?
        .create_app_server_team_leader_chat(
            owner_id,
            Some(format!("{} (leader)", team.summary.name)),
            model,
            workspace.path().to_string_lossy().into_owned(),
            Some(workspace.workspace_id().to_owned()),
            None,
            nomifun_conversation::AppServerTeamLeaderBindings {
                connector_ids,
                skill_names: leader_skills,
                execution_template_id: template_id,
                // The Team tier. Server-computed, never from the request: a Store
                // client can ask for a Team Run, not for a delegation policy.
                delegation_policy: nomifun_common::DelegationPolicy::Automatic,
            },
        )
        .await
        .map_err(AppServerError::from)?;
    let leader_id = leader.conversation_id.clone();

    // One turn. The Leader's contract for this turn is "call the delegate tool once
    // and end the turn", so waiting for it does not mean waiting for the Team: the
    // planning and the member work all happen inside the engine, after the tool
    // returns. A failure here is reported *after* the link lookup below, because a
    // turn that failed after the tool call still produced a real execution.
    let delivery = send_conversation_message_for_user(
        state,
        user,
        &leader_id,
        ConversationSendRequest {
            content: goal,
            idempotency_key: generate_id(),
            attachments: Vec::new(),
        },
    )
    .await;

    // Reverse-map: the server wrote the `lead` link, so this is the only
    // trustworthy answer to "which run did that produce?".
    let execution_id = require_engine(state)?
        .execution_for_lead_conversation(owner_id, &leader_id)
        .await
        .map_err(AppServerError::from)?;
    let Some(execution_id) = execution_id else {
        return Err(match delivery {
            Ok(receipt) => AppServerError::new(
                "team_run_not_started",
                format!(
                    "the team leader finished its turn without starting a run (leader conversation {leader_id}{})",
                    receipt
                        .result_error
                        .as_deref()
                        .map(|error| format!(": {error}"))
                        .unwrap_or_default()
                ),
                StatusCode::CONFLICT,
                false,
            ),
            Err(error) => error,
        });
    };

    let view = runtime
        .get_run(owner_id, &execution_id)
        .await
        .map_err(nomifun_agent_execution::AgentRuntimeAdapter::map_error)
        .map_err(AppServerError::from)?;
    let public_run_id = match map_public_run_id(state, owner_id, &execution_id).await {
        Ok(public_run_id) => public_run_id,
        Err(error) => {
            // An execution without a public mapping is not reachable through this
            // protocol, and the Leader already ended its turn — so nothing else
            // would ever collect it. Cancel best-effort before surfacing the
            // mapping failure, exactly as `execute_agent_run` does, so a retry
            // cannot leave orphaned active work behind.
            let _ = runtime
                .cancel_run(owner_id, &execution_id, view.version)
                .await;
            return Err(error);
        }
    };
    Ok(TeamRunReceipt {
        run_id: public_run_id,
        status: view.status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn team(workflow_limits: serde_json::Value) -> AppServerTeamDetail {
        AppServerTeamDetail {
            summary: nomifun_api_types::AppServerTeamSummary {
                id: "wb-demo-software-company".into(),
                version: "1.0.0".into(),
                name: "Software Company".into(),
                description: None,
                lead_agent_id: "wb-demo-lead".into(),
                member_agent_ids: vec!["wb-demo-dev".into()],
                source: "imported".into(),
                compatibility_status: nomifun_api_types::AppServerCompatibilityStatus::Compatible,
            },
            planner_policy: "planned".into(),
            routing_constraints: vec!["prefer_frontend".into()],
            workflow_limits,
            team_runtime_capabilities: vec!["planned_dag".into()],
            connectors: vec![],
        }
    }

    #[test]
    fn the_declared_ceiling_is_read_only_when_it_is_a_positive_integer() {
        assert_eq!(declared_max_parallel(&team(serde_json::json!({"max_parallel": 4}))), Some(4));
        assert_eq!(declared_max_parallel(&team(serde_json::json!({}))), None);
        assert_eq!(
            declared_max_parallel(&team(serde_json::json!({"max_parallel": 0}))),
            None,
            "zero means 'not declared', not 'no parallelism'"
        );
        assert_eq!(
            declared_max_parallel(&team(serde_json::json!({"max_parallel": -2}))),
            None
        );
        assert_eq!(
            declared_max_parallel(&team(serde_json::json!({"max_parallel": "4"}))),
            None,
            "a string ceiling is not a ceiling"
        );
    }

    #[test]
    fn template_provenance_is_read_from_our_own_context_key() {
        let mut template = nomifun_api_types::AgentExecutionTemplate {
            execution_template_id: nomifun_common::AgentExecutionTemplateId::new().into_string(),
            name: "t".into(),
            description: None,
            max_parallel: None,
            work_dir: None,
            context: Some(serde_json::json!({ "agent_store_team_id": "wb-demo" })),
            version: 0,
            created_at: 1,
            updated_at: 1,
        };
        assert_eq!(template_team_id(&template), Some("wb-demo"));

        template.context = Some(serde_json::json!({ "agent_store_team_id": "  " }));
        assert_eq!(
            template_team_id(&template),
            None,
            "a blank id must not match every team with a blank id"
        );
        template.context = Some(serde_json::json!({ "team_id": "wb-demo" }));
        assert_eq!(
            template_team_id(&template),
            None,
            "a user-authored `team_id` is not our provenance marker"
        );
        template.context = None;
        assert_eq!(template_team_id(&template), None);
    }

    #[test]
    fn a_team_run_request_has_no_planning_surface() {
        // `16` §7 决策 3 revoked the model/client-settable planning block. The
        // enforcement is `deny_unknown_fields`, so prove the field really is
        // rejected rather than quietly dropped.
        let parsed = serde_json::from_value::<TeamRunRequest>(serde_json::json!({
            "team_id": "wb-demo",
            "goal": "ship it",
            "planning": { "mode": "planned", "max_parallel": 8 }
        }));
        assert!(parsed.is_err(), "a planning block must be refused, not ignored");

        let parsed = serde_json::from_value::<TeamRunRequest>(serde_json::json!({
            "team_id": "wb-demo",
            "goal": "ship it",
            "members": ["wb-evil"]
        }));
        assert!(parsed.is_err(), "the member pool is never client input");

        let ok = serde_json::from_value::<TeamRunRequest>(serde_json::json!({
            "team_id": "wb-demo",
            "goal": "ship it"
        }))
        .expect("the minimal request is valid");
        assert_eq!(ok.normalized_goal().unwrap(), "ship it");
    }

    #[test]
    fn the_participant_fallback_names_the_effective_model() {
        let alias = ProviderWithModel {
            provider_id: "0190f5fe-7c00-7a00-8000-0000000000c1".to_owned(),
            model: "catalog-alias".to_owned(),
            use_model: Some("wire-model".to_owned()),
        };
        let preference = provider_model_preference(&alias);
        assert_eq!(preference.provider_id.as_deref(), Some(alias.provider_id.as_str()));
        assert_eq!(
            preference.model, "wire-model",
            "the fallback must name the model that will actually be called"
        );
        assert!(preference.required);

        let plain = provider_model_preference(&ProviderWithModel {
            provider_id: alias.provider_id.clone(),
            model: "only-model".to_owned(),
            use_model: None,
        });
        assert_eq!(plain.model, "only-model");
    }

    #[test]
    fn goal_falls_back_to_input_text() {
        let request = serde_json::from_value::<TeamRunRequest>(serde_json::json!({
            "team_id": "wb-demo",
            "input": { "text": "  build the thing  " }
        }))
        .unwrap();
        assert_eq!(request.normalized_goal().unwrap(), "build the thing");

        let empty = serde_json::from_value::<TeamRunRequest>(serde_json::json!({
            "team_id": "wb-demo"
        }))
        .unwrap();
        assert_eq!(empty.normalized_goal().unwrap_err().code, "invalid_request");
    }
}

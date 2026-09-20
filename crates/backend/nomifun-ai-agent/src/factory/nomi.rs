use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use nomi_agent::session::{Session, SessionManager};
use nomi_config::config::{CliArgs, Config, McpServerConfig, TransportType};
use nomifun_api_types::{
    GatewayMcpConfig, HealthStatus, McpServerId, McpTransport, ModelHealthStatus, ModelTask,
    ModelTrait, NomiBuildExtra, NomiMcpDeclarations, NomiToolPolicy, SessionMcpServer,
    SessionMcpTransport,
};
use nomifun_common::{
    AppError, DelegationPolicy, ExecutionAuthority, LoopbackCapabilityLease,
    LoopbackCapabilityLeaseSet, ProviderId, ProviderWithModel, normalize_ui_language,
    read_installation_language,
};
use nomifun_db::IMcpServerRepository;
use nomifun_db::models::McpServerRow;
use nomifun_mcp::McpOAuthService;
use nomifun_runtime::resolve_command_path;
use tracing::{debug, info, warn};

use crate::factory::mcp_oauth::{NomiMcpOAuthRefresher, inject_oauth_bearer};
use crate::runtime_handle::AgentRuntimeHandle;
use crate::factory::AgentFactoryDeps;
use crate::factory::context::FactoryContext;
use crate::factory::platform_table;
use crate::manager::nomi::{
    NomiAgentManager, NomiHostWiring, NomiSummonWiring, sanitize_session_messages,
};
use crate::types::{
    AgentRuntimeBuildOptions, ImageAnalysisModelConfig, NomiCompatOverrides, NomiResolvedConfig,
};

/// Apply the complete ceiling for an authenticated principal that does not own
/// this installation.  This is model-only execution: no OS tools, configured
/// MCP, platform domains, knowledge mounts, autonomous goal loop or Agent
/// delegation.  The non-empty allowlist is intentional because an empty
/// `retain_named` list means "keep everything".
/// Marker written only by the trusted ConversationService App Server creation
/// seam. It is not a client-facing capability grant: factory code uses it only
/// to subtract integrations from an already-authorized Nomi runtime.
const APP_SERVER_CHAT_EXTRA_KEY: &str = "app_server_chat";

fn apply_model_only_ceiling(overrides: &mut NomiBuildExtra) {
    overrides.computer_use = Some(false);
    overrides.browser_use = Some(false);
    overrides.gateway_mcp_config = None;
    overrides.mcp_server_ids = None;
    overrides.session_mcp_servers.clear();
    overrides.companion = false;
    overrides.companion_id = None;
    overrides.channel_platform = None;
    overrides.knowledge_mounts.clear();
    overrides.knowledge_writeback = false;
    overrides.knowledge_channel_write_enabled = false;
    overrides.allowed_tools = vec!["update_plan".to_owned()];
    overrides.session_mode = Some("default".to_owned());
    overrides.max_turns = Some(1);
    overrides.goal = None;
    overrides.moa = None;
    overrides.delegation_policy = DelegationPolicy::Disabled;
    // Summon loads local companion memories/skills — installation-owner only.
    overrides.summon = None;
}

/// App Server chat is an owner-visible Nomi session, but intentionally not an
/// integration host. Keep ordinary local Agent tools available while removing
/// every dynamic Team/Skill/MCP path before any process-owned gateway or
/// repository-backed server configuration is considered.
///
/// This ceiling deliberately does **not** touch `delegation_policy`. Delegation is
/// a first-class typed Conversation field, and the tier a Store chat runs on is
/// decided by *which trusted seam created it*: `create_app_server_nomi_chat`
/// writes `Disabled` (a single Agent Run gets no `nomi_delegate`), while
/// `create_app_server_team_leader_chat` writes the Team tier. Clamping it here
/// would make every Store chat structurally unable to delegate — including the
/// Team Leader, whose entire purpose is to call `nomi_delegate(strategy=planned)`
/// (`16` §7 决策 3). Forging the `app_server_chat` marker cannot widen anything
/// either: the marker only ever subtracts, and a Conversation *without* it keeps
/// whatever policy its own row carries.
fn apply_app_server_chat_ceiling(overrides: &mut NomiBuildExtra) {
    overrides.gateway_mcp_config = None;
    // Connectors are fenced by **explicit id**, never by an absent key: `None`
    // means "every enabled MCP server on this host" to `load_user_mcp_servers`.
    // The trusted create seam always writes `extra.mcp_server_ids` (possibly
    // empty), so this only backstops a legacy/malformed row — and it must not
    // wipe the fence when the Definition did bind Connectors.
    if overrides.mcp_server_ids.is_none() {
        overrides.mcp_server_ids = Some(Vec::new());
    }
    overrides.session_mcp_servers.clear();
    overrides.summon = None;
}

/// Apply the host's global tool policy on top of whatever the session asked for.
///
/// This is the *only* place the policy subtracts from a session, and it runs
/// unconditionally: every field can only narrow, so it composes safely with both
/// the App Server ceiling above and the secondary-principal model-only ceiling
/// below (order between them cannot matter).
///
/// Deliberately split from `[tools]`-driven switches the *engine* owns:
/// `web` / `plan` / `lsp` are read from `Config::resolve` in the manager, so
/// they are applied there, not here (see `NomiResolvedConfig::tool_policy`).
fn apply_host_tool_policy(overrides: &mut NomiBuildExtra, policy: &NomiToolPolicy) {
    if !policy.computer {
        overrides.computer_use = Some(false);
    }
    if !policy.browser {
        overrides.browser_use = Some(false);
    }
    // Companion sessions own memory/skill tools and a persona prompt; without
    // the domain there is no companion binding to host.
    if !policy.domains.companion {
        overrides.companion = false;
        overrides.companion_id = None;
        overrides.summon = None;
    }
    // Knowledge mounts drive both the retrieval tools and the system-prompt
    // section, so clearing the mounts is what actually removes the surface
    // (a `None` sink alone would leave `knowledge_search` visible).
    if !policy.domains.knowledge {
        overrides.knowledge_mounts.clear();
        overrides.knowledge_writeback = false;
        overrides.knowledge_channel_write_enabled = false;
    }
    // Goals come from conversation config/DB; both the fresh spec and the
    // restore snapshot must go, or `update_goal` stays registered.
    if !policy.domains.goal {
        overrides.goal = None;
    }
}

fn retarget_resumed_session(session: &mut Session, provider: &str, model: &str) -> bool {
    let changed = session.provider != provider || session.model != model;
    session.provider = provider.to_owned();
    session.model = model.to_owned();
    changed
}

fn persist_repaired_session(manager: &SessionManager, session: &Session) -> Result<(), String> {
    manager.save(session).map_err(|error| error.to_string())?;
    manager
        .update_index_for(session)
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// The Flowy cloud catalog is authoritative when it provides a model output
/// ceiling. All other providers (and uncataloged Flowy models) retain the

/// Keep a user-selected effort only when the active model advertises it.
/// When the catalog lists levels but the session has no (valid) selection,
/// default to `medium` (or the first advertised level) so reasoning models
/// still receive an explicit `reasoning_effort` on the wire.
fn resolve_session_reasoning_effort(
    selected: Option<&str>,
    effort_levels: Option<&[String]>,
) -> Option<String> {
    let levels = effort_levels.filter(|levels| !levels.is_empty())?;
    if let Some(selected) = selected.map(str::trim).filter(|value| !value.is_empty()) {
        if let Some(matched) = levels.iter().find(|level| level.as_str() == selected) {
            return Some(matched.clone());
        }
    }
    levels
        .iter()
        .find(|level| level.as_str() == "medium")
        .cloned()
        .or_else(|| levels.first().cloned())
}

/// Sanitize a resumed transcript without losing an exact rewind boundary.
///
/// The sanitizer removes messages but never reorders or inserts them. Splitting
/// at the root-turn boundary therefore lets each side be repaired independently
/// and remaps `start_len` to the retained prefix length. No valid tool-call /
/// tool-result pair can cross this boundary because a root user message starts
/// the suffix.
fn sanitize_resumed_session(
    session: &mut Session,
    provider_changed: bool,
) -> crate::manager::nomi::history_sanitize::SessionRepairStats {
    let Some(start_len) = session
        .editable_turn
        .as_ref()
        .map(|checkpoint| checkpoint.start_len)
    else {
        return sanitize_session_messages(&mut session.messages, provider_changed);
    };
    if start_len > session.messages.len() {
        session.editable_turn = None;
        return sanitize_session_messages(&mut session.messages, provider_changed);
    }

    let mut suffix = session.messages.split_off(start_len);
    let mut stats = sanitize_session_messages(&mut session.messages, provider_changed);
    stats.merge(sanitize_session_messages(&mut suffix, provider_changed));
    if let Some(checkpoint) = session.editable_turn.as_mut() {
        checkpoint.start_len = session.messages.len();
    }
    session.messages.append(&mut suffix);
    stats
}

pub(super) async fn build(
    deps: Arc<AgentFactoryDeps>,
    options: AgentRuntimeBuildOptions,
    ctx: FactoryContext,
    authority: ExecutionAuthority,
) -> Result<AgentRuntimeHandle, AppError> {
    let is_app_server_chat = options
        .extra
        .get(APP_SERVER_CHAT_EXTRA_KEY)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let mut overrides: NomiBuildExtra = serde_json::from_value(options.extra)
        .map_err(|error| AppError::BadRequest(format!("Invalid Nomi build options: {error}")))?;
    overrides.user_id = Some(options.user_id.clone());
    // The first-class conversation field is authoritative. Never let an
    // open-ended extra payload override execution policy.
    overrides.delegation_policy = options.delegation_policy;
    let is_instance_owner = authority.controls_host();
    if is_app_server_chat {
        apply_app_server_chat_ceiling(&mut overrides);
    }
    // Host policy is applied after the session's own request so it can only ever
    // subtract from it, and before every capability-resolution site below so the
    // cleared fields actually change what gets wired.
    apply_host_tool_policy(&mut overrides, &deps.tool_policy);

    // Gateway entitlement is derived from the immutable principal, never from
    // persisted/open JSON. Process-owned config is injected only after the
    // ownership ceiling has been applied.
    overrides.gateway_mcp_config = None;

    // A non-owner runtime is deliberately model-only.  Hiding a few tools is
    // insufficient because every native shell/ACP process shares the backend's
    // OS uid; the single ceiling below is the enforceable boundary.
    if !is_instance_owner {
        apply_model_only_ceiling(&mut overrides);
    }

    // Merge reusable preset instructions into `system_prompt` (used as
    // `custom_prompt` in Nomi's prompt builder).
    if let Some(rules) = overrides.preset_rules.take() {
        overrides.system_prompt = Some(match overrides.system_prompt.take() {
            Some(existing) => format!("{existing}\n\n{rules}"),
            None => rules,
        });
    }

    // Companion-companion sessions without a persisted persona prompt (channel
    // Channel Agent sessions) get one built fresh per Agent build, so the
    // embedded memory snapshot stays current across restarts. `extra.companion_id`
    // picks the persona (per-bot binding > legacy platform binding); when no
    // companion is bound (None / dead id) there is no persona — an unbound channel
    // is hosted by no companion (no default-companion fallback).
    if overrides.companion
        && overrides.system_prompt.is_none()
        && let Some(provider) = deps.companion_prompt.as_ref()
        && let Some(prompt) = provider
            .build_system_prompt(
                overrides.companion_id.as_deref(),
                overrides.channel_platform.as_deref(),
            )
            .await
    {
        overrides.system_prompt = Some(prompt);
    }

    // In-session companion summon (spec §设计 B2/B3): only owner-authority,
    // non-companion work sessions. The persona is never taken
    // over — the system prompt gains exactly one loading notice; memories are
    // injected per turn by a ContextContributor and stay read-only.
    let summon_config = if is_instance_owner && !is_app_server_chat && !overrides.companion {
        overrides.summon.clone()
    } else {
        None
    };
    let mut summon_wiring: Option<NomiSummonWiring> = None;
    if let Some(provider) = deps.companion_summon.as_ref() {
        match summon_config.as_ref() {
            Some(summon) => {
                // Skill materialization (workspace is resolved by now). Manifest
                // ownership makes this idempotent and prunes stale entries when
                // exclusions change. Best-effort: a skill failure degrades the
                // session, it must not block chatting.
                match provider
                    .sync_summon_workspace_skills(
                        &ctx.conversation_id,
                        std::path::Path::new(&ctx.workspace),
                        &summon.companion_id,
                        &summon.skill_exclusions,
                    )
                    .await
                {
                    Ok(linked) => debug!(
                        conversation_id = %ctx.conversation_id,
                        skills = linked.len(),
                        "summon: companion skills materialized into workspace"
                    ),
                    Err(error) => warn!(
                        conversation_id = %ctx.conversation_id,
                        error = %error,
                        "summon: companion skill materialization failed; continuing without them"
                    ),
                }
                let name = provider.companion_name(&summon.companion_id).await;
                let notice = format!(
                    "本会话已装载伙伴「{}」的技能与所选记忆（只读）。伙伴人格不接管本会话。\
                     需要补查伙伴记忆用 recall_memories；发现长期有价值的新事实用 \
                     propose_companion_memory 提议（主人确认后才写入伙伴记忆），宁缺毋滥。",
                    name.as_deref().unwrap_or("（已不存在）")
                );
                overrides.system_prompt = Some(match overrides.system_prompt.take() {
                    Some(existing) if !existing.trim().is_empty() => {
                        format!("{existing}\n\n{notice}")
                    }
                    _ => notice,
                });
                match (
                    provider.summon_memory_sink(&summon.companion_id),
                    provider.summon_proposal_sink(&summon.companion_id),
                    provider.summon_context_sink(summon),
                ) {
                    (Ok(memory_sink), Ok(proposal_sink), Ok(context_sink)) => {
                        summon_wiring = Some(NomiSummonWiring {
                            memory_sink,
                            proposal_sink,
                            context_sink,
                        });
                    }
                    (memory, proposal, context) => {
                        warn!(
                            conversation_id = %ctx.conversation_id,
                            memory_sink_err = ?memory.err().map(|e| e.to_string()),
                            proposal_sink_err = ?proposal.err().map(|e| e.to_string()),
                            context_sink_err = ?context.err().map(|e| e.to_string()),
                            "summon: sink construction failed; session continues without summon tools"
                        );
                    }
                }
            }
            None if is_instance_owner && !is_app_server_chat && !overrides.companion => {
                // A cleared (or never-set) summon unloads its manifest-owned
                // skills on the next build. No-op without a manifest; companion
                // threads manage their own manifest and are excluded above.
                if let Err(error) = provider
                    .clear_summon_workspace_skills(
                        &ctx.conversation_id,
                        std::path::Path::new(&ctx.workspace),
                    )
                    .await
                {
                    warn!(
                        conversation_id = %ctx.conversation_id,
                        error = %error,
                        "summon: workspace skill cleanup failed"
                    );
                }
            }
            None => {}
        }
    }

    // A process-owned configuration object is the capability. There is no
    // serializable boolean grant that persisted or client JSON can forge.
    let platform_gateway_entitled =
        is_instance_owner && !is_app_server_chat && overrides.allowed_tools.is_empty();
    overrides.gateway_mcp_config = if platform_gateway_entitled {
        deps.gateway_mcp_config.clone()
    } else {
        None
    };
    if overrides.gateway_mcp_config.is_some() {
        info!(
            conversation_id = %ctx.conversation_id,
            gateway_mcp_port = deps.gateway_mcp_config.as_ref().map(|c| c.port()),
            "gateway_mcp: injected into owner nomi session"
        );
    }
    let has_platform_gateway = overrides.gateway_mcp_config.is_some();

    // Host composition is decided once per session: either the embedded
    // (synchronous, parallel-only) deployment or the host's durable facade owns the
    // `nomi_delegate` name — never both, since a second registration under the same
    // name is rejected as a duplicate route. Computed here (not just before the
    // registration below) because the *prompt* must describe whichever deployment
    // actually owns the name, and the prompt is assembled a few lines down.
    let install_embedded_agent_execution = should_install_embedded_agent_execution(
        has_platform_gateway,
        is_instance_owner,
        deps.embedded_agent_execution,
    );
    let delegate_deployment = delegate_deployment(
        has_platform_gateway,
        install_embedded_agent_execution,
        is_instance_owner
            && deps
                .delegate_sink_provider
                .as_ref()
                .and_then(|slot| slot.get())
                .is_some(),
    );

    let (mut extra_mcp_servers, loopback_capability_leases) =
        resolve_mcp_servers(&overrides, &ctx.conversation_id);
    // Host-declared servers load next, before the `mcp_servers` rows below, so a
    // declaration wins a name collision against an imported row (that loop is
    // first-writer-wins) while a request-level binding still outranks both.
    // Owner-gated exactly like those rows: a declaration is a host capability.
    merge_host_declared_mcp_servers(
        &mut extra_mcp_servers,
        &deps.mcp_declarations,
        &ctx.conversation_id,
        is_instance_owner,
    );
    // Connector rows load for App Server chats too, but strictly by the id fence
    // the ceiling above established: an App Server chat with no bound Connector
    // carries `Some(vec![])`, so this selects nothing. Session-scoped servers
    // (a desktop-request concept) stay owner-only and are cleared by the ceiling.
    if is_instance_owner && let Some(repo) = deps.mcp_server_repo.as_ref() {
        for (name, config) in load_user_mcp_servers(
            repo.as_ref(),
            overrides.mcp_server_ids.as_deref(),
            &ctx.conversation_id,
            deps.mcp_oauth_service.as_ref(),
        )
        .await
        {
            extra_mcp_servers.entry(name).or_insert(config);
        }
    }
    if is_instance_owner && !is_app_server_chat {
        let enabled_session_mcp_servers = super::filter_enabled_session_mcp_servers(
            deps.mcp_server_repo.as_deref(),
            &overrides.session_mcp_servers,
            &ctx.conversation_id,
        )
        .await;
        merge_session_snapshot_mcp_servers(
            &mut extra_mcp_servers,
            &enabled_session_mcp_servers,
            &ctx.conversation_id,
            deps.mcp_oauth_service.as_ref(),
        )
        .await;
    }

    // Per-surface write policy (spec §3.2 unit 5): companion → direct, external
    // IM channel → disabled (P1; opt-in re-enable is P2), regular chat → the
    // binding's staged|direct (staged default). Resolved here where the surface
    // is known from the build extra, reusing the shared rule so the gateway path
    // can't drift. Expressed downstream via existing signals: sink=None disables
    // the tool; the staged bool drives placement.
    let knowledge_write_surface = if overrides.companion {
        nomifun_knowledge::WriteSurface::Companion
    } else if overrides.channel_platform.is_some() {
        nomifun_knowledge::WriteSurface::ExternalChannel
    } else {
        nomifun_knowledge::WriteSurface::RegularChat
    };
    let knowledge_write_policy = nomifun_knowledge::resolve_write_policy(
        knowledge_write_surface,
        &nomifun_knowledge::KnowledgeBinding {
            enabled: true,
            writeback: overrides.knowledge_writeback,
            // Threaded from the binding via MountOutcome → build-extra so the
            // external-IM-channel opt-in actually reaches resolve_write_policy;
            // a `..Default::default()` here would pin it to `false` and keep
            // channel write-back permanently disabled on the nomi engine.
            channel_write_enabled: overrides.knowledge_channel_write_enabled,
            ..Default::default()
        },
    );
    let knowledge_write_enabled = !matches!(
        knowledge_write_policy.mode,
        nomifun_knowledge::WriteMode::Disabled
    );

    // Knowledge bases: append the mounted-bases section (per-base TOC +
    // write-back contract) to the system prompt, so nomi-engine sessions
    // (companion companion threads included) see the same knowledge context the
    // ACP path gets via its preset_context.
    overrides.system_prompt = append_knowledge_context(
        overrides.system_prompt.take(),
        &overrides,
        knowledge_write_enabled,
    );

    // 持久委派提示：**必须描述实际拥有 `nomi_delegate` 这个名字的那个部署**。
    // 同一个名字下有三种实现，能力完全不同（见 `DelegateDeployment`）：Gateway
    // 版有 planned + parallel + `nomi_execution_get`；宿主自有持久 facade 版
    // **只有 planned**（`host_delegate_tool.rs` 的 schema 直接拒绝其它字段）；
    // 嵌入式版只支持 parallel、不落库，且它自己用工具描述向模型表达。所以这里按
    // 部署挑提示，而不是按「有没有 gateway」挑——App Server（Store）会话永远没有
    // gateway，但仍可能有宿主 facade 版，Team 的 Leader 完全依赖它。该策略只影响
    // 提示，不授予工具能力或改变审批模式。
    let delegation_hint = match delegate_deployment {
        DelegateDeployment::Gateway => compose_delegation_hint(
            overrides.system_prompt.take(),
            should_inject_delegation_hint(
                has_platform_gateway,
                overrides.companion,
                overrides.channel_platform.is_some(),
            ),
            overrides.delegation_policy,
        ),
        DelegateDeployment::HostFacade => compose_host_delegation_hint(
            overrides.system_prompt.take(),
            should_inject_delegation_hint(
                true,
                overrides.companion,
                overrides.channel_platform.is_some(),
            ),
            overrides.delegation_policy,
        ),
        // Embedded describes itself; `None` has nothing to describe.
        DelegateDeployment::Embedded | DelegateDeployment::None => overrides.system_prompt.take(),
    };
    overrides.system_prompt = delegation_hint;

    // Every native Nomi session — regular desktop chat, companion, IM
    // Channel Agent — must think AND reply in the
    // app's UI language, not a hardcoded one. The persona prompt no longer forces
    // a language, so it is decided HERE from the live system setting and appended
    // LAST (so it wins over the English base prompt / any earlier persisted
    // language line, and the first turn follows the system language). Read live
    // per build → switching the language takes effect on the next new session.
    // External ACP/openclaw agents own their own prompts (built elsewhere) and
    // are intentionally unaffected.
    {
        let lang = read_app_language(&deps.data_dir).await;
        let directive = output_language_directive(&lang);
        overrides.system_prompt = Some(match overrides.system_prompt.take() {
            Some(existing) => format!("{existing}\n\n{directive}"),
            None => directive.to_owned(),
        });
    }

    if !extra_mcp_servers.is_empty() {
        info!(
            conversation_id = %ctx.conversation_id,
            mcp_count = extra_mcp_servers.len(),
            mcp_names = ?extra_mcp_servers.keys().collect::<Vec<_>>(),
            "Injecting MCP servers into nomi session"
        );
    }

    let model_selection = options.model.as_ref().ok_or_else(|| {
        AppError::BadRequest("Nomi runtime requires a provider and model".to_owned())
    })?;
    ProviderId::try_from(model_selection.provider_id.as_str()).map_err(|_| {
        AppError::BadRequest("Nomi runtime requires a canonical provider_id".to_owned())
    })?;
    if model_selection.model.is_empty() || model_selection.model.trim() != model_selection.model {
        return Err(AppError::BadRequest(
            "Nomi runtime requires a trimmed, non-empty model".to_owned(),
        ));
    }
    if model_selection.use_model.as_deref().is_some_and(|model| {
        model.is_empty() || model.trim() != model
    }) {
        return Err(AppError::BadRequest(
            "Nomi runtime model override must be trimmed and non-empty".to_owned(),
        ));
    }
    let provider_id = &model_selection.provider_id;

    let model_id = model_selection
        .use_model
        .as_deref()
        .unwrap_or(&model_selection.model)
        .to_owned();

    let fields = super::provider_config::resolve_provider_fields_with_fallback(
        &deps.provider_repo,
        &deps.provider_model_repo,
        &deps.encryption_key,
        provider_id,
        &model_id,
    )
    .await?;

    let image_analysis_model = if fields.compat_overrides.supports_image == Some(false) {
        resolve_image_analysis_model(&deps, &ctx.workspace).await?
    } else {
        None
    };

    let session_directory = deps.data_dir.join("nomi-sessions");

    // Stable identity of this conversation instance (row `created_at`).
    // `accept_owned` rejects a session file whose owner token does not match,
    // providing defense in depth against stale or misplaced derived state.
    let conv_created_ms = options.conversation_created_at.ok_or_else(|| {
        AppError::Internal(format!(
            "conversation {} is missing its v3 runtime owner token",
            ctx.conversation_id
        ))
    })?;
    let owner_token = Some(conv_created_ms.to_string());
    let accept_owned =
        |session: nomi_agent::session::Session| -> Option<nomi_agent::session::Session> {
            if !nomi_agent::session::session_belongs_to(
                session.owner_token.as_deref(),
                session.created_at.timestamp_millis(),
                owner_token
                    .as_deref()
                    .expect("v3 nomi owner token was derived above"),
                conv_created_ms,
            ) {
                warn!(
                    conversation_id = %ctx.conversation_id,
                    session_id = %session.id,
                    "Discarding stale nomi session (belongs to a prior conversation that reused this id); starting fresh"
                );
                return None;
            }
            Some(session)
        };

    let resume_session = {
        let session_mgr = SessionManager::new(session_directory.clone(), 100);
        match session_mgr.load(&ctx.conversation_id) {
            Ok(mut session) => {
                // Drop orphaned assistant tool-calls left behind when the user
                // pressed Stop mid-stream. Strict providers (Ollama-style,
                // some OpenAI-compatible proxies) reject replayed assistants
                // with `tool_calls != null` and `content == null` when no
                // matching tool_result follows. See ELECTRON-1HV / ELECTRON-1J6.
                let provider_changed = session.provider != fields.provider;
                let repair = sanitize_resumed_session(&mut session, provider_changed);
                info!(
                    conversation_id = %ctx.conversation_id,
                    session_id = %session.id,
                    message_count = session.messages.len(),
                    provider_changed,
                    removed_messages = repair.removed_messages,
                    removed_tool_calls = repair.removed_tool_calls,
                    removed_tool_results = repair.removed_tool_results,
                    removed_images = repair.removed_images,
                    removed_thinking = repair.removed_thinking,
                    "Loaded existing nomi session for resume"
                );
                retarget_resumed_session(&mut session, &fields.provider, &fields.model);
                let accepted = accept_owned(session);
                if let Some(ref repaired) = accepted
                    && let Err(error) = persist_repaired_session(&session_mgr, repaired)
                {
                    warn!(
                        conversation_id = %ctx.conversation_id,
                        session_id = %repaired.id,
                        error = %error,
                        "Failed to persist repaired nomi session metadata"
                    );
                }
                accepted
            }
            Err(e) => {
                debug!(
                    conversation_id = %ctx.conversation_id,
                    error = %e,
                    "No current-generation nomi session found, starting fresh"
                );
                None
            }
        }
    };

    // System Settings capability toggles, read LIVE per session (toggling in
    // System Settings affects new sessions without a restart). No setting row →
    // host default. computer-use defaults ON on the desktop build (the only one
    // with the feature); browser-use also defaults ON. Browser execution is
    // delegated through a runtime-scoped BrowserLaneClient to the process-wide
    // BrowserSessionHub, which starts managed Browser Hosts lazily. The toggle
    // only controls whether this runtime exposes Browser tools.
    let computer_use_default = read_bool_pref(
        &deps,
        PREF_COMPUTER_USE,
        cfg!(feature = "computer-use") || env_flag("NOMIFUN_COMPUTER_USE"),
    )
    .await;
    // browser-use has a cargo-feature gate (`browser-use`, desktop builds); on
    // those builds it defaults **ON** (user decision). The main-process
    // BrowserSessionHub is the only Chromium/profile owner and shares managed
    // Primary or Crawl Hosts across authorized Lanes. A Nomi runtime receives
    // only a BrowserLaneClient. Builds without the feature register no Browser
    // tools. `NOMIFUN_BROWSER_USE` forces the setting on for parity/testing.
    let browser_use_default = read_bool_pref(
        &deps,
        PREF_BROWSER_USE,
        cfg!(feature = "browser-use") || env_flag("NOMIFUN_BROWSER_USE"),
    )
    .await;
    // F1-sec: evaluate「全权模式」LIVE 值（裁决⑨，default-deny）。用户在 System Settings 显式 opt-in
    // 的 `agent.browserUse.fullPower` 开关，每会话构造时 LIVE 读（read_bool_pref 范式，与上面的启用开关
    // 同源），灌进 BrowserConfig.full_power，由 Hub-backed Browser tool adapter 在进入
    // BrowserLaneClient 前执行 evaluate gate。默认 OFF（host_default=false）——evaluate 是最高危
    // 逃生舱，无 opt-in 即封死。**绝不看 session_mode**（不变量⑧）。
    let browser_full_power_default = read_bool_pref(
        &deps,
        PREF_BROWSER_FULL_POWER,
        env_flag("NOMIFUN_BROWSER_FULL_POWER"),
    )
    .await;
    // SD-6: 持久登录 LIVE 值（DESIGN §16/§27 互斥约束）。产品默认 ON（host_default=true）——持久登录
    // 开启时与全权互斥（evaluate Blocked）。用户可在 System Settings 关闭以解除互斥。
    let browser_persistent_login_default =
        read_bool_pref(&deps, PREF_BROWSER_PERSISTENT_LOGIN, true).await;
    // P7A: site-memory LIVE 值。host_default=false（OFF）——把站点交互持久化到磁盘是隐私相关行为，
    // 须用户在 System Settings 显式 opt-in。
    let browser_site_memory_default = read_bool_pref(&deps, PREF_BROWSER_SITE_MEMORY, false).await;
    // Phase D: takeover/approval gate LIVE value. host_default=true (ON): install a gate by
    // default. Non-yolo sessions can prompt for risky Browser actions / gated cross-origin
    // POSTs; full-auto/yolo sessions still install the gate, but the gate approves directly.
    let browser_takeover_default = read_bool_pref(&deps, PREF_BROWSER_TAKEOVER, true).await;
    let browser_unrestricted_approval_default =
        read_bool_pref(&deps, PREF_BROWSER_UNRESTRICTED_APPROVAL, false).await;
    // P7B: visual-fallback LIVE 值。host_default=false（OFF）——每次兜底都过一遍视觉模型，有额外 token
    // 成本，须用户在 System Settings 显式 opt-in。
    let browser_visual_fallback_default =
        read_bool_pref(&deps, PREF_BROWSER_VISUAL_FALLBACK, false).await;
    // Browser management is status-only. Primary Chromium is always shown in
    // its managed external window; historical embedded/headless/silent values
    // are frontend migration inputs only and never affect runtime headlessness.
    // Browser Host 可执行文件来源偏好（与 silent 正交）。host_default="system"，优先系统安装
    // 的 Chrome/Edge，未探测到时回退 managed。该值不授予 runtime 所有权：主进程
    // BrowserSessionHub 统一创建/共享 Host，Primary 使用应用管理的稳定 profile，Crawl 使用临时 profile。
    let browser_source_default =
        read_string_pref(&deps, PREF_BROWSER_SOURCE, BROWSER_SOURCE_DEFAULT).await;

    let coding_profile =
        nomi_agent::TaskProfile::parse(overrides.task_profile.as_deref()).is_coding();

    // Coding sessions default computer/browser off unless the session extra
    // explicitly opts in. Global prefs are not mutated.
    let browser_use_enabled = if coding_profile && overrides.browser_use.is_none() {
        false
    } else {
        overrides.browser_use.unwrap_or(browser_use_default)
    };
    let computer_use_enabled = if coding_profile && overrides.computer_use.is_none() {
        false
    } else {
        overrides.computer_use.unwrap_or(computer_use_default)
    };

    // Build the shared browser secret-vault descriptor when browser-use is on.
    // Every caller uses `{data_dir}/browser-secrets/shared`; this policy store
    // backs Native, Gateway, and registration paths. Bootstrap passes the
    // machine-bound key to the Hub-backed Browser tool adapter so `secret:NAME`
    // resolves under origin checks and registered `allowed_origins` contribute
    // to the egress firewall. It is not a Chromium profile and conveys no
    // Browser Host ownership.
    let browser_secret_vault = if browser_use_enabled {
        Some(crate::types::BrowserSecretVault {
            vault_path: nomifun_secret::shared_vault_path(&deps.data_dir),
            key: deps.encryption_key,
        })
    } else {
        None
    };

    #[cfg(feature = "browser-use")]
    let browser_lane_binding = if browser_use_enabled {
        match deps.browser_lane_provider.as_ref() {
            Some(slot) => {
                let provider = slot.get().ok_or_else(|| {
                    AppError::Internal(
                        "browser use is enabled but the process-wide Browser Session Hub provider \
                         has not been installed"
                            .to_owned(),
                    )
                })?;
                let runtime_instance_id = format!(
                    "native:{}:{}",
                    ctx.conversation_id,
                    uuid::Uuid::now_v7()
                );
                Some(
                    provider
                        .issue(
                            crate::factory::browser_lane::TrustedBrowserRuntimeContext {
                                user_id: options.user_id.clone(),
                                conversation_id: Some(ctx.conversation_id.clone()),
                                runtime_instance_id,
                                agent_id: Some("nomi".to_owned()),
                                // Execution ownership is resolved by the host
                                // provider from the authoritative persisted
                                // ConversationLink. It is never read from
                                // `options.extra`.
                                execution_id: None,
                                step_id: None,
                                attempt_id: None,
                                surface:
                                    nomifun_browser_platform::BrowserSurface::Native,
                            },
                        )
                        .await?,
                )
            }
            // Explicit standalone/test composition. Production AppServices
            // always supplies a slot, so a provider outage cannot create an
            // alternate browser owner outside BrowserSessionHub.
            None => None,
        }
    } else {
        None
    };

    // MoA global fallback: a session that carries no explicit `extra.moa`
    // inherits the System-Settings-wide `moa_settings` preference. Session
    // extra always wins; a non-owner runtime was already ceilinged to
    // `moa = None` above and must NOT be re-widened from global settings;
    // companion sessions never enable MoA (the bridge gate below also holds).
    if is_instance_owner && overrides.moa.is_none() && !overrides.companion {
        let global_raw = read_raw_pref(&deps, PREF_MOA_SETTINGS).await;
        apply_global_moa_fallback(&mut overrides, global_raw.as_deref());
    }

    // MoA bridge: convert the opt-in `extra.moa` DTO and resolve each reference
    // slot's provider row into a ready Config. Companion sessions and sessions
    // with no usable slot stay single-model (`None` → the manager never calls
    // `set_moa_state`, byte-identical to a build without MoA).
    let moa = super::moa::resolve_moa_bridge(
        &overrides,
        &deps.provider_repo,
        &deps.provider_model_repo,
        &deps.encryption_key,
        std::path::Path::new(&ctx.workspace),
        &deps.data_dir,
        &ctx.conversation_id,
    )
    .await;
    if let Some(ref bridge) = moa {
        info!(
            conversation_id = %ctx.conversation_id,
            reference_slots = bridge.slots.len(),
            fanout = %bridge.config.fanout,
            "MoA reference fan-out enabled for nomi session"
        );
    }

    // Goal restore source, first match wins: an explicit `resume_state`
    // carried in the build extra, else the persisted active/paused/waiting
    // row for this conversation (goal persistence on). Terminal rows
    // (complete/blocked/cleared) stay in the DB for audit but never restart
    // continuation; a corrupt payload fails soft to "no restore".
    let goal_resume_state: Option<nomi_agent::goal::state::GoalState> =
        match overrides.goal.as_ref().and_then(|g| g.resume_state.clone()) {
            Some(raw) => match serde_json::from_value(raw) {
                Ok(state) => Some(state),
                Err(e) => {
                    warn!(
                        conversation_id = %ctx.conversation_id,
                        error = %e,
                        "Ignoring malformed goal resume_state in build extra"
                    );
                    None
                }
            },
            None => match deps.goal_repo.as_ref() {
                Some(repo) => match repo.load_by_session(&ctx.conversation_id).await {
                    Ok(Some(row))
                        if crate::goal_bridge::goal_status_is_restorable(&row.status) =>
                    {
                        Some(crate::goal_bridge::goal_row_to_state(&row))
                    }
                    Ok(_) => None,
                    Err(e) => {
                        warn!(
                            conversation_id = %ctx.conversation_id,
                            error = %e,
                            "Failed to load persisted goal; session starts without restore"
                        );
                        None
                    }
                },
                None => None,
            },
        };
    // Host policy: with the goal domain off, a persisted active row must not
    // re-arm the loop either (the fresh spec was already cleared above), or
    // `update_goal` would stay registered through the restore path.
    let goal_resume_state = if deps.tool_policy.domains.goal {
        goal_resume_state
    } else {
        None
    };

    let output_ceiling = fields.output_limit;
    let reasoning_effort = resolve_session_reasoning_effort(
        overrides.reasoning_effort.as_deref(),
        fields.compat_overrides.effort_levels.as_deref(),
    );

    // Host composition was decided once near the top of this function (it also
    // shapes the delegation prompt); nothing recomputes it here.
    let config = NomiResolvedConfig {
        // provider_id was validated as a canonical UUID just above.
        provider_id: ProviderId::parse(&model_selection.provider_id).expect(
            "session provider id already validated as a canonical ProviderId",
        ),
        provider: fields.provider,
        api_key: fields.api_key,
        model: fields.model.clone(),
        base_url: fields.base_url,
        system_prompt: overrides.system_prompt,
        output_ceiling,
        max_turns: overrides.max_turns,
        context_limit: fields.context_limit.map(|v| v as u64),
        compat_overrides: fields.compat_overrides,
        image_analysis_model,
        session_directory,
        // 默认授权模式 = 全自动（yolo）。产品决策：所有 nomi 会话默认自动批准
        // 标准工具类别（info/edit/exec/mcp —— 文件编辑 / Shell / 标准工具 & MCP），
        // 不再反复弹授权框。理由：
        //  - companion / IM Channel Agent 本就无审批 UI（其首个 gateway/file/bash
        //    工具调用会 park 在 rx.await，turn 永不 finish → 聊天永久「思考中」），
        //    所以它们历来必须 yolo；现在把这一默认推广到普通桌面会话。
        //  - **显式 `extra.session_mode` 仍胜出**：用户在权限选择器里手动降级为
        //    `default` / `auto_edit` 会写偏好并经 extra 传入，这里的 `.or_else` 让显式值
        //    优先，降级正常生效。
        //  - Full-power evaluate and desktop-control toggles remain separate System Settings
        //    and are not granted by session_mode. Browser approval prompts are ordinary
        //    permission friction: full-auto/yolo is honored by the Browser approval gate, so
        //    gated Browser actions approve without UI.
        session_mode: overrides
            .session_mode
            .clone()
            .or_else(|| Some("yolo".to_owned())),
        extra_mcp_servers,
        loopback_capability_leases,
        bedrock_config: fields.bedrock_config,
        computer_use: computer_use_enabled,
        browser_use: browser_use_enabled,
        // Browser Host 可执行文件来源偏好；BrowserSessionHub 仍是唯一 owner。
        browser_source: browser_source_default,
        // F1-sec: 全权模式 LIVE 值（无 per-session override，纯 client_preferences 全局开关）。
        browser_full_power: browser_full_power_default,
        // SD-6: 持久登录 LIVE 值（产品默认 ON，无 per-session override）。
        browser_persistent_login: browser_persistent_login_default,
        // P7A: site-memory LIVE 值（默认 OFF，opt-in；无 per-session override）。
        browser_site_memory: browser_site_memory_default,
        // Phase D: takeover/审批 gate LIVE 值（产品默认 ON；无 per-session override）。
        browser_takeover: browser_takeover_default,
        browser_unrestricted_approval: browser_unrestricted_approval_default,
        // P7B: visual-fallback LIVE 值（默认 OFF，opt-in；无 per-session override）。
        browser_visual_fallback: browser_visual_fallback_default,
        goal: overrides.goal.clone().map(|g| {
            nomi_agent::goal::runtime::GoalSpec::new(
                g.objective,
                g.max_auto_continuations.unwrap_or(8),
            )
        }),
        goal_resume_state,
        // MoA bridge payload (resolved above; None = single-model, zero change).
        moa,
        // Shared browser secret-vault descriptor (built above; None when
        // browser-use is off). It carries policy credentials, not Host/profile
        // ownership.
        browser_secret_vault,
        // Owning conversation instance identity — the nomi manager stamps it
        // onto the session after build so a future reused id is rejected.
        owner_token: owner_token.clone(),
        // Host composition is backend-authoritative, never user config. A
        // Platform Gateway owns persistent AgentExecution; secondary users
        // cannot install host execution. Only trusted no-gateway standalone
        // sessions receive the embedded adapter.
        install_embedded_agent_execution,
        // Per-session 工具白名单（受限角色的 Agent attempt；普通会话恒空）。
        allowed_tools: overrides.allowed_tools.clone(),
        // 宿主级工具策略（agent-store `[tools]`；未采纳的宿主为 permissive 默认值）。
        tool_policy: deps.tool_policy.clone(),
        // 宿主级内置记忆总开关（agent-store `[memory] enabled`；未声明的宿主为 ON）。
        memory_enabled: deps.memory_enabled,
        // 原生文件工具写根：本地桌面全权（None），渠道会话收窄到工作区。
        // Coding profile forces workspace containment when write_root would
        // otherwise be None (local desktop unrestricted).
        // 与 gateway file-service 的 PathAuthority 同一信任模型（file-access spec）。
        write_root: {
            let mut root = if is_instance_owner {
                resolve_native_write_root(
                    overrides.channel_platform.as_deref(),
                    &ctx.workspace,
                )
            } else {
                Some(ctx.workspace.clone())
            };
            if coding_profile && root.is_none() {
                root = Some(ctx.workspace.clone());
            }
            root
        },
        reasoning_effort,
        task_profile: overrides.task_profile.clone(),
        coding_verification: overrides.coding_verification.clone(),
        coding_protect_read: overrides.coding_protect_read,
        coding_micro_keep_recent: overrides.coding_micro_keep_recent,
    };

    // Scope of the native knowledge_search / knowledge_read tools, derived
    // from the mounted bases.
    let knowledge_kb_ids: Vec<nomifun_common::KnowledgeBaseId> = overrides
        .knowledge_mounts
        .iter()
        .map(|m| m.knowledge_base_id.clone())
        .collect();

    // Write-back ("回血") wiring for the native knowledge_write tool. The sink
    // is passed only when the resolved policy permits writing (channel sessions
    // resolve to Disabled → sink=None → tool not registered). `(id, name)` lets
    // the tool resolve the base the model names back to the opaque id. The
    // staged/direct decision was made above by the per-surface policy.
    let knowledge_write_bases: Vec<(nomifun_common::KnowledgeBaseId, String)> = overrides
        .knowledge_mounts
        .iter()
        .map(|m| (m.knowledge_base_id.clone(), m.name.clone()))
        .collect();
    let knowledge_writeback_sink = if knowledge_write_enabled {
        deps.knowledge_writeback.clone()
    } else {
        None
    };

    // Course-generation wiring for the native learning_generate_course tool:
    // turns a mounted knowledge base into a learning course through the
    // backend learning service. Owner-authority only, same posture as the
    // knowledge retrieval sinks above; the manager further gates registration
    // on mounted bases.
    let learning_course_sink = (is_instance_owner && deps.tool_policy.domains.learning)
        .then(|| deps.learning_course.clone())
        .flatten();

    let knowledge_prelude: Option<String> = if overrides.knowledge_mounts.is_empty() {
        None
    } else {
        let names: Vec<&str> = overrides
            .knowledge_mounts
            .iter()
            .map(|m| m.name.as_str())
            .collect();
        Some(format!(
            "[Knowledge bases mounted: {}] Before answering, if this task relates to any of these, \
             call the knowledge_search tool first and open the matching document. Do not rely on \
             memory for topics these bases cover.",
            names.join(", ")
        ))
    };

    let conv_id_for_cron = ctx.conversation_id.clone();
    let owner_id_for_cron = overrides
        .user_id
        .as_deref()
        .map(str::trim)
        .filter(|owner| !owner.is_empty())
        .map(ToOwned::to_owned);
    // SSH-bound session: connect the saved host now (decrypt credential, dial,
    // open shell + SFTP) so the runtime gets the remote tool family. A binding
    // without a configured provider, or a failed connect, fails the build with a
    // clear error rather than silently running against the local machine. The
    // conversation id goes with the request: the provider pools one link per
    // (conversation, host), so a runtime rebuilt by a model switch rejoins the
    // session its predecessor was using instead of dialling again.
    let ssh_session = if let Some(ssh_host_id) = overrides.ssh_host_id.clone() {
        let user_id = overrides.user_id.clone().unwrap_or_default();
        let remote_cwd = overrides
            .ssh_remote_cwd
            .clone()
            .unwrap_or_else(|| ".".to_string());
        match &deps.ssh_provider {
            Some(provider) => Some(
                provider
                    .connect(
                        &user_id,
                        ctx.conversation_id.as_str(),
                        &ssh_host_id,
                        &remote_cwd,
                    )
                    .await
                    .map_err(|e| AppError::Internal(format!("SSH connect failed: {e}")))?,
            ),
            None => {
                return Err(AppError::BadRequest(
                    "conversation is bound to an SSH host but SSH support is not configured".into(),
                ));
            }
        }
    } else {
        None
    };
    let host_wiring = NomiHostWiring {
        #[cfg(feature = "browser-use")]
        browser_lane_binding,
        ssh_backend: ssh_session.as_ref().map(|s| Arc::clone(&s.backend)),
        ssh_lease: ssh_session.map(|s| s.lease),
        // Engine-side OAuth refresh hook: on a 401 the MCP manager refreshes
        // once, updates the Authorization header and retries once.
        mcp_oauth_refresher: deps
            .mcp_oauth_service
            .as_ref()
            .map(|oauth| -> Arc<dyn nomi_mcp::manager::McpOAuthRefresher> {
                Arc::new(NomiMcpOAuthRefresher::new(oauth.clone()))
            }),
    };
    let agent = NomiAgentManager::new_with_search_provider(
        ctx.conversation_id,
        ctx.workspace,
        config,
        resume_session,
        (is_instance_owner && deps.tool_policy.domains.requirement)
            .then(|| deps.requirement_sink.clone())
            .flatten(),
        if is_instance_owner && overrides.companion {
            deps.companion_sink.clone()
        } else {
            None
        },
        (is_instance_owner && deps.tool_policy.domains.knowledge)
            .then(|| deps.knowledge_retrieval.clone())
            .flatten(),
        knowledge_kb_ids,
        knowledge_prelude,
        knowledge_writeback_sink,
        knowledge_write_bases,
        learning_course_sink,
        // Owner user id for the native course-generation tools: jobs are
        // created under this principal, so the tools can start/query jobs on
        // behalf of the installation owner (same identity the cron sink uses).
        owner_id_for_cron.clone(),
        if is_instance_owner && overrides.companion {
            deps.companion_skill_sink.clone()
        } else {
            None
        },
        deps.search_provider.clone(),
        deps.extract_coordinator.clone(),
        summon_wiring,
        host_wiring,
    )
    .await?;
    // Goal persistence: wire the repository so the manager mirrors every goal
    // state change (turn end / user actions) into the `goals` table. Absent
    // (`None`) = persistence off, goals stay in-memory only (fail-safe).
    if let Some(goal_repo) = deps.goal_repo.clone() {
        agent.register_goal_persistence(goal_repo);
    }
    // A restored fresh goal (contract-less, no turns burned — typically set
    // through the DB fallback while no runtime existed, e.g. the guid-page
    // goal switch) gets its completion contract auto-drafted in the
    // background. Runs after the repo wiring above so the drafted contract
    // is persisted; a no-op for contract-carrying or mid-flight goals.
    agent.spawn_goal_contract_autodraft();
    // Native cron tools persist background work and can recursively create
    // model traffic. They are host-control capabilities, not part of the
    // secondary principal's model-only ceiling. Register them only for the
    // installation owner, after the manager has been assembled.
    if is_instance_owner
        && deps.tool_policy.domains.cron
        && let (Some(make_sink), Some(owner_id)) =
        (deps.cron_sink_factory.as_ref(), owner_id_for_cron.as_deref())
    {
        agent
            .register_cron_sink(make_sink(owner_id, &conv_id_for_cron))
            .await;
    }
    if is_instance_owner
        && deps.tool_policy.domains.meeting
        && let (Some(make_sink), Some(owner_id)) =
        (deps.meeting_sink_factory.as_ref(), owner_id_for_cron.as_deref())
    {
        agent
            .register_meeting_sink(make_sink(owner_id, &conv_id_for_cron))
            .await;
    }
    if is_instance_owner
        && deps.tool_policy.domains.meeting
        && let Some(make_listen) = deps.meeting_listen_context_factory.as_ref()
    {
        agent
            .register_meeting_listen_context(make_listen(&conv_id_for_cron))
            .await;
    }
    // Host-backed `nomi_delegate` (`16` §7 决策 3): the host owns a durable Agent
    // execution facade, so its leader sessions delegate through that. Registered
    // only when this session did **not** get the embedded deployment — one tool
    // name, one owner. A slot that was never installed (or a host without one)
    // registers nothing, which is the same shape as cron/meeting above.
    if is_instance_owner
        && !install_embedded_agent_execution
        && let (Some(provider), Some(owner_id)) = (
            deps.delegate_sink_provider
                .as_ref()
                .and_then(|slot| slot.get()),
            owner_id_for_cron.as_deref(),
        )
    {
        agent
            .register_delegate_sink(provider.sink_for(owner_id, &conv_id_for_cron))
            .await;
    }
    // Per-turn background review (optimization 2): register the default
    // lightweight reviewer for non-companion sessions where distill is enabled.
    // Fire-and-forget — never blocks the conversation loop.
    agent.register_default_post_turn_review();
    Ok(AgentRuntimeHandle::Nomi(Arc::new(agent)))
}

/// Host-level default for opt-in tool capabilities ("1"/"true" enables).
fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// `client_preferences` keys for the System Settings capability toggles
/// (written by the frontend via `configService`, read here per session).
const PREF_COMPUTER_USE: &str = "agent.computerUse";
const PREF_BROWSER_USE: &str = "agent.browserUse";
/// **F1-sec**: browser-use evaluate「全权模式」开关（裁决⑨）。`true` → evaluate 放行（仍受与持久登录
/// 互斥约束）；缺/`false` → evaluate 默认 OFF（最高危逃生舱 default-deny）。前端 System Settings 写。
const PREF_BROWSER_FULL_POWER: &str = "agent.browserUse.fullPower";
/// **SD-6**: browser-use 持久登录开关（裁决⑨ 互斥约束）。`true`（产品默认）→ 与全权互斥；`false` → 解除互斥。
const PREF_BROWSER_PERSISTENT_LOGIN: &str = "agent.browserUse.persistentLogin";
/// **P7A**: browser-use 站点记忆开关（opt-in，隐私相关）。`true` → 跨会话记住站点结构 + 注入 hints；
/// 缺/`false`（host_default）→ OFF（不持久化、零行为变化）。前端 System Settings 写。
const PREF_BROWSER_SITE_MEMORY: &str = "agent.browserUse.siteMemory";
/// **Phase D**: browser-use 人机接管 + 跨域 POST 审批 gate。`true` → 注入审批 gate
/// （默认会话浮给用户；full-auto/yolo 直接通过）；缺失时 host_default=true。前端 System Settings 写。
const PREF_BROWSER_TAKEOVER: &str = "agent.browserUse.takeover";
/// **Phase D**: browser-use 显式无限制审批开关。`true` → Browser approval gate 不再浮出确认。
const PREF_BROWSER_UNRESTRICTED_APPROVAL: &str = "agent.browserUse.unrestrictedApproval";
/// **P7B**: browser-use 视觉兜底点击（opt-in，有 token 成本）。`true` → DOM/aria 锚定失败时截图交视觉
/// 模型定位再点；缺/`false`（host_default）→ OFF（不注入 locator、零行为变化）。前端 System Settings 写。
const PREF_BROWSER_VISUAL_FALLBACK: &str = "agent.browserUse.visualFallback";
/// Browser Host 可执行文件来源偏好（与 silent 正交）。`"managed"` = 内置/下载 CfT；
/// `"system"`（默认）= 系统 Chrome/Edge 本体优先（未探到回退 managed）。前端写入偏好；
/// 主进程 BrowserSessionHub 仍统一拥有 Host 和应用管理 profile。
const PREF_BROWSER_SOURCE: &str = "agent.browserUse.source";
/// Browser Host 来源默认值（无设置行/无 client_prefs 时）：系统安装的 Chrome / Edge。
const BROWSER_SOURCE_DEFAULT: &str = "system";
/// Global (System Settings) MoA configuration, stored in `client_preferences`
/// as a `MoaSettings` JSON string. Written by the frontend through the generic
/// `GET/PUT /api/settings/client` key-value endpoints; read here per session as
/// the fallback when the conversation carries no explicit `extra.moa`.
const PREF_MOA_SETTINGS: &str = "moa_settings";
const PREF_IMAGE_ANALYSIS_MODEL: &str = "tools.imageAnalysisModel";

fn catalog_model_base(model: &str) -> &str {
    model
        .get(..5)
        .filter(|prefix| prefix.eq_ignore_ascii_case("AIPC-"))
        .and_then(|_| model.get(5..))
        .unwrap_or(model)
}

fn is_preferred_image_analysis_model(model: &str) -> bool {
    image_analysis_model_priority(model) == 3
}

/// Image analysis model priority ranking:
/// 3: DeepSeek Vision (deepseek-v4-flash-vision, deepseek-vl, etc.) - top priority (fast, low-cost)
/// 2: MiniMax Vision (MiniMax-M3, etc.) - secondary priority
/// 1: Kimi / Moonshot Vision - fallback
/// 0: Others
fn image_analysis_model_priority(model: &str) -> u8 {
    let lower = catalog_model_base(model).to_lowercase();
    if lower.contains("deepseek") && (lower.contains("vision") || lower.contains("vl") || lower.contains("flash-vision")) {
        3
    } else if lower.contains("minimax-m3") || lower.contains("minimax") {
        2
    } else if lower.contains("kimi") || lower.contains("moonshot") {
        1
    } else {
        0
    }
}

fn is_image_analysis_eligible_provider(platform: &str) -> bool {
    !platform.eq_ignore_ascii_case("nomifun-free-model")
}

fn is_usable_image_analysis_model(
    provider_enabled: bool,
    model: &nomifun_db::models::ProviderModelRow,
) -> bool {
    if !provider_enabled || !model.enabled {
        return false;
    }
    let tasks = serde_json::from_str::<Vec<ModelTask>>(&model.tasks).unwrap_or_default();
    let traits = serde_json::from_str::<Vec<ModelTrait>>(&model.traits).unwrap_or_default();
    let health = model
        .health
        .as_deref()
        .and_then(|value| serde_json::from_str::<ModelHealthStatus>(value).ok())
        .map(|status| status.status);
    tasks.contains(&ModelTask::Chat)
        && traits.contains(&ModelTrait::VisionInput)
        && health != Some(HealthStatus::Unhealthy)
}

fn parse_image_analysis_preference(raw: &str) -> Result<ProviderWithModel, AppError> {
    let parsed = serde_json::from_str(raw)
        .or_else(|_| serde_json::from_str::<String>(raw).and_then(|value| serde_json::from_str(&value)))
        .map_err(|error| AppError::BadRequest(format!("Invalid image analysis model setting: {error}")))?;
    let selection: ProviderWithModel = serde_json::from_value(parsed)
        .map_err(|error| AppError::BadRequest(format!("Invalid image analysis model setting: {error}")))?;
    selection
        .validate()
        .map_err(|error| AppError::BadRequest(format!("Invalid image analysis model setting: {error}")))?;
    if selection.use_model.is_some() {
        return Err(AppError::BadRequest(
            "Invalid image analysis model setting: use_model is not supported".to_owned(),
        ));
    }
    Ok(selection)
}

fn config_for_image_analysis(
    fields: super::provider_config::ResolvedProviderFields,
    workspace: &str,
) -> Result<Config, AppError> {
    let cli_args = CliArgs {
        provider: Some(fields.provider),
        api_key: Some(fields.api_key),
        base_url: fields.base_url,
        model: Some(fields.model),
        max_tokens: Some(4096),
        max_turns: Some(1),
        system_prompt: None,
        profile: None,
        auto_approve: false,
        project_dir: Some(std::path::PathBuf::from(workspace)),
    };
    let mut config = Config::resolve(&cli_args)
        .map_err(|error| AppError::Internal(format!("Image analysis config resolve failed: {error}")))?;
    config.bedrock = fields.bedrock_config;
    if let Some(field) = fields.compat_overrides.max_tokens_field {
        config.compat.max_tokens_field = Some(field);
    }
    if let Some(path) = fields.compat_overrides.api_path {
        config.compat.api_path = Some(path);
    }
    if let Some(supports_image) = fields.compat_overrides.supports_image {
        config.compat.supports_image = Some(supports_image);
    }
    if let Some(header) = fields.compat_overrides.mirror_bearer_header {
        config.compat.mirror_bearer_header = Some(header);
    }
    if let Some(required) = fields.compat_overrides.require_reasoning_content {
        config.compat.require_reasoning_content = Some(required);
    }
    Ok(config)
}

/// Resolve the separately configured image analyzer. An explicit preference is
/// strict: a removed, disabled, unhealthy, or text-only model is a user-visible
/// configuration error. The absence of a preference is intentionally lenient
/// until a non-vision conversation actually attaches an image.
async fn resolve_image_analysis_model(
    deps: &AgentFactoryDeps,
    workspace: &str,
) -> Result<Option<ImageAnalysisModelConfig>, AppError> {
    let providers = deps
        .provider_repo
        .list()
        .await
        .map_err(|error| AppError::Internal(format!("Failed to list providers: {error}")))?;
    let configured = read_raw_pref(deps, PREF_IMAGE_ANALYSIS_MODEL).await;
    let explicit = configured
        .as_deref()
        .map(parse_image_analysis_preference)
        .transpose()?;

    let candidate = if let Some(selection) = explicit {
        let provider = providers
            .iter()
            .find(|provider| provider.provider_id == selection.provider_id)
            .ok_or_else(|| AppError::BadRequest("Configured image analysis provider no longer exists".to_owned()))?;
        let model = deps
            .provider_model_repo
            .get(&selection.provider_id, &selection.model)
            .await
            .map_err(|error| AppError::Internal(format!("Failed to load image analysis model: {error}")))?
            .ok_or_else(|| AppError::BadRequest("Configured image analysis model no longer exists".to_owned()))?;
        if !is_usable_image_analysis_model(provider.enabled, &model) {
            return Err(AppError::BadRequest(
                "Configured image analysis model must be enabled, healthy, chat-capable, and support image input".to_owned(),
            ));
        }
        Some((selection.provider_id, selection.model))
    } else {
        let mut best_candidate = None;
        let mut best_priority = 0u8;

        for provider in &providers {
            if !is_image_analysis_eligible_provider(&provider.platform) {
                continue;
            }
            let models = deps
                .provider_model_repo
                .list_for_provider(&provider.provider_id)
                .await
                .map_err(|error| AppError::Internal(format!("Failed to list provider models: {error}")))?;
            for model in models {
                if !is_usable_image_analysis_model(provider.enabled, &model) {
                    continue;
                }
                let priority = image_analysis_model_priority(&model.model);
                let candidate = (provider.provider_id.clone(), model.model.clone());

                if best_candidate.is_none() || priority > best_priority {
                    best_priority = priority;
                    best_candidate = Some(candidate);
                    if priority == 3 {
                        // Max priority (DeepSeek vision) found, stop scanning
                        break;
                    }
                }
            }
            if best_priority == 3 {
                break;
            }
        }
        best_candidate
    };

    let Some((provider_id, model)) = candidate else {
        return Ok(None);
    };
    let fields = super::provider_config::resolve_provider_fields(
        &deps.provider_repo,
        &deps.provider_model_repo,
        &deps.encryption_key,
        &provider_id,
        &model,
    )
    .await?;
    Ok(Some(ImageAnalysisModelConfig {
        config: config_for_image_analysis(fields, workspace)?,
        label: format!("{provider_id}/{model}"),
    }))
}

/// Read a boolean `client_preferences` toggle live, falling back to
/// `host_default` when there is no setting row (fresh install) or no
/// client_prefs repo is wired. The frontend `configService` persists bare JSON
/// (`true`/`false`); the raw settings API may store the quoted string forms.
/// Read per session so toggling the setting affects new sessions without a
/// restart.
async fn read_bool_pref(deps: &AgentFactoryDeps, key: &str, host_default: bool) -> bool {
    let Some(repo) = deps.client_prefs.as_ref() else {
        return host_default;
    };
    match repo.get_by_keys(&[key]).await {
        Ok(rows) => rows
            .into_iter()
            .find(|r| r.key == key)
            .map(|r| parse_bool_pref(&r.value, host_default))
            .unwrap_or(host_default),
        Err(_) => host_default,
    }
}

/// Shared boolean-preference parse semantics for the `agent.browserUse.*`
/// toggles this factory shares with the Hub.
///
/// Deliberately identical to the boot-time reader in nomifun-app
/// `load_browser_startup_preferences` (services.rs), so Hub startup policy and
/// this per-session policy can never disagree about the same stored row:
/// quotes are trimmed (a raw settings-API write stores JSON strings like
/// `"false"`), an explicit opposite value flips the toggle, and any junk value
/// resolves to `host_default` — default-ON toggles (e.g. persistentLogin)
/// parse as `value != "false"`, default-OFF toggles (e.g. fullPower) parse as
/// `value == "true"`.
fn parse_bool_pref(value: &str, host_default: bool) -> bool {
    let value = value.trim().trim_matches('"');
    if host_default {
        value != "false"
    } else {
        value == "true"
    }
}

/// Read a string `client_preferences` value live, falling back to `host_default`
/// when there is no setting row (fresh install), no client_prefs repo is wired, or
/// the stored value is blank. Mirrors [`read_bool_pref`] for stringly settings
/// (e.g. `agent.browserUse.source` = `"managed"`/`"system"`). Read per session so
/// toggling the setting affects new sessions without a restart.
async fn read_string_pref(deps: &AgentFactoryDeps, key: &str, host_default: &str) -> String {
    let Some(repo) = deps.client_prefs.as_ref() else {
        return host_default.to_owned();
    };
    match repo.get_by_keys(&[key]).await {
        Ok(rows) => rows
            .into_iter()
            .find(|r| r.key == key)
            .map(|r| r.value.trim().trim_matches('"').to_owned())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| host_default.to_owned()),
        Err(_) => host_default.to_owned(),
    }
}

/// Read a raw `client_preferences` value live, verbatim (no quote stripping —
/// the stored value may be a JSON document, e.g. [`PREF_MOA_SETTINGS`]).
/// `None` when there is no row, the row is blank, no repo is wired, or the
/// read fails — callers treat all of those as "no global value".
async fn read_raw_pref(deps: &AgentFactoryDeps, key: &str) -> Option<String> {
    let repo = deps.client_prefs.as_ref()?;
    match repo.get_by_keys(&[key]).await {
        Ok(rows) => rows
            .into_iter()
            .find(|r| r.key == key)
            .map(|r| r.value)
            .filter(|v| !v.trim().is_empty()),
        Err(_) => None,
    }
}

/// Apply the global `moa_settings` fallback onto the session overrides.
/// Precedence: an explicit session `extra.moa` always wins; companion
/// sessions never inherit MoA; a malformed global JSON degrades to "no
/// fallback" with a warning (never a build error). Pure so the precedence
/// matrix is unit-testable without a repo.
fn apply_global_moa_fallback(overrides: &mut NomiBuildExtra, global_raw: Option<&str>) {
    if overrides.moa.is_some() || overrides.companion {
        return;
    }
    let Some(raw) = global_raw else {
        return;
    };
    // The frontend persists the settings as a JSON *string* value, and the
    // client-prefs service re-serializes every value — so the stored row is a
    // double-encoded string literal (`"{\"enabled\":...}"`). Unwrap that layer
    // first; a bare object (written by non-UI clients) still parses directly.
    let unwrapped = serde_json::from_str::<String>(raw);
    let effective = unwrapped.as_deref().unwrap_or(raw);
    match serde_json::from_str::<nomifun_api_types::MoaSettings>(effective) {
        Ok(settings) => overrides.moa = Some(settings),
        Err(error) => {
            warn!(error = %error, "Ignoring malformed global moa_settings preference");
        }
    }
}

/// Resolve the effective app language: an explicitly **persisted installation**
/// `language` preference wins; otherwise fall back to the host OS locale (so a
/// fresh install on a Chinese system replies in Chinese without the owner
/// touching settings); finally English. `os_locale` is injected so the
/// resolution is deterministically unit-testable. The SQLite `system_settings`
/// seed `en-US` is intentionally ignored — it is not a user choice.
fn resolve_language(installation: Option<&str>, os_locale: Option<&str>) -> String {
    if let Some(language) = installation.map(str::trim).filter(|value| !value.is_empty()) {
        return normalize_ui_language(Some(language));
    }
    normalize_ui_language(os_locale)
}

/// Read the effective app UI language live: installation preference if set,
/// else the host OS locale, else English. Read per build so a language switch
/// — or first-run OS detection — takes effect on the next agent (re)build.
async fn read_app_language(data_dir: &Path) -> String {
    resolve_language(
        read_installation_language(data_dir).as_deref(),
        sys_locale::get_locale().as_deref(),
    )
}

/// Map a stored app-language code to the output-language directive appended LAST
/// to every nomi session's system prompt. Covers BOTH the final reply and the
/// model's reasoning / thinking, phrased as an explicit override so it wins over
/// the English base prompt and any earlier (possibly persisted) language line,
/// while still letting the owner pull the session into another language by
/// writing in it. Unknown / empty / en-US all resolve to English (the app
/// default); only the supported `zh-CN` selects Chinese (supported set lives in
/// `nomifun-system`).
fn output_language_directive(lang: &str) -> &'static str {
    match lang {
        "zh-CN" => {
            "【输出语言】无论上文的指令或记忆使用何种语言，请始终用简体中文进行思考与回复\
                    （包括你的推理/思考过程）——除非主人主动用其他语言和你说话，或明确要求你换一种语言。"
        }
        _ => {
            "[Output language] Regardless of the language used in the instructions or memories \
              above, always think and reply in English (including your reasoning / thinking \
              process) — unless the owner writes to you in another language or explicitly asks \
              you to switch."
        }
    }
}

/// Append the knowledge-base section to the system prompt when the
/// conversation service mounted bases into the workspace. Rendering is
/// delegated to the shared builder
/// (`nomifun_knowledge::context::build_knowledge_context`,
/// `PromptSection` format) so nomi-engine sessions (companion companion threads
/// included) see exactly the same knowledge context the ACP path gets via
/// its preset_context — single source of truth, no more structural copies.
fn append_knowledge_context(
    base: Option<String>,
    config: &NomiBuildExtra,
    has_write_tool: bool,
) -> Option<String> {
    use nomifun_knowledge::context::{
        KnowledgeContextFormat, KnowledgeContextOptions, build_knowledge_context,
    };

    let section = build_knowledge_context(
        &config.knowledge_mounts,
        &KnowledgeContextOptions {
            format: KnowledgeContextFormat::PromptSection,
            writeback: config.knowledge_writeback,
            writeback_eagerness: config.knowledge_writeback_eagerness.as_deref(),
            has_search_tool: true,
            // The nomi engine registers the native knowledge_write tool whenever
            // the backend wired a write-back sink; the contract must then point
            // the model at that tool, not the (unreachable) generic Write path.
            has_write_tool,
        },
    );
    match (base, section) {
        (Some(ctx), Some(section)) => Some(format!("{ctx}\n\n{section}")),
        (base, None) => base,
        (None, section) => section,
    }
}

/// Standard persistent-delegation guidance for an ordinary desktop session.
pub(crate) const DELEGATION_STANDARD_HINT: &str = "遇到可并行的独立工作，或需要成体系拆解的复杂多步目标时，统一使用 `nomi_delegate`：独立工作传 `strategy=parallel` 和 tasks，复杂目标传 `strategy=planned` 和 goal，让规划器生成依赖 DAG。每个受委派的 Agent 都在右侧画布实时显示状态与转录。顶层会话委派会创建一个 Agent Execution；执行中的 Attempt 再委派只会向同一个 Execution 追加 Step，不会创建子执行。拿到 execution_id（以及追加时的 added_step_ids）后立即结束本轮，不要轮询等待或重复创建。全部结束时系统会把持久化最终结果直接作为 assistant 回执写入顶层会话，不会再启动一轮模型汇总；用户主动询问进度时才用 `nomi_execution_get` 读取一次。简单或单步问题直接作答，无需委派。";

/// Additional guidance for [`DelegationPolicy::PreferParallel`].
pub(crate) const DELEGATION_PREFER_PARALLEL_HINT: &str = "本会话偏好并行委派：面对每个请求都先明确评估能否拆成多个互相独立的 Agent 工作，并在确有并行收益时优先使用 `nomi_delegate`。只有任务确实单步可答或无法安全拆分时才直接处理；不要为了形式并行制造重复工作。";

/// 是否给本会话追加常驻 delegation 提示（纯策略，可单测）。提示点名的
/// `nomi_delegate` 工具只随进程签发的桌面网关能力提供给本地可信会话，
/// 故必须 `has_gateway` 才注入——否则会话拿不到这些工具，提示就成了空头支票（远程
/// WebUI 未授信、对外服务被钳制关网关等）。伙伴、渠道/远程和对外服务
/// 都走各自的受限能力面，故一并排除。
pub(crate) fn should_inject_delegation_hint(
    has_durable_delegation: bool,
    is_companion: bool,
    is_channel: bool,
) -> bool {
    has_durable_delegation && !is_companion && !is_channel
}

/// Which implementation owns the `nomi_delegate` tool name in this session.
///
/// One name, three contracts — and they are not interchangeable:
///
/// - [`DelegateDeployment::Gateway`] is Platform Gateway's capability surface
///   (`planned` + `parallel` + `nomi_execution_get` reads);
/// - [`DelegateDeployment::HostFacade`] is the host's **own** durable execution
///   facade (`nomifun-app::app_server_delegate`, `16` §7 决策 3). Its tool schema
///   accepts only `{strategy: "planned", goal}`;
/// - [`DelegateDeployment::Embedded`] is the in-process engine delegate
///   (`nomi_agent::local_delegate_tool`): `parallel` only, no persistence.
///
/// The distinction matters because the *prompt* must not advertise a capability
/// the session does not have. A Store session never has the Gateway (the App
/// Server ceiling clears `gateway_mcp_config`), so gating the hint on "has
/// gateway" alone would leave the Team Leader with `nomi_delegate` registered and
/// no idea it exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DelegateDeployment {
    Gateway,
    HostFacade,
    Embedded,
    None,
}

/// Exactly one deployment owns the name; this is the single place that decides
/// which. `host_facade_available` must already include the owner check and the
/// "embedded is off" half of the one-or-the-other rule, because the registration
/// site below re-derives it the same way.
pub(crate) fn delegate_deployment(
    has_platform_gateway: bool,
    install_embedded: bool,
    host_facade_available: bool,
) -> DelegateDeployment {
    if has_platform_gateway {
        DelegateDeployment::Gateway
    } else if install_embedded {
        DelegateDeployment::Embedded
    } else if host_facade_available {
        DelegateDeployment::HostFacade
    } else {
        DelegateDeployment::None
    }
}

/// Append typed persistent-delegation guidance without replacing preset,
/// persona, or knowledge context. Unavailable surfaces and
/// [`DelegationPolicy::Disabled`] preserve `base` unchanged.
pub(crate) fn compose_delegation_hint(
    base: Option<String>,
    available: bool,
    policy: DelegationPolicy,
) -> Option<String> {
    if !available || policy == DelegationPolicy::Disabled {
        return base;
    }
    let hint = match policy {
        DelegationPolicy::Automatic => DELEGATION_STANDARD_HINT.to_owned(),
        DelegationPolicy::PreferParallel => {
            format!("{DELEGATION_STANDARD_HINT}\n\n{DELEGATION_PREFER_PARALLEL_HINT}")
        }
        DelegationPolicy::Disabled => unreachable!("disabled policy returned above"),
    };
    Some(match base {
        Some(existing) if !existing.is_empty() => format!("{existing}\n\n{hint}"),
        _ => hint,
    })
}

/// Planned-only guidance for a host whose `nomi_delegate` is its own durable
/// execution facade ([`DelegateDeployment::HostFacade`]).
///
/// It deliberately does **not** reuse [`DELEGATION_STANDARD_HINT`]: that text
/// teaches `strategy=parallel` and `nomi_execution_get`, and this deployment has
/// neither. Advertising them would produce tool calls rejected by the schema.
pub(crate) const HOST_DELEGATE_STANDARD_HINT: &str = "需要成体系拆解的复杂、多步目标时，用 `nomi_delegate(strategy=\"planned\", goal=\"…\")` 把目标交给宿主规划：宿主会基于绑定的 Team 模板生成依赖 DAG，并让成员 Agent 分工执行。这个入口只接受 `goal`——成员、并发上限、规划与重规划策略都由宿主与服务端决定，不要尝试在调用里指定它们。发出调用后立刻结束本轮，不要轮询等待，也不要重复调用；规划与执行结果会由宿主写回本会话。简单或单步问题直接作答，无需委派。";

/// Append the host-facade delegation guidance without replacing preset, persona
/// or knowledge context. Unavailable surfaces and [`DelegationPolicy::Disabled`]
/// preserve `base` unchanged (same contract as [`compose_delegation_hint`]).
pub(crate) fn compose_host_delegation_hint(
    base: Option<String>,
    available: bool,
    policy: DelegationPolicy,
) -> Option<String> {
    if !available || policy == DelegationPolicy::Disabled {
        return base;
    }
    Some(match base {
        Some(existing) if !existing.is_empty() => {
            format!("{existing}\n\n{HOST_DELEGATE_STANDARD_HINT}")
        }
        _ => HOST_DELEGATE_STANDARD_HINT.to_owned(),
    })
}

/// Backend-authoritative host composition gate. It is intentionally derived
/// from resolved runtime authority plus the host's own composition decision
/// rather than user configuration: Platform Gateway owns durable Agent
/// execution, a dedicated host that owns its own execution facade opts out
/// (`AgentFactoryDeps::embedded_agent_execution`), and untrusted identities
/// never receive an embedded host execution surface.
pub(crate) fn should_install_embedded_agent_execution(
    has_platform_gateway: bool,
    is_instance_owner: bool,
    host_allows_embedded: bool,
) -> bool {
    host_allows_embedded && !has_platform_gateway && is_instance_owner
}

/// 原生文件工具（Write/Edit/ApplyPatch）的写根钳制解析（纯函数，可单测）。与
/// gateway `caps_files::file_authority` 同一信任模型:仅**本地桌面**会话
/// (无渠道平台)获得不钳制(`None` = OS 用户全权,今日行为);渠道(channel)会话
/// 一律收窄到会话工作区(`Some(workspace)`)。工作区为空时回退 `None`
/// (无从钳制则不劣于今日行为)。
pub(crate) fn resolve_native_write_root(
    channel_platform: Option<&str>,
    workspace: &str,
) -> Option<String> {
    let is_channel = channel_platform.map(str::trim).is_some_and(|s| !s.is_empty());
    if !is_channel {
        return None;
    }
    let ws = workspace.trim();
    if ws.is_empty() { None } else { Some(ws.to_owned()) }
}

/// Map Nomi DB platform name to the nomi provider identifier.
///
/// Mirrors the frontend `src/process/agent/nomi/envBuilder.ts` mapping. Pure
/// table lookup against [`platform_table::PLATFORM_CHAT_RULES`] (default row:
/// `openai`), except the new-api gateway special case: for the `new-api`
/// platform the model's per-row `protocol` override (from its
/// `provider_models` row) takes precedence over the table.
pub(crate) fn map_nomi_provider(platform: &str, protocol: Option<&str>) -> String {
    if platform == "new-api" && protocol == Some("anthropic") {
        return "anthropic".to_owned();
    }
    // Native OpenAI Responses API (previous_response_id chaining). Accept both
    // the ModelInvoke-style id and the short nomi provider id.
    if matches!(
        protocol,
        Some("openai.responses") | Some("openai-responses")
    ) {
        return "openai-responses".to_owned();
    }

    platform_table::platform_chat_rule(platform).nomi_provider.to_owned()
}

/// Resolve base_url and compat overrides for the nomi provider.
///
/// `is_full_url` bypasses every platform rule (the configured URL is the
/// request URL, minus trailing `/`, with an empty `api_path`). Otherwise the
/// platform's [`platform_table::UrlRule`] decides:
/// - `GeminiOpenAiCompat`: prepend `/v1beta/openai`, pin `api_path` to
///   `/chat/completions`
/// - `ConfiguredChatBase`: keep the configured base (nonstandard version
///   path), pin `api_path` to `/chat/completions`
/// - `StripTrailingV1` (default row): strip trailing `/v1` (nomi appends its
///   own path); OpenAI official (`api.openai.com`, mapped provider `openai`)
///   additionally sets `max_tokens_field = max_completion_tokens`
pub(crate) fn resolve_nomi_url_and_compat(
    platform: &str,
    raw_base_url: &str,
    mapped_provider: &str,
    is_full_url: bool,
) -> (Option<String>, NomiCompatOverrides) {
    let mut compat = NomiCompatOverrides::default();

    if mapped_provider == "openai-responses" {
        if is_full_url {
            let trimmed = raw_base_url.trim_end_matches('/');
            compat.api_path = Some(String::new());
            compat.max_tokens_field = Some("max_output_tokens".to_owned());
            return (Some(trimmed.to_owned()), compat);
        }
        let normalized = normalize_nomi_base_url(raw_base_url);
        compat.api_path = Some("/v1/responses".to_owned());
        compat.max_tokens_field = Some("max_output_tokens".to_owned());
        return (
            Some(normalized).filter(|u| !u.is_empty()),
            compat,
        );
    }

    if is_full_url {
        let trimmed = raw_base_url.trim_end_matches('/');
        // The configured URL already IS the request URL (`is_full_url` means
        // "base_url is the complete endpoint" — the same rule
        // `nomifun-api-types::dispatch_target` applies), so the path suffix must
        // stay empty. `nomi-providers::openai` builds
        // `format!("{base_url}{api_path}")`, so any non-empty suffix here appends
        // a SECOND `/chat/completions` to a URL that already ends with one.
        // Matches the openai-responses branch above, this function's own doc
        // comment, and the `resolve_full_url_mode_*` tests / platform snapshot in
        // this module.
        compat.api_path = Some(String::new());
        return (Some(trimmed.to_owned()), compat);
    }

    match platform_table::platform_chat_rule(platform).url_rule {
        platform_table::UrlRule::GeminiOpenAiCompat => {
            let trimmed = raw_base_url.trim_end_matches('/');
            let base = format!("{trimmed}/v1beta/openai");
            compat.api_path = Some("/chat/completions".to_owned());
            (Some(base), compat)
        }
        platform_table::UrlRule::ConfiguredChatBase => {
            let base = raw_base_url.trim_end_matches('/').to_owned();
            compat.api_path = Some("/chat/completions".to_owned());
            (Some(base).filter(|u| !u.is_empty()), compat)
        }
        platform_table::UrlRule::StripTrailingV1 => {
            let normalized = normalize_nomi_base_url(raw_base_url);
            let base_url = Some(normalized).filter(|u| !u.is_empty());

            if mapped_provider == "openai" && is_openai_host(raw_base_url) {
                compat.max_tokens_field = Some("max_completion_tokens".to_owned());
            }

            (base_url, compat)
        }
    }
}

fn is_openai_host(url: &str) -> bool {
    let lower = url.to_lowercase();
    lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .map(|rest| rest == "api.openai.com" || rest.starts_with("api.openai.com/"))
        .unwrap_or(false)
}

/// Strip trailing `/v1`, `/v1/`, or lone `/` from a base URL so that
/// nomi can append its own path suffix (`/v1/messages`, `/v1/chat/completions`).
fn normalize_nomi_base_url(url: &str) -> String {
    let trimmed = url.trim_end_matches('/');
    trimmed.strip_suffix("/v1").unwrap_or(trimmed).to_owned()
}

pub(crate) fn resolve_bedrock_config(
    json: Option<&str>,
) -> Option<nomi_config::config::BedrockConfig> {
    let bc: nomifun_api_types::BedrockConfig = serde_json::from_str(json?).ok()?;
    Some(nomi_config::config::BedrockConfig {
        region: Some(bc.region),
        access_key_id: bc.access_key_id,
        secret_access_key: bc.secret_access_key,
        session_token: None,
        profile: bc.profile,
    })
}

async fn load_user_mcp_servers(
    repo: &dyn IMcpServerRepository,
    selected_ids: Option<&[McpServerId]>,
    conversation_id: &str,
    oauth: Option<&McpOAuthService>,
) -> HashMap<String, McpServerConfig> {
    let rows_result = match selected_ids {
        Some(ids) => {
            let ids = ids.iter().map(ToString::to_string).collect::<Vec<_>>();
            repo.list_by_ids_any(&ids).await
        }
        None => repo.list().await,
    };
    let rows = match rows_result {
        Ok(r) => r,
        Err(err) => {
            warn!(
                conversation_id,
                error = %err,
                "user_mcp: list() failed; skipping injection"
            );
            return HashMap::new();
        }
    };

    let mut servers = HashMap::new();
    for row in rows {
        if !should_load_user_mcp_row(&row, selected_ids) {
            continue;
        }

        match row_to_mcp_server_config(&row) {
            Ok(mut config) => {
                // Request-time OAuth bearer injection for remote transports
                // (stdio servers carry no URL; user-configured Authorization
                // headers win). A missing token leaves the header untouched —
                // the engine's 401-refresh path covers expiry at call time.
                if let Some(url) = config.url.clone()
                    && let Some(headers) = config.headers.as_mut()
                {
                    if let Err(error) = inject_oauth_bearer(oauth, &url, headers).await {
                        warn!(
                            conversation_id,
                            mcp_server_id = %row.mcp_server_id,
                            server_name = %row.name,
                            %error,
                            "user_mcp: oauth token lookup failed; continuing without injection"
                        );
                    }
                }
                servers.insert(row.name.clone(), config);
            }
            Err(err) => {
                warn!(
                    conversation_id,
                    mcp_server_id = %row.mcp_server_id,
                    server_name = %row.name,
                    error = %err,
                    "user_mcp: failed to convert row; skipping"
                );
            }
        }
    }

    servers
}

/// Merge the host's `mcp.json` declarations into this session's extra servers
/// (`20` §7.9 / `21` D14).
///
/// Called **before** the `mcp_servers` rows load, because that loop uses
/// `entry().or_insert()` (first writer wins): the declaration file is the
/// operator's explicit intent, while a row is usually the residue of an import.
/// A name a request-level binding already took is left alone — that caller asked
/// for *that* server on this run.
///
/// `secret:NAME` references resolve here, at session build time, so a credential
/// installed in-process is honoured exactly as it is for a row. HTTP headers are
/// resolved too: the declaration contract points header credentials at
/// `secret:NAME` (`20` §7.9), and a literal value simply passes through.
///
/// Returns early unless `is_instance_owner`: a declaration is a **host
/// capability** — a stdio server runs a local process and a remote one can carry
/// credentials — and a non-owner principal only ever gets a plain Nomi
/// conversation (see `docs/architecture/data-and-storage.zh.md` §安装级执行权限).
///
/// `pub(crate)` so the manager-level session test can drive the real merge
/// instead of a copy of it (`manager::nomi::agent::tests`).
pub(crate) fn merge_host_declared_mcp_servers(
    extra_mcp_servers: &mut HashMap<String, McpServerConfig>,
    declarations: &NomiMcpDeclarations,
    conversation_id: &str,
    is_instance_owner: bool,
) {
    if !is_instance_owner {
        return;
    }
    for declared in declarations.enabled_servers() {
        if extra_mcp_servers.contains_key(&declared.name) {
            continue;
        }
        let startup_timeout_secs = declared.startup_timeout_secs;
        let enabled_tools = declared.enabled_tools.clone();
        let disabled_tools = declared.disabled_tools.clone();
        let config = match &declared.transport {
            McpTransport::Stdio { command, args, env } => {
                let resolved = nomifun_common::secret_ref::resolve_env(env);
                report_missing_credentials(
                    conversation_id,
                    &declared.name,
                    "env",
                    &resolved.missing,
                );
                McpServerConfig {
                    transport: TransportType::Stdio,
                    command: Some(command.clone()),
                    args: Some(args.clone()),
                    env: Some(resolved.env),
                    url: None,
                    headers: None,
                    // Eager schemas, matching how a `mcp_servers` row is mapped.
                    deferred: Some(false),
                    request_timeout_secs: declared.request_timeout_secs,
                    startup_timeout_secs,
                    // Already absolute: the host resolved it against the
                    // declaration file when the file was loaded.
                    cwd: declared.cwd.clone(),
                    enabled_tools,
                    disabled_tools,
                }
            }
            McpTransport::Sse { url, headers } => {
                let headers = declared_remote_headers(
                    conversation_id,
                    &declared.name,
                    headers,
                    declared.bearer_token_env_var.as_deref(),
                );
                McpServerConfig {
                    transport: TransportType::Sse,
                    command: None,
                    args: None,
                    env: None,
                    url: Some(url.clone()),
                    headers: Some(headers),
                    deferred: Some(false),
                    request_timeout_secs: declared.request_timeout_secs,
                    startup_timeout_secs,
                    cwd: None,
                    enabled_tools,
                    disabled_tools,
                }
            }
            McpTransport::Http { url, headers } => {
                let headers = declared_remote_headers(
                    conversation_id,
                    &declared.name,
                    headers,
                    declared.bearer_token_env_var.as_deref(),
                );
                McpServerConfig {
                    transport: TransportType::StreamableHttp,
                    command: None,
                    args: None,
                    env: None,
                    url: Some(url.clone()),
                    headers: Some(headers),
                    deferred: Some(false),
                    request_timeout_secs: declared.request_timeout_secs,
                    startup_timeout_secs,
                    cwd: None,
                    enabled_tools,
                    disabled_tools,
                }
            }
        };
        extra_mcp_servers.insert(declared.name.clone(), config);
    }
}

/// The header map a declared remote server connects with: `secret:<NAME>`
/// references resolved in memory, then the optional `bearerTokenEnvVar` turned
/// into an `Authorization` header.
fn declared_remote_headers(
    conversation_id: &str,
    server_name: &str,
    headers: &HashMap<String, String>,
    bearer_token_env_var: Option<&str>,
) -> HashMap<String, String> {
    let mut headers = resolve_header_secrets(Some(conversation_id), server_name, headers);
    if bearer_token_env_var.is_some() {
        // Cloned only when a bearer token was actually declared; the
        // `resolve_header_secrets` call above already clones the same map once.
        let credentials = nomifun_common::secret_ref::credentials();
        apply_bearer_token(
            conversation_id,
            server_name,
            bearer_token_env_var,
            &mut headers,
            &credentials,
        );
    }
    headers
}

/// Turn `bearerTokenEnvVar` into `Authorization: Bearer <value>`.
///
/// The field names the variable rather than carrying the token, so the token is
/// still never persisted — the same rule as `secret:<NAME>`, with the lookup
/// precedence owned by `nomifun_common::secret_ref` (`config.toml [credentials]`
/// first, process environment as the fallback).
///
/// An explicitly declared `Authorization` header wins, because the declaration
/// said so literally. A name that resolves to nothing omits the header and says
/// which name failed — never a literal `Bearer <name>` and never an empty value.
///
/// The credential map is a parameter rather than a global read so both rules are
/// assertable without mutating the process-wide store.
fn apply_bearer_token(
    conversation_id: &str,
    server_name: &str,
    bearer_token_env_var: Option<&str>,
    headers: &mut HashMap<String, String>,
    credentials: &HashMap<String, String>,
) {
    let Some(name) = bearer_token_env_var else {
        return;
    };
    if headers.contains_key("Authorization") {
        warn!(
            conversation_id,
            server_name,
            env_var = name,
            "host_mcp: declaration sets both `headers.Authorization` and \
             `bearerTokenEnvVar`; the explicit header wins"
        );
        return;
    }
    match nomifun_common::secret_ref::lookup_with(name, credentials) {
        Some(token) => {
            headers.insert("Authorization".to_owned(), format!("Bearer {token}"));
        }
        None => {
            warn!(
                conversation_id,
                server_name,
                env_var = name,
                "host_mcp: `bearerTokenEnvVar` names a variable that is neither in \
                 `[credentials]` nor in the process environment; sending no Authorization header"
            );
        }
    }
}

/// Resolve `secret:<NAME>` references in a header map exactly as the env map is
/// resolved, and report the shapes that *look* like a reference but are not one.
///
/// The DB-row and session-snapshot paths used to hand headers to the engine
/// verbatim while resolving `env`, so a reference written into a header — the
/// natural way to supply a bearer token without persisting it — was sent as the
/// literal text and authentication failed with no local signal. All three paths
/// (DB row, session snapshot, `mcp.json` declaration) now go through here.
///
/// A reference has to be the **whole** value (`secret_ref::parse_secret_ref`),
/// so `Authorization: Bearer secret:TOKEN` is not one; that shape is the likeliest
/// mistake and is therefore named in a warning. The header *name* is logged, never
/// the value.
fn resolve_header_secrets(
    conversation_id: Option<&str>,
    server_name: &str,
    headers: &HashMap<String, String>,
) -> HashMap<String, String> {
    let resolved = nomifun_common::secret_ref::resolve_env(headers);
    if !resolved.missing.is_empty() {
        match conversation_id {
            Some(conversation_id) => {
                report_missing_credentials(
                    conversation_id,
                    server_name,
                    "headers",
                    &resolved.missing,
                );
            }
            None => warn!(
                server_name,
                field = "headers",
                missing = ?resolved.missing,
                "host_mcp: unresolved credential references; omitting them"
            ),
        }
    }
    for (name, value) in headers {
        let is_a_reference =
            nomifun_common::secret_ref::parse_secret_ref(value).is_some();
        if !is_a_reference && value.contains(nomifun_common::secret_ref::SECRET_PREFIX) {
            warn!(
                server_name,
                header = %name,
                "host_mcp: a header value contains `secret:` but is not exactly a \
                 `secret:<NAME>` reference, so it is sent literally; use the whole value as the \
                 reference, or `bearerTokenEnvVar` for an `Authorization: Bearer <token>` header"
            );
        }
    }
    resolved.env
}

/// The name of an unresolved reference is not a secret, so it is safe to log;
/// the value never was available. The variable is omitted rather than sent as
/// the literal `secret:NAME`.
fn report_missing_credentials(
    conversation_id: &str,
    server_name: &str,
    field: &str,
    missing: &[String],
) {
    if !missing.is_empty() {
        warn!(
            conversation_id,
            server_name,
            field,
            missing = ?missing,
            "host_mcp: unresolved credential references; omitting them"
        );
    }
}

fn should_load_user_mcp_row(row: &McpServerRow, selected_ids: Option<&[McpServerId]>) -> bool {
    row.enabled
        && !row.builtin
        && selected_ids
            .map(|ids| ids.iter().any(|id| id.as_str() == row.mcp_server_id))
            .unwrap_or(true)
}

fn row_to_mcp_server_config(row: &McpServerRow) -> Result<McpServerConfig, String> {
    let value: serde_json::Value = serde_json::from_str(&row.transport_config)
        .map_err(|e| format!("invalid transport_config JSON: {e}"))?;

    match row.transport_type.as_str() {
        "stdio" => {
            let command = value
                .get("command")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "stdio: missing command".to_owned())?;
            let resolved_command = resolve_stdio_command(command);
            let args = value
                .get("args")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(ToOwned::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            let env = value
                .get("env")
                .and_then(|v| v.as_object())
                .map(|obj| {
                    obj.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                        .collect::<HashMap<_, _>>()
                })
                .unwrap_or_default();
            // `17` §6 / `21` D5=C: the persisted row holds `secret:NAME`
            // references, never values. Resolve them in memory right before the
            // child is spawned; a reference with no credential is omitted.
            let env = nomifun_common::secret_ref::resolve_env(&env);
            if !env.missing.is_empty() {
                warn!(
                    mcp_server_id = %row.mcp_server_id,
                    server_name = %row.name,
                    missing = ?env.missing,
                    "user_mcp: unresolved credential references; omitting them"
                );
            }

            Ok(McpServerConfig {
                transport: TransportType::Stdio,
                command: Some(resolved_command),
                args: Some(args),
                env: Some(env.env),
                url: None,
                headers: None,
                deferred: Some(false),
                request_timeout_secs: None,
                startup_timeout_secs: None,
                cwd: None,
                enabled_tools: None,
                disabled_tools: None,
            })
        }
        "http" | "streamable_http" => {
            let url = value
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "http: missing url".to_owned())?;
            let headers = value
                .get("headers")
                .and_then(|v| v.as_object())
                .map(|obj| {
                    obj.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                        .collect::<HashMap<_, _>>()
                })
                .unwrap_or_default();
            let headers = resolve_header_secrets(None, &row.name, &headers);

            Ok(McpServerConfig {
                transport: TransportType::StreamableHttp,
                command: None,
                args: None,
                env: None,
                url: Some(url.to_owned()),
                headers: Some(headers),
                deferred: Some(false),
                request_timeout_secs: None,
                startup_timeout_secs: None,
                cwd: None,
                enabled_tools: None,
                disabled_tools: None,
            })
        }
        "sse" => {
            let url = value
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "sse: missing url".to_owned())?;
            let headers = value
                .get("headers")
                .and_then(|v| v.as_object())
                .map(|obj| {
                    obj.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                        .collect::<HashMap<_, _>>()
                })
                .unwrap_or_default();
            let headers = resolve_header_secrets(None, &row.name, &headers);

            Ok(McpServerConfig {
                transport: TransportType::Sse,
                command: None,
                args: None,
                env: None,
                url: Some(url.to_owned()),
                headers: Some(headers),
                deferred: Some(false),
                request_timeout_secs: None,
                startup_timeout_secs: None,
                cwd: None,
                enabled_tools: None,
                disabled_tools: None,
            })
        }
        other => Err(format!("unsupported transport_type: {other}")),
    }
}

fn session_server_to_mcp_server_config(
    server: &SessionMcpServer,
) -> Result<McpServerConfig, String> {
    match &server.transport {
        SessionMcpTransport::Stdio { command, args, env } => {
            if command.is_empty() {
                return Err("stdio: missing command".to_owned());
            }
            // `17` §6 / `21` D5=C: resolve `secret:NAME` references from a
            // session-carried snapshot just as for a persisted row.
            let resolved = nomifun_common::secret_ref::resolve_env(env);
            if !resolved.missing.is_empty() {
                warn!(
                    server_name = %server.name,
                    missing = ?resolved.missing,
                    "user_mcp: unresolved credential references; omitting them"
                );
            }
            Ok(McpServerConfig {
                transport: TransportType::Stdio,
                command: Some(resolve_stdio_command(command)),
                args: Some(args.clone()),
                env: Some(resolved.env),
                url: None,
                headers: None,
                deferred: Some(false),
                request_timeout_secs: None,
                startup_timeout_secs: None,
                cwd: None,
                enabled_tools: None,
                disabled_tools: None,
            })
        }
        SessionMcpTransport::Http { url, headers } => {
            if url.is_empty() {
                return Err("http: missing url".to_owned());
            }
            let headers = resolve_header_secrets(None, &server.name, headers);
            Ok(McpServerConfig {
                transport: TransportType::StreamableHttp,
                command: None,
                args: None,
                env: None,
                url: Some(url.clone()),
                headers: Some(headers),
                deferred: Some(false),
                request_timeout_secs: None,
                startup_timeout_secs: None,
                cwd: None,
                enabled_tools: None,
                disabled_tools: None,
            })
        }
        SessionMcpTransport::Sse { url, headers } => {
            if url.is_empty() {
                return Err("sse: missing url".to_owned());
            }
            let headers = resolve_header_secrets(None, &server.name, headers);
            Ok(McpServerConfig {
                transport: TransportType::Sse,
                command: None,
                args: None,
                env: None,
                url: Some(url.clone()),
                headers: Some(headers),
                deferred: Some(false),
                request_timeout_secs: None,
                startup_timeout_secs: None,
                cwd: None,
                enabled_tools: None,
                disabled_tools: None,
            })
        }
        SessionMcpTransport::StreamableHttp { url, headers } => {
            if url.is_empty() {
                return Err("streamable_http: missing url".to_owned());
            }
            let headers = resolve_header_secrets(None, &server.name, headers);
            Ok(McpServerConfig {
                transport: TransportType::StreamableHttp,
                command: None,
                args: None,
                env: None,
                url: Some(url.clone()),
                headers: Some(headers),
                deferred: Some(false),
                request_timeout_secs: None,
                startup_timeout_secs: None,
                cwd: None,
                enabled_tools: None,
                disabled_tools: None,
            })
        }
    }
}

async fn merge_session_snapshot_mcp_servers(
    extra_mcp_servers: &mut HashMap<String, McpServerConfig>,
    session_mcp_servers: &[SessionMcpServer],
    conversation_id: &str,
    oauth: Option<&McpOAuthService>,
) {
    for server in session_mcp_servers {
        match session_server_to_mcp_server_config(server) {
            Ok(mut config) => {
                if let Some(url) = config.url.clone()
                    && let Some(headers) = config.headers.as_mut()
                {
                    if let Err(error) = inject_oauth_bearer(oauth, &url, headers).await {
                        warn!(
                            conversation_id = %conversation_id,
                            mcp_server_id = %server.mcp_server_id,
                            server_name = %server.name,
                            %error,
                            "session_mcp: oauth token lookup failed; continuing without injection"
                        );
                    }
                }
                if extra_mcp_servers
                    .insert(server.name.clone(), config)
                    .is_some()
                {
                    debug!(
                        conversation_id = %conversation_id,
                        server_name = %server.name,
                        "session_mcp: session snapshot overrides repo-backed MCP config"
                    );
                }
            }
            Err(err) => {
                warn!(
                    conversation_id = %conversation_id,
                    mcp_server_id = %server.mcp_server_id,
                    server_name = %server.name,
                    error = %err,
                    "session_mcp: failed to convert session snapshot; skipping"
                );
            }
        }
    }
}

fn resolve_stdio_command(command: &str) -> String {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return command.to_owned();
    }

    let path = std::path::Path::new(trimmed);
    if path.is_absolute()
        || trimmed.contains(std::path::MAIN_SEPARATOR)
        || trimmed.contains('/')
        || trimmed.contains('\\')
    {
        return trimmed.to_owned();
    }

    resolve_command_path(trimmed)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| trimmed.to_owned())
}

fn resolve_mcp_servers(
    overrides: &NomiBuildExtra,
    conversation_id: &str,
) -> (HashMap<String, McpServerConfig>, LoopbackCapabilityLeaseSet) {
    let mut servers = HashMap::new();
    let mut leases = LoopbackCapabilityLeaseSet::new();
    // Presence of the process-owned config is the capability grant.
    if let Some(gw_cfg) = &overrides.gateway_mcp_config {
        if let Some((name, server, lease)) =
            gateway_mcp_to_config(gw_cfg, overrides, conversation_id)
        {
            servers.insert(name, server);
            leases.push(lease);
        }
    }
    (servers, leases)
}

fn resolved_session_mode(overrides: &NomiBuildExtra) -> String {
    overrides
        .session_mode
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("yolo")
        .to_owned()
}

/// Platform Gateway MCP stdio bridge config for the Nomi engine, mirroring the
/// ACP assembler's `gateway_mcp_server`. Caller conversation + user ids ride
/// along for self-protection and data scoping; the companion binding (when present)
/// rides along for attribution.
fn gateway_mcp_to_config(
    cfg: &GatewayMcpConfig,
    overrides: &NomiBuildExtra,
    conversation_id: &str,
) -> Option<(String, McpServerConfig, LoopbackCapabilityLease)> {
    let session_mode = resolved_session_mode(overrides);
    let Some(user_id) = overrides.user_id.as_deref() else {
        warn!(conversation_id, "gateway MCP capability issuance requires a user ID");
        return None;
    };
    let child = match cfg.issue_for_conversation(
        user_id,
        conversation_id,
        overrides.companion_id.as_deref(),
        overrides.channel_platform.as_deref(),
        Some(&session_mode),
        &overrides.gateway_excluded_tools,
    ) {
        Ok(child) => child,
        Err(error) => {
            warn!(%error, conversation_id, "gateway MCP capability issuance failed closed");
            return None;
        }
    };
    let mut env = HashMap::new();
    env.insert(
        GatewayMcpConfig::ENV_CAPABILITY.into(),
        child
            .bootstrap_json()
            .expect("validated gateway bootstrap serializes"),
    );

    let server = McpServerConfig {
        transport: TransportType::Stdio,
        command: Some(child.binary_path),
        args: Some(vec!["mcp-gateway-stdio".into()]),
        env: Some(env),
        url: None,
        headers: None,
        deferred: Some(true),
        request_timeout_secs: None,
        startup_timeout_secs: None,
        cwd: None,
        enabled_tools: None,
        disabled_tools: None,
    };

    Some((
        GatewayMcpConfig::SERVER_NAME.to_owned(),
        server,
        child.lease,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferred_image_analysis_model_matches_flowy_catalog_id() {
        assert!(is_preferred_image_analysis_model("MiniMax-M3"));
        assert!(is_preferred_image_analysis_model("AIPC-Minimax-M3"));
        assert!(is_preferred_image_analysis_model("aipc-minimax-m3"));
        assert!(!is_preferred_image_analysis_model("AIPC-GPT5.5"));
        assert!(!is_preferred_image_analysis_model("MiniMax-M2.7"));
    }

    #[test]
    fn image_analysis_auto_fallback_skips_managed_free_models() {
        assert!(!is_image_analysis_eligible_provider("nomifun-free-model"));
        assert!(is_image_analysis_eligible_provider("openai"));
        assert!(is_image_analysis_eligible_provider("minimax"));
    }

    #[test]
    fn resolve_session_reasoning_effort_requires_catalog_allowlist() {
        let levels = ["low".to_owned(), "medium".to_owned(), "xhigh".to_owned()];
        assert_eq!(
            resolve_session_reasoning_effort(Some("xhigh"), Some(&levels)).as_deref(),
            Some("xhigh")
        );
        // Invalid selection falls back to medium when advertised.
        assert_eq!(
            resolve_session_reasoning_effort(Some("high"), Some(&levels)).as_deref(),
            Some("medium")
        );
        assert_eq!(
            resolve_session_reasoning_effort(None, Some(&levels)).as_deref(),
            Some("medium")
        );
        assert_eq!(
            resolve_session_reasoning_effort(Some("medium"), None),
            None
        );
        let low_only = ["low".to_owned(), "xhigh".to_owned()];
        assert_eq!(
            resolve_session_reasoning_effort(None, Some(&low_only)).as_deref(),
            Some("low")
        );
    }

    #[test]
    fn bool_pref_parse_matches_the_boot_time_reader_semantics() {
        // Fail-open default-ON keys (persistentLogin): only an explicit
        // "false" (bare or JSON-quoted) turns them off; junk keeps the default.
        for on in ["true", "\"true\"", "yes", "\"yes\"", "", "junk"] {
            assert!(parse_bool_pref(on, true), "{on:?} must keep a default-ON toggle on");
        }
        assert!(!parse_bool_pref("false", true));
        assert!(!parse_bool_pref("\"false\"", true));
        assert!(!parse_bool_pref("  \"false\"  ", true));

        // Fail-closed default-OFF keys (fullPower): only an explicit "true"
        // (bare or JSON-quoted) turns them on; junk keeps them off.
        for off in ["false", "\"false\"", "yes", "\"yes\"", "", "junk"] {
            assert!(!parse_bool_pref(off, false), "{off:?} must keep a default-OFF toggle off");
        }
        assert!(parse_bool_pref("true", false));
        assert!(parse_bool_pref("\"true\"", false));
        assert!(parse_bool_pref("  \"true\"  ", false));
    }

    #[test]
    fn disabled_user_mcp_never_passes_the_runtime_gate() {
        let mut row = McpServerRow {
            mcp_server_id: "0190f5fe-7c00-7a00-8000-000000000001".to_owned(),
            name: "disabled".to_owned(),
            description: None,
            enabled: false,
            transport_type: "stdio".to_owned(),
            transport_config: r#"{"command":"npx"}"#.to_owned(),
            tools: None,
            last_test_status: "connected".to_owned(),
            last_connected: Some(1),
            original_json: None,
            builtin: false,
            deleted_at: None,
            created_at: 1,
            updated_at: 1,
        };
        let selected = [McpServerId::parse(row.mcp_server_id.clone()).unwrap()];

        assert!(!should_load_user_mcp_row(&row, Some(&selected)));
        assert!(!should_load_user_mcp_row(&row, None));

        row.enabled = true;
        assert!(should_load_user_mcp_row(&row, Some(&selected)));
    }

    fn gateway_config(port: u16, binary: &str, owner: &str) -> GatewayMcpConfig {
        GatewayMcpConfig::from_issuer(
            port,
            Arc::new(nomifun_common::LoopbackCapabilityIssuer::random().unwrap()),
            binary.into(),
            Arc::<str>::from(owner),
        )
    }

    #[test]
    fn app_server_chat_ceiling_clears_integrations_and_normalises_the_connector_fence() {
        let mcp_server_id = McpServerId::new();
        let mut overrides = NomiBuildExtra {
            // Absent on purpose: the ceiling must turn it into an explicit empty
            // fence, because `None` would bind every enabled host MCP server.
            mcp_server_ids: None,
            session_mcp_servers: vec![SessionMcpServer {
                mcp_server_id,
                name: "test-mcp".into(),
                transport: SessionMcpTransport::Stdio {
                    command: "server".into(),
                    args: Vec::new(),
                    env: Default::default(),
                },
            }],
            summon: Some(nomifun_api_types::SummonConfig {
                companion_id: "0190f5fe-7c00-7a00-8abc-012345678969".into(),
                memory_ids: vec![],
                skill_exclusions: vec![],
                summoned_at: 1,
            }),
            delegation_policy: DelegationPolicy::Automatic,
            ..Default::default()
        };

        apply_app_server_chat_ceiling(&mut overrides);

        assert!(overrides.gateway_mcp_config.is_none());
        // The fence, not `None`: `None` would mean "bind every enabled host MCP
        // server" (see `load_user_mcp_servers`), which is the opposite of intent.
        assert_eq!(
            overrides.mcp_server_ids,
            Some(Vec::new()),
            "an App Server chat with no bound Connector must bind none, not all"
        );
        assert!(overrides.session_mcp_servers.is_empty());
        assert!(overrides.summon.is_none());
        assert_eq!(
            overrides.delegation_policy,
            DelegationPolicy::Automatic,
            "the ceiling must not clamp the Conversation's own typed delegation tier"
        );
    }

    /// The delegation tier is the create seam's decision, never the ceiling's:
    /// whether a Store chat may delegate is already fully expressed by the
    /// Conversation row the trusted seam wrote, and the ceiling must leave it
    /// alone in *both* directions.
    #[test]
    fn app_server_chat_ceiling_leaves_the_delegation_tier_untouched() {
        let mut disabled = NomiBuildExtra {
            delegation_policy: DelegationPolicy::Disabled,
            ..Default::default()
        };
        apply_app_server_chat_ceiling(&mut disabled);
        assert_eq!(disabled.delegation_policy, DelegationPolicy::Disabled);

        let mut prefer_parallel = NomiBuildExtra {
            delegation_policy: DelegationPolicy::PreferParallel,
            ..Default::default()
        };
        apply_app_server_chat_ceiling(&mut prefer_parallel);
        assert_eq!(
            prefer_parallel.delegation_policy,
            DelegationPolicy::PreferParallel,
            "a Team Leader tier must survive the ceiling"
        );
    }

    /// Definition-bound Connectors must survive the ceiling: it may only add the
    /// empty fence when the key is absent, never overwrite a real binding.
    #[test]
    fn app_server_chat_ceiling_keeps_bound_connectors() {
        let bound = McpServerId::new();
        let mut overrides = NomiBuildExtra {
            mcp_server_ids: Some(vec![bound.clone()]),
            ..Default::default()
        };

        apply_app_server_chat_ceiling(&mut overrides);

        assert_eq!(overrides.mcp_server_ids, Some(vec![bound]));
    }

    /// A session that opts into everything the host policy is able to remove.
    fn fully_opted_in_session() -> NomiBuildExtra {
        NomiBuildExtra {
            computer_use: Some(true),
            browser_use: Some(true),
            companion: true,
            companion_id: Some("0190f5fe-7c00-7a00-8abc-012345678969".into()),
            summon: Some(nomifun_api_types::SummonConfig {
                companion_id: "0190f5fe-7c00-7a00-8abc-012345678969".into(),
                memory_ids: vec![],
                skill_exclusions: vec![],
                summoned_at: 1,
            }),
            knowledge_mounts: vec![nomifun_api_types::KnowledgeMountInfo {
                knowledge_base_id: nomifun_common::KnowledgeBaseId::new(),
                name: "test knowledge".into(),
                description: "mount".into(),
                rel_path: ".flowy/knowledge/test".into(),
                toc: Vec::new(),
                summary: None,
                live_sources: Vec::new(),
            }],
            knowledge_writeback: true,
            knowledge_channel_write_enabled: true,
            goal: Some(nomifun_api_types::NomiGoalSpec {
                objective: "finish the task".into(),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn permissive_host_policy_changes_nothing() {
        let mut overrides = fully_opted_in_session();
        apply_host_tool_policy(&mut overrides, &NomiToolPolicy::default());

        assert_eq!(overrides.computer_use, Some(true));
        assert_eq!(overrides.browser_use, Some(true));
        assert!(overrides.companion && overrides.companion_id.is_some());
        assert!(overrides.summon.is_some());
        assert_eq!(overrides.knowledge_mounts.len(), 1);
        assert!(overrides.knowledge_writeback && overrides.knowledge_channel_write_enabled);
        assert!(overrides.goal.is_some());
    }

    /// The host policy subtracts exactly the families it names and leaves every
    /// other opt-in alone — it is not a ceiling that resets the session.
    #[test]
    fn host_tool_policy_subtracts_only_the_named_families() {
        use nomifun_api_types::NomiToolDomains;

        let mut overrides = fully_opted_in_session();
        apply_host_tool_policy(
            &mut overrides,
            &NomiToolPolicy {
                computer: false,
                browser: false,
                domains: NomiToolDomains {
                    companion: false,
                    knowledge: false,
                    goal: false,
                    // Deliberately left on: these must survive untouched.
                    cron: true,
                    meeting: true,
                    learning: true,
                    media: true,
                    requirement: true,
                },
                ..NomiToolPolicy::default()
            },
        );

        // `Some(false)` (not `None`) is what defeats the coding-profile branch in
        // the computer/browser resolution further down.
        assert_eq!(overrides.computer_use, Some(false));
        assert_eq!(overrides.browser_use, Some(false));
        assert!(
            !overrides.companion && overrides.companion_id.is_none(),
            "without the companion domain there is no binding to host"
        );
        assert!(overrides.summon.is_none());
        assert!(overrides.knowledge_mounts.is_empty());
        assert!(!overrides.knowledge_writeback && !overrides.knowledge_channel_write_enabled);
        assert!(overrides.goal.is_none());
    }

    /// `web` / `plan` / `lsp` are deliberately *not* handled here: the engine owns
    /// them through its own config file, and the manager applies them after
    /// `Config::resolve`. Turning them off in the policy must not touch the
    /// session's build extra.
    #[test]
    fn engine_owned_switches_are_left_to_the_manager() {
        let mut overrides = fully_opted_in_session();
        let before = overrides.clone();
        apply_host_tool_policy(
            &mut overrides,
            &NomiToolPolicy {
                web: false,
                plan: false,
                lsp: false,
                ..NomiToolPolicy::default()
            },
        );

        assert_eq!(overrides.computer_use, before.computer_use);
        assert_eq!(overrides.browser_use, before.browser_use);
        assert_eq!(overrides.companion, before.companion);
        assert_eq!(overrides.goal.is_some(), before.goal.is_some());
    }

    #[test]
    fn secondary_nomi_session_is_model_only() {        let mcp_server_id = McpServerId::new();
        let mut overrides = NomiBuildExtra {
            computer_use: Some(true),
            browser_use: Some(true),
            mcp_server_ids: Some(vec![mcp_server_id.clone()]),
            session_mcp_servers: vec![SessionMcpServer {
                mcp_server_id,
                name: "test-mcp".into(),
                transport: SessionMcpTransport::Stdio {
                    command: "server".into(),
                    args: Vec::new(),
                    env: Default::default(),
                },
            }],
            companion: true,
            companion_id: Some("0190f5fe-7c00-7a00-8abc-012345678967".into()),
            knowledge_mounts: vec![nomifun_api_types::KnowledgeMountInfo {
                knowledge_base_id: nomifun_common::KnowledgeBaseId::new(),
                name: "test knowledge".into(),
                description: "test mount removed by model-only ceiling".into(),
                rel_path: ".flowy/knowledge/test".into(),
                toc: Vec::new(),
                summary: None,
                live_sources: Vec::new(),
            }],
            knowledge_writeback: true,
            knowledge_channel_write_enabled: true,
            summon: Some(nomifun_api_types::SummonConfig {
                companion_id: "0190f5fe-7c00-7a00-8abc-012345678969".into(),
                memory_ids: vec![],
                skill_exclusions: vec![],
                summoned_at: 1,
            }),
            ..Default::default()
        };

        apply_model_only_ceiling(&mut overrides);

        assert!(overrides.gateway_mcp_config.is_none());
        assert_eq!(overrides.computer_use, Some(false));
        assert_eq!(overrides.browser_use, Some(false));
        assert!(overrides.mcp_server_ids.is_none());
        assert!(overrides.session_mcp_servers.is_empty());
        assert!(!overrides.companion && overrides.companion_id.is_none());
        assert!(overrides.knowledge_mounts.is_empty());
        assert!(!overrides.knowledge_writeback);
        assert!(!overrides.knowledge_channel_write_enabled);
        assert!(
            overrides.summon.is_none(),
            "summon loads local companion memories/skills — owner only"
        );
        assert_eq!(overrides.allowed_tools, vec!["update_plan"]);
        assert_eq!(overrides.session_mode.as_deref(), Some("default"));
        assert_eq!(overrides.max_turns, Some(1));
        assert_eq!(overrides.delegation_policy, DelegationPolicy::Disabled);
    }

    #[test]
    fn resumed_session_metadata_tracks_each_provider_switch() {
        let now = chrono::Utc::now();
        let mut session = Session {
            id: "provider-switch".into(),
            created_at: now,
            updated_at: now,
            provider: "provider-a".into(),
            model: "model-a".into(),
            cwd: "/workspace".into(),
            total_usage: Default::default(),
            messages: Vec::new(),
            owner_token: None,
            activated_deferred_tools: Vec::new(),
            editable_turn: None,
            last_turn_ended_at: None,
        };

        assert!(retarget_resumed_session(
            &mut session,
            "provider-b",
            "model-b"
        ));
        assert_eq!(session.provider, "provider-b");
        assert_eq!(session.model, "model-b");
        assert!(retarget_resumed_session(
            &mut session,
            "provider-a",
            "model-a2"
        ));
        assert_eq!(session.provider, "provider-a");
        assert_eq!(session.model, "model-a2");
    }

    #[test]
    fn session_sanitizer_remaps_the_editable_turn_boundary() {
        use nomi_agent::session::EditableTurnCheckpoint;
        use nomi_types::message::{ContentBlock, Message, Role};

        let now = chrono::Utc::now();
        let mut session = Session {
            id: "rewind-boundary".into(),
            created_at: now,
            updated_at: now,
            provider: "provider-a".into(),
            model: "model-a".into(),
            cwd: "/workspace".into(),
            total_usage: Default::default(),
            messages: vec![
                Message::new(
                    Role::User,
                    vec![ContentBlock::Text {
                        text: "stable history".into(),
                    }],
                ),
                Message::new(
                    Role::Assistant,
                    vec![ContentBlock::Text {
                        text: String::new(),
                    }],
                ),
                Message::new(
                    Role::User,
                    vec![ContentBlock::Text {
                        text: "editable root".into(),
                    }],
                ),
            ],
            owner_token: None,
            activated_deferred_tools: Vec::new(),
            editable_turn: Some(EditableTurnCheckpoint {
                source_message_id: "message-root".into(),
                start_len: 2,
            }),
            last_turn_ended_at: None,
        };

        let repair = sanitize_resumed_session(&mut session, false);

        assert_eq!(repair.removed_messages, 1);
        assert_eq!(session.messages.len(), 2);
        assert_eq!(
            session
                .editable_turn
                .as_ref()
                .map(|checkpoint| checkpoint.start_len),
            Some(1)
        );
    }

    #[test]
    fn resolved_fallback_metadata_is_persisted_to_session_and_index() {
        let directory = tempfile::tempdir().unwrap();
        let manager = SessionManager::new(directory.path().to_path_buf(), 10);
        let mut session = manager
            .create("deleted-provider", "stale-model", "/workspace", Some("fallback"))
            .unwrap();

        retarget_resumed_session(&mut session, "resolved-provider", "resolved-fallback-model");
        persist_repaired_session(&manager, &session).unwrap();

        let reloaded = manager.load("fallback").unwrap();
        assert_eq!(reloaded.provider, "resolved-provider");
        assert_eq!(reloaded.model, "resolved-fallback-model");
        let metadata = manager
            .list()
            .unwrap()
            .into_iter()
            .find(|entry| entry.id == "fallback")
            .unwrap();
        assert_eq!(metadata.model, "resolved-fallback-model");
    }

    // ----- output-language directive (thinking + reply follow system language) -----

    #[test]
    fn output_language_directive_maps_supported_and_defaults_to_english() {
        // zh-CN steers BOTH reply and thinking to Simplified Chinese.
        let zh = output_language_directive("zh-CN");
        assert!(zh.contains("简体中文"));
        assert!(zh.contains("思考"), "zh directive must cover the thinking process: {zh}");
        // en-US, unknown codes, and the empty string all resolve to English.
        for lang in ["en-US", "fr-FR", "zh-TW", ""] {
            let d = output_language_directive(lang);
            assert!(
                d.contains("in English"),
                "{lang} should map to English: {d}"
            );
            assert!(
                d.contains("think"),
                "{lang} directive must cover the thinking process: {d}"
            );
            assert!(!d.contains("简体中文"), "{lang} must not select Chinese");
        }
    }

    #[test]
    fn resolve_language_prefers_installation_then_os_then_default() {
        // Installation preference always wins (ignores OS locale).
        assert_eq!(resolve_language(Some("en-US"), Some("zh-CN")), "en-US");
        assert_eq!(resolve_language(Some("zh-CN"), Some("en-US")), "zh-CN");
        // No installation preference → follow the OS locale (首轮跟随系统语言).
        assert_eq!(resolve_language(None, Some("zh-CN")), "zh-CN");
        assert_eq!(resolve_language(Some("  "), Some("zh_CN")), "zh-CN");
        assert_eq!(resolve_language(None, Some("en-US")), "en-US");
        assert_eq!(resolve_language(None, Some("ja-JP")), "en-US");
        // Neither → hard default. The SQLite seed en-US is not consulted here.
        assert_eq!(resolve_language(None, None), "en-US");
        assert_eq!(resolve_language(Some(""), Some("   ")), "en-US");
    }

    #[tokio::test]
    async fn read_app_language_returns_installation_preference() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(nomifun_common::INSTALLATION_PREFERENCES_FILE),
            br#"{"language":"zh-CN"}"#,
        )
        .unwrap();
        assert_eq!(read_app_language(dir.path()).await, "zh-CN");
    }

    #[tokio::test]
    async fn read_app_language_ignores_missing_installation_file() {
        let dir = tempfile::tempdir().unwrap();
        let language = read_app_language(dir.path()).await;
        assert!(
            language == "zh-CN" || language == "en-US",
            "host OS locale must normalize to a supported UI language, got {language}"
        );
    }

    #[test]
    fn resolve_mcp_servers_adds_gateway_when_process_config_present() {
        let overrides = NomiBuildExtra {
            gateway_mcp_config: Some(gateway_config(41237, "/usr/bin/nomicore", "owner")),
            user_id: Some("0190f5fe-7c00-7a00-8abc-012345678961".into()),
            companion_id: Some("0190f5fe-7c00-7a00-8abc-012345678965".into()),
            gateway_excluded_tools: vec!["nomi_delegate".into()],
            ..Default::default()
        };
        let (servers, leases) = resolve_mcp_servers(&overrides, "0190f5fe-7c00-7a00-8abc-012345678963");
        assert_eq!(leases.len(), 1);
        let gw = servers
            .get(GatewayMcpConfig::SERVER_NAME)
            .expect("gateway server registered");
        assert_eq!(
            gw.args.as_deref(),
            Some(&["mcp-gateway-stdio".to_owned()][..])
        );
        let env = gw.env.as_ref().expect("env set");
        assert_eq!(env.len(), 1);
        let bootstrap: nomifun_api_types::ScopedMcpChildBootstrap<
            nomifun_api_types::GatewayCapabilityClaims,
        > = serde_json::from_str(
            env.get(GatewayMcpConfig::ENV_CAPABILITY)
                .expect("capability bootstrap env"),
        )
        .unwrap();
        assert_eq!(bootstrap.port, 41237);
        let claims = bootstrap.access.claims;
        assert_eq!(
            claims.user_id.as_str(),
            "0190f5fe-7c00-7a00-8abc-012345678961"
        );
        assert_eq!(claims.session.session_id, "0190f5fe-7c00-7a00-8abc-012345678963");
        assert_eq!(claims.session.conversation_id.as_deref(), Some("0190f5fe-7c00-7a00-8abc-012345678963"));
        assert_eq!(claims.scope.companion_id.as_deref(), Some("0190f5fe-7c00-7a00-8abc-012345678965"));
        assert_eq!(claims.scope.profile, GatewayMcpConfig::PROFILE_WORK);
        assert_eq!(claims.scope.session_mode.as_deref(), Some("yolo"));
        assert_eq!(claims.scope.excluded_tools, vec!["nomi_delegate"]);
        assert!(!claims.scope.instance_owner);
        assert!(!env[GatewayMcpConfig::ENV_CAPABILITY].contains("gw-root-secret"));
        assert_eq!(gw.deferred, Some(true));
    }

    #[test]
    fn gateway_env_omits_companion_id_when_unbound() {
        let overrides = NomiBuildExtra {
            gateway_mcp_config: Some(gateway_config(41237, "/usr/bin/nomicore", "owner")),
            user_id: Some("0190f5fe-7c00-7a00-8abc-012345678961".into()),
            companion_id: None,
            ..Default::default()
        };
        let (servers, _leases) = resolve_mcp_servers(&overrides, "0190f5fe-7c00-7a00-8abc-012345678963");
        let env = servers[GatewayMcpConfig::SERVER_NAME].env.as_ref().unwrap();
        let bootstrap: nomifun_api_types::ScopedMcpChildBootstrap<
            nomifun_api_types::GatewayCapabilityClaims,
        > = serde_json::from_str(env.get(GatewayMcpConfig::ENV_CAPABILITY).unwrap()).unwrap();
        let claims = bootstrap.access.claims;
        assert!(claims.scope.companion_id.is_none());
    }

    #[test]
    fn gateway_env_uses_lite_profile_for_channel_sessions() {
        let overrides = NomiBuildExtra {
            gateway_mcp_config: Some(gateway_config(41237, "/usr/bin/nomicore", "owner")),
            user_id: Some("0190f5fe-7c00-7a00-8abc-012345678961".into()),
            channel_platform: Some("lark".into()),
            ..Default::default()
        };
        let (servers, _leases) = resolve_mcp_servers(&overrides, "0190f5fe-7c00-7a00-8abc-012345678963");
        let env = servers[GatewayMcpConfig::SERVER_NAME].env.as_ref().unwrap();
        let bootstrap: nomifun_api_types::ScopedMcpChildBootstrap<
            nomifun_api_types::GatewayCapabilityClaims,
        > = serde_json::from_str(env.get(GatewayMcpConfig::ENV_CAPABILITY).unwrap()).unwrap();
        let claims = bootstrap.access.claims;
        assert_eq!(claims.scope.profile, GatewayMcpConfig::PROFILE_LITE);
    }

    #[test]
    fn resolve_mcp_servers_skips_gateway_without_process_config() {
        let overrides = NomiBuildExtra::default();
        let (servers, leases) = resolve_mcp_servers(&overrides, "0190f5fe-7c00-7a00-8abc-012345678963");
        assert!(!servers.contains_key(GatewayMcpConfig::SERVER_NAME));
        assert!(leases.is_empty());
    }

    #[test]
    fn normalize_nomi_base_url_strips_v1() {
        assert_eq!(
            normalize_nomi_base_url("https://api.openai.com/v1"),
            "https://api.openai.com"
        );
        assert_eq!(
            normalize_nomi_base_url("https://api.openai.com/v1/"),
            "https://api.openai.com"
        );
        assert_eq!(
            normalize_nomi_base_url("https://api.anthropic.com"),
            "https://api.anthropic.com"
        );
        assert_eq!(
            normalize_nomi_base_url("https://api.deepseek.com/"),
            "https://api.deepseek.com"
        );
        assert_eq!(
            normalize_nomi_base_url("http://localhost:11434"),
            "http://localhost:11434"
        );
        assert_eq!(normalize_nomi_base_url(""), "");
    }

    #[test]
    fn map_nomi_provider_known_platforms() {
        assert_eq!(map_nomi_provider("anthropic", None), "anthropic");
        assert_eq!(map_nomi_provider("bedrock", None), "bedrock");
        assert_eq!(map_nomi_provider("gemini-vertex-ai", None), "vertex");
    }

    #[test]
    fn map_nomi_provider_custom_and_others_default_to_openai() {
        assert_eq!(map_nomi_provider("custom", None), "openai");
        assert_eq!(map_nomi_provider("gemini", None), "openai");
        assert_eq!(map_nomi_provider("new-api", None), "openai");
        assert_eq!(map_nomi_provider("unknown", None), "openai");
    }

    #[test]
    fn map_nomi_provider_new_api_with_anthropic_protocol() {
        assert_eq!(
            map_nomi_provider("new-api", Some("anthropic")),
            "anthropic"
        );
        assert_eq!(map_nomi_provider("new-api", Some("openai")), "openai");
        assert_eq!(map_nomi_provider("new-api", None), "openai");
    }

    #[test]
    fn map_nomi_provider_non_new_api_ignores_protocol_override() {
        assert_eq!(map_nomi_provider("custom", Some("anthropic")), "openai");
    }

    #[test]
    fn is_openai_host_detects_official_api() {
        assert!(is_openai_host("https://api.openai.com/v1"));
        assert!(is_openai_host("https://api.openai.com"));
        assert!(is_openai_host("https://API.OPENAI.COM/v1"));
        assert!(!is_openai_host("https://api.deepseek.com/v1"));
        assert!(!is_openai_host("https://openai.example.com/v1"));
        assert!(!is_openai_host(""));
        assert!(!is_openai_host("not-a-url"));
    }

    #[test]
    fn resolve_openai_official_sets_max_completion_tokens() {
        let (base_url, compat) =
            resolve_nomi_url_and_compat("custom", "https://api.openai.com/v1", "openai", false);
        assert_eq!(base_url.as_deref(), Some("https://api.openai.com"));
        assert_eq!(
            compat.max_tokens_field.as_deref(),
            Some("max_completion_tokens")
        );
        assert!(compat.api_path.is_none());
    }

    #[test]
    fn resolve_non_openai_keeps_default_max_tokens() {
        let (base_url, compat) =
            resolve_nomi_url_and_compat("custom", "https://api.deepseek.com/v1", "openai", false);
        assert_eq!(base_url.as_deref(), Some("https://api.deepseek.com"));
        assert!(compat.max_tokens_field.is_none());
    }

    #[test]
    fn resolve_gemini_prepends_path_and_sets_api_path() {
        let (base_url, compat) = resolve_nomi_url_and_compat(
            "gemini",
            "https://generativelanguage.googleapis.com",
            "openai",
            false,
        );
        assert_eq!(
            base_url.as_deref(),
            Some("https://generativelanguage.googleapis.com/v1beta/openai")
        );
        assert_eq!(compat.api_path.as_deref(), Some("/chat/completions"));
        assert!(compat.max_tokens_field.is_none());
    }

    #[test]
    fn resolve_anthropic_no_compat_overrides() {
        let (base_url, compat) = resolve_nomi_url_and_compat(
            "anthropic",
            "https://api.anthropic.com",
            "anthropic",
            false,
        );
        assert_eq!(base_url.as_deref(), Some("https://api.anthropic.com"));
        assert!(compat.max_tokens_field.is_none());
        assert!(compat.api_path.is_none());
    }

    #[test]
    fn resolve_full_url_mode_uses_url_as_is() {
        let (base_url, compat) = resolve_nomi_url_and_compat(
            "custom",
            "https://proxy.example.com/v1/chat/completions",
            "openai",
            true,
        );
        assert_eq!(
            base_url.as_deref(),
            Some("https://proxy.example.com/v1/chat/completions")
        );
        assert_eq!(compat.api_path.as_deref(), Some(""));
        assert!(compat.max_tokens_field.is_none());
    }

    #[test]
    fn resolve_full_url_mode_strips_trailing_slash() {
        let (base_url, compat) = resolve_nomi_url_and_compat(
            "custom",
            "https://proxy.example.com/v1/chat/completions/",
            "openai",
            true,
        );
        assert_eq!(
            base_url.as_deref(),
            Some("https://proxy.example.com/v1/chat/completions")
        );
        assert_eq!(compat.api_path.as_deref(), Some(""));
    }

    #[test]
    fn resolve_full_url_false_still_normalizes() {
        let (base_url, compat) =
            resolve_nomi_url_and_compat("custom", "https://api.deepseek.com/v1", "openai", false);
        assert_eq!(base_url.as_deref(), Some("https://api.deepseek.com"));
        assert!(compat.api_path.is_none());
    }

    #[test]
    fn resolve_domestic_openai_compatible_platforms_use_configured_chat_base() {
        for (platform, base) in [
            ("ark", "https://ark.cn-beijing.volces.com/api/v3"),
            ("stepfun", "https://api.stepfun.com/v1"),
            ("zhipu", "https://open.bigmodel.cn/api/paas/v4"),
            ("qianfan", "https://qianfan.baidubce.com/v2"),
        ] {
            let (base_url, compat) = resolve_nomi_url_and_compat(platform, base, "openai", false);
            assert_eq!(base_url.as_deref(), Some(base), "platform={platform}");
            assert_eq!(
                compat.api_path.as_deref(),
                Some("/chat/completions"),
                "platform={platform}"
            );
        }
    }

    #[test]
    fn resolve_coding_plan_platforms_use_chat_completions_at_configured_base() {
        for (platform, base) in [
            (
                "ark-coding-plan",
                "https://ark.cn-beijing.volces.com/api/coding/v3",
            ),
            (
                "ark-agent-plan",
                "https://ark.cn-beijing.volces.com/api/plan/v3",
            ),
            ("stepfun-plan", "https://api.stepfun.com/step_plan/v1"),
            (
                "dashscope-coding",
                "https://coding.dashscope.aliyuncs.com/v1",
            ),
            (
                "glm-coding-plan",
                "https://open.bigmodel.cn/api/coding/paas/v4",
            ),
            (
                "qianfan-coding-plan",
                "https://qianfan.baidubce.com/v2/coding",
            ),
        ] {
            let (base_url, compat) = resolve_nomi_url_and_compat(platform, base, "openai", false);
            assert_eq!(base_url.as_deref(), Some(base), "platform={platform}");
            assert_eq!(
                compat.api_path.as_deref(),
                Some("/chat/completions"),
                "platform={platform}"
            );
        }
    }

    #[test]
    fn resolve_mcp_servers_empty_when_no_config() {
        let overrides = NomiBuildExtra::default();
        let (result, leases) = resolve_mcp_servers(&overrides, "conv-3");
        assert!(result.is_empty());
        assert!(leases.is_empty());
    }

    #[tokio::test]
    async fn session_snapshot_overrides_repo_backed_mcp_config() {
        let mut servers = HashMap::from([(
            "demo-mcp".to_owned(),
            McpServerConfig {
                transport: TransportType::Stdio,
                command: Some("npx".into()),
                args: Some(vec!["-y".into(), "@old/server".into()]),
                env: Some(HashMap::new()),
                url: None,
                headers: None,
                deferred: Some(false),
                request_timeout_secs: None,
                startup_timeout_secs: None,
                cwd: None,
                enabled_tools: None,
                disabled_tools: None,
            },
        )]);

        let snapshot = vec![SessionMcpServer {
            mcp_server_id: McpServerId::new(),
            name: "demo-mcp".into(),
            transport: SessionMcpTransport::Stdio {
                command: "uvx".into(),
                args: vec!["new-server".into()],
                env: HashMap::from([("TOKEN".into(), "abc".into())]),
            },
        }];

        merge_session_snapshot_mcp_servers(&mut servers, &snapshot, "conv-override", None).await;

        let server = servers.get("demo-mcp").expect("snapshot should remain");
        assert_eq!(server.transport, TransportType::Stdio);
        // `resolve_command_path` may resolve to an absolute path; on Windows
        // that includes the `.exe` extension.
        let command = server
            .command
            .as_deref()
            .expect("stdio command should exist");
        let command = command.replace('\\', "/").to_lowercase();
        assert!(
            command == "uvx" || command.ends_with("/uvx") || command.ends_with("/uvx.exe"),
            "unexpected stdio command path: {command}",
        );
        assert_eq!(server.args.as_deref(), Some(&["new-server".to_owned()][..]));
        assert_eq!(
            server.env.as_ref().and_then(|env| env.get("TOKEN")),
            Some(&"abc".to_owned())
        );
    }

    fn declarations_from(source: &str) -> NomiMcpDeclarations {
        let declarations = NomiMcpDeclarations::parse(source).expect("declarations must parse");
        assert!(
            declarations.rejected.is_empty(),
            "unexpected rejections: {:?}",
            declarations.rejected
        );
        declarations
    }

    /// `~/.agent-store/mcp.json` entries reach a session with every transport
    /// mapped, the per-server timeout applied and disabled entries left out.
    #[test]
    fn host_declarations_reach_the_session_and_map_every_transport() {
        let declarations = declarations_from(
            r#"{ "mcpServers": {
                 "filesystem": { "command": "npx", "args": ["-y", "srv"], "env": { "TOKEN": "literal" }, "toolTimeoutMs": 2500 },
                 "linear": { "url": "https://x/mcp", "headers": { "X-Tenant": "acme" } },
                 "legacy": { "transport": "sse", "url": "https://x/sse" },
                 "off": { "command": "never", "enabled": false }
               } }"#,
        );

        let mut servers = HashMap::new();
        merge_host_declared_mcp_servers(&mut servers, &declarations, "conv-decl", true);

        assert_eq!(
            servers.len(),
            3,
            "the disabled entry must not be merged: {servers:?}"
        );
        assert!(!servers.contains_key("off"));

        let filesystem = &servers["filesystem"];
        assert_eq!(filesystem.transport, TransportType::Stdio);
        assert_eq!(filesystem.command.as_deref(), Some("npx"));
        assert_eq!(
            filesystem.args.as_deref(),
            Some(&["-y".to_owned(), "srv".to_owned()][..])
        );
        // A literal value passes through the `secret:` resolver untouched.
        assert_eq!(
            filesystem
                .env
                .as_ref()
                .and_then(|env| env.get("TOKEN"))
                .map(String::as_str),
            Some("literal")
        );
        // 2500ms rounds up to the engine's whole-second granularity.
        assert_eq!(filesystem.request_timeout_secs, Some(3));
        assert_eq!(filesystem.deferred, Some(false));

        assert_eq!(servers["linear"].transport, TransportType::StreamableHttp);
        assert_eq!(servers["linear"].url.as_deref(), Some("https://x/mcp"));
        assert_eq!(
            servers["linear"]
                .headers
                .as_ref()
                .and_then(|headers| headers.get("X-Tenant"))
                .map(String::as_str),
            Some("acme")
        );
        assert_eq!(servers["legacy"].transport, TransportType::Sse);
    }

    /// A declared entry's stdio extras and tool filters reach the engine config
    /// verbatim, so the engine stays the single place they take effect: `cwd` for
    /// the child process, the filters inside `nomi-mcp`'s registration.
    #[test]
    fn declared_extras_reach_the_engine_config() {
        let declarations = declarations_from(
            r#"{ "mcpServers": {
                 "filesystem": {
                   "command": "npx",
                   "cwd": "/srv/data",
                   "startupTimeoutMs": 5000,
                   "enabledTools": ["read_file"],
                   "disabledTools": ["write_file"]
                 },
                 "linear": { "url": "https://x/mcp" }
               } }"#,
        );
        let mut servers = HashMap::new();
        merge_host_declared_mcp_servers(&mut servers, &declarations, "conv-extras", true);

        let filesystem = &servers["filesystem"];
        assert_eq!(filesystem.cwd.as_deref(), Some("/srv/data"));
        assert_eq!(filesystem.startup_timeout_secs, Some(5));
        assert_eq!(filesystem.enabled_tools, Some(vec!["read_file".to_owned()]));
        assert_eq!(filesystem.disabled_tools, Some(vec!["write_file".to_owned()]));

        // The remote entry declares none of them, and gains none by accident.
        let linear = &servers["linear"];
        assert!(linear.cwd.is_none());
        assert!(linear.enabled_tools.is_none());
        assert!(linear.disabled_tools.is_none());
        assert!(linear.startup_timeout_secs.is_none());
    }

    /// `bearerTokenEnvVar` names a credential instead of carrying it, and becomes
    /// a real `Authorization: Bearer …` header — with an explicit header winning
    /// and an unresolvable name omitting the header rather than inventing one.
    #[test]
    fn bearer_token_env_var_becomes_an_authorization_header() {
        let credentials = HashMap::from([("GITHUB_TOKEN".to_owned(), "s3cr3t".to_owned())]);

        let mut headers = HashMap::new();
        apply_bearer_token(
            "conv-bearer",
            "linear",
            Some("GITHUB_TOKEN"),
            &mut headers,
            &credentials,
        );
        assert_eq!(
            headers.get("Authorization").map(String::as_str),
            Some("Bearer s3cr3t")
        );

        // A declared `Authorization` header wins: the declaration said so literally.
        let mut headers = HashMap::from([("Authorization".to_owned(), "Basic abc".to_owned())]);
        apply_bearer_token(
            "conv-bearer",
            "linear",
            Some("GITHUB_TOKEN"),
            &mut headers,
            &credentials,
        );
        assert_eq!(
            headers.get("Authorization").map(String::as_str),
            Some("Basic abc")
        );

        // A name with no value omits the header — never a literal `Bearer <name>`.
        let mut headers = HashMap::new();
        apply_bearer_token(
            "conv-bearer",
            "linear",
            Some("__NOMIFUN_DEFINITELY_UNSET__"),
            &mut headers,
            &credentials,
        );
        assert!(headers.is_empty(), "{headers:?}");

        let mut headers = HashMap::new();
        apply_bearer_token("conv-bearer", "linear", None, &mut headers, &credentials);
        assert!(headers.is_empty(), "{headers:?}");
    }

    /// Every path that builds an engine config resolves `secret:<NAME>` in
    /// **headers**, not only in `env`. A reference written into a header used to be
    /// forwarded verbatim from a DB row or a session snapshot, so the literal text
    /// was sent and the remote end answered 401 with nothing local to explain it.
    #[test]
    fn a_header_reference_resolves_on_every_path() {
        let declarations = declarations_from(
            r#"{ "mcpServers": {
                 "linear": { "url": "https://x/mcp", "headers": {
                   "Authorization": "secret:__NOMIFUN_DEFINITELY_UNSET__",
                   "X-Tenant": "acme"
                 } }
               } }"#,
        );
        let mut servers = HashMap::new();
        merge_host_declared_mcp_servers(&mut servers, &declarations, "conv-headers", true);
        let headers = servers["linear"].headers.as_ref().expect("headers");
        assert!(
            !headers.contains_key("Authorization"),
            "an unresolvable reference is omitted, never sent literally: {headers:?}"
        );
        assert_eq!(headers.get("X-Tenant").map(String::as_str), Some("acme"));

        // The session-snapshot path calls this same helper.
        let resolved = resolve_header_secrets(
            None,
            "linear",
            &HashMap::from([(
                "Authorization".to_owned(),
                "secret:__NOMIFUN_DEFINITELY_UNSET__".to_owned(),
            )]),
        );
        assert!(resolved.is_empty(), "{resolved:?}");

        // `Bearer secret:X` is *not* a whole-value reference, so it is forwarded
        // and only warned about — the shape a user is likeliest to write, and the
        // reason that warning exists.
        let resolved = resolve_header_secrets(
            None,
            "linear",
            &HashMap::from([("Authorization".to_owned(), "Bearer secret:X".to_owned())]),
        );
        assert_eq!(
            resolved.get("Authorization").map(String::as_str),
            Some("Bearer secret:X")
        );
    }

    /// A persisted `mcp_servers` row is not a weaker path than a declaration: it
    /// goes through the same header resolution.
    #[test]
    fn a_persisted_row_resolves_its_headers_too() {
        let row = McpServerRow {
            mcp_server_id: "0192f000-0000-7000-8000-000000000000".to_owned(),
            name: "linear".to_owned(),
            description: None,
            enabled: true,
            transport_type: "http".to_owned(),
            transport_config: serde_json::json!({
                "url": "https://x/mcp",
                "headers": {
                    "X-Tenant": "acme",
                    "Authorization": "secret:__NOMIFUN_DEFINITELY_UNSET__"
                }
            })
            .to_string(),
            tools: None,
            last_test_status: "disconnected".to_owned(),
            last_connected: None,
            original_json: None,
            builtin: false,
            deleted_at: None,
            created_at: 0,
            updated_at: 0,
        };

        let config = row_to_mcp_server_config(&row).expect("row must map");
        let headers = config.headers.as_ref().expect("headers");
        assert!(
            !headers.contains_key("Authorization"),
            "the row path resolves references instead of forwarding them: {headers:?}"
        );
        assert_eq!(headers.get("X-Tenant").map(String::as_str), Some("acme"));
    }

    /// The declaration merge never clobbers a name that is already taken. In the
    /// factory that name is either a request-level binding (merged before it) or
    /// a `mcp_servers` row is *skipped* by the same rule — declarations are
    /// merged first and that loop is `entry().or_insert(...)`.
    ///
    /// The two steps are mirrored here because the ordering lives at the call
    /// site; the end-to-end shape is covered by the real-binary run recorded in
    /// `docs/agent-store/20-tool-injection-policy.zh.md` §9.3.
    #[test]
    fn host_declarations_never_clobber_a_name_that_is_already_taken() {
        let declarations =
            declarations_from(r#"{ "mcpServers": { "shared": { "command": "from-file" } } }"#);

        // (1) Request-level binding first: the declared server must not replace it.
        let mut bound = HashMap::from([(
            "shared".to_owned(),
            McpServerConfig {
                transport: TransportType::Stdio,
                command: Some("from-request".into()),
                args: None,
                env: None,
                url: None,
                headers: None,
                deferred: Some(false),
                request_timeout_secs: None,
                startup_timeout_secs: None,
                cwd: None,
                enabled_tools: None,
                disabled_tools: None,
            },
        )]);
        merge_host_declared_mcp_servers(&mut bound, &declarations, "conv-bound", true);
        assert_eq!(bound["shared"].command.as_deref(), Some("from-request"));

        // (2) Declarations first, then a repo row via the production rule: the
        // declaration owns the name, so the row cannot overwrite it.
        let mut merged = HashMap::new();
        merge_host_declared_mcp_servers(&mut merged, &declarations, "conv-row", true);
        merged.entry("shared".to_owned()).or_insert(McpServerConfig {
            transport: TransportType::Stdio,
            command: Some("from-row".into()),
            args: None,
            env: None,
            url: None,
            headers: None,
            deferred: Some(false),
            request_timeout_secs: None,
            startup_timeout_secs: None,
            cwd: None,
            enabled_tools: None,
            disabled_tools: None,
        });
        assert_eq!(merged["shared"].command.as_deref(), Some("from-file"));
    }

    /// A declaration is a host capability (a stdio server runs a local process),
    /// so a non-owner principal must not receive one — the same gate the
    /// `mcp_servers` rows already sit behind.
    #[test]
    fn host_declarations_are_owner_gated() {
        let declarations =
            declarations_from(r#"{ "mcpServers": { "filesystem": { "command": "npx" } } }"#);
        let mut servers = HashMap::new();
        merge_host_declared_mcp_servers(&mut servers, &declarations, "conv-secondary", false);
        assert!(servers.is_empty(), "{servers:?}");
    }

    /// The default (a host that never opted in, or a `mcp.json` that is absent
    /// or broken) declares nothing and therefore changes nothing.
    #[test]
    fn empty_declarations_are_a_no_op() {
        let mut servers = HashMap::new();
        merge_host_declared_mcp_servers(
            &mut servers,
            &NomiMcpDeclarations::default(),
            "conv-empty",
            true,
        );
        assert!(servers.is_empty());
    }

    /// The §7.9 key rules (length + charset) exist so that a user's whole-server
    /// pattern `mcp__<key>__*` really addresses **every** tool of a declared
    /// server. This is the cross-crate half of that promise: the pattern is
    /// matched against the engine's real canonical tool name, for every accepted
    /// key length and for tool names long enough to force slug truncation.
    #[test]
    fn declaration_keys_stay_addressable_by_a_whole_server_pattern() {
        use nomifun_api_types::MAX_DECLARATION_KEY_LEN;

        for length in 1..=MAX_DECLARATION_KEY_LEN {
            let key = "k".repeat(length);
            let pattern = format!("mcp__{key}__");
            for tool in [
                "read",
                "a_very_long_tool_name_that_will_definitely_be_truncated_by_the_slug_budget",
            ] {
                let canonical = nomi_mcp::tool_proxy::canonical_mcp_display_name(&key, tool);
                assert!(
                    canonical.starts_with(&pattern),
                    "`mcp__{key}__*` must match {canonical} (key length {length})"
                );
            }
        }
    }

    #[test]
    fn resolve_bedrock_config_access_key() {
        let json = r#"{"auth_method":"accessKey","region":"us-west-2","access_key_id":"AKIA123","secret_access_key":"secret456"}"#;
        let result = resolve_bedrock_config(Some(json)).unwrap();
        assert_eq!(result.region.as_deref(), Some("us-west-2"));
        assert_eq!(result.access_key_id.as_deref(), Some("AKIA123"));
        assert_eq!(result.secret_access_key.as_deref(), Some("secret456"));
        assert!(result.profile.is_none());
        assert!(result.session_token.is_none());
    }

    #[test]
    fn resolve_bedrock_config_profile() {
        let json = r#"{"auth_method":"profile","region":"eu-west-1","profile":"my-profile"}"#;
        let result = resolve_bedrock_config(Some(json)).unwrap();
        assert_eq!(result.region.as_deref(), Some("eu-west-1"));
        assert_eq!(result.profile.as_deref(), Some("my-profile"));
        assert!(result.access_key_id.is_none());
        assert!(result.secret_access_key.is_none());
    }

    #[test]
    fn resolve_bedrock_config_none_when_json_missing() {
        assert!(resolve_bedrock_config(None).is_none());
    }

    #[test]
    fn resolve_bedrock_config_none_when_json_invalid() {
        assert!(resolve_bedrock_config(Some("not-json")).is_none());
    }

    #[test]
    fn preset_rules_merged_into_system_prompt_when_no_existing() {
        let json = serde_json::json!({
            "preset_rules": "You are a data analyst. Always use Python.",
        });
        let mut overrides: NomiBuildExtra = serde_json::from_value(json).unwrap();

        if let Some(rules) = overrides.preset_rules.take() {
            overrides.system_prompt = Some(match overrides.system_prompt.take() {
                Some(existing) => format!("{existing}\n\n{rules}"),
                None => rules,
            });
        }

        assert_eq!(
            overrides.system_prompt.as_deref(),
            Some("You are a data analyst. Always use Python.")
        );
        assert!(overrides.preset_rules.is_none());
    }

    #[test]
    fn preset_rules_appended_to_existing_system_prompt() {
        let json = serde_json::json!({
            "system_prompt": "Be concise.",
            "preset_rules": "You are a data analyst.",
        });
        let mut overrides: NomiBuildExtra = serde_json::from_value(json).unwrap();

        if let Some(rules) = overrides.preset_rules.take() {
            overrides.system_prompt = Some(match overrides.system_prompt.take() {
                Some(existing) => format!("{existing}\n\n{rules}"),
                None => rules,
            });
        }

        assert_eq!(
            overrides.system_prompt.as_deref(),
            Some("Be concise.\n\nYou are a data analyst.")
        );
    }

    #[test]
    fn no_preset_rules_leaves_system_prompt_unchanged() {
        let json = serde_json::json!({
            "system_prompt": "Be concise.",
        });
        let mut overrides: NomiBuildExtra = serde_json::from_value(json).unwrap();

        if let Some(rules) = overrides.preset_rules.take() {
            overrides.system_prompt = Some(match overrides.system_prompt.take() {
                Some(existing) => format!("{existing}\n\n{rules}"),
                None => rules,
            });
        }

        assert_eq!(overrides.system_prompt.as_deref(), Some("Be concise."));
    }

    #[test]
    fn embedded_agent_execution_requires_trusted_no_gateway_host() {
        assert!(should_install_embedded_agent_execution(false, true, true));
        assert!(!should_install_embedded_agent_execution(true, true, true));
        assert!(!should_install_embedded_agent_execution(false, false, true));
        // A host that owns its own durable execution facade opts out even when it
        // is the installation owner and runs no gateway.
        assert!(!should_install_embedded_agent_execution(false, true, false));
    }

    #[test]
    fn automatic_delegation_hint_injects_for_plain_desktop_session() {
        assert!(super::should_inject_delegation_hint(true, false, false));
        let out = super::compose_delegation_hint(
            Some("基础提示".to_string()),
            true,
            DelegationPolicy::Automatic,
        );
        let s = out.unwrap();
        assert!(s.starts_with("基础提示"));
        assert!(s.contains("nomi_delegate"));
        assert!(s.contains("strategy=parallel"));
        assert!(s.contains("strategy=planned"));
        assert!(s.contains("nomi_execution_get"));
        assert!(!s.contains(super::DELEGATION_PREFER_PARALLEL_HINT));
    }

    #[test]
    fn delegation_hint_skips_when_gateway_absent() {
        assert!(!super::should_inject_delegation_hint(false, false, false));
    }

    #[test]
    fn delegation_hint_skips_restricted_surfaces() {
        assert!(!super::should_inject_delegation_hint(true, true, false));
        assert!(!super::should_inject_delegation_hint(true, false, true));
        let base = Some("仅基础".to_string());
        assert_eq!(
            super::compose_delegation_hint(base.clone(), false, DelegationPolicy::Automatic),
            base
        );
    }

    #[test]
    fn automatic_delegation_hint_handles_empty_base() {
        let out = super::compose_delegation_hint(None, true, DelegationPolicy::Automatic);
        assert_eq!(out, Some(super::DELEGATION_STANDARD_HINT.to_string()));
    }

    #[test]
    fn prefer_parallel_hint_appends_after_standard_hint() {
        let out = super::compose_delegation_hint(
            Some("基础提示".to_string()),
            true,
            DelegationPolicy::PreferParallel,
        )
        .unwrap();
        assert!(out.starts_with("基础提示"));
        let standard_pos = out.find(super::DELEGATION_STANDARD_HINT).expect("标准提示在场");
        let preference_pos = out
            .find(super::DELEGATION_PREFER_PARALLEL_HINT)
            .expect("并行偏好提示在场");
        assert!(standard_pos < preference_pos);
        assert!(out.contains("优先使用"));
    }

    #[test]
    fn disabled_delegation_policy_preserves_base() {
        let base = Some("仅基础".to_string());
        assert_eq!(
            super::compose_delegation_hint(base.clone(), true, DelegationPolicy::Disabled),
            base
        );
    }

    /// One tool name, exactly one owner, and the precedence is the registration
    /// precedence: Gateway wins, then the embedded engine, then the host facade.
    #[test]
    fn exactly_one_delegate_deployment_owns_the_name() {
        assert_eq!(
            delegate_deployment(true, false, true),
            DelegateDeployment::Gateway,
            "the Gateway owns the name whenever it is wired"
        );
        assert_eq!(delegate_deployment(false, true, true), DelegateDeployment::Embedded);
        assert_eq!(
            delegate_deployment(false, false, true),
            DelegateDeployment::HostFacade
        );
        assert_eq!(delegate_deployment(false, false, false), DelegateDeployment::None);
    }

    /// The facade hint must describe the facade, not Platform Gateway: this
    /// deployment rejects `strategy=parallel` and has no `nomi_execution_get`.
    #[test]
    fn host_facade_delegation_hint_is_planned_only() {
        let out = compose_host_delegation_hint(
            Some("基础提示".to_string()),
            true,
            DelegationPolicy::Automatic,
        )
        .unwrap();
        assert!(out.starts_with("基础提示"), "preset context must survive");
        assert!(out.contains("nomi_delegate"));
        assert!(out.contains("planned"));
        assert!(
            !out.contains("strategy=parallel"),
            "the facade schema rejects parallel: {out}"
        );
        assert!(
            !out.contains("nomi_execution_get"),
            "this deployment exposes no execution reads: {out}"
        );
    }

    #[test]
    fn host_facade_delegation_hint_respects_surface_and_policy() {
        let base = Some("仅基础".to_string());
        for policy in [DelegationPolicy::Automatic, DelegationPolicy::PreferParallel] {
            assert_eq!(
                compose_host_delegation_hint(base.clone(), false, policy),
                base,
                "companion / channel surfaces get no delegation hint"
            );
        }
        assert_eq!(
            compose_host_delegation_hint(base.clone(), true, DelegationPolicy::Disabled),
            base,
            "a Disabled tier has no delegate to describe"
        );
        assert_eq!(
            compose_host_delegation_hint(None, true, DelegationPolicy::Automatic),
            Some(super::HOST_DELEGATE_STANDARD_HINT.to_owned())
        );
    }

    #[test]
    fn native_write_root_unrestricted_only_for_local_desktop() {
        // 本地桌面(无渠道)→ None(OS 用户全权,今日行为)。
        assert_eq!(resolve_native_write_root(None, "/ws"), None);
        assert_eq!(resolve_native_write_root(Some(""), "/ws"), None);
        // 渠道 → 收窄到工作区。
        assert_eq!(
            resolve_native_write_root(Some("lark"), "/ws"),
            Some("/ws".to_owned())
        );
        // 渠道但工作区为空 → 回退 None(无从钳制,不劣于今日)。
        assert_eq!(resolve_native_write_root(Some("lark"), "  "), None);
    }

    #[test]
    fn append_knowledge_context_without_mounts_is_passthrough() {
        let config = NomiBuildExtra::default();
        assert_eq!(
            append_knowledge_context(None, &config, true),
            None
        );
        assert_eq!(
            append_knowledge_context(Some("hello".into()), &config, true),
            Some("hello".into())
        );
    }

    #[test]
    fn append_knowledge_context_renders_mounts_and_writeback() {
        use nomifun_api_types::KnowledgeMountInfo;

        let conversation_id = "0190f5fe-7c00-7a00-8abc-012345678963";

        let mut config = NomiBuildExtra {
            knowledge_mounts: vec![KnowledgeMountInfo {
                knowledge_base_id: nomifun_common::KnowledgeBaseId::new(),
                name: "领域知识".into(),
                description: "domain docs".into(),
                rel_path: ".flowy/knowledge/领域知识".into(),
                toc: vec!["intro.md — 简介".into()],
                summary: Some("Covers deployment flows and runbooks.".into()),
                live_sources: vec![],
            }],
            knowledge_writeback: false,
            ..Default::default()
        };

        let readonly =
            append_knowledge_context(Some("base".into()), &config, true).unwrap();
        assert!(readonly.starts_with("base\n\n"));
        assert!(readonly.contains("## Knowledge bases"));
        assert!(readonly.contains("领域知识"));
        assert!(readonly.contains("intro.md — 简介"));
        assert!(readonly.contains("READ-ONLY"));
        // Hit-rate contract: retrieval protocol (once), per-base summary and
        // when-to-consult guidance — same shared builder as the ACP path.
        assert_eq!(readonly.matches("Retrieval protocol").count(), 1);
        assert!(readonly.contains("Covers deployment flows and runbooks."));
        assert!(readonly.contains("When to consult"));

        // nomi surface has the native tool → the write-back contract points at
        // it, and no session id or inbox path can leak into the prompt any more.
        config.knowledge_writeback = true;
        let tooled = append_knowledge_context(None, &config, true).unwrap();
        assert!(tooled.contains("Write-back is ENABLED"));
        assert!(tooled.contains("knowledge_write"));
        assert!(!tooled.contains("STAGED"));
        assert!(!tooled.contains("_inbox"));
        assert!(!tooled.contains(conversation_id));
        // Disposition (回写意识) threads from build-extra → contract.
        assert!(tooled.contains("Disposition — MANUAL"));
        config.knowledge_writeback_eagerness = Some("auto".into());
        let eager = append_knowledge_context(None, &config, true).unwrap();
        assert!(eager.contains("Disposition — AUTO"));
    }

    #[test]
    fn knowledge_fields_deserialize_from_extra_and_reach_prompt() {
        // The conversation service writes snake_case keys into build-extra
        // JSON; the nomi build path must surface them in the system prompt.
        let json = serde_json::json!({
            "knowledge_mounts": [{
                "knowledge_base_id": "0190f5fe-7c00-7a00-8abc-012345678964",
                "name": "运维手册",
                "description": "",
                "rel_path": ".flowy/knowledge/运维手册",
                "toc": ["deploy.md — 部署"],
            }],
            "knowledge_writeback": true,
            "knowledge_writeback_eagerness": "auto",
        });
        let overrides: NomiBuildExtra = serde_json::from_value(json).unwrap();
        assert_eq!(overrides.knowledge_mounts.len(), 1);
        assert!(overrides.knowledge_writeback);
        assert_eq!(
            overrides.knowledge_writeback_eagerness.as_deref(),
            Some("auto")
        );

        let prompt = append_knowledge_context(None, &overrides, true).unwrap();
        assert!(prompt.contains("Knowledge bases"));
        assert!(prompt.contains("运维手册"));
        assert!(prompt.contains("knowledge_write"));
        // The disposition keyword threads all the way from extra JSON to prompt.
        assert!(prompt.contains("Disposition — AUTO"));
        // Optional summary/live_sources may be absent while the canonical
        // knowledge-base identity contract remains strict.
        assert!(prompt.contains("When to consult"));
    }

    #[test]
    fn channel_write_opt_in_threads_from_extra_into_write_policy() {
        // Regression: the `channel_write_enabled` opt-in must survive the
        // build-extra round-trip so the nomi factory can resolve the
        // external-IM-channel write policy. Before the fix this field was never
        // threaded, so the reconstructed binding defaulted it to false and
        // channel write-back was permanently Disabled on the nomi engine.
        use nomifun_knowledge::{KnowledgeBinding, WriteMode, WriteSurface, resolve_write_policy};

        // Absent in JSON → serde default false (the previous, broken behavior).
        let off: NomiBuildExtra = serde_json::from_value(serde_json::json!({
            "knowledge_writeback": true,
        }))
        .unwrap();
        assert!(!off.knowledge_channel_write_enabled);

        // Present and true → carried through.
        let on: NomiBuildExtra = serde_json::from_value(serde_json::json!({
            "knowledge_writeback": true,
            "knowledge_channel_write_enabled": true,
        }))
        .unwrap();
        assert!(on.knowledge_channel_write_enabled);

        // Reconstruct the binding exactly as build_nomi does and confirm the
        // policy flips from Disabled to Staged for an external channel.
        let reconstruct = |extra: &NomiBuildExtra| KnowledgeBinding {
            enabled: true,
            writeback: extra.knowledge_writeback,
            channel_write_enabled: extra.knowledge_channel_write_enabled,
            ..Default::default()
        };

        let disabled = resolve_write_policy(WriteSurface::ExternalChannel, &reconstruct(&off));
        assert!(matches!(disabled.mode, WriteMode::Disabled));

        let enabled = resolve_write_policy(WriteSurface::ExternalChannel, &reconstruct(&on));
        assert!(matches!(enabled.mode, WriteMode::Direct));
    }

    #[test]
    fn global_moa_fallback_fills_absent_extra() {
        let mut overrides = NomiBuildExtra::default();
        let global = r#"{"enabled":true,"references":[{"provider_id":"p1","model":"m1"}]}"#;
        apply_global_moa_fallback(&mut overrides, Some(global));
        let moa = overrides.moa.expect("global settings must be inherited");
        assert!(moa.enabled);
        assert_eq!(moa.references.len(), 1);
        assert_eq!(moa.references[0].model, "m1");
    }

    #[test]
    fn global_moa_fallback_unwraps_double_encoded_string_row() {
        // The real `client_preferences` row: the frontend writes the settings
        // as a JSON string value and the prefs service serializes that value
        // again, so the stored row is a JSON *string literal* containing the
        // MoaSettings document.
        let mut overrides = NomiBuildExtra::default();
        let inner = r#"{"enabled":true,"references":[{"provider_id":"p1","model":"m1"}]}"#;
        let stored = serde_json::to_string(inner).expect("encode string row");
        apply_global_moa_fallback(&mut overrides, Some(&stored));
        let moa = overrides.moa.expect("double-encoded settings must be inherited");
        assert!(moa.enabled);
        assert_eq!(moa.references.len(), 1);
        assert_eq!(moa.references[0].model, "m1");
    }

    #[test]
    fn global_moa_fallback_never_overrides_session_extra() {
        let mut overrides = NomiBuildExtra {
            moa: Some(nomifun_api_types::MoaSettings {
                enabled: false,
                ..Default::default()
            }),
            ..Default::default()
        };
        let global = r#"{"enabled":true,"references":[{"provider_id":"p1","model":"m1"}]}"#;
        apply_global_moa_fallback(&mut overrides, Some(global));
        // The explicit (disabled) session value wins over the enabled global.
        let moa = overrides.moa.expect("session extra must be preserved");
        assert!(!moa.enabled);
        assert!(moa.references.is_empty());
    }

    #[test]
    fn global_moa_fallback_skips_companion_sessions() {
        let mut overrides = NomiBuildExtra {
            companion: true,
            ..Default::default()
        };
        let global = r#"{"enabled":true,"references":[{"provider_id":"p1","model":"m1"}]}"#;
        apply_global_moa_fallback(&mut overrides, Some(global));
        assert!(overrides.moa.is_none(), "companions never inherit MoA");
    }

    #[test]
    fn global_moa_fallback_degrades_on_malformed_json() {
        let mut overrides = NomiBuildExtra::default();
        apply_global_moa_fallback(&mut overrides, Some("{not json"));
        assert!(overrides.moa.is_none(), "bad JSON must degrade to no fallback");

        apply_global_moa_fallback(&mut overrides, None);
        assert!(overrides.moa.is_none());
    }
}

/// P2 Task 6 behavior snapshot for the chat-path platform mapping.
///
/// Locks the EXACT `(provider, base_url, api_path, compat)` outputs of
/// `map_nomi_provider` + `resolve_nomi_url_and_compat` over the full platform
/// matrix — every `MODEL_PLATFORMS` entry from
/// `ui/src/renderer/utils/model/modelPlatforms.ts` (kept per-entry, so custom
/// presets with distinct base URLs each get a row) × representative base_url
/// variants (configured / trailing slash / toggled `/v1` / empty / full-URL),
/// plus new-api per-model protocol-override edge cases.
///
/// `SNAPSHOT` was generated by CALLING the pre-refactor implementation
/// (2026-07-29, commit eff19c8f working tree). It must stay byte-identical —
/// UNCHANGED — through the `platform_table` refactor; any diff means the
/// chat-path behavior regressed.
#[cfg(test)]
mod platform_chat_snapshot {
    use super::{map_nomi_provider, resolve_nomi_url_and_compat};

    /// Every `MODEL_PLATFORMS` entry as `(platform key, configured base_url)`,
    /// in file order. Entries without a preset base_url (Custom / New API /
    /// Bedrock) use a representative or empty base. Two extra rows:
    /// the managed free-model platform and an unknown platform (default row).
    const PLATFORM_MATRIX: &[(&str, &str)] = &[
        ("custom", "https://api.example.com/v1"), // Custom (user-supplied base)
        ("new-api", "https://gateway.example.com/v1"), // New API gateway
        ("gemini", "https://generativelanguage.googleapis.com"),
        ("openai", "https://api.openai.com/v1"),
        ("anthropic", "https://api.anthropic.com"),
        ("bedrock", ""),
        ("deepseek", "https://api.deepseek.com/v1"),
        ("mimo", "https://api.xiaomimimo.com/v1"),
        ("mimo-token-plan-cn", "https://token-plan-cn.xiaomimimo.com/v1"),
        ("mimo-token-plan-sgp", "https://token-plan-sgp.xiaomimimo.com/v1"),
        ("mimo-token-plan-ams", "https://token-plan-ams.xiaomimimo.com/v1"),
        ("minimax", "https://api.minimaxi.com/v1"),
        ("minimax-code", "https://api.minimax.io/v1"),
        ("minimax-coding-plan", "https://api.minimaxi.com/v1"),
        ("novita", "https://api.novita.ai/openai/v1"),
        ("openrouter", "https://openrouter.ai/api/v1"),
        ("dashscope", "https://dashscope.aliyuncs.com/compatible-mode/v1"),
        ("dashscope-coding", "https://coding.dashscope.aliyuncs.com/v1"),
        ("siliconflow", "https://api.siliconflow.cn/v1"), // SiliconFlow-CN
        ("siliconflow", "https://api.siliconflow.com/v1"), // SiliconFlow
        ("zhipu", "https://open.bigmodel.cn/api/paas/v4"),
        ("glm-coding-plan", "https://open.bigmodel.cn/api/coding/paas/v4"),
        ("moonshot-cn", "https://api.moonshot.cn/v1"),
        ("moonshot-global", "https://api.moonshot.ai/v1"),
        ("xai", "https://api.x.ai/v1"),
        ("ark", "https://ark.cn-beijing.volces.com/api/v3"),
        ("ark-coding-plan", "https://ark.cn-beijing.volces.com/api/coding/v3"),
        ("ark-agent-plan", "https://ark.cn-beijing.volces.com/api/plan/v3"),
        ("qianfan", "https://qianfan.baidubce.com/v2"),
        ("qianfan-coding-plan", "https://qianfan.baidubce.com/v2/coding"),
        ("hunyuan", "https://tokenhub.tencentmaas.com/v1"),
        ("hunyuan-global", "https://tokenhub-intl.tencentmaas.com/v1"),
        ("lingyi", "https://api.lingyiwanwu.com/v1"),
        ("poe", "https://api.poe.com/v1"),
        ("ppio", "https://api.ppio.com/openai/v1"),
        ("modelscope", "https://api-inference.modelscope.cn/v1"),
        ("infiniai", "https://cloud.infini-ai.com/maas/v1"),
        ("ctyun", "https://wishub-x6.ctyun.cn/v1"),
        ("stepfun", "https://api.stepfun.com/v1"),
        ("stepfun-plan", "https://api.stepfun.com/step_plan/v1"),
        ("nomifun-free-model", "https://free.nomifun.example/v1"), // managed free model
        ("totally-unknown", "https://api.example.org/v1"), // default row
    ];

    /// `(platform, base_url, is_full_url, protocol)` extras: the new-api
    /// per-model protocol override, its interaction with the api.openai.com
    /// host rule, and full-URL edge cases (empty base; full-URL beating the
    /// gemini / domestic-whitelist platform rules).
    const EXTRA_CASES: &[(&str, &str, bool, Option<&str>)] = &[
        ("new-api", "https://gateway.example.com/v1", false, Some("anthropic")),
        ("new-api", "https://gateway.example.com/v1", false, Some("openai")),
        ("new-api", "https://gateway.example.com/v1", false, Some("gemini")),
        ("custom", "https://api.example.com/v1", false, Some("anthropic")),
        ("anthropic", "https://api.anthropic.com", false, Some("openai")),
        ("new-api", "https://api.openai.com/v1", false, None),
        ("new-api", "https://api.openai.com/v1", false, Some("anthropic")),
        ("custom", "", true, None),
        ("gemini", "https://proxy.example.com/gemini/chat", true, None),
        ("ark", "https://proxy.example.com/ark/chat", true, None),
    ];

    /// Base-url variants per platform row: configured / trailing slash /
    /// toggled `/v1` (stripped when present, appended when absent) / empty /
    /// full-URL (`is_full_url = true`).
    fn variants(base: &str) -> Vec<(String, bool)> {
        let toggled_v1 = match base.strip_suffix("/v1") {
            Some(stripped) => stripped.to_owned(),
            None => format!("{base}/v1"),
        };
        vec![
            (base.to_owned(), false),
            (format!("{base}/"), false),
            (toggled_v1, false),
            (String::new(), false),
            (format!("{base}/chat/completions"), true),
        ]
    }

    /// Mirrors the production call sequence (`provider_config.rs` /
    /// `provider_health.rs`): map the platform first, then resolve URL/compat
    /// with the MAPPED provider.
    fn render_case(platform: &str, base: &str, full: bool, proto: Option<&str>) -> String {
        let provider = map_nomi_provider(platform, proto);
        let (base_url, compat) = resolve_nomi_url_and_compat(platform, base, &provider, full);
        format!(
            "{platform} | in={base:?} | full={full} | proto={proto:?} => provider={provider} \
             | base={base_url:?} | api_path={:?} | max_tokens={:?} | image={:?} | reasoning={:?}",
            compat.api_path,
            compat.max_tokens_field,
            compat.supports_image,
            compat.require_reasoning_content,
        )
    }

    fn render_all() -> String {
        let mut out = String::new();
        for (platform, base) in PLATFORM_MATRIX {
            for (variant, full) in variants(base) {
                out.push_str(&render_case(platform, &variant, full, None));
                out.push('\n');
            }
        }
        for (platform, base, full, proto) in EXTRA_CASES {
            out.push_str(&render_case(platform, base, *full, *proto));
            out.push('\n');
        }
        out
    }

    #[test]
    fn platform_chat_rules_snapshot_locked() {
        let actual = render_all();
        if actual != SNAPSHOT {
            println!("=== ACTUAL SNAPSHOT BEGIN ===");
            print!("{actual}");
            println!("=== ACTUAL SNAPSHOT END ===");
            panic!(
                "platform chat snapshot changed — chat-path (provider, base_url, api_path, \
                 compat) must stay byte-identical to the pre-table behavior"
            );
        }
    }

    #[rustfmt::skip]
    const SNAPSHOT: &str = r#"custom | in="https://api.example.com/v1" | full=false | proto=None => provider=openai | base=Some("https://api.example.com") | api_path=None | max_tokens=None | image=None | reasoning=None
custom | in="https://api.example.com/v1/" | full=false | proto=None => provider=openai | base=Some("https://api.example.com") | api_path=None | max_tokens=None | image=None | reasoning=None
custom | in="https://api.example.com" | full=false | proto=None => provider=openai | base=Some("https://api.example.com") | api_path=None | max_tokens=None | image=None | reasoning=None
custom | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
custom | in="https://api.example.com/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://api.example.com/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
new-api | in="https://gateway.example.com/v1" | full=false | proto=None => provider=openai | base=Some("https://gateway.example.com") | api_path=None | max_tokens=None | image=None | reasoning=None
new-api | in="https://gateway.example.com/v1/" | full=false | proto=None => provider=openai | base=Some("https://gateway.example.com") | api_path=None | max_tokens=None | image=None | reasoning=None
new-api | in="https://gateway.example.com" | full=false | proto=None => provider=openai | base=Some("https://gateway.example.com") | api_path=None | max_tokens=None | image=None | reasoning=None
new-api | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
new-api | in="https://gateway.example.com/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://gateway.example.com/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
gemini | in="https://generativelanguage.googleapis.com" | full=false | proto=None => provider=openai | base=Some("https://generativelanguage.googleapis.com/v1beta/openai") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
gemini | in="https://generativelanguage.googleapis.com/" | full=false | proto=None => provider=openai | base=Some("https://generativelanguage.googleapis.com/v1beta/openai") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
gemini | in="https://generativelanguage.googleapis.com/v1" | full=false | proto=None => provider=openai | base=Some("https://generativelanguage.googleapis.com/v1/v1beta/openai") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
gemini | in="" | full=false | proto=None => provider=openai | base=Some("/v1beta/openai") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
gemini | in="https://generativelanguage.googleapis.com/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://generativelanguage.googleapis.com/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
openai | in="https://api.openai.com/v1" | full=false | proto=None => provider=openai | base=Some("https://api.openai.com") | api_path=None | max_tokens=Some("max_completion_tokens") | image=None | reasoning=None
openai | in="https://api.openai.com/v1/" | full=false | proto=None => provider=openai | base=Some("https://api.openai.com") | api_path=None | max_tokens=Some("max_completion_tokens") | image=None | reasoning=None
openai | in="https://api.openai.com" | full=false | proto=None => provider=openai | base=Some("https://api.openai.com") | api_path=None | max_tokens=Some("max_completion_tokens") | image=None | reasoning=None
openai | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
openai | in="https://api.openai.com/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://api.openai.com/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
anthropic | in="https://api.anthropic.com" | full=false | proto=None => provider=anthropic | base=Some("https://api.anthropic.com") | api_path=None | max_tokens=None | image=None | reasoning=None
anthropic | in="https://api.anthropic.com/" | full=false | proto=None => provider=anthropic | base=Some("https://api.anthropic.com") | api_path=None | max_tokens=None | image=None | reasoning=None
anthropic | in="https://api.anthropic.com/v1" | full=false | proto=None => provider=anthropic | base=Some("https://api.anthropic.com") | api_path=None | max_tokens=None | image=None | reasoning=None
anthropic | in="" | full=false | proto=None => provider=anthropic | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
anthropic | in="https://api.anthropic.com/chat/completions" | full=true | proto=None => provider=anthropic | base=Some("https://api.anthropic.com/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
bedrock | in="" | full=false | proto=None => provider=bedrock | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
bedrock | in="/" | full=false | proto=None => provider=bedrock | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
bedrock | in="/v1" | full=false | proto=None => provider=bedrock | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
bedrock | in="" | full=false | proto=None => provider=bedrock | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
bedrock | in="/chat/completions" | full=true | proto=None => provider=bedrock | base=Some("/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
deepseek | in="https://api.deepseek.com/v1" | full=false | proto=None => provider=openai | base=Some("https://api.deepseek.com") | api_path=None | max_tokens=None | image=None | reasoning=None
deepseek | in="https://api.deepseek.com/v1/" | full=false | proto=None => provider=openai | base=Some("https://api.deepseek.com") | api_path=None | max_tokens=None | image=None | reasoning=None
deepseek | in="https://api.deepseek.com" | full=false | proto=None => provider=openai | base=Some("https://api.deepseek.com") | api_path=None | max_tokens=None | image=None | reasoning=None
deepseek | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
deepseek | in="https://api.deepseek.com/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://api.deepseek.com/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
mimo | in="https://api.xiaomimimo.com/v1" | full=false | proto=None => provider=openai | base=Some("https://api.xiaomimimo.com") | api_path=None | max_tokens=None | image=None | reasoning=None
mimo | in="https://api.xiaomimimo.com/v1/" | full=false | proto=None => provider=openai | base=Some("https://api.xiaomimimo.com") | api_path=None | max_tokens=None | image=None | reasoning=None
mimo | in="https://api.xiaomimimo.com" | full=false | proto=None => provider=openai | base=Some("https://api.xiaomimimo.com") | api_path=None | max_tokens=None | image=None | reasoning=None
mimo | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
mimo | in="https://api.xiaomimimo.com/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://api.xiaomimimo.com/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
mimo-token-plan-cn | in="https://token-plan-cn.xiaomimimo.com/v1" | full=false | proto=None => provider=openai | base=Some("https://token-plan-cn.xiaomimimo.com") | api_path=None | max_tokens=None | image=None | reasoning=None
mimo-token-plan-cn | in="https://token-plan-cn.xiaomimimo.com/v1/" | full=false | proto=None => provider=openai | base=Some("https://token-plan-cn.xiaomimimo.com") | api_path=None | max_tokens=None | image=None | reasoning=None
mimo-token-plan-cn | in="https://token-plan-cn.xiaomimimo.com" | full=false | proto=None => provider=openai | base=Some("https://token-plan-cn.xiaomimimo.com") | api_path=None | max_tokens=None | image=None | reasoning=None
mimo-token-plan-cn | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
mimo-token-plan-cn | in="https://token-plan-cn.xiaomimimo.com/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://token-plan-cn.xiaomimimo.com/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
mimo-token-plan-sgp | in="https://token-plan-sgp.xiaomimimo.com/v1" | full=false | proto=None => provider=openai | base=Some("https://token-plan-sgp.xiaomimimo.com") | api_path=None | max_tokens=None | image=None | reasoning=None
mimo-token-plan-sgp | in="https://token-plan-sgp.xiaomimimo.com/v1/" | full=false | proto=None => provider=openai | base=Some("https://token-plan-sgp.xiaomimimo.com") | api_path=None | max_tokens=None | image=None | reasoning=None
mimo-token-plan-sgp | in="https://token-plan-sgp.xiaomimimo.com" | full=false | proto=None => provider=openai | base=Some("https://token-plan-sgp.xiaomimimo.com") | api_path=None | max_tokens=None | image=None | reasoning=None
mimo-token-plan-sgp | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
mimo-token-plan-sgp | in="https://token-plan-sgp.xiaomimimo.com/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://token-plan-sgp.xiaomimimo.com/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
mimo-token-plan-ams | in="https://token-plan-ams.xiaomimimo.com/v1" | full=false | proto=None => provider=openai | base=Some("https://token-plan-ams.xiaomimimo.com") | api_path=None | max_tokens=None | image=None | reasoning=None
mimo-token-plan-ams | in="https://token-plan-ams.xiaomimimo.com/v1/" | full=false | proto=None => provider=openai | base=Some("https://token-plan-ams.xiaomimimo.com") | api_path=None | max_tokens=None | image=None | reasoning=None
mimo-token-plan-ams | in="https://token-plan-ams.xiaomimimo.com" | full=false | proto=None => provider=openai | base=Some("https://token-plan-ams.xiaomimimo.com") | api_path=None | max_tokens=None | image=None | reasoning=None
mimo-token-plan-ams | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
mimo-token-plan-ams | in="https://token-plan-ams.xiaomimimo.com/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://token-plan-ams.xiaomimimo.com/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
minimax | in="https://api.minimaxi.com/v1" | full=false | proto=None => provider=openai | base=Some("https://api.minimaxi.com") | api_path=None | max_tokens=None | image=None | reasoning=None
minimax | in="https://api.minimaxi.com/v1/" | full=false | proto=None => provider=openai | base=Some("https://api.minimaxi.com") | api_path=None | max_tokens=None | image=None | reasoning=None
minimax | in="https://api.minimaxi.com" | full=false | proto=None => provider=openai | base=Some("https://api.minimaxi.com") | api_path=None | max_tokens=None | image=None | reasoning=None
minimax | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
minimax | in="https://api.minimaxi.com/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://api.minimaxi.com/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
minimax-code | in="https://api.minimax.io/v1" | full=false | proto=None => provider=openai | base=Some("https://api.minimax.io") | api_path=None | max_tokens=None | image=None | reasoning=None
minimax-code | in="https://api.minimax.io/v1/" | full=false | proto=None => provider=openai | base=Some("https://api.minimax.io") | api_path=None | max_tokens=None | image=None | reasoning=None
minimax-code | in="https://api.minimax.io" | full=false | proto=None => provider=openai | base=Some("https://api.minimax.io") | api_path=None | max_tokens=None | image=None | reasoning=None
minimax-code | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
minimax-code | in="https://api.minimax.io/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://api.minimax.io/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
minimax-coding-plan | in="https://api.minimaxi.com/v1" | full=false | proto=None => provider=openai | base=Some("https://api.minimaxi.com") | api_path=None | max_tokens=None | image=None | reasoning=None
minimax-coding-plan | in="https://api.minimaxi.com/v1/" | full=false | proto=None => provider=openai | base=Some("https://api.minimaxi.com") | api_path=None | max_tokens=None | image=None | reasoning=None
minimax-coding-plan | in="https://api.minimaxi.com" | full=false | proto=None => provider=openai | base=Some("https://api.minimaxi.com") | api_path=None | max_tokens=None | image=None | reasoning=None
minimax-coding-plan | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
minimax-coding-plan | in="https://api.minimaxi.com/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://api.minimaxi.com/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
novita | in="https://api.novita.ai/openai/v1" | full=false | proto=None => provider=openai | base=Some("https://api.novita.ai/openai") | api_path=None | max_tokens=None | image=None | reasoning=None
novita | in="https://api.novita.ai/openai/v1/" | full=false | proto=None => provider=openai | base=Some("https://api.novita.ai/openai") | api_path=None | max_tokens=None | image=None | reasoning=None
novita | in="https://api.novita.ai/openai" | full=false | proto=None => provider=openai | base=Some("https://api.novita.ai/openai") | api_path=None | max_tokens=None | image=None | reasoning=None
novita | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
novita | in="https://api.novita.ai/openai/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://api.novita.ai/openai/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
openrouter | in="https://openrouter.ai/api/v1" | full=false | proto=None => provider=openai | base=Some("https://openrouter.ai/api") | api_path=None | max_tokens=None | image=None | reasoning=None
openrouter | in="https://openrouter.ai/api/v1/" | full=false | proto=None => provider=openai | base=Some("https://openrouter.ai/api") | api_path=None | max_tokens=None | image=None | reasoning=None
openrouter | in="https://openrouter.ai/api" | full=false | proto=None => provider=openai | base=Some("https://openrouter.ai/api") | api_path=None | max_tokens=None | image=None | reasoning=None
openrouter | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
openrouter | in="https://openrouter.ai/api/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://openrouter.ai/api/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
dashscope | in="https://dashscope.aliyuncs.com/compatible-mode/v1" | full=false | proto=None => provider=openai | base=Some("https://dashscope.aliyuncs.com/compatible-mode") | api_path=None | max_tokens=None | image=None | reasoning=None
dashscope | in="https://dashscope.aliyuncs.com/compatible-mode/v1/" | full=false | proto=None => provider=openai | base=Some("https://dashscope.aliyuncs.com/compatible-mode") | api_path=None | max_tokens=None | image=None | reasoning=None
dashscope | in="https://dashscope.aliyuncs.com/compatible-mode" | full=false | proto=None => provider=openai | base=Some("https://dashscope.aliyuncs.com/compatible-mode") | api_path=None | max_tokens=None | image=None | reasoning=None
dashscope | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
dashscope | in="https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
dashscope-coding | in="https://coding.dashscope.aliyuncs.com/v1" | full=false | proto=None => provider=openai | base=Some("https://coding.dashscope.aliyuncs.com/v1") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
dashscope-coding | in="https://coding.dashscope.aliyuncs.com/v1/" | full=false | proto=None => provider=openai | base=Some("https://coding.dashscope.aliyuncs.com/v1") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
dashscope-coding | in="https://coding.dashscope.aliyuncs.com" | full=false | proto=None => provider=openai | base=Some("https://coding.dashscope.aliyuncs.com") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
dashscope-coding | in="" | full=false | proto=None => provider=openai | base=None | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
dashscope-coding | in="https://coding.dashscope.aliyuncs.com/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://coding.dashscope.aliyuncs.com/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
siliconflow | in="https://api.siliconflow.cn/v1" | full=false | proto=None => provider=openai | base=Some("https://api.siliconflow.cn") | api_path=None | max_tokens=None | image=None | reasoning=None
siliconflow | in="https://api.siliconflow.cn/v1/" | full=false | proto=None => provider=openai | base=Some("https://api.siliconflow.cn") | api_path=None | max_tokens=None | image=None | reasoning=None
siliconflow | in="https://api.siliconflow.cn" | full=false | proto=None => provider=openai | base=Some("https://api.siliconflow.cn") | api_path=None | max_tokens=None | image=None | reasoning=None
siliconflow | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
siliconflow | in="https://api.siliconflow.cn/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://api.siliconflow.cn/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
siliconflow | in="https://api.siliconflow.com/v1" | full=false | proto=None => provider=openai | base=Some("https://api.siliconflow.com") | api_path=None | max_tokens=None | image=None | reasoning=None
siliconflow | in="https://api.siliconflow.com/v1/" | full=false | proto=None => provider=openai | base=Some("https://api.siliconflow.com") | api_path=None | max_tokens=None | image=None | reasoning=None
siliconflow | in="https://api.siliconflow.com" | full=false | proto=None => provider=openai | base=Some("https://api.siliconflow.com") | api_path=None | max_tokens=None | image=None | reasoning=None
siliconflow | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
siliconflow | in="https://api.siliconflow.com/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://api.siliconflow.com/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
zhipu | in="https://open.bigmodel.cn/api/paas/v4" | full=false | proto=None => provider=openai | base=Some("https://open.bigmodel.cn/api/paas/v4") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
zhipu | in="https://open.bigmodel.cn/api/paas/v4/" | full=false | proto=None => provider=openai | base=Some("https://open.bigmodel.cn/api/paas/v4") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
zhipu | in="https://open.bigmodel.cn/api/paas/v4/v1" | full=false | proto=None => provider=openai | base=Some("https://open.bigmodel.cn/api/paas/v4/v1") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
zhipu | in="" | full=false | proto=None => provider=openai | base=None | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
zhipu | in="https://open.bigmodel.cn/api/paas/v4/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://open.bigmodel.cn/api/paas/v4/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
glm-coding-plan | in="https://open.bigmodel.cn/api/coding/paas/v4" | full=false | proto=None => provider=openai | base=Some("https://open.bigmodel.cn/api/coding/paas/v4") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
glm-coding-plan | in="https://open.bigmodel.cn/api/coding/paas/v4/" | full=false | proto=None => provider=openai | base=Some("https://open.bigmodel.cn/api/coding/paas/v4") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
glm-coding-plan | in="https://open.bigmodel.cn/api/coding/paas/v4/v1" | full=false | proto=None => provider=openai | base=Some("https://open.bigmodel.cn/api/coding/paas/v4/v1") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
glm-coding-plan | in="" | full=false | proto=None => provider=openai | base=None | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
glm-coding-plan | in="https://open.bigmodel.cn/api/coding/paas/v4/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://open.bigmodel.cn/api/coding/paas/v4/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
moonshot-cn | in="https://api.moonshot.cn/v1" | full=false | proto=None => provider=openai | base=Some("https://api.moonshot.cn") | api_path=None | max_tokens=None | image=None | reasoning=None
moonshot-cn | in="https://api.moonshot.cn/v1/" | full=false | proto=None => provider=openai | base=Some("https://api.moonshot.cn") | api_path=None | max_tokens=None | image=None | reasoning=None
moonshot-cn | in="https://api.moonshot.cn" | full=false | proto=None => provider=openai | base=Some("https://api.moonshot.cn") | api_path=None | max_tokens=None | image=None | reasoning=None
moonshot-cn | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
moonshot-cn | in="https://api.moonshot.cn/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://api.moonshot.cn/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
moonshot-global | in="https://api.moonshot.ai/v1" | full=false | proto=None => provider=openai | base=Some("https://api.moonshot.ai") | api_path=None | max_tokens=None | image=None | reasoning=None
moonshot-global | in="https://api.moonshot.ai/v1/" | full=false | proto=None => provider=openai | base=Some("https://api.moonshot.ai") | api_path=None | max_tokens=None | image=None | reasoning=None
moonshot-global | in="https://api.moonshot.ai" | full=false | proto=None => provider=openai | base=Some("https://api.moonshot.ai") | api_path=None | max_tokens=None | image=None | reasoning=None
moonshot-global | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
moonshot-global | in="https://api.moonshot.ai/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://api.moonshot.ai/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
xai | in="https://api.x.ai/v1" | full=false | proto=None => provider=openai | base=Some("https://api.x.ai") | api_path=None | max_tokens=None | image=None | reasoning=None
xai | in="https://api.x.ai/v1/" | full=false | proto=None => provider=openai | base=Some("https://api.x.ai") | api_path=None | max_tokens=None | image=None | reasoning=None
xai | in="https://api.x.ai" | full=false | proto=None => provider=openai | base=Some("https://api.x.ai") | api_path=None | max_tokens=None | image=None | reasoning=None
xai | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
xai | in="https://api.x.ai/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://api.x.ai/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
ark | in="https://ark.cn-beijing.volces.com/api/v3" | full=false | proto=None => provider=openai | base=Some("https://ark.cn-beijing.volces.com/api/v3") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
ark | in="https://ark.cn-beijing.volces.com/api/v3/" | full=false | proto=None => provider=openai | base=Some("https://ark.cn-beijing.volces.com/api/v3") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
ark | in="https://ark.cn-beijing.volces.com/api/v3/v1" | full=false | proto=None => provider=openai | base=Some("https://ark.cn-beijing.volces.com/api/v3/v1") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
ark | in="" | full=false | proto=None => provider=openai | base=None | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
ark | in="https://ark.cn-beijing.volces.com/api/v3/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://ark.cn-beijing.volces.com/api/v3/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
ark-coding-plan | in="https://ark.cn-beijing.volces.com/api/coding/v3" | full=false | proto=None => provider=openai | base=Some("https://ark.cn-beijing.volces.com/api/coding/v3") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
ark-coding-plan | in="https://ark.cn-beijing.volces.com/api/coding/v3/" | full=false | proto=None => provider=openai | base=Some("https://ark.cn-beijing.volces.com/api/coding/v3") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
ark-coding-plan | in="https://ark.cn-beijing.volces.com/api/coding/v3/v1" | full=false | proto=None => provider=openai | base=Some("https://ark.cn-beijing.volces.com/api/coding/v3/v1") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
ark-coding-plan | in="" | full=false | proto=None => provider=openai | base=None | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
ark-coding-plan | in="https://ark.cn-beijing.volces.com/api/coding/v3/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://ark.cn-beijing.volces.com/api/coding/v3/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
ark-agent-plan | in="https://ark.cn-beijing.volces.com/api/plan/v3" | full=false | proto=None => provider=openai | base=Some("https://ark.cn-beijing.volces.com/api/plan/v3") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
ark-agent-plan | in="https://ark.cn-beijing.volces.com/api/plan/v3/" | full=false | proto=None => provider=openai | base=Some("https://ark.cn-beijing.volces.com/api/plan/v3") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
ark-agent-plan | in="https://ark.cn-beijing.volces.com/api/plan/v3/v1" | full=false | proto=None => provider=openai | base=Some("https://ark.cn-beijing.volces.com/api/plan/v3/v1") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
ark-agent-plan | in="" | full=false | proto=None => provider=openai | base=None | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
ark-agent-plan | in="https://ark.cn-beijing.volces.com/api/plan/v3/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://ark.cn-beijing.volces.com/api/plan/v3/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
qianfan | in="https://qianfan.baidubce.com/v2" | full=false | proto=None => provider=openai | base=Some("https://qianfan.baidubce.com/v2") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
qianfan | in="https://qianfan.baidubce.com/v2/" | full=false | proto=None => provider=openai | base=Some("https://qianfan.baidubce.com/v2") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
qianfan | in="https://qianfan.baidubce.com/v2/v1" | full=false | proto=None => provider=openai | base=Some("https://qianfan.baidubce.com/v2/v1") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
qianfan | in="" | full=false | proto=None => provider=openai | base=None | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
qianfan | in="https://qianfan.baidubce.com/v2/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://qianfan.baidubce.com/v2/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
qianfan-coding-plan | in="https://qianfan.baidubce.com/v2/coding" | full=false | proto=None => provider=openai | base=Some("https://qianfan.baidubce.com/v2/coding") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
qianfan-coding-plan | in="https://qianfan.baidubce.com/v2/coding/" | full=false | proto=None => provider=openai | base=Some("https://qianfan.baidubce.com/v2/coding") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
qianfan-coding-plan | in="https://qianfan.baidubce.com/v2/coding/v1" | full=false | proto=None => provider=openai | base=Some("https://qianfan.baidubce.com/v2/coding/v1") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
qianfan-coding-plan | in="" | full=false | proto=None => provider=openai | base=None | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
qianfan-coding-plan | in="https://qianfan.baidubce.com/v2/coding/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://qianfan.baidubce.com/v2/coding/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
hunyuan | in="https://tokenhub.tencentmaas.com/v1" | full=false | proto=None => provider=openai | base=Some("https://tokenhub.tencentmaas.com") | api_path=None | max_tokens=None | image=None | reasoning=None
hunyuan | in="https://tokenhub.tencentmaas.com/v1/" | full=false | proto=None => provider=openai | base=Some("https://tokenhub.tencentmaas.com") | api_path=None | max_tokens=None | image=None | reasoning=None
hunyuan | in="https://tokenhub.tencentmaas.com" | full=false | proto=None => provider=openai | base=Some("https://tokenhub.tencentmaas.com") | api_path=None | max_tokens=None | image=None | reasoning=None
hunyuan | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
hunyuan | in="https://tokenhub.tencentmaas.com/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://tokenhub.tencentmaas.com/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
hunyuan-global | in="https://tokenhub-intl.tencentmaas.com/v1" | full=false | proto=None => provider=openai | base=Some("https://tokenhub-intl.tencentmaas.com") | api_path=None | max_tokens=None | image=None | reasoning=None
hunyuan-global | in="https://tokenhub-intl.tencentmaas.com/v1/" | full=false | proto=None => provider=openai | base=Some("https://tokenhub-intl.tencentmaas.com") | api_path=None | max_tokens=None | image=None | reasoning=None
hunyuan-global | in="https://tokenhub-intl.tencentmaas.com" | full=false | proto=None => provider=openai | base=Some("https://tokenhub-intl.tencentmaas.com") | api_path=None | max_tokens=None | image=None | reasoning=None
hunyuan-global | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
hunyuan-global | in="https://tokenhub-intl.tencentmaas.com/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://tokenhub-intl.tencentmaas.com/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
lingyi | in="https://api.lingyiwanwu.com/v1" | full=false | proto=None => provider=openai | base=Some("https://api.lingyiwanwu.com") | api_path=None | max_tokens=None | image=None | reasoning=None
lingyi | in="https://api.lingyiwanwu.com/v1/" | full=false | proto=None => provider=openai | base=Some("https://api.lingyiwanwu.com") | api_path=None | max_tokens=None | image=None | reasoning=None
lingyi | in="https://api.lingyiwanwu.com" | full=false | proto=None => provider=openai | base=Some("https://api.lingyiwanwu.com") | api_path=None | max_tokens=None | image=None | reasoning=None
lingyi | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
lingyi | in="https://api.lingyiwanwu.com/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://api.lingyiwanwu.com/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
poe | in="https://api.poe.com/v1" | full=false | proto=None => provider=openai | base=Some("https://api.poe.com") | api_path=None | max_tokens=None | image=None | reasoning=None
poe | in="https://api.poe.com/v1/" | full=false | proto=None => provider=openai | base=Some("https://api.poe.com") | api_path=None | max_tokens=None | image=None | reasoning=None
poe | in="https://api.poe.com" | full=false | proto=None => provider=openai | base=Some("https://api.poe.com") | api_path=None | max_tokens=None | image=None | reasoning=None
poe | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
poe | in="https://api.poe.com/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://api.poe.com/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
ppio | in="https://api.ppio.com/openai/v1" | full=false | proto=None => provider=openai | base=Some("https://api.ppio.com/openai") | api_path=None | max_tokens=None | image=None | reasoning=None
ppio | in="https://api.ppio.com/openai/v1/" | full=false | proto=None => provider=openai | base=Some("https://api.ppio.com/openai") | api_path=None | max_tokens=None | image=None | reasoning=None
ppio | in="https://api.ppio.com/openai" | full=false | proto=None => provider=openai | base=Some("https://api.ppio.com/openai") | api_path=None | max_tokens=None | image=None | reasoning=None
ppio | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
ppio | in="https://api.ppio.com/openai/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://api.ppio.com/openai/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
modelscope | in="https://api-inference.modelscope.cn/v1" | full=false | proto=None => provider=openai | base=Some("https://api-inference.modelscope.cn") | api_path=None | max_tokens=None | image=None | reasoning=None
modelscope | in="https://api-inference.modelscope.cn/v1/" | full=false | proto=None => provider=openai | base=Some("https://api-inference.modelscope.cn") | api_path=None | max_tokens=None | image=None | reasoning=None
modelscope | in="https://api-inference.modelscope.cn" | full=false | proto=None => provider=openai | base=Some("https://api-inference.modelscope.cn") | api_path=None | max_tokens=None | image=None | reasoning=None
modelscope | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
modelscope | in="https://api-inference.modelscope.cn/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://api-inference.modelscope.cn/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
infiniai | in="https://cloud.infini-ai.com/maas/v1" | full=false | proto=None => provider=openai | base=Some("https://cloud.infini-ai.com/maas") | api_path=None | max_tokens=None | image=None | reasoning=None
infiniai | in="https://cloud.infini-ai.com/maas/v1/" | full=false | proto=None => provider=openai | base=Some("https://cloud.infini-ai.com/maas") | api_path=None | max_tokens=None | image=None | reasoning=None
infiniai | in="https://cloud.infini-ai.com/maas" | full=false | proto=None => provider=openai | base=Some("https://cloud.infini-ai.com/maas") | api_path=None | max_tokens=None | image=None | reasoning=None
infiniai | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
infiniai | in="https://cloud.infini-ai.com/maas/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://cloud.infini-ai.com/maas/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
ctyun | in="https://wishub-x6.ctyun.cn/v1" | full=false | proto=None => provider=openai | base=Some("https://wishub-x6.ctyun.cn") | api_path=None | max_tokens=None | image=None | reasoning=None
ctyun | in="https://wishub-x6.ctyun.cn/v1/" | full=false | proto=None => provider=openai | base=Some("https://wishub-x6.ctyun.cn") | api_path=None | max_tokens=None | image=None | reasoning=None
ctyun | in="https://wishub-x6.ctyun.cn" | full=false | proto=None => provider=openai | base=Some("https://wishub-x6.ctyun.cn") | api_path=None | max_tokens=None | image=None | reasoning=None
ctyun | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
ctyun | in="https://wishub-x6.ctyun.cn/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://wishub-x6.ctyun.cn/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
stepfun | in="https://api.stepfun.com/v1" | full=false | proto=None => provider=openai | base=Some("https://api.stepfun.com/v1") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
stepfun | in="https://api.stepfun.com/v1/" | full=false | proto=None => provider=openai | base=Some("https://api.stepfun.com/v1") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
stepfun | in="https://api.stepfun.com" | full=false | proto=None => provider=openai | base=Some("https://api.stepfun.com") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
stepfun | in="" | full=false | proto=None => provider=openai | base=None | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
stepfun | in="https://api.stepfun.com/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://api.stepfun.com/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
stepfun-plan | in="https://api.stepfun.com/step_plan/v1" | full=false | proto=None => provider=openai | base=Some("https://api.stepfun.com/step_plan/v1") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
stepfun-plan | in="https://api.stepfun.com/step_plan/v1/" | full=false | proto=None => provider=openai | base=Some("https://api.stepfun.com/step_plan/v1") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
stepfun-plan | in="https://api.stepfun.com/step_plan" | full=false | proto=None => provider=openai | base=Some("https://api.stepfun.com/step_plan") | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
stepfun-plan | in="" | full=false | proto=None => provider=openai | base=None | api_path=Some("/chat/completions") | max_tokens=None | image=None | reasoning=None
stepfun-plan | in="https://api.stepfun.com/step_plan/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://api.stepfun.com/step_plan/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
nomifun-free-model | in="https://free.nomifun.example/v1" | full=false | proto=None => provider=openai | base=Some("https://free.nomifun.example") | api_path=None | max_tokens=None | image=None | reasoning=None
nomifun-free-model | in="https://free.nomifun.example/v1/" | full=false | proto=None => provider=openai | base=Some("https://free.nomifun.example") | api_path=None | max_tokens=None | image=None | reasoning=None
nomifun-free-model | in="https://free.nomifun.example" | full=false | proto=None => provider=openai | base=Some("https://free.nomifun.example") | api_path=None | max_tokens=None | image=None | reasoning=None
nomifun-free-model | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
nomifun-free-model | in="https://free.nomifun.example/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://free.nomifun.example/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
totally-unknown | in="https://api.example.org/v1" | full=false | proto=None => provider=openai | base=Some("https://api.example.org") | api_path=None | max_tokens=None | image=None | reasoning=None
totally-unknown | in="https://api.example.org/v1/" | full=false | proto=None => provider=openai | base=Some("https://api.example.org") | api_path=None | max_tokens=None | image=None | reasoning=None
totally-unknown | in="https://api.example.org" | full=false | proto=None => provider=openai | base=Some("https://api.example.org") | api_path=None | max_tokens=None | image=None | reasoning=None
totally-unknown | in="" | full=false | proto=None => provider=openai | base=None | api_path=None | max_tokens=None | image=None | reasoning=None
totally-unknown | in="https://api.example.org/v1/chat/completions" | full=true | proto=None => provider=openai | base=Some("https://api.example.org/v1/chat/completions") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
new-api | in="https://gateway.example.com/v1" | full=false | proto=Some("anthropic") => provider=anthropic | base=Some("https://gateway.example.com") | api_path=None | max_tokens=None | image=None | reasoning=None
new-api | in="https://gateway.example.com/v1" | full=false | proto=Some("openai") => provider=openai | base=Some("https://gateway.example.com") | api_path=None | max_tokens=None | image=None | reasoning=None
new-api | in="https://gateway.example.com/v1" | full=false | proto=Some("gemini") => provider=openai | base=Some("https://gateway.example.com") | api_path=None | max_tokens=None | image=None | reasoning=None
custom | in="https://api.example.com/v1" | full=false | proto=Some("anthropic") => provider=openai | base=Some("https://api.example.com") | api_path=None | max_tokens=None | image=None | reasoning=None
anthropic | in="https://api.anthropic.com" | full=false | proto=Some("openai") => provider=anthropic | base=Some("https://api.anthropic.com") | api_path=None | max_tokens=None | image=None | reasoning=None
new-api | in="https://api.openai.com/v1" | full=false | proto=None => provider=openai | base=Some("https://api.openai.com") | api_path=None | max_tokens=Some("max_completion_tokens") | image=None | reasoning=None
new-api | in="https://api.openai.com/v1" | full=false | proto=Some("anthropic") => provider=anthropic | base=Some("https://api.openai.com") | api_path=None | max_tokens=None | image=None | reasoning=None
custom | in="" | full=true | proto=None => provider=openai | base=Some("") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
gemini | in="https://proxy.example.com/gemini/chat" | full=true | proto=None => provider=openai | base=Some("https://proxy.example.com/gemini/chat") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
ark | in="https://proxy.example.com/ark/chat" | full=true | proto=None => provider=openai | base=Some("https://proxy.example.com/ark/chat") | api_path=Some("") | max_tokens=None | image=None | reasoning=None
"#;
}

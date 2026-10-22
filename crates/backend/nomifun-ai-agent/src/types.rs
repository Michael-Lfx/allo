use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use nomifun_api_types::NomiToolPolicy;
use nomifun_common::{
    AgentType, ConversationId, DelegationPolicy, ProviderId, ProviderWithModel, UserId,
};
use nomifun_knowledge::WorkspaceBindingLease;

fn deserialize_user_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    UserId::parse(&value).map_err(serde::de::Error::custom)?;
    Ok(value)
}

fn deserialize_conversation_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    ConversationId::parse(&value).map_err(serde::de::Error::custom)?;
    Ok(value)
}

/// Data payload for sending a user message to an Agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageData {
    /// User message content.
    pub content: String,
    /// Client-generated message ID for correlation.
    pub msg_id: String,
    /// Durable root user-message identity shared by every automatic
    /// continuation or provider retry for the same logical turn.
    ///
    /// Older/custom callers omit it and fall back to `msg_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_message_id: Option<String>,
    /// File paths attached to the message.
    #[serde(default)]
    pub files: Vec<String>,
    /// Skills to inject into this message turn.
    #[serde(default)]
    pub inject_skills: Vec<String>,
    /// Immutable `SKILL.md` snapshots resolved by the conversation boundary
    /// for this turn. They are plain instruction text, never executable hooks.
    #[serde(default)]
    pub loaded_skill_snapshots: Vec<LoadedSkillSnapshot>,
    /// Host-ranked catalog hint for this user turn (L1 assist, not full bodies).
    #[serde(default)]
    pub catalog_discovery: Option<CatalogDiscoveryHint>,
    /// Turn origin marker (companion/cron/autowork/idmm). `None`/empty = a human
    /// owner is speaking. Same semantics as the collector's `payload_origin`
    /// red line: non-empty origins are NOT human intent and must not be
    /// distilled into file-based memory.
    #[serde(default)]
    pub origin: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoadedSkillSnapshot {
    pub skill_id: String,
    pub name: String,
    pub source: String,
    pub version_hash: String,
    pub content: String,
    /// Absolute SKILL.md path when known. The procedure itself is inlined into
    /// the user turn; this path is only for supporting files next to it.
    #[serde(default)]
    pub location: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CatalogDiscoveryHint {
    pub ranked_ids: Vec<String>,
    pub listing_truncated: bool,
    pub method: String,
    pub entries: Vec<CatalogDiscoveryEntry>,
    #[serde(default)]
    pub applied_to_l1: bool,
    #[serde(default)]
    pub latency_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CatalogDiscoveryEntry {
    pub skill_id: String,
    pub name: String,
    pub description: String,
}

/// Character budget for one explicitly selected Skill procedure.
///
/// The user already chose this Skill, so the turn includes its instructions
/// instead of only a filesystem pointer. Larger files keep a prefix and point
/// at `SKILL.md` for the rest.
const SKILL_INSTRUCTION_CHAR_LIMIT: usize = 32_000;

pub fn inject_loaded_skill_context(content: String, skills: &[LoadedSkillSnapshot]) -> String {
    if skills.is_empty() {
        return content;
    }
    let sections = skills
        .iter()
        .map(render_loaded_skill)
        .collect::<Vec<_>>()
        .join("\n\n");
    format!(
        "[Loaded Skills]\n\
         The user explicitly selected these Skills (force). The instructions below are already loaded and in effect for this turn.\n\
         Follow those instructions before taking task actions. Do not call the Skill tool to reload a skill listed here — it is already selected.\n\
         Other catalog skills that were not selected may still be loaded with the Skill tool when they match the task.\n\
         If a selected Skill names another catalog Skill, call the Skill tool with that other catalog id before continuing.\n\
         If a selected Skill refers to a slash command, perform the described workflow directly instead of passing that slash command to the Skill tool.\n\
         Host tools named by a Skill (for example briefing_create) remain available even if they are not quoted below.\n\
         Prefer those tools over reconstructing the workflow with web_search or nomi_delegate.\n\
         If a selected Skill does not match the user request, say so and use the matching host tool instead.\n\n\
         {sections}\n\
         [/Loaded Skills]\n\n\
         [Current User Request]\n\
         {content}"
    )
}

pub fn inject_catalog_discovery_context(
    content: String,
    hint: Option<&CatalogDiscoveryHint>,
) -> String {
    let Some(hint) = hint else {
        return content;
    };
    if !hint.applied_to_l1 || hint.entries.is_empty() {
        return content;
    }
    let mut lines = vec![
        "[Catalog Skills this turn]".to_owned(),
        "These catalog skills are the best matches for the current user request.".to_owned(),
        "If one matches, call the Skill tool with its catalog id before acting.".to_owned(),
        "Skip any skill already listed under [Loaded Skills].".to_owned(),
    ];
    if hint.listing_truncated {
        lines.push(
            "The system listing is truncated; prefer this ranked subset, or SkillSearch if available."
                .to_owned(),
        );
    }
    for entry in &hint.entries {
        lines.push(format!(
            "- {} (`{}`): {}",
            entry.name, entry.skill_id, entry.description
        ));
    }
    if lines.iter().all(|line| !line.starts_with("- ")) {
        for id in &hint.ranked_ids {
            lines.push(format!("- `{id}`"));
        }
    }
    lines.push("[/Catalog Skills this turn]".to_owned());
    format!("{}\n\n{content}", lines.join("\n"))
}

fn render_loaded_skill(skill: &LoadedSkillSnapshot) -> String {
    let (frontmatter, body) = split_skill_frontmatter(&skill.content);
    let description = frontmatter_field(&frontmatter, "description")
        .or_else(|| frontmatter_field(&frontmatter, "when_to_use"))
        .unwrap_or_default();
    let location = skill
        .location
        .as_deref()
        .or_else(|| frontmatter_field(&frontmatter, "location"))
        .unwrap_or("");
    let mut lines = vec![
        format!("[Loaded Skill: {}]", skill.name),
        format!("Source: {}", skill.source),
        format!("Version: {}", skill.version_hash),
    ];
    if !description.is_empty() {
        lines.push(format!("Description: {description}"));
    }
    if !location.is_empty() {
        lines.push(format!("Path: {location}"));
    }
    let procedure = {
        let trimmed_body = body.trim();
        if trimmed_body.is_empty() {
            skill.content.trim()
        } else {
            trimmed_body
        }
    };
    let (instructions, truncated) = bound_skill_instructions(procedure);
    if !instructions.is_empty() {
        lines.push(String::new());
        lines.push(instructions);
    }
    if truncated && !location.is_empty() {
        lines.push(format!(
            "The instructions above are truncated. Read `{location}` before acting on anything that is not fully included."
        ));
    } else if truncated {
        lines.push(
            "The instructions above are truncated. Continue only from the included steps and do not invent the missing ones."
                .to_owned(),
        );
    } else if !location.is_empty() {
        lines.push(format!(
            "Open `{location}` only when these instructions name a supporting file."
        ));
    }
    lines.push("[/Loaded Skill]".to_owned());
    lines.join("\n")
}

fn split_skill_frontmatter(content: &str) -> (&str, &str) {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return ("", content);
    }
    let after_open = &trimmed[3..];
    let after_open = after_open
        .strip_prefix('\n')
        .or_else(|| after_open.strip_prefix("\r\n"))
        .unwrap_or(after_open);
    let Some(end) = after_open.find("\n---") else {
        return ("", content);
    };
    let frontmatter = &after_open[..end];
    let rest = &after_open[end + 4..];
    let body = rest
        .strip_prefix('\n')
        .or_else(|| rest.strip_prefix("\r\n"))
        .unwrap_or(rest);
    (frontmatter, body)
}

fn frontmatter_field<'a>(frontmatter: &'a str, key: &str) -> Option<&'a str> {
    for line in frontmatter.lines() {
        let line = line.trim();
        let Some((found_key, value)) = line.split_once(':') else {
            continue;
        };
        if found_key.trim() == key {
            let value = value.trim().trim_matches('"').trim_matches('\'');
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

fn bound_skill_instructions(body: &str) -> (String, bool) {
    if body.is_empty() {
        return (String::new(), false);
    }
    if body.chars().count() <= SKILL_INSTRUCTION_CHAR_LIMIT {
        return (body.to_owned(), false);
    }
    let mut excerpt = body
        .chars()
        .take(SKILL_INSTRUCTION_CHAR_LIMIT)
        .collect::<String>();
    excerpt.push('…');
    (excerpt, true)
}

/// Attach the immutable conversation preset to the first prompt understood by
/// runtimes that do not expose a native system-prompt channel.
///
/// The caller decides what "first" means for its transport/session lifecycle.
/// Keeping the envelope identical across adapters makes the active contract
/// explicit to both the model and runtime-level tests.
pub(crate) fn inject_runtime_preset_context(
    content: String,
    preset_context: Option<&str>,
    should_inject: bool,
) -> String {
    if !should_inject {
        return content;
    }
    let Some(context) = preset_context
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return content;
    };
    format!(
        "[Assistant Rules]\n{context}\n[/Assistant Rules]\n\n\
         [Current User Request]\n{content}"
    )
}

/// Options for creating or resuming a per-conversation Agent runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRuntimeBuildOptions {
    /// First-class owner of the runtime. Production callers copy this from the
    /// authoritative Conversation/Cron target row; factories fail closed when
    /// it is empty and never accept an identity from type-specific `extra`.
    #[serde(deserialize_with = "deserialize_user_id")]
    pub user_id: String,
    /// Type of agent to create.
    pub agent_type: AgentType,
    /// Working directory for the agent.
    pub workspace: String,
    /// Model selection config. Nomi runtimes require this; runtimes whose
    /// backend owns model selection (for example ACP) keep it absent instead
    /// of using an empty provider/model sentinel.
    pub model: Option<ProviderWithModel>,
    /// Conversation ID this runtime belongs to.
    #[serde(deserialize_with = "deserialize_conversation_id")]
    pub conversation_id: String,
    /// Typed conversation-level delegation policy. This is sourced from the
    /// first-class conversation column and takes precedence over type-specific
    /// JSON so execution policy never has two authorities.
    #[serde(default)]
    pub delegation_policy: DelegationPolicy,
    /// Type-specific extra parameters (JSON object).
    #[serde(default)]
    pub extra: serde_json::Value,
    /// Owning conversation row's `created_at` (ms). Stable per conversation
    /// instance and mandatory for persisted Nomi runtimes. It stamps and
    /// validates the session owner token so derived state never crosses an
    /// entity lifetime.
    #[serde(default)]
    pub conversation_created_at: Option<i64>,
    /// Process-local authority over this runtime's physical
    /// `.nomi/knowledge` namespace.  It is never serialized or supplied by a
    /// client.  The runtime registry transfers it from build options to the
    /// exact runtime slot before the factory starts and retains it until
    /// process teardown is proven.
    #[serde(skip)]
    pub workspace_binding_lease: Option<WorkspaceBindingLease>,
}

/// Provider-specific compat overrides resolved in the factory.
#[derive(Debug, Clone, Default)]
pub struct NomiCompatOverrides {
    pub max_tokens_field: Option<String>,
    pub api_path: Option<String>,
    /// None = 默认支持图片;Some(false) = registry 已标记不支持,发送时剔图。
    pub supports_image: Option<bool>,
    /// Duplicate Bearer into this header (Flowy Cloud).
    pub mirror_bearer_header: Option<String>,
    /// Some(true) = gateway requires assistant reasoning_content placeholders.
    pub require_reasoning_content: Option<bool>,
    /// Some(true) = omit assistant reasoning that precedes the last user message.
    pub drop_prior_turn_reasoning: Option<bool>,
    /// Catalog-advertised OpenAI-style `reasoning_effort` support. When set,
    /// overrides the provider-type default from [`nomi_config::ProviderCompat`].
    pub supports_effort: Option<bool>,
    /// Allowed effort levels from the Flowy catalog (`extra.reasoning_effort`).
    pub effort_levels: Option<Vec<String>>,
    /// Explicit opt-in for OpenAI Responses provider-side round retention.
    /// Request builders still apply their own lifecycle gate.
    pub chain_rounds: Option<bool>,
    /// Provider-native request body fields after local Agent controls have
    /// been removed. Typed serializer fields overwrite conflicts at send time.
    pub extra_body: Option<serde_json::Map<String, serde_json::Value>>,
}

/// A separately resolved multimodal model used only to analyze image
/// attachments for text-only Nomi conversation models.
#[derive(Debug, Clone)]
pub struct ImageAnalysisModelConfig {
    pub config: nomi_config::config::Config,
    pub label: String,
}

/// Fully resolved Nomi configuration passed to the agent manager.
#[derive(Debug, Clone)]
pub struct NomiResolvedConfig {
    /// Canonical provider row id (UUID) backing this session's model. Kept
    /// alongside the nomi provider name so UUID-keyed downstream services
    /// (e.g. learning course generation) can honor the conversation model
    /// instead of re-resolving a default.
    pub provider_id: ProviderId,
    /// LLM provider name (anthropic, openai, bedrock, vertex).
    pub provider: String,
    /// Decrypted API key.
    pub api_key: String,
    /// Model identifier.
    pub model: String,
    /// Provider base URL.
    pub base_url: Option<String>,
    /// System prompt override (persona / preset / knowledge — not language).
    pub system_prompt: Option<String>,
    /// Output-language directive placed last in the cache-stable system prompt
    /// so it wins over the English intro and any coding overlay.
    pub output_language_directive: Option<String>,
    /// Capability-declared output ceiling. `None` means omit it where the
    /// protocol permits; required protocols fail before a turn starts.
    pub output_ceiling: Option<u32>,
    /// Max agentic turns.
    pub max_turns: Option<usize>,
    /// Provider's declared context window (tokens), if configured. Drives the
    /// engine's compaction window and the context-usage gauge denominator.
    pub context_limit: Option<u64>,
    /// Provider-specific compat overrides.
    pub compat_overrides: NomiCompatOverrides,
    /// Optional independent vision model. It is used only when the selected
    /// conversation model does not accept image input.
    pub image_analysis_model: Option<ImageAnalysisModelConfig>,
    /// Directory for nomi session persistence files.
    pub session_directory: PathBuf,
    /// Session mode (default, auto_edit, yolo).
    pub session_mode: Option<String>,
    /// Frozen workspace restriction for trusted voice-owned Conversations.
    pub voice_workspace: Option<PathBuf>,
    /// Session-scoped MCP servers to inject.
    pub extra_mcp_servers: HashMap<String, nomi_config::config::McpServerConfig>,
    /// Process-local guards for renewable loopback MCP capabilities. These are
    /// never serialized; the Nomi manager holds them until runtime teardown.
    pub loopback_capability_leases: nomifun_common::LoopbackCapabilityLeaseSet,
    /// AWS Bedrock credentials (region + access key or profile).
    pub bedrock_config: Option<nomi_config::config::BedrockConfig>,
    /// Enable the Computer tool (screen/mouse/keyboard control).
    pub computer_use: bool,
    /// Enable Browser tools backed by a main-process `BrowserLaneClient`.
    /// This runtime never owns Chromium or a browser profile.
    pub browser_use: bool,
    /// **浏览器来源 LIVE 值**（Browser Host 可执行文件偏好，与 silent 正交）。`"managed"` =
    /// 内置/下载 CfT；`"system"`（默认）= 系统 Chrome/Edge 本体优先（未探到回退 managed）。
    /// 工厂经 `read_string_pref` LIVE 读 `agent.browserUse.source`（host_default=`"system"`）。
    /// 主进程 `BrowserSessionHub` 仍是唯一 Host/profile owner：Primary 使用应用管理的稳定
    /// profile，Crawl Host 使用临时 profile，runtime 不拥有独立 Chromium。
    pub browser_source: String,
    /// **F1-sec: browser-use evaluate「全权模式」LIVE 值**（裁决⑨，default-deny）。`true` 当且仅当
    /// 用户在 System Settings 显式 opt-in（`client_preferences` `agent.browserUse.fullPower`，工厂经
    /// `read_bool_pref` 范式 LIVE 读）。`false`（默认）→ 引擎 `evaluate` 动作返 `Unsupported`。**绝不看
    /// session_mode**（yolo/companion 无从豁免，不变量⑧）。
    pub browser_full_power: bool,
    /// **SD-6: browser-use 持久登录 LIVE 值**（DESIGN §16/§27 互斥约束）。`true`（产品默认）→ 与全权
    /// 互斥（evaluate 在两者皆 true 时 Blocked）。工厂经 `read_bool_pref` 范式 LIVE 读
    /// `agent.browserUse.persistentLogin`（host_default=true）。`false` → 互斥不生效（evaluate 仅受
    /// full_power 开关控制）。代码级 Default = `false`（与 full_power 同范式 default-deny 基线）。
    pub browser_persistent_login: bool,
    /// **P7A site-memory LIVE 值**（opt-in，隐私相关）。`true` → bootstrap 给 Hub-backed
    /// Browser tool adapter 注入文件型 `SiteMemorySink`（跨会话记住站点结构 + 向 observe
    /// 注入 hints）。工厂经 `read_bool_pref` 范式 LIVE 读 `agent.browserUse.siteMemory`
    /// （host_default=**false**=OFF）。`false`（默认）→ 不挂 sink，零行为变化。
    pub browser_site_memory: bool,
    /// **Phase D takeover/审批 LIVE 值**（opt-in，安全）。`true` → 桌面会话构造期注入
    /// `DesktopApprovalGate`：不可逆动作（bypass 会话）+ 被门控跨域 POST（SD-5）浮给用户审批后
    /// 才放行（否则 fail-closed 硬挡）。工厂经 `read_bool_pref` LIVE 读 `agent.browserUse.takeover`
    /// （host_default=**false**=OFF）。`false`（默认）→ 不注入 gate，维持 fail-closed 零回归。
    pub browser_takeover: bool,
    /// Explicit Browser Use approval bypass. Default false. When true, Browser-specific
    /// irreversible and egress approval prompts approve immediately.
    pub browser_unrestricted_approval: bool,
    /// **P7B visual-fallback LIVE 值**（opt-in，有 token 成本）。`true` → bootstrap 给
    /// Hub-backed Browser tool adapter 注入会话模型的 `VisualLocator`：DOM/aria 锚定失败
    /// （ref stale/detached）时截图交视觉模型按描述定位再点。工厂经 `read_bool_pref` 范式
    /// LIVE 读 `agent.browserUse.visualFallback`（host_default=**false**=OFF）。`false`
    /// （默认）→ 不注入 locator，适配层保持 Unavailable（零行为变化）。
    pub browser_visual_fallback: bool,
    /// Opt-in goal-driven continuation (objective + auto-continuation cap).
    /// `None` (default) = normal one-shot turn behavior.
    pub goal: Option<nomi_agent::goal::runtime::GoalSpec>,
    /// Restore-semantics goal snapshot (persisted DB row or an explicit
    /// `resume_state` carried in the build extra). When `Some`, the manager
    /// injects it via `engine.set_goal_state` right after bootstrap — status /
    /// turns_used / breaker counters / created_at are taken as-is. Wins over
    /// `goal` (the fresh-start spec) because `set_goal_state` swaps in place.
    pub goal_resume_state: Option<nomi_agent::goal::state::GoalState>,
    /// Opt-in Mixture-of-Agents fan-out: bridged engine config + host-resolved
    /// reference slots. `None` (default) = single-model behavior; the manager
    /// injects a fresh `MoaState` into the engine only when `Some`.
    pub moa: Option<ResolvedMoaBridge>,
    /// Shared browser secret-vault descriptor (vault path + machine-bound key).
    /// Bootstrap passes it to the Hub-backed Browser tool policy so
    /// user-registered `secret:NAME` values resolve under origin checks and
    /// contribute their `allowed_origins` to the egress firewall. This is a
    /// shared policy store, not a browser profile or per-runtime Chromium owner.
    /// `None` (browser-use off / probe sessions) keeps the compatibility empty
    /// store behavior. The raw key is carried without a `nomi_browser` type so
    /// this crate needs no `nomi-browser` dependency.
    pub browser_secret_vault: Option<BrowserSecretVault>,
    /// Stable identity of the owning conversation instance (the conversation
    /// row's `created_at`, stringified). Persisted Nomi runtimes always provide
    /// it; probe-only runtimes may leave it absent because they do not resume a
    /// conversation session.
    pub owner_token: Option<String>,
    /// Backend-authoritative host composition switch. Platform Gateway and
    /// secondary-user sessions leave embedded AgentExecution uninstalled;
    /// trusted no-gateway standalone sessions install it. This is
    /// internal runtime state and is never serialized as user configuration.
    pub install_embedded_agent_execution: bool,
    /// Per-session 工具白名单（空 = 不限制），源自 `NomiBuildExtra.allowed_tools`，
    /// 由 manager 灌进 `config.tools.builtin_allowlist`。
    pub allowed_tools: Vec<String>,
    /// 宿主级工具策略（agent-store 的 `[tools]`），由 `AgentFactoryDeps` 注入。
    ///
    /// 它是**宿主策略而非会话输入**：不走 `NomiBuildExtra`（客户端 JSON），且每个
    /// 字段只能收窄。manager 在 `Config::resolve` 之后用它落会话级开关
    /// (`web`/`plan`/`lsp`)、`builtin_denylist`，并决定 media 是否接线；
    /// 默认值（[`NomiToolPolicy::default`]）不约束任何东西，故未采纳该策略的
    /// 宿主行为与之前完全一致。
    pub tool_policy: NomiToolPolicy,
    /// 内置（文件型）记忆系统的宿主总开关（agent-store 的 `[memory] enabled`），
    /// 由 `AgentFactoryDeps` 注入。`false` 时 manager 不解析 distill 目标目录
    /// （不蒸馏、不回写引用计数），bootstrap 也不注入记忆提示词段落、不注册
    /// `remember` 工具。它是**宿主策略而非会话输入**（不走 `NomiBuildExtra`），
    /// 默认 `true` = 与之前完全一致。注意它比
    /// `manager::nomi::distill` 的 `distill_enabled` 更宽：后者只管轮后蒸馏。
    pub memory_enabled: bool,
    /// 原生文件工具（Write/Edit/ApplyPatch）的写根钳制，按会话**信任面**解析：
    /// 本地桌面（`Private` 且非渠道）= `None`（OS 用户全权，不钳制，今日行为）；
    /// 渠道 / 远程 / 对外 = `Some(workspace)`（收窄到会话工作区，堵住对外面过度开放）。
    /// manager 灌进 `config.tools.write_root`。与 gateway file-service 的
    /// `PathAuthority` 同一信任模型（见 file-access-authority spec）。
    pub write_root: Option<String>,
    /// User-selected OpenAI-style `reasoning_effort` for this session.
    /// Validated against catalog/`compat` effort levels in the factory; `None`
    /// means the engine omits the field (provider default).
    pub reasoning_effort: Option<String>,
    /// Session work mode (`office` | `coding`). Parsed by
    /// [`nomi_agent::TaskProfile::parse`]; `None` means office default.
    pub task_profile: Option<String>,
    /// Coding verification: `soft_hint` | `hard_gate` | `off`.
    pub coding_verification: Option<String>,
    pub coding_protect_read: Option<bool>,
    pub coding_micro_keep_recent: Option<usize>,
}

/// Host-resolved Mixture-of-Agents payload carried from the factory to the
/// nomi manager. Kept as (config, slots) parts — not a ready `MoaState` — so
/// `NomiResolvedConfig` stays `Clone` and the manager builds a fresh state
/// (turn bookkeeping starts empty) via `MoaState::new` at engine assembly.
#[derive(Debug, Clone)]
pub struct ResolvedMoaBridge {
    pub config: nomi_config::config::MoaConfig,
    pub slots: Vec<nomi_agent::moa::MoaResolvedSlot>,
    /// Per-slot catalog pricing (USD per million tokens), aligned to `slots`
    /// by label. A slot with no catalog price carries `None` costs, so the
    /// manager reports token counts without a cost figure for it.
    pub slot_prices: Vec<MoaSlotPrice>,
    /// Where to append per-message MoA trace records
    /// (`<data_dir>/moa-trace/<conversation_id>.jsonl`). `None` = tracing off.
    pub trace_path: Option<std::path::PathBuf>,
}

/// Catalog pricing for one MoA reference slot (USD per million tokens).
/// `None` = the models.dev catalog has no price for that side, so the turn
/// stats report tokens without a dollar figure.
#[derive(Debug, Clone)]
pub struct MoaSlotPrice {
    /// Slot label, matching `MoaResolvedSlot::label` (`"provider_id/model"`).
    pub label: String,
    pub cost_input: Option<f64>,
    pub cost_output: Option<f64>,
}

/// Shared browser secret-vault location plus its machine-bound key.
///
/// One application vault serves all callers. It contains policy-managed
/// credentials, not the Primary identity profile; Chromium/profile ownership
/// remains exclusively in the main-process `BrowserSessionHub`. Debug redacts
/// the key so it never lands in a `NomiResolvedConfig` log line.
#[derive(Clone)]
pub struct BrowserSecretVault {
    /// The shared secret vault file path
    /// (`{data_dir}/browser-secrets/shared/secrets.json`).
    pub vault_path: std::path::PathBuf,
    /// The machine-bound AES-256-GCM `encryption_key` (32 bytes).
    pub key: [u8; 32],
}

impl std::fmt::Debug for BrowserSecretVault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrowserSecretVault")
            .field("vault_path", &self.vault_path)
            .field("key", &"<redacted>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomifun_api_types::{
        AcpBuildExtra, AcpModelInfo, NomiBuildExtra, OpenClawGatewayConfig, SlashCommandItem,
    };
    use serde_json::json;

    #[test]
    fn runtime_preset_context_is_injected_only_when_requested() {
        let injected = inject_runtime_preset_context(
            "write the copy".to_owned(),
            Some("Preset: Copywriter r3"),
            true,
        );
        assert!(injected.contains("[Assistant Rules]"));
        assert!(injected.contains("Preset: Copywriter r3"));
        assert!(injected.ends_with("write the copy"));

        assert_eq!(
            inject_runtime_preset_context(
                "second turn".to_owned(),
                Some("Preset: Copywriter r3"),
                false,
            ),
            "second turn"
        );
        assert_eq!(
            inject_runtime_preset_context("plain".to_owned(), Some("  "), true),
            "plain"
        );
    }

    #[test]
    fn loaded_skill_snapshots_are_injected_as_instruction_context() {
        let content = inject_loaded_skill_context(
            "Summarize the document".to_owned(),
            &[LoadedSkillSnapshot {
                skill_id: "user:pdf".to_owned(),
                name: "pdf".to_owned(),
                source: "user".to_owned(),
                version_hash: "abc123".to_owned(),
                content: "Inspect the PDF before answering.".to_owned(),
                location: None,
            }],
        );

        assert!(content.contains("[Loaded Skills]"));
        assert!(content.contains("The instructions below are already loaded and in effect"));
        assert!(content.contains("Do not call the Skill tool to reload a skill listed here"));
        assert!(content.contains("Other catalog skills that were not selected may still be loaded"));
        assert!(content.contains("perform the described workflow directly"));
        assert!(content.contains("Inspect the PDF before answering."));
        assert!(content.contains("Prefer those tools over reconstructing the workflow"));
        assert!(content.ends_with("Summarize the document"));
    }

    #[test]
    fn loaded_skill_injection_includes_the_selected_procedure() {
        let body = "Render the narration as motion graphics before answering.";
        let content = inject_loaded_skill_context(
            "做个当日新闻口播".to_owned(),
            &[LoadedSkillSnapshot {
                skill_id: "builtin:art-motion".to_owned(),
                name: "art-motion".to_owned(),
                source: "builtin".to_owned(),
                version_hash: "def456".to_owned(),
                content: format!(
                    "---\nname: art-motion\ndescription: Motion graphics from narration\n---\n{body}"
                ),
                location: Some("C:/data/builtin-skills/art-motion/SKILL.md".to_owned()),
            }],
        );
        assert!(content.contains("Description: Motion graphics from narration"));
        assert!(content.contains("Path: C:/data/builtin-skills/art-motion/SKILL.md"));
        assert!(content.contains(&body));
        assert!(content.contains("Open `C:/data/builtin-skills/art-motion/SKILL.md` only when"));
        assert!(content.contains("briefing_create"));
        assert!(content.ends_with("做个当日新闻口播"));
    }

    #[test]
    fn loaded_skill_without_a_path_still_includes_the_procedure() {
        let body = "Lock shared geography before choosing a camera.";
        let content = inject_loaded_skill_context(
            "排这场戏".to_owned(),
            &[LoadedSkillSnapshot {
                skill_id: "builtin:scene-directing".to_owned(),
                name: "scene-directing".to_owned(),
                source: "builtin".to_owned(),
                version_hash: "abc123".to_owned(),
                content: format!(
                    "---\nname: scene-directing\ndescription: Direct the scene\n---\n{body}"
                ),
                location: None,
            }],
        );
        assert!(content.contains(&body));
        assert!(!content.contains("Read that file"));
        assert!(content.ends_with("排这场戏"));
    }

    #[test]
    fn oversized_selected_skill_is_truncated_and_points_at_the_file() {
        let body = "y".repeat(SKILL_INSTRUCTION_CHAR_LIMIT + 40);
        let content = inject_loaded_skill_context(
            "继续".to_owned(),
            &[LoadedSkillSnapshot {
                skill_id: "builtin:officecli-docx".to_owned(),
                name: "officecli-docx".to_owned(),
                source: "builtin".to_owned(),
                version_hash: "fff".to_owned(),
                content: format!("---\nname: officecli-docx\ndescription: Word documents\n---\n{body}"),
                location: Some("C:/data/builtin-skills/officecli-docx/SKILL.md".to_owned()),
            }],
        );
        assert!(!content.contains(&body));
        assert!(content.contains("The instructions above are truncated."));
        assert!(content.contains("Read `C:/data/builtin-skills/officecli-docx/SKILL.md`"));
    }

    #[test]
    fn catalog_discovery_is_omitted_while_shadowing() {
        let hint = CatalogDiscoveryHint {
            ranked_ids: vec!["builtin:officecli".to_owned()],
            listing_truncated: true,
            method: "weighted_lexical_v1".to_owned(),
            entries: vec![CatalogDiscoveryEntry {
                skill_id: "builtin:officecli".to_owned(),
                name: "officecli".to_owned(),
                description: "Office documents".to_owned(),
            }],
            applied_to_l1: false,
            latency_ms: 2,
        };
        assert_eq!(
            inject_catalog_discovery_context("Write a workbook".to_owned(), Some(&hint)),
            "Write a workbook"
        );
    }

    #[test]
    fn catalog_discovery_lists_ranked_ids_when_applied() {
        let hint = CatalogDiscoveryHint {
            ranked_ids: vec!["builtin:officecli".to_owned()],
            listing_truncated: true,
            method: "weighted_lexical_v1".to_owned(),
            entries: vec![CatalogDiscoveryEntry {
                skill_id: "builtin:officecli".to_owned(),
                name: "officecli".to_owned(),
                description: "Office documents".to_owned(),
            }],
            applied_to_l1: true,
            latency_ms: 3,
        };
        let content =
            inject_catalog_discovery_context("Write a workbook".to_owned(), Some(&hint));
        assert!(content.contains("[Catalog Skills this turn]"));
        assert!(content.contains("builtin:officecli"));
        assert!(content.contains("call the Skill tool"));
        assert!(content.ends_with("Write a workbook"));
    }

    #[test]
    fn acp_build_extra_accepts_payload_without_skills() {
        let legacy = r#"{"backend":"claude"}"#;
        let parsed: AcpBuildExtra = serde_json::from_str(legacy).unwrap();
        assert!(parsed.skills.is_empty());
    }

    #[test]
    fn acp_build_extra_accepts_skills() {
        let with_field = r#"{"backend":"claude","skills":["cron","pdf"]}"#;
        let parsed: AcpBuildExtra = serde_json::from_str(with_field).unwrap();
        assert_eq!(parsed.skills, vec!["cron".to_owned(), "pdf".to_owned()]);
    }

    #[test]
    fn send_message_data_serde_roundtrip() {
        let data = SendMessageData {
            content: "Hello".into(),
            msg_id: "msg-001".into(),
            source_message_id: Some("root-001".into()),
            files: vec!["/tmp/a.txt".into()],
            inject_skills: vec!["review".into()],
            loaded_skill_snapshots: vec![],
            catalog_discovery: None,
            origin: None,
        };
        let json = serde_json::to_value(&data).unwrap();
        assert_eq!(json["content"], "Hello");
        assert_eq!(json["msg_id"], "msg-001");
        assert_eq!(json["source_message_id"], "root-001");
        assert_eq!(json["files"], json!(["/tmp/a.txt"]));
        assert_eq!(json["inject_skills"], json!(["review"]));

        let parsed: SendMessageData = serde_json::from_value(json).unwrap();
        assert_eq!(parsed.content, "Hello");
        assert_eq!(parsed.msg_id, "msg-001");
        assert_eq!(parsed.source_message_id.as_deref(), Some("root-001"));
    }

    #[test]
    fn send_message_data_defaults_optional_fields() {
        let json = json!({ "content": "Hi", "msg_id": "m1" });
        let data: SendMessageData = serde_json::from_value(json).unwrap();
        assert!(data.files.is_empty());
        assert!(data.inject_skills.is_empty());
        assert!(data.loaded_skill_snapshots.is_empty());
        assert!(data.origin.is_none());
        assert!(data.source_message_id.is_none());
        assert!(data.catalog_discovery.is_none());
    }

    #[test]
    fn send_message_data_origin_roundtrips() {
        let json = json!({ "content": "Hi", "msg_id": "m1", "origin": "cron" });
        let data: SendMessageData = serde_json::from_value(json).unwrap();
        assert_eq!(data.origin.as_deref(), Some("cron"));
    }

    #[test]
    fn agent_runtime_build_options_serde() {
        let opts = AgentRuntimeBuildOptions {
            user_id: "0190f5fe-7c00-7a00-8000-000000000001".into(),
            agent_type: AgentType::Acp,
            workspace: "/project".into(),
            model: Some(ProviderWithModel {
                provider_id: "0190f5fe-7c00-7a00-8000-000000000001".into(),
                model: "claude-sonnet".into(),
                use_model: None,
            }),
            conversation_id: "0190f5fe-7c00-7a00-8000-000000000001".into(),
            delegation_policy: DelegationPolicy::Automatic,
            extra: json!({ "backend": "claude" }),
            conversation_created_at: None,
            workspace_binding_lease: None,
        };
        let json = serde_json::to_value(&opts).unwrap();
        assert_eq!(json["agent_type"], "acp");
        assert_eq!(json["user_id"], "0190f5fe-7c00-7a00-8000-000000000001");
        assert_eq!(json["workspace"], "/project");
        assert_eq!(
            json["conversation_id"],
            "0190f5fe-7c00-7a00-8000-000000000001"
        );
        assert_eq!(json["delegation_policy"], "automatic");
    }

    #[test]
    fn acp_model_info_serde() {
        let info = AcpModelInfo {
            model_id: "claude-sonnet-4".into(),
            model_name: Some("Claude Sonnet 4".into()),
            provider: Some("anthropic".into()),
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["model_id"], "claude-sonnet-4");
        assert_eq!(json["model_name"], "Claude Sonnet 4");
    }

    #[test]
    fn slash_command_item_serde() {
        let cmd = SlashCommandItem {
            command: "/review".into(),
            description: "Code review".into(),
            origin: nomifun_api_types::SlashCommandOrigin::Agent,
        };
        let json = serde_json::to_value(&cmd).unwrap();
        assert_eq!(json["command"], "/review");
    }

    #[test]
    fn openclaw_gateway_config_defaults() {
        let json = json!({});
        let config: OpenClawGatewayConfig = serde_json::from_value(json).unwrap();
        assert!(!config.use_external_gateway);
        assert!(config.host.is_none());
        assert!(config.port.is_none());
    }

    #[test]
    fn nomi_build_extra_serde_with_preset_rules() {
        let json = json!({
            "preset_rules": "You are a data analyst."
        });
        let extra: NomiBuildExtra = serde_json::from_value(json).unwrap();
        assert!(extra.system_prompt.is_none());
        assert_eq!(extra.preset_rules.unwrap(), "You are a data analyst.");
    }

    #[test]
    fn runtime_options_deserialization_rejects_noncanonical_entity_ids() {
        let base = serde_json::json!({
            "user_id": "0190f5fe-7c00-7a00-8000-000000000001",
            "agent_type": "nomi",
            "workspace": "/tmp",
            "model": null,
            "conversation_id": "0190f5fe-7c00-7a00-8000-000000000001"
        });
        assert!(serde_json::from_value::<AgentRuntimeBuildOptions>(base.clone()).is_ok());
        let mut invalid_user = base.clone();
        invalid_user["user_id"] = serde_json::json!("1");
        assert!(serde_json::from_value::<AgentRuntimeBuildOptions>(invalid_user).is_err());
        let mut invalid_conversation = base;
        invalid_conversation["conversation_id"] = serde_json::json!("1");
        assert!(serde_json::from_value::<AgentRuntimeBuildOptions>(invalid_conversation).is_err());
    }
}

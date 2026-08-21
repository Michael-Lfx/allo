use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
}

pub fn inject_loaded_skill_context(content: String, skills: &[LoadedSkillSnapshot]) -> String {
    if skills.is_empty() {
        return content;
    }
    let sections = skills
        .iter()
        .map(|skill| {
            format!(
                "[Loaded Skill: {}]\nSource: {}\nVersion: {}\n\n{}\n[/Loaded Skill]",
                skill.name, skill.source, skill.version_hash, skill.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    format!(
        "[Loaded Skills]\n\
         The user explicitly selected these Skills. Their instructions are already active for this turn.\n\
         Apply them directly. Do not call the Skill tool to load or invoke a selected Skill by name;\n\
         that tool only exposes the runtime-native Skill catalog and may not contain this selection.\n\
         If a selected Skill refers to a slash command or another Skill, perform the described workflow directly\n\
         instead of using the Skill tool to load that reference.\n\n\
         {sections}\n\
         [/Loaded Skills]\n\n\
         [Current User Request]\n\
         {content}"
    )
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
    /// Catalog-advertised OpenAI-style `reasoning_effort` support. When set,
    /// overrides the provider-type default from [`nomi_config::ProviderCompat`].
    pub supports_effort: Option<bool>,
    /// Allowed effort levels from the Flowy catalog (`extra.reasoning_effort`).
    pub effort_levels: Option<Vec<String>>,
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
    /// System prompt override.
    pub system_prompt: Option<String>,
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
            }],
        );

        assert!(content.contains("[Loaded Skills]"));
        assert!(content.contains("instructions are already active for this turn"));
        assert!(content.contains("Do not call the Skill tool to load or invoke a selected Skill"));
        assert!(content.contains("perform the described workflow directly"));
        assert!(content.contains("Inspect the PDF before answering."));
        assert!(content.ends_with("Summarize the document"));
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

//! `~/.agent-store/config.toml` — the local agent-store settings file.
//!
//! The App Server treats this file as an *optional* integration surface so it
//! can drive a real Nomi conversation without first creating providers through
//! the full Allo UI. The file lives next to the user's other agent tooling
//! (Claude Code / Codex style `config.toml`), and its `[providers.<name>]` +
//! `[models."<provider>/<model>"]` tables map onto the App Server's provider
//! model selection:
//!
//! ```toml
//! default_model = "opencode/mimo-v2.5-free"
//!
//! [providers.opencode]
//! type = "openai"
//! api_key = "sk-..."
//! base_url = "https://opencode.ai/zen/v1"
//!
//! [models."opencode/mimo-v2.5-free"]
//! provider = "opencode"
//! model = "mimo-v2.5-free"
//! display_name = "MiMo V2.5 Free"
//! max_context_size = 200000
//! ```
//!
//! Unknown top-level and nested keys are tolerated (the file belongs to the
//! user and intentionally carries settings that Allo does not consume).
//! Secrets are never logged: `api_key` is only read for the encrypted
//! provider registration and is dropped before any tracing payload.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use nomifun_api_types::NomiToolPolicy;
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentStoreConfig {
    /// `"<provider>/<model>"` selection used when the caller sends no model.
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub providers: HashMap<String, AgentStoreProvider>,
    /// Keyed by `"<provider>/<model>"`.
    #[serde(default)]
    pub models: HashMap<String, AgentStoreModel>,
    /// Default marketplace sources (store software sources) declared by this
    /// host; keyed by a stable marketplace id (`experts` etc.). The App Server
    /// registers each one **and downloads it** during boot, so this table is
    /// the opt-in to downloading: leave it out and the builtin official sources
    /// are still registered, but fetched only when the user asks for them.
    ///
    /// ```toml
    /// [default_marketplaces.experts]
    /// source_kind = "url"      # url | github | git | directory | zip
    /// source = "http://127.0.0.1:8300/marketplace.json"
    /// ```
    #[serde(default)]
    pub default_marketplaces: HashMap<String, AgentStoreMarketplace>,
    /// `[memory]` — session-end memory behaviour for this host.
    ///
    /// ```toml
    /// [memory]
    /// distill_enabled = false   # skip the post-answer distillation model call
    /// ```
    #[serde(default)]
    pub memory: Option<AgentStoreMemory>,
    /// `[marketplace]` — background auto-update cadence for this host.
    ///
    /// ```toml
    /// [marketplace]
    /// auto_update_interval_hours = 6   # omit to keep the sweep off entirely
    /// ```
    #[serde(default)]
    pub marketplace: Option<AgentStoreMarketplaceSettings>,
    /// `[import]` — load/import-time strictness knobs.
    ///
    /// ```toml
    /// [import]
    /// strict_dependencies = true   # refuse extensions with unsatisfied deps
    /// ```
    #[serde(default)]
    pub import: Option<AgentStoreImport>,
    /// `[tools]` — the host's global tool policy for every Nomi session it
    /// builds (`20-tool-injection-policy.zh.md`). **Only the `agent-store` host
    /// consumes this table**; the desktop and web hosts read the same file for
    /// providers/marketplaces but never adopt the tool policy (see
    /// `AppConfig::adopt_store_tool_policy`).
    ///
    /// ```toml
    /// [tools]
    /// enabled  = []                             # non-empty = only these tools
    /// disabled = ["remember", "mcp__notion__*"] # subtracted after `enabled`
    /// computer = false
    ///
    /// [tools.domains]
    /// cron = false
    /// ```
    ///
    /// Absent table = the permissive default (every family on, both lists
    /// empty), i.e. exactly the behaviour before this table existed. The value
    /// is the same typed policy the agent factory consumes, so the file shape
    /// and the runtime shape cannot drift apart.
    #[serde(default)]
    pub tools: Option<NomiToolPolicy>,
    /// `[connector_proxy]` — which MCP tools this host will **call on a third
    /// party's behalf** (doc `24` §5, shape revised by doc `26`).
    ///
    /// ```toml
    /// [connector_proxy]
    /// enabled = true
    /// allow = ["mcp__github__*"]           # optional narrowing; absent = everything
    /// deny = ["mcp__*__delete_*"]          # optional subtraction, applied after `allow`
    /// ```
    ///
    /// **Fail-closed about the table, permissive about its contents.** An absent
    /// table or an absent `enabled` still means *the proxy is off*. But once the
    /// host's operator has turned it on, the default is that the enabled
    /// connectors are callable: `allow` is an **optional** narrowing (present but
    /// empty = nothing callable) and `deny` subtracts afterwards.
    ///
    /// This replaced a mandatory per-tool allowlist. The reason is in doc `26`
    /// §3.1: a grant to a tool name nobody could inspect was a blind signature,
    /// and the fix for that (exposing the schema, §5 there) is what makes the
    /// larger unit defensible.
    ///
    /// Entries use the engine's tool vocabulary — `mcp__<connector>__<tool>`,
    /// where `<connector>` may be the registered name or the id, and only
    /// `mcp__…` entries are globs (`mcp__github__*` for a whole connector). The
    /// same rule `[tools].enabled`/`disabled` use.
    ///
    /// Like `[tools]`, **only the `agent-store` host adopts this**; the desktop
    /// and web hosts read the same file for providers/marketplaces and never
    /// expose a call proxy.
    #[serde(default)]
    pub connector_proxy: Option<AgentStoreConnectorProxy>,
    /// `[expert_export]` — the expert-definition export face (`agent/export`,
    /// `team/export`; doc `32` §6.2).
    ///
    /// ```toml
    /// [expert_export]
    /// enabled = true    # absent table = on
    /// deny = []         # optional subtraction; agent or team ids
    /// ```
    ///
    /// **A *subtraction* table, and therefore fail-open** — the opposite of
    /// `[connector_proxy]`, which *grants* execution. This face only reads
    /// curated bytes, which is the same class of thing `skill/files` already
    /// serves with no gate at all; a broken file here is at worst "the
    /// subtraction did not happen". Doc `32` §6.2 records why the earlier
    /// fail-closed shape was wrong.
    #[serde(default)]
    pub expert_export: Option<AgentStoreExpertExport>,
    /// `[credentials]` — values for `secret:NAME` references (`17` §6 / `21`
    /// D5=C).
    ///
    /// ```toml
    /// [credentials]
    /// DEMO_TOKEN = "…"   # fills `secret:DEMO_TOKEN` in an imported MCP env
    /// ```
    ///
    /// **Hand-edited, never on the wire.** This table is deliberately absent
    /// from both the read view (`config_view`, `config/get`) and the write
    /// whitelist (`AgentStoreConfigPatch`, `config/set`), so a credential cannot
    /// be read back or written through the protocol. It exists only so the host
    /// can install the values in-process at startup ([`nomifun_common::
    /// secret_ref::set_credentials`]); MCP spawn paths resolve references against
    /// that in-memory map, and the values are never persisted.
    #[serde(default)]
    pub credentials: HashMap<String, String>,
}

/// `[connector_proxy]` in `~/.agent-store/config.toml` (doc `24` §5.1, doc `26`).
///
/// Hand-edited, and **not** on the `config/set` whitelist: this table decides
/// what a third party is allowed to execute through this host, which is not a
/// setting a remote caller should be able to widen. Kept out of `config/get`'s
/// projection for the same reason `[credentials]` is.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentStoreConnectorProxy {
    /// Absent or `false` = the call proxy is off entirely.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// **Optional narrowing.** Entries are `mcp__<connector>__<tool>` patterns
    /// (`mcp__github__*` for a whole connector); `<connector>` may be the
    /// registered name or the id.
    ///
    /// **Absent = every tool of every enabled connector is callable.** Present
    /// but empty (`allow = []`) = nothing is callable, which is why the field
    /// stays an `Option`: "I did not narrow" and "I narrowed to nothing" are
    /// different statements and must not collapse into one.
    ///
    /// The old spelling was `<connector>__<tool>` without the `mcp__` prefix.
    /// Those entries no longer match anything (fail-closed, i.e. they narrow to
    /// nothing rather than widening); [`ConnectorProxyPolicy::warnings`] names
    /// them so the operator can rewrite them.
    #[serde(default)]
    pub allow: Option<Vec<String>>,
    /// **Optional subtraction**, applied *after* `allow` — the same order
    /// `[tools].disabled` uses. Absent or empty subtracts nothing.
    #[serde(default)]
    pub deny: Option<Vec<String>>,
}

/// Whether a policy entry selects `tool_name`.
///
/// The rule is the engine's (`nomi-tools/src/registry.rs`), and it is
/// deliberately not re-invented here: builtin-style names are compared exactly
/// and case-sensitively, and only entries in the `mcp__` namespace are globs.
/// Both sides use the same `glob` crate, so the semantics cannot drift; the
/// engine's own case table is copied into this file's tests so that even the
/// wrapper cannot drift silently.
///
/// Every candidate name this module builds starts with `mcp__`, so a non-`mcp__`
/// entry can never match — which is exactly the stale-allowlist case
/// [`ConnectorProxyPolicy::warnings`] reports.
fn tool_name_matches(pattern: &str, tool_name: &str) -> bool {
    if pattern.starts_with(MCP_TOOL_PREFIX) {
        glob::Pattern::new(pattern)
            .map(|parsed| parsed.matches(tool_name))
            .unwrap_or(false)
    } else {
        pattern == tool_name
    }
}

fn matches_any(patterns: &std::collections::HashSet<String>, tool_name: &str) -> bool {
    patterns.iter().any(|pattern| tool_name_matches(pattern, tool_name))
}

/// The reserved prefix for MCP proxy tool names.
const MCP_TOOL_PREFIX: &str = "mcp__";

/// The `[connector_proxy]` table, resolved into the form the call gate uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnectorProxyPolicy {
    enabled: bool,
    /// `None` = no narrowing was declared (everything is callable).
    allow: Option<std::collections::HashSet<String>>,
    deny: std::collections::HashSet<String>,
}

impl ConnectorProxyPolicy {
    /// The fail-closed default: no tool may be called.
    pub fn deny_all() -> Self {
        Self::default()
    }

    /// Build from the declared table.
    pub fn from_declared(declared: &AgentStoreConnectorProxy) -> Self {
        Self {
            enabled: declared.enabled.unwrap_or(false),
            allow: declared.allow.as_ref().map(|entries| clean_entries(entries)),
            deny: declared
                .deny
                .as_ref()
                .map(|entries| clean_entries(entries))
                .unwrap_or_default(),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Configuration states worth telling the operator about, as
    /// caller-rendered messages (same shape as
    /// `NomiToolPolicy::syntax_warnings`, and for the same reason: a pure
    /// function is testable, a log inside a constructor is not).
    pub fn warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        if !self.enabled {
            return warnings;
        }
        match self.allow.as_ref() {
            None => warnings.push(
                "[connector_proxy] is enabled with neither `allow` nor `deny`: every tool of \
                 every enabled connector is callable by third parties."
                    .to_owned(),
            ),
            Some(allow) if allow.is_empty() => warnings.push(
                "[connector_proxy].allow is present but empty, which allows nothing (an absent \
                 `allow` would allow everything)."
                    .to_owned(),
            ),
            Some(_) => {}
        }
        for (field, entries) in [("allow", self.allow.as_ref()), ("deny", Some(&self.deny))] {
            for entry in entries.into_iter().flatten() {
                if !entry.starts_with(MCP_TOOL_PREFIX) {
                    warnings.push(format!(
                        "[connector_proxy].{field} entry `{entry}` is not an `mcp__` pattern and \
                         can never match; write `mcp__{entry}` (a whole connector is \
                         `mcp__{entry}__*`)."
                    ));
                }
            }
        }
        warnings
    }

    /// May `tool` be called on this connector?
    ///
    /// `allow` narrows, `deny` subtracts afterwards. Neither is required: an
    /// enabled proxy with no `allow` lets the enabled connectors through, which
    /// is the point of doc `26`. The connector may be named by **id** (precise)
    /// or by **name** (convenient) — id is offered because Agent Store upserts
    /// MCP servers *by name*, so a later install can legitimately take over a
    /// name and would otherwise inherit its permission.
    pub fn decide(&self, connector_id: &str, connector_name: &str, tool: &str) -> Result<(), String> {
        if !self.enabled {
            return Err(
                "the connector call proxy is disabled on this host; set [connector_proxy] enabled = true"
                    .to_owned(),
            );
        }
        let candidates = [
            format!("{MCP_TOOL_PREFIX}{connector_id}__{tool}"),
            format!("{MCP_TOOL_PREFIX}{connector_name}__{tool}"),
        ];
        if let Some(allow) = self.allow.as_ref()
            && !candidates.iter().any(|name| matches_any(allow, name))
        {
            return Err(format!(
                "tool `{tool}` on connector `{connector_name}` is not in [connector_proxy].allow"
            ));
        }
        if candidates.iter().any(|name| matches_any(&self.deny, name)) {
            return Err(format!(
                "tool `{tool}` on connector `{connector_name}` is excluded by [connector_proxy].deny"
            ));
        }
        Ok(())
    }
}

/// Trim entries and drop the blank ones, so `allow = ["", "  "]` is an empty
/// narrowing rather than a pattern that could match something.
fn clean_entries(entries: &[String]) -> std::collections::HashSet<String> {
    entries
        .iter()
        .map(|entry| entry.trim().to_owned())
        .filter(|entry| !entry.is_empty())
        .collect()
}

/// `[expert_export]` in `~/.agent-store/config.toml` (doc `32` §6.2).
///
/// Hand-edited, and **not** on the `config/set` whitelist — same reason as
/// `[connector_proxy]`: it is a host-policy table, not a per-request setting, and
/// a remote caller has no business widening who may take definitions off this
/// machine.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentStoreExpertExport {
    /// Absent = **on**. `false` switches the whole export face off.
    ///
    /// The default is deliberately the permissive one: this table only
    /// subtracts, and `enabled = false` is an operator's opt-out, not the
    /// baseline (doc `32` §6.2).
    #[serde(default)]
    pub enabled: Option<bool>,
    /// **Optional subtraction**: these ids are never exported. Entries are agent
    /// or team component ids (the same opaque ids `agent/list` / `team/list`
    /// publish).
    ///
    /// Absent and empty mean the same thing here — nothing is subtracted — which
    /// is why this is an `Option` purely for shape symmetry with
    /// `[connector_proxy]`, not because the two states differ.
    #[serde(default)]
    pub deny: Option<Vec<String>>,
}

/// The `[expert_export]` table, resolved into the form the export gate uses.
///
/// Fail-**open** on purpose (see [`AgentStoreConfig::expert_export_policy`]):
/// the wire still distinguishes `policy_denied` from `not_found`, so a caller can
/// tell "you turned this off" from "there is no such expert" — the same
/// distinction `preset_disabled` / `agent_not_installed` exist for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpertExportPolicy {
    enabled: bool,
    deny: std::collections::HashSet<String>,
}

impl Default for ExpertExportPolicy {
    /// The default-open policy: the face is available and nothing is subtracted.
    fn default() -> Self {
        Self {
            enabled: true,
            deny: std::collections::HashSet::new(),
        }
    }
}

impl ExpertExportPolicy {
    /// The default-open policy, named so call sites read as an intent rather than
    /// as `::default()`.
    pub fn allow_all() -> Self {
        Self::default()
    }

    /// Build from the declared table. A table with no `enabled` key is **on**.
    pub fn from_declared(declared: &AgentStoreExpertExport) -> Self {
        Self {
            enabled: declared.enabled.unwrap_or(true),
            deny: declared
                .deny
                .as_ref()
                .map(|entries| clean_entries(entries))
                .unwrap_or_default(),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// `Ok(())` when `id` may be exported, otherwise the operator-facing reason.
    ///
    /// Messages are caller-rendered, the same shape
    /// [`ConnectorProxyPolicy::decide`] uses. The id is matched exactly: unlike
    /// `[tools]`, there is no glob namespace here, so a typo narrows nothing and
    /// silently leaves the entry exportable — which is the honest reading of a
    /// subtraction table.
    pub fn decide(&self, id: &str) -> Result<(), String> {
        if !self.enabled {
            return Err(
                "expert export is switched off on this host; remove `[expert_export] enabled = false`"
                    .to_owned(),
            );
        }
        if self.deny.contains(id) {
            return Err(format!("`{id}` is listed in [expert_export].deny"));
        }
        Ok(())
    }
}

impl AgentStoreConfig {
    /// The `[connector_proxy]` policy, defaults filled in.
    ///
    /// Absent **table** → [`ConnectorProxyPolicy::deny_all`]: there is
    /// deliberately no "unset means permissive" arm at this level, because the
    /// permissive reading here is what would turn every installed connector
    /// into a callable surface without anyone asking for it.
    ///
    /// Inside a table that *is* enabled, an absent `allow` is permissive by
    /// design (doc `26` §4): the operator opted in, and `allow`/`deny` are the
    /// optional narrowing and subtraction.
    pub fn connector_proxy_policy(&self) -> ConnectorProxyPolicy {
        match self.connector_proxy.as_ref() {
            Some(declared) => ConnectorProxyPolicy::from_declared(declared),
            None => ConnectorProxyPolicy::deny_all(),
        }
    }

    /// The `[expert_export]` policy, defaults filled in.
    ///
    /// Absent **table** → [`ExpertExportPolicy::allow_all`], mirroring `[tools]`
    /// (fail-open): this table only ever subtracts, so "unset" means "nothing was
    /// subtracted", not "everything was granted". Compare
    /// [`AgentStoreConfig::connector_proxy_policy`], where the same absence means
    /// deny-all — because *that* table grants execution.
    pub fn expert_export_policy(&self) -> ExpertExportPolicy {
        match self.expert_export.as_ref() {
            Some(declared) => ExpertExportPolicy::from_declared(declared),
            None => ExpertExportPolicy::allow_all(),
        }
    }
}

/// `[import]` in `~/.agent-store/config.toml` (`16` R23 / `17` §7).
///
/// A hand-edited escape hatch — deliberately **not** on the `config/set`
/// whitelist and not rendered in the settings dialog, because the registry
/// reads it once at construction: a settings "switch" would imply a per-change
/// effect that does not exist.
///
/// Default is **off**, which is byte-for-byte the pre-existing behaviour: an
/// unsatisfiable dependency only warns.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentStoreImport {
    /// `true` refuses extensions whose declared dependencies are missing or out
    /// of range. Absent or `false` keeps the warn-only behaviour.
    #[serde(default)]
    pub strict_dependencies: Option<bool>,
}

/// `[memory]` in `~/.agent-store/config.toml`.
///
/// Two independent switches live here, both of them this host's opt-out from an
/// upstream default that is ON:
///
/// - `distill_enabled` — the post-answer distillation half, which is one extra
///   model call awaited **before** the turn's terminal `Finish`, so a client
///   sees its answer complete while the turn still reports "processing" for the
///   whole call (measured 6–15s in the agent-store host).
/// - `enabled` — the whole built-in file-based memory system, including the
///   parts that are *not* distillation: the system-prompt memory section and
///   the `remember` tool.
///
/// Declaring this table is how the Agent Store host opts out without touching
/// upstream defaults for other hosts.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentStoreMemory {
    /// `false` disables post-answer distillation on this host; `true` or an
    /// absent key keeps the upstream default. `NOMIFUN_MEMORY_DISTILL` still
    /// overrides whichever value lands here.
    #[serde(default)]
    pub distill_enabled: Option<bool>,
    /// `false` turns the entire built-in file-based memory system off on this
    /// host: no memory section in the system prompt, no `remember` tool, no
    /// post-answer distillation, and no citation usage write-back. `true` or an
    /// absent key keeps the upstream default (ON).
    ///
    /// Distinct from — and strictly wider than — `distill_enabled`: turning
    /// `enabled` off makes `distill_enabled` irrelevant, so the two are not
    /// required to agree. Memory files already on disk are left alone, so
    /// setting this back to `true` restores exactly what was there.
    #[serde(default)]
    pub enabled: Option<bool>,
}

impl AgentStoreMemory {
    /// Whether the built-in memory system is enabled. An absent table, an empty
    /// table and an absent key all mean enabled: opting out must be explicit, so
    /// a host that only ever wrote `distill_enabled` keeps the full system.
    pub fn enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }
}

impl AgentStoreConfig {
    /// `[memory] enabled`, defaulted ON for the same reason
    /// [`Self::tool_policy`] defaults permissive: a host that declares no
    /// `[memory]` table must behave exactly as it did before the key existed.
    pub fn memory_enabled(&self) -> bool {
        self.memory
            .as_ref()
            .map(AgentStoreMemory::enabled)
            .unwrap_or(true)
    }
}

/// `[marketplace]` in `~/.agent-store/config.toml`.
///
/// The background auto-update sweep is **off unless a cadence is declared
/// here**, and even then it only ever covers marketplaces whose source is one
/// of the builtin official mirrors (`18` §7: V1 never auto-updates third-party
/// sources). Both conditions are deliberate: the sweep re-downloads market
/// trees, which is exactly the bandwidth that D-SDK-1 ④ is trying to shrink.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentStoreMarketplaceSettings {
    /// Hours between sweeps. Absent or `0` keeps the scheduler off.
    #[serde(default)]
    pub auto_update_interval_hours: Option<u64>,
}

impl AgentStoreMarketplaceSettings {
    /// `Some(hours)` only when a positive cadence was declared.
    pub fn cadence(&self) -> Option<std::time::Duration> {
        let hours = self.auto_update_interval_hours?;
        (hours > 0).then(|| std::time::Duration::from_secs(hours.saturating_mul(3_600)))
    }
}

/// One default marketplace source declared in `~/.agent-store/config.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentStoreMarketplace {
    /// `url` | `github` | `git` | `directory` | `zip`.
    #[serde(default)]
    pub source_kind: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
}

impl AgentStoreMarketplace {
    /// Resolve to `(source_kind, source)` when both are present and non-empty.
    ///
    /// Deliberately does **not** validate `source_kind`. This used to carry its
    /// own `matches!(kind, "url" | "github" | "git" | "directory")` list — a
    /// second copy of the kind set, and a silent one: when `zip` landed (doc
    /// 30) this method rejected it, so the caller's `filter_map` dropped every
    /// configured zip source before its unknown-kind warning could run, and a
    /// config declaring *only* zip sources produced an empty store reported as
    /// a complete run. The kind set is `AppServerMarketplaceSourceKind::parse`;
    /// an unrecognised kind is passed through here so the caller can warn.
    pub fn resolved(&self) -> Option<(String, String)> {
        let kind = self.source_kind.as_deref()?.trim().to_owned();
        let source = self.source.as_deref()?.trim().to_owned();
        if kind.is_empty() || source.is_empty() {
            return None;
        }
        Some((kind, source))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentStoreProvider {
    /// e.g. `"openai"`/`"anthropic"` — maps to the provider `platform`.
    #[serde(rename = "type")]
    pub r#type: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentStoreModel {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub max_context_size: Option<i64>,
    /// Per-request output ceiling, mapped onto `provider_models.output_limit`.
    ///
    /// This is the key whose absence fails an `anthropic` provider at runtime
    /// (`Config::requires_output_ceiling`): the request really does need
    /// `max_tokens` on the wire, so a NULL row here is not "let the provider
    /// decide", it is a build error. Values `<= 0` are dropped by
    /// [`AgentStoreConfig::output_limits_for_provider`] because the column
    /// carries `CHECK (output_limit IS NULL OR output_limit > 0)`.
    #[serde(default)]
    pub max_output_size: Option<i64>,
    /// Per-model protocol override (`"anthropic"`, `"openai.responses"`, ...).
    ///
    /// Only the spellings `map_nomi_provider` already honours take effect
    /// (`new-api` + `anthropic`, and the responses forms); the point of
    /// carrying it here is that it is no longer *silently* discarded.
    #[serde(default)]
    pub protocol: Option<String>,
    /// Not consumed yet: declared-and-ignored like `max_output_size` used to
    /// be. Kept parseable so the file stays compatible with the kimi-code
    /// layout this section follows.
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub reasoning_key: Option<String>,
}

impl AgentStoreConfig {
    /// Parse the file at `path`. Returns a human-readable error for logging.
    pub fn load(path: &Path) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        Self::from_source(&raw).map_err(|error| format!("{}: {error}", path.display()))
    }

    /// Parse a config *source* (already-read file body). `load` is the
    /// file-backed entry point; this one exists for the write path, which must
    /// parse the exact text it is about to edit (`config/set`).
    pub fn from_source(source: &str) -> Result<Self, String> {
        toml::from_str(source).map_err(|error| error.to_string())
    }

    /// The host's tool policy, defaults filled in.
    ///
    /// A host that declares no `[tools]` table (or no config file at all) gets
    /// [`NomiToolPolicy::default`], which constrains nothing — so adopting the
    /// policy is safe even on a host that only ever wrote `default_model`.
    pub fn tool_policy(&self) -> NomiToolPolicy {
        self.tools.clone().unwrap_or_default()
    }

    /// `[import].strict_dependencies` (`16` R23 / `17` §7).
    ///
    /// The registry passes this into `ExtensionRegistry::with_strict_dependencies`.
    /// Default **off**: absent table, empty table and an explicit `false` all
    /// return `false`, so the historical warn-only behaviour is the default.
    pub fn strict_dependencies(&self) -> bool {
        self.import
            .as_ref()
            .and_then(|import| import.strict_dependencies)
            .unwrap_or(false)
    }

    /// Minimal-change rewrite of the whitelisted `default_model` key.
    ///
    /// `source` in, `source` out: every other key, every comment and the file's
    /// original layout survive untouched (`toml_edit` is lossless by design), so
    /// a settings save can never reformat or drop a hand-edited config. A key
    /// that is not present yet is inserted as a **top-level** key — never
    /// appended after a `[table]`, where it would silently become a member of
    /// that table.
    pub fn with_default_model(source: &str, value: &str) -> Result<String, String> {
        let mut document = source
            .parse::<toml_edit::Document>()
            .map_err(|error| format!("config.toml is not valid TOML: {error}"))?;

        if let Some(existing) = document.get("default_model") {
            // Only the value is re-rendered: the key's own indentation and its
            // trailing comment survive with it.
            let decor = existing.as_value().map(|value| value.decor().clone());
            let mut item = toml_edit::value(value);
            if let (Some(decor), Some(rendered)) = (decor, item.as_value_mut()) {
                *rendered.decor_mut() = decor;
            }
            document["default_model"] = item;
            return Ok(document.to_string());
        }

        // Missing key: insert it above the first real entry — after a leading
        // file header comment, so that comment stays at the top. Appending
        // would land the key inside whichever `[table]` came last, and the
        // first lines of a file are top-level by construction.
        let mut inserted = toml_edit::Document::new();
        inserted["default_model"] = toml_edit::value(value);
        let offset = source
            .split_inclusive('\n')
            .take_while(|line| {
                let trimmed = line.trim();
                trimmed.is_empty() || trimmed.starts_with('#')
            })
            .map(str::len)
            .sum::<usize>();
        Ok(format!("{}{}{}", &source[..offset], inserted, &source[offset..]))
    }

    /// Minimal-change rewrite of the whitelisted `[memory] distill_enabled` key.
    ///
    /// Same contract as [`Self::with_default_model`]: only the named key is
    /// re-rendered, every sibling key inside `[memory]` (and the rest of the
    /// file) survives byte-for-byte. The `[memory]` table itself is created
    /// when absent **and** when present as the *implicit* form
    /// (`memory.distill_enabled = …` written as a dotted key) — `toml_edit`
    /// treats those as the same table, so the dotted form is upgraded in place
    /// rather than duplicated.
    ///
    /// The value is the host's real switch: `apps/agent-store` reads
    /// `[memory].distill_enabled` at startup and forwards it to
    /// `manager::nomi::distill::set_distill_host_override`, so a write here
    /// changes the next launch's behaviour (and the read view reports it back).
    pub fn with_distill_enabled(source: &str, enabled: bool) -> Result<String, String> {
        let mut document = source
            .parse::<toml_edit::Document>()
            .map_err(|error| format!("config.toml is not valid TOML: {error}"))?;

        if let Some(existing) = document.get("memory").and_then(|item| item.get("distill_enabled"))
        {
            let decor = existing.as_value().map(|value| value.decor().clone());
            let mut item = toml_edit::value(enabled);
            if let (Some(decor), Some(rendered)) = (decor, item.as_value_mut()) {
                *rendered.decor_mut() = decor;
            }
            document["memory"]["distill_enabled"] = item;
            return Ok(document.to_string());
        }

        // No `[memory]` table at all: append one. Unlike a top-level key, a
        // table may safely be appended at the end of the file — and appending
        // is the only lossless option, because inserting a table *above* an
        // existing key would move that key into the new table.
        if document.get("memory").is_none() {
            let mut table = toml_edit::Table::new();
            table.insert("distill_enabled", toml_edit::value(enabled));
            document["memory"] = toml_edit::Item::Table(table);
            return Ok(document.to_string());
        }

        // `[memory]` exists without the key: insert it as the table's last key,
        // so the table's own header comment and its other keys stay put.
        document["memory"]["distill_enabled"] = toml_edit::value(enabled);
        Ok(document.to_string())
    }

    /// Minimal-change rewrite of the whitelisted `[tools] enabled` /
    /// `[tools] disabled` lists.
    ///
    /// Same contract as [`Self::with_distill_enabled`]: only the named keys are
    /// re-rendered, so every sibling key, comment and the rest of the file
    /// survive byte-for-byte. `[tools]` is created (appended as a table, which
    /// is the only lossless option) when the file has no such table.
    ///
    /// Both lists are canonicalised here (trimmed, deduplicated, sorted), so the
    /// file and the read view always agree on ordering and no caller can write a
    /// non-canonical list.
    pub fn with_tool_lists(
        source: &str,
        enabled: Option<&[String]>,
        disabled: Option<&[String]>,
    ) -> Result<String, String> {
        let mut document = source
            .parse::<toml_edit::Document>()
            .map_err(|error| format!("config.toml is not valid TOML: {error}"))?;
        if enabled.is_none() && disabled.is_none() {
            return Ok(document.to_string());
        }
        let enabled = enabled.map(canonical_patterns).transpose()?;
        let disabled = disabled.map(canonical_patterns).transpose()?;
        {
            let table = ensure_table(&mut document, "tools", "[tools]")?;
            if let Some(values) = enabled.as_deref() {
                set_item_preserving_decor(table, "enabled", toml_edit::value(string_array(values)));
            }
            if let Some(values) = disabled.as_deref() {
                set_item_preserving_decor(table, "disabled", toml_edit::value(string_array(values)));
            }
        }
        Ok(document.to_string())
    }

    /// Minimal-change rewrite of one `[tools]` boolean switch.
    ///
    /// `key` must be a declared switch; anything else is refused rather than
    /// written, so the write face cannot grow beyond its whitelist.
    pub fn with_tool_switch(source: &str, key: &str, value: bool) -> Result<String, String> {
        if !TOOL_SWITCH_KEYS.contains(&key) {
            return Err(format!(
                "`{key}` is not a [tools] switch (expected one of {})",
                TOOL_SWITCH_KEYS.join(", ")
            ));
        }
        let mut document = source
            .parse::<toml_edit::Document>()
            .map_err(|error| format!("config.toml is not valid TOML: {error}"))?;
        {
            let table = ensure_table(&mut document, "tools", "[tools]")?;
            set_item_preserving_decor(table, key, toml_edit::value(value));
        }
        Ok(document.to_string())
    }

    /// Minimal-change rewrite of one `[credentials]` entry (`34` §6.1).
    ///
    /// `key` is the **stored** key, which is what makes the caller decide whose
    /// credential this is: a bare `NAME` is the host-level entry (the installation
    /// owner's), `<principal>:NAME` is one principal's own (`34` §7). The file is
    /// rewritten through `toml_edit`, so the operator's comments and formatting
    /// around every other key survive — this file is hand-edited by design.
    ///
    /// Nothing here reads or logs the value; the caller persists the result.
    pub fn with_credential(source: &str, key: &str, value: &str) -> Result<String, String> {
        let key = key.trim();
        if key.is_empty() {
            return Err("a credential key must not be empty".to_owned());
        }
        let mut document = source
            .parse::<toml_edit::Document>()
            .map_err(|error| format!("config.toml is not valid TOML: {error}"))?;
        {
            let table = ensure_table(&mut document, "credentials", "[credentials]")?;
            set_item_preserving_decor(table, key, toml_edit::value(value));
        }
        Ok(document.to_string())
    }

    /// Remove one `[credentials]` entry, leaving the rest of the file byte-identical.
    ///
    /// Removing a key that is not there is success, not an error: `credential/clear`
    /// is idempotent, and a client retrying it must not see a failure.
    pub fn without_credential(source: &str, key: &str) -> Result<String, String> {
        let key = key.trim();
        if key.is_empty() {
            return Err("a credential key must not be empty".to_owned());
        }
        let mut document = source
            .parse::<toml_edit::Document>()
            .map_err(|error| format!("config.toml is not valid TOML: {error}"))?;
        if let Some(table) = document
            .get_mut("credentials")
            .and_then(toml_edit::Item::as_table_mut)
        {
            table.remove(key);
        }
        Ok(document.to_string())
    }

    /// Minimal-change rewrite of one `[tools.domains]` boolean switch.
    pub fn with_tool_domain(source: &str, key: &str, value: bool) -> Result<String, String> {
        if !TOOL_DOMAIN_KEYS.contains(&key) {
            return Err(format!(
                "`{key}` is not a [tools.domains] switch (expected one of {})",
                TOOL_DOMAIN_KEYS.join(", ")
            ));
        }
        let mut document = source
            .parse::<toml_edit::Document>()
            .map_err(|error| format!("config.toml is not valid TOML: {error}"))?;
        {
            let table = ensure_table(&mut document, "tools", "[tools]")?;
            if table.get("domains").is_none() {
                table.insert("domains", toml_edit::Item::Table(toml_edit::Table::new()));
            }
            let domains = table
                .get_mut("domains")
                .and_then(toml_edit::Item::as_table_mut)
                .ok_or_else(|| "[tools.domains] is not a table".to_owned())?;
            set_item_preserving_decor(domains, key, toml_edit::value(value));
        }
        Ok(document.to_string())
    }

    /// `load` flattened to `Option` for read-only catalog projections: a
    /// missing/unparseable config simply contributes nothing.
    pub fn load_ok(path: &Path) -> Option<Self> {
        Self::load(path).ok()
    }

    /// Builtin marketplace sources used when `~/.agent-store/config.toml` is
    /// missing (or has no `[default_marketplaces]`): the product's own public
    /// markets, so a fresh install can *see* the official sources in the store
    /// registry without touching any config.
    ///
    /// Registering them is **not** downloading them: the fallback path
    /// registers these unfetched (`register_unfetched`) and waits for an
    /// explicit `market/refresh`. Declaring a source in the config file is the
    /// opt-in to registering *and fetching* it at startup.
    ///
    /// Doc 30: each market is **one `zip` archive** on ModelScope, and the
    /// archive root *is* the market root. That replaced the per-file tree the
    /// site repo used to serve under `/source/<market>/…` alongside a
    /// pre-generated `_files.txt`; those URLs and their inventories are retired,
    /// so nothing here is a "full-tree mirror" any more. The source is a single
    /// request whose revision comes from `HEAD`'s `X-Linked-Etag` (content
    /// sha256) — see [`AppServerMarketplaceSourceKind::parse`] and
    /// `looks_like_market` for the `zip` contract.
    ///
    /// The fallback short-circuits whenever the user declares their own
    /// `[default_marketplaces]`.
    ///
    /// This list is the release default **and** the definition of "official":
    /// `is_official_source` compares the `(source_kind, source)` pair against
    /// it, so it classifies a source by **address**, not by who declared it —
    /// and it is what makes a manually added official archive eligible for the
    /// auto-update sweep. Moving a URL here therefore re-classifies the old
    /// address as third-party. (The lazy fallback registration deliberately
    /// pins `auto_update = false` anyway: see `register_unfetched`.)
    pub fn builtin_default_marketplaces() -> Vec<(String, String, String)> {
        // Doc 30: the official bundles ship as **one archive per market** on
        // ModelScope, not as a file-per-entry tree on the site. The `url` form
        // cost a first fetch 14,714 requests / 611 MiB for `experts` alone
        // (the site mirrors every listed file); the archive is 1 request /
        // 289 MiB and is the same bytes the mirror would have pulled.
        //
        // `master` is deliberate, not sloppiness: a market is a *moving*
        // target, so pinning a commit sha here would freeze every client at
        // whatever was current when this binary was built, and shipping a new
        // binary would be the only way to publish a market change. Clients
        // detect the change with a `HEAD` (`X-Linked-Etag` = content sha256),
        // so an unchanged archive is never re-downloaded.
        const MARKET_HOST: &str =
            "https://www.modelscope.cn/models/me9rez/flowy-marketplace/resolve/master";
        ["experts", "skills", "connectors"]
            .into_iter()
            .map(|id| (id.to_owned(), "zip".to_owned(), format!("{MARKET_HOST}/{id}.zip")))
            .collect()
    }

    /// Default location: `~/.agent-store/config.toml` (Windows then POSIX).
    pub fn default_path() -> Option<PathBuf> {
        let home = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))?;
        Some(home.join(".agent-store").join("config.toml"))
    }

    /// `default_model` split into `(provider_key, model)`.
    pub fn default_selection(&self) -> Option<(String, String)> {
        let value = self.default_model.as_deref()?.trim();
        let (provider, model) = value.split_once('/')?;
        let provider = provider.trim();
        let model = model.trim();
        if provider.is_empty() || model.is_empty() {
            return None;
        }
        Some((provider.to_owned(), model.to_owned()))
    }

    /// Sorted model names whose `[models."<provider>/<model>"]` entry points at
    /// the given provider key.
    pub fn models_for_provider(&self, provider_key: &str) -> Vec<String> {
        let mut names = self
            .models
            .values()
            .filter(|entry| entry.provider.as_deref() == Some(provider_key))
            .filter_map(|entry| {
                entry
                    .model
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
            })
            .collect::<Vec<_>>();
        names.sort();
        names.dedup();
        names
    }

    /// Context-window map for `provider_models` (`model -> max_context_size`).
    pub fn context_limits_for_provider(&self, provider_key: &str) -> HashMap<String, i64> {
        self.models
            .iter()
            .filter(|(_, entry)| entry.provider.as_deref() == Some(provider_key))
            .filter_map(|(key, entry)| {
                let model = entry
                    .model
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())?;
                let limit = entry.max_context_size?;
                let _ = key;
                Some((model.to_owned(), limit))
            })
            .collect()
    }

    /// Display-name map for `provider_models` (`model -> display_name`).
    pub fn display_names_for_provider(&self, provider_key: &str) -> HashMap<String, String> {
        self.models
            .iter()
            .filter(|(_, entry)| entry.provider.as_deref() == Some(provider_key))
            .filter_map(|(_, entry)| {
                let model = entry
                    .model
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())?;
                let display_name = entry
                    .display_name
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())?;
                Some((model.to_owned(), display_name.to_owned()))
            })
            .collect()
    }

    /// Output-ceiling map for `provider_models` (`model -> max_output_size`).
    ///
    /// Non-positive values are deliberately dropped: the column is guarded by
    /// `CHECK (output_limit IS NULL OR output_limit > 0)` (migration `045`), so
    /// forwarding a `0` would fail the write instead of meaning "no ceiling".
    /// An absent or unusable value therefore leaves the row NULL — the same
    /// state as before this key was wired up, never a synthesized default.
    pub fn output_limits_for_provider(&self, provider_key: &str) -> HashMap<String, i64> {
        self.models
            .iter()
            .filter(|(_, entry)| entry.provider.as_deref() == Some(provider_key))
            .filter_map(|(_, entry)| {
                let model = entry
                    .model
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())?;
                let limit = entry.max_output_size.filter(|limit| *limit > 0)?;
                Some((model.to_owned(), limit))
            })
            .collect()
    }

    /// Per-model protocol map for `provider_models` (`model -> protocol`).
    pub fn protocols_for_provider(&self, provider_key: &str) -> HashMap<String, String> {
        self.models
            .iter()
            .filter(|(_, entry)| entry.provider.as_deref() == Some(provider_key))
            .filter_map(|(_, entry)| {
                let model = entry
                    .model
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())?;
                let protocol = entry
                    .protocol
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())?;
                Some((model.to_owned(), protocol.to_owned()))
            })
            .collect()
    }
}

/// The `[tools]` boolean switches, in a stable order (the `config/get` view and
/// the write face agree on this list).
const TOOL_SWITCH_KEYS: &[&str] = &["web", "computer", "browser", "plan", "lsp"];

/// The `[tools.domains]` boolean switches, in a stable order.
const TOOL_DOMAIN_KEYS: &[&str] = &[
    "cron",
    "meeting",
    "knowledge",
    "learning",
    "media",
    "companion",
    "requirement",
    "goal",
];

/// Borrow `[key]`, creating it as an empty table when absent.
///
/// A non-table value under the key (`tools = 1`) is a refusal rather than a
/// silent overwrite of the user's file.
fn ensure_table<'a>(
    document: &'a mut toml_edit::Document,
    key: &str,
    what: &str,
) -> Result<&'a mut toml_edit::Table, String> {
    if document.get(key).is_none() {
        document[key] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    document[key]
        .as_table_mut()
        .ok_or_else(|| format!("{what} is not a table"))
}

/// Replace `key` with `item` while keeping the key's own decor (the comments
/// written above it) and the value's decor (indentation and trailing comment).
///
/// Assigning through the index is load-bearing: `Table::insert` would rebuild
/// the `Key` from a plain string and silently drop the comment above the key.
fn set_item_preserving_decor(table: &mut toml_edit::Table, key: &str, mut item: toml_edit::Item) {
    if let Some(existing) = table.get(key)
        && let (Some(decor), Some(rendered)) =
            (existing.as_value().map(|value| value.decor().clone()), item.as_value_mut())
    {
        *rendered.decor_mut() = decor;
    }
    table[key] = item;
}

fn string_array(values: &[String]) -> toml_edit::Array {
    let mut array = toml_edit::Array::new();
    for value in values {
        array.push(value.as_str());
    }
    array
}

/// Canonicalise a written pattern list: trimmed, empty entries dropped,
/// deduplicated and sorted, so the file and the read view agree.
///
/// Control characters are refused: they cannot appear in a tool name and would
/// only ever make the file unreadable.
fn canonical_patterns(values: &[String]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::with_capacity(values.len());
    for raw in values {
        let value = raw.trim();
        if value.is_empty() {
            continue;
        }
        if value.chars().any(char::is_control) {
            return Err("tool patterns must not contain control characters".to_owned());
        }
        out.push(value.to_owned());
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// The **write whitelist** for `~/.agent-store/config.toml` (`config/set`).
///
/// The whitelist *is* the security boundary, not a convenience list:
///
/// - `api_key` / `base_url` have no variant here, so a credential can never be
///   written through this face, and an attempt is a hard `invalid_request`
///   (`deny_unknown_fields`) instead of a silently dropped field;
/// - there is no path / owner / arbitrary-key variant, so the method cannot be
///   turned into an arbitrary-file writer;
/// - a field that is absent from the request leaves the file untouched
///   (宁缺毋滥): a patch only ever rewrites the keys it names.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentStoreConfigPatch {
    /// `"<provider_key>/<model>"` — the host default the App Server falls back
    /// to when a run/turn carries no explicit model.
    #[serde(default)]
    pub default_model: Option<String>,
    /// `[memory]` — the host's post-answer distillation switch.
    #[serde(default)]
    pub memory: Option<AgentStoreMemoryPatch>,
    /// `[tools]` — the host's global tool policy. Naming this table means the
    /// caller may only ever **narrow** the tool surface: there is no field here
    /// that turns a family back on against the engine's own configuration.
    #[serde(default)]
    pub tools: Option<AgentStoreToolsPatch>,
}

/// Whitelisted `[tools]` subset of a `config/set` patch.
///
/// Every field is `Option` so a patch rewrites exactly the keys it names
/// (宁缺毋滥): an absent key leaves the file untouched, which is what lets two
/// independent callers patch different switches without clobbering each other.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentStoreToolsPatch {
    #[serde(default)]
    pub enabled: Option<Vec<String>>,
    #[serde(default)]
    pub disabled: Option<Vec<String>>,
    #[serde(default)]
    pub web: Option<bool>,
    #[serde(default)]
    pub computer: Option<bool>,
    #[serde(default)]
    pub browser: Option<bool>,
    #[serde(default)]
    pub plan: Option<bool>,
    #[serde(default)]
    pub lsp: Option<bool>,
    #[serde(default)]
    pub domains: Option<AgentStoreToolDomainsPatch>,
}

/// Whitelisted `[tools.domains]` subset of a `config/set` patch.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentStoreToolDomainsPatch {
    #[serde(default)]
    pub cron: Option<bool>,
    #[serde(default)]
    pub meeting: Option<bool>,
    #[serde(default)]
    pub knowledge: Option<bool>,
    #[serde(default)]
    pub learning: Option<bool>,
    #[serde(default)]
    pub media: Option<bool>,
    #[serde(default)]
    pub companion: Option<bool>,
    #[serde(default)]
    pub requirement: Option<bool>,
    #[serde(default)]
    pub goal: Option<bool>,
}

impl AgentStoreToolsPatch {
    /// True when the request named at least one `[tools]` key.
    pub fn names_any_key(&self) -> bool {
        self.enabled.is_some()
            || self.disabled.is_some()
            || self.switches().next().is_some()
            || self.domain_switches().next().is_some()
    }

    /// The named `[tools]` boolean switches, in the canonical order.
    fn switches(&self) -> impl Iterator<Item = (&'static str, bool)> {
        TOOL_SWITCH_KEYS.iter().filter_map(|key| {
            let value = match *key {
                "web" => self.web,
                "computer" => self.computer,
                "browser" => self.browser,
                "plan" => self.plan,
                "lsp" => self.lsp,
                _ => None,
            };
            value.map(|value| (*key, value))
        })
    }

    /// The named `[tools.domains]` boolean switches, in the canonical order.
    fn domain_switches(&self) -> impl Iterator<Item = (&'static str, bool)> {
        let domains = self.domains.as_ref();
        TOOL_DOMAIN_KEYS.iter().filter_map(move |key| {
            let value = match *key {
                "cron" => domains?.cron,
                "meeting" => domains?.meeting,
                "knowledge" => domains?.knowledge,
                "learning" => domains?.learning,
                "media" => domains?.media,
                "companion" => domains?.companion,
                "requirement" => domains?.requirement,
                "goal" => domains?.goal,
                _ => None,
            };
            value.map(|value| (*key, value))
        })
    }

    /// Apply this patch to `source`, one minimal-change edit per named key.
    ///
    /// Nothing is written until the caller persists the result, so an error on a
    /// later key cannot leave a half-written file behind.
    pub fn apply_edits(&self, source: &str) -> Result<String, String> {
        let mut edited = source.to_owned();
        if self.enabled.is_some() || self.disabled.is_some() {
            edited = AgentStoreConfig::with_tool_lists(
                &edited,
                self.enabled.as_deref(),
                self.disabled.as_deref(),
            )?;
        }
        for (key, value) in self.switches() {
            edited = AgentStoreConfig::with_tool_switch(&edited, key, value)?;
        }
        for (key, value) in self.domain_switches() {
            edited = AgentStoreConfig::with_tool_domain(&edited, key, value)?;
        }
        Ok(edited)
    }
}

/// Whitelisted `[memory]` subset of a `config/set` patch: the host-side session
/// memory behaviour that `apps/agent-store` really consumes at startup.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentStoreMemoryPatch {
    /// `Some(false)` turns post-answer distillation off on this host.
    #[serde(default)]
    pub distill_enabled: Option<bool>,
}

impl AgentStoreConfigPatch {
    /// True when the request named at least one whitelisted key.
    ///
    /// A patch that names nothing is refused instead of answering 200 with an
    /// unchanged file: "sent" must never be mistakable for "stored".
    pub fn names_any_key(&self) -> bool {
        self.default_model.is_some()
            || self
                .memory
                .as_ref()
                .is_some_and(|memory| memory.distill_enabled.is_some())
            || self
                .tools
                .as_ref()
                .is_some_and(AgentStoreToolsPatch::names_any_key)
    }
}

impl AgentStoreConfigPatch {
    /// Validate the patch against the config it would be applied to and return
    /// the canonical value to write.
    ///
    /// The provider key must exist as a `[providers.<key>]` table: that is what
    /// `agent/run` resolution requires, so refusing it here keeps the host from
    /// being handed a default that fails on the next run. An *undeclared model*
    /// is deliberately accepted — the runtime registers the requested model on
    /// the provider it already knows.
    ///
    /// Error messages are wire-visible and talk about the shape of the value
    /// only; no credential is ever echoed.
    pub fn validated_default_model(&self, config: &AgentStoreConfig) -> Result<String, String> {
        let raw = self
            .default_model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                "default_model must be a non-empty \"<provider>/<model>\" selection".to_owned()
            })?;
        if raw.chars().any(char::is_control) {
            return Err("default_model must not contain control characters".to_owned());
        }
        let (provider, model) = raw
            .split_once('/')
            .ok_or_else(|| format!("default_model \"{raw}\" must be \"<provider>/<model>\""))?;
        let (provider, model) = (provider.trim(), model.trim());
        if provider.is_empty() || model.is_empty() {
            return Err(format!("default_model \"{raw}\" must be \"<provider>/<model>\""));
        }
        if !config.providers.contains_key(provider) {
            return Err(format!(
                "no [providers.{provider}] entry in ~/.agent-store/config.toml: a default_model \
                 the runtime cannot resolve is not written"
            ));
        }
        Ok(format!("{provider}/{model}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- [connector_proxy] (doc 24 §5.1, doc 26) ------------------------

    fn enabled_proxy(allow: Option<&[&str]>, deny: &[&str]) -> ConnectorProxyPolicy {
        ConnectorProxyPolicy::from_declared(&AgentStoreConnectorProxy {
            enabled: Some(true),
            allow: allow.map(|entries| entries.iter().map(|entry| (*entry).to_owned()).collect()),
            deny: Some(deny.iter().map(|entry| (*entry).to_owned()).collect()),
        })
    }

    #[test]
    fn connector_proxy_denies_everything_by_default() {
        // The whole point of the table: an absent one grants nothing.
        let policy = ConnectorProxyPolicy::deny_all();
        assert!(!policy.is_enabled());
        assert!(policy.decide("id-1", "github", "create_issue").is_err());
    }

    #[test]
    fn an_absent_table_denies_everything() {
        // `AgentStoreConfig::default()` has no `[connector_proxy]` table, which
        // must read as "off" rather than "permissive".
        let config = AgentStoreConfig::default();
        assert!(!config.connector_proxy_policy().is_enabled());
    }

    /// The behaviour change of doc `26` §4.4, stated as a test: turning the
    /// proxy on *is* the grant now, and `allow` only narrows it.
    #[test]
    fn enabling_without_an_allow_grants_every_enabled_connector() {
        let policy = enabled_proxy(None, &[]);
        assert!(policy.is_enabled());
        assert!(policy.decide("id-1", "github", "create_issue").is_ok());
        assert!(policy.decide("id-1", "github", "delete_repo").is_ok());
        assert!(policy.decide("id-2", "anything-at-all", "whatever").is_ok());
    }

    /// "I did not narrow" and "I narrowed to nothing" are different statements.
    #[test]
    fn an_empty_allow_narrows_to_nothing_but_an_absent_one_does_not() {
        assert!(enabled_proxy(Some(&[]), &[]).decide("id-1", "github", "create_issue").is_err());
        assert!(enabled_proxy(None, &[]).decide("id-1", "github", "create_issue").is_ok());
        // Blank entries are blanks, not wildcards.
        assert!(enabled_proxy(Some(&["", "   "]), &[]).decide("id-1", "github", "x").is_err());
    }

    #[test]
    fn allow_narrows_to_the_named_patterns_and_matches_by_name_or_by_id() {
        let by_name = enabled_proxy(Some(&["mcp__github__create_issue"]), &[]);
        assert!(by_name.decide("some-id", "github", "create_issue").is_ok());
        // A different tool on the same connector is not covered.
        assert!(by_name.decide("some-id", "github", "delete_repo").is_err());
        // Nor is the same tool on a different connector.
        assert!(by_name.decide("some-id", "gitlab", "create_issue").is_err());

        // The id spelling is offered because servers are upserted *by name*: a
        // later install can take over a name, and the id form is the pin that
        // survives it.
        let by_id = enabled_proxy(Some(&["mcp__some-id__create_issue"]), &[]);
        assert!(by_id.decide("some-id", "github", "create_issue").is_ok());
        assert!(by_id.decide("some-id", "renamed-later", "create_issue").is_ok());

        // A whole connector at once — the unit doc `26` moved the grant to.
        let whole = enabled_proxy(Some(&["mcp__github__*"]), &[]);
        assert!(whole.decide("some-id", "github", "create_issue").is_ok());
        assert!(whole.decide("some-id", "github", "delete_repo").is_ok());
        assert!(whole.decide("some-id", "gitlab", "create_issue").is_err());
    }

    #[test]
    fn deny_subtracts_after_allow() {
        // Allow everything, subtract one tool.
        let policy = enabled_proxy(None, &["mcp__github__delete_repo"]);
        assert!(policy.decide("id-1", "github", "create_issue").is_ok());
        assert!(policy.decide("id-1", "github", "delete_repo").is_err());

        // Subtract across every connector by glob.
        let by_glob = enabled_proxy(None, &["mcp__*__delete_*"]);
        assert!(by_glob.decide("id-1", "github", "create_issue").is_ok());
        assert!(by_glob.decide("id-1", "github", "delete_repo").is_err());
        assert!(by_glob.decide("id-2", "gitlab", "delete_project").is_err());

        // `deny` wins over a narrowing that would have allowed the pair, and the
        // message says which gate refused.
        let both = enabled_proxy(Some(&["mcp__github__*"]), &["mcp__github__delete_repo"]);
        assert!(both.decide("id-1", "github", "create_issue").is_ok());
        let refused = both.decide("id-1", "github", "delete_repo").expect_err("denied");
        assert!(refused.contains("deny"), "{refused}");
        let not_allowed = both.decide("id-1", "gitlab", "create_issue").expect_err("denied");
        assert!(not_allowed.contains("allow"), "{not_allowed}");
    }

    #[test]
    fn a_disabled_proxy_refuses_even_a_declared_pair() {
        let policy = ConnectorProxyPolicy::from_declared(&AgentStoreConnectorProxy {
            enabled: Some(false),
            allow: Some(vec!["mcp__github__*".to_owned()]),
            deny: None,
        });
        assert!(policy.decide("id-1", "github", "create_issue").is_err());
    }

    /// The matching rule is the engine's, so its case table is copied verbatim
    /// from `nomi-tools/src/registry.rs` — if the wrapper here ever diverges,
    /// this fails rather than silently widening or narrowing a real grant.
    #[test]
    fn policy_entries_match_exactly_except_in_the_mcp_namespace() {
        assert!(tool_name_matches("mcp__github__list_issues", "mcp__github__list_issues"));
        assert!(
            !tool_name_matches("*", "mcp__github__list_issues"),
            "a bare wildcard is not a glob outside mcp__, so it selects nothing"
        );
        assert!(
            !tool_name_matches("mcp__github", "mcp__github__list_issues"),
            "an mcp__ name without a tool segment can never match"
        );
        assert!(
            !tool_name_matches("mcp__[", "mcp__github__list_issues"),
            "a malformed pattern is a non-match, not a panic"
        );
        assert!(tool_name_matches("mcp__github__*", "mcp__github__list_issues"));
        assert!(tool_name_matches("mcp__*", "mcp__github__list_issues"));
        assert!(!tool_name_matches("mcp__notion__*", "mcp__github__list_issues"));
    }

    /// The migration trap of doc `26` §4.4: pre-`mcp__` entries are dead, and
    /// the operator has to be told rather than left guessing why nothing works.
    #[test]
    fn warnings_name_configuration_that_cannot_do_what_it_looks_like() {
        // The whole point: enabling with no lists is a real grant, so say so.
        let open = enabled_proxy(None, &[]);
        assert_eq!(open.warnings().len(), 1);
        assert!(open.warnings()[0].contains("every tool"), "{:?}", open.warnings());

        // The stale spelling from before doc `26`.
        let stale = enabled_proxy(Some(&["github__create_issue"]), &[]);
        let warnings = stale.warnings();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("mcp__github__create_issue"), "{warnings:?}");

        // Present but empty is not the same as absent, and is worth a word.
        let empty = enabled_proxy(Some(&[]), &[]);
        assert_eq!(empty.warnings().len(), 1);
        assert!(empty.warnings()[0].contains("allows nothing"), "{:?}", empty.warnings());

        // A well-formed narrowing says nothing.
        assert!(enabled_proxy(Some(&["mcp__github__*"]), &["mcp__*__delete_*"]).warnings().is_empty());
        // And a disabled proxy says nothing at all.
        assert!(ConnectorProxyPolicy::deny_all().warnings().is_empty());
    }

    #[test]
    fn the_declared_table_parses_from_toml() {
        // End-to-end through the file shape an operator actually writes.
        let source = r#"
[connector_proxy]
enabled = true
allow = ["mcp__github__*"]
deny = ["mcp__github__delete_repo"]
"#;
        let config: AgentStoreConfig = toml::from_str(source).expect("config parses");
        let policy = config.connector_proxy_policy();
        assert!(policy.is_enabled());
        assert!(policy.decide("id", "github", "create_issue").is_ok());
        assert!(policy.decide("id", "github", "delete_repo").is_err());
        assert!(policy.decide("id", "gitlab", "create_issue").is_err());
    }

    const SAMPLE: &str = r#"
default_plan_mode = false
default_model = "opencode/mimo-v2.5-free"

[providers.opencode]
type = "openai"
api_key = "sk-test-not-a-real-key"
base_url = "https://opencode.ai/zen/v1"

[models."opencode/laguna-s-2.1-free"]
provider = "opencode"
model = "laguna-s-2.1-free"
max_context_size = 256000
display_name = "Laguna S 2.1 Free"

[models."opencode/mimo-v2.5-free"]
provider = "opencode"
model = "mimo-v2.5-free"
max_context_size = 200000
max_output_size = 32000
protocol = "anthropic"
display_name = "MiMo V2.5 Free"
reasoning_key = "reasoning_content"
"#;

    #[test]
    fn parses_provider_and_models_from_the_agent_store_file() {
        let config = AgentStoreConfig::load_from_str(SAMPLE);
        let (provider, model) = config
            .default_selection()
            .expect("default_model must resolve to provider/model");
        assert_eq!(provider, "opencode");
        assert_eq!(model, "mimo-v2.5-free");

        let opencode = config
            .providers
            .get("opencode")
            .expect("provider table must be present");
        assert_eq!(opencode.r#type.as_deref(), Some("openai"));
        assert_eq!(opencode.api_key.as_deref(), Some("sk-test-not-a-real-key"));
        assert_eq!(opencode.base_url.as_deref(), Some("https://opencode.ai/zen/v1"));

        assert_eq!(
            config.models_for_provider("opencode"),
            vec!["laguna-s-2.1-free", "mimo-v2.5-free"]
        );
        let limits = config.context_limits_for_provider("opencode");
        assert_eq!(limits.get("mimo-v2.5-free"), Some(&200_000));
        assert_eq!(limits.get("laguna-s-2.1-free"), Some(&256_000));
        let names = config.display_names_for_provider("opencode");
        assert_eq!(names.get("mimo-v2.5-free").map(String::as_str), Some("MiMo V2.5 Free"));
        let outputs = config.output_limits_for_provider("opencode");
        assert_eq!(outputs.get("mimo-v2.5-free"), Some(&32_000));
        // Only the model that declares the key is present: an absent
        // `max_output_size` must not be invented.
        assert_eq!(outputs.get("laguna-s-2.1-free"), None);
        let protocols = config.protocols_for_provider("opencode");
        assert_eq!(protocols.get("mimo-v2.5-free").map(String::as_str), Some("anthropic"));
        assert_eq!(protocols.get("laguna-s-2.1-free"), None);
    }

    /// The output ceiling feeds a column guarded by
    /// `CHECK (output_limit IS NULL OR output_limit > 0)`, so a non-positive
    /// value must be dropped here rather than forwarded into a failed write.
    /// "No ceiling declared" and "declared as 0" both mean NULL.
    #[test]
    fn output_limits_drop_non_positive_values_and_stay_per_provider() {
        let config = AgentStoreConfig::load_from_str(
            r#"
[providers.a]
type = "anthropic"

[providers.b]
type = "anthropic"

[models."a/zero"]
provider = "a"
model = "zero"
max_output_size = 0

[models."a/negative"]
provider = "a"
model = "negative"
max_output_size = -5

[models."a/absent"]
provider = "a"
model = "absent"

[models."a/good"]
provider = "a"
model = "good"
max_output_size = 8192

[models."b/other-provider"]
provider = "b"
model = "other-provider"
max_output_size = 4096
"#,
        );

        let limits = config.output_limits_for_provider("a");
        assert_eq!(limits.get("zero"), None, "0 must not reach the CHECK constraint");
        assert_eq!(limits.get("negative"), None);
        assert_eq!(limits.get("absent"), None);
        assert_eq!(limits.get("good"), Some(&8192));
        // A sibling provider's ceiling must not leak into this one.
        assert_eq!(limits.get("other-provider"), None);
        assert_eq!(config.output_limits_for_provider("b").get("other-provider"), Some(&4096));
    }

    /// Blank / whitespace-only `protocol` values are "absent", matching the
    /// trim-and-drop rule every other per-model map in this module uses.
    #[test]
    fn protocols_drop_blank_values() {
        let config = AgentStoreConfig::load_from_str(
            r#"
[providers.a]
type = "anthropic"

[models."a/blank"]
provider = "a"
model = "blank"
protocol = "   "

[models."a/real"]
provider = "a"
model = "real"
protocol = "anthropic"
"#,
        );

        let protocols = config.protocols_for_provider("a");
        assert_eq!(protocols.get("blank"), None);
        assert_eq!(protocols.get("real").map(String::as_str), Some("anthropic"));
    }

    /// Hand-edited host config with comments in every position the write path
    /// could destroy: a file-level comment, a trailing comment on the target
    /// key and on an unrelated key.
    const COMMENTED: &str = r#"# agent-store host config — hand edited, do not reformat
default_model = "opencode/mimo-v2.5-free"   # current pick

[providers.opencode]
type = "openai"
api_key = "«redacted:sk-…»"
base_url = "https://opencode.ai/zen/v1"   # keep

[models."opencode/mimo-v2.5-free"]
provider = "opencode"
model = "mimo-v2.5-free"
"#;

    #[test]
    fn default_model_write_touches_only_the_target_key() {
        let edited = AgentStoreConfig::with_default_model(COMMENTED, "opencode/laguna-s-2.1-free")
            .expect("edit must succeed");

        // Every comment, every other key and the layout survive.
        assert!(edited.contains("# agent-store host config — hand edited, do not reformat"));
        assert!(edited.contains("# current pick"));
        assert!(edited.contains("base_url = \"https://opencode.ai/zen/v1\"   # keep"));
        assert!(edited.contains("api_key = \"«redacted:sk-…»\""));
        assert_eq!(edited.matches("default_model").count(), 1);

        let parsed = AgentStoreConfig::from_source(&edited).expect("edited source must parse");
        assert_eq!(parsed.default_model.as_deref(), Some("opencode/laguna-s-2.1-free"));
        assert_eq!(parsed.providers.len(), 1);
        assert_eq!(parsed.models_for_provider("opencode"), vec!["mimo-v2.5-free"]);
    }

    #[test]
    fn default_model_write_inserts_a_top_level_key_when_absent() {
        let source = "# host config\n[providers.opencode]\ntype = \"openai\"\n";
        let edited = AgentStoreConfig::with_default_model(source, "opencode/mimo-v2.5-free")
            .expect("edit must succeed");

        // Inserted below the file header comment, above the first table, and
        // nothing else moved.
        assert_eq!(
            edited,
            "# host config\ndefault_model = \"opencode/mimo-v2.5-free\"\n[providers.opencode]\ntype = \"openai\"\n"
        );

        // Top level, not a late member of `[providers.opencode]`.
        let parsed = AgentStoreConfig::from_source(&edited).expect("edited source must parse");
        assert_eq!(parsed.default_model.as_deref(), Some("opencode/mimo-v2.5-free"));
        assert!(parsed.providers.contains_key("opencode"));
        // Writing the same value twice is stable (no key duplication).
        assert_eq!(
            AgentStoreConfig::with_default_model(&edited, "opencode/mimo-v2.5-free").unwrap(),
            edited
        );

        // A file with no header comment gets the key as line 1.
        let bare = AgentStoreConfig::with_default_model(
            "[providers.octo]\ntype = \"openai\"\n",
            "octo/coral",
        )
        .expect("edit must succeed");
        assert_eq!(
            bare,
            "default_model = \"octo/coral\"\n[providers.octo]\ntype = \"openai\"\n"
        );
        assert_eq!(
            AgentStoreConfig::from_source(&bare).unwrap().default_model.as_deref(),
            Some("octo/coral")
        );
    }

    #[test]
    fn default_model_write_refuses_an_unparseable_file() {
        let error = AgentStoreConfig::with_default_model("default_model = \"oops\n", "octo/coral")
            .expect_err("invalid TOML must never be silently rewritten");
        assert!(error.contains("not valid TOML"), "{error}");
    }

    #[test]
    fn config_patch_whitelists_only_resolvable_default_models() {
        let config = AgentStoreConfig::load_from_str(SAMPLE);

        let patch = AgentStoreConfigPatch {
            default_model: Some("  opencode/mimo-v2.5-free  ".to_owned()),
            memory: None,
            tools: None,
        };
        assert_eq!(
            patch.validated_default_model(&config).unwrap(),
            "opencode/mimo-v2.5-free"
        );

        // An undeclared *model* under a declared provider is resolvable: the
        // runtime registers the requested model itself.
        let patch = AgentStoreConfigPatch {
            default_model: Some("opencode/never-heard-of-it".to_owned()),
            memory: None,
            tools: None,
        };
        assert_eq!(
            patch.validated_default_model(&config).unwrap(),
            "opencode/never-heard-of-it"
        );

        for bad in [
            "",
            "   ",
            "opencode",
            "/mimo-v2.5-free",
            "opencode/",
            "ghost/model",
            "opencode/a\nb",
        ] {
            let patch = AgentStoreConfigPatch {
                default_model: Some(bad.to_owned()),
                memory: None,
                tools: None,
            };
            assert!(patch.validated_default_model(&config).is_err(), "{bad:?} must be refused");
        }

        // No writable field at all: refused, never a silent no-op.
        assert!(
            AgentStoreConfigPatch {
                default_model: None,
                memory: None,
                tools: None,
            }
            .validated_default_model(&config)
            .is_err()
        );
    }

    #[test]
    fn config_patch_rejects_credentials_and_paths() {
        // The request type is the boundary: an out-of-envelope key fails to
        // parse instead of being silently ignored.
        for body in [
            r#"{"api_key":"sk-live-not-a-real-key"}"#,
            r#"{"path":"/tmp/other.toml"}"#,
            r#"{"owner":"someone-else"}"#,
            r#"{"default_model":"opencode/mimo-v2.5-free","base_url":"http://evil"}"#,
        ] {
            assert!(
                serde_json::from_str::<AgentStoreConfigPatch>(body).is_err(),
                "{body} must be refused by the whitelist"
            );
        }

        let ok: AgentStoreConfigPatch =
            serde_json::from_str(r#"{"default_model":"opencode/mimo-v2.5-free"}"#)
                .expect("whitelisted field");
        assert_eq!(ok.default_model.as_deref(), Some("opencode/mimo-v2.5-free"));
        assert!(serde_json::from_str::<AgentStoreConfigPatch>("{}").expect("empty patch").default_model.is_none());
    }

    #[test]
    fn builtin_marketplaces_are_complete_and_resolvable() {
        let builtin = AgentStoreConfig::builtin_default_marketplaces();
        let ids: Vec<&str> = builtin.iter().map(|(id, _, _)| id.as_str()).collect();
        assert_eq!(ids, ["experts", "skills", "connectors"]);
        for (id, kind, source) in &builtin {
            assert_eq!(kind, "zip");
            assert!(source.ends_with(&format!("/{id}.zip")), "{id} source: {source}");
            assert!(source.starts_with("https://"), "{id} source must be absolute: {source}");
        }
        // The release default is the product's own market host. Pinned because
        // a typo'd or moved host would (a) point every fresh install at a dead
        // mirror and (b) silently re-classify the real sources as third-party
        // via `is_official_source` (auto-update off). Since doc 30 each market
        // ships as **one archive** (not a file-per-entry tree on the site), and
        // `master` is the mutable market tip — publishing a market must not
        // require shipping a new binary.
        let sources: Vec<&str> = builtin.iter().map(|(_, _, source)| source.as_str()).collect();
        assert_eq!(
            sources,
            [
                "https://www.modelscope.cn/models/me9rez/flowy-marketplace/resolve/master/experts.zip",
                "https://www.modelscope.cn/models/me9rez/flowy-marketplace/resolve/master/skills.zip",
                "https://www.modelscope.cn/models/me9rez/flowy-marketplace/resolve/master/connectors.zip",
            ]
        );
    }

    /// "Resolvable" is the half of `…_complete_and_resolvable` that a second,
    /// hand-written kind list can silently break: these sources are `zip`, and
    /// a config declaring them is exactly what `agent-store init` writes. The
    /// round-trip below is the property that matters to a user — the loader
    /// accepts what the release default (and the generated template) emits —
    /// so a kind the loader refuses fails here instead of ending up as an
    /// empty store that reports itself complete.
    #[test]
    fn builtin_marketplaces_survive_the_config_loader() {
        let builtin = AgentStoreConfig::builtin_default_marketplaces();
        let toml: String = builtin
            .iter()
            .map(|(id, kind, source)| {
                format!(
                    "[default_marketplaces.{id}]\nsource_kind = \"{kind}\"\nsource = \"{source}\"\n\n"
                )
            })
            .collect();
        let config = AgentStoreConfig::from_source(&toml).expect("builtin sources must parse");
        assert_eq!(config.default_marketplaces.len(), builtin.len());
        for (id, kind, source) in &builtin {
            let entry = config
                .default_marketplaces
                .get(id)
                .unwrap_or_else(|| panic!("{id} must survive the load"));
            assert_eq!(
                entry.resolved().as_ref(),
                Some(&(kind.clone(), source.clone())),
                "{id} must resolve to what the builtin list declared"
            );
            // The kind string is only usable if the *other* authority — the one
            // that resolves it into a real source — recognises it too.
            assert!(
                nomifun_api_types::AppServerMarketplaceSourceKind::parse(kind).is_some(),
                "{id}: parse() must know kind `{kind}`"
            );
        }
    }

    /// A kind this build does not have must stay visible: `resolved()` passes
    /// it through so the registration loop can warn and report the run
    /// incomplete, rather than dropping the entry and reporting success.
    #[test]
    fn unknown_source_kind_is_passed_through_not_dropped() {
        let config = AgentStoreConfig::from_source(
            "[default_marketplaces.typo]\nsource_kind = \"zipp\"\nsource = \"https://example.com/m.zip\"\n",
        )
        .expect("config must parse");
        let entry = config.default_marketplaces.get("typo").expect("entry kept");
        assert_eq!(
            entry.resolved(),
            Some(("zipp".to_owned(), "https://example.com/m.zip".to_owned())),
            "an unknown kind must reach the caller, which warns and marks the run incomplete"
        );
        assert!(nomifun_api_types::AppServerMarketplaceSourceKind::parse("zipp").is_none());
    }

    #[test]
    fn unknown_top_level_sections_are_tolerated() {
        let config = AgentStoreConfig::load_from_str(
            r#"
default_model = "octo/coral"
telemetry = false

[experimental]
secondary-model = true

[providers.octo]
type = "openai"
api_key = "sk-test"
base_url = "https://example.test/v1"
"#,
        );
        assert_eq!(config.default_selection().unwrap().0, "octo");
        assert!(config.providers.contains_key("octo"));
    }

    #[test]
    fn memory_distill_gate_is_optional_and_explicit() {
        // No `[memory]` table → None: the upstream nomi default stays in charge.
        let absent = AgentStoreConfig::load_from_str("default_model = \"octo/coral\"\n");
        assert!(absent.memory.is_none());

        // Declared but silent → present, no opinion.
        let silent = AgentStoreConfig::load_from_str("[memory]\n");
        assert_eq!(silent.memory.and_then(|memory| memory.distill_enabled), None);

        // The switch the Agent Store host reads (doc 16 / D-STREAM-2).
        let off = AgentStoreConfig::load_from_str("[memory]\ndistill_enabled = false\n");
        assert_eq!(off.memory.and_then(|memory| memory.distill_enabled), Some(false));
        let on = AgentStoreConfig::load_from_str("[memory]\ndistill_enabled = true\n");
        assert_eq!(on.memory.and_then(|memory| memory.distill_enabled), Some(true));
    }

    #[test]
    fn memory_enabled_reads_the_real_file_shape() {
        // The shipped `agent-store init` output is a *full* file: providers,
        // models and (now) a host-policy table. Assert the switch resolves
        // correctly alongside all of it, not just in isolation — a realistic
        // file is where a misplaced key or an interaction with a sibling table
        // would actually show up.
        let source = format!("{SAMPLE}\n[memory]\nenabled = false\n");
        let config = AgentStoreConfig::load_from_str(&source);
        assert!(!config.memory_enabled());
        // The sibling tables must be unaffected by the new section.
        assert!(
            config.default_selection().is_some(),
            "default_model must still resolve"
        );
        assert!(config.providers.contains_key("opencode"));
    }

    #[test]
    fn memory_enabled_defaults_on_and_is_independent_of_distill() {
        // No `[memory]` table at all → the system stays on, so a host that only
        // ever wrote providers behaves exactly as it did before the key existed.
        let absent = AgentStoreConfig::load_from_str("default_model = \"octo/coral\"\n");
        assert!(absent.memory_enabled());

        // Declared but silent → still on: the table alone is not consent to turn
        // the whole memory system off.
        let silent = AgentStoreConfig::load_from_str("[memory]\n");
        assert!(silent.memory_enabled());

        // The wide switch this host reads for the prompt section + `remember`.
        let off = AgentStoreConfig::load_from_str("[memory]\nenabled = false\n");
        assert!(!off.memory_enabled());
        let on = AgentStoreConfig::load_from_str("[memory]\nenabled = true\n");
        assert!(on.memory_enabled());

        // The two keys are independent, in both directions: turning the wide
        // switch off while distillation is nominally on must still disable the
        // system, and vice versa must keep it on.
        let wide_off_narrow_on =
            AgentStoreConfig::load_from_str("[memory]\nenabled = false\ndistill_enabled = true\n");
        assert!(!wide_off_narrow_on.memory_enabled());
        assert_eq!(
            wide_off_narrow_on
                .memory
                .as_ref()
                .and_then(|memory| memory.distill_enabled),
            Some(true),
            "the narrow key must still parse; `enabled` is what gates it"
        );

        let wide_on_narrow_off =
            AgentStoreConfig::load_from_str("[memory]\nenabled = true\ndistill_enabled = false\n");
        assert!(wide_on_narrow_off.memory_enabled());
        assert_eq!(
            wide_on_narrow_off
                .memory
                .as_ref()
                .and_then(|memory| memory.distill_enabled),
            Some(false)
        );
    }

    #[test]
    fn import_strict_dependencies_is_an_explicit_opt_in() {
        // No `[import]` table → off (the historical warn-only behaviour).
        let absent = AgentStoreConfig::load_from_str("default_model = \"octo/coral\"\n");
        assert!(!absent.strict_dependencies());

        // A table without the key stays off — the table alone is not consent.
        let silent = AgentStoreConfig::load_from_str("[import]\n");
        assert!(!silent.strict_dependencies());

        let off = AgentStoreConfig::load_from_str("[import]\nstrict_dependencies = false\n");
        assert!(!off.strict_dependencies());

        // The switch the extension registry reads (`16` R23 / `17` §7).
        let on = AgentStoreConfig::load_from_str("[import]\nstrict_dependencies = true\n");
        assert!(on.strict_dependencies());
        assert!(on.import.is_some());
    }

    #[test]
    fn marketplace_cadence_is_off_unless_declared() {
        use std::time::Duration;

        // No `[marketplace]` table → no sweep at all.
        let absent = AgentStoreConfig::load_from_str("default_model = \"octo/coral\"\n");
        assert!(absent.marketplace.is_none());

        // A table without a cadence stays off (the table alone is not consent).
        let silent = AgentStoreConfig::load_from_str("[marketplace]\n");
        assert_eq!(silent.marketplace.unwrap_or_default().cadence(), None);

        // `0` reads as an explicit off, not as "every tick".
        let zero =
            AgentStoreConfig::load_from_str("[marketplace]\nauto_update_interval_hours = 0\n");
        assert_eq!(zero.marketplace.unwrap_or_default().cadence(), None);

        let six =
            AgentStoreConfig::load_from_str("[marketplace]\nauto_update_interval_hours = 6\n");
        assert_eq!(
            six.marketplace.unwrap_or_default().cadence(),
            Some(Duration::from_secs(6 * 3_600))
        );
    }

    #[test]
    fn distill_enabled_write_creates_updates_and_preserves_the_rest() {
        // 1. No `[memory]` table at all → one is appended, nothing else moves.
        let created = AgentStoreConfig::with_distill_enabled(
            "# allo host config\ndefault_model = \"opencode/mimo-v2.5-free\"\n\n[providers.opencode]\ntype = \"openai\"\n",
            false,
        )
        .expect("edit");
        assert!(created.contains("[memory]"), "{created}");
        assert!(created.contains("distill_enabled = false"), "{created}");
        assert!(created.contains("# allo host config"), "{created}");
        assert!(created.contains("default_model = \"opencode/mimo-v2.5-free\""), "{created}");
        // The new table did not swallow the provider table's keys.
        let reparsed = AgentStoreConfig::load_from_str(&created);
        assert!(reparsed.providers.contains_key("opencode"));
        assert_eq!(reparsed.memory.and_then(|m| m.distill_enabled), Some(false));

        // 2. Existing key → only its value moves; the sibling survives.
        let updated = AgentStoreConfig::with_distill_enabled(
            "[memory]\ndistill_enabled = true\nother_flag = 1   # keep me\n",
            false,
        )
        .expect("edit");
        assert_eq!(updated.matches("distill_enabled").count(), 1, "{updated}");
        assert!(updated.contains("distill_enabled = false"), "{updated}");
        assert!(updated.contains("other_flag = 1   # keep me"), "{updated}");

        // 3. Dotted form is the same table: upgraded in place, never duplicated.
        let dotted =
            AgentStoreConfig::with_distill_enabled("memory.distill_enabled = true\n", false)
                .expect("edit");
        assert_eq!(dotted.matches("distill_enabled").count(), 1, "{dotted}");
        assert_eq!(
            AgentStoreConfig::load_from_str(&dotted)
                .memory
                .and_then(|m| m.distill_enabled),
            Some(false)
        );

        // 4. Unparseable input stays a refusal, never a silent overwrite.
        assert!(AgentStoreConfig::with_distill_enabled("not = = toml\n", true).is_err());
    }

    #[test]
    fn config_patch_names_a_key_only_when_one_is_actually_present() {
        let empty: AgentStoreConfigPatch = serde_json::from_value(serde_json::json!({})).expect("empty");
        assert!(!empty.names_any_key());

        let model: AgentStoreConfigPatch =
            serde_json::from_value(serde_json::json!({ "default_model": "octo/coral" })).expect("model");
        assert!(model.names_any_key());

        let memory: AgentStoreConfigPatch = serde_json::from_value(
            serde_json::json!({ "memory": { "distill_enabled": false } }),
        )
        .expect("memory");
        assert!(memory.names_any_key());

        // An empty `[memory]` table names nothing, and credentials are refused
        // at parse time (R22 gate: no write face accepts them).
        let empty_memory: AgentStoreConfigPatch =
            serde_json::from_value(serde_json::json!({ "memory": {} })).expect("empty memory");
        assert!(!empty_memory.names_any_key());
        assert!(serde_json::from_value::<AgentStoreConfigPatch>(
            serde_json::json!({ "memory": { "api_key": "«redacted»" } })
        )
        .is_err());

        // `[credentials]` is likewise absent from the write whitelist: a patch
        // cannot even name it, so the protocol has no way to write a credential.
        assert!(serde_json::from_value::<AgentStoreConfigPatch>(
            serde_json::json!({ "credentials": { "DEMO_TOKEN": "x" } })
        )
        .is_err());
    }

    /// R22 (`17` §6 / `21` D5=C): `[credentials]` parses into the plain map the
    /// host installs in-process (`set_credentials`). Hand-edited, never on the
    /// wire — the read view omits it and the write patch cannot name it.
    #[test]
    fn parses_credentials_table() {
        let config = AgentStoreConfig::load_from_str(
            "[credentials]\nDEMO_TOKEN = \"s3cr3t\"\nOTHER = \"v\"\n",
        );
        assert_eq!(
            config.credentials.get("DEMO_TOKEN").map(String::as_str),
            Some("s3cr3t")
        );
        assert_eq!(config.credentials.get("OTHER").map(String::as_str), Some("v"));
    }

    const TOOLS_SAMPLE: &str = r#"# host settings
[tools]
# the denylist is the only way to drop an always-registered tool
disabled = ["remember"]  # trailing note
computer = false

[tools.domains]
cron = false
meeting = false

[providers.opencode]
type = "openai"
"#;

    #[test]
    fn parses_tools_table_into_the_typed_policy() {
        let config = AgentStoreConfig::load_from_str(TOOLS_SAMPLE);
        let policy = config.tool_policy();

        assert_eq!(policy.disabled, vec!["remember".to_owned()]);
        assert!(!policy.computer, "declared switch must be read");
        assert!(!policy.domains.cron && !policy.domains.meeting);
        // Undeclared switches stay on: `[tools]` only ever narrows.
        assert!(policy.web && policy.browser && policy.plan && policy.lsp);
        assert!(policy.domains.knowledge && policy.domains.goal);
        assert!(policy.enabled.is_empty());
    }

    #[test]
    fn absent_tools_table_is_the_unrestricted_policy() {
        // The pre-`[tools]` file shape must stay byte-for-byte equivalent.
        let config = AgentStoreConfig::load_from_str(SAMPLE);
        assert!(config.tools.is_none());
        assert!(config.tool_policy().is_unrestricted());

        let empty = AgentStoreConfig::load_from_str("");
        assert!(empty.tools.is_none());
        assert!(empty.tool_policy().is_unrestricted());
    }

    #[test]
    fn with_tool_lists_touches_only_the_named_keys() {
        // Deliberately unsorted input: the writer canonicalises it.
        let edited = AgentStoreConfig::with_tool_lists(
            TOOLS_SAMPLE,
            None,
            Some(&["update_plan".to_owned(), "remember".to_owned()]),
        )
        .expect("edit must succeed");

        // The list is replaced (not appended to) and every comment survives —
        // including the one written *above* the key, which owns the key's decor.
        assert!(edited.contains("# host settings"), "{edited}");
        assert!(edited.contains("# the denylist is the only way"), "{edited}");
        assert!(edited.contains("# trailing note"), "{edited}");
        assert!(edited.contains("update_plan"), "{edited}");
        // Untouched siblings keep their values.
        assert!(edited.contains("computer = false"), "{edited}");
        assert!(edited.contains("cron = false"), "{edited}");
        assert!(edited.contains("[providers.opencode]"), "{edited}");
        // And the result still parses into the canonical policy.
        let reparsed = AgentStoreConfig::load_from_str(&edited);
        assert_eq!(
            reparsed.tool_policy().disabled,
            vec!["remember".to_owned(), "update_plan".to_owned()]
        );
    }

    #[test]
    fn with_tool_switch_creates_the_table_when_absent() {
        let edited = AgentStoreConfig::with_tool_switch(SAMPLE, "lsp", false)
            .expect("edit must succeed");
        let reparsed = AgentStoreConfig::load_from_str(&edited);
        assert!(!reparsed.tool_policy().lsp);
        // Existing providers/models are untouched by the inserted table.
        assert_eq!(
            reparsed.default_selection(),
            Some(("opencode".to_owned(), "mimo-v2.5-free".to_owned()))
        );
    }

    #[test]
    fn a_credential_edit_preserves_every_other_key_and_its_comments() {
        // This file is hand-edited by design (D5=C): writing a credential must not
        // reformat or re-comment the rest of it.
        let source = "# my host\n[memory]\ndistill_enabled = false\n\n[credentials]\n# the old demo token\nDEMO_TOKEN = \"old\"\n";
        let edited = AgentStoreConfig::with_credential(source, "DEMO_TOKEN", "new")
            .expect("edit must succeed");

        assert!(edited.contains("# my host"), "{edited}");
        assert!(edited.contains("# the old demo token"), "{edited}");
        assert!(edited.contains("distill_enabled = false"), "{edited}");
        let reparsed = AgentStoreConfig::load_from_str(&edited);
        assert!(reparsed.credentials.get("DEMO_TOKEN").is_some_and(|v| v == "new"));
        assert!(!reparsed.memory.as_ref().and_then(|m| m.distill_enabled).unwrap_or(true));
    }

    #[test]
    fn a_per_principal_credential_round_trips_through_a_quoted_key() {
        // `alice:TOKEN` is not a bare TOML key, so it must come back quoted — and
        // parse back to the same map entry (34 §7).
        let edited = AgentStoreConfig::with_credential("", "alice:TOKEN", "a")
            .expect("edit must succeed");
        let reparsed = AgentStoreConfig::load_from_str(&edited);
        assert_eq!(
            reparsed.credentials.get("alice:TOKEN").map(String::as_str),
            Some("a")
        );
        // The host-level key and the per-principal key coexist.
        let edited = AgentStoreConfig::with_credential(&edited, "TOKEN", "host")
            .expect("edit must succeed");
        let reparsed = AgentStoreConfig::load_from_str(&edited);
        assert_eq!(reparsed.credentials.len(), 2);
        assert_eq!(reparsed.credentials.get("TOKEN").map(String::as_str), Some("host"));
    }

    #[test]
    fn clearing_a_credential_is_idempotent_and_leaves_the_rest_alone() {
        let source = "[credentials]\nA = \"1\"\nB = \"2\"\n\n[tools]\nlsp = false\n";
        let edited = AgentStoreConfig::without_credential(source, "A").expect("edit");
        let reparsed = AgentStoreConfig::load_from_str(&edited);
        assert!(!reparsed.credentials.contains_key("A"));
        assert_eq!(reparsed.credentials.get("B").map(String::as_str), Some("2"));
        assert!(!reparsed.tool_policy().lsp);

        // Clearing again — or a key that was never there — is success.
        let again = AgentStoreConfig::without_credential(&edited, "A").expect("edit");
        assert_eq!(AgentStoreConfig::load_from_str(&again).credentials.len(), 1);
        assert!(AgentStoreConfig::without_credential("", "NEVER_SET").is_ok());
    }

    #[test]
    fn a_credential_key_must_not_be_empty() {
        assert!(AgentStoreConfig::with_credential("", "", "v").is_err());
        assert!(AgentStoreConfig::with_credential("", "   ", "v").is_err());
        assert!(AgentStoreConfig::without_credential("", "").is_err());
    }

    #[test]
    fn with_tool_domain_creates_the_nested_table_when_absent() {
        let edited = AgentStoreConfig::with_tool_domain(SAMPLE, "media", false)
            .expect("edit must succeed");
        let reparsed = AgentStoreConfig::load_from_str(&edited);
        assert!(!reparsed.tool_policy().domains.media);
        assert!(reparsed.tool_policy().domains.cron);
    }

    #[test]
    fn tool_writers_refuse_keys_outside_the_whitelist() {
        assert!(AgentStoreConfig::with_tool_switch(SAMPLE, "credentials", false).is_err());
        assert!(AgentStoreConfig::with_tool_domain(SAMPLE, "unknown", false).is_err());
        // A non-table value under the key is a refusal, never an overwrite.
        assert!(AgentStoreConfig::with_tool_switch("tools = 3\n", "web", false).is_err());
        // Unparseable input stays a refusal.
        assert!(AgentStoreConfig::with_tool_switch("not = = toml\n", "web", false).is_err());
    }

    #[test]
    fn tools_patch_names_a_key_only_when_one_is_actually_present() {
        let empty: AgentStoreConfigPatch = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(!empty.names_any_key());

        let empty_tools: AgentStoreConfigPatch =
            serde_json::from_value(serde_json::json!({ "tools": {} })).unwrap();
        assert!(
            !empty_tools.names_any_key(),
            "an empty [tools] table names nothing and must be refused like an empty [memory]"
        );

        let switch: AgentStoreConfigPatch =
            serde_json::from_value(serde_json::json!({ "tools": { "computer": false } })).unwrap();
        assert!(switch.names_any_key());

        let domain: AgentStoreConfigPatch = serde_json::from_value(
            serde_json::json!({ "tools": { "domains": { "cron": false } } }),
        )
        .unwrap();
        assert!(domain.names_any_key());

        let lists: AgentStoreConfigPatch = serde_json::from_value(
            serde_json::json!({ "tools": { "enabled": [], "disabled": ["remember"] } }),
        )
        .unwrap();
        assert!(lists.names_any_key());

        // The write whitelist is the security boundary: an unknown key inside
        // `[tools]` is a hard refusal, not a silently dropped field.
        assert!(serde_json::from_value::<AgentStoreConfigPatch>(
            serde_json::json!({ "tools": { "force_enable_everything": true } })
        )
        .is_err());
        assert!(serde_json::from_value::<AgentStoreConfigPatch>(
            serde_json::json!({ "tools": { "domains": { "bogus": false } } })
        )
        .is_err());
    }

    #[test]
    fn tools_patch_apply_edits_canonicalises_lists_and_applies_switches() {
        let patch: AgentStoreConfigPatch = serde_json::from_value(serde_json::json!({
            "tools": {
                "disabled": [" update_plan ", "remember", "remember", ""],
                "computer": false,
                "domains": { "media": false }
            }
        }))
        .unwrap();

        let edited = patch
            .tools
            .as_ref()
            .unwrap()
            .apply_edits(TOOLS_SAMPLE)
            .expect("edit must succeed");
        let reparsed = AgentStoreConfig::load_from_str(&edited);
        let policy = reparsed.tool_policy();

        // Trimmed, deduplicated, sorted — and an explicit empty entry dropped.
        assert_eq!(policy.disabled, vec!["remember", "update_plan"]);
        assert!(!policy.computer);
        assert!(!policy.domains.media);
        assert!(!policy.domains.cron, "an unrelated domain edit must not reset siblings");
    }

    #[test]
    fn tools_patch_refuses_control_characters_without_writing_anything() {
        let patch: AgentStoreConfigPatch = serde_json::from_value(serde_json::json!({
            "tools": { "disabled": ["bad\u{7}name"] }
        }))
        .unwrap();
        assert!(
            patch
                .tools
                .as_ref()
                .unwrap()
                .apply_edits(TOOLS_SAMPLE)
                .is_err()
        );
    }

    impl AgentStoreConfig {
        fn load_from_str(raw: &str) -> Self {
            toml::from_str(raw).expect("sample config must parse")
        }
    }
}
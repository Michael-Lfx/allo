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
    /// Default marketplace sources (store software sources) registered
    /// automatically by the App Server; keyed by a stable marketplace id
    /// (`experts` etc.).
    ///
    /// ```toml
    /// [default_marketplaces.experts]
    /// source_kind = "url"      # url | github | git | directory
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
/// Upstream (`nomi` `[memory].distill_enabled`) defaults to ON: after every
/// human turn the runtime makes one extra model call that distils the session
/// into file-based memory. That call is awaited **before** the turn's terminal
/// `Finish`, so a client sees its answer complete while the turn still reports
/// "processing" for the whole call (measured 6–15s in the agent-store host).
/// Declaring this table is how the Agent Store host opts out without touching
/// upstream defaults for other hosts.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentStoreMemory {
    /// `false` disables post-answer distillation on this host; `true` or an
    /// absent key keeps the upstream default. `NOMIFUN_MEMORY_DISTILL` still
    /// overrides whichever value lands here.
    #[serde(default)]
    pub distill_enabled: Option<bool>,
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
    /// `url` | `github` | `git` | `directory`.
    #[serde(default)]
    pub source_kind: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
}

impl AgentStoreMarketplace {
    /// Resolve to `(source_kind, source)` when both are present and valid.
    pub fn resolved(&self) -> Option<(String, String)> {
        let kind = self.source_kind.as_deref()?.trim().to_owned();
        let source = self.source.as_deref()?.trim().to_owned();
        if kind.is_empty() || source.is_empty() {
            return None;
        }
        if !matches!(kind.as_str(), "url" | "github" | "git" | "directory") {
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
    #[serde(default)]
    pub max_output_size: Option<i64>,
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

    /// `load` flattened to `Option` for read-only catalog projections: a
    /// missing/unparseable config simply contributes nothing.
    pub fn load_ok(path: &Path) -> Option<Self> {
        Self::load(path).ok()
    }

    /// Builtin marketplace sources used when `~/.agent-store/config.toml` is
    /// missing (or has no `[default_marketplaces]`): the official public
    /// mirror, so a fresh install can browse the store before touching any
    /// config. `id -> (source_kind, source)`.
    ///
    /// NOTE: the public mirror host is expected to move to a domain-backed
    /// HTTPS endpoint (`https://market.flowyaipc.cn/...`) once DNS/SSL are
    /// wired up; the fallback short-circuits whenever the user declares their
    /// own `[default_marketplaces]`.
    pub fn builtin_default_marketplaces() -> Vec<(String, String, String)> {
        vec![
            (
                "experts".to_owned(),
                "url".to_owned(),
                "http://111.170.173.22:10072/experts/.codebuddy-plugin/marketplace.json".to_owned(),
            ),
            (
                "skills".to_owned(),
                "url".to_owned(),
                "http://111.170.173.22:10072/skills/.codebuddy-skill/marketplace.json".to_owned(),
            ),
            (
                "connectors".to_owned(),
                "url".to_owned(),
                "http://111.170.173.22:10072/connectors/.codebuddy-connector/connectors.json".to_owned(),
            ),
        ]
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
            let patch = AgentStoreConfigPatch { default_model: Some(bad.to_owned()), memory: None };
            assert!(patch.validated_default_model(&config).is_err(), "{bad:?} must be refused");
        }

        // No writable field at all: refused, never a silent no-op.
        assert!(
            AgentStoreConfigPatch { default_model: None, memory: None }
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
            assert_eq!(kind, "url");
            assert!(source.ends_with("marketplace.json") || source.ends_with("connectors.json"));
            assert!(source.starts_with("http"), "{id} source must be absolute: {source}");
        }
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

    impl AgentStoreConfig {
        fn load_from_str(raw: &str) -> Self {
            toml::from_str(raw).expect("sample config must parse")
        }
    }
}
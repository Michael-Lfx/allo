//! Manifest parsing and path validation (docs/agent-store/02 §2 steps 1–4).
//!
//! V1 supports four local source kinds. A marketplace manifest is a much
//! lighter shape than a plugin manifest; both converge on the same
//! `PluginManifest`-like identity fields. A single CLI connector directory
//! (`cli.json` + `skills/`) has no manifest identity of its own — the
//! directory name becomes the plugin id (real CodeBuddy markets layout).

use std::path::Path;

use serde::Deserialize;
use serde_json::json;

use crate::models::SourceKind;

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("manifest read failed: {0}")]
    Io(String),
    #[error("manifest parse failed: {0}")]
    Json(String),
    #[error("manifest lacks a required identity field: {0}")]
    MissingIdentity(String),
    #[error("unsafe manifest path: {0}")]
    UnsafePath(String),
}

/// `teamInfo` extension (02 §6, WorkBuddy/CodeBuddy specific).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamInfo {
    pub lead_agent: String,
    pub member_agents: Vec<String>,
    #[serde(default)]
    pub expert_type: Option<String>,
}

/// Localized display metadata. Real markets emit both a plain string
/// (`"displayName": "FBSir"`) and a locale map
/// (`{"en": "...", "zh": "..."}`); both normalize to a locale map.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LocalizedText {
    pub en: Option<String>,
    pub zh: Option<String>,
}

impl LocalizedText {
    /// First value that is non-empty, preferring `zh` when both exist.
    pub fn primary(&self) -> Option<&str> {
        self.zh.as_deref().or(self.en.as_deref())
    }

    pub fn is_empty(&self) -> bool {
        self.en.is_none() && self.zh.is_none()
    }
}

fn deserialize_localized_text<'de, D>(deserializer: D) -> Result<Option<LocalizedText>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::String(text) if !text.is_empty() => Some(LocalizedText { en: None, zh: Some(text) }),
        serde_json::Value::Object(map) => {
            let mut out = LocalizedText::default();
            for (key, value) in map {
                if let Some(text) = value.as_str() {
                    match key.as_str() {
                        "en" => out.en = Some(text.to_owned()),
                        "zh" => out.zh = Some(text.to_owned()),
                        _ => {}
                    }
                }
            }
            if out.is_empty() { None } else { Some(out) }
        }
        _ => None,
    })
}

/// A list of localized strings (`quickPrompts` / `tags`): either an array of
/// plain strings (`["text-a", "text-b"]`) or an array of locale maps
/// (`[{"en": "...", "zh": "..."}]`). Each item retains its own locale pair.
fn deserialize_localized_list<'de, D>(deserializer: D) -> Result<Vec<LocalizedText>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Array(items) => items
            .into_iter()
            .filter_map(|item| match item {
                serde_json::Value::String(text) if !text.is_empty() => {
                    Some(LocalizedText { en: None, zh: Some(text) })
                }
                serde_json::Value::Object(map) => {
                    let mut out = LocalizedText::default();
                    for (key, value) in map {
                        if let Some(text) = value.as_str() {
                            match key.as_str() {
                                "en" => out.en = Some(text.to_owned()),
                                "zh" => out.zh = Some(text.to_owned()),
                                _ => {}
                            }
                        }
                    }
                    if out.is_empty() { None } else { Some(out) }
                }
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    })
}

/// CodeBuddy/WorkBuddy `plugin.json` declared subset (02 §4).
///
/// Wire keys are camelCase (`teamInfo`, `userConfig`, `mcpServers`,
/// `lspServers`, `defaultEnabled`); the struct uses snake_case internally.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Display metadata; real markets use both `"author": "name"` and
    /// `"author": {"name": "...", "email": "..."}` — normalized to the human
    /// name (email appended when present).
    #[serde(default, deserialize_with = "deserialize_author")]
    pub author: Option<String>,
    /// One-shot component roots; real markets declare both a single string
    /// (`"./agents/"`) and an array (`["./agents/a.md"]`).
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub agents: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub skills: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub commands: Vec<String>,
    #[serde(default)]
    pub hooks: Option<serde_json::Value>,
    #[serde(default)]
    pub mcp_servers: Option<serde_json::Value>,
    #[serde(default)]
    pub lsp_servers: Option<serde_json::Value>,
    #[serde(default)]
    pub user_config: Option<serde_json::Value>,
    /// Declared dependencies, normalized to a flat array of entries.
    ///
    /// Real CodeBuddy markets use both shapes: a legacy array
    /// (`[{"name": "x"}]`) and an object grouped by kind
    /// (`{"connectors": ["westock-mcp"]}`). The object form is expanded
    /// into one entry per item with a `group` marker (02 §8).
    #[serde(default, deserialize_with = "deserialize_dependencies")]
    pub dependencies: Vec<serde_json::Value>,
    #[serde(default)]
    pub team_info: Option<TeamInfo>,
    /// Localized display metadata (real WorkBuddy experts carry all of these;
    /// none of them affect the runtime, so all stay optional).
    #[serde(default, deserialize_with = "deserialize_localized_text")]
    pub display_name: Option<LocalizedText>,
    #[serde(default, deserialize_with = "deserialize_localized_text")]
    pub profession: Option<LocalizedText>,
    #[serde(default, deserialize_with = "deserialize_localized_text")]
    pub display_description: Option<LocalizedText>,
    #[serde(default)]
    pub default_init_prompt: Option<LocalizedText>,
    #[serde(default, deserialize_with = "deserialize_localized_list")]
    pub quick_prompts: Vec<LocalizedText>,
    #[serde(default, deserialize_with = "deserialize_localized_list")]
    pub tags: Vec<LocalizedText>,
    /// Relative path to the avatar asset (e.g. `avatars/expert.png`). The
    /// asset itself is already copied into the immutable snapshot.
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub expert_type: Option<String>,
    #[serde(default)]
    pub category_id: Option<String>,
    #[serde(default)]
    pub agent_name: Option<String>,
}

impl PluginManifest {
    /// Resolved version; `plugin.json` may omit it (02 §4: `version` 参与快照
    /// 版本). Defaults to `1.0.0` so identity stays stable.
    pub fn version(&self) -> &str {
        self.version.as_deref().unwrap_or("1.0.0")
    }
}

/// Market manifests (`.codebuddy-skill/marketplace.json` /
/// `.codebuddy-connector/connectors.json`): identity + optional entry lists.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketManifest {
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub skills: Vec<serde_json::Value>,
    #[serde(default)]
    pub connectors: Vec<serde_json::Value>,
}

impl MarketManifest {
    pub fn version(&self) -> &str {
        self.version.as_deref().unwrap_or("1.0.0")
    }
}

/// A single CLI connector directory (`cli.json` + `skills/`), e.g. wecom /
/// feishu / tmeet in `~/.workbuddy/connectors-marketplace/connectors/`.
///
/// `cli.json` carries no identity of its own; the sibling `connectors.json`
/// index row (`id`, `name`, `description`, `examples_*`, `minWorkbuddyVersion`)
/// supplies metadata. V1 uses the directory name as the plugin id and derives
/// a display name from the directory name when no index row is available.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliManifest {
    /// E.g. `{ "type": "node", "version": ">=18" }`.
    #[serde(default)]
    pub runtime: Option<serde_json::Value>,
    #[serde(default)]
    pub init: Option<serde_json::Value>,
    #[serde(default)]
    pub version_check: Option<serde_json::Value>,
    #[serde(default)]
    pub auth: Option<serde_json::Value>,
    #[serde(default)]
    pub un_auth: Option<serde_json::Value>,
    #[serde(default)]
    pub status: Option<serde_json::Value>,
    #[serde(default)]
    pub status_match: Option<serde_json::Value>,
    #[serde(default)]
    pub status_match_json: Option<serde_json::Value>,
    #[serde(default)]
    pub auth_url_domain: Option<String>,
    #[serde(default)]
    pub auth_wait_for_exit: Option<bool>,
    #[serde(default)]
    pub auth_qr_modal: Option<bool>,
}

/// Flatten a per-platform map (`{"win32": "...", "darwin": "..."}`) to its
/// first resolved value for a summary string.
pub fn platform_summary(value: &Option<serde_json::Value>) -> Option<String> {
    let value = value.as_ref()?;
    match value {
        serde_json::Value::String(platform) => Some(platform.clone()),
        serde_json::Value::Object(map) => Some(
            map.values()
                .filter_map(|value| value.as_str())
                .find(|s| !s.is_empty())
                .map(str::to_owned)?,
        ),
        _ => None,
    }
}

/// Normalize `dependencies` into a flat array for the V1 component model.
///
/// Accepts either a JSON array of entry objects (legacy shape) or an
/// object keyed by kind (`connectors` / `plugins` / `skills` …) whose
/// values are lists of names or entry objects. Each expanded item gains a
/// `group` field capturing the original kind when it came from the object
/// form, so consumers can still distinguish `westock-mcp` (a connector)
/// from a plugin dependency.
fn deserialize_dependencies<'de, D>(deserializer: D) -> Result<Vec<serde_json::Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Array(items) => items,
        serde_json::Value::Object(map) => {
            let mut out: Vec<serde_json::Value> = Vec::new();
            for (group, entries) in map {
                for entry in entries.as_array().cloned().unwrap_or_default() {
                    let mut item = match entry {
                        serde_json::Value::String(name) => json!({ "name": name }),
                        serde_json::Value::Object(object) => serde_json::Value::Object(object),
                        other => other,
                    };
                    if let Some(object) = item.as_object_mut() {
                        object.insert("group".into(), json!(group));
                    }
                    out.push(item);
                }
            }
            out
        }
        _ => Vec::new(),
    })
}

/// `author`, `displayName`, `profession`, … fields that real markets emit
/// both as a plain string and as an object (`{"name": "...", "email": "..."}`).
fn deserialize_author<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::String(name) => Some(name),
        serde_json::Value::Object(map) => {
            let name = map.get("name").and_then(|value| value.as_str()).unwrap_or("");
            let email = map.get("email").and_then(|value| value.as_str()).filter(|e| !e.is_empty());
            let joined = match email {
                Some(email) => format!("{name} <{email}>"),
                None => name.to_owned(),
            };
            Some(joined)
        }
        _ => None,
    })
}

/// Component root declarations: `"./agents/"` (single string) or
/// `["./agents/a.md", "./agents/b.md"]` (array).
fn deserialize_string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::String(item) => vec![item],
        serde_json::Value::Array(items) => items
            .into_iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    })
}

/// Parsed manifest for any supported source kind.
#[derive(Debug, Clone)]
pub enum ParsedManifest {
    Plugin(PluginManifest),
    Market(MarketManifest),
    /// Single CLI connector directory identity (name derived from directory).
    Cli(CliManifest, String),
    /// Single skill directory (`skills/<slug>/` without a marketplace.json);
    /// identity is the directory name.
    SingleSkill(String),
}

impl ParsedManifest {
    pub fn name(&self) -> &str {
        match self {
            Self::Plugin(manifest) => &manifest.name,
            Self::Market(manifest) => &manifest.name,
            Self::Cli(_, directory_name) => directory_name,
            Self::SingleSkill(directory_name) => directory_name,
        }
    }

    pub fn version(&self) -> &str {
        match self {
            Self::Plugin(manifest) => manifest.version(),
            Self::Market(manifest) => manifest.version(),
            // CLI connectors do not declare a version in cli.json; V1 uses a
            // stable placeholder so identity stays honest (ads 02 §4 defaults).
            Self::Cli(_, _) => "1.0.0",
            Self::SingleSkill(_) => "1.0.0",
        }
    }
}

/// Validate a manifest-referenced relative path (02 §4 / §7):
/// - must be relative and start with `./` (or be `"."`);
/// - no `..` component, no absolute path, no Windows drive prefix.
pub fn validate_relative_path(path: &str) -> Result<String, ManifestError> {
    if path.is_empty() {
        return Err(ManifestError::UnsafePath("empty path".into()));
    }
    // Absolute: POSIX root, Windows drive prefix or UNC.
    if path.starts_with('/')
        || path.starts_with('\\')
        || (path.len() >= 2 && path.as_bytes()[1] == b':')
    {
        return Err(ManifestError::UnsafePath(format!(
            "absolute path is not allowed: {path}"
        )));
    }
    let mut components = path.split('/');
    match components.next() {
        Some(".") | Some("") => {}
        _ => {
            return Err(ManifestError::UnsafePath(format!(
                "manifest paths must start with './': {path}"
            )))
        }
    }
    if components.any(|component| component == "..") {
        return Err(ManifestError::UnsafePath(format!(
            "path traversal is not allowed: {path}"
        )));
    }
    // Normalize `./x` → `x`.
    Ok(if path == "." {
        String::new()
    } else {
        path.strip_prefix("./").unwrap_or(path).to_owned()
    })
}

/// Read and parse the manifest for a source kind. Missing non-plugin
/// component files degrade per-component; a missing *identity manifest* is a
/// blocked import (02 §11.1).
pub fn parse_manifest(
    root: &Path,
    source_kind: SourceKind,
) -> Result<ParsedManifest, ManifestError> {
    let rel = source_kind.manifest_rel_path();
    let path = root.join(rel);
    match source_kind {
        SourceKind::WorkBuddySkillMarket if !path.is_file() => {
            // Single skill directory (`skills/<slug>/`): the identity manifest
            // is absent by design; accept the root when it contains a
            // SKILL.md directly. Identity = directory name (02 §4).
            let directory_name = root
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            if directory_name.trim().is_empty() {
                return Err(ManifestError::MissingIdentity("skill directory name".into()));
            }
            if !root.join("SKILL.md").is_file() {
                return Err(ManifestError::Io(format!(
                    "{}: expected marketplace.json or SKILL.md",
                    path.display()
                )));
            }
            Ok(ParsedManifest::SingleSkill(directory_name))
        }
        _ => {
            let text = std::fs::read_to_string(&path).map_err(|error| {
                ManifestError::Io(format!("{}: {error}", path.display()))
            })?;
            match source_kind {
                SourceKind::CodeBuddyPlugin => {
                    parse_plugin_manifest(&text).map(ParsedManifest::Plugin)
                }
                SourceKind::WorkBuddyCliConnector => {
                    let directory_name = root
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if directory_name.trim().is_empty() {
                        return Err(ManifestError::MissingIdentity("connector directory name".into()));
                    }
                    parse_cli_manifest(&text).map(|manifest| {
                        ParsedManifest::Cli(manifest, directory_name)
                    })
                }
                _ => parse_market_manifest(&text).map(ParsedManifest::Market),
            }
        }
    }
}

pub fn parse_cli_manifest(text: &str) -> Result<CliManifest, ManifestError> {
    let manifest: CliManifest =
        serde_json::from_str(text).map_err(|error| ManifestError::Json(error.to_string()))?;
    Ok(manifest)
}

pub fn parse_market_manifest(text: &str) -> Result<MarketManifest, ManifestError> {
    let manifest: MarketManifest =
        serde_json::from_str(text).map_err(|error| ManifestError::Json(error.to_string()))?;
    if manifest.name.trim().is_empty() {
        return Err(ManifestError::MissingIdentity("name".into()));
    }
    Ok(manifest)
}

pub fn parse_plugin_manifest(text: &str) -> Result<PluginManifest, ManifestError> {
    let manifest: PluginManifest =
        serde_json::from_str(text).map_err(|error| ManifestError::Json(error.to_string()))?;
    let name = manifest.name.trim();
    if name.is_empty() {
        return Err(ManifestError::MissingIdentity("name".into()));
    }
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_manifest_paths_normalize() {
        assert_eq!(validate_relative_path("./agents").unwrap(), "agents");
        assert_eq!(validate_relative_path("./skills/hello").unwrap(), "skills/hello");
        assert_eq!(validate_relative_path(".").unwrap(), "");
    }

    #[test]
    fn absolute_and_traversal_paths_are_rejected() {
        assert!(validate_relative_path("/etc/passwd").is_err());
        assert!(validate_relative_path("C:\\evil").is_err());
        assert!(validate_relative_path("../shared").is_err());
        assert!(validate_relative_path("./agents/../../etc").is_err());
        assert!(validate_relative_path("agents").is_err(), "must start with ./");
    }

    #[test]
    fn plugin_manifest_identity_required() {
        let ok = parse_plugin_manifest(r#"{"name":"demo","agents":["./agents"]}"#).unwrap();
        assert_eq!(ok.name, "demo");
        assert_eq!(ok.version(), "1.0.0", "missing version defaults");
        let missing = parse_plugin_manifest(r#"{"version":"1.0.0"}"#);
        assert!(matches!(missing, Err(ManifestError::Json(_))), "serde rejects a missing name");
        let blank = parse_plugin_manifest(r#"{"name":"  ","version":"1.0.0"}"#);
        assert!(matches!(blank, Err(ManifestError::MissingIdentity(_))));
        let broken = parse_plugin_manifest("not json");
        assert!(matches!(broken, Err(ManifestError::Json(_))));
    }

    #[test]
    fn market_manifest_parses_entries_per_kind() {
        let skill_market =
            parse_market_manifest(r#"{"name":"ms","skills":[{"name":"a"}]}"#).unwrap();
        assert_eq!(skill_market.name, "ms");
        assert_eq!(skill_market.skills.len(), 1);
        let connector_market =
            parse_market_manifest(r#"{"name":"mc","connectors":[{"name":"c"}]}"#).unwrap();
        assert_eq!(connector_market.connectors.len(), 1);
        assert!(parse_market_manifest(r#"{"version":"1"}"#).is_err());
    }

    #[test]
    fn team_info_uses_camel_case_wire_fields() {
        let manifest = parse_plugin_manifest(
            r#"{"name":"t","teamInfo":{"leadAgent":"lead","memberAgents":["a","b"],"expertType":"team"}}"#,
        )
        .unwrap();
        let team = manifest.team_info.unwrap();
        assert_eq!(team.lead_agent, "lead");
        assert_eq!(team.member_agents, vec!["a", "b"]);
        assert_eq!(team.expert_type.as_deref(), Some("team"));
    }
}
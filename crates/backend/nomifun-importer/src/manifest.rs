//! Manifest parsing and path validation (docs/agent-store/02 §2 steps 1–4).
//!
//! V1 supports three local source kinds. A marketplace manifest is a much
//! lighter shape than a plugin manifest; both converge on the same
//! `PluginManifest`-like identity fields.

use std::path::Path;

use serde::Deserialize;

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
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub agents: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub commands: Vec<String>,
    #[serde(default)]
    pub hooks: Option<serde_json::Value>,
    #[serde(default)]
    pub mcp_servers: Option<serde_json::Value>,
    #[serde(default)]
    pub lsp_servers: Option<serde_json::Value>,
    #[serde(default)]
    pub user_config: Option<serde_json::Value>,
    #[serde(default)]
    pub dependencies: Vec<serde_json::Value>,
    #[serde(default)]
    pub team_info: Option<TeamInfo>,
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

/// Parsed manifest for any supported source kind.
#[derive(Debug, Clone)]
pub enum ParsedManifest {
    Plugin(PluginManifest),
    Market(MarketManifest),
}

impl ParsedManifest {
    pub fn name(&self) -> &str {
        match self {
            Self::Plugin(manifest) => &manifest.name,
            Self::Market(manifest) => &manifest.name,
        }
    }

    pub fn version(&self) -> &str {
        match self {
            Self::Plugin(manifest) => manifest.version(),
            Self::Market(manifest) => manifest.version(),
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
    let text = std::fs::read_to_string(&path).map_err(|error| {
        ManifestError::Io(format!("{}: {error}", path.display()))
    })?;
    match source_kind {
        SourceKind::CodeBuddyPlugin => parse_plugin_manifest(&text).map(ParsedManifest::Plugin),
        _ => parse_market_manifest(&text).map(ParsedManifest::Market),
    }
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
use nomifun_common::ConversationId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// A. Skill catalog, list & info
// ---------------------------------------------------------------------------

/// User-facing origin for a catalogued Skill.
///
/// This is deliberately separate from [`SkillSourceResponse`], whose
/// `custom` spelling is retained for the legacy Skills Hub API. New catalog
/// consumers use the product-facing scopes below and identify a Skill through
/// [`SkillId`] rather than its display name.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SkillCatalogSource {
    Builtin,
    User,
    Project,
    Extension,
    Mcp,
    Legacy,
}

impl SkillCatalogSource {
    /// Stable namespace segment used by [`SkillId`].
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::User => "user",
            Self::Project => "project",
            Self::Extension => "extension",
            Self::Mcp => "mcp",
            Self::Legacy => "legacy",
        }
    }
}

/// Opaque, source-qualified identifier for one discoverable Skill.
///
/// The canonical form is `<source>:<local_key>` when the source has no owner,
/// or `<source>:<source_key>:<local_key>` when it does. Source-key and
/// local-key components use percent encoding, so a future extension or MCP
/// server name cannot make an identifier ambiguous.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct SkillId(String);

impl SkillId {
    pub fn new(source: SkillCatalogSource, source_key: Option<&str>, local_key: &str) -> Self {
        let mut value = source.as_str().to_owned();
        if let Some(source_key) = source_key.filter(|key| !key.is_empty()) {
            value.push(':');
            value.push_str(&encode_skill_id_component(source_key));
        }
        value.push(':');
        value.push_str(&encode_skill_id_component(local_key));
        Self(value)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Catalog owner encoded in this canonical identity.
    pub fn source(&self) -> SkillCatalogSource {
        let source = self
            .0
            .split(':')
            .next()
            .expect("a SkillId always contains a source segment");
        parse_skill_catalog_source(source)
            .expect("a SkillId is constructed or parsed from a known source")
    }

    /// Parse an already-qualified catalog identity and reject non-canonical
    /// encodings. Display names are deliberately not accepted here: callers
    /// that need to preserve a legacy name must use [`Self::legacy`].
    pub fn parse(value: &str) -> Result<Self, String> {
        let segments = value.split(':').collect::<Vec<_>>();
        let (source, source_key, local_key) = match segments.as_slice() {
            [source, local_key] => (parse_skill_catalog_source(source)?, None, *local_key),
            [source, source_key, local_key] => {
                (parse_skill_catalog_source(source)?, Some(*source_key), *local_key)
            }
            _ => {
                return Err(
                    "skill_id must be '<source>:<local_key>' or '<source>:<source_key>:<local_key>'"
                        .to_owned(),
                );
            }
        };

        let source_key = source_key
            .map(decode_skill_id_component)
            .transpose()?;
        let local_key = decode_skill_id_component(local_key)?;
        let parsed = Self::new(source, source_key.as_deref(), &local_key);
        if parsed.as_str() != value {
            return Err("skill_id must use canonical percent encoding".to_owned());
        }
        Ok(parsed)
    }

    /// Preserve an old, name-only binding without guessing whether it referred
    /// to the builtin or user catalog entry of the same name.
    pub fn legacy(name: &str) -> Self {
        Self::new(SkillCatalogSource::Legacy, None, name)
    }

    /// Recover the opaque name carried by a legacy binding. Canonical catalog
    /// identities deliberately return `None`: they must be loaded by their
    /// source-qualified identity rather than routed through name precedence.
    pub fn legacy_name(&self) -> Option<String> {
        let mut segments = self.0.split(':');
        let source = segments.next()?;
        let local_key = segments.next()?;
        if source != SkillCatalogSource::Legacy.as_str() || segments.next().is_some() {
            return None;
        }
        decode_skill_id_component(local_key).ok()
    }
}

fn parse_skill_catalog_source(value: &str) -> Result<SkillCatalogSource, String> {
    match value {
        "builtin" => Ok(SkillCatalogSource::Builtin),
        "user" => Ok(SkillCatalogSource::User),
        "project" => Ok(SkillCatalogSource::Project),
        "extension" => Ok(SkillCatalogSource::Extension),
        "mcp" => Ok(SkillCatalogSource::Mcp),
        "legacy" => Ok(SkillCatalogSource::Legacy),
        _ => Err(format!("unknown skill catalog source '{value}'")),
    }
}

fn encode_skill_id_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
            encoded.push(byte as char);
        } else {
            use std::fmt::Write;
            write!(encoded, "%{byte:02X}").expect("writing to String cannot fail");
        }
    }
    encoded
}

fn decode_skill_id_component(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                if index + 2 >= bytes.len() {
                    return Err("truncated percent escape in skill_id".to_owned());
                }
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3])
                    .map_err(|_| "invalid percent escape in skill_id".to_owned())?;
                let byte = u8::from_str_radix(hex, 16)
                    .map_err(|_| "invalid percent escape in skill_id".to_owned())?;
                decoded.push(byte);
                index += 3;
            }
            byte if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') => {
                decoded.push(byte);
                index += 1;
            }
            _ => return Err("skill_id contains an unescaped component character".to_owned()),
        }
    }
    String::from_utf8(decoded).map_err(|_| "skill_id component is not valid UTF-8".to_owned())
}

/// One discoverable, user-facing Skill (`GET /api/skills/catalog`).
///
/// Catalog discovery contains only lightweight metadata. It intentionally
/// omits filesystem locations and `SKILL.md` bodies; those are loaded through
/// the later, policy-aware loading flow.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SkillCatalogItemResponse {
    pub skill_id: SkillId,
    pub name: String,
    pub description: String,
    pub source: SkillCatalogSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_key: Option<String>,
}

/// Response payload for the user-facing Skill catalog.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SkillCatalogResponse {
    pub skills: Vec<SkillCatalogItemResponse>,
}

/// Origin of a listed skill — `builtin`, `custom`, or `extension`.
///
/// Matches the renderer contract in
/// `src/common/adapter/ipcBridge.ts::listAvailableSkills`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SkillSourceResponse {
    Builtin,
    Custom,
    Extension,
}

/// Single item in the available skills list (`GET /api/skills`).
///
/// For `source=builtin` entries, `location` is a synthesized absolute path
/// under `{data_dir}/builtin-skills-view/{name}/SKILL.md` (lazily
/// materialized from the embedded corpus so the export-symlink flow can
/// resolve it), and `relative_location` carries the path the frontend
/// passes back into `POST /api/skills/builtin-skill` (e.g.
/// `"auto-inject/cron/SKILL.md"` or `"{name}/SKILL.md"`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillListItemResponse {
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub name_i18n: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub description_i18n: HashMap<String, String>,
    pub location: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative_location: Option<String>,
    pub is_custom: bool,
    pub source: SkillSourceResponse,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audience_tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scenario_tags: Vec<String>,
}

/// Request body for `PUT /api/skills/{name}/tags`.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SetSkillTagsRequest {
    #[serde(default)]
    pub audience_tags: Vec<String>,
    #[serde(default)]
    pub scenario_tags: Vec<String>,
}

/// An auto-injected built-in skill (`GET /api/skills/builtin-auto`).
///
/// `location` is the relative path the frontend passes back into
/// `POST /api/skills/builtin-skill` (e.g. `"auto-inject/cron/SKILL.md"`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BuiltinAutoSkillResponse {
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub name_i18n: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub description_i18n: HashMap<String, String>,
    pub location: String,
}

/// Request body for `POST /api/skills/info`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadSkillInfoRequest {
    pub skill_path: String,
}

/// Response for `POST /api/skills/info`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReadSkillInfoResponse {
    pub name: String,
    pub description: String,
}

// ---------------------------------------------------------------------------
// B. Skill import / export / delete
// ---------------------------------------------------------------------------

/// Request body for `POST /api/skills/import` and `POST /api/skills/import-symlink`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportSkillRequest {
    pub skill_path: String,
}

/// Response for skill import operations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImportSkillResponse {
    pub skill_name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skill_names: Vec<String>,
    /// Canonical catalog identities for `skill_names`, in the same order.
    /// This lets callers retain the imported source identity without trying to
    /// reconstruct it from a display name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skill_ids: Vec<String>,
}

/// Request body for `POST /api/skills/export-symlink`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportSkillRequest {
    pub skill_path: String,
    pub target_dir: String,
}

// ---------------------------------------------------------------------------
// C. Skill scanning & discovery
// ---------------------------------------------------------------------------

/// Request body for `POST /api/skills/scan`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScanForSkillsRequest {
    pub folder_path: String,
}

/// A skill discovered by directory scanning.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScannedSkillResponse {
    pub name: String,
    pub description: String,
    pub path: String,
}

/// Response for `POST /api/skills/scan`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScanForSkillsResponse {
    pub skills: Vec<ScannedSkillResponse>,
}

/// An external skill source with count (`GET /api/skills/detect-external`).
///
/// `source` is a stable slug identifying the origin (e.g. `"claude"`,
/// `"gemini"`, `"agents"`, or `"custom-<abs-path>"` for user-added paths).
/// The renderer uses it as a React key and `data-testid` suffix in
/// `SkillsHubSettings.tsx`, so it must be unique across the returned list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExternalSkillSourceResponse {
    pub name: String,
    pub path: String,
    pub source: String,
    pub skill_count: usize,
    pub skills: Vec<ScannedSkillResponse>,
}

/// A named filesystem path (`GET /api/skills/detect-paths`, external paths).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NamedPathResponse {
    pub name: String,
    pub path: String,
}

/// Response for `GET /api/skills/paths`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillPathsResponse {
    pub user_skills_dir: String,
    pub builtin_skills_dir: String,
}

// ---------------------------------------------------------------------------
// D. Preset rules & skills
// ---------------------------------------------------------------------------

/// Request body for `POST /api/skills/preset-rule/read` and
/// `POST /api/skills/preset-skill/read`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadPresetRuleRequest {
    #[serde(deserialize_with = "crate::serde_util::deserialize_preset_reference")]
    pub preset_id: String,
    #[serde(default)]
    pub locale: Option<String>,
}

/// Request body for `POST /api/skills/preset-rule/write` and
/// `POST /api/skills/preset-skill/write`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WritePresetRuleRequest {
    #[serde(deserialize_with = "crate::serde_util::deserialize_preset_reference")]
    pub preset_id: String,
    pub content: String,
    #[serde(default)]
    pub locale: Option<String>,
}

/// Request body for `POST /api/skills/builtin-rule` and
/// `POST /api/skills/builtin-skill`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadBuiltinResourceRequest {
    pub file_name: String,
}

/// Request body for `POST /api/skills/materialize-for-agent`.
///
/// Callers pass the resolved skill snapshot (see
/// `conversation.extra.skills`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterializeSkillsRequest {
    pub conversation_id: ConversationId,
    #[serde(default)]
    pub skills: Vec<String>,
}

/// One entry in the `MaterializeSkillsResponse::skills` list.
///
/// Each entry tells the frontend the absolute on-disk directory of a
/// resolved skill. The frontend is expected to symlink that directory
/// into the agent CLI's native skills dir — the backend no longer
/// copies files per-conversation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MaterializedSkillRef {
    pub name: String,
    /// Absolute path on disk to the skill's source directory. May live
    /// under `{data_dir}/builtin-skills/` (top-level or `auto-inject/`)
    /// or `{data_dir}/skills/` (user-created skills).
    pub source_path: String,
}

/// Response for `POST /api/skills/materialize-for-agent`.
///
/// Returns a list of resolved skill references rather than a copied
/// directory; the frontend symlinks each `source_path` into the CLI's
/// native skills dir. Unknown names from the request are silently
/// omitted from the list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MaterializeSkillsResponse {
    pub skills: Vec<MaterializedSkillRef>,
}

// ---------------------------------------------------------------------------
// E. External path management
// ---------------------------------------------------------------------------

/// Request body for `POST /api/skills/external-paths`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddExternalPathRequest {
    pub name: String,
    pub path: String,
}

/// Request body for `DELETE /api/skills/external-paths`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoveExternalPathRequest {
    pub path: String,
}

// ---------------------------------------------------------------------------
// F. Skill market ranking
// ---------------------------------------------------------------------------

/// Request body for `POST /api/skills/market/rankings/sync`.
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SkillMarketSyncRequest {
    /// Optional source allow-list. Empty means all supported sources.
    #[serde(default)]
    pub sources: Vec<String>,
}

/// Single entry scraped from a skill market ranking.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillMarketItemResponse {
    /// Stable source-local id, e.g. `clawhub:owner/skill`.
    pub id: String,
    /// Source slug, e.g. `clawhub`, `loophub`, `skillhub_mcp`, or `mcpworld`.
    pub source: String,
    pub rank: usize,
    pub name: String,
    pub description: String,
    pub url: String,
    /// Command shown to the user/Nomi. The backend only returns text; it never executes it.
    pub install_command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audience_tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scenario_tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<String>,
}

/// Response for `POST /api/skills/market/rankings/sync`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillMarketSyncResponse {
    pub fetched_at: i64,
    pub items: Vec<SkillMarketItemResponse>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

/// Request body for resolving a market MCP entry into importable MCP JSON.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SkillMarketMcpConfigRequest {
    pub source: String,
    pub id: String,
    pub url: String,
}

/// Response for a resolved market MCP config.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillMarketMcpConfigResponse {
    pub config_json: serde_json::Value,
}

/// Request body for resolving a SkillHub expert package entry.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SkillMarketPackageRequest {
    pub source: String,
    pub id: String,
    pub url: String,
}

/// Response for a resolved expert package that can be imported as a user preset.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillMarketPackageResponse {
    pub name: String,
    pub description: String,
    pub instructions: String,
    #[serde(default)]
    pub skill_slugs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
}

/// One failed child skill install while importing a SkillHub expert package.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillMarketPackageInstallError {
    pub skill_slug: String,
    pub error: String,
}

/// Response for installing the skills behind a SkillHub expert package.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillMarketPackageInstallResponse {
    pub package: SkillMarketPackageResponse,
    #[serde(default)]
    pub installed_skill_names: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<SkillMarketPackageInstallError>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn catalog_skill_id_qualifies_and_escapes_source_components() {
        let id = SkillId::new(
            SkillCatalogSource::Extension,
            Some("office:plugin"),
            "pdf/export",
        );

        assert_eq!(id.as_str(), "extension:office%3Aplugin:pdf%2Fexport");
        assert_eq!(id.source(), SkillCatalogSource::Extension);
        assert_eq!(serde_json::to_value(id).unwrap(), json!("extension:office%3Aplugin:pdf%2Fexport"));
    }

    #[test]
    fn legacy_skill_id_decodes_only_name_based_bindings() {
        let legacy = SkillId::legacy("review notes");
        assert_eq!(legacy.as_str(), "legacy:review%20notes");
        assert_eq!(legacy.legacy_name().as_deref(), Some("review notes"));
        assert_eq!(
            SkillId::parse("builtin:review%20notes")
                .unwrap()
                .legacy_name(),
            None,
        );
    }

    // -- Skill list --

    #[test]
    fn test_skill_list_item_serde() {
        let item = SkillListItemResponse {
            name: "my-skill".into(),
            description: "Does things".into(),
            name_i18n: HashMap::new(),
            description_i18n: HashMap::new(),
            location: "/home/user/.nomifun/skills/my-skill".into(),
            relative_location: None,
            is_custom: true,
            source: SkillSourceResponse::Custom,
            audience_tags: vec![],
            scenario_tags: vec![],
        };
        let json = serde_json::to_value(&item).unwrap();
        assert_eq!(json["name"], "my-skill");
        // Project-wide wire contract: field names are snake_case.
        assert_eq!(json["is_custom"], true);
        assert!(json.get("isCustom").is_none());
        assert_eq!(json["source"], "custom");
        // Absent for custom source — Option<String>::None is skipped.
        assert!(json.get("relative_location").is_none());
        assert!(json.get("relativeLocation").is_none());
    }

    #[test]
    fn test_skill_list_item_builtin_with_relative_location() {
        let item = SkillListItemResponse {
            name: "cron".into(),
            description: "Schedule recurring tasks".into(),
            name_i18n: HashMap::new(),
            description_i18n: HashMap::new(),
            location: "/home/user/.nomifun/builtin-skills-view/cron/SKILL.md".into(),
            relative_location: Some("auto-inject/cron/SKILL.md".into()),
            is_custom: false,
            source: SkillSourceResponse::Builtin,
            audience_tags: vec![],
            scenario_tags: vec![],
        };
        let json = serde_json::to_value(&item).unwrap();
        // Project-wide wire contract: relative_location stays snake_case.
        assert_eq!(json["relative_location"], "auto-inject/cron/SKILL.md");
        assert!(json.get("relativeLocation").is_none());
        assert_eq!(json["source"], "builtin");
    }

    #[test]
    fn test_skill_list_item_deserializes_snake_case() {
        // Frontend wire format → backend deserialization round-trip.
        let raw = json!({
            "name": "cron",
            "description": "Schedule",
            "location": "/tmp/view/cron/SKILL.md",
            "relative_location": "auto-inject/cron/SKILL.md",
            "is_custom": false,
            "source": "builtin",
        });
        let item: SkillListItemResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(item.name, "cron");
        assert!(!item.is_custom);
        assert_eq!(item.relative_location.as_deref(), Some("auto-inject/cron/SKILL.md"));
    }

    #[test]
    fn test_skill_tags_default_and_skip_empty() {
        let item = SkillListItemResponse {
            name: "x".into(),
            description: "d".into(),
            name_i18n: HashMap::new(),
            description_i18n: HashMap::new(),
            location: "/l".into(),
            relative_location: None,
            is_custom: true,
            source: SkillSourceResponse::Custom,
            audience_tags: vec![],
            scenario_tags: vec!["document".into()],
        };
        let j = serde_json::to_value(&item).unwrap();
        assert!(j.get("audience_tags").is_none()); // empty skipped
        assert_eq!(j["scenario_tags"], serde_json::json!(["document"]));
    }

    #[test]
    fn test_materialize_request_roundtrip() {
        let conversation_id = "0190f5fe-7c00-7a00-8abc-012345678901";
        let raw = json!({
            "conversation_id": conversation_id,
            "skills": ["planning-with-files", "pdf"],
        });
        let req: MaterializeSkillsRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(req.conversation_id.as_str(), conversation_id);
        assert_eq!(req.skills, vec!["planning-with-files", "pdf"]);
    }

    #[test]
    fn test_materialize_request_rejects_unknown_retired_field() {
        let raw = json!({
            "conversation_id": "0190f5fe-7c00-7a00-8abc-012345678901",
            "enabled_skills": ["pdf"],
        });
        assert!(serde_json::from_value::<MaterializeSkillsRequest>(raw).is_err());
    }

    #[test]
    fn test_materialize_request_rejects_numeric_conversation_id() {
        let raw = json!({
            "conversation_id": 42,
            "skills": [],
        });
        assert!(serde_json::from_value::<MaterializeSkillsRequest>(raw).is_err());
    }

    #[test]
    fn test_materialize_request_default_enabled() {
        let raw = json!({"conversation_id": "0190f5fe-7c00-7a00-8abc-012345678901"});
        let req: MaterializeSkillsRequest = serde_json::from_value(raw).unwrap();
        assert!(req.skills.is_empty());
    }

    #[test]
    fn test_materialize_response_serializes_snake() {
        let resp = MaterializeSkillsResponse {
            skills: vec![
                MaterializedSkillRef {
                    name: "cron".into(),
                    source_path: "/tmp/builtin-skills/auto-inject/cron".into(),
                },
                MaterializedSkillRef {
                    name: "planning-with-files".into(),
                    source_path: "/tmp/builtin-skills/planning-with-files".into(),
                },
            ],
        };
        let json = serde_json::to_value(&resp).unwrap();
        let skills = json["skills"].as_array().unwrap();
        assert_eq!(skills.len(), 2);
        // Project-wide wire contract: snake_case fields on the wire.
        assert_eq!(skills[0]["name"], "cron");
        assert_eq!(skills[0]["source_path"], "/tmp/builtin-skills/auto-inject/cron");
        assert!(skills[0].get("sourcePath").is_none());
    }

    #[test]
    fn test_materialize_response_roundtrip() {
        let raw = json!({
            "skills": [
                {"name": "cron", "source_path": "/tmp/builtin-skills/auto-inject/cron"}
            ]
        });
        let resp: MaterializeSkillsResponse = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(resp.skills.len(), 1);
        assert_eq!(resp.skills[0].name, "cron");
        assert_eq!(resp.skills[0].source_path, "/tmp/builtin-skills/auto-inject/cron");
        assert_eq!(serde_json::to_value(&resp).unwrap(), raw);
    }

    #[test]
    fn test_skill_source_serializes_lowercase() {
        assert_eq!(
            serde_json::to_value(SkillSourceResponse::Builtin).unwrap(),
            serde_json::json!("builtin")
        );
        assert_eq!(
            serde_json::to_value(SkillSourceResponse::Custom).unwrap(),
            serde_json::json!("custom")
        );
        assert_eq!(
            serde_json::to_value(SkillSourceResponse::Extension).unwrap(),
            serde_json::json!("extension")
        );
    }

    #[test]
    fn test_read_skill_info_request() {
        // Project-wide wire contract: skill_path on the wire.
        let raw = json!({"skill_path": "/path/to/skill"});
        let req: ReadSkillInfoRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(req.skill_path, "/path/to/skill");
        // Legacy camelCase must now fail.
        let legacy = json!({"skillPath": "/path/to/skill"});
        assert!(serde_json::from_value::<ReadSkillInfoRequest>(legacy).is_err());
    }

    #[test]
    fn test_read_skill_info_response() {
        let resp = ReadSkillInfoResponse {
            name: "test".into(),
            description: "A test skill".into(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["name"], "test");
        assert_eq!(json["description"], "A test skill");
    }

    // -- Import / Export --

    #[test]
    fn test_import_skill_request() {
        let raw = json!({"skill_path": "/external/skill"});
        let req: ImportSkillRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(req.skill_path, "/external/skill");
    }

    #[test]
    fn test_import_skill_response() {
        let resp = ImportSkillResponse {
            skill_name: "imported-skill".into(),
            skill_names: vec!["imported-skill".into(), "second-skill".into()],
            skill_ids: vec!["user:imported-skill".into(), "user:second-skill".into()],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["skill_name"], "imported-skill");
        assert_eq!(json["skill_names"], json!(["imported-skill", "second-skill"]));
        assert_eq!(json["skill_ids"], json!(["user:imported-skill", "user:second-skill"]));
        assert!(json.get("skillName").is_none());
    }

    #[test]
    fn test_export_skill_request() {
        let raw = json!({"skill_path": "/user/skill", "target_dir": "/external/dir"});
        let req: ExportSkillRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(req.skill_path, "/user/skill");
        assert_eq!(req.target_dir, "/external/dir");
    }

    // -- Scanning --

    #[test]
    fn test_scan_for_skills_request() {
        let raw = json!({"folder_path": "/some/dir"});
        let req: ScanForSkillsRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(req.folder_path, "/some/dir");
    }

    #[test]
    fn test_scanned_skill_response() {
        let skill = ScannedSkillResponse {
            name: "found-skill".into(),
            description: "Found during scan".into(),
            path: "/dir/found-skill".into(),
        };
        let json = serde_json::to_value(&skill).unwrap();
        assert_eq!(json["name"], "found-skill");
        assert_eq!(json["path"], "/dir/found-skill");
    }

    #[test]
    fn test_external_skill_source_response() {
        let source = ExternalSkillSourceResponse {
            name: "Claude Skills".into(),
            path: "/home/user/.claude/skills".into(),
            source: "claude".into(),
            skill_count: 2,
            skills: vec![
                ScannedSkillResponse {
                    name: "s1".into(),
                    description: "d1".into(),
                    path: "/p1".into(),
                },
                ScannedSkillResponse {
                    name: "s2".into(),
                    description: "d2".into(),
                    path: "/p2".into(),
                },
            ],
        };
        let json = serde_json::to_value(&source).unwrap();
        // Project-wide wire contract: skill_count stays snake_case.
        assert_eq!(json["skill_count"], 2);
        assert!(json.get("skillCount").is_none());
        assert_eq!(json["skills"].as_array().unwrap().len(), 2);
        assert_eq!(json["source"], "claude");
    }

    #[test]
    fn test_external_skill_source_response_custom_source() {
        let source = ExternalSkillSourceResponse {
            name: "My Extras".into(),
            path: "/opt/extras".into(),
            source: "custom-/opt/extras".into(),
            skill_count: 0,
            skills: vec![],
        };
        let json = serde_json::to_value(&source).unwrap();
        assert_eq!(json["source"], "custom-/opt/extras");
        assert_eq!(json["name"], "My Extras");
    }

    #[test]
    fn test_external_skill_source_response_roundtrip() {
        let raw = json!({
            "name": "Gemini Skills",
            "path": "/home/user/.gemini/skills",
            "source": "gemini",
            "skill_count": 0,
            "skills": []
        });
        let parsed: ExternalSkillSourceResponse = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(parsed.source, "gemini");
        assert_eq!(parsed.name, "Gemini Skills");
        assert_eq!(parsed.skill_count, 0);
        let round = serde_json::to_value(&parsed).unwrap();
        assert_eq!(round, raw);
    }

    #[test]
    fn test_named_path_response() {
        let path = NamedPathResponse {
            name: "Claude Config".into(),
            path: "/home/user/.claude".into(),
        };
        let json = serde_json::to_value(&path).unwrap();
        assert_eq!(json["name"], "Claude Config");
        assert_eq!(json["path"], "/home/user/.claude");
    }

    #[test]
    fn test_skill_paths_response() {
        let resp = SkillPathsResponse {
            user_skills_dir: "/home/user/.nomifun/skills".into(),
            builtin_skills_dir: "/app/resources/skills".into(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        // Project-wide wire contract: snake_case fields on the wire.
        assert_eq!(json["user_skills_dir"], "/home/user/.nomifun/skills");
        assert_eq!(json["builtin_skills_dir"], "/app/resources/skills");
        assert!(json.get("userSkillsDir").is_none());
        assert!(json.get("builtinSkillsDir").is_none());
    }

    // -- Preset rules --

    const PRESET_ID: &str = "0190f5fe-7c00-7a00-8000-000000000001";

    #[test]
    fn test_read_preset_rule_request_with_locale() {
        let raw = json!({"preset_id": PRESET_ID, "locale": "zh-CN"});
        let req: ReadPresetRuleRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(req.preset_id, PRESET_ID);
        assert_eq!(req.locale.as_deref(), Some("zh-CN"));
    }

    #[test]
    fn test_read_preset_rule_request_without_locale() {
        let raw = json!({"preset_id": PRESET_ID});
        let req: ReadPresetRuleRequest = serde_json::from_value(raw).unwrap();
        assert!(req.locale.is_none());
    }

    #[test]
    fn test_write_preset_rule_request() {
        let raw = json!({
            "preset_id": PRESET_ID,
            "content": "# Rules\nBe helpful.",
            "locale": "en-US"
        });
        let req: WritePresetRuleRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(req.preset_id, PRESET_ID);
        assert_eq!(req.content, "# Rules\nBe helpful.");
        assert_eq!(req.locale.as_deref(), Some("en-US"));
    }

    #[test]
    fn preset_rule_request_rejects_prefixed_uuidv7_entity_value() {
        let raw = json!({
            "preset_id": "preset_0190f5fe-7c00-7a00-8abc-012345678901"
        });
        assert!(serde_json::from_value::<ReadPresetRuleRequest>(raw).is_err());
    }

    #[test]
    fn preset_rule_request_rejects_catalog_natural_key() {
        assert!(
            serde_json::from_value::<ReadPresetRuleRequest>(json!({
                "preset_id": "builtin-office"
            }))
            .is_err()
        );
    }

    #[test]
    fn test_read_builtin_resource_request() {
        // Project-wide wire contract: the frontend sends `file_name`.
        let raw = json!({"file_name": "code-review.md"});
        let req: ReadBuiltinResourceRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(req.file_name, "code-review.md");

        // Legacy camelCase now fails — matches project-wide wire contract.
        let legacy = json!({"fileName": "code-review.md"});
        assert!(serde_json::from_value::<ReadBuiltinResourceRequest>(legacy).is_err());
    }

    // -- External paths --

    #[test]
    fn test_add_external_path_request() {
        let raw = json!({"name": "My Skills", "path": "/path/to/skills"});
        let req: AddExternalPathRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(req.name, "My Skills");
        assert_eq!(req.path, "/path/to/skills");
    }

    #[test]
    fn test_remove_external_path_request() {
        let raw = json!({"path": "/path/to/skills"});
        let req: RemoveExternalPathRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(req.path, "/path/to/skills");
    }

    #[test]
    fn test_skill_market_sync_request_defaults_sources() {
        let req: SkillMarketSyncRequest = serde_json::from_value(json!({})).unwrap();
        assert!(req.sources.is_empty());
    }

    #[test]
    fn test_skill_market_response_serializes_snake_case() {
        let resp = SkillMarketSyncResponse {
            fetched_at: 123,
            items: vec![SkillMarketItemResponse {
                id: "clawhub:owner/demo".into(),
                source: "clawhub".into(),
                rank: 1,
                name: "demo".into(),
                description: "Demo skill".into(),
                url: "https://clawhub.ai/owner/skills/demo".into(),
                install_command: "openclaw skills install @owner/demo".into(),
                tags: vec!["coding".into()],
                audience_tags: vec!["developer".into()],
                scenario_tags: vec!["coding".into()],
                stats: Some("1.2k installs".into()),
            }],
            errors: vec![],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["fetched_at"], 123);
        assert!(json.get("fetchedAt").is_none());
        assert_eq!(json["items"][0]["install_command"], "openclaw skills install @owner/demo");
        assert!(json["items"][0].get("installCommand").is_none());
        assert!(json.get("errors").is_none());
    }

    #[test]
    fn test_skill_market_mcp_config_response_serializes_snake_case() {
        let req = SkillMarketMcpConfigRequest {
            source: "mcpworld".into(),
            id: "mcpworld:c7897f8abf0350fbbf5a7fccc3e79bb8".into(),
            url: "https://www.mcpworld.com/zh/detail/c7897f8abf0350fbbf5a7fccc3e79bb8".into(),
        };
        let req_json = serde_json::to_value(&req).unwrap();
        assert_eq!(req_json["source"], "mcpworld");
        assert!(req_json.get("sourceUrl").is_none());

        let resp = SkillMarketMcpConfigResponse {
            config_json: json!({
                "mcpServers": {
                    "playwright": {
                        "command": "npx",
                        "args": ["@playwright/mcp@latest"]
                    }
                }
            }),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json["config_json"].get("mcpServers").is_some());
        assert!(json.get("configJson").is_none());
    }

    #[test]
    fn test_skill_market_package_install_response_serializes_snake_case() {
        let resp = SkillMarketPackageInstallResponse {
            package: SkillMarketPackageResponse {
                name: "Test Automation".into(),
                description: "Testing workflow package".into(),
                instructions: "# Test Automation".into(),
                skill_slugs: vec!["superpowers-tdd".into()],
                avatar: None,
            },
            installed_skill_names: vec!["superpowers-tdd".into(), "test-case-generator".into()],
            errors: vec![SkillMarketPackageInstallError {
                skill_slug: "missing-skill".into(),
                error: "download failed".into(),
            }],
        };

        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(
            json["installed_skill_names"],
            serde_json::json!(["superpowers-tdd", "test-case-generator"])
        );
        assert!(json.get("installedSkillNames").is_none());
        assert_eq!(json["errors"][0]["skill_slug"], "missing-skill");
    }
}

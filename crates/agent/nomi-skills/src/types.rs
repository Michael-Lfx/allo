use serde::{Deserialize, Serialize};

// Re-export EffortLevel from nomi-types (single source of truth)
pub use nomi_types::skill_types::EffortLevel;

/// Raw fields from skill frontmatter (YAML deserialization target).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FrontmatterData {
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "allowed-tools")]
    pub allowed_tools: Option<StringOrVec>,
    #[serde(rename = "argument-hint")]
    pub argument_hint: Option<String>,
    pub arguments: Option<StringOrVec>,
    #[serde(rename = "when-to-use")]
    pub when_to_use: Option<String>,
    pub version: Option<String>,
    pub model: Option<String>,
    pub effort: Option<StringOrNumber>,
    /// "inline" | "fork"
    pub context: Option<String>,
    pub agent: Option<String>,
    pub paths: Option<StringOrVec>,
    #[serde(rename = "user-invocable")]
    pub user_invocable: Option<BoolOrString>,
    #[serde(rename = "hide-from-slash-command-tool")]
    pub hide_from_model_invocation: Option<BoolOrString>,
    #[serde(rename = "disable-model-invocation")]
    pub disable_model_invocation: Option<BoolOrString>,
    #[serde(rename = "invocation-policy")]
    pub invocation_policy: Option<String>,
    #[serde(rename = "catalog-id")]
    pub catalog_id: Option<String>,
    /// Raw hooks YAML — converted to serde_json::Value in SkillMetadata (Phase 11 will parse fully)
    pub hooks: Option<serde_yaml::Value>,
    // No serde(flatten) + HashMap — known serde_yaml bug with that combination
}

/// String or list of strings (used for allowed-tools, paths, arguments, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StringOrVec {
    Single(String),
    Multiple(Vec<String>),
}

/// String or integer (used for effort field)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StringOrNumber {
    Str(String),
    Num(i64),
}

/// Boolean or "true"/"false" string (used for user-invocable, hide-from-slash-command-tool)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BoolOrString {
    Bool(bool),
    Str(String),
}

/// Parsed frontmatter plus body content.
#[derive(Debug, Clone)]
pub struct ParsedMarkdown {
    pub frontmatter: FrontmatterData,
    pub content: String,
}

/// Skill execution context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExecutionContext {
    #[default]
    Inline,
    Fork,
}

/// How a skill may be activated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InvocationPolicy {
    /// Appear in the L1 catalog; the model may call the Skill tool.
    #[default]
    ModelAuto,
    /// Only the user `/` / chip path may load this skill.
    SlashOnly,
}

impl InvocationPolicy {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("/invoke") | Some("slash-only") | Some("user-only") | Some("@invoke") => {
                Self::SlashOnly
            }
            _ => Self::ModelAuto,
        }
    }

    pub fn is_slash_only(self) -> bool {
        matches!(self, Self::SlashOnly)
    }
}

/// Where the skill file originates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SkillSource {
    /// `<config_dir>/nomi/skills/`
    #[default]
    User,
    /// .nomi/skills/ (project-level)
    Project,
    /// .nomi/.managed/skills/
    Managed,
    /// Built-in bundled skills
    Bundled,
    /// Loaded via MCP protocol
    Mcp,
    /// .nomi/commands/ (legacy compatibility)
    Legacy,
}

/// How the skill was discovered during loading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoadedFrom {
    #[default]
    Skills,
    CommandsDeprecated,
    Managed,
    Bundled,
    Mcp,
}

/// Normalized skill metadata, derived from FrontmatterData.
#[derive(Debug, Clone, Default)]
pub struct SkillMetadata {
    pub name: String,
    pub display_name: Option<String>,
    pub description: String,
    pub has_user_specified_description: bool,
    pub allowed_tools: Vec<String>,
    pub argument_hint: Option<String>,
    pub argument_names: Vec<String>,
    pub when_to_use: Option<String>,
    pub version: Option<String>,
    /// None means "don't override"; "inherit" in frontmatter is normalized to None
    pub model: Option<String>,
    pub disable_model_invocation: bool,
    pub user_invocable: bool,
    pub execution_context: ExecutionContext,
    pub agent: Option<String>,
    pub effort: Option<EffortLevel>,
    /// Glob patterns after brace expansion
    pub paths: Vec<String>,
    /// Hooks converted from serde_yaml::Value — full parse deferred to Phase 11
    pub hooks_raw: Option<serde_json::Value>,
    pub source: SkillSource,
    pub loaded_from: LoadedFrom,
    /// Body content after frontmatter
    pub content: String,
    /// Character count of body (approximate token estimate: ~4 chars/token for English, not exact)
    pub content_length: usize,
    /// Directory containing the skill file
    pub skill_root: Option<String>,
    /// Source-qualified Flowy catalog identity (`builtin:officecli`).
    pub catalog_id: Option<String>,
    pub invocation_policy: InvocationPolicy,
    /// True when the skill ships hooks, scripts, or privileged tools.
    pub requires_confirmation: bool,
}

/// True when a skill should require a human confirmation before implicit load.
pub fn skill_requires_confirmation(
    hooks_raw: &Option<serde_json::Value>,
    allowed_tools: &[String],
    has_embedded_shell: bool,
    skill_root: Option<&str>,
) -> bool {
    if hooks_raw.is_some() || !allowed_tools.is_empty() || has_embedded_shell {
        return true;
    }
    let Some(root) = skill_root else {
        return false;
    };
    std::path::Path::new(root).join("scripts").is_dir()
}

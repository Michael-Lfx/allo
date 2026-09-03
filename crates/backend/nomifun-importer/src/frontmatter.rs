//! Markdown frontmatter parsing for `agents/*.md` and `skills/*/SKILL.md`
//! (docs/agent-store/02 §5.1). Frontmatter fields are structured-preserved;
//! freeform body text is kept but never treated as executable content.

use serde_json::{Map, Value, json};

#[derive(Debug, thiserror::Error)]
pub enum DocError {
    #[error("frontmatter parse failed: {0}")]
    Yaml(String),
    #[error("missing required field: {0}")]
    MissingField(String),
}

/// Split leading `---` frontmatter from the body. Returns `None` when the
/// document has no frontmatter block.
///
/// Real marketplace files use both LF and CRLF line endings, and their YAML
/// is frequently *not* strictly parseable (unquoted quotes, embedded JSON
/// strings, folded scalars). The parser therefore degrades lazily: strict
/// YAML first, then a line-oriented fallback that keeps every top-level
/// `key: value` scalar so at least `name` / `description` survive.
pub fn parse_frontmatter(text: &str) -> Option<(Value, String)> {
    // Normalize CRLF to LF up front so all subsequent scans see one shape.
    let text = text.replace("\r\n", "\n");
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    let yaml = &rest[..end];
    let mut body = &rest[end + "\n---".len()..];
    body = body.strip_prefix('\n').unwrap_or(body);

    match serde_yaml::from_str::<serde_yaml::Value>(yaml) {
        Ok(value) => match serde_json::to_value(value) {
            Ok(json) => Some((json, body.to_owned())),
            Err(_) => Some((loose_frontmatter(yaml), body.to_owned())),
        },
        Err(_) => Some((loose_frontmatter(yaml), body.to_owned())),
    }
}

/// Line-oriented fallback: extract every **top-level** (no leading space)
/// `key: value` scalar as a string. Unknown keys keep their raw scalar so
/// structured fields (`description_zh`, `version`, `homepage`, …) still
/// survive; complex/multiline values are dropped rather than guessed.
fn loose_frontmatter(yaml: &str) -> Value {
    let mut fields = Map::new();
    for line in yaml.lines() {
        if line.starts_with(' ') || line.starts_with('\t') || line.trim().is_empty() {
            continue;
        }
        let Some(colon) = line.find(':') else { continue };
        let key = line[..colon].trim().to_owned();
        if key.is_empty() {
            continue;
        }
        let raw = line[colon + 1..].trim().to_owned();
        let value = unquote_scalar(&raw);
        fields.insert(key, Value::String(value));
    }
    Value::Object(fields)
}

/// Strip a single wrapping pair of matching quotes from a scalar.
fn unquote_scalar(raw: &str) -> String {
    let s = raw.trim();
    let trimmed = match s {
        _ if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 => &s[1..s.len() - 1],
        _ if s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2 => &s[1..s.len() - 1],
        _ => s,
    };
    trimmed.to_owned()
}

/// Strict variant: frontmatter must exist and parse; used for definitions
/// whose `name` is required (02 §4: `name` 唯一必填).
pub fn parse_frontmatter_strict(text: &str, what: &str) -> Result<(Value, String), DocError> {
    let (fields, body) =
        parse_frontmatter(text).ok_or_else(|| DocError::Yaml(format!("{what} lacks a parseable frontmatter block")))?;
    Ok((fields, body))
}

// --- field extraction helpers (lenient across types) ---

pub fn str_field(fields: &Value, key: &str) -> Option<String> {
    fields.get(key).and_then(|value| match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    })
}

fn array_of_strings(fields: &Value, key: &str) -> Vec<String> {
    fields
        .get(key)
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| match item {
                    Value::String(text) => Some(text.clone()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn u32_field(fields: &Value, key: &str) -> Option<u32> {
    fields.get(key).and_then(|value| value.as_u64()).map(|n| n as u32)
}

/// Structured Agent document (`agents/*.md`, 02 §5.1). Every listed
/// frontmatter field is preserved; the full prompt body is retained in
/// `body` for later adapter stages but is never exposed through the public
/// protocol.
#[derive(Debug, Clone)]
pub struct AgentDoc {
    pub name: String,
    pub description: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub max_turns: Option<u32>,
    pub tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
    pub skills: Vec<String>,
    pub memory: Option<String>,
    pub background: Option<String>,
    pub isolation: Option<String>,
    /// Plugin-level Agents: `permissionMode` is ignored by the source runtime
    /// itself (02 §5.1) — tracked so the compatibility report can say so.
    pub permission_mode: Option<String>,
    /// Localized display metadata (WorkBuddy experts carry these on the agent
    /// frontmatter too, mirroring `plugin.json`).
    pub display_name: crate::manifest::LocalizedText,
    pub profession: crate::manifest::LocalizedText,
    pub body: String,
}

impl AgentDoc {
    pub fn to_payload(&self, id: &str, version: &str, relative_path: &str) -> Value {
        let mut payload = json!({
            "id": id,
            "version": version,
            "name": self.name,
            "tools": self.tools,
            "disallowed_tools": self.disallowed_tools,
            "skills": self.skills,
            "permission_mode_ignored": self.permission_mode.is_some(),
            "relative_path": relative_path,
        });
        set_opt(&mut payload, "description", &self.description);
        set_opt(&mut payload, "model", &self.model);
        set_opt(&mut payload, "effort", &self.effort);
        set_opt(&mut payload, "max_turns", &self.max_turns.map(|n| json!(n)));
        set_opt(&mut payload, "memory", &self.memory);
        set_opt(&mut payload, "background", &self.background);
        set_opt(&mut payload, "isolation", &self.isolation);
        set_localized(&mut payload, "display_name", &self.display_name);
        set_localized(&mut payload, "profession", &self.profession);
        payload
    }

    pub fn has_ignored_permission_fields(&self) -> bool {
        self.permission_mode.is_some()
    }
}

fn set_opt<T: serde::Serialize>(payload: &mut Value, key: &str, value: &Option<T>) {
    if let Some(value) = value {
        payload[key] = serde_json::to_value(value).unwrap_or(Value::Null);
    }
}

fn set_localized(payload: &mut Value, key: &str, value: &crate::manifest::LocalizedText) {
    if !value.is_empty() {
        let mut map = serde_json::Map::new();
        if let Some(en) = &value.en {
            map.insert("en".into(), json!(en));
        }
        if let Some(zh) = &value.zh {
            map.insert("zh".into(), json!(zh));
        }
        payload[key] = Value::Object(map);
    }
}

/// Parse a locale map or plain string into `LocalizedText`
/// (`displayName: {en, zh}` or `displayName: "FBSir"`).
fn parse_localized(fields: &Value, key: &str) -> crate::manifest::LocalizedText {
    let mut out = crate::manifest::LocalizedText::default();
    match fields.get(key) {
        Some(Value::String(text)) => out.zh = Some(text.clone()),
        Some(Value::Object(map)) => {
            for (k, v) in map {
                if let Some(text) = v.as_str() {
                    match k.as_str() {
                        "en" => out.en = Some(text.to_owned()),
                        "zh" => out.zh = Some(text.to_owned()),
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
    out
}

pub fn parse_agent(text: &str, what: &str) -> Result<AgentDoc, DocError> {
    let (fields, body) = parse_frontmatter_strict(text, what)?;
    let name = str_field(&fields, "name")
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| DocError::MissingField(format!("{what}::name")))?;
    Ok(AgentDoc {
        name,
        description: str_field(&fields, "description"),
        model: str_field(&fields, "model"),
        effort: str_field(&fields, "effort"),
        max_turns: u32_field(&fields, "maxTurns"),
        tools: array_of_strings(&fields, "tools"),
        disallowed_tools: array_of_strings(&fields, "disallowedTools"),
        skills: array_of_strings(&fields, "skills"),
        memory: str_field(&fields, "memory"),
        background: str_field(&fields, "background"),
        isolation: str_field(&fields, "isolation"),
        permission_mode: str_field(&fields, "permissionMode"),
        display_name: parse_localized(&fields, "displayName"),
        profession: parse_localized(&fields, "profession"),
        body,
    })
}

/// Skill document (`skills/<slug>/SKILL.md`, 01 §6). The full body stays in
/// the materialized snapshot; `instructions_ref` points at it.
#[derive(Debug, Clone)]
pub struct SkillDoc {
    pub name: String,
    pub description: Option<String>,
    /// `$ARGUMENTS` usage retained in the raw body; we only record presence.
    pub has_arguments_note: bool,
    pub body: String,
}

pub fn parse_skill(text: &str, what: &str) -> Result<SkillDoc, DocError> {
    let (fields, body) = parse_frontmatter_strict(text, what)?;
    let name = str_field(&fields, "name").filter(|name| !name.trim().is_empty());
    Ok(SkillDoc {
        name: name.unwrap_or_else(|| what.to_owned()),
        description: str_field(&fields, "description"),
        has_arguments_note: body.contains("$ARGUMENTS"),
        body,
    })
}

/// A scalar map helper for `userConfig`-style values: `{"key": schema}`.
pub fn object_map(value: &Value) -> Option<&Map<String, Value>> {
    value.as_object()
}

#[cfg(test)]
mod tests {
    use super::*;

    const AGENT_MD: &str = r#"---
name: software-team-lead
description: Lead planner
model: gpt-5
effort: high
maxTurns: 30
tools:
  - read_file
  - write_file
disallowedTools:
  - rm
skills:
  - planning
memory: project
background: Architect-minded planner
isolation: worktree
permissionMode: default
---
Plan the team's release.
"#;

    #[test]
    fn agent_frontmatter_fields_are_structured_preserved() {
        let doc = parse_agent(AGENT_MD, "agents/software-team-lead.md").unwrap();
        assert_eq!(doc.name, "software-team-lead");
        assert_eq!(doc.description.as_deref(), Some("Lead planner"));
        assert_eq!(doc.model.as_deref(), Some("gpt-5"));
        assert_eq!(doc.effort.as_deref(), Some("high"));
        assert_eq!(doc.max_turns, Some(30));
        assert_eq!(doc.tools, vec!["read_file", "write_file"]);
        assert_eq!(doc.disallowed_tools, vec!["rm"]);
        assert_eq!(doc.skills, vec!["planning"]);
        assert_eq!(doc.memory.as_deref(), Some("project"));
        assert_eq!(doc.background.as_deref(), Some("Architect-minded planner"));
        assert_eq!(doc.isolation.as_deref(), Some("worktree"));
        assert_eq!(doc.permission_mode.as_deref(), Some("default"));
        assert!(doc.has_ignored_permission_fields());
        assert!(doc.body.contains("Plan the team's release."));
    }

    #[test]
    fn missing_frontmatter_or_name_is_a_component_error() {
        assert!(parse_agent("no frontmatter", "x.md").is_err());
        assert!(parse_agent("---\ndescription: nameless\n---\nbody", "x.md").is_err());
    }

    #[test]
    fn skill_arguments_note_is_detected() {
        let text = "---\nname: hello\ndescription: demo\n---\nRun `$ARGUMENTS` here.\n";
        let doc = parse_skill(text, "skills/hello/SKILL.md").unwrap();
        assert_eq!(doc.name, "hello");
        assert!(doc.has_arguments_note);
    }
}
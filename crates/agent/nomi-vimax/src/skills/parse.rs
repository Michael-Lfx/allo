//! Parse ViMax vertical skill packages (`SKILL.md` with YAML frontmatter).

use std::path::Path;

use crate::domain::WorkflowKind;
use crate::error::{VimaxError, VimaxResult};

use super::model::{
    sanitize_skill_name, SkillId, SkillSource, SkillVisibility, VerticalSkill, VerticalSkillDraft,
};

pub const SKILL_MANIFEST: &str = "SKILL.md";

#[derive(Debug, Default)]
struct Frontmatter {
    name: Option<String>,
    display_name: Option<String>,
    description: Option<String>,
    category: Option<String>,
    version: Option<String>,
    tags: Vec<String>,
    compatible_modes: Vec<WorkflowKind>,
    visibility: SkillVisibility,
    requirement_overlay: Option<String>,
    style_overlay: Option<String>,
}

/// Split YAML frontmatter (`---` … `---`) from markdown body.
pub fn split_frontmatter(raw: &str) -> VimaxResult<(String, String)> {
    let trimmed = raw.trim_start_matches('\u{feff}');
    let rest = trimmed.strip_prefix("---").ok_or_else(|| {
        VimaxError::InvalidParams("SKILL.md missing YAML frontmatter".into())
    })?;
    let rest = rest.strip_prefix('\n').or_else(|| rest.strip_prefix("\r\n")).unwrap_or(rest);
    let (fm, body) = if let Some(idx) = rest.find("\n---") {
        let fm = &rest[..idx];
        let after = &rest[idx + 4..];
        let body = after
            .strip_prefix('\n')
            .or_else(|| after.strip_prefix("\r\n"))
            .unwrap_or(after);
        (fm.to_string(), body.to_string())
    } else {
        return Err(VimaxError::InvalidParams(
            "SKILL.md frontmatter not closed".into(),
        ));
    };
    Ok((fm, body))
}

fn parse_frontmatter_yaml(fm: &str) -> VimaxResult<Frontmatter> {
    let value: serde_yaml::Value = serde_yaml::from_str(fm)
        .map_err(|e| VimaxError::InvalidParams(format!("invalid SKILL.md frontmatter: {e}")))?;
    let map = value.as_mapping().ok_or_else(|| {
        VimaxError::InvalidParams("SKILL.md frontmatter must be a mapping".into())
    })?;

    let mut out = Frontmatter::default();
    for (k, v) in map {
        let key = k.as_str().unwrap_or("").to_ascii_lowercase().replace('_', "-");
        match key.as_str() {
            "name" => out.name = v.as_str().map(|s| s.to_string()),
            "display-name" | "displayname" | "title" => {
                out.display_name = v.as_str().map(|s| s.to_string())
            }
            "description" => out.description = v.as_str().map(|s| s.to_string()),
            "category" => out.category = v.as_str().map(|s| s.to_string()),
            "version" => out.version = Some(match v {
                serde_yaml::Value::String(s) => s.clone(),
                serde_yaml::Value::Number(n) => n.to_string(),
                _ => String::new(),
            }),
            "tags" => out.tags = yaml_string_list(v),
            "compatible-modes" | "compatible_modes" | "modes" => {
                out.compatible_modes = yaml_string_list(v)
                    .into_iter()
                    .filter_map(|s| WorkflowKind::parse(&s))
                    .collect();
            }
            "visibility" => {
                if let Some(s) = v.as_str() {
                    out.visibility = SkillVisibility::parse(s).unwrap_or(SkillVisibility::Private);
                }
            }
            "requirement-overlay" | "requirement_overlay" | "user-requirement" => {
                out.requirement_overlay = yaml_block_string(v);
            }
            "style-overlay" | "style_overlay" | "style" => {
                out.style_overlay = yaml_block_string(v);
            }
            _ => {}
        }
    }
    Ok(out)
}

fn yaml_string_list(v: &serde_yaml::Value) -> Vec<String> {
    match v {
        serde_yaml::Value::Sequence(seq) => seq
            .iter()
            .filter_map(|item| item.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect(),
        serde_yaml::Value::String(s) => s
            .split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn yaml_block_string(v: &serde_yaml::Value) -> Option<String> {
    match v {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Null => None,
        other => Some(format!("{other:?}")),
    }
}

/// Parse a raw SKILL.md document into a VerticalSkill.
pub fn parse_skill_md(
    raw: &str,
    id: SkillId,
    dir: impl Into<String>,
) -> VimaxResult<VerticalSkill> {
    let (fm, body) = split_frontmatter(raw)?;
    let meta = parse_frontmatter_yaml(&fm)?;
    let name = meta
        .name
        .as_deref()
        .and_then(sanitize_skill_name)
        .unwrap_or_else(|| id.name.clone());
    if name != id.name && id.source != SkillSource::Builtin {
        // Prefer directory / id name for stability; frontmatter name should match.
        tracing::warn!(
            skill = %id.qualified(),
            frontmatter_name = %name,
            "skill frontmatter name differs from id; using id name"
        );
    }
    let description = meta
        .description
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            VimaxError::InvalidParams(format!("skill '{}' missing description", id.qualified()))
        })?;
    let display_name = meta
        .display_name
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| humanize_name(&id.name));

    Ok(VerticalSkill {
        name: id.name.clone(),
        display_name,
        description,
        category: meta.category.unwrap_or_default(),
        version: meta
            .version
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "1.0.0".into()),
        tags: meta.tags,
        compatible_modes: meta.compatible_modes,
        visibility: meta.visibility,
        requirement_overlay: meta.requirement_overlay.unwrap_or_default(),
        style_overlay: meta.style_overlay.unwrap_or_default(),
        playbook: body.trim().to_string(),
        dir: dir.into(),
        id,
    })
}

fn humanize_name(name: &str) -> String {
    name.split('-')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut chars = p.chars();
            match chars.next() {
                Some(c) => format!("{}{}", c.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Load skill from a directory containing SKILL.md.
pub fn load_skill_dir(dir: &Path, source: SkillSource) -> VimaxResult<VerticalSkill> {
    let manifest = dir.join(SKILL_MANIFEST);
    let raw = std::fs::read_to_string(&manifest)?;
    let fallback_name = dir
        .file_name()
        .and_then(|s| s.to_str())
        .and_then(sanitize_skill_name)
        .ok_or_else(|| {
            VimaxError::InvalidParams(format!("invalid skill directory: {}", dir.display()))
        })?;
    let (fm, _) = split_frontmatter(&raw)?;
    let meta = parse_frontmatter_yaml(&fm)?;
    let name = meta
        .name
        .as_deref()
        .and_then(sanitize_skill_name)
        .unwrap_or(fallback_name);
    let id = SkillId::new(source, name);
    let mut skill = parse_skill_md(&raw, id, dir.to_string_lossy())?;
    if source == SkillSource::Hub {
        skill.visibility = SkillVisibility::Hub;
    } else if source == SkillSource::Builtin {
        skill.visibility = SkillVisibility::Hub;
    }
    Ok(skill)
}

/// Serialize a draft into SKILL.md text.
pub fn build_skill_md(draft: &VerticalSkillDraft) -> VimaxResult<String> {
    let name = sanitize_skill_name(&draft.name).ok_or_else(|| {
        VimaxError::InvalidParams(format!("invalid skill name: {}", draft.name))
    })?;
    if draft.description.trim().is_empty() {
        return Err(VimaxError::InvalidParams(format!(
            "skill '{name}' has empty description"
        )));
    }
    let display = draft
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&name);
    let version = draft
        .version
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("1.0.0");
    let category = draft.category.as_deref().unwrap_or("").trim();
    let modes: Vec<String> = draft
        .compatible_modes
        .iter()
        .filter_map(|m| WorkflowKind::parse(m).map(|k| k.as_str().to_string()))
        .collect();

    let mut fm = String::from("---\n");
    fm.push_str(&format!("name: {name}\n"));
    fm.push_str(&format!("display-name: {display}\n"));
    fm.push_str(&format!("description: {}\n", yaml_escape(&draft.description)));
    if !category.is_empty() {
        fm.push_str(&format!("category: {category}\n"));
    }
    fm.push_str(&format!("version: \"{version}\"\n"));
    if !draft.tags.is_empty() {
        fm.push_str("tags:\n");
        for tag in &draft.tags {
            let t = tag.trim();
            if !t.is_empty() {
                fm.push_str(&format!("  - {t}\n"));
            }
        }
    }
    if !modes.is_empty() {
        fm.push_str("compatible-modes:\n");
        for m in &modes {
            fm.push_str(&format!("  - {m}\n"));
        }
    }
    fm.push_str("visibility: private\n");
    if let Some(req) = draft.requirement_overlay.as_deref() {
        if !req.trim().is_empty() {
            fm.push_str("requirement-overlay: |\n");
            for line in req.lines() {
                fm.push_str(&format!("  {line}\n"));
            }
        }
    }
    if let Some(style) = draft.style_overlay.as_deref() {
        if !style.trim().is_empty() {
            fm.push_str("style-overlay: |\n");
            for line in style.lines() {
                fm.push_str(&format!("  {line}\n"));
            }
        }
    }
    fm.push_str("---\n\n");
    let body = draft.playbook.as_deref().unwrap_or("").trim();
    if body.is_empty() {
        fm.push_str("# Playbook\n\nDescribe the director methodology for this vertical skill.\n");
    } else {
        fm.push_str(body);
        if !body.ends_with('\n') {
            fm.push('\n');
        }
    }
    Ok(fm)
}

fn yaml_escape(s: &str) -> String {
    if s.contains('\n') || s.contains(':') || s.contains('#') || s.contains('"') {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_skill_md() {
        let raw = r#"---
name: luxury-tvc
display-name: 高奢版TVC
description: High-end commercial TVC director
category: advertising
version: "1.0.0"
tags: [tvc, luxury]
compatible-modes: [idea2video, script2video]
requirement-overlay: |
  Keep pacing premium and sparse.
style-overlay: |
  luxury commercial cinematography
---

# Playbook

Shoot like a luxury house film.
"#;
        let skill = parse_skill_md(
            raw,
            SkillId::new(SkillSource::Builtin, "luxury-tvc"),
            "",
        )
        .unwrap();
        assert_eq!(skill.display_name, "高奢版TVC");
        assert!(skill.requirement_overlay.contains("premium"));
        assert!(skill.style_overlay.contains("luxury"));
        assert!(skill.playbook.contains("luxury house"));
        assert_eq!(skill.compatible_modes.len(), 2);
    }
}

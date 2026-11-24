use crate::{InstallRecord, SkillId};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledSkill {
    pub id: SkillId,
    pub name: String,
    pub description: String,
    pub root: PathBuf,
    pub auto_invoke: bool,
}

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("could not read installed skill: {0}")]
    Io(#[from] std::io::Error),
    #[error("SKILL.md frontmatter is malformed")]
    MalformedFrontmatter,
    #[error("SKILL.md frontmatter is missing {0}")]
    MissingField(&'static str),
}

pub fn project_installed(
    root: &Path,
    record: &InstallRecord,
) -> Result<InstalledSkill, RegistryError> {
    let source = fs::read_to_string(root.join("SKILL.md"))?;
    let frontmatter = frontmatter(&source)?;
    Ok(InstalledSkill {
        id: record.id.clone(),
        name: frontmatter.0.ok_or(RegistryError::MissingField("name"))?,
        description: frontmatter
            .1
            .ok_or(RegistryError::MissingField("description"))?,
        root: root.to_path_buf(),
        auto_invoke: record.auto_invoke,
    })
}

#[must_use]
pub fn prompt_lines(skills: &[InstalledSkill]) -> Vec<String> {
    skills
        .iter()
        .filter(|skill| skill.auto_invoke)
        .map(|skill| format!("{}: {}", skill.name, skill.description))
        .collect()
}

fn frontmatter(
    source: &str,
) -> Result<(Option<String>, Option<String>), RegistryError> {
    let mut lines = source.lines();
    let opened = lines
        .by_ref()
        .find(|line| !line.trim().is_empty())
        .is_some_and(|line| line.trim() == "---");
    if !opened {
        return Err(RegistryError::MalformedFrontmatter);
    }
    let mut name = None;
    let mut description = None;
    let mut closed = false;
    for line in lines {
        if line.trim() == "---" {
            closed = true;
            break;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = unquote(value.trim()).to_owned();
        match key.trim() {
            "name" => name = Some(value),
            "description" => description = Some(value),
            _ => {}
        }
    }
    if !closed {
        return Err(RegistryError::MalformedFrontmatter);
    }
    Ok((name, description))
}

fn unquote(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InstallStep, parse_skill_id};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let sequence = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "flowy-skills-registry-{name}-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn record(auto_invoke: bool) -> InstallRecord {
        InstallRecord {
            id: parse_skill_id("awspace", "pdf", "1.2.3").unwrap(),
            step: InstallStep::Commit,
            auto_invoke,
            scripts_enabled: false,
            content_hash: "abc".into(),
        }
    }

    #[test]
    fn projects_only_frontmatter_summary() {
        let dir = TestDir::new("project");
        fs::write(
            dir.path.join("SKILL.md"),
            "---\nname: pdf\ndescription: Fill PDF forms.\n---\n\n正文不进入摘要。\n",
        )
        .unwrap();
        let skill = project_installed(&dir.path, &record(true)).unwrap();
        assert_eq!(skill.name, "pdf");
        assert_eq!(skill.description, "Fill PDF forms.");
        assert_eq!(skill.id.slug, "pdf");
        assert!(skill.auto_invoke);
    }

    #[test]
    fn prompt_lines_include_only_auto_invoke_skills() {
        let skill = |name: &str, auto_invoke| InstalledSkill {
            id: parse_skill_id("flowy", name, "1.0.0").unwrap(),
            name: name.into(),
            description: format!("{name} description"),
            root: PathBuf::from(name),
            auto_invoke,
        };
        let lines =
            prompt_lines(&[skill("excel", true), skill("community", false)]);
        assert_eq!(lines, ["excel: excel description"]);
    }
}

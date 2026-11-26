use crate::{
    InstallOrigin, InstallRecord, SkillId, Store, StoreError, install_dir,
};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledSkill {
    pub id: SkillId,
    pub name: String,
    pub description: String,
    pub root: PathBuf,
    pub skill_path: PathBuf,
    pub content: String,
    pub auto_invoke: bool,
    pub scripts_enabled: bool,
    pub origin: InstallOrigin,
    pub latest_version: Option<String>,
}

#[derive(Debug, Default)]
pub struct InstalledCatalog {
    pub skills: Vec<InstalledSkill>,
    pub failures: Vec<String>,
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
    let skill_path = root.join("SKILL.md");
    let source = fs::read_to_string(&skill_path)?;
    let frontmatter = frontmatter(&source)?;
    Ok(InstalledSkill {
        id: record.id.clone(),
        name: frontmatter.0.ok_or(RegistryError::MissingField("name"))?,
        description: frontmatter
            .1
            .ok_or(RegistryError::MissingField("description"))?,
        root: root.to_path_buf(),
        skill_path,
        content: source,
        auto_invoke: record.auto_invoke,
        scripts_enabled: record.scripts_enabled,
        origin: record.origin,
        latest_version: record.latest_version.clone(),
    })
}

pub fn load_installed(root: &Path) -> Result<InstalledCatalog, StoreError> {
    let store = Store::open(&root.join("skills").join("market.sqlite3"))?;
    let mut catalog = InstalledCatalog::default();
    for record in store.installed()? {
        let skill_root = install_dir(root, &record.id);
        match project_installed(&skill_root, &record) {
            Ok(skill) => catalog.skills.push(skill),
            Err(error) => catalog
                .failures
                .push(format!("{}: {error}", record.id.slug)),
        }
    }
    Ok(catalog)
}

#[must_use]
pub fn prompt_lines(skills: &[InstalledSkill]) -> Vec<String> {
    skills
        .iter()
        .filter(|skill| skill.auto_invoke)
        .map(|skill| format!("{}: {}", skill.name, skill.description))
        .collect()
}

pub const SKILL_LISTING_CHAR_BUDGET: usize = 20_000;
pub const SKILL_DESC_MAX_CHARS: usize = 250;

fn truncate_description(description: &str, max_chars: usize) -> String {
    if description.chars().count() <= max_chars {
        return description.to_owned();
    }
    let keep = max_chars.saturating_sub(3);
    let mut out: String = description.chars().take(keep).collect();
    out.push_str("...");
    out
}

fn render_available_skills(
    visible: &[&InstalledSkill],
    include_descriptions: bool,
) -> String {
    let mut lines = vec![
        "The following skills provide specialized instructions for specific tasks.".to_string(),
        "Read the full skill file when the task matches its description.".to_string(),
        "When a skill file references a relative path, resolve it against the skill directory (parent of SKILL.md / dirname of the path) and use that absolute path in tool commands.".to_string(),
        String::new(),
        "<available_skills>".to_string(),
    ];
    for skill in visible {
        lines.push("  <skill>".to_string());
        lines.push(format!("    <name>{}</name>", escape_xml(&skill.name)));
        if include_descriptions {
            let desc =
                truncate_description(&skill.description, SKILL_DESC_MAX_CHARS);
            lines.push(format!(
                "    <description>{}</description>",
                escape_xml(&desc)
            ));
        }
        lines.push(format!(
            "    <location>{}</location>",
            escape_xml(&skill.skill_path.display().to_string())
        ));
        lines.push("  </skill>".to_string());
    }
    lines.push("</available_skills>".to_string());
    lines.join("\n")
}

/// PI-compatible model-visible skill listing. The listing escapes every field
/// because descriptions and paths come from installed packages.
#[must_use]
pub fn format_available_skills(skills: &[InstalledSkill]) -> String {
    let mut visible: Vec<&InstalledSkill> =
        skills.iter().filter(|skill| skill.auto_invoke).collect();
    if visible.is_empty() {
        return String::new();
    }
    visible.sort_by_key(|skill| {
        (
            skill.name.as_str(),
            skill.id.owner_handle.as_str(),
            skill.id.slug.as_str(),
        )
    });
    let full = render_available_skills(&visible, true);
    if full.chars().count() > SKILL_LISTING_CHAR_BUDGET {
        render_available_skills(&visible, false)
    } else {
        full
    }
}

#[must_use]
pub fn format_skill_invocation(
    skill: &InstalledSkill,
    additional_instructions: Option<&str>,
) -> String {
    let dirname = skill.root.display();
    let block = format!(
        "<skill name=\"{}\" location=\"{}\">\nReferences are relative to {dirname}.\n\n{}\n</skill>",
        escape_xml(&skill.name),
        escape_xml(&skill.skill_path.display().to_string()),
        skill.content,
    );
    match additional_instructions.filter(|text| !text.is_empty()) {
        Some(extra) => format!("{block}\n\n{extra}"),
        None => block,
    }
}

#[must_use]
pub fn escape_xml(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
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
            origin: InstallOrigin::Community,
            display_name: "PDF".into(),
            summary: "Fill PDF forms.".into(),
            installed_at: 1000,
            latest_version: Some("1.2.3".into()),
            checked_at: Some(1000),
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
        assert!(skill.content.contains("正文不进入摘要"));
    }

    #[test]
    fn prompt_lines_include_only_auto_invoke_skills() {
        let skill = |name: &str, auto_invoke| InstalledSkill {
            id: parse_skill_id("flowy", name, "1.0.0").unwrap(),
            name: name.into(),
            description: format!("{name} description"),
            root: PathBuf::from(name),
            skill_path: PathBuf::from(name).join("SKILL.md"),
            content: "body".into(),
            auto_invoke,
            scripts_enabled: false,
            origin: InstallOrigin::Official,
            latest_version: Some("1.0.0".into()),
        };
        let lines =
            prompt_lines(&[skill("excel", true), skill("community", false)]);
        assert_eq!(lines, ["excel: excel description"]);
    }

    fn make_skill(
        owner: &str,
        slug: &str,
        name: &str,
        description: &str,
        auto_invoke: bool,
    ) -> InstalledSkill {
        let root = PathBuf::from(format!("/skills/{owner}/{slug}"));
        InstalledSkill {
            id: parse_skill_id(owner, slug, "1.0.0").unwrap(),
            name: name.to_owned(),
            description: description.to_owned(),
            skill_path: root.join("SKILL.md"),
            root,
            content: "body".to_owned(),
            auto_invoke,
            scripts_enabled: false,
            origin: InstallOrigin::Community,
            latest_version: Some("1.0.0".to_owned()),
        }
    }

    #[test]
    fn format_available_skills_sorts_deterministically() {
        let a = make_skill("bob", "zeta", "alpha", "first", true);
        let b = make_skill("alice", "beta", "alpha", "second", true);
        let c = make_skill("alice", "alpha", "alpha", "third", true);
        let d = make_skill("carol", "doc", "beta", "fourth", true);
        let hidden = make_skill("dan", "secret", "aaa", "hidden", false);

        let forward = format_available_skills(&[
            d.clone(),
            a.clone(),
            hidden.clone(),
            c.clone(),
            b.clone(),
        ]);
        let reversed = format_available_skills(&[b, c, hidden, a, d]);
        assert_eq!(forward, reversed);

        let pos_c = forward.find("third").unwrap();
        let pos_b = forward.find("second").unwrap();
        let pos_a = forward.find("first").unwrap();
        let pos_d = forward.find("fourth").unwrap();
        assert!(pos_c < pos_b && pos_b < pos_a && pos_a < pos_d);
        assert!(!forward.contains("hidden"));
    }

    #[test]
    fn format_available_skills_truncates_utf8_cjk_descriptions_to_250_chars() {
        let long_cjk = "中".repeat(260);
        let skill = make_skill("flowy", "cjk", "cjk", &long_cjk, true);
        let rendered = format_available_skills(&[skill]);
        let start =
            rendered.find("<description>").unwrap() + "<description>".len();
        let end = rendered.find("</description>").unwrap();
        let desc = &rendered[start..end];
        assert_eq!(desc.chars().count(), SKILL_DESC_MAX_CHARS);
        assert!(desc.ends_with("..."));
        assert_eq!(desc, format!("{}...", "中".repeat(247)));
    }

    #[test]
    fn format_available_skills_uniformly_degrades_over_budget() {
        let desc = "x".repeat(240);
        let skills: Vec<InstalledSkill> = (0..80)
            .map(|idx| {
                make_skill(
                    "owner",
                    &format!("slug-{idx:03}"),
                    &format!("skill-{idx:03}"),
                    &desc,
                    true,
                )
            })
            .collect();
        let rendered = format_available_skills(&skills);
        assert!(!rendered.contains("<description>"));
        assert!(!rendered.contains("</description>"));
        for idx in 0..80 {
            assert!(rendered.contains(&format!("<name>skill-{idx:03}</name>")));
            assert!(rendered.contains(&format!("slug-{idx:03}")));
        }
    }

    #[test]
    fn format_skill_invocation_escapes_xml_attributes() {
        let root = PathBuf::from("/tmp/skills/a&b\"c<d>");
        let skill = InstalledSkill {
            id: parse_skill_id("owner", "slug", "1.0.0").unwrap(),
            name: "my\"<skill>&'".to_owned(),
            description: "desc".to_owned(),
            skill_path: root.join("SKILL.md"),
            root,
            content: "Instructions body".to_owned(),
            auto_invoke: true,
            scripts_enabled: false,
            origin: InstallOrigin::Official,
            latest_version: None,
        };
        let rendered = format_skill_invocation(&skill, Some("extra step"));
        assert!(rendered.starts_with(
            "<skill name=\"my&quot;&lt;skill&gt;&amp;&apos;\" location=\""
        ));
        assert!(rendered.contains("a&amp;b&quot;c&lt;d&gt;"));
        assert!(rendered.ends_with("extra step"));
    }
}

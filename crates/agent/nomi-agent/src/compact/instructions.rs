//! Standing Compact Instructions read from AGENTS.md.
//!
//! A heading whose text is `Compact Instructions` (any ATX level) is guidance
//! the summarizer must keep. The section runs until the next heading of the
//! same or higher level. Global and project AGENTS.md files are both read;
//! later files in the resolver's order are appended so a closer project file
//! can add rules without erasing the user's global ones.

use std::path::Path;

use nomi_config::config::ProjectInstructionsConfig;

use crate::agents_md::resolve_agents_md;

/// Pull every `Compact Instructions` section out of one markdown document.
pub fn extract_compact_instructions(markdown: &str) -> Option<String> {
    let lines: Vec<&str> = markdown.lines().collect();
    let mut sections = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let Some(level) = compact_heading_level(lines[index]) else {
            index += 1;
            continue;
        };
        let start = index + 1;
        let mut end = lines.len();
        for (next_index, line) in lines.iter().enumerate().skip(start) {
            if atx_heading_level(line).is_some_and(|next_level| next_level <= level) {
                end = next_index;
                break;
            }
        }
        let body = lines[start..end].join("\n");
        let body = body.trim();
        if !body.is_empty() {
            sections.push(body.to_string());
        }
        index = end;
    }
    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n\n"))
    }
}

/// Read Compact Instructions from the AGENTS.md files that apply to `cwd`.
pub fn load_compact_instructions(cwd: &Path) -> Option<String> {
    let snapshot = resolve_agents_md(cwd, &ProjectInstructionsConfig::default());
    let mut parts = Vec::new();
    for file in &snapshot.files {
        if let Some(section) = extract_compact_instructions(&file.content) {
            parts.push(section);
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

fn compact_heading_level(line: &str) -> Option<usize> {
    let (level, text) = atx_heading(line)?;
    text.eq_ignore_ascii_case("Compact Instructions")
        .then_some(level)
}

fn atx_heading_level(line: &str) -> Option<usize> {
    atx_heading(line).map(|(level, _)| level)
}

fn atx_heading(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim();
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = trimmed.get(hashes..)?;
    if !rest.starts_with(|c: char| c.is_whitespace()) {
        return None;
    }
    let text = rest.trim().trim_end_matches('#').trim();
    if text.is_empty() {
        None
    } else {
        Some((hashes, text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_section_until_the_next_same_level_heading() {
        let markdown = "\
# Rules\n\
\n\
ignore me\n\
\n\
## Compact Instructions\n\
\n\
keep the API contract\n\
keep migration steps\n\
\n\
## Other\n\
\n\
not instructions\n";
        assert_eq!(
            extract_compact_instructions(markdown).as_deref(),
            Some("keep the API contract\nkeep migration steps")
        );
    }

    #[test]
    fn keeps_nested_headings_inside_the_section() {
        let markdown = "\
## Compact Instructions\n\
\n\
### Must keep\n\
\n\
the schema\n\
\n\
## Next\n";
        let section = extract_compact_instructions(markdown).unwrap();
        assert!(section.contains("### Must keep"));
        assert!(section.contains("the schema"));
        assert!(!section.contains("## Next"));
    }

    #[test]
    fn missing_section_is_none() {
        assert!(extract_compact_instructions("# Agents\n\nno compact block\n").is_none());
    }

    #[test]
    fn heading_match_ignores_ascii_case_and_trailing_hashes() {
        let markdown = "## compact instructions ##\n\nretain the ban list\n";
        assert_eq!(
            extract_compact_instructions(markdown).as_deref(),
            Some("retain the ban list")
        );
    }

    #[test]
    fn loads_the_section_from_a_project_agents_md() {
        let root = std::env::temp_dir().join(format!(
            "nomi-compact-instructions-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(
            root.join("AGENTS.md"),
            "# Project\n\n## Compact Instructions\n\nUNIQUE_COMPACT_RULE\n",
        )
        .unwrap();

        let loaded = load_compact_instructions(&root).unwrap();
        assert!(loaded.contains("UNIQUE_COMPACT_RULE"));
        let _ = std::fs::remove_dir_all(&root);
    }
}

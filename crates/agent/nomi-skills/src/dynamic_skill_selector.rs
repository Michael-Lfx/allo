//! Host-side cheap skill ranking (Codex weighted-lexical style).
//!
//! The ranked ids stay out of the model-visible listing until the L1 catalog
//! exceeds its prompt budget. Ranking always records enough data for SQLite
//! shadow metrics.

use std::cmp::Ordering;

use crate::prompt::{format_skill_entry, get_char_budget};
use crate::types::SkillMetadata;

pub const MAX_QUERY_BYTES: usize = 4_096;
pub const MAX_CANDIDATES: usize = 1_000;
pub const MAX_RESULTS: usize = 50;
pub const SKILL_SEARCH_CATALOG_THRESHOLD: usize = 200;

const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "to", "of", "for", "in", "on", "with", "is", "are", "be",
    "this", "that", "it", "as", "at", "by", "from", "please", "帮我", "一下", "一个",
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SkillSelectionDocument<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub short_description: &'a str,
    pub description: &'a str,
    pub paths: &'a [String],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheapSkillSelection {
    pub candidate_ids: Vec<String>,
    pub query_term_count: usize,
    pub query_truncated: bool,
    pub candidate_set_truncated: bool,
    pub method: &'static str,
}

impl Default for CheapSkillSelection {
    fn default() -> Self {
        Self {
            candidate_ids: Vec::new(),
            query_term_count: 0,
            query_truncated: false,
            candidate_set_truncated: false,
            method: "weighted_lexical_v1",
        }
    }
}

pub trait CheapSkillSelector {
    fn method(&self) -> &'static str;
    fn select(
        &self,
        query: &str,
        documents: &[SkillSelectionDocument<'_>],
        limit: usize,
        attached_files: &[String],
    ) -> CheapSkillSelection;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WeightedLexicalSkillSelector;

impl CheapSkillSelector for WeightedLexicalSkillSelector {
    fn method(&self) -> &'static str {
        "weighted_lexical_v1"
    }

    fn select(
        &self,
        query: &str,
        documents: &[SkillSelectionDocument<'_>],
        limit: usize,
        attached_files: &[String],
    ) -> CheapSkillSelection {
        let (query, query_bytes_truncated) = bounded(query, MAX_QUERY_BYTES);
        let query_phrase = normalize_phrase(query);
        let (query_terms, query_terms_truncated) = query_terms(&query_phrase);
        let query_truncated = query_bytes_truncated || query_terms_truncated;
        let candidate_set_truncated = documents.len() > MAX_CANDIDATES;
        if query_terms.is_empty() || limit == 0 {
            return CheapSkillSelection {
                candidate_ids: Vec::new(),
                query_term_count: query_terms.len(),
                query_truncated,
                candidate_set_truncated,
                method: self.method(),
            };
        }

        let mut scored = documents
            .iter()
            .take(MAX_CANDIDATES)
            .filter_map(|document| {
                let mut score = score_document(&query_phrase, &query_terms, document);
                if path_boost(document.paths, attached_files) {
                    score = score.saturating_add(18);
                }
                (score > 0).then_some((score, document.id, document.name))
            })
            .collect::<Vec<_>>();
        scored.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| left.2.cmp(right.2))
                .then_with(|| left.1.cmp(right.1))
        });

        CheapSkillSelection {
            candidate_ids: scored
                .into_iter()
                .take(limit.min(MAX_RESULTS))
                .map(|(_, id, _)| id.to_string())
                .collect(),
            query_term_count: query_terms.len(),
            query_truncated,
            candidate_set_truncated,
            method: self.method(),
        }
    }
}

/// Documents used by the selector, derived from engine skill metadata.
pub fn documents_from_skills(skills: &[SkillMetadata]) -> Vec<SkillSelectionDocument<'_>> {
    skills
        .iter()
        .map(|skill| SkillSelectionDocument {
            id: skill.catalog_id.as_deref().unwrap_or(skill.name.as_str()),
            name: skill.name.as_str(),
            short_description: skill.when_to_use.as_deref().unwrap_or(""),
            description: skill.description.as_str(),
            paths: skill.paths.as_slice(),
        })
        .collect()
}

/// True when a full L1 listing would exceed the skill prompt budget.
pub fn listing_exceeds_budget(
    skills: &[SkillMetadata],
    context_window_tokens: Option<usize>,
) -> bool {
    if skills.is_empty() {
        return false;
    }
    let budget = get_char_budget(context_window_tokens);
    let full_entries: Vec<String> = skills.iter().map(format_skill_entry).collect();
    let full_total: usize = full_entries
        .iter()
        .map(|entry| unicode_width::UnicodeWidthStr::width(entry.as_str()))
        .sum::<usize>()
        + full_entries.len().saturating_sub(1);
    full_total > budget
}

/// Keep ranked skills first, then fill with the remainder until `limit`.
pub fn apply_ranked_topk(
    skills: &[SkillMetadata],
    ranked_ids: &[String],
    limit: usize,
) -> Vec<SkillMetadata> {
    if limit == 0 {
        return Vec::new();
    }
    let mut selected = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for id in ranked_ids {
        if selected.len() >= limit {
            break;
        }
        if let Some(skill) = skills.iter().find(|skill| skill_matches_id(skill, id))
            && seen.insert(id.clone())
        {
            selected.push(skill.clone());
        }
    }
    for skill in skills {
        if selected.len() >= limit {
            break;
        }
        let key = skill
            .catalog_id
            .clone()
            .unwrap_or_else(|| skill.name.clone());
        if seen.insert(key) {
            selected.push(skill.clone());
        }
    }
    selected
}

pub fn skill_matches_id(skill: &SkillMetadata, id: &str) -> bool {
    let needle = id.trim().trim_start_matches('/');
    skill.catalog_id.as_deref() == Some(needle) || skill.name == needle
}

fn bounded(input: &str, max_bytes: usize) -> (&str, bool) {
    if input.len() <= max_bytes {
        return (input, false);
    }
    let mut end = max_bytes;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    (&input[..end], true)
}

fn normalize_phrase(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || !ch.is_ascii() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn query_terms(phrase: &str) -> (Vec<String>, bool) {
    const MAX_TERMS: usize = 32;
    let mut terms = Vec::new();
    let mut truncated = false;
    for term in phrase.split_whitespace() {
        if STOP_WORDS.contains(&term) || term.len() < 2 {
            continue;
        }
        if terms.len() >= MAX_TERMS {
            truncated = true;
            break;
        }
        if !terms.iter().any(|existing| existing == term) {
            terms.push(term.to_string());
        }
    }
    (terms, truncated)
}

fn score_document(query_phrase: &str, query_terms: &[String], document: &SkillSelectionDocument<'_>) -> u32 {
    let name = normalize_phrase(document.name);
    let short = normalize_phrase(document.short_description);
    let desc = normalize_phrase(document.description);
    let mut score = 0u32;
    if !query_phrase.is_empty() && name == *query_phrase {
        score = score.saturating_add(25);
    }
    for term in query_terms {
        if name == *term {
            score = score.saturating_add(12);
        } else if name.split_whitespace().any(|part| part == term) {
            score = score.saturating_add(8);
        } else if name.contains(term.as_str()) {
            score = score.saturating_add(5);
        }
        if !short.is_empty() && short.contains(term.as_str()) {
            score = score.saturating_add(4);
        }
        if desc.contains(term.as_str()) {
            score = score.saturating_add(2);
        }
    }
    score
}

fn path_boost(globs: &[String], files: &[String]) -> bool {
    if globs.is_empty() || files.is_empty() {
        return false;
    }
    files.iter().any(|file| globs.iter().any(|glob| wildcard_match(glob, file)))
}

pub fn wildcard_match(pattern: &str, candidate: &str) -> bool {
    let pat = pattern.replace('\\', "/");
    let text = candidate.replace('\\', "/");
    if pat == "*" || pat == "**" || pat == "**/*" {
        return true;
    }
    if !pat.contains('*') {
        return text == pat || text.ends_with(&pat) || text.ends_with(&format!("/{pat}"));
    }
    let parts: Vec<&str> = pat.split('*').filter(|part| !part.is_empty()).collect();
    if parts.is_empty() {
        return true;
    }
    let mut idx = 0usize;
    if !pat.starts_with('*') && !text.starts_with(parts[0]) {
        return false;
    }
    for part in &parts {
        match text[idx..].find(part) {
            Some(found) => idx += found + part.len(),
            None => return false,
        }
    }
    if !pat.ends_with('*') {
        return text.ends_with(parts[parts.len() - 1]);
    }
    true
}

pub fn stable_rank_cmp(left: &(u32, &str, &str), right: &(u32, &str, &str)) -> Ordering {
    right
        .0
        .cmp(&left.0)
        .then_with(|| left.2.cmp(right.2))
        .then_with(|| left.1.cmp(right.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc<'a>(id: &'a str, name: &'a str, desc: &'a str) -> SkillSelectionDocument<'a> {
        SkillSelectionDocument {
            id,
            name,
            short_description: "",
            description: desc,
            paths: &[],
        }
    }

    #[test]
    fn exact_name_outranks_description_match() {
        let docs = vec![
            doc("builtin:writer", "writer", "general writing help"),
            doc("builtin:officecli", "officecli", "create office documents with python"),
        ];
        let ranked = WeightedLexicalSkillSelector.select("officecli spreadsheet", &docs, 5, &[]);
        assert_eq!(ranked.candidate_ids[0], "builtin:officecli");
        assert_eq!(ranked.method, "weighted_lexical_v1");
    }

    #[test]
    fn paths_boost_matching_attachments() {
        let paths = vec!["**/*.xlsx".to_string()];
        let docs = vec![
            SkillSelectionDocument {
                id: "builtin:officecli-xlsx",
                name: "officecli-xlsx",
                short_description: "",
                description: "spreadsheets",
                paths: &paths,
            },
            doc("builtin:writer", "writer", "spreadsheets narrative"),
        ];
        let ranked = WeightedLexicalSkillSelector.select(
            "spreadsheets",
            &docs,
            5,
            &["C:/tmp/book.xlsx".to_string()],
        );
        assert_eq!(ranked.candidate_ids[0], "builtin:officecli-xlsx");
    }

    #[test]
    fn wildcard_match_accepts_nested_xlsx() {
        assert!(wildcard_match("**/*.xlsx", "docs/q3/book.xlsx"));
        assert!(!wildcard_match("**/*.xlsx", "docs/q3/book.docx"));
    }
}

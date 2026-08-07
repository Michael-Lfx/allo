//! Load and validate conversation evaluation manifests.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use thiserror::Error;

use crate::types::{Case, Manifest, ScorerSpec, SCHEMA_VERSION};

#[derive(Debug, Error)]
pub enum CorpusError {
    #[error("failed to read manifest: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse manifest JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid manifest: {0}")]
    Invalid(String),
}

/// Load a corpus manifest from disk and validate it.
pub fn load_manifest(path: impl AsRef<Path>) -> Result<Manifest, CorpusError> {
    let text = fs::read_to_string(path)?;
    let manifest: Manifest = serde_json::from_str(&text)?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

/// Validate schema, uniqueness, and scorer shape.
pub fn validate_manifest(manifest: &Manifest) -> Result<(), CorpusError> {
    if manifest.schema_version != SCHEMA_VERSION {
        return Err(CorpusError::Invalid(format!(
            "unsupported schema_version {} (expected {SCHEMA_VERSION})",
            manifest.schema_version
        )));
    }
    if manifest.corpus_version.trim().is_empty() {
        return Err(CorpusError::Invalid(
            "corpus_version must not be empty".to_owned(),
        ));
    }
    if manifest.suite.trim().is_empty() {
        return Err(CorpusError::Invalid("suite must not be empty".to_owned()));
    }
    if manifest.cases.is_empty() {
        return Err(CorpusError::Invalid(
            "manifest must contain at least one case".to_owned(),
        ));
    }

    let mut ids = HashSet::new();
    for case in &manifest.cases {
        validate_case(case)?;
        if !ids.insert(case.id.as_str()) {
            return Err(CorpusError::Invalid(format!(
                "duplicate case id {}",
                case.id
            )));
        }
    }
    Ok(())
}

fn validate_case(case: &Case) -> Result<(), CorpusError> {
    if case.id.trim().is_empty() {
        return Err(CorpusError::Invalid(
            "case id must not be empty".to_owned(),
        ));
    }
    if case.id.contains("://") || case.id.contains('?') || case.id.contains('#') {
        return Err(CorpusError::Invalid(format!(
            "case {} id must be a stable identifier, not a URL or fragment",
            case.id
        )));
    }
    if case.category.trim().is_empty() {
        return Err(CorpusError::Invalid(format!(
            "case {} category must not be empty",
            case.id
        )));
    }
    if case.prompt.trim().is_empty() {
        return Err(CorpusError::Invalid(format!(
            "case {} prompt must not be empty",
            case.id
        )));
    }
    if case.scorers.is_empty() {
        return Err(CorpusError::Invalid(format!(
            "case {} must define at least one scorer",
            case.id
        )));
    }
    for scorer in &case.scorers {
        validate_scorer(&case.id, scorer)?;
    }
    Ok(())
}

fn validate_scorer(case_id: &str, scorer: &ScorerSpec) -> Result<(), CorpusError> {
    match scorer {
        ScorerSpec::AssistantContains { marker, minimum_hits } => {
            if marker.is_empty() {
                return Err(CorpusError::Invalid(format!(
                    "case {case_id} assistant_contains marker must not be empty"
                )));
            }
            if *minimum_hits == 0 {
                return Err(CorpusError::Invalid(format!(
                    "case {case_id} assistant_contains minimum_hits must be positive"
                )));
            }
        }
        ScorerSpec::AssistantNotContains { marker } => {
            if marker.is_empty() {
                return Err(CorpusError::Invalid(format!(
                    "case {case_id} assistant_not_contains marker must not be empty"
                )));
            }
        }
        ScorerSpec::ToolCalled { name } => {
            if name.trim().is_empty() {
                return Err(CorpusError::Invalid(format!(
                    "case {case_id} tool_called name must not be empty"
                )));
            }
        }
        ScorerSpec::MaxToolCalls { .. } => {}
        ScorerSpec::MaxTurns { max } => {
            if *max == 0 {
                return Err(CorpusError::Invalid(format!(
                    "case {case_id} max_turns must be positive"
                )));
            }
        }
        ScorerSpec::RegexMatch { pattern, minimum_hits } => {
            if pattern.is_empty() {
                return Err(CorpusError::Invalid(format!(
                    "case {case_id} regex_match pattern must not be empty"
                )));
            }
            if *minimum_hits == 0 {
                return Err(CorpusError::Invalid(format!(
                    "case {case_id} regex_match minimum_hits must be positive"
                )));
            }
            regex::Regex::new(pattern).map_err(|e| {
                CorpusError::Invalid(format!("case {case_id} regex_match pattern invalid: {e}"))
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CaseBudgets;

    fn sample_case() -> Case {
        Case {
            id: "hello-ack".into(),
            category: "basic_chat".into(),
            prompt: "Reply with exactly: HELLO_OK".into(),
            enabled: true,
            budgets: CaseBudgets {
                max_turns: Some(3),
                max_tokens: Some(4096),
            },
            scorers: vec![ScorerSpec::AssistantContains {
                marker: "HELLO_OK".into(),
                minimum_hits: 1,
            }],
            notes: None,
        }
    }

    #[test]
    fn rejects_duplicate_ids() {
        let manifest = Manifest {
            schema_version: SCHEMA_VERSION,
            corpus_version: "v1".into(),
            suite: "session_dialogue".into(),
            cases: vec![sample_case(), sample_case()],
        };
        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn rejects_empty_scorers() {
        let mut case = sample_case();
        case.scorers.clear();
        let manifest = Manifest {
            schema_version: SCHEMA_VERSION,
            corpus_version: "v1".into(),
            suite: "session_dialogue".into(),
            cases: vec![case],
        };
        assert!(validate_manifest(&manifest).is_err());
    }
}

//! Artifact data-source provenance verification (`最终方案.md` §6.8).
//!
//! Policy:
//! - Missing provenance or `model_recall` emits an advisory warning tag only
//!   and NEVER blocks delivery (`ok` stays `true`).
//! - Fabricated or unverified `user_file` provenance (missing path, path
//!   escaping the session workspace, nonexistent/unreadable file, or SHA-256
//!   mismatch against the actual file bytes) is a HARD BLOCK
//!   (`PROVENANCE_INVALID`, `ok = false`, `next_action = "clarify_or_rebuild"`).

use std::path::Path;

use serde::Deserialize;

use crate::delivery;

use super::VerifyIssue;
use super::ooxml::push_unique;

/// Hard-block issue code for fabricated, escaping, missing, or digest-mismatched
/// `user_file` provenance claims.
pub const PROVENANCE_INVALID: &str = "PROVENANCE_INVALID";

/// Origin category for one claimed data point inside a deliverable (`§6.8`).
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceKind {
    UserFile,
    Fetched,
    Browser,
    ModelRecall,
}

/// One optional provenance declaration submitted to `verify_deck` /
/// `verify_artifact`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProvenanceClaim {
    pub field: String,
    pub kind: ProvenanceKind,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

/// Outcome of validating provenance claims for one artifact.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProvenanceEvaluation {
    /// Hard-blocking issues (only populated for invalid `user_file` claims).
    pub issues: Vec<VerifyIssue>,
    /// Advisory tags (e.g. `provenance_missing`, `model_recall:<field>`).
    pub warnings: Vec<String>,
}

/// Maximum provenance claims evaluated per verification call.
const MAX_PROVENANCE_CLAIMS: usize = 64;

/// Normalize an optional case-insensitive `"sha256:"` prefix and validate that
/// the remaining digest consists of exactly 64 ASCII hex digits.
fn normalize_sha256_digest(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    let digest = if trimmed
        .as_bytes()
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"sha256:"))
    {
        &trimmed[7..]
    } else {
        trimmed
    };
    (digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()))
        .then_some(digest)
}

/// Evaluate provenance claims against the workspace filesystem.
#[cfg(test)]
#[must_use]
pub fn evaluate_provenance(
    claims: &[ProvenanceClaim],
    cwd: Option<&Path>,
) -> ProvenanceEvaluation {
    evaluate_provenance_with_signal(claims, cwd, None)
}

pub(super) fn evaluate_provenance_with_signal(
    claims: &[ProvenanceClaim],
    cwd: Option<&Path>,
    signal: Option<&flowy_agent_core::CancelToken>,
) -> ProvenanceEvaluation {
    let mut eval = ProvenanceEvaluation::default();
    if claims.is_empty() {
        eval.warnings.push("provenance_missing".to_owned());
        return eval;
    }
    let mut hash_cache: std::collections::HashMap<
        std::path::PathBuf,
        Option<String>,
    > = std::collections::HashMap::new();
    for claim in claims.iter().take(MAX_PROVENANCE_CLAIMS) {
        if signal.is_some_and(flowy_agent_core::CancelToken::is_cancelled) {
            break;
        }
        let field = claim.field.trim();
        let field_label = if field.is_empty() {
            "provenance"
        } else {
            field
        };
        match claim.kind {
            ProvenanceKind::ModelRecall => {
                eval.warnings.push(format!("model_recall:{field_label}"));
            }
            ProvenanceKind::Fetched => {
                let has_ref =
                    claim.path.as_deref().is_some_and(|p| !p.trim().is_empty())
                        || claim
                            .note
                            .as_deref()
                            .is_some_and(|n| !n.trim().is_empty());
                if !has_ref {
                    eval.warnings
                        .push(format!("unattributed_fetched:{field_label}"));
                }
            }
            ProvenanceKind::Browser => {
                let has_ref =
                    claim.path.as_deref().is_some_and(|p| !p.trim().is_empty())
                        || claim
                            .note
                            .as_deref()
                            .is_some_and(|n| !n.trim().is_empty());
                if !has_ref {
                    eval.warnings
                        .push(format!("unattributed_browser:{field_label}"));
                }
            }
            ProvenanceKind::UserFile => {
                let Some(raw_path) = claim
                    .path
                    .as_deref()
                    .map(str::trim)
                    .filter(|raw| !raw.is_empty())
                else {
                    push_unique(
                        &mut eval.issues,
                        PROVENANCE_INVALID,
                        field_label,
                    );
                    continue;
                };
                // Fail-closed: require a known workspace root so `user_file`
                // provenance can never probe arbitrary host paths.
                let Some(root) = cwd else {
                    push_unique(
                        &mut eval.issues,
                        PROVENANCE_INVALID,
                        field_label,
                    );
                    continue;
                };
                if !crate::handle::confined_to(root, raw_path) {
                    push_unique(
                        &mut eval.issues,
                        PROVENANCE_INVALID,
                        field_label,
                    );
                    continue;
                }
                let candidate = {
                    let path = Path::new(raw_path);
                    if path.is_absolute() {
                        path.to_path_buf()
                    } else {
                        root.join(path)
                    }
                };
                if let Some(raw_hash) = claim
                    .sha256
                    .as_deref()
                    .map(str::trim)
                    .filter(|hash| !hash.is_empty())
                {
                    let Some(expected_hash) = normalize_sha256_digest(raw_hash)
                    else {
                        push_unique(
                            &mut eval.issues,
                            PROVENANCE_INVALID,
                            field_label,
                        );
                        continue;
                    };
                    let cache_key = std::fs::canonicalize(&candidate)
                        .unwrap_or_else(|_| candidate.clone());
                    let actual = hash_cache
                        .entry(cache_key)
                        .or_insert_with(|| {
                            delivery::hash_regular_file_streaming(
                                &candidate,
                                crate::MAX_ARTIFACT_BYTES,
                                signal,
                            )
                            .ok()
                        })
                        .as_deref();
                    let Some(actual_hash) = actual else {
                        push_unique(
                            &mut eval.issues,
                            PROVENANCE_INVALID,
                            field_label,
                        );
                        continue;
                    };
                    if !actual_hash.eq_ignore_ascii_case(expected_hash) {
                        push_unique(
                            &mut eval.issues,
                            PROVENANCE_INVALID,
                            field_label,
                        );
                    }
                } else if delivery::verify_regular_file_exists(&candidate)
                    .is_err()
                {
                    push_unique(
                        &mut eval.issues,
                        PROVENANCE_INVALID,
                        field_label,
                    );
                }
            }
        }
    }
    eval
}

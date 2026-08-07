//! Deterministic scorers over [`TurnTranscript`].

use regex::Regex;

use crate::types::{ScorerResult, ScorerSpec, TurnTranscript};

/// Apply every scorer; overall success is the conjunction of all results.
pub fn score_all(specs: &[ScorerSpec], transcript: &TurnTranscript) -> (bool, Vec<ScorerResult>) {
    let results: Vec<ScorerResult> = specs.iter().map(|s| score_one(s, transcript)).collect();
    let success = results.iter().all(|r| r.passed);
    (success, results)
}

pub fn score_one(spec: &ScorerSpec, transcript: &TurnTranscript) -> ScorerResult {
    match spec {
        ScorerSpec::AssistantContains {
            marker,
            minimum_hits,
        } => {
            let hits = count_contains(&transcript.assistant_text, marker);
            ScorerResult {
                scorer_type: "assistant_contains".into(),
                passed: hits >= *minimum_hits,
                detail: Some(format!("hits={hits} minimum={minimum_hits} marker={marker}")),
            }
        }
        ScorerSpec::AssistantNotContains { marker } => {
            let hits = count_contains(&transcript.assistant_text, marker);
            ScorerResult {
                scorer_type: "assistant_not_contains".into(),
                passed: hits == 0,
                detail: Some(format!("hits={hits} marker={marker}")),
            }
        }
        ScorerSpec::ToolCalled { name } => {
            let called = transcript.tool_names.iter().any(|n| n == name);
            ScorerResult {
                scorer_type: "tool_called".into(),
                passed: called,
                detail: Some(format!("name={name} called={called}")),
            }
        }
        ScorerSpec::MaxToolCalls { max } => {
            let count = transcript.tool_names.len() as u32;
            ScorerResult {
                scorer_type: "max_tool_calls".into(),
                passed: count <= *max,
                detail: Some(format!("count={count} max={max}")),
            }
        }
        ScorerSpec::MaxTurns { max } => ScorerResult {
            scorer_type: "max_turns".into(),
            passed: transcript.turns <= *max,
            detail: Some(format!("turns={} max={max}", transcript.turns)),
        },
        ScorerSpec::RegexMatch {
            pattern,
            minimum_hits,
        } => match Regex::new(pattern) {
            Ok(re) => {
                let hits = re.find_iter(&transcript.assistant_text).count();
                ScorerResult {
                    scorer_type: "regex_match".into(),
                    passed: hits >= *minimum_hits,
                    detail: Some(format!("hits={hits} minimum={minimum_hits}")),
                }
            }
            Err(e) => ScorerResult {
                scorer_type: "regex_match".into(),
                passed: false,
                detail: Some(format!("invalid pattern: {e}")),
            },
        },
    }
}

fn count_contains(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    haystack.matches(needle).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assistant_contains_and_not_contains() {
        let t = TurnTranscript {
            assistant_text: "hello HELLO_OK world".into(),
            tool_names: vec![],
            turns: 1,
        };
        let pass = score_one(
            &ScorerSpec::AssistantContains {
                marker: "HELLO_OK".into(),
                minimum_hits: 1,
            },
            &t,
        );
        assert!(pass.passed);
        let fail = score_one(
            &ScorerSpec::AssistantNotContains {
                marker: "HELLO_OK".into(),
            },
            &t,
        );
        assert!(!fail.passed);
    }

    #[test]
    fn tool_and_turn_budgets() {
        let t = TurnTranscript {
            assistant_text: "DONE".into(),
            tool_names: vec!["echo".into(), "echo".into()],
            turns: 3,
        };
        assert!(
            score_one(
                &ScorerSpec::ToolCalled {
                    name: "echo".into()
                },
                &t
            )
            .passed
        );
        assert!(
            !score_one(&ScorerSpec::MaxToolCalls { max: 1 }, &t).passed
        );
        assert!(score_one(&ScorerSpec::MaxTurns { max: 3 }, &t).passed);
        assert!(!score_one(&ScorerSpec::MaxTurns { max: 2 }, &t).passed);
    }
}

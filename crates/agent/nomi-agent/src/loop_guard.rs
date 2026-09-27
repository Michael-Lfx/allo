//! Loop-stagnation guard: detects either an identical completed tool outcome or
//! consecutive turns where every tool call failed. The guard first injects a
//! corrective nudge, then aborts if the no-progress cycle continues. Successful
//! polling is excluded before the guard observes the exact-outcome signal, so
//! legitimate external waiting remains unaffected while failed polling and
//! alternating failures stay bounded.
//!
//! This guard does **not** catch serial recon of *different* successful tools
//! (Read A, then Read B, then `git status`). Each outcome signature is unique,
//! so the cost is a provider round-trip per file rather than a repeated call.
//! Coding mode owns that policy in [`nomi_coding::CodingProgressGuard`]
//! (consecutive tour, request-lifetime recon, 1-tool round-trip tax).

use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
};

use nomi_types::message::ContentBlock;

/// Stable structural hash of a JSON value for loop signatures.
///
/// Provider/object construction order is not semantic, so object keys are visited
/// in sorted order; array order remains significant. Every node is framed with a
/// type tag and a collection length, so distinct shapes cannot alias — the
/// canonical-JSON string this replaced could not tell `["a","b"]` from
/// `["a,b"]`, nor the number `1` from the string `"1"`.
///
/// It also removes the allocation: the previous encoder rebuilt a canonical
/// `String`, with a `Vec` and a sort at every object node, twice per turn.
fn hash_json(value: &serde_json::Value, hasher: &mut impl Hasher) {
    match value {
        serde_json::Value::Null => 0u8.hash(hasher),
        serde_json::Value::Bool(flag) => {
            1u8.hash(hasher);
            flag.hash(hasher);
        }
        serde_json::Value::Number(number) => {
            2u8.hash(hasher);
            // Hashing the numeric value directly keeps equal numbers equivalent and
            // avoids a `to_string` allocation per number.
            if let Some(integer) = number.as_i64() {
                0u8.hash(hasher);
                integer.hash(hasher);
            } else if let Some(unsigned) = number.as_u64() {
                1u8.hash(hasher);
                unsigned.hash(hasher);
            } else if let Some(float) = number.as_f64() {
                2u8.hash(hasher);
                float.to_bits().hash(hasher);
            }
        }
        serde_json::Value::String(text) => {
            3u8.hash(hasher);
            text.hash(hasher);
        }
        serde_json::Value::Array(items) => {
            4u8.hash(hasher);
            items.len().hash(hasher);
            for item in items {
                hash_json(item, hasher);
            }
        }
        serde_json::Value::Object(map) => {
            5u8.hash(hasher);
            map.len().hash(hasher);
            // `serde_json::Map` is a `BTreeMap` by default, but a feature enabled
            // anywhere in the dependency graph can switch it to an insertion-ordered
            // map, so normalise the order here rather than assume it.
            let mut entries: Vec<(&String, &serde_json::Value)> = map.iter().collect();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            for (key, value) in entries {
                key.hash(hasher);
                hash_json(value, hasher);
            }
        }
    }
}

/// Structural signature of one call: its tool name and its arguments.
fn call_signature(name: &str, input: &serde_json::Value) -> u64 {
    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
    hash_json(input, &mut hasher);
    hasher.finish()
}

/// Fold an order-normalised list of per-call signatures into one turn signature.
///
/// `slice::hash` writes the length before the elements, so multiplicity and count
/// are part of the result.
fn fold_signatures(signatures: &[u64]) -> u64 {
    let mut hasher = DefaultHasher::new();
    signatures.hash(&mut hasher);
    hasher.finish()
}

/// Canonical signature of a turn's tool calls: each call's name plus a structural
/// hash of its input, sorted to be order-independent while preserving duplicate
/// calls. The tool `id` is deliberately excluded — it changes every turn, but two
/// turns that issue the same logical call(s) with the same arguments must collide.
/// Returns `None` when there are no tool calls (a text-only turn never stagnates).
pub fn tool_calls_signature(tool_calls: &[ContentBlock]) -> Option<u64> {
    let mut signatures: Vec<u64> = tool_calls
        .iter()
        .filter_map(|call| match call {
            ContentBlock::ToolUse { name, input, .. } => Some(call_signature(name, input)),
            _ => None,
        })
        .collect();
    if signatures.is_empty() {
        None
    } else {
        signatures.sort_unstable();
        Some(fold_signatures(&signatures))
    }
}

/// Signature of a completed tool turn. IDs pair each result to its logical call
/// but are excluded from the final signature because providers generate fresh
/// IDs every turn. Result payloads (including images) are hashed so large
/// screenshots are not retained in guard state.
pub fn tool_outcome_signature(
    tool_calls: &[ContentBlock],
    tool_results: &[ContentBlock],
) -> Option<u64> {
    tool_outcome_signature_filtered(tool_calls, tool_results, |_, _, _| true)
}

/// Build an outcome signature while excluding invocations whose unchanged
/// results represent normal external waiting (for example an empty
/// `write_stdin` poll). Results are paired to tracked calls by tool-use ID so a
/// mixed turn cannot hide a repeated non-polling action behind a polling call.
pub fn tool_outcome_signature_filtered<F>(
    tool_calls: &[ContentBlock],
    tool_results: &[ContentBlock],
    mut should_track: F,
) -> Option<u64>
where
    F: FnMut(&str, &str, &serde_json::Value) -> bool,
{
    let tracked_calls: Vec<(&str, u64)> = tool_calls
        .iter()
        .filter_map(|call| match call {
            ContentBlock::ToolUse {
                id, name, input, ..
            } if should_track(id, name, input) => Some((id.as_str(), call_signature(name, input))),
            _ => None,
        })
        .collect();
    if tracked_calls.is_empty() {
        return None;
    }

    let mut result_hashes_by_id: HashMap<&str, Vec<u64>> = HashMap::new();
    for block in tool_results {
        if let ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
            images,
        } = block
        {
            let mut hasher = DefaultHasher::new();
            is_error.hash(&mut hasher);
            content.hash(&mut hasher);
            for image in images {
                image.media_type.hash(&mut hasher);
                image.data.hash(&mut hasher);
            }
            result_hashes_by_id
                .entry(tool_use_id.as_str())
                .or_default()
                .push(hasher.finish());
        }
    }
    // Several results for one id must collide regardless of arrival order.
    for hashes in result_hashes_by_id.values_mut() {
        hashes.sort_unstable();
    }

    let mut paired_signatures: Vec<u64> = tracked_calls
        .into_iter()
        .map(|(id, call)| {
            let mut hasher = DefaultHasher::new();
            call.hash(&mut hasher);
            match result_hashes_by_id.get(id) {
                Some(hashes) => {
                    true.hash(&mut hasher);
                    hashes.hash(&mut hasher);
                }
                // Distinguishes "no result observed" from "results hashed".
                None => false.hash(&mut hasher),
            }
            hasher.finish()
        })
        .collect();
    paired_signatures.sort_unstable();
    Some(fold_signatures(&paired_signatures))
}

/// Whether a completed tool turn contained at least one result and every result
/// was an error. Mixed success/error turns are progress for the consecutive-
/// failure guard and therefore return `false`.
pub(crate) fn all_tool_results_failed(tool_results: &[ContentBlock]) -> bool {
    let mut saw_result = false;
    for block in tool_results {
        if let ContentBlock::ToolResult { is_error, .. } = block {
            saw_result = true;
            if !*is_error {
                return false;
            }
        }
    }
    saw_result
}

/// The guidance injected when stagnation is detected.
pub const STAGNATION_NUDGE: &str = "Loop guard: recent tool turns are making no progress: either \
the same call(s) keep returning the same outcome, or every tool call has failed repeatedly. Stop \
repeating the same action. Either try a materially different approach (different arguments, a \
different tool, or a different sub-problem), or stop and report what you have found and what is \
blocking you.";

/// Terminal diagnostic persisted in the transcript when the model ignores the
/// corrective nudge and continues making no progress.
pub const STAGNATION_ABORT: &str = "Stopped: tool turns continued making no progress after a \
loop-guard warning. No further automatic retries were attempted.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StagnationAction {
    Continue,
    Nudge,
    Abort,
}

/// Tracks consecutive identical outcomes (compared as structural hashes) and
/// consecutive all-failed turns.
pub struct StagnationGuard {
    nudge_threshold: usize,
    abort_threshold: usize,
    last: Option<u64>,
    repeats: usize,
    consecutive_failures: usize,
}

impl StagnationGuard {
    /// `nudge_threshold` is the number of identical outcomes that triggers the
    /// corrective message. The same cycle is aborted after one additional full
    /// threshold, giving the model a bounded opportunity to recover.
    pub fn new(nudge_threshold: usize) -> Self {
        let nudge_threshold = nudge_threshold.max(2);
        Self {
            nudge_threshold,
            abort_threshold: nudge_threshold.saturating_mul(2),
            last: None,
            repeats: 0,
            consecutive_failures: 0,
        }
    }

    /// Start a fresh progress window, for example after a new user instruction.
    pub fn reset(&mut self) {
        self.last = None;
        self.repeats = 0;
        self.consecutive_failures = 0;
    }

    /// Observe this turn's completed tool outcome. A `None` signature or a
    /// changed call/result breaks the identical-outcome streak. Separately,
    /// `all_failed` tracks consecutive turns where every call failed, so
    /// alternating errors cannot evade the guard. Any successful result must
    /// be represented by `all_failed == false`.
    pub fn observe(
        &mut self,
        signature: Option<u64>,
        all_failed: bool,
    ) -> StagnationAction {
        let repeated_outcome_action = match signature {
            Some(sig) => {
                if self.last == Some(sig) {
                    self.repeats += 1;
                } else {
                    self.last = Some(sig);
                    self.repeats = 1;
                }
                self.action_for(self.repeats)
            }
            None => {
                self.last = None;
                self.repeats = 0;
                StagnationAction::Continue
            }
        };

        let failed_outcome_action = if all_failed {
            self.consecutive_failures += 1;
            self.action_for(self.consecutive_failures)
        } else {
            self.consecutive_failures = 0;
            StagnationAction::Continue
        };

        if all_failed {
            // The all-failed counter is the stronger signal for this turn and
            // owns its single nudge/abort schedule. This avoids injecting a
            // second nudge if the exact-signature counter reaches its own
            // threshold later in the same failure streak.
            failed_outcome_action
        } else {
            repeated_outcome_action
        }
    }

    fn action_for(&self, count: usize) -> StagnationAction {
        if count >= self.abort_threshold {
            StagnationAction::Abort
        } else if count == self.nudge_threshold {
            StagnationAction::Nudge
        } else {
            StagnationAction::Continue
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool_use(name: &str, input: serde_json::Value, id: &str) -> ContentBlock {
        ContentBlock::ToolUse {
            id: id.to_string(),
            name: name.to_string(),
            input,
            extra: None,
        }
    }

    #[test]
    fn signature_ignores_id_and_order_but_not_args() {
        let a = vec![tool_use("Bash", json!({"command": "ls"}), "id-1")];
        let b = vec![tool_use("Bash", json!({"command": "ls"}), "id-2-different")];
        assert_eq!(tool_calls_signature(&a), tool_calls_signature(&b), "id must not affect signature");

        let two_ab = vec![
            tool_use("Read", json!({"p": "a"}), "1"),
            tool_use("Grep", json!({"q": "x"}), "2"),
        ];
        let two_ba = vec![
            tool_use("Grep", json!({"q": "x"}), "3"),
            tool_use("Read", json!({"p": "a"}), "4"),
        ];
        assert_eq!(tool_calls_signature(&two_ab), tool_calls_signature(&two_ba), "order must not matter");

        let diff_args = vec![tool_use("Bash", json!({"command": "pwd"}), "5")];
        assert_ne!(tool_calls_signature(&a), tool_calls_signature(&diff_args), "different args must differ");

        let duplicate = vec![
            tool_use("Bash", json!({"command": "ls"}), "6"),
            tool_use("Bash", json!({"command": "ls"}), "7"),
        ];
        assert_ne!(
            tool_calls_signature(&a),
            tool_calls_signature(&duplicate),
            "call multiplicity must affect the signature"
        );
    }

    #[test]
    fn signature_canonicalizes_object_keys_recursively_but_preserves_array_order() {
        let left_input: serde_json::Value = serde_json::from_str(
            r#"{"outer":{"b":2,"a":1},"items":[{"y":2,"x":1},3]}"#,
        )
        .unwrap();
        let reordered_input: serde_json::Value = serde_json::from_str(
            r#"{"items":[{"x":1,"y":2},3],"outer":{"a":1,"b":2}}"#,
        )
        .unwrap();
        let reordered_array: serde_json::Value = serde_json::from_str(
            r#"{"outer":{"a":1,"b":2},"items":[3,{"x":1,"y":2}]}"#,
        )
        .unwrap();

        let left = vec![tool_use("Read", left_input, "left")];
        let same = vec![tool_use("Read", reordered_input, "right")];
        let different = vec![tool_use("Read", reordered_array, "array")];

        assert_eq!(tool_calls_signature(&left), tool_calls_signature(&same));
        assert_ne!(
            tool_calls_signature(&left),
            tool_calls_signature(&different),
            "array order remains semantically significant"
        );
    }

    #[test]
    fn text_only_turn_has_no_signature() {
        let blocks = vec![ContentBlock::Text { text: "hello".into() }];
        assert_eq!(tool_calls_signature(&blocks), None);
        assert_eq!(tool_calls_signature(&[]), None);
    }

    #[test]
    fn all_failed_requires_results_and_rejects_any_success() {
        let result = |id: &str, is_error: bool| ContentBlock::ToolResult {
            tool_use_id: id.to_string(),
            content: String::new(),
            is_error,
            images: Vec::new(),
        };

        assert!(!all_tool_results_failed(&[]));
        assert!(!all_tool_results_failed(&[ContentBlock::Text {
            text: "progress".into(),
        }]));
        assert!(all_tool_results_failed(&[
            result("a", true),
            result("b", true),
        ]));
        assert!(!all_tool_results_failed(&[
            result("a", true),
            result("b", false),
        ]));
    }

    #[test]
    fn nudges_then_aborts_consecutive_identical_outcomes() {
        let mut guard = StagnationGuard::new(3);
        let sig = Some(11u64);
        assert_eq!(guard.observe(sig, false), StagnationAction::Continue);
        assert_eq!(guard.observe(sig, false), StagnationAction::Continue);
        assert_eq!(guard.observe(sig, false), StagnationAction::Nudge);
        assert_eq!(guard.observe(sig, false), StagnationAction::Continue);
        assert_eq!(guard.observe(sig, false), StagnationAction::Continue);
        assert_eq!(guard.observe(sig, false), StagnationAction::Abort);
    }

    #[test]
    fn alternating_all_failed_outcomes_nudge_then_abort() {
        let mut guard = StagnationGuard::new(3);
        let a = Some(1u64);
        let b = Some(2u64);

        let actions = [a, b, a, b, a, b]
            .into_iter()
            .map(|signature| guard.observe(signature, true))
            .collect::<Vec<_>>();

        assert_eq!(actions[2], StagnationAction::Nudge);
        assert_eq!(actions[5], StagnationAction::Abort);
    }

    #[test]
    fn a_successful_outcome_resets_the_all_failed_streak() {
        let mut guard = StagnationGuard::new(3);
        let a = Some(1u64);
        let b = Some(2u64);
        let success = Some(3u64);

        assert_eq!(guard.observe(a, true), StagnationAction::Continue);
        assert_eq!(guard.observe(b, true), StagnationAction::Continue);
        // `false` represents a turn with at least one successful result. The
        // exact-outcome guard remains independent and still catches unchanged
        // successful non-polling cycles.
        assert_eq!(guard.observe(success, false), StagnationAction::Continue);
        assert_eq!(guard.observe(a, true), StagnationAction::Continue);
        assert_eq!(guard.observe(b, true), StagnationAction::Continue);
        assert_eq!(guard.observe(a, true), StagnationAction::Nudge);
    }

    #[test]
    fn successful_explicit_polling_never_advances_either_streak() {
        let mut guard = StagnationGuard::new(3);
        for _ in 0..12 {
            assert_eq!(guard.observe(None, false), StagnationAction::Continue);
        }
    }

    #[test]
    fn a_different_turn_breaks_the_streak() {
        let mut guard = StagnationGuard::new(3);
        let a = Some(1u64);
        let b = Some(2u64);
        guard.observe(a, false);
        guard.observe(a, false);
        guard.observe(b, false); // breaks the streak
        assert_eq!(guard.observe(a, false), StagnationAction::Continue);
        assert_eq!(guard.observe(a, false), StagnationAction::Continue);
        assert_eq!(guard.observe(a, false), StagnationAction::Nudge);
    }

    #[test]
    fn text_turn_between_identical_calls_breaks_streak() {
        let mut guard = StagnationGuard::new(3);
        let a = Some(1u64);
        guard.observe(a, false);
        guard.observe(a, false);
        assert_eq!(guard.observe(None, false), StagnationAction::Continue);
        assert_eq!(guard.observe(a, false), StagnationAction::Continue);
        assert_eq!(guard.observe(a, false), StagnationAction::Continue);
        assert_eq!(guard.observe(a, false), StagnationAction::Nudge);
    }

    #[test]
    fn changing_tool_result_breaks_the_streak() {
        let first_calls = vec![tool_use("status", json!({}), "call-1")];
        let second_calls = vec![tool_use("status", json!({}), "call-2")];
        let result = |content: &str, id: &str| {
            vec![ContentBlock::ToolResult {
                tool_use_id: id.to_string(),
                content: content.to_string(),
                is_error: false,
                images: Vec::new(),
            }]
        };
        let first = tool_outcome_signature(&first_calls, &result("pending", "call-1"));
        let same_with_new_ids =
            tool_outcome_signature(&second_calls, &result("pending", "call-2"));
        assert_eq!(first, same_with_new_ids, "paired IDs must not affect the signature");

        let second = tool_outcome_signature(&second_calls, &result("complete", "call-2"));
        assert_ne!(first, second);

        let mut guard = StagnationGuard::new(3);
        assert_eq!(guard.observe(first, false), StagnationAction::Continue);
        assert_eq!(guard.observe(first, false), StagnationAction::Continue);
        assert_eq!(guard.observe(second, false), StagnationAction::Continue);
    }

    #[test]
    fn polling_calls_are_excluded_without_masking_other_calls() {
        let calls = vec![
            tool_use("write_stdin", json!({"session_id": 7}), "poll"),
            tool_use("update", json!({"id": 1}), "mutation"),
        ];
        let results = vec![
            ContentBlock::ToolResult {
                tool_use_id: "poll".into(),
                content: "still running".into(),
                is_error: false,
                images: Vec::new(),
            },
            ContentBlock::ToolResult {
                tool_use_id: "mutation".into(),
                content: "updated".into(),
                is_error: false,
                images: Vec::new(),
            },
        ];

        let mixed =
            tool_outcome_signature_filtered(&calls, &results, |_, name, _| name != "write_stdin");
        assert!(
            mixed.is_some(),
            "the non-polling mutation remains tracked"
        );
        // Excluding the poll must leave exactly the mutation's outcome: dropping an
        // untracked call cannot change the signature of the tracked ones.
        let mutation_only =
            tool_outcome_signature_filtered(&calls[1..], &results[1..], |_, name, _| {
                name != "write_stdin"
            });
        assert_eq!(mixed, mutation_only);

        let poll_only =
            tool_outcome_signature_filtered(&calls[..1], &results[..1], |_, name, _| {
                name != "write_stdin"
            });
        assert_eq!(poll_only, None);
    }

    #[test]
    fn successful_non_polling_cycles_nudge_then_abort() {
        let mut guard = StagnationGuard::new(3);
        let sig = Some(1u64);
        let actions: Vec<StagnationAction> = (0..6).map(|_| guard.observe(sig, false)).collect();
        assert_eq!(actions[2], StagnationAction::Nudge);
        assert_eq!(actions[5], StagnationAction::Abort);
    }

    #[test]
    fn swapped_results_are_not_the_same_outcome() {
        let calls = vec![
            tool_use("first", json!({}), "call-a"),
            tool_use("second", json!({}), "call-b"),
        ];
        let result = |id: &str, content: &str| ContentBlock::ToolResult {
            tool_use_id: id.to_string(),
            content: content.to_string(),
            is_error: false,
            images: Vec::new(),
        };
        let normal = vec![result("call-a", "A"), result("call-b", "B")];
        let swapped = vec![result("call-a", "B"), result("call-b", "A")];
        assert_ne!(
            tool_outcome_signature(&calls, &normal),
            tool_outcome_signature(&calls, &swapped)
        );
    }

    /// The JSON-string join this replaced could not tell `["a","b"]` from
    /// `["a,b"]`, nor the number `1` from the string `"1"`. A false collision
    /// silently suppresses a real loop, so type and length framing are load-bearing.
    #[test]
    fn signature_frames_types_and_lengths() {
        let signature = |input: serde_json::Value| {
            tool_calls_signature(&[tool_use("Read", input, "call")])
        };

        assert_ne!(
            signature(json!({"parts": ["a", "b"]})),
            signature(json!({"parts": ["a,b"]})),
            "adjacent array items must not alias a single joined string"
        );
        assert_ne!(
            signature(json!({"value": 1})),
            signature(json!({"value": "1"})),
            "a number and its decimal string must not alias"
        );
    }

    /// Providers build the same arguments with different key order. A structural
    /// hash must be blind to that, exactly as the canonical encoder was.
    #[test]
    fn signature_is_stable_across_object_key_order() {
        let left: serde_json::Value =
            serde_json::from_str(r#"{"b":2,"a":{"d":4,"c":3}}"#).unwrap();
        let right: serde_json::Value =
            serde_json::from_str(r#"{"a":{"c":3,"d":4},"b":2}"#).unwrap();

        assert_eq!(
            tool_calls_signature(&[tool_use("Read", left, "first")]),
            tool_calls_signature(&[tool_use("Read", right, "second")])
        );
    }
}

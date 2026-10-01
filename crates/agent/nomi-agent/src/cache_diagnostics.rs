//! Prompt cache break detection.
//!
//! Pairs request-side prompt state (hashes) with response-side cache tokens
//! to detect and diagnose prompt cache breaks across turns.

use std::hash::{DefaultHasher, Hash, Hasher};

use nomi_types::message::Message;
use nomi_types::tool::ToolDef;

struct HasherWriter<'a, H: Hasher>(&'a mut H);

impl<H: Hasher> std::io::Write for HasherWriter<'_, H> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Snapshot of prompt state taken before each API call.
#[derive(Debug, Clone)]
struct PromptSnapshot {
    system_hash: u64,
    tools_hash: u64,
    /// Per-message hashes of the transcript that was about to be sent.
    ///
    /// Append-only growth keeps the previous hashes as a prefix. A snip or
    /// microcompact rewrite changes an earlier hash (or shortens the list),
    /// which is a different cache break from a server-side TTL expiry.
    message_hashes: Vec<u64>,
}

/// Cache token statistics from a single API response.
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub input_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
}

/// Prompt-cache reuse measured over provider rounds.
///
/// The first round after a cold start can never hit the cache, so the `warm_*`
/// sums leave it out of the denominator. `last_round_*` is the most recent
/// round regardless.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheReuseStats {
    pub last_round_input_tokens: u64,
    pub last_round_cache_read_tokens: u64,
    pub warm_input_tokens: u64,
    pub warm_cache_read_tokens: u64,
}

/// Diagnostic result after comparing two consecutive turns.
#[derive(Debug, Clone)]
pub enum CacheDiagnostic {
    Healthy {
        hit_rate: f64,
    },
    PartialMiss {
        hit_rate: f64,
        cause: CacheBreakCause,
    },
    FullMiss {
        cause: CacheBreakCause,
    },
}

/// What caused a cache break.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheBreakCause {
    SystemPromptChanged,
    ToolsChanged,
    TtlExpiry,
    FirstRequest,
    /// Compaction replaced the message history — a deliberate cache-reset point.
    /// Mirrors Reasonix's `log_rewrite` change reason.
    Compaction,
    /// An earlier message was rewritten or removed (snip, microcompact, or
    /// another in-place edit). Append-only growth is not this cause.
    MessageHistoryRewritten,
}

/// Detects prompt cache breaks by comparing consecutive turns.
pub struct CacheBreakDetector {
    /// Snapshot from the PREVIOUS turn (used for attribution on cache break).
    prev_snapshot: Option<PromptSnapshot>,
    /// Snapshot from the CURRENT turn (just recorded by record_request).
    current_snapshot: Option<PromptSnapshot>,
    /// Cache stats from the previous API response.
    prev_stats: Option<CacheStats>,
    /// Set by `notify_compaction` — consumed by the next `attribute_cause` call
    /// so a cache miss right after compaction is attributed correctly instead
    /// of falling through to `TtlExpiry`.
    compaction_pending: bool,
    rounds_seen: u64,
    reuse: CacheReuseStats,
}

impl CacheBreakDetector {
    pub fn new() -> Self {
        Self {
            prev_snapshot: None,
            current_snapshot: None,
            prev_stats: None,
            compaction_pending: false,
            rounds_seen: 0,
            reuse: CacheReuseStats::default(),
        }
    }

    /// Reuse statistics, or `None` before the first provider round.
    pub fn reuse_stats(&self) -> Option<CacheReuseStats> {
        (self.rounds_seen > 0).then_some(self.reuse)
    }

    fn record_reuse(&mut self, stats: &CacheStats) {
        self.rounds_seen += 1;
        self.reuse.last_round_input_tokens = stats.input_tokens;
        self.reuse.last_round_cache_read_tokens = stats.cache_read_tokens;
        if self.rounds_seen > 1 {
            self.reuse.warm_input_tokens = self
                .reuse
                .warm_input_tokens
                .saturating_add(stats.input_tokens);
            self.reuse.warm_cache_read_tokens = self
                .reuse
                .warm_cache_read_tokens
                .saturating_add(stats.cache_read_tokens);
        }
    }

    /// Notify the detector that a compaction just happened. The next cache-miss
    /// diagnosis will be attributed to [`CacheBreakCause::Compaction`] instead
    /// of `TtlExpiry`. This mirrors Reasonix's `RewriteVersion` increment.
    pub fn notify_compaction(&mut self) {
        self.compaction_pending = true;
    }

    /// Record the prompt state before an API call.
    ///
    /// Tools are sorted by name before hashing so that a different
    /// presentation order (e.g. plan-mode filtering that keeps the same
    /// tools but shifts their positions) does not cause a false
    /// `ToolsChanged` cache-break diagnosis. Mirrors Reasonix's
    /// `normalizeToolSchemas`.
    pub fn record_request(&mut self, system: &str, tools: &[ToolDef], messages: &[Message]) {
        let mut system_hasher = DefaultHasher::new();
        system.hash(&mut system_hasher);
        let system_hash = system_hasher.finish();

        // Sort tools by name before hashing so order doesn't affect the hash.
        let mut sorted_tools: Vec<&ToolDef> = tools.iter().collect();
        sorted_tools.sort_by(|a, b| a.name.cmp(&b.name));

        let mut tools_hasher = DefaultHasher::new();
        for t in &sorted_tools {
            t.name.hash(&mut tools_hasher);
            t.description.hash(&mut tools_hasher);
            let _ = serde_json::to_writer(HasherWriter(&mut tools_hasher), &t.input_schema);
            t.deferred.hash(&mut tools_hasher);
        }
        let tools_hash = tools_hasher.finish();
        let message_hashes = messages.iter().map(hash_message).collect();

        // Rotate: current becomes prev, new snapshot becomes current
        self.prev_snapshot = self.current_snapshot.take();
        self.current_snapshot = Some(PromptSnapshot {
            system_hash,
            tools_hash,
            message_hashes,
        });
    }

    /// Check the response cache tokens against the previous turn.
    ///
    /// Returns `None` if no snapshot was recorded before the call.
    pub fn check_response(&mut self, stats: CacheStats) -> Option<CacheDiagnostic> {
        self.record_reuse(&stats);
        let current = self.current_snapshot.clone()?;
        let diagnostic = self.compute_diagnostic(&current, &stats);
        self.prev_stats = Some(stats);
        Some(diagnostic)
    }

    fn compute_diagnostic(&mut self, current: &PromptSnapshot, stats: &CacheStats) -> CacheDiagnostic {
        let Some(prev) = &self.prev_stats else {
            // First request — no previous data to compare
            return CacheDiagnostic::Healthy { hit_rate: 0.0 };
        };

        // If provider doesn't support caching (both turns have 0 cache tokens),
        // report healthy to avoid false alarms (e.g., OpenAI).
        if prev.cache_read_tokens == 0
            && prev.cache_creation_tokens == 0
            && stats.cache_read_tokens == 0
            && stats.cache_creation_tokens == 0
        {
            return CacheDiagnostic::Healthy { hit_rate: 0.0 };
        }

        let prev_had_cache = prev.cache_read_tokens > 0 || prev.cache_creation_tokens > 0;

        // Full miss: had cache before, now read tokens dropped to 0
        if prev_had_cache && stats.cache_read_tokens == 0 {
            let cause = self.attribute_cause(current);
            return CacheDiagnostic::FullMiss { cause };
        }

        // Calculate hit rate
        let hit_rate = if stats.input_tokens > 0 {
            stats.cache_read_tokens as f64 / stats.input_tokens as f64
        } else {
            0.0
        };

        // Partial miss: cache_read dropped >5% compared to previous
        if prev.cache_read_tokens > 0 {
            let drop_pct = 1.0 - (stats.cache_read_tokens as f64 / prev.cache_read_tokens as f64);
            if drop_pct > 0.05 {
                let cause = self.attribute_cause(current);
                return CacheDiagnostic::PartialMiss { hit_rate, cause };
            }
        }

        CacheDiagnostic::Healthy { hit_rate }
    }

    /// Determine what caused the cache break by comparing prev vs current snapshots.
    fn attribute_cause(&mut self, current: &PromptSnapshot) -> CacheBreakCause {
        // Compaction is the highest-priority cause: if a compaction happened
        // since the last turn, the message history was replaced and the cache
        // miss is expected.
        if self.compaction_pending {
            self.compaction_pending = false;
            return CacheBreakCause::Compaction;
        }

        let Some(prev) = &self.prev_snapshot else {
            return CacheBreakCause::FirstRequest;
        };

        if prev.system_hash != current.system_hash {
            return CacheBreakCause::SystemPromptChanged;
        }
        if prev.tools_hash != current.tools_hash {
            return CacheBreakCause::ToolsChanged;
        }
        if message_prefix_rewritten(&prev.message_hashes, &current.message_hashes) {
            return CacheBreakCause::MessageHistoryRewritten;
        }

        // Prefix hashes match but cache was lost — server-side TTL expiry.
        // A longer transcript whose earlier hashes are unchanged is an append,
        // not a rewrite, so it stays in this bucket.
        CacheBreakCause::TtlExpiry
    }
}

/// Hash the fields that hit the provider wire. Timestamps and provider round
/// ids are local metadata and must not look like a cache-prefix rewrite.
fn hash_message(message: &Message) -> u64 {
    let mut hasher = DefaultHasher::new();
    role_label(message.role).hash(&mut hasher);
    for block in &message.content {
        serde_json::to_string(block)
            .unwrap_or_default()
            .hash(&mut hasher);
    }
    hasher.finish()
}

fn role_label(role: nomi_types::message::Role) -> &'static str {
    match role {
        nomi_types::message::Role::User => "user",
        nomi_types::message::Role::Assistant => "assistant",
        nomi_types::message::Role::System => "system",
        nomi_types::message::Role::Tool => "tool",
    }
}

/// True when `current` is not `prev` plus an appended tail.
fn message_prefix_rewritten(prev: &[u64], current: &[u64]) -> bool {
    if current.len() < prev.len() {
        return true;
    }
    prev.iter()
        .zip(current.iter())
        .any(|(previous, next)| previous != next)
}

impl Default for CacheBreakDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_tools() -> Vec<ToolDef> {
        vec![ToolDef {
            name: "Read".into(),
            description: "Read a file".into(),
            input_schema: json!({"type": "object"}),
            deferred: false,
        }]
    }

    fn record(detector: &mut CacheBreakDetector, system: &str, tools: &[ToolDef]) {
        detector.record_request(system, tools, &[]);
    }

    fn text_message(text: &str) -> Message {
        Message::new(
            nomi_types::message::Role::User,
            vec![nomi_types::message::ContentBlock::Text {
                text: text.to_string(),
            }],
        )
    }

    #[test]
    fn reuse_stats_exclude_the_cold_first_round_from_the_warm_sums() {
        let mut detector = CacheBreakDetector::new();
        assert_eq!(detector.reuse_stats(), None);

        let mut round = |input: u64, read: u64| {
            record(&mut detector, "system prompt", &make_tools());
            detector.check_response(CacheStats {
                input_tokens: input,
                cache_read_tokens: read,
                cache_creation_tokens: 0,
            });
            detector.reuse_stats().unwrap()
        };

        let cold = round(10_000, 0);
        assert_eq!(cold.last_round_input_tokens, 10_000);
        assert_eq!(cold.warm_input_tokens, 0);

        round(12_000, 8_000);
        let warm = round(14_000, 8_000);
        assert_eq!(warm.last_round_input_tokens, 14_000);
        assert_eq!(warm.last_round_cache_read_tokens, 8_000);
        assert_eq!(warm.warm_input_tokens, 26_000);
        assert_eq!(warm.warm_cache_read_tokens, 16_000);
    }

    #[test]
    fn first_request_returns_healthy() {
        let mut detector = CacheBreakDetector::new();
        record(&mut detector, "system prompt", &make_tools());
        let diag = detector
            .check_response(CacheStats {
                input_tokens: 10000,
                cache_read_tokens: 0,
                cache_creation_tokens: 5000,
            })
            .unwrap();
        assert!(matches!(diag, CacheDiagnostic::Healthy { .. }));
    }

    #[test]
    fn healthy_when_cache_read_stable() {
        let mut detector = CacheBreakDetector::new();

        // Turn 1
        record(&mut detector, "prompt", &make_tools());
        detector.check_response(CacheStats {
            input_tokens: 10000,
            cache_read_tokens: 8000,
            cache_creation_tokens: 2000,
        });

        // Turn 2 — similar cache_read
        record(&mut detector, "prompt", &make_tools());
        let diag = detector
            .check_response(CacheStats {
                input_tokens: 11000,
                cache_read_tokens: 8000,
                cache_creation_tokens: 0,
            })
            .unwrap();

        assert!(matches!(diag, CacheDiagnostic::Healthy { .. }));
    }

    #[test]
    fn full_miss_when_cache_read_drops_to_zero() {
        let mut detector = CacheBreakDetector::new();

        // Turn 1 — cache established
        record(&mut detector, "prompt", &make_tools());
        detector.check_response(CacheStats {
            input_tokens: 10000,
            cache_read_tokens: 8000,
            cache_creation_tokens: 2000,
        });

        // Turn 2 — cache_read drops to 0
        record(&mut detector, "prompt", &make_tools());
        let diag = detector
            .check_response(CacheStats {
                input_tokens: 10000,
                cache_read_tokens: 0,
                cache_creation_tokens: 10000,
            })
            .unwrap();

        assert!(matches!(diag, CacheDiagnostic::FullMiss { .. }));
    }

    #[test]
    fn full_miss_system_prompt_changed() {
        let mut detector = CacheBreakDetector::new();

        // Turn 1
        record(&mut detector, "prompt v1", &make_tools());
        detector.check_response(CacheStats {
            input_tokens: 10000,
            cache_read_tokens: 8000,
            cache_creation_tokens: 2000,
        });

        // Turn 2 — different system prompt
        record(&mut detector, "prompt v2", &make_tools());
        let diag = detector
            .check_response(CacheStats {
                input_tokens: 10000,
                cache_read_tokens: 0,
                cache_creation_tokens: 10000,
            })
            .unwrap();

        match diag {
            CacheDiagnostic::FullMiss { cause } => {
                assert_eq!(cause, CacheBreakCause::SystemPromptChanged);
            }
            _ => panic!("expected FullMiss"),
        }
    }

    #[test]
    fn full_miss_tools_changed() {
        let mut detector = CacheBreakDetector::new();

        // Turn 1
        record(&mut detector, "prompt", &make_tools());
        detector.check_response(CacheStats {
            input_tokens: 10000,
            cache_read_tokens: 8000,
            cache_creation_tokens: 2000,
        });

        // Turn 2 — different tools
        let new_tools = vec![ToolDef {
            name: "Write".into(),
            description: "Write a file".into(),
            input_schema: json!({"type": "object"}),
            deferred: false,
        }];
        record(&mut detector, "prompt", &new_tools);
        let diag = detector
            .check_response(CacheStats {
                input_tokens: 10000,
                cache_read_tokens: 0,
                cache_creation_tokens: 10000,
            })
            .unwrap();

        match diag {
            CacheDiagnostic::FullMiss { cause } => {
                assert_eq!(cause, CacheBreakCause::ToolsChanged);
            }
            _ => panic!("expected FullMiss"),
        }
    }

    #[test]
    fn schema_only_change_is_attributed_to_tools() {
        let mut detector = CacheBreakDetector::new();
        record(&mut detector, "prompt", &make_tools());
        detector.check_response(CacheStats {
            input_tokens: 10000,
            cache_read_tokens: 8000,
            cache_creation_tokens: 2000,
        });

        let mut tools = make_tools();
        tools[0].input_schema = json!({"type": "object", "required": ["path"]});
        record(&mut detector, "prompt", &tools);
        let diag = detector
            .check_response(CacheStats {
                input_tokens: 10000,
                cache_read_tokens: 0,
                cache_creation_tokens: 10000,
            })
            .unwrap();

        assert!(matches!(
            diag,
            CacheDiagnostic::FullMiss {
                cause: CacheBreakCause::ToolsChanged
            }
        ));
    }

    #[test]
    fn full_miss_ttl_expiry() {
        let mut detector = CacheBreakDetector::new();

        // Turn 1
        record(&mut detector, "prompt", &make_tools());
        detector.check_response(CacheStats {
            input_tokens: 10000,
            cache_read_tokens: 8000,
            cache_creation_tokens: 2000,
        });

        // Turn 2 — same prompt and tools but cache lost (TTL expired server-side)
        record(&mut detector, "prompt", &make_tools());
        let diag = detector
            .check_response(CacheStats {
                input_tokens: 10000,
                cache_read_tokens: 0,
                cache_creation_tokens: 10000,
            })
            .unwrap();

        match diag {
            CacheDiagnostic::FullMiss { cause } => {
                assert_eq!(cause, CacheBreakCause::TtlExpiry);
            }
            _ => panic!("expected FullMiss"),
        }
    }

    #[test]
    fn partial_miss_when_cache_read_drops_significantly() {
        let mut detector = CacheBreakDetector::new();

        // Turn 1
        record(&mut detector, "prompt", &make_tools());
        detector.check_response(CacheStats {
            input_tokens: 10000,
            cache_read_tokens: 8000,
            cache_creation_tokens: 2000,
        });

        // Turn 2 — 50% drop in cache_read
        record(&mut detector, "prompt", &make_tools());
        let diag = detector
            .check_response(CacheStats {
                input_tokens: 10000,
                cache_read_tokens: 4000,
                cache_creation_tokens: 6000,
            })
            .unwrap();

        assert!(matches!(diag, CacheDiagnostic::PartialMiss { .. }));
    }

    #[test]
    fn openai_no_false_alarm() {
        // OpenAI never returns cache tokens — both turns have all zeros
        let mut detector = CacheBreakDetector::new();

        record(&mut detector, "prompt", &make_tools());
        detector.check_response(CacheStats {
            input_tokens: 10000,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
        });

        record(&mut detector, "prompt", &make_tools());
        let diag = detector
            .check_response(CacheStats {
                input_tokens: 10000,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
            })
            .unwrap();

        // Should be Healthy, not FullMiss
        assert!(matches!(diag, CacheDiagnostic::Healthy { .. }));
    }

    #[test]
    fn no_diagnostic_without_record_request() {
        let mut detector = CacheBreakDetector::new();
        let diag = detector.check_response(CacheStats {
            input_tokens: 10000,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
        });
        assert!(diag.is_none());
    }

    #[test]
    fn full_miss_after_compaction_attributed_to_compaction() {
        // After compaction, the message history is replaced — the cache miss
        // should be attributed to Compaction, not TtlExpiry.
        let mut detector = CacheBreakDetector::new();

        // Turn 1 — cache established
        record(&mut detector, "prompt", &make_tools());
        detector.check_response(CacheStats {
            input_tokens: 10000,
            cache_read_tokens: 8000,
            cache_creation_tokens: 2000,
        });

        // Compaction happens
        detector.notify_compaction();

        // Turn 2 — same prompt and tools but cache lost (compaction replaced messages)
        record(&mut detector, "prompt", &make_tools());
        let diag = detector
            .check_response(CacheStats {
                input_tokens: 10000,
                cache_read_tokens: 0,
                cache_creation_tokens: 10000,
            })
            .unwrap();

        match diag {
            CacheDiagnostic::FullMiss { cause } => {
                assert_eq!(cause, CacheBreakCause::Compaction);
            }
            _ => panic!("expected FullMiss with Compaction cause"),
        }
    }

    #[test]
    fn compaction_flag_consumed_after_one_diagnosis() {
        // The compaction flag should be consumed after one diagnosis, so a
        // subsequent cache miss is attributed correctly (not to Compaction).
        let mut detector = CacheBreakDetector::new();

        // Turn 1
        record(&mut detector, "prompt", &make_tools());
        detector.check_response(CacheStats {
            input_tokens: 10000,
            cache_read_tokens: 8000,
            cache_creation_tokens: 2000,
        });

        // Compaction + Turn 2 (cache miss attributed to Compaction)
        detector.notify_compaction();
        record(&mut detector, "prompt", &make_tools());
        detector.check_response(CacheStats {
            input_tokens: 10000,
            cache_read_tokens: 0,
            cache_creation_tokens: 10000,
        });

        // Turn 3 — same prompt and tools, cache miss again (TTL expiry)
        record(&mut detector, "prompt", &make_tools());
        let diag = detector
            .check_response(CacheStats {
                input_tokens: 10000,
                cache_read_tokens: 0,
                cache_creation_tokens: 10000,
            })
            .unwrap();

        match diag {
            CacheDiagnostic::FullMiss { cause } => {
                assert_eq!(
                    cause, CacheBreakCause::TtlExpiry,
                    "compaction flag should have been consumed — cause should be TtlExpiry"
                );
            }
            _ => panic!("expected FullMiss"),
        }
    }

    #[test]
    fn tool_order_invariance_no_false_break() {
        // Same tools in different order should NOT trigger ToolsChanged.
        // Mirrors Reasonix's normalizeToolSchemas which sorts before hashing.
        let mut detector = CacheBreakDetector::new();

        let tools_order_a = vec![
            ToolDef {
                name: "Read".into(),
                description: "Read a file".into(),
                input_schema: json!({"type": "object"}),
                deferred: false,
            },
            ToolDef {
                name: "Write".into(),
                description: "Write a file".into(),
                input_schema: json!({"type": "object"}),
                deferred: false,
            },
            ToolDef {
                name: "Bash".into(),
                description: "Run a command".into(),
                input_schema: json!({"type": "object"}),
                deferred: false,
            },
        ];

        // Same tools, reversed order
        let tools_order_b: Vec<ToolDef> = tools_order_a.iter().rev().cloned().collect();

        // Turn 1 — cache established with order A
        record(&mut detector, "prompt", &tools_order_a);
        detector.check_response(CacheStats {
            input_tokens: 10000,
            cache_read_tokens: 8000,
            cache_creation_tokens: 2000,
        });

        // Turn 2 — same prompt, same tools in different order, full cache hit
        record(&mut detector, "prompt", &tools_order_b);
        let diag = detector
            .check_response(CacheStats {
                input_tokens: 10000,
                cache_read_tokens: 10000,
                cache_creation_tokens: 0,
            })
            .unwrap();

        // Should be Healthy (full hit), NOT a ToolsChanged break
        assert!(
            matches!(diag, CacheDiagnostic::Healthy { .. }),
            "tool order change should not cause cache break: {:?}",
            diag
        );
    }

    #[test]
    fn rewritten_message_is_not_attributed_to_ttl() {
        let mut detector = CacheBreakDetector::new();
        let original = text_message("keep the API contract");
        detector.record_request("prompt", &make_tools(), &[original.clone()]);
        detector.check_response(CacheStats {
            input_tokens: 10000,
            cache_read_tokens: 8000,
            cache_creation_tokens: 2000,
        });

        let rewritten = text_message("[Tool result cleared]");
        detector.record_request("prompt", &make_tools(), &[rewritten]);
        let diag = detector
            .check_response(CacheStats {
                input_tokens: 10000,
                cache_read_tokens: 0,
                cache_creation_tokens: 10000,
            })
            .unwrap();

        match diag {
            CacheDiagnostic::FullMiss { cause } => {
                assert_eq!(cause, CacheBreakCause::MessageHistoryRewritten);
            }
            other => panic!("expected FullMiss, got {other:?}"),
        }
    }

    #[test]
    fn appended_message_stays_ttl_when_the_prefix_is_unchanged() {
        let mut detector = CacheBreakDetector::new();
        let first = text_message("keep the API contract");
        detector.record_request("prompt", &make_tools(), &[first.clone()]);
        detector.check_response(CacheStats {
            input_tokens: 10000,
            cache_read_tokens: 8000,
            cache_creation_tokens: 2000,
        });

        let second = text_message("next turn");
        detector.record_request("prompt", &make_tools(), &[first, second]);
        let diag = detector
            .check_response(CacheStats {
                input_tokens: 12000,
                cache_read_tokens: 0,
                cache_creation_tokens: 12000,
            })
            .unwrap();

        match diag {
            CacheDiagnostic::FullMiss { cause } => {
                assert_eq!(cause, CacheBreakCause::TtlExpiry);
            }
            other => panic!("expected FullMiss, got {other:?}"),
        }
    }

    #[test]
    fn shortened_history_is_a_rewrite() {
        let mut detector = CacheBreakDetector::new();
        let first = text_message("one");
        let second = text_message("two");
        detector.record_request("prompt", &make_tools(), &[first.clone(), second]);
        detector.check_response(CacheStats {
            input_tokens: 10000,
            cache_read_tokens: 8000,
            cache_creation_tokens: 2000,
        });

        detector.record_request("prompt", &make_tools(), &[first]);
        let diag = detector
            .check_response(CacheStats {
                input_tokens: 10000,
                cache_read_tokens: 0,
                cache_creation_tokens: 10000,
            })
            .unwrap();

        match diag {
            CacheDiagnostic::FullMiss { cause } => {
                assert_eq!(cause, CacheBreakCause::MessageHistoryRewritten);
            }
            other => panic!("expected FullMiss, got {other:?}"),
        }
    }
}

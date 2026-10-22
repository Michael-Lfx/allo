//! Local heuristic pre-filter for noise and meaningless filler detection.
//!
//! Authoritative baseline: docs/architecture/voice-agent/interaction.zh.md §2.3.

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, PartialEq)]
pub enum PreFilterVerdict {
    Pass,
    Drop(DropReason),
}

#[derive(Debug, Clone, PartialEq)]
pub enum DropReason {
    EmptyText,
    TooShortDuration { duration_sec: f32 },
    LowConfidence { score: f32 },
    MeaninglessFiller,
}

/// Local pre-filter executed immediately after ASR before calling cloud LLM.
/// Drops short/filler noises with 0 tokens consumed and 0 bytes egress.
pub struct HeuristicPreFilter {
    dropped_count: AtomicU64,
}

impl HeuristicPreFilter {
    pub fn new() -> Self {
        Self {
            dropped_count: AtomicU64::new(0),
        }
    }

    pub fn dropped_count(&self) -> u64 {
        self.dropped_count.load(Ordering::SeqCst)
    }

    pub fn evaluate(
        &self,
        transcript: &str,
        duration_sec: Option<f32>,
        confidence: Option<f32>,
    ) -> PreFilterVerdict {
        let trimmed = transcript.trim();

        // 1. Empty text
        if trimmed.is_empty() {
            self.dropped_count.fetch_add(1, Ordering::SeqCst);
            return PreFilterVerdict::Drop(DropReason::EmptyText);
        }

        // 2. Effective duration < 0.4s (interaction.zh.md §2.3)
        if let Some(dur) = duration_sec {
            if dur < 0.4 {
                self.dropped_count.fetch_add(1, Ordering::SeqCst);
                return PreFilterVerdict::Drop(DropReason::TooShortDuration { duration_sec: dur });
            }
        }

        // 3. Confidence score < 0.35
        if let Some(score) = confidence {
            if score < 0.35 {
                self.dropped_count.fetch_add(1, Ordering::SeqCst);
                return PreFilterVerdict::Drop(DropReason::LowConfidence { score });
            }
        }

        // 4. Meaningless filler particles / swallowed sounds
        if is_filler_text(trimmed) {
            self.dropped_count.fetch_add(1, Ordering::SeqCst);
            return PreFilterVerdict::Drop(DropReason::MeaninglessFiller);
        }

        PreFilterVerdict::Pass
    }
}

impl Default for HeuristicPreFilter {
    fn default() -> Self {
        Self::new()
    }
}

fn is_filler_text(text: &str) -> bool {
    let clean: String = text
        .chars()
        .filter(|c| {
            !c.is_whitespace()
                && !c.is_ascii_punctuation()
                && *c != '…'
                && *c != '—'
                && *c != '。'
                && *c != '，'
                && *c != '？'
                && *c != '！'
        })
        .collect();

    if clean.is_empty() {
        return true;
    }

    let lower = clean.to_lowercase();
    const EN_FILLERS: &[&str] = &["uh", "um", "ah", "eh", "er", "mm", "hmm"];
    if EN_FILLERS.contains(&lower.as_str()) {
        return true;
    }

    const ZH_FILLER_CHARS: &[char] = &['呃', '啊', '哎', '嗯', '哦', '唔', '咦', '呀', '哈'];
    if clean.chars().all(|c| ZH_FILLER_CHARS.contains(&c)) {
        return true;
    }

    false
}

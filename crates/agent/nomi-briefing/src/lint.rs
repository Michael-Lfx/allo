use serde::{Deserialize, Serialize};

use crate::cards::card_exists;
use crate::ir::Beat;
use crate::voice::TimingFile;

pub const TAIL_GUARD_SECS: f64 = 0.5;
pub const WORD_ANCHOR_DELTA_SECS: f64 = 0.1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct LintReport {
    pub ok: bool,
    pub errors: Vec<String>,
}

pub fn beat_lint(beats: &[Beat], timing: &TimingFile) -> LintReport {
    let mut errors = Vec::new();
    for beat in beats {
        if beat.anchors.is_empty() {
            errors.push(format!("beat {} missing word anchors", beat.id));
            continue;
        }
        for window in beat.anchors.windows(2) {
            let delta = (window[1].start_secs - window[0].end_secs).abs();
            if delta > WORD_ANCHOR_DELTA_SECS {
                errors.push(format!(
                    "beat {} word-anchor gap {delta:.3}s exceeds {WORD_ANCHOR_DELTA_SECS}",
                    beat.id
                ));
            }
        }
        let last = beat.anchors.last().map(|a| a.end_secs).unwrap_or(0.0);
        let shot_end = timing
            .chunks
            .iter()
            .filter(|c| c.beat_id == beat.id)
            .map(|c| c.end_secs)
            .fold(last, f64::max);
        if shot_end + 1e-9 < last {
            errors.push(format!("beat {} timing inverted", beat.id));
        }
        if shot_end - last + 1e-9 < TAIL_GUARD_SECS {
            errors.push(format!(
                "beat {} missing {TAIL_GUARD_SECS}s tail guard",
                beat.id
            ));
        }
    }
    LintReport {
        ok: errors.is_empty(),
        errors,
    }
}

pub fn card_lint(beats: &[Beat]) -> LintReport {
    let mut errors = Vec::new();
    for beat in beats {
        if beat.card.trim().is_empty() || !card_exists(&beat.card) {
            errors.push(format!(
                "beat {} card '{}' is not in the original sidecar catalog",
                beat.id, beat.card
            ));
        }
    }
    LintReport {
        ok: errors.is_empty(),
        errors,
    }
}

pub fn motion_check(beats: &[Beat]) -> LintReport {
    let mut errors = Vec::new();
    for beat in beats {
        if beat.spoken_text.trim().is_empty() {
            errors.push(format!("beat {} would render an empty desk", beat.id));
        }
    }
    LintReport {
        ok: errors.is_empty(),
        errors,
    }
}

pub fn merge_reports(parts: &[LintReport]) -> LintReport {
    let mut errors = Vec::new();
    for part in parts {
        errors.extend(part.errors.clone());
    }
    LintReport {
        ok: errors.is_empty(),
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{VisualKind, WordAnchor};

    fn beat_with_card(card: &str) -> Beat {
        Beat {
            id: "b1".into(),
            spoken_text: "今日要闻".into(),
            on_screen: String::new(),
            visual: VisualKind::EvidenceScreenshot,
            card: card.into(),
            claims: vec![],
            citations: vec![],
            anchors: vec![WordAnchor {
                word: "今".into(),
                start_secs: 0.0,
                end_secs: 0.2,
            }],
        }
    }

    #[test]
    fn unknown_card_fails_lint() {
        let report = card_lint(&[beat_with_card("apple_talkcraft")]);
        assert!(!report.ok);
    }

    #[test]
    fn catalog_card_passes() {
        assert!(card_lint(&[beat_with_card("title_desk")]).ok);
    }
}

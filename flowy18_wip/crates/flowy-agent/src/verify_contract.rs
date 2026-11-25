//! Host-owned verification contract extracted independently from the user's
//! brief (`最终方案.md` §6.6 & §6.7).
//!
//! Expectations must never come solely from the Worker model's self-reported
//! recipe. This module extracts only explicit, unambiguous constraints from the
//! literal user brief (minimum slide count, explicit target filename) and
//! enforces L1 per-slide structural completeness (non-empty title and content)
//! alongside the L2 anchor comparison.

use std::path::Path;

use crate::verify::{STRUCTURE, VALUE_MISMATCH, VerifyIssue, VerifyReport};

/// Per-slide structural measurements collected directly from the artifact bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlideL1Stats {
    pub index: usize,
    pub has_title: bool,
    pub has_body: bool,
    pub non_empty_shapes: usize,
}

/// Host-side constraints extracted from the literal user brief (or future
/// Router `ExpectedManifest`), immutable to the Worker model.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BriefContract {
    /// Minimum slide count explicitly requested in the brief (e.g. `3页`,
    /// `三页`, `3 slides`). `None` when the brief does not specify a count.
    pub min_slides: Option<usize>,
    /// Explicit filenames with supported deliverable extensions mentioned in
    /// the brief (e.g. `q3-east.pptx`, `summary.xlsx`).
    pub expected_filenames: Vec<String>,
    /// Reserved for Router-injected required fields (`ExecInput.context.decided`)
    /// so `BriefContract` upgrades cleanly without changing verifier signatures.
    pub required_fields: Vec<String>,
}

impl BriefContract {
    /// Deterministically extract only literal, unambiguous constraints from the
    /// user brief. Never guesses or invents requirements not stated in `brief`.
    #[must_use]
    pub fn from_brief(brief: &str) -> Self {
        Self {
            min_slides: extract_explicit_slide_count(brief),
            expected_filenames: extract_explicit_filenames(brief),
            required_fields: Vec::new(),
        }
    }

    /// Returns the expected filename for `ext` (without leading dot, e.g.
    /// `"pptx"`) only when the brief names exactly one file with that extension.
    #[must_use]
    pub fn expected_filename_for(&self, ext: &str) -> Option<&str> {
        let suffix = format!(".{}", ext.to_ascii_lowercase());
        let matches: Vec<&str> = self
            .expected_filenames
            .iter()
            .filter(|name| name.to_ascii_lowercase().ends_with(&suffix))
            .map(String::as_str)
            .collect();
        if matches.len() == 1 {
            Some(matches[0])
        } else {
            None
        }
    }

    /// Check filename compliance for any deliverable extension.
    pub fn check_filename(
        &self,
        artifact_path: &Path,
        ext: &str,
        issues: &mut Vec<VerifyIssue>,
    ) {
        let Some(expected) = self.expected_filename_for(ext) else {
            return;
        };
        let actual = artifact_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if !actual.eq_ignore_ascii_case(expected) {
            push_unique(issues, STRUCTURE, "brief_filename");
        }
    }

    /// Enforce §6.6 independent brief constraints and §6.7 L1 per-slide
    /// structural checks on a `.pptx` deck.
    pub fn check_deck(
        &self,
        artifact_path: &Path,
        actual_slides: usize,
        declared_slides: usize,
        per_slide: &[SlideL1Stats],
        anchor_count: usize,
        issues: &mut Vec<VerifyIssue>,
    ) {
        if let Some(min) = self.min_slides
            && (actual_slides < min || declared_slides < min)
        {
            push_unique(issues, STRUCTURE, "brief_min_slides");
        }
        self.check_filename(artifact_path, "pptx", issues);
        if anchor_count == 0 {
            push_unique(issues, STRUCTURE, "anchors");
        }
        for slide in per_slide {
            if slide.non_empty_shapes == 0 {
                push_unique(
                    issues,
                    STRUCTURE,
                    &format!("slide_{}_empty", slide.index),
                );
            } else {
                if !slide.has_title {
                    push_unique(
                        issues,
                        STRUCTURE,
                        &format!("slide_{}_title", slide.index),
                    );
                }
                if !slide.has_body {
                    push_unique(
                        issues,
                        STRUCTURE,
                        &format!("slide_{}_body", slide.index),
                    );
                }
            }
        }
    }
}

/// Deterministic post-verification branching recommendation (`最终方案.md`
/// main flow step 11).
#[must_use]
pub fn next_action_for(report: &VerifyReport) -> &'static str {
    if report.ok {
        "publish"
    } else if !report.issues.is_empty()
        && report
            .issues
            .iter()
            .all(|issue| issue.code == VALUE_MISMATCH)
    {
        "rerender_once"
    } else {
        "clarify_or_rebuild"
    }
}

fn push_unique(issues: &mut Vec<VerifyIssue>, code: &'static str, field: &str) {
    if issues
        .iter()
        .any(|issue| issue.code == code && issue.field == field)
    {
        return;
    }
    issues.push(VerifyIssue {
        code,
        field: field.to_owned(),
    });
}

fn extract_explicit_slide_count(brief: &str) -> Option<usize> {
    let chars: Vec<(usize, char)> = brief.char_indices().collect();
    let mut best: Option<usize> = None;

    for idx in 0..chars.len() {
        let (byte_pos, ch) = chars[idx];
        // Skip ordinals such as "第1页" / "第2页" so references to a specific
        // slide are never mistaken for a total slide count.
        if idx > 0 && chars[idx - 1].1 == '第' {
            continue;
        }
        if ch.is_ascii_digit() {
            if idx > 0 && chars[idx - 1].1.is_ascii_digit() {
                continue;
            }
            let mut end = idx;
            while end < chars.len() && chars[end].1.is_ascii_digit() {
                end += 1;
            }
            let end_byte = if end < chars.len() {
                chars[end].0
            } else {
                brief.len()
            };
            let Ok(num) = brief[byte_pos..end_byte].parse::<usize>() else {
                continue;
            };
            if !(1..=50).contains(&num) {
                continue;
            }
            let rest = brief[end_byte..].trim_start_matches([' ', '\t', '-']);
            if starts_with_slide_unit(rest) {
                best = Some(best.map_or(num, |prev| prev.max(num)));
            }
            continue;
        }

        if let Some((num, consumed_chars)) =
            parse_chinese_numeral(&chars[idx..])
        {
            let next_idx = idx + consumed_chars;
            let next_byte = if next_idx < chars.len() {
                chars[next_idx].0
            } else {
                brief.len()
            };
            let rest = brief[next_byte..].trim_start_matches([' ', '\t']);
            if starts_with_chinese_slide_unit(rest) {
                best = Some(best.map_or(num, |prev| prev.max(num)));
            }
        }
    }
    best
}

fn starts_with_slide_unit(rest: &str) -> bool {
    if starts_with_chinese_slide_unit(rest) {
        return true;
    }
    let lower = rest.to_ascii_lowercase();
    for unit in ["slides", "slide"] {
        if let Some(after) = lower.strip_prefix(unit)
            && after
                .chars()
                .next()
                .is_none_or(|c| !c.is_ascii_alphanumeric())
        {
            return true;
        }
    }
    false
}

fn starts_with_chinese_slide_unit(rest: &str) -> bool {
    rest.starts_with('页')
        || rest.starts_with("张PPT")
        || rest.starts_with("张ppt")
        || rest.starts_with("张幻灯片")
}

fn parse_chinese_numeral(slice: &[(usize, char)]) -> Option<(usize, usize)> {
    let first = slice.first()?.1;
    let second = slice.get(1).map(|(_, ch)| *ch);
    match (first, second) {
        ('十', Some('一')) => Some((11, 2)),
        ('十', Some('二')) => Some((12, 2)),
        ('十', _) => Some((10, 1)),
        ('一', _) => Some((1, 1)),
        ('二' | '两', _) => Some((2, 1)),
        ('三', _) => Some((3, 1)),
        ('四', _) => Some((4, 1)),
        ('五', _) => Some((5, 1)),
        ('六', _) => Some((6, 1)),
        ('七', _) => Some((7, 1)),
        ('八', _) => Some((8, 1)),
        ('九', _) => Some((9, 1)),
        _ => None,
    }
}

fn extract_explicit_filenames(brief: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut start: Option<usize> = None;
    for (idx, ch) in brief.char_indices() {
        let valid = ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-');
        if valid {
            if start.is_none() {
                start = Some(idx);
            }
        } else if let Some(begin) = start.take() {
            maybe_push_filename(&brief[begin..idx], &mut found);
        }
    }
    if let Some(begin) = start {
        maybe_push_filename(&brief[begin..], &mut found);
    }
    found
}

fn maybe_push_filename(raw: &str, out: &mut Vec<String>) {
    let candidate = raw.trim_matches('.');
    let lower = candidate.to_ascii_lowercase();
    let is_target = [".pptx", ".docx", ".xlsx"]
        .iter()
        .any(|ext| lower.ends_with(ext) && lower.len() > ext.len());
    if is_target
        && !out
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(candidate))
    {
        out.push(candidate.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_explicit_slide_counts_and_ignores_ordinals_and_unstated_briefs()
    {
        assert_eq!(
            BriefContract::from_brief("请做一份3页华东区汇报 q3-east.pptx")
                .min_slides,
            Some(3)
        );
        assert_eq!(
            BriefContract::from_brief("生成三页PPT总结产品路线图").min_slides,
            Some(3)
        );
        assert_eq!(
            BriefContract::from_brief("Create a 4-slide executive deck")
                .min_slides,
            Some(4)
        );
        assert_eq!(
            BriefContract::from_brief("请参考第5页的风格整理一份汇报")
                .min_slides,
            None
        );
        assert_eq!(
            BriefContract::from_brief("整理华东区季度销售数据").min_slides,
            None
        );
    }

    #[test]
    fn extracts_filenames_per_extension_without_cross_format_confusion() {
        let contract = BriefContract::from_brief(
            "读取 input.xlsx 数据并生成三页汇报 q3-east.pptx 交付给我。",
        );
        assert_eq!(
            contract.expected_filename_for("pptx"),
            Some("q3-east.pptx")
        );
        assert_eq!(contract.expected_filename_for("xlsx"), Some("input.xlsx"));
        assert_eq!(contract.expected_filename_for("docx"), None);

        // Multiple filenames of the same extension stay ambiguous -> None.
        let multi = BriefContract::from_brief("对比 a.pptx 和 b.pptx");
        assert_eq!(multi.expected_filename_for("pptx"), None);
    }

    #[test]
    fn shrunken_worker_slides_and_empty_slides_are_caught_by_host_contract() {
        let contract = BriefContract::from_brief(
            "生成一份3页PPT，命名为 q3-east.pptx，汇总华东区Q3业绩",
        );
        let mut issues = Vec::new();
        // Worker shrank the deck to 2 slides and self-declared slides=2.
        contract.check_deck(
            Path::new("outputs/sub/wrong-name.pptx"),
            2,
            2,
            &[
                SlideL1Stats {
                    index: 0,
                    has_title: true,
                    has_body: true,
                    non_empty_shapes: 2,
                },
                SlideL1Stats {
                    index: 1,
                    has_title: false,
                    has_body: false,
                    non_empty_shapes: 0,
                },
            ],
            0,
            &mut issues,
        );
        let fields: Vec<&str> =
            issues.iter().map(|issue| issue.field.as_str()).collect();
        assert!(fields.contains(&"brief_min_slides"));
        assert!(fields.contains(&"brief_filename"));
        assert!(fields.contains(&"anchors"));
        assert!(fields.contains(&"slide_1_empty"));
    }

    #[test]
    fn next_action_distinguishes_publish_rerender_and_rebuild() {
        let pass = VerifyReport {
            artifact: "a.pptx".into(),
            anchor: "shape_name",
            ok: true,
            issues: Vec::new(),
        };
        assert_eq!(next_action_for(&pass), "publish");

        let mismatch = VerifyReport {
            artifact: "a.pptx".into(),
            anchor: "shape_name",
            ok: false,
            issues: vec![VerifyIssue {
                code: VALUE_MISMATCH,
                field: "S0-body".into(),
            }],
        };
        assert_eq!(next_action_for(&mismatch), "rerender_once");

        let broken = VerifyReport {
            artifact: "a.pptx".into(),
            anchor: "shape_name",
            ok: false,
            issues: vec![VerifyIssue {
                code: STRUCTURE,
                field: "brief_min_slides".into(),
            }],
        };
        assert_eq!(next_action_for(&broken), "clarify_or_rebuild");
    }
}

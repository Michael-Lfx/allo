//! Deterministic delivery-time verification for `.pptx`, `.docx`, and `.xlsx`.
//!
//! All three formats share one [`VerifyReport`] contract, one four-code anchor
//! vocabulary (`ANCHOR_MISSING`, `ANCHOR_DUPLICATE`, `ANCHOR_INVALID`,
//! `VALUE_MISMATCH`, plus L1 `STRUCTURE`), host-owned [`BriefContract`] checks,
//! and one persistence/telemetry path into [`DeliveryStore`].

use std::collections::{BTreeMap, HashMap};
use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use flowy_agent_core::tools::ToolDefinition;
use flowy_agent_core::{
    RequiredDeliverable, ToolCallArgs, ToolExecutionResult, ToolRunContext,
    WorkContract,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::delivery::{self, DeliveryStore};
use crate::pptx::resolve_turn_path;
use crate::semantic_verify::{
    SemanticContext, SemanticOutcome, probe_deck_semantics,
};

#[allow(clippy::redundant_pub_crate)]
pub(crate) type VerifyFailureTracker = Arc<Mutex<HashMap<String, u32>>>;

pub const ANCHOR_MISSING: &str = "ANCHOR_MISSING";
pub const ANCHOR_DUPLICATE: &str = "ANCHOR_DUPLICATE";
pub const ANCHOR_INVALID: &str = "ANCHOR_INVALID";
pub const VALUE_MISMATCH: &str = "VALUE_MISMATCH";
pub const STRUCTURE: &str = "STRUCTURE";

/// Safety bound per decompressed XML entry inside an OOXML archive (16 MiB).
const MAX_XML_ENTRY_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyIssue {
    pub code: &'static str,
    pub field: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyReport {
    pub artifact: String,
    pub anchor: &'static str,
    pub ok: bool,
    pub issues: Vec<VerifyIssue>,
}

impl VerifyReport {
    #[must_use]
    pub fn to_value_with_meta(
        &self,
        summary: &str,
        report_id: &str,
        artifact_hash: &str,
        next_action: &str,
        contract_revision: Option<u32>,
        semantic_hint: Option<&str>,
    ) -> Value {
        json!({
            "summary": summary,
            "report_id": report_id,
            "artifact": self.artifact,
            "artifact_hash": artifact_hash,
            "contract_revision": contract_revision,
            "anchor": self.anchor,
            "ok": self.ok,
            "next_action": next_action,
            "semantic_hint": semantic_hint,
            "issues": self.issues.iter().map(|issue| json!({
                "code": issue.code,
                "field": issue.field,
                "ui_key": ui_key(issue.code),
            })).collect::<Vec<_>>(),
        })
    }
}

#[must_use]
pub fn ui_key(code: &str) -> &'static str {
    match code {
        ANCHOR_MISSING => "verify.anchor_missing",
        ANCHOR_DUPLICATE => "verify.anchor_duplicate",
        ANCHOR_INVALID => "verify.anchor_invalid",
        VALUE_MISMATCH => "verify.value_mismatch",
        _ => "verify.structure",
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeckAnchor {
    pub anchor: String,
    pub value: String,
}

#[derive(Clone, Copy, Debug)]
struct SlideL1Stats {
    index: usize,
    has_title: bool,
    has_body: bool,
    non_empty_shapes: usize,
}

/// Host-owned literal constraints extracted from user briefs and evolved
/// across follow-up revisions (§6.6, §6.7 & kernel review §2.4).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BriefContract {
    pub revision: u32,
    pub min_slides: Option<usize>,
    pub expected_filenames: Vec<String>,
    pub required_items: Vec<RequiredDeliverable>,
    pub pinned_facts: BTreeMap<String, String>,
}

impl BriefContract {
    #[must_use]
    pub fn from_brief(brief: &str) -> Self {
        let min_slides = extract_explicit_slide_count(brief);
        let expected_filenames = extract_explicit_filenames(brief);
        let required_items =
            extract_required_items(brief, &expected_filenames, min_slides);
        let pinned_facts = extract_pinned_facts(brief);
        Self {
            revision: 1,
            min_slides,
            expected_filenames,
            required_items,
            pinned_facts,
        }
    }

    /// Evolve a prior persisted [`WorkContract`] with a follow-up prompt.
    ///
    /// Unrevoked slide counts, required deliverables, and pinned facts (such
    /// as quote amount) survive narrow follow-ups like "only change the date",
    /// while newly mentioned values update their corresponding constraint and
    /// bump `revision`.
    #[must_use]
    pub fn evolve(previous: Option<&WorkContract>, prompt: &str) -> Self {
        let delta = Self::from_brief(prompt);
        let Some(prev) = previous else {
            return delta;
        };
        let mut required_items = prev.required_items.clone();
        for item in delta.required_items {
            let duplicate = required_items.iter().any(|existing| {
                existing.extension.eq_ignore_ascii_case(&item.extension)
                    && match (&existing.filename, &item.filename) {
                        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                        (None, None) => true,
                        _ => false,
                    }
            });
            if !duplicate {
                // If an explicit filename is introduced for an extension that
                // previously only had a generic role placeholder, upgrade it.
                if item.filename.is_some()
                    && let Some(slot) = required_items.iter_mut().find(|e| {
                        e.filename.is_none()
                            && e.extension.eq_ignore_ascii_case(&item.extension)
                    })
                {
                    *slot = item;
                } else {
                    required_items.push(item);
                }
            }
        }
        let expected_filenames: Vec<String> = required_items
            .iter()
            .filter_map(|item| item.filename.clone())
            .collect();
        let mut pinned_facts = prev.pinned_facts.clone();
        for (key, value) in delta.pinned_facts {
            pinned_facts.insert(key, value);
        }
        Self {
            revision: prev.revision.saturating_add(1).max(1),
            min_slides: delta.min_slides.or(prev.min_slides),
            expected_filenames,
            required_items,
            pinned_facts,
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.min_slides.is_none()
            && self.expected_filenames.is_empty()
            && self.required_items.is_empty()
            && self.pinned_facts.is_empty()
    }

    #[must_use]
    pub fn to_work_contract(&self) -> WorkContract {
        WorkContract {
            revision: self.revision.max(1),
            required_items: self.required_items.clone(),
            min_slides: self.min_slides,
            pinned_facts: self.pinned_facts.clone(),
        }
    }

    fn check_filename(
        &self,
        artifact_path: &Path,
        ext: &str,
        issues: &mut Vec<VerifyIssue>,
    ) {
        let suffix = format!(".{}", ext.to_ascii_lowercase());
        let matches: Vec<&str> = self
            .expected_filenames
            .iter()
            .filter(|name| name.to_ascii_lowercase().ends_with(&suffix))
            .map(String::as_str)
            .collect();
        if matches.len() != 1 {
            return;
        }
        let actual = artifact_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if !actual.eq_ignore_ascii_case(matches[0]) {
            push_unique(issues, STRUCTURE, "brief_filename");
        }
    }

    fn check_pinned_facts(
        &self,
        observed_text: &str,
        issues: &mut Vec<VerifyIssue>,
    ) {
        if self.pinned_facts.is_empty() {
            return;
        }
        let normalized_doc = compact_text(observed_text);
        let single_deliverable = self.required_items.len() <= 1;
        if let Some(expected_quote) = self.pinned_facts.get("quote") {
            let mentions_price = normalized_doc.contains("报价")
                || normalized_doc.contains("总价")
                || normalized_doc.contains("预算")
                || normalized_doc.contains("万")
                || normalized_doc.contains("元")
                || normalized_doc.contains('¥')
                || normalized_doc.contains('$')
                || normalized_doc.contains("price")
                || normalized_doc.contains("quote");
            if (single_deliverable || mentions_price)
                && !quote_matches(&normalized_doc, expected_quote)
            {
                push_unique(issues, VALUE_MISMATCH, "brief_quote");
            }
        }
        if let Some(expected_date) = self.pinned_facts.get("date") {
            let mentions_date = normalized_doc.contains("日期")
                || normalized_doc.contains("202")
                || normalized_doc.contains('年')
                || normalized_doc.contains('月')
                || normalized_doc.contains("date");
            if (single_deliverable || mentions_date)
                && !date_matches(&normalized_doc, expected_date)
            {
                push_unique(issues, VALUE_MISMATCH, "brief_date");
            }
        }
    }

    fn check_deck(
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

fn next_action_for(report: &VerifyReport) -> &'static str {
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

fn extract_explicit_slide_count(brief: &str) -> Option<usize> {
    let chars: Vec<(usize, char)> = brief.char_indices().collect();
    let mut best: Option<usize> = None;
    for idx in 0..chars.len() {
        // Skip ordinals ("第1页"/"第3页") and reference-document counts
        // ("参考那份20页的旧报告") so they never override the requested target.
        if idx > 0 && chars[idx - 1].1 == '第' {
            continue;
        }
        let prefix_start = idx.saturating_sub(6);
        let prefix_slice: String =
            chars[prefix_start..idx].iter().map(|(_, c)| *c).collect();
        if ["参考", "旧", "原", "对比", "已有", "那份", "这份"]
            .iter()
            .any(|marker| prefix_slice.contains(marker))
        {
            continue;
        }
        let (byte_pos, ch) = chars[idx];
        let (num, end_idx, allow_en) = if ch.is_ascii_digit() {
            if idx > 0 && chars[idx - 1].1.is_ascii_digit() {
                continue;
            }
            let mut end = idx;
            while end < chars.len() && chars[end].1.is_ascii_digit() {
                end += 1;
            }
            let end_byte = chars.get(end).map_or(brief.len(), |(pos, _)| *pos);
            let Ok(num) = brief[byte_pos..end_byte].parse::<usize>() else {
                continue;
            };
            if !(1..=50).contains(&num) {
                continue;
            }
            (num, end, true)
        } else if let Some((num, len)) = parse_chinese_numeral(&chars[idx..]) {
            (num, idx + len, false)
        } else {
            continue;
        };
        let next_byte = chars.get(end_idx).map_or(brief.len(), |(pos, _)| *pos);
        let rest = brief[next_byte..].trim_start_matches([' ', '\t', '-']);
        let is_cn_unit = rest.starts_with('页')
            || rest.starts_with("张PPT")
            || rest.starts_with("张ppt")
            || rest.starts_with("张幻灯片");
        let is_en_unit = allow_en && {
            let lower = rest.to_ascii_lowercase();
            ["slides", "slide"].iter().any(|unit| {
                lower.strip_prefix(unit).is_some_and(|after| {
                    after
                        .chars()
                        .next()
                        .is_none_or(|c| !c.is_ascii_alphanumeric())
                })
            })
        };
        if is_cn_unit || is_en_unit {
            best = Some(best.map_or(num, |prev| prev.max(num)));
        }
    }
    best
}

fn parse_chinese_numeral(slice: &[(usize, char)]) -> Option<(usize, usize)> {
    match (slice.first()?.1, slice.get(1).map(|(_, ch)| *ch)) {
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
    let mut out: Vec<String> = Vec::new();
    for token in brief.split(|ch: char| {
        !(ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    }) {
        let candidate = token.trim_matches('.');
        let lower = candidate.to_ascii_lowercase();
        let is_target = [".pptx", ".docx", ".xlsx", ".pdf", ".csv"]
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
    out
}

fn extract_required_items(
    brief: &str,
    explicit_filenames: &[String],
    min_slides: Option<usize>,
) -> Vec<RequiredDeliverable> {
    let mut items: Vec<RequiredDeliverable> = explicit_filenames
        .iter()
        .filter_map(|name| {
            let ext = Path::new(name)
                .extension()
                .and_then(|ext| ext.to_str())
                .map(str::to_ascii_lowercase)?;
            Some(RequiredDeliverable {
                filename: Some(name.clone()),
                extension: ext,
            })
        })
        .collect();
    let lower = brief.to_ascii_lowercase();
    let has_ext = |ext: &str, list: &[RequiredDeliverable]| {
        list.iter()
            .any(|item| item.extension.eq_ignore_ascii_case(ext))
    };
    let has_production_intent = [
        "做", "生成", "写", "交", "输出", "给", "制作", "整理", "create",
        "build", "deliver", "make", "generate", "publish",
    ]
    .iter()
    .any(|verb| lower.contains(verb));
    if !has_ext("xlsx", &items)
        && !has_ext("csv", &items)
        && (brief.contains("报价表")
            || (has_production_intent
                && (lower.contains("excel") || lower.contains(".xlsx"))))
    {
        items.push(RequiredDeliverable {
            filename: None,
            extension: "xlsx".to_owned(),
        });
    }
    if !has_ext("pptx", &items)
        && (min_slides.is_some()
            || (has_production_intent
                && (brief.contains("汇报")
                    || brief.contains("幻灯片")
                    || lower.contains("ppt")
                    || lower.contains("deck")
                    || lower.contains("slides"))))
    {
        items.push(RequiredDeliverable {
            filename: None,
            extension: "pptx".to_owned(),
        });
    }
    if !has_ext("docx", &items)
        && has_production_intent
        && (lower.contains("word") || lower.contains(".docx"))
    {
        items.push(RequiredDeliverable {
            filename: None,
            extension: "docx".to_owned(),
        });
    }
    items
}

fn extract_pinned_facts(brief: &str) -> BTreeMap<String, String> {
    let mut facts = BTreeMap::new();
    if let Some(quote) = extract_quote_fact(brief) {
        facts.insert("quote".to_owned(), quote);
    }
    if let Some(date) = extract_date_fact(brief) {
        facts.insert("date".to_owned(), date);
    }
    facts
}

fn extract_quote_fact(brief: &str) -> Option<String> {
    let lower = brief.to_ascii_lowercase();
    for keyword in ["报价", "总价", "预算", "quote", "price"] {
        for (pos, _) in lower.match_indices(keyword) {
            let after = &brief[pos + keyword.len()..];
            // Skip document nouns such as "报价表" / "报价单".
            if after.starts_with('表') || after.starts_with('单') {
                continue;
            }
            let window: String = after.chars().take(24).collect();
            let chars: Vec<(usize, char)> = window.char_indices().collect();
            let Some(start_idx) =
                chars.iter().position(|(_, c)| c.is_ascii_digit())
            else {
                continue;
            };
            let prefix_ok = chars[..start_idx].iter().all(|(_, c)| {
                c.is_whitespace()
                    || matches!(
                        c,
                        ':' | '：'
                            | '='
                            | '为'
                            | '是'
                            | '约'
                            | '共'
                            | '¥'
                            | '￥'
                            | '$'
                            | '('
                            | '（'
                    )
            });
            if !prefix_ok {
                continue;
            }
            let mut end_idx = start_idx;
            while end_idx < chars.len()
                && (chars[end_idx].1.is_ascii_digit()
                    || matches!(chars[end_idx].1, '.' | ','))
            {
                end_idx += 1;
            }
            let start_byte = chars[start_idx].0;
            let end_byte = chars.get(end_idx).map_or(window.len(), |(p, _)| *p);
            let number = window[start_byte..end_byte].replace(',', "");
            if number.is_empty() {
                continue;
            }
            let tail = window[end_byte..].trim_start();
            if tail.starts_with('页')
                || tail.starts_with('张')
                || tail.to_ascii_lowercase().starts_with("slide")
            {
                continue;
            }
            let unit = if tail.starts_with("万元") || tail.starts_with('万')
            {
                "万"
            } else if tail.starts_with("亿元") || tail.starts_with('亿') {
                "亿"
            } else if tail.starts_with('元') {
                "元"
            } else {
                ""
            };
            return Some(format!("{number}{unit}"));
        }
    }
    None
}

fn extract_date_fact(brief: &str) -> Option<String> {
    let bytes = brief.as_bytes();
    // 1. YYYY-MM-DD or YYYY/MM/DD
    for i in 0..bytes.len().saturating_sub(7) {
        if bytes[i..].starts_with(b"20")
            && bytes.get(i + 2).is_some_and(u8::is_ascii_digit)
            && bytes.get(i + 3).is_some_and(u8::is_ascii_digit)
            && let Some(sep @ (b'-' | b'/')) = bytes.get(i + 4).copied()
        {
            let rest = &brief[i + 5..];
            let mut parts = rest.split(char::from(sep));
            if let (Some(m_str), Some(d_tail)) = (parts.next(), parts.next()) {
                let d_digits: String =
                    d_tail.chars().take_while(char::is_ascii_digit).collect();
                if let (Ok(m), Ok(d)) =
                    (m_str.parse::<u32>(), d_digits.parse::<u32>())
                    && (1..=12).contains(&m)
                    && (1..=31).contains(&d)
                {
                    let year = &brief[i..i + 4];
                    return Some(format!("{year}-{m:02}-{d:02}"));
                }
            }
        }
    }
    // 2. YYYY年M月D日
    let chars: Vec<(usize, char)> = brief.char_indices().collect();
    for i in 0..chars.len() {
        if chars[i].1 == '年' && i >= 4 {
            let y: String = chars[i - 4..i].iter().map(|(_, c)| *c).collect();
            if y.starts_with("20") && y.chars().all(|c| c.is_ascii_digit()) {
                let after_y = &brief[chars[i].0 + '年'.len_utf8()..];
                if let Some((m_str, after_m)) = after_y.split_once('月') {
                    let d_digits: String = after_m
                        .chars()
                        .take_while(char::is_ascii_digit)
                        .collect();
                    if let (Ok(m), Ok(d)) =
                        (m_str.trim().parse::<u32>(), d_digits.parse::<u32>())
                        && (1..=12).contains(&m)
                        && (1..=31).contains(&d)
                    {
                        return Some(format!("{y}-{m:02}-{d:02}"));
                    }
                }
            }
        }
    }
    None
}

fn compact_text(input: &str) -> String {
    input
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != ',')
        .flat_map(char::to_lowercase)
        .collect()
}

fn quote_matches(normalized_doc: &str, expected_quote: &str) -> bool {
    let target = compact_text(expected_quote);
    if normalized_doc.contains(&target) {
        return true;
    }
    if let Some(wan) = target.strip_suffix('万')
        && let Ok(val) = wan.parse::<f64>()
    {
        let expanded = format!("{}", (val * 10_000.0).round() as u64);
        if normalized_doc.contains(&expanded) {
            return true;
        }
    }
    false
}

fn date_matches(normalized_doc: &str, expected_date: &str) -> bool {
    let target = compact_text(expected_date);
    if normalized_doc.contains(&target) {
        return true;
    }
    let parts: Vec<&str> = expected_date.split('-').collect();
    if let [y, m, d] = parts.as_slice()
        && let (Ok(m_num), Ok(d_num)) = (m.parse::<u32>(), d.parse::<u32>())
    {
        let slash = format!("{y}/{m}/{d}");
        let slash_short = format!("{y}/{m_num}/{d_num}");
        let dash_short = format!("{y}-{m_num}-{d_num}");
        let cn = format!("{y}年{m_num}月{d_num}日");
        let cn_pad = format!("{y}年{m}月{d}日");
        return [slash, slash_short, dash_short, cn, cn_pad]
            .iter()
            .any(|variant| normalized_doc.contains(variant));
    }
    false
}

fn push_unique(issues: &mut Vec<VerifyIssue>, code: &'static str, field: &str) {
    if !issues
        .iter()
        .any(|issue| issue.code == code && issue.field == field)
    {
        issues.push(VerifyIssue {
            code,
            field: field.to_owned(),
        });
    }
}

fn evaluate_pairs(
    observed: &[(String, String)],
    expected: &[DeckAnchor],
    flag_extra: bool,
    issues: &mut Vec<VerifyIssue>,
) {
    let mut seen: Vec<&str> = Vec::with_capacity(expected.len());
    for anchor in expected {
        let matches: Vec<&str> = observed
            .iter()
            .filter(|(name, _)| name == &anchor.anchor)
            .map(|(_, value)| value.as_str())
            .collect();
        match matches.as_slice() {
            [] => push_unique(issues, ANCHOR_MISSING, &anchor.anchor),
            [actual] if *actual != anchor.value => {
                push_unique(issues, VALUE_MISMATCH, &anchor.anchor);
            }
            [_] => {}
            _ => push_unique(issues, ANCHOR_DUPLICATE, &anchor.anchor),
        }
        seen.push(&anchor.anchor);
    }
    if flag_extra {
        for (name, _) in observed {
            if !seen.contains(&name.as_str()) {
                push_unique(issues, ANCHOR_INVALID, name);
            }
        }
    }
}

fn evaluate(
    artifact: &str,
    slide_count: usize,
    expected_slides: usize,
    deck: &[(String, String)],
    expected: &[DeckAnchor],
) -> VerifyReport {
    let mut issues = Vec::new();
    if slide_count != expected_slides {
        push_unique(&mut issues, STRUCTURE, "slides");
    }
    evaluate_pairs(deck, expected, true, &mut issues);
    VerifyReport {
        artifact: artifact.to_owned(),
        anchor: "shape_name",
        ok: issues.is_empty(),
        issues,
    }
}

struct VerifiedDeck {
    report: VerifyReport,
    bytes: Vec<u8>,
    shapes: Vec<(String, String)>,
}

fn verify_deck_file_with_shapes(
    path: &Path,
    expected_slides: usize,
    expected: &[DeckAnchor],
    contract: Option<&BriefContract>,
) -> Result<VerifiedDeck, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let presentation = rpptx::Presentation::from_bytes(&bytes)
        .map_err(|error| error.to_string())?;
    let slides: Vec<rpptx::SlideRef<'_>> = presentation.slides().collect();
    let mut deck = Vec::new();
    let mut per_slide = Vec::with_capacity(slides.len());
    for (index, slide) in slides.iter().enumerate() {
        let mut has_title = false;
        let mut has_body = false;
        let mut non_empty_shapes = 0_usize;
        for shape in slide.shapes() {
            if let Some(name) = shape.non_visual_name() {
                let text = shape.text().unwrap_or_default();
                if !text.trim().is_empty() {
                    non_empty_shapes += 1;
                    if name.to_ascii_lowercase().contains("title") {
                        has_title = true;
                    } else {
                        has_body = true;
                    }
                    deck.push((name, text));
                }
            }
        }
        per_slide.push(SlideL1Stats {
            index,
            has_title,
            has_body,
            non_empty_shapes,
        });
    }
    let mut report = evaluate(
        &path.display().to_string(),
        slides.len(),
        expected_slides,
        &deck,
        expected,
    );
    let default_contract = BriefContract::default();
    let active_contract = contract.unwrap_or(&default_contract);
    active_contract.check_deck(
        path,
        slides.len(),
        expected_slides,
        &per_slide,
        expected.len(),
        &mut report.issues,
    );
    let combined_text: String = deck
        .iter()
        .map(|(_, text)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    active_contract.check_pinned_facts(&combined_text, &mut report.issues);
    report.ok = report.issues.is_empty();
    Ok(VerifiedDeck {
        report,
        bytes,
        shapes: deck,
    })
}

struct VerifiedArtifact {
    report: VerifyReport,
    bytes: Vec<u8>,
    snippets: Vec<(String, String)>,
}

fn read_zip_xml(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    entry_name: &str,
) -> Option<String> {
    let entry = archive.by_name(entry_name).ok()?;
    let mut buf = Vec::new();
    entry
        .take(MAX_XML_ENTRY_BYTES + 1)
        .read_to_end(&mut buf)
        .ok()?;
    if buf.len() as u64 > MAX_XML_ENTRY_BYTES {
        return None;
    }
    String::from_utf8(buf).ok()
}

fn verify_docx_file(
    path: &Path,
    expected: &[DeckAnchor],
    contract: Option<&BriefContract>,
) -> Result<VerifiedArtifact, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let mut issues = Vec::new();
    if let Some(contract) = contract {
        contract.check_filename(path, "docx", &mut issues);
    }
    if expected.is_empty() {
        push_unique(&mut issues, STRUCTURE, "anchors");
    }
    let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(bytes.as_slice()))
    else {
        push_unique(&mut issues, STRUCTURE, "docx_archive");
        return Ok(VerifiedArtifact {
            report: report_for(path, "bookmark", issues),
            bytes,
            snippets: Vec::new(),
        });
    };
    let Some(xml) = read_zip_xml(&mut archive, "word/document.xml") else {
        push_unique(&mut issues, STRUCTURE, "docx_document");
        return Ok(VerifiedArtifact {
            report: report_for(path, "bookmark", issues),
            bytes,
            snippets: Vec::new(),
        });
    };
    let Ok(doc) = roxmltree::Document::parse(&xml) else {
        push_unique(&mut issues, STRUCTURE, "docx_document");
        return Ok(VerifiedArtifact {
            report: report_for(path, "bookmark", issues),
            bytes,
            snippets: Vec::new(),
        });
    };
    let mut non_empty_paragraphs = 0_usize;
    let mut bookmarks: Vec<(String, String)> = Vec::new();
    let mut snippets: Vec<(String, String)> = Vec::new();
    let mut all_text = String::new();
    for para in doc
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "p")
    {
        let mut text = String::new();
        for node in para.descendants().filter(roxmltree::Node::is_element) {
            match node.tag_name().name() {
                "t" => {
                    if let Some(chunk) = node.text() {
                        text.push_str(chunk);
                    }
                }
                "tab" => text.push('\t'),
                "br" => text.push('\n'),
                _ => {}
            }
        }
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            non_empty_paragraphs += 1;
            all_text.push_str(trimmed);
            all_text.push('\n');
            snippets
                .push((format!("P{non_empty_paragraphs}"), trimmed.to_owned()));
        }
        for bm in para.descendants().filter(|node| {
            node.is_element() && node.tag_name().name() == "bookmarkStart"
        }) {
            if let Some(name) = bm
                .attributes()
                .find(|attr| attr.name() == "name")
                .map(|attr| attr.value())
                && !name.is_empty()
                && !name.starts_with('_')
            {
                bookmarks.push((name.to_owned(), trimmed.to_owned()));
            }
        }
    }
    if non_empty_paragraphs == 0 {
        push_unique(&mut issues, STRUCTURE, "docx_empty");
    }
    if !expected.is_empty() {
        evaluate_pairs(&bookmarks, expected, false, &mut issues);
    }
    if let Some(contract) = contract {
        contract.check_pinned_facts(&all_text, &mut issues);
    }
    Ok(VerifiedArtifact {
        report: report_for(path, "bookmark", issues),
        bytes,
        snippets,
    })
}

fn verify_xlsx_file(
    path: &Path,
    expected: &[DeckAnchor],
    contract: Option<&BriefContract>,
) -> Result<VerifiedArtifact, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let mut issues = Vec::new();
    if let Some(contract) = contract {
        contract.check_filename(path, "xlsx", &mut issues);
    }
    if expected.is_empty() {
        push_unique(&mut issues, STRUCTURE, "anchors");
    }
    let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(bytes.as_slice()))
    else {
        push_unique(&mut issues, STRUCTURE, "xlsx_archive");
        return Ok(VerifiedArtifact {
            report: report_for(path, "row_key", issues),
            bytes,
            snippets: Vec::new(),
        });
    };
    let Some(wb_xml) = read_zip_xml(&mut archive, "xl/workbook.xml") else {
        push_unique(&mut issues, STRUCTURE, "xlsx_workbook");
        return Ok(VerifiedArtifact {
            report: report_for(path, "row_key", issues),
            bytes,
            snippets: Vec::new(),
        });
    };
    let Ok(wb_doc) = roxmltree::Document::parse(&wb_xml) else {
        push_unique(&mut issues, STRUCTURE, "xlsx_workbook");
        return Ok(VerifiedArtifact {
            report: report_for(path, "row_key", issues),
            bytes,
            snippets: Vec::new(),
        });
    };
    let sheet_titles: Vec<String> = wb_doc
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "sheet")
        .filter_map(|node| node.attribute("name").map(str::to_owned))
        .collect();
    let shared_strings: Vec<String> =
        read_zip_xml(&mut archive, "xl/sharedStrings.xml")
            .and_then(|xml| {
                let doc = roxmltree::Document::parse(&xml).ok()?;
                Some(
                    doc.descendants()
                        .filter(|node| {
                            node.is_element() && node.tag_name().name() == "si"
                        })
                        .map(|si| {
                            si.descendants()
                                .filter(|node| {
                                    node.is_element()
                                        && node.tag_name().name() == "t"
                                })
                                .filter_map(|node| node.text())
                                .collect::<String>()
                        })
                        .collect(),
                )
            })
            .unwrap_or_default();

    let mut sheet_names: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|e| e.name().to_owned()))
        .filter(|name| {
            let lower = name.to_ascii_lowercase();
            lower.starts_with("xl/worksheets/sheet")
                && Path::new(&lower)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("xml"))
        })
        .collect();
    sheet_names.sort();
    if sheet_names.is_empty() {
        push_unique(&mut issues, STRUCTURE, "xlsx_sheets");
        return Ok(VerifiedArtifact {
            report: report_for(path, "row_key", issues),
            bytes,
            snippets: Vec::new(),
        });
    }

    let mut non_empty_rows = 0_usize;
    let mut cells: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut qualified_cells: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut row_summaries: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut row_col_values: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut qualified_row_col: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut snippets: Vec<(String, String)> = Vec::new();
    let mut all_text = String::new();

    for (sheet_idx, sheet_name) in sheet_names.iter().enumerate() {
        let sheet_title = sheet_titles
            .get(sheet_idx)
            .cloned()
            .unwrap_or_else(|| format!("Sheet{}", sheet_idx + 1));
        let sheet_key = sheet_title.to_ascii_lowercase();
        let Some(sheet_xml) = read_zip_xml(&mut archive, sheet_name) else {
            push_unique(&mut issues, STRUCTURE, "xlsx_sheet");
            continue;
        };
        let Ok(doc) = roxmltree::Document::parse(&sheet_xml) else {
            push_unique(&mut issues, STRUCTURE, "xlsx_sheet");
            continue;
        };
        // Isolate column headers per sheet so Sheet 2 never inherits Sheet 1 headers.
        let mut headers: BTreeMap<String, String> = BTreeMap::new();
        for (row_idx, row) in doc
            .descendants()
            .filter(|node| node.is_element() && node.tag_name().name() == "row")
            .enumerate()
        {
            let mut row_cells: Vec<(String, String, String)> = Vec::new();
            for (col_idx, cell) in row
                .children()
                .filter(|node| {
                    node.is_element() && node.tag_name().name() == "c"
                })
                .enumerate()
            {
                let cell_ref = cell.attribute("r").map_or_else(
                    || format!("{}{}", column_letters(col_idx), row_idx + 1),
                    str::to_ascii_uppercase,
                );
                let col: String = cell_ref
                    .chars()
                    .take_while(char::is_ascii_alphabetic)
                    .collect();
                let raw = match cell.attribute("t") {
                    Some("s") => cell
                        .children()
                        .find(|n| n.is_element() && n.tag_name().name() == "v")
                        .and_then(|n| n.text())
                        .and_then(|s| s.trim().parse::<usize>().ok())
                        .and_then(|idx| shared_strings.get(idx))
                        .cloned()
                        .unwrap_or_default(),
                    Some("inlineStr") => cell
                        .descendants()
                        .filter(|n| {
                            n.is_element() && n.tag_name().name() == "t"
                        })
                        .filter_map(|n| n.text())
                        .collect::<String>(),
                    _ => {
                        let cached = cell
                            .children()
                            .find(|n| {
                                n.is_element() && n.tag_name().name() == "v"
                            })
                            .and_then(|n| n.text())
                            .map_or_default(str::trim);
                        if !cached.is_empty() {
                            cached.to_owned()
                        } else if let Some(formula) = cell
                            .children()
                            .find(|n| {
                                n.is_element() && n.tag_name().name() == "f"
                            })
                            .and_then(|n| n.text())
                            .map(str::trim)
                            .filter(|f| !f.is_empty())
                        {
                            format!("={formula}")
                        } else {
                            String::new()
                        }
                    }
                };
                let value = raw.trim().to_owned();
                if !value.is_empty() {
                    row_cells.push((col, cell_ref, value));
                }
            }
            if row_cells.is_empty() {
                continue;
            }
            non_empty_rows += 1;
            if headers.is_empty() {
                for (col, _, value) in &row_cells {
                    headers.insert(col.clone(), value.clone());
                }
            }
            let row_key = row_cells[0].2.clone();
            let summary = row_cells
                .iter()
                .map(|(_, _, val)| val.as_str())
                .collect::<Vec<_>>()
                .join(" | ");
            all_text.push_str(&summary);
            all_text.push('\n');
            snippets
                .push((format!("{sheet_title}!{row_key}"), summary.clone()));
            row_summaries
                .entry(row_key.clone())
                .or_default()
                .push(summary);
            for (col, cell_ref, value) in row_cells {
                qualified_cells
                    .entry(format!("{sheet_key}!{cell_ref}"))
                    .or_default()
                    .push(value.clone());
                cells.entry(cell_ref).or_default().push(value.clone());
                if let Some(header) = headers.get(&col) {
                    qualified_row_col
                        .entry(format!("{sheet_key}!{row_key}:{header}"))
                        .or_default()
                        .push(value.clone());
                    row_col_values
                        .entry(format!("{row_key}:{header}"))
                        .or_default()
                        .push(value);
                }
            }
        }
    }
    if non_empty_rows == 0 {
        push_unique(&mut issues, STRUCTURE, "xlsx_empty");
    }
    for anchor in expected {
        let lookup = if let Some((sheet_part, target)) =
            anchor.anchor.split_once('!')
        {
            let sheet_norm = sheet_part.trim().to_ascii_lowercase();
            let target_trimmed = target.trim();
            if target_trimmed.contains(':') {
                qualified_row_col.get(&format!("{sheet_norm}!{target_trimmed}"))
            } else if is_cell_ref(target_trimmed) {
                qualified_cells.get(&format!(
                    "{sheet_norm}!{}",
                    target_trimmed.to_ascii_uppercase()
                ))
            } else {
                row_summaries.get(target_trimmed)
            }
        } else if anchor.anchor.contains(':') {
            row_col_values.get(&anchor.anchor)
        } else if is_cell_ref(&anchor.anchor) {
            cells.get(&anchor.anchor.to_ascii_uppercase())
        } else {
            row_summaries.get(&anchor.anchor)
        };
        match lookup.map(Vec::as_slice) {
            None | Some([]) => {
                push_unique(&mut issues, ANCHOR_MISSING, &anchor.anchor);
            }
            Some([actual]) if actual != &anchor.value => {
                push_unique(&mut issues, VALUE_MISMATCH, &anchor.anchor);
            }
            Some([_]) => {}
            Some(_) => {
                push_unique(&mut issues, ANCHOR_DUPLICATE, &anchor.anchor);
            }
        }
    }
    if let Some(contract) = contract {
        contract.check_pinned_facts(&all_text, &mut issues);
    }
    Ok(VerifiedArtifact {
        report: report_for(path, "row_key", issues),
        bytes,
        snippets,
    })
}

fn report_for(
    path: &Path,
    anchor: &'static str,
    issues: Vec<VerifyIssue>,
) -> VerifyReport {
    VerifyReport {
        artifact: path.display().to_string(),
        anchor,
        ok: issues.is_empty(),
        issues,
    }
}

fn is_cell_ref(token: &str) -> bool {
    let bytes = token.as_bytes();
    let letters = bytes.iter().take_while(|b| b.is_ascii_alphabetic()).count();
    (1..=3).contains(&letters)
        && letters < bytes.len()
        && bytes[letters..].iter().all(u8::is_ascii_digit)
}

fn column_letters(mut index: usize) -> String {
    let mut out = Vec::new();
    loop {
        let rem = u8::try_from(index % 26).unwrap_or(0);
        out.push(b'A' + rem);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_else(|_| "A".to_owned())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

#[expect(
    clippy::too_many_arguments,
    reason = "shared verification finalizer keeps report, verified bytes, revision, failure budget, and telemetry in one path"
)]
async fn finalize_report(
    path: &Path,
    raw_path: &str,
    report: VerifyReport,
    verified_bytes: &[u8],
    snippets: Option<&[(String, String)]>,
    contract_revision: Option<u32>,
    failures: Option<&VerifyFailureTracker>,
    context: &ToolRunContext,
    store: Option<&DeliveryStore>,
    semantic: Option<&SemanticContext>,
) -> ToolExecutionResult {
    let mut report_id = String::new();
    let mut artifact_hash = delivery::hash_bytes(verified_bytes);
    if let Some(store) = store {
        let issues = report
            .issues
            .iter()
            .map(|issue| delivery::Issue {
                code: issue.code.to_owned(),
                field: issue.field.clone(),
            })
            .collect();
        match store.record_report_bytes_with_revision(
            path,
            verified_bytes,
            issues,
            contract_revision,
            now_ms(),
        ) {
            Ok(persisted) => {
                report_id = persisted.id;
                artifact_hash = persisted.artifact_hash;
            }
            Err(error) => {
                tracing::warn!(
                    target: "flowy.delivery",
                    %error,
                    path = %path.display(),
                    "could not persist the verification report"
                );
            }
        }
    }
    // Hard cap on rerender_once: the second failed verification of the same
    // artifact within one turn locks next_action to clarify_or_rebuild.
    let mut next_action = next_action_for(&report);
    if !report.ok
        && let Some(tracker) = failures
    {
        let key = delivery::artifact_key(path);
        let next_count = {
            let mut map = tracker
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let next = map.get(&key).copied().unwrap_or(0).saturating_add(1);
            map.insert(key, next);
            next
        };
        if next_count >= 2 {
            next_action = "clarify_or_rebuild";
        }
    }
    let artifact_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(raw_path);
    let (l3_outcome, l3_duration_ms) = if report.ok
        && let (Some(semantic_ctx), Some(extracted)) = (semantic, snippets)
        && !extracted.is_empty()
    {
        probe_deck_semantics(
            semantic_ctx,
            artifact_name,
            extracted,
            &context.signal,
        )
        .await
    } else {
        (SemanticOutcome::Skipped, 0)
    };
    let semantic_hint = l3_outcome.hint_text();
    let session_id = context.agent_id.as_deref().unwrap_or("");
    let submission_id = context.submission_id.as_deref().unwrap_or("");
    tracing::info!(
        target: "flowy.verify",
        session_id,
        root_session_id = session_id,
        submission_id,
        report_id = %report_id,
        artifact_hash = %artifact_hash,
        contract_revision = contract_revision.unwrap_or(0),
        anchor = report.anchor,
        ok = report.ok,
        next_action,
        issue_count = report.issues.len(),
        l3_status = l3_outcome.status_code(),
        l3_reason = l3_outcome.reason(),
        l3_issue_count = l3_outcome.issue_count(),
        l3_duration_ms,
        "artifact verification completed"
    );
    let summary = if report.ok {
        "verification passed".to_owned()
    } else {
        format!(
            "verification failed: {}",
            report
                .issues
                .iter()
                .map(|issue| issue.code)
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let value = report.to_value_with_meta(
        &summary,
        &report_id,
        &artifact_hash,
        next_action,
        contract_revision,
        semantic_hint.as_deref(),
    );
    let content = serde_json::to_string(&value).unwrap_or(summary);
    ToolExecutionResult::success_text(content, value)
}

async fn execute_deck(
    arguments: Value,
    context: &ToolRunContext,
    store: Option<&DeliveryStore>,
    semantic: Option<&SemanticContext>,
    contract: Option<&BriefContract>,
    failures: Option<&VerifyFailureTracker>,
) -> ToolExecutionResult {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Args {
        path: String,
        slides: usize,
        anchors: Vec<DeckAnchor>,
    }
    let Ok(args) = serde_json::from_value::<Args>(arguments) else {
        return ToolExecutionResult::error(
            "INVALID_ARGS",
            "expected path, slides and anchors",
        );
    };
    let path = resolve_turn_path(
        context.cwd.as_deref(),
        context.output_root.as_deref(),
        &args.path,
    );
    let revision = contract.map(|c| c.revision.max(1));
    match verify_deck_file_with_shapes(
        &path,
        args.slides,
        &args.anchors,
        contract,
    ) {
        Ok(verified) => {
            finalize_report(
                &path,
                &args.path,
                verified.report,
                &verified.bytes,
                Some(&verified.shapes),
                revision,
                failures,
                context,
                store,
                semantic,
            )
            .await
        }
        Err(error) => ToolExecutionResult::error(
            "VERIFY_FAILED",
            format!("cannot verify {}: {error}", args.path),
        ),
    }
}

async fn execute_artifact(
    arguments: Value,
    context: &ToolRunContext,
    store: Option<&DeliveryStore>,
    semantic: Option<&SemanticContext>,
    contract: Option<&BriefContract>,
    failures: Option<&VerifyFailureTracker>,
) -> ToolExecutionResult {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Args {
        path: String,
        #[serde(default)]
        slides: Option<usize>,
        #[serde(default)]
        anchors: Vec<DeckAnchor>,
    }
    let Ok(args) = serde_json::from_value::<Args>(arguments) else {
        return ToolExecutionResult::error(
            "INVALID_ARGS",
            "expected path and optional slides/anchors",
        );
    };
    let path = resolve_turn_path(
        context.cwd.as_deref(),
        context.output_root.as_deref(),
        &args.path,
    );
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase);
    let revision = contract.map(|c| c.revision.max(1));
    let result = match ext.as_deref() {
        Some("pptx") => match args.slides {
            Some(slides) => verify_deck_file_with_shapes(
                &path,
                slides,
                &args.anchors,
                contract,
            )
            .map(|verified| VerifiedArtifact {
                report: verified.report,
                bytes: verified.bytes,
                snippets: verified.shapes,
            }),
            None => Err(
                "pptx verification requires expected slide count (`slides`)"
                    .to_owned(),
            ),
        },
        Some("docx") => verify_docx_file(&path, &args.anchors, contract),
        Some("xlsx") => verify_xlsx_file(&path, &args.anchors, contract),
        _ => {
            return ToolExecutionResult::error(
                "UNSUPPORTED_FORMAT",
                format!(
                    "no deterministic verifier for {}; supported: .pptx, .docx, .xlsx",
                    args.path
                ),
            );
        }
    };
    match result {
        Ok(verified) => {
            finalize_report(
                &path,
                &args.path,
                verified.report,
                &verified.bytes,
                Some(&verified.snippets),
                revision,
                failures,
                context,
                store,
                semantic,
            )
            .await
        }
        Err(error) => ToolExecutionResult::error(
            "VERIFY_FAILED",
            format!("cannot verify {}: {error}", args.path),
        ),
    }
}

pub fn definition(store: Option<DeliveryStore>) -> Arc<ToolDefinition> {
    definition_with_options(store, None, None, None)
}

#[cfg(test)]
pub fn definition_with_semantic_and_contract(
    store: Option<DeliveryStore>,
    semantic: Option<SemanticContext>,
    contract: Option<BriefContract>,
) -> Arc<ToolDefinition> {
    definition_with_options(store, semantic, contract, None)
}

#[allow(clippy::redundant_pub_crate)]
pub(crate) fn definition_with_options(
    store: Option<DeliveryStore>,
    semantic: Option<SemanticContext>,
    contract: Option<BriefContract>,
    failures: Option<VerifyFailureTracker>,
) -> Arc<ToolDefinition> {
    Arc::new(ToolDefinition::new(
        "verify_deck",
        "Verify a generated deck before delivery. Checks host brief constraints, L1 per-slide title/body completeness, slide count, and each named shape against the expected text. Each anchor must be the slide shape name (for create_deck outputs: S0-title, S0-body, S1-title, S1-body, where slide 0 title is title and slide i>0 title is '{title} · {i+1}'). Returns ok, next_action ('publish' | 'rerender_once' | 'clarify_or_rebuild'), and ANCHOR_MISSING / ANCHOR_DUPLICATE / ANCHOR_INVALID / VALUE_MISMATCH / STRUCTURE issues; a failed report is not a tool error.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "minLength": 1},
                "slides": {"type": "integer", "minimum": 1},
                "anchors": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "anchor": {"type": "string", "minLength": 1},
                            "value": {"type": "string"}
                        },
                        "required": ["anchor", "value"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["path", "slides", "anchors"],
            "additionalProperties": false
        }),
        move |args: ToolCallArgs, context: ToolRunContext| {
            let store = store.clone();
            let semantic = semantic.clone();
            let contract = contract.clone();
            let failures = failures.clone();
            Box::pin(async move {
                execute_deck(
                    args.arguments,
                    &context,
                    store.as_ref(),
                    semantic.as_ref(),
                    contract.as_ref(),
                    failures.as_ref(),
                )
                .await
            })
        },
    ))
}

pub fn artifact_definition(
    store: Option<DeliveryStore>,
    contract: Option<BriefContract>,
) -> Arc<ToolDefinition> {
    artifact_definition_with_options(store, None, contract, None)
}

#[allow(clippy::redundant_pub_crate)]
pub(crate) fn artifact_definition_with_options(
    store: Option<DeliveryStore>,
    semantic: Option<SemanticContext>,
    contract: Option<BriefContract>,
    failures: Option<VerifyFailureTracker>,
) -> Arc<ToolDefinition> {
    Arc::new(ToolDefinition::new(
        "verify_artifact",
        "Verify a generated .docx, .xlsx, or .pptx artifact before calling publish_file. Checks OOXML structure, non-empty content, host brief constraints, and required non-empty anchors (Word bookmark names for .docx; row_key, 'row_key:ColumnHeader', 'Sheet1!B2', or cell ref like 'B2' for .xlsx; shape names + slides for .pptx). Persists a delivery gate report; a failed report is not a tool error.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "minLength": 1},
                "slides": {"type": "integer", "minimum": 1},
                "anchors": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "anchor": {"type": "string", "minLength": 1},
                            "value": {"type": "string"}
                        },
                        "required": ["anchor", "value"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["path", "anchors"],
            "additionalProperties": false
        }),
        move |args: ToolCallArgs, context: ToolRunContext| {
            let store = store.clone();
            let semantic = semantic.clone();
            let contract = contract.clone();
            let failures = failures.clone();
            Box::pin(async move {
                execute_artifact(
                    args.arguments,
                    &context,
                    store.as_ref(),
                    semantic.as_ref(),
                    contract.as_ref(),
                    failures.as_ref(),
                )
                .await
            })
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn anchor(name: &str, value: &str) -> DeckAnchor {
        DeckAnchor {
            anchor: name.to_owned(),
            value: value.to_owned(),
        }
    }

    fn pair(name: &str, value: &str) -> (String, String) {
        (name.to_owned(), value.to_owned())
    }

    fn write_zip(path: &Path, entries: &[(&str, &str)]) {
        let file = std::fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        for (name, body) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(body.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
    }

    fn tool_context(root: &Path) -> ToolRunContext {
        ToolRunContext {
            signal: flowy_agent_core::CancelToken::new(),
            agent_id: Some("s-verify".to_owned()),
            cwd: Some(root.display().to_string()),
            todos: None,
            submission_id: Some("sub-verify".to_owned()),
            session_origin: flowy_agent_core::SessionOrigin::Home,
            output_root: None,
            artifact_delivery_available: true,
        }
    }

    #[test]
    fn every_code_is_reported_distinctly() {
        let deck = vec![
            pair("S0-title", "T"),
            pair("S0-title", "T"),
            pair("S0-extra", "x"),
        ];
        let expected = vec![
            anchor("S0-title", "T"),
            anchor("S0-body", "one"),
            anchor("S0-value", "wrong"),
        ];
        let report = evaluate("deck.pptx", 2, 1, &deck, &expected);
        assert!(!report.ok);
        let codes: Vec<&str> =
            report.issues.iter().map(|issue| issue.code).collect();
        assert!(codes.contains(&STRUCTURE));
        assert!(codes.contains(&ANCHOR_DUPLICATE));
        assert!(codes.contains(&ANCHOR_MISSING));
        assert!(codes.contains(&ANCHOR_INVALID));
    }

    #[test]
    fn brief_contract_extracts_counts_and_skips_ordinals() {
        let east =
            BriefContract::from_brief("请做一份3页华东区汇报 q3-east.pptx");
        assert_eq!(east.min_slides, Some(3));
        assert_eq!(east.expected_filenames, vec!["q3-east.pptx"]);

        assert_eq!(
            BriefContract::from_brief("制作两页总结").min_slides,
            Some(2)
        );
        assert_eq!(
            BriefContract::from_brief("Create a 4-slide review").min_slides,
            Some(4)
        );
        assert_eq!(
            BriefContract::from_brief("把第3页标题改成Q4").min_slides,
            None
        );
    }

    #[tokio::test]
    async fn shrinking_deck_below_brief_min_slides_fails_even_when_worker_lowers_declared_slides()
     {
        let root = std::env::temp_dir()
            .join(format!("flowy-verify-anticheat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let deck = root.join("q3-east.pptx");
        crate::pptx::write_deck(
            &deck,
            "华东区Q3汇报",
            &["上海6.1亿".to_owned(), "江苏4.2亿".to_owned()],
        )
        .unwrap();
        let store = DeliveryStore::new(root.join("delivery"));
        let contract = BriefContract::from_brief(
            "请生成一份3页华东区汇报 q3-east.pptx 并核验后交付",
        );
        let tool = definition_with_semantic_and_contract(
            Some(store.clone()),
            None,
            Some(contract),
        );
        let args = ToolCallArgs {
            call_id: "call-cheat".to_owned(),
            name: "verify_deck".to_owned(),
            arguments: json!({
                "path": "q3-east.pptx",
                "slides": 2,
                "anchors": [
                    {"anchor": "S0-title", "value": "华东区Q3汇报"},
                    {"anchor": "S0-body", "value": "上海6.1亿"},
                    {"anchor": "S1-title", "value": "华东区Q3汇报 · 2"},
                    {"anchor": "S1-body", "value": "江苏4.2亿"},
                ],
            }),
        };
        let ToolExecutionResult::Success { value, .. } =
            (tool.execute)(args, tool_context(&root)).await
        else {
            panic!("failed verification must return a structured report");
        };
        assert_eq!(value["ok"], false);
        assert_eq!(value["next_action"], "clarify_or_rebuild");
        let issues = value["issues"].as_array().unwrap();
        assert!(issues.iter().any(|item| {
            item["code"] == STRUCTURE && item["field"] == "brief_min_slides"
        }));
        assert_eq!(
            store.check(&deck).unwrap(),
            Some(delivery::BlockReason::VerificationFailed)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn docx_and_xlsx_verifiers_pass_valid_ooxml_and_block_broken_archives()
     {
        let root = std::env::temp_dir()
            .join(format!("flowy-verify-ooxml-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = DeliveryStore::new(root.join("delivery"));

        let docx = root.join("memo.docx");
        write_zip(
            &docx,
            &[(
                "word/document.xml",
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="0" w:name="summary"/><w:r><w:t>Q3 revenue &amp; growth</w:t></w:r></w:p></w:body></w:document>"#,
            )],
        );
        let xlsx = root.join("sales.xlsx");
        write_zip(
            &xlsx,
            &[
                ("xl/workbook.xml", "<workbook/>"),
                (
                    "xl/sharedStrings.xml",
                    "<sst><si><t>Region</t></si><si><t>Revenue</t></si><si><t>East</t></si></sst>",
                ),
                (
                    "xl/worksheets/sheet1.xml",
                    r#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row><row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2"><v>6.1</v></c></row></sheetData></worksheet>"#,
                ),
            ],
        );
        let broken = root.join("broken.xlsx");
        std::fs::write(&broken, b"hello").unwrap();

        let tool = artifact_definition(Some(store.clone()), None);
        let docx_res = (tool.execute)(
            ToolCallArgs {
                call_id: "c-docx".to_owned(),
                name: "verify_artifact".to_owned(),
                arguments: json!({
                    "path": "memo.docx",
                    "anchors": [{"anchor": "summary", "value": "Q3 revenue & growth"}],
                }),
            },
            tool_context(&root),
        )
        .await;
        let ToolExecutionResult::Success { value: dv, .. } = docx_res else {
            panic!("docx verification failed");
        };
        assert_eq!(dv["ok"], true);
        assert_eq!(dv["anchor"], "bookmark");
        assert_eq!(store.check(&docx).unwrap(), None);

        let xlsx_res = (tool.execute)(
            ToolCallArgs {
                call_id: "c-xlsx".to_owned(),
                name: "verify_artifact".to_owned(),
                arguments: json!({
                    "path": "sales.xlsx",
                    "anchors": [
                        {"anchor": "East:Revenue", "value": "6.1"},
                        {"anchor": "B2", "value": "6.1"},
                        {"anchor": "East", "value": "East | 6.1"},
                    ],
                }),
            },
            tool_context(&root),
        )
        .await;
        let ToolExecutionResult::Success { value: xv, .. } = xlsx_res else {
            panic!("xlsx verification failed");
        };
        assert_eq!(xv["ok"], true);
        assert_eq!(xv["anchor"], "row_key");
        assert_eq!(store.check(&xlsx).unwrap(), None);

        let broken_res = (tool.execute)(
            ToolCallArgs {
                call_id: "c-broken".to_owned(),
                name: "verify_artifact".to_owned(),
                arguments: json!({ "path": "broken.xlsx" }),
            },
            tool_context(&root),
        )
        .await;
        let ToolExecutionResult::Success { value: bv, .. } = broken_res else {
            panic!("broken archive must return structured failed report");
        };
        assert_eq!(bv["ok"], false);
        assert_eq!(
            store.check(&broken).unwrap(),
            Some(delivery::BlockReason::VerificationFailed)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn followup_contract_inherits_slides_and_quote_while_updating_date() {
        let root = std::env::temp_dir()
            .join(format!("flowy-verify-followup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = DeliveryStore::new(root.join("delivery"));

        // Reference document count ("参考那份20页的旧报告") must not override target "3页".
        let rev1 = BriefContract::from_brief(
            "参考那份20页的旧报告，请做报价表＋3页汇报，报价 35 万，日期 2026-11-25",
        );
        assert_eq!(rev1.revision, 1);
        assert_eq!(rev1.min_slides, Some(3));
        assert_eq!(rev1.required_items.len(), 2);
        assert_eq!(
            rev1.pinned_facts.get("quote").map(String::as_str),
            Some("35万")
        );
        assert_eq!(
            rev1.pinned_facts.get("date").map(String::as_str),
            Some("2026-11-25")
        );

        // Turn 2 only edits the date: must keep min_slides=3, required_items=2, quote=35万.
        let rev2 = BriefContract::evolve(
            Some(&rev1.to_work_contract()),
            "只改一下日期为 2026-12-01",
        );
        assert_eq!(rev2.revision, 2);
        assert_eq!(rev2.min_slides, Some(3));
        assert_eq!(rev2.required_items.len(), 2);
        assert_eq!(
            rev2.pinned_facts.get("quote").map(String::as_str),
            Some("35万")
        );
        assert_eq!(
            rev2.pinned_facts.get("date").map(String::as_str),
            Some("2026-12-01")
        );

        let tool = definition_with_options(
            Some(store.clone()),
            None,
            Some(rev2.clone()),
            None,
        );

        // Wrong quote (38万 instead of inherited 35万) must fail VALUE_MISMATCH/brief_quote.
        let wrong_quote = root.join("wrong-quote.pptx");
        crate::pptx::write_deck(
            &wrong_quote,
            "项目汇报",
            &[
                "日期 2026-12-01".to_owned(),
                "报价 38万".to_owned(),
                "结论".to_owned(),
            ],
        )
        .unwrap();
        let ToolExecutionResult::Success { value: wq, .. } = (tool.execute)(
            ToolCallArgs {
                call_id: "c-wq".to_owned(),
                name: "verify_deck".to_owned(),
                arguments: json!({
                    "path": "wrong-quote.pptx",
                    "slides": 3,
                    "anchors": [
                        {"anchor": "S0-title", "value": "项目汇报"},
                        {"anchor": "S0-body", "value": "日期 2026-12-01"},
                        {"anchor": "S1-title", "value": "项目汇报 · 2"},
                        {"anchor": "S1-body", "value": "报价 38万"},
                        {"anchor": "S2-title", "value": "项目汇报 · 3"},
                        {"anchor": "S2-body", "value": "结论"},
                    ],
                }),
            },
            tool_context(&root),
        )
        .await
        else {
            panic!("expected structured report");
        };
        assert_eq!(wq["ok"], false);
        assert_eq!(wq["contract_revision"], 2);
        assert!(wq["issues"].as_array().unwrap().iter().any(|i| {
            i["code"] == VALUE_MISMATCH && i["field"] == "brief_quote"
        }));

        // Correct 3 slides + inherited 35万 + updated 2026-12-01 passes and tags rev=2.
        let good = root.join("good-rev2.pptx");
        crate::pptx::write_deck(
            &good,
            "项目汇报",
            &[
                "日期 2026-12-01".to_owned(),
                "报价 35万".to_owned(),
                "结论".to_owned(),
            ],
        )
        .unwrap();
        let ToolExecutionResult::Success { value: ok_val, .. } =
            (tool.execute)(
                ToolCallArgs {
                    call_id: "c-ok".to_owned(),
                    name: "verify_deck".to_owned(),
                    arguments: json!({
                        "path": "good-rev2.pptx",
                        "slides": 3,
                        "anchors": [
                            {"anchor": "S0-title", "value": "项目汇报"},
                            {"anchor": "S0-body", "value": "日期 2026-12-01"},
                            {"anchor": "S1-title", "value": "项目汇报 · 2"},
                            {"anchor": "S1-body", "value": "报价 35万"},
                            {"anchor": "S2-title", "value": "项目汇报 · 3"},
                            {"anchor": "S2-body", "value": "结论"},
                        ],
                    }),
                },
                tool_context(&root),
            )
            .await
        else {
            panic!("expected structured report");
        };
        assert_eq!(ok_val["ok"], true);
        assert_eq!(ok_val["contract_revision"], 2);
        assert_eq!(
            store
                .latest_report(&good)
                .unwrap()
                .unwrap()
                .contract_revision,
            Some(2)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn empty_anchors_blocked_and_multisheet_xlsx_isolates_headers_and_formulas()
     {
        let root = std::env::temp_dir()
            .join(format!("flowy-verify-multisheet-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = DeliveryStore::new(root.join("delivery"));

        let multi = root.join("quote.xlsx");
        write_zip(
            &multi,
            &[
                (
                    "xl/workbook.xml",
                    r#"<workbook><sheets><sheet name="Summary" sheetId="1"/><sheet name="Detail" sheetId="2"/></sheets></workbook>"#,
                ),
                (
                    "xl/worksheets/sheet1.xml",
                    r#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Item</t></is></c><c r="B1" t="inlineStr"><is><t>Price</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>Total</t></is></c><c r="B2"><f>SUM(Detail!B2:B3)</f></c></row></sheetData></worksheet>"#,
                ),
                (
                    "xl/worksheets/sheet2.xml",
                    r#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>SKU</t></is></c><c r="B1" t="inlineStr"><is><t>Cost</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>A-1</t></is></c><c r="B2"><v>350000</v></c></row></sheetData></worksheet>"#,
                ),
            ],
        );

        let tool = artifact_definition(Some(store.clone()), None);
        // Empty anchors must be refused as STRUCTURE/anchors.
        let ToolExecutionResult::Success {
            value: empty_res, ..
        } = (tool.execute)(
            ToolCallArgs {
                call_id: "c-empty".to_owned(),
                name: "verify_artifact".to_owned(),
                arguments: json!({
                    "path": "quote.xlsx",
                    "anchors": [],
                }),
            },
            tool_context(&root),
        )
        .await
        else {
            panic!("expected structured failed report");
        };
        assert_eq!(empty_res["ok"], false);
        assert!(
            empty_res["issues"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| { i["code"] == STRUCTURE && i["field"] == "anchors" })
        );

        // Sheet-qualified cells, isolated Sheet 2 headers, and formula cell without <v> all pass.
        let ToolExecutionResult::Success { value: ok_res, .. } = (tool.execute)(
            ToolCallArgs {
                call_id: "c-multi".to_owned(),
                name: "verify_artifact".to_owned(),
                arguments: json!({
                    "path": "quote.xlsx",
                    "anchors": [
                        {"anchor": "Summary!A1", "value": "Item"},
                        {"anchor": "Detail!A1", "value": "SKU"},
                        {"anchor": "Detail!A-1:Cost", "value": "350000"},
                        {"anchor": "Summary!B2", "value": "=SUM(Detail!B2:B3)"},
                    ],
                }),
            },
            tool_context(&root),
        )
        .await
        else {
            panic!("expected structured report");
        };
        assert_eq!(ok_res["ok"], true, "got {ok_res}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn second_value_mismatch_locks_rerender_once_to_clarify_or_rebuild() {
        let root = std::env::temp_dir()
            .join(format!("flowy-verify-rerender-cap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let deck = root.join("cap.pptx");
        crate::pptx::write_deck(&deck, "T", &["actual".to_owned()]).unwrap();
        let failures: VerifyFailureTracker =
            Arc::new(Mutex::new(HashMap::new()));
        let tool = definition_with_options(None, None, None, Some(failures));
        let call = || ToolCallArgs {
            call_id: "c-cap".to_owned(),
            name: "verify_deck".to_owned(),
            arguments: json!({
                "path": "cap.pptx",
                "slides": 1,
                "anchors": [
                    {"anchor": "S0-title", "value": "T"},
                    {"anchor": "S0-body", "value": "expected-different"},
                ],
            }),
        };
        let ToolExecutionResult::Success { value: first, .. } =
            (tool.execute)(call(), tool_context(&root)).await
        else {
            panic!("first report");
        };
        assert_eq!(first["ok"], false);
        assert_eq!(first["next_action"], "rerender_once");

        let ToolExecutionResult::Success { value: second, .. } =
            (tool.execute)(call(), tool_context(&root)).await
        else {
            panic!("second report");
        };
        assert_eq!(second["ok"], false);
        assert_eq!(
            second["next_action"], "clarify_or_rebuild",
            "second failure on same artifact in one turn must lock next_action"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

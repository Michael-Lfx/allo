//! Host-owned brief contract extraction and cross-revision inheritance.

use super::{
    BTreeMap, Path, RequiredDeliverable, STRUCTURE, SlideL1Stats,
    VALUE_MISMATCH, VerifyIssue, WorkContract, push_unique,
};

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

    pub(super) fn check_filename(
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

    pub(super) fn check_pinned_facts(
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

    pub(super) fn check_deck(
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

pub(super) fn extract_explicit_slide_count(brief: &str) -> Option<usize> {
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

pub(super) fn parse_chinese_numeral(
    slice: &[(usize, char)],
) -> Option<(usize, usize)> {
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

pub(super) fn extract_explicit_filenames(brief: &str) -> Vec<String> {
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

pub(super) fn extract_required_items(
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

pub(super) fn extract_pinned_facts(brief: &str) -> BTreeMap<String, String> {
    let mut facts = BTreeMap::new();
    if let Some(quote) = extract_quote_fact(brief) {
        facts.insert("quote".to_owned(), quote);
    }
    if let Some(date) = extract_date_fact(brief) {
        facts.insert("date".to_owned(), date);
    }
    facts
}

pub(super) fn extract_quote_fact(brief: &str) -> Option<String> {
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

pub(super) fn extract_date_fact(brief: &str) -> Option<String> {
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

pub(super) fn compact_text(input: &str) -> String {
    input
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != ',')
        .flat_map(char::to_lowercase)
        .collect()
}

pub(super) fn quote_matches(
    normalized_doc: &str,
    expected_quote: &str,
) -> bool {
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

pub(super) fn date_matches(normalized_doc: &str, expected_date: &str) -> bool {
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

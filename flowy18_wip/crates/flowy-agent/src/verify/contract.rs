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
        let is_rename = is_rename_prompt(prompt);
        let prompt_lower = prompt.to_ascii_lowercase();
        for item in delta.required_items {
            if is_rename && let Some(new_name) = item.filename.as_deref() {
                // Prefer replacing the specifically mentioned previous filename
                // so multi-file decks (e.g. `east.pptx` + `west.pptx`) never
                // lose sibling deliverables when renaming one file.
                if let Some(slot) = required_items.iter_mut().find(|existing| {
                    existing.extension.eq_ignore_ascii_case(&item.extension)
                        && existing.filename.as_deref().is_some_and(|old| {
                            !old.eq_ignore_ascii_case(new_name)
                                && prompt_lower
                                    .contains(&old.to_ascii_lowercase())
                        })
                }) {
                    *slot = item;
                    continue;
                }
                let same_ext_indices: Vec<usize> = required_items
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, existing)| {
                        existing
                            .extension
                            .eq_ignore_ascii_case(&item.extension)
                            .then_some(idx)
                    })
                    .collect();
                if same_ext_indices.len() == 1 {
                    required_items[same_ext_indices[0]] = item;
                    continue;
                }
            }
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
    let mut matches: Vec<usize> = Vec::new();
    for idx in 0..chars.len() {
        // Skip ordinals ("第1页"/"第十二页") and middle digits of multi-char
        // Chinese numerals ("十五页" -> parse "十五"=15 at idx of '十', skip '五').
        if idx > 0 {
            let prev_ch = chars[idx - 1].1;
            if prev_ch == '第' || is_cn_numeral_char(prev_ch) {
                continue;
            }
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
            matches.push(num);
        }
    }
    if matches.len() >= 2 {
        let lower = brief.to_ascii_lowercase();
        let has_change_cue = [
            "改", "调", "精简", "缩减", "变", "至", "到", "->", "→", " to ",
        ]
        .iter()
        .any(|cue| lower.contains(cue));
        if has_change_cue {
            return matches.last().copied();
        }
    }
    matches.into_iter().max()
}

const fn is_cn_numeral_char(ch: char) -> bool {
    matches!(
        ch,
        '一' | '二'
            | '两'
            | '三'
            | '四'
            | '五'
            | '六'
            | '七'
            | '八'
            | '九'
            | '十'
    )
}

const fn cn_digit(ch: char) -> Option<usize> {
    match ch {
        '一' => Some(1),
        '二' | '两' => Some(2),
        '三' => Some(3),
        '四' => Some(4),
        '五' => Some(5),
        '六' => Some(6),
        '七' => Some(7),
        '八' => Some(8),
        '九' => Some(9),
        _ => None,
    }
}

pub(super) fn parse_chinese_numeral(
    slice: &[(usize, char)],
) -> Option<(usize, usize)> {
    let first = slice.first()?.1;
    let second = slice.get(1).map(|(_, ch)| *ch);
    let third = slice.get(2).map(|(_, ch)| *ch);
    if first == '十' {
        if let Some(ones) = second.and_then(cn_digit) {
            return Some((10 + ones, 2));
        }
        return Some((10, 1));
    }
    let first_val = cn_digit(first)?;
    if second == Some('十') {
        let base = first_val * 10;
        if let Some(ones) = third.and_then(cn_digit) {
            let total = base + ones;
            return (total <= 50).then_some((total, 3));
        }
        return (base <= 50).then_some((base, 2));
    }
    Some((first_val, 1))
}

fn is_rename_prompt(prompt: &str) -> bool {
    let lower = prompt.to_ascii_lowercase();
    ["改名", "重命名", "更名", "rename", "文件名改"]
        .iter()
        .any(|marker| lower.contains(marker))
        || (prompt.contains("文件名")
            && ["改", "换", "变"].iter().any(|verb| prompt.contains(verb)))
}

fn rfind_marker(haystack: &str, marker: &str) -> Option<usize> {
    let starts_ascii = marker
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric());
    let ends_ascii = marker
        .chars()
        .next_back()
        .is_some_and(|c| c.is_ascii_alphanumeric());
    if !starts_ascii && !ends_ascii {
        return haystack.rfind(marker);
    }
    for (pos, _) in haystack.rmatch_indices(marker) {
        let before_ok = !starts_ascii
            || haystack[..pos]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_ascii_alphanumeric());
        let after_ok = !ends_ascii
            || haystack[pos + marker.len()..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_ascii_alphanumeric());
        if before_ok && after_ok {
            return Some(pos);
        }
    }
    None
}

fn is_input_source_context(prefix: &str) -> bool {
    let lower = prefix.to_ascii_lowercase();
    let input_markers = [
        "根据", "基于", "参考", "读取", "按照", "按", "依据", "从", "由",
        "原有", "原", "based on", "using", "from", "read",
    ];
    let output_markers = [
        "生成",
        "输出",
        "制作",
        "整理",
        "保存为",
        "导出为",
        "命名为",
        "改名为",
        "重命名为",
        "更名为",
        "改为",
        "改成",
        "交付",
        "做",
        "写",
        "叫",
        "create",
        "build",
        "make",
        "generate",
        "deliver",
        "publish",
        "save as",
        "rename to",
        "to",
        "output",
    ];
    let last_input = input_markers
        .iter()
        .filter_map(|m| rfind_marker(&lower, m))
        .max();
    let Some(in_pos) = last_input else {
        return false;
    };
    let last_output = output_markers
        .iter()
        .filter_map(|m| rfind_marker(&lower, m))
        .max();
    last_output.is_none_or(|out_pos| in_pos > out_pos)
}

pub(super) fn extract_explicit_filenames(brief: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cursor = 0_usize;
    for raw_token in brief.split(|ch: char| {
        !(ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    }) {
        let token_pos = brief[cursor..]
            .find(raw_token)
            .map_or(cursor, |rel| cursor + rel);
        cursor = token_pos + raw_token.len();
        let candidate = raw_token.trim_matches('.');
        let lower = candidate.to_ascii_lowercase();
        let is_target = [".pptx", ".docx", ".xlsx", ".pdf", ".csv"]
            .iter()
            .any(|ext| lower.ends_with(ext) && lower.len() > ext.len());
        if !is_target {
            continue;
        }
        // Look back up to 24 chars before this filename (stopping at sentence
        // boundaries) to ignore input source files like "根据 source.csv 做 q3.pptx".
        let prefix_full = &brief[..token_pos];
        let clause_prefix = prefix_full
            .rsplit(['。', '\n', ';', '；', '，', ','])
            .next()
            .unwrap_or(prefix_full);
        let tail_chars: String = clause_prefix
            .chars()
            .rev()
            .take(24)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if is_input_source_context(&tail_chars) {
            continue;
        }
        if !out
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

fn split_quote_clause(after: &str) -> &str {
    for (idx, ch) in after.char_indices() {
        if matches!(ch, '，' | '。' | ';' | '；' | '\n') {
            return &after[..idx];
        }
        if ch == ',' {
            let next_is_digit = after[idx + 1..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit());
            if !next_is_digit {
                return &after[..idx];
            }
        }
    }
    after
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
            // Limit scan to the current clause without breaking thousand
            // separators like `350,000`.
            let clause = split_quote_clause(after);
            let window: String = clause.chars().take(48).collect();
            let chars: Vec<(usize, char)> = window.char_indices().collect();
            // Collect valid price numbers in this clause: `(start_char_idx,
            // end_char_idx, formatted_quote)`.
            let mut found: Vec<(usize, usize, String)> = Vec::new();
            let mut idx = 0_usize;
            while idx < chars.len() {
                if !chars[idx].1.is_ascii_digit() {
                    idx += 1;
                    continue;
                }
                let start_idx = idx;
                let mut end_idx = start_idx;
                while end_idx < chars.len() {
                    let ch = chars[end_idx].1;
                    let is_internal_sep = matches!(ch, '.' | ',')
                        && chars
                            .get(end_idx + 1)
                            .is_some_and(|(_, next)| next.is_ascii_digit());
                    if ch.is_ascii_digit() || is_internal_sep {
                        end_idx += 1;
                    } else {
                        break;
                    }
                }
                let start_byte = chars[start_idx].0;
                let end_byte =
                    chars.get(end_idx).map_or(window.len(), |(p, _)| *p);
                let number = window[start_byte..end_byte]
                    .replace(',', "")
                    .trim_end_matches('.')
                    .to_owned();
                let tail = window[end_byte..].trim_start();
                let tail_lower = tail.to_ascii_lowercase();
                let is_non_currency_unit = tail.starts_with('页')
                    || tail.starts_with('张')
                    || tail.starts_with('年')
                    || tail.starts_with('月')
                    || tail.starts_with('日')
                    || tail.starts_with('期')
                    || tail.starts_with('%')
                    || tail.starts_with('倍')
                    || tail.starts_with('人')
                    || tail.starts_with('个')
                    || tail.starts_with("小时")
                    || tail.starts_with('天')
                    || tail_lower.starts_with("slide")
                    || (tail.starts_with('-')
                        && tail[1..]
                            .chars()
                            .next()
                            .is_some_and(|c| c.is_ascii_digit()));
                if !number.is_empty() && !is_non_currency_unit {
                    let unit = if tail.starts_with("万元")
                        || tail.starts_with('万')
                    {
                        "万"
                    } else if tail.starts_with("亿元") || tail.starts_with('亿')
                    {
                        "亿"
                    } else if tail.starts_with('元') {
                        "元"
                    } else {
                        ""
                    };
                    found.push((start_idx, end_idx, format!("{number}{unit}")));
                }
                idx = end_idx;
            }
            let Some((first_start, _, _)) = found.first() else {
                continue;
            };
            let prefix_ok = chars[..*first_start].iter().all(|(_, c)| {
                c.is_whitespace()
                    || matches!(
                        c,
                        ':' | '：'
                            | '='
                            | '为'
                            | '是'
                            | '约'
                            | '共'
                            | '改'
                            | '成'
                            | '调'
                            | '整'
                            | '变'
                            | '更'
                            | '新'
                            | '至'
                            | '到'
                            | '从'
                            | '由'
                            | '¥'
                            | '￥'
                            | '$'
                            | '('
                            | '（'
                            | 't'
                            | 'o'
                            | 'T'
                            | 'O'
                    )
            });
            if !prefix_ok {
                continue;
            }
            // Start with `found[0]` and only advance to a subsequent price when
            // the intervening text contains an explicit replacement cue (so
            // "报价 35 万（有效期至 2026-12-01）" or "报价 35 万分 3 期" never
            // overwrites the quote with an unrelated trailing number).
            let mut chosen = 0_usize;
            for next_i in 1..found.len() {
                let prev_end = found[chosen].1;
                let next_start = found[next_i].0;
                let between: String = chars[prev_end..next_start]
                    .iter()
                    .map(|(_, c)| *c)
                    .collect();
                let between_lower = between.to_ascii_lowercase();
                let is_replacement =
                    ["改", "调", "变", "至", "到", "成", "->", "→", " to "]
                        .iter()
                        .any(|cue| between_lower.contains(cue))
                        && !between.contains("有效")
                        && !between.contains("截止");
                if is_replacement {
                    chosen = next_i;
                }
            }
            return Some(found[chosen].2.clone());
        }
    }
    None
}

pub(super) fn extract_date_fact(brief: &str) -> Option<String> {
    let mut matches: Vec<(usize, String)> = Vec::new();
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
                    matches.push((i, format!("{year}-{m:02}-{d:02}")));
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
                        matches.push((
                            chars[i - 4].0,
                            format!("{y}-{m:02}-{d:02}"),
                        ));
                    }
                }
            }
        }
    }
    matches.sort_by_key(|(pos, _)| *pos);
    if matches.len() >= 2 {
        let lower = brief.to_ascii_lowercase();
        let has_change_verb =
            ["改", "调", "变", "换", "更新", "->", "→", " to "]
                .iter()
                .any(|marker| lower.contains(marker));
        if has_change_verb {
            return matches.last().map(|(_, date)| date.clone());
        }
    }
    matches.first().map(|(_, date)| date.clone())
}

pub(super) fn compact_text(input: &str) -> String {
    input
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != ',')
        .flat_map(char::to_lowercase)
        .collect()
}

fn contains_with_numeric_boundary(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let needle_starts_digit =
        needle.chars().next().is_some_and(|c| c.is_ascii_digit());
    let needle_ends_digit = needle
        .chars()
        .next_back()
        .is_some_and(|c| c.is_ascii_digit());
    for (pos, _) in haystack.match_indices(needle) {
        let before_ok = if needle_starts_digit {
            haystack[..pos]
                .chars()
                .next_back()
                .is_none_or(|c| !(c.is_ascii_digit() || c == '.'))
        } else {
            true
        };
        let after_ok = if needle_ends_digit {
            let after_slice = &haystack[pos + needle.len()..];
            match after_slice.chars().next() {
                Some(c) if c.is_ascii_digit() => false,
                Some('.') => {
                    // Allow sentence-ending periods (`350000.`) and zero
                    // fractional decimals (`350000.00`), while rejecting
                    // distinct decimal values (`350000.5` / `35.5`).
                    let after_dot = &after_slice[1..];
                    let frac: String = after_dot
                        .chars()
                        .take_while(char::is_ascii_digit)
                        .collect();
                    frac.is_empty() || frac.chars().all(|d| d == '0')
                }
                None | Some(_) => true,
            }
        } else {
            true
        };
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

pub(super) fn quote_matches(
    normalized_doc: &str,
    expected_quote: &str,
) -> bool {
    let target = compact_text(expected_quote);
    if contains_with_numeric_boundary(normalized_doc, &target) {
        return true;
    }
    if let Some(wan) = target.strip_suffix('万')
        && let Ok(val) = wan.parse::<f64>()
    {
        let expanded = format!("{}", (val * 10_000.0).round() as u64);
        if contains_with_numeric_boundary(normalized_doc, &expanded) {
            return true;
        }
    }
    false
}

pub(super) fn date_matches(normalized_doc: &str, expected_date: &str) -> bool {
    let target = compact_text(expected_date);
    if contains_with_numeric_boundary(normalized_doc, &target) {
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
        return [slash, slash_short, dash_short, cn, cn_pad].iter().any(
            |variant| contains_with_numeric_boundary(normalized_doc, variant),
        );
    }
    false
}

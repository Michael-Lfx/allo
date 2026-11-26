//! Multi-object work-contract resolution, proposal/record isolation, and
//! deterministic disambiguation (`最终方案.md` §12.5 A2).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};

use flowy_agent_core::{
    DisambiguationOption, FactEntry, FactStatus, ObjectVerifyState,
    PendingCard, TrackedObject, TrackedObjectKind, WorkContract,
};

use super::contract::{
    BriefContract, contains_deck_role_word, extract_date_fact,
    extract_quote_fact, is_rename_prompt,
};

/// Output of resolving a user prompt against an optional existing [`WorkContract`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolveOutcome {
    /// Unambiguous contract update ready to freeze & dispatch.
    Resolved {
        contract: BriefContract,
        target_object_ids: Vec<String>,
        used_llm: bool,
    },
    /// Ambiguous multi-object mutation requiring user disambiguation before starting a worker.
    Ambiguous { card: PendingCard, used_llm: bool },
}

/// Populate initial [`TrackedObject`] entries for a fresh [`BriefContract`].
pub(super) fn build_initial_objects(
    contract: &BriefContract,
) -> Vec<TrackedObject> {
    let mut objects = Vec::with_capacity(contract.required_items.len());
    for (idx, item) in contract.required_items.iter().enumerate() {
        let object_id = format!("obj-{}-{}", item.extension, idx + 1);
        let mut obj = TrackedObject {
            object_id,
            kind: TrackedObjectKind::Deliverable,
            filename: item.filename.clone(),
            extension: item.extension.clone(),
            anchor_summary: String::new(),
            overrides: BTreeMap::new(),
            verify_state: ObjectVerifyState::Unverified,
        };
        obj.anchor_summary =
            format_anchor_summary(&obj, &contract.pinned_facts);
        objects.push(obj);
    }
    objects
}

fn format_anchor_summary(
    obj: &TrackedObject,
    session_facts: &BTreeMap<String, String>,
) -> String {
    let mut parts = Vec::new();
    let label = match obj.filename.as_deref() {
        Some(name) => name.to_owned(),
        None if obj.kind == TrackedObjectKind::Record => {
            "会议记录归档".to_owned()
        }
        None if obj.extension.eq_ignore_ascii_case("xlsx") => {
            "报价表 (*.xlsx)".to_owned()
        }
        None => format!("*.{}", obj.extension),
    };
    parts.push(label);
    let effective_date = obj
        .overrides
        .get("date")
        .filter(|entry| entry.status == FactStatus::Confirmed)
        .map(|entry| entry.value.as_str())
        .or_else(|| session_facts.get("date").map(String::as_str));
    if let Some(date) = effective_date {
        parts.push(date.to_owned());
    }
    let effective_quote = obj
        .overrides
        .get("quote")
        .filter(|entry| entry.status == FactStatus::Confirmed)
        .map(|entry| entry.value.as_str())
        .or_else(|| session_facts.get("quote").map(String::as_str));
    if let Some(quote) = effective_quote {
        parts.push(quote.to_owned());
    }
    parts.join(" · ")
}

fn refresh_anchor_summaries(
    objects: &mut [TrackedObject],
    session_facts: &BTreeMap<String, String>,
) {
    for obj in objects {
        if obj.kind == TrackedObjectKind::Deliverable {
            obj.anchor_summary = format_anchor_summary(obj, session_facts);
        }
    }
}

fn ensure_objects_seeded(prev: &WorkContract) -> Vec<TrackedObject> {
    if !prev.objects.is_empty() {
        return prev.objects.clone();
    }
    let synthetic = BriefContract::from_work_contract(prev);
    build_initial_objects(&synthetic)
}

fn next_object_id(objects: &[TrackedObject], prefix: &str) -> String {
    let mut seq = objects.len() + 1;
    loop {
        let candidate = format!("{prefix}-{seq}");
        if !objects.iter().any(|obj| obj.object_id == candidate) {
            return candidate;
        }
        seq += 1;
    }
}

fn next_card_id(now_ms: u64) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!("card-{now_ms}-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

fn record_object_ids(objects: &[TrackedObject]) -> Vec<String> {
    objects
        .iter()
        .filter(|o| o.kind == TrackedObjectKind::Record)
        .map(|o| o.object_id.clone())
        .collect()
}

fn write_confirmed_overrides(
    obj: &mut TrackedObject,
    quote: Option<&str>,
    date: Option<&str>,
    now_ms: u64,
) {
    if let Some(quote) = quote {
        obj.overrides.insert(
            "quote".to_owned(),
            FactEntry {
                value: quote.to_owned(),
                status: FactStatus::Confirmed,
                updated_at_ms: now_ms,
            },
        );
    }
    if let Some(date) = date {
        obj.overrides.insert(
            "date".to_owned(),
            FactEntry {
                value: date.to_owned(),
                status: FactStatus::Confirmed,
                updated_at_ms: now_ms,
            },
        );
    }
}

/// Detect whether `prompt` records a meeting note / pending proposal rather
/// than an authoritative update to formal deliverables.
#[must_use]
fn is_proposal_or_record_prompt(prompt: &str) -> bool {
    let lower = prompt.to_ascii_lowercase();
    let strong_pending = [
        "尚待确认",
        "待确认",
        "暂未确认",
        "尚未确认",
        "未确认",
        "先别改正式文件",
        "别改正式文件",
        "不改正式文件",
        "先不改",
        "暂不修改",
        "可考虑",
        "pending confirmation",
        "do not modify",
        "don't modify",
    ]
    .iter()
    .any(|cue| lower.contains(cue));
    if strong_pending {
        return true;
    }
    let archival_note = [
        "会议记录",
        "会议纪要",
        "纪要归档",
        "归档记录",
        "先归档",
        "口头说",
        "口头提到",
        "仅记录",
        "先记一下",
        "记一下",
        "meeting note",
        "meeting minutes",
    ]
    .iter()
    .any(|cue| lower.contains(cue));
    let explicit_confirm =
        ["正式改", "已确认", "确认改", "立即更新", "confirmed"]
            .iter()
            .any(|cue| lower.contains(cue));
    archival_note && !explicit_confirm
}

fn has_all_objects_scope_cue(prompt: &str) -> bool {
    let lower = prompt.to_ascii_lowercase();
    [
        "全部",
        "所有",
        "都改",
        "一起改",
        "同步改",
        "同步更新",
        "统一改",
        "all files",
        "all deliverables",
        "both ",
    ]
    .iter()
    .any(|cue| lower.contains(cue))
}

fn has_demonstrative_ambiguity_cue(prompt: &str) -> bool {
    let lower = prompt.to_ascii_lowercase();
    [
        "刚才那份",
        "刚刚那份",
        "前面那份",
        "上面那份",
        "那份",
        "这份",
        "那个",
        "这个",
        "that one",
        "this one",
        "the previous one",
        "the last one",
    ]
    .iter()
    .any(|cue| lower.contains(cue))
}

fn split_clauses(prompt: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0_usize;
    let chars: Vec<(usize, char)> = prompt.char_indices().collect();
    for idx in 0..chars.len() {
        let (byte_pos, ch) = chars[idx];
        let is_split = matches!(ch, '，' | '。' | '；' | ';' | '\n')
            || (ch == ','
                && !chars
                    .get(idx + 1)
                    .is_some_and(|(_, next)| next.is_ascii_digit()));
        if is_split {
            let slice = prompt[start..byte_pos].trim();
            if !slice.is_empty() {
                out.push(slice);
            }
            start = byte_pos + ch.len_utf8();
        }
    }
    let tail = prompt[start..].trim();
    if !tail.is_empty() {
        out.push(tail);
    }
    out
}

fn match_clause_deliverables(
    clause: &str,
    objects: &[TrackedObject],
) -> Vec<String> {
    let lower = clause.to_ascii_lowercase();
    let mut matched = Vec::new();
    // 1. Exact filename or filename stem match.
    for obj in objects {
        if obj.kind != TrackedObjectKind::Deliverable {
            continue;
        }
        if let Some(name) = obj.filename.as_deref() {
            let name_lower = name.to_ascii_lowercase();
            let stem = name
                .rsplit_once('.')
                .map_or(name, |(s, _)| s)
                .to_ascii_lowercase();
            if lower.contains(&name_lower)
                || (!stem.is_empty()
                    && stem.chars().count() >= 3
                    && lower.contains(&stem))
            {
                matched.push(obj.object_id.clone());
            }
        }
    }
    if !matched.is_empty() {
        return matched;
    }
    // Never hijack an explicit all-objects clause via a loose role word.
    if has_all_objects_scope_cue(clause) {
        return matched;
    }
    // 2. Role/extension cue only when it identifies a unique deliverable of that format.
    let role_ext = if clause.contains("报价表")
        || lower.contains("excel")
        || lower.contains(".xlsx")
    {
        Some("xlsx")
    } else if lower.contains("word") || lower.contains(".docx") {
        Some("docx")
    } else if contains_deck_role_word(clause) {
        Some("pptx")
    } else {
        None
    };
    if let Some(ext) = role_ext {
        let candidates: Vec<&TrackedObject> = objects
            .iter()
            .filter(|obj| {
                obj.kind == TrackedObjectKind::Deliverable
                    && obj.extension.eq_ignore_ascii_case(ext)
            })
            .collect();
        if candidates.len() == 1 {
            matched.push(candidates[0].object_id.clone());
        }
    }
    matched
}

fn clause_is_untouched_marker(clause: &str) -> bool {
    let lower = clause.to_ascii_lowercase();
    let has_value = extract_date_fact(clause).is_some()
        || extract_quote_fact(clause).is_some();
    if has_value {
        return false;
    }
    [
        "不动",
        "保持原样",
        "不用改",
        "不改",
        "unchanged",
        "leave as is",
    ]
    .iter()
    .any(|cue| lower.contains(cue))
}

/// Build a [`PendingCard::Disambiguation`] strictly from host-owned writable
/// deliverables (filtering out [`TrackedObjectKind::Record`]).
#[must_use]
pub fn build_disambiguation_card(
    contract: &WorkContract,
    prompt: &str,
    now_ms: u64,
) -> PendingCard {
    let options: Vec<DisambiguationOption> = contract
        .writable_deliverables()
        .into_iter()
        .map(|obj| {
            let summary = if obj.anchor_summary.is_empty() {
                format_anchor_summary(obj, &contract.pinned_facts)
            } else {
                obj.anchor_summary.clone()
            };
            DisambiguationOption {
                object_id: obj.object_id.clone(),
                label: obj.display_label(),
                anchor_summary: summary,
                extension: obj.extension.clone(),
            }
        })
        .collect();
    let preserved = contract
        .pinned_facts
        .get("quote")
        .map_or_default(|q| format!("（如 {q}）"));
    PendingCard::Disambiguation {
        card_id: next_card_id(now_ms),
        created_at_ms: now_ms,
        prompt: prompt.to_owned(),
        impact_boundary: format!("只改所选对象，其余保持原值{preserved}"),
        options,
        allow_multi: true,
    }
}

fn apply_record_proposal(
    prev: &WorkContract,
    prompt: &str,
    now_ms: u64,
) -> BriefContract {
    let mut objects = ensure_objects_seeded(prev);
    let mut proposal_overrides = BTreeMap::new();
    if let Some(quote) = extract_quote_fact(prompt) {
        proposal_overrides.insert(
            "quote".to_owned(),
            FactEntry {
                value: quote,
                status: FactStatus::PendingProposal,
                updated_at_ms: now_ms,
            },
        );
    }
    if let Some(date) = extract_date_fact(prompt) {
        proposal_overrides.insert(
            "date".to_owned(),
            FactEntry {
                value: date,
                status: FactStatus::PendingProposal,
                updated_at_ms: now_ms,
            },
        );
    }
    let summary: String = prompt.trim().chars().take(48).collect();
    if let Some(record) = objects
        .iter_mut()
        .find(|obj| obj.kind == TrackedObjectKind::Record)
    {
        record.overrides.extend(proposal_overrides);
        if !summary.is_empty() {
            record.anchor_summary = summary;
        }
    } else {
        let record_id = next_object_id(&objects, "record");
        objects.push(TrackedObject {
            object_id: record_id,
            kind: TrackedObjectKind::Record,
            filename: None,
            extension: "md".to_owned(),
            anchor_summary: if summary.is_empty() {
                "会议记录归档".to_owned()
            } else {
                summary
            },
            overrides: proposal_overrides,
            verify_state: ObjectVerifyState::Unverified,
        });
    }
    let mut contract = BriefContract::from_work_contract(prev);
    contract.objects = objects;
    contract
}

fn sync_objects_with_required_items(
    objects: &mut Vec<TrackedObject>,
    required_items: &[flowy_agent_core::RequiredDeliverable],
    pinned_facts: &BTreeMap<String, String>,
) {
    // Drop deliverable objects whose item was explicitly revoked.
    objects.retain(|obj| {
        if obj.kind == TrackedObjectKind::Record {
            return true;
        }
        required_items.iter().any(|req| {
            req.extension.eq_ignore_ascii_case(&obj.extension)
                && match (&req.filename, &obj.filename) {
                    (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                    (None | Some(_), None) => true,
                    (None, Some(_)) => false,
                }
        })
    });
    // Upgrade generic objects or append newly introduced deliverables.
    for req in required_items {
        let exists = objects.iter().any(|obj| {
            obj.kind == TrackedObjectKind::Deliverable
                && obj.extension.eq_ignore_ascii_case(&req.extension)
                && match (&obj.filename, &req.filename) {
                    (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                    (None, None) => true,
                    _ => false,
                }
        });
        if exists {
            continue;
        }
        if let Some(new_name) = req.filename.as_deref()
            && let Some(generic) = objects.iter_mut().find(|obj| {
                obj.kind == TrackedObjectKind::Deliverable
                    && obj.filename.is_none()
                    && obj.extension.eq_ignore_ascii_case(&req.extension)
            })
        {
            generic.filename = Some(new_name.to_owned());
            continue;
        }
        let id = next_object_id(objects, &format!("obj-{}", req.extension));
        objects.push(TrackedObject {
            object_id: id,
            kind: TrackedObjectKind::Deliverable,
            filename: req.filename.clone(),
            extension: req.extension.clone(),
            anchor_summary: String::new(),
            overrides: BTreeMap::new(),
            verify_state: ObjectVerifyState::Unverified,
        });
    }
    refresh_anchor_summaries(objects, pinned_facts);
}

struct EvolvedStructure {
    required_items: Vec<flowy_agent_core::RequiredDeliverable>,
    inherited_min_slides: Option<usize>,
    pinned_facts: BTreeMap<String, String>,
}

fn evolve_deliverable_structure(
    prev: &WorkContract,
    prompt: &str,
    delta: &BriefContract,
    objects: &mut Vec<TrackedObject>,
) -> EvolvedStructure {
    let mut required_items = prev.required_items.clone();
    let is_rename = is_rename_prompt(prompt);
    let prompt_lower = prompt.to_ascii_lowercase();
    let mut inherited_min_slides = prev.min_slides;

    let revoke_xlsx = [
        "不用做报价表",
        "不需要报价表",
        "不要报价表",
        "去掉报价表",
        "删除报价表",
        "不用做excel",
        "不需要excel",
        "不需要xlsx",
        "去掉excel",
        "去掉表格",
        "只做ppt",
        "只要ppt",
        "only ppt",
    ]
    .iter()
    .any(|cue| prompt_lower.contains(cue));
    let revoke_pptx =
        ["不用做ppt", "不需要ppt", "去掉ppt", "no ppt", "no slides"]
            .iter()
            .any(|cue| prompt_lower.contains(cue));
    let revoke_docx = ["不用做word", "不需要word", "不需要docx", "去掉word"]
        .iter()
        .any(|cue| prompt_lower.contains(cue));

    for item in &delta.required_items {
        if is_rename && let Some(new_name) = item.filename.as_deref() {
            if let Some(slot) = required_items.iter_mut().find(|existing| {
                existing.extension.eq_ignore_ascii_case(&item.extension)
                    && existing.filename.as_deref().is_some_and(|old| {
                        !old.eq_ignore_ascii_case(new_name)
                            && prompt_lower.contains(&old.to_ascii_lowercase())
                    })
            }) {
                let old_name = slot.filename.clone();
                *slot = item.clone();
                if let Some(old_name) = old_name
                    && let Some(obj) = objects.iter_mut().find(|o| {
                        o.kind == TrackedObjectKind::Deliverable
                            && o.filename.as_deref().is_some_and(|f| {
                                f.eq_ignore_ascii_case(&old_name)
                            })
                    })
                {
                    obj.filename.clone_from(&item.filename);
                }
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
                let old_name =
                    required_items[same_ext_indices[0]].filename.clone();
                required_items[same_ext_indices[0]] = item.clone();
                if let Some(obj) = objects.iter_mut().find(|o| {
                    o.kind == TrackedObjectKind::Deliverable
                        && o.extension.eq_ignore_ascii_case(&item.extension)
                        && (old_name.is_none() || o.filename == old_name)
                }) {
                    obj.filename.clone_from(&item.filename);
                }
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
            if item.filename.is_some()
                && let Some(slot) = required_items.iter_mut().find(|e| {
                    e.filename.is_none()
                        && e.extension.eq_ignore_ascii_case(&item.extension)
                })
            {
                *slot = item.clone();
            } else {
                required_items.push(item.clone());
            }
        }
    }
    if revoke_xlsx {
        required_items.retain(|existing| {
            !existing.extension.eq_ignore_ascii_case("xlsx")
                && !existing.extension.eq_ignore_ascii_case("csv")
        });
    }
    if revoke_pptx {
        required_items.retain(|existing| {
            !existing.extension.eq_ignore_ascii_case("pptx")
        });
        inherited_min_slides = None;
    }
    if revoke_docx {
        required_items.retain(|existing| {
            !existing.extension.eq_ignore_ascii_case("docx")
        });
    }

    let mut pinned_facts = prev.pinned_facts.clone();
    if [
        "去掉日期",
        "去掉具体日期",
        "删除日期",
        "不需要日期",
        "不用日期",
        "不要日期",
        "remove date",
        "no date",
    ]
    .iter()
    .any(|cue| prompt_lower.contains(cue))
    {
        pinned_facts.remove("date");
        for obj in objects.iter_mut() {
            if obj.kind == TrackedObjectKind::Deliverable {
                obj.overrides.remove("date");
            }
        }
    }
    if [
        "去掉报价",
        "删除报价",
        "不需要报价",
        "不要报价",
        "remove quote",
        "no price",
    ]
    .iter()
    .any(|cue| prompt_lower.contains(cue))
    {
        pinned_facts.remove("quote");
        for obj in objects.iter_mut() {
            if obj.kind == TrackedObjectKind::Deliverable {
                obj.overrides.remove("quote");
            }
        }
    }

    sync_objects_with_required_items(objects, &required_items, &pinned_facts);
    EvolvedStructure {
        required_items,
        inherited_min_slides,
        pinned_facts,
    }
}

fn apply_scoped_facts_or_disambiguate(
    prev: &WorkContract,
    prompt: &str,
    delta: BriefContract,
    structure: EvolvedStructure,
    mut objects: Vec<TrackedObject>,
    now_ms: u64,
) -> ResolveOutcome {
    let EvolvedStructure {
        required_items,
        inherited_min_slides,
        mut pinned_facts,
    } = structure;
    let deliverable_count = objects
        .iter()
        .filter(|obj| obj.kind == TrackedObjectKind::Deliverable)
        .count();

    // Per-clause, per-key fact scoping: cleanly separate targeted clause facts
    // from unscoped facts so mixed clauses never silently drop unscoped edits.
    let clauses = split_clauses(prompt);
    let mut clause_targets: BTreeSet<String> = BTreeSet::new();
    let mut targeted_by_key: BTreeMap<&'static str, BTreeSet<String>> =
        BTreeMap::new();
    let mut unscoped_facts: BTreeMap<String, String> = BTreeMap::new();

    for clause in &clauses {
        if clause_is_untouched_marker(clause) {
            continue;
        }
        let clause_quote = extract_quote_fact(clause);
        let clause_date = extract_date_fact(clause);
        if clause_quote.is_none() && clause_date.is_none() {
            continue;
        }
        let targets = match_clause_deliverables(clause, &objects);
        if targets.is_empty() {
            if let Some(q) = clause_quote {
                unscoped_facts.insert("quote".to_owned(), q);
            }
            if let Some(d) = clause_date {
                unscoped_facts.insert("date".to_owned(), d);
            }
            continue;
        }
        for target_id in targets {
            clause_targets.insert(target_id.clone());
            if clause_quote.is_some() {
                targeted_by_key
                    .entry("quote")
                    .or_default()
                    .insert(target_id.clone());
            }
            if clause_date.is_some() {
                targeted_by_key
                    .entry("date")
                    .or_default()
                    .insert(target_id.clone());
            }
            if let Some(obj) =
                objects.iter_mut().find(|o| o.object_id == target_id)
            {
                write_confirmed_overrides(
                    obj,
                    clause_quote.as_deref(),
                    clause_date.as_deref(),
                    now_ms,
                );
            }
        }
    }

    // Catch any whole-prompt fact that spanned clause punctuation without
    // landing in a clause-level bucket.
    for (key, value) in delta.pinned_facts {
        if !targeted_by_key.contains_key(key.as_str())
            && !unscoped_facts.contains_key(&key)
        {
            unscoped_facts.insert(key, value);
        }
    }

    let same_ext_duplicates = {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for obj in &objects {
            if obj.kind == TrackedObjectKind::Deliverable {
                *counts
                    .entry(obj.extension.to_ascii_lowercase())
                    .or_insert(0) += 1;
            }
        }
        counts.values().any(|&count| count >= 2)
    };
    let ambiguous = !unscoped_facts.is_empty()
        && deliverable_count >= 2
        && !has_all_objects_scope_cue(prompt)
        && (has_demonstrative_ambiguity_cue(prompt) || same_ext_duplicates);

    if ambiguous {
        // Build the disambiguation card strictly from persisted `prev`
        // deliverables so every presented `object_id` is guaranteed to pass
        // `apply_disambiguation_selection` when the user clicks an option.
        let mut persisted_snapshot = prev.clone();
        persisted_snapshot.objects = ensure_objects_seeded(prev);
        refresh_anchor_summaries(
            &mut persisted_snapshot.objects,
            &persisted_snapshot.pinned_facts,
        );
        return ResolveOutcome::Ambiguous {
            card: build_disambiguation_card(
                &persisted_snapshot,
                prompt,
                now_ms,
            ),
            used_llm: false,
        };
    }

    // Apply unambiguous unscoped facts globally and clear stale per-object
    // overrides on deliverables not explicitly targeted in this turn so old
    // overrides never shadow newer session-wide updates.
    for (key, value) in unscoped_facts {
        let explicitly_targeted = targeted_by_key.get(key.as_str());
        pinned_facts.insert(key.clone(), value);
        for obj in &mut objects {
            if obj.kind == TrackedObjectKind::Deliverable
                && explicitly_targeted
                    .is_none_or(|ids| !ids.contains(&obj.object_id))
            {
                obj.overrides.remove(&key);
            }
        }
    }

    refresh_anchor_summaries(&mut objects, &pinned_facts);
    let target_object_ids: Vec<String> = if clause_targets.is_empty() {
        objects
            .iter()
            .filter(|o| o.kind == TrackedObjectKind::Deliverable)
            .map(|o| o.object_id.clone())
            .collect()
    } else {
        clause_targets.into_iter().collect()
    };

    let work = WorkContract {
        revision: prev.revision.saturating_add(1).max(1),
        min_slides: delta.min_slides.or(inherited_min_slides),
        required_items,
        pinned_facts,
        objects,
    };
    ResolveOutcome::Resolved {
        contract: BriefContract::from_work_contract(&work),
        target_object_ids,
        used_llm: false,
    }
}

/// Deterministic contract evolution with multi-object isolation, proposal
/// guarding, and ambiguity detection (0ms fast-path).
#[must_use]
pub fn resolve_contract_sync(
    previous: Option<&WorkContract>,
    prompt: &str,
    reply_to_object_id: Option<&str>,
    now_ms: u64,
) -> ResolveOutcome {
    // 1. Meeting notes / pending proposals NEVER overwrite confirmed facts,
    // including when they arrive on the very first turn.
    if is_proposal_or_record_prompt(prompt) {
        let empty = WorkContract::default();
        let base = previous.unwrap_or(&empty);
        let contract = apply_record_proposal(base, prompt, now_ms);
        let record_ids = record_object_ids(&contract.objects);
        return ResolveOutcome::Resolved {
            contract,
            target_object_ids: record_ids,
            used_llm: false,
        };
    }

    let Some(prev) = previous else {
        let contract = BriefContract::from_brief(prompt);
        let targets: Vec<String> = contract
            .objects
            .iter()
            .filter(|o| o.kind == TrackedObjectKind::Deliverable)
            .map(|o| o.object_id.clone())
            .collect();
        return ResolveOutcome::Resolved {
            contract,
            target_object_ids: targets,
            used_llm: false,
        };
    };

    let mut objects = ensure_objects_seeded(prev);

    // 2. Explicit `reply_to_object_id` fast-path (0ms, fail-closed on invalid/Record ID).
    if let Some(target_id) = reply_to_object_id {
        let valid = objects.iter().any(|obj| {
            obj.object_id == target_id
                && obj.kind == TrackedObjectKind::Deliverable
        });
        let mut prev_seeded = prev.clone();
        prev_seeded.objects.clone_from(&objects);
        if !valid {
            refresh_anchor_summaries(
                &mut prev_seeded.objects,
                &prev_seeded.pinned_facts,
            );
            return ResolveOutcome::Ambiguous {
                card: build_disambiguation_card(&prev_seeded, prompt, now_ms),
                used_llm: false,
            };
        }
        if let Ok(contract) = apply_disambiguation_selection(
            &prev_seeded,
            prompt,
            &[target_id.to_owned()],
            now_ms,
        ) {
            return ResolveOutcome::Resolved {
                contract,
                target_object_ids: vec![target_id.to_owned()],
                used_llm: false,
            };
        }
    }

    // 3. Evolve structural deliverables, then apply per-object facts or park ambiguity.
    let delta = BriefContract::from_brief(prompt);
    let structure =
        evolve_deliverable_structure(prev, prompt, &delta, &mut objects);
    apply_scoped_facts_or_disambiguate(
        prev, prompt, delta, structure, objects, now_ms,
    )
}

/// Apply a user's explicit [`DisambiguationOption`] selection to `previous`,
/// mutating ONLY the selected `object_id`(s) and leaving every other object at
/// its original confirmed value.
pub fn apply_disambiguation_selection(
    previous: &WorkContract,
    prompt: &str,
    selected_object_ids: &[String],
    now_ms: u64,
) -> Result<BriefContract, &'static str> {
    apply_object_fact_updates(
        previous,
        selected_object_ids,
        extract_quote_fact(prompt),
        extract_date_fact(prompt),
        now_ms,
    )
}

fn apply_object_fact_updates(
    previous: &WorkContract,
    selected_object_ids: &[String],
    quote: Option<String>,
    date: Option<String>,
    now_ms: u64,
) -> Result<BriefContract, &'static str> {
    if selected_object_ids.is_empty() {
        return Err("notice.card_selection_empty");
    }
    let mut objects = ensure_objects_seeded(previous);
    for id in selected_object_ids {
        let valid = objects.iter().any(|obj| {
            obj.object_id == *id && obj.kind == TrackedObjectKind::Deliverable
        });
        if !valid {
            return Err("notice.card_selection_invalid");
        }
    }
    for target_id in selected_object_ids {
        if let Some(obj) =
            objects.iter_mut().find(|o| o.object_id == *target_id)
        {
            write_confirmed_overrides(
                obj,
                quote.as_deref(),
                date.as_deref(),
                now_ms,
            );
        }
    }
    refresh_anchor_summaries(&mut objects, &previous.pinned_facts);
    let work = WorkContract {
        revision: previous.revision.saturating_add(1).max(1),
        min_slides: previous.min_slides,
        required_items: previous.required_items.clone(),
        pinned_facts: previous.pinned_facts.clone(),
        objects,
    };
    Ok(BriefContract::from_work_contract(&work))
}

/// Test-only LLM contract-resolver probe (`complete_structured` validation and
/// opt-in live cloud verification). Kept out of production exports because
/// `resolve_contract_sync` already settles disambiguation at 0ms without
/// risking model hallucination or holding dispatch locks across HTTP calls.
#[cfg(test)]
mod llm_probe {
    use std::time::Duration;

    use flowy_agent_core::{
        CancelToken, ModelRoute, StructuredRequest, complete_structured,
    };
    use serde_json::{Value, json};

    use super::*;

    const PROPOSE_CONTRACT_DELTA_TOOL: &str = "propose_contract_delta";
    const RESOLVER_TIMEOUT: Duration = Duration::from_secs(20);

    const RESOLVER_SYSTEM_PROMPT: &str = "\
你是工作契约消歧解析器。你只接收当前会话已存在的对象列表和用户新消息。\
规则：\
1. 只能引用对象列表中真实存在的 object_id，禁止编造对象，禁止修改 kind=record 的归档记录。\
2. 若用户只是记录会议纪要、口头提议或含「尚待确认 / 先别改正式文件 / 可考虑」，则 action 必须为 record_proposal，不得修改任何 deliverable 的已确认事实。\
3. 若会话存在多个可写 deliverable 且用户未明确指明修改哪一份（如「把刚才那份的报价改成 32 万」），action 必须为 disambiguate，由宿主弹出消歧卡让用户选择。\
4. 仅当用户明确指明唯一目标对象（或明确要求修改全部交付物）时，才返回 action=update 并列出目标 object_ids。\
请调用 propose_contract_delta 提交结构化结果。";

    #[derive(Clone, Debug)]
    struct ResolverLlmConfig {
        route: ModelRoute,
        api_key: String,
    }

    fn contract_delta_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["update", "disambiguate", "record_proposal", "noop"]
                },
                "target_object_ids": {
                    "type": "array",
                    "items": { "type": "string" }
                },
                "quote": { "type": "string" },
                "date": { "type": "string" },
                "fact_status": {
                    "type": "string",
                    "enum": ["confirmed", "pending_proposal"]
                },
                "reason": { "type": "string" }
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }

    /// Strictly fail-closed validator: rejects mixed valid+invalid/Record IDs,
    /// deduplicates IDs, seeds legacy contracts, and blocks `"noop"` when the
    /// prompt carries a concrete fact mutation.
    fn validate_structured_proposal(
        previous: &WorkContract,
        prompt: &str,
        proposal: &Value,
        now_ms: u64,
    ) -> ResolveOutcome {
        let mut seeded = previous.clone();
        seeded.objects = ensure_objects_seeded(previous);
        let fallback = || ResolveOutcome::Ambiguous {
            card: build_disambiguation_card(&seeded, prompt, now_ms),
            used_llm: true,
        };
        let Some(action) = proposal.get("action").and_then(Value::as_str)
        else {
            return fallback();
        };
        match action {
            "record_proposal" => {
                let contract = apply_record_proposal(&seeded, prompt, now_ms);
                let record_ids = record_object_ids(&contract.objects);
                ResolveOutcome::Resolved {
                    contract,
                    target_object_ids: record_ids,
                    used_llm: true,
                }
            }
            "update" => {
                let fact_status = proposal
                    .get("fact_status")
                    .and_then(Value::as_str)
                    .unwrap_or("confirmed");
                if fact_status == "pending_proposal" {
                    let contract =
                        apply_record_proposal(&seeded, prompt, now_ms);
                    let record_ids = record_object_ids(&contract.objects);
                    return ResolveOutcome::Resolved {
                        contract,
                        target_object_ids: record_ids,
                        used_llm: true,
                    };
                }
                let Some(raw_ids) =
                    proposal.get("target_object_ids").and_then(Value::as_array)
                else {
                    return fallback();
                };
                let mut dedup: BTreeSet<String> = BTreeSet::new();
                for entry in raw_ids {
                    let Some(id) = entry.as_str() else {
                        return fallback();
                    };
                    let valid = seeded.objects.iter().any(|obj| {
                        obj.object_id == id
                            && obj.kind == TrackedObjectKind::Deliverable
                    });
                    if !valid {
                        return fallback();
                    }
                    dedup.insert(id.to_owned());
                }
                if dedup.is_empty() {
                    return fallback();
                }
                let valid_ids: Vec<String> = dedup.into_iter().collect();
                let quote = proposal
                    .get("quote")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|raw| {
                        extract_quote_fact(raw)
                            .unwrap_or_else(|| raw.to_owned())
                    })
                    .or_else(|| extract_quote_fact(prompt));
                let date = proposal
                    .get("date")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|raw| {
                        extract_date_fact(raw).unwrap_or_else(|| raw.to_owned())
                    })
                    .or_else(|| extract_date_fact(prompt));
                match apply_object_fact_updates(
                    &seeded, &valid_ids, quote, date, now_ms,
                ) {
                    Ok(contract) => ResolveOutcome::Resolved {
                        contract,
                        target_object_ids: valid_ids,
                        used_llm: true,
                    },
                    Err(_) => fallback(),
                }
            }
            "noop" => {
                if extract_quote_fact(prompt).is_some()
                    || extract_date_fact(prompt).is_some()
                {
                    return fallback();
                }
                ResolveOutcome::Resolved {
                    contract: BriefContract::from_work_contract(&seeded),
                    target_object_ids: Vec::new(),
                    used_llm: true,
                }
            }
            _ => fallback(),
        }
    }

    async fn resolve_contract_async(
        previous: Option<&WorkContract>,
        prompt: &str,
        reply_to_object_id: Option<&str>,
        llm: Option<&ResolverLlmConfig>,
        signal: &CancelToken,
        now_ms: u64,
    ) -> ResolveOutcome {
        let deterministic =
            resolve_contract_sync(previous, prompt, reply_to_object_id, now_ms);
        let ResolveOutcome::Ambiguous { card, .. } = &deterministic else {
            return deterministic;
        };
        let (Some(prev), Some(llm_cfg)) = (previous, llm) else {
            return deterministic;
        };
        if llm_cfg.api_key.trim().is_empty() {
            return deterministic;
        }
        let mut seeded = prev.clone();
        seeded.objects = ensure_objects_seeded(prev);
        let mut body = String::from("【当前会话对象表】\n");
        for obj in &seeded.objects {
            use std::fmt::Write as _;
            let kind = match obj.kind {
                TrackedObjectKind::Deliverable => "deliverable",
                TrackedObjectKind::Record => "record",
            };
            let facts = seeded.confirmed_facts_for(&obj.object_id);
            let _ = writeln!(
                body,
                "- object_id={} kind={} file={} ext={} anchor=\"{}\" facts={facts:?}",
                obj.object_id,
                kind,
                obj.filename.as_deref().unwrap_or("(unnamed)"),
                obj.extension,
                obj.anchor_summary,
            );
        }
        body.push_str("\n【用户新消息】\n");
        body.push_str(prompt);

        let call_signal = signal.child_token();
        let call = complete_structured(
            StructuredRequest {
                session_id: Some("contract-resolver".to_owned()),
                system: RESOLVER_SYSTEM_PROMPT.to_owned(),
                prompt: body,
                tool_name: PROPOSE_CONTRACT_DELTA_TOOL.to_owned(),
                tool_description:
                    "Propose a validated multi-object contract delta or request disambiguation."
                        .to_owned(),
                schema: contract_delta_schema(),
                route: llm_cfg.route.clone(),
                signal: call_signal.clone(),
            },
            Some(llm_cfg.api_key.clone()),
        );
        match tokio::time::timeout(RESOLVER_TIMEOUT, call).await {
            Ok(Ok(value)) => {
                validate_structured_proposal(&seeded, prompt, &value, now_ms)
            }
            Ok(Err(_)) => ResolveOutcome::Ambiguous {
                card: card.clone(),
                used_llm: false,
            },
            Err(_) => {
                call_signal.cancel();
                ResolveOutcome::Ambiguous {
                    card: card.clone(),
                    used_llm: false,
                }
            }
        }
    }

    #[test]
    fn host_validation_rejects_hallucinated_mixed_record_and_noop_on_mutations()
    {
        let base = BriefContract::from_brief(
            "请帮青禾项目做一份报价表和三页青禾项目报价汇报.pptx，报价 30 万元，汇报日期是 2026 年 11 月 27 日",
        );
        let with_record = BriefContract::evolve(
            Some(&base.to_work_contract()),
            "会议记录：客户希望报价调整为 35 万元，尚待确认。",
        );
        let wc = with_record.to_work_contract();
        let record_id = wc
            .objects
            .iter()
            .find(|o| o.kind == TrackedObjectKind::Record)
            .unwrap()
            .object_id
            .clone();
        let ppt_id = wc
            .objects
            .iter()
            .find(|o| o.extension == "pptx")
            .unwrap()
            .object_id
            .clone();

        // Hallucinated object_id must fall back to Ambiguous.
        let fake = validate_structured_proposal(
            &wc,
            "把刚才那份的报价改成 32 万元。",
            &json!({"action": "update", "target_object_ids": ["nonexistent-id"], "quote": "32万"}),
            500,
        );
        let ResolveOutcome::Ambiguous {
            card:
                PendingCard::Disambiguation {
                    options,
                    impact_boundary,
                    allow_multi,
                    ..
                },
            used_llm,
        } = fake
        else {
            panic!("hallucinated target must fall back to Ambiguous: {fake:?}");
        };
        assert!(used_llm);
        assert!(allow_multi);
        assert!(impact_boundary.contains("只改所选"));
        assert!(options.iter().all(|opt| opt.object_id != record_id));

        // Mixed valid + Record / hallucinated IDs must fail-closed to Ambiguous.
        let mixed = validate_structured_proposal(
            &wc,
            "把刚才那份的报价改成 32 万元。",
            &json!({"action": "update", "target_object_ids": [ppt_id, record_id], "quote": "32万"}),
            501,
        );
        assert!(matches!(mixed, ResolveOutcome::Ambiguous { .. }));

        // `"noop"` on a concrete fact-mutating prompt must fail-closed to Ambiguous.
        let noop = validate_structured_proposal(
            &wc,
            "把刚才那份的报价改成 32 万元。",
            &json!({"action": "noop"}),
            502,
        );
        assert!(matches!(noop, ResolveOutcome::Ambiguous { .. }));
    }

    #[tokio::test]
    #[ignore = "hits live cloud gateway server.flowyaipc.com using local Flowy auth session"]
    async fn live_cloud_contract_resolver_disambiguates() {
        let appdata =
            std::env::var("APPDATA").expect("APPDATA env var must be set");
        let session_path = std::path::PathBuf::from(appdata)
            .join("Flowy")
            .join("Flowy")
            .join("data")
            .join("auth")
            .join("session.json");
        let raw = std::fs::read_to_string(&session_path)
            .expect("read local Flowy auth session.json");
        let session: Value =
            serde_json::from_str(&raw).expect("parse auth session.json");
        let api_key = session
            .get("access_token")
            .and_then(Value::as_str)
            .expect("access_token in session.json")
            .to_owned();
        let llm = ResolverLlmConfig {
            route: ModelRoute {
                provider: "flowy".to_owned(),
                model: "AIPC-qwen3.8-flash".to_owned(),
                base_url: "https://server.flowyaipc.com/claw/v1".to_owned(),
                ..ModelRoute::default()
            },
            api_key,
        };

        let base = BriefContract::from_brief(
            "请帮青禾项目做一份报价表和三页青禾项目报价汇报.pptx，报价 30 万元，汇报日期是 2026 年 11 月 27 日",
        );
        let c_rec = BriefContract::evolve(
            Some(&base.to_work_contract()),
            "会议记录：客户希望报价调整为 35 万元，尚待确认。",
        );
        let c_twin = BriefContract::evolve(
            Some(&c_rec.to_work_contract()),
            "保留青禾项目报价汇报.pptx，另做一份三页的青禾项目内部讨论稿.pptx",
        );
        let c_split = BriefContract::evolve(
            Some(&c_twin.to_work_contract()),
            "仅把青禾项目报价汇报.pptx 的汇报日期改为 2026-12-02，青禾项目内部讨论稿.pptx 仍保持 2026-11-30",
        );
        let wc = c_split.to_work_contract();
        let outcome = resolve_contract_async(
            Some(&wc),
            "把刚才那份的报价改成 32 万元。",
            None,
            Some(&llm),
            &CancelToken::new(),
            1_000,
        )
        .await;
        println!("live cloud contract resolver outcome: {outcome:?}");
        let ResolveOutcome::Ambiguous {
            card:
                PendingCard::Disambiguation {
                    options,
                    impact_boundary,
                    ..
                },
            used_llm,
        } = outcome
        else {
            panic!("ambiguous quote prompt must trigger Disambiguation card");
        };
        assert!(used_llm);
        assert_eq!(options.len(), 3);
        assert!(impact_boundary.contains("只改所选"));
    }
}

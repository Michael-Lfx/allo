//! Chronological verification settlement, deliverable deduplication, and badge assignment.

use std::collections::{HashMap, HashSet};

use flowy_agent::ChatLine;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliverableVerifyBadge {
    Verified,
    Overridden,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeliverableItem {
    pub entry: flowy_agent::FileEntry,
    pub badge: Option<DeliverableVerifyBadge>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailedVerificationItem {
    pub report_id: String,
    pub artifact_path: String,
    pub issue_count: usize,
    pub summary: String,
}

pub(super) struct LatestVerdict<'a> {
    pub(super) order: usize,
    pub(super) report_id: &'a str,
    pub(super) artifact_path: &'a str,
    pub(super) ok: bool,
    pub(super) override_used: bool,
    pub(super) issues: &'a [flowy_agent::VerificationIssue],
}

pub(super) fn analyze_session_verifications<'a>(
    lines: &'a [ChatLine],
) -> (
    HashMap<String, bool>,
    HashMap<String, DeliverableVerifyBadge>,
    Vec<LatestVerdict<'a>>,
) {
    let mut latest_by_key: HashMap<String, LatestVerdict<'a>> = HashMap::new();
    for (order, line) in lines.iter().enumerate() {
        if let ChatLine::Verification {
            report_id,
            artifact_path,
            ok,
            issues,
            override_used,
            ..
        } = line
            && !artifact_path.trim().is_empty()
        {
            let key = crate::ports::canonical_path_key(artifact_path);
            latest_by_key.insert(
                key,
                LatestVerdict {
                    order,
                    report_id: report_id.as_str(),
                    artifact_path: artifact_path.as_str(),
                    ok: *ok,
                    override_used: *override_used,
                    issues: issues.as_slice(),
                },
            );
        }
    }
    let mut settled = HashMap::with_capacity(latest_by_key.len());
    let mut badges = HashMap::with_capacity(latest_by_key.len());
    let mut failures = Vec::new();
    for (key, verdict) in latest_by_key {
        let passed = verdict.ok || verdict.override_used;
        settled.insert(key.clone(), passed);
        if verdict.ok {
            badges.insert(key, DeliverableVerifyBadge::Verified);
        } else if verdict.override_used {
            badges.insert(key, DeliverableVerifyBadge::Overridden);
        } else {
            failures.push(verdict);
        }
    }
    failures.sort_by_key(|item| std::cmp::Reverse(item.order));
    (settled, badges, failures)
}

/// Records the chronological latest verification verdict (`ok || override_used`)
/// per normalized local path across an entire session timeline (§8.3).
#[cfg(test)]
fn session_verification_settlement(
    lines: &[ChatLine],
) -> HashMap<String, bool> {
    analyze_session_verifications(lines).0
}

pub(super) fn collect_settled_entries(
    lines: &[ChatLine],
    latest_settled: &HashMap<String, bool>,
    latest_only: bool,
) -> Option<Vec<flowy_agent::FileEntry>> {
    let mut turn_starts: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter_map(|(ix, line)| {
            matches!(line, ChatLine::User { .. }).then_some(ix)
        })
        .collect();
    if turn_starts.first().copied() != Some(0) {
        turn_starts.insert(0, 0);
    }
    turn_starts.push(lines.len());

    let mut all_entries: Vec<flowy_agent::FileEntry> = Vec::new();
    let mut seen = HashSet::new();
    for bounds in turn_starts.windows(2).rev() {
        let mut turn_entries: Vec<flowy_agent::FileEntry> = Vec::new();
        for line in &lines[bounds[0]..bounds[1]] {
            match line {
                ChatLine::FileChanges { files } => {
                    for file in files {
                        let norm = crate::ports::canonical_path_key(&file.path);
                        if latest_settled.get(&norm) == Some(&false) {
                            continue;
                        }
                        if seen.insert(norm) {
                            turn_entries.push(file.clone());
                        }
                    }
                }
                ChatLine::FileChange {
                    path,
                    change,
                    added,
                    removed,
                    binary,
                } => {
                    let norm = crate::ports::canonical_path_key(path);
                    if latest_settled.get(&norm) == Some(&false) {
                        continue;
                    }
                    if seen.insert(norm) {
                        turn_entries.push(flowy_agent::FileEntry {
                            path: path.clone(),
                            change: *change,
                            added: *added,
                            removed: *removed,
                            binary: *binary,
                        });
                    }
                }
                ChatLine::Verification { artifact_path, .. }
                    if !artifact_path.trim().is_empty() =>
                {
                    let norm = crate::ports::canonical_path_key(artifact_path);
                    if latest_settled.get(&norm).copied() == Some(true)
                        && seen.insert(norm)
                    {
                        turn_entries.push(flowy_agent::FileEntry {
                            path: artifact_path.clone(),
                            change: flowy_agent::FileChangeKind::Add,
                            added: 0,
                            removed: 0,
                            binary: false,
                        });
                    }
                }
                _ => {}
            }
        }
        if !turn_entries.is_empty() {
            if latest_only {
                return Some(turn_entries);
            }
            all_entries.extend(turn_entries);
        }
    }
    (!all_entries.is_empty()).then_some(all_entries)
}

pub(super) fn attach_badges(
    entries: Vec<flowy_agent::FileEntry>,
    badges: &HashMap<String, DeliverableVerifyBadge>,
) -> Vec<DeliverableItem> {
    entries
        .into_iter()
        .map(|entry| {
            let key = crate::ports::canonical_path_key(&entry.path);
            DeliverableItem {
                badge: badges.get(&key).copied(),
                entry,
            }
        })
        .collect()
}

/// Collects deliverable files from the most recent turn that produced any,
/// merging file-change summaries with verified/overridden verification
/// receipts while strictly excluding any path whose latest session-wide
/// verification failed without override (§8.3).
#[cfg(test)]
pub(super) fn collect_latest_deliverables(
    lines: &[ChatLine],
) -> Option<Vec<flowy_agent::FileEntry>> {
    collect_settled_entries(
        lines,
        &session_verification_settlement(lines),
        true,
    )
}

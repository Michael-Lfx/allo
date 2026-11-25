use std::collections::{HashMap, HashSet};

use flowy_agent::ChatLine;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fold {
    /// Source index of the first collapsible row.
    pub key: usize,
    pub expanded: bool,
}

/// Aggregated summary metadata for a collapsed or expandable turn process block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldMeta {
    pub step_count: usize,
    pub live: bool,
    pub duration_secs: u64,
    pub active_step: Option<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Projection {
    pub source_rows: Vec<usize>,
    pub folds: HashMap<usize, Fold>,
    pub fold_meta: HashMap<usize, FoldMeta>,
    pub hidden: HashMap<usize, usize>,
    pub streaming: Option<usize>,
}

impl Projection {
    /// Completed-turn projection kept intact for legacy folding invariants.
    #[cfg(test)]
    pub fn build(
        lines: &[ChatLine],
        streaming: Option<usize>,
        latest_completed: bool,
        expanded: Option<&HashSet<usize>>,
    ) -> Self {
        Self::build_with_mode(
            lines,
            streaming,
            latest_completed,
            expanded,
            false,
        )
    }

    /// Super Agent live-collapsed projection: aggregates `Reasoning`, `Tool`,
    /// `ToolCall`, and non-error `ToolResult` rows into ONE collapsible turn
    /// summary starting from frame 1 (even while the turn is live), while
    /// keeping `User`, `Assistant`, `Artifact`, `Verification`, and error rows
    /// continuously visible.
    pub fn build_live(
        lines: &[ChatLine],
        streaming: Option<usize>,
        latest_completed: bool,
        expanded: Option<&HashSet<usize>>,
    ) -> Self {
        Self::build_with_mode(
            lines,
            streaming,
            latest_completed,
            expanded,
            true,
        )
    }

    fn build_with_mode(
        lines: &[ChatLine],
        streaming: Option<usize>,
        latest_completed: bool,
        expanded: Option<&HashSet<usize>>,
        live_collapse: bool,
    ) -> Self {
        let mut result = Self::default();
        let mut starts: Vec<_> = lines
            .iter()
            .enumerate()
            .filter_map(|(index, line)| {
                matches!(line, ChatLine::User { .. }).then_some(index)
            })
            .collect();
        starts.push(lines.len());
        for source in 0..starts[0] {
            result.push(source, streaming);
        }
        for turn in starts.windows(2) {
            let (start, end) = (turn[0], turn[1]);
            let is_latest_turn = end == lines.len();
            let last_turn_row = lines[start..end].iter().rfind(|line| {
                !matches!(
                    line,
                    ChatLine::TaskReceipt { .. }
                        | ChatLine::Artifact { .. }
                        | ChatLine::Verification { .. }
                )
            });
            let complete = (!is_latest_turn || latest_completed)
                && matches!(last_turn_row, Some(ChatLine::Assistant { .. }));
            let should_fold = live_collapse || complete;
            let marker = should_fold
                .then(|| {
                    (start + 1..end)
                        .find(|&row| is_process_row(&lines[row], live_collapse))
                })
                .flatten();
            let Some(marker) = marker else {
                for source in start..end {
                    result.push(source, streaming);
                }
                continue;
            };
            let expanded =
                expanded.is_some_and(|state| state.contains(&marker));
            let meta = summarize_turn_process(
                &lines[start + 1..end],
                is_latest_turn && !latest_completed,
                live_collapse,
            );
            let mut button_row = 0;
            for (source, line) in lines.iter().enumerate().take(end).skip(start)
            {
                if source == marker {
                    button_row = result.source_rows.len();
                    result.folds.insert(
                        button_row,
                        Fold {
                            key: marker,
                            expanded,
                        },
                    );
                    result.fold_meta.insert(button_row, meta.clone());
                } else if !expanded && is_process_row(line, live_collapse) {
                    result.hidden.insert(source, button_row);
                    continue;
                }
                result.push(source, streaming);
            }
        }
        result
    }

    fn push(&mut self, source: usize, streaming: Option<usize>) {
        if streaming == Some(source) {
            self.streaming = Some(self.source_rows.len());
        }
        self.source_rows.push(source);
    }
}

const fn is_process_row(line: &ChatLine, include_live: bool) -> bool {
    match line {
        ChatLine::Reasoning { live, .. } => include_live || !*live,
        ChatLine::ToolCall { running, .. } => include_live || !*running,
        ChatLine::Tool { .. }
        | ChatLine::ToolResult {
            is_error: false, ..
        } => true,
        _ => false,
    }
}

fn summarize_turn_process(
    slice: &[ChatLine],
    turn_still_running: bool,
    include_live: bool,
) -> FoldMeta {
    let mut primary_steps = 0usize;
    let mut fallback_results = 0usize;
    let has_assistant = slice
        .iter()
        .any(|line| matches!(line, ChatLine::Assistant { .. }));
    let mut live = turn_still_running && !has_assistant;
    let mut duration_secs = 0u64;
    let mut active_step: Option<String> = None;

    for line in slice {
        if !is_process_row(line, include_live) {
            continue;
        }
        match line {
            ChatLine::Reasoning {
                text,
                duration_secs: secs,
                live: row_live,
            } => {
                primary_steps += 1;
                duration_secs = duration_secs.saturating_add(*secs);
                if *row_live {
                    live = true;
                    if let Some(snippet) = last_line_snippet(text, 44) {
                        active_step = Some(snippet);
                    }
                }
            }
            ChatLine::ToolCall {
                name,
                target,
                running,
                ..
            } => {
                primary_steps += 1;
                if *running {
                    live = true;
                    let summary = if target.trim().is_empty() {
                        name.clone()
                    } else {
                        let short: String = target.chars().take(36).collect();
                        format!("{name} {short}")
                    };
                    active_step = Some(summary);
                }
            }
            ChatLine::Tool { text } => {
                primary_steps += 1;
                if turn_still_running && active_step.is_none() {
                    active_step = last_line_snippet(text, 40);
                }
            }
            ChatLine::ToolResult {
                is_error: false, ..
            } => {
                fallback_results += 1;
            }
            _ => {}
        }
    }
    let step_count = if primary_steps > 0 {
        primary_steps
    } else {
        fallback_results.max(1)
    };
    FoldMeta {
        step_count,
        live,
        duration_secs,
        active_step,
    }
}

pub fn clip_words(text: &str, max_chars: usize) -> Option<String> {
    let collapsed: String =
        text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    let mut chars = collapsed.chars();
    let head: String = chars.by_ref().take(max_chars).collect();
    Some(if chars.next().is_some() {
        format!("{head}\u{2026}")
    } else {
        head
    })
}

pub fn last_line_snippet(text: &str, max_chars: usize) -> Option<String> {
    let line = text.lines().rev().map(str::trim).find(|l| !l.is_empty())?;
    clip_words(line, max_chars)
}

#[cfg(test)]
mod tests {
    use super::{ChatLine, HashSet, Projection};

    #[test]
    fn completed_turn_folds_successful_details_but_keeps_errors_and_live_rows()
    {
        let lines = [
            ChatLine::User {
                text: "first".into(),
            },
            ChatLine::ToolResult {
                text: "ok".into(),
                is_error: false,
            },
            ChatLine::ToolResult {
                text: "error".into(),
                is_error: true,
            },
            ChatLine::ToolResult {
                text: "more".into(),
                is_error: false,
            },
            ChatLine::Assistant {
                text: "previous result".into(),
            },
            ChatLine::TaskReceipt {
                task_id: "inventory-test".into(),
                goal: "list files".into(),
            },
            ChatLine::User {
                text: "second".into(),
            },
            ChatLine::Reasoning {
                text: "live".into(),
                duration_secs: 0,
                live: true,
            },
            ChatLine::Assistant {
                text: "incomplete".into(),
            },
        ];
        let folded = Projection::build(&lines, Some(8), false, None);
        assert_eq!(folded.source_rows, [0, 1, 2, 4, 5, 6, 7, 8]);
        assert_eq!(folded.hidden[&3], 1);
        assert_eq!(folded.streaming, Some(7));
        assert!(!folded.folds[&1].expanded);
        let expanded = Projection::build(
            &lines,
            Some(8),
            false,
            Some(&HashSet::from([1])),
        );
        assert!(expanded.folds[&1].expanded);
        assert_eq!(expanded.source_rows, (0..lines.len()).collect::<Vec<_>>());

        let latest = Projection::build(&lines[..6], None, true, None);
        assert_eq!(latest.source_rows, [0, 1, 2, 4, 5]);
        assert_eq!(latest.hidden[&3], 1);
    }

    #[test]
    fn artifacts_after_assistant_remain_visible_when_tools_fold() {
        let lines = [
            ChatLine::User {
                text: "make a report".into(),
            },
            ChatLine::ToolResult {
                text: "generated".into(),
                is_error: false,
            },
            ChatLine::ToolResult {
                text: "published".into(),
                is_error: false,
            },
            ChatLine::Assistant {
                text: "Here is the report.".into(),
            },
            ChatLine::Artifact {
                artifact_id: "artifact-report".into(),
            },
        ];
        let projection = Projection::build(&lines, None, true, None);
        assert_eq!(projection.source_rows, [0, 1, 3, 4]);
        assert_eq!(projection.hidden[&2], 1);
        assert!(!projection.hidden.contains_key(&4));
    }

    #[test]
    fn live_collapse_aggregates_running_reasoning_and_tools_into_one_fold() {
        let lines = [
            ChatLine::User {
                text: "build q3 deck".into(),
            },
            ChatLine::Reasoning {
                text: "Planning 3 slides".into(),
                duration_secs: 2,
                live: false,
            },
            ChatLine::ToolCall {
                name: "create_deck".into(),
                target: "q3.pptx".into(),
                call_id: "c1".into(),
                running: false,
            },
            ChatLine::ToolResult {
                text: "created 3 slides".into(),
                is_error: false,
            },
            ChatLine::ToolCall {
                name: "verify_deck".into(),
                target: "q3.pptx".into(),
                call_id: "c2".into(),
                running: true,
            },
            ChatLine::ToolResult {
                text: "warning error".into(),
                is_error: true,
            },
        ];
        let live = Projection::build_live(&lines, None, false, None);
        // Only User (0), single fold anchor (1), and error row (5) are shown!
        assert_eq!(live.source_rows, [0, 1, 5]);
        assert!(!live.folds[&1].expanded);
        assert_eq!(live.hidden[&2], 1);
        assert_eq!(live.hidden[&3], 1);
        assert_eq!(live.hidden[&4], 1);
        let meta = &live.fold_meta[&1];
        assert!(meta.live);
        assert_eq!(meta.step_count, 3);
        assert_eq!(meta.active_step.as_deref(), Some("verify_deck q3.pptx"));
    }
}

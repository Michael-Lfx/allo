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
            let first_tool = if live_collapse {
                (start + 1..end).find(|&row| is_tool_row(&lines[row]))
            } else {
                None
            };
            let opening_ack = first_tool.and_then(|tool_row| {
                (start + 1..tool_row).find(|&row| {
                    matches!(
                        &lines[row],
                        ChatLine::Assistant { text } if !text.trim().is_empty()
                    )
                })
            });
            let last_process = (start + 1..end)
                .rfind(|&row| is_process_row(&lines[row], live_collapse));
            let final_reply = last_process.and_then(|lp| {
                (lp + 1..end).rfind(|&row| {
                    matches!(lines[row], ChatLine::Assistant { .. })
                })
            });
            let marker = if should_fold {
                if opening_ack.is_some() {
                    first_tool
                } else {
                    (start + 1..end)
                        .find(|&row| is_process_row(&lines[row], live_collapse))
                }
            } else {
                None
            };
            let Some(marker) = marker else {
                for source in start..end {
                    result.push(source, streaming);
                }
                continue;
            };
            let expanded =
                expanded.is_some_and(|state| state.contains(&marker));
            let meta = summarize_turn_process(
                lines,
                start + 1..end,
                is_latest_turn && !latest_completed,
                live_collapse,
                opening_ack,
                final_reply,
            );
            let is_folded = |source: usize| -> bool {
                let line = &lines[source];
                is_process_row(line, live_collapse)
                    || (live_collapse
                        && matches!(line, ChatLine::Assistant { .. })
                        && Some(source) != opening_ack
                        && Some(source) != final_reply)
            };

            result.push(start, streaming);
            let mut deferred_folded = Vec::new();
            for source in start + 1..marker {
                if is_folded(source) {
                    deferred_folded.push(source);
                } else {
                    result.push(source, streaming);
                }
            }

            let button_row = result.source_rows.len();
            result.folds.insert(
                button_row,
                Fold {
                    key: marker,
                    expanded,
                },
            );
            result.fold_meta.insert(button_row, meta);
            result.push(marker, streaming);

            for source in deferred_folded {
                if expanded {
                    result.push(source, streaming);
                } else {
                    result.hidden.insert(source, button_row);
                }
            }

            for source in marker + 1..end {
                if !expanded && is_folded(source) {
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

const fn is_tool_row(line: &ChatLine) -> bool {
    matches!(
        line,
        ChatLine::ToolCall { .. }
            | ChatLine::Tool { .. }
            | ChatLine::ToolResult {
                is_error: false,
                ..
            }
    )
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
    lines: &[ChatLine],
    range: std::ops::Range<usize>,
    turn_still_running: bool,
    include_live: bool,
    opening_ack: Option<usize>,
    final_reply: Option<usize>,
) -> FoldMeta {
    let mut primary_steps = 0usize;
    let mut fallback_results = 0usize;
    // Keep the fold live for the entire busy turn even after intermediate
    // assistant prose has flushed between tool calls; once the turn settles,
    // stale `live`/`running` flags on interrupted rows must never pin it live.
    let live = turn_still_running;
    let mut active_step: Option<String> = None;

    for source in range {
        let line = &lines[source];
        if include_live
            && matches!(line, ChatLine::Assistant { .. })
            && Some(source) != opening_ack
            && Some(source) != final_reply
        {
            primary_steps += 1;
            continue;
        }
        if !is_process_row(line, include_live) {
            continue;
        }
        match line {
            ChatLine::Reasoning { .. } => {
                primary_steps += 1;
            }
            ChatLine::ToolCall {
                name,
                target,
                running,
                ..
            } => {
                primary_steps += 1;
                if turn_still_running && *running {
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

    #[test]
    fn live_collapse_stays_live_after_intermediate_assistant_and_hides_cot_glimpse()
     {
        let lines = [
            ChatLine::User {
                text: "build landing page".into(),
            },
            ChatLine::Reasoning {
                text: "I'll do a quick glob\n- Section A: unified".into(),
                duration_secs: 4,
                live: true,
            },
            ChatLine::ToolCall {
                name: "glob".into(),
                target: "*.html".into(),
                call_id: "c1".into(),
                running: false,
            },
            ChatLine::Assistant {
                text: "我先了解一下现有项目结构。".into(),
            },
        ];
        // Turn is still running (`latest_completed == false`), even though an
        // intermediate Assistant line flushed between tool steps.
        let live = Projection::build_live(&lines, None, false, None);
        let meta = &live.fold_meta[&1];
        assert!(meta.live);
        assert_eq!(meta.active_step, None);
        // Once the turn settles (`latest_completed == true`), fold settles too.
        let settled = Projection::build_live(&lines, None, true, None);
        assert!(!settled.fold_meta[&1].live);
    }

    #[test]
    fn dot_style_keeps_opening_ack_above_fold_and_hides_mid_loop_prose() {
        let lines = [
            ChatLine::User {
                text: "查一下深圳天气".into(),
            },
            ChatLine::Reasoning {
                text: "需要先派发后台命令任务".into(),
                duration_secs: 2,
                live: false,
            },
            ChatLine::Assistant {
                text: "我派个后台任务帮你查一下深圳天气。".into(),
            },
            ChatLine::ToolCall {
                name: "dispatch_work".into(),
                target: "weather".into(),
                call_id: "c1".into(),
                running: false,
            },
            ChatLine::ToolResult {
                text: "{\"work_id\":\"w1\"}".into(),
                is_error: false,
            },
            ChatLine::Assistant {
                text: "已派发任务，我看一下执行结果：".into(),
            },
            ChatLine::ToolCall {
                name: "query_work_status".into(),
                target: "w1".into(),
                call_id: "c2".into(),
                running: false,
            },
            ChatLine::ToolResult {
                text: "running".into(),
                is_error: false,
            },
            ChatLine::Assistant {
                text: "先拿到一条过程状态，继续整理中…".into(),
            },
            ChatLine::Assistant {
                text: "深圳今天多云转晴，22°C–27°C。".into(),
            },
        ];

        // Collapsed view: User(0) -> OpeningAck(2) -> FoldPill(3) -> FinalReply(9).
        let collapsed = Projection::build_live(&lines, Some(9), true, None);
        assert_eq!(collapsed.source_rows, [0, 2, 3, 9]);
        assert_eq!(collapsed.streaming, Some(3));
        assert_eq!(collapsed.folds[&2].key, 3);
        assert!(!collapsed.folds[&2].expanded);
        assert_eq!(collapsed.hidden[&1], 2);
        assert_eq!(collapsed.hidden[&4], 2);
        assert_eq!(collapsed.hidden[&5], 2);
        assert_eq!(collapsed.hidden[&6], 2);
        assert_eq!(collapsed.hidden[&7], 2);
        assert_eq!(collapsed.hidden[&8], 2);
        assert_eq!(collapsed.fold_meta[&2].step_count, 5);

        // Expanding keeps OpeningAck above and groups deferred Reasoning inside the drawer.
        let expanded = Projection::build_live(
            &lines,
            Some(9),
            true,
            Some(&HashSet::from([3])),
        );
        assert!(expanded.folds[&2].expanded);
        assert_eq!(expanded.source_rows, [0, 2, 3, 1, 4, 5, 6, 7, 8, 9]);
        assert!(expanded.hidden.is_empty());
    }

    #[test]
    fn settled_worker_dispatch_without_final_reply_keeps_only_ack_and_fold() {
        let lines = [
            ChatLine::User {
                text: "查一下今天新闻".into(),
            },
            ChatLine::Assistant {
                text: "好，我立即查今天的新闻要点。".into(),
            },
            ChatLine::ToolCall {
                name: "dispatch_work".into(),
                target: "news".into(),
                call_id: "c1".into(),
                running: false,
            },
            ChatLine::ToolResult {
                text: "{\"work_id\":\"w2\"}".into(),
                is_error: false,
            },
        ];
        let folded = Projection::build_live(&lines, None, true, None);
        assert_eq!(folded.source_rows, [0, 1, 2]);
        assert_eq!(folded.hidden[&3], 2);
        assert_eq!(folded.fold_meta[&2].step_count, 1);
    }
}

use std::collections::{HashMap, HashSet};

use flowy_agent::ChatLine;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fold {
    /// Source index of the first collapsible row.
    pub key: usize,
    pub expanded: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Projection {
    pub source_rows: Vec<usize>,
    pub folds: HashMap<usize, Fold>,
    pub hidden: HashMap<usize, usize>,
    pub streaming: Option<usize>,
}

impl Projection {
    pub fn build(
        lines: &[ChatLine],
        streaming: Option<usize>,
        latest_completed: bool,
        expanded: Option<&HashSet<usize>>,
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
            let last_turn_row = lines[start..end]
                .iter()
                .rfind(|line| !matches!(line, ChatLine::TaskReceipt { .. }));
            let complete = (end < lines.len() || latest_completed)
                && matches!(last_turn_row, Some(ChatLine::Assistant { .. }));
            let marker = complete
                .then(|| (start + 1..end).find(|&row| collapsible(&lines[row])))
                .flatten();
            let Some(marker) = marker else {
                for source in start..end {
                    result.push(source, streaming);
                }
                continue;
            };
            let expanded =
                expanded.is_some_and(|state| state.contains(&marker));
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
                } else if !expanded && collapsible(line) {
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

const fn collapsible(line: &ChatLine) -> bool {
    matches!(
        line,
        ChatLine::Reasoning { live: false, .. }
            | ChatLine::Tool { .. }
            | ChatLine::ToolCall { running: false, .. }
            | ChatLine::ToolResult {
                is_error: false,
                ..
            }
    )
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
}

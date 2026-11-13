//! Manus-style task thread projection + mock stream script.

use std::collections::VecDeque;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Thread {
    pub id: String,
    pub title: String,
    pub items: Vec<Item>,
    pub context_chip: Option<String>,
    pub waiting: bool,
    pub streaming: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Text(String),
    Status { label: String },
    Confirm,
    Waiting(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Beat {
    /// Start a new text block (next deltas append here).
    BreakText,
    Delta(String),
    Status(String),
    Confirm,
    Waiting(String),
    Chip(String),
    Done,
}

#[derive(Debug, Default)]
pub struct MockStream {
    queue: VecDeque<(Duration, Beat)>,
}

impl Thread {
    #[must_use]
    pub fn empty(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            items: Vec::new(),
            context_chip: None,
            waiting: false,
            streaming: true,
        }
    }

    pub fn apply(&mut self, beat: Beat) {
        match beat {
            Beat::BreakText => self.items.push(Item::Text(String::new())),
            Beat::Delta(chunk) => match self.items.last_mut() {
                Some(Item::Text(text)) => text.push_str(&chunk),
                _ => self.items.push(Item::Text(chunk)),
            },
            Beat::Status(label) => {
                self.items.push(Item::Status { label });
            }
            Beat::Confirm => {
                self.items.push(Item::Confirm);
            }
            Beat::Waiting(label) => {
                self.waiting = true;
                self.items.push(Item::Waiting(label));
            }
            Beat::Chip(label) => {
                self.context_chip = Some(label);
            }
            Beat::Done => {
                self.streaming = false;
            }
        }
    }
}

impl MockStream {
    #[must_use]
    pub fn demo(title: &str, product: &str) -> Self {
        let mut queue = VecDeque::new();
        queue.push_back((
            Duration::from_millis(180),
            Beat::Chip("script.md".into()),
        ));
        push_text(
            &mut queue,
            &format!(
                "先确认工具与参考素材。当前任务会用视频生成链路完成「{title}」。"
            ),
            22,
        );
        queue.push_back((
            Duration::from_millis(420),
            Beat::Status("读取了 1 个文件".into()),
        ));
        push_text(
            &mut queue,
            "参考图生成完成后，我会按时长、镜头节奏和配乐参数合成成片，并在画布里给你预览。",
            20,
        );
        queue.push_back((
            Duration::from_millis(480),
            Beat::Status("等待角色与场景参考图生成完成…".into()),
        ));
        push_text(
            &mut queue,
            "在开始合成前，需要你确认媒体使用权。勾选后我会继续后续步骤。",
            20,
        );
        queue.push_back((Duration::from_millis(360), Beat::Confirm));
        queue.push_back((
            Duration::from_millis(220),
            Beat::Waiting(format!("{product} 将在您回复后继续")),
        ));
        queue.push_back((Duration::from_millis(0), Beat::Done));
        Self { queue }
    }

    #[must_use]
    pub fn after_confirm(reply: &str) -> Self {
        let mut queue = VecDeque::new();
        push_text(&mut queue, reply, 18);
        queue.push_back((Duration::from_millis(0), Beat::Done));
        Self { queue }
    }

    pub fn pop(&mut self) -> Option<(Duration, Beat)> {
        self.queue.pop_front()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

fn push_text(queue: &mut VecDeque<(Duration, Beat)>, text: &str, char_ms: u64) {
    queue.push_back((Duration::from_millis(char_ms.saturating_mul(2)), Beat::BreakText));
    for ch in text.chars() {
        queue.push_back((
            Duration::from_millis(char_ms),
            Beat::Delta(ch.to_string()),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::{Beat, Item, MockStream, Thread};

    #[test]
    fn mock_stream_builds_full_demo() {
        let mut stream = MockStream::demo("demo task", "Flowy");
        let mut thread = Thread::empty("1", "demo task");
        while let Some((_, beat)) = stream.pop() {
            thread.apply(beat);
        }
        assert!(!thread.streaming);
        assert!(thread.waiting);
        assert_eq!(thread.context_chip.as_deref(), Some("script.md"));
        assert!(thread.items.iter().any(|item| matches!(item, Item::Confirm)));
        assert!(thread.items.iter().any(|item| matches!(item, Item::Status { .. })));
        let text: String = thread
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");
        assert!(text.contains("demo task"));
    }

    #[test]
    fn deltas_append_to_open_text() {
        let mut thread = Thread::empty("1", "t");
        thread.apply(Beat::BreakText);
        thread.apply(Beat::Delta("你".into()));
        thread.apply(Beat::Delta("好".into()));
        assert_eq!(thread.items, vec![Item::Text("你好".into())]);
    }
}

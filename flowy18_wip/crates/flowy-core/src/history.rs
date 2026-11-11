//! Snapshot-based undo and redo.
//!
//! A flow document is small and cloning it is cheap, so snapshots are simpler
//! and less error-prone than inverse commands. Drags and text edits record one
//! snapshot per gesture rather than one per frame.

use crate::model::Document;

/// The number of undo steps kept in memory.
pub const DEFAULT_LIMIT: usize = 200;

/// An undo/redo stack of document snapshots.
#[derive(Clone, Debug)]
pub struct History {
    undo: Vec<Document>,
    redo: Vec<Document>,
    limit: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::new(DEFAULT_LIMIT)
    }
}

impl History {
    /// Creates a history with at most `limit` undo steps.
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            limit: limit.max(1),
        }
    }

    /// Records the document as it was before a change. Consecutive identical
    /// snapshots are collapsed, and a new edit clears the redo stack.
    ///
    /// Returns whether a snapshot was recorded.
    pub fn record(&mut self, before: Document) -> bool {
        if self.undo.last() == Some(&before) {
            return false;
        }
        self.undo.push(before);
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
        self.redo.clear();
        true
    }

    /// Returns the previous document, given the current one, and moves the
    /// current document onto the redo stack.
    pub fn undo(&mut self, current: &Document) -> Option<Document> {
        let previous = self.undo.pop()?;
        self.redo.push(current.clone());
        Some(previous)
    }

    /// Returns the next document, given the current one, and moves the current
    /// document back onto the undo stack.
    pub fn redo(&mut self, current: &Document) -> Option<Document> {
        let next = self.redo.pop()?;
        self.undo.push(current.clone());
        Some(next)
    }

    /// Returns whether undo is available.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Returns whether redo is available.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Drops all history.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    /// Returns the number of undo steps available.
    #[must_use]
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    /// Returns the number of redo steps available.
    #[must_use]
    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Vec2;
    use crate::model::NodeKind;

    fn doc(title: &str) -> Document {
        Document::new(title)
    }

    #[test]
    fn undo_and_redo_walk_the_history() {
        let mut history = History::default();
        let first = doc("First");
        let second = doc("Second");
        let third = doc("Third");

        history.record(first.clone());
        history.record(second.clone());

        assert_eq!(history.undo(&third), Some(second.clone()));
        assert_eq!(history.undo(&second), Some(first.clone()));
        assert!(!history.can_undo());
        assert_eq!(history.redo(&first), Some(second.clone()));
        assert_eq!(history.redo(&second), Some(third));
        assert!(!history.can_redo());
    }

    #[test]
    fn a_new_edit_clears_redo() {
        let mut history = History::default();
        history.record(doc("one"));
        let restored = history.undo(&doc("two")).unwrap();
        assert!(history.can_redo());
        history.record(restored);
        assert!(!history.can_redo());
    }

    #[test]
    fn consecutive_duplicate_snapshots_are_collapsed() {
        let mut history = History::default();
        let snapshot = doc("same");
        assert!(history.record(snapshot.clone()));
        assert!(!history.record(snapshot));
        assert_eq!(history.undo_len(), 1);
    }

    #[test]
    fn history_is_bounded() {
        let mut history = History::new(2);
        for index in 0..5 {
            let mut doc = Document::new("bounded");
            doc.add_node(NodeKind::Action, Vec2::new(index as f32, 0.0));
            history.record(doc);
        }
        assert_eq!(history.undo_len(), 2);
    }
}

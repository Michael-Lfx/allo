//! Document editing session: current document plus undo/redo.

use crate::geometry::Vec2;
use crate::history::History;
use crate::model::{Document, NodeId, NodeKind};

/// Owns a [`Document`] and its [`History`]. UI sends commands; it never mutates
/// the graph directly.
#[derive(Clone, Debug)]
pub struct DocumentSession {
    document: Document,
    history: History,
}

impl DocumentSession {
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            document: Document::new(title),
            history: History::default(),
        }
    }

    #[must_use]
    pub fn from_document(document: Document) -> Self {
        Self {
            document,
            history: History::default(),
        }
    }

    #[must_use]
    pub const fn document(&self) -> &Document {
        &self.document
    }

    #[must_use]
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    #[must_use]
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.history.undo(&self.document) else {
            return false;
        };
        self.document = previous;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.history.redo(&self.document) else {
            return false;
        };
        self.document = next;
        true
    }

    pub fn set_title(&mut self, title: impl Into<String>) {
        self.record_then(|doc| {
            doc.title = title.into();
        });
    }

    pub fn add_node(&mut self, kind: NodeKind, position: Vec2) -> NodeId {
        let mut id = NodeId(0);
        self.record_then(|doc| {
            id = doc.add_node(kind, position);
        });
        id
    }

    pub fn add_named_node(
        &mut self,
        kind: NodeKind,
        title: impl Into<String>,
        position: Vec2,
    ) -> NodeId {
        let title = title.into();
        let mut id = NodeId(0);
        self.record_then(|doc| {
            id = doc.add_node(kind, position);
            if let Some(node) = doc.nodes.iter_mut().rev().find(|n| n.id == id)
            {
                node.title = title;
            }
        });
        id
    }

    fn record_then(&mut self, edit: impl FnOnce(&mut Document)) {
        let before = self.document.clone();
        edit(&mut self.document);
        if before != self.document {
            self.history.record(before);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_node_is_undoable() {
        let mut session = DocumentSession::new("Demo");
        assert_eq!(session.document().nodes.len(), 0);
        session.add_node(NodeKind::Action, Vec2::new(10.0, 20.0));
        assert_eq!(session.document().nodes.len(), 1);
        assert!(session.undo());
        assert_eq!(session.document().nodes.len(), 0);
        assert!(session.redo());
        assert_eq!(session.document().nodes.len(), 1);
    }
}

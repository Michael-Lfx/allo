//! Actionable recovery hints for Edit / ApplyPatch failures.

/// Classifies a failed edit/patch for hint selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditFailureKind {
    OldStringNotFound,
    MultipleMatches,
    MustReadFirst,
    StaleAfterRead,
    OutsideWriteRoot,
    Other,
}

/// Best-effort classify from tool error text (engine forwards content; no JSON).
pub fn infer_edit_failure_kind(content: &str) -> EditFailureKind {
    let lower = content.to_ascii_lowercase();
    if lower.contains("must read") || lower.contains("use the read tool first") {
        EditFailureKind::MustReadFirst
    } else if lower.contains("modified externally") || lower.contains("stale") {
        EditFailureKind::StaleAfterRead
    } else if lower.contains("multiple matches") || lower.contains("occurs multiple times") {
        EditFailureKind::MultipleMatches
    } else if lower.contains("write root") || lower.contains("coding_boundary") {
        EditFailureKind::OutsideWriteRoot
    } else if lower.contains("old_string") && lower.contains("not found") {
        EditFailureKind::OldStringNotFound
    } else if lower.contains("not found") {
        EditFailureKind::OldStringNotFound
    } else {
        EditFailureKind::Other
    }
}

/// Append a short, actionable recovery paragraph to an existing error message.
pub fn append_edit_recovery_hint(base: &str, kind: EditFailureKind) -> String {
    let hint = match kind {
        EditFailureKind::OldStringNotFound => {
            "Next: re-Read the file and prefer anchor-mode Edit with fresh `line:hash` \
             anchors, or copy a smaller unique old_string. Do not retry the identical Edit."
        }
        EditFailureKind::MultipleMatches => {
            "Next: add surrounding context to old_string until unique, or set replace_all=true \
             if every occurrence should change; or switch to anchor-mode Edit."
        }
        EditFailureKind::MustReadFirst => {
            "Next: call Read on this path, then Edit with anchors (or exact text) from that Read."
        }
        EditFailureKind::StaleAfterRead => {
            "Next: Read/Grep again for fresh `line:hash` anchors, then retry the full Edit batch."
        }
        EditFailureKind::OutsideWriteRoot => {
            "Next: use a path under the session write root / working directory."
        }
        EditFailureKind::Other => {
            "Next: inspect the error, adjust path or hunk size, then retry once with a new plan."
        }
    };
    if base.contains(hint) {
        return base.to_string();
    }
    format!("{base}\n{hint}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_not_found_hint() {
        let msg = append_edit_recovery_hint("old_string not found", EditFailureKind::OldStringNotFound);
        assert!(msg.contains("re-Read"));
        assert!(msg.contains("old_string not found"));
    }

    #[test]
    fn infers_must_read() {
        assert_eq!(
            infer_edit_failure_kind("You must Read foo.rs before editing."),
            EditFailureKind::MustReadFirst
        );
    }
}

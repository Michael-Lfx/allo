//! User-visible stripping of the `<nomi-mem-citation>` protocol block.
//!
//! The model is instructed ([`crate::prompt::CITATION_CONTRACT`]) to append
//! this block so the backend can bump memory-file usage stats. The block is
//! not part of the answer: sinks and renderers must hide it.

/// Opening tag of the memory-citation protocol block.
pub const CITATION_OPEN: &str = "<nomi-mem-citation>";
/// Closing tag of the memory-citation protocol block.
pub const CITATION_CLOSE: &str = "</nomi-mem-citation>";

/// Remove every complete `<nomi-mem-citation>…</nomi-mem-citation>` block, and
/// drop a trailing unclosed opening tag (the model started the protocol but
/// the stream ended). A suffix that is only a *prefix* of the opening tag is
/// kept — it is ordinary text, not a citation.
pub fn strip_citation_blocks(text: &str) -> String {
    let mut filter = CitationBlockFilter::new();
    let mut out = filter.push(text);
    out.push_str(&filter.finish());
    out
}

/// Streaming adapter for [`strip_citation_blocks`].
///
/// `AgentStreamEvent::Text` is a delta, so the opening tag, body, and closing
/// tag often arrive in separate chunks. Holding at most
/// `CITATION_OPEN.len() - 1` (or close-tag prefix while inside a block) bytes
/// prevents a half-tag from flashing in the transcript.
#[derive(Debug, Default)]
pub struct CitationBlockFilter {
    in_block: bool,
    held: String,
}

impl CitationBlockFilter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one delta. Returns the newly visible suffix (empty when the delta
    /// was entirely citation markup or a held tag prefix).
    pub fn push(&mut self, delta: &str) -> String {
        if delta.is_empty() && self.held.is_empty() {
            return String::new();
        }
        self.held.push_str(delta);
        let mut out = String::new();
        loop {
            if self.in_block {
                if let Some(end) = self.held.find(CITATION_CLOSE) {
                    self.held.replace_range(..end + CITATION_CLOSE.len(), "");
                    self.in_block = false;
                    continue;
                }
                self.held = longest_tag_prefix_suffix(&self.held, CITATION_CLOSE);
                break;
            }
            if let Some(start) = self.held.find(CITATION_OPEN) {
                out.push_str(&self.held[..start]);
                self.held.replace_range(..start + CITATION_OPEN.len(), "");
                self.in_block = true;
                continue;
            }
            let keep = longest_tag_prefix_suffix(&self.held, CITATION_OPEN);
            let emit_end = self.held.len() - keep.len();
            out.push_str(&self.held[..emit_end]);
            self.held = keep;
            break;
        }
        out
    }

    /// End of stream. Drops an unclosed citation body; flushes a held opening
    /// prefix as literal text.
    pub fn finish(&mut self) -> String {
        if self.in_block {
            self.in_block = false;
            self.held.clear();
            String::new()
        } else {
            std::mem::take(&mut self.held)
        }
    }

    pub fn reset(&mut self) {
        self.in_block = false;
        self.held.clear();
    }
}

/// Longest suffix of `held` that is a *strict* prefix of `tag` (so a complete
/// tag is never parked here — `find` handles those).
fn longest_tag_prefix_suffix(held: &str, tag: &str) -> String {
    let max = held.len().min(tag.len().saturating_sub(1));
    for len in (1..=max).rev() {
        let start = held.len() - len;
        if held.is_char_boundary(start) && tag.starts_with(&held[start..]) {
            return held[start..].to_owned();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_removes_complete_block_after_the_answer() {
        let text = "Here is the answer.\n\n<nomi-mem-citation>\n\
project_18ccc4bf1460b224.md|note=[4 缺陷基线]\n\
</nomi-mem-citation>";
        assert_eq!(strip_citation_blocks(text), "Here is the answer.\n\n");
    }

    #[test]
    fn strip_is_noop_without_tags() {
        let text = "just a plain answer";
        assert_eq!(strip_citation_blocks(text), text);
    }

    #[test]
    fn strip_drops_unclosed_block() {
        let text = "answer\n<nomi-mem-citation>\nuser_role.md|note=[x]\n(no close tag)";
        assert_eq!(strip_citation_blocks(text), "answer\n");
    }

    #[test]
    fn strip_keeps_incomplete_open_prefix() {
        assert_eq!(strip_citation_blocks("the symbol is <"), "the symbol is <");
        assert_eq!(strip_citation_blocks("see <nomi"), "see <nomi");
    }

    #[test]
    fn strip_removes_multiple_blocks() {
        let text = "A<nomi-mem-citation>\na.md\n</nomi-mem-citation>B\
<nomi-mem-citation>\nb.md\n</nomi-mem-citation>C";
        assert_eq!(strip_citation_blocks(text), "ABC");
    }

    #[test]
    fn filter_holds_a_block_split_across_deltas() {
        let mut filter = CitationBlockFilter::new();
        assert_eq!(filter.push("Here is the answer.\n\n"), "Here is the answer.\n\n");
        assert_eq!(filter.push("<nomi-mem-"), "");
        assert_eq!(filter.push("citation>\nuser_role.md|note=[x]\n"), "");
        assert_eq!(filter.push("</nomi-mem-citation>"), "");
        assert_eq!(filter.finish(), "");
    }

    #[test]
    fn filter_resumes_after_a_closed_block() {
        let mut filter = CitationBlockFilter::new();
        assert_eq!(
            filter.push("before<nomi-mem-citation>\nx.md\n</nomi-mem-citation>after"),
            "beforeafter"
        );
        assert_eq!(filter.finish(), "");
    }

    #[test]
    fn filter_finish_flushes_a_held_open_prefix() {
        let mut filter = CitationBlockFilter::new();
        assert_eq!(filter.push("size <"), "size ");
        assert_eq!(filter.finish(), "<");
    }
}

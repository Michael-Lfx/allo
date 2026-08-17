//! Codex-compatible freeform apply_patch grammar (lean subset).
//!
//! Accepts the same markers models already know from Codex:
//! `*** Begin Patch` … `*** End Patch`, with Add/Update/Delete File hunks.
//! Converts into structured file ops that [`nomi_tools`] ApplyPatch can apply
//! via existing Edit/Write paths — no OS sandbox / escalate dependency.

use std::fmt;

const BEGIN: &str = "*** Begin Patch";
const END: &str = "*** End Patch";
const ADD: &str = "*** Add File: ";
const DELETE: &str = "*** Delete File: ";
const UPDATE: &str = "*** Update File: ";
const MOVE_TO: &str = "*** Move to: ";
const EOF: &str = "*** End of File";

/// One file operation produced by a freeform patch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchFileOp {
    Add {
        path: String,
        content: String,
    },
    Delete {
        path: String,
    },
    Update {
        path: String,
        /// Optional rename destination.
        move_to: Option<String>,
        /// Each hunk is unique old→new text for Edit-style apply.
        hunks: Vec<PatchHunk>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchHunk {
    pub old_text: String,
    pub new_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedPatch {
    pub files: Vec<PatchFileOp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchParseError {
    pub message: String,
    pub line: Option<usize>,
}

impl fmt::Display for PatchParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(n) => write!(f, "invalid patch at line {n}: {}", self.message),
            None => write!(f, "invalid patch: {}", self.message),
        }
    }
}

impl std::error::Error for PatchParseError {}

/// Parse a Codex-style freeform patch document.
pub fn parse_freeform_patch(input: &str) -> Result<ParsedPatch, PatchParseError> {
    let text = strip_fences(input.trim());
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return Err(PatchParseError {
            message: "empty patch".into(),
            line: None,
        });
    }

    let mut i = 0usize;
    // Lenient: allow missing Begin marker (models sometimes omit it).
    if lines[i].trim() == BEGIN || lines[i].trim().starts_with(BEGIN) {
        i += 1;
    }
    // Skip optional Environment ID line.
    if i < lines.len() && lines[i].trim().starts_with("*** Environment ID:") {
        i += 1;
    }

    let mut files = Vec::new();
    while i < lines.len() {
        let raw = lines[i].trim_end();
        let line = raw.trim();
        if line.is_empty() {
            i += 1;
            continue;
        }
        if line == END || line.starts_with(END) {
            break;
        }
        if let Some(path) = line.strip_prefix(ADD) {
            i += 1;
            let mut content = String::new();
            while i < lines.len() {
                let l = lines[i];
                let t = l.trim();
                if t.starts_with("*** ") && !t.starts_with('+') {
                    break;
                }
                if let Some(rest) = l.strip_prefix('+') {
                    content.push_str(rest);
                    content.push('\n');
                } else if l.starts_with("*** ") {
                    break;
                } else {
                    // Lenient: bare lines inside Add File count as content.
                    content.push_str(l);
                    content.push('\n');
                }
                i += 1;
            }
            files.push(PatchFileOp::Add {
                path: path.trim().to_string(),
                content,
            });
            continue;
        }
        if let Some(path) = line.strip_prefix(DELETE) {
            files.push(PatchFileOp::Delete {
                path: path.trim().to_string(),
            });
            i += 1;
            continue;
        }
        if let Some(path) = line.strip_prefix(UPDATE) {
            i += 1;
            let mut move_to = None;
            if i < lines.len()
                && let Some(dest) = lines[i].trim().strip_prefix(MOVE_TO)
            {
                move_to = Some(dest.trim().to_string());
                i += 1;
            }
            let mut hunks = Vec::new();
            let mut old_lines: Vec<String> = Vec::new();
            let mut new_lines: Vec<String> = Vec::new();
            let mut in_hunk = false;

            let flush = |old_lines: &mut Vec<String>,
                         new_lines: &mut Vec<String>,
                         hunks: &mut Vec<PatchHunk>| {
                if old_lines.is_empty() && new_lines.is_empty() {
                    return;
                }
                hunks.push(PatchHunk {
                    old_text: old_lines.join("\n"),
                    new_text: new_lines.join("\n"),
                });
                old_lines.clear();
                new_lines.clear();
            };

            while i < lines.len() {
                let l = lines[i];
                let t = l.trim_end();
                let trimmed = t.trim();
                if trimmed == END || trimmed.starts_with(END) {
                    break;
                }
                if trimmed.starts_with(ADD)
                    || trimmed.starts_with(DELETE)
                    || trimmed.starts_with(UPDATE)
                {
                    break;
                }
                if trimmed == EOF {
                    i += 1;
                    continue;
                }
                if trimmed == "@@" || trimmed.starts_with("@@ ") {
                    if in_hunk {
                        flush(&mut old_lines, &mut new_lines, &mut hunks);
                    }
                    in_hunk = true;
                    i += 1;
                    continue;
                }
                if let Some(rest) = t.strip_prefix('-') {
                    in_hunk = true;
                    old_lines.push(rest.to_string());
                    i += 1;
                    continue;
                }
                if let Some(rest) = t.strip_prefix('+') {
                    in_hunk = true;
                    new_lines.push(rest.to_string());
                    i += 1;
                    continue;
                }
                if let Some(rest) = t.strip_prefix(' ') {
                    in_hunk = true;
                    old_lines.push(rest.to_string());
                    new_lines.push(rest.to_string());
                    i += 1;
                    continue;
                }
                if trimmed.is_empty() {
                    // Blank line inside hunk: treat as context empty line.
                    if in_hunk {
                        old_lines.push(String::new());
                        new_lines.push(String::new());
                    }
                    i += 1;
                    continue;
                }
                return Err(PatchParseError {
                    message: format!("unexpected line in Update File hunk: {trimmed:?}"),
                    line: Some(i + 1),
                });
            }
            flush(&mut old_lines, &mut new_lines, &mut hunks);
            if hunks.is_empty() && move_to.is_none() {
                return Err(PatchParseError {
                    message: format!("Update File {path} has no hunks"),
                    line: Some(i.saturating_sub(1) + 1),
                });
            }
            files.push(PatchFileOp::Update {
                path: path.trim().to_string(),
                move_to,
                hunks,
            });
            continue;
        }
        return Err(PatchParseError {
            message: format!("expected Add/Update/Delete File marker, got {line:?}"),
            line: Some(i + 1),
        });
    }

    if files.is_empty() {
        return Err(PatchParseError {
            message: "patch contained no file hunks".into(),
            line: None,
        });
    }
    Ok(ParsedPatch { files })
}

fn strip_fences(s: &str) -> String {
    let mut t = s.trim();
    if t.starts_with("```") {
        if let Some(rest) = t.strip_prefix("```") {
            t = rest;
            if let Some(nl) = t.find('\n') {
                t = &t[nl + 1..];
            }
        }
        if let Some(end) = t.rfind("```") {
            t = t[..end].trim_end();
        }
    }
    t.to_string()
}

/// True when `input` looks like a freeform Codex patch (not JSON files[]).
pub fn looks_like_freeform_patch(input: &str) -> bool {
    let t = input.trim();
    t.contains(BEGIN)
        || t.contains(UPDATE)
        || t.contains(ADD)
        || t.contains(DELETE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_update_hunk() {
        let patch = "\
*** Begin Patch
*** Update File: src/foo.rs
@@
 fn a() {
-    old();
+    new();
 }
*** End Patch
";
        let parsed = parse_freeform_patch(patch).unwrap();
        assert_eq!(parsed.files.len(), 1);
        match &parsed.files[0] {
            PatchFileOp::Update { path, hunks, .. } => {
                assert_eq!(path, "src/foo.rs");
                assert_eq!(hunks.len(), 1);
                assert!(hunks[0].old_text.contains("old()"));
                assert!(hunks[0].new_text.contains("new()"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parses_add_and_delete() {
        let patch = "\
*** Begin Patch
*** Add File: new.txt
+hello
+world
*** Delete File: gone.txt
*** End Patch
";
        let parsed = parse_freeform_patch(patch).unwrap();
        assert_eq!(parsed.files.len(), 2);
    }

    #[test]
    fn detects_freeform() {
        assert!(looks_like_freeform_patch("*** Update File: a.rs\n@@\n-a\n+b\n"));
        assert!(!looks_like_freeform_patch(r#"{"files":[]}"#));
    }
}

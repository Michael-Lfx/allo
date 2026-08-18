use std::path::Path;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use serde_json::{Value, json};

use nomi_protocol::events::ToolCategory;
use nomi_types::tool::{JsonSchema, ToolResult};

use crate::Tool;
use crate::anchors::{
    AnchorValidation, parse_anchor, render_anchor_region, structural_closer_ok,
    validate_anchor_default,
};
use crate::file_cache::{FileStateCache, file_mtime_ms, update_cache_after_write};

/// A single find/replace operation within a file.
pub(crate) struct EditOp {
    pub old_string: String,
    pub new_string: String,
    pub replace_all: bool,
}

/// One anchor-mode edit (preferred coding path).
struct AnchorEditOp {
    anchor: String,
    end_anchor: Option<String>,
    new_text: String,
    insert_after: bool,
}

fn apply_anchor_edits(content: &str, ops: &[AnchorEditOp]) -> Result<(String, usize, String), String> {
    if ops.is_empty() {
        return Err("edits array is empty — provide at least one anchor edit.".into());
    }
    let lines: Vec<&str> = content.lines().collect();
    // Keep trailing newline info.
    let ends_with_newline = content.ends_with('\n');

    struct Resolved {
        start: usize, // 0-based inclusive
        end: usize,   // 0-based inclusive
        new_text: String,
        insert_after: bool,
    }

    let mut resolved = Vec::with_capacity(ops.len());
    let mut stale_reports = Vec::new();

    for (i, op) in ops.iter().enumerate() {
        let start_anchor = match parse_anchor(&op.anchor) {
            Some(a) => a,
            None => {
                return Err(format!(
                    "edits[{i}].anchor {:?} is not a valid line:hash anchor. Copy the WHOLE \
                     \"line:hash\" prefix from Read/Grep output (e.g. \"42:h7x2\").",
                    op.anchor
                ));
            }
        };
        if op.insert_after && op.end_anchor.is_some() {
            return Err(format!(
                "edits[{i}]: insert_after cannot be combined with end_anchor."
            ));
        }
        let start_line = match validate_anchor_default(&lines, &start_anchor) {
            AnchorValidation::Ok { line } | AnchorValidation::Shifted { line } => line,
            AnchorValidation::Stale => {
                stale_reports.push(format!(
                    "edits[{i}] anchor \"{}\" is STALE (content changed). Fresh anchors near line {}:\n{}",
                    op.anchor,
                    start_anchor.line,
                    render_anchor_region(&lines, start_anchor.line, 3)
                ));
                continue;
            }
        };

        let end_line = if let Some(ref end_raw) = op.end_anchor {
            let end_anchor = match parse_anchor(end_raw) {
                Some(a) => a,
                None => {
                    return Err(format!(
                        "edits[{i}].end_anchor {:?} is not a valid line:hash anchor.",
                        end_raw
                    ));
                }
            };
            match validate_anchor_default(&lines, &end_anchor) {
                AnchorValidation::Ok { line } | AnchorValidation::Shifted { line } => {
                    if start_line > line {
                        stale_reports.push(format!(
                            "edits[{i}] range inverted (start {start_line} > end {line}). Fresh anchors:\n{}",
                            render_anchor_region(&lines, start_line, 3)
                        ));
                        continue;
                    }
                    if !structural_closer_ok(lines[line - 1], &op.new_text) {
                        return Err(format!(
                            "Anchor replacement rejected: end line {} is a structural closer, but \
                             new_text drops that closer. Include the closing line in new_text, or \
                             use empty new_text to delete. Fresh anchors:\n{}",
                            line,
                            render_anchor_region(&lines, line, 3)
                        ));
                    }
                    line
                }
                AnchorValidation::Stale => {
                    stale_reports.push(format!(
                        "edits[{i}] end_anchor \"{}\" is STALE. Fresh anchors near line {}:\n{}",
                        end_raw,
                        end_anchor.line,
                        render_anchor_region(&lines, end_anchor.line, 3)
                    ));
                    continue;
                }
            }
        } else {
            start_line
        };

        resolved.push(Resolved {
            start: start_line - 1,
            end: end_line - 1,
            new_text: op.new_text.clone(),
            insert_after: op.insert_after,
        });
    }

    if !stale_reports.is_empty() {
        return Err(format!(
            "Anchor edit rejected — {} of {} anchor(s) failed; NO changes were made (edits are atomic).\n\n{}\n\n\
             Retry the FULL batch using the fresh anchors above. Never fabricate or reuse stale anchors.",
            stale_reports.len(),
            ops.len(),
            stale_reports.join("\n\n")
        ));
    }

    // Apply bottom-up so indices stay valid.
    resolved.sort_by(|a, b| b.start.cmp(&a.start).then(b.end.cmp(&a.end)));
    let mut out_lines: Vec<String> = lines.iter().map(|s| (*s).to_string()).collect();
    for r in &resolved {
        if r.insert_after {
            let insert_at = r.start + 1;
            let new_lines: Vec<String> = if r.new_text.is_empty() {
                Vec::new()
            } else {
                r.new_text.lines().map(str::to_string).collect()
            };
            for (offset, line) in new_lines.into_iter().enumerate() {
                out_lines.insert(insert_at + offset, line);
            }
        } else {
            let new_lines: Vec<String> = if r.new_text.is_empty() {
                Vec::new()
            } else {
                r.new_text.lines().map(str::to_string).collect()
            };
            out_lines.splice(r.start..=r.end, new_lines);
        }
    }

    let mut new_content = out_lines.join("\n");
    if ends_with_newline && !new_content.ends_with('\n') {
        new_content.push('\n');
    }

    let receipt_centers: Vec<usize> = {
        let mut v: Vec<_> = resolved.iter().map(|r| r.start + 1).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let out_refs: Vec<&str> = out_lines.iter().map(|s| s.as_str()).collect();
    let mut receipt = String::new();
    for center in receipt_centers.iter().take(5) {
        if !receipt.is_empty() {
            receipt.push_str("\n\n");
        }
        receipt.push_str(&render_anchor_region(&out_refs, *center, 3));
    }

    Ok((new_content, resolved.len(), receipt))
}

fn edits_array_is_anchor_mode(arr: &[Value]) -> bool {
    arr.iter().any(|e| e.get("anchor").is_some())
}

/// Build a whitespace-normalized view (CRLF→LF, trim trailing ws per line)
/// plus a map from each normalized char index to the original byte offset.
fn normalize_ws_mapped(s: &str) -> (String, Vec<usize>) {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let mut norm = String::new();
    let mut map = Vec::new();
    let mut idx = 0usize;
    while idx < chars.len() {
        let line_start = idx;
        while idx < chars.len() && chars[idx].1 != '\n' && chars[idx].1 != '\r' {
            idx += 1;
        }
        let line_end = idx;
        let mut trimmed_end = line_end;
        while trimmed_end > line_start {
            let c = chars[trimmed_end - 1].1;
            if c == ' ' || c == '\t' {
                trimmed_end -= 1;
            } else {
                break;
            }
        }
        for i in line_start..trimmed_end {
            map.push(chars[i].0);
            norm.push(chars[i].1);
        }
        if idx < chars.len() {
            if chars[idx].1 == '\r' {
                map.push(chars[idx].0);
                norm.push('\n');
                idx += 1;
                if idx < chars.len() && chars[idx].1 == '\n' {
                    idx += 1;
                }
            } else if chars[idx].1 == '\n' {
                map.push(chars[idx].0);
                norm.push('\n');
                idx += 1;
            }
        }
    }
    (norm, map)
}

/// When exact match fails, try a unique whitespace-tolerant match and replace
/// the corresponding original region.
fn try_whitespace_tolerant_replace(content: &str, old: &str, new: &str) -> Option<String> {
    let (norm_content, map) = normalize_ws_mapped(content);
    let (norm_old, _) = normalize_ws_mapped(old);
    if norm_old.is_empty() {
        return None;
    }
    let matches: Vec<usize> = norm_content.match_indices(&norm_old).map(|(i, _)| i).collect();
    if matches.len() != 1 {
        return None;
    }
    let start = matches[0];
    let end = start + norm_old.len();
    if start >= map.len() {
        return None;
    }
    let orig_start = map[start];
    let orig_end = if end < map.len() {
        map[end]
    } else {
        content.len()
    };
    if orig_start > orig_end || orig_end > content.len() {
        return None;
    }
    let mut out = String::with_capacity(content.len() - (orig_end - orig_start) + new.len());
    out.push_str(&content[..orig_start]);
    out.push_str(new);
    out.push_str(&content[orig_end..]);
    Some(out)
}

fn not_found_message(label: &str, content: &str, old: &str) -> String {
    let mut msg = format!(
        "{label}old_string not found in file. Hint: re-Read the file and copy the exact text (including indentation)."
    );
    let prefix: String = old.chars().take(40).collect();
    if !prefix.is_empty()
        && let Some(pos) = content.find(&prefix)
    {
        let end = (pos + prefix.len() + 40).min(content.len());
        let snippet: String = content[pos..end]
            .chars()
            .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
            .take(80)
            .collect();
        msg.push_str(&format!(" Found similar text near: {snippet:?}"));
    }
    nomi_coding::append_edit_recovery_hint(&msg, nomi_coding::EditFailureKind::OldStringNotFound)
}

fn classify_apply_err(msg: &str) -> nomi_coding::EditFailureKind {
    if msg.contains("not found") {
        nomi_coding::EditFailureKind::OldStringNotFound
    } else if msg.contains("Multiple matches") {
        nomi_coding::EditFailureKind::MultipleMatches
    } else {
        nomi_coding::EditFailureKind::Other
    }
}

/// Apply a sequence of edits to `content` in order, returning the new content
/// and the total number of replacements. All-or-nothing: if any hunk fails to
/// match (or is ambiguous without `replace_all`), returns `Err` and the caller
/// MUST NOT write — so a multi-edit never leaves a file partially modified.
/// Later hunks see the text produced by earlier ones (sequential semantics).
/// Single-edit error messages stay unprefixed for backward compatibility.
pub(crate) fn apply_edits(content: &str, ops: &[EditOp]) -> Result<(String, usize), String> {
    let multi = ops.len() > 1;
    let mut current = content.to_string();
    let mut total = 0usize;
    for (i, op) in ops.iter().enumerate() {
        let label = if multi { format!("edit #{}: ", i + 1) } else { String::new() };
        let count = current.matches(&op.old_string).count();
        if count == 0 {
            if !op.replace_all
                && let Some(replaced) =
                    try_whitespace_tolerant_replace(&current, &op.old_string, &op.new_string)
            {
                current = replaced;
                total += 1;
                continue;
            }
            return Err(not_found_message(&label, &current, &op.old_string));
        }
        if count > 1 && !op.replace_all {
            return Err(nomi_coding::append_edit_recovery_hint(
                &format!(
                    "{label}Multiple matches found ({count}). Use replace_all or provide more context."
                ),
                nomi_coding::EditFailureKind::MultipleMatches,
            ));
        }
        current = if op.replace_all {
            current.replace(&op.old_string, &op.new_string)
        } else {
            current.replacen(&op.old_string, &op.new_string, 1)
        };
        total += if op.replace_all { count } else { 1 };
    }
    Ok((current, total))
}

pub struct EditTool {
    file_cache: Option<Arc<RwLock<FileStateCache>>>,
    /// Optional containment root; when set, edits outside it are rejected.
    write_root: Option<std::path::PathBuf>,
    /// Session working directory used to resolve relative `file_path` inputs
    /// (matching ReadTool / Grep / Glob / Bash). `None` = legacy process-cwd.
    cwd: Option<std::path::PathBuf>,
    /// When true, write-root rejections use CODING_BOUNDARY: copy.
    coding_boundary: bool,
}

impl EditTool {
    /// Create an EditTool with optional file state cache.
    ///
    /// When cache is `Some`, the tool enforces:
    /// - "Must Read first" guard (file must be in cache before editing)
    /// - Staleness detection (disk mtime must match cached mtime)
    /// - Post-write cache update (mtime + content refreshed after edit)
    ///
    /// Pass `None` to disable all cache-related guards (legacy behavior).
    pub fn new(file_cache: Option<Arc<RwLock<FileStateCache>>>) -> Self {
        Self {
            file_cache,
            write_root: None,
            cwd: None,
            coding_boundary: false,
        }
    }

    /// Restrict edits to within `root` (design §3.6 write-root containment).
    pub fn with_write_root(mut self, root: Option<std::path::PathBuf>) -> Self {
        self.write_root = root;
        self
    }

    /// Resolve relative `file_path` inputs against `cwd` (the session working
    /// directory), matching ReadTool/Grep/Glob/Bash.
    pub fn with_cwd(mut self, cwd: Option<std::path::PathBuf>) -> Self {
        self.cwd = cwd;
        self
    }

    /// Use coding-mode boundary error copy for write-root rejections.
    pub fn with_coding_boundary(mut self, enabled: bool) -> Self {
        self.coding_boundary = enabled;
        self
    }
}

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "Edit"
    }

    fn description(&self) -> &str {
        "Edit a file surgically. Two modes — anchor mode (preferred) and exact-text mode.\n\n\
         ANCHOR MODE (preferred): pass `edits`, an array of { anchor, end_anchor?, new_text, insert_after? }.\n\
         - Anchors are the `line:hash` prefixes in Read/Grep/Edit output (e.g. `42:h7x2` from \
         `42:h7x2→…`). Copy them VERBATIM — never fabricate hashes.\n\
         - One edit replaces the anchor line (or inclusive range anchor..end_anchor) with new_text. \
         Empty new_text deletes the range. insert_after: true inserts after the anchor line.\n\
         - Atomic: if ANY anchor is stale the whole batch is rejected and fresh anchors are returned.\n\n\
         EXACT-TEXT MODE (fallback): old_string + new_string, or edits:[{old_string,new_string,replace_all?}].\n\
         - You must Read the file first. Prefer anchor mode when you have Read/Grep output.\n\
         - Multi exact edits are atomic (all or nothing).\n\
         - Prefer Edit over Write for modifying existing files."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file to modify (absolute preferred; a relative path resolves against the session working directory)"
                },
                "old_string": {
                    "type": "string",
                    "description": "Exact-text mode: the text to replace"
                },
                "new_string": {
                    "type": "string",
                    "description": "Exact-text mode: the replacement text"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Exact-text mode: replace all occurrences (default false)"
                },
                "edits": {
                    "type": "array",
                    "description": "Batch edits: either anchor-mode objects ({anchor, new_text, ...}) or exact-text objects ({old_string, new_string}). Do not mix modes in one call. Atomic.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "anchor": {
                                "type": "string",
                                "description": "Anchor mode: whole line:hash prefix from Read/Grep (e.g. \"42:h7x2\")"
                            },
                            "end_anchor": {
                                "type": "string",
                                "description": "Anchor mode: inclusive end anchor for a multi-line range"
                            },
                            "new_text": {
                                "type": "string",
                                "description": "Anchor mode: replacement text (empty = delete range)"
                            },
                            "insert_after": {
                                "type": "boolean",
                                "description": "Anchor mode: insert new_text after the anchor line instead of replacing"
                            },
                            "old_string": { "type": "string", "description": "Exact-text mode: text to replace" },
                            "new_string": { "type": "string", "description": "Exact-text mode: replacement text" },
                            "replace_all": { "type": "boolean", "description": "Exact-text mode: replace all" }
                        }
                    }
                }
            },
            "required": ["file_path"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let Some(file_path) = input["file_path"].as_str() else {
            return ToolResult {
                content: "Missing required parameter: file_path".to_string(),
                is_error: true,
                images: Vec::new(),
            };
        };

        // Resolve a relative file_path against the session working directory
        // (matching ReadTool/Grep/Glob/Bash) before any filesystem use.
        let resolved = crate::path_guard::resolve_against_cwd(file_path, self.cwd.as_deref());
        let file_path = resolved.as_str();

        // Write-root containment (opt-in): reject edits outside the configured root.
        if let Some(msg) = crate::path_guard::ensure_within_root_ex(
            file_path,
            self.write_root.as_deref(),
            self.coding_boundary,
        ) {
            return ToolResult {
                content: msg,
                is_error: true,
                images: Vec::new(),
            };
        }

        // Accept anchor-mode `edits`, exact-text `edits`, or legacy single old/new.
        let anchor_ops: Option<Vec<AnchorEditOp>> = if let Some(arr) = input["edits"].as_array() {
            if arr.is_empty() {
                return ToolResult {
                    content: "edits array must not be empty".to_string(),
                    is_error: true,
                    images: Vec::new(),
                };
            }
            if edits_array_is_anchor_mode(arr) {
                let mut ops = Vec::with_capacity(arr.len());
                for (i, e) in arr.iter().enumerate() {
                    if e.get("old_string").is_some() || e.get("new_string").is_some() {
                        return ToolResult {
                            content: format!(
                                "edit #{}: do not mix anchor fields with old_string/new_string in one batch",
                                i + 1
                            ),
                            is_error: true,
                            images: Vec::new(),
                        };
                    }
                    let Some(anchor) = e["anchor"].as_str() else {
                        return ToolResult {
                            content: format!("edit #{}: missing anchor", i + 1),
                            is_error: true,
                            images: Vec::new(),
                        };
                    };
                    let Some(new_text) = e["new_text"].as_str() else {
                        return ToolResult {
                            content: format!("edit #{}: missing new_text", i + 1),
                            is_error: true,
                            images: Vec::new(),
                        };
                    };
                    ops.push(AnchorEditOp {
                        anchor: anchor.to_string(),
                        end_anchor: e["end_anchor"].as_str().map(str::to_string),
                        new_text: new_text.to_string(),
                        insert_after: e["insert_after"].as_bool().unwrap_or(false),
                    });
                }
                Some(ops)
            } else {
                None
            }
        } else {
            None
        };

        let ops: Option<Vec<EditOp>> = if anchor_ops.is_some() {
            None
        } else if let Some(arr) = input["edits"].as_array() {
            let mut ops = Vec::with_capacity(arr.len());
            for (i, e) in arr.iter().enumerate() {
                let (Some(o), Some(n)) = (e["old_string"].as_str(), e["new_string"].as_str()) else {
                    return ToolResult {
                        content: format!("edit #{}: missing old_string or new_string", i + 1),
                        is_error: true,
                        images: Vec::new(),
                    };
                };
                ops.push(EditOp {
                    old_string: o.to_string(),
                    new_string: n.to_string(),
                    replace_all: e["replace_all"].as_bool().unwrap_or(false),
                });
            }
            Some(ops)
        } else {
            let Some(old_string) = input["old_string"].as_str() else {
                return ToolResult {
                    content: "Missing edit payload: provide `edits` (anchor mode, preferred) or both old_string and new_string.".to_string(),
                    is_error: true,
                    images: Vec::new(),
                };
            };
            let Some(new_string) = input["new_string"].as_str() else {
                return ToolResult {
                    content: "Missing required parameter: new_string".to_string(),
                    is_error: true,
                    images: Vec::new(),
                };
            };
            Some(vec![EditOp {
                old_string: old_string.to_string(),
                new_string: new_string.to_string(),
                replace_all: input["replace_all"].as_bool().unwrap_or(false),
            }])
        };

        let path = Path::new(file_path);

        // Cache guard: "must Read first" + staleness detection.
        if let Some(cache_arc) = &self.file_cache
            && let Ok(mut cache) = cache_arc.write()
        {
            let cached = cache.get(path);
            if cached.is_none() {
                return ToolResult {
                    content: nomi_coding::append_edit_recovery_hint(
                        &format!(
                            "You must Read {} before editing. Use the Read tool first \
                             so the file content is loaded into context.",
                            file_path
                        ),
                        nomi_coding::EditFailureKind::MustReadFirst,
                    ),
                    is_error: true,
                    images: Vec::new(),
                };
            }
            // Staleness check: compare cached mtime with current disk mtime.
            let cached_mtime = cached.map(|s| s.mtime_ms);
            let disk_mtime = file_mtime_ms(path);
            if let (Some(cached_mt), Some(disk_mt)) = (cached_mtime, disk_mtime)
                && cached_mt != disk_mt
            {
                return ToolResult {
                    content: nomi_coding::append_edit_recovery_hint(
                        &format!(
                            "File {} has been modified externally since last read. \
                             Read the file again to see the current content before editing.",
                            file_path
                        ),
                        nomi_coding::EditFailureKind::StaleAfterRead,
                    ),
                    is_error: true,
                    images: Vec::new(),
                };
            }
        }

        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(e) => {
                return ToolResult {
                    content: crate::path_guard::format_file_read_error(file_path, &e),
                    is_error: true,
                    images: Vec::new(),
                };
            }
        };

        let (new_content, summary) = if let Some(anchor_ops) = anchor_ops {
            match apply_anchor_edits(&content, &anchor_ops) {
                Ok((new_content, count, receipt)) => {
                    let summary = format!(
                        "Applied {count} anchor edit(s) to {file_path}.\n\n\
                         Fresh anchors around the changes (use these to continue editing):\n{receipt}"
                    );
                    (new_content, summary)
                }
                Err(msg) => {
                    let kind = if msg.to_ascii_lowercase().contains("stale") {
                        nomi_coding::EditFailureKind::StaleAfterRead
                    } else {
                        classify_apply_err(&msg)
                    };
                    return ToolResult {
                        content: nomi_coding::append_edit_recovery_hint(&msg, kind),
                        is_error: true,
                        images: Vec::new(),
                    };
                }
            }
        } else {
            let ops = ops.expect("exact-text ops");
            match apply_edits(&content, &ops) {
                Ok((new_content, total)) => {
                    let summary = if ops.len() > 1 {
                        format!(
                            "Edited {}: {} replacement(s) across {} edits",
                            file_path,
                            total,
                            ops.len()
                        )
                    } else {
                        format!("Edited {}: replaced {} occurrence(s)", file_path, total)
                    };
                    (new_content, summary)
                }
                Err(msg) => {
                    let kind = classify_apply_err(&msg);
                    return ToolResult {
                        content: nomi_coding::append_edit_recovery_hint(&msg, kind),
                        is_error: true,
                        images: Vec::new(),
                    };
                }
            }
        };

        if let Err(e) = crate::atomic_write(file_path, &new_content) {
            return ToolResult {
                content: format!("Failed to write file: {}", e),
                is_error: true,
                images: Vec::new(),
            };
        }

        // Post-write cache update: refresh mtime and content.
        if let Some(cache_arc) = &self.file_cache {
            update_cache_after_write(cache_arc, path, &new_content);
        }

        ToolResult {
            content: summary,
            is_error: false,
            images: Vec::new(),
        }
    }

    fn max_result_size(&self) -> usize {
        10_000
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Edit
    }

    fn describe(&self, input: &Value) -> String {
        let path = input
            .get("file_path")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        format!("Edit {}", path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    use crate::file_cache::update_cache_after_write;
    use nomi_config::file_cache::FileCacheConfig;

    fn make_cache() -> Arc<RwLock<FileStateCache>> {
        let config = FileCacheConfig {
            max_entries: 100,
            max_size_bytes: 25 * 1024 * 1024,
            enabled: true,
        };
        Arc::new(RwLock::new(FileStateCache::new(&config)))
    }

    /// Simulate a Read by inserting a cache entry for the given file path.
    fn simulate_read(cache: &Arc<RwLock<FileStateCache>>, path: &Path) {
        let content = std::fs::read_to_string(path).unwrap_or_default();
        update_cache_after_write(cache, path, &content);
    }

    #[tokio::test]
    async fn edit_resolves_relative_path_against_cwd() {
        // A relative file_path must resolve against the injected workspace cwd,
        // not the process cwd. Use no cache so the must-read guard is off.
        let workspace = tempdir().unwrap();
        let rel = "__nomi_reltest_edit__.txt";
        std::fs::write(workspace.path().join(rel), "alpha").unwrap();
        let tool = EditTool::new(None).with_cwd(Some(workspace.path().to_path_buf()));
        let result = tool
            .execute(json!({ "file_path": rel, "old_string": "alpha", "new_string": "beta" }))
            .await;
        assert!(!result.is_error, "relative edit should succeed: {}", result.content);
        assert_eq!(
            std::fs::read_to_string(workspace.path().join(rel)).unwrap(),
            "beta",
            "the relative edit must have applied to the workspace file"
        );
    }

    #[tokio::test]
    async fn multi_edit_applies_all_hunks_in_one_call() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("m.txt");
        std::fs::write(&file_path, "alpha beta gamma").unwrap();
        let tool = EditTool::new(None);
        let input = json!({
            "file_path": file_path.to_str().unwrap(),
            "edits": [
                { "old_string": "alpha", "new_string": "A" },
                { "old_string": "gamma", "new_string": "G" }
            ]
        });
        let result = tool.execute(input).await;
        assert!(!result.is_error, "unexpected error: {}", result.content);
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "A beta G");
    }

    #[tokio::test]
    async fn multi_edit_failing_hunk_leaves_file_untouched() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("m.txt");
        std::fs::write(&file_path, "alpha beta").unwrap();
        let tool = EditTool::new(None);
        let input = json!({
            "file_path": file_path.to_str().unwrap(),
            "edits": [
                { "old_string": "alpha", "new_string": "A" },
                { "old_string": "NOPE", "new_string": "x" }
            ]
        });
        let result = tool.execute(input).await;
        assert!(result.is_error, "a failing hunk must fail the whole edit");
        // Atomic: the first (matching) hunk must NOT have been written.
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "alpha beta");
    }

    // -- Legacy tests (no cache) --

    #[test]
    fn apply_edits_applies_multiple_hunks_in_order() {
        use super::{EditOp, apply_edits};
        let ops = vec![
            EditOp { old_string: "foo".into(), new_string: "bar".into(), replace_all: false },
            EditOp { old_string: "bar".into(), new_string: "baz".into(), replace_all: false },
        ];
        // Sequential: edit 1 foo->bar => "bar X"; edit 2 sees "bar" and -> baz => "baz X".
        let (out, n) = apply_edits("foo X", &ops).unwrap();
        assert_eq!(out, "baz X");
        assert_eq!(n, 2);
    }

    #[test]
    fn apply_edits_aborts_on_missing_hunk_identifying_which() {
        use super::{EditOp, apply_edits};
        let ops = vec![
            EditOp { old_string: "foo".into(), new_string: "bar".into(), replace_all: false },
            EditOp { old_string: "NOPE".into(), new_string: "x".into(), replace_all: false },
        ];
        let err = apply_edits("foo", &ops).unwrap_err();
        assert!(err.contains("not found"));
        assert!(err.contains("edit #2"), "must identify the failing hunk: {err}");
    }

    #[test]
    fn apply_edits_replace_all_counts_all_occurrences() {
        use super::{EditOp, apply_edits};
        let ops = vec![EditOp { old_string: "a".into(), new_string: "b".into(), replace_all: true }];
        let (out, n) = apply_edits("a a a", &ops).unwrap();
        assert_eq!(out, "b b b");
        assert_eq!(n, 3);
    }

    #[test]
    fn apply_edits_single_hunk_messages_unprefixed() {
        use super::{EditOp, apply_edits};
        // A single edit keeps the legacy unprefixed error message (back-compat).
        let ops = vec![EditOp { old_string: "x".into(), new_string: "y".into(), replace_all: false }];
        let err = apply_edits("no match here", &ops).unwrap_err();
        assert!(
            err.starts_with("old_string not found in file"),
            "single-edit errors stay unprefixed: {err}"
        );
        assert!(err.contains("Hint: re-Read"), "{err}");
    }

    #[test]
    fn apply_edits_whitespace_tolerant_unique_match() {
        use super::{EditOp, apply_edits};
        // File has trailing spaces / CRLF; needle uses LF without trailing spaces.
        let content = "fn main() {  \r\n    println!(\"hi\");  \r\n}\r\n";
        let old = "fn main() {\n    println!(\"hi\");\n}";
        let ops = vec![EditOp {
            old_string: old.into(),
            new_string: "fn main() {\n    println!(\"yo\");\n}".into(),
            replace_all: false,
        }];
        let (out, n) = apply_edits(content, &ops).unwrap();
        assert_eq!(n, 1);
        assert!(out.contains("println!(\"yo\")"), "{out}");
        assert!(!out.contains("println!(\"hi\")"), "{out}");
    }

    #[test]
    fn atomic_write_creates_and_replaces_without_leftover_temp() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("f.txt");
        let ps = p.to_str().unwrap();

        crate::atomic_write(ps, "hello").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "hello");

        crate::atomic_write(ps, "world").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "world");

        // The temp file must be renamed onto the target, never left behind.
        let leftover = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains(".tmp."));
        assert!(!leftover, "atomic_write must rename the temp file away");
    }

    #[tokio::test]
    async fn test_edit_replace_block() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello world").unwrap();

        let tool = EditTool::new(None);
        let input = json!({
            "file_path": file_path.to_str().unwrap(),
            "old_string": "hello",
            "new_string": "goodbye"
        });

        let result = tool.execute(input).await;

        assert!(!result.is_error, "unexpected error: {}", result.content);
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "goodbye world");
    }

    #[tokio::test]
    async fn test_edit_old_string_not_found() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello world").unwrap();

        let tool = EditTool::new(None);
        let input = json!({
            "file_path": file_path.to_str().unwrap(),
            "old_string": "nonexistent",
            "new_string": "replacement"
        });

        let result = tool.execute(input).await;

        assert!(result.is_error);
        assert!(
            result.content.contains("not found"),
            "expected 'not found' in error message, got: {}",
            result.content
        );
    }

    #[tokio::test]
    async fn test_edit_preserves_surrounding() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "aaa\nbbb\nccc\n").unwrap();

        let tool = EditTool::new(None);
        let input = json!({
            "file_path": file_path.to_str().unwrap(),
            "old_string": "bbb",
            "new_string": "XXX"
        });

        let result = tool.execute(input).await;

        assert!(!result.is_error, "unexpected error: {}", result.content);
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "aaa\nXXX\nccc\n");
    }

    #[tokio::test]
    async fn test_edit_nonexistent_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("does_not_exist.txt");

        let tool = EditTool::new(None);
        let input = json!({
            "file_path": file_path.to_str().unwrap(),
            "old_string": "anything",
            "new_string": "replacement"
        });

        let result = tool.execute(input).await;

        assert!(result.is_error);
        assert!(
            result.content.contains("Failed to read file"),
            "expected read failure message, got: {}",
            result.content
        );
        assert!(
            result.content.contains(&format!("'{}'", file_path.display())),
            "path must be quoted: {}",
            result.content
        );
    }

    // -- Cache guard tests --

    #[tokio::test]
    async fn edit_without_read_returns_error() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("unread.txt");
        std::fs::write(&file_path, "hello").unwrap();

        let cache = make_cache();
        let tool = EditTool::new(Some(cache));

        let input = json!({
            "file_path": file_path.to_str().unwrap(),
            "old_string": "hello",
            "new_string": "bye"
        });

        let result = tool.execute(input).await;

        assert!(result.is_error);
        assert!(
            result.content.contains("must Read"),
            "expected 'must Read' in error: {}",
            result.content
        );
        // File must be unchanged.
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "hello");
    }

    #[tokio::test]
    async fn edit_after_read_succeeds() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("read_then_edit.txt");
        std::fs::write(&file_path, "hello world").unwrap();

        let cache = make_cache();
        simulate_read(&cache, &file_path);

        let tool = EditTool::new(Some(cache));
        let input = json!({
            "file_path": file_path.to_str().unwrap(),
            "old_string": "hello",
            "new_string": "goodbye"
        });

        let result = tool.execute(input).await;

        assert!(!result.is_error, "unexpected error: {}", result.content);
        assert_eq!(
            std::fs::read_to_string(&file_path).unwrap(),
            "goodbye world"
        );
    }

    #[tokio::test]
    async fn edit_detects_external_modification() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("stale.txt");
        std::fs::write(&file_path, "original").unwrap();

        let cache = make_cache();
        simulate_read(&cache, &file_path);

        // External modification: change file after caching.
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(&file_path, "externally changed").unwrap();

        let tool = EditTool::new(Some(cache));
        let input = json!({
            "file_path": file_path.to_str().unwrap(),
            "old_string": "original",
            "new_string": "new"
        });

        let result = tool.execute(input).await;

        assert!(result.is_error);
        assert!(
            result.content.contains("modified externally"),
            "expected staleness error: {}",
            result.content
        );
    }

    #[tokio::test]
    async fn edit_then_edit_succeeds_via_cache_update() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("double_edit.txt");
        std::fs::write(&file_path, "aaa bbb ccc").unwrap();

        let cache = make_cache();
        simulate_read(&cache, &file_path);

        let tool = EditTool::new(Some(cache));

        // First edit.
        let input1 = json!({
            "file_path": file_path.to_str().unwrap(),
            "old_string": "aaa",
            "new_string": "AAA"
        });
        let r1 = tool.execute(input1).await;
        assert!(!r1.is_error, "first edit failed: {}", r1.content);

        // Second edit should succeed because first edit updated the cache.
        let input2 = json!({
            "file_path": file_path.to_str().unwrap(),
            "old_string": "bbb",
            "new_string": "BBB"
        });
        let r2 = tool.execute(input2).await;
        assert!(!r2.is_error, "second edit failed: {}", r2.content);
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "AAA BBB ccc");
    }

    #[tokio::test]
    async fn no_cache_edit_bypasses_guard() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("nocache.txt");
        std::fs::write(&file_path, "hello").unwrap();

        let tool = EditTool::new(None);
        let input = json!({
            "file_path": file_path.to_str().unwrap(),
            "old_string": "hello",
            "new_string": "bye"
        });

        let result = tool.execute(input).await;
        assert!(
            !result.is_error,
            "expected success without cache: {}",
            result.content
        );
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "bye");
    }

    #[tokio::test]
    async fn replace_all_updates_cache() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("replaceall.txt");
        std::fs::write(&file_path, "a-a-a").unwrap();

        let cache = make_cache();
        simulate_read(&cache, &file_path);

        let tool = EditTool::new(Some(cache.clone()));
        let input = json!({
            "file_path": file_path.to_str().unwrap(),
            "old_string": "a",
            "new_string": "b",
            "replace_all": true
        });

        let result = tool.execute(input).await;
        assert!(!result.is_error, "replace_all failed: {}", result.content);

        // Verify cache was updated: mtime should match current disk mtime.
        let disk_mtime = file_mtime_ms(&file_path).unwrap();
        let mut c = cache.write().unwrap();
        let cached = c.get(&file_path).expect("file should be in cache");
        assert_eq!(cached.mtime_ms, disk_mtime);
    }

    #[tokio::test]
    async fn anchor_mode_replaces_line() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("anchor.rs");
        std::fs::write(&file_path, "alpha\nbeta\ngamma\n").unwrap();

        let cache = make_cache();
        simulate_read(&cache, &file_path);

        let hash = crate::anchors::anchor_line_hash("beta");
        let tool = EditTool::new(Some(cache));
        let result = tool
            .execute(json!({
                "file_path": file_path.to_str().unwrap(),
                "edits": [{
                    "anchor": format!("2:{hash}"),
                    "new_text": "BETA"
                }]
            }))
            .await;
        assert!(!result.is_error, "{}", result.content);
        assert!(result.content.contains("Fresh anchors"));
        let body = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(body, "alpha\nBETA\ngamma\n");
    }

    #[tokio::test]
    async fn anchor_mode_rejects_stale() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("stale.rs");
        std::fs::write(&file_path, "alpha\nbeta\ngamma\n").unwrap();

        let cache = make_cache();
        simulate_read(&cache, &file_path);

        let tool = EditTool::new(Some(cache));
        let result = tool
            .execute(json!({
                "file_path": file_path.to_str().unwrap(),
                "edits": [{
                    "anchor": "2:zzzz",
                    "new_text": "nope"
                }]
            }))
            .await;
        assert!(result.is_error);
        assert!(result.content.contains("STALE") || result.content.to_ascii_lowercase().contains("stale"));
    }
}

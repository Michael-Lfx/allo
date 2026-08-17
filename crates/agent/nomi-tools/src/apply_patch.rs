use std::path::Path;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use serde_json::{Value, json};

use nomi_protocol::events::ToolCategory;
use nomi_types::tool::{JsonSchema, ToolResult};

use crate::Tool;
use crate::edit::{EditOp, apply_edits};
use crate::file_cache::{FileStateCache, file_mtime_ms, update_cache_after_write};

/// Apply edits to SEVERAL files in one call. Every file is read and validated
/// first; writes happen only if every file's edits apply cleanly — so a
/// non-matching hunk in any file aborts the whole patch with nothing written
/// (all-or-nothing across files for the common failure mode). Cuts the N-call
/// cost of a cross-file refactor to one. Reuses the single-file apply_edits
/// engine and atomic per-file writes.
pub struct ApplyPatchTool {
    file_cache: Option<Arc<RwLock<FileStateCache>>>,
    /// Optional containment root; when set, patches outside it are rejected.
    write_root: Option<std::path::PathBuf>,
    /// Session working directory used to resolve relative `file_path` inputs
    /// (matching ReadTool / Grep / Glob / Bash). `None` = legacy process-cwd.
    cwd: Option<std::path::PathBuf>,
    /// When true, write-root rejections use CODING_BOUNDARY: copy.
    coding_boundary: bool,
}

fn err(msg: impl Into<String>) -> ToolResult {
    ToolResult {
        content: msg.into(),
        is_error: true,
        images: Vec::new(),
    }
}

impl ApplyPatchTool {
    pub fn new(file_cache: Option<Arc<RwLock<FileStateCache>>>) -> Self {
        Self {
            file_cache,
            write_root: None,
            cwd: None,
            coding_boundary: false,
        }
    }

    /// Restrict patched files to within `root` (design §3.6 write-root containment).
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

    /// Must-Read-first + staleness guard for one file (mirrors EditTool). Returns
    /// `Some(error)` if rejected, `None` if OK or no cache is wired.
    fn cache_guard(&self, path: &Path) -> Option<String> {
        let cache_arc = self.file_cache.as_ref()?;
        let mut cache = cache_arc.write().ok()?;
        let cached = cache.get(path);
        if cached.is_none() {
            return Some(nomi_coding::append_edit_recovery_hint(
                &format!("You must Read {} before patching it.", path.display()),
                nomi_coding::EditFailureKind::MustReadFirst,
            ));
        }
        let cached_mtime = cached.map(|s| s.mtime_ms);
        let disk_mtime = file_mtime_ms(path);
        if let (Some(c), Some(d)) = (cached_mtime, disk_mtime)
            && c != d
        {
            return Some(nomi_coding::append_edit_recovery_hint(
                &format!(
                    "File {} changed on disk since last read; Read it again before patching.",
                    path.display()
                ),
                nomi_coding::EditFailureKind::StaleAfterRead,
            ));
        }
        None
    }
}

#[async_trait]
impl Tool for ApplyPatchTool {
    fn name(&self) -> &str {
        "ApplyPatch"
    }

    fn description(&self) -> &str {
        "Apply edits across one or more files in a single call (all-or-nothing for the\n\
         common failure mode: if any hunk fails, nothing is written).\n\n\
         Two input styles (pick one):\n\
         1) Codex freeform — preferred for multi-hunk edits. Pass `patch` (or `input`):\n\
            *** Begin Patch\n\
            *** Update File: path/to/file.rs\n\
            @@\n\
             context\n\
            -old line\n\
            +new line\n\
            *** Add File: new.rs\n\
            +fn main() {}\n\
            *** Delete File: obsolete.rs\n\
            *** End Patch\n\
         2) JSON `files:[{file_path, edits|content|delete}]` — same as before.\n\n\
         Always Read a file before updating or deleting it. Prefer this over many Edit/Write calls."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "patch": {
                    "type": "string",
                    "description": "Codex-style freeform patch document (*** Begin Patch … *** End Patch). Prefer this for multi-hunk / multi-file edits."
                },
                "input": {
                    "type": "string",
                    "description": "Alias for `patch` (Codex freeform apply_patch input)."
                },
                "files": {
                    "type": "array",
                    "description": "Structured alternative to `patch`. Each item is {file_path, edits:[…]} or {file_path, content} or {file_path, delete:true}.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "Path to the file (absolute preferred; a relative path resolves against the session working directory)" },
                            "content": {
                                "type": "string",
                                "description": "Full file content. Use to CREATE a new file or replace an existing one whole. Mutually exclusive with `edits`."
                            },
                            "edits": {
                                "type": "array",
                                "description": "Patch an existing file. Mutually exclusive with `content`.",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "old_string": { "type": "string" },
                                        "new_string": { "type": "string" },
                                        "replace_all": { "type": "boolean" }
                                    },
                                    "required": ["old_string", "new_string"]
                                }
                            },
                            "delete": {
                                "type": "boolean",
                                "description": "Delete the file. Mutually exclusive with `content`/`edits`. The file must exist (and have been read first)."
                            }
                        },
                        "required": ["file_path"]
                    }
                }
            }
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let freeform = input
            .get("patch")
            .and_then(|v| v.as_str())
            .or_else(|| input.get("input").and_then(|v| v.as_str()))
            .filter(|s| !s.trim().is_empty());
        if let Some(patch) = freeform {
            return self.execute_freeform(patch);
        }

        let Some(files) = input["files"].as_array() else {
            return err(
                "Missing required parameter: provide Codex freeform `patch`/`input`, \
                 or structured `files` array",
            );
        };
        if files.is_empty() {
            return err("files array must not be empty");
        }

        self.execute_files_array(files)
    }

    fn describe(&self, input: &Value) -> String {
        if input.get("patch").and_then(|v| v.as_str()).is_some()
            || input.get("input").and_then(|v| v.as_str()).is_some()
        {
            return "ApplyPatch (freeform)".to_string();
        }
        let n = input
            .get("files")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        format!("ApplyPatch across {n} file(s)")
    }

    fn max_result_size(&self) -> usize {
        10_000
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Edit
    }
}

impl ApplyPatchTool {
    fn execute_freeform(&self, patch: &str) -> ToolResult {
        let parsed = match nomi_coding::parse_freeform_patch(patch) {
            Ok(p) => p,
            Err(e) => {
                return err(format!(
                    "{e}\nNext: use Codex markers (`*** Begin Patch`, `*** Update File: path`, \
                     `@@` hunks with -/+/space lines, `*** End Patch`), or pass structured `files`."
                ));
            }
        };

        let mut planned: Vec<(String, String)> = Vec::new();
        let mut to_delete: Vec<String> = Vec::new();
        let mut renames: Vec<(String, String)> = Vec::new();
        let mut total = 0usize;
        let mut created = 0usize;

        for op in parsed.files {
            match op {
                nomi_coding::PatchFileOp::Add { path, content } => {
                    let resolved =
                        crate::path_guard::resolve_against_cwd(&path, self.cwd.as_deref());
                    if let Some(msg) = crate::path_guard::ensure_within_root_ex(
                        &resolved,
                        self.write_root.as_deref(),
                        self.coding_boundary,
                    ) {
                        return err(msg);
                    }
                    let p = Path::new(&resolved);
                    if p.exists()
                        && let Some(msg) = self.cache_guard(p)
                    {
                        return err(msg);
                    }
                    if !p.exists() {
                        created += 1;
                    }
                    planned.push((resolved, content));
                }
                nomi_coding::PatchFileOp::Delete { path } => {
                    let resolved =
                        crate::path_guard::resolve_against_cwd(&path, self.cwd.as_deref());
                    if let Some(msg) = crate::path_guard::ensure_within_root_ex(
                        &resolved,
                        self.write_root.as_deref(),
                        self.coding_boundary,
                    ) {
                        return err(msg);
                    }
                    let p = Path::new(&resolved);
                    if !p.exists() {
                        return err(format!("{resolved}: cannot delete — file does not exist"));
                    }
                    if let Some(msg) = self.cache_guard(p) {
                        return err(msg);
                    }
                    to_delete.push(resolved);
                }
                nomi_coding::PatchFileOp::Update {
                    path,
                    move_to,
                    hunks,
                } => {
                    let resolved =
                        crate::path_guard::resolve_against_cwd(&path, self.cwd.as_deref());
                    if let Some(msg) = crate::path_guard::ensure_within_root_ex(
                        &resolved,
                        self.write_root.as_deref(),
                        self.coding_boundary,
                    ) {
                        return err(msg);
                    }
                    if let Some(dest) = move_to.as_ref() {
                        let dest_r =
                            crate::path_guard::resolve_against_cwd(dest, self.cwd.as_deref());
                        if let Some(msg) = crate::path_guard::ensure_within_root_ex(
                            &dest_r,
                            self.write_root.as_deref(),
                            self.coding_boundary,
                        ) {
                            return err(msg);
                        }
                        renames.push((resolved.clone(), dest_r));
                    }
                    let p = Path::new(&resolved);
                    if let Some(msg) = self.cache_guard(p) {
                        return err(msg);
                    }
                    let content = match std::fs::read_to_string(&resolved) {
                        Ok(c) => c,
                        Err(e) => return err(format!("Failed to read {resolved}: {e}")),
                    };
                    let ops: Vec<EditOp> = hunks
                        .into_iter()
                        .map(|h| EditOp {
                            old_string: h.old_text,
                            new_string: h.new_text,
                            replace_all: false,
                        })
                        .collect();
                    if ops.is_empty() && move_to.is_some() {
                        // Rename-only: keep content as-is.
                        planned.push((resolved, content));
                        continue;
                    }
                    match apply_edits(&content, &ops) {
                        Ok((new_content, n)) => {
                            total += n;
                            planned.push((resolved, new_content));
                        }
                        Err(msg) => {
                            let kind = if msg.contains("not found") {
                                nomi_coding::EditFailureKind::OldStringNotFound
                            } else if msg.contains("Multiple matches") {
                                nomi_coding::EditFailureKind::MultipleMatches
                            } else {
                                nomi_coding::EditFailureKind::Other
                            };
                            return err(nomi_coding::append_edit_recovery_hint(
                                &format!("{resolved}: {msg}"),
                                kind,
                            ));
                        }
                    }
                }
            }
        }

        let result = self.commit_plan(planned, to_delete, created, total);
        if result.is_error {
            return result;
        }
        // Apply renames after successful writes (source path content already updated).
        for (from, to) in renames {
            if from == to {
                continue;
            }
            if let Some(parent) = Path::new(&to).parent()
                && !parent.as_os_str().is_empty()
            {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::rename(&from, &to) {
                return err(format!("Patched {from} but failed to rename to {to}: {e}"));
            }
            if let Some(cache_arc) = &self.file_cache
                && let Ok(mut cache) = cache_arc.write()
            {
                if let Some(state) = cache.remove(Path::new(&from)) {
                    cache.insert(Path::new(&to).to_path_buf(), state);
                }
            }
        }
        result
    }

    fn execute_files_array(&self, files: &[Value]) -> ToolResult {
        // PHASE 1 — validate + compute the new content for every file. No writes.
        let mut planned: Vec<(String, String)> = Vec::with_capacity(files.len());
        let mut to_delete: Vec<String> = Vec::new();
        let mut total = 0usize;
        let mut created = 0usize;
        for (i, f) in files.iter().enumerate() {
            let Some(file_path) = f["file_path"].as_str() else {
                return err(format!("file #{}: missing file_path", i + 1));
            };
            // Resolve a relative file_path against the session working directory
            // (matching ReadTool/Grep/Glob/Bash) before validating/writing.
            let resolved = crate::path_guard::resolve_against_cwd(file_path, self.cwd.as_deref());
            let file_path = resolved.as_str();
            // Write-root containment (opt-in): reject any file outside the root
            // before validating/writing anything (keeps the all-or-nothing
            // guarantee — a single out-of-root file aborts the whole patch).
            if let Some(msg) = crate::path_guard::ensure_within_root_ex(
                file_path,
                self.write_root.as_deref(),
                self.coding_boundary,
            ) {
                return err(msg);
            }
            let content_field = f.get("content").and_then(|v| v.as_str());
            let edits_field = f.get("edits").and_then(|v| v.as_array());
            let delete_field = f.get("delete").and_then(|v| v.as_bool()).unwrap_or(false);

            // Delete: remove the file. Mutually exclusive with content/edits, must
            // exist, and (with a cache wired) must have been read first.
            if delete_field {
                if content_field.is_some() || edits_field.is_some() {
                    return err(format!(
                        "{file_path}: `delete` cannot be combined with `content` or `edits`"
                    ));
                }
                let path = Path::new(file_path);
                if !path.exists() {
                    return err(format!("{file_path}: cannot delete — file does not exist"));
                }
                if let Some(msg) = self.cache_guard(path) {
                    return err(msg);
                }
                to_delete.push(file_path.to_string());
                continue;
            }

            match (content_field, edits_field) {
                (Some(_), Some(_)) => {
                    return err(format!(
                        "{file_path}: specify either `content` (create/replace whole file) or `edits` (patch existing), not both"
                    ));
                }
                (None, None) => {
                    return err(format!(
                        "{file_path}: each file needs either `content` or `edits`"
                    ));
                }
                // Create or replace the whole file with `content`.
                (Some(content), None) => {
                    let path = Path::new(file_path);
                    // Overwriting an existing file requires must-read-first (mirrors
                    // edits / WriteTool). Creating a new file reads nothing, so no
                    // guard applies.
                    if path.exists()
                        && let Some(msg) = self.cache_guard(path)
                    {
                        return err(msg);
                    }
                    if !path.exists() {
                        created += 1;
                    }
                    planned.push((file_path.to_string(), content.to_string()));
                }
                // Patch an existing file with `edits`.
                (None, Some(edits_arr)) => {
                    if edits_arr.is_empty() {
                        return err(format!("{file_path}: edits array must not be empty"));
                    }
                    let mut ops = Vec::with_capacity(edits_arr.len());
                    for e in edits_arr {
                        let (Some(o), Some(n)) =
                            (e["old_string"].as_str(), e["new_string"].as_str())
                        else {
                            return err(format!(
                                "{file_path}: each edit needs old_string and new_string"
                            ));
                        };
                        ops.push(EditOp {
                            old_string: o.to_string(),
                            new_string: n.to_string(),
                            replace_all: e["replace_all"].as_bool().unwrap_or(false),
                        });
                    }
                    let path = Path::new(file_path);
                    if let Some(msg) = self.cache_guard(path) {
                        return err(msg);
                    }
                    let content = match std::fs::read_to_string(file_path) {
                        Ok(c) => c,
                        Err(e) => return err(format!("Failed to read {file_path}: {e}")),
                    };
                    match apply_edits(&content, &ops) {
                        Ok((new_content, n)) => {
                            total += n;
                            planned.push((file_path.to_string(), new_content));
                        }
                        // Abort: a hunk did not apply — leave ALL files untouched.
                        Err(msg) => {
                            let kind = if msg.contains("not found") {
                                nomi_coding::EditFailureKind::OldStringNotFound
                            } else if msg.contains("Multiple matches") {
                                nomi_coding::EditFailureKind::MultipleMatches
                            } else {
                                nomi_coding::EditFailureKind::Other
                            };
                            return err(nomi_coding::append_edit_recovery_hint(
                                &format!("{file_path}: {msg}"),
                                kind,
                            ));
                        }
                    }
                }
            }
        }

        self.commit_plan(planned, to_delete, created, total)
    }

    fn commit_plan(
        &self,
        planned: Vec<(String, String)>,
        to_delete: Vec<String>,
        created: usize,
        total: usize,
    ) -> ToolResult {
        // PHASE 2 — every file validated; commit writes atomically per file.
        for (path_str, new_content) in &planned {
            // Create the parent directory so a `content` create into a new
            // subdirectory succeeds (no-op when it already exists).
            if let Some(parent) = Path::new(path_str).parent()
                && !parent.as_os_str().is_empty()
            {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = crate::atomic_write(path_str, new_content) {
                return err(format!("Failed to write {path_str}: {e}"));
            }
            if let Some(cache_arc) = &self.file_cache {
                update_cache_after_write(cache_arc, Path::new(path_str), new_content);
            }
        }

        // PHASE 2b — deletions (after writes; independent paths, order-agnostic).
        for path_str in &to_delete {
            if let Err(e) = std::fs::remove_file(path_str) {
                return err(format!("Failed to delete {path_str}: {e}"));
            }
            if let Some(cache_arc) = &self.file_cache
                && let Ok(mut cache) = cache_arc.write()
            {
                cache.remove(Path::new(path_str));
            }
        }

        ToolResult {
            content: format!(
                "Applied patch to {} file(s) ({} created, {} deleted, {} total replacement(s))",
                planned.len() + to_delete.len(),
                created,
                to_delete.len(),
                total
            ),
            is_error: false,
            images: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[tokio::test]
    async fn apply_patch_accepts_codex_freeform() {
        let dir = tempdir().unwrap();
        let f = dir.path().join("foo.rs");
        std::fs::write(&f, "fn a() {\n    old();\n}\n").unwrap();
        let tool = ApplyPatchTool::new(None);
        let patch = format!(
            "*** Begin Patch\n*** Update File: {}\n@@\n fn a() {{\n-    old();\n+    new();\n }}\n*** End Patch\n",
            f.display()
        );
        let result = tool.execute(json!({ "patch": patch })).await;
        assert!(!result.is_error, "freeform patch should succeed: {}", result.content);
        let body = std::fs::read_to_string(&f).unwrap();
        assert!(body.contains("new()"), "got: {body}");
        assert!(!body.contains("old()"));
    }

    #[tokio::test]
    async fn apply_patch_resolves_relative_path_against_cwd() {
        // A relative file_path in a patch must resolve against the injected
        // workspace cwd, not the process cwd.
        let workspace = tempdir().unwrap();
        let rel = "__nomi_reltest_patch__.txt";
        let tool = ApplyPatchTool::new(None).with_cwd(Some(workspace.path().to_path_buf()));
        let result = tool
            .execute(json!({ "files": [{ "file_path": rel, "content": "fresh\n" }] }))
            .await;
        assert!(!result.is_error, "relative create should succeed: {}", result.content);
        assert!(
            workspace.path().join(rel).exists(),
            "relative create must land in the workspace, not the process cwd"
        );
        assert_eq!(
            std::fs::read_to_string(workspace.path().join(rel)).unwrap(),
            "fresh\n"
        );
    }

    #[tokio::test]
    async fn apply_patch_patches_multiple_files_in_one_call() {
        let dir = tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, "alpha").unwrap();
        std::fs::write(&b, "beta").unwrap();

        let tool = ApplyPatchTool::new(None);
        let result = tool
            .execute(json!({
                "files": [
                    { "file_path": a.to_str().unwrap(), "edits": [{ "old_string": "alpha", "new_string": "A" }] },
                    { "file_path": b.to_str().unwrap(), "edits": [{ "old_string": "beta", "new_string": "B" }] }
                ]
            }))
            .await;

        assert!(!result.is_error, "unexpected: {}", result.content);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "A");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "B");
    }

    #[tokio::test]
    async fn apply_patch_aborts_all_files_when_one_hunk_fails() {
        let dir = tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, "alpha").unwrap();
        std::fs::write(&b, "beta").unwrap();

        let tool = ApplyPatchTool::new(None);
        let result = tool
            .execute(json!({
                "files": [
                    { "file_path": a.to_str().unwrap(), "edits": [{ "old_string": "alpha", "new_string": "A" }] },
                    { "file_path": b.to_str().unwrap(), "edits": [{ "old_string": "NOPE", "new_string": "x" }] }
                ]
            }))
            .await;

        assert!(result.is_error, "a failing hunk must fail the whole patch");
        // Atomic across files: the first (valid) file must NOT have been written.
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "alpha");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "beta");
    }

    #[tokio::test]
    async fn apply_patch_creates_new_file_via_content() {
        let dir = tempdir().unwrap();
        let new_file = dir.path().join("sub/created.txt");
        let tool = ApplyPatchTool::new(None);
        let result = tool
            .execute(json!({
                "files": [
                    { "file_path": new_file.to_str().unwrap(), "content": "fresh contents\n" }
                ]
            }))
            .await;

        assert!(!result.is_error, "create should succeed: {}", result.content);
        // Parent dir is created as needed.
        assert_eq!(std::fs::read_to_string(&new_file).unwrap(), "fresh contents\n");
    }

    #[tokio::test]
    async fn apply_patch_content_and_edits_are_mutually_exclusive() {
        let dir = tempdir().unwrap();
        let f = dir.path().join("f.txt");
        std::fs::write(&f, "x").unwrap();
        let tool = ApplyPatchTool::new(None);
        let result = tool
            .execute(json!({
                "files": [{
                    "file_path": f.to_str().unwrap(),
                    "content": "y",
                    "edits": [{ "old_string": "x", "new_string": "z" }]
                }]
            }))
            .await;
        assert!(result.is_error, "specifying both content and edits must be rejected");
        // Nothing written.
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "x");
    }

    #[tokio::test]
    async fn apply_patch_create_is_atomic_with_a_failing_edit() {
        let dir = tempdir().unwrap();
        let existing = dir.path().join("e.txt");
        let created = dir.path().join("created.txt");
        std::fs::write(&existing, "alpha").unwrap();

        let tool = ApplyPatchTool::new(None);
        let result = tool
            .execute(json!({
                "files": [
                    { "file_path": created.to_str().unwrap(), "content": "should not survive" },
                    { "file_path": existing.to_str().unwrap(), "edits": [{ "old_string": "NOPE", "new_string": "x" }] }
                ]
            }))
            .await;

        assert!(result.is_error, "a failing edit must abort the whole patch");
        // The create must NOT have happened (validate-all before write-any).
        assert!(!created.exists(), "new file must not exist when the patch aborts");
        assert_eq!(std::fs::read_to_string(&existing).unwrap(), "alpha");
    }

    #[tokio::test]
    async fn apply_patch_deletes_file() {
        let dir = tempdir().unwrap();
        let f = dir.path().join("gone.txt");
        std::fs::write(&f, "bye").unwrap();
        let tool = ApplyPatchTool::new(None);
        let result = tool
            .execute(json!({
                "files": [{ "file_path": f.to_str().unwrap(), "delete": true }]
            }))
            .await;
        assert!(!result.is_error, "delete should succeed: {}", result.content);
        assert!(!f.exists(), "file must be removed");
    }

    #[tokio::test]
    async fn apply_patch_delete_rejects_missing_file() {
        let dir = tempdir().unwrap();
        let f = dir.path().join("nope.txt");
        let tool = ApplyPatchTool::new(None);
        let result = tool
            .execute(json!({
                "files": [{ "file_path": f.to_str().unwrap(), "delete": true }]
            }))
            .await;
        assert!(result.is_error, "deleting a non-existent file must error");
    }

    #[tokio::test]
    async fn apply_patch_delete_is_atomic_with_a_failing_edit() {
        let dir = tempdir().unwrap();
        let doomed = dir.path().join("doomed.txt");
        let other = dir.path().join("other.txt");
        std::fs::write(&doomed, "still here").unwrap();
        std::fs::write(&other, "alpha").unwrap();

        let tool = ApplyPatchTool::new(None);
        let result = tool
            .execute(json!({
                "files": [
                    { "file_path": doomed.to_str().unwrap(), "delete": true },
                    { "file_path": other.to_str().unwrap(), "edits": [{ "old_string": "NOPE", "new_string": "x" }] }
                ]
            }))
            .await;
        assert!(result.is_error, "a failing edit must abort the whole patch");
        assert!(doomed.exists(), "file must NOT be deleted when the patch aborts");
        assert_eq!(std::fs::read_to_string(&doomed).unwrap(), "still here");
    }

    #[tokio::test]
    async fn apply_patch_delete_is_mutually_exclusive_with_content() {
        let dir = tempdir().unwrap();
        let f = dir.path().join("f.txt");
        std::fs::write(&f, "x").unwrap();
        let tool = ApplyPatchTool::new(None);
        let result = tool
            .execute(json!({
                "files": [{ "file_path": f.to_str().unwrap(), "delete": true, "content": "y" }]
            }))
            .await;
        assert!(result.is_error, "delete + content must be rejected");
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "x", "nothing written/deleted");
    }
}

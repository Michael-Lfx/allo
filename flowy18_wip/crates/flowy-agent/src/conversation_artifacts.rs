//! Import files written by successful conversation tool calls.
//!
//! Only structured `write_file` / `edit_file` pairs are considered. The file
//! must still exist inside the session workspace when the import runs.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use flowy_agent_core::{
    ContentBlock, DeliveryCommit, Message, MessageSource, Role, SessionOrigin,
    SessionRecord,
};
use serde::Deserialize;

use crate::artifact_store::{
    ArtifactStore, MAX_ARTIFACT_BYTES, MAX_DELIVERY_BYTES, MAX_DELIVERY_FILES,
    PrepareArtifact,
};
use crate::delivery::DeliveryStore;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversationWrite {
    pub(crate) raw_path: String,
    pub(crate) tool_call_id: String,
    pub(crate) turn_id: String,
}

#[derive(Debug, Deserialize)]
struct PathArguments {
    path: String,
}

#[derive(Clone, Debug)]
struct ResolvedWrite {
    source_root: PathBuf,
    identity: String,
}

#[derive(Default)]
struct TurnBudget {
    identities: BTreeSet<String>,
    bytes: u64,
}

#[must_use]
pub const fn session_supports_import(session: &SessionRecord) -> bool {
    matches!(
        session.origin,
        SessionOrigin::Home | SessionOrigin::Project { .. }
    ) && session.workspace.is_some()
}

#[must_use]
pub fn turn_id_for_submission(submission: u128) -> String {
    format!("submission-{submission:032x}")
}

/// Pair successful `write_file` / `edit_file` tool calls with their persisted
/// results. Failed calls, orphaned results, and unknown tools are ignored.
#[must_use]
pub fn parse_successful_writes(messages: &[Message]) -> Vec<ConversationWrite> {
    parse_successful_writes_filtered(messages, None)
}

#[must_use]
pub fn parse_successful_writes_for_turn(
    messages: &[Message],
    turn_id: &str,
) -> Vec<ConversationWrite> {
    parse_successful_writes_filtered(messages, Some(turn_id))
}

fn parse_successful_writes_filtered(
    messages: &[Message],
    wanted_turn: Option<&str>,
) -> Vec<ConversationWrite> {
    let mut calls = HashMap::<String, (String, String, String)>::new();
    let mut current_turn = "legacy".to_owned();
    let mut turn_index = 0_usize;
    for message in messages {
        if message.role == Role::User
            && matches!(&message.source, MessageSource::User)
        {
            current_turn = if message.id.is_empty() {
                format!("turn-{turn_index}")
            } else {
                message.id.clone()
            };
            turn_index = turn_index.saturating_add(1);
        }
        for block in &message.content {
            let ContentBlock::ToolCall {
                id,
                name,
                arguments,
            } = block
            else {
                continue;
            };
            if !matches!(name.as_str(), "write_file" | "edit_file") {
                continue;
            }
            let Ok(arguments) =
                serde_json::from_str::<PathArguments>(arguments)
            else {
                continue;
            };
            if arguments.path.trim().is_empty()
                || arguments.path.chars().any(char::is_control)
            {
                continue;
            }
            calls.insert(
                id.clone(),
                (name.clone(), arguments.path, current_turn.clone()),
            );
        }
    }

    let mut seen = BTreeSet::new();
    let mut writes = Vec::new();
    for message in messages {
        let MessageSource::Tool { tool } = &message.source else {
            continue;
        };
        for block in &message.content {
            let ContentBlock::ToolResult {
                tool_call_id,
                is_error,
                ..
            } = block
            else {
                continue;
            };
            if is_error == &Some(true) {
                continue;
            }
            let Some((call_tool, raw_path, turn_id)) = calls.get(tool_call_id)
            else {
                continue;
            };
            if call_tool != tool
                || wanted_turn.is_some_and(|wanted| wanted != turn_id)
                || !seen.insert(tool_call_id.clone())
            {
                continue;
            }
            writes.push(ConversationWrite {
                raw_path: raw_path.clone(),
                tool_call_id: tool_call_id.clone(),
                turn_id: turn_id.clone(),
            });
        }
    }
    writes
}

/// Prepare one durable one-file commit per current source path. The caller
/// must persist each returned commit before committing it to the store.
#[cfg(test)]
#[must_use]
pub fn prepare_conversation_artifacts(
    store: &ArtifactStore,
    session: &SessionRecord,
    writes: &[ConversationWrite],
) -> Vec<DeliveryCommit> {
    prepare_conversation_artifacts_gated(store, None, session, writes)
}

/// Prepare conversation artifact commits while enforcing [`DeliveryStore`] so
/// intermediate scripts (`.ps1`/`.py`) and unverified `.pptx`/`.docx`/`.xlsx`
/// files cannot bypass the hard delivery gate into the Library (`最终方案.md` §6.4).
#[must_use]
pub fn prepare_conversation_artifacts_gated(
    store: &ArtifactStore,
    gate: Option<&DeliveryStore>,
    session: &SessionRecord,
    writes: &[ConversationWrite],
) -> Vec<DeliveryCommit> {
    if !session_supports_import(session) {
        return Vec::new();
    }
    let Some(workspace) = session.workspace.as_ref() else {
        return Vec::new();
    };
    let Some(root) = normalize_path(&workspace.root) else {
        return Vec::new();
    };

    // Multiple writes to one file in the same turn all see the final bytes.
    // Keep only the last successful call as the stable source identity while
    // enforcing the same per-turn file and byte budgets as publish_file.
    let mut latest = BTreeMap::<String, (ResolvedWrite, String)>::new();
    let mut budgets = BTreeMap::<String, TurnBudget>::new();
    for write in writes {
        let Some(resolved) = resolve_write(&root, &write.raw_path) else {
            continue;
        };
        let metadata = match fs::symlink_metadata(
            resolved.source_root.join(&resolved.identity),
        ) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                store.record_import_diagnostic(format!(
                    "Could not inspect conversation file {}: {error}",
                    resolved.identity
                ));
                continue;
            }
        };
        if metadata.file_type().is_symlink()
            || is_windows_reparse_point(&metadata)
        {
            store.record_import_diagnostic(format!(
                "Skipped linked or reparse-point conversation file {}",
                resolved.identity
            ));
            continue;
        }
        if !metadata.is_file() || metadata.len() == 0 {
            store.record_import_diagnostic(format!(
                "Skipped non-empty regular file requirement for conversation file {}",
                resolved.identity
            ));
            continue;
        }
        if metadata.len() > MAX_ARTIFACT_BYTES {
            store.record_import_diagnostic(format!(
                "Skipped conversation file {} above the 128 MiB limit",
                resolved.identity
            ));
            continue;
        }

        let budget = budgets.entry(write.turn_id.clone()).or_default();
        if !budget.identities.contains(&resolved.identity) {
            if budget.identities.len() >= MAX_DELIVERY_FILES {
                store.record_import_diagnostic(format!(
                    "Skipped conversation file {} because turn {} exceeds the {MAX_DELIVERY_FILES}-file limit",
                    resolved.identity, write.turn_id
                ));
                continue;
            }
            let next_bytes = budget.bytes.saturating_add(metadata.len());
            if next_bytes > MAX_DELIVERY_BYTES {
                store.record_import_diagnostic(format!(
                    "Skipped conversation file {} because turn {} exceeds the {MAX_DELIVERY_BYTES}-byte limit",
                    resolved.identity, write.turn_id
                ));
                continue;
            }
            budget.identities.insert(resolved.identity.clone());
            budget.bytes = next_bytes;
        }
        latest.insert(
            resolved.identity.clone(),
            (resolved, write.tool_call_id.clone()),
        );
    }

    let mut committed = match store.committed_ids() {
        Ok(ids) => ids,
        Err(error) => {
            store.record_import_diagnostic(format!(
                "Could not read committed artifacts before importing conversation files: {error}"
            ));
            return Vec::new();
        }
    };
    let mut commits = Vec::new();
    for (identity, (resolved, tool_call_id)) in latest {
        if let Some(gate) = gate {
            let full_path = resolved.source_root.join(&resolved.identity);
            match gate.check(&full_path) {
                Ok(None) => {}
                Ok(Some(reason)) => {
                    tracing::info!(
                        target: "flowy.delivery",
                        session_id = %session.id,
                        root_session_id = %session.id,
                        path = %full_path.display(),
                        code = reason.code(),
                        "conversation artifact import skipped by delivery gate"
                    );
                    store.record_import_diagnostic(format!(
                        "Skipped unverified conversation file {identity} ({})",
                        reason.code()
                    ));
                    continue;
                }
                Err(error) => {
                    store.record_import_diagnostic(format!(
                        "Skipped conversation file {identity} after gate read error: {error}"
                    ));
                    continue;
                }
            }
        }
        let submission_id = match submission_id() {
            Ok(id) => id,
            Err(error) => {
                store.record_import_diagnostic(format!(
                    "Could not allocate artifact submission for {identity}: {error}"
                ));
                continue;
            }
        };
        let receipt = match store.prepare(PrepareArtifact {
            source_root: resolved.source_root,
            relative_path: identity.clone(),
            source_relative_path: identity.clone(),
            conversation_import: true,
            session_id: session.id.clone(),
            submission_id,
            tool_call_id,
            delivery_key: identity.clone(),
            display_name: None,
            origin: session.origin.clone(),
        }) {
            Ok(receipt) => receipt,
            Err(error) => {
                store.record_import_diagnostic(format!(
                    "Skipped conversation file {identity}: {error}"
                ));
                continue;
            }
        };
        if committed.contains(&receipt.artifact_id) {
            continue;
        }
        committed.insert(receipt.artifact_id.clone());
        commits.push(DeliveryCommit {
            session_id: session.id.clone(),
            submission_id: receipt.submission_id.clone(),
            artifact_ids: vec![receipt.artifact_id],
            completed_successfully: true,
            committed_at_ms: now_ms(),
            origin: session.origin.clone(),
            origin_title_snapshot: session.title.clone(),
        });
    }
    commits
}

fn resolve_write(root: &Path, raw: &str) -> Option<ResolvedWrite> {
    let raw_path = Path::new(raw);
    let candidate = if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        root.join(raw_path)
    };
    let candidate = normalize_path(&candidate)?;
    let relative = relative_to(root, &candidate)?;
    let identity = slash_path(&relative)?;
    Some(ResolvedWrite {
        source_root: root.to_path_buf(),
        identity,
    })
}

fn relative_to(root: &Path, candidate: &Path) -> Option<PathBuf> {
    if let Ok(relative) = candidate.strip_prefix(root) {
        return valid_relative(relative);
    }
    let canonical_root = fs::canonicalize(root).ok()?;
    let canonical_root = normalize_path(&canonical_root)?;
    let canonical_candidate = fs::canonicalize(candidate).ok()?;
    let canonical_candidate = normalize_path(&canonical_candidate)?;
    valid_relative(canonical_candidate.strip_prefix(canonical_root).ok()?)
}

fn valid_relative(path: &Path) -> Option<PathBuf> {
    if path.as_os_str().is_empty()
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return None;
    }
    Some(path.to_path_buf())
}

fn slash_path(path: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        let Component::Normal(part) = component else {
            return None;
        };
        parts.push(part.to_string_lossy());
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn normalize_path(path: &Path) -> Option<PathBuf> {
    let path = strip_verbatim_prefix(path);
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    (!normalized.as_os_str().is_empty()).then_some(normalized)
}

fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let raw = path.as_os_str().to_string_lossy();
        for prefix in [r"\\?\UNC\", r"//?/UNC/"] {
            if let Some(rest) = raw.strip_prefix(prefix) {
                return PathBuf::from(format!(r"\\{rest}"));
            }
        }
        for prefix in [r"\\?\", r"//?/"] {
            if let Some(rest) = raw.strip_prefix(prefix) {
                return PathBuf::from(rest);
            }
        }
    }
    path.to_path_buf()
}

fn submission_id() -> io::Result<String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|error| io::Error::other(error.to_string()))?;
    Ok(format!("conversation-{}", hex(&bytes)))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

#[cfg(windows)]
fn is_windows_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
const fn is_windows_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentHandle, LlmConfig};
    use flowy_agent_core::{
        ContentBlock, Message, MessageSource, Role, SessionIndex,
        SessionWorkspace, TurnState,
    };
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "flowy-conversation-artifacts-{}-{tag}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn user_message(id: &str) -> Message {
        Message {
            id: id.into(),
            role: Role::User,
            content: vec![ContentBlock::text("write files")],
            source: MessageSource::User,
        }
    }

    fn call_message(call_id: &str, tool: &str, path: &str) -> Message {
        Message {
            id: format!("assistant-{call_id}"),
            role: Role::Assistant,
            content: vec![ContentBlock::ToolCall {
                id: call_id.into(),
                name: tool.into(),
                arguments: json!({ "path": path }).to_string(),
            }],
            source: MessageSource::Model {
                provider: "test".into(),
                model: "test".into(),
            },
        }
    }

    fn result_message(call_id: &str, tool: &str, failed: bool) -> Message {
        Message {
            id: format!("result-{call_id}"),
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_call_id: call_id.into(),
                content: vec![ContentBlock::text("ok")],
                is_error: Some(failed),
                artifact: None,
            }],
            source: MessageSource::Tool { tool: tool.into() },
        }
    }

    fn session(
        id: &str,
        root: &Path,
        origin: SessionOrigin,
        messages: Vec<Message>,
    ) -> SessionRecord {
        SessionRecord {
            id: id.into(),
            title: "Conversation".into(),
            status: TurnState::Completed,
            messages,
            origin,
            workspace: Some(SessionWorkspace {
                root: root.to_path_buf(),
            }),
            ..SessionRecord::default()
        }
    }

    fn commit_writes(store: &ArtifactStore, session: &SessionRecord) -> usize {
        let writes = parse_successful_writes(&session.messages);
        let commits = prepare_conversation_artifacts(store, session, &writes);
        for commit in &commits {
            store.commit_prepared(commit).unwrap();
        }
        commits.len()
    }

    fn project_origin() -> SessionOrigin {
        SessionOrigin::Project {
            project_id: Some("project-1".into()),
        }
    }

    #[test]
    fn md_write_is_snapshotted_and_edit_replaces_the_visible_version() {
        let scratch = Scratch::new("edit");
        let workspace = scratch.0.join("project");
        fs::create_dir_all(&workspace).unwrap();
        let file = workspace.join("notes.md");
        fs::write(&file, b"# first\n").unwrap();
        let store = ArtifactStore::open(scratch.0.join("artifacts")).unwrap();
        let mut record = session(
            "session-edit",
            &workspace,
            project_origin(),
            vec![
                call_message("write-1", "write_file", "notes.md"),
                result_message("write-1", "write_file", false),
            ],
        );
        assert_eq!(commit_writes(&store, &record), 1);
        let first = store.list().unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].manifest.source_relative_path, "notes.md");
        assert_eq!(
            store
                .read_payload(&first[0].manifest.receipt.artifact_id)
                .unwrap(),
            b"# first\n"
        );

        fs::write(&file, b"# second\n").unwrap();
        record.messages.extend([
            call_message("edit-2", "edit_file", "notes.md"),
            result_message("edit-2", "edit_file", false),
        ]);
        assert_eq!(commit_writes(&store, &record), 1);
        let latest = store.list().unwrap();
        assert_eq!(latest.len(), 1);
        assert_eq!(latest[0].manifest.source_relative_path, "notes.md");
        assert_eq!(
            store
                .read_payload(&latest[0].manifest.receipt.artifact_id)
                .unwrap(),
            b"# second\n"
        );

        // A later backfill observes the same persisted write call but new
        // bytes. It must allocate a new submission rather than collide with
        // the previous snapshot's stable source identity.
        fs::write(&file, b"# externally changed\n").unwrap();
        assert_eq!(commit_writes(&store, &record), 1);
        let latest = store.list().unwrap();
        assert_eq!(latest.len(), 1);
        assert_eq!(
            store
                .read_payload(&latest[0].manifest.receipt.artifact_id)
                .unwrap(),
            b"# externally changed\n"
        );
    }

    #[test]
    fn outside_path_failed_call_and_unknown_session_are_skipped() {
        let scratch = Scratch::new("skip");
        let workspace = scratch.0.join("project");
        fs::create_dir_all(&workspace).unwrap();
        let outside = scratch.0.join("outside.md");
        fs::write(&outside, b"outside").unwrap();
        fs::write(workspace.join("inside.md"), b"inside").unwrap();
        let store = ArtifactStore::open(scratch.0.join("artifacts")).unwrap();

        let outside_record = session(
            "session-outside",
            &workspace,
            project_origin(),
            vec![
                call_message(
                    "outside-call",
                    "write_file",
                    &outside.display().to_string(),
                ),
                result_message("outside-call", "write_file", false),
            ],
        );
        assert_eq!(commit_writes(&store, &outside_record), 0);

        let traversal_record = session(
            "session-traversal",
            &workspace,
            project_origin(),
            vec![
                call_message("traversal-call", "write_file", "../outside.md"),
                result_message("traversal-call", "write_file", false),
            ],
        );
        assert_eq!(commit_writes(&store, &traversal_record), 0);

        let failed_record = session(
            "session-failed",
            &workspace,
            project_origin(),
            vec![
                call_message("failed-call", "write_file", "inside.md"),
                result_message("failed-call", "write_file", true),
            ],
        );
        assert_eq!(commit_writes(&store, &failed_record), 0);

        let unknown_record = session(
            "session-unknown",
            &workspace,
            SessionOrigin::Unknown,
            vec![
                call_message("unknown-call", "write_file", "inside.md"),
                result_message("unknown-call", "write_file", false),
            ],
        );
        assert_eq!(commit_writes(&store, &unknown_record), 0);
        assert_eq!(store.list().unwrap().as_slice(), []);
    }

    #[test]
    fn homepage_publish_and_parsed_write_share_one_snapshot() {
        let scratch = Scratch::new("publish-dedup");
        let workspace = scratch.0.join("home");
        let output = workspace.join("outputs").join("submission-published");
        fs::create_dir_all(&output).unwrap();
        let file = output.join("notes.md");
        fs::write(&file, b"# home\n").unwrap();
        let store = ArtifactStore::open(scratch.0.join("artifacts")).unwrap();
        let receipt = store
            .prepare(PrepareArtifact {
                source_root: output,
                relative_path: "notes.md".into(),
                source_relative_path: "outputs/submission-published/notes.md"
                    .into(),
                conversation_import: false,
                session_id: "home-session".into(),
                submission_id: "submission-published".into(),
                tool_call_id: "publish-call".into(),
                delivery_key: "notes".into(),
                display_name: None,
                origin: SessionOrigin::Home,
            })
            .unwrap();
        store
            .commit_prepared(&DeliveryCommit {
                session_id: receipt.session_id.clone(),
                submission_id: receipt.submission_id.clone(),
                artifact_ids: vec![receipt.artifact_id.clone()],
                completed_successfully: true,
                committed_at_ms: 1,
                origin: SessionOrigin::Home,
                origin_title_snapshot: "Home".into(),
            })
            .unwrap();

        let record = session(
            "home-session",
            &workspace,
            SessionOrigin::Home,
            vec![
                call_message(
                    "write-home",
                    "write_file",
                    "outputs/submission-published/notes.md",
                ),
                result_message("write-home", "write_file", false),
            ],
        );
        assert_eq!(commit_writes(&store, &record), 0);
        let records = store.list().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].manifest.receipt.artifact_id,
            receipt.artifact_id
        );
    }

    #[test]
    fn startup_backfill_imports_a_project_markdown_file() {
        let scratch = Scratch::new("backfill");
        let workspace = scratch.0.join("project");
        fs::create_dir_all(&workspace).unwrap();
        let file = workspace.join("锂电池-科普介绍.md");
        fs::write(&file, b"# battery\n").unwrap();
        let workspace = fs::canonicalize(workspace).unwrap();
        let record = session(
            "session-backfill",
            &workspace,
            project_origin(),
            vec![
                call_message(
                    "backfill-write",
                    "write_file",
                    "锂电池-科普介绍.md",
                ),
                result_message("backfill-write", "write_file", false),
            ],
        );
        let mut index = SessionIndex {
            active: Some(record.id.clone()),
            ..SessionIndex::default()
        };
        index.upsert(record);
        index.save(&scratch.0).unwrap();

        let handle = AgentHandle::with_config(
            scratch.0.clone(),
            LlmConfig {
                api_key: None,
                base_url: String::new(),
                model: String::new(),
                provider: String::new(),
                ..LlmConfig::default()
            },
        );
        assert!(!handle.is_initializing());
        let records = handle.artifacts().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].manifest.source_relative_path,
            "锂电池-科普介绍.md"
        );
        assert_eq!(records[0].manifest.origin, project_origin());
        let artifact_id = records[0].manifest.receipt.artifact_id.clone();
        assert_eq!(handle.read_artifact(&artifact_id).unwrap(), b"# battery\n");
        drop(handle);

        let reopened = AgentHandle::with_config(
            scratch.0.clone(),
            LlmConfig {
                api_key: None,
                base_url: String::new(),
                model: String::new(),
                provider: String::new(),
                ..LlmConfig::default()
            },
        );
        let records = reopened.artifacts().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].manifest.receipt.artifact_id, artifact_id);
        reopened.trash_artifact(&artifact_id).unwrap();
        reopened.permanently_delete_artifact(&artifact_id).unwrap();
        assert_eq!(reopened.artifacts().unwrap().as_slice(), []);
        drop(reopened);

        let after_delete = AgentHandle::with_config(
            scratch.0.clone(),
            LlmConfig {
                api_key: None,
                base_url: String::new(),
                model: String::new(),
                provider: String::new(),
                ..LlmConfig::default()
            },
        );
        assert!(
            after_delete.artifacts().unwrap().is_empty(),
            "startup backfill resurrected a permanently deleted import"
        );
    }

    #[test]
    fn oversized_file_is_skipped_with_a_storage_diagnostic() {
        let scratch = Scratch::new("oversized");
        let workspace = scratch.0.join("project");
        fs::create_dir_all(&workspace).unwrap();
        let file = fs::File::create(workspace.join("large.md")).unwrap();
        file.set_len(MAX_ARTIFACT_BYTES + 1).unwrap();
        let store = ArtifactStore::open(scratch.0.join("artifacts")).unwrap();
        let record = session(
            "session-large",
            &workspace,
            project_origin(),
            vec![
                call_message("large-call", "write_file", "large.md"),
                result_message("large-call", "write_file", false),
            ],
        );
        assert_eq!(commit_writes(&store, &record), 0);
        assert_eq!(store.list().unwrap().as_slice(), []);
        assert!(
            store
                .diagnostics()
                .unwrap()
                .iter()
                .any(|message| message.contains("128 MiB"))
        );
    }

    #[test]
    fn parser_keeps_only_results_matching_write_and_edit_calls() {
        let messages = vec![
            call_message("write-ok", "write_file", "one.md"),
            result_message("write-ok", "write_file", false),
            call_message("edit-failed", "edit_file", "two.md"),
            result_message("edit-failed", "edit_file", true),
            call_message("read", "read_file", "three.md"),
            result_message("read", "read_file", false),
        ];
        let writes = parse_successful_writes(&messages);
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].raw_path, "one.md");
        assert_eq!(writes[0].tool_call_id, "write-ok");
    }
    #[test]
    fn deleted_exact_source_is_not_reimported() {
        let scratch = Scratch::new("deleted-tombstone");
        let workspace = scratch.0.join("project");
        fs::create_dir_all(&workspace).unwrap();
        let file = workspace.join("notes.md");
        fs::write(
            &file,
            b"# permanent
",
        )
        .unwrap();
        let store = ArtifactStore::open(scratch.0.join("artifacts")).unwrap();
        let record = session(
            "session-deleted-tombstone",
            &workspace,
            project_origin(),
            vec![
                call_message("write-deleted", "write_file", "notes.md"),
                result_message("write-deleted", "write_file", false),
            ],
        );
        assert_eq!(commit_writes(&store, &record), 1);
        let artifact_id = store.list().unwrap()[0]
            .manifest
            .receipt
            .artifact_id
            .clone();
        store.trash(&artifact_id).unwrap();
        store.permanently_delete(&artifact_id).unwrap();
        assert_eq!(store.list().unwrap().as_slice(), []);

        assert_eq!(commit_writes(&store, &record), 0);
        assert_eq!(store.list().unwrap().as_slice(), []);
    }

    #[test]
    fn per_turn_limit_applies_to_conversation_imports() {
        let scratch = Scratch::new("turn-limit");
        let workspace = scratch.0.join("project");
        fs::create_dir_all(&workspace).unwrap();
        let mut messages = Vec::new();
        for index in 0..=MAX_DELIVERY_FILES {
            let name = format!("file-{index}.txt");
            fs::write(workspace.join(&name), format!("file {index}")).unwrap();
            let call_id = format!("write-{index}");
            messages.push(call_message(&call_id, "write_file", &name));
            messages.push(result_message(&call_id, "write_file", false));
        }
        let store = ArtifactStore::open(scratch.0.join("artifacts")).unwrap();
        let record = session(
            "session-turn-limit",
            &workspace,
            project_origin(),
            messages,
        );
        assert_eq!(commit_writes(&store, &record), MAX_DELIVERY_FILES);
        assert_eq!(store.list().unwrap().len(), MAX_DELIVERY_FILES);
        assert!(
            store
                .diagnostics()
                .unwrap()
                .iter()
                .any(|message| { message.contains("file limit") })
        );
    }

    #[test]
    fn turn_filter_keeps_calls_isolated_after_a_fast_follow_up() {
        let messages = vec![
            user_message("submission-00000000000000000000000000000001"),
            call_message("first-call", "write_file", "first.md"),
            result_message("first-call", "write_file", false),
            user_message("submission-00000000000000000000000000000002"),
            call_message("second-call", "write_file", "second.md"),
            result_message("second-call", "write_file", false),
        ];
        let writes = parse_successful_writes_for_turn(
            &messages,
            "submission-00000000000000000000000000000001",
        );
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].raw_path, "first.md");
        assert_eq!(
            writes[0].turn_id,
            "submission-00000000000000000000000000000001"
        );
    }

    #[test]
    fn gated_conversation_import_blocks_unverified_scripts_and_decks_while_keeping_exempt_markdown()
     {
        let scratch = Scratch::new("gated-import");
        let workspace = scratch.0.join("home");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(workspace.join("build_helper.ps1"), b"Write-Host 'tmp'")
            .unwrap();
        fs::write(workspace.join("unverified.pptx"), b"PK\x03\x04deck")
            .unwrap();
        fs::write(workspace.join("notes.md"), b"# Notes\n").unwrap();

        let store = ArtifactStore::open(scratch.0.join("artifacts")).unwrap();
        let gate = DeliveryStore::new(scratch.0.join("delivery"));
        let record = session(
            "session-gated-import",
            &workspace,
            SessionOrigin::Home,
            vec![
                call_message("c-ps1", "write_file", "build_helper.ps1"),
                result_message("c-ps1", "write_file", false),
                call_message("c-pptx", "write_file", "unverified.pptx"),
                result_message("c-pptx", "write_file", false),
                call_message("c-md", "write_file", "notes.md"),
                result_message("c-md", "write_file", false),
            ],
        );
        let writes = parse_successful_writes(&record.messages);
        let commits = prepare_conversation_artifacts_gated(
            &store,
            Some(&gate),
            &record,
            &writes,
        );
        for commit in &commits {
            store.commit_prepared(commit).unwrap();
        }
        let imported: Vec<String> = store
            .list()
            .unwrap()
            .into_iter()
            .map(|item| item.manifest.source_relative_path)
            .collect();
        assert_eq!(imported, vec!["notes.md".to_owned()]);
    }
}

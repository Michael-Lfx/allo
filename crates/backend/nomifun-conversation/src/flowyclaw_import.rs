//! One-shot import of chats, knowledge, and memory notes from the previous desktop app.
//!
//! The previous app's directory is only opened for shared reads. The receipt
//! in the new data directory freezes the file identities of the first run; a
//! crash may finish that frozen list, and a sealed receipt cannot start another.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::UNIX_EPOCH;

use chrono::DateTime;
use nomifun_ai_agent::conversation_title_completer::clamp_title;
use nomifun_api_types::{
    CreateConversationRequest, KnowledgeDocumentImportStatus, LegacyImportPhase,
    LegacyImportPreview, LegacyImportReport,
};
use nomifun_common::{AppError, MessageId, now_ms};
use nomifun_db::models::MessageRow;
use nomifun_db::{ConversationRowUpdate, TurnLifecycleTransition};
use nomifun_knowledge::document_import::{self, MAX_IMPORTED_SOURCE_BYTES};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tracing::warn;

use crate::service::ConversationService;

const RECEIPT_VERSION: u32 = 1;
const MAX_TRANSCRIPT_BYTES: u64 = 32 * 1024 * 1024;
const MAX_CATALOG_BYTES: u64 = 8 * 1024 * 1024;
const MAX_RECEIPT_BYTES: u64 = 32 * 1024 * 1024;
const MAX_UI_TEXT_CHARS: usize = 100_000;
const MAX_UI_TOOL_CHARS: usize = 8 * 1024;
const MAX_SEED_TOOL_CHARS: usize = 2 * 1024;
const MAX_SEED_CHARS: usize = 80_000;
const MAX_SEED_MESSAGES: usize = 200;
const MAX_SESSIONS: usize = 2_000;
const MAX_DOCUMENTS: usize = 5_000;

#[derive(Clone, Copy)]
struct CopyNotes {
    image: &'static str,
    truncated: &'static str,
    unnamed_chat: &'static str,
    unnamed_base: &'static str,
    memory_base: &'static str,
    chinese: bool,
}

fn notes_for(data_dir: &Path) -> CopyNotes {
    let chinese = nomifun_common::normalize_ui_language(
        nomifun_common::read_installation_language(data_dir).as_deref(),
    ) == "zh-CN";
    if chinese {
        CopyNotes {
            image: "（历史图片未导入）",
            truncated: "…（已截断）",
            unnamed_chat: "导入的对话",
            unnamed_base: "导入的知识库",
            memory_base: "旧版记忆",
            chinese: true,
        }
    } else {
        CopyNotes {
            image: "(An image from the previous app was not imported.)",
            truncated: "…(truncated)",
            unnamed_chat: "Imported chat",
            unnamed_base: "Imported knowledge",
            memory_base: "Previous memory",
            chinese: false,
        }
    }
}

fn english_notes() -> CopyNotes {
    CopyNotes {
        image: "(An image from the previous app was not imported.)",
        truncated: "…(truncated)",
        unnamed_chat: "Imported chat",
        unnamed_base: "Imported knowledge",
        memory_base: "Previous memory",
        chinese: false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReceiptState {
    Frozen,
    Sealed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ItemStatus {
    Pending,
    Imported,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct SessionItem {
    agent_id: String,
    session_key: String,
    session_id: String,
    source_path: String,
    source_len: u64,
    source_mtime_ms: i64,
    model: String,
    model_provider: String,
    updated_at_ms: i64,
    deleted_ids: Vec<String>,
    snapshot_rel: Option<String>,
    conversation_id: Option<String>,
    message_count: u32,
    status: ItemStatus,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct FileItem {
    display_name: String,
    source_path: String,
    source_len: u64,
    source_mtime_ms: i64,
    snapshot_rel: Option<String>,
    import_name: Option<String>,
    status: ItemStatus,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct FolderItem {
    folder_id: String,
    name: String,
    knowledge_base_id: Option<String>,
    files: Vec<FileItem>,
    status: ItemStatus,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Receipt {
    version: u32,
    state: ReceiptState,
    sessions: Vec<SessionItem>,
    folders: Vec<FolderItem>,
}

#[derive(Debug, Default)]
struct Scan {
    sessions: Vec<PlannedSession>,
    folders: Vec<PlannedFolder>,
}

#[derive(Debug)]
struct PlannedSession {
    agent_id: String,
    session_key: String,
    session_id: String,
    source_path: String,
    source_len: u64,
    source_mtime_ms: i64,
    model: String,
    model_provider: String,
    updated_at_ms: i64,
    deleted_ids: Vec<String>,
    failure: Option<String>,
}

#[derive(Debug)]
struct PlannedFolder {
    folder_id: String,
    name: String,
    files: Vec<PlannedFile>,
    failure: Option<String>,
}

#[derive(Debug)]
struct PlannedFile {
    display_name: String,
    source_path: String,
    source_len: u64,
    source_mtime_ms: i64,
    failure: Option<String>,
    skipped: bool,
}

fn import_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn openclaw_root() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let root = home.join(".openclaw");
    root.is_dir().then_some(root)
}

fn receipt_path(data_dir: &Path) -> PathBuf {
    data_dir.join("imports").join("flowyclaw-receipt.json")
}

fn import_root(data_dir: &Path) -> PathBuf {
    data_dir.join("imports").join("flowyclaw")
}

impl ConversationService {
    pub async fn legacy_import_preview(&self) -> Result<LegacyImportPreview, AppError> {
        let data_dir = self.import_data_dir()?;
        let source_found = openclaw_root().is_some();
        if let Some(receipt) = load_receipt(&data_dir)? {
            return Ok(preview_from_receipt(&receipt, source_found));
        }
        let scan = scan_openclaw().await;
        Ok(preview_from_scan(source_found, &scan))
    }

    pub async fn legacy_import_execute(
        &self,
        user_id: &str,
    ) -> Result<LegacyImportReport, AppError> {
        let _guard = import_lock()
            .try_lock()
            .map_err(|_| AppError::Conflict("legacy_import_busy".into()))?;
        let data_dir = self.import_data_dir()?;
        if let Some(receipt) = load_receipt(&data_dir)? {
            if receipt.state == ReceiptState::Sealed {
                return Err(AppError::Conflict("legacy_import_completed".into()));
            }
            return self.finish_receipt(user_id, &data_dir, receipt).await;
        }
        let scan = scan_openclaw().await;
        if scan.sessions.is_empty() && scan.folders.is_empty() {
            return Ok(LegacyImportReport {
                phase: LegacyImportPhase::Absent,
                conversations_imported: 0,
                messages_imported: 0,
                knowledge_bases_imported: 0,
                documents_imported: 0,
                failed_count: 0,
            });
        }
        let receipt = receipt_from_scan(scan);
        store_receipt(&data_dir, &receipt)?;
        self.finish_receipt(user_id, &data_dir, receipt).await
    }

    fn import_data_dir(&self) -> Result<PathBuf, AppError> {
        self.knowledge_service_handle()
            .map(|service| service.data_dir().to_path_buf())
            .ok_or_else(|| AppError::Internal("knowledge service is not available for import".into()))
    }

    async fn finish_receipt(
        &self,
        user_id: &str,
        data_dir: &Path,
        mut receipt: Receipt,
    ) -> Result<LegacyImportReport, AppError> {
        let notes = notes_for(data_dir);
        self.import_sessions(user_id, data_dir, &mut receipt, &notes).await?;
        self.import_folders(data_dir, &mut receipt, &notes).await?;
        // Ordinary chats only search knowledge bases on the default workpath
        // binding. Attaching the imported bases there is what makes the copied
        // documents and memory notes readable by later conversations.
        self.publish_imported_bases(&receipt).await?;
        if has_pending(&receipt) {
            store_receipt(data_dir, &receipt)?;
            return Err(AppError::Internal(
                "legacy import stopped before every item finished".into(),
            ));
        }
        receipt.state = ReceiptState::Sealed;
        store_receipt(data_dir, &receipt)?;
        Ok(report_from(&receipt))
    }

    async fn import_sessions(
        &self,
        user_id: &str,
        data_dir: &Path,
        receipt: &mut Receipt,
        notes: &CopyNotes,
    ) -> Result<(), AppError> {
        let indexes = work_indexes(receipt.sessions.iter().map(|item| item.status));
        for index in indexes {
            self.import_session_at(user_id, data_dir, receipt, index, notes).await?;
            store_receipt(data_dir, receipt)?;
        }
        Ok(())
    }

    async fn import_session_at(
        &self,
        user_id: &str,
        data_dir: &Path,
        receipt: &mut Receipt,
        index: usize,
        notes: &CopyNotes,
    ) -> Result<(), AppError> {
        let item = &receipt.sessions[index];
        let loaded = load_or_snapshot(
            data_dir,
            &item.source_path,
            item.source_len,
            item.source_mtime_ms,
            &item.snapshot_rel,
            &format!("snapshots/{index}.jsonl"),
            MAX_TRANSCRIPT_BYTES,
        );
        let bytes = match loaded {
            SnapshotLoad::Ready { bytes, rel } => {
                receipt.sessions[index].snapshot_rel = Some(rel);
                store_receipt(data_dir, receipt)?;
                bytes
            }
            SnapshotLoad::Item(reason) => {
                receipt.sessions[index].status = ItemStatus::Failed;
                receipt.sessions[index].error = Some(reason);
                return Ok(());
            }
            SnapshotLoad::Abort(error) => return Err(error),
        };
        let deleted = receipt.sessions[index].deleted_ids.clone();
        let projection = project_transcript(&bytes, &deleted, notes);
        if !projection.has_user_text {
            receipt.sessions[index].status = ItemStatus::Failed;
            receipt.sessions[index].error = Some("no_messages".into());
            return Ok(());
        }
        let title = {
            let clamped = clamp_title(&projection.raw_title);
            if clamped.is_empty() { notes.unnamed_chat.to_owned() } else { clamped }
        };
        let item = &receipt.sessions[index];
        let request = import_request(&title, item)?;
        let response = match self.create_idempotent(user_id, request, &creation_key(item)).await {
            Ok(response) => response,
            Err(AppError::BadRequest(message)) => {
                receipt.sessions[index].status = ItemStatus::Failed;
                receipt.sessions[index].error = Some(message);
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        let conversation_id = response.conversation_id.clone();
        if !is_safe_conversation_id(&conversation_id) {
            return Err(AppError::Internal("imported conversation id cannot own a seed".into()));
        }
        receipt.sessions[index].conversation_id = Some(conversation_id.clone());
        store_receipt(data_dir, receipt)?;
        write_seed(data_dir, &conversation_id, &projection.seed)?;
        // Stamp the imported title as user-owned before messages exist, so an
        // open chat cannot claim an automatic rename while this copy is still
        // inserting history.
        let extra = legacy_extra(&receipt.sessions[index], response.extra);
        if !conversation_can_continue(&extra) {
            return Err(AppError::Internal(
                "imported conversation has no workspace and cannot be continued".into(),
            ));
        }
        let model_json = if response.model.is_none() {
            self.saved_default_model_json().await
        } else {
            None
        };
        self.store_imported_conversation(
            user_id,
            &conversation_id,
            &title,
            &extra,
            now_ms(),
            model_json.as_deref(),
        )
        .await?;
        self.conversation_repo()
            .delete_messages_by_conversation(&conversation_id)
            .await?;
        let mut cursor = 0i64;
        let mut count = 0u32;
        let base_time = if receipt.sessions[index].updated_at_ms > 0 {
            receipt.sessions[index].updated_at_ms
        } else {
            now_ms()
        };
        for message in &projection.messages {
            let mut created_at = if message.created_at > 0 { message.created_at } else { base_time };
            if created_at <= cursor {
                created_at = cursor + 1;
            }
            cursor = created_at;
            self.conversation_repo()
                .insert_message(&message_row(&conversation_id, message, created_at))
                .await?;
            count += 1;
        }
        let updated_at = if cursor > 0 { cursor } else { base_time };
        self.store_imported_conversation(
            user_id,
            &conversation_id,
            &title,
            &extra,
            updated_at,
            model_json.as_deref(),
        )
        .await?;
        receipt.sessions[index].message_count = count;
        receipt.sessions[index].status = ItemStatus::Imported;
        receipt.sessions[index].error = None;
        Ok(())
    }

    async fn import_folders(
        &self,
        data_dir: &Path,
        receipt: &mut Receipt,
        notes: &CopyNotes,
    ) -> Result<(), AppError> {
        let indexes = work_indexes(receipt.folders.iter().map(|item| item.status));
        for index in indexes {
            self.import_folder_at(data_dir, receipt, index, notes).await?;
            store_receipt(data_dir, receipt)?;
        }
        Ok(())
    }

    async fn import_folder_at(
        &self,
        data_dir: &Path,
        receipt: &mut Receipt,
        index: usize,
        notes: &CopyNotes,
    ) -> Result<(), AppError> {
        if receipt.folders[index].folder_id.is_empty() {
            receipt.folders[index].status = ItemStatus::Failed;
            if receipt.folders[index].error.is_none() {
                receipt.folders[index].error = Some("unreadable".into());
            }
            return Ok(());
        }
        let knowledge = self.import_data_dir().and_then(|_| {
            self.knowledge_service_handle()
                .ok_or_else(|| AppError::Internal("knowledge service is not available for import".into()))
        })?;
        if receipt.folders[index].knowledge_base_id.is_none() {
            let name = imported_base_name(&receipt.folders[index], notes);
            let folder_id = receipt.folders[index].folder_id.clone();
            match create_imported_base(&knowledge, &name, &folder_id).await {
                Ok(id) => {
                    receipt.folders[index].knowledge_base_id = Some(id);
                    store_receipt(data_dir, receipt)?;
                }
                Err(AppError::BadRequest(message)) => {
                    receipt.folders[index].status = ItemStatus::Failed;
                    receipt.folders[index].error = Some(message);
                    return Ok(());
                }
                Err(error) => return Err(error),
            }
        }
        let base_id = receipt.folders[index]
            .knowledge_base_id
            .clone()
            .ok_or_else(|| AppError::Internal("imported knowledge base is missing its id".into()))?;
        let file_indexes = work_indexes(receipt.folders[index].files.iter().map(|item| item.status));
        for file_index in file_indexes {
            self.import_file_at(data_dir, receipt, index, file_index, &knowledge, &base_id)
                .await?;
            store_receipt(data_dir, receipt)?;
        }
        receipt.folders[index].status = ItemStatus::Imported;
        receipt.folders[index].error = None;
        Ok(())
    }

    async fn import_file_at(
        &self,
        data_dir: &Path,
        receipt: &mut Receipt,
        folder_index: usize,
        file_index: usize,
        knowledge: &nomifun_knowledge::KnowledgeService,
        base_id: &str,
    ) -> Result<(), AppError> {
        let item = &receipt.folders[folder_index].files[file_index];
        if item.status == ItemStatus::Skipped {
            return Ok(());
        }
        let loaded = load_or_snapshot(
            data_dir,
            &item.source_path,
            item.source_len,
            item.source_mtime_ms,
            &item.snapshot_rel,
            &format!("kb-snapshots/{folder_index}/{file_index}.bin"),
            MAX_IMPORTED_SOURCE_BYTES,
        );
        let bytes = match loaded {
            SnapshotLoad::Ready { bytes, rel } => {
                receipt.folders[folder_index].files[file_index].snapshot_rel = Some(rel);
                store_receipt(data_dir, receipt)?;
                bytes
            }
            SnapshotLoad::Item(reason) => {
                let status = if reason == "missing" || reason == "unsupported" {
                    ItemStatus::Skipped
                } else {
                    ItemStatus::Failed
                };
                receipt.folders[folder_index].files[file_index].status = status;
                receipt.folders[folder_index].files[file_index].error = Some(reason);
                return Ok(());
            }
            SnapshotLoad::Abort(error) => return Err(error),
        };
        let display_name = receipt.folders[folder_index].files[file_index].display_name.clone();
        let source_path = receipt.folders[folder_index].files[file_index].source_path.clone();
        let mut used = HashSet::new();
        for (other_index, other) in receipt.folders[folder_index].files.iter().enumerate() {
            if other_index == file_index {
                continue;
            }
            if let Some(name) = &other.import_name {
                used.insert(document_import::target_markdown_path(name, ""));
            }
        }
        let prepared = if let Some(name) = receipt.folders[folder_index].files[file_index]
            .import_name
            .clone()
        {
            PreparedDoc::Import { name, bytes }
        } else {
            prepare_document(&display_name, Path::new(&source_path), &bytes, &mut used)
        };
        let PreparedDoc::Import { name, bytes } = prepared else {
            receipt.folders[folder_index].files[file_index].status = ItemStatus::Skipped;
            receipt.folders[folder_index].files[file_index].error = Some("unsupported".into());
            return Ok(());
        };
        receipt.folders[folder_index].files[file_index].import_name = Some(name.clone());
        match knowledge.import_document(base_id, &name, "", bytes).await {
            Ok(result) => {
                if matches!(
                    result.status,
                    KnowledgeDocumentImportStatus::Written | KnowledgeDocumentImportStatus::Conflict
                ) {
                    receipt.folders[folder_index].files[file_index].status = ItemStatus::Imported;
                    receipt.folders[folder_index].files[file_index].error = None;
                } else {
                    receipt.folders[folder_index].files[file_index].status = ItemStatus::Failed;
                    receipt.folders[folder_index].files[file_index].error =
                        Some(import_status_error(result.status).to_owned());
                }
                Ok(())
            }
            Err(AppError::BadRequest(_)) => {
                receipt.folders[folder_index].files[file_index].status = ItemStatus::Failed;
                receipt.folders[folder_index].files[file_index].error = Some("could not store document".into());
                Ok(())
            }
            Err(error) => Err(error),
        }
    }
}

enum PreparedDoc {
    Import { name: String, bytes: Vec<u8> },
    Skip,
}

fn prepare_document(
    display_name: &str,
    source: &Path,
    bytes: &[u8],
    used: &mut HashSet<String>,
) -> PreparedDoc {
    let mut name = document_name(display_name, source);
    if !document_import::supports_source_path(&name) {
        let extension = extension_of(&name).unwrap_or_default();
        if matches!(extension.as_str(), "txt" | "text" | "log") && std::str::from_utf8(bytes).is_ok() {
            name = with_md_extension(&name);
        } else {
            return PreparedDoc::Skip;
        }
    }
    PreparedDoc::Import { name: allocate_name(&name, used), bytes: bytes.to_vec() }
}

fn document_name(display_name: &str, source: &Path) -> String {
    let source_name = source.file_name().and_then(|value| value.to_str()).unwrap_or("document");
    let display = safe_file_name(display_name);
    let display_ext = extension_of(&display);
    let source_ext = extension_of(source_name);
    if display_ext.is_some() && display_ext == source_ext && !display.is_empty() {
        return display;
    }
    if !display.is_empty()
        && let Some(extension) = source_ext
    {
        let stem = Path::new(&display)
            .file_stem()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("document");
        return format!("{stem}.{extension}");
    }
    safe_file_name(source_name)
}

fn allocate_name(preferred: &str, used: &mut HashSet<String>) -> String {
    let mut name = safe_file_name(preferred);
    if name.is_empty() {
        name = "document".to_owned();
    }
    let mut suffix = 2u32;
    loop {
        let target = document_import::target_markdown_path(&name, "");
        if used.insert(target) {
            return name;
        }
        let stem = Path::new(&name)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("document");
        let extension = extension_of(&name).unwrap_or_else(|| "md".to_owned());
        name = format!("{stem}-{suffix}.{extension}");
        suffix += 1;
        if suffix > 1_000 {
            return format!("document-{suffix}.md");
        }
    }
}

fn safe_file_name(name: &str) -> String {
    let base = Path::new(name).file_name().and_then(|value| value.to_str()).unwrap_or(name);
    let cleaned: String = base
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            character if character.is_control() => '_',
            character => character,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() { "document".to_owned() } else { trimmed.to_owned() }
}

fn with_md_extension(name: &str) -> String {
    let stem = Path::new(name)
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("document");
    format!("{stem}.md")
}

fn extension_of(name: &str) -> Option<String> {
    Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
}

fn import_status_error(status: KnowledgeDocumentImportStatus) -> &'static str {
    match status {
        KnowledgeDocumentImportStatus::Written | KnowledgeDocumentImportStatus::Conflict => "stored",
        KnowledgeDocumentImportStatus::Unsupported => "unsupported",
        KnowledgeDocumentImportStatus::Malformed => "unreadable document",
        KnowledgeDocumentImportStatus::Encrypted => "encrypted document",
        KnowledgeDocumentImportStatus::ResourceLimit => "document is too large",
        KnowledgeDocumentImportStatus::MissingPart => "document is incomplete",
        KnowledgeDocumentImportStatus::InvalidUtf8 => "document is not valid text",
    }
}

async fn create_imported_base(
    knowledge: &nomifun_knowledge::KnowledgeService,
    desired: &str,
    folder_id: &str,
) -> Result<String, AppError> {
    let base = clamp_base_name(desired, folder_id);
    for suffix in 0..20 {
        let candidate = if suffix == 0 { base.clone() } else { format!("{base} ({})", suffix + 1) };
        if knowledge.find_base_id_by_name(&candidate).await?.is_some() {
            continue;
        }
        match knowledge.create_base(&candidate, "", None, None).await {
            Ok(info) => return Ok(info.knowledge_base_id.as_str().to_owned()),
            Err(AppError::Conflict(_)) | Err(AppError::BadRequest(_)) => continue,
            Err(error) => return Err(error),
        }
    }
    Err(AppError::BadRequest("could not choose a knowledge base name".into()))
}

fn imported_base_name(folder: &FolderItem, notes: &CopyNotes) -> String {
    if let Some(agent) = folder.folder_id.strip_prefix("memory:") {
        if agent.is_empty() || agent == "main" {
            notes.memory_base.to_owned()
        } else {
            format!("{} · {agent}", notes.memory_base)
        }
    } else if folder.name.trim().is_empty() {
        notes.unnamed_base.to_owned()
    } else {
        folder.name.clone()
    }
}

fn clamp_base_name(name: &str, folder_id: &str) -> String {
    let trimmed = name.trim();
    let source = if trimmed.is_empty() { "Imported knowledge" } else { trimmed };
    let short: String = source.chars().take(80).collect();
    let short = short.trim();
    if short.is_empty() {
        format!("Imported knowledge {folder_id}")
    } else {
        short.to_owned()
    }
}

fn import_request(title: &str, item: &SessionItem) -> Result<CreateConversationRequest, AppError> {
    serde_json::from_value(json!({
        "type": "nomi",
        "name": title,
        "extra": {
            "legacy": {
                "source": "flowyclaw",
                "session_key": item.session_key,
                "session_id": item.session_id,
                "agent_id": item.agent_id,
                "file_size": item.source_len,
                "file_mtime_ms": item.source_mtime_ms,
                "model": item.model,
                "model_provider": item.model_provider,
            }
        }
    }))
    .map_err(|error| AppError::Internal(format!("could not build the imported conversation: {error}")))
}

fn legacy_extra(item: &SessionItem, existing: Value) -> Value {
    let mut extra = if existing.is_object() { existing } else { json!({}) };
    if let Some(object) = extra.as_object_mut() {
        // This flag is computed on every read. Persisting the response copy
        // would freeze a stale value into the row.
        object.remove("is_temporary_workspace");
    }
    extra["titleSource"] = json!("user");
    extra["legacy"] = json!({
        "source": "flowyclaw",
        "session_key": item.session_key,
        "session_id": item.session_id,
        "agent_id": item.agent_id,
        "file_size": item.source_len,
        "file_mtime_ms": item.source_mtime_ms,
        "model": item.model,
        "model_provider": item.model_provider,
    });
    extra
}

fn conversation_can_continue(extra: &Value) -> bool {
    extra
        .get("temp_workspace_id")
        .and_then(Value::as_str)
        .is_some_and(|token| !token.is_empty())
}

fn creation_key(item: &SessionItem) -> String {
    let identity = if item.session_id.is_empty() { &item.session_key } else { &item.session_id };
    let safe: String = identity
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
        .take(80)
        .collect();
    let agent: String = item
        .agent_id
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-' || *character == '_')
        .take(40)
        .collect();
    format!("flowyclaw:{agent}:{safe}")
}

fn message_row(conversation_id: &str, message: &DisplayMessage, created_at: i64) -> MessageRow {
    let id = MessageId::new().as_str().to_owned();
    let (kind, position, status, content) = match message.kind {
        DisplayKind::User => ("text", "right", "finish", json!({ "content": message.text })),
        DisplayKind::Assistant => (
            "text",
            "left",
            if message.is_error { "error" } else { "finish" },
            json!({ "content": message.text }),
        ),
        DisplayKind::Thinking => (
            "thinking",
            "left",
            "finish",
            json!({ "content": message.text, "status": "done" }),
        ),
        DisplayKind::Tool => (
            "tool_call",
            "left",
            "finish",
            json!({
                "call_id": message.call_id,
                "name": message.tool_name,
                "args": tool_args_object(&message.tool_args),
                "output": message.text,
                "status": if message.is_error { "error" } else { "completed" },
            }),
        ),
    };
    MessageRow {
        id: 0,
        message_id: id.clone(),
        conversation_id: conversation_id.to_owned(),
        msg_id: Some(id),
        r#type: kind.to_owned(),
        content: content.to_string(),
        position: Some(position.to_owned()),
        status: Some(status.to_owned()),
        hidden: false,
        created_at,
    }
}

fn tool_args_object(args: &Value) -> Value {
    if args.is_object() {
        args.clone()
    } else if args.is_null() {
        json!({})
    } else {
        json!({ "input": args })
    }
}

/// Attach imported bases to the default workpath.
///
/// A binding the user has never configured (disabled and empty) is turned on
/// as read-only, so later chats can search the copied documents. A binding the
/// user already turned off keeps that switch and only gains the new ids, so
/// turning knowledge back on includes them. Existing ids and write-back stay.
fn merge_default_workpath_binding(
    current: &nomifun_knowledge::KnowledgeBinding,
    imported_ids: &[String],
) -> Result<nomifun_knowledge::KnowledgeBinding, AppError> {
    let mut ids: Vec<String> = current.kb_ids.iter().map(|id| id.as_str().to_owned()).collect();
    for id in imported_ids {
        if !id.is_empty() && !ids.iter().any(|existing| existing == id) {
            ids.push(id.clone());
        }
    }
    let kb_ids = ids
        .iter()
        .map(|id| {
            nomifun_common::KnowledgeBaseId::parse(id).map_err(|error| {
                AppError::Internal(format!("imported knowledge base id is invalid: {error}"))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let never_configured = !current.enabled && current.kb_ids.is_empty();
    let eagerness = if current.writeback_eagerness.is_empty() {
        "manual".to_owned()
    } else {
        current.writeback_eagerness.clone()
    };
    Ok(nomifun_knowledge::KnowledgeBinding {
        enabled: current.enabled || never_configured,
        writeback: current.writeback,
        writeback_eagerness: eagerness,
        channel_write_enabled: current.channel_write_enabled,
        kb_ids,
    })
}

fn binding_unchanged(
    current: &nomifun_knowledge::KnowledgeBinding,
    next: &nomifun_knowledge::KnowledgeBinding,
) -> bool {
    current.enabled == next.enabled
        && current.writeback == next.writeback
        && current.writeback_eagerness == next.writeback_eagerness
        && current.channel_write_enabled == next.channel_write_enabled
        && current
            .kb_ids
            .iter()
            .map(|id| id.as_str())
            .eq(next.kb_ids.iter().map(|id| id.as_str()))
}

impl ConversationService {
    async fn saved_default_model_json(&self) -> Option<String> {
        match self.default_voice_model_preference().await {
            Ok(Some(model)) => serde_json::to_string(&model).ok(),
            Ok(None) => None,
            Err(error) => {
                warn!(%error, "legacy import left the conversation model unset");
                None
            }
        }
    }

    async fn publish_imported_bases(&self, receipt: &Receipt) -> Result<(), AppError> {
        let imported: Vec<String> = receipt
            .folders
            .iter()
            .filter(|folder| folder.status == ItemStatus::Imported)
            .filter_map(|folder| folder.knowledge_base_id.clone())
            .filter(|id| !id.is_empty())
            .collect();
        if imported.is_empty() {
            return Ok(());
        }
        let knowledge = self.knowledge_service_handle().ok_or_else(|| {
            AppError::Internal("knowledge service is not available for import".into())
        })?;
        let current = knowledge
            .get_binding(
                nomifun_knowledge::WORKPATH_BINDING_KIND,
                nomifun_knowledge::DEFAULT_WORKPATH_KEY,
            )
            .await?;
        let next = merge_default_workpath_binding(&current, &imported)?;
        if binding_unchanged(&current, &next) {
            return Ok(());
        }
        knowledge
            .set_binding(
                nomifun_knowledge::WORKPATH_BINDING_KIND,
                nomifun_knowledge::DEFAULT_WORKPATH_KEY,
                next,
            )
            .await?;
        Ok(())
    }

    async fn store_imported_conversation(
        &self,
        user_id: &str,
        conversation_id: &str,
        title: &str,
        extra: &Value,
        updated_at: i64,
        model_json: Option<&str>,
    ) -> Result<(), AppError> {
        let extra = serde_json::to_string(extra).map_err(|error| {
            AppError::Internal(format!("could not store imported conversation details: {error}"))
        })?;
        // Seal before rewriting the title. A chat that has already started a
        // turn must keep both its lifecycle and its current metadata.
        match self
            .conversation_repo()
            .seal_imported_history(user_id, conversation_id, updated_at)
            .await?
        {
            TurnLifecycleTransition::Committed | TurnLifecycleTransition::AlreadyApplied => {}
            TurnLifecycleTransition::Stale => {
                return Err(AppError::Conflict(
                    "imported conversation must stay idle and pending until its history is sealed"
                        .into(),
                ));
            }
        }
        self.conversation_repo()
            .update(
                conversation_id,
                &ConversationRowUpdate {
                    name: Some(title.to_owned()),
                    model: model_json.map(|value| Some(value.to_owned())),
                    extra: Some(extra),
                    updated_at: Some(updated_at),
                    ..Default::default()
                },
            )
            .await?;
        Ok(())
    }
}

fn write_seed(data_dir: &Path, conversation_id: &str, messages: &[Value]) -> Result<(), AppError> {
    let Some(path) = seed_path(data_dir, conversation_id) else {
        return Err(AppError::Internal("imported conversation id cannot own a seed".into()));
    };
    let bytes = serde_json::to_vec(&json!({ "version": 1, "messages": messages })).map_err(|error| {
        AppError::Internal(format!("could not encode the imported engine seed: {error}"))
    })?;
    replace_file(&path, &bytes).map_err(|error| {
        warn!(error = %error, "legacy import seed write failed");
        AppError::Internal("could not store the imported engine seed".into())
    })
}

fn seed_path(data_dir: &Path, conversation_id: &str) -> Option<PathBuf> {
    is_safe_conversation_id(conversation_id).then(|| {
        import_root(data_dir)
            .join("seeds")
            .join(format!("{conversation_id}.json"))
    })
}

fn is_safe_conversation_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    (32..=36).contains(&bytes.len()) && bytes.iter().all(|byte| byte.is_ascii_hexdigit() || *byte == b'-')
}

fn work_indexes(statuses: impl Iterator<Item = ItemStatus>) -> Vec<usize> {
    statuses
        .enumerate()
        .filter(|(_, status)| matches!(status, ItemStatus::Pending | ItemStatus::Failed))
        .map(|(index, _)| index)
        .collect()
}

fn has_pending(receipt: &Receipt) -> bool {
    receipt.sessions.iter().any(|item| item.status == ItemStatus::Pending)
        || receipt.folders.iter().any(|folder| {
            folder.status == ItemStatus::Pending
                || folder.files.iter().any(|file| file.status == ItemStatus::Pending)
        })
}

fn preview_from_receipt(receipt: &Receipt, source_found: bool) -> LegacyImportPreview {
    LegacyImportPreview {
        phase: match receipt.state {
            ReceiptState::Frozen => LegacyImportPhase::Frozen,
            ReceiptState::Sealed => LegacyImportPhase::Sealed,
        },
        source_found,
        session_count: u32_len(receipt.sessions.len()),
        knowledge_base_count: u32_len(receipt.folders.len()),
        document_count: u32_len(receipt.folders.iter().map(|folder| folder.files.len()).sum()),
        pending_count: pending_count(receipt),
        failed_count: failed_count(receipt),
    }
}

fn preview_from_scan(source_found: bool, scan: &Scan) -> LegacyImportPreview {
    LegacyImportPreview {
        phase: LegacyImportPhase::Absent,
        source_found,
        session_count: u32_len(scan.sessions.len()),
        knowledge_base_count: u32_len(scan.folders.len()),
        document_count: u32_len(scan.folders.iter().map(|folder| folder.files.len()).sum()),
        pending_count: 0,
        failed_count: 0,
    }
}

fn report_from(receipt: &Receipt) -> LegacyImportReport {
    LegacyImportReport {
        phase: LegacyImportPhase::Sealed,
        conversations_imported: count_status(receipt.sessions.iter().map(|item| item.status), ItemStatus::Imported),
        messages_imported: receipt.sessions.iter().map(|item| item.message_count).sum(),
        knowledge_bases_imported: count_status(receipt.folders.iter().map(|item| item.status), ItemStatus::Imported),
        documents_imported: count_status(
            receipt.folders.iter().flat_map(|folder| folder.files.iter().map(|file| file.status)),
            ItemStatus::Imported,
        ),
        failed_count: failed_count(receipt),
    }
}

fn pending_count(receipt: &Receipt) -> u32 {
    let sessions = receipt.sessions.iter().filter(|item| item.status == ItemStatus::Pending).count();
    let folders = receipt.folders.iter().filter(|item| item.status == ItemStatus::Pending).count();
    let files = receipt
        .folders
        .iter()
        .flat_map(|folder| &folder.files)
        .filter(|item| item.status == ItemStatus::Pending)
        .count();
    u32_len(sessions + folders + files)
}

fn failed_count(receipt: &Receipt) -> u32 {
    let sessions = receipt.sessions.iter().filter(|item| item.status == ItemStatus::Failed).count();
    let folders = receipt.folders.iter().filter(|item| item.status == ItemStatus::Failed).count();
    let files = receipt
        .folders
        .iter()
        .flat_map(|folder| &folder.files)
        .filter(|item| matches!(item.status, ItemStatus::Failed | ItemStatus::Skipped))
        .count();
    u32_len(sessions + folders + files)
}

fn count_status(statuses: impl Iterator<Item = ItemStatus>, wanted: ItemStatus) -> u32 {
    u32_len(statuses.filter(|status| *status == wanted).count())
}

fn u32_len(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn receipt_from_scan(scan: Scan) -> Receipt {
    Receipt {
        version: RECEIPT_VERSION,
        state: ReceiptState::Frozen,
        sessions: scan.sessions.into_iter().map(session_item).collect(),
        folders: scan.folders.into_iter().map(folder_item).collect(),
    }
}

fn session_item(planned: PlannedSession) -> SessionItem {
    let (status, error) = match planned.failure {
        Some(reason) => (ItemStatus::Failed, Some(reason)),
        None => (ItemStatus::Pending, None),
    };
    SessionItem {
        agent_id: planned.agent_id,
        session_key: planned.session_key,
        session_id: planned.session_id,
        source_path: planned.source_path,
        source_len: planned.source_len,
        source_mtime_ms: planned.source_mtime_ms,
        model: planned.model,
        model_provider: planned.model_provider,
        updated_at_ms: planned.updated_at_ms,
        deleted_ids: planned.deleted_ids,
        snapshot_rel: None,
        conversation_id: None,
        message_count: 0,
        status,
        error,
    }
}

fn folder_item(planned: PlannedFolder) -> FolderItem {
    let (status, error) = match planned.failure {
        Some(reason) => (ItemStatus::Failed, Some(reason)),
        None => (ItemStatus::Pending, None),
    };
    FolderItem {
        folder_id: planned.folder_id,
        name: planned.name,
        knowledge_base_id: None,
        files: planned.files.into_iter().map(file_item).collect(),
        status,
        error,
    }
}

fn file_item(planned: PlannedFile) -> FileItem {
    let (status, error) = if planned.skipped {
        (ItemStatus::Skipped, planned.failure.or_else(|| Some("unsupported".into())))
    } else if let Some(reason) = planned.failure {
        (ItemStatus::Failed, Some(reason))
    } else {
        (ItemStatus::Pending, None)
    };
    FileItem {
        display_name: planned.display_name,
        source_path: planned.source_path,
        source_len: planned.source_len,
        source_mtime_ms: planned.source_mtime_ms,
        snapshot_rel: None,
        import_name: None,
        status,
        error,
    }
}

async fn scan_openclaw() -> Scan {
    let Some(root) = openclaw_root() else {
        return Scan::default();
    };
    match tokio::task::spawn_blocking(move || scan_source(&root)).await {
        Ok(scan) => scan,
        Err(error) => {
            warn!(error = %error, "legacy import scan failed");
            Scan::default()
        }
    }
}

fn scan_source(root: &Path) -> Scan {
    let mut scan = Scan::default();
    let agents = root.join("agents");
    if let Ok(entries) = fs::read_dir(&agents) {
        for entry in entries.flatten() {
            if scan.sessions.len() >= MAX_SESSIONS {
                break;
            }
            let Some(agent_id) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if agent_id.starts_with('.') || agent_id.contains(['/', '\\']) {
                continue;
            }
            scan_agent(&mut scan, &agent_id, &entry.path().join("sessions"));
        }
    }
    scan_knowledge(&mut scan, root);
    scan_memory(&mut scan, root);
    scan
}

const MEMORY_CORE_FILES: [&str; 6] = [
    "MEMORY.md",
    "USER.md",
    "IDENTITY.md",
    "SOUL.md",
    "AGENTS.md",
    "TOOLS.md",
];

fn scan_memory(scan: &mut Scan, root: &Path) {
    for (agent_id, workspace) in memory_workspaces(root) {
        if scan.folders.len() >= 500 {
            break;
        }
        let mut files = Vec::new();
        collect_memory_files(scan, &workspace, &mut files);
        if files.is_empty() {
            continue;
        }
        scan.folders.push(PlannedFolder {
            folder_id: unique_memory_id(scan, &agent_id),
            name: String::new(),
            files,
            failure: None,
        });
    }
}

fn collect_memory_files(scan: &Scan, workspace: &Path, files: &mut Vec<PlannedFile>) {
    for name in MEMORY_CORE_FILES {
        if files.len() + document_total(scan) >= MAX_DOCUMENTS {
            return;
        }
        if let Some(file) = plan_existing_file(name, workspace, &workspace.join(name)) {
            files.push(file);
        }
    }
    let memory_dir = workspace.join("memory");
    let Ok(entries) = fs::read_dir(&memory_dir) else {
        return;
    };
    let mut names = Vec::new();
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        names.push((name, entry.path()));
    }
    names.sort_by(|left, right| left.0.cmp(&right.0));
    for (name, path) in names {
        if files.len() + document_total(scan) >= MAX_DOCUMENTS {
            return;
        }
        if name.starts_with('.') || !name.to_ascii_lowercase().ends_with(".md") {
            continue;
        }
        if let Some(file) = plan_existing_file(&format!("memory-{name}"), &memory_dir, &path) {
            files.push(file);
        }
    }
}

fn plan_existing_file(display: &str, jail: &Path, candidate: &Path) -> Option<PlannedFile> {
    let path = jailed_file(jail, candidate)?;
    let meta = fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() == 0 {
        return None;
    }
    let failure = (meta.len() > MAX_IMPORTED_SOURCE_BYTES).then(|| "too_large".to_owned());
    Some(PlannedFile {
        display_name: display.to_owned(),
        source_path: path.display().to_string(),
        source_len: meta.len(),
        source_mtime_ms: mtime_ms(&meta),
        failure,
        skipped: false,
    })
}

fn memory_workspaces(root: &Path) -> Vec<(String, PathBuf)> {
    let config = read_agent_workspaces(root);
    let default_dir = config
        .as_ref()
        .and_then(|item| item.default_workspace.as_deref())
        .and_then(|raw| resolve_workspace(root, raw))
        .or_else(|| resolve_workspace(root, "workspace"));
    let agents = config.map(|item| item.agents).unwrap_or_default();
    let mut listed = Vec::new();
    if agents.is_empty() {
        if let Some(dir) = default_dir {
            listed.push(("main".to_owned(), dir));
        }
    } else {
        for agent in agents {
            let dir = agent
                .workspace
                .as_deref()
                .and_then(|raw| resolve_workspace(root, raw))
                .or_else(|| default_dir.clone());
            let Some(dir) = dir else {
                continue;
            };
            listed.push((agent.id.unwrap_or_else(|| "main".to_owned()), dir));
        }
    }
    let mut seen = HashSet::new();
    listed
        .into_iter()
        .filter(|(_, dir)| seen.insert(dir.clone()))
        .collect()
}

struct AgentWorkspaceConfig {
    default_workspace: Option<String>,
    agents: Vec<AgentWorkspaceEntry>,
}

struct AgentWorkspaceEntry {
    id: Option<String>,
    workspace: Option<String>,
}

#[derive(Deserialize)]
struct WorkspaceConfigFile {
    agents: Option<WorkspaceAgentsFile>,
}

#[derive(Deserialize)]
struct WorkspaceAgentsFile {
    defaults: Option<WorkspaceDefaultsFile>,
    list: Option<Vec<WorkspaceAgentFile>>,
}

#[derive(Deserialize)]
struct WorkspaceDefaultsFile {
    workspace: Option<String>,
}

#[derive(Deserialize)]
struct WorkspaceAgentFile {
    id: Option<String>,
    workspace: Option<String>,
}

fn read_agent_workspaces(root: &Path) -> Option<AgentWorkspaceConfig> {
    let bytes = read_shared(&root.join("openclaw.json"), MAX_CATALOG_BYTES).ok()?;
    let value: WorkspaceConfigFile = serde_json::from_slice(&bytes).ok()?;
    let agents = value.agents?;
    Some(AgentWorkspaceConfig {
        default_workspace: agents.defaults.and_then(|item| item.workspace),
        agents: agents
            .list
            .unwrap_or_default()
            .into_iter()
            .map(|agent| AgentWorkspaceEntry {
                id: agent.id,
                workspace: agent.workspace,
            })
            .collect(),
    })
}

fn resolve_workspace(root: &Path, raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let expanded = if trimmed == "~" {
        dirs::home_dir()?
    } else if let Some(rest) = trimmed.strip_prefix("~/").or_else(|| trimmed.strip_prefix("~\\")) {
        dirs::home_dir()?.join(rest)
    } else {
        let path = PathBuf::from(trimmed);
        if path.is_absolute() { path } else { root.join(path) }
    };
    let canon = fs::canonicalize(&expanded).ok()?;
    canon.is_dir().then_some(canon)
}

fn unique_memory_id(scan: &Scan, agent_id: &str) -> String {
    let safe = sanitize_id(agent_id);
    let mut folder_id = format!("memory:{safe}");
    let mut suffix = 2u32;
    while scan.folders.iter().any(|folder| folder.folder_id == folder_id) {
        folder_id = format!("memory:{safe}-{suffix}");
        suffix += 1;
        if suffix > 50 {
            break;
        }
    }
    folder_id
}

fn sanitize_id(agent_id: &str) -> String {
    let safe: String = agent_id
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-' || *character == '_')
        .take(40)
        .collect();
    if safe.is_empty() { "main".to_owned() } else { safe }
}

fn scan_agent(scan: &mut Scan, agent_id: &str, sessions_dir: &Path) {
    let catalog = sessions_dir.join("sessions.json");
    if !catalog.exists() {
        return;
    }
    let bytes = match read_shared(&catalog, MAX_CATALOG_BYTES) {
        Ok(bytes) => bytes,
        Err(_) => {
            push_failed_session(scan, agent_id, "unreadable");
            return;
        }
    };
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        push_failed_session(scan, agent_id, "unreadable");
        return;
    };
    let Some(entries) = value.as_object().or_else(|| value.get("sessions").and_then(Value::as_object)) else {
        push_failed_session(scan, agent_id, "unreadable");
        return;
    };
    let signatures = load_signatures(sessions_dir);
    let notes = english_notes();
    for (key, entry) in entries {
        if scan.sessions.len() >= MAX_SESSIONS {
            break;
        }
        if is_cron_session(key, entry) {
            continue;
        }
        let Some(entry) = entry.as_object() else {
            continue;
        };
        let Some(session_file) = string_field(entry, "sessionFile", "session_file") else {
            continue;
        };
        let raw = PathBuf::from(&session_file);
        let candidate = if raw.is_absolute() { raw } else { sessions_dir.join(raw) };
        let Some(path) = jailed_file(sessions_dir, &candidate) else {
            continue;
        };
        if ignored_transcript(&path) {
            continue;
        }
        let meta = match fs::metadata(&path) {
            Ok(meta) if meta.is_file() => meta,
            _ => {
                push_failed_session(scan, agent_id, "unreadable");
                continue;
            }
        };
        if meta.len() > MAX_TRANSCRIPT_BYTES {
            scan.sessions.push(planned_session(agent_id, key, entry, &path, &meta, Vec::new(), Some("too_large".into())));
            continue;
        }
        let bytes = match read_shared(&path, MAX_TRANSCRIPT_BYTES) {
            Ok(bytes) => bytes,
            Err(reason) => {
                scan.sessions.push(planned_session(
                    agent_id,
                    key,
                    entry,
                    &path,
                    &meta,
                    Vec::new(),
                    Some(if reason == "too large" { "too_large".into() } else { "unreadable".into() }),
                ));
                continue;
            }
        };
        let deleted = signatures.get(key).cloned().unwrap_or_default();
        let projection = project_transcript(&bytes, &deleted, &notes);
        if !projection.has_user_text {
            continue;
        }
        scan.sessions.push(planned_session(agent_id, key, entry, &path, &meta, deleted, None));
    }
}

fn planned_session(
    agent_id: &str,
    key: &str,
    entry: &serde_json::Map<String, Value>,
    path: &Path,
    meta: &fs::Metadata,
    deleted_ids: Vec<String>,
    failure: Option<String>,
) -> PlannedSession {
    PlannedSession {
        agent_id: agent_id.to_owned(),
        session_key: key.to_owned(),
        session_id: string_field(entry, "sessionId", "session_id").unwrap_or_default(),
        source_path: path.display().to_string(),
        source_len: meta.len(),
        source_mtime_ms: mtime_ms(meta),
        model: string_field(entry, "model", "model").unwrap_or_default(),
        model_provider: string_field(entry, "modelProvider", "model_provider").unwrap_or_default(),
        updated_at_ms: json_i64(entry, &["updatedAt", "updated_at", "lastInteractionAt"]),
        deleted_ids,
        failure,
    }
}

fn push_failed_session(scan: &mut Scan, agent_id: &str, reason: &str) {
    scan.sessions.push(PlannedSession {
        agent_id: agent_id.to_owned(),
        session_key: String::new(),
        session_id: String::new(),
        source_path: String::new(),
        source_len: 0,
        source_mtime_ms: 0,
        model: String::new(),
        model_provider: String::new(),
        updated_at_ms: 0,
        deleted_ids: Vec::new(),
        failure: Some(reason.to_owned()),
    });
}

fn ignored_transcript(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return true;
    };
    let name = name.to_ascii_lowercase();
    name == "sessions.json"
        || name.ends_with(".deleted.jsonl")
        || name.contains(".jsonl.reset.")
        || name.ends_with(".trajectory.jsonl")
        || name.ends_with(".trajectory-path.json")
}

fn is_cron_session(key: &str, entry: &Value) -> bool {
    let mut parts = key.split(':');
    let cron_key = parts.next() == Some("agent") && parts.next().is_some() && parts.next() == Some("cron");
    let chat_type = entry
        .get("chatType")
        .or_else(|| entry.get("chat_type"))
        .and_then(Value::as_str)
        .unwrap_or("");
    cron_key || chat_type.eq_ignore_ascii_case("cron")
}

fn load_signatures(sessions_dir: &Path) -> HashMap<String, Vec<String>> {
    let path = sessions_dir.join(".deleted-message-signatures.json");
    let Ok(bytes) = read_shared(&path, MAX_CATALOG_BYTES) else {
        return HashMap::new();
    };
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return HashMap::new();
    };
    let Some(object) = value.as_object() else {
        return HashMap::new();
    };
    object
        .iter()
        .map(|(key, value)| {
            let signatures = value
                .as_array()
                .map(|items| items.iter().filter_map(Value::as_str).map(str::to_owned).collect())
                .unwrap_or_default();
            (key.clone(), signatures)
        })
        .collect()
}

fn scan_knowledge(scan: &mut Scan, root: &Path) {
    let catalog = root.join("knowledge").join("meta.json");
    if !catalog.exists() {
        return;
    }
    let bytes = match read_shared(&catalog, MAX_CATALOG_BYTES) {
        Ok(bytes) => bytes,
        Err(_) => {
            scan.folders.push(failed_folder("unreadable"));
            return;
        }
    };
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        scan.folders.push(failed_folder("unreadable"));
        return;
    };
    let folders = list_field(&value, "folders");
    let files = list_field(&value, "files");
    if folders.is_empty() && !files.is_empty() {
        scan.folders.push(failed_folder("unreadable"));
        return;
    }
    for folder in folders {
        if scan.folders.len() >= 500 {
            break;
        }
        let Some(folder_id) = string_field_value(&folder, "id", "id") else {
            continue;
        };
        if folder_id.is_empty() {
            continue;
        }
        let name = string_field_value(&folder, "name", "name").unwrap_or_default();
        let mut planned_files = Vec::new();
        for file in &files {
            if planned_files.len() + document_total(scan) >= MAX_DOCUMENTS {
                break;
            }
            if string_field_value(file, "folderId", "folder_id").as_deref() != Some(folder_id.as_str()) {
                continue;
            }
            planned_files.push(plan_file(file));
        }
        scan.folders.push(PlannedFolder {
            folder_id,
            name,
            files: planned_files,
            failure: None,
        });
    }
}

fn document_total(scan: &Scan) -> usize {
    scan.folders.iter().map(|folder| folder.files.len()).sum()
}

fn failed_folder(reason: &str) -> PlannedFolder {
    PlannedFolder {
        folder_id: String::new(),
        name: String::new(),
        files: Vec::new(),
        failure: Some(reason.to_owned()),
    }
}

fn plan_file(file: &Value) -> PlannedFile {
    let display_name = string_field_value(file, "originalName", "original_name")
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "document".to_owned());
    let status = string_field_value(file, "status", "status").unwrap_or_default();
    let Some(path) = pick_knowledge_path(file, &status) else {
        let skipped = status == "converting" || status == "failed";
        return PlannedFile {
            display_name,
            source_path: String::new(),
            source_len: 0,
            source_mtime_ms: 0,
            failure: Some(if skipped { "not_ready".into() } else { "missing".into() }),
            skipped,
        };
    };
    let Ok(meta) = fs::metadata(&path) else {
        return PlannedFile {
            display_name,
            source_path: String::new(),
            source_len: 0,
            source_mtime_ms: 0,
            failure: Some("unreadable".into()),
            skipped: false,
        };
    };
    if meta.len() > MAX_IMPORTED_SOURCE_BYTES {
        return PlannedFile {
            display_name,
            source_path: path.display().to_string(),
            source_len: meta.len(),
            source_mtime_ms: mtime_ms(&meta),
            failure: Some("too_large".into()),
            skipped: false,
        };
    }
    PlannedFile {
        display_name,
        source_path: path.display().to_string(),
        source_len: meta.len(),
        source_mtime_ms: mtime_ms(&meta),
        failure: None,
        skipped: false,
    }
}

fn pick_knowledge_path(file: &Value, status: &str) -> Option<PathBuf> {
    let original = string_field_value(file, "originalPath", "original_path").and_then(|value| regular_file(Path::new(&value)));
    if status == "converting" || status == "failed" {
        return original;
    }
    if original.is_some() {
        return original;
    }
    string_field_value(file, "mdPath", "md_path").and_then(|value| regular_file(Path::new(&value)))
}

fn list_field(value: &Value, key: &str) -> Vec<Value> {
    match value.get(key) {
        Some(Value::Array(items)) => items.clone(),
        Some(Value::Object(map)) => map.values().cloned().collect(),
        _ => Vec::new(),
    }
}

fn string_field(entry: &serde_json::Map<String, Value>, camel: &str, snake: &str) -> Option<String> {
    entry
        .get(camel)
        .or_else(|| entry.get(snake))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn string_field_value(value: &Value, camel: &str, snake: &str) -> Option<String> {
    value
        .get(camel)
        .or_else(|| value.get(snake))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn json_i64(entry: &serde_json::Map<String, Value>, keys: &[&str]) -> i64 {
    for key in keys {
        let Some(value) = entry.get(*key) else { continue };
        if let Some(number) = value.as_i64() {
            return number;
        }
        if let Some(number) = value.as_u64() {
            return i64::try_from(number).unwrap_or(i64::MAX);
        }
        if let Some(number) = value.as_f64() {
            return number as i64;
        }
    }
    0
}

enum SnapshotLoad {
    Ready { bytes: Vec<u8>, rel: String },
    Item(String),
    Abort(AppError),
}

fn load_or_snapshot(
    data_dir: &Path,
    source_path: &str,
    source_len: u64,
    source_mtime_ms: i64,
    snapshot_rel: &Option<String>,
    new_rel: &str,
    max_bytes: u64,
) -> SnapshotLoad {
    if let Some(rel) = snapshot_rel {
        return match snapshot_file(data_dir, rel).and_then(|path| read_shared(&path, max_bytes)) {
            Ok(bytes) => SnapshotLoad::Ready { bytes, rel: rel.clone() },
            Err(_) => SnapshotLoad::Item("unreadable".into()),
        };
    }
    if source_path.is_empty() {
        return SnapshotLoad::Item("missing".into());
    }
    let path = PathBuf::from(source_path);
    if !metadata_matches(&path, source_len, source_mtime_ms) {
        return SnapshotLoad::Item("changed".into());
    }
    let bytes = match read_shared(&path, max_bytes) {
        Ok(bytes) => bytes,
        Err(reason) => {
            return SnapshotLoad::Item(if reason == "too large" { "too_large".into() } else { "unreadable".into() });
        }
    };
    let dest = match snapshot_dest(data_dir, new_rel) {
        Ok(dest) => dest,
        Err(_) => return SnapshotLoad::Abort(AppError::Internal("could not store imported bytes".into())),
    };
    if let Err(error) = replace_file(&dest, &bytes) {
        warn!(error = %error, "legacy import snapshot write failed");
        return SnapshotLoad::Abort(AppError::Internal("could not store imported bytes".into()));
    }
    SnapshotLoad::Ready { bytes, rel: new_rel.to_owned() }
}

fn metadata_matches(path: &Path, len: u64, mtime: i64) -> bool {
    let Ok(meta) = fs::metadata(path) else { return false };
    meta.is_file() && meta.len() == len && mtime_ms(&meta) == mtime
}

fn snapshot_dest(data_dir: &Path, rel: &str) -> Result<PathBuf, ()> {
    if !safe_rel(rel) {
        return Err(());
    }
    Ok(import_root(data_dir).join(rel))
}

fn snapshot_file(data_dir: &Path, rel: &str) -> Result<PathBuf, String> {
    if !safe_rel(rel) {
        return Err("invalid snapshot".into());
    }
    let root = fs::canonicalize(import_root(data_dir)).map_err(|error| error.to_string())?;
    let file = fs::canonicalize(import_root(data_dir).join(rel)).map_err(|error| error.to_string())?;
    if file.starts_with(&root) && file.is_file() {
        Ok(file)
    } else {
        Err("invalid snapshot".into())
    }
}

fn safe_rel(rel: &str) -> bool {
    !rel.is_empty() && !rel.contains("..") && Path::new(rel).is_relative()
}

fn load_receipt(data_dir: &Path) -> Result<Option<Receipt>, AppError> {
    let path = receipt_path(data_dir);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = read_shared(&path, MAX_RECEIPT_BYTES).map_err(|_| {
        AppError::Internal("could not read the import receipt".into())
    })?;
    let receipt: Receipt = serde_json::from_slice(&bytes)
        .map_err(|_| AppError::Internal("the import receipt is unreadable".into()))?;
    if receipt.version != RECEIPT_VERSION {
        return Err(AppError::Internal("the import receipt is unreadable".into()));
    }
    Ok(Some(receipt))
}

fn store_receipt(data_dir: &Path, receipt: &Receipt) -> Result<(), AppError> {
    let bytes = serde_json::to_vec_pretty(receipt).map_err(|error| {
        AppError::Internal(format!("could not encode the import receipt: {error}"))
    })?;
    replace_file(&receipt_path(data_dir), &bytes).map_err(|error| {
        warn!(error = %error, "import receipt write failed");
        AppError::Internal("could not store the import receipt".into())
    })
}

fn read_shared(path: &Path, max_bytes: u64) -> Result<Vec<u8>, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let meta = file.metadata().map_err(|error| error.to_string())?;
    if !meta.is_file() {
        return Err("not a file".into());
    }
    if meta.len() > max_bytes {
        return Err("too large".into());
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(max_bytes)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 != meta.len() {
        return Err("changed while reading".into());
    }
    Ok(bytes)
}

fn replace_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("partial");
    fs::write(&temporary, bytes)?;
    let result = replace_existing(&temporary, path);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(windows)]
fn replace_existing(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let wide = |path: &Path| {
        path.as_os_str().encode_wide().chain(std::iter::once(0)).collect::<Vec<u16>>()
    };
    let source = wide(source);
    let target = wide(target);
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_existing(source: &Path, target: &Path) -> std::io::Result<()> {
    fs::rename(source, target)
}

fn jailed_file(root: &Path, candidate: &Path) -> Option<PathBuf> {
    let root = fs::canonicalize(root).ok()?;
    let file = fs::canonicalize(candidate).ok()?;
    (file.starts_with(&root) && file.is_file()).then_some(file)
}

fn regular_file(path: &Path) -> Option<PathBuf> {
    let file = fs::canonicalize(path).ok()?;
    file.is_file().then_some(file)
}

fn mtime_ms(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[derive(Clone)]
struct Node {
    id: String,
    parent_id: Option<String>,
    role: String,
    created_at: i64,
    is_error: bool,
    blocks: Vec<Block>,
}

#[derive(Clone)]
enum Block {
    Text(String),
    Thinking(String),
    Image,
    ToolUse { id: String, name: String, input: Value },
    ToolResult { id: String, output: String, is_error: bool },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DisplayKind {
    User,
    Assistant,
    Thinking,
    Tool,
}

struct DisplayMessage {
    kind: DisplayKind,
    text: String,
    tool_name: String,
    tool_args: Value,
    call_id: String,
    created_at: i64,
    is_error: bool,
}

struct Projection {
    raw_title: String,
    messages: Vec<DisplayMessage>,
    seed: Vec<Value>,
    has_user_text: bool,
}

fn project_transcript(bytes: &[u8], deleted: &[String], notes: &CopyNotes) -> Projection {
    let nodes = parse_nodes(bytes);
    let branch = select_branch(nodes);
    let mut events = Vec::new();
    let mut tools = Vec::new();
    for node in branch {
        if is_deleted(&node.id, deleted) {
            continue;
        }
        match node.role.as_str() {
            "user" => render_user(&node, notes, &mut events, &mut tools),
            "assistant" => render_assistant(&node, notes, &mut events, &mut tools),
            "tool" | "toolresult" | "tool_result" => render_tool_carrier(&node, &mut tools),
            _ => {}
        }
    }
    let messages = into_messages(events, &tools, notes);
    let raw_title = messages
        .iter()
        .find(|message| message.kind == DisplayKind::User)
        .map(|message| collapse(&message.text))
        .unwrap_or_default();
    let has_user_text = messages
        .iter()
        .any(|message| message.kind == DisplayKind::User && !message.text.trim().is_empty());
    let seed = engine_seed(&messages, notes);
    Projection { raw_title, messages, seed, has_user_text }
}

fn parse_nodes(bytes: &[u8]) -> Vec<Node> {
    let text = String::from_utf8_lossy(bytes);
    let mut nodes = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else { continue };
        if value.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        let Some(message) = value.get("message") else { continue };
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if role.is_empty() {
            continue;
        }
        let id = value
            .get("id")
            .or_else(|| message.get("id"))
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("line-{index}"));
        let parent_id = value
            .get("parentId")
            .or_else(|| value.get("parent_id"))
            .or_else(|| message.get("parentId"))
            .or_else(|| message.get("parent_id"))
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_owned);
        nodes.push(Node {
            id,
            parent_id,
            role,
            created_at: timestamp_ms(&value, message),
            is_error: stop_is_error(&value, message),
            blocks: parse_content(message.get("content").unwrap_or(&Value::Null)),
        });
    }
    nodes
}

fn select_branch(nodes: Vec<Node>) -> Vec<Node> {
    if nodes.len() <= 1 {
        return nodes;
    }
    let ids: HashSet<&str> = nodes.iter().map(|node| node.id.as_str()).collect();
    let mut referenced = HashSet::new();
    for node in &nodes {
        if let Some(parent) = node.parent_id.as_deref()
            && ids.contains(parent)
        {
            referenced.insert(parent.to_owned());
        }
    }
    let mut index = nodes
        .iter()
        .enumerate()
        .rev()
        .find(|(_, node)| !referenced.contains(node.id.as_str()))
        .map(|(index, _)| index)
        .unwrap_or(nodes.len() - 1);
    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    let mut broke = false;
    loop {
        if !seen.insert(index) {
            break;
        }
        chain.push(index);
        match nodes[index].parent_id.as_deref() {
            None => break,
            Some(parent) => match nodes.iter().position(|node| node.id == parent) {
                Some(parent_index) => index = parent_index,
                None => {
                    broke = true;
                    break;
                }
            },
        }
    }
    if broke {
        return nodes;
    }
    chain.reverse();
    chain.into_iter().map(|index| nodes[index].clone()).collect()
}

fn is_deleted(id: &str, signatures: &[String]) -> bool {
    signatures.iter().any(|signature| signature == &format!("id:{id}") || signature == id)
}

fn render_user(node: &Node, notes: &CopyNotes, events: &mut Vec<Event>, tools: &mut [ToolSlot]) {
    let mut text = String::new();
    let mut saw_image = false;
    for block in &node.blocks {
        match block {
            Block::Text(value) => push_line(&mut text, value),
            Block::Image => saw_image = true,
            Block::ToolResult { id, output, is_error } => fill_tool(tools, id, output, *is_error),
            Block::Thinking(_) | Block::ToolUse { .. } => {}
        }
    }
    if saw_image {
        push_line(&mut text, notes.image);
    }
    push_text_event(events, EventKind::User, &text, node.created_at, false, notes);
}

fn render_assistant(node: &Node, notes: &CopyNotes, events: &mut Vec<Event>, tools: &mut Vec<ToolSlot>) {
    let mut text = String::new();
    let mut saw_image = false;
    for block in &node.blocks {
        match block {
            Block::Text(value) => push_line(&mut text, value),
            Block::Thinking(value) => {
                push_text_event(events, EventKind::Assistant, &text, node.created_at, node.is_error, notes);
                text.clear();
                push_text_event(events, EventKind::Thinking, value, node.created_at, false, notes);
            }
            Block::Image => saw_image = true,
            Block::ToolUse { id, name, input } => {
                push_text_event(events, EventKind::Assistant, &text, node.created_at, node.is_error, notes);
                text.clear();
                let index = tools.len();
                tools.push(ToolSlot {
                    call_id: if id.is_empty() { format!("legacy-{index}") } else { id.clone() },
                    name: if name.is_empty() { "tool".to_owned() } else { name.clone() },
                    args: input.clone(),
                    output: String::new(),
                    is_error: false,
                    created_at: node.created_at,
                });
                events.push(Event::Tool { index });
            }
            Block::ToolResult { id, output, is_error } => fill_tool(tools, id, output, *is_error),
        }
    }
    if saw_image {
        push_line(&mut text, notes.image);
    }
    push_text_event(events, EventKind::Assistant, &text, node.created_at, node.is_error, notes);
}

fn render_tool_carrier(node: &Node, tools: &mut [ToolSlot]) {
    let mut saw_result = false;
    for block in &node.blocks {
        if let Block::ToolResult { id, output, is_error } = block {
            fill_tool(tools, id, output, *is_error);
            saw_result = true;
        }
    }
    if saw_result {
        return;
    }
    let text = node
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Text(value) => Some(value.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    fill_tool(tools, "", &text, node.is_error);
}

fn push_line(text: &mut String, value: &str) {
    if value.is_empty() {
        return;
    }
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(value);
}

fn push_text_event(
    events: &mut Vec<Event>,
    kind: EventKind,
    text: &str,
    created_at: i64,
    is_error: bool,
    notes: &CopyNotes,
) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    let text = truncate_chars(trimmed, MAX_UI_TEXT_CHARS, notes.truncated);
    events.push(match kind {
        EventKind::User => Event::User { text, created_at },
        EventKind::Assistant => Event::Assistant { text, created_at, is_error },
        EventKind::Thinking => Event::Thinking { text, created_at },
    });
}

#[derive(Clone, Copy)]
enum EventKind {
    User,
    Assistant,
    Thinking,
}

enum Event {
    User { text: String, created_at: i64 },
    Assistant { text: String, created_at: i64, is_error: bool },
    Thinking { text: String, created_at: i64 },
    Tool { index: usize },
}

struct ToolSlot {
    call_id: String,
    name: String,
    args: Value,
    output: String,
    is_error: bool,
    created_at: i64,
}

fn fill_tool(tools: &mut [ToolSlot], id: &str, output: &str, is_error: bool) {
    let slot = if !id.is_empty() {
        tools.iter_mut().rev().find(|tool| tool.call_id == id)
    } else {
        tools.iter_mut().rev().find(|tool| tool.output.is_empty())
    };
    if let Some(slot) = slot {
        push_line(&mut slot.output, output);
        slot.is_error |= is_error;
    }
}

fn into_messages(events: Vec<Event>, tools: &[ToolSlot], notes: &CopyNotes) -> Vec<DisplayMessage> {
    events
        .into_iter()
        .filter_map(|event| match event {
            Event::User { text, created_at } => Some(text_message(DisplayKind::User, text, created_at, false)),
            Event::Assistant { text, created_at, is_error } => {
                Some(text_message(DisplayKind::Assistant, text, created_at, is_error))
            }
            Event::Thinking { text, created_at } => Some(text_message(DisplayKind::Thinking, text, created_at, false)),
            Event::Tool { index } => {
                let slot = tools.get(index)?;
                Some(DisplayMessage {
                    kind: DisplayKind::Tool,
                    text: truncate_chars(&slot.output, MAX_UI_TOOL_CHARS, notes.truncated),
                    tool_name: slot.name.clone(),
                    tool_args: slot.args.clone(),
                    call_id: slot.call_id.clone(),
                    created_at: slot.created_at,
                    is_error: slot.is_error,
                })
            }
        })
        .collect()
}

fn text_message(kind: DisplayKind, text: String, created_at: i64, is_error: bool) -> DisplayMessage {
    DisplayMessage {
        kind,
        text,
        tool_name: String::new(),
        tool_args: Value::Null,
        call_id: String::new(),
        created_at,
        is_error,
    }
}

fn engine_seed(messages: &[DisplayMessage], notes: &CopyNotes) -> Vec<Value> {
    let mut seed = Vec::new();
    for message in messages {
        match message.kind {
            DisplayKind::User => push_seed(&mut seed, "user", &message.text),
            DisplayKind::Assistant => push_seed(&mut seed, "assistant", &message.text),
            DisplayKind::Thinking => {}
            DisplayKind::Tool => append_assistant(&mut seed, &tool_note(notes, &message.tool_name, &message.text)),
        }
    }
    cap_seed(seed)
}

fn tool_note(notes: &CopyNotes, name: &str, output: &str) -> String {
    let output = truncate_chars(output, MAX_SEED_TOOL_CHARS, notes.truncated);
    if notes.chinese {
        format!("（历史工具调用：{name}）\n{output}")
    } else {
        format!("(Earlier tool call: {name})\n{output}")
    }
}

fn push_seed(seed: &mut Vec<Value>, role: &str, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    seed.push(json!({ "role": role, "content": [{ "type": "text", "text": text }] }));
}

fn append_assistant(seed: &mut Vec<Value>, text: &str) {
    if let Some(last) = seed.last_mut()
        && last.get("role").and_then(Value::as_str) == Some("assistant")
        && let Some(existing) = last
            .get_mut("content")
            .and_then(|content| content.get_mut(0))
            .and_then(|block| block.get_mut("text"))
        && let Some(current) = existing.as_str()
    {
        *existing = Value::String(format!("{current}\n{text}"));
        return;
    }
    push_seed(seed, "assistant", text);
}

fn cap_seed(messages: Vec<Value>) -> Vec<Value> {
    let mut kept = Vec::new();
    let mut chars = 0usize;
    for message in messages.into_iter().rev() {
        let len = message_text_len(&message);
        if !kept.is_empty() && (kept.len() >= MAX_SEED_MESSAGES || chars.saturating_add(len) > MAX_SEED_CHARS) {
            break;
        }
        chars = chars.saturating_add(len);
        kept.push(message);
    }
    kept.reverse();
    kept
}

fn message_text_len(message: &Value) -> usize {
    message
        .get("content")
        .and_then(Value::as_array)
        .and_then(|blocks| blocks.first())
        .and_then(|block| block.get("text"))
        .and_then(Value::as_str)
        .map(|text| text.chars().count())
        .unwrap_or(0)
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(text: &str, max: usize, suffix: &str) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let keep = max.saturating_sub(suffix.chars().count());
    let mut out: String = text.chars().take(keep).collect();
    out.push_str(suffix);
    out
}

fn parse_content(content: &Value) -> Vec<Block> {
    match content {
        Value::String(text) => vec![Block::Text(text.clone())],
        Value::Array(items) => items.iter().filter_map(parse_block).collect(),
        _ => Vec::new(),
    }
}

fn parse_block(value: &Value) -> Option<Block> {
    let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
    match kind {
        "text" => Some(Block::Text(value.get("text").and_then(Value::as_str).unwrap_or("").to_owned())),
        "thinking" | "reasoning" => {
            let text = value
                .get("thinking")
                .or_else(|| value.get("text"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if text.is_empty() { None } else { Some(Block::Thinking(text.to_owned())) }
        }
        "image" | "image_url" | "input_image" => Some(Block::Image),
        "tool_use" | "tool_call" => Some(Block::ToolUse {
            id: value.get("id").and_then(Value::as_str).unwrap_or("").to_owned(),
            name: value.get("name").and_then(Value::as_str).unwrap_or("tool").to_owned(),
            input: value.get("input").or_else(|| value.get("arguments")).cloned().unwrap_or(Value::Null),
        }),
        "tool_result" => Some(Block::ToolResult {
            id: value
                .get("tool_use_id")
                .or_else(|| value.get("toolUseId"))
                .or_else(|| value.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            output: stringify_tool_output(value.get("content").unwrap_or(&Value::Null)),
            is_error: value.get("is_error").and_then(Value::as_bool).unwrap_or(false),
        }),
        _ => None,
    }
}

fn stringify_tool_output(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str).or_else(|| item.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn timestamp_ms(line: &Value, message: &Value) -> i64 {
    let Some(value) = message.get("timestamp").or_else(|| line.get("timestamp")) else {
        return 0;
    };
    if let Some(number) = value.as_i64() {
        return number;
    }
    if let Some(number) = value.as_u64() {
        return i64::try_from(number).unwrap_or(i64::MAX);
    }
    if let Some(number) = value.as_f64() {
        return number as i64;
    }
    value
        .as_str()
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|time| time.timestamp_millis())
        .unwrap_or(0)
}

fn stop_is_error(line: &Value, message: &Value) -> bool {
    message
        .get("stopReason")
        .or_else(|| message.get("stop_reason"))
        .or_else(|| line.get("stopReason"))
        .and_then(Value::as_str)
        .is_some_and(|reason| reason.eq_ignore_ascii_case("error"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomifun_common::AgentType;
    use sha2::{Digest, Sha256};

    #[test]
    fn import_create_request_is_a_plain_nomi_conversation() {
        let request = import_request("hello", &sample_session())
        .unwrap();
        assert_eq!(request.r#type, AgentType::Nomi);
        assert!(request.model.is_none());
        assert_eq!(request.extra["legacy"]["source"], "flowyclaw");
        assert!(request.extra.get("model").is_none());
    }

    #[test]
    fn imported_conversation_keeps_the_workspace_it_needs_to_continue() {
        let extra = legacy_extra(
            &sample_session(),
            json!({
                "temp_workspace_id": "0190f5fe-7c00-7000-8000-0000000000ad",
                "workspace": "C:/data/conversations/imported",
                "is_temporary_workspace": true,
                "skills": ["notes"]
            }),
        );
        assert!(conversation_can_continue(&extra));
        assert!(extra.get("is_temporary_workspace").is_none());
        assert_eq!(extra["temp_workspace_id"], "0190f5fe-7c00-7000-8000-0000000000ad");
        assert_eq!(extra["workspace"], "C:/data/conversations/imported");
        assert_eq!(extra["skills"][0], "notes");
        assert_eq!(extra["titleSource"], "user");
        assert!(!conversation_can_continue(&json!({ "workspace": "C:/custom" })));
    }

    #[test]
    fn imported_bases_join_the_default_workpath_without_enabling_writeback() {
        use nomifun_common::KnowledgeBaseId;
        use nomifun_knowledge::KnowledgeBinding;

        let imported = "0190f5fe-7c00-7000-8000-0000000000ab".to_owned();
        let merged = merge_default_workpath_binding(&KnowledgeBinding::default(), &[imported.clone()]).unwrap();
        assert!(merged.enabled);
        assert!(!merged.writeback);
        assert_eq!(merged.writeback_eagerness, "manual");
        assert_eq!(merged.kb_ids[0].as_str(), imported);

        let existing = KnowledgeBaseId::parse("0190f5fe-7c00-7000-8000-0000000000ac").unwrap();
        let disabled = KnowledgeBinding {
            enabled: false,
            writeback: false,
            writeback_eagerness: "auto".into(),
            channel_write_enabled: true,
            kb_ids: vec![existing.clone()],
        };
        let merged = merge_default_workpath_binding(&disabled, &[imported.clone(), existing.as_str().to_owned()]).unwrap();
        assert!(!merged.enabled);
        assert!(!merged.writeback);
        assert!(merged.channel_write_enabled);
        assert_eq!(merged.writeback_eagerness, "auto");
        assert_eq!(merged.kb_ids.len(), 2);

        let enabled = KnowledgeBinding {
            enabled: true,
            writeback: true,
            writeback_eagerness: "manual".into(),
            channel_write_enabled: false,
            kb_ids: vec![existing],
        };
        let merged = merge_default_workpath_binding(&enabled, &[imported]).unwrap();
        assert!(merged.enabled);
        assert!(merged.writeback);
        assert_eq!(merged.kb_ids.len(), 2);
    }

    #[test]
    fn tool_history_keeps_arguments_the_chat_can_expand() {
        let row = message_row(
            "0190f5fe-7c00-7000-8000-0000000000ae",
            &DisplayMessage {
                kind: DisplayKind::Tool,
                text: "pong".into(),
                tool_name: "echo".into(),
                tool_args: json!("plain"),
                call_id: "t1".into(),
                created_at: 1,
                is_error: true,
            },
            1,
        );
        let content: Value = serde_json::from_str(&row.content).unwrap();
        assert_eq!(row.r#type, "tool_call");
        assert_eq!(row.position.as_deref(), Some("left"));
        assert_eq!(row.status.as_deref(), Some("finish"));
        assert_eq!(content["call_id"], "t1");
        assert_eq!(content["name"], "echo");
        assert_eq!(content["args"]["input"], "plain");
        assert_eq!(content["output"], "pong");
        assert_eq!(content["status"], "error");
        assert!(content["args"].is_object());
    }

    fn sample_session() -> SessionItem {
        SessionItem {
            agent_id: "main".into(),
            session_key: "agent:main:main".into(),
            session_id: "11111111-1111-4111-8111-111111111111".into(),
            source_path: String::new(),
            source_len: 4,
            source_mtime_ms: 5,
            model: "old".into(),
            model_provider: "old-provider".into(),
            updated_at_ms: 6,
            deleted_ids: Vec::new(),
            snapshot_rel: None,
            conversation_id: None,
            message_count: 0,
            status: ItemStatus::Pending,
            error: None,
        }
    }

    #[test]
    fn project_transcript_keeps_the_active_branch() {
        let bytes = br#"{"type":"session","id":"s"}
{"type":"message","id":"u1","message":{"role":"user","content":"hello","timestamp":1700000000000}}
{"type":"message","id":"a1","parentId":"u1","message":{"role":"assistant","content":[{"type":"thinking","thinking":"secret"},{"type":"text","text":"hi"},{"type":"tool_use","id":"t1","name":"echo","input":{"x":1}}],"stopReason":"error"}}
{"type":"message","id":"r1","parentId":"a1","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"pong"}]}}
{"type":"message","id":"u2","parentId":"a1","message":{"role":"user","content":"other branch"}}
NOT JSON
{"type":"message","id":"a2","parentId":"r1","message":{"role":"assistant","content":"done"}}
{"type":"custom","customType":"model-snapshot"}
"#;
        let projection = project_transcript(bytes, &[], &english_notes());
        let texts: Vec<_> = projection
            .messages
            .iter()
            .map(|message| (message.kind, message.text.as_str(), message.is_error))
            .collect();
        assert!(texts.contains(&(DisplayKind::User, "hello", false)));
        assert!(texts.contains(&(DisplayKind::Assistant, "hi", true)));
        assert!(texts.contains(&(DisplayKind::Assistant, "done", false)));
        assert!(texts.contains(&(DisplayKind::Thinking, "secret", false)));
        assert!(texts.iter().any(|(kind, text, _)| *kind == DisplayKind::Tool && text.contains("pong")));
        assert!(texts.iter().all(|(_, text, _)| !text.contains("other branch")));
        let seed = serde_json::to_string(&projection.seed).unwrap();
        assert!(seed.contains("hello"));
        assert!(seed.contains("Earlier tool call: echo"));
        assert!(!seed.contains("secret"));
        assert!(!seed.contains("other branch"));

        let deleted = project_transcript(bytes, &["id:a2".into()], &english_notes());
        assert!(deleted.messages.iter().all(|message| message.text != "done"));
        assert!(deleted.messages.iter().any(|message| message.text == "hello"));
    }

    #[test]
    fn reading_sources_does_not_change_them_and_ignores_unlisted_data() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("agents").join("main").join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        let chat = sessions.join("chat.jsonl");
        fs::write(&chat, b"{\"type\":\"message\",\"id\":\"u\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n").unwrap();
        let deleted = sessions.join("old.deleted.jsonl");
        let reset = sessions.join("chat.jsonl.reset.1");
        let trajectory = sessions.join("chat.trajectory.jsonl");
        let user_line = b"{\"type\":\"message\",\"id\":\"u\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n";
        fs::write(&deleted, user_line).unwrap();
        fs::write(&reset, user_line).unwrap();
        fs::write(&trajectory, user_line).unwrap();
        let outside = root.path().join("outside.txt");
        fs::write(&outside, b"secret").unwrap();
        let vault = root.path().join("vault");
        fs::create_dir_all(vault.join("gbrain")).unwrap();
        let listed = vault.join("listed.md");
        fs::write(&listed, b"# listed").unwrap();
        fs::write(vault.join("unlisted.md"), b"# hidden").unwrap();
        fs::write(vault.join("gbrain").join("index.md"), b"# index").unwrap();
        let catalog = json!({
            "agent:main:main": {"sessionId": "11111111-1111-4111-8111-111111111111", "sessionFile": chat, "updatedAt": 10, "model": "m", "modelProvider": "p"},
            "agent:main:cron:job": {"sessionId": "22222222-2222-4222-8222-222222222222", "sessionFile": chat},
            "agent:main:escape": {"sessionId": "33333333-3333-4333-8333-333333333333", "sessionFile": outside},
            "agent:main:deleted": {"sessionId": "44444444-4444-4444-8444-444444444444", "sessionFile": deleted},
            "agent:main:reset": {"sessionId": "55555555-5555-4555-8555-555555555555", "sessionFile": reset},
            "agent:main:trajectory": {"sessionId": "66666666-6666-4666-8666-666666666666", "sessionFile": trajectory}
        });
        fs::write(sessions.join("sessions.json"), serde_json::to_vec(&catalog).unwrap()).unwrap();
        fs::create_dir_all(root.path().join("knowledge")).unwrap();
        let meta = json!({
            "folders": [{"id": "f1", "name": "Notes", "rootDir": vault}],
            "files": [
                {"folderId": "f1", "originalName": "listed.md", "originalPath": listed, "status": "ready"},
                {"folderId": "f1", "originalName": "gone.bin", "originalPath": vault.join("gone.bin"), "status": "failed"}
            ]
        });
        fs::write(root.path().join("knowledge").join("meta.json"), serde_json::to_vec(&meta).unwrap()).unwrap();
        let workspace = root.path().join("workspace");
        fs::create_dir_all(workspace.join("memory").join("nested")).unwrap();
        fs::create_dir_all(workspace.join("skills")).unwrap();
        fs::write(workspace.join("USER.md"), b"Ada").unwrap();
        fs::write(workspace.join("MEMORY.md"), b"likes tea").unwrap();
        fs::write(workspace.join("HEARTBEAT.md"), b"every day").unwrap();
        fs::write(workspace.join("notes-unlisted.md"), b"stray").unwrap();
        fs::write(workspace.join("memory").join("2024-01-02.md"), b"met Sam").unwrap();
        fs::write(workspace.join("memory").join("skip.txt"), b"not markdown").unwrap();
        fs::write(workspace.join("memory").join("nested").join("secret.md"), b"hidden").unwrap();
        fs::write(workspace.join("skills").join("SKILL.md"), b"skill").unwrap();
        fs::write(
            root.path().join("openclaw.json"),
            br#"{"token":"super-secret-token","agents":{"list":[{"id":"main","workspace":"workspace"}]}}"#,
        )
        .unwrap();

        let before = fingerprints(root.path());
        let scan = scan_source(root.path());
        assert_eq!(fingerprints(root.path()), before);
        let ready: Vec<_> = scan.sessions.iter().filter(|session| session.failure.is_none()).collect();
        assert_eq!(ready.len(), 1);
        assert!(ready[0].source_path.contains("chat.jsonl"));
        assert!(!scan.sessions.iter().any(|session| {
            session.source_path.contains("outside.txt")
                || session.source_path.contains("deleted")
                || session.source_path.contains("reset")
                || session.source_path.contains("trajectory")
        }));
        let knowledge = scan.folders.iter().find(|folder| folder.folder_id == "f1").unwrap();
        let knowledge_sources: Vec<_> = knowledge
            .files
            .iter()
            .filter(|file| !file.source_path.is_empty())
            .map(|file| file.source_path.as_str())
            .collect();
        assert_eq!(knowledge_sources.len(), 1);
        assert!(knowledge_sources[0].contains("listed.md"));
        let memory = scan
            .folders
            .iter()
            .find(|folder| folder.folder_id == "memory:main")
            .unwrap();
        let memory_names: Vec<_> = memory.files.iter().map(|file| file.display_name.as_str()).collect();
        assert!(memory_names.contains(&"USER.md"));
        assert!(memory_names.contains(&"MEMORY.md"));
        assert!(memory_names.contains(&"memory-2024-01-02.md"));
        assert_eq!(memory.files.len(), 3);
        let sources: Vec<_> = scan
            .folders
            .iter()
            .flat_map(|folder| &folder.files)
            .map(|file| file.source_path.as_str())
            .collect();
        assert!(!sources.iter().any(|path| {
            path.contains("unlisted")
                || path.contains("gbrain")
                || path.contains("HEARTBEAT")
                || path.contains("notes-unlisted")
                || path.contains("skip.txt")
                || path.contains("secret.md")
                || path.contains("SKILL.md")
                || path.contains("super-secret-token")
        }));
    }

    #[test]
    fn memory_workspace_outside_the_root_is_read_only() {
        let root = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        fs::write(elsewhere.path().join("USER.md"), b"Ada").unwrap();
        fs::write(elsewhere.path().join("random.md"), b"leave this").unwrap();
        let config = json!({
            "token": "super-secret-token",
            "agents": { "list": [{ "id": "main", "workspace": elsewhere.path() }] }
        });
        fs::write(root.path().join("openclaw.json"), serde_json::to_vec(&config).unwrap()).unwrap();
        let before_root = fingerprints(root.path());
        let before_elsewhere = fingerprints(elsewhere.path());
        let scan = scan_source(root.path());
        assert_eq!(fingerprints(root.path()), before_root);
        assert_eq!(fingerprints(elsewhere.path()), before_elsewhere);
        let memory = scan.folders.iter().find(|folder| folder.folder_id == "memory:main").unwrap();
        assert_eq!(memory.files.len(), 1);
        assert!(memory.files[0].display_name == "USER.md");
        assert!(memory.files[0].source_path.contains("USER.md"));
    }

    #[test]
    fn frozen_receipt_round_trip_keeps_the_manifest() {
        let root = tempfile::tempdir().unwrap();
        let mut receipt = Receipt {
            version: 1,
            state: ReceiptState::Frozen,
            sessions: vec![pending_session("one"), pending_session("two")],
            folders: Vec::new(),
        };
        store_receipt(root.path(), &receipt).unwrap();
        receipt.sessions[0].status = ItemStatus::Imported;
        store_receipt(root.path(), &receipt).unwrap();
        let loaded = load_receipt(root.path()).unwrap().unwrap();
        assert_eq!(loaded.state, ReceiptState::Frozen);
        assert_eq!(loaded.sessions[0].status, ItemStatus::Imported);
        assert_eq!(loaded.sessions[1].status, ItemStatus::Pending);
        let mut sealed = loaded;
        sealed.state = ReceiptState::Sealed;
        store_receipt(root.path(), &sealed).unwrap();
        assert_eq!(load_receipt(root.path()).unwrap().unwrap().state, ReceiptState::Sealed);
    }

    fn pending_session(id: &str) -> SessionItem {
        SessionItem {
            agent_id: "main".into(),
            session_key: format!("agent:main:{id}"),
            session_id: id.into(),
            source_path: format!("C:\\kept\\{id}.jsonl"),
            source_len: 1,
            source_mtime_ms: 2,
            model: String::new(),
            model_provider: String::new(),
            updated_at_ms: 3,
            deleted_ids: Vec::new(),
            snapshot_rel: None,
            conversation_id: None,
            message_count: 0,
            status: ItemStatus::Pending,
            error: None,
        }
    }

    fn fingerprints(root: &Path) -> Vec<(String, u64, i64, Vec<u8>)> {
        let mut files = Vec::new();
        collect_files(root, &mut files);
        files.sort();
        files
            .into_iter()
            .map(|path| {
                let meta = fs::metadata(&path).unwrap();
                let bytes = fs::read(&path).unwrap();
                (
                    path.display().to_string(),
                    meta.len(),
                    mtime_ms(&meta),
                    Sha256::digest(&bytes).to_vec(),
                )
            })
            .collect()
    }

    fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_files(&path, files);
            } else {
                files.push(path);
            }
        }
    }
}

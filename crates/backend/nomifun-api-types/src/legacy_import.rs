use serde::{Deserialize, Serialize};

/// Where a one-shot import of the previous desktop app currently stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyImportPhase {
    /// No receipt has been written. Preview may still report data that can be imported.
    Absent,
    /// The first run froze a file list. A later run may finish that list only.
    Frozen,
    /// The run finished. Further imports are refused.
    Sealed,
}

/// What Settings shows before the user starts or resumes an import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyImportPreview {
    pub phase: LegacyImportPhase,
    pub source_found: bool,
    pub session_count: u32,
    pub knowledge_base_count: u32,
    pub document_count: u32,
    pub pending_count: u32,
    pub failed_count: u32,
}

/// Totals after a finished or empty import attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyImportReport {
    pub phase: LegacyImportPhase,
    pub conversations_imported: u32,
    pub messages_imported: u32,
    pub knowledge_bases_imported: u32,
    pub documents_imported: u32,
    pub failed_count: u32,
}

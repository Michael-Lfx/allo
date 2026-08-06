//! Durable, non-destructive work-root relocation.
//!
//! A work-root change is a filesystem operation, not a dataset reset. The
//! database and every data-root side store stay in place; only the managed
//! `<work_root>/conversations` tree is published under the new root and the
//! v3 lifecycle markers are rebound to the same storage generation.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::dataset_roots::WORK_ROOT_OWNER_FILE;
use crate::error::AppError;
use crate::factory_reset as factory;
use crate::factory_reset::DatasetReceiptStatus;
use crate::id::validate_uuidv7;
use crate::paths::{paths_equivalent, simplified, stored_path_matches};
use crate::timestamp::now_ms;

pub const PENDING_RELOCATION_DIR: &str = ".work-dir-relocation.pending";
pub const RELOCATION_PLAN_FILE: &str = "plan.json";
pub const RELOCATION_RESULT_FILE: &str = ".work-dir-relocation.last.json";
const PLAN_VERSION: u32 = 1;
const MAX_CONTROL_FILE_BYTES: u64 = 64 * 1024;
const CONVERSATIONS_DIR: &str = "conversations";
const BACKUP_DIR: &str = ".nomifun-work-relocation-backups";
const MAX_STATUS_ERROR_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkDirRelocationPlan {
    pub version: u32,
    pub operation_id: String,
    pub data_dir: String,
    pub source_work_dir: String,
    pub target_work_dir: String,
    pub generation: String,
    pub created_at: i64,
    /// Whether the source had a managed conversations tree when the request
    /// was armed. This distinguishes a crash after an atomic move from a
    /// target directory that was populated by another process while the app
    /// was stopped.
    #[serde(default)]
    pub source_conversations_present: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkDirRelocationState {
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkDirRelocationStatus {
    pub state: WorkDirRelocationState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_work_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_work_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollback_copy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelocationPhase {
    Requested,
    Copying,
    TargetPublished,
    BindingsRebound,
    Completed,
}

impl RelocationPhase {
    const ALL: [Self; 5] = [
        Self::Requested,
        Self::Copying,
        Self::TargetPublished,
        Self::BindingsRebound,
        Self::Completed,
    ];

    const fn file_name(self) -> &'static str {
        match self {
            Self::Requested => "phase-requested",
            Self::Copying => "phase-copying",
            Self::TargetPublished => "phase-target-published",
            Self::BindingsRebound => "phase-bindings-rebound",
            Self::Completed => "phase-completed",
        }
    }
}

fn pending_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(PENDING_RELOCATION_DIR)
}

fn plan_path(data_dir: &Path) -> PathBuf {
    pending_dir(data_dir).join(RELOCATION_PLAN_FILE)
}

fn phase_path(data_dir: &Path, phase: RelocationPhase) -> PathBuf {
    pending_dir(data_dir).join(phase.file_name())
}

fn staging_path(target_work_dir: &Path, operation_id: &str) -> PathBuf {
    target_work_dir.join(format!(
        ".nomifun-work-relocation-{operation_id}.staging"
    ))
}

fn backup_path(source_work_dir: &Path, operation_id: &str) -> PathBuf {
    source_work_dir
        .join(BACKUP_DIR)
        .join(operation_id)
        .join(CONVERSATIONS_DIR)
}

fn ensure_real_directory(path: &Path, label: &str) -> Result<(), AppError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        AppError::Internal(format!("inspect {label} {}: {error}", path.display()))
    })?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err(AppError::Conflict(format!(
            "{label} must be a real directory: {}",
            path.display()
        )));
    }
    Ok(())
}

/// Check every existing component instead of only the leaf. Canonicalization
/// alone would silently follow a junction/symlink in a parent directory and
/// could publish the dataset somewhere other than the user-selected root.
fn ensure_real_directory_tree(path: &Path, label: &str) -> Result<(), AppError> {
    ensure_real_directory(path, label)?;
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor).map_err(|error| {
            AppError::Internal(format!(
                "inspect {label} path component {}: {error}",
                ancestor.display()
            ))
        })?;
        if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
            return Err(AppError::Conflict(format!(
                "{label} path contains a symlink, junction, or non-directory component: {}",
                ancestor.display()
            )));
        }
    }
    Ok(())
}

fn ensure_pending_directory(data_dir: &Path) -> Result<PathBuf, AppError> {
    let path = pending_dir(data_dir);
    match fs::symlink_metadata(&path) {
        Ok(_) => ensure_real_directory(&path, "work-dir relocation state directory")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&path).map_err(|error| {
                AppError::Internal(format!(
                    "create work-dir relocation state directory {}: {error}",
                    path.display()
                ))
            })?;
            ensure_real_directory(&path, "work-dir relocation state directory")?;
        }
        Err(error) => {
            return Err(AppError::Internal(format!(
                "inspect work-dir relocation state directory {}: {error}",
                path.display()
            )));
        }
    }
    Ok(path)
}

fn canonical_real_directory(path: &Path, label: &str) -> Result<PathBuf, AppError> {
    ensure_real_directory_tree(path, label)?;
    fs::canonicalize(path)
        .map(|path| simplified(&path))
        .map_err(|error| {
            AppError::Internal(format!("canonicalize {label} {}: {error}", path.display()))
        })
}

/// Create a user-selected target one component at a time, refusing to follow
/// an existing symlink/junction in any parent. `create_dir_all` is not safe for
/// this boundary because it resolves a linked parent before the relocation plan
/// gets a chance to inspect the canonical destination.
pub fn prepare_work_dir_target(target: &Path) -> Result<PathBuf, AppError> {
    if !target.is_absolute() {
        return Err(AppError::BadRequest(format!(
            "work-dir relocation target must be absolute: {}",
            target.display()
        )));
    }

    let mut missing = Vec::new();
    let mut cursor = target.to_path_buf();
    loop {
        match fs::symlink_metadata(&cursor) {
            Ok(metadata) => {
                if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
                    return Err(AppError::BadRequest(format!(
                        "work-dir relocation target contains a symlink, junction, or non-directory component: {}",
                        cursor.display()
                    )));
                }
                ensure_real_directory_tree(&cursor, "work-dir relocation target")?;
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(cursor.clone());
                cursor = cursor.parent().map(Path::to_path_buf).ok_or_else(|| {
                    AppError::BadRequest(format!(
                        "work-dir relocation target has no existing parent: {}",
                        target.display()
                    ))
                })?;
            }
            Err(error) => {
                return Err(AppError::BadRequest(format!(
                    "inspect work-dir relocation target {}: {error}",
                    cursor.display()
                )));
            }
        }
    }

    for directory in missing.into_iter().rev() {
        match fs::create_dir(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(AppError::BadRequest(format!(
                    "create work-dir relocation target {}: {error}",
                    directory.display()
                )));
            }
        }
        ensure_real_directory_tree(&directory, "work-dir relocation target")?;
    }

    canonical_real_directory(target, "work-dir relocation target")
}

/// Reject a work-root relationship before creating a user-selected target.
///
/// The target may not exist yet, so this check intentionally uses normalized
/// path spellings instead of filesystem canonicalization. The authoritative
/// canonical check is repeated after the target is created and again when a
/// durable plan is consumed during startup.
pub fn validate_work_dir_relationship(
    source_work_dir: &Path,
    target_work_dir: &Path,
) -> Result<(), AppError> {
    if lexical_paths_equivalent(source_work_dir, target_work_dir)
        || lexical_path_is_prefix(source_work_dir, target_work_dir)
        || lexical_path_is_prefix(target_work_dir, source_work_dir)
    {
        return Err(AppError::BadRequest(
            "work-dir relocation source and target must be distinct, non-nested directories"
                .into(),
        ));
    }
    Ok(())
}

fn validate_plan(plan: &WorkDirRelocationPlan, data_dir: &Path) -> Result<(), AppError> {
    if plan.version != PLAN_VERSION {
        return Err(AppError::Internal(format!(
            "unsupported work-dir relocation plan version {}",
            plan.version
        )));
    }
    validate_uuidv7(&plan.operation_id).map_err(|error| {
        AppError::Internal(format!("invalid work-dir relocation operation ID: {error}"))
    })?;
    validate_uuidv7(&plan.generation).map_err(|error| {
        AppError::Internal(format!("invalid work-dir relocation generation: {error}"))
    })?;
    if plan.created_at <= 0 {
        return Err(AppError::Internal(
            "work-dir relocation created_at must be positive".into(),
        ));
    }
    let canonical_data = canonical_real_directory(data_dir, "dataset root")?;
    let stored_data = Path::new(&plan.data_dir);
    if plan.data_dir.is_empty()
        || !stored_data.is_absolute()
        || !stored_path_matches(&plan.data_dir, &canonical_data)
    {
        return Err(AppError::Conflict(
            "work-dir relocation plan is bound to a different data root".into(),
        ));
    }
    for (label, value) in [
        ("source work root", &plan.source_work_dir),
        ("target work root", &plan.target_work_dir),
    ] {
        let path = Path::new(value);
        if value.is_empty()
            || !path.is_absolute()
            || crate::workspace_path_has_edge_whitespace_segment(path)
        {
            return Err(AppError::Internal(format!(
                "work-dir relocation plan has an unsafe {label}"
            )));
        }
    }
    let source = canonical_real_directory(
        Path::new(&plan.source_work_dir),
        "relocation source work root",
    )?;
    let target = canonical_real_directory(
        Path::new(&plan.target_work_dir),
        "relocation target work root",
    )?;
    if lexical_paths_equivalent(&source, &target) {
        return Err(AppError::Conflict(
            "work-dir relocation source and target are identical".into(),
        ));
    }
    if paths_overlap(&source, &target) {
        return Err(AppError::Conflict(
            "work-dir relocation source and target must not be nested".into(),
        ));
    }
    if paths_overlap(&canonical_data, &target) {
        return Err(AppError::Conflict(
            "work-dir relocation target overlaps the dataset root".into(),
        ));
    }
    Ok(())
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    !lexical_paths_equivalent(left, right)
        && (lexical_path_is_prefix(left, right) || lexical_path_is_prefix(right, left))
}

fn normalized_path_string(path: &Path) -> String {
    let mut value = simplified(path).to_string_lossy().replace('\\', "/");
    while value.len() > 1 && value.ends_with('/') {
        value.pop();
    }
    #[cfg(windows)]
    {
        value.make_ascii_lowercase();
    }
    value
}

fn lexical_paths_equivalent(left: &Path, right: &Path) -> bool {
    normalized_path_string(left) == normalized_path_string(right)
}

fn lexical_path_is_prefix(parent: &Path, child: &Path) -> bool {
    let parent = normalized_path_string(parent);
    let child = normalized_path_string(child);
    if parent == "/" {
        return child.starts_with('/');
    }
    child.starts_with(&parent)
        && child
            .as_bytes()
            .get(parent.len())
            .is_some_and(|separator| *separator == b'/')
}

fn read_storage_generation(data_dir: &Path) -> Result<String, AppError> {
    let path = data_dir.join("storage-generation");
    let bytes = crate::dir_config::read_bounded_regular_file(&path, 128).map_err(|error| {
        AppError::Internal(format!("read storage generation {}: {error}", path.display()))
    })?;
    let generation = std::str::from_utf8(&bytes)
        .map_err(|error| AppError::Internal(format!("storage generation is not UTF-8: {error}")))?
        .trim()
        .to_owned();
    validate_uuidv7(&generation).map_err(|error| {
        AppError::Internal(format!("storage generation is invalid: {error}"))
    })?;
    Ok(generation)
}

fn validate_target_for_plan(data_dir: &Path, target: &Path) -> Result<PathBuf, AppError> {
    let canonical_data = canonical_real_directory(data_dir, "dataset root")?;
    let canonical_target = canonical_real_directory(target, "work-dir relocation target")?;
    if paths_overlap(&canonical_data, &canonical_target) {
        return Err(AppError::BadRequest(format!(
            "work-dir relocation target {} overlaps data root {}",
            canonical_target.display(),
            canonical_data.display()
        )));
    }
    for reserved in [CONVERSATIONS_DIR, WORK_ROOT_OWNER_FILE] {
        match fs::symlink_metadata(canonical_target.join(reserved)) {
            Ok(_) => {
                return Err(AppError::BadRequest(format!(
                    "work-dir relocation target {} already contains reserved entry {reserved}",
                    canonical_target.display()
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(AppError::Internal(format!(
                    "inspect work-dir relocation target entry {reserved}: {error}"
                )));
            }
        }
    }
    for entry in fs::read_dir(&canonical_target).map_err(|error| {
        AppError::Internal(format!(
            "read work-dir relocation target {}: {error}",
            canonical_target.display()
        ))
    })? {
        let entry = entry.map_err(|error| {
            AppError::Internal(format!(
                "read work-dir relocation target entry: {error}"
            ))
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(".nomifun-work-relocation-") {
            return Err(AppError::Conflict(format!(
                "work-dir relocation target contains unfinished migration state: {}",
                entry.path().display()
            )));
        }
    }
    Ok(canonical_target)
}

/// Persist a non-destructive work-root relocation request.
pub fn request_work_dir_relocation(
    data_dir: &Path,
    source_work_dir: &Path,
    target_work_dir: &Path,
) -> Result<WorkDirRelocationPlan, AppError> {
    factory::require_data_root_not_owned_as_external_work(data_dir)?;
    let canonical_data = canonical_real_directory(data_dir, "dataset root")?;
    let canonical_source = canonical_real_directory(source_work_dir, "current work root")?;
    let canonical_target = validate_target_for_plan(&canonical_data, target_work_dir)?;
    validate_work_dir_relationship(&canonical_source, &canonical_target)?;
    factory::require_safe_work_dir_change_target(&canonical_data, &canonical_target)?;
    let bound_source = factory::finalized_v3_work_dir(&canonical_data)?.ok_or_else(|| {
        AppError::Conflict(
            "the current dataset has no finalized v3 work-root binding".into(),
        )
    })?;
    if !paths_equivalent(&bound_source, &canonical_source) {
        return Err(AppError::Conflict(
            "current work root does not match the finalized dataset binding".into(),
        ));
    }
    let generation = read_storage_generation(&canonical_data)?;
    let source_conversations_present = entry_exists(
        &canonical_source.join(CONVERSATIONS_DIR),
        "relocation source conversations",
    )?;

    if let Some(existing) = read_pending_plan(&canonical_data)? {
        if paths_equivalent(Path::new(&existing.source_work_dir), &canonical_source)
            && paths_equivalent(Path::new(&existing.target_work_dir), &canonical_target)
            && existing.generation == generation
        {
            return Ok(existing);
        }
        return Err(AppError::Conflict(
            "a different work-dir relocation is already pending".into(),
        ));
    }

    let plan = WorkDirRelocationPlan {
        version: PLAN_VERSION,
        operation_id: Uuid::now_v7().to_string(),
        data_dir: canonical_data.display().to_string(),
        source_work_dir: canonical_source.display().to_string(),
        target_work_dir: canonical_target.display().to_string(),
        generation,
        created_at: now_ms(),
        source_conversations_present,
    };
    ensure_pending_directory(&canonical_data)?;
    let bytes = serde_json::to_vec_pretty(&plan).map_err(|error| {
        AppError::Internal(format!("serialize work-dir relocation plan: {error}"))
    })?;
    crate::dir_config::write_atomic_replace(&plan_path(&canonical_data), &bytes).map_err(
        |error| AppError::Internal(format!("write work-dir relocation plan: {error}")),
    )?;
    write_phase(&canonical_data, &plan, RelocationPhase::Requested)?;
    Ok(plan)
}

pub fn read_pending_plan(
    data_dir: &Path,
) -> Result<Option<WorkDirRelocationPlan>, AppError> {
    match fs::symlink_metadata(pending_dir(data_dir)) {
        Ok(_) => ensure_real_directory(&pending_dir(data_dir), "work-dir relocation state directory")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(AppError::Internal(format!(
                "inspect work-dir relocation state directory {}: {error}",
                pending_dir(data_dir).display()
            )));
        }
    }
    let path = plan_path(data_dir);
    let bytes = match crate::dir_config::read_bounded_regular_file(&path, MAX_CONTROL_FILE_BYTES) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(AppError::Internal(format!(
                "read work-dir relocation plan {}: {error}",
                path.display()
            )));
        }
    };
    let plan: WorkDirRelocationPlan = serde_json::from_slice(&bytes).map_err(|error| {
        AppError::Internal(format!(
            "malformed work-dir relocation plan {}: {error}",
            path.display()
        ))
    })?;
    validate_plan(&plan, data_dir)?;
    Ok(Some(plan))
}

fn write_phase(
    data_dir: &Path,
    plan: &WorkDirRelocationPlan,
    phase: RelocationPhase,
) -> Result<(), AppError> {
    ensure_pending_directory(data_dir)?;
    crate::dir_config::write_atomic_replace(
        &phase_path(data_dir, phase),
        plan.operation_id.as_bytes(),
    )
    .map_err(|error| {
        AppError::Internal(format!(
            "write work-dir relocation phase {}: {error}",
            phase.file_name()
        ))
    })
}

fn phase_matches(
    data_dir: &Path,
    plan: &WorkDirRelocationPlan,
    phase: RelocationPhase,
) -> Result<bool, AppError> {
    let path = phase_path(data_dir, phase);
    let bytes = match crate::dir_config::read_bounded_regular_file(&path, 256) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(AppError::Internal(format!(
                "read work-dir relocation phase {}: {error}",
                phase.file_name()
            )));
        }
    };
    if bytes != plan.operation_id.as_bytes() {
        return Err(AppError::Conflict(format!(
            "work-dir relocation phase {} belongs to another operation",
            phase.file_name()
        )));
    }
    Ok(true)
}

pub fn read_last_status(
    data_dir: &Path,
) -> Result<Option<WorkDirRelocationStatus>, AppError> {
    let path = data_dir.join(RELOCATION_RESULT_FILE);
    let bytes = match crate::dir_config::read_bounded_regular_file(&path, MAX_CONTROL_FILE_BYTES) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(AppError::Internal(format!(
                "read work-dir relocation result {}: {error}",
                path.display()
            )));
        }
    };
    let status = serde_json::from_slice(&bytes).map_err(|error| {
        AppError::Internal(format!(
            "malformed work-dir relocation result {}: {error}",
            path.display()
        ))
    })?;
    Ok(Some(status))
}

fn write_status(data_dir: &Path, status: &WorkDirRelocationStatus) -> Result<(), AppError> {
    let mut bytes = serde_json::to_vec_pretty(status).map_err(|error| {
        AppError::Internal(format!("serialize work-dir relocation result: {error}"))
    })?;
    if bytes.len() as u64 > MAX_CONTROL_FILE_BYTES {
        // Paths are user-controlled and can be close to the platform limit.
        // Preserve the operation identity and bounded error, but drop the
        // optional path diagnostics rather than leaving no result file at all.
        let compact = WorkDirRelocationStatus {
            state: status.state.clone(),
            operation_id: status.operation_id.clone(),
            source_work_dir: None,
            target_work_dir: None,
            rollback_copy: None,
            error: status.error.clone().map(bounded_status_error),
        };
        bytes = serde_json::to_vec_pretty(&compact).map_err(|error| {
            AppError::Internal(format!("serialize compact work-dir relocation result: {error}"))
        })?;
        if bytes.len() as u64 > MAX_CONTROL_FILE_BYTES {
            return Err(AppError::Internal(
                "compact work-dir relocation result exceeds the control-file size limit".into(),
            ));
        }
    }
    crate::dir_config::write_atomic_replace(&data_dir.join(RELOCATION_RESULT_FILE), &bytes)
        .map_err(|error| AppError::Internal(format!("write work-dir relocation result: {error}")))
}

fn bounded_status_error(error: impl Into<String>) -> String {
    let mut error = error.into();
    if error.len() <= MAX_STATUS_ERROR_BYTES {
        return error;
    }
    let mut end = MAX_STATUS_ERROR_BYTES.saturating_sub("...".len());
    while !error.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    error.truncate(end);
    error.push_str("...");
    error
}

/// Persist a failed state when the pending plan itself cannot be parsed or
/// safely inspected. There may be no trustworthy operation ID in that case,
/// so the status intentionally contains only the error; startup still falls
/// back to the finalized receipt's source work root.
pub fn record_relocation_failure(
    data_dir: &Path,
    error: impl Into<String>,
) -> Result<(), AppError> {
    write_status(
        data_dir,
        &WorkDirRelocationStatus {
            state: WorkDirRelocationState::Failed,
            operation_id: None,
            source_work_dir: None,
            target_work_dir: None,
            rollback_copy: None,
            error: Some(bounded_status_error(error)),
        },
    )
}

/// Persist a failure for a trusted plan while retaining its operation and
/// source/target identity. The plan remains pending so the next startup can
/// retry after the external lock or filesystem problem is fixed.
pub fn record_plan_relocation_failure(
    data_dir: &Path,
    plan: &WorkDirRelocationPlan,
    error: impl Into<String>,
) -> Result<(), AppError> {
    write_status(
        data_dir,
        &failed_status(plan, &AppError::Internal(bounded_status_error(error)), None),
    )
}

fn failed_status(plan: &WorkDirRelocationPlan, error: &AppError, rollback_copy: Option<PathBuf>) -> WorkDirRelocationStatus {
    WorkDirRelocationStatus {
        state: WorkDirRelocationState::Failed,
        operation_id: Some(plan.operation_id.clone()),
        source_work_dir: Some(plan.source_work_dir.clone()),
        target_work_dir: Some(plan.target_work_dir.clone()),
        rollback_copy: rollback_copy.map(|path| path.display().to_string()),
        error: Some(bounded_status_error(error.to_string())),
    }
}

fn completed_status(plan: &WorkDirRelocationPlan, rollback_copy: Option<PathBuf>) -> WorkDirRelocationStatus {
    WorkDirRelocationStatus {
        state: WorkDirRelocationState::Completed,
        operation_id: Some(plan.operation_id.clone()),
        source_work_dir: Some(plan.source_work_dir.clone()),
        target_work_dir: Some(plan.target_work_dir.clone()),
        rollback_copy: rollback_copy.map(|path| path.display().to_string()),
        error: None,
    }
}

/// Convert the old destructive WorkDirChange request before the reset
/// coordinator can see it. This is intentionally called before normal work
/// root resolution during startup.
pub fn convert_legacy_work_dir_change_request(
    data_dir: &Path,
) -> Result<Option<WorkDirRelocationPlan>, AppError> {
    if let Some(existing) = read_pending_plan(data_dir)? {
        if let Some(legacy) = factory::pending_work_dir_change_request(data_dir)? {
            let target = Path::new(&legacy.work_dir);
            if !paths_equivalent(Path::new(&existing.target_work_dir), target) {
                return Err(AppError::Conflict(
                    "legacy work-dir request conflicts with a pending relocation plan".into(),
                ));
            }
            factory::consume_work_dir_change_request(data_dir, &legacy.operation_id)?;
        }
        return Ok(Some(existing));
    }
    let Some(legacy) = factory::pending_work_dir_change_request(data_dir)? else {
        return Ok(None);
    };
    let source = factory::finalized_v3_work_dir(data_dir)?.ok_or_else(|| {
        AppError::Conflict(
            "cannot convert a legacy work-dir request without a current v3 receipt".into(),
        )
    })?;
    let target = validate_target_for_plan(data_dir, &legacy.work_dir)?;
    validate_work_dir_relationship(&source, &target)?;
    factory::require_safe_work_dir_change_target(data_dir, &target)?;
    let generation = read_storage_generation(data_dir)?;
    let plan = WorkDirRelocationPlan {
        version: PLAN_VERSION,
        // Preserve the durable operation identity across the compatibility
        // conversion so support logs can correlate the old request and new
        // result file.
        operation_id: legacy.operation_id.clone(),
        data_dir: canonical_real_directory(data_dir, "dataset root")?.display().to_string(),
        source_work_dir: source.display().to_string(),
        target_work_dir: target.display().to_string(),
        generation,
        created_at: legacy.requested_at,
        source_conversations_present: entry_exists(
            &source.join(CONVERSATIONS_DIR),
            "relocation source conversations",
        )?,
    };
    ensure_pending_directory(data_dir)?;
    let bytes = serde_json::to_vec_pretty(&plan).map_err(|error| {
        AppError::Internal(format!("serialize converted work-dir relocation plan: {error}"))
    })?;
    crate::dir_config::write_atomic_replace(&plan_path(data_dir), &bytes).map_err(|error| {
        AppError::Internal(format!("write converted work-dir relocation plan: {error}"))
    })?;
    write_phase(data_dir, &plan, RelocationPhase::Requested)?;
    factory::consume_work_dir_change_request(data_dir, &legacy.operation_id)?;
    Ok(Some(plan))
}

fn copy_file_with_hash(source: &Path, destination: &Path) -> Result<(u64, [u8; 32]), AppError> {
    let mut input = File::open(source).map_err(|error| {
        AppError::Internal(format!("open relocation source file {}: {error}", source.display()))
    })?;
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)
        .map_err(|error| {
            AppError::Internal(format!(
                "create relocation staging file {}: {error}",
                destination.display()
            ))
        })?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 128 * 1024];
    let mut total = 0_u64;
    loop {
        let read = input.read(&mut buffer).map_err(|error| {
            AppError::Internal(format!("read relocation source file {}: {error}", source.display()))
        })?;
        if read == 0 {
            break;
        }
        total = total.checked_add(read as u64).ok_or_else(|| {
            AppError::Conflict("relocation file size overflow".into())
        })?;
        digest.update(&buffer[..read]);
        output.write_all(&buffer[..read]).map_err(|error| {
            AppError::Internal(format!(
                "write relocation staging file {}: {error}",
                destination.display()
            ))
        })?;
    }
    output.sync_all().map_err(|error| {
        AppError::Internal(format!(
            "flush relocation staging file {}: {error}",
            destination.display()
        ))
    })?;
    let hash: [u8; 32] = digest.finalize().into();
    Ok((total, hash))
}

fn hash_regular_file(path: &Path) -> Result<(u64, [u8; 32]), AppError> {
    let mut file = File::open(path).map_err(|error| {
        AppError::Internal(format!("open relocation file {}: {error}", path.display()))
    })?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 128 * 1024];
    let mut total = 0_u64;
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            AppError::Internal(format!("hash relocation file {}: {error}", path.display()))
        })?;
        if read == 0 {
            break;
        }
        total = total.checked_add(read as u64).ok_or_else(|| {
            AppError::Conflict("relocation file size overflow".into())
        })?;
        digest.update(&buffer[..read]);
    }
    Ok((total, digest.finalize().into()))
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), AppError> {
    let source_metadata = fs::symlink_metadata(source).map_err(|error| {
        AppError::Internal(format!("inspect relocation source {}: {error}", source.display()))
    })?;
    if unsupported_reparse(&source_metadata) {
        return Err(AppError::Conflict(format!(
            "unsupported reparse point in relocation source {}",
            source.display()
        )));
    }
    if source_metadata.file_type().is_symlink() {
        create_link(source, destination)?;
        return Ok(());
    }
    if !source_metadata.is_dir() {
        return Err(AppError::Conflict(format!(
            "relocation source is not a directory: {}",
            source.display()
        )));
    }
    fs::create_dir(destination).map_err(|error| {
        AppError::Internal(format!("create relocation staging directory {}: {error}", destination.display()))
    })?;
    for entry in fs::read_dir(source).map_err(|error| {
        AppError::Internal(format!("read relocation source directory {}: {error}", source.display()))
    })? {
        let entry = entry.map_err(|error| {
            AppError::Internal(format!("read relocation directory entry: {error}"))
        })?;
        let child_source = entry.path();
        let child_destination = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&child_source).map_err(|error| {
            AppError::Internal(format!("inspect relocation entry {}: {error}", child_source.display()))
        })?;
        if unsupported_reparse(&metadata) {
            return Err(AppError::Conflict(format!(
                "unsupported reparse point in relocation source {}",
                child_source.display()
            )));
        }
        if metadata.file_type().is_symlink() {
            create_link(&child_source, &child_destination)?;
        } else if metadata.is_dir() {
            copy_tree(&child_source, &child_destination)?;
        } else if metadata.is_file() {
            let (source_size, source_hash) = copy_file_with_hash(&child_source, &child_destination)?;
            let (target_size, target_hash) = hash_regular_file(&child_destination)?;
            if source_size != target_size || source_hash != target_hash {
                return Err(AppError::Conflict(format!(
                    "relocation checksum verification failed for {}",
                    child_source.display()
                )));
            }
        } else {
            return Err(AppError::Conflict(format!(
                "unsupported filesystem entry in relocation source {}",
                child_source.display()
            )));
        }
    }
    Ok(())
}

fn compare_tree(left: &Path, right: &Path) -> Result<(), AppError> {
    compare_entry(left, right)
}

fn validate_managed_tree_entry(path: &Path, label: &str) -> Result<(), AppError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        AppError::Internal(format!(
            "inspect {label} {}: {error}",
            path.display()
        ))
    })?;
    if unsupported_reparse(&metadata) {
        return Err(AppError::Conflict(format!(
            "unsupported reparse point in {label} {}",
            path.display()
        )));
    }
    if !metadata.file_type().is_symlink() && !metadata.is_dir() {
        return Err(AppError::Conflict(format!(
            "{label} must be a directory or symlink: {}",
            path.display()
        )));
    }
    Ok(())
}

/// Validate a tree before deleting it. Never recurse through a symlink or a
/// Windows reparse point: the staging path is operation-owned, but its
/// contents can still have been tampered with while the application was down.
fn validate_managed_tree_for_removal(path: &Path, label: &str) -> Result<(), AppError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        AppError::Internal(format!("inspect {label} {}: {error}", path.display()))
    })?;
    if unsupported_reparse(&metadata) {
        return Err(AppError::Conflict(format!(
            "unsupported reparse point in {label} {}",
            path.display()
        )));
    }
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_file() {
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(AppError::Conflict(format!(
            "{label} must be a directory or symlink: {}",
            path.display()
        )));
    }
    for entry in fs::read_dir(path).map_err(|error| {
        AppError::Internal(format!("read {label} {}: {error}", path.display()))
    })? {
        let entry = entry.map_err(|error| {
            AppError::Internal(format!("read {label} entry {}: {error}", path.display()))
        })?;
        validate_managed_tree_for_removal(&entry.path(), label)?;
    }
    Ok(())
}

fn remove_managed_tree(path: &Path, label: &str) -> Result<(), AppError> {
    validate_managed_tree_for_removal(path, label)?;
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        AppError::Internal(format!("inspect {label} {}: {error}", path.display()))
    })?;
    let result = if metadata.file_type().is_symlink() {
        fs::remove_file(path)
    } else {
        fs::remove_dir_all(path)
    };
    result.map_err(|error| AppError::Internal(format!("remove {label} {}: {error}", path.display())))
}

fn entry_exists(path: &Path, label: &str) -> Result<bool, AppError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(AppError::Internal(format!(
            "inspect {label} {}: {error}",
            path.display()
        ))),
    }
}

fn compare_entry(left: &Path, right: &Path) -> Result<(), AppError> {
    let left_metadata = fs::symlink_metadata(left).map_err(|error| {
        AppError::Internal(format!("inspect relocation source {}: {error}", left.display()))
    })?;
    let right_metadata = fs::symlink_metadata(right).map_err(|error| {
        AppError::Internal(format!("inspect relocation target {}: {error}", right.display()))
    })?;
    if unsupported_reparse(&left_metadata) || unsupported_reparse(&right_metadata) {
        return Err(AppError::Conflict("unsupported reparse point during relocation verification".into()));
    }
    if left_metadata.file_type().is_symlink() || right_metadata.file_type().is_symlink() {
        if !(left_metadata.file_type().is_symlink() && right_metadata.file_type().is_symlink())
            || fs::read_link(left).map_err(|error| AppError::Internal(error.to_string()))?
                != fs::read_link(right).map_err(|error| AppError::Internal(error.to_string()))?
        {
            return Err(AppError::Conflict("relocation link verification failed".into()));
        }
        return Ok(());
    }
    if left_metadata.is_file() && right_metadata.is_file() {
        if hash_regular_file(left)? != hash_regular_file(right)? {
            return Err(AppError::Conflict(format!(
                "relocation checksum verification failed for {}",
                left.display()
            )));
        }
        return Ok(());
    }
    if !(left_metadata.is_dir() && right_metadata.is_dir()) {
        return Err(AppError::Conflict("relocation entry types differ".into()));
    }

    let mut left_names = BTreeSet::new();
    for entry in fs::read_dir(left).map_err(|error| AppError::Internal(error.to_string()))? {
        let entry = entry.map_err(|error| AppError::Internal(error.to_string()))?;
        left_names.insert(entry.file_name());
    }
    let mut right_names = BTreeSet::new();
    for entry in fs::read_dir(right).map_err(|error| AppError::Internal(error.to_string()))? {
        let entry = entry.map_err(|error| AppError::Internal(error.to_string()))?;
        right_names.insert(entry.file_name());
    }
    if left_names != right_names {
        return Err(AppError::Conflict("relocation tree entries changed during copy".into()));
    }
    for name in left_names {
        compare_entry(&left.join(&name), &right.join(&name))?;
    }
    Ok(())
}

fn publish_conversations(
    data_dir: &Path,
    plan: &WorkDirRelocationPlan,
) -> Result<bool, AppError> {
    let source = Path::new(&plan.source_work_dir).join(CONVERSATIONS_DIR);
    let target = Path::new(&plan.target_work_dir).join(CONVERSATIONS_DIR);
    let staging = staging_path(Path::new(&plan.target_work_dir), &plan.operation_id);
    let backup = backup_path(Path::new(&plan.source_work_dir), &plan.operation_id);
    let source_exists = entry_exists(&source, "relocation source conversations")?;
    let target_exists = entry_exists(&target, "relocation target conversations")?;
    let staging_exists = entry_exists(&staging, "relocation staging tree")?;
    let backup_exists = entry_exists(&backup, "relocation backup tree")?;
    if source_exists {
        validate_managed_tree_entry(&source, "relocation source conversations")?;
    }
    if target_exists {
        validate_managed_tree_entry(&target, "relocation target conversations")?;
    }
    if staging_exists {
        validate_managed_tree_entry(&staging, "relocation staging tree")?;
    }
    if backup_exists {
        validate_managed_tree_entry(&backup, "relocation backup tree")?;
    }
    let copying_phase = phase_matches(data_dir, plan, RelocationPhase::Copying)?;
    let target_was_published = phase_matches(data_dir, plan, RelocationPhase::TargetPublished)?;
    if source_exists && !plan.source_conversations_present {
        return Err(AppError::Conflict(
            "relocation source conversations appeared after the plan was created".into(),
        ));
    }
    if !source_exists && !target_exists && !staging_exists {
        if plan.source_conversations_present || backup_exists {
            return Err(AppError::Conflict(
                "relocation source conversations disappeared before they were published".into(),
            ));
        }
        return Ok(false);
    }
    if target_exists {
        if source_exists && !target_was_published {
            // A cross-volume publish can complete immediately before the
            // target phase marker is written. Both trees are then expected to
            // exist and the copying phase is the durable proof that this is a
            // recovery of our own operation rather than a pre-existing target.
            if !copying_phase {
                return Err(AppError::Conflict(
                    "work-dir relocation target gained conversations before it was published"
                        .into(),
                ));
            }
            compare_tree(&source, &target)?;
            return Ok(true);
        }
        if !source_exists && !target_was_published && !copying_phase {
            return Err(AppError::Conflict(
                "relocation target conversations were not published by this operation".into(),
            ));
        }
        if source_exists {
            // The only supported both-present state is a verified cross-volume
            // copy (or a crash after its publish). A pre-existing target was
            // rejected when the plan was created, so mismatched trees fail.
            compare_tree(&source, &target)?;
        }
        if !source_exists && !target_was_published && !plan.source_conversations_present {
            return Err(AppError::Conflict(
                "work-dir relocation target gained conversations, but the source had no managed tree"
                    .into(),
            ));
        }
        if !source_exists && backup_exists {
            // A crash may happen after the cross-volume source archive is
            // published but before the final phase marker. The backup is the
            // durable source of truth in that state; verify the target before
            // allowing the retry to finish and retain its visible path.
            compare_tree(&target, &backup)?;
        }
        if staging_exists {
            compare_tree(&target, &staging)?;
            remove_managed_tree(&staging, "completed relocation staging tree")?;
        }
        return Ok(source_exists || backup_exists);
    }
    if staging_exists {
        if !source_exists {
            return Err(AppError::Conflict(
                "work-dir relocation staging exists but the source tree is missing"
                    .into(),
            ));
        }
        // A process can die after creating the staging root or part of its
        // contents. It is safe to discard exactly this operation-owned path,
        // copy once more, and verify the complete tree. A second mismatch is
        // returned to the caller; there is deliberately no retry loop.
        if compare_tree(&source, &staging).is_err() {
            remove_managed_tree(&staging, "incomplete relocation staging tree")?;
            copy_tree(&source, &staging)?;
            compare_tree(&source, &staging)?;
        }
        fs::rename(&staging, &target).map_err(|error| {
            AppError::Internal(format!("publish relocation staging tree: {error}"))
        })?;
        return Ok(true);
    }
    match fs::rename(&source, &target) {
        Ok(()) => Ok(false),
        Err(error) if is_cross_device(&error) => {
            fs::create_dir(&staging).map_err(|error| {
                AppError::Internal(format!("create relocation staging tree: {error}"))
            })?;
            copy_tree(&source, &staging)?;
            compare_tree(&source, &staging)?;
            fs::rename(&staging, &target).map_err(|error| {
                AppError::Internal(format!("publish cross-volume relocation tree: {error}"))
            })?;
            Ok(true)
        }
        Err(error) => Err(AppError::Internal(format!(
            "move managed conversations {} -> {}: {error}",
            source.display(),
            target.display()
        ))),
    }
}

fn preserve_cross_volume_source(
    plan: &WorkDirRelocationPlan,
    cross_volume_copy: bool,
) -> Result<Option<PathBuf>, AppError> {
    if !cross_volume_copy {
        return Ok(None);
    }
    let source = Path::new(&plan.source_work_dir).join(CONVERSATIONS_DIR);
    if !entry_exists(&source, "relocation source conversations")? {
        return Ok(Some(backup_path(
            Path::new(&plan.source_work_dir),
            &plan.operation_id,
        )));
    }
    let backup = backup_path(Path::new(&plan.source_work_dir), &plan.operation_id);
    let backup_parent = backup.parent().ok_or_else(|| {
        AppError::Internal("relocation backup path has no parent".into())
    })?;
    ensure_real_directory_tree(Path::new(&plan.source_work_dir), "relocation source work root")?;
    let backup_root = Path::new(&plan.source_work_dir).join(BACKUP_DIR);
    match fs::symlink_metadata(&backup_root) {
        Ok(_) => ensure_real_directory(&backup_root, "relocation backup directory")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&backup_root).map_err(|error| {
                AppError::Internal(format!("create relocation backup directory: {error}"))
            })?;
        }
        Err(error) => {
            return Err(AppError::Internal(format!(
                "inspect relocation backup directory {}: {error}",
                backup_root.display()
            )));
        }
    }
    let operation_dir = backup_parent;
    match fs::symlink_metadata(operation_dir) {
        Ok(_) => ensure_real_directory(operation_dir, "relocation operation backup directory")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(operation_dir).map_err(|error| {
                AppError::Internal(format!("create relocation operation backup directory: {error}"))
            })?;
        }
        Err(error) => {
            return Err(AppError::Internal(format!(
                "inspect relocation operation backup directory {}: {error}",
                operation_dir.display()
            )));
        }
    }
    match fs::symlink_metadata(&backup) {
        Ok(_) => {
            compare_tree(&source, &backup)?;
            // A previous failed attempt may have restored the source from
            // this already verified backup. Keep the backup as the rollback
            // copy, then remove the duplicate source before completing the
            // retry.
            fs::remove_dir_all(&source).map_err(|error| {
                AppError::Internal(format!(
                    "remove restored source conversations after backup verification: {error}"
                ))
            })?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::rename(&source, &backup).map_err(|error| {
                AppError::Internal(format!("archive old conversations tree: {error}"))
            })?;
        }
        Err(error) => {
            return Err(AppError::Internal(format!("inspect relocation backup: {error}")));
        }
    }
    if !entry_exists(&backup, "relocation backup tree")? {
        return Err(AppError::Internal(format!(
            "cross-volume relocation backup was not published: {}",
            backup.display()
        )));
    }
    Ok(Some(backup))
}

fn cleanup_pending_state(data_dir: &Path) -> Result<(), AppError> {
    for phase in RelocationPhase::ALL {
        let path = phase_path(data_dir, phase);
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(AppError::Internal(format!(
                    "remove completed work-dir relocation phase {}: {error}",
                    path.display()
                )));
            }
        }
    }
    match fs::remove_file(plan_path(data_dir)) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(AppError::Internal(format!(
                "remove completed work-dir relocation plan: {error}"
            )));
        }
    }
    match fs::remove_dir(pending_dir(data_dir)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => Ok(()),
        Err(error) => Err(AppError::Internal(format!(
            "remove completed work-dir relocation state directory: {error}"
        ))),
    }
}

fn rollback_conversations_tree(plan: &WorkDirRelocationPlan) -> Result<(), AppError> {
    if !plan.source_conversations_present {
        return Ok(());
    }
    let source_root = Path::new(&plan.source_work_dir);
    let target_root = Path::new(&plan.target_work_dir);
    let source = source_root.join(CONVERSATIONS_DIR);
    let target = target_root.join(CONVERSATIONS_DIR);
    let backup = backup_path(source_root, &plan.operation_id);
    if entry_exists(&source, "relocation source conversations")? {
        return Ok(());
    }
    if entry_exists(&backup, "relocation backup tree")? {
        fs::rename(&backup, &source).map_err(|error| {
            AppError::Internal(format!(
                "restore old conversations backup {} -> {}: {error}",
                backup.display(),
                source.display()
            ))
        })?;
        return Ok(());
    }
    if entry_exists(&target, "relocation target conversations")? {
        fs::rename(&target, &source).map_err(|error| {
            AppError::Internal(format!(
                "restore old conversations tree {} -> {}: {error}",
                target.display(),
                source.display()
            ))
        })?;
    }
    Ok(())
}

fn rollback_rebound_markers(plan: &WorkDirRelocationPlan) {
    let data_dir = Path::new(&plan.data_dir);
    let source = Path::new(&plan.source_work_dir);
    let target = Path::new(&plan.target_work_dir);
    // Rebind unconditionally when possible. A failure can occur after the
    // data-side binding was published but before the receipt was written, so
    // checking only for a current target receipt would leave split markers.
    if let Err(error) = factory::rebind_v3_dataset_work_root(
        data_dir,
        target,
        source,
        &plan.generation,
    ) {
        tracing::warn!(
            target: "work_dir_relocation",
            error = %error,
            "could not roll back work-root lifecycle markers"
        );
    }
    if let Err(error) = crate::dir_config::set_work_dir(data_dir, source) {
        tracing::warn!(
            target: "work_dir_relocation",
            error = %error,
            "could not roll back persisted work-dir pointer"
        );
    }
    if let Err(error) = factory::finish_v3_dataset_work_root_relocation(
        data_dir,
        target,
        source,
        &plan.generation,
    ) {
        tracing::warn!(
            target: "work_dir_relocation",
            error = %error,
            "could not finish rollback of the target owner marker"
        );
    }
}

/// Consume one pending relocation during startup. Any failure is recorded and
/// returned to the caller, which deliberately continues booting on the old
/// work root rather than allowing the ordinary v3 reset gate to run.
pub fn consume_pending_relocation(
    data_dir: &Path,
) -> Result<Option<(WorkDirRelocationPlan, Option<PathBuf>)>, AppError> {
    let Some(plan) = read_pending_plan(data_dir)? else {
        return Ok(None);
    };
    let result = consume_pending_relocation_inner(data_dir, &plan);
    match result {
        Ok(result) => Ok(Some(result)),
        Err(error) => {
            if let Err(rollback_error) = rollback_conversations_tree(&plan) {
                tracing::error!(
                    target: "work_dir_relocation",
                    error = %rollback_error,
                    "work-dir relocation failed and conversation rollback was incomplete"
                );
            }
            rollback_rebound_markers(&plan);
            let _ = write_status(data_dir, &failed_status(&plan, &error, None));
            Err(error)
        }
    }
}

fn consume_pending_relocation_inner(
    data_dir: &Path,
    plan: &WorkDirRelocationPlan,
) -> Result<(WorkDirRelocationPlan, Option<PathBuf>), AppError> {
    validate_plan(plan, data_dir)?;
    let generation = read_storage_generation(data_dir)?;
    if generation != plan.generation {
        return Err(AppError::Conflict(
            "work-dir relocation generation differs from storage-generation".into(),
        ));
    }
    let source = Path::new(&plan.source_work_dir);
    let target = Path::new(&plan.target_work_dir);
    ensure_real_directory_tree(source, "relocation source work root")?;
    ensure_real_directory_tree(target, "relocation target work root")?;
    let persisted_work_dir = crate::dir_config::checked_persisted_work_dir(data_dir)?;
    if let Some(persisted) = &persisted_work_dir
        && !paths_equivalent(persisted, source)
        && !paths_equivalent(persisted, target)
    {
        return Err(AppError::Conflict(
            "dir-config points to a third work root during relocation".into(),
        ));
    }
    let source_status = factory::inspect_v3_dataset_receipt(data_dir, source)?;
    let target_status = factory::inspect_v3_dataset_receipt(data_dir, target)?;
    if source_status != DatasetReceiptStatus::Current
        && target_status != DatasetReceiptStatus::Current
    {
        return Err(AppError::Conflict(
            "work-dir relocation requires the existing finalized v3 dataset".into(),
        ));
    }

    write_phase(data_dir, plan, RelocationPhase::Copying)?;
    let cross_volume_copy = publish_conversations(data_dir, plan)?;
    write_phase(data_dir, plan, RelocationPhase::TargetPublished)?;

    if target_status != DatasetReceiptStatus::Current {
        factory::rebind_v3_dataset_work_root(
            data_dir,
            source,
            target,
            &plan.generation,
        )?;
        crate::dir_config::set_work_dir(data_dir, target)?;
    } else if persisted_work_dir
        .as_deref()
        .is_none_or(|persisted| !paths_equivalent(persisted, target))
    {
        crate::dir_config::set_work_dir(data_dir, target)?;
    }
    write_phase(data_dir, plan, RelocationPhase::BindingsRebound)?;

    let rollback_copy = preserve_cross_volume_source(plan, cross_volume_copy)?;
    factory::finish_v3_dataset_work_root_relocation(
        data_dir,
        source,
        target,
        &plan.generation,
    )?;
    if let Err(error) = write_phase(data_dir, plan, RelocationPhase::Completed) {
        tracing::warn!(
            target: "work_dir_relocation",
            error = %error,
            "work-dir relocation completed but its final phase marker could not be written"
        );
    }
    if let Err(error) = write_status(data_dir, &completed_status(plan, rollback_copy.clone())) {
        tracing::warn!(
            target: "work_dir_relocation",
            error = %error,
            "work-dir relocation completed but its result status could not be written"
        );
    }
    if let Err(error) = cleanup_pending_state(data_dir) {
        tracing::warn!(
            target: "work_dir_relocation",
            error = %error,
            "work-dir relocation completed but pending control files could not be cleaned"
        );
    }
    Ok((plan.clone(), rollback_copy))
}

#[cfg(windows)]
fn metadata_is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn metadata_is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn unsupported_reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        return metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            && !metadata.file_type().is_symlink();
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}

fn create_link(source: &Path, destination: &Path) -> Result<(), AppError> {
    let link_target = fs::read_link(source).map_err(|error| {
        AppError::Internal(format!("read relocation symlink {}: {error}", source.display()))
    })?;
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&link_target, destination).map_err(|error| {
            AppError::Internal(format!("create relocation symlink {}: {error}", destination.display()))
        })?;
    }
    #[cfg(windows)]
    {
        let target_is_dir = fs::metadata(source).map(|metadata| metadata.is_dir()).unwrap_or(false);
        let result = if target_is_dir {
            std::os::windows::fs::symlink_dir(&link_target, destination)
        } else {
            std::os::windows::fs::symlink_file(&link_target, destination)
        };
        result.map_err(|error| {
            AppError::Internal(format!("create relocation symlink {}: {error}", destination.display()))
        })?;
    }
    Ok(())
}

fn is_cross_device(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(18) | Some(17))
        || error.kind() == std::io::ErrorKind::CrossesDevices
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_dataset(data: &Path, work: &Path) -> String {
        fs::create_dir_all(data).unwrap();
        fs::create_dir_all(work.join(CONVERSATIONS_DIR)).unwrap();
        fs::write(work.join(CONVERSATIONS_DIR).join("history.txt"), b"hello").unwrap();
        let generation = Uuid::now_v7().to_string();
        fs::write(data.join("storage-generation"), &generation).unwrap();
        fs::write(data.join(crate::storage_paths::DATABASE_FILE), b"sqlite-sentinel").unwrap();
        factory::write_v3_dataset_receipt_for_work_dir(data, work, &generation).unwrap();
        generation
    }

    #[test]
    fn request_writes_plan_without_reset_request() {
        let data = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let generation = seed_dataset(data.path(), source.path());
        let plan = request_work_dir_relocation(data.path(), source.path(), target.path()).unwrap();
        assert_eq!(plan.generation, generation);
        assert!(plan_path(data.path()).is_file());
        assert!(phase_path(data.path(), RelocationPhase::Requested).is_file());
        assert!(!data.path().join(factory::V3_DATASET_RESET_REQUEST_FILE).exists());
        assert!(source.path().join(CONVERSATIONS_DIR).join("history.txt").is_file());
    }

    #[test]
    fn rejects_nested_source_and_target_in_both_directions() {
        let data = tempfile::tempdir().unwrap();
        let outer = tempfile::tempdir().unwrap();
        let source = outer.path().join("source");
        let nested_target = source.join("nested-target");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&nested_target).unwrap();
        seed_dataset(data.path(), &source);
        assert!(request_work_dir_relocation(data.path(), &source, &nested_target).is_err());

        let reverse_root = tempfile::tempdir().unwrap();
        let reverse_target = reverse_root.path().join("target");
        let reverse_source = reverse_target.join("nested-source");
        fs::create_dir_all(&reverse_source).unwrap();
        let reverse_data = tempfile::tempdir().unwrap();
        seed_dataset(reverse_data.path(), &reverse_source);
        assert!(request_work_dir_relocation(
            reverse_data.path(),
            &reverse_source,
            reverse_root.path()
        )
        .is_err());
    }

    #[test]
    fn missing_source_conversations_fails_without_rebinding_old_root() {
        let data = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        seed_dataset(data.path(), source.path());
        request_work_dir_relocation(data.path(), source.path(), target.path()).unwrap();
        fs::remove_dir_all(source.path().join(CONVERSATIONS_DIR)).unwrap();

        assert!(consume_pending_relocation(data.path()).is_err());
        assert_eq!(
            factory::finalized_v3_work_dir(data.path()).unwrap().unwrap(),
            simplified(&fs::canonicalize(source.path()).unwrap())
        );
        assert_eq!(
            read_last_status(data.path()).unwrap().unwrap().state,
            WorkDirRelocationState::Failed
        );
    }

    #[test]
    fn tampered_pending_plan_rejects_nested_roots_at_consumption() {
        let data = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        seed_dataset(data.path(), source.path());
        let plan = request_work_dir_relocation(data.path(), source.path(), target.path()).unwrap();
        let nested = source.path().join("tampered-target");
        fs::create_dir(&nested).unwrap();
        let mut tampered = plan.clone();
        tampered.target_work_dir = nested.display().to_string();
        let bytes = serde_json::to_vec_pretty(&tampered).unwrap();
        crate::dir_config::write_atomic_replace(&plan_path(data.path()), &bytes).unwrap();

        assert!(read_pending_plan(data.path()).is_err());
        assert_eq!(
            factory::finalized_v3_work_dir(data.path()).unwrap().unwrap(),
            simplified(&fs::canonicalize(source.path()).unwrap())
        );
    }

    #[test]
    fn empty_source_conversations_can_relocate_as_an_empty_work_root() {
        let data = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let generation = seed_dataset(data.path(), source.path());
        fs::remove_dir_all(source.path().join(CONVERSATIONS_DIR)).unwrap();

        let plan = request_work_dir_relocation(data.path(), source.path(), target.path()).unwrap();
        assert!(!plan.source_conversations_present);
        consume_pending_relocation(data.path()).unwrap().unwrap();
        assert!(!target.path().join(CONVERSATIONS_DIR).exists());
        assert_eq!(
            fs::read_to_string(data.path().join("storage-generation")).unwrap(),
            generation
        );
        assert_eq!(
            factory::finalized_v3_work_dir(data.path()).unwrap().unwrap(),
            simplified(&fs::canonicalize(target.path()).unwrap())
        );
    }

    #[test]
    fn incomplete_staging_is_rebuilt_once_before_publish() {
        let data = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        seed_dataset(data.path(), source.path());
        let plan = request_work_dir_relocation(data.path(), source.path(), target.path()).unwrap();
        let staging = staging_path(target.path(), &plan.operation_id);
        fs::create_dir(&staging).unwrap();
        fs::write(staging.join("history.txt"), b"partial").unwrap();
        write_phase(data.path(), &plan, RelocationPhase::Copying).unwrap();

        consume_pending_relocation(data.path()).unwrap().unwrap();
        assert_eq!(
            fs::read(target.path().join(CONVERSATIONS_DIR).join("history.txt")).unwrap(),
            b"hello"
        );
    }

    #[test]
    fn same_volume_consume_preserves_generation_and_moves_conversations() {
        let data = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let generation = seed_dataset(data.path(), source.path());
        let database_before = fs::read(data.path().join(crate::storage_paths::DATABASE_FILE)).unwrap();
        fs::create_dir(data.path().join("logs")).unwrap();
        fs::write(data.path().join("logs").join("app.log"), b"keep-log").unwrap();
        fs::write(data.path().join("installation-preferences.json"), b"{\"language\":\"zh-CN\"}").unwrap();
        fs::write(data.path().join("encryption_key"), b"keep-key").unwrap();
        fs::write(source.path().join("user-file.txt"), b"keep-user-file").unwrap();
        let external_workspace = tempfile::tempdir().unwrap();
        fs::write(external_workspace.path().join("keep.txt"), b"external").unwrap();
        request_work_dir_relocation(data.path(), source.path(), target.path()).unwrap();
        let result = consume_pending_relocation(data.path()).unwrap().unwrap();
        assert!(result.1.is_none());
        assert_eq!(fs::read_to_string(data.path().join("storage-generation")).unwrap(), generation);
        assert_eq!(fs::read(data.path().join(crate::storage_paths::DATABASE_FILE)).unwrap(), database_before);
        assert_eq!(fs::read(data.path().join("logs").join("app.log")).unwrap(), b"keep-log");
        assert_eq!(fs::read(data.path().join("installation-preferences.json")).unwrap(), b"{\"language\":\"zh-CN\"}");
        assert_eq!(fs::read(data.path().join("encryption_key")).unwrap(), b"keep-key");
        assert!(!source.path().join(CONVERSATIONS_DIR).exists());
        assert_eq!(fs::read(source.path().join("user-file.txt")).unwrap(), b"keep-user-file");
        assert!(target.path().join(CONVERSATIONS_DIR).join("history.txt").is_file());
        assert_eq!(fs::read(external_workspace.path().join("keep.txt")).unwrap(), b"external");
        assert_eq!(
            factory::finalized_v3_work_dir(data.path()).unwrap().unwrap(),
            simplified(&fs::canonicalize(target.path()).unwrap())
        );
        assert!(!source.path().join(crate::dataset_roots::WORK_ROOT_OWNER_FILE).exists());
    }

    #[test]
    fn target_collision_after_request_keeps_old_root_and_records_failure() {
        let data = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let generation = seed_dataset(data.path(), source.path());
        request_work_dir_relocation(data.path(), source.path(), target.path()).unwrap();
        fs::create_dir_all(target.path().join(CONVERSATIONS_DIR)).unwrap();
        fs::write(target.path().join(CONVERSATIONS_DIR).join("foreign.txt"), b"foreign").unwrap();

        assert!(consume_pending_relocation(data.path()).is_err());
        assert!(source.path().join(CONVERSATIONS_DIR).join("history.txt").is_file());
        assert!(target.path().join(CONVERSATIONS_DIR).join("foreign.txt").is_file());
        assert_eq!(fs::read_to_string(data.path().join("storage-generation")).unwrap(), generation);
        assert_eq!(factory::finalized_v3_work_dir(data.path()).unwrap().unwrap(), simplified(&fs::canonicalize(source.path()).unwrap()));
        let status = read_last_status(data.path()).unwrap().unwrap();
        assert_eq!(status.state, WorkDirRelocationState::Failed);
    }

    #[test]
    fn legacy_work_dir_reset_request_is_converted_without_rotating_generation() {
        let data = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let generation = seed_dataset(data.path(), source.path());
        factory::write_legacy_work_dir_change_request_for_test(
            data.path(),
            target.path(),
        )
        .unwrap();

        let plan = convert_legacy_work_dir_change_request(data.path()).unwrap().unwrap();
        assert_eq!(plan.generation, generation);
        assert!(!data.path().join(factory::V3_DATASET_RESET_REQUEST_FILE).exists());
        assert!(data.path().join(PENDING_RELOCATION_DIR).join(RELOCATION_PLAN_FILE).is_file());
        consume_pending_relocation(data.path()).unwrap().unwrap();
        assert!(target.path().join(CONVERSATIONS_DIR).join("history.txt").is_file());
        assert_eq!(factory::finalized_v3_work_dir(data.path()).unwrap().unwrap(), simplified(&fs::canonicalize(target.path()).unwrap()));
    }

    #[test]
    fn target_collision_is_rejected_before_plan_write() {
        let data = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        fs::create_dir_all(target.path().join(CONVERSATIONS_DIR)).unwrap();
        seed_dataset(data.path(), source.path());
        assert!(request_work_dir_relocation(data.path(), source.path(), target.path()).is_err());
        assert!(!plan_path(data.path()).exists());
    }
}

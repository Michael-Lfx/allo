//! The first local deliverable: a bounded, read-only directory listing.

use std::fs::{self, Metadata, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use flowy_agent_core::tools::{ToolDefinition, ToolRegistry};
use flowy_agent_core::{CancelToken, ToolExecutionResult};
use serde::{Deserialize, Serialize};

use crate::index::save_json;

const FILE_NAME: &str = "inventory.md";
const ENTRY_LIMIT: usize = 1000;
const OUTPUT_LIMIT: usize = 512 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryStatus {
    WaitingForFolder,
    Running,
    Paused,
    NeedsReview,
    Delivered,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Inventory {
    pub id: String,
    pub origin: Option<String>,
    pub goal: String,
    pub folder: Option<PathBuf>,
    pub status: InventoryStatus,
    pub attempt: u64,
    /// Expected bytes are saved before attempting to create the output.
    expected: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InventorySummary {
    pub goal: String,
    pub folder: Option<PathBuf>,
    pub status: InventoryStatus,
}

impl From<Inventory> for InventorySummary {
    fn from(task: Inventory) -> Self {
        Self {
            goal: task.goal,
            folder: task.folder,
            status: task.status,
        }
    }
}

pub fn task_id(submission: u128) -> String {
    format!("inventory-{submission:032x}")
}

impl Inventory {
    pub const fn new(id: String, origin: Option<String>, goal: String) -> Self {
        Self {
            id,
            origin,
            goal,
            folder: None,
            status: InventoryStatus::WaitingForFolder,
            attempt: 0,
            expected: None,
        }
    }

    pub fn load(root: &Path, id: &str) -> io::Result<Option<Self>> {
        if !valid_id(id) {
            return Ok(None);
        }
        let path = task_dir(root, id).join("task.json");
        checked_storage_path(root, &path)?;
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        let task: Self =
            serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if task.id != id
            || task
                .expected
                .as_ref()
                .is_some_and(|text| text.len() > OUTPUT_LIMIT)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid inventory record",
            ));
        }
        Ok(Some(task))
    }

    pub fn save(&self, root: &Path) -> io::Result<()> {
        let path = task_dir(root, &self.id).join("task.json");
        checked_storage_path(root, &path)?;
        save_json(&path, self)
    }

    pub fn ids(root: &Path) -> io::Result<Vec<String>> {
        let dir = root.join("inventories");
        checked_storage_path(root, &dir)?;
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Vec::new());
            }
            Err(error) => return Err(error),
        };
        let mut ids = Vec::new();
        for entry in entries {
            let name = entry?.file_name();
            if let Some(id) = name.to_str().filter(|id| valid_id(id)) {
                ids.push(id.to_owned());
            }
        }
        Ok(ids)
    }

    pub fn set_folder(&mut self, folder: &Path) -> io::Result<()> {
        checked_path(folder)?;
        let folder = fs::canonicalize(folder)?;
        if !fs::symlink_metadata(&folder)?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected a directory",
            ));
        }
        self.folder = Some(folder);
        Ok(())
    }

    pub fn reconcile(&mut self, root: &Path) -> io::Result<()> {
        if self.status != InventoryStatus::Running {
            return Ok(());
        }
        self.status = match self.verify_output(root) {
            Ok(Some(_)) => InventoryStatus::Delivered,
            Ok(None) => InventoryStatus::Paused,
            Err(_) => InventoryStatus::NeedsReview,
        };
        self.save(root)
    }

    pub fn finish(
        &mut self,
        root: &Path,
        listing: Option<String>,
    ) -> io::Result<()> {
        let Some(listing) = listing else {
            self.status = InventoryStatus::Failed;
            return self.save(root);
        };
        self.expected = Some(listing);
        self.save(root)?;
        let output = task_dir(root, &self.id).join(FILE_NAME);
        let created = (|| -> io::Result<()> {
            checked_storage_path(root, &output)?;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&output)?;
            file.write_all(
                self.expected.as_deref().unwrap_or_default().as_bytes(),
            )?;
            file.sync_all()
        })();
        if created.is_ok() {
            return self.reconcile(root);
        }
        self.status = if matches!(self.verify_output(root), Ok(None)) {
            InventoryStatus::Paused
        } else {
            InventoryStatus::NeedsReview
        };
        self.save(root)?;
        created
    }

    pub fn artifact(&self, root: &Path) -> io::Result<Option<PathBuf>> {
        if self.status == InventoryStatus::Delivered {
            self.verify_output(root)
        } else {
            Ok(None)
        }
    }

    fn verify_output(&self, root: &Path) -> io::Result<Option<PathBuf>> {
        let path = task_dir(root, &self.id).join(FILE_NAME);
        checked_storage_path(root, &path)?;
        let info = match fs::symlink_metadata(&path) {
            Ok(info) => info,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        let expected = self.expected.as_deref().filter(|text| !text.is_empty());
        if let Some(text) = expected
            && info.is_file()
            && info.len() == text.len() as u64
            && fs::read(&path)? == text.as_bytes()
        {
            return Ok(Some(path));
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unverified inventory output",
        ))
    }
}

fn task_dir(root: &Path, id: &str) -> PathBuf {
    root.join("inventories").join(id)
}

pub fn valid_id(id: &str) -> bool {
    id.strip_prefix("inventory-").is_some_and(|tail| {
        tail.len() == 32 && tail.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn is_link(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        const REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn checked_path(path: &Path) -> io::Result<()> {
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "relative traversal",
        ));
    }
    for part in path.ancestors().filter(|part| part.file_name().is_some()) {
        reject_link(part)?;
    }
    Ok(())
}

fn checked_storage_path(root: &Path, path: &Path) -> io::Result<()> {
    // The host's data root can be redirected by the OS or user profile.
    // Check each managed component before probing anything beneath it.
    let relative = path.strip_prefix(root).map_err(|_| {
        io::Error::new(io::ErrorKind::PermissionDenied, "outside data root")
    })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "relative traversal",
            ));
        };
        current.push(name);
        reject_link(&current)?;
    }
    Ok(())
}

fn reject_link(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(info) if is_link(&info) => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "linked path",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn scan(folder: &Path, signal: &CancelToken) -> io::Result<String> {
    use std::fmt::Write as _;

    checked_path(folder)?;
    let mut entries = Vec::new();
    for entry in fs::read_dir(folder)? {
        if signal.is_cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "cancelled",
            ));
        }
        if entries.len() == ENTRY_LIMIT {
            return Err(io::Error::other(
                "directory has more than 1000 entries",
            ));
        }
        let entry = entry?;
        let info = fs::symlink_metadata(entry.path())?;
        let kind = if is_link(&info) {
            "link (not followed)"
        } else if info.is_dir() {
            "directory"
        } else {
            "file"
        };
        let name = entry.file_name();
        entries.push((name, kind));
    }
    entries.sort_unstable();
    let mut text = String::from("# Directory inventory\n\n");
    writeln!(text, "{} entries (top level)\n", entries.len()).unwrap();
    for (name, kind) in entries {
        text.push_str("- ");
        for char in name.to_string_lossy().chars() {
            if char.is_control() {
                text.extend(char.escape_default());
            } else {
                if char.is_ascii_punctuation() {
                    text.push('\\');
                }
                text.push(char);
            }
        }
        writeln!(text, " ({kind})").unwrap();
        if text.len() > OUTPUT_LIMIT {
            return Err(io::Error::other("inventory exceeds the output limit"));
        }
    }
    Ok(text)
}

pub fn inventory_tool(
    folder: PathBuf,
    listing: Arc<Mutex<Option<String>>>,
) -> ToolRegistry {
    let registry = ToolRegistry::new();
    registry.register(Arc::new(ToolDefinition::new(
        "inventory_selected_folder",
        "Collect a top-level inventory of the user-selected folder. Reads names and types only. Paths and tool arguments are controlled by the host.",
        serde_json::json!({"type": "object", "properties": {}, "additionalProperties": false}),
        move |_, ctx| {
            let folder = folder.clone();
            let listing = Arc::clone(&listing);
            Box::pin(async move {
                match tokio::task::spawn_blocking(move || scan(&folder, &ctx.signal)).await {
                    Ok(Ok(text)) => {
                        *listing.lock().unwrap() = Some(text);
                        ToolExecutionResult::success_text("Directory inventoried by the host.", serde_json::Value::Null)
                    }
                    Ok(Err(error)) => ToolExecutionResult::error("INVENTORY_FAILED", error.to_string()),
                    Err(error) => ToolExecutionResult::error("INVENTORY_FAILED", error.to_string()),
                }
            })
        },
    )));
    registry
}

#[cfg(test)]
mod tests {
    use super::{FILE_NAME, Inventory, InventoryStatus, fs, task_dir, task_id};

    #[cfg(windows)]
    fn create_junction(link: &std::path::Path, target: &std::path::Path) {
        let output = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$ErrorActionPreference = 'Stop'; New-Item -ItemType Junction -Path $env:FLOWY_TEST_LINK -Target $env:FLOWY_TEST_TARGET | Out-Null",
            ])
            .env("FLOWY_TEST_LINK", link)
            .env("FLOWY_TEST_TARGET", target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "create junction: {}",
            String::from_utf8_lossy(&output.stderr),
        );
    }

    #[cfg(windows)]
    #[test]
    fn accepts_redirected_data_directory_but_rejects_managed_junctions() {
        use std::io::ErrorKind;

        use crate::{AgentHandle, AgentUpdate, LlmConfig};

        let sandbox = std::env::temp_dir().join(format!(
            "flowy-inventory-junctions-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let profile = sandbox.join("profile");
        let target = sandbox.join("redirected");
        fs::create_dir_all(&target).unwrap();
        create_junction(&profile, &target);

        let root = profile.join("data");
        assert_eq!(Inventory::ids(&root).unwrap(), Vec::<String>::new());
        let id = task_id(1);
        let mut task = Inventory::new(id.clone(), None, "inventory".into());
        task.status = InventoryStatus::Running;
        let listing = "# Directory inventory\n";
        task.finish(&root, Some(listing.into())).unwrap();
        assert_eq!(
            Inventory::ids(&root).unwrap().as_slice(),
            std::slice::from_ref(&id)
        );
        let reopened = Inventory::load(&root, &id).unwrap().unwrap();
        assert_eq!(reopened.status, InventoryStatus::Delivered);
        let output = reopened.artifact(&root).unwrap().unwrap();
        assert_eq!(fs::read_to_string(&output).unwrap(), listing);
        {
            let handle =
                AgentHandle::with_config(root.clone(), LlmConfig::default());
            assert!(
                !handle
                    .drain_updates()
                    .iter()
                    .any(|update| matches!(update, AgentUpdate::StorageError))
            );
            assert!(
                handle
                    .list_sessions()
                    .iter()
                    .any(|session| session.id == id)
            );
        }

        let task_path = task_dir(&root, &id);
        let moved_task = sandbox.join("moved-task");
        fs::rename(&task_path, &moved_task).unwrap();
        create_junction(&task_path, &moved_task);
        assert_eq!(
            Inventory::load(&root, &id).unwrap_err().kind(),
            ErrorKind::PermissionDenied
        );
        assert_eq!(
            reopened.save(&root).unwrap_err().kind(),
            ErrorKind::PermissionDenied
        );
        assert_eq!(
            reopened.artifact(&root).unwrap_err().kind(),
            ErrorKind::PermissionDenied
        );
        assert_eq!(
            fs::read_to_string(moved_task.join(FILE_NAME)).unwrap(),
            listing
        );
        fs::remove_dir(task_path).unwrap();

        let inventories = root.join("inventories");
        let moved_inventories = sandbox.join("moved-inventories");
        fs::rename(&inventories, &moved_inventories).unwrap();
        create_junction(&inventories, &moved_inventories);
        assert_eq!(
            Inventory::ids(&root).unwrap_err().kind(),
            ErrorKind::PermissionDenied
        );
        fs::remove_dir(inventories).unwrap();
        fs::remove_dir(profile).unwrap();
        fs::remove_dir_all(sandbox).unwrap();
    }

    #[test]
    fn recovery_claims_verified_bytes_only_and_preserves_unknown_output() {
        let root = std::env::temp_dir()
            .join(format!("flowy-inventory-recovery-{}", std::process::id()));
        for (submission, bytes, expected_status) in [
            (1, b"# Known\n".as_slice(), InventoryStatus::Delivered),
            (2, b"".as_slice(), InventoryStatus::NeedsReview),
        ] {
            let id = task_id(submission);
            let mut task = Inventory::new(id.clone(), None, "inventory".into());
            task.status = InventoryStatus::Running;
            task.expected = Some("# Known\n".into());
            task.save(&root).unwrap();
            let output = task_dir(&root, &id).join(FILE_NAME);
            fs::write(&output, bytes).unwrap();
            let mut reopened = Inventory::load(&root, &id).unwrap().unwrap();
            reopened.reconcile(&root).unwrap();
            assert_eq!(reopened.status, expected_status);
            assert_eq!(fs::read(&output).unwrap(), bytes);
            assert_eq!(
                reopened.artifact(&root).unwrap().is_some(),
                submission == 1
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}

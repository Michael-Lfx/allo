use super::{AgentHandle, lock};
use flowy_agent_core::{
    ExecutionOwner, ExecutionSource, SessionIndex, SessionOrigin,
};
use flowy_automation::retention::RetentionUsage;
use flowy_automation::store::AttemptRecord;
use flowy_automation::{PortError, PrincipalId, RunSpec};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const OWNERSHIP_FILE: &str = ".flowy-automation-workspace";

pub(super) fn initialize_owned_workspace(
    parent: &Path,
    id: &str,
) -> io::Result<PathBuf> {
    let path = parent.join(id);
    match fs::create_dir(&path) {
        Ok(()) => {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path.join(OWNERSHIP_FILE))?;
            io::Write::write_all(&mut file, id.as_bytes())?;
            file.sync_all()?;
        }
        // An interrupted create can resume only in the directory the host marked.
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            check_workspace_owner(&path, id)?;
        }
        Err(error) => return Err(error),
    }
    Ok(path)
}

fn check_workspace_owner(path: &Path, id: &str) -> io::Result<()> {
    let dir = fs::symlink_metadata(path)?;
    let marker = path.join(OWNERSHIP_FILE);
    let file = fs::symlink_metadata(&marker)?;
    if !dir.is_dir()
        || redirected(&dir)
        || !file.is_file()
        || redirected(&file)
        || file.len() != id.len() as u64
        || fs::read(marker)? != id.as_bytes()
    {
        return Err(io::Error::other("automation workspace ownership changed"));
    }
    Ok(())
}

fn remove_owned_workspace(path: &Path, id: &str) -> Result<(), PortError> {
    check_workspace_owner(path, id).map_err(|_| PortError::Storage)?;
    tree_bytes(path)?;
    for entry in fs::read_dir(path).map_err(|_| PortError::Storage)? {
        let entry = entry.map_err(|_| PortError::Storage)?;
        if entry.file_name() == OWNERSHIP_FILE {
            continue;
        }
        let target = entry.path();
        let metadata =
            fs::symlink_metadata(&target).map_err(|_| PortError::Storage)?;
        if redirected(&metadata) {
            return Err(PortError::Storage);
        }
        let result = if metadata.is_dir() {
            fs::remove_dir_all(target)
        } else {
            fs::remove_file(target)
        };
        result.map_err(|_| PortError::Storage)?;
    }
    check_workspace_owner(path, id).map_err(|_| PortError::Storage)?;
    fs::remove_file(path.join(OWNERSHIP_FILE))
        .map_err(|_| PortError::Storage)?;
    if fs::remove_dir(path).is_err() {
        // Retain proof for a retry when a platform handle still occupies the directory.
        let _ = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path.join(OWNERSHIP_FILE))
            .and_then(|mut file| {
                io::Write::write_all(&mut file, id.as_bytes())?;
                file.sync_all()
            });
        return Err(PortError::Storage);
    }
    Ok(())
}

impl AgentHandle {
    pub fn automation_storage_usage(
        &self,
        principal: &PrincipalId,
        attempts: &[AttemptRecord],
    ) -> Result<RetentionUsage, PortError> {
        if self.initialization_state() != crate::AgentInitialization::Ready {
            return Err(PortError::Storage);
        }
        let engine =
            lock(&self.inner.engine).clone().ok_or(PortError::Storage)?;
        let root = &self.inner.state.root;
        let owner = ExecutionOwner {
            authority: principal.authority().to_owned(),
            subject: principal.external_user_id().to_string(),
        };
        let mut usage = RetentionUsage::default();
        engine.with_sessions(|sessions| {
            for record in sessions {
                if record.execution.owner.as_ref() == Some(&owner)
                    && matches!(
                        record.execution.source,
                        ExecutionSource::Automation { .. }
                    )
                {
                    let mut count = ByteCount(0);
                    serde_json::to_writer(&mut count, record)
                        .map_err(|_| PortError::Storage)?;
                    usage.owner_history_bytes =
                        usage.owner_history_bytes.saturating_add(count.0);
                    usage.sessions += 1;
                }
            }
            Ok(())
        })?;
        usage.shared_history_bytes = file_bytes(&SessionIndex::path(root))?
            .saturating_add(file_bytes(
                &root.join("sessions").join("index.json"),
            )?);
        for attempt in attempts {
            if attempt.spec.workspace.is_none()
                && let Some(path) = private_workspace(root, &attempt.spec)?
            {
                usage.private_workspace_bytes = usage
                    .private_workspace_bytes
                    .saturating_add(tree_bytes(&path)?);
            }
        }
        let artifact_usage = self
            .artifact_store()
            .map_err(|_| PortError::Storage)?
            .usage()
            .map_err(|_| PortError::Storage)?;
        usage.artifact_headroom_bytes =
            artifact_usage.max_bytes.saturating_sub(
                artifact_usage
                    .stored_bytes
                    .saturating_add(artifact_usage.staging_bytes)
                    .saturating_add(artifact_usage.view_bytes),
            );
        usage.artifact_headroom_items = artifact_usage
            .max_items
            .saturating_sub(artifact_usage.stored_items);
        Ok(usage)
    }

    pub fn automation_retention_protected(
        &self,
        spec: &RunSpec,
    ) -> Result<bool, PortError> {
        let _dispatch = lock(&self.inner.state.dispatch);
        self.automation_retention_protected_locked(spec)
    }

    fn automation_retention_protected_locked(
        &self,
        spec: &RunSpec,
    ) -> Result<bool, PortError> {
        if self.initialization_state() != crate::AgentInitialization::Ready {
            return Err(PortError::Storage);
        }
        let id = attempt_session(spec)?;
        if spec.workspace.is_none()
            && let Some(path) = private_workspace(&self.inner.state.root, spec)?
            && check_workspace_owner(&path, id).is_err()
        {
            return Ok(true);
        }
        let engine =
            lock(&self.inner.engine).clone().ok_or(PortError::Storage)?;
        let record = engine.session(id);
        if let Some(record) = &record {
            if record.execution
                != crate::automation_executor::execution_metadata(spec)
                || (spec.workspace.is_none()
                    && record.origin != SessionOrigin::Home)
            {
                return Err(PortError::Storage);
            }
            if record.status.is_busy()
                || engine.is_active(id)
                || lock(&self.inner.state.active).as_deref() == Some(id)
            {
                return Ok(true);
            }
        }
        let store = self.artifact_store().map_err(|_| PortError::Storage)?;
        store
            .retention_protected(id)
            .map_err(|_| PortError::Storage)
    }

    pub fn remove_automation_result(
        &self,
        spec: &RunSpec,
    ) -> Result<(), PortError> {
        let _dispatch = lock(&self.inner.state.dispatch);
        if self.automation_retention_protected_locked(spec)? {
            return Err(PortError::Unavailable);
        }
        let id = attempt_session(spec)?;
        self.remove_automation_requests(&spec.principal, id)?;
        let engine =
            lock(&self.inner.engine).clone().ok_or(PortError::Storage)?;
        let root = &self.inner.state.root;
        let private = if spec.workspace.is_none() {
            private_workspace(root, spec)?
        } else {
            None
        };
        if let Some(path) = &private {
            check_workspace_owner(path, id).map_err(|_| PortError::Storage)?;
        }
        if let Some(record) = engine.session(id) {
            if let Some(path) = &private
                && record
                    .workspace
                    .as_ref()
                    .is_none_or(|workspace| workspace.root != *path)
            {
                return Err(PortError::Storage);
            }
            self.delete_session_locked(id)
                .map_err(|_| PortError::Storage)?;
        }
        self.artifact_store()
            .map_err(|_| PortError::Storage)?
            .resume_retention_deletion(id)
            .map_err(|_| PortError::Storage)?;
        if let Some(path) = private {
            // Resolve the managed chain again after the separate history commit.
            if private_workspace(root, spec)?.as_ref() != Some(&path) {
                return Err(PortError::Storage);
            }
            remove_owned_workspace(&path, id)?;
        }
        Ok(())
    }
}

fn attempt_session(spec: &RunSpec) -> Result<&str, PortError> {
    let id = spec.session_id.as_ref().ok_or(PortError::Storage)?.as_str();
    if id.strip_prefix("automation-") != Some(spec.attempt_id.as_str()) {
        return Err(PortError::Storage);
    }
    Ok(id)
}

fn private_workspace(
    root: &Path,
    spec: &RunSpec,
) -> Result<Option<PathBuf>, PortError> {
    let id = attempt_session(spec)?;
    let mut path = root.canonicalize().map_err(|_| PortError::Storage)?;
    for component in ["sessions", "workspaces", id] {
        path.push(component);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Ok(metadata) if metadata.is_dir() && !redirected(&metadata) => {}
            _ => return Err(PortError::Storage),
        }
    }
    if path.canonicalize().map_err(|_| PortError::Storage)? != path {
        return Err(PortError::Storage);
    }
    Ok(Some(path))
}

fn file_bytes(path: &Path) -> Result<u64, PortError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !redirected(&metadata) => {
            Ok(metadata.len())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
        _ => Err(PortError::Storage),
    }
}

fn tree_bytes(root: &Path) -> Result<u64, PortError> {
    let mut directories = vec![root.to_path_buf()];
    let mut bytes = 0_u64;
    let mut entries = 0_u32;
    while let Some(path) = directories.pop() {
        for entry in fs::read_dir(path).map_err(|_| PortError::Storage)? {
            entries += 1;
            if entries > 100_000 {
                return Err(PortError::Storage);
            }
            let path = entry.map_err(|_| PortError::Storage)?.path();
            let metadata =
                fs::symlink_metadata(&path).map_err(|_| PortError::Storage)?;
            if redirected(&metadata) {
                return Err(PortError::Storage);
            }
            if metadata.is_dir() {
                directories.push(path);
            } else if metadata.is_file() {
                bytes = bytes.saturating_add(metadata.len());
            }
        }
    }
    Ok(bytes)
}

fn redirected(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

struct ByteCount(u64);

impl io::Write for ByteCount {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len() as u64);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaced_private_workspace_without_ownership_is_preserved() {
        let key = flowy_automation::AttemptId::generate();
        let temp = std::env::temp_dir().join(key.as_str());
        fs::create_dir(&temp).unwrap();
        let id = "automation-fixture";
        let path = initialize_owned_workspace(&temp, id).unwrap();
        let old = temp.join("old-workspace");
        fs::rename(&path, &old).unwrap();
        fs::create_dir(&path).unwrap();
        let user_file = path.join("user.txt");
        fs::write(&user_file, b"keep").unwrap();
        assert_eq!(remove_owned_workspace(&path, id), Err(PortError::Storage));
        assert_eq!(fs::read(user_file).unwrap(), b"keep");
        fs::write(old.join("output.txt"), b"ordinary result").unwrap();
        remove_owned_workspace(&old, id).unwrap();
        assert!(!old.exists());
        fs::remove_dir_all(temp).unwrap();
    }
}

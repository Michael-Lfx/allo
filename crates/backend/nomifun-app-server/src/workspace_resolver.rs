use std::fs;
use std::path::{Path, PathBuf};

use nomifun_common::AppError;

const APP_SERVER_WORKSPACES_DIR: &str = "app-server-workspaces";

/// Resolves an opaque workspace identity to a server-controlled filesystem path.
///
/// Implementations must never interpret the identity as a path. A successful
/// result is an existing, canonical directory that remains below the resolver's
/// configured workspace registry root.
pub trait WorkspaceResolver: Send + Sync {
    fn resolve(&self, workspace_id: &str) -> Result<ResolvedWorkspace, AppError>;

    fn ensure(&self, workspace_id: &str) -> Result<ResolvedWorkspace, AppError>;

    /// Validate a persisted workspace row without interpreting its ID as a path.
    /// Implementations may constrain registered roots to their configured policy.
    fn resolve_registered(
        &self,
        workspace_id: &str,
        root_path: &str,
    ) -> Result<ResolvedWorkspace, AppError>;

    /// Validate a user-supplied local directory path and resolve it to a
    /// canonical, real directory. The App Server registers the result as the
    /// owner's workspace root. Implementations must reject relative paths,
    /// missing targets, symlinks/reparse points and any path that cannot be
    /// canonicalized without changing meaning.
    fn resolve_user_path(&self, path: &str) -> Result<ResolvedWorkspace, AppError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWorkspace {
    workspace_id: String,
    path: PathBuf,
}

impl ResolvedWorkspace {
    pub fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn into_path(self) -> PathBuf {
        self.path
    }
}

/// Filesystem-backed resolver for App Server workspaces.
///
/// Every opaque id maps to `{workspace_root}/app-server-workspaces/{id}`. The
/// id is restricted to a single portable segment, and both the registry root
/// and resolved directory are rejected when they are links or Windows reparse
/// points.
#[derive(Debug, Clone)]
pub struct FilesystemWorkspaceResolver {
    registry_root: PathBuf,
}

impl FilesystemWorkspaceResolver {
    pub fn new(workspace_root: impl AsRef<Path>) -> Result<Self, AppError> {
        let workspace_root = workspace_root.as_ref();
        if !workspace_root.is_absolute() {
            return Err(workspace_denied(
                "configured workspace root must be absolute",
            ));
        }

        fs::create_dir_all(workspace_root)
            .map_err(|_| workspace_denied("configured workspace root is unavailable"))?;
        reject_link_or_reparse(workspace_root, "configured workspace root is not a real directory")?;

        let canonical_root = fs::canonicalize(workspace_root)
            .map_err(|_| workspace_denied("configured workspace root cannot be resolved"))?;
        let registry_root = canonical_root.join(APP_SERVER_WORKSPACES_DIR);
        fs::create_dir_all(&registry_root)
            .map_err(|_| workspace_denied("workspace registry is unavailable"))?;
        reject_link_or_reparse(&registry_root, "workspace registry is not a real directory")?;

        let registry_root = fs::canonicalize(&registry_root)
            .map_err(|_| workspace_denied("workspace registry cannot be resolved"))?;
        Ok(Self { registry_root })
    }

    pub fn registry_root(&self) -> &Path {
        &self.registry_root
    }

    fn candidate(&self, workspace_id: &str) -> Result<PathBuf, AppError> {
        validate_workspace_id(workspace_id)?;
        self.validate_registry_root()?;
        Ok(self.registry_root.join(workspace_id))
    }

    fn validate_registry_root(&self) -> Result<(), AppError> {
        reject_link_or_reparse(&self.registry_root, "workspace registry is not a real directory")?;
        let canonical = fs::canonicalize(&self.registry_root)
            .map_err(|_| workspace_denied("workspace registry cannot be resolved"))?;
        if canonical != self.registry_root {
            return Err(workspace_denied("workspace registry was replaced or retargeted"));
        }
        Ok(())
    }

    fn resolved(&self, workspace_id: &str, candidate: &Path) -> Result<ResolvedWorkspace, AppError> {
        reject_link_or_reparse(candidate, "workspace is not a real directory")?;
        let canonical = fs::canonicalize(candidate)
            .map_err(|_| AppError::NotFound("workspace not found".into()))?;
        if !canonical.starts_with(&self.registry_root) || canonical == self.registry_root {
            return Err(workspace_denied("workspace resolved outside the registry"));
        }
        Ok(ResolvedWorkspace {
            workspace_id: workspace_id.to_owned(),
            path: canonical,
        })
    }
}

impl WorkspaceResolver for FilesystemWorkspaceResolver {
    fn resolve(&self, workspace_id: &str) -> Result<ResolvedWorkspace, AppError> {
        let candidate = self.candidate(workspace_id)?;
        if !candidate.exists() {
            return Err(AppError::NotFound("workspace not found".into()));
        }
        self.resolved(workspace_id, &candidate)
    }

    fn ensure(&self, workspace_id: &str) -> Result<ResolvedWorkspace, AppError> {
        let candidate = self.candidate(workspace_id)?;
        match fs::create_dir(&candidate) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(workspace_denied("workspace could not be created")),
        }
        self.resolved(workspace_id, &candidate)
    }

    fn resolve_registered(
        &self,
        workspace_id: &str,
        root_path: &str,
    ) -> Result<ResolvedWorkspace, AppError> {
        validate_workspace_id(workspace_id)?;
        self.validate_registry_root()?;
        let candidate = Path::new(root_path);
        reject_link_or_reparse(candidate, "workspace is not a real directory")?;
        let canonical = fs::canonicalize(candidate)
            .map_err(|_| AppError::NotFound("workspace not found".into()))?;
        if canonical.as_os_str() != candidate {
            return Err(workspace_denied("workspace registration does not match its identity"));
        }
        Ok(ResolvedWorkspace {
            workspace_id: workspace_id.to_owned(),
            path: canonical,
        })
    }

    fn resolve_user_path(&self, path: &str) -> Result<ResolvedWorkspace, AppError> {
        let candidate = Path::new(path);
        if path.trim().is_empty() || !candidate.is_absolute() {
            return Err(workspace_denied("workspace path must be an absolute directory path"));
        }
        reject_link_or_reparse(candidate, "workspace is not a real directory")?;
        let canonical = fs::canonicalize(candidate)
            .map_err(|_| workspace_denied("workspace path cannot be resolved"))?;
        Ok(ResolvedWorkspace {
            workspace_id: String::new(),
            path: canonical,
        })
    }
}

fn validate_workspace_id(workspace_id: &str) -> Result<(), AppError> {
    nomifun_common::validate_uuidv7(workspace_id)
        .map(|_| ())
        .map_err(|_| workspace_denied("workspace id must be a canonical UUIDv7"))
}

fn reject_link_or_reparse(path: &Path, message: &'static str) -> Result<(), AppError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| workspace_denied(message))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() || metadata_is_reparse_point(&metadata) {
        return Err(workspace_denied(message));
    }
    Ok(())
}

#[cfg(windows)]
fn metadata_is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn metadata_is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

fn workspace_denied(message: impl Into<String>) -> AppError {
    AppError::Forbidden(format!("workspace policy denied: {}", message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "nomifun-app-server-workspace-resolver-{}",
                nomifun_common::generate_id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn ensure_creates_and_resolves_workspace_below_registry() {
        let root = TestRoot::new();
        let resolver = FilesystemWorkspaceResolver::new(&root.0).unwrap();

        let workspace_id = nomifun_common::generate_id();
        let ensured = resolver.ensure(&workspace_id).unwrap();
        let resolved = resolver.resolve(&workspace_id).unwrap();

        assert_eq!(ensured, resolved);
        assert_eq!(resolved.workspace_id(), workspace_id);
        assert!(resolved.path().starts_with(resolver.registry_root()));
        assert!(resolved.path().is_dir());
    }

    #[test]
    fn resolve_does_not_create_missing_workspace() {
        let root = TestRoot::new();
        let resolver = FilesystemWorkspaceResolver::new(&root.0).unwrap();

        let missing = nomifun_common::generate_id();
        let error = resolver.resolve(&missing).unwrap_err();
        assert!(matches!(error, AppError::NotFound(_)));
        assert!(!resolver.registry_root().join(missing).exists());
    }

    #[test]
    fn rejects_path_like_and_ambiguous_workspace_ids() {
        let root = TestRoot::new();
        let resolver = FilesystemWorkspaceResolver::new(&root.0).unwrap();

        for id in ["", ".", "..", "../escape", "a/b", "a\\b", "c:escape", "white space"] {
            assert!(matches!(resolver.ensure(id), Err(AppError::Forbidden(_))), "accepted {id:?}");
        }
    }

    #[test]
    fn rejects_existing_non_directory_target() {
        let root = TestRoot::new();
        let resolver = FilesystemWorkspaceResolver::new(&root.0).unwrap();
        fs::write(resolver.registry_root().join("occupied"), b"not a directory").unwrap();

        assert!(matches!(resolver.ensure("occupied"), Err(AppError::Forbidden(_))));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_workspace_target() {
        use std::os::unix::fs::symlink;

        let root = TestRoot::new();
        let outside = TestRoot::new();
        let resolver = FilesystemWorkspaceResolver::new(&root.0).unwrap();
        symlink(&outside.0, resolver.registry_root().join("linked")).unwrap();

        assert!(matches!(resolver.resolve("linked"), Err(AppError::Forbidden(_))));
    }
}

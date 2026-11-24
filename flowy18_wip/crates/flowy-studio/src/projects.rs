//! Persistent local projects registered with Studio.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const FORMAT: &str = "flowy.projects";
const VERSION: u32 = 1;
const FILE_NAME: &str = "projects.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug)]
pub enum ProjectError {
    NameRequired,
    FolderUnavailable,
    FolderRegistered,
    Storage,
}

impl ProjectError {
    #[must_use]
    pub const fn notice_key(&self) -> &'static str {
        match self {
            Self::NameRequired => "project.name_required",
            Self::FolderUnavailable => "project.invalid_folder",
            Self::FolderRegistered => "project.folder_registered",
            Self::Storage => "project.storage_error",
        }
    }
}

#[derive(Serialize, Deserialize)]
struct ProjectFile {
    format: String,
    version: u32,
    projects: Vec<Project>,
}

#[must_use]
pub fn path(root: &Path) -> PathBuf {
    root.join(FILE_NAME)
}

pub fn root_available(root: &Path) -> bool {
    root.is_dir() && fs::read_dir(root).is_ok()
}

pub fn load(root: &Path) -> io::Result<Vec<Project>> {
    let bytes = match fs::read(path(root)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(Vec::new());
        }
        Err(error) => return Err(error),
    };
    let file: ProjectFile = serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if file.format != FORMAT || file.version != VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported project registry format",
        ));
    }
    Ok(file.projects)
}

pub fn save(root: &Path, projects: &[Project]) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(&ProjectFile {
        format: FORMAT.into(),
        version: VERSION,
        projects: projects.to_vec(),
    })
    .map_err(io::Error::other)?;
    flowy_core::persistence::atomic_write(&path(root), &bytes)
}

pub fn update(
    app_root: &Path,
    projects: &mut Vec<Project>,
    id: &str,
    name: &str,
    folder: &Path,
) -> Result<Project, ProjectError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ProjectError::NameRequired);
    }
    let root = fs::canonicalize(folder)
        .ok()
        .filter(|path| root_available(path))
        .ok_or(ProjectError::FolderUnavailable)?;
    let index = projects
        .iter()
        .position(|project| project.id == id)
        .ok_or(ProjectError::FolderUnavailable)?;
    if projects.iter().enumerate().any(|(other, project)| {
        other != index
            && fs::canonicalize(&project.root)
                .unwrap_or_else(|_| project.root.clone())
                == root
    }) {
        return Err(ProjectError::FolderRegistered);
    }
    if load(app_root).map_err(|_| ProjectError::Storage)? != *projects {
        return Err(ProjectError::Storage);
    }
    let mut next = projects.clone();
    next[index].name = name.to_owned();
    next[index].root = root;
    save(app_root, &next).map_err(|_| ProjectError::Storage)?;
    *projects = next;
    Ok(projects[index].clone())
}

pub fn remove(
    app_root: &Path,
    projects: &mut Vec<Project>,
    id: &str,
) -> Result<Project, ProjectError> {
    if load(app_root).map_err(|_| ProjectError::Storage)? != *projects {
        return Err(ProjectError::Storage);
    }
    let index = projects
        .iter()
        .position(|project| project.id == id)
        .ok_or(ProjectError::FolderUnavailable)?;
    let removed = projects[index].clone();
    let mut next = projects.clone();
    next.remove(index);
    save(app_root, &next).map_err(|_| ProjectError::Storage)?;
    *projects = next;
    Ok(removed)
}

pub fn set_pinned(
    app_root: &Path,
    projects: &mut Vec<Project>,
    id: &str,
    pinned: bool,
) -> Result<(), ProjectError> {
    if load(app_root).map_err(|_| ProjectError::Storage)? != *projects {
        return Err(ProjectError::Storage);
    }
    let index = projects
        .iter()
        .position(|project| project.id == id)
        .ok_or(ProjectError::FolderUnavailable)?;
    let mut next = projects.clone();
    next[index].pinned = pinned;
    save(app_root, &next).map_err(|_| ProjectError::Storage)?;
    *projects = next;
    Ok(())
}

pub fn create(
    app_root: &Path,
    projects: &mut Vec<Project>,
    name: &str,
    folder: &Path,
) -> Result<Project, ProjectError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ProjectError::NameRequired);
    }
    let root = fs::canonicalize(folder)
        .ok()
        .filter(|path| root_available(path))
        .ok_or(ProjectError::FolderUnavailable)?;
    if projects.iter().any(|project| {
        fs::canonicalize(&project.root).unwrap_or_else(|_| project.root.clone())
            == root
    }) {
        return Err(ProjectError::FolderRegistered);
    }
    if load(app_root).map_err(|_| ProjectError::Storage)? != *projects {
        return Err(ProjectError::Storage);
    }

    let project = Project {
        id: new_project_id(),
        name: name.to_owned(),
        root,
        pinned: false,
    };
    let mut next = projects.clone();
    next.push(project.clone());
    save(app_root, &next).map_err(|_| ProjectError::Storage)?;
    *projects = next;
    Ok(project)
}

fn new_project_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
    format!("p-{}-{stamp}-{sequence}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::{
        Project, ProjectError, create, load, path, remove, save, set_pinned,
        update,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("flowy-projects-{}-{stamp}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn registry_roundtrips_projects_and_missing_file_is_empty() {
        let scratch = Scratch::new();
        let folder = scratch.0.join("workspace");
        fs::create_dir(&folder).unwrap();
        let mut projects = Vec::new();
        let created =
            create(&scratch.0, &mut projects, "  Demo  ", &folder).unwrap();

        assert_eq!(created.name, "Demo");
        assert_eq!(created.root, fs::canonicalize(folder).unwrap());
        assert_eq!(load(&scratch.0).unwrap(), vec![created]);
        assert_eq!(path(&scratch.0).file_name().unwrap(), "projects.json");
    }

    #[test]
    fn rejects_blank_names_non_directories_and_duplicate_roots() {
        let scratch = Scratch::new();
        let folder = scratch.0.join("workspace");
        fs::create_dir(&folder).unwrap();
        let file = scratch.0.join("file.txt");
        fs::write(&file, "data").unwrap();
        let mut projects = Vec::new();

        assert!(matches!(
            create(&scratch.0, &mut projects, "  ", &folder),
            Err(ProjectError::NameRequired)
        ));
        assert!(matches!(
            create(&scratch.0, &mut projects, "File", &file),
            Err(ProjectError::FolderUnavailable)
        ));
        create(&scratch.0, &mut projects, "First", &folder).unwrap();
        assert!(matches!(
            create(&scratch.0, &mut projects, "Second", &folder),
            Err(ProjectError::FolderRegistered)
        ));
        assert_eq!(projects.len(), 1);
    }

    #[test]
    fn failed_load_does_not_replace_corrupt_registry() {
        let scratch = Scratch::new();
        let registry = path(&scratch.0);
        fs::write(&registry, b"{}").unwrap();
        let original = fs::read(&registry).unwrap();
        let folder = scratch.0.join("workspace");
        fs::create_dir(&folder).unwrap();
        let mut projects = Vec::<Project>::new();

        assert!(matches!(
            load(&scratch.0),
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData
        ));
        assert!(matches!(
            create(&scratch.0, &mut projects, "Project", &folder),
            Err(ProjectError::Storage)
        ));
        assert_eq!(fs::read(registry).unwrap(), original);
    }

    #[test]
    fn saves_empty_registry_and_rejects_unsupported_version() {
        let scratch = Scratch::new();
        save(&scratch.0, &[]).unwrap();
        assert!(load(&scratch.0).unwrap().is_empty());

        fs::write(
            path(&scratch.0),
            br#"{"format":"flowy.projects","version":99,"projects":[]}"#,
        )
        .unwrap();
        assert!(matches!(
            load(&scratch.0),
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData
        ));
    }

    #[test]
    fn edits_pins_and_removes_registry_without_deleting_folder() {
        let scratch = Scratch::new();
        let folder = scratch.0.join("workspace");
        fs::create_dir(&folder).unwrap();
        let mut projects = Vec::new();
        let project =
            create(&scratch.0, &mut projects, "Client", &folder).unwrap();

        set_pinned(&scratch.0, &mut projects, &project.id, true).unwrap();
        assert!(load(&scratch.0).unwrap()[0].pinned);
        let updated =
            update(&scratch.0, &mut projects, &project.id, "Website", &folder)
                .unwrap();
        assert_eq!(updated.name, "Website");
        assert!(updated.pinned);

        remove(&scratch.0, &mut projects, &project.id).unwrap();
        assert!(load(&scratch.0).unwrap().is_empty());
        assert!(
            folder.is_dir(),
            "unregistering a project must keep its local files"
        );
    }
}

use crate::error::UpdateError;
use crate::install_gate::InstallContext;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Replace the running `.app` with the contents of a verified `.app.tar.gz`.
pub fn replace_app(
    archive: &Path,
    context: &InstallContext,
) -> Result<PathBuf, UpdateError> {
    if !context.auto_install_supported() {
        let key = context.reason_key().unwrap_or("ota.err_blocked");
        return Err(UpdateError::InstallBlocked(key));
    }
    let Some(app_bundle) = context.app_bundle().map(Path::to_path_buf) else {
        return Err(UpdateError::InstallBlocked("ota.reason_bundle"));
    };
    let extract_root = context
        .extract_dir()
        .join(format!(".flowy-ota-{}", std::process::id()));
    let _ = fs::remove_dir_all(&extract_root);
    fs::create_dir_all(&extract_root)
        .map_err(|error| UpdateError::Install(error.to_string()))?;

    let status = Command::new("tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(&extract_root)
        .status()
        .map_err(|error| UpdateError::Install(error.to_string()))?;
    if !status.success() {
        let _ = fs::remove_dir_all(&extract_root);
        return Err(UpdateError::Install("tar extract failed".into()));
    }

    let new_app = find_single_app(&extract_root)?;
    let backup = app_bundle.with_extension("app.bak");
    let _ = fs::remove_dir_all(&backup);

    if let Err(error) = fs::rename(&app_bundle, &backup) {
        let _ = fs::remove_dir_all(&extract_root);
        return Err(UpdateError::Install(error.to_string()));
    }
    if let Err(error) = fs::rename(&new_app, &app_bundle) {
        let _ = fs::rename(&backup, &app_bundle);
        let _ = fs::remove_dir_all(&extract_root);
        return Err(UpdateError::Install(error.to_string()));
    }
    let _ = fs::remove_dir_all(&extract_root);
    let _ = fs::remove_dir_all(&backup);
    let _ = Command::new("xattr")
        .args(["-dr", "com.apple.quarantine"])
        .arg(&app_bundle)
        .status();
    Ok(app_bundle)
}

pub fn relaunch_and_exit(app_bundle: &Path) -> Result<(), UpdateError> {
    Command::new("open")
        .arg(app_bundle)
        .spawn()
        .map_err(|error| UpdateError::Install(error.to_string()))?;
    std::process::exit(0);
}

fn find_single_app(root: &Path) -> Result<PathBuf, UpdateError> {
    let mut found = Vec::new();
    let entries = fs::read_dir(root)
        .map_err(|error| UpdateError::Install(error.to_string()))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| UpdateError::Install(error.to_string()))?;
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
        {
            found.push(path);
        }
    }
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => Err(UpdateError::Install("archive has no .app".into())),
        _ => Err(UpdateError::Install(
            "archive has multiple .app bundles".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_single_app_requires_exactly_one() {
        let root = std::env::temp_dir()
            .join(format!("flowy-ota-find-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        assert!(find_single_app(&root).is_err());
        fs::create_dir_all(root.join("One.app")).unwrap();
        assert_eq!(find_single_app(&root).unwrap(), root.join("One.app"));
        fs::create_dir_all(root.join("Two.app")).unwrap();
        assert!(find_single_app(&root).is_err());
        let _ = fs::remove_dir_all(&root);
    }
}

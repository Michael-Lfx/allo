use crate::{
    IdError, InstallOrigin, InstallRecord, InstallStep, SkillId, Store,
    StoreError, install_dir, parse_skill_id, sha256_hex,
};
use serde::Deserialize;
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficialSkill {
    pub id: SkillId,
    pub title: String,
    pub summary: String,
    pub auto_invoke: bool,
}

#[derive(Debug, Error)]
pub enum OfficialError {
    #[error("official skill catalog is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid official skill identity: {0}")]
    Id(#[from] IdError),
    #[error("official skill store: {0}")]
    Store(#[from] StoreError),
    #[error("official skill filesystem: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Deserialize)]
struct OfficialRecord {
    owner_handle: String,
    slug: String,
    version: String,
    title: String,
    summary: String,
    auto_invoke: bool,
}

pub fn bundled_official() -> Result<Vec<OfficialSkill>, OfficialError> {
    parse_official(include_str!("../assets/official-skills.json"))
}

#[must_use]
pub fn bundled_markdown(slug: &str) -> Option<&'static str> {
    match slug {
        "excel" => Some(include_str!("../assets/official/excel/SKILL.md")),
        "pdf" => Some(include_str!("../assets/official/pdf/SKILL.md")),
        _ => None,
    }
}

/// Materialize bundled official skills and keep their install records current.
/// User-controlled toggles survive application upgrades.
pub fn seed_official(root: &Path) -> Result<Vec<InstallRecord>, OfficialError> {
    let store = Store::open(&root.join("skills").join("market.sqlite3"))?;
    let mut records = Vec::new();
    for skill in bundled_official()? {
        let markdown = bundled_markdown(&skill.id.slug).ok_or_else(|| {
            OfficialError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("missing bundled skill {}", skill.id.slug),
            ))
        })?;
        let content_hash = sha256_hex(markdown.as_bytes());
        let dir = install_dir(root, &skill.id);
        let should_write = fs::read_to_string(dir.join("SKILL.md"))
            .map_or(true, |existing| {
                sha256_hex(existing.as_bytes()) != content_hash
            });
        if should_write {
            if dir.exists() {
                fs::remove_dir_all(&dir)?;
            }
            fs::create_dir_all(&dir)?;
            fs::write(dir.join("SKILL.md"), markdown)?;
        }
        let existing =
            store.find_install(&skill.id.owner_handle, &skill.id.slug)?;
        let mut record = existing.unwrap_or_else(|| {
            InstallRecord::official(
                skill.id.clone(),
                skill.title.clone(),
                skill.summary.clone(),
            )
        });
        record.id = skill.id;
        record.step = InstallStep::Commit;
        record.origin = InstallOrigin::Official;
        record.content_hash = content_hash;
        record.display_name = skill.title;
        record.summary = skill.summary;
        record.latest_version = Some(record.id.version.clone());
        record.checked_at = Some(crate::install::unix_now());
        store.upsert_install(&record)?;
        for stale in store.installed()?.into_iter().filter(|saved| {
            saved.id.owner_handle == record.id.owner_handle
                && saved.id.slug == record.id.slug
                && saved.id.version != record.id.version
                && saved.origin == InstallOrigin::Official
        }) {
            store.remove_install(&stale.id)?;
            let stale_dir = install_dir(root, &stale.id);
            if stale_dir.exists() {
                fs::remove_dir_all(stale_dir)?;
            }
        }
        records.push(record);
    }
    Ok(records)
}

pub fn parse_official(body: &str) -> Result<Vec<OfficialSkill>, OfficialError> {
    let records: Vec<OfficialRecord> = serde_json::from_str(body)?;
    records
        .into_iter()
        .map(|record| {
            Ok(OfficialSkill {
                id: parse_skill_id(
                    &record.owner_handle,
                    &record.slug,
                    &record.version,
                )?,
                title: record.title,
                summary: record.summary,
                auto_invoke: record.auto_invoke,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(name: &str) -> Self {
            let sequence = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "flowy-official-{name}-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn bundled_catalog_contains_excel_and_allows_auto_invoke() {
        let skills = bundled_official().unwrap();
        let excel = skills
            .iter()
            .find(|skill| skill.id.slug == "excel")
            .unwrap();
        assert_eq!(excel.id.owner_handle, "flowy");
        assert!(excel.auto_invoke);
    }

    #[test]
    fn malformed_official_entry_fails_the_whole_catalog() {
        let body = r#"
        [
          {
            "owner_handle": "flowy",
            "slug": "excel",
            "title": "Excel",
            "summary": "Spreadsheets",
            "auto_invoke": true
          }
        ]
        "#;
        assert!(parse_official(body).is_err());
    }

    #[test]
    fn seed_is_idempotent_and_preserves_user_toggles() {
        let dir = TestDir::new("seed");
        let records = seed_official(&dir.0).unwrap();
        assert_eq!(records.len(), 2);
        let excel = records
            .iter()
            .find(|record| record.id.slug == "excel")
            .unwrap();
        let store =
            Store::open(&dir.0.join("skills").join("market.sqlite3")).unwrap();
        assert!(excel.scripts_enabled);
        store.set_auto_invoke(&excel.id, false).unwrap();
        store.set_scripts_enabled(&excel.id, false).unwrap();
        seed_official(&dir.0).unwrap();
        let catalog = crate::load_installed(&dir.0).unwrap();
        let excel = catalog
            .skills
            .iter()
            .find(|skill| skill.id.slug == "excel")
            .unwrap();
        assert!(!excel.auto_invoke);
        assert!(!excel.scripts_enabled);
        assert!(excel.content.contains("# Excel"));
    }
}

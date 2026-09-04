use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::{debug, warn};
use nomifun_common::AppError;
use nomifun_api_types::{SkillCatalogSource, SkillId};

mod prompt_builder;
pub use prompt_builder::*;

/// A discovered skill definition.
#[derive(Debug, Clone)]
pub struct SkillDefinition {
    /// Skill name (directory name or frontmatter `name`).
    pub name: String,
    /// One-line description from SKILL.md frontmatter.
    pub description: String,
    /// File system path to the SKILL.md file (absolute for custom/extension,
    /// or the materialized view path for builtin).
    pub location: PathBuf,
    /// Origin of this skill (builtin/custom/extension).
    pub source: nomifun_extension::SkillSource,
    /// Relative path inside the builtin skill corpus
    /// (e.g. `auto-inject/cron/SKILL.md`); `None` for non-builtin sources.
    pub relative_location: Option<String>,
}

/// Lightweight skill reference for index listings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillIndex {
    pub name: String,
    pub description: String,
}

/// Manages skill discovery and indexing for first-message injection.
///
/// Skills are stored in directories containing a `SKILL.md` file.
/// The SKILL.md frontmatter provides `name` and `description`.
pub struct AcpSkillManager {
    /// Cached skill definitions keyed by skill name.
    cache: RwLock<HashMap<String, SkillDefinition>>,
    /// Resolved skill paths, shared across the app.
    paths: Arc<nomifun_extension::SkillPaths>,
}

impl AcpSkillManager {
    pub fn new(paths: Arc<nomifun_extension::SkillPaths>) -> Arc<Self> {
        Arc::new(Self {
            cache: RwLock::new(HashMap::new()),
            paths,
        })
    }

    /// Populate the cache with only the named skills (no filtering by
    /// auto-inject/opt-in). Returns the resulting index. Used by the
    /// snapshot-driven first-message injector.
    pub async fn discover_by_names(&self, names: &[String]) -> Vec<SkillIndex> {
        let mut legacy_names = Vec::new();
        let mut canonical_ids = Vec::new();
        for raw_name in names {
            let name = raw_name.trim();
            if name.is_empty() {
                continue;
            }
            match SkillId::parse(name) {
                Ok(skill_id) if skill_id.source() == SkillCatalogSource::Legacy => {
                    if let Some(legacy_name) = skill_id.legacy_name() {
                        if !legacy_names.contains(&legacy_name) {
                            legacy_names.push(legacy_name);
                        }
                    }
                }
                Ok(skill_id) => {
                    let canonical_id = skill_id.as_str().to_owned();
                    if !canonical_ids.contains(&canonical_id) {
                        canonical_ids.push(canonical_id);
                    }
                }
                Err(error)
                    if ["builtin:", "user:", "project:", "extension:", "mcp:", "legacy:"]
                        .iter()
                        .any(|prefix| name.starts_with(prefix)) =>
                {
                    warn!(skill_id = name, %error, "discover_by_names: malformed canonical Skill id");
                    return Vec::new();
                }
                Err(_) => {
                    let legacy_name = name.to_owned();
                    if !legacy_names.contains(&legacy_name) {
                        legacy_names.push(legacy_name);
                    }
                }
            }
        }

        // Canonical bindings must never be downgraded to a same-name lookup.
        // A normal runtime configuration can contain both auto-injected legacy
        // names and source-qualified preset bindings, so resolve both sides
        // atomically instead of treating a mixed request as an error.
        if !canonical_ids.is_empty() {
            if !legacy_names.is_empty() {
                return match self
                    .discover_mixed_bindings_strict(names, &canonical_ids, &legacy_names)
                    .await
                {
                    Ok(index) => index,
                    Err(error) => {
                        warn!(error = %error, "discover_by_names: mixed strict discovery failed");
                        Vec::new()
                    }
                };
            }
            return match self.discover_by_skill_ids_strict(&canonical_ids).await {
                Ok(index) => index,
                Err(error) => {
                    warn!(error = %error, "discover_by_names: strict canonical discovery failed");
                    Vec::new()
                }
            };
        }
        match self.discover_by_names_strict(&legacy_names).await {
            Ok(index) => index,
            Err(error) => {
                // The historical prompt hook cannot return a Result without
                // changing every ACP hook, so it fails closed: a mixed
                // result is never cached or injected as if it were complete.
                warn!(error = %error, "discover_by_names: strict discovery failed");
                Vec::new()
            }
        }
    }

    /// Resolve a runtime selection that combines legacy auto-inject names with
    /// canonical preset bindings. Both catalogs must resolve successfully
    /// before an index is returned; otherwise the legacy Vec API fails closed
    /// without exposing a partial result.
    async fn discover_mixed_bindings_strict(
        &self,
        raw_names: &[String],
        canonical_ids: &[String],
        legacy_names: &[String],
    ) -> Result<Vec<SkillIndex>, AppError> {
        // Resolve canonical bindings first. The canonical resolver never
        // falls back to display-name precedence and leaves no partial index
        // when one source-qualified Skill is missing.
        let canonical_index = self.discover_by_skill_ids_strict(canonical_ids).await?;
        // The legacy resolver has the same all-or-nothing behavior for the
        // name-based half. If it fails, clear the canonical resolver's cache
        // side effect before returning the combined request as failed.
        let legacy_index = match self.discover_by_names_strict(legacy_names).await {
            Ok(index) => index,
            Err(error) => {
                self.cache.write().await.clear();
                return Err(error);
            }
        };

        let canonical_by_id = canonical_ids
            .iter()
            .zip(canonical_index.iter())
            .map(|(skill_id, index)| (skill_id.as_str(), index))
            .collect::<HashMap<_, _>>();
        let legacy_by_name = legacy_names
            .iter()
            .zip(legacy_index.iter())
            .map(|(name, index)| (name.as_str(), index))
            .collect::<HashMap<_, _>>();

        let mut result = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for raw_name in raw_names {
            let name = raw_name.trim();
            if name.is_empty() {
                continue;
            }
            let key = match SkillId::parse(name) {
                Ok(skill_id) if skill_id.source() == SkillCatalogSource::Legacy => {
                    skill_id.legacy_name().map(|legacy_name| legacy_name.to_owned())
                }
                Ok(skill_id) => Some(skill_id.as_str().to_owned()),
                Err(_) => Some(name.to_owned()),
            };
            let Some(key) = key else {
                continue;
            };
            if !seen.insert(key.clone()) {
                continue;
            }
            let index = if canonical_by_id.contains_key(key.as_str()) {
                canonical_by_id.get(key.as_str())
            } else {
                legacy_by_name.get(key.as_str())
            };
            let Some(index) = index else {
                return Err(AppError::NotFound(format!(
                    "resolved Skill binding is unavailable: {key}"
                )));
            };
            result.push((*index).clone());
        }

        // The existing cache stores filesystem-backed legacy definitions. The
        // combined index also contains immutable catalog snapshots whose
        // source location is intentionally opaque, so do not leave a partial
        // legacy-only cache representing the mixed request.
        self.cache.write().await.clear();
        debug!(count = result.len(), "Skills discovered by mixed bindings");
        Ok(result)
    }

    /// Discover a name-based selection atomically. A caller that needs to
    /// distinguish a missing Skill from a catalog/IO failure should use this
    /// method instead of the legacy Vec-returning wrapper above.
    pub async fn discover_by_names_strict(&self, names: &[String]) -> Result<Vec<SkillIndex>, AppError> {
        let requested = names
            .iter()
            .map(|name| name.trim())
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>();
        if requested.is_empty() {
            let mut cache = self.cache.write().await;
            cache.clear();
            return Ok(Vec::new());
        }

        let items = nomifun_extension::list_available_skills(&self.paths)
            .await
            .map_err(|error| AppError::Internal(format!("list available Skills: {error}")))?;
        let by_name = items
            .into_iter()
            .map(|item| (item.name.clone(), item))
            .collect::<HashMap<_, _>>();
        let mut missing = Vec::new();
        let mut definitions = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for name in requested {
            if !seen.insert(name.to_owned()) {
                continue;
            }
            let Some(item) = by_name.get(name) else {
                missing.push(name.to_owned());
                continue;
            };
            definitions.push((
                item.name.clone(),
                SkillDefinition {
                    name: item.name.clone(),
                    description: item.description.clone(),
                    location: PathBuf::from(&item.location),
                    source: item.source,
                    relative_location: item.relative_location.clone(),
                },
            ));
        }
        if !missing.is_empty() {
            return Err(AppError::NotFound(format!(
                "requested Skills are unavailable: {}",
                missing.join(", ")
            )));
        }

        let index = definitions
            .iter()
            .map(|(_, definition)| SkillIndex {
                name: definition.name.clone(),
                description: definition.description.clone(),
            })
            .collect::<Vec<_>>();
        let mut cache = self.cache.write().await;
        cache.clear();
        cache.extend(definitions);
        debug!(count = index.len(), "Skills discovered by name");
        Ok(index)
    }

    /// Resolve source-qualified catalog identities without downgrading them to
    /// display-name lookup. The conversation boundary normally resolves and
    /// freezes the full Markdown snapshot before ACP starts; this method keeps
    /// any direct ACP caller all-or-nothing as well.
    pub async fn discover_by_skill_ids_strict(&self, skill_ids: &[String]) -> Result<Vec<SkillIndex>, AppError> {
        let mut requested = Vec::new();
        for raw in skill_ids {
            let skill_id = SkillId::parse(raw).map_err(|error| {
                AppError::BadRequest(format!("invalid canonical Skill id '{raw}': {error}"))
            })?;
            if skill_id.source() == SkillCatalogSource::Legacy {
                return Err(AppError::BadRequest(
                    "legacy Skill bindings must use name discovery".to_owned(),
                ));
            }
            if !requested.contains(&skill_id.as_str().to_owned()) {
                requested.push(skill_id.as_str().to_owned());
            }
        }
        if requested.is_empty() {
            let mut cache = self.cache.write().await;
            cache.clear();
            return Ok(Vec::new());
        }

        let loaded = nomifun_extension::load_catalog_skills(&self.paths, &requested)
            .await
            .map_err(AppError::from)?;
        if loaded.len() != requested.len()
            || loaded.iter().zip(&requested).any(|(skill, requested)| skill.skill_id != *requested)
        {
            return Err(AppError::BadRequest(
                "canonical Skill discovery returned an incomplete result".to_owned(),
            ));
        }
        let index = loaded
            .into_iter()
            .map(|skill| SkillIndex {
                name: skill.name,
                description: skill.description,
            })
            .collect::<Vec<_>>();
        self.cache.write().await.clear();
        debug!(count = index.len(), "Skills discovered by canonical id");
        Ok(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn new_accepts_skill_paths() {
        let tmp = TempDir::new().unwrap();
        let paths = std::sync::Arc::new(nomifun_extension::resolve_skill_paths(tmp.path(), tmp.path()));
        let mgr = AcpSkillManager::new(paths.clone());
        assert!(mgr.discover_by_names(&[]).await.is_empty());
    }

    #[tokio::test]
    async fn strict_discovery_reports_missing_names_without_caching_partial_results() {
        let tmp = TempDir::new().unwrap();
        let paths = nomifun_extension::resolve_skill_paths(tmp.path(), tmp.path());
        let skill_dir = paths.user_skills_dir.join("known");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: known\ndescription: Known skill\n---\nBody",
        )
        .unwrap();
        let mgr = AcpSkillManager::new(Arc::new(paths));

        let error = mgr
            .discover_by_names_strict(&["known".into(), "missing".into()])
            .await
            .unwrap_err();
        assert!(error.to_string().contains("missing"));
        assert!(mgr.discover_by_names(&["known".into(), "missing".into()]).await.is_empty());
    }

    #[tokio::test]
    async fn mixed_legacy_and_canonical_bindings_resolve_in_request_order() {
        let tmp = TempDir::new().unwrap();
        let paths = nomifun_extension::resolve_skill_paths(tmp.path(), tmp.path());
        for (name, description) in [("legacy", "Legacy skill"), ("canonical", "Canonical skill")] {
            let skill_dir = paths.user_skills_dir.join(name);
            std::fs::create_dir_all(&skill_dir).unwrap();
            std::fs::write(
                skill_dir.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: {description}\n---\nBody"),
            )
            .unwrap();
        }
        let mgr = AcpSkillManager::new(Arc::new(paths));
        let canonical_id = SkillId::new(SkillCatalogSource::User, None, "canonical");

        let index = mgr
            .discover_by_names(&["legacy".into(), canonical_id.as_str().into()])
            .await;

        assert_eq!(
            index,
            vec![
                SkillIndex {
                    name: "legacy".into(),
                    description: "Legacy skill".into(),
                },
                SkillIndex {
                    name: "canonical".into(),
                    description: "Canonical skill".into(),
                },
            ]
        );
    }

    #[test]
    fn skill_definition_has_source_and_relative_location() {
        let def = SkillDefinition {
            name: "x".into(),
            description: "d".into(),
            location: PathBuf::from("/tmp/x"),
            source: nomifun_extension::SkillSource::Builtin,
            relative_location: Some("auto-inject/x/SKILL.md".into()),
        };
        assert_eq!(def.source, nomifun_extension::SkillSource::Builtin);
        assert_eq!(def.relative_location.as_deref(), Some("auto-inject/x/SKILL.md"));
    }

    // Frontmatter parsing tests live in nomifun-extension (covers
    // parse_frontmatter_fields there); removed from here when
    // skill_manager stopped owning that helper.
}

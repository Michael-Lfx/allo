//! Abstraction over "what are the auto-inject skill names right now?" so
//! `ConversationService` can compute the initial snapshot without forcing
//! every test setup to stand up a real `SkillPaths`.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use nomifun_common::AppError;
pub use nomifun_extension::ResolvedAgentSkill;

/// Immutable markdown snapshot resolved at the conversation boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSkillSnapshot {
    pub skill_id: String,
    pub name: String,
    pub source: String,
    pub version_hash: String,
    pub content: String,
}

#[async_trait]
pub trait SkillResolver: Send + Sync {
    /// Returns the sorted list of auto-inject builtin skill names currently
    /// available on this installation.
    async fn auto_inject_names(&self) -> Vec<String>;

    /// Resolve each skill name to its on-disk source directory, using the
    /// same search order as `materialize_skills_for_agent`.
    async fn resolve_skills(&self, names: &[String]) -> Vec<ResolvedAgentSkill>;

    /// Create symlinks pointing at each resolved skill inside the given
    /// workspace's per-backend native skills directories. `rel_dirs` is
    /// the list of relative paths (e.g. `.claude/skills`) to populate.
    /// Returns the number of symlinks successfully created.
    async fn link_workspace_skills(&self, workspace: &Path, rel_dirs: &[&str], skills: &[ResolvedAgentSkill]) -> usize;

    /// Resolve explicit, source-qualified catalog IDs into immutable Markdown.
    /// The default keeps test-only resolvers source-compatible while making an
    /// unsupported explicit load fail rather than silently disappear.
    async fn load_catalog_skills(
        &self,
        skill_ids: &[String],
    ) -> Result<Vec<ResolvedSkillSnapshot>, AppError> {
        if skill_ids.is_empty() {
            return Ok(Vec::new());
        }
        Err(AppError::BadRequest(
            "This runtime cannot load the selected Skills".to_owned(),
        ))
    }
}

/// Production adapter backed by `nomifun_extension::skill_service`.
pub struct ExtensionSkillResolver {
    paths: Arc<nomifun_extension::SkillPaths>,
}

impl ExtensionSkillResolver {
    pub fn new(paths: Arc<nomifun_extension::SkillPaths>) -> Self {
        Self { paths }
    }
}

#[async_trait]
impl SkillResolver for ExtensionSkillResolver {
    async fn auto_inject_names(&self) -> Vec<String> {
        match nomifun_extension::list_builtin_auto_skills(&self.paths).await {
            Ok(items) => {
                let mut names: Vec<String> = items.into_iter().map(|i| i.name).collect();
                names.sort();
                names
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "auto_inject_names: list_builtin_auto_skills failed, falling back to empty"
                );
                Vec::new()
            }
        }
    }

    async fn resolve_skills(&self, names: &[String]) -> Vec<ResolvedAgentSkill> {
        if names.is_empty() {
            return Vec::new();
        }
        // Conversation_id is validated upstream; we don't use a real one here
        // because this resolver is purely a path-resolution helper.
        match nomifun_extension::materialize_skills_for_agent(&self.paths, "workspace-link", names).await {
            Ok(list) => list,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "resolve_skills failed; returning empty list"
                );
                Vec::new()
            }
        }
    }

    async fn link_workspace_skills(&self, workspace: &Path, rel_dirs: &[&str], skills: &[ResolvedAgentSkill]) -> usize {
        if rel_dirs.is_empty() || skills.is_empty() {
            return 0;
        }
        match nomifun_extension::link_workspace_skills(workspace, rel_dirs, skills).await {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!(
                    workspace = %workspace.display(),
                    error = %e,
                    "link_workspace_skills failed"
                );
                0
            }
        }
    }

    async fn load_catalog_skills(
        &self,
        skill_ids: &[String],
    ) -> Result<Vec<ResolvedSkillSnapshot>, AppError> {
        nomifun_extension::load_catalog_skills(&self.paths, skill_ids)
            .await
            .map(|skills| {
                skills
                    .into_iter()
                    .map(|skill| ResolvedSkillSnapshot {
                        skill_id: skill.skill_id,
                        name: skill.name,
                        source: skill.source,
                        version_hash: skill.version_hash,
                        content: skill.content,
                    })
                    .collect()
            })
            .map_err(Into::into)
    }
}

#[cfg(test)]
pub struct FixedSkillResolver {
    pub names: Vec<String>,
}

#[cfg(test)]
#[async_trait]
impl SkillResolver for FixedSkillResolver {
    async fn auto_inject_names(&self) -> Vec<String> {
        self.names.clone()
    }

    async fn resolve_skills(&self, _names: &[String]) -> Vec<ResolvedAgentSkill> {
        Vec::new()
    }

    async fn link_workspace_skills(
        &self,
        _workspace: &Path,
        _rel_dirs: &[&str],
        _skills: &[ResolvedAgentSkill],
    ) -> usize {
        0
    }
}

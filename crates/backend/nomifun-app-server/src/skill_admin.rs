//! Skill write face (`skill/create` · `skill/update` · `skill/delete`, `16` R17 / W12).
//!
//! ## Semantics this module enforces (design record: `16` R17, `05` §4.11)
//!
//! The public id of a skill is its **name** (see
//! `nomifun_extension::skill_service::list_available_skills`): flat,
//! unqualified, newest-copy-wins, with user-root skills shadowing same-name
//! built-ins. A name therefore says nothing about who owns the content, and
//! "delete" is ambiguous the moment a name exists in more than one tree. The
//! write face resolves that by classifying the **on-disk location** of the
//! resolved entry ([`skill_origin_of`]) and refusing anything but the one
//! writable class:
//!
//! | origin | location | writable |
//! | --- | --- | --- |
//! | `user` | `{user_skills_dir}/{name}/` | **yes** |
//! | `shared` | `{user_skills_dir}/shared/{name}/` | no (companion flow owns it) |
//! | `companion` | `{user_skills_dir}/companion/{cid}/{name}/` | no (companion flow) |
//! | `draft` | `{user_skills_dir}/_drafts/{cid}/{name}/` | no (review staging) |
//! | `marketplace` | `{user_skills_dir}/agent-store/{snapshot}/{slug}/` | no — uninstall goes through `install/uninstall` |
//! | `builtin` | `{builtin_skills_dir}/…` | no |
//!
//! `{user_skills_dir}/{name}/` is not a new invention: it is exactly the
//! directory the existing [`delete_skill`] primitive removes and the first
//! entry of [`resolve_skill_source_path`] (the resolution order the runtime
//! uses to materialize a skill for an agent), so a skill created here is both
//! deletable and actually usable.
//!
//! ## Boundaries
//!
//! - **No path parameter.** The target root comes from the host-side
//!   `SkillPaths`; a request can only name a skill, never a directory.
//! - **No credential field.** Every request struct is
//!   `#[serde(deny_unknown_fields)]`, so `api_key` / `env` / `token` are
//!   `invalid_request` rather than silently ignored (R22 keeps its gate).
//! - **No silent overwrite.** `create` on an existing name is a `conflict`
//!   naming the existing origin, whatever that origin is.
//! - **No optimistic echo.** The caller re-reads through the read seam
//!   (`skill/get`) after every write; this module returns nothing it did not
//!   read back from disk.

use async_trait::async_trait;
use std::path::Path;

use nomifun_common::AppError;
use nomifun_extension::skill_service::{
    self, SkillDraftInput, SkillFieldPatch, SkillOrigin, SkillPaths, SkillScope,
};
use nomifun_extension::{ExtensionError, SKILL_MANIFEST_FILE};

/// Longest accepted skill name. Bounded so a name can never approach a
/// filesystem limit, and short enough to stay a usable public id.
const MAX_SKILL_NAME_CHARS: usize = 64;

/// Write-face request for `skill/create`.
///
/// Structured rather than raw Markdown: [`create_skill`] assembles the
/// canonical frontmatter (name / description required, the rest optional), so
/// a caller cannot author a document whose frontmatter name disagrees with the
/// id it is stored under.
#[derive(Debug, Clone)]
pub struct SkillCreateRequest {
    pub name: String,
    pub description: String,
    pub when_to_use: Option<String>,
    pub allowed_tools: Option<String>,
    pub paths: Option<String>,
    pub body: String,
}

/// Skill write seam (`skill/create` · `skill/update` · `skill/delete`).
///
/// Implementations own the filesystem primitives; the App Server layer owns
/// validation, ownership and re-reads. Returns `AppError` so the existing wire
/// mapping (`not_found` / `invalid_request` / `conflict` / `policy_denied`)
/// applies unchanged — no new error family.
#[async_trait]
pub trait SkillWriteProvider: Send + Sync {
    /// Create a new user skill. Must fail with `Conflict` when the name is
    /// already taken by *any* origin, and must never overwrite.
    async fn create_skill(&self, request: SkillCreateRequest) -> Result<(), AppError>;

    /// Replace the `SKILL.md` of an existing **writable** skill.
    async fn update_skill(&self, skill_id: &str, patch: &SkillFieldPatch<'_>) -> Result<(), AppError>;

    /// Delete an existing **writable** skill (its directory).
    async fn delete_skill(&self, skill_id: &str) -> Result<(), AppError>;

    /// Copy an existing skill — from **any** origin — into the user root under
    /// `new_name`. Never overwrites: an existing name is a `conflict`.
    async fn copy_skill(&self, skill_id: &str, new_name: &str) -> Result<(), AppError>;
}

/// Validation of a caller-supplied skill name.
///
/// The name becomes one path segment under the host's user skills root, so the
/// gate is a whitelist of shape, not a blacklist of tricks: empty,
/// whitespace-only, `.`, `..`, anything with a separator (which also covers
/// POSIX-absolute paths and Windows drive prefixes), control characters, and
/// over-long names are all `invalid_request`. The filesystem primitives
/// validate again (`validate_filename`) — this gate exists so a rejected name
/// never reaches the filesystem at all.
pub fn validate_skill_name(name: &str) -> Result<(), AppError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest("skill name must not be empty".into()));
    }
    if trimmed != name {
        return Err(AppError::BadRequest(
            "skill name must not start or end with whitespace".into(),
        ));
    }
    if name == "." || name == ".." || name.contains("..") {
        return Err(AppError::BadRequest(
            "skill name must not contain '..'".into(),
        ));
    }
    if name.contains('/') || name.contains('\\') || name.contains(':') {
        return Err(AppError::BadRequest(
            "skill name must not contain a path separator, drive prefix or absolute path".into(),
        ));
    }
    if name.chars().any(char::is_control) {
        return Err(AppError::BadRequest(
            "skill name must not contain control characters".into(),
        ));
    }
    if name.chars().count() > MAX_SKILL_NAME_CHARS {
        return Err(AppError::BadRequest(format!(
            "skill name must be at most {MAX_SKILL_NAME_CHARS} characters"
        )));
    }
    Ok(())
}

/// The directory a listed skill lives in.
///
/// `SkillListItem.location` is the `SKILL.md` **file** for built-ins and the
/// skill **directory** for the other origins, so a caller that needs the tree
/// (the copy primitive does) derives it instead of assuming either shape. The
/// trailing component is compared exactly, so a skill legitimately named
/// `SKILL.md-something` is not truncated.
fn skill_source_dir(location: &str) -> &Path {
    let path = Path::new(location);
    match path.file_name().and_then(|name| name.to_str()) {
        Some(SKILL_MANIFEST_FILE) => path.parent().unwrap_or(path),
        _ => path,
    }
}

/// Filesystem-backed [`SkillWriteProvider`] over the host's `SkillPaths`.
///
/// Reuses `nomifun_extension::skill_service` primitives
/// (`create_skill` / `write_skill` / `delete_skill`) — there is no second
/// implementation of skill file IO here.
pub struct SkillAdmin {
    paths: SkillPaths,
}

impl SkillAdmin {
    pub fn new(paths: SkillPaths) -> Self {
        Self { paths }
    }

    /// Resolve the entry that owns the public id, using the *same* list the
    /// read face serves (`list_available_skills`), so id resolution and
    /// ownership cannot disagree with what `skill/get` returns.
    async fn resolve(&self, skill_id: &str) -> Result<(SkillOrigin, String), AppError> {
        let item = skill_service::list_available_skills(&self.paths)
            .await
            .map_err(|error| AppError::Internal(format!("list skills for write: {error}")))?
            .into_iter()
            .find(|item| item.name == skill_id)
            .ok_or_else(|| AppError::NotFound(format!("skill {skill_id} not found")))?;
        let origin = skill_service::skill_origin_of(&self.paths, Path::new(&item.location));
        Ok((origin, item.location))
    }

    /// Gate every mutation: read-only origins get an origin-specific reason,
    /// and a user-root directory whose basename disagrees with the id is
    /// refused rather than guessed at (`delete_skill` joins the *id*, so
    /// proceeding would remove a path the caller never named).
    ///
    /// The exact same predicate backs the `writable` field on the read face
    /// ([`skill_service::is_writable_skill`]), so a UI that offers a write
    /// action is never contradicted by this gate.
    fn require_writable(&self, origin: SkillOrigin, location: &str, skill_id: &str) -> Result<(), AppError> {
        if !origin.is_writable() {
            return Err(AppError::Forbidden(format!(
                "policy: skill {skill_id} is not writable (origin={}): {}",
                origin.as_str(),
                origin
                    .read_only_reason()
                    .unwrap_or("this origin is read-only")
            )));
        }
        if !skill_service::is_writable_skill(&self.paths, skill_id, Path::new(location)) {
            return Err(AppError::Forbidden(format!(
                "policy: skill {skill_id} is stored at a directory whose name disagrees with its \
                 frontmatter name; the write face only manages skills at the canonical user location"
            )));
        }
        // Symlink escape: `write_skill` follows the directory and the manifest,
        // so either one being a link means the bytes would land outside the
        // user root. Judged on the real entries, not on the id.
        for entry in [
            Path::new(location).to_path_buf(),
            Path::new(location).join(SKILL_MANIFEST_FILE),
        ] {
            if std::fs::symlink_metadata(&entry)
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
            {
                return Err(AppError::Forbidden(format!(
                    "policy: refusing to write skill {skill_id} through a link; the write face only \
                     manages real directories inside the user skills root"
                )));
            }
        }
        Ok(())
    }
}

#[async_trait]
impl SkillWriteProvider for SkillAdmin {
    async fn create_skill(&self, request: SkillCreateRequest) -> Result<(), AppError> {
        let name = request.name.clone();
        validate_skill_name(&name)?;
        if request.description.trim().is_empty() {
            return Err(AppError::BadRequest(
                "skill description must not be empty".into(),
            ));
        }

        // No silent overwrite, and no shadowing either: a name that already
        // resolves to *any* origin would make the new skill ambiguous (the
        // read face dedupes by name, newest copy wins), so it is a conflict
        // that names the origin the caller collided with.
        if let Ok((origin, _)) = self.resolve(&name).await {
            return Err(AppError::Conflict(format!(
                "skill {name} already exists (origin={}); {}",
                origin.as_str(),
                match origin {
                    SkillOrigin::User =>
                        "edit it with skill/update instead of creating it again",
                    SkillOrigin::Marketplace =>
                        "an installed marketplace skill owns this name; uninstall it through \
                         install/uninstall to free the name",
                    SkillOrigin::Builtin =>
                        "a built-in skill owns this name and built-ins are read-only; choose another name",
                    _ => "the existing skill's owner flow manages this name; choose another name",
                }
            )));
        }

        // `SkillScope::User` is `{user_skills_dir}/{name}` — the one location
        // this face may write. Nothing may already be there: a directory that
        // the scanner did not report (missing/invalid `SKILL.md`, or an
        // unreadable one) would otherwise be merged into silently, and a link
        // would be followed straight out of the user root.
        let target = self.paths.user_skills_dir.join(&name);
        match std::fs::symlink_metadata(&target) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(AppError::Forbidden(format!(
                    "policy: refusing to create skill {name} through an existing link at the user \
                     skills root"
                )))
            }
            Ok(_) => {
                return Err(AppError::Conflict(format!(
                    "skill {name} already exists on disk at the canonical user location but is not \
                     in the catalog (missing or invalid SKILL.md); remove it outside the store or \
                     choose another name"
                )))
            }
            Err(_) => {}
        }

        let input = SkillDraftInput {
            name: name.clone(),
            description: request.description,
            when_to_use: request.when_to_use,
            allowed_tools: request.allowed_tools,
            paths: request.paths,
            body: request.body,
        };
        // `SkillScope::User` is `{user_skills_dir}/{name}` — the one location
        // this face may write.
        skill_service::create_skill(&self.paths, &SkillScope::User, false, &input)
            .await
            .map_err(|error| AppError::BadRequest(format!("create skill {name}: {error}")))?;
        Ok(())
    }

    async fn update_skill(&self, skill_id: &str, patch: &SkillFieldPatch<'_>) -> Result<(), AppError> {
        validate_skill_name(skill_id)?;
        if patch.is_empty() {
            return Err(AppError::BadRequest(
                "skill update names no field; say which fields change".into(),
            ));
        }
        if let Some(description) = patch.description {
            if description.trim().is_empty() {
                return Err(AppError::BadRequest(
                    "skill description must not be empty".into(),
                ));
            }
        }

        let (origin, location) = self.resolve(skill_id).await?;
        self.require_writable(origin, &location, skill_id)?;

        // Field-level merge inside the extension primitive: it reads the
        // document it is about to edit, so the caller only needs the fields it
        // changes (`skill/get` deliberately caps the body it exposes) and
        // `name` stays out of reach.
        skill_service::patch_skill(&self.paths, &SkillScope::User, false, skill_id, patch)
            .await
            .map_err(|error: ExtensionError| match AppError::from(error) {
                AppError::BadRequest(message) => {
                    AppError::BadRequest(format!("update skill {skill_id}: {message}"))
                }
                other => other,
            })?;
        Ok(())
    }

    async fn copy_skill(&self, skill_id: &str, new_name: &str) -> Result<(), AppError> {
        validate_skill_name(skill_id)?;
        validate_skill_name(new_name)?;
        if skill_id == new_name {
            return Err(AppError::BadRequest(format!(
                "skill copy needs a new name; '{new_name}' already names this skill"
            )));
        }

        // Source: any origin, because deriving a writable copy from a read-only
        // skill is exactly what this method exists for. A user-root source must
        // still be the canonical directory (same predicate as update/delete), so
        // a copy cannot be used to launder an unmanaged directory into one.
        let (origin, location) = self.resolve(skill_id).await?;
        if origin == SkillOrigin::User {
            self.require_writable(origin, &location, skill_id)?;
        }
        // `location` is the SKILL.md **file** for built-ins and the skill
        // **directory** for the other origins (see `SkillListItem::location`), so
        // the directory is derived rather than assumed.
        let source_dir = skill_source_dir(&location);

        // Target: the name must be free in *every* origin — the read face
        // dedupes by name, so a second skill with this name would be ambiguous.
        if let Ok((existing, _)) = self.resolve(new_name).await {
            return Err(AppError::Conflict(format!(
                "skill {new_name} already exists (origin={}); choose another name",
                existing.as_str()
            )));
        }
        match std::fs::symlink_metadata(self.paths.user_skills_dir.join(new_name)) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(AppError::Forbidden(format!(
                    "policy: refusing to copy a skill onto an existing link in the user skills root \
                     ({new_name})"
                )))
            }
            Ok(_) => {
                return Err(AppError::Conflict(format!(
                    "skill {new_name} already exists on disk at the canonical user location but is \
                     not in the catalog (missing or invalid SKILL.md); choose another name"
                )))
            }
            Err(_) => {}
        }

        skill_service::copy_skill_directory(&self.paths, source_dir, new_name)
            .await
            .map_err(|error| match error {
                ExtensionError::SkillExists(name) => {
                    AppError::Conflict(format!("skill {name} already exists"))
                }
                other => AppError::from(other),
            })?;
        Ok(())
    }

    async fn delete_skill(&self, skill_id: &str) -> Result<(), AppError> {
        validate_skill_name(skill_id)?;
        let (origin, location) = self.resolve(skill_id).await?;
        self.require_writable(origin, &location, skill_id)?;

        // `delete_skill` removes `{user_skills_dir}/{id}` only; a marketplace
        // product under `agent-store/<snapshot>/<slug>` cannot be reached from
        // here even by accident.
        skill_service::delete_skill(&self.paths, skill_id)
            .await
            .map_err(|error| match error {
                ExtensionError::SkillNotFound(_) => {
                    AppError::NotFound(format!("skill {skill_id} not found"))
                }
                ExtensionError::BuiltinSkillDeletion(_) => AppError::Forbidden(format!(
                    "policy: skill {skill_id} is built-in and read-only"
                )),
                other => AppError::Internal(format!("delete skill {skill_id}: {other}")),
            })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_gate_rejects_traversal_and_junk() {
        for bad in [
            "",
            "   ",
            ".",
            "..",
            "../escape",
            "a/../b",
            "a/b",
            "a\\b",
            "C:\\skills\\evil",
            "/etc/passwd",
            "skill\u{0}name",
            "skill\nname",
        ] {
            let error = validate_skill_name(bad).unwrap_err();
            assert!(
                matches!(error, AppError::BadRequest(_)),
                "{bad:?} must be invalid_request, got {error:?}"
            );
        }
        let long = "x".repeat(65);
        assert!(matches!(
            validate_skill_name(&long).unwrap_err(),
            AppError::BadRequest(_)
        ));
        for good in ["demo", "code-review", "我的技能", "skill.v2"] {
            assert!(validate_skill_name(good).is_ok(), "{good} must be accepted");
        }
    }

    /// The id *is* the frontmatter name, and `skill/update` can no longer
    /// rewrite it: the patch type has no `name` field, and the merge primitive
    /// never touches the `name:` line (asserted in `nomifun-extension`).
    #[test]
    fn the_write_face_has_no_rename_path() {
        let patch = SkillFieldPatch {
            description: Some("new description"),
            ..SkillFieldPatch::default()
        };
        assert!(!patch.is_empty());
        assert!(SkillFieldPatch::default().is_empty());
    }

    /// Both shapes the read face reports are understood: a built-in's
    /// `…/SKILL.md` file path and another origin's skill directory.
    #[test]
    fn skill_source_dir_handles_the_file_and_directory_shapes() {
        assert_eq!(
            skill_source_dir("/data/skills/builtin-name/SKILL.md"),
            Path::new("/data/skills/builtin-name")
        );
        assert_eq!(
            skill_source_dir("/data/skills/user-name"),
            Path::new("/data/skills/user-name")
        );
        // Only the exact manifest file name is stripped.
        assert_eq!(
            skill_source_dir("/data/skills/SKILL.md.notes"),
            Path::new("/data/skills/SKILL.md.notes")
        );
    }
}

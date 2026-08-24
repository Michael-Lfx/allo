//! Thin wrapper around `nomi_vimax::VimaxService` with GatewayConfig reload.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nomi_config::{GatewayConfig, config_yaml_path, load_user_config_file};
use nomi_vimax::{
    pack_skill_dir, ArtifactNode, CameoPhotoEntry, CameoUpdate, FlowyVimaxServices, RenderStatus,
    RunStatus, SessionRecord, SessionSummary, SkillSource, VerticalSkill, VerticalSkillDraft,
    VerticalSkillSummary, VimaxService, WorkflowKind,
};
use nomifun_api_types::{
    TvShowLikeResponse, TvShowListResponse, TvShowPublishRequest, TvShowPublishResponse,
    TvShowPublishSessionRequest, TvShowVideo, VimaxCloudSkill, VimaxCloudSkillInstallResponse,
    VimaxCloudSkillLikeResponse, VimaxCloudSkillListResponse, VimaxCloudSkillPublishLocalRequest,
    VimaxCloudSkillPublishRequest, VimaxCloudSkillPublishResponse, VimaxSessionSummary,
};
use nomifun_cloud::{FlowyApiClient, ServerSession};
use nomifun_common::AppError;
use sha2::{Digest, Sha256};
use tracing::{info, warn};

/// Soft cap when buffering a `.nomivimax` archive for OSS upload.
const MAX_TV_SHOW_PACKAGE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Soft cap for Skill Hub packages (matches cloud + packer).
const MAX_SKILL_PACKAGE_BYTES: u64 = 5 * 1024 * 1024;

pub struct VimaxApiService {
    data_dir: PathBuf,
    inner: Arc<VimaxService>,
}

impl VimaxApiService {
    pub fn new(data_dir: PathBuf) -> Result<Self, AppError> {
        let flowy = load_flowy(&data_dir);
        let inner = VimaxService::start(&data_dir, flowy)
            .map_err(|e| AppError::Internal(e.to_string()))?;
        Ok(Self { data_dir, inner })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    async fn refresh_backends(&self) {
        let flowy = load_flowy(&self.data_dir);
        self.inner.set_flowy(flowy).await;
    }

    pub fn list_sessions(&self) -> Result<Vec<SessionRecord>, AppError> {
        self.inner
            .list_sessions()
            .map_err(|e| AppError::Internal(e.to_string()))
    }

    pub fn list_session_summaries(&self) -> Result<Vec<VimaxSessionSummary>, AppError> {
        self.inner
            .list_session_summaries()
            .map(|summaries| summaries.into_iter().map(session_summary_from).collect())
            .map_err(|e| AppError::Internal(e.to_string()))
    }

    pub fn create_session(
        &self,
        workflow: &str,
        title: Option<String>,
    ) -> Result<SessionRecord, AppError> {
        let kind = WorkflowKind::parse(workflow)
            .ok_or_else(|| AppError::BadRequest(format!("unknown workflow: {workflow}")))?;
        self.inner
            .create_session(kind, title)
            .map_err(|e| AppError::Internal(e.to_string()))
    }

    pub fn list_vertical_skills(
        &self,
        mode: Option<&str>,
        source: Option<&str>,
    ) -> Result<Vec<VerticalSkillSummary>, AppError> {
        let mode = mode
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| {
                WorkflowKind::parse(s)
                    .ok_or_else(|| AppError::BadRequest(format!("unknown workflow: {s}")))
            })
            .transpose()?;
        let source = source
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| {
                SkillSource::parse(s)
                    .ok_or_else(|| AppError::BadRequest(format!("unknown skill source: {s}")))
            })
            .transpose()?;
        self.inner
            .list_vertical_skills(mode, source)
            .map_err(map_vimax_err)
    }

    pub fn get_vertical_skill(&self, id: &str) -> Result<(VerticalSkill, String), AppError> {
        self.inner.get_vertical_skill(id).map_err(map_vimax_err)
    }

    pub fn create_vertical_skill(
        &self,
        draft: VerticalSkillDraft,
    ) -> Result<VerticalSkill, AppError> {
        self.inner
            .create_vertical_skill(draft)
            .map_err(map_vimax_err)
    }

    pub fn update_vertical_skill(
        &self,
        name: &str,
        draft: VerticalSkillDraft,
    ) -> Result<VerticalSkill, AppError> {
        self.inner
            .update_vertical_skill(name, draft)
            .map_err(map_vimax_err)
    }

    pub fn delete_vertical_skill(&self, name: &str) -> Result<(), AppError> {
        self.inner
            .delete_vertical_skill(name)
            .map_err(map_vimax_err)
    }

    pub fn publish_vertical_skill(&self, name: &str) -> Result<VerticalSkill, AppError> {
        self.inner
            .publish_vertical_skill(name)
            .map_err(map_vimax_err)
    }

    pub fn unpublish_vertical_skill(&self, name: &str) -> Result<(), AppError> {
        self.inner
            .unpublish_vertical_skill(name)
            .map_err(map_vimax_err)
    }

    pub fn import_vertical_skill(&self, path: &str) -> Result<VerticalSkill, AppError> {
        let path = Path::new(path);
        if !path.exists() {
            return Err(AppError::BadRequest(format!(
                "skill path not found: {}",
                path.display()
            )));
        }
        self.inner
            .import_vertical_skill(path)
            .map_err(map_vimax_err)
    }

    pub fn get_session(&self, id: &str) -> Result<SessionRecord, AppError> {
        self.inner.get_session(id).map_err(map_vimax_err)
    }

    pub fn working_dir(&self, id: &str) -> Result<PathBuf, AppError> {
        self.inner.working_dir(id).map_err(map_vimax_err)
    }

    pub fn set_session_final_video(
        &self,
        id: &str,
        final_video: Option<String>,
    ) -> Result<SessionRecord, AppError> {
        self.inner
            .set_final_video(id, final_video)
            .map_err(map_vimax_err)
    }

    pub async fn plan(
        &self,
        id: &str,
        idea: Option<String>,
        script: Option<String>,
        novel_text: Option<String>,
        user_requirement: Option<String>,
        style: Option<String>,
        vertical_skill_ids: Option<Vec<String>>,
        llm_model: Option<String>,
        image_model: Option<String>,
        video_model: Option<String>,
        target_duration_secs: Option<u32>,
        aspect_ratio: Option<String>,
        resolution: Option<String>,
        fps: Option<u32>,
    ) -> Result<(), AppError> {
        self.refresh_backends().await;
        self.inner
            .plan(
                id,
                idea,
                script,
                novel_text,
                user_requirement,
                style,
                vertical_skill_ids,
                llm_model,
                image_model,
                video_model,
                target_duration_secs,
                aspect_ratio,
                resolution,
                fps,
            )
            .await
            .map_err(map_vimax_err)
    }

    pub async fn revise(
        &self,
        id: &str,
        revision_target: String,
        revision_instruction: String,
    ) -> Result<(), AppError> {
        self.refresh_backends().await;
        self.inner
            .revise(id, revision_target, revision_instruction)
            .await
            .map_err(map_vimax_err)
    }

    pub async fn write_artifact_text(
        &self,
        id: &str,
        relative_path: &str,
        content: &str,
    ) -> Result<nomi_vimax::ReviseResult, AppError> {
        self.inner
            .write_artifact_text(id, relative_path, content)
            .await
            .map_err(map_vimax_err)
    }

    pub async fn replace_artifact_file(
        &self,
        id: &str,
        relative_path: &str,
        bytes: Vec<u8>,
    ) -> Result<nomi_vimax::ReviseResult, AppError> {
        self.inner
            .replace_artifact_file(id, relative_path, bytes)
            .await
            .map_err(map_vimax_err)
    }

    pub async fn get_artifact_image_prompt(
        &self,
        id: &str,
        image_path: &str,
    ) -> Result<nomi_vimax::ImagePromptInfo, AppError> {
        self.inner
            .get_artifact_image_prompt(id, image_path)
            .await
            .map_err(map_vimax_err)
    }

    pub async fn update_artifact_image_prompt(
        &self,
        id: &str,
        image_path: &str,
        prompt: &str,
    ) -> Result<nomi_vimax::ReviseResult, AppError> {
        self.inner
            .update_artifact_image_prompt(id, image_path, prompt)
            .await
            .map_err(map_vimax_err)
    }

    pub async fn render(
        &self,
        id: &str,
        llm_model: Option<String>,
        image_model: Option<String>,
        video_model: Option<String>,
        resolution: Option<String>,
        fps: Option<u32>,
    ) -> Result<(), AppError> {
        self.refresh_backends().await;
        self.inner
            .render(id, llm_model, image_model, video_model, resolution, fps)
            .await
            .map_err(map_vimax_err)
    }

    pub async fn status(&self, id: &str) -> Result<RenderStatus, AppError> {
        self.inner.status(id).await.map_err(map_vimax_err)
    }

    pub async fn cancel(&self, id: &str) -> Result<(), AppError> {
        self.inner.cancel(id).await.map_err(map_vimax_err)
    }

    /// Pause all active video jobs for process shutdown (keeps checkpoints).
    pub async fn interrupt_all(&self) -> usize {
        self.inner.interrupt_all().await
    }

    pub async fn delete_session(&self, id: &str) -> Result<(), AppError> {
        self.inner.delete_session(id).await.map_err(map_vimax_err)
    }

    pub fn list_artifacts(&self, id: &str) -> Result<Vec<ArtifactNode>, AppError> {
        self.inner.list_artifacts(id).map_err(map_vimax_err)
    }

    pub fn artifact_path(&self, id: &str, rel: &str) -> Result<PathBuf, AppError> {
        self.inner.artifact_path(id, rel).map_err(map_vimax_err)
    }

    /// Export session project to a local `.nomivimax` path chosen by the client.
    pub async fn export_session(
        &self,
        id: &str,
        dest_path: impl AsRef<Path>,
    ) -> Result<PathBuf, AppError> {
        self.inner
            .export_session(id, dest_path)
            .await
            .map_err(map_vimax_err)
    }

    /// Import a local `.nomivimax` archive as a new session.
    pub async fn import_session(
        &self,
        archive_path: impl AsRef<Path>,
    ) -> Result<SessionRecord, AppError> {
        self.inner
            .import_session(archive_path)
            .await
            .map_err(map_vimax_err)
    }

    pub fn list_cameos(&self, id: &str) -> Result<Vec<CameoPhotoEntry>, AppError> {
        self.inner.list_cameos(id).map_err(map_vimax_err)
    }

    pub async fn upload_cameo(
        &self,
        id: &str,
        bytes: Vec<u8>,
        character_name: String,
        description: String,
    ) -> Result<CameoPhotoEntry, AppError> {
        self.inner
            .upload_cameo(id, bytes, character_name, description)
            .await
            .map_err(map_vimax_err)
    }

    pub async fn update_cameo(
        &self,
        id: &str,
        photo_id: &str,
        update: CameoUpdate,
    ) -> Result<CameoPhotoEntry, AppError> {
        self.inner
            .update_cameo(id, photo_id, update)
            .await
            .map_err(map_vimax_err)
    }

    pub async fn delete_cameo(&self, id: &str, photo_id: &str) -> Result<(), AppError> {
        self.inner
            .delete_cameo(id, photo_id)
            .await
            .map_err(map_vimax_err)
    }

    pub fn cameo_photo_path(&self, id: &str, photo_id: &str) -> Result<PathBuf, AppError> {
        self.inner
            .cameo_photo_path(id, photo_id)
            .map_err(map_vimax_err)
    }

    pub fn list_action_assets(
        &self,
        id: &str,
    ) -> Result<nomi_vimax::ActionAssetsInfo, AppError> {
        self.inner.list_action_assets(id).map_err(map_vimax_err)
    }

    pub async fn upload_action_assets(
        &self,
        id: &str,
        character: Option<Vec<u8>>,
        video: Option<Vec<u8>>,
    ) -> Result<nomi_vimax::ActionAssetsInfo, AppError> {
        self.inner
            .upload_action_assets(id, character, video)
            .await
            .map_err(map_vimax_err)
    }

    // ── TV Show (Flowy cloud) ──────────────────────────────────────────────

    async fn flowy_client_and_session(&self) -> Result<(FlowyApiClient, ServerSession), AppError> {
        let cfg: GatewayConfig =
            load_user_config_file(&config_yaml_path(Some(&self.data_dir))).map_err(|e| {
                AppError::BadRequest(format!("failed to load config: {e}"))
            })?;
        if !cfg.server.api_ready() {
            return Err(AppError::BadRequest(
                "server base_url not configured".into(),
            ));
        }
        let session = ServerSession::from_config(&cfg.server, &self.data_dir);
        let token = session
            .access_token()
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?
            .filter(|t| !t.trim().is_empty());
        if token.is_none() {
            return Err(AppError::Unauthorized(
                "cloud login required".into(),
            ));
        }
        let client =
            FlowyApiClient::new(&cfg.server).map_err(|e| AppError::Internal(e.to_string()))?;
        Ok((client, session))
    }

    /// Package local session (cover + `.nomivimax`) via OSS, then publish to TV Show.
    pub async fn publish_session_to_tv_show(
        &self,
        id: &str,
        req: TvShowPublishSessionRequest,
    ) -> Result<TvShowPublishResponse, AppError> {
        let session = self.get_session(id)?;
        if session.status != RunStatus::Succeeded {
            return Err(AppError::BadRequest(
                "only succeeded sessions can be published to TV Show".into(),
            ));
        }
        let cover_rel = session
            .cover
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                AppError::BadRequest("cover image is required before publishing".into())
            })?;
        let final_video = session
            .final_video
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if final_video.is_none() {
            return Err(AppError::BadRequest(
                "finished video is required before publishing".into(),
            ));
        }

        let (client, cloud_session) = self.flowy_client_and_session().await?;

        let cover_path = self.artifact_path(id, cover_rel)?;
        if !cover_path.is_file() {
            return Err(AppError::BadRequest(format!(
                "cover file missing: {}",
                cover_path.display()
            )));
        }
        let cover_bytes = tokio::fs::read(&cover_path)
            .await
            .map_err(|e| AppError::Internal(format!("read cover: {e}")))?;
        let cover_name = cover_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("cover.png");
        let cover_mime = mime_guess::from_path(&cover_path)
            .first_or_octet_stream()
            .essence_str()
            .to_string();
        let cover_mime = if cover_mime.starts_with("image/") {
            cover_mime
        } else {
            "image/png".into()
        };

        info!(session_id = %id, bytes = cover_bytes.len(), "TV Show: uploading cover");
        let cover_upload = client
            .upload_bytes_via_oss_detailed(
                &cloud_session,
                &cover_bytes,
                cover_name,
                &cover_mime,
                None,
            )
            .await
            .map_err(map_cloud_err)?;

        let title_raw = req
            .title
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(session.title.trim());
        let title_raw = if title_raw.is_empty() {
            "Untitled"
        } else {
            title_raw
        };
        let safe_title = sanitize_archive_stem(title_raw);
        let package_name = format!("{safe_title}.nomivimax");
        let tmp_path = std::env::temp_dir().join(format!(
            "vimax-tv-show-{}-{}.nomivimax",
            id,
            uuid_simple()
        ));
        let export_path = self.export_session(id, &tmp_path).await?;
        let package_meta = tokio::fs::metadata(&export_path)
            .await
            .map_err(|e| AppError::Internal(format!("package stat: {e}")))?;
        if package_meta.len() > MAX_TV_SHOW_PACKAGE_BYTES {
            let _ = tokio::fs::remove_file(&export_path).await;
            return Err(AppError::BadRequest(format!(
                "project package too large ({} bytes); max is {MAX_TV_SHOW_PACKAGE_BYTES}",
                package_meta.len()
            )));
        }
        let package_bytes = tokio::fs::read(&export_path)
            .await
            .map_err(|e| AppError::Internal(format!("read package: {e}")))?;
        if let Err(e) = tokio::fs::remove_file(&export_path).await {
            warn!(
                path = %export_path.display(),
                error = %e,
                "failed to remove temp TV Show package"
            );
        }

        info!(
            session_id = %id,
            bytes = package_bytes.len(),
            "TV Show: uploading project package"
        );
        let package_upload = client
            .upload_package_via_oss(&cloud_session, &package_bytes, &package_name)
            .await
            .map_err(map_cloud_err)?;

        let title = title_raw.chars().take(200).collect::<String>();

        let description = req
            .description
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.chars().take(1000).collect::<String>());

        let style = {
            let s = session.style.trim();
            if s.is_empty() {
                None
            } else {
                Some(s.chars().take(200).collect::<String>())
            }
        };

        let target_duration_secs = if session.target_duration_secs > 0 {
            Some(session.target_duration_secs as i32)
        } else {
            None
        };

        let body = TvShowPublishRequest {
            client_session_id: session.session_id.clone(),
            title,
            description,
            workflow: session.workflow.as_str().to_string(),
            style,
            target_duration_secs,
            cover_url: cover_upload.public_url,
            cover_object_key: cover_upload.object_key,
            package_url: package_upload.public_url,
            package_object_key: package_upload.object_key,
            package_size_bytes: Some(package_upload.byte_size as i64),
            package_sha256: None,
            archive_version: Some(1),
        };

        client
            .tv_show_publish(&cloud_session, &body)
            .await
            .map_err(map_cloud_err)
    }

    pub async fn tv_show_list(
        &self,
        page: Option<i32>,
        page_size: Option<i32>,
        workflow: Option<String>,
        keyword: Option<String>,
        sort: Option<String>,
    ) -> Result<TvShowListResponse, AppError> {
        let (client, session) = self.flowy_client_and_session().await?;
        client
            .tv_show_list(
                &session,
                page,
                page_size,
                workflow.as_deref(),
                keyword.as_deref(),
                sort.as_deref(),
            )
            .await
            .map_err(map_cloud_err)
    }

    pub async fn tv_show_mine(
        &self,
        page: Option<i32>,
        page_size: Option<i32>,
        status: Option<String>,
    ) -> Result<TvShowListResponse, AppError> {
        let (client, session) = self.flowy_client_and_session().await?;
        client
            .tv_show_mine(&session, page, page_size, status.as_deref())
            .await
            .map_err(map_cloud_err)
    }

    pub async fn tv_show_detail(&self, id: i64) -> Result<TvShowVideo, AppError> {
        let (client, session) = self.flowy_client_and_session().await?;
        client
            .tv_show_detail(&session, id)
            .await
            .map_err(map_cloud_err)
    }

    pub async fn tv_show_like(&self, id: i64) -> Result<TvShowLikeResponse, AppError> {
        let (client, session) = self.flowy_client_and_session().await?;
        client
            .tv_show_like(&session, id)
            .await
            .map_err(map_cloud_err)
    }

    pub async fn tv_show_unlike(&self, id: i64) -> Result<TvShowLikeResponse, AppError> {
        let (client, session) = self.flowy_client_and_session().await?;
        client
            .tv_show_unlike(&session, id)
            .await
            .map_err(map_cloud_err)
    }

    pub async fn tv_show_delete(&self, id: i64) -> Result<(), AppError> {
        let (client, session) = self.flowy_client_and_session().await?;
        client
            .tv_show_delete(&session, id)
            .await
            .map_err(map_cloud_err)
    }

    /// Download a published TV Show package and import as a new local session.
    pub async fn import_tv_show(&self, id: i64) -> Result<SessionRecord, AppError> {
        let detail = self.tv_show_detail(id).await?;
        let package_url = detail
            .package_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AppError::BadRequest("TV Show package URL unavailable".into()))?;

        let tmp_path = std::env::temp_dir().join(format!(
            "vimax-tv-show-import-{}-{}.nomivimax",
            id,
            uuid_simple()
        ));
        download_url_to_file(package_url, &tmp_path).await?;
        let imported = self.import_session(&tmp_path).await;
        if let Err(e) = tokio::fs::remove_file(&tmp_path).await {
            warn!(
                path = %tmp_path.display(),
                error = %e,
                "failed to remove imported TV Show temp package"
            );
        }
        imported
    }

    // ── Skill Hub (Flowy cloud) ────────────────────────────────────────────

    /// Pack a local user/hub skill, upload via OSS, then publish to cloud Skill Hub.
    pub async fn publish_skill_to_cloud(
        &self,
        id: &str,
        req: VimaxCloudSkillPublishLocalRequest,
    ) -> Result<VimaxCloudSkillPublishResponse, AppError> {
        let (skill, manifest) = self.get_vertical_skill(id)?;
        let skill_dir = self
            .inner
            .vertical_skill_dir(id)
            .map_err(map_vimax_err)?;

        let (client, cloud_session) = self.flowy_client_and_session().await?;

        let package_name = format!("{}.vimaxskill", skill.name);
        let tmp_path = std::env::temp_dir().join(format!(
            "vimax-skill-{}-{}.vimaxskill",
            skill.name,
            uuid_simple()
        ));
        let package_size = tokio::task::spawn_blocking({
            let skill_dir = skill_dir.clone();
            let tmp_path = tmp_path.clone();
            move || pack_skill_dir(&skill_dir, &tmp_path)
        })
        .await
        .map_err(|e| AppError::Internal(format!("pack skill join: {e}")))?
        .map_err(map_vimax_err)?;

        if package_size > MAX_SKILL_PACKAGE_BYTES {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(AppError::BadRequest(format!(
                "skill package too large ({package_size} bytes); max is {MAX_SKILL_PACKAGE_BYTES}"
            )));
        }

        let package_bytes = tokio::fs::read(&tmp_path)
            .await
            .map_err(|e| AppError::Internal(format!("read skill package: {e}")))?;
        if let Err(e) = tokio::fs::remove_file(&tmp_path).await {
            warn!(
                path = %tmp_path.display(),
                error = %e,
                "failed to remove temp skill package"
            );
        }

        let package_sha256 = {
            let mut hasher = Sha256::new();
            hasher.update(&package_bytes);
            Some(hex::encode(hasher.finalize()))
        };

        info!(
            skill_id = %id,
            bytes = package_bytes.len(),
            "Skill Hub: uploading package"
        );
        let package_upload = client
            .upload_package_via_oss(&cloud_session, &package_bytes, &package_name)
            .await
            .map_err(map_cloud_err)?;

        let package_object_key = package_upload.object_key.clone().ok_or_else(|| {
            AppError::Internal("OSS upload missing objectKey for skill package".into())
        })?;

        let mut cover_url = req
            .cover_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let mut cover_object_key = None;

        // Fallback: cover-url from local SKILL.md frontmatter.
        if cover_url.is_none() {
            cover_url = extract_frontmatter_value(&manifest, "cover-url")
                .or_else(|| extract_frontmatter_value(&manifest, "cover_url"));
        }

        if let Some(ref cover) = cover_url.clone() {
            if let Some((mime, bytes)) = decode_data_url_image(cover) {
                let ext = mime_to_ext(&mime);
                let cover_name = format!("cover.{ext}");
                let cover_upload = client
                    .upload_bytes_via_oss_detailed(
                        &cloud_session,
                        &bytes,
                        &cover_name,
                        &mime,
                        None,
                    )
                    .await
                    .map_err(map_cloud_err)?;
                cover_url = Some(cover_upload.public_url);
                cover_object_key = cover_upload.object_key;
            } else {
                // Local path under the skill dir, or absolute file path.
                let local = if Path::new(cover).is_file() {
                    Some(PathBuf::from(cover))
                } else {
                    let rel = skill_dir.join(cover);
                    rel.is_file().then_some(rel)
                };
                if let Some(cover_path) = local {
                    let cover_bytes = tokio::fs::read(&cover_path)
                        .await
                        .map_err(|e| AppError::Internal(format!("read cover: {e}")))?;
                    let cover_name = cover_path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("cover.png");
                    let cover_mime = mime_guess::from_path(&cover_path)
                        .first_or_octet_stream()
                        .essence_str()
                        .to_string();
                    let cover_mime = if cover_mime.starts_with("image/") {
                        cover_mime
                    } else {
                        "image/png".into()
                    };
                    let cover_upload = client
                        .upload_bytes_via_oss_detailed(
                            &cloud_session,
                            &cover_bytes,
                            cover_name,
                            &cover_mime,
                            None,
                        )
                        .await
                        .map_err(map_cloud_err)?;
                    cover_url = Some(cover_upload.public_url);
                    cover_object_key = cover_upload.object_key;
                } else if !(cover.starts_with("http://") || cover.starts_with("https://")) {
                    // Not a usable URL / path — drop rather than send garbage to cloud.
                    cover_url = None;
                }
            }
        }

        let case_url = req
            .case_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let body = VimaxCloudSkillPublishRequest {
            name: skill.name.clone(),
            display_name: Some(skill.display_name.clone()),
            description: Some(skill.description.clone()),
            category: Some(normalize_skill_hub_category(skill.category.as_str())),
            version: Some(skill.version.clone()).filter(|s| !s.trim().is_empty()),
            tags: skill.tags.clone(),
            use_scenario: extract_frontmatter_value(&manifest, "use-scenario")
                .or_else(|| extract_frontmatter_value(&manifest, "use_scenario")),
            how_to_use: extract_frontmatter_value(&manifest, "how-to-use")
                .or_else(|| extract_frontmatter_value(&manifest, "how_to_use")),
            output: extract_frontmatter_value(&manifest, "output"),
            compatible_modes: skill
                .compatible_modes
                .iter()
                .map(|m| m.as_str().to_string())
                .collect(),
            requirement_overlay: Some(skill.requirement_overlay.clone())
                .filter(|s| !s.trim().is_empty()),
            style_overlay: Some(skill.style_overlay.clone()).filter(|s| !s.trim().is_empty()),
            playbook: Some(skill.playbook.clone()).filter(|s| !s.trim().is_empty()),
            package_url: package_upload.public_url,
            package_object_key,
            package_size_bytes: Some(package_upload.byte_size as i64),
            package_sha256,
            cover_url,
            cover_object_key,
            case_url,
            client_skill_id: Some(skill.id.qualified()),
        };

        client
            .vimax_skill_publish(&cloud_session, &body)
            .await
            .map_err(map_cloud_err)
    }

    pub async fn skill_hub_list(
        &self,
        page: Option<i32>,
        page_size: Option<i32>,
        keyword: Option<String>,
        category: Option<String>,
        mode: Option<String>,
        sort: Option<String>,
        author_id: Option<i64>,
    ) -> Result<VimaxCloudSkillListResponse, AppError> {
        let (client, session) = self.flowy_client_and_session().await?;
        client
            .vimax_skill_list(
                &session,
                page,
                page_size,
                keyword.as_deref(),
                category.as_deref(),
                mode.as_deref(),
                sort.as_deref(),
                author_id,
            )
            .await
            .map_err(map_cloud_err)
    }

    pub async fn skill_hub_mine(
        &self,
        page: Option<i32>,
        page_size: Option<i32>,
        status: Option<String>,
    ) -> Result<VimaxCloudSkillListResponse, AppError> {
        let (client, session) = self.flowy_client_and_session().await?;
        client
            .vimax_skill_mine(&session, page, page_size, status.as_deref())
            .await
            .map_err(map_cloud_err)
    }

    pub async fn skill_hub_detail(&self, id: i64) -> Result<VimaxCloudSkill, AppError> {
        let (client, session) = self.flowy_client_and_session().await?;
        client
            .vimax_skill_detail(&session, id)
            .await
            .map_err(map_cloud_err)
    }

    pub async fn skill_hub_like(&self, id: i64) -> Result<VimaxCloudSkillLikeResponse, AppError> {
        let (client, session) = self.flowy_client_and_session().await?;
        client
            .vimax_skill_like(&session, id)
            .await
            .map_err(map_cloud_err)
    }

    pub async fn skill_hub_unlike(
        &self,
        id: i64,
    ) -> Result<VimaxCloudSkillLikeResponse, AppError> {
        let (client, session) = self.flowy_client_and_session().await?;
        client
            .vimax_skill_unlike(&session, id)
            .await
            .map_err(map_cloud_err)
    }

    pub async fn skill_hub_unpublish(&self, id: i64) -> Result<(), AppError> {
        let (client, session) = self.flowy_client_and_session().await?;
        client
            .vimax_skill_unpublish(&session, id)
            .await
            .map_err(map_cloud_err)
    }

    pub async fn skill_hub_delete(&self, id: i64) -> Result<(), AppError> {
        let (client, session) = self.flowy_client_and_session().await?;
        client
            .vimax_skill_delete(&session, id)
            .await
            .map_err(map_cloud_err)
    }

    /// Call cloud install, download package, import into local user catalog.
    pub async fn skill_hub_install(&self, id: i64) -> Result<VerticalSkill, AppError> {
        let (client, session) = self.flowy_client_and_session().await?;
        let install: VimaxCloudSkillInstallResponse = client
            .vimax_skill_install(&session, id)
            .await
            .map_err(map_cloud_err)?;

        let package_url = install.package_url.trim();
        if package_url.is_empty() {
            return Err(AppError::BadRequest(
                "Skill Hub package URL unavailable".into(),
            ));
        }

        let tmp_path = std::env::temp_dir().join(format!(
            "vimax-skill-install-{}-{}.vimaxskill",
            id,
            uuid_simple()
        ));
        download_url_to_file_capped(package_url, &tmp_path, MAX_SKILL_PACKAGE_BYTES).await?;
        let imported = self
            .inner
            .import_vertical_skill_package(
                &tmp_path,
                Some(install.id),
                Some(install.version.as_str()),
            )
            .map_err(map_vimax_err);
        if let Err(e) = tokio::fs::remove_file(&tmp_path).await {
            warn!(
                path = %tmp_path.display(),
                error = %e,
                "failed to remove imported skill temp package"
            );
        }
        imported
    }
}

fn session_summary_from(summary: SessionSummary) -> VimaxSessionSummary {
    VimaxSessionSummary {
        id: summary.session_id,
        title: summary.title,
        workflow: summary.workflow.as_str().to_string(),
        stage: summary.stage,
        status: summary.status.as_str().to_string(),
        final_video: summary.final_video,
        cover: summary.cover,
        created_at: summary.created_at,
        updated_at: summary.updated_at,
    }
}

fn sanitize_archive_stem(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | ' ' => '-',
            _ => c,
        })
        .collect();
    let trimmed = cleaned.trim_matches('-');
    let stem: String = trimmed.chars().take(48).collect();
    if stem.is_empty() {
        "nomi-video".into()
    } else {
        stem
    }
}

fn uuid_simple() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn extract_frontmatter_value(md: &str, key: &str) -> Option<String> {
    let key_lc = key.to_ascii_lowercase();
    for line in md.lines() {
        let trimmed = line.trim();
        if trimmed == "---" {
            continue;
        }
        let Some((k, v)) = trimmed.split_once(':') else {
            continue;
        };
        if k.trim().to_ascii_lowercase().replace('_', "-") != key_lc.replace('_', "-") {
            continue;
        }
        let mut val = v.trim().to_string();
        if (val.starts_with('"') && val.ends_with('"'))
            || (val.starts_with('\'') && val.ends_with('\''))
        {
            val = val[1..val.len() - 1].to_string();
        }
        if val.is_empty() || val == "|" || val == ">" || val.starts_with('|') || val.starts_with('>')
        {
            return None;
        }
        return Some(val);
    }
    None
}

fn decode_data_url_image(raw: &str) -> Option<(String, Vec<u8>)> {
    let raw = raw.trim();
    let rest = raw.strip_prefix("data:")?;
    let (meta, b64) = rest.split_once(',')?;
    if !meta.contains(";base64") {
        return None;
    }
    let mime = meta
        .split(';')
        .next()
        .unwrap_or("image/png")
        .trim()
        .to_string();
    if !mime.starts_with("image/") {
        return None;
    }
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some((mime, bytes))
}

fn mime_to_ext(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "png",
    }
}

/// Cloud Skill Hub category enum (client API §3.1). Empty / unknown → `creative-social`.
fn normalize_skill_hub_category(raw: &str) -> String {
    const ALLOWED: &[&str] = &[
        "short-drama",
        "film",
        "advertising",
        "creative-social",
        "music-mv",
        // legacy
        "travel",
        "action",
        "drama",
        "aesthetic",
        "product",
        "documentary",
    ];
    let trimmed = raw.trim().to_ascii_lowercase();
    if ALLOWED.contains(&trimmed.as_str()) {
        trimmed
    } else {
        "creative-social".into()
    }
}

async fn download_url_to_file(url: &str, dest: &Path) -> Result<(), AppError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| AppError::Internal(format!("http client: {e}")))?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("download package: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::Internal(format!(
            "download package failed: HTTP {}",
            resp.status()
        )));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| AppError::Internal(format!("download body: {e}")))?;
    if bytes.len() as u64 > MAX_TV_SHOW_PACKAGE_BYTES {
        return Err(AppError::BadRequest(format!(
            "downloaded package too large ({} bytes)",
            bytes.len()
        )));
    }
    tokio::fs::write(dest, &bytes)
        .await
        .map_err(|e| AppError::Internal(format!("write package: {e}")))?;
    Ok(())
}

async fn download_url_to_file_capped(
    url: &str,
    dest: &Path,
    max_bytes: u64,
) -> Result<(), AppError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| AppError::Internal(format!("http client: {e}")))?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("download skill package: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::Internal(format!(
            "download skill package failed: HTTP {}",
            resp.status()
        )));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| AppError::Internal(format!("download body: {e}")))?;
    if bytes.len() as u64 > max_bytes {
        return Err(AppError::BadRequest(format!(
            "downloaded skill package too large ({} bytes); max is {max_bytes}",
            bytes.len()
        )));
    }
    tokio::fs::write(dest, &bytes)
        .await
        .map_err(|e| AppError::Internal(format!("write skill package: {e}")))?;
    Ok(())
}

fn load_flowy(data_dir: &Path) -> Option<FlowyVimaxServices> {
    let cfg: GatewayConfig = load_user_config_file(&config_yaml_path(Some(data_dir))).ok()?;
    FlowyVimaxServices::try_new(&cfg, data_dir)
}

fn map_vimax_err(e: nomi_vimax::VimaxError) -> AppError {
    match e {
        nomi_vimax::VimaxError::SessionNotFound(id) => AppError::NotFound(format!("session {id}")),
        nomi_vimax::VimaxError::InvalidParams(m) => AppError::BadRequest(m),
        nomi_vimax::VimaxError::NotAuthenticated => AppError::Unauthorized(e.to_string()),
        other => AppError::Internal(other.to_string()),
    }
}

fn map_cloud_err(err: nomifun_cloud::ServerClientError) -> AppError {
    err.into_app_error()
}

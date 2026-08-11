//! TV Show cloud browse + generic package publish (+ montage adapter).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nomi_config::{GatewayConfig, config_yaml_path, load_user_config_file};
use nomi_montage::{MontageError, MontageService};
use nomifun_api_types::{
    TvShowLikeResponse, TvShowListResponse, TvShowPublishRequest, TvShowPublishResponse,
    TvShowPublishSessionRequest, TvShowVideo,
};
use nomifun_cloud::{FlowyApiClient, ServerSession};
use nomifun_common::AppError;
use tracing::{info, warn};

/// Soft cap when buffering a `.nomimontage` archive for OSS upload.
const MAX_TV_SHOW_PACKAGE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub struct TvShowService {
    data_dir: PathBuf,
}

impl TvShowService {
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

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
                "cloud login required to use TV Show".into(),
            ));
        }
        let client =
            FlowyApiClient::new(&cfg.server).map_err(|e| AppError::Internal(e.to_string()))?;
        Ok((client, session))
    }

    pub async fn list(
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

    pub async fn mine(
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

    pub async fn detail(&self, id: i64) -> Result<TvShowVideo, AppError> {
        let (client, session) = self.flowy_client_and_session().await?;
        client
            .tv_show_detail(&session, id)
            .await
            .map_err(map_cloud_err)
    }

    pub async fn like(&self, id: i64) -> Result<TvShowLikeResponse, AppError> {
        let (client, session) = self.flowy_client_and_session().await?;
        client
            .tv_show_like(&session, id)
            .await
            .map_err(map_cloud_err)
    }

    pub async fn unlike(&self, id: i64) -> Result<TvShowLikeResponse, AppError> {
        let (client, session) = self.flowy_client_and_session().await?;
        client
            .tv_show_unlike(&session, id)
            .await
            .map_err(map_cloud_err)
    }

    pub async fn delete(&self, id: i64) -> Result<(), AppError> {
        let (client, session) = self.flowy_client_and_session().await?;
        client
            .tv_show_delete(&session, id)
            .await
            .map_err(map_cloud_err)
    }

    /// Publish an already-exported package + cover from local paths.
    pub async fn publish_package(
        &self,
        req: PublishPackageRequest,
    ) -> Result<TvShowPublishResponse, AppError> {
        let package_path = PathBuf::from(req.package_path.trim());
        let cover_path = PathBuf::from(req.cover_path.trim());
        if !package_path.is_file() {
            return Err(AppError::BadRequest(format!(
                "package missing: {}",
                package_path.display()
            )));
        }
        if !cover_path.is_file() {
            return Err(AppError::BadRequest(format!(
                "cover missing: {}",
                cover_path.display()
            )));
        }

        let package_meta = tokio::fs::metadata(&package_path)
            .await
            .map_err(|e| AppError::Internal(format!("package stat: {e}")))?;
        if package_meta.len() > MAX_TV_SHOW_PACKAGE_BYTES {
            return Err(AppError::BadRequest(format!(
                "project package too large ({} bytes); max is {MAX_TV_SHOW_PACKAGE_BYTES}",
                package_meta.len()
            )));
        }

        let (client, cloud_session) = self.flowy_client_and_session().await?;

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

        info!(bytes = cover_bytes.len(), "TV Show: uploading cover");
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
            .unwrap_or("Untitled");
        let safe_title = sanitize_archive_stem(title_raw);
        let package_name = req
            .package_name
            .unwrap_or_else(|| format!("{safe_title}.nomimontage"));

        let package_bytes = tokio::fs::read(&package_path)
            .await
            .map_err(|e| AppError::Internal(format!("read package: {e}")))?;

        info!(
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

        let body = TvShowPublishRequest {
            client_session_id: req.client_session_id,
            title,
            description,
            workflow: req.workflow,
            style: req.style,
            target_duration_secs: req.target_duration_secs,
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

    /// Export a Montage project to `.nomimontage`, pick cover/final, then publish.
    pub async fn publish_from_montage(
        &self,
        montage: &Arc<MontageService>,
        project_id: &str,
        req: TvShowPublishSessionRequest,
    ) -> Result<TvShowPublishResponse, AppError> {
        let detail = montage
            .get_project(project_id)
            .await
            .map_err(map_montage_err)?;
        let status = montage.status(project_id).await.map_err(map_montage_err)?;
        if status.status != "succeeded" {
            return Err(AppError::BadRequest(
                "only succeeded montage projects can be published to TV Show".into(),
            ));
        }

        let film = montage
            .scan_creative_film(project_id)
            .await
            .map_err(map_montage_err)?;
        if film.final_video.is_none() {
            return Err(AppError::BadRequest(
                "finished video is required before publishing".into(),
            ));
        }
        let cover = pick_cover_from_film(&film).ok_or_else(|| {
            AppError::BadRequest(
                "cover image is required before publishing (no image asset found)".into(),
            )
        })?;

        let tmp_path = std::env::temp_dir().join(format!(
            "montage-tv-show-{}-{}.nomimontage",
            project_id,
            uuid_simple()
        ));
        let export_path = montage
            .export_zip(project_id, &tmp_path)
            .await
            .map_err(map_montage_err)?;

        let title = req
            .title
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| detail.record.title.clone());
        let style = detail.record.style_playbook.clone();

        let result = self
            .publish_package(PublishPackageRequest {
                package_path: export_path.to_string_lossy().into_owned(),
                cover_path: cover.abs_path.to_string_lossy().into_owned(),
                client_session_id: project_id.to_string(),
                title: Some(title),
                description: req.description,
                workflow: detail.record.pipeline.clone(),
                style,
                target_duration_secs: None,
                package_name: None,
            })
            .await;

        if let Err(e) = tokio::fs::remove_file(&export_path).await {
            warn!(
                path = %export_path.display(),
                error = %e,
                "failed to remove temp TV Show package"
            );
        }
        result
    }

    /// Download a TV Show `.nomimontage` package and import it as a new Montage project.
    pub async fn import_to_montage(
        &self,
        montage: &Arc<MontageService>,
        tv_show_id: i64,
    ) -> Result<nomi_montage::project::ProjectRecord, AppError> {
        let detail = self.detail(tv_show_id).await?;
        let package_url = detail
            .package_url
            .filter(|u| !u.trim().is_empty())
            .ok_or_else(|| {
                AppError::BadRequest(
                    "TV Show package is not available for import (missing packageUrl)".into(),
                )
            })?;

        let tmp_path = std::env::temp_dir().join(format!(
            "tv-show-import-{}-{}.nomimontage",
            tv_show_id,
            uuid_simple()
        ));

        download_url_to_file(&package_url, &tmp_path).await?;

        let result = montage
            .import_project(&tmp_path)
            .await
            .map_err(map_montage_err);

        if let Err(e) = tokio::fs::remove_file(&tmp_path).await {
            warn!(
                path = %tmp_path.display(),
                error = %e,
                "failed to remove temp TV Show import package"
            );
        }

        let record = result?;
        info!(
            tv_show_id,
            project_id = %record.id,
            "imported TV Show package as Montage project"
        );
        Ok(record)
    }
}

#[derive(Debug, Clone)]
pub struct PublishPackageRequest {
    pub package_path: String,
    pub cover_path: String,
    pub client_session_id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub workflow: String,
    pub style: Option<String>,
    pub target_duration_secs: Option<i32>,
    pub package_name: Option<String>,
}

fn pick_cover_from_film(
    film: &nomi_montage::creative::CreativeFilm,
) -> Option<nomi_montage::creative::CreativeMediaRef> {
    for scene in &film.scenes {
        for shot in &scene.shots {
            if let Some(media) = &shot.media {
                if matches!(
                    media.kind,
                    nomi_montage::creative::CreativeMediaKind::Image
                ) {
                    return Some(media.clone());
                }
            }
        }
    }
    // Fall back: any media (even video first frame is not extracted here).
    film.scenes
        .iter()
        .flat_map(|s| s.shots.iter())
        .find_map(|s| s.media.clone())
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

async fn download_url_to_file(url: &str, out_path: &Path) -> Result<(), AppError> {
    let resp = reqwest::Client::new()
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("download package: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::BadRequest(format!(
            "download package failed: HTTP {}",
            resp.status()
        )));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| AppError::Internal(format!("download package body: {e}")))?;
    if let Some(parent) = out_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| AppError::Internal(format!("create temp dir: {e}")))?;
    }
    tokio::fs::write(out_path, &bytes)
        .await
        .map_err(|e| AppError::Internal(format!("write temp package: {e}")))?;
    Ok(())
}

fn map_montage_err(e: MontageError) -> AppError {
    match e {
        MontageError::ProjectNotFound(id) => AppError::NotFound(format!("project {id}")),
        MontageError::InvalidParams(m) | MontageError::Message(m) => AppError::BadRequest(m),
        MontageError::NotAuthenticated => AppError::Unauthorized(e.to_string()),
        MontageError::Busy(id) => AppError::BadRequest(format!("project busy: {id}")),
        other => AppError::Internal(other.to_string()),
    }
}

fn map_cloud_err(err: nomifun_cloud::ServerClientError) -> AppError {
    let msg = err.to_string();
    let lower = msg.to_ascii_lowercase();
    if lower.contains("unauthorized")
        || lower.contains("401")
        || lower.contains("not authenticated")
        || lower.contains("jwt")
    {
        return AppError::Unauthorized(msg);
    }
    if lower.contains("403") || lower.contains("forbidden") {
        return AppError::Forbidden(msg);
    }
    if lower.contains("404") || lower.contains("not_found") || lower.contains("not found") {
        return AppError::NotFound(msg);
    }
    if lower.contains("400") || lower.contains("invalid") {
        return AppError::BadRequest(msg);
    }
    AppError::Internal(msg)
}

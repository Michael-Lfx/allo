//! Canvas project archive (`.nomiccanvas`) — ZIP of the canvas document,
//! sidecar extras, drawings, and every referenced media binary.
//!
//! Layout:
//! - `manifest.json` — `{ version, app, exported_at, project_id, title }`
//! - `meta.json` — gallery metadata (imported with a new id)
//! - `doc.json` — canvas document
//! - `sidecar.json` — optional chat / director extras
//! - `drawings/**` — drawing documents + preview / generation renders
//! - `media/{id}.{ext}` — referenced binaries
//! - `media-index.json` — packed media metadata (old ids)

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use nomi_config::{GatewayConfig, config_yaml_path, load_user_config_file};
use nomifun_api_types::{TvShowPublishRequest, TvShowPublishResponse, TvShowPublishSessionRequest};
use nomifun_cloud::{FlowyApiClient, ServerSession};
use nomifun_common::AppError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{info, warn};
use zip::CompressionMethod;
use zip::read::ZipArchive;
use zip::write::{SimpleFileOptions, ZipWriter};

use crate::dto::CanvasProjectMeta;
use crate::fsio::{ensure_dir, read_json_file, write_json_file};
use crate::service::{CanvasService, MediaIndexEntry, validate_project_id};

pub const ARCHIVE_APP: &str = "nomifun-canvas";
pub const ARCHIVE_VERSION: u32 = 1;
pub const ARCHIVE_EXTENSION: &str = "nomiccanvas";
/// Shared Flowy TV plaza; canvas projects publish as `workflow: "canvas"`.
pub const TV_SHOW_WORKFLOW: &str = "canvas";

const MANIFEST_ENTRY: &str = "manifest.json";
const META_ENTRY: &str = "meta.json";
const DOC_ENTRY: &str = "doc.json";
const SIDECAR_ENTRY: &str = "sidecar.json";
const MEDIA_INDEX_ENTRY: &str = "media-index.json";
const MEDIA_PREFIX: &str = "media/";
const DRAWINGS_PREFIX: &str = "drawings/";

const MAX_ENTRIES: usize = 50_000;
const MAX_ENTRY_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_TV_SHOW_PACKAGE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub(crate) const MAX_EXTRAS_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveManifest {
    pub version: u32,
    pub app: String,
    pub exported_at: String,
    pub project_id: String,
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PackedMedia {
    media_id: String,
    kind: String,
    title: String,
    mime: String,
    ext: String,
    bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PackedMediaIndex {
    #[serde(default)]
    items: Vec<PackedMedia>,
}

pub struct ImportedCanvasProject {
    pub meta: CanvasProjectMeta,
}

impl CanvasService {
    /// Export a canvas project (doc + extras + referenced media) to `.nomiccanvas`.
    pub async fn export_project(
        &self,
        id: &str,
        dest_path: impl AsRef<Path>,
    ) -> Result<PathBuf, AppError> {
        validate_project_id(id)?;
        let project = self.get_project(id).await?;
        let dest = normalize_dest_path(dest_path.as_ref())?;
        let packed = self.collect_packed_media(id, &project.doc).await?;
        let project_dir = self.project_dir(id);
        let sidecar = optional_json_file(&project_dir.join(SIDECAR_ENTRY)).await?;
        let drawing_files = list_drawing_files(&project_dir).await?;

        let project_id = project.meta.project_id.clone();
        let title = project.meta.title.clone();
        let meta = project.meta.clone();
        let doc = project.doc.clone();
        tokio::task::spawn_blocking(move || {
            write_archive(
                &dest,
                ArchiveWriteInput {
                    project_id,
                    title,
                    meta,
                    doc,
                    sidecar,
                    packed,
                    drawing_files,
                },
            )
        })
        .await
        .map_err(|e| AppError::Internal(format!("export join: {e}")))?
    }

    /// Import a `.nomiccanvas` archive as a new local canvas project.
    pub async fn import_project(
        &self,
        archive_path: impl AsRef<Path>,
    ) -> Result<ImportedCanvasProject, AppError> {
        self.ensure_dirs().await?;
        let archive_path = archive_path.as_ref().to_path_buf();
        if !archive_path.is_file() {
            return Err(AppError::BadRequest(format!(
                "archive not found: {}",
                archive_path.display()
            )));
        }
        let staging = self.root().join("scratch").join(format!(
            "canvas-import-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let extracted = tokio::task::spawn_blocking({
            let staging = staging.clone();
            let archive_path = archive_path.clone();
            move || extract_archive(&archive_path, &staging)
        })
        .await
        .map_err(|e| AppError::Internal(format!("import join: {e}")))?;
        let extracted = match extracted {
            Ok(v) => v,
            Err(e) => {
                let _ = tokio::fs::remove_dir_all(&staging).await;
                return Err(e);
            }
        };

        let import_result = self.materialize_extracted(extracted).await;
        if let Err(e) = tokio::fs::remove_dir_all(&staging).await {
            warn!(
                path = %staging.display(),
                error = %e,
                "failed to remove canvas import staging"
            );
        }
        import_result
    }

    /// Replace project extras (sidecar + drawings) from a ZIP produced by the client.
    pub async fn put_project_extras(
        &self,
        id: &str,
        zip_bytes: Vec<u8>,
    ) -> Result<(), AppError> {
        validate_project_id(id)?;
        if zip_bytes.len() as u64 > MAX_EXTRAS_BYTES {
            return Err(AppError::BadRequest(format!(
                "extras package too large ({} bytes); max is {MAX_EXTRAS_BYTES}",
                zip_bytes.len()
            )));
        }
        let dir = self.project_dir(id);
        if !dir.join("meta.json").exists() {
            return Err(AppError::NotFound(format!("project {id}")));
        }
        tokio::task::spawn_blocking(move || extract_extras_zip(&dir, &zip_bytes))
            .await
            .map_err(|e| AppError::Internal(format!("extras join: {e}")))?
    }

    /// ZIP of sidecar.json + drawings/ (empty archive when neither exists).
    pub async fn project_extras_zip(&self, id: &str) -> Result<Vec<u8>, AppError> {
        validate_project_id(id)?;
        let dir = self.project_dir(id);
        if !dir.join("meta.json").exists() {
            return Err(AppError::NotFound(format!("project {id}")));
        }
        let sidecar = optional_json_file(&dir.join(SIDECAR_ENTRY)).await?;
        let drawing_files = list_drawing_files(&dir).await?;
        tokio::task::spawn_blocking(move || build_extras_zip(sidecar, drawing_files))
            .await
            .map_err(|e| AppError::Internal(format!("extras zip join: {e}")))?
    }

    /// Package the canvas project and publish it to the shared Flowy TV plaza.
    pub async fn publish_project_to_tv_show(
        &self,
        id: &str,
        req: TvShowPublishSessionRequest,
    ) -> Result<TvShowPublishResponse, AppError> {
        let project = self.get_project(id).await?;
        let cover = self
            .pick_cover_media(id, &project.doc)
            .await?
            .ok_or_else(|| {
                AppError::BadRequest(
                    "cover image is required before publishing; add a generated or uploaded image to the canvas"
                        .into(),
                )
            })?;

        let (client, cloud_session) = self.flowy_client_and_session().await?;
        let cover_bytes = tokio::fs::read(&cover.path)
            .await
            .map_err(|e| AppError::Internal(format!("read cover: {e}")))?;
        let cover_name = cover
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("cover.png");
        let cover_mime = if cover.mime.starts_with("image/") {
            cover.mime.clone()
        } else {
            "image/png".into()
        };

        info!(project_id = %id, bytes = cover_bytes.len(), "TV Show: uploading canvas cover");
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
            .unwrap_or(project.meta.title.trim());
        let title_raw = if title_raw.is_empty() {
            "Untitled"
        } else {
            title_raw
        };
        let safe_title = sanitize_archive_stem(title_raw);
        let package_name = format!("{safe_title}.{ARCHIVE_EXTENSION}");
        let tmp_path = std::env::temp_dir().join(format!(
            "canvas-tv-show-{}-{}.{ARCHIVE_EXTENSION}",
            id,
            uuid::Uuid::new_v4().simple()
        ));
        let export_path = self.export_project(id, &tmp_path).await?;
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
                "failed to remove temp canvas TV Show package"
            );
        }

        info!(
            project_id = %id,
            bytes = package_bytes.len(),
            "TV Show: uploading canvas project package"
        );
        let package_upload = client
            .upload_package_via_oss(&cloud_session, &package_bytes, &package_name)
            .await
            .map_err(map_cloud_err)?;
        info!(
            project_id = %id,
            bytes = package_upload.byte_size,
            "TV Show: canvas package uploaded, publishing metadata"
        );

        let title = title_raw.chars().take(200).collect::<String>();
        let description = req
            .description
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.chars().take(1000).collect::<String>());
        let target_duration_secs = first_video_duration_secs(&project.doc);

        let body = TvShowPublishRequest {
            client_session_id: project.meta.project_id.clone(),
            title,
            description,
            workflow: TV_SHOW_WORKFLOW.into(),
            style: None,
            target_duration_secs,
            cover_url: cover_upload.public_url,
            cover_object_key: cover_upload.object_key,
            package_url: package_upload.public_url,
            package_object_key: package_upload.object_key,
            package_size_bytes: Some(package_upload.byte_size as i64),
            package_sha256: None,
            archive_version: Some(ARCHIVE_VERSION as i32),
        };

        client
            .tv_show_publish(&cloud_session, &body)
            .await
            .map_err(map_cloud_err)
    }

    /// Download a Flowy TV package and import it as a new local canvas project.
    pub async fn import_tv_show(&self, id: i64) -> Result<ImportedCanvasProject, AppError> {
        let (client, session) = self.flowy_client_and_session().await?;
        let detail = client
            .tv_show_detail(&session, id)
            .await
            .map_err(map_cloud_err)?;
        let package_url = detail
            .package_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AppError::BadRequest("TV Show package URL unavailable".into()))?;

        let tmp_path = std::env::temp_dir().join(format!(
            "canvas-tv-show-import-{}-{}.{ARCHIVE_EXTENSION}",
            id,
            uuid::Uuid::new_v4().simple()
        ));
        download_url_to_file(package_url, &tmp_path).await?;
        let imported = self.import_project(&tmp_path).await;
        if let Err(e) = tokio::fs::remove_file(&tmp_path).await {
            warn!(
                path = %tmp_path.display(),
                error = %e,
                "failed to remove imported canvas TV Show temp package"
            );
        }
        imported
    }

    async fn flowy_client_and_session(&self) -> Result<(FlowyApiClient, ServerSession), AppError> {
        let cfg: GatewayConfig =
            load_user_config_file(&config_yaml_path(Some(self.data_dir()))).map_err(|e| {
                AppError::BadRequest(format!("failed to load config: {e}"))
            })?;
        if !cfg.server.api_ready() {
            return Err(AppError::BadRequest(
                "server base_url not configured".into(),
            ));
        }
        let session = ServerSession::from_config(&cfg.server, self.data_dir());
        let token = session
            .access_token()
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?
            .filter(|t| !t.trim().is_empty());
        if token.is_none() {
            return Err(AppError::Unauthorized("cloud login required".into()));
        }
        let client =
            FlowyApiClient::new(&cfg.server).map_err(|e| AppError::Internal(e.to_string()))?;
        Ok((client, session))
    }

    async fn collect_packed_media(
        &self,
        project_id: &str,
        doc: &Value,
    ) -> Result<Vec<(PackedMedia, PathBuf)>, AppError> {
        let project_dir = self.project_dir(project_id);
        let mut ids = collect_media_ids(doc);
        if let Some(sidecar) = optional_json_file(&project_dir.join(SIDECAR_ENTRY)).await? {
            ids.extend(collect_media_ids(&sidecar));
        }
        for (rel, path) in list_drawing_files(&project_dir).await? {
            if !rel.ends_with(".json") {
                continue;
            }
            if let Some(value) = optional_json_file(&path).await? {
                ids.extend(collect_media_ids(&value));
            }
        }
        let idx = self.load_media_index().await?;
        let mut out = Vec::new();
        for id in ids {
            let Some(entry) = idx.items.iter().find(|e| e.media_id == id) else {
                warn!(media_id = %id, "canvas export skipped missing media");
                continue;
            };
            let path = self
                .media_dir()
                .join(format!("{}.{}", entry.media_id, entry.ext));
            if !path.is_file() {
                warn!(media_id = %id, "canvas export skipped missing media file");
                continue;
            }
            out.push((packed_from_entry(entry), path));
        }
        Ok(out)
    }

    async fn pick_cover_media(
        &self,
        project_id: &str,
        doc: &Value,
    ) -> Result<Option<CoverFile>, AppError> {
        let idx = self.load_media_index().await?;
        let mut seen = BTreeSet::new();
        for id in preferred_cover_ids(doc) {
            if !seen.insert(id.clone()) {
                continue;
            }
            if let Some(cover) = cover_file_from_index(&idx, &self.media_dir(), &id) {
                return Ok(Some(cover));
            }
        }
        self.pick_drawing_cover(project_id).await
    }

    async fn pick_drawing_cover(&self, project_id: &str) -> Result<Option<CoverFile>, AppError> {
        let files = list_drawing_files(&self.project_dir(project_id)).await?;
        let mut preview: Option<PathBuf> = None;
        for (rel, path) in files {
            let name = Path::new(&rel)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            if !path.is_file() {
                continue;
            }
            if name.eq_ignore_ascii_case("generation.png") {
                return Ok(Some(CoverFile {
                    path,
                    mime: "image/png".into(),
                }));
            }
            if preview.is_none() && name.eq_ignore_ascii_case("preview.png") {
                preview = Some(path);
            }
        }
        Ok(preview.map(|path| CoverFile {
            path,
            mime: "image/png".into(),
        }))
    }

    async fn materialize_extracted(
        &self,
        extracted: ExtractedArchive,
    ) -> Result<ImportedCanvasProject, AppError> {
        let mut id_map: BTreeMap<String, String> = BTreeMap::new();
        for item in &extracted.media {
            let src = extracted.staging.join(MEDIA_PREFIX).join(format!(
                "{}.{}",
                item.media_id, item.ext
            ));
            if !src.is_file() {
                warn!(media_id = %item.media_id, "imported archive missing media file");
                continue;
            }
            let ingested = self
                .ingest_local_file(
                    &src,
                    &item.kind,
                    &item.mime,
                    &item.ext,
                    item.title.clone(),
                )
                .await?;
            id_map.insert(item.media_id.clone(), ingested.media_id);
        }

        let mut doc = remap_media_ids(extracted.doc, &id_map);
        canonicalize_media_urls(&mut doc);
        let title = doc
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(extracted.meta.title.trim());
        let title = if title.is_empty() {
            extracted.manifest.title.clone()
        } else {
            title.to_string()
        };

        let meta = self.create_project(Some(title)).await?;
        if let Err(e) = self.put_doc(&meta.project_id, doc).await {
            let _ = self.delete_project(&meta.project_id).await;
            return Err(e);
        }

        let dest_dir = self.project_dir(&meta.project_id);
        if let Some(mut sidecar) = extracted.sidecar {
            sidecar = remap_media_ids(sidecar, &id_map);
            canonicalize_media_urls(&mut sidecar);
            if let Err(e) = write_json_file(&dest_dir.join(SIDECAR_ENTRY), &sidecar).await {
                warn!(error = %e, "failed to restore imported canvas sidecar");
            }
        }

        let drawings_src = extracted.staging.join("drawings");
        if drawings_src.is_dir() {
            let drawings_dest = dest_dir.join("drawings");
            if let Err(e) = copy_dir_recursive(&drawings_src, &drawings_dest).await {
                warn!(error = %e, "failed to restore imported canvas drawings");
            } else {
                remap_json_files_in_dir(&drawings_dest, &id_map).await;
            }
        }

        let meta = self.get_project(&meta.project_id).await?.meta;
        Ok(ImportedCanvasProject { meta })
    }
}

struct CoverFile {
    path: PathBuf,
    mime: String,
}

struct ArchiveWriteInput {
    project_id: String,
    title: String,
    meta: CanvasProjectMeta,
    doc: Value,
    sidecar: Option<Value>,
    packed: Vec<(PackedMedia, PathBuf)>,
    drawing_files: Vec<(String, PathBuf)>,
}

struct ExtractedArchive {
    manifest: ArchiveManifest,
    meta: CanvasProjectMeta,
    doc: Value,
    sidecar: Option<Value>,
    media: Vec<PackedMedia>,
    staging: PathBuf,
}

fn packed_from_entry(entry: &MediaIndexEntry) -> PackedMedia {
    PackedMedia {
        media_id: entry.media_id.clone(),
        kind: entry.kind.clone(),
        title: entry.title.clone(),
        mime: entry.mime.clone(),
        ext: entry.ext.clone(),
        bytes: entry.bytes,
        width: entry.width,
        height: entry.height,
        duration_ms: entry.duration_ms,
    }
}

fn write_archive(dest: &Path, input: ArchiveWriteInput) -> Result<PathBuf, AppError> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| AppError::Internal(format!("mkdir export dest: {e}")))?;
    }
    let tmp = dest.with_extension(format!("{ARCHIVE_EXTENSION}.tmp"));
    if tmp.exists() {
        let _ = fs::remove_file(&tmp);
    }
    let file =
        File::create(&tmp).map_err(|e| AppError::Internal(format!("create archive: {e}")))?;
    let mut zip = ZipWriter::new(file);
    let mut file_count = 0usize;
    let mut total_bytes: u64 = 0;

    let mut meta = input.meta;
    meta.source_vimax_session_id = None;

    let manifest = ArchiveManifest {
        version: ARCHIVE_VERSION,
        app: ARCHIVE_APP.into(),
        exported_at: chrono::Local::now().to_rfc3339(),
        project_id: input.project_id,
        title: input.title,
    };
    write_json_entry(&mut zip, MANIFEST_ENTRY, &manifest)?;
    write_json_entry(&mut zip, META_ENTRY, &meta)?;
    write_json_entry(&mut zip, DOC_ENTRY, &input.doc)?;
    if let Some(sidecar) = &input.sidecar {
        write_json_entry(&mut zip, SIDECAR_ENTRY, sidecar)?;
    }
    let media_index = PackedMediaIndex {
        items: input.packed.iter().map(|(m, _)| m.clone()).collect(),
    };
    write_json_entry(&mut zip, MEDIA_INDEX_ENTRY, &media_index)?;

    for (media, path) in &input.packed {
        let name = format!("{MEDIA_PREFIX}{}.{}", media.media_id, media.ext);
        append_file(
            &mut zip,
            &name,
            path,
            &mut file_count,
            &mut total_bytes,
        )?;
    }
    for (rel, path) in &input.drawing_files {
        let name = format!("{DRAWINGS_PREFIX}{rel}");
        append_file(
            &mut zip,
            &name,
            path,
            &mut file_count,
            &mut total_bytes,
        )?;
    }

    zip.finish()
        .map_err(|e| AppError::Internal(format!("zip finish: {e}")))?;
    if dest.exists() {
        fs::remove_file(dest).map_err(|e| AppError::Internal(format!("replace archive: {e}")))?;
    }
    fs::rename(&tmp, dest).map_err(|e| AppError::Internal(format!("rename archive: {e}")))?;
    Ok(dest.to_path_buf())
}

fn extract_archive(archive_path: &Path, staging: &Path) -> Result<ExtractedArchive, AppError> {
    if staging.exists() {
        fs::remove_dir_all(staging)
            .map_err(|e| AppError::Internal(format!("clear staging: {e}")))?;
    }
    fs::create_dir_all(staging).map_err(|e| AppError::Internal(format!("mkdir staging: {e}")))?;

    let file = File::open(archive_path)
        .map_err(|e| AppError::BadRequest(format!("open archive: {e}")))?;
    let mut zip = ZipArchive::new(file)
        .map_err(|e| AppError::BadRequest(format!("invalid canvas archive: {e}")))?;

    let mut manifest_bytes = None;
    let mut meta_bytes = None;
    let mut doc_bytes = None;
    let mut sidecar_bytes = None;
    let mut media_index_bytes = None;
    let mut total = 0u64;
    let mut file_count = 0usize;

    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| AppError::BadRequest(format!("archive entry: {e}")))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().replace('\\', "/");
        if name.contains("..") || name.starts_with('/') {
            return Err(AppError::BadRequest(format!(
                "invalid archive path: {name}"
            )));
        }
        file_count += 1;
        if file_count > MAX_ENTRIES {
            return Err(AppError::BadRequest(format!(
                "archive has too many entries (>{MAX_ENTRIES})"
            )));
        }
        let cap = MAX_ENTRY_BYTES.min(MAX_TOTAL_BYTES.saturating_sub(total));
        match name.as_str() {
            MANIFEST_ENTRY => manifest_bytes = Some(read_entry_capped(&mut entry, cap, &mut total)?),
            META_ENTRY => meta_bytes = Some(read_entry_capped(&mut entry, cap, &mut total)?),
            DOC_ENTRY => doc_bytes = Some(read_entry_capped(&mut entry, cap, &mut total)?),
            SIDECAR_ENTRY => sidecar_bytes = Some(read_entry_capped(&mut entry, cap, &mut total)?),
            MEDIA_INDEX_ENTRY => {
                media_index_bytes = Some(read_entry_capped(&mut entry, cap, &mut total)?)
            }
            other if other.starts_with(MEDIA_PREFIX) || other.starts_with(DRAWINGS_PREFIX) => {
                let rel = other.to_string();
                let dest = staging.join(&rel);
                stream_copy_bounded(&mut entry, &dest, cap, &mut total)?;
            }
            _ => {
                discard_bounded(&mut entry, cap, &mut total)?;
            }
        }
    }

    let manifest: ArchiveManifest = serde_json::from_slice(&manifest_bytes.ok_or_else(|| {
        AppError::BadRequest("canvas archive missing manifest.json".into())
    })?)
    .map_err(|e| AppError::BadRequest(format!("invalid manifest: {e}")))?;
    if manifest.app != ARCHIVE_APP {
        return Err(AppError::BadRequest(format!(
            "not a canvas project archive (app={})",
            manifest.app
        )));
    }
    if manifest.version == 0 || manifest.version > ARCHIVE_VERSION {
        return Err(AppError::BadRequest(format!(
            "unsupported canvas archive version {}",
            manifest.version
        )));
    }
    let meta: CanvasProjectMeta = serde_json::from_slice(&meta_bytes.ok_or_else(|| {
        AppError::BadRequest("canvas archive missing meta.json".into())
    })?)
    .map_err(|e| AppError::BadRequest(format!("invalid meta: {e}")))?;
    let doc: Value = serde_json::from_slice(&doc_bytes.ok_or_else(|| {
        AppError::BadRequest("canvas archive missing doc.json".into())
    })?)
    .map_err(|e| AppError::BadRequest(format!("invalid doc: {e}")))?;
    let sidecar = sidecar_bytes
        .map(|b| serde_json::from_slice(&b))
        .transpose()
        .map_err(|e| AppError::BadRequest(format!("invalid sidecar: {e}")))?;
    let media = media_index_bytes
        .map(|b| serde_json::from_slice::<PackedMediaIndex>(&b))
        .transpose()
        .map_err(|e| AppError::BadRequest(format!("invalid media-index: {e}")))?
        .unwrap_or_default()
        .items;

    Ok(ExtractedArchive {
        manifest,
        meta,
        doc,
        sidecar,
        media,
        staging: staging.to_path_buf(),
    })
}

fn extract_extras_zip(project_dir: &Path, zip_bytes: &[u8]) -> Result<(), AppError> {
    let cursor = std::io::Cursor::new(zip_bytes);
    let mut zip = ZipArchive::new(cursor)
        .map_err(|e| AppError::BadRequest(format!("invalid extras archive: {e}")))?;
    let sidecar_path = project_dir.join(SIDECAR_ENTRY);
    let drawings_dir = project_dir.join("drawings");
    if drawings_dir.exists() {
        fs::remove_dir_all(&drawings_dir)
            .map_err(|e| AppError::Internal(format!("clear drawings: {e}")))?;
    }
    if sidecar_path.exists() {
        let _ = fs::remove_file(&sidecar_path);
    }
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| AppError::BadRequest(format!("extras entry: {e}")))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().replace('\\', "/");
        if name.contains("..") || name.starts_with('/') {
            return Err(AppError::BadRequest(format!("invalid extras path: {name}")));
        }
        let cap = MAX_ENTRY_BYTES.min(MAX_EXTRAS_BYTES.saturating_sub(total));
        if name == SIDECAR_ENTRY {
            let dest = project_dir.join(SIDECAR_ENTRY);
            stream_copy_bounded(&mut entry, &dest, cap, &mut total)?;
        } else if let Some(rel) = name.strip_prefix(DRAWINGS_PREFIX) {
            if rel.is_empty() {
                continue;
            }
            let dest = drawings_dir.join(rel);
            stream_copy_bounded(&mut entry, &dest, cap, &mut total)?;
        } else {
            discard_bounded(&mut entry, cap, &mut total)?;
        }
    }
    Ok(())
}

fn build_extras_zip(
    sidecar: Option<Value>,
    drawing_files: Vec<(String, PathBuf)>,
) -> Result<Vec<u8>, AppError> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut cursor);
        if let Some(sidecar) = sidecar {
            let bytes = serde_json::to_vec_pretty(&sidecar)
                .map_err(|e| AppError::Internal(format!("serialize sidecar: {e}")))?;
            let opts =
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            zip.start_file(SIDECAR_ENTRY, opts)
                .map_err(|e| AppError::Internal(format!("zip sidecar: {e}")))?;
            zip.write_all(&bytes)
                .map_err(|e| AppError::Internal(format!("write sidecar: {e}")))?;
        }
        for (rel, path) in drawing_files {
            let name = format!("{DRAWINGS_PREFIX}{rel}");
            let opts = SimpleFileOptions::default().compression_method(compression_for(&name));
            zip.start_file(&name, opts)
                .map_err(|e| AppError::Internal(format!("zip start {name}: {e}")))?;
            let mut src =
                File::open(&path).map_err(|e| AppError::Internal(format!("open drawing: {e}")))?;
            io::copy(&mut src, &mut zip)
                .map_err(|e| AppError::Internal(format!("copy drawing: {e}")))?;
        }
        zip.finish()
            .map_err(|e| AppError::Internal(format!("extras zip finish: {e}")))?;
    }
    Ok(cursor.into_inner())
}

fn append_file(
    zip: &mut ZipWriter<File>,
    name: &str,
    path: &Path,
    file_count: &mut usize,
    total_bytes: &mut u64,
) -> Result<(), AppError> {
    *file_count += 1;
    if *file_count > MAX_ENTRIES {
        return Err(AppError::BadRequest(format!(
            "project has too many files to export (>{MAX_ENTRIES})"
        )));
    }
    let meta = fs::metadata(path).map_err(|e| AppError::Internal(format!("stat {}: {e}", path.display())))?;
    if meta.len() > MAX_ENTRY_BYTES {
        return Err(AppError::BadRequest(format!(
            "file too large to export ({} > {MAX_ENTRY_BYTES} bytes): {}",
            meta.len(),
            path.display()
        )));
    }
    *total_bytes = total_bytes.saturating_add(meta.len());
    if *total_bytes > MAX_TOTAL_BYTES {
        return Err(AppError::BadRequest(format!(
            "project exceeds export size budget ({MAX_TOTAL_BYTES} bytes)"
        )));
    }
    let opts = SimpleFileOptions::default().compression_method(compression_for(name));
    zip.start_file(name, opts)
        .map_err(|e| AppError::Internal(format!("zip start_file {name}: {e}")))?;
    let mut src =
        File::open(path).map_err(|e| AppError::Internal(format!("open {}: {e}", path.display())))?;
    io::copy(&mut src, zip).map_err(|e| AppError::Internal(format!("copy {name}: {e}")))?;
    Ok(())
}

fn write_json_entry<T: Serialize>(
    zip: &mut ZipWriter<File>,
    name: &str,
    value: &T,
) -> Result<(), AppError> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|e| AppError::Internal(format!("serialize {name}: {e}")))?;
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file(name, opts)
        .map_err(|e| AppError::Internal(format!("zip start_file {name}: {e}")))?;
    zip.write_all(&bytes)
        .map_err(|e| AppError::Internal(format!("write {name}: {e}")))?;
    Ok(())
}

fn read_entry_capped<R: Read>(
    reader: &mut R,
    cap: u64,
    total: &mut u64,
) -> Result<Vec<u8>, AppError> {
    let mut buf = Vec::new();
    reader
        .take(cap.saturating_add(1))
        .read_to_end(&mut buf)
        .map_err(|e| AppError::BadRequest(format!("read archive entry: {e}")))?;
    if buf.len() as u64 > cap {
        return Err(AppError::BadRequest(
            "archive entry exceeds decompression budget".into(),
        ));
    }
    *total = total.saturating_add(buf.len() as u64);
    Ok(buf)
}

fn stream_copy_bounded<R: Read>(
    reader: &mut R,
    dest: &Path,
    cap: u64,
    total: &mut u64,
) -> Result<(), AppError> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| AppError::Internal(format!("mkdir {}: {e}", parent.display())))?;
    }
    let mut out =
        File::create(dest).map_err(|e| AppError::Internal(format!("create {}: {e}", dest.display())))?;
    let written = io::copy(&mut reader.take(cap.saturating_add(1)), &mut out)
        .map_err(|e| AppError::Internal(format!("write {}: {e}", dest.display())))?;
    if written > cap {
        let _ = fs::remove_file(dest);
        return Err(AppError::BadRequest(
            "archive entry exceeds decompression budget".into(),
        ));
    }
    *total = total.saturating_add(written);
    Ok(())
}

fn discard_bounded<R: Read>(reader: &mut R, cap: u64, total: &mut u64) -> Result<(), AppError> {
    let written = io::copy(&mut reader.take(cap.saturating_add(1)), &mut io::sink())
        .map_err(|e| AppError::BadRequest(format!("skip archive entry: {e}")))?;
    if written > cap {
        return Err(AppError::BadRequest(
            "archive entry exceeds decompression budget".into(),
        ));
    }
    *total = total.saturating_add(written);
    Ok(())
}

fn compression_for(rel: &str) -> CompressionMethod {
    match Path::new(rel)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "mp4" | "webm" | "mov" | "mkv" | "avi" | "png" | "jpg" | "jpeg" | "webp" | "gif"
        | "mp3" | "wav" | "aac" | "m4a" | "zip" | "gz" | "bz2" | "7z" => CompressionMethod::Stored,
        _ => CompressionMethod::Deflated,
    }
}

fn normalize_dest_path(dest: &Path) -> Result<PathBuf, AppError> {
    let mut out = dest.to_path_buf();
    let has_ext = out
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case(ARCHIVE_EXTENSION))
        .unwrap_or(false);
    if !has_ext {
        out.set_extension(ARCHIVE_EXTENSION);
    }
    if out
        .file_name()
        .map(|n| n.to_string_lossy().contains(".."))
        .unwrap_or(true)
    {
        return Err(AppError::BadRequest(
            "invalid destination archive path".into(),
        ));
    }
    Ok(out)
}

async fn optional_json_file(path: &Path) -> Result<Option<Value>, AppError> {
    if !path.is_file() {
        return Ok(None);
    }
    match read_json_file::<Value>(path).await {
        Ok(v) => Ok(Some(v)),
        Err(e) => {
            warn!(path = %path.display(), error = %e, "skip unreadable canvas sidecar");
            Ok(None)
        }
    }
}

async fn list_drawing_files(project_dir: &Path) -> Result<Vec<(String, PathBuf)>, AppError> {
    let drawings = project_dir.join("drawings");
    if !drawings.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    collect_files_rel(&drawings, &drawings, &mut out).await?;
    Ok(out)
}

async fn collect_files_rel(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(String, PathBuf)>,
) -> Result<(), AppError> {
    let mut rd = tokio::fs::read_dir(dir)
        .await
        .map_err(|e| AppError::Internal(format!("read drawings: {e}")))?;
    while let Some(entry) = rd
        .next_entry()
        .await
        .map_err(|e| AppError::Internal(format!("read drawings entry: {e}")))?
    {
        let path = entry.path();
        let ft = entry
            .file_type()
            .await
            .map_err(|e| AppError::Internal(format!("drawings file type: {e}")))?;
        if ft.is_dir() {
            Box::pin(collect_files_rel(root, &path, out)).await?;
        } else if ft.is_file() {
            let rel = path
                .strip_prefix(root)
                .map_err(|_| AppError::Internal("drawing path escapes drawings dir".into()))?
                .to_string_lossy()
                .replace('\\', "/");
            if rel.is_empty() || rel.contains("..") {
                continue;
            }
            out.push((rel, path));
        }
    }
    Ok(())
}

async fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), AppError> {
    ensure_dir(dest).await?;
    let mut rd = tokio::fs::read_dir(src)
        .await
        .map_err(|e| AppError::Internal(format!("copy drawings: {e}")))?;
    while let Some(entry) = rd
        .next_entry()
        .await
        .map_err(|e| AppError::Internal(format!("copy drawings entry: {e}")))?
    {
        let from = entry.path();
        let to = dest.join(entry.file_name());
        let ft = entry
            .file_type()
            .await
            .map_err(|e| AppError::Internal(format!("copy drawings type: {e}")))?;
        if ft.is_dir() {
            Box::pin(copy_dir_recursive(&from, &to)).await?;
        } else if ft.is_file() {
            tokio::fs::copy(&from, &to)
                .await
                .map_err(|e| AppError::Internal(format!("copy {}: {e}", from.display())))?;
        }
    }
    Ok(())
}

async fn remap_json_files_in_dir(dir: &Path, id_map: &BTreeMap<String, String>) {
    let Ok(mut rd) = tokio::fs::read_dir(dir).await else {
        return;
    };
    while let Ok(Some(entry)) = rd.next_entry().await {
        let path = entry.path();
        let Ok(ft) = entry.file_type().await else {
            continue;
        };
        if ft.is_dir() {
            Box::pin(remap_json_files_in_dir(&path, id_map)).await;
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(bytes) = tokio::fs::read(&path).await else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        let mut next = remap_media_ids(value, id_map);
        canonicalize_media_urls(&mut next);
        if let Ok(out) = serde_json::to_vec_pretty(&next) {
            let _ = tokio::fs::write(&path, out).await;
        }
    }
}

pub(crate) fn collect_media_ids(value: &Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    walk_collect(value, &mut out);
    out
}

fn walk_collect(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::String(s) => {
            if let Some(id) = parse_media_id_from_string(s) {
                out.insert(id);
            }
        }
        Value::Array(items) => items.iter().for_each(|v| walk_collect(v, out)),
        Value::Object(map) => {
            for (key, child) in map {
                match key.as_str() {
                    "assetId"
                    | "mediaId"
                    | "result_media_id"
                    | "first_frame_media_id"
                    | "last_frame_media_id"
                    | "referenceMediaId" => {
                        if let Some(id) = child.as_str().map(str::trim).filter(|s| is_media_id(s)) {
                            out.insert(id.to_string());
                        }
                    }
                    "reference_media_ids" | "referenceMediaIds" => {
                        if let Some(arr) = child.as_array() {
                            for item in arr {
                                if let Some(id) =
                                    item.as_str().map(str::trim).filter(|s| is_media_id(s))
                                {
                                    out.insert(id.to_string());
                                }
                            }
                        }
                    }
                    _ => walk_collect(child, out),
                }
            }
        }
        _ => {}
    }
}

fn parse_media_id_from_string(raw: &str) -> Option<String> {
    let s = raw.trim();
    if let Some(rest) = s.strip_prefix("resource:") {
        let id = rest.split(['?', '#']).next().unwrap_or(rest).trim();
        return is_media_id(id).then(|| id.to_string());
    }
    if let Some(idx) = s.find("/api/video-canvas/media/") {
        let rest = &s[idx + "/api/video-canvas/media/".len()..];
        let id = rest.split(['?', '#', '/']).next().unwrap_or("");
        let decoded = percent_decode(id);
        return is_media_id(&decoded).then_some(decoded);
    }
    None
}

fn is_media_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && !id.contains('/')
        && !id.contains('\\')
        && !id.contains("..")
        && !id.contains('\0')
}

fn percent_decode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(a), Some(b)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push(char::from((a << 4) | b));
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn remap_media_ids(mut value: Value, id_map: &BTreeMap<String, String>) -> Value {
    if id_map.is_empty() {
        return value;
    }
    walk_remap(&mut value, id_map);
    value
}

fn walk_remap(value: &mut Value, id_map: &BTreeMap<String, String>) {
    match value {
        Value::String(s) => {
            *s = replace_ids(s, id_map);
        }
        Value::Array(items) => items.iter_mut().for_each(|v| walk_remap(v, id_map)),
        Value::Object(map) => map.values_mut().for_each(|v| walk_remap(v, id_map)),
        _ => {}
    }
}

fn replace_ids(input: &str, id_map: &BTreeMap<String, String>) -> String {
    let mut out = input.to_string();
    for (old, new) in id_map {
        if old == new {
            continue;
        }
        out = out.replace(old, new);
    }
    out
}

fn canonicalize_media_urls(value: &mut Value) {
    match value {
        Value::String(s) => {
            *s = strip_media_url_origin(s);
        }
        Value::Array(items) => items.iter_mut().for_each(canonicalize_media_urls),
        Value::Object(map) => map.values_mut().for_each(canonicalize_media_urls),
        _ => {}
    }
}

fn strip_media_url_origin(s: &str) -> String {
    let needle = "/api/video-canvas/media/";
    let Some(idx) = s.find(needle) else {
        return s.to_string();
    };
    let prefix = &s[..idx];
    let url_start = prefix
        .rfind("https://")
        .or_else(|| prefix.rfind("http://"))
        .unwrap_or(idx);
    if url_start < idx {
        format!("{}{}", &s[..url_start], &s[idx..])
    } else {
        s.to_string()
    }
}

fn cover_file_from_index(idx: &crate::service::MediaIndex, media_dir: &Path, id: &str) -> Option<CoverFile> {
    let entry = idx.items.iter().find(|e| e.media_id == id)?;
    if !entry.mime.starts_with("image/") && entry.kind != "image" {
        return None;
    }
    let path = media_dir.join(format!("{}.{}", entry.media_id, entry.ext));
    path.is_file().then(|| CoverFile {
        path,
        mime: if entry.mime.starts_with("image/") {
            entry.mime.clone()
        } else {
            "image/png".into()
        },
    })
}

fn node_is_tv_cover(node: &Value) -> bool {
    node.get("metadata")
        .and_then(|m| m.get("tvCover"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn preferred_cover_ids(doc: &Value) -> Vec<String> {
    let mut pinned = Vec::new();
    let mut images = Vec::new();
    let mut others = Vec::new();
    if let Some(nodes) = doc.get("nodes").and_then(|v| v.as_array()) {
        for node in nodes {
            let ty = node.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let ids = node_cover_candidates(node);
            if ty == "image" && node_is_tv_cover(node) {
                pinned.extend(ids);
            } else if ty == "image" {
                images.extend(ids);
            } else {
                others.extend(ids);
            }
        }
    }
    pinned.extend(images);
    pinned.extend(others);
    pinned
}

/// Canvas media refs first. `assetId` is often a project/library asset id, not
/// a file in `{data_dir}/video-canvas/media/`, so it is only a last resort.
fn node_cover_candidates(node: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    let mut push = |id: String| {
        if is_media_id(&id) && !ids.iter().any(|existing| existing == &id) {
            ids.push(id);
        }
    };
    let meta = node.get("metadata");
    if let Some(meta) = meta {
        if let Some(id) = meta.get("mediaId").and_then(|v| v.as_str()).map(str::trim) {
            push(id.to_string());
        }
        if let Some(id) = meta
            .get("storageKey")
            .and_then(|v| v.as_str())
            .and_then(parse_media_id_from_string)
        {
            push(id);
        }
        if let Some(id) = meta
            .get("content")
            .and_then(|v| v.as_str())
            .and_then(parse_media_id_from_string)
        {
            push(id);
        }
    }
    if let Some(id) = node
        .get("content")
        .and_then(|v| v.as_str())
        .and_then(parse_media_id_from_string)
    {
        push(id);
    }
    if let Some(id) = meta
        .and_then(|m| m.get("assetId"))
        .and_then(|v| v.as_str())
        .map(str::trim)
    {
        push(id.to_string());
    }
    ids
}

fn first_video_duration_secs(doc: &Value) -> Option<i32> {
    let nodes = doc.get("nodes")?.as_array()?;
    for node in nodes {
        if node.get("type").and_then(|v| v.as_str()) != Some("video") {
            continue;
        }
        let ms = node
            .pointer("/metadata/durationMs")
            .and_then(|v| v.as_u64())
            .or_else(|| {
                node.pointer("/metadata/duration_ms")
                    .and_then(|v| v.as_u64())
            })?;
        if ms == 0 {
            continue;
        }
        let secs = ((ms + 500) / 1000).clamp(1, 86_400);
        return Some(secs as i32);
    }
    None
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
        "nomi-canvas".into()
    } else {
        stem
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

fn map_cloud_err(err: nomifun_cloud::ServerClientError) -> AppError {
    err.into_app_error()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cover_prefers_canvas_media_over_project_asset_id() {
        let media = "0190f5fe-7c00-7a00-8000-000000000111";
        let asset = "0190f5fe-7c00-7a00-8000-000000000999";
        let doc = json!({
            "nodes": [{
                "type": "image",
                "metadata": {
                    "assetId": asset,
                    "storageKey": format!("resource:{media}"),
                    "content": format!("/api/video-canvas/media/{media}")
                }
            }]
        });
        let ids = preferred_cover_ids(&doc);
        assert_eq!(ids.first().map(String::as_str), Some(media));
        assert_eq!(ids.last().map(String::as_str), Some(asset));
    }

    #[test]
    fn cover_prefers_pinned_tv_cover_image() {
        let first = "0190f5fe-7c00-7a00-8000-000000000111";
        let pinned = "0190f5fe-7c00-7a00-8000-000000000222";
        let doc = json!({
            "nodes": [
                {
                    "type": "image",
                    "metadata": { "mediaId": first }
                },
                {
                    "type": "image",
                    "metadata": { "tvCover": true, "mediaId": pinned }
                }
            ]
        });
        let ids = preferred_cover_ids(&doc);
        assert_eq!(ids.first().map(String::as_str), Some(pinned));
        assert!(ids.contains(&first.to_string()));
    }

    #[test]
    fn collect_media_ids_from_nodes_and_urls() {
        let id = "0190f5fe-7c00-7a00-8000-000000000111";
        let id2 = "0190f5fe-7c00-7a00-8000-000000000222";
        let doc = json!({
            "nodes": [
                {
                    "type": "image",
                    "metadata": {
                        "assetId": id,
                        "storageKey": format!("resource:{id}"),
                        "content": format!("http://127.0.0.1:9/api/video-canvas/media/{id}")
                    }
                },
                {
                    "type": "video",
                    "metadata": { "mediaId": id2 }
                }
            ]
        });
        let ids = collect_media_ids(&doc);
        assert!(ids.contains(id));
        assert!(ids.contains(id2));
    }

    #[test]
    fn remap_rewrites_resource_keys_and_absolute_urls() {
        let old = "0190f5fe-7c00-7a00-8000-000000000111";
        let new = "0190f5fe-7c00-7a00-8000-000000000999";
        let mut doc = json!({
            "nodes": [{
                "metadata": {
                    "assetId": old,
                    "storageKey": format!("resource:{old}"),
                    "content": format!("http://127.0.0.1:5173/api/video-canvas/media/{old}?x=1")
                }
            }]
        });
        let mut map = BTreeMap::new();
        map.insert(old.to_string(), new.to_string());
        doc = remap_media_ids(doc, &map);
        canonicalize_media_urls(&mut doc);
        let content = doc["nodes"][0]["metadata"]["content"].as_str().unwrap();
        assert_eq!(
            content,
            "/api/video-canvas/media/0190f5fe-7c00-7a00-8000-000000000999?x=1"
        );
        assert_eq!(
            doc["nodes"][0]["metadata"]["storageKey"].as_str().unwrap(),
            "resource:0190f5fe-7c00-7a00-8000-000000000999"
        );
    }

    #[tokio::test]
    async fn export_import_roundtrip_copies_media_and_remaps_ids() {
        let dir = tempfile::tempdir().expect("temp dir");
        let service = CanvasService::new(dir.path().to_path_buf());
        let media_id = service
            .ingest_generated_bytes(b"PNGDATA".to_vec(), "image", "image/png", "png", "cover".into())
            .await
            .expect("ingest");
        let meta = service
            .create_project(Some("雨夜画布".into()))
            .await
            .expect("create");
        let doc = json!({
            "schema": 1,
            "title": "雨夜画布",
            "nodes": [{
                "id": "n1",
                "type": "image",
                "title": "cover",
                "position": { "x": 0, "y": 0 },
                "width": 240,
                "height": 220,
                "metadata": {
                    "assetId": media_id,
                    "storageKey": format!("resource:{media_id}"),
                    "content": format!("/api/video-canvas/media/{media_id}")
                }
            }],
            "connections": [],
            "viewport": { "x": 0, "y": 0, "k": 1 },
            "backgroundMode": "lines"
        });
        service.put_doc(&meta.project_id, doc).await.expect("put doc");
        write_json_file(
            &service.project_dir(&meta.project_id).join("sidecar.json"),
            &json!({ "activeChatId": null, "chatSessions": [] }),
        )
        .await
        .expect("sidecar");

        let archive = dir.path().join("share.nomiccanvas");
        service
            .export_project(&meta.project_id, &archive)
            .await
            .expect("export");
        assert!(archive.is_file());

        let imported = service.import_project(&archive).await.expect("import");
        assert_ne!(imported.meta.project_id, meta.project_id);
        assert_eq!(imported.meta.title, "雨夜画布");
        let restored = service
            .get_project(&imported.meta.project_id)
            .await
            .expect("get imported");
        let restored_id = restored.doc["nodes"][0]["metadata"]["assetId"]
            .as_str()
            .unwrap();
        assert_ne!(restored_id, media_id);
        let path = service.media_file_path(restored_id).await.expect("media");
        assert_eq!(std::fs::read(path).unwrap(), b"PNGDATA");
        assert!(service
            .project_dir(&imported.meta.project_id)
            .join("sidecar.json")
            .is_file());
    }
}

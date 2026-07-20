//! Download remote media and persist under `{data_dir}/media/generated/`.

use std::path::PathBuf;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use nomi_types::ToolError;

/// A locally persisted media file plus metadata for tool responses and `MEDIA:` delivery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaArtifact {
    pub local_path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_url: Option<String>,
    pub mime: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<f32>,
    pub provider: String,
    pub model: String,
    pub job_id: String,
}

impl MediaArtifact {
    pub fn media_tag(&self) -> String {
        format!("MEDIA:{}", self.local_path.display())
    }
}

fn generated_media_root() -> PathBuf {
    let day = chrono::Local::now().format("%Y-%m-%d").to_string();
    nomi_config::data_dir()
        .join("media")
        .join("generated")
        .join(day)
}

fn extension_for_mime(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        _ if mime.starts_with("image/") => "png",
        _ if mime.starts_with("video/") => "mp4",
        _ => "bin",
    }
}

fn guess_mime_from_url(url: &str) -> &'static str {
    let lower = url.to_ascii_lowercase();
    if lower.contains(".png") {
        "image/png"
    } else if lower.contains(".jpg") || lower.contains(".jpeg") {
        "image/jpeg"
    } else if lower.contains(".webp") {
        "image/webp"
    } else if lower.contains(".gif") {
        "image/gif"
    } else if lower.contains(".webm") {
        "video/webm"
    } else if lower.contains(".mp4") {
        "video/mp4"
    } else {
        "application/octet-stream"
    }
}

/// Persist raw bytes to the generated media directory.
pub async fn persist_bytes(
    bytes: &[u8],
    mime: &str,
    provider: &str,
    model: &str,
) -> Result<MediaArtifact, ToolError> {
    let dir = generated_media_root();
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("create media dir: {e}")))?;

    let job_id = Uuid::new_v4().to_string();
    let ext = extension_for_mime(mime);
    let path = dir.join(format!("{job_id}.{ext}"));
    tokio::fs::write(&path, bytes)
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("write media file: {e}")))?;

    let mut artifact = MediaArtifact {
        local_path: path,
        remote_url: None,
        mime: mime.to_string(),
        width: None,
        height: None,
        duration_secs: None,
        provider: provider.to_string(),
        model: model.to_string(),
        job_id,
    };
    fill_image_dimensions(&mut artifact, bytes);
    Ok(artifact)
}

/// Remote-only artifact when local persist failed.
pub fn remote_fallback_artifact(
    url: String,
    mime: &str,
    provider: &str,
    model: &str,
) -> MediaArtifact {
    MediaArtifact {
        local_path: PathBuf::new(),
        remote_url: Some(url),
        mime: mime.to_string(),
        width: None,
        height: None,
        duration_secs: None,
        provider: provider.to_string(),
        model: model.to_string(),
        job_id: Uuid::new_v4().to_string(),
    }
}

fn fill_image_dimensions(artifact: &mut MediaArtifact, bytes: &[u8]) {
    if !artifact.mime.starts_with("image/") {
        return;
    }
    use std::io::Cursor;
    let reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format();
    if let Ok(reader) = reader
        && let Ok(img) = reader.decode()
    {
        artifact.width = Some(img.width());
        artifact.height = Some(img.height());
    }
}

/// Download a remote URL and persist locally.
pub async fn persist_from_url(
    url: &str,
    provider: &str,
    model: &str,
) -> Result<MediaArtifact, ToolError> {
    let client = Client::new();
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("download media: {e}")))?;

    if !resp.status().is_success() {
        return Err(ToolError::ExecutionFailed(format!(
            "download media HTTP {}",
            resp.status()
        )));
    }

    let mime = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or(s).trim())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| guess_mime_from_url(url))
        .to_string();

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("read media body: {e}")))?;

    let mut artifact = persist_bytes(&bytes, &mime, provider, model).await?;
    artifact.remote_url = Some(url.to_string());
    Ok(artifact)
}

/// Recursively extract HTTP(S) image URLs from upstream image-generation JSON.
pub fn extract_image_urls(value: &serde_json::Value) -> Vec<String> {
    let mut urls = Vec::new();
    collect_urls(value, &mut urls);
    urls.sort();
    urls.dedup();
    urls
}

fn collect_urls(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) if looks_like_media_url(s) => out.push(s.clone()),
        serde_json::Value::Array(arr) => {
            for item in arr {
                collect_urls(item, out);
            }
        }
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                if (key.eq_ignore_ascii_case("url")
                    || key.eq_ignore_ascii_case("image_url")
                    || key.eq_ignore_ascii_case("image")
                    || key.ends_with("_url"))
                    && let Some(s) = val.as_str()
                    && looks_like_media_url(s)
                {
                    out.push(s.to_string());
                }
                collect_urls(val, out);
            }
        }
        _ => {}
    }
}

fn looks_like_media_url(s: &str) -> bool {
    let t = s.trim();
    (t.starts_with("http://") || t.starts_with("https://"))
        && !t.contains("example.com/placeholder")
}

/// Decode a `data:image/...;base64,...` URL and persist.
pub async fn persist_data_url(
    data_url: &str,
    provider: &str,
    model: &str,
) -> Result<MediaArtifact, ToolError> {
    let (mime, b64) = parse_data_url(data_url)?;
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| ToolError::ExecutionFailed(format!("decode data URL: {e}")))?;
    persist_bytes(&bytes, &mime, provider, model).await
}

fn parse_data_url(data_url: &str) -> Result<(String, &str), ToolError> {
    let trimmed = data_url.trim();
    let Some(rest) = trimmed.strip_prefix("data:") else {
        return Err(ToolError::InvalidParams("not a data URL".into()));
    };
    let (meta, data) = rest
        .split_once(',')
        .ok_or_else(|| ToolError::InvalidParams("malformed data URL".into()))?;
    let mime = meta
        .split(';')
        .next()
        .unwrap_or("application/octet-stream")
        .to_string();
    Ok((mime, data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_image_urls_finds_nested_urls() {
        let body = json!({
            "output": {
                "results": [{"url": "https://cdn.example/a.png"}]
            },
            "images": [{"url": "https://cdn.example/b.jpg"}]
        });
        let urls = extract_image_urls(&body);
        assert_eq!(urls.len(), 2);
        assert!(urls[0].contains("cdn.example"));
    }

    #[tokio::test]
    async fn persist_bytes_writes_file() {
        let artifact = persist_bytes(b"png-bytes", "image/png", "test", "m1")
            .await
            .expect("persist");
        assert!(artifact.local_path.exists());
        assert_eq!(artifact.mime, "image/png");
    }
}

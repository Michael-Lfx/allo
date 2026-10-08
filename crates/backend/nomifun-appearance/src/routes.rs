//! `/api/appearance/wallpapers` handlers.

use axum::Router;
use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, Json, Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Response;
use axum::routing::get;
use nomifun_api_types::ApiResponse;
use nomifun_auth::CurrentUser;
use nomifun_common::AppError;
use serde::Deserialize;

use crate::library::WallpaperBytes;
use crate::library::WallpaperMeta;
use crate::state::AppearanceRouterState;

pub fn appearance_routes(state: AppearanceRouterState) -> Router {
    Router::new()
        .route(
            "/api/appearance/wallpapers",
            get(list_wallpapers).post(create_wallpaper),
        )
        .route(
            "/api/appearance/wallpapers/{wallpaper_id}",
            axum::routing::get(get_wallpaper)
                .patch(update_wallpaper)
                .delete(delete_wallpaper),
        )
        .with_state(state)
}

/// AUTH-EXEMPT binary serve: native `<img>` / `<video>` cannot send the local-trust
/// header. Listing and mutation stay authenticated. Opaque UUIDv7 ids only.
pub fn appearance_public_routes(state: AppearanceRouterState) -> Router {
    Router::new()
        .route(
            "/api/appearance/wallpapers/{wallpaper_id}/display",
            get(get_display),
        )
        .route(
            "/api/appearance/wallpapers/{wallpaper_id}/thumb",
            get(get_thumb),
        )
        .route(
            "/api/appearance/wallpapers/{wallpaper_id}/original",
            get(get_original),
        )
        .with_state(state)
}

async fn list_wallpapers(
    State(state): State<AppearanceRouterState>,
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<Vec<WallpaperMeta>>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.list().await?)))
}

#[derive(Deserialize)]
struct CreateWallpaperRequest {
    source_path: String,
    #[serde(default)]
    name: String,
}

async fn create_wallpaper(
    State(state): State<AppearanceRouterState>,
    Extension(_user): Extension<CurrentUser>,
    body: Result<Json<CreateWallpaperRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<ApiResponse<WallpaperMeta>>), AppError> {
    let Json(req) = body.map_err(|error| AppError::BadRequest(error.to_string()))?;
    let meta = state.service.ingest(&req.source_path, &req.name).await?;
    Ok((StatusCode::CREATED, Json(ApiResponse::ok(meta))))
}

async fn get_wallpaper(
    State(state): State<AppearanceRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(wallpaper_id): Path<String>,
) -> Result<Json<ApiResponse<WallpaperMeta>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.get(&wallpaper_id).await?)))
}

#[derive(Deserialize)]
struct UpdateWallpaperRequest {
    name: Option<String>,
}

async fn update_wallpaper(
    State(state): State<AppearanceRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(wallpaper_id): Path<String>,
    body: Result<Json<UpdateWallpaperRequest>, JsonRejection>,
) -> Result<Json<ApiResponse<WallpaperMeta>>, AppError> {
    let Json(req) = body.map_err(|error| AppError::BadRequest(error.to_string()))?;
    let name = req
        .name
        .ok_or_else(|| AppError::BadRequest("name is required".into()))?;
    Ok(Json(ApiResponse::ok(
        state.service.rename(&wallpaper_id, &name).await?,
    )))
}

async fn delete_wallpaper(
    State(state): State<AppearanceRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(wallpaper_id): Path<String>,
) -> Result<StatusCode, AppError> {
    state.service.delete(&wallpaper_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_display(
    State(state): State<AppearanceRouterState>,
    Path(wallpaper_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    serve_bytes(state.service.read_display(&wallpaper_id).await?, headers)
}

async fn get_thumb(
    State(state): State<AppearanceRouterState>,
    Path(wallpaper_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    serve_bytes(state.service.read_thumb(&wallpaper_id).await?, headers)
}

async fn get_original(
    State(state): State<AppearanceRouterState>,
    Path(wallpaper_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    serve_bytes(state.service.read_original(&wallpaper_id).await?, headers)
}

fn serve_bytes(payload: WallpaperBytes, headers: HeaderMap) -> Result<Response, AppError> {
    let etag = format!("\"{}-{}\"", payload.mtime, payload.bytes.len());
    let if_none_match_hits = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').map(str::trim).any(|candidate| candidate == etag || candidate == "*"));
    if if_none_match_hits {
        return Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
            .header(header::ETAG, etag)
            .body(Body::empty())
            .map_err(|error| AppError::Internal(error.to_string()));
    }
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, payload.content_type)
        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
        .header(header::ETAG, etag)
        .body(Body::from(payload.bytes))
        .map_err(|error| AppError::Internal(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::AppearanceService;
    use axum::http::Request;
    use image::{DynamicImage, Rgb, RgbImage};
    use nomifun_common::WallpaperId;
    use std::io::Cursor;
    use tower::ServiceExt;

    fn stage_png() -> std::path::PathBuf {
        let img = RgbImage::from_pixel(24, 16, Rgb([12, 80, 180]));
        let mut buf = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        let dir = crate::media::upload_root();
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{}.png", WallpaperId::new()));
        std::fs::write(&path, buf).unwrap();
        path
    }

    #[tokio::test]
    async fn public_display_is_auth_exempt() {
        let tmp = tempfile::TempDir::new().unwrap();
        let service = AppearanceService::open(tmp.path()).unwrap();
        let source = stage_png();
        let meta = service
            .ingest(source.to_str().unwrap(), "sky")
            .await
            .unwrap();
        let app = appearance_public_routes(AppearanceRouterState::new(service));
        let missing = WallpaperId::new();
        let not_found = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/appearance/wallpapers/{missing}/display"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(not_found.status(), StatusCode::NOT_FOUND);

        let ok = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/appearance/wallpapers/{}/display",
                        meta.wallpaper_id
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ok.status(), StatusCode::OK);
        assert_eq!(
            ok.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/jpeg"
        );
    }
}

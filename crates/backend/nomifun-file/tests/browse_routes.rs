//! Router-level regression tests for the WebUI host file routes
//! (`GET /api/fs/browse`, `POST /api/fs/list`).
//!
//! The desktop shell uses the native OS dialog and never hits these routes, so
//! contract breaks here only surface in WebUI deployments (the picker's first
//! load shows "Unknown error"). These tests drive the real axum router with
//! the exact wire format the frontend sends, so an extractor rejection (e.g. a
//! camelCase/snake_case mismatch under `deny_unknown_fields`) fails here
//! instead of in production.

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use nomifun_api_types::WebSocketMessage;
use nomifun_file::{FileRouterState, FileService, FileWatchService, SnapshotService, file_routes};
use nomifun_realtime::UserEventSink;
use tower::ServiceExt;

/// A no-op broadcaster for testing (events are silently discarded).
struct NoopBroadcaster;

impl UserEventSink for NoopBroadcaster {
    fn send_to_user(&self, _user_id: &str, _event: WebSocketMessage<serde_json::Value>) {}
}

/// Build the real file router with its sandbox rooted at `root`.
fn make_router(root: &std::path::Path) -> axum::Router {
    let broadcaster = Arc::new(NoopBroadcaster);
    let roots = vec![root.to_path_buf()];
    file_routes(FileRouterState {
        file_service: Arc::new(FileService::new(broadcaster.clone(), roots.clone())),
        watch_service: Arc::new(FileWatchService::new(broadcaster).expect("watch service")),
        snapshot_service: Arc::new(SnapshotService::new()),
        allowed_roots: roots.clone(),
        browse_roots: roots,
    })
}

/// Percent-encode a filesystem path for use as a query-string value, exactly
/// as the frontend's `encodeURIComponent` does for `path`.
fn browse_uri(path: &std::path::Path, extra: &str) -> String {
    let encoded =
        serde_urlencoded::to_string([("path", path.to_str().expect("utf-8 temp path"))]).expect("encode path");
    format!("/api/fs/browse?{encoded}{extra}")
}

async fn get_json(router: axum::Router, uri: &str) -> (StatusCode, serde_json::Value, String) {
    let response = router
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let raw = String::from_utf8_lossy(&bytes).into_owned();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json, raw)
}

async fn post_json(
    router: axum::Router,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value, String) {
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let raw = String::from_utf8_lossy(&bytes).into_owned();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json, raw)
}

#[tokio::test]
async fn browse_accepts_the_webui_picker_wire_format() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("sub")).unwrap();
    std::fs::write(tmp.path().join("a.txt"), "x").unwrap();

    // Exactly what DirectorySelectionModal sends: camelCase `showFiles`.
    let uri = browse_uri(tmp.path(), "&showFiles=false");
    let (status, json, raw) = get_json(make_router(tmp.path()), &uri).await;

    assert_eq!(status, StatusCode::OK, "browse rejected the WebUI wire format: {raw}");
    assert_eq!(json["success"], true, "unexpected envelope: {raw}");
    let items = json["data"]["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1, "showFiles=false must list directories only: {raw}");
    assert_eq!(items[0]["name"], "sub");
    assert_eq!(items[0]["isDirectory"], true);
}

#[tokio::test]
async fn browse_show_files_true_includes_files() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("sub")).unwrap();
    std::fs::write(tmp.path().join("a.txt"), "x").unwrap();

    let uri = browse_uri(tmp.path(), "&showFiles=true");
    let (status, json, raw) = get_json(make_router(tmp.path()), &uri).await;

    assert_eq!(status, StatusCode::OK, "browse rejected showFiles=true: {raw}");
    let items = json["data"]["items"].as_array().expect("items array");
    assert_eq!(items.len(), 2, "showFiles=true must include files: {raw}");
}

#[tokio::test]
async fn browse_error_bodies_are_json_envelopes() {
    // The picker surfaces `errorData.error` from a JSON body; a sandbox
    // rejection must therefore arrive as the structured error envelope, not
    // plain text.
    let sandbox = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();

    let uri = browse_uri(outside.path(), "&showFiles=false");
    let (status, json, raw) = get_json(make_router(sandbox.path()), &uri).await;

    assert_eq!(status, StatusCode::FORBIDDEN, "expected sandbox rejection: {raw}");
    assert_eq!(json["success"], false, "error body must be the JSON envelope: {raw}");
    assert!(
        json["error"].as_str().is_some_and(|e| !e.is_empty()),
        "error body must carry a human-readable message: {raw}"
    );
}

#[tokio::test]
async fn list_accepts_the_workspace_the_frontend_registered() {
    // The Artifact panel lists the **selected conversation's workspace**, and
    // the owner registers that path through the picker — anywhere on disk, so
    // routinely outside this service's `allowed_roots` (temp / home / work
    // dir). Unlike `/api/fs/read`, a list request carries no separate
    // `workspace` field to authorize against: its `root` *is* the workspace,
    // so the route itself has to admit it the way the read / metadata / zip
    // routes admit theirs through `extra_root`. Without that the panel's only
    // data path is dead for every such workspace (HTTP 403
    // `PATH_OUTSIDE_SANDBOX`).
    //
    // The service-level sandbox is deliberately untouched — the non-scoped
    // `IFileService::list_workspace_files` still confines to `allowed_roots`
    // (`directory_browsing::list_workspace_files_rejects_outside_sandbox`),
    // which is what the Channel / Remote gateway caller relies on.
    let sandbox = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("artifact.txt"), "hi").unwrap();

    let (status, json, raw) = post_json(
        make_router(sandbox.path()),
        "/api/fs/list",
        serde_json::json!({ "root": workspace.path().to_str().unwrap() }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "list rejected the owner's workspace: {raw}");
    assert_eq!(json["success"], true, "unexpected envelope: {raw}");
    let names: Vec<&str> = json["data"]
        .as_array()
        .expect("list array")
        .iter()
        .filter_map(|entry| entry["name"].as_str())
        .collect();
    assert!(names.contains(&"artifact.txt"), "expected the workspace listing, got {names:?}");
}

#[tokio::test]
async fn list_still_rejects_a_root_that_does_not_exist() {
    // Widening the authority must not turn a bad `root` into a silent empty
    // listing: the panel renders "no files" from a 200, so a typo'd or deleted
    // workspace has to stay an error.
    let sandbox = tempfile::tempdir().unwrap();
    let missing = sandbox.path().join("not-a-directory");

    let (status, json, raw) = post_json(
        make_router(sandbox.path()),
        "/api/fs/list",
        serde_json::json!({ "root": missing.to_str().unwrap() }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "expected a resolution error: {raw}");
    assert_eq!(json["success"], false, "unexpected envelope: {raw}");
}

#[tokio::test]
async fn create_directory_adds_a_folder_visible_to_the_picker() {
    let tmp = tempfile::tempdir().unwrap();
    let parent_path = tmp.path().to_str().unwrap();
    let (status, json, raw) = post_json(
        make_router(tmp.path()),
        "/api/fs/directory",
        serde_json::json!({ "parentPath": parent_path, "name": "new project" }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "folder creation failed: {raw}");
    assert_eq!(json["success"], true, "unexpected envelope: {raw}");
    assert_eq!(json["data"]["name"], "new project");
    assert_eq!(json["data"]["isDirectory"], true);
    assert!(tmp.path().join("new project").is_dir());
}

#[tokio::test]
async fn create_directory_rejects_nested_names() {
    let tmp = tempfile::tempdir().unwrap();
    let (status, json, raw) = post_json(
        make_router(tmp.path()),
        "/api/fs/directory",
        serde_json::json!({
            "parentPath": tmp.path().to_str().unwrap(),
            "name": "../outside"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "unexpected response: {raw}");
    assert_eq!(json["success"], false);
}

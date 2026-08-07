use serde::{Deserialize, Serialize};

/// Response for `GET /api/system/info`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SystemInfoResponse {
    pub cache_dir: String,
    pub work_dir: String,
    pub log_dir: String,
    /// Opaque identity of the current on-disk dataset. Browser caches must
    /// include it in their namespace so a reset/restore cannot attach stale
    /// UI state to a different entity graph.
    pub storage_generation: String,
    /// Operating system: `darwin`, `win32`, or `linux`.
    pub platform: String,
    /// CPU architecture: `x64` or `arm64`.
    pub arch: String,
    /// Result of the most recent work-root relocation, when one has been
    /// attempted. The field is omitted on installations with no relocation
    /// history.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_dir_change: Option<WorkDirChangeStatus>,
    /// Capabilities supplied by the host composition. The renderer must use
    /// this contract instead of inferring the runtime from browser globals.
    #[serde(default)]
    pub runtime_capabilities: RuntimeCapabilities,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeKind {
    Desktop,
    Web,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeCapabilities {
    pub runtime: RuntimeKind,
    pub can_restart_application: bool,
    pub can_change_work_directory: bool,
}

impl RuntimeCapabilities {
    pub const fn desktop() -> Self {
        Self {
            runtime: RuntimeKind::Desktop,
            can_restart_application: true,
            can_change_work_directory: true,
        }
    }

    pub const fn web() -> Self {
        Self {
            runtime: RuntimeKind::Web,
            can_restart_application: false,
            can_change_work_directory: false,
        }
    }
}

impl Default for RuntimeCapabilities {
    fn default() -> Self {
        Self::web()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkDirChangeStatus {
    pub state: WorkDirChangeState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_work_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_work_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollback_copy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkDirChangeState {
    None,
    Completed,
    Failed,
}

/// Response for `POST /api/system/support-logs/pack`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SupportLogsPackResponse {
    pub zip_path: String,
    pub file_name: String,
    pub byte_size: i64,
    pub included_files: Vec<String>,
    pub truncated: bool,
}

/// Optional correlation context for `POST /api/system/support-logs/pack`.
///
/// A turn-specific support report may include the bounded raw SSE diagnostic
/// captured for that failed provider turn. An omitted id preserves the normal
/// application-log-only archive.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackSupportLogsRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
}

/// Request body for `POST /api/system/check-update`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct UpdateCheckRequest {
    #[serde(default)]
    pub include_prerelease: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
}

/// Request body for `POST /api/system/work-dir`: request a user-chosen
/// conversation workspace root for the next boot. The current v3 generation
/// and data-root side stores remain in place; only managed conversations are
/// relocated during the restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateWorkDirRequest {
    pub work_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UpdateWorkDirResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    pub restart_required: bool,
}

/// Management state for a durable work-root relocation. Unlike the compact
/// last-status field above, this state is backed by the pending plan and its
/// phase markers and can be acted on by the settings UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkDirRelocationOperationState {
    Planned,
    Copying,
    Verified,
    Published,
    SourcePreserved,
    Completed,
    Failed,
    Paused,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkDirRelocationErrorClass {
    Transient,
    Deterministic,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkDirRelocationOperation {
    pub operation_id: String,
    pub state: WorkDirRelocationOperationState,
    pub source_work_dir: String,
    pub target_work_dir: String,
    pub restart_required: bool,
    pub attempt_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error_class: Option<WorkDirRelocationErrorClass>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shortfall_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calculation_mode: Option<WorkDirRelocationCalculationMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkDirRelocationCalculationMode {
    SameVolumeRename,
    CrossVolumeCopy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkDirRelocationBackup {
    pub operation_id: String,
    pub generation: String,
    pub source_work_dir: String,
    pub target_work_dir: String,
    pub backup_path: String,
    pub byte_size: u64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkDirRelocationResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<WorkDirRelocationOperation>,
    pub backups: Vec<WorkDirRelocationBackup>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReplaceWorkDirRelocationRequest {
    pub work_dir: String,
}

/// Response for `POST /api/system/check-update`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub update_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<UpdateReleaseInfo>,
}

/// A single GitHub Release entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UpdateReleaseInfo {
    pub tag_name: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    pub html_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    pub prerelease: bool,
    pub draft: bool,
    pub assets: Vec<GitHubReleaseAsset>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_asset: Option<GitHubReleaseAsset>,
}

/// A downloadable asset attached to a GitHub Release.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GitHubReleaseAsset {
    pub name: String,
    pub url: String,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -- SystemInfoResponse --

    #[test]
    fn test_system_info_response_serialization() {
        let resp = SystemInfoResponse {
            cache_dir: "/home/user/.cache/nomifun".into(),
            work_dir: "/home/user/.local/share/nomifun".into(),
            log_dir: "/home/user/.local/state/nomifun/logs".into(),
            storage_generation: "01900000-0000-7000-8000-000000000000".into(),
            platform: "linux".into(),
            arch: "x64".into(),
            work_dir_change: None,
            runtime_capabilities: RuntimeCapabilities::web(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["cache_dir"], "/home/user/.cache/nomifun");
        assert_eq!(json["work_dir"], "/home/user/.local/share/nomifun");
        assert_eq!(json["log_dir"], "/home/user/.local/state/nomifun/logs");
        assert_eq!(
            json["storage_generation"],
            "01900000-0000-7000-8000-000000000000"
        );
        assert_eq!(json["platform"], "linux");
        assert_eq!(json["arch"], "x64");
        // Verify snake_case
        assert!(json.get("cacheDir").is_none());
    }

    #[test]
    fn test_system_info_response_roundtrip() {
        let original = SystemInfoResponse {
            cache_dir: "/tmp/cache".into(),
            work_dir: "/tmp/work".into(),
            log_dir: "/tmp/logs".into(),
            storage_generation: "01900000-0000-7000-8000-000000000001".into(),
            platform: "darwin".into(),
            arch: "arm64".into(),
            work_dir_change: None,
            runtime_capabilities: RuntimeCapabilities::desktop(),
        };
        let serialized = serde_json::to_string(&original).unwrap();
        let parsed: SystemInfoResponse = serde_json::from_str(&serialized).unwrap();
        assert_eq!(parsed, original);
    }

    // -- UpdateCheckRequest --

    #[test]
    fn test_update_check_request_default() {
        let raw = json!({});
        let req: UpdateCheckRequest = serde_json::from_value(raw).unwrap();
        assert!(!req.include_prerelease);
        assert!(req.repo.is_none());
    }

    #[test]
    fn test_update_check_request_with_options() {
        let raw = json!({
            "include_prerelease": true,
            "repo": "nomifun/nomifun-app"
        });
        let req: UpdateCheckRequest = serde_json::from_value(raw).unwrap();
        assert!(req.include_prerelease);
        assert_eq!(req.repo.as_deref(), Some("nomifun/nomifun-app"));
    }

    // -- UpdateWorkDirRequest --

    #[test]
    fn test_update_work_dir_request_deserializes_snake_case() {
        let raw = json!({ "work_dir": "/Users/me/Workspaces/nomi" });
        let req: UpdateWorkDirRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(req.work_dir, "/Users/me/Workspaces/nomi");
    }

    #[test]
    fn test_work_dir_relocation_management_serializes_snake_case_and_optional_fields() {
        let operation = WorkDirRelocationOperation {
            operation_id: "01900000-0000-7000-8000-000000000002".into(),
            state: WorkDirRelocationOperationState::Paused,
            source_work_dir: "C:/old".into(),
            target_work_dir: "D:/new".into(),
            restart_required: true,
            attempt_count: 3,
            last_attempt_at: Some(123),
            last_error_class: Some(WorkDirRelocationErrorClass::Transient),
            last_error_code: Some("INTERNAL_ERROR".into()),
            error: Some("target is locked".into()),
            required_bytes: Some(42),
            available_bytes: None,
            shortfall_bytes: None,
            calculation_mode: Some(WorkDirRelocationCalculationMode::CrossVolumeCopy),
        };
        let value = serde_json::to_value(&operation).unwrap();
        assert_eq!(value["operation_id"], operation.operation_id);
        assert_eq!(value["state"], "paused");
        assert_eq!(value["last_error_class"], "transient");
        assert_eq!(value["calculation_mode"], "cross_volume_copy");
        assert!(value.get("available_bytes").is_none());
    }

    // -- UpdateCheckResult --

    #[test]
    fn test_update_check_result_no_update() {
        let result = UpdateCheckResult {
            current_version: "1.5.0".into(),
            update_available: false,
            latest: None,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["current_version"], "1.5.0");
        assert_eq!(json["update_available"], false);
        assert!(json.get("latest").is_none());
    }

    #[test]
    fn test_update_check_result_with_update() {
        let result = UpdateCheckResult {
            current_version: "1.0.0".into(),
            update_available: true,
            latest: Some(UpdateReleaseInfo {
                tag_name: "v2.0.0".into(),
                version: "2.0.0".into(),
                name: Some("Version 2.0.0".into()),
                body: Some("Major release".into()),
                html_url: "https://github.com/nomifun/nomifun-app/releases/tag/v2.0.0".into(),
                published_at: Some("2026-04-01T00:00:00Z".into()),
                prerelease: false,
                draft: false,
                assets: vec![GitHubReleaseAsset {
                    name: "nomifun-2.0.0-darwin-arm64.dmg".into(),
                    url: "https://github.com/download/nomifun.dmg".into(),
                    size: 85_000_000,
                    content_type: Some("application/x-apple-diskimage".into()),
                }],
                recommended_asset: None,
            }),
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["update_available"], true);
        assert_eq!(json["latest"]["tag_name"], "v2.0.0");
        assert_eq!(json["latest"]["version"], "2.0.0");
        assert!(!json["latest"]["html_url"].as_str().unwrap().is_empty());
        assert_eq!(json["latest"]["assets"].as_array().unwrap().len(), 1);
        assert_eq!(json["latest"]["assets"][0]["name"], "nomifun-2.0.0-darwin-arm64.dmg");
        assert_eq!(json["latest"]["assets"][0]["size"], 85_000_000_u64);
    }

    // -- GitHubReleaseAsset --

    #[test]
    fn test_github_release_asset_serialization() {
        let asset = GitHubReleaseAsset {
            name: "app.zip".into(),
            url: "https://example.com/app.zip".into(),
            size: 1024,
            content_type: None,
        };
        let json = serde_json::to_value(&asset).unwrap();
        assert_eq!(json["name"], "app.zip");
        assert_eq!(json["url"], "https://example.com/app.zip");
        assert_eq!(json["size"], 1024);
        assert!(json.get("content_type").is_none());
    }

    #[test]
    fn test_github_release_asset_with_content_type() {
        let asset = GitHubReleaseAsset {
            name: "app.exe".into(),
            url: "https://example.com/app.exe".into(),
            size: 50_000_000,
            content_type: Some("application/octet-stream".into()),
        };
        let json = serde_json::to_value(&asset).unwrap();
        assert_eq!(json["content_type"], "application/octet-stream");
    }

    // -- UpdateReleaseInfo --

    #[test]
    fn test_update_release_info_minimal() {
        let info = UpdateReleaseInfo {
            tag_name: "v1.0.0".into(),
            version: "1.0.0".into(),
            name: None,
            body: None,
            html_url: "https://github.com/org/repo/releases/tag/v1.0.0".into(),
            published_at: None,
            prerelease: false,
            draft: false,
            assets: vec![],
            recommended_asset: None,
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["tag_name"], "v1.0.0");
        assert!(json.get("name").is_none());
        assert!(json.get("body").is_none());
        assert!(json.get("published_at").is_none());
        assert!(json.get("recommended_asset").is_none());
    }

    #[test]
    fn test_update_release_info_prerelease() {
        let info = UpdateReleaseInfo {
            tag_name: "v2.0.0-beta.1".into(),
            version: "2.0.0-beta.1".into(),
            name: Some("Beta 1".into()),
            body: None,
            html_url: "https://github.com/org/repo/releases/tag/v2.0.0-beta.1".into(),
            published_at: None,
            prerelease: true,
            draft: false,
            assets: vec![],
            recommended_asset: None,
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["prerelease"], true);
        assert_eq!(json["draft"], false);
    }
}

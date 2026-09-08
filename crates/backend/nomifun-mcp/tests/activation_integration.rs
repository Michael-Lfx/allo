//! Integration tests for `McpActivationService` with real SQLite.
//!
//! Covers the explicit "test and enable" activation contract:
//! success enables, failure / auth / config-drift stay disabled, and
//! test-by-id always persists its result to the authoritative row.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nomifun_api_types::{
    McpConnectionTestErrorCode, McpConnectionTestResult, McpServerId, McpToolResponse, McpTransport,
};
use nomifun_db::SqliteMcpServerRepository;
use nomifun_mcp::{McpActivationService, McpConfigService, McpConnectionTester, McpError, McpServerTransport};

async fn make_config_service() -> McpConfigService {
    let db = nomifun_db::init_database_memory().await.unwrap();
    let repo = Arc::new(SqliteMcpServerRepository::new(db.pool().clone()));
    McpConfigService::new(repo)
}

fn activation_service(config: McpConfigService, tester: Arc<dyn McpConnectionTester>) -> McpActivationService {
    McpActivationService::new(config, tester)
}

fn ok_result(tools: Vec<McpToolResponse>) -> McpConnectionTestResult {
    McpConnectionTestResult {
        success: true,
        tools: Some(tools),
        error: None,
        code: None,
        details: None,
        needs_auth: None,
        auth_method: None,
        www_authenticate: None,
    }
}

fn failed_result(error: &str, code: McpConnectionTestErrorCode) -> McpConnectionTestResult {
    McpConnectionTestResult {
        success: false,
        tools: None,
        error: Some(error.to_owned()),
        code: Some(code),
        details: None,
        needs_auth: None,
        auth_method: None,
        www_authenticate: None,
    }
}

fn needs_auth_result() -> McpConnectionTestResult {
    McpConnectionTestResult {
        success: false,
        tools: None,
        error: Some("authentication required".to_owned()),
        code: Some(McpConnectionTestErrorCode::HttpError),
        details: None,
        needs_auth: Some(true),
        auth_method: Some(nomifun_api_types::McpAuthMethod::Oauth),
        www_authenticate: None,
    }
}

fn static_tester(result: McpConnectionTestResult) -> Arc<dyn McpConnectionTester> {
    Arc::new(StaticTester {
        result: Mutex::new(result),
    })
}

struct StaticTester {
    result: Mutex<McpConnectionTestResult>,
}

#[async_trait::async_trait]
impl McpConnectionTester for StaticTester {
    async fn test_connection(&self, _name: &str, _transport: &McpServerTransport) -> McpConnectionTestResult {
        self.result.lock().unwrap().clone()
    }
}

/// Tester that mutates the persisted configuration while "testing", simulating
/// an edit racing the activation flow.
struct ConfigChangingTester {
    config: McpConfigService,
}

#[async_trait::async_trait]
impl McpConnectionTester for ConfigChangingTester {
    async fn test_connection(&self, _name: &str, transport: &McpServerTransport) -> McpConnectionTestResult {
        let servers = self.config.list_servers().await.unwrap();
        let server = servers.first().expect("server exists");
        let mut changed = transport.clone();
        if let McpServerTransport::Http { url, headers } = &mut changed {
            *url = format!("{url}-changed");
            headers.clear();
        }
        self.config
            .edit_server(
                &server.mcp_server_id,
                nomifun_api_types::UpdateMcpServerRequest {
                    name: None,
                    description: None,
                    transport: Some(nomifun_api_types::McpTransport::from(changed)),
                    original_json: None,
                    builtin: None,
                },
            )
            .await
            .unwrap();
        ok_result(vec![McpToolResponse {
            name: "stale-tool".into(),
            description: None,
            input_schema: None,
        }])
    }
}

async fn seed_http_server(config: &McpConfigService, name: &str) -> McpServerId {
    let created = config
        .add_server(nomifun_api_types::CreateMcpServerRequest {
            name: name.to_owned(),
            description: None,
            transport: McpTransport::Http {
                url: "https://example.com/mcp".into(),
                headers: HashMap::new(),
            },
            original_json: None,
            builtin: false,
        })
        .await
        .unwrap();
    created.mcp_server_id
}

#[tokio::test]
async fn successful_activation_persists_test_and_enables() {
    let config = make_config_service().await;
    let id = seed_http_server(&config, "activate-ok").await;
    let svc = activation_service(
        config.clone(),
        static_tester(ok_result(vec![McpToolResponse {
            name: "echo".into(),
            description: Some("Echoes".into()),
            input_schema: None,
        }])),
    );

    let response = svc.test_and_enable(&id).await.unwrap();

    assert!(response.enabled);
    assert!(response.enable_rejected_reason.is_none());
    assert!(response.test.success);
    assert_eq!(response.server.enabled, true);
    assert_eq!(response.server.last_test_status, nomifun_common::McpServerStatus::Connected);
    let tools = response.server.tools.expect("tools persisted");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "echo");

    // The row itself is enabled (visible to conversation selectors).
    let persisted = config.get_server(&id).await.unwrap();
    assert!(persisted.enabled);
}

#[tokio::test]
async fn failed_activation_stays_disabled_with_reason() {
    let config = make_config_service().await;
    let id = seed_http_server(&config, "activate-fail").await;
    let svc = activation_service(
        config.clone(),
        static_tester(failed_result(
            "command not found",
            McpConnectionTestErrorCode::CommandNotFound,
        )),
    );

    let response = svc.test_and_enable(&id).await.unwrap();

    assert!(!response.enabled);
    assert_eq!(
        response.enable_rejected_reason.as_deref(),
        Some("command not found")
    );
    assert!(!response.server.enabled);
    assert_eq!(response.server.last_test_status, nomifun_common::McpServerStatus::Error);

    let persisted = config.get_server(&id).await.unwrap();
    assert!(!persisted.enabled);
}

#[tokio::test]
async fn needs_auth_activation_stays_disabled() {
    let config = make_config_service().await;
    let id = seed_http_server(&config, "activate-auth").await;
    let svc = activation_service(config.clone(), static_tester(needs_auth_result()));

    let response = svc.test_and_enable(&id).await.unwrap();

    assert!(!response.enabled);
    assert!(response.needs_auth);
    let reason = response.enable_rejected_reason.expect("auth rejection reason");
    assert!(reason.to_lowercase().contains("authentication"));
    assert!(!response.server.enabled);
}

#[tokio::test]
async fn config_change_during_test_refuses_enable() {
    let config = make_config_service().await;
    let id = seed_http_server(&config, "activate-drift").await;
    let svc = activation_service(config.clone(), Arc::new(ConfigChangingTester { config: config.clone() }));

    let response = svc.test_and_enable(&id).await.unwrap();

    assert!(!response.enabled);
    assert!(response.config_changed);
    let reason = response.enable_rejected_reason.expect("drift rejection reason");
    assert!(reason.contains("changed"));
    assert!(!response.server.enabled);
    // The drift edit cleared the test status; a stale `connected` must not remain.
    let persisted = config.get_server(&id).await.unwrap();
    assert!(!persisted.enabled);
    assert_eq!(persisted.last_test_status, nomifun_common::McpServerStatus::Disconnected);
}

#[tokio::test]
async fn saved_configuration_changes_advance_the_activation_revision() {
    let config = make_config_service().await;
    let id = seed_http_server(&config, "revision-check").await;

    let initial_revision = config.config_revision(&id).await.unwrap();
    assert_eq!(initial_revision, 0);

    config
        .edit_server(
            &id,
            nomifun_api_types::UpdateMcpServerRequest {
                name: None,
                description: Some(Some("updated".into())),
                transport: None,
                original_json: None,
                builtin: None,
            },
        )
        .await
        .unwrap();

    assert!(config.config_revision(&id).await.unwrap() > initial_revision);
}

#[tokio::test]
async fn test_by_id_persists_failure_result() {
    let config = make_config_service().await;
    let id = seed_http_server(&config, "test-by-id").await;
    let svc = activation_service(
        config.clone(),
        static_tester(failed_result("timed out", McpConnectionTestErrorCode::Timeout)),
    );

    let response = svc.test_server_by_id(&id).await.unwrap();

    assert!(!response.test.success);
    assert!(!response.server.enabled);
    assert_eq!(response.server.last_test_status, nomifun_common::McpServerStatus::Error);
    assert!(response.server.tools.is_none());

    let persisted = config.get_server(&id).await.unwrap();
    assert_eq!(persisted.last_test_status, nomifun_common::McpServerStatus::Error);
}

#[tokio::test]
async fn test_by_id_success_persists_connected_and_tools_without_enabling() {
    let config = make_config_service().await;
    let id = seed_http_server(&config, "test-by-id-ok").await;
    let svc = activation_service(
        config.clone(),
        static_tester(ok_result(vec![McpToolResponse {
            name: "list_files".into(),
            description: None,
            input_schema: None,
        }])),
    );

    let response = svc.test_server_by_id(&id).await.unwrap();

    assert!(response.test.success);
    // Test-by-id alone must NOT enable — that is the explicit activation step.
    assert!(!response.server.enabled);
    assert_eq!(response.server.last_test_status, nomifun_common::McpServerStatus::Connected);
    let tools = response.server.tools.expect("tools persisted");
    assert_eq!(tools[0].name, "list_files");
}

#[tokio::test]
async fn activation_of_missing_server_is_not_found() {
    let config = make_config_service().await;
    let svc = activation_service(config, static_tester(ok_result(vec![])));
    let missing = McpServerId::parse("0190f5fe-7c00-7a00-8000-000000000999").unwrap();

    let error = svc.test_and_enable(&missing).await.unwrap_err();
    assert!(matches!(error, McpError::NotFound(_)));
}

//! Agent Store catalog provider seams for the App Server protocol.
//!
//! The App Server protocol layer stays decoupled from the system Skill / MCP
//! services: production adapters are implemented in the composition root
//! (`nomifun-app`), while tests use the fakes below. Providers return
//! `nomifun_common::AppError`, which the protocol layer maps onto its stable
//! wire codes (`not_found`, `connector_unavailable`, ...).

use async_trait::async_trait;

use nomifun_api_types::{
    AppServerConnectorDetail, AppServerConnectorProbeResult, AppServerConnectorStatusView,
    AppServerConnectorSummary, AppServerOAuthStartResult, AppServerOAuthStatusView,
    AppServerSkillDetail, AppServerSkillSummary,
};
use nomifun_common::AppError;

/// Read-side Skill catalog (`skill/list`, `skill/get`).
#[async_trait]
pub trait SkillCatalogProvider: Send + Sync {
    async fn list(&self) -> Result<Vec<AppServerSkillSummary>, AppError>;
    async fn get(&self, id: &str) -> Result<AppServerSkillDetail, AppError>;
}

/// Read-side Connector catalog + status/probe (`connector/list|get|status|test`).
#[async_trait]
pub trait ConnectorCatalogProvider: Send + Sync {
    async fn list(&self) -> Result<Vec<AppServerConnectorSummary>, AppError>;
    async fn get(&self, id: &str) -> Result<AppServerConnectorDetail, AppError>;
    async fn status(&self, id: &str) -> Result<AppServerConnectorStatusView, AppError>;
    async fn test(&self, id: &str) -> Result<AppServerConnectorProbeResult, AppError>;
}

/// Connector OAuth pass-through. Only states and public errors cross this
/// seam; tokens never do.
#[async_trait]
pub trait ConnectorAuthProvider: Send + Sync {
    async fn auth_status(&self, id: &str) -> Result<AppServerOAuthStatusView, AppError>;
    /// Kick off the browser flow on the trusted host and return immediately.
    async fn auth_start(&self, id: &str) -> Result<AppServerOAuthStartResult, AppError>;
    async fn logout(&self, id: &str) -> Result<(), AppError>;
}

// ---------------------------------------------------------------------------
// Test fakes
// ---------------------------------------------------------------------------

/// In-memory Skill catalog fake.
pub struct FakeSkillCatalog {
    pub skills: Vec<AppServerSkillSummary>,
}

#[async_trait]
impl SkillCatalogProvider for FakeSkillCatalog {
    async fn list(&self) -> Result<Vec<AppServerSkillSummary>, AppError> {
        Ok(self.skills.clone())
    }

    async fn get(&self, id: &str) -> Result<AppServerSkillDetail, AppError> {
        let summary = self
            .skills
            .iter()
            .find(|skill| skill.id == id || skill.name == id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("skill {id} not found")))?;
        Ok(AppServerSkillDetail {
            summary,
            mode: "store-agent".into(),
            invocation_policy: "model-auto".into(),
            instructions_summary: None,
        })
    }
}

/// In-memory Connector catalog fake. `auth_status` is derived from a mutable
/// set of authenticated connectors; `probe_fail` forces `test` to fail so the
/// `connected` merge rule can be verified.
pub struct FakeConnectorCatalog {
    pub connectors: Vec<AppServerConnectorSummary>,
    pub auth_required_ids: Vec<String>,
    pub probe_fail_ids: Vec<String>,
}

impl FakeConnectorCatalog {
    fn find(&self, id: &str) -> Result<AppServerConnectorSummary, AppError> {
        self.connectors
            .iter()
            .find(|connector| connector.id == id || connector.name == id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("connector {id} not found")))
    }
}

#[async_trait]
impl ConnectorCatalogProvider for FakeConnectorCatalog {
    async fn list(&self) -> Result<Vec<AppServerConnectorSummary>, AppError> {
        Ok(self.connectors.clone())
    }

    async fn get(&self, id: &str) -> Result<AppServerConnectorDetail, AppError> {
        let summary = self.find(id)?;
        Ok(AppServerConnectorDetail {
            summary,
            tool_filter: Some("connector__<name>__<tool>".into()),
            tools: vec![],
            auth_status: None,
            source: "system".into(),
            compatibility_status: nomifun_api_types::AppServerCompatibilityStatus::Compatible,
        })
    }

    async fn status(&self, id: &str) -> Result<AppServerConnectorStatusView, AppError> {
        let summary = self.find(id)?;
        let connector_id = summary.id.clone();
        let authenticated = !self.auth_required_ids.contains(&connector_id);
        let status = if !authenticated {
            nomifun_api_types::AppServerConnectorStatus::AuthorizationRequired
        } else if self.probe_fail_ids.contains(&connector_id) {
            nomifun_api_types::AppServerConnectorStatus::Error
        } else {
            nomifun_api_types::AppServerConnectorStatus::Connected
        };
        Ok(AppServerConnectorStatusView {
            connector_id,
            status,
            auth_status: Some(if authenticated {
                AppServerOAuthStatusView { state: "authenticated".into(), error: None }
            } else {
                AppServerOAuthStatusView { state: "not_authenticated".into(), error: None }
            }),
            last_error: if self.probe_fail_ids.contains(&summary.id) {
                Some("mock probe failed".into())
            } else {
                None
            },
        })
    }

    async fn test(&self, id: &str) -> Result<AppServerConnectorProbeResult, AppError> {
        let summary = self.find(id)?;
        let connector_id = summary.id.clone();
        let failed = self.probe_fail_ids.contains(&connector_id);
        Ok(AppServerConnectorProbeResult {
            connector_id,
            success: !failed,
            tools: None,
            error: if failed { Some("mock probe failed".into()) } else { None },
            code: if failed { Some("MCP_CONNECTION_FAILED".into()) } else { None },
        })
    }
}

/// In-memory OAuth fake: `start` flips the connector into the authenticated set.
pub struct FakeConnectorAuth {
    pub authenticated: std::sync::Mutex<Vec<String>>,
}

#[async_trait]
impl ConnectorAuthProvider for FakeConnectorAuth {
    async fn auth_status(&self, id: &str) -> Result<AppServerOAuthStatusView, AppError> {
        let authenticated = self.authenticated.lock().map_err(|_| {
            AppError::Internal("fake oauth state lock poisoned".into())
        })?.iter().any(|value| value == id);
        Ok(AppServerOAuthStatusView {
            state: if authenticated { "authenticated".into() } else { "not_authenticated".into() },
            error: None,
        })
    }

    async fn auth_start(&self, id: &str) -> Result<AppServerOAuthStartResult, AppError> {
        self.authenticated.lock().map_err(|_| {
            AppError::Internal("fake oauth state lock poisoned".into())
        })?.push(id.to_owned());
        Ok(AppServerOAuthStartResult { connector_id: id.to_owned(), state: "started".into(), error: None })
    }

    async fn logout(&self, id: &str) -> Result<(), AppError> {
        let mut authenticated = self.authenticated.lock().map_err(|_| {
            AppError::Internal("fake oauth state lock poisoned".into())
        })?;
        authenticated.retain(|value| value != id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_skill() -> AppServerSkillSummary {
        AppServerSkillSummary {
            id: "builtin:demo".into(),
            name: "demo".into(),
            description: Some("a demo skill".into()),
            version: "builtin".into(),
            source: "builtin".into(),
            compatibility_status: nomifun_api_types::AppServerCompatibilityStatus::Compatible,
            enabled: true,
            required_connectors: vec![],
        }
    }

    fn sample_connector() -> AppServerConnectorSummary {
        AppServerConnectorSummary {
            id: "0190f5fe-7c00-7a00-8000-000000000001".into(),
            name: "playwright".into(),
            description: None,
            kind: "stdio-mcp".into(),
            transport_summary: "npx @playwright/mcp".into(),
            auth_mode: "none".into(),
            enabled: true,
            status: nomifun_api_types::AppServerConnectorStatus::Connected,
        }
    }

    #[tokio::test]
    async fn skill_catalog_lists_and_gets_by_id_or_name() {
        let catalog = FakeSkillCatalog { skills: vec![sample_skill()] };
        let listed = catalog.list().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(catalog.get("demo").await.unwrap().summary.name, "demo");
        assert!(matches!(
            catalog.get("missing").await,
            Err(AppError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn connector_status_never_reports_connected_after_a_failed_probe() {
        let catalog = FakeConnectorCatalog {
            connectors: vec![sample_connector()],
            auth_required_ids: vec![],
            probe_fail_ids: vec!["0190f5fe-7c00-7a00-8000-000000000001".into()],
        };
        let status = catalog.status("playwright").await.unwrap();
        assert_eq!(status.status.as_str(), "error");
        assert_ne!(status.status.as_str(), "connected");
    }

    #[tokio::test]
    async fn connector_status_requires_authorization_when_auth_missing() {
        let catalog = FakeConnectorCatalog {
            connectors: vec![sample_connector()],
            auth_required_ids: vec!["0190f5fe-7c00-7a00-8000-000000000001".into()],
            probe_fail_ids: vec![],
        };
        let status = catalog.status("playwright").await.unwrap();
        assert_eq!(status.status.as_str(), "authorization_required");
    }

    #[tokio::test]
    async fn connector_auth_start_then_status_then_logout_roundtrip() {
        let auth = FakeConnectorAuth { authenticated: std::sync::Mutex::new(vec![]) };
        let started = auth.auth_start("playwright").await.unwrap();
        assert_eq!(started.state, "started");
        assert_eq!(auth.auth_status("playwright").await.unwrap().state, "authenticated");
        auth.logout("playwright").await.unwrap();
        assert_eq!(auth.auth_status("playwright").await.unwrap().state, "not_authenticated");
    }
}
//! The `credential` block: what a client needs to render and judge one
//! connector's credential form (`34` §6.1).
//!
//! Two halves, deliberately separate:
//!
//! - [`ConnectorDeclaration`] is the **imported declaration**, read from the
//!   snapshot's `credential` component (written by the importer, `34` §5.2);
//! - [`credential_block`] is the **pure projection** of that declaration plus the
//!   connector's current state into the wire shape.
//!
//! The projection takes the credential map and the plain values as arguments
//! rather than reading globals, so it is testable without a database, a config
//! file or a running host — which is what keeps its rules (which field counts as
//! missing, when the status is `error`) honest.

use std::collections::HashMap;
use std::sync::Arc;

use nomifun_api_types::{
    AppServerConnectorCredential, AppServerCredentialField, AppServerCredentialMode,
    AppServerCredentialStatus, AppServerLocalizedString,
};
use nomifun_common::secret_ref;
use nomifun_common::{AppError, McpServerStatus};
use nomifun_api_types::McpTransport;
use nomifun_db::IPluginSnapshotRepository;
use serde_json::Value;

/// One field of an imported declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredField {
    pub key: String,
    /// `secret` (the credential store) or `plain` (the connector's own values).
    pub kind: String,
    pub required: bool,
    pub label: AppServerLocalizedString,
    pub placeholder: AppServerLocalizedString,
    pub description: AppServerLocalizedString,
    pub doc_url: AppServerLocalizedString,
    pub doc_label: AppServerLocalizedString,
}

impl DeclaredField {
    pub fn is_secret(&self) -> bool {
        self.kind == "secret"
    }
}

/// What a connector's marketplace entry declared, as the host stored it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorDeclaration {
    /// The marketplace directory name — the link between a connector row and its
    /// declaration (`34` §5.2).
    pub connector_id: String,
    /// `none` | `oauth` | `token`, as stored by the importer from the market's
    /// `auth_mode` (`34` §6.1).
    pub auth_mode: String,
    pub title: Option<AppServerLocalizedString>,
    pub description: Option<AppServerLocalizedString>,
    pub fields: Vec<DeclaredField>,
}

fn localized(value: Option<&Value>, fallback: &str) -> AppServerLocalizedString {
    let read = |key: &str| value.and_then(|v| v.get(key)).and_then(Value::as_str);
    AppServerLocalizedString {
        zh: read("zh").unwrap_or(fallback).to_owned(),
        en: read("en").unwrap_or(fallback).to_owned(),
    }
}

fn optional_localized(value: Option<&Value>) -> Option<AppServerLocalizedString> {
    let pair = localized(value, "");
    (!pair.zh.is_empty() || !pair.en.is_empty()).then_some(pair)
}

/// Parse a `credential` component payload into a declaration.
///
/// Returns `None` when the payload is not one — an older snapshot, or a different
/// producer — rather than inventing an empty form.
pub fn declaration_from_payload(payload: &str) -> Option<ConnectorDeclaration> {
    let value: Value = serde_json::from_str(payload).ok()?;
    let connector_id = value.get("connector_id")?.as_str()?.to_owned();
    let fields: Vec<DeclaredField> = value
        .get("fields")?
        .as_array()?
        .iter()
        .filter_map(|field| {
            let key = field.get("key")?.as_str()?.to_owned();
            Some(DeclaredField {
                key: key.clone(),
                kind: field
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("secret")
                    .to_owned(),
                required: field
                    .get("required")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                label: localized(field.get("label"), &key),
                placeholder: localized(field.get("placeholder"), ""),
                description: localized(field.get("description"), ""),
                doc_url: localized(value.get("doc_url"), ""),
                doc_label: localized(value.get("doc_label"), ""),
            })
        })
        .collect();
    if fields.is_empty() {
        return None;
    }
    Some(ConnectorDeclaration {
        connector_id,
        auth_mode: value
            .get("auth_mode")
            .and_then(Value::as_str)
            .unwrap_or("none")
            .to_owned(),
        title: optional_localized(value.get("title")),
        description: optional_localized(value.get("description")),
        fields,
    })
}

/// The `credential.mode` for a connector.
///
/// A declaration decides it. A connector with **no** declaration keeps the
/// transport-derived answer it has always had: a hand-registered remote server
/// still shows the OAuth entry point, while the 61 `token` connectors and the 204
/// with an empty `auth_mode` no longer do (`34` §6.1).
pub fn credential_mode(
    declaration: Option<&ConnectorDeclaration>,
    transport: &McpTransport,
) -> AppServerCredentialMode {
    match declaration {
        Some(declaration) => match declaration.auth_mode.as_str() {
            "token" => AppServerCredentialMode::Token,
            "oauth" => AppServerCredentialMode::Oauth,
            _ => AppServerCredentialMode::None,
        },
        None => match transport {
            McpTransport::Stdio { .. } => AppServerCredentialMode::None,
            McpTransport::Sse { .. } | McpTransport::Http { .. } => AppServerCredentialMode::Oauth,
        },
    }
}

/// Project a declaration plus the connector's state into the wire block (`34` §6.1).
///
/// `credentials` is the whole installed map — the function applies the
/// per-principal rule itself via [`secret_ref::lookup_for_with`], so "missing"
/// means missing **for this caller**. `values` is the plain layer in effect.
pub fn credential_block(
    connector_id: &str,
    declaration: Option<&ConnectorDeclaration>,
    mode: AppServerCredentialMode,
    transport: &McpTransport,
    values: &HashMap<String, String>,
    credentials: &HashMap<String, String>,
    operator: Option<&str>,
    principal: Option<&str>,
    last_test_status: McpServerStatus,
) -> AppServerConnectorCredential {
    let plain_values = |key: &str| values.get(key).cloned();
    let secret_values = |key: &str| {
        secret_ref::lookup_for_with(principal, key, credentials, operator)
    };

    let mut missing: Vec<String> = Vec::new();
    let mut fields: Vec<AppServerCredentialField> = Vec::new();
    for field in declaration.map(|d| d.fields.as_slice()).unwrap_or_default() {
        // A field counts as satisfied only for the caller being described: a
        // secret another principal holds is not this caller's.
        let resolved = if field.is_secret() {
            secret_values(&field.key)
        } else {
            plain_values(&field.key)
        };
        if field.required && resolved.is_none() {
            missing.push(field.key.clone());
        }
        fields.push(AppServerCredentialField {
            key: field.key.clone(),
            kind: field.kind.clone(),
            required: field.required,
            label: field.label.clone(),
            placeholder: field.placeholder.clone(),
            description: field.description.clone(),
            // Never a secret's value: only a plain field's own setting.
            value: (!field.is_secret()).then(|| resolved).flatten(),
            doc_url: field.doc_url.clone(),
            doc_label: field.doc_label.clone(),
        });
    }
    missing.sort();

    let status = match mode {
        AppServerCredentialMode::None => AppServerCredentialStatus::NotRequired,
        AppServerCredentialMode::Oauth => {
            // The OAuth state is projected separately (`auth_status`); from this
            // block's point of view the question is only whether the caller has
            // something usable, which the probe reports.
            match last_test_status {
                McpServerStatus::Connected => AppServerCredentialStatus::Configured,
                McpServerStatus::Error => AppServerCredentialStatus::Error,
                _ => AppServerCredentialStatus::RequiresInput,
            }
        }
        AppServerCredentialMode::Token => {
            if !missing.is_empty() {
                AppServerCredentialStatus::RequiresInput
            } else if last_test_status == McpServerStatus::Error {
                // Configured, and the server rejected it. The probe is the only
                // producer of this state (`34` §6.1).
                AppServerCredentialStatus::Error
            } else {
                AppServerCredentialStatus::Configured
            }
        }
    };

    let _ = transport;
    AppServerConnectorCredential {
        connector_id: connector_id.to_owned(),
        mode,
        status,
        missing,
        fields,
        title: declaration.and_then(|d| d.title.clone()),
        description: declaration.and_then(|d| d.description.clone()),
    }
}

/// Reads the credential declaration a connector was imported with (`34` §5.2).
///
/// The link is the snapshot, not a new table: `find_snapshot_by_mcp_server_id`
/// already answers "which import registered this `mcp_servers` row", and that
/// snapshot holds both the `connector` component (which carries the marketplace
/// directory name) and the `credential` component keyed by the same name.
#[derive(Clone)]
pub struct AppServerConnectorCredentials {
    snapshots: Arc<dyn IPluginSnapshotRepository>,
    /// Present when the host can also *write*: the MCP config service (plain
    /// values) and the `config.toml` path (credentials). Absent in read-only
    /// wirings, where only the declaration reader is needed.
    writer: Option<ConnectorCredentialWriter>,
}

#[derive(Clone)]
struct ConnectorCredentialWriter {
    config: nomifun_mcp::McpConfigService,
    config_path: std::path::PathBuf,
}

impl AppServerConnectorCredentials {
    pub fn new(snapshots: Arc<dyn IPluginSnapshotRepository>) -> Self {
        Self { snapshots, writer: None }
    }

    /// Make the credential **write** face available (`connector/credential/set|clear`).
    pub fn with_writer(
        mut self,
        config: nomifun_mcp::McpConfigService,
        config_path: std::path::PathBuf,
    ) -> Self {
        self.writer = Some(ConnectorCredentialWriter { config, config_path });
        self
    }

    /// The declaration for one registered connector, or `None` when it was not
    /// imported from a marketplace (nothing declared anything).
    pub async fn declaration_for(&self, mcp_server_id: &str) -> Option<ConnectorDeclaration> {
        let snapshot = self
            .snapshots
            .find_snapshot_by_mcp_server_id(mcp_server_id)
            .await
            .ok()??;
        let components = self.snapshots.get_components(&snapshot.snapshot_id).await.ok()?;

        // Which marketplace directory registered this row?
        let connector_id = components
            .iter()
            .filter(|component| component.kind == "connector")
            .find_map(|component| {
                let runtime: Value =
                    serde_json::from_str(component.runtime_ref.as_deref()?).ok()?;
                if runtime.get("mcp_server_id").and_then(Value::as_str) != Some(mcp_server_id) {
                    return None;
                }
                let payload: Value = serde_json::from_str(&component.payload_json).ok()?;
                payload
                    .get("connector_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })?;

        components
            .iter()
            .filter(|component| component.kind == "credential")
            .filter_map(|component| declaration_from_payload(&component.payload_json))
            .find(|declaration| declaration.connector_id == connector_id)
    }

    /// The `credential` block for one connector, as `principal` sees it.
    pub async fn describe(
        &self,
        mcp_server_id: &str,
        transport: &McpTransport,
        last_test_status: McpServerStatus,
        principal: Option<&str>,
    ) -> Option<AppServerConnectorCredential> {
        let declaration = self.declaration_for(mcp_server_id).await;
        let mode = credential_mode(declaration.as_ref(), transport);
        if declaration.is_none() && mode == AppServerCredentialMode::None {
            return None;
        }
        let values = transport_values(transport);
        let installed = secret_ref::credentials();
        let operator = secret_ref::operator_principal();
        Some(credential_block(
            mcp_server_id,
            declaration.as_ref(),
            mode,
            transport,
            &values,
            &installed,
            operator.as_deref(),
            principal,
            last_test_status,
        ))
    }

    /// Store what the user typed (`34` §6.1).
    ///
    /// Secrets go to the credential store under **this caller's** namespace;
    /// plain settings go into the connector's own transport values. Either way the
    /// frozen `credential` block comes back, so the client never has to guess the
    /// new state.
    pub async fn set(
        &self,
        mcp_server_id: &str,
        values: HashMap<String, String>,
        principal: Option<&str>,
    ) -> Result<AppServerConnectorCredential, AppError> {
        let writer = self.writer.as_ref().ok_or_else(|| {
            AppError::BadRequest("this host cannot store connector credentials".into())
        })?;
        let server = writer
            .config
            .get_server(&parse_server_id(mcp_server_id)?)
            .await
            .map_err(AppError::from)?;
        let declaration = self.declaration_for(mcp_server_id).await.ok_or_else(|| {
            AppError::BadRequest(format!(
                "connector {mcp_server_id} declares no credential form to fill"
            ))
        })?;
        let (secrets, plains) = route_values(&declaration, &values).map_err(AppError::BadRequest)?;

        if !secrets.is_empty() {
            write_secrets(writer, principal, &secrets).await?;
        }
        if !plains.is_empty() {
            write_plain_values(writer, &server, &plains).await?;
        }
        self.reloaded_describe(mcp_server_id, principal).await
    }

    /// Forget what this caller stored (`34` §6.1). Idempotent.
    pub async fn clear(
        &self,
        mcp_server_id: &str,
        keys: Option<Vec<String>>,
        principal: Option<&str>,
    ) -> Result<AppServerConnectorCredential, AppError> {
        let writer = self.writer.as_ref().ok_or_else(|| {
            AppError::BadRequest("this host cannot store connector credentials".into())
        })?;
        let declaration = self.declaration_for(mcp_server_id).await;
        let all_keys: Vec<String> = match (&declaration, &keys) {
            (Some(declaration), None) => declaration
                .fields
                .iter()
                .filter(|field| field.is_secret())
                .map(|field| field.key.clone())
                .collect(),
            (_, Some(keys)) => keys.clone(),
            (None, None) => Vec::new(),
        };
        let secret_keys: Vec<String> = all_keys
            .into_iter()
            .filter(|key| {
                declaration
                    .as_ref()
                    .map(|d| {
                        d.fields
                            .iter()
                            .any(|field| &field.key == key && field.is_secret())
                    })
                    .unwrap_or(true)
            })
            .collect();
        if !secret_keys.is_empty() {
            forget_secrets(writer, principal, &secret_keys).await?;
        }
        self.reloaded_describe(mcp_server_id, principal).await
    }

    async fn reloaded_describe(
        &self,
        mcp_server_id: &str,
        principal: Option<&str>,
    ) -> Result<AppServerConnectorCredential, AppError> {
        let writer = self.writer.as_ref().expect("checked by the caller");
        let server = writer
            .config
            .get_server(&parse_server_id(mcp_server_id)?)
            .await
            .map_err(AppError::from)?;
        self.describe(
            mcp_server_id,
            &server.transport,
            server.last_test_status,
            principal,
        )
        .await
        .ok_or_else(|| AppError::BadRequest("connector declares no credential form".into()))
    }
}

#[async_trait::async_trait]
impl nomifun_app_server::ConnectorCredentialProvider for AppServerConnectorCredentials {
    async fn get(
        &self,
        connector_id: &str,
        principal: Option<&str>,
    ) -> Result<AppServerConnectorCredential, AppError> {
        let writer = self.writer.as_ref().ok_or_else(|| {
            AppError::BadRequest("this host cannot read connector credentials".into())
        })?;
        let server = writer
            .config
            .get_server(&parse_server_id(connector_id)?)
            .await
            .map_err(AppError::from)?;
        self.describe(
            connector_id,
            &server.transport,
            server.last_test_status,
            principal,
        )
        .await
        .ok_or_else(|| AppError::BadRequest("connector declares no credential form".into()))
    }

    async fn set(
        &self,
        connector_id: &str,
        values: HashMap<String, String>,
        principal: Option<&str>,
    ) -> Result<AppServerConnectorCredential, AppError> {
        AppServerConnectorCredentials::set(self, connector_id, values, principal).await
    }

    async fn clear(
        &self,
        connector_id: &str,
        keys: Option<Vec<String>>,
        principal: Option<&str>,
    ) -> Result<AppServerConnectorCredential, AppError> {
        AppServerConnectorCredentials::clear(self, connector_id, keys, principal).await
    }
}

fn parse_server_id(id: &str) -> Result<nomifun_api_types::McpServerId, AppError> {    nomifun_api_types::McpServerId::parse(id)
        .map_err(|error| AppError::BadRequest(format!("connector {id} is not a valid id: {error}")))
}

/// A remote transport's plain values (a stdio server has none, `34` §5.3).
pub fn transport_values(transport: &McpTransport) -> HashMap<String, String> {
    match transport {
        McpTransport::Http { values, .. } | McpTransport::Sse { values, .. } => values.clone(),
        McpTransport::Stdio { .. } => HashMap::new(),
    }
}

/// Split a `credential/set` body by where each field belongs (`34` §5.3).
///
/// A key the declaration does not name is **refused**: the write face must not
/// grow beyond the form it published, or a client could seed arbitrary keys into
/// the host's credential file.
pub fn route_values(
    declaration: &ConnectorDeclaration,
    values: &HashMap<String, String>,
) -> Result<(Vec<(String, String)>, Vec<(String, String)>), String> {
    let mut secrets = Vec::new();
    let mut plains = Vec::new();
    for (key, value) in values {
        let Some(field) = declaration.fields.iter().find(|field| &field.key == key) else {
            return Err(format!("{key} is not a field of this connector's credential form"));
        };
        if field.is_secret() {
            secrets.push((key.clone(), value.clone()));
        } else {
            plains.push((key.clone(), value.clone()));
        }
    }
    Ok((secrets, plains))
}

/// Write this principal's secrets into the host config, then refresh the process
/// map so the next probe uses them without a restart (`34` §5.3).
async fn write_secrets(
    writer: &ConnectorCredentialWriter,
    principal: Option<&str>,
    secrets: &[(String, String)],
) -> Result<(), AppError> {
    let path = writer.config_path.clone();
    let scoped: Vec<(String, String)> = secrets
        .iter()
        .map(|(key, value)| (secret_ref::credential_key_for(principal, key), value.clone()))
        .collect();
    tokio::task::spawn_blocking(move || {
        let source = std::fs::read_to_string(&path).unwrap_or_default();
        let mut document = source;
        for (key, value) in &scoped {
            document = nomifun_app_server::agent_store::AgentStoreConfig::with_credential(
                &document, key, value,
            )
            .map_err(|error| AppError::BadRequest(error))?;
        }
        write_private(&path, &document)
    })
    .await
    .map_err(|error| AppError::Internal(format!("credential write task failed: {error}")))??;
    reload_credentials(&writer.config_path);
    Ok(())
}

/// Remove this principal's secrets. Removing a key that is not there is fine.
async fn forget_secrets(
    writer: &ConnectorCredentialWriter,
    principal: Option<&str>,
    keys: &[String],
) -> Result<(), AppError> {
    let path = writer.config_path.clone();
    let scoped: Vec<String> = keys
        .iter()
        .map(|key| secret_ref::credential_key_for(principal, key))
        .collect();
    tokio::task::spawn_blocking(move || {
        let source = std::fs::read_to_string(&path).unwrap_or_default();
        let mut document = source;
        for key in &scoped {
            document = nomifun_app_server::agent_store::AgentStoreConfig::without_credential(
                &document, key,
            )
            .map_err(|error| AppError::BadRequest(error))?;
        }
        write_private(&path, &document)
    })
    .await
    .map_err(|error| AppError::Internal(format!("credential write task failed: {error}")))??;
    reload_credentials(&writer.config_path);
    Ok(())
}

/// A connector's plain settings live in its own transport row, so the runtime
/// resolves them from where it already reads the URL and headers (`34` §5.3).
async fn write_plain_values(
    writer: &ConnectorCredentialWriter,
    server: &nomifun_api_types::McpServerResponse,
    plains: &[(String, String)],
) -> Result<(), AppError> {
    let mut transport = server.transport.clone();
    match &mut transport {
        McpTransport::Http { values, .. } | McpTransport::Sse { values, .. } => {
            for (key, value) in plains {
                values.insert(key.clone(), value.clone());
            }
        }
        McpTransport::Stdio { .. } => {
            return Err(AppError::BadRequest(
                "this connector has no plain settings to store".into(),
            ));
        }
    }
    writer
        .config
        .edit_server(
            &server.mcp_server_id,
            nomifun_api_types::UpdateMcpServerRequest {
                name: None,
                description: None,
                transport: Some(transport),
                original_json: None,
                builtin: None,
            },
        )
        .await
        .map_err(AppError::from)?;
    Ok(())
}

/// Replace the config file atomically, owner-only.
///
/// The write goes to a sibling temp file first: a crash mid-write must not leave
/// the operator's hand-edited config truncated.
fn write_private(path: &std::path::Path, document: &str) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| AppError::Internal(format!("create config dir: {error}")))?;
    }
    let temp = path.with_extension("toml.tmp");
    std::fs::write(&temp, document)
        .map_err(|error| AppError::Internal(format!("write config: {error}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&temp, path)
        .map_err(|error| AppError::Internal(format!("replace config: {error}")))?;
    Ok(())
}

/// Reinstall the process-wide credential map from the file just written.
///
/// Without this the write would only take effect after a restart, and the next
/// probe would answer from the stale map (`34` §5.3).
fn reload_credentials(path: &std::path::Path) {
    if let Some(config) = nomifun_app_server::agent_store::AgentStoreConfig::load_ok(path) {
        secret_ref::set_credentials(config.credentials);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomifun_common::secret_ref::scoped_key;

    fn declaration_payload() -> String {
        serde_json::json!({
            "connector_id": "tdengine",
            "auth_mode": "token",
            "title": { "zh": "TDengine 配置", "en": "TDengine configuration" },
            "fields": [
                { "key": "TDENGINE_API_KEY", "kind": "secret", "required": true,
                  "label": { "zh": "密钥", "en": "Key" } },
                { "key": "TDENGINE_API_HOST", "kind": "plain", "required": false,
                  "label": { "zh": "主机", "en": "Host" } },
            ],
        })
        .to_string()
    }

    fn http_transport() -> McpTransport {
        McpTransport::Http {
            url: "https://${TDENGINE_API_HOST}/mcp".to_owned(),
            headers: HashMap::new(),
            values: HashMap::new(),
        }
    }

    #[test]
    fn a_declaration_round_trips_from_its_stored_payload() {
        let declaration = declaration_from_payload(&declaration_payload()).expect("declaration");
        assert_eq!(declaration.connector_id, "tdengine");
        assert_eq!(declaration.auth_mode, "token");
        assert_eq!(declaration.fields.len(), 2);
        assert!(declaration.fields[0].is_secret());
        assert!(!declaration.fields[1].is_secret());
        assert_eq!(
            declaration.title.as_ref().map(|t| t.en.as_str()),
            Some("TDengine configuration")
        );
        // A payload that is not a declaration stays `None` rather than becoming an
        // empty form the UI would render as "nothing to fill".
        assert!(declaration_from_payload("{}").is_none());
        assert!(declaration_from_payload(r#"{"connector_id":"x","fields":[]}"#).is_none());
        assert!(declaration_from_payload("not json").is_none());
    }

    #[test]
    fn the_mode_comes_from_the_declaration_and_the_transport_is_only_a_fallback() {
        let declaration = declaration_from_payload(&declaration_payload()).unwrap();
        assert_eq!(
            credential_mode(Some(&declaration), &http_transport()),
            AppServerCredentialMode::Token
        );

        let mut oauth = declaration.clone();
        oauth.auth_mode = "oauth".to_owned();
        assert_eq!(
            credential_mode(Some(&oauth), &http_transport()),
            AppServerCredentialMode::Oauth
        );

        // The market's empty / `server-side` / `mcp` / `oneid-token` all mean
        // "nothing the client can fill" — this is the 204-connector behaviour
        // change (`34` §6.1).
        for mode in ["", "none", "server-side", "mcp", "oneid-token"] {
            let mut declared = declaration.clone();
            declared.auth_mode = mode.to_owned();
            assert_eq!(
                credential_mode(Some(&declared), &http_transport()),
                AppServerCredentialMode::None,
                "{mode} must not offer an auth entry point"
            );
        }

        // No declaration: keep what a hand-registered server has always shown.
        assert_eq!(
            credential_mode(None, &http_transport()),
            AppServerCredentialMode::Oauth
        );
        assert_eq!(
            credential_mode(
                None,
                &McpTransport::Stdio {
                    command: "npx".into(),
                    args: vec![],
                    env: HashMap::new(),
                }
            ),
            AppServerCredentialMode::None
        );
    }

    #[test]
    fn missing_required_fields_are_reported_by_key_name_for_this_caller_only() {
        let declaration = declaration_from_payload(&declaration_payload()).unwrap();
        let credentials = HashMap::from([(scoped_key("alice", "TDENGINE_API_KEY"), "a".to_owned())]);

        // Alice has the secret but not the (optional) plain host: nothing is
        // missing, because only `required` fields count.
        let block = credential_block(
            "conn-1",
            Some(&declaration),
            AppServerCredentialMode::Token,
            &http_transport(),
            &HashMap::new(),
            &credentials,
            Some("alice"),
            Some("alice"),
            McpServerStatus::Disconnected,
        );
        assert!(block.missing.is_empty());
        assert_eq!(block.status, AppServerCredentialStatus::Configured);

        // Bob has nothing of his own, and is not the operator: the same connector
        // is unconfigured *for him*.
        let block = credential_block(
            "conn-1",
            Some(&declaration),
            AppServerCredentialMode::Token,
            &http_transport(),
            &HashMap::new(),
            &credentials,
            Some("alice"),
            Some("bob"),
            McpServerStatus::Disconnected,
        );
        assert_eq!(block.missing, vec!["TDENGINE_API_KEY".to_owned()]);
        assert_eq!(block.status, AppServerCredentialStatus::RequiresInput);
        // …and the block still describes the form, so the UI can render it.
        assert_eq!(block.fields.len(), 2);
        assert_eq!(block.fields[0].key, "TDENGINE_API_KEY");
    }

    #[test]
    fn a_plain_field_carries_its_value_and_a_secret_field_never_does() {
        let declaration = declaration_from_payload(&declaration_payload()).unwrap();
        let credentials = HashMap::from([(scoped_key("alice", "TDENGINE_API_KEY"), "s3cr3t".to_owned())]);
        let values = HashMap::from([("TDENGINE_API_HOST".to_owned(), "localhost".to_owned())]);

        let block = credential_block(
            "conn-1",
            Some(&declaration),
            AppServerCredentialMode::Token,
            &http_transport(),
            &values,
            &credentials,
            Some("alice"),
            Some("alice"),
            McpServerStatus::Disconnected,
        );
        let secret = &block.fields[0];
        let plain = &block.fields[1];
        assert_eq!(secret.value, None, "a secret's value must never cross the wire");
        assert_eq!(plain.value.as_deref(), Some("localhost"));
        // The serialized form is the real guarantee: no secret anywhere in it.
        let wire = serde_json::to_string(&block).unwrap();
        assert!(!wire.contains("s3cr3t"), "{wire}");
        assert!(wire.contains("localhost"), "{wire}");
    }

    #[test]
    fn a_configured_connector_the_server_rejected_reads_as_error() {
        let declaration = declaration_from_payload(&declaration_payload()).unwrap();
        let credentials = HashMap::from([(scoped_key("alice", "TDENGINE_API_KEY"), "a".to_owned())]);

        // Configured + the last probe failed => the only producer of `error`
        // (`34` §6.1).
        let block = credential_block(
            "conn-1",
            Some(&declaration),
            AppServerCredentialMode::Token,
            &http_transport(),
            &HashMap::new(),
            &credentials,
            Some("alice"),
            Some("alice"),
            McpServerStatus::Error,
        );
        assert_eq!(block.status, AppServerCredentialStatus::Error);

        // Missing beats error: asking the user to fill something in is the more
        // useful statement, and the probe's failure may be downstream of it.
        let block = credential_block(
            "conn-1",
            Some(&declaration),
            AppServerCredentialMode::Token,
            &http_transport(),
            &HashMap::new(),
            &HashMap::new(),
            None,
            Some("alice"),
            McpServerStatus::Error,
        );
        assert_eq!(block.status, AppServerCredentialStatus::RequiresInput);
    }

    #[test]
    fn a_set_body_is_routed_by_field_kind_and_unknown_keys_are_refused() {
        let declaration = declaration_from_payload(&declaration_payload()).unwrap();
        let values = HashMap::from([
            ("TDENGINE_API_KEY".to_owned(), "s3cr3t".to_owned()),
            ("TDENGINE_API_HOST".to_owned(), "db.internal".to_owned()),
        ]);
        let (secrets, plains) = route_values(&declaration, &values).expect("both keys are declared");
        assert_eq!(secrets, vec![("TDENGINE_API_KEY".to_owned(), "s3cr3t".to_owned())]);
        assert_eq!(plains, vec![("TDENGINE_API_HOST".to_owned(), "db.internal".to_owned())]);

        // The write face must not grow beyond the form it published: an
        // undeclared key would otherwise seed arbitrary entries into the host's
        // credential file.
        let error = route_values(
            &declaration,
            &HashMap::from([("ANYTHING".to_owned(), "x".to_owned())]),
        )
        .expect_err("undeclared key");
        assert!(error.contains("ANYTHING"), "{error}");
    }

    #[test]
    fn an_atomic_write_replaces_the_file_and_leaves_no_temp_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[memory]\ndistill_enabled = false\n").unwrap();

        let document = nomifun_app_server::agent_store::AgentStoreConfig::with_credential(
            &std::fs::read_to_string(&path).unwrap(),
            "alice:TOKEN",
            "v",
        )
        .unwrap();
        write_private(&path, &document).expect("write");

        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("distill_enabled = false"), "{written}");
        let parsed = nomifun_app_server::agent_store::AgentStoreConfig::from_source(&written)
            .expect("the written file parses");
        assert_eq!(
            parsed.credentials.get("alice:TOKEN").map(String::as_str),
            Some("v")
        );
        assert!(
            !path.with_extension("toml.tmp").exists(),
            "the temp file must be renamed away, not left next to the config"
        );
    }

    #[test]
    fn a_connector_with_nothing_to_fill_is_not_required() {        let block = credential_block(
            "conn-1",
            None,
            AppServerCredentialMode::None,
            &McpTransport::Stdio {
                command: "npx".into(),
                args: vec![],
                env: HashMap::new(),
            },
            &HashMap::new(),
            &HashMap::new(),
            None,
            Some("alice"),
            McpServerStatus::Disconnected,
        );
        assert_eq!(block.status, AppServerCredentialStatus::NotRequired);
        assert!(block.fields.is_empty());
        assert!(block.missing.is_empty());
        // No declaration means no form text either — the client shows no form.
        assert!(block.title.is_none());
    }
}

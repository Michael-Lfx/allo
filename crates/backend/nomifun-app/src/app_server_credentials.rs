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
use nomifun_common::McpServerStatus;
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
}

impl AppServerConnectorCredentials {
    pub fn new(snapshots: Arc<dyn IPluginSnapshotRepository>) -> Self {
        Self { snapshots }
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
}

/// The `credential` block for one connector, ready to attach to a summary.
///
/// Returns `None` when the connector has no declaration **and** the transport
/// needs no credentials — there is nothing to render, so the field stays off the
/// wire rather than describing an empty form.
pub async fn describe(
    credentials: &AppServerConnectorCredentials,
    mcp_server_id: &str,
    transport: &McpTransport,
    values: &HashMap<String, String>,
    installed: &HashMap<String, String>,
    operator: Option<&str>,
    principal: Option<&str>,
    last_test_status: McpServerStatus,
) -> Option<AppServerConnectorCredential> {
    let declaration = credentials.declaration_for(mcp_server_id).await;
    let mode = credential_mode(declaration.as_ref(), transport);
    if declaration.is_none() && mode == AppServerCredentialMode::None {
        return None;
    }
    Some(credential_block(
        mcp_server_id,
        declaration.as_ref(),
        mode,
        transport,
        values,
        installed,
        operator,
        principal,
        last_test_status,
    ))
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
    fn a_connector_with_nothing_to_fill_is_not_required() {
        let block = credential_block(
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

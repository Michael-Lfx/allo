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
}

impl DeclaredField {
    pub fn is_secret(&self) -> bool {
        self.kind == "secret"
    }
}

/// What a connector's marketplace entry declared, as the host stored it.
///
/// The title, the description and the "where do I get a key" link belong to the
/// **form**, not to a field: `token-schema.json` declares them once, at the top
/// level (`34` §5.2). The market ships no per-field documentation, so spreading
/// the form's link across its fields is an invention — and it rendered as
/// 「如何获取密钥？」 under `PORT`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorDeclaration {
    /// The marketplace directory name — the link between a connector row and its
    /// declaration (`34` §5.2).
    pub connector_id: String,
    pub title: Option<AppServerLocalizedString>,
    pub description: Option<AppServerLocalizedString>,
    pub doc_url: Option<AppServerLocalizedString>,
    pub doc_label: Option<AppServerLocalizedString>,
    pub fields: Vec<DeclaredField>,
}

/// Everything the host knows about one connector's credential face.
///
/// The mode and the form come from **two different imported components**, and the
/// two do not exist under the same conditions:
///
/// - the normalized `auth_mode` rides in the `connector` component, which every
///   marketplace connector has — including the 14 that need no form at all
///   (`server-side` / `mcp` / `oneid-token`, `34` §6.1);
/// - the form rides in the `credential` component, which only the 61 connectors
///   that shipped a `token-schema.json` have (`34` §5.2).
///
/// So the mode has to be read from the connector component. Reading it from the
/// form instead makes every declared connector answer `none` — the form payload
/// carries no `auth_mode` — and falling back to the transport re-shows 授权 on the
/// 14 that declare they need nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorCredentialSource {
    pub connector_id: String,
    /// `none` | `oauth` | `token`, as the importer normalized the market index's
    /// `auth_mode` (`34` §6.1).
    pub auth_mode: String,
    /// The form, when this connector shipped a `token-schema.json`.
    pub declaration: Option<ConnectorDeclaration>,
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
            })
        })
        .collect();
    if fields.is_empty() {
        return None;
    }
    Some(ConnectorDeclaration {
        connector_id,
        title: optional_localized(value.get("title")),
        description: optional_localized(value.get("description")),
        // Form-level, read from the payload's own top level — never copied onto
        // the fields below (`34` §5.2).
        doc_url: optional_localized(value.get("doc_url")),
        doc_label: optional_localized(value.get("doc_label")),
        fields,
    })
}

/// One name a connector's template asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TemplateReference {
    name: String,
    /// `true` for `${secret:NAME}` / a whole-value `secret:NAME`, `false` for
    /// `${NAME}` — the two namespaces the resolver distinguishes (`34` §5.1).
    secret: bool,
}

/// Collect the references inside one string, the way the resolver reads it.
///
/// Mirrors [`secret_ref::resolve_request_string`]: a whole-value `secret:NAME`
/// short-circuits (the rest of the string is not scanned), then `${…}` runs are
/// walked. Kept in step with that function deliberately — a scan that looked at
/// different syntax would invent fields nothing can fill.
fn scan_references(value: &str, out: &mut Vec<TemplateReference>) {
    if let Some(name) = secret_ref::parse_secret_ref(value) {
        out.push(TemplateReference { name: name.to_owned(), secret: true });
        return;
    }
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            return;
        };
        let raw = &after[..end];
        match raw.strip_prefix("secret:") {
            Some(name) if !name.is_empty() => {
                out.push(TemplateReference { name: name.to_owned(), secret: true });
            }
            // `${secret:}` names nothing; the resolver keeps it literal.
            Some(_) => {}
            None if !raw.is_empty() => {
                out.push(TemplateReference { name: raw.to_owned(), secret: false });
            }
            None => {}
        }
        rest = &after[end + 1..];
    }
}

/// Every reference a connector's **own transport template** asks for (`34` §5.4).
///
/// The runtime counterpart of the import-time normalization. A marketplace
/// connector declares its form in a `token-schema.json`; a server registered
/// through `connector/register` — or added by hand on the host — has no such file,
/// but its transport still says exactly what has to be sent, and the namespace of
/// each reference is the one the resolver looks in. So the scan **follows the
/// resolver** instead of guessing from names.
///
/// Deterministic: url, then headers and env in key order. First mention wins, and a
/// name that shows up in both namespaces counts as a **secret** — a value meant for
/// the vault must never be routed to the connector's own settings, which are
/// readable and exportable.
///
/// A stdio server's `${NAME}` is deliberately *not* collected: its spawn path
/// resolves against an empty plain-value map (`resolve_env_for`), so nothing could
/// ever fill such a field — reporting it as a missing credential is the honest
/// outcome, and inventing a form for it would only hide the template bug.
fn template_references(transport: &McpTransport) -> Vec<TemplateReference> {
    let mut found: Vec<TemplateReference> = Vec::new();
    match transport {
        McpTransport::Http { url, headers, .. } | McpTransport::Sse { url, headers, .. } => {
            scan_references(url, &mut found);
            let mut names: Vec<&String> = headers.keys().collect();
            names.sort();
            for name in names {
                if let Some(value) = headers.get(name) {
                    scan_references(value, &mut found);
                }
            }
        }
        McpTransport::Stdio { env, .. } => {
            let mut names: Vec<&String> = env.keys().collect();
            names.sort();
            for name in names {
                if let Some(value) = env.get(name) {
                    scan_references(value, &mut found);
                }
            }
            // A stdio server has no plain-value store: its spawn path resolves
            // `${NAME}` against an empty map (`resolve_env_for`), and a plain write
            // to it is refused outright (`write_plain_values`). Offering a field for
            // one would be a form entry that can never take effect, so the template
            // scan keeps only the namespace that can actually be filled. A `${NAME}`
            // left in a stdio `env` is a template bug, and the probe reporting it as
            // a missing reference is the honest outcome.
            found.retain(|reference| reference.secret);
        }
    }

    let mut out: Vec<TemplateReference> = Vec::with_capacity(found.len());
    for reference in found {
        match out.iter_mut().find(|kept| kept.name == reference.name) {
            // Secret wins: see the doc comment above.
            Some(kept) => kept.secret |= reference.secret,
            None => out.push(reference),
        }
    }
    out
}

/// The form a connector's template implies, when no marketplace declaration
/// describes one (`34` §5.4 规则 4 / §6.5).
fn template_declaration(transport: &McpTransport) -> Option<ConnectorDeclaration> {
    let values = transport_values(transport);
    let fields: Vec<DeclaredField> = template_references(transport)
        .into_iter()
        .map(|reference| {
            let label = AppServerLocalizedString {
                zh: reference.name.clone(),
                en: reference.name.clone(),
            };
            DeclaredField {
                required: reference.secret || !values.contains_key(&reference.name),
                kind: if reference.secret { "secret".to_owned() } else { "plain".to_owned() },
                key: reference.name,
                label,
                placeholder: AppServerLocalizedString { zh: String::new(), en: String::new() },
                description: AppServerLocalizedString { zh: String::new(), en: String::new() },
            }
        })
        .collect();
    (!fields.is_empty()).then(|| ConnectorDeclaration {
        // No marketplace directory: this form was not imported from one.
        connector_id: String::new(),
        title: None,
        description: None,
        doc_url: None,
        doc_label: None,
        fields,
    })
}

/// The form a connector has, from both places it can come from.
///
/// The marketplace declaration wins for any name it declares — it carries the
/// author's own labels, placeholders and documentation. Every name the template
/// asks for that no declaration covers is added from the template, which is what
/// gives a hand-registered server a form at all, and what makes an undeclared
/// placeholder reachable instead of unfillable (`34` §5.4 规则 4).
pub fn effective_declaration(
    declared: Option<ConnectorDeclaration>,
    transport: &McpTransport,
) -> Option<ConnectorDeclaration> {
    let Some(from_template) = template_declaration(transport) else {
        return declared;
    };
    let mut declaration = declared.unwrap_or(ConnectorDeclaration {
        connector_id: String::new(),
        title: None,
        description: None,
        doc_url: None,
        doc_label: None,
        fields: Vec::new(),
    });
    for field in from_template.fields {
        if !declaration.fields.iter().any(|existing| existing.key == field.key) {
            declaration.fields.push(field);
        }
    }
    Some(declaration)
}

/// The `credential.mode` for a connector.
///
/// A marketplace declaration decides it, through the normalized `auth_mode`
/// [`ConnectorCredentialSource`] carries. A connector with **no** declaration still
/// has one signal left: if its own template names a secret, it is asking for a key
/// and is reported as `token` — the transport-derived `oauth` was a guess, and the
/// template is a declaration (`34` §6.5). Otherwise the guess stands: a
/// hand-registered remote server with nothing to fill keeps the OAuth entry point,
/// a stdio one needs nothing, and the 61 `token` connectors and the 204 with an
/// empty `auth_mode` no longer get either (`34` §6.1).
pub fn credential_mode(
    source: Option<&ConnectorCredentialSource>,
    transport: &McpTransport,
) -> AppServerCredentialMode {
    match source {
        Some(source) => match source.auth_mode.as_str() {
            "token" => AppServerCredentialMode::Token,
            "oauth" => AppServerCredentialMode::Oauth,
            _ => AppServerCredentialMode::None,
        },
        None if template_references(transport).iter().any(|reference| reference.secret) => {
            AppServerCredentialMode::Token
        }
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
        doc_url: declaration.and_then(|d| d.doc_url.clone()),
        doc_label: declaration.and_then(|d| d.doc_label.clone()),
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

    /// What one registered connector's marketplace entry declared, or `None` when
    /// it was not imported from a marketplace (`34` §5.2).
    ///
    /// The link is the snapshot, not a new table: `find_snapshot_by_mcp_server_id`
    /// already answers "which import registered this `mcp_servers` row", and that
    /// snapshot holds both the `connector` component (which carries the
    /// marketplace directory name **and** the normalized `auth_mode`) and the
    /// `credential` component keyed by the same name.
    pub async fn source_for(&self, mcp_server_id: &str) -> Option<ConnectorCredentialSource> {
        let snapshot = self
            .snapshots
            .find_snapshot_by_mcp_server_id(mcp_server_id)
            .await
            .ok()??;
        let components = self.snapshots.get_components(&snapshot.snapshot_id).await.ok()?;

        // Which marketplace directory registered this row, and how does that
        // entry authenticate? Both answers live in the `connector` component: a
        // `connector_id` is what distinguishes a marketplace entry from the
        // `.mcp.json` path (which hard-codes an `auth_mode` and has no directory
        // of its own).
        let (connector_id, auth_mode) = components
            .iter()
            .filter(|component| component.kind == "connector")
            .find_map(|component| {
                let runtime: Value =
                    serde_json::from_str(component.runtime_ref.as_deref()?).ok()?;
                if runtime.get("mcp_server_id").and_then(Value::as_str) != Some(mcp_server_id) {
                    return None;
                }
                let payload: Value = serde_json::from_str(&component.payload_json).ok()?;
                let connector_id = payload.get("connector_id").and_then(Value::as_str)?;
                // Absent only on a snapshot predating the normalization; `none` is
                // then the fail-closed answer, not a guess at `oauth`.
                let auth_mode = payload
                    .get("auth_mode")
                    .and_then(Value::as_str)
                    .unwrap_or("none");
                Some((connector_id.to_owned(), auth_mode.to_owned()))
            })?;

        let declaration = components
            .iter()
            .filter(|component| component.kind == "credential")
            .filter_map(|component| declaration_from_payload(&component.payload_json))
            .find(|declaration| declaration.connector_id == connector_id);

        Some(ConnectorCredentialSource {
            connector_id,
            auth_mode,
            declaration,
        })
    }

    /// The `credential` block for one connector, as `principal` sees it.
    pub async fn describe(
        &self,
        mcp_server_id: &str,
        transport: &McpTransport,
        last_test_status: McpServerStatus,
        principal: Option<&str>,
    ) -> Option<AppServerConnectorCredential> {
        let source = self.source_for(mcp_server_id).await;
        let mode = credential_mode(source.as_ref(), transport);
        // The form from both places it can come from: the marketplace's declaration
        // and the connector's own template (`34` §6.5).
        let declaration = effective_declaration(
            source.as_ref().and_then(|source| source.declaration.clone()),
            transport,
        );
        // No block only when there is genuinely nothing to say: a connector the host
        // did not import, whose template asks for nothing, and whose transport
        // implies no auth either. A marketplace connector **always** gets one, even
        // with no fields — `mode: none` / `not_required` is how the UI learns that
        // this connector needs no authentication at all (`34` §6.1: 「其余显示无需认证」),
        // and dropping it would leave 216 connectors indistinguishable from ones the
        // host knows nothing about.
        if source.is_none() && declaration.is_none() && mode == AppServerCredentialMode::None {
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
        let declaration = effective_declaration(
            self.source_for(mcp_server_id)
                .await
                .and_then(|source| source.declaration),
            &server.transport,
        )
        .ok_or_else(|| {
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
        // The last probe's verdict was reached against the **previous** value, and a
        // secret write changes what a request carries without changing the
        // transport — so nothing else would invalidate it (`34` §6.1: a successful
        // set clears the `error` state). Without this, filling in the key a
        // connector just rejected leaves the UI reporting 「验证失败」 for a value
        // that is no longer in use.
        writer
            .config
            .clear_test_verdict(&server.mcp_server_id)
            .await
            .map_err(AppError::from)?;
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
        let server = writer
            .config
            .get_server(&parse_server_id(mcp_server_id)?)
            .await
            .map_err(AppError::from)?;
        let declaration = effective_declaration(
            self.source_for(mcp_server_id)
                .await
                .and_then(|source| source.declaration),
            &server.transport,
        );
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

fn parse_server_id(id: &str) -> Result<nomifun_api_types::McpServerId, AppError> {
    nomifun_api_types::McpServerId::parse(id)
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

/// Move the host-level `[credentials]` entries under the installation owner
/// (`34` §9 第 6 步).
///
/// One-time and idempotent, and a **no-op unless the host has declared who owns
/// it** — the caller passes the owner it just declared, so a host that never
/// declares one keeps the single-user reading of `[credentials]` it has always
/// had. The file is read, rewritten only when something actually moved, written
/// atomically and owner-only, and the in-process map is refreshed from the result
/// so the rename takes effect without a restart.
///
/// Returns what happened, including the conflicts that were deliberately left
/// alone; the caller reports them.
pub fn scope_host_level_credentials(
    path: &std::path::Path,
    owner: &str,
) -> Result<nomifun_app_server::agent_store::CredentialScoping, AppError> {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        // No config file yet: there is nothing to migrate, and creating one here
        // would invent a file the operator never asked for.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Default::default());
        }
        Err(error) => {
            return Err(AppError::Internal(format!("read config: {error}")));
        }
    };
    let (document, report) = nomifun_app_server::agent_store::AgentStoreConfig::scope_credentials(
        &source, owner,
    )
    .map_err(AppError::Internal)?;
    if report.changed_anything() {
        write_private(path, &document)?;
        reload_credentials(path);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomifun_common::secret_ref::scoped_key;

    /// A stored `credential` component payload — what the importer writes
    /// (`credential_component` in `nomifun-importer`), which carries **no**
    /// `auth_mode`: the mode is not the form's business.
    fn declaration_payload() -> String {
        serde_json::json!({
            "connector_id": "tdengine",
            "title": { "zh": "TDengine 配置", "en": "TDengine configuration" },
            "doc_url": { "zh": "https://docs.example.com/tdengine", "en": "https://docs.example.com/tdengine/en" },
            "doc_label": { "zh": "如何获取密钥？", "en": "" },
            "fields": [
                { "key": "TDENGINE_API_KEY", "kind": "secret", "required": true,
                  "label": { "zh": "密钥", "en": "Key" } },
                { "key": "TDENGINE_API_HOST", "kind": "plain", "required": false,
                  "label": { "zh": "主机", "en": "Host" } },
            ],
        })
        .to_string()
    }

    fn source(auth_mode: &str) -> ConnectorCredentialSource {
        ConnectorCredentialSource {
            connector_id: "tdengine".to_owned(),
            auth_mode: auth_mode.to_owned(),
            declaration: declaration_from_payload(&declaration_payload()),
        }
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
        // An `auth_mode` in the form payload is ignored, not believed: the mode is
        // read from the connector component (`ConnectorCredentialSource`).
        let with_mode: Value = serde_json::from_str(&declaration_payload()).unwrap();
        assert!(with_mode.get("auth_mode").is_none());
    }

    #[test]
    fn the_mode_comes_from_the_market_index_and_the_transport_is_only_a_fallback() {
        assert_eq!(
            credential_mode(Some(&source("token")), &http_transport()),
            AppServerCredentialMode::Token
        );
        assert_eq!(
            credential_mode(Some(&source("oauth")), &http_transport()),
            AppServerCredentialMode::Oauth
        );

        // The market's empty / `server-side` / `mcp` / `oneid-token` all mean
        // "nothing the client can fill" — this is the 204-connector behaviour
        // change (`34` §6.1). `server-side` / `mcp` / `oneid-token` matter for a
        // second reason: their 14 connectors ship no `token-schema.json`, so there
        // is a source and no form.
        for mode in ["", "none", "server-side", "mcp", "oneid-token"] {
            assert_eq!(
                credential_mode(Some(&source(mode)), &http_transport()),
                AppServerCredentialMode::None,
                "{mode} must not offer an auth entry point"
            );
        }

        // No source: keep what a hand-registered server has always shown.
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

    /// The wiring that step 5 caught: a declaration with no `auth_mode` of its own
    /// must still report `token`, because the mode comes from the connector
    /// component. Before this, `credential_mode` was handed the form's parsed
    /// `auth_mode` — which the importer never writes — so all 61 `token`
    /// connectors answered `none` and the WebUI had no form to render.
    #[test]
    fn a_declared_token_connector_reports_token_without_an_auth_mode_in_the_form() {
        let source = source("token");
        let declaration = source.declaration.as_ref().expect("a form");
        assert!(
            !declaration_payload().contains("auth_mode"),
            "the fixture must stay faithful to what the importer stores"
        );
        assert_eq!(
            credential_mode(Some(&source), &http_transport()),
            AppServerCredentialMode::Token
        );
        assert_eq!(declaration.fields.len(), 2);
    }

    /// The 14 `server-side` / `mcp` / `oneid-token` connectors: a source, no form,
    /// and therefore no auth affordance — the transport must not override that.
    #[test]
    fn a_source_without_a_form_still_reports_none_on_a_url_transport() {
        let source = ConnectorCredentialSource {
            connector_id: "server-side-demo".to_owned(),
            auth_mode: "none".to_owned(),
            declaration: None,
        };
        assert_eq!(
            credential_mode(Some(&source), &http_transport()),
            AppServerCredentialMode::None
        );
    }

    // -----------------------------------------------------------------------
    // The form a connector declares by its own template (`34` §6.5)
    // -----------------------------------------------------------------------

    /// A hand-registered server: no snapshot, no `token-schema.json`, and a
    /// transport that names what it needs.
    fn registered_http(url: &str, headers: &[(&str, &str)], values: &[(&str, &str)]) -> McpTransport {
        McpTransport::Http {
            url: url.to_owned(),
            headers: headers
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
            values: values
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
        }
    }

    fn names(transport: &McpTransport) -> Vec<(String, bool)> {
        template_references(transport)
            .into_iter()
            .map(|reference| (reference.name, reference.secret))
            .collect()
    }

    #[test]
    fn the_template_names_what_it_needs_in_the_resolvers_own_namespaces() {
        // url first, then headers in key order; `${secret:…}` is a secret and
        // `${…}` is one of the connector's own settings.
        assert_eq!(
            names(&registered_http(
                "https://${SCHEMA}://${HOST}:${PORT}/mcp",
                &[("Authorization", "Bearer ${secret:API_KEY}"), ("X-Tenant", "${TENANT}")],
                &[],
            )),
            vec![
                ("SCHEMA".to_owned(), false),
                ("HOST".to_owned(), false),
                ("PORT".to_owned(), false),
                ("API_KEY".to_owned(), true),
                ("TENANT".to_owned(), false),
            ]
        );
    }

    #[test]
    fn a_whole_value_reference_counts_and_a_name_in_both_namespaces_is_a_secret() {
        // The whole-value form is what the pre-`34` env maps used, and the resolver
        // short-circuits on it — so the scan has to as well. The `${TENANT}` beside
        // it yields no field: a stdio server has nowhere to store a plain value.
        let stdio = McpTransport::Stdio {
            command: "npx".into(),
            args: vec![],
            env: HashMap::from([
                ("TOKEN".to_owned(), "secret:API_KEY".to_owned()),
                ("TENANT".to_owned(), "${TENANT}".to_owned()),
            ]),
        };
        assert_eq!(names(&stdio), vec![("API_KEY".to_owned(), true)]);

        // The same name in both namespaces — `${DUP}` in one header and
        // `${secret:DUP}` in another. The vault reading wins, because the other one
        // would write the value into the connector's own settings, which are readable
        // and exportable.
        assert_eq!(
            names(&registered_http(
                "https://x/mcp",
                &[("A", "${DUP}"), ("B", "${secret:DUP}")],
                &[],
            )),
            vec![("DUP".to_owned(), true)]
        );
    }

    #[test]
    fn a_registered_server_gets_a_form_and_reports_token() {
        let transport = registered_http(
            "https://mcp.acme.com/mcp",
            &[("Authorization", "Bearer ${secret:ACME_KEY}")],
            &[],
        );

        // No declaration, but the template is one.
        assert_eq!(credential_mode(None, &transport), AppServerCredentialMode::Token);
        let declaration = effective_declaration(None, &transport).expect("a form");
        assert_eq!(declaration.fields.len(), 1);
        assert_eq!(declaration.fields[0].key, "ACME_KEY");
        assert!(declaration.fields[0].is_secret());
        assert!(declaration.fields[0].required);
        // The label falls back to the key: there is no author to name it.
        assert_eq!(declaration.fields[0].label.zh, "ACME_KEY");

        // …and the write face accepts exactly that key, which is what makes
        // `credential/set` usable for a server the host never imported.
        let plains = route_values(
            &declaration,
            &HashMap::from([("ACME_KEY".to_owned(), "v".to_owned())]),
        )
        .expect("the template's own key is writable");
        assert_eq!(plains.0, vec![("ACME_KEY".to_owned(), "v".to_owned())]);
        assert!(
            route_values(
                &declaration,
                &HashMap::from([("SOMETHING_ELSE".to_owned(), "v".to_owned())])
            )
            .is_err(),
            "a key the template never names must still be refused"
        );

        // The block then shows the form and the missing key, per caller.
        let block = credential_block(
            "conn-1",
            Some(&declaration),
            AppServerCredentialMode::Token,
            &transport,
            &HashMap::new(),
            &HashMap::new(),
            None,
            Some("alice"),
            McpServerStatus::Disconnected,
        );
        assert_eq!(block.status, AppServerCredentialStatus::RequiresInput);
        assert_eq!(block.missing, vec!["ACME_KEY".to_owned()]);
        assert_eq!(block.fields[0].value, None, "a secret's value never crosses");
    }

    #[test]
    fn a_template_derived_field_joins_the_declared_ones_without_replacing_them() {
        // The declaration keeps its author-written labels; the template only adds
        // what nothing declared.
        let declared = declaration_from_payload(&declaration_payload()).expect("declaration");
        let transport = registered_http(
            "https://${TDENGINE_API_HOST}/mcp",
            &[
                ("Authorization", "Bearer ${secret:TDENGINE_API_KEY}"),
                ("X-Extra", "${secret:EXTRA_KEY}"),
            ],
            &[],
        );
        let merged = effective_declaration(Some(declared), &transport).expect("a form");
        let keys: Vec<&str> = merged.fields.iter().map(|field| field.key.as_str()).collect();
        assert_eq!(keys, vec!["TDENGINE_API_KEY", "TDENGINE_API_HOST", "EXTRA_KEY"]);
        // The declared field is untouched — including the i18n the author wrote.
        assert_eq!(merged.fields[0].label.zh, "密钥");
        assert_eq!(merged.title.as_ref().map(|t| t.zh.as_str()), Some("TDengine 配置"));
        // The added one is required and carries no copy.
        assert!(merged.fields[2].required);
        assert_eq!(merged.fields[2].label.zh, "EXTRA_KEY");
    }

    #[test]
    fn a_plain_reference_with_a_value_is_a_prefilled_setting_not_a_missing_one() {
        let transport = registered_http(
            "https://${HOST}:${PORT}/mcp",
            &[],
            &[("HOST", "mcp.acme.com"), ("PORT", "443")],
        );
        let declaration = effective_declaration(None, &transport).expect("a form");
        assert!(declaration.fields.iter().all(|field| !field.is_secret()));
        assert!(
            declaration.fields.iter().all(|field| !field.required),
            "a value in `values` is already satisfied: {:?}",
            declaration.fields
        );
        // A reference with nothing behind it *is* missing, and fillable.
        let transport = registered_http("https://${HOST}:${PORT}/mcp", &[], &[("HOST", "h")]);
        let declaration = effective_declaration(None, &transport).expect("a form");
        let port = declaration
            .fields
            .iter()
            .find(|field| field.key == "PORT")
            .expect("PORT");
        assert!(port.required);

        let block = credential_block(
            "conn-1",
            Some(&declaration),
            AppServerCredentialMode::Token,
            &transport,
            &HashMap::from([("HOST".to_owned(), "h".to_owned())]),
            &HashMap::new(),
            None,
            Some("alice"),
            McpServerStatus::Disconnected,
        );
        assert_eq!(block.missing, vec!["PORT".to_owned()]);
        assert_eq!(
            block.fields.iter().find(|f| f.key == "HOST").and_then(|f| f.value.clone()),
            Some("h".to_owned()),
            "a plain field carries the value in effect"
        );
    }

    #[test]
    fn a_template_with_nothing_to_fill_changes_nothing() {
        // A URL with no references: no form, and the transport-derived answer stands
        // (this is the hand-registered remote server the doc already covers).
        let plain = registered_http("https://mcp.example.com/mcp", &[], &[]);
        assert!(template_references(&plain).is_empty());
        assert!(effective_declaration(None, &plain).is_none());
        assert_eq!(credential_mode(None, &plain), AppServerCredentialMode::Oauth);

        // …and a stdio server with no secret reference still needs nothing.
        let stdio = McpTransport::Stdio {
            command: "npx".into(),
            args: vec!["-y".into(), "some-mcp".into()],
            env: HashMap::from([("NO_PROXY".to_owned(), "*".to_owned())]),
        };
        assert!(effective_declaration(None, &stdio).is_none());
        assert_eq!(credential_mode(None, &stdio), AppServerCredentialMode::None);

        // A secret in a stdio server's env is the case that used to report nothing
        // at all: no snapshot, no declaration, `mode: none`.
        let with_secret = McpTransport::Stdio {
            command: "npx".into(),
            args: vec![],
            env: HashMap::from([("API_KEY".to_owned(), "secret:API_KEY".to_owned())]),
        };
        assert_eq!(credential_mode(None, &with_secret), AppServerCredentialMode::Token);
        assert_eq!(
            effective_declaration(None, &with_secret)
                .expect("a form")
                .fields
                .len(),
            1
        );
    }

    /// The "where do I get a key" link belongs to the **form**, not to a field
    /// (`34` §5.2): the market declares one `docUrl` per `token-schema.json`.
    ///
    /// The projection used to read it out of the payload for *every* field, which
    /// put 「如何获取密钥？」 under `PORT` and repeated the same link four times in
    /// the live WebUI.
    #[test]
    fn the_documentation_link_is_form_level_and_not_repeated_per_field() {
        let declaration = declaration_from_payload(&declaration_payload()).expect("declaration");
        assert_eq!(
            declaration.doc_url.as_ref().map(|url| url.zh.as_str()),
            Some("https://docs.example.com/tdengine")
        );
        assert_eq!(
            declaration.doc_url.as_ref().map(|url| url.en.as_str()),
            Some("https://docs.example.com/tdengine/en")
        );

        let block = credential_block(
            "conn-1",
            Some(&declaration),
            AppServerCredentialMode::Token,
            &http_transport(),
            &HashMap::new(),
            &HashMap::new(),
            None,
            None,
            McpServerStatus::Disconnected,
        );
        assert_eq!(
            block.doc_url.as_ref().map(|url| url.zh.as_str()),
            Some("https://docs.example.com/tdengine")
        );
        assert_eq!(
            block.doc_label.as_ref().map(|label| label.zh.as_str()),
            Some("如何获取密钥？")
        );
        // The field shape has nowhere to put it — asserted on the wire form so a
        // future re-addition has to argue with this test.
        let json = serde_json::to_value(&block).expect("serializable");
        for field in json["fields"].as_array().expect("fields") {
            assert!(field.get("doc_url").is_none(), "{field}");
            assert!(field.get("doc_label").is_none(), "{field}");
        }
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

    /// The storage migration, end to end on a real file (`34` §9 第 6 步).
    #[test]
    fn host_level_credentials_are_scoped_to_the_owner_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "# hand-edited\n[credentials]\n# the demo token\nDEMO_TOKEN = \"v\"\n\"bob:OWN\" = \"b\"\n",
        )
        .unwrap();

        let report = scope_host_level_credentials(&path, "alice").expect("migration");
        assert_eq!(report.moved, vec!["DEMO_TOKEN"]);
        assert!(report.conflicts.is_empty());

        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("# hand-edited"), "{written}");
        assert!(written.contains("# the demo token"), "{written}");
        // The value is still there — under a name that says whose it is.
        let reparsed =
            nomifun_app_server::agent_store::AgentStoreConfig::from_source(&written).unwrap();
        assert_eq!(reparsed.credentials.get("alice:DEMO_TOKEN").map(String::as_str), Some("v"));
        assert_eq!(reparsed.credentials.get("bob:OWN").map(String::as_str), Some("b"));
        assert!(!reparsed.credentials.contains_key("DEMO_TOKEN"), "{written}");

        // The in-process map was refreshed, so the rename is live in this process:
        // the owner resolves it, and the host-internal path acts for the owner.
        assert_eq!(
            secret_ref::lookup_for_with(
                Some("alice"),
                "DEMO_TOKEN",
                &reparsed.credentials,
                Some("alice")
            )
            .as_deref(),
            Some("v")
        );
        assert_eq!(
            secret_ref::lookup_for_with(None, "DEMO_TOKEN", &reparsed.credentials, Some("alice"))
                .as_deref(),
            Some("v")
        );
        // …and a second principal still sees nothing, which is the acceptance item
        // the whole feature exists for.
        assert_eq!(
            secret_ref::lookup_for_with(
                Some("bob"),
                "DEMO_TOKEN",
                &reparsed.credentials,
                Some("alice")
            ),
            None
        );

        // Idempotent, and it does not rewrite the file a second time: the mtime is
        // the cheap witness that a no-op stayed a no-op.
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();
        let again = scope_host_level_credentials(&path, "alice").expect("second pass");
        assert!(again.is_empty(), "{again:?}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), written);
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), before);
    }

    #[test]
    fn scoping_a_host_with_no_config_file_invents_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing").join("config.toml");
        let report = scope_host_level_credentials(&path, "alice").expect("no file is not an error");
        assert!(report.is_empty());
        assert!(!path.exists(), "a host with no config must not get one");
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

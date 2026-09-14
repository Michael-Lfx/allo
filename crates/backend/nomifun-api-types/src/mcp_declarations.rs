//! Host-owned MCP server declarations — the typed form of
//! `~/.agent-store/mcp.json` (`docs/agent-store/20-tool-injection-policy.zh.md`
//! §7.9, decision `21` D14).
//!
//! The file shape follows the reference implementation (Kimi Code CLI
//! `mcp.json`): a single `mcpServers` object whose entries are stdio servers
//! (`command` + `args`/`env`) or remote servers (`url` + `headers`, with
//! `transport: "sse"` selecting the legacy SSE transport).
//!
//! This is **host configuration, not client input**: it is read from the host's
//! own file and injected through the agent factory's process-owned dependencies,
//! never through conversation `extra` JSON, so no request can forge a
//! declaration. Declaring a server widens the *sources* of the tool surface, but
//! never the *policy* over it — the host `[tools]` denylist stays the last layer
//! applied (`20` §7.9).
//!
//! Every optional field the reference implementation documents is honoured
//! (`env`, `cwd`, `headers`, `bearerTokenEnvVar`, `enabled`, `startupTimeoutMs`,
//! `toolTimeoutMs`, `enabledTools`, `disabledTools`), so a copied `mcp.json`
//! loads unchanged — with one bounded exception: a timeout above
//! [`MAX_TOOL_TIMEOUT_MS`] is refused here although the reference implementation
//! permits it (see that constant).
//!
//! Three deliberate differences are enforced here rather than only documented:
//!
//! - any unknown field **rejects its own entry** instead of being silently
//!   ignored — ignoring `enabledTools` would leave tools the user believed
//!   excluded callable;
//! - a field that does not apply to the chosen transport (`headers` or
//!   `bearerTokenEnvVar` on a stdio entry, `args`/`env`/`cwd` on a remote entry)
//!   is rejected for the same reason;
//! - the project-level `<workspace>/.agent-store/mcp.json` layer is **not** read
//!   (the path is reserved, not implemented).
//!
//! Failure granularity is deliberate: a malformed file is a file-level error
//! (the caller then declares nothing — never a widened surface), while a bad
//! entry rejects only itself so one typo cannot disable every server.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde::Deserialize;

use crate::McpTransport;

/// Maximum declaration key length.
///
/// Derived, not chosen: the provider-visible tool name is
/// `mcp__` + `sanitize("<server>__<tool>")` (truncated) + `__` + a 16-character
/// digest, bounded by `MAX_PROVIDER_TOOL_NAME_LEN = 64`
/// (`nomi-mcp/src/tool_proxy.rs`). The slug budget is therefore
/// `64 - 4 - 2 - 16 = 42`, and requiring the server's own separator to survive
/// truncation means `len(key) + 2 <= 42`.
///
/// That is a **conservative** bound — prefix matching tolerates a little more —
/// but it keeps the rule `mcp__<key>__*` obvious instead of depending on the
/// separator being cut mid-way. Pinned by
/// `factory::nomi::tests::declaration_keys_stay_addressable_by_a_whole_server_pattern`.
pub const MAX_DECLARATION_KEY_LEN: usize = 40;

/// Minimum accepted `toolTimeoutMs`. Zero is refused (it can never be intended);
/// a sub-second value is accepted and rounded **up** to the engine's whole-second
/// granularity, so a declaration never gets less time than it asked for.
pub const MIN_TOOL_TIMEOUT_MS: u64 = 1;

/// Maximum accepted `toolTimeoutMs` — the engine rejects a per-server request
/// timeout above 600 seconds (`nomi-mcp/src/manager.rs`).
///
/// This is the one place a copied `mcp.json` can still be refused: the reference
/// implementation permits up to `2147483647` ms. The bound is deliberate — a
/// lost MCP response has to become an actionable tool error rather than an Agent
/// turn that runs for hours — so an out-of-range value is **reported, not
/// clamped**.
pub const MAX_TOOL_TIMEOUT_MS: u64 = 600_000;

/// Minimum accepted `startupTimeoutMs`; zero is refused, mirroring
/// [`MIN_TOOL_TIMEOUT_MS`].
pub const MIN_STARTUP_TIMEOUT_MS: u64 = 1;

/// Maximum accepted `startupTimeoutMs` — the same ceiling as a tool call.
///
/// The connect timeout covers spawning the child, the `initialize` handshake and
/// `tools/list`. A hang there blocks the whole Agent bootstrap, and the engine's
/// own default is 30 seconds (`MCP_CONNECT_TIMEOUT`), so a larger declared value
/// is honoured but bounded rather than unbounded.
pub const MAX_STARTUP_TIMEOUT_MS: u64 = 600_000;

/// The accepted entry fields, appended to serde's unknown-field message: serde
/// says which key it did not expect, never which keys exist.
const ACCEPTED_FIELDS: &str = "`command`, `args`, `env`, `cwd`, `url`, `headers`, \
     `bearerTokenEnvVar`, `transport`, `enabled`, `startupTimeoutMs`, `toolTimeoutMs`, \
     `enabledTools`, `disabledTools`";

/// The raw file: `mcpServers` is kept as per-entry JSON so one bad entry cannot
/// invalidate the whole file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDeclarations {
    #[serde(rename = "mcpServers", default)]
    mcp_servers: BTreeMap<String, serde_json::Value>,
}

/// One entry, parsed strictly: an unknown key is a deserialization error rather
/// than a silently dropped field.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawServer {
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Option<Vec<String>>,
    #[serde(default)]
    env: Option<BTreeMap<String, String>>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    headers: Option<BTreeMap<String, String>>,
    #[serde(default)]
    transport: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    tool_timeout_ms: Option<u64>,
    // --- reference-implementation optional fields, all honoured ---
    /// stdio: the child's working directory. Stored as declared; the host
    /// resolves a relative path against the declaration file's directory.
    #[serde(default)]
    cwd: Option<String>,
    /// remote: name of an environment variable (or `config.toml [credentials]`
    /// entry) holding the bearer token. Never reaches the engine — the host
    /// turns it into an `Authorization` header.
    #[serde(default)]
    bearer_token_env_var: Option<String>,
    /// all transports: connect handshake timeout.
    #[serde(default)]
    startup_timeout_ms: Option<u64>,
    /// all transports: per-server tool allowlist.
    #[serde(default)]
    enabled_tools: Option<Vec<String>>,
    /// all transports: per-server tool blocklist, applied after the allowlist.
    #[serde(default)]
    disabled_tools: Option<Vec<String>>,
}

/// A declaration that was read and accepted.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedNomiMcpServer {
    /// The `mcpServers` key. It becomes the engine's server name and therefore
    /// the `<server>` segment of every `mcp__<server>__<tool>` tool name.
    pub name: String,
    pub transport: McpTransport,
    /// `false` keeps the entry visible (read view) but out of every session.
    pub enabled: bool,
    /// Mirrors the engine's per-server request timeout; see
    /// [`MAX_TOOL_TIMEOUT_MS`]. `None` leaves the engine's own default in place.
    pub request_timeout_secs: Option<u64>,
    /// `cwd` for a stdio server, as declared — possibly relative. The host calls
    /// [`NomiMcpDeclarations::resolve_cwd_relative_to`] before handing it to the
    /// engine so one file cannot mean two directories on two launches.
    pub cwd: Option<String>,
    /// `bearerTokenEnvVar` for a remote server. Resolved by the host into an
    /// `Authorization: Bearer <value>` header; the engine never sees this field.
    pub bearer_token_env_var: Option<String>,
    /// Mirrors the engine's connect handshake timeout; see
    /// [`MAX_STARTUP_TIMEOUT_MS`]. `None` leaves the engine's own 30s default.
    pub startup_timeout_secs: Option<u64>,
    /// `enabledTools`: when present, only a matching tool of this server is
    /// registered. `None` = no allowlist.
    pub enabled_tools: Option<Vec<String>>,
    /// `disabledTools`: a matching tool of this server is never registered.
    /// Applied **after** `enabled_tools`, so a pattern in both lists excludes.
    /// `None` = no blocklist.
    pub disabled_tools: Option<Vec<String>>,
}

impl ResolvedNomiMcpServer {
    /// Transport label for read views (`stdio` / `http` / `sse`).
    pub fn transport_kind(&self) -> &'static str {
        match self.transport {
            McpTransport::Stdio { .. } => "stdio",
            McpTransport::Sse { .. } => "sse",
            McpTransport::Http { .. } => "http",
        }
    }
}

/// An entry that was refused, with the reason the user needs to fix it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NomiMcpDeclarationRejection {
    pub name: String,
    pub reason: String,
}

/// The resolved declaration set. `Default` is empty, so a host that never wrote
/// `mcp.json` behaves exactly as before this type existed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NomiMcpDeclarations {
    /// Accepted entries, ordered by key.
    pub servers: Vec<ResolvedNomiMcpServer>,
    pub rejected: Vec<NomiMcpDeclarationRejection>,
}

impl NomiMcpDeclarations {
    /// Parse a `mcp.json` body.
    ///
    /// `Err` is reserved for **file-level** problems (invalid JSON, unknown
    /// top-level key, `mcpServers` not an object): the caller then declares
    /// nothing at all. A structurally invalid *entry* lands in
    /// [`Self::rejected`] and does not affect its siblings.
    pub fn parse(source: &str) -> Result<Self, String> {
        // Parsed through `Value` first: serde's derive would happily read the
        // struct from a JSON *array*, and a wrong document shape must be a loud
        // file-level error rather than a silently empty declaration set.
        let value: serde_json::Value = serde_json::from_str(source)
            .map_err(|error| format!("mcp.json is not valid JSON: {error}"))?;
        if !value.is_object() {
            return Err("mcp.json must be a JSON object at the top level".to_owned());
        }
        let raw: RawDeclarations = serde_json::from_value(value)
            .map_err(|error| format!("mcp.json is not a valid declaration file: {error}"))?;

        let mut servers = Vec::with_capacity(raw.mcp_servers.len());
        let mut rejected = Vec::new();
        for (name, value) in raw.mcp_servers {
            match resolve_entry(&name, value) {
                Ok(server) => servers.push(server),
                Err(reason) => rejected.push(NomiMcpDeclarationRejection { name, reason }),
            }
        }
        Ok(Self { servers, rejected })
    }

    /// True when nothing was declared and nothing was refused.
    pub fn is_empty(&self) -> bool {
        self.servers.is_empty() && self.rejected.is_empty()
    }

    /// The entries a session may actually connect to.
    pub fn enabled_servers(&self) -> impl Iterator<Item = &ResolvedNomiMcpServer> {
        self.servers.iter().filter(|server| server.enabled)
    }

    /// Resolve every declared `cwd` against the directory holding `mcp.json`.
    ///
    /// `cwd` is relative **to the declaration file**, by definition: the same
    /// file is loaded by hosts started from arbitrary working directories, so
    /// resolving against the process CWD would make one file mean two different
    /// directories on two launches. An absolute path is kept verbatim.
    ///
    /// A path that is not valid UTF-8 is replaced lossily rather than dropped:
    /// the child spawn then reports a real error against a real path instead of
    /// the entry silently running in the wrong directory.
    pub fn resolve_cwd_relative_to(&mut self, directory: &Path) {
        for server in &mut self.servers {
            let Some(cwd) = server.cwd.as_deref() else {
                continue;
            };
            let path = Path::new(cwd);
            if path.is_absolute() {
                continue;
            }
            server.cwd = Some(directory.join(path).to_string_lossy().into_owned());
        }
    }
}

fn resolve_entry(name: &str, value: serde_json::Value) -> Result<ResolvedNomiMcpServer, String> {
    validate_key(name)?;

    let raw: RawServer = serde_json::from_value(value).map_err(|error| explain(&error))?;
    let startup_timeout_secs = whole_seconds(
        "startupTimeoutMs",
        raw.startup_timeout_ms,
        MIN_STARTUP_TIMEOUT_MS,
        MAX_STARTUP_TIMEOUT_MS,
    )?;
    let request_timeout_secs = whole_seconds(
        "toolTimeoutMs",
        raw.tool_timeout_ms,
        MIN_TOOL_TIMEOUT_MS,
        MAX_TOOL_TIMEOUT_MS,
    )?;

    let RawServer {
        command,
        args,
        env,
        url,
        headers,
        transport,
        enabled,
        tool_timeout_ms: _,
        cwd,
        bearer_token_env_var,
        startup_timeout_ms: _,
        enabled_tools,
        disabled_tools,
    } = raw;
    let command = trim_non_empty(command);
    let url = trim_non_empty(url);
    let cwd = trim_non_empty(cwd);
    let bearer_token_env_var = trim_non_empty(bearer_token_env_var);
    let enabled_tools = normalise_tool_filter("enabledTools", enabled_tools)?;
    let disabled_tools = normalise_tool_filter("disabledTools", disabled_tools)?;

    let transport = match (command, url, transport.as_deref()) {
        (Some(command), None, None) => {
            reject_remote_only_fields(&headers, &bearer_token_env_var, "a stdio server (`command`)")?;
            McpTransport::Stdio {
                command,
                args: args.unwrap_or_default(),
                env: to_map(env),
            }
        }
        (None, Some(url), None) => {
            reject_stdio_only_fields(&args, &env, &cwd, "a remote server (`url`)")?;
            McpTransport::Http {
                url,
                headers: to_map(headers),
            }
        }
        (None, Some(url), Some("sse")) => {
            reject_stdio_only_fields(&args, &env, &cwd, "a remote server (`url`)")?;
            McpTransport::Sse {
                url,
                headers: to_map(headers),
            }
        }
        (None, Some(_), Some(other)) => {
            return Err(format!(
                "`transport` is `{other}`, but `sse` is the only accepted value — omit `transport` \
                 entirely for a Streamable HTTP server"
            ));
        }
        (Some(_), Some(_), _) => {
            return Err(
                "an entry must be either a stdio server (`command`) or a remote server (`url`), \
                 not both"
                    .to_owned(),
            );
        }
        (None, None, _) => {
            return Err("an entry needs either `command` (stdio) or `url` (HTTP/SSE)".to_owned());
        }
        (Some(_), None, Some(other)) => {
            return Err(format!(
                "`transport` is `{other}`, but it only applies to a remote server (`url`); \
                 a `command` entry is always stdio"
            ));
        }
    };

    Ok(ResolvedNomiMcpServer {
        name: name.to_owned(),
        transport,
        enabled: enabled.unwrap_or(true),
        request_timeout_secs,
        cwd,
        bearer_token_env_var,
        startup_timeout_secs,
        enabled_tools,
        disabled_tools,
    })
}

/// Convert a reference-implementation millisecond timeout into the engine's
/// whole-second granularity, refusing values outside the engine's own bound.
///
/// A sub-second value is rounded **up**, so a declaration never gets less time
/// than it asked for; an absent value stays absent so the engine default applies;
/// an out-of-range value is an error rather than a silent clamp, because the
/// declaration file is the user's only statement of intent.
fn whole_seconds(
    field: &str,
    milliseconds: Option<u64>,
    min: u64,
    max: u64,
) -> Result<Option<u64>, String> {
    let Some(milliseconds) = milliseconds else {
        return Ok(None);
    };
    if !(min..=max).contains(&milliseconds) {
        return Err(format!(
            "`{field}` must be between {min} and {max} milliseconds, got {milliseconds}"
        ));
    }
    Ok(Some(milliseconds.div_ceil(1_000)))
}

/// Normalise a per-server tool filter: trim every entry, refuse a blank one (it
/// could never name a tool), collapse duplicates, and read an **empty** list as
/// "no filter" so `"enabledTools": []` cannot mean "no tools".
fn normalise_tool_filter(
    field: &str,
    values: Option<Vec<String>>,
) -> Result<Option<Vec<String>>, String> {
    let Some(values) = values else {
        return Ok(None);
    };
    let mut normalised: Vec<String> = Vec::with_capacity(values.len());
    for value in values {
        let value = value.trim().to_owned();
        if value.is_empty() {
            return Err(format!("`{field}` must not contain an empty entry"));
        }
        if !normalised.contains(&value) {
            normalised.push(value);
        }
    }
    if normalised.is_empty() {
        return Ok(None);
    }
    Ok(Some(normalised))
}

fn reject_remote_only_fields(
    headers: &Option<BTreeMap<String, String>>,
    bearer_token_env_var: &Option<String>,
    what: &str,
) -> Result<(), String> {
    if headers.as_ref().is_some_and(|values| !values.is_empty()) {
        return Err(format!("`headers` only applies to a remote server, but this entry is {what}"));
    }
    if bearer_token_env_var.is_some() {
        return Err(format!(
            "`bearerTokenEnvVar` only applies to a remote server, but this entry is {what}"
        ));
    }
    Ok(())
}

fn reject_stdio_only_fields(
    args: &Option<Vec<String>>,
    env: &Option<BTreeMap<String, String>>,
    cwd: &Option<String>,
    what: &str,
) -> Result<(), String> {
    if args.as_ref().is_some_and(|values| !values.is_empty()) {
        return Err(format!("`args` only applies to a stdio server, but this entry is {what}"));
    }
    if env.as_ref().is_some_and(|values| !values.is_empty()) {
        return Err(format!("`env` only applies to a stdio server, but this entry is {what}"));
    }
    if cwd.is_some() {
        return Err(format!("`cwd` only applies to a stdio server, but this entry is {what}"));
    }
    Ok(())
}

fn trim_non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// The declaration key becomes the engine's server name, and the user's
/// whole-server denylist pattern is `mcp__<key>__*`, so the key has to survive
/// the engine's slug sanitization byte-for-byte.
fn validate_key(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("the server key must not be empty".to_owned());
    }
    if name.len() > MAX_DECLARATION_KEY_LEN {
        return Err(format!(
            "the server key is {} characters, but at most {MAX_DECLARATION_KEY_LEN} are addressable \
             (`mcp__<key>__*` has to fit the engine's 42-character tool-name slug)",
            name.len()
        ));
    }
    let alphanumeric = |byte: u8| byte.is_ascii_alphanumeric();
    let bytes = name.as_bytes();
    if !alphanumeric(bytes[0]) || !alphanumeric(bytes[bytes.len() - 1]) {
        return Err(
            "the server key must start and end with an ASCII letter or digit (the engine trims \
             `_` from tool-name slugs, which would break `mcp__<key>__*`)"
                .to_owned(),
        );
    }
    if !bytes
        .iter()
        .all(|byte| alphanumeric(*byte) || matches!(byte, b'_' | b'-'))
    {
        return Err("the server key may only contain ASCII letters, digits, `_` and `-`".to_owned());
    }
    Ok(())
}

/// Turn a strict-entry deserialization error into something actionable: serde
/// names the key it did not expect but never the keys that exist.
fn explain(error: &serde_json::Error) -> String {
    let message = error.to_string();
    if message.contains("unknown field") {
        return format!("{message} — accepted fields are {ACCEPTED_FIELDS}");
    }
    message
}

fn to_map(values: Option<BTreeMap<String, String>>) -> HashMap<String, String> {
    values.unwrap_or_default().into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a one-entry file body. `key` is JSON-escaped by `{:?}`, so odd keys
    /// (dots, spaces, non-ASCII) reach the validator intact.
    fn source_for(key: &str, body: &str) -> String {
        format!("{{ \"mcpServers\": {{ {key:?}: {body} }} }}")
    }

    fn accepted(source: &str) -> NomiMcpDeclarations {
        let declarations = NomiMcpDeclarations::parse(source).expect("file must parse");
        assert!(
            declarations.rejected.is_empty(),
            "unexpected rejections: {:?}",
            declarations.rejected
        );
        declarations
    }

    fn only(source: &str) -> ResolvedNomiMcpServer {
        let mut declarations = accepted(source);
        assert_eq!(declarations.servers.len(), 1);
        declarations.servers.remove(0)
    }

    /// The single entry's rejection reason (the entry must not be accepted).
    fn rejection(key: &str, body: &str) -> String {
        let source = source_for(key, body);
        let declarations = NomiMcpDeclarations::parse(&source).expect("file must parse");
        assert_eq!(
            declarations.rejected.len(),
            1,
            "entry must be rejected: {source}"
        );
        assert!(declarations.servers.is_empty(), "entry must not be accepted");
        declarations.rejected[0].reason.clone()
    }

    #[test]
    fn default_declares_nothing() {
        let empty = NomiMcpDeclarations::default();
        assert!(empty.is_empty());
        assert_eq!(empty.enabled_servers().count(), 0);
    }

    #[test]
    fn parses_the_three_transport_shapes() {
        let declarations = accepted(
            r#"{
              "mcpServers": {
                "filesystem": { "command": "npx", "args": ["-y", "srv", "/tmp"], "env": { "TOKEN": "secret:T" } },
                "linear": { "url": "https://mcp.linear.app/mcp", "headers": { "X-Tenant": "acme" } },
                "legacy": { "transport": "sse", "url": "https://mcp.example.com/sse" }
              }
            }"#,
        );

        let by_name = |name: &str| {
            declarations
                .servers
                .iter()
                .find(|server| server.name == name)
                .expect("server must be present")
        };

        assert!(matches!(
            by_name("filesystem").transport,
            McpTransport::Stdio { .. }
        ));
        assert!(matches!(
            by_name("linear").transport,
            McpTransport::Http { .. }
        ));
        assert!(matches!(
            by_name("legacy").transport,
            McpTransport::Sse { .. }
        ));
        assert_eq!(by_name("filesystem").transport_kind(), "stdio");
        assert_eq!(by_name("linear").transport_kind(), "http");
        assert_eq!(by_name("legacy").transport_kind(), "sse");

        // `secret:` references survive verbatim — resolution happens at spawn.
        let McpTransport::Stdio { args, env, .. } = &by_name("filesystem").transport else {
            unreachable!("checked above")
        };
        assert_eq!(args.len(), 3);
        assert_eq!(env.get("TOKEN").map(String::as_str), Some("secret:T"));
        assert!(declarations.servers.iter().all(|server| {
            server.enabled
                && server.request_timeout_secs.is_none()
                && server.startup_timeout_secs.is_none()
                && server.cwd.is_none()
                && server.bearer_token_env_var.is_none()
                && server.enabled_tools.is_none()
                && server.disabled_tools.is_none()
        }));

        // Entries are ordered by key, so read views are stable.
        let names: Vec<&str> = declarations.servers.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["filesystem", "legacy", "linear"]);
    }

    #[test]
    fn absent_transport_with_url_is_http() {
        assert_eq!(
            only(r#"{ "mcpServers": { "a": { "url": "https://x/mcp" } } }"#).transport_kind(),
            "http"
        );
    }

    #[test]
    fn explicit_non_sse_transport_is_rejected_with_a_hint() {
        let reason = rejection("a", r#"{ "url": "https://x/mcp", "transport": "http" }"#);
        assert!(reason.contains("`http`"), "{reason}");
        assert!(reason.contains("omit `transport`"), "{reason}");
    }

    #[test]
    fn transport_on_a_stdio_entry_is_rejected() {
        let reason = rejection("a", r#"{ "command": "npx", "transport": "sse" }"#);
        assert!(reason.contains("always stdio"), "{reason}");
    }

    #[test]
    fn command_and_url_together_are_rejected() {
        let reason = rejection("a", r#"{ "command": "npx", "url": "https://x/mcp" }"#);
        assert!(reason.contains("not both"), "{reason}");
    }

    #[test]
    fn neither_command_nor_url_is_rejected() {
        let reason = rejection("a", r#"{ "args": ["x"] }"#);
        assert!(
            reason.contains("`command`") && reason.contains("`url`"),
            "{reason}"
        );
    }

    #[test]
    fn blank_command_is_rejected_as_missing() {
        let reason = rejection("a", r#"{ "command": "   " }"#);
        assert!(reason.contains("`command`"), "{reason}");
    }

    #[test]
    fn misplaced_transport_specific_fields_are_rejected() {
        let headers_on_stdio = rejection("a", r#"{ "command": "npx", "headers": { "X": "1" } }"#);
        assert!(headers_on_stdio.contains("`headers`"), "{headers_on_stdio}");

        let args_on_remote = rejection("a", r#"{ "url": "https://x/mcp", "args": ["x"] }"#);
        assert!(args_on_remote.contains("`args`"), "{args_on_remote}");

        let env_on_remote = rejection("a", r#"{ "url": "https://x/mcp", "env": { "K": "v" } }"#);
        assert!(env_on_remote.contains("`env`"), "{env_on_remote}");

        // An empty object is not a misplaced value: it declares nothing.
        assert_eq!(
            only(r#"{ "mcpServers": { "a": { "command": "npx", "headers": {} } } }"#)
                .transport_kind(),
            "stdio"
        );
    }

    #[test]
    fn every_reference_optional_field_is_accepted() {
        // The whole documented field set of the reference implementation has to
        // load unchanged — this is the "copy your `mcp.json` over" contract.
        let stdio = only(
            r#"{ "mcpServers": { "a": {
                 "command": "npx",
                 "args": ["-y", "srv"],
                 "env": { "TOKEN": "secret:T" },
                 "cwd": "/srv/data",
                 "startupTimeoutMs": 5000,
                 "toolTimeoutMs": 2500,
                 "enabledTools": ["read", "mcp__a__search"],
                 "disabledTools": ["write"],
                 "enabled": true
               } } }"#,
        );
        assert_eq!(stdio.cwd.as_deref(), Some("/srv/data"));
        assert_eq!(stdio.startup_timeout_secs, Some(5));
        assert_eq!(stdio.request_timeout_secs, Some(3));
        assert_eq!(
            stdio.enabled_tools,
            Some(vec!["read".to_owned(), "mcp__a__search".to_owned()])
        );
        assert_eq!(stdio.disabled_tools, Some(vec!["write".to_owned()]));
        assert!(stdio.bearer_token_env_var.is_none());

        let remote = only(
            r#"{ "mcpServers": { "a": {
                 "url": "https://x/mcp",
                 "headers": { "X-Tenant": "acme" },
                 "bearerTokenEnvVar": "GITHUB_TOKEN"
               } } }"#,
        );
        assert_eq!(remote.bearer_token_env_var.as_deref(), Some("GITHUB_TOKEN"));
        assert!(remote.cwd.is_none());
    }

    #[test]
    fn cwd_and_bearer_token_env_var_are_transport_scoped() {
        let cwd_on_remote = rejection("a", r#"{ "url": "https://x/mcp", "cwd": "/tmp" }"#);
        assert!(
            cwd_on_remote.contains("`cwd`") && cwd_on_remote.contains("stdio"),
            "{cwd_on_remote}"
        );

        let bearer_on_stdio = rejection("a", r#"{ "command": "npx", "bearerTokenEnvVar": "T" }"#);
        assert!(bearer_on_stdio.contains("`bearerTokenEnvVar`"), "{bearer_on_stdio}");

        // A blank value declares nothing, so it is not a misplaced field.
        assert_eq!(
            only(r#"{ "mcpServers": { "a": { "url": "https://x/mcp", "cwd": "   " } } }"#)
                .transport_kind(),
            "http"
        );
    }

    #[test]
    fn startup_timeout_maps_to_whole_seconds_and_is_bounded() {
        for (ms, expected) in [(1_000_u64, 1_u64), (500, 1), (30_000, 30), (600_000, 600)] {
            let body = format!(r#"{{ "command": "x", "startupTimeoutMs": {ms} }}"#);
            assert_eq!(
                only(&source_for("a", &body)).startup_timeout_secs,
                Some(expected),
                "ms={ms}"
            );
        }

        for ms in [0_u64, 600_001] {
            let body = format!(r#"{{ "command": "x", "startupTimeoutMs": {ms} }}"#);
            let reason = rejection("a", &body);
            assert!(reason.contains("startupTimeoutMs"), "ms={ms}: {reason}");
        }

        assert_eq!(
            only(r#"{ "mcpServers": { "a": { "command": "x" } } }"#).startup_timeout_secs,
            None
        );
    }

    #[test]
    fn tool_filters_are_normalised_and_an_empty_list_is_no_filter() {
        let entry = only(
            r#"{ "mcpServers": { "a": {
                 "command": "x",
                 "enabledTools": [" read ", "read", "write"],
                 "disabledTools": []
               } } }"#,
        );
        assert_eq!(
            entry.enabled_tools,
            Some(vec!["read".to_owned(), "write".to_owned()]),
            "entries are trimmed and duplicates collapse"
        );
        assert!(entry.disabled_tools.is_none(), "an empty list is not 'no tools'");

        for body in [
            r#"{ "command": "x", "enabledTools": [""] }"#,
            r#"{ "command": "x", "enabledTools": ["  "] }"#,
            r#"{ "command": "x", "disabledTools": [""] }"#,
        ] {
            let reason = rejection("a", body);
            assert!(reason.contains("empty entry"), "{body}: {reason}");
        }
    }

    #[test]
    fn cwd_is_resolved_against_the_declaration_file_only_when_relative() {
        let base = std::env::temp_dir().join("agent-store");
        let absolute = std::env::temp_dir().join("elsewhere");
        let source = format!(
            r#"{{ "mcpServers": {{
                 "rel":  {{ "command": "x", "cwd": "servers/rel" }},
                 "abs":  {{ "command": "y", "cwd": {absolute:?} }},
                 "none": {{ "command": "z" }}
               }} }}"#,
            absolute = absolute.to_string_lossy(),
        );
        let mut declarations = accepted(&source);
        declarations.resolve_cwd_relative_to(&base);

        let cwd = |name: &str| {
            declarations
                .servers
                .iter()
                .find(|server| server.name == name)
                .expect("server must be present")
                .cwd
                .clone()
        };
        assert_eq!(
            cwd("rel").as_deref(),
            Some(base.join("servers/rel").to_string_lossy().as_ref())
        );
        assert_eq!(
            cwd("abs").as_deref(),
            Some(absolute.to_string_lossy().as_ref()),
            "an absolute path is never rewritten"
        );
        assert!(cwd("none").is_none());
    }

    #[test]
    fn an_unknown_field_is_rejected_and_named_with_the_accepted_set() {
        let reason = rejection("a", r#"{ "command": "npx", "mystery": 1 }"#);
        assert!(reason.contains("unknown field"), "{reason}");
        assert!(reason.contains("mystery"), "{reason}");
        // serde names the key it did not expect, never the keys that exist.
        assert!(reason.contains("accepted fields are"), "{reason}");
        assert!(reason.contains("`enabledTools`"), "{reason}");
    }

    #[test]
    fn key_length_is_bounded_by_the_tool_name_slug() {
        let longest = "a".repeat(MAX_DECLARATION_KEY_LEN);
        assert_eq!(
            only(&source_for(&longest, r#"{ "command": "x" }"#)).name,
            longest
        );

        let too_long = "a".repeat(MAX_DECLARATION_KEY_LEN + 1);
        let reason = rejection(&too_long, r#"{ "command": "x" }"#);
        assert!(reason.contains("at most 40"), "{reason}");
    }

    #[test]
    fn key_charset_is_enforced() {
        for key in ["a.b", "a b", "_a", "a_", "-a", "a-", "a/b", "中文", ""] {
            let reason = rejection(key, r#"{ "command": "x" }"#);
            assert!(!reason.is_empty(), "{key:?} must produce a reason");
        }
        for key in ["a", "A1", "a-b_c", "agent-earth", "mcp_server"] {
            assert_eq!(only(&source_for(key, r#"{ "command": "x" }"#)).name, key);
        }
    }

    #[test]
    fn tool_timeout_maps_to_whole_seconds() {
        for (ms, expected) in [
            (1_000_u64, 1_u64),
            (500, 1),
            (1_001, 2),
            (2_500, 3),
            (600_000, 600),
        ] {
            let body = format!(r#"{{ "command": "x", "toolTimeoutMs": {ms} }}"#);
            assert_eq!(
                only(&source_for("a", &body)).request_timeout_secs,
                Some(expected),
                "ms={ms}"
            );
        }

        for ms in [0_u64, 600_001] {
            let body = format!(r#"{{ "command": "x", "toolTimeoutMs": {ms} }}"#);
            let reason = rejection("a", &body);
            assert!(reason.contains("toolTimeoutMs"), "ms={ms}: {reason}");
        }

        // An absent value stays absent so the engine's own default applies.
        assert_eq!(
            only(r#"{ "mcpServers": { "a": { "command": "x" } } }"#).request_timeout_secs,
            None
        );
    }

    #[test]
    fn disabled_entries_stay_visible_but_are_not_enabled() {
        let declarations = accepted(
            r#"{ "mcpServers": {
                 "on":  { "command": "x" },
                 "off": { "command": "y", "enabled": false }
               } }"#,
        );
        assert_eq!(declarations.servers.len(), 2);
        let enabled: Vec<&str> = declarations
            .enabled_servers()
            .map(|server| server.name.as_str())
            .collect();
        assert_eq!(enabled, ["on"]);
        assert!(!declarations.is_empty());
    }

    #[test]
    fn file_level_problems_are_errors_not_empty_successes() {
        for source in [
            "",
            "{",
            "[]",
            r#"{ "mcpServers": [] }"#,
            r#"{ "mcpservers": {} }"#,
            r#"{ "mcpServers": {}, "extra": 1 }"#,
        ] {
            assert!(
                NomiMcpDeclarations::parse(source).is_err(),
                "{source:?} must be a file-level error"
            );
        }
        // An empty object is a valid, declaration-free file.
        for source in [r#"{ "mcpServers": {} }"#, "{}"] {
            assert!(NomiMcpDeclarations::parse(source).unwrap().is_empty());
        }

        // A non-object *entry* is an entry-level rejection, not a file error:
        // one malformed server must not take its siblings down with it.
        let declarations = NomiMcpDeclarations::parse(r#"{ "mcpServers": { "a": 5 } }"#).unwrap();
        assert!(declarations.servers.is_empty());
        assert_eq!(declarations.rejected.len(), 1);
        assert_eq!(declarations.rejected[0].name, "a");
    }

    #[test]
    fn one_bad_entry_does_not_invalidate_its_siblings() {
        let declarations = NomiMcpDeclarations::parse(
            r#"{ "mcpServers": {
                 "good":  { "command": "npx" },
                 "bad":   { "command": "npx", "headers": { "X": "1" } },
                 "also":  { "url": "https://x/mcp" }
               } }"#,
        )
        .unwrap();

        let names: Vec<&str> = declarations.servers.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["also", "good"]);
        assert_eq!(declarations.rejected.len(), 1);
        assert_eq!(declarations.rejected[0].name, "bad");
        assert!(declarations.rejected[0].reason.contains("`headers`"));
    }

    #[test]
    fn credential_values_may_contain_newlines() {
        // A PEM key in an env value is legitimate; only the *placement* of the
        // object is restricted, never the characters inside a value.
        let entry = only(
            r#"{ "mcpServers": {
                 "a": { "command": "x", "env": { "KEY": "-----BEGIN-----\nMIIB\n-----END-----" } }
               } }"#,
        );
        let McpTransport::Stdio { env, .. } = &entry.transport else {
            unreachable!("stdio")
        };
        assert!(env["KEY"].contains('\n'));
    }

    #[test]
    fn env_and_args_default_to_empty_rather_than_absent() {
        let entry = only(r#"{ "mcpServers": { "a": { "command": "x" } } }"#);
        let McpTransport::Stdio { args, env, .. } = &entry.transport else {
            unreachable!("stdio")
        };
        assert!(args.is_empty());
        assert!(env.is_empty());
    }
}

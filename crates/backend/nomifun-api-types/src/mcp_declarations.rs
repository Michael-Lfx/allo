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
//! Three deliberate differences from the reference implementation are enforced
//! here rather than only documented:
//!
//! - fields this host cannot honour (`cwd`, `bearerTokenEnvVar`,
//!   `startupTimeoutMs`, `enabledTools`, `disabledTools`) and any unknown field
//!   **reject their own entry** instead of being silently ignored — ignoring
//!   `enabledTools` would leave tools the user believed excluded callable;
//! - a field that does not apply to the chosen transport (`headers` on a stdio
//!   entry, `args`/`env` on a remote entry) is rejected for the same reason;
//! - the project-level `<workspace>/.agent-store/mcp.json` layer is **not** read
//!   (the path is reserved, not implemented).
//!
//! Failure granularity is deliberate: a malformed file is a file-level error
//! (the caller then declares nothing — never a widened surface), while a bad
//! entry rejects only itself so one typo cannot disable every server.

use std::collections::{BTreeMap, HashMap};

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
pub const MAX_TOOL_TIMEOUT_MS: u64 = 600_000;

/// Fields the reference implementation supports and this host deliberately does
/// not, paired with what to use instead. Named so a rejection can say *why*
/// instead of only *that*.
const UNSUPPORTED_FIELDS: &[(&str, &str)] = &[
    (
        "cwd",
        "a stdio server always inherits the session working directory, so a per-server `cwd` is not supported",
    ),
    (
        "bearerTokenEnvVar",
        "use `headers` with a `secret:<NAME>` reference plus `[credentials]` in `config.toml` instead",
    ),
    (
        "startupTimeoutMs",
        "the MCP connect handshake timeout is fixed (30s) and not configurable",
    ),
    (
        "enabledTools",
        "narrow tools through the host `[tools]` policy (`disabled = [\"mcp__<key>__*\"]`) instead",
    ),
    (
        "disabledTools",
        "narrow tools through the host `[tools]` policy (`disabled = [\"mcp__<key>__*\"]`) instead",
    ),
];

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
}

fn resolve_entry(name: &str, value: serde_json::Value) -> Result<ResolvedNomiMcpServer, String> {
    validate_key(name)?;

    let raw: RawServer = serde_json::from_value(value).map_err(|error| explain(&error))?;
    if let Some(timeout) = raw.tool_timeout_ms
        && !(MIN_TOOL_TIMEOUT_MS..=MAX_TOOL_TIMEOUT_MS).contains(&timeout)
    {
        return Err(format!(
            "`toolTimeoutMs` must be between {MIN_TOOL_TIMEOUT_MS} and {MAX_TOOL_TIMEOUT_MS} \
             milliseconds, got {timeout}"
        ));
    }
    let request_timeout_secs = raw.tool_timeout_ms.map(|ms| ms.div_ceil(1_000));

    let RawServer {
        command,
        args,
        env,
        url,
        headers,
        transport,
        enabled,
        tool_timeout_ms: _,
    } = raw;
    let command = trim_non_empty(command);
    let url = trim_non_empty(url);

    let transport = match (command, url, transport.as_deref()) {
        (Some(command), None, None) => {
            reject_remote_only_fields(&headers, "a stdio server (`command`)")?;
            McpTransport::Stdio {
                command,
                args: args.unwrap_or_default(),
                env: to_map(env),
            }
        }
        (None, Some(url), None) => {
            reject_stdio_only_fields(&args, &env, "a remote server (`url`)")?;
            McpTransport::Http {
                url,
                headers: to_map(headers),
            }
        }
        (None, Some(url), Some("sse")) => {
            reject_stdio_only_fields(&args, &env, "a remote server (`url`)")?;
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
    })
}

fn reject_remote_only_fields(
    headers: &Option<BTreeMap<String, String>>,
    what: &str,
) -> Result<(), String> {
    if headers.as_ref().is_some_and(|values| !values.is_empty()) {
        return Err(format!("`headers` only applies to a remote server, but this entry is {what}"));
    }
    Ok(())
}

fn reject_stdio_only_fields(
    args: &Option<Vec<String>>,
    env: &Option<BTreeMap<String, String>>,
    what: &str,
) -> Result<(), String> {
    if args.as_ref().is_some_and(|values| !values.is_empty()) {
        return Err(format!("`args` only applies to a stdio server, but this entry is {what}"));
    }
    if env.as_ref().is_some_and(|values| !values.is_empty()) {
        return Err(format!("`env` only applies to a stdio server, but this entry is {what}"));
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

/// Turn a strict-entry deserialization error into something actionable: the
/// reference implementation's optional fields are the likeliest surprise, so
/// they get a named explanation instead of serde's field list.
fn explain(error: &serde_json::Error) -> String {
    let message = error.to_string();
    if message.contains("unknown field") {
        for (field, explanation) in UNSUPPORTED_FIELDS {
            if message.contains(&format!("`{field}`")) {
                return format!(
                    "`{field}` is not supported: {explanation} — remove it to load this server"
                );
            }
        }
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
        assert!(
            declarations
                .servers
                .iter()
                .all(|server| server.enabled && server.request_timeout_secs.is_none())
        );

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
    fn unsupported_reference_fields_are_rejected_by_name() {
        let cases = [
            ("cwd", r#"{ "command": "npx", "cwd": "/tmp" }"#),
            (
                "bearerTokenEnvVar",
                r#"{ "url": "https://x/mcp", "bearerTokenEnvVar": "T" }"#,
            ),
            (
                "startupTimeoutMs",
                r#"{ "command": "npx", "startupTimeoutMs": 5000 }"#,
            ),
            (
                "enabledTools",
                r#"{ "command": "npx", "enabledTools": ["read"] }"#,
            ),
            (
                "disabledTools",
                r#"{ "command": "npx", "disabledTools": ["write"] }"#,
            ),
        ];
        for (field, body) in cases {
            let reason = rejection("a", body);
            assert!(reason.contains(&format!("`{field}`")), "{field}: {reason}");
            assert!(reason.contains("not supported"), "{field}: {reason}");
            // The reason has to be actionable, not just a refusal.
            assert!(reason.contains("remove it"), "{field}: {reason}");
        }
    }

    #[test]
    fn an_unknown_field_is_rejected_and_named() {
        let reason = rejection("a", r#"{ "command": "npx", "mystery": 1 }"#);
        assert!(reason.contains("unknown field"), "{reason}");
        assert!(reason.contains("mystery"), "{reason}");
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
                 "bad":   { "command": "npx", "cwd": "/tmp" },
                 "also":  { "url": "https://x/mcp" }
               } }"#,
        )
        .unwrap();

        let names: Vec<&str> = declarations.servers.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["also", "good"]);
        assert_eq!(declarations.rejected.len(), 1);
        assert_eq!(declarations.rejected[0].name, "bad");
        assert!(declarations.rejected[0].reason.contains("`cwd`"));
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

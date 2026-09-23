//! `secret:NAME` references in process-bound env maps (`17` §6 / `21` D5=C).
//!
//! A credential must never be persisted: not in a marketplace snapshot, not in
//! the MCP server DB row, not in logs. The value lives in the host's config
//! (`~/.agent-store/config.toml [credentials]`, with the process environment as
//! a fallback) and reaches a child process **only** at spawn time, in memory.
//!
//! The carrier between those two points is a **reference**, in one of two forms:
//! an env value that is exactly `secret:NAME`, or the embedded `${secret:NAME}`
//! inside a longer string (a header carrying `Bearer ${secret:TOKEN}`, or a URL
//! with the token in its query). This module owns the reference syntax and the
//! resolution, so the importer (which writes the reference) and every MCP spawn
//! path (which resolves it) agree byte-for-byte.
//!
//! Wiring: the host installs the credentials once at startup
//! ([`set_credentials`]); spawn paths call [`resolve_env`] on the env map they
//! are about to hand a child process. An unresolvable reference is **omitted**
//! rather than guessed — fail-closed, the same posture as the browser engine's
//! `SecretStore::resolve`.

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

/// Prefix that marks an env value as a credential reference.
///
/// Deliberately the same literal as the browser engine's `secret:NAME`
/// (`nomi-browser` `tool.rs`, 裁决⑦) so the codebase has one reference syntax,
/// not two.
pub const SECRET_PREFIX: &str = "secret:";

/// The host's installed credentials, `NAME -> value`.
///
/// Process-wide because the resolution happens in several crates (the MCP
/// connection test and the nomi / ACP assembly paths) that all run inside the
/// host process but do not share a config handle. `RwLock` (not `OnceLock`) so a
/// host that reloads its config can replace the map; the write happens once at
/// startup in practice.
static CREDENTIALS: OnceLock<RwLock<HashMap<String, String>>> = OnceLock::new();

fn credentials_cell() -> &'static RwLock<HashMap<String, String>> {
    CREDENTIALS.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Parse a `secret:NAME` reference.
///
/// Returns `Some(name)` only when `value` is exactly `secret:` followed by a
/// non-empty name. A bare `"secret:"` (no name) is **not** a reference — it is
/// treated as literal text, so it can never resolve to an unnamed credential.
/// Pure (no I/O), mirroring `nomi-browser`'s `parse_secret_ref`.
pub fn parse_secret_ref(value: &str) -> Option<&str> {
    let name = value.strip_prefix(SECRET_PREFIX)?;
    if name.is_empty() { None } else { Some(name) }
}

/// Install (or replace) the host's credentials.
///
/// Called once at host startup from `~/.agent-store/config.toml [credentials]`.
/// Replacing is allowed so a config reload is not silently ignored.
pub fn set_credentials(credentials: HashMap<String, String>) {
    let mut guard = credentials_cell()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = credentials;
}

/// The currently installed credentials, cloned.
pub fn credentials() -> HashMap<String, String> {
    credentials_cell()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Look up a credential **by name** — the non-reference half of
/// [`resolve_value_with`].
///
/// Needed where a declaration names the variable instead of carrying a
/// `secret:NAME` reference (`mcp.json`'s `bearerTokenEnvVar`), so that the
/// precedence lives here once rather than being restated at each call site.
pub fn lookup(name: &str) -> Option<String> {
    lookup_with(name, &credentials())
}

/// [`lookup`] against an explicit credential map (pure; for tests and for
/// callers that already hold a snapshot).
pub fn lookup_with(name: &str, credentials: &HashMap<String, String>) -> Option<String> {
    credentials
        .get(name)
        .cloned()
        .or_else(|| std::env::var(name).ok())
}

/// The outcome of resolving one templated string (see [`resolve_template_with`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemplateResolution {
    /// `Some(resolved)` when every reference resolved; `None` when at least one
    /// did not. Callers must treat `None` as "do not use this string at all" —
    /// the partial result is deliberately discarded rather than half-filled.
    pub value: Option<String>,
    /// Credential **names** that could not be resolved, sorted and de-duplicated.
    /// A name is not a secret, so this is safe to log.
    pub missing: Vec<String>,
}

/// Delimiter of the embedded reference form: `${secret:NAME}`.
pub const TEMPLATE_PREFIX: &str = "${secret:";

/// Resolve a string that may carry credential references, in either form.
///
/// - exactly `secret:NAME` — the original whole-value form, unchanged;
/// - `${secret:NAME}`, possibly several and possibly embedded in surrounding
///   text (`Authorization: Bearer ${secret:TOKEN}`, `…/mcp?token=${secret:K}`),
///   replaced by their values;
/// - anything else is returned unchanged, including the shapes that only *look*
///   like a reference: `secret:` with no name, `${secret:}` with no name, and an
///   unterminated `${secret:NAME`. Guessing at those would invent a credential
///   name, so they stay literal (the same posture as [`parse_secret_ref`]).
///
/// Fail-closed at the string level: if any one reference is unresolvable the
/// whole value comes back `None`, so a caller can never send a half-substituted
/// Authorization header.
pub fn resolve_template_with(value: &str, credentials: &HashMap<String, String>) -> TemplateResolution {
    if let Some(name) = parse_secret_ref(value) {
        return match lookup_with(name, credentials) {
            Some(actual) => TemplateResolution { value: Some(actual), missing: Vec::new() },
            None => TemplateResolution { value: None, missing: vec![name.to_owned()] },
        };
    }
    if !value.contains(TEMPLATE_PREFIX) {
        return TemplateResolution { value: Some(value.to_owned()), missing: Vec::new() };
    }

    let mut out = String::with_capacity(value.len());
    let mut missing = Vec::new();
    let mut rest = value;
    while let Some(start) = rest.find(TEMPLATE_PREFIX) {
        out.push_str(&rest[..start]);
        let after = &rest[start + TEMPLATE_PREFIX.len()..];
        let Some(end) = after.find('}') else {
            // Unterminated: the rest is literal text.
            out.push_str(&rest[start..]);
            return TemplateResolution { value: Some(out), missing };
        };
        let name = &after[..end];
        if name.is_empty() {
            // `${secret:}` names nothing; keep the literal rather than inventing one.
            out.push_str(&rest[start..start + TEMPLATE_PREFIX.len() + end + 1]);
        } else {
            match lookup_with(name, credentials) {
                Some(actual) => out.push_str(&actual),
                None => missing.push(name.to_owned()),
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);

    missing.sort();
    missing.dedup();
    if missing.is_empty() {
        TemplateResolution { value: Some(out), missing }
    } else {
        TemplateResolution { value: None, missing }
    }
}

/// The installation owner's principal id.
///
/// Hand-edited, host-level `[credentials]` entries belong to the machine's
/// operator. Once the host declares who that is, a bare `NAME` entry stops being
/// visible to every other principal — which is what makes a shared host safe
/// (34 §7). A host that declares no owner keeps the single-user behaviour it has
/// always had, so nothing existing changes meaning on upgrade.
static OPERATOR: OnceLock<RwLock<Option<String>>> = OnceLock::new();

fn operator_cell() -> &'static RwLock<Option<String>> {
    OPERATOR.get_or_init(|| RwLock::new(None))
}

/// Declare which principal the host-level `[credentials]` entries belong to.
pub fn set_operator_principal(principal: Option<String>) {
    let mut guard = operator_cell()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = principal;
}

/// The declared installation owner, if any.
pub fn operator_principal() -> Option<String> {
    operator_cell()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Are the bare (host-level) entries visible to this caller?
///
/// - no owner declared → yes: the legacy single-user host;
/// - an unnamed caller (`None`) → yes: a host-internal path acts for the operator;
/// - otherwise → only the operator itself.
fn host_entries_visible_to(principal: Option<&str>, operator: Option<&str>) -> bool {
    match operator {
        None => true,
        Some(operator) => match principal {
            None => true,
            Some(principal) => principal == operator,
        },
    }
}

/// Look up a credential **for one principal** (`34` §7).
///
/// `<principal>:NAME` wins; a bare `NAME` is the host-level fallback and is
/// visible only per [`host_entries_visible_to`]. A caller's own entry never falls
/// back to another principal's — cross-principal substitution is the one thing
/// this must not do.
pub fn lookup_for(principal: Option<&str>, name: &str) -> Option<String> {
    let operator = operator_principal();
    lookup_for_with(principal, name, &credentials(), operator.as_deref())
}

/// [`lookup_for`] against an explicit credential map and owner (pure; for tests).
pub fn lookup_for_with(
    principal: Option<&str>,
    name: &str,
    credentials: &HashMap<String, String>,
    operator: Option<&str>,
) -> Option<String> {
    if let Some(principal) = principal
        && let Some(value) = credentials.get(&scoped_key(principal, name))
    {
        return Some(value.clone());
    }
    if !host_entries_visible_to(principal, operator) {
        return None;
    }
    // The same ladder as before: an explicit config entry, then the ambient
    // environment (which is also the operator's).
    credentials
        .get(name)
        .cloned()
        .or_else(|| std::env::var(name).ok())
}

/// The per-principal form of a credential key: `<principal>:NAME`.
pub fn scoped_key(principal: &str, name: &str) -> String {
    format!("{principal}:{name}")
}

/// The **stored** key a write for `principal` should use (`34` §7).
///
/// A caller with an identity gets its own namespaced entry; a host-internal
/// caller (`None`) writes the host-level one — the form the installation owner's
/// hand-edited config has always used.
pub fn credential_key_for(principal: Option<&str>, name: &str) -> String {
    match principal {
        Some(principal) => scoped_key(principal, name),
        None => name.to_owned(),
    }
}

/// Parse a `<principal>:NAME` key back into its parts.
///
/// Returns `None` for a bare `NAME` — the host-level, pre-`34` form.
pub fn parse_scoped_key(key: &str) -> Option<(&str, &str)> {
    let (principal, name) = key.split_once(':')?;
    if principal.is_empty() || name.is_empty() {
        return None;
    }
    Some((principal, name))
}

/// [`resolve_template_with`] against the installed credentials.
pub fn resolve_template(value: &str) -> TemplateResolution {
    resolve_template_with(value, &credentials())
}

/// What a request string may refer to: secrets, and the connector's own plain
/// values.
pub struct TransportScope<'a> {
    pub credentials: &'a HashMap<String, String>,
    /// A connector's non-secret settings (`HOST`, `PORT`, `ENV`), which the
    /// marketplace writes as `${NAME}` placeholders too. They travel in the
    /// connector's own transport config, never in the credential store.
    pub values: &'a HashMap<String, String>,
    /// Whose credentials this request may use (`34` §7). `None` is a host-internal
    /// caller, which acts for the installation owner.
    pub principal: Option<&'a str>,
    /// The declared installation owner, for the host-level fallback rule.
    pub operator: Option<&'a str>,
}

impl<'a> TransportScope<'a> {
    /// A scope with no principal: the pre-`34` behaviour, for callers that have no
    /// identity to offer yet.
    pub fn new(credentials: &'a HashMap<String, String>, values: &'a HashMap<String, String>) -> Self {
        Self { credentials, values, principal: None, operator: None }
    }

    /// A scope for one principal, with the host's declared owner.
    pub fn for_principal(
        credentials: &'a HashMap<String, String>,
        values: &'a HashMap<String, String>,
        principal: Option<&'a str>,
        operator: Option<&'a str>,
    ) -> Self {
        Self { credentials, values, principal, operator }
    }
}

/// Resolve one request string against both namespaces (`34` §5.1).
///
/// - `secret:NAME` / `${secret:NAME}` → the credential store;
/// - `${NAME}` → the connector's plain values;
/// - anything else is returned unchanged.
///
/// Fail-closed for both: one unresolvable reference discards the whole string, so
/// a half-substituted URL or `Authorization` header can never be sent. A missing
/// plain value counts as a missing reference for the same reason — a connector
/// whose URL still says `${HOST}` is not usable either.
pub fn resolve_request_string(value: &str, scope: &TransportScope<'_>) -> TemplateResolution {
    if let Some(name) = parse_secret_ref(value) {
        return match lookup_for_with(scope.principal, name, scope.credentials, scope.operator) {
            Some(actual) => TemplateResolution { value: Some(actual), missing: Vec::new() },
            None => TemplateResolution { value: None, missing: vec![name.to_owned()] },
        };
    }
    if !value.contains("${") {
        return TemplateResolution { value: Some(value.to_owned()), missing: Vec::new() };
    }

    let mut out = String::with_capacity(value.len());
    let mut missing = Vec::new();
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            // Unterminated: the rest is literal text.
            out.push_str(&rest[start..]);
            return TemplateResolution { value: Some(out), missing };
        };
        let raw_name = &after[..end];
        let (secret, name) = match raw_name.strip_prefix("secret:") {
            Some(name) => (true, name),
            None => (false, raw_name),
        };
        if name.is_empty() {
            out.push_str(&rest[start..start + 2 + end + 1]);
        } else {
            let resolved = if secret {
                lookup_for_with(scope.principal, name, scope.credentials, scope.operator)
            } else {
                scope.values.get(name).cloned()
            };
            match resolved {
                Some(actual) => out.push_str(&actual),
                None => missing.push(name.to_owned()),
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);

    missing.sort();
    missing.dedup();
    if missing.is_empty() {
        TemplateResolution { value: Some(out), missing }
    } else {
        TemplateResolution { value: None, missing }
    }
}

/// Resolve every value in a map, treating each as a template ([`resolve_template`]).
///
/// Keys whose value cannot be resolved are omitted, and the **key names** are
/// reported — never the values. Used for header maps, where a key that cannot be
/// filled means the request must not be sent.
pub fn resolve_map(values: &HashMap<String, String>) -> ResolvedEnv {
    resolve_map_with(values, &credentials())
}

/// [`resolve_map`] against an explicit credential map (pure; for tests).
pub fn resolve_map_with(
    values: &HashMap<String, String>,
    credentials: &HashMap<String, String>,
) -> ResolvedEnv {
    let mut resolved = ResolvedEnv::default();
    for (key, value) in values {
        match resolve_template_with(value, credentials).value {
            Some(actual) => {
                resolved.env.insert(key.clone(), actual);
            }
            None => resolved.missing.push(key.clone()),
        }
    }
    resolved.missing.sort();
    resolved
}

/// Resolve one env value against an explicit credential map plus the process
/// environment — the pure, testable half of [`resolve_env`].
///
/// - an ordinary value is returned unchanged (non-secret env keeps working);
/// - `secret:NAME` resolves from `credentials` first, then the process env;
/// - an unresolvable reference returns `None` (the caller omits it).
///
/// The credential lookup order is deliberate: an explicit config entry wins
/// over an ambient environment variable of the same name.
///
/// Whole-value references only: use [`resolve_template_with`] where a reference
/// may be embedded in a larger string (headers, URLs). This function stays as
/// the strict, single-slot form because `mcp.json`'s `bearerTokenEnvVar` and the
/// edit paths rely on "the whole value is the reference".
pub fn resolve_value_with(
    value: &str,
    credentials: &HashMap<String, String>,
) -> Option<String> {
    match parse_secret_ref(value) {
        None => Some(value.to_owned()),
        Some(name) => lookup_with(name, credentials),
    }
}

/// The outcome of resolving an env map.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedEnv {
    /// Env entries to hand the child process, with every resolvable reference
    /// replaced by its value.
    pub env: HashMap<String, String>,
    /// **Key names** whose reference could not be resolved. A name is not a
    /// secret, so this is safe to log; the value never was available.
    pub missing: Vec<String>,
}

/// Resolve every `secret:NAME` reference in an env map against the installed
/// credentials and the process environment.
///
/// Both reference forms are honoured ([`resolve_template`]): the whole-value
/// `secret:NAME`, and `${secret:NAME}` embedded in a longer value. Non-reference
/// values pass through, so a hand-registered server (whose env was never
/// rewritten at import) behaves exactly as before. A reference with no matching
/// credential is dropped and reported in [`ResolvedEnv::missing`] — the child
/// simply does not get that variable, rather than getting the literal
/// `secret:NAME` string or a fabricated value.
pub fn resolve_env(env: &HashMap<String, String>) -> ResolvedEnv {
    resolve_env_with(env, &credentials())
}

/// [`resolve_env`] against an explicit credential map (pure; for tests).
pub fn resolve_env_with(
    env: &HashMap<String, String>,
    credentials: &HashMap<String, String>,
) -> ResolvedEnv {
    let mut resolved = ResolvedEnv::default();
    for (key, value) in env {
        match resolve_template_with(value, credentials).value {
            Some(actual) => {
                resolved.env.insert(key.clone(), actual);
            }
            None => resolved.missing.push(key.clone()),
        }
    }
    resolved.missing.sort();
    resolved
}

/// Resolve an env map for one principal (`34` §7).
///
/// Per-variable, like [`resolve_env`]: a stdio child is still useful with the
/// variables that *did* resolve, so an unresolvable reference omits just that
/// variable (and reports its name) rather than dropping the whole server.
pub fn resolve_env_for(principal: Option<&str>, env: &HashMap<String, String>) -> ResolvedEnv {
    let operator = operator_principal();
    resolve_env_for_with(principal, env, &credentials(), operator.as_deref())
}

/// [`resolve_env_for`] against explicit state (pure; for tests).
pub fn resolve_env_for_with(
    principal: Option<&str>,
    env: &HashMap<String, String>,
    credentials: &HashMap<String, String>,
    operator: Option<&str>,
) -> ResolvedEnv {
    // A stdio server has no plain-value layer of its own: a `${NAME}` in its env
    // is not a setting it could look up, so it counts as a missing reference
    // rather than reaching the child as literal text.
    let empty = HashMap::new();
    let scope = TransportScope::for_principal(credentials, &empty, principal, operator);
    let mut resolved = ResolvedEnv::default();
    for (key, value) in env {
        match resolve_request_string(value, &scope).value {
            Some(actual) => {
                resolved.env.insert(key.clone(), actual);
            }
            None => resolved.missing.push(key.clone()),
        }
    }
    resolved.missing.sort();
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

    fn creds(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn parse_accepts_named_reference_only() {
        assert_eq!(parse_secret_ref("secret:DEMO_TOKEN"), Some("DEMO_TOKEN"));
        assert_eq!(parse_secret_ref("secret:"), None, "a bare prefix is literal");
        assert_eq!(parse_secret_ref("Secret:X"), None, "prefix is case-sensitive");
        assert_eq!(parse_secret_ref("plain"), None);
        assert_eq!(parse_secret_ref(""), None);
    }

    #[test]
    fn ordinary_values_pass_through_untouched() {
        // Non-secret env (PATH, NODE_ENV, …) must keep working verbatim.
        let got = resolve_env_with(&creds(&[("NODE_ENV", "production")]), &HashMap::new());
        assert_eq!(got.env.get("NODE_ENV").map(String::as_str), Some("production"));
        assert!(got.missing.is_empty());
    }

    #[test]
    fn reference_resolves_from_credentials() {
        let env = HashMap::from([("DEMO_TOKEN".to_owned(), "secret:DEMO_TOKEN".to_owned())]);
        let got = resolve_env_with(&env, &creds(&[("DEMO_TOKEN", "s3cr3t")]));
        assert_eq!(got.env.get("DEMO_TOKEN").map(String::as_str), Some("s3cr3t"));
        assert!(got.missing.is_empty());
    }

    #[test]
    fn missing_reference_is_dropped_and_reported_not_faked() {
        // Fail-closed: no value, no `secret:NAME` literal, no empty string.
        let env = HashMap::from([("DEMO_TOKEN".to_owned(), "secret:DEMO_TOKEN".to_owned())]);
        let got = resolve_env_with(&env, &HashMap::new());
        assert!(!got.env.contains_key("DEMO_TOKEN"), "unresolvable reference must be omitted");
        assert_eq!(got.missing, vec!["DEMO_TOKEN".to_owned()], "the key name is reportable");
    }

    #[test]
    fn bare_prefix_stays_literal() {
        let env = HashMap::from([("ODD".to_owned(), "secret:".to_owned())]);
        let got = resolve_env_with(&env, &HashMap::new());
        assert_eq!(got.env.get("ODD").map(String::as_str), Some("secret:"));
        assert!(got.missing.is_empty());
    }

    #[test]
    fn unknown_reference_with_no_ambient_value_is_missing() {
        // A name that is in neither the credentials nor (plausibly) the process
        // env resolves to nothing. `resolve_value_with` is asserted directly so
        // the test never has to mutate the process environment.
        assert_eq!(
            resolve_value_with("secret:__NOMIFUN_DEFINITELY_UNSET__", &HashMap::new()),
            None
        );
        // Precedence: an explicit credential answers without consulting env.
        assert_eq!(
            resolve_value_with("secret:K", &creds(&[("K", "config")])),
            Some("config".to_owned())
        );
    }

    #[test]
    fn embedded_template_resolves_inside_a_longer_string() {
        // The market's shape: the prefix (`Bearer `) is part of the template text
        // and must survive verbatim — nothing may be inferred from the key name.
        let got = resolve_template_with("Bearer ${secret:TOKEN}", &creds(&[("TOKEN", "abc")]));
        assert_eq!(got.value.as_deref(), Some("Bearer abc"));
        assert!(got.missing.is_empty());

        // Several references in one value, and non-reference text untouched.
        let got = resolve_template_with(
            "${secret:SCHEME}://h/?token=${secret:TOKEN}&x=1",
            &creds(&[("SCHEME", "https"), ("TOKEN", "t")]),
        );
        assert_eq!(got.value.as_deref(), Some("https://h/?token=t&x=1"));
    }

    #[test]
    fn one_unresolvable_reference_discards_the_whole_string() {
        // Fail-closed at the string level: a half-substituted Authorization
        // header is worse than no header, because it looks configured.
        let got = resolve_template_with("Bearer ${secret:TOKEN}", &HashMap::new());
        assert_eq!(got.value, None);
        assert_eq!(got.missing, vec!["TOKEN".to_owned()]);
    }

    #[test]
    fn missing_names_are_sorted_and_deduplicated() {
        let got = resolve_template_with("${secret:B}${secret:A}${secret:B}", &HashMap::new());
        assert_eq!(got.value, None);
        assert_eq!(got.missing, vec!["A".to_owned(), "B".to_owned()]);
    }

    #[test]
    fn shapes_that_only_look_like_templates_stay_literal() {
        // Inventing a credential name from these would be guessing.
        for literal in ["${secret:}", "${secret:NAME", "${secretNAME}", "$secret:NAME"] {
            let got = resolve_template_with(literal, &creds(&[("NAME", "n")]));
            assert_eq!(got.value.as_deref(), Some(literal), "{literal} must stay literal");
            assert!(got.missing.is_empty(), "{literal} must not report a missing name");
        }
    }

    #[test]
    fn whole_value_reference_still_wins_over_template_scanning() {
        // `secret:NAME` is not a template; it keeps the original behaviour
        // (including resolving `secret:` with no name as literal text).
        let got = resolve_template_with("secret:NAME", &creds(&[("NAME", "n")]));
        assert_eq!(got.value.as_deref(), Some("n"));
        let got = resolve_template_with("secret:", &HashMap::new());
        assert_eq!(got.value.as_deref(), Some("secret:"));
    }

    #[test]
    fn env_and_header_maps_resolve_both_forms() {
        let env = HashMap::from([
            ("A".to_owned(), "secret:TOKEN".to_owned()),
            ("B".to_owned(), "prefix-${secret:TOKEN}".to_owned()),
            ("C".to_owned(), "plain".to_owned()),
            ("D".to_owned(), "${secret:UNSET}".to_owned()),
        ]);
        let got = resolve_env_with(&env, &creds(&[("TOKEN", "t")]));
        assert_eq!(got.env.get("A").map(String::as_str), Some("t"));
        assert_eq!(got.env.get("B").map(String::as_str), Some("prefix-t"));
        assert_eq!(got.env.get("C").map(String::as_str), Some("plain"));
        assert!(!got.env.contains_key("D"));
        assert_eq!(got.missing, vec!["D".to_owned()], "the map key is what gets reported");
    }

    #[test]
    fn a_request_string_resolves_both_namespaces() {
        let creds = creds(&[("TOKEN", "t")]);
        let values = HashMap::from([
            ("HOST".to_owned(), "localhost".to_owned()),
            ("PORT".to_owned(), "6042".to_owned()),
        ]);
        let scope = TransportScope::new(&creds, &values);

        // The tdengine shape: plain settings in the URL, a secret in a header.
        assert_eq!(
            resolve_request_string("${HOST}:${PORT}/api", &scope).value.as_deref(),
            Some("localhost:6042/api"),
        );
        assert_eq!(
            resolve_request_string("Bearer ${secret:TOKEN}", &scope).value.as_deref(),
            Some("Bearer t"),
        );

        // A missing plain value is a missing reference: the connector is not
        // usable, and saying so is better than sending `${HOST}`.
        let missing = resolve_request_string("${HOST}:${SCHEME}", &scope);
        assert_eq!(missing.value, None);
        assert_eq!(missing.missing, vec!["SCHEME".to_owned()]);

        // Namespaces do not leak into each other: a plain name is never looked up
        // in the credential store, and vice versa.
        assert_eq!(resolve_request_string("${TOKEN}", &scope).value, None);
        assert_eq!(resolve_request_string("${secret:HOST}", &scope).value, None);
    }

    #[test]
    fn a_principal_reads_its_own_entry_before_the_host_level_one() {
        let credentials = HashMap::from([
            ("TOKEN".to_owned(), "host".to_owned()),
            (scoped_key("alice", "TOKEN"), "alice-token".to_owned()),
            (scoped_key("bob", "TOKEN"), "bob-token".to_owned()),
        ]);
        // Two principals, one name, two values: this is the whole point of D1.
        assert_eq!(
            lookup_for_with(Some("alice"), "TOKEN", &credentials, Some("alice")).as_deref(),
            Some("alice-token")
        );
        assert_eq!(
            lookup_for_with(Some("bob"), "TOKEN", &credentials, Some("alice")).as_deref(),
            Some("bob-token")
        );
    }

    #[test]
    fn a_host_level_entry_belongs_to_the_operator_only() {
        let credentials = HashMap::from([("TOKEN".to_owned(), "host".to_owned())]);

        // No owner declared: the legacy single-user host, unchanged behaviour.
        assert_eq!(
            lookup_for_with(Some("alice"), "TOKEN", &credentials, None).as_deref(),
            Some("host")
        );

        // Owner declared: only that principal sees the bare entry. A different
        // principal gets nothing — not the operator's value, and no error that
        // would tell it one exists.
        assert_eq!(
            lookup_for_with(Some("alice"), "TOKEN", &credentials, Some("alice")).as_deref(),
            Some("host")
        );
        assert_eq!(
            lookup_for_with(Some("bob"), "TOKEN", &credentials, Some("alice")),
            None
        );
        // A host-internal caller with no identity acts for the operator.
        assert_eq!(
            lookup_for_with(None, "TOKEN", &credentials, Some("alice")).as_deref(),
            Some("host")
        );
    }

    #[test]
    fn a_principal_never_substitutes_another_principals_entry() {
        // Bob has an entry, Alice does not: Alice resolves nothing rather than
        // borrowing Bob's token, even on a host with no declared owner.
        let credentials = HashMap::from([(scoped_key("bob", "TOKEN"), "bob-token".to_owned())]);
        assert_eq!(lookup_for_with(Some("alice"), "TOKEN", &credentials, None), None);
        assert_eq!(
            lookup_for_with(Some("bob"), "TOKEN", &credentials, None).as_deref(),
            Some("bob-token")
        );
    }

    #[test]
    fn scoped_keys_round_trip_and_bare_keys_are_not_scoped() {
        assert_eq!(scoped_key("alice", "TOKEN"), "alice:TOKEN");
        assert_eq!(parse_scoped_key("alice:TOKEN"), Some(("alice", "TOKEN")));
        assert_eq!(parse_scoped_key("TOKEN"), None, "a bare key is host-level");
        assert_eq!(parse_scoped_key(":TOKEN"), None);
        assert_eq!(parse_scoped_key("alice:"), None);
    }

    #[test]
    fn a_scoped_request_string_resolves_for_the_caller() {
        let credentials = HashMap::from([
            (scoped_key("alice", "TOKEN"), "alice-token".to_owned()),
            ("HOST".to_owned(), "host".to_owned()),
        ]);
        let values = HashMap::from([("ENV".to_owned(), "prod".to_owned())]);
        let alice = TransportScope::for_principal(&credentials, &values, Some("alice"), Some("alice"));
        let bob = TransportScope::for_principal(&credentials, &values, Some("bob"), Some("alice"));

        assert_eq!(
            resolve_request_string("Bearer ${secret:TOKEN}", &alice).value.as_deref(),
            Some("Bearer alice-token")
        );
        // Bob has no TOKEN of his own and is not the operator, so his request must
        // not go out with somebody else's credential.
        let missing = resolve_request_string("Bearer ${secret:TOKEN}", &bob);
        assert_eq!(missing.value, None);
        assert_eq!(missing.missing, vec!["TOKEN".to_owned()]);
    }

    #[test]
    fn lookup_by_name_shares_the_reference_precedence() {
        // `bearerTokenEnvVar` names a variable instead of writing a reference, so
        // it must resolve through the same ladder — otherwise one credential
        // would have two meanings depending on which field named it.
        assert_eq!(
            lookup_with("K", &creds(&[("K", "config")])),
            Some("config".to_owned())
        );
        assert_eq!(
            lookup_with("__NOMIFUN_DEFINITELY_UNSET__", &HashMap::new()),
            None
        );
        // A name is used verbatim, never re-parsed as a reference.
        assert_eq!(
            lookup_with("secret:K", &creds(&[("K", "config")])),
            None,
            "the name is a lookup key, not a nested reference"
        );
    }
}

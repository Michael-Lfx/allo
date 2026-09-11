//! `secret:NAME` references in process-bound env maps (`17` §6 / `21` D5=C).
//!
//! A credential must never be persisted: not in a marketplace snapshot, not in
//! the MCP server DB row, not in logs. The value lives in the host's config
//! (`~/.agent-store/config.toml [credentials]`, with the process environment as
//! a fallback) and reaches a child process **only** at spawn time, in memory.
//!
//! The carrier between those two points is a **reference**: an env value that is
//! exactly `secret:NAME` names the credential to inject there. This module owns
//! the reference syntax and the resolution, so the importer (which writes the
//! reference) and every MCP spawn path (which resolves it) agree byte-for-byte.
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

/// Resolve one env value against an explicit credential map plus the process
/// environment — the pure, testable half of [`resolve_env`].
///
/// - an ordinary value is returned unchanged (non-secret env keeps working);
/// - `secret:NAME` resolves from `credentials` first, then the process env;
/// - an unresolvable reference returns `None` (the caller omits it).
///
/// The credential lookup order is deliberate: an explicit config entry wins
/// over an ambient environment variable of the same name.
pub fn resolve_value_with(
    value: &str,
    credentials: &HashMap<String, String>,
) -> Option<String> {
    match parse_secret_ref(value) {
        None => Some(value.to_owned()),
        Some(name) => credentials
            .get(name)
            .cloned()
            .or_else(|| std::env::var(name).ok()),
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
/// Non-reference values pass through, so a hand-registered server (whose env was
/// never rewritten at import) behaves exactly as before. A reference with no
/// matching credential is dropped and reported in [`ResolvedEnv::missing`] —
/// the child simply does not get that variable, rather than getting the literal
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
        match resolve_value_with(value, credentials) {
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
}

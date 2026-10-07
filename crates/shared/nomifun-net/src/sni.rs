//! SNI-less TLS fallback for hosts whose name is filtered on the network path.
//!
//! Some networks reset TLS handshakes that carry a particular server name
//! while the same IP accepts handshakes without SNI. Callers decide which
//! hosts are eligible; this module only classifies the failure and remembers,
//! process-wide, which hosts already needed the fallback so every HTTP client
//! in the process can skip the doomed SNI handshake.

use std::collections::HashSet;
use std::error::Error;
use std::sync::{LazyLock, RwLock};

static PREFER_NO_SNI: LazyLock<RwLock<HashSet<String>>> =
    LazyLock::new(|| RwLock::new(HashSet::new()));

fn host_of(url: &str) -> Option<String> {
    reqwest::Url::parse(url)
        .ok()?
        .host_str()
        .map(str::to_ascii_lowercase)
}

pub fn url_host_is(url: &str, host: &str) -> bool {
    host_of(url).is_some_and(|h| h.eq_ignore_ascii_case(host))
}

pub fn prefers_no_sni(url: &str) -> bool {
    let Some(host) = host_of(url) else {
        return false;
    };
    PREFER_NO_SNI
        .read()
        .map(|set| set.contains(&host))
        .unwrap_or(false)
}

/// Returns true when the preference changed.
pub fn set_prefers_no_sni(url: &str, prefer: bool) -> bool {
    let Some(host) = host_of(url) else {
        return false;
    };
    let Ok(mut set) = PREFER_NO_SNI.write() else {
        return false;
    };
    if prefer {
        set.insert(host)
    } else {
        set.remove(&host)
    }
}

pub fn is_connection_reset(err: &reqwest::Error) -> bool {
    let mut source = Error::source(err);
    while let Some(cause) = source {
        if io_error_is_reset(cause) {
            return true;
        }
        source = cause.source();
    }
    false
}

fn io_error_is_reset(err: &(dyn Error + 'static)) -> bool {
    let Some(io) = err.downcast_ref::<std::io::Error>() else {
        return false;
    };
    if io.kind() == std::io::ErrorKind::ConnectionReset {
        return true;
    }
    // `io::Error::source` skips a wrapped custom error, so descend via `get_ref`.
    io.get_ref()
        .is_some_and(|inner| io_error_is_reset(inner as &(dyn Error + 'static)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_host_is_matches_case_insensitively() {
        assert!(url_host_is("https://SERVER.example.com/claw/v1", "server.example.com"));
        assert!(!url_host_is("https://other.example.com/", "server.example.com"));
        assert!(!url_host_is("not a url", "server.example.com"));
    }

    #[test]
    fn preference_is_per_host_and_reversible() {
        let url = "https://sni-pref-test.example/claw/v1/chat";
        assert!(!prefers_no_sni(url));
        assert!(set_prefers_no_sni(url, true));
        assert!(!set_prefers_no_sni("https://SNI-PREF-TEST.example/other", true));
        assert!(prefers_no_sni("https://sni-pref-test.example/health"));
        assert!(!prefers_no_sni("https://unrelated.example/"));
        assert!(set_prefers_no_sni(url, false));
        assert!(!prefers_no_sni(url));
    }
}

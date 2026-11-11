//! Shared outbound HTTP transport for Flowy.
//!
//! Product URLs and update policy do not live here. Callers configure timeouts
//! and headers on the builder after [`configure`].

mod proxy;

pub use proxy::apply_proxy;

use reqwest::blocking::{Client, ClientBuilder};

/// Apply system proxy detection to a blocking reqwest builder.
pub fn configure(builder: ClientBuilder) -> ClientBuilder {
    apply_proxy(builder)
}

/// Build a blocking client with system proxy detection.
pub fn blocking_client() -> Result<Client, reqwest::Error> {
    configure(Client::builder()).build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocking_client_builds() {
        assert!(blocking_client().is_ok());
    }
}

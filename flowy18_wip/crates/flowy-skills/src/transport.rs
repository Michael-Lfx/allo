use crate::MAX_PACKAGE_BYTES;
use reqwest::blocking::Client;
use serde_json::Value;
use std::io::Read;
use std::time::Duration;
use thiserror::Error;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpText {
    pub status: u16,
    pub body: String,
    pub retry_after_secs: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpBytes {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    pub retry_after_secs: Option<u64>,
}

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("could not build HTTP client: {0}")]
    Client(String),
    #[error("HTTP request failed: {0}")]
    Request(String),
    #[error("could not read HTTP body: {0}")]
    Body(String),
    #[error("HTTP body exceeds the {limit} byte limit")]
    BodyTooLarge { limit: u64 },
}

pub trait CatalogTransport {
    fn get_text(&self, url: &str) -> Result<HttpText, TransportError>;
    fn post_json(
        &self,
        url: &str,
        body: &Value,
    ) -> Result<HttpText, TransportError>;
}

pub trait InstallTransport {
    fn get_bytes(&self, url: &str) -> Result<HttpBytes, TransportError>;
}

pub struct NetTransport {
    client: Client,
}

impl std::fmt::Debug for NetTransport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NetTransport")
            .finish_non_exhaustive()
    }
}

impl NetTransport {
    pub fn new() -> Result<Self, TransportError> {
        let client = flowy_net::configure(
            Client::builder()
                .connect_timeout(CONNECT_TIMEOUT)
                .timeout(REQUEST_TIMEOUT)
                .user_agent(format!(
                    "flowy-skills/{}",
                    env!("CARGO_PKG_VERSION")
                )),
        )
        .build()
        .map_err(|error| TransportError::Client(error.to_string()))?;
        Ok(Self { client })
    }

    fn text_response(
        response: reqwest::blocking::Response,
    ) -> Result<HttpText, TransportError> {
        let status = response.status().as_u16();
        let retry_after_secs = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse().ok());
        let body = response
            .text()
            .map_err(|error| TransportError::Body(error.to_string()))?;
        Ok(HttpText {
            status,
            body,
            retry_after_secs,
        })
    }
}

impl CatalogTransport for NetTransport {
    fn get_text(&self, url: &str) -> Result<HttpText, TransportError> {
        let response = self
            .client
            .get(url)
            .send()
            .map_err(|error| TransportError::Request(error.to_string()))?;
        Self::text_response(response)
    }

    fn post_json(
        &self,
        url: &str,
        body: &Value,
    ) -> Result<HttpText, TransportError> {
        let response = self
            .client
            .post(url)
            .json(body)
            .send()
            .map_err(|error| TransportError::Request(error.to_string()))?;
        Self::text_response(response)
    }
}

impl InstallTransport for NetTransport {
    fn get_bytes(&self, url: &str) -> Result<HttpBytes, TransportError> {
        let response = self
            .client
            .get(url)
            .send()
            .map_err(|error| TransportError::Request(error.to_string()))?;
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let retry_after_secs = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse().ok());
        if response
            .content_length()
            .is_some_and(|length| length > MAX_PACKAGE_BYTES)
        {
            return Err(TransportError::BodyTooLarge {
                limit: MAX_PACKAGE_BYTES,
            });
        }
        let mut response = response;
        let body = read_limited(&mut response, MAX_PACKAGE_BYTES)?;
        Ok(HttpBytes {
            status,
            content_type,
            body,
            retry_after_secs,
        })
    }
}

fn read_limited(
    reader: &mut impl Read,
    limit: u64,
) -> Result<Vec<u8>, TransportError> {
    let mut body =
        Vec::with_capacity(usize::try_from(limit.min(64 * 1024)).unwrap_or(0));
    let mut limited = reader.take(limit.saturating_add(1));
    limited
        .read_to_end(&mut body)
        .map_err(|error| TransportError::Body(error.to_string()))?;
    if u64::try_from(body.len()).unwrap_or(u64::MAX) > limit {
        return Err(TransportError::BodyTooLarge { limit });
    }
    Ok(body)
}

#[must_use]
pub fn retry_delay(
    status: u16,
    retry_after_secs: Option<u64>,
    jitter_ms: u64,
) -> Option<u64> {
    matches!(status, 429 | 503).then(|| {
        retry_after_secs
            .unwrap_or(1)
            .saturating_mul(1000)
            .saturating_add(jitter_ms)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_delay_only_applies_to_rate_limit_and_unavailable() {
        assert_eq!(retry_delay(429, Some(2), 10), Some(2010));
        assert_eq!(retry_delay(503, None, 10), Some(1010));
        assert_eq!(retry_delay(404, Some(2), 10), None);
    }

    #[test]
    fn bounded_reader_accepts_the_limit_and_rejects_one_more_byte() {
        let mut exact = std::io::Cursor::new(vec![0_u8; 32]);
        assert_eq!(read_limited(&mut exact, 32).unwrap().len(), 32);
        let mut over = std::io::Cursor::new(vec![0_u8; 33]);
        assert!(matches!(
            read_limited(&mut over, 32),
            Err(TransportError::BodyTooLarge { limit: 32 })
        ));
    }
}

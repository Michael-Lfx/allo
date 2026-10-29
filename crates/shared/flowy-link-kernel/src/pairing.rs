use sha2::{Digest, Sha256};
use thiserror::Error;

/// Preferred TLS port for Link. Plaintext WebUI stays on 25808.
pub const LINK_TLS_PORT: u16 = 25818;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingUrl {
    pub version: u32,
    pub port: u16,
    pub fingerprint: String,
    pub nonce: String,
    pub addresses: Vec<String>,
    pub name: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PairingUrlError {
    #[error("pairing url must start with flowy://pair")]
    Scheme,
    #[error("pairing url is missing {0}")]
    Missing(&'static str),
    #[error("pairing url field {0} is invalid")]
    Invalid(&'static str),
}

/// SHA-256 of certificate bytes, lowercase hex. This is the host id.
pub fn fingerprint_sha256(der: &[u8]) -> String {
    let digest = Sha256::digest(der);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn build_pairing_url(url: &PairingUrl) -> String {
    let hosts = url.addresses.join(",");
    format!(
        "flowy://pair?v={}&p={}&fp={}&n={}&h={}&name={}",
        url.version,
        url.port,
        url.fingerprint,
        url.nonce,
        percent_encode(&hosts),
        percent_encode(&url.name),
    )
}

pub fn parse_pairing_url(raw: &str) -> Result<PairingUrl, PairingUrlError> {
    let rest = raw
        .strip_prefix("flowy://pair?")
        .ok_or(PairingUrlError::Scheme)?;
    let mut version = None;
    let mut port = None;
    let mut fingerprint = None;
    let mut nonce = None;
    let mut addresses = Vec::new();
    let mut name = None;
    for pair in rest.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            return Err(PairingUrlError::Invalid("pair"));
        };
        let value = percent_decode(value);
        match key {
            "v" => version = Some(parse_u32(&value)?),
            "p" => port = Some(parse_port(&value)?),
            "fp" => fingerprint = Some(require_fingerprint(&value)?),
            "n" => nonce = Some(require_nonce(&value)?),
            "h" => addresses = split_addresses(&value)?,
            "name" => name = Some(value),
            _ => return Err(PairingUrlError::Invalid("key")),
        }
    }
    Ok(PairingUrl {
        version: version.ok_or(PairingUrlError::Missing("v"))?,
        port: port.ok_or(PairingUrlError::Missing("p"))?,
        fingerprint: fingerprint.ok_or(PairingUrlError::Missing("fp"))?,
        nonce: nonce.ok_or(PairingUrlError::Missing("n"))?,
        addresses,
        name: name.ok_or(PairingUrlError::Missing("name"))?,
    })
}

fn parse_u32(value: &str) -> Result<u32, PairingUrlError> {
    value.parse().map_err(|_| PairingUrlError::Invalid("v"))
}

fn parse_port(value: &str) -> Result<u16, PairingUrlError> {
    value.parse().map_err(|_| PairingUrlError::Invalid("p"))
}

fn require_fingerprint(value: &str) -> Result<String, PairingUrlError> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(value.to_ascii_lowercase())
    } else {
        Err(PairingUrlError::Invalid("fp"))
    }
}

fn require_nonce(value: &str) -> Result<String, PairingUrlError> {
    if (16..=128).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(value.to_ascii_lowercase())
    } else {
        Err(PairingUrlError::Invalid("n"))
    }
}

fn split_addresses(value: &str) -> Result<Vec<String>, PairingUrlError> {
    let addresses: Vec<String> = value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect();
    if addresses.is_empty() {
        return Err(PairingUrlError::Invalid("h"));
    }
    Ok(addresses)
}

fn percent_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b',' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(decoded) = u8::from_str_radix(
                std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or(""),
                16,
            ) {
                out.push(decoded);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_self_contained_code() {
        let fingerprint = "ab".repeat(32);
        let url = format!(
            "flowy://pair?v=1&p=25818&fp={fingerprint}&n={nonce}&h=192.168.1.20,10.0.0.8&name=Study",
            nonce = "cd".repeat(16)
        );
        let parsed = parse_pairing_url(&url).unwrap();
        assert_eq!(parsed.port, LINK_TLS_PORT);
        assert_eq!(parsed.addresses.len(), 2);
        assert_eq!(parsed.name, "Study");
        assert_eq!(build_pairing_url(&parsed), url);
    }

    #[test]
    fn round_trips_a_name_with_spaces() {
        let parsed = PairingUrl {
            version: 1,
            port: LINK_TLS_PORT,
            fingerprint: "ab".repeat(32),
            nonce: "cd".repeat(16),
            addresses: vec!["192.168.1.20".into()],
            name: "Study PC".into(),
        };
        let raw = build_pairing_url(&parsed);
        assert_eq!(parse_pairing_url(&raw).unwrap(), parsed);
    }

    #[test]
    fn rejects_a_short_fingerprint() {
        let url = "flowy://pair?v=1&p=1&fp=abcd&n=00112233445566778899aabbccddeeff&h=127.0.0.1&name=a";
        assert_eq!(parse_pairing_url(url), Err(PairingUrlError::Invalid("fp")));
    }
}

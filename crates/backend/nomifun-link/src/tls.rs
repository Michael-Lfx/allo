//! Stable TLS identity and the Link listener on port 25818.
//!
//! The certificate is self-signed and pinned by SHA-256 of its DER encoding.
//! Phone clients do not use the system trust store for this connection.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::Mutex;

use axum::Router;
use flowy_link_kernel::{LINK_TLS_PORT, fingerprint_sha256};
use rcgen::{CertificateParams, DnType, KeyPair};
use thiserror::Error;

static HOST: Mutex<Option<LinkHost>> = Mutex::new(None);

#[derive(Debug, Clone)]
pub struct LinkHost {
    pub fingerprint: String,
    pub name: String,
}

pub struct LinkIdentity {
    pub fingerprint: String,
    pub cert_pem: String,
    pub key_pem: String,
}

#[derive(Debug, Error)]
pub enum LinkTlsError {
    #[error("{0}")]
    Message(String),
}

pub fn publish_host(host: LinkHost) {
    *HOST.lock().expect("link host") = Some(host);
}

pub fn current_host() -> Option<LinkHost> {
    HOST.lock().expect("link host").clone()
}

pub fn load_or_create_identity(dir: &Path) -> Result<LinkIdentity, LinkTlsError> {
    std::fs::create_dir_all(dir).map_err(|error| LinkTlsError::Message(error.to_string()))?;
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    if cert_path.is_file() && key_path.is_file() {
        let cert_pem = std::fs::read_to_string(&cert_path).map_err(|error| LinkTlsError::Message(error.to_string()))?;
        let key_pem = std::fs::read_to_string(&key_path).map_err(|error| LinkTlsError::Message(error.to_string()))?;
        let der = pem_certificate_der(&cert_pem)?;
        return Ok(LinkIdentity {
            fingerprint: fingerprint_sha256(&der),
            cert_pem,
            key_pem,
        });
    }
    let identity = generate_identity()?;
    std::fs::write(&cert_path, &identity.cert_pem).map_err(|error| LinkTlsError::Message(error.to_string()))?;
    std::fs::write(&key_path, &identity.key_pem).map_err(|error| LinkTlsError::Message(error.to_string()))?;
    Ok(identity)
}

fn generate_identity() -> Result<LinkIdentity, LinkTlsError> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut params = CertificateParams::new(vec!["localhost".to_string()])
        .map_err(|error| LinkTlsError::Message(error.to_string()))?;
    params
        .distinguished_name
        .push(DnType::CommonName, "Flowy Link");
    let key = KeyPair::generate().map_err(|error| LinkTlsError::Message(error.to_string()))?;
    let cert = params
        .self_signed(&key)
        .map_err(|error| LinkTlsError::Message(error.to_string()))?;
    Ok(LinkIdentity {
        fingerprint: fingerprint_sha256(cert.der()),
        cert_pem: cert.pem(),
        key_pem: key.serialize_pem(),
    })
}

fn pem_certificate_der(pem: &str) -> Result<Vec<u8>, LinkTlsError> {
    let block = pem
        .split("-----END CERTIFICATE-----")
        .next()
        .and_then(|section| section.split("-----BEGIN CERTIFICATE-----").nth(1))
        .ok_or_else(|| LinkTlsError::Message("link certificate pem is missing".into()))?;
    let encoded: String = block.chars().filter(|ch| !ch.is_whitespace()).collect();
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| LinkTlsError::Message(error.to_string()))
}

pub fn advertise_addresses() -> Vec<String> {
    let mut addresses = Vec::new();
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            if iface.is_loopback() {
                continue;
            }
            let std::net::IpAddr::V4(ip) = iface.ip() else {
                continue;
            };
            if ip.is_unspecified() || ip.is_link_local() || ip.is_broadcast() {
                continue;
            }
            let text = ip.to_string();
            if !addresses.contains(&text) {
                addresses.push(text);
            }
        }
    }
    if addresses.is_empty() {
        addresses.push(Ipv4Addr::LOCALHOST.to_string());
    }
    addresses
}

pub fn advertise_link(fingerprint: &str) -> Option<mdns_sd::ServiceDaemon> {
    let address = advertise_addresses().into_iter().find(|item| item != "127.0.0.1")?;
    let ip = address.parse::<std::net::IpAddr>().ok()?;
    let daemon = mdns_sd::ServiceDaemon::new().ok()?;
    let properties = [("fp", fingerprint)];
    let info = mdns_sd::ServiceInfo::new(
        "_flowy-link._tcp.local.",
        "Flowy",
        "flowy-link.local.",
        ip,
        flowy_link_kernel::LINK_TLS_PORT,
        &properties[..],
    )
    .ok()?;
    daemon.register(info).ok()?;
    Some(daemon)
}

pub async fn serve_tls(router: Router, identity: LinkIdentity) -> Result<(), LinkTlsError> {
    serve_tls_on(router, identity, LINK_TLS_PORT).await
}

pub async fn serve_tls_on(router: Router, identity: LinkIdentity, port: u16) -> Result<(), LinkTlsError> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let config = axum_server::tls_rustls::RustlsConfig::from_pem(
        identity.cert_pem.into_bytes(),
        identity.key_pem.into_bytes(),
    )
    .await
    .map_err(|error| LinkTlsError::Message(error.to_string()))?;
    let address = SocketAddr::from((Ipv4Addr::UNSPECIFIED, port));
    axum_server::bind_rustls(address, config)
        .serve(router.into_make_service())
        .await
        .map_err(|error| LinkTlsError::Message(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use rustls::{DigitallySignedStruct, Error, SignatureScheme};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    #[test]
    fn identity_fingerprint_is_stable() {
        let dir = std::env::temp_dir().join(format!("flowy-link-identity-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let first = load_or_create_identity(&dir).unwrap();
        let second = load_or_create_identity(&dir).unwrap();
        assert_eq!(first.fingerprint, second.fingerprint);
        assert_eq!(first.fingerprint.len(), 64);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[derive(Debug)]
    struct PinVerifier(String);

    impl ServerCertVerifier for PinVerifier {
        fn verify_server_cert(
            &self,
            end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp_response: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, Error> {
            let actual = fingerprint_sha256(end_entity.as_ref());
            if actual == self.0 {
                Ok(ServerCertVerified::assertion())
            } else {
                Err(Error::General("fingerprint mismatch".into()))
            }
        }

        fn verify_tls12_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            rustls::crypto::verify_tls12_signature(
                message,
                cert,
                dss,
                &rustls::crypto::ring::default_provider().signature_verification_algorithms,
            )
        }

        fn verify_tls13_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            rustls::crypto::verify_tls13_signature(
                message,
                cert,
                dss,
                &rustls::crypto::ring::default_provider().signature_verification_algorithms,
            )
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            rustls::crypto::ring::default_provider()
                .signature_verification_algorithms
                .supported_schemes()
        }
    }

    #[tokio::test]
    async fn pinned_client_reaches_the_listener() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let identity = generate_identity().unwrap();
        let expected = identity.fingerprint.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let app = Router::new().route("/health", get(|| async { "ok" }));
        tokio::spawn(async move {
            let _ = serve_tls_on(app, identity, port).await;
        });
        let address = format!("127.0.0.1:{port}");
        let mut connected = None;
        for _ in 0..50 {
            if let Ok(stream) = TcpStream::connect(&address).await {
                connected = Some(stream);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let stream = connected.expect("listener");
        let config = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(PinVerifier(expected)))
            .with_no_client_auth();
        let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
        let name = ServerName::try_from("localhost").unwrap();
        let mut tls = connector.connect(name, stream).await.unwrap();
        tls.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut body = Vec::new();
        tls.read_to_end(&mut body).await.unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("200"), "{text}");
        assert!(text.contains("ok"), "{text}");
    }
}

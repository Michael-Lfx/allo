pub mod proxy;
pub mod secret_redaction;
pub mod ssrf;

pub fn http_client() -> reqwest::Client {
    proxy::apply_detected_proxy(reqwest::Client::builder())
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

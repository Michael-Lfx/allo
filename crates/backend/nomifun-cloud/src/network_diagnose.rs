//! Staged connectivity probe for the Flowy cloud API host.
//!
//! Used by the login screen so mainland / proxy failures can be reported as
//! DNS / TCP / TLS / HTTP steps instead of a single opaque toast.

use std::net::ToSocketAddrs;
use std::time::{Duration, Instant};

use nomi_config::{DEFAULT_WECHAT_FLOWY_SERVER_BASE, ServerConfig};
use serde::Serialize;
use tokio::net::TcpStream;
use tokio::time::timeout;

const STEP_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkDiagnoseStep {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkDiagnoseReport {
    pub target: String,
    pub host: String,
    pub steps: Vec<NetworkDiagnoseStep>,
    pub ok: bool,
    pub summary: String,
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u64::MAX as u128) as u64
}

fn resolve_base_url(config: &ServerConfig) -> String {
    let trimmed = config.base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        DEFAULT_WECHAT_FLOWY_SERVER_BASE.to_string()
    } else {
        trimmed.to_string()
    }
}

fn parse_host_port(base_url: &str) -> Result<(String, u16, bool), String> {
    let url = url::Url::parse(base_url).map_err(|e| format!("invalid base_url: {e}"))?;
    let host = url
        .host_str()
        .ok_or_else(|| "base_url missing host".to_string())?
        .to_string();
    let https = url.scheme() == "https";
    let port = url
        .port()
        .unwrap_or(if https { 443 } else { 80 });
    Ok((host, port, https))
}

async fn step_dns(host: &str) -> NetworkDiagnoseStep {
    let started = Instant::now();
    let lookup_host = host.to_owned();
    match tokio::task::spawn_blocking(move || {
        (lookup_host.as_str(), 0u16)
            .to_socket_addrs()
            .map(|iter| {
                iter.map(|addr| addr.ip().to_string())
                    .take(4)
                    .collect::<Vec<_>>()
            })
    })
    .await
    {
        Ok(Ok(ips)) if !ips.is_empty() => NetworkDiagnoseStep {
            name: "dns",
            ok: true,
            detail: ips.join(", "),
            duration_ms: elapsed_ms(started),
        },
        Ok(Ok(_)) => NetworkDiagnoseStep {
            name: "dns",
            ok: false,
            detail: "no addresses".into(),
            duration_ms: elapsed_ms(started),
        },
        Ok(Err(err)) => NetworkDiagnoseStep {
            name: "dns",
            ok: false,
            detail: err.to_string(),
            duration_ms: elapsed_ms(started),
        },
        Err(err) => NetworkDiagnoseStep {
            name: "dns",
            ok: false,
            detail: format!("join error: {err}"),
            duration_ms: elapsed_ms(started),
        },
    }
}

async fn step_tcp(host: &str, port: u16) -> NetworkDiagnoseStep {
    let started = Instant::now();
    let addr = format!("{host}:{port}");
    match timeout(STEP_TIMEOUT, TcpStream::connect(&addr)).await {
        Ok(Ok(_stream)) => NetworkDiagnoseStep {
            name: "tcp",
            ok: true,
            detail: format!("connected to {addr}"),
            duration_ms: elapsed_ms(started),
        },
        Ok(Err(err)) => NetworkDiagnoseStep {
            name: "tcp",
            ok: false,
            detail: err.to_string(),
            duration_ms: elapsed_ms(started),
        },
        Err(_) => NetworkDiagnoseStep {
            name: "tcp",
            ok: false,
            detail: format!("connect timed out after {}s", STEP_TIMEOUT.as_secs()),
            duration_ms: elapsed_ms(started),
        },
    }
}

async fn step_http(base_url: &str) -> NetworkDiagnoseStep {
    let started = Instant::now();
    let health_url = format!("{}/health", base_url.trim_end_matches('/'));
    let client = match reqwest::Client::builder()
        .connect_timeout(STEP_TIMEOUT)
        .timeout(STEP_TIMEOUT)
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            return NetworkDiagnoseStep {
                name: "http",
                ok: false,
                detail: format!("build client: {err}"),
                duration_ms: elapsed_ms(started),
            };
        }
    };

    match client.get(&health_url).send().await {
        Ok(resp) => {
            let status = resp.status();
            let version = format!("{:?}", resp.version());
            NetworkDiagnoseStep {
                name: "http",
                ok: status.is_success() || status.as_u16() == 404,
                detail: format!("{health_url} → {status} ({version})"),
                duration_ms: elapsed_ms(started),
            }
        }
        Err(err) => {
            let kind = if err.is_timeout() {
                "timeout"
            } else if err.is_connect() {
                "connect"
            } else if err.is_request() {
                "request"
            } else {
                "other"
            };
            NetworkDiagnoseStep {
                name: "http",
                ok: false,
                detail: format!("{err} [kind={kind}]"),
                duration_ms: elapsed_ms(started),
            }
        }
    }
}

pub async fn diagnose_cloud_network(config: &ServerConfig) -> NetworkDiagnoseReport {
    let target = resolve_base_url(config);
    let (host, port, _https) = match parse_host_port(&target) {
        Ok(parts) => parts,
        Err(err) => {
            return NetworkDiagnoseReport {
                target: target.clone(),
                host: String::new(),
                steps: vec![NetworkDiagnoseStep {
                    name: "parse",
                    ok: false,
                    detail: err,
                    duration_ms: 0,
                }],
                ok: false,
                summary: "invalid server base_url".into(),
            };
        }
    };

    let dns = step_dns(&host).await;
    let tcp = if dns.ok {
        step_tcp(&host, port).await
    } else {
        NetworkDiagnoseStep {
            name: "tcp",
            ok: false,
            detail: "skipped (dns failed)".into(),
            duration_ms: 0,
        }
    };
    let http = if tcp.ok {
        step_http(&target).await
    } else {
        NetworkDiagnoseStep {
            name: "http",
            ok: false,
            detail: "skipped (tcp failed)".into(),
            duration_ms: 0,
        }
    };

    let steps = vec![dns, tcp, http];
    let ok = steps.iter().all(|step| step.ok);
    let summary = if ok {
        format!("ok → {target}")
    } else {
        steps
            .iter()
            .find(|step| !step.ok)
            .map(|step| format!("{} failed: {}", step.name, step.detail))
            .unwrap_or_else(|| "failed".into())
    };

    NetworkDiagnoseReport {
        target,
        host,
        steps,
        ok,
        summary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_base_url_falls_back_to_default() {
        let cfg = ServerConfig::default();
        assert_eq!(resolve_base_url(&cfg), DEFAULT_WECHAT_FLOWY_SERVER_BASE);
    }

    #[test]
    fn parse_host_port_reads_https_default() {
        let (host, port, https) =
            parse_host_port("https://server.flowyaipc.com/claw").expect("parse");
        assert_eq!(host, "server.flowyaipc.com");
        assert_eq!(port, 443);
        assert!(https);
    }

    #[tokio::test]
    async fn diagnose_loopback_closed_port_fails_tcp() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let cfg = ServerConfig {
            base_url: format!("http://127.0.0.1:{port}/claw"),
            ..Default::default()
        };
        let report = diagnose_cloud_network(&cfg).await;
        assert!(!report.ok);
        assert_eq!(report.steps[0].name, "dns");
        assert!(report.steps[0].ok);
        assert_eq!(report.steps[1].name, "tcp");
        assert!(!report.steps[1].ok);
    }
}

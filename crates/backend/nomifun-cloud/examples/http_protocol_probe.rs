//! HTTP Protocol Performance Benchmark Probe (HTTP/2 vs HTTP/3)
//!
//! Measures cold-connect handshake latency, warm-connection TTFB, total response time,
//! and transport protocol behavior against Flowy servers or any HTTP endpoint.

use std::error::Error;
use std::time::{Duration, Instant};
use clap::{Parser, ValueEnum};
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use reqwest::{Client, Version};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ProtocolTarget {
    H2,
    H3,
    Both,
}

#[derive(Parser, Debug)]
#[command(author, version, about = "HTTP/2 vs HTTP/3 Performance Benchmark Probe", long_about = None)]
struct Args {
    /// Target URL to probe
    #[arg(short, long, default_value = "https://server.flowyaipc.com/claw/health")]
    target: String,

    /// Protocol to test (h2, h3, or both)
    #[arg(short, long, value_enum, default_value_t = ProtocolTarget::Both)]
    protocol: ProtocolTarget,

    /// Number of test rounds per protocol
    #[arg(short, long, default_value_t = 10)]
    rounds: usize,

    /// Request timeout in seconds
    #[arg(long, default_value_t = 10)]
    timeout: u64,
}

#[derive(Debug, Default)]
struct ProbeMetrics {
    protocol_label: String,
    negotiated_version: String,
    cold_total_ms: f64,
    warm_samples_ms: Vec<f64>,
    success_count: usize,
    failure_count: usize,
    errors: Vec<String>,
}

impl ProbeMetrics {
    fn calculate_stats(&self) -> (f64, f64, f64, f64) {
        if self.warm_samples_ms.is_empty() {
            return (0.0, 0.0, 0.0, 0.0);
        }
        let mut sorted = self.warm_samples_ms.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let count = sorted.len();
        let sum: f64 = sorted.iter().sum();
        let mean = sum / count as f64;
        let p50 = sorted[count * 50 / 100];
        let p90 = sorted[count * 90 / 100];
        let p99 = sorted[(count * 99 / 100).min(count - 1)];
        (mean, p50, p90, p99)
    }

    fn print_report(&self) {
        println!("\n============================================================");
        println!(" Protocol Test Report: {}", self.protocol_label);
        println!(" Negotiated Version:    {}", self.negotiated_version);
        println!(" Total Requests:        {}", self.success_count + self.failure_count);
        println!(" Successful:            {}", self.success_count);
        println!(" Failed:                {}", self.failure_count);
        println!("------------------------------------------------------------");
        println!(" [Cold Start Connection (1st Request)]");
        println!("   Total Request Time:   {:.2} ms", self.cold_total_ms);
        println!("------------------------------------------------------------");
        if !self.warm_samples_ms.is_empty() {
            let (mean, p50, p90, p99) = self.calculate_stats();
            println!(" [Warm Connection (Reused Connection, {} rounds)]", self.warm_samples_ms.len());
            println!("   Mean Latency:         {:.2} ms", mean);
            println!("   P50  Latency:         {:.2} ms", p50);
            println!("   P90  Latency:         {:.2} ms", p90);
            println!("   P99  Latency:         {:.2} ms", p99);
        }
        if !self.errors.is_empty() {
            println!("------------------------------------------------------------");
            println!(" [Errors Encountered (first 3)]:");
            for err in self.errors.iter().take(3) {
                println!("   * {}", err);
            }
        }
        println!("============================================================");
    }
}

fn build_h2_client(timeout: Duration) -> Result<Client, Box<dyn Error>> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static("flowy-protocol-probe/h2"));

    let client = Client::builder()
        .timeout(timeout)
        .default_headers(headers)
        .build()?;
    Ok(client)
}

#[cfg(feature = "http3-experimental")]
fn build_h3_client(timeout: Duration) -> Result<Client, Box<dyn Error>> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static("flowy-protocol-probe/h3"));

    let client = Client::builder()
        .timeout(timeout)
        .default_headers(headers)
        .http3_prior_knowledge()
        .build()?;
    Ok(client)
}

#[cfg(not(feature = "http3-experimental"))]
fn build_h3_client(_timeout: Duration) -> Result<Client, Box<dyn Error>> {
    Err("http3-experimental feature is not enabled. Re-run with --features http3-experimental and RUSTFLAGS='--cfg reqwest_unstable'".into())
}

fn extract_error_chain(err: &reqwest::Error) -> String {
    let mut s = format!("{}", err);
    let mut source = err.source();
    while let Some(e) = source {
        s.push_str(&format!(" -> {}", e));
        source = e.source();
    }
    s
}

async fn run_protocol_test(
    label: &str,
    client: Client,
    target: &str,
    rounds: usize,
) -> ProbeMetrics {
    let mut metrics = ProbeMetrics {
        protocol_label: label.to_string(),
        ..Default::default()
    };

    println!("\n>>> Testing [{}] against: {}", label, target);

    // 1. Cold start request
    let t_start = Instant::now();
    match client.get(target).send().await {
        Ok(resp) => {
            let status = resp.status();
            let version = format_version(resp.version());
            metrics.negotiated_version = version;
            let _body = resp.bytes().await;
            let elapsed = t_start.elapsed().as_secs_f64() * 1000.0;
            metrics.cold_total_ms = elapsed;
            metrics.success_count += 1;
            println!("  [Cold #1] Status: {}, Version: {}, Time: {:.2} ms", status, metrics.negotiated_version, elapsed);
        }
        Err(e) => {
            let detail = extract_error_chain(&e);
            metrics.failure_count += 1;
            metrics.errors.push(detail.clone());
            println!("  [Cold #1] Failed: {}", detail);
        }
    }

    // 2. Warm reused connection requests
    for i in 2..=rounds {
        let t0 = Instant::now();
        match client.get(target).send().await {
            Ok(resp) => {
                let _body = resp.bytes().await;
                let elapsed = t0.elapsed().as_secs_f64() * 1000.0;
                metrics.warm_samples_ms.push(elapsed);
                metrics.success_count += 1;
                print!("  [Warm #{}] {:.2}ms ", i, elapsed);
                if i % 5 == 0 || i == rounds {
                    println!();
                }
            }
            Err(e) => {
                let detail = extract_error_chain(&e);
                metrics.failure_count += 1;
                metrics.errors.push(detail.clone());
                println!("\n  [Warm #{}] Failed: {}", i, detail);
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    metrics
}

fn format_version(v: Version) -> String {
    match v {
        Version::HTTP_09 => "HTTP/0.9".into(),
        Version::HTTP_10 => "HTTP/1.0".into(),
        Version::HTTP_11 => "HTTP/1.1".into(),
        Version::HTTP_2 => "HTTP/2".into(),
        Version::HTTP_3 => "HTTP/3 (QUIC)".into(),
        _ => format!("{:?}", v),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let timeout = Duration::from_secs(args.timeout);

    println!("============================================================");
    println!(" Flowy HTTP/2 vs HTTP/3 Protocol Benchmark Probe");
    println!(" Target:   {}", args.target);
    println!(" Protocol: {:?}", args.protocol);
    println!(" Rounds:   {}", args.rounds);
    println!("============================================================");

    let mut h2_metrics: Option<ProbeMetrics> = None;
    let mut h3_metrics: Option<ProbeMetrics> = None;

    if args.protocol == ProtocolTarget::H2 || args.protocol == ProtocolTarget::Both {
        match build_h2_client(timeout) {
            Ok(client) => {
                let m = run_protocol_test("HTTP/2 (TCP)", client, &args.target, args.rounds).await;
                m.print_report();
                h2_metrics = Some(m);
            }
            Err(e) => {
                eprintln!("Failed to build HTTP/2 client: {}", e);
            }
        }
    }

    if args.protocol == ProtocolTarget::H3 || args.protocol == ProtocolTarget::Both {
        match build_h3_client(timeout) {
            Ok(client) => {
                let m = run_protocol_test("HTTP/3 (QUIC/UDP)", client, &args.target, args.rounds).await;
                m.print_report();
                h3_metrics = Some(m);
            }
            Err(e) => {
                eprintln!("\nFailed to build HTTP/3 client: {}", e);
            }
        }
    }

    // Direct comparative summary
    if let (Some(h2), Some(h3)) = (h2_metrics, h3_metrics) {
        println!("\n============================================================");
        println!("             HEAD-TO-HEAD COMPARISON SUMMARY                ");
        println!("============================================================");
        println!("{:<24} | {:<16} | {:<16}", "Metric", "HTTP/2", "HTTP/3");
        println!("------------------------------------------------------------");
        println!("{:<24} | {:<16} | {:<16}", "Negotiated Protocol", h2.negotiated_version, h3.negotiated_version);
        println!("{:<24} | {:<16} | {:<16}", "Success Rate", format!("{}/{}", h2.success_count, h2.success_count + h2.failure_count), format!("{}/{}", h3.success_count, h3.success_count + h3.failure_count));
        println!("{:<24} | {:<16.2} | {:<16.2}", "Cold Request (ms)", h2.cold_total_ms, h3.cold_total_ms);
        let (h2_mean, h2_p50, h2_p90, _) = h2.calculate_stats();
        let (h3_mean, h3_p50, h3_p90, _) = h3.calculate_stats();
        println!("{:<24} | {:<16.2} | {:<16.2}", "Warm Mean (ms)", h2_mean, h3_mean);
        println!("{:<24} | {:<16.2} | {:<16.2}", "Warm P50 (ms)", h2_p50, h3_p50);
        println!("{:<24} | {:<16.2} | {:<16.2}", "Warm P90 (ms)", h2_p90, h3_p90);
        println!("============================================================");
    }

    Ok(())
}

//! OBS-1 controlled sampling for the managed web search chain.
//!
//! Runs a bounded number of live searches through the keyless provider chain
//! (parallel -> you -> duckduckgo) and aggregates the structured
//! `managed_search` tracing events into a JSON summary.
//!
//! Manual run:
//!   cargo run -p flowy-web --example obs_sampling
//!
//! Optional env:
//!   OBS_QUERIES=<n>     number of searches (default 20, keep <= 30)
//!   OBS_LOG=<path>      JSONL capture file (default: temp dir)

use std::collections::BTreeMap;
use std::time::Instant;

use flowy_web::managed::{ManagedExtractMode, ManagedWebService};
use flowy_web::types::SearchQuery;
use serde_json::Value;

fn queries() -> Vec<&'static str> {
    vec![
        "深圳今天天气",
        "上海到北京高铁时刻表",
        "2026年诺贝尔物理学奖",
        "Rust 2024 edition release notes",
        "OpenAI GPT-5.6 capabilities",
        "特斯拉最新财报",
        "世界杯预选赛积分榜",
        "Nginx 502 Bad Gateway 排查",
        "macOS Sequoia 最新版本号",
        "Kubernetes 1.32 release notes",
    ]
}

fn percentile(values: &mut [u128], p: f64) -> u128 {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    let index = ((p / 100.0) * values.len() as f64).ceil() as usize;
    values[index.saturating_sub(1).min(values.len() - 1)]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let total: usize = std::env::var("OBS_QUERIES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(20);
    let log_path = std::env::var("OBS_LOG").unwrap_or_else(|_| {
        std::env::temp_dir()
            .join(format!(
                "obs-managed-search-{}.jsonl",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            ))
            .to_string_lossy()
            .into_owned()
    });
    let log_file = std::fs::File::create(&log_path)?;

    tracing_subscriber::fmt()
        .json()
        .with_env_filter("managed_search=info")
        .with_writer(move || log_file.try_clone().expect("clone log file"))
        .init();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let mut wall_ms = Vec::new();
    runtime.block_on(async {
        let service = ManagedWebService::keyless_default(ManagedExtractMode::Disabled)
            .map_err(|error| format!("managed web service init failed: {error}"))?;
        let provider = service.search_provider();
        let base = queries();

        for index in 0..total {
            let query = base[index % base.len()];
            let started = Instant::now();
            let outcome = provider
                .search(SearchQuery {
                    query: query.to_owned(),
                    count: 5,
                })
                .await;
            let elapsed = started.elapsed().as_millis();
            wall_ms.push(elapsed);
            match outcome {
                Ok(result) => println!(
                    "[{index:02}] ok provider={} hits={} wall_ms={elapsed}",
                    result.provider,
                    result.hits.len()
                ),
                Err(error) => println!("[{index:02}] failed wall_ms={elapsed}: {error}"),
            }
        }

        if let Err(error) = service.shutdown().await {
            eprintln!("shutdown warning: {error}");
        }
        Ok::<(), String>(())
    })
    .map_err(std::io::Error::other)?;

    summarize(&log_path, &mut wall_ms)?;
    Ok(())
}

fn summarize(path: &str, wall_ms: &mut Vec<u128>) -> Result<(), Box<dyn std::error::Error>> {
    #[derive(Default)]
    struct ProviderStats {
        ok: Vec<u128>,
        fail: BTreeMap<String, usize>,
    }

    let text = std::fs::read_to_string(path)?;
    let mut providers: BTreeMap<String, ProviderStats> = BTreeMap::new();
    let mut successes = 0usize;
    let mut all_failed = 0usize;
    let mut empty = 0usize;
    let mut skipped = 0usize;
    let mut fallback_hist: BTreeMap<u64, usize> = BTreeMap::new();
    let mut search_ids: BTreeMap<String, u128> = BTreeMap::new();

    for line in text.lines() {
        let Ok(row) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if row.get("target").and_then(Value::as_str) != Some("managed_search") {
            continue;
        }
        let fields = &row["fields"];
        let message = fields.get("message").and_then(Value::as_str).unwrap_or("");
        let provider = fields
            .get("provider")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        if provider.is_empty() {
            continue;
        }
        let elapsed = fields
            .get("elapsed_ms")
            .and_then(|value| {
                value
                    .as_u64()
                    .or_else(|| value.as_str().and_then(|raw| raw.parse().ok()))
            })
            .unwrap_or(0) as u128;
        let request_id = fields
            .get("request_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        if request_id.is_empty() {
            continue;
        }
        let serial = search_ids.entry(request_id).or_default();

        let entry = providers.entry(provider.clone()).or_default();
        match message {
            "managed web search succeeded" => {
                successes += 1;
                entry.ok.push(elapsed);
                let fallback = fields.get("fallback_count").and_then(Value::as_u64).unwrap_or(0);
                *fallback_hist.entry(fallback).or_default() += 1;
                *serial = serial.saturating_add(elapsed);
            }
            "managed web search provider failed" => {
                let class = fields
                    .get("error_class")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_owned();
                *entry.fail.entry(class).or_default() += 1;
                *serial = serial.saturating_add(elapsed);
            }
            "managed web search provider skipped" => {
                skipped += 1;
                *entry.fail.entry("skipped".to_owned()).or_default() += 1;
            }
            "all managed web search providers were unavailable" => {
                all_failed += 1;
            }
            "managed web search completed with no results" => {
                empty += 1;
            }
            _ => {}
        }
    }

    let mut p50 = Vec::new();
    let mut p95 = Vec::new();
    for stats in providers.values() {
        let mut ok = stats.ok.clone();
        if !ok.is_empty() {
            p50.push(percentile(&mut ok, 50.0));
            p95.push(percentile(&mut ok, 95.0));
        }
    }
    let serial_waits: Vec<u128> = search_ids.values().copied().collect();

    println!("\n== OBS-1 managed search summary ==");
    println!(
        "searches: ok={successes} all_failed={all_failed} empty={empty} skipped={skipped}"
    );
    println!(
        "wall_ms p50={} p95={}",
        percentile(wall_ms, 50.0),
        percentile(wall_ms, 95.0)
    );
    println!(
        "provider-attempt elapsed p50={} p95={} (across providers)",
        percentile(&mut p50, 50.0),
        percentile(&mut p95, 50.0)
    );
    println!("fallback_count histogram: {fallback_hist:?}");
    println!("attempt-time sums (serial wait proxy): {serial_waits:?}");
    for (provider, stats) in &providers {
        println!(
            "{provider}: ok={} fails={:?}",
            stats.ok.len(),
            stats.fail
        );
    }
    println!("raw capture: {path}");
    Ok(())
}

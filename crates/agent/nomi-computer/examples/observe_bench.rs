//! Local observe-path benchmark.
//!
//! Run:
//!   cargo run -p nomi-computer --example observe_bench
//!   cargo run -p nomi-computer --example observe_bench -- 3 123456
//!
//! Arguments: `[iterations] [window_id]`. The benchmark prints only timing and
//! observation metadata; screenshots and UI text stay local and are not dumped.

use std::time::{Duration, Instant};

use nomi_computer::{ComputerTool, fallback_backend};
use nomi_config::config::ComputerConfig;
use nomi_tools::Tool;
use serde_json::json;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let iterations = args
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(3)
        .clamp(1, 10);
    let requested_window_id = args.next().and_then(|value| value.parse::<u64>().ok());
    let selected_window = if requested_window_id.is_none() {
        fallback_backend::foreground_window().ok().flatten()
    } else {
        None
    };
    let window_id = requested_window_id.or_else(|| selected_window.as_ref().map(|window| u64::from(window.id)));
    if let Some(window) = &selected_window {
        println!(
            "target window: id={} app={:?} at ({}, {}) size {}x{}",
            window.id, window.app_name, window.x, window.y, window.width, window.height
        );
    }
    let tool = ComputerTool::new(&ComputerConfig::default());

    println!(
        "observe benchmark: iterations={iterations} window_id={:?}",
        window_id
    );
    for mode in ["semantic", "auto", "visual"] {
        let mut samples = Vec::with_capacity(iterations);
        for index in 0..iterations {
            let mut input = json!({ "action": "observe", "mode": mode });
            if let Some(window_id) = window_id {
                input["window_id"] = json!(window_id);
            }
            let started = Instant::now();
            let result = tool.execute(input).await;
            let elapsed = started.elapsed();
            samples.push(elapsed);
            let summary = result.content.lines().next().unwrap_or_default();
            println!(
                "  mode={mode:<8} run={} time_ms={:.1} images={} error={} summary={summary}",
                index + 1,
                elapsed.as_secs_f64() * 1_000.0,
                result.images.len(),
                result.is_error
            );
            if result.is_error {
                break;
            }
        }
        samples.sort_unstable();
        if !samples.is_empty() {
            let p50 = percentile(&samples, 50);
            let p95 = percentile(&samples, 95);
            println!(
                "  mode={mode:<8} p50_ms={:.1} p95_ms={:.1}",
                p50.as_secs_f64() * 1_000.0,
                p95.as_secs_f64() * 1_000.0
            );
        }
    }
}

fn percentile(samples: &[Duration], percentile: usize) -> Duration {
    let index = ((samples.len() - 1) * percentile).div_ceil(100);
    samples[index.min(samples.len() - 1)]
}

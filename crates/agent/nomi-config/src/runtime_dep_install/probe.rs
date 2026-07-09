use std::time::{Duration, Instant};

use futures::future::join_all;
use reqwest::Client;
use tokio::time::timeout;
use tracing::debug;

pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Parallel reachability probe; returns the index of the fastest responding URL.
pub async fn pick_fastest_url(client: &Client, urls: &[&str]) -> Option<usize> {
    if urls.is_empty() {
        return None;
    }
    if urls.len() == 1 {
        return Some(0);
    }

    let probes = urls.iter().enumerate().map(|(idx, url)| {
        let client = client.clone();
        let url = (*url).to_string();
        async move {
            let elapsed = probe_url_latency(&client, &url).await?;
            Some((idx, elapsed))
        }
    });

    let mut winners: Vec<(usize, Duration)> =
        join_all(probes).await.into_iter().flatten().collect();

    winners.sort_by_key(|(_, elapsed)| *elapsed);
    winners.first().map(|(idx, elapsed)| {
        debug!(
            index = idx,
            ms = elapsed.as_millis(),
            "selected fastest mirror"
        );
        *idx
    })
}

async fn probe_url_latency(client: &Client, url: &str) -> Option<Duration> {
    let started = Instant::now();
    let response = timeout(
        PROBE_TIMEOUT,
        client
            .get(url)
            .header("User-Agent", "allo/dep-probe")
            .header("Range", "bytes=0-0")
            .send(),
    )
    .await
    .ok()?
    .ok()?;

    let status = response.status();
    if status.is_success()
        || status == reqwest::StatusCode::PARTIAL_CONTENT
        || status == reqwest::StatusCode::FOUND
        || status == reqwest::StatusCode::MOVED_PERMANENTLY
        || status == reqwest::StatusCode::TEMPORARY_REDIRECT
        || status == reqwest::StatusCode::PERMANENT_REDIRECT
    {
        Some(started.elapsed())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pick_fastest_url_empty_returns_none() {
        let client = Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .expect("client");
        assert!(pick_fastest_url(&client, &[]).await.is_none());
    }
}

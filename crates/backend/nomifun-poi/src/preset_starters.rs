//! Remote curated starter topics for users with no local POI yet.
//!
//! Fetches `GET {server.base_url}/recommendedTopics` (no auth).

use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use nomi_config::DEFAULT_WECHAT_FLOWY_SERVER_BASE;
use nomifun_api_types::PoiStarterResponse;
use nomifun_common::AppError;
use serde::Deserialize;
use tracing::{debug, info, warn};

/// Relative path under the Flowy `/claw` API root.
pub const RECOMMENDED_TOPICS_PATH: &str = "recommendedTopics";

const FETCH_TIMEOUT: Duration = Duration::from_secs(15);
const CACHE_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone)]
pub struct RankedStarter {
    pub sort: i32,
    pub starter: PoiStarterResponse,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    fetched_at: Instant,
    /// Pre-sorted by `sort` descending.
    items: Vec<RankedStarter>,
}

static PRESET_CACHE: OnceLock<Mutex<Option<CacheEntry>>> = OnceLock::new();

#[derive(Debug, Deserialize)]
struct RecommendedTopicsEnvelope {
    code: i64,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    data: Vec<RecommendedTopicItem>,
}

#[derive(Debug, Deserialize)]
struct RecommendedTopicItem {
    id: serde_json::Value,
    content: String,
    #[serde(default)]
    sort: i32,
}

/// Resolve Flowy API root: configured `server.base_url`, else domestic default.
pub fn resolve_server_base_url(configured: &str) -> String {
    let trimmed = configured.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        DEFAULT_WECHAT_FLOWY_SERVER_BASE
            .trim_end_matches('/')
            .to_string()
    } else {
        trimmed.to_string()
    }
}

fn cache_slot() -> &'static Mutex<Option<CacheEntry>> {
    PRESET_CACHE.get_or_init(|| Mutex::new(None))
}

/// Fetch curated conversation starters from the remote service.
///
/// Auth is not required. Results are cached briefly so Guid “换一批” only
/// reshuffles locally instead of re-hitting the network every time.
pub async fn fetch_preset_starters(
    server_base_url: &str,
    locale: &str,
) -> Result<Vec<RankedStarter>, AppError> {
    let locale = if locale.trim().is_empty() {
        "zh-CN"
    } else {
        locale.trim()
    };
    let base = resolve_server_base_url(server_base_url);

    if let Some(cached) = read_fresh_cache() {
        debug!(
            count = cached.len(),
            locale, "poi preset starters: serving from cache"
        );
        return Ok(apply_locale(cached, locale));
    }

    let url = format!("{base}/{RECOMMENDED_TOPICS_PATH}");
    info!(%url, locale, "poi preset starters: fetching recommendedTopics");

    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .build()
        .map_err(|e| AppError::Internal(format!("poi preset http client: {e}")))?;

    let resp = client
        .get(&url)
        .header(
            reqwest::header::USER_AGENT,
            format!("nomifun/{}", env!("CARGO_PKG_VERSION")),
        )
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("poi preset fetch failed: {e}")))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| AppError::Internal(format!("poi preset read body: {e}")))?;

    if !status.is_success() {
        return Err(AppError::Internal(format!(
            "poi preset HTTP {status}: {}",
            truncate(&body, 200)
        )));
    }

    let envelope: RecommendedTopicsEnvelope = serde_json::from_str(&body).map_err(|e| {
        AppError::Internal(format!(
            "poi preset parse failed: {e}; body={}",
            truncate(&body, 200)
        ))
    })?;

    if envelope.code != 200 {
        return Err(AppError::Internal(format!(
            "poi preset API code={}: {}",
            envelope.code,
            if envelope.msg.is_empty() {
                "query failed"
            } else {
                &envelope.msg
            }
        )));
    }

    let mut items = map_recommended_topics(envelope.data, locale);
    // Higher `sort` first (API contract).
    items.sort_by(|a, b| b.sort.cmp(&a.sort).then_with(|| a.starter.id.cmp(&b.starter.id)));

    write_cache(items.clone());
    info!(count = items.len(), "poi preset starters: fetched");
    Ok(items)
}

fn map_recommended_topics(raw: Vec<RecommendedTopicItem>, locale: &str) -> Vec<RankedStarter> {
    let mut out = Vec::with_capacity(raw.len());
    for item in raw {
        let content = item.content.trim();
        if content.is_empty() {
            continue;
        }
        let id_str = json_id_to_string(&item.id);
        if id_str.is_empty() {
            continue;
        }
        out.push(RankedStarter {
            sort: item.sort,
            starter: PoiStarterResponse {
                id: format!("preset:{id_str}"),
                topic_id: format!("preset:{id_str}"),
                topic_label: String::new(),
                text: content.to_string(),
                locale: locale.to_string(),
                source: "preset".to_string(),
            },
        });
    }
    out
}

fn json_id_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.trim().to_string(),
        _ => String::new(),
    }
}

fn apply_locale(mut items: Vec<RankedStarter>, locale: &str) -> Vec<RankedStarter> {
    for item in &mut items {
        item.starter.locale = locale.to_string();
    }
    items
}

fn read_fresh_cache() -> Option<Vec<RankedStarter>> {
    let guard = cache_slot().lock().ok()?;
    let entry = guard.as_ref()?;
    if entry.fetched_at.elapsed() > CACHE_TTL {
        return None;
    }
    Some(entry.items.clone())
}

fn write_cache(items: Vec<RankedStarter>) {
    match cache_slot().lock() {
        Ok(mut guard) => {
            *guard = Some(CacheEntry {
                fetched_at: Instant::now(),
                items,
            });
        }
        Err(err) => warn!(error = %err, "poi preset cache lock poisoned"),
    }
}

/// Page recommended topics: primary order by `sort` DESC; when `seed != 0`,
/// reshape within the same `sort` bucket for “换一批”.
pub fn page_preset_starters(
    mut items: Vec<RankedStarter>,
    limit: usize,
    offset: usize,
    seed: u64,
) -> (Vec<PoiStarterResponse>, usize) {
    let total = items.len();
    if seed != 0 {
        items.sort_by(|a, b| {
            b.sort.cmp(&a.sort).then_with(|| {
                let ha = shuffle_key(seed, &a.starter.id);
                let hb = shuffle_key(seed, &b.starter.id);
                ha.cmp(&hb).then_with(|| a.starter.id.cmp(&b.starter.id))
            })
        });
    } else {
        items.sort_by(|a, b| b.sort.cmp(&a.sort).then_with(|| a.starter.id.cmp(&b.starter.id)));
    }
    let page = items
        .into_iter()
        .skip(offset)
        .take(limit.max(1))
        .map(|r| r.starter)
        .collect();
    (page, total)
}

fn shuffle_key(seed: u64, id: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    seed.hash(&mut hasher);
    id.hash(&mut hasher);
    hasher.finish()
}

fn truncate(s: &str, max_chars: usize) -> String {
    let mut out: String = s.chars().take(max_chars).collect();
    if s.chars().count() > max_chars {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_falls_back_to_default_when_empty() {
        let url = resolve_server_base_url("  ");
        assert!(url.contains("flowyaipc"));
        assert!(!url.ends_with('/'));
    }

    #[test]
    fn map_and_page_prefer_higher_sort() {
        let raw = vec![
            RecommendedTopicItem {
                id: serde_json::json!(2),
                content: "低权重".into(),
                sort: 10,
            },
            RecommendedTopicItem {
                id: serde_json::json!(1),
                content: "高权重".into(),
                sort: 100,
            },
            RecommendedTopicItem {
                id: serde_json::json!(3),
                content: "中权重".into(),
                sort: 50,
            },
        ];
        let items = map_recommended_topics(raw, "zh-CN");
        let (page, total) = page_preset_starters(items, 10, 0, 0);
        assert_eq!(total, 3);
        assert_eq!(page[0].text, "高权重");
        assert_eq!(page[1].text, "中权重");
        assert_eq!(page[2].text, "低权重");
        assert!(page[0].id.starts_with("preset:"));
    }

    #[test]
    fn seed_reshuffles_within_same_sort_only() {
        let items = vec![
            RankedStarter {
                sort: 100,
                starter: PoiStarterResponse {
                    id: "preset:a".into(),
                    topic_id: "preset:a".into(),
                    topic_label: String::new(),
                    text: "A".into(),
                    locale: "zh-CN".into(),
                    source: "preset".into(),
                },
            },
            RankedStarter {
                sort: 100,
                starter: PoiStarterResponse {
                    id: "preset:b".into(),
                    topic_id: "preset:b".into(),
                    topic_label: String::new(),
                    text: "B".into(),
                    locale: "zh-CN".into(),
                    source: "preset".into(),
                },
            },
            RankedStarter {
                sort: 1,
                starter: PoiStarterResponse {
                    id: "preset:low".into(),
                    topic_id: "preset:low".into(),
                    topic_label: String::new(),
                    text: "LOW".into(),
                    locale: "zh-CN".into(),
                    source: "preset".into(),
                },
            },
        ];
        let (page, _) = page_preset_starters(items, 10, 0, 42);
        assert_ne!(page[2].text, "A");
        assert_ne!(page[2].text, "B");
        assert_eq!(page[2].text, "LOW");
    }

    #[test]
    fn parse_envelope_sample() {
        let body = r#"{"code":200,"msg":"Success","data":[{"id":1,"content":"帮我写一份周报提纲","sort":100}]}"#;
        let env: RecommendedTopicsEnvelope = serde_json::from_str(body).unwrap();
        assert_eq!(env.code, 200);
        assert_eq!(env.data.len(), 1);
        assert_eq!(env.data[0].sort, 100);
    }
}

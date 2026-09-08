//! SkillHub-only ordinary Skill market adapter.
//!
//! The mixed-source market endpoints are still used by the independent MCP,
//! plugin, and expert-package surfaces.  Ordinary Skills use this module so
//! that their identity, filtering, pagination, and installation contract is
//! owned by one provider instead of being inferred from scraped pages.

use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, FixedOffset};
use nomifun_api_types::{
    SkillHubMarketCategoriesResponse, SkillHubMarketCategoryItem, SkillHubMarketContentSource,
    SkillHubMarketItem, SkillHubMarketQueryRequest, SkillHubMarketQueryResponse, SkillHubMarketSort,
    SkillHubMarketSource, SkillHubMarketSubCategory,
};
use nomifun_common::AppError;
use reqwest::Url;
use serde_json::Value;

use super::client::{
    MARKET_REQUEST_TIMEOUT, build_market_client, read_market_response,
    send_skillhub_get_with_retry,
};
use super::parse::{clean_market_text, is_market_slug, json_string_array, market_https_image_url, title_from_slug};

const SKILLHUB_SKILLS_URL: &str = "https://api.skillhub.cn/api/skills";
const SKILLHUB_CATEGORIES_URL: &str = "https://api.skillhub.cn/api/v1/categories";
const SKILLHUB_WEB_SKILLS_URL: &str = "https://skillhub.cn/skills";
const SKILLHUB_MARKET_DEADLINE: Duration = Duration::from_secs(30);
const DEFAULT_PAGE_SIZE: u32 = 20;
const MAX_PAGE_SIZE: u32 = 100;
const MAX_KEYWORD_CHARS: usize = 80;
const MAX_CATEGORY_CHARS: usize = 96;
const MAX_MARKET_TEXT_CHARS: usize = 8 * 1024;

/// Error boundary for the dedicated SkillHub market endpoints. The shared
/// [`AppError::Timeout`] intentionally keeps the application's historical
/// 502 mapping; the outer SkillHub deadline must instead be distinguishable
/// so its route can return the plan's required 504 without changing unrelated
/// endpoints.
#[derive(Debug, thiserror::Error)]
pub enum SkillHubMarketError {
    #[error("SkillHub market request timed out")]
    Deadline,
    #[error(transparent)]
    App(#[from] AppError),
}

/// Query the ordinary SkillHub catalog with a single end-to-end deadline.
pub async fn query_skillhub_market(
    request: SkillHubMarketQueryRequest,
) -> Result<SkillHubMarketQueryResponse, SkillHubMarketError> {
    let normalized = normalize_query(request)?;
    let client = build_market_client()?;
    tokio::time::timeout(
        SKILLHUB_MARKET_DEADLINE,
        query_skillhub_market_with_client(&client, normalized),
    )
    .await
    .map_err(|_| SkillHubMarketError::Deadline)?
}

/// Load the runtime SkillHub first-level category dictionary.
pub async fn list_skillhub_market_categories() -> Result<SkillHubMarketCategoriesResponse, SkillHubMarketError> {
    let client = build_market_client()?;
    tokio::time::timeout(
        SKILLHUB_MARKET_DEADLINE,
        list_skillhub_market_categories_with_client(&client),
    )
    .await
    .map_err(|_| SkillHubMarketError::Deadline)?
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedQuery {
    source: Option<SkillHubMarketSource>,
    keyword: Option<String>,
    category: Option<String>,
    requires_api_key: Option<bool>,
    sort_by: SkillHubMarketSort,
    page: u32,
    page_size: u32,
}

fn normalize_query(request: SkillHubMarketQueryRequest) -> Result<NormalizedQuery, AppError> {
    let keyword = normalize_optional_text(request.keyword, MAX_KEYWORD_CHARS, "keyword")?;
    let category = normalize_optional_text(request.category, MAX_CATEGORY_CHARS, "category")?;
    if let Some(category) = &category
        && !is_market_slug(category)
    {
        return Err(AppError::BadRequest("category is invalid".into()));
    }

    Ok(NormalizedQuery {
        source: request.source,
        keyword,
        category,
        requires_api_key: request.requires_api_key,
        sort_by: request.sort_by.unwrap_or_default(),
        page: request.page.unwrap_or(1).max(1),
        page_size: request
            .page_size
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE),
    })
}

fn normalize_optional_text(
    value: Option<String>,
    max_chars: usize,
    field: &str,
) -> Result<Option<String>, AppError> {
    let Some(value) = value else { return Ok(None) };
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Ok(None);
    }
    if value.chars().count() > max_chars {
        return Err(AppError::BadRequest(format!(
            "{field} must be at most {max_chars} characters"
        )));
    }
    Ok(Some(value))
}

async fn query_skillhub_market_with_client(
    client: &reqwest::Client,
    query: NormalizedQuery,
) -> Result<SkillHubMarketQueryResponse, SkillHubMarketError> {
    let url = skillhub_query_url(&query)?;
    let root = fetch_json(client, url, "SkillHub skill list").await?;
    Ok(parse_skillhub_query_response(
        &root,
        query.page,
        query.page_size,
    )?)
}

async fn list_skillhub_market_categories_with_client(
    client: &reqwest::Client,
) -> Result<SkillHubMarketCategoriesResponse, SkillHubMarketError> {
    let root = fetch_json(
        client,
        Url::parse(SKILLHUB_CATEGORIES_URL).map_err(|_| AppError::Internal("invalid SkillHub categories URL".into()))?,
        "SkillHub categories",
    )
    .await?;
    Ok(parse_skillhub_categories_response(&root)?)
}

async fn fetch_json(
    client: &reqwest::Client,
    url: Url,
    label: &str,
) -> Result<Value, AppError> {
    let mut response = send_skillhub_get_with_retry(client, url, "application/json", MARKET_REQUEST_TIMEOUT)
        .await
        ?;
    if !response.status().is_success() {
        return Err(AppError::BadGateway(format!("{label} returned {}", response.status())));
    }
    let body = read_market_response(&mut response).await?;
    serde_json::from_str(&body).map_err(|_| AppError::BadGateway(format!("{label} returned invalid JSON")))
}

fn skillhub_query_url(query: &NormalizedQuery) -> Result<Url, AppError> {
    let mut url = Url::parse(SKILLHUB_SKILLS_URL)
        .map_err(|_| AppError::Internal("invalid SkillHub skills URL".into()))?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs
            .append_pair("page", &query.page.to_string())
            .append_pair("pageSize", &query.page_size.to_string())
            .append_pair("sortBy", skillhub_sort_name(query.sort_by))
            .append_pair("order", "desc");
        if let Some(source) = query.source {
            pairs.append_pair("source", skillhub_source_name(source));
        }
        if let Some(keyword) = &query.keyword {
            pairs.append_pair("keyword", keyword);
        }
        if let Some(category) = &query.category {
            pairs.append_pair("category", category);
        }
        if let Some(requires_api_key) = query.requires_api_key {
            pairs.append_pair(
                "labels",
                if requires_api_key {
                    "requires_api_key:true"
                } else {
                    "requires_api_key:false"
                },
            );
        }
    }
    Ok(url)
}

fn skillhub_sort_name(sort_by: SkillHubMarketSort) -> &'static str {
    match sort_by {
        SkillHubMarketSort::Score => "score",
        SkillHubMarketSort::Downloads => "downloads",
        SkillHubMarketSort::UpdatedAt => "updated_at",
    }
}

fn skillhub_source_name(source: SkillHubMarketSource) -> &'static str {
    match source {
        SkillHubMarketSource::Skillhub => "community",
        SkillHubMarketSource::Clawhub => "clawhub",
    }
}

fn parse_skillhub_query_response(
    root: &Value,
    page: u32,
    page_size: u32,
) -> Result<SkillHubMarketQueryResponse, AppError> {
    require_success_code(root, "SkillHub skill list")?;
    let data = root
        .get("data")
        .ok_or_else(|| AppError::BadGateway("SkillHub skill list response has no data".into()))?;
    let total = json_u64(data.get("total"))
        .ok_or_else(|| AppError::BadGateway("SkillHub skill list response has no total".into()))?;
    let raw_items = data
        .get("skills")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::BadGateway("SkillHub skill list response has no skills array".into()))?;

    let mut dropped = 0_usize;
    let mut seen = HashSet::new();
    let mut items = Vec::with_capacity(raw_items.len());
    for raw in raw_items {
        let Some(mut item) = parse_skillhub_item(raw) else {
            dropped += 1;
            continue;
        };
        if !seen.insert(item.id.clone()) {
            dropped += 1;
            continue;
        }
        item.rank = page
            .saturating_sub(1)
            .saturating_mul(page_size)
            .saturating_add(items.len() as u32)
            .saturating_add(1) as usize;
        items.push(item);
    }
    if dropped > 0 {
        tracing::warn!(dropped, "discarded malformed or duplicate SkillHub market items");
    }
    if !raw_items.is_empty() && items.is_empty() {
        return Err(AppError::BadGateway("SkillHub skill list items failed schema validation".into()));
    }

    Ok(SkillHubMarketQueryResponse {
        fetched_at: now_epoch_ms(),
        total,
        page,
        page_size,
        items,
    })
}

fn parse_skillhub_item(raw: &Value) -> Option<SkillHubMarketItem> {
    // `ownerName` is the account owner returned by the upstream API. Public
    // SkillHub URLs use the namespace handle when one is present (enterprise
    // entries commonly have different account and namespace identities).
    let owner = match raw.get("namespace") {
        None | Some(Value::Null) => json_text(raw, "ownerName", 96)?,
        Some(namespace) => json_text(namespace, "handle", 96)?,
    };
    let slug = json_text(raw, "slug", 96)?;
    if !is_market_slug(&owner) || !is_market_slug(&slug) {
        return None;
    }
    let version = json_text(raw, "version", 96)?;
    let upstream_source = json_text(raw, "source", 64);
    let market_source = match upstream_source.as_deref() {
        Some(value) if value.eq_ignore_ascii_case("clawhub") => SkillHubMarketContentSource::Clawhub,
        Some(value)
            if value.eq_ignore_ascii_case("community")
                || value.eq_ignore_ascii_case("enterprise") =>
        {
            SkillHubMarketContentSource::Skillhub
        }
        _ => SkillHubMarketContentSource::Unknown,
    };
    let name = json_text(raw, "name", 160).unwrap_or_else(|| title_from_slug(&slug));
    let description = json_text(raw, "description_zh", MAX_MARKET_TEXT_CHARS)
        .or_else(|| json_text(raw, "description", MAX_MARKET_TEXT_CHARS))
        .unwrap_or_default();
    let category = json_text(raw, "category", MAX_CATEGORY_CHARS);
    let tags = json_string_array(raw.get("tags"), 96);
    let sub_categories = raw
        .get("subCategories")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(parse_sub_category)
        .collect();
    let requires_api_key = parse_requires_api_key(raw.get("labels"));
    let downloads = json_u64(raw.get("downloads")).unwrap_or(0);
    let installs = json_u64(raw.get("installs")).unwrap_or(0);
    let stars = json_u64(raw.get("stars")).unwrap_or(0);
    let score = json_f64(raw.get("score")).unwrap_or(0.0);
    if !score.is_finite() || score < 0.0 {
        return None;
    }
    let id = format!("skillhub:{owner}/skills/{slug}");
    let url = format!("{SKILLHUB_WEB_SKILLS_URL}/{owner}/{slug}");
    let avatar = raw
        .get("iconUrl")
        .and_then(Value::as_str)
        .and_then(market_https_image_url);

    Some(SkillHubMarketItem {
        id,
        owner,
        slug,
        market_source,
        upstream_source,
        rank: 0,
        name,
        description,
        version,
        category,
        tags,
        sub_categories,
        requires_api_key,
        downloads,
        installs,
        stars,
        score,
        created_at: json_timestamp(raw.get("created_at")),
        updated_at: json_timestamp(raw.get("updated_at")),
        url,
        avatar,
    })
}

fn parse_sub_category(value: &Value) -> Option<SkillHubMarketSubCategory> {
    let key = json_text(value, "key", MAX_CATEGORY_CHARS)?;
    let name = json_text(value, "name", 160)?;
    if !is_market_slug(&key) {
        return None;
    }
    Some(SkillHubMarketSubCategory { key, name })
}

fn parse_requires_api_key(value: Option<&Value>) -> Option<bool> {
    let value = value?.get("requires_api_key")?;
    match value {
        Value::Bool(value) => Some(*value),
        Value::String(value) if value.eq_ignore_ascii_case("true") => Some(true),
        Value::String(value) if value.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}

fn parse_skillhub_categories_response(
    root: &Value,
) -> Result<SkillHubMarketCategoriesResponse, AppError> {
    require_success_code(root, "SkillHub categories")?;
    let data = root.get("data").unwrap_or(root);
    let raw_items = data
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::BadGateway("SkillHub categories response has no items array".into()))?;
    let mut dropped = 0_usize;
    let mut parsed = 0_usize;
    let mut seen = HashSet::new();
    let mut items = Vec::with_capacity(raw_items.len());
    for raw in raw_items {
        let Some(item) = parse_category(raw) else {
            dropped += 1;
            continue;
        };
        parsed += 1;
        if item.level != 1 || !item.active || !seen.insert(item.key.clone()) {
            continue;
        }
        items.push(SkillHubMarketCategoryItem {
            key: item.key,
            name: item.name,
            name_en: item.name_en,
            sort_order: item.sort_order,
        });
    }
    if dropped > 0 {
        tracing::warn!(dropped, "discarded malformed SkillHub categories");
    }
    // A non-empty response containing only valid child/inactive categories is
    // a legitimate filtered result. Only fail closed when every raw item
    // failed schema parsing, otherwise the UI would show a network error for
    // a valid-but-empty runtime dictionary.
    if !raw_items.is_empty() && parsed == 0 {
        return Err(AppError::BadGateway("SkillHub categories failed schema validation".into()));
    }
    items.sort_by_key(|item| item.sort_order);
    Ok(SkillHubMarketCategoriesResponse {
        fetched_at: now_epoch_ms(),
        items,
    })
}

struct RawCategory {
    key: String,
    name: String,
    name_en: String,
    level: i64,
    sort_order: i32,
    active: bool,
}

fn parse_category(value: &Value) -> Option<RawCategory> {
    let key = json_text(value, "key", MAX_CATEGORY_CHARS)?;
    let name = json_text(value, "name", 160)?;
    let name_en = json_text(value, "nameEn", 160)?;
    if !is_market_slug(&key) {
        return None;
    }
    Some(RawCategory {
        key,
        name,
        name_en,
        level: value.get("level")?.as_i64()?,
        sort_order: value.get("sortOrder")?.as_i64()?.try_into().ok()?,
        active: value.get("active")?.as_bool()?,
    })
}

fn require_success_code(root: &Value, label: &str) -> Result<(), AppError> {
    if let Some(code) = root.get("code") {
        let success = code.as_i64().is_some_and(|code| code == 0)
            || code.as_str().is_some_and(|code| code == "0");
        if !success {
            return Err(AppError::BadGateway(format!("{label} upstream business code is not zero")));
        }
    }
    Ok(())
}

fn json_text(value: &Value, key: &str, max_chars: usize) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(|value| clean_market_text(value, max_chars))
        .filter(|value| !value.is_empty())
}

fn json_u64(value: Option<&Value>) -> Option<u64> {
    let value = value?;
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|value| u64::try_from(value).ok()))
        .or_else(|| value.as_f64().filter(|value| value.is_finite() && *value >= 0.0).map(|value| value as u64))
        .or_else(|| value.as_str()?.trim().parse::<u64>().ok())
}

fn json_f64(value: Option<&Value>) -> Option<f64> {
    let value = value?;
    value
        .as_f64()
        .or_else(|| value.as_u64().map(|value| value as f64))
        .or_else(|| value.as_str()?.trim().parse::<f64>().ok())
}

fn json_timestamp(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    if let Some(timestamp) = value.as_i64() {
        return Some(timestamp);
    }
    if let Some(timestamp) = value.as_u64() {
        return i64::try_from(timestamp).ok();
    }
    let text = value.as_str()?.trim();
    if let Ok(timestamp) = text.parse::<i64>() {
        return Some(timestamp);
    }
    DateTime::<FixedOffset>::parse_from_rfc3339(text)
        .ok()
        .map(|timestamp| timestamp.timestamp_millis())
}

fn now_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(value: serde_json::Value) -> SkillHubMarketQueryResponse {
        parse_skillhub_query_response(&value, 1, 20).unwrap()
    }

    #[test]
    fn normalizes_filters_and_builds_official_query_contract() {
        let normalized = normalize_query(SkillHubMarketQueryRequest {
            source: Some(SkillHubMarketSource::Clawhub),
            keyword: Some("  pdf  ".into()),
            category: Some("  content-creation ".into()),
            requires_api_key: Some(false),
            sort_by: Some(SkillHubMarketSort::Downloads),
            page: Some(0),
            page_size: Some(1000),
        })
        .unwrap();
        assert_eq!(normalized.page, 1);
        assert_eq!(normalized.page_size, 100);
        let url = skillhub_query_url(&normalized).unwrap();
        assert_eq!(url.path(), "/api/skills");
        let pairs = url.query_pairs().collect::<Vec<_>>();
        assert!(pairs.contains(&("sortBy".into(), "downloads".into())));
        assert!(pairs.contains(&("order".into(), "desc".into())));
        assert!(pairs.contains(&("labels".into(), "requires_api_key:false".into())));
        assert!(pairs.contains(&("source".into(), "clawhub".into())));
    }

    #[test]
    fn rejects_overlong_keyword_and_invalid_category() {
        assert!(normalize_query(SkillHubMarketQueryRequest {
            keyword: Some("x".repeat(MAX_KEYWORD_CHARS + 1)),
            ..Default::default()
        })
        .is_err());
        assert!(normalize_query(SkillHubMarketQueryRequest {
            category: Some("../private".into()),
            ..Default::default()
        })
        .is_err());
    }

    #[test]
    fn parses_owner_name_and_preserves_structured_metadata() {
        let response = query(serde_json::json!({
            "code": 0,
            "data": {
                "total": 1,
                "skills": [{
                    "slug": "pdf-tools",
                    "ownerName": "u_d95b6787",
                    "namespace": {"handle": "tencent-adm"},
                    "name": "PDF tools",
                    "description": "English",
                    "description_zh": "中文描述",
                    "version": "1.2.3",
                    "source": "enterprise",
                    "category": "office-efficiency",
                    "tags": ["pdf", "latest"],
                    "subCategories": [{"key": "document", "name": "文档"}],
                    "labels": {"requires_api_key": "false"},
                    "downloads": 12,
                    "installs": 8,
                    "stars": 3,
                    "score": 99.5,
                    "created_at": 1742000000000_i64,
                    "updated_at": "2025-03-15T00:00:00Z",
                    "iconUrl": "https://evil.example/icon.png"
                }]
            }
        }));
        let item = &response.items[0];
        assert_eq!(item.owner, "tencent-adm");
        assert_eq!(item.id, "skillhub:tencent-adm/skills/pdf-tools");
        assert_eq!(item.url, "https://skillhub.cn/skills/tencent-adm/pdf-tools");
        assert_eq!(item.description, "中文描述");
        assert_eq!(item.sub_categories[0].key, "document");
        assert_eq!(item.requires_api_key, Some(false));
        assert_eq!(item.updated_at, Some(1741996800000));
        assert_eq!(item.avatar, None);
        assert_eq!(item.market_source, SkillHubMarketContentSource::Skillhub);
        assert_eq!(item.upstream_source.as_deref(), Some("enterprise"));
    }

    #[test]
    fn rejects_present_namespace_without_a_public_handle() {
        let value = serde_json::json!({
            "code": 0,
            "data": {
                "total": 1,
                "skills": [{
                    "slug": "pdf-tools",
                    "ownerName": "u_d95b6787",
                    "namespace": {"displayName": "Tencent ADM"},
                    "version": "1.0.0"
                }]
            }
        });

        assert!(parse_skillhub_query_response(&value, 1, 20).is_err());
    }

    #[test]
    fn maps_clawhub_and_unknown_sources_without_filtering_skillhub_enterprise() {
        let response = query(serde_json::json!({
            "code": 0,
            "data": {"total": 3, "skills": [
                {"slug": "community", "ownerName": "owner", "version": "1.0.0", "source": "community"},
                {"slug": "claw", "ownerName": "owner", "version": "1.0.0", "source": "clawhub"},
                {"slug": "future", "ownerName": "owner", "version": "1.0.0", "source": "future-provider"}
            ]}
        }));
        assert_eq!(response.items[0].market_source, SkillHubMarketContentSource::Skillhub);
        assert_eq!(response.items[1].market_source, SkillHubMarketContentSource::Clawhub);
        assert_eq!(response.items[2].market_source, SkillHubMarketContentSource::Unknown);
        assert_eq!(response.items[2].upstream_source.as_deref(), Some("future-provider"));
    }

    #[test]
    fn unknown_api_key_label_stays_unknown_and_empty_is_real_empty() {
        let response = query(serde_json::json!({
            "code": 0,
            "data": {"total": 0, "skills": []}
        }));
        assert!(response.items.is_empty());

        let response = query(serde_json::json!({
            "code": 0,
            "data": {"total": 1, "skills": [{
                "slug": "demo", "ownerName": "alice", "version": "1.0.0",
                "labels": {"requires_api_key": "maybe"}
            }]}
        }));
        assert_eq!(response.items[0].requires_api_key, None);
    }

    #[test]
    fn serializes_empty_metadata_arrays_for_frontend_contract() {
        let response = query(serde_json::json!({
            "code": 0,
            "data": {"total": 1, "skills": [{
                "slug": "demo", "ownerName": "alice", "version": "1.0.0"
            }]}
        }));
        let item = serde_json::to_value(&response.items[0]).unwrap();
        assert_eq!(item.get("tags"), Some(&serde_json::json!([])));
        assert_eq!(item.get("sub_categories"), Some(&serde_json::json!([])));
    }

    #[test]
    fn drops_partial_corruption_but_fails_when_every_item_is_invalid() {
        let value = serde_json::json!({
            "code": 0,
            "data": {"total": 2, "skills": [
                {"slug": "valid", "ownerName": "alice", "version": "1.0.0"},
                {"slug": "../invalid", "ownerName": "alice", "version": "1.0.0"}
            ]}
        });
        assert_eq!(query(value).items.len(), 1);

        let value = serde_json::json!({
            "code": 0,
            "data": {"total": 1, "skills": [{"slug": "../invalid", "ownerName": "alice"}]}
        });
        assert!(parse_skillhub_query_response(&value, 1, 20).is_err());
    }

    #[test]
    fn filters_and_sorts_active_first_level_categories() {
        let response = parse_skillhub_categories_response(&serde_json::json!({
            "items": [
                {"key": "child", "name": "child", "nameEn": "child", "level": 2, "sortOrder": 1, "active": true},
                {"key": "z", "name": "Z", "nameEn": "Z", "level": 1, "sortOrder": 20, "active": true},
                {"key": "a", "name": "A", "nameEn": "A", "level": 1, "sortOrder": 10, "active": true},
                {"key": "off", "name": "Off", "nameEn": "Off", "level": 1, "sortOrder": 0, "active": false}
            ],
            "count": 4
        }))
        .unwrap();
        assert_eq!(response.items.iter().map(|item| item.key.as_str()).collect::<Vec<_>>(), ["a", "z"]);
    }

    #[test]
    fn valid_but_filtered_categories_are_a_real_empty_result() {
        let response = parse_skillhub_categories_response(&serde_json::json!({
            "items": [
                {"key": "child", "name": "child", "nameEn": "child", "level": 2, "sortOrder": 1, "active": true},
                {"key": "off", "name": "Off", "nameEn": "Off", "level": 1, "sortOrder": 0, "active": false}
            ]
        }))
        .unwrap();
        assert!(response.items.is_empty());
    }

    #[test]
    fn nonzero_business_code_is_an_upstream_error() {
        assert!(parse_skillhub_query_response(
            &serde_json::json!({"code": 17, "message": "bad", "data": {"total": 0, "skills": []}}),
            1,
            20
        )
        .is_err());
    }
}

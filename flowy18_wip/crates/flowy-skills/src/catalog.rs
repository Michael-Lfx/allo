use crate::{Card, UNVERSIONED, parse_skill_id};
use serde_json::Value;
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Category {
    pub slug: String,
    pub label: String,
}

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("clawhub returned invalid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("clawhub response is missing the expected array")]
    MissingArray,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogPage {
    pub cards: Vec<Card>,
    pub next_cursor: Option<String>,
}

pub fn parse_skill_page(body: &str) -> Result<CatalogPage, CatalogError> {
    let value: Value = serde_json::from_str(body)?;
    Ok(CatalogPage {
        cards: parse_cards(array_field(&value, "items")?, false),
        next_cursor: page_cursor(&value),
    })
}

/// Parse discovery rows returned by skill search.
///
/// `ClawHub` currently omits the published version and embedded verdict from
/// most search results, so the shelf keeps them with [`UNVERSIONED`] identity
/// instead of dropping the whole result set. A concrete version must be
/// resolved before install-time verification.
pub fn parse_search_page(body: &str) -> Result<CatalogPage, CatalogError> {
    let value: Value = serde_json::from_str(body)?;
    Ok(CatalogPage {
        cards: parse_cards(array_field(&value, "results")?, true),
        next_cursor: page_cursor(&value),
    })
}

fn page_cursor(value: &Value) -> Option<String> {
    string_field(value, "nextCursor")
}

pub fn parse_categories(body: &str) -> Result<Vec<Category>, CatalogError> {
    let value: Value = serde_json::from_str(body)?;
    Ok(array_field(&value, "categories")?
        .iter()
        .filter_map(|entry| {
            let slug = string_field(entry, "slug")?;
            let label = string_field(entry, "label")?;
            Some(Category { slug, label })
        })
        .collect())
}

fn array_field<'a>(
    value: &'a Value,
    field: &str,
) -> Result<&'a [Value], CatalogError> {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or(CatalogError::MissingArray)
}

fn parse_cards(rows: &[Value], allow_unversioned: bool) -> Vec<Card> {
    rows.iter()
        .filter_map(|entry| parse_card(entry, allow_unversioned))
        .collect()
}

fn parse_card(entry: &Value, allow_unversioned: bool) -> Option<Card> {
    let owner_handle = owner_handle(entry)?;
    let slug = string_field(entry, "slug")?;
    let verdict = embedded_verdict(entry);
    if matches!(verdict.as_deref(), Some(value) if value != "clean") {
        return None;
    }
    let published = published_version(entry);
    let preverified = verdict.as_deref() == Some("clean");
    let version = match published {
        Some(version) => version,
        None if preverified || allow_unversioned => UNVERSIONED.to_owned(),
        None => return None,
    };
    let id = parse_skill_id(&owner_handle, &slug, &version).ok()?;
    let summary = string_field(entry, "summary")
        .or_else(|| nested_path_string(entry, &["native", "skill", "summary"]))
        .unwrap_or_default();
    let title = string_field(entry, "displayName")
        .or_else(|| {
            nested_path_string(entry, &["native", "skill", "displayName"])
        })
        .unwrap_or_default();
    let categories = categories(entry)
        .or_else(|| {
            entry
                .get("native")
                .and_then(|native| native.get("skill"))
                .and_then(categories)
        })
        .unwrap_or_default();
    Some(Card {
        id,
        summary,
        categories,
        title,
        preverified,
        downloads: entry
            .get("stats")
            .and_then(|stats| stats.get("downloads"))
            .and_then(number_u64)
            .or_else(|| entry.get("downloads").and_then(number_u64))
            .or_else(|| {
                nested_path_u64(
                    entry,
                    &["native", "skill", "stats", "downloads"],
                )
            }),
        installs: entry
            .get("stats")
            .and_then(|stats| stats.get("installs"))
            .and_then(number_u64)
            .or_else(|| {
                entry
                    .get("metrics")
                    .and_then(|metrics| metrics.get("lifetimeInstalls"))
                    .and_then(number_u64)
            })
            .or_else(|| {
                nested_path_u64(
                    entry,
                    &["native", "skill", "stats", "installs"],
                )
            }),
        updated_at_ms: entry
            .get("updatedAt")
            .and_then(number_i64)
            .or_else(|| {
                entry
                    .get("metrics")
                    .and_then(|metrics| metrics.get("updatedAt"))
                    .and_then(number_i64)
            })
            .or_else(|| {
                nested_path_number(entry, &["native", "skill", "updatedAt"])
            }),
    })
}

fn owner_handle(entry: &Value) -> Option<String> {
    string_field(entry, "ownerHandle")
        .or_else(|| nested_string(entry, "publisher", "handle"))
        .or_else(|| nested_string(entry, "sourceIdentity", "owner"))
        .or_else(|| nested_path_string(entry, &["native", "ownerHandle"]))
}

fn categories(entry: &Value) -> Option<Vec<String>> {
    let categories: Vec<_> = entry
        .get("categories")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect();
    (!categories.is_empty()).then_some(categories)
}

fn published_version(entry: &Value) -> Option<String> {
    latest_version(entry)
        .or_else(|| string_field(entry, "version"))
        .or_else(|| nested_string(entry, "tags", "latest"))
}

fn embedded_verdict(entry: &Value) -> Option<String> {
    nested_string(entry, "trust", "clawHubVerdict")
}

fn nested_string(entry: &Value, object: &str, field: &str) -> Option<String> {
    nested_path_string(entry, &[object, field])
}

fn nested_path_string(entry: &Value, path: &[&str]) -> Option<String> {
    let mut value = entry;
    for field in path {
        value = value.get(*field)?;
    }
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn nested_path_number(entry: &Value, path: &[&str]) -> Option<i64> {
    let mut value = entry;
    for field in path {
        value = value.get(*field)?;
    }
    number_i64(value)
}

fn nested_path_u64(entry: &Value, path: &[&str]) -> Option<u64> {
    let mut value = entry;
    for field in path {
        value = value.get(*field)?;
    }
    number_u64(value)
}

fn latest_version(entry: &Value) -> Option<String> {
    entry
        .get("latestVersion")
        .and_then(|value| string_field(value, "version"))
}

fn number_u64(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        value.as_i64().and_then(|number| u64::try_from(number).ok())
    })
}

fn number_i64(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| {
        value.as_u64().and_then(|number| i64::try_from(number).ok())
    })
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_keeps_valid_rows_and_skips_missing_versions() {
        let body = r#"
        {
          "items": [
            {
              "ownerHandle": "pskoett",
              "slug": "self-improving-agent",
              "summary": "Captures learnings",
              "categories": ["agents", "knowledge"],
              "latestVersion": { "version": "4.0.5" }
            },
            {
              "ownerHandle": "awspace",
              "slug": "pdf",
              "summary": "no version",
              "latestVersion": null
            }
          ]
        }
        "#;
        let cards = parse_skill_page(body).unwrap().cards;
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].id.owner_handle, "pskoett");
        assert_eq!(cards[0].id.version, "4.0.5");
        assert_eq!(cards[0].categories, ["agents", "knowledge"]);
        assert!(!cards[0].preverified);
    }

    #[test]
    fn trending_card_uses_publisher_and_skips_unclean_verdicts() {
        let body = r#"
        {
          "items": [
            {
              "slug": "clawcall-dev",
              "displayName": "ClawCall",
              "summary": "Place a call",
              "categories": ["integrations"],
              "publisher": { "handle": "ClawCall-dev" },
              "trust": { "clawHubVerdict": "clean" }
            },
            {
              "slug": "bad",
              "displayName": "Bad",
              "publisher": { "handle": "someone" },
              "trust": { "clawHubVerdict": "malicious" }
            }
          ]
        }
        "#;
        let cards = parse_skill_page(body).unwrap().cards;
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].id.owner_handle, "clawcall-dev");
        assert_eq!(cards[0].id.version, crate::UNVERSIONED);
        assert_eq!(cards[0].title, "ClawCall");
        assert!(cards[0].preverified);
    }

    #[test]
    fn search_keeps_unversioned_runtime_rows() {
        let body = r#"
        {
          "results": [
            {
              "ownerHandle": "A",
              "slug": "one",
              "summary": "first",
              "version": "2.0.0",
              "categories": ["knowledge"]
            },
            {
              "ownerHandle": "b",
              "slug": "two",
              "version": null,
              "displayName": "Two",
              "downloads": 9,
              "trust": { "clawHubVerdict": null },
              "native": {
                "skill": {
                  "displayName": "Two",
                  "summary": "second",
                  "categories": ["knowledge"],
                  "stats": { "downloads": 10, "installs": 2 },
                  "tags": { "latest": "opaque-version-id" }
                }
              }
            },
            {
              "slug": "three",
              "version": "3.0.0"
            }
          ]
        }
        "#;
        let cards = parse_search_page(body).unwrap().cards;
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].id.owner_handle, "a");
        assert_eq!(cards[0].id.version, "2.0.0");
        assert_eq!(cards[1].id.owner_handle, "b");
        assert_eq!(cards[1].id.version, UNVERSIONED);
        assert_eq!(cards[1].title, "Two");
        assert_eq!(cards[1].summary, "second");
        assert_eq!(cards[1].downloads, Some(9));
        assert_eq!(cards[1].installs, Some(2));
    }

    #[test]
    fn categories_keep_api_order() {
        let body = r#"
        {
          "categories": [
            {"slug":"knowledge","label":"Knowledge","description":"","icon":"book-open"},
            {"slug":"agents","label":"Agents"}
          ]
        }
        "#;
        let categories = parse_categories(body).unwrap();
        assert_eq!(categories.len(), 2);
        assert_eq!(categories[0].slug, "knowledge");
        assert_eq!(categories[0].label, "Knowledge");
        assert_eq!(categories[1].slug, "agents");
    }

    #[test]
    fn card_keeps_counts_and_updated_at_from_either_payload() {
        let body = r#"
        {
          "items": [
            {
              "ownerHandle": "pskoett",
              "slug": "self-improving-agent",
              "displayName": "self-improving agent",
              "summary": "Captures learnings",
              "version": "4.0.5",
              "stats": {"downloads": 495021, "installs": 18550},
              "updatedAt": 1792672926676
            },
            {
              "slug": "clawcall-dev",
              "displayName": "ClawCall",
              "summary": "Place a call",
              "publisher": {"handle": "clawcall-dev"},
              "trust": {"clawHubVerdict": "clean"},
              "metrics": {
                "lifetimeInstalls": 693,
                "trending24hDownloads": 1327,
                "updatedAt": 1795501891222
              }
            }
          ]
        }
        "#;
        let cards = parse_skill_page(body).unwrap().cards;
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].downloads, Some(495_021));
        assert_eq!(cards[0].installs, Some(18_550));
        assert_eq!(cards[0].updated_at_ms, Some(1_792_672_926_676));
        assert_eq!(cards[1].downloads, None);
        assert_eq!(cards[1].installs, Some(693));
        assert_eq!(cards[1].updated_at_ms, Some(1_795_501_891_222));
    }

    #[test]
    fn page_keeps_a_non_empty_cursor() {
        let body = r#"{"items":[],"nextCursor":" page-2 "}"#;
        let page = parse_skill_page(body).unwrap();
        assert_eq!(page.next_cursor.as_deref(), Some("page-2"));
        let bare = parse_search_page(r#"{"results":[]}"#).unwrap();
        assert_eq!(bare.next_cursor, None);
        let empty =
            parse_skill_page(r#"{"items":[],"nextCursor":"  "}"#).unwrap();
        assert_eq!(empty.next_cursor, None);
    }
}

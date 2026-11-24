use crate::{Card, parse_skill_id};
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

pub fn parse_skill_page(body: &str) -> Result<Vec<Card>, CatalogError> {
    let value: Value = serde_json::from_str(body)?;
    Ok(parse_cards(array_field(&value, "items")?))
}

pub fn parse_search_page(body: &str) -> Result<Vec<Card>, CatalogError> {
    let value: Value = serde_json::from_str(body)?;
    Ok(parse_cards(array_field(&value, "results")?))
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

fn parse_cards(rows: &[Value]) -> Vec<Card> {
    rows.iter().filter_map(parse_card).collect()
}

fn parse_card(entry: &Value) -> Option<Card> {
    let owner_handle = string_field(entry, "ownerHandle")?;
    let slug = string_field(entry, "slug")?;
    let version =
        latest_version(entry).or_else(|| string_field(entry, "version"))?;
    let id = parse_skill_id(&owner_handle, &slug, &version).ok()?;
    let summary = string_field(entry, "summary").unwrap_or_default();
    let categories = entry
        .get("categories")
        .and_then(Value::as_array)
        .map_or_default(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect()
        });
    Some(Card {
        id,
        summary,
        categories,
    })
}

fn latest_version(entry: &Value) -> Option<String> {
    entry
        .get("latestVersion")
        .and_then(|value| string_field(value, "version"))
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
        let cards = parse_skill_page(body).unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].id.owner_handle, "pskoett");
        assert_eq!(cards[0].id.version, "4.0.5");
        assert_eq!(cards[0].categories, ["agents", "knowledge"]);
    }

    #[test]
    fn search_uses_top_level_version_and_skips_incomplete_rows() {
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
              "version": null
            },
            {
              "slug": "three",
              "version": "3.0.0"
            }
          ]
        }
        "#;
        let cards = parse_search_page(body).unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].id.owner_handle, "a");
        assert_eq!(cards[0].id.slug, "one");
        assert_eq!(cards[0].id.version, "2.0.0");
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
}

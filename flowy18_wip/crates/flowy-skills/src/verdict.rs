use crate::{Card, SkillId, keep_passing, parse_skill_id};
use serde_json::{Value, json};
use std::collections::HashSet;
use thiserror::Error;

const MAX_VERDICT_BATCH: usize = 100;

#[derive(Debug, Error)]
pub enum VerdictError {
    #[error("security verdict batch exceeds 100 cards")]
    BatchTooLarge,
    #[error("verdict response is not valid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("verdict response is missing the expected items array")]
    MissingItems,
}

pub fn verdict_body(cards: &[Card]) -> Result<Value, VerdictError> {
    if cards.len() > MAX_VERDICT_BATCH {
        return Err(VerdictError::BatchTooLarge);
    }
    let items: Vec<Value> = cards
        .iter()
        .map(|card| {
            json!({
                "slug": card.id.slug,
                "ownerHandle": card.id.owner_handle,
                "version": card.id.version,
            })
        })
        .collect();
    Ok(json!({ "items": items }))
}

pub fn passing_ids(body: &str) -> Result<HashSet<SkillId>, VerdictError> {
    let value: Value = serde_json::from_str(body)?;
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .ok_or(VerdictError::MissingItems)?;
    let mut passing = HashSet::new();
    for item in items {
        let decision = item.get("decision").and_then(Value::as_str);
        let security_status = item
            .get("security")
            .and_then(|security| security.get("status"))
            .and_then(Value::as_str);
        if decision != Some("pass") || security_status != Some("clean") {
            continue;
        }
        let owner = item
            .get("requestedOwnerHandle")
            .and_then(Value::as_str)
            .or_else(|| item.get("publisherHandle").and_then(Value::as_str));
        let slug = item
            .get("requestedSlug")
            .and_then(Value::as_str)
            .or_else(|| item.get("slug").and_then(Value::as_str));
        let version = item
            .get("requestedVersion")
            .and_then(Value::as_str)
            .or_else(|| item.get("version").and_then(Value::as_str));
        let (Some(owner), Some(slug), Some(version)) = (owner, slug, version)
        else {
            continue;
        };
        if let Ok(id) = parse_skill_id(owner, slug, version) {
            passing.insert(id);
        }
    }
    Ok(passing)
}

pub fn apply_verdicts(
    cards: &[Card],
    body: &str,
) -> Result<Vec<Card>, VerdictError> {
    let passing = passing_ids(body)?;
    Ok(keep_passing(cards, &passing))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(owner: &str, slug: &str) -> Card {
        Card {
            id: parse_skill_id(owner, slug, "1.0.0").unwrap(),
            summary: slug.into(),
            categories: vec![],
            title: String::new(),
            preverified: false,
            downloads: None,
            installs: None,
            updated_at_ms: None,
        }
    }

    #[test]
    fn body_contains_identity_fields_in_order() {
        let cards = vec![card("a", "one"), card("b", "two")];
        assert_eq!(
            verdict_body(&cards).unwrap(),
            json!({
                "items": [
                    {"slug":"one","ownerHandle":"a","version":"1.0.0"},
                    {"slug":"two","ownerHandle":"b","version":"1.0.0"}
                ]
            })
        );
    }

    #[test]
    fn only_clean_pass_is_kept_in_card_order() {
        let cards =
            vec![card("a", "one"), card("b", "two"), card("c", "three")];
        let body = r#"
        {
          "items": [
            {"ok":true,"decision":"pass","security":{"status":"clean"},"requestedOwnerHandle":"a","requestedSlug":"one","requestedVersion":"1.0.0"},
            {"ok":true,"decision":"fail","security":{"status":"clean"},"requestedOwnerHandle":"b","requestedSlug":"two","requestedVersion":"1.0.0"},
            {"ok":true,"decision":"pass","security":{"status":"warn"},"requestedOwnerHandle":"c","requestedSlug":"three","requestedVersion":"1.0.0"}
          ]
        }
        "#;
        let kept = apply_verdicts(&cards, body).unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id.slug, "one");
    }

    #[test]
    fn response_field_fallbacks_are_supported() {
        let body = r#"
        {
          "items": [
            {"decision":"pass","security":{"status":"clean"},"publisherHandle":"@A","slug":"one","version":"1.0.0"}
          ]
        }
        "#;
        let ids = passing_ids(body).unwrap();
        assert_eq!(ids.len(), 1);
        assert!(ids.contains(&parse_skill_id("a", "one", "1.0.0").unwrap()));
    }

    #[test]
    fn more_than_one_hundred_cards_is_rejected() {
        let cards = (0..101)
            .map(|index| card("a", &format!("skill-{index}")))
            .collect::<Vec<_>>();
        assert!(matches!(
            verdict_body(&cards),
            Err(VerdictError::BatchTooLarge)
        ));
    }
}

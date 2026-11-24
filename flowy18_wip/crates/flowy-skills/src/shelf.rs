use crate::SkillId;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    pub id: SkillId,
    pub summary: String,
    pub categories: Vec<String>,
    #[serde(default)]
    pub title: String,
    /// The catalog payload already reported this card as clean.
    #[serde(default)]
    pub preverified: bool,
    /// Lifetime downloads from a skills-list or search row.
    #[serde(default)]
    pub downloads: Option<u64>,
    /// Lifetime installs. Trending rows report this instead of downloads.
    #[serde(default)]
    pub installs: Option<u64>,
    /// Unix timestamp in milliseconds.
    #[serde(default)]
    pub updated_at_ms: Option<i64>,
}

#[must_use]
#[allow(clippy::implicit_hasher)]
pub fn keep_passing(cards: &[Card], passing: &HashSet<SkillId>) -> Vec<Card> {
    cards
        .iter()
        .filter(|card| passing.contains(&card.id))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_skill_id;

    fn card(owner: &str, slug: &str) -> Card {
        Card {
            id: parse_skill_id(owner, slug, "1.0.0").unwrap(),
            summary: slug.into(),
            categories: vec!["knowledge".into()],
            title: String::new(),
            preverified: false,
            downloads: None,
            installs: None,
            updated_at_ms: None,
        }
    }

    #[test]
    fn filter_drops_failures_and_keeps_api_order() {
        let cards =
            vec![card("a", "one"), card("b", "two"), card("c", "three")];
        let mut passing = HashSet::new();
        passing.insert(cards[0].id.clone());
        passing.insert(cards[2].id.clone());
        let kept = keep_passing(&cards, &passing);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].id.slug, "one");
        assert_eq!(kept[1].id.slug, "three");
    }
}

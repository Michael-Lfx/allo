//! Localized variant values carried by market manifests.
//!
//! Real markets ship display text per language as `<field>_<lang>` siblings of
//! the baseline field (`description_zh` / `description_en`, `tags_zh` /
//! `tags_en`, `legacy_tags_*`, `name_*`, `category_*`, `examples_*`), see
//! `docs/agent-store/18-marketplace-spec.zh.md` §4.
//!
//! Decision **D8=A** (doc `21`): the fallback chain `{field}_{lang}` →
//! `{field}` is resolved by the *reader*, because only the client knows its UI
//! language. The store therefore transports the variants verbatim and never
//! picks a language on the server's side.
//!
//! A variant value is either a plain string or a list of strings — the only
//! two shapes the three real markets (experts / skills / connectors) carry.

use serde::{Deserialize, Serialize};

/// One `<field>_<lang>` value: a string, or a list of strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LocalizedVariant {
    /// `description_zh`, `name_en`, `category_zh` — a plain string.
    Text(String),
    /// `tags_zh`, `legacy_tags_en`, `examples_zh` — a list of strings.
    List(Vec<String>),
}

impl LocalizedVariant {
    /// Whether the variant carries no content (empty string / empty list).
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Text(text) => text.is_empty(),
            Self::List(items) => items.is_empty(),
        }
    }

    /// Read a variant out of a raw manifest value.
    ///
    /// Returns `None` when the value is neither a string nor an array of
    /// strings — such values are not display variants and are left out of the
    /// projection instead of being coerced.
    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        match value {
            serde_json::Value::String(text) => Some(Self::Text(text.clone())),
            serde_json::Value::Array(items) => items
                .iter()
                .map(|item| item.as_str().map(str::to_owned))
                .collect::<Option<Vec<String>>>()
                .map(Self::List),
            _ => None,
        }
    }
}

/// Collect every `<field>_zh` / `<field>_en` variant from a manifest entry.
///
/// Only `_zh` / `_en` suffixed keys whose value is a string or a list of
/// strings are kept, so unrelated manifest fields (`featured` as a number,
/// `name_map` as an object) never leak into the projection. Empty variants are
/// dropped: they would only add wire weight without changing any fallback.
pub fn collect_localized_variants(
    item: &serde_json::Value,
) -> std::collections::BTreeMap<String, LocalizedVariant> {
    let mut collected = std::collections::BTreeMap::new();
    let Some(object) = item.as_object() else {
        return collected;
    };
    for (key, value) in object {
        if !(key.ends_with("_zh") || key.ends_with("_en")) {
            continue;
        }
        let Some(variant) = LocalizedVariant::from_json(value) else {
            continue;
        };
        if variant.is_empty() {
            continue;
        }
        collected.insert(key.clone(), variant);
    }
    collected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_string_and_list_variants_only() {
        let item = serde_json::json!({
            "name": "demo",
            "description": "base",
            "description_zh": "中文简介",
            "description_en": "English summary",
            "tags_zh": ["效率", "办公"],
            "legacy_tags_en": ["productivity"],
            "featured": 3,
            "name_map": { "a": "b" },
            "empty_en": "",
            "mixed_zh": ["ok", 7],
            "unrelated_zh_suffixless": "x"
        });
        let variants = collect_localized_variants(&item);
        let keys: Vec<&str> = variants.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec!["description_en", "description_zh", "legacy_tags_en", "tags_zh"]
        );
        assert_eq!(variants["description_zh"], LocalizedVariant::Text("中文简介".into()));
        assert_eq!(
            variants["tags_zh"],
            LocalizedVariant::List(vec!["效率".into(), "办公".into()])
        );
    }

    #[test]
    fn empty_and_non_display_values_are_skipped() {
        let item = serde_json::json!({ "name_zh": "", "tags_en": [], "featured": 1, "nested_zh": { "a": 1 } });
        assert!(collect_localized_variants(&item).is_empty());
    }

    #[test]
    fn wire_shape_is_string_or_string_list() {
        let variants = collect_localized_variants(&serde_json::json!({
            "description_zh": "文本",
            "tags_zh": ["a", "b"]
        }));
        let wire = serde_json::to_value(&variants).expect("serialize");
        assert_eq!(wire["description_zh"], serde_json::json!("文本"));
        assert_eq!(wire["tags_zh"], serde_json::json!(["a", "b"]));
    }
}

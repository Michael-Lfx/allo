//! Display metadata read off a snapshot component payload (`50` §3.2 / §3.5).
//!
//! The importer stamps four keys into every skill / connector / team payload —
//! `display_name`, `display_description`, `quick_prompts`, `tags` — under names
//! identical to the wire fields, so a catalog only ever deserializes them. No
//! market lookup, no disk read, no extra DB round trip: every read face here
//! already had the payload in hand for another reason.
//!
//! Nothing is invented on the way out (`50` INV-3): a payload without the key
//! yields `None` / an empty list, never an empty object or a copy of the
//! single-language `name` / `description`.

use nomifun_api_types::AppServerLocalizedText;
use nomifun_importer::{DisplayFields, LocalizedText};

/// Wire form of one `{zh, en}` pair.
pub(crate) fn to_localized(text: &LocalizedText) -> AppServerLocalizedText {
    AppServerLocalizedText { en: text.en.clone(), zh: text.zh.clone() }
}

/// `Some` only when the pair declares something. An all-blank pair is
/// indistinguishable from "the market declared nothing", and the wire says the
/// latter by omitting the field.
pub(crate) fn localized_or_none(text: &LocalizedText) -> Option<AppServerLocalizedText> {
    (!text.is_empty()).then(|| to_localized(text))
}

/// The four display fields a component payload carries.
pub(crate) fn display_from_payload(payload: &serde_json::Value) -> DisplayFields {
    DisplayFields {
        name: localized_field(payload, "display_name"),
        description: localized_field(payload, "display_description"),
        prompts: localized_items(payload, "quick_prompts"),
        tags: localized_items(payload, "tags"),
    }
}

/// One `{zh, en}` object. Blank sides are dropped, so a one-sided declaration
/// keeps the absent locale absent (`50` INV-4).
fn localized_field(payload: &serde_json::Value, key: &str) -> Option<LocalizedText> {
    let value = payload.get(key)?;
    let text = LocalizedText {
        zh: localized_side(value.get("zh")),
        en: localized_side(value.get("en")),
    };
    (!text.is_empty()).then_some(text)
}

/// A list of `{zh, en}` objects. A bare string is accepted as its zh value —
/// the legacy market shape `tags: ["效率"]` that `44` already tolerates.
fn localized_items(payload: &serde_json::Value, key: &str) -> Vec<LocalizedText> {
    let Some(items) = payload.get(key).and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| match item {
            serde_json::Value::String(text) => {
                let text = text.trim();
                (!text.is_empty()).then(|| LocalizedText {
                    zh: Some(text.to_owned()),
                    en: None,
                })
            }
            serde_json::Value::Object(_) => {
                let text = LocalizedText {
                    zh: localized_side(item.get("zh")),
                    en: localized_side(item.get("en")),
                };
                (!text.is_empty()).then_some(text)
            }
            _ => None,
        })
        .collect()
}

fn localized_side(value: Option<&serde_json::Value>) -> Option<String> {
    value
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TC-8 / INV-4: a one-sided declaration keeps the other side absent, blank
    /// sides count as absent, and a bare string is read as its zh value.
    #[test]
    fn reads_the_four_keys_and_drops_blank_or_absent_sides() {
        let payload = serde_json::json!({
            "display_name": { "zh": "演示网盘", "en": null },
            "display_description": { "zh": "  ", "en": "Demo Netdisk" },
            "quick_prompts": [
                { "zh": "查一下我的文件", "en": "List my files" },
                "只有中文的示例",
                { "zh": "", "en": "" }
            ],
            "tags": [{ "zh": "效率", "en": "Productivity" }],
            "unrelated": "ignored",
        });

        let display = display_from_payload(&payload);
        let name = display.name.expect("display_name");
        assert_eq!(name.zh.as_deref(), Some("演示网盘"));
        assert_eq!(name.en, None, "a null side stays absent");

        let description = display.description.expect("display_description");
        assert_eq!(description.zh, None, "a blank side declares nothing");
        assert_eq!(description.en.as_deref(), Some("Demo Netdisk"));

        assert_eq!(display.prompts.len(), 2, "the all-blank entry is dropped");
        assert_eq!(display.prompts[1].zh.as_deref(), Some("只有中文的示例"));
        assert_eq!(display.prompts[1].en, None);

        assert_eq!(display.tags.len(), 1);
        assert_eq!(display.tags[0].en.as_deref(), Some("Productivity"));
    }

    /// TC-11: a payload from a snapshot that predates `50` carries none of the
    /// keys, so the fields stay absent instead of becoming empty placeholders.
    #[test]
    fn a_payload_without_the_keys_yields_nothing() {
        let display = display_from_payload(&serde_json::json!({
            "name": "fenbi_mcp",
            "description": "English text",
        }));
        assert!(display.is_empty());
        assert!(localized_or_none(&LocalizedText::default()).is_none());
    }
}

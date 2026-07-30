//! Provider-specific MCP payload decoders.
//!
//! The remote peer knows only MCP wire types. This module is the private seam
//! where provider contracts become the small normalized representation used
//! by the managed router.

mod parallel;
mod shared;
mod you;

use crate::types::SearchHit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecodeError {
    MalformedResponse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecodeSource {
    Structured,
    TextJsonFallback,
    LabelledTextFallback,
}

impl DecodeSource {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Structured => "structured",
            Self::TextJsonFallback => "text_json_fallback",
            Self::LabelledTextFallback => "labelled_text_fallback",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DecodeOutcome {
    pub(crate) hits: Vec<SearchHit>,
    pub(crate) source: DecodeSource,
    pub(crate) dropped_items: usize,
    pub(crate) contract_degraded: bool,
    pub(crate) structured_fallback: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DecodedItems {
    pub(crate) hits: Vec<NormalizedSearchHit>,
    pub(crate) dropped_items: usize,
}

pub(crate) use parallel::decode_parallel;
pub(crate) use shared::NormalizedSearchHit;
pub(crate) use you::decode_you;

#[cfg(test)]
mod tests {
    use nomi_mcp::protocol::McpToolResult;
    use serde_json::{Value, json};

    use super::*;

    fn structured_fixture(path: &str) -> McpToolResult {
        let value: Value = serde_json::from_str(path).expect("fixture JSON");
        serde_json::from_value(json!({"structuredContent": value})).expect("tool result")
    }

    #[test]
    fn parallel_decoder_keeps_date_and_excerpt_boundaries() {
        let result = structured_fixture(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/managed_search/parallel-structured.json"
        )));
        let hits = decode_parallel(&result, 5).expect("parallel fixture").hits;
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].url, "https://example.com/one");
        assert_eq!(hits[0].published_at.as_deref(), Some("2026-07-30"));
        assert_eq!(hits[0].snippet, "First evidence sentence.\nA second supporting paragraph.");
    }

    #[test]
    fn you_decoder_interleaves_web_and_news() {
        let result = structured_fixture(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/managed_search/you-structured-mixed.json"
        )));
        let hits = decode_you(&result, 5).expect("you fixture").hits;
        assert_eq!(
            hits.iter().map(|hit| hit.title.as_str()).collect::<Vec<_>>(),
            ["Web fixture", "News fixture"]
        );
        assert_eq!(hits[1].published_at.as_deref(), Some("2026-07-28T12:00:00Z"));
    }

    #[test]
    fn you_decoder_deduplicates_canonical_urls_and_merges_evidence() {
        let result = structured_fixture(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/managed_search/you-structured-duplicates.json"
        )));
        let hits = decode_you(&result, 5).expect("you duplicate fixture").hits;
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "First title");
        assert_eq!(hits[0].published_at.as_deref(), Some("2026-07-30"));
        assert_eq!(hits[0].snippet, "same evidence\nfirst-only\nsecond-only");
    }

    #[test]
    fn you_decoder_falls_back_to_text_json_then_labelled_text() {
        let json_value: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/managed_search/you-text-json.json"
        )))
        .unwrap();
        let json_result: McpToolResult = serde_json::from_value(json!({
            "content": [{"type": "text", "text": json_value.to_string()}]
        }))
        .unwrap();
        assert_eq!(
            decode_you(&json_result, 5).unwrap().hits[0].title,
            "Text JSON fixture"
        );

        let labelled_result: McpToolResult = serde_json::from_value(json!({
            "content": [{"type": "text", "text": include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/managed_search/you-text-tagged.txt"
            ))}]
        }))
        .unwrap();
        let hits = decode_you(&labelled_result, 5).unwrap().hits;
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].title, "Tagged web fixture");
        assert_eq!(hits[1].title, "Tagged news fixture");
    }

    #[test]
    fn you_decoder_distinguishes_empty_from_malformed() {
        let empty = structured_fixture(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/managed_search/you-empty.json"
        )));
        assert!(decode_you(&empty, 5).unwrap().hits.is_empty());

        let malformed = structured_fixture(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/managed_search/you-malformed.json"
        )));
        assert!(matches!(
            decode_you(&malformed, 5),
            Err(DecodeError::MalformedResponse)
        ));
    }

    #[test]
    fn shared_url_normalization_rejects_credentials_and_non_http_schemes() {
        assert!(super::shared::normalize_url("https://user:pass@example.com").is_none());
        assert!(super::shared::normalize_url("file:///tmp/result").is_none());
        assert_eq!(
            super::shared::normalize_url("HTTPS://EXAMPLE.COM:443/path#fragment")
                .unwrap()
                .as_str(),
            "https://example.com/path"
        );
    }

    #[test]
    fn decoders_keep_valid_items_when_one_item_is_malformed() {
        let parallel: McpToolResult = serde_json::from_value(json!({
            "structuredContent": {
                "results": [
                    {"title": "bad", "url": "file:///not-allowed", "excerpts": ["ignored"]},
                    {"title": "good", "url": "https://example.com/good", "excerpts": ["kept"]}
                ]
            }
        }))
        .unwrap();
        let parallel = decode_parallel(&parallel, 5).expect("valid item remains");
        assert_eq!(parallel.hits.len(), 1);
        assert_eq!(parallel.dropped_items, 1);
        assert!(parallel.contract_degraded);

        let you: McpToolResult = serde_json::from_value(json!({
            "structuredContent": {
                "results": {
                    "web": [
                        {"title": "bad", "url": "not a url"},
                        {"title": "good", "url": "https://example.com/good", "description": "kept"}
                    ],
                    "news": []
                }
            }
        }))
        .unwrap();
        let you = decode_you(&you, 5).expect("valid item remains");
        assert_eq!(you.hits.len(), 1);
        assert_eq!(you.dropped_items, 1);
        assert!(you.contract_degraded);
    }

    #[test]
    fn decoder_fallback_reports_source_without_content() {
        let result: McpToolResult = serde_json::from_value(json!({
            "content": [{"type": "text", "text": "not json"}]
        }))
        .unwrap();
        let outcome = decode_you(&result, 5).expect_err("unlabelled text is malformed");
        assert_eq!(outcome, DecodeError::MalformedResponse);

        let structured: McpToolResult = serde_json::from_value(json!({
            "structuredContent": {"results": {"web": [], "news": []}},
            "content": [{"type": "text", "text": "{\"results\": {\"web\": [], \"news\": []}}"}]
        }))
        .unwrap();
        let outcome = decode_you(&structured, 5).expect("structured empty result");
        assert_eq!(outcome.source, DecodeSource::Structured);
        assert!(!outcome.structured_fallback);
    }
}

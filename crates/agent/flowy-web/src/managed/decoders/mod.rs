//! Provider-specific MCP payload decoders.
//!
//! The remote peer knows only MCP wire types. This module is the private seam
//! where provider contracts become the small normalized representation used
//! by the managed router.

mod parallel;
mod shared;
mod you;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecodeError {
    MalformedResponse,
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
        let hits = decode_parallel(&result, 5).expect("parallel fixture");
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
        let hits = decode_you(&result, 5).expect("you fixture");
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
        let hits = decode_you(&result, 5).expect("you duplicate fixture");
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
        assert_eq!(decode_you(&json_result, 5).unwrap()[0].title, "Text JSON fixture");

        let labelled_result: McpToolResult = serde_json::from_value(json!({
            "content": [{"type": "text", "text": include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/managed_search/you-text-tagged.txt"
            ))}]
        }))
        .unwrap();
        let hits = decode_you(&labelled_result, 5).unwrap();
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
        assert!(decode_you(&empty, 5).unwrap().is_empty());

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
}

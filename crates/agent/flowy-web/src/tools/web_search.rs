use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use nomi_protocol::events::ToolCategory;
use nomi_tools::Tool;
use nomi_types::tool::{JsonSchema, ToolResult};

use crate::provider::SearchProvider;
use crate::types::{DEFAULT_SEARCH_COUNT, MAX_SEARCH_COUNT, SearchQuery, SearchResult};

pub struct WebSearchTool {
    provider: Arc<dyn SearchProvider>,
}

impl WebSearchTool {
    pub fn new(provider: Arc<dyn SearchProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the open web for current facts, news, traffic limits, weather, and other public \
         information. Prefer this before Browser. If search snippets already answer the question, \
         do not call web_extract. Only use web_extract when you need the page body beyond \
         snippets."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "count": {
                    "type": "integer",
                    "description": "Max results to return (default 5, max 10)"
                }
            },
            "required": ["query"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let query = match input.get("query").and_then(|v| v.as_str()) {
            Some(s) if !s.trim().is_empty() => s.trim().to_owned(),
            _ => {
                return ToolResult::error(
                    "Missing required 'query' string (must not be empty or whitespace)",
                );
            }
        };

        let count = input
            .get("count")
            .and_then(|v| v.as_u64())
            .map(|n| (n as u32).clamp(1, MAX_SEARCH_COUNT))
            .unwrap_or(DEFAULT_SEARCH_COUNT);

        match self
            .provider
            .search(SearchQuery {
                query: query.clone(),
                count,
            })
            .await
        {
            Ok(result) => ToolResult::text(format_search_result(&result)),
            Err(e) => ToolResult::error(format!("web_search failed: {e}")),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Info
    }

    fn describe(&self, input: &Value) -> String {
        let q = input.get("query").and_then(|v| v.as_str()).unwrap_or("");
        format!("web_search '{q}'")
    }
}

fn format_search_result(result: &SearchResult) -> String {
    if result.hits.is_empty() {
        return format!("No results.\nprovider={}", result.provider);
    }

    let mut out = String::new();
    for hit in &result.hits {
        out.push_str(&format!(
            "{}. {}\n{}\n{}\n\n",
            hit.rank, hit.title, hit.url, hit.snippet
        ));
    }
    out.push_str(&format!("provider={}", result.provider));
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use serde_json::json;

    use nomi_tools::Tool;

    use crate::provider::SearchProvider;
    use crate::types::{SearchHit, SearchQuery, SearchResult, WebError};

    use super::WebSearchTool;

    struct MockSearch;

    #[async_trait]
    impl SearchProvider for MockSearch {
        fn name(&self) -> &str {
            "mock"
        }

        async fn search(&self, q: SearchQuery) -> Result<SearchResult, WebError> {
            Ok(SearchResult {
                provider: "mock".into(),
                hits: vec![SearchHit {
                    title: format!("R:{}", q.query),
                    url: "https://example.com".into(),
                    snippet: "s".into(),
                    rank: 1,
                }],
            })
        }
    }

    #[tokio::test]
    async fn web_search_tool_rejects_empty_query() {
        let tool = WebSearchTool::new(Arc::new(MockSearch));
        let r = tool.execute(json!({"query": "  "})).await;
        assert!(r.is_error);
    }

    #[tokio::test]
    async fn web_search_tool_formats_hits() {
        let tool = WebSearchTool::new(Arc::new(MockSearch));
        let r = tool
            .execute(json!({"query": "beijing", "count": 3}))
            .await;
        assert!(!r.is_error);
        assert!(r.content.contains("https://example.com"));
        assert!(r.content.contains("R:beijing"));
    }
}

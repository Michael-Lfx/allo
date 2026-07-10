use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use nomi_protocol::events::ToolCategory;
use nomi_tools::Tool;
use nomi_types::tool::{JsonSchema, ToolResult};

use crate::provider::ExtractProvider;
use crate::types::{ExtractRequest, MAX_EXTRACT_URLS};

pub struct WebExtractTool {
    provider: Arc<dyn ExtractProvider>,
}

impl WebExtractTool {
    pub fn new(provider: Arc<dyn ExtractProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl Tool for WebExtractTool {
    fn name(&self) -> &str {
        "web_extract"
    }

    fn description(&self) -> &str {
        "Fetch public URLs and return readable markdown of the main article body (boilerplate \
         stripped when possible, truncated for context). Use when you already have URLs and \
         snippets from web_search are not enough. Do not use Browser just to read public pages."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "urls": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "URLs to extract (1–3)"
                }
            },
            "required": ["urls"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let urls = match input.get("urls").and_then(|v| v.as_array()) {
            Some(arr) => arr,
            None => return ToolResult::error("Missing required 'urls' array"),
        };

        if urls.is_empty() {
            return ToolResult::error("urls must contain at least one URL");
        }
        if urls.len() > MAX_EXTRACT_URLS {
            return ToolResult::error(format!(
                "urls length {} exceeds max {}",
                urls.len(),
                MAX_EXTRACT_URLS
            ));
        }

        let mut sections = Vec::new();
        let mut success_count = 0usize;

        // Serial per-URL extract (concurrency = 1).
        for (i, url_val) in urls.iter().enumerate() {
            let Some(url) = url_val.as_str() else {
                sections.push(format!(
                    "### URL {} — error\ninvalid argument: urls[{i}] must be a string",
                    i + 1
                ));
                continue;
            };

            match self
                .provider
                .extract(ExtractRequest {
                    url: url.to_owned(),
                })
                .await
            {
                Ok(page) => {
                    success_count += 1;
                    let title = page.title.as_deref().unwrap_or("(no title)");
                    let truncated = if page.truncated {
                        "\ntruncated: true"
                    } else {
                        ""
                    };
                    sections.push(format!(
                        "### URL {} — ok\nurl: {}\ntitle: {}\nprovider: {}\nextractor: {}{}\n\n{}",
                        i + 1,
                        page.url,
                        title,
                        page.provider,
                        page.extractor,
                        truncated,
                        page.markdown
                    ));
                }
                Err(e) => {
                    sections.push(format!("### URL {} — error\nurl: {}\nerror: {e}", i + 1, url));
                }
            }
        }

        let content = sections.join("\n\n");
        if success_count == 0 {
            ToolResult::error(content)
        } else {
            ToolResult::text(content)
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Info
    }

    fn describe(&self, input: &Value) -> String {
        let n = input
            .get("urls")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        format!("web_extract {n} url(s)")
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use serde_json::json;

    use nomi_tools::Tool;

    use crate::provider::ExtractProvider;
    use crate::types::{
        ExtractRequest, ExtractedPage, EXTRACTOR_READABILITY, MAX_EXTRACT_URLS, WebError,
    };

    use super::WebExtractTool;

    struct MockExtract {
        fail_urls: Vec<String>,
    }

    #[async_trait]
    impl ExtractProvider for MockExtract {
        fn name(&self) -> &str {
            "mock"
        }

        async fn extract(&self, req: ExtractRequest) -> Result<ExtractedPage, WebError> {
            if self.fail_urls.iter().any(|u| u == &req.url) {
                return Err(WebError::Provider(format!("failed: {}", req.url)));
            }
            Ok(ExtractedPage {
                url: req.url.clone(),
                title: Some(format!("Title:{}", req.url)),
                markdown: format!("Body for {}", req.url),
                truncated: false,
                provider: "mock".into(),
                extractor: EXTRACTOR_READABILITY.to_owned(),
            })
        }
    }

    #[tokio::test]
    async fn web_extract_tool_rejects_too_many_urls() {
        let tool = WebExtractTool::new(Arc::new(MockExtract {
            fail_urls: vec![],
        }));
        let urls: Vec<String> = (0..=MAX_EXTRACT_URLS)
            .map(|i| format!("https://example.com/{i}"))
            .collect();
        let r = tool.execute(json!({ "urls": urls })).await;
        assert!(r.is_error);
    }

    #[tokio::test]
    async fn web_extract_tool_rejects_empty_urls() {
        let tool = WebExtractTool::new(Arc::new(MockExtract {
            fail_urls: vec![],
        }));
        let r = tool.execute(json!({ "urls": [] })).await;
        assert!(r.is_error);
    }

    #[tokio::test]
    async fn web_extract_tool_formats_page_content() {
        let tool = WebExtractTool::new(Arc::new(MockExtract {
            fail_urls: vec![],
        }));
        let r = tool
            .execute(json!({ "urls": ["https://example.com/a"] }))
            .await;
        assert!(!r.is_error);
        assert!(r.content.contains("https://example.com/a"));
        assert!(r.content.contains("Body for https://example.com/a"));
        assert!(r.content.contains("Title:https://example.com/a"));
    }

    #[tokio::test]
    async fn web_extract_tool_partial_failure_not_error() {
        let ok = "https://example.com/ok".to_string();
        let bad = "https://example.com/bad".to_string();
        let tool = WebExtractTool::new(Arc::new(MockExtract {
            fail_urls: vec![bad.clone()],
        }));
        let r = tool.execute(json!({ "urls": [ok, bad] })).await;
        assert!(!r.is_error);
        assert!(r.content.contains("Body for https://example.com/ok"));
        assert!(
            r.content.contains("failed")
                || r.content.contains("error")
                || r.content.contains("Error"),
            "content should mention the failure: {}",
            r.content
        );
        assert!(r.content.contains("https://example.com/bad"));
    }

    #[tokio::test]
    async fn web_extract_tool_includes_extractor_label() {
        let tool = WebExtractTool::new(Arc::new(MockExtract {
            fail_urls: vec![],
        }));
        let r = tool
            .execute(json!({"urls":["https://example.com/a"]}))
            .await;
        assert!(!r.is_error);
        assert!(
            r.content.contains("extractor: readability"),
            "{}",
            r.content
        );
    }

    #[tokio::test]
    async fn web_extract_tool_all_failures_is_error() {
        let a = "https://example.com/a".to_string();
        let b = "https://example.com/b".to_string();
        let tool = WebExtractTool::new(Arc::new(MockExtract {
            fail_urls: vec![a.clone(), b.clone()],
        }));
        let r = tool.execute(json!({ "urls": [a, b] })).await;
        assert!(r.is_error);
    }
}

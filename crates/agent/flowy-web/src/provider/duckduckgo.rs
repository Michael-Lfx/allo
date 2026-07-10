use async_trait::async_trait;

use crate::provider::SearchProvider;
use crate::types::{SearchHit, SearchQuery, SearchResult, WebError, MAX_SEARCH_COUNT};

pub struct DuckDuckGoSearchProvider {
    client: reqwest::Client,
    timeout: std::time::Duration,
}

impl DuckDuckGoSearchProvider {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("FlowyWeb/0.1 (+https://github.com/flowy)")
                .build()
                .expect("reqwest client"),
            timeout: std::time::Duration::from_secs(15),
        }
    }
}

#[async_trait]
impl SearchProvider for DuckDuckGoSearchProvider {
    fn name(&self) -> &str {
        "duckduckgo"
    }

    async fn search(&self, query: SearchQuery) -> Result<SearchResult, WebError> {
        let q = query.query.trim();
        if q.is_empty() {
            return Err(WebError::InvalidArgument("query must not be empty".into()));
        }
        let count = query.count.clamp(1, MAX_SEARCH_COUNT);
        let url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            urlencoding_encode(q)
        );
        let response = self
            .client
            .get(&url)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    WebError::Timeout(e.to_string())
                } else {
                    WebError::Network(e.to_string())
                }
            })?;
        if !response.status().is_success() {
            return Err(WebError::Provider(format!(
                "duckduckgo HTTP {}",
                response.status()
            )));
        }
        let body = response
            .text()
            .await
            .map_err(|e| WebError::Network(e.to_string()))?;
        let hits = parse_ddg_html(&body, count);
        Ok(SearchResult {
            provider: self.name().to_owned(),
            hits,
        })
    }
}

/// Minimal percent-encoding for query strings (space → `+`, reserve unreserved).
fn urlencoding_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub(crate) fn parse_ddg_html(html: &str, limit: u32) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    let mut rest = html;
    while hits.len() < limit as usize {
        let Some(a_idx) = rest.find("result__a") else {
            break;
        };
        let after_class = &rest[a_idx..];
        let Some(href_rel) = after_class.find("href=\"") else {
            rest = &after_class[1..];
            continue;
        };
        let href_start = href_rel + "href=\"".len();
        let href_body = &after_class[href_start..];
        let Some(href_end) = href_body.find('"') else {
            break;
        };
        let url = href_body[..href_end].to_owned();
        let after_href = &href_body[href_end + 1..];
        let Some(gt) = after_href.find('>') else {
            break;
        };
        let title_body = &after_href[gt + 1..];
        let Some(close) = title_body.find('<') else {
            break;
        };
        let title = title_body[..close]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let after_title = &title_body[close..];
        let snippet = after_title
            .find("result__snippet")
            .and_then(|i| {
                let s = &after_title[i..];
                let gt = s.find('>')?;
                let body = &s[gt + 1..];
                let end = body.find('<')?;
                Some(
                    body[..end]
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" "),
                )
            })
            .unwrap_or_default();
        if !url.is_empty() && !title.is_empty() {
            hits.push(SearchHit {
                title,
                url,
                snippet,
                rank: (hits.len() as u32) + 1,
            });
        }
        rest = after_title;
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture_hits() {
        let html = include_str!("../../tests/fixtures/ddg_sample.html");
        let hits = parse_ddg_html(html, 5);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].title, "Alpha Title");
        assert_eq!(hits[0].url, "https://example.com/a");
        assert!(hits[0].snippet.contains("Alpha"));
        assert_eq!(hits[0].rank, 1);
    }
}

//! Host web search for briefing when the user did not paste enough URLs.

use flowy_web::provider::{DuckDuckGoSearchProvider, SearchProvider};
use flowy_web::{SearchQuery, MAX_SEARCH_COUNT};
use nomi_briefing::{domain_of, Citation, ResearchDepth, SourceRetriever};

const FAST_COUNT: u32 = 8;

pub struct DdgSourceRetriever {
    provider: DuckDuckGoSearchProvider,
}

impl DdgSourceRetriever {
    pub fn try_new() -> Result<Self, String> {
        Ok(Self {
            provider: DuckDuckGoSearchProvider::try_new().map_err(|e| e.to_string())?,
        })
    }

    async fn retrieve_async(
        &self,
        intent: &str,
        time_window_hours: u32,
        depth: ResearchDepth,
    ) -> Result<Vec<Citation>, String> {
        let queries = search_queries(intent, time_window_hours, depth);
        let retrieved_at = chrono::Local::now().to_rfc3339();
        let mut found = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for query in queries {
            let result = self
                .provider
                .search(SearchQuery {
                    query,
                    count: FAST_COUNT.min(MAX_SEARCH_COUNT),
                })
                .await
                .map_err(|e| e.to_string())?;
            for hit in result.hits {
                let Some(url) = unwrap_hit_url(&hit.url) else {
                    continue;
                };
                let Some(domain) = domain_of(&url) else {
                    continue;
                };
                if is_search_shell(&domain) {
                    continue;
                }
                if !seen.insert(domain.clone()) {
                    continue;
                }
                found.push(Citation {
                    url,
                    domain,
                    excerpt: excerpt_of(&hit.title, &hit.snippet),
                    retrieved_at: retrieved_at.clone(),
                });
            }
        }
        Ok(found)
    }
}

impl SourceRetriever for DdgSourceRetriever {
    fn retrieve(
        &self,
        intent: &str,
        time_window_hours: u32,
        depth: ResearchDepth,
    ) -> Result<Vec<Citation>, String> {
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| "briefing research requires a tokio runtime".to_string())?;
        handle.block_on(self.retrieve_async(intent, time_window_hours, depth))
    }
}

fn search_queries(intent: &str, time_window_hours: u32, depth: ResearchDepth) -> Vec<String> {
    let intent = intent.trim();
    let recency = if time_window_hours <= 24 {
        " 最新"
    } else if time_window_hours <= 72 {
        " 近日"
    } else {
        ""
    };
    let primary = format!("{intent}{recency}");
    if depth == ResearchDepth::Deep {
        vec![primary, format!("{intent} 最新报道")]
    } else {
        vec![primary]
    }
}

fn is_search_shell(domain: &str) -> bool {
    matches!(
        domain,
        "duckduckgo.com" | "google.com" | "bing.com" | "baidu.com" | "yahoo.com"
    )
}

fn excerpt_of(title: &str, snippet: &str) -> String {
    let snippet = snippet.trim();
    if snippet.is_empty() {
        title.trim().to_string()
    } else {
        snippet.to_string()
    }
}

fn unwrap_hit_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let absolute = if let Some(rest) = trimmed.strip_prefix("//") {
        format!("https://{rest}")
    } else if trimmed.starts_with('/') {
        format!("https://duckduckgo.com{trimmed}")
    } else {
        trimmed.to_string()
    };
    let parsed = url::Url::parse(&absolute).ok()?;
    if let Some((_, value)) = parsed.query_pairs().find(|(key, _)| key == "uddg") {
        let decoded = value.into_owned();
        if decoded.starts_with("http://") || decoded.starts_with("https://") {
            return Some(decoded);
        }
    }
    if parsed.scheme() == "http" || parsed.scheme() == "https" {
        Some(parsed.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unwraps_ddg_redirect() {
        let raw = "https://duckduckgo.com/l/?uddg=https%3A%2F%2Freuters.example%2Fa&rut=1";
        assert_eq!(
            unwrap_hit_url(raw).as_deref(),
            Some("https://reuters.example/a")
        );
    }

    #[test]
    fn keeps_direct_https() {
        assert_eq!(
            unwrap_hit_url("https://news.example/a").as_deref(),
            Some("https://news.example/a")
        );
    }

    #[test]
    fn deep_adds_a_second_query() {
        let queries = search_queries("芯片出口", 24, ResearchDepth::Deep);
        assert_eq!(queries.len(), 2);
        assert!(queries[0].contains("芯片出口"));
        assert!(queries[1].contains("最新报道"));
    }
}

//! Pluggable main-content (article) extraction before markdown conversion.

pub struct ArticleHtml {
    pub html: String,
    pub title: Option<String>,
}

pub trait ArticleExtractor: Send + Sync {
    /// Returns `None` when no usable article body is found.
    fn extract_article(&self, html: &str, document_url: Option<&str>) -> Option<ArticleHtml>;
}

pub struct DomSmoothieExtractor;

impl DomSmoothieExtractor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DomSmoothieExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl ArticleExtractor for DomSmoothieExtractor {
    fn extract_article(&self, html: &str, document_url: Option<&str>) -> Option<ArticleHtml> {
        // dom_smoothie::Readability::new errors if document_url is Some but not absolute.
        // Prefer Some(absolute_url) when caller has final_url; else None.
        let mut reader = dom_smoothie::Readability::new(html, document_url, None).ok()?;
        let article = reader.parse().ok()?;
        let content = article.content.to_string();
        if content.trim().is_empty() {
            return None;
        }
        let title = {
            let t = article.title.trim();
            (!t.is_empty()).then(|| t.to_owned())
        };
        Some(ArticleHtml {
            html: content,
            title,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readability_strips_nav_and_keeps_article_body() {
        let html = include_str!("../../tests/fixtures/article_with_chrome.html");
        let ext = DomSmoothieExtractor::new();
        let article = ext
            .extract_article(html, Some("https://example.com/limits"))
            .expect("article");
        let lower = article.html.to_lowercase();
        assert!(
            lower.contains("restricted tail numbers") || lower.contains("beijing plate"),
        );
        assert!(
            !lower.contains("buy insurance now"),
            "ads should be stripped: {lower}"
        );
        assert!(
            !lower.contains("login"),
            "nav chrome should be stripped: {lower}"
        );
    }

    #[test]
    fn empty_html_returns_none() {
        let ext = DomSmoothieExtractor::new();
        assert!(ext
            .extract_article("<html><body></body></html>", None)
            .is_none());
    }
}

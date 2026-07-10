# flowy-web Article Extract + Token Budget Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 `web_extract` 先抽正文再截断到 3k，并启用 gzip，降低 token 与慢站超时。

**Architecture:** 在 `flowy-web` 增加 `ArticleExtractor` trait + 默认 `DomSmoothieExtractor`；`HttpExtractProvider` 编排 fetch(+gzip) → 正文抽取 → 过短回退整页 → markdown → 3k 截断，并在 `ExtractedPage.extractor` 暴露 `readability|fullpage`。

**Tech Stack:** Rust, `dom_smoothie`, `reqwest` (gzip/deflate), existing `htmd` / `HttpExtractProvider`

**Spec:** `docs/superpowers/specs/2026-07-10-flowy-web-article-extract-design.md`

---

## 文件总览

| 操作 | 路径 | 职责 |
|------|------|------|
| Modify | `crates/agent/flowy-web/Cargo.toml` | `reqwest` 加 gzip/deflate；依赖 `dom_smoothie` |
| Modify | `crates/agent/flowy-web/src/types.rs` | `EXTRACT_CHAR_LIMIT=3000`；`ExtractedPage.extractor`；质量门常量 |
| Create | `crates/agent/flowy-web/src/provider/article.rs` | `ArticleExtractor` trait + `DomSmoothieExtractor` |
| Create | `crates/agent/flowy-web/tests/fixtures/article_with_chrome.html` | 含 nav/footer 的正文 fixture |
| Modify | `crates/agent/flowy-web/src/provider/mod.rs` | 导出 article 模块 |
| Modify | `crates/agent/flowy-web/src/provider/http_extract.rs` | 注入 extractor；编排回退；填 `extractor` 字段 |
| Modify | `crates/agent/flowy-web/src/tools/web_extract.rs` | 输出 `extractor:`；更新 description |
| Modify | `crates/agent/flowy-web/src/tools/web_search.rs` | description：snippet 够则少 extract |
| Modify | mock/tool 测试中构造 `ExtractedPage` 的地方 | 补 `extractor` 字段 |

---

### Task 1: Types — 3k budget + `extractor` field

**Files:**
- Modify: `crates/agent/flowy-web/src/types.rs`
- Modify: any in-crate `ExtractedPage { ... }` literals (tool mocks, http_extract tests)

- [ ] **Step 1: Write a failing unit test for the new constant**

In `types.rs` add under `#[cfg(test)]`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_char_limit_is_three_thousand() {
        assert_eq!(EXTRACT_CHAR_LIMIT, 3_000);
    }

    #[test]
    fn min_article_chars_gate_is_four_hundred() {
        assert_eq!(MIN_ARTICLE_CHARS, 400);
    }
}
```

Also add constants/fields that the test needs (or expect compile fail until Step 3).

- [ ] **Step 2: Run test — expect fail**

Run: `cargo test -p flowy-web extract_char_limit_is_three_thousand -- --nocapture`

Expected: FAIL (still 15_000 or missing `MIN_ARTICLE_CHARS`)

- [ ] **Step 3: Update types**

```rust
pub const EXTRACT_CHAR_LIMIT: usize = 3_000;
/// Quality gate: readability markdown shorter than this falls back to full page.
pub const MIN_ARTICLE_CHARS: usize = 400;

pub const EXTRACTOR_READABILITY: &str = "readability";
pub const EXTRACTOR_FULLPAGE: &str = "fullpage";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtractedPage {
    pub url: String,
    pub title: Option<String>,
    pub markdown: String,
    pub truncated: bool,
    pub provider: String,
    /// `"readability"` or `"fullpage"`
    pub extractor: String,
}
```

Fix all `ExtractedPage { ... }` construction sites to include `extractor` (use `EXTRACTOR_FULLPAGE` or `"fullpage"` temporarily in mocks until Task 3 wires real values).

- [ ] **Step 4: Run tests — expect pass for this constant; other tests may need field fixes**

Run: `cargo test -p flowy-web -- --nocapture`

Expected: compile + tests pass (mocks updated)

- [ ] **Step 5: Commit**

```bash
git add crates/agent/flowy-web/src/types.rs crates/agent/flowy-web/src/tools/web_extract.rs crates/agent/flowy-web/src/provider/http_extract.rs
git commit -m "feat(flowy-web): lower extract budget to 3k and add extractor field"
```

---

### Task 2: `ArticleExtractor` + DomSmoothie default

**Files:**
- Create: `crates/agent/flowy-web/src/provider/article.rs`
- Create: `crates/agent/flowy-web/tests/fixtures/article_with_chrome.html`
- Modify: `crates/agent/flowy-web/src/provider/mod.rs`
- Modify: `crates/agent/flowy-web/Cargo.toml`

- [ ] **Step 1: Add fixture**

`tests/fixtures/article_with_chrome.html`:

```html
<html>
<head><title>Limit Rules</title></head>
<body>
  <nav>Home | About | Contact | Login</nav>
  <aside class="ads">Buy insurance now</aside>
  <article>
    <h1>Beijing plate limits</h1>
    <p>On Friday the restricted tail numbers are 5 and 0 for the current rotation period.</p>
    <p>Out-of-town vehicles follow the same weekday tail-number rules inside the Fifth Ring Road.</p>
  </article>
  <footer>Copyright 2026 Local Portal</footer>
</body>
</html>
```

- [ ] **Step 2: Write failing extractor tests**

In `article.rs` (or its `#[cfg(test)]`):

```rust
#[test]
fn readability_strips_nav_and_keeps_article_body() {
    let html = include_str!("../../tests/fixtures/article_with_chrome.html");
    let ext = DomSmoothieExtractor::new();
    let article = ext.extract_article(html, Some("https://example.com/limits")).expect("article");
    let lower = article.html.to_lowercase();
    assert!(lower.contains("restricted tail numbers") || lower.contains("beijing plate"));
    assert!(!lower.contains("buy insurance now"), "ads should be stripped: {lower}");
    assert!(!lower.contains("login"), "nav chrome should be stripped: {lower}");
}

#[test]
fn empty_html_returns_none() {
    let ext = DomSmoothieExtractor::new();
    assert!(ext.extract_article("<html><body></body></html>", None).is_none());
}
```

- [ ] **Step 3: Run — expect fail**

Run: `cargo test -p flowy-web readability_strips_nav -- --nocapture`

Expected: FAIL (module/type missing)

- [ ] **Step 4: Add dependency + implement**

`Cargo.toml`:

```toml
dom_smoothie = "0.18"
```

`article.rs`:

```rust
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
```

Export from `provider/mod.rs`:

```rust
pub mod article;
pub use article::{ArticleExtractor, ArticleHtml, DomSmoothieExtractor};
```

If `dom_smoothie` API/integration fails (compile or fixture flaky), fall back to `legible` with the same `ArticleExtractor` surface — do not change the trait.

- [ ] **Step 5: Run — expect pass**

Run: `cargo test -p flowy-web article -- --nocapture`

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/agent/flowy-web/Cargo.toml crates/agent/flowy-web/src/provider/article.rs crates/agent/flowy-web/src/provider/mod.rs crates/agent/flowy-web/tests/fixtures/article_with_chrome.html Cargo.lock
git commit -m "feat(flowy-web): add ArticleExtractor with dom_smoothie default"
```

---

### Task 3: Wire HttpExtractProvider funnel + gzip

**Files:**
- Modify: `crates/agent/flowy-web/Cargo.toml` (`reqwest` features)
- Modify: `crates/agent/flowy-web/src/provider/http_extract.rs`

- [ ] **Step 1: Enable compression features**

```toml
reqwest = { workspace = true, features = ["json", "rustls-tls", "stream", "gzip", "deflate"], default-features = false }
```

- [ ] **Step 2: Write failing provider tests**

Add to `http_extract.rs` tests (use `allow_private_for_tests` + wiremock):

```rust
#[tokio::test]
async fn extract_uses_readability_on_chrome_heavy_page() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_raw(
            include_str!("../../tests/fixtures/article_with_chrome.html"),
            "text/html",
        ))
        .mount(&server)
        .await;

    let provider = HttpExtractProvider::new().allow_private_for_tests();
    let page = provider
        .extract(ExtractRequest { url: server.uri() })
        .await
        .unwrap();
    assert_eq!(page.extractor, EXTRACTOR_READABILITY);
    assert!(page.markdown.to_lowercase().contains("tail numbers")
        || page.markdown.to_lowercase().contains("fifth ring"));
    assert!(!page.markdown.to_lowercase().contains("buy insurance now"));
}

#[tokio::test]
async fn extract_falls_back_to_fullpage_when_article_too_thin() {
    let server = wiremock::MockServer::start().await;
    // Tiny body: readability may return None or < 400 chars after md → fullpage
    let body = "<html><head><title>X</title></head><body><p>Hi</p></body></html>";
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_raw(body, "text/html"))
        .mount(&server)
        .await;

    let provider = HttpExtractProvider::new().allow_private_for_tests();
    let page = provider
        .extract(ExtractRequest { url: server.uri() })
        .await
        .unwrap();
    assert_eq!(page.extractor, EXTRACTOR_FULLPAGE);
    assert!(page.markdown.to_lowercase().contains("hi"));
}
```

Update `truncates_long_markdown` expectation: still `truncated` with `EXTRACT_CHAR_LIMIT` (now 3000). Assert `page.extractor` is one of the two labels.

- [ ] **Step 3: Run — expect fail**

Run: `cargo test -p flowy-web extract_uses_readability -- --nocapture`

Expected: FAIL (`extractor` still missing real wiring / always fullpage)

- [ ] **Step 4: Implement funnel in `HttpExtractProvider`**

Hold `article: Arc<dyn ArticleExtractor>` (default `DomSmoothieExtractor`).

```rust
pub struct HttpExtractProvider {
    timeout: Duration,
    max_bytes: usize,
    allow_private: bool,
    article: Arc<dyn ArticleExtractor>,
}

impl Default for HttpExtractProvider {
    fn default() -> Self {
        Self {
            timeout: EXTRACT_TIMEOUT,
            max_bytes: EXTRACT_MAX_BYTES,
            allow_private: false,
            article: Arc::new(DomSmoothieExtractor::new()),
        }
    }
}

impl HttpExtractProvider {
    pub fn with_article_extractor(mut self, article: Arc<dyn ArticleExtractor>) -> Self {
        self.article = article;
        self
    }
}

async fn extract(...) {
    let (final_url, html) = self.fetch_html(&req.url).await?;
    let url_str = final_url.as_str();

    let (chosen_html, extractor_label, preferred_title) =
        match self.article.extract_article(&html, Some(url_str)) {
            Some(article) => {
                let (title_from_article, md_probe) = html_to_markdown(&article.html);
                if md_probe.chars().count() < MIN_ARTICLE_CHARS {
                    (html, EXTRACTOR_FULLPAGE.to_owned(), None)
                } else {
                    (
                        article.html,
                        EXTRACTOR_READABILITY.to_owned(),
                        article.title.or(title_from_article),
                    )
                }
            }
            None => (html, EXTRACTOR_FULLPAGE.to_owned(), None),
        };

    // If we already probed readability markdown above and kept it, avoid double convert:
    // prefer restructuring so readability path converts once.
    let (title, markdown) = if extractor_label == EXTRACTOR_READABILITY {
        // reconvert OR reuse md_probe — implement once-convert carefully
        let (t, md) = html_to_markdown(&chosen_html);
        (preferred_title.or(t), md)
    } else {
        html_to_markdown(&chosen_html)
    };

    let (markdown, truncated) = truncate_chars(&markdown, EXTRACT_CHAR_LIMIT);
    Ok(ExtractedPage {
        url: final_url.to_string(),
        title,
        markdown,
        truncated,
        provider: self.name().to_owned(),
        extractor: extractor_label,
    })
}
```

**Important:** avoid converting twice on the happy path. Clean structure:

```rust
let article = self.article.extract_article(&html, Some(url_str));
let (raw_html, extractor, title_hint) = match article {
    Some(a) => (a.html, EXTRACTOR_READABILITY, a.title),
    None => (html.clone(), EXTRACTOR_FULLPAGE, None),
};
let (title, markdown) = html_to_markdown(&raw_html);
let title = title_hint.or(title);
if extractor == EXTRACTOR_READABILITY && markdown.chars().count() < MIN_ARTICLE_CHARS {
    let (title, markdown) = html_to_markdown(&html);
    let (markdown, truncated) = truncate_chars(&markdown, EXTRACT_CHAR_LIMIT);
    return Ok(ExtractedPage { extractor: EXTRACTOR_FULLPAGE.into(), title, markdown, truncated, ... });
}
let (markdown, truncated) = truncate_chars(&markdown, EXTRACT_CHAR_LIMIT);
Ok(ExtractedPage { extractor: extractor.into(), ... })
```

gzip: no code change beyond Cargo features — `reqwest` auto-decompresses when feature enabled.

- [ ] **Step 5: Run — expect pass**

Run: `cargo test -p flowy-web -- --nocapture`

Expected: all PASS

- [ ] **Step 6: Commit**

```bash
git add crates/agent/flowy-web/Cargo.toml crates/agent/flowy-web/src/provider/http_extract.rs Cargo.lock
git commit -m "feat(flowy-web): readability-first extract with fullpage fallback and gzip"
```

---

### Task 4: Tool output + strategy descriptions

**Files:**
- Modify: `crates/agent/flowy-web/src/tools/web_extract.rs`
- Modify: `crates/agent/flowy-web/src/tools/web_search.rs`

- [ ] **Step 1: Write failing tool assertion**

In `web_extract` tests, extend mock `ExtractedPage` and add:

```rust
#[tokio::test]
async fn web_extract_tool_includes_extractor_label() {
    // Mock returns extractor: "readability"
    let r = tool.execute(json!({"urls":["https://example.com/a"]})).await;
    assert!(!r.is_error);
    assert!(r.content.contains("extractor: readability"), "{}", r.content);
}
```

Update mock `ExtractedPage` to set `extractor: "readability".into()`.

- [ ] **Step 2: Run — expect fail**

Run: `cargo test -p flowy-web web_extract_tool_includes_extractor -- --nocapture`

Expected: FAIL (format string missing extractor)

- [ ] **Step 3: Update format + descriptions**

Success format:

```rust
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
```

`web_extract` description:

```text
Fetch public URLs and return readable markdown of the main article body (boilerplate
stripped when possible, truncated for context). Use when you already have URLs and
snippets from web_search are not enough. Do not use Browser just to read public pages.
```

`web_search` description:

```text
Search the open web for current facts, news, traffic limits, weather, and other public
information. Prefer this before Browser. If search snippets already answer the question,
do not call web_extract. Only use web_extract when you need the page body beyond snippets.
```

- [ ] **Step 4: Run — expect pass**

Run: `cargo test -p flowy-web -- --nocapture`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/agent/flowy-web/src/tools/web_extract.rs crates/agent/flowy-web/src/tools/web_search.rs
git commit -m "feat(flowy-web): surface extractor label and prefer search-before-extract"
```

---

### Task 5: Final verification

- [ ] **Step 1: Full crate tests**

```bash
cargo test -p flowy-web -- --nocapture
cargo test -p nomi-agent --test bootstrap_test -- --nocapture
```

Expected: all PASS

- [ ] **Step 2: Optional live smoke (manual)**

Against `https://bj.bendibao.com/traffic/2026518/382877.shtm` in app: expect faster fetch, shorter body, `extractor: readability` or `fullpage` visible; no need for CI.

- [ ] **Step 3: Commit only if fixes were required**

---

## Spec coverage self-check

| Spec requirement | Task |
|------------------|------|
| gzip (+ deflate) | 3 |
| `ArticleExtractor` + default Readability/`dom_smoothie` | 2 |
| 15k → 3k | 1 |
| 空/过短 (<400) 回退整页 | 3 |
| `extractor` readability\|fullpage | 1, 3, 4 |
| tool descriptions search-first | 4 |
| Trafilatura / query-aware / LLM / 落盘 | intentionally no task |

## Consistency notes

- Labels: use constants `EXTRACTOR_READABILITY` / `EXTRACTOR_FULLPAGE` everywhere.
- `provider` stays `"http"`; path choice is `extractor`.
- Quality gate measures **markdown char count** after `html_to_markdown` of the article HTML.
- If `dom_smoothie` rejects non-absolute `document_url`, always pass `Some(final_url.as_str())` from provider (wiremock URIs are absolute).

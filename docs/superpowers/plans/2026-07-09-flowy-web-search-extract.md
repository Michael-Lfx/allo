# flowy-web Search + Extract Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在主 agent 上落地无 key 的 `web_search` / `web_extract`，crate `flowy-web` 分层清晰，并引导模型优先用它们而不是 Browser 查公开信息。

**Architecture:** 新建 `crates/agent/flowy-web`：`types` → `provider`（`SearchProvider` / `ExtractProvider` + DDG / HTTP 默认实现）→ `tools`（`WebSearchTool` / `WebExtractTool`）。`nomi-agent` bootstrap 在 `tools.web.enabled`（默认 true）时注册；不依赖 `nomi-browser` / `nomifun-knowledge`（SSRF + html→md 在 crate 内自包含，避免 agent→backend 依赖）。

**Tech Stack:** Rust 2024, `nomi-tools::Tool`, `reqwest`, `htmd`, `url`, `tokio`, `async-trait`, `thiserror`, `serde_json`

**Spec:** `docs/superpowers/specs/2026-07-09-flowy-web-search-extract-design.md`

---

## 文件总览

| 操作 | 路径 | 职责 |
|------|------|------|
| Create | `crates/agent/flowy-web/Cargo.toml` | crate 清单 |
| Create | `crates/agent/flowy-web/src/lib.rs` | 模块导出 |
| Create | `crates/agent/flowy-web/src/types.rs` | 查询/结果/错误类型 |
| Create | `crates/agent/flowy-web/src/provider/mod.rs` | trait + 子模块 |
| Create | `crates/agent/flowy-web/src/provider/search.rs` | `SearchProvider` |
| Create | `crates/agent/flowy-web/src/provider/extract.rs` | `ExtractProvider` |
| Create | `crates/agent/flowy-web/src/provider/duckduckgo.rs` | 无 key search |
| Create | `crates/agent/flowy-web/src/provider/http_extract.rs` | 无 key extract |
| Create | `crates/agent/flowy-web/src/provider/ssrf.rs` | URL / 私网校验 |
| Create | `crates/agent/flowy-web/src/provider/html_md.rs` | HTML→markdown |
| Create | `crates/agent/flowy-web/src/tools/mod.rs` | tool 模块 |
| Create | `crates/agent/flowy-web/src/tools/web_search.rs` | `WebSearchTool` |
| Create | `crates/agent/flowy-web/src/tools/web_extract.rs` | `WebExtractTool` |
| Create | `crates/agent/flowy-web/tests/fixtures/ddg_sample.html` | DDG 解析 fixture |
| Create | `crates/agent/flowy-web/tests/fixtures/page_sample.html` | extract fixture |
| Modify | `Cargo.toml` | workspace dep `flowy-web` |
| Modify | `crates/agent/nomi-config/src/config.rs` | `WebConfig` + `ToolsConfig.web` |
| Modify | `crates/agent/nomi-agent/Cargo.toml` | 依赖 `flowy-web` |
| Modify | `crates/agent/nomi-agent/src/bootstrap.rs` | 注册两个工具 |
| Modify | `crates/agent/nomi-agent/tests/bootstrap_test.rs` | 断言工具名 / 开关 |
| Modify | `crates/agent/nomi-browser/src/tool.rs` | DESCRIPTION 引导优先 search/extract |
| Modify | `crates/agent/nomi-agent/src/context.rs` | `browser_preset` 一句引导 |
| Modify | `crates/backend/nomifun-app/assets/builtin-assistants/rules/cowork.zh-CN.md` | WebSearch 节对齐真实工具名 |
| Modify | `crates/backend/nomifun-app/assets/builtin-assistants/rules/cowork.en-US.md` | 同上 |
| Modify | `crates/backend/nomifun-app/assets/builtin-assistants/rules/cowork.ru-RU.md` | 同上 |

---

### Task 1: Scaffold `flowy-web` + types

**Files:**
- Create: `crates/agent/flowy-web/Cargo.toml`
- Create: `crates/agent/flowy-web/src/lib.rs`
- Create: `crates/agent/flowy-web/src/types.rs`
- Modify: `Cargo.toml` (workspace.dependencies)

- [ ] **Step 1: Add workspace dependency**

In root `Cargo.toml` under `[workspace.dependencies]` (near other `nomi-*` agent crates), add:

```toml
flowy-web = { path = "crates/agent/flowy-web" }
```

(`members = ["crates/agent/*", ...]` already picks up the new crate.)

- [ ] **Step 2: Create crate manifest**

`crates/agent/flowy-web/Cargo.toml`:

```toml
[package]
name = "flowy-web"
description = "Keyless web_search / web_extract tools with pluggable providers"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
nomi-types.workspace = true
nomi-protocol.workspace = true
nomi-tools.workspace = true

async-trait.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tokio.workspace = true
tracing.workspace = true
reqwest = { workspace = true, features = ["json", "rustls-tls"], default-features = false }
htmd.workspace = true
url.workspace = true

[dev-dependencies]
tokio = { workspace = true, features = ["macros", "rt-multi-thread", "test-util"] }
wiremock.workspace = true
```

- [ ] **Step 3: Write types + failing compile check via lib**

`crates/agent/flowy-web/src/types.rs`:

```rust
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DEFAULT_SEARCH_COUNT: u32 = 5;
pub const MAX_SEARCH_COUNT: u32 = 10;
pub const MAX_EXTRACT_URLS: usize = 3;
pub const EXTRACT_CHAR_LIMIT: usize = 15_000;

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub query: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub rank: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchResult {
    pub provider: String,
    pub hits: Vec<SearchHit>,
}

#[derive(Debug, Clone)]
pub struct ExtractRequest {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtractedPage {
    pub url: String,
    pub title: Option<String>,
    pub markdown: String,
    pub truncated: bool,
    pub provider: String,
}

#[derive(Debug, Error)]
pub enum WebError {
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("blocked URL: {0}")]
    BlockedUrl(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("timeout: {0}")]
    Timeout(String),
    #[error("provider error: {0}")]
    Provider(String),
}
```

`crates/agent/flowy-web/src/lib.rs`:

```rust
pub mod provider;
pub mod tools;
pub mod types;

pub use types::*;
```

Stub empty `provider/mod.rs` and `tools/mod.rs` for now:

```rust
// provider/mod.rs
pub mod search;
pub mod extract;
// filled in later tasks
```

```rust
// tools/mod.rs
// filled in later tasks
```

And minimal trait stubs so the crate compiles — put in `provider/search.rs` / `provider/extract.rs`:

```rust
// provider/search.rs
use async_trait::async_trait;
use crate::types::{SearchQuery, SearchResult, WebError};

#[async_trait]
pub trait SearchProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn search(&self, query: SearchQuery) -> Result<SearchResult, WebError>;
}
```

```rust
// provider/extract.rs
use async_trait::async_trait;
use crate::types::{ExtractRequest, ExtractedPage, WebError};

#[async_trait]
pub trait ExtractProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn extract(&self, req: ExtractRequest) -> Result<ExtractedPage, WebError>;
}
```

Update `provider/mod.rs`:

```rust
pub mod extract;
pub mod search;

pub use extract::ExtractProvider;
pub use search::SearchProvider;
```

- [ ] **Step 4: Verify crate builds**

Run: `cargo check -p flowy-web`

Expected: success (or only unused warnings).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/agent/flowy-web
git commit -m "feat(flowy-web): scaffold crate and shared types"
```

---

### Task 2: SSRF guard + html→markdown helpers

**Files:**
- Create: `crates/agent/flowy-web/src/provider/ssrf.rs`
- Create: `crates/agent/flowy-web/src/provider/html_md.rs`
- Modify: `crates/agent/flowy-web/src/provider/mod.rs`

- [ ] **Step 1: Write failing SSRF unit tests** (in `ssrf.rs` `#[cfg(test)]`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejects_localhost() {
        let err = validate_extract_url("http://localhost/x", false)
            .await
            .unwrap_err();
        assert!(matches!(err, crate::types::WebError::BlockedUrl(_)));
    }

    #[tokio::test]
    async fn rejects_rfc1918_literal() {
        let err = validate_extract_url("http://192.168.1.1/", false)
            .await
            .unwrap_err();
        assert!(matches!(err, crate::types::WebError::BlockedUrl(_)));
    }

    #[tokio::test]
    async fn rejects_non_http_scheme() {
        let err = validate_extract_url("file:///etc/passwd", false)
            .await
            .unwrap_err();
        assert!(matches!(err, crate::types::WebError::InvalidArgument(_)));
    }

    #[tokio::test]
    async fn accepts_public_https_example() {
        // example.com resolves publicly in most environments; if DNS fails in
        // offline CI, skip — but prefer asserting Ok when resolution works.
        match validate_extract_url("https://example.com/", false).await {
            Ok(u) => assert_eq!(u.scheme(), "https"),
            Err(crate::types::WebError::BlockedUrl(_)) => panic!("example.com must not be blocked"),
            Err(crate::types::WebError::Network(_)) => {
                // offline / DNS flake — acceptable for this environment
            }
            Err(e) => panic!("unexpected: {e}"),
        }
    }
}
```

- [ ] **Step 2: Run tests — expect fail (module missing / fn missing)**

Run: `cargo test -p flowy-web ssrf -- --nocapture`

Expected: compile fail or test fail until implemented.

- [ ] **Step 3: Implement SSRF + html_md**

`ssrf.rs` — validate scheme, blocklist hosts (`localhost`, `metadata.google.internal`, `metadata.internal`), literal private IPs, and DNS resolution to non-global addresses (mirror knowledge crate spirit; map errors to `WebError::BlockedUrl` / `InvalidArgument` / `Network`).

`html_md.rs`:

```rust
pub fn html_to_markdown(html: &str) -> (Option<String>, String) {
    let title = extract_title(html);
    let converter = htmd::HtmlToMarkdown::builder()
        .skip_tags(vec!["script", "style", "head", "iframe", "noscript"])
        .build();
    let markdown = match converter.convert(html) {
        Ok(md) if !md.trim().is_empty() => md,
        _ => title
            .as_ref()
            .map(|t| format!("# {t}\n\n{}", strip_tags(html)))
            .unwrap_or_else(|| strip_tags(html)),
    };
    (title, markdown)
}

// extract_title / strip_tags: same approach as nomifun-knowledge source_url.rs
```

Also add:

```rust
pub fn truncate_chars(s: &str, max_chars: usize) -> (String, bool) {
    if s.chars().count() <= max_chars {
        return (s.to_owned(), false);
    }
    let truncated: String = s.chars().take(max_chars).collect();
    (truncated, true)
}
```

Export from `provider/mod.rs`.

- [ ] **Step 4: Run SSRF + a small html_md unit test — expect pass**

```rust
#[test]
fn html_to_markdown_keeps_title() {
    let (title, md) = html_to_markdown("<html><head><title>Hi</title></head><body><p>Hello</p></body></html>");
    assert_eq!(title.as_deref(), Some("Hi"));
    assert!(md.to_lowercase().contains("hello"));
}
```

Run: `cargo test -p flowy-web -- --nocapture`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/agent/flowy-web
git commit -m "feat(flowy-web): add SSRF guard and html-to-markdown helpers"
```

---

### Task 3: `DuckDuckGoSearchProvider` (fixture-based parse)

**Files:**
- Create: `crates/agent/flowy-web/src/provider/duckduckgo.rs`
- Create: `crates/agent/flowy-web/tests/fixtures/ddg_sample.html`
- Modify: `crates/agent/flowy-web/src/provider/mod.rs`

- [ ] **Step 1: Add fixture HTML**

Create a minimal DDG HTML-lite style fixture with 2–3 result blocks the parser will look for. Prefer stable selectors you control in the fixture, e.g. links with `class="result__a"` and snippets `class="result__snippet"` (classic DDG HTML). Example:

```html
<!-- tests/fixtures/ddg_sample.html -->
<html><body>
  <div class="result">
    <a class="result__a" href="https://example.com/a">Alpha Title</a>
    <a class="result__snippet">Alpha snippet</a>
  </div>
  <div class="result">
    <a class="result__a" href="https://example.com/b">Beta Title</a>
    <a class="result__snippet">Beta snippet</a>
  </div>
</body></html>
```

- [ ] **Step 2: Write failing parse test**

In `duckduckgo.rs`:

```rust
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
```

- [ ] **Step 3: Run — expect fail**

Run: `cargo test -p flowy-web parses_fixture_hits -- --nocapture`

Expected: FAIL (fn missing)

- [ ] **Step 4: Implement parser + provider**

```rust
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
    fn name(&self) -> &str { "duckduckgo" }

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
    // Scan for `class="result__a"` anchors and nearby `result__snippet`.
    // Keep this deliberately small and fixture-tested; live DDG markup drift
    // is acceptable risk for keyless v1.
    let mut hits = Vec::new();
    let mut rest = html;
    while hits.len() < limit as usize {
        let Some(a_idx) = rest.find("result__a") else { break };
        let after_class = &rest[a_idx..];
        let Some(href_rel) = after_class.find("href=\"") else {
            rest = &after_class[1..];
            continue;
        };
        let href_start = href_rel + "href=\"".len();
        let href_body = &after_class[href_start..];
        let Some(href_end) = href_body.find('"') else { break };
        let url = href_body[..href_end].to_owned();
        let after_href = &href_body[href_end + 1..];
        let Some(gt) = after_href.find('>') else { break };
        let title_body = &after_href[gt + 1..];
        let Some(close) = title_body.find('<') else { break };
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
```

Export `DuckDuckGoSearchProvider` from `provider/mod.rs`.

- [ ] **Step 5: Run unit tests — expect pass**

Run: `cargo test -p flowy-web duckduckgo -- --nocapture`

Expected: PASS (no live network required)

- [ ] **Step 6: Commit**

```bash
git add crates/agent/flowy-web
git commit -m "feat(flowy-web): add DuckDuckGo search provider with fixture parser"
```

---

### Task 4: `HttpExtractProvider`

**Files:**
- Create: `crates/agent/flowy-web/src/provider/http_extract.rs`
- Create: `crates/agent/flowy-web/tests/fixtures/page_sample.html`
- Modify: `crates/agent/flowy-web/src/provider/mod.rs`

- [ ] **Step 1: Write failing tests with wiremock**

```rust
#[tokio::test]
async fn extracts_public_page_via_mock() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_raw(
            include_str!("../../tests/fixtures/page_sample.html"),
            "text/html",
        ))
        .mount(&server)
        .await;

    // allow_private_for_tests so loopback mock works
    let provider = HttpExtractProvider::new().allow_private_for_tests();
    let page = provider
        .extract(ExtractRequest {
            url: server.uri(),
        })
        .await
        .unwrap();
    assert_eq!(page.title.as_deref(), Some("Sample"));
    assert!(page.markdown.contains("Hello world"));
    assert!(!page.truncated);
    assert_eq!(page.provider, "http");
}

#[tokio::test]
async fn truncates_long_markdown() {
    let server = wiremock::MockServer::start().await;
    let body = format!(
        "<html><head><title>T</title></head><body><p>{}</p></body></html>",
        "x".repeat(20_000)
    );
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_raw(body, "text/html"))
        .mount(&server)
        .await;

    let provider = HttpExtractProvider::new().allow_private_for_tests();
    let page = provider
        .extract(ExtractRequest { url: server.uri() })
        .await
        .unwrap();
    assert!(page.truncated);
    assert!(page.markdown.chars().count() <= EXTRACT_CHAR_LIMIT);
}

#[tokio::test]
async fn blocks_private_by_default() {
    let provider = HttpExtractProvider::new();
    let err = provider
        .extract(ExtractRequest {
            url: "http://127.0.0.1/".into(),
        })
        .await
        .unwrap_err();
    assert!(matches!(err, WebError::BlockedUrl(_)));
}
```

Fixture `page_sample.html`:

```html
<html><head><title>Sample</title></head><body><p>Hello world</p></body></html>
```

- [ ] **Step 2: Run — expect fail**

Run: `cargo test -p flowy-web http_extract -- --nocapture`

Expected: FAIL

- [ ] **Step 3: Implement `HttpExtractProvider`**

- `new()` / `allow_private_for_tests()` / timeout ~20s / body cap ~2 MiB
- `extract`: `validate_extract_url` → GET → `html_to_markdown` → `truncate_chars(..., EXTRACT_CHAR_LIMIT)`
- Follow redirects with re-validation per hop (or disable redirects and document; prefer re-validate like knowledge)

- [ ] **Step 4: Run — expect pass**

Run: `cargo test -p flowy-web -- --nocapture`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/agent/flowy-web
git commit -m "feat(flowy-web): add HTTP extract provider with SSRF and truncation"
```

---

### Task 5: `WebSearchTool` + `WebExtractTool`

**Files:**
- Create: `crates/agent/flowy-web/src/tools/web_search.rs`
- Create: `crates/agent/flowy-web/src/tools/web_extract.rs`
- Modify: `crates/agent/flowy-web/src/tools/mod.rs`
- Modify: `crates/agent/flowy-web/src/lib.rs`

- [ ] **Step 1: Write failing tool tests** (mock providers in test module)

```rust
struct MockSearch;
#[async_trait]
impl SearchProvider for MockSearch {
    fn name(&self) -> &str { "mock" }
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
    let r = tool.execute(json!({"query": "beijing", "count": 3})).await;
    assert!(!r.is_error);
    assert!(r.content.contains("https://example.com"));
    assert!(r.content.contains("R:beijing"));
}
```

Similar for extract: mock returns one page; tool rejects `urls.len() > 3`; serial partial failure returns mixed results; all failures → `is_error: true`.

- [ ] **Step 2: Run — expect fail**

Run: `cargo test -p flowy-web tools -- --nocapture`

Expected: FAIL

- [ ] **Step 3: Implement tools**

`WebSearchTool`:
- `name()` → `"web_search"`
- `description()` → explain: use for current facts / news / limits; prefer before Browser; then `web_extract` for full pages
- `input_schema`: `query` required, `count` optional integer
- `is_concurrency_safe` → `true`
- `category` → `ToolCategory::Info`
- `execute`: parse args, default count `DEFAULT_SEARCH_COUNT`, clamp to `MAX_SEARCH_COUNT`, call provider, format numbered list `title\nurl\nsnippet` + `provider=` footer

`WebExtractTool`:
- `name()` → `"web_extract"`
- description: use when you already have URLs; do not use Browser just to read public pages
- schema: `urls` array required
- `is_concurrency_safe` → `true` (tool-level; internal URL loop still serial)
- `category` → `Info`
- `execute`: require 1..=`MAX_EXTRACT_URLS` urls; **for url in urls { extract.await }** serial; collect per-URL success/error JSON or markdown sections; if every URL failed → `is_error: true`

Export from `tools/mod.rs` and `lib.rs`.

- [ ] **Step 4: Run — expect pass**

Run: `cargo test -p flowy-web -- --nocapture`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/agent/flowy-web
git commit -m "feat(flowy-web): add web_search and web_extract tools"
```

---

### Task 6: Config `tools.web.enabled`

**Files:**
- Modify: `crates/agent/nomi-config/src/config.rs`

- [ ] **Step 1: Add `WebConfig` next to `BrowserConfig`**

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebConfig {
    /// Register `web_search` / `web_extract`. Default true (keyless providers).
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}
```

Add field on `ToolsConfig`:

```rust
#[serde(default)]
pub web: WebConfig,
```

Update `ToolsConfig::default()` to include `web: WebConfig::default()`.

Confirm `default_true` already exists in this file (it does for `in_process_spawn`).

- [ ] **Step 2: Run config crate tests**

Run: `cargo test -p nomi-config -- --nocapture`

Expected: PASS (serde defaults still deserialize old configs without `web`)

- [ ] **Step 3: Commit**

```bash
git add crates/agent/nomi-config/src/config.rs
git commit -m "feat(config): add tools.web.enabled for search/extract tools"
```

---

### Task 7: Bootstrap registration + tests

**Files:**
- Modify: `crates/agent/nomi-agent/Cargo.toml`
- Modify: `crates/agent/nomi-agent/src/bootstrap.rs`
- Modify: `crates/agent/nomi-agent/tests/bootstrap_test.rs`

- [ ] **Step 1: Add dependency**

In `nomi-agent/Cargo.toml` `[dependencies]`:

```toml
flowy-web.workspace = true
```

- [ ] **Step 2: Write failing bootstrap assertions**

In `bootstrap_registers_all_expected_tools`, extend expected names:

```rust
for expected in &[
    "Read", "Write", "Edit", "Bash", "Grep", "Glob",
    "web_search", "web_extract",
] {
```

Add new test:

```rust
#[tokio::test]
async fn bootstrap_web_tools_gated_off_when_disabled() {
    let mut config = minimal_config();
    config.tools.web.enabled = false;
    let result = AgentBootstrap::new(config, "/tmp/test-workspace", null_output())
        .build()
        .await
        .unwrap();
    let names = result.engine.tool_names();
    assert!(!names.iter().any(|n| n == "web_search"));
    assert!(!names.iter().any(|n| n == "web_extract"));
}
```

- [ ] **Step 3: Run — expect fail**

Run: `cargo test -p nomi-agent bootstrap_registers_all_expected_tools bootstrap_web_tools -- --nocapture`

Expected: FAIL (missing tools)

- [ ] **Step 4: Register in bootstrap**

After Grep/Glob registration (near line ~516), add:

```rust
if self.config.tools.web.enabled {
    let search = std::sync::Arc::new(flowy_web::provider::DuckDuckGoSearchProvider::new());
    let extract = std::sync::Arc::new(flowy_web::provider::HttpExtractProvider::new());
    registry.register(Box::new(flowy_web::tools::WebSearchTool::new(search)));
    registry.register(Box::new(flowy_web::tools::WebExtractTool::new(extract)));
}
```

(Adjust paths to match actual `pub use` exports.)

- [ ] **Step 5: Run — expect pass**

Run: `cargo test -p nomi-agent bootstrap_ -- --nocapture`

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/agent/nomi-agent
git commit -m "feat(agent): register web_search and web_extract when tools.web.enabled"
```

---

### Task 8: Prompt / Browser strategy copy

**Files:**
- Modify: `crates/agent/nomi-browser/src/tool.rs` (`DESCRIPTION` const ~line 100)
- Modify: `crates/agent/nomi-agent/src/context.rs` (`browser_preset`)
- Modify: `crates/backend/nomifun-app/assets/builtin-assistants/rules/cowork.zh-CN.md` (section 9)
- Modify: `cowork.en-US.md`, `cowork.ru-RU.md`（对应 WebSearch 节）

- [ ] **Step 1: Update Browser DESCRIPTION**

At the top of `DESCRIPTION`, after the first sentence, insert:

```text
For open-web facts, news, or reading public pages, prefer `web_search` then
`web_extract`. Use Browser only when you must interact (click, login, fill forms)
or the page cannot be fetched as static content.
```

Keep the CORE LOOP section intact.

- [ ] **Step 2: Update `browser_preset`**

```rust
fn browser_preset() -> &'static str {
    "[Browsing the web] Prefer `web_search` then `web_extract` for public information. \
Use the `Browser` tool when a page must be opened, rendered, inspected, or operated \
interactively. Do not ask the user for permission to browse. After each Browser \
navigation or interaction run `observe` for fresh refs before acting again."
}
```

- [ ] **Step 3: Update cowork rules WebSearch section**

Replace vague “WebSearch” with concrete tool names and priority:

```markdown
### 9. web_search / web_extract

- 查公开事实、新闻、限号等：先 `web_search`，需要正文再用 `web_extract`
- 需要点击、登录、填表时才用 `Browser`
- 回答后尽量附带 Sources（含 URL）
```

Mirror in English rules if present.

- [ ] **Step 4: Compile check**

Run: `cargo check -p nomi-agent --features browser-use`

Expected: success

- [ ] **Step 5: Commit**

```bash
git add crates/agent/nomi-browser/src/tool.rs \
  crates/agent/nomi-agent/src/context.rs \
  crates/backend/nomifun-app/assets/builtin-assistants/rules/
git commit -m "docs(agent): prefer web_search/web_extract over Browser for public info"
```

---

### Task 9: Final verification

- [ ] **Step 1: Run full flowy-web + bootstrap tests**

```bash
cargo test -p flowy-web -- --nocapture
cargo test -p nomi-agent bootstrap_ -- --nocapture
```

Expected: all PASS; no network-required tests in CI path.

- [ ] **Step 2: Manual smoke (optional checklist, not CI)**

1. Enable tools (default), ask「北京今天限号多少」
2. Confirm tool calls include `web_search` (and maybe `web_extract`)
3. Confirm Browser is not the first/only strategy

- [ ] **Step 3: Final commit if any leftover fixes**

Only if Step 1 required fixes; otherwise done.

---

## Spec coverage self-check

| Spec requirement | Task |
|------------------|------|
| Crate `flowy-web` + types/provider/tools | 1–5 |
| `SearchProvider` / `ExtractProvider` | 1, 3, 4 |
| DDG default search (no key) | 3 |
| HTTP extract (no key) | 4 |
| Tool names `web_search` / `web_extract` | 5 |
| count default 5 / max 10 | 5 |
| extract max 3 URLs, serial concurrency 1 | 5 |
| 15_000 char truncate + `truncated` | 4 |
| SSRF private URL reject | 2, 4 |
| `tools.web.enabled` default true | 6–7 |
| Bootstrap register | 7 |
| Prompt / Browser / rules strategy | 8 |
| Fixture tests, no CI public net | 3–5, 9 |
| Non-goals (routing, read_file, Browser extract) | intentionally no task |

## Placeholder / consistency notes

- Provider names: `"duckduckgo"` / `"http"` — tools must print the same strings from `provider.name()`.
- Config field: `tools.web.enabled` only (not separate search/extract flags) per approved spec.
- Do **not** add `nomifun-knowledge` as a dependency of `flowy-web` or `nomi-agent`.

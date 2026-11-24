# ClawHub Skill Market Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 `flowy18_wip` 里加一个纯 Rust 技能市场引擎：官方技能走包内 JSON，社区技能只缓存 ClawHub 第一页卡片，审核通过后才保留，正文只在安装时拉取。

**Architecture:** 新 crate `flowy-skills` 拥有目录、SQLite 缓存、排序保持、审核过滤、安装状态机和 Agent 可见的技能摘要。HTTP 走 `flowy-net` 的阻塞客户端，调用放在 `spawn_blocking`。`flowy-app` 里现有的官方 `SKILLS` 卡片本计划不改；界面只读引擎返回的 `Shelf`。不自建服务端，不翻全库，不把插件 / npm 端点接进来。

**Tech Stack:** Rust 1.100、`flowy-net`（阻塞 `reqwest` + 系统代理）、`rusqlite` `bundled`、`serde` / `serde_json`、`sha2`、`zip`、tokio 已有运行时上的 `spawn_blocking`。

## Global Constraints

- 基址固定为 `https://clawhub.ai`。读接口不带 token。
- 身份键是 `owner_handle + slug + version`。请求详情、审核、下载时必须带 `ownerHandle`。
- 冷启动网络预算：官方 JSON 零请求；后台最多 `GET /api/v1/trending?kind=skills` 一次。分类只在用户点进该分类时请求一次，`limit=20`，`sort=downloads`，`nonSuspiciousOnly=true`，不跟随 `nextCursor`。
- 搜索停手 300ms 后发一次 `GET /api/v1/search`，`limit=15`，`nonSuspiciousOnly=true`，相同 `q + category` 15 分钟内用缓存。
- 分类表 `GET /api/v1/skills/-/categories` 缓存 24 小时。启动时不预拉 14 个分类。
- 每一页卡片之后发一次 `POST /api/v1/skills/-/security-verdicts`，每批最多 100 条。默认货架只保留 `decision=pass` 且 `security.status=clean`。删除后保持接口原顺序。
- `GET /api/v1/skills/{slug}/verify` 只在用户安装时调用一次。`ok != true` 或结论不是 pass 则不下载。
- 单包上限 8 MiB（8_388_608 字节），文件数上限 200，路径深度上限 8。拒绝符号链接、绝对路径、`..`。
- GitHub 交接只接受主机 `github.com` 的 `archiveUrl`，检出后核对 `contentHash`。`sourceRef` 必须是 `public-github`。
- 社区技能 `auto_invoke=false`，`scripts_enabled=false`。官方 JSON 里的技能可以 `auto_invoke=true`。
- `429` / `503` 只重试一次，等待 `Retry-After` 秒数加 0–250ms 抖动，然后停止并保留旧缓存。
- 错误体按 `text/plain` 读取。界面线程不发 HTTP、不查 SQLite。
- 不调用 `/api/v1/plugins`、`/api/v1/packages`、`/api/npm`。
- 官方名单打进二进制：`crates/flowy-skills/assets/official-skills.json`。

---

## File structure

- Create: `crates/flowy-skills/Cargo.toml`
- Create: `crates/flowy-skills/assets/official-skills.json`
- Create: `crates/flowy-skills/src/lib.rs`
- Create: `crates/flowy-skills/src/id.rs`
- Create: `crates/flowy-skills/src/shelf.rs`
- Create: `crates/flowy-skills/src/plan.rs`
- Create: `crates/flowy-skills/src/catalog.rs`
- Create: `crates/flowy-skills/src/verdict.rs`
- Create: `crates/flowy-skills/src/store.rs`
- Create: `crates/flowy-skills/src/official.rs`
- Create: `crates/flowy-skills/src/archive.rs`
- Create: `crates/flowy-skills/src/scan.rs`
- Create: `crates/flowy-skills/src/install.rs`
- Create: `crates/flowy-skills/src/registry.rs`
- Modify: `Cargo.toml` workspace members、default-members、dependencies
- Modify: `docs/plans/README.md` 索引一行

`flowy-app/src/app/skills.rs` 的官方网格留到后续界面计划。本计划交付的是 `SkillMarket`，界面以后调用它。

---

### Task 1: 身份与货架过滤

**Files:**
- Create: `crates/flowy-skills/Cargo.toml`
- Create: `crates/flowy-skills/src/lib.rs`
- Create: `crates/flowy-skills/src/id.rs`
- Create: `crates/flowy-skills/src/shelf.rs`
- Modify: `Cargo.toml`

**Interfaces:**
- Consumes: 无
- Produces:
  - `SkillId { owner_handle: String, slug: String, version: String }`
  - `fn parse_skill_id(owner_handle: &str, slug: &str, version: &str) -> Result<SkillId, IdError>`
  - `Card { id: SkillId, summary: String, categories: Vec<String> }`
  - `fn keep_passing(cards: &[Card], passing: &HashSet<SkillId>) -> Vec<Card>`

- [x] **Step 1: 把 crate 加进 workspace**

在根 `Cargo.toml` 的 `members` 和 `default-members` 两个数组末尾各加一行 `"crates/flowy-skills"`。在 `[workspace.dependencies]` 增加：

```toml
flowy-skills = { path = "crates/flowy-skills" }
rusqlite = { version = "0.32", features = ["bundled"] }
zip = { version = "2", default-features = false, features = ["deflate"] }
```

`crates/flowy-skills/Cargo.toml`：

```toml
[package]
name = "flowy-skills"
description = "Local skill catalog, cache, and installer"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
publish = false

[dependencies]
flowy-net = { workspace = true }
rusqlite = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
sha2 = { workspace = true }
thiserror = { workspace = true }
zip = { workspace = true }

[lints]
workspace = true
```

- [x] **Step 2: 写失败测试**

`crates/flowy-skills/src/id.rs` 底部：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_and_slug_are_required_and_normalized() {
        let id = parse_skill_id(" @Awspace ", "pdf", "1.2.3").unwrap();
        assert_eq!(id.owner_handle, "awspace");
        assert_eq!(id.slug, "pdf");
        assert_eq!(id.version, "1.2.3");
        assert!(parse_skill_id("", "pdf", "1.2.3").is_err());
        assert!(parse_skill_id("awspace", "pdf", "").is_err());
    }
}
```

`crates/flowy-skills/src/shelf.rs` 底部：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn card(owner: &str, slug: &str) -> Card {
        Card {
            id: parse_skill_id(owner, slug, "1.0.0").unwrap(),
            summary: slug.into(),
            categories: vec!["knowledge".into()],
        }
    }

    #[test]
    fn filter_drops_failures_and_keeps_api_order() {
        let cards = vec![card("a", "one"), card("b", "two"), card("c", "three")];
        let mut passing = HashSet::new();
        passing.insert(cards[0].id.clone());
        passing.insert(cards[2].id.clone());
        let kept = keep_passing(&cards, &passing);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].id.slug, "one");
        assert_eq!(kept[1].id.slug, "three");
    }
}
```

- [x] **Step 3: 跑测试确认失败**

Run: `cargo test -p flowy-skills --lib id::tests shelf::tests`

Expected: FAIL，模块或函数不存在。

- [x] **Step 4: 写实现**

`id.rs`：`owner_handle` trim、去掉前缀 `@`、转小写。`slug` 和 `version` 只 trim。任一为空返回 `IdError::Empty`。

`shelf.rs`：`keep_passing` 按 `cards` 原顺序过滤，`passing.contains(&card.id)` 为真才留下。不要按下载量重排。

`lib.rs` 声明 `mod id; mod shelf;` 并 `pub use` 上述类型和函数。

- [x] **Step 5: 跑测试确认通过**

Run: `cargo test -p flowy-skills --lib id::tests shelf::tests`

Expected: PASS

- [x] **Step 6: Commit**

```bash
git add Cargo.toml crates/flowy-skills
git commit -m "feat: add skill identity and order-preserving shelf filter"
```

---

### Task 2: 请求计划

**Files:**
- Create: `crates/flowy-skills/src/plan.rs`
- Modify: `crates/flowy-skills/src/lib.rs`

**Interfaces:**
- Consumes: 无
- Produces:
  - `enum Fetch { Trending, Category { slug: String }, Search { query: String, category: Option<String> }, Categories, Verdicts }`
  - `fn trending_url() -> &'static str`
  - `fn category_url(slug: &str) -> Result<String, PlanError>`
  - `fn search_url(query: &str, category: Option<&str>) -> Result<String, PlanError>`
  - `const CATEGORY_TTL_SECS: u64 = 86_400`
  - `const SEARCH_TTL_SECS: u64 = 900`
  - `const PAGE_LIMIT_CATEGORY: u32 = 20`
  - `const PAGE_LIMIT_SEARCH: u32 = 15`
  - `const MAX_PACKAGE_BYTES: u64 = 8_388_608`

- [x] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_url_is_one_page_of_downloads() {
        let url = category_url("knowledge").unwrap();
        assert_eq!(
            url,
            "https://clawhub.ai/api/v1/skills?category=knowledge&sort=downloads&limit=20&nonSuspiciousOnly=true"
        );
        assert!(category_url("nope/slash").is_err());
        assert!(!url.contains("cursor"));
    }

    #[test]
    fn search_url_keeps_category_when_present() {
        let url = search_url("pdf form", Some("knowledge")).unwrap();
        assert!(url.contains("q=pdf%20form") || url.contains("q=pdf+form"));
        assert!(url.contains("limit=15"));
        assert!(url.contains("nonSuspiciousOnly=true"));
        assert!(url.contains("category=knowledge"));
        assert!(search_url("   ", None).is_err());
    }
}
```

- [x] **Step 2: 跑测试确认失败**

Run: `cargo test -p flowy-skills --lib plan::tests`

Expected: FAIL

- [x] **Step 3: 写实现**

`trending_url` 返回 `https://clawhub.ai/api/v1/trending?kind=skills`。

`category_url` 只接受 `^[a-z0-9-]+$`。用 `url::form_urlencoded` 或手写百分号编码，不要拼接未编码的查询。本 crate 不新增 `url` 依赖时，查询值只放行该字符集，空格编码为 `%20`。

`search_url` 在 trim 后为空时返回 `PlanError::EmptyQuery`。有分类时追加 `&category=`。两个 URL 都不写 `cursor`。

- [x] **Step 4: 跑测试确认通过**

Run: `cargo test -p flowy-skills --lib plan::tests`

Expected: PASS

- [x] **Step 5: Commit**

```bash
git add crates/flowy-skills/src/plan.rs crates/flowy-skills/src/lib.rs
git commit -m "feat: pin skill market requests to one page"
```

---

### Task 3: 目录解析

**Files:**
- Create: `crates/flowy-skills/src/catalog.rs`
- Modify: `crates/flowy-skills/src/lib.rs`

**Interfaces:**
- Consumes: `parse_skill_id`, `Card`
- Produces:
  - `fn parse_skill_page(body: &str) -> Result<Vec<Card>, CatalogError>`
  - `fn parse_search_page(body: &str) -> Result<Vec<Card>, CatalogError>`
  - `fn parse_categories(body: &str) -> Result<Vec<Category>, CatalogError>`
  - `Category { slug: String, label: String }`
  - 没有 `ownerHandle`、没有 `latestVersion.version`、或 `version` 为空的条目丢弃，不报错中断整页

- [x] **Step 1: 写失败测试**

用两段固定 JSON。列表项：

```json
{
  "items": [
    {
      "ownerHandle": "pskoett",
      "slug": "self-improving-agent",
      "summary": "Captures learnings",
      "categories": ["agents", "knowledge"],
      "latestVersion": { "version": "4.0.5" }
    },
    {
      "ownerHandle": "awspace",
      "slug": "pdf",
      "summary": "no version",
      "latestVersion": null
    }
  ]
}
```

期望只留下第一条，`owner_handle=pskoett`，`version=4.0.5`，分类顺序不变。

搜索体用 `results` 数组。条目带顶层 `version: null` 且没有 `latestVersion` 时丢弃。`ownerHandle` 缺失时丢弃。合法条目仍按数组顺序返回。

分类体：

```json
{"categories":[{"slug":"knowledge","label":"Knowledge","description":"","icon":"book-open"}]}
```

- [x] **Step 2: 跑测试确认失败**

Run: `cargo test -p flowy-skills --lib catalog::tests`

Expected: FAIL

- [x] **Step 3: 写实现**

用 `serde_json::Value` 读取，不要为 ClawHub 的可选嵌套字段建一个会在缺字段时失败的严格结构体。`parse_skill_page` 读 `items`，`parse_search_page` 读 `results`。版本优先取 `latestVersion.version` 的非空字符串，否则取顶层 `version` 的非空字符串。`summary` 缺省为空字符串。`categories` 只收字符串。

- [x] **Step 4: 跑测试确认通过**

Run: `cargo test -p flowy-skills --lib catalog::tests`

Expected: PASS

- [x] **Step 5: Commit**

```bash
git add crates/flowy-skills/src/catalog.rs crates/flowy-skills/src/lib.rs
git commit -m "feat: parse clawhub pages without dropping the whole page"
```

---

### Task 4: 审核批处理

**Files:**
- Create: `crates/flowy-skills/src/verdict.rs`
- Modify: `crates/flowy-skills/src/lib.rs`

**Interfaces:**
- Consumes: `SkillId`, `Card`, `keep_passing`
- Produces:
  - `fn verdict_body(cards: &[Card]) -> Result<serde_json::Value, VerdictError>`
  - `fn passing_ids(body: &str) -> Result<HashSet<SkillId>, VerdictError>`
  - `fn apply_verdicts(cards: &[Card], body: &str) -> Result<Vec<Card>, VerdictError>`
  - 一批超过 100 条返回 `VerdictError::BatchTooLarge`

- [x] **Step 1: 写失败测试**

`verdict_body` 对两条卡片生成：

```json
{"items":[
  {"slug":"one","ownerHandle":"a","version":"1.0.0"},
  {"slug":"two","ownerHandle":"b","version":"1.0.0"}
]}
```

响应里第一条 `ok: true, decision: "pass", security.status: "clean"`，第二条 `decision: "fail"`。`apply_verdicts` 只留下第一条，且仍在原下标顺序。`decision: "pass"` 但 `security.status: "warn"` 的条目不留下。101 条卡片让 `verdict_body` 失败。

- [x] **Step 2: 跑测试确认失败**

Run: `cargo test -p flowy-skills --lib verdict::tests`

Expected: FAIL

- [x] **Step 3: 写实现**

`passing_ids` 遍历 `items`。同时满足 `decision == "pass"` 和 `security.status == "clean"` 才插入 `SkillId`。用响应里的 `requestedOwnerHandle`（没有则用 `publisherHandle`）、`requestedSlug`（没有则用 `slug`）、`requestedVersion`（没有则用 `version`）调用 `parse_skill_id`。字段不够的条目跳过。

`apply_verdicts` 调用 `passing_ids` 再调用 `keep_passing`。

- [x] **Step 4: 跑测试确认通过**

Run: `cargo test -p flowy-skills --lib verdict::tests`

Expected: PASS

- [x] **Step 5: Commit**

```bash
git add crates/flowy-skills/src/verdict.rs crates/flowy-skills/src/lib.rs
git commit -m "feat: keep only clean passing skill cards"
```

---

### Task 5: SQLite 缓存

**Files:**
- Create: `crates/flowy-skills/src/store.rs`
- Modify: `crates/flowy-skills/src/lib.rs`
- Modify: `crates/flowy-skills/Cargo.toml`（Task 1 已声明 `rusqlite`）

**Interfaces:**
- Consumes: `Card`, `Category`, `SEARCH_TTL_SECS`, `CATEGORY_TTL_SECS`
- Produces:
  - `struct Store { /* rusqlite::Connection，不对外 */ }`
  - `fn Store::open(path: &Path) -> Result<Store, StoreError>`
  - `fn Store::replace_shelf(&self, shelf_key: &str, cards: &[Card], fetched_at_unix: i64) -> Result<(), StoreError>`
  - `fn Store::shelf(&self, shelf_key: &str) -> Result<Vec<Card>, StoreError>`
  - `fn Store::search_fresh(&self, cache_key: &str, now_unix: i64) -> Result<Option<Vec<Card>>, StoreError>`
  - `fn Store::save_search(&self, cache_key: &str, cards: &[Card], fetched_at_unix: i64) -> Result<(), StoreError>`
  - `fn Store::categories_fresh(&self, now_unix: i64) -> Result<Option<Vec<Category>>, StoreError>`

- [x] **Step 1: 写失败测试**

用 `tempfile` 不引入新依赖：`std::env::temp_dir()` 加测试名和进程 id 建目录。打开库，写入 shelf key `trending` 的两张卡片，再读出，顺序与写入一致。`search_fresh` 在 `now - fetched_at < 900` 时返回 `Some`，等于 900 时返回 `None`。`categories_fresh` 以 86400 秒为界，规则相同。

- [x] **Step 2: 跑测试确认失败**

Run: `cargo test -p flowy-skills --lib store::tests`

Expected: FAIL

- [x] **Step 3: 写实现**

建表：

```sql
CREATE TABLE IF NOT EXISTS shelf_card (
  shelf_key TEXT NOT NULL,
  position INTEGER NOT NULL,
  owner_handle TEXT NOT NULL,
  slug TEXT NOT NULL,
  version TEXT NOT NULL,
  summary TEXT NOT NULL,
  categories_json TEXT NOT NULL,
  fetched_at INTEGER NOT NULL,
  PRIMARY KEY (shelf_key, position)
);
CREATE TABLE IF NOT EXISTS search_cache (
  cache_key TEXT PRIMARY KEY,
  body_json TEXT NOT NULL,
  fetched_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS category_cache (
  slug TEXT PRIMARY KEY,
  label TEXT NOT NULL,
  position INTEGER NOT NULL,
  fetched_at INTEGER NOT NULL
);
```

`replace_shelf` 在一个事务里删除该 `shelf_key` 再按切片下标插入。连接使用 `OpenFlags::SQLITE_OPEN_READ_WRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_FULL_MUTEX`。`Store` 不实现 `Sync` 的共享跨线程借用；调用方把整个 `Store` 放进 `spawn_blocking`。

- [x] **Step 4: 跑测试确认通过**

Run: `cargo test -p flowy-skills --lib store::tests`

Expected: PASS

- [x] **Step 5: Commit**

```bash
git add crates/flowy-skills/src/store.rs crates/flowy-skills/src/lib.rs
git commit -m "feat: cache skill shelves and searches in sqlite"
```

---

### Task 6: 官方名单与传输

**Files:**
- Create: `crates/flowy-skills/assets/official-skills.json`
- Create: `crates/flowy-skills/src/official.rs`
- Create: `crates/flowy-skills/src/transport.rs`
- Modify: `crates/flowy-skills/src/lib.rs`

**Interfaces:**
- Consumes: `SkillId`
- Produces:
  - `OfficialSkill { id: SkillId, title: String, summary: String, auto_invoke: bool }`
  - `fn bundled_official() -> Result<Vec<OfficialSkill>, OfficialError>`
  - `trait CatalogTransport { fn get_text(&self, url: &str) -> Result<HttpText, TransportError>; fn post_json(&self, url: &str, body: &serde_json::Value) -> Result<HttpText, TransportError>; }`
  - `HttpText { status: u16, body: String, retry_after_secs: Option<u64> }`
  - `struct NetTransport`
  - `fn NetTransport::new() -> Result<Self, TransportError>`，用 `flowy_net::configure` 包一层带 10 秒连接超时、20 秒总超时的阻塞客户端。不要调用 `flowy_net::blocking_client()`，那个函数没有这些超时。
  - `fn retry_delay(status: u16, retry_after_secs: Option<u64>, jitter_ms: u64) -> Option<u64>`，单位是毫秒

- [x] **Step 1: 写失败测试和包内 JSON**

`official-skills.json` 放两条，`auto_invoke` 都是 true：

```json
[
  {
    "owner_handle": "flowy",
    "slug": "excel",
    "version": "1.0.0",
    "title": "Excel",
    "summary": "Spreadsheets",
    "auto_invoke": true
  }
]
```

测试断言 `bundled_official()` 至少含 `flowy/excel`，且 `auto_invoke` 为 true。缺 `version` 的数组在单独的 `parse_official` 测试里让整个文件失败（官方名单是我们自己的，不像社区页那样跳过坏行）。

`retry_delay(429, Some(2), 10)` 等于 2010 毫秒。`retry_delay(404, Some(2), 10)` 为 `None`。`retry_delay(503, None, 10)` 等于 1010 毫秒（缺省等待 1 秒）。

- [x] **Step 2: 跑测试确认失败**

Run: `cargo test -p flowy-skills --lib official::tests transport::tests`

Expected: FAIL

- [x] **Step 3: 写实现**

`bundled_official` 使用 `include_str!("../assets/official-skills.json")`。

`NetTransport::get_text` 用阻塞客户端，超时由 `flowy_net::configure(Client::builder().timeout(Duration::from_secs(20)).connect_timeout(Duration::from_secs(10)))` 设置。`flowy_net::blocking_client()` 没有超时，所以这里不要调用它，改为 `configure` 后再 `build`。读取 `retry-after` 头的整数字秒。User-Agent 设为 `flowy-skills/1.8.0`。

`retry_delay` 只对 429 和 503 返回 `Some`。等待毫秒是 `(retry_after_secs.unwrap_or(1) * 1000) + jitter_ms`。抖动由调用方传入，本函数不读时钟。

- [x] **Step 4: 跑测试确认通过**

Run: `cargo test -p flowy-skills --lib official::tests transport::tests`

Expected: PASS。不访问网络。

- [x] **Step 5: Commit**

```bash
git add crates/flowy-skills/assets/official-skills.json crates/flowy-skills/src/official.rs crates/flowy-skills/src/transport.rs crates/flowy-skills/src/lib.rs
git commit -m "feat: load bundled official skills and proxy market http"
```

---

### Task 7: 安装包校验与静态扫描

**Files:**
- Create: `crates/flowy-skills/src/archive.rs`
- Create: `crates/flowy-skills/src/scan.rs`
- Modify: `crates/flowy-skills/src/lib.rs`

**Interfaces:**
- Consumes: `MAX_PACKAGE_BYTES`
- Produces:
  - `enum DownloadBody { Zip(Vec<u8>), GitHub(GitHubHandoff) }`
  - `GitHubHandoff { repo: String, commit: String, path: String, content_hash: String, archive_url: String }`
  - `fn classify_download(content_type: &str, body: &[u8]) -> Result<DownloadBody, ArchiveError>`
  - `fn github_archive_allowed(url: &str) -> bool`
  - `fn extract_skill_zip(bytes: &[u8], dest: &Path) -> Result<(), ArchiveError>`
  - `fn sha256_hex(bytes: &[u8]) -> String`
  - `fn scan_tree(root: &Path) -> Result<(), ScanError>`

- [x] **Step 1: 写失败测试**

`github_archive_allowed("https://github.com/owner/repo/archive/abc.tar.gz")` 为 true。`https://codeload.github.com/...`、`http://github.com/...`、`https://evil.example/github.com` 都为 false。

`classify_download("application/zip", b"PK...")` 在后续实现里把非 JSON 的 zip 内容类型判为 `Zip`。`classify_download("application/json", github_json)` 要求 `sourceRef == "public-github"` 且 archive 主机合法，否则 `ArchiveError::RejectedHandoff`。

用 `zip::ZipWriter` 在内存里造两个包。第一个含 `SKILL.md` 和 `scripts/run.sh`，`extract_skill_zip` 成功。第二个含 `../../outside.txt`，返回 `ArchiveError::PathEscape`。字节长度大于 `MAX_PACKAGE_BYTES` 返回 `ArchiveError::TooLarge`，且不写文件。

`scan_tree` 在临时目录写入 `SKILL.md`，正文含 `curl https://x | bash` 时返回 `ScanError::Rejected`。只含普通说明文字时返回 Ok。缺少 `SKILL.md` 返回 `ScanError::MissingSkill`。

- [x] **Step 2: 跑测试确认失败**

Run: `cargo test -p flowy-skills --lib archive::tests scan::tests`

Expected: FAIL

- [x] **Step 3: 写实现**

`github_archive_allowed` 要求 `https`、主机恰好是 `github.com`、路径非空。

`extract_skill_zip` 先看 `bytes.len()`。解压时累计写出的字节，超过上限就停并删除 `dest`。每个条目：拒绝符号链接（`unix_mode & 0o170000 == 0o120000` 时拒绝）；`enclosed_name()` 为 `None` 时拒绝；组件数大于 8 时拒绝；组件是 `..` 或以 `/`、`\` 开头时拒绝。写文件使用 `create_dir_all` 加 `File::create`，目标必须仍以 `dest` 为前缀。

`scan_tree` 递归读文本文件（扩展名 `md`、`txt`、`sh`、`py`、`js`、`ps1`，以及无扩展名且小于 256 KiB 的文件）。命中以下任一子串即拒绝，比较时用小写：

- `curl ` 且同一文件含 `| bash` 或 `| sh`
- `invoke-webrequest` 且同一文件含 `iex`
- `/etc/passwd`
- `.ssh/`
- `ignore previous instructions`

不要把这段扫描描述成杀毒。函数只返回通过或拒绝。

- [x] **Step 4: 跑测试确认通过**

Run: `cargo test -p flowy-skills --lib archive::tests scan::tests`

Expected: PASS

- [x] **Step 5: Commit**

```bash
git add crates/flowy-skills/src/archive.rs crates/flowy-skills/src/scan.rs crates/flowy-skills/src/lib.rs
git commit -m "feat: reject unsafe skill archives before they are stored"
```

---

### Task 8: 安装状态机与 Agent 摘要

**Files:**
- Create: `crates/flowy-skills/src/install.rs`
- Create: `crates/flowy-skills/src/registry.rs`
- Modify: `crates/flowy-skills/src/store.rs`
- Modify: `crates/flowy-skills/src/lib.rs`

**Interfaces:**
- Consumes: `SkillId`, `extract_skill_zip`, `scan_tree`, `sha256_hex`
- Produces:
  - `enum InstallStep { Resolve, Verify, Fetch, Hash, Scan, Stage, Commit }`
  - `struct InstallRecord { id: SkillId, step: InstallStep, auto_invoke: bool, scripts_enabled: bool, content_hash: String }`
  - `fn begin_community_install(id: &SkillId) -> InstallRecord`
  - `fn advance(record: &InstallRecord, event: InstallEvent) -> Result<InstallRecord, InstallError>`
  - `InstalledSkill { id: SkillId, name: String, description: String, root: PathBuf, auto_invoke: bool }`
  - `fn project_installed(root: &Path, record: &InstallRecord) -> Result<InstalledSkill, RegistryError>`
  - `fn prompt_lines(skills: &[InstalledSkill]) -> Vec<String>`
  - `fn verify_url(id: &SkillId) -> String`
  - `fn download_url(id: &SkillId) -> String`
  - `fn accept_verify_body(body: &str) -> Result<(), InstallError>`

状态只允许前进：`Resolve → Verify → Fetch → Hash → Scan → Stage → Commit`。`InstallEvent::Verified { ok: false }` 返回 `InstallError::Rejected`。`InstallEvent::Hashed { expected, actual }` 在两者不等时返回 `InstallError::HashMismatch`。社区记录的 `auto_invoke` 和 `scripts_enabled` 恒为 false。

`project_installed` 读取 `{root}/SKILL.md` 的 YAML frontmatter，只取 `name` 和 `description`。`prompt_lines` 对 `auto_invoke == true` 的技能输出 `name: description`，其余不输出。

SQLite 增加：

```sql
CREATE TABLE IF NOT EXISTS install (
  owner_handle TEXT NOT NULL,
  slug TEXT NOT NULL,
  version TEXT NOT NULL,
  step TEXT NOT NULL,
  auto_invoke INTEGER NOT NULL,
  scripts_enabled INTEGER NOT NULL,
  content_hash TEXT NOT NULL,
  PRIMARY KEY (owner_handle, slug, version)
);
```

`Store::upsert_install` 和 `Store::installed` 读写这张表。

- [x] **Step 1: 写失败测试**

从 `begin_community_install` 出发，连续喂 `Verified { ok: true }`、`Fetched`、`Hashed` 相等、`Scanned`、`Staged`、`Committed`，最终 `step == Commit`。中途 `Verified { ok: false }` 失败，且记录停在 `Verify` 之前的调用结果不被写成 Commit。哈希不一致失败。`prompt_lines` 在社区 `auto_invoke=false` 时为空；官方记录手动设成 true 后出现一行。

Frontmatter 样例：

```markdown
---
name: pdf
description: Fill PDF forms.
---

正文不进入摘要。
```

`project_installed` 的 `name` 为 `pdf`，`description` 为 `Fill PDF forms.`。

`verify_url` 对 `awspace/pdf/1.2.3` 返回 `https://clawhub.ai/api/v1/skills/pdf/verify?ownerHandle=awspace&tag=latest`。`download_url` 返回 `https://clawhub.ai/api/v1/download?slug=pdf&ownerHandle=awspace&version=1.2.3`。`accept_verify_body` 只在 JSON 里 `ok == true` 且 `decision == "pass"` 时返回 Ok；`pending`、`warn`、`malicious`、缺字段都返回 `InstallError::Rejected`。

- [x] **Step 2: 跑测试确认失败**

Run: `cargo test -p flowy-skills --lib install::tests registry::tests`

Expected: FAIL

- [x] **Step 3: 写实现**

`advance` 用匹配当前 `step` 的事件决定下一状态，其他事件返回 `InstallError::Unexpected`。不要提供跳到 `Commit` 的函数。

Frontmatter 只解析首个 `---` 与下一个 `---` 之间的 `name:` / `description:` 行。值去掉首尾空格和一对引号。不引入 YAML crate。

- [x] **Step 4: 跑测试确认通过**

Run: `cargo test -p flowy-skills --lib install::tests registry::tests store::tests`

Expected: PASS

- [x] **Step 5: Commit**

```bash
git add crates/flowy-skills/src/install.rs crates/flowy-skills/src/registry.rs crates/flowy-skills/src/store.rs crates/flowy-skills/src/lib.rs
git commit -m "feat: install skills only after verify, hash, and scan"
```

---

### Task 9: 把一页刷新串起来

**Files:**
- Create: `crates/flowy-skills/src/market.rs`
- Modify: `crates/flowy-skills/src/lib.rs`

**Interfaces:**
- Consumes: Task 2–6 的 URL、解析、审核、存储、传输
- Produces:
  - `struct SkillMarket<T: CatalogTransport> { transport: T, store: Store }`
  - `fn SkillMarket::refresh_trending(&mut self, now_unix: i64) -> Result<Vec<Card>, MarketError>`
  - `fn SkillMarket::refresh_category(&mut self, slug: &str, now_unix: i64) -> Result<Vec<Card>, MarketError>`
  - `fn SkillMarket::search(&mut self, query: &str, category: Option<&str>, now_unix: i64) -> Result<Vec<Card>, MarketError>`
  - `fn SkillMarket::official(&self) -> Result<Vec<OfficialSkill>, OfficialError>`

行为：

1. `search` 先读 `search_fresh`。命中则零请求返回。
2. 未命中则 `get_text(search_url)`。状态 429 或 503 时按 `retry_delay` 睡一次再请求一次。第二次仍失败则返回旧缓存；没有旧缓存才返回 `MarketError::Unavailable`。
3. 解析卡片后 `post_json("https://clawhub.ai/api/v1/skills/-/security-verdicts", verdict_body)`。审核请求失败时不把未审核卡片写入默认货架，返回旧缓存或空列表，并带 `MarketError` 仅当两边都没有。成功则 `apply_verdicts`，再 `save_search` 或 `replace_shelf`。
4. `refresh_trending` 的 shelf key 是 `trending`。`refresh_category` 的 shelf key 是 `category:{slug}`。
5. 函数是同步的。文档写明调用方放到 `spawn_blocking`。本任务不在库内启动 tokio 运行时。

- [x] **Step 1: 写失败测试**

在 `market.rs` 的测试模块里写：

```rust
struct ScriptedTransport {
    next: std::sync::Mutex<Vec<(u16, String)>>,
    hits: std::sync::Mutex<Vec<String>>,
}

impl CatalogTransport for ScriptedTransport {
    fn get_text(&self, url: &str) -> Result<HttpText, TransportError> {
        self.hits.lock().unwrap().push(url.to_string());
        let (status, body) = self.next.lock().unwrap().remove(0);
        Ok(HttpText { status, body, retry_after_secs: if status == 429 { Some(0) } else { None } })
    }

    fn post_json(&self, url: &str, _body: &serde_json::Value) -> Result<HttpText, TransportError> {
        self.get_text(url)
    }
}
```

测试覆盖：

- 第一次搜索发出 search URL 和 verdict URL 两条请求，结果只有 pass 卡片，顺序与搜索结果一致。
- 15 分钟内第二次搜索的请求数为 0。
- verdict 响应把第二张卡片判 fail 后，缓存里没有它。
- 传输第一次返回 429、`retry_after_secs: Some(0)`，第二次返回 200，最终成功。测试里 jitter 固定为 0，并把睡眠替换成记录下来的等待值，避免真的等待。给 `refresh_*` 增加参数不合适；改为 `CatalogTransport` 不负责睡眠，`SkillMarket` 调用可注入的 `fn sleeper(Duration)`。测试 sleeper 只记录 Duration。

- [x] **Step 2: 跑测试确认失败**

Run: `cargo test -p flowy-skills --lib market::tests`

Expected: FAIL

- [x] **Step 3: 写实现**

按上面的 5 条行为写 `refresh_page(fetch, shelf_key, now)`，三个公开方法都调用它。不要在这个函数里循环 `nextCursor`。

- [x] **Step 4: 跑测试确认通过**

Run: `cargo test -p flowy-skills --lib`

Expected: PASS，且测试日志里没有对 `clawhub.ai` 的真实连接。

- [x] **Step 5: Commit**

```bash
git add crates/flowy-skills/src/market.rs crates/flowy-skills/src/lib.rs
git commit -m "feat: refresh one skill page through cache and verdicts"
```

---

## 本计划不改的部分

- `crates/flowy-app/src/app/skills.rs` 的官方网格继续读编译期 `SKILLS`。社区货架的 GPUI 卡片、分类芯片和安装按钮另开界面计划，调用 `SkillMarket::official`、`refresh_trending`、`refresh_category`、`search`。
- Agent 系统提示只追加 `prompt_lines` 的结果。不把 `SKILL.md` 正文放进每轮提示。接入 `flowy-agent` 的调用点留到界面计划一起做，避免在市场引擎还没稳定时改对话循环。

## 验证

每个任务的命令都要在该任务结束时通过。全部任务完成后跑：

```bash
cargo test -p flowy-skills --lib
cargo clippy -p flowy-skills --all-targets -- -D warnings
```

Expected: 测试通过，clippy 无 warning。不要同时开另一个 cargo 进程。

## 执行说明

实现时按 Task 1 到 Task 9 的顺序做。每任务一次提交。遇到 ClawHub 响应字段和夹具不一致时，改解析器并补一条夹具，不要放宽 `ownerHandle`、版本号和 `decision=pass && security.status=clean` 这三条。

# flowy-web：正文抽取与 Token 预算优化设计

**日期：** 2026-07-10  
**状态：** 已批准（brainstorming）  
**前置：** `docs/superpowers/specs/2026-07-09-flowy-web-search-extract-design.md`  
**背景证据：** 对话 #79「晋K PR055」中 `web_extract` 拉 `bj.bendibao.com` 出现 body 超时；实测同 URL 无压缩约 10–18s / 56KB，gzip 约 1.2s / 15KB；整页转 markdown 易把导航噪声塞进最高 15k 字符预算。

## 目标与非目标

### 目标（本版）

- **传输：** `HttpExtractProvider` 的 `reqwest` 启用 **gzip（+ deflate）**，降低慢站超时。
- **正文优先：** 引入可替换的 `ArticleExtractor`；默认 Readability 系实现（优先 `dom_smoothie`，集成不顺可换 `legible`）。
- **预算：** `EXTRACT_CHAR_LIMIT` 从 **15_000 → 3_000**；超限截断并标 `truncated: true`。
- **回退：** 正文抽空或过短时回退整页 `html_to_markdown`，保证不因抽取失败而空结果。
- **可观测：** 结果带 `extractor`（`readability` | `fullpage`），tool 输出可见。
- **策略文案：** `web_search` / `web_extract` description 引导事实类先 search，snippet 够则少 extract。

### 非目标（本版明确不做）

- Trafilatura / `rs-trafilatura` / `readex` 双引擎兜底（第二 PR）
- query-aware 段落截取
- LLM 摘要压缩
- 大页落盘 + `read_file`
- Browser / CDP 渲染兜底 extract
- 付费云端抽取 API

## 决策摘要

| 项 | 选择 |
|----|------|
| 总体方案 | 最小漏斗：gzip + Readability 正文 + 3k + 抽空回退整页 |
| 抽取抽象 | `ArticleExtractor` trait；默认 Readability 系 |
| 默认实现 | 优先 `dom_smoothie`；不顺则 `legible` |
| 成功标准 | 管线 + 策略文案 + `extractor` 可观测 |
| Trafilatura | 延后 |

## 架构

```
WebExtractTool
  → HttpExtractProvider::extract
       ├─ fetch(+gzip/deflate, SSRF, timeout)
       ├─ ArticleExtractor::extract(html) → Option<article_html>
       │     默认 DomSmoothieExtractor（或 LegibleExtractor）
       ├─ 质量门：空/过短 → 使用整页 HTML，extractor=fullpage
       │         否则 extractor=readability
       ├─ html_to_markdown(chosen_html)
       └─ truncate_chars(3000) → ExtractedPage
```

### 模块边界

| 单元 | 做什么 | 不做什么 |
|------|--------|----------|
| `ArticleExtractor` | 从完整 HTML 得到正文 HTML 子树（或 `None`） | 不发网络、不转 md、不管字符预算 |
| 默认 Readability 实现 | 去 nav/footer/ads 等 boilerplate | 不绑在 Tool / bootstrap |
| `HttpExtractProvider` | 编排 fetch → 抽取 → 回退 → md → 截断 | 不知 search / Browser |
| `WebExtractTool` | 校验、格式化、description | 不知具体算法 crate |

**依赖方向：** `flowy-web` 可新增 Readability 系 crate；仍不依赖 `nomi-browser` / `nomifun-knowledge`。

## 回退与质量门

Readability 路径结果满足任一条件则回退整页：

| 条件 | 阈值 |
|------|------|
| 抽取返回 `None` / 空串 | — |
| 正文过短 | 转 md 后（或纯文本）**< 400 字符** |
| 明显无实质段落 | 实现可再加简单启发式；第一版以长度门为主 |

回退不视为 tool 错误；仅 `extractor` 标记为 `fullpage`。

## 数据契约变更

`ExtractedPage` 增加（或调整）字段：

```rust
pub struct ExtractedPage {
    pub url: String,
    pub title: Option<String>,
    pub markdown: String,
    pub truncated: bool,
    pub provider: String,   // 仍为 "http"
    pub extractor: String,  // "readability" | "fullpage"
}
```

`WebExtractTool` 成功输出中包含 `extractor: ...` 行，与现有 `provider:` 并列。

`EXTRACT_CHAR_LIMIT`：**3000**。

## 传输

- `crates/agent/flowy-web/Cargo.toml` 中 `reqwest` features 增加 `gzip`，建议同时 `deflate`。
- 超时、SSRF、串行、最多 3 URL 等行为保持现有设计不变。

## 策略文案

- **`web_search`：** 查公开事实、新闻、限行、天气等优先使用；snippet 足够回答时不要再 `web_extract`。
- **`web_extract`：** 已有 URL 且需要正文时使用；会抽取正文并截断，勿用 Browser 仅读公开页。

## 错误处理

| 情况 | 行为 |
|------|------|
| fetch 超时/网络失败 | 与现网一致：该 URL error；部分成功不整工具失败 |
| Readability 失败但整页可转 | 回退 `fullpage`，成功返回 |
| 整页也失败 | 该 URL error |
| 全部 URL 失败 | tool `is_error` |

## 测试

| 层 | 内容 |
|----|------|
| ArticleExtractor fixture | 含 nav/footer 的 HTML → 正文路径不含导航关键字 |
| 质量门 | 极短/空抽取 → `extractor=fullpage` |
| 截断 | 长正文 → `chars <= 3000` 且 `truncated=true` |
| 回归 | 现有 SSRF、wiremock extract、tool 单测仍过 |
| 手工验收 | 本地宝限行 URL：更快、更短、可见 `extractor` |

## 后续（第二 PR，不在本 spec）

- Trafilatura 系作为 L3：仅当 Readability 过短/质量差时触发
- `web_extract(query=...)` 相关窗口截取
- 可选落盘 + 分页读取

## 与前置 spec 的关系

本设计 **修订** 2026-07-09 spec 中的：

- 单页 inline 预算 15_000 → **3_000**
- extract 管线从「整页 html_to_markdown」→「正文抽取 + 回退 + 截断」
- 增加 gzip 与 `extractor` 可观测性

其余（工具名、无 key、SSRF、串行、不上 Routing v2）不变。

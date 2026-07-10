# flowy-web：无 Key Web Search + Extract 设计

**日期：** 2026-07-09  
**状态：** 已批准（brainstorming）  
**对齐参考：** [hermes-web-search-plus](https://github.com/robbyczgw-cla/hermes-web-search-plus)（工具面与使用策略，非 Routing v2 / 多 provider 运营层）

## 背景

主对话 agent 缺少一等公民的检索工具，模型倾向用 Browser 反复 navigate/observe/click 查公开信息（如限号、会议），导致调用多、失败多、上下文噪声大。Hermes 的做法是 `web_search` + `web_extract` 优先，Browser 仅用于交互。本设计在 Nomi/Flowy 用 Rust 落地同等工具面，且第一版默认无 API key。

## 目标与非目标

### 目标（第一版）

- 新增 crate **`flowy-web`**，分层清晰：tools / providers / types
- 主 agent 注册 **`web_search`**、**`web_extract`**（对内类型名 `WebSearchTool` / `WebExtractTool`）
- 默认 **无 key**：`DuckDuckGoSearchProvider` + `HttpExtractProvider`
- 更新 tool description 与 Browser / rules 文案：信息检索优先 search → extract，Browser 仅交互
- Provider 可替换，为后续 Serper / SearXNG / 付费 extract 留接口，第一版不做 auto-routing

### 非目标（第一版明确不做）

- 接入 Hermes Python 插件或 MCP 作为生产主路径
- Routing v2、research mode、quality_report、provider cooldown 矩阵、14 provider 池
- extract 全文落盘 + `read_file` 续读（Hermes 大页策略的完整版）
- Browser / CDP 渲染兜底 extract
- 付费 search/extract API key 作为默认路径

## 决策摘要

| 项 | 选择 |
|----|------|
| 对齐层 | A：工具面 + 使用策略 |
| 范围 | search（无 key）+ extract（本地 HTTP，无 key） |
| 默认 search | Provider 抽象 + DuckDuckGo HTML/Lite |
| 成功标准 | 架构落地 + prompt/tool 引导 |
| Crate | `flowy-web` |
| 命名 | 对外 `web_search` / `web_extract`；对内 `WebSearchTool` / `WebExtractTool` |
| 实现路径 | 方案 1：`types` / `provider` / `tools` 三模块清晰分离 |
| Crate 路径 | `crates/agent/flowy-web`（与 `nomi-tools` 同层，workspace 成员） |

## 架构

```
Agent bootstrap / ToolRegistry
  → 注入 SearchProvider / ExtractProvider
  → 注册 WebSearchTool / WebExtractTool

crates/agent/flowy-web
├── types/       SearchQuery, SearchHit, SearchResult,
│                ExtractRequest, ExtractedPage, WebError
├── provider/    trait SearchProvider, trait ExtractProvider
│                DuckDuckGoSearchProvider, HttpExtractProvider
└── tools/       WebSearchTool, WebExtractTool（impl nomi_tools::Tool）
```

### 职责边界

| 单元 | 做什么 | 不做什么 |
|------|--------|----------|
| `WebSearchTool` | 校验参数 → 调 `SearchProvider` → 格式化给模型 | 不解析 HTML、不知 DDG |
| `WebExtractTool` | 校验 URL 列表 → 调 `ExtractProvider` → 截断/标记 | 不点页面、不开 CDP |
| `SearchProvider` | `search(query) → hits` | 不管 tool schema / prompt |
| `ExtractProvider` | `extract(url) → title + markdown` | 不管搜索、不管 knowledge 入库 |
| Browser | 点击 / 登录 / 填表等交互 | 不当默认「搜一下 / 读网页」 |

### 依赖方向（硬约束）

- `flowy-web` → `nomi-tools`（`Tool` trait）+ HTTP/HTML 依赖
- agent / bootstrap → `flowy-web`（注册与注入）
- `flowy-web` **不**依赖 `nomi-browser` / `nomi-browser-engine`
- search **不**进入 `nomifun-knowledge`
- extract 可复用 `HttpFetcher` / `html_to_markdown` 的思路或实现细节；若依赖 knowledge，仅作为 extract 实现细节，不反向把 search 塞进 knowledge

## 工具契约

### `web_search`

| 参数 | 类型 | 说明 |
|------|------|------|
| `query` | string | 必填 |
| `count` | number | 可选；**默认 5**，上限 **10** |

第一版 tool schema **不**暴露 `provider` / `time_range` / `research`（types 层可预留扩展字段）。

### `web_extract`

| 参数 | 类型 | 说明 |
|------|------|------|
| `urls` | string[] | 必填；**最多 3 条** |

- 单次调用内部：**串行**（并发 = 1）
- 每个 URL 独立成功/失败；部分失败仍返回成功条目
- 不做 `render_js`、不做 format/html/images 开关

### 大页与限制

| 限制 | 第一版 |
|------|--------|
| extract URL 上限 | 3 |
| extract 并发 | 1（串行） |
| 单页 inline 预算 | 约 **15_000** chars（对齐 Hermes `extract_char_limit` 默认量级）；超出则截断并标 `truncated: true` |
| 超时 | search 整次超时；extract **每 URL** 单独超时 |
| 落盘 / read_file | **不做** |

### Provider 契约（示意）

```rust
trait SearchProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn search(&self, q: SearchQuery) -> Result<SearchResult, WebError>;
}

trait ExtractProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn extract(&self, req: ExtractRequest) -> Result<ExtractedPage, WebError>;
}
```

- Tool 只依赖 `Arc<dyn SearchProvider>` / `Arc<dyn ExtractProvider>`
- 默认实现：`DuckDuckGoSearchProvider`、`HttpExtractProvider`
- 换 Serper / SearXNG = 新 impl + bootstrap 注入，不动 tool

### 安全

- extract **默认拒绝**私网 / 内网目标：localhost、RFC1918、link-local、cloud metadata、解析到非全局地址的 DNS（对齐 Hermes extract 目标校验精神）
- 仅允许 `http://` / `https://`
- 第一版可不提供 `allow_private_urls` 配置（需要时再加）

## 接线与策略

### Bootstrap

1. 构造默认 `DuckDuckGoSearchProvider`、`HttpExtractProvider`
2. `WebSearchTool::new` / `WebExtractTool::new`
3. 注册进 `ToolRegistry`
4. 配置开关：第一版用统一 `tools.web.enabled`（默认 **true**）；关闭则两个工具都不注册

### Prompt / 文案

1. `web_search` / `web_extract` 的 `description`：写明适用场景与优先级  
2. Browser DESCRIPTION 与相关 assistant rules（如 cowork）：信息检索走 search → extract；Browser 仅交互  

成功标准：模型在公开事实查询上优先调用新工具，而不是 Browser 乱点。

## 错误处理

| 情况 | 行为 |
|------|------|
| search 网络/解析失败 | 明确错误；不假装有结果 |
| extract 单 URL 失败 | 该条 error，其它 URL 继续 |
| 私网 / 非法 URL | 该条拒绝，不发起请求 |
| 超时 | 该次或该 URL 失败，带原因 |
| 全部失败 | 结构化错误，禁止「空成功」 |

第一版不做 Hermes 式 cooldown / 多 provider fallback；可选一次短超时内简单重试（非必须）。

## 测试

| 层 | 内容 |
|----|------|
| Provider 单测 | DDG 用 fixture HTML；HttpExtract 用本地 HTML；SSRF 用例 |
| Tool 单测 | 空 query、urls > 3、截断标记、部分失败 |
| 接线烟测 | registry 可见 `web_search` / `web_extract` |
| CI | 不依赖公网必过 |
| 手工（非阻塞设计） | 如「北京今天限号」：search → extract，少用 Browser |

## 与 Hermes 的对齐关系

| Hermes 能力 | 第一版 |
|-------------|--------|
| 双工具面 search + extract | 对齐 |
| 检索优先于 Browser 的策略文案 | 对齐 |
| `max_extract_urls ≈ 3` | 对齐 |
| 多 URL 并行 extract | 不对齐（Hermes 工具面也非此模型；我们串行） |
| 私网 URL 拒绝 | 对齐（精神） |
| 大页 head/tail + 落盘 + read_file | 不对齐；改为简单截断 + `truncated` |
| Routing v2 / research / cooldown | 不对齐 |

## 后续扩展（不在本 spec 实现范围）

- 付费 `SearchProvider`（Serper / Tavily / You.com）与配置注入
- SearXNG（自托管、无 key）
- extract 落盘 + 分页读取
- BrowserFetcher 作为 JS 重页面的 extract 兜底
- 轻量 fallback / cooldown（若默认 keyless 不稳定）

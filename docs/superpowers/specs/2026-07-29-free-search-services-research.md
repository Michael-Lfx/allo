# Flowy 免费搜索服务、访问风险与 Agent 搜索架构调研

**日期：** 2026-07-29
**状态：** 调研结论，待产品决策

> 历史状态：这是更早期的调研结论，不作为当前实现状态。当前 Search/Fetch
> 架构和决策见
> `docs/architecture/web/managed-web-search-fetch-evolution.md`。

**前置设计：** `2026-07-09-flowy-web-search-extract-design.md`、`2026-07-10-flowy-web-article-extract-design.md`
**范围：** 免费或有免费额度的通用网页、新闻、百科和学术检索；IP/反爬限制；中国大陆与境外访问；Codex、Hermes Agent、OpenCode、OpenClaw、腾讯 WorkBuddy 的搜索链路。

## 执行摘要

没有找到同时满足以下条件的正式通用 Web Search API：

- 免费且免 Key；
- 允许多用户生产分发；
- 有明确稳定性承诺和公开限额；
- 允许缓存、展示和再分发结果；
- 在中国大陆与境外都提供可依赖的访问保证。

因此，Flowy 不宜把“免费搜索”理解为换一个永久免费的公共 API，而应采用分层链路：

```text
统一 web_search 契约
  ├─ 默认：DuckDuckGo HTML（低频、best-effort、可明确降级）
  ├─ 用户配置：私有 SearXNG
  ├─ 垂直源：Crossref / DataCite / arXiv / PubMed / Europe PMC
  │          Wikimedia / GDELT
  ├─ BYOK：百度千帆（大陆）/ Brave、OpenAlex 等（境外或学术）
  └─ 已知 URL：web_extract → JS/登录/交互页面才进入 Browser
```

最值得优先做的不是再增加一个不可控的公共免费端点，而是：

1. 保留 DuckDuckGo，但在产品和代码中明确为非官方、低频、可失效的 `best-effort` provider。
2. 增加用户自带 SearXNG URL，不依赖某个公共实例。
3. 学术搜索采用多源联合，不把论文搜索交给通用网页搜索。
4. 搜索、正文抽取和 Browser 保持三个独立层次。
5. 免费 provider 必须显式选择或有清晰降级提示，不能在后台无声切换。

## 研究方法与证据边界

本报告只用项目或服务的官方文档、官方仓库和官方状态页作为产品能力、配额与条款依据。中国网络连接证据仅来自 2026-07-29 当前 Windows 主机、当前单一网络出口的一次 TCP 443 / HTTP 单点探测；其中应用层探测使用 Node `fetch`，单请求超时约 20 秒，不主动读取系统代理环境变量。

该探测只能回答“当前这一个网络出口能否完成 DNS/TLS/HTTP 请求”，不能证明：

- 服务在中国大陆整体可用或不可用；
- 服务在境外普遍可用；
- 服务具备生产 SLA；
- 无 Key 请求成功就代表官方允许该用法。

当前网络使用 Fake-IP。DuckDuckGo 和 Wikimedia 等域名被解析到明显的代理保留映射地址后超时，因此这些失败尤其不能解释成服务在中国大陆被封锁。中国大陆可用性优先采用服务商明确的官方证据；没有官方证据时统一标记为“未知”，不作全国性推断。

## 当前出口实测

| 服务 | 探测结果 | 解释 |
|---|---:|---|
| OpenAlex | HTTP 200，约 2.1s | 端点当前可达；无 Key 仅有约 `$0.10/日`试用预算，规模使用仍应配置免费 Key |
| Crossref | HTTP 200，约 1.3s | 当前出口可达 |
| Semantic Scholar | HTTP 429，约 0.9s | 端点可达，但匿名共享池当时已限流 |
| arXiv | HTTP 200，约 0.9s | 当前出口可达 |
| NCBI E-utilities | HTTP 200，约 1.9s | 当前出口可达 |
| Europe PMC | HTTP 200，约 3.3s | 当前出口可达 |
| 百度千帆 AI Search | HTTP 401 | DNS/TLS/端点可达；无 Key，未验证成功检索 |
| Parallel `search.parallel.ai` | TCP 443 可达 | 只证明当前出口能建立 TCP 连接，未验证匿名额度、成功检索或生产可用性 |
| DuckDuckGo HTML | 超时 | Fake-IP 环境下的单点失败，原因不确定 |
| Wikimedia API | 超时 | Fake-IP 环境下的单点失败；另有基金会关于大陆屏蔽的历史官方声明 |
| GDELT DOC | 超时 | 单点失败，原因不确定 |
| Jina Search | 直连超时 | 单点失败，原因不确定 |
| Brave Search API | 直连超时 | 单点失败，原因不确定 |

## 通用网页搜索候选

### 推荐矩阵

| 候选 | Key / 成本 | IP、反爬和限流 | 中国大陆 | 境外 | Flowy 建议 |
|---|---|---|---|---|---|
| DuckDuckGo HTML/Lite | 无 Key、无正式 API 费用 | 非官方 HTML 集成；可能出现 CAPTCHA、IP 封禁和 DOM 变化；没有公开 API 配额或 SLA | 未获官方可用保证；当前出口超时，结论不确定 | 普通网站可访问性，但无自动化 API 保证 | 保留为低频 `best-effort` 默认；缓存、并发限制、熔断、清晰报错 |
| SearXNG 私有实例 | 软件免费，自托管有机器和运维成本 | 实例出口会被上游按 IP 限流、CAPTCHA 或封禁；自身 limiter 可保护实例 | 部署位置和所选引擎决定；可在大陆自建，但上游仍受网络限制 | 同理 | 最推荐的零供应商费用扩展；由用户填写可信私有 URL |
| SearXNG 公共实例 | 无 Key | JSON 常被关闭；公共实例可能记录请求、少结果、遭滥用或上游封禁 | 不确定 | 不确定 | 不硬编码为产品默认 |
| Exa Hosted MCP | 官方 Hosted MCP 可无 Key使用 | 未找到匿名生产额度和 SLA；服务行为可随政策变化 | 无官方保证 | 可作为远程 MCP 试验 | 仅显式启用的外部 MCP，不作为内置生产默认 |
| Parallel Free MCP | 无 Key 免费 MCP | 官方将免费额度定位于 hobby / personal agents；生产规模建议付费，未公开精确匿名额度 | 无官方保证 | 可作为远程 MCP 试验 | 仅显式启用的外部 MCP，不作为多用户共享默认 |
| YaCy | GPL，自托管 | 没有供应商配额；索引、抓取、机器资源、站点反爬均由部署者承担 | 本地端点可控；抓取范围仍受网络影响 | 同理 | 长期实验；不是开箱即用的相关性搜索 |
| Common Crawl | 数据免费 | CDX 按 IP 严格限流，过量会 503 或临时封 IP；不适合交互查询 | 数据下载路径需另测 | 官方公共数据 | 仅用于未来离线索引，不接入实时 `web_search` |

### DuckDuckGo 的准确定位

当前 Flowy 使用的是 DuckDuckGo HTML/Lite 页面，不是 DuckDuckGo 提供并承诺稳定的 Search API。官方 `robots.txt` 禁止抓取 `/html`、`/lite` 和若干结果路径；官方可接受使用政策也限制将其服务嵌入、展示或再分发到另一服务。SearXNG 的官方 DuckDuckGo 引擎文档还记录了 `vqd`、bot protection、CAPTCHA/IP block 和字段变化。

这意味着它可以解决“个人本地 agent 偶尔查询”的零配置问题，却不应被包装成：

- 稳定的生产 API；
- 面向公共后端的高并发搜索池；
- 有确定吞吐和可用率的服务；
- 可以永久依赖的页面协议。

若继续保留，至少应有：

- 全局和每会话并发上限；
- 查询结果短 TTL 缓存和重复请求合并；
- 403、429、CAPTCHA、解析空结果的独立错误分类；
- 指数退避和 provider 熔断；
- “当前免费搜索源暂不可用”的用户可见状态；
- 不把失败自动伪装成“没有搜索结果”。

官方依据：[DuckDuckGo Acceptable Use](https://duckduckgo.com/acceptable-use)、[robots.txt](https://duckduckgo.com/robots.txt)、[URL parameters](https://duckduckgo.com/duckduckgo-help-pages/settings/params)、[SearXNG DuckDuckGo engine](https://docs.searxng.org/dev/engines/online/duckduckgo.html)。

### SearXNG

SearXNG 是最适合 Flowy 当前约束的扩展方向，但“免费”是软件免费，不是公共算力和上游额度免费。它的优势是 provider 可控、可自托管、可组合多个上游；代价是需要部署、监控和处理出口 IP 被上游限制。

SearXNG `/search` 支持 JSON，但实例必须启用对应 format；许多公共实例只开放 HTML，JSON 请求会返回 403。官方 limiter 文档明确说明公共实例的出口 IP 可能遇到 CAPTCHA 或封禁。因此建议只支持：

```text
provider = "searxng"
base_url = 用户自己的可信实例
```

不要在 Flowy 中内置一个随机公共实例地址。
官方依据：[Search API](https://docs.searxng.org/dev/search_api.html)、[Limiter](https://docs.searxng.org/admin/searx.limiter.html)、[Own instance](https://docs.searxng.org/own-instance.html)、[官方仓库](https://github.com/searxng/searxng)。

### 有免费额度但不适合作为平台共享 Key

| 服务 | 官方免费额度 / 条件 | 关键限制 | 建议 |
|---|---|---|---|
| 百度千帆 AI Search | 1,500 次/月，按天约 50 次；需实名和 API Key | 默认约 1 QPS；开后付费提高额度；存储和再分发权需接入前继续核对 | 大陆用户 BYOK 首选，不内置平台 Key |
| Brave Search API | 每月赠送 $5 credits，约 1,000 次；要求信用卡和 API Key | $5/1,000；标准方案不自动授予长期存储权，限制 Key 分享和限额规避 | 境外用户 BYOK 备选 |
| OpenAlex | 无 Key 有约 `$0.10/日`试用预算；免费 Key 有约 `$1/日`预算，按当前价格约可做 1,000 次 search 或 10,000 次 list/filter | 不再是固定“10 万请求/日”；不同操作扣费不同，规模使用应带 Key | 学术 BYOK 或使用其免费 snapshot 自建 |
| Semantic Scholar | 大部分端点可匿名；可申请免费 Key | 匿名为共享池，负载高时会进一步限流；本次实测 429；Key 不得共享 | 学术可选 fallback / BYOK，不作唯一默认 |
| Jina Search | 免费 Key 100 RPM、100K TPM、并发 2 | 无 Key search 会被拦截；固定搜索成本较高；按 IP/Key 计费或限流 | 个人 BYOK/试用，不作共享默认 |

官方依据：[百度千帆 AI Search API](https://cloud.baidu.com/doc/qianfan-api/s/Wmbq4z7e5)、[百度价格](https://cloud.baidu.com/doc/qianfan/s/1mh4sv6c4)、[Brave Search API](https://brave.com/search/api/)、[Brave API Terms](https://api-dashboard.search.brave.com/terms-of-service)、[OpenAlex Authentication & Pricing](https://developers.openalex.org/guides/authentication)、[Semantic Scholar API](https://www.semanticscholar.org/product/api)、[Semantic Scholar License](https://www.semanticscholar.org/product/api/license)、[Jina Reader/Search](https://jina.ai/reader/)。

## 垂直免费搜索

### 新闻与百科

| 服务 | 能力 | Key / 额度 | IP / 条款风险 | 建议 |
|---|---|---|---|---|
| Wikimedia / MediaWiki | 百科和知识条目搜索，不是全网搜索 | 公共读取无 Key；2026 未识别 IP 10 次/分钟，合规 User-Agent 200 次/分钟，并发最多 3 | 429/503 需遵守 `Retry-After`；页面内容通常按 CC BY-SA 等页面许可证；基金会曾正式声明 Wikipedia 在中国大陆被全语言屏蔽 | 作为 `encyclopedia_search` 补充；不能成为大陆默认通用源 |
| GDELT DOC | 全球新闻和事件检索 | 无 Key；未公开精确数值配额 | 官方明确存在高峰限流，legacy 搜索基础设施承压；文章正文版权仍归出版方 | 作为可选 `news_search` / enrichment；严格缓存、退避，不作唯一新闻源 |

官方依据：[Wikimedia Search API](https://www.mediawiki.org/wiki/API:Search_and_discovery/en)、[User-Agent Policy](https://foundation.wikimedia.org/wiki/Policy:Wikimedia_Foundation_User-Agent_Policy)、[2026 API rate limits](https://www.mediawiki.org/wiki/Wikimedia_APIs/Rate_limits)、[Wikimedia 关于中国大陆屏蔽的声明](https://wikimediafoundation.org/news/2019/05/17/wikimedia-foundation-urges-chinese-authorities-to-lift-block-of-wikipedia-in-china/)、[GDELT DOC API](https://blog.gdeltproject.org/gdelt-doc-2-0-api-debuts/)、[GDELT rate limiting](https://blog.gdeltproject.org/ukraine-api-rate-limiting-web-ngrams-3-0/)、[GDELT About](https://www.gdeltproject.org/about.html)。

### 论文搜索

论文检索不应只走 DuckDuckGo。通用搜索擅长发现网页，论文检索还需要 DOI、作者、期刊、引用、开放获取状态和学科数据库。建议建立独立的 `academic_search` 聚合层：

```text
academic_search(query)
  ├─ Crossref：跨学科 DOI / 出版元数据
  ├─ DataCite：数据集、软件、预印本等 DOI 元数据
  ├─ arXiv：物理、数学、计算机等预印本
  ├─ PubMed / Europe PMC：生物医学与生命科学
  ├─ Semantic Scholar：引用关系和语义排序，可选
  ├─ OpenAlex：大规模学术图谱，BYOK 或 snapshot
  └─ Unpaywall / DOAJ：开放获取状态和 OA 期刊补充
```

| 服务 | Key 与官方限额 | 适合用途 | 注意事项 | 推荐级别 |
|---|---|---|---|---|
| Crossref | 无 Key；2026 list/search 公共池 1 rps，带有效 `mailto` 的 polite 池 3 rps；单记录 5/10 rps | DOI、标题、作者、期刊、发布日期 | 按 email/IP 限流；429 退避；不要伪造或复用无效邮箱 | 默认 |
| DataCite | 公共检索无 Key；未识别 500 次/5 分钟/IP，带标识 1,000 次/5 分钟/IP | 数据集、软件、研究对象 DOI | 遵守 429 和标识要求 | 默认 |
| arXiv | 无 Key；所有机器合计 3 秒 1 次、单连接 | 预印本 | 元数据 CC0；论文 PDF 版权逐篇不同，不能默认存储或再分发 | 默认、慢速队列 |
| PubMed E-utilities | 无 Key 3 次/秒/IP；Key 10 次/秒 | 生物医学文献 | 超额可封 IP；分布式软件若需更高额度应让用户 BYOK；摘要可能受版权保护 | 生物医学默认 |
| Europe PMC | 公共 REST，无 Key；官方未给出清晰数字限额 | 生命科学、引用、OA 全文线索 | 主站禁止自动批量抓取；走 REST/OAI/FTP；全文只复用明确许可的 OA 子集 | 生物医学补充 |
| Semantic Scholar | 多数端点可匿名；免费 Key 通常从 1 rps 起 | 语义检索、引用图 | 匿名共享池会波动；许可证限制重新打包/转售；本次实测 429 | 可选 |
| OpenAlex | 无 Key约 `$0.10/日`；免费 Key 约 `$1/日`，相当于约 1,000 次 search 或 10,000 次 list/filter；snapshot 可下载 | 大规模学术图谱 | API 已改为按操作计价的 freemium；规模使用需要 Key，分发产品不能共享单个用户 Key | BYOK / 自建 |
| Unpaywall | email 参数，100,000 次/日 | 按 DOI 查询合法 OA 位置 | 不是搜索引擎；文章许可证仍需逐项判断 | enrichment |
| DOAJ | 公共 API / dump，元数据 CC0 | OA 期刊和文章 | 搜索 API 的当前精确限额未在本轮官方资料中确认 | enrichment / 先做 spike |
| CORE | 有免费接入，但官方页面对注册和限额描述不一致 | OA 聚合 | 默认接入前需向服务方确认当前合同、商业资格和准确限额 | 暂缓 |

官方依据：[Crossref 2026 rate update](https://community.crossref.org/t/refining-rest-api-limits-for-improved-stability-and-reliability/16137)、[Crossref pools](https://community.crossref.org/t/rest-api-pools-which-to-use-and-when/15317)、[DataCite API](https://support.datacite.org/docs/api)、[DataCite rate limit](https://support.datacite.org/docs/rate-limit)、[arXiv API Terms](https://info.arxiv.org/help/api/tou.html)、[NCBI E-utilities](https://www.ncbi.nlm.nih.gov/books/NBK25497/?report=reader)、[Europe PMC REST](https://europepmc.org/RestfulWebService)、[Semantic Scholar API](https://www.semanticscholar.org/product/api)、[OpenAlex Authentication & Pricing](https://developers.openalex.org/guides/authentication)、[Unpaywall API](https://data.unpaywall.org/products/api)、[DOAJ FAQ](https://doaj.org/docs/faq/)、[CORE API](https://core.ac.uk/services/api)。

## IP 限制与反爬风险

免费服务是否会限制 IP，答案是“通常会”，只是形式不同：

| 类型 | 常见识别方式 | 典型服务 | Flowy 应对 |
|---|---|---|---|
| HTML 反爬 | 出口 IP、Cookie、UA、行为频率、验证码 | DuckDuckGo HTML、SearXNG 的上游引擎 | 低并发、缓存、熔断，不尝试代理轮换或绕过验证码 |
| 公共 API 限流 | IP、User-Agent、email、API Key | Wikimedia、Crossref、DataCite、PubMed | 合规标识、令牌桶、`Retry-After`、全局配额 |
| 匿名共享池 | 所有无 Key 用户共享吞吐 | Semantic Scholar | 不作唯一来源；429 后切换独立学术源 |
| 自托管聚合器 | 实例自身 limiter + 所有上游看到同一出口 IP | SearXNG | 用户私有实例、控制来源、监控每个 engine 的 cooldown |
| 免费 Key | Key、账户、信用卡、日/月额度 | 百度、Brave、OpenAlex、Jina | BYOK，本地安全存储；绝不在发行包中共享平台 Key |

不建议通过代理池、轮换 IP、伪造多个身份或并行放大请求规避限制。这会增加封禁和条款风险，也与本地优先、无后台数据传输的项目原则不相容。

Flowy 的两种 host 模式还应分别处理配额：

- Desktop 直连 provider 时，请求自然分散到各用户出口 IP，但每个用户仍需独立限流，且不能把这种分散当成规避服务限制的手段。
- Web 后端集中调用时，所有用户共享同一出口 IP；必须做服务级令牌桶、请求合并、缓存与熔断，不能只做单会话限流。

## 中国大陆与境外访问结论

### 中国大陆

- 没有找到经官方验证、免 Key、适合生产分发的大陆通用网页搜索 API。
- 百度千帆 AI Search 是本轮证据中大陆最明确的正式 API，但需要实名和 Key，免费额度有限，适合 BYOK。
- 私有 SearXNG 可以把服务端点部署在用户可控区域，但搜索质量仍取决于每个上游在该网络出口的可达性。
- Wikimedia 不可作为大陆稳定源；基金会有 Wikipedia 在中国大陆被屏蔽的正式历史声明。
- Crossref、DataCite、arXiv、PubMed、Europe PMC 等学术 API 的本次单点探测多数成功，但其官方文档没有承诺中国大陆地区 SLA。
- Parallel `search.parallel.ai` 在 2026-07-29 当前机器的 TCP 443 单点探测可达；这不证明搜索成功、免费额度或生产可用性。
- DuckDuckGo、Jina、Brave 的本次直连超时，以及 GDELT 的本次超时，均不足以证明全国不可用。
- 未找到百度、搜狗、360 提供的“免费、免 Key、允许多用户生产使用”的正式通用 Web Search API；这里是“本轮未找到”，不是断言其永远不存在。

### 境外

- DuckDuckGo、Wikimedia、GDELT 等有公开服务，但“普通用户网页可访问”不等同于“允许自动化生产调用”。
- Brave、OpenAlex、Semantic Scholar 等正式 API 的合同和限额更清晰，但需要 Key 或存在匿名共享池风险。
- 自托管 SearXNG 仍是成本和控制力较均衡的选择。
- 本轮没有独立境外节点实测，因此不对特定国家或云区域作可用率承诺。

### 产品层建议

不要把地区识别写成硬编码的“中国走 A、境外走 B”。更稳妥的是：

1. 用户显式选择 provider；
2. 配置页显示是否需要 Key、官方额度、数据会发往哪里；
3. 测试连接并显示真实错误；
4. 搜索结果标注来源；
5. provider 不可用时允许用户选择替代项，不无声跨境切换。

## 其他 Agent 的搜索架构

### 本次调研所用的当前 Codex 搜索链路

本次会话可调用的是 Codex 平台托管的 `web.run` 工具，而不是仓库里的
DuckDuckGo provider，也不是当前会话临时连接的第三方 MCP。它向模型暴露
`search_query`、`open`、`click`、`find` 等统一动作：

```text
search_query → 返回标题、URL、短摘要
  └─ 需要核对正文时：open / click → 页面内容
       └─ 页面内定位：find
```

因此它的工作方式与 Flowy 当前链路在概念上相同：先搜索发现，再按需抓取页面，
并不会把每个命中页面的完整正文全部塞进上下文。当前工具契约没有公开底层搜索
索引供应商，官方资料也未确认是 DuckDuckGo、Bing 或其他具体引擎，所以本报告
不作猜测。Codex 同时支持 MCP 扩展，但“支持 MCP”不等于本次搜索就是经 MCP
执行。

| Agent | Search | Fetch / Extract | Browser | 扩展与启示 |
|---|---|---|---|---|
| Codex | CLI 不内置 DDG/Bing provider，而是请求 OpenAI/Codex 服务端第一方 `web_search`；默认可使用 cached index，`--search` / `live` 切实时；底层搜索引擎未公开 | 服务端 search/open_page/find_in_page 语义 | CLI 未公开自动 Browser fallback；可通过 MCP 接 Browser | 无单独搜索 Key，但依赖 ChatGPT/Codex 登录或 API 计费；不能视作独立免费 API |
| Hermes Agent | `web_search` provider abstraction：Firecrawl、SearXNG、Brave、DDGS、Tavily、Exa、Parallel、xAI | `web_extract` 可与 search 选不同 provider | 内置完整 browser 工具；动态和交互页面再使用 | 与 Flowy 当前“search → extract → browser”最接近；免费 provider 也必须正确配置 |
| OpenCode | 稳定文档称 `websearch` 使用无 Key 的 Exa Hosted MCP；只在 OpenCode provider 或显式环境开关下可用 | 内置独立 `webfetch` | 官方核心工具列表无内置 Browser | `dev` 源码已出现 Exa/Parallel 远程 MCP 按 session 分配；不能把开发分支行为当稳定版承诺 |
| OpenClaw | provider plugin 输出统一结果；支持 DDG、SearXNG、Brave、Exa、Parallel Free、Codex Hosted、模型原生搜索等；缓存 15 分钟 | 本地 HTTP + Readability，Firecrawl 可选 fallback | 官方明确 JS-heavy / 登录页使用 Browser | 最值得借鉴：key-free provider 不参与自动探测，必须显式选择，避免公共免费端点被无声使用 |
| 腾讯 WorkBuddy | 官方宣称深度研究和网络信息搜集，但未公开默认 provider、索引源和调用协议；Skill 市场有 Web Search / Google Search | 更新日志证明存在 WebFetch 和浏览器模式 | 可安装 Agent Browser Skill | 支持 MCP、Skill 和 CLI 连接器；不能把腾讯 CodeBuddy CLI 的 `WebSearch` 文档直接推断为 WorkBuddy 内部实现 |

对应官方资料：

- Codex：[Web search](https://learn.chatgpt.com/docs/web-search)、[CLI commands](https://learn.chatgpt.com/docs/developer-commands?surface=cli)、[MCP](https://learn.chatgpt.com/docs/extend/mcp?surface=cli)、[App Server](https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md)
- Hermes Agent：[Web Search & Extract](https://hermes-agent.nousresearch.com/docs/user-guide/features/web-search)、[Browser](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/user-guide/features/browser.md)、[MCP](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/user-guide/features/mcp.md)
- OpenCode：[Tools](https://opencode.ai/docs/tools)、[MCP servers](https://opencode.ai/docs/mcp-servers)、[`dev` websearch source](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/tool/websearch.ts)
- OpenClaw：[Web Search providers](https://docs.openclaw.ai/tools/web)、[Tool / Skill / Plugin boundaries](https://github.com/openclaw/openclaw/blob/main/docs/tools/index.md)
- WorkBuddy：[Overview](https://www.workbuddy.cn/docs/workbuddy/Overview)、[Skills Market](https://www.workbuddy.ai/docs/workbuddy/From-Beginner-to-Expert-Guide/Function-Description/Skills-Market)、[Connector](https://www.workbuddy.cn/docs/workbuddy/From-Beginner-to-Expert-Guide/Function-Description/Connector)、[Changelog](https://www.workbuddy.cn/docs/workbuddy/Changelog)

共同的成熟模式不是“绑定一个最强搜索商”，而是：

```text
稳定工具契约
  → 可替换 SearchProvider
  → 独立 ExtractProvider
  → 动态页 / 登录页 Browser
  → MCP / Plugin 作为外部扩展
```

## 排除项

| 服务 | 排除原因 |
|---|---|
| Bing Search APIs | 已于 2025-08-11 退役，不能新订阅 |
| Google Custom Search JSON API | 已对新客户关闭；既有客户到 2027-01-01；免费仅 100 次/日 |
| Mojeek API | 只有有限试用，生产按 CPM 收费；普通网页自动抓取被条款禁止，低档计划存储权受限 |
| 腾讯 Web Search API（WSA） | 需实名和开通，官方按量计费，未见适合本需求的生产免费档 |
| NewsAPI | Developer 免费档只允许开发测试，100 次/日且新闻延迟 |
| GNews | 免费档明确是非商业、开发和测试用途 |
| Mediastack | 免费档 100 次/月且延迟；Commercial Use 列在付费档，存储/分发受限 |

官方依据：[Bing retirement](https://learn.microsoft.com/en-us/lifecycle/announcements/bing-search-api-retirement)、[Google Custom Search](https://developers.google.com/custom-search/v1/overview)、[Mojeek API](https://www.mojeek.com/services/search/web-search-api/)、[Mojeek Terms](https://www.mojeek.com/about/terms.html)、[腾讯 WSA](https://cloud.tencent.com/product/wsa)、[NewsAPI Pricing](https://newsapi.org/pricing)、[GNews Pricing](https://gnews.io/pricing)、[Mediastack Pricing](https://mediastack.com/pricing)。

## 推荐实施顺序

### P0：加固现有免费链路

- 将 DuckDuckGo provider 明确标记为 `best_effort` 和非官方集成。
- 实现全局限流、缓存、同 query 合并、429/403/CAPTCHA 分类、退避、熔断。
- 失败时不返回伪装的空结果；让 agent 和用户知道 provider 不可用。
- 保持 `web_search`、`web_extract`、Browser 三层分离。

### P1：加入可控免费来源

- 支持用户配置私有 SearXNG URL。
- 新增 `academic_search`：首批 Crossref + DataCite，随后 arXiv + PubMed / Europe PMC。
- Wikimedia 和 GDELT 以独立垂直 provider 接入，不假装成全网覆盖。

### P2：地区与 BYOK

- 大陆：百度千帆 AI Search BYOK。
- 境外：Brave BYOK。
- 学术：OpenAlex / Semantic Scholar BYOK 或匿名 fallback。
- 配置页展示官方额度、地区证据、数据目的地和结果存储限制。

### P3：可选生态

- Exa Hosted MCP、Parallel Free MCP 作为用户显式安装或启用的实验选项。
- YaCy / OpenAlex snapshot / Common Crawl 作为组织自建索引方案评估。

## 最终推荐列表

按当前“多用户分发、平台不承担不可预测 Key 费用”的约束排序：

1. **DuckDuckGo HTML：保留，但只作为低频 best-effort 默认。**
2. **SearXNG：新增用户自托管 / 私有实例配置，作为最重要的免费扩展。**
3. **Crossref + DataCite：论文检索的首选免费默认源。**
4. **arXiv + PubMed + Europe PMC：按学科路由的免费补充。**
5. **Wikimedia + GDELT：百科和新闻的垂直补充，不替代通用搜索。**
6. **百度千帆 / Brave / OpenAlex / Semantic Scholar：只做 BYOK 或明确的可选连接。**
7. **Exa / Parallel 免费 MCP：只做显式实验连接，不承诺多用户生产额度。**
8. **YaCy / Common Crawl：未来自建索引方向，不进入当前交互式默认链路。**

这个组合不会消除免费服务的不稳定性，但会把风险从“一个免费网页端点决定全部搜索能力”降为“多个职责清楚、可观测、可替换的 provider”，并保持平台本身没有不可预测的共享 Key 成本。

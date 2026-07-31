# Managed Web Search Alternatives (2026-07-30)

> Status: historical provider research. Current architecture and decisions:
> [managed-web-search-fetch-evolution.md](managed-web-search-fetch-evolution.md).

## Scope and evidence boundary

This note evaluates general web-search providers for Allo Desktop under one
hard requirement: a shipped client must work without asking the user to create
an account, complete OAuth, or configure an API key. It uses provider-owned
documentation, provider-owned source repositories, official status pages, and
the SearXNG project's own documentation. It does not treat third-party MCP
catalog claims as provider guarantees.

No provider reviewed here publishes a mainland-China reachability guarantee or
China-region SLA. A global status page is not evidence of mainland reachability.
China results below therefore remain **unknown pending direct tests** across
multiple mainland networks, with and without the developer proxy. This is
separate from protocol/authentication compatibility.

## Decision summary

| Class | Provider | No account/key path | Published anonymous limit | Default-chain decision |
| --- | --- | --- | --- | --- |
| Directly eligible | Parallel Search MCP | Yes | Not quantified; described for hobby/personal or light use | Keep first while real-device success remains healthy |
| Directly eligible, probe required | You.com free MCP profile | Yes | 100 searches/day | Best new candidate; probe before admission |
| Directly eligible but constrained | Exa hosted MCP | Yes | Not quantified; free plan explicitly returns 429 when exhausted | Do not delete solely from a small sample; demote or remove if telemetry shows frequent cooldown |
| Keyless but unsupported integration | DuckDuckGo HTML Search | Yes | No API quota or programmatic-search contract published | Keep only as resilient fallback, not a contractual API |
| Operator-dependent | SearXNG | Self-hosted instance needs no user key | Operator controlled | Suitable only if Allo operates/bundles an instance; public instances are not a reliable default |
| Fetch only | Jina Reader | Yes for `r.jina.ai` | 20 RPM without key | Possible future `web_extract` fallback, not search |
| Excluded | Brave, Tavily, Serper, Google, Bing replacement, Jina Search | No | Free tiers still require account/key or OAuth | Future BYOK only, or unavailable |

The most defensible next experiment is:

```text
Parallel → You.com free MCP → Exa → DuckDuckGo
```

This is a probe order, not an immediate production-order recommendation.
You.com must first pass the same protocol, schema, sequential/concurrent,
latency, output-bound and mainland/direct-network probes used for Parallel and
Exa. If Exa continues to enter long cooldowns in a representative sample, use:

```text
Parallel → You.com free MCP → DuckDuckGo
```

## Eligible providers

### Parallel Search MCP

- Official endpoint: `https://search.parallel.ai/mcp`.
- Authentication: none by default; an optional Bearer key raises limits.
  Parallel explicitly separates this endpoint from the auth-enforced
  `mcp-oauth` endpoint
  ([programmatic-use documentation](https://docs.parallel.ai/integrations/mcp/programmatic-use)).
- The official announcement says no account/API key is necessary and describes
  `web_search` as returning ranked URLs plus compressed query-relevant excerpts
  ([announcement](https://parallel.ai/blog/free-web-search-mcp)).
- The same announcement characterizes the anonymous service as intended for
  hobby use and personal agents, and recommends the paid service for production
  agents at scale. The quickstart similarly says it is suited to exploration
  and light use
  ([quickstart](https://docs.parallel.ai/integrations/mcp/quickstart)).
- Parallel publishes API-key quotas, but does not publish a numeric anonymous
  MCP quota. The API table therefore must not be presented as the anonymous MCP
  allowance
  ([API rate limits](https://docs.parallel.ai/getting-started/rate-limits)).
- Official status history records a roughly ten-minute Search MCP DNS incident,
  showing that fallback remains necessary even for the preferred provider
  ([status history](https://status.parallel.ai/history)).

**Assessment:** the strongest current keyless primary because its output is
explicitly designed for agents, but it is not a contractual free production
capacity guarantee. Keep cooldown/fallback and collect only privacy-safe
provider outcome metrics.

### You.com free MCP profile

- Official endpoint:
  `https://api.you.com/mcp?profile=free`.
- Authentication: no signup, API key, or OAuth for this profile.
- Allow-list: only `you-search` is exposed; contents, research, finance,
  balance and discovery tools are excluded.
- Published limit: 100 queries/day
  ([MCP server documentation](https://you.com/docs/build-with-agents/mcp-server),
  [Codex-specific setup](https://you.com/docs/build-with-agents/agent-harnesses/codex-cli)).
- Search output is described as structured JSON containing web/news results,
  multiple snippets, dates and metadata, with no HTML parsing required
  ([Web Search overview](https://you.com/docs/guides/search)).

**Assessment:** the best newly found candidate because the credential-free
endpoint, tool scope and daily allowance are explicit. Its daily quota is
finite and the documentation does not state whether it is calculated per IP,
device, or another anonymous identity; do not infer that behavior. It needs a
real protocol probe before code admission.

### Exa hosted MCP

- Official endpoint: `https://mcp.exa.ai/mcp`; the hosted setup is advertised
  as requiring no API key
  ([Exa MCP page](https://exa.ai/mcp)).
- The endpoint can be narrowed with `?tools=web_search_exa`; the official
  documentation defines `web_search_exa` as clean, ready-to-use search content
  ([MCP reference](https://exa.ai/docs/reference/exa-mcp)).
- Exa calls this a generous free plan but publishes no numeric anonymous MCP
  quota. Its troubleshooting guide explicitly says a 429 means the free-plan
  limit was hit and recommends adding a key. It also says a key is needed to
  overcome free limits and enable production use
  ([MCP reference](https://exa.ai/docs/reference/exa-mcp)).
- The documented 10 QPS `/search` limit applies to the authenticated API and
  must not be misreported as the anonymous MCP quota
  ([API rate limits](https://exa.ai/docs/reference/rate-limits)).
- The official status page separately tracks Exa MCP and recent Search API
  incidents
  ([status page](https://status.exa.ai/)).

**Assessment:** the observed Allo 429 is consistent with the official free-plan
behavior, but a handful of calls cannot establish its long-run rate. Keep it
behind cooldown while measuring attempts/success/429 without queries. Demote or
remove it from the default chain if a larger real-device sample shows that it
rarely contributes before DuckDuckGo.

### DuckDuckGo HTML Search

Allo currently requests `https://html.duckduckgo.com/html/?q=...` and parses
HTML into title/URL/snippet records. DuckDuckGo documents its search UI, URL
parameters and non-JavaScript experience, but does not publish a supported
general Web Search JSON/MCP API or an application quota. Its parameter guide
says parameters are intended for individual use and directs app/extension use
to partnership guidance
([parameter guide](https://duckduckgo.com/duckduckgo-help-pages/settings/params)).
Its acceptable-use policy also forbids interference with service performance
and resale
([acceptable-use policy](https://duckduckgo.com/acceptable-use)).

DuckDuckGo says traditional links are largely sourced from Bing, supplemented
by its own crawler/indexes and specialist sources
([result sources](https://duckduckgo.com/duckduckgo-help-pages/results/sources)).

**Assessment:** keyless and proven useful in Allo, but HTML parsing is an
unsupported integration rather than a stable provider API. It remains valuable
as the last fallback because it is independent of MCP negotiation, but markup,
anti-bot policy or upstream composition may change without an API compatibility
notice.

## Conditional option: SearXNG

SearXNG provides a simple GET/POST HTTP Search API with JSON/CSV/RSS formats,
but each instance controls which formats are enabled; the official docs warn
that many public instances disable these formats
([Search API](https://docs.searxng.org/dev/search_api.html)).

Public instances are operated by third parties. SearXNG's own privacy guidance
says users must trust each public-instance administrator and warns that abuse
can cause upstream CAPTCHAs, IP bans and fewer results
([public versus private instances](https://docs.searxng.org/own-instance.html)).
The server supports a limiter specifically to rate-limit requests and block
bots
([server settings](https://docs.searxng.org/admin/settings/settings_server.html)).

Self-hosting avoids an external user account/key and gives Allo control of JSON
output and logs. However it adds a container/service lifecycle, upgrades,
resource use, abuse control and upstream search-engine blocking. The official
container installation requires Docker or Podman
([container installation](https://docs.searxng.org/admin/installation-docker)).

**Assessment:** never rotate across arbitrary public instances as a default
consumer API. Reconsider only as a separately approved Allo-operated service or
an optional advanced self-hosted endpoint; either choice changes the current
zero-operations product constraint.

## Excluded credentialed services

| Provider | Official requirement and free allowance | Why excluded |
| --- | --- | --- |
| Brave Search | Every API request needs `X-Subscription-Token`; account/subscription/key required. The free monthly credit still requires signup and anti-fraud card verification ([authentication](https://api-dashboard.search.brave.com/documentation/guides/authentication), [pricing](https://brave.com/search/api/), [official MCP](https://github.com/brave/brave-search-mcp-server)) | Free price is not keyless distribution |
| Tavily | REST requires an API key; remote MCP uses key or OAuth. Free plan is 1,000 credits/month and development keys are rate-limited ([API introduction](https://docs.tavily.com/documentation/api-reference/introduction), [credits](https://docs.tavily.com/documentation/api-credits), [MCP](https://docs.tavily.com/documentation/mcp)) | OAuth still requires user login; key violates the product constraint |
| Serper | Signup grants 2,500 free queries without a credit card; official material does not say this refreshes monthly ([official site](https://serper.dev/)) | Requires account/key; no official MCP found |
| Google Custom Search JSON API | Requires API key plus Programmable Search Engine ID; closed to new customers, with existing customers directed to migrate before 2027-01-01 ([official overview](https://developers.google.com/custom-search/v1/overview)) | Cannot support a new anonymous integration |
| Bing | Bing Search APIs were fully retired on 2025-08-11 ([retirement notice](https://learn.microsoft.com/en-us/lifecycle/announcements/bing-search-api-retirement)) | The former API is unavailable |
| Grounding with Bing Search | Replacement requires an Azure subscription, paid Bing resource, Foundry project and Azure identity ([official setup](https://learn.microsoft.com/en-us/azure/foundry/agents/how-to/tools/bing-tools)) | Not anonymous and not a drop-in search API |
| Jina Search | `s.jina.ai` is blocked without an API key; free-key tier is 100 RPM. The official MCP's search tool also requires a key ([Reader/Search limits](https://jina.ai/en-US/reader/), [official MCP](https://github.com/jina-ai/MCP)) | Search is not keyless |

Jina Reader is a separate case: `r.jina.ai` allows 20 RPM without a key and
converts a known URL to LLM-friendly text
([official limits](https://jina.ai/en-US/reader/)). It may be evaluated as a
future `web_extract` fallback, but cannot replace a search provider.

## Agent usefulness of result shapes

For the current `web_search(query, count)` contract:

1. **Parallel** is the strongest documented agent-oriented shape: ranked URLs
   plus dense, query-relevant compressed excerpts and native Markdown.
2. **You.com** is also strong: structured JSON, multiple snippets, publication
   dates and a separate news section. Allo can normalize this without parsing
   HTML.
3. **Exa** offers clean, ready-to-use content, but its hosted free MCP quota is
   opaque and observed 429 cooldowns reduce availability.
4. **SearXNG JSON** can normalize cleanly but quality and availability inherit
   the chosen instance and enabled upstream engines.
5. **DuckDuckGo HTML** yields conventional title/URL/snippet records. It is
   compact, but supplies less query-focused evidence and depends on fragile
   markup parsing.

All providers must still be normalized into bounded `SearchHit` values. Raw MCP
payloads, provider tool schemas and full page bodies must not be injected into
the model context.

## Mainland China and international access

| Provider | International evidence | Mainland-China evidence |
| --- | --- | --- |
| Parallel | Official global status page and Allo's existing proxy-environment probe | No official mainland availability/SLA; direct multi-network test required |
| You.com | Official endpoint and free quota documentation | No official mainland availability/SLA; direct multi-network test required |
| Exa | Official global status page tracks MCP/API | No official mainland availability/SLA; direct multi-network test required |
| DuckDuckGo | Official search supports a `cn-zh` result-region setting | A locale option is not reachability evidence; direct test required |
| SearXNG | Depends on the selected instance and its upstream engines | No general answer; an Allo-operated instance and each upstream engine need separate tests |
| Jina | Official status tracks `r.jina.ai` and `s.jina.ai` | Status data is global/region-limited and does not promise mainland access |

Do not document a provider as “China available” merely because its marketing
site opens once. Acceptance requires DNS, TLS, MCP/HTTP, consecutive calls,
concurrency, proxy-off behavior and multiple mainland ISP samples.

## Recommended next actions

1. Preserve the current adapters and cooldown logic while collecting a larger
   privacy-safe Exa sample: attempts, successes, 429 count, cooldown duration
   and fallback outcome only.
2. Add a development probe for
   `https://api.you.com/mcp?profile=free`; do not add production code until it
   passes anonymous `tools/list`/`tools/call`, schema, limits and output tests.
3. If You.com passes, compare two candidate chains under identical queries and
   direct-network conditions:
   `Parallel → You.com → Exa → DDG` and
   `Parallel → You.com → DDG`.
4. Keep DuckDuckGo last, but record its unsupported-HTML integration risk in
   release criteria.
5. Do not add arbitrary public SearXNG instances, credentialed providers, a
   shared paid key, or user-facing BYOK configuration without a separate
   architecture decision.

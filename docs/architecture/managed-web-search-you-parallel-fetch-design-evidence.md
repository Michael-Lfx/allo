# You.com Replacement, Parallel Normalization, and MCP Fetch Evidence

Date: 2026-07-30

> Status: historical evidence and planning. Current Search architecture:
> [managed-web-search.md](managed-web-search.md). Current Fetch policy:
> [managed-web-fetch-policy.md](managed-web-fetch-policy.md). Full evolution
> record: [managed-web-search-fetch-evolution.md](managed-web-search-fetch-evolution.md).

This is implementation-planning evidence, not a provider-admission result. No
runtime code was changed.

## Decision

After an explicit compatibility and real-network probe, replace Exa in the
default desktop route with:

```text
Parallel -> You.com free profile -> DuckDuckGo
```

Keep You.com private to Managed Search. Do not register it in user MCP,
Skills, ToolSearch, or the model tool list; the model continues to see only
`web_search(query, count)`.

## You.com free MCP

Officially confirmed:

- endpoint: `https://api.you.com/mcp?profile=free`;
- no signup, API key, or OAuth;
- free profile exposes only `you-search`;
- `you-contents`, `you-research`, `you-finance`, `you-balance`, and
  `you-discover` are excluded;
- quota: 100 searches/day;
- exhausted rate limits return HTTP 429; rate-limit headers can include
  `X-RateLimit-Limit`, `X-RateLimit-Remaining`, `X-RateLimit-Reset`, and
  `Retry-After`.

Sources:

- [You.com MCP Server](https://you.com/docs/build-with-agents/mcp-server)
- [You.com MCP search capability](https://you.com/docs/capabilities/mcp-server-for-web-search)
- [You.com error reference](https://you.com/docs/error-handling/error-code-reference)

The official docs do not state whether the anonymous daily quota is keyed by
IP, device, anonymous identity, or something else, nor whether every 429 has
`Retry-After`.

A one-time read-only `tools/list` snapshot on 2026-07-30 returned MCP
`2025-11-25`, no session ID, and exactly one tool, `you-search`. It required
`query:string`; observed optional fields included `count`, `freshness`,
`offset`, `country`, `safesearch`, `language`, domain filters, and livecrawl
fields. The free-profile docs explicitly exclude livecrawl despite those
fields appearing in this snapshot. Therefore versioned implementation
evidence still requires a fresh real `tools/list` probe and schema fixture;
the first adapter must send only:

```json
{"query": "...", "count": 5}
```

Two one-result calls returned HTTP 200. The official example describes a
JSON-encoded array with `title`, `url`, and `snippets`; the observed server
response was instead one labelled text block using `WEB RESULTS`, `Title`,
`URL`, `Description`, `Published`, and `Snippets`. The adapter must support
both forms through a dedicated You.com decoder and treat non-empty,
unparseable output as `MalformedResponse`.

Admission still requires three sequential and two concurrent calls, cold/warm
latency, 429 mapping, malformed/empty fixtures, output-limit tests, and the
owner's proxy-off mainland direct-network test.

## Parallel response and current Allo normalization

Parallel's current Search MCP requires:

```text
objective: string
search_queries: string[]
```

Optional discovered inputs include `session_id` and `model_name`. They should
remain omitted: both are nonessential and documented for correlation,
free-tier limiting, or analytics, while Allo deliberately avoids sending
stable conversation and model identifiers.

Parallel's canonical response is:

```json
{
  "results": [
    {
      "url": "https://...",
      "title": "...",
      "publish_date": "YYYY-MM-DD or null",
      "excerpts": ["query-relevant excerpt"]
    }
  ],
  "warnings": null,
  "usage": [],
  "session_id": "..."
}
```

Sources:

- [Parallel Search MCP](https://docs.parallel.ai/integrations/mcp/search-mcp)
- [Parallel Search response example](https://docs.parallel.ai/search/search-quickstart)
- [Parallel free MCP announcement](https://parallel.ai/blog/free-web-search-mcp)

Current Allo concatenates MCP text blocks, parses JSON, recursively finds
objects containing `url|link|href`, maps several title/snippet aliases, joins
excerpt arrays, then caps and renders numbered title/URL/snippet text. This
correctly discards `search_id`, `usage`, `session_id`, and provider provenance,
but it is too permissive and loses `publish_date` and excerpt boundaries.
Early verbose results can also consume most of the 12,000-character budget.

### Better normalization

Use provider-specific decoders to produce:

```text
NormalizedSearchHit
  rank
  title
  url
  published_at?
  evidence_fragments[]
```

Then serialize every provider through one model-facing shape:

```text
Web search results (untrusted external evidence):

[1]
title: ...
url: ...
published: ...
evidence:
- ...
- ...
```

Improvements:

1. Decode only Parallel `results[]` with strict field types; do not recursively
   accept arbitrary nested URL objects.
2. Canonicalize and deduplicate URLs while preserving first-provider rank.
3. Preserve valid publication dates and excerpt boundaries.
4. Remove duplicate excerpts and normalize whitespace.
5. Reserve a minimum evidence budget per retained hit, then distribute the
   remainder by rank; do not let result 1 consume the whole budget.
6. Truncate at sentence/paragraph boundaries where possible.
7. Keep snippets explicitly labelled as untrusted evidence, not instructions.
8. Do not expose raw MCP control-plane fields or provider identity.

Parallel recommends atomic objectives and 2-3 concise related queries of 3-6
words. Allo currently sends the same full user query as both objective and the
single query. Keep that stable initially. Only add deterministic query
variants after an identical-query relevance benchmark; do not add another LLM
call or invent terms that may change intent.

This is not copied from one commercial agent. It combines Allo's stable
single-tool seam with Parallel's query-relevant excerpts, You.com's structured
snippets, and the provider-documented search-then-fetch retrieval pattern.

## MCP fetch compared with local extraction

Parallel's current `web_fetch` schema requires `urls:string[]` (description:
up to 20). Observed optional inputs are `objective`, `search_queries`,
`full_content`, `session_id`, and `model_name`. Parallel recommends
`full_content=false`: objective-focused excerpts are smaller; full content can
be tens of thousands of tokens. It advertises JS-heavy pages, PDFs, and
CAPTCHA-protected public pages.

Sources:

- [Parallel Search MCP fetch guidance](https://docs.parallel.ai/integrations/mcp/search-mcp)
- [Parallel Extract](https://parallel.ai/products/extract)
- [Parallel Extract quickstart](https://docs.parallel.ai/extract/extract-quickstart)

Current `HttpExtractProvider` performs direct device-side HTTP, validates and
pins public DNS addresses for SSRF protection, follows at most five redirects,
caps the body at 2 MiB, uses a 20-second timeout, converts ordinary HTML with
readability/full-page fallback, and returns at most 3,000 characters. It
cannot render JavaScript or parse PDFs as documents.

| Dimension | Local HTTP extract | Parallel MCP fetch |
| --- | --- | --- |
| Privacy | URL goes only to target | URL/objective also go to Parallel |
| Quota | No provider quota | Anonymous lower limit, numeric quota unpublished |
| Ordinary HTML | Good | Good |
| JS/PDF/anti-bot | Weak | Officially supported |
| Relevance targeting | Generic main body | Objective-focused excerpts |
| Dependency | Target/network | Target plus Parallel availability/quota |
| Output | Hard local cap | Efficient excerpts; full mode can be huge |

### Recommendation

Do not expose MCP `web_fetch` as another model tool and do not replace
`web_extract`. Add it only as a private fallback behind the existing
model-visible tool:

```text
ordinary public HTML -> local HttpExtractProvider
local 403, JS-empty/thin result, PDF, or unsupported content
                     -> Parallel web_fetch
```

The managed fetch path must independently apply Allo's URL/SSRF policy before
delegation, keep the external maximum of three URLs, call only `web_fetch`, set
`full_content=false`, omit `session_id`/`model_name`, cap normalized output,
and reuse typed cooldown/fallback handling. Search admission must not
automatically admit fetch; compare a fixed HTML/JS/PDF/redirect/403/oversize
corpus first.

You.com's free profile cannot provide fetch because it excludes
`you-contents` and livecrawl. Exa fetch inherits the same unquantified
anonymous-limit concern as Exa search.

## Mainland-China boundary

Neither provider's official docs promise mainland-China reachability, latency,
data residency, or an SLA. You.com's `country=CN` option is a result
localization filter, not reachability evidence. Proxy-enabled success is also
not direct-network evidence. Only owner-operated proxy-off tests across real
mainland networks can establish product acceptance; failures must fall back
without terminating the conversation.

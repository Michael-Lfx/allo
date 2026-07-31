# Managed Web Search Next Goal

Date: 2026-07-30

> Status: historical implementation-planning note. Current Search architecture:
> [managed-web-search.md](../../architecture/web/managed-web-search.md). Full evolution record:
> [managed-web-search-fetch-evolution.md](../../architecture/web/managed-web-search-fetch-evolution.md).

## Baseline

The implementation baseline is commit `d3a034d1`:

```text
feat: add desktop managed web search
```

It provides a Desktop-only, process-shared `ManagedSearchService` with the
current first-success route:

```text
Parallel -> Exa -> DuckDuckGo
```

The model still sees only:

```text
web_search(query, count)
```

Parallel and Exa are private managed MCP peers. They are not registered in the
user MCP manager, Skills, ToolSearch, or the model-visible tool catalog. Web
Host and standalone CLI remain DuckDuckGo-only.

The baseline was accepted with real `bun run dev` conversation searches and
provider-selection logs. Parallel and DuckDuckGo succeeded. Exa also succeeded
initially, but repeated anonymous use produced HTTP 429 responses, including
the first Exa request after a new process start. The router correctly fell back
to DuckDuckGo.

## Why the next change is needed

Exa's hosted MCP documentation says that anonymous access is subject to a free
plan rate limit and recommends an API key for production use or after HTTP 429.
That does not fit Allo's fixed requirement that distributed desktop users must
not create accounts, complete OAuth, or configure keys.

You.com now exposes a keyless MCP profile:

```text
https://api.you.com/mcp?profile=free
```

Official documentation states that this profile:

- needs no signup, API key, or OAuth;
- exposes only `you-search`;
- excludes contents, research, finance, balance, discovery, and livecrawl;
- allows 100 searches per day.

A read-only development snapshot found exactly one tool and two anonymous
search calls returned HTTP 200. The observed MCP result was labelled text,
while official examples describe structured JSON. You.com therefore remains a
candidate until the full admission probe freezes its real schema and behavior.

The current Parallel/Exa parser is intentionally permissive: it recursively
searches arbitrary JSON for URL-bearing objects and guesses common title and
snippet fields. That enabled the first integration quickly, but it can silently
accept provider schema drift, loses Parallel publication dates and excerpt
boundaries, and lets early verbose results consume most of the shared context
budget.

## Objective for the next code commit

Deliver one cohesive change:

```text
Provider-specific decoding + You.com admission + Exa removal
```

The target default Desktop route is:

```text
Parallel -> You.com free -> DuckDuckGo
```

If the You.com admission probe fails, do not ship a partial You adapter. Use:

```text
Parallel -> DuckDuckGo
```

instead.

## Required design

### External interface remains stable

Do not change the model tool name or arguments:

```text
web_search(query, count)
```

Do not add provider selection, account setup, OAuth, API keys, UI, prompts,
database migrations, background probes, or model-visible provider provenance.

### Provider-specific decoders

Replace the generic result-field guesser with private provider decoders:

```text
ParallelDecoder
YouSearchDecoder
DuckDuckGoDecoder
        |
        v
NormalizedSearchHit
        |
        v
one bounded model-facing formatter
```

The private normalized representation should preserve:

```text
rank
title
url
published_at?
evidence_fragments[]
```

Parallel decoding must accept only its verified `results[]` shape. You decoding
must support only the two verified forms: the official structured JSON shape
and the observed labelled-text shape. Non-empty unparseable output is
`MalformedResponse`, not an empty success.

### MCP structured output support

Extend the minimal remote peer only as needed to retain:

```text
Tool.outputSchema?
CallToolResult.structuredContent?
```

Prefer validated `structuredContent` when present. Fall back to the provider's
verified text representation for legacy compatibility. Keep protocol concerns
in `nomi-mcp` and provider field knowledge in `flowy-web`.

### Deterministic normalization

The shared normalizer must:

- accept only public `http` and `https` result URLs;
- canonicalize and deduplicate URLs while preserving first rank;
- preserve valid publication dates;
- retain evidence-fragment boundaries;
- normalize whitespace and remove duplicate fragments;
- allocate a minimum evidence budget per retained hit before distributing the
  remaining 12,000-character budget by rank;
- truncate at a sentence or paragraph boundary when practical;
- expose no provider name, session ID, search ID, usage, or raw control fields;
- label web excerpts as untrusted external evidence.

Do not add an LLM normalization or reranking call.

## You.com admission gate

Before production construction, the explicit probe must record:

- DNS, TLS, endpoint, protocol version, and session behavior;
- exact `tools/list` output and required input schema;
- exactly one allowed tool: `you-search`;
- real anonymous `tools/call` with only `query` and bounded `count`;
- three consecutive calls;
- two concurrent calls;
- cold and warm latency;
- empty and malformed result behavior;
- HTTP 401, 403, 429, 5xx, timeout, and `Retry-After` behavior when observable;
- body and output limits;
- current proxy-enabled reachability.

The provider is not admitted if it requires an account, OAuth, an API key, or
additional tools. Anonymous quota identity remains unknown unless You.com
documents it; do not infer per-device or per-IP behavior.

Proxy-off mainland reachability remains an owner-operated product acceptance
step after automated checks.

## Production changes

After the gate passes:

1. Add `SearchProviderId::You`.
2. Add the fixed free-profile endpoint and exact `you-search` allow-list.
3. Map `SearchQuery` to only `query` and `count`.
4. Insert You.com between Parallel and DuckDuckGo with a three-second provider
   budget.
5. Remove Exa from production construction and production parsing.
6. Keep Exa only in the explicit probe/history for one review cycle, then
   remove it separately if no longer useful.
7. Remove any local development provider-disable override before real
   acceptance.
8. Update the architecture document and provider matrix with observed facts,
   not assumptions.

## Tests

All normal tests remain offline and use fixtures or local mock servers.

Required cases:

- Parallel strict JSON success;
- Parallel publication date and multiple excerpts preserved;
- You structured JSON success;
- You labelled-text success;
- You non-empty malformed output fails closed;
- duplicate and non-HTTP(S) URLs are removed;
- evidence budget is deterministic and fair across results;
- Parallel success does not call You or DuckDuckGo;
- Parallel empty/failure falls through to You;
- You 429 observes cooldown and falls through to DuckDuckGo;
- You tool/schema mismatch disables it for the process;
- all providers failing returns one safe error;
- cancellation and per-provider concurrency behavior remain unchanged;
- Web Host and CLI remain DuckDuckGo-only;
- the model tool list still contains one `web_search`;
- provider MCP tools and provenance remain absent from model context.

Minimum verification:

```text
cargo test -p flowy-web
cargo test -p nomi-mcp
cargo test -p nomi-agent --test bootstrap_test
cargo test -p nomifun-ai-agent --features managed-search --test managed_search_handle
cargo check -p nomifun-ai-agent
cargo check -p nomifun-app --features managed-search
cargo check -p nomifun-web
cargo fmt --all -- --check
git diff --check
```

Repository-wide baseline failures must be reported separately from these
focused gates.

## Explicitly out of scope

The next code commit must not add Parallel `web_fetch`, You.com contents or
livecrawl, a new model tool, an optional `web_extract` focus argument, search
aggregation, hedged requests, LLM reranking, provider UI, user credentials,
query/result caching, or background network traffic.

Parallel MCP fetch remains a later independent goal. Its intended shape is a
private fallback behind the existing `web_extract` interface for local HTTP
403 responses, JavaScript-empty pages, PDFs, unsupported content types, or
unusable thin extraction. It requires its own probe, threat review, output
budget, and acceptance commit.

## Rollback

Commit `d3a034d1` is the rollback point.

- If typed decoding regresses Parallel, revert to the baseline decoder.
- If You.com fails admission, ship `Parallel -> DuckDuckGo`.
- If host isolation regresses, disable the Desktop managed-search capability
  and retain Bootstrap DuckDuckGo.
- Do not modify user MCP configuration or the existing local
  `HttpExtractProvider` as part of this goal.

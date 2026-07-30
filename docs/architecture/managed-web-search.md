# Managed Web Search

## Status

Accepted for the first desktop-only implementation.

## Decision

The model-facing interface remains the existing `web_search(query, count)`
tool. The desktop host may inject one process-wide `SearchProvider`
implementation, `ManagedSearchService`, behind that interface. The managed
implementation tries fixed, application-owned providers in first-success
order:

```text
Parallel -> You.com Free -> DuckDuckGo
```

The You.com free profile is included only when its unauthenticated
`you-search` admission contract is available. A provider that needs an
account, OAuth, or an API key is outside this version. Exa is not part of the
production chain.

`ManagedSearchService` and its provider adapters are application-managed
capabilities, not user MCP servers. They must not enter the user MCP
configuration, MCP management UI, skill loading, ToolSearch, or model tool
registry. Provider endpoints and allowed tool names are fixed in their
adapters.

## Host ownership

The desktop host enables managed search through both a Cargo feature and an
explicit runtime host capability. The runtime capability is authoritative:
Cargo workspace feature unification must never make the standalone web host
or Nomi CLI opt in accidentally. Hosts that do not inject a provider retain
the current DuckDuckGo-only bootstrap behavior.

Construction is lazy with respect to the network. Creating application
services, an agent factory, or a conversation runtime must not contact a
search provider. The first real `web_search` call performs any required
protocol discovery.

## Shared and private state

The desktop process shares:

- HTTP clients and connection pools;
- probe-selected protocol client state and tool-schema discovery;
- per-provider health and cooldown state;
- per-provider concurrency limits;
- legacy initialization single-flight, if a legacy provider is required.

The process does not share:

- queries, results, or tool results;
- conversation messages or model context;
- per-turn deadlines or cancellation;
- cross-conversation search caches.

Cancelling one conversation drops only that call and its provider permit. It
must not close a shared client, cancel another conversation, or reset shared
health.

The host-to-agent binding is explicit and three-state:

```text
DefaultDdg -> construct DDG lazily and register web_search when construction succeeds
Provided(provider) -> register the injected process-wide provider
Disabled -> skip web_search, but keep local web_extract available
```

Desktop construction degrades from Managed Search to a DDG-only managed
handle, then to `Disabled` if both constructions fail. A construction failure
is recorded only as a safe category and never prevents the desktop host from
starting. `tools.web.enabled=false` takes precedence and registers neither
web tool.

## Routing policy

The router returns the first non-empty successful result. An empty successful
result falls through without affecting provider health. Typed provider errors
drive provider-local cooldown or disablement; callers receive one safe
top-level search error only after all eligible providers fail.

This version does not aggregate providers, hedge requests, rerank with an LLM,
or cache search results. DuckDuckGo is a best-effort final fallback, not an
SLA-backed provider.

## Protocol ownership

MCP transport and protocol behavior belongs to `nomi-mcp`. Provider-specific
tool names, arguments, result parsing, and endpoints belong to `flowy-web`.
Routing and health policy must not enter the MCP implementation.

The managed remote peer accepts only the negotiated MCP protocol version
`2025-11-25`. It supports both sessionless and stateful peers under that
version, validates response correlation, and keeps structured tool output.
Each Ready peer transport carries a monotonic generation. A stateful 404 can
invalidate only the generation that issued the failed request, so a late
response cannot clear a newly established session. JSON-RPC envelopes must
declare version `2.0` and contain exactly one result or error.
Existing user `McpManager` behavior is left unchanged. A provider-specific
decoder, rather than a generic recursive URL guesser, is the contract for
Parallel and You.

Provider queue saturation is a request-local `QueueBusy` outcome. Queue wait
uses the same provider deadline as the network attempt, falls through without
calling health `record_error`, and therefore does not cool down a healthy
provider. Decoder diagnostics are private and contain only source, fallback,
drop count, and a contract-degraded flag; queries, URLs, evidence, and raw
remote payloads are never logged.

## Product behavior

Managed search is keyless and opens without additional prompts. It adds no
consent state, settings UI, provider UI, database migration, or user-supplied
endpoint. The existing `tools.web.enabled` switch remains authoritative.

There is no background probing. Diagnostics may record a random request ID,
provider, attempt order, elapsed time, result count, fallback count, error
class, queue wait, decoder source, dropped-item count, contract-degraded flag,
and truncation. They must not record the complete query, result body,
conversation content, full URL list, evidence, structured content, or a stable
query hash.

Local `web_extract` remains the only page-content path in this version. Its
model-facing result begins with an untrusted-evidence instruction and removes
provider/extractor provenance. The renderer preserves input order, keeps
success-page bodies fair, marks source or renderer truncation, and caps the
final rendered output at 8,000 characters. Existing SSRF, redirect, timeout,
Readability, response-body, and 3,000-character-per-page limits are unchanged.
MCP Fetch, remote page-content tools, and intermediate LLM rewriting remain
out of scope.

## Rollback

Disabling the desktop host capability restores the existing DuckDuckGo-only
bootstrap path. If You.com fails compatibility, quota, or anonymous-call
acceptance, the router falls back to DuckDuckGo without changing the public
`web_search` interface. Exa remains historical probe evidence only.

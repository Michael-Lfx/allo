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
Existing user `McpManager` behavior is left unchanged. A provider-specific
decoder, rather than a generic recursive URL guesser, is the contract for
Parallel and You.

## Product behavior

Managed search is keyless and opens without additional prompts. It adds no
consent state, settings UI, provider UI, database migration, or user-supplied
endpoint. The existing `tools.web.enabled` switch remains authoritative.

There is no background probing. Diagnostics may record a random request ID,
provider, attempt order, elapsed time, result count, fallback count, error
class, and truncation. They must not record the complete query, result body,
conversation content, full URL list, or a stable query hash.

## Rollback

Disabling the desktop host capability restores the existing DuckDuckGo-only
bootstrap path. If You.com fails compatibility, quota, or anonymous-call
acceptance, the router falls back to DuckDuckGo without changing the public
`web_search` interface. Exa remains historical probe evidence only.

# Managed Web Search: You.com Rollout

Date: 2026-07-30
Baseline: `d3a034d1` (`feat: add desktop managed web search`)

## Decision

The Desktop managed search route is:

```text
Parallel -> You.com Free -> DuckDuckGo
```

The model-facing contract remains exactly:

```text
web_search(query, count)
```

Only the Desktop Host constructs `ManagedSearchService`. Web Host and the
standalone CLI remain DuckDuckGo-only. Parallel and You.com are private managed
adapters: they are not user MCP servers, Skills, ToolSearch entries, or
model-visible tools. No account, OAuth flow, API key, provider UI, migration,
background probe, query cache, or provider provenance is introduced.

MCP Fetch, You.com Contents, livecrawl, aggregation, hedged requests, and LLM
result rewriting are separate future work and are out of scope here.

## Admission evidence

You.com Free Profile:

```text
https://api.you.com/mcp?profile=free
```

The documented profile is unauthenticated, exposes `you-search`, and has a
published daily limit. The local admission evidence observed MCP
`2025-11-25`, no `MCP-Session-Id`, one `you-search` tool, an input schema with
`query` and optional `count`, an output schema, and a result containing both
structured content and a text compatibility copy. A `count=3` call returned
separate Web and News sections, so the adapter must enforce Allo's final total
count after merging and deduplication.

The evidence is protocol and shape evidence only. Proxy-off mainland
reachability, anonymous quota identity, and long-run rate-limit behaviour remain
owner-operated product acceptance items. Raw responses, queries, Session IDs,
search UUIDs, and result bodies are not committed.

## MCP peer invariants

The public `RemoteMcpPeer` Interface remains `discover_tools`, `call_tool`, and
`shutdown`. Its private state is:

```rust
enum PeerState {
    Uninitialized,
    Ready {
        protocol_version: ProtocolVersion,
        session: SessionMode,
        tools: Option<Vec<McpToolDef>>,
    },
}

enum SessionMode {
    Sessionless,
    Stateful(SessionId),
}
```

The first version accepts only `2025-11-25`. The Ready state is committed only
after initialize, version validation, and `notifications/initialized` succeed.
The negotiated version is used on every later HTTP request. Request IDs are
monotonic for the Peer lifetime and response IDs must match the request ID.

Sessionless is valid: no Session header is sent and shutdown sends no DELETE.
Only a request that carried a Stateful Session ID can turn HTTP 404 into a
`SessionExpired` recovery. The Peer clears its complete state and does not
retry; the Provider Adapter owns one complete re-discovery/retry.

Streamable HTTP SSE is parsed by expected response ID. Notifications are
ignored after validation. Server Requests are rejected as unsupported rather
than silently discarded. Body, event, event-count, deadline, and cancellation
limits remain unchanged.

Managed initialization sends an empty client capability object through a
private wire payload. The existing shared `ClientCapabilities` type and user
MCP Manager initialization path are not rewritten.

## Provider decoding and normalization

Provider field knowledge stays behind private adapters:

```text
Parallel Contract -> ParallelDecoder
You Contract      -> YouDecoder
DuckDuckGo        -> existing HTML adapter
```

The private normalized hit preserves title, canonical URL, optional validated
publication date, evidence fragments, and original rank. Parallel accepts only
its verified `results[]` shape. You accepts structured content first, then its
verified JSON/text compatibility forms. Non-empty unparseable output is a
malformed response, never a successful empty result.

Allo's final normalization:

- accepts only HTTP/HTTPS URLs with a host and no credentials;
- lowercases scheme/host, removes default ports and fragments, and preserves
  path/query/UTM parameters;
- keeps first rank/title and lets duplicates fill only missing dates and new
  evidence fragments;
- interleaves You Web and News as `W0, N0, W1, N1...`;
- keeps at most four evidence fragments per hit and 2,000 characters per
  fragment;
- reserves 256 evidence characters per retained hit, then allocates remaining
  space by rank under the 12,000-character model budget;
- drops lowest-ranked hits if fixed fields and minimum evidence cannot fit;
- renders all results as untrusted external evidence and exposes no provider,
  Session, usage, or raw control fields.

## You.com rate policy

`Retry-After` supports both delay-seconds and HTTP-date. You clamps a hinted
delay to 30 seconds through 24 hours. A first consecutive 429 without a hint
uses a 30-minute cooldown. A second such 429, with no successful valid result
between them, disables You until process restart. Successful valid responses,
including valid empty results, reset temporary failure and unhinted-rate-limit
counters; they do not clear a permanent disable reason.

Parallel keeps its shorter 30-second-to-15-minute hinted cooldown policy. HTTP
401, RPC method unavailable, fixed-tool missing, and schema mismatch disable a
Provider for the process. Invalid requests do not poison health.

## Execution commits

1. `docs(web): define You.com managed search rollout` — this document only.
2. `test(probe): add You.com MCP admission probe` — the explicit developer
   probe; normal mode performs one call, `--raw` exposes raw data, and an
   explicit admission mode performs the stress checks.
3. `fix(mcp): support sessionless peers and negotiated protocol versions` —
   Peer state, version, IDs, SSE correlation, 404 semantics, Retry-After, and
   protocol tests.
4. `feat(mcp): preserve output schemas and structured tool content` — additive
   MCP result fields and offline deserialization tests.
5. `refactor(web): add typed managed search decoders` — Parallel and You
   decoders, normalization, fixtures, and bounded model formatting. Exa keeps
   the old parser temporarily so the commit remains independently buildable.
6. `feat(web): replace Exa with You.com in managed search` — You route, health
   policy, one-shot cache recovery, Exa production removal, and final docs.

Each commit is explicitly staged and individually verified. The four existing
untracked research/protected files remain untouched. No workflow YAML is
created or modified.

## Verification and rollback

Focused gates are offline and include `nomi-mcp`, `flowy-web`, bootstrap,
managed-search handle, Desktop/Web Host compilation, formatting, and diff
checks. The broad workspace and Bun checks are reported separately if an
existing baseline failure is reproduced. Final product acceptance is the
owner-operated `bun run dev` test with proxy-on and proxy-off network checks.

The rollback point is `d3a034d1`. If You admission or real acceptance fails,
remove You from the route and use `Parallel -> DuckDuckGo`; do not add keys or
change user MCP configuration.

# Managed Web Search Provider Matrix

This file records explicit, developer-run compatibility probes. It is not a
claim of universal availability and is never refreshed by the application.

## 2026-07-29 initial probe (historical)

Environment:

- host: Windows desktop development machine;
- network: current developer network environment, including its configured
  proxy;
- authentication: no API key, OAuth, account, or provider-specific headers;
- direct-network acceptance: pending owner verification with the proxy
  disabled.

| Provider | Endpoint | Anonymous call | MCP era/version | Search tool | Result |
| --- | --- | --- | --- | --- | --- |
| Parallel | `https://search.parallel.ai/mcp` | Passed: 3 sequential + 2 concurrent calls | Legacy initialization; Modern probe returned HTTP 400 / JSON-RPC `-32600` | `web_search`; required `objective`, `search_queries` | Admitted |
| Exa | `https://mcp.exa.ai/mcp?tools=web_search_exa` | Passed anonymously; a repeated two-way concurrency probe also produced one HTTP 429 | Historical MCP probe | `web_search_exa`; required `query` | Historical evidence only; removed from the production chain |
| DuckDuckGo | Existing HTML endpoint | Existing behavior | Not MCP | Built-in adapter | Baseline fallback |

Observed response profile:

- Parallel returned one text content item of approximately 28,000 characters;
  calls completed in approximately 1.4-2.2 seconds. The text payload parsed as
  JSON with a `results` field and ten URL-bearing result objects. Its catalog
  exposed two tools; the adapter allow-lists only `web_search` and never calls
  the other tool.
- Exa returned one text content item of approximately 4,800 characters; the
  cold call completed in approximately 1.4 seconds and warm calls in
  approximately 0.3-0.5 seconds. Its text payload used a non-JSON labelled
  result format with one URL line for the one-result probe. The
  `tools=web_search_exa` endpoint returned exactly one tool and no unexpected
  tools.
- Parallel and Exa accepted unauthenticated calls without an account, OAuth, an
  API key, or provider-specific headers in this historical probe. Exa's
  repeated rate-limit observation is why it is not a production dependency.
- The historical probe used the compatibility path available at that time.
  The current managed peer is version-gated to negotiated `2025-11-25` and
  supports both sessionless and stateful peers. The existing user `McpManager`
  remains unchanged.
- 401, 403, and 5xx responses were not deliberately induced against the public
  services. A repeated Exa concurrency probe naturally returned one HTTP 429;
  all runtime classifications are also covered by offline tests.

The probe records status codes, protocol metadata, tool names, schema shape,
latency, and response size only. It does not persist the fixed probe query or
provider response text.

Admission rules:

- Parallel is omitted if an unauthenticated real search call fails for a
  provider-contract reason.
- Exa is not admitted to the current production chain even though the
  historical probe was keyless; it remains only as documented historical
  evidence.
- Network reachability is recorded separately from protocol/authentication
  compatibility; per-device runtime health handles later reachability changes.
- Only protocol version `2025-11-25` is accepted by the managed peer.

## 2026-07-30 You.com replacement admission

The current fixed free-profile endpoint is:

```text
https://api.you.com/mcp?profile=free
```

| Provider | Endpoint | Anonymous call | MCP version/session | Search tool | Production decision |
| --- | --- | --- | --- | --- | --- |
| You.com Free | `https://api.you.com/mcp?profile=free` | Keyless admission probe passed; direct-network owner acceptance remains separate | Negotiated `2025-11-25`; sessionless in the recorded probe | Exactly one `you-search`; only `query` and `count` are sent | Admitted between Parallel and DuckDuckGo |

The observed structured result separates `results.web` and `results.news`;
the adapter interleaves them `W0,N0,W1,N1...`. The decoder also accepts the
verified JSON-in-text and labelled text compatibility forms. It retains only
validated HTTP(S) URLs, dates, and bounded evidence fragments. MCP Fetch,
Contents, livecrawl, and all other You.com tools remain outside the route.

The application does not persist the probe query, response body, session ID,
search UUID, or full URL list. Proxy-off mainland reachability remains an
owner-operated product acceptance step.

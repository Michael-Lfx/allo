# Managed Web Search Provider Matrix

This file records explicit, developer-run compatibility probes. It is not a
claim of universal availability and is never refreshed by the application.

## 2026-07-29 initial probe

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
| Exa | `https://mcp.exa.ai/mcp?tools=web_search_exa` | Passed anonymously; a repeated two-way concurrency probe also produced one HTTP 429 | Legacy initialization; Modern probe returned HTTP 400 / JSON-RPC `-32000` | `web_search_exa`; required `query` | Admitted with per-provider concurrency and 429 cooldown, pending owner direct-network check |
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
- Both providers accepted unauthenticated calls without an account, OAuth, an
  API key, or provider-specific headers.
- Both providers require the legacy initialization path at the time of this
  probe. Managed Search therefore implements an isolated minimal legacy remote
  peer. The existing user `McpManager` remains unchanged.
- 401, 403, and 5xx responses were not deliberately induced against the public
  services. A repeated Exa concurrency probe naturally returned one HTTP 429;
  all runtime classifications are also covered by offline tests.

The probe records status codes, protocol metadata, tool names, schema shape,
latency, and response size only. It does not persist the fixed probe query or
provider response text.

Admission rules:

- Parallel is omitted if an unauthenticated real search call fails for a
  provider-contract reason.
- Exa is omitted if it requires an account, OAuth, or an API key.
- Network reachability is recorded separately from protocol/authentication
  compatibility; per-device runtime health handles later reachability changes.
- Only protocol eras demonstrated by an admitted provider are implemented.

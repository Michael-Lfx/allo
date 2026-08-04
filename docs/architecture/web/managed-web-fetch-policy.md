# Local-first Managed Extract Policy

Date: 2026-08-02
Last updated: 2026-08-04

This policy governs the private Parallel `web_fetch` fallback behind the
existing `web_extract` tool. Desktop now starts in the evidence-backed profile
after the completed PDF/JavaScript acceptance. `NOMIFUN_MANAGED_FETCH_MODE=off`
is the emergency Local-only rollback; an explicit `evidence-backed` value is
equivalent to the default.

Full evolution record and lessons:
[managed-web-search-fetch-evolution.md](managed-web-search-fetch-evolution.md).

## Model Surface

The model continues to see only:

```text
web_search
web_extract
```

No `parallel_web_fetch`, MCP tool, provider, extractor, or fallback name is
exposed to the model. `final_url` and provider diagnostics stay outside model
context.

### Model routing contract

`web_extract` is the sole model-visible reader for a known public HTTP(S) URL.
Its tool description and the generic agent guidance explicitly cover HTML,
direct PDF text, JavaScript-shell pages, and short or empty pages. A URL ending
in `.pdf` is therefore an instruction to try `web_extract` first, not a reason
to create a download or parsing script.

`web_search` is for discovery: when the user or current context already gives a
public direct URL, the model should skip search and call `web_extract`. Browser
is reserved for interaction (clicking, login, or a browser-only rendering
workflow); Bash, Python, and `exec_command` remain appropriate for local
artifacts or a genuine `web_extract` failure. An explicit request to download
or save an original file is an artifact/file workflow, not content extraction.

This is a routing aid, not an authorization rule. It never promises that every
URL reaches Remote: Local-first routing, the evidence-backed category profile,
budget, URL safety, and source contract still decide whether a private fallback
is attempted.

## Routing

Each `web_extract` batch follows:

```text
Local-first
-> Remote once
-> Final
```

Local successes never enter the remote stage. A remote stage is attempted at
most once per batch. Local retries and remote retries after a remote stage are
forbidden. Remote results are mapped only by canonical requested URL; any
index/position fallback is forbidden. One remote result for a canonical URL may
fan out to every original request index.

## Remote Eligible

The evidence-backed Desktop profile may send these to Parallel only after local
failure:

- PDF
- JavaScript shell with no useful rendered body
- Empty content

The following remain policy-deferred and Local-only until separate production
evidence is approved:

- Unsupported public document content types
- Unclassified Local no-valid-body outcomes (only the explicit `EmptyContent`
  classification is currently eligible)
- Public URL transient DNS, TLS, or network failure
- Local timeout when the remaining tool budget is sufficient

## Remote Forbidden

The first version never sends:

- HTTP 401, 403, 404, 410, or 429
- Other 4xx and 5xx responses
- Local parse errors
- CAPTCHA, WAF challenge, paywall, or login pages
- Private or local addresses
- `localhost`
- Link-local addresses
- Non-HTTP(S) schemes
- URLs containing username/password
- URLs whose query looks like a token, signature, credential, or presigned URL
- URLs whose fragment contains a token, credential, session, or auth key

502, 503, and 504 remain disabled until separate probe and acceptance data
justifies them.

## Sensitive URLs

Sensitive URL semantics are:

```text
Local extraction: allowed
Remote sending: forbidden
```

A sensitive URL is still fetched locally. If local extraction succeeds, the
content is returned normally. If local extraction fails, the original local
error is returned and Parallel is never contacted.

Local SSRF and Remote egress share the same crate-private host/IP safety
primitives: lowercase hosts, trailing-dot normalization, special-use and
single-label domain rejection, and non-public address ranges. Remote egress
then applies the additional credential and decoded query/fragment parameter
checks, rejecting token, signature, OAuth, session, and presigned-key variants
(including camelCase and encoded names). Ordinary parameters such as
`filename` remain allowed. The complete Remote proof runs again immediately
before quota reservation and `tools/call`.

The model-visible `requested_url` keeps the original fragment for local error
and output attribution. The actual Parallel outbound URL is derived separately:
non-sensitive fragments are stripped before sending, while sensitive fragments
are forbidden from remote entirely.

## Privacy

The first version sends only:

```text
URLs that pass remote admission
```

It never sends:

- User question
- Search query
- Conversation history
- System prompt
- Cookie or Authorization
- Browser login state
- Local session or conversation IDs
- Tool use IDs
- Local error bodies
- Sensitive fragments or raw requested URLs with fragments

`objective`, `search_queries`, `session_id`, and `model_name` are omitted.

## Content Completeness

`source_truncated` is true when Parallel returns excerpts or an explicit
truncated result. `context_truncated` remains the Allo-side 3,000/8,000
character budget truncation. The model only receives the unified `truncated`
flag.

## Remote Readiness & Health

Readiness is based on the live `RemoteMcpPeer` state, not a historical success
boolean:

```text
ColdTransport -> peer not initialized
WarmTransportToolUnknown -> peer initialized, fetch compatibility not verified
Ready -> peer initialized and current-generation fetch schema verified
```

Fetch compatibility is bound to the peer generation. Session expiry or an
explicit unknown tool invalidates both the peer tools cache and the adapter
compatibility cache before at most one rediscovery.

Search and Fetch share transport-level endpoint health for 401, 403, 429,
network failure, and MCP protocol/transport malformed responses. Fetch
tool-level upstream errors, decoder malformed responses, and tool timeouts use
a separate Fetch-only cooldown and do not disable Parallel Search.

### Remote start budget

Readiness is an input to a start decision, not proof that a network call has
already happened. With the existing 12-second `web_extract` deadline, the
Parallel production policy requires the following remaining time after Local
classification:

```text
Ready                     >= 4 seconds
WarmTransportToolUnknown  >= 6 seconds
ColdTransport             >= 8 seconds
```

The provider receives the original absolute deadline, so these values only
prevent starting a call that cannot finish safely; they do not extend the
deadline. No background Fetch discovery is performed for ordinary HTML. If a
qualified PDF/JavaScript/Empty failure has less remaining time than its
threshold, the Local error is returned with a budget-deferred diagnostic.

Endpoint health uses an attempt epoch. A success that started before a newer
401/429/network failure cannot clear the newer disable or cooldown. An
`Unauthorized` state is cleared only after a new MCP session generation has
successfully reinitialized; an ordinary late tool success is insufficient.

## Context Budget

The existing budgets remain unchanged:

```text
Single page model body <= 3,000 characters
Whole web_extract ToolResult <= 8,000 characters
```

Remote fetch cannot bypass these budgets.

## Host Capability

Desktop owns a `ManagedWebHandle` that shares one Parallel MCP peer between
Managed Search and Managed Extract.

```text
managed_search=true
managed_extract=Disabled | EvidenceBacked
```

`EvidenceBacked` currently permits only PDF, JavaScript shell, and Empty
Content fallback categories. The default is `EvidenceBacked`. Blank or unknown
`NOMIFUN_MANAGED_FETCH_MODE` values fail closed to `Disabled` and emit a
structured warning.

If Parallel initialization fails:

- Search falls back through the existing Parallel -> You.com -> DDG logic or
  DDG-only path.
- Extract stays Local-only.

If only the Fetch tool is incompatible:

- Search remains Managed.
- Extract stays Local-only.

## Lifecycle

The shared Parallel peer is shutdown exactly once by the process-level
`ManagedWebHandle`. Repeated shutdown is idempotent and bounded by an outer
timeout so Browser and Database cleanup can still run. Concurrent tool
discovery is serialized inside `RemoteMcpPeer`, so Search/Fetch initialization
never emits duplicate `tools/list` requests for the same peer.

Admission Campaigns also hold a per-campaign run lock for their whole lifetime;
another process fails closed instead of competing for pending batches. A
cumulative campaign cap is terminal when pending work remains, while a fully
completed final batch still produces its summary even if it exactly consumes
the cap.

## Diagnostics

Logs record counts and timing only:

- requested, local success/failure, final success/failure
- remote policy candidates/deferred/eligible/forbidden/budget-skipped
- remote attempted/success/failure and fallback/forbidden reason counts
- timeout category counts (per-url, tool deadline, remote queue/call)
- source_truncated_count and context_truncated_count
- source-contract, safety rejection, recovery, provider-unavailable and
  provider-init-failure counts
- remote queue/call time and total elapsed
- readiness, remaining budget, minimum start budget, and budget decision

Logs never record URLs, query parameters, page titles, bodies, user questions,
conversation IDs, or raw MCP payloads. `url_index` may be recorded.

### Acceptance evidence interpretation

For a PDF or JavaScript-shell Local failure, `remote_attempted=true` and a
positive `remote_success_count` are the evidence that the managed Remote stage
was actually used. `remote_attempted=false` with `remote_budget_skipped_count`
means no Remote call occurred, even if a later Browser or script succeeds. For
ordinary HTML, `local_success_count>0` with `remote_attempted=false` is the
expected Local-only outcome. Tool-registration log lines are not tool calls;
inspect the actual tool-use record and the managed-extract completion counters.

## Limits

This capability does not guarantee bypassing anti-bot, CAPTCHA, or access
control. It is a bounded fallback for local extraction gaps, not a general
remote fetch service.

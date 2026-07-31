# Local-first Managed Extract Policy

Date: 2026-07-31

This policy governs the private Parallel `web_fetch` fallback behind the
existing `web_extract` tool. It is the production policy for the managed
Desktop capability once real acceptance passes. Until then, Desktop keeps
`managed_extract=false`.

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

## Routing

Each `web_extract` batch follows:

```text
Local-first
-> Remote once
-> Final
```

Local successes never enter the remote stage. A remote stage is attempted at
most once per batch. Local retries and remote retries after a remote stage are
forbidden.

## Remote Eligible

The first version may send these to Parallel only after local failure:

- PDF
- JavaScript shell with no useful rendered body
- Unsupported public document content types
- Local extraction with no valid body
- Public URL transient DNS, TLS, or network failure
- Local timeout when the remaining tool budget is sufficient

## Remote Forbidden

The first version never sends:

- HTTP 401, 403, 404, 410, or 429
- CAPTCHA, WAF challenge, paywall, or login pages
- Private or local addresses
- `localhost`
- Link-local addresses
- Non-HTTP(S) schemes
- URLs containing username/password
- URLs whose query looks like a token, signature, credential, or presigned URL

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

`objective`, `search_queries`, `session_id`, and `model_name` are omitted.

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
managed_extract=false (until real acceptance)
```

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
timeout so Browser and Database cleanup can still run.

## Diagnostics

Logs record counts and timing only:

- requested, local success/failure, remote eligible/forbidden/budget-skipped
- remote attempted/success/failure
- remote queue/call time and total elapsed

Logs never record URLs, query parameters, page titles, bodies, user questions,
conversation IDs, or raw MCP payloads. `url_index` may be recorded.

## Limits

This capability does not guarantee bypassing anti-bot, CAPTCHA, or access
control. It is a bounded fallback for local extraction gaps, not a general
remote fetch service.

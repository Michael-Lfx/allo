# Managed Fetch model tool-routing decision

Date: 2026-08-04

## Decision

The model-facing content-reading contract is explicit: when a user supplies a
known public HTTP(S) URL and asks to read, extract, or summarize it, the Agent
uses the existing `web_extract` tool first. This includes ordinary HTML, direct
PDF text, JavaScript-shell pages, and short or empty pages.

The contract does not expose Parallel, MCP, a provider name, payloads, or
`final_url`. It also does not promise Remote egress: Local-first extraction and
the private policy remain the only authority for any fallback.

## Why

The managed PDF/JavaScript fallback was working, but the previous tool wording
emphasized article bodies and did not make direct-PDF or JavaScript-shell
reading explicit. The Agent could therefore choose Browser or create a Python
download/parsing script before trying the existing reader. That detour both
adds latency and bypasses the intended Local-first/managed-fallback path.

## Implemented contract

The following surfaces carry the same provider-free routing guidance:

- `WebExtractTool` description and its `urls` schema description;
- `web_search` description, which says known direct URLs do not need search;
- generic agent context guidance; and
- Browser preset guidance.

Browser remains for interaction or browser-only rendering. Bash, Python, and
`exec_command` remain valid for local-file/artifact workflows or after an
actual `web_extract` failure. A request to download or save the original PDF is
an artifact request, not a content-reading request.

Regression tests verify wording coverage and propagation through
`ToolRegistry::to_tool_defs()`. The contract is intentionally descriptive: a
selected `web_extract` call still cannot bypass URL safety, rollout profile,
Provider capabilities, deadline/budget, or source-contract checks.

## Acceptance evidence

After commit `1c99d497c42c612deb52c8d14a7a4618e58760f0`, Owner ran a fresh
Desktop session with natural user requests that did not name internal tools.
The sanitized 2026-08-04 logs show:

| Case | First content-reading action | Managed result |
| --- | --- | --- |
| Public PDF | `web_extract` | `remote_attempted=true`, `remote_success_count=1` |
| JavaScript Shell | `web_extract` | `remote_attempted=true`, `remote_success_count=1` |
| Ordinary HTML | `web_extract` | Local success, `remote_attempted=false` |

Each case used one tool call, with no Browser, Bash, Python, or
`exec_command` detour in the corresponding window. Desktop closed normally.
The logs contain no URL, page body, Query, Cookie, Header, or user-question
content.

## Maintenance and pitfalls

1. Treat this as a model-routing acceptance, not a formal Admission result.
   Browser/script success after an extract failure is not evidence of Remote
   fetch success.
2. When adding a document category, update all four model-facing surfaces and
   the propagation test together; do not expose provider implementation detail.
3. Do not tell end users to invoke an internal tool by name. Test with natural
   reading/summarization requests and inspect the first actual tool use.
4. Keep `web_search` for discovery only. A known public direct URL should avoid
   an unnecessary search call.
5. Keep the evidence boundary precise: PDF/JS need `remote_attempted=true` and
   a positive remote success count; ordinary HTML is correct with zero Remote.
6. Before PR creation, rebase this branch to the live `origin/main` and rerun
   affected tests/Canary. Historical SHA and ahead/behind values must never be
   presented as live branch status.

Related architecture references:

- [Managed Fetch policy](../../architecture/web/managed-web-fetch-policy.md)
- [Provider maintenance guide](../../architecture/web/managed-web-fetch-provider-maintenance.md)
- [Evaluation maintenance contract](../../architecture/web/managed-fetch-evaluation.md)

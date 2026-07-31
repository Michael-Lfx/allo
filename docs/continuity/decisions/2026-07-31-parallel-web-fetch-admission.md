# Parallel web_fetch Admission Decision

Date: 2026-07-31

Status: pass; continue with the managed local-first implementation phases.
Desktop `managed_extract` remains disabled until the full acceptance gate
passes.

Full evolution record and lessons:
[managed-web-search-fetch-evolution.md](../../architecture/web/managed-web-search-fetch-evolution.md).

## Probe Scope

The admission probe lives in
`crates/agent/flowy-web/examples/parallel_web_fetch_probe.rs`. It is not
registered as an Agent tool and does not run during normal `cargo test`.
Probe responses are printed only when `--raw` or
`ALLO_WEB_FETCH_PROBE_RAW=1` is set; no raw response text is committed here.

Tested fixture categories: static HTML, PDF, JavaScript-heavy page, Chinese
article, legal short page, long page, HTTP 404, HTTP 403, two successes, one
success plus one failure, two successes plus one failure, duplicate URLs, and
a redirect.

## Observed Contract

- Endpoint: `https://search.parallel.ai/mcp`
- Protocol: MCP `2025-11-25`
- Session mode: stateful; the server returns `Mcp-Session-Id`
- Tool name: `web_fetch`
- Catalog size on this probe run: 2 tools
- Required input: `urls: string[]` (up to 20 per schema)
- Optional inputs: `full_content`, `objective`, `search_queries`,
  `session_id`, `model_name`
- Minimal call sent by the probe: `{"urls": [...], "full_content": false}`
- Success response: `structuredContent.results[]` with `url`, `title`,
  `publish_date`, `excerpts[]`, and nullable `full_content`
- Partial failure: `structuredContent.errors[]` with `url`, `error_type`,
  `http_status_code`, and nullable `content`
- Text content is a JSON copy of the structured result and can be used as a
  deterministic fallback
- Duplicate requested URLs returned one result, so production must fan the
  result out to every original index
- Redirect probe returned the requested URL in `result.url`, which supports
  mapping by requested URL rather than trusting an unverified final URL

PDF and JavaScript-heavy fixtures returned usable Markdown, so remote fetch
has a real capability gain over the local provider for those cases.

## Measured Notes

These are single-machine observations from this network, not P50/P95
acceptance numbers:

- `initialize`: 274-910 ms
- `tools/list`: 278-333 ms
- Static HTML calls: roughly 300-400 ms
- PDF calls: roughly 400-750 ms
- JavaScript-heavy calls: roughly 300-4400 ms
- Chinese article: roughly 2800 ms and about 1.09 MiB wire response
- Long RFC text: roughly 1200 ms and about 1.02 MiB wire response
- HTTP 403 took about 15 s once; HTTP 404 took about 5 s once
- Repeated probing produced occasional `error sending request` failures, so
  the anonymous endpoint is not assumed to be stable under sustained use

No 429 was observed, but the intermittent transport failures reinforce the
need for a global fetch semaphore and bounded cooldown before enabling this in
production.

## Blocking Constraint

`nomi-mcp::remote_peer` currently caps bodies at 1 MiB. A Chinese article
probe returned about 1.09 MiB, so the shared peer cannot be used for fetch
without raising its body limit or giving the fetch path its own larger cap.
The model-facing 3,000-character page budget and 8,000-character tool budget
remain unchanged.

## Decisions

- Proceed with production implementation.
- Decode `structuredContent` first, then fall back to the text JSON copy.
- Map results and errors by requested URL; do not invent recursive URL
  guessing.
- Deduplicate allowed remote URLs for a single batch and fan results back to
  original indexes.
- Omit `objective`, `search_queries`, `session_id`, and `model_name` in the
  first version so remote fetch sends only admitted URLs.
- Keep the model-visible tools as `web_search` and `web_extract`.
- Keep `managed_extract=false` on Desktop until the real acceptance gate and
  performance measurements are complete.

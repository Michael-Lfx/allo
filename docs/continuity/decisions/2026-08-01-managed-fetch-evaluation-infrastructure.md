# Managed Fetch Evaluation Infrastructure

Date: 2026-08-01

Status: IMPLEMENTED; public preflight stopped by safety gate

## Scope

The `feat/fetch-optimization` branch now contains an opt-in,
production-backed evaluation path for Local extraction, Parallel MCP fetch,
Local/MCP comparison, and the managed end-to-end coordinator. It does not
enable Desktop `managed_extract` and does not change the model-visible tool
surface.

## Safety and evidence rules

- `managed_search=false + managed_extract=true` remains Local-only and now
  emits a structured warning identifying the unsupported combination.
- The evaluation feature is compiled only with `fetch-eval`.
- Evaluation results contain case IDs and sanitized metrics only; URLs, query
  strings, fragments, page bodies, credentials, cookies, and raw MCP payloads
  are excluded.
- Local fault injection is available only in tests and never counts as MCP
  Provider gain.
- The first rate-limit result stops a batch, persists a bounded Retry-After and
  cooldown, and never sleeps or retries. A file-locked quota ledger decrements
  only immediately before an actual remote tool call; it enforces the
  configured per-run and daily caps.
- Summary statistics ignore Local and MCP diagnostics. Admission uses one cold
  Compare plus two warm E2E attempts, requires a real Local failure, applies
  two-of-three success and warm-E2E gates, and rejects missing safety evidence.
- A deterministic offline Demo covers all four modes, cold/warm/search-warmed
  lifecycle, fault-policy boundaries, sensitive URL zero-egress, source
  mismatch, and first-429 stop behavior.

## Current verification

```text
cargo test -p flowy-web --features fetch-eval --all-targets
166 flowy-web feature tests passed (including 7 evaluation-runner tests)
4 existing Probe target tests passed
```

The offline Demo and Local-only public classification run verified sanitized
JSONL/status output without consuming MCP quota. The 2026-08-01 developer
Probe showed a reachable stateful MCP `web_fetch` and structured PDF/JS response
shapes; that is only a Provider-level signal, not production fallback,
content-quality, or stability acceptance. Browser success is not counted as
MCP Fetch evidence.

## Public preflight evidence

The single authorized preflight used the clean commit recorded in the status
file, corpus version `2026-08-01-public-preflight-v3`, cold Compare, three
seconds pacing, and a ten-call ceiling. It wrote nine attempts and made nine
actual `web_fetch` calls before stopping. Seven attempts were effective
incremental successes; two were failures. The ninth attempt reported one
unmatched Remote item/source mismatch, so the Runner set
`stop_reason=safety_violation` and made no tenth call or retry. No 429 was
observed. The sanitized Summary treats the observed safety violation as
`reject`; this is a preflight safety result, not a category admission decision.

The evidence is intentionally insufficient for Pilot or enablement. The
preflight does not authorize a production fallback, and no Desktop default or
managed-extract capability was changed.

## Pilot entry point

Use the `preflight` tag in `crates/agent/flowy-web/evaluation/corpus.public.json`
for the five-PDF/five-JavaScript public preflight. Supply the real private case through
`ALLO_FETCH_CASE_REAL_PDF` in an ignored local manifest.

The final decision record must remain one of:

```text
candidate_for_enablement
retain_experimental
reject
inconclusive_due_to_quota
insufficient_evidence
```

No result from this infrastructure alone authorizes production enablement.

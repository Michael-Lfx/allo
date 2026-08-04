# Managed Fetch Evaluation Infrastructure

> Historical evidence: commit SHA fields in this record refer to the pre-rewrite
> checkpoint. The rebase-era merge decision is recorded separately.

Date: 2026-08-01

Status: IMPLEMENTED; v5 preflight complete, retain experimental

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
  two-of-three success and warm-E2E gates, and fails closed on missing,
  incomplete, dirty, legacy, or provenance-inconsistent Safety evidence.
- A deterministic offline Demo covers all four modes, cold/warm/search-warmed
  lifecycle, fault-policy boundaries, sensitive URL zero-egress, source
  mismatch, and first-429 stop behavior.

## Current verification

```text
cargo test -p flowy-web                         150 passed
cargo test -p flowy-web --features fetch-eval --all-targets
                                               177 passed (plus 4 probe-shape tests)
cargo test -p nomifun-app --features managed-search managed_web
                                               3 passed
cargo check -p flowy-web --no-default-features  passed
cargo check -p flowy-web --features fetch-eval  passed
cargo check --workspace                         passed
cargo fmt --all -- --check                      passed
git diff --check                                passed
```

The repository Bun gate reached typecheck, i18n, theme, icon, process-runtime,
and browser-boundary checks, then stopped at the pre-existing agent-vocabulary
baseline: eight retired active references. Those unrelated references were
not modified.

The status schema is version 3 and the Safety schema is version 2. A summary is
version 2 and carries the input result schema, corpus, scoring/profile, run and
Git provenance, `evidence_complete`, and a fail-closed `decision_reason`.
Legacy schema-2 results/status-2/Safety-1 files remain readable only as
diagnostic evidence and can never produce a candidate decision. The production
CLI reads the real HEAD and has no Git SHA or dirty-worktree override; internal
tests use a metadata seam.

The offline Demo and Local-only public classification run verified sanitized
JSONL/status output without consuming MCP quota. The 2026-08-01 developer
Probe showed a reachable stateful MCP `web_fetch` and structured PDF/JS response
shapes; that is only a Provider-level signal, not production fallback,
content-quality, or stability acceptance. Browser success is not counted as
MCP Fetch evidence.

## Historical v3 public preflight evidence

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

## v5 public preflight evidence

After the Local-only qualification, the committed corpus was upgraded to
2026-08-01-public-preflight-v5 with exactly five PDF and five JavaScript
cases. The clean commit was 78e0f944e2c0298a7a7966ff800b72d87557c0cb.
The single preflight run used cold Compare, three-second pacing, a ten-call
ceiling, and the production Parallel MCP client through the call Gate. Its
sanitized evidence is kept under
fetch-evaluation-raw/v5-019fbda8-cb05-74c2-b654-b011736a3112/.

```text
run_id                 019fbda8-cb05-74c2-b654-b011736a3112
git_sha                78e0f944e2c0298a7a7966ff800b72d87557c0cb
planned/completed      10 / 10
actual fetch/search    10 / 0
stop_reason            completed
retry_after/cooldown   null / null
safety                 source=0 dropped=0 sensitive=0 retries=0 late=0
```

Per-case sanitized result:

| case id | category | Local failure | Remote success | quality | chars | markers | elapsed |
| --- | --- | --- | --- | --- | ---: | ---: | ---: |
| public-pdf-w3c-dummy | PDF | Pdf | yes | Q3 | 21 | 1/1 | 1116 ms |
| public-pdf-nist-fips-180-4 | PDF | Pdf | yes | Q3 | 57298 | 2/2 | 7574 ms |
| public-pdf-rfc-9110 | PDF | Pdf | yes | Q3 | 469639 | 2/2 | 2035 ms |
| public-pdf-irs-w9-instructions | PDF | Pdf | yes | Q2 | 43673 | 1/2 | 3055 ms |
| public-pdf-nist-sp-800-53r5 | PDF | Pdf | no | Q0 | 0 | 0/2 | 25950 ms |
| public-js-swagger-petstore | JS | JavascriptShell | yes | Q3 | 2002 | 2/2 | 2241 ms |
| public-js-eslint-code-explorer | JS | JavascriptShell | yes | Q3 | 1552 | 2/2 | 16150 ms |
| public-js-swagger-editor | JS | JavascriptShell | yes | Q2 | 8469 | 1/2 | 2706 ms |
| public-js-vue-playground | JS | JavascriptShell | yes | Q3 | 634 | 2/2 | 1155 ms |
| public-js-tailwind-playground | JS | JavascriptShell | yes | Q2 | 2718 | 1/2 | 6771 ms |

The NIST SP 800-53 response was a bounded malformedresponse Provider failure;
it was not retried and did not trigger a safety violation. The Safety Report
is complete and all five safety-zero counters are zero. Its
cancellation_events_observed=0 means this run observed no cancellation; it is
not a cancellation certification. Cancellation correctness remains covered by
the Wiremock cancellation tests.

The preflight is intentionally not an Admission triple and cannot produce
candidate_for_enablement. It is useful evidence for choosing the next Pilot
corpus, but the NIST response quality and the long-tail JS warm latency require
additional independently sampled URLs before any Pilot approval.

Decision: insufficient_evidence
Reason: preflight profile, ten independent URLs only, and no Admission triple;
retain the experimental path and seek separate Pilot approval.

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

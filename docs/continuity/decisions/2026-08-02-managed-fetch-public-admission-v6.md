# Managed Fetch public Admission v6 decision

> Historical evidence: commit SHA fields in this record refer to the pre-rewrite
> checkpoint. The rebase-era merge decision is recorded separately.

Date: 2026-08-02
Branch: `feat/fetch-optimization`
Evidence SHA: `e90045a04fd82386e02178c5cc8a89606e1a341d`
Implementation/ADR follow-up SHA: `b5db9614479aa779c8781cae2a550ed79bc8a292`

## Scope and safety boundary

This record covers the v6 public corpus qualification and the 30-attempt Warm
Pilot. Desktop remains `managed_extract=false`. No private URL, Browser URL,
Cookie/Referer, Browser-to-MCP loop, GitHub Actions, Push, or production
fallback switch was used. The formal Admission Campaign was not started.

The feature-gated runner now has a real `ManagedMcpCallGate` at the production
`tools/call` seam, process-safe quota accounting, atomic status/Safety writes,
strict recovery limits, a resumable `AdmissionCampaign`, and a fail-closed
candidate-pool check. The production command refuses an Admission campaign
when either category has fewer than 15 qualified cases.

## Corpus qualification

The frozen corpus is `2026-08-02-public-admission-v6`.

- PDF: 16 new official candidates were classified twice with Local-only runs;
  all 16 were stable `Pdf` failures and Remote Eligible. Four RFC candidates
  returned HTTP 404 twice and were disabled. Four previously qualified PDFs
  plus NIST FIPS 197 form the five-case `pilot-v6` set. SP 800-53 Rev. 5 and
  ECMA-262 remain diagnostic-only. The `admission-v6` tag contains exactly 20
  PDF cases.
- JavaScript: 15 fixed-order candidates were classified twice with Local-only
  runs. Five existing shell cases remain qualified. RunKit was the only new
  Remote Eligible failure, but it was not promoted because the existing
  response-shape evidence did not provide safe requested-URL identity. The
  remaining candidates were Local successes, challenge/403/404 results, or
  otherwise not Remote Eligible. The `admission-v6` tag therefore contains 5
  JS cases.

Local qualification made zero MCP calls. Raw Local evidence is retained under
the ignored `fetch-evaluation-raw/` directory and contains no URL or body in
the serialized attempt records.

## Warm Pilot evidence

The Pilot ran two independent Admission triples per category set: one cold
Compare and two warm E2E attempts per case. It used a clean worktree, a shared
file-locked quota ledger, three-second pacing, and no retries.

| category | cases | logical attempts | actual Fetch calls | Search/recovery calls | eligible | incremental success | Q2+ | Warm P50/P95 | Wilson interval |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- |
| public_pdf_text | 5 | 15 | 15 | 0 / 0 | 5 | 5 (100%) | 5 (100%) | 1322 / 2382 ms | 0.566–1.000 |
| javascript_shell | 5 | 15 | 15 | 0 / 0 | 5 | 5 (100%) | 5 (100%) | 503 / 1294 ms | 0.566–1.000 |
| total | 10 | 30 | 30 | 0 / 0 | 10 | 10 (100%) | 10 (100%) | — | — |

All 30 attempts completed normally. Every Pilot case had a complete exact
triple, 3/3 Remote attempts, at least one warm success, and zero source
mismatch or dropped items. Per-case sanitized evidence:

| case ID | category | Local failure | Remote attempts | Q grades (cold,warm,warm) | warm elapsed ms | marker hits (cold,warm,warm) | stop |
| --- | --- | --- | --- | --- | --- | --- | --- |
| public-pdf-w3c-dummy | PDF | Pdf | 3/3 success | Q3,Q3,Q3 | 353,377 | 1,1,1 | completed |
| public-pdf-nist-fips-180-4 | PDF | Pdf | 3/3 success | Q3,Q3,Q3 | 1692,1801 | 2,2,2 | completed |
| public-pdf-rfc-9110 | PDF | Pdf | 3/3 success | Q3,Q3,Q3 | 1322,1319 | 2,2,2 | completed |
| public-pdf-irs-w9-instructions | PDF | Pdf | 3/3 success | Q2,Q2,Q2 | 1417,1355 | 1,1,1 | completed |
| public-pdf-nist-fips-197 | PDF | Pdf | 3/3 success | Q3,Q3,Q3 | 1160,2382 | 2,2,2 | completed |
| public-js-eslint-code-explorer | JS | JavascriptShell | 3/3 success | Q3,Q3,Q3 | 502,503 | 2,2,2 | completed |
| public-js-swagger-editor | JS | JavascriptShell | 3/3 success | Q2,Q2,Q2 | 1294,1275 | 1,1,1 | completed |
| public-js-swagger-petstore | JS | JavascriptShell | 3/3 success | Q3,Q3,Q3 | 1237,1221 | 2,2,2 | completed |
| public-js-tailwind-playground | JS | JavascriptShell | 3/3 success | Q2,Q2,Q2 | 389,419 | 1,1,1 | completed |
| public-js-vue-playground | JS | JavascriptShell | 3/3 success | Q3,Q3,Q3 | 504,380 | 2,2,2 | completed |

Evidence files are:

- `fetch-evaluation-raw/v6-pilot/pdf-pilot.jsonl`
- `fetch-evaluation-raw/v6-pilot/js-pilot.jsonl`
- `fetch-evaluation-raw/v6-pilot/pilot-summary.json`
- `fetch-evaluation-raw/v6-pilot/quota.json` (`used_calls=30`, UTC 2026-08-02)

## Safety Gate Report

The two Pilot status/Safety pairs agree on provenance, counters, and terminal
state. The five zero gates are:

| gate | count |
| --- | ---: |
| sensitive URL egress | 0 |
| source mismatch | 0 |
| dropped Remote item | 0 |
| retry-limit violation | 0 |
| cancellation late result | 0 |

Fetch calls used exactly `{"urls": [...], "full_content": false}`. There were
no Search calls, recovery calls, 429s, quota pauses, or safety stops.

## Decision

`insufficient_evidence / candidate_pool_shortage`.

The PDF and JS Pilot signals are strong but each has only five independent
eligible cases. More importantly, JS has only five qualified cases, below the
15-case formal Admission minimum. The new Campaign command is intentionally
blocked for `admission-v6`; it cannot turn this Pilot into an enablement
recommendation. The v6 public Admission is therefore neither `candidate_for_enablement`
nor a production recommendation.

## Next human boundary

No Desktop manual test is requested for this result. The next useful approval
is to add or validate at least ten more stable, publicly mappable JS shell
cases (or explicitly accept a JS-specific evidence-shortage stop), then run
the resumable campaign on a clean SHA. Private PDF, Browser final URL,
Desktop experiment toggles, and a production `RemoteFallbackPolicy` remain
separate, manually approved work.

# Managed Fetch post-fix public preflight decision

Date: 2026-08-01

Status: insufficient_evidence; candidate pool stopped the new preflight

## Scope

The evaluation hardening is implemented on `feat/fetch-optimization`. The
Parallel MCP quota/safety seam now counts actual `tools/call` operations,
including recovery calls, and the Runner writes a provenance-bound sanitized
Safety Report. Desktop `managed_extract` remains disabled.

## Bounded diagnostics

The authorized diagnostic budget was two public `web_fetch` calls, in the
order ECMA-262 then RunKit. The explicit Probe emitted only response shape,
source identity and content-length metadata.

- ECMA-262 exceeded the explicit 2 MiB Probe response limit. No decoder or
  output-size relaxation was admitted.
- RunKit returned HTTP 200 with zero successful result items and one structured
  error item. No source identity was available, so no positional, single-item,
  same-origin or Browser-derived mapping was attempted.
- No 429 was observed. No additional diagnostic call was made.

## Local-only candidate classification

Six ordered JavaScript candidates were classified without MCP calls. StackBlitz,
PlayCode, Observable and W3Schools were Local successes and therefore not
Remote Eligible. JSFiddle was an access challenge and JSBin returned HTTP 403;
both remain forbidden by policy. The fixed candidate order produced no legal
replacement for the removed RunKit preflight case.

The new NIST SP 800-53 PDF candidate was Local-classified as `Pdf`, and the
production policy classified that failure as Remote Eligible. It remains a
candidate and is not counted as MCP evidence.

## Evidence and decision

Corpus version: `2026-08-01-public-preflight-v4`.

The previous v3 preflight remains a rejected safety experiment because of a
source mismatch. The v4 post-fix preflight was not started: the corpus had
only four eligible PDF and four eligible JavaScript preflight cases after the
safe exclusions. Running a partial batch would not satisfy the planned 5+5
entry condition.

Decision: `insufficient_evidence` with reason `candidate_pool_shortage`.
This is not a production enablement recommendation. A future run needs one
additional independently classified, non-challenge JavaScript shell, followed
by a clean 5-PDF + 5-JavaScript cold preflight and then the separately approved
Admission Pilot.

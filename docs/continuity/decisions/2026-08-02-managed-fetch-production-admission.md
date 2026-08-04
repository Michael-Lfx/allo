# Managed Fetch Production Admission

> Historical evidence: commit SHA fields in this record refer to the pre-rewrite
> checkpoint. The rebase-era merge decision is recorded separately.

Date: 2026-08-02
Branch: `feat/fetch-optimization`

## Decision

`insufficient_evidence` for the evidence-backed Desktop profile. The bounded
canary proves the production safety chain and normal PDF/JavaScript fallback,
but it is not the required 15+ independent-case Admission Campaign. PDF and
JavaScript remain the approved experimental categories already wired to the
Desktop profile; Empty Content remains in the same evidence-backed policy but
has no independent Admission evidence and must not be described as a formal
enablement recommendation. Unsupported Document, DNS/TLS/Network, Timeout,
Browser-to-MCP loops, and user-configurable provider selection remain deferred.

## Evidence boundary

- Desktop acceptance was completed with the existing `web_extract` surface:
  HTML stayed Local-only; the accepted PDF remote call completed in 528 ms and
  the accepted JavaScript Shell remote call completed in 928 ms; Desktop shut
  down normally.
- The prior production-backed canary recorded 9 logical attempts and 6 Fetch
  calls: PDF and JavaScript succeeded, the Static HTML control made zero remote
  calls, and the five Safety counters were zero.
- This change adds a mandatory production `ParallelMcpCallPolicy`, a separate
  evaluation/quota Control, explicit profile/capability intersection, exact
  source-contract rejection, and exactly-once provider lifecycle shutdown.

## Post-enable Canary

The clean post-enable commit `9e2c9b016ea1ff31c55b8e73435ae57c2cd6d41d`
completed one bounded Admission-shaped canary run (`run_id`
`019fc192-56b3-74b1-bb37-16370d503dac`) with 9 logical attempts and 6 actual
Fetch calls. The PDF and JavaScript Shell each completed one cold Compare plus
two warm E2E attempts; the static HTML control completed three Local-only
attempts and made zero Remote calls.

Sanitized outcomes:

- PDF: 3/3 effective Remote successes, Q3 on each attempt, warm P50 366 ms,
  warm P95 389 ms.
- JavaScript Shell: 3/3 effective Remote successes, Q3 on each attempt, warm
  P50 462 ms, warm P95 496 ms.
- Static HTML control: 3/3 Local successes, 0/3 Remote attempts.

Safety evidence was complete and all zero: Fetch calls 6, Search calls 0,
recovery calls 0, source mismatches 0, dropped items 0, sensitive egress 0,
retry-limit violations 0, and cancellation-late results 0. The canary is not
15+ URL admission evidence; its per-category decision remains
`insufficient_evidence` and it must not be used to claim full campaign
enablement.

## Safety and rollback

Every Parallel call is authorized before network I/O. Fetch arguments are
limited to `urls` and `full_content=false`; prepared public URLs must match the
outbound value; recovery is capped at three calls and a fourth call is blocked
before the network. Source mismatch or dropped items fail the complete batch
closed. `NOMIFUN_MANAGED_FETCH_MODE=off` restores Local-only extraction after a
restart; invalid or blank values also fail closed.

## Final Desktop acceptance

The owner-operated Desktop session was completed after the default-mode
change, with `NOMIFUN_MANAGED_FETCH_MODE` unset. The owner ran the HTML, public
PDF, and JavaScript Shell `web_extract` cases and then closed Desktop normally.
The development launcher did not persist this session's stdout to the
repository or the historical file logger, so this record deliberately does
not invent per-request counters. The acceptance is backed by the existing
sanitized canary (real PDF/JS Remote success, HTML zero Remote) and the host
default-mode/unit evidence; it is not additional Admission-campaign data.

## Remaining boundary

This record is not evidence for the 15+ URL public Admission campaign. The next
expansion requires a fresh corpus and a separate approval for broader
categories. No Push, PR, GitHub Actions, Browser-to-MCP loop, database setting,
or UI switch is introduced by this change.

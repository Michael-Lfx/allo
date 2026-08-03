# Managed Fetch merge-readiness decision

Date: 2026-08-03
Branch: `feat/fetch-optimization`
Code/Canary HEAD: `9348bbe63d556cc415cdf20ef69a094c99284b7b`
Documentation was reviewed again after the follow-up wording fixes.

## Scope

This record covers the security closure and bounded canary needed before a
local merge request may be opened. No push, merge, PR, UI/database/Web Host
switch, Browser-to-MCP loop, GitHub Actions workflow, private URL, or category
expansion was performed.

## Security closure

Local SSRF and Remote egress share host/IP safety primitives. Remote egress
additionally rejects credentials, decoded or camelCase sensitive query/fragment
names, and presigned-key variants while allowing ordinary query parameters.
Authorization occurs before quota reserve and before transport. Remote
source-contract errors remain batch fail-closed.

Quota evidence is atomic and fails closed on an empty ledger. Admission runs
hold an ignored per-campaign lock for their lifetime, and cumulative cap
exhaustion cannot wake the same campaign indefinitely. The final exact-cap
completed state is preserved and summarized.

## Code review

- Round 1 security-specialized review: P0=0, P1=0. Covered sensitive key
  variants, encoded fragments, special domains, reserved ranges, and policy
  ordering.
- Round 2 full branch Standards review: P0=0, P1=0, P2=6 maintenance items
  (runner decomposition, stringly campaign state, duplicate MIME logic,
  duplicate control seams, cancellation cleanup shape, and missing concurrency
  tests); none is P0/P1. The review passed after fixing evidence-path, shutdown
  cancellation, quota atomicity, campaign locking, and exact-cap state
  ordering.
- Round 2 full branch Spec review: P0=0, P1=0, P2=0. Evidence-backed mode,
  deferred/forbidden zero-egress, source mapping, rollback, and scope limits
  matched the plan.
- Round 3 frozen-candidate Standards review: P0=0, P1=0, P2=6 existing
  non-blocking maintenance debts; Spec review: P0=0, P1=0, P2=0. The frozen
  range was `origin/main...HEAD` with `origin/main=46fc4af1` unchanged.

## Final gates

- `cargo test -p flowy-web`: 167/167 passed.
- `cargo test -p flowy-web --features fetch-eval --all-targets`: 209 library
  tests plus 4 example integration tests passed.
- `cargo test -p nomifun-app --features managed-search --lib managed_web`: 3/3
  passed. The focused `nomifun-ai-agent` managed-web handle test was 4/4.
- `cargo check --workspace`, flowy-web default/feature checks, `cargo fmt`,
  and `git diff --check`: passed.
- The full `nomifun-ai-agent` test binary compiled but its 915-test run was
  stopped by the 10-minute tool limit at an existing long-running
  `runtime_registry` test; this is reported as an environment/baseline limit,
  not as a feature failure.
- `bun run check` and each individual Bun check were blocked before script
  execution by the environment's `Operation not permitted` error; no UI or
  unrelated baseline file was changed.
- `.github/workflows` contains zero YAML files. The pre-ADR gate snapshot was
  ahead 49 and behind 0; the current documentation HEAD is ahead 50 and
  behind 0 relative to `origin/main`.

## Bounded canary

The new clean-HEAD run used corpus `2026-08-02-public-admission-v6` and six
Remote Fetch calls total:

| Case class | Attempts | Remote calls | Effective success | Quality | Warm elapsed |
| --- | ---: | ---: | ---: | --- | --- |
| public PDF | 3 | 3 | 3/3 | Q3 | 1.681–1.817 s |
| JavaScript Shell | 3 | 3 | 3/3 | Q3 | 0.669–2.460 s |
| static HTML control | 1 Local | 0 | Local success | Q4 | 0.678 s |

All three status/safety reports were complete and provenance-clean. Safety
counts were zero for source mismatch, dropped items, sensitive egress,
retry-limit violations, and cancellation late results. No 429 or quota stop
occurred. The first attempted run was rejected before external calls because
its worktree was dirty; its evidence was preserved separately and is not part
of the passing canary.

## Decision and merge boundary

Decision: `retain_experimental` for product rollout, and `merge-ready` for a
local owner-reviewed integration into `main`. The canary demonstrates the
repaired safety and normal PDF/JS fallback, but it is not the 15+ independent-
case Admission Campaign.

Rollback remains:

```text
NOMIFUN_MANAGED_FETCH_MODE=off
```

The next step is a local-only merge-readiness review against the latest
`origin/main`. Formal 15+ URL Admission, private PDFs, unsupported/network
categories, and any further default-policy expansion remain separate work.

# Managed Fetch cold-start readiness decision

Date: 2026-08-04
Branch: `feat/fetch-optimization`
Code SHA: `f70a81416586f0abb923836e251b3550e41970d7`
Base: `origin/main=81b199fbc8fa`
Decision: `cold_start_canary_passed_pending_desktop_acceptance`

## Finding

The 8/3 PDF miss was a Coordinator budget decision, not a failed MCP request.
`ColdTransport` and `WarmTransportToolUnknown` were both charged an 11-second
start gate inside a 12-second `web_extract` deadline. Normal Local PDF/JS
classification consumed roughly 1–2 seconds, so the remote stage was skipped
before `tools/call` (`remote_attempted=false`). Browser, download, and Python
actions were Agent-level recovery after `web_extract` returned an error; they
were not Managed Fetch fallback evidence.

## Implemented closure

- `RemoteAttemptPolicy` now requires 8 seconds for ColdTransport, 6 seconds for
  WarmTransportToolUnknown, and 4 seconds for Ready. The original absolute
  deadline is still passed to the Provider; no background prewarm was added.
- Cold and ToolUnknown budget boundaries are table-tested, including delayed
  Local PDF and JavaScript failures reaching the remote stage.
- Endpoint Health uses epoch-guarded attempt and Session recovery tokens. A late
  success or stale recovery cannot clear a newer Unauthorized/cooldown state.
  Discovery checks shared endpoint health before issuing another request.
- `application/*+json` and `application/*+xml` remain Local structured content.
- Existing Local-first, EvidenceBacked category intersection, exact URL mapping,
  Policy → Control → tools/call order, three-call recovery cap, cancellation,
  and zero-egress safety rules are unchanged.

## Verification

Focused and repository gates on the final code:

- `cargo test -p flowy-web`: 177 passed;
- `cargo test -p flowy-web --features fetch-eval --all-targets`: 230 library
  tests and 4 example tests passed;
- `cargo test -p nomifun-ai-agent --features managed-search --test managed_web_handle`:
  4 passed;
- `cargo test -p nomifun-app --features managed-search --lib managed_web`: 3 passed;
- both feature/no-feature `cargo check` modes and `cargo check --workspace` passed;
- `cargo fmt --all -- --check`, `git diff --check`, and `bun run check` passed;
- `.github/workflows` contains zero YAML workflow files.

The missing `sccache.exe` wrapper was cleared only in child Cargo processes. The
restricted `bun run check` failure was an environment `Operation not permitted`
result; the approved elevated run passed. Existing unrelated Rust warnings remain
separate from this feature’s result.

## Final bounded Canary

Evidence is in ignored `fetch-evaluation-raw/post-budget-fix-f70a8141/` and is
fully sanitized. It uses corpus `2026-08-02-public-admission-v6`, profile
`preflight`, scoring `managed-fetch-2-of-3-e2e-v1`, and the final Git SHA above.

| Case | Attempts | Remote calls | Result | Timing |
| --- | ---: | ---: | --- | --- |
| `public-pdf-w3c-dummy` | 1 cold + 2 warm E2E | 3 Fetch | 3/3 Q3 | cold 1.943s; warm P95 0.820s |
| `public-js-eslint-code-explorer` | 1 cold + 2 warm E2E | 3 Fetch | 3/3 Q3 | cold 2.170s; warm P95 1.105s |
| `public-static-example-domain` | 1 Local control | 0 | Local Q4 | no Remote |

All five Safety reports are `complete=true, all_zero=true`: source mismatch,
dropped items, sensitive egress, retry-limit violations, and cancellation-late
results are all zero. The quota ledger records exactly six Fetch calls, zero
Search calls, zero recovery calls, and no cooldown. The combined Summary is
`evidence_complete=true` but `decision_reason=preflight_never_candidate`; this
is not a 15+ URL Admission result.

## Review record

1. Budget/readiness review: no P0/P1. Confirmed no prewarm, no deadline
   extension, HTML Local success has zero Remote, and cold E2E reaches MCP.
2. Safety/Lifecycle review: one P1 was found in stale Session recovery clearing
   a newer Unauthorized epoch. It was fixed with recovery tokens and a health
   check before discovery; the focused and full suites passed again.
3. Frozen rebase/spec review: performed against the final feature diff and
   latest `origin/main`; no P0/P1 remains. Final status is waiting only for
   the owner’s fresh Desktop cold-session acceptance.

## Remaining boundary

Before calling the branch merge-ready, run one new Desktop process without
`NOMIFUN_MANAGED_FETCH_MODE` set: first a PDF, then a JavaScript Shell, then
ordinary HTML through `web_extract`. The expected evidence is cold PDF/JS
`remote_attempted=true` with Remote success and HTML `remote_attempted=false`.
If that check passes, the owner may push the feature branch and open the PR.
`NOMIFUN_MANAGED_FETCH_MODE=off` remains the immediate Local-only rollback.

Formal 15+ URL Admission, private PDFs, Browser→MCP, and expansion beyond
PDF/JavaScript/Empty remain separate future work.

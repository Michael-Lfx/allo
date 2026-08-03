# Managed Fetch current state and Admission case-plan review

> Historical evidence: commit SHA fields in this record refer to the pre-rewrite
> checkpoint. The rebase-era merge decision is recorded separately.

Date: 2026-08-02
Branch: `feat/fetch-optimization`
Snapshot HEAD: `3a8e4ae020d5d06e1795a7caf598ff1301145003`
Worktree at review start: clean

## Purpose

This record freezes the current Managed Fetch implementation/evidence state and
reviews the externally supplied `managed-web-fetch-Admission测试用例方案.md` as a
possible premise for the next execution phase. It is a planning decision, not
new production acceptance evidence and not authorization to enable Managed
Extract on Desktop.

## Current verified state

- Desktop remains `managed_extract=false`; production Desktop extraction is
  still Local-only by default.
- The feature-gated Rust `fetch-eval` module owns the production-backed Local,
  MCP, Compare, E2E, quota, Safety, status, summary, and resumable Admission
  Campaign paths. A separate TypeScript evaluation stack has not been added.
- The frozen public corpus is `2026-08-02-public-admission-v6`:
  - PDF: 20 `admission-v6` cases and 5 `pilot-v6` cases.
  - JavaScript: 5 `admission-v6` cases and 5 `pilot-v6` cases.
- The v6 Warm Pilot completed 30 logical attempts and 30 real MCP Fetch
  `tools/call` operations. Search/recovery calls were zero. All five Safety
  counters were zero.
- Pilot results were 5/5 effective incremental successes for PDF and 5/5 for
  JavaScript. PDF Warm P50/P95 was 1322/2382 ms; JavaScript Warm P50/P95 was
  503/1294 ms.
- Formal Admission did not run. Its fail-closed candidate-pool check correctly
  stopped because JavaScript has only 5 qualified cases, below the minimum of
  15. The current decision remains
  `insufficient_evidence / candidate_pool_shortage`.
- The current evidence does not authorize a Desktop switch, Browser-to-MCP
  loop, or production fallback policy.

The detailed evidence and sanitized per-case results remain in
`2026-08-02-managed-fetch-public-admission-v6.md` and the ignored local
`fetch-evaluation-raw/v6-pilot/` directory.

## Review decision

The supplied plan is **conditionally accepted as a v7 candidate catalog and
test matrix**, but **is not accepted unchanged as the next formal Admission
execution premise**.

It contains useful public-source candidates, explicit Marker expectations,
negative cases, and a sensible desire to separate static HTML, PDF, and CSR
behavior. Those are suitable inputs after they are normalized into the
existing Rust evaluation module and requalified through production policy.

The plan's current `A all green -> resume Admission` rule is not valid for the
existing admission model. Most Channel A pages are expected Local successes;
they can prove Local Extract regressions, but cannot increase the denominator
for MCP incremental success. The actual formal blocker is at least ten more
stable JavaScript cases whose Local result is a production Remote-Eligible
failure and whose MCP response can be mapped exactly to the requested URL.

## Required corrections before execution

### 1. Split the three evidence questions

- **Local Extract regression gate:** static HTML, JSON, redirects, 404, MIME
  handling, and body-limit behavior.
- **Managed MCP Admission:** only production Remote-Eligible Local failures,
  real Remote attempts, exact source mapping, Marker/length quality, complete
  Admission triples, and Safety evidence.
- **Browser differential research:** shell versus rendered DOM. Browser success
  must not count as MCP Fetch success and must not be wired into production
  fallback in this phase.

Channel A may gate Local Extract quality, but it cannot gate or substitute for
Managed MCP Admission. Channel B is useful for parser/transport robustness,
but PDF is not the current candidate-pool blocker. Channel C must remain an
independent prototype until a separate product and security decision approves
Browser routing.

### 2. Reuse the existing Rust module instead of copying TypeScript fixtures

The proposed TypeScript fixture/spec files would create a second evaluation
stack beside the feature-gated Rust runner. Cases should instead be represented
in the versioned JSON corpus, while deterministic redirect, status, MIME,
payload-shape, retry, and cancellation behavior should use the existing Rust
test seams and Wiremock adapters. This preserves one deep module and keeps
production policy, quota, source mapping, and Safety behavior local to it.

### 3. Make live-network cases qualification inputs, not deterministic gates

Every public URL and Marker claim in the supplied plan is currently an
unverified proposal assertion for this repository. Before promotion, each
candidate must be checked twice through the production Local path, pass URL
policy, and have stable, human-readable Markers. Drift, challenge pages,
403/404, login requirements, Local success, or unsafe source identity must
disable the candidate without fabricating Admission evidence.

The plan also conflicts with current policy in several places:

- The public redirect example contains a Query and must not enter the committed
  production-backed corpus. Redirect/query behavior belongs in Wiremock.
- Automatic live retries conflict with the current stop-on-429/no-retry rule.
  Retry/recovery behavior belongs in deterministic integration tests.
- A 42.7 MB PDF is a body-limit/streaming test, not a safe default public MCP
  Admission case.
- Private-auth and wrong-MIME PDF cases require local fixtures or explicitly
  approved private infrastructure; credentials must never enter the corpus.
- Byte hashes are appropriate for pinned local/binary fixtures, not MCP
  `full_content=false` quality scoring.
- The plan's `W3C dummy.pdf = 403` exclusion conflicts with the current v6 case,
  which completed 3/3 MCP successes on 2026-08-02. The exact URL and execution
  path must be reconciled before treating either statement as current truth.
- The target branch is `feat/fetch-optimization`, not the plan's stale
  `feat/managed-web-fetch` reference.
- “CI repeatable” must mean repeatable local commands. GitHub Actions workflows
  remain prohibited.

### 4. Preserve fail-closed evidence rules

Any next formal campaign must continue to freeze Git SHA, corpus, schema,
scoring, and profile; require matching JSONL/status/Safety evidence; reject
dirty or incomplete Admission evidence; and use exactly one Cold Compare plus
two Warm E2E attempts per case. Browser output, Local-only success, or manually
observed page content cannot be promoted into remote success.

## Recommended next execution premise

The supplied plan can become the next planning premise only after adopting the
following wording:

1. Treat Channels A/B/C as three independent work packages, not one Admission
   gate.
2. Convert Channel A into a deterministic Local Extract regression suite plus
   a small public canary set.
3. Convert Channel B into PDF robustness tests; retain current v6 PDF Admission
   evidence and do not block on additional PDFs.
4. Keep Channel C as Browser differential research with no production routing
   and no contribution to MCP Admission.
5. Make the formal next objective the qualification of at least ten additional
   public JavaScript shell/empty-content cases with exact requested-URL mapping.
6. Only after the JavaScript pool reaches at least 15, run the existing
   resumable Admission Campaign from a clean committed SHA.

## Proposed execution order

1. Add deterministic Wiremock regressions for Channel A redirect/status/JSON
   and Channel B MIME/magic/body-limit cases.
2. Import only safe public candidates into a new diagnostic corpus version;
   do not silently replace or promote cases.
3. Run two Local-only qualifications per candidate and record the exclusion
   reason for every failure.
4. For Remote-Eligible JavaScript candidates, run bounded MCP mapping/quality
   preflight through the existing quota and Safety gate.
5. Freeze a v7 corpus only if at least 15 JavaScript cases qualify; otherwise
   record `candidate_pool_shortage` without starting formal Admission.
6. Run the existing exact-triple Admission Campaign, summarize fail-closed,
   update the ADR, and only then decide whether Desktop manual testing is worth
   requesting.

## Manual boundary

No user manual test is needed to perform the deterministic tests, public Local
qualification, or bounded MCP preflight above. User involvement is required
before using private/authenticated PDF infrastructure, accepting a Browser
final URL, implementing Browser-to-MCP routing, enabling a Desktop experiment,
or changing the production default.

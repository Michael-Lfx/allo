# Managed Fetch Evaluation Maintenance Contract

Date: 2026-08-03
Last updated: 2026-08-24（元数据维护，未重写结论；核对基准 commit `d791691c6`）

This document is the durable maintenance contract for the opt-in
`fetch-eval` evaluation capability. It describes how evidence is produced and
validated; it is not a product configuration or a second fetch policy.

## Responsibilities and non-responsibilities

Evaluation measures the production Local, Parallel MCP, Compare, and E2E
paths with sanitized, resumable evidence. It owns corpus validation, quota
accounting, attempt persistence, scoring, campaign recovery, and offline
demo adapters.

Evaluation does not choose Desktop rollout modes, expose MCP tools to the
model, bypass production URL safety, store URLs or bodies, or authorize a
category for production. Formal 15+ case Admission remains a separate
approval step.

Evaluation also does not prove model tool selection. The question "did the
agent choose `web_extract` for a known public PDF/JavaScript URL?" is a
separate Desktop acceptance lane: use a natural user request, inspect the
first actual tool use, and correlate it with sanitized managed-extract
counters. A Browser or script succeeding after extraction fails is not
evaluation or Remote-fallback success evidence.

## Stable Interface and internal Modules

The external Interface remains small:

```rust
FetchEvaluationHarness::keyless_production()
FetchEvaluationHarness::run_case(case, mode, peer_mode)
FetchEvaluationHarness::shutdown()

run(RunConfig)
summarize(...)
summarize_with_evidence(...)
AdmissionCampaign::open_or_create(...)
AdmissionCampaign::run_available()
AdmissionCampaign::summarize()
```

The implementation is split behind the runner façade:

```text
execution  -> backend lifecycle and attempt coordination
quota      -> file lock, reservation, cooldown, call accounting
evidence   -> JSONL/status/safety atomic persistence and recovery
scoring    -> eligibility, quality, latency, confidence, decision
campaign   -> batch lock, provenance, resume and idempotency
demo       -> deterministic offline adapters and scenarios
```

These are internal Modules and Seams. Production uses the real HTTP/Parallel
Adapters; tests use Fake, Wiremock, and failure-injecting Adapters through the
same internal Interface. The crate root exports only the stable harness and
runner façade.

## MCP call order and safety

Every Parallel call follows this order and cannot bypass it:

```text
ParallelMcpCallPolicy::authorize
  -> ManagedMcpCallControl::reserve
  -> RemoteMcpPeer::call_tool
  -> ManagedMcpCallControl::observe_result
```

The policy validates the exact Fetch argument shape, prepared public URLs,
retry ceiling, and Search argument contract. The Control adds quota and
evidence accounting; it is not a safety policy and cannot replace the
authorization step.

## Evidence and failure semantics

Each attempt is flushed immediately to JSONL. Status and Safety files are
written through a temporary file, `sync_all`, and atomic replacement. This
guarantees process-crash atomicity; it does not promise platform-specific
power-loss durability for the parent directory. A run
is eligible for summary only when provenance, schema, per-run result counts,
cross-sidecar counters, completion, and the five-field Safety gate agree.
Results are bound to their own `run_id` sidecars; a different run's SHA or a
missing/extra attempt cannot be masked by the first result file. Legacy or
incomplete evidence remains readable for diagnosis but cannot produce
`candidate_for_enablement`.

Normal success and error paths explicitly await provider shutdown. If a run
future is cancelled, in-flight work is dropped, no detached task may issue a
late request or result, and evidence remains incomplete and recoverable. Rust
`Drop` is not used to promise asynchronous session deletion.

## Test lanes and rollout boundaries

Pure scoring and schema tests run offline. Wiremock tests prove the real MCP
client, Search/Fetch recovery, quota ordering, cancellation, and zero-egress
rejections. The bounded six-call canary must include one production-shaped
`mode=e2e, peer_mode=cold` attempt per category; `Compare` is provider
diagnostic evidence and cannot stand in for cold production reachability.
Warm E2E attempts may explicitly complete Fetch compatibility before timing.
The canary also includes an HTML zero-remote control. It is not formal
Admission and cannot by itself widen the production category allow-list.

The emergency rollback remains:

```text
NOMIFUN_MANAGED_FETCH_MODE=off
```

## Maintenance debt register

The following items are intentionally closed by the current maintenance
refactor and should not be reintroduced:

- `MF-EVAL-001`: one giant runner mixing execution, quota, evidence, scoring,
  demo, and campaign state.
- `MF-EVAL-002`: stringly typed campaign lifecycle state.
- `MF-PROVIDER-001`: duplicated document MIME classification.
- `MF-MCP-001`: legacy call-gate and control interfaces in parallel.
- `MF-EVAL-003`: ordinary error paths skipping explicit shutdown.
- `MF-LIFE-001`: missing campaign-lock and concurrent-shutdown proofs.

Any future provider must preserve these interfaces and add contract tests at
the deep module seam before changing production policy.

## Deferred evaluation-only follow-up

`FileQuotaControl::record_rate_limit` currently stops the in-memory run even if
persisting its cooldown ledger fails. This preserves the immediate safety stop,
but a process crash can lose the durable cooldown. Before starting a multi-day
Admission campaign, make that persistence failure surface
`quota_ledger_failed` and add an injected rate-limit-ledger failure test. This
does not affect the production `web_extract` egress path or bounded canaries;
it is deliberately documented so a future campaign does not assume durable
cooldown evidence that was not written.

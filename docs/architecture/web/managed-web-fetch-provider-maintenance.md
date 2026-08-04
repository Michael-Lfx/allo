# Managed Fetch Provider Maintenance Guide

Date: 2026-08-02

This guide describes the seams that must remain stable when the managed fetch
provider changes. The Desktop host selects only `ManagedExtractMode`; it never
selects a provider, endpoint, MCP tool, or health policy.

## Ownership

```text
Desktop Host
  -> ManagedWebHandle / ManagedWebService
      -> ManagedProviderBundle
          -> ManagedSearchService
          -> ManagedExtractCoordinator
          -> ManagedProviderLifecycle (exactly once)
              -> ParallelMcpClient
                  -> ParallelMcpCallPolicy
                  -> ManagedMcpCallControl (quota/evidence only)
```

The coordinator owns Local-first routing, the rollout profile, provider
capabilities, budgets, and source-contract handling. `ParallelMcpClient` owns
the transport and endpoint/tool health. `ParallelMcpCallPolicy` is mandatory
for every Parallel `tools/call`; a quota or evaluation Control can reserve a
call but cannot authorize unsafe arguments.

The production Coordinator keeps the 12-second `web_extract` deadline and
uses a provider-injected start policy of 8 seconds for ColdTransport, 6 seconds
for WarmTransportToolUnknown, and 4 seconds for Ready. These are admission
thresholds only; the provider still receives the original absolute deadline.
Do not add startup prewarming for ordinary HTML. Endpoint health is epoch
guarded so a late success cannot clear a newer cooldown or unauthorized state.

## Adding or replacing a provider

1. Implement the crate-private `RemoteExtractProvider` contract with readiness
   and batch extraction only. Evaluation warmup belongs in the feature-gated
   evaluation control, not in the production provider trait.
2. Declare the provider's `RemoteExtractCapabilities` explicitly. Do not add a
   category to `EvidenceBacked` without a new admission record and review.
3. Add a `ManagedProviderLifecycle` entry to the bundle. Shared runtimes must
   be registered once; shutdown is bounded and idempotent.
4. Reuse the coordinator contract tests: prepared URLs, exact requested-URL
   mapping, duplicate fan-out, missing/extra/malformed/dropped item rejection,
   deadline, cancellation, and Local-error restoration.
5. Route all Parallel calls through `ParallelMcpCallPolicy`; never add a raw
   `peer.call_tool` path or an optional safety gate.
6. Keep provider-specific failures in provider health. Fetch decoder/schema or
   source-contract failures must not disable unrelated Search providers.

## Safe response contract

Remote results are assigned only by canonical requested URL. `final_url`, array
position, same-origin guesses, and a single-result fallback are not identity
signals. Any missing, extra, malformed, unmatched, or dropped item rejects the
whole remote batch and restores each original Local error.

Logs may contain counts, categories, and elapsed time only. Never add URLs,
query/fragment text, body content, markers, headers, cookies, authorization,
conversation text, or raw MCP payloads to diagnostics.

## Rollback and verification

Set `NOMIFUN_MANAGED_FETCH_MODE=off` and restart Desktop to force Local-only
extraction. Search remains available through its normal provider chain. After a
provider change, run the focused `flowy-web` tests, the managed-app tests, and
the documented six-call PDF/JavaScript/HTML canary before requesting a new
Desktop session acceptance.

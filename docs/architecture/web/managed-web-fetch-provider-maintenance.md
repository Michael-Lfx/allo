# Managed Fetch Provider Maintenance Guide

Date: 2026-08-02
Last updated: 2026-08-04

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

## Model contract companion

Changing a supported document category is not complete until the model-facing
contract remains accurate. Keep these four surfaces synchronized:

1. `WebExtractTool::description()`;
2. the `urls` JSON-schema description;
3. the generic agent context guidance; and
4. the Browser preset guidance.

The wording must stay provider-free: it may say that `web_extract` reads known
public HTML, direct PDFs, and JavaScript-shell pages, but must not expose MCP,
Parallel, endpoint names, payloads, or `final_url`. Keep the direct-URL rule:
known public URLs use `web_extract`; search discovers unknown URLs; Browser is
for interaction; scripts are for local artifacts or a genuine extract failure.
Add a propagation test from `WebExtractTool::description()` through
`ToolRegistry::to_tool_defs()` so a registry refactor cannot silently erase the
contract.

Model guidance does not weaken the production policy. Every actual Remote call
still has to satisfy the policy, capabilities, budget, and source-contract
checks described in [managed-web-fetch-policy.md](managed-web-fetch-policy.md).

The current maintenance entry points are:

```text
crates/agent/flowy-web/src/tools/web_extract.rs
crates/agent/flowy-web/src/tools/web_search.rs
crates/agent/nomi-agent/src/context.rs
crates/backend/nomifun-app/src/services.rs
```

Keep Desktop environment parsing in the Host (`services.rs`); do not add a
second mode parser to `flowy-web` while changing model guidance or a Provider.

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
Desktop session acceptance. For model-routing changes, use natural user-facing
requests (do not tell the user to name an internal tool) and verify both the
first tool-use record and the sanitized completion counters:

- public PDF and JavaScript-shell: first read action is `web_extract`, with a
  Remote attempt/success only when Local fails and policy permits it;
- ordinary HTML: `web_extract` succeeds locally with zero Remote attempt; and
- no Browser, Bash, Python, or `exec_command` detour appears unless
  `web_extract` genuinely failed or the task asked for an artifact/interaction.

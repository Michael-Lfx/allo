# Intent Resolution

**Status:** discussion-ready architecture; no execution implementation approved  
**Branch:** `feat/intent-resolution-layer`  
**Owner scope:** advisory intent understanding and bounded read-only evidence acquisition  
**Not owned:** message admission, Conversation policy, Task/Plan creation, Runtime control, execution, approval, or persistence

## Decision

Flowy should add an **Evidence-Assisted Intent Resolver**: a deep Rust module that interprets a user
utterance and may perform one bounded, scoped, read-only evidence lookup before returning an advisory
`IntentResolution`.

It must not become:

- a Front Door owner;
- a smaller Agent Runtime;
- a second Tool Registry;
- a Conversation gate;
- a Task, Goal, or Plan state machine;
- an execution or approval authority.

The first implementation proposal is a non-blocking Shadow experiment, not a production routing change.

## Why Shadow First

The current Conversation send path already owns message idempotency, turn admission, persistence,
Runtime reuse, cancellation, recovery, and AgentExecution relationships. A synchronous Intent model
inserted before that path would add latency and failure modes to its most sensitive seam.

The proposed shape is:

```text
existing send path
  -> message accepted normally
  -> canonical conversation_id + message_id available
  -> async Shadow analysis
  -> local evaluation record
```

Shadow failure must not change message delivery, user-visible response, or Runtime state.

## Module Shape

```text
Host-owned Shadow adapter
  -> IntentResolver::resolve
       deterministic prepass
       one-shot semantic interpretation
       optional SearchPastConversations request
       one refinement pass
       invariant validation
  -> advisory IntentResolution
```

Dependency direction:

```text
nomifun-intent
  depends on no nomi-* crate, Conversation crate, Runtime, or AgentExecution

host integration
  depends on nomifun-intent
  uses nomifun-ai-agent for one-shot model access
  uses a scoped Conversation adapter for historical evidence

nomifun-app
  assembles the implementation at the composition root
```

The core module exposes one public `resolve` interface. Its interpreter and evidence broker are
accepted dependencies, not internally created Runtime objects.

## First Vertical Slice

The experiment is deliberately split:

### P0-R0: one-shot Shadow without evidence

Prove:

- a real model adapter can produce the contract;
- deterministic and model outputs can be validated;
- failures degrade without affecting the send path;
- latency and token cost are measurable;
- the new schema beats or matches the existing one-shot baseline.

### P0-R1: add one historical evidence operation

Add only:

```text
SearchPastConversations
```

Prove:

- shared-history cues are detected accurately;
- historical search improves the relevant subset;
- the resolver does not search ordinary requests;
- assistant proposals are not promoted to user decisions;
- empty, irrelevant, or truncated results degrade safely.

P0-R does not include Memory, Web, capability search, Runtime status, Task/Artifact search,
persistent Intent state, user-visible Task Briefs, automatic planning, or execution.

## Current Repository Facts

- The main Conversation implementation is already a large, stateful subsystem; Intent must not be
  inserted as a synchronous precondition.
- Cross-conversation message search exists at `/api/messages/search` and
  `ConversationService::search_messages`.
- Search results currently contain a bounded preview and conversation metadata, not a proven
  surrounding-turn digest. The first history adapter therefore cannot be a shallow route wrapper.
- A standalone one-shot completion path already exists in `nomifun-ai-agent`.
- Guid / Sessions is a primary product path, but it can launch several Agent types with different
  model ownership. The first supported Agent type remains an explicit pending decision.

## Invariants

1. Current user instructions, quoted text, examples, historical messages, tool observations, and
   resolver inference retain distinct provenance.
2. Evidence is data, never authority.
3. Model-generated identifiers cannot grant access to a resource.
4. Evidence scope is fixed by the host adapter and invisible to the model.
5. The core exposes no arbitrary tool name, filesystem path, SQL, credential, Provider, or Runtime handle.
6. `IntentResolution` is advisory and may be ignored without breaking the existing path.
7. Shadow work begins only after the original message has been accepted.
8. Shadow records are local-only and must never enter cloud telemetry.
9. A stale Shadow result is marked superseded or discarded; it does not create a new cancellation Runtime.
10. No GitHub Actions workflow is introduced for this work.

## Pending Decisions

These decisions must be made before implementation:

| Decision | Recommended answer |
| --- | --- |
| First supported Agent type | Built-in Nomi sessions only |
| Intent model | Reuse the resolved Nomi Provider/Model for the sampled session |
| History scope | Same user and same verified project/workspace when available; never model-selected |
| Shadow record persistence | Local-only, explicit feature flag, bounded retention, no raw text by default |
| P0 trigger | After accepted message, never concurrently with an unaccepted draft |
| Old analysis handling | Mark/discard as superseded; do not build cancellation state in P0 |
| User-visible behavior | None until P0-R evaluation passes |

## Document Map

- [`contract.md`](contract.md) — authoritative domain and interface contract.
- [`evidence.md`](evidence.md) — the single P0 evidence operation and broker invariants.
- [`shadow-evaluation.md`](shadow-evaluation.md) — integration proposal, privacy rules, baselines, and exit criteria.
- [`research-notes/2026-07-28-design-evolution.md`](research-notes/2026-07-28-design-evolution.md) —
  non-authoritative discussion and research history.

## Approval Boundary

This document approves no code change. The next decision is whether to implement P0-R0, and under which
pending-decision answers. P0-R1 must not begin until P0-R0 produces a real latency, cost, and quality baseline.

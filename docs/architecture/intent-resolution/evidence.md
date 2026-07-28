# Intent Evidence

This document defines bounded read-only evidence acquisition for Intent Resolution.

## P0 Scope

P0-R0 uses `NoEvidenceBroker`.

P0-R1 adds exactly one domain operation:

```rust
pub enum EvidenceRequestKind {
    SearchPastConversations {
        terms: Vec<String>,
    },
}
```

Memory, Web, Task, Artifact, capability, Provider, attachment, and Runtime-status operations are
explicitly excluded from P0-R.

Current UI state, recent messages, pending interactions, and caller-validated attachments are initial
input facts rather than evidence tools.

## Broker Interface

```rust
#[async_trait]
pub trait IntentEvidenceBroker: Send + Sync {
    async fn observe(
        &self,
        request: EvidenceRequest,
    ) -> Result<ObservationBatch, EvidenceError>;
}

pub struct EvidenceRequest {
    pub request_id: EvidenceRequestId,
    pub kind: EvidenceRequestKind,
    pub reason: String,
    pub max_items: usize,
}
```

The broker instance is pre-scoped by the host. User identity, project/workspace scope, credentials, and
authority are not model-controlled request parameters.

## Search Admission

`SearchPastConversations` is allowed only when the deterministic and semantic passes identify a
shared-history cue such as:

- explicit prior work: "继续我们之前的季度汇报";
- a prior-assistant reference: "你上次建议的方案";
- a shared possessive without a visible target: "我的那个项目";
- an explicit recall request.

It is not allowed merely because historical search is available.

Query rules:

- use distinctive content nouns, project names, or proper nouns;
- omit meta words such as "之前", "讨论", and "昨天" unless they are part of a name;
- never submit the full utterance as the query;
- apply item, term-count, and character limits;
- redact or reject secrets and unnecessary private text;
- issue at most one request in P0-R1.

## A Deep Conversation Adapter

The existing `/api/messages/search` result contains a matched preview, message role, timestamp, and
Conversation metadata. That is enough to locate candidates but may be insufficient to decide whether a
claim was a user decision, an assistant proposal, or a hypothetical discussion.

The host adapter must therefore hide the required retrieval complexity behind one evidence operation:

```text
SearchPastConversations
  -> user-scoped message search
  -> candidate filtering
  -> bounded surrounding-turn retrieval when available
  -> role and timestamp preservation
  -> normalized ContextObservation
```

If surrounding turns cannot be obtained safely, the observation is marked truncated and cannot support
a strong commitment claim. The adapter must not silently treat a preview as a complete decision record.

The Intent module never calls `/api/messages/search` directly and never imports Conversation DTOs.

## Observation Contract

```rust
pub struct ContextObservation {
    pub observation_id: ObservationId,
    pub kind: ObservationKind,
    pub summary: String,
    pub resource_refs: Vec<ResourceRef>,
    pub claims: Vec<ObservedClaim>,
    pub origin: ContextFactOrigin,
    pub sensitivity: SensitivityHint,
    pub observed_at: Option<Timestamp>,
    pub truncated: bool,
}

pub struct ObservedClaim {
    pub statement: String,
    pub speaker: ObservationSpeaker,
    pub evidence: Vec<EvidenceRef>,
    pub hypothetical: bool,
}
```

Observations are evidence, not current instructions or accepted facts.

## Scope Rules

Before a real adapter is implemented, its retrieval scope must be approved.

Recommended policy:

1. the host fixes the current user identity;
2. the current Conversation is excluded from "past conversation" search;
3. a verified Project/workspace binding restricts search to that scope where the repository can enforce it;
4. without a verified binding, the adapter uses the narrowest same-user scope the current repository can
   prove and records the limitation;
5. archived, deleted, foreign-user, or inaccessible content is excluded;
6. model-generated Conversation IDs are never accepted;
7. evidence never automatically binds Focus or resumes a Runtime.

If the repository cannot enforce the selected scope, P0-R1 remains disabled rather than broadening
search silently.

## Budget and Fuse

P0-R1 hard limits:

```text
model calls                 <= 2
evidence requests           <= 1
observation text            <= 8 KiB
repeated normalized query   denied
```

Empty, denied, unavailable, or irrelevant evidence does not trigger further exploration. The resolver
returns a provisional or insufficient-evidence result.

## Failure Semantics

Expected non-fatal outcomes:

- `Unavailable`;
- `Denied`;
- `NotFound`;
- `Truncated`;
- `BudgetExhausted`;
- `Superseded`.

Contract violations are fatal to the Shadow analysis but never to the original message:

- foreign-scope reference;
- missing provenance;
- unbounded raw result;
- arbitrary path, SQL, credential, authority, or tool name;
- an observation presented as a current user instruction.

## Future Evidence

Future sources require separate owner agreements and evaluations. They are not dormant P0 enum variants.
Adding a source requires:

- a real use case;
- an owner;
- a scoped adapter;
- a normalized observation contract;
- privacy and egress review;
- an independent latency and quality comparison.

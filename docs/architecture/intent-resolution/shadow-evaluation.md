# Intent Shadow Evaluation

This document proposes the first real validation path. It does not approve implementation.

## Experiment Question

P0-R must answer:

1. Does the structured contract improve intent interpretation over the current one-shot baseline?
2. Does historical evidence improve history-dependent requests without causing unnecessary searches?
3. Are the latency, token cost, privacy cost, and maintenance cost worth the gain?

## Supported Surface

Recommended first scope:

```text
Guid / Sessions
+ built-in Nomi conversations
+ explicit feature flag
```

ACP, OpenClaw, Nanobot, Remote, Companion, and IM channels remain excluded until a model-ownership and
integration policy is approved for each.

This recommendation is pending user approval.

## Trigger Placement

The Shadow request must not race the original unaccepted draft.

Correct ordering:

```text
user submits message
  -> existing send path
  -> backend accepts or deduplicates the message
  -> caller receives canonical conversation_id + message_id
  -> caller schedules Shadow analysis without awaiting it
```

Consequences:

- a failed or rejected send is not analyzed as an accepted turn;
- duplicate submissions can share a stable analysis identity;
- a Guid first message is analyzed only after Conversation creation;
- edit/resubmit, steer, queue, and initial-message paths can be distinguished explicitly;
- Shadow never changes the original send result.

P0 may use a dedicated host route, but the UI integration calls it only after acceptance. A later
server-side accepted-message observer requires separate Conversation-owner review.

## Host Adapter

The host integration owns:

- sampling and feature flags;
- authenticated user and Conversation scope;
- resolved model configuration;
- request timeout;
- superseded-result handling;
- local evaluation recording;
- the real `LlmIntentInterpreter`;
- the scoped Conversation evidence adapter in P0-R1.

The existing one-shot completion functions in `nomifun-ai-agent` are the intended model seam. The
Intent core does not import `nomi-providers`, create an Agent Runtime, or persist a model session.

Proposed maximums:

```text
P0-R0 model calls       1
P0-R1 model calls       2
P0-R1 evidence calls    1
```

## P0-R0: Contract Baseline

Candidate:

```text
accepted message + bounded recent context
  -> deterministic prepass
  -> one-shot structured interpretation
  -> invariant validation
  -> IntentResolution
```

No evidence operation is available.

Exit evidence:

- real Nomi model responses;
- valid and invalid structured-output examples;
- latency and token distribution;
- deterministic degradation behavior;
- comparison with Baseline A;
- zero change to message-delivery behavior.

## P0-R1: Historical Evidence

Candidate:

```text
P0-R0 draft
  -> admitted SearchPastConversations request
  -> normalized observation
  -> one refinement pass
  -> invariant validation
```

P0-R1 starts only after P0-R0 establishes a usable baseline.

## Baselines

```text
Baseline A
  current utterance + bounded recent messages
  -> existing/simple one-shot interpretation

Baseline B
  new Intent contract
  -> one-shot interpretation, no evidence

Candidate C
  new Intent contract
  -> optional historical evidence
  -> refinement
```

Comparisons:

- A vs B measures contract/schema value;
- B vs C measures historical-evidence value;
- each comparison uses the same model configuration where possible.

## Superseded Analyses

P0 does not create a per-Conversation cancellation Runtime.

Each analysis is associated with the accepted message identity and observed Conversation sequence.
When a newer accepted user message exists before recording or evaluation:

```text
old result
  -> mark Superseded or discard
  -> never influence a later turn
```

Timeout and natural completion are sufficient for P0. Active cancellation may be reconsidered only if
measurements show material wasted cost.

## Shadow Evaluation Record

Do not call this product telemetry. It is a local development/evaluation record.

Proposed fields:

```text
analysis identity
message identity or irreversible local correlation
schema and resolver version
model identifier
evidence requested / outcome
resolution status
normalized metrics
latency
token usage
error category
superseded flag
optional human label reference
```

Privacy invariants:

- local-only;
- disabled unless an explicit development/evaluation flag is enabled;
- never forwarded to cloud telemetry;
- no credential, secret, attachment body, or raw tool result;
- raw utterance is not persisted by default;
- any optional text capture requires explicit opt-in and redaction;
- bounded retention and a clear delete/export path;
- no database migration or new durable store without separate approval.

Human labels come from an explicit evaluation/export workflow. They are not assumed to appear
automatically in production records.

## Dataset

The labeled set covers:

- ordinary questions;
- discussion without action;
- new work;
- continuation and modification;
- control expressions;
- answers to pending interactions;
- explicit planning requests;
- compound intent;
- quoted or pasted pseudo-instructions;
- assistant suggestion vs user commitment;
- relevant, irrelevant, and absent history;
- output mode;
- current/realtime information cues.

Labeling should preserve adjudication notes. Two independent labels plus adjudication is preferred for
the final gate; a smaller single-label pilot may be used to debug the schema but cannot establish the gate.

## Metrics

Primary:

- False Action Suggestion Rate;
- Provenance Upgrade Violation Rate;
- History Cue Precision / Recall;
- Unnecessary Evidence Request Rate;
- Compound Intent Unit Recall;
- Over-asking Rate;
- P50 / P95 latency;
- token cost per analyzed message.

Secondary:

- structured-output failure rate;
- provisional/degraded rate;
- superseded rate;
- history-result relevance;
- user-correction rate when later measurable.

## Thresholds

The following are hypotheses, not approved gates:

```text
Provenance Upgrade Violation    0 on the adjudicated set
False Action Suggestion         <= 1%
history-dependent improvement  >= 5 percentage points
unnecessary evidence request   <= 10%
send-path blocking             0
```

Before using percentage gates, the evaluation plan must state:

- total and per-category sample sizes;
- model and configuration;
- repeat count for non-deterministic runs;
- confidence intervals;
- treatment of abstentions and degraded outputs.

## Latency Targets

Shadow adds zero awaited latency to the accepted send path.

Initial measurement targets:

```text
overall Shadow timeout          3 seconds
P0-R0 target P95                <= 1.5 seconds
P0-R1 target P95                <= 2.5 seconds
```

These are feasibility targets, not claims about current Providers. If the recommended Nomi model cannot
meet them, the experiment records the result rather than weakening message-delivery semantics.

## Exit Criteria

P0-R is successful only when:

- real accepted messages reach the Shadow path;
- a real model produces validated results;
- P0-R1 returns real scoped historical observations;
- A/B/C comparisons are reproducible;
- privacy rules are verified;
- no original send waits for or depends on Intent;
- no Runtime, Task, Plan, Focus, approval, or execution state is changed;
- results justify a separate decision about user-visible P1.

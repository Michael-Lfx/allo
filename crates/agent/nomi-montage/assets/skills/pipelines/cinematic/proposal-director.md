# Stage Director: proposal (cinematic)

Produce `proposal_packet` — the creative proposal that locks the delivery
promise every later stage (and the `video_compose` tool itself) will be held
to.

## What to decide here

- **Concept**: a specific, filmable idea grounded in `research_brief`, not a
  generic restatement of the topic. State it as `concept_summary` in enough
  detail that a script could be written directly from it.
- **Delivery promise**: default to `motion` for this pipeline unless the
  human's prompt or the creative concept genuinely calls for a deliberate
  still/motion mix (in which case recommend `hybrid`, the sibling pipeline,
  instead — but if the human specifically asked for `cinematic`, honor
  `motion` unless they explicitly say otherwise).
- **Scope estimate**: `estimated_shot_count` and `estimated_duration_secs`
  should be realistic for the stated budget — use `cost_estimate` against
  the `flowy_video`/`flowy_image` tools' typical per-shot cost to sanity
  check before committing to a shot count the budget can't actually cover.

## Human approval

This stage requires human approval by default — the delivery promise and
overall concept are the two decisions most expensive to change later. Write
a `concept_summary` detailed enough that a human can approve or reject it
without needing to read the (not-yet-written) script.

## Red flags to avoid

- A `concept_summary` that could apply to any brand/topic — it should be
  clearly, specifically about *this* project's research findings.
- Setting `delivery_promise: motion` while privately planning a mostly-still
  edit "to save budget" — say that plan out loud as `hybrid` instead, or
  flag the budget constraint to the human rather than quietly under-promising
  in the artifact and over-delivering nothing later.

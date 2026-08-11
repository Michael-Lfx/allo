# Stage Director: scene_plan (screen-demo)

Translate the locked `script` (+ `proposal_packet`) into a concrete
`scene_plan` the assets stage can execute without guessing.

## Priorities
Explicitly mark each shot as reused screen-capture vs newly generated callout/cutaway. Never leave that ambiguous for the assets stage.

## Rules
- Every script scene needs at least one shot.
- Shot descriptions must be prompt-ready (subject, action, framing,
  lighting, continuity notes).
- Declare `is_motion` explicitly on every shot.

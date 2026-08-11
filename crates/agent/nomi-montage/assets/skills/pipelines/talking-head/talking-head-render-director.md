# Stage Director: talking_head_render (talking-head)

Generate the single continuous avatar clip from the locked script and
proposal. Write `asset_manifest` (one entry) and `render_report`.

## Priorities
- Natural lip-sync and delivery across the full duration.
- Persona/tone from proposal_packet must be visible in the take.
- Use cost_estimate before and cost_reconcile after the Flowy video call.

## Rules
- One clip only — do not invent multi-take coverage here.
- If the take fails quality, regenerate; do not degrade to a still.
- Record the file location clearly for the compose stage to finalize.

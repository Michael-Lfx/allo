# Stage Director: edit (avatar-spokesperson)

Produce `edit_decisions` from `asset_manifest` + `scene_plan`.

## Priorities
Order segments for presentation flow. Keep optional b-roll inserts brief so the spokesperson remains the spine of the cut.

## Rules
- Reference only files present in `asset_manifest`.
- Set `render_runtime` to `ffmpeg` unless a later compose runtime is
  explicitly available and chosen in `decision_log`.
- Still holds must be deliberate durations, never silent defaults that
  inflate slideshow risk.

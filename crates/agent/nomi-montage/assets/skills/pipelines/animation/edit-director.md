# Stage Director: edit (animation)

Produce `edit_decisions` from `asset_manifest` + `scene_plan`.

## Priorities
Match the style_playbook's pacing guidance. Preserve character-performance beats rather than cutting mid-gesture.

## Rules
- Reference only files present in `asset_manifest`.
- Set `render_runtime` to `ffmpeg` unless a later compose runtime is
  explicitly available and chosen in `decision_log`.
- Still holds must be deliberate durations, never silent defaults that
  inflate slideshow risk.

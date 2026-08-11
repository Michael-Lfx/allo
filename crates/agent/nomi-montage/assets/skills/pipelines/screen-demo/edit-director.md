# Stage Director: edit (screen-demo)

Produce `edit_decisions` from `asset_manifest` + `scene_plan`.

## Priorities
Trim source segments so the viewer always sees the UI state the narration refers to. Keep callouts synchronized to the moment they explain.

## Rules
- Reference only files present in `asset_manifest`.
- Set `render_runtime` to `ffmpeg` unless a later compose runtime is
  explicitly available and chosen in `decision_log`.
- Still holds must be deliberate durations, never silent defaults that
  inflate slideshow risk.

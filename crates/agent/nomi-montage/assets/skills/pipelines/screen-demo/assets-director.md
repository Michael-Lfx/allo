# Stage Director: assets (screen-demo)

Generate media for every shot in `scene_plan` and write `asset_manifest`.
Prefer `image_selector` / `video_selector` over raw Flowy calls when
visual quality is a primary success criterion.

## Priorities
Generate only the callout/cutaway graphics that scene_plan requested. Match the recording's real UI chrome and colors.

## Rules
- One manifest entry per shot; tag `is_motion` honestly.
- Never silently replace a required motion shot with a still — log and
  escalate instead.
- Use `cost_estimate` / `cost_reconcile` around expensive batches.
- Use `extract_last_frame` when `scene_plan` marks continuity links.

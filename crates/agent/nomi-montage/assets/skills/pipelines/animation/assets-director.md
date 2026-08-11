# Stage Director: assets (animation)

Generate media for every shot in `scene_plan` and write `asset_manifest`.
Prefer `image_selector` / `video_selector` over raw Flowy calls when
visual quality is a primary success criterion.

## Priorities
Judge cross-shot consistency as hard as per-shot beauty. Reject generations that break the locked style_playbook's visual_language.

## Rules
- One manifest entry per shot; tag `is_motion` honestly.
- Never silently replace a required motion shot with a still — log and
  escalate instead.
- Use `cost_estimate` / `cost_reconcile` around expensive batches.
- Use `extract_last_frame` when `scene_plan` marks continuity links.
